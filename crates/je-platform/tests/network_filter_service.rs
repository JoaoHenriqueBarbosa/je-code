//! Porte de `src/vs/platform/networkFilter/test/common/networkFilterService.test.ts` (suíte
//! `AgentNetworkFilterService`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` e o `disposables` do `setup` são o [`LeakGuard`] criado no
//!   começo de cada caso; o `TestConfigurationService` tem o emissor entregue ao guarda, como o
//!   `githubEnterprise.test.ts` faz.
//! - `fireConfigChange(key)` monta o [`ConfigurationChangeEvent`] real com a chave alterada e a origem `USER`
//!   (o literal do original só implementa `affectsConfiguration` por igualdade de chave).
//! - Os `for` que geram um `test(...)` por política (`deny-only` e `allow-all`) são um caso que percorre as
//!   políticas, com o nome na mensagem de falha.
//! - `String.raw` é um literal bruto do Rust; `assert.deepStrictEqual` sobre arrays e objetos é `assert_eq!`
//!   sobre `Vec` e tuplas.

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::*;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::uri::{URI, UriComponents};
use je_platform::configuration::ConfigurationTarget;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::network_filter::network_filter_service::{AgentNetworkFilterService, IAgentNetworkFilterService};
use je_platform::network_filter::settings::AgentNetworkDomainSettingId;
use je_platform::sandbox::sandbox_settings_resolution_helper::SandboxNetworkRestrictions;
use je_platform::sandbox::settings::{AgentSandboxEnabledValue, AgentSandboxSettingId};

use AgentNetworkDomainSettingId::{AllowedNetworkDomains, DeniedNetworkDomains, NetworkFilter};

/// O `setup`: o serviço de configuração com o filtro ligado e as duas listas vazias.
fn setup() -> (LeakGuard, Rc<TestConfigurationService>) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let config = TestConfigurationService::new(None);
    guard.add_rc(config.on_did_change_configuration_emitter.clone());
    set(&config, NetworkFilter, Dynamic::Bool(true));
    set(&config, AllowedNetworkDomains, dyn_strs(&[]));
    set(&config, DeniedNetworkDomains, dyn_strs(&[]));
    (guard, config)
}

/// `configService.setUserConfiguration(AgentNetworkDomainSettingId.X, value)`.
fn set(config: &TestConfigurationService, id: AgentNetworkDomainSettingId, value: Dynamic) {
    let _ = config.set_user_configuration(id.as_str(), value, None);
}

fn set_domains(config: &TestConfigurationService, id: AgentNetworkDomainSettingId, domains: &[&str]) {
    set(config, id, dyn_strs(domains));
}

/// `createService()`.
fn create_service(guard: &LeakGuard, config: &Rc<TestConfigurationService>) -> Rc<AgentNetworkFilterService> {
    guard.add_rc(AgentNetworkFilterService::new(config.clone()))
}

/// `fireConfigChange(key)`.
fn fire_config_change(config: &TestConfigurationService, key: &str) {
    let event = ConfigurationChangeEvent::new(change(&[key], &[]), None, Rc::new(empty_test_configuration()), None, null_log());
    event.source.set(Some(ConfigurationTarget::User));
    config.on_did_change_configuration_emitter.fire(Rc::new(event));
}

fn uri(text: &str) -> URI {
    URI::parse(text, false)
}

/// `URI.from({ scheme, authority, path })`.
fn uri_from(scheme: &str, authority: Option<&str>, path: &str) -> URI {
    URI::from(
        &UriComponents { scheme: Some(JsString::from(scheme)), authority: authority.map(JsString::from), path: Some(JsString::from(path)), ..UriComponents::default() },
        false,
    )
}

/// `urls.map(url => service.isUriAllowed(URI.parse(url)))`.
fn allowed_of(service: &AgentNetworkFilterService, urls: &[&str]) -> Vec<bool> {
    urls.iter().map(|url| service.is_uri_allowed(&uri(url), None)).collect()
}

fn restrictions(sandbox_enabled: bool, allow_network: bool, allowed: &[&str], denied: &[&str]) -> SandboxNetworkRestrictions {
    SandboxNetworkRestrictions {
        sandbox_enabled,
        allow_network,
        allowed_domains: allowed.iter().map(|domain| (*domain).to_owned()).collect(),
        denied_domains: denied.iter().map(|domain| (*domain).to_owned()).collect(),
    }
}

