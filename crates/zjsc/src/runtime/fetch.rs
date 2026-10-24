//! `fetch` do global, só a parte offline. O JavaScriptCore não o define: quem o instala é o bun, como propriedade de
//! dados `writable`, `enumerable` e `configurable`, função nativa de `length` 1, sem `prototype`, que não constrói, com a
//! propriedade própria enumerável `preconnect` (`length` 1). Medido no bun 1.4.2 (`scripts/gen-fetch-offline-golden.js`,
//! `tests/golden/fetch_offline_bun.tsv`):
//!
//! - NUNCA lança de forma síncrona: todo erro é uma promessa rejeitada, e toda promessa já nasce liquidada (a ordem das
//!   microtarefas é a de `Promise.resolve(...)`: o `then` do `fetch` roda antes do `m1` registrado depois dele);
//! - o `init` (método, corpo, cabeçalhos, `redirect`, `signal`...) é ignorado por inteiro, sem sequer ser lido, salvo o
//!   `signal` já abortado nos caminhos que falhariam de qualquer jeito (`blob:` não registrado, rede), que rejeita com o
//!   `reason` do sinal tal qual (`'why'`, um `RangeError`, o `AbortError` padrão), não com um `AbortError` novo;
//! - argumento: sem argumento `ERR_MISSING_ARGS`; string vale; objeto vale pela propriedade `url` (uma `Request`, ou
//!   `{ url }`; o getter alheio lança o `TypeError` do getter), senão pelo `ToString` (matriz, `String`, `URL`, objeto com
//!   `toString`), exceto função, `URLSearchParams` e o que dá `[object ...]`, que contam como vazios; símbolo lança o
//!   `ToString`; o resto (número, booleano, `bigint`, `undefined`, `null`) conta como vazio; vazio é
//!   `fetch() URL must not be a blank string.` (`ERR_INVALID_URL`); o que não parseia é `fetch() URL is invalid`;
//! - `data:`: nada é normalizado (o `url` da resposta é o texto cru, `#` e espaços ficam); vírgula obrigatória (senão
//!   `Error: failed to fetch the data URL`); só `;base64` minúsculo colado à vírgula decodifica (estrito: espaço, `=`
//!   demais ou fora do alfabeto é o mesmo erro); o resto é percent-decode de bytes (`%zz` e `%` solto ficam). `statusText`
//!   "OK"; o `Content-Type` sai do tipo MIME (sem tipo, sem cabeçalho);
//! - `blob:`: o conteúdo de `URL.createObjectURL` (registro de `url.rs`), `statusText` vazio, `Content-Type` do `type` do
//!   Blob; revogado antes da chamada, `TypeError` `ERR_INVALID_ARG_VALUE` `Failed to resolve <url>`; `blob:` seguido de
//!   URL com esquema, `protocol must be http:, https: or s3:`;
//! - `file:`: o esquema vale em qualquer caixa; o caminho é o `pathname` do `URLParser` decodificado (`%2F` vira `/`), cortado
//!   no primeiro NUL na hora de abrir, e o `url` da resposta é `file://` mais esse caminho recodificado; a resposta sai
//!   sempre 200 e o ARQUIVO SÓ É LIDO NA HORA DA LEITURA DO CORPO, pelo `ModuleFs` do programa (o mesmo do `require`), então
//!   os erros (`ENOENT`, `EISDIR`, `ENOTDIR`, `EACCES`) rejeitam o `text()` e não o `fetch`;
//! - o `Content-Type` implícito das três respostas some dos cabeçalhos na primeira leitura do corpo (`response.rs`);
//! - `ws:`, `wss:`, `ftp:`, `chrome:`: `protocol must be http:, https: or s3:` (`ERR_INVALID_ARG_VALUE`); `s3:`:
//!   `ERR_S3_MISSING_CREDENTIALS`.
//!
//! LACUNAS declaradas: `http:` e `https:` (e os esquemas desconhecidos, que o bun trata como `http:`) não têm rede aqui e
//! rejeitam como o bun numa máquina sem rede (`network_failure`, medido); `Response.body` (`ReadableStream`) segue como em `response.rs`.

