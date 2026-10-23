//! Porte de `src/vs/platform/configuration/test/common/configurationVariables.test.ts` (suíte `hasConfigurationVariable`), caso a caso e na mesma ordem.
//!
//! Os dois últimos casos são gerados por um laço no original (`for (const suffix of ['', '${env:PATH}'])`) e
//! ficam na mesma ordem. O orçamento de tempo usa `Instant` no lugar de `performance.now()`.

mod common;

use std::time::Instant;

use je_base::JsString;
use je_platform::configuration::variables::{has_configuration_variable, parse_configuration_variable};

use common::*;

fn has(value: &str) -> bool {
    has_configuration_variable(&JsString::from(value))
}

#[test]
fn recognizes_complete_variables_including_empty_expressions() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let values = ["${name}", "${env:PATH}", "${}", "${:}", "${input:}", "${command:name:argument}", "before ${name} after", "${one}${two}", "$${name}", "\\${name}"];

    assert_eq!(values.iter().map(|v| has(v)).collect::<Vec<_>>(), values.iter().map(|_| true).collect::<Vec<_>>());
}

#[test]
fn recognizes_nested_ordinary_braces_and_variables() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let values = ["${{}}", "${{{name}}}", "${command:name{argument}tail}", "${outer:${inner}}", "${outer:{${inner:{argument}}}}"];

    assert_eq!(values.iter().map(|v| has(v)).collect::<Vec<_>>(), values.iter().map(|_| true).collect::<Vec<_>>());
}

#[test]
fn leaves_incomplete_markers_and_ordinary_braces_literal() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let values = ["", "literal", "$", "{name}", "$ {name}", "${", "${name", "${name{}", "${{name}", "${{{}}", "${outer:{inner}{tail}", "}${", "}${{name}"];

    assert_eq!(values.iter().map(|v| has(v)).collect::<Vec<_>>(), values.iter().map(|_| false).collect::<Vec<_>>());
}

#[test]
fn finds_complete_inner_variables_after_incomplete_outer_expressions() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let values = ["${outer ${inner}", "${outer:{${inner}", "${${}", "${${name{}}", "${outer ${incomplete ${env:PATH} trailing ${"];

    assert_eq!(values.iter().map(|v| has(v)).collect::<Vec<_>>(), values.iter().map(|_| true).collect::<Vec<_>>());
}

#[test]
fn ignores_unmatched_braces_outside_complete_variables() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let values = ["}${name}", "${name}{", "${name}}", "{${name}{", "${name} trailing ${"];

    assert_eq!(values.iter().map(|v| has(v)).collect::<Vec<_>>(), values.iter().map(|_| true).collect::<Vec<_>>());
}

#[test]
fn agrees_with_the_parser_on_all_short_brace_expressions() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    fn check(value: &str, remaining: usize, mismatches: &mut Vec<String>) {
        let js = JsString::from(value);
        let mut expected = false;
        for offset in 0..js.len() {
            if parse_configuration_variable(&js, offset).is_some() {
                expected = true;
                break;
            }
        }

        if has_configuration_variable(&js) != expected {
            mismatches.push(value.to_string());
        }

        if remaining > 0 {
            for character in "${}a:".chars() {
                check(&format!("{value}{character}"), remaining - 1, mismatches);
            }
        }
    }
    let mut mismatches: Vec<String> = Vec::new();
    check("", 6, &mut mismatches);
    assert_eq!(mismatches, Vec::<String>::new());
}

fn scans_64_kib_of_incomplete_markers(suffix: &str) {
    let value = JsString::from(format!("{}{suffix}", "${".repeat(32 * 1024)));
    let start = Instant::now();
    let actual = has_configuration_variable(&value);
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;

    assert_eq!(actual, !suffix.is_empty());
    assert!(elapsed < 1000.0, "Expected the scan to take less than 1000 ms, took {elapsed:.1} ms");
}

#[test]
fn scans_64_kib_of_incomplete_markers_within_the_performance_budget() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    scans_64_kib_of_incomplete_markers("");
}

#[test]
fn scans_64_kib_of_incomplete_markers_followed_by_a_complete_variable_within_the_performance_budget() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    scans_64_kib_of_incomplete_markers("${env:PATH}");
}
