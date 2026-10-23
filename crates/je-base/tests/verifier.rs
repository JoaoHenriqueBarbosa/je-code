//! Testes próprios de `verifier.rs`, que o VS Code não testa. Não é porte de teste do VS Code.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::js_string::JsString;
use je_base::verifier::{BooleanVerifier, EnumVerifier, IVerifier, NumberVerifier, ObjectVerifier, verify_object};

fn key(name: &str) -> JsString {
    JsString::from(name)
}

#[test]
fn primitive_verifiers_fall_back_to_the_default() {
    let boolean = BooleanVerifier::new(true);
    assert!(boolean.verify(&Dynamic::Bool(false)).strict_equals(&Dynamic::Bool(false)));
    assert!(boolean.verify(&Dynamic::from("x")).strict_equals(&Dynamic::Bool(true)));
    let number = NumberVerifier::new(7.0);
    assert!(number.verify(&Dynamic::Number(3.0)).strict_equals(&Dynamic::Number(3.0)));
    assert!(number.verify(&Dynamic::Undefined).strict_equals(&Dynamic::Number(7.0)));
}

#[test]
fn enum_verifier_checks_membership() {
    let verifier = EnumVerifier::new(Dynamic::from("a"), vec![Dynamic::from("a"), Dynamic::from("b"), Dynamic::Number(f64::NAN)]);
    assert!(verifier.verify(&Dynamic::from("b")).strict_equals(&Dynamic::from("b")));
    assert!(verifier.verify(&Dynamic::from("c")).strict_equals(&Dynamic::from("a")));
    // `includes` usa SameValueZero: NaN pertence à lista.
    assert!(matches!(verifier.verify(&Dynamic::Number(f64::NAN)), Dynamic::Number(n) if n.is_nan()));
}

#[test]
fn object_verifier_keeps_only_the_verified_keys() {
    let verifiers: Vec<(JsString, Rc<dyn IVerifier>)> = vec![(key("on"), Rc::new(BooleanVerifier::new(false))), (key("n"), Rc::new(NumberVerifier::new(1.0)))];
    let verifier = ObjectVerifier::new(Dynamic::Null, verifiers.clone());
    let input = Dynamic::object([("on", Dynamic::Bool(true)), ("n", Dynamic::from("oops")), ("extra", Dynamic::Number(9.0))]);
    let result = verifier.verify(&input);
    assert_eq!(result.own_keys(), vec![key("on"), key("n")]);
    assert!(result.get_str("on").strict_equals(&Dynamic::Bool(true)));
    assert!(result.get_str("n").strict_equals(&Dynamic::Number(1.0)));
    assert!(verifier.verify(&Dynamic::from("not an object")).is_null());
    assert_eq!(verify_object(&verifiers, &Dynamic::object::<&str>([])).own_keys().len(), 2);
}
