//! Porte de `src/vs/platform/url/common/urlGlob.ts`: o casamento de uma URL com um glob (`*` e `*.`
//! de subdomínio), normalizando como o navegador faria antes de comparar.
//!
//! Divergências:
//! - O `URL` do JavaScript é a crate `url` (ver [`super::whatwg`]); onde o original captura a exceção do
//!   `new URL(...)` (`normalizeAuthorityForMatching`), aqui o `Option` faz o mesmo papel.
//! - `testUrlMatchesGlob(uri: string | URI, ...)` recebe [`UrlInput`].
//! - O casamento de partes (`doUrlPartMatch`) trabalha em unidades UTF-16, como o original; o `Set` do
//!   JavaScript, que enxerga elementos acrescentados durante a iteração, vira uma fila por deslocamento.
//! - As regex do original (todas simples) estão escritas à mão sobre unidades UTF-16; `\s` é
//!   [`je_base::js_string::is_js_whitespace`].

use std::collections::{HashMap, HashSet, VecDeque};

use ::url::Url;
use je_base::JsString;
use je_base::js_string::is_js_whitespace;
use je_base::network::{UriOrString, matches_some_scheme, schemas};
use je_base::uri::{Field, URI, UriChange};

use super::whatwg::{host_after_label, new_url, pathname, set_pathname};

const SLASH: u16 = b'/' as u16;
const BACKSLASH: u16 = b'\\' as u16;
const TAB: u16 = 0x09;
const LF: u16 = 0x0A;
const CR: u16 = 0x0D;
const STAR: u16 = b'*' as u16;
const DOT: u16 = b'.' as u16;
const COLON: u16 = b':' as u16;
const PERCENT: u16 = b'%' as u16;

/// `string | URI`, o tipo do primeiro argumento de `testUrlMatchesGlob` e de `normalizeURL`.
#[derive(Clone, Debug)]
pub enum UrlInput {
    Str(JsString),
    Uri(URI),
}

impl From<&str> for UrlInput {
    fn from(value: &str) -> Self {
        UrlInput::Str(JsString::from(value))
    }
}
impl From<String> for UrlInput {
    fn from(value: String) -> Self {
        UrlInput::Str(JsString::from(value))
    }
}
impl From<&JsString> for UrlInput {
    fn from(value: &JsString) -> Self {
        UrlInput::Str(value.clone())
    }
}
impl From<JsString> for UrlInput {
    fn from(value: JsString) -> Self {
        UrlInput::Str(value)
    }
}
impl From<&URI> for UrlInput {
    fn from(value: &URI) -> Self {
        UrlInput::Uri(value.clone())
    }
}
impl From<URI> for UrlInput {
    fn from(value: URI) -> Self {
        UrlInput::Uri(value)
    }
}

impl UrlInput {
    fn into_uri(self) -> URI {
        match self {
            UrlInput::Str(text) => URI::parse(text, false),
            UrlInput::Uri(uri) => uri,
        }
    }
}

pub(crate) fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

pub(crate) fn is_http_or_https(uri: &URI) -> bool {
    matches_some_scheme(UriOrString::Uri(uri), &[schemas::HTTP, schemas::HTTPS])
}

fn field(units: Vec<u16>) -> Field {
    Field::Set(JsString::from_units(units))
}

/// `toUpperCase` de um dígito hexadecimal.
fn upper_hex(unit: u16) -> u16 {
    if (0x61..=0x66).contains(&unit) { unit - 0x20 } else { unit }
}

fn is_hex(unit: u16) -> bool {
    char::from_u32(u32::from(unit)).is_some_and(|c| c.is_ascii_hexdigit())
}

/// `/^[^./:]*:\/\//.test(text)`: o texto começa com um esquema seguido de `://`.
pub(crate) fn has_scheme(text: &[u16]) -> bool {
    let end = text.iter().position(|&u| u == DOT || u == SLASH || u == COLON);
    matches!(end, Some(i) if text[i] == COLON && text.get(i + 1) == Some(&SLASH) && text.get(i + 2) == Some(&SLASH))
}

/// `value.replace(/\/+$/, '')`.
fn trim_trailing_slashes(value: &[u16]) -> Vec<u16> {
    let end = value.iter().rposition(|&u| u != SLASH).map_or(0, |i| i + 1);
    value[..end].to_vec()
}

