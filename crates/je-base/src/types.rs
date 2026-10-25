//! Porte de `src/vs/base/common/types.ts`.
//!
//! Os predicados e asserções que olham o tipo em tempo de execução recebem um
//! [`Dynamic`](crate::dynamic::Dynamic) (o `unknown` do original; ver `dynamic.rs` para a
//! representação). `assertReturnsDefined`/`assertDefined` também aceitam `Option<T>` (o `T | null |
//! undefined` tipado), pela característica [`Nullable`].
//!
//! Não portado, por existir só no sistema de tipos do TypeScript e não ter forma em tempo de execução:
//! `upcast`, `AddFirstParameterToFunctions`, `AtLeastOne`, `OmitOptional`, `Mutable`, `DeepImmutable`,
//! `SingleOrMany` (o equivalente é `arrays::OneOrMany`), `WithDefinedProps`, `DeepRequiredNonNullable`,
//! `DeepPartial`, `PartialExcept`. `typeCheck` fica como função vazia (verificação em compilação).
//! `isAsyncIterable` não é portado: nenhum [`Dynamic`] pode ser iterável assíncrono.
//! `assertReturnsAllDefined` recebe uma lista de `Dynamic` (as sobrecargas de 2 a 4 argumentos
//! tipados do TypeScript não têm equivalente).

use crate::assert::{ErrorOrMessage, assert};
use crate::dynamic::{Dynamic, FunctionRef, Truthy};
use crate::errors::{self, Error};
use crate::js_string::JsString;

/// `T | null | undefined`: o que as asserções de "definido" aceitam.
pub trait Nullable {
    type Value;

    /// O valor, ou `None` se for `undefined`/`null`.
    fn into_defined(self) -> Option<Self::Value>;
}

impl<T> Nullable for Option<T> {
    type Value = T;

    fn into_defined(self) -> Option<T> {
        self
    }
}

impl Nullable for Dynamic {
    type Value = Dynamic;

    fn into_defined(self) -> Option<Dynamic> {
        if self.is_nullish() { None } else { Some(self) }
    }
}

/// É uma `string` do JavaScript?
pub fn is_string(value: &Dynamic) -> bool {
    matches!(value, Dynamic::String(_))
}

/// É um array em que todo elemento é uma string?
pub fn is_string_array(value: &Dynamic) -> bool {
    is_array_of(value, is_string)
}

/// É um array em que todo elemento satisfaz `check`?
pub fn is_array_of(value: &Dynamic, check: impl Fn(&Dynamic) -> bool) -> bool {
    match value {
        Dynamic::Array(a) => a.to_vec().iter().all(check),
        _ => false,
    }
}

/// É do tipo `object` mas NÃO `null`, array, `RegExp` nem `Date` (inclui arrays tipados e instâncias
/// de classe).
pub fn is_object(obj: &Dynamic) -> bool {
    matches!(obj, Dynamic::Object(_) | Dynamic::TypedArray(_))
}

/// É um `Uint8Array` (ou derivado)?
pub fn is_typed_array(obj: &Dynamic) -> bool {
    matches!(obj, Dynamic::TypedArray(_))
}

/// Ao contrário de só olhar o `typeof`, devolve falso para `NaN`.
pub fn is_number(obj: &Dynamic) -> bool {
    matches!(obj, Dynamic::Number(n) if !n.is_nan())
}

/// `!!obj && typeof obj[Symbol.iterator] === 'function'`: strings não vazias, arrays e arrays tipados.
pub fn is_iterable(obj: &Dynamic) -> bool {
    match obj {
        Dynamic::String(s) => !s.is_empty(),
        Dynamic::Array(_) | Dynamic::TypedArray(_) => true,
        _ => false,
    }
}

pub fn is_boolean(obj: &Dynamic) -> bool {
    matches!(obj, Dynamic::Bool(_))
}

pub fn is_undefined(obj: &Dynamic) -> bool {
    obj.is_undefined()
}

/// Não é `undefined` nem `null`.
pub fn is_defined(arg: &Dynamic) -> bool {
    !is_undefined_or_null(arg)
}

pub fn is_undefined_or_null(obj: &Dynamic) -> bool {
    obj.is_nullish()
}

/// Lança `Unexpected type` (com o tipo esperado, se dado) quando a condição é falsa.
pub fn assert_type(condition: &impl Truthy, type_name: Option<&str>) {
    if !condition.is_truthy() {
        errors::throw(Error::new(match type_name {
            Some(t) if !t.is_empty() => format!("Unexpected type, expected '{t}'"),
            _ => "Unexpected type".to_string(),
        }));
    }
}

