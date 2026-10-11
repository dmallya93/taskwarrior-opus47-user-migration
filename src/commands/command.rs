//! Rust translation of `src/commands/Command.h` and `src/commands/Command.cpp`.
//!
//! # Shape (milestone 1 Design Decision 1, foundation design notes DD1)
//!
//! - C++ abstract class `Command` becomes the object-safe trait [`Command`].
//!   Its protected data members (`_keyword`, `_usage`, …, the permission
//!   state) live in [`CommandMeta`], which every implementor holds and
//!   exposes through [`Command::meta`]; all the C++ non-virtual accessors are
//!   provided trait methods reading it. The only C++ virtuals,
//!   `uses_context` and `execute`, are the methods meant to be overridden.
//! - `Context::commands` (C++ `std::map<std::string, Command*>`) is a
//!   [`Registry`]: a `BTreeMap` (same order as `std::map`) of
//!   `Rc<dyn Command>`. Dispatch clones the `Rc` before calling
//!   [`Command::execute`] with `&mut Context`, so there is no borrow
//!   conflict.
//! - [`factory`] is C++ `Command::factory`.
//!
//! # How command modules plug in (task #8 and later milestones)
//!
//! Each `src/commands/cmd_<name>.rs` defines one struct per C++ class, e.g.
//!
//! ```ignore
//! pub struct CmdVersion { meta: CommandMeta }
//!
//! impl CmdVersion {
//!     pub fn new() -> Self {
//!         CmdVersion { meta: CommandMeta {
//!             keyword: "version".into(), usage: "task version".into(),
//!             description: "Shows the Taskwarrior version number".into(),
//!             read_only: true, displays_id: false, needs_gc: false,
//!             category: Category::Misc, ..CommandMeta::default() } }
//!     }
//! }
//!
//! impl Command for CmdVersion {
//!     fn meta(&self) -> &CommandMeta { &self.meta }
//!     fn execute(&self, ctx: &mut Context, output: &mut String) -> Result<i32> { … }
//! }
//! ```
//!
//! and [`factory`] registers it with `register(all, Rc::new(CmdVersion::new()))`.
//!
//! # Registration (task #8)
//!
//! [`factory`] registers one struct per C++ command class (75 built-ins, in
//! the C++ order) plus one [`CmdCustom`] per `report.<name>.columns`. Every
//! struct carries the exact C++ constructor metadata; commands whose port has
//! not landed return [`TwError::NotImplemented`] from `execute` (foundation
//! DD4), and each stub's doc comment names the milestone that ports it.
//!
//! # `uses_context` overrides
//!
//! `CmdCustom` and `CmdTimesheet` override the C++ virtual `uses_context`
//! with a `report.<name>.context` lookup, which is why
//! [`Command::uses_context`] takes the configuration. `CLI2::categorizeArgs`
//! (which in C++ `dynamic_cast`s to those two classes) simply calls the trait
//! method.
//!
//! # What else of `Command.cpp` / `Command.h` exists, and where
//!
//! `Command.cpp` holds only the constructor defaults ([`CommandMeta`]'s
//! `Default`), the accessors, [`factory`], `categoryNames` and
//! [`Command::permission`]. There is no `Command::filter`, `projectSync` or
//! `Command::confirm` in Taskwarrior 3.5.0: commands build a `Filter` and
//! call `Filter::subset` themselves ([`crate::filter::Filter`]); `confirm`
//! / `confirm4` are the free functions [`crate::shared::shared::confirm`] and
//! [`crate::util::confirm4`]; project-change feedback is
//! [`crate::feedback::on_project_change`]. The metadata checks are performed
//! where C++ performs them:
//!
//! - `accepts_filter` / `accepts_modifications` / `accepts_miscellaneous`:
//!   `CLI2::categorizeArgs` (`The '<cmd>' command does not allow '<arg>'.`,
//!   and "Unknown error. Please report." for the inconsistent
//!   modifications+miscellaneous combinations), and `Context::dispatch`
//!   (`prepareFilter` for commands accepting modifications but no filter).
//! - `uses_context`: `CLI2::categorizeArgs` → `addContext`; also tagged
//!   `USESCONTEXT` by `CLI2::findCommand` with the other DNA tags.
//! - `needs_gc` / `needs_recur_update` / `read_only`: `Context::initialize`
//!   (replica opened read-write) and `Context::dispatch` (gc, then
//!   `handleUntil` / `handleRecurrence`).
//! - `needs_confirm`: only [`Command::permission`].
//!
//! # Permission state (`_permission_quit`, `_permission_all`, `_first_iteration`)
//!
//! C++ keeps these on the `Command` object, which is created once per
//! process by `Command::factory`; one process executes one command, so the
//! state lives for exactly one command execution, and nothing resets it.
//! Here the registry is built by [`factory`] once per `Context`
//! (`Context::initialize`), and the state is held in [`Cell`]s inside that
//! command's [`CommandMeta`] (commands are shared as `Rc<dyn Command>` and
//! executed through `&self`). So the state is per Context, i.e. per command
//! execution, as in C++: a second `Context` gets fresh state, and a command
//! looping over tasks sees `all` / `quit` answers from earlier iterations.
//! Commands read the C++ `_permission_quit` (to `break` their loop) as
//! `self.meta().permission_quit.get()`.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use super::cmd_add::CmdAdd;
use super::cmd_aliases::CmdCompletionAliases;
use super::cmd_annotate::CmdAnnotate;
use super::cmd_append::CmdAppend;
use super::cmd_attributes::CmdZshAttributes;
use super::cmd_burndown::{CmdBurndownAnnual, CmdBurndownDaily, CmdBurndownMonthly, CmdBurndownWeekly};
use super::cmd_calc::CmdCalc;
use super::cmd_calendar::CmdCalendar;
use super::cmd_color::CmdColor;
use super::cmd_columns::{CmdColumns, CmdCompletionColumns};
use super::cmd_commands::{CmdCommands, CmdCompletionCommands, CmdZshCommands};
use super::cmd_config::{CmdCompletionConfig, CmdConfig};
use super::cmd_context::{CmdCompletionContext, CmdContext};
use super::cmd_count::CmdCount;
use super::cmd_custom::CmdCustom;
use super::cmd_delete::CmdDelete;
use super::cmd_denotate::CmdDenotate;
use super::cmd_diagnostics::CmdDiagnostics;
use super::cmd_done::CmdDone;
use super::cmd_duplicate::CmdDuplicate;
use super::cmd_edit::CmdEdit;
use super::cmd_exec::CmdExec;
use super::cmd_export::CmdExport;
use super::cmd_get::CmdGet;
use super::cmd_help::CmdHelp;
use super::cmd_history::{CmdGHistoryAnnual, CmdGHistoryDaily, CmdGHistoryMonthly, CmdGHistoryWeekly, CmdHistoryAnnual, CmdHistoryDaily, CmdHistoryMonthly, CmdHistoryWeekly};
use super::cmd_ids::{CmdCompletionIds, CmdCompletionUuids, CmdIDs, CmdUUIDs, CmdZshCompletionIds, CmdZshCompletionUuids};
use super::cmd_import::CmdImport;
use super::cmd_import_v2::CmdImportV2;
use super::cmd_info::CmdInfo;
use super::cmd_log::CmdLog;
use super::cmd_logo::CmdLogo;
use super::cmd_modify::CmdModify;
use super::cmd_news::CmdNews;
use super::cmd_prepend::CmdPrepend;
use super::cmd_projects::{CmdCompletionProjects, CmdProjects};
use super::cmd_purge::CmdPurge;
use super::cmd_reports::CmdReports;
use super::cmd_show::{CmdShow, CmdShowRaw};
use super::cmd_start::CmdStart;
use super::cmd_stats::CmdStats;
use super::cmd_stop::CmdStop;
use super::cmd_summary::CmdSummary;
use super::cmd_sync::CmdSync;
use super::cmd_tags::{CmdCompletionTags, CmdTags};
use super::cmd_timesheet::CmdTimesheet;
use super::cmd_udas::{CmdCompletionUDAs, CmdUDAs};
use super::cmd_undo::CmdUndo;
use super::cmd_unique::CmdUnique;
use super::cmd_urgency::CmdUrgency;
use super::cmd_version::{CmdCompletionVersion, CmdVersion};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::shared::configuration::Configuration;
use crate::shared::format::format;
use crate::shared::shared::{confirm, with_console};
use crate::util::confirm4;

