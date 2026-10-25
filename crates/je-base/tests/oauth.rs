//! Porte de `src/vs/base/test/common/oauth.test.ts` (suíte `OAuth`), caso a caso e na mesma ordem: `Type Guards`,
//! `Scope Matching`, `Utility Functions`, `Parsing Functions`, `Network Functions`, `Client ID Fallback Scenarios`,
//! `fetchResourceMetadata`, `fetchAuthorizationServerMetadata` e `Cross App Access (ID-JAG) wire format`.
//!
//! Notas de tradução:
//! - Os objetos literais do TypeScript são JSON lido por `v(...)`; `undefined` e `null` são os dois
//!   `Value::Null` (para todas as funções `is*` as duas são "não é um objeto"), e `'not an object'` é uma string.
//! - `assert.throws(fn, /texto/)` é o `Err` com a mensagem; `scopesMatch(undefined, ...)` é `None`.
//! - `getDefaultMetadataForUrl(new URL(...))` recebe o texto da URL.
//! - O `sinon.stub()` do `fetch` é o [`FetchStub`]: guarda as chamadas (`callCount`, `args`) e responde na ordem dos
//!   `onFirstCall`, `onSecondCall`, `onThirdCall`, ou sempre o mesmo (`resolves`/`rejects`). O `sandbox.stub(globalThis,
//!   'fetch')` é o `set_global_fetch`, desfeito pelo [`GlobalFetchGuard`] (o `sandbox.restore()`).
//! - O `Response` do mock é o [`MockResponse`]: `ok`, `status`, `statusText`, `json()` e `text()`; o que o mock do
//!   TypeScript não define é um `Err` (a chamada a uma função inexistente lança).
//! - O corpo do teste assíncrono roda em `run_with_real_timers`; `assert.rejects(fn, /texto/)` confere a mensagem do
//!   `Err`, e os trechos separados por `.*` na expressão regular viram `contains_in_order`.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::errors::Error;
use je_base::json::{self, Value};
use je_base::node::pfs::LocalFuture;
use je_base::oauth::*;

fn v(source: &str) -> Value {
    json::parse_str(source, &mut Vec::new()).expect("JSON válido")
}

fn text(s: &str) -> Value {
    Value::String(s.into())
}

fn server_metadata_error(source: &str) -> String {
    is_authorization_server_metadata(&v(source)).expect_err("devia lançar").message
}

/// O objeto tipado que o TypeScript escreve como literal.
fn typed<T: serde::de::DeserializeOwned>(source: &str) -> T {
    serde_json::from_str(source).expect("JSON do tipo")
}

/// `await assert.rejects(...)`: o erro da promessa rejeitada.
fn rejected<T>(result: Result<T, Error>) -> Error {
    match result {
        Ok(_) => panic!("devia rejeitar"),
        Err(error) => error,
    }
}

/// A expressão regular `/a.*b.*c/`: os trechos aparecem na mensagem nessa ordem.
fn contains_in_order(message: &str, parts: &[&str]) -> bool {
    let mut rest = message;
    for part in parts {
        match rest.find(part) {
            Some(i) => rest = &rest[i + part.len()..],
            None => return false,
        }
    }
    true
}

/// `{ ok, status, statusText, json, text }` do mock de `Response`.
#[derive(Clone)]
struct MockResponse {
    ok: bool,
    status: u16,
    status_text: String,
    json: Result<Value, Error>,
    text: Result<String, Error>,
}

impl MockResponse {
    fn new(ok: bool, status: u16) -> Self {
        let missing = |name: &str| Error::named("TypeError", format!("response.{name} is not a function"));
        Self { ok, status, status_text: String::new(), json: Err(missing("json")), text: Err(missing("text")) }
    }

    /// `{ status, json: async () => body, text: async () => JSON.stringify(body) }` (o corpo já vem compacto).
    fn with_body(status: u16, body: &str) -> Self {
        Self { json: Ok(v(body)), text: Ok(body.to_owned()), ..Self::new(status == 200, status) }
    }

    fn status_text(mut self, status_text: &str) -> Self {
        self.status_text = status_text.to_owned();
        self
    }

    fn json_body(mut self, body: &str) -> Self {
        self.json = Ok(v(body));
        self
    }

    fn json_throws(mut self, message: &str) -> Self {
        self.json = Err(Error::new(message));
        self
    }

    fn text_body(mut self, body: &str) -> Self {
        self.text = Ok(body.to_owned());
        self
    }

    fn text_throws(mut self, message: &str) -> Self {
        self.text = Err(Error::new(message));
        self
    }
}

impl ICommonResponse for MockResponse {
    fn ok(&self) -> bool {
        self.ok
    }

    fn status(&self) -> u16 {
        self.status
    }

    fn status_text(&self) -> String {
        self.status_text.clone()
    }

    fn json(&self) -> LocalFuture<'_, Result<Value, Error>> {
        Box::pin(std::future::ready(self.json.clone()))
    }

    fn text(&self) -> LocalFuture<'_, Result<String, Error>> {
        Box::pin(std::future::ready(self.text.clone()))
    }
}

/// O `sinon.stub()` do `fetch`.
#[derive(Default)]
struct FetchStub {
    calls: RefCell<Vec<(String, IFetchInit)>>,
    /// `onFirstCall`, `onSecondCall`, `onThirdCall`.
    scripted: RefCell<Vec<Option<Result<MockResponse, Error>>>>,
    /// `resolves`/`rejects` sem `onCall`.
    fallback: RefCell<Option<Result<MockResponse, Error>>>,
}

impl FetchStub {
    fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// `fetchStub.resolves(response)`.
    fn resolves(&self, response: MockResponse) {
        *self.fallback.borrow_mut() = Some(Ok(response));
    }

    /// `fetchStub.rejects(new Error(message))`.
    fn rejects(&self, message: &str) {
        *self.fallback.borrow_mut() = Some(Err(Error::new(message)));
    }

    fn on_call(&self, index: usize, reply: Result<MockResponse, Error>) {
        let mut scripted = self.scripted.borrow_mut();
        if scripted.len() <= index {
            scripted.resize(index + 1, None);
        }
        scripted[index] = Some(reply);
    }

    /// `fetchStub.onFirstCall().resolves(response)`.
    fn on_first_call_resolves(&self, response: MockResponse) {
        self.on_call(0, Ok(response));
    }

    /// `fetchStub.onSecondCall().resolves(response)`.
    fn on_second_call_resolves(&self, response: MockResponse) {
        self.on_call(1, Ok(response));
    }

    /// `fetchStub.onThirdCall().resolves(response)`.
    fn on_third_call_resolves(&self, response: MockResponse) {
        self.on_call(2, Ok(response));
    }

    /// `fetchStub.onFirstCall().rejects(new Error(message))`.
    fn on_first_call_rejects(&self, message: &str) {
        self.on_call(0, Err(Error::new(message)));
    }

    /// `fetchStub.callCount`.
    fn call_count(&self) -> usize {
        self.calls.borrow().len()
    }

    /// `fetchStub.getCall(index).args`.
    fn args(&self, index: usize) -> (String, IFetchInit) {
        self.calls.borrow()[index].clone()
    }

    /// `fetchStub.firstCall.args[0]`.
    fn url(&self, index: usize) -> String {
        self.args(index).0
    }

    /// `fetchStub.getCall(index).args[1]`.
    fn init(&self, index: usize) -> IFetchInit {
        self.args(index).1
    }
}

impl IFetcher for FetchStub {
    fn fetch(&self, input: &str, init: IFetchInit) -> LocalFuture<'static, Result<Rc<dyn ICommonResponse>, Error>> {
        let index = self.calls.borrow().len();
        self.calls.borrow_mut().push((input.to_owned(), init));
        let reply = self
            .scripted
            .borrow()
            .get(index)
            .cloned()
            .flatten()
            .or_else(|| self.fallback.borrow().clone())
            .unwrap_or_else(|| Err(Error::named("TypeError", "Cannot read properties of undefined")));
        Box::pin(std::future::ready(reply.map(|response| Rc::new(response) as Rc<dyn ICommonResponse>)))
    }
}

/// `sandbox.stub(globalThis, 'fetch')`, e o `sandbox.restore()` quando sai de escopo.
struct GlobalFetchGuard(Option<Rc<dyn IFetcher>>);

impl Drop for GlobalFetchGuard {
    fn drop(&mut self) {
        set_global_fetch(self.0.take());
    }
}

fn stub_global_fetch(stub: &Rc<FetchStub>) -> GlobalFetchGuard {
    GlobalFetchGuard(set_global_fetch(Some(stub.clone())))
}

/// O `fetch` injetável de `{ fetch: fetchStub }`.
fn injected(stub: &Rc<FetchStub>) -> Rc<dyn IFetcher> {
    stub.clone()
}

/// `setup()` com `fetchStub = sandbox.stub(globalThis, 'fetch')` e `teardown()` com `sandbox.restore()`.
fn run_with_global_stub<F: std::future::Future<Output = ()> + 'static>(body: impl FnOnce(Rc<FetchStub>) -> F) {
    let fetch_stub = FetchStub::new();
    let _global = stub_global_fetch(&fetch_stub);
    run_with_real_timers(body(fetch_stub));
}

/// `setup()` com `fetchStub = sandbox.stub()` (o `fetch` não é global).
fn run_with_stub<F: std::future::Future<Output = ()> + 'static>(body: impl FnOnce(Rc<FetchStub>) -> F) {
    run_with_real_timers(body(FetchStub::new()));
}

fn resource_options(stub: &Rc<FetchStub>) -> IFetchResourceMetadataOptions {
    IFetchResourceMetadataOptions { fetch: Some(injected(stub)), ..Default::default() }
}

fn headers_of(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(k, val)| ((*k).to_owned(), (*val).to_owned())).collect()
}

fn auth_server_options(stub: &Rc<FetchStub>) -> IFetchAuthorizationServerMetadataOptions {
    IFetchAuthorizationServerMetadataOptions { fetch: Some(injected(stub)), ..Default::default() }
}

