//! Porte de `src/vs/base/common/verifier.ts`: verificadores que devolvem o valor se ele tem o tipo certo e o
//! valor padrão se não tem.
//!
//! O `T` do original some em tempo de execução, e o `unknown` é [`Dynamic`]: todo verificador recebe e
//! devolve um [`Dynamic`]. `SetVerifier` (`value instanceof Set`) nunca reconhece nada, porque o
//! [`Dynamic`] não modela `Set` (ver o topo de `dynamic.rs`): devolve sempre o padrão.

use std::rc::Rc;

use crate::dynamic::{Dynamic, ObjectRef};
use crate::js_string::JsString;
use crate::types::is_object;

/// `IVerifier<T>`.
pub trait IVerifier {
    fn verify(&self, value: &Dynamic) -> Dynamic;
}

/// A classe base `Verifier<T>`: o padrão e o teste de tipo.
struct Base {
    default_value: Dynamic,
}

impl Base {
    fn verify(&self, value: &Dynamic, is_type: impl Fn(&Dynamic) -> bool) -> Dynamic {
        if is_type(value) { value.clone() } else { self.default_value.clone() }
    }
}

pub struct BooleanVerifier(Base);

impl BooleanVerifier {
    pub fn new(default_value: bool) -> Self {
        Self(Base { default_value: Dynamic::Bool(default_value) })
    }
}

impl IVerifier for BooleanVerifier {
    fn verify(&self, value: &Dynamic) -> Dynamic {
        self.0.verify(value, |v| matches!(v, Dynamic::Bool(_)))
    }
}

pub struct NumberVerifier(Base);

impl NumberVerifier {
    pub fn new(default_value: f64) -> Self {
        Self(Base { default_value: Dynamic::Number(default_value) })
    }
}

impl IVerifier for NumberVerifier {
    fn verify(&self, value: &Dynamic) -> Dynamic {
        self.0.verify(value, |v| matches!(v, Dynamic::Number(_)))
    }
}

pub struct SetVerifier(Base);

impl SetVerifier {
    pub fn new(default_value: Dynamic) -> Self {
        Self(Base { default_value })
    }
}

impl IVerifier for SetVerifier {
    fn verify(&self, value: &Dynamic) -> Dynamic {
        // `value instanceof Set`: o Dynamic não tem Set.
        self.0.verify(value, |_| false)
    }
}

pub struct EnumVerifier {
    base: Base,
    allowed_values: Vec<Dynamic>,
}

impl EnumVerifier {
    pub fn new(default_value: Dynamic, allowed_values: Vec<Dynamic>) -> Self {
        Self { base: Base { default_value }, allowed_values }
    }
}

/// `SameValueZero`, a igualdade de `Array.prototype.includes`.
fn same_value_zero(a: &Dynamic, b: &Dynamic) -> bool {
    match (a, b) {
        (Dynamic::Number(x), Dynamic::Number(y)) if x.is_nan() && y.is_nan() => true,
        _ => a.strict_equals(b),
    }
}

impl IVerifier for EnumVerifier {
    fn verify(&self, value: &Dynamic) -> Dynamic {
        self.base.verify(value, |v| self.allowed_values.iter().any(|allowed| same_value_zero(allowed, v)))
    }
}

pub struct ObjectVerifier {
    base: Base,
    verifier: Vec<(JsString, Rc<dyn IVerifier>)>,
}

impl ObjectVerifier {
    pub fn new(default_value: Dynamic, verifier: Vec<(JsString, Rc<dyn IVerifier>)>) -> Self {
        Self { base: Base { default_value }, verifier }
    }
}

impl IVerifier for ObjectVerifier {
    fn verify(&self, value: &Dynamic) -> Dynamic {
        if !is_object(value) {
            return self.base.default_value.clone();
        }
        verify_object(&self.verifier, value)
    }
}

/// `verifyObject`: um objeto novo (sem protótipo, no original) só com as chaves dos verificadores.
pub fn verify_object(verifiers: &[(JsString, Rc<dyn IVerifier>)], value: &Dynamic) -> Dynamic {
    let result = ObjectRef::new();
    for (key, verifier) in verifiers {
        result.set(key, verifier.verify(&value.get(key)));
    }
    Dynamic::Object(result)
}
