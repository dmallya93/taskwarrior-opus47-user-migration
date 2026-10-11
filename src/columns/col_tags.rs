//! Rust translation of `src/columns/ColTags.cpp`.
//!
//! Constructor metadata, the label-linking `setStyle`, `modify` and the
//! `measure` / `render` overrides (`list` / `indicator` / `count`);
//! `validate` is inherited from `ColumnTypeString`.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{
    column_base, label_prefix, render_string_left, render_string_right, strings, Column, ModifyEnv, RenderEnv,
    MODIFICATION_LABEL,
};
use crate::error::Result;
use crate::lexer::{Lexer, Type};
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::shared::format::format_value;
use crate::shared::shared::{join, split, wrap_text};
use crate::shared::utf8::utf8_width;
use crate::task::Task;

/// C++ `class ColumnTags : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnTags {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
    /// C++ `_hyphenate` (always `false`).
    pub hyphenate: bool,
}

impl ColumnTags {
    /// C++ `ColumnTags::ColumnTags ()`.
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "tags".to_string();
        parent.base.style = "list".to_string();
        parent.base.label = "Tags".to_string();
        parent.base.styles = strings(&["list", "indicator", "count"]);
        parent.base.examples = vec!["home @chore next".to_string(), config.get("tag.indicator"), "[2]".to_string()];
        ColumnTags { parent, hyphenate: false }
    }
}

impl Column for ColumnTags {
    column_base!(parent.base);

    /// C++ `void ColumnTags::setStyle (const std::string& value)`:
    /// overridden so that style <----> label are linked (the `indicator`
    /// label is cut to the width of `rc.tag.indicator`).
    fn set_style(&mut self, value: &str, config: &Configuration) -> Result<()> {
        self.base_mut().set_style(value)?;
        let base = self.base_mut();
        if base.style == "indicator" && base.label == "Tags" {
            base.label = label_prefix(&base.label, config.get("tag.indicator").len());
        } else if base.style == "count" && base.label == "Tags" {
            base.label = "Tag".to_string();
        }
        Ok(())
    }

    fn validate(&self, input: &str) -> bool {
        ColumnTypeString::validate_string(input)
    }

    /// C++ `void ColumnTags::modify (Task& task, const std::string& value)`:
    /// replaces the tags with the comma-separated list in `value` (evaluated
    /// first when it starts with a DOM reference), mentioning special tags.
    ///
    /// # Errors
    /// Any Eval error.
    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        let label = MODIFICATION_LABEL;
        let mut tags: Vec<String> = Vec::new();

        // If it's a DOM ref, eval it first.
        let mut lexer = Lexer::new(value);
        let mut dom_ref = String::new();
        let mut type_ = Type::word;
        let commasep = if lexer.token(&mut dom_ref, &mut type_) && type_ == Type::dom {
            env.evaluate_dom_expression(value)?.to_string()
        } else {
            value.to_string()
        };

        for tag in split(&commasep, ',') {
            env.debug(&format!("{label}tags <-- '{tag}'"));
            env.feedback_special_tags(task, &tag);
            tags.push(tag);
        }

        task.set_tags(&tags);
        Ok(())
    }

    /// C++ `void ColumnTags::measure (Task& task, unsigned int& minimum,
    /// unsigned int& maximum)`: for `list` the widest tag (minimum) and the
    /// width of all tags space-separated (maximum); the width of
    /// `rc.tag.indicator`; 3 for `count`. `(0, 0)` without tags.
    fn measure(&self, task: &mut Task, env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let mut minimum: usize = 0;
        let mut maximum: usize = 0;
        if task.get_tag_count() > 0 {
            let style = self.style();
            if style == "indicator" {
                minimum = utf8_width(&env.config().get("tag.indicator")) as usize;
                maximum = minimum;
            } else if style == "count" {
                minimum = 3;
                maximum = 3;
            } else if style == "default" || style == "list" {
                let tags = task.get_tags();
                // Find the widest tag (minimum) and width of all tags together (maximum)
                maximum = tags.len().wrapping_sub(1);
                for tag in &tags {
                    let length = utf8_width(tag) as usize;
                    maximum = maximum.wrapping_add(length);
                    if length > minimum {
                        minimum = length;
                    }
                }
            }
        }
        Ok((minimum, maximum))
    }

    /// C++ `void ColumnTags::render (std::vector<std::string>& lines, Task&
    /// task, int width, Color& color)`: several tags are sorted (byte
    /// order, `std::sort`), joined by spaces and wrapped at `width`; a single
    /// tag is rendered as is.
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        env: &mut dyn RenderEnv,
    ) -> Result<()> {
        let mut all = task.get_tags();
        if !all.is_empty() {
            let style = self.style();
            if style == "default" || style == "list" {
                if all.len() > 1 {
                    all.sort();
                    let tags = join(" ", &all);
                    all.clear();
                    wrap_text(&mut all, &tags, width as i32, self.hyphenate);
                    for i in &all {
                        render_string_left(lines, width, color, i);
                    }
                } else {
                    render_string_left(lines, width, color, &all[0]);
                }
            } else if style == "indicator" {
                let indicator = env.config().get("tag.indicator");
                render_string_right(lines, width, color, &indicator);
            } else if style == "count" {
                render_string_right(lines, width, color, &format!("[{}]", format_value(&(all.len() as i32))));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::columns::column::{factory, FakeRenderEnv};

    fn run(style: &str, tags: &[&str], width: usize) -> ((usize, usize), Vec<String>) {
        let mut config = Configuration::new();
        config.set("tag.indicator", "+");
        let column = factory(&format!("tags.{style}"), "list", &config).unwrap();
        let mut env = FakeRenderEnv::with_config(&[("tag.indicator", "+")]);
        let mut task = Task::new();
        let tags: Vec<String> = tags.iter().map(|s| s.to_string()).collect();
        task.set_tags(&tags);
        let m = column.measure(&mut task, &mut env).unwrap();
        let mut lines = Vec::new();
        column.render(&mut lines, &mut task, width, &Color::new(), &mut env).unwrap();
        (m, lines)
    }

    #[test]
    fn list() {
        // Sorted, joined, wrapped.
        assert_eq!(run("list", &["next", "home", "@chore"], 16), ((6, 16), vec!["@chore home next".to_string()]));
        assert_eq!(
            run("default", &["next", "home", "@chore"], 11),
            ((6, 16), vec!["@chore home".to_string(), "next       ".to_string()])
        );
        // A single tag is not wrapped.
        assert_eq!(run("list", &["longtag"], 7), ((7, 7), vec!["longtag".to_string()]));
        assert_eq!(run("list", &["界界"], 6), ((4, 4), vec!["界界  ".to_string()]));
        assert_eq!(run("list", &[], 4), ((0, 0), vec![]));
    }

    #[test]
    fn indicator() {
        assert_eq!(run("indicator", &["a", "b"], 2), ((1, 1), vec![" +".to_string()]));
        assert_eq!(run("indicator", &[], 2), ((0, 0), vec![]));
    }

    #[test]
    fn count() {
        assert_eq!(run("count", &["a", "b"], 4), ((3, 3), vec![" [2]".to_string()]));
        assert_eq!(run("count", &[], 4), ((0, 0), vec![]));
    }
}
