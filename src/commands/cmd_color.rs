//! Rust translation of `src/commands/CmdColor.cpp`.
//!
//! `task colors`: the full colour chart, a sample of a colour spec, or a
//! legend of the `color.*` settings in use.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::shared::color::Color;
use crate::shared::shared::close_enough;
use crate::shared::table::Table;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdColor` (`colors`).
pub struct CmdColor {
    meta: CommandMeta,
}

impl CmdColor {
    /// C++ `CmdColor::CmdColor`.
    pub fn new() -> Self {
        CmdColor {
            meta: CommandMeta {
                keyword: "colors".into(),
                usage: "task          colors [sample | legend]".into(),
                description: "All colors, a sample, or a legend".into(),
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

impl Default for CmdColor {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdColor {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdColor::execute`.
    ///
    /// With colour enabled: `legend` (any prefix of it among the words) shows
    /// a table of every `color*` setting rendered in its own colour; other
    /// words are joined into a colour spec and shown as a sample; no words
    /// shows every supported colour. With colour off, prints the C++ hint and
    /// returns 1. An unrecognized colour spec fails with the `Color` error,
    /// as the C++ constructor throws.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let mut rc = 0;

        // Get the non-attribute, non-fancy command line arguments.
        let words = context.cli2.get_words(&context.config, &mut context.debug_messages);
        let legend = words.iter().any(|word| close_enough("legend", word, 0));

        let mut out = String::new();
        if context.color() {
            // If the description contains 'legend', show all the colors
            // currently in use.
            if legend {
                out.push_str("\nHere are the colors currently in use:\n");

                let mut view = Table::new();
                view.width(context.get_width());
                view.with_color(context.config.get_boolean("color"));
                view.add("Color");
                view.add("Definition");

                let items: Vec<(String, String)> =
                    context.config.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                for (name, value) in items {
                    // Skip items with 'color' in their name, that are not
                    // referring to actual colors.
                    if name != "_forcecolor" && name != "color" && name.starts_with("color") {
                        let color = Color::parse(&context.config.get(&name))?;
                        let row = view.add_row();
                        view.set_colored(row, 0, &name, color);
                        view.set_colored(row, 1, &value, color);
                    }
                }

                out.push_str(&view.render());
                out.push('\n');
            }
            // If there is something in the description, then assume that is a
            // color, and display it as a sample.
            else if !words.is_empty() {
                let one = Color::parse("black on bright yellow")?;
                let two = Color::parse("underline cyan on bright blue")?;
                let three = Color::parse("color214 on color202")?;
                let four = Color::parse("rgb150 on rgb020")?;
                let five = Color::parse("underline grey10 on grey3")?;
                let six = Color::parse("red on color173")?;

                let swatch = words.join(" ");
                let sample = Color::parse(&swatch)?;

                out.push('\n');
                out.push_str("Use this command to see how colors are displayed by your terminal.\n");
                out.push_str("\n\n");
                out.push_str("16-color usage (supports underline, bold text, bright background):\n");
                for (color, text) in [
                    (one, "task color black on bright yellow"),
                    (two, "task color underline cyan on bright blue"),
                ] {
                    out.push_str("  ");
                    out.push_str(&color.colorize(text));
                    out.push('\n');
                }
                out.push('\n');
                out.push_str("256-color usage (supports underline):\n");
                for (color, text) in [
                    (three, "task color color214 on color202"),
                    (four, "task color rgb150 on rgb020"),
                    (five, "task color underline grey10 on grey3"),
                    (six, "task color red on color173"),
                ] {
                    out.push_str("  ");
                    out.push_str(&color.colorize(text));
                    out.push('\n');
                }
                out.push('\n');
                out.push_str("Your sample:\n\n");
                out.push_str("  ");
                out.push_str(&sample.colorize(&format!("task color {swatch}")));
                out.push_str("\n\n");
            }
            // Show all supported colors.  Possibly show some unsupported ones
            // too.
            else {
                show_all_colors(&mut out)?;
            }
        } else {
            out.push_str(
                "Color is currently turned off in your .taskrc file.  To enable color, remove the line \
                 'color=off', or change the 'off' to 'on'.\n",
            );
            rc = 1;
        }

        *output = out;
        Ok(rc)
    }
}

