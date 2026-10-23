//! Porte de `src/vs/platform/url/test/common/trustedDomains.test.ts` (suíte `trustedDomains`), caso a caso e na mesma ordem.

mod common;

use common::*;
use je_base::JsString;
use je_base::uri::URI;
use je_platform::url::trusted_domains::{
    is_all_interfaces_authority, is_localhost_authority, is_url_domain_trusted_str as trusted, normalize_url,
};

fn uri(text: &str) -> URI {
    URI::parse(text, false)
}

fn unit_text(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn localhost(authority: &str) -> bool {
    is_localhost_authority(&unit_text(authority))
}

fn all_interfaces(authority: &str) -> bool {
    is_all_interfaces_authority(&unit_text(authority))
}

#[test]
fn localhost_is_always_trusted() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(trusted(&uri("http://localhost:3000"), &[]));
    assert!(trusted(&uri("http://127.0.0.1:3000"), &[]));
    assert!(trusted(&uri("http://subdomain.localhost"), &[]));
    assert!(trusted(&uri("https://[::1]"), &[]));
    assert!(trusted(&uri("http://[::1]:3000"), &[]));
}

#[test]
fn backslashes_are_treated_as_url_path_separators() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!trusted(&uri("https://example.com\\.localhost"), &[]));
    assert!(!trusted(&uri("https://example.com\\.github.com"), &["https://*.github.com"]));
}

#[test]
fn percent_encoded_backslash_user_information_is_not_implicitly_trusted_as_localhost() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!trusted(&uri("https://localhost%5C@evil.example/"), &[]));
}

#[test]
fn percent_encoded_backslash_user_information_does_not_match_configured_trusted_domains() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        [
            trusted(&uri("https://example.com%5C@evil.example/"), &["https://example.com"]),
            trusted(&uri("https://api.github.com%5C@evil.example/"), &["https://*.github.com"]),
        ],
        [false, false]
    );
}

#[test]
fn encoded_user_information_does_not_match_implicit_or_configured_trusted_domains() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for separator in ["%2F", "%2f", "%5c", "%25%32%46", "%25%32%66", "%25%35%43", "%25%35%63"] {
        assert!(!trusted(&uri(&format!("https://localhost{separator}@evil.example/")), &[]), "localhost {separator}");
        assert!(!trusted(&uri(&format!("https://example.com{separator}@evil.example/")), &["https://example.com"]), "exact {separator}");
        assert!(!trusted(&uri(&format!("https://api.example.com{separator}@evil.example/")), &["https://*.example.com"]), "wildcard {separator}");
    }
}

#[test]
fn authorityless_http_urls_cannot_become_trusted_through_backslash_normalization() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let urls = [
        r"https:\localhost%2F@evil.example/resource",
        r"https:/\localhost%25%32%66@evil.example/resource",
        r"https:\example.com%5C@evil.example/resource",
        r"https:/\api.example.com%25%35%43@evil.example/resource",
    ];
    for url in urls {
        assert!(!trusted(&uri(url), &[]), "localhost {url}");
        assert!(!trusted(&uri(url), &["https://example.com"]), "exact {url}");
        assert!(!trusted(&uri(url), &["https://*.example.com"]), "wildcard {url}");
        assert!(trusted(&uri(url), &["*"]), "explicitTrustAll {url}");
    }
}

#[test]
fn bare_wildcard_explicitly_trusts_urls_with_encoded_user_information() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(trusted(&uri("https://example.com%2F@evil.example/"), &["*"]));
}

#[test]
fn wildcard_matches_everything() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(trusted(&uri("https://example.com"), &["*"]));
    assert!(trusted(&uri("http://anything.org"), &["*"]));
    assert!(trusted(&uri("https://github.com/microsoft"), &["*"]));
}

#[test]
fn exact_domain_match() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(trusted(&uri("https://example.com"), &["https://example.com"]));
    assert!(trusted(&uri("https://example.com/path"), &["https://example.com"]));
    assert!(!trusted(&uri("http://example.com"), &["https://example.com"]));
}

