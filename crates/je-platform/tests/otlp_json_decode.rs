//! Porte de `src/vs/platform/otel/test/node/otlp/otlpJsonDecode.test.ts` (suíte `platform/otel - otlpJsonDecode`), caso a caso e na mesma ordem.
//!
//! Divergências: o corpo da requisição é o valor dinâmico do JSON (o `unknown` do original, que passa
//! valores malformados com `as never`), montado com literais no estilo do TypeScript (`j("{...}")`) e com
//! os construtores de `common`. Os `deepStrictEqual` sobre `ICompletedSpanData` comparam as structs
//! tipadas; o `Record` de atributos é um `IndexMap`, cuja igualdade não considera a ordem das chaves, como a
//! do `deepStrictEqual`. `ensureNoDisposablesAreLeakedInTestSuite` não tem o que vigiar (nada aqui é
//! descartável).

mod common;

use common::{arr, j, n, obj, s};
use je_base::JsString;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_platform::otel::otlp::otlp_json_decode::{DecodeResult, decode_export_trace_request};
use je_platform::otel::otlp::otlp_json_types::{OtlpSpanKind, OtlpStatusCode};
use je_platform::otel::span_data::{AttributeValue, Attributes, CompletedSpanData, SpanEventRecord, SpanStatus, SpanStatusCode};

// 1700000000 seconds = 2023-11-14T22:13:20Z
// 1_700_000_000_000_000_000 ns = same instant
const START_NS: &str = "1700000000000000000";
const END_NS: &str = "1700000000050000000"; // +50ms

const TRACE_ID: &str = "aabbccddeeff00112233445566778899";
const SPAN_ID: &str = "0011223344556677";
const PARENT_SPAN_ID: &str = "8899aabbccddeeff";

/// `minimalRequest(spans, resourceAttrs?)`.
fn minimal_request(spans: Vec<Dynamic>, resource_attrs: Option<Vec<Dynamic>>) -> Dynamic {
    obj(vec![(
        "resourceSpans",
        arr(vec![obj(vec![
            ("resource", resource_attrs.map_or(Dynamic::Undefined, |attributes| obj(vec![("attributes", arr(attributes))]))),
            (
                "scopeSpans",
                arr(vec![obj(vec![("scope", obj(vec![("name", s("@github/copilot/sdk")), ("version", s("0.0.0"))])), ("spans", arr(spans))])]),
            ),
        ])]),
    )])
}

/// `{ key, value }`.
fn kv(key: &str, value: Dynamic) -> Dynamic {
    obj(vec![("key", s(key)), ("value", value)])
}

/// `{ ...base, ...overrides }`.
fn spread(base: &Dynamic, overrides: Vec<(&str, Dynamic)>) -> Dynamic {
    let result = ObjectRef::from_entries(base.own_entries());
    for (key, value) in overrides {
        result.set(&JsString::from(key), value);
    }
    Dynamic::Object(result)
}

/// Os campos que quase todo span do arquivo repete: `traceId`, `spanId`, `name` e os dois tempos.
fn span(span_id: &str, name: &str, extra: Vec<(&str, Dynamic)>) -> Dynamic {
    let mut entries = vec![("traceId", s(TRACE_ID)), ("spanId", s(span_id)), ("name", s(name))];
    entries.extend(extra);
    entries.extend([("startTimeUnixNano", s(START_NS)), ("endTimeUnixNano", s(END_NS))]);
    obj(entries)
}

fn kind(kind: OtlpSpanKind) -> (&'static str, Dynamic) {
    ("kind", n(kind.value()))
}

fn text(value: &str) -> AttributeValue {
    AttributeValue::String(JsString::from(value))
}

fn attrs(entries: Vec<(&str, AttributeValue)>) -> Attributes {
    entries.into_iter().map(|(key, value)| (JsString::from(key), value)).collect()
}

fn attribute<'a>(span: &'a CompletedSpanData, key: &str) -> Option<&'a AttributeValue> {
    span.attributes.get(&JsString::from(key))
}

