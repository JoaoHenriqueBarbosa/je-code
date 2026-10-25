//! Porte de `src/vs/base/common/oauth.ts` (a parte sem rede): as constantes e os tipos do OAuth 2.x
//! (RFC 6749, 7591, 8414, 9728, 8628), as funções `is*` que reconhecem as respostas, o `WWW-Authenticate`, os
//! claims de um JWT, a comparação de escopos e o corpo do Cross App Access (ID-JAG).
//!
//! Decisões de tradução:
//! - Os valores `unknown` que as funções `is*` recebem são [`Value`] (o JSON do porte); `undefined` e `null`
//!   são os dois [`Value::Null`], porque as duas são "não é um objeto" para todas elas.
//! - `isAuthorizationServerMetadata` lança erros em vez de devolver `false` para um objeto com campo inválido;
//!   aqui devolve `Result<bool, Error>`.
//! - `URL` do WHATWG é `&str`; [`get_default_metadata_for_url`] resolve `/authorize`, `/token` e `/register`
//!   contra o servidor e escreve o `issuer` com a barra final do caminho vazio, sem os outros ajustes do WHATWG
//!   (porta padrão, punycode).
//! - `URLSearchParams` é [`UrlSearchParams`], com a codificação `application/x-www-form-urlencoded`.
//!
//! - O `fetch` (o global e o injetável `IFetcher`) é o trait [`IFetcher`], e a `Response` é [`ICommonResponse`]; o
//!   `fetch` global do JavaScript é o que [`set_global_fetch`] instala (o porte não tem pilha HTTP em `je-base`, e
//!   sem um instalado a chamada falha com `ReferenceError: fetch is not defined`). `Promise` é `async fn` e a
//!   exceção é `Err(Error)`; o `AggregateError` é [`Error::aggregate`].
//! - `new URL(texto)` é [`WhatwgUrl`], o mínimo para `http(s)`: `origin`, `pathname` e `toString` com o host em
//!   minúsculas, a porta padrão omitida e a barra do caminho vazio (sem a normalização de percent-encoding e de `.`
//!   e `..` do WHATWG).
//! - `fetchAuthorizationServerMetadata` e `fetchResourceMetadata` devolvem os metadados já tipados; um corpo que
//!   passa no `is*` mas não cabe no tipo (por exemplo sem `response_types_supported`, que o TypeScript não confere)
//!   conta como "não é um metadado" no primeiro e como erro no segundo.

use std::cell::RefCell;
use std::rc::Rc;

use crate::buffer::decode_base64;
use crate::errors::Error;
use crate::json::{self, Value};
use crate::json_formatter::json_stringify;
use crate::node::pfs::LocalFuture;
use crate::record::JsNumber;
use crate::uri::{Field, URI, UriChange};


const WELL_KNOWN_ROUTE: &str = "/.well-known";
pub const AUTH_PROTECTED_RESOURCE_METADATA_DISCOVERY_PATH: &str = "/.well-known/oauth-protected-resource";
pub const AUTH_SERVER_METADATA_DISCOVERY_PATH: &str = "/.well-known/oauth-authorization-server";
pub const OPENID_CONNECT_DISCOVERY_PATH: &str = "/.well-known/openid-configuration";
pub const AUTH_SCOPE_SEPARATOR: &str = " ";

/// O `well-known` base dos caminhos de descoberta.
pub const fn well_known_route() -> &'static str {
    WELL_KNOWN_ROUTE
}

/// RFC 8693, `grant_type` do Token Exchange.
pub const GRANT_TYPE_TOKEN_EXCHANGE: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
pub const TOKEN_TYPE_ACCESS_TOKEN: &str = "urn:ietf:params:oauth:token-type:access_token";
pub const TOKEN_TYPE_ID_TOKEN: &str = "urn:ietf:params:oauth:token-type:id_token";
/// Cross App Access (draft-ietf-oauth-identity-assertion-authz-grant): o tipo do token ID-JAG.
pub const TOKEN_TYPE_ID_JAG: &str = "urn:ietf:params:oauth:token-type:id-jag";
/// RFC 7523, `grant_type` do JWT Bearer.
pub const GRANT_TYPE_JWT_BEARER: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

/// Default port for the authorization flow. We try to use this port so that the redirect URI does not change
/// when running on localhost.
pub const DEFAULT_AUTH_FLOW_PORT: u16 = 33418;

/// `URLSearchParams`: os pares na ordem de inserção, escritos como `application/x-www-form-urlencoded`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UrlSearchParams {
    pairs: Vec<(String, String)>,
}

impl UrlSearchParams {
    pub fn new() -> Self {
        Self::default()
    }

    /// `new URLSearchParams(init)` com um texto: o `?` inicial é ignorado e cada par `nome=valor` separado por `&`
    /// é lido como `application/x-www-form-urlencoded` (`+` vira espaço, `%XX` vira o byte, UTF-8 com perda).
    pub fn parse(query: &str) -> Self {
        let query = query.strip_prefix('?').unwrap_or(query);
        let pairs = query
            .split('&')
            .filter(|sequence| !sequence.is_empty())
            .map(|sequence| {
                let (name, value) = sequence.split_once('=').unwrap_or((sequence, ""));
                (form_decode(name), form_decode(value))
            })
            .collect();
        Self { pairs }
    }

    pub fn append(&mut self, name: &str, value: &str) {
        self.pairs.push((name.to_owned(), value.to_owned()));
    }

