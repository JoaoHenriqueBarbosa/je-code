//! Porte de `src/vs/platform/url/common/trustedDomains.ts`: quais domínios o usuário confia, para abrir
//! links sem perguntar.
//!
//! Divergências: o `try`/`catch` de `normalizeURL` é [`je_base::errors::catch`]; as regex de autoridade
//! (`rLocalhost`, `r127`, `rIPv6Localhost`, `r0000`, `rIPv6AllInterfaces`) são escritas à mão sobre
//! unidades UTF-16, com a mesma semântica (`.` não casa quebra de linha, `\d` é ASCII, só o `localhost`
//! ignora maiúsculas).

use je_base::JsString;
use je_base::errors;
use je_base::network::schemas;
use je_base::uri::{Field, URI, UriChange};

use super::url_glob::{
    UrlInput, decode_unreserved_url_path_characters, has_scheme, is_http_or_https, normalize_url_path_separators, test_url_matches_glob_js, units,
};

const AT: u16 = b'@' as u16;

/// `isURLDomainTrusted`: confere se um domínio como `https://www.microsoft.com` casa com a lista de
/// domínios confiáveis.
///
/// - Os esquemas precisam casar.
/// - Não há casamento de subdomínio: `https://microsoft.com` não casa `https://www.microsoft.com`.
/// - A estrela casa todos os subdomínios: `https://*.microsoft.com` casa `https://www.microsoft.com` e
///   `https://foo.bar.microsoft.com`.
pub fn is_url_domain_trusted(url: &URI, trusted_domains: &[JsString]) -> bool {
    if trusted_domains.iter().any(|d| *d == "*") {
        return true;
    }
    if !is_url_safe_for_trust(url) {
        return false;
    }
    let url = normalize_url_for_trust(url);
    if !is_url_safe_for_trust(&url) {
        return false;
    }
    if is_localhost_authority(url.authority().units()) {
        return true;
    }
    trusted_domains.iter().any(|domain| {
        let replaced = JsString::from_units(domain.units().iter().map(|&u| if u == b'\\' as u16 { b'/' as u16 } else { u }).collect());
        test_url_matches_glob_js(UrlInput::Uri(url.clone()), &normalize_url_pattern(&replaced))
    })
}

/// Conveniência de [`is_url_domain_trusted`] para listas de `&str`.
pub fn is_url_domain_trusted_str(url: &URI, trusted_domains: &[&str]) -> bool {
    let domains: Vec<JsString> = trusted_domains.iter().map(|d| JsString::from(*d)).collect();
    is_url_domain_trusted(url, &domains)
}

/// `isURLSafeForTrust`: a URL tem uma autoridade que pode servir a uma decisão de confiança.
pub fn is_url_safe_for_trust(url: &URI) -> bool {
    !url.authority().is_empty() && !has_url_user_information(url)
}

/// `hasURLUserInformation`: a autoridade tem informação de usuário.
pub fn has_url_user_information(url: &URI) -> bool {
    url.authority().units().contains(&AT)
}

/// `normalizeURL`: normaliza caminhos HTTP(S) efetivos e caminhos que não diferenciam maiúsculas (GitHub).
pub fn normalize_url(url: impl Into<UrlInput>) -> JsString {
    let input: UrlInput = url.into();
    let fallback = match &input {
        UrlInput::Str(text) => text.clone(),
        UrlInput::Uri(uri) => uri.to_string(false),
    };
    errors::catch(|| {
        let uri = match input {
            UrlInput::Str(text) => URI::parse(text, true),
            UrlInput::Uri(uri) => uri,
        };
        normalize_url_for_trust(&uri).to_string(true)
    })
    .unwrap_or(fallback)
}

/// `normalizeURLForTrust`: normaliza uma URL para comparar confiança sem decodificar escapes reservados.
pub fn normalize_url_for_trust(url: &URI) -> URI {
    let parsed = normalize_url_path_separators(url);
    let units_of_authority = parsed.authority().units();
    let host_offset = units_of_authority.iter().position(|&u| u == AT).map_or(0, |i| i + 1);
    let mut authority = units_of_authority[..host_offset].to_vec();
    authority.extend_from_slice(JsString::from_units(units_of_authority[host_offset..].to_vec()).to_lower_case().units());
    let authority = JsString::from_units(authority);

    let mut path = parsed.path().clone();
    if authority == "github.com" {
        path = if is_http_or_https(&parsed) { lowercase_url_path(&path) } else { path.to_lower_case() };
    }
    parsed.with(&UriChange { authority: Field::Set(authority), path: Field::Set(path), ..UriChange::default() })
}

