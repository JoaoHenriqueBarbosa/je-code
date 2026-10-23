//! Testes próprios de `src/vs/base/common/equals.ts` (o VS Code não tem `equals.test.ts`); os valores
//! esperados de `getStructuralKey` saem do Node.

use je_base::dynamic::Dynamic;
use je_base::equals::*;

fn num(n: f64) -> Dynamic {
    Dynamic::Number(n)
}

fn obj(entries: Vec<(&str, Dynamic)>) -> Dynamic {
    Dynamic::object(entries)
}

#[test]
fn strict_and_array_equals() {
    assert!(strict_equals(&1, &1));
    assert!(!strict_equals(&1, &2));
    assert!(strict_equals_c::<String>()(&"a".to_string(), &"a".to_string()));
    assert!(array_equals(&[1, 2, 3], &[1, 2, 3]));
    assert!(!array_equals(&[1, 2, 3], &[1, 2]));
    assert!(!array_equals(&[1, 2, 3], &[1, 2, 4]));
    assert!(array_equals_with(&[1, 2], &[11, 12], |a, b| a % 10 == b % 10));
    assert!(array_equals_c::<i32>()(&vec![1], &vec![1]));
    let by_len: EqualityComparer<String> = std::rc::Rc::new(|a, b| a.len() == b.len());
    assert!(array_equals_c_with(by_len)(&vec!["ab".to_string()], &vec!["cd".to_string()]));
}

#[test]
fn structural_equals_drills_into_arrays_and_plain_objects() {
    assert!(structural_equals(&num(1.0), &num(1.0)));
    assert!(!structural_equals(&num(f64::NAN), &num(f64::NAN)));
    let a = obj(vec![("x", Dynamic::array([num(1.0), obj(vec![("y", Dynamic::string("z"))])])), ("k", Dynamic::Null)]);
    let b = obj(vec![("k", Dynamic::Null), ("x", Dynamic::array([num(1.0), obj(vec![("y", Dynamic::string("z"))])]))]);
    assert!(structural_equals(&a, &b));
    let c = obj(vec![("k", Dynamic::Null), ("x", Dynamic::array([num(1.0), obj(vec![("y", Dynamic::string("w"))])]))]);
    assert!(!structural_equals(&a, &c));
    assert!(!structural_equals(&Dynamic::array([num(1.0)]), &Dynamic::array([num(1.0), num(2.0)])));
    assert!(!structural_equals(&obj(vec![("a", num(1.0))]), &obj(vec![("a", num(1.0)), ("b", num(2.0))])));
    assert!(!structural_equals(&obj(vec![("a", num(1.0))]), &obj(vec![("b", num(1.0))])));
    // Objetos que não são simples (aqui, `Date`) só são iguais por identidade.
    assert!(!structural_equals(&Dynamic::date(1.0), &Dynamic::date(1.0)));
    let date = Dynamic::date(1.0);
    assert!(structural_equals(&date, &date.clone()));
    assert!(structural_equals_c()(&a, &b));
}

#[test]
fn get_structural_key_normalizes_key_order() {
    let a = obj(vec![("b", num(2.0)), ("a", Dynamic::array([num(1.0), Dynamic::Null]))]);
    let b = obj(vec![("a", Dynamic::array([num(1.0), Dynamic::Null])), ("b", num(2.0))]);
    assert_eq!(get_structural_key(&a).as_deref(), Some(r#"{"a":[1,null],"b":2}"#));
    assert_eq!(get_structural_key(&a), get_structural_key(&b));
    assert_eq!(get_structural_key(&Dynamic::Undefined), None);
    assert_eq!(get_structural_key(&Dynamic::string("x")).as_deref(), Some("\"x\""));
}

#[test]
fn get_structural_key_gives_other_objects_a_stable_id() {
    let date = Dynamic::date(0.0);
    let first = get_structural_key(&date).unwrap();
    assert!(first.ends_with("----2b76a038c20c4bcc\""));
    assert_eq!(get_structural_key(&date).unwrap(), first);
    assert_ne!(get_structural_key(&Dynamic::date(0.0)).unwrap(), first);
}

#[test]
fn json_stringify_equals_compares_the_text() {
    let a = obj(vec![("a", num(1.0)), ("b", Dynamic::Undefined)]);
    let b = obj(vec![("a", num(1.0))]);
    assert!(json_stringify_equals(&a, &b));
    assert!(!json_stringify_equals(&obj(vec![("a", num(1.0)), ("b", num(2.0))]), &obj(vec![("b", num(2.0)), ("a", num(1.0))])));
    assert!(json_stringify_equals_c()(&Dynamic::Undefined, &Dynamic::Undefined));
}

struct Name(&'static str);

impl Equatable for Name {
    fn equals(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(other.0)
    }
}

#[test]
fn this_equals_and_equals_if_defined() {
    assert!(this_equals_c::<Name>()(&Name("A"), &Name("a")));
    assert!(equals_if_defined(None::<&i32>, None, |a, b| a == b));
    assert!(!equals_if_defined(Some(&1), None, |a, b| a == b));
    assert!(equals_if_defined(Some(&1), Some(&1), |a, b| a == b));
    let c = equals_if_defined_c::<i32>(std::rc::Rc::new(|a, b| a == b));
    assert!(c(&None, &None));
    assert!(!c(&Some(1), &Some(2)));
}
