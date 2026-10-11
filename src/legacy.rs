//! Rust translation of `src/legacy.cpp`.
//!
//! C++ reads `Context::getContext ()`; here the column maps take the context
//! (for its footnotes) and the deprecation checks take the configuration.

use crate::context::Context;
use crate::shared::configuration::Configuration;
use crate::shared::format::format;

const STRING_LEGACY_PRIORITY: &str = "Legacy attribute found.  Please change '{1}' to '{2}'.";

/// C++ function-static `legacyMap` shared by both column maps.
fn legacy_map(name: &str) -> Option<&'static str> {
    match name {
        "priority." => Some("priority"),
        _ => None,
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `legacyColumnMap`.
///
/// 2014-01-26: priority_long        --> priority.long        Mapping removed
/// 2014-01-26: entry_time           --> entry                Mapping removed
/// 2014-01-26: start_time           --> start                Mapping removed
/// 2014-01-26: end_time             --> end                  Mapping removed
/// 2014-01-26: countdown            --> due.countdown        Mapping removed
/// 2014-01-26: countdown_compact    --> due.countdown        Mapping removed
/// 2014-01-26: age                  --> entry.age            Mapping removed
/// 2014-01-26: age_compact          --> entry.age            Mapping removed
/// 2014-01-26: active               --> start.active         Mapping removed
/// 2014-01-26: recurrence_indicator --> recur.indicator      Mapping removed
/// 2014-01-26: tag_indicator        --> tags.indicator       Mapping removed
/// 2014-01-26: description_only     --> description.desc     Mapping removed
pub fn legacy_column_map(context: &mut Context, name: &mut String) {
    // If a legacy column was used, complain about it, but modify it anyway.
    if let Some(found) = legacy_map(name) {
        context.footnote(&format(STRING_LEGACY_PRIORITY, &[&name.as_str(), &found]));
        *name = found.to_string();
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `legacySortColumnMap`.
///
/// 2014-01-26: priority_long        --> priority             Mapping removed
/// 2014-01-26: entry_time           --> entry                Mapping removed
/// 2014-01-26: start_time           --> start                Mapping removed
/// 2014-01-26: end_time             --> end                  Mapping removed
/// 2014-01-26: countdown            --> due                  Mapping removed
/// 2014-01-26: countdown_compact    --> due                  Mapping removed
/// 2014-01-26: age                  --> entry                Mapping removed
/// 2014-01-26: age_compact          --> entry                Mapping removed
/// 2014-01-26: active               --> start                Mapping removed
/// 2014-01-26: recurrence_indicator --> recur                Mapping removed
/// 2014-01-26: tag_indicator        --> tags                 Mapping removed
/// 2014-01-26: description_only     --> description          Mapping removed
pub fn legacy_sort_column_map(context: &mut Context, name: &mut String) {
    // If a legacy column was used, complain about it, but modify it anyway.
    if let Some(found) = legacy_map(name) {
        context.footnote(&format(STRING_LEGACY_PRIORITY, &[&name.as_str(), &found]));
        *name = found.to_string();
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `legacyCheckForDeprecatedVariables`: a report of deprecated
/// configuration variables (in `std::map` order), or `""` if none.
pub fn legacy_check_for_deprecated_variables(config: &Configuration) -> String {
    let mut deprecated: Vec<&str> = Vec::new();
    for name in config.keys() {
        // 2014-07-04: report.*.limit removed.
        // 2016-02-24: alias._query removed.

        // Deprecated in 2.5.0.
        // report.*.annotations
        if name.len() > 19 && name.starts_with("report.") && name.ends_with(".annotations") {
            deprecated.push(name);
        }

        // Deprecated in 2.5.0.
        if name == "next" || name == "annotations" || name == "export.ical.class" {
            deprecated.push(name);
        }

        // Deprecated in 2.5.0.
        if name == "urgency.inherit.coefficient" {
            deprecated.push(name);
        }
    }

    let mut out = String::new();
    if !deprecated.is_empty() {
        out.push_str("Your .taskrc file contains variables that are deprecated:\n");

        for dep in &deprecated {
            out.push_str("  ");
            out.push_str(dep);
            out.push('\n');
        }

        out.push('\n');
    }

    out
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `legacyCheckForDeprecatedColumns`: always starts with `"\n"`, then
/// lists `report*` variables whose value mentions `entry_time`, `start_time`
/// or `end_time`.
pub fn legacy_check_for_deprecated_columns(config: &Configuration) -> String {
    let mut deprecated: Vec<&str> = Vec::new();
    for name in config.keys() {
        if name.starts_with("report") {
            // Deprecated in 2.0.0
            let value = config.get(name);
            if value.contains("entry_time")
                || value.contains("start_time")
                || value.contains("end_time")
            {
                deprecated.push(name);
            }
        }
    }

    let mut out = String::from("\n");

    if !deprecated.is_empty() {
        out.push_str(
            "Your .taskrc file contains reports with deprecated columns.  Please check for \
             entry_time, start_time or end_time in:\n",
        );

        for dep in &deprecated {
            out.push_str("  ");
            out.push_str(dep);
            out.push('=');
            out.push_str(&config.get(dep));
            out.push('\n');
        }

        out.push('\n');
    }

    out
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `legacyAttributeMap`.
pub fn legacy_attribute_map(name: &mut String) {
    // TW-1274, 2.4.0
    if name == "modification" {
        *name = "modified".to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::env_lock;

    fn config_with(settings: &[(&str, &str)]) -> Configuration {
        let mut config = Configuration::new();
        for (name, value) in settings {
            config.set(name, *value);
        }
        config
    }

    #[test]
    fn column_maps() {
        let _lock = env_lock(); // Context::new reads $HOME.
        let mut context = Context::new();

        let mut name = String::from("priority.");
        legacy_column_map(&mut context, &mut name);
        assert_eq!(name, "priority");
        assert_eq!(
            context.footnotes,
            vec!["Legacy attribute found.  Please change 'priority.' to 'priority'."]
        );

        let mut name = String::from("priority.");
        legacy_sort_column_map(&mut context, &mut name);
        assert_eq!(name, "priority");
        // Footnotes are de-duplicated.
        assert_eq!(context.footnotes.len(), 1);

        for unchanged in [
            "priority",
            "priority_long",
            "entry_time",
            "description_only",
            "",
        ] {
            let mut name = unchanged.to_string();
            legacy_column_map(&mut context, &mut name);
            assert_eq!(name, unchanged);
            legacy_sort_column_map(&mut context, &mut name);
            assert_eq!(name, unchanged);
        }
        assert_eq!(context.footnotes.len(), 1);
    }

    #[test]
    fn attribute_map() {
        let mut name = String::from("modification");
        legacy_attribute_map(&mut name);
        assert_eq!(name, "modified");
        let mut name = String::from("modified");
        legacy_attribute_map(&mut name);
        assert_eq!(name, "modified");
        let mut name = String::from("Modification");
        legacy_attribute_map(&mut name);
        assert_eq!(name, "Modification");
    }

    #[test]
    fn deprecated_variables() {
        assert_eq!(
            legacy_check_for_deprecated_variables(&config_with(&[("report.x.columns", "id")])),
            ""
        );

        let config = config_with(&[
            ("urgency.inherit.coefficient", "1"),
            ("report.next.annotations", "full"),
            ("report.a.annotations", "x"),
            // Length 19 exactly: not > 19, so not reported.
            ("report..annotations", "x"),
            ("next", "2"),
            ("annotations", "full"),
            ("export.ical.class", "x"),
            ("nextx", "x"),
        ]);
        assert_eq!(
            legacy_check_for_deprecated_variables(&config),
            "Your .taskrc file contains variables that are deprecated:\n\
             \x20 annotations\n\
             \x20 export.ical.class\n\
             \x20 next\n\
             \x20 report.a.annotations\n\
             \x20 report.next.annotations\n\
             \x20 urgency.inherit.coefficient\n\
             \n"
        );
    }

    #[test]
    fn deprecated_variables_length_boundary() {
        // "report.a.annotations" is 20 bytes (> 19); "report.annotations" is 18.
        let config = config_with(&[("report.annotations", "x"), ("reportx.annotations", "x")]);
        assert_eq!(legacy_check_for_deprecated_variables(&config), "");
    }

    #[test]
    fn deprecated_columns() {
        assert_eq!(
            legacy_check_for_deprecated_columns(&config_with(&[("report.x.columns", "id")])),
            "\n"
        );

        let config = config_with(&[
            ("report.a.columns", "id,entry_time"),
            ("report.b.columns", "id,start_time,description"),
            ("report.c.columns", "end_time"),
            ("report.d.columns", "id,entry"),
            ("reports", "end_time"),
            ("xreport.e.columns", "entry_time"),
        ]);
        assert_eq!(
            legacy_check_for_deprecated_columns(&config),
            "\nYour .taskrc file contains reports with deprecated columns.  Please check for entry_time, start_time \
             or end_time in:\n\
             \x20 report.a.columns=id,entry_time\n\
             \x20 report.b.columns=id,start_time,description\n\
             \x20 report.c.columns=end_time\n\
             \x20 reports=end_time\n\
             \n"
        );
    }
}
