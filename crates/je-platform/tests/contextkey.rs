//! Porte de `src/vs/platform/contextkey/test/common/contextkey.test.ts` (suíte `ContextKeyExpr`), caso a caso e na mesma ordem.
//!
//! Divergências: o `createContext(ctx)` do original recebe um objeto JavaScript, aqui um valor dinâmico
//! (objeto). Os literais `/d.*/` e `/D/i` do `testBatch` viram [`ContextRegExp`].

mod common;

use common::*;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::platform::Platform;
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression, ContextRegExp, IContext, implies};
use je_platform::js::{loose_equals, to_js_string};

struct TestContext(Dynamic);

impl IContext for TestContext {
    fn get_value(&self, key: &str) -> Dynamic {
        self.0.get_str(key)
    }
}

fn create_context(ctx: &str) -> TestContext {
    TestContext(j(ctx))
}

fn re(pattern: &str) -> ContextRegExp {
    ContextRegExp::new(pattern, "").unwrap()
}

fn deserialize(text: &str) -> Option<ContextKeyExpression> {
    ContextKeyExpr::deserialize(Some(text))
}

#[test]
fn context_key_expr_equals() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let a = ContextKeyExpr::and([
        Some(ContextKeyExpr::has("a1")),
        ContextKeyExpr::and([Some(ContextKeyExpr::has("and.a"))]),
        Some(ContextKeyExpr::has("a2")),
        Some(ContextKeyExpr::regex("d3", re("d.*"))),
        Some(ContextKeyExpr::regex("d4", re("\\*\\*3*"))),
        Some(ContextKeyExpr::equals("b1", "bb1")),
        Some(ContextKeyExpr::equals("b2", "bb2")),
        Some(ContextKeyExpr::not_equals("c1", "cc1")),
        Some(ContextKeyExpr::not_equals("c2", "cc2")),
        Some(ContextKeyExpr::not("d1")),
        Some(ContextKeyExpr::not("d2")),
    ])
    .unwrap();
    let b = ContextKeyExpr::and([
        Some(ContextKeyExpr::equals("b2", "bb2")),
        Some(ContextKeyExpr::not_equals("c1", "cc1")),
        Some(ContextKeyExpr::not("d1")),
        Some(ContextKeyExpr::regex("d4", re("\\*\\*3*"))),
        Some(ContextKeyExpr::not_equals("c2", "cc2")),
        Some(ContextKeyExpr::has("a2")),
        Some(ContextKeyExpr::equals("b1", "bb1")),
        Some(ContextKeyExpr::regex("d3", re("d.*"))),
        Some(ContextKeyExpr::has("a1")),
        ContextKeyExpr::and([Some(ContextKeyExpr::equals("and.a", true))]),
        Some(ContextKeyExpr::not("d2")),
    ])
    .unwrap();
    assert!(a.equals(&b), "expressions should be equal");
}

#[test]
fn issue_134942_equals_in_comparator_expressions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_equals = |expr: Option<ContextKeyExpression>, text: &str| {
        let deserialized = deserialize(text);
        let expr = expr.expect(text);
        let deserialized = deserialized.expect(text);
        assert!(expr.equals(&deserialized), "{text}");
    };
    test_equals(Some(ContextKeyExpr::greater("value", 0)), "value > 0");
    test_equals(Some(ContextKeyExpr::greater_equals("value", 0)), "value >= 0");
    test_equals(Some(ContextKeyExpr::smaller("value", 0)), "value < 0");
    test_equals(Some(ContextKeyExpr::smaller_equals("value", 0)), "value <= 0");
}

#[test]
fn normalize() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let key1_is_true = ContextKeyExpr::equals("key1", true);
    let key1_is_not_false = ContextKeyExpr::not_equals("key1", false);
    let key1_is_false = ContextKeyExpr::equals("key1", false);
    let key1_is_not_true = ContextKeyExpr::not_equals("key1", true);

    assert!(key1_is_true.equals(&ContextKeyExpr::has("key1")));
    assert!(key1_is_not_false.equals(&ContextKeyExpr::has("key1")));
    assert!(key1_is_false.equals(&ContextKeyExpr::not("key1")));
    assert!(key1_is_not_true.equals(&ContextKeyExpr::not("key1")));
}

