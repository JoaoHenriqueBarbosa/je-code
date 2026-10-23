//! Porte de `src/vs/platform/url/test/common/urlPathNormalization.test.ts` (suíte `URL effective path normalization`), caso a caso e na mesma ordem.
//!
//! Os dois laços `for` do original que geram um `test(...)` por caso (as URLs de autoridade e os casos de
//! travessia) viram um teste cada, que percorre todos os casos.

mod common;

use common::*;
use je_base::JsString;
use je_base::errors;
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_platform::url::trusted_domains::{is_url_domain_trusted_str as trusted, normalize_url};
use je_platform::url::url_glob::{normalize_url_path_separators, test_url_matches_glob as glob};
use je_platform::url::whatwg::{host, hostname, href, new_url, pathname};

const WIKI: &str = "https://github.com/microsoft/vscode/wiki";

fn uri(text: &str) -> URI {
    URI::parse(text, false)
}

/// `new URL(text)`, as propriedades que os testes leem.
fn url_href(text: &JsString) -> JsString {
    href(&new_url(text))
}

fn set_path(url: &URI, path: &str) -> URI {
    url.with(&UriChange { path: Field::from(path), ..UriChange::default() })
}

#[test]
fn review_authority_controls_preserve_the_original_destination_before_path_resolution() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for control in ["%09", "%0D", "%0A", "\t", "\r", "\n"] {
        let value = uri(&format!("https://{control}\\trusted.example/../evil.example/private?token=fixture"));
        let normalized = normalize_url_path_separators(&value);
        let expected = "https://trusted.example/evil.example/private?token=fixture";
        assert_eq!(url_href(&value.to_string(true)), expected, "original {control:?}");
        assert_eq!(url_href(&normalized.to_string(true)), expected, "normalized {control:?}");
        assert_eq!(url_href(&normalize_url_path_separators(&normalized).to_string(true)), expected, "repeated {control:?}");
    }
}

#[test]
fn review_http_drive_like_paths_retain_case_through_normalization_and_serialization() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let value = uri("https://example.test/public/../C:/Secret");
    let normalized = normalize_url_path_separators(&value);
    assert_eq!(url_href(&value.to_string(true)), "https://example.test/C:/Secret");
    assert_eq!(url_href(&normalized.to_string(true)), "https://example.test/C:/Secret");
    assert_eq!(url_href(&normalize_url_path_separators(&normalized).to_string(true)), "https://example.test/C:/Secret");
    assert!(trusted(&value, &["https://example.test/C:/*"]));
    assert!(!trusted(&value, &["https://example.test/c:/*"]));
}

#[test]
fn review_trusted_patterns_preserve_literal_escaped_data_across_normalization() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let cases: [(&str, &str, bool); 4] = [
        ("https://example.test/percent%2525value/page", "https://example.test/percent%2525value/page", true),
        ("https://example.test/allowed/private%252fsecret/file", "https://example.test/allowed/private%252fsecret/*", true),
        ("https://example.test/allowed/private/secret/file", "https://example.test/allowed/private%252fsecret/*", false),
        ("https://api.example.test:8443/allowed/private%252fsecret/file", "https://*.example.test:*/allowed/private%252fsecret/*", true),
    ];
    for (url, pattern, expected) in cases {
        assert_eq!(trusted(&uri(url), &[pattern]), expected, "{url} contra {pattern}");
    }
}

#[test]
fn review_github_unicode_trust_folds_characters_without_decoding_escaped_separators() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        [
            trusted(&uri("https://github.com/owner/repo/wiki/%C3%84"), &["https://github.com/owner/repo/wiki/%C3%A4"]),
            trusted(&uri("https://example.test/wiki/%C3%84"), &["https://example.test/wiki/%C3%A4"]),
            trusted(&uri("https://github.com/owner/repo/wiki/private%252fsecret"), &["https://github.com/owner/repo/wiki/private/secret"]),
            trusted(&uri("https://github.com/owner/repo/wiki/private%252fsecret"), &["https://github.com/owner/repo/wiki/private%252fsecret"]),
            trusted(&uri("https://github.com/owner/repo/wiki/%25C3%2584"), &["https://github.com/owner/repo/wiki/%C3%A4"]),
            trusted(&uri("https://github.com/owner/repo/wiki/%2525C3%252584"), &["https://github.com/owner/repo/wiki/%C3%A4"]),
        ],
        [true, false, false, true, true, false]
    );
}

