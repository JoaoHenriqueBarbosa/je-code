//! Porte de `src/vs/platform/url/test/common/urlGlob.test.ts` (suíte `urlGlob`), caso a caso e na mesma ordem.

mod common;

use common::*;
use je_base::JsString;
use je_base::uri::URI;
use je_platform::url::test_url_matches_glob as glob;
use je_platform::url::whatwg::{new_url, pathname};

fn uri(text: &str) -> URI {
    URI::parse(text, false)
}

/// Uma URL com uma unidade UTF-16 (um surrogate isolado) entre `prefix` e `suffix`.
fn with_unit(prefix: &str, unit: u16, suffix: &str) -> JsString {
    let mut text = JsString::from(prefix);
    text.push_unit(unit);
    text.push_units(&JsString::from(suffix).units().to_vec());
    text
}

#[test]
fn exact_match() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(glob("https://example.com", "https://example.com"));
    assert!(glob("http://example.com", "http://example.com"));
    assert!(glob("https://example.com/path", "https://example.com/path"));
}

#[test]
fn trailing_slashes_are_ignored() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(glob("https://example.com/", "https://example.com"));
    assert!(glob("https://example.com", "https://example.com/"));
    assert!(glob("https://example.com//", "https://example.com"));
    assert!(glob("https://example.com/path/", "https://example.com/path"));
}

#[test]
fn query_and_fragment_are_ignored() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(glob("https://example.com?query=value", "https://example.com"));
    assert!(glob("https://example.com#fragment", "https://example.com"));
    assert!(glob("https://example.com?query=value#fragment", "https://example.com"));
}

#[test]
fn scheme_matching() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(glob("https://example.com", "https://example.com"));
    assert!(!glob("http://example.com", "https://example.com"));
    assert!(!glob("ftp://example.com", "https://example.com"));
}

#[test]
fn glob_without_scheme_assumes_http_https() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(glob("https://example.com", "example.com"));
    assert!(glob("http://example.com", "example.com"));
    assert!(!glob("ftp://example.com", "example.com"));
}

#[test]
fn wildcard_matching_in_path() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(glob("https://example.com/anything", "https://example.com/*"));
    assert!(glob("https://example.com/path/to/resource", "https://example.com/*"));
    assert!(glob("https://example.com/path/to/resource", "https://example.com/path/*"));
    assert!(glob("https://example.com/path/to/resource", "https://example.com/path/*/resource"));
}

#[test]
fn preserves_nonempty_trailing_wildcard_and_path_prefix_semantics() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        [
            glob("https://example.test/a", "https://example.test/a*"),
            glob("https://example.test/ab", "https://example.test/a*"),
            glob("https://example.test/ab", "https://example.test/*b"),
            glob("https://example.test/ab", "https://example.test/*b*"),
            glob("https://example.test/a/b", "https://example.test/a"),
            glob("https://example.test/", "https://example.test:*"),
        ],
        [false, true, true, false, true, true]
    );
}

#[test]
fn matches_long_paths_without_recursive_stack_growth() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let segment = "x".repeat(8192);
    assert_eq!(
        [
            glob(format!("https://example.test/docs/{segment}/match"), "https://example.test/docs/*/match"),
            glob(format!("https://example.test/docs/{segment}/miss"), "https://example.test/docs/*/match"),
        ],
        [true, false]
    );
}

#[test]
fn dot_segments_cannot_escape_a_path_scoped_glob() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let paths = [
        "/allowed/../outside",
        "/allowed/child/../../outside",
        "/allowed/./../outside",
        "/allowed/%2e%2e/outside",
        "/allowed/.%2E/outside",
        "/allowed/%2E./outside",
        "/allowed/..",
    ];
    let globs = ["https://example.com/allowed", "example.com/allowed", "https://*.example.com:*/allowed/*"];
    for path in paths {
        let url = format!("https://example.com{path}");
        for g in globs {
            assert!(!glob(url.as_str(), g), "string: {url} contra {g}");
            assert!(!glob(uri(&url), g), "uri: {url} contra {g}");
        }
    }
}