mod type_guards {
    use super::*;

    #[test]
    fn is_authorization_protected_resource_metadata_should_correctly_identify_protected_resource_metadata() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        // Valid metadata with minimal required fields
        assert!(is_authorization_protected_resource_metadata(&v(r#"{"resource":"https://example.com"}"#)));

        // Valid metadata with scopes_supported as array
        assert!(is_authorization_protected_resource_metadata(&v(r#"{"resource":"https://example.com","scopes_supported":["read","write"]}"#)));

        // Invalid cases - missing resource
        assert!(!is_authorization_protected_resource_metadata(&Value::Null));
        assert!(!is_authorization_protected_resource_metadata(&Value::Null));
        assert!(!is_authorization_protected_resource_metadata(&v("{}")));
        assert!(!is_authorization_protected_resource_metadata(&text("not an object")));

        // Invalid cases - scopes_supported is not an array when provided
        assert!(!is_authorization_protected_resource_metadata(&v(r#"{"resource":"https://example.com","scopes_supported":"not an array"}"#)));
    }

    #[test]
    fn is_authorization_server_metadata_should_correctly_identify_server_metadata() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        let ok = |source: &str| is_authorization_server_metadata(&v(source)).unwrap();
        // Valid metadata with minimal required fields
        assert!(ok(r#"{"issuer":"https://example.com","response_types_supported":["code"]}"#));

        // Valid metadata with valid URLs
        assert!(ok(
            r#"{"issuer":"https://example.com","authorization_endpoint":"https://example.com/auth","token_endpoint":"https://example.com/token","registration_endpoint":"https://example.com/register","jwks_uri":"https://example.com/jwks","response_types_supported":["code"]}"#
        ));

        // Valid metadata with http URLs (for localhost/testing)
        assert!(ok(r#"{"issuer":"http://localhost:8080","authorization_endpoint":"http://localhost:8080/auth","token_endpoint":"http://localhost:8080/token","response_types_supported":["code"]}"#));

        // Invalid cases - not an object
        assert!(!is_authorization_server_metadata(&Value::Null).unwrap());
        assert!(!is_authorization_server_metadata(&Value::Null).unwrap());
        assert!(!is_authorization_server_metadata(&text("not an object")).unwrap());

        // Invalid cases - missing issuer should throw
        assert!(server_metadata_error("{}").contains("Authorization server metadata must have an issuer"));
        assert!(server_metadata_error(r#"{"response_types_supported":["code"]}"#).contains("Authorization server metadata must have an issuer"));

        // Invalid cases - URI fields must be strings when provided (truthy values)
        for (field, value) in [("authorization_endpoint", "123"), ("token_endpoint", "123"), ("registration_endpoint", "[]"), ("jwks_uri", "{}")] {
            let message = server_metadata_error(&format!(r#"{{"issuer":"https://example.com","{field}":{value},"response_types_supported":["code"]}}"#));
            assert!(message.contains(&format!("Authorization server metadata '{field}' must be a string")), "{message}");
        }

        // Invalid cases - URI fields must start with http:// or https://
        let message = server_metadata_error(r#"{"issuer":"ftp://example.com","response_types_supported":["code"]}"#);
        assert!(message.contains("Authorization server metadata 'issuer' must start with http:// or https://"), "{message}");
        for (field, value) in [("authorization_endpoint", "ftp://example.com/auth"), ("token_endpoint", "file:///path/to/token"), ("registration_endpoint", "mailto:admin@example.com"), ("jwks_uri", "data:application/json,{}")] {
            let message = server_metadata_error(&format!(r#"{{"issuer":"https://example.com","{field}":"{value}","response_types_supported":["code"]}}"#));
            assert!(message.contains(&format!("Authorization server metadata '{field}' must start with http:// or https://")), "{message}");
        }
    }

    #[test]
    fn is_authorization_dynamic_client_registration_response_should_correctly_identify_registration_response() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        // Valid response
        assert!(is_authorization_dynamic_client_registration_response(&v(r#"{"client_id":"client-123","client_name":"Test Client"}"#)));

        // Invalid cases
        assert!(!is_authorization_dynamic_client_registration_response(&Value::Null));
        assert!(!is_authorization_dynamic_client_registration_response(&Value::Null));
        assert!(!is_authorization_dynamic_client_registration_response(&v("{}")));
        assert!(is_authorization_dynamic_client_registration_response(&v(r#"{"client_id":"just-id"}"#)));
        assert!(!is_authorization_dynamic_client_registration_response(&v(r#"{"client_name":"missing-id"}"#)));
        assert!(!is_authorization_dynamic_client_registration_response(&text("not an object")));
    }

    #[test]
    fn is_authorization_authorize_response_should_correctly_identify_authorization_response() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        // Valid response
        assert!(is_authorization_authorize_response(&v(r#"{"code":"auth-code-123","state":"state-123"}"#)));

        // Invalid cases
        assert!(!is_authorization_authorize_response(&Value::Null));
        assert!(!is_authorization_authorize_response(&Value::Null));
        assert!(!is_authorization_authorize_response(&v("{}")));
        assert!(!is_authorization_authorize_response(&v(r#"{"code":"missing-state"}"#)));
        assert!(!is_authorization_authorize_response(&v(r#"{"state":"missing-code"}"#)));
        assert!(!is_authorization_authorize_response(&text("not an object")));
    }

    #[test]
    fn is_authorization_token_response_should_correctly_identify_token_response() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        // Valid response
        assert!(is_authorization_token_response(&v(r#"{"access_token":"token-123","token_type":"Bearer"}"#)));

        // Invalid cases
        assert!(!is_authorization_token_response(&Value::Null));
        assert!(!is_authorization_token_response(&Value::Null));
        assert!(!is_authorization_token_response(&v("{}")));
        assert!(!is_authorization_token_response(&v(r#"{"access_token":"missing-type"}"#)));
        assert!(!is_authorization_token_response(&v(r#"{"token_type":"missing-token"}"#)));
        assert!(!is_authorization_token_response(&text("not an object")));
    }

    #[test]
    fn is_authorization_device_response_should_correctly_identify_device_authorization_response() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        // Valid response
        assert!(is_authorization_device_response(&v(r#"{"device_code":"device-code-123","user_code":"ABCD-EFGH","verification_uri":"https://example.com/verify","expires_in":1800}"#)));

        // Valid response with optional fields
        assert!(is_authorization_device_response(&v(
            r#"{"device_code":"device-code-123","user_code":"ABCD-EFGH","verification_uri":"https://example.com/verify","verification_uri_complete":"https://example.com/verify?user_code=ABCD-EFGH","expires_in":1800,"interval":5}"#
        )));

        // Invalid cases
        assert!(!is_authorization_device_response(&Value::Null));
        assert!(!is_authorization_device_response(&Value::Null));
        assert!(!is_authorization_device_response(&v("{}")));
        assert!(!is_authorization_device_response(&v(r#"{"device_code":"missing-others"}"#)));
        assert!(!is_authorization_device_response(&v(r#"{"user_code":"missing-others"}"#)));
        assert!(!is_authorization_device_response(&v(r#"{"verification_uri":"missing-others"}"#)));
        assert!(!is_authorization_device_response(&v(r#"{"expires_in":1800}"#)));
        // Missing expires_in
        assert!(!is_authorization_device_response(&v(r#"{"device_code":"device-code-123","user_code":"ABCD-EFGH","verification_uri":"https://example.com/verify"}"#)));
        assert!(!is_authorization_device_response(&text("not an object")));
    }

    #[test]
    fn is_authorization_error_response_should_correctly_identify_error_response() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        // Valid error response
        assert!(is_authorization_error_response(&v(r#"{"error":"authorization_pending","error_description":"The authorization request is still pending"}"#)));

        // Valid error response with different error codes
        assert!(is_authorization_error_response(&v(r#"{"error":"slow_down","error_description":"Polling too fast"}"#)));
        assert!(is_authorization_error_response(&v(r#"{"error":"access_denied","error_description":"The user denied the request"}"#)));
        assert!(is_authorization_error_response(&v(r#"{"error":"expired_token","error_description":"The device code has expired"}"#)));

        // Valid response with optional error_uri
        assert!(is_authorization_error_response(&v(r#"{"error":"invalid_request","error_description":"The request is missing a required parameter","error_uri":"https://example.com/error"}"#)));

        // Invalid cases
        assert!(!is_authorization_error_response(&Value::Null));
        assert!(!is_authorization_error_response(&Value::Null));
        assert!(!is_authorization_error_response(&v("{}")));
        assert!(!is_authorization_error_response(&v(r#"{"error_description":"missing-error"}"#)));
        assert!(!is_authorization_error_response(&text("not an object")));
    }
}

mod scope_matching {
    use super::*;

    #[test]
    fn scopes_match_should_return_true_for_identical_scopes() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        assert!(scopes_match(Some(&["test", "scopes"]), Some(&["test", "scopes"])));
    }

    #[test]
    fn scopes_match_should_return_true_for_scopes_in_different_order() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        let scopes1 = ["6f1cc985-85e8-487e-b0dd-aa633302a731/.default", "VSCODE_TENANT:organizations"];
        let scopes2 = ["VSCODE_TENANT:organizations", "6f1cc985-85e8-487e-b0dd-aa633302a731/.default"];
        assert!(scopes_match(Some(&scopes1), Some(&scopes2)));
    }

    #[test]
    fn scopes_match_should_return_false_for_different_scopes() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        assert!(!scopes_match(Some(&["test", "scopes"]), Some(&["different", "scopes"])));
    }

    #[test]
    fn scopes_match_should_return_false_for_different_length_arrays() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        assert!(!scopes_match(Some(&["test"]), Some(&["test", "scopes"])));
    }

    #[test]
    fn scopes_match_should_handle_complex_microsoft_scopes() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        let scopes1 = ["6f1cc985-85e8-487e-b0dd-aa633302a731/.default", "VSCODE_TENANT:organizations"];
        let scopes2 = ["VSCODE_TENANT:organizations", "6f1cc985-85e8-487e-b0dd-aa633302a731/.default"];
        assert!(scopes_match(Some(&scopes1), Some(&scopes2)));
    }

    #[test]
    fn scopes_match_should_handle_empty_arrays() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        assert!(scopes_match(Some(&[]), Some(&[])));
    }

    #[test]
    fn scopes_match_should_handle_single_scope_arrays() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        assert!(scopes_match(Some(&["single"]), Some(&["single"])));
        assert!(!scopes_match(Some(&["single"]), Some(&["different"])));
    }

    #[test]
    fn scopes_match_should_handle_duplicate_scopes_within_arrays() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        assert!(scopes_match(Some(&["scope1", "scope2", "scope1"]), Some(&["scope2", "scope1", "scope1"])));
    }

    #[test]
    fn scopes_match_should_handle_undefined_values() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        assert!(scopes_match(None, None));
        assert!(!scopes_match(Some(&["read"]), None));
        assert!(!scopes_match(None, Some(&["write"])));
    }

    #[test]
    fn scopes_match_should_handle_mixed_undefined_and_empty_arrays() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        assert!(!scopes_match(Some(&[]), None));
        assert!(!scopes_match(None, Some(&[])));
        assert!(scopes_match(Some(&[]), Some(&[])));
    }
}

mod utility_functions {
    use super::*;

    #[test]
    fn get_default_metadata_for_url_should_return_correct_default_endpoints() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        let metadata = get_default_metadata_for_url("https://auth.example.com");

        assert_eq!(metadata.issuer, "https://auth.example.com/");
        assert_eq!(metadata.authorization_endpoint.as_deref(), Some("https://auth.example.com/authorize"));
        assert_eq!(metadata.token_endpoint.as_deref(), Some("https://auth.example.com/token"));
        assert_eq!(metadata.registration_endpoint.as_deref(), Some("https://auth.example.com/register"));
        assert_eq!(metadata.response_types_supported, vec!["code", "id_token", "id_token token"]);
    }
}

mod parsing_functions {
    use super::*;

    fn params(challenge: &IAuthenticationChallenge) -> Vec<(&str, &str)> {
        challenge.params.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
    }

    #[test]
    fn parse_www_authenticate_header_should_correctly_parse_simple_header() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        let result = parse_www_authenticate_header("Bearer");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].scheme, "Bearer");
        assert!(result[0].params.is_empty());
    }

    #[test]
    fn parse_www_authenticate_header_should_correctly_parse_header_with_parameters() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        let result = parse_www_authenticate_header(r#"Bearer realm="api", error="invalid_token", error_description="The access token expired""#);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].scheme, "Bearer");
        assert_eq!(params(&result[0]), vec![("realm", "api"), ("error", "invalid_token"), ("error_description", "The access token expired")]);
    }

    #[test]
    fn parse_www_authenticate_header_should_correctly_parse_parameters_with_equal_signs() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        let result = parse_www_authenticate_header(r#"Bearer resource_metadata="https://example.com/.well-known/oauth-protected-resource?v=1""#);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].scheme, "Bearer");
        assert_eq!(params(&result[0]), vec![("resource_metadata", "https://example.com/.well-known/oauth-protected-resource?v=1")]);
    }

    #[test]
    fn parse_www_authenticate_header_should_correctly_parse_multiple() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        let result = parse_www_authenticate_header(r#"Bearer realm="api", error="invalid_token", error_description="The access token expired", Basic realm="hi""#);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].scheme, "Bearer");
        assert_eq!(params(&result[0]), vec![("realm", "api"), ("error", "invalid_token"), ("error_description", "The access token expired")]);
        assert_eq!(result[1].scheme, "Basic");
        assert_eq!(params(&result[1]), vec![("realm", "hi")]);
    }

    fn jwt_part(source: &str) -> String {
        encode_base64(&VSBuffer::from_string(source), true, false)
    }

    #[test]
    fn get_claims_from_jwt_should_correctly_parse_a_jwt_token() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        // Create a sample JWT with known payload
        let payload = r#"{"jti":"id123","sub":"user123","iss":"https://example.com","aud":"client123","exp":1716239022,"iat":1716235422,"name":"Test User"}"#;

        // Create fake but properly formatted JWT
        let header = r#"{"alg":"HS256","typ":"JWT"}"#;
        let token = format!("{}.{}.fake-signature", jwt_part(header), jwt_part(payload));

        let claims = get_claims_from_jwt(&token).unwrap();
        assert_eq!(claims, v(payload));
    }

    #[test]
    fn get_claims_from_jwt_should_throw_for_invalid_jwt_format() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        // Test with wrong number of parts - should throw "Invalid JWT token format"
        for token in ["only.two", "one", "has.four.parts.here"] {
            let message = get_claims_from_jwt(token).expect_err("devia lançar").message;
            assert!(message.starts_with("Invalid JWT token format") && message.contains("three parts"), "{message}");
        }
    }

    #[test]
    fn get_claims_from_jwt_should_throw_for_invalid_header_content() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        // Create JWT with invalid header
        let token = format!("{}.{}.signature", jwt_part("not-json"), jwt_part(r#"{"sub":"test"}"#));

        let message = get_claims_from_jwt(&token).expect_err("devia lançar").message;
        assert!(message.contains("Failed to parse JWT token"), "{message}");
    }

    #[test]
    fn get_claims_from_jwt_should_throw_for_invalid_payload_content() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        // Create JWT with valid header but invalid payload
        let token = format!("{}.{}.signature", jwt_part(r#"{"alg":"HS256","typ":"JWT"}"#), jwt_part("not-json"));

        let message = get_claims_from_jwt(&token).expect_err("devia lançar").message;
        assert!(message.contains("Failed to parse JWT token"), "{message}");
    }
}

mod network_functions {
    use super::*;

    const BODY_OK: &str = r#"{"client_id":"generated-client-id","client_name":"Test Client"}"#;

    fn server() -> IAuthorizationServerMetadata {
        typed(r#"{"issuer":"https://auth.example.com","registration_endpoint":"https://auth.example.com/register","response_types_supported":["code"]}"#)
    }

    /// `JSON.parse(options.body)`.
    fn request_body(fetch_stub: &FetchStub) -> Value {
        v(&fetch_stub.init(0).body.expect("corpo da requisição"))
    }

    fn member<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
        match value {
            Value::Object(object) => object.get(name),
            _ => None,
        }
    }

    #[test]
    fn fetch_dynamic_registration_should_make_correct_request_and_parse_response() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|fetch_stub| async move {
            // Setup successful response
            let mock_response = r#"{"client_id":"generated-client-id","client_name":"Test Client","client_uri":"https://code.visualstudio.com"}"#;

            fetch_stub.resolves(MockResponse::new(true, 0).json_body(mock_response));

            let server_metadata = server();

            let result = fetch_dynamic_registration(&server_metadata, "Test Client", None).await.unwrap();

            // Verify fetch was called correctly
            assert_eq!(fetch_stub.call_count(), 1);
            let (url, options) = fetch_stub.args(0);
            assert_eq!(url, "https://auth.example.com/register");
            assert_eq!(options.method, "POST");
            assert_eq!(options.header("Content-Type"), Some("application/json"));

            // Verify request body
            let request_body = request_body(&fetch_stub);
            assert_eq!(member(&request_body, "client_name"), Some(&text("Test Client")));
            assert_eq!(member(&request_body, "client_uri"), Some(&text("https://code.visualstudio.com")));
            assert_eq!(member(&request_body, "grant_types"), Some(&v(r#"["authorization_code","refresh_token","urn:ietf:params:oauth:grant-type:device_code"]"#)));
            assert_eq!(member(&request_body, "response_types"), Some(&v(r#"["code"]"#)));
            assert_eq!(
                member(&request_body, "redirect_uris"),
                Some(&v(&format!(r#"["https://insiders.vscode.dev/redirect","https://vscode.dev/redirect","http://127.0.0.1/","http://127.0.0.1:{DEFAULT_AUTH_FLOW_PORT}/"]"#)))
            );

            // Verify response is processed correctly
            assert_eq!(result, typed::<IAuthorizationDynamicClientRegistrationResponse>(mock_response));
        });
    }

    #[test]
    fn fetch_dynamic_registration_should_throw_error_on_non_ok_response() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|fetch_stub| async move {
            fetch_stub.resolves(MockResponse::new(false, 0).status_text("Bad Request").text_body("Bad Request"));

            let error = rejected(fetch_dynamic_registration(&server(), "Test Client", None).await);
            assert!(error.message.contains("Registration to https://auth.example.com/register failed: Bad Request"), "{}", error.message);
        });
    }

    #[test]
    fn fetch_dynamic_registration_should_throw_error_on_invalid_response_format() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|fetch_stub| async move {
            // Missing required fields
            fetch_stub.resolves(MockResponse::new(true, 0).json_body(r#"{"invalid":"response"}"#));

            let error = rejected(fetch_dynamic_registration(&server(), "Test Client", None).await);
            assert!(error.message.contains("Invalid authorization dynamic client registration response"), "{}", error.message);
        });
    }

    #[test]
    fn fetch_dynamic_registration_should_filter_grant_types_based_on_server_metadata() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|fetch_stub| async move {
            // Setup successful response
            fetch_stub.resolves(MockResponse::new(true, 0).json_body(BODY_OK));

            // Mix of supported and unsupported
            let server_metadata: IAuthorizationServerMetadata = typed(
                r#"{"issuer":"https://auth.example.com","registration_endpoint":"https://auth.example.com/register","response_types_supported":["code"],"grant_types_supported":["authorization_code","client_credentials","refresh_token"]}"#,
            );

            fetch_dynamic_registration(&server_metadata, "Test Client", None).await.unwrap();

            // Verify fetch was called correctly
            assert_eq!(fetch_stub.call_count(), 1);

            // Verify request body contains only the intersection of supported grant types (client_credentials should be filtered out)
            assert_eq!(member(&request_body(&fetch_stub), "grant_types"), Some(&v(r#"["authorization_code","refresh_token"]"#)));
        });
    }

    #[test]
    fn fetch_dynamic_registration_should_use_default_grant_types_when_server_metadata_has_none() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|fetch_stub| async move {
            // Setup successful response
            fetch_stub.resolves(MockResponse::new(true, 0).json_body(BODY_OK));

            // No grant_types_supported specified
            fetch_dynamic_registration(&server(), "Test Client", None).await.unwrap();

            // Verify fetch was called correctly
            assert_eq!(fetch_stub.call_count(), 1);

            // Verify request body contains default grant types
            assert_eq!(member(&request_body(&fetch_stub), "grant_types"), Some(&v(r#"["authorization_code","refresh_token","urn:ietf:params:oauth:grant-type:device_code"]"#)));
        });
    }

    #[test]
    fn fetch_dynamic_registration_should_throw_error_when_registration_endpoint_is_missing() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|_fetch_stub| async move {
            // registration_endpoint is missing
            let server_metadata: IAuthorizationServerMetadata = typed(r#"{"issuer":"https://auth.example.com","response_types_supported":["code"]}"#);

            let error = rejected(fetch_dynamic_registration(&server_metadata, "Test Client", None).await);
            assert!(error.message.contains("Server does not support dynamic registration"), "{}", error.message);
        });
    }

    #[test]
    fn fetch_dynamic_registration_should_handle_structured_error_response() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|fetch_stub| async move {
            let error_response = r#"{"error":"invalid_client_metadata","error_description":"The client metadata is invalid"}"#;

            fetch_stub.resolves(MockResponse::new(false, 0).text_body(error_response));

            let error = rejected(fetch_dynamic_registration(&server(), "Test Client", None).await);
            assert!(error.message.contains("Registration to https://auth.example.com/register failed: invalid_client_metadata: The client metadata is invalid"), "{}", error.message);
        });
    }

    #[test]
    fn fetch_dynamic_registration_should_handle_structured_error_response_without_description() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|fetch_stub| async move {
            fetch_stub.resolves(MockResponse::new(false, 0).text_body(r#"{"error":"invalid_redirect_uri"}"#));

            let error = rejected(fetch_dynamic_registration(&server(), "Test Client", None).await);
            assert!(error.message.contains("Registration to https://auth.example.com/register failed: invalid_redirect_uri"), "{}", error.message);
        });
    }

    #[test]
    fn fetch_dynamic_registration_should_handle_malformed_json_error_response() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|fetch_stub| async move {
            fetch_stub.resolves(MockResponse::new(false, 0).text_body("Invalid JSON {"));

            let error = rejected(fetch_dynamic_registration(&server(), "Test Client", None).await);
            assert!(error.message.contains("Registration to https://auth.example.com/register failed: Invalid JSON {"), "{}", error.message);
        });
    }

    #[test]
    fn fetch_dynamic_registration_should_include_scopes_in_request_when_provided() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|fetch_stub| async move {
            fetch_stub.resolves(MockResponse::new(true, 0).json_body(BODY_OK));

            fetch_dynamic_registration(&server(), "Test Client", Some(&["read", "write"])).await.unwrap();

            // Verify request includes scopes
            assert_eq!(member(&request_body(&fetch_stub), "scope"), Some(&text("read write")));
        });
    }

    #[test]
    fn fetch_dynamic_registration_should_omit_scope_from_request_when_not_provided() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|fetch_stub| async move {
            fetch_stub.resolves(MockResponse::new(true, 0).json_body(BODY_OK));

            fetch_dynamic_registration(&server(), "Test Client", None).await.unwrap();

            // Verify request does not include scope when not provided
            assert_eq!(member(&request_body(&fetch_stub), "scope"), None);
        });
    }

    #[test]
    fn fetch_dynamic_registration_should_handle_empty_scopes_array() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|fetch_stub| async move {
            fetch_stub.resolves(MockResponse::new(true, 0).json_body(BODY_OK));

            fetch_dynamic_registration(&server(), "Test Client", Some(&[])).await.unwrap();

            // Verify request includes empty scope
            assert_eq!(member(&request_body(&fetch_stub), "scope"), Some(&text("")));
        });
    }

    #[test]
    fn fetch_dynamic_registration_should_handle_network_fetch_failure() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|fetch_stub| async move {
            fetch_stub.rejects("Network error");

            let error = rejected(fetch_dynamic_registration(&server(), "Test Client", None).await);
            assert!(error.message.contains("Network error"), "{}", error.message);
        });
    }

    #[test]
    fn fetch_dynamic_registration_should_handle_response_json_failure() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|fetch_stub| async move {
            fetch_stub.resolves(MockResponse::new(true, 0).json_throws("JSON parsing failed"));

            let error = rejected(fetch_dynamic_registration(&server(), "Test Client", None).await);
            assert!(error.message.contains("JSON parsing failed"), "{}", error.message);
        });
    }

    #[test]
    fn fetch_dynamic_registration_should_handle_response_text_failure_for_error_cases() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|fetch_stub| async move {
            fetch_stub.resolves(MockResponse::new(false, 0).text_throws("Text parsing failed"));

            let error = rejected(fetch_dynamic_registration(&server(), "Test Client", None).await);
            assert!(error.message.contains("Text parsing failed"), "{}", error.message);
        });
    }
}

mod client_id_fallback_scenarios {
    use super::*;

    #[test]
    fn fetch_dynamic_registration_should_throw_specific_error_for_missing_registration_endpoint() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|_fetch_stub| async move {
            // registration_endpoint is missing
            let server_metadata: IAuthorizationServerMetadata = typed(r#"{"issuer":"https://auth.example.com","response_types_supported":["code"]}"#);

            let error = rejected(fetch_dynamic_registration(&server_metadata, "Test Client", None).await);
            assert_eq!(error.message, "Server does not support dynamic registration");
        });
    }

    #[test]
    fn fetch_dynamic_registration_should_throw_specific_error_for_dcr_failure() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|fetch_stub| async move {
            fetch_stub.resolves(MockResponse::new(false, 0).text_body("DCR not supported"));

            let server_metadata: IAuthorizationServerMetadata = typed(
                r#"{"issuer":"https://auth.example.com","registration_endpoint":"https://auth.example.com/register","response_types_supported":["code"]}"#,
            );

            let error = rejected(fetch_dynamic_registration(&server_metadata, "Test Client", None).await);
            assert!(error.message.contains("Registration to https://auth.example.com/register failed: DCR not supported"), "{}", error.message);
        });
    }
}

mod fetch_resource_metadata {
    use super::*;

    const PRM_URL: &str = "https://example.com/.well-known/oauth-protected-resource";

    fn metadata_of(source: &str) -> IAuthorizationProtectedResourceMetadata {
        typed(source)
    }

    fn not_found() -> MockResponse {
        MockResponse::new(false, 404).text_body("Not Found").status_text("Not Found")
    }

    /// `error instanceof AggregateError || regex.test(error.message)`.
    fn is_aggregate_or_message(error: &Error, parts: &[&str]) -> bool {
        error.is_aggregate() || contains_in_order(&error.message, parts)
    }

    #[test]
    fn should_successfully_fetch_and_validate_resource_metadata() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api";
            let expected_metadata = r#"{"resource":"https://example.com/api","scopes_supported":["read","write"]}"#;

            fetch_stub.resolves(MockResponse::with_body(200, expected_metadata));

            let result = fetch_resource_metadata(target_resource, Some(PRM_URL), resource_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(result.discovery_url, PRM_URL);
            assert_eq!(result.errors, Vec::<Error>::new());
            assert_eq!(fetch_stub.call_count(), 1);
            assert_eq!(fetch_stub.url(0), PRM_URL);
            assert_eq!(fetch_stub.init(0).method, "GET");
            assert_eq!(fetch_stub.init(0).header("Accept"), Some("application/json"));
        });
    }

    #[test]
    fn should_include_same_origin_headers_when_origins_match() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api";
            let same_origin_headers = headers_of(&[("X-Test-Header", "test-value"), ("X-Custom-Header", "value")]);
            let expected_metadata = r#"{"resource":"https://example.com/api"}"#;

            fetch_stub.resolves(MockResponse::with_body(200, expected_metadata));

            let result = fetch_resource_metadata(target_resource, Some(PRM_URL), IFetchResourceMetadataOptions { fetch: Some(injected(&fetch_stub)), same_origin_headers }).await.unwrap();

            assert_eq!(result.discovery_url, PRM_URL);
            let init = fetch_stub.init(0);
            assert_eq!(init.header("Accept"), Some("application/json"));
            assert_eq!(init.header("X-Test-Header"), Some("test-value"));
            assert_eq!(init.header("X-Custom-Header"), Some("value"));
        });
    }

    #[test]
    fn should_not_include_same_origin_headers_when_origins_differ() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api";
            let resource_metadata_url = "https://other-domain.com/.well-known/oauth-protected-resource";
            let same_origin_headers = headers_of(&[("X-Test-Header", "test-value")]);
            let expected_metadata = r#"{"resource":"https://example.com/api"}"#;

            fetch_stub.resolves(MockResponse::with_body(200, expected_metadata));

            let result = fetch_resource_metadata(target_resource, Some(resource_metadata_url), IFetchResourceMetadataOptions { fetch: Some(injected(&fetch_stub)), same_origin_headers }).await.unwrap();

            assert_eq!(result.discovery_url, resource_metadata_url);
            let init = fetch_stub.init(0);
            assert_eq!(init.header("Accept"), Some("application/json"));
            assert_eq!(init.header("X-Test-Header"), None);
        });
    }

    #[test]
    fn should_throw_error_when_fetch_returns_non_200_status() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api";

            // Stub all possible URLs to return 404 for robust fallback testing
            fetch_stub.resolves(MockResponse::new(false, 404).text_body("Not Found"));

            let error = rejected(fetch_resource_metadata(target_resource, Some(PRM_URL), resource_options(&fetch_stub)).await);
            // Should be AggregateError since all URLs fail
            assert!(is_aggregate_or_message(&error, &["Failed to fetch resource metadata from", "404 Not Found"]), "{}", error.message);
        });
    }

    #[test]
    fn should_handle_error_when_response_text_throws() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api";

            // Stub all possible URLs to return 500 for robust fallback testing
            fetch_stub.resolves(MockResponse::new(false, 500).status_text("Internal Server Error").text_throws("Cannot read response"));

            let error = rejected(fetch_resource_metadata(target_resource, Some(PRM_URL), resource_options(&fetch_stub)).await);
            // Should be AggregateError since all URLs fail
            assert!(is_aggregate_or_message(&error, &["Failed to fetch resource metadata from", "500 Internal Server Error"]), "{}", error.message);
        });
    }

    #[test]
    fn should_throw_error_when_resource_property_does_not_match_target_resource() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api";

            // Stub all possible URLs to return invalid metadata for robust fallback testing
            fetch_stub.resolves(MockResponse::with_body(200, r#"{"resource":"https://different.com/api"}"#));

            let error = rejected(fetch_resource_metadata(target_resource, Some(PRM_URL), resource_options(&fetch_stub)).await);
            // Should be AggregateError since all URLs fail validation
            assert!(error.is_aggregate());
            assert!(error.errors.iter().any(|e| e.message.contains("does not match expected value")));
        });
    }

    #[test]
    fn should_normalize_urls_when_comparing_resource_values() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://EXAMPLE.COM/api";
            let metadata = r#"{"resource":"https://example.com/api"}"#;

            fetch_stub.resolves(MockResponse::with_body(200, metadata));

            // URL normalization should handle hostname case differences
            let result = fetch_resource_metadata(target_resource, Some(PRM_URL), resource_options(&fetch_stub)).await.unwrap();
            assert_eq!(result.metadata, metadata_of(metadata));
            assert_eq!(result.discovery_url, PRM_URL);
        });
    }

    #[test]
    fn should_throw_error_when_response_is_not_valid_resource_metadata() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api";
            // Missing required 'resource' property
            fetch_stub.resolves(MockResponse::with_body(200, r#"{"scopes_supported":["read","write"]}"#));

            let error = rejected(fetch_resource_metadata(target_resource, Some(PRM_URL), resource_options(&fetch_stub)).await);
            // Should be AggregateError since all URLs return invalid metadata
            assert!(is_aggregate_or_message(&error, &["Invalid resource metadata"]), "{}", error.message);
        });
    }

    #[test]
    fn should_throw_error_when_scopes_supported_is_not_an_array() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api";
            fetch_stub.resolves(MockResponse::with_body(200, r#"{"resource":"https://example.com/api","scopes_supported":"not an array"}"#));

            let error = rejected(fetch_resource_metadata(target_resource, Some(PRM_URL), resource_options(&fetch_stub)).await);
            // Should be AggregateError since all URLs return invalid metadata
            assert!(is_aggregate_or_message(&error, &["Invalid resource metadata"]), "{}", error.message);
        });
    }

    #[test]
    fn should_handle_metadata_with_optional_fields() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api";
            let metadata = r#"{"resource":"https://example.com/api","resource_name":"Example API","authorization_servers":["https://auth.example.com"],"jwks_uri":"https://example.com/jwks","scopes_supported":["read","write","admin"],"bearer_methods_supported":["header","body"],"resource_documentation":"https://example.com/docs"}"#;

            fetch_stub.resolves(MockResponse::with_body(200, metadata));

            let result = fetch_resource_metadata(target_resource, Some(PRM_URL), resource_options(&fetch_stub)).await.unwrap();
            assert_eq!(result.metadata, metadata_of(metadata));
        });
    }

    #[test]
    fn should_use_global_fetch_when_custom_fetch_is_not_provided() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|global_fetch_stub| async move {
            let target_resource = "https://example.com/api";
            let metadata = r#"{"resource":"https://example.com/api"}"#;

            global_fetch_stub.resolves(MockResponse::with_body(200, metadata));

            let result = fetch_resource_metadata(target_resource, Some(PRM_URL), IFetchResourceMetadataOptions::default()).await.unwrap();

            assert_eq!(result.metadata, metadata_of(metadata));
            assert_eq!(result.discovery_url, PRM_URL);
            assert_eq!(global_fetch_stub.call_count(), 1);
        });
    }

    #[test]
    fn should_handle_same_origin_with_different_ports() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com:8080/api";
            let resource_metadata_url = "https://example.com:9090/.well-known/oauth-protected-resource";
            let same_origin_headers = headers_of(&[("X-Test-Header", "test-value")]);
            let metadata = r#"{"resource":"https://example.com:8080/api"}"#;

            fetch_stub.resolves(MockResponse::with_body(200, metadata));

            let result = fetch_resource_metadata(target_resource, Some(resource_metadata_url), IFetchResourceMetadataOptions { fetch: Some(injected(&fetch_stub)), same_origin_headers }).await.unwrap();

            assert_eq!(result.discovery_url, resource_metadata_url);
            // Different ports mean different origins
            assert_eq!(fetch_stub.init(0).header("X-Test-Header"), None);
        });
    }

    #[test]
    fn should_handle_same_origin_with_different_protocols() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "http://example.com/api";
            let same_origin_headers = headers_of(&[("X-Test-Header", "test-value")]);
            let metadata = r#"{"resource":"http://example.com/api"}"#;

            fetch_stub.resolves(MockResponse::with_body(200, metadata));

            let result = fetch_resource_metadata(target_resource, Some(PRM_URL), IFetchResourceMetadataOptions { fetch: Some(injected(&fetch_stub)), same_origin_headers }).await.unwrap();

            assert_eq!(result.discovery_url, PRM_URL);
            // Different protocols mean different origins
            assert_eq!(fetch_stub.init(0).header("X-Test-Header"), None);
        });
    }

    #[test]
    fn should_include_error_details_in_message_with_resource_values() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api";

            // Stub all possible URLs to return invalid metadata for robust fallback testing
            fetch_stub.resolves(MockResponse::with_body(200, r#"{"resource":"https://different.com/other"}"#));

            // Should be AggregateError with validation errors
            let error = rejected(fetch_resource_metadata(target_resource, Some(PRM_URL), resource_options(&fetch_stub)).await);
            let error_message = if error.is_aggregate() { error.errors.iter().map(|e| e.message.as_str()).collect::<Vec<_>>().join(" ") } else { error.message.clone() };
            assert!(error_message.contains("does not match expected value"), "Error message should mention mismatch");
            assert!(error_message.contains("https://different.com/other"), "Error message should include actual resource value");
            assert!(error_message.contains("https://example.com/api"), "Error message should include expected resource value");
        });
    }

    #[test]
    fn should_fallback_to_well_known_uri_with_path_when_no_resource_metadata_url_provided() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api/v1";
            let expected_metadata = r#"{"resource":"https://example.com/api/v1","scopes_supported":["read","write"]}"#;

            fetch_stub.resolves(MockResponse::with_body(200, expected_metadata));

            let result = fetch_resource_metadata(target_resource, None, resource_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(result.discovery_url, "https://example.com/.well-known/oauth-protected-resource/api/v1");
            assert_eq!(fetch_stub.call_count(), 1);
            // Should try path-appended version first
            assert_eq!(fetch_stub.url(0), "https://example.com/.well-known/oauth-protected-resource/api/v1");
        });
    }

    #[test]
    fn should_fallback_to_well_known_uri_at_root_when_path_version_fails() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api/v1";
            let expected_metadata = r#"{"resource":"https://example.com/","scopes_supported":["read","write"]}"#;

            // First call fails, second succeeds
            fetch_stub.on_first_call_resolves(not_found());
            fetch_stub.on_second_call_resolves(MockResponse::with_body(200, expected_metadata));

            let result = fetch_resource_metadata(target_resource, None, resource_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(result.discovery_url, "https://example.com/.well-known/oauth-protected-resource");
            assert_eq!(result.errors.len(), 1);
            assert_eq!(fetch_stub.call_count(), 2);
            // First attempt with path
            assert_eq!(fetch_stub.url(0), "https://example.com/.well-known/oauth-protected-resource/api/v1");
            // Second attempt at root
            assert_eq!(fetch_stub.url(1), "https://example.com/.well-known/oauth-protected-resource");
        });
    }

    #[test]
    fn should_throw_error_when_all_well_known_uris_fail() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api/v1";

            fetch_stub.resolves(not_found());

            let error = rejected(fetch_resource_metadata(target_resource, None, resource_options(&fetch_stub)).await);
            assert!(error.is_aggregate(), "Should be an AggregateError");
            assert_eq!(error.errors.len(), 2, "Should contain 2 errors");
            assert!(contains_in_order(&error.errors[0].message, &["Failed to fetch resource metadata from", "/api/v1", "404"]), "First error should mention /api/v1 and 404");
            assert!(contains_in_order(&error.errors[1].message, &["Failed to fetch resource metadata from", ".well-known", "404"]), "Second error should mention .well-known and 404");
            assert_eq!(fetch_stub.call_count(), 2);
        });
    }

    #[test]
    fn should_not_append_path_when_target_resource_is_root() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/";
            let expected_metadata = r#"{"resource":"https://example.com/","scopes_supported":["read"]}"#;

            fetch_stub.resolves(MockResponse::with_body(200, expected_metadata));

            let result = fetch_resource_metadata(target_resource, None, resource_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(result.discovery_url, "https://example.com/.well-known/oauth-protected-resource");
            assert_eq!(fetch_stub.call_count(), 1);
            // Both URLs should be the same when path is /
            assert_eq!(fetch_stub.url(0), "https://example.com/.well-known/oauth-protected-resource");
        });
    }

    #[test]
    fn should_include_same_origin_headers_when_using_well_known_fallback() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api";
            let same_origin_headers = headers_of(&[("X-Test-Header", "test-value"), ("X-Custom-Header", "value")]);
            let expected_metadata = r#"{"resource":"https://example.com/api"}"#;

            fetch_stub.resolves(MockResponse::with_body(200, expected_metadata));

            let result = fetch_resource_metadata(target_resource, None, IFetchResourceMetadataOptions { fetch: Some(injected(&fetch_stub)), same_origin_headers }).await.unwrap();

            assert_eq!(result.discovery_url, "https://example.com/.well-known/oauth-protected-resource/api");
            let init = fetch_stub.init(0);
            assert_eq!(init.header("Accept"), Some("application/json"));
            assert_eq!(init.header("X-Test-Header"), Some("test-value"));
            assert_eq!(init.header("X-Custom-Header"), Some("value"));
        });
    }

    #[test]
    fn should_handle_fetch_impl_throwing_network_error_and_continue_to_next_url() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api/v1";
            let expected_metadata = r#"{"resource":"https://example.com/","scopes_supported":["read","write"]}"#;

            // First call throws network error, second succeeds
            fetch_stub.on_first_call_rejects("Network connection failed");
            fetch_stub.on_second_call_resolves(MockResponse::with_body(200, expected_metadata));

            let result = fetch_resource_metadata(target_resource, None, resource_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(result.discovery_url, "https://example.com/.well-known/oauth-protected-resource");
            assert_eq!(result.errors.len(), 1);
            assert!(result.errors[0].message.contains("Network connection failed"));
            assert_eq!(fetch_stub.call_count(), 2);
            // First attempt with path should have thrown error
            assert_eq!(fetch_stub.url(0), "https://example.com/.well-known/oauth-protected-resource/api/v1");
            // Second attempt at root should succeed
            assert_eq!(fetch_stub.url(1), "https://example.com/.well-known/oauth-protected-resource");
        });
    }

    #[test]
    fn should_throw_aggregate_error_when_fetch_impl_throws_on_all_urls() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api/v1";

            // Both calls throw network errors
            fetch_stub.rejects("Network connection failed");

            let error = rejected(fetch_resource_metadata(target_resource, None, resource_options(&fetch_stub)).await);
            assert!(error.is_aggregate(), "Should be an AggregateError");
            assert_eq!(error.errors.len(), 2, "Should contain 2 errors");
            assert!(error.errors[0].message.contains("Network connection failed"), "First error should mention network failure");
            assert!(error.errors[1].message.contains("Network connection failed"), "Second error should mention network failure");

            assert_eq!(fetch_stub.call_count(), 2);
        });
    }

