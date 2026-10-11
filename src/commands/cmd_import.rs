//! Rust translation of `src/commands/CmdImport.cpp`.
//!
//! `task import [<file> ...]`: read JSON tasks (an array, a single object,
//! or one object per line) from the named files or standard input, and add
//! each new task with `TDB2::add` or update an existing one (same UUID)
//! through `CmdModify`, printing ` add  `, ` mod  ` or ` skip ` lines.

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::commands::cmd_modify::CmdModify;
use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::shared::format::format;
use crate::shared::fs::File;
use crate::shared::json::{self, JType, JsonValue};
use crate::shared::shared::{split, with_console};
use crate::task::Task;
use crate::tdb2;
use crate::util::nontrivial;

/// C++ `std::cout << text`, flushed so it interleaves with the other
/// standard output writers.
fn cout(text: &str) {
    with_console(|_, stdout| {
        let _ = stdout.write_all(text.as_bytes());
        let _ = stdout.flush();
    });
}

/// C++ `while (std::getline (std::cin, line)) json += line + '\n';`: all of
/// standard input, with a final newline added when the last line lacks one.
fn read_stdin() -> String {
    let mut bytes = Vec::new();
    with_console(|stdin, _| {
        let _ = stdin.read_to_end(&mut bytes);
    });
    let mut json = String::from_utf8_lossy(&bytes).into_owned();
    if !json.is_empty() && !json.ends_with('\n') {
        json.push('\n');
    }
    json
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdImport` (`import`).
///
/// Deviation: C++ `uuid_occurrences` is a `std::unordered_map`, whose
/// iteration order (and so the order of the `Input contains UUID` footnotes
/// when several UUIDs repeat) depends on libstdc++ hashing. Rust keeps them
/// sorted by UUID, which is deterministic.
pub struct CmdImport {
    meta: CommandMeta,
    /// C++ `std::unordered_map<std::string, unsigned int> uuid_occurrences`.
    uuid_occurrences: RefCell<BTreeMap<String, u32>>,
}

impl CmdImport {
    /// C++ `CmdImport::CmdImport`.
    pub fn new() -> Self {
        CmdImport {
            meta: CommandMeta {
                keyword: "import".into(),
                usage: "task          import [<file> ...]".into(),
                description: "Imports JSON files".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Migration,
                ..CommandMeta::default()
            },
            uuid_occurrences: RefCell::new(BTreeMap::new()),
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `int CmdImport::import (const std::string& input)`: imports a JSON
    /// object or array of objects; when that fails with a thrown string and
    /// the input starts with `{`, falls back to one JSON object per line
    /// (all lines are parsed before any is imported). Returns the number of
    /// tasks imported.
    ///
    /// C++ quirk kept: the `catch (std::string&)` also catches errors thrown
    /// while importing the parsed object or array elements, so a failing
    /// first-line `{` input is retried line by line.
    ///
    /// Deviation: a non-object array element is cast to `json::object*` in
    /// C++ (undefined behaviour); here it imports as an empty task, which is
    /// rejected with `Cannot import an empty task.`.
    ///
    /// # Errors
    /// JSON parse errors, `Invalid JSON: {1}`, and those of
    /// [`CmdImport::import_single_task`].
    pub fn import(&self, ctx: &mut Context, input: &str) -> Result<i32> {
        let mut count = 0;
        let attempt = (|| -> Result<()> {
            let root = json::parse(input)?;
            // Single object parse. Input looks like:
            //   { ... }
            if root.jtype() == JType::Object {
                // For each object element...
                self.import_single_task(ctx, &root)?;
                count += 1;
            }
            // Multiple object array. Input looks like:
            //   [ { ... } , { ... } ]
            else if let Some(elements) = root.as_array() {
                // For each object element...
                for element in elements {
                    self.import_single_task(ctx, element)?;
                    count += 1;
                }
            }
            Ok(())
        })();

        match attempt {
            Ok(()) => {}
            // If an exception is caught, then it is because the free-form JSON
            // objects/array above failed to parse. This may be an indication that the input
            // is an old-style line-by-line set of JSON objects, because both an array of
            // objects, and a single object have failed to parse..
            //
            // Input looks like:
            //   { ... }
            //   { ... }
            Err(TwError::Message(e)) => {
                // Make a very cursory check for the old-style format.
                if !input.starts_with('{') {
                    return Err(TwError::Message(e));
                }

                // Read the entire file, so that errors do not result in a partial import.
                let mut objects: Vec<JsonValue> = Vec::new();
                for line in split(input, '\n') {
                    if !line.is_empty() {
                        let root = json::parse(&line)?;
                        if root.jtype() == JType::Object {
                            objects.push(root);
                        } else {
                            return Err(TwError::Message(format("Invalid JSON: {1}", &[&line])));
                        }
                    }
                }

                // Import the tasks.
                for root in &objects {
                    self.import_single_task(ctx, root)?;
                    count += 1;
                }
            }
            Err(other) => return Err(other),
        }

        Ok(count)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void CmdImport::importSingleTask (json::object* obj)`: adds the
    /// task, or updates the stored task with the same UUID (ignoring the
    /// generated `modified`, `entry` and `end` values), and prints the
    /// ` add  ` / ` mod  ` / ` skip ` line.
    ///
    /// # Errors
    /// `Cannot import an empty task.`, task parse and validation errors,
    /// [`CmdModify::check_consistency`] errors, storage and hook errors.
    pub fn import_single_task(&self, ctx: &mut Context, obj: &JsonValue) -> Result<()> {
        // Parse the whole thing, validate the data.
        let mut task = Task::from_json_object(ctx, obj)?;

        // An empty task is probably not intentional - at least a UUID should be included.
        if task.is_empty() {
            return Err(TwError::Message("Cannot import an empty task.".into()));
        }

        let has_generated_entry = !task.has("entry");
        let has_explicit_end = task.has("end");

        task.validate(ctx, true)?;

        let has_generated_end = !has_explicit_end && task.has("end");

        // Check whether the imported task is new or a modified existing task.
        let uuid = task.get("uuid");
        *self
            .uuid_occurrences
            .borrow_mut()
            .entry(uuid.clone())
            .or_insert(0) += 1;
        if let Some(before) = ctx.tdb2.get_by_uuid(&uuid)? {
            // We need to neglect updates from attributes with dynamic defaults
            // unless they have been explicitly specified on import.
            //
            // There are three attributes with dynamic defaults, besides uuid:
            //   - modified: Ignored in any case.
            //   - entry: Ignored if generated.
            //   - end: Ignored if generated.

            // The 'modified' attribute is ignored in any case, since if it
            // were the only difference between the tasks, it would have been
            // neglected anyway, since it is bumped on each modification.
            task.set("modified", &before.get("modified"));

            // Other generated values are replaced by values from existing task,
            // so that they are ignored on comparison.
            if has_generated_entry {
                task.set("entry", &before.get("entry"));
            }

            if has_generated_end {
                task.set("end", &before.get("end"));
            }

            if before != task {
                CmdModify::check_consistency(&before, &task)?;
                CmdModify::modify_and_update(ctx, &before, &mut task, None)?;
                cout(" mod  ");
            } else {
                cout(" skip ");
            }
        } else {
            tdb2::add(ctx, &mut task)?;
            cout(" add  ");
        }

        cout(&format!(
            "{} {}\n",
            task.get("uuid"),
            task.get("description")
        ));
        Ok(())
    }
}

impl Default for CmdImport {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdImport {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdImport::execute`.
    fn execute(&self, context: &mut Context, _output: &mut String) -> Result<i32> {
        let rc = 0;
        let mut count = 0;

        // Get filenames from command line arguments.
        let words = context
            .cli2
            .get_words(&context.config, &mut context.debug_messages);
        if words.is_empty() || (words.len() == 1 && words[0] == "-") {
            cout(&format("Importing '{1}'\n", &[&"STDIN"]));

            let json = read_stdin();

            if nontrivial(&json) {
                count = self.import(context, &json)?;
            }
        } else {
            // Import tasks from all specified files.
            for word in &words {
                let incoming = File::new(word);
                if !incoming.exists() {
                    return Err(TwError::Message(format("File '{1}' not found.", &[word])));
                }

                cout(&format("Importing '{1}'\n", &[word]));

                // Load the file.
                let json = incoming.read()?;
                if nontrivial(&json) {
                    count += self.import(context, &json)?;
                }
            }
        }

        context.footnote(&format("Imported {1} tasks.", &[&count]));

        // Warn the user about multiple occurrences of the same UUID.
        let mut duplicates = false;
        let occurrences = self.uuid_occurrences.borrow().clone();
        for (uuid, occurrences) in &occurrences {
            if *occurrences > 1 {
                duplicates = true;
                context.footnote(&format(
                    "Input contains UUID '{1}' {2} times.",
                    &[uuid, occurrences],
                ));
            }
        }
        if duplicates {
            context
                .footnote("Tasks with the same UUID have been merged. Please check the results.");
        }

        Ok(rc)
    }
}
