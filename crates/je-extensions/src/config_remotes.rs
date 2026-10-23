//! Porte de `src/vs/platform/extensionManagement/common/configRemotes.ts`: extrai os remotes (`url = ...`) de um
//! `.git/config`, para o domínio anonimizado e para a forma normalizada que as recomendações de extensão usam.
//!
//! Os regex são os do original, executados pelo `zyarr` (YARR do JavaScriptCore), com a mesma semântica do
//! `RegExp` do JavaScript.

use std::ops::Range;
use std::sync::OnceLock;

use je_base::uri::URI;
use zyarr::RegExp;

fn regex(pattern: &str, flags: &str) -> RegExp {
    RegExp::from_str(pattern, flags).expect("regex do configRemotes é válido")
}

struct Matchers {
    ssh_protocol: RegExp,
    ssh_url: RegExp,
    authority: RegExp,
    second_level_domain: RegExp,
    remote: RegExp,
}

fn matchers() -> &'static Matchers {
    // Os regex não têm estado (o `lastIndex` é explícito em `exec`).
    static MATCHERS: OnceLock<Matchers> = OnceLock::new();
    MATCHERS.get_or_init(|| Matchers {
        ssh_protocol: regex(r"^([^@:]+@)?([^:]+):", ""),
        ssh_url: regex(r"^([^@:]+@)?([^:]+):(.+)$", ""),
        authority: regex(r"^([^@]+@)?([^:]+)(:\d+)?$", ""),
        second_level_domain: regex(r"([^@:.]+\.[^@:.]+)(:\d+)?$", ""),
        remote: regex(r"^\s*url\s*=\s*(.+\S)\s*$", "mg"),
    })
}

/// `AllowedSecondLevelDomains`.
pub const ALLOWED_SECOND_LEVEL_DOMAINS: [&str; 13] = [
    "github.com",
    "bitbucket.org",
    "visualstudio.com",
    "gitlab.com",
    "heroku.com",
    "azurewebsites.net",
    "ibm.com",
    "amazon.com",
    "amazonaws.com",
    "cloudapp.net",
    "rhcloud.com",
    "google.com",
    "azure.com",
];

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn slice(text: &[u16], range: &Range<usize>) -> String {
    String::from_utf16_lossy(&text[range.clone()])
}

/// O grupo `n` de `text.match(regex)`.
fn match_group(regex: &RegExp, text: &str, group: usize) -> Option<String> {
    let text_units = units(text);
    let found = regex.exec(&text_units, 0)?;
    found.captures.get(group)?.as_ref().map(|range| slice(&text_units, range))
}

fn strip_low_level_domains(domain: &str) -> Option<String> {
    match_group(&matchers().second_level_domain, domain, 1)
}

fn uri_authority_and_path(url: &str) -> Option<(String, String)> {
    let uri = URI::parse(url, false);
    let authority = uri.authority().to_string_lossy();
    (!authority.is_empty()).then(|| (authority, uri.path().to_string_lossy()))
}

fn extract_domain(url: &str) -> Option<String> {
    if !url.contains("://") {
        return match_group(&matchers().ssh_protocol, url, 2).and_then(|host| strip_low_level_domains(&host));
    }
    uri_authority_and_path(url).and_then(|(authority, _)| strip_low_level_domains(&authority))
}

/// Cada `url = ...` do texto (`RemoteMatcher.exec` em laço).
fn remote_urls(text: &str) -> Vec<String> {
    let text_units = units(text);
    let remote = &matchers().remote;
    let mut urls = Vec::new();
    let mut last_index = 0;
    while let Some(found) = remote.exec(&text_units, last_index) {
        let whole = found.captures[0].clone().expect("o casamento inteiro sempre existe");
        if let Some(Some(range)) = found.captures.get(1) {
            urls.push(slice(&text_units, range));
        }
        // Um casamento vazio avança uma unidade, como o `lastIndex` do JavaScript.
        last_index = if whole.end == whole.start { whole.end + 1 } else { whole.end };
    }
    urls
}

/// `getDomainsOfRemotes`: os domínios de segundo nível dos remotes; os que não estão em `allowed_domains` são
/// anonimizados (cada caractere que não é `.` vira `a`).
pub fn get_domains_of_remotes(text: &str, allowed_domains: &[&str]) -> Vec<String> {
    let mut domains: Vec<String> = Vec::new();
    for url in remote_urls(text) {
        if let Some(domain) = extract_domain(&url)
            && !domains.contains(&domain)
        {
            domains.push(domain);
        }
    }
    domains
        .into_iter()
        .map(|key| if allowed_domains.contains(&key.as_str()) { key } else { key.chars().map(|c| if c == '.' { '.' } else { 'a' }).collect() })
        .collect()
}

fn strip_port(authority: &str) -> Option<String> {
    match_group(&matchers().authority, authority, 2)
}

fn normalize_remote(host: Option<String>, path: &str, strip_ending_dot_git: bool) -> Option<String> {
    let host = host.filter(|h| !h.is_empty())?;
    if path.is_empty() {
        return None;
    }
    let path = if strip_ending_dot_git { path.strip_suffix(".git").unwrap_or(path) } else { path };
    Some(if path.starts_with('/') { format!("{host}{path}") } else { format!("{host}/{path}") })
}

fn extract_remote(url: &str, strip_ending_dot_git: bool) -> Option<String> {
    if !url.contains("://") {
        let text_units = units(url);
        if let Some(found) = matchers().ssh_url.exec(&text_units, 0) {
            let group = |n: usize| found.captures.get(n).and_then(|r| r.as_ref()).map(|r| slice(&text_units, r));
            return normalize_remote(group(2), &group(3).unwrap_or_default(), strip_ending_dot_git);
        }
    }
    let (authority, path) = uri_authority_and_path(url)?;
    normalize_remote(strip_port(&authority), &path, strip_ending_dot_git)
}

/// `getRemotes`.
pub fn get_remotes(text: &str, strip_ending_dot_git: bool) -> Vec<String> {
    remote_urls(text).iter().filter_map(|url| extract_remote(url, strip_ending_dot_git)).collect()
}
