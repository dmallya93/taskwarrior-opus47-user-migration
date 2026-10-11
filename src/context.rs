//! Rust translation of `src/Context.h` / `src/Context.cpp`.
//!
//! # Shape (milestone 1 Design Decision 1, foundation design notes DD1/DD2)
//!
//! There is no `Context::getContext ()` singleton: a [`Context`] is created by
//! `main` and passed explicitly as `&mut Context`. Its fields mirror the C++
//! public members one for one (see the struct documentation); in addition
//! [`Context::defaults`] holds the compiled-in defaults parsed on their own,
//! for `show`'s "differs from default" checks (DD2).
//!
//! # Driving it (what `main.rs` does, mirroring `main.cpp`)
//!
//! ```ignore
//! let mut context = Context::new();
//! let mut status = context.initialize(&args)?;   // all startup, never Err today
//! if status == 0 { status = context.run(); }
//! ```
//!
//! [`Context::initialize`] and [`Context::run`] write diagnostics to the real
//! stderr and report output to the real stdout, exactly where C++ writes
//! them; [`Context::initialize_with`] / [`Context::run_with`] take the two
//! streams explicitly (used by the unit tests).
//!
//! # Completeness, and the remaining hook points
//!
//! Every function of `Context.cpp` is ported, in the C++ order:
//!
//! - `initialize`: rc file resolution and load, `rc.` overrides
//!   ([`cli2::apply_overrides`]), data location, `createDefaultConfig`,
//!   `Command::factory` / `Column::factory` entities, modifier and operator
//!   entities, `initializeColorRules` ([`rules::initialize_color_rules`],
//!   which makes the `color ()` / `_forcecolor` decision and loads the
//!   `color.*` rules), `staticInitialization` (CLI2 / Lexer / Task / Variant
//!   / Datetime / Duration / TDB2 statics, thread-local here), `propagateDebug`,
//!   `loadAliases`, the full [`Cli2`] parse (`analyze`, run detached via
//!   [`cli2::with_cli2`]), the `[<default command>]` and "assuming
//!   'information'" headers, `TDB2::open_replica` and `Hooks::initialize`.
//! - `run`: `Hooks::onLaunch`, `dispatch`, `Hooks::onExit`, the Perf debug
//!   line, then debug messages, headers, output, footnotes and errors, each
//!   coloured with the `rules.cpp` `colorize*` functions when `color ()`.
//! - `dispatch`: `updateXtermTitle`, `updateVerbosity`, `TDB2::gc` for
//!   `needs_gc` commands, `prepareFilter` for commands accepting
//!   modifications but no filter, the `rc.debug.parser=1` parse-tree dump,
//!   `handleUntil` / `handleRecurrence` for `needs_recur_update` commands
//!   with `rc.gc`, then the command (or `help` when there is none).
//! - `getWidth` / `getHeight` (`ioctl (TIOCGWINSZ)`), `getTaskContext`,
//!   `color`, `verbose`, `getColumns`, `getLimits`, `decomposeSortField`,
//!   `debugTiming`, `withCurrentTask`, `header` / `footnote` / `error` /
//!   `debug`.
//!
//! DOM lookups (`getDOM`, Eval's `domSource`) are [`crate::dom`] functions
//! taking `&mut Context`, with the contextual task passed separately
//! (`current_task` is an `Rc`, cloned first).
//!
//! The call sites for subsystems owned by later milestones exist and are
//! called at the C++ places; their bodies are documented no-ops (milestone 5
//! Design Decision 3, option 1):
//!
//! - **Milestone 7**: `handleUntil` / `handleRecurrence`
//!   ([`crate::recur`]).
//!
//! Hooks (`Hooks::initialize` / `onLaunch` / `onExit`) are
//! [`crate::hooks::Hooks`] associated functions taking the Context.
//!
//! Per-task colour rules (`rules.cpp` `autoColorize`) are not part of
//! Context: they are rendered by [`crate::view_task::ViewTask`], which calls
//! [`crate::rules::auto_colorize`] for each row, and Context never calls them.
//!
//! # `TASK_RCDIR`
//!
//! The C++ passes the compile-time install directory `TASK_RCDIR`
//! (`${CMAKE_INSTALL_PREFIX}/share/doc/task/rc`) as the only include search
//! path. Rust has no install step, so [`task_rcdir`] decides at run time:
//! the compile-time environment variable `TASK_RCDIR` if it was set for the
//! build; otherwise the nearest ancestor directory of the running executable
//! that has `share/doc/task/rc/default.theme` (an installed layout) or
//! `doc/rc/default.theme` (the repository layout, which covers
//! `target/{debug,release}/task` and the test binaries in
//! `target/debug/deps`); otherwise `<exe dir>/../share/doc/task/rc`.

use std::collections::BTreeSet;
use std::io::Write;
use std::rc::Rc;

use crate::cli2::{self, Cli2};
use crate::columns::column;
use crate::commands::command::{self, Registry};
use crate::error::{Result, TwError};
use crate::eval::Eval;
use crate::hooks::Hooks;
use crate::lexer::Lexer;
use crate::recur;
use crate::rules::{self, colorize_debug, colorize_error, colorize_footnote, colorize_header};
use crate::shared::configuration::Configuration;
use crate::shared::datetime::Datetime;
use crate::shared::duration::Duration;
use crate::shared::format::{format, printf_f};
use crate::shared::fs::{Directory, File, Path};
use crate::shared::shared::{confirm, split};
use crate::shared::timer::Timer;
use crate::task::Task;
use crate::tdb2::Tdb2;
use crate::version::{Version, COMMIT, PACKAGE_STRING};
use crate::variant::Variant;

/// C++ `std::map<std::string, Column*>` as held by `Context::columns`.
pub use crate::columns::column::ColumnRegistry;