    #[test]
    fn should_handle_mix_of_fetch_error_and_non_200_response() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api/v1";

            // First call throws network error
            fetch_stub.on_first_call_rejects("Connection timeout");

            // Second call returns 404
            fetch_stub.on_second_call_resolves(not_found());

            let error = rejected(fetch_resource_metadata(target_resource, None, resource_options(&fetch_stub)).await);
            assert!(error.is_aggregate(), "Should be an AggregateError");
            assert_eq!(error.errors.len(), 2, "Should contain 2 errors");
            assert!(error.errors[0].message.contains("Connection timeout"), "First error should be network error");
            assert!(contains_in_order(&error.errors[1].message, &["Failed to fetch resource metadata", "404"]), "Second error should be 404");

            assert_eq!(fetch_stub.call_count(), 2);
        });
    }

    #[test]
    fn should_accept_root_url_in_prm_resource_when_using_root_discovery_fallback_no_trailing_slash() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api/v1";
            // Per RFC 9728: when metadata retrieved from root discovery URL, the resource value must match the root URL
            // (where well-known was inserted)
            let expected_metadata = r#"{"resource":"https://example.com","scopes_supported":["read","write"]}"#;

            // First call (path-appended) fails, second (root) succeeds
            fetch_stub.on_first_call_resolves(not_found());
            fetch_stub.on_second_call_resolves(MockResponse::with_body(200, expected_metadata));

            let result = fetch_resource_metadata(target_resource, None, resource_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(fetch_stub.call_count(), 2);
        });
    }

    #[test]
    fn should_accept_root_url_in_prm_resource_when_using_root_discovery_fallback_with_trailing_slash() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api/v1";
            // Test that trailing slash form is also accepted (URL normalization)
            let expected_metadata = r#"{"resource":"https://example.com/","scopes_supported":["read","write"]}"#;

            // First call (path-appended) fails, second (root) succeeds
            fetch_stub.on_first_call_resolves(not_found());
            fetch_stub.on_second_call_resolves(MockResponse::with_body(200, expected_metadata));

            let result = fetch_resource_metadata(target_resource, None, resource_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(fetch_stub.call_count(), 2);
        });
    }

    #[test]
    fn should_reject_prm_with_full_path_resource_when_using_root_discovery_fallback() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api/v1";
            // This violates RFC 9728: root discovery PRM should have root URL, not full path
            let invalid_metadata = r#"{"resource":"https://example.com/api/v1","scopes_supported":["read"]}"#;

            // First call (path-appended) fails, second (root) returns invalid metadata
            fetch_stub.on_first_call_resolves(not_found());
            fetch_stub.on_second_call_resolves(MockResponse::with_body(200, invalid_metadata));

            let error = rejected(fetch_resource_metadata(target_resource, None, resource_options(&fetch_stub)).await);
            assert!(error.is_aggregate(), "Should be an AggregateError");
            assert_eq!(error.errors.len(), 2);
            // First error is 404 from path-appended attempt
            assert!(error.errors[0].message.contains("404"));
            // Second error is validation failure from root attempt
            assert!(error.errors[1].message.contains("does not match expected value"));
            // Check that validation was against root URL (origin) not full path
            assert!(contains_in_order(&error.errors[1].message, &["https://example.com/api/v1", "https://example.com"]));

            assert_eq!(fetch_stub.call_count(), 2);
        });
    }

    #[test]
    fn should_reject_prm_with_root_resource_when_using_path_appended_discovery() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api/v1";
            // This violates RFC 9728: path-appended discovery PRM should match full target URL
            let invalid_metadata = r#"{"resource":"https://example.com/","scopes_supported":["read"]}"#;

            // First attempt (path-appended) gets the wrong resource value. It will fail validation and continue to second
            // URL (root). Second attempt (root) will succeed because root expects root resource
            fetch_stub.resolves(MockResponse::with_body(200, invalid_metadata));

            // This should actually succeed on the second (root) attempt
            let result = fetch_resource_metadata(target_resource, None, resource_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(invalid_metadata));
            assert_eq!(result.discovery_url, "https://example.com/.well-known/oauth-protected-resource");
            assert_eq!(result.errors.len(), 1);
            assert_eq!(fetch_stub.call_count(), 2);
            // Verify both URLs were tried
            assert_eq!(fetch_stub.url(0), "https://example.com/.well-known/oauth-protected-resource/api/v1");
            assert_eq!(fetch_stub.url(1), "https://example.com/.well-known/oauth-protected-resource");
        });
    }

    #[test]
    fn should_validate_against_target_resource_when_resource_metadata_url_is_explicitly_provided() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api/v1";
            // When explicit URL provided (e.g., from WWW-Authenticate), must match targetResource
            let valid_metadata = r#"{"resource":"https://example.com/api/v1","scopes_supported":["read"]}"#;

            fetch_stub.resolves(MockResponse::with_body(200, valid_metadata));

            let result = fetch_resource_metadata(target_resource, Some(PRM_URL), resource_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(valid_metadata));
            assert_eq!(result.discovery_url, PRM_URL);
            assert_eq!(fetch_stub.call_count(), 1);
            assert_eq!(fetch_stub.url(0), PRM_URL);
        });
    }

    #[test]
    fn should_fallback_to_root_discovery_when_explicit_resource_metadata_url_validation_fails() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api/v1";
            let invalid_metadata = r#"{"resource":"https://example.com/","scopes_supported":["read"]}"#;

            // Stub all URLs to return root resource metadata. Explicit URL returns root (validation fails), path-appended
            // fails, root succeeds
            fetch_stub.resolves(MockResponse::with_body(200, invalid_metadata));

            // Should succeed on root discovery fallback
            let result = fetch_resource_metadata(target_resource, Some(PRM_URL), resource_options(&fetch_stub)).await.unwrap();
            assert_eq!(result.metadata, metadata_of(invalid_metadata));
            assert_eq!(result.discovery_url, "https://example.com/.well-known/oauth-protected-resource");
            assert!(!result.errors.is_empty());
            // Should have tried explicit URL, path-appended, then succeeded on root
            assert!(fetch_stub.call_count() >= 2);
        });
    }

    #[test]
    fn should_handle_fetch_impl_throwing_error_with_explicit_resource_metadata_url() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let target_resource = "https://example.com/api";

            // Stub all possible URLs to throw network error for robust fallback testing
            fetch_stub.rejects("DNS resolution failed");

            let error = rejected(fetch_resource_metadata(target_resource, Some(PRM_URL), resource_options(&fetch_stub)).await);
            // Should be AggregateError since all URLs fail
            assert!(is_aggregate_or_message(&error, &["DNS resolution failed"]), "{}", error.message);

            // Should have tried explicit URL and well-known discovery
            assert!(fetch_stub.call_count() >= 2);
        });
    }
}