#[test]
fn review_effective_user_information_requires_explicit_trust_all() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for url in ["https://%5C/user@api.github.com/private", "https://%5C/user@sub.localhost/private", "https://user@api.github.com/private"] {
        let value = uri(url);
        assert_eq!(
            [trusted(&value, &[]), trusted(&value, &["https://*.github.com"]), trusted(&value, &["*"])],
            [false, false, true],
            "{url}"
        );
    }
}

#[test]
fn review_regression_decoded_authority_slashes_cannot_grant_localhost_or_domain_trust() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for separator in ["%2F", "%2f"] {
        assert!(!trusted(&uri(&format!("https://evil.example{separator}.localhost/collect")), &[]), "localhost {separator}");
        assert!(!trusted(&uri(&format!("https://evil.example{separator}.github.com/collect")), &["https://*.github.com"]), "domain {separator}");
    }
}

#[test]
fn review_regression_preserves_the_serialized_destination_of_each_authority_case() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let authority_cases = [
        r"https://\trusted.example/../evil.example/private",
        r"https://\\trusted.example/../evil.example/private",
        "https://%2Ftrusted.example/../evil.example/private",
        "https://%5C%5Ctrusted.example/../evil.example/private",
        r"https://EXAMPLE.test\CaseSensitive/Resource?query=\Value#\Fragment",
        r"https://example.test\path",
    ];
    for url in authority_cases {
        let value = uri(url);
        let normalized = normalize_url_path_separators(&value);
        let destination = new_url(&value.to_string(true));
        assert_eq!(*normalized.authority(), host(&destination), "authority de {url}");
        assert_eq!(url_href(&normalized.to_string(true)), href(&destination), "destination de {url}");
        assert_eq!(normalize_url_path_separators(&normalized).to_string(true), normalized.to_string(true), "repeated de {url}");
    }
}

#[test]
fn review_regression_preserves_case_insensitive_host_and_github_path_trust() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        [
            trusted(&uri("https://GitHub.com/Microsoft/vscode"), &["https://github.com/microsoft/vscode"]),
            trusted(&uri("https://WWW.Microsoft.COM/en-us"), &["https://*.microsoft.com"]),
            trusted(&uri("https://Docs.Example.com/a"), &["https://Docs.Example.com"]),
        ],
        [true, true, true]
    );
}

#[test]
fn review_regression_canonical_encoded_paths_match_equivalent_glob_paths() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for path in ["private%20docs", "%E6%97%A5%E6%9C%AC%E8%AA%9E", "report%7B2024%7D", "file%3Fpart%23part"] {
        let normalized = normalize_url_path_separators(&uri(&format!("https://example.test/{path}/secret")));
        assert!(glob(&normalized, &format!("https://example.test/{path}/*")), "{path}");
    }
}

#[test]
fn rejects_each_traversal_case_leaving_the_trusted_path() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let traversal_cases = [
        ("raw dots", format!("{WIKI}/../../../attacker/repo/wiki/Home")),
        ("encoded dots", format!("{WIKI}/%2e%2e/%2E%2E/.%2e/attacker/repo/wiki/Home")),
        ("encoded slashes", format!("{WIKI}/..%2f..%2F..%2fattacker/repo/wiki/Home")),
        ("encoded backslashes", format!("{WIKI}/..%5c..%5C..%5cattacker/repo/wiki/Home")),
        ("literal backslashes", format!(r"{WIKI}\..\..\..\attacker\repo\wiki\Home")),
        ("browser-encoded dots", format!("{WIKI}/%252e%252e/%252e%252e/%252e%252e/attacker/repo/wiki/Home")),
    ];
    for (name, url) in traversal_cases {
        let value = uri(&url);
        let destination = new_url(&value.to_string(true));
        let normalized = normalize_url_path_separators(&value);
        assert_eq!(hostname(&destination), "github.com", "{name}");
        assert_eq!(pathname(&destination), "/attacker/repo/wiki/Home", "{name}");
        assert!(!glob(normalized.to_string(true), &format!("{WIKI}/*")), "stringMatch {name}");
        assert!(!glob(&normalized, &format!("{WIKI}/*")), "uriMatch {name}");
        assert!(!trusted(&value, &[&format!("{WIKI}/*")]), "trusted {name}");
    }
}