#[test]
fn subdomain_wildcard_matching() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(trusted(&uri("https://api.github.com"), &["https://*.github.com"]));
    assert!(trusted(&uri("https://github.com"), &["https://*.github.com"]));
    assert!(trusted(&uri("https://sub.api.github.com"), &["https://*.github.com"]));
}

#[test]
fn idn_trusted_domains_match_equivalent_spellings_without_trusting_unrelated_hosts() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hosts = ["bücher.example.test", "xn--bcher-kva.example.test"];
    let patterns = ["bücher.example.test", "xn--bcher-kva.example.test"];
    for host in hosts {
        for pattern in patterns {
            let exact = format!("https://{pattern}");
            let wildcard = format!("https://*.{pattern}");
            assert!(trusted(&uri(&format!("https://{host}/page")), &[&exact]), "exact {host} {pattern}");
            assert!(trusted(&uri(&format!("https://x.{host}/page")), &[&wildcard]), "wildcard {host} {pattern}");
            assert!(!trusted(&uri("https://other.example.test"), &[&wildcard]), "unrelated {host} {pattern}");
            assert!(!trusted(&uri(&format!("https://{host}.evil.test")), &[&wildcard]), "suffix {host} {pattern}");
            assert!(!trusted(&uri(&format!("https://{host}%2F@evil.test")), &[&wildcard]), "userInfo {host} {pattern}");
            assert!(
                !trusted(&uri(&format!("https://{host}/allowed/../outside")), &[&format!("https://{pattern}/allowed")]),
                "outsidePath {host} {pattern}"
            );
        }
    }
}

#[test]
fn path_matching() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(trusted(&uri("https://example.com/api/v1"), &["https://example.com/api/*"]));
    // O caminho sem conteúdo final não casa com um padrão com curinga que pede mais segmentos
    assert!(!trusted(&uri("https://example.com/api"), &["https://example.com/api/*"]));
}

#[test]
fn path_scoped_trust_matches_the_resolved_destination() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let paths = [
        "/allowed/../outside",
        "/allowed/%2e%2e/outside",
        "/allowed/.\t./outside",
        "/allowed/.\n./outside",
        "/allowed/.\r./outside",
        "/outside",
        "/allowed/child/../page",
    ];
    let rules = ["https://example.com/allowed", "https://example.com/allowed/*"];
    let expected = [[false, false], [false, false], [false, false], [false, false], [false, false], [false, false], [true, true]];
    for (path, expected) in paths.iter().zip(expected) {
        let actual = rules.map(|rule| trusted(&uri(&format!("https://example.com{path}")), &[rule]));
        assert_eq!(actual, expected, "{path:?}");
    }
}

#[test]
fn unpaired_surrogates_preserve_path_scoped_trust_decisions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let trusted_domains = [JsString::from("https://example.com/allowed")];
    for surrogate in [0xD800u16, 0xDC00] {
        let build = |prefix: &str, suffix: &str| {
            let mut text = JsString::from(prefix);
            text.push_unit(surrogate);
            text.push_units(&unit_text(suffix));
            URI::parse(text, false)
        };
        assert!(je_platform::url::is_url_domain_trusted(&build("https://example.com/allowed/", "/page"), &trusted_domains));
        assert!(!je_platform::url::is_url_domain_trusted(&build("https://example.com/allowed/", "/../../outside"), &trusted_domains));
    }
}

#[test]
fn resolving_paths_preserves_user_information_guards_and_explicit_trust_all() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let url = uri("https://example.com:user@other.example/allowed/../outside");
    assert_eq!(
        [
            trusted(&url, &["https://example.com:*"]),
            trusted(&url, &["*"]),
            trusted(&uri("https://example.com/allowed/../outside"), &[]),
        ],
        [false, true, false]
    );
}

#[test]
fn scheme_must_match() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!trusted(&uri("https://example.com"), &["http://example.com"]));
    assert!(!trusted(&uri("http://example.com"), &["https://example.com"]));
}

#[test]
fn not_trusted_when_no_match() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!trusted(&uri("https://example.com"), &["https://other.com"]));
    assert!(!trusted(&uri("https://example.com"), &[]));
}