mod fetch_authorization_server_metadata {
    use super::*;

    fn metadata_of(source: &str) -> IAuthorizationServerMetadata {
        typed(source)
    }

    /// `{ status: 200, json, text, statusText: 'OK' }`.
    fn ok_with(body: &str) -> MockResponse {
        MockResponse::with_body(200, body).status_text("OK")
    }

    /// `{ status, text: async () => text, statusText, json: async () => { throw new Error('Not JSON'); } }`.
    fn failing(status: u16, text: &str, status_text: &str) -> MockResponse {
        MockResponse::new(false, status).text_body(text).status_text(status_text).json_throws("Not JSON")
    }

    #[test]
    fn should_successfully_fetch_metadata_from_oauth_discovery_endpoint_with_path_insertion() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com/tenant";
            let expected_metadata = r#"{"issuer":"https://auth.example.com/tenant","authorization_endpoint":"https://auth.example.com/tenant/authorize","token_endpoint":"https://auth.example.com/tenant/token","response_types_supported":["code"]}"#;

            fetch_stub.resolves(ok_with(expected_metadata));

            let result = fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(result.discovery_url, "https://auth.example.com/.well-known/oauth-authorization-server/tenant");
            assert_eq!(result.errors, Vec::<Error>::new());
            assert_eq!(fetch_stub.call_count(), 1);
            // Should try OAuth discovery with path insertion: https://auth.example.com/.well-known/oauth-authorization-server/tenant
            assert_eq!(fetch_stub.url(0), "https://auth.example.com/.well-known/oauth-authorization-server/tenant");
            assert_eq!(fetch_stub.init(0).method, "GET");
        });
    }

    #[test]
    fn should_fallback_to_open_id_connect_discovery_with_path_insertion() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com/tenant";
            let expected_metadata = r#"{"issuer":"https://auth.example.com/tenant","authorization_endpoint":"https://auth.example.com/tenant/authorize","token_endpoint":"https://auth.example.com/tenant/token","response_types_supported":["code"]}"#;

            // First call fails, second succeeds
            fetch_stub.on_first_call_resolves(failing(404, "Not Found", "Not Found"));
            fetch_stub.on_second_call_resolves(ok_with(expected_metadata));

            let result = fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(result.discovery_url, "https://auth.example.com/.well-known/openid-configuration/tenant");
            assert_eq!(result.errors.len(), 1);
            assert_eq!(fetch_stub.call_count(), 2);
            // First attempt: OAuth discovery
            assert_eq!(fetch_stub.url(0), "https://auth.example.com/.well-known/oauth-authorization-server/tenant");
            // Second attempt: OpenID Connect discovery with path insertion
            assert_eq!(fetch_stub.url(1), "https://auth.example.com/.well-known/openid-configuration/tenant");
        });
    }

    #[test]
    fn should_fallback_to_open_id_connect_discovery_with_path_addition() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com/tenant";
            let expected_metadata = r#"{"issuer":"https://auth.example.com/tenant","authorization_endpoint":"https://auth.example.com/tenant/authorize","token_endpoint":"https://auth.example.com/tenant/token","response_types_supported":["code"]}"#;

            // First two calls fail, third succeeds
            fetch_stub.on_first_call_resolves(failing(404, "Not Found", "Not Found"));
            fetch_stub.on_second_call_resolves(failing(404, "Not Found", "Not Found"));
            fetch_stub.on_third_call_resolves(ok_with(expected_metadata));

            let result = fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(result.discovery_url, "https://auth.example.com/tenant/.well-known/openid-configuration");
            assert_eq!(result.errors.len(), 2);
            assert_eq!(fetch_stub.call_count(), 3);
            // First attempt: OAuth discovery
            assert_eq!(fetch_stub.url(0), "https://auth.example.com/.well-known/oauth-authorization-server/tenant");
            // Second attempt: OpenID Connect discovery with path insertion
            assert_eq!(fetch_stub.url(1), "https://auth.example.com/.well-known/openid-configuration/tenant");
            // Third attempt: OpenID Connect discovery with path addition
            assert_eq!(fetch_stub.url(2), "https://auth.example.com/tenant/.well-known/openid-configuration");
        });
    }

    #[test]
    fn should_handle_authorization_server_at_root_without_extra_path() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com";
            let expected_metadata = r#"{"issuer":"https://auth.example.com/","authorization_endpoint":"https://auth.example.com/authorize","token_endpoint":"https://auth.example.com/token","response_types_supported":["code"]}"#;

            fetch_stub.resolves(ok_with(expected_metadata));

            let result = fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(result.discovery_url, "https://auth.example.com/.well-known/oauth-authorization-server");
            assert_eq!(result.errors, Vec::<Error>::new());
            assert_eq!(fetch_stub.call_count(), 1);
            // For root URLs, no extra path is added
            assert_eq!(fetch_stub.url(0), "https://auth.example.com/.well-known/oauth-authorization-server");
        });
    }

    #[test]
    fn should_handle_authorization_server_with_trailing_slash() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com/tenant/";
            let expected_metadata = r#"{"issuer":"https://auth.example.com/tenant/","authorization_endpoint":"https://auth.example.com/tenant/authorize","token_endpoint":"https://auth.example.com/tenant/token","response_types_supported":["code"]}"#;

            fetch_stub.resolves(ok_with(expected_metadata));

            let result = fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(result.discovery_url, "https://auth.example.com/.well-known/oauth-authorization-server/tenant/");
            assert_eq!(result.errors, Vec::<Error>::new());
            assert_eq!(fetch_stub.call_count(), 1);
        });
    }

    #[test]
    fn should_include_additional_headers_in_all_requests() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com/tenant";
            let additional_headers = headers_of(&[("X-Custom-Header", "custom-value"), ("Authorization", "Bearer token123")]);
            let expected_metadata = r#"{"issuer":"https://auth.example.com/tenant","response_types_supported":["code"]}"#;

            fetch_stub.resolves(ok_with(expected_metadata));

            let result = fetch_authorization_server_metadata(authorization_server, IFetchAuthorizationServerMetadataOptions { fetch: Some(injected(&fetch_stub)), additional_headers }).await.unwrap();

            assert_eq!(result.discovery_url, "https://auth.example.com/.well-known/oauth-authorization-server/tenant");
            let init = fetch_stub.init(0);
            assert_eq!(init.header("X-Custom-Header"), Some("custom-value"));
            assert_eq!(init.header("Authorization"), Some("Bearer token123"));
            assert_eq!(init.header("Accept"), Some("application/json"));
        });
    }

    #[test]
    fn should_throw_aggregate_error_when_all_discovery_endpoints_fail() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com/tenant";

            fetch_stub.resolves(failing(404, "Not Found", "Not Found"));

            let error = rejected(fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await);
            assert!(error.is_aggregate(), "Should be an AggregateError");
            assert_eq!(error.errors.len(), 3, "Should contain 3 errors (one for each URL)");
            assert_eq!(error.message, "Failed to fetch authorization server metadata from all attempted URLs");
            // Verify each error includes the URL it attempted
            assert!(contains_in_order(&error.errors[0].message, &["oauth-authorization-server", "404"]), "First error should mention OAuth discovery and 404");
            assert!(contains_in_order(&error.errors[1].message, &["openid-configuration", "404"]), "Second error should mention OpenID path insertion and 404");
            assert!(contains_in_order(&error.errors[2].message, &["openid-configuration", "404"]), "Third error should mention OpenID path addition and 404");

            // Should have tried all three endpoints
            assert_eq!(fetch_stub.call_count(), 3);
        });
    }

    #[test]
    fn should_throw_single_error_not_aggregate_error_when_only_one_url_is_tried_and_fails() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com";

            // First attempt succeeds on second try, so only one error is collected for first URL
            fetch_stub.on_first_call_resolves(failing(500, "Internal Server Error", "Internal Server Error"));

            let expected_metadata = r#"{"issuer":"https://auth.example.com/","response_types_supported":["code"]}"#;

            fetch_stub.on_second_call_resolves(ok_with(expected_metadata));

            // Should succeed on second attempt
            let result = fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await.unwrap();
            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(result.errors.len(), 1);
            assert_eq!(fetch_stub.call_count(), 2);
        });
    }

    #[test]
    fn should_throw_aggregate_error_when_multiple_urls_fail_with_mixed_error_types() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com/tenant";

            // First call: network error
            fetch_stub.on_first_call_rejects("Connection timeout");

            // Second call: 404
            fetch_stub.on_second_call_resolves(failing(404, "Not Found", "Not Found"));

            // Third call: 500
            fetch_stub.on_third_call_resolves(failing(500, "Internal Server Error", "Internal Server Error"));

            let error = rejected(fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await);
            assert!(error.is_aggregate(), "Should be an AggregateError");
            assert_eq!(error.errors.len(), 3, "Should contain 3 errors");
            // First error is network error
            assert!(error.errors[0].message.contains("Connection timeout"), "First error should be network error");
            // Second error is 404
            assert!(contains_in_order(&error.errors[1].message, &["404", "Not Found"]), "Second error should be 404");
            // Third error is 500
            assert!(contains_in_order(&error.errors[2].message, &["500", "Internal Server Error"]), "Third error should be 500");

            assert_eq!(fetch_stub.call_count(), 3);
        });
    }

    #[test]
    fn should_handle_invalid_json_response() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com";

            fetch_stub.resolves(MockResponse::new(true, 200).json_throws("Invalid JSON").text_body("Invalid JSON").status_text("OK"));

            let error = rejected(fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await);
            assert!(error.message.contains("Failed to fetch authorization server metadata"), "{}", error.message);
        });
    }

    #[test]
    fn should_handle_valid_json_but_invalid_metadata_structure() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com";
            // Missing required 'issuer' field
            let invalid_metadata = r#"{"authorization_endpoint":"https://auth.example.com/authorize"}"#;

            fetch_stub.resolves(ok_with(invalid_metadata));

            let error = rejected(fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await);
            assert!(error.message.contains("Failed to fetch authorization server metadata"), "{}", error.message);
        });
    }

    #[test]
    fn should_use_global_fetch_when_custom_fetch_is_not_provided() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_global_stub(|global_fetch_stub| async move {
            let authorization_server = "https://auth.example.com";
            let expected_metadata = r#"{"issuer":"https://auth.example.com/","response_types_supported":["code"]}"#;

            global_fetch_stub.resolves(ok_with(expected_metadata));

            let result = fetch_authorization_server_metadata(authorization_server, IFetchAuthorizationServerMetadataOptions::default()).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(result.discovery_url, "https://auth.example.com/.well-known/oauth-authorization-server");
            assert_eq!(result.errors, Vec::<Error>::new());
            assert_eq!(global_fetch_stub.call_count(), 1);
        });
    }

    #[test]
    fn should_handle_network_fetch_failure_and_continue_to_next_endpoint() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com";
            let expected_metadata = r#"{"issuer":"https://auth.example.com/","response_types_supported":["code"]}"#;

            // First call throws network error, second succeeds
            fetch_stub.on_first_call_rejects("Network error");
            fetch_stub.on_second_call_resolves(ok_with(expected_metadata));

            let result = fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(result.errors.len(), 1);
            assert!(result.errors[0].message.contains("Network error"));
            // Should have tried two endpoints
            assert_eq!(fetch_stub.call_count(), 2);
        });
    }

    #[test]
    fn should_throw_error_when_network_fails_on_all_endpoints() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com";

            fetch_stub.rejects("Network error");

            let error = rejected(fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await);
            assert!(error.is_aggregate(), "Should be an AggregateError");
            assert_eq!(error.errors.len(), 3, "Should contain 3 errors");
            assert_eq!(error.message, "Failed to fetch authorization server metadata from all attempted URLs");
            // All errors should be network errors
            assert!(error.errors[0].message.contains("Network error"), "First error should be network error");
            assert!(error.errors[1].message.contains("Network error"), "Second error should be network error");
            assert!(error.errors[2].message.contains("Network error"), "Third error should be network error");

            // Should have tried all three endpoints
            assert_eq!(fetch_stub.call_count(), 3);
        });
    }

    #[test]
    fn should_handle_mix_of_network_error_and_non_200_response() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com/tenant";
            let expected_metadata = r#"{"issuer":"https://auth.example.com/tenant","response_types_supported":["code"]}"#;

            // First call throws network error
            fetch_stub.on_first_call_rejects("Connection timeout");

            // Second call returns 404
            fetch_stub.on_second_call_resolves(failing(404, "Not Found", "Not Found"));

            // Third call succeeds
            fetch_stub.on_third_call_resolves(ok_with(expected_metadata));

            let result = fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(result.errors.len(), 2);
            // Should have tried all three endpoints
            assert_eq!(fetch_stub.call_count(), 3);
        });
    }

    #[test]
    fn should_handle_response_text_failure_in_error_case() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com";

            fetch_stub.resolves(MockResponse::new(false, 500).text_throws("Cannot read text").status_text("Internal Server Error").json_throws("Cannot read json"));

            let error = rejected(fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await);
            assert!(error.is_aggregate(), "Should be an AggregateError");
            assert_eq!(error.errors.len(), 3, "Should contain 3 errors");
            // All errors should include status code and statusText (fallback when text() fails)
            for err in &error.errors {
                assert!(err.message.contains("500 Internal Server Error"), "Error should mention 500 and statusText: {}", err.message);
            }
        });
    }

    #[test]
    fn should_correctly_handle_path_addition_with_trailing_slash() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com/tenant/";
            let expected_metadata = r#"{"issuer":"https://auth.example.com/tenant/","response_types_supported":["code"]}"#;

            // First two calls fail, third succeeds
            fetch_stub.on_first_call_resolves(failing(404, "Not Found", "Not Found"));
            fetch_stub.on_second_call_resolves(failing(404, "Not Found", "Not Found"));
            fetch_stub.on_third_call_resolves(ok_with(expected_metadata));

            let result = fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(result.discovery_url, "https://auth.example.com/tenant/.well-known/openid-configuration");
            assert_eq!(result.errors.len(), 2);
            assert_eq!(fetch_stub.call_count(), 3);
            // Third attempt should correctly handle trailing slash (not double-slash)
            assert_eq!(fetch_stub.url(2), "https://auth.example.com/tenant/.well-known/openid-configuration");
        });
    }

    #[test]
    fn should_handle_deeply_nested_paths() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com/tenant/org/sub";
            let expected_metadata = r#"{"issuer":"https://auth.example.com/tenant/org/sub","response_types_supported":["code"]}"#;

            fetch_stub.resolves(ok_with(expected_metadata));

            let result = fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            assert_eq!(result.discovery_url, "https://auth.example.com/.well-known/oauth-authorization-server/tenant/org/sub");
            assert_eq!(result.errors, Vec::<Error>::new());
            assert_eq!(fetch_stub.call_count(), 1);
            // Should correctly insert well-known path with nested paths
            assert_eq!(fetch_stub.url(0), "https://auth.example.com/.well-known/oauth-authorization-server/tenant/org/sub");
        });
    }

    #[test]
    fn should_handle_200_response_with_non_metadata_json() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com";
            let invalid_response = r#"{"error":"not_supported","message":"Metadata not available"}"#;

            fetch_stub.resolves(ok_with(invalid_response));

            let error = rejected(fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await);
            assert!(error.is_aggregate(), "Should be an AggregateError");
            assert_eq!(error.errors.len(), 3, "Should contain 3 errors");
            // All errors should indicate failed to fetch with status code
            for err in &error.errors {
                assert!(err.message.contains("Failed to fetch authorization server metadata from"), "Error should mention failed fetch: {}", err.message);
            }

            // Should try all three endpoints
            assert_eq!(fetch_stub.call_count(), 3);
        });
    }

    #[test]
    fn should_validate_metadata_according_to_is_authorization_server_metadata() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com";
            // Valid metadata with all required fields
            let valid_metadata = r#"{"issuer":"https://auth.example.com/","authorization_endpoint":"https://auth.example.com/authorize","token_endpoint":"https://auth.example.com/token","jwks_uri":"https://auth.example.com/jwks","registration_endpoint":"https://auth.example.com/register","response_types_supported":["code","token"]}"#;

            fetch_stub.resolves(ok_with(valid_metadata));

            let result = fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(valid_metadata));
            assert_eq!(result.discovery_url, "https://auth.example.com/.well-known/oauth-authorization-server");
            assert_eq!(result.errors, Vec::<Error>::new());
            assert_eq!(fetch_stub.call_count(), 1);
        });
    }

    #[test]
    fn should_handle_ur_ls_with_query_parameters() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com/tenant?version=v2";
            let expected_metadata = r#"{"issuer":"https://auth.example.com/tenant?version=v2","response_types_supported":["code"]}"#;

            fetch_stub.resolves(ok_with(expected_metadata));

            let result = fetch_authorization_server_metadata(authorization_server, auth_server_options(&fetch_stub)).await.unwrap();

            assert_eq!(result.metadata, metadata_of(expected_metadata));
            // Query parameters are not included in the discovery URL (only pathname is extracted)
            assert_eq!(result.discovery_url, "https://auth.example.com/.well-known/oauth-authorization-server/tenant");
            assert_eq!(result.errors, Vec::<Error>::new());
            assert_eq!(fetch_stub.call_count(), 1);
        });
    }

    #[test]
    fn should_handle_empty_additional_headers() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_stub(|fetch_stub| async move {
            let authorization_server = "https://auth.example.com";
            let expected_metadata = r#"{"issuer":"https://auth.example.com/","response_types_supported":["code"]}"#;

            fetch_stub.resolves(ok_with(expected_metadata));

            let result = fetch_authorization_server_metadata(authorization_server, IFetchAuthorizationServerMetadataOptions { fetch: Some(injected(&fetch_stub)), additional_headers: Vec::new() }).await.unwrap();

            assert_eq!(result.discovery_url, "https://auth.example.com/.well-known/oauth-authorization-server");
            assert_eq!(fetch_stub.init(0).header("Accept"), Some("application/json"));
        });
    }
}

