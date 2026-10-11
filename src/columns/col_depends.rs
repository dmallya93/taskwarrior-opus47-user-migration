//! Rust translation of `src/columns/ColDepends.cpp`.
//!
//! Constructor metadata, the label-linking `setStyle`, `measure` /
//! `render` (the `list`, `count` and `indicator` styles over the pending
//! blocking tasks, read through [`RenderEnv::dependency_tasks`]) and
//! `modify`; `validate` is inherited from `ColumnTypeString`.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{
    column_base, label_prefix, render_string_left, render_string_right, strings, Column, ModifyEnv,
    RenderEnv,
};
use crate::error::{Result, TwError};
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::shared::format::format;
use crate::shared::shared::{join, split, wrap_text};
use crate::shared::utf8::utf8_width;
use crate::task::Task;
use crate::util::extract_long_integer;

/// C++ `#define STRING_COLUMN_LABEL_DEP "Depends"`.
const STRING_COLUMN_LABEL_DEP: &str = "Depends";

/// C++ `class ColumnDepends : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDepends {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
    /// C++ `_hyphenate` (always `false`).
    pub hyphenate: bool,
}

impl ColumnDepends {
    /// C++ `ColumnDepends::ColumnDepends ()`.
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "depends".to_string();
        parent.base.style = "list".to_string();
        parent.base.label = STRING_COLUMN_LABEL_DEP.to_string();
        parent.base.styles = strings(&["list", "count", "indicator"]);
        parent.base.examples = vec![
            "1 2 10".to_string(),
            "[3]".to_string(),
            config.get("dependency.indicator"),
        ];
        ColumnDepends {
            parent,
            hyphenate: false,
        }
    }
}

/// C++ `std::regex_match (dep, std::regex ("[a-f0-9]{8}([a-f0-9-]{4,28})?"))`.
fn is_valid_uuid_prefix(dep: &str) -> bool {
    let bytes = dep.as_bytes();
    if bytes.len() < 8 {
        return false;
    }
    let hex = |c: &u8| c.is_ascii_digit() || (b'a'..=b'f').contains(c);
    if !bytes[..8].iter().all(hex) {
        return false;
    }
    let rest = &bytes[8..];
    rest.is_empty() || ((4..=28).contains(&rest.len()) && rest.iter().all(|c| hex(c) || *c == b'-'))
}

impl Column for ColumnDepends {
    column_base!(parent.base);

    /// C++ `void ColumnDepends::setStyle (const std::string& value)`:
    /// overridden so that style <----> label are linked (the `indicator`
    /// label is cut to the width of `rc.dependency.indicator`).
    fn set_style(&mut self, value: &str, config: &Configuration) -> Result<()> {
        self.base_mut().set_style(value)?;
        let base = self.base_mut();
        if base.style == "indicator" && base.label == STRING_COLUMN_LABEL_DEP {
            base.label = label_prefix(&base.label, config.get("dependency.indicator").len());
        } else if base.style == "count" && base.label == STRING_COLUMN_LABEL_DEP {
            base.label = "Dep".to_string();
        }
        Ok(())
    }

    fn validate(&self, input: &str) -> bool {
        ColumnTypeString::validate_string(input)
    }

    /// C++ `void ColumnDepends::measure (Task& task, unsigned int& minimum,
    /// unsigned int& maximum)`: `(0, 0)` without pending blocking tasks;
    /// `indicator` is the width of `rc.dependency.indicator`, `count` that
    /// of `[<n>]`, and `list` ranges from the widest single ID to all IDs
    /// joined by spaces.
    ///
    /// # Errors
    /// Those of [`RenderEnv::dependency_tasks`].
    fn measure(&self, task: &mut Task, env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let (mut minimum, mut maximum) = (0, 0);
        let deptasks = env.dependency_tasks(task)?;

        if !deptasks.is_empty() {
            let style = self.style();
            if style == "indicator" {
                minimum = utf8_width(&env.config().get("dependency.indicator")) as usize;
                maximum = minimum;
            } else if style == "count" {
                minimum = 2 + (deptasks.len() as i32).to_string().len();
                maximum = minimum;
            } else if style == "default" || style == "list" {
                minimum = 0;

                let blocking_ids: Vec<i32> = deptasks.iter().map(|i| i.id).collect();

                let all = join(" ", &blocking_ids);
                maximum = all.len();

                for i in &deptasks {
                    let length = i.id.to_string().len();
                    if length > minimum {
                        minimum = length;
                    }
                }
            }
        }
        Ok((minimum, maximum))
    }

