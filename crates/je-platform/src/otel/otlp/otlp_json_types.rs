//! Porte de `src/vs/platform/otel/node/otlp/otlpJsonTypes.ts`.
//!
//! Tipos mínimos do corpo JSON do OTLP/HTTP para a requisição de exportação de traces.
//!
//! Referência:
//! - opentelemetry-proto/opentelemetry/proto/trace/v1/trace.proto
//! - opentelemetry-proto/opentelemetry/proto/common/v1/common.proto
//! - opentelemetry-proto/opentelemetry/proto/resource/v1/resource.proto
//! - opentelemetry-proto/docs/specification.md#json-protobuf-encoding
//!
//! Regras da codificação JSON usadas aqui:
//! - nomes de campo em camelCase (NÃO snake_case);
//! - `traceId`/`spanId`/`parentSpanId` são strings hexadecimais minúsculas (32 e 16 caracteres;
//!   `parentSpanId` pode ser vazio);
//! - `startTimeUnixNano`/`endTimeUnixNano`/`timeUnixNano` são inteiros decimais de 64 bits sem sinal,
//!   escritos como string;
//! - `intValue` de `AnyValue` é um int64 em string; os demais campos numéricos são números JSON;
//! - `bytesValue` é base64;
//! - em proto3 todo campo é opcional, e o produtor PODE omitir os valores padrão.
//!
//! Só o subconjunto de campos consumido hoje é modelado. Campos desconhecidos são ignorados pelo
//! decodificador (compatível com o futuro).
//!
//! Divergência de representação: as `interface` do original (`IOtlpAnyValue`, `IOtlpSpan`...) existem só
//! em tempo de compilação, e o decodificador as recebe do JSON como `unknown` (os testes passam valores
//! malformados com `as never`). Aqui o corpo é, portanto, o valor dinâmico do JSON, e os aliases abaixo
//! documentam a forma esperada de cada nível. Os enums, que têm valor em tempo de execução, e a resposta,
//! que o porte constrói em vez de ler, são tipos de verdade.

use je_base::dynamic::Dynamic;

/// Código de status, como no enum de status do trace OTLP (`OtlpStatusCode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OtlpStatusCode {
    Unset = 0,
    Ok = 1,
    Error = 2,
}

impl OtlpStatusCode {
    /// O valor numérico que vai no JSON.
    pub fn value(self) -> f64 {
        f64::from(self as u8)
    }

    /// O `switch (code)` do original: `OK` e `ERROR` casam por igualdade estrita com o número, e tudo o
    /// mais (`UNSET`, ausente, número desconhecido, outro tipo) cai em `UNSET`.
    pub fn from_value(value: &Dynamic) -> Self {
        match value {
            Dynamic::Number(n) if *n == Self::Ok.value() => Self::Ok,
            Dynamic::Number(n) if *n == Self::Error.value() => Self::Error,
            _ => Self::Unset,
        }
    }
}

/// Tipo de span, como no enum `span_kind` do trace OTLP (`OtlpSpanKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OtlpSpanKind {
    Unspecified = 0,
    Internal = 1,
    Server = 2,
    Client = 3,
    Producer = 4,
    Consumer = 5,
}

impl OtlpSpanKind {
    /// O valor numérico que vai no JSON.
    pub fn value(self) -> f64 {
        f64::from(self as u8)
    }
}

/// `IOtlpAnyValue`: um valor de atributo. Exatamente um dos campos opcionais está presente, exceto nos
/// valores vazios, que podem faltar por inteiro (padrões do proto3). Campos: `stringValue: string`,
/// `boolValue: boolean`, `intValue: string | number` (int64 em string), `doubleValue: number`,
/// `arrayValue: { values?: IOtlpAnyValue[] }`, `kvlistValue: { values?: IOtlpKeyValue[] }`,
/// `bytesValue: string` (base64).
pub type OtlpAnyValue = Dynamic;

/// `IOtlpKeyValue`: `{ key: string, value?: IOtlpAnyValue }`.
pub type OtlpKeyValue = Dynamic;

/// `IOtlpStatus`: `{ code?: OtlpStatusCode, message?: string }`.
pub type OtlpStatus = Dynamic;

/// `IOtlpEvent`: `{ timeUnixNano?, name?, attributes?: IOtlpKeyValue[], droppedAttributesCount? }`.
pub type OtlpEvent = Dynamic;

/// `IOtlpLink`: `{ traceId?, spanId?, traceState?, attributes?, droppedAttributesCount?, flags? }`.
pub type OtlpLink = Dynamic;

/// `IOtlpSpan`: `{ traceId, spanId, traceState?, parentSpanId?, flags?, name?, kind?: OtlpSpanKind,
/// startTimeUnixNano?, endTimeUnixNano?, attributes?, droppedAttributesCount?, events?: IOtlpEvent[],
/// droppedEventsCount?, links?: IOtlpLink[], droppedLinksCount?, status?: IOtlpStatus }`.
pub type OtlpSpan = Dynamic;

/// `IOtlpInstrumentationScope`: `{ name?, version?, attributes?, droppedAttributesCount? }`.
pub type OtlpInstrumentationScope = Dynamic;

/// `IOtlpScopeSpans`: `{ scope?: IOtlpInstrumentationScope, spans?: IOtlpSpan[], schemaUrl? }`.
pub type OtlpScopeSpans = Dynamic;

/// `IOtlpResource`: `{ attributes?: IOtlpKeyValue[], droppedAttributesCount? }`.
pub type OtlpResource = Dynamic;

/// `IOtlpResourceSpans`: `{ resource?: IOtlpResource, scopeSpans?: IOtlpScopeSpans[], schemaUrl? }`.
pub type OtlpResourceSpans = Dynamic;

/// `IOtlpExportTraceServiceRequest`: o corpo de `POST /v1/traces`, `{ resourceSpans?: IOtlpResourceSpans[] }`.
pub type OtlpExportTraceServiceRequest = Dynamic;

/// O `partialSuccess` de [`OtlpExportTraceServiceResponse`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OtlpPartialSuccess {
    pub rejected_spans: Option<f64>,
    pub error_message: Option<String>,
}

/// `IOtlpExportTraceServiceResponse`: a resposta de `POST /v1/traces`.
///
/// Em sucesso total, `{}` é aceitável (pela especificação). Em sucesso parcial, `partialSuccess` vem
/// preenchido e a requisição NÃO é reenviada.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OtlpExportTraceServiceResponse {
    pub partial_success: Option<OtlpPartialSuccess>,
}
