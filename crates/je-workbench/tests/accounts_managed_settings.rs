//! Porte de `src/vs/workbench/services/accounts/test/browser/managedSettings.test.ts` (suíte
//! `adaptManagedSettings`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - As respostas do servidor são valores dinâmicos (`j("{...}")` lê o literal no estilo do TypeScript, com aspas
//!   simples; o `JSON.parse('...')` do original passa pela mesma função), e os resultados são
//!   `IPolicyData` comparados por `PartialEq`: o `Partial<IPolicyData>` do original é o `IPolicyData` com os
//!   demais campos ausentes, e o `IndexMap` do `managedSettings` compara sem ordem, como o `deepStrictEqual`.
//! - Onde o original usa `undefined` dentro de um objeto (`temporaryAllowLocalHarness: undefined`), o valor é
//!   [`Dynamic::Undefined`]; um `{ ...response, futureControl: {} }` é o objeto novo que reaproveita `vscode`.
//! - `appendManagedSettingsClientIdentity(url, { version, copilotVersions })` recebe a versão e as
//!   `copilotVersions` como dois argumentos; a identidade de compatibilidade volta como
//!   `IManagedSettingsCompatibilityError` (os textos como `JsString`).
//! - O `Object.prototype` do JavaScript não existe no valor dinâmico (não há cadeia de protótipos): a última
//!   asserção do caso de chaves de mapa de telemetria (`({}).polluted === undefined`) não tem par, e o que sobra
//!   é a conferência do resultado, que é o que garante que a chave `__proto__` foi descartada.
//! - `warnings.push` é um `RefCell<Vec<String>>` passado como `onWarn`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::default_account::IPolicyData;
use je_base::dynamic::Dynamic;
use je_base::json;
use je_base::policy::{ManagedSettingsData, PolicyValue};
use je_base::product::ICopilotVersions;
use je_platform::js::from_json;
use je_workbench::services::accounts::managed_settings::{
    IManagedSettingsCompatibilityError, IManagedSettingsResponse, MANAGED_SETTINGS_UPDATE_REQUIRED_ERROR_CODE, adapt_managed_settings, append_managed_settings_client_identity,
    parse_managed_settings_compatibility_error,
};

/// Um literal no estilo do TypeScript: JSON com aspas simples. Também lê o argumento de `JSON.parse`.
fn j(text: &str) -> Dynamic {
    let mut errors = Vec::new();
    let value = json::parse_str(&text.replace('\'', "\""), &mut errors).unwrap_or_else(|| panic!("literal inválido: {text}"));
    assert!(errors.is_empty(), "literal inválido: {text}");
    from_json(&value)
}

fn obj(entries: Vec<(&str, Dynamic)>) -> Dynamic {
    Dynamic::object(entries)
}

fn text(value: &str) -> PolicyValue {
    PolicyValue::String(value.to_owned())
}

/// `adaptManagedSettings(response)`.
fn adapt(response: &IManagedSettingsResponse) -> IPolicyData {
    adapt_managed_settings(response, None)
}

/// `adaptManagedSettings(response, msg => warnings.push(msg))`.
fn adapt_collecting_warnings(response: &IManagedSettingsResponse) -> (IPolicyData, Vec<String>) {
    let warnings = RefCell::new(Vec::new());
    let result = adapt_managed_settings(response, Some(&|message: &str| warnings.borrow_mut().push(message.to_owned())));
    (result, warnings.into_inner())
}

/// Um `Partial<IPolicyData>` literal: `{ managedSettings, managedSettingsActive? }`.
fn adapted<K: Into<String>>(entries: Vec<(K, PolicyValue)>, managed_settings_active: Option<bool>) -> IPolicyData {
    let managed_settings: ManagedSettingsData = entries.into_iter().map(|(key, value)| (key.into(), value)).collect();
    IPolicyData {
        mcp: None,
        chat_preview_features_enabled: None,
        chat_agent_enabled: None,
        cloud_session_storage_enabled: None,
        mcp_registry_url: None,
        mcp_access: None,
        managed_settings: Some(managed_settings),
        managed_settings_active,
        extra: Default::default(),
    }
}