#[test]
fn decodes_a_complete_chat_span_with_all_attribute_types_and_resource_merge() {
    let request = minimal_request(
        vec![obj(vec![
            ("traceId", s(TRACE_ID)),
            ("spanId", s(SPAN_ID)),
            ("parentSpanId", s(PARENT_SPAN_ID)),
            ("name", s("chat gpt-4o")),
            kind(OtlpSpanKind::Client),
            ("startTimeUnixNano", s(START_NS)),
            ("endTimeUnixNano", s(END_NS)),
            (
                "attributes",
                arr(vec![
                    kv("gen_ai.operation.name", j("{ 'stringValue': 'chat' }")),
                    kv("gen_ai.usage.input_tokens", j("{ 'intValue': '1500' }")),
                    kv("gen_ai.request.temperature", j("{ 'doubleValue': 0.1 }")),
                    kv("copilot_chat.streaming", j("{ 'boolValue': true }")),
                    kv("gen_ai.response.finish_reasons", j("{ 'arrayValue': { 'values': [{ 'stringValue': 'stop' }] } }")),
                ]),
            ),
            (
                "events",
                arr(vec![obj(vec![
                    ("timeUnixNano", s(START_NS)),
                    ("name", s("gen_ai.user.message")),
                    ("attributes", arr(vec![kv("content", j("{ 'stringValue': 'hi' }"))])),
                ])]),
            ),
            ("status", obj(vec![("code", n(OtlpStatusCode::Ok.value()))])),
        ])],
        Some(vec![kv("service.name", j("{ 'stringValue': 'github-copilot' }"))]),
    );

    let DecodeResult { spans, rejected, errors } = decode_export_trace_request(&request);

    assert_eq!(rejected, 0);
    assert_eq!(errors, Vec::<String>::new());
    assert_eq!(spans.len(), 1);

    let [decoded] = spans.as_slice() else { unreachable!() };
    assert_eq!(
        *decoded,
        CompletedSpanData {
            name: JsString::from("chat gpt-4o"),
            trace_id: JsString::from(TRACE_ID),
            span_id: JsString::from(SPAN_ID),
            parent_span_id: Some(JsString::from(PARENT_SPAN_ID)),
            start_time: 1_700_000_000_000.0,
            end_time: 1_700_000_000_050.0,
            status: SpanStatus { code: SpanStatusCode::Ok, message: None },
            attributes: attrs(vec![
                ("service.name", text("github-copilot")),
                ("gen_ai.operation.name", text("chat")),
                ("gen_ai.usage.input_tokens", AttributeValue::Number(1500.0)),
                ("gen_ai.request.temperature", AttributeValue::Number(0.1)),
                ("copilot_chat.streaming", AttributeValue::Bool(true)),
                ("gen_ai.response.finish_reasons", AttributeValue::Strings(vec![JsString::from("stop")])),
            ]),
            events: vec![SpanEventRecord {
                name: JsString::from("gen_ai.user.message"),
                timestamp: 1_700_000_000_000.0,
                attributes: Some(attrs(vec![("content", text("hi"))])),
            }],
        }
    );
}

#[test]
fn handles_an_empty_request() {
    let empty = DecodeResult { spans: vec![], rejected: 0, errors: vec![] };
    assert_eq!(decode_export_trace_request(&obj(vec![("resourceSpans", arr(vec![]))])), empty);
    assert_eq!(decode_export_trace_request(&Dynamic::Undefined), empty);
}

#[test]
fn rejects_spans_with_invalid_trace_id_span_id_missing_times() {
    let valid = obj(vec![
        ("traceId", s(TRACE_ID)),
        ("spanId", s(SPAN_ID)),
        ("name", s("ok")),
        kind(OtlpSpanKind::Internal),
        ("startTimeUnixNano", s(START_NS)),
        ("endTimeUnixNano", s(END_NS)),
    ]);
    let request = obj(vec![(
        "resourceSpans",
        arr(vec![obj(vec![(
            "scopeSpans",
            arr(vec![obj(vec![(
                "spans",
                arr(vec![
                    valid.clone(),
                    spread(&valid, vec![("traceId", s("tooShort"))]),                                 // bad hex
                    spread(&valid, vec![("traceId", s("00000000000000000000000000000000"))]),         // all zero
                    spread(&valid, vec![("spanId", s("00ZZ00ZZ00ZZ00ZZ"))]),                         // non-hex
                    spread(&valid, vec![("startTimeUnixNano", Dynamic::Undefined)]),                  // missing time
                ]),
            )])]),
        )])]),
    )]);

    let DecodeResult { spans, rejected, .. } = decode_export_trace_request(&request);
    assert_eq!(spans.len(), 1);
    assert_eq!(rejected, 4);
    assert_eq!(spans[0].name, "ok");
}