////////////////////////////////////////////////////////////////////////////////
/// C++ `std::string configurationDefaults`: the compiled-in default
/// configuration, parsed before the user's `.taskrc`.
///
/// Copied verbatim from `Context.cpp`, one C++ string literal per element in
/// the same order (a unit test compares it with the C++ source).
// This string is parsed and used as default values for configuration.
// Note: New configuration options should be added to the vim syntax file in
// scripts/vim/syntax/taskrc.vim
pub const CONFIGURATION_DEFAULTS: &str = concat!(
    "# Taskwarrior program configuration file.\n",
    "# For more documentation, see https://taskwarrior.org or try 'man task', 'man task-color',\n",
    "# 'man task-sync' or 'man taskrc'\n",
    "\n",
    "# Here is an example of entries that use the default, override and blank values\n",
    "#   variable=foo   -- By specifying a value, this overrides the default\n",
    "#   variable=      -- By specifying no value, this means no default\n",
    "#   #variable=foo  -- By commenting out the line, or deleting it, this uses the default\n",
    "\n",
    "# You can also refence environment variables:\n",
    "#   variable=$HOME/task\n",
    "#   variable=$VALUE\n",
    "\n",
    "# Use the command 'task show' to see all defaults and overrides\n",
    "\n",
    "# Files\n",
    "data.location=~/.task\n",
    "gc=1                                           # Garbage-collect data files - DO NOT CHANGE ",
    "unless you are sure\n",
    "exit.on.missing.db=0                           # Whether to exit if ~/.task is not found\n",
    "hooks=1                                        # Master control switch for hooks\n",
    "\n",
    "# Terminal\n",
    "detection=1                                    # Detects terminal width\n",
    "defaultwidth=80                                # Without detection, assumed width\n",
    "defaultheight=24                               # Without detection, assumed height\n",
    "avoidlastcolumn=0                              # Fixes Cygwin width problem\n",
    "hyphenate=1                                    # Hyphenates lines wrapped on non-word-breaks\n",
    "#editor=vi                                     # Preferred text editor\n",
    "reserved.lines=1                               # Assume a 1-line prompt\n",
    "\n",
    "# Miscellaneous\n",
    "# verbose=                                     # Comma-separated list.  May contain any ",
    "subset of:\n",
    "# ",
    "affected,blank,context,default,edit,filter,footnote,header,label,new-id,new-uuid,news,",
    "override,project,recur,special,sync\n",
    "verbose=affected,blank,context,edit,header,footnote,label,new-id,news,project,special,sync,",
    "override,recur\n",
    "confirmation=1                                 # Confirmation on delete, big changes\n",
    "recurrence=1                                   # Enable recurrence\n",
    "recurrence.confirmation=prompt                 # Confirmation for propagating changes among ",
    "recurring tasks (yes/no/prompt)\n",
    "allow.empty.filter=1                           # An empty filter gets a warning and requires ",
    "confirmation\n",
    "annotation.info=1                              # Display annotations below the description ",
    "with info\n",
    "indent.annotation=2                            # Indent spaces for annotations\n",
    "indent.report=0                                # Indent spaces for whole report\n",
    "row.padding=0                                  # Left and right padding for each row of ",
    "report\n",
    "column.padding=1                               # Spaces between each column in a report\n",
    "bulk=3                                         # 3 or more tasks considered a bulk change and ",
    "is confirmed\n",
    "nag=You have more urgent tasks.                # Nag message to keep you honest\n",  // TODO
    "search.case.sensitive=1                        # Setting to no allows case insensitive ",
    "searches\n",
    "active.indicator=*                             # What to show as an active task indicator\n",
    "tag.indicator=+                                # What to show as a tag indicator\n",
    "dependency.indicator=D                         # What to show as a dependency indicator\n",
    "recurrence.indicator=R                         # What to show as a task recurrence indicator\n",
    "recurrence.limit=1                             # Number of future recurring pending tasks\n",
    "regex=1                                        # Assume all search/filter strings are ",
    "regexes\n",
    "xterm.title=0                                  # Sets xterm title for some commands\n",
    "expressions=infix                              # Prefer infix over postfix expressions\n",
    "json.array=1                                   # Enclose JSON output in [ ]\n",
    "abbreviation.minimum=2                         # Shortest allowed abbreviation\n",
    "news.version=                                  # Latest version highlights read by the user\n",
    "purge.on-sync=0                                # Purge old tasks on sync\n",
    "\n",
    "# Dates\n",
    "dateformat=Y-M-D                               # Preferred input and display date format\n",
    "dateformat.holiday=YMD                         # Preferred input date format for holidays\n",
    "dateformat.edit=Y-M-D H:N:S                    # Preferred display date format when editing\n",
    "dateformat.info=Y-M-D H:N:S                    # Preferred display date format for ",
    "information\n",
    "dateformat.report=                             # Preferred display date format for reports\n",
    "dateformat.annotation=                         # Preferred display date format for ",
    "annotations\n",
    "date.iso=1                                     # Enable ISO date support\n",
    "weekstart=sunday                               # Sunday or Monday only\n",
    "displayweeknumber=1                            # Show week numbers on calendar\n",
    "due=7                                          # Task is considered due in 7 days\n",
    "\n",
    "# Calendar controls\n",
    "calendar.legend=1                              # Display the legend on calendar\n",
    "calendar.details=sparse                        # Calendar shows information for tasks w/due ",
    "dates: full, sparse or none\n",
    "calendar.details.report=list                   # Report to use when showing task information ",
    "in cal\n",
    "calendar.offset=0                              # Apply an offset value to control the first ",
    "month of the calendar\n",
    "calendar.offset.value=-1                       # The number of months the first month of the ",
    "calendar is moved\n",
    "calendar.holidays=none                         # Show public holidays on calendar:full, ",
    "sparse or none\n",
    "#calendar.monthsperline=3                      # Number of calendar months on a line\n",
    "\n",
    "# Journal controls\n",
    "journal.time=0                                 # Record start/stop commands as annotation\n",
    "journal.time.start.annotation=Started task     # Annotation description for the start journal ",
    "entry\n",
    "journal.time.stop.annotation=Stopped task      # Annotation description for the stop  journal ",
    "entry\n",
    "journal.info=1                                 # Display task journal with info command\n",
    "\n",
    "# Dependency controls\n",
    "dependency.reminder=1                          # Nags on dependency chain violations\n",
    "dependency.confirmation=1                      # Should dependency chain repair be ",
    "confirmed?\n",
    "\n",
    "# Urgency Coefficients\n",
    "urgency.user.tag.next.coefficient=15.0         # Urgency coefficient for 'next' special tag\n",
    "urgency.due.coefficient=12.0                   # Urgency coefficient for due dates\n",
    "urgency.blocking.coefficient=8.0               # Urgency coefficient for blocking tasks\n",
    "urgency.active.coefficient=4.0                 # Urgency coefficient for active tasks\n",
    "urgency.scheduled.coefficient=5.0              # Urgency coefficient for scheduled tasks\n",
    "urgency.age.coefficient=2.0                    # Urgency coefficient for age\n",
    "urgency.annotations.coefficient=1.0            # Urgency coefficient for annotations\n",
    "urgency.tags.coefficient=1.0                   # Urgency coefficient for tags\n",
    "urgency.project.coefficient=1.0                # Urgency coefficient for projects\n",
    "urgency.blocked.coefficient=-5.0               # Urgency coefficient for blocked tasks\n",
    "urgency.waiting.coefficient=-3.0               # Urgency coefficient for waiting status\n",
    "urgency.inherit=0                              # Recursively inherit highest urgency value ",
    "from blocked tasks\n",
    "urgency.age.max=365                            # Maximum age in days\n",
    "\n",
    "#urgency.user.project.foo.coefficient=5.0      # Urgency coefficients for 'foo' project\n",
    "#urgency.user.tag.foo.coefficient=5.0          # Urgency coefficients for 'foo' tag\n",
    "#urgency.uda.foo.coefficient=5.0               # Urgency coefficients for UDA 'foo'\n",
    "\n",
    "# Color controls.\n",
    "color=1                                        # Enable color\n",
    // C++ `#ifdef TASK_TEST_RCDIR` (test builds only) is not taken; the `#else` branch follows.
    "include default.theme\n",
    "\n",
    "# Here is the rule precedence order, highest to lowest.\n",
    "# Note that these are just the color rule names, without the leading 'color.'\n",
    "#      and any trailing '.value'.\n",
    "rule.precedence.color=deleted,completed,active,keyword.,tag.,project.,overdue,scheduled,due.",
    "today,due,blocked,blocking,recurring,tagged,uda.\n",
    "\n",
    "# UDA priority\n",
    "uda.priority.type=string                       # UDA priority is a string type\n",
    "uda.priority.label=Priority                    # UDA priority has a display label'\n",
    "uda.priority.values=H,M,L,                     # UDA priority values are 'H', 'M', 'L' or ''\n",
    "                                               # UDA priority sorting is 'H' > 'M' > 'L' > '' ",
    "(highest to lowest)\n",
    "#uda.priority.default=M                        # UDA priority default value of 'M'\n",
    "urgency.uda.priority.H.coefficient=6.0         # UDA priority coefficient for value 'H'\n",
    "urgency.uda.priority.M.coefficient=3.9         # UDA priority coefficient for value 'M'\n",
    "urgency.uda.priority.L.coefficient=1.8         # UDA priority coefficient for value 'L'\n",
    "\n",
    "#default.project=foo                           # Default project for 'add' command\n",
    "#default.due=eom                               # Default due date for 'add' command\n",
    "#default.scheduled=eom                         # Default scheduled date for 'add' command\n",
    "default.command=next                           # When no arguments are specified\n",
    "default.timesheet.filter=( +PENDING and start.after:now-4wks ) or ( +COMPLETED and ",
    "end.after:now-4wks )\n",
    "\n",
    "_forcecolor=0                                  # Forces color to be on, even for non TTY ",
    "output\n",
    "complete.all.tags=0                            # Include old tag names in '_ags' command\n",
    "list.all.projects=0                            # Include old project names in 'projects' ",
    "command\n",
    "summary.all.projects=0                         # Include old project names in 'summary' ",
    "command\n",
    "list.all.tags=0                                # Include old tag names in 'tags' command\n",
    "print.empty.columns=0                          # Print columns which have no data for any ",
    "task\n",
    "debug=0                                        # Display diagnostics\n",
    "sugar=1                                        # Syntactic sugar\n",
    "obfuscate=0                                    # Obfuscate data for error reporting\n",
    "fontunderline=1                                # Uses underlines rather than -------\n",
    "\n",
    "# WARNING: Please read the documentation (man task-sync) before setting up\n",
    "#          Taskwarrior for Taskserver synchronization.\n",
    "\n",
    "#sync.encryption_secret                        # Encryption secret for sync to a server\n",
    "#sync.server.client_id                         # Client ID for sync to a server\n",
    "#sync.server.url                               # URL of the sync server\n",
    "#sync.local.server_dir                         # Directory for local sync\n",
    "#sync.aws.region                               # region for AWS sync\n",
    "#sync.aws.bucket                               # bucket for AWS sync\n",
    "#sync.aws.access_key_id                        # access_key_id for AWS sync\n",
    "#sync.aws.secret_access_key                    # secret_access_key for AWS sync\n",
    "#sync.aws.profile                              # profile name for AWS sync\n",
    "#sync.aws.default_credentials                  # use default credentials for AWS sync\n",
    "#sync.aws.endpoint_url                         # endpoint URL for S3-compatible sync\n",
    "#sync.aws.force_path_style=0                   # use path-style S3 URLs\n",
    "#sync.gcp.credential_path                      # Path to JSON file containing credentials to ",
    "authenticate GCP Sync\n",
    "#sync.gcp.bucket                               # Bucket for sync to GCP\n",
    "\n",
    "# Aliases - alternate names for commands\n",
    "alias.rm=delete                                # Alias for the delete command\n",
    "alias.history=history.monthly                  # Prefer monthly over annual history reports\n",
    "alias.ghistory=ghistory.monthly                # Prefer monthly graphical over annual history ",
    "reports\n",
    "alias.burndown=burndown.weekly                 # Prefer the weekly burndown chart\n",
    "\n",
    "# Reports\n",
    "\n",
    "report.long.description=All details of tasks\n",
    "report.long.labels=ID,A,Created,Mod,Deps,P,Project,Tags,Recur,Wait,Sched,Due,Until,",
    "Description\n",
    "report.long.columns=id,start.active,entry,modified.age,depends,priority,project,tags,recur,",
    "wait.remaining,scheduled,due,until,description\n",
    "report.long.filter=+PENDING -WAITING\n",
    "report.long.sort=modified-\n",
    "report.long.context=1\n",
    "\n",
    "report.list.description=Most details of tasks\n",
    "report.list.labels=ID,Active,Age,D,P,Project,Tags,R,Sch,Due,Until,Description,Urg\n",
    "report.list.columns=id,start.age,entry.age,depends.indicator,priority,project,tags,recur.",
    "indicator,scheduled.countdown,due,until.remaining,description.count,urgency\n",
    "report.list.filter=+PENDING -WAITING\n",
    "report.list.sort=start-,due+,project+,urgency-\n",
    "report.list.context=1\n",
    "\n",
    "report.ls.description=Few details of tasks\n",
    "report.ls.labels=ID,A,D,Project,Tags,R,Wait,S,Due,Until,Description\n",
    "report.ls.columns=id,start.active,depends.indicator,project,tags,recur.indicator,wait.",
    "remaining,scheduled.countdown,due.countdown,until.countdown,description.count\n",
    "report.ls.filter=+PENDING -WAITING\n",
    "report.ls.sort=start-,description+\n",
    "report.ls.context=1\n",
    "\n",
    "report.minimal.description=Minimal details of tasks\n",
    "report.minimal.labels=ID,Project,Tags,Description\n",
    "report.minimal.columns=id,project,tags.count,description.count\n",
    "report.minimal.filter=+PENDING -WAITING\n",
    "report.minimal.sort=project+/,description+\n",
    "report.minimal.context=1\n",
    "\n",
    "report.newest.description=Newest tasks\n",
    "report.newest.labels=ID,Active,Created,Age,Mod,D,P,Project,Tags,R,Wait,Sch,Due,Until,",
    "Description\n",
    "report.newest.columns=id,start.age,entry,entry.age,modified.age,depends.indicator,priority,",
    "project,tags,recur.indicator,wait.remaining,scheduled.countdown,due,until.age,description\n",
    "report.newest.filter=+PENDING -WAITING\n",
    "report.newest.sort=entry-\n",
    "report.newest.context=1\n",
    "\n",
    "report.oldest.description=Oldest tasks\n",
    "report.oldest.labels=ID,Active,Created,Age,Mod,D,P,Project,Tags,R,Wait,Sch,Due,Until,",
    "Description\n",
    "report.oldest.columns=id,start.age,entry,entry.age,modified.age,depends.indicator,priority,",
    "project,tags,recur.indicator,wait.remaining,scheduled.countdown,due,until.age,description\n",
    "report.oldest.filter=+PENDING -WAITING\n",
    "report.oldest.sort=entry+\n",
    "report.oldest.context=1\n",
    "\n",
    "report.overdue.description=Overdue tasks\n",
    "report.overdue.labels=ID,Active,Age,Deps,P,Project,Tag,R,S,Due,Until,Description,Urg\n",
    "report.overdue.columns=id,start.age,entry.age,depends,priority,project,tags,recur.indicator,",
    "scheduled.countdown,due,until,description,urgency\n",
    "report.overdue.filter=+PENDING -WAITING +OVERDUE\n",
    "report.overdue.sort=urgency-,due+\n",
    "report.overdue.context=1\n",
    "\n",
    "report.active.description=Active tasks\n",
    "report.active.labels=ID,Started,Active,Age,D,P,Project,Tags,Recur,W,Sch,Due,Until,",
    "Description\n",
    "report.active.columns=id,start,start.age,entry.age,depends.indicator,priority,project,tags,",
    "recur,wait,scheduled.remaining,due,until,description\n",
    "report.active.filter=+PENDING -WAITING +ACTIVE\n",
    "report.active.sort=project+,start+\n",
    "report.active.context=1\n",
    "\n",
    "report.completed.description=Completed tasks\n",
    "report.completed.labels=ID,UUID,Created,Completed,Age,Deps,P,Project,Tags,R,Due,Description\n",
    "report.completed.columns=id,uuid.short,entry,end,entry.age,depends,priority,project,tags,",
    "recur.indicator,due,description\n",
    "report.completed.filter=status:completed -WAITING \n",
    "report.completed.sort=end+\n",
    "report.completed.context=1\n",
    "\n",
    "report.recurring.description=Recurring Tasks\n",
    "report.recurring.labels=ID,Active,Age,D,P,Parent,Project,Tags,Recur,Sch,Due,Until,Description,",
    "Urg\n",
    "report.recurring.columns=id,start.age,entry.age,depends.indicator,priority,parent.short,",
    "project,tags,recur,scheduled.countdown,due,until.remaining,description,urgency\n",
    "report.recurring.filter=(+PENDING -WAITING +CHILD) or (status:recurring -WAITING ",
    "+PARENT)\n",
    "report.recurring.sort=due+,urgency-,entry+\n",
    "report.recurring.context=1\n",
    "\n",
    "report.waiting.description=Waiting (hidden) tasks\n",
    "report.waiting.labels=ID,A,Age,D,P,Project,Tags,R,Wait,Remaining,Sched,Due,Until,Description\n",
    "report.waiting.columns=id,start.active,entry.age,depends.indicator,priority,project,tags,",
    "recur.indicator,wait,wait.remaining,scheduled,due,until,description\n",
    "report.waiting.filter=+WAITING\n",
    "report.waiting.sort=due+,wait+,entry+\n",
    "report.waiting.context=1\n",
    "\n",
    "report.all.description=All tasks\n",
    "report.all.labels=ID,St,UUID,A,Age,Done,D,P,Project,Tags,R,Wait,Sch,Due,Until,Description\n",
    "report.all.columns=id,status.short,uuid.short,start.active,entry.age,end.age,depends.",
    "indicator,priority,project.parent,tags.count,recur.indicator,wait.remaining,scheduled.",
    "remaining,due,until.remaining,description\n",
    "report.all.sort=entry-\n",
    "report.all.context=1\n",
    "\n",
    "report.next.description=Most urgent tasks\n",
    "report.next.labels=ID,Active,Age,Deps,P,Project,Tag,Recur,S,Due,Until,Description,Urg\n",
    "report.next.columns=id,start.age,entry.age,depends,priority,project,tags,recur,scheduled.",
    "countdown,due.relative,until.remaining,description,urgency\n",
    "report.next.filter=+PENDING -WAITING limit:page\n",
    "report.next.sort=urgency-\n",
    "report.next.context=1\n",
    "\n",
    "report.ready.description=Most urgent actionable tasks\n",
    "report.ready.labels=ID,Active,Age,D,P,Project,Tags,R,S,Due,Until,Description,Urg\n",
    "report.ready.columns=id,start.age,entry.age,depends.indicator,priority,project,tags,recur.",
    "indicator,scheduled.countdown,due.countdown,until.remaining,description,urgency\n",
    "report.ready.filter=+READY\n",
    "report.ready.sort=start-,urgency-\n",
    "report.ready.context=1\n",
    "\n",
    "report.blocked.description=Blocked tasks\n",
    "report.blocked.columns=id,depends,project,priority,due,start.active,entry.age,description\n",
    "report.blocked.labels=ID,Deps,Proj,Pri,Due,Active,Age,Description\n",
    "report.blocked.sort=due+,priority-,start-,project+\n",
    "report.blocked.filter=+PENDING -WAITING +BLOCKED\n",
    "report.blocked.context=1\n",
    "\n",
    "report.unblocked.description=Unblocked tasks\n",
    "report.unblocked.columns=id,depends,project,priority,due,start.active,entry.age,description\n",
    "report.unblocked.labels=ID,Deps,Proj,Pri,Due,Active,Age,Description\n",
    "report.unblocked.sort=due+,priority-,start-,project+\n",
    "report.unblocked.filter=+PENDING -WAITING -BLOCKED\n",
    "report.unblocked.context=1\n",
    "\n",
    "report.blocking.description=Blocking tasks\n",
    "report.blocking.labels=ID,UUID,A,Deps,Project,Tags,R,W,Sch,Due,Until,Description,Urg\n",
    "report.blocking.columns=id,uuid.short,start.active,depends,project,tags,recur,wait,scheduled.",
    "remaining,due.relative,until.remaining,description.count,urgency\n",
    "report.blocking.sort=urgency-,due+,entry+\n",
    "report.blocking.filter=+PENDING -WAITING +BLOCKING\n",
    "report.blocking.context=1\n",
    "\n",
    "report.timesheet.filter=(+PENDING -WAITING start.after:now-4wks) or (+COMPLETED -WAITING ",
    "end.after:now-4wks)\n",
    "report.timesheet.context=0\n",
    "\n",
);

/// C++ `static const char* modifierNames[]`: supported modifiers, synonyms on
/// the same line.
pub const MODIFIER_NAMES: [&str; 22] = [
    "before", "under", "below", "after", "over", "above", "by", "none", //
    "any", "is", "equals", "isnt", "not", "has", "contains", "hasnt", //
    "startswith", "left", "endswith", "right", "word", "noword",
];

