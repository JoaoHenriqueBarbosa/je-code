//! Porte de `src/vs/platform/networkFilter/common/domainMatcher.ts`: normalização de domínios e padrões
//! de administrador (permitir/negar), casamento com curinga e a regra de `isDomainAllowed`.
//!
//! Os domínios são `&str`/`String`: o original só compara ASCII de nomes de host, e as expressões regulares
//! dele são classes ASCII simples, traduzidas aqui como verificações de caractere (sem índices UTF-16).
//! O `new URL(...)` do JavaScript é a crate `url` ([`crate::url::whatwg`]), que decide IDN (Punycode),
//! IPv4 em formas alternativas (`127.1`, `0x7f000001`) e serialização de IPv6, como o navegador.
//! O `trim()` e o `toLowerCase()` seguem o JavaScript (`trim` inclui U+FEFF e não inclui U+0085).

use je_base::errors;
use je_base::uri::{URI, decode_uri_component};
use je_base::JsString;
use ::url::Url;

use crate::url::whatwg::{hostname, try_new_url};

/// Extensões de arquivo comuns que dificilmente são domínios de rede.
/// Usadas para filtrar falsos positivos ao extrair domínios de texto livre.
const FILE_EXTENSION_SUFFIXES: &[&str] = &[
    "7z", "bz2", "cjs", "class", "cpp", "cs", "css", "csv", "dll", "exe", "gif", "gz", "ico", "jar", "env", "java", "jpeg", "jpg",
    "js", "json", "jsx", "lock", "log", "md", "mjs", "pdf", "php", "png", "py", "rar", "rs", "so", "sql", "svg", "tar", "tgz",
    "toml", "ts", "tsx", "txt", "wasm", "webp", "xml", "yaml", "yml", "zip",
];

/// A detecção de host sem esquema é uma heurística usada só para perguntar antes de rodar comandos que parecem
/// acessar a rede. A lista é conservadora de propósito: TLDs comuns em hosts e serviços de programação.
const WELL_KNOWN_DOMAIN_SUFFIXES: &[&str] = &["ai", "cloud", "com", "dev", "io", "me", "net", "org", "tech"];

/// `String.prototype.trim`: espaço em branco e terminadores de linha do ECMAScript.
fn js_trim(value: &str) -> &str {
    value.trim_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{FEFF}')
}

