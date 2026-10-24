//! Valor dinâmico com a semântica de valor do JavaScript, para os módulos do `vs/base/common` que
//! trabalham com `any`/`unknown` em tempo de execução (`types.ts`, `objects.ts`, `hash.ts`).
//!
//! `json::Value` não serve a esses módulos por três motivos: não tem `undefined` nem `bigint`, não
//! tem identidade (`seen.has(obj)`, `===` entre objetos, referências compartilhadas e circulares) e
//! não tem funções, `RegExp`, `Date` nem arrays tipados, que `isObject`/`deepClone` distinguem.
//!
//! Escolhas de representação:
//! - Arrays, objetos, funções, `RegExp`, `Date` e arrays tipados são `Rc` com identidade: clonar o
//!   [`Dynamic`] copia a referência (como atribuir em JS) e [`Dynamic::strict_equals`] compara por
//!   ponteiro. Estruturas circulares são possíveis (e vazam, como todo ciclo de `Rc`; o uso é de
//!   teste e de dado de configuração, não de grafo de longa vida).
//! - Um objeto guarda as propriedades próprias enumeráveis com a ordem do JavaScript: chaves que
//!   são índices de array primeiro, em ordem numérica, o resto na ordem de inserção (a mesma regra
//!   de `json::Object`).
//! - NÃO há cadeia de protótipos. `key in obj`, `obj[key]` e `for (const k in obj)` olham só as
//!   propriedades próprias (no JS olhariam também `Object.prototype`, onde só `constructor`,
//!   `toString` etc. diferem). Uma classe é uma [`FunctionRef`] de corpo [`FunctionBody::Class`]
//!   guardada no objeto, o bastante para `instanceof` e `.constructor`.
//! - `Object.freeze` marca o objeto/array como congelado; escrever num congelado lança `TypeError`
//!   (os módulos do VS Code são ESM, logo em modo estrito).
//! - `RegExp` guarda só `source` e `flags`; compilar é com a crate `zyarr`, fora do `je-base`.
//! - Números são `f64`; `bigint` é `i128` (basta para os usos do VS Code).
//! - Símbolos, `Map`, `Set`, `WeakMap` e iteradores customizados não estão modelados.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use crate::errors::{self, Error};
use crate::js_string::JsString;

/// "Verdadeiro" no sentido do JavaScript (`!!value`).
pub trait Truthy {
    fn is_truthy(&self) -> bool;
}

impl<T: Truthy + ?Sized> Truthy for &T {
    fn is_truthy(&self) -> bool {
        (**self).is_truthy()
    }
}

impl<T: Truthy> Truthy for Option<T> {
    /// `None` é `undefined`/`null`.
    fn is_truthy(&self) -> bool {
        self.as_ref().is_some_and(Truthy::is_truthy)
    }
}

impl Truthy for bool {
    fn is_truthy(&self) -> bool {
        *self
    }
}

impl Truthy for f64 {
    fn is_truthy(&self) -> bool {
        *self != 0.0 && !self.is_nan()
    }
}

macro_rules! truthy_integer {
    ($($t:ty),*) => {$(impl Truthy for $t { fn is_truthy(&self) -> bool { *self != 0 } })*};
}
truthy_integer!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);

impl Truthy for str {
    fn is_truthy(&self) -> bool {
        !self.is_empty()
    }
}

impl Truthy for String {
    fn is_truthy(&self) -> bool {
        !self.is_empty()
    }
}

impl Truthy for JsString {
    fn is_truthy(&self) -> bool {
        !self.is_empty()
    }
}

/// Um valor JavaScript. Ver o topo do módulo para as escolhas de representação.
#[derive(Clone)]
pub enum Dynamic {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    BigInt(i128),
    String(JsString),
    Array(ArrayRef),
    Object(ObjectRef),
    Function(FunctionRef),
    RegExp(RegExpRef),
    Date(DateRef),
    TypedArray(TypedArrayRef),
}

struct ArrayData {
    items: RefCell<Vec<Dynamic>>,
    frozen: Cell<bool>,
}