/// C++ `std::map<std::string, Command*>` as held by `Context::commands`.
pub type Registry = BTreeMap<String, Rc<dyn Command>>;

////////////////////////////////////////////////////////////////////////////////
/// C++ `Command::Category`, in the same order (so `Ord` matches the C++
/// enum-class comparison used to sort commands by "usefulness").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Category {
    /// `unassigned`.
    #[default]
    Unassigned,
    // In presentation ("usefulness") order: frequently-used categories first.
    /// `metadata`.
    Metadata,
    /// `report`.
    Report,
    /// `operation`.
    Operation,
    /// `context`.
    Context,
    /// `graphs`.
    Graphs,
    /// `config`.
    Config,
    /// `migration`.
    Migration,
    /// `misc`.
    Misc,
    /// `internal`.
    Internal,
    /// `UNDOCUMENTED`.
    Undocumented,
    // Whenever you extend this enum, update CATEGORY_NAMES.
}

/// C++ `Command::categoryNames`, in key order. These strings are
/// intentionally not l10n'd: they are used as identifiers.
pub const CATEGORY_NAMES: [(Category, &str); 11] = [
    (Category::Unassigned, "unassigned"), // should never happen
    (Category::Metadata, "metadata"),
    (Category::Report, "report"),
    (Category::Operation, "operation"),
    (Category::Context, "context"),
    (Category::Graphs, "graphs"),
    (Category::Config, "config"),
    (Category::Migration, "migration"),
    (Category::Misc, "misc"),
    (Category::Internal, "internal"),
    (Category::Undocumented, "undocumented"),
];

impl Category {
    /// Every category, in enum order.
    pub const ALL: [Category; 11] = [
        Category::Unassigned,
        Category::Metadata,
        Category::Report,
        Category::Operation,
        Category::Context,
        Category::Graphs,
        Category::Config,
        Category::Migration,
        Category::Misc,
        Category::Internal,
        Category::Undocumented,
    ];

    /// `Command::categoryNames.at (category)`.
    pub fn name(self) -> &'static str {
        CATEGORY_NAMES[self as usize].1
    }
}

/// C++ `Command::categoryNames` as a map.
pub fn category_names() -> BTreeMap<Category, &'static str> {
    CATEGORY_NAMES.iter().copied().collect()
}

