//! Porte de `src/vs/platform/networkFilter/test/common/domainMatcher.test.ts` (suíte `domainMatcher`), caso a caso e na mesma ordem.
//!
//! Divergências: `normalizeDomain(undefined)` é `normalize_domain(None, false)`, e o `undefined` do resultado
//! é `None`. Os `assert.deepStrictEqual` sobre arrays viram `assert_eq!` sobre `Vec`. As listas vazias de
//! `isDomainAllowed` usam `NONE` (`[&str; 0]`) porque a função é genérica sobre `AsRef<str>`.
//! `ensureNoDisposablesAreLeakedInTestSuite` não tem o que vigiar (nada aqui é descartável).

use je_base::JsString;
use je_base::uri::{URI, UriComponents};
use je_platform::network_filter::domain_matcher::{
    extract_domain_from_uri, extract_domain_pattern, is_domain_allowed, matches_domain_pattern, normalize_domain, normalize_domain_pattern,
};

const NONE: [&str; 0] = [];

fn s(text: &str) -> Option<String> {
    Some(text.to_owned())
}

fn nd(value: &str) -> Option<String> {
    normalize_domain(Some(value), false)
}

fn parse(text: &str) -> URI {
    URI::parse(text, false)
}

fn from_components(scheme: &str, authority: Option<&str>, path: &str) -> URI {
    URI::from(
        &UriComponents {
            scheme: Some(JsString::from(scheme)),
            authority: authority.map(JsString::from),
            path: Some(JsString::from(path)),
            ..UriComponents::default()
        },
        false,
    )
}

fn matches(domain: &str, pattern: &str) -> bool {
    matches_domain_pattern(domain, pattern)
}

// ---- normalizeDomain -------------------------------------------------------------------------------

#[test]
fn normalize_domain_returns_undefined_for_empty_falsy_input() {
    assert_eq!(vec![normalize_domain(None, false), nd(""), nd("  ")], vec![None, None, None]);
}

#[test]
fn normalize_domain_lowercases_and_trims() {
    assert_eq!(nd("  Example.COM  "), s("example.com"));
}

#[test]
fn normalize_domain_strips_user_info() {
    assert_eq!(nd("user@example.com"), s("example.com"));
}

#[test]
fn normalize_domain_strips_port() {
    assert_eq!(nd("example.com:8080"), s("example.com"));
}

#[test]
fn normalize_domain_strips_trailing_dots() {
    assert_eq!(nd("example.com.."), s("example.com"));
}

#[test]
fn normalize_domain_rejects_paths() {
    assert_eq!(nd("example.com/path"), None);
}

#[test]
fn normalize_domain_rejects_dot_and_double_dot() {
    assert_eq!(vec![nd("."), nd("..")], vec![None, None]);
}

#[test]
fn normalize_domain_accepts_bare_wildcard() {
    assert_eq!(nd("*"), s("*"));
}

#[test]
fn normalize_domain_accepts_wildcard_prefix() {
    assert_eq!(nd("*.example.com"), s("*.example.com"));
}

#[test]
fn normalize_domain_strips_trailing_punctuation() {
    assert_eq!(nd("example.com,"), s("example.com"));
    assert_eq!(nd("example.com;"), s("example.com"));
    assert_eq!(nd("example.com)"), s("example.com"));
}

#[test]
fn normalize_domain_rejects_file_extension_like_tlds_when_not_from_url() {
    assert_eq!(nd("foo.js"), None);
    assert_eq!(nd("foo.json"), None);
    assert_eq!(nd("foo.ts"), None);
}

#[test]
fn normalize_domain_allows_file_extension_like_tlds_when_from_url_is_true() {
    assert_eq!(normalize_domain(Some("foo.js"), true), s("foo.js"));
}

#[test]
fn normalize_domain_rejects_invalid_characters() {
    assert_eq!(nd("exam ple.com"), None);
    assert_eq!(nd("example!.com"), None);
}

#[test]
fn normalize_domain_handles_complex_valid_domains() {
    assert_eq!(nd("sub.domain.example.com"), s("sub.domain.example.com"));
}

// ---- extractDomainPattern --------------------------------------------------------------------------

#[test]
fn extract_domain_pattern_returns_trimmed_input_when_no_scheme() {
    assert_eq!(extract_domain_pattern("  example.com  "), "example.com");
}

#[test]
fn extract_domain_pattern_returns_bare_wildcard_as_is() {
    assert_eq!(extract_domain_pattern("*"), "*");
}

#[test]
fn extract_domain_pattern_extracts_authority_from_url() {
    assert_eq!(extract_domain_pattern("https://example.com/path"), "example.com");
}