/// Referência a um `Array`.
#[derive(Clone)]
pub struct ArrayRef(Rc<ArrayData>);

struct ObjectData {
    class: Option<FunctionRef>,
    entries: RefCell<Vec<(JsString, Dynamic)>>,
    frozen: Cell<bool>,
}

/// Referência a um objeto simples ou instância de classe.
#[derive(Clone)]
pub struct ObjectRef(Rc<ObjectData>);

/// Os construtores nativos que o porte distingue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    Object,
    Array,
    Function,
    Number,
    String,
    Boolean,
    RegExp,
    Date,
    Uint8Array,
}

/// O corpo de uma função comum.
pub type NativeFn = Rc<dyn Fn(&[Dynamic]) -> Dynamic>;

/// Como uma função se comporta.
#[derive(Clone)]
pub enum FunctionBody {
    /// Função comum; recebe os argumentos e devolve o resultado.
    Native(NativeFn),
    /// Construtor nativo (`Number`, `String`, `Array`...). Chamado como função, devolve
    /// `Boolean(!!arg)` no caso de `Boolean` e `undefined` nos demais: o resultado nunca é `true`,
    /// que é a única coisa que os chamadores (`validateConstraint`) comparam.
    Builtin(Builtin),
    /// Classe definida pelo usuário (`class Foo {}`).
    Class,
}

struct FunctionData {
    name: String,
    length: usize,
    body: FunctionBody,
}

/// Referência a uma função (inclusive construtor e classe).
#[derive(Clone)]
pub struct FunctionRef(Rc<FunctionData>);

struct RegExpData {
    source: JsString,
    flags: JsString,
}

/// Referência a um `RegExp` (só `source` e `flags`).
#[derive(Clone)]
pub struct RegExpRef(Rc<RegExpData>);

/// Referência a um `Date` (o valor de tempo em milissegundos desde a época, `NaN` se inválido).
#[derive(Clone)]
pub struct DateRef(Rc<f64>);

/// Referência a um `Uint8Array`.
#[derive(Clone)]
pub struct TypedArrayRef(Rc<RefCell<Vec<u8>>>);

/// `key` é um índice de array canônico (inteiro abaixo de 2^32 - 1)?
fn array_index(key: &JsString) -> Option<u32> {
    let units = key.units();
    if units.is_empty() || units.len() > 10 || (units.len() > 1 && units[0] == u16::from(b'0')) {
        return None;
    }
    let mut n: u64 = 0;
    for &u in units {
        if !(u16::from(b'0')..=u16::from(b'9')).contains(&u) {
            return None;
        }
        n = n * 10 + u64::from(u - u16::from(b'0'));
    }
    u32::try_from(n).ok().filter(|&n| n != u32::MAX)
}

fn type_error(message: String) -> ! {
    errors::throw(Error::type_error(message))
}

impl ArrayRef {
    pub fn new(items: Vec<Dynamic>) -> Self {
        Self(Rc::new(ArrayData { items: RefCell::new(items), frozen: Cell::new(false) }))
    }

    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `array[index]`: `undefined` fora dos limites.
    pub fn get(&self, index: usize) -> Dynamic {
        self.0.items.borrow().get(index).cloned().unwrap_or(Dynamic::Undefined)
    }

    /// Cópia dos elementos.
    pub fn to_vec(&self) -> Vec<Dynamic> {
        self.0.items.borrow().clone()
    }

    pub fn push(&self, value: Dynamic) {
        self.check_writable("push");
        self.0.items.borrow_mut().push(value);
    }

    /// `array[index] = value`, preenchendo com `undefined` o que faltar até o índice.
    pub fn set(&self, index: usize, value: Dynamic) {
        self.check_writable(&index.to_string());
        let mut items = self.0.items.borrow_mut();
        if index >= items.len() {
            items.resize(index + 1, Dynamic::Undefined);
        }
        items[index] = value;
    }