#[test]
fn multiple_trusted_domains() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let domains = ["https://github.com", "https://microsoft.com"];
    assert!(trusted(&uri("https://github.com"), &domains));
    assert!(trusted(&uri("https://microsoft.com"), &domains));
    assert!(!trusted(&uri("https://google.com"), &domains));
}

#[test]
fn case_normalization_for_github() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(trusted(&uri("https://github.com/Microsoft/VSCode"), &["https://github.com/microsoft/vscode"]));
    assert!(trusted(&uri("https://github.com/microsoft/vscode"), &["https://github.com/Microsoft/VSCode"]));
}

#[test]
fn normalizes_github_com_urls_to_lowercase_path() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(normalize_url("https://github.com/Microsoft/VSCode"), "https://github.com/microsoft/vscode");
    assert_eq!(normalize_url("https://github.com/OWNER/REPO"), "https://github.com/owner/repo");
}

#[test]
fn does_not_normalize_non_github_urls() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(normalize_url("https://example.com/Path/To/Resource"), "https://example.com/Path/To/Resource");
    assert_eq!(normalize_url("https://microsoft.com/Products"), "https://microsoft.com/Products");
}

#[test]
fn handles_uri_objects() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let value = uri("https://github.com/Microsoft/VSCode");
    assert_eq!(normalize_url(&value), "https://github.com/microsoft/vscode");
}

#[test]
fn handles_invalid_uris_gracefully() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // O original só confere que o resultado é uma string: o tipo de retorno já garante.
    let _result: JsString = normalize_url("not-a-valid-uri");
}

#[test]
fn recognizes_localhost() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(localhost("localhost"));
    assert!(localhost("localhost:3000"));
    assert!(localhost("localhost:8080"));
}

#[test]
fn recognizes_subdomains_of_localhost() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(localhost("subdomain.localhost"));
    assert!(localhost("api.localhost:3000"));
    assert!(localhost("a.b.c.localhost"));
}

#[test]
fn recognizes_127_0_0_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(localhost("127.0.0.1"));
    assert!(localhost("127.0.0.1:3000"));
    assert!(localhost("127.0.0.1:8080"));
}

#[test]
fn case_insensitive_for_localhost() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(localhost("LOCALHOST"));
    assert!(localhost("LocalHost:3000"));
    assert!(localhost("SUB.LOCALHOST"));
}

#[test]
fn recognizes_ipv6_localhost_and_the_full_form() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(localhost("[::1]"));
    assert!(localhost("[::1]:3000"));
    assert!(localhost("[::1]:8080"));
    assert!(localhost("[0:0:0:0:0:0:0:1]"));
    assert!(localhost("[0:0:0:0:0:0:0:1]:3000"));
    assert!(localhost("[0:0:0:0:0:0:0:1]:8080"));
}

#[test]
fn does_not_match_non_localhost_authorities() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!localhost("example.com"));
    assert!(!localhost("notlocalhost.com"));
    assert!(!localhost("127.0.0.2"));
    assert!(!localhost("192.168.1.1"));
    assert!(!localhost("[::]"));
    assert!(!localhost("[::2]"));
    assert!(!localhost("[::1"));
}

#[test]
fn recognizes_0_0_0_0() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(all_interfaces("0.0.0.0"));
    assert!(all_interfaces("0.0.0.0:3000"));
    assert!(all_interfaces("0.0.0.0:8080"));
}

#[test]
fn recognizes_ipv6_all_interfaces() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(all_interfaces("[::]"));
    assert!(all_interfaces("[::]:3000"));
    assert!(all_interfaces("[::]:8080"));
}

#[test]
fn recognizes_full_form_ipv6_all_interfaces() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(all_interfaces("[0:0:0:0:0:0:0:0]"));
    assert!(all_interfaces("[0:0:0:0:0:0:0:0]:3000"));
}

#[test]
fn does_not_match_localhost_or_other_non_all_interfaces_authorities() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!all_interfaces("localhost"));
    assert!(!all_interfaces("127.0.0.1"));
    assert!(!all_interfaces("[::1]"));
    assert!(!all_interfaces("example.com"));
}
