//! Porte de `src/vs/workbench/services/accounts/test/common/githubEnterprise.test.ts` (suítes
//! `GitHub Enterprise enrollment configuration`, `GitHub Enterprise instance validation` e
//! `GitHub Enterprise session provenance`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso; o
//!   `store.add(service.onDidChangeConfigurationEmitter)` de `configuration()` é `add_rc` no guarda. O
//!   `teardown(() => sinon.restore())` não tem par: os substitutos vivem na instância do serviço.
//! - `sinon.stub(service, 'inspect').returns({...})` é `stub_inspect(TestConfigurationValue {...})` e
//!   `sinon.stub(service, 'updateValue')` é `stub_update_value` (`.resolves()` e `.callsFake((key, value) =>
//!   service.setUserConfiguration(key, value))` são os dois comportamentos); `write.getCalls().map(call =>
//!   call.args)` é `update_value_calls()`. Os valores das chamadas são comparados como texto, porque um
//!   [`Dynamic`] de array compara por identidade.
//! - Os testes `async` rodam sob `run_with_faked_timers`; `assert.rejects(promise, /re/)` é o erro da promessa
//!   rejeitada com a mensagem conferida, e `assert.throws(fn, /re/)` é o `Err` devolvido.
//! - Os `for` que geram um `test(...)` por escopo são um caso que percorre os escopos, com o escopo na mensagem.
//! - Uma chave de escopo (`'applicationValue'`, ...) é a camada do `TestConfigurationValue` de mesmo nome
//!   ([`with_scope`]).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, LeakGuard};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::uri::URI;
use je_platform::configuration::ConfigurationTarget;
use je_workbench::services::accounts::github_enterprise::{
    GITHUB_ENTERPRISE_URIS_SETTING, add_github_enterprise_uri, get_configured_github_enterprise_uris, get_github_enterprise_uri, is_valid_github_enterprise_uri,
};
use support::github_enterprise_configuration_service::{TestConfigurationService, TestConfigurationValue, UpdateValueBehavior};

const LEGACY_SETTING: &str = "github-enterprise.uri";

fn text(value: &str) -> Dynamic {
    Dynamic::string(value)
}

/// Um array de textos como valor de configuração.
fn texts(values: &[&str]) -> Dynamic {
    Dynamic::array(values.iter().map(|value| text(value)))
}

/// `Vec<JsString>` do resultado de `getConfiguredGitHubEnterpriseUris`, como `Vec<String>`.
fn uris_of(result: Result<Vec<JsString>, je_base::errors::Error>) -> Vec<String> {
    result.expect("a lista de instâncias é válida").iter().map(JsString::to_string_lossy).collect()
}

