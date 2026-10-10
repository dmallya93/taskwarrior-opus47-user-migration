//! Rust translation of `src/columns/ColDepends.cpp`.
//!
//! Constructor metadata, the label-linking `setStyle` and `modify`;
//! `validate` is inherited from `ColumnTypeString`. The `measure` /
//! `render` overrides arrive with milestone 6.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{column_base, label_prefix, strings, Column, ModifyEnv};
use crate::error::{Result, TwError};
use crate::shared::configuration::Configuration;
use crate::shared::format::format;
use crate::shared::shared::split;
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
        parent.base.examples = vec!["1 2 10".to_string(), "[3]".to_string(), config.get("dependency.indicator")];
        ColumnDepends { parent, hyphenate: false }
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
                Some((extract_long_integer(&dep[..hyphen])?, extract_long_integer(&dep[hyphen + 1..])?))
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
                return Err(TwError::Message(format("Invalid dependency value: '{1}'", &[&dep])));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::is_valid_uuid_prefix;

    #[test]
    fn uuid_prefix_regex() {
        assert!(is_valid_uuid_prefix("abcdef01"));
        assert!(is_valid_uuid_prefix("abcdef01-234"));
        assert!(is_valid_uuid_prefix("f30cb9c3-3fc0-483f-bfb2-3bf134f00694"));
        assert!(!is_valid_uuid_prefix("abcdef0"));
        assert!(!is_valid_uuid_prefix("abcdef012"), "1..3 trailing chars do not match");
        assert!(!is_valid_uuid_prefix("ABCDEF01"));
        assert!(!is_valid_uuid_prefix("abcdef01-3fc0-483f-bfb2-3bf134f00694x"), "more than 28 trailing chars");
        assert!(!is_valid_uuid_prefix("abcdefg1"));
    }
}
