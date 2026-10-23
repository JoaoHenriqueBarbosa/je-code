//! Porte de `src/vs/workbench/contrib/url/test/browser/trustedDomains.test.ts` (suíte
//! `Link protection domain matching`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `linkAllowedByRules` e `linkNotAllowedByRules` mantêm o `URI.parse(link)` (não estrito) e a mensagem
//!   de falha do original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::uri::URI;
use je_platform::url::trusted_domains::is_url_domain_trusted_str;

fn link_allowed_by_rules(link: &str, rules: &[&str]) {
    assert!(
        is_url_domain_trusted_str(&URI::parse(link, false), rules),
        "Link\n{link}\n should be allowed by rules\n{rules:?}"
    );
}

fn link_not_allowed_by_rules(link: &str, rules: &[&str]) {
    assert!(
        !is_url_domain_trusted_str(&URI::parse(link, false), rules),
        "Link\n{link}\n should NOT be allowed by rules\n{rules:?}"
    );
}

#[test]
fn simple() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    link_not_allowed_by_rules("https://x.org", &[]);

    link_allowed_by_rules("https://x.org", &["https://x.org"]);
    link_allowed_by_rules("https://x.org/foo", &["https://x.org"]);

    link_not_allowed_by_rules("https://x.org", &["http://x.org"]);
    link_not_allowed_by_rules("http://x.org", &["https://x.org"]);

    link_not_allowed_by_rules("https://www.x.org", &["https://x.org"]);

    link_allowed_by_rules("https://www.x.org", &["https://www.x.org", "https://y.org"]);
}

#[test]
fn localhost() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    link_allowed_by_rules("https://127.0.0.1", &[]);
    link_allowed_by_rules("https://127.0.0.1:3000", &[]);
    link_allowed_by_rules("https://localhost", &[]);
    link_allowed_by_rules("https://localhost:3000", &[]);
    link_allowed_by_rules("https://dev.localhost", &[]);
    link_allowed_by_rules("https://dev.localhost:3000", &[]);
    link_allowed_by_rules("https://app.localhost", &[]);
    link_allowed_by_rules("https://api.localhost:8080", &[]);
    link_allowed_by_rules("https://myapp.dev.localhost:8080", &[]);
}

#[test]
fn star() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    link_allowed_by_rules("https://a.x.org", &["https://*.x.org"]);
    link_allowed_by_rules("https://a.b.x.org", &["https://*.x.org"]);
}

#[test]
fn no_scheme() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    link_allowed_by_rules("https://a.x.org", &["a.x.org"]);
    link_allowed_by_rules("https://a.x.org", &["*.x.org"]);
    link_allowed_by_rules("https://a.b.x.org", &["*.x.org"]);
    link_allowed_by_rules("https://x.org", &["*.x.org"]);
    // https://github.com/microsoft/vscode/issues/249353
    link_allowed_by_rules("https://x.org:3000", &["*.x.org:3000"]);
}

#[test]
fn sub_paths() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    link_allowed_by_rules("https://x.org/foo", &["https://x.org/foo"]);
    link_allowed_by_rules("https://x.org/foo/bar", &["https://x.org/foo"]);

    link_allowed_by_rules("https://x.org/foo", &["https://x.org/foo/"]);
    link_allowed_by_rules("https://x.org/foo/bar", &["https://x.org/foo/"]);

    link_allowed_by_rules("https://x.org/foo", &["x.org/foo"]);
    link_allowed_by_rules("https://x.org/foo", &["*.org/foo"]);

    link_not_allowed_by_rules("https://x.org/bar", &["https://x.org/foo"]);
    link_not_allowed_by_rules("https://x.org/bar", &["x.org/foo"]);
    link_not_allowed_by_rules("https://x.org/bar", &["*.org/foo"]);

    link_allowed_by_rules("https://x.org/foo/bar", &["https://x.org/foo"]);
    link_not_allowed_by_rules("https://x.org/foo2", &["https://x.org/foo"]);

    link_not_allowed_by_rules("https://www.x.org/foo", &["https://x.org/foo"]);

    link_not_allowed_by_rules("https://a.x.org/bar", &["https://*.x.org/foo"]);
    link_not_allowed_by_rules("https://a.b.x.org/bar", &["https://*.x.org/foo"]);

    link_allowed_by_rules("https://github.com", &["https://github.com/foo/bar", "https://github.com"]);
}

#[test]
fn ports() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    link_not_allowed_by_rules("https://x.org:8080/foo/bar", &["https://x.org:8081/foo"]);
    link_allowed_by_rules("https://x.org:8080/foo/bar", &["https://x.org:*/foo"]);
    link_allowed_by_rules("https://x.org/foo/bar", &["https://x.org:*/foo"]);
    link_allowed_by_rules("https://x.org:8080/foo/bar", &["https://x.org:8080/foo"]);
}

#[test]
fn ip_addresses() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    link_allowed_by_rules("http://192.168.1.7/", &["http://192.168.1.7/"]);
    link_allowed_by_rules("http://192.168.1.7/", &["http://192.168.1.7"]);
    link_allowed_by_rules("http://192.168.1.7/", &["http://192.168.1.*"]);

    link_not_allowed_by_rules("http://192.168.1.7:3000/", &["http://192.168.*.6:*"]);
    link_allowed_by_rules("http://192.168.1.7:3000/", &["http://192.168.1.7:3000/"]);
    link_allowed_by_rules("http://192.168.1.7:3000/", &["http://192.168.1.7:*"]);
    link_allowed_by_rules("http://192.168.1.7:3000/", &["http://192.168.1.*:*"]);
    link_not_allowed_by_rules("http://192.168.1.7:3000/", &["http://192.168.*.6:*"]);
}

#[test]
fn scheme_match() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    link_allowed_by_rules("http://192.168.1.7/", &["http://*"]);
    link_allowed_by_rules("http://twitter.com", &["http://*"]);
    link_allowed_by_rules("http://twitter.com/hello", &["http://*"]);
    link_not_allowed_by_rules("https://192.168.1.7/", &["http://*"]);
    link_not_allowed_by_rules("https://twitter.com/", &["http://*"]);
}

#[test]
fn case_normalization() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // https://github.com/microsoft/vscode/issues/99294
    link_allowed_by_rules("https://github.com/microsoft/vscode/issues/new", &["https://github.com/microsoft"]);
    link_allowed_by_rules("https://github.com/microsoft/vscode/issues/new", &["https://github.com/microsoft"]);
}

#[test]
fn ignore_query_and_fragment() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // https://github.com/microsoft/vscode/issues/156839
    link_allowed_by_rules("https://github.com/login/oauth/authorize?foo=4", &["https://github.com/login/oauth/authorize"]);
    link_allowed_by_rules("https://github.com/login/oauth/authorize#foo", &["https://github.com/login/oauth/authorize"]);
}

#[test]
fn ensure_individual_parts_of_url_are_compared_and_wildcard_does_not_leak_out() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    link_not_allowed_by_rules("https://x.org/github.com", &["https://*.github.com"]);
    link_not_allowed_by_rules("https://x.org/y.github.com", &["https://*.github.com"]);
}
