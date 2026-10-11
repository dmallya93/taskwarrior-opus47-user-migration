//! Rust translation of `src/commands/CmdContext.cpp`.
//!
//! `task context [<name> | define | delete | list | none | show]` manages
//! the `context.<name>.read` / `context.<name>.write` definitions and the
//! active `context` setting in the rc file; `task _context` lists the
//! defined context names for completion.
//!
//! Each struct below is one C++ class from `CmdContext.cpp`, carrying the exact
//! metadata of its C++ constructor. The rc file is edited only through
//! [`CmdConfig::set_config_variable`] / [`CmdConfig::unset_config_variable`],
//! which (as in C++) rewrite the file but leave the in-memory configuration
//! untouched, so every lookup below sees the configuration the command
//! started with.

use std::collections::BTreeSet;

use crate::cli2::{with_cli2, A2};
use crate::commands::cmd_config::CmdConfig;
use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::filter::Filter;
use crate::lexer::Type as LexerType;
use crate::rules::colorize_footnote;
use crate::shared::configuration::Configuration;
use crate::shared::format::format;
use crate::shared::shared::confirm;
use crate::shared::table::Table;
use crate::task::Task;
use crate::util::{optional_blank_line, set_header_underline};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdContext` (`context`).
pub struct CmdContext {
    meta: CommandMeta,
}

impl CmdContext {
    /// C++ `CmdContext::CmdContext`.
    pub fn new() -> Self {
        CmdContext {
            meta: CommandMeta {
                keyword: "context".into(),
                usage: "task          context [<name> | <subcommand>]".into(),
                description: "Set and define contexts (default filters / modifications)".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Context,
                ..CommandMeta::default()
            },
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CmdContext::joinWords (words, from, to = 0)`: joins all the words
    /// in the interval `[from, to)` with single spaces. If `to` is 0, all the
    /// remaining words are joined.
    pub fn join_words(words: &[String], from: usize, to: usize) -> String {
        let mut value = String::new();

        let to = if to == 0 { words.len() } else { to };

        for (i, word) in words.iter().enumerate().take(to).skip(from) {
            if i > from {
                value.push(' ');
            }

            value += word;
        }

        value
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CmdContext::validateWriteContext`: validates the context as valid
    /// for writing. A valid write context:
    ///   - does not contain any operators except AND
    ///   - does not contain tag exclusion
    ///   - does not use modifiers, except for 'equals' and 'is'
    ///
    /// Returns true if the context is a valid write context; otherwise
    /// `reason` describes the first offending argument.
    pub fn validate_write_context(lexed_args: &[A2], reason: &mut String) -> bool {
        for arg in lexed_args {
            if arg.lextype == LexerType::op && arg.attribute("raw") == "or" {
                *reason = "contains the 'OR' operator".into();
                return false;
            }

            if arg.lextype == LexerType::pair {
                let modifier = arg.attribute("modifier");
                if !modifier.is_empty() && modifier != "is" && modifier != "equals" {
                    *reason = format(
                        "contains an attribute modifier '{1}'",
                        &[&arg.attribute("raw")],
                    );
                    return false;
                }
            }

            if arg.lextype == LexerType::tag && arg.attribute("sign") == "-" {
                *reason = format("contains tag exclusion '{1}'", &[&arg.attribute("raw")]);
                return false;
            }
        }

        true
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ static `CmdContext::getContexts`: all user defined contexts, i.e.
    /// the sorted, distinct `<name>` of every `context.<name>[.<kind>]`
    /// setting. Takes the configuration instead of reading
    /// `Context::getContext ().config`.
    pub fn get_contexts(config: &Configuration) -> Vec<String> {
        let mut contexts = BTreeSet::new();

        for (name, _) in config {
            if let Some(suffix) = name.strip_prefix("context.") {
                match suffix.find('.') {
                    Some(dot) => contexts.insert(suffix[..dot].to_string()),
                    None => contexts.insert(suffix.to_string()),
                };
            }
        }

        contexts.into_iter().collect()
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CmdContext::defineContext`: defines a new user-provided context.
    ///  - The context definition is written into .taskrc as
    ///    `context.<name>.read` (and `.write`, if the filter is a valid
    ///    modification string) variables.
    ///  - Each write requires confirmation if `rc.confirmation=yes`.
    ///
    /// Invoked with: `task context define <name> <filter>`
    /// Example:      `task context define home project:Home`
    ///
    /// As in C++, the filter is "validated" by adding it to the live CLI2
    /// parser and filtering the pending tasks; since `context` accepts no
    /// filter, the re-analysis files the added terms as MISCELLANEOUS, so
    /// only lexing/parsing errors fail the validation.
    fn define_context(
        &self,
        context: &mut Context,
        words: &[String],
        out: &mut String,
    ) -> Result<()> {
        // C++ `auto config = Context::getContext ().config;` (a copy).
        let config = context.config.clone();
        let confirmation = config.get_boolean("confirmation");

        if words.len() > 2 {
            let name = format!("context.{}", words[1]);
            let value = Self::join_words(words, 2, 0);

            // Make sure nobody creates a context with name 'list', 'none' or 'show'
            if words[1] == "none" || words[1] == "list" || words[1] == "show" {
                return Err(TwError::Message(format(
                    "The name '{1}' is reserved and not allowed to use as a context name.",
                    &[&words[1]],
                )));
            }

            // Extract MISCELLANEOUS arguments (containing the filter definition) for later analysis
            let lexed_args = context.cli2.get_miscellaneous();

            // Check if the value is a proper filter by filtering current pending.data
            let mut filter = Filter::new();
            let mut filtered: Vec<Task> = Vec::new();
            let pending: Vec<Task> = context.tdb2.pending_tasks()?.to_vec();

            // This result is not used, and is just to check validity.
            let validation = with_cli2(context, |cli2, ctx| cli2.add_filter(ctx, &value))
                .and_then(|()| filter.subset_of(context, &pending, &mut filtered));
            match validation {
                Ok(()) => {}
                Err(TwError::Message(exception)) => {
                    return Err(TwError::Message(format(
                        "Filter validation failed: {1}",
                        &[&exception],
                    )));
                }
                Err(e) => return Err(e),
            }

            // Make user explicitly confirm filters that are matching no pending tasks
            if filtered.is_empty()
                && confirmation
                && !confirm(&format(
                    "The filter '{1}' matches 0 pending tasks. Do you wish to continue?",
                    &[&value],
                ))
            {
                return Err(TwError::Message("Context definition aborted.".into()));
            }

            let mut reason = String::new();
            let valid_write_context = Self::validate_write_context(&lexed_args, &mut reason);

            if !valid_write_context {
                let mut warning = String::new();
                warning += &format(
                    "The filter '{1}' is not a valid modification string, because it {2}.",
                    &[&value, &reason],
                );
                warning += "\nAs such, value for the write context cannot be set (context will not apply on task add / task log).\n\n";
                warning += &format(
                    "Please use 'task config context.{1}.write <default mods>' to set default attribute values for new tasks in this context manually.\n\n",
                    &[&words[1]],
                );
                *out += &colorize_footnote(&warning);
            }

            // Set context definition config variable
            let read_success = CmdConfig::set_config_variable(
                &context.config,
                &format!("{name}.read"),
                &value,
                confirmation,
            )?;
            let mut write_success = false;

            if valid_write_context {
                write_success = CmdConfig::set_config_variable(
                    &context.config,
                    &format!("{name}.write"),
                    &value,
                    confirmation,
                )?;
            }

            // Remove old-school context name, if it exists, assuming the read context was defined
            if read_success && config.has(&name) {
                CmdConfig::unset_config_variable(&context.config, &name, false)?;
            }

            if !read_success && !write_success {
                return Err(TwError::Message(format(
                    "Context '{1}' not defined.",
                    &[&words[1]],
                )));
            } else if !read_success {
                *out += &format("Context '{1}' defined (write only).", &[&words[1]]);
            } else if !write_success {
                *out += &format("Context '{1}' defined (read only).", &[&words[1]]);
            } else {
                *out += &format("Context '{1}' defined (read, write).", &[&words[1]]);
            }

            *out += &format(" Use 'task context {1}' to activate.\n", &[&words[1]]);
            Ok(())
        } else {
            Err(TwError::Message(
                "Both context name and its definition must be provided.".into(),
            ))
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CmdContext::deleteContext`: deletes the specified context (the
    /// legacy `context.<name>` and its `.read`/`.write` flavours).
    ///  - If the deleted context is currently active, unset it.
    ///  - Deletion of the context requires confirmation if `rc.confirmation=yes`.
    ///
    /// Invoked with: `task context delete <name>`
    /// Example:      `task context delete home`
    fn delete_context(
        &self,
        context: &mut Context,
        words: &[String],
        out: &mut String,
    ) -> Result<()> {
        if words.len() > 1 {
            // Delete the specified context
            let name = format!("context.{}", words[1]);

            let confirmation = context.config.get_boolean("confirmation");
            if confirmation
                && !confirm(&format(
                    "Do you want to delete context '{1}'?",
                    &[&words[1]],
                ))
            {
                return Err(TwError::Message(format(
                    "Context '{1}' not deleted.",
                    &[&words[1]],
                )));
            }

            // Delete legacy format and .read / .write flavours
            let mut rc = CmdConfig::unset_config_variable(&context.config, &name, false)?;
            rc +=
                CmdConfig::unset_config_variable(&context.config, &format!("{name}.read"), false)?;
            rc +=
                CmdConfig::unset_config_variable(&context.config, &format!("{name}.write"), false)?;

            // If the currently set context was deleted, unset it
            if context.config.get("context") == words[1] {
                CmdConfig::unset_config_variable(&context.config, "context", false)?;
            }

            // Output feedback, rc should be even because only 0 (found and removed)
            // and 2 (not found) are aceptable return values from unsetConfigVariable
            if rc % 2 != 0 {
                return Err(TwError::Message(format(
                    "Context '{1}' not deleted.",
                    &[&words[1]],
                )));
            } else if rc == 6 {
                return Err(TwError::Message(format(
                    "Context '{1}' not found.",
                    &[&words[1]],
                )));
            }

            *out += &format("Context '{1}' deleted.\n", &[&words[1]]);
            Ok(())
        } else {
            Err(TwError::Message(
                "Context name needs to be specified.".into(),
            ))
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CmdContext::listContexts`: renders a table of context names, with
    /// a `read` and a `write` row each (Name/Type/Definition/Active).
    ///
    /// Fails with `No contexts defined.` when there are none.
    ///
    /// Invoked with: `task context list`
    fn list_contexts(&self, context: &mut Context, out: &mut String) -> Result<()> {
        let mut contexts = Self::get_contexts(&context.config);
        if !contexts.is_empty() {
            contexts.sort();

            let mut table = Table::new();
            let width = context.get_width();
            table.width(width);
            table.add("Name");
            table.add("Type");
            table.add("Definition");
            table.add("Active");
            set_header_underline(context, &mut table)?;

            let active_context = context.config.get("context");

            for user_context in &contexts {
                let mut active = "no";
                if *user_context == active_context {
                    active = "yes";
                }

                let row = table.add_row();
                table.set(row, 0, user_context);
                table.set(row, 1, "read");
                let read = context.get_task_context("read", user_context, true);
                table.set(row, 2, &read);
                table.set(row, 3, active);

                let row = table.add_row();
                table.set(row, 0, "");
                table.set(row, 1, "write");
                let write = context.get_task_context("write", user_context, true);
                table.set(row, 2, &write);
                table.set(row, 3, active);
            }

            *out += optional_blank_line(context);
            *out += &table.render();
            *out += optional_blank_line(context);
            Ok(())
        } else {
            Err(TwError::Message("No contexts defined.".into()))
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CmdContext::setContext`: sets the specified context as currently
    /// active (replacing any other). Setting does not require confirmation.
    ///
    /// Invoked with: `task context <name>`
    /// Example:      `task context home`
    fn set_context(&self, context: &mut Context, words: &[String], out: &mut String) -> Result<()> {
        let value = &words[0];
        let contexts = Self::get_contexts(&context.config);

        // Check that the specified context is defined
        if !contexts.contains(value) {
            return Err(TwError::Message(format(
                "Context '{1}' not found.",
                &[value],
            )));
        }

        // Set the active context.
        // Should always succeed, as we do not require confirmation.
        let success = CmdConfig::set_config_variable(&context.config, "context", value, false)?;

        if !success {
            return Err(TwError::Message(format(
                "Context '{1}' not applied.",
                &[value],
            )));
        }

        *out += &format(
            "Context '{1}' set. Use 'task context none' to remove.\n",
            &[value],
        );
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CmdContext::showContext`: shows the currently active context.
    ///
    /// Invoked with: `task context show`
    fn show_context(&self, context: &mut Context, out: &mut String) {
        let current_context = context.config.get("context");

        if current_context.is_empty() {
            *out += "No context is currently applied.\n";
        } else {
            let read = context.get_task_context("read", "", true);
            let write = context.get_task_context("write", "", true);
            *out += &format(
                "Context '{1}' with \n\n* read filter: '{2}'\n* write filter: '{3}'\n\nis currently applied.\n",
                &[&current_context, &read, &write],
            );
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CmdContext::unsetContext`: unsets the currently active context
    /// (no confirmation). Fails with `Context not unset.` if no `context`
    /// line was removed (also when no context is active).
    ///
    /// Invoked with: `task context none`
    fn unset_context(&self, context: &mut Context, out: &mut String) -> Result<()> {
        if CmdConfig::unset_config_variable(&context.config, "context", false)? != 0 {
            return Err(TwError::Message("Context not unset.".into()));
        }

        *out += "Context unset.\n";
        Ok(())
    }
}

impl Default for CmdContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdContext {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdContext::execute`: dispatches on the first word (`define`,
    /// `delete`, `list`, `none`, `show`, else a context name to activate);
    /// without words, lists the contexts followed by the "Use 'task context
    /// none'" hint. Always returns 0; failures are message errors.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let mut out = String::new();

        // Get the non-attribute, non-fancy command line arguments.
        let words = context
            .cli2
            .get_words(&context.config, &mut context.debug_messages);
        if !words.is_empty() {
            let subcommand = words[0].as_str();

            if subcommand == "define" {
                self.define_context(context, &words, &mut out)?;
            } else if subcommand == "delete" {
                self.delete_context(context, &words, &mut out)?;
            } else if subcommand == "list" {
                self.list_contexts(context, &mut out)?;
            } else if subcommand == "none" {
                self.unset_context(context, &mut out)?;
            } else if subcommand == "show" {
                self.show_context(context, &mut out);
            } else {
                self.set_context(context, &words, &mut out)?;
            }
        } else {
            self.list_contexts(context, &mut out)?;
            out += "Use 'task context none' to unset the current context.\n";
        }

        *output = out;
        Ok(0)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionContext` (`_context`).
pub struct CmdCompletionContext {
    meta: CommandMeta,
}

impl CmdCompletionContext {
    /// C++ `CmdCompletionContext::CmdCompletionContext`.
    pub fn new() -> Self {
        CmdCompletionContext {
            meta: CommandMeta {
                keyword: "_context".into(),
                usage: "task          _context".into(),
                description: "Lists all supported contexts, for completion purposes".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCompletionContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionContext {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionContext::execute`: one defined context name per line.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        for name in CmdContext::get_contexts(&context.config) {
            *output += &name;
            output.push('\n');
        }

        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::shared::{begin_console_override, end_console_override};
    use crate::test_util::CommandFixture;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn fixture(rc: &str, args: &[&str]) -> CommandFixture {
        CommandFixture::new(&[r#"{"description":"one","project":"Home"}"#], rc, args)
    }

    fn rc_text(f: &CommandFixture) -> String {
        std::fs::read_to_string(f.ctx.config.file()).unwrap()
    }

    fn run(f: &mut CommandFixture, command: &dyn Command) -> (Result<i32>, String) {
        let mut output = String::new();
        let rc = command.execute(&mut f.ctx, &mut output);
        (rc, output)
    }

    #[test]
    fn join_words_intervals() {
        let w = words(&["define", "home", "project:Home", "+x"]);
        assert_eq!(CmdContext::join_words(&w, 2, 0), "project:Home +x");
        assert_eq!(CmdContext::join_words(&w, 0, 2), "define home");
        assert_eq!(CmdContext::join_words(&w, 1, 2), "home");
        assert_eq!(CmdContext::join_words(&w, 4, 0), "");
    }

    fn arg(raw: &str, lextype: LexerType, attrs: &[(&str, &str)]) -> A2 {
        let mut a = A2::new(raw, lextype);
        for (k, v) in attrs {
            a.set_attribute(k, v);
        }
        a
    }

    #[test]
    fn validate_write_context_reasons() {
        let mut reason = String::new();
        let ok = [
            arg("project:Home", LexerType::pair, &[("modifier", "")]),
            arg("and", LexerType::op, &[]),
            arg("project.is:x", LexerType::pair, &[("modifier", "is")]),
            arg("+work", LexerType::tag, &[("sign", "+")]),
        ];
        assert!(CmdContext::validate_write_context(&ok, &mut reason));
        assert_eq!(reason, "");

        let or = [arg("or", LexerType::op, &[])];
        assert!(!CmdContext::validate_write_context(&or, &mut reason));
        assert_eq!(reason, "contains the 'OR' operator");

        let modifier = [arg(
            "due.before:today",
            LexerType::pair,
            &[("modifier", "before")],
        )];
        assert!(!CmdContext::validate_write_context(&modifier, &mut reason));
        assert_eq!(reason, "contains an attribute modifier 'due.before:today'");

        let exclusion = [arg("-work", LexerType::tag, &[("sign", "-")])];
        assert!(!CmdContext::validate_write_context(&exclusion, &mut reason));
        assert_eq!(reason, "contains tag exclusion '-work'");
    }

    #[test]
    fn get_contexts_sorted_distinct_names() {
        let mut config = Configuration::new();
        config.set("context.work.read", "+work");
        config.set("context.work.write", "+work");
        config.set("context.home", "project:Home");
        config.set("context.a.b.c", "x");
        config.set("context", "work");
        config.set("contexts.x", "y");
        assert_eq!(CmdContext::get_contexts(&config), ["a", "home", "work"]);
        assert!(CmdContext::get_contexts(&Configuration::new()).is_empty());
    }

    #[test]
    fn define_writes_read_and_write() {
        let mut f = fixture(
            "confirmation=off\ncontext.home=old\n",
            &["context", "define", "home", "project:Home"],
        );
        let (rc, out) = run(&mut f, &CmdContext::new());
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(
            out,
            "Context 'home' defined (read, write). Use 'task context home' to activate.\n"
        );
        let text = rc_text(&f);
        assert!(text.contains("context.home.read=project:Home\n"), "{text}");
        assert!(text.contains("context.home.write=project:Home\n"), "{text}");
        assert!(!text.contains("context.home=old"), "{text}");
    }

    #[test]
    fn define_invalid_write_context_is_read_only() {
        let mut f = fixture(
            "confirmation=off\n",
            &["context", "define", "w", "+a", "or", "+b"],
        );
        let (rc, out) = run(&mut f, &CmdContext::new());
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(
            out,
            "The filter '+a or +b' is not a valid modification string, because it contains the 'OR' operator.\n\
             As such, value for the write context cannot be set (context will not apply on task add / task log).\n\n\
             Please use 'task config context.w.write <default mods>' to set default attribute values for new tasks in this context manually.\n\n\
             Context 'w' defined (read only). Use 'task context w' to activate.\n"
        );
        let text = rc_text(&f);
        assert!(text.contains("context.w.read=+a or +b\n"), "{text}");
        assert!(!text.contains("context.w.write"), "{text}");
    }

    #[test]
    fn define_errors() {
        let mut f = fixture("confirmation=off\n", &["context", "define", "list", "x"]);
        let (rc, out) = run(&mut f, &CmdContext::new());
        assert_eq!(
            rc.unwrap_err().to_string(),
            "The name 'list' is reserved and not allowed to use as a context name."
        );
        assert_eq!(out, "");
        drop(f);

        let mut f = fixture("confirmation=off\n", &["context", "define", "home"]);
        let (rc, _) = run(&mut f, &CmdContext::new());
        assert_eq!(
            rc.unwrap_err().to_string(),
            "Both context name and its definition must be provided."
        );
    }

    #[test]
    fn define_with_confirmation_prompts_for_each_variable() {
        let mut f = fixture("confirmation=on\n", &["context", "define", "h", "+x"]);
        begin_console_override("yes\nno\n");
        let (rc, out) = run(&mut f, &CmdContext::new());
        let shown = end_console_override();
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(
            shown,
            "Are you sure you want to add 'context.h.read' with a value of '+x'? (yes/no) \
             Are you sure you want to add 'context.h.write' with a value of '+x'? (yes/no) "
        );
        assert_eq!(
            out,
            "Context 'h' defined (read only). Use 'task context h' to activate.\n"
        );
    }

    #[test]
    fn define_declined_entirely_is_not_defined() {
        let mut f = fixture("confirmation=on\n", &["context", "define", "h", "+x"]);
        begin_console_override("no\nno\n");
        let (rc, _) = run(&mut f, &CmdContext::new());
        end_console_override();
        assert_eq!(rc.unwrap_err().to_string(), "Context 'h' not defined.");
    }

    #[test]
    fn delete_found_missing_and_active() {
        let mut f = fixture(
            "confirmation=off\ncontext.h.read=+x\ncontext.h.write=+x\ncontext=h\n",
            &["context", "delete", "h"],
        );
        let (rc, out) = run(&mut f, &CmdContext::new());
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(out, "Context 'h' deleted.\n");
        let text = rc_text(&f);
        assert!(!text.contains("context"), "{text}");
        drop(f);

        let mut f = fixture("confirmation=off\n", &["context", "delete", "nope"]);
        let (rc, _) = run(&mut f, &CmdContext::new());
        assert_eq!(rc.unwrap_err().to_string(), "Context 'nope' not found.");
        drop(f);

        let mut f = fixture("confirmation=off\n", &["context", "delete"]);
        let (rc, _) = run(&mut f, &CmdContext::new());
        assert_eq!(
            rc.unwrap_err().to_string(),
            "Context name needs to be specified."
        );
    }

    #[test]
    fn delete_declined() {
        let mut f = fixture(
            "confirmation=on\ncontext.h=+x\n",
            &["context", "delete", "h"],
        );
        begin_console_override("no\n");
        let (rc, _) = run(&mut f, &CmdContext::new());
        let shown = end_console_override();
        assert_eq!(shown, "Do you want to delete context 'h'? (yes/no) ");
        assert_eq!(rc.unwrap_err().to_string(), "Context 'h' not deleted.");
        assert!(rc_text(&f).contains("context.h=+x"));
    }

    #[test]
    fn list_and_default_listing() {
        let rc = "context.home.read=project:Home\ncontext.home.write=project:Home\ncontext.old=+x\ncontext=old\n";
        // Not a TTY: fontunderline (default on) underlines the headers.
        let expected = "\n\x1b[4mName\x1b[0m \x1b[4mType \x1b[0m \x1b[4mDefinition  \x1b[0m \x1b[4mActive\x1b[0m\n\
                        home read  project:Home no\n     write project:Home no\n\
                        old  read  +x           yes\n     write              yes\n\n";
        let mut f = fixture(rc, &["rc.verbose=blank", "context", "list"]);
        let (code, out) = run(&mut f, &CmdContext::new());
        assert_eq!(code.unwrap(), 0);
        assert_eq!(out, expected);
        drop(f);

        let mut f = fixture(rc, &["rc.verbose=blank", "context"]);
        let (code, out) = run(&mut f, &CmdContext::new());
        assert_eq!(code.unwrap(), 0);
        assert_eq!(
            out,
            format!("{expected}Use 'task context none' to unset the current context.\n")
        );
        drop(f);

        let mut f = fixture("", &["context"]);
        let (code, out) = run(&mut f, &CmdContext::new());
        assert_eq!(code.unwrap_err().to_string(), "No contexts defined.");
        assert_eq!(out, "");
    }

    #[test]
    fn set_show_and_none() {
        let rc = "context.home.read=project:Home\ncontext.home.write=project:Home\n";
        let mut f = fixture(rc, &["context", "home"]);
        let (code, out) = run(&mut f, &CmdContext::new());
        assert_eq!(code.unwrap(), 0);
        assert_eq!(
            out,
            "Context 'home' set. Use 'task context none' to remove.\n"
        );
        assert!(rc_text(&f).ends_with("context=home\n"));
        drop(f);

        let mut f = fixture(rc, &["context", "nope"]);
        let (code, _) = run(&mut f, &CmdContext::new());
        assert_eq!(code.unwrap_err().to_string(), "Context 'nope' not found.");
        drop(f);

        let mut f = fixture(&format!("{rc}context=home\n"), &["context", "show"]);
        let (code, out) = run(&mut f, &CmdContext::new());
        assert_eq!(code.unwrap(), 0);
        assert_eq!(
            out,
            "Context 'home' with \n\n* read filter: 'project:Home'\n* write filter: 'project:Home'\n\nis currently applied.\n"
        );
        drop(f);

        let mut f = fixture(rc, &["context", "show"]);
        let (_, out) = run(&mut f, &CmdContext::new());
        assert_eq!(out, "No context is currently applied.\n");
        drop(f);

        let mut f = fixture(&format!("{rc}context=home\n"), &["context", "none"]);
        let (code, out) = run(&mut f, &CmdContext::new());
        assert_eq!(code.unwrap(), 0);
        assert_eq!(out, "Context unset.\n");
        assert!(!rc_text(&f).contains("context=home"));
        drop(f);

        let mut f = fixture(rc, &["context", "none"]);
        let (code, _) = run(&mut f, &CmdContext::new());
        assert_eq!(code.unwrap_err().to_string(), "Context not unset.");
    }

    #[test]
    fn completion_lists_names() {
        let mut f = fixture(
            "context.b.read=+b\ncontext.a=+a\ncontext=a\n",
            &["_context"],
        );
        let (code, out) = run(&mut f, &CmdCompletionContext::new());
        assert_eq!(code.unwrap(), 0);
        assert_eq!(out, "a\nb\n");
    }
}
