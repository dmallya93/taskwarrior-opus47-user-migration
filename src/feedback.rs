//! Rust translation of `src/feedback.cpp`.
//!
//! Partially ported: [`feedback_special_tags`] (needed by
//! `ColumnTags::modify`). The rest of the module (`feedback_affected`,
//! `feedback_reserved_tags`, `feedback_unblocked`, `onProjectChange`, ...)
//! is filled in by the milestone 5 feedback port.

use crate::context::Context;
use crate::shared::format::format;
use crate::task::Task;

////////////////////////////////////////////////////////////////////////////////
/// C++ `void feedback_special_tags (const Task& task, const std::string&
/// tag)`: with `rc.verbose` including `special`, explains the special tags
/// `nocolor`, `nonag`, `nocal` and `next` on stdout.
pub fn feedback_special_tags(ctx: &mut Context, task: &Task, tag: &str) {
    if ctx.verbose("special") {
        let msg = if tag == "nocolor" {
            "The 'nocolor' special tag will disable color rules for this task."
        } else if tag == "nonag" {
            "The 'nonag' special tag will prevent nagging when this task is modified."
        } else if tag == "nocal" {
            "The 'nocal' special tag will keep this task off the 'calendar' report."
        } else if tag == "next" {
            "The 'next' special tag will boost the urgency of this task so it appears on the 'next' report."
        } else {
            ""
        };

        if !msg.is_empty() {
            // C++ `std::cout << format (msg, task.identifier ()) << "\n";`
            println!("{}", format(msg, &[&task.identifier(false)]));
        }
    }
}