    fn check_writable(&self, what: &str) {
        if self.0.frozen.get() {
            type_error(format!("Cannot add property {what}, object is not extensible"));
        }
    }

    pub fn freeze(&self) {
        self.0.frozen.set(true);
    }

    pub fn is_frozen(&self) -> bool {
        self.0.frozen.get()
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Default for ObjectRef {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjectRef {
    /// `{}`.
    pub fn new() -> Self {
        Self(Rc::new(ObjectData { class: None, entries: RefCell::new(Vec::new()), frozen: Cell::new(false) }))
    }

    /// `new Class()`: um objeto vazio cuja classe é `class`.
    pub fn with_class(class: &FunctionRef) -> Self {
        Self(Rc::new(ObjectData { class: Some(class.clone()), entries: RefCell::new(Vec::new()), frozen: Cell::new(false) }))
    }

    /// Objeto com as propriedades dadas, na ordem do JavaScript.
    pub fn from_entries<K: Into<JsString>>(entries: impl IntoIterator<Item = (K, Dynamic)>) -> Self {
        let object = Self::new();
        for (key, value) in entries {
            object.set(&key.into(), value);
        }
        object
    }

    pub fn class(&self) -> Option<&FunctionRef> {
        self.0.class.as_ref()
    }

    /// `object[key]` próprio, `None` se não existe.
    pub fn get(&self, key: &JsString) -> Option<Dynamic> {
        self.0.entries.borrow().iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    pub fn has_own(&self, key: &JsString) -> bool {
        self.0.entries.borrow().iter().any(|(k, _)| k == key)
    }

    /// `object[key] = value`. Num objeto congelado lança `TypeError`.
    pub fn set(&self, key: &JsString, value: Dynamic) {
        let mut entries = self.0.entries.borrow_mut();
        if self.0.frozen.get() {
            if entries.iter().any(|(k, _)| k == key) {
                type_error(format!("Cannot assign to read only property '{key}' of object '#<Object>'"));
            }
            type_error(format!("Cannot add property {key}, object is not extensible"));
        }
        if let Some(slot) = entries.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value;
            return;
        }
        let position = match array_index(key) {
            Some(index) => entries
                .iter()
                .position(|(k, _)| array_index(k).is_none_or(|other| other > index))
                .unwrap_or(entries.len()),
            None => entries.len(),
        };
        entries.insert(position, (key.clone(), value));
    }

    /// `delete object[key]`.
    pub fn delete(&self, key: &JsString) -> bool {
        if self.0.frozen.get() {
            type_error(format!("Cannot delete property '{key}' of #<Object>"));
        }
        let mut entries = self.0.entries.borrow_mut();
        let before = entries.len();
        entries.retain(|(k, _)| k != key);
        entries.len() != before
    }

    /// `Object.entries(object)`.
    pub fn entries(&self) -> Vec<(JsString, Dynamic)> {
        self.0.entries.borrow().clone()
    }

    /// `Object.keys(object)`.
    pub fn keys(&self) -> Vec<JsString> {
        self.0.entries.borrow().iter().map(|(k, _)| k.clone()).collect()
    }

    pub fn freeze(&self) {
        self.0.frozen.set(true);
    }

    pub fn is_frozen(&self) -> bool {
        self.0.frozen.get()
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl FunctionRef {
    /// Função comum com `name` e `length` (o número de parâmetros declarados).
    pub fn native(name: &str, length: usize, body: impl Fn(&[Dynamic]) -> Dynamic + 'static) -> Self {
        Self(Rc::new(FunctionData { name: name.to_string(), length, body: FunctionBody::Native(Rc::new(body)) }))
    }

    /// Um construtor nativo.
    pub fn builtin(kind: Builtin) -> Self {
        let (name, length) = match kind {
            Builtin::Object => ("Object", 1),
            Builtin::Array => ("Array", 1),
            Builtin::Function => ("Function", 1),
            Builtin::Number => ("Number", 1),
            Builtin::String => ("String", 1),
            Builtin::Boolean => ("Boolean", 1),
            Builtin::RegExp => ("RegExp", 2),
            Builtin::Date => ("Date", 7),
            Builtin::Uint8Array => ("Uint8Array", 3),
        };
        Self(Rc::new(FunctionData { name: name.to_string(), length, body: FunctionBody::Builtin(kind) }))
    }

    /// `class Name {}`.
    pub fn class(name: &str) -> Self {
        Self(Rc::new(FunctionData { name: name.to_string(), length: 0, body: FunctionBody::Class }))
    }

    pub fn name(&self) -> &str {
        &self.0.name
    }

    /// `fn.length`.
    pub fn length(&self) -> usize {
        self.0.length
    }

    pub fn body(&self) -> &FunctionBody {
        &self.0.body
    }

    /// `fn.call(undefined, ...args)`. Uma classe não pode ser chamada sem `new`.
    pub fn call(&self, args: &[Dynamic]) -> Dynamic {
        match &self.0.body {
            FunctionBody::Native(f) => f(args),
            FunctionBody::Builtin(Builtin::Boolean) => Dynamic::Bool(args.first().is_some_and(Truthy::is_truthy)),
            FunctionBody::Builtin(_) => Dynamic::Undefined,
            FunctionBody::Class => type_error(format!("Class constructor {} cannot be invoked without 'new'", self.0.name)),
        }
    }

    /// `new Class()` de uma classe do usuário.
    pub fn instantiate(&self) -> Dynamic {
        Dynamic::Object(ObjectRef::with_class(self))
    }

    /// Mesma função: mesma referência, ou o mesmo construtor nativo.
    pub fn same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
            || matches!((&self.0.body, &other.0.body), (FunctionBody::Builtin(a), FunctionBody::Builtin(b)) if a == b)
    }
}

impl RegExpRef {
    pub fn new(source: &str, flags: &str) -> Self {
        Self(Rc::new(RegExpData { source: source.into(), flags: flags.into() }))
    }

    pub fn source(&self) -> &JsString {
        &self.0.source
    }

    pub fn flags(&self) -> &JsString {
        &self.0.flags
    }
}

impl DateRef {
    /// `new Date(ms)`.
    pub fn new(time_value: f64) -> Self {
        Self(Rc::new(time_value))
    }

    pub fn time_value(&self) -> f64 {
        *self.0
    }
}

impl TypedArrayRef {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(Rc::new(RefCell::new(bytes)))
    }

    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.0.borrow().clone()
    }
}

impl Dynamic {
    pub fn string(text: &str) -> Self {
        Self::String(text.into())
    }

    /// `[a, b, c]`.
    pub fn array(items: impl IntoIterator<Item = Dynamic>) -> Self {
        Self::Array(ArrayRef::new(items.into_iter().collect()))
    }

    /// `{ k: v, ... }`.
    pub fn object<K: Into<JsString>>(entries: impl IntoIterator<Item = (K, Dynamic)>) -> Self {
        Self::Object(ObjectRef::from_entries(entries))
    }

    pub fn regexp(source: &str, flags: &str) -> Self {
        Self::RegExp(RegExpRef::new(source, flags))
    }

    pub fn date(time_value: f64) -> Self {
        Self::Date(DateRef::new(time_value))
    }

    pub fn typed_array(bytes: Vec<u8>) -> Self {
        Self::TypedArray(TypedArrayRef::new(bytes))
    }

    /// O resultado do operador `typeof`.
    pub fn type_of(&self) -> &'static str {
        match self {
            Self::Undefined => "undefined",
            Self::Bool(_) => "boolean",
            Self::Number(_) => "number",
            Self::BigInt(_) => "bigint",
            Self::String(_) => "string",
            Self::Function(_) => "function",
            Self::Null | Self::Array(_) | Self::Object(_) | Self::RegExp(_) | Self::Date(_) | Self::TypedArray(_) => "object",
        }
    }

    pub fn is_undefined(&self) -> bool {
        matches!(self, Self::Undefined)
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// `undefined` ou `null`.
    pub fn is_nullish(&self) -> bool {
        matches!(self, Self::Undefined | Self::Null)
    }

    /// O endereço do objeto referenciado, `None` para primitivos.
    pub fn identity(&self) -> Option<usize> {
        match self {
            Self::Array(a) => Some(Rc::as_ptr(&a.0) as *const u8 as usize),
            Self::Object(o) => Some(Rc::as_ptr(&o.0) as *const u8 as usize),
            Self::Function(f) => Some(Rc::as_ptr(&f.0) as *const u8 as usize),
            Self::RegExp(r) => Some(Rc::as_ptr(&r.0) as *const u8 as usize),
            Self::Date(d) => Some(Rc::as_ptr(&d.0) as *const u8 as usize),
            Self::TypedArray(t) => Some(Rc::as_ptr(&t.0) as *const u8 as usize),
            _ => None,
        }
    }

    /// `a === b`: `NaN` não é igual a `NaN`; objetos por identidade.
    pub fn strict_equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Undefined, Self::Undefined) | (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::BigInt(a), Self::BigInt(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Function(a), Self::Function(b)) => a.same(b),
            _ => match (self.identity(), other.identity()) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            },
        }
    }

    /// `Array.isArray`.
    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array(_))
    }

    /// `Object.entries(value)` para o que tem propriedades próprias enumeráveis: objetos, arrays
    /// (índices) e arrays tipados (índices). Vazio para o resto (`Date`, `RegExp`, funções,
    /// primitivos; strings não entram porque nenhum chamador do porte as passa).
    pub fn own_entries(&self) -> Vec<(JsString, Dynamic)> {
        match self {
            Self::Object(o) => o.entries(),
            Self::Array(a) => a.to_vec().into_iter().enumerate().map(|(i, v)| (JsString::from(i.to_string()), v)).collect(),
            Self::TypedArray(t) => t
                .to_vec()
                .into_iter()
                .enumerate()
                .map(|(i, b)| (JsString::from(i.to_string()), Self::Number(f64::from(b))))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `Object.keys(value)`.
    pub fn own_keys(&self) -> Vec<JsString> {
        self.own_entries().into_iter().map(|(k, _)| k).collect()
    }

    /// `value[key]` (leitura): `undefined` se não existe. Arrays também respondem a `length`.
    /// Em `undefined`/`null` lança `TypeError`, como o JavaScript.
    pub fn get(&self, key: &JsString) -> Dynamic {
        match self {
            Self::Undefined | Self::Null => type_error(format!("Cannot read properties of {} (reading '{key}')", if self.is_null() { "null" } else { "undefined" })),
            Self::Object(o) => o.get(key).unwrap_or(Self::Undefined),
            Self::Array(a) => {
                if *key == "length" {
                    Self::Number(a.len() as f64)
                } else {
                    array_index(key).map_or(Self::Undefined, |i| a.get(i as usize))
                }
            }
            Self::TypedArray(t) => {
                if *key == "length" {
                    Self::Number(t.len() as f64)
                } else {
                    array_index(key).and_then(|i| t.0.borrow().get(i as usize).copied()).map_or(Self::Undefined, |b| Self::Number(f64::from(b)))
                }
            }
            Self::String(s) => {
                if *key == "length" {
                    Self::Number(s.len() as f64)
                } else {
                    array_index(key).and_then(|i| s.units().get(i as usize).copied()).map_or(Self::Undefined, |u| Self::String(JsString::from_units(vec![u])))
                }
            }
            _ => Self::Undefined,
        }
    }

    /// `value[key]` com a chave em `&str`.
    pub fn get_str(&self, key: &str) -> Dynamic {
        self.get(&JsString::from(key))
    }

    /// `key in value` (só propriedades próprias, ver o topo do módulo). Em primitivos lança
    /// `TypeError`, como o operador `in`.
    pub fn has_property(&self, key: &JsString) -> bool {
        match self {
            Self::Object(o) => o.has_own(key),
            Self::Array(a) => *key == "length" || array_index(key).is_some_and(|i| (i as usize) < a.len()),
            Self::TypedArray(t) => *key == "length" || array_index(key).is_some_and(|i| (i as usize) < t.len()),
            Self::Function(_) | Self::RegExp(_) | Self::Date(_) => false,
            _ => type_error(format!("Cannot use 'in' operator to search for '{key}' in {}", self.type_of())),
        }
    }

    /// `value.constructor`.
    pub fn constructor(&self) -> Option<FunctionRef> {
        let builtin = match self {
            Self::Undefined | Self::Null | Self::BigInt(_) => return None,
            Self::Bool(_) => Builtin::Boolean,
            Self::Number(_) => Builtin::Number,
            Self::String(_) => Builtin::String,
            Self::Array(_) => Builtin::Array,
            Self::Function(_) => Builtin::Function,
            Self::RegExp(_) => Builtin::RegExp,
            Self::Date(_) => Builtin::Date,
            Self::TypedArray(_) => Builtin::Uint8Array,
            Self::Object(o) => match o.class() {
                Some(class) => return Some(class.clone()),
                None => Builtin::Object,
            },
        };
        Some(FunctionRef::builtin(builtin))
    }

    /// `value instanceof ctor`. Primitivos nunca são instância.
    pub fn instance_of(&self, ctor: &FunctionRef) -> bool {
        match ctor.body() {
            FunctionBody::Class => matches!(self, Self::Object(o) if o.class().is_some_and(|c| c.same(ctor))),
            FunctionBody::Builtin(kind) => match kind {
                Builtin::Object => self.identity().is_some(),
                Builtin::Array => self.is_array(),
                Builtin::Function => matches!(self, Self::Function(_)),
                Builtin::RegExp => matches!(self, Self::RegExp(_)),
                Builtin::Date => matches!(self, Self::Date(_)),
                Builtin::Uint8Array => matches!(self, Self::TypedArray(_)),
                Builtin::Number | Builtin::String | Builtin::Boolean => false,
            },
            FunctionBody::Native(_) => false,
        }
    }
}

/// Igualdade `SameValueZero` (a de `Array.prototype.includes`, `Set` e `Map`): como `===`, mas
/// `NaN` é igual a `NaN`.
impl PartialEq for Dynamic {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) if a.is_nan() && b.is_nan() => true,
            _ => self.strict_equals(other),
        }
    }
}