#[test]
fn allows_all_domains_when_filter_is_disabled_regardless_of_configured_lists() {
    let (guard, config) = setup();
    set(&config, NetworkFilter, Dynamic::Bool(false));
    set_domains(&config, AllowedNetworkDomains, &["example.com"]);
    set_domains(&config, DeniedNetworkDomains, &["blocked.com"]);

    let service = create_service(&guard, &config);

    assert!(service.is_uri_allowed(&uri("https://example.com"), None));
    assert!(service.is_uri_allowed(&uri("https://anything.test"), None));
    assert!(service.is_uri_allowed(&uri("https://blocked.com"), None));
}

#[test]
fn explicit_browser_sandbox_restrictions_reuse_the_domain_filter_without_enabling_network_filter() {
    let (guard, config) = setup();
    set(&config, NetworkFilter, Dynamic::Bool(false));
    let service = create_service(&guard, &config);
    let restrictions = restrictions(true, true, &["*.example.com"], &["private.example.com"]);
    let urls = ["https://example.com", "https://docs.example.com", "https://private.example.com", "https://other.com"];
    assert_eq!(
        (service.is_enabled(Some(&restrictions)), urls.iter().map(|url| service.is_uri_allowed(&uri(url), Some(&restrictions))).collect::<Vec<_>>()),
        (true, vec![true, true, false, false])
    );
}

#[test]
fn explicit_browser_sandbox_outbound_access_without_an_allow_list_permits_non_denied_hosts() {
    let (guard, config) = setup();
    set(&config, NetworkFilter, Dynamic::Bool(false));
    let service = create_service(&guard, &config);
    let restrictions = restrictions(true, true, &[], &[]);
    let without_lists = service.is_uri_allowed(&uri("https://example.com"), Some(&restrictions));
    let with_denied_restrictions = SandboxNetworkRestrictions { denied_domains: vec!["evil.com".to_owned()], ..restrictions.clone() };
    let with_denied_list: Vec<bool> = ["https://example.com", "https://evil.com"].iter().map(|url| service.is_uri_allowed(&uri(url), Some(&with_denied_restrictions))).collect();
    assert_eq!((without_lists, with_denied_list), (true, vec![true, false]));
}

#[test]
fn copilot_session_restrictions_do_not_affect_other_harnesses_or_sessions() {
    let (guard, config) = setup();
    set(&config, NetworkFilter, Dynamic::Bool(false));
    set(&config, AllowedNetworkDomains, dyn_strs(&["example.com"]));
    let _ = config.set_user_configuration(AgentSandboxSettingId::AgentSandboxEnabled.as_str(), s(AgentSandboxEnabledValue::Off.as_str()), None);
    let _ = config.set_user_configuration(AgentSandboxSettingId::AgentSandboxAllowNetwork.as_str(), Dynamic::Bool(true), None);
    let service = create_service(&guard, &config);
    let restrictions = restrictions(true, true, &["example.com"], &[]);
    let snapshot = |values: Option<&SandboxNetworkRestrictions>| {
        (service.is_enabled(values), ["https://example.com", "https://other.com"].iter().map(|url| service.is_uri_allowed(&uri(url), values)).collect::<Vec<_>>())
    };
    let without_network = SandboxNetworkRestrictions { allow_network: false, ..restrictions.clone() };
    let states = vec![snapshot(None), snapshot(Some(&restrictions)), snapshot(Some(&without_network)), snapshot(None)];
    assert_eq!(
        states,
        vec![
            (false, vec![true, true]),
            (true, vec![true, false]),
            (true, vec![false, false]),
            (false, vec![true, true]),
        ]
    );
}

#[test]
fn denies_all_domains_when_both_lists_are_empty() {
    let (guard, config) = setup();
    let service = create_service(&guard, &config);
    assert!(!service.is_uri_allowed(&uri("https://example.com"), None));
    assert!(!service.is_uri_allowed(&uri("https://anything.test"), None));
}