fn normalize_url_authority_and_path(url: &URI) -> URI {
    if !is_http_or_https(url) {
        return url.clone();
    }
    if !url.authority().units().iter().any(|&u| matches!(u, BACKSLASH | SLASH | TAB | CR | LF)) {
        return url.clone();
    }
    let serialized = url.with(&UriChange { query: Field::Clear, fragment: Field::Clear, ..UriChange::default() }).to_string(true);
    let serialized: Vec<u16> = serialized.units().iter().copied().filter(|&u| !matches!(u, TAB | CR | LF)).collect();
    let rest = serialized.get(url.scheme().len() + 3..).unwrap_or(&[]);
    let rest: Vec<u16> = rest.iter().map(|&u| if u == BACKSLASH { SLASH } else { u }).collect();
    let skip = rest.iter().take_while(|&&u| u == SLASH).count();
    let authority_and_path = &rest[skip..];
    match authority_and_path.iter().position(|&u| u == SLASH) {
        None => url.with(&UriChange { authority: field(authority_and_path.to_vec()), path: field(Vec::new()), ..UriChange::default() }),
        Some(separator) => url.with(&UriChange {
            authority: field(authority_and_path[..separator].to_vec()),
            path: field(authority_and_path[separator..].to_vec()),
            ..UriChange::default()
        }),
    }
}

/// `normalizeURLPathSeparators`: normaliza a autoridade e o caminho HTTP(S) efetivos (separadores e
/// segmentos de ponto) sem mexer no conteúdo de consulta e fragmento.
pub fn normalize_url_path_separators(url: &URI) -> URI {
    if url.authority().is_empty() || !is_http_or_https(url) {
        return url.clone();
    }
    let normalized = normalize_url_authority_and_path(url);
    if normalized.authority().is_empty() {
        return normalized;
    }
    // A autoridade fixa preserva a sintaxe de glob, como `*` no host e na porta.
    let fixed = normalized.with(&UriChange { authority: Field::from("url.invalid"), ..UriChange::default() });
    let path = pathname(&new_url(&fixed.to_string(true)));
    normalized.with(&UriChange { path: Field::Set(path), ..UriChange::default() })
}

/// `normalizeURL` (privada do `urlGlob.ts`): sem barras finais, consulta nem fragmento; com `resolve_path`,
/// resolve o caminho HTTP(S) como o navegador.
fn normalize_url(url: UrlInput, resolve_path: bool) -> URI {
    let uri = normalize_url_authority_and_path(&url.into_uri());
    let mut path = uri.path().clone();
    if resolve_path && is_http_or_https(&uri) {
        // Pré-processamento do navegador sem reanalisar a autoridade nem decodificar escapes de novo.
        let well_formed = path.to_string_lossy();
        let cleaned: String = well_formed.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).map(|c| if c == '\\' { '/' } else { c }).collect();
        let mut path_url = Url::parse("http://localhost").expect("URL fixa válida");
        set_pathname(&mut path_url, &encode_uri(&cleaned));
        path = URI::parse(JsString::from(path_url.as_str()), false).path().clone();
    }
    uri.with(&UriChange {
        // Remove as barras finais.
        path: field(trim_trailing_slashes(path.units())),
        // Remove consulta e fragmento.
        query: Field::Clear,
        fragment: Field::Clear,
        ..UriChange::default()
    })
}