impl Truthy for Dynamic {
    fn is_truthy(&self) -> bool {
        match self {
            Self::Undefined | Self::Null => false,
            Self::Bool(b) => *b,
            Self::Number(n) => n.is_truthy(),
            Self::BigInt(n) => *n != 0,
            Self::String(s) => !s.is_empty(),
            _ => true,
        }
    }
}

impl From<bool> for Dynamic {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

impl From<f64> for Dynamic {
    fn from(n: f64) -> Self {
        Self::Number(n)
    }
}

impl From<i32> for Dynamic {
    fn from(n: i32) -> Self {
        Self::Number(f64::from(n))
    }
}

impl From<&str> for Dynamic {
    fn from(s: &str) -> Self {
        Self::string(s)
    }
}

impl From<String> for Dynamic {
    fn from(s: String) -> Self {
        Self::String(s.into())
    }
}

impl From<JsString> for Dynamic {
    fn from(s: JsString) -> Self {
        Self::String(s)
    }
}

impl From<ArrayRef> for Dynamic {
    fn from(a: ArrayRef) -> Self {
        Self::Array(a)
    }
}

impl From<ObjectRef> for Dynamic {
    fn from(o: ObjectRef) -> Self {
        Self::Object(o)
    }
}

impl From<FunctionRef> for Dynamic {
    fn from(f: FunctionRef) -> Self {
        Self::Function(f)
    }
}

const DEBUG_MAX_DEPTH: usize = 6;

fn debug_value(value: &Dynamic, depth: usize, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if depth > DEBUG_MAX_DEPTH {
        return f.write_str("...");
    }
    match value {
        Dynamic::Undefined => f.write_str("undefined"),
        Dynamic::Null => f.write_str("null"),
        Dynamic::Bool(b) => write!(f, "{b}"),
        Dynamic::Number(n) => f.write_str(&number_to_js_string(*n)),
        Dynamic::BigInt(n) => write!(f, "{n}n"),
        Dynamic::String(s) => write!(f, "{s:?}"),
        Dynamic::Function(func) => write!(f, "[Function {}]", func.name()),
        Dynamic::RegExp(r) => write!(f, "/{}/{}", r.source(), r.flags()),
        Dynamic::Date(d) => write!(f, "Date({})", number_to_js_string(d.time_value())),
        Dynamic::TypedArray(t) => write!(f, "Uint8Array{:?}", t.to_vec()),
        Dynamic::Array(a) => {
            f.write_str("[")?;
            for (i, item) in a.to_vec().iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                debug_value(item, depth + 1, f)?;
            }
            f.write_str("]")
        }
        Dynamic::Object(o) => {
            if let Some(class) = o.class() {
                write!(f, "{} ", class.name())?;
            }
            f.write_str("{")?;
            for (i, (key, item)) in o.entries().iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{key}: ")?;
                debug_value(item, depth + 1, f)?;
            }
            f.write_str("}")
        }
    }
}

