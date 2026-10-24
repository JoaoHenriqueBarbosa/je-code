//! Porte parcial de `src/vs/workbench/contrib/output/common/outputChannelModel.ts`: o `ILogEntry` (de
//! `services/output/common/output.ts`) e `parseLogEntryAt`, que lê uma entrada de log (com as linhas de continuação) de um
//! modelo de texto.
//!
//! Fora do porte: os modelos de canal (`OutputChannelModel` e derivados), que dependem do serviço de arquivos, do serviço de
//! logs e do editor de saída, e as funções auxiliares privadas deles (`logEntryIterator`, `changeStartLineNumber`).
//!
//! Notas de tradução:
//! - **`new Date(match[1]).getTime()`** é o `Date.parse` do porte ([`try_parse_date`], fuso local para data e hora sem
//!   deslocamento); um valor que não interpreta vira `NaN`, como no JavaScript.
//! - **`LOG_ENTRY_REGEX`** é um [`JsRegExp`] sem estado (sem `g`), então cada `exec` parte do zero.
//! - **`ITextModel`** é o [`TextModel`] concreto.

use std::cell::LazyCell;

use je_base::errors::{self, Error};
use je_base::js_regexp::JsRegExp;
use je_base::js_string::JsString;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_platform::log::LogLevel;

use crate::contrib::update::update_utils::try_parse_date;

/// `ILogEntry`.
#[derive(Clone, Debug, PartialEq)]
pub struct ILogEntry {
    pub range: Range,
    pub timestamp: f64,
    pub timestamp_range: Range,
    pub log_level: LogLevel,
    pub log_level_range: Range,
    pub category: Option<String>,
}

thread_local! {
    static LOG_ENTRY_REGEX: LazyCell<JsRegExp> = LazyCell::new(|| {
        JsRegExp::from_str(r"^(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3})\s(\[(info|trace|debug|error|warning)\])\s(\[(.*?)\])?", "")
            .expect("regex do registro de log válida")
    });
}

/// `parseLogEntryAt`.
pub fn parse_log_entry_at(model: &TextModel, line_number: i32) -> Option<ILogEntry> {
    let line_content = CursorSimpleModel::get_line_content(model, line_number);
    let found = LOG_ENTRY_REGEX.with(|regex| regex.exec(line_content.units()))?;
    let group = |index: usize| found.captures.get(index).and_then(|range| range.clone()).map(|range| JsString::from(&line_content.units()[range]));

    let timestamp_text = group(1).expect("o grupo do carimbo de tempo participa").to_string_lossy();
    let timestamp = try_parse_date(Some(&timestamp_text)).unwrap_or(f64::NAN);
    let timestamp_range = Range::new(line_number, 1, line_number, timestamp_text.encode_utf16().count() as i32);
    let log_level = parse_log_level(&group(3).expect("o grupo do nível participa").to_string_lossy());
    let log_level_text_length = group(2).expect("o grupo do nível entre colchetes participa").len() as i32;
    let log_level_range = Range::new(line_number, timestamp_range.end_column + 1, line_number, timestamp_range.end_column + 1 + log_level_text_length);
    let category = group(5).map(|category| category.to_string_lossy());
    let start_line = line_number;
    let mut end_line = line_number;

    let line_count = CursorSimpleModel::get_line_count(model);
    while end_line < line_count {
        let next_line_content = CursorSimpleModel::get_line_content(model, end_line + 1);
        let is_last_line = end_line + 1 == line_count && next_line_content.is_empty(); // Last line will be always empty
        if is_last_line || LOG_ENTRY_REGEX.with(|regex| regex.exec(next_line_content.units()).is_some()) {
            break;
        }
        end_line += 1;
    }
    let range = Range::new(start_line, 1, end_line, CursorSimpleModel::get_line_max_column(model, end_line));
    Some(ILogEntry { range, timestamp, timestamp_range, log_level, log_level_range, category })
}

fn parse_log_level(level: &str) -> LogLevel {
    match level.to_lowercase().as_str() {
        "trace" => LogLevel::Trace,
        "debug" => LogLevel::Debug,
        "info" => LogLevel::Info,
        "warning" => LogLevel::Warning,
        "error" => LogLevel::Error,
        _ => errors::throw(Error::new(format!("Unknown log level: {level}"))),
    }
}
