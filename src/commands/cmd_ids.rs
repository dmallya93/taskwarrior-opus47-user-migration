//! Rust translation of `src/commands/CmdIDs.cpp`.
//!
//! The `ids`/`uuids` family, all applying `Filter::subset`:
//! - `ids`: sorted non-zero IDs, range-compressed (`1-3 5`), plus newline.
//! - `_ids`: sorted IDs of non-deleted, non-completed tasks, one per line.
//! - `_zshids`: `id:description` (colons in the description replaced by
//!   `,`) per non-deleted, non-completed task, in filter order.
//! - `uuids`/`_uuids`: sorted UUIDs, space- or newline-separated, plus newline.
//! - `_zshuuids`: `uuid:description` per task, in filter order.
//!
//! Each clears the Context headers, as the C++ does.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::filter::Filter;
use crate::shared::shared::{join, str_replace};
use crate::task::{Status, Task};

/// C++ global `zshColonReplacement`.
pub const ZSH_COLON_REPLACEMENT: &str = ",";

/// Shared prologue of every C++ `execute` here: `Filter filter;
/// filter.subset (filtered);`.
fn filtered_tasks(context: &mut Context) -> Result<Vec<Task>> {
    let mut filter = Filter::new();
    let mut filtered: Vec<Task> = Vec::new();
    filter.subset(context, &mut filtered)?;
    Ok(filtered)
}