impl fmt::Debug for Dynamic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_value(self, 0, f)
    }
}

/// `String(number)`: a conversão de número para texto do JavaScript (`Number::toString`), com
/// os dígitos mais curtos que fazem a viagem de volta e a notação exponencial fora de `1e-6..1e21`.
pub fn number_to_js_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-Infinity".into() } else { "Infinity".into() };
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let scientific = format!("{:e}", x.abs());
    let (mantissa, exponent) = scientific.split_once('e').expect("formato científico do Rust");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let n: i32 = exponent.parse::<i32>().expect("expoente inteiro") + 1;
    let k = digits.len() as i32;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let exp = format!("e{}{}", if e < 0 { '-' } else { '+' }, e.abs());
        if k == 1 { format!("{digits}{exp}") } else { format!("{}.{}{exp}", &digits[..1], &digits[1..]) }
    };
    format!("{sign}{body}")
}

/// `JSON.stringify(value)`: `None` onde o JavaScript devolve `undefined` (valor `undefined` ou função).
pub fn json_stringify(value: &Dynamic) -> Option<String> {
    json_stringify_with_gap(value, "", "")
}

/// `JSON.stringify(value, null, indent)` com `indent` espaços (no máximo 10, como o JavaScript).
pub fn json_stringify_indented(value: &Dynamic, indent: usize) -> Option<String> {
    json_stringify_with_gap(value, &" ".repeat(indent.min(10)), "")
}