#[test]
fn http_paths_remove_ascii_tabs_and_newlines_before_resolving_dot_segments() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for character in ["\t", "\n", "\r"] {
        for scheme in ["http", "https"] {
            let url = format!("{scheme}://example.com/allowed/.{character}./outside");
            let g = format!("{scheme}://example.com/allowed");
            assert_eq!(pathname(&new_url(&JsString::from(url.as_str()))), "/outside");
            assert!(!glob(url.as_str(), &g));
            assert!(!glob(uri(&url), &g));
            assert!(!glob(url.as_str(), &format!("{g}/*")));
            assert!(glob(format!("{scheme}://example.com/allowed/in{character}side"), &g));
        }
    }
}

#[test]
fn dot_segments_that_stay_within_allowed_paths_still_match() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for path in ["/allowed/./page", "/allowed/child/../page", "/allowed/child/.."] {
        assert!(glob(format!("https://example.com{path}"), "https://example.com/allowed"), "{path}");
    }
}

#[test]
fn http_paths_resolve_backslash_separators() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        [
            glob("http://example.com/allowed/../outside", "http://example.com/allowed"),
            glob(r"https://example.com/allowed/..\outside", "https://example.com/allowed"),
            glob(r"http://example.com/allowed/..\outside", "http://example.com/allowed"),
            glob(r"https://example.com/allowed/child\..\page", "https://example.com/allowed"),
        ],
        [false, false, false, true]
    );
}

#[test]
fn resolving_dot_segments_preserves_empty_path_segments() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        [
            glob("https://example.com/allowed//../page", "https://example.com/allowed"),
            glob("https://example.com/other//../allowed/page", "https://example.com/allowed"),
            glob("https://example.com/allowed//../../outside", "https://example.com/allowed"),
            glob("https://example.com/allowed//page", "https://example.com/allowed/page"),
        ],
        [true, false, false, false]
    );
}

#[test]
fn resolving_dot_segments_preserves_encoded_path_characters() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        [
            glob("https://example.com/allowed/child/../file%3Fpart%23part", "https://example.com/allowed/*file%3Fpart%23part"),
            glob("https://example.com/allowed/child/../space%20name", "https://example.com/allowed/*space%20name"),
            glob("https://example.com/allowed/%252e%252e/page", "https://example.com/allowed"),
            glob("https://example.com/allowed/child/../%F0%9F%9A%80", "https://example.com/allowed/*%F0%9F%9A%80"),
        ],
        [true, true, true, true]
    );
}

#[test]
fn unpaired_surrogates_do_not_throw_or_bypass_path_scoped_globs() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let g = "https://example.com/allowed";
    for surrogate in [0xD800u16, 0xDC00] {
        let url = with_unit("https://example.com/allowed/", surrogate, "/page");
        assert!(glob(&url, g));
        assert!(glob(URI::parse(url.clone(), false), g));
        assert!(!glob(with_unit("https://example.com/allowed/", surrogate, "/../../outside"), g));
    }
}

#[test]
fn resolving_dot_segments_does_not_broaden_the_supplied_path_match() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        [
            glob("https://example.com/other/../allowed/page", "https://example.com/allowed"),
            glob("https://example.com/other%2F..%2Fallowed/page", "https://example.com/allowed"),
            glob("https://example.com/allowed/page/../outside", "https://example.com/allowed/page"),
        ],
        [false, false, false]
    );
}

#[test]
fn glob_paths_and_non_http_paths_are_not_resolved() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        [
            glob("https://example.com/outside", "https://example.com/allowed/../outside"),
            glob("https://example.com/outside", "https://example.com/allowed/*/../../outside"),
            glob("custom://example.com/allowed/../outside", "custom://example.com/allowed"),
        ],
        [false, false, true]
    );
}

#[test]
fn subdomain_wildcard_matching() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(glob("https://sub.example.com", "https://*.example.com"));
    assert!(glob("https://sub.domain.example.com", "https://*.example.com"));
    assert!(glob("https://example.com", "https://*.example.com"));
}

