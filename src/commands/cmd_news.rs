//! Rust translation of `src/commands/CmdNews.cpp`.
//!
//! `task news`: pages through the release highlights (`NewsItem`s) newer
//! than `news.version`, waiting for enter between sections, then records the
//! current version in `news.version` of the rc file. As in C++, everything is
//! written straight to stdout (the command output buffer stays empty) and
//! every "Press enter to continue.." waits on stdin; with stdin at end of
//! input (non-interactive use) the waits return immediately.
//!
//! `CmdNews::should_nag` is used by `CmdCustom` for the "Recently upgraded
//! to ..." footnote.

use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::commands::cmd_config::CmdConfig;
use crate::commands::cmd_context::CmdContext;
use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::shared::format::format;
use crate::shared::shared::with_console;
use crate::version::Version;

/* Adding a new version:
 *
 * - Add a new `versionX_Y_Z` method to `NewsItem`, and add news items for the new
 *   release.
 * - Call the new method in `NewsItem.all()`. Calls should be in version order.
 * - Test with `task news`.
 */

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdNews` (`news`).
pub struct CmdNews {
    meta: CommandMeta,
}

impl CmdNews {
    /// C++ `CmdNews::CmdNews`.
    pub fn new() -> Self {
        CmdNews {
            meta: CommandMeta {
                keyword: "news".into(),
                usage: "task          news".into(),
                description: "Displays news about the recent releases".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Misc,
                ..CommandMeta::default()
            },
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ static `CmdNews::should_nag`: whether to remind the user to read
    /// `task news` (the `news` verbosity token is on and `news.version` is
    /// older than the current version, with news items newer than it).
    pub fn should_nag(context: &mut Context) -> bool {
        if !context.verbose("news") {
            return false;
        }

        let mut news_version = Version::new(&context.config.get("news.version"));
        if !news_version.is_valid() {
            news_version = Version::new("2.6.0");
        }

        let current_version = Version::current();

        if news_version >= current_version {
            return false;
        }

        // Check if there are actually any interesting news items to show.
        let items = NewsItem::all(&context.config);
        for item in &items {
            if item.version > news_version {
                return true;
            }
        }

        false
    }
}

impl Default for CmdNews {
    fn default() -> Self {
        Self::new()
    }
}

////////////////////////////////////////////////////////////////////////////////
// The message the C++ `signal_handler` prints. C++ builds it inside the
// handler from `Context::getContext ()`; a signal handler cannot allocate, so
// `execute` prepares the bytes beforehand (same colour decision) and the
// handler only writes them. A length of `usize::MAX` marks a `color.footnote`
// that does not parse: the C++ `Color` constructor throws inside the handler,
// which terminates the process, reproduced with `abort`.
static SIGINT_MESSAGE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static SIGINT_MESSAGE_LEN: AtomicUsize = AtomicUsize::new(0);

/// Prepares the C++ `signal_handler` text for the current configuration.
fn prepare_signal_message(config: &Configuration, color: bool) {
    let mut footnote = Color::new();
    let mut valid = true;
    if color && config.has("color.footnote") {
        match Color::parse(&config.get("color.footnote")) {
            Ok(c) => footnote = c,
            Err(_) => valid = false,
        }
    }

    let mut message = String::from("\n\nCome back and read about new features later!\n");
    message += &footnote.colorize(concat!(
        "\nIf you enjoy Taskwarrior, please consider supporting the project at:\n",
        "    https://github.com/sponsors/GothenburgBitFactory/\n"
    ));

    let bytes: &'static mut [u8] = Box::leak(message.into_bytes().into_boxed_slice());
    // Publish the length as "unset" first so a signal in between never pairs
    // the new pointer with a stale length.
    SIGINT_MESSAGE_LEN.store(0, Ordering::SeqCst);
    SIGINT_MESSAGE.store(bytes.as_mut_ptr(), Ordering::SeqCst);
    SIGINT_MESSAGE_LEN.store(
        if valid { bytes.len() } else { usize::MAX },
        Ordering::SeqCst,
    );
}

////////////////////////////////////////////////////////////////////////////////
/// C++ static `signal_handler`.
extern "C" fn signal_handler(s: libc::c_int) {
    if s == libc::SIGINT {
        let len = SIGINT_MESSAGE_LEN.load(Ordering::SeqCst);
        let ptr = SIGINT_MESSAGE.load(Ordering::SeqCst);
        // SAFETY: write(2), abort(3) and exit paths below are
        // async-signal-safe; `ptr`/`len` describe a leaked, never freed
        // buffer published by `prepare_signal_message`.
        unsafe {
            if len == usize::MAX {
                let head = b"\n\nCome back and read about new features later!\n";
                libc::write(libc::STDOUT_FILENO, head.as_ptr().cast(), head.len());
                libc::abort();
            }
            if !ptr.is_null() && len > 0 {
                libc::write(libc::STDOUT_FILENO, ptr.cast(), len);
            }
            libc::_exit(1);
        }
    }
}

/// Installs [`signal_handler`] for SIGINT (C++ `signal (SIGINT, signal_handler)`).
fn install_signal_handler() {
    let handler: extern "C" fn(libc::c_int) = signal_handler;
    // SAFETY: installing a handler that only calls async-signal-safe functions.
    unsafe {
        libc::signal(libc::SIGINT, handler as libc::sighandler_t);
    }
}

/// Writes `text` to the console (C++ `std::cout << text`). Flushed at once,
/// so the output order matches C++ when the signal handler writes or exits.
fn print(text: &str) {
    with_console(|_, stdout| {
        let _ = stdout.write_all(text.as_bytes());
        let _ = stdout.flush();
    });
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `wait_for_enter`.
pub fn wait_for_enter() {
    install_signal_handler();

    with_console(|stdin, stdout| {
        let _ = stdout.write_all(b"\nPress enter to continue..");
        let _ = stdout.flush();
        // std::getline (std::cin, dummy): up to and including '\n', or to end
        // of input.
        let mut dummy = Vec::new();
        let _ = stdin.read_until(b'\n', &mut dummy);
        let _ = stdout.write_all(b"\x1b[2K\x1b[A\x1b[2K"); // Erase current line, move up, and erase again
        let _ = stdout.flush();
    });

    // SAFETY: restoring the default disposition.
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_DFL);
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class NewsItem`: holds information about single improvement / bug.
#[derive(Debug, Clone)]
pub struct NewsItem {
    pub version: Version,
    pub title: String,
    pub bg_title: String,
    pub background: String,
    pub punchline: String,
    pub update: String,
    pub reasoning: String,
    pub actions: String,
}

