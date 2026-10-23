//! Porte de `src/vs/platform/otel/node/otlp/otlpJsonDecode.ts`.
//!
//! Decodifica o `ExportTraceServiceRequest` do OTLP/HTTP JSON em registros [`CompletedSpanData`] planos.
//!
//! O decodificador é leniente de propósito: um span malformado é descartado e reportado em
//! [`DecodeResult::errors`], para o receptor poder devolver 200 com `partial_success` preenchido (pela
//! especificação OTLP/HTTP) em vez de falhar o lote inteiro.
//!
//! Os atributos do recurso (por exemplo `service.name`) são mesclados no mapa de atributos de cada span, e
//! o atributo do span vence na colisão de chave. Isso mantém o esquema do SQLite plano e dispensa uma
//! tabela de recurso. Campos OTLP desconhecidos (`links`, `traceState`...) são ignorados em silêncio.
//!
//! A entrada é o JSON como valor dinâmico (o `unknown` do TypeScript), então o porte reproduz a semântica
//! do JavaScript sobre valores de tipo errado: ler propriedade de `undefined`/`null`, chamar
//! `toLowerCase` ou `slice` em não string e iterar um não iterável lançam `TypeError`, que dentro de um
//! span vira `errors[]` (o `try/catch` do original, via `je_base::errors::catch`) e fora dele, nos
//! atributos do recurso, propaga como pânico, como a exceção do original.
//!
//! Divergências:
//! - `int64` em string: o original usa `Number(...)` (perde precisão além de `Number.MAX_SAFE_INTEGER`),
//!   e o porte usa `f64`, igual. Não há `BigInt` no caminho.
//! - Os campos que o tipo declara `string` (`name`, `status.message`) e chegam com outro tipo seguem como
//!   `String(valor)`; `nulo` e ausente viram vazio e `None`.
//! - No `nanosToMillis` com valor que não é string, nem `undefined`: o original chama `s.slice`, que só
//!   existe em string (e em array, caso não modelado aqui, que também lança).
//! - As mensagens de `TypeError` de valores de tipo errado seguem o V8 nos casos comuns; as de
//!   "is not iterable" e "is not a function" usam o texto da expressão do original.

use je_base::JsString;
use je_base::dynamic::{Dynamic, ObjectRef, Truthy, json_stringify};
use je_base::errors::{self, Error};

use super::otlp_json_types::{OtlpExportTraceServiceRequest, OtlpStatusCode};
use crate::js::{string_to_number, to_js_string};
use crate::otel::span_data::{AttributeValue, Attributes, CompletedSpanData, SpanEventRecord, SpanStatus, SpanStatusCode};

const ALL_ZERO_TRACE_ID: &str = "00000000000000000000000000000000";
const ALL_ZERO_SPAN_ID: &str = "0000000000000000";

/// `IDecodeResult`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DecodeResult {
    pub spans: Vec<CompletedSpanData>,
    pub rejected: usize,
    pub errors: Vec<String>,
}

/// `decodeExportTraceRequest`. `request` é `undefined` (`&Dynamic::Undefined`) quando não há corpo.
pub fn decode_export_trace_request(request: &OtlpExportTraceServiceRequest) -> DecodeResult {
    let mut result = DecodeResult::default();
    if !request.is_truthy() {
        return result;
    }
    let resource_spans = request.get_str("resourceSpans");
    let Dynamic::Array(resource_spans) = resource_spans else {
        return result;
    };

    for rs in resource_spans.to_vec() {
        if !rs.is_truthy() {
            continue;
        }
        let resource_attrs = decode_attributes(&get_optional(&rs.get_str("resource"), "attributes"));

        for ss in iterate_or_empty(&rs.get_str("scopeSpans"), "rs.scopeSpans") {
            if !ss.is_truthy() {
                continue;
            }
            for span in iterate_or_empty(&ss.get_str("spans"), "ss.spans") {
                match errors::catch(|| decode_span(&span, &resource_attrs)) {
                    Ok(Some(decoded)) => result.spans.push(decoded),
                    Ok(None) => result.rejected += 1,
                    Err(error) => {
                        result.rejected += 1;
                        result.errors.push(error.message);
                    }
                }
            }
        }
    }

    result
}

/// `throw new TypeError(message)`.
fn type_error(message: String) -> ! {
    errors::throw(Error::type_error(message))
}

/// `value?.key`.
fn get_optional(value: &Dynamic, key: &str) -> Dynamic {
    if value.is_nullish() { Dynamic::Undefined } else { value.get_str(key) }
}