/// O `SerializeJSONProperty` da especificação: `gap` é o recuo de um nível (vazio para o formato
/// compacto) e `indent` o recuo acumulado do nível atual.
fn json_stringify_with_gap(value: &Dynamic, gap: &str, indent: &str) -> Option<String> {
    let inner_indent = format!("{indent}{gap}");
    // `[a,b]` no formato compacto; um item por linha, recuado, com `gap`.
    let wrap = |open: &str, close: &str, parts: Vec<String>| {
        if parts.is_empty() {
            format!("{open}{close}")
        } else if gap.is_empty() {
            format!("{open}{}{close}", parts.join(","))
        } else {
            format!("{open}\n{inner_indent}{}\n{indent}{close}", parts.join(&format!(",\n{inner_indent}")))
        }
    };
    match value {
        Dynamic::Undefined | Dynamic::Function(_) => None,
        Dynamic::Null => Some("null".to_string()),
        Dynamic::Bool(b) => Some(b.to_string()),
        Dynamic::Number(n) => Some(if n.is_finite() { number_to_js_string(*n) } else { "null".to_string() }),
        Dynamic::BigInt(_) => errors::throw(Error::type_error("Do not know how to serialize a BigInt".to_string())),
        Dynamic::String(text) => Some(json_quote(text)),
        Dynamic::Array(items) => {
            let parts = items.to_vec().iter().map(|item| json_stringify_with_gap(item, gap, &inner_indent).unwrap_or_else(|| "null".to_string())).collect();
            Some(wrap("[", "]", parts))
        }
        Dynamic::Date(date) => Some(date_to_iso_string(date.time_value()).map_or_else(|| "null".to_string(), |iso| json_quote(&JsString::from(iso)))),
        Dynamic::RegExp(_) => Some("{}".to_string()),
        Dynamic::Object(_) | Dynamic::TypedArray(_) => {
            let separator = if gap.is_empty() { ":" } else { ": " };
            let parts = value
                .own_entries()
                .iter()
                .filter_map(|(key, item)| json_stringify_with_gap(item, gap, &inner_indent).map(|item| format!("{}{separator}{item}", json_quote(key))))
                .collect();
            Some(wrap("{", "}", parts))
        }
    }
}