/// `[a-z0-9]`.
fn is_alnum(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

/// `/^(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)(?:\.(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?))*$/`: rótulos separados
/// por ponto, cada um começando e terminando em letra minúscula ou dígito, com hífen só no meio.
fn is_valid_hostname(value: &str) -> bool {
    value.split('.').all(|label| {
        let mut chars = label.chars();
        match (chars.next(), label.chars().next_back()) {
            (Some(first), Some(last)) => is_alnum(first) && is_alnum(last) && label.chars().all(|c| is_alnum(c) || c == '-'),
            _ => false,
        }
    })
}

/// `/^\*?\.?[a-z0-9.;,)!?:-]+$/`: como o ponto também está no conjunto, o `\.?` opcional nunca decide nada;
/// só o `*` inicial opcional e um resto não vazio de caracteres do conjunto.
fn has_allowed_characters(value: &str) -> bool {
    let rest = value.strip_prefix('*').unwrap_or(value);
    !rest.is_empty() && rest.chars().all(|c| is_alnum(c) || matches!(c, '.' | ';' | ',' | ')' | '!' | '?' | ':' | '-'))
}

/// `value.replace(/:\d+$/, '')`.
fn strip_trailing_port(value: &str) -> &str {
    let without_digits = value.trim_end_matches(|c: char| c.is_ascii_digit());
    if without_digits.len() < value.len() {
        if let Some(before_colon) = without_digits.strip_suffix(':') {
            return before_colon;
        }
    }
    value
}

/// `value.replace(/^[^@]+@/, '')`: o prefixo até o primeiro `@`, se houver ao menos um caractere antes dele.
fn strip_user_info(value: &str) -> &str {
    match value.find('@') {
        Some(index) if index > 0 => &value[index + 1..],
        _ => value,
    }
}

/// Normaliza e valida uma string de domínio.
///
/// Remove informação de usuário, porta, pontos finais e pontuação final.
/// Aceita o curinga solto (`*`) e o prefixo curinga (`*.example.com`).
///
/// `from_url` indica que o valor veio de um contexto de URL (pula o filtro de extensão de arquivo).
/// Devolve `None` quando a entrada é inválida.
pub fn normalize_domain(value: Option<&str>, from_url: bool) -> Option<String> {
    let value = value.filter(|v| !v.is_empty())?;

    let lowered = js_trim(value).to_lowercase();
    let normalized = strip_trailing_port(strip_user_info(&lowered)).trim_end_matches('.');
    if normalized.is_empty() || normalized.contains('/') || normalized == "." || normalized == ".." {
        return None;
    }

    // Allow a bare wildcard pattern early, before hostname validation.
    if normalized == "*" {
        return Some("*".to_owned());
    }

    if !has_allowed_characters(normalized) {
        return None;
    }

    // Strip common trailing punctuation that may follow a domain in text, e.g. "example.com,".
    let stripped = normalized.trim_end_matches([')', ',', ';', ':', '!', '?']);
    if stripped.is_empty() {
        return None;
    }

    let domain_to_validate = stripped.strip_prefix("*.").unwrap_or(stripped);
    if !is_valid_hostname(domain_to_validate) {
        return None;
    }

    // Support wildcard domain patterns like "*.example.com".
    let has_wildcard_prefix = stripped.starts_with("*.");
    let host = domain_to_validate;
    if host.is_empty() {
        return None;
    }

    // Validate that the host part only contains valid hostname characters.
    if !host.chars().all(|c| is_alnum(c) || c == '.' || c == '-') {
        return None;
    }

    // Disallow patterns that look like file names with common extensions, as these are unlikely
    // to be intended as network domains and may be false positives from the regex.
    if !from_url {
        let last_label = &host[host.rfind('.').map_or(0, |index| index + 1)..];
        if FILE_EXTENSION_SUFFIXES.contains(&last_label) {
            return None;
        }
        if !WELL_KNOWN_DOMAIN_SUFFIXES.contains(&last_label) {
            return None;
        }
    }

    Some(if has_wildcard_prefix { format!("*.{host}") } else { host.to_owned() })
}

fn normalize_uri_authority(authority: &str) -> Option<String> {
    if authority.is_empty() || authority.contains(['/', '?', '#', '\\']) {
        return None;
    }

    let url = try_new_url(&JsString::from(format!("http://{authority}")))?;
    if has_host_like_credentials(&url) {
        return None;
    }
    let host = hostname(&url).to_string_lossy().to_lowercase();

    if host.starts_with('[') {
        return if host.ends_with(']') { Some(host) } else { None };
    }

    normalize_domain(Some(&host), true)
}

fn has_host_like_credentials(url: &Url) -> bool {
    [Some(url.username()), url.password()].into_iter().flatten().any(|value| {
        if value.is_empty() {
            return false;
        }

        let units: Vec<u16> = value.encode_utf16().collect();
        let decoded = match decode_uri_component(&units) {
            Ok(decoded) => String::from_utf16_lossy(&decoded).to_lowercase(),
            Err(_) => return true,
        };

        if decoded.contains('@') {
            return false;
        }

        (decoded.contains('.') && normalize_domain(Some(&decoded), true).is_some()) || normalize_bare_ipv6(&decoded).is_some()
    })
}

fn normalize_bare_ipv6(value: &str) -> Option<String> {
    if !value.contains(':') || value.contains(['/', '?', '#', '\\', '@', '[', ']']) {
        return None;
    }
    normalize_uri_authority(&format!("[{value}]"))
}

/// Normaliza um padrão de permitir/negar do administrador para a forma canônica de host usada no casamento.
pub fn normalize_domain_pattern(pattern: &str) -> Option<String> {
    let extracted_pattern = extract_domain_pattern(pattern);

    if extracted_pattern == "*" {
        return Some(extracted_pattern);
    }

    if let Some(suffix_input) = extracted_pattern.strip_prefix("*.") {
        let candidate = match normalize_uri_authority(suffix_input) {
            Some(normalized_suffix) => format!("*.{normalized_suffix}"),
            None => extracted_pattern.clone(),
        };
        let normalized_pattern = normalize_domain(Some(&candidate), true)?;
        let suffix = normalized_pattern.get(2..).unwrap_or("");
        return Some(format!("*.{}", normalize_uri_authority(suffix).unwrap_or_else(|| suffix.to_owned())));
    }

    normalize_bare_ipv6(&extracted_pattern)
        .or_else(|| normalize_uri_authority(&extracted_pattern))
        .or_else(|| normalize_domain(Some(&extracted_pattern), true))
}

/// `/^[0-9a-f]{1,4}$/` e o `Number.parseInt(..., 16)` que o original aplica ao grupo.
fn parse_hex_group(group: &str) -> Option<u32> {
    if (1..=4).contains(&group.len()) && group.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
        u32::from_str_radix(group, 16).ok()
    } else {
        None
    }
}

