//! Rust translation of `src/TF2.h` / `src/TF2.cpp`.
//!
//! [`Tf2`] represents a single 2.x-style file in the task database. It is
//! only used for importing tasks from 2.x (`import-v2`, CmdImportV2.cpp). It
//! only reads format 4, based on a stripped-down version of the TF2 class
//! from v2.6.2.
//!
//! Narrow inputs (instead of `Context::getContext ()`):
//! * `config.getBoolean ("locking")` in `load_lines` is the `locking`
//!   argument of [`Tf2::get_tasks`] / [`Tf2::load_tasks`] /
//!   [`Tf2::load_lines`].
//! * `Context::getContext ().time_load_us += ...` in `load_tasks` adds to
//!   [`Tf2::time_load_us`], which the caller folds into
//!   `Context::time_load_us` (as [`crate::tdb2::Tdb2::time_load_us`]).
//! * The C++ `throw std::string` errors are [`TwError::Message`] with the
//!   same text.

use std::collections::BTreeMap;

use crate::error::{Result, TwError};
use crate::legacy::legacy_attribute_map;
use crate::shared::format::format;
use crate::shared::fs::File;
use crate::shared::json;
use crate::shared::pig::Pig;
use crate::shared::shared::{split, str_replace};
use crate::shared::timer::Timer;
use crate::task::Task;

/// One parsed FF4 record: C++ `std::map<std::string, std::string>`.
pub type Tf2Task = BTreeMap<String, String>;

////////////////////////////////////////////////////////////////////////////////
/// C++ `class TF2`.
#[derive(Debug, Default)]
pub struct Tf2 {
    /// C++ `_loaded_tasks`.
    pub loaded_tasks: bool,
    /// C++ `_loaded_lines`.
    pub loaded_lines: bool,
    /// C++ `_tasks`.
    pub tasks: Vec<Tf2Task>,
    /// C++ `_lines`.
    pub lines: Vec<String>,
    /// C++ `_file`.
    pub file: File,
    /// Microseconds spent in [`load_tasks`](Self::load_tasks); C++ adds
    /// these directly to `Context::time_load_us`.
    pub time_load_us: i64,
}