fn owned(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// `configuration(values)`: o serviço, com o emissor entregue ao guarda.
fn configuration(guard: &LeakGuard, values: Vec<(&str, Dynamic)>) -> Rc<TestConfigurationService> {
    let service = TestConfigurationService::new(Some(Dynamic::object(values)));
    guard.add_rc(service.on_did_change_configuration_emitter.clone());
    service
}

/// As chamadas de `updateValue` como `[key, value, target]`, com o valor como lista de textos.
fn writes(service: &TestConfigurationService) -> Vec<(String, Vec<String>, ConfigurationTarget)> {
    service.update_value_calls().into_iter().map(|call| (call.key, array_of_texts(&call.value), call.target)).collect()
}

/// O valor como texto (`getValue('github-enterprise.uri')` é um texto).
fn text_of(value: &Dynamic) -> String {
    match value {
        Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("o valor não é texto: {other:?}"),
    }
}

fn array_of_texts(value: &Dynamic) -> Vec<String> {
    let Dynamic::Array(items) = value else { panic!("o valor não é um array: {value:?}") };
    items.to_vec().iter().map(text_of).collect()
}

/// `{ [scope]: value }` sobre o objeto de `inspect`.
fn with_scope(mut stub: TestConfigurationValue, scope: &str, value: Dynamic) -> TestConfigurationValue {
    match scope {
        "applicationValue" => stub.application_value = value,
        "userValue" => stub.user_value = value,
        "userLocalValue" => stub.user_local_value = value,
        "userRemoteValue" => stub.user_remote_value = value,
        "workspaceValue" => stub.workspace_value = value,
        "workspaceFolderValue" => stub.workspace_folder_value = value,
        other => panic!("escopo desconhecido: {other}"),
    }
    stub
}

// suite('GitHub Enterprise enrollment configuration')

#[test]
fn plural_takes_precedence_including_explicit_empty_and_absent_plural_falls_back_to_legacy() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let legacy = "https://legacy.ghe.com";
    let cases: Vec<Vec<(&str, Dynamic)>> = vec![
        vec![],
        vec![(LEGACY_SETTING, text(legacy))],
        vec![(LEGACY_SETTING, text(legacy)), (GITHUB_ENTERPRISE_URIS_SETTING, texts(&["https://b.ghe.com", "https://a.ghe.com"]))],
        vec![(LEGACY_SETTING, text(legacy)), (GITHUB_ENTERPRISE_URIS_SETTING, texts(&[]))],
    ];
    let results: Vec<Vec<String>> = cases.into_iter().map(|values| uris_of(get_configured_github_enterprise_uris(&*configuration(&guard, values), true, None))).collect();
    assert_eq!(results, vec![owned(&[]), owned(&[legacy]), owned(&["https://b.ghe.com", "https://a.ghe.com"]), owned(&[])]);
}

#[test]
fn schema_defaults_do_not_disable_the_deprecated_setting() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = configuration(&guard, vec![(LEGACY_SETTING, text("https://legacy.ghe.com"))]);
    service.stub_inspect(TestConfigurationValue { default_value: texts(&[]), value: texts(&[]), ..Default::default() });
    assert_eq!(uris_of(get_configured_github_enterprise_uris(&*service, true, None)), owned(&["https://legacy.ghe.com"]));
}

#[test]
fn an_explicit_host_list_shared_across_profiles_takes_precedence_over_the_deprecated_setting() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let uris = ["https://shared.ghe.com"];
    let service = configuration(&guard, vec![(LEGACY_SETTING, text("https://legacy.ghe.com")), (GITHUB_ENTERPRISE_URIS_SETTING, texts(&uris))]);
    service.stub_inspect(TestConfigurationValue { application_value: texts(&uris), value: texts(&uris), ..Default::default() });
    assert_eq!(uris_of(get_configured_github_enterprise_uris(&*service, true, None)), owned(&uris));
}

#[test]
fn invalid_plural_configuration_never_falls_back_to_the_deprecated_setting() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = configuration(&guard, vec![(LEGACY_SETTING, text("https://legacy.ghe.com")), (GITHUB_ENTERPRISE_URIS_SETTING, text("https://invalid.ghe.com"))]);
    let error = get_configured_github_enterprise_uris(&*service, true, None).expect_err("a configuração plural inválida lança");
    assert!(error.message.contains("must be an array"), "mensagem inesperada: {}", error.message);
}

#[test]
fn enrollment_appends_to_the_effective_host_list_without_overwriting_it_or_the_legacy_setting() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = configuration(&guard, vec![(LEGACY_SETTING, text("https://legacy.ghe.com"))]);
    service.stub_update_value(UpdateValueBehavior::SetsUserConfiguration);
    let service_in_body = service.clone();
    run_with_faked_timers(async move {
        let service = service_in_body;
        add_github_enterprise_uri(service.clone(), "https://second.ghe.com", true, None, None).await.expect("o enrolamento escreve");
        add_github_enterprise_uri(service.clone(), "https://third.ghe.com", true, None, None).await.expect("o enrolamento escreve");
        add_github_enterprise_uri(service.clone(), "https://third.ghe.com", true, None, None).await.expect("o enrolamento repetido não escreve");
        assert_eq!(
            (writes(&service), text_of(&service.get_value(Some(LEGACY_SETTING), None)), uris_of(get_configured_github_enterprise_uris(&*service, true, None))),
            (
                vec![
                    (GITHUB_ENTERPRISE_URIS_SETTING.to_owned(), owned(&["https://legacy.ghe.com", "https://second.ghe.com"]), ConfigurationTarget::User),
                    (GITHUB_ENTERPRISE_URIS_SETTING.to_owned(), owned(&["https://legacy.ghe.com", "https://second.ghe.com", "https://third.ghe.com"]), ConfigurationTarget::User),
                ],
                "https://legacy.ghe.com".to_owned(),
                owned(&["https://legacy.ghe.com", "https://second.ghe.com", "https://third.ghe.com"]),
            )
        );
    });
}