#[test]
fn extract_domain_pattern_extracts_authority_with_port_from_url() {
    assert_eq!(extract_domain_pattern("http://example.com:8080/path"), "example.com:8080");
}

// ---- normalizeDomainPattern ------------------------------------------------------------------------

#[test]
fn normalize_domain_pattern_canonicalizes_unicode_wildcard_suffixes() {
    let patterns = [
        "*.b\u{fc}cher.de",
        "*.B\u{dc}CHER.DE",
        "  *.b\u{fc}cher.de.  ",
        "*.b\u{fc}cher.de:443",
        "https://*.b\u{fc}cher.de:443/private",
        "*.xn--bcher-kva.de",
    ];

    assert_eq!(
        patterns.iter().map(|pattern| normalize_domain_pattern(pattern)).collect::<Vec<_>>(),
        patterns.iter().map(|_| s("*.xn--bcher-kva.de")).collect::<Vec<_>>(),
    );
}

// ---- matchesDomainPattern --------------------------------------------------------------------------

#[test]
fn matches_domain_pattern_exact_match() {
    assert!(matches("example.com", "example.com"));
    assert!(!matches("example.com", "other.com"));
}

#[test]
fn matches_domain_pattern_case_insensitive() {
    assert!(matches("example.com", "Example.COM"));
}

#[test]
fn matches_domain_pattern_bare_wildcard_matches_anything() {
    assert!(matches("example.com", "*"));
    assert!(matches("anything.test", "*"));
}

#[test]
fn matches_domain_pattern_wildcard_prefix_matches_subdomains() {
    assert!(matches("sub.example.com", "*.example.com"));
    assert!(matches("deep.sub.example.com", "*.example.com"));
    assert!(matches("example.com", "*.example.com"));
}

#[test]
fn matches_domain_pattern_wildcard_prefix_does_not_match_unrelated_domains() {
    assert!(!matches("notexample.com", "*.example.com"));
}

#[test]
fn matches_domain_pattern_matches_unicode_wildcard_patterns_against_canonical_idn_hosts() {
    assert_eq!(
        vec![
            matches("xn--bcher-kva.de", "*.b\u{fc}cher.de"),
            matches("sub.xn--bcher-kva.de", "*.b\u{fc}cher.de"),
            matches("deep.sub.xn--bcher-kva.de", "*.b\u{fc}cher.de"),
            matches("notxn--bcher-kva.de", "*.b\u{fc}cher.de"),
            matches("xn--bcher-kva.de.example.com", "*.b\u{fc}cher.de"),
            matches("example.com", "*.b\u{fc}cher.de"),
            matches("xn--bcher-kva.de", "b\u{fc}cher.de"),
            matches("sub.xn--bcher-kva.de", "b\u{fc}cher.de"),
            matches("xn--bcher-kva.de", "*.xn--bcher-kva.de"),
            matches("sub.xn--bcher-kva.de", "*.xn--bcher-kva.de"),
        ],
        vec![true, true, true, false, false, false, true, false, true, true]
    );
}

#[test]
fn matches_domain_pattern_matches_domain_from_url_pattern() {
    assert!(matches("example.com", "https://example.com/page"));
}

#[test]
fn matches_domain_pattern_matches_explicitly_configured_local_and_private_host_patterns() {
    assert_eq!(
        vec![
            matches("localhost", "localhost"),
            matches("sub.localhost", "*.localhost"),
            matches("127.0.0.1", "127.0.0.1"),
            matches("0.0.0.0", "0.0.0.0"),
            matches("service.internal", "service.internal"),
            matches("localhost", "other.localhost"),
        ],
        vec![true, true, true, true, true, false]
    );
}

#[test]
fn matches_domain_pattern_canonicalizes_alternative_ipv4_administrator_patterns() {
    assert_eq!(
        vec![
            matches("127.0.0.1", "127.1"),
            matches("127.0.0.1", "0177.0.0.1"),
            matches("127.0.0.1", "0x7f000001"),
            matches("127.0.0.1", "2130706433"),
            matches("192.168.1.1", "0300.0250.01.01"),
            matches("169.254.169.254", "0xa9fea9fe"),
        ],
        vec![true, true, true, true, true, true]
    );
}

#[test]
fn matches_domain_pattern_matches_bracketed_and_bare_ipv6_patterns() {
    assert_eq!(
        vec![
            matches("[::1]", "[::1]"),
            matches("[::1]", "::1"),
            matches("[fd00::1]", "fd00::1"),
            matches("fd00", "fd00::1"),
            matches("[2001:db8::1]", "2001:0db8:0:0:0:0:0:1"),
            matches("[2001:db8::1]", "2001:db8::2"),
        ],
        vec![true, true, true, false, true, false]
    );
}