/// `JSON.stringify(string)`: aspas, escapes curtos, `\u00xx` para controle e `\udxxx` para
/// surrogate isolado (o `JSON.stringify` bem-formado do ES2019).
pub fn json_quote(text: &JsString) -> String {
    let units = text.units();
    let mut out = String::with_capacity(units.len() + 2);
    out.push('"');
    let mut i = 0;
    while i < units.len() {
        let u = units[i];
        match u {
            0x22 => out.push_str("\\\""),
            0x5C => out.push_str("\\\\"),
            0x08 => out.push_str("\\b"),
            0x0C => out.push_str("\\f"),
            0x0A => out.push_str("\\n"),
            0x0D => out.push_str("\\r"),
            0x09 => out.push_str("\\t"),
            0..=0x1F => out.push_str(&format!("\\u{u:04x}")),
            0xD800..=0xDBFF => match units.get(i + 1) {
                Some(&low) if (0xDC00..=0xDFFF).contains(&low) => {
                    let cp = 0x10000 + ((u32::from(u) - 0xD800) << 10) + (u32::from(low) - 0xDC00);
                    out.push(char::from_u32(cp).expect("par de surrogates válido"));
                    i += 1;
                }
                _ => out.push_str(&format!("\\u{u:04x}")),
            },
            0xDC00..=0xDFFF => out.push_str(&format!("\\u{u:04x}")),
            _ => out.push(char::from_u32(u32::from(u)).expect("unidade fora dos surrogates")),
        }
        i += 1;
    }
    out.push('"');
    out
}

