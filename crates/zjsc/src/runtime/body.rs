//! O corpo compartilhável de `Response` (e, adiante, `Request`): bytes mais o `Content-Type` implícito, a marca de uso e
//! as leituras `text`/`json`/`arrayBuffer`/`bytes`/`blob`. Medido no bun 1.4.2 (`scripts/gen-fetch-types-golden.js`):
//!
//! - extração: `undefined`/`null` não dão corpo; `Blob` vale com o seu `type`; `URLSearchParams` vira urlencoded com
//!   `application/x-www-form-urlencoded;charset=UTF-8`; `FormData` vira multipart com `multipart/form-data; boundary=...`;
//!   typed array, `DataView` e `ArrayBuffer` valem como bytes crus, sem tipo; o resto passa por `ToString` em UTF-8
//!   (símbolo lança `Cannot convert a symbol to a string`). Só `Blob`, `URLSearchParams` e `FormData` põem `Content-Type`
//!   nos cabeçalhos; o texto leva `text/plain;charset=utf-8` só no `blob()`;
//! - leitura: devolve promessa; num corpo que existe, a primeira marca `bodyUsed` e a segunda rejeita com `Body already
//!   used` (`ERR_BODY_ALREADY_USED`); sem corpo, ler não marca nada e pode repetir; `clone` de corpo usado lança `Body is
//!   disturbed or locked` (`ERR_BODY_ALREADY_USED`);
//! - `text` tira o BOM UTF-8 do começo (quem chama; `Blob.text` não tira).
//! - `formData()` olha o `Content-Type` dos CABEÇALHOS (o implícito do texto não vale): sem `urlencoded`, ou `multipart` com
//!   `boundary`, rejeita `TypeError` `Can't decode form data from body because of incorrect MIME type/boundary`
//!   (`ERR_FORMDATA_PARSE_ERROR`), corpo vazio inclusive; com ele, segue `form_data_parse`.
//!
//! `body` é um `ReadableStream` que não é de bytes (ver `streams/body_stream.rs`), o mesmo objeto a cada leitura da
//! propriedade; `bodyUsed` vale também com o stream travado ou lido, e então `text()` e afins rejeitam `Body already used`.
//!
//! - corpo `ReadableStream` do usuário (`new Response(stream)`, `new Request(url, {method:'POST', body: stream})`): guarda o
//!   próprio objeto (`body === stream`), sem `Content-Type` nos cabeçalhos; stream travado ou já lido lança no construtor
//!   `TypeError` `Body object should not be disturbed or locked` (sem `code`). Os métodos de leitura marcam `bodyUsed` na
//!   hora, leem os pedaços em cadeia de promessas por um leitor interno (o stream fica `locked: true` enquanto a leitura corre, salvo quando os pedaços e o fechamento já estão na fila) e rejeitam com o
//!   erro do stream (o motivo cru) ou, num pedaço que não é string, `ArrayBuffer` nem view, `TypeError` `Expected text,
//!   ArrayBuffer or ArrayBufferView`. `blob()` de stream tem `type` vazio, mesmo com `Content-Type` nos cabeçalhos.
//!
//! `clone()` de corpo que é stream do usuário divide o stream por `tee` (o original lê o primeiro ramo, o clone o
//! segundo); `textStream()` deixa o `body` com `locked: true`, como o bun.
//!
//! LACUNA medida contra o bun: `getReader()` durante a leitura interna (o bun deixa; aqui lança por stream travado).

use crate::runtime::js_promise_host::PromiseHost;
use std::cell::Cell;

use crate::runtime::array_buffer::{ArrayBuffer, ArrayBufferSharingMode};
use crate::runtime::blob::{make_blob, resolved_promise, state_of, string_bytes, BlobState};
use crate::runtime::form_data::{entries_of_instance, make_form_data, FormValue};
use crate::runtime::form_data_parse::{has_form_encoding, parse_body};
use crate::runtime::headers::header_value;
use crate::runtime::host_call::{HostResult, Thrown};
use crate::runtime::js_array_buffer::JSArrayBuffer;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_promise::JSPromise;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::JSValue;
use crate::runtime::json_object::{json_parse, throw_json_error};
use crate::runtime::literal_parser::JsonError;
use crate::runtime::node_error::{throw_coded_error, throw_coded_type_error, throw_native_type_error};
use crate::runtime::streams::{bytes_stream, Chunks, is_readable_stream, lock_stream, read_stream_bytes, stream_used, tee_of, text_stream};
use crate::runtime::text_decoder::input_bytes;
use crate::runtime::uint8_array_base64::create_uint8_array;
use crate::runtime::url_search_params::{pairs_of_instance, serialize};
use crate::wtf::text::wtf_string::String as WtfString;