    /// `get`: o primeiro valor do nome.
    /// `getAll(name)`: todos os valores do nome, na ordem em que aparecem.
    pub fn get_all(&self, name: &str) -> Vec<&str> {
        self.pairs.iter().filter(|(key, _)| key == name).map(|(_, value)| value.as_str()).collect()
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.pairs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    pub fn has(&self, name: &str) -> bool {
        self.pairs.iter().any(|(n, _)| n == name)
    }

    /// `toString`.
    pub fn to_query_string(&self) -> String {
        self.pairs.iter().map(|(n, v)| format!("{}={}", form_encode(n), form_encode(v))).collect::<Vec<_>>().join("&")
    }
}

/// A codificação de formulário do WHATWG: `*-._` e alfanuméricos ASCII ficam, o espaço vira `+`, o resto vai em
/// `%XX` sobre os bytes UTF-8.
fn form_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'*' | b'-' | b'.' | b'_' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' => out.push(byte as char),
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// A leitura inversa de [`form_encode`]: `+` vira espaço e `%XX` vira o byte; um `%` sem dois dígitos
/// hexadecimais fica como está. Os bytes são lidos como UTF-8 com perda.
fn form_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let hex_value = |byte: Option<&u8>| byte.and_then(|b| char::from(*b).to_digit(16)).and_then(|digit| u8::try_from(digit).ok());
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => match (hex_value(bytes.get(i + 1)), hex_value(bytes.get(i + 2))) {
                (Some(high), Some(low)) => {
                    out.push(high * 16 + low);
                    i += 3;
                }
                _ => {
                    out.push(b'%');
                    i += 1;
                }
            },
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `buildIdJagExchangeBody`.
pub fn build_id_jag_exchange_body(client_id: &str, client_secret: Option<&str>, id_token: &str, audience: &str, resource: Option<&str>, scopes: &[&str]) -> UrlSearchParams {
    let mut body = UrlSearchParams::new();
    body.append("client_id", client_id);
    if let Some(secret) = client_secret.filter(|s| !s.is_empty()) {
        body.append("client_secret", secret);
    }
    body.append("grant_type", GRANT_TYPE_TOKEN_EXCHANGE);
    body.append("subject_token", id_token);
    body.append("subject_token_type", TOKEN_TYPE_ID_TOKEN);
    body.append("requested_token_type", TOKEN_TYPE_ID_JAG);
    body.append("audience", audience);
    if let Some(resource) = resource.filter(|r| !r.is_empty()) {
        body.append("resource", resource);
    }
    if !scopes.is_empty() {
        body.append("scope", &scopes.join(AUTH_SCOPE_SEPARATOR));
    }
    body
}

/// `buildResourceRedemptionBody`.
pub fn build_resource_redemption_body(client_id: &str, client_secret: Option<&str>, id_jag: &str, resource: Option<&str>, scopes: &[&str]) -> UrlSearchParams {
    let mut body = UrlSearchParams::new();
    body.append("client_id", client_id);
    if let Some(secret) = client_secret.filter(|s| !s.is_empty()) {
        body.append("client_secret", secret);
    }
    body.append("grant_type", GRANT_TYPE_JWT_BEARER);
    body.append("assertion", id_jag);
    if let Some(resource) = resource.filter(|r| !r.is_empty()) {
        body.append("resource", resource);
    }
    if !scopes.is_empty() {
        body.append("scope", &scopes.join(AUTH_SCOPE_SEPARATOR));
    }
    body
}

//#region types

/// `AuthorizationErrorType`.
pub mod authorization_error_type {
    pub const INVALID_REQUEST: &str = "invalid_request";
    pub const INVALID_CLIENT: &str = "invalid_client";
    pub const INVALID_GRANT: &str = "invalid_grant";
    pub const UNAUTHORIZED_CLIENT: &str = "unauthorized_client";
    pub const UNSUPPORTED_GRANT_TYPE: &str = "unsupported_grant_type";
    pub const INVALID_SCOPE: &str = "invalid_scope";
}

/// `AuthorizationDeviceCodeErrorType`.
pub mod authorization_device_code_error_type {
    pub const AUTHORIZATION_PENDING: &str = "authorization_pending";
    pub const SLOW_DOWN: &str = "slow_down";
    pub const ACCESS_DENIED: &str = "access_denied";
    pub const EXPIRED_TOKEN: &str = "expired_token";
}

/// `AuthorizationRegistrationErrorType`.
pub mod authorization_registration_error_type {
    pub const INVALID_REDIRECT_URI: &str = "invalid_redirect_uri";
    pub const INVALID_CLIENT_METADATA: &str = "invalid_client_metadata";
    pub const INVALID_SOFTWARE_STATEMENT: &str = "invalid_software_statement";
    pub const UNAPPROVED_SOFTWARE_STATEMENT: &str = "unapproved_software_statement";
}

record! {
    @rename "snake_case";
    /// `IAuthorizationProtectedResourceMetadata`: Metadata for an OAuth 2.0 Protected Resource (RFC 9728).
    pub struct IAuthorizationProtectedResourceMetadata {
        req { resource: String }
        opt {
            resource_name: String,
            authorization_servers: Vec<String>,
            jwks_uri: String,
            scopes_supported: Vec<String>,
            bearer_methods_supported: Vec<String>,
            resource_signing_alg_values_supported: Vec<String>,
            resource_encryption_alg_values_supported: Vec<String>,
            resource_encryption_enc_values_supported: Vec<String>,
            resource_documentation: String,
            resource_policy_uri: String,
            resource_tos_uri: String,
        }
    }
}

record! {
    @rename "snake_case";
    /// `IAuthorizationServerMetadata`: OAuth 2.0 Authorization Server Metadata (RFC 8414).
    pub struct IAuthorizationServerMetadata {
        req {
            issuer: String,
            response_types_supported: Vec<String>,
        }
        opt {
            authorization_endpoint: String,
            token_endpoint: String,
            device_authorization_endpoint: String,
            jwks_uri: String,
            registration_endpoint: String,
            scopes_supported: Vec<String>,
            response_modes_supported: Vec<String>,
            grant_types_supported: Vec<String>,
            token_endpoint_auth_methods_supported: Vec<String>,
            token_endpoint_auth_signing_alg_values_supported: Vec<String>,
            service_documentation: String,
            ui_locales_supported: Vec<String>,
            op_policy_uri: String,
            op_tos_uri: String,
            revocation_endpoint: String,
            revocation_endpoint_auth_methods_supported: Vec<String>,
            revocation_endpoint_auth_signing_alg_values_supported: Vec<String>,
            introspection_endpoint: String,
            introspection_endpoint_auth_methods_supported: Vec<String>,
            introspection_endpoint_auth_signing_alg_values_supported: Vec<String>,
            code_challenge_methods_supported: Vec<String>,
            client_id_metadata_document_supported: bool,
        }
    }
}

record! {
    @rename "snake_case";
    /// `IAuthorizationDynamicClientRegistrationRequest` (RFC 7591); as chaves que o tipo não declara
    /// (`[key: string]: unknown`) ficam em `extra`.
    pub struct IAuthorizationDynamicClientRegistrationRequest {
        opt {
            redirect_uris: Vec<String>,
            token_endpoint_auth_method: String,
            grant_types: Vec<String>,
            response_types: Vec<String>,
            client_name: String,
            client_uri: String,
            logo_uri: String,
            scope: String,
            contacts: Vec<String>,
            tos_uri: String,
            policy_uri: String,
            jwks_uri: String,
            jwks: serde_json::Value,
            software_id: String,
            software_version: String,
            software_statement: String,
            /// `'native' | 'web' | string`.
            application_type: String,
        }
    }
}

record! {
    @rename "snake_case";
    /// `IAuthorizationDynamicClientRegistrationResponse`.
    pub struct IAuthorizationDynamicClientRegistrationResponse {
        req { client_id: String }
        opt {
            client_secret: String,
            client_secret_expires_at: JsNumber,
            client_name: String,
            client_uri: String,
            redirect_uris: Vec<String>,
            grant_types: Vec<String>,
            response_types: Vec<String>,
            token_endpoint_auth_method: String,
        }
    }
}

record! {
    @rename "snake_case";
    /// `IAuthorizationAuthorizeResponse`.
    pub struct IAuthorizationAuthorizeResponse {
        req { code: String, state: String }
    }
}

record! {
    @rename "snake_case";
    /// `IAuthorizationAuthorizeErrorResponse`.
    pub struct IAuthorizationAuthorizeErrorResponse {
        req { error: String, state: String }
        opt { error_description: String, error_uri: String }
    }
}

record! {
    @rename "snake_case";
    /// `IAuthorizationTokenResponse`.
    pub struct IAuthorizationTokenResponse {
        req { access_token: String, token_type: String }
        opt { expires_in: JsNumber, refresh_token: String, scope: String, id_token: String }
    }
}

record! {
    @rename "snake_case";
    /// `IAuthorizationTokenErrorResponse`.
    pub struct IAuthorizationTokenErrorResponse {
        req { error: String }
        opt { error_description: String, error_uri: String }
    }
}

record! {
    @rename "snake_case";
    /// `IAuthorizationDeviceResponse` (RFC 8628).
    pub struct IAuthorizationDeviceResponse {
        req { device_code: String, user_code: String, verification_uri: String, expires_in: JsNumber }
        opt { verification_uri_complete: String, interval: JsNumber }
    }
}

record! {
    @rename "snake_case";
    /// `IAuthorizationErrorResponse` (e o `IAuthorizationDeviceTokenErrorResponse`, que só estreita `error`).
    pub struct IAuthorizationErrorResponse {
        req { error: String }
        opt { error_description: String, error_uri: String }
    }
}

record! {
    @rename "snake_case";
    /// `IAuthorizationRegistrationErrorResponse`.
    pub struct IAuthorizationRegistrationErrorResponse {
        req { error: String }
        opt { error_description: String }
    }
}

record! {
    @rename "snake_case";
    /// `IAuthorizationJWTClaims.address`.
    pub struct IAuthorizationJWTClaimsAddress {
        opt { formatted: String, street_address: String, locality: String, region: String, postal_code: String, country: String }
    }
}

/// `aud?: string | string[]`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Audience {
    One(String),
    Many(Vec<String>),
}