#[test]
fn matches_domain_pattern_returns_false_for_invalid_pattern() {
    assert!(!matches("example.com", ""));
}

#[test]
fn matches_domain_pattern_matches_bare_administrator_patterns_outside_well_known_public_suffixes() {
    assert_eq!(
        vec![
            matches("evil.xyz", "evil.xyz"),
            matches("evil.co.uk", "evil.co.uk"),
            matches("evil.ru", "evil.ru"),
            matches("evil.app", "evil.app"),
            matches("evil.cn", "evil.cn"),
            matches("evil.info", "evil.info"),
            matches("evil.biz", "evil.biz"),
            matches("evil.de", "evil.de"),
            matches("evil.jp", "evil.jp"),
            matches("evil.site", "evil.site"),
            matches("evil.zip", "evil.zip"),
            matches("metadata.internal", "metadata.internal"),
            matches("host.local", "host.local"),
            matches("db.corp", "db.corp"),
            matches("srv.lan", "srv.lan"),
            matches("169.254.169.254", "169.254.169.254"),
            matches("10.0.0.5", "10.0.0.5"),
            matches("127.0.0.1", "127.0.0.1"),
            matches("192.168.1.1", "192.168.1.1"),
            matches("localhost", "localhost"),
            matches("metadata", "metadata"),
            matches("metadata.google.internal", "metadata.google.internal"),
        ],
        vec![true; 22]
    );
}

#[test]
fn matches_domain_pattern_matches_wildcard_administrator_patterns_outside_well_known_public_suffixes() {
    assert_eq!(
        vec![
            matches("service.internal", "*.internal"),
            matches("service.corp", "*.corp"),
            matches("service.lan", "*.lan"),
            matches("service.corp.local", "*.corp.local"),
            matches("service.co.uk", "*.co.uk"),
            matches("service.example.co.uk", "*.example.co.uk"),
        ],
        vec![true; 6]
    );
}

#[test]
fn matches_domain_pattern_canonicalizes_wildcard_ipv4_administrator_patterns() {
    assert_eq!(
        vec![
            matches("[::ffff:7f00:1]", "*.127.0.0.1"),
            matches("[::7f00:1]", "*.127.0.0.1"),
            matches("127.0.0.1", "*.127.1"),
            matches("[::ffff:7f00:1]", "*.0x7f000001"),
            matches("[::ffff:7f00:1]", "*.0177.0.0.1"),
            matches("[::1]", "*.127.0.0.1"),
        ],
        vec![true, true, true, true, true, false]
    );
}

#[test]
fn matches_domain_pattern_matches_unbracketed_ipv6_administrator_patterns() {
    assert_eq!(
        vec![
            matches("[::1]", "::1"),
            matches("[::1]", "0:0:0:0:0:0:0:1"),
            matches("[::ffff:7f00:1]", "::ffff:127.0.0.1"),
            matches("[fe80::1]", "fe80::1"),
        ],
        vec![true; 4]
    );
}

#[test]
fn matches_domain_pattern_matches_ipv4_mapped_ipv6_literals_against_ipv4_patterns() {
    assert_eq!(
        vec![matches("[::ffff:7f00:1]", "127.0.0.1"), matches("[::ffff:a9fe:a9fe]", "169.254.169.254")],
        vec![true, true]
    );
}

#[test]
fn matches_domain_pattern_matches_ipv4_literals_and_embedded_ipv4_patterns_symmetrically() {
    assert_eq!(
        vec![
            matches("127.0.0.1", "[::ffff:127.0.0.1]"),
            matches("[::7f00:1]", "127.0.0.1"),
            matches("127.0.0.1", "[::127.0.0.1]"),
            matches("[::1]", "0.0.0.1"),
        ],
        vec![true, true, true, false]
    );
}

// ---- extractDomainFromUri --------------------------------------------------------------------------

#[test]
fn extract_domain_from_uri_extracts_domain_from_https_uri() {
    assert_eq!(extract_domain_from_uri(&parse("https://example.com/path")), s("example.com"));
}

#[test]
fn extract_domain_from_uri_strips_port() {
    assert_eq!(extract_domain_from_uri(&parse("https://example.com:443/path")), s("example.com"));
}

#[test]
fn extract_domain_from_uri_uses_the_last_user_information_separator() {
    assert_eq!(
        vec![
            extract_domain_from_uri(&parse("http://a@b@127.0.0.1:8931/private")),
            extract_domain_from_uri(&parse("http://a%40b@127.0.0.1:8931/private")),
        ],
        vec![s("127.0.0.1"), s("127.0.0.1")]
    );
}