    /// C++ `void ColumnDepends::render (std::vector<std::string>& lines,
    /// Task& task, int width, Color& color)`: nothing without pending
    /// blocking tasks; `indicator` and `count` (`[<n>]`) are right-justified,
    /// `list` is the space-joined IDs wrapped to `width` (no hyphenation),
    /// one left-justified line each.
    ///
    /// # Errors
    /// Those of [`RenderEnv::dependency_tasks`].
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        env: &mut dyn RenderEnv,
    ) -> Result<()> {
        let deptasks = env.dependency_tasks(task)?;

        if !deptasks.is_empty() {
            let style = self.style();
            if style == "indicator" {
                render_string_right(
                    lines,
                    width,
                    color,
                    &env.config().get("dependency.indicator"),
                );
            } else if style == "count" {
                render_string_right(lines, width, color, &format!("[{}]", deptasks.len() as i32));
            } else if style == "default" || style == "list" {
                let blocking_ids: Vec<i32> = deptasks.iter().map(|t| t.id).collect();

                let combined = join(" ", &blocking_ids);

                let mut all = Vec::new();
                wrap_text(&mut all, &combined, width as i32, self.hyphenate);

                for i in &all {
                    render_string_left(lines, width, color, i);
                }
            }
        }
        Ok(())
    }

    /// C++ `void ColumnDepends::modify (Task& task, const std::string&
    /// value)`: applies or removes (`-` prefix) each comma-separated
    /// dependency in turn: a full UUID, a UUID prefix (looked up in TDB2), an
    /// ID range `lo-hi`, or a single ID.
    ///
    /// # Errors
    /// `Dependency could not be set - task with UUID '{1}' does not exist.`,
    /// `Invalid dependency value: '{1}'`, and the errors of the `Task`
    /// dependency operations.
    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        // Apply or remove dendencies in turn.
        for mut dep in split(value, ',') {
            let mut removal = false;
            if dep.starts_with('-') {
                removal = true;
                dep = dep[1..].to_string();
            }

            let hyphen = dep.find('-');

            // UUID
            if dep.len() >= 8 && is_valid_uuid_prefix(&dep) {
                // Full UUID, can be added directly
                if dep.len() == 36 {
                    if removal {
                        task.remove_dependency(&dep)?;
                    } else {
                        env.add_dependency(task, &dep)?;
                    }
                }
                // Short UUID, need to look up full form
                else {
                    match env.get_task(&dep)? {
                        Some(loaded_task) => {
                            if removal {
                                task.remove_dependency(&loaded_task.get("uuid"))?;
                            } else {
                                env.add_dependency(task, &loaded_task.get("uuid"))?;
                            }
                        }
                        None => {
                            return Err(TwError::Message(format(
                                "Dependency could not be set - task with UUID '{1}' does not exist.",
                                &[&dep],
                            )));
                        }
                    }
                }
            }
            // ID range
            else if let Some((lower, upper)) = hyphen.and_then(|hyphen| {
                Some((
                    extract_long_integer(&dep[..hyphen])?,
                    extract_long_integer(&dep[hyphen + 1..])?,
                ))
            }) {
                for i in lower..=upper {
                    // C++ passes the `long` to `addDependency (int)`.
                    if removal {
                        env.remove_dependency_id(task, i as i32)?;
                    } else {
                        env.add_dependency_id(task, i as i32)?;
                    }
                }
            }
            // Simple ID
            else if let Some(lower) = extract_long_integer(&dep) {
                if removal {
                    env.remove_dependency_id(task, lower as i32)?;
                } else {
                    env.add_dependency_id(task, lower as i32)?;
                }
            } else {
                return Err(TwError::Message(format(
                    "Invalid dependency value: '{1}'",
                    &[&dep],
                )));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::columns::column::{factory, FakeRenderEnv};

    const BLOCKER: [&str; 3] = [
        "00000000-0000-4000-8000-000000000001",
        "00000000-0000-4000-8000-000000000002",
        "00000000-0000-4000-8000-000000000003",
    ];

    /// A fake whose replica holds blockers with IDs 3, 12 and 100.
    fn env() -> FakeRenderEnv {
        let mut env = FakeRenderEnv::with_config(&[("dependency.indicator", "D")]);
        for (uuid, id) in BLOCKER.iter().zip([3, 12, 100]) {
            let mut t = Task::new();
            t.set("uuid", uuid);
            t.id = id;
            env.tasks.push(t);
        }
        env
    }

    /// A task depending on the first `n` blockers.
    fn blocked(n: usize) -> Task {
        let mut task = Task::new();
        for uuid in &BLOCKER[..n] {
            task.set(&format!("dep_{uuid}"), "x");
        }
        task
    }

    /// `(measure, render lines)` of `column` in report `list`.
    fn run(
        column: &str,
        task: &mut Task,
        width: usize,
        color: &Color,
        env: &mut FakeRenderEnv,
    ) -> ((usize, usize), Vec<String>) {
        let c = factory(column, "list", &env.config.clone()).unwrap();
        let m = c.measure(task, env).unwrap();
        let mut lines = Vec::new();
        c.render(&mut lines, task, width, color, env).unwrap();
        (m, lines)
    }

    #[test]
    fn list_style() {
        let mut env = env();
        let none = Color::new();
        let mut task = blocked(3);
        assert_eq!(
            run("depends", &mut task, 9, &none, &mut env),
            ((3, 8), vec!["3 12 100 ".to_string()])
        );
        assert_eq!(
            run("depends.list", &mut task, 8, &none, &mut env),
            ((3, 8), vec!["3 12 100".to_string()])
        );
        // Wrapped at word boundaries, each line padded to the width.
        assert_eq!(
            run("depends.list", &mut task, 4, &none, &mut env),
            ((3, 8), vec!["3 12".to_string(), "100 ".to_string()])
        );
        assert_eq!(
            run("depends.list", &mut task, 3, &none, &mut env).1,
            vec!["3  ".to_string(), "12 ".to_string(), "100".to_string()]
        );
        let red = Color::parse("red").unwrap();
        let mut one = blocked(1);
        assert_eq!(
            run("depends", &mut one, 2, &red, &mut env),
            ((1, 1), vec!["\x1b[31m3 \x1b[0m".to_string()])
        );
    }

    #[test]
    fn count_and_indicator_styles() {
        let mut env = env();
        let none = Color::new();
        let mut task = blocked(2);
        assert_eq!(
            run("depends.count", &mut task, 3, &none, &mut env),
            ((3, 3), vec!["[2]".to_string()])
        );
        assert_eq!(
            run("depends.count", &mut task, 5, &none, &mut env),
            ((3, 3), vec!["  [2]".to_string()])
        );
        assert_eq!(
            run("depends.indicator", &mut task, 1, &none, &mut env),
            ((1, 1), vec!["D".to_string()])
        );
        assert_eq!(
            run("depends.indicator", &mut task, 3, &none, &mut env),
            ((1, 1), vec!["  D".to_string()])
        );
        env.config.set("dependency.indicator", "\u{2192}\u{2192}");
        assert_eq!(
            run("depends.indicator", &mut task, 2, &none, &mut env),
            ((2, 2), vec!["\u{2192}\u{2192}".to_string()])
        );
    }

    #[test]
    fn no_pending_blockers() {
        let mut env = env();
        let none = Color::new();
        // No dependencies, or only ones that are not pending (not returned
        // by the replica): nothing in any style.
        let mut unblocked = Task::new();
        let mut gone = Task::new();
        gone.set("dep_00000000-0000-4000-8000-0000000000ff", "x");
        for style in ["list", "count", "indicator"] {
            assert_eq!(
                run(
                    &format!("depends.{style}"),
                    &mut unblocked,
                    5,
                    &none,
                    &mut env
                ),
                ((0, 0), vec![])
            );
            assert_eq!(
                run(&format!("depends.{style}"), &mut gone, 5, &none, &mut env),
                ((0, 0), vec![])
            );
        }
    }

    #[test]
    fn uuid_prefix_regex() {
        assert!(is_valid_uuid_prefix("abcdef01"));
        assert!(is_valid_uuid_prefix("abcdef01-234"));
        assert!(is_valid_uuid_prefix("f30cb9c3-3fc0-483f-bfb2-3bf134f00694"));
        assert!(!is_valid_uuid_prefix("abcdef0"));
        assert!(
            !is_valid_uuid_prefix("abcdef012"),
            "1..3 trailing chars do not match"
        );
        assert!(!is_valid_uuid_prefix("ABCDEF01"));
        assert!(
            !is_valid_uuid_prefix("abcdef01-3fc0-483f-bfb2-3bf134f00694x"),
            "more than 28 trailing chars"
        );
        assert!(!is_valid_uuid_prefix("abcdefg1"));
    }
}