/// Garante que o argumento não é `undefined` nem `null` e o devolve.
/// A mensagem do `assertReturnsDefined`, para quem precisa lançar o mesmo erro sem passar pela função.
pub const UNDEFINED_ARGUMENT_MESSAGE: &str = "Argument is `undefined` or `null`.";

pub fn assert_returns_defined<N: Nullable>(arg: N) -> N::Value {
    let defined = arg.into_defined();
    assert(defined.is_some(), UNDEFINED_ARGUMENT_MESSAGE);
    defined.expect("conferido acima")
}

/// Garante que `value` está definido (nem `undefined` nem `null`) e o devolve; senão lança o erro
/// dado (uma mensagem vira `Error`, um erro é lançado como veio).
pub fn assert_defined<N: Nullable>(value: N, error: impl Into<ErrorOrMessage>) -> N::Value {
    match value.into_defined() {
        Some(defined) => defined,
        None => errors::throw(match error.into() {
            ErrorOrMessage::Message(message) => Error::new(message),
            ErrorOrMessage::Error(error) => error,
        }),
    }
}

/// Garante que nenhum argumento é `undefined` ou `null` e os devolve.
pub fn assert_returns_all_defined(args: &[Dynamic]) -> Vec<Dynamic> {
    args.iter()
        .enumerate()
        .map(|(i, arg)| {
            if arg.is_nullish() {
                errors::throw(Error::new(format!("Assertion Failed: argument at index {i} is undefined or null")));
            }
            arg.clone()
        })
        .collect()
}

/// O valor está na lista? A comparação é a de `Array.prototype.includes` (`SameValueZero`).
pub fn is_one_of<T: PartialEq>(value: &T, valid_values: &[T]) -> bool {
    valid_values.contains(value)
}

/// Verificação de tipo em tempo de compilação; não faz nada em tempo de execução.
pub fn type_check<T>(_thing: T) {}

/// É um objeto (no sentido de [`is_object`]) sem propriedades próprias?
pub fn is_empty_object(obj: &Dynamic) -> bool {
    is_object(obj) && obj.own_entries().is_empty()
}

pub fn is_function(obj: &Dynamic) -> bool {
    matches!(obj, Dynamic::Function(_))
}

/// Há pelo menos um argumento e todos são funções?
pub fn are_functions(objects: &[Dynamic]) -> bool {
    !objects.is_empty() && objects.iter().all(is_function)
}

/// `string | Function`: o nome de um `typeof`, ou uma função (construtor ou predicado).
#[derive(Clone)]
pub enum TypeConstraint {
    TypeOf(String),
    Function(FunctionRef),
}

impl From<&str> for TypeConstraint {
    fn from(name: &str) -> Self {
        Self::TypeOf(name.to_string())
    }
}

impl From<FunctionRef> for TypeConstraint {
    fn from(function: FunctionRef) -> Self {
        Self::Function(function)
    }
}

pub fn validate_constraints(args: &[Dynamic], constraints: &[Option<TypeConstraint>]) {
    for (arg, constraint) in args.iter().zip(constraints) {
        validate_constraint(arg, constraint.as_ref());
    }
}

/// Lança se `arg` não satisfaz `constraint`. Uma função satisfaz se `arg instanceof constraint`,
/// ou `arg.constructor === constraint`, ou ela tem um parâmetro e `constraint(arg) === true`.
/// (Um construtor nativo chamado como função só devolve `true` no caso de `Boolean`, ver
/// [`crate::dynamic::FunctionBody::Builtin`].)
pub fn validate_constraint(arg: &Dynamic, constraint: Option<&TypeConstraint>) {
    match constraint {
        Some(TypeConstraint::TypeOf(name)) => {
            if arg.type_of() != name {
                errors::throw(Error::new(format!("argument does not match constraint: typeof {name}")));
            }
        }
        Some(TypeConstraint::Function(function)) => {
            if arg.instance_of(function) {
                return;
            }
            if !arg.is_nullish() && arg.constructor().is_some_and(|c| c.same(function)) {
                return;
            }
            if function.length() == 1 && matches!(function.call(std::slice::from_ref(arg)), Dynamic::Bool(true)) {
                return;
            }
            errors::throw(Error::new(
                "argument does not match one of these constraints: arg instanceof constraint, arg.constructor === constraint, nor constraint(arg) === true",
            ));
        }
        None => {}
    }
}

/// `keys` são todos propriedades de `x` (o operador `in`)? Lança `TypeError` se `x` é primitivo.
pub fn has_key(x: &Dynamic, keys: &[&str]) -> bool {
    keys.iter().all(|k| x.has_property(&JsString::from(*k)))
}