record! {
    @rename "snake_case";
    /// `IAuthorizationJWTClaims`: os claims de um JWT; as chaves desconhecidas ficam em `extra`.
    pub struct IAuthorizationJWTClaims {
        req { jti: String, sub: String, iss: String }
        opt {
            aud: Audience,
            exp: JsNumber,
            nbf: JsNumber,
            iat: JsNumber,
            azp: String,
            scope: String,
            name: String,
            given_name: String,
            family_name: String,
            middle_name: String,
            preferred_username: String,
            email: String,
            email_verified: bool,
            picture: String,
            auth_time: JsNumber,
            acr: String,
            amr: Vec<String>,
            sid: String,
            address: IAuthorizationJWTClaimsAddress,
            groups: Vec<String>,
            roles: Vec<String>,
            tid: String,
        }
    }
}

//#endregion

//#region is functions

/// `typeof obj === 'object' && obj !== null`, e a leitura da propriedade `name` (`undefined` se ausente; um
/// array não tem propriedades de objeto).
fn property<'a>(obj: &'a Value, name: &str) -> Option<&'a Value> {
    match obj {
        Value::Object(o) => o.get(name),
        _ => None,
    }
}

fn is_object(obj: &Value) -> bool {
    matches!(obj, Value::Object(_) | Value::Array(_))
}

/// A verdade do JavaScript para o JSON.
fn is_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => *n != 0.0 && !n.is_nan(),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `!== undefined`: a propriedade existe (um `null` conta).
fn is_defined(obj: &Value, name: &str) -> bool {
    property(obj, name).is_some()
}

/// `isAuthorizationProtectedResourceMetadata`.
pub fn is_authorization_protected_resource_metadata(obj: &Value) -> bool {
    if !is_object(obj) {
        return false;
    }
    if !is_truthy(property(obj, "resource")) {
        return false;
    }
    if property(obj, "scopes_supported").is_some_and(|v| !matches!(v, Value::Array(_))) {
        return false;
    }
    true
}

