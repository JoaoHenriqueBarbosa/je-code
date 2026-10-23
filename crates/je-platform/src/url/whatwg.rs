//! O `new URL(...)` do padrão WHATWG (V8, navegador), sobre a crate `url` (Rust puro, com `idna`).
//!
//! O `urlGlob.ts` e o `trustedDomains.ts` perguntam ao `URL` do JavaScript onde o navegador realmente iria
//! parar (autoridade, a barra invertida como `/`, segmentos de ponto, nome IDN). Aqui o `URL` é a crate `url`; o texto de
//! entrada é um `USVString`, então surrogates isolados viram U+FFFD, como no JavaScript.
//!
//! Divergência da crate `url` que este módulo contorna: ao resolver `..`, a crate não remove um segmento
//! que parece letra de unidade do Windows (`C:`) mesmo quando o esquema não é `file` (`last_slash_can_be_removed`
//! em `parser.rs`), e o padrão só abre essa exceção para `file`. Em `http://x/a/C:/../b` o navegador
//! e o Node dão `/a/b`, e a crate daria `/a/C:/b`, o que deixaria uma URL escapar de um glob com escopo de
//! caminho. Por isso, nos esquemas especiais que não são `file`, os segmentos de ponto são resolvidos aqui
//! ([`resolve_special_path`], o estado de caminho do padrão) antes de a crate codificar e serializar.

use je_base::JsString;
use je_base::errors::{self, Error};
use ::url::Url;

/// `new URL(input)`: lança `TypeError: Invalid URL` quando o parser rejeita, como o Node.
pub fn new_url(input: &JsString) -> Url {
    match try_new_url(input) {
        Some(url) => url,
        None => errors::throw(Error::type_error("Invalid URL")),
    }
}

/// `new URL(input)` que devolve `None` onde o JavaScript lançaria (o `try`/`catch` do chamador).
pub fn try_new_url(input: &JsString) -> Option<Url> {
    Url::parse(&resolve_dot_segments_in_input(&input.to_string_lossy())).ok()
}

/// `url.pathname = path`, com a resolução de segmentos de ponto do padrão (ver o comentário do módulo).
pub fn set_pathname(url: &mut Url, path: &str) {
    if matches!(url.scheme(), "http" | "https" | "ws" | "wss" | "ftp") {
        let cleaned: String = path.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
        url.set_path(&resolve_special_path(&cleaned));
    } else {
        url.set_path(path);
    }
}

fn is_special_non_file(scheme: &str) -> bool {
    matches!(scheme, "http" | "https" | "ws" | "wss" | "ftp")
}

fn is_double_dot(segment: &str) -> bool {
    matches!(segment.to_ascii_lowercase().as_str(), ".." | ".%2e" | "%2e." | "%2e%2e")
}

fn is_single_dot(segment: &str) -> bool {
    matches!(segment.to_ascii_lowercase().as_str(), "." | "%2e")
}

/// O estado de caminho do padrão para um esquema especial que não é `file`: separa em `/` e na barra invertida, resolve
/// `.` e `..` (incluindo `%2e`) e devolve o caminho com `/` inicial, sem codificar nada.
pub fn resolve_special_path(path: &str) -> String {
    let rest = path.strip_prefix(['/', '\\']).unwrap_or(path);
    let mut segments: Vec<&str> = Vec::new();
    let mut buffer_start = 0;
    let bytes = rest.as_bytes();
    let mut index = 0;
    loop {
        let at_end = index >= bytes.len();
        if at_end || bytes[index] == b'/' || bytes[index] == b'\\' {
            let buffer = &rest[buffer_start..index];
            if is_double_dot(buffer) {
                segments.pop();
                if at_end {
                    segments.push("");
                }
            } else if is_single_dot(buffer) {
                if at_end {
                    segments.push("");
                }
            } else {
                segments.push(buffer);
            }
            if at_end {
                break;
            }
            buffer_start = index + 1;
        }
        index += 1;
    }
    format!("/{}", segments.join("/"))
}

/// Antes de entregar o texto à crate `url`, resolve o caminho de um esquema especial que não é `file`.
fn resolve_dot_segments_in_input(input: &str) -> String {
    let trimmed = input.trim_matches(|c: char| c <= ' ');
    let cleaned: String = trimmed.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
    let Some(colon) = cleaned.find(':') else { return cleaned };
    let scheme = cleaned[..colon].to_ascii_lowercase();
    if !is_special_non_file(&scheme) {
        return cleaned;
    }
    let after_scheme = &cleaned[colon + 1..];
    let authority_start = after_scheme.find(|c| !matches!(c, '/' | '\\')).unwrap_or(after_scheme.len());
    let from_authority = &after_scheme[authority_start..];
    let authority_end = from_authority.find(['/', '\\', '?', '#']).unwrap_or(from_authority.len());
    let (authority, from_path) = from_authority.split_at(authority_end);
    let path_end = from_path.find(['?', '#']).unwrap_or(from_path.len());
    let (path, tail) = from_path.split_at(path_end);
    format!("{scheme}://{authority}{}{tail}", resolve_special_path(path))
}

/// `URL#pathname`.
pub fn pathname(url: &Url) -> JsString {
    JsString::from(url.path())
}

/// `URL#hostname`: o host serializado (IPv6 entre colchetes), `""` quando não há host.
pub fn hostname(url: &Url) -> JsString {
    JsString::from(url.host_str().unwrap_or(""))
}

/// `URL#host`: o `hostname` mais `:porta` quando a porta não é a padrão do esquema.
pub fn host(url: &Url) -> JsString {
    let hostname = url.host_str().unwrap_or("");
    match url.port() {
        Some(port) => JsString::from(format!("{hostname}:{port}")),
        None => JsString::from(hostname),
    }
}

/// `URL#href`.
pub fn href(url: &Url) -> JsString {
    JsString::from(url.as_str())
}

/// O `hostname` que o `URL` dá a `http://<rótulo>.invalid`, sem o sufixo: a forma canônica (minúsculas,
/// Punycode) de um rótulo DNS isolado. `None` quando o `URL` lança ou o host não termina no sufixo.
pub fn host_after_label(label: &JsString) -> Option<JsString> {
    const SUFFIX: &str = ".invalid";
    let mut text = JsString::from("http://");
    text.push_units(label.units());
    text.push_units(&SUFFIX.encode_utf16().collect::<Vec<_>>());
    let url = try_new_url(&text)?;
    let hostname = url.host_str()?;
    hostname.strip_suffix(SUFFIX).map(JsString::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Saídas conferidas no Node 26 (`new URL(entrada).href`): a crate `url` sozinha erra a primeira.
    #[test]
    fn href_matches_node_for_special_paths() {
        let cases = [
            ("http://x/a/C:/../b", "http://x/a/b"),
            ("http://x/a/b/..", "http://x/a/"),
            ("http://x/a/./b/%2E%2e/c?q=../#f", "http://x/a/c?q=../#f"),
            (r"https://h\a\..\b", "https://h/b"),
            ("http://x/a/%2e%2E", "http://x/"),
            ("http://x//..//a", "http://x//a"),
        ];
        for (input, expected) in cases {
            assert_eq!(href(&new_url(&JsString::from(input))), expected, "{input}");
        }
    }

    #[test]
    fn invalid_input_throws_type_error() {
        let error = errors::catch(|| new_url(&JsString::from("not-a-valid-url"))).unwrap_err();
        assert_eq!(error.message, "Invalid URL");
    }
}
