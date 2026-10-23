//! Porte de `src/vs/base/common/equals.ts`: comparadores de igualdade combináveis.
//!
//! Cada função tem a variante `*_c` que devolve um [`EqualityComparer`]. As que olham valores
//! JavaScript sem tipo (`structuralEquals`, `getStructuralKey`, `jsonStringifyEquals`) trabalham sobre
//! [`Dynamic`]; as genéricas (`strictEquals`, `arrayEquals`, `equalsIfDefined`, `thisEqualsC`) sobre
//! qualquer `T`, com `PartialEq` onde o original usa `===`.
//!
//! O namespace `equals` do original (`equals.strict`, `equals.array`...) é este próprio módulo.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::JsString;
use crate::arrays;
use crate::dynamic::Dynamic;
use crate::objects::safe_stringify;

/// `EqualityComparer<T>`: decide se dois valores são iguais.
pub type EqualityComparer<T> = Rc<dyn Fn(&T, &T) -> bool>;

/// `IEquatable<T>`.
pub trait Equatable {
    fn equals(&self, other: &Self) -> bool;
}

/// `strictEquals`: igualdade estrita (`===`).
pub fn strict_equals<T: PartialEq>(a: &T, b: &T) -> bool {
    a == b
}

/// `strictEqualsC`.
pub fn strict_equals_c<T: PartialEq + 'static>() -> EqualityComparer<T> {
    Rc::new(|a, b| a == b)
}

/// `arrayEquals(a, b)`: os itens de dois arrays são iguais (igualdade estrita).
pub fn array_equals<T: PartialEq>(a: &[T], b: &[T]) -> bool {
    arrays::equals(Some(a), Some(b), strict_equals)
}

/// `arrayEquals(a, b, itemEquals)`.
pub fn array_equals_with<T>(a: &[T], b: &[T], item_equals: impl Fn(&T, &T) -> bool) -> bool {
    arrays::equals(Some(a), Some(b), item_equals)
}

/// `arrayEqualsC()`.
pub fn array_equals_c<T: PartialEq + 'static>() -> EqualityComparer<Vec<T>> {
    Rc::new(|a, b| array_equals(a, b))
}

/// `arrayEqualsC(itemEquals)`.
pub fn array_equals_c_with<T: 'static>(item_equals: EqualityComparer<T>) -> EqualityComparer<Vec<T>> {
    Rc::new(move |a, b| array_equals_with(a, b, |x, y| item_equals(x, y)))
}

/// Um objeto é "simples" quando o protótipo dele é `Object.prototype`: criado por `{}`, sem classe.
fn is_plain_object(value: &Dynamic) -> bool {
    matches!(value, Dynamic::Object(o) if o.class().is_none())
}

/// `structuralEquals`: desce em arrays (itens ordenados) e objetos simples (chaves sem ordem) e usa
/// igualdade estrita no resto.
pub fn structural_equals(a: &Dynamic, b: &Dynamic) -> bool {
    if a.strict_equals(b) {
        return true;
    }

    if let (Dynamic::Array(x), Dynamic::Array(y)) = (a, b) {
        let (x, y) = (x.to_vec(), y.to_vec());
        if x.len() != y.len() {
            return false;
        }
        return x.iter().zip(&y).all(|(p, q)| structural_equals(p, q));
    }

    if is_plain_object(a) && is_plain_object(b) {
        let (keys_a, keys_b) = (a.own_keys(), b.own_keys());
        if keys_a.len() != keys_b.len() {
            return false;
        }
        for key in &keys_a {
            if !keys_b.contains(key) {
                return false;
            }
            if !structural_equals(&a.get(key), &b.get(key)) {
                return false;
            }
        }
        return true;
    }

    false
}

/// `structuralEqualsC`.
pub fn structural_equals_c() -> EqualityComparer<Dynamic> {
    Rc::new(structural_equals)
}

thread_local! {
    static OBJECT_ID: Cell<u64> = const { Cell::new(0) };
    /// O `WeakMap<object, number>` do original. Guarda o próprio valor para o endereço não ser reaproveitado
    /// por outro objeto enquanto o id vale (o `WeakMap` do JS tem a mesma garantia, por referência fraca).
    static OBJECT_IDS: RefCell<HashMap<usize, (u64, Dynamic)>> = RefCell::new(HashMap::new());
}

/// `getStructuralKey`: `getStructuralKey(a) === getStructuralKey(b)` se e só se `structuralEquals(a, b)`
/// (supondo que não sejam estruturas cíclicas). `None` é o `undefined` do `JSON.stringify`.
pub fn get_structural_key(t: &Dynamic) -> Option<String> {
    safe_stringify(&to_normalized_json_structure(t))
}

fn to_normalized_json_structure(t: &Dynamic) -> Dynamic {
    if let Dynamic::Array(array) = t {
        return Dynamic::array(array.to_vec().iter().map(to_normalized_json_structure));
    }

    if matches!(t, Dynamic::Array(_) | Dynamic::Object(_) | Dynamic::RegExp(_) | Dynamic::Date(_) | Dynamic::TypedArray(_)) {
        if is_plain_object(t) {
            let mut keys = t.own_keys();
            // `Array#sort()` sem comparador: por unidades UTF-16.
            keys.sort_by(|a, b| a.units().cmp(b.units()));
            return Dynamic::object(keys.into_iter().map(|key| {
                let value = to_normalized_json_structure(&t.get(&key));
                (key, value)
            }));
        }
        let identity = t.identity().expect("objeto tem identidade");
        let id = OBJECT_IDS.with(|ids| {
            let mut ids = ids.borrow_mut();
            if let Some((id, _)) = ids.get(&identity) {
                return *id;
            }
            let id = OBJECT_ID.with(|counter| {
                let id = counter.get();
                counter.set(id + 1);
                id
            });
            ids.insert(identity, (id, t.clone()));
            id
        });
        // String aleatória para evitar colisões.
        return Dynamic::String(JsString::from(format!("{id}----2b76a038c20c4bcc")));
    }
    t.clone()
}

/// `jsonStringifyEquals`: dois itens são iguais se as representações em JSON são iguais.
pub fn json_stringify_equals(a: &Dynamic, b: &Dynamic) -> bool {
    safe_stringify(a) == safe_stringify(b)
}

/// `jsonStringifyEqualsC`.
pub fn json_stringify_equals_c() -> EqualityComparer<Dynamic> {
    Rc::new(json_stringify_equals)
}

/// `thisEqualsC`: usa `item.equals(other)`.
pub fn this_equals_c<T: Equatable + 'static>() -> EqualityComparer<T> {
    Rc::new(|a, b| a.equals(b))
}

/// `equalsIfDefined`: dois itens ausentes (`undefined` ou `null`, aqui `None`) são iguais, ou iguais
/// segundo o comparador.
pub fn equals_if_defined<T>(v1: Option<&T>, v2: Option<&T>, equals: impl Fn(&T, &T) -> bool) -> bool {
    match (v1, v2) {
        (Some(a), Some(b)) => equals(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// `equalsIfDefinedC`.
pub fn equals_if_defined_c<T: 'static>(equals: EqualityComparer<T>) -> EqualityComparer<Option<T>> {
    Rc::new(move |v1, v2| equals_if_defined(v1.as_ref(), v2.as_ref(), |a, b| equals(a, b)))
}
