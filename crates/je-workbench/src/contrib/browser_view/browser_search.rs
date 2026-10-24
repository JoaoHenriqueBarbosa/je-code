//! Porte de `src/vs/workbench/contrib/browserView/common/browserSearch.ts`.
//!
//! NOTA do original: `resolveAddressBarInputType` é uma APROXIMAÇÃO deliberada e autocontida do parser da
//! omnibox do Chromium (`AutocompleteInput::Parse`), não um porte fiel. Ela diverge de propósito em alguns
//! pontos (o principal: qualquer último rótulo de 2 ou mais letras conta como TLD, em vez de consultar a
//! Public Suffix List). Os testes de `tests/browser_search.rs` são a especificação: não "conserte" para bater
//! byte a byte com o Chromium.
//!
//! Notas de tradução:
//! - `new URL(`http://${host}`).hostname` é o `new URL` do padrão WHATWG de `je_platform::url::whatwg`
//!   (a crate `url`, com `idna`), que converte IDN para Punycode e canonicaliza IPv6 entre colchetes.
//! - Os identificadores de motor de busca são cadeias no original (`'nonexistent' as BrowserSearchEngineId`
//!   chega às funções), então `build_search_url` e `get_browser_search_engine_label` recebem `&str`;
//!   [`BrowserSearchEngineId`] dá a forma tipada e o texto de cada id.
//! - `\s` das expressões regulares do original é `is_js_whitespace` (não coincide com `char::is_whitespace`).

use je_base::JsString;
use je_base::js_string::is_js_whitespace;
use je_base::nls::localize;
use je_base::uri::encode_uri_component;
use je_platform::url::whatwg::{hostname, try_new_url};

/// Identificador do motor de busca da barra de endereço do navegador integrado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserSearchEngineId {
    Bing,
    Google,
    Yahoo,
    DuckDuckGo,
}

impl BrowserSearchEngineId {
    /// O valor de texto do id (`'bing'`, `'google'`, `'yahoo'`, `'duckduckgo'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bing => "bing",
            Self::Google => "google",
            Self::Yahoo => "yahoo",
            Self::DuckDuckGo => "duckduckgo",
        }
    }
}

pub const BROWSER_SEARCH_ENGINE_SETTING_ID: &str = "workbench.browser.searchEngine";

/// Valor de [`BROWSER_SEARCH_ENGINE_SETTING_ID`] quando nenhum motor está selecionado (busca na barra de
/// endereço desligada). Selecionar qualquer outro valor liga a busca e escolhe o motor.
pub const BROWSER_SEARCH_NONE: &str = "none";

/// O valor do setting de busca da barra de endereço: `'none'` (busca desligada) ou um
/// [`BrowserSearchEngineId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserSearchEngineValue {
    None,
    Engine(BrowserSearchEngineId),
}

impl BrowserSearchEngineValue {
    /// O texto do valor, como gravado no setting.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => BROWSER_SEARCH_NONE,
            Self::Engine(id) => id.as_str(),
        }
    }
}

/// Um motor de busca que pode ser escolhido como padrão do navegador integrado.
#[derive(Debug, Clone)]
pub struct BrowserSearchEngine {
    pub id: BrowserSearchEngineId,
    /// Rótulo legível mostrado na UI de settings.
    pub label: String,
    build_url: fn(&str) -> String,
}

impl BrowserSearchEngine {
    /// Monta a URL de busca para a consulta. A consulta é a entrada crua do usuário (já aparada); a
    /// codificação da URL é responsabilidade daqui.
    pub fn build_search_url(&self, query: &str) -> String {
        (self.build_url)(query)
    }
}

/// Codifica a consulta para a URL do motor. Igual à dos navegadores populares: `encodeURIComponent` e depois
/// `%20` vira `+`.
fn encode_query(query: &str) -> String {
    let encoded = encode_uri_component(JsString::from(query).units());
    JsString::from_units(encoded).to_string_lossy().replace("%20", "+")
}