/// Os elementos de `for (const x of value)`: arrays, arrays tipados e strings (um elemento por unidade
/// de código, ou por ponto de código quando o par de surrogates é válido). O resto lança `TypeError`.
fn iterate(value: &Dynamic, expression: &str) -> Vec<Dynamic> {
    match value {
        Dynamic::Array(items) => items.to_vec(),
        Dynamic::TypedArray(_) => value.own_entries().into_iter().map(|(_, item)| item).collect(),
        Dynamic::String(text) => char::decode_utf16(text.units().iter().copied())
            .map(|decoded| match decoded {
                Ok(c) => Dynamic::string(c.encode_utf8(&mut [0; 4])),
                Err(lone) => Dynamic::String(JsString::from_units(vec![lone.unpaired_surrogate()])),
            })
            .collect(),
        _ => type_error(format!("{expression} is not iterable")),
    }
}

/// `for (const x of value ?? [])`.
fn iterate_or_empty(value: &Dynamic, expression: &str) -> Vec<Dynamic> {
    if value.is_nullish() { Vec::new() } else { iterate(value, expression) }
}

/// O texto de um campo que o tipo declara `string`: `None` para `undefined`/`null`.
fn optional_string(value: &Dynamic) -> Option<JsString> {
    match value {
        Dynamic::Undefined | Dynamic::Null => None,
        Dynamic::String(text) => Some(text.clone()),
        other => Some(to_js_string(other)),
    }
}

/// `value.toLowerCase()`, que só existe em string.
fn to_lower_case(value: &Dynamic, expression: &str) -> JsString {
    match value {
        Dynamic::String(text) => text.to_lower_case(),
        _ => type_error(format!("{expression}.toLowerCase is not a function")),
    }
}

fn decode_span(span: &Dynamic, resource_attrs: &Attributes) -> Option<CompletedSpanData> {
    if !span.is_truthy() {
        return None;
    }

    let trace_id_value = span.get_str("traceId");
    let span_id_value = span.get_str("spanId");
    // `(span.traceId ?? '').toLowerCase()`
    let lower_or_empty = |value: &Dynamic| if value.is_nullish() { JsString::new() } else { to_lower_case(value, "(intermediate value)") };
    let trace_id = lower_or_empty(&trace_id_value);
    let span_id = lower_or_empty(&span_id_value);
    if !is_valid_hex(&trace_id, 32) || trace_id == ALL_ZERO_TRACE_ID {
        errors::throw(Error::new(format!("invalid traceId: {}", to_js_string(&trace_id_value))));
    }
    if !is_valid_hex(&span_id, 16) || span_id == ALL_ZERO_SPAN_ID {
        errors::throw(Error::new(format!("invalid spanId: {}", to_js_string(&span_id_value))));
    }

    let parent_span_id_value = span.get_str("parentSpanId");
    let parent_span_id = if parent_span_id_value.is_truthy() {
        let parent = to_lower_case(&parent_span_id_value, "span.parentSpanId");
        (is_valid_hex(&parent, 16) && parent != ALL_ZERO_SPAN_ID).then_some(parent)
    } else {
        None
    };

    let start_time = nanos_to_millis(&span.get_str("startTimeUnixNano"));
    let end_time = nanos_to_millis(&span.get_str("endTimeUnixNano"));
    let (Some(start_time), Some(end_time)) = (start_time, end_time) else {
        errors::throw(Error::new("missing span time bounds"));
    };

    let mut attributes = resource_attrs.clone();
    for kv in iterate_or_empty(&span.get_str("attributes"), "span.attributes") {
        set_attribute(&mut attributes, &kv);
    }

    let events = iterate_or_empty(&span.get_str("events"), "span.events").iter().filter_map(decode_event).collect();

    let status_value = span.get_str("status");
    let status = decode_status(&get_optional(&status_value, "code"), &get_optional(&status_value, "message"));

    Some(CompletedSpanData {
        name: optional_string(&span.get_str("name")).unwrap_or_default(),
        trace_id,
        span_id,
        parent_span_id,
        start_time,
        end_time,
        status,
        attributes,
        events,
    })
}

fn decode_event(ev: &Dynamic) -> Option<SpanEventRecord> {
    if !ev.is_truthy() {
        return None;
    }
    let timestamp = nanos_to_millis(&ev.get_str("timeUnixNano"))?;
    let mut attributes = Attributes::new();
    for kv in iterate_or_empty(&ev.get_str("attributes"), "ev.attributes") {
        set_attribute(&mut attributes, &kv);
    }
    Some(SpanEventRecord {
        name: optional_string(&ev.get_str("name")).unwrap_or_default(),
        timestamp,
        attributes: (!attributes.is_empty()).then_some(attributes),
    })
}

/// `decodeStatus`: `OK` e `ERROR` mapeiam para si, e `UNSET`, ausente e desconhecido para `UNSET`; a
/// mensagem acompanha o código nos três casos.
fn decode_status(code: &Dynamic, message: &Dynamic) -> SpanStatus {
    let code = match OtlpStatusCode::from_value(code) {
        OtlpStatusCode::Ok => SpanStatusCode::Ok,
        OtlpStatusCode::Error => SpanStatusCode::Error,
        OtlpStatusCode::Unset => SpanStatusCode::Unset,
    };
    SpanStatus { code, message: optional_string(message) }
}