#[test]
fn evaluate() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let context = create_context("{'a': true, 'b': false, 'c': '5', 'd': 'd'}");
    let test_expression = |expr: &str, expected: bool| {
        let rules = deserialize(expr);
        assert_eq!(rules.unwrap().evaluate(&context), expected, "{expr}");
    };
    let test_batch = |expr: &str, value: Dynamic| {
        let truthy = value.is_truthy();
        test_expression(expr, truthy);
        test_expression(&format!("{expr} == true"), truthy);
        test_expression(&format!("{expr} != true"), !truthy);
        test_expression(&format!("{expr} == false"), !truthy);
        test_expression(&format!("{expr} != false"), truthy);
        test_expression(&format!("{expr} == 5"), loose_equals(&value, &s("5")));
        test_expression(&format!("{expr} != 5"), !loose_equals(&value, &s("5")));
        test_expression(&format!("!{expr}"), !truthy);
        let text = to_js_string(&value);
        test_expression(&format!("{expr} =~ /d.*/"), re("d.*").test(text.units()));
        test_expression(&format!("{expr} =~ /D/i"), ContextRegExp::new("D", "i").unwrap().test(text.units()));
    };

    test_batch("a", Dynamic::Bool(true));
    test_batch("b", Dynamic::Bool(false));
    test_batch("c", s("5"));
    test_batch("d", s("d"));
    test_batch("z", Dynamic::Undefined);

    test_expression("true", true);
    test_expression("false", false);
    test_expression("a && !b", true);
    test_expression("a && b", false);
    test_expression("a && !b && c == 5", true);
    test_expression("d =~ /e.*/", false);

    // precedence test: false && true || true === true because && is evaluated first
    test_expression("b && a || a", true);

    test_expression("a || b", true);
    test_expression("b || b", false);
    test_expression("b && a || a && b", false);
}

#[test]
fn negate() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_negate = |expr: &str, expected: &str| {
        let actual = deserialize(expr).unwrap().negate().serialize();
        assert_eq!(actual, expected);
    };
    test_negate("true", "false");
    test_negate("false", "true");
    test_negate("a", "!a");
    test_negate("a && b || c", "!a && !c || !b && !c");
    test_negate("a && b || c || d", "!a && !c && !d || !b && !c && !d");
    test_negate("!a && !b || !c && !d", "a && c || a && d || b && c || b && d");
    test_negate(
        "!a && !b || !c && !d || !e && !f",
        "a && c && e || a && c && f || a && d && e || a && d && f || b && c && e || b && c && f || b && d && e || b && d && f",
    );
}

#[test]
fn false_true() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let platform = Platform::detect();
    let test_normalize = |expr: &str, expected: &str| {
        let actual = deserialize(expr).unwrap().serialize();
        assert_eq!(actual, expected);
    };
    let bool_str = |b: bool| if b { "true" } else { "false" };
    test_normalize("true", "true");
    test_normalize("!true", "false");
    test_normalize("false", "false");
    test_normalize("!false", "true");
    test_normalize("a && true", "a");
    test_normalize("a && false", "false");
    test_normalize("a || true", "true");
    test_normalize("a || false", "a");
    test_normalize("isMac", bool_str(platform.is_macintosh));
    test_normalize("isLinux", bool_str(platform.is_linux));
    test_normalize("isWindows", bool_str(platform.is_windows));
    test_normalize("isChromeOS", bool_str(platform.is_chrome_os));
}

#[test]
fn issue_101015_distribute_or() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let t = |expr1: &str, expr2: &str, expected: Option<&str>| {
        let e1 = deserialize(expr1);
        let e2 = deserialize(expr2);
        let actual = ContextKeyExpr::and([e1, e2]).map(|e| e.serialize());
        assert_eq!(actual.as_deref(), expected);
    };
    t("a", "b", Some("a && b"));
    t("a || b", "c", Some("a && c || b && c"));
    t("a || b", "c || d", Some("a && c || a && d || b && c || b && d"));
    t("a || b", "c && d", Some("a && c && d || b && c && d"));
    t("a || b", "c && d || e", Some("a && e || b && e || a && c && d || b && c && d"));
}

#[test]
fn context_key_in_expr() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ainb = deserialize("a in b").unwrap();
    let eval = |ctx: &str| ainb.evaluate(&create_context(ctx));
    assert!(eval("{'a': 3, 'b': [3, 2, 1]}"));
    assert!(eval("{'a': 3, 'b': [1, 2, 3]}"));
    assert!(!eval("{'a': 3, 'b': [1, 2]}"));
    assert!(!eval("{'a': 3}"));
    assert!(!eval("{'a': 3, 'b': null}"));
    assert!(eval("{'a': 'x', 'b': ['x']}"));
    assert!(!eval("{'a': 'x', 'b': ['y']}"));
    assert!(!eval("{'a': 'x', 'b': {}}"));
    assert!(eval("{'a': 'x', 'b': {'x': false}}"));
    assert!(eval("{'a': 'x', 'b': {'x': true}}"));
    assert!(!eval("{'a': 'prototype', 'b': {}}"));

    // file URI case-insensitive comparison on Windows
    if Platform::detect().is_windows {
        // Array source: file URIs with different casing should match on Windows
        assert!(eval("{'a': 'file:///c%3A/Users/path/file.ts', 'b': ['file:///c%3A/users/path/file.ts']}"));
        assert!(eval("{'a': 'file:///c%3A/users/path/file.ts', 'b': ['file:///c%3A/Users/path/file.ts']}"));
        // Object source: file URIs with different casing should match on Windows
        assert!(eval("{'a': 'file:///c%3A/Users/path/file.ts', 'b': {'file:///c%3A/users/path/file.ts': true}}"));
        // Non-file URIs should still be case-sensitive
        assert!(!eval("{'a': 'git:/path/File.ts', 'b': ['git:/path/file.ts']}"));
        // Exact match still works
        assert!(eval("{'a': 'file:///c%3A/Users/path/file.ts', 'b': ['file:///c%3A/Users/path/file.ts']}"));
    }
}