const URL_ENCODED_TYPE: &[u8] = b"application/x-www-form-urlencoded;charset=UTF-8";
const TEXT_TYPE: &[u8] = b"text/plain;charset=utf-8";
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// O corpo de uma instância: os bytes, o tipo que `blob()` herda e se já foi lido.
#[derive(Clone)]
pub(crate) struct Body {
    pub(crate) bytes: Vec<u8>,
    pub(crate) blob_type: Vec<u8>,
    pub(crate) used: bool,
    /// Caminho de arquivo lido só na hora da leitura do corpo (`fetch('file://...')`); `bytes` fica vazio até lá.
    pub(crate) lazy_path: Option<String>,
    /// O `ReadableStream` de `body`, criado na primeira leitura da propriedade (o mesmo objeto nas seguintes).
    pub(crate) stream: Option<JSValue>,
    /// `stream` é um `ReadableStream` do usuário (a fonte dos bytes), não o stream criado sobre `bytes`.
    pub(crate) from_stream: bool,
    /// A identidade de `File` (nome e `lastModified`) da fonte: `blob()` devolve um `File` quando o corpo veio de um
    /// (`new Response(file)`, `fetch(URL.createObjectURL(file))`), medido no bun 1.4.2.
    pub(crate) file: Option<FileIdentity>,
    /// `blob_type` só vem da leitura (string e array dão `text/plain`, sem `Content-Type`): o armazenamento do corpo não
    /// tem tipo, e o console do bun escreve `Blob (N bytes)` sem o campo `type` (medido no bun 1.4.2).
    pub(crate) implied_type: bool,
    /// O corpo veio de um `Blob` (o `Content-Type` que `Bun.serve` manda sem tipo: `text/plain` para ele,
    /// `application/octet-stream` para bytes de `ArrayBuffer` ou view; medido no bun 1.4.2).
    pub(crate) from_blob: bool,
}
#[derive(Clone)]
pub(crate) struct FileIdentity {
    pub(crate) name: Option<Vec<u16>>,
    pub(crate) last_modified: Option<f64>,
}

impl FileIdentity {
    /// A identidade do `state`, se ele é um `File`.
    pub(crate) fn of(state: &BlobState) -> Option<FileIdentity> {
        state.is_file.then(|| FileIdentity { name: state.name.clone(), last_modified: state.last_modified })
    }
}

impl Body {
    /// Já lido por um método, ou com o stream travado/lido (`getReader`, `read`).
    pub(crate) fn is_used(&self) -> bool {
        self.used || self.stream.is_some_and(stream_used)
    }

    /// O `ReadableStream` de `body`: o guardado ou um novo sobre os bytes (vazio se o corpo já foi lido por um método).
    pub(crate) fn stream_value(&mut self, global_object: &JSGlobalObject) -> JSValue {
        if let Some(stream) = self.stream {
            return stream;
        }
        let loaded;
        let content: &[u8] = match (&self.lazy_path, self.used) {
            (_, true) => &[],
            (Some(path), false) => {
                loaded = crate::runtime::fetch::read_file_body(global_object, path).unwrap_or_default();
                &loaded
            }
            (None, false) => &self.bytes,
        };
        let stream = bytes_stream(global_object, content);
        // Medido no bun 1.4.2: o `body` lido pela primeira vez depois de uma leitura (`text()`, `formData()`...) já
        // nasce travado; o que foi lido antes da leitura segue destravado.
        if self.used {
            lock_stream(global_object, stream);
        }
        self.stream = Some(stream);
        stream
    }
}

/// O que a extração de um valor de corpo produz.
#[derive(Default)]
pub(crate) struct Extracted {
    pub(crate) bytes: Vec<u8>,
    /// O `Content-Type` que entra nos cabeçalhos (vazio: nenhum).
    pub(crate) header_type: Vec<u8>,
    /// O `type` do `Blob` que `blob()` devolve.
    pub(crate) blob_type: Vec<u8>,
    /// O `ReadableStream` do usuário que é a fonte dos bytes (`bytes` fica vazio).
    pub(crate) stream: Option<JSValue>,
    /// A identidade de `File` da fonte, se ela era um.
    pub(crate) file: Option<FileIdentity>,
    /// A fonte era um `Blob`.
    pub(crate) from_blob: bool,
}