////////////////////////////////////////////////////////////////////////////////
/// The C++ `Command` protected data members. [`Default`] is the C++
/// `Command::Command` initialiser list.
#[derive(Debug, Clone)]
pub struct CommandMeta {
    /// `_keyword`.
    pub keyword: String,
    /// `_usage`.
    pub usage: String,
    /// `_description`.
    pub description: String,
    /// `_read_only` (default `true`).
    pub read_only: bool,
    /// `_displays_id` (default `true`).
    pub displays_id: bool,
    /// `_needs_confirm` (default `false`).
    pub needs_confirm: bool,
    /// `_needs_gc` (default `true`).
    pub needs_gc: bool,
    /// `_needs_recur_update` (default `false`).
    pub needs_recur_update: bool,
    /// `_uses_context` (default `false`).
    pub uses_context: bool,
    /// `_accepts_filter` (default `false`).
    pub accepts_filter: bool,
    /// `_accepts_modifications` (default `false`).
    pub accepts_modifications: bool,
    /// `_accepts_miscellaneous` (default `false`).
    pub accepts_miscellaneous: bool,
    /// `_category` (default `unassigned`).
    pub category: Category,
    /// `_permission_quit` (default `false`); interior-mutable because
    /// commands are shared (`Rc`) and executed through `&self`.
    pub permission_quit: Cell<bool>,
    /// `_permission_all` (default `false`).
    pub permission_all: Cell<bool>,
    /// `_first_iteration` (default `true`).
    pub first_iteration: Cell<bool>,
}