mod cross_app_access_id_jag_wire_format {
    use super::*;

    // Spec: draft-ietf-oauth-identity-assertion-authz-grant-03
    #[test]
    fn build_id_jag_exchange_body_emits_the_exact_spec_parameters() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        let body = build_id_jag_exchange_body(
            "my_idp_client_id",
            Some("secret_xyz"),
            "<id_token>",
            "https://auth.resource.example.com",
            Some("https://api.resource.example.com"),
            &["todos.read", "mcp.access"],
        );

        assert_eq!(body.get("client_id"), Some("my_idp_client_id"));
        assert_eq!(body.get("client_secret"), Some("secret_xyz"));
        assert_eq!(body.get("grant_type"), Some("urn:ietf:params:oauth:grant-type:token-exchange"));
        assert_eq!(body.get("subject_token"), Some("<id_token>"));
        assert_eq!(body.get("subject_token_type"), Some("urn:ietf:params:oauth:token-type:id_token"));
        assert_eq!(body.get("requested_token_type"), Some("urn:ietf:params:oauth:token-type:id-jag"));
        assert_eq!(body.get("audience"), Some("https://auth.resource.example.com"));
        assert_eq!(body.get("resource"), Some("https://api.resource.example.com"));
        assert_eq!(body.get("scope"), Some("todos.read mcp.access"));
    }

    #[test]
    fn build_id_jag_exchange_body_omits_client_secret_when_not_provided() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        let body = build_id_jag_exchange_body("public_client_id", None, "<id_token>", "https://auth.resource.example.com", None, &[]);

        assert!(!body.has("client_secret"));
        assert!(!body.has("resource"));
        assert!(!body.has("scope"));
    }

    #[test]
    fn build_resource_redemption_body_emits_an_rfc_7523_jwt_bearer_grant() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        let body = build_resource_redemption_body("my_idp_client_id-at-todo0", Some("secret_xyz"), "<id_jag>", Some("https://api.resource.example.com"), &["todos.read", "mcp.access"]);

        assert_eq!(body.get("client_id"), Some("my_idp_client_id-at-todo0"));
        assert_eq!(body.get("client_secret"), Some("secret_xyz"));
        assert_eq!(body.get("grant_type"), Some("urn:ietf:params:oauth:grant-type:jwt-bearer"));
        assert_eq!(body.get("assertion"), Some("<id_jag>"));
        assert_eq!(body.get("resource"), Some("https://api.resource.example.com"));
        assert_eq!(body.get("scope"), Some("todos.read mcp.access"));
    }
}