const URIS_TO_CHECK: [&str; 5] = ["issuer", "authorization_endpoint", "token_endpoint", "registration_endpoint", "jwks_uri"];

/// `isAuthorizationServerMetadata`. Lança (devolve `Err`) para um objeto sem `issuer` ou com um campo de URI
/// que não é uma string `http(s)://`.
pub fn is_authorization_server_metadata(obj: &Value) -> Result<bool, Error> {
    if !is_object(obj) {
        return Ok(false);
    }
    if !is_truthy(property(obj, "issuer")) {
        return Err(Error::new("Authorization server metadata must have an issuer"));
    }

    for uri in URIS_TO_CHECK {
        let value = property(obj, uri);
        if !is_truthy(value) {
            continue;
        }
        let Some(Value::String(text)) = value else {
            return Err(Error::new(format!("Authorization server metadata '{uri}' must be a string")));
        };
        let text = text.to_string_lossy();
        if !text.starts_with("https://") && !text.starts_with("http://") {
            return Err(Error::new(format!("Authorization server metadata '{uri}' must start with http:// or https://")));
        }
    }
    Ok(true)
}

/// `isAuthorizationDynamicClientRegistrationResponse`.
pub fn is_authorization_dynamic_client_registration_response(obj: &Value) -> bool {
    is_object(obj) && is_defined(obj, "client_id")
}

/// `isAuthorizationAuthorizeResponse`.
pub fn is_authorization_authorize_response(obj: &Value) -> bool {
    is_object(obj) && is_defined(obj, "code") && is_defined(obj, "state")
}

/// `isAuthorizationTokenResponse`.
pub fn is_authorization_token_response(obj: &Value) -> bool {
    is_object(obj) && is_defined(obj, "access_token") && is_defined(obj, "token_type")
}

/// `isAuthorizationDeviceResponse`.
pub fn is_authorization_device_response(obj: &Value) -> bool {
    is_object(obj) && is_defined(obj, "device_code") && is_defined(obj, "user_code") && is_defined(obj, "verification_uri") && is_defined(obj, "expires_in")
}

/// `isAuthorizationErrorResponse`.
pub fn is_authorization_error_response(obj: &Value) -> bool {
    is_object(obj) && is_defined(obj, "error")
}

/// `isAuthorizationRegistrationErrorResponse`.
pub fn is_authorization_registration_error_response(obj: &Value) -> bool {
    is_object(obj) && is_defined(obj, "error")
}

//#endregion

/// `getDefaultMetadataForUrl`: os endpoints padrão de um servidor de autorização sem metadados publicados.
pub fn get_default_metadata_for_url(authorization_server: &str) -> IAuthorizationServerMetadata {
    let server = URI::parse(authorization_server, false);
    let url = |path: &str| server.with(&UriChange { path: Field::from(path), query: Field::Clear, fragment: Field::Clear, ..UriChange::default() }).to_string(false).to_string_lossy();
    // `URL#toString` escreve a barra de um caminho vazio.
    let issuer = if server.path().is_empty() { url("/") } else { server.to_string(false).to_string_lossy() };
    let metadata = serde_json::json!({
        "issuer": issuer,
        "authorization_endpoint": url("/authorize"),
        "token_endpoint": url("/token"),
        "registration_endpoint": url("/register"),
        // Default values for Dynamic OpenID Providers: https://openid.net/specs/openid-connect-discovery-1_0.html
        "response_types_supported": ["code", "id_token", "id_token token"],
    });
    serde_json::from_value(metadata).expect("os campos obrigatórios estão presentes")
}

/// `IAuthenticationChallenge`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IAuthenticationChallenge {
    pub scheme: String,
    /// `params`, na ordem em que aparecem.
    pub params: Vec<(String, String)>,
}

impl IAuthenticationChallenge {
    /// `params[name]`.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// `params[key] = value`: repetir a chave troca o valor e mantém a posição.
    fn set(&mut self, key: &str, value: &str) {
        match self.params.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value.to_owned(),
            None => self.params.push((key.to_owned(), value.to_owned())),
        }
    }
}

/// `String.prototype.trim` para texto UTF-8.
fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() && c != '\u{85}' || c == '\u{feff}')
}

/// `.replace(/^"|"$/g, '')`: tira uma aspa do começo e uma do fim.
fn strip_quotes(text: &str) -> &str {
    let text = text.strip_prefix('"').unwrap_or(text);
    text.strip_suffix('"').unwrap_or(text)
}

/// `parseWWWAuthenticateHeader`: os desafios de um cabeçalho `WWW-Authenticate` (RFC 7235). Vários desafios se
/// separam por vírgula, e os parâmetros de um desafio também; a diferença é que o desafio novo começa por um
/// esquema (um token sem `=`).
pub fn parse_www_authenticate_header(www_authenticate_header_value: &str) -> Vec<IAuthenticationChallenge> {
    let mut challenges: Vec<IAuthenticationChallenge> = Vec::new();

    // First, split by commas while respecting quoted strings
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in www_authenticate_header_value.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
            current.push(ch);
        } else if ch == ',' && !in_quotes {
            if !js_trim(&current).is_empty() {
                tokens.push(js_trim(&current).to_owned());
            }
            current.clear();
        } else {
            current.push(ch);
        }
    }

    if !js_trim(&current).is_empty() {
        tokens.push(js_trim(&current).to_owned());
    }

    // Now process tokens to identify challenges. A challenge starts with a scheme name (a token that doesn't
    // contain '=' and is followed by parameters or is standalone)
    let mut current_challenge: Option<IAuthenticationChallenge> = None;

    for token in &tokens {
        if !token.contains('=') {
            // This token doesn't have '=', so it's likely a scheme name
            if let Some(challenge) = current_challenge.take() {
                challenges.push(challenge);
            }
            current_challenge = Some(IAuthenticationChallenge { scheme: js_trim(token).to_owned(), params: Vec::new() });
        } else {
            // This token has '=', it could be: 1. A parameter for the current challenge; 2. A new challenge that
            // starts with "Scheme param=value"
            if let Some(space_index) = token.find(' ').filter(|&i| i > 0) {
                let (before_space, after_space) = (&token[..space_index], &token[space_index + 1..]);

                // Check if what's before the space looks like a scheme name (no '=')
                if !before_space.contains('=') && after_space.contains('=') {
                    // This is a new challenge starting with "Scheme param=value"
                    if let Some(challenge) = current_challenge.take() {
                        challenges.push(challenge);
                    }
                    let mut challenge = IAuthenticationChallenge { scheme: js_trim(before_space).to_owned(), params: Vec::new() };

                    // Parse the parameter part
                    if let Some(equal_index) = after_space.find('=').filter(|&i| i > 0) {
                        let key = js_trim(&after_space[..equal_index]);
                        let value = strip_quotes(js_trim(&after_space[equal_index + 1..]));
                        if !key.is_empty() {
                            challenge.set(key, value);
                        }
                    }
                    current_challenge = Some(challenge);
                    continue;
                }
            }

            // This is a parameter for the current challenge
            if let Some(challenge) = current_challenge.as_mut()
                && let Some(equal_index) = token.find('=').filter(|&i| i > 0)
            {
                let key = js_trim(&token[..equal_index]);
                let value = strip_quotes(js_trim(&token[equal_index + 1..]));
                if !key.is_empty() {
                    challenge.set(key, value);
                }
            }
        }
    }

    // Don't forget the last challenge
    if let Some(challenge) = current_challenge {
        challenges.push(challenge);
    }

    challenges
}

