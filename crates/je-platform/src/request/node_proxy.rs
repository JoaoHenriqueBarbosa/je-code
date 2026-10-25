//! Porte de `src/vs/platform/request/node/proxy.ts`: a escolha do proxy (`http.proxy` ou as variáveis `HTTP_PROXY`,
//! `HTTPS_PROXY` e minúsculas) e o "agent" que o `nodeRequest` entrega ao cliente HTTP.
//!
//! Decisões de tradução:
//! - O `Agent` do original é `any` (um `HttpProxyAgent` ou `HttpsProxyAgent` do pacote `http-proxy-agent` e
//!   `https-proxy-agent`). Aqui é o [`ProxyAgent`]: os dados que esses agents recebem (a URL do proxy, o host, a porta,
//!   a autenticação, o `rejectUnauthorized`) e qual dos dois seria criado. Quem faz a conexão pelo proxy é o cliente
//!   HTTP do `node_request_service`.
//! - O `url.parse` legado do Node é o [`parse_url`], sobre o parser WHATWG do crate `url` (o mesmo de `url::whatwg`):
//!   devolve só os campos que o original lê (`protocol`, `hostname`, `port`, `path`, `auth`).
//! - Fiel ao original, a porta padrão do proxy é sempre 80: o `proxyEndpoint.protocol === 'https'` do original compara o
//!   protocolo, que no `url.parse` termina em `:`, com `'https'`, então nunca é verdadeiro.

use je_base::process::ProcessEnvironment;
use je_base::promise::Promise;

/// `Agent` (`any` no original): o proxy pelo qual a requisição passa.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyAgent {
    /// A URL do proxy como veio da configuração ou do ambiente (o primeiro argumento do agent).
    pub proxy_url: String,
    /// `host: proxyEndpoint.hostname || ''`.
    pub host: String,
    pub port: u16,
    /// `auth: proxyEndpoint.auth`, já decodificado (`usuario:senha`).
    pub auth: Option<String>,
    /// `rejectUnauthorized: isBoolean(options.strictSSL) ? options.strictSSL : true`.
    pub reject_unauthorized: bool,
    /// `true` para o `HttpsProxyAgent` (requisição `https:`), `false` para o `HttpProxyAgent` (requisição `http:`).
    pub for_https_request: bool,
}

pub type Agent = ProxyAgent;

/// `IOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IOptions {
    pub proxy_url: Option<String>,
    pub strict_ssl: Option<bool>,
}

/// O resultado do `url.parse` do Node, só com o que o porte usa. Uma entrada que não é uma URL absoluta (`/caminho`)
/// fica só com `path`, como no `url.parse`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedUrl {
    /// Minúsculo e com `:` no fim (`http:`).
    pub protocol: Option<String>,
    /// Sem os colchetes de um IPv6.
    pub hostname: Option<String>,
    pub port: Option<String>,
    /// O caminho com a consulta (`/a/b?c=d`), sem o fragmento.
    pub path: Option<String>,
    /// `usuario:senha` decodificado.
    pub auth: Option<String>,
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = |byte: u8| char::from(byte).to_digit(16);
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2]))
        {
            out.push((high * 16 + low) as u8);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `url.parse(raw)`.
pub fn parse_url(raw: &str) -> ParsedUrl {
    let Ok(url) = ::url::Url::parse(raw.trim()) else {
        return ParsedUrl { path: Some(raw.to_owned()), ..ParsedUrl::default() };
    };
    let username = percent_decode(url.username());
    let auth = match url.password() {
        Some(password) => Some(format!("{username}:{}", percent_decode(password))),
        None if !username.is_empty() => Some(username),
        None => None,
    };
    let path = match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    };
    ParsedUrl {
        protocol: Some(format!("{}:", url.scheme())),
        hostname: url.host_str().map(|host| host.trim_start_matches('[').trim_end_matches(']').to_owned()).filter(|host| !host.is_empty()),
        port: url.port().map(|port| port.to_string()),
        path: Some(path),
        auth,
    }
}

/// O primeiro valor não vazio entre as variáveis (o `a || b || null` do original).
fn first_non_empty(env: &ProcessEnvironment, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| env.get(*name).filter(|value| !value.is_empty()).cloned())
}

fn get_system_proxy_uri(request_url: &ParsedUrl, env: &ProcessEnvironment) -> Option<String> {
    match request_url.protocol.as_deref() {
        Some("http:") => first_non_empty(env, &["HTTP_PROXY", "http_proxy"]),
        Some("https:") => first_non_empty(env, &["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"]),
        _ => None,
    }
}

/// `getProxyAgent`: o agent do proxy configurado (`options.proxyUrl`) ou, na falta dele, o do ambiente; `None` (o
/// `null` do original) se não há proxy ou ele não é `http:` nem `https:`.
pub fn get_proxy_agent(raw_request_url: &str, env: &ProcessEnvironment, options: &IOptions) -> Promise<Option<Agent>> {
    let request_url = parse_url(raw_request_url);
    let proxy_url = options.proxy_url.clone().filter(|url| !url.is_empty()).or_else(|| get_system_proxy_uri(&request_url, env));
    let Some(proxy_url) = proxy_url else {
        return Promise::resolved(None);
    };

    let proxy_endpoint = parse_url(&proxy_url);
    if !matches!(proxy_endpoint.protocol.as_deref(), Some("http:" | "https:")) {
        return Promise::resolved(None);
    }

    // `(proxyEndpoint.port ? +proxyEndpoint.port : 0) || (proxyEndpoint.protocol === 'https' ? 443 : 80)`: um `+port` que
    // não é número (NaN) ou é zero cai no padrão, que (ver o topo) é sempre 80.
    let port = proxy_endpoint.port.as_deref().and_then(|port| port.parse::<u16>().ok()).filter(|port| *port != 0).unwrap_or(80);
    Promise::resolved(Some(ProxyAgent {
        proxy_url,
        host: proxy_endpoint.hostname.unwrap_or_default(),
        port,
        auth: proxy_endpoint.auth,
        reject_unauthorized: options.strict_ssl.unwrap_or(true),
        for_https_request: request_url.protocol.as_deref() != Some("http:"),
    }))
}