#[test]
fn only_explicit_browser_restrictions_enable_sandbox_filtering_without_caching_across_calls() {
    let (guard, config) = setup();
    set(&config, NetworkFilter, Dynamic::Bool(false));
    let _ = config.set_user_configuration(AgentSandboxSettingId::AgentSandboxEnabled.as_str(), s(AgentSandboxEnabledValue::On.as_str()), None);
    let _ = config.set_user_configuration(AgentSandboxSettingId::AgentSandboxAllowNetwork.as_str(), Dynamic::Bool(false), None);
    let service = create_service(&guard, &config);
    let target = uri("https://example.com");
    let restrictions = restrictions(true, true, &[], &[]);
    assert_eq!(
        vec![
            service.is_uri_allowed(&target, None),
            service.is_uri_allowed(&target, Some(&restrictions)),
            service.is_uri_allowed(&target, Some(&SandboxNetworkRestrictions { denied_domains: vec!["example.com".to_owned()], ..restrictions.clone() })),
            service.is_uri_allowed(&target, Some(&SandboxNetworkRestrictions { sandbox_enabled: false, ..restrictions.clone() })),
            service.is_uri_allowed(&uri("wss://example.com"), Some(&SandboxNetworkRestrictions { allow_network: false, ..restrictions.clone() })),
            service.is_uri_allowed(&uri_file("C:\\test.txt"), Some(&SandboxNetworkRestrictions { allow_network: false, ..restrictions.clone() })),
        ],
        vec![true, true, false, true, false, true]
    );
}

#[test]
fn blocks_denied_domains() {
    let (guard, config) = setup();
    set_domains(&config, DeniedNetworkDomains, &["evil.com"]);
    let service = create_service(&guard, &config);
    assert!(!service.is_uri_allowed(&uri("https://evil.com"), None));
    assert!(service.is_uri_allowed(&uri("https://good.com"), None));
}

#[test]
fn restricts_to_allowed_domains() {
    let (guard, config) = setup();
    set_domains(&config, AllowedNetworkDomains, &["example.com"]);
    let service = create_service(&guard, &config);
    assert!(service.is_uri_allowed(&uri("https://example.com"), None));
    assert!(!service.is_uri_allowed(&uri("https://other.com"), None));
}

#[test]
fn denied_takes_precedence_over_allowed() {
    let (guard, config) = setup();
    set_domains(&config, AllowedNetworkDomains, &["*.com"]);
    set_domains(&config, DeniedNetworkDomains, &["evil.com"]);
    let service = create_service(&guard, &config);
    assert!(service.is_uri_allowed(&uri("https://safe.com"), None));
    assert!(!service.is_uri_allowed(&uri("https://evil.com"), None));
}

// suite('isUriAllowed')

#[test]
fn blocks_unicode_wildcard_deny_patterns_with_each_policy() {
    for (name, allowed_domains) in [("deny-only", &[][..]), ("allow-all", &["*"][..])] {
        let (guard, config) = setup();
        set_domains(&config, AllowedNetworkDomains, allowed_domains);
        set_domains(&config, DeniedNetworkDomains, &["*.b\u{fc}cher.de"]);
        let service = create_service(&guard, &config);
        let urls = [
            "https://xn--bcher-kva.de/",
            "https://sub.xn--bcher-kva.de/",
            "https://xn--bcher-kva.de/private",
            "https://sub.xn--bcher-kva.de/private",
            "https://b\u{fc}cher.de/private",
            "https://sub.b\u{fc}cher.de/private",
            "https://example.com/private",
            "https://notxn--bcher-kva.de/private",
            "https://xn--bcher-kva.de.example.com/private",
        ];

        assert_eq!(
            (service.is_enabled(None), allowed_of(&service, &urls)),
            (true, vec![false, false, false, false, false, false, true, true, true]),
            "política {name}"
        );
    }
}

#[test]
fn allows_unicode_wildcard_allow_patterns_without_allowing_unrelated_domains() {
    let (guard, config) = setup();
    set_domains(&config, AllowedNetworkDomains, &["*.b\u{fc}cher.de"]);
    let service = create_service(&guard, &config);
    let urls = [
        "https://xn--bcher-kva.de/private",
        "https://sub.xn--bcher-kva.de/private",
        "https://b\u{fc}cher.de/private",
        "https://sub.b\u{fc}cher.de/private",
        "https://example.com/private",
        "https://notxn--bcher-kva.de/private",
        "https://xn--bcher-kva.de.example.com/private",
    ];

    assert_eq!(allowed_of(&service, &urls), vec![true, true, true, true, false, false, false]);
}

#[test]
fn allows_file_uris() {
    let (guard, config) = setup();
    let service = create_service(&guard, &config);
    set_domains(&config, DeniedNetworkDomains, &["*"]);
    assert!(service.is_uri_allowed(&uri_file("/tmp/test.txt"), None));
}

