//! Testes próprios de `src/vs/base/common/validation.ts` (o VS Code não tem `validation.test.ts`):
//! mensagens de erro, ordem das propriedades e o schema JSON de cada combinador.

use je_base::dynamic::Dynamic;
use je_base::objects::safe_stringify;
use je_base::validation::*;

fn obj(entries: Vec<(&str, Dynamic)>) -> Dynamic {
    Dynamic::object(entries)
}

fn schema_text(validator: &ValidatorRef) -> String {
    safe_stringify(&validator.get_json_schema()).unwrap()
}

#[test]
fn primitives() {
    assert!(v_string().validate(&Dynamic::string("a")).is_ok());
    assert_eq!(v_string().validate(&Dynamic::Number(1.0)).unwrap_err().message, "Expected string, but got number");
    assert_eq!(v_number().validate(&Dynamic::Null).unwrap_err().message, "Expected number, but got object");
    assert_eq!(v_boolean().validate(&Dynamic::Undefined).unwrap_err().message, "Expected boolean, but got undefined");
    assert!(v_obj_any().validate(&Dynamic::array([])).is_ok());
    assert_eq!(v_undefined().validate(&Dynamic::Bool(true)).unwrap_err().message, "Expected undefined, but got boolean");
    assert!(v_unknown().validate(&Dynamic::Undefined).is_ok());
    assert_eq!(schema_text(&v_string()), r#"{"type":"string"}"#);
    assert_eq!(schema_text(&v_unchecked()), "{}");
}

#[test]
fn object_with_optional_property() {
    let validator = v_obj([("name", v_string().into()), ("age", v_optional_prop(v_number()))]);
    let ok = validator.validate(&obj(vec![("name", Dynamic::string("x")), ("extra", Dynamic::Null)])).unwrap();
    assert_eq!(safe_stringify(&ok).unwrap(), r#"{"name":"x"}"#);
    let err = validator.validate(&obj(vec![("name", Dynamic::string("x")), ("age", Dynamic::string("1"))])).unwrap_err();
    assert_eq!(err.message, "Error in property 'age': Expected number, but got string");
    assert_eq!(validator.validate(&Dynamic::Null).unwrap_err().message, "Expected object");
    assert_eq!(validator.validate(&obj(vec![])).unwrap_err().message, "Error in property 'name': Expected string, but got undefined");
    assert_eq!(
        schema_text(&validator),
        r#"{"type":"object","properties":{"name":{"type":"string"},"age":{"type":"number"}},"required":["name"]}"#
    );
}

#[test]
fn array_and_tuple() {
    let array = v_array(v_number());
    assert!(array.validate(&Dynamic::array([Dynamic::Number(1.0)])).is_ok());
    assert_eq!(
        array.validate(&Dynamic::array([Dynamic::Number(1.0), Dynamic::string("a")])).unwrap_err().message,
        "Error in element 1: Expected number, but got string"
    );
    assert_eq!(array.validate(&Dynamic::Null).unwrap_err().message, "Expected array");
    let tuple = v_tuple([v_string(), v_number()]);
    assert_eq!(
        tuple.validate(&Dynamic::array([Dynamic::string("a")])).unwrap_err().message,
        "Expected tuple of length 2, but got 1"
    );
    assert!(tuple.validate(&Dynamic::array([Dynamic::string("a"), Dynamic::Number(2.0)])).is_ok());
    assert_eq!(schema_text(&array), r#"{"type":"array","items":{"type":"number"}}"#);
    assert_eq!(schema_text(&tuple), r#"{"type":"array","items":[{"type":"string"},{"type":"number"}]}"#);
}

#[test]
fn union_enum_literal_lazy_and_ref() {
    let union = v_union([v_string(), v_number(), v_undefined()]);
    assert!(union.validate(&Dynamic::Number(1.0)).is_ok());
    assert!(union.validate(&Dynamic::Undefined).is_ok());
    assert_eq!(union.validate(&Dynamic::Null).unwrap_err().message, "Expected undefined, but got object");
    assert_eq!(schema_text(&union), r#"{"oneOf":[{"type":"string"},{"type":"number"}]}"#);

    let e = v_enum(["a", "b"]);
    assert!(e.validate(&Dynamic::string("a")).is_ok());
    assert_eq!(e.validate(&Dynamic::string("c")).unwrap_err().message, "Expected one of: a, b");
    assert_eq!(schema_text(&e), r#"{"enum":["a","b"]}"#);

    let literal = v_literal("x");
    assert!(literal.validate(&Dynamic::string("x")).is_ok());
    assert_eq!(literal.validate(&Dynamic::string("y")).unwrap_err().message, "Expected: x");
    assert_eq!(schema_text(&literal), r#"{"const":"x"}"#);

    let lazy = v_lazy(v_boolean);
    assert!(lazy.validate(&Dynamic::Bool(true)).is_ok());
    assert_eq!(schema_text(&lazy), r#"{"type":"boolean"}"#);

    let referenced = v_with_json_schema_ref("#/$defs/x", v_string());
    assert!(referenced.validate(&Dynamic::string("a")).is_ok());
    assert!(referenced.validate(&Dynamic::Null).is_err());
    assert_eq!(schema_text(&referenced), r##"{"$ref":"#/$defs/x"}"##);
}