/// `getClaimsFromJWT`: o payload de um JWT (sem verificar a assinatura). Devolve `Err` se o token não tem três
/// partes, ou se o cabeçalho ou o payload não são JSON de objeto; a mensagem da segunda falha começa por
/// `Failed to parse JWT token`.
pub fn get_claims_from_jwt(token: &str) -> Result<Value, Error> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(Error::new("Invalid JWT token format: token must have three parts separated by dots"));
    }

    let parse = |part: &str, what: &str| -> Result<Value, Error> {
        let decoded = decode_base64(part);
        let text = String::from_utf8_lossy(&decoded.to_vec()).into_owned();
        let mut errors = Vec::new();
        let value = json::parse_str(&text, &mut errors).filter(|_| errors.is_empty()).ok_or_else(|| Error::named("SyntaxError", format!("Unexpected token in JSON: {text}")))?;
        // `typeof value !== 'object'` (`null` e arrays são `object`).
        if !matches!(value, Value::Object(_) | Value::Array(_) | Value::Null) {
            return Err(Error::new(format!("Invalid JWT token format: {what} is not a JSON object")));
        }
        Ok(value)
    };

    let result = parse(parts[0], "header").and_then(|_| parse(parts[1], "payload"));
    result.map_err(|e| Error::new(format!("Failed to parse JWT token: {}", e.message)))
}

/// `scopesMatch`: as duas listas de escopos têm os mesmos escopos, em qualquer ordem? `None` é `undefined`.
pub fn scopes_match(scopes1: Option<&[&str]>, scopes2: Option<&[&str]>) -> bool {
    let (Some(scopes1), Some(scopes2)) = (scopes1, scopes2) else {
        return scopes1.is_none() && scopes2.is_none();
    };
    if scopes1.len() != scopes2.len() {
        return false;
    }

    // Sort both arrays for comparison to handle different orderings. O `sort()` do JavaScript compara por
    // unidades UTF-16.
    let sorted = |scopes: &[&str]| {
        let mut units: Vec<Vec<u16>> = scopes.iter().map(|s| s.encode_utf16().collect()).collect();
        units.sort();
        units
    };
    sorted(scopes1) == sorted(scopes2)
}

//#region fetch

/// `new URL(texto)` para `http(s)://`: o que `fetchResourceMetadata` e `fetchAuthorizationServerMetadata` leem
/// (`origin`, `pathname`, `toString`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhatwgUrl {
    scheme: String,
    userinfo: String,
    host: String,
    port: Option<String>,
    /// `pathname`: nunca vazio (`/` para o caminho vazio).
    pub pathname: String,
    search: String,
    hash: String,
}

impl WhatwgUrl {
    /// `new URL(input)`: `Err` com `TypeError: Invalid URL` quando não há `esquema://host`.
    pub fn parse(input: &str) -> Result<Self, Error> {
        let invalid = || Error::named("TypeError", "Invalid URL");
        let input = js_trim(input);
        let (scheme, rest) = input.split_once("://").ok_or_else(invalid)?;
        let valid_scheme = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        if !valid_scheme {
            return Err(invalid());
        }
        let scheme = scheme.to_ascii_lowercase();

        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(authority_end);
        let (userinfo, host_port) = match authority.rfind('@') {
            Some(at) => (format!("{}@", &authority[..at]), &authority[at + 1..]),
            None => (String::new(), authority),
        };
        let (host, port) = match host_port.rfind(':').filter(|&colon| !host_port[colon..].contains(']')) {
            Some(colon) => (&host_port[..colon], Some(&host_port[colon + 1..])),
            None => (host_port, None),
        };
        if host.is_empty() || port.is_some_and(|p| !p.chars().all(|c| c.is_ascii_digit())) {
            return Err(invalid());
        }
        let default_port = match scheme.as_str() {
            "http" | "ws" => Some("80"),
            "https" | "wss" => Some("443"),
            _ => None,
        };
        let port = port.filter(|p| !p.is_empty()).map(|p| p.trim_start_matches('0')).map(|p| if p.is_empty() { "0" } else { p }).filter(|p| Some(*p) != default_port).map(str::to_owned);

        let (before_hash, hash) = match tail.find('#') {
            Some(i) => (&tail[..i], &tail[i..]),
            None => (tail, ""),
        };
        let (path, search) = match before_hash.find('?') {
            Some(i) => (&before_hash[..i], &before_hash[i..]),
            None => (before_hash, ""),
        };
        let pathname = if path.is_empty() { "/".to_owned() } else { path.to_owned() };
        Ok(Self { scheme, userinfo, host: host.to_ascii_lowercase(), port, pathname, search: if search == "?" { String::new() } else { search.to_owned() }, hash: if hash == "#" { String::new() } else { hash.to_owned() } })
    }