#[test]
fn empty_response_yields_an_empty_managed_settings_bag() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(adapt(&j("{}")), adapted(Vec::<(&str, PolicyValue)>::new(), Some(false)));
}

#[test]
fn temporary_local_permission_never_implies_governance_or_masks_unrepresentable_settings() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let key = "vscode.temporaryAllowLocalHarness";
    let values = vec![Dynamic::Bool(true), Dynamic::Bool(false), Dynamic::string("false"), Dynamic::Null, obj(vec![]), Dynamic::array(Vec::new()), j("{ 'future': true }"), Dynamic::Undefined];
    let results: Vec<(IPolicyData, IPolicyData)> = values
        .into_iter()
        .map(|value| {
            let response = obj(vec![("vscode", obj(vec![("temporaryAllowLocalHarness", value)]))]);
            let with_other_settings = obj(vec![("vscode", response.get_str("vscode")), ("futureControl", obj(vec![]))]);
            (adapt(&response), adapt(&with_other_settings))
        })
        .collect();
    let expected_entries: Vec<Vec<(String, PolicyValue)>> = vec![
        vec![(key.to_owned(), PolicyValue::Boolean(true))],
        vec![(key.to_owned(), PolicyValue::Boolean(false))],
        vec![(key.to_owned(), text("false"))],
        vec![],
        vec![],
        vec![],
        vec![(format!("{key}.future"), PolicyValue::Boolean(true))],
        vec![],
    ];
    let expected: Vec<(IPolicyData, IPolicyData)> = expected_entries.into_iter().map(|entries| (adapted(entries.clone(), Some(false)), adapted(entries, Some(true)))).collect();
    assert_eq!(results, expected);
}

#[test]
fn preserves_an_empty_telemetry_block_and_explicit_identity_leaves() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        (
            adapt(&j("{ 'telemetry': {} }")),
            adapt(&j("{ 'telemetry': { 'capture': { 'identity': false } } }")),
            adapt(&j("{ 'telemetry': { 'capture': { 'identity': true } } }")),
        ),
        (
            adapted(vec![("telemetry", text("{}"))], None),
            adapted(vec![("telemetry.capture.identity", PolicyValue::Boolean(false))], None),
            adapted(vec![("telemetry.capture.identity", PolicyValue::Boolean(true))], None),
        )
    );
}

#[test]
fn appends_client_identity_to_the_request_url() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let versions = ICopilotVersions { runtime: "0.0.344".to_owned(), sdk: "0.1.0".to_owned(), extra: Default::default() };
    assert_eq!(
        (
            append_managed_settings_client_identity("https://api.github.com/copilot_internal/managed_settings", "1.132.0", Some(&versions)),
            append_managed_settings_client_identity("https://api.github.com/copilot_internal/managed_settings", "1.132.0", None),
            append_managed_settings_client_identity("https://api.github.com/copilot_internal/managed_settings?foo=bar", "1.132.0", None),
            append_managed_settings_client_identity("https://api.github.com/copilot_internal/managed_settings?copilot_runtime_version=0.0.1", "1.132.0", None),
            append_managed_settings_client_identity("not a url", "1.132.0", None),
        ),
        (
            "https://api.github.com/copilot_internal/managed_settings?client_id=vscode&client_version=1.132.0&copilot_runtime_version=0.0.344".to_owned(),
            "https://api.github.com/copilot_internal/managed_settings?client_id=vscode&client_version=1.132.0".to_owned(),
            "https://api.github.com/copilot_internal/managed_settings?foo=bar&client_id=vscode&client_version=1.132.0".to_owned(),
            "https://api.github.com/copilot_internal/managed_settings?client_id=vscode&client_version=1.132.0".to_owned(),
            "not a url".to_owned(),
        )
    );
}

#[test]
fn normalizes_permissions_into_a_dot_path_managed_setting() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        adapt(&j("{ 'permissions': { 'disableBypassPermissionsMode': 'disable' } }")),
        adapted(vec![("permissions.disableBypassPermissionsMode", text("disable"))], None)
    );
}