/// The no-argument branch of C++ `CmdColor::execute`: basic colours,
/// effects, the 16 system colours, the 6x6x6 colour cube and the gray ramp.
fn show_all_colors(out: &mut String) -> Result<()> {
    let c = Color::colorize_with;

    out.push('\n');
    out.push_str("Basic colors\n");
    for (text, spec) in [
        (" black ", "black"),
        (" red ", "red"),
        (" blue ", "blue"),
        (" green ", "green"),
        (" magenta ", "magenta"),
        (" cyan ", "cyan"),
        (" yellow ", "yellow"),
        (" white ", "white"),
    ] {
        out.push(' ');
        out.push_str(&c(text, spec)?);
    }
    out.push('\n');
    for (text, spec) in [
        (" black ", "white on black"),
        (" red ", "white on red"),
        (" blue ", "white on blue"),
        (" green ", "black on green"),
        (" magenta ", "black on magenta"),
        (" cyan ", "black on cyan"),
        (" yellow ", "black on yellow"),
        (" white ", "black on white"),
    ] {
        out.push(' ');
        out.push_str(&c(text, spec)?);
    }
    out.push_str("\n\n");

    out.push_str("Effects\n");
    for (text, spec) in [
        (" red ", "red"),
        (" bold red ", "bold red"),
        (" underline on blue ", "underline on blue"),
        (" on green ", "black on green"),
        (" on bright green ", "black on bright green"),
        (" inverse ", "inverse"),
    ] {
        out.push(' ');
        out.push_str(&c(text, spec)?);
    }
    out.push_str("\n\n");

    // 16 system colors.
    out.push_str("color0 - color15\n  0 1 2 . . .\n");
    for r in 0..2 {
        out.push_str("  ");
        for col in 0..8 {
            out.push_str(&c("  ", &format!("on color{}", r * 8 + col))?);
        }
        out.push('\n');
    }
    out.push_str("          . . . 15\n\n");

    // Color cube.
    out.push_str("Color cube rgb");
    out.push_str(&c("0", "bold red")?);
    out.push_str(&c("0", "bold green")?);
    out.push_str(&c("0", "bold blue")?);
    out.push_str(" - rgb");
    out.push_str(&c("5", "bold red")?);
    out.push_str(&c("5", "bold green")?);
    out.push_str(&c("5", "bold blue")?);
    out.push_str(" (also color16 - color231)\n");
    out.push_str("  ");
    out.push_str(&c(
        "0            1            2            3            4            5",
        "bold red",
    )?);
    out.push('\n');
    out.push_str("  ");
    out.push_str(&c(
        "0 1 2 3 4 5  0 1 2 3 4 5  0 1 2 3 4 5  0 1 2 3 4 5  0 1 2 3 4 5  0 1 2 3 4 5",
        "bold blue",
    )?);
    out.push('\n');

    for g in 0..6 {
        out.push_str(&c(&format!(" {g}"), "bold green")?);
        for r in 0..6 {
            for b in 0..6 {
                out.push_str(&c("  ", &format!("on rgb{r}{g}{b}"))?);
            }
            out.push(' ');
        }
        out.push('\n');
    }
    out.push('\n');

    // Grey ramp.
    out.push_str("Gray ramp gray0 - gray23 (also color232 - color255)\n");
    out.push_str("  0 1 2 . . .                             . . . 23\n");
    out.push_str("  ");
    for g in 0..24 {
        out.push_str(&c("  ", &format!("on gray{g}"))?);
    }
    out.push_str("\n\nTry running 'task color white on red'.\n\n");
    Ok(())
}