#[test]
fn allows_uris_without_authority() {
    let (guard, config) = setup();
    let service = create_service(&guard, &config);
    set_domains(&config, DeniedNetworkDomains, &["*"]);
    assert!(service.is_uri_allowed(&uri_from("untitled", None, "Untitled-1"), None));
}

#[test]
fn fails_closed_for_reported_http_s_parser_differential_urls_with_empty_authorities() {
    let (guard, config) = setup();
    set_domains(&config, AllowedNetworkDomains, &["*"]);
    let service = create_service(&guard, &config);
    let urls = [
        r"http:\\\\evil.example/x",
        r"http:/\\/\\evil.example/x",
        r"http:\\/evil.example/x",
        r"http:\\evil.example/x",
        r"https:\\evil.example/x",
    ];

    let results: Vec<(String, String, bool)> = urls
        .iter()
        .map(|url| {
            let parsed = uri(url);
            (parsed.scheme().to_string_lossy(), parsed.authority().to_string_lossy(), service.is_uri_allowed(&parsed, None))
        })
        .collect();
    let expected: Vec<(String, String, bool)> = ["http", "http", "http", "http", "https"].iter().map(|scheme| ((*scheme).to_owned(), String::new(), false)).collect();
    assert_eq!(results, expected);
}

#[test]
fn fails_closed_for_web_socket_parser_differential_urls_with_empty_authorities() {
    let (guard, config) = setup();
    set_domains(&config, AllowedNetworkDomains, &["*"]);
    let service = create_service(&guard, &config);
    let urls = [r"ws:\\evil.example/socket", r"wss:\evil.example/socket"];

    let results: Vec<(String, String, bool)> = urls
        .iter()
        .map(|url| {
            let parsed = uri(url);
            (parsed.scheme().to_string_lossy(), parsed.authority().to_string_lossy(), service.is_uri_allowed(&parsed, None))
        })
        .collect();
    assert_eq!(results, vec![("ws".to_owned(), String::new(), false), ("wss".to_owned(), String::new(), false)]);
}

#[test]
fn checks_domain_for_http_https_uris() {
    let (guard, config) = setup();
    set_domains(&config, AllowedNetworkDomains, &["example.com"]);
    let service = create_service(&guard, &config);
    assert!(service.is_uri_allowed(&uri("https://example.com/page"), None));
    assert!(!service.is_uri_allowed(&uri("https://other.com/page"), None));
}

#[test]
fn allows_explicitly_configured_local_hosts() {
    let (guard, config) = setup();
    set_domains(&config, AllowedNetworkDomains, &["localhost", "*.localhost", "127.0.0.1", "0.0.0.0", "::1"]);
    let service = create_service(&guard, &config);
    assert_eq!(
        allowed_of(
            &service,
            &[
                "http://localhost:3000",
                "http://sub.localhost:3000",
                "http://127.0.0.1:3000",
                "http://0.0.0.0:3000",
                "http://[::1]:3000",
                "http://other.internal:3000",
            ]
        ),
        vec![true, true, true, true, true, false]
    );
}

#[test]
fn blocks_parser_differential_authorities_denied_by_policy() {
    let (guard, config) = setup();
    set_domains(&config, AllowedNetworkDomains, &["github.com"]);
    set_domains(&config, DeniedNetworkDomains, &["127.0.0.1", "169.254.169.254", "evil.com", "*.evil.com"]);
    let service = create_service(&guard, &config);
    assert_eq!(
        allowed_of(
            &service,
            &[
                "http://127.0.0.1:8931/control",
                "http://a@b@127.0.0.1:8931/private",
                "http://a%40b@127.0.0.1:8931/private",
                "http://[::1]:8931/private",
                "http://[::ffff:169.254.169.254]/private",
                "https://attacker.evil.com/private",
            ]
        ),
        vec![false; 6]
    );
}

#[test]
fn blocks_alternative_ipv4_forms_denied_by_policy() {
    let (guard, config) = setup();
    set_domains(&config, DeniedNetworkDomains, &["127.1", "0300.0250.01.01", "0xa9fea9fe"]);
    let service = create_service(&guard, &config);
    assert_eq!(
        allowed_of(&service, &["http://127.0.0.1/private", "http://192.168.1.1/private", "http://169.254.169.254/private"]),
        vec![false; 3]
    );
}