impl Tf2 {
    /// C++ `TF2::TF2`.
    pub fn new() -> Self {
        Self::default()
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void TF2::target (const std::string&)`.
    pub fn target(&mut self, f: &str) {
        self.file = File::new(f);
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `const std::vector<std::map<...>>& TF2::get_tasks ()`.
    ///
    /// # Errors
    /// A parse error from [`load_tasks`](Self::load_tasks), or a file error
    /// from [`load_lines`](Self::load_lines).
    pub fn get_tasks(&mut self, locking: bool) -> Result<&[Tf2Task]> {
        if !self.loaded_tasks {
            self.load_tasks(locking)?;
        }

        Ok(&self.tasks)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `std::map<...> TF2::load_task (const std::string&)`: attempt an
    /// FF4 parse.
    ///
    /// Note that FF1, FF2, FF3, and JSON are no longer supported.
    ///
    /// ```text
    /// start --> [ --> Att --> ] --> end
    ///              ^       |
    ///              +-------+
    /// ```
    ///
    /// As in C++, the parsed text is `input.substr (1, ending_bracket)`, so it
    /// keeps the final `]` (which the attribute loop then consumes), and a
    /// line starting with `[` but without any `]` yields no attributes.
    ///
    /// # Errors
    /// `Record not recognized as format 4.`, `Empty record in input.` or
    /// `Unrecognized characters at end of line.`
    pub fn load_task(&self, input: &str) -> Result<Tf2Task> {
        let mut data = Tf2Task::new();

        // File format version 4, from 2009-5-16 - now, v1.7.1+
        // This is the parse format tried first, because it is most used.
        data.clear();

        // C++ `input[0]` is '\0' for an empty string.
        if input.as_bytes().first() == Some(&b'[') {
            // Not using Pig to parse here (which would be idiomatic), because we
            // don't need to differentiate betwen utf-8 and normal characters.
            // Pig's scanning the string can be expensive.
            if let Some(ending_bracket) = input.rfind(']') {
                // C++ `input.substr (1, ending_bracket)`: `ending_bracket`
                // characters from 1, clamped to the end of `input`.
                let end = (1 + ending_bracket).min(input.len());
                let line = &input[1..end];

                if line.is_empty() {
                    return Err(TwError::Message("Empty record in input.".into()));
                }

                let mut att_line = Pig::new(line);
                let mut name = String::new();
                let mut value = String::new();
                while !att_line.eos() {
                    if att_line.get_until_ascii(b':', &mut name)
                        && att_line.skip(':')
                        && att_line.get_quoted('"', &mut value)
                    {
                        legacy_attribute_map(&mut name);

                        data.insert(name.clone(), self.decode(&json::decode(&value)));
                    }

                    att_line.skip(' ');
                }

                let mut remainder = String::new();
                att_line.get_remainder(&mut remainder);
                if !remainder.is_empty() {
                    return Err(TwError::Message(
                        "Unrecognized characters at end of line.".into(),
                    ));
                }
            }
        } else {
            return Err(TwError::Message(
                "Record not recognized as format 4.".into(),
            ));
        }

        // for compatibility, include all tags in `tags` as `tag_..` attributes
        if let Some(tags) = data.get("tags").cloned() {
            for tag in split(&tags, ',') {
                data.insert(Task::tag2attr(&tag), "x".into());
            }
        }

        // same for `depends` / `dep_..`
        if let Some(depends) = data.get("depends").cloned() {
            for dep in split(&depends, ',') {
                data.insert(Task::dep2attr(&dep), "x".into());
            }
        }

        Ok(data)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `const std::string TF2::decode (const std::string&) const`:
    /// decode values after parse.
    ///
    /// ```text
    ///   [  <- &open;
    ///   ]  <- &close;
    /// ```
    pub fn decode(&self, value: &str) -> String {
        if !value.contains('&') {
            return value.to_string();
        }

        let modified = str_replace(value, "&open;", "[");
        str_replace(&modified, "&close;", "]")
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void TF2::load_tasks ()`.
    ///
    /// # Errors
    /// A [`load_task`](Self::load_task) error, with
    /// ` in <file> at line <n>` appended; or a file error from
    /// [`load_lines`](Self::load_lines).
    pub fn load_tasks(&mut self, locking: bool) -> Result<()> {
        let timer = Timer::new();

        if !self.loaded_lines {
            self.load_lines(locking)?;
        }

        // Reduce unnecessary allocations/copies.
        // Calling it on _tasks is the right thing to do even when from_gc is set.
        self.tasks.reserve(self.lines.len());

        let mut line_number = 0; // Used for error message in catch block.
        for line in &self.lines {
            line_number += 1;
            match self.load_task(line) {
                Ok(task) => self.tasks.push(task),
                Err(TwError::Message(e)) => {
                    return Err(TwError::Message(
                        e + &format(" in {1} at line {2}", &[&self.file.data, &line_number]),
                    ));
                }
                Err(e) => return Err(e),
            }
        }

        self.loaded_tasks = true;

        self.time_load_us += timer.total_us() as i64;
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void TF2::load_lines ()`.
    ///
    /// # Errors
    /// `File::open` / `File::close` failures (e.g. `Insufficient permissions
    /// for '<file>'.`).
    pub fn load_lines(&mut self, locking: bool) -> Result<()> {
        if self.file.open()? {
            if locking {
                self.file.lock();
            }

            self.lines = self.file.read_lines();
            self.file.close()?;
            self.loaded_lines = true;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Result<Tf2Task> {
        Tf2::new().load_task(input)
    }

    fn map(pairs: &[(&str, &str)]) -> Tf2Task {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn load_task_parses_ff4() {
        let task = parse(
            r#"[description:"bing" due:"1734480000" status:"pending" uuid:"ad7f7585-bff3-4b57-a116-abfc9f71ee4a"]"#,
        )
        .unwrap();
        assert_eq!(
            task,
            map(&[
                ("description", "bing"),
                ("due", "1734480000"),
                ("status", "pending"),
                ("uuid", "ad7f7585-bff3-4b57-a116-abfc9f71ee4a"),
            ])
        );
    }

    #[test]
    fn load_task_decodes_json_and_brackets() {
        let task =
            parse(r#"[description:"a &open;b&close; \"q\" \u00e9\tx" annotation_1:"&amp;"]"#)
                .unwrap();
        assert_eq!(task["description"], "a [b] \"q\" \u{e9}\tx");
        // Only &open; and &close; are decoded.
        assert_eq!(task["annotation_1"], "&amp;");
    }

    #[test]
    fn load_task_maps_legacy_attributes() {
        let task = parse(r#"[modification:"1" description:"x"]"#).unwrap();
        assert_eq!(task, map(&[("description", "x"), ("modified", "1")]));
    }

    #[test]
    fn load_task_adds_tag_and_dep_attributes() {
        let task = parse(r#"[tags:"a,b" depends:"u1,u2"]"#).unwrap();
        assert_eq!(
            task,
            map(&[
                ("dep_u1", "x"),
                ("dep_u2", "x"),
                ("depends", "u1,u2"),
                ("tag_a", "x"),
                ("tag_b", "x"),
                ("tags", "a,b"),
            ])
        );
        // An empty list adds nothing (C++ `split` of "" yields no tokens).
        assert_eq!(parse(r#"[tags:""]"#).unwrap(), map(&[("tags", "")]));
    }

    #[test]
    fn load_task_errors() {
        for input in ["", "x", " [a:\"b\"]", "description:\"x\""] {
            assert_eq!(
                parse(input).unwrap_err().to_string(),
                "Record not recognized as format 4.",
                "{input:?}"
            );
        }
    }

    #[test]
    fn load_task_lenient_cases() {
        // No closing bracket: no attributes, no error.
        assert_eq!(parse(r#"[a:"b""#).unwrap(), Tf2Task::new());
        // `[]`: substr (1, 1) is "]", not empty.
        assert_eq!(parse("[]").unwrap(), Tf2Task::new());
        // Malformed attributes are skipped.
        assert_eq!(parse("[foo bar]").unwrap(), Tf2Task::new());
        // A failed quote leaves the cursor at the value, which becomes part
        // of the next name, as with C++ Pig.
        assert_eq!(parse(r#"[a:b c:"d"]"#).unwrap(), map(&[("b c", "d")]));
        assert_eq!(parse(r#"[a:"b" c:d]"#).unwrap(), map(&[("a", "b")]));
        // Text after the last bracket is ignored.
        assert_eq!(parse(r#"[a:"b"] junk"#).unwrap(), map(&[("a", "b")]));
        // A later duplicate attribute wins.
        assert_eq!(parse(r#"[a:"1" a:"2"]"#).unwrap(), map(&[("a", "2")]));
    }

    #[test]
    fn decode_brackets() {
        let tf2 = Tf2::new();
        assert_eq!(tf2.decode("plain"), "plain");
        assert_eq!(tf2.decode("&open;x&close;&open;"), "[x][");
        assert_eq!(tf2.decode("&amp;"), "&amp;");
    }

    #[test]
    fn load_tasks_from_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("pending.data");
        std::fs::write(&path, "[description:\"one\"]\n[description:\"two\"]\n").unwrap();
        let mut tf2 = Tf2::new();
        tf2.target(path.to_str().unwrap());
        let tasks = tf2.get_tasks(true).unwrap().to_vec();
        assert_eq!(
            tasks,
            vec![
                map(&[("description", "one")]),
                map(&[("description", "two")])
            ]
        );
        assert!(tf2.loaded_lines && tf2.loaded_tasks);
        // Cached: a second call does not re-read.
        std::fs::write(&path, "").unwrap();
        assert_eq!(tf2.get_tasks(false).unwrap().len(), 2);
    }

    #[test]
    fn load_tasks_error_names_file_and_line() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("completed.data");
        std::fs::write(&path, "[description:\"one\"]\n\n[description:\"two\"]\n").unwrap();
        let name = path.to_str().unwrap().to_string();
        let mut tf2 = Tf2::new();
        tf2.target(&name);
        assert_eq!(
            tf2.get_tasks(false).unwrap_err().to_string(),
            format!("Record not recognized as format 4. in {name} at line 2")
        );
        assert!(!tf2.loaded_tasks);
    }

    #[test]
    fn load_lines_without_target_reads_nothing() {
        let mut tf2 = Tf2::new();
        tf2.load_lines(false).unwrap();
        assert!(!tf2.loaded_lines);
        assert!(tf2.get_tasks(false).unwrap().is_empty());
    }
}