impl NewsItem {
    /// C++ private `NewsItem::NewsItem`; the C++ default arguments (`""`)
    /// are passed explicitly.
    #[allow(clippy::too_many_arguments)]
    fn new(
        version: Version,
        title: &str,
        bg_title: &str,
        background: &str,
        punchline: &str,
        update: &str,
        reasoning: &str,
        actions: &str,
    ) -> Self {
        NewsItem {
            version,
            title: title.to_string(),
            bg_title: bg_title.to_string(),
            background: background.to_string(),
            punchline: punchline.to_string(),
            update: update.to_string(),
            reasoning: reasoning.to_string(),
            actions: actions.to_string(),
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `NewsItem::render`. Takes the configuration and the
    /// `Context::color ()` decision instead of the global Context.
    ///
    /// # Errors
    /// An unparsable `color.header` / `color.footnote` (C++ `Color`
    /// constructor throws).
    pub fn render(&mut self, config: &Configuration, color: bool) -> Result<()> {
        let mut header = Color::new();
        let mut footnote = Color::new();
        let mut bold = Color::new();
        let mut underline = Color::new();
        if color {
            bold = Color::parse("bold")?;
            underline = Color::parse("underline")?;
            if config.has("color.header") {
                header = Color::parse(&config.get("color.header"))?;
            }
            if config.has("color.footnote") {
                footnote = Color::parse(&config.get("color.footnote"))?;
            }
        }

        // TODO: For some reason, bold cannot be blended in 256-color terminals
        // Apply this workaround of colorizing twice.
        let version = self.version.to_string();
        print(&bold.colorize(&header.colorize(&format(
            "{1} ({2})\n",
            &[&self.title, &version],
        ))));
        if !self.background.is_empty() {
            if self.bg_title.is_empty() {
                self.bg_title = "Background".to_string();
            }

            print(&format!(
                "\n  {}\n{}\n",
                underline.colorize(&self.bg_title),
                self.background
            ));
        }

        wait_for_enter();

        print(&format!(
            "  {}",
            underline.colorize(&format("What changed in {1}?\n", &[&version]))
        ));
        if !self.punchline.is_empty() {
            print(&footnote.colorize(&format("{1}\n", &[&self.punchline])));
        }

        if !self.update.is_empty() {
            print(&format("{1}\n", &[&self.update]));
        }

        wait_for_enter();

        if !self.reasoning.is_empty() {
            print(&format!(
                "  {}{}\n",
                underline.colorize("What was the motivation behind this feature?\n"),
                self.reasoning
            ));
            wait_for_enter();
        }

        if !self.actions.is_empty() {
            print(&format!(
                "  {}{}\n",
                underline.colorize("What do I have to do?\n"),
                self.actions
            ));
            wait_for_enter();
        }

        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ static `NewsItem::all`. Takes the configuration that
    /// `version2_6_0` reads from the global Context.
    pub fn all(config: &Configuration) -> Vec<NewsItem> {
        let mut items = Vec::new();
        Self::version2_6_0(config, &mut items);
        Self::version3_0_0(&mut items);
        Self::version3_1_0(&mut items);
        Self::version3_2_0(&mut items);
        Self::version3_3_0(&mut items);
        Self::version3_4_0(&mut items);
        Self::version3_5_0(&mut items);
        items
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `NewsItem::version2_6_0`: generate the highlights for the 2.6.0
    /// version.
    ///
    /// - XDG directory mode (high)
    /// - Support for Unicode 11 characters (high)
    /// - 64 bit values, UDAs, Datetime values until year 9999 (high)
    /// - Config context variables
    /// - Reports outside of context
    /// - Environment variables in taskrc (high)
    /// - Waiting is a virtual concept (high)
    /// - Improved parser and task display mechanism
    /// - The .by attribute modifier
    /// - Exporting a report
    /// - Multi-day holidays
    pub fn version2_6_0(config: &Configuration, items: &mut Vec<NewsItem>) {
        let version = Version::new("2.6.0");
        /////////////////////////////////////////////////////////////////////////
        // - Writeable context

        // Detect whether user uses any contexts
        let mut advice = String::new();

        let defined = CmdContext::get_contexts(config);
        if !defined.is_empty() {
            // Detect the old-style contexts
            let old_style: Vec<&String> = defined
                .iter()
                .filter(|name| config.has(&format!("context.{name}")))
                .collect();

            if !old_style.is_empty() {
                let count = defined
                    .iter()
                    .filter(|name| config.has(&format!("context.{name}")))
                    .count();
                advice += &format(
                    "  You have {1} defined contexts, out of which {2} are old-style:\n",
                    &[&defined.len(), &count],
                );

                for context in &defined {
                    let old_definition = config.get(&format!("context.{context}"));
                    if !old_definition.is_empty() {
                        advice += &format("  * {1}: {2}\n", &[context, &old_definition]);
                    }
                }

                advice += concat!(
                    "\n",
                    "  These need to be migrated to new-style, which uses context.<name>.read and\n",
                    "  context.<name>.write config variables. Please run the following commands:\n"
                );

                for context in &defined {
                    let old_definition = config.get(&format!("context.{context}"));
                    if !old_definition.is_empty() {
                        advice += &format(
                            "  $ task context define {1} '{2}'\n",
                            &[context, &old_definition],
                        );
                    }
                }

                advice += concat!(
                    "\n",
                    "  Please check these filters are also valid modifications. If a context filter is ",
                    "not\n",
                    "  a valid modification, you can set the context.<name>.write configuration variable ",
                    "to\n",
                    "  specify the write context explicitly. Read more in CONTEXT section of man taskrc."
                );
            } else {
                advice += "  You don't have any old-style contexts defined, so you're good to go as is!";
            }
        } else {
            advice += concat!(
                "  You don't have any contexts defined, so you're good to go as is!\n",
                "  Read more about how to use contexts in CONTEXT section of 'man task'."
            );
        }

        let writeable_context = NewsItem::new(
            version,
            "'Writeable' context",
            "Background - what is context?",
            concat!(
                "  The 'context' is a feature (introduced in 2.5.0) that allows users to apply a\n",
                "  predefined filter to all task reports.\n",
                "  \n",
                "    $ task context define work \"project:Work or +urgent\"\n",
                "    $ task context work\n",
                "    Context 'work' set. Use 'task context none' to remove.\n",
                "  \n",
                "  Now if we proceed to add two tasks:\n",
                "    $ task add Talk to Jeff pro:Work\n",
                "    $ task add Call mom pro:Personal\n",
                "  \n",
                "    $ task\n",
                "    ID Age   Project Description  Urg\n",
                "     1 16s   Work    Talk to Jeff    1\n",
                "  \n",
                "  The task \"Call mom\" will not be listed, because it does not match\n",
                "  the active context (its project is 'Personal' and not 'Work')."
            ),
            concat!(
                "  The currently active context definition is now applied as default modifications\n",
                "  when creating new tasks using 'task add' and 'task log'."
            ),
            concat!(
                "  \n",
                "  Consider following example, using context 'work' defined as 'project:Work' above:\n",
                "  \n",
                "    $ task context work\n",
                "    $ task add Talk to Jeff\n",
                "    $ task\n",
                "    ID Age  Project Description  Urg \n",
                "     1 1s   Work    Talk to Jeff    1\n",
                "            ^^^^^^^\n",
                "  \n",
                "  Note that project attribute was set to 'Work' automatically."
            ),
            concat!(
                "  This was a popular feature request. Now, if you have a context active,\n",
                "  newly added tasks no longer \"fall outside\" of the context by default."
            ),
            &advice,
        );
        items.push(writeable_context);

        /////////////////////////////////////////////////////////////////////////
        // - 64-bit datetime support

        let uint64_support = NewsItem::new(
            version,
            "Support for 64-bit timestamps and numeric values",
            "",
            "",
            concat!(
                "  Taskwarrior now supports 64-bit timestamps, making it possible to set due dates\n",
                "  and other date attributes beyond 19 January 2038 (limit of 32-bit timestamps).\n"
            ),
            "  The current limit is 31 December 9999 for display reasons (last 4-digit year).",
            concat!(
                "  With each year passing by faster than the last, setting tasks for 2040s\n",
                "  is not as unfeasible as it once was."
            ),
            "  Don't forget that 50-year anniversary and 'task add' a long-term task today!",
        );
        items.push(uint64_support);

        /////////////////////////////////////////////////////////////////////////
        // - Waiting is a virtual status

        let waiting_status = NewsItem::new(
            version,
            "Deprecation of the status:waiting",
            "",
            concat!(
                "  If a task has a 'wait' attribute set to a date in the future, it is modified\n",
                "  to have a 'waiting' status. Once that date is no longer in the future, the status\n",
                "  is modified to back to 'pending'."
            ),
            concat!(
                "  The 'waiting' value of status is deprecated, instead users should use +WAITING\n",
                "  virtual tag, or explicitly query for wait.after:now (the two are equivalent)."
            ),
            concat!(
                "  \n",
                "  The status:waiting query still works in 2.6.0, but support will be dropped in 3.0."
            ),
            "",
            concat!(
                "  In your custom report definitions, the following expressions should be replaced:\n",
                "  * 'status:pending or status:waiting' should be replaced by 'status:pending'\n",
                "  * 'status:pending' should be replaced by 'status:pending -WAITING'"
            ),
        );
        items.push(waiting_status);

        /////////////////////////////////////////////////////////////////////////
        // - Support for environment variables in the taskrc

        let env_vars = NewsItem::new(
            version,
            "Environment variables in the taskrc",
            "",
            "",
            concat!(
                "  Taskwarrior now supports expanding environment variables in the taskrc file,\n",
                "  allowing users to customize the behaviour of 'task' based on the current env.\n"
            ),
            concat!(
                "  The environment variables can either be used in paths, or as separate values:\n",
                "    data.location=$XDG_DATA_HOME/task/\n",
                "    default.project=$PROJECT"
            ),
            "",
            "",
        );
        items.push(env_vars);

        /////////////////////////////////////////////////////////////////////////
        // - Reports outside of context

        let contextless_reports = NewsItem::new(
            version,
            "Context-less reports",
            "",
            "  By default, every report is affected by currently active context.",
            concat!(
                "  You can now make a selected report ignore currently active context by setting\n",
                "  'report.<name>.context' configuration variable to 0."
            ),
            "",
            concat!(
                "  This is useful for users who utilize a single place (such as project:Inbox)\n",
                "  to collect their new tasks that are then triaged on a regular basis\n",
                "  (such as in GTD methodology).\n",
                "  \n",
                "  In such a case, defining a report that filters for project:Inbox and making it\n",
                "  fully accessible from any context is a major usability improvement."
            ),
            "",
        );
        items.push(contextless_reports);

        /////////////////////////////////////////////////////////////////////////
        // - Exporting a particular report

        let exportable_reports = NewsItem::new(
            version,
            "Exporting a particular report",
            "",
            "",
            concat!(
                "  You can now export the tasks listed by a particular report as JSON by simply\n",
                "  calling 'task export <report>'.\n"
            ),
            "  The export mirrors the filter and the sort order of the report.",
            concat!(
                "  This feature can be used to quickly process the data displayed in a particular\n",
                "  report using other CLI tools. For example, the following oneliner\n",
                "  \n",
                "      $ task export next | jq '.[].urgency' | datamash mean 1\n",
                "      3.3455535142857\n",
                "  \n",
                "  combines jq and GNU datamash to compute average urgency of the tasks displayed\n",
                "  in the 'next' report."
            ),
            "",
        );
        items.push(exportable_reports);

        /////////////////////////////////////////////////////////////////////////
        // - Multi-day holidays

        let multi_holidays = NewsItem::new(
            version,
            "Multi-day holidays",
            "",
            concat!(
                "  Holidays are currently used in 'task calendar' to visualize the workload during\n",
                "  the upcoming weeks/months. Up to date country-specific holiday data files can be\n",
                "  obtained from our website, holidata.net."
            ),
            concat!(
                "  Instead of single-day holiday entries only, Taskwarrior now supports holidays\n",
                "  that span a range of days (i.e. vacation).\n"
            ),
            concat!(
                "  Use a holiday.<name>.start and holiday.<name>.end to configure a multi-day holiday:\n",
                "  \n",
                "      holiday.sysadmin.name=System Administrator Appreciation Week\n",
                "      holiday.sysadmin.start=20100730\n",
                "      holiday.sysadmin.end=20100805"
            ),
            "",
            "",
        );
        items.push(multi_holidays);

        /////////////////////////////////////////////////////////////////////////
        // - Unicode 12

        let unicode_12 = NewsItem::new(
            version,
            "Extended Unicode support (Unicode 12)",
            "",
            "",
            concat!(
                "  The support for Unicode character set was improved to support Unicode 12.\n",
                "  This means better support for various language-specific characters - and emojis!"
            ),
            "",
            concat!(
                "  Extended unicode support for language-specific characters helps non-English speakers.\n",
                "  While most users don't enter emojis as task metadata, automated task creation tools,\n",
                "  such as bugwarrior, might create tasks with exotic Unicode data."
            ),
            "  You can try it out - 'task add Prepare for an 👽 invasion!'",
        );
        items.push(unicode_12);

        /////////////////////////////////////////////////////////////////////////
        // - The .by attribute modifier

        let by_modifier = NewsItem::new(
            version,
            "The .by attribute modifier",
            "",
            "",
            "  A new attribute modifier '.by' was introduced, equivalent to the operator '<='.\n",
            concat!(
                "  This modifier can be used to list all tasks due by the end of the months,\n",
                "  including the last day of the month, using: 'due.by:eom' query"
            ),
            concat!(
                "  There was no convenient way to express '<=' relation using attribute modifiers.\n",
                "  As a workaround, instead of 'due.by:eom' one could use 'due.before:eom+1d',\n",
                "  but that requires a certain amount of mental overhead."
            ),
            "",
        );
        items.push(by_modifier);

        /////////////////////////////////////////////////////////////////////////
        // - Context-specific configuration overrides

        let context_config = NewsItem::new(
            version,
            "Context-specific configuration overrides",
            "",
            "",
            concat!(
                "  Any context can now define context-specific configuration overrides\n",
                "  via context.<name>.rc.<setting>=<value>.\n"
            ),
            concat!(
                "  This allows the user to customize the behaviour of Taskwarrior in a given context,\n",
                "  for example, to change the default command in the 'work' context to 'overdue':\n",
                "\n",
                "      $ task config context.work.rc.default.command overdue\n",
                "\n",
                "  Another example would be to ensure that while context 'work' is active, tasks get\n",
                "  stored in a ~/.worktasks directory:\n",
                "\n",
                "      $ task config context.work.rc.data.location=~/.worktasks"
            ),
            "",
            "",
        );
        items.push(context_config);

        /////////////////////////////////////////////////////////////////////////
        // - XDG config home support

        let xdg_support = NewsItem::new(
            version,
            "Support for XDG Base Directory Specification",
            "",
            concat!(
                "  The XDG Base Directory specification provides standard locations to store\n",
                "  application data, configuration, state, and cached data in order to keep $HOME\n",
                "  clutter-free. The locations are usually set to ~/.local/share, ~/.config,\n",
                "  ~/.local/state and ~/.cache respectively."
            ),
            concat!(
                "  If taskrc is not found at '~/.taskrc', Taskwarrior will attempt to find it\n",
                "  at '$XDG_CONFIG_HOME/task/taskrc' (defaults to '~/.config/task/taskrc')."
            ),
            "",
            concat!(
                "  This allows users to fully follow XDG Base Directory Spec by moving their taskrc:\n",
                "      $ mkdir $XDG_CONFIG_HOME/task\n",
                "      $ mv ~/.taskrc $XDG_CONFIG_HOME/task/taskrc\n\n",
                "  and further setting:\n",
                "      data.location=$XDG_DATA_HOME/task/\n",
                "      hooks.location=$XDG_CONFIG_HOME/task/hooks/\n\n",
                "  Solutions in the past required symlinks or more cumbersome configuration overrides."
            ),
            concat!(
                "  If you configure your data.location and hooks.location as above, ensure\n",
                "  that the XDG_DATA_HOME and XDG_CONFIG_HOME environment variables are set,\n",
                "  otherwise they're going to expand to empty string. Alternatively you can\n",
                "  hardcode the desired paths on your system."
            ),
        );
        items.push(xdg_support);

        /////////////////////////////////////////////////////////////////////////
        // - Update holiday data

        let holidata_2022 = NewsItem::new(
            version,
            "Updated holiday data for 2022",
            "",
            "",
            concat!(
                "  Holiday data has been refreshed for 2022 and five more holiday locales\n",
                "  have been added: fr-CA, hu-HU, pt-BR, sk-SK and sv-FI."
            ),
            "",
            concat!(
                "  Refreshing the holiday data is part of every release. The addition of the new\n",
                "  locales allows us to better support users in those particular countries."
            ),
            "",
        );
        items.push(holidata_2022);
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `NewsItem::version3_0_0`.
    pub fn version3_0_0(items: &mut Vec<NewsItem>) {
        let version = Version::new("3.0.0");
        let sync = NewsItem::new(
            version,
            /*title=*/ "New data model and sync backend",
            /*bg_title=*/ "",
            /*background=*/ "",
            /*punchline=*/
            concat!(
                "The sync functionality for Taskwarrior has been rewritten entirely, and no longer\n",
                "supports taskserver/taskd. The most robust solution is a cloud-storage backend,\n",
                "although a less-mature taskchampion-sync-server is also available. See `task-sync(5)`\n",
                "For details. As part of this change, the on-disk storage format has also changed.\n"
            ),
            /*update=*/
            concat!(
                "This is a breaking upgrade: you must export your task database from 2.x and re-import\n",
                "it into 3.x. Hooks run during task import, so if you have any hooks defined,\n",
                "temporarily disable them for this operation.\n\n",
                "See https://taskwarrior.org/docs/upgrade-3/ for information on upgrading to Taskwarrior ",
                "3.0."
            ),
            "",
            "",
        );
        items.push(sync);
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `NewsItem::version3_1_0`.
    pub fn version3_1_0(items: &mut Vec<NewsItem>) {
        let version = Version::new("3.1.0");
        let purge = NewsItem::new(
            version,
            /*title=*/ "Purging Tasks, Manually or Automatically",
            /*bg_title=*/ "",
            /*background=*/ "",
            /*punchline=*/
            concat!(
                "Support for `task purge` has been restored, and new support added for automatically\n",
                "expiring old tasks.\n\n"
            ),
            /*update=*/
            concat!(
                "The `task purge` command removes tasks entirely, in contrast to `task delete` which merely\n",
                "sets the task status to 'Deleted'. This functionality existed in versions 2.x but was\n",
                "temporarily removed in 3.0.\n\n",
                "The new `purge.on-sync` configuration parameter controls automatic purging of old tasks.\n",
                "An old task is one with status 'Deleted' that has not been modified in 180 days. This\n",
                "functionality is optional and not enabled by default."
            ),
            "",
            "",
        );
        items.push(purge);
        let news = NewsItem::new(
            version,
            /*title=*/ "Improved 'task news'",
            /*bg_title=*/ "",
            /*background=*/ "",
            /*punchline=*/
            "The news you are reading now is improved.\n\n",
            /*update=*/
            concat!(
                "The `task news` command now always shows all new information, not just 'major' news,\n",
                "and will only show that news once. New installs will assume all news has been read.\n",
                "Finally, news can be completely hidden by removing 'news' from the 'verbose' config."
            ),
            "",
            "",
        );
        items.push(news);
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `NewsItem::version3_2_0`.
    pub fn version3_2_0(items: &mut Vec<NewsItem>) {
        let version = Version::new("3.2.0");
        let info = NewsItem::new(
            version,
            /*title=*/ "`task info` Journal Restored",
            /*bg_title=*/ "",
            /*background=*/ "",
            /*punchline=*/ "",
            /*update=*/
            concat!(
                "Support for the \"journal\" output in `task info` has been restored. The command now\n",
                "displays a list of changes made to the task, with timestamps.\n\n"
            ),
            "",
            "",
        );
        items.push(info);
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `NewsItem::version3_3_0`.
    pub fn version3_3_0(items: &mut Vec<NewsItem>) {
        let version = Version::new("3.3.0");
        let info = NewsItem::new(
            version,
            /*title=*/ "AWS S3 Sync",
            /*bg_title=*/ "",
            /*background=*/ "",
            /*punchline=*/ "Use an AWS S3 bucket to sync Taskwarrior",
            /*update=*/
            concat!(
                "Taskwarrior now supports AWS as a backend for sync, in addition to existing support\n",
                "for GCP and taskchampion-sync-server. See `man task-sync` for details.\n\n"
            ),
            "",
            "",
        );
        items.push(info);
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `NewsItem::version3_4_0`.
    pub fn version3_4_0(items: &mut Vec<NewsItem>) {
        let version = Version::new("3.4.0");
        let info = NewsItem::new(
            version,
            /*title=*/ "Read-Only Access",
            /*bg_title=*/ "",
            /*background=*/ "",
            /*punchline=*/ "Some Taskwarrior commands operate faster in read-only mode",
            /*update=*/
            concat!(
                "Some commands do not need to write to the DB, so can open it in read-only\n",
                "mode and thus more quickly. This does not include reports (task lists),\n",
                "unless the `gc` config is false. Use `rc.gc=0` in command-lines to allow\n",
                "read-only access.\n\n"
            ),
            "",
            "",
        );
        items.push(info);
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `NewsItem::version3_5_0`.
    pub fn version3_5_0(items: &mut Vec<NewsItem>) {
        let version = Version::new("3.5.0");
        let info = NewsItem::new(
            version,
            /*title=*/ "Scheduled attribute of recurring tasks",
            /*bg_title=*/ "",
            /*background=*/ "",
            /*punchline=*/ "The `scheduled` attribute is now updated for new instances of recurring tasks",
            /*update=*/
            concat!(
                "Previously, the value of `scheduled` for a recurring task was copied from the\n",
                "parent task without modification. It is now offset from the recurrence time\n",
                "just like `wait`. While this fixes a bug, it may interact poorly with any hooks\n",
                "that assume the old behavior.\n\n"
            ),
            "",
            "",
        );
        items.push(info);
    }
}

impl Command for CmdNews {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdNews::execute`: prints the highlights of every release newer
    /// than `news.version` (default 2.6.0) to stdout, waiting for enter
    /// between sections, and then stores the current version in
    /// `news.version` (without confirmation) if it was older.
    ///
    /// As in C++, `Press enter to continue..` is shown (and stdin read) even
    /// when there is nothing to show, and the `You are up to date!` /
    /// release-highlights banner is bold regardless of `color`.
    ///
    /// # Errors
    /// An unparsable `color.header` / `color.footnote`, and the
    /// `Could not write to '<file>'.` of `CmdConfig::setConfigVariable`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // The C++ copies of `cli2.getWords ()` and `config` are unused.

        // Supress compiler warning about unused argument
        output.clear();

        let mut items = NewsItem::all(&context.config);
        let mut news_version = Version::new(&context.config.get("news.version"));
        let current_version = Version::current();

        // 2.6.0 is the earliest version with news support.
        if !news_version.is_valid() {
            news_version = Version::new("2.6.0");
        }

        let color = context.color();
        prepare_signal_message(&context.config, color);
        install_signal_handler();

        // Remove items that have already been shown
        items.retain(|n| n.version > news_version);

        let bold = Color::parse("bold")?;
        if items.is_empty() {
            print(&bold.colorize("You are up to date!\n"));
        } else {
            // Print release notes
            print(&bold.colorize(&format(
                concat!(
                    "\n",
                    "================================================\n",
                    "Taskwarrior {1} through {2} Release Highlights\n",
                    "================================================\n"
                ),
                &[&news_version.to_string(), &current_version.to_string()],
            )));

            let total = items.len();
            for (i, item) in items.iter_mut().enumerate() {
                print(&format("\n({1}/{2}) ", &[&(i + 1), &total]));
                item.render(&context.config, color)?;
            }
            print("Thank you for catching up on the new features!\n");
        }
        wait_for_enter();

        // Set a mark in the config to remember which version's release notes were displayed
        if news_version < current_version {
            CmdConfig::set_config_variable(
                &context.config,
                "news.version",
                &current_version.to_string(),
                false,
            )?;
        }

        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::shared::{begin_console_override, end_console_override};

    const PRESS_ENTER: &str = "\nPress enter to continue..\x1b[2K\x1b[A\x1b[2K";

    fn context(verbose: &str, news_version: &str) -> Context {
        let mut context = Context::new();
        context.config.set("verbose", verbose);
        context.config.set("news.version", news_version);
        context
    }

    #[test]
    fn should_nag_follows_news_version_and_verbosity() {
        // Older than the current release, with newer news items.
        assert!(CmdNews::should_nag(&mut context("news", "2.6.0")));
        assert!(CmdNews::should_nag(&mut context("news", "3.4.9")));
        // An invalid or empty version counts as 2.6.0.
        assert!(CmdNews::should_nag(&mut context("news", "")));
        assert!(CmdNews::should_nag(&mut context("news", "garbage")));
        // Up to date (or newer).
        assert!(!CmdNews::should_nag(&mut context("news", "3.5.0")));
        assert!(!CmdNews::should_nag(&mut context("news", "4.0.0")));
        // Without the 'news' verbosity token.
        assert!(!CmdNews::should_nag(&mut context("nothing", "2.6.0")));
        assert!(!CmdNews::should_nag(&mut context("affected", "2.6.0")));
    }

    #[test]
    fn all_items_in_version_order() {
        let items = NewsItem::all(&Configuration::new());
        let versions: Vec<String> = items.iter().map(|i| i.version.to_string()).collect();
        assert_eq!(
            versions,
            [
                "2.6.0", "2.6.0", "2.6.0", "2.6.0", "2.6.0", "2.6.0", "2.6.0", "2.6.0", "2.6.0",
                "2.6.0", "2.6.0", "2.6.0", "3.0.0", "3.1.0", "3.1.0", "3.2.0", "3.3.0", "3.4.0", "3.5.0"
            ]
        );
        assert_eq!(items[0].title, "'Writeable' context");
        assert_eq!(items[18].title, "Scheduled attribute of recurring tasks");
    }

    #[test]
    fn version2_6_0_context_advice() {
        // No contexts.
        let items = NewsItem::all(&Configuration::new());
        assert_eq!(
            items[0].actions,
            "  You don't have any contexts defined, so you're good to go as is!\n  Read more about how to use contexts in CONTEXT section of 'man task'."
        );

        // Only new-style contexts.
        let mut config = Configuration::new();
        config.set("context.work.read", "project:Work");
        let items = NewsItem::all(&config);
        assert_eq!(
            items[0].actions,
            "  You don't have any old-style contexts defined, so you're good to go as is!"
        );

        // Old-style contexts.
        config.set("context.home", "project:Home");
        let items = NewsItem::all(&config);
        assert_eq!(
            items[0].actions,
            concat!(
                "  You have 2 defined contexts, out of which 1 are old-style:\n",
                "  * home: project:Home\n",
                "\n",
                "  These need to be migrated to new-style, which uses context.<name>.read and\n",
                "  context.<name>.write config variables. Please run the following commands:\n",
                "  $ task context define home 'project:Home'\n",
                "\n",
                "  Please check these filters are also valid modifications. If a context filter is not\n",
                "  a valid modification, you can set the context.<name>.write configuration variable to\n",
                "  specify the write context explicitly. Read more in CONTEXT section of man taskrc."
            )
        );
    }

    const HIGHLIGHT_3_5_0: &str = concat!(
        "Scheduled attribute of recurring tasks (3.5.0)\n",
        "\nPress enter to continue..\x1b[2K\x1b[A\x1b[2K",
        "  What changed in 3.5.0?\n",
        "The `scheduled` attribute is now updated for new instances of recurring tasks\n",
        "Previously, the value of `scheduled` for a recurring task was copied from the\n",
        "parent task without modification. It is now offset from the recurrence time\n",
        "just like `wait`. While this fixes a bug, it may interact poorly with any hooks\n",
        "that assume the old behavior.\n\n\n",
        "\nPress enter to continue..\x1b[2K\x1b[A\x1b[2K",
    );

    #[test]
    fn execute_shows_unread_news_and_records_version() {
        let mut f = crate::test_util::CommandFixture::new(&[], "news.version=3.4.0\n", &["news"]);
        begin_console_override("\n\n\n");
        let rc = f.run(&CmdNews::new()).unwrap();
        let out = end_console_override();
        assert_eq!(rc, 0);
        assert_eq!(
            out,
            format!(
                concat!(
                    "\x1b[1m\n",
                    "================================================\n",
                    "Taskwarrior 3.4.0 through 3.5.0 Release Highlights\n",
                    "================================================\n",
                    "\x1b[0m",
                    "\n(1/1) {}",
                    "Thank you for catching up on the new features!\n",
                    "{}"
                ),
                HIGHLIGHT_3_5_0, PRESS_ENTER
            )
        );
        let rc_text = std::fs::read_to_string(f.ctx.config.file()).unwrap();
        assert!(rc_text.ends_with("\nnews.version=3.5.0\n"), "{rc_text}");
        assert!(!rc_text.contains("3.4.0"), "{rc_text}");
    }

    #[test]
    fn execute_up_to_date_waits_and_keeps_version() {
        let mut f = crate::test_util::CommandFixture::new(&[], "news.version=3.5.0\n", &["news"]);
        let before = std::fs::read_to_string(f.ctx.config.file()).unwrap();
        // End of input (non-interactive): the wait returns at once.
        begin_console_override("");
        let rc = f.run(&CmdNews::new()).unwrap();
        let out = end_console_override();
        assert_eq!(rc, 0);
        assert_eq!(out, format!("\x1b[1mYou are up to date!\n\x1b[0m{PRESS_ENTER}"));
        assert_eq!(std::fs::read_to_string(f.ctx.config.file()).unwrap(), before);
    }

    #[test]
    fn execute_invalid_version_shows_everything_after_2_6_0() {
        let mut f = crate::test_util::CommandFixture::new(&[], "news.version=bogus\n", &["news"]);
        begin_console_override("");
        assert_eq!(f.run(&CmdNews::new()).unwrap(), 0);
        let out = end_console_override();
        assert!(out.contains("Taskwarrior 2.6.0 through 3.5.0 Release Highlights"));
        assert!(out.contains("\n(1/7) New data model and sync backend (3.0.0)\n"));
        assert!(out.contains("\n(7/7) Scheduled attribute of recurring tasks (3.5.0)\n"));
        let rc_text = std::fs::read_to_string(f.ctx.config.file()).unwrap();
        assert!(rc_text.contains("news.version=3.5.0\n"), "{rc_text}");
    }

    #[test]
    fn render_without_color() {
        let mut item = NewsItem::new(
            Version::new("9.9.9"),
            "Title",
            "",
            "back",
            "punch",
            "upd",
            "why",
            "todo",
        );
        begin_console_override("\n\n\n\n");
        item.render(&Configuration::new(), false).unwrap();
        let out = end_console_override();
        assert_eq!(item.bg_title, "Background");
        assert_eq!(
            out,
            format!(
                "Title (9.9.9)\n\n  Background\nback\n{PRESS_ENTER}  What changed in 9.9.9?\npunch\nupd\n{PRESS_ENTER}  What was the motivation behind this feature?\nwhy\n{PRESS_ENTER}  What do I have to do?\ntodo\n{PRESS_ENTER}"
            )
        );
    }

    #[test]
    fn render_with_color() {
        let mut item = NewsItem::new(Version::new("1.2.3"), "T", "", "", "p", "", "", "");
        let mut config = Configuration::new();
        config.set("color.header", "green");
        config.set("color.footnote", "red");
        begin_console_override("");
        item.render(&config, true).unwrap();
        let out = end_console_override();
        assert_eq!(
            out,
            format!(
                "\x1b[1m\x1b[32mT (1.2.3)\n\x1b[0m\x1b[0m{PRESS_ENTER}  \x1b[4mWhat changed in 1.2.3?\n\x1b[0m\x1b[31mp\n\x1b[0m{PRESS_ENTER}"
            )
        );
    }
}