/// `encodeURI` sobre texto bem formado: mantém o conjunto reservado e `#`, codifica o resto em UTF-8.
pub(crate) fn encode_uri(text: &str) -> String {
    const KEEP: &str = ";,/?:@&=+$-_.!~*'()#";
    let mut out = String::with_capacity(text.len());
    let mut buffer = [0u8; 4];
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || KEEP.contains(c) {
            out.push(c);
        } else {
            for byte in c.encode_utf8(&mut buffer).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

fn encode_url_path_for_matching(url: &URI) -> Vec<u16> {
    if !is_http_or_https(url) {
        return url.path().units().to_vec();
    }
    // Codifica os caracteres decodificados do caminho sem decodificar nem codificar duas vezes os escapes.
    let encoded = units(&encode_uri(&url.path().to_string_lossy()));
    let mut collapsed = Vec::with_capacity(encoded.len());
    let mut i = 0;
    while i < encoded.len() {
        if encoded[i..].starts_with(&units("%25")) {
            collapsed.push(PERCENT);
            i += 3;
        } else {
            collapsed.push(encoded[i]);
            i += 1;
        }
    }
    let mut escaped = Vec::with_capacity(collapsed.len());
    for unit in collapsed {
        match unit {
            0x3F => escaped.extend(units("%3F")),
            0x23 => escaped.extend(units("%23")),
            other => escaped.push(other),
        }
    }
    decode_unreserved_url_path_characters(&escaped)
}

/// `decodeUnreservedURLPathCharacters`: decodifica uma vez os caracteres não reservados, preservando
/// escapes reservados (em maiúsculas) e sinais de porcentagem literais.
pub fn decode_unreserved_url_path_characters(path: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(path.len());
    let mut i = 0;
    while i < path.len() {
        if path[i] == PERCENT && i + 2 < path.len() && is_hex(path[i + 1]) && is_hex(path[i + 2]) {
            let hi = char::from_u32(u32::from(path[i + 1])).and_then(|c| c.to_digit(16)).unwrap_or(0);
            let lo = char::from_u32(u32::from(path[i + 2])).and_then(|c| c.to_digit(16)).unwrap_or(0);
            let code = u16::try_from(hi * 16 + lo).unwrap_or(0);
            let character = char::from_u32(u32::from(code)).unwrap_or('\0');
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '~' | '-') {
                out.push(code);
            } else {
                out.extend([PERCENT, upper_hex(path[i + 1]), upper_hex(path[i + 2])]);
            }
            i += 3;
        } else {
            out.push(path[i]);
            i += 1;
        }
    }
    out
}

/// `testUrlMatchesGlob`: confere uma URL com um glob com curingas (`*`) e casamento de subdomínio (`*.`).
/// Os caminhos HTTP(S) precisam casar antes e depois da resolução de segmentos de ponto do navegador.
pub fn test_url_matches_glob(uri: impl Into<UrlInput>, glob_url: &str) -> bool {
    test_url_matches_glob_js(uri.into(), &JsString::from(glob_url))
}

/// [`test_url_matches_glob`] com o glob já em UTF-16.
pub fn test_url_matches_glob_js(uri: UrlInput, glob_url: &JsString) -> bool {
    let normalized_url = normalize_url(uri, false);
    let glob_has_scheme = has_scheme(glob_url.units());

    // se o glob não tem esquema, assume-se http ou https; então, se a URL não é http nem https, é falso
    let normalized_glob_url = if !glob_has_scheme {
        if normalized_url.scheme() != "http" && normalized_url.scheme() != "https" {
            return false;
        }
        let mut full = normalized_url.scheme().clone();
        full.push_units(&units("://"));
        full.push_units(glob_url.units());
        normalize_url(UrlInput::Str(full), false)
    } else {
        normalize_url(UrlInput::Str(glob_url.clone()), false)
    };

    if !do_url_part_match(normalized_url.scheme().units(), normalized_glob_url.scheme().units(), false)
        // A autoridade é a única parte que faz a lógica de porta.
        || !do_url_authority_match(normalized_url.authority().units(), normalized_glob_url.authority().units(), is_http_or_https(&normalized_url))
    {
        return false;
    }

    if *normalized_glob_url.path() == "/" {
        return true;
    }

    let path = encode_url_path_for_matching(&normalized_url);
    let glob_path = encode_url_path_for_matching(&normalized_glob_url);
    if !do_url_part_match(&path, &glob_path, false) {
        return false;
    }

    let resolved_path = encode_url_path_for_matching(&normalize_url(UrlInput::Uri(normalized_url.clone()), true));
    resolved_path == path || do_url_part_match(&resolved_path, &glob_path, false)
}

fn do_url_authority_match(authority: &[u16], glob_authority: &[u16], decode_percent_encoding: bool) -> bool {
    if do_url_part_match(authority, glob_authority, true) {
        return true;
    }
    let normalized_authority = normalize_authority_for_matching(authority, decode_percent_encoding);
    let normalized_glob_authority = normalize_authority_for_matching(glob_authority, decode_percent_encoding);
    (normalized_authority != authority || normalized_glob_authority != glob_authority)
        && do_url_part_match(&normalized_authority, &normalized_glob_authority, true)
}

/// Canonicaliza rótulos DNS literais sem reinterpretar rótulos com curinga, portas nem informação de usuário.
fn normalize_authority_for_matching(authority: &[u16], decode_percent_encoding: bool) -> Vec<u16> {
    let hostname_start = authority.iter().rposition(|&u| u == b'@' as u16).map_or(0, |i| i + 1);
    if authority.get(hostname_start) == Some(&(b'[' as u16)) {
        return authority.to_vec();
    }
    let port_start = authority[hostname_start..].iter().position(|&u| u == COLON).map(|i| i + hostname_start);
    let hostname_end = port_start.unwrap_or(authority.len());
    let hostname: Vec<Vec<u16>> = authority[hostname_start..hostname_end]
        .split(|&u| u == DOT)
        .map(|label| normalize_label(label, decode_percent_encoding))
        .collect();
    let mut out = authority[..hostname_start].to_vec();
    for (index, label) in hostname.iter().enumerate() {
        if index > 0 {
            out.push(DOT);
        }
        out.extend_from_slice(label);
    }
    out.extend_from_slice(&authority[hostname_end..]);
    out
}

fn normalize_label(label: &[u16], decode_percent_encoding: bool) -> Vec<u16> {
    let has = |unit: u16| label.contains(&unit);
    if has(STAR)
        || label.iter().any(|&u| matches!(u, SLASH | BACKSLASH | 0x3F | 0x23) || is_js_whitespace(u))
        || (!decode_percent_encoding && has(PERCENT))
    {
        return label.to_vec();
    }
    // `/^[\w-]*$/`
    if label.iter().all(|&u| u == b'-' as u16 || u == b'_' as u16 || char::from_u32(u32::from(u)).is_some_and(|c| c.is_ascii_alphanumeric())) {
        return JsString::from_units(label.to_vec()).to_lower_case().units().to_vec();
    }
    // Um sufixo impede que rótulos numéricos sejam lidos como endereços IPv4.
    match host_after_label(&JsString::from_units(label.to_vec())) {
        Some(normalized) if !normalized.units().contains(&STAR) => normalized.units().to_vec(),
        _ => label.to_vec(),
    }
}

fn unit_at(text: &[u16], index: usize) -> Option<u16> {
    text.get(index).copied()
}

/// Casa partes de URL sem recursão nem tabela URL por padrão alocada de uma vez.
pub(crate) fn do_url_part_match(url_part: &[u16], glob_url_part: &[u16], include_port_logic: bool) -> bool {
    if url_part == glob_url_part {
        return true;
    }
    if !glob_url_part.contains(&STAR) {
        return url_part.len() > glob_url_part.len() && url_part.starts_with(glob_url_part) && url_part[glob_url_part.len()] == SLASH;
    }

    struct Pending {
        queue: VecDeque<usize>,
        seen: HashSet<usize>,
    }
    let mut pending: HashMap<usize, Pending> = HashMap::new();
    let add_state = |pending: &mut HashMap<usize, Pending>, url_offset: usize, glob_offset: usize| {
        let entry = pending.entry(url_offset).or_insert_with(|| Pending { queue: VecDeque::new(), seen: HashSet::new() });
        if entry.seen.insert(glob_offset) {
            entry.queue.push_back(glob_offset);
        }
    };
    add_state(&mut pending, 0, 0);

    let mut url_offset = 0;
    while url_offset <= url_part.len() && !pending.is_empty() {
        let Some(mut current) = pending.remove(&url_offset) else {
            url_offset += 1;
            continue;
        };
        // Estados acrescentados ao deslocamento atual durante a iteração também são visitados.
        loop {
            let Some(glob_offset) = current.queue.pop_front() else { break };
            let any_port = include_port_logic && unit_at(glob_url_part, glob_offset) == Some(COLON) && unit_at(glob_url_part, glob_offset + 1) == Some(STAR);
            if url_offset == url_part.len() {
                if glob_offset == glob_url_part.len() || (any_port && glob_offset + 2 == glob_url_part.len()) {
                    return true;
                }
                continue;
            }
            if glob_offset == glob_url_part.len() {
                if url_part[url_offset] == SLASH {
                    return true;
                }
                continue;
            }

            let push = |url_offset_new: usize, glob_offset_new: usize, pending: &mut HashMap<usize, Pending>, current: &mut Pending| {
                if url_offset_new == url_offset {
                    if current.seen.insert(glob_offset_new) {
                        current.queue.push_back(glob_offset_new);
                    }
                } else {
                    add_state(pending, url_offset_new, glob_offset_new);
                }
            };

            if url_part[url_offset] == glob_url_part[glob_offset] {
                push(url_offset + 1, glob_offset + 1, &mut pending, &mut current);
            }
            if glob_url_part[glob_offset] == STAR {
                if glob_offset + 1 == glob_url_part.len() {
                    return true;
                }
                if glob_url_part[glob_offset + 1] == DOT {
                    if !matches!(url_part[url_offset], SLASH | COLON) {
                        push(url_offset + 1, glob_offset, &mut pending, &mut current);
                    }
                    if url_offset == 0 || url_part[url_offset - 1] == DOT {
                        push(url_offset, glob_offset + 2, &mut pending, &mut current);
                    }
                }
                push(url_offset + 1, if url_offset + 1 == url_part.len() { glob_offset + 1 } else { glob_offset }, &mut pending, &mut current);
                push(url_offset, glob_offset + 1, &mut pending, &mut current);
            }
            if any_port {
                let mut end_port_index = url_offset;
                if url_part[url_offset] == COLON {
                    end_port_index += 1;
                    loop {
                        end_port_index += 1;
                        if !unit_at(url_part, end_port_index).is_some_and(|u| (0x30..=0x39).contains(&u)) {
                            break;
                        }
                    }
                }
                push(end_port_index, glob_offset + 2, &mut pending, &mut current);
            }
        }
        // Os estados nunca andam para trás, então os deslocamentos concluídos não precisam ficar na memória.
        url_offset += 1;
    }
    false
}