#[test]
fn span_level_attribute_overrides_resource_attribute_on_collision() {
    let request = minimal_request(
        vec![span(
            SPAN_ID,
            "x",
            vec![kind(OtlpSpanKind::Internal), ("attributes", arr(vec![kv("service.name", j("{ 'stringValue': 'span-wins' }"))]))],
        )],
        Some(vec![kv("service.name", j("{ 'stringValue': 'resource-loses' }"))]),
    );
    let DecodeResult { spans, .. } = decode_export_trace_request(&request);
    assert_eq!(attribute(&spans[0], "service.name"), Some(&text("span-wins")));
}

#[test]
fn maps_status_codes_error_to_error_missing_to_unset() {
    let request = minimal_request(
        vec![
            span(
                "1111111111111111",
                "a",
                vec![kind(OtlpSpanKind::Internal), ("status", obj(vec![("code", n(OtlpStatusCode::Error.value())), ("message", s("boom"))]))],
            ),
            span("2222222222222222", "b", vec![kind(OtlpSpanKind::Internal)]),
        ],
        None,
    );
    let DecodeResult { spans, .. } = decode_export_trace_request(&request);
    assert_eq!(spans[0].status.code, SpanStatusCode::Error);
    assert_eq!(spans[0].status.message, Some(JsString::from("boom")));
    assert_eq!(spans[1].status.code, SpanStatusCode::Unset);
}

#[test]
fn falls_back_to_json_for_mixed_type_nested_attribute_arrays_and_kvlist_value() {
    let request = minimal_request(
        vec![span(
            SPAN_ID,
            "a",
            vec![
                kind(OtlpSpanKind::Internal),
                (
                    "attributes",
                    arr(vec![
                        kv("mixed", j("{ 'arrayValue': { 'values': [{ 'stringValue': 'x' }, { 'intValue': '7' }] } }")),
                        kv("nested", j("{ 'kvlistValue': { 'values': [{ 'key': 'k', 'value': { 'stringValue': 'v' } }] } }")),
                    ]),
                ),
            ],
        )],
        None,
    );
    let DecodeResult { spans, .. } = decode_export_trace_request(&request);
    assert_eq!(attribute(&spans[0], "mixed"), Some(&text("[\"x\",7]")));
    assert_eq!(attribute(&spans[0], "nested"), Some(&text("{\"k\":\"v\"}")));
}

#[test]
fn treats_time_stringified_as_decimal_nanos_and_floors_to_ms() {
    let request = minimal_request(
        vec![obj(vec![
            ("traceId", s(TRACE_ID)),
            ("spanId", s(SPAN_ID)),
            ("name", s("a")),
            kind(OtlpSpanKind::Internal),
            ("startTimeUnixNano", s("1700000000123456789")), // .123456789 sec
            ("endTimeUnixNano", s("1700000000123999999")),
        ])],
        None,
    );
    let DecodeResult { spans, .. } = decode_export_trace_request(&request);
    assert_eq!(spans[0].start_time, 1_700_000_000_123.0);
    assert_eq!(spans[0].end_time, 1_700_000_000_123.0);
}

#[test]
fn returns_errors_when_an_exception_is_thrown_for_a_span() {
    let request = minimal_request(
        vec![obj(vec![
            ("traceId", s("nothex")),
            ("spanId", s(SPAN_ID)),
            ("name", s("x")),
            kind(OtlpSpanKind::Internal),
            ("startTimeUnixNano", s(START_NS)),
            ("endTimeUnixNano", s(END_NS)),
        ])],
        None,
    );
    let DecodeResult { rejected, errors, .. } = decode_export_trace_request(&request);
    assert_eq!(rejected, 1);
    assert_eq!(errors.len(), 1);
}

#[test]
fn preserves_parent_span_id_drops_empty_all_zero_parent() {
    let parent_span = |span_id: &str, name: &str, parent: &str| {
        obj(vec![
            ("traceId", s(TRACE_ID)),
            ("spanId", s(span_id)),
            ("parentSpanId", s(parent)),
            ("name", s(name)),
            ("startTimeUnixNano", s(START_NS)),
            ("endTimeUnixNano", s(END_NS)),
        ])
    };
    let request = minimal_request(
        vec![
            parent_span("1111111111111111", "a", "0000000000000000"),
            parent_span("2222222222222222", "b", ""),
            parent_span("3333333333333333", "c", "ddccbbaa11223344"),
        ],
        None,
    );
    let DecodeResult { spans, .. } = decode_export_trace_request(&request);
    assert_eq!(spans[0].parent_span_id, None);
    assert_eq!(spans[1].parent_span_id, None);
    assert_eq!(spans[2].parent_span_id, Some(JsString::from("ddccbbaa11223344")));
}
