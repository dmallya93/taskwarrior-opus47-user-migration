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
//! # Still to come
//!
//! - The `factory` registration list: task #8 adds every built-in command
//!   (the C++ list is reproduced in [`factory`]'s documentation) and the
//!   `CmdCustom` construction for `report.<name>.columns`; the custom-report
//!   name scan and its conflict error are already here.
//! - Nothing else in `Command.cpp` depends on Task/TDB2: `permission` is
//!   ported as [`Command::permission`].

use std::cell::Cell;
use std::collections::BTreeMap;
use std::io::Write;
use std::rc::Rc;

use crate::context::Context;
use crate::error::{Result, TwError};
use crate::shared::configuration::Configuration;
use crate::shared::format::format;
use crate::shared::shared::confirm;
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

    /// C++ virtual `Command::uses_context`.
    fn uses_context(&self) -> bool {
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
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(b"\n");
            let _ = stdout.flush();
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
/// The C++ registers, in this order: CmdAdd, CmdAnnotate, CmdAppend,
/// CmdBurndownAnnual, CmdBurndownDaily, CmdBurndownMonthly, CmdBurndownWeekly,
/// CmdCalc, CmdCalendar, CmdColor, CmdColumns, CmdCommands,
/// CmdCompletionAliases, CmdCompletionColumns, CmdCompletionCommands,
/// CmdCompletionConfig, CmdCompletionContext, CmdCompletionIds,
/// CmdCompletionUDAs, CmdCompletionUuids, CmdCompletionProjects,
/// CmdCompletionTags, CmdCompletionVersion, CmdConfig, CmdContext, CmdCount,
/// CmdDelete, CmdDenotate, CmdDiagnostics, CmdDone, CmdDuplicate, CmdEdit,
/// CmdExec (`HAVE_EXECUTE`), CmdExport, CmdGet, CmdGHistoryDaily,
/// CmdGHistoryWeekly, CmdGHistoryMonthly, CmdGHistoryAnnual, CmdHelp,
/// CmdHistoryDaily, CmdHistoryWeekly, CmdHistoryMonthly, CmdHistoryAnnual,
/// CmdIDs, CmdImport, CmdImportV2, CmdInfo, CmdLog, CmdLogo, CmdModify,
/// CmdNews, CmdPrepend, CmdProjects, CmdPurge, CmdReports, CmdShow,
/// CmdShowRaw, CmdStart, CmdStats, CmdStop, CmdSummary, CmdSync, CmdTags,
/// CmdTimesheet, CmdUDAs, CmdUndo, CmdUnique, CmdUrgency, CmdUUIDs,
/// CmdVersion, CmdZshAttributes, CmdZshCommands, CmdZshCompletionIds,
/// CmdZshCompletionUuids.
///
/// Task #8 hook: none of those structs exists yet, so nothing is registered
/// here; task #8 adds one `register(all, Rc::new(CmdX::new()));` line per
/// class above and the `CmdCustom` construction below.
pub fn factory(all: &mut Registry, config: &Configuration) -> Result<()> {
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

        // Task #8 hook: C++
        //   c = new CmdCustom (report, "task <filter> " + report,
        //                      config.get ("report." + report + ".description"));
        //   all[c->keyword ()] = c;
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

    #[test]
    fn factory_custom_report_conflict() {
        let mut config = Configuration::new();
        config.set("report.probe.columns", "id");
        config.set("report.mine.columns", "id");
        let mut all = Registry::new();
        factory(&mut all, &config).unwrap();

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
}