#[test]
fn backslash_url_authorities_match_only_the_browser_destination() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let cases: [(&str, &str, bool); 10] = [
        (r"https://evil.example\.github.com/collect?leak=<data>", "https://*.github.com", false),
        (r"https://evil.example\\.github.com/collect?leak=<data>", "https://*.github.com", false),
        (r"https://169.254.169.254\.github.com/latest/meta-data/", "*.github.com", false),
        (r"https://169.254.169.254\\.github.com/latest/meta-data/", "*.github.com", false),
        (r"http://127.0.0.2:38651\.github.com/exfil?data=fixture", "http://*.github.com:*", false),
        (r"https://github.com\.evil.example/resource", "https://*.github.com", true),
        ("https://api.github.com/resource", "https://*.github.com", true),
        ("https://evil.example/resource", "https://*.github.com", false),
        (r"custom://example.com\segment/resource", r"custom://example.com\segment", true),
        (r"custom://example.com\segment/resource", "custom://example.com/segment", false),
    ];
    for (url, pattern, matches) in cases {
        assert_eq!(glob(url, pattern), matches, "string: {url} contra {pattern}");
        assert_eq!(glob(uri(url), pattern), matches, "uri: {url} contra {pattern}");
    }
}

#[test]
fn backslash_url_patterns_use_browser_path_separators() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let cases: [(&str, &str, bool); 4] = [
        ("https://example.com/.github.com/resource", r"https://example.com\.github.com", true),
        ("https://example.com/.github.com/resource", r"example.com\.github.com", true),
        ("https://example.com/path/resource", r"https://example.com\path\*", true),
        ("https://api.github.com/resource", r"https://example.com\.github.com", false),
    ];
    for (url, pattern, matches) in cases {
        assert_eq!(glob(url, pattern), matches, "{url} contra {pattern}");
    }
}

#[test]
fn idn_literal_hostname_components_match_unicode_and_punycode_spellings() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hosts = ["bücher.example.test", "xn--bcher-kva.example.test", "b%C3%BCcher.example.test", "BÜCHER.EXAMPLE.TEST", "XN--BCHER-KVA.EXAMPLE.TEST"];
    let patterns = ["bücher.example.test", "xn--bcher-kva.example.test"];
    for host in hosts {
        for pattern in patterns {
            assert!(glob(format!("https://{host}"), &format!("https://{pattern}")), "exact {host} {pattern}");
            assert!(glob(format!("https://x.{host}"), &format!("https://*.{pattern}")), "wildcard {host} {pattern}");
            assert!(glob(format!("https://{host}"), &format!("https://*.{pattern}")), "bare {host} {pattern}");
            assert!(glob(uri(&format!("https://x.{host}")), &format!("https://*.{pattern}")), "uri {host} {pattern}");
        }
    }
}

#[test]
fn idn_normalization_preserves_wildcard_scheme_port_and_path_semantics() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let cases: [(&str, &str, bool); 22] = [
        ("https://api1.xn--bcher-kva.example.test", "https://api*.bücher.example.test", true),
        ("https://xn--bcher-kva.a.example.test", "https://bücher.*.example.test", true),
        ("https://x.xn--bcher-kva.example.test", "*.bücher.example.test", true),
        ("http://x.xn--bcher-kva.example.test", "*.bücher.example.test", true),
        ("https://x.xn--bcher-kva.example.test:8443/allowed/page", "https://*.bücher.example.test:*/allowed/*", true),
        ("https://xn--bcher-kva.example.test/allowed/page", "https://bücher.example.test:*/allowed", true),
        ("https://xn--bcher-kva.example.test:443", "https://bücher.example.test:443", true),
        ("https://xn--bcher-kva.example.test:443", "https://bücher.example.test", false),
        ("https://xn--bcher-kva.example.test", "https://bücher.example.test:443", false),
        ("https://xn--bcher-kva.example.test:8443", "https://bücher.example.test:8080", false),
        ("http://xn--bcher-kva.example.test", "https://bücher.example.test", false),
        ("custom://xn--bcher-kva.example.test", "*.bücher.example.test", false),
        ("https://xn--bcher-kva.example.test/Allowed/page", "https://bücher.example.test/allowed", false),
        ("https://xn--bcher-kva.example.test/allowed/../outside", "https://bücher.example.test/allowed", false),
        ("https://xn--bcher-kva.example.test/allowed/child/../page", "https://bücher.example.test/allowed", true),
        ("https://evilxn--bcher-kva.example.test", "https://*.bücher.example.test", false),
        ("https://x.xn--bcher-kva.example.test.evil.test", "https://*.bücher.example.test", false),
        ("https://x.other.example.test", "https://*.bücher.example.test", false),
        ("https://bücher.example.test", "https://bü*.example.test", true),
        ("https://x.bücher.example.test", "https://＊.bücher.example.test", false),
        ("https://127.0.0.1", "https://127.1", false),
        ("https://[::1]:8443", "https://[::1]:*", true),
    ];
    for (url, pattern, expected) in cases {
        assert_eq!(glob(url, pattern), expected, "string: {url} contra {pattern}");
        assert_eq!(glob(uri(url), pattern), expected, "uri: {url} contra {pattern}");
    }
}