/// Lista ordenada dos motores de busca suportados (`BROWSER_SEARCH_ENGINES`).
pub fn browser_search_engines() -> Vec<BrowserSearchEngine> {
    vec![
        BrowserSearchEngine {
            id: BrowserSearchEngineId::Bing,
            label: localize("browser.search.engine.bing", "Bing", &[]),
            build_url: |q| format!("https://www.bing.com/search?q={}", encode_query(q)),
        },
        BrowserSearchEngine {
            id: BrowserSearchEngineId::Google,
            label: localize("browser.search.engine.google", "Google", &[]),
            build_url: |q| format!("https://www.google.com/search?q={}", encode_query(q)),
        },
        BrowserSearchEngine {
            id: BrowserSearchEngineId::Yahoo,
            label: localize("browser.search.engine.yahoo", "Yahoo!", &[]),
            build_url: |q| format!("https://search.yahoo.com/search?p={}", encode_query(q)),
        },
        BrowserSearchEngine {
            id: BrowserSearchEngineId::DuckDuckGo,
            label: localize("browser.search.engine.duckduckgo", "DuckDuckGo", &[]),
            build_url: |q| format!("https://duckduckgo.com/?q={}", encode_query(q)),
        },
    ]
}

/// Classificação de uma entrada da barra de endereço. Espelha os quatro valores não obsoletos do
/// `metrics::OmniboxInputType` do Chromium:
/// - `Empty`: a entrada só tem espaços.
/// - `Url`: reconhecida como URL navegável.
/// - `Query`: reconhecida como consulta de busca (ou URL inválida que só pode ser tratada como consulta).
/// - `Unknown`: ambígua, pode ser URL (host de intranet, TLD novo) ou busca; quem chama deve buscar por
///   padrão, mas pode oferecer "você quis navegar?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressBarInputKind {
    Empty,
    Url,
    Query,
    Unknown,
}

/// Esquemas de URL conhecidos além de http/https e javascript. Qualquer um deles, digitado como prefixo
/// `esquema:`, classifica a entrada como URL.
const KNOWN_URL_SCHEMES: [&str; 11] = [
    "file",
    "ftp",
    "ftps",
    "about",
    "data",
    "view-source",
    "mailto",
    "chrome",
    "edge",
    "vscode",
    "vscode-insiders",
];

/// Todos os esquemas que reconhecemos como esquemas de URL de verdade (para distinguir `esquema:operando` de
/// `host:porta`: `localhost:3000` parece esquema na sintaxe, mas `localhost` não é um esquema conhecido).
fn is_known_scheme(scheme: &str) -> bool {
    KNOWN_URL_SCHEMES.contains(&scheme) || matches!(scheme, "http" | "https" | "javascript")
}

/// TLDs especiais da RFC 6761 / RFC 6762 / ICANN, tratados como conhecidos só quando há subdomínio.
/// `.invalid` é reservado como não navegável.
const SUBDOMAIN_REQUIRED_TLDS: [&str; 4] = ["example", "test", "local", "internal"];

fn is_ws(c: char) -> bool {
    u16::try_from(c).ok().is_some_and(is_js_whitespace)
}

fn has_whitespace(text: &str) -> bool {
    text.chars().any(is_ws)
}

/// `String.prototype.trim`.
fn js_trim(text: &str) -> &str {
    text.trim_matches(is_ws)
}