#[test]
fn extract_domain_from_uri_returns_undefined_for_empty_authority() {
    assert_eq!(extract_domain_from_uri(&from_components("file", None, "/tmp/test")), None);
}

#[test]
fn extract_domain_from_uri_rejects_authorities_whose_percent_decoded_representation_changes_the_host() {
    assert_eq!(extract_domain_from_uri(&parse("http://169.254.169.254%40allowed.com/private")), None);
}

#[test]
fn extract_domain_from_uri_rejects_percent_encoded_path_separators_in_authorities() {
    assert_eq!(
        vec![extract_domain_from_uri(&parse("https://evil.com%2fx/")), extract_domain_from_uri(&parse("https://evil.com%5c/"))],
        vec![None, None]
    );
}

#[test]
fn extract_domain_from_uri_extracts_and_canonicalizes_ipv6_literals() {
    assert_eq!(
        vec![
            extract_domain_from_uri(&parse("http://[::1]:3000/path")),
            extract_domain_from_uri(&parse("http://[0:0:0:0:0:0:0:1]/path")),
            extract_domain_from_uri(&parse("http://[::ffff:127.0.0.1]/path")),
            extract_domain_from_uri(&parse("http://[::ffff:7f00:1]/path")),
            extract_domain_from_uri(&parse("https://[2001:0db8:0:0:0:0:0:1]/path")),
            extract_domain_from_uri(&parse("https://[fe80:0:0:0:0:0:0:1]/path")),
        ],
        vec![s("[::1]"), s("[::1]"), s("[::ffff:7f00:1]"), s("[::ffff:7f00:1]"), s("[2001:db8::1]"), s("[fe80::1]")]
    );
}

#[test]
fn extract_domain_from_uri_returns_undefined_for_malformed_ipv6_authorities() {
    assert_eq!(
        vec![
            extract_domain_from_uri(&from_components("http", Some("[::1"), "/")),
            extract_domain_from_uri(&from_components("http", Some("::1]"), "/")),
            extract_domain_from_uri(&from_components("http", Some("[::1]extra"), "/")),
            extract_domain_from_uri(&from_components("http", Some("[fe80::1%25eth0]"), "/")),
        ],
        vec![None, None, None, None]
    );
}

// ---- isDomainAllowed -------------------------------------------------------------------------------

#[test]
fn is_domain_allowed_denies_everything_when_both_lists_empty() {
    assert!(!is_domain_allowed("example.com", &NONE, &NONE));
}

#[test]
fn is_domain_allowed_denied_takes_precedence_over_allowed() {
    assert!(!is_domain_allowed("evil.com", &["*.com"], &["evil.com"]));
}

#[test]
fn is_domain_allowed_allowed_list_restricts_to_matching_domains() {
    assert!(is_domain_allowed("example.com", &["example.com"], &NONE));
    assert!(!is_domain_allowed("other.com", &["example.com"], &NONE));
}

#[test]
fn is_domain_allowed_deny_only_config_allows_non_denied_domains() {
    assert!(is_domain_allowed("good.com", &NONE, &["evil.com"]));
    assert!(!is_domain_allowed("evil.com", &NONE, &["evil.com"]));
}

#[test]
fn is_domain_allowed_wildcard_allowed_with_specific_deny() {
    assert!(is_domain_allowed("safe.com", &["*"], &["evil.com"]));
    assert!(!is_domain_allowed("evil.com", &["*"], &["evil.com"]));
}

#[test]
fn is_domain_allowed_wildcard_deny_blocks_everything() {
    assert!(!is_domain_allowed("example.com", &["example.com"], &["*"]));
}

#[test]
fn is_domain_allowed_subdomain_matching_in_allow_deny() {
    assert!(is_domain_allowed("api.example.com", &["*.example.com"], &NONE));
    assert!(!is_domain_allowed("api.example.com", &NONE, &["*.example.com"]));
}

#[test]
fn is_domain_allowed_matches_canonical_ipv6_literals_against_equivalent_patterns() {
    assert_eq!(
        vec![
            matches("[::1]", "[0:0:0:0:0:0:0:1]"),
            matches("[::ffff:7f00:1]", "http://[::ffff:127.0.0.1]:3000/path"),
            matches("[2001:db8::1]", "[2001:0db8:0:0:0:0:0:1]"),
            matches("[2001:db8::1]", "[2001:db8::2]"),
        ],
        vec![true, true, true, false]
    );
}