#[test]
fn enrollment_honors_an_explicitly_empty_workspace_host_list_instead_of_writing_a_shadowed_user_value() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = configuration(&guard, vec![(LEGACY_SETTING, text("https://legacy.ghe.com")), (GITHUB_ENTERPRISE_URIS_SETTING, texts(&[]))]);
    service.stub_inspect(TestConfigurationValue { workspace_value: texts(&[]), value: texts(&[]), ..Default::default() });
    service.stub_update_value(UpdateValueBehavior::Resolves);
    let service_in_body = service.clone();
    run_with_faked_timers(async move {
        add_github_enterprise_uri(service_in_body, "https://new.ghe.com", true, None, None).await.expect("o enrolamento escreve");
    });
    assert_eq!(writes(&service).first(), Some(&(GITHUB_ENTERPRISE_URIS_SETTING.to_owned(), owned(&["https://new.ghe.com"]), ConfigurationTarget::Workspace)));
}

#[test]
fn correction_replaces_only_the_selected_invalid_uri_and_preserves_the_latest_host_list() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = configuration(&guard, vec![(GITHUB_ENTERPRISE_URIS_SETTING, texts(&["https://first.ghe.com", "not-a-url"]))]);
    service.stub_update_value(UpdateValueBehavior::SetsUserConfiguration);
    let service_in_body = service.clone();
    run_with_faked_timers(async move {
        let service = service_in_body;
        service
            .set_user_configuration(GITHUB_ENTERPRISE_URIS_SETTING, texts(&["https://first.ghe.com", "not-a-url", "https://added.ghe.com"]), None)
            .await
            .expect("a configuração do usuário é gravada");
        add_github_enterprise_uri(service.clone(), "http://ghe.local:8080/Team", true, Some(LEGACY_SETTING), Some("not-a-url")).await.expect("a correção escreve");
        assert_eq!(array_of_texts(&service.get_value(Some(GITHUB_ENTERPRISE_URIS_SETTING), None)), owned(&["https://first.ghe.com", "https://added.ghe.com", "http://ghe.local:8080/Team"]));
    });
}

#[test]
fn correction_does_not_replace_a_different_entry_after_the_selected_invalid_uri_disappears() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = configuration(&guard, vec![(GITHUB_ENTERPRISE_URIS_SETTING, texts(&["https://first.ghe.com", "different invalid URI"]))]);
    service.stub_update_value(UpdateValueBehavior::SetsUserConfiguration);
    let service_in_body = service.clone();
    run_with_faked_timers(async move {
        let service = service_in_body;
        add_github_enterprise_uri(service.clone(), "https://fixed.ghe.com", true, Some(LEGACY_SETTING), Some("not-a-url")).await.expect("a correção escreve");
        assert_eq!(array_of_texts(&service.get_value(Some(GITHUB_ENTERPRISE_URIS_SETTING), None)), owned(&["https://first.ghe.com", "different invalid URI", "https://fixed.ghe.com"]));
    });
}