fn is_port(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// `SCHEME_REGEX = /^([a-z][a-z0-9+\-.]*):/i`: o tamanho do nome do esquema (sem os dois pontos).
fn scheme_name_len(input: &str) -> Option<usize> {
    let mut bytes = input.bytes();
    if !bytes.next().is_some_and(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let tail = bytes.take_while(|&b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.')).count();
    let len = 1 + tail;
    (input.as_bytes().get(len) == Some(&b':')).then_some(len)
}

/// `JAVASCRIPT_QUERY_REGEX = /^javascript:[^;=().\"]*$/i`.
fn is_javascript_query(input: &str) -> bool {
    const PREFIX: &str = "javascript:";
    input.len() >= PREFIX.len()
        && input.is_char_boundary(PREFIX.len())
        && input[..PREFIX.len()].eq_ignore_ascii_case(PREFIX)
        && !input[PREFIX.len()..].contains([';', '=', '(', ')', '.', '"'])
}

/// `USERINFO_WITH_PASSWORD_REGEX = /^[^\s:@/?#]+:[^\s@/?#]+@/`.
fn looks_like_userinfo_with_password(input: &str) -> bool {
    let user_len = input.find(|c: char| is_ws(c) || matches!(c, ':' | '@' | '/' | '?' | '#')).unwrap_or(input.len());
    if user_len == 0 {
        return false;
    }
    let Some(after_colon) = input[user_len..].strip_prefix(':') else { return false };
    let password_len = after_colon.find(|c: char| is_ws(c) || matches!(c, '@' | '/' | '?' | '#')).unwrap_or(after_colon.len());
    password_len > 0 && after_colon[password_len..].starts_with('@')
}

/// `/^\d+(?:[/?#]|$)/`: dígitos seguidos de `/`, `?`, `#` ou do fim (um `host:porta` plausível).
fn starts_with_port_like(input: &str) -> bool {
    let digits = input.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && input[digits..].chars().next().is_none_or(|c| matches!(c, '/' | '?' | '#'))
}

/// `HOST_CHARS_REGEX = /^[a-zA-Z0-9\-._~%]+$/`.
fn has_only_host_chars(host: &str) -> bool {
    !host.is_empty() && host.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'%'))
}

/// `IPV4_REGEX`: quatro grupos de 1 a 3 dígitos separados por ponto.
fn parse_ipv4_octets(host: &str) -> Option<[u32; 4]> {
    let mut octets = [0u32; 4];
    let mut parts = host.split('.');
    for octet in &mut octets {
        let part = parts.next()?;
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *octet = part.parse().ok()?;
    }
    parts.next().is_none().then_some(octets)
}

/// Canonicaliza o host com o parser de URL da plataforma: nomes não ASCII (IDN) viram Punycode (`xn--`) e
/// literais IPv6 entre colchetes são validados e canonicalizados (`[2001:0db8::0001]` vira `[2001:db8::1]`).
/// `None` se o host não puder ser canonicalizado. Hosts ASCII sem colchete voltam como estão (a validação
/// acontece em [`has_only_host_chars`] no chamador).
fn to_ascii_host(host: &str) -> Option<String> {
    let needs_url_parse = host.starts_with('[') || !host.is_ascii();
    if !needs_url_parse {
        return Some(host.to_string());
    }
    let url = try_new_url(&JsString::from(format!("http://{host}")))?;
    Some(hostname(&url).to_string_lossy())
}

struct ParsedAuthority<'a> {
    userinfo: Option<&'a str>,
    host: &'a str,
    port: Option<&'a str>,
    path_and_rest: &'a str,
}

fn parse_host_and_path(rest: &str) -> ParsedAuthority<'_> {
    let sep = rest.find(['/', '?', '#']);
    let (authority, path_and_rest) = sep.map_or((rest, ""), |index| rest.split_at(index));

    let (userinfo, hostport) = match authority.rfind('@') {
        Some(at) => (Some(&authority[..at]), &authority[at + 1..]),
        None => (None, authority),
    };

    let mut host = hostport;
    let mut port = None;
    if host.starts_with('[') {
        if let Some(end) = host.find(']') {
            let after = &host[end + 1..];
            if let Some(maybe_port) = after.strip_prefix(':')
                && is_port(maybe_port)
            {
                port = Some(maybe_port);
                host = &host[..=end];
            } else if after.is_empty() {
                host = &host[..=end];
            }
        }
    } else if let Some(colon) = host.rfind(':') {
        let maybe_port = &host[colon + 1..];
        if is_port(maybe_port) {
            port = Some(maybe_port);
            host = &host[..colon];
        }
    }

    ParsedAuthority { userinfo, host, port, path_and_rest }
}

fn has_known_tld(host: &str) -> bool {
    let lowered = host.to_ascii_lowercase();
    let trimmed = lowered.strip_suffix('.').unwrap_or(&lowered);
    let labels: Vec<&str> = trimmed.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let last = labels[labels.len() - 1];
    if last == "invalid" {
        return false;
    }
    // Qualquer último rótulo só de letras com 2 ou mais caracteres conta como TLD. É um substituto leve da
    // consulta à Public Suffix List do Chromium, que cobre ccTLDs, gTLDs e TLDs novos/de marca sem lista.
    if last.len() >= 2 && last.bytes().all(|b| b.is_ascii_lowercase()) {
        return true;
    }
    // TLDs IDN em Punycode.
    if last.starts_with("xn--") && last.len() >= 5 {
        return true;
    }
    if SUBDOMAIN_REQUIRED_TLDS.contains(&last) {
        // Exige subdomínio (o host precisa ser mais comprido que `${tld}.`).
        return trimmed.len() > last.len() + 1;
    }
    false
}

