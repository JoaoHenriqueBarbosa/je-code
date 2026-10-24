//! Porte de `src/vs/workbench/contrib/output/test/browser/outputChannelModel.test.ts` (suíte `Logs Parsing`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite`.** O guarda de vazamentos é o [`LeakGuard`] criado no começo de cada
//!   caso. O `workbenchInstantiationService` do original só serve para criar o `TextModel` (`createInstance(TextModel, content,
//!   'log', TextModel.DEFAULT_CREATION_OPTIONS, null)`); o porte cria o modelo com o `create_text_model_with` do `testTextModel`,
//!   com a linguagem `log`, as opções padrão e sem URI, e o descarta com o guarda.
//! - **`new Date(text).getTime()`** é o `try_parse_date` do porte (o mesmo `Date.parse` que a produção usa).
//! - **`entry?.x`.** Um `Option<ILogEntry>` ausente é o `undefined`/`null` do original.
//! - **`for ... of Object.entries(...)`** e `for (const timestamp of ...)` são laços dentro do caso (o original os tem num caso só).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[allow(dead_code)]
#[path = "../../je-editor/tests/support/test_language_configuration_service.rs"]
mod test_language_configuration_service;
#[allow(dead_code)]
#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::text_model::TextModel;
use je_platform::log::LogLevel;
use je_workbench::contrib::output::common::output_channel_model::parse_log_entry_at;
use je_workbench::contrib::update::update_utils::try_parse_date;
use test_text_model::create_text_model_with;

fn create_model(guard: &LeakGuard, content: &str) -> Rc<TextModel> {
    guard.add_rc(create_text_model_with(content, Some("log"), RelaxedTextModelCreationOptions::default(), None))
}

/// `new Date(text).getTime()`.
fn time_of(text: &str) -> f64 {
    try_parse_date(Some(text)).expect("a data interpreta")
}

#[test]
fn should_parse_log_entry_with_all_components() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "2023-10-15 14:30:45.123 [info] [Git] Initializing repository";
    let model = create_model(&guard, text);
    let entry = parse_log_entry_at(&model, 1).expect("há uma entrada");

    assert_eq!(entry.timestamp, time_of("2023-10-15 14:30:45.123"));
    assert_eq!(entry.log_level, LogLevel::Info);
    assert_eq!(entry.category.as_deref(), Some("Git"));
    assert_eq!(model.get_value_in_range(&entry.range), text);
}

#[test]
fn should_parse_multi_line_log_entry() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = [
        "2023-10-15 14:30:45.123 [error] [Extension] Failed with error:",
        "Error: Could not load extension",
        "    at Object.load (/path/to/file:10:5)",
    ]
    .join("\n");
    let model = create_model(&guard, &text);
    let entry = parse_log_entry_at(&model, 1).expect("há uma entrada");

    assert_eq!(entry.timestamp, time_of("2023-10-15 14:30:45.123"));
    assert_eq!(entry.log_level, LogLevel::Error);
    assert_eq!(entry.category.as_deref(), Some("Extension"));
    assert_eq!(model.get_value_in_range(&entry.range), text.as_str());
}

#[test]
fn should_parse_log_entry_without_category() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "2023-10-15 14:30:45.123 [warning] System is running low on memory";
    let model = create_model(&guard, text);
    let entry = parse_log_entry_at(&model, 1).expect("há uma entrada");

    assert_eq!(entry.timestamp, time_of("2023-10-15 14:30:45.123"));
    assert_eq!(entry.log_level, LogLevel::Warning);
    assert_eq!(entry.category, None);
    assert_eq!(model.get_value_in_range(&entry.range), text);
}

#[test]
fn should_return_null_for_invalid_log_entry() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_model(&guard, "Not a valid log entry");
    let entry = parse_log_entry_at(&model, 1);

    assert_eq!(entry, None);
}

#[test]
fn should_parse_all_supported_log_levels() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let levels = [("info", LogLevel::Info), ("trace", LogLevel::Trace), ("debug", LogLevel::Debug), ("warning", LogLevel::Warning), ("error", LogLevel::Error)];

    for (level_text, expected_level) in levels {
        let model = create_model(&guard, &format!("2023-10-15 14:30:45.123 [{level_text}] Test message"));
        let entry = parse_log_entry_at(&model, 1);
        assert_eq!(entry.map(|entry| entry.log_level), Some(expected_level), "Failed for log level: {level_text}");
    }
}

#[test]
fn should_parse_timestamp_correctly() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let timestamps = ["2023-01-01 00:00:00.000", "2023-12-31 23:59:59.999", "2023-06-15 12:30:45.500"];

    for timestamp in timestamps {
        let model = create_model(&guard, &format!("{timestamp} [info] Test message"));
        let entry = parse_log_entry_at(&model, 1);
        assert_eq!(entry.map(|entry| entry.timestamp), Some(time_of(timestamp)), "Failed for timestamp: {timestamp}");
    }
}

#[test]
fn should_handle_last_line_of_file() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_model(&guard, &["2023-10-15 14:30:45.123 [info] First message", "2023-10-15 14:30:45.124 [info] Last message", ""].join("\n"));

    let mut actual = parse_log_entry_at(&model, 1).expect("há uma entrada");
    assert_eq!(actual.timestamp, time_of("2023-10-15 14:30:45.123"));
    assert_eq!(actual.log_level, LogLevel::Info);
    assert_eq!(actual.category, None);
    assert_eq!(model.get_value_in_range(&actual.range), "2023-10-15 14:30:45.123 [info] First message");

    actual = parse_log_entry_at(&model, 2).expect("há uma entrada");
    assert_eq!(actual.timestamp, time_of("2023-10-15 14:30:45.124"));
    assert_eq!(actual.log_level, LogLevel::Info);
    assert_eq!(actual.category, None);
    assert_eq!(model.get_value_in_range(&actual.range), "2023-10-15 14:30:45.124 [info] Last message");

    assert_eq!(parse_log_entry_at(&model, 3), None);
}

#[test]
fn should_parse_multi_line_log_entry_with_empty_lines() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = [
        "2025-01-27 09:53:00.450 [info] Found with version <20.18.1>",
        "Now using node v20.18.1 (npm v10.8.2)",
        "",
        "> husky - npm run -s precommit",
        "> husky - node v20.18.1",
        "",
        "Reading git index versions...",
    ]
    .join("\n");
    let model = create_model(&guard, &text);
    let entry = parse_log_entry_at(&model, 1).expect("há uma entrada");

    assert_eq!(entry.timestamp, time_of("2025-01-27 09:53:00.450"));
    assert_eq!(entry.log_level, LogLevel::Info);
    assert_eq!(entry.category, None);
    assert_eq!(model.get_value_in_range(&entry.range), text.as_str());
}