/// `new Date(ms).toISOString()`; `None` quando o valor de tempo é inválido (`NaN` ou fora de
/// ±8,64e15), caso em que `toJSON` devolve `null`.
pub fn date_to_iso_string(time_value: f64) -> Option<String> {
    if !time_value.is_finite() || time_value.abs() > 8.64e15 {
        return None;
    }
    let ms_total = time_value.trunc() as i64;
    let days = ms_total.div_euclid(86_400_000);
    let ms_of_day = ms_total.rem_euclid(86_400_000);
    // Algoritmo `civil_from_days` de Howard Hinnant.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    let year_text = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{}{:06}", if year < 0 { '-' } else { '+' }, year.abs())
    };
    Some(format!(
        "{year_text}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        ms_of_day / 3_600_000,
        ms_of_day / 60_000 % 60,
        ms_of_day / 1000 % 60,
        ms_of_day % 1000
    ))
}

impl Dynamic {
    /// O valor que `JSON.parse` devolveria para `value`: objetos e arrays novos, na mesma ordem de chaves.
    pub fn from_json(value: &crate::json::Value) -> Self {
        use crate::json::Value;
        match value {
            Value::Null => Self::Null,
            Value::Bool(b) => Self::Bool(*b),
            Value::Number(n) => Self::Number(*n),
            Value::String(s) => Self::String(s.clone()),
            Value::Array(items) => Self::array(items.iter().map(Self::from_json)),
            Value::Object(object) => Self::object(object.iter().map(|(k, v)| (k.clone(), Self::from_json(v)))),
        }
    }
}