impl Default for CommandMeta {
    fn default() -> Self {
        CommandMeta {
            keyword: String::new(),
            usage: String::new(),
            description: String::new(),
            read_only: true,
            displays_id: true,
            needs_confirm: false,
            needs_gc: true,
            needs_recur_update: false,
            uses_context: false,
            accepts_filter: false,
            accepts_modifications: false,
            accepts_miscellaneous: false,
            category: Category::Unassigned,
            permission_quit: Cell::new(false),
            permission_all: Cell::new(false),
            first_iteration: Cell::new(true),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class Command`.
pub trait Command {
    /// The metadata (C++ protected data members) of this command.
    fn meta(&self) -> &CommandMeta;

    /// C++ `Command::keyword`.
    fn keyword(&self) -> &str {
        &self.meta().keyword
    }

    /// C++ `Command::usage`.
    fn usage(&self) -> &str {
        &self.meta().usage
    }

    /// C++ `Command::description`.
    fn description(&self) -> &str {
        &self.meta().description
    }

    /// C++ `Command::read_only`.
    fn read_only(&self) -> bool {
        self.meta().read_only
    }

    /// C++ `Command::displays_id`.
    fn displays_id(&self) -> bool {
        self.meta().displays_id
    }

    /// C++ `_needs_confirm` (protected in C++; exposed for `permission`).
    fn needs_confirm(&self) -> bool {
        self.meta().needs_confirm
    }

    /// C++ `Command::needs_gc`.
    fn needs_gc(&self) -> bool {
        self.meta().needs_gc
    }

    /// C++ `Command::needs_recur_update`.
    fn needs_recur_update(&self) -> bool {
        self.meta().needs_recur_update
    }

    /// C++ virtual `Command::uses_context`. The configuration replaces the
    /// C++ `Context::getContext ().config` read by the `CmdCustom` /
    /// `CmdTimesheet` overrides (`report.<name>.context`).
    fn uses_context(&self, _config: &Configuration) -> bool {
        self.meta().uses_context
    }

    /// C++ `Command::accepts_filter`.
    fn accepts_filter(&self) -> bool {
        self.meta().accepts_filter
    }

    /// C++ `Command::accepts_modifications`.
    fn accepts_modifications(&self) -> bool {
        self.meta().accepts_modifications
    }

    /// C++ `Command::accepts_miscellaneous`.
    fn accepts_miscellaneous(&self) -> bool {
        self.meta().accepts_miscellaneous
    }

    /// C++ `Command::category`.
    fn category(&self) -> Category {
        self.meta().category
    }

    /// C++ pure virtual `Command::execute (std::string& output)`: runs the
    /// command, appending its report text to `output`, and returns the exit
    /// status. C++ `throw std::string` / `throw int` become `Err`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32>;

    /// C++ protected `Command::permission`: whether to proceed with a write
    /// command, on a per-task basis, after (potentially) asking.
    ///
    /// Factors: `quantity` (the filtered size), `rc.bulk`, `rc.confirmation`
    /// and `read_only`.
    fn permission(&self, context: &mut Context, question: &str, quantity: u32) -> bool {
        let meta = self.meta();

        // Read-only commands do not need to seek permission.  Write commands are
        // granted permission automatically if the 'all' selection was made in an
        // earlier call.  Or if the 'all' option has already been made.
        if meta.read_only || meta.permission_all.get() {
            return true;
        }

        // If the 'quit' selection has already been made.
        if meta.permission_quit.get() {
            return false;
        }

        // What remains are write commands that have not yet selected 'all' or 'quit'.
        // Describe the task.
        let confirmation = context.config.get_boolean("confirmation");
        // C++ `unsigned int bulk = getInteger (...)`: negative values wrap.
        let bulk = context.config.get_integer("bulk") as u32;

        // Quantity 1 modifications have optional confirmation, and only (y/n).
        if quantity == 1 {
            if !meta.needs_confirm || !confirmation {
                return true;
            }

            return confirm(question);
        }

        // 1 < Quantity < bulk modifications have optional confirmation, in the (y/n/a/q)
        // style. Bulk = 0 denotes infinite bulk.
        if (bulk == 0 || quantity < bulk) && (!meta.needs_confirm || !confirmation) {
            return true;
        }

        if context.verbose("blank") && !meta.first_iteration.get() {
            with_console(|_, stdout| {
                let _ = stdout.write_all(b"\n");
                let _ = stdout.flush();
            });
        }
        let answer = confirm4(context, question);
        meta.first_iteration.set(false);
        match answer {
            1 => true, // yes
            2 => {
                meta.permission_all.set(true);
                true // all
            }
            3 => {
                meta.permission_quit.set(true);
                false // quit
            }
            _ => false,
        }
    }
}

/// `all[c->keyword ()] = c;`
pub fn register(all: &mut Registry, command: Rc<dyn Command>) {
    all.insert(command.keyword().to_string(), command);
}

////////////////////////////////////////////////////////////////////////////////
/// C++ static `Command::factory`: instantiates every built-in command into
/// `all`, then one `CmdCustom` per `report.<name>.columns` setting in
/// `config` (the C++ reads `Context::getContext().config`).
///
/// Fails with `Custom report '<name>' conflicts with built-in task command.`
/// when a custom report has a built-in command's name.
///
/// The built-in commands are registered in the C++ order (one per C++ class,
/// see the `cmd_*` modules); `CmdExec` sits behind `#ifdef HAVE_EXECUTE`,
/// which `cmake.h.in` defines unconditionally, so it is always registered.
pub fn factory(all: &mut Registry, config: &Configuration) -> Result<()> {
    register(all, Rc::new(CmdAdd::new()));
    register(all, Rc::new(CmdAnnotate::new()));
    register(all, Rc::new(CmdAppend::new()));
    register(all, Rc::new(CmdBurndownAnnual::new()));
    register(all, Rc::new(CmdBurndownDaily::new()));
    register(all, Rc::new(CmdBurndownMonthly::new()));
    register(all, Rc::new(CmdBurndownWeekly::new()));
    register(all, Rc::new(CmdCalc::new()));
    register(all, Rc::new(CmdCalendar::new()));
    register(all, Rc::new(CmdColor::new()));
    register(all, Rc::new(CmdColumns::new()));
    register(all, Rc::new(CmdCommands::new()));
    register(all, Rc::new(CmdCompletionAliases::new()));
    register(all, Rc::new(CmdCompletionColumns::new()));
    register(all, Rc::new(CmdCompletionCommands::new()));
    register(all, Rc::new(CmdCompletionConfig::new()));
    register(all, Rc::new(CmdCompletionContext::new()));
    register(all, Rc::new(CmdCompletionIds::new()));
    register(all, Rc::new(CmdCompletionUDAs::new()));
    register(all, Rc::new(CmdCompletionUuids::new()));
    register(all, Rc::new(CmdCompletionProjects::new()));
    register(all, Rc::new(CmdCompletionTags::new()));
    register(all, Rc::new(CmdCompletionVersion::new()));
    register(all, Rc::new(CmdConfig::new()));
    register(all, Rc::new(CmdContext::new()));
    register(all, Rc::new(CmdCount::new()));
    register(all, Rc::new(CmdDelete::new()));
    register(all, Rc::new(CmdDenotate::new()));
    register(all, Rc::new(CmdDiagnostics::new()));
    register(all, Rc::new(CmdDone::new()));
    register(all, Rc::new(CmdDuplicate::new()));
    register(all, Rc::new(CmdEdit::new()));
    // C++ `#ifdef HAVE_EXECUTE`: `cmake.h.in` always defines it.
    register(all, Rc::new(CmdExec::new()));
    register(all, Rc::new(CmdExport::new()));
    register(all, Rc::new(CmdGet::new()));
    register(all, Rc::new(CmdGHistoryDaily::new()));
    register(all, Rc::new(CmdGHistoryWeekly::new()));
    register(all, Rc::new(CmdGHistoryMonthly::new()));
    register(all, Rc::new(CmdGHistoryAnnual::new()));
    register(all, Rc::new(CmdHelp::new()));
    register(all, Rc::new(CmdHistoryDaily::new()));
    register(all, Rc::new(CmdHistoryWeekly::new()));
    register(all, Rc::new(CmdHistoryMonthly::new()));
    register(all, Rc::new(CmdHistoryAnnual::new()));
    register(all, Rc::new(CmdIDs::new()));
    register(all, Rc::new(CmdImport::new()));
    register(all, Rc::new(CmdImportV2::new()));
    register(all, Rc::new(CmdInfo::new()));
    register(all, Rc::new(CmdLog::new()));
    register(all, Rc::new(CmdLogo::new()));
    register(all, Rc::new(CmdModify::new()));
    register(all, Rc::new(CmdNews::new()));
    register(all, Rc::new(CmdPrepend::new()));
    register(all, Rc::new(CmdProjects::new()));
    register(all, Rc::new(CmdPurge::new()));
    register(all, Rc::new(CmdReports::new()));
    register(all, Rc::new(CmdShow::new()));
    register(all, Rc::new(CmdShowRaw::new()));
    register(all, Rc::new(CmdStart::new()));
    register(all, Rc::new(CmdStats::new()));
    register(all, Rc::new(CmdStop::new()));
    register(all, Rc::new(CmdSummary::new()));
    register(all, Rc::new(CmdSync::new()));
    register(all, Rc::new(CmdTags::new()));
    register(all, Rc::new(CmdTimesheet::new()));
    register(all, Rc::new(CmdUDAs::new()));
    register(all, Rc::new(CmdUndo::new()));
    register(all, Rc::new(CmdUnique::new()));
    register(all, Rc::new(CmdUrgency::new()));
    register(all, Rc::new(CmdUUIDs::new()));
    register(all, Rc::new(CmdVersion::new()));
    register(all, Rc::new(CmdZshAttributes::new()));
    register(all, Rc::new(CmdZshCommands::new()));
    register(all, Rc::new(CmdZshCompletionIds::new()));
    register(all, Rc::new(CmdZshCompletionUuids::new()));

    // Instantiate a command object for each custom report.
    let mut reports = Vec::new();
    for key in config.keys() {
        if let Some(report) = key.strip_prefix("report.") {
            if let Some(columns) = report.find(".columns") {
                reports.push(report[..columns].to_string());
            }
        }
    }

    for report in &reports {
        // Make sure a custom report does not clash with a built-in command.
        if all.contains_key(report) {
            return Err(TwError::Message(format(
                "Custom report '{1}' conflicts with built-in task command.",
                &[report],
            )));
        }

        register(
            all,
            Rc::new(CmdCustom::new(
                report,
                &format!("task <filter> {report}"),
                &config.get(&format!("report.{report}.description")),
            )),
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Probe {
        meta: CommandMeta,
    }

    impl Command for Probe {
        fn meta(&self) -> &CommandMeta {
            &self.meta
        }

        fn execute(&self, _context: &mut Context, output: &mut String) -> Result<i32> {
            output.push_str("probe\n");
            Ok(0)
        }
    }

    #[test]
    fn category_order_and_names() {
        for pair in Category::ALL.windows(2) {
            assert!(pair[0] < pair[1]);
        }
        let names: Vec<&str> = Category::ALL.iter().map(|c| c.name()).collect();
        assert_eq!(
            names,
            [
                "unassigned",
                "metadata",
                "report",
                "operation",
                "context",
                "graphs",
                "config",
                "migration",
                "misc",
                "internal",
                "undocumented"
            ]
        );
        assert_eq!(category_names()[&Category::Graphs], "graphs");
        assert_eq!(Category::default(), Category::Unassigned);
    }

    #[test]
    fn meta_defaults_match_cpp_constructor() {
        let m = CommandMeta::default();
        assert_eq!(m.keyword, "");
        assert!(m.read_only && m.displays_id && m.needs_gc && m.first_iteration.get());
        assert!(!m.needs_confirm && !m.needs_recur_update && !m.uses_context);
        assert!(!m.accepts_filter && !m.accepts_modifications && !m.accepts_miscellaneous);
        assert!(!m.permission_quit.get() && !m.permission_all.get());
        assert_eq!(m.category, Category::Unassigned);
    }

    #[test]
    fn accessors_and_register() {
        let probe = Probe {
            meta: CommandMeta {
                keyword: "probe".into(),
                usage: "task probe".into(),
                description: "Probe".into(),
                read_only: false,
                accepts_filter: true,
                category: Category::Misc,
                ..CommandMeta::default()
            },
        };
        assert_eq!(probe.keyword(), "probe");
        assert_eq!(probe.usage(), "task probe");
        assert_eq!(probe.description(), "Probe");
        assert!(!probe.read_only());
        assert!(probe.accepts_filter());
        assert_eq!(probe.category(), Category::Misc);

        let mut all = Registry::new();
        register(&mut all, Rc::new(probe));
        assert!(all.contains_key("probe"));
    }

    #[test]
    fn permission_without_prompting() {
        let mut context = Context::new();
        let read_only = Probe {
            meta: CommandMeta::default(),
        };
        assert!(read_only.permission(&mut context, "q?", 5));

        let write = Probe {
            meta: CommandMeta {
                read_only: false,
                needs_confirm: true,
                ..CommandMeta::default()
            },
        };
        // confirmation off: single and below-bulk changes proceed (a bulk
        // change, quantity >= bulk, would prompt regardless).
        context.config.set("confirmation", "off");
        context.config.set("bulk", 3);
        assert!(write.permission(&mut context, "q?", 1));
        assert!(write.permission(&mut context, "q?", 2));
        // bulk=0 means infinite bulk.
        context.config.set("bulk", 0);
        assert!(write.permission(&mut context, "q?", 1000));
        context.config.set("bulk", 3);

        // 'all' / 'quit' already chosen.
        write.meta.permission_quit.set(true);
        assert!(!write.permission(&mut context, "q?", 1));
        write.meta.permission_all.set(true);
        assert!(write.permission(&mut context, "q?", 1));

        // Quantity below bulk without needs_confirm proceeds.
        let no_confirm = Probe {
            meta: CommandMeta {
                read_only: false,
                ..CommandMeta::default()
            },
        };
        context.config.set("confirmation", "on");
        assert!(no_confirm.permission(&mut context, "q?", 2));
    }

    fn write_probe(needs_confirm: bool) -> Probe {
        Probe {
            meta: CommandMeta {
                keyword: "probe".into(),
                read_only: false,
                needs_confirm,
                ..CommandMeta::default()
            },
        }
    }

    /// `permission` with a scripted console: (answer, stdout).
    fn ask(probe: &Probe, context: &mut Context, input: &str, quantity: u32) -> (bool, String) {
        crate::shared::shared::begin_console_override(input);
        let answer = probe.permission(context, "Change it?", quantity);
        (answer, crate::shared::shared::end_console_override())
    }

    fn confirming_context() -> Context {
        let mut context = Context::new();
        context.config.set("confirmation", "on");
        context.config.set("bulk", 3);
        context.config.set("verbose", "blank");
        context
    }

    #[test]
    fn permission_single_task_asks_yes_no() {
        let mut context = confirming_context();
        let probe = write_probe(true);
        assert_eq!(ask(&probe, &mut context, "yes\n", 1), (true, "Change it? (yes/no) ".to_string()));
        assert_eq!(ask(&probe, &mut context, "  NO \n", 1), (false, "Change it? (yes/no) ".to_string()));
        // End of input means no; 'all' is not an option for a single task.
        assert_eq!(ask(&probe, &mut context, "", 1), (false, "Change it? (yes/no) ".to_string()));
        assert_eq!(
            ask(&probe, &mut context, "all\ny\n", 1),
            (true, "Change it? (yes/no) Change it? (yes/no) ".to_string())
        );
        // (y/n) style never touches the 'all'/'quit' state or first iteration.
        assert!(!probe.meta.permission_all.get() && !probe.meta.permission_quit.get());
        assert!(probe.meta.first_iteration.get());

        // Without rc.confirmation (or without needs_confirm) a single task proceeds.
        context.config.set("confirmation", "off");
        assert_eq!(ask(&probe, &mut context, "no\n", 1), (true, String::new()));
        context.config.set("confirmation", "on");
        assert_eq!(ask(&write_probe(false), &mut context, "no\n", 1), (true, String::new()));
    }

    #[test]
    fn permission_bulk_asks_yes_no_all_quit() {
        let mut context = confirming_context();
        let probe = write_probe(false);

        // Below bulk without needs_confirm: no prompt.
        assert_eq!(ask(&probe, &mut context, "no\n", 2), (true, String::new()));

        // At bulk the (yes/no/all/quit) prompt appears even without needs_confirm.
        let prompt = "Change it? (yes/no/all/quit) ";
        assert_eq!(ask(&probe, &mut context, "y\n", 3), (true, prompt.to_string()));
        assert!(!probe.meta.first_iteration.get());
        // Later prompts are preceded by a blank line (verbose=blank); unknown
        // and ambiguous-free retries re-ask; 'Yes' lower-cases to 'yes'.
        assert_eq!(
            ask(&probe, &mut context, "maybe\n\nYes\n", 3),
            (true, format!("\n{prompt}{prompt}{prompt}"))
        );
        assert_eq!(ask(&probe, &mut context, "n\n", 3), (false, format!("\n{prompt}")));
        assert!(!probe.meta.permission_all.get() && !probe.meta.permission_quit.get());

        // 'all': granted now and for every later call, without asking.
        assert_eq!(ask(&probe, &mut context, "a\n", 3), (true, format!("\n{prompt}")));
        assert!(probe.meta.permission_all.get());
        assert_eq!(ask(&probe, &mut context, "no\n", 5), (true, String::new()));
        assert_eq!(ask(&probe, &mut context, "no\n", 1), (true, String::new()));

        // The raw answers are recorded as debug messages.
        assert!(context.debug_messages.iter().any(|m| m == "STDIN 'maybe'"));
        assert!(context.debug_messages.iter().any(|m| m == "STDIN ''"));
        assert!(context.debug_messages.iter().any(|m| m == "STDIN 'Yes'"));
    }

    #[test]
    fn permission_quit_and_end_of_input() {
        let mut context = confirming_context();
        context.config.set("verbose", "nothing");
        let probe = write_probe(true);
        let prompt = "Change it? (yes/no/all/quit) ";

        // needs_confirm + confirmation: even 1 < quantity < bulk asks.
        assert_eq!(ask(&probe, &mut context, "quit\n", 2), (false, prompt.to_string()));
        assert!(probe.meta.permission_quit.get());
        // 'quit' refuses every later call without asking.
        assert_eq!(ask(&probe, &mut context, "yes\n", 2), (false, String::new()));
        assert_eq!(ask(&probe, &mut context, "yes\n", 1), (false, String::new()));

        // End of input means 'quit'; no blank line without verbose=blank.
        let fresh = write_probe(true);
        assert_eq!(ask(&fresh, &mut context, "y\n", 4), (true, prompt.to_string()));
        assert_eq!(ask(&fresh, &mut context, "", 4), (false, prompt.to_string()));
        assert!(fresh.meta.permission_quit.get());
    }

    #[test]
    fn permission_state_is_per_registry() {
        // Each Context builds its own registry, so the C++ per-object (per
        // execution) permission state never leaks between executions.
        let config = default_config();
        let mut first = Registry::new();
        let mut second = Registry::new();
        factory(&mut first, &config).unwrap();
        factory(&mut second, &config).unwrap();
        let delete = &first["delete"];
        delete.meta().permission_quit.set(true);
        delete.meta().permission_all.set(true);
        delete.meta().first_iteration.set(false);
        let other = second["delete"].meta();
        assert!(!other.permission_quit.get() && !other.permission_all.get() && other.first_iteration.get());
    }

    #[test]
    fn factory_custom_report_conflict() {
        // A custom report may not reuse a built-in keyword.
        let mut config = Configuration::new();
        config.set("report.add.columns", "id");
        let err = factory(&mut Registry::new(), &config).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Custom report 'add' conflicts with built-in task command."
        );

        // Nor the keyword of a command registered before the factory runs.
        let mut config = Configuration::new();
        config.set("report.probe.columns", "id");
        config.set("report.mine.columns", "id");
        let mut all = Registry::new();
        factory(&mut all, &config).unwrap();
        assert!(all.contains_key("probe") && all.contains_key("mine"));

        let mut all = Registry::new();
        register(
            &mut all,
            Rc::new(Probe {
                meta: CommandMeta {
                    keyword: "probe".into(),
                    ..CommandMeta::default()
                },
            }),
        );
        let err = factory(&mut all, &config).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Custom report 'probe' conflicts with built-in task command."
        );
    }

    /// The keywords of the 75 classes C++ `Command::factory` instantiates
    /// (with `HAVE_EXECUTE`), sorted.
    const CPP_BUILTIN_KEYWORDS: [&str; 75] = [
        "_aliases",
        "_columns",
        "_commands",
        "_config",
        "_context",
        "_get",
        "_ids",
        "_projects",
        "_show",
        "_tags",
        "_udas",
        "_unique",
        "_urgency",
        "_uuids",
        "_version",
        "_zshattributes",
        "_zshcommands",
        "_zshids",
        "_zshuuids",
        "add",
        "annotate",
        "append",
        "burndown.annual",
        "burndown.daily",
        "burndown.monthly",
        "burndown.weekly",
        "calc",
        "calendar",
        "colors",
        "columns",
        "commands",
        "config",
        "context",
        "count",
        "delete",
        "denotate",
        "diagnostics",
        "done",
        "duplicate",
        "edit",
        "execute",
        "export",
        "ghistory.annual",
        "ghistory.daily",
        "ghistory.monthly",
        "ghistory.weekly",
        "help",
        "history.annual",
        "history.daily",
        "history.monthly",
        "history.weekly",
        "ids",
        "import",
        "import-v2",
        "information",
        "log",
        "logo",
        "modify",
        "news",
        "prepend",
        "projects",
        "purge",
        "reports",
        "show",
        "start",
        "stats",
        "stop",
        "summary",
        "synchronize",
        "tags",
        "timesheet",
        "udas",
        "undo",
        "uuids",
        "version",
    ];

    /// The custom reports of the C++ `configurationDefaults`, with their
    /// `report.<name>.description`.
    const CPP_DEFAULT_REPORTS: [(&str, &str); 17] = [
        ("active", "Active tasks"),
        ("all", "All tasks"),
        ("blocked", "Blocked tasks"),
        ("blocking", "Blocking tasks"),
        ("completed", "Completed tasks"),
        ("list", "Most details of tasks"),
        ("long", "All details of tasks"),
        ("ls", "Few details of tasks"),
        ("minimal", "Minimal details of tasks"),
        ("newest", "Newest tasks"),
        ("next", "Most urgent tasks"),
        ("oldest", "Oldest tasks"),
        ("overdue", "Overdue tasks"),
        ("ready", "Most urgent actionable tasks"),
        ("recurring", "Recurring Tasks"),
        ("unblocked", "Unblocked tasks"),
        ("waiting", "Waiting (hidden) tasks"),
    ];

    /// Commands whose `execute` has been ported (milestone 1: `version`,
    /// `_version`, `_show`, `_config`, `config`; milestone 3: `calc`;
    /// milestone 5: `add`, `log`, `export`, `_get`, `count`); every other
    /// one must still be a stub.
    const PORTED: [&str; 26] = [
        "_columns",
        "_config",
        "_get",
        "_ids",
        "_show",
        "_unique",
        "_urgency",
        "_uuids",
        "_version",
        "_zshattributes",
        "_zshids",
        "_zshuuids",
        "add",
        "calc",
        "colors",
        "columns",
        "config",
        "count",
        "export",
        "ids",
        "information",
        "log",
        "modify",
        "reports",
        "uuids",
        "version",
    ];

    fn default_config() -> Configuration {
        let mut config = Configuration::new();
        config
            .parse(
                crate::context::CONFIGURATION_DEFAULTS,
                1,
                &[crate::context::task_rcdir()],
                "",
            )
            .unwrap();
        config
    }

    #[test]
    fn factory_registers_exactly_the_cpp_builtins() {
        let mut all = Registry::new();
        factory(&mut all, &Configuration::new()).unwrap();
        let keywords: Vec<&str> = all.keys().map(String::as_str).collect();
        assert_eq!(keywords, CPP_BUILTIN_KEYWORDS);
        for (keyword, command) in &all {
            assert_eq!(command.keyword(), keyword);
        }
    }

    #[test]
    fn factory_registers_default_custom_reports() {
        let config = default_config();
        let mut all = Registry::new();
        factory(&mut all, &config).unwrap();
        assert_eq!(all.len(), CPP_BUILTIN_KEYWORDS.len() + CPP_DEFAULT_REPORTS.len());

        let mut expected: Vec<&str> = CPP_BUILTIN_KEYWORDS.to_vec();
        expected.extend(CPP_DEFAULT_REPORTS.iter().map(|(name, _)| *name));
        expected.sort_unstable();
        let keywords: Vec<&str> = all.keys().map(String::as_str).collect();
        assert_eq!(keywords, expected);

        for (name, description) in CPP_DEFAULT_REPORTS {
            let c = &all[name];
            assert_eq!(c.keyword(), name);
            assert_eq!(c.usage(), format!("task <filter> {name}"));
            assert_eq!(c.description(), description);
            assert!(c.read_only() && c.displays_id() && c.needs_gc() && c.needs_recur_update());
            assert!(c.uses_context(&Configuration::new()) && c.accepts_filter());
            assert!(!c.needs_confirm() && !c.accepts_modifications() && !c.accepts_miscellaneous());
            assert_eq!(c.category(), Category::Report);
        }
    }

    #[test]
    fn custom_report_without_description() {
        let mut config = Configuration::new();
        config.set("report.mine.columns", "id");
        let mut all = Registry::new();
        factory(&mut all, &config).unwrap();
        assert_eq!(all["mine"].usage(), "task <filter> mine");
        assert_eq!(all["mine"].description(), "");
    }

    /// (keyword, usage, description, read_only, displays_id, needs_confirm,
    /// needs_gc, needs_recur_update, uses_context, accepts_filter,
    /// accepts_modifications, accepts_miscellaneous, category), as set by
    /// the C++ constructors (or left at the `Command::Command` defaults).
    #[allow(clippy::type_complexity)]
    const SPOT_CHECKS: [(
        &str,
        &str,
        &str,
        [bool; 9],
        Category,
    ); 10] = [
        (
            "add",
            "task          add <mods>",
            "Adds a new task",
            [false, false, false, false, false, true, false, true, false],
            Category::Operation,
        ),
        (
            "delete",
            "task <filter> delete <mods>",
            "Deletes the specified task",
            [false, false, true, false, false, true, true, true, false],
            Category::Operation,
        ),
        (
            "purge",
            "task <filter> purge",
            "Removes the specified tasks from the data files. Causes permanent loss of data.",
            [false, false, true, true, false, true, true, false, false],
            Category::Operation,
        ),
        (
            "show",
            "task          show [all | substring]",
            "Shows all configuration variables or subset",
            [true, false, false, false, false, false, false, false, true],
            Category::Config,
        ),
        (
            "_show",
            "task          _show",
            "Shows all configuration settings in a machine-readable format",
            [true, false, false, true, false, false, false, false, false],
            Category::Internal,
        ),
        (
            "version",
            "task          version",
            "Shows the Taskwarrior version number",
            [true, false, false, false, false, false, false, false, false],
            Category::Misc,
        ),
        (
            "_version",
            "task          _version",
            "Shows only the Taskwarrior version number",
            [true, false, false, false, false, false, false, false, false],
            Category::Internal,
        ),
        (
            "burndown.weekly",
            "task <filter> burndown.weekly",
            "Shows a graphical burndown chart, by weekly",
            [true, false, false, true, true, true, true, false, false],
            Category::Graphs,
        ),
        (
            "ghistory.monthly",
            "task <filter> ghistory.monthly",
            "Shows a graphical report of task history, by month",
            [true, false, false, false, true, true, true, false, false],
            Category::Graphs,
        ),
        (
            "information",
            "task <filter> information",
            "Shows all data and metadata",
            [true, false, false, false, false, false, true, false, false],
            Category::Metadata,
        ),
    ];

    #[test]
    fn builtin_metadata_matches_cpp_constructors() {
        let mut all = Registry::new();
        factory(&mut all, &Configuration::new()).unwrap();
        for (keyword, usage, description, flags, category) in SPOT_CHECKS {
            let c = &all[keyword];
            let actual = [
                c.read_only(),
                c.displays_id(),
                c.needs_confirm(),
                c.needs_gc(),
                c.needs_recur_update(),
                c.uses_context(&Configuration::new()),
                c.accepts_filter(),
                c.accepts_modifications(),
                c.accepts_miscellaneous(),
            ];
            assert_eq!(c.usage(), usage, "{keyword}");
            assert_eq!(c.description(), description, "{keyword}");
            assert_eq!(actual, flags, "{keyword}");
            assert_eq!(c.category(), category, "{keyword}");
        }
    }

    #[test]
    fn unported_commands_are_not_implemented() {
        let mut all = Registry::new();
        factory(&mut all, &default_config()).unwrap();
        let mut context = Context::new();
        let config = default_config();
        let mut stubs = 0;
        let mut custom = 0;
        for (keyword, command) in &all {
            if PORTED.contains(&keyword.as_str()) {
                continue;
            }
            // Custom reports (one CmdCustom per `report.<name>.columns`) are
            // registered dynamically and are all ported.
            if config.has(&format!("report.{keyword}.columns")) {
                custom += 1;
                continue;
            }
            let mut output = String::new();
            let err = command.execute(&mut context, &mut output).unwrap_err();
            assert!(matches!(&err, TwError::NotImplemented(k) if k == keyword), "{keyword}");
            assert_eq!(err.to_string(), format!("Command '{keyword}' is not yet implemented."));
            assert_eq!(output, "");
            stubs += 1;
        }
        assert!(custom > 0);
        assert_eq!(stubs, all.len() - PORTED.len() - custom);
    }

    #[test]
    fn ported_commands_execute() {
        let mut all = Registry::new();
        factory(&mut all, &default_config()).unwrap();
        let mut context = Context::new();
        context.config = default_config();
        for keyword in PORTED {
            let mut output = String::new();
            if keyword == "config" {
                // Without a name to modify, `config` fails like C++.
                let err = all[keyword].execute(&mut context, &mut output).unwrap_err();
                assert_eq!(err.to_string(), "Specify the name of a config variable to modify.");
                assert_eq!(output, "");
                continue;
            }
            if keyword == "calc" {
                // Without words, `calc` has nothing to evaluate, like C++.
                let err = all[keyword].execute(&mut context, &mut output).unwrap_err();
                assert_eq!(err.to_string(), "No expression to evaluate.");
                assert_eq!(output, "");
                continue;
            }
            if keyword == "add" || keyword == "log" {
                // Without modifications there is no description (text_required).
                let err = all[keyword].execute(&mut context, &mut output).unwrap_err();
                assert_eq!(err.to_string(), "Additional text must be provided.");
                assert_eq!(output, "");
                continue;
            }
            if keyword == "_get" {
                let err = all[keyword].execute(&mut context, &mut output).unwrap_err();
                assert_eq!(err.to_string(), "No DOM reference specified.");
                assert_eq!(output, "");
                continue;
            }
            if matches!(
                keyword,
                "count"
                    | "export"
                    | "ids"
                    | "_ids"
                    | "_zshids"
                    | "uuids"
                    | "_uuids"
                    | "_zshuuids"
                    | "_unique"
                    | "_urgency"
                    | "information"
                    | "_zshattributes"
                    | "_columns"
                    | "modify"
            ) {
                // These read the replica (or, for `_zshattributes` and
                // `_columns`, the column registry Context::initialize builds);
                // covered end to end in tests/cli.rs.
                continue;
            }
            if keyword == "colors" {
                // Without colour (no TTY, no _forcecolor) the C++ hint and 1.
                let rc = all[keyword].execute(&mut context, &mut output).unwrap();
                if !context.color() {
                    assert_eq!(rc, 1);
                    assert!(output.starts_with("Color is currently turned off"), "{output:?}");
                } else {
                    assert_eq!(rc, 0);
                    assert!(output.contains("Basic colors"), "{output:?}");
                }
                continue;
            }
            let rc = all[keyword].execute(&mut context, &mut output).unwrap();
            assert_eq!(rc, 0, "{keyword}");
            assert!(output.ends_with('\n') && output.len() > 1, "{keyword}: {output:?}");
        }
    }
}