#[test]
fn correction_to_an_existing_host_removes_the_invalid_uri_without_duplicating_the_host() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = configuration(&guard, vec![(GITHUB_ENTERPRISE_URIS_SETTING, texts(&["https://first.ghe.com", "not-a-url"]))]);
    service.stub_update_value(UpdateValueBehavior::SetsUserConfiguration);
    let service_in_body = service.clone();
    run_with_faked_timers(async move {
        let service = service_in_body;
        add_github_enterprise_uri(service.clone(), "https://first.ghe.com", true, Some(LEGACY_SETTING), Some("not-a-url")).await.expect("a correção escreve");
        assert_eq!(array_of_texts(&service.get_value(Some(GITHUB_ENTERPRISE_URIS_SETTING), None)), owned(&["https://first.ghe.com"]));
    });
}

#[test]
fn invalid_replacement_urls_are_rejected_without_changing_configuration() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = configuration(&guard, vec![(GITHUB_ENTERPRISE_URIS_SETTING, texts(&["https://first.ghe.com", "not-a-url"]))]);
    service.stub_update_value(UpdateValueBehavior::Resolves);
    let service_in_body = service.clone();
    run_with_faked_timers(async move {
        let error = add_github_enterprise_uri(service_in_body.clone(), "still not a URL", true, Some(LEGACY_SETTING), Some("not-a-url"))
            .await
            .expect_err("o endereço inválido é rejeitado");
        assert!(error.message.contains("Invalid GitHub Enterprise instance URI"), "mensagem inesperada: {}", error.message);
        assert_eq!(service_in_body.update_value_calls().len(), 0);
    });
}

#[test]
fn untrusted_workspace_scopes_do_not_disable_the_legacy_user_host_or_redirect_enrollment() {
    for scope in ["workspaceValue", "workspaceFolderValue"] {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let legacy = "https://legacy.ghe.com";
        let service = configuration(&guard, vec![(LEGACY_SETTING, text(legacy)), (GITHUB_ENTERPRISE_URIS_SETTING, texts(&[]))]);
        service.stub_inspect(with_scope(TestConfigurationValue { default_value: texts(&[]), value: texts(&[]), ..Default::default() }, scope, texts(&[])));
        service.stub_update_value(UpdateValueBehavior::Resolves);
        let hosts = uris_of(get_configured_github_enterprise_uris(&*service, false, None));
        let service_in_body = service.clone();
        run_with_faked_timers(async move {
            add_github_enterprise_uri(service_in_body, "https://new.ghe.com", false, None, None).await.expect("o enrolamento escreve");
        });
        assert_eq!(
            (hosts, writes(&service)),
            (owned(&[legacy]), vec![(GITHUB_ENTERPRISE_URIS_SETTING.to_owned(), owned(&[legacy, "https://new.ghe.com"]), ConfigurationTarget::User)]),
            "untrusted {scope} does not disable the legacy user host or redirect enrollment"
        );
    }
}

#[test]
fn untrusted_workspace_preserves_explicit_user_scopes() {
    for scope in ["applicationValue", "userValue", "userLocalValue", "userRemoteValue"] {
        for uris in [vec![], vec!["https://user.ghe.com"]] {
            let guard = ensure_no_disposables_are_leaked_in_test_suite();
            let service = configuration(&guard, vec![(LEGACY_SETTING, text("https://legacy.ghe.com")), (GITHUB_ENTERPRISE_URIS_SETTING, texts(&uris))]);
            service.stub_inspect(with_scope(TestConfigurationValue { workspace_value: texts(&[]), value: texts(&uris), ..Default::default() }, scope, texts(&uris)));
            service.stub_update_value(UpdateValueBehavior::Resolves);
            let hosts = uris_of(get_configured_github_enterprise_uris(&*service, false, None));
            let service_in_body = service.clone();
            run_with_faked_timers(async move {
                add_github_enterprise_uri(service_in_body, "https://new.ghe.com", false, None, None).await.expect("o enrolamento escreve");
            });
            let mut expected_hosts = uris.clone();
            expected_hosts.push("https://new.ghe.com");
            assert_eq!(
                (hosts, writes(&service)),
                (owned(&uris), vec![(GITHUB_ENTERPRISE_URIS_SETTING.to_owned(), owned(&expected_hosts), ConfigurationTarget::User)]),
                "untrusted workspace preserves explicit {scope} ({uris:?})"
            );
        }
    }
}

