//! Porte de `src/vs/workbench/services/accounts/test/common/githubEnterpriseInput.test.ts` (suíte
//! `parseGheInstanceInput`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `{ kind: GheParseResultKind.X, resolvedUri }` é a variante de [`GheParseResult`] com o mesmo campo.
//! - O `for` que gera um `test(...)` por endereço é um único caso que percorre os cinco, com o endereço na
//!   mensagem de falha.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::services::accounts::github_enterprise::{GheParseResult, GheParseResultKind, parse_ghe_instance_input};

fn single_word(resolved_uri: &str) -> GheParseResult {
    GheParseResult::SingleWord { resolved_uri: resolved_uri.to_owned() }
}

fn full_uri(resolved_uri: &str) -> GheParseResult {
    GheParseResult::FullUri { resolved_uri: resolved_uri.to_owned() }
}

#[test]
fn empty_input_returns_empty() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(parse_ghe_instance_input(""), GheParseResult::Empty);
    assert_eq!(parse_ghe_instance_input("   "), GheParseResult::Empty);
    assert_eq!(parse_ghe_instance_input("").kind(), GheParseResultKind::Empty);
}

#[test]
fn single_word_returns_single_word_with_resolved_uri() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(parse_ghe_instance_input("octocat"), single_word("https://octocat.ghe.com"));
    assert_eq!(parse_ghe_instance_input("my-org"), single_word("https://my-org.ghe.com"));
}

#[test]
fn single_word_with_surrounding_whitespace_is_trimmed() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(parse_ghe_instance_input("  octocat  "), single_word("https://octocat.ghe.com"));
}

#[test]
fn single_word_with_numbers_returns_single_word() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(parse_ghe_instance_input("org123"), single_word("https://org123.ghe.com"));
}

#[test]
fn single_word_with_underscores_is_invalid_not_valid_dns() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(parse_ghe_instance_input("my_org"), GheParseResult::Invalid);
}

#[test]
fn full_uri_with_https_prefix_returns_full_uri() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(parse_ghe_instance_input("https://octocat.ghe.com"), full_uri("https://octocat.ghe.com"));
    assert_eq!(parse_ghe_instance_input("https://octocat.ghe.com/"), full_uri("https://octocat.ghe.com/"));
}

#[test]
fn full_uri_without_https_prefix_gets_it_prepended() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(parse_ghe_instance_input("octocat.ghe.com"), full_uri("https://octocat.ghe.com"));
    assert_eq!(parse_ghe_instance_input("sub.octocat.ghe.com"), full_uri("https://sub.octocat.ghe.com"));
}

#[test]
fn full_uri_with_subdomain_returns_full_uri() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(parse_ghe_instance_input("https://sub.domain.ghe.com"), full_uri("https://sub.domain.ghe.com"));
}

#[test]
fn invalid_input_returns_invalid() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(parse_ghe_instance_input("not a valid url"), GheParseResult::Invalid);
    assert_eq!(parse_ghe_instance_input("https://github.com"), GheParseResult::Invalid);
    assert_eq!(parse_ghe_instance_input("ftp://octocat.ghe.com"), GheParseResult::Invalid);
}

#[test]
fn input_with_numbers_in_domain_is_valid() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(parse_ghe_instance_input("https://org123.ghe.com"), full_uri("https://org123.ghe.com"));
}

#[test]
fn single_word_with_only_hyphens_returns_single_word() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(parse_ghe_instance_input("my-long-org-name"), single_word("https://my-long-org-name.ghe.com"));
}

#[test]
fn uri_with_trailing_slash_is_valid() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(parse_ghe_instance_input("https://octocat.ghe.com/"), full_uri("https://octocat.ghe.com/"));
}

#[test]
fn copilot_enrollment_rejects_a_non_ghe_com_https_instance() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for uri in ["https://github.example.com", "http://ghe.local:8080/Team", "https://ghe.local/Team%20One/", "HTTPS://GHE.LOCAL:443/Team", "http://octocat.ghe.com"] {
        assert_eq!(parse_ghe_instance_input(uri), GheParseResult::Invalid, "Copilot enrollment rejects a non-GHE.com HTTPS instance ({uri})");
    }
}

#[test]
fn copilot_enrollment_rejects_paths_and_query_strings_on_ghe_com() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let results: Vec<GheParseResult> = ["https://tenant.ghe.com/other", "https://tenant.ghe.com?query", "https://tenant.ghe.com#fragment"].into_iter().map(parse_ghe_instance_input).collect();
    assert_eq!(results, vec![GheParseResult::Invalid, GheParseResult::Invalid, GheParseResult::Invalid]);
}