fn lowercase_url_path(path: &JsString) -> JsString {
    let unreserved = decode_unreserved_url_path_characters(path.units());
    // `/%[89a-f][0-9a-f]/i`
    let has_high_escape = unreserved.windows(3).any(|w| {
        w[0] == b'%' as u16 && hex_value(w[1]).is_some_and(|v| v >= 8) && hex_value(w[2]).is_some()
    });
    if !has_high_escape {
        return JsString::from_units(unreserved).to_lower_case();
    }
    // Decodifica o Unicode pelo decodificador tolerante do URI, sem reinterpretar sequências ASCII.
    let mut protected_path: Vec<u16> = Vec::with_capacity(unreserved.len());
    let mut i = 0;
    while i < unreserved.len() {
        let is_low_escape =
            unreserved[i] == b'%' as u16 && hex_value(unreserved.get(i + 1).copied().unwrap_or(0)).is_some_and(|v| v <= 7) && unreserved.get(i + 2).copied().and_then(hex_value).is_some();
        if is_low_escape {
            protected_path.extend(units("%25"));
            protected_path.extend_from_slice(&unreserved[i + 1..i + 3]);
            i += 3;
        } else {
            protected_path.push(unreserved[i]);
            i += 1;
        }
    }
    let mut text = JsString::from(format!("{}://url.invalid", schemas::HTTPS));
    text.push_units(&protected_path);
    URI::parse(text, false).path().to_lower_case()
}

fn hex_value(unit: u16) -> Option<u32> {
    char::from_u32(u32::from(unit)).and_then(|c| c.to_digit(16))
}

/// `serializeURLPattern`: serializa um padrão normalizado sem perder escapes literais quando o casador
/// o analisa de novo.
pub fn serialize_url_pattern(url: &URI) -> JsString {
    let escape = |text: &JsString| JsString::from_units(text.units().iter().flat_map(|&u| if u == b'%' as u16 { units("%25") } else { vec![u] }).collect());
    url.with(&UriChange { authority: Field::Set(escape(url.authority())), path: Field::Set(escape(url.path())), ..UriChange::default() }).to_string(true)
}

/// `normalizeURLPattern`: normaliza padrões configurados preservando a aplicabilidade de globs sem esquema.
pub fn normalize_url_pattern(pattern: &JsString) -> JsString {
    let has = has_scheme(pattern.units());
    let prefix = format!("{}://", schemas::HTTPS);
    let source = if has {
        pattern.clone()
    } else {
        let mut full = JsString::from(prefix.as_str());
        full.push_units(pattern.units());
        full
    };
    let normalized = serialize_url_pattern(&normalize_url_for_trust(&URI::parse(source, false)));
    if has { normalized } else { normalized.slice(prefix.len(), normalized.len()) }
}

/// Casa `authority` com `<host><:porta opcional>`, para cada `host` da lista (comparação exata).
fn is_host_with_optional_port(authority: &[u16], hosts: &[&str]) -> bool {
    hosts.iter().any(|host| {
        let host = units(host);
        authority.starts_with(&host) && {
            let rest = &authority[host.len()..];
            rest.is_empty() || (rest[0] == b':' as u16 && rest.len() > 1 && rest[1..].iter().all(|&u| (0x30..=0x39).contains(&u)))
        }
    })
}

/// `isLocalhostAuthority`: `localhost`, `*.localhost`, `127.0.0.1` e `[::1]`, com porta opcional.
pub fn is_localhost_authority(authority: &[u16]) -> bool {
    // `/^(.+\.)?localhost(:\d+)?$/i`: `localhost` sem distinção de maiúsculas, precedido por `<algo>.`.
    let lowered: Vec<u16> = authority.iter().map(|&u| if (0x41..=0x5A).contains(&u) { u + 0x20 } else { u }).collect();
    let localhost = units("localhost");
    let candidates = std::iter::once(lowered.as_slice()).chain(strip_port(&lowered));
    let r_localhost = candidates.into_iter().any(|text| {
        text == localhost.as_slice()
            || (text.ends_with(&units(".localhost")) && {
                let prefix = &text[..text.len() - localhost.len() - 1];
                !prefix.is_empty() && !prefix.iter().any(|&u| matches!(u, 0x0A | 0x0D | 0x2028 | 0x2029))
            })
    });
    r_localhost || is_host_with_optional_port(authority, &["127.0.0.1", "[::1]", "[0:0:0:0:0:0:0:1]"])
}

/// O texto sem o sufixo `:<dígitos>`, quando ele existe.
fn strip_port(text: &[u16]) -> Option<&[u16]> {
    let colon = text.iter().rposition(|&u| u == b':' as u16)?;
    let digits = &text[colon + 1..];
    (!digits.is_empty() && digits.iter().all(|&u| (0x30..=0x39).contains(&u))).then_some(&text[..colon])
}

/// `isAllInterfacesAuthority`: `0.0.0.0` e `[::]`, com porta opcional.
pub fn is_all_interfaces_authority(authority: &[u16]) -> bool {
    is_host_with_optional_port(authority, &["0.0.0.0", "[::]", "[0:0:0:0:0:0:0:0]"])
}