/// IPv4 embutido em IPv6: `[::ffff:H:L]` (mapeado) e `[::H:L]` ou `[::L]` (compatível), em forma hexadecimal.
fn normalize_embedded_ipv4(value: &str) -> Option<String> {
    let mapped = value
        .strip_prefix("[::ffff:")
        .and_then(|rest| rest.strip_suffix(']'))
        .and_then(|inner| inner.split_once(':'))
        .and_then(|(high, low)| Some((parse_hex_group(high)?, parse_hex_group(low)?)));

    let (high, low, compatible) = match mapped {
        Some((high, low)) => (high, low, false),
        None => {
            let inner = value.strip_prefix("[::")?.strip_suffix(']')?;
            let (high, low) = match inner.split_once(':') {
                Some((high, low)) => (parse_hex_group(high)?, parse_hex_group(low)?),
                None => (0, parse_hex_group(inner)?),
            };
            (high, low, true)
        }
    };

    if compatible && high == 0 && low <= 1 {
        return None;
    }

    Some(format!("{}.{}.{}.{}", high >> 8, high & 0xff, low >> 8, low & 0xff))
}

/// Extrai a parte de domínio de um padrão.
/// Se o padrão contém `://`, ele é lido como URI e a autoridade é devolvida.
/// Senão, devolve o padrão aparado, como está.
pub fn extract_domain_pattern(pattern: &str) -> String {
    let trimmed = js_trim(pattern);
    if trimmed == "*" || !trimmed.contains("://") {
        return trimmed.to_owned();
    }
    match errors::catch(|| URI::parse(trimmed, false)) {
        Ok(uri) => uri.authority().to_string_lossy(),
        Err(_) => trimmed.to_owned(),
    }
}

/// Confere se um domínio normalizado casa com um padrão de permitir/negar.
/// Suporta igualdade exata, curinga solto (`*`) e prefixo curinga (`*.example.com`).
///
/// `domain` é a saída de [`normalize_domain`]; `pattern` pode ser um domínio, uma URL ou um curinga.
pub fn matches_domain_pattern(domain: &str, pattern: &str) -> bool {
    let Some(normalized_pattern) = normalize_domain_pattern(pattern) else {
        return false;
    };
    if normalized_pattern == "*" {
        return true;
    }
    if let Some(suffix) = normalized_pattern.strip_prefix("*.") {
        let normalized_domain = normalize_embedded_ipv4(domain).unwrap_or_else(|| domain.to_owned());
        return normalized_domain == suffix || normalized_domain.ends_with(&format!(".{suffix}"));
    }
    normalize_embedded_ipv4(domain).unwrap_or_else(|| domain.to_owned())
        == normalize_embedded_ipv4(&normalized_pattern).unwrap_or(normalized_pattern)
}

/// Extrai e normaliza o domínio de uma URI.
/// Separa informação de usuário e porta, e canoniza literais IPv6.
/// Devolve `None` quando não há domínio válido.
pub fn extract_domain_from_uri(uri: &URI) -> Option<String> {
    normalize_uri_authority(&uri.authority().to_string_lossy())
}

/// Decide se um domínio é permitido pelas listas de permitir e negar.
///
/// Regras:
/// - Se as duas listas estão vazias, o domínio é negado (padrão restritivo).
/// - Os padrões negados têm precedência: se o domínio casa com algum, é bloqueado.
/// - Se só há padrões negados (a lista de permitidos está vazia), qualquer domínio não negado é permitido.
/// - Se há padrões permitidos, o domínio precisa casar com ao menos um.
pub fn is_domain_allowed<A: AsRef<str>, D: AsRef<str>>(domain: &str, allowed_patterns: &[A], denied_patterns: &[D]) -> bool {
    // Restrictive default: deny all when both lists are empty.
    if allowed_patterns.is_empty() && denied_patterns.is_empty() {
        return false;
    }

    // Denied patterns take precedence.
    if denied_patterns.iter().any(|pattern| matches_domain_pattern(domain, pattern.as_ref())) {
        return false;
    }

    // If no allowed patterns are configured, allow anything not denied.
    if allowed_patterns.is_empty() {
        return true;
    }

    // The domain must match at least one allowed pattern.
    allowed_patterns.iter().any(|pattern| matches_domain_pattern(domain, pattern.as_ref()))
}