fn decode_attributes(kvs: &Dynamic) -> Attributes {
    let mut out = Attributes::new();
    if !kvs.is_truthy() {
        return out;
    }
    for kv in iterate(kvs, "kvs") {
        set_attribute(&mut out, &kv);
    }
    out
}

fn set_attribute(target: &mut Attributes, kv: &Dynamic) {
    if !kv.is_truthy() {
        return;
    }
    let Dynamic::String(key) = kv.get_str("key") else {
        return;
    };
    if key.is_empty() {
        return;
    }
    if let Some(value) = decode_any_value(&kv.get_str("value")) {
        target.insert(key, value);
    }
}

/// O `JSON.stringify` de um array ou objeto, como atributo de texto.
fn json_text(value: &Dynamic) -> AttributeValue {
    AttributeValue::String(JsString::from(json_stringify(value).expect("JSON.stringify de array ou objeto devolve texto")))
}

fn decode_any_value(v: &Dynamic) -> Option<AttributeValue> {
    if !v.is_truthy() {
        return None;
    }
    if let Dynamic::String(text) = v.get_str("stringValue") {
        return Some(AttributeValue::String(text));
    }
    if let Dynamic::Bool(flag) = v.get_str("boolValue") {
        return Some(AttributeValue::Bool(flag));
    }
    let int_value = v.get_str("intValue");
    if !int_value.is_undefined() {
        // intValue é um int64 em string; a precisão além de Number.MAX_SAFE_INTEGER se perde
        let n = match &int_value {
            Dynamic::String(text) => string_to_number(text.units()),
            Dynamic::Number(n) => *n,
            // `Number.isFinite` não converte: qualquer outro tipo não é finito
            _ => f64::NAN,
        };
        return n.is_finite().then_some(AttributeValue::Number(n));
    }
    if let Dynamic::Number(n) = v.get_str("doubleValue") {
        return Some(AttributeValue::Number(n));
    }
    let array_values = get_optional(&v.get_str("arrayValue"), "values");
    if array_values.is_truthy() {
        let Dynamic::Array(array_values) = &array_values else {
            type_error("v.arrayValue.values.map is not a function".to_string());
        };
        // Só arrays planos de strings são de primeira classe nos atributos do span.
        // Para arrays mistos, numéricos ou aninhados, o JSON preserva a fidelidade.
        let items: Vec<Option<AttributeValue>> = array_values.to_vec().iter().map(decode_any_value).collect();
        let strings: Option<Vec<JsString>> = items
            .iter()
            .map(|item| match item {
                Some(AttributeValue::String(text)) => Some(text.clone()),
                _ => None,
            })
            .collect();
        return Some(match strings {
            Some(strings) => AttributeValue::Strings(strings),
            None => json_text(&Dynamic::array(items.iter().map(|item| item.as_ref().map_or(Dynamic::Undefined, AttributeValue::to_dynamic)))),
        });
    }
    let kvlist_values = get_optional(&v.get_str("kvlistValue"), "values");
    if kvlist_values.is_truthy() {
        // Raro; preserva como JSON.
        let object = ObjectRef::new();
        for kv in iterate(&kvlist_values, "v.kvlistValue.values") {
            if kv.is_truthy()
                && let Dynamic::String(key) = kv.get_str("key")
            {
                object.set(&key, decode_any_value(&kv.get_str("value")).map_or(Dynamic::Undefined, |value| value.to_dynamic()));
            }
        }
        return Some(json_text(&Dynamic::Object(object)));
    }
    if let Dynamic::String(text) = v.get_str("bytesValue") {
        return Some(AttributeValue::String(text));
    }
    None
}

/// `nanosToMillis`: nanossegundos em decimal (string) para milissegundos, truncando os 6 últimos dígitos.
/// `s` é um inteiro não negativo pela especificação. `None` para ausente, `''`, `'0'` e o que não vira
/// número finito.
fn nanos_to_millis(s: &Dynamic) -> Option<f64> {
    let text = match s {
        Dynamic::Undefined => return None,
        Dynamic::String(text) => text,
        // `s.length` lança para `null`; para os demais tipos o original chega ao `s.slice`, que não existe
        other => {
            other.get_str("length");
            type_error("s.slice is not a function".to_string());
        }
    };
    if text.is_empty() || *text == "0" {
        return None;
    }
    // Evita BigInt: lê como string decimal e corta os 6 últimos dígitos (ns -> ms).
    let trimmed = if text.len() <= 6 { JsString::from("0") } else { text.slice(0, text.len() - 6) };
    let n = string_to_number(trimmed.units());
    n.is_finite().then_some(n)
}

/// `s.length === len && /^[0-9a-fA-F]+$/.test(s)`.
fn is_valid_hex(s: &JsString, len: usize) -> bool {
    s.len() == len && s.units().iter().all(|&unit| u8::try_from(unit).is_ok_and(|byte| byte.is_ascii_hexdigit()))
}
