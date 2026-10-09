//! Rust translation of `src/commands/CmdHistory.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdHistory.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdHistoryDaily` (`history.daily`).
pub struct CmdHistoryDaily {
    meta: CommandMeta,
}

impl CmdHistoryDaily {
    /// C++ `CmdHistoryBase<DailyHistoryStrategy>::CmdHistoryBase` (`typedef CmdHistoryDaily`).
    pub fn new() -> Self {
        CmdHistoryDaily {
            meta: CommandMeta {
                keyword: "history.daily".into(),
                usage: "task <filter> history.daily".into(),
                description: "Shows a report of task history, by day".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Graphs,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdHistoryDaily {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdHistoryDaily {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdHistoryDaily::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdGHistoryDaily` (`ghistory.daily`).
pub struct CmdGHistoryDaily {
    meta: CommandMeta,
}

impl CmdGHistoryDaily {
    /// C++ `CmdHistoryBase<DailyGHistoryStrategy>::CmdHistoryBase` (`typedef CmdGHistoryDaily`).
    pub fn new() -> Self {
        CmdGHistoryDaily {
            meta: CommandMeta {
                keyword: "ghistory.daily".into(),
                usage: "task <filter> ghistory.daily".into(),
                description: "Shows a graphical report of task history, by day".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Graphs,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdGHistoryDaily {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdGHistoryDaily {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdGHistoryDaily::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdHistoryWeekly` (`history.weekly`).
pub struct CmdHistoryWeekly {
    meta: CommandMeta,
}

impl CmdHistoryWeekly {
    /// C++ `CmdHistoryBase<WeeklyHistoryStrategy>::CmdHistoryBase` (`typedef CmdHistoryWeekly`).
    pub fn new() -> Self {
        CmdHistoryWeekly {
            meta: CommandMeta {
                keyword: "history.weekly".into(),
                usage: "task <filter> history.weekly".into(),
                description: "Shows a report of task history, by week".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Graphs,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdHistoryWeekly {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdHistoryWeekly {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdHistoryWeekly::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdGHistoryWeekly` (`ghistory.weekly`).
pub struct CmdGHistoryWeekly {
    meta: CommandMeta,
}

impl CmdGHistoryWeekly {
    /// C++ `CmdHistoryBase<WeeklyGHistoryStrategy>::CmdHistoryBase` (`typedef CmdGHistoryWeekly`).
    pub fn new() -> Self {
        CmdGHistoryWeekly {
            meta: CommandMeta {
                keyword: "ghistory.weekly".into(),
                usage: "task <filter> ghistory.weekly".into(),
                description: "Shows a graphical report of task history, by week".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Graphs,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdGHistoryWeekly {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdGHistoryWeekly {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdGHistoryWeekly::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdHistoryMonthly` (`history.monthly`).
pub struct CmdHistoryMonthly {
    meta: CommandMeta,
}

impl CmdHistoryMonthly {
    /// C++ `CmdHistoryBase<MonthlyHistoryStrategy>::CmdHistoryBase` (`typedef CmdHistoryMonthly`).
    pub fn new() -> Self {
        CmdHistoryMonthly {
            meta: CommandMeta {
                keyword: "history.monthly".into(),
                usage: "task <filter> history.monthly".into(),
                description: "Shows a report of task history, by month".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Graphs,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdHistoryMonthly {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdHistoryMonthly {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdHistoryMonthly::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdGHistoryMonthly` (`ghistory.monthly`).
pub struct CmdGHistoryMonthly {
    meta: CommandMeta,
}

impl CmdGHistoryMonthly {
    /// C++ `CmdHistoryBase<MonthlyGHistoryStrategy>::CmdHistoryBase` (`typedef
    /// CmdGHistoryMonthly`).
    pub fn new() -> Self {
        CmdGHistoryMonthly {
            meta: CommandMeta {
                keyword: "ghistory.monthly".into(),
                usage: "task <filter> ghistory.monthly".into(),
                description: "Shows a graphical report of task history, by month".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Graphs,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdGHistoryMonthly {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdGHistoryMonthly {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdGHistoryMonthly::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdHistoryAnnual` (`history.annual`).
pub struct CmdHistoryAnnual {
    meta: CommandMeta,
}

impl CmdHistoryAnnual {
    /// C++ `CmdHistoryBase<AnnualHistoryStrategy>::CmdHistoryBase` (`typedef CmdHistoryAnnual`).
    pub fn new() -> Self {
        CmdHistoryAnnual {
            meta: CommandMeta {
                keyword: "history.annual".into(),
                usage: "task <filter> history.annual".into(),
                description: "Shows a report of task history, by year".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Graphs,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdHistoryAnnual {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdHistoryAnnual {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdHistoryAnnual::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdGHistoryAnnual` (`ghistory.annual`).
pub struct CmdGHistoryAnnual {
    meta: CommandMeta,
}

impl CmdGHistoryAnnual {
    /// C++ `CmdHistoryBase<AnnualGHistoryStrategy>::CmdHistoryBase` (`typedef CmdGHistoryAnnual`).
    pub fn new() -> Self {
        CmdGHistoryAnnual {
            meta: CommandMeta {
                keyword: "ghistory.annual".into(),
                usage: "task <filter> ghistory.annual".into(),
                description: "Shows a graphical report of task history, by year".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Graphs,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdGHistoryAnnual {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdGHistoryAnnual {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdGHistoryAnnual::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