#[test]
fn subdomain_wildcard_must_match_on_dot_boundary() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Não deve casar: não há ponto antes do domínio
    assert!(!glob("https://notexample.com", "https://*.example.com"));
    assert!(!glob("https://evil-microsoft.com", "https://*.microsoft.com"));
    assert!(!glob("https://evilmicrosoft.com", "https://*.microsoft.com"));
    assert!(!glob("https://evil-example.com", "https://*.example.com"));
    assert!(!glob("https://myexample.com", "https://*.example.com"));
    assert!(!glob("https://notexample.com/path", "https://*.example.com/path"));

    // Deve casar: subdomínio de verdade, com ponto
    assert!(glob("https://sub.microsoft.com", "https://*.microsoft.com"));
    assert!(glob("https://a.b.c.microsoft.com", "https://*.microsoft.com"));
    assert!(glob("https://microsoft.com", "https://*.microsoft.com"));
    assert!(glob("https://sub.example.com/path", "https://*.example.com/path"));
}

#[test]
fn subdomain_wildcard_without_scheme_must_match_on_dot_boundary() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!glob("https://evil-microsoft.com", "*.microsoft.com"));
    assert!(!glob("http://evil-microsoft.com", "*.microsoft.com"));
    assert!(glob("https://sub.microsoft.com", "*.microsoft.com"));
    assert!(glob("http://sub.microsoft.com", "*.microsoft.com"));
}

#[test]
fn port_matching() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(glob("https://example.com:8080", "https://example.com:8080"));
    assert!(!glob("https://example.com:8080", "https://example.com:9090"));
    assert!(!glob("https://example.com", "https://example.com:8080"));
}

#[test]
fn wildcard_port_matching() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(glob("https://example.com:8080", "https://example.com:*"));
    assert!(glob("https://example.com:9090", "https://example.com:*"));
    assert!(glob("https://example.com", "https://example.com:*"));
    assert!(glob("https://example.com:8080/path", "https://example.com:*/path"));
}

#[test]
fn root_path_glob() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(glob("https://example.com", "https://example.com/"));
    assert!(glob("https://example.com/", "https://example.com/"));
    assert!(glob("https://example.com/path", "https://example.com/"));
}

#[test]
fn mismatch_cases() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!glob("https://example.com/path", "https://example.com/other"));
    assert!(!glob("https://example.com", "https://other.com"));
    assert!(!glob("https://sub.example.com", "https://example.com"));
}

#[test]
fn uri_object_input() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let value = URI::parse("https://example.com/path", false);
    assert!(glob(&value, "https://example.com/path"));
    assert!(glob(&value, "https://example.com/*"));
}

#[test]
fn complex_patterns() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(glob("https://api.github.com/repos/microsoft/vscode", "https://*.github.com/repos/*/*"));
    assert!(!glob("https://github.com/microsoft/vscode", "https://*.github.com/repos/*/*"));
    assert!(glob("https://api.github.com:443/repos/microsoft/vscode", "https://*.github.com:*/repos/*/*"));
}

#[test]
fn edge_cases() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // O curinga depois da autoridade não casa sem caminho adicional
    assert!(!glob("https://example.com", "https://example.com*"));
    assert!(glob("https://example.com.extra", "https://example.com*"));
    assert!(glob("https://example.com", "*"));
}