/// C++ `task.getStatus () != Task::deleted && task.getStatus () != Task::completed`.
fn is_not_deleted_or_completed(task: &Task) -> Result<bool> {
    let status = task.get_status()?;
    Ok(status != Status::Deleted && status != Status::Completed)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdIDs` (`ids`).
pub struct CmdIDs {
    meta: CommandMeta,
}

impl CmdIDs {
    /// C++ `CmdIDs::CmdIDs`.
    pub fn new() -> Self {
        CmdIDs {
            meta: CommandMeta {
                keyword: "ids".into(),
                usage: "task <filter> ids".into(),
                description: "Shows the IDs of matching tasks, as a range".into(),
                read_only: true,
                displays_id: true,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Metadata,
                ..CommandMeta::default()
            },
        }
    }
}

impl CmdIDs {
    /// C++ `CmdIDs::compressIds`: render sorted IDs as space-separated
    /// numbers, collapsing consecutive runs into `first-last` (`1-3 5`).
    pub fn compress_ids(ids: &[i32]) -> String {
        let mut result = String::new();

        let mut range_start: usize = 0;
        let mut range_end: usize = 0;

        let emit = |result: &mut String, range_start: usize, range_end: usize| {
            if !result.is_empty() {
                result.push(' ');
            }

            if range_start < range_end {
                *result += &format!("{}-{}", ids[range_start], ids[range_end]);
            } else {
                *result += &ids[range_start].to_string();
            }
        };

        for i in 0..ids.len() {
            if i + 1 == ids.len() {
                emit(&mut result, range_start, range_end);
            } else if ids[range_end].wrapping_add(1) == ids[i + 1] {
                range_end += 1;
            } else {
                emit(&mut result, range_start, range_end);
                range_start = i + 1;
                range_end = i + 1;
            }
        }

        result
    }
}

impl Default for CmdIDs {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdIDs {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdIDs::execute`.
    ///
    /// # Errors
    /// Those of `Filter::subset` and of `Task::getStatus`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Apply filter.
        let filtered = filtered_tasks(context)?;

        // Find number of matching tasks.
        let mut ids: Vec<i32> = filtered.iter().filter(|t| t.id != 0).map(|t| t.id).collect();

        ids.sort();
        *output = Self::compress_ids(&ids) + "\n";

        context.headers.clear();
        Ok(0)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionIds` (`_ids`).
pub struct CmdCompletionIds {
    meta: CommandMeta,
}

impl CmdCompletionIds {
    /// C++ `CmdCompletionIds::CmdCompletionIds`.
    pub fn new() -> Self {
        CmdCompletionIds {
            meta: CommandMeta {
                keyword: "_ids".into(),
                usage: "task <filter> _ids".into(),
                description: "Shows the IDs of matching tasks, in the form of a list".into(),
                read_only: true,
                displays_id: true,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCompletionIds {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionIds {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionIds::execute`.
    ///
    /// # Errors
    /// Those of `Filter::subset` and of `Task::getStatus`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Apply filter.
        let filtered = filtered_tasks(context)?;

        let mut ids: Vec<i32> = Vec::new();
        for task in &filtered {
            if is_not_deleted_or_completed(task)? {
                ids.push(task.id);
            }
        }

        ids.sort();
        *output = join("\n", &ids) + "\n";

        context.headers.clear();
        Ok(0)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdZshCompletionIds` (`_zshids`).
pub struct CmdZshCompletionIds {
    meta: CommandMeta,
}

impl CmdZshCompletionIds {
    /// C++ `CmdZshCompletionIds::CmdZshCompletionIds`.
    pub fn new() -> Self {
        CmdZshCompletionIds {
            meta: CommandMeta {
                keyword: "_zshids".into(),
                usage: "task <filter> _zshids".into(),
                description: "Shows the IDs and descriptions of matching tasks".into(),
                read_only: true,
                displays_id: true,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdZshCompletionIds {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdZshCompletionIds {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdZshCompletionIds::execute`.
    ///
    /// # Errors
    /// Those of `Filter::subset` and of `Task::getStatus`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Apply filter.
        let filtered = filtered_tasks(context)?;

        let mut out = String::new();
        for task in &filtered {
            if is_not_deleted_or_completed(task)? {
                out += &format!(
                    "{}:{}\n",
                    task.id,
                    str_replace(&task.get("description"), ":", ZSH_COLON_REPLACEMENT)
                );
            }
        }

        *output = out;

        context.headers.clear();
        Ok(0)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdUUIDs` (`uuids`).
pub struct CmdUUIDs {
    meta: CommandMeta,
}

impl CmdUUIDs {
    /// C++ `CmdUUIDs::CmdUUIDs`.
    pub fn new() -> Self {
        CmdUUIDs {
            meta: CommandMeta {
                keyword: "uuids".into(),
                usage: "task <filter> uuids".into(),
                description: "Shows the UUIDs of matching tasks, as a space-separated list".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Metadata,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdUUIDs {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdUUIDs {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdUUIDs::execute`.
    ///
    /// # Errors
    /// Those of `Filter::subset` and of `Task::getStatus`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Apply filter.
        let filtered = filtered_tasks(context)?;

        let mut uuids: Vec<String> = filtered.iter().map(|t| t.get("uuid")).collect();

        uuids.sort();
        *output = join(" ", &uuids) + "\n";

        context.headers.clear();
        Ok(0)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionUuids` (`_uuids`).
pub struct CmdCompletionUuids {
    meta: CommandMeta,
}

impl CmdCompletionUuids {
    /// C++ `CmdCompletionUuids::CmdCompletionUuids`.
    pub fn new() -> Self {
        CmdCompletionUuids {
            meta: CommandMeta {
                keyword: "_uuids".into(),
                usage: "task <filter> _uuids".into(),
                description: "Shows the UUIDs of matching tasks, as a list".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCompletionUuids {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionUuids {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionUuids::execute`.
    ///
    /// # Errors
    /// Those of `Filter::subset` and of `Task::getStatus`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Apply filter.
        let filtered = filtered_tasks(context)?;

        let mut uuids: Vec<String> = filtered.iter().map(|t| t.get("uuid")).collect();

        uuids.sort();
        *output = join("\n", &uuids) + "\n";

        context.headers.clear();
        Ok(0)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdZshCompletionUuids` (`_zshuuids`).
pub struct CmdZshCompletionUuids {
    meta: CommandMeta,
}

impl CmdZshCompletionUuids {
    /// C++ `CmdZshCompletionUuids::CmdZshCompletionUuids`.
    pub fn new() -> Self {
        CmdZshCompletionUuids {
            meta: CommandMeta {
                keyword: "_zshuuids".into(),
                usage: "task <filter> _zshuuids".into(),
                description: "Shows the UUIDs and descriptions of matching tasks".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdZshCompletionUuids {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdZshCompletionUuids {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdZshCompletionUuids::execute`.
    ///
    /// # Errors
    /// Those of `Filter::subset` and of `Task::getStatus`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Apply filter.
        let filtered = filtered_tasks(context)?;

        let mut out = String::new();
        for task in &filtered {
            out += &format!(
                "{}:{}\n",
                task.get("uuid"),
                str_replace(&task.get("description"), ":", ZSH_COLON_REPLACEMENT)
            );
        }

        *output = out;

        context.headers.clear();
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_ids_cases() {
        assert_eq!(CmdIDs::compress_ids(&[]), "");
        assert_eq!(CmdIDs::compress_ids(&[1]), "1");
        assert_eq!(CmdIDs::compress_ids(&[1, 2]), "1-2");
        assert_eq!(CmdIDs::compress_ids(&[1, 2, 3, 5]), "1-3 5");
        assert_eq!(CmdIDs::compress_ids(&[1, 3, 5]), "1 3 5");
        assert_eq!(CmdIDs::compress_ids(&[1, 3, 4, 5, 7, 8, 10]), "1 3-5 7-8 10");
    }
}
