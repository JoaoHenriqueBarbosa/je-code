//! Testes do `create_matchers` (`textMateScopeMatcher.ts`). O oráculo não tem teste dele:
//! estes casos cobrem prioridade `L:`/`R:`, negação, parênteses, `|` e `,`.

use std::rc::Rc;

use je_workbench::services::themes::text_mate_scope_matcher::{create_matchers, MatcherWithPriority};

/// A entrada é a lista de escopos do token; um nome casa (pontuação 1) se estiver na lista, senão -1.
fn matches_name(names: &[String], scopes: &Vec<&'static str>) -> i32 {
    if names.iter().all(|name| scopes.contains(&name.as_str())) {
        1
    } else {
        -1
    }
}

fn build(selector: &str) -> Vec<MatcherWithPriority<Vec<&'static str>>> {
    let mut results = Vec::new();
    create_matchers(selector, Rc::new(matches_name), &mut results);
    results
}

fn score(selector: &str, scopes: &[&'static str]) -> i32 {
    let results = build(selector);
    assert_eq!(results.len(), 1, "seletor {selector:?}");
    (results[0].matcher)(&scopes.to_vec())
}

#[test]
fn simple_identifier_and_conjunction() {
    assert_eq!(score("comment", &["comment"]), 1);
    assert_eq!(score("comment", &["string"]), -1);
    assert_eq!(score("a b", &["a", "b"]), 1);
    assert_eq!(score("a b", &["a"]), -1);
}

#[test]
fn priority_prefixes() {
    assert_eq!(build("a")[0].priority, 0);
    assert_eq!(build("R:a")[0].priority, 1);
    assert_eq!(build("L:a")[0].priority, -1);
    assert_eq!(build("L:  a")[0].priority, -1);
}

#[test]
fn comma_separated_selectors_make_separate_matchers() {
    let results = build("L:a, R:b, c");
    let priorities: Vec<i8> = results.iter().map(|r| r.priority).collect();
    assert_eq!(priorities, vec![-1, 1, 0]);
    assert_eq!((results[1].matcher)(&vec!["b"]), 1);
    assert_eq!((results[2].matcher)(&vec!["b"]), -1);
}

#[test]
fn negation() {
    assert_eq!(score("a -b", &["a"]), 0);
    assert_eq!(score("a -b", &["a", "b"]), -1);
    assert_eq!(score("-b", &["a"]), 0);
    assert_eq!(score("-b", &["b"]), -1);
}

#[test]
fn parentheses_with_pipe_and_comma() {
    assert_eq!(score("x (a | b)", &["x", "b"]), 1);
    assert_eq!(score("x (a | b)", &["x", "c"]), -1);
    assert_eq!(score("(a, b)", &["b"]), 1);
    assert_eq!(score("(a || ,, b)", &["a"]), 1);
    assert_eq!(score("(a b | c)", &["c"]), 1);
    assert_eq!(score("(a b | c)", &["a"]), -1);
}

#[test]
fn unclosed_parenthesis_is_tolerated() {
    assert_eq!(score("(a | b", &["a"]), 1);
}

#[test]
fn identifiers_keep_dots_and_dashes() {
    assert_eq!(score("meta.tag-name.js", &["meta.tag-name.js"]), 1);
}

#[test]
fn empty_selector_makes_no_matcher() {
    assert!(build("").is_empty());
    assert!(build("   ").is_empty());
}
