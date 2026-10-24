//! Porte de `src/vs/base/test/common/virtualScheduling/traceLogger.ts`: um logger mínimo para testes que
//! captura o [`Trace`] ativo na hora do log, para as mensagens poderem ser entrelaçadas depois num
//! diagrama de raias ao lado dos eventos de timer que as produziram.
//!
//! Notas de tradução:
//! - `ITraceLogger` (interface) é a struct [`TraceLogger`], e `ITraceLogEntry` (que estende
//!   `LogEntryLike`) é [`TraceLogEntry`], com [`TraceLogEntry::to_log_entry_like`] para o
//!   `build_history_from_tasks`.
//! - `logRun(fn)` loga `fn.toString()`, o texto-fonte da função. Um fecho Rust não tem texto-fonte, então
//!   quem chama passa o texto (`stringify!` da expressão serve) e [`TraceLogger::log_run`] o descreve do
//!   mesmo jeito que o `_describeFn` do original: tira o invólucro `() => ` ou `function () { }` (e o
//!   `|| ` / `move || ` do fecho Rust) e colapsa o espaço em branco. A função genérica sobre o retorno
//!   `T` é por isso um método e não um membro de objeto.
//! - O `buffer` é compartilhado (`Rc<RefCell<Vec<..>>>`), como o array do original.

use std::cell::RefCell;
use std::rc::Rc;

use super::trace::{Trace, TraceContext};
use crate::execution_graph::LogEntryLike;

/// O nível de uma entrada (`'log' | 'warn' | 'error'`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceLogLevel {
    Log,
    Warn,
    Error,
}

/// `ITraceLogEntry`: uma entrada de log com o nível, para os renderizadores diferenciarem `log`, `warn` e
/// `error`.
#[derive(Clone)]
pub struct TraceLogEntry {
    pub trace: Rc<Trace>,
    pub level: TraceLogLevel,
    pub message: String,
}

impl TraceLogEntry {
    /// A entrada no formato que o `build_history_from_tasks` consome.
    pub fn to_log_entry_like(&self) -> LogEntryLike {
        LogEntryLike { trace: self.trace.as_trace_like(), message: self.message.clone() }
    }
}

/// O buffer em que o logger empilha as entradas.
pub type TraceLogBuffer = Rc<RefCell<Vec<TraceLogEntry>>>;

/// `ITraceLogger`.
pub struct TraceLogger {
    buffer: TraceLogBuffer,
}

impl TraceLogger {
    fn push(&self, level: TraceLogLevel, message: &str) {
        let message = match level {
            TraceLogLevel::Log => message.to_owned(),
            TraceLogLevel::Warn => format!("[warn] {message}"),
            TraceLogLevel::Error => format!("[error] {message}"),
        };
        self.buffer.borrow_mut().push(TraceLogEntry { trace: TraceContext::instance().current_trace(), level, message });
    }

    pub fn log(&self, message: &str) {
        self.push(TraceLogLevel::Log, message);
    }

    pub fn warn(&self, message: &str) {
        self.push(TraceLogLevel::Warn, message);
    }

    pub fn error(&self, message: &str) {
        self.push(TraceLogLevel::Error, message);
    }

    /// Roda `f` de forma síncrona e a loga como um marcador no trace. O texto do marcador é a descrição
    /// de `source` (o texto-fonte de `f`), para os pontos de chamada lerem natural, como em
    /// `logger.log_run("model.trigger()", || model.trigger())`. Devolve o que `f` devolve.
    pub fn log_run<T>(&self, source: &str, f: impl FnOnce() -> T) -> T {
        self.log(&format!("run: {}", describe_fn(source)));
        f()
    }
}

/// `createTraceLogger(buffer)`: um logger que empilha cada chamada em `buffer`, marcando cada entrada com
/// o trace que é o corrente na hora da chamada.
///
/// Passe o mesmo `buffer` ao `build_history_from_tasks` (via [`TraceLogEntry::to_log_entry_like`]) para
/// entrelaçar as linhas de log com as raias dos timers.
pub fn create_trace_logger(buffer: TraceLogBuffer) -> TraceLogger {
    TraceLogger { buffer }
}

/// `_describeFn`: descrição de uma linha do texto-fonte de `f` para os marcadores de log.
fn describe_fn(source: &str) -> String {
    let body = arrow_body(source).or_else(|| function_expression_body(source)).or_else(|| closure_body(source)).unwrap_or(source);
    collapse_whitespace(body)
}

/// O texto sem um `async` inicial seguido de espaço (`(?:async\s+)?`).
fn skip_async(source: &str) -> &str {
    match source.strip_prefix("async") {
        Some(rest) if rest.starts_with(char::is_whitespace) => rest.trim_start(),
        _ => source,
    }
}

/// `() => corpo` (com `async` opcional): o corpo.
fn arrow_body(source: &str) -> Option<&str> {
    let rest = skip_async(source.trim_start()).strip_prefix('(')?.trim_start().strip_prefix(')')?.trim_start().strip_prefix("=>")?;
    non_empty(rest.trim())
}

/// `function nome() { corpo }` (com `async` opcional): o corpo.
fn function_expression_body(source: &str) -> Option<&str> {
    let rest = skip_async(source.trim_start()).strip_prefix("function")?.trim_start();
    let rest = rest.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_').trim_start();
    let rest = rest.strip_prefix('(')?.trim_start().strip_prefix(')')?.trim_start().strip_prefix('{')?;
    non_empty(rest.trim_end().strip_suffix('}')?.trim())
}

/// `|| corpo` ou `move || corpo` (o fecho Rust sem parâmetros): o corpo.
fn closure_body(source: &str) -> Option<&str> {
    let source = source.trim_start();
    let source = source.strip_prefix("move").filter(|rest| rest.starts_with(char::is_whitespace)).map_or(source, str::trim_start);
    non_empty(source.strip_prefix("||")?.trim())
}

fn non_empty(text: &str) -> Option<&str> {
    (!text.is_empty()).then_some(text)
}

/// `s.replace(/\s+/g, ' ').trim()`.
fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
