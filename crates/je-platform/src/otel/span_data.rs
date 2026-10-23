//! Porte de `src/vs/platform/otel/common/spanData.ts`.
//!
//! Tipos mínimos, independentes de SDK, para spans OTel concluídos. Espelham a forma usada pela extensão
//! Copilot, para que o mesmo processador-ponte possa ser portado para o Agent Host sem trazer toda a
//! superfície do `IOTelService` da extensão.
//!
//! Escolhas de representação:
//! - O texto é [`JsString`] (UTF-16, como o `string` do JavaScript), porque vem de JSON já decodificado.
//! - `Readonly<Record<string, string | number | boolean | string[]>>` é [`Attributes`], um mapa na ordem de
//!   inserção (o `IndexMap` compara sem considerar a ordem, como o `deepStrictEqual` do original). A regra
//!   do JavaScript que põe chaves de índice de array primeiro não é reproduzida: nenhum consumidor depende
//!   da ordem das chaves.
//! - `string | number | boolean | string[]` é [`AttributeValue`]; tempos em milissegundos desde a época
//!   são `f64`, como o `number` do JavaScript.
//! - Campos `?:` são `Option`.

use indexmap::IndexMap;
use je_base::JsString;
use je_base::dynamic::Dynamic;

/// `SpanStatusCode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanStatusCode {
    Unset = 0,
    Ok = 1,
    Error = 2,
}

/// `string | number | boolean | string[]`: o valor de um atributo.
#[derive(Clone, Debug, PartialEq)]
pub enum AttributeValue {
    String(JsString),
    Number(f64),
    Bool(bool),
    Strings(Vec<JsString>),
}

impl AttributeValue {
    /// O mesmo valor como valor dinâmico (o que o `JSON.stringify` do original enxerga).
    pub fn to_dynamic(&self) -> Dynamic {
        match self {
            Self::String(text) => Dynamic::String(text.clone()),
            Self::Number(number) => Dynamic::Number(*number),
            Self::Bool(flag) => Dynamic::Bool(*flag),
            Self::Strings(items) => Dynamic::array(items.iter().cloned().map(Dynamic::String)),
        }
    }
}

/// `Readonly<Record<string, string | number | boolean | string[]>>`.
pub type Attributes = IndexMap<JsString, AttributeValue>;

/// `{ code: SpanStatusCode; message?: string }`.
#[derive(Clone, Debug, PartialEq)]
pub struct SpanStatus {
    pub code: SpanStatusCode,
    pub message: Option<JsString>,
}

/// `ISpanEventRecord`: um evento de um span.
#[derive(Clone, Debug, PartialEq)]
pub struct SpanEventRecord {
    pub name: JsString,
    /// Milissegundos desde a época.
    pub timestamp: f64,
    pub attributes: Option<Attributes>,
}

/// `ICompletedSpanData`: retrato serializável de um span concluído. Contém todos os atributos em memória
/// (inclusive os de conteúdo, independentemente de `captureContent`).
#[derive(Clone, Debug, PartialEq)]
pub struct CompletedSpanData {
    pub name: JsString,
    pub span_id: JsString,
    pub trace_id: JsString,
    pub parent_span_id: Option<JsString>,
    /// Milissegundos desde a época.
    pub start_time: f64,
    /// Milissegundos desde a época.
    pub end_time: f64,
    pub status: SpanStatus,
    pub attributes: Attributes,
    pub events: Vec<SpanEventRecord>,
}