use crate::host_function;
use crate::runtime::module_fs::EntryKind;
use crate::runtime::abort_signal::{is_aborted, throw_reason};
use crate::runtime::blob::resolved_promise;
use crate::runtime::body::{pending_reason, Body, FileIdentity};
use crate::runtime::host_call::{throw_thrown, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, put_direct_native_function_without_transition, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_promise::JSPromise;
use crate::runtime::js_typeof::js_type_string_for_value;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_class_support::install_global_with_attributes;
use crate::runtime::node_error::{throw_coded_error, throw_coded_type_error, throw_network_error, throw_plain_error, NetworkProperty};
use crate::runtime::response::{make_fetched_response, property_of};
use crate::runtime::url::object_url_state;
use crate::runtime::url_search_params::pairs_of_instance;
use crate::runtime::web_iterable::units_of;
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::url::URL;

const PROTOCOL_MESSAGE: &str = "protocol must be http:, https: or s3:";
const OBJECT_PREFIX: &str = "[object ";

/// O erro de leitura de um arquivo: a mensagem e o `code`.
type ReadFailure = (String, &'static str);

fn invalid_url(global_object: &JSGlobalObject) -> Thrown {
    throw_coded_type_error(global_object, "fetch() URL is invalid", "ERR_INVALID_URL")
}

fn protocol_error(global_object: &JSGlobalObject) -> Thrown {
    throw_coded_type_error(global_object, PROTOCOL_MESSAGE, "ERR_INVALID_ARG_VALUE")
}

/// Se o `init` (segundo argumento) traz um `signal` já abortado: o `AbortError` do bun, só nos caminhos que falham.
fn abort_if_signaled(global_object: &JSGlobalObject, call: &HostCall) -> Result<(), Thrown> {
    let init = call.argument(1);
    if !init.is_object() {
        return Ok(());
    }
    let signal = property_of(global_object, init, b"signal")?;
    if signal.is_object() && is_aborted(signal) {
        return Err(throw_reason(global_object, signal));
    }
    Ok(())
}

/// O texto da URL pedida, em unidades UTF-16 (vazio: o bun reclama de URL em branco).
fn url_units(global_object: &JSGlobalObject, first: JSValue) -> Result<Vec<u16>, Thrown> {
    if first.is_string() {
        return units_of(global_object, first);
    }
    if first.is_object() {
        let url = property_of(global_object, first, b"url")?;
        if !url.is_undefined() {
            return units_of(global_object, url);
        }
        if first.is_callable() || pairs_of_instance(first).is_some() {
            return Ok(Vec::new());
        }
        let units = units_of(global_object, first)?;
        let prefix: Vec<u16> = OBJECT_PREFIX.encode_utf16().collect();
        return Ok(if units.starts_with(&prefix) { Vec::new() } else { units });
    }
    if js_type_string_for_value(first) == "symbol" {
        return units_of(global_object, first);
    }
    Ok(Vec::new())
}

/// O esquema (em minúsculas) do texto, se começa por um esquema válido seguido de `:`.
fn scheme_of(text: &str) -> Option<String> {
    let end = text.find(':')?;
    let scheme = &text[..end];
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|first| first.is_ascii_alphabetic()) && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid.then(|| scheme.to_ascii_lowercase())
}

/// Os bytes de `input` com `%XX` decodificado (o que não é `%` e dois hexadecimais fica como está).
fn percent_decode(input: &[u8]) -> Vec<u8> {
    let hex = |byte: u8| (byte as char).to_digit(16).map(|digit| digit as u8);
    let mut out = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' && index + 2 < input.len() {
            if let (Some(high), Some(low)) = (hex(input[index + 1]), hex(input[index + 2])) {
                out.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        out.push(input[index]);
        index += 1;
    }
    out
}

/// O caminho como o `url` da resposta o escreve: tudo fora de `unreservados`, `/` e pontuação de caminho vira `%XX`.
fn encode_path(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &byte in bytes {
        if byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// O base64 estrito do `data:`: até dois `=` no fim (e então o comprimento fecha em quatro), nada fora do alfabeto, e
/// resto 1 módulo 4 é erro. Sem remover espaço.
fn decode_base64(input: &[u8]) -> Option<Vec<u8>> {
    let mut end = input.len();
    while end > 0 && input[end - 1] == b'=' {
        end -= 1;
    }
    let padding = input.len() - end;
    let body = &input[..end];
    if padding > 2 || (padding > 0 && (body.len() + padding) % 4 != 0) || body.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    let (mut accumulator, mut bits) = (0u32, 0u32);
    for &byte in body {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        accumulator = (accumulator << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((accumulator >> bits) as u8);
            accumulator &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// O `Content-Type` de um tipo MIME: o bun acrescenta `;charset=utf-8` aos tipos de texto sem `charset`.
fn with_charset(mime: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(mime).into_owned();
    let textual = text.starts_with("text/") || ["json", "javascript", "xml", "x-www-form-urlencoded"].iter().any(|suffix| text.ends_with(suffix));
    let mut out = text.clone().into_bytes();
    if textual && !text.contains("charset") {
        out.extend_from_slice(b";charset=utf-8");
    }
    out
}

/// O tipo MIME de um arquivo pela extensão (a tabela do bun, só as extensões comuns).
fn file_mime(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or("");
    let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match extension.as_str() {
        "txt" => "text/plain",
        "json" => "application/json",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "md" => "text/markdown",
        "xml" => "text/xml",
        "csv" => "text/csv",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "pdf" => "application/pdf",
        _ => return None,
    })
}

/// Lê o arquivo de um corpo de `fetch('file://...')` na hora da leitura, pelo `ModuleFs` do programa; a falha tem a
/// mensagem e o `code` do bun.
pub(crate) fn read_file_body(global_object: &JSGlobalObject, path: &str) -> Result<Vec<u8>, ReadFailure> {
    let no_entry = || (format!("ENOENT: no such file or directory, open '{path}'"), "ENOENT");
    let Some(fs) = global_object.module_fs() else {
        return Err(no_entry());
    };
    let trimmed = path.trim_end_matches('/');
    if trimmed.len() < path.len() && !trimmed.is_empty() && fs.stat(trimmed) == Some(EntryKind::File) {
        return Err((format!("ENOTDIR: not a directory, open '{path}'"), "ENOTDIR"));
    }
    if let Some(bytes) = fs.read_bytes(path) {
        return Ok(bytes);
    }
    match fs.stat(path) {
        Some(EntryKind::Directory) => Err(("Directories cannot be read like files".to_owned(), "EISDIR")),
        Some(EntryKind::File) => Err((format!("EACCES: permission denied, open '{path}'"), "EACCES")),
        None => Err(no_entry()),
    }
}

/// `data:`: o corpo decodificado e o `Content-Type`, ou `None` se a URL não tem vírgula ou o base64 é inválido.
pub(crate) fn parse_data_url(text: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    let rest = text.strip_prefix("data:")?;
    let (header, data) = rest.split_once(',')?;
    let decoded = percent_decode(data.as_bytes());
    let (media, bytes) = match header.strip_suffix(";base64") {
        Some(media) => (media, decode_base64(&decoded)?),
        None => (header, decoded),
    };
    let mime = media.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let content_type = if mime.contains('/') { with_charset(mime.as_bytes()) } else { Vec::new() };
    Some((bytes, content_type))
}

fn fetch_data(global_object: &JSGlobalObject, text: &str, units: Vec<u16>) -> HostResult {
    let Some((bytes, content_type)) = parse_data_url(text) else {
        return Err(throw_plain_error(global_object, "failed to fetch the data URL"));
    };
    let body = Body { bytes, blob_type: content_type.clone(), used: false, lazy_path: None, stream: None, from_stream: false, file: None, implied_type: false, from_blob: false };
    make_fetched_response(global_object, "OK", units, &content_type, body)
}

fn fetch_blob(global_object: &JSGlobalObject, call: &HostCall, text: &str, units: Vec<u16>) -> HostResult {
    let Some(state) = object_url_state(text) else {
        if text["blob:".len()..].contains("://") {
            return Err(protocol_error(global_object));
        }
        abort_if_signaled(global_object, call)?;
        return Err(throw_coded_type_error(global_object, &format!("Failed to resolve {text}"), "ERR_INVALID_ARG_VALUE"));
    };
    let content_type = if state.content_type.is_empty() { Vec::new() } else { with_charset(&state.content_type) };
    let file = FileIdentity::of(&state);
    let body = Body { bytes: state.bytes, blob_type: content_type.clone(), used: false, lazy_path: None, stream: None, from_stream: false, file, implied_type: false, from_blob: false };
make_fetched_response(global_object, "", units, &content_type, body)
}

fn fetch_file(global_object: &JSGlobalObject, units: &[u16]) -> HostResult {
    let url = URL::from_string(&WtfString::from_utf16(units));
    if !url.is_valid() {
        return Err(invalid_url(global_object));
    }
    let pathname = url.path().utf8(ConversionMode::LenientConversion);
    // Quirk do bun 1.4.2, medido em qualquer caixa do esquema (`file:///x`, `FILE:///a/`, `file:///x/.`, `file:///..`): o
    // `pathname` codificado que, sem as barras finais, tem no máximo dois bytes (`/` mais um) abre a raiz.
    let decoded = if pathname.iter().rposition(|&byte| byte != b'/').map_or(0, |last| last + 1) <= 2 { b"/".to_vec() } else { percent_decode(&pathname) };
    let opened = decoded.split(|&byte| byte == 0).next().unwrap_or(&[]);
    let path = String::from_utf8_lossy(opened).into_owned();
    let content_type = file_mime(&path).map_or_else(Vec::new, |mime| with_charset(mime.as_bytes()));
    let response_url: Vec<u16> = format!("file://{}", encode_path(&decoded)).encode_utf16().collect();
    let body = Body { bytes: Vec::new(), blob_type: content_type.clone(), used: false, lazy_path: Some(path), stream: None, from_stream: false, file: None, implied_type: false, from_blob: false };
    make_fetched_response(global_object, "", response_url, &content_type, body)
}

/// Esquemas de rede (`http:`, `https:` e os que o bun trata como eles): LACUNA, sem rede aqui.
fn fetch_network(global_object: &JSGlobalObject, call: &HostCall, units: &[u16]) -> HostResult {
    let url = URL::from_string(&WtfString::from_utf16(units));
    if !url.is_valid() {
        return Err(invalid_url(global_object));
    }
    let protocol = String::from_utf16_lossy(&(0..url.protocol().length()).map(|index| url.protocol().code_unit_at(index)).collect::<Vec<u16>>()).to_ascii_lowercase();
    match protocol.trim_end_matches(':') {
        "ws" | "wss" | "ftp" | "chrome" => return Err(protocol_error(global_object)),
        "s3" => {
            return Err(throw_coded_error(
                global_object,
                "Missing S3 credentials. 'accessKeyId', 'secretAccessKey', 'bucket', and 'endpoint' are required",
                "ERR_S3_MISSING_CREDENTIALS",
            ))
        }
        _ => {}
    }
    abort_if_signaled(global_object, call)?;
    let host = String::from_utf8_lossy(&url.host().utf8(ConversionMode::LenientConversion)).into_owned();
    if protocol.trim_end_matches(':') == "http" && crate::runtime::bun_http::is_loopback_host(&host) {
        return crate::runtime::bun_http::call_module(global_object, "fetch", call.arguments());
    }
    Err(network_failure(global_object, &url))
}

/// A conexão a `path` foi recusada (medido no bun 1.4.2: `localhost` e IP de loopback sem ouvinte).
pub(crate) fn connection_refused(global_object: &JSGlobalObject, path: &str) -> Thrown {
    let properties = [("code", NetworkProperty::Text("ConnectionRefused")), ("path", NetworkProperty::Text(path)), ("errno", NetworkProperty::Number(0))];
    throw_network_error(global_object, "Unable to connect. Is the computer able to access the url?", &properties)
}

/// A falha de rede de uma máquina SEM rede, medida no bun 1.4.2 com `unshare -rn` (`tests/golden/fetch_network_bun.tsv`):
/// `localhost` resolve pelo `hosts` e a conexão é recusada (`ConnectionRefused`); IP literal não abre o socket
/// (`FailedToOpenSocket`); qualquer outro nome não resolve (`getaddrinfo ETIMEOUT`, `errno` 12). `path` é a URL
/// normalizada (com credenciais e fragmento) e `hostname` o host normalizado (minúsculo, punycode).
fn network_failure(global_object: &JSGlobalObject, url: &URL) -> Thrown {
    let host = String::from_utf8_lossy(&url.host().utf8(ConversionMode::LenientConversion)).into_owned();
    let path = String::from_utf8_lossy(&url.string().utf8(ConversionMode::LenientConversion)).into_owned();
    let is_literal_ip = host.starts_with('[') || (!host.is_empty() && host.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.'));
    if host == "localhost" {
        connection_refused(global_object, &path)
    } else if is_literal_ip {
        let properties = [("code", NetworkProperty::Text("FailedToOpenSocket")), ("path", NetworkProperty::Text(&path)), ("errno", NetworkProperty::Number(0))];
        throw_network_error(global_object, "Was there a typo in the url or port?", &properties)
    } else {
        let properties = [
            ("code", NetworkProperty::Text("ETIMEOUT")),
            ("path", NetworkProperty::Text(&path)),
            ("syscall", NetworkProperty::Text("getaddrinfo")),
            ("hostname", NetworkProperty::Text(&host)),
            ("errno", NetworkProperty::Number(12)),
        ];
        throw_network_error(global_object, &format!("getaddrinfo ETIMEOUT {host}"), &properties)
    }
}

/// O `Response` do pedido, ou o erro que rejeita a promessa.
fn start(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() == 0 {
        return Err(throw_coded_type_error(global_object, "fetch() expects a string but received no arguments.", "ERR_MISSING_ARGS"));
    }
    let units = url_units(global_object, call.argument(0))?;
    if units.is_empty() {
        return Err(throw_coded_type_error(global_object, "fetch() URL must not be a blank string.", "ERR_INVALID_URL"));
    }
    let text = String::from_utf16_lossy(&units);
    if text.starts_with("data:") {
        return fetch_data(global_object, &text, units);
    }
    if text.starts_with("blob:") {
        return fetch_blob(global_object, call, &text, units);
    }
    match scheme_of(&text).as_deref() {
        Some("file") => fetch_file(global_object, &units),
        _ => fetch_network(global_object, call, &units),
    }
}

fn fetch_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    match start(global_object, call) {
        Ok(response) => Ok(resolved_promise(global_object, response)),
        Err(thrown) => {
            throw_thrown(global_object, thrown);
            Ok(JSPromise::rejected_promise(global_object, pending_reason(global_object)).as_value())
        }
    }
}

/// `fetch.preconnect(url)`: sem rede não há o que pré-conectar.
fn preconnect_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(JSValue::undefined())
}

host_function!(fetch_fn, fetch_body);
host_function!(preconnect_fn, preconnect_body);

/// Instala `fetch` no global (propriedade de dados comum, como as demais funções web do bun).
pub fn install_fetch(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let function = JSFunction::create_native_with_own_structure(
        vm,
        global_object,
        1,
        &WtfString::from_latin1(b"fetch"),
        fetch_fn,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    put_direct_native_function_without_transition(
        vm,
        global_object,
        &function,
        &Identifier::from_span(vm, b"preconnect"),
        1,
        preconnect_fn,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        0,
    );
    install_global_with_attributes(global_object, "fetch", function.as_value(), 0);
}