#[test]
fn matches_a_normalized_allowed_path_reached_through_dot_segments() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let url = format!("{WIKI}/topics/../home");
    assert!(glob(normalize_url_path_separators(&uri(&url)), &format!("{WIKI}/home")));
    assert!(trusted(&uri(&url), &[&format!("{WIKI}/home")]));
}

#[test]
fn normalizes_trusted_domain_paths_without_changing_raw_glob_syntax_or_wildcard_ports() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let url = "http://api.example.test:8123/reports/latest";
    let pattern = "http://*.example.test:*/docs/../reports/*";
    assert!(!glob(url, pattern), "rawMatch");
    assert!(glob(url, &normalize_url(pattern).to_string_lossy()), "match");
    assert!(trusted(&uri(url), &[pattern]), "trusted");
}

#[test]
fn normalizes_a_terminal_dot_before_matching_a_required_path_suffix() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!glob(format!("{WIKI}/topics/../."), &format!("{WIKI}/*")));
}

#[test]
fn preserves_empty_segments_and_treats_drive_like_segments_as_url_paths() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(normalize_url("http://example.test/a//topic/../C:/../home"), "http://example.test/a//home");
}

#[test]
fn preserves_query_and_fragment_values_while_normalizing_the_path() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let url = URI::from(
        &UriComponents {
            scheme: Some("https".into()),
            authority: Some("example.test".into()),
            path: Some("/allowed/topic/../home".into()),
            query: Some(r"next=\folder\..\child&value=%2e%2e".into()),
            fragment: Some(r"/..\part%2f".into()),
        },
        false,
    );
    assert_eq!(normalize_url(&url), set_path(&url, "/allowed/home").to_string(true));
}

#[test]
fn preserves_escaped_path_data_when_normalizing_backslash_separators() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let urls = [
        uri(r"https://example.test/allowed/%252e%252e%252fprivate\file"),
        uri(r"https://example.test\allowed/%252e%252e%252fprivate\file"),
    ];
    for url in urls {
        assert_eq!(*normalize_url_path_separators(&url).path(), pathname(&new_url(&url.to_string(true))));
    }
}

#[test]
fn preserves_ordinary_in_scope_and_out_of_scope_wildcard_controls() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let cases: Vec<(String, String, bool)> = vec![
        (format!("{WIKI}/Home"), format!("{WIKI}/*"), true),
        ("https://github.com/attacker/repo/wiki/Home".into(), format!("{WIKI}/*"), false),
        (format!("{WIKI}Extra/Home"), format!("{WIKI}/*"), false),
        (format!("{WIKI}/Home?next=/../../../outside#../outside"), format!("{WIKI}/*"), true),
        ("https://api.example.test:8443/docs/v1/index.html".into(), "https://*.example.test:*/docs/*/index*".into(), true),
        ("http://example.test/docs/v1/index.html".into(), "*.example.test:*/docs/*/index*".into(), true),
        ("https://api.example.test/docs/v1/index.html".into(), "*.example.test:*/docs/*/index*".into(), true),
        ("https://example.test/outside".into(), "https://example.test:*/docs/*".into(), false),
    ];
    for (url, pattern, matches) in cases {
        assert_eq!(glob(url.as_str(), &pattern), matches, "{url} contra {pattern}");
    }
}

#[test]
fn preserves_rejection_of_wildcard_scheme_patterns() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let error = errors::catch(|| glob("https://example.test/docs/home", "*://example.test/docs/*")).unwrap_err();
    assert!(error.message.contains("Scheme contains illegal characters"), "{}", error.message);
}

#[test]
fn does_not_reinterpret_non_http_paths_or_escaped_separator_data() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let urls = [
        uri(r"file:///c:/allowed/../private\resource?query=\value#..\part"),
        uri(r"custom://example.test/allowed/../private\resource?query=\value#..\part"),
        uri("https://example.test/allowed/%252e%252e%252fprivate"),
    ];
    for url in urls {
        assert_eq!(normalize_url_path_separators(&normalize_url_path_separators(&url)).to_string(true), url.to_string(true));
    }
}

#[test]
fn preserves_userinfo_and_authorityless_trust_restrictions_and_explicit_trust_all() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let urls = [
        uri(&format!("{}/../private", WIKI.replace("github.com", "user@github.com"))),
        uri(r"https:\github.com\microsoft\vscode\wiki\..\private"),
    ];
    let pattern = format!("{WIKI}/*");
    for url in urls {
        assert!(!trusted(&url, &[&pattern]));
        assert!(trusted(&url, &["*"]));
    }
}
