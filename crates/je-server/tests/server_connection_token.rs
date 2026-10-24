//! Porte de `src/vs/server/test/node/serverConnectionToken.test.ts` (suítes `parseServerConnectionToken`,
//! `requestHasValidConnectionToken` e `getRedactedServerParsedArgs`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`{ ... } as ServerParsedArgs`** é o objeto dinâmico dos argumentos, montado por [`args`] com as chaves dadas.
//! - **`async () => 'defaultTokenValue'`** é [`default_token`], a promessa resolvida com o texto padrão.
//! - **`getRandomTestPath(os.tmpdir(), ...)` e o `.build` do diretório corrente** são diretórios do `tempfile`, apagados no
//!   fim (o `rmSync`/`rm` do original).
//! - **`assert.deepStrictEqual(result, new ServerConnectionTokenParseError(...))`** compara os valores (`PartialEq`).
//! - **`new URLSearchParams(...)`** é o `UrlSearchParams::parse`; `{ headers: {} }` é o mapa de cabeçalhos vazio.
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`**: nenhum caso cria descartáveis; o guarda é criado em cada um.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::collections::HashMap;

use common::{assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::network::{CONNECTION_TOKEN_COOKIE_NAME, CONNECTION_TOKEN_QUERY_NAME};
use je_base::oauth::UrlSearchParams;
use je_base::promise::Promise;
use je_server::server_connection_token::{
    MandatoryServerConnectionToken, ParsedServerConnectionToken, ServerConnectionToken, ServerConnectionTokenParseError, ServerConnectionTokenType,
    parse_server_connection_token, request_has_valid_connection_token,
};
use je_server::server_environment_service::{ServerParsedArgs, get_redacted_server_parsed_args};

fn args(entries: &[(&str, Dynamic)]) -> ServerParsedArgs {
    ServerParsedArgs::from_dynamic(&Dynamic::object(entries.iter().map(|(key, value)| (JsString::from(*key), value.clone()))))
}

fn text(value: &str) -> Dynamic {
    Dynamic::from(JsString::from(value))
}

fn default_token() -> Promise<String> {
    Promise::resolved("defaultTokenValue".to_owned())
}

fn is_error(result: &ParsedServerConnectionToken) -> bool {
    result.is_err()
}

fn assert_is_error(result: &ParsedServerConnectionToken) {
    assert!(is_error(result));
}

// ---- parseServerConnectionToken ------------------------------------------------------------------------------------

#[test]
fn no_arguments_generates_a_token_that_is_mandatory() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let result = parse_server_connection_token(&args(&[]), default_token).await.unwrap();
        let token = result.expect("sem argumentos o token é gerado");
        assert_eq!(token.token_type(), ServerConnectionTokenType::Mandatory);
    });
}

#[test]
fn without_connection_token() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let result = parse_server_connection_token(&args(&[("without-connection-token", Dynamic::Bool(true))]), default_token).await.unwrap();
        let token = result.expect("--without-connection-token não é erro");
        assert_eq!(token.token_type(), ServerConnectionTokenType::None);
    });
}

#[test]
fn without_connection_token_connection_token_results_in_error() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        assert_is_error(
            &parse_server_connection_token(&args(&[("without-connection-token", Dynamic::Bool(true)), ("connection-token", text("0"))]), default_token).await.unwrap(),
        );
    });
}

#[test]
fn without_connection_token_connection_token_file_results_in_error() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        assert_is_error(
            &parse_server_connection_token(&args(&[("without-connection-token", Dynamic::Bool(true)), ("connection-token-file", text("0"))]), default_token)
                .await
                .unwrap(),
        );
    });
}

#[test]
fn connection_token_file_connection_token_results_in_error() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        assert_is_error(&parse_server_connection_token(&args(&[("connection-token-file", text("0")), ("connection-token", text("0"))]), default_token).await.unwrap());
    });
}

#[test]
fn connection_token_file() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let test_dir = tempfile::tempdir().unwrap();
        let filename = test_dir.path().join("connection-token-file");
        let connection_token = "12345-123-abc";
        std::fs::write(&filename, connection_token).unwrap();
        let result = parse_server_connection_token(&args(&[("connection-token-file", text(filename.to_str().unwrap()))]), default_token).await.unwrap();
        let token = result.expect("o arquivo tem um token válido");
        assert_eq!(token.token_type(), ServerConnectionTokenType::Mandatory);
        assert_eq!(token, ServerConnectionToken::Mandatory(MandatoryServerConnectionToken::new(connection_token)));
    });
}

#[test]
fn connection_token_file_strips_a_trailing_lf_or_crlf_and_rejects_invalid_content() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let directory = tempfile::tempdir().unwrap();
        let filename = directory.path().join("token");
        let filename_text = filename.to_str().unwrap().to_owned();
        let mut results: Vec<ParsedServerConnectionToken> = Vec::new();
        for contents in ["valid-token\n", "valid-token\r\n", "invalid token\n"] {
            std::fs::write(&filename, contents).unwrap();
            results.push(parse_server_connection_token(&args(&[("connection-token-file", text(&filename_text))]), default_token).await.unwrap());
        }
        assert_eq!(
            results,
            vec![
                Ok(ServerConnectionToken::Mandatory(MandatoryServerConnectionToken::new("valid-token"))),
                Ok(ServerConnectionToken::Mandatory(MandatoryServerConnectionToken::new("valid-token"))),
                Err(ServerConnectionTokenParseError::new(format!("The connection token defined in '{filename_text} does not adhere to the characters 0-9, a-z, A-Z, _, or -."))),
            ]
        );
    });
}

#[test]
fn connection_token_file_reports_an_unreadable_file() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let directory = tempfile::tempdir().unwrap();
        let filename = directory.path().join("missing-token");
        let filename_text = filename.to_str().unwrap().to_owned();
        let result = parse_server_connection_token(&args(&[("connection-token-file", text(&filename_text))]), default_token).await.unwrap();
        assert_eq!(result, Err(ServerConnectionTokenParseError::new(format!("Unable to read the connection token file at '{filename_text}'."))));
    });
}

#[test]
fn connection_token() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let connection_token = "12345-123-abc";
        let result = parse_server_connection_token(&args(&[("connection-token", text(connection_token))]), default_token).await.unwrap();
        let token = result.expect("o token da linha de comando é válido");
        assert_eq!(token.token_type(), ServerConnectionTokenType::Mandatory);
        assert_eq!(token, ServerConnectionToken::Mandatory(MandatoryServerConnectionToken::new(connection_token)));
    });
}

// ---- requestHasValidConnectionToken --------------------------------------------------------------------------------

fn valid_token() -> ServerConnectionToken {
    ServerConnectionToken::Mandatory(MandatoryServerConnectionToken::new("valid token"))
}

#[test]
fn validates_a_decoded_query_parameter() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let search_params = UrlSearchParams::parse(&format!("{CONNECTION_TOKEN_QUERY_NAME}=valid+token"));

    assert!(request_has_valid_connection_token(&valid_token(), &HashMap::new(), &search_params));
}

#[test]
fn rejects_repeated_query_parameters() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let search_params = UrlSearchParams::parse(&format!("{CONNECTION_TOKEN_QUERY_NAME}=valid+token&{CONNECTION_TOKEN_QUERY_NAME}=valid+token"));

    assert!(!request_has_valid_connection_token(&valid_token(), &HashMap::new(), &search_params));
}

#[test]
fn falls_back_to_a_cookie() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let headers = HashMap::from([("cookie".to_owned(), format!("{CONNECTION_TOKEN_COOKIE_NAME}=valid%20token"))]);

    assert!(request_has_valid_connection_token(&valid_token(), &headers, &UrlSearchParams::new()));
}

// ---- getRedactedServerParsedArgs -----------------------------------------------------------------------------------

#[test]
fn redacts_connection_tokens_without_changing_the_original_arguments() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let original = args(&[
        ("connection-token", text("server-token")),
        ("agent-host-bridge-connection-token", text("bridge-token")),
        ("agent-host-bridge-port", text("9000")),
    ]);

    let redacted_args = get_redacted_server_parsed_args(&original);

    assert_deep_strict_equal(
        &Dynamic::object([("redactedArgs", redacted_args.to_dynamic()), ("args", original.to_dynamic())]),
        &Dynamic::object([
            (
                "redactedArgs",
                Dynamic::object([
                    ("connection-token", text("<redacted>")),
                    ("agent-host-bridge-connection-token", text("<redacted>")),
                    ("agent-host-bridge-port", text("9000")),
                ]),
            ),
            (
                "args",
                Dynamic::object([
                    ("connection-token", text("server-token")),
                    ("agent-host-bridge-connection-token", text("bridge-token")),
                    ("agent-host-bridge-port", text("9000")),
                ]),
            ),
        ]),
    );
}