#[test]
fn retains_permission_rules_for_effective_harness_eligibility() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(adapt(&j("{ 'permissions': { 'deny': ['Shell'] } }")), adapted(vec![("permissions.deny", text(r#"["Shell"]"#))], None));
}

#[test]
fn parses_the_stable_compatibility_error_and_optional_versions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        parse_managed_settings_compatibility_error(&j("{ 'error_code': 'client_update_required', 'client_id': 'vscode', 'client_version': '1.132.0', 'minimum_client_version': '1.133.0' }")),
        Some(IManagedSettingsCompatibilityError {
            error_code: MANAGED_SETTINGS_UPDATE_REQUIRED_ERROR_CODE,
            client_version: Some(JsString::from("1.132.0")),
            minimum_client_version: Some(JsString::from("1.133.0")),
        })
    );
}

#[test]
fn rejects_an_unrecognized_compatibility_error_shape() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(parse_managed_settings_compatibility_error(&j("{ 'error_code': 'unexpected' }")), None);
}

#[test]
fn carries_enabled_plugins_as_a_canonical_json_string_under_a_single_key() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let response: IManagedSettingsResponse = j("{ 'enabledPlugins': { 'assign-issue-to-copilot@agent-skills': true, 'my-plugin@acme': false } }");
    assert_eq!(
        adapt(&response),
        adapted(vec![("enabledPlugins", text(r#"{"assign-issue-to-copilot@agent-skills":true,"my-plugin@acme":false}"#))], None)
    );
}

#[test]
fn carries_strict_known_marketplaces_as_a_canonical_json_string_under_a_single_key() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        adapt(&j("{ 'strictKnownMarketplaces': [{ 'source': 'github', 'repo': 'rwoll/markdown-review' }] }")),
        adapted(vec![("strictKnownMarketplaces", text(r#"[{"source":"github","repo":"rwoll/markdown-review"}]"#))], None)
    );
}

#[test]
fn carries_an_empty_strict_known_marketplaces_array_lockdown_as_a_json_string() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(adapt(&j("{ 'strictKnownMarketplaces': [] }")), adapted(vec![("strictKnownMarketplaces", text("[]"))], None));
}

#[test]
fn carries_allowed_mcp_servers_as_a_canonical_json_string_under_a_single_key() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        adapt(&j("{ 'allowedMcpServers': [{ 'serverName': 'github' }, { 'serverUrl': 'https://mcp.example.com/*' }, { 'serverCommand': ['npx', '-y', 'server'] }] }")),
        adapted(
            vec![("allowedMcpServers", text(r#"[{"serverName":"github"},{"serverUrl":"https://mcp.example.com/*"},{"serverCommand":["npx","-y","server"]}]"#))],
            None
        )
    );
}

#[test]
fn carries_an_empty_allowed_mcp_servers_array_as_a_json_string() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(adapt(&j("{ 'allowedMcpServers': [] }")), adapted(vec![("allowedMcpServers", text("[]"))], None));
}

#[test]
fn carries_denied_mcp_servers_as_a_canonical_json_string_under_a_single_key() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        adapt(&j("{ 'deniedMcpServers': [{ 'serverName': 'blocked' }, { 'serverUrl': 'https://*.untrusted.example.com/*' }] }")),
        adapted(vec![("deniedMcpServers", text(r#"[{"serverName":"blocked"},{"serverUrl":"https://*.untrusted.example.com/*"}]"#))], None)
    );
}

#[test]
fn carries_customization_lockdown_controls() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        adapt(&j("{ 'strictPluginOnlyCustomization': true, 'allowManagedMcpServersOnly': true, 'allowManagedHooksOnly': true, 'forceRemoteSettingsRefresh': true }")),
        adapted(
            vec![
                ("strictPluginOnlyCustomization", PolicyValue::Boolean(true)),
                ("allowManagedMcpServersOnly", PolicyValue::Boolean(true)),
                ("allowManagedHooksOnly", PolicyValue::Boolean(true)),
                ("forceRemoteSettingsRefresh", PolicyValue::Boolean(true)),
            ],
            None
        )
    );
}

#[test]
fn flattens_scalar_telemetry_leaves_and_carries_resource_attributes_and_headers_as_single_json_keys() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        adapt(&j(
            "{ 'telemetry': { 'enabled': true, 'serviceName': 'acme-copilot', 'resourceAttributes': { 'deployment.environment': 'prod', 'service.namespace': 'acme' }, 'headers': { 'x-api-key': 'secret' } } }"
        )),
        adapted(
            vec![
                ("telemetry.enabled", PolicyValue::Boolean(true)),
                ("telemetry.serviceName", text("acme-copilot")),
                ("telemetry.resourceAttributes", text(r#"{"deployment.environment":"prod","service.namespace":"acme"}"#)),
                ("telemetry.headers", text(r#"{"x-api-key":"secret"}"#)),
            ],
            None
        )
    );
}

#[test]
fn adapts_managed_telemetry_file_paths_through_ordinary_normalization() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let valid_path: IManagedSettingsResponse = j("{ 'telemetry': { 'filePath': '/managed/copilot.jsonl' } }");
    let null_path = j(r#"{"telemetry":{"filePath":null,"enabled":false,"capture":{"identity":false},"resourceAttributes":{"service.namespace":"acme"}}}"#);
    let number_path = j(r#"{"telemetry":{"filePath":1,"enabled":false}}"#);
    assert_eq!(
        (adapt(&valid_path), adapt(&null_path), adapt(&number_path)),
        (
            adapted(vec![("telemetry.filePath", text("/managed/copilot.jsonl"))], None),
            adapted(
                vec![
                    ("telemetry.enabled", PolicyValue::Boolean(false)),
                    ("telemetry.capture.identity", PolicyValue::Boolean(false)),
                    ("telemetry.resourceAttributes", text(r#"{"service.namespace":"acme"}"#)),
                ],
                None
            ),
            adapted(vec![("telemetry.filePath", PolicyValue::Number(1.0)), ("telemetry.enabled", PolicyValue::Boolean(false))], None),
        )
    );
}

#[test]
fn encodes_github_marketplaces_as_a_name_shorthand_json_dict() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        adapt(&j("{ 'extraKnownMarketplaces': { 'a': { 'source': { 'source': 'github', 'repo': 'github/agent-skills' } }, 'b': { 'source': { 'source': 'github', 'repo': 'acme/things', 'ref': 'main' } } } }")),
        adapted(vec![("extraKnownMarketplaces", text(r##"{"a":"github/agent-skills","b":"acme/things#main"}"##))], None)
    );
}

#[test]
fn encodes_git_marketplaces_as_a_name_url_json_dict() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        adapt(&j("{ 'extraKnownMarketplaces': { 'a': { 'source': { 'source': 'git', 'url': 'https://example.com/repo.git' } }, 'b': { 'source': { 'source': 'git', 'url': 'ssh://git@host/path.git', 'ref': 'v1' } } } }")),
        adapted(vec![("extraKnownMarketplaces", text(r##"{"a":"https://example.com/repo.git","b":"ssh://git@host/path.git#v1"}"##))], None)
    );
}

#[test]
fn encodes_mixed_github_and_git_marketplaces_dedups_by_name() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        adapt(&j("{ 'extraKnownMarketplaces': { 'a': { 'source': { 'source': 'github', 'repo': 'a/b' } }, 'b': { 'source': { 'source': 'git', 'url': 'https://example.com/r.git' } } } }")),
        adapted(vec![("extraKnownMarketplaces", text(r#"{"a":"a/b","b":"https://example.com/r.git"}"#))], None)
    );
}

#[test]
fn handles_a_full_populated_response_all_three_structured_settings_together() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        adapt(&j("{ 'enabledPlugins': { 'p@m': true }, 'extraKnownMarketplaces': { 'a': { 'source': { 'source': 'github', 'repo': 'a/b', 'ref': 'r' } } }, 'strictKnownMarketplaces': [{ 'source': 'github', 'repo': 'a/b' }] }")),
        adapted(
            vec![
                ("strictKnownMarketplaces", text(r#"[{"source":"github","repo":"a/b"}]"#)),
                ("enabledPlugins", text(r#"{"p@m":true}"#)),
                ("extraKnownMarketplaces", text(r##"{"a":"a/b#r"}"##)),
            ],
            None
        )
    );
}

#[test]
fn resilience_unknown_scalar_keys_flatten_into_the_bag_alongside_structured_keys() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        adapt(&j("{ 'enabledPlugins': { 'p@m': true }, 'strictKnownMarketplaces': [], 'joshsFakeSetting': true }")),
        adapted(
            vec![
                ("strictKnownMarketplaces", text("[]")),
                ("joshsFakeSetting", PolicyValue::Boolean(true)),
                ("enabledPlugins", text(r#"{"p@m":true}"#)),
            ],
            None
        )
    );
}

#[test]
fn resilience_a_server_sent_own_proto_key_is_carried_like_any_scalar_never_applied_to_the_prototype() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // JSON.parse (not an object literal) yields an OWN enumerable `__proto__` data property.
    // The scalar remainder must keep `{ ...rest }` semantics: copy it as data (so it flattens
    // to `__proto__.polluted`) rather than assigning through the inherited `__proto__` setter
    // (which would swap the prototype and instead surface the inherited `polluted` key).
    let response: IManagedSettingsResponse = j(r#"{"permissions":{"x":1},"__proto__":{"polluted":true}}"#);
    assert_eq!(
        adapt(&response),
        adapted(vec![("permissions.x", PolicyValue::Number(1.0)), ("__proto__.polluted", PolicyValue::Boolean(true))], None)
    );
}

#[test]
fn resilience_a_primitive_own_proto_scalar_is_dropped_never_pollutes_the_result() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // The reviewer-flagged case. flattenManagedSettings only assigns at the bare `__proto__`
    // key when the value is a PRIMITIVE, where the inherited `__proto__` setter is a no-op, so
    // the value is simply dropped (no prototype mutation), matching the original `...rest`.
    let response: IManagedSettingsResponse = j(r#"{"permissions":{"x":1},"__proto__":true}"#);
    assert_eq!(adapt(&response), adapted(vec![("permissions.x", PolicyValue::Number(1.0))], None));
}

#[test]
fn resilience_malformed_marketplace_entries_are_skipped_valid_entries_still_processed() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (result, warnings) = adapt_collecting_warnings(&j(
        "{ 'extraKnownMarketplaces': { 'good': { 'source': { 'source': 'github', 'repo': 'a/b' } }, 'bad-no-source': {}, 'bad-unknown-type': { 'source': { 'source': 'ftp', 'url': 'ftp://x' } } } }",
    ));
    assert_eq!(result, adapted(vec![("extraKnownMarketplaces", text(r#"{"good":"a/b"}"#))], None));
    assert_eq!(warnings.len(), 2);
}

#[test]
fn resilience_extra_known_marketplaces_github_entry_missing_repo_is_skipped_with_a_warning() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (result, warnings) = adapt_collecting_warnings(&j("{ 'extraKnownMarketplaces': { 'example-key': { 'source': { 'source': 'github' } } } }"));
    assert_eq!(
        (result, warnings.len(), warnings.iter().any(|warning| warning.contains(r#"requires "repo""#))),
        (adapted(Vec::<(&str, PolicyValue)>::new(), Some(true)), 1, true)
    );
}

#[test]
fn resilience_a_marketplace_string_array_wrong_format_is_treated_as_missing_no_throw() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        adapt(&j("{ 'extraKnownMarketplaces': ['https://plugins.acme.com'] }")),
        adapted(Vec::<(&str, PolicyValue)>::new(), Some(true))
    );
}

#[test]
fn resilience_telemetry_map_keys_that_could_pollute_the_prototype_are_dropped() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // JSON.parse yields an OWN enumerable `__proto__` data property on the nested map.
    let response: IManagedSettingsResponse = j(r#"{"telemetry":{"resourceAttributes":{"__proto__":"polluted","constructor":"x","service.namespace":"acme"}}}"#);
    assert_eq!(
        adapt(&response),
        adapted(vec![("telemetry.resourceAttributes", text(r#"{"service.namespace":"acme"}"#))], None)
    );
}