impl Extracted {
    pub(crate) fn into_body(self) -> Body {
        let implied_type = self.header_type.is_empty() && !self.blob_type.is_empty();
        Body { bytes: self.bytes, blob_type: self.blob_type, used: false, lazy_path: None, from_stream: self.stream.is_some(), stream: self.stream, file: self.file, implied_type, from_blob: self.from_blob }
    }
}

/// Que leitura se pede ao corpo.
#[derive(Clone)]
pub(crate) enum Read {
    Text,
    ArrayBuffer,
    Bytes,
    Json,
    /// `json()` de um `ReadableStream`: o `JSON.parse` direto, sem a mensagem própria do corpo vazio.
    StreamJson,
    /// `blob()`, com o `type` do resultado.
    Blob(Vec<u8>, Option<FileIdentity>),
    /// `formData()`, com o `Content-Type` dos cabeçalhos (vazio: sem cabeçalho).
    FormData(Vec<u8>),
}

thread_local! {
    static BOUNDARY_COUNTER: Cell<u64> = const { Cell::new(0) };
}

fn splitmix(mut state: u64) -> u64 {
    state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    state = (state ^ (state >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    state = (state ^ (state >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    state ^ (state >> 31)
}

/// `----WebKitFormBoundary` e 32 dígitos hexadecimais, diferentes a cada chamada.
fn new_boundary() -> String {
    let counter = BOUNDARY_COUNTER.with(|cell| {
        cell.set(cell.get() + 1);
        cell.get()
    });
    let nanos = ul_common::time::realtime_nanos() as u64;
    let first = splitmix(nanos ^ counter);
    format!("----WebKitFormBoundary{:016x}{:016x}", first, splitmix(first ^ counter))
}

/// O nome de campo no `Content-Disposition`: `"`, CR e LF escapam em porcentagem.
fn escaped_field(units: &[u16]) -> Vec<u8> {
    String::from_utf16_lossy(units).replace('"', "%22").replace('\r', "%0D").replace('\n', "%0A").into_bytes()
}

fn multipart(entries: &[(Vec<u16>, FormValue)]) -> Extracted {
    let boundary = new_boundary();
    let mut bytes = Vec::new();
    for (name, value) in entries {
        bytes.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"").as_bytes());
        bytes.extend_from_slice(&escaped_field(name));
        bytes.push(b'"');
        match value {
            FormValue::Text(units) => {
                bytes.extend_from_slice(b"\r\n\r\n");
                bytes.extend_from_slice(String::from_utf16_lossy(units).as_bytes());
            }
            FormValue::Blob(state) => {
                bytes.extend_from_slice(b"; filename=\"");
                bytes.extend_from_slice(&state.name.as_deref().map_or_else(Vec::new, escaped_field));
                bytes.extend_from_slice(b"\"\r\nContent-Type: ");
                bytes.extend_from_slice(if state.content_type.is_empty() { &b"application/octet-stream"[..] } else { &state.content_type[..] });
                bytes.extend_from_slice(b"\r\n\r\n");
                bytes.extend_from_slice(&state.bytes);
            }
        }
        bytes.extend_from_slice(b"\r\n");
    }
    bytes.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let content_type = format!("multipart/form-data; boundary={boundary}").into_bytes();
    Extracted { bytes, header_type: content_type.clone(), blob_type: content_type, ..Extracted::default() }
}

/// Extrai o corpo de `value`; `None` para `undefined` e `null`.
pub(crate) fn extract(global_object: &JSGlobalObject, value: JSValue) -> Result<Option<Extracted>, Thrown> {
    if value.is_undefined_or_null() {
        return Ok(None);
    }
    if is_readable_stream(value) {
        if stream_used(value) {
            return Err(throw_native_type_error(global_object, "Body object should not be disturbed or locked"));
        }
        return Ok(Some(Extracted { stream: Some(value), ..Extracted::default() }));
    }
    if let Some(mut state) = state_of(value) {
        if let Some(backing) = crate::runtime::blob_file::backing_of(value) {
            state.bytes = crate::runtime::blob_file::read(&backing).unwrap_or_default();
        }
        let file = FileIdentity::of(&state);
        return Ok(Some(Extracted { bytes: state.bytes, header_type: state.content_type.clone(), blob_type: state.content_type, file, from_blob: true, ..Extracted::default() }));
    }
    if let Some(stream) = crate::runtime::streams::async_iterable_stream(global_object, value)? {
        return Ok(Some(Extracted { stream: Some(stream), ..Extracted::default() }));
    }
    if let Some(pairs) = pairs_of_instance(value) {
        return Ok(Some(Extracted { bytes: serialize(&pairs).into_bytes(), header_type: URL_ENCODED_TYPE.to_vec(), blob_type: URL_ENCODED_TYPE.to_vec(), ..Extracted::default() }));
    }
    if let Some(entries) = entries_of_instance(value) {
        return Ok(Some(multipart(&entries)));
    }
    if let Some(bytes) = input_bytes(value) {
        return Ok(Some(Extracted { bytes, ..Extracted::default() }));
    }
    // Um array de verdade são as partes de um `Blob` (medido no bun 1.4.2: `new Response([1, 2])` lê "12", sem
    // `Content-Type`; `Set`, iterável e objeto com `length` viram texto).
    if crate::runtime::js_array::JSArray::from_value(&value).is_some() {
        return Ok(Some(Extracted { bytes: crate::runtime::blob::parts_bytes(global_object, value)?, blob_type: TEXT_TYPE.to_vec(), ..Extracted::default() }));
    }
    Ok(Some(Extracted { bytes: string_bytes(global_object, value)?, blob_type: TEXT_TYPE.to_vec(), ..Extracted::default() }))
}

/// A leitura `read` de `bytes`, numa promessa (a rejeição do JSON malformado também).
pub(crate) fn read_body(global_object: &JSGlobalObject, read: Read, bytes: &[u8]) -> HostResult {
    let vm = global_object.vm();
    match read {
        Read::Text => {
            let text = WtfString::from_utf8_replacing_invalid_sequences(bytes);
            Ok(resolved_promise(global_object, JSValue::from_js_string(js_string(vm, &text))))
        }
        Read::ArrayBuffer => {
            let structure = global_object.array_buffer_realm.array_buffer_structure(ArrayBufferSharingMode::Default);
            let buffer = JSArrayBuffer::create(vm, &structure, ArrayBuffer::create_from_span(bytes)).as_value();
            Ok(resolved_promise(global_object, buffer))
        }
        Read::Bytes => {
            let array = create_uint8_array(global_object, bytes.len())?;
            array.with_vector_mut(|destination| destination[..bytes.len()].copy_from_slice(bytes));
            Ok(resolved_promise(global_object, array.as_value()))
        }
        Read::Json if bytes.is_empty() => {
            // Corpo vazio: o bun usa a mensagem do V8-style, não a do `JSON.parse` (medido no bun 1.4.2).
            let error = crate::runtime::error::create_syntax_error(global_object, &WtfString::from_utf8(b"Unexpected end of JSON input")).as_value();
            Ok(JSPromise::rejected_promise(global_object, error).as_value())
        }
        Read::Json | Read::StreamJson => match json_parse(global_object, &WtfString::from_utf8_replacing_invalid_sequences(bytes), None, false) {
            Ok(value) => Ok(resolved_promise(global_object, value)),
            Err(JsonError::Syntax(message)) => {
                let error = crate::runtime::error::create_syntax_error(global_object, &message).as_value();
                Ok(JSPromise::rejected_promise(global_object, error).as_value())
            }
            Err(error) => {
                throw_json_error(global_object, error);
                Err(Thrown::Pending)
            }
        },
        Read::Blob(content_type, file) => {
            let state = match file {
                Some(FileIdentity { name, last_modified }) => BlobState { bytes: bytes.to_vec(), content_type, name, is_file: true, last_modified },
                None => BlobState { bytes: bytes.to_vec(), content_type, ..BlobState::default() },
            };
            Ok(resolved_promise(global_object, make_blob(global_object, state)))
        }
        Read::FormData(content_type) => {
            if !has_form_encoding(&content_type) {
                return Ok(rejected_type_error(
                    global_object,
                    "Can't decode form data from body because of incorrect MIME type/boundary",
                    "ERR_FORMDATA_PARSE_ERROR",
                ));
            }
            // Medido: multipart com corpo vazio também rejeita (o parser do `Blob` devolve vazio).
            let parsed = if bytes.is_empty() && content_type.to_ascii_lowercase().starts_with(b"multipart/form-data") {
                Err("FormData encoding failed: missing final boundary".to_string())
            } else {
                parse_body(bytes, &content_type)
            };
            match parsed {
                Ok(entries) => Ok(resolved_promise(global_object, make_form_data(global_object, entries))),
                Err(message) => {
                    // O `Response`/`Request` rejeitam com `TypeError` `FormData parse error <motivo>` (ERR_FORMDATA_PARSE_ERROR).
                    let reason = message.strip_prefix("FormData encoding failed: ").unwrap_or(&message);
                    Ok(rejected_type_error(global_object, &format!("FormData parse error {reason}"), "ERR_FORMDATA_PARSE_ERROR"))
                }
            }
        }
    }
}

/// O erro pendente no `VM` (lançado por um `throw_*`), tirado de lá: o motivo de uma promessa a rejeitar.
pub(crate) fn pending_reason(global_object: &JSGlobalObject) -> JSValue {
    let vm = global_object.vm();
    let error = vm.exception().map_or_else(JSValue::undefined, |exception| exception.value());
    vm.clear_exception();
    error
}

/// Uma promessa rejeitada com um `TypeError` com `code`.
pub(crate) fn rejected_type_error(global_object: &JSGlobalObject, message: &str, code: &str) -> JSValue {
    let _ = throw_coded_type_error(global_object, message, code);
    JSPromise::rejected_promise(global_object, pending_reason(global_object)).as_value()
}

/// Uma promessa rejeitada com um `Error` simples com `code` (os erros de `errno` da leitura de um arquivo).
fn rejected_error(global_object: &JSGlobalObject, message: &str, code: &str) -> JSValue {
    let _ = throw_coded_error(global_object, message, code);
    JSPromise::rejected_promise(global_object, pending_reason(global_object)).as_value()
}

/// Qual leitura o método do `Response`/`Request` pede; o `type` do `blob()` e o `Content-Type` do `formData()` saem dos
/// cabeçalhos de quem chama (ver [`consume_kind`]).
#[derive(Clone, Copy)]
pub(crate) enum ReadKind {
    Text,
    Json,
    ArrayBuffer,
    Bytes,
    Blob,
    FormData,
}

/// A leitura `kind` do corpo de uma instância com os cabeçalhos `headers`: `blob()` herda o `Content-Type` dos
/// cabeçalhos (senão o do corpo) e `formData()` olha só o dos cabeçalhos.
pub(crate) fn consume_kind(global_object: &JSGlobalObject, kind: ReadKind, headers: JSValue, body: &mut Option<Body>) -> HostResult {
    let read = match kind {
        ReadKind::Text => Read::Text,
        ReadKind::Json => Read::Json,
        ReadKind::ArrayBuffer => Read::ArrayBuffer,
        ReadKind::Bytes => Read::Bytes,
        ReadKind::Blob => {
            // O `Content-Type` do cabeçalho vai ao `Blob` como está (medido: `Text/HTML;Charset=UTF-8` não muda).
            let header = header_value(headers, "content-type");
            // Medido: o `Blob` de um corpo stream tem `type` vazio, mesmo com `Content-Type` nos cabeçalhos.
            let from_stream = body.as_ref().is_some_and(|body| body.from_stream);
            let file = body.as_ref().and_then(|body| body.file.clone());
            Read::Blob(if from_stream { Vec::new() } else { header.or_else(|| body.as_ref().map(|body| body.blob_type.clone())).unwrap_or_default() }, file)
        }
        ReadKind::FormData => Read::FormData(header_value(headers, "content-type").unwrap_or_default()),
    };
    consume(global_object, body, read)
}

/// Lê o corpo (`None`: vazio, lê sem marcar uso). O segundo uso rejeita com `Body already used`.
pub(crate) fn consume(global_object: &JSGlobalObject, body: &mut Option<Body>, read: Read) -> HostResult {
    let Some(body) = body else {
        return read_body(global_object, read, &[]);
    };
    if body.is_used() {
        return Ok(rejected_type_error(global_object, "Body already used", "ERR_BODY_ALREADY_USED"));
    }
    body.used = true;
    if let (true, Some(stream)) = (body.from_stream, body.stream) {
        return Ok(consume_stream(global_object, stream, Chunks::Text, read));
    }
    let loaded;
    let content: &[u8] = match &body.lazy_path {
        Some(path) => match crate::runtime::fetch::read_file_body(global_object, path) {
            Ok(bytes) => {
                loaded = bytes;
                &loaded
            }
            Err((message, code)) => return Ok(rejected_error(global_object, &message, code)),
        },
        None => &body.bytes,
    };
    let bytes = match read {
        Read::Text => content.strip_prefix(BOM).unwrap_or(content),
        _ => content,
    };
    read_body(global_object, read, bytes)
}

/// A leitura `read` de um corpo que é `ReadableStream` do usuário: uma promessa que se resolve quando o stream fecha
/// (com a leitura dos bytes juntados) e rejeita com o motivo do erro do stream ou do pedaço inválido.
pub(crate) fn consume_stream(global_object: &JSGlobalObject, stream: JSValue, chunks: Chunks, read: Read) -> JSValue {
    let promise = JSPromise::create(global_object.vm(), &global_object.promise_structure());
    let settled = promise.as_value();
    let started = read_stream_bytes(global_object, stream, chunks, &[settled], move |global_object, result| {
        let outcome = result.and_then(|bytes| {
            let bytes = if matches!(read, Read::Text) { bytes.strip_prefix(BOM).unwrap_or(&bytes).to_vec() } else { bytes };
            read_body(global_object, read.clone(), &bytes).map_err(|_| pending_reason(global_object))
        });
        match (JSPromise::from_value(&settled), outcome) {
            (Some(promise), Ok(value)) => promise.resolve(global_object, value),
            (Some(promise), Err(reason)) => promise.reject(global_object, reason),
            (None, _) => {}
        }
    });
    if started { settled } else { rejected_type_error(global_object, "Body already used", "ERR_BODY_ALREADY_USED") }
}

/// A propriedade `body`: `null` sem corpo, senão o `ReadableStream` (guardado em `body`, para voltar o mesmo objeto).
pub(crate) fn stream_property(global_object: &JSGlobalObject, body: &mut Option<Body>) -> JSValue {
    body.as_mut().map_or_else(JSValue::null, |body| body.stream_value(global_object))
}

/// `textStream()`: um `ReadableStream` de texto sobre o corpo. Sem corpo, um stream fechado sem marcar nada; com corpo,
/// marca o uso (o segundo uso, ou um corpo já travado/lido, lança `Body is disturbed or locked` na hora, não rejeita).
pub(crate) fn text_stream_of(global_object: &JSGlobalObject, body: &mut Option<Body>) -> HostResult {
    let Some(body) = body else {
        return Ok(text_stream(global_object, None, &[]));
    };
    if body.is_used() {
        return Err(throw_coded_type_error(global_object, "Body is disturbed or locked", "ERR_BODY_ALREADY_USED"));
    }
    body.used = true;
    if let (true, Some(stream)) = (body.from_stream, body.stream) {
        let result = text_stream(global_object, Some(stream), &[]);
        lock_stream(global_object, stream);
        return Ok(result);
    }
    let content = match &body.lazy_path {
        Some(path) => match crate::runtime::fetch::read_file_body(global_object, path) {
            Ok(bytes) => bytes,
            Err((message, code)) => return Err(throw_coded_error(global_object, &message, code)),
        },
        None => body.bytes.clone(),
    };
    let result = text_stream(global_object, None, &content);
    let stream = body.stream_value(global_object);
    lock_stream(global_object, stream);
    Ok(result)
}

/// `clone` de um corpo: uma cópia independente; lançar se já foi lido. Um corpo que é um stream do usuário é
/// dividido por `tee`: o original passa a ler o primeiro ramo e o clone o segundo.
pub(crate) fn clone_body(global_object: &JSGlobalObject, body: &mut Option<Body>) -> Result<Option<Body>, Thrown> {
    match body {
        Some(body) if body.is_used() => Err(throw_coded_type_error(global_object, "Body is disturbed or locked", "ERR_BODY_ALREADY_USED")),
        Some(body) => {
            let mut copy = Body { stream: None, ..body.clone() };
            if let (true, Some(stream)) = (body.from_stream, body.stream) {
                if let Some(branches) = tee_of(global_object, stream) {
                    let [first, second] = branches?;
                    body.stream = Some(first);
                    copy.stream = Some(second);
                }
            }
            Ok(Some(copy))
        }
        None => Ok(None),
    }
}