/// The commit field of the `rc.debug` Perf line: C++ `COMMIT` with
/// `HAVE_COMMIT`, otherwise `-`.
const PERF_COMMIT: &str = match COMMIT {
    Some(commit) => commit,
    None => "-",
};

////////////////////////////////////////////////////////////////////////////////
/// The include search path the C++ gets from the `TASK_RCDIR` macro (see the
/// module documentation for how it is resolved).
pub fn task_rcdir() -> String {
    if let Some(dir) = option_env!("TASK_RCDIR") {
        return dir.to_string();
    }

    let exe = std::env::current_exe().ok();
    let exe_dir = exe.as_ref().and_then(|e| e.parent());
    if let Some(exe_dir) = exe_dir {
        for ancestor in exe_dir.ancestors() {
            for candidate in [ancestor.join("share/doc/task/rc"), ancestor.join("doc/rc")] {
                if candidate.join("default.theme").is_file() {
                    return candidate.to_string_lossy().into_owned();
                }
            }
        }
        return exe_dir.join("../share/doc/task/rc").to_string_lossy().into_owned();
    }

    "share/doc/task/rc".to_string()
}

#[cfg(test)]
thread_local! {
    /// Test-only override of `isatty (STDOUT_FILENO)` for this thread.
    static STDOUT_TTY_OVERRIDE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Test-only: make [`stdout_is_tty`] report `value` on this thread (`None`
/// restores the real check).
#[cfg(test)]
pub(crate) fn set_stdout_tty_override(value: Option<bool>) {
    STDOUT_TTY_OVERRIDE.with(|o| o.set(value));
}

#[cfg(test)]
fn stdout_tty_override() -> Option<bool> {
    STDOUT_TTY_OVERRIDE.with(|o| o.get())
}

#[cfg(not(test))]
fn stdout_tty_override() -> Option<bool> {
    None
}

/// C++ `isatty (STDOUT_FILENO)`.
pub fn stdout_is_tty() -> bool {
    if let Some(value) = stdout_tty_override() {
        return value;
    }

    // SAFETY: isatty only inspects the descriptor.
    unsafe { libc::isatty(libc::STDOUT_FILENO) == 1 }
}

/// C++ `(int) strtol (text, nullptr, 10)`: optional leading whitespace and
/// sign, then the leading decimal digits; 0 if none.
pub(crate) fn strtol(text: &str) -> i32 {
    let text = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let mut value: i64 = 0;
    for b in digits.bytes().take_while(u8::is_ascii_digit) {
        value = value.saturating_mul(10).saturating_add(i64::from(b - b'0'));
    }
    let value = if negative { -value } else { value };
    value as i32
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class Context`.
pub struct Context {
    /// C++ `cli2`.
    pub cli2: Cli2,
    /// C++ `home_dir` (`$HOME`).
    pub home_dir: String,
    /// C++ `rc_file` (default `~/.taskrc`).
    pub rc_file: File,
    /// C++ `data_dir` (a `Path`, default `~/.task`).
    pub data_dir: Path,
    /// C++ `config`.
    pub config: Configuration,
    /// The compiled-in defaults alone, parsed the way C++ `CmdShow` parses
    /// them (`parse (configurationDefaults, 0, {TASK_RCDIR})`); filled by
    /// [`initialize`](Self::initialize). Foundation DD2.
    pub defaults: Configuration,
    /// C++ `tdb2`.
    pub tdb2: Tdb2,
    /// C++ `hooks`.
    pub hooks: Hooks,
    /// C++ `determine_color_use` (default `true`).
    pub determine_color_use: bool,
    /// C++ `use_color` (default `true`).
    pub use_color: bool,
    /// C++ `verbosity_legacy`.
    pub verbosity_legacy: bool,
    /// C++ `verbosity`.
    pub verbosity: BTreeSet<String>,
    /// C++ `headers`.
    pub headers: Vec<String>,
    /// C++ `footnotes`.
    pub footnotes: Vec<String>,
    /// C++ `errors`.
    pub errors: Vec<String>,
    /// C++ `debugMessages`.
    pub debug_messages: Vec<String>,
    /// C++ `commands`.
    pub commands: Registry,
    /// C++ `columns` (filled by `Column::factory`, [`column::factory_all`]).
    pub columns: ColumnRegistry,
    /// C++ `terminal_width`.
    pub terminal_width: i32,
    /// C++ `terminal_height`.
    pub terminal_height: i32,
    /// C++ `timer_total`.
    pub timer_total: Timer,
    /// C++ `time_total_us`.
    pub time_total_us: i64,
    /// C++ `time_init_us`.
    pub time_init_us: i64,
    /// C++ `time_load_us`.
    pub time_load_us: i64,
    /// C++ `time_gc_us`.
    pub time_gc_us: i64,
    /// C++ `time_filter_us`.
    pub time_filter_us: i64,
    /// C++ `time_commit_us`.
    pub time_commit_us: i64,
    /// C++ `time_sort_us`.
    pub time_sort_us: i64,
    /// C++ `time_render_us`.
    pub time_render_us: i64,
    /// C++ `time_hooks_us`.
    pub time_hooks_us: i64,
    /// C++ `currentTask`: the current task for DOM references, or `None`.
    pub current_task: Option<Rc<Task>>,
}

impl Default for Context {
    fn default() -> Self {
        Context::new()
    }
}

impl Context {
    /// C++ `Context::Context` (member initialisers). Note that, as in C++,
    /// `~/.taskrc` and `~/.task` are expanded with `$HOME` at construction.
    pub fn new() -> Self {
        Context {
            cli2: Cli2::new(),
            home_dir: String::new(),
            rc_file: File::new("~/.taskrc"),
            data_dir: Path::new("~/.task"),
            config: Configuration::new(),
            defaults: Configuration::new(),
            tdb2: Tdb2::new(),
            hooks: Hooks::new(),
            determine_color_use: true,
            use_color: true,
            verbosity_legacy: false,
            verbosity: BTreeSet::new(),
            headers: Vec::new(),
            footnotes: Vec::new(),
            errors: Vec::new(),
            debug_messages: Vec::new(),
            commands: Registry::new(),
            columns: ColumnRegistry::new(),
            terminal_width: 0,
            terminal_height: 0,
            timer_total: Timer::new(),
            time_total_us: 0,
            time_init_us: 0,
            time_load_us: 0,
            time_gc_us: 0,
            time_filter_us: 0,
            time_commit_us: 0,
            time_sort_us: 0,
            time_render_us: 0,
            time_hooks_us: 0,
            current_task: None,
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::initialize (argc, argv)`: all startup. `args` is the full
    /// argv, including `argv[0]`.
    ///
    /// Returns the C++ status: 0, 2 (a message error, already queued and
    /// printed), 3 (unknown error) or 4 (a hook/`throw int`). On failure the
    /// debug messages (with `rc.debug`), headers, footnotes and errors are
    /// written to stderr, as in C++. The `Result` exists for errors C++ would
    /// let escape to `main`; there are none today.
    pub fn initialize(&mut self, args: &[String]) -> Result<i32> {
        let mut stdout = std::io::stdout();
        let mut stderr = std::io::stderr();
        self.initialize_with(args, &mut stdout, &mut stderr)
    }

    /// [`initialize`](Self::initialize) writing to the given streams.
    pub fn initialize_with(&mut self, args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> Result<i32> {
        self.timer_total.start();
        let mut rc = 0;
        // Deviation: C++ `home_dir = getenv ("HOME")` is undefined when HOME is
        // unset; here it is empty.
        self.home_dir = std::env::var_os("HOME")
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_default();

        let search_paths = vec![task_rcdir()];

        if let Err(err) = self.initialize_steps(args, &search_paths) {
            rc = self.status_for_error(err);
        }

        // On initialization failure...
        if rc != 0 {
            self.emit_messages(None, false, stdout, stderr);
        }

        self.time_init_us += self.timer_total.total_us() as i64;
        Ok(rc)
    }

    /// The body of the C++ `initialize` `try` block.
    fn initialize_steps(&mut self, args: &[String], search_paths: &[String]) -> Result<()> {
        ////////////////////////////////////////////////////////////////////////
        //
        // [1] Load the correct config file.
        //     - Default to ~/.taskrc (ctor).
        //     - If no ~/.taskrc, use $XDG_CONFIG_HOME/task/taskrc if exists, or
        //       ~/.config/task/taskrc if $XDG_CONFIG_HOME is unset
        //     - Allow $TASKRC override.
        //     - Allow command line override rc:<file>
        //     - Load resultant file.
        //     - Apply command line overrides to the config.
        //
        ////////////////////////////////////////////////////////////////////////

        let mut taskrc_overridden = false;

        // XDG_CONFIG_HOME doesn't count as an override (no warning header)
        if !self.rc_file.exists() {
            // Use XDG_CONFIG_HOME if defined, otherwise default to ~/.config
            let mut xdg_config_home = match std::env::var_os("XDG_CONFIG_HOME") {
                Some(env) => env.to_string_lossy().into_owned(),
                None => format("{1}/.config", &[&self.home_dir]),
            };

            // Ensure the path does not end with '/'
            if xdg_config_home.ends_with('/') {
                xdg_config_home.pop();
            }

            // https://github.com/GothenburgBitFactory/libshared/issues/32
            let rcfile_path = format("{1}/task/taskrc", &[&xdg_config_home]);

            let maybe_rc_file = File::new(&rcfile_path);
            if maybe_rc_file.exists() {
                self.rc_file = maybe_rc_file;
            }
        }

        if let Some(value) = std::env::var_os("TASKRC") {
            self.rc_file = File::new(&value.to_string_lossy());
            taskrc_overridden = true;
        }

        taskrc_overridden = cli2::get_override(args, &mut self.rc_file) || taskrc_overridden;

        // Artificial scope for timing purposes.
        {
            let timer = Timer::new();
            self.config.parse(CONFIGURATION_DEFAULTS, 1, search_paths, "")?;
            // Foundation DD2: the defaults snapshot, parsed as CmdShow does.
            self.defaults = Configuration::new();
            self.defaults.parse(CONFIGURATION_DEFAULTS, 0, search_paths, "")?;
            let rc_path = self.rc_file.data.clone();
            self.config.load(&rc_path, 1, search_paths)?;
            self.debug_timing(&format("Config::load ({1})", &[&rc_path]), &timer);
        }

        cli2::apply_overrides(args, self);

        if taskrc_overridden && self.verbose("override") {
            let message = format("TASKRC override: {1}", &[&self.rc_file.data]);
            self.header(&message);
        }

        ////////////////////////////////////////////////////////////////////////
        //
        // [2] Locate the data directory.
        //     - Default to ~/.task (ctor).
        //     - Allow $TASKDATA override.
        //     - Allow command line override rc.data.location:<dir>
        //     - Inform TDB2 where to find data.
        //     - Create the rc_file and data_dir, if necessary.
        //
        ////////////////////////////////////////////////////////////////////////

        let mut taskdata_overridden = false;

        if let Some(value) = std::env::var_os("TASKDATA") {
            // C++ `data_dir = Directory (override)`.
            self.data_dir = Path::new(&value.to_string_lossy());
            let location = self.data_dir.data.clone();
            self.config.set("data.location", &location);
            taskdata_overridden = true;
        }

        taskdata_overridden =
            cli2::get_data_location(args, &mut self.data_dir, &self.config) || taskdata_overridden;

        if taskdata_overridden && self.verbose("override") {
            let message = format("TASKDATA override: {1}", &[&self.data_dir.data]);
            self.header(&message);
        }

        self.create_default_config(search_paths)?;

        ////////////////////////////////////////////////////////////////////////
        //
        // [3] Instantiate Command objects and capture command entities.
        //
        ////////////////////////////////////////////////////////////////////////

        command::factory(&mut self.commands, &self.config)?;
        for keyword in self.commands.keys() {
            self.cli2.entity("cmd", keyword);
        }

        ////////////////////////////////////////////////////////////////////////
        //
        // [4] Instantiate Column objects and capture column entities.
        //
        ////////////////////////////////////////////////////////////////////////

        column::factory_all(&mut self.columns, &self.config)?;
        for name in self.columns.keys() {
            self.cli2.entity("attribute", name);
        }

        self.cli2.entity("pseudo", "limit");

        ////////////////////////////////////////////////////////////////////////
        //
        // [5] Capture modifier and operator entities.
        //
        ////////////////////////////////////////////////////////////////////////

        for modifier_name in MODIFIER_NAMES {
            self.cli2.entity("modifier", modifier_name);
        }

        for op in Eval::get_operators() {
            self.cli2.entity("operator", &op);
        }

        for op in Eval::get_binary_operators() {
            self.cli2.entity("binary_operator", &op);
        }

        ////////////////////////////////////////////////////////////////////////
        //
        // [6] Complete the Context initialization.
        //
        ////////////////////////////////////////////////////////////////////////

        rules::initialize_color_rules(self)?;
        self.static_initialization()?;
        self.propagate_debug();
        self.load_aliases();

        ////////////////////////////////////////////////////////////////////////
        //
        // [7] Parse the command line.
        //
        ////////////////////////////////////////////////////////////////////////

        for arg in args {
            self.cli2.add(arg);
        }

        cli2::with_cli2(self, |cli2, ctx| cli2.analyze(ctx))?;

        // Extract a recomposed command line.
        let mut found_default = false;
        let mut found_assumed = false;
        let mut combined = String::new();
        for a in &self.cli2.args {
            if !combined.is_empty() {
                combined.push(' ');
            }

            combined.push_str(&a.attribute("raw"));

            if a.has_tag("DEFAULT") {
                found_default = true;
            }

            if a.has_tag("ASSUMED") {
                found_assumed = true;
            }
        }

        if self.verbose("default") {
            if found_default {
                self.header(&format!("[{combined}]"));
            }

            if found_assumed {
                self.header("No command specified - assuming 'information'.");
            }
        }

        ////////////////////////////////////////////////////////////////////////
        //
        // [7.5] Open the Replica.
        //
        ////////////////////////////////////////////////////////////////////////

        let create_if_missing = !self.config.get_boolean("exit.on.missing.db");
        if let Some(c) = self.commands.get(&self.cli2.get_command()).cloned() {
            // We must allow writes if either 'gc' is enabled and the command performs GC, or the command
            // itself is read-write.
            let read_write =
                (self.config.get_boolean("gc") && (c.needs_gc() || c.needs_recur_update())) || !c.read_only();
            self.tdb2.open_replica(&self.data_dir, create_if_missing, read_write)?;
        }

        ////////////////////////////////////////////////////////////////////////
        //
        // [8] Initialize hooks.
        //
        ////////////////////////////////////////////////////////////////////////

        Hooks::initialize(self)?;

        Ok(())
    }

    /// The C++ `catch` clauses of `initialize` and `run`: queues the message
    /// and returns the status. `throw std::string` and `rust::Error` give 2,
    /// `throw int` gives 4, `catch (...)` (`std::runtime_error`, i.e.
    /// [`TwError::Runtime`]) queues "Unknown error. Please report." and gives 3.
    ///
    /// Rust-only variants: `NotImplemented` (a stub command) is a message
    /// error like `throw std::string`; `Io` is reported like the libshared
    /// I/O failures, which throw strings.
    fn status_for_error(&mut self, err: TwError) -> i32 {
        match err {
            TwError::Status(_) => {
                // Hooks can terminate processing by throwing integers.
                4
            }
            TwError::Message(_) | TwError::TaskChampion(_) | TwError::NotImplemented(_) | TwError::Io(_) => {
                self.error(&err.to_string());
                2
            }
            // catch (...): e.g. the std::runtime_error of an invalid regex.
            TwError::Runtime(_) => {
                self.error("Unknown error. Please report.");
                3
            }
        }
    }

    /// Writes the queued messages, as the end of C++ `initialize` (on failure)
    /// and `run` do: debug messages (with `rc.debug`), headers (`header`
    /// verbosity), then `output` to stdout (run only), footnotes (`footnote`
    /// verbosity) and errors (always). `run` colours errors with
    /// `colorizeError`, `initialize` with `colorizeFootnote`.
    ///
    /// stdout is flushed before each stderr write, as `std::cerr` is tied to
    /// `std::cout`.
    fn emit_messages(&mut self, output: Option<&str>, run: bool, stdout: &mut dyn Write, stderr: &mut dyn Write) {
        let mut to_stderr = |stdout: &mut dyn Write, line: String| {
            let _ = stdout.flush();
            let _ = writeln!(stderr, "{line}");
        };

        // Dump all debug messages, controlled by rc.debug.
        if self.config.get_boolean("debug") {
            for d in self.debug_messages.clone() {
                let line = if self.color() { colorize_debug(&d) } else { d };
                to_stderr(stdout, line);
            }
        }

        // Dump all headers, controlled by 'header' verbosity token.
        if self.verbose("header") {
            for h in self.headers.clone() {
                let line = if self.color() { colorize_header(&h) } else { h };
                to_stderr(stdout, line);
            }
        }

        // Dump the report output.
        if let Some(output) = output {
            let _ = stdout.write_all(output.as_bytes());
        }

        // Dump all footnotes, controlled by 'footnote' verbosity token.
        if self.verbose("footnote") {
            for f in self.footnotes.clone() {
                let line = if self.color() { colorize_footnote(&f) } else { f };
                to_stderr(stdout, line);
            }
        }

        // Dump all errors, non-maskable.
        // Colorized as footnotes (initialize) / errors (run).
        for e in self.errors.clone() {
            let line = if !self.color() {
                e
            } else if run {
                colorize_error(&e)
            } else {
                colorize_footnote(&e)
            };
            to_stderr(stdout, line);
        }

        let _ = stdout.flush();
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::run`: dispatches the command, then prints debug
    /// messages, headers, the command output, footnotes and errors. Returns
    /// the exit status.
    pub fn run(&mut self) -> i32 {
        let mut stdout = std::io::stdout();
        let mut stderr = std::io::stderr();
        self.run_with(&mut stdout, &mut stderr)
    }

    /// [`run`](Self::run) writing to the given streams.
    pub fn run_with(&mut self, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
        let mut output = String::new();

        let rc = match self.run_steps(&mut output) {
            Ok(rc) => rc,
            Err(err) => self.status_for_error(err),
        };

        self.emit_messages(Some(&output), true, stdout, stderr);
        rc
    }

    /// The body of the C++ `run` `try` block.
    fn run_steps(&mut self, output: &mut String) -> Result<i32> {
        Hooks::on_launch(self)?;
        let rc = self.dispatch(output)?;
        Hooks::on_exit(self)?; // No chance to update data.

        self.timer_total.stop();
        self.time_total_us += self.timer_total.total_us() as i64;

        // C++ `TDB2::all_tasks` / `pending_tasks` add their load time straight
        // to `Context::time_load_us`; Tdb2 accumulates it separately.
        self.time_load_us += std::mem::take(&mut self.tdb2.time_load_us);

        let gc = if self.time_gc_us > 0 {
            self.time_gc_us - self.time_load_us
        } else {
            self.time_gc_us
        };
        let other = self.time_total_us
            - self.time_init_us
            - self.time_gc_us
            - self.time_filter_us
            - self.time_commit_us
            - self.time_sort_us
            - self.time_render_us
            - self.time_hooks_us;
        let s = format!(
            "Perf {PACKAGE_STRING} {PERF_COMMIT} {} init:{} load:{} gc:{} filter:{} commit:{} sort:{} render:{} hooks:{} other:{} total:{}\n",
            Datetime::new().to_iso(),
            self.time_init_us,
            self.time_load_us,
            gc,
            self.time_filter_us,
            self.time_commit_us,
            self.time_sort_us,
            self.time_render_us,
            self.time_hooks_us,
            other,
            self.time_total_us
        );
        self.debug(&s);

        Ok(rc)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::dispatch`: runs the command found by the CLI parser
    /// (`help` if there is none), appending its report to `out`.
    ///
    /// The command's `Rc` handle is cloned out of the registry before
    /// `execute` borrows the Context mutably (foundation DD1).
    pub fn dispatch(&mut self, out: &mut String) -> Result<i32> {
        // Autocomplete args against keywords.
        let command = self.cli2.get_command();
        if !command.is_empty() {
            self.update_xterm_title();
            self.update_verbosity();

            let c = self
                .commands
                .get(&command)
                .cloned()
                .ok_or_else(|| TwError::NotImplemented(command.clone()))?;

            // The command know whether they need a GC or recurrence update.
            if c.needs_gc() {
                self.time_gc_us += self.tdb2.gc(self.config.get_boolean("gc"))?;
            }

            // This is something that is only needed for write commands with no other
            // filter processing.
            if c.accepts_modifications() && !c.accepts_filter() {
                cli2::with_cli2(self, |cli2, ctx| cli2.prepare_filter(ctx))?;
            }

            // With rc.debug.parser == 2, there are more tree dumps than you might want,
            // but we need the rc.debug.parser == 1 case covered also, with the final
            // tree.
            if self.config.get_boolean("debug") && self.config.get_integer("debug.parser") == 1 {
                // The C++ title contains U+0456 (Cyrillic i) in "specifіc".
                let dump = self.cli2.dump("Parse Tree (before command-specif\u{456}c processing)");
                self.debug(&dump);
            }

            if c.needs_recur_update() && self.config.get_boolean("gc") {
                // Milestone 7 recurrence (`recur.cpp`): no-ops for now.
                recur::handle_until(self)?;
                recur::handle_recurrence(self)?;
            }

            return c.execute(self, out);
        }

        let help = self
            .commands
            .get("help")
            .cloned()
            .ok_or_else(|| TwError::NotImplemented("help".to_string()))?;
        help.execute(self, out)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Queries `TIOCGWINSZ` on stdout into `terminal_height`/`terminal_width`
    /// if both are still 0.
    fn detect_terminal_size(&mut self) {
        if self.terminal_width == 0 && self.terminal_height == 0 {
            // SAFETY: TIOCGWINSZ fills a `winsize`; a zeroed one is valid.
            unsafe {
                let mut buff: libc::winsize = std::mem::zeroed();
                if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut buff) != -1 {
                    self.terminal_height = i32::from(buff.ws_row);
                    self.terminal_width = i32::from(buff.ws_col);
                }
            }
        }
    }

    /// C++ `Context::getWidth`: the terminal width (`defaultwidth` without
    /// `detection`; 65536 for `defaultwidth=0`).
    pub fn get_width(&mut self) -> i32 {
        // Determine window size.
        let mut width = self.config.get_integer("defaultwidth");

        // A zero width value means 'infinity', which is approximated here by 2^16.
        if width == 0 {
            return 65536;
        }

        if self.config.get_boolean("detection") {
            self.detect_terminal_size();

            width = self.terminal_width;

            // Ncurses does this, and perhaps we need to as well, to avoid a problem on
            // Cygwin where the display goes right up to the terminal width, and causes
            // an odd color wrapping problem.
            if self.config.get_boolean("avoidlastcolumn") {
                width -= 1;
            }
        }

        width
    }

    /// C++ `Context::getHeight`: the terminal height (`defaultheight`
    /// without `detection`; 65536 for `defaultheight=0`).
    pub fn get_height(&mut self) -> i32 {
        // Determine window size.
        let mut height = self.config.get_integer("defaultheight");

        // A zero height value means 'infinity', which is approximated here by 2^16.
        if height == 0 {
            return 65536;
        }

        if self.config.get_boolean("detection") {
            self.detect_terminal_size();

            height = self.terminal_height;
        }

        height
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::getTaskContext (kind, name, fallback = true)`: the
    /// `context.<name>.<kind>` filter (`name` defaults to the active
    /// `context`), falling back to the old-style `context.<name>` for `read`.
    pub fn get_task_context(&mut self, kind: &str, name: &str, fallback: bool) -> String {
        // Consider currently selected context, if none specified
        let name = if name.is_empty() {
            self.config.get("context")
        } else {
            name.to_string()
        };

        // Detect if any context is set, and bail out if not
        if !name.is_empty() {
            self.debug(&format("Applying context '{1}'", &[&name]));
        } else {
            self.debug("No context set");
            return String::new();
        }

        // Figure out the context string for this kind (read/write)
        let mut context_string = String::new();

        if !self.config.has(&format!("context.{name}.{kind}")) && kind == "read" {
            self.debug(&format!("Specific {kind} context for '{name}' not defined. "));
            if fallback {
                self.debug("Trying to interpret old-style context definition as read context.");
                context_string = self.config.get(&format!("context.{name}"));
            }
        } else {
            context_string = self.config.get(&format!("context.{name}.{kind}"));
        }

        let shown = if context_string.is_empty() {
            "(empty)".to_string()
        } else {
            context_string.clone()
        };
        self.debug(&format("Detected context string: {1}", &[&shown]));
        context_string
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::color`: whether to use colour (TTY or `_forcecolor`),
    /// decided once. When stdout is not a TTY this also turns `detection` and
    /// `color` off in the configuration, as C++ does.
    pub fn color(&mut self) -> bool {
        if self.determine_color_use {
            // What the config says.
            self.use_color = self.config.get_boolean("color");

            // Only tty's support color.
            if !stdout_is_tty() {
                // No ioctl.
                self.config.set("detection", "off");
                self.config.set("color", "off");

                // Files don't get color.
                self.use_color = false;
            }

            // Override.
            if self.config.get_boolean("_forcecolor") {
                self.config.set("color", "on");
                self.use_color = true;
            }

            // No need to go through this again.
            self.determine_color_use = false;
        }

        // Cached result.
        self.use_color
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::verbose`: verbosity control.
    ///
    /// - `rc.verbose=1` (any true value): show all feedback.
    /// - `rc.verbose=0`: show regular feedback (`blank,label,new-id,edit`).
    /// - `rc.verbose=nothing`: show the absolute minimum.
    /// - `rc.verbose=one,two`: show verbosity for `one` and `two` only.
    ///
    /// The token set is computed on first use and cached in `verbosity`.
    pub fn verbose(&mut self, token: &str) -> bool {
        if self.verbosity.is_empty() {
            self.verbosity_legacy = self.config.get_boolean("verbose");
            for token in split(&self.config.get("verbose"), ',') {
                self.verbosity.insert(token);
            }

            // Regular feedback means almost everything.
            // This odd test is to see if a Boolean-false value is a real one, which
            // means it is not 1/true/T/yes/on, but also should not be one of the
            // valid tokens either.
            if !self.verbosity_legacy && !self.verbosity.is_empty() {
                let v = self.verbosity.iter().next().cloned().unwrap_or_default();
                // This list must be complete.
                const VALID: [&str; 17] = [
                    "nothing", "affected", "blank", "context", "default", "edit", "filter", "footnote", "header",
                    "label", "new-id", "new-uuid", "news", "override", "project", "recur", "special",
                ];
                if !VALID.contains(&v.as_str()) && v != "sync" {
                    // This list emulates rc.verbose=off in version 1.9.4.
                    self.verbosity = ["blank", "label", "new-id", "edit"]
                        .iter()
                        .map(|s| s.to_string())
                        .collect();
                }
            }

            // Some flags imply "footnote" verbosity being active.  Make it so.
            if !self.verbosity.contains("footnote") {
                // TODO: Some of these may not use footnotes yet.  They should.
                for flag in ["affected", "new-id", "new-uuid", "project", "override", "recur"] {
                    if self.verbosity.contains(flag) {
                        self.verbosity.insert("footnote".to_string());
                        break;
                    }
                }
            }

            // Some flags imply "header" verbosity being active.  Make it so.
            // (The C++ loops over the one-element list {"default"}.)
            if !self.verbosity.contains("header") && self.verbosity.contains("default") {
                self.verbosity.insert("header".to_string());
            }
        }

        // rc.verbose=true|y|yes|1|on overrides all.
        if self.verbosity_legacy {
            return true;
        }

        // rc.verbose=nothing overrides all.
        if self.verbosity.len() == 1 && self.verbosity.iter().next().map(String::as_str) == Some("nothing") {
            return false;
        }

        // Specific token match.
        self.verbosity.contains(token)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::getColumns`: every column name, sorted.
    pub fn get_columns(&self) -> Vec<String> {
        self.columns.keys().cloned().collect()
    }

    /// C++ `Context::getLimits (rows, lines)`, returning `(rows, lines)`.
    /// A value of zero means unlimited; `limit:page` means however many
    /// screen lines there are; a positive integer is a row/task limit.
    pub fn get_limits(&mut self) -> (i32, i32) {
        let mut rows = 0;
        let mut lines = 0;

        // This is an integer specified as a filter (limit:10).
        let limit = self.config.get("limit");
        if !limit.is_empty() {
            if limit == "page" {
                rows = 0;
                lines = self.get_height();
            } else {
                rows = strtol(&limit);
                lines = 0;
            }
        }

        (rows, lines)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::staticInitialization`: copies settings into the static
    /// state of other classes. Fails with the C++ message when `weekstart`
    /// is not Sunday or Monday.
    fn static_initialization(&mut self) -> Result<()> {
        cli2::set_minimum_match_length(self.config.get_integer("abbreviation.minimum"));
        // C++ assigns the int to a std::string::size_type (wrapping negatives).
        Lexer::set_minimum_match_length(self.config.get_integer("abbreviation.minimum") as usize);

        Task::set_default_project(&self.config.get("default.project"));
        Task::set_default_due(&self.config.get("default.due"));
        Task::set_default_scheduled(&self.config.get("default.scheduled"));

        // C++ `Task::searchCaseSensitive = Variant::searchCaseSensitive =
        // search.case.sensitive`.
        let search_case_sensitive = self.config.get_boolean("search.case.sensitive");
        Task::set_search_case_sensitive(search_case_sensitive);
        Variant::set_search_case_sensitive(search_case_sensitive);
        // C++ `Task::regex = Variant::searchUsingRegex = regex`.
        let regex = self.config.get_boolean("regex");
        Task::set_regex(regex);
        Variant::set_search_using_regex(regex);
        // C++ `Lexer::dateFormat = Variant::dateFormat = dateformat`.
        Variant::set_date_format(&self.config.get("dateformat"));
        Lexer::set_date_format(&self.config.get("dateformat"));

        let week_start = Datetime::day_of_week_str(&self.config.get("weekstart"));
        if week_start != 0 && week_start != 1 {
            return Err(TwError::Message(
                "The 'weekstart' configuration variable may only contain 'Sunday' or 'Monday'.".to_string(),
            ));
        }
        Datetime::set_weekstart(week_start);
        Datetime::set_iso_enabled(self.config.get_boolean("date.iso"));
        Datetime::set_standalone_date_enabled(false);
        Datetime::set_standalone_time_enabled(false);
        Duration::set_standalone_seconds_enabled(false);

        self.tdb2.debug_mode = self.config.get_boolean("debug");

        let mut custom_order = Task::custom_order();
        for (key, value) in self.config.iter() {
            if let Some(rest) = key.strip_prefix("uda.") {
                // C++ `rc.first.substr (rc.first.length () - 7, 7)` throws
                // std::out_of_range for "uda.", "uda.x" and "uda.xx"; that
                // reaches the user as "Unknown error. Please report.".
                if key.len() < 7 {
                    return Err(TwError::Runtime("basic_string::substr".to_string()));
                }
                if key.ends_with(".values") {
                    // C++ `substr (4, length - 7 - 4)`: for "uda.values" the
                    // count wraps to npos, giving "values".
                    let name = if key.len() >= 11 { &rest[..rest.len() - 7] } else { rest };
                    let values = split(value, ',');

                    // C++ `Task::customOrder[name]` is only reached inside
                    // the loop, so an empty `uda.X.values` (split yields no
                    // values) creates no entry and sort_compare falls back to
                    // plain string order with empty values last.
                    for r in values.iter().rev() {
                        custom_order.entry(name.to_string()).or_default().push(r.clone());
                    }
                }
            }
        }
        Task::set_custom_order(custom_order);

        for (name, col) in &self.columns {
            Task::insert_attribute(name, col.type_());
            Lexer::insert_attribute(name, col.type_());
        }

        Task::set_urgency_project_coefficient(self.config.get_real("urgency.project.coefficient") as f32);
        Task::set_urgency_active_coefficient(self.config.get_real("urgency.active.coefficient") as f32);
        Task::set_urgency_scheduled_coefficient(self.config.get_real("urgency.scheduled.coefficient") as f32);
        Task::set_urgency_waiting_coefficient(self.config.get_real("urgency.waiting.coefficient") as f32);
        Task::set_urgency_blocked_coefficient(self.config.get_real("urgency.blocked.coefficient") as f32);
        Task::set_urgency_annotations_coefficient(self.config.get_real("urgency.annotations.coefficient") as f32);
        Task::set_urgency_tags_coefficient(self.config.get_real("urgency.tags.coefficient") as f32);
        Task::set_urgency_due_coefficient(self.config.get_real("urgency.due.coefficient") as f32);
        Task::set_urgency_blocking_coefficient(self.config.get_real("urgency.blocking.coefficient") as f32);
        Task::set_urgency_age_coefficient(self.config.get_real("urgency.age.coefficient") as f32);
        Task::set_urgency_age_max(self.config.get_real("urgency.age.max") as f32);

        // Tag- and project-specific coefficients.
        let mut coefficients = Task::coefficients();
        for var in self.config.all() {
            // urgency.uda.<name>.* does not apply to built-in attributes such as tags;
            // 'task show' reports these.
            if let Some(rest) = var.strip_prefix("urgency.uda.") {
                let name = rest.find('.').map_or(rest, |dot| &rest[..dot]);
                if let Some(col) = self.columns.get(name) {
                    if !col.is_uda() {
                        continue;
                    }
                }
            }

            if var.starts_with("urgency.user.") || var.starts_with("urgency.uda.") {
                coefficients.insert(var.clone(), self.config.get_real(&var) as f32);
            }
        }
        Task::set_coefficients(coefficients);

        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::createDefaultConfig`: on first run (no rc file), creates
    /// a sample `.taskrc` after asking, and loads it.
    ///
    /// When stdout is not a TTY (completion scripts, tests) it neither
    /// prompts nor writes, and fails with `Cannot proceed without rc file.`
    fn create_default_config(&mut self, search_paths: &[String]) -> Result<()> {
        // Do we need to create a default rc?
        if !self.rc_file.data.is_empty() && !self.rc_file.exists() {
            // If stdout is not a file, we are probably executing in a completion context and should not
            // prompt (as the user won't see it) or modify the config (as completion functions are typically
            // read-only).
            if !stdout_is_tty() {
                return Err(TwError::Message("Cannot proceed without rc file.".to_string()));
            }

            if self.config.get_boolean("confirmation")
                && !confirm(&format(
                    "A configuration file could not be found in {1}\n\nWould you like a sample {2} created, so Taskwarrior can proceed?",
                    &[&self.home_dir, &self.rc_file.data],
                ))
            {
                return Err(TwError::Message("Cannot proceed without rc file.".to_string()));
            }

            let now = Datetime::new();
            let mut contents = String::new();
            contents.push_str(&format!("# [Created by {PACKAGE_STRING} {}]\n", now.to_string("m/d/Y H:N:S")));
            contents.push_str(&format!("data.location={}\n", self.data_dir.original));
            contents.push_str(&format!("news.version={}\n", Version::current()));
            contents.push_str("\n# To use the default location of the XDG directories,\n");
            contents.push_str(
                "# move this configuration file from ~/.taskrc to ~/.config/task/taskrc and update location config as follows:\n",
            );
            contents.push_str("\n#data.location=~/.local/share/task\n");
            contents.push_str("#hooks.location=~/.config/task/hooks\n");
            contents.push_str("\n# Color theme (uncomment one to use)\n");
            contents.push_str("include default.theme\n");
            contents.push_str("#include light-16.theme\n");
            contents.push_str("#include light-256.theme\n");
            contents.push_str("#include bubblegum-256.theme\n");
            contents.push_str("#include dark-16.theme\n");
            contents.push_str("#include dark-256.theme\n");
            contents.push_str("#include dark-red-256.theme\n");
            contents.push_str("#include dark-green-256.theme\n");
            contents.push_str("#include dark-blue-256.theme\n");
            contents.push_str("#include dark-violets-256.theme\n");
            contents.push_str("#include dark-yellow-green.theme\n");
            contents.push_str("#include dark-gray-256.theme\n");
            contents.push_str("#include dark-gray-blue-256.theme\n");
            contents.push_str("#include solarized-dark-256.theme\n");
            contents.push_str("#include solarized-light-256.theme\n");
            contents.push_str("#include no-color.theme\n");
            contents.push('\n');

            // Create the parent directory if needed, for platforms where ~/ isn't the
            // default config directory.
            let rc_parent = Directory::new(&self.rc_file.parent());
            if !rc_parent.exists() && !rc_parent.create(0o755) {
                return Err(TwError::Message(format(
                    "Could not create directory '{1}'.",
                    &[&rc_parent.data],
                )));
            }
            // Write out the new file.
            if !File::write_file(&self.rc_file.data, &contents) {
                return Err(TwError::Message(format("Could not write to '{1}'.", &[&self.rc_file.data])));
            }

            // Load it so that it takes effect for this run.
            let rc_path = self.rc_file.data.clone();
            self.config.load(&rc_path, 1, search_paths)?;
        }

        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::decomposeSortField (field, key, ascending,
    /// breakIndicator)`, returning `(key, ascending, break_indicator)`:
    /// a trailing `/` is a break indicator, then a trailing `+`/`-` the
    /// direction (default ascending).
    pub fn decompose_sort_field(field: &str) -> (String, bool, bool) {
        let bytes = field.as_bytes();
        let length = bytes.len();

        let mut decoration = 1;
        let mut break_indicator = false;
        if length >= decoration && bytes[length - decoration] == b'/' {
            break_indicator = true;
            decoration += 1;
        }

        if length >= decoration && bytes[length - decoration] == b'+' {
            (field[..length - decoration].to_string(), true, break_indicator)
        } else if length >= decoration && bytes[length - decoration] == b'-' {
            (field[..length - decoration].to_string(), false, break_indicator)
        } else {
            (field.to_string(), true, break_indicator)
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::debugTiming`: queues `Timer <details> <seconds> sec`
    /// with six decimals.
    pub fn debug_timing(&mut self, details: &str, timer: &Timer) {
        let out = format!("Timer {details} {} sec", printf_f(timer.total_us() / 1.0e6, 6));
        self.debug(&out);
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::withCurrentTask` + `CurrentTask`: sets `current_task`
    /// for the duration of `f`, then restores the previous value (the C++
    /// RAII guard's destructor).
    pub fn with_current_task<R>(&mut self, task: Option<Rc<Task>>, f: impl FnOnce(&mut Context) -> R) -> R {
        let previous = std::mem::replace(&mut self.current_task, task);
        let result = f(self);
        self.current_task = previous;
        result
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::updateXtermTitle`: with `xterm.title` on a TTY, sets the
    /// terminal title to the command line, answering 'what did I just do to
    /// generate this output?'.
    fn update_xterm_title(&mut self) {
        if self.config.get_boolean("xterm.title") && stdout_is_tty() {
            let command = self.cli2.get_command();
            let title: Vec<String> = self.cli2.args.iter().map(|a| a.attribute("raw")).collect();
            let mut stdout = std::io::stdout();
            let _ = write!(stdout, "\x1b]0;task {command} {}\x07", title.join(" "));
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::updateVerbosity`: helper subcommands (`_…`) get clean
    /// output (`verbose=nothing`).
    fn update_verbosity(&mut self) {
        let command = self.cli2.get_command();
        if command.starts_with('_') {
            self.verbosity = ["nothing".to_string()].into_iter().collect();
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::loadAliases`: every `alias.<name>` setting becomes a CLI2
    /// alias.
    fn load_aliases(&mut self) {
        for (key, value) in &self.config {
            if let Some(name) = key.strip_prefix("alias.") {
                self.cli2.alias(name, value);
            }
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::propagateDebug`: `rc.debug` turns on `debug.hooks` and
    /// `debug.parser` unless they already have values (which by default they
    /// do not); either of those being non-zero turns on `debug`.
    fn propagate_debug(&mut self) {
        if self.config.get_boolean("debug") {
            if !self.config.has("debug.hooks") {
                self.config.set("debug.hooks", 1);
            }

            if !self.config.has("debug.parser") {
                self.config.set("debug.parser", 1);
            }
        } else if (self.config.has("debug.hooks") && self.config.get_integer("debug.hooks") != 0)
            || (self.config.has("debug.parser") && self.config.get_integer("debug.parser") != 0)
        {
            // C++ `config.set ("debug", true)`: the bool converts to int 1.
            self.config.set("debug", 1);
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Context::header`: header message sink. No duplicates, no empties.
    pub fn header(&mut self, input: &str) {
        if !input.is_empty() && !self.headers.iter().any(|h| h == input) {
            self.headers.push(input.to_string());
        }
    }

    /// C++ `Context::footnote`: footnote message sink. No duplicates, no
    /// empties.
    pub fn footnote(&mut self, input: &str) {
        if !input.is_empty() && !self.footnotes.iter().any(|f| f == input) {
            self.footnotes.push(input.to_string());
        }
    }

    /// C++ `Context::error`: error message sink, non-maskable. No duplicates,
    /// no empties.
    pub fn error(&mut self, input: &str) {
        if !input.is_empty() && !self.errors.iter().any(|e| e == input) {
            self.errors.push(input.to_string());
        }
    }

    /// C++ `Context::debug`: debug message sink (duplicates kept, empties
    /// dropped).
    pub fn debug(&mut self, input: &str) {
        if !input.is_empty() {
            self.debug_messages.push(input.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::command::{register, Command, CommandMeta};
    use crate::test_util::{env_lock, EnvGuard};
    use std::collections::BTreeMap;
    use std::sync::MutexGuard;

    /// Extracts the C++ `configurationDefaults` literal concatenation from
    /// `Context.cpp` source text: string literals up to the terminating `;`,
    /// skipping `//` comments and the `#ifdef TASK_TEST_RCDIR` branch (the
    /// release build takes `#else`), with C escapes decoded.
    fn extract_cpp_defaults(source: &str) -> String {
        let marker = "std::string configurationDefaults =";
        let start = source.find(marker).expect("configurationDefaults in Context.cpp") + marker.len();
        let mut result = String::new();
        let mut skipping = false;
        'lines: for line in source[start..].lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("#ifdef TASK_TEST_RCDIR") {
                skipping = true;
                continue;
            }
            if trimmed.starts_with("#else") {
                skipping = false;
                continue;
            }
            if trimmed.starts_with("#endif") || skipping {
                continue;
            }
            let mut chars = trimmed.chars().peekable();
            while let Some(c) = chars.next() {
                match c {
                    '"' => loop {
                        match chars.next().expect("unterminated literal") {
                            '"' => break,
                            '\\' => match chars.next().expect("escape") {
                                'n' => result.push('\n'),
                                't' => result.push('\t'),
                                '"' => result.push('"'),
                                '\\' => result.push('\\'),
                                other => panic!("unexpected escape \\{other}"),
                            },
                            other => result.push(other),
                        }
                    },
                    ';' => break 'lines,
                    '/' if chars.peek() == Some(&'/') => continue 'lines,
                    c if c.is_whitespace() => {}
                    other => panic!("unexpected character {other:?} in: {line}"),
                }
            }
        }
        result
    }

    #[test]
    fn defaults_match_cpp_source_verbatim() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../taskwarrior-opus47-dmallya93/src/Context.cpp");
        let Ok(source) = std::fs::read_to_string(&path) else {
            eprintln!("skipping: C++ reference {} not found", path.display());
            return;
        };
        let expected = extract_cpp_defaults(&source);
        assert!(expected.len() > 10_000);
        if expected != CONFIGURATION_DEFAULTS {
            let e: Vec<&str> = expected.lines().collect();
            let a: Vec<&str> = CONFIGURATION_DEFAULTS.lines().collect();
            for (i, (x, y)) in e.iter().zip(a.iter()).enumerate() {
                assert_eq!(x, y, "first difference at line {}", i + 1);
            }
            assert_eq!(e.len(), a.len(), "line count");
            panic!("defaults differ");
        }
    }

    #[test]
    fn defaults_parse_with_theme_include() {
        let mut config = Configuration::new();
        config.parse(CONFIGURATION_DEFAULTS, 1, &[task_rcdir()], "").unwrap();
        assert_eq!(config.get("default.command"), "next");
        assert_eq!(config.get("abbreviation.minimum"), "2");
        assert!(config.has("color.header"), "default.theme was included");
    }

    #[test]
    fn helpers() {
        assert_eq!(strtol("  12abc"), 12);
        assert_eq!(strtol("-3"), -3);
        assert_eq!(strtol("page"), 0);
        assert_eq!(MODIFIER_NAMES.len(), 22);
    }

    /// An isolated environment: HOME is a fresh temporary directory, TASKRC,
    /// TASKDATA and XDG_CONFIG_HOME are unset, stdout is "not a TTY".
    ///
    /// Fields drop in declaration order: the environment is restored and the
    /// temporary directory removed before `_lock` releases the env lock.
    struct Fixture {
        _env: EnvGuard,
        _dir: tempfile::TempDir,
        _lock: MutexGuard<'static, ()>,
        base: String,
        home: String,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            set_stdout_tty_override(None);
        }
    }

    fn fixture() -> Fixture {
        let lock = env_lock();
        let env = EnvGuard::save(&["HOME", "TASKRC", "TASKDATA", "XDG_CONFIG_HOME"]);
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_str().unwrap().to_string();
        let home = format!("{base}/home");
        std::fs::create_dir(&home).unwrap();
        std::env::set_var("HOME", &home);
        std::env::remove_var("TASKRC");
        std::env::remove_var("TASKDATA");
        std::env::remove_var("XDG_CONFIG_HOME");
        set_stdout_tty_override(Some(false));
        Fixture {
            _env: env,
            _dir: dir,
            _lock: lock,
            base,
            home,
        }
    }

    fn write(path: &str, contents: &str) {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// What a test command does when executed.
    enum Behaviour {
        Output(&'static str),
        Fail(fn() -> TwError),
        Messages,
    }

    struct TestCmd {
        meta: CommandMeta,
        behaviour: Behaviour,
    }

    impl Command for TestCmd {
        fn meta(&self) -> &CommandMeta {
            &self.meta
        }

        fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
            match &self.behaviour {
                Behaviour::Output(text) => {
                    output.push_str(text);
                    Ok(0)
                }
                Behaviour::Fail(make) => Err(make()),
                Behaviour::Messages => {
                    context.header("a header");
                    context.footnote("a footnote");
                    output.push_str("body\n");
                    Ok(1)
                }
            }
        }
    }

    fn add_cmd(context: &mut Context, keyword: &str, behaviour: Behaviour) {
        register(
            &mut context.commands,
            Rc::new(TestCmd {
                // Accepts miscellaneous arguments, so `categorizeArgs`
                // allows extra words on the test command lines.
                meta: CommandMeta {
                    keyword: keyword.to_string(),
                    accepts_miscellaneous: true,
                    ..CommandMeta::default()
                },
                behaviour,
            }),
        );
    }

    /// A Context (created after the fixture set HOME) with test commands.
    fn context() -> Context {
        let mut context = Context::new();
        add_cmd(&mut context, "alpha", Behaviour::Output("alpha output\n"));
        add_cmd(&mut context, "beta", Behaviour::Output("beta output\n"));
        add_cmd(&mut context, "_gamma", Behaviour::Output("gamma\n"));
        add_cmd(&mut context, "messages", Behaviour::Messages);
        add_cmd(
            &mut context,
            "fail",
            Behaviour::Fail(|| TwError::Message("it failed".to_string())),
        );
        add_cmd(&mut context, "status", Behaviour::Fail(|| TwError::Status(9)));
        add_cmd(
            &mut context,
            "runtime",
            Behaviour::Fail(|| TwError::Runtime("Parenthesis is not closed.".to_string())),
        );
        add_cmd(
            &mut context,
            "stub",
            Behaviour::Fail(|| TwError::NotImplemented("stub".to_string())),
        );
        context
    }

    fn init(context: &mut Context, list: &[&str]) -> (i32, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let rc = context.initialize_with(&args(list), &mut out, &mut err).unwrap();
        (
            rc,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    fn run(context: &mut Context) -> (i32, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let rc = context.run_with(&mut out, &mut err);
        (
            rc,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn taskrc_and_taskdata_environment() {
        let f = fixture();
        let rc = format!("{}/custom/rc", f.base);
        let data = format!("{}/data", f.base);
        write(&rc, "foo=bar\n");
        std::env::set_var("TASKRC", &rc);
        std::env::set_var("TASKDATA", &data);

        let mut context = context();
        let (status, _, stderr) = init(&mut context, &["task", "alpha"]);
        assert_eq!(status, 0, "{stderr}");
        assert_eq!(context.home_dir, f.home);
        assert_eq!(context.rc_file.data, rc);
        assert_eq!(context.config.file(), rc);
        assert_eq!(context.data_dir.data, data);
        assert_eq!(context.config.get("foo"), "bar");
        assert_eq!(context.config.get("data.location"), data);
        assert_eq!(
            context.headers,
            [format!("TASKRC override: {rc}"), format!("TASKDATA override: {data}")]
        );
        assert_eq!(context.cli2.get_command(), "alpha");
        assert_eq!(context.defaults.get("data.location"), format!("{}/.task", f.home));
        assert!(context
            .debug_messages
            .iter()
            .any(|d| d.starts_with(&format!("Timer Config::load ({rc}) ")) && d.ends_with(" sec")));
    }

    #[test]
    fn rc_and_data_location_command_line_overrides() {
        let f = fixture();
        write(&format!("{}/.taskrc", f.home), "which=home\n");
        let other = format!("{}/other.rc", f.base);
        write(&other, "which=other\n");
        let data = format!("{}/d", f.base);
        let data_arg = format!("rc.data.location={data}");
        let rc_arg = format!("rc:{other}");

        let mut context = context();
        let (status, _, stderr) = init(&mut context, &["task", &rc_arg, &data_arg, "beta"]);
        assert_eq!(status, 0, "{stderr}");
        assert_eq!(context.rc_file.data, other);
        assert_eq!(context.config.get("which"), "other");
        assert_eq!(context.data_dir.data, data);
        assert_eq!(context.config.get("data.location"), data);
        assert_eq!(
            context.headers,
            [format!("TASKRC override: {other}"), format!("TASKDATA override: {data}")]
        );
        assert_eq!(context.footnotes, [format!("Configuration override {data_arg}")]);
        assert_eq!(context.cli2.get_command(), "beta");
    }

    #[test]
    fn home_taskrc_without_overrides() {
        let f = fixture();
        write(&format!("{}/.taskrc", f.home), "which=home\n");
        let mut context = context();
        let (status, _, _) = init(&mut context, &["task", "alpha"]);
        assert_eq!(status, 0);
        assert_eq!(context.rc_file.data, format!("{}/.taskrc", f.home));
        assert_eq!(context.data_dir.data, format!("{}/.task", f.home));
        assert!(context.headers.is_empty(), "{:?}", context.headers);
        assert!(context.footnotes.is_empty());
    }

    #[test]
    fn xdg_config_home_fallback() {
        let f = fixture();
        let xdg = format!("{}/xdg/", f.base);
        let xdg_rc = format!("{}/xdg/task/taskrc", f.base);
        write(&xdg_rc, "which=xdg\n");
        std::env::set_var("XDG_CONFIG_HOME", &xdg);

        let mut context = context();
        let (status, _, stderr) = init(&mut context, &["task", "alpha"]);
        assert_eq!(status, 0, "{stderr}");
        assert_eq!(context.rc_file.data, xdg_rc, "trailing '/' removed");
        assert_eq!(context.config.get("which"), "xdg");
        assert!(context.headers.is_empty(), "XDG is not an override");

        // ~/.taskrc wins over XDG.
        write(&format!("{}/.taskrc", f.home), "which=home\n");
        let mut context = self::context();
        init(&mut context, &["task", "alpha"]);
        assert_eq!(context.config.get("which"), "home");
    }

    #[test]
    fn dot_config_fallback_without_xdg() {
        let f = fixture();
        let rc = format!("{}/.config/task/taskrc", f.home);
        write(&rc, "which=dotconfig\n");
        let mut context = context();
        let (status, _, _) = init(&mut context, &["task", "alpha"]);
        assert_eq!(status, 0);
        assert_eq!(context.rc_file.data, rc);
        assert_eq!(context.config.get("which"), "dotconfig");
    }

    #[test]
    fn first_run_non_interactive_refuses() {
        let f = fixture();
        let mut context = context();
        let (status, stdout, stderr) = init(&mut context, &["task", "alpha"]);
        assert_eq!(status, 2);
        assert_eq!(stdout, "");
        assert_eq!(context.errors, ["Cannot proceed without rc file."]);
        assert_eq!(stderr, "Cannot proceed without rc file.\n");
        assert!(!std::path::Path::new(&format!("{}/.taskrc", f.home)).exists());
    }

    #[test]
    fn first_run_interactive_without_confirmation_creates_rc() {
        let f = fixture();
        set_stdout_tty_override(Some(true));
        let mut context = context();
        let (status, _, stderr) = init(&mut context, &["task", "rc.confirmation=0", "alpha"]);
        assert_eq!(status, 0, "{stderr}");
        let rc = format!("{}/.taskrc", f.home);
        let contents = std::fs::read_to_string(&rc).unwrap();
        assert!(contents.starts_with("# [Created by task 3.5.0 "), "{contents}");
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines[1], format!("data.location={}/.task", f.home));
        assert_eq!(lines[2], "news.version=3.5.0");
        assert_eq!(lines[3], "");
        assert_eq!(lines[4], "# To use the default location of the XDG directories,");
        assert!(contents.contains("\ninclude default.theme\n#include light-16.theme\n"));
        assert!(contents.ends_with("#include no-color.theme\n\n"));
        assert_eq!(context.config.get("news.version"), "3.5.0");
        assert_eq!(context.config.file(), rc);
    }

    #[test]
    fn first_run_creates_missing_parent_directory() {
        let f = fixture();
        set_stdout_tty_override(Some(true));
        let rc = format!("{}/new/dir/taskrc", f.base);
        std::env::set_var("TASKRC", &rc);
        let mut context = context();
        let (status, _, stderr) = init(&mut context, &["task", "rc.confirmation:off", "alpha"]);
        assert_eq!(status, 0, "{stderr}");
        assert!(std::path::Path::new(&rc).is_file());
    }

    #[test]
    fn apply_overrides_sets_values_and_footnotes() {
        let _f = fixture();
        let mut context = Context::new();
        context.config.set("verbose", "override");
        let list = args(&["task", "rc.a=1", "rc.b:2", "rc.c=x:y", "rc.nosep", "x", "--", "rc.d=4"]);
        cli2::apply_overrides(&list, &mut context);
        assert_eq!(context.config.get("a"), "1");
        assert_eq!(context.config.get("b"), "2");
        assert_eq!(context.config.get("c"), "x:y", "first separator splits");
        assert!(!context.config.has("nosep"));
        assert!(!context.config.has("d"), "nothing after --");
        assert_eq!(
            context.footnotes,
            [
                "Configuration override rc.a=1",
                "Configuration override rc.b:2",
                "Configuration override rc.c=x:y",
                "Configuration override rc.nosep",
            ]
        );

        // The overrides apply before verbosity is decided.
        let mut context = Context::new();
        context.config.set("verbose", "override");
        cli2::apply_overrides(&args(&["task", "rc.verbose=nothing"]), &mut context);
        assert!(context.footnotes.is_empty());
    }

    #[test]
    fn verbosity_rules() {
        let check = |value: &str, token: &str| {
            let mut context = Context::new();
            context.config.set("verbose", value);
            context.verbose(token)
        };
        // Legacy true overrides all.
        assert!(check("on", "anything"));
        assert!(check("1", "header"));
        // nothing overrides all.
        assert!(!check("nothing", "header"));
        // Specific tokens.
        assert!(check("header,label", "label"));
        assert!(!check("header,label", "footnote"));
        // Implied footnote / header.
        assert!(check("affected", "footnote"));
        assert!(check("override", "footnote"));
        assert!(check("default", "header"));
        assert!(!check("default", "footnote"));
        // A false value that is not a token emulates rc.verbose=off of 1.9.4.
        for token in ["blank", "label", "new-id", "edit"] {
            assert!(check("off", token));
            assert!(check("0", token));
        }
        assert!(!check("off", "header"));
        // Only the first (sorted) token is validated.
        assert!(check("affected,bogus", "bogus"));
        assert!(!check("sync,zzz", "header"));
        assert!(check("sync,zzz", "zzz"));

        // Cached once computed.
        let mut context = Context::new();
        context.config.set("verbose", "header");
        assert!(context.verbose("header"));
        context.config.set("verbose", "nothing");
        assert!(context.verbose("header"));
    }

    #[test]
    fn message_queues() {
        let mut context = Context::new();
        context.header("h");
        context.header("h");
        context.header("");
        context.footnote("f");
        context.footnote("f");
        context.footnote("g");
        context.error("e");
        context.error("e");
        context.debug("d");
        context.debug("d");
        context.debug("");
        assert_eq!(context.headers, ["h"]);
        assert_eq!(context.footnotes, ["f", "g"]);
        assert_eq!(context.errors, ["e"]);
        assert_eq!(context.debug_messages, ["d", "d"]);

        let timer = Timer::new();
        context.debug_timing("x", &timer);
        let last = context.debug_messages.last().unwrap();
        assert!(last.starts_with("Timer x 0.") && last.ends_with(" sec"), "{last}");
        let seconds = &last["Timer x ".len()..last.len() - " sec".len()];
        assert_eq!(seconds.split('.').nth(1).unwrap().len(), 6);
    }

    #[test]
    fn dispatch_by_command_word() {
        let f = fixture();
        write(&format!("{}/.taskrc", f.home), "");
        let mut context = context();
        let (status, _, _) = init(&mut context, &["task", "rc.verbose=nothing", "+tag", "beta", "alpha"]);
        assert_eq!(status, 0);
        let (rc, stdout, stderr) = run(&mut context);
        assert_eq!((rc, stdout.as_str(), stderr.as_str()), (0, "beta output\n", ""));
    }

    #[test]
    fn dispatch_abbreviated_and_aliased_command() {
        let f = fixture();
        write(&format!("{}/.taskrc", f.home), "alias.al=alpha\n");
        let mut context = context();
        assert_eq!(init(&mut context, &["task", "bet"]).0, 0);
        assert_eq!(context.cli2.get_command(), "beta");

        let mut context = self::context();
        assert_eq!(init(&mut context, &["task", "al"]).0, 0);
        assert_eq!(context.cli2.get_command(), "alpha");
    }

    #[test]
    fn dispatch_default_command() {
        let f = fixture();
        write(&format!("{}/.taskrc", f.home), "default.command=alpha\nverbose=default\n");
        let mut context = context();
        let (status, _, _) = init(&mut context, &["task"]);
        assert_eq!(status, 0);
        assert_eq!(context.headers, ["[task alpha]"]);
        let (rc, stdout, stderr) = run(&mut context);
        assert_eq!(rc, 0);
        assert_eq!(stdout, "alpha output\n");
        assert_eq!(stderr, "[task alpha]\n");

        // An ID with no command assumes 'information' (not registered here).
        let mut context = self::context();
        add_cmd(&mut context, "information", Behaviour::Output("info\n"));
        let (status, _, _) = init(&mut context, &["task", "1"]);
        assert_eq!(status, 0);
        assert_eq!(context.headers, ["No command specified - assuming 'information'."]);
        assert_eq!(context.cli2.get_command(), "information");
    }

    #[test]
    fn stub_command_dispatch_reports_not_implemented() {
        // The real registry (Command::factory), no test commands: `prepend`
        // is a stub, so run takes the `throw std::string` path: message on
        // stderr, exit status 2.
        let f = fixture();
        write(&format!("{}/.taskrc", f.home), "");
        let mut context = Context::new();
        let (status, _, stderr) = init(&mut context, &["task", "prepend", "foo"]);
        assert_eq!(status, 0, "{stderr}");
        assert_eq!(context.cli2.get_command(), "prepend");
        let (rc, stdout, stderr) = run(&mut context);
        assert_eq!(rc, 2);
        assert_eq!(stdout, "");
        assert_eq!(stderr, "Command 'prepend' is not yet implemented.\n");

        // A default custom report (CmdCustom, ported) dispatches to the
        // report: no tasks, so the `No matches.` footnote and status 1.
        let mut context = Context::new();
        assert_eq!(init(&mut context, &["task", "rc.verbose=footnote", "next"]).0, 0);
        let (rc, stdout, stderr) = run(&mut context);
        assert_eq!((rc, stdout.as_str()), (1, ""));
        assert!(stderr.ends_with("No matches.\n"), "{stderr}");
    }

    #[test]
    fn no_command_is_an_error() {
        let f = fixture();
        write(&format!("{}/.taskrc", f.home), "default.command=\n");
        let mut context = context();
        let (status, _, stderr) = init(&mut context, &["task", "unknownword"]);
        assert_eq!(status, 2);
        assert_eq!(stderr, "You must specify a command or a task to modify.\n");
    }

    #[test]
    fn helper_command_silences_verbosity() {
        let f = fixture();
        write(&format!("{}/.taskrc", f.home), "");
        let mut context = context();
        init(&mut context, &["task", "_gamma"]);
        context.header("hidden");
        let (rc, stdout, stderr) = run(&mut context);
        assert_eq!((rc, stdout.as_str(), stderr.as_str()), (0, "gamma\n", ""));
        assert!(context.verbosity.contains("nothing"));
    }

    #[test]
    fn run_emission_order_and_error_statuses() {
        let f = fixture();
        write(&format!("{}/.taskrc", f.home), "");
        let mut context = context();
        init(&mut context, &["task", "messages"]);
        let (rc, stdout, stderr) = run(&mut context);
        assert_eq!(rc, 1);
        assert_eq!(stdout, "body\n");
        assert_eq!(stderr, "a header\na footnote\n");

        let mut context = self::context();
        init(&mut context, &["task", "fail"]);
        let (rc, _, stderr) = run(&mut context);
        assert_eq!((rc, stderr.as_str()), (2, "it failed\n"));

        let mut context = self::context();
        init(&mut context, &["task", "status"]);
        let (rc, _, stderr) = run(&mut context);
        assert_eq!((rc, stderr.as_str()), (4, ""));

        let mut context = self::context();
        init(&mut context, &["task", "stub"]);
        let (rc, _, stderr) = run(&mut context);
        assert_eq!((rc, stderr.as_str()), (2, "Command 'stub' is not yet implemented.\n"));

        let mut context = self::context();
        init(&mut context, &["task", "runtime"]);
        let (rc, _, stderr) = run(&mut context);
        assert_eq!((rc, stderr.as_str()), (3, "Unknown error. Please report.\n"));
    }

    #[test]
    fn run_debug_output() {
        let f = fixture();
        write(&format!("{}/.taskrc", f.home), "");
        let mut context = context();
        let (status, _, _) = init(&mut context, &["task", "rc.debug=1", "rc.verbose=nothing", "alpha"]);
        assert_eq!(status, 0);
        assert_eq!(context.config.get("debug.hooks"), "1", "propagateDebug");
        assert_eq!(context.config.get("debug.parser"), "1");
        let (rc, stdout, stderr) = run(&mut context);
        assert_eq!(rc, 0);
        assert_eq!(stdout, "alpha output\n");
        let lines: Vec<&str> = stderr.lines().collect();
        assert!(lines[0].starts_with("Timer Config::load ("), "{stderr}");
        assert!(stderr.contains("Parse Tree (before command-specif\u{456}c processing)"));
        let perf = lines.iter().find(|l| l.starts_with("Perf task 3.5.0 ")).expect(&stderr);
        let fields: Vec<&str> = perf.split(' ').collect();
        assert_eq!(fields[3], PERF_COMMIT);
        assert_eq!(fields[4].len(), 16);
        let names: Vec<&str> = fields[5..].iter().map(|f| f.split(':').next().unwrap()).collect();
        assert_eq!(
            names,
            ["init", "load", "gc", "filter", "commit", "sort", "render", "hooks", "other", "total"]
        );
        assert!(stderr.ends_with("\n\n"), "the Perf message ends in its own newline");
    }

    #[test]
    fn initialize_failure_dumps_debug_and_headers() {
        let f = fixture();
        let rc = format!("{}/rc", f.base);
        write(&rc, "weekstart=friday\ndebug=1\n");
        std::env::set_var("TASKRC", &rc);
        let mut context = context();
        let (status, _, stderr) = init(&mut context, &["task", "alpha"]);
        assert_eq!(status, 2);
        let lines: Vec<&str> = stderr.lines().collect();
        assert!(lines[0].starts_with("Timer Config::load ("), "{stderr}");
        assert_eq!(lines[1], format!("TASKRC override: {rc}"));
        assert_eq!(
            lines[2],
            "The 'weekstart' configuration variable may only contain 'Sunday' or 'Monday'."
        );
    }

    #[test]
    fn custom_report_conflict_fails_initialize() {
        let f = fixture();
        write(&format!("{}/.taskrc", f.home), "report.alpha.columns=id\n");
        let mut context = context();
        let (status, _, stderr) = init(&mut context, &["task", "alpha"]);
        assert_eq!(status, 2);
        assert_eq!(stderr, "Custom report 'alpha' conflicts with built-in task command.\n");
    }

    #[test]
    fn initialize_registers_operator_entities() {
        let f = fixture();
        write(&format!("{}/.taskrc", f.home), "");
        let mut context = context();
        let (status, _, _) = init(&mut context, &["task", "alpha"]);
        assert_eq!(status, 0);
        assert_eq!(context.cli2.entities["operator"], Eval::get_operators());
        assert_eq!(context.cli2.entities["operator"].len(), 26);
        assert_eq!(context.cli2.entities["binary_operator"], Eval::get_binary_operators());
        assert_eq!(context.cli2.entities["binary_operator"].len(), 21);
        assert!(context.cli2.entities["binary_operator"].iter().any(|o| o == "_hastag_"));
        assert!(!context.cli2.entities["binary_operator"].iter().any(|o| o == "_neg_"));
    }

    #[test]
    fn static_initialization_sets_match_length() {
        let f = fixture();
        write(&format!("{}/.taskrc", f.home), "abbreviation.minimum=4\n");
        let mut context = context();
        let (status, _, _) = init(&mut context, &["task", "alph"]);
        assert_eq!(status, 0);
        assert_eq!(cli2::minimum_match_length(), 4);
        assert_eq!(context.cli2.get_command(), "alpha");
        cli2::set_minimum_match_length(3);
    }

    /// Restores the Lexer/Datetime/Duration statics (and the CLI2 match
    /// length) to their defaults when dropped. Declared after the fixture
    /// so it drops first, before the fixture releases the env lock.
    struct StaticsGuard;

    impl Drop for StaticsGuard {
        fn drop(&mut self) {
            cli2::set_minimum_match_length(3);
            Lexer::set_minimum_match_length(3);
            Lexer::set_date_format("");
            Variant::set_search_case_sensitive(true);
            Variant::set_search_using_regex(true);
            Variant::set_date_format("");
            Datetime::set_weekstart(1);
            Datetime::set_iso_enabled(true);
            Datetime::set_standalone_date_enabled(true);
            Datetime::set_standalone_time_enabled(true);
            Duration::set_standalone_seconds_enabled(true);
            Task::set_default_project("");
            Task::set_default_due("");
            Task::set_default_scheduled("");
            Task::set_search_case_sensitive(true);
            Task::set_regex(false);
            Task::set_custom_order(BTreeMap::new());
            Task::set_coefficients(BTreeMap::new());
            Task::set_attributes(BTreeMap::new());
            Lexer::set_attributes(BTreeMap::new());
            for set in [
                Task::set_urgency_project_coefficient,
                Task::set_urgency_active_coefficient,
                Task::set_urgency_scheduled_coefficient,
                Task::set_urgency_waiting_coefficient,
                Task::set_urgency_blocked_coefficient,
                Task::set_urgency_annotations_coefficient,
                Task::set_urgency_tags_coefficient,
                Task::set_urgency_due_coefficient,
                Task::set_urgency_blocking_coefficient,
                Task::set_urgency_age_coefficient,
                Task::set_urgency_age_max,
            ] {
                set(0.0);
            }
        }
    }

    #[test]
    fn static_initialization_sets_task_statics() {
        let f = fixture();
        let _statics = StaticsGuard;
        write(
            &format!("{}/.taskrc", f.home),
            "default.project=home\ndefault.due=eom\ndefault.scheduled=tomorrow\nsearch.case.sensitive=0\n\
             regex=0\nuda.size.values=S,M,L\nuda.blank.values=\nurgency.user.tag.foo.coefficient=2.5\nurgency.uda.size.L.coefficient=1.5\n",
        );
        let mut context = context();
        let (status, _, _) = init(&mut context, &["task", "alpha"]);
        assert_eq!(status, 0);
        assert_eq!(Task::default_project(), "home");
        assert_eq!(Task::default_due(), "eom");
        assert_eq!(Task::default_scheduled(), "tomorrow");
        assert!(!Task::search_case_sensitive());
        assert!(!Task::regex());
        let order = Task::custom_order();
        assert_eq!(order["size"], vec!["L", "M", "S"]);
        // An empty uda.X.values creates no entry (C++ only indexes the map
        // inside the per-value loop).
        assert!(!order.contains_key("blank"));
        assert_eq!(order["priority"], vec!["", "L", "M", "H"]);
        assert_eq!(Task::urgency_project_coefficient(), 1.0);
        assert_eq!(Task::urgency_active_coefficient(), 4.0);
        assert_eq!(Task::urgency_scheduled_coefficient(), 5.0);
        assert_eq!(Task::urgency_waiting_coefficient(), -3.0);
        assert_eq!(Task::urgency_blocked_coefficient(), -5.0);
        assert_eq!(Task::urgency_annotations_coefficient(), 1.0);
        assert_eq!(Task::urgency_tags_coefficient(), 1.0);
        assert_eq!(Task::urgency_due_coefficient(), 12.0);
        assert_eq!(Task::urgency_blocking_coefficient(), 8.0);
        assert_eq!(Task::urgency_age_coefficient(), 2.0);
        assert_eq!(Task::urgency_age_max(), 365.0);
        let coefficients = Task::coefficients();
        assert_eq!(coefficients["urgency.user.tag.next.coefficient"], 15.0);
        assert_eq!(coefficients["urgency.user.tag.foo.coefficient"], 2.5);
        assert_eq!(coefficients["urgency.uda.size.L.coefficient"], 1.5);
        assert_eq!(coefficients["urgency.uda.priority.M.coefficient"], 3.9f32);
        assert!(!coefficients.contains_key("urgency.due.coefficient"));
    }

    #[test]
    fn initialize_registers_columns_and_attribute_types() {
        let f = fixture();
        let _statics = StaticsGuard;
        write(
            &format!("{}/.taskrc", f.home),
            "uda.size.type=numeric\nuda.size.label=Size\nuda.ref.type=uuid\nuda.est.type=duration\n\
             uda.when.type=date\nuda.labelonly.label=x\n",
        );
        let mut context = context();
        let (status, _, err) = init(&mut context, &["task", "alpha"]);
        assert_eq!(status, 0, "{err}");

        // C++ Column::factory (columns): built-ins plus UDAs (priority comes
        // from the defaults; a UDA without a type has no column).
        let names = context.get_columns();
        for name in [
            "depends", "description", "due", "end", "entry", "id", "imask", "last", "mask", "modified", "parent",
            "project", "recur", "rtype", "scheduled", "start", "status", "tags", "template", "until", "urgency",
            "uuid", "wait", "priority", "size", "ref", "est", "when",
        ] {
            assert!(names.contains(&name.to_string()), "{name}");
        }
        assert!(!names.contains(&"labelonly".to_string()));
        assert_eq!(names.len(), 28);
        assert_eq!(context.columns["size"].label(), "Size");
        assert!(context.columns["size"].is_uda());

        // Every column is an `attribute` entity.
        assert!(context.cli2.entities.get("attribute").is_some_and(|set| set.iter().any(|e| e == "project")));

        // staticInitialization: Task::attributes and Lexer::attributes.
        let expected = [
            ("due", "date"),
            ("project", "string"),
            ("id", "numeric"),
            ("urgency", "numeric"),
            ("tags", "string"),
            ("size", "numeric"),
            ("ref", "uuid"),
            ("est", "duration"),
            ("when", "date"),
            ("priority", "string"),
        ];
        for (name, type_) in expected {
            assert_eq!(Task::attributes().get(name).map(String::as_str), Some(type_), "{name}");
            assert_eq!(Lexer::attributes().get(name).map(String::as_str), Some(type_), "{name}");
        }
        assert_eq!(Task::attributes().len(), 28);
    }

    #[test]
    fn initialize_rejects_bad_udas() {
        let f = fixture();
        let _statics = StaticsGuard;
        write(&format!("{}/.taskrc", f.home), "uda.due.type=string\n");
        let mut ctx = context();
        let (status, _, err) = init(&mut ctx, &["task", "alpha"]);
        assert_eq!(status, 2);
        assert_eq!(err, "The UDA named 'due' is the same as a core attribute, and is not permitted.\n");

        write(&format!("{}/.taskrc", f.home), "uda.x.type=bool\n");
        let mut ctx = context();
        let (status, _, err) = init(&mut ctx, &["task", "alpha"]);
        assert_eq!(status, 2);
        assert_eq!(
            err,
            "User defined attributes may only be of type 'string', 'uuid', 'date', 'duration' or 'numeric'.\n"
        );
    }

    #[test]
    fn static_initialization_short_uda_key_is_unknown_error() {
        let f = fixture();
        let _statics = StaticsGuard;
        write(&format!("{}/.taskrc", f.home), "uda.ab=1\n");
        let mut context = context();
        let (status, _, err) = init(&mut context, &["task", "alpha"]);
        assert_ne!(status, 0);
        assert!(err.contains("Unknown error. Please report."), "{err}");
    }

    #[test]
    fn static_initialization_sets_lexer_datetime_duration_statics() {
        let f = fixture();
        let _statics = StaticsGuard;
        write(
            &format!("{}/.taskrc", f.home),
            "abbreviation.minimum=5\ndateformat=D.M.Y\nweekstart=Monday\ndate.iso=0\nsearch.case.sensitive=0\nregex=0\n",
        );
        let mut configured = context();
        let (status, _, _) = init(&mut configured, &["task", "alpha"]);
        assert_eq!(status, 0);
        assert_eq!(Lexer::minimum_match_length(), 5);
        assert_eq!(Lexer::date_format(), "D.M.Y");
        assert_eq!(Variant::date_format(), "D.M.Y");
        assert!(!Variant::search_case_sensitive());
        assert!(!Variant::search_using_regex());
        assert_eq!(Datetime::weekstart(), 1);
        assert!(!Datetime::iso_enabled());
        assert!(!Datetime::standalone_date_enabled());
        assert!(!Datetime::standalone_time_enabled());
        assert!(!Duration::standalone_seconds_enabled());

        // The compiled-in defaults: abbreviation.minimum=2, dateformat=Y-M-D,
        // weekstart=sunday, date.iso=1.
        write(&format!("{}/.taskrc", f.home), "\n");
        let mut defaulted = context();
        let (status, _, _) = init(&mut defaulted, &["task", "alpha"]);
        assert_eq!(status, 0);
        assert_eq!(Lexer::minimum_match_length(), 2);
        assert_eq!(Lexer::date_format(), "Y-M-D");
        assert_eq!(Variant::date_format(), "Y-M-D");
        assert!(Variant::search_case_sensitive());
        assert!(Variant::search_using_regex());
        assert_eq!(Datetime::weekstart(), 0);
        assert!(Datetime::iso_enabled());
        assert!(!Datetime::standalone_date_enabled());
        assert!(!Duration::standalone_seconds_enabled());
    }

    #[test]
    fn color_decision() {
        let _f = fixture();
        let mut context = Context::new();
        context.config.set("color", "on");
        context.config.set("detection", "on");
        assert!(!context.color(), "not a tty");
        assert_eq!(context.config.get("color"), "off");
        assert_eq!(context.config.get("detection"), "off");

        let mut context = Context::new();
        context.config.set("_forcecolor", "1");
        assert!(context.color());
        assert_eq!(context.config.get("color"), "on");
        context.config.set("_forcecolor", "0");
        assert!(context.color(), "cached");

        set_stdout_tty_override(Some(true));
        let mut context = Context::new();
        context.config.set("color", "on");
        assert!(context.color());
        let mut context = Context::new();
        context.config.set("color", "off");
        assert!(!context.color());
    }

    #[test]
    fn width_height_and_limits() {
        let mut context = Context::new();
        context.config.set("defaultwidth", 0);
        context.config.set("defaultheight", 0);
        assert_eq!(context.get_width(), 65536);
        assert_eq!(context.get_height(), 65536);

        context.config.set("defaultwidth", 100);
        context.config.set("defaultheight", 40);
        context.config.set("detection", "off");
        assert_eq!(context.get_width(), 100);
        assert_eq!(context.get_height(), 40);

        context.config.set("detection", "on");
        context.config.set("avoidlastcolumn", "on");
        context.terminal_width = 50;
        context.terminal_height = 20;
        assert_eq!(context.get_width(), 49);
        assert_eq!(context.get_height(), 20);

        assert_eq!(context.get_limits(), (0, 0));
        context.config.set("limit", "10");
        assert_eq!(context.get_limits(), (10, 0));
        context.config.set("limit", "page");
        assert_eq!(context.get_limits(), (0, 20));
    }

    #[test]
    fn task_context_lookup() {
        let mut context = Context::new();
        assert_eq!(context.get_task_context("read", "", true), "");
        assert_eq!(context.debug_messages, ["No context set"]);

        context.debug_messages.clear();
        context.config.set("context", "work");
        context.config.set("context.work", "+work");
        assert_eq!(context.get_task_context("read", "", true), "+work");
        assert_eq!(
            context.debug_messages,
            [
                "Applying context 'work'",
                "Specific read context for 'work' not defined. ",
                "Trying to interpret old-style context definition as read context.",
                "Detected context string: +work",
            ]
        );
        assert_eq!(context.get_task_context("read", "work", false), "");
        assert_eq!(context.debug_messages.last().unwrap(), "Detected context string: (empty)");

        context.config.set("context.home.read", "+home");
        context.config.set("context.home.write", "+w");
        assert_eq!(context.get_task_context("read", "home", true), "+home");
        assert_eq!(context.get_task_context("write", "home", true), "+w");
        assert_eq!(context.get_task_context("write", "work", true), "");
    }

    #[test]
    fn sort_field_decomposition() {
        assert_eq!(Context::decompose_sort_field("due+"), ("due".into(), true, false));
        assert_eq!(Context::decompose_sort_field("due-"), ("due".into(), false, false));
        assert_eq!(Context::decompose_sort_field("due"), ("due".into(), true, false));
        assert_eq!(Context::decompose_sort_field("project+/"), ("project".into(), true, true));
        assert_eq!(Context::decompose_sort_field("project/"), ("project/".into(), true, true));
        assert_eq!(Context::decompose_sort_field(""), ("".into(), true, false));
    }

    #[test]
    fn propagate_debug_rules() {
        let mut context = Context::new();
        context.config.set("debug.parser", 2);
        context.propagate_debug();
        assert_eq!(context.config.get("debug"), "1");
        assert!(!context.config.has("debug.hooks"));

        let mut context = Context::new();
        context.config.set("debug", "on");
        context.config.set("debug.parser", 3);
        context.propagate_debug();
        assert_eq!(context.config.get("debug.hooks"), "1");
        assert_eq!(context.config.get("debug.parser"), "3");

        let mut context = Context::new();
        context.config.set("debug.hooks", 0);
        context.propagate_debug();
        assert!(!context.config.has("debug"));
    }

    #[test]
    fn current_task_scope() {
        let mut context = Context::new();
        let task = Rc::new(Task::default());
        let inside = context.with_current_task(Some(task.clone()), |c| c.current_task.is_some());
        assert!(inside);
        assert!(context.current_task.is_none());
    }

    #[test]
    fn rcdir_has_default_theme() {
        let dir = task_rcdir();
        assert!(
            std::path::Path::new(&dir).join("default.theme").is_file(),
            "{dir}"
        );
    }
}