/// Classifica a entrada crua da barra de endereço em um dos quatro valores não obsoletos do
/// `metrics::OmniboxInputType`: vazia, URL, consulta ou desconhecida.
///
/// Adaptada do `AutocompleteInput::Parse` do Chromium.
///
/// - Entrada vazia ou só de espaços: `Empty`.
/// - Esquema reconhecido que não é http(s) (`file:`, `ftp:`, `about:`, `data:`, `view-source:`, `mailto:`,
///   `chrome:`, `edge:`, `vscode:`...): `Url`.
/// - `javascript:` seguido de algo que não parece código: `Unknown`.
/// - Esquema desconhecido no formato `usuario:senha@host...`: `Url`. Outros (`site:foo`): `Unknown`.
/// - http(s) ou entrada sem esquema é separada em autoridade e caminho:
///   - Espaço no host: `Query`; espaço no userinfo: `Unknown` (salvo esquema http(s) explícito). Espaço no
///     caminho/consulta/fragmento é permitido (codificado na navegação, como no Chromium).
///   - Caractere inválido no host: `Query`.
///   - Literal IPv6 entre colchetes: `Url`.
///   - IPv4 com primeiro octeto diferente de 0 (ou exatamente `0.0.0.0`): `Url`; os demais "parecidos com
///     IPv4" (`0.1.2.3`) caem nas heurísticas seguintes.
///   - `localhost` (com porta/caminho opcionais): `Url`.
///   - Esquema http/https explícito: `Url`.
///   - Barra no fim do caminho: `Url` (atalho de intranet).
///   - Porta explícita: `Url`.
///   - Host com TLD conhecido: `Url`.
///   - `usuario@host` sem TLD explícito: `Unknown` (mais provável ser e-mail).
///   - Mais de um componente além do host: `Url`.
///   - Qualquer outra coisa (palavra solta, host com TLD desconhecido): `Unknown`.
///
/// Quem chama costuma mapear `Url` para navegar e `Query`/`Unknown` para buscar.
pub fn resolve_address_bar_input_type(raw_input: &str) -> AddressBarInputKind {
    let trimmed = js_trim(raw_input);
    if trimmed.is_empty() {
        return AddressBarInputKind::Empty;
    }

    let scheme_len = scheme_name_len(trimmed);
    let candidate_scheme = scheme_len.map(|len| trimmed[..len].to_ascii_lowercase());
    let after_scheme = scheme_len.map_or("", |len| &trimmed[len + 1..]);
    let has_scheme_separator = after_scheme.starts_with("//");
    // Só trata o `xxx:` inicial como esquema se for um esquema reconhecido ou se vier seguido de `//`. Evita
    // ler errado `localhost:3000` ou `sub.example.com:8080/path`, que o URLFixerUpper do Chromium resolve como
    // host:porta e não como operando de esquema.
    let scheme = candidate_scheme.as_deref().filter(|candidate| is_known_scheme(candidate) || has_scheme_separator);
    let is_http_scheme = matches!(scheme, Some("http" | "https"));

    if let Some(scheme) = scheme
        && !is_http_scheme
    {
        if scheme == "file" {
            return AddressBarInputKind::Url;
        }
        if scheme == "javascript" {
            return if is_javascript_query(trimmed) { AddressBarInputKind::Unknown } else { AddressBarInputKind::Url };
        }
        if KNOWN_URL_SCHEMES.contains(&scheme) {
            return AddressBarInputKind::Url;
        }
        // Reconhecido como esquema só por causa do `//`, mas fora do conjunto conhecido. É URL apenas se
        // parecer userinfo com senha.
        if looks_like_userinfo_with_password(trimmed) && !has_whitespace(trimmed) {
            return AddressBarInputKind::Url;
        }
        return AddressBarInputKind::Unknown;
    }

    // Esquema desconhecido SEM `//` e que NÃO é um host:porta plausível (`localhost:3000`). Por exemplo
    // `site:foo`, `unknownscheme:bar`: operadores de busca, classificados como `Unknown`.
    if candidate_scheme.is_some() && scheme.is_none() && !starts_with_port_like(after_scheme) {
        if looks_like_userinfo_with_password(trimmed) && !has_whitespace(trimmed) {
            return AddressBarInputKind::Url;
        }
        return AddressBarInputKind::Unknown;
    }

    // http / https / sem esquema daqui em diante. Remove o prefixo do esquema (e o `//` opcional) para ler a
    // autoridade, mas só se um esquema foi reconhecido. Em `localhost:3000`, `candidate_scheme` existe e
    // `scheme` não, então os dois pontos ficam e viram host:porta.
    let mut rest = trimmed;
    if scheme.is_some() {
        rest = after_scheme;
        rest = rest.strip_prefix("//").unwrap_or(rest);
    }

    let ParsedAuthority { userinfo, host: raw_host, port, path_and_rest } = parse_host_and_path(rest);

    // Entrada sem host que começa com `/` é um caminho absoluto (`/usr/local/bin`, `//example.com`).
    if raw_host.is_empty() {
        return if path_and_rest.starts_with('/') { AddressBarInputKind::Url } else { AddressBarInputKind::Query };
    }

    // Canonicaliza o host pelo parser WHATWG; se não der, é consulta.
    let Some(host) = to_ascii_host(raw_host) else {
        return AddressBarInputKind::Query;
    };

    // Um literal IPv6 entre colchetes já canonicalizado é URL.
    if host.starts_with('[') && host.ends_with(']') {
        return AddressBarInputKind::Url;
    }

    if !has_only_host_chars(&host) {
        return AddressBarInputKind::Query;
    }

    // IPv4 com quatro grupos.
    if let Some(octets) = parse_ipv4_octets(&host)
        && octets.iter().all(|&octet| octet <= 255)
    {
        let all_zero = octets.iter().all(|&octet| octet == 0);
        if octets[0] != 0 || all_zero {
            return AddressBarInputKind::Url;
        }
        // Primeiro octeto zero e não é 0.0.0.0: "IP de origem", não navegável.
        return AddressBarInputKind::Query;
    }

    // Espaço no userinfo (`dep missing: @test/`) não são credenciais de verdade, então não é URL, salvo
    // esquema http(s) explícito, que vence. Fica de propósito DEPOIS dos retornos de IPv4/IPv6: o Chromium
    // classifica hosts IP como URL antes dessa heurística, então `user name@127.0.0.1` e `user name@[::1]`
    // continuam URLs.
    if userinfo.is_some_and(has_whitespace) && !is_http_scheme {
        return AddressBarInputKind::Unknown;
    }

    if host.eq_ignore_ascii_case("localhost") {
        return AddressBarInputKind::Url;
    }

    // Esquema http/https explícito com host que o parser aceitou: pronto.
    if is_http_scheme {
        return AddressBarInputKind::Url;
    }

    // Barra no fim do caminho força URL (atalho de intranet).
    if path_and_rest.ends_with('/') || path_and_rest.ends_with('\\') {
        return AddressBarInputKind::Url;
    }

    if port.is_some() {
        return AddressBarInputKind::Url;
    }

    if has_known_tld(&host) {
        return AddressBarInputKind::Url;
    }

    if userinfo.is_some() {
        return AddressBarInputKind::Unknown;
    }

    let has_path = path_and_rest.starts_with('/');
    let has_query = path_and_rest.contains('?');
    let has_fragment = path_and_rest.contains('#');
    let non_host_components = usize::from(has_path) + usize::from(has_query) + usize::from(has_fragment);
    if non_host_components > 1 {
        return AddressBarInputKind::Url;
    }

    AddressBarInputKind::Unknown
}

/// O motor com o id dado, ou o primeiro da lista (o padrão, Bing) para um id desconhecido.
fn engine_or_default(engine_id: &str) -> BrowserSearchEngine {
    let mut engines = browser_search_engines();
    let index = engines.iter().position(|engine| engine.id.as_str() == engine_id).unwrap_or(0);
    engines.swap_remove(index)
}

/// Monta a URL de busca da consulta com o motor indicado. A consulta é aparada e tem os espaços internos
/// colapsados antes de codificar.
pub fn build_search_url(query: &str, engine_id: &str) -> String {
    let mut collapsed = String::new();
    let mut in_whitespace = false;
    for c in js_trim(query).chars() {
        if is_ws(c) {
            if !in_whitespace {
                collapsed.push(' ');
            }
            in_whitespace = true;
        } else {
            collapsed.push(c);
            in_whitespace = false;
        }
    }
    engine_or_default(engine_id).build_search_url(&collapsed)
}

/// Rótulo legível do motor (por exemplo "Bing"). Um id desconhecido cai no rótulo do motor padrão.
pub fn get_browser_search_engine_label(engine_id: &str) -> String {
    engine_or_default(engine_id).label
}
