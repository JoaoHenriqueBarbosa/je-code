//! Porte de `src/vs/base/test/common/oauth.test.ts` (suíte `OAuth`), caso a caso e na mesma ordem, para as
//! funções que não fazem requisição HTTP: `Type Guards`, `Scope Matching`, `Utility Functions`,
//! `Parsing Functions` e `Cross App Access (ID-JAG) wire format`.
//!
//! Notas de tradução:
//! - Os objetos literais do TypeScript são JSON lido por `v(...)`; `undefined` e `null` são os dois
//!   `Value::Null` (para todas as funções `is*` as duas são "não é um objeto"), e `'not an object'` é uma string.
//! - `assert.throws(fn, /texto/)` é o `Err` com a mensagem; `scopesMatch(undefined, ...)` é `None`.
//! - `getDefaultMetadataForUrl(new URL(...))` recebe o texto da URL.
//!
//! Pendentes (cada caso depende do `fetch`, que o porte injeta por um trait que `oauth.rs` não tem): a suíte
//! `Network Functions` (15 casos de `fetchDynamicRegistration`), `Client ID Fallback Scenarios` (2),
//! `fetchResourceMetadata` (31) e `fetchAuthorizationServerMetadata` (30).

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::json::{self, Value};
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
