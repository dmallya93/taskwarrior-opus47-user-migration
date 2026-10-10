//! Rust translation of `src/commands/CmdCalc.cpp`.
//!
//! The struct is the C++ class from `CmdCalc.cpp`, carrying the exact
//! metadata of its C++ constructor.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::eval::{DomResolver, Eval};
use crate::variant::{Variant, VariantTask};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCalc` (`calc`).
pub struct CmdCalc {
    meta: CommandMeta,
}

impl CmdCalc {
    /// C++ `CmdCalc::CmdCalc`.
    pub fn new() -> Self {
        CmdCalc {
            meta: CommandMeta {
                keyword: "calc".into(),
                usage: "task          calc <expression>".into(),
                description: "Calculator".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Misc,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCalc {
    fn default() -> Self {
        Self::new()
    }
}

/// The resolver for `e.addSource (domSource)`: `Context::getContext
/// ().currentTask` is null for `calc` (nothing sets it), and Eval's debug
/// output goes to the Context debug sink (C++ `Context::getContext ().debug`).
struct CalcResolver<'c> {
    context: &'c mut Context,
}

impl DomResolver for CalcResolver<'_> {
    /// C++ `domSource`, i.e. `getDOM (identifier, nullptr, value)`.
    ///
    /// Deviation: `getDOM` (`src/DOM.cpp`, Rust `dom.rs`) is ported by
    /// milestone 5; until it lands no DOM reference (`rc.*`, `tw.*`,
    /// `system.*`, `context.*`, `<id>.<attribute>`) resolves, so such an
    /// identifier is left to Eval exactly as C++ treats an unknown name.
    /// Milestone 5 replaces this body with `dom_source (name, |n| get_dom
    /// (n, ...))`.
    fn resolve(&mut self, _name: &str) -> Option<Variant> {
        None
    }

    fn current_task(&self) -> Option<&dyn VariantTask> {
        None
    }

    fn debug(&mut self, message: &str) {
        self.context.debug(message);
    }
}

impl Command for CmdCalc {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCalc::execute`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Configurable infix/postfix
        let mut infix = true;
        if context.config.get("expressions") == "postfix" {
            infix = false;
        }

        // Create an evaluator with DOM access.
        let mut e = Eval::new();
        e.debug(context.config.get_boolean("debug"));

        // Compile all the args into one expression.
        let mut expression = String::new();
        for word in context.cli2.get_words(&context.config, &mut context.debug_messages) {
            expression += &word;
            expression.push(' ');
        }

        // Evaluate according to preference.
        let mut resolver = CalcResolver { context };
        let result = if infix {
            e.evaluate_infix_expression(&expression, &mut resolver)?
        } else {
            e.evaluate_postfix_expression(&expression, &mut resolver)?
        };

        *output = format!("{result}\n");
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::datetime::Datetime;
    use crate::shared::duration::Duration;

    /// A Context whose CLI2 holds `args` (as `Context::initialize` would
    /// add them) and whose configuration holds the `config` settings.
    /// Also applies the `Context::staticInitialization` lexer settings
    /// (thread-local, so they only affect this test's thread).
    fn context_with(args: &[&str], config: &[(&str, &str)]) -> Context {
        Datetime::set_standalone_date_enabled(false);
        Datetime::set_standalone_time_enabled(false);
        Duration::set_standalone_seconds_enabled(false);
        let mut context = Context::new();
        for (name, value) in config {
            context.config.set(name, *value);
        }
        crate::commands::command::register(&mut context.commands, std::rc::Rc::new(CmdCalc::new()));
        context.cli2.entity("cmd", "calc");
        for a in args {
            context.cli2.add(a);
        }
        crate::cli2::with_cli2(&mut context, |cli2, ctx| cli2.analyze(ctx)).unwrap();
        context
    }

    fn run(args: &[&str], config: &[(&str, &str)]) -> (Result<i32>, String, Vec<String>) {
        let mut context = context_with(args, config);
        let mut output = String::new();
        let rc = CmdCalc::new().execute(&mut context, &mut output);
        (rc, output, context.debug_messages)
    }

    #[test]
    fn infix_by_default() {
        let (rc, out, debug) = run(&["task", "calc", "12", "*", "3600", "+", "34*60+56"], &[]);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(out, "45296\n");
        assert!(debug.is_empty());
    }

    #[test]
    fn postfix_by_configuration() {
        let (rc, out, _) = run(&["task", "calc", "12 3600 * 34 60 * 56 + +"], &[("expressions", "postfix")]);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(out, "45296\n");
    }

    #[test]
    fn debug_goes_to_the_context_sink() {
        let (rc, out, debug) = run(&["task", "calc", "2- -3"], &[("debug", "1")]);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(out, "5\n");
        assert!(debug.iter().any(|m| m == "Eval literal number ↑'2'"), "{debug:?}");
        assert!(debug.iter().any(|m| m == "Eval _neg_ ↓'3' → ↑'-3'"), "{debug:?}");
        assert!(debug.iter().any(|m| m.starts_with("\x1b[1;37;42mFILTER\x1b[0m Infix ")), "{debug:?}");
    }

    #[test]
    fn errors_propagate() {
        let (rc, out, _) = run(&["task", "calc", "1 +"], &[]);
        assert!(rc.is_err());
        assert_eq!(out, "");
    }
}