#[test]
fn blocks_ipv4_mapped_ipv6_literals_in_a_deny_only_configuration() {
    let (guard, config) = setup();
    set_domains(&config, DeniedNetworkDomains, &["127.0.0.1", "169.254.169.254"]);
    let service = create_service(&guard, &config);
    assert_eq!(
        allowed_of(
            &service,
            &[
                "http://[::ffff:127.0.0.1]/private",
                "http://[::ffff:7f00:1]/private",
                "http://[::ffff:169.254.169.254]/private",
                "http://[::ffff:a9fe:a9fe]/private",
            ]
        ),
        vec![false; 4]
    );
}

#[test]
fn blocks_bare_administrator_deny_patterns_outside_well_known_public_suffixes() {
    let (guard, config) = setup();
    set_domains(
        &config,
        DeniedNetworkDomains,
        &[
            "evil.xyz",
            "evil.co.uk",
            "evil.ru",
            "evil.app",
            "evil.cn",
            "evil.info",
            "evil.biz",
            "evil.de",
            "evil.jp",
            "evil.site",
            "evil.zip",
            "metadata.internal",
            "host.local",
            "db.corp",
            "srv.lan",
            "169.254.169.254",
            "10.0.0.5",
            "127.0.0.1",
            "192.168.1.1",
            "localhost",
            "metadata",
            "metadata.google.internal",
        ],
    );
    let service = create_service(&guard, &config);
    assert_eq!(
        allowed_of(
            &service,
            &[
                "https://evil.xyz/private",
                "https://evil.co.uk/private",
                "https://evil.ru/private",
                "https://evil.app/private",
                "https://evil.cn/private",
                "https://evil.info/private",
                "https://evil.biz/private",
                "https://evil.de/private",
                "https://evil.jp/private",
                "https://evil.site/private",
                "https://evil.zip/private",
                "https://metadata.internal/private",
                "https://host.local/private",
                "https://db.corp/private",
                "https://srv.lan/private",
                "http://169.254.169.254/private",
                "http://10.0.0.5/private",
                "http://127.0.0.1/private",
                "http://192.168.1.1/private",
                "http://localhost/private",
                "http://metadata/private",
                "https://metadata.google.internal/private",
            ]
        ),
        vec![false; 22]
    );
}

#[test]
fn blocks_wildcard_administrator_deny_patterns_outside_well_known_public_suffixes() {
    let (guard, config) = setup();
    set_domains(&config, DeniedNetworkDomains, &["*.internal", "*.corp", "*.lan", "*.corp.local", "*.co.uk", "*.example.co.uk"]);
    let service = create_service(&guard, &config);
    assert_eq!(
        allowed_of(
            &service,
            &[
                "https://service.internal/private",
                "https://service.corp/private",
                "https://service.lan/private",
                "https://service.corp.local/private",
                "https://service.co.uk/private",
                "https://service.example.co.uk/private",
            ]
        ),
        vec![false; 6]
    );
}

#[test]
fn blocks_mapped_and_compatible_ipv6_forms_of_wildcard_ipv4_deny_patterns() {
    let (guard, config) = setup();
    set_domains(&config, DeniedNetworkDomains, &["*.127.1", "*.0300.0250.01.01", "*.0xa9fea9fe"]);
    let service = create_service(&guard, &config);
    assert_eq!(
        allowed_of(
            &service,
            &[
                "http://[::ffff:127.0.0.1]/private",
                "http://[::127.0.0.1]/private",
                "http://[::ffff:192.168.1.1]/private",
                "http://[::ffff:169.254.169.254]/private",
                "http://[::1]/allowed",
            ]
        ),
        vec![false, false, false, false, true]
    );
}

#[test]
fn blocks_percent_encoded_path_separators_in_denied_authorities() {
    let (guard, config) = setup();
    set_domains(&config, DeniedNetworkDomains, &["evil.com"]);
    let service = create_service(&guard, &config);
    assert_eq!(allowed_of(&service, &["https://evil.com/x", "https://evil.com%2fx/", "https://evil.com%5c/"]), vec![false; 3]);
}

#[test]
fn denies_ipv6_literals_when_both_domain_lists_are_empty() {
    let (guard, config) = setup();
    let service = create_service(&guard, &config);
    assert_eq!(
        allowed_of(
            &service,
            &[
                "http://[::1]:3000/private",
                "http://[0:0:0:0:0:0:0:1]/private",
                "http://[::ffff:127.0.0.1]/private",
                "http://[::ffff:7f00:1]/private",
                "https://[2001:db8::1]/private",
                "https://[fe80::1]/private",
            ]
        ),
        vec![false; 6]
    );
}