    /// `origin`.
    pub fn origin(&self) -> String {
        match &self.port {
            Some(port) => format!("{}://{}:{port}", self.scheme, self.host),
            None => format!("{}://{}", self.scheme, self.host),
        }
    }

    /// `toString()` (`href`).
    pub fn href(&self) -> String {
        format!("{}://{}{}{}{}{}", self.scheme, self.userinfo, self.host_with_port(), self.pathname, self.search, self.hash)
    }

    fn host_with_port(&self) -> String {
        self.origin().split_once("://").map(|(_, host)| host.to_owned()).unwrap_or_default()
    }
}

/// `IFetcher` recebe `{ method, headers }`; o `fetch` global do `fetchDynamicRegistration` recebe também o `body`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct IFetchInit {
    pub method: String,
    /// `headers`, na ordem em que o objeto os escreve.
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

impl IFetchInit {
    /// `headers[name]`.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// `headers[name] = value`: repetir a chave troca o valor e mantém a posição (como num objeto).
fn set_header(headers: &mut Vec<(String, String)>, name: &str, value: &str) {
    match headers.iter_mut().find(|(k, _)| k == name) {
        Some(slot) => slot.1 = value.to_owned(),
        None => headers.push((name.to_owned(), value.to_owned())),
    }
}

/// `CommonResponse` (e, para o `fetchDynamicRegistration`, o `ok` da `Response`).
pub trait ICommonResponse {
    fn ok(&self) -> bool;
    fn status(&self) -> u16;
    fn status_text(&self) -> String;
    fn json(&self) -> LocalFuture<'_, Result<Value, Error>>;
    fn text(&self) -> LocalFuture<'_, Result<String, Error>>;
}

/// `IFetcher`: o `fetch`.
pub trait IFetcher {
    fn fetch(&self, input: &str, init: IFetchInit) -> LocalFuture<'static, Result<Rc<dyn ICommonResponse>, Error>>;
}

thread_local! {
    static GLOBAL_FETCH: RefCell<Option<Rc<dyn IFetcher>>> = const { RefCell::new(None) };
}

/// `globalThis.fetch = fetcher` (e `sandbox.restore()` com o valor anterior que ela devolve).
pub fn set_global_fetch(fetcher: Option<Rc<dyn IFetcher>>) -> Option<Rc<dyn IFetcher>> {
    GLOBAL_FETCH.with(|slot| slot.replace(fetcher))
}

struct UnavailableFetch;

impl IFetcher for UnavailableFetch {
    fn fetch(&self, _input: &str, _init: IFetchInit) -> LocalFuture<'static, Result<Rc<dyn ICommonResponse>, Error>> {
        Box::pin(async { Err(Error::named("ReferenceError", "fetch is not defined")) })
    }
}

/// `fetch: fetchImpl = fetch`.
fn resolve_fetch(custom: Option<Rc<dyn IFetcher>>) -> Rc<dyn IFetcher> {
    custom.or_else(|| GLOBAL_FETCH.with(|slot| slot.borrow().clone())).unwrap_or_else(|| Rc::new(UnavailableFetch))
}

/// `JSON.parse(texto)`: o valor, ou `None` se o texto não é JSON.
fn parse_json_text(text: &str) -> Option<Value> {
    let mut errors = Vec::new();
    json::parse_str(text, &mut errors).filter(|_| errors.is_empty())
}

/// O corpo que passou no `is*` lido como o tipo (as chaves desconhecidas vão para `extra`).
fn typed_body<T: serde::de::DeserializeOwned>(body: &Value) -> Result<T, Error> {
    serde_json::from_str(&json_stringify(body, None)).map_err(|e| Error::new(e.to_string()))
}

/// A `grantTypesSupported` do original: os grant types que o VS Code implementa.
const GRANT_TYPES_SUPPORTED: [&str; 3] = ["authorization_code", "refresh_token", "urn:ietf:params:oauth:grant-type:device_code"];

/// `fetchDynamicRegistration`: registra o cliente no `registration_endpoint` com o `fetch` global.
pub async fn fetch_dynamic_registration(server_metadata: &IAuthorizationServerMetadata, client_name: &str, scopes: Option<&[&str]>) -> Result<IAuthorizationDynamicClientRegistrationResponse, Error> {
    let Some(registration_endpoint) = server_metadata.registration_endpoint.as_deref().filter(|e| !e.is_empty()) else {
        return Err(Error::new("Server does not support dynamic registration"));
    };

    let to_strings = |items: &[&str]| items.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let request_body = IAuthorizationDynamicClientRegistrationRequest {
        client_name: Some(client_name.to_owned()),
        client_uri: Some("https://code.visualstudio.com".to_owned()),
        grant_types: Some(match &server_metadata.grant_types_supported {
            Some(supported) => supported.iter().filter(|gt| GRANT_TYPES_SUPPORTED.contains(&gt.as_str())).cloned().collect(),
            None => to_strings(&GRANT_TYPES_SUPPORTED),
        }),
        response_types: Some(to_strings(&["code"])),
        redirect_uris: Some(vec![
            "https://insiders.vscode.dev/redirect".to_owned(),
            "https://vscode.dev/redirect".to_owned(),
            "http://127.0.0.1/".to_owned(),
            // Added these for any server that might do only exact match on the redirect URI even though the spec says it
            // should not care about the port.
            format!("http://127.0.0.1:{DEFAULT_AUTH_FLOW_PORT}/"),
        ]),
        scope: scopes.map(|scopes| scopes.join(AUTH_SCOPE_SEPARATOR)),
        token_endpoint_auth_method: Some("none".to_owned()),
        application_type: Some("native".to_owned()),
        logo_uri: None,
        contacts: None,
        tos_uri: None,
        policy_uri: None,
        jwks_uri: None,
        jwks: None,
        software_id: None,
        software_version: None,
        software_statement: None,
        extra: Default::default(),
    };

    let body = serde_json::to_string(&request_body).map_err(|e| Error::new(e.to_string()))?;
    let init = IFetchInit { method: "POST".to_owned(), headers: vec![("Content-Type".to_owned(), "application/json".to_owned())], body: Some(body) };
    let response = resolve_fetch(None).fetch(registration_endpoint, init).await?;

    if !response.ok() {
        let result = response.text().await?;
        let mut error_details = result.clone();

        if let Some(error_response) = parse_json_text(&result)
            && is_authorization_registration_error_response(&error_response)
        {
            let field = |name: &str| match property(&error_response, name) {
                Some(Value::String(s)) => s.to_string_lossy(),
                Some(other) => json_stringify(other, None),
                None => "undefined".to_owned(),
            };
            error_details = field("error");
            if is_truthy(property(&error_response, "error_description")) {
                error_details = format!("{error_details}: {}", field("error_description"));
            }
        }

        return Err(Error::new(format!("Registration to {registration_endpoint} failed: {error_details}")));
    }

    let registration = response.json().await?;
    if is_authorization_dynamic_client_registration_response(&registration) {
        return typed_body(&registration);
    }
    Err(Error::new(format!("Invalid authorization dynamic client registration response: {}", json_stringify(&registration, None))))
}