// suite('GitHub Enterprise instance validation')

#[test]
fn accepts_cloud_and_server_urls_without_changing_path_case_ports_or_escapes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let values = ["https://tenant.ghe.com", "HTTP://GHE.LOCAL:8080/Team", "https://ghe.local:443/Team%20One%25/", "https://ghe.local/Team///"];
    assert_eq!(values.map(is_valid_github_enterprise_uri), values.map(|_| true));
}

#[test]
fn rejects_values_that_the_authentication_extension_cannot_use_as_instance_urls() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let values = [
        "", "not-a-url", "https:/ghe.local", "file:///ghe.local", "ftp://ghe.local",
        "https://github.com", "https://github.com.:443", "https://www.github.com", "https://api.github.com",
        "https://user:password@ghe.local", "https://ghe.local?query", "https://ghe.local#fragment",
        "https://ghe.local/a/../b", "https://ghe.local/a//b", "https://ghe.local:invalid",
    ];
    assert_eq!(values.map(is_valid_github_enterprise_uri), values.map(|_| false));
}

// suite('GitHub Enterprise session provenance')

/// `getGitHubEnterpriseUri(URI.parse(value))?.toString()`.
fn enterprise_uri_text(value: &str) -> Option<String> {
    get_github_enterprise_uri(Some(&URI::parse(value, false))).map(|uri| uri.to_string(false).to_string_lossy())
}

#[test]
fn a_missing_issuer_has_no_enterprise_base() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(get_github_enterprise_uri(None), None);
}

#[test]
fn derives_enterprise_cloud_and_server_bases_from_the_oauth_issuer() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let issuers = [
        "https://a.ghe.com/login/oauth",
        "https://b.ghe.com/login/oauth",
        "http://ghe.local:8080/Team/login/oauth",
        "HTTPS://GHE.LOCAL:443/Team/login/oauth",
        "https://ghe.local:443/team/login/oauth",
        "http://[::1]:8080/login/oauth",
    ];
    let bases: Vec<Option<String>> = issuers.into_iter().map(enterprise_uri_text).collect();
    let expected = ["https://a.ghe.com/", "https://b.ghe.com/", "http://ghe.local:8080/Team", "https://ghe.local:443/Team", "https://ghe.local:443/team", "http://[::1]:8080/"];
    assert_eq!(bases, expected.into_iter().map(|base| Some(base.to_owned())).collect::<Vec<_>>());
}

#[test]
fn preserves_escaped_base_paths_and_explicit_default_ports() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(enterprise_uri_text("https://GHE.LOCAL:443/Team%20One%25/login/oauth"), Some("https://ghe.local:443/Team%20One%25".to_owned()));
}

#[test]
fn public_github_issuers_do_not_identify_an_enterprise_base() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let issuers = ["https://github.com/login/oauth", "https://github.com.:443/login/oauth", "https://api.github.com/login/oauth", "https://www.github.com/login/oauth"];
    assert_eq!(issuers.map(|value| get_github_enterprise_uri(Some(&URI::parse(value, false)))), issuers.map(|_| None));
}

#[test]
fn rejects_invalid_or_non_oauth_provenance() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let issuers = [
        "https://ghe.local",
        "https://ghe.local/not-an-oauth-server",
        "ftp://ghe.local/login/oauth",
        "https://ghe.local:bad/login/oauth",
        "https://user@ghe.local/login/oauth",
        "https://ghe.local/base//login/oauth",
        "https://ghe.local/../base/login/oauth",
        "https://ghe.local/base/login/oauth?tenant=a",
        "https://ghe.local/base/login/oauth#fragment",
        "https://ghe.local/base/login/oauth//",
    ];
    assert_eq!(issuers.map(|value| get_github_enterprise_uri(Some(&URI::parse(value, false)))), issuers.map(|_| None));
}