#[test]
fn does_not_allow_ipv6_literals_through_a_dns_only_allowlist() {
    let (guard, config) = setup();
    set_domains(&config, AllowedNetworkDomains, &["github.com"]);
    let service = create_service(&guard, &config);
    assert_eq!(allowed_of(&service, &["https://github.com", "https://[2001:db8::1]", "http://[::ffff:127.0.0.1]"]), vec![true, false, false]);
}

#[test]
fn matches_explicit_ipv6_allow_and_deny_patterns() {
    let (guard, config) = setup();
    set_domains(&config, AllowedNetworkDomains, &["[::1]", "[2001:db8::1]"]);
    set_domains(&config, DeniedNetworkDomains, &["[0:0:0:0:0:0:0:1]"]);
    let service = create_service(&guard, &config);
    assert_eq!(allowed_of(&service, &["http://[::1]", "https://[2001:0db8:0:0:0:0:0:1]", "http://[::ffff:127.0.0.1]"]), vec![false, true, false]);
}

#[test]
fn blocks_unbracketed_ipv6_administrator_deny_patterns() {
    let (guard, config) = setup();
    set_domains(&config, DeniedNetworkDomains, &["::1", "0:0:0:0:0:0:0:1", "::ffff:127.0.0.1", "fe80::1"]);
    let service = create_service(&guard, &config);
    assert_eq!(allowed_of(&service, &["http://[::1]/private", "http://[::ffff:127.0.0.1]/private", "http://[fe80::1]/private"]), vec![false; 3]);
}

#[test]
fn fails_closed_for_malformed_non_empty_http_authorities() {
    let (guard, config) = setup();
    set_domains(&config, AllowedNetworkDomains, &["*"]);
    let service = create_service(&guard, &config);
    assert_eq!(
        vec![
            service.is_uri_allowed(&uri_from("http", Some("[::1"), "/"), None),
            service.is_uri_allowed(&uri_from("https", Some("::1]"), "/"), None),
            service.is_uri_allowed(&uri_from("http", Some("[::1]extra"), "/"), None),
            service.is_uri_allowed(&uri_from("http", Some("[fe80::1%25eth0]"), "/"), None),
            service.is_uri_allowed(&uri_from("HTTP", Some("[::1"), "/"), None),
            service.is_uri_allowed(&uri("Https://allowed.com%2F@evil.com/private"), None),
        ],
        vec![false; 6]
    );
}

#[test]
fn fails_closed_for_malformed_non_empty_web_socket_authorities() {
    let (guard, config) = setup();
    set_domains(&config, AllowedNetworkDomains, &["*"]);
    let service = create_service(&guard, &config);
    assert_eq!(
        vec![
            service.is_uri_allowed(&uri_from("ws", Some("[::1"), "/"), None),
            service.is_uri_allowed(&uri_from("wss", Some("::1]"), "/"), None),
            service.is_uri_allowed(&uri_from("WS", Some("[::1]extra"), "/"), None),
        ],
        vec![false; 3]
    );
}

#[test]
fn rejects_url_authorities_whose_decoded_representation_changes_the_host() {
    let (guard, config) = setup();
    set_domains(&config, AllowedNetworkDomains, &["allowed.com"]);
    let service = create_service(&guard, &config);
    assert!(!service.is_uri_allowed(&uri("http://169.254.169.254%40allowed.com/private"), None));
}

#[test]
fn fires_on_did_change_when_configuration_changes() {
    let (guard, config) = setup();
    let service = create_service(&guard, &config);
    let fired = Rc::new(Cell::new(false));
    let observed = fired.clone();
    guard.add(service.on_did_change().listen(move |_| observed.set(true)));

    set_domains(&config, AllowedNetworkDomains, &["example.com"]);
    fire_config_change(&config, AllowedNetworkDomains.as_str());

    assert!(fired.get());
}

#[test]
fn updates_filtering_after_configuration_change() {
    let (guard, config) = setup();
    set_domains(&config, AllowedNetworkDomains, &["example.com"]);
    let service = create_service(&guard, &config);
    assert!(service.is_uri_allowed(&uri("https://example.com"), None));

    set_domains(&config, DeniedNetworkDomains, &["example.com"]);
    fire_config_change(&config, DeniedNetworkDomains.as_str());

    assert!(!service.is_uri_allowed(&uri("https://example.com"), None));
}