/// `{ metadata, discoveryUrl, errors }`.
#[derive(Clone, Debug, PartialEq)]
pub struct IDiscoveryResult<T> {
    pub metadata: T,
    pub discovery_url: String,
    pub errors: Vec<Error>,
}

/// `IFetchResourceMetadataOptions`.
#[derive(Clone, Default)]
pub struct IFetchResourceMetadataOptions {
    /// Headers to include only when the resource metadata URL has the same origin as the target resource.
    pub same_origin_headers: Vec<(String, String)>,
    /// Optional custom fetch implementation (defaults to global fetch).
    pub fetch: Option<Rc<dyn IFetcher>>,
}

/// `fetchPrm`, a função interna de `fetchResourceMetadata`.
async fn fetch_prm(
    fetch: &Rc<dyn IFetcher>,
    target_origin: &str,
    same_origin_headers: &[(String, String)],
    prm_url: &str,
    validate_url: &str,
) -> Result<IAuthorizationProtectedResourceMetadata, Error> {
    // Determine if we should include same-origin headers
    let mut headers = vec![("Accept".to_owned(), "application/json".to_owned())];

    let resource_metadata_url = WhatwgUrl::parse(prm_url)?;
    if resource_metadata_url.origin() == target_origin {
        for (name, value) in same_origin_headers {
            set_header(&mut headers, name, value);
        }
    }

    let response = fetch.fetch(prm_url, IFetchInit { method: "GET".to_owned(), headers, body: None }).await?;
    if response.status() != 200 {
        let error_text = match response.text().await {
            Ok(text) => text,
            Err(_) => response.status_text(),
        };
        return Err(Error::new(format!("Failed to fetch resource metadata from {prm_url}: {} {error_text}", response.status())));
    }

    let body = response.json().await?;
    if is_authorization_protected_resource_metadata(&body) {
        // Validate that the resource matches the target resource. Use URL constructor for normalization - it handles
        // hostname case and trailing slashes
        let resource = match property(&body, "resource") {
            Some(Value::String(s)) => s.to_string_lossy(),
            _ => return Err(Error::named("TypeError", "Invalid URL")),
        };
        let prm_value = WhatwgUrl::parse(&resource)?.href();
        let expected_resource = WhatwgUrl::parse(validate_url)?.href();
        if prm_value != expected_resource {
            return Err(Error::new(format!("Protected Resource Metadata 'resource' property value \"{prm_value}\" does not match expected value \"{expected_resource}\" for URL {prm_url}. Per RFC 9728, these MUST match. See https://datatracker.ietf.org/doc/html/rfc9728#PRConfigurationValidation")));
        }
        typed_body(&body)
    } else {
        Err(Error::new(format!("Invalid resource metadata from {prm_url}. Expected to follow shape of https://datatracker.ietf.org/doc/html/rfc9728#name-protected-resource-metadata (Hints: is scopes_supported an array? Is resource a string?). Current payload: {}", json_stringify(&body, None))))
    }
}

/// `fetchResourceMetadata`: busca e valida os metadados de recurso protegido (RFC 9728), primeiro na URL dada, depois
/// no `well-known` com o caminho acrescentado e por fim no da raiz. `Err` é o erro único, ou o `AggregateError` de
/// todas as tentativas.
pub async fn fetch_resource_metadata(target_resource: &str, resource_metadata_url: Option<&str>, options: IFetchResourceMetadataOptions) -> Result<IDiscoveryResult<IAuthorizationProtectedResourceMetadata>, Error> {
    let IFetchResourceMetadataOptions { same_origin_headers, fetch } = options;
    let fetch = resolve_fetch(fetch);

    let target = WhatwgUrl::parse(target_resource)?;
    let target_origin = target.origin();

    let mut errors: Vec<Error> = Vec::new();
    if let Some(url) = resource_metadata_url.filter(|u| !u.is_empty()) {
        match fetch_prm(&fetch, &target_origin, &same_origin_headers, url, target_resource).await {
            Ok(metadata) => return Ok(IDiscoveryResult { metadata, discovery_url: url.to_owned(), errors }),
            Err(e) => errors.push(e),
        }
    }

    // Try well-known URIs starting with path-appended, then root
    let has_path_component = target.pathname != "/";
    let root_url = format!("{target_origin}{AUTH_PROTECTED_RESOURCE_METADATA_DISCOVERY_PATH}");

    if has_path_component {
        let path_appended_url = format!("{root_url}{}", target.pathname);
        match fetch_prm(&fetch, &target_origin, &same_origin_headers, &path_appended_url, target_resource).await {
            Ok(metadata) => return Ok(IDiscoveryResult { metadata, discovery_url: path_appended_url, errors }),
            Err(e) => errors.push(e),
        }
    }

    // Finally, try root discovery
    match fetch_prm(&fetch, &target_origin, &same_origin_headers, &root_url, &target_origin).await {
        Ok(metadata) => return Ok(IDiscoveryResult { metadata, discovery_url: root_url, errors }),
        Err(e) => errors.push(e),
    }

    // If we've tried all methods and none worked, throw the error(s)
    Err(single_or_aggregate(errors, "Failed to fetch resource metadata from all attempted URLs"))
}

