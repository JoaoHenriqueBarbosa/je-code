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
//! Não portado: `fetchDynamicRegistration`, `fetchResourceMetadata` e `fetchAuthorizationServerMetadata` (e as
//! interfaces `IFetcher`, `IFetchResourceMetadataOptions` e `IFetchAuthorizationServerMetadataOptions` deles),
//! que fazem requisições HTTP com `fetch`; a parte de `oauth.test.ts` que as cobre (suítes `Network Functions`,
//! `Client ID Fallback Scenarios`, `fetchResourceMetadata` e `fetchAuthorizationServerMetadata`) fica pendente.

use crate::buffer::decode_base64;
use crate::errors::Error;
use crate::json::{self, Value};
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