#[test]
fn context_key_not_in_expr() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let a_not_in_b = deserialize("a not in b").unwrap();
    let eval = |ctx: &str| a_not_in_b.evaluate(&create_context(ctx));
    assert!(!eval("{'a': 3, 'b': [3, 2, 1]}"));
    assert!(!eval("{'a': 3, 'b': [1, 2, 3]}"));
    assert!(eval("{'a': 3, 'b': [1, 2]}"));
    assert!(eval("{'a': 3}"));
    assert!(eval("{'a': 3, 'b': null}"));
    assert!(!eval("{'a': 'x', 'b': ['x']}"));
    assert!(eval("{'a': 'x', 'b': ['y']}"));
    assert!(eval("{'a': 'x', 'b': {}}"));
    assert!(!eval("{'a': 'x', 'b': {'x': false}}"));
    assert!(!eval("{'a': 'x', 'b': {'x': true}}"));
    assert!(eval("{'a': 'prototype', 'b': {}}"));

    // file URI case-insensitive comparison on Windows
    if Platform::detect().is_windows {
        assert!(!eval("{'a': 'file:///c%3A/Users/path/file.ts', 'b': ['file:///c%3A/users/path/file.ts']}"));
        assert!(!eval("{'a': 'file:///c%3A/users/path/file.ts', 'b': ['file:///c%3A/Users/path/file.ts']}"));
        assert!(eval("{'a': 'git:/path/File.ts', 'b': ['git:/path/file.ts']}"));
    }
}

#[test]
fn issue_106524_distributing_and_should_normalize() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = ContextKeyExpr::and([ContextKeyExpr::or([ContextKeyExpr::has("a"), ContextKeyExpr::has("b")]), Some(ContextKeyExpr::has("c"))]);
    let expected = ContextKeyExpr::or([
        ContextKeyExpr::and([ContextKeyExpr::has("a"), ContextKeyExpr::has("c")]),
        ContextKeyExpr::and([ContextKeyExpr::has("b"), ContextKeyExpr::has("c")]),
    ]);
    assert!(actual.unwrap().equals(&expected.unwrap()));
}

#[test]
fn issue_129625_removes_duplicated_terms_in_or_expressions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ContextKeyExpr::or([ContextKeyExpr::has("A"), ContextKeyExpr::has("B"), ContextKeyExpr::has("A")]).unwrap();
    assert_eq!(expr.serialize(), "A || B");
}

#[test]
fn resolves_true_constant_or_expressions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ContextKeyExpr::or([ContextKeyExpr::has("A"), ContextKeyExpr::not("A")]).unwrap();
    assert_eq!(expr.serialize(), "true");
}

#[test]
fn resolves_false_constant_and_expressions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ContextKeyExpr::and([ContextKeyExpr::has("A"), ContextKeyExpr::not("A")]).unwrap();
    assert_eq!(expr.serialize(), "false");
}

#[test]
fn issue_129625_removes_duplicated_terms_in_and_expressions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ContextKeyExpr::and([ContextKeyExpr::has("A"), ContextKeyExpr::has("B"), ContextKeyExpr::has("A")]).unwrap();
    assert_eq!(expr.serialize(), "A && B");
}

#[test]
fn issue_129625_remove_duplicated_terms_when_negating() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expr = ContextKeyExpr::and([Some(ContextKeyExpr::has("A")), ContextKeyExpr::or([ContextKeyExpr::has("B1"), ContextKeyExpr::has("B2")])]).unwrap();
    assert_eq!(expr.serialize(), "A && B1 || A && B2");
    assert_eq!(expr.negate().serialize(), "!A || !A && !B1 || !A && !B2 || !B1 && !B2");
    assert_eq!(expr.negate().negate().serialize(), "A && B1 || A && B2");
    assert_eq!(expr.negate().negate().negate().serialize(), "!A || !A && !B1 || !A && !B2 || !B1 && !B2");
}