/// `if (errors.length === 1) throw errors[0]; else throw new AggregateError(errors, message)`.
fn single_or_aggregate(mut errors: Vec<Error>, message: &str) -> Error {
    if errors.len() == 1 { errors.remove(0) } else { Error::aggregate(errors, message) }
}

/// `IFetchAuthorizationServerMetadataOptions`.
#[derive(Clone, Default)]
pub struct IFetchAuthorizationServerMetadataOptions {
    /// Headers to include in the requests.
    pub additional_headers: Vec<(String, String)>,
    /// Optional custom fetch implementation (defaults to global fetch).
    pub fetch: Option<Rc<dyn IFetcher>>,
}

/// `getErrText`.
async fn get_err_text(response: &dyn ICommonResponse) -> String {
    match response.text().await {
        Ok(text) => text,
        Err(_) => response.status_text(),
    }
}

/// `tryParseAuthServerMetadata`.
async fn try_parse_auth_server_metadata(response: &dyn ICommonResponse) -> Option<IAuthorizationServerMetadata> {
    if response.status() != 200 {
        return None;
    }
    // Failed to parse as JSON or not valid metadata
    let body = response.json().await.ok()?;
    if !is_authorization_server_metadata(&body).ok()? {
        return None;
    }
    typed_body(&body).ok()
}

/// `doFetch`, a função interna de `fetchAuthorizationServerMetadata`: os erros vão para `errors`.
async fn fetch_auth_server_metadata_from(fetch: &Rc<dyn IFetcher>, additional_headers: &[(String, String)], url: &str, errors: &mut Vec<Error>) -> Option<IAuthorizationServerMetadata> {
    let mut headers = additional_headers.to_vec();
    set_header(&mut headers, "Accept", "application/json");
    match fetch.fetch(url, IFetchInit { method: "GET".to_owned(), headers, body: None }).await {
        Ok(response) => {
            if let Some(metadata) = try_parse_auth_server_metadata(response.as_ref()).await {
                return Some(metadata);
            }
            // No metadata found, collect error from response
            errors.push(Error::new(format!("Failed to fetch authorization server metadata from {url}: {} {}", response.status(), get_err_text(response.as_ref()).await)));
            None
        }
        Err(e) => {
            // Collect error from fetch failure
            errors.push(e);
            None
        }
    }
}

/// `fetchAuthorizationServerMetadata`: tenta o `oauth-authorization-server` com o caminho inserido (RFC 8414), o
/// `openid-configuration` com o caminho inserido e o `openid-configuration` com o caminho acrescentado.
pub async fn fetch_authorization_server_metadata(authorization_server: &str, options: IFetchAuthorizationServerMetadataOptions) -> Result<IDiscoveryResult<IAuthorizationServerMetadata>, Error> {
    let IFetchAuthorizationServerMetadataOptions { additional_headers, fetch } = options;
    let fetch = resolve_fetch(fetch);

    let authorization_server_url = WhatwgUrl::parse(authorization_server)?;
    let extra_path = if authorization_server_url.pathname == "/" { "" } else { authorization_server_url.pathname.as_str() };
    let origin = authorization_server_url.origin();

    let mut errors: Vec<Error> = Vec::new();

    // For the oauth server metadata discovery path, we _INSERT_ the well known path after the origin and before the path.
    // https://datatracker.ietf.org/doc/html/rfc8414#section-3
    let path_to_fetch = format!("{origin}{AUTH_SERVER_METADATA_DISCOVERY_PATH}{extra_path}");
    if let Some(metadata) = fetch_auth_server_metadata_from(&fetch, &additional_headers, &path_to_fetch, &mut errors).await {
        return Ok(IDiscoveryResult { metadata, discovery_url: path_to_fetch, errors });
    }

    // Try fetching the OpenID Connect Discovery with path insertion. For issuer URLs with path components, this inserts
    // the well-known path after the origin and before the path.
    let openid_path_insertion_url = format!("{origin}{OPENID_CONNECT_DISCOVERY_PATH}{extra_path}");
    if let Some(metadata) = fetch_auth_server_metadata_from(&fetch, &additional_headers, &openid_path_insertion_url, &mut errors).await {
        return Ok(IDiscoveryResult { metadata, discovery_url: openid_path_insertion_url, errors });
    }

    // Try fetching the other discovery URL. For the openid metadata discovery path, we _ADD_ the well known path after
    // the existing path. https://datatracker.ietf.org/doc/html/rfc8414#section-3
    let openid_path_addition_url = if authorization_server.ends_with('/') {
        // Remove leading slash if authServer ends with slash
        format!("{authorization_server}{}", &OPENID_CONNECT_DISCOVERY_PATH[1..])
    } else {
        format!("{authorization_server}{OPENID_CONNECT_DISCOVERY_PATH}")
    };
    if let Some(metadata) = fetch_auth_server_metadata_from(&fetch, &additional_headers, &openid_path_addition_url, &mut errors).await {
        return Ok(IDiscoveryResult { metadata, discovery_url: openid_path_addition_url, errors });
    }

    // If we've tried all URLs and none worked, throw the error(s)
    Err(single_or_aggregate(errors, "Failed to fetch authorization server metadata from all attempted URLs"))
}

//#endregion