#[test]
fn issue_129625_remove_redundant_terms_in_or_expressions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let str_implies = |p0: &str, q0: &str| {
        let p = deserialize(p0).unwrap();
        let q = deserialize(q0).unwrap();
        implies(&p, &q)
    };
    assert!(str_implies("a && b", "a"));
    assert!(!str_implies("a", "a && b"));
}

#[test]
fn implies_cases() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let str_implies = |p0: &str, q0: &str| {
        let p = deserialize(p0).unwrap();
        let q = deserialize(q0).unwrap();
        implies(&p, &q)
    };
    assert!(str_implies("a", "a"));
    assert!(str_implies("a", "a || b"));
    assert!(!str_implies("a", "a && b"));
    assert!(!str_implies("a", "a && b || a && c"));
    assert!(str_implies("a && b", "a"));
    assert!(str_implies("a && b", "b"));
    assert!(str_implies("a && b", "a && b || c"));
    assert!(!str_implies("a || b", "a || c"));
    assert!(str_implies("a || b", "a || b"));
    assert!(str_implies("a && b", "a && b"));
    assert!(str_implies("a || b", "a || b || c"));
    assert!(str_implies("c && a && b", "c && a"));
}

#[test]
fn greater_greater_equals_smaller_smaller_equals_evaluate() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let check_evaluate = |expr: &str, ctx: &str, expected: bool| {
        let parsed = deserialize(expr).unwrap();
        assert_eq!(parsed.evaluate(&create_context(ctx)), expected, "{expr} com {ctx}");
    };

    check_evaluate("a > 1", "{}", false);
    check_evaluate("a > 1", "{'a': 0}", false);
    check_evaluate("a > 1", "{'a': 1}", false);
    check_evaluate("a > 1", "{'a': 2}", true);
    check_evaluate("a > 1", "{'a': '0'}", false);
    check_evaluate("a > 1", "{'a': '1'}", false);
    check_evaluate("a > 1", "{'a': '2'}", true);
    check_evaluate("a > 1", "{'a': 'a'}", false);

    check_evaluate("a > 10", "{'a': 2}", false);
    check_evaluate("a > 10", "{'a': 11}", true);
    check_evaluate("a > 10", "{'a': '11'}", true);
    check_evaluate("a > 10", "{'a': '2'}", false);
    check_evaluate("a > 10", "{'a': '11'}", true);

    check_evaluate("a > 1.1", "{'a': 1}", false);
    check_evaluate("a > 1.1", "{'a': 2}", true);
    check_evaluate("a > 1.1", "{'a': 11}", true);
    check_evaluate("a > 1.1", "{'a': '1.1'}", false);
    check_evaluate("a > 1.1", "{'a': '2'}", true);
    check_evaluate("a > 1.1", "{'a': '11'}", true);

    check_evaluate("a > b", "{'a': 'b'}", false);
    check_evaluate("a > b", "{'a': 'c'}", false);
    check_evaluate("a > b", "{'a': 1000}", false);

    check_evaluate("a >= 2", "{'a': '1'}", false);
    check_evaluate("a >= 2", "{'a': '2'}", true);
    check_evaluate("a >= 2", "{'a': '3'}", true);

    check_evaluate("a < 2", "{'a': '1'}", true);
    check_evaluate("a < 2", "{'a': '2'}", false);
    check_evaluate("a < 2", "{'a': '3'}", false);

    check_evaluate("a <= 2", "{'a': '1'}", true);
    check_evaluate("a <= 2", "{'a': '2'}", true);
    check_evaluate("a <= 2", "{'a': '3'}", false);
}

#[test]
fn greater_greater_equals_smaller_smaller_equals_negate() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let check_negate = |expr: &str, expected: &str| {
        let a = deserialize(expr).unwrap();
        let b = a.negate();
        assert_eq!(b.serialize(), expected);
    };

    check_negate("a > 1", "a <= 1");
    check_negate("a > 1.1", "a <= 1.1");
    check_negate("a > b", "a <= b");

    check_negate("a >= 1", "a < 1");
    check_negate("a >= 1.1", "a < 1.1");
    check_negate("a >= b", "a < b");

    check_negate("a < 1", "a >= 1");
    check_negate("a < 1.1", "a >= 1.1");
    check_negate("a < b", "a >= b");

    check_negate("a <= 1", "a > 1");
    check_negate("a <= 1.1", "a > 1.1");
    check_negate("a <= b", "a > b");
}

#[test]
fn issue_111899_context_keys_can_use_less_or_greater() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let actual = deserialize("editorTextFocus && vim.active && vim.use<C-r>").unwrap();
    assert!(actual.equals(
        &ContextKeyExpr::and([
            ContextKeyExpr::has("editorTextFocus"),
            ContextKeyExpr::has("vim.active"),
            ContextKeyExpr::has("vim.use<C-r>"),
        ])
        .unwrap()
    ));
}
