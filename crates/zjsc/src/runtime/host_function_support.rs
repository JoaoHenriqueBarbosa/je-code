//! O que as funções nativas (`JSC_DEFINE_HOST_FUNCTION`) usam e que o C++ tem como membro de `JSValue`
//! ou como função livre do `Error.h`: `isObject`, `isCallable`, `toObject`, `toPropertyKey`,
//! `throwVMTypeError`, `JSObject::defaultHasInstance` e o `asObject` que também alcança `JSFunction`.
//!
//! LACUNA, e por quê (um `panic!` com a mensagem, nunca um valor inventado):
//! - `toObject` de `BigInt` precisa de `BigIntObject` e de `BigInt.prototype`, que o porte ainda não
//!   tem. O resto (`undefined`/`null` lançam o `TypeError` do C++; número, booleano, string e símbolo
//!   embrulham) está feito.
//! - `toPropertyKey` de objeto usa `JSValue::to_primitive_preferred` (`@@toPrimitive`, `toString`,
//!   `valueOf`), em `object_to_primitive`.
//! - `JSFunction` sobrescreve `getOwnPropertySlot` (`name`, `length` e `prototype` materializam
//!   preguiçosamente pelos `reifyLazy*`, em `js_function_reify.rs`): o [`ObjectRef`] despacha para a de
//!   função (`get_own_property_slot`, `get_property_slot`, `get`, `has_own_property`,
//!   `for_property_lookup`). `getPrototype` não é sobrescrito e funciona.
//!
//! DIVERGÊNCIA: `JSObject::from_value` não devolve `JSFunction` (a entrada do registro ainda não expõe a
//! base `JSObject` dela, porque a função sobrescreve `getOwnPropertySlot`); o [`ObjectRef`] cobre as duas.

use std::ops::Deref;

use crate::runtime::boolean_object::construct_boolean_from_immediate_boolean;
use crate::runtime::cell_registry;
use crate::runtime::exception_helpers::{
    create_not_an_object_error, throw_out_of_memory_error, throw_stack_overflow_error,
};
use crate::runtime::error::create_type_error;
use crate::runtime::host_call::{throw_thrown, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_function::JSFunctionRef;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSObject, JSObjectHandle, PutError};
use crate::runtime::js_type::is_object_type;
use crate::runtime::js_typeof::is_callable_cell;
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::number_object::construct_number;
use crate::runtime::object_to_primitive::PreferredPrimitiveType;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::string_object::StringObject;
use crate::runtime::symbol::Symbol;
use crate::runtime::throw_scope::{throw_exception, throw_vm_exception, ThrowScope};
use crate::wtf::text::wtf_string::String as WtfString;

/// `JSObject*` de qualquer célula que seja objeto, inclusive `JSFunction`.
#[derive(Clone, Debug)]
pub enum ObjectRef {
    /// `JSObject::from_value`: objeto comum e as subclasses que o registro expõe.
    Handle(JSObjectHandle),
    /// `jsDynamicCast<JSFunction*>`.
    Function(JSFunctionRef),
}

impl ObjectRef {
    /// `asObject(value)`: `None` se o valor não é uma célula-objeto que este porte alcança.
    pub fn from_value(value: &JSValue) -> Option<ObjectRef> {
        // A função primeiro: `JSObject::from_value` também aceita a célula de uma `JSFunction`
        // (`as_js_object` faz o `Deref` até a base), e o `Handle` pularia a materialização preguiçosa
        // de `length`/`name`/`prototype` do `getOwnPropertySlot`/`defineOwnProperty` da função.
        if let Some(function) = value.as_js_function() {
            return Some(ObjectRef::Function(function));
        }
        JSObject::from_value(value).map(ObjectRef::Handle)
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        match self {
            ObjectRef::Handle(object) => object.as_value(),
            ObjectRef::Function(function) => function.as_value(),
        }
    }

    /// `getOwnPropertySlot(globalObject, propertyName, slot)`: a de `JSFunction` materializa
    /// `name`/`length`/`prototype`. A exceção fica pendente no `VM` (e o resultado é `false`).
    pub fn get_own_property_slot(&self, global_object: &JSGlobalObject, property_name: &PropertyName, slot: &mut PropertySlot) -> bool {
        match self {
            ObjectRef::Handle(object) => object.get_own_property_slot(global_object.vm(), property_name, slot),
            ObjectRef::Function(function) => function.get_own_property_slot(global_object, property_name, slot),
        }
    }

    /// `getPropertySlot(globalObject, propertyName, slot)`.
    pub fn get_property_slot(&self, global_object: &JSGlobalObject, property_name: &PropertyName, slot: &mut PropertySlot) -> bool {
        match self {
            ObjectRef::Handle(object) => object.get_property_slot(global_object.vm(), property_name, slot),
            ObjectRef::Function(function) => function.get_property_slot(global_object, property_name, slot),
        }
    }

    /// `get(globalObject, propertyName)`: `undefined` se a propriedade não existe (com a exceção
    /// pendente no `VM`, `JSValue()` vazio não é devolvido: quem chama confere `vm.exception()`).
    pub fn get(&self, global_object: &JSGlobalObject, property_name: &PropertyName) -> JSValue {
        let mut slot = PropertySlot::new(self.as_value(), InternalMethodType::Get);
        if self.get_property_slot(global_object, property_name, &mut slot) {
            return slot.get_value_for(property_name);
        }
        JSValue::undefined()
    }

    /// `hasOwnProperty(globalObject, propertyName)`.
    pub fn has_own_property(&self, global_object: &JSGlobalObject, property_name: &PropertyName) -> bool {
        let mut slot = PropertySlot::new(self.as_value(), InternalMethodType::GetOwnProperty);
        self.get_own_property_slot(global_object, property_name, &mut slot)
    }

    /// `defineOwnProperty(globalObject, propertyName, descriptor, throwException)`: a de `JSFunction`
    /// tem o caminho próprio de `prototype` e materializa `name`/`length` antes.
    pub fn define_own_property(
        &self,
        global_object: &JSGlobalObject,
        property_name: &PropertyName,
        descriptor: &PropertyDescriptor,
        throw_exception: bool,
    ) -> Result<bool, PutError> {
        match self {
            ObjectRef::Handle(object) => object.define_own_property(global_object.vm(), property_name, descriptor, throw_exception),
            ObjectRef::Function(function) => function.define_own_property(global_object, property_name, descriptor, throw_exception),
        }
    }

    /// O objeto para as operações de `JSObject` que consultam a propriedade própria por dentro
    /// (`getOwnPropertyDescriptor`): função materializa a propriedade preguiçosa
    /// `property_name` antes (`reifyLazyPropertyIfNeeded`). `None` com a exceção pendente.
    pub fn for_property_lookup(&self, global_object: &JSGlobalObject, property_name: &PropertyName) -> Option<&JSObject> {
        match self {
            ObjectRef::Handle(object) => Some(object),
            ObjectRef::Function(function) => {
                function.reify_lazy_property_if_needed(global_object, property_name, false);
                if global_object.vm().exception().is_some() {
                    return None;
                }
                Some(function)
            }
        }
    }
}

impl Deref for ObjectRef {
    type Target = JSObject;

    /// A base `JSObject`, para o que `JSFunction` não sobrescreve (`getPrototype`).
    fn deref(&self) -> &JSObject {
        match self {
            ObjectRef::Handle(object) => object,
            ObjectRef::Function(function) => function,
        }
    }
}

impl JSValue {
    /// `isObject()`.
    pub fn is_object(&self) -> bool {
        match self {
            JSValue::Cell(cell_id) => cell_registry::cell_type(*cell_id).is_some_and(is_object_type),
            _ => false,
        }
    }

    /// `isCallable()`.
    pub fn is_callable(&self) -> bool {
        match self {
            JSValue::Cell(cell_id) => cell_registry::get(*cell_id).is_some_and(|cell| is_callable_cell(&cell)),
            _ => false,
        }
    }

    /// `asObject(value)`: invariante de `isObject`, como o `ASSERT` do C++.
    pub fn as_object(&self) -> ObjectRef {
        debug_assert!(self.is_object());
        ObjectRef::from_value(self).expect("asObject em objeto que o registro ainda não expõe (escopo)")
    }

    /// `toObject(globalObject)`: `None` com a exceção pendente. `JSValue::toObjectSlowCase` e
    /// `JSCell::toObjectSlow` (`String`, `Symbol` e os imediatos).
    pub fn to_object(&self, global_object: &JSGlobalObject) -> Option<ObjectRef> {
        if let Some(object) = ObjectRef::from_value(self) {
            return Some(object);
        }
        let vm = global_object.vm();
        if self.is_undefined_or_null() {
            let mut scope = ThrowScope::new(vm);
            throw_exception(global_object, &mut scope, create_not_an_object_error(global_object, *self));
            return None;
        }
        if self.is_number() {
            return ObjectRef::from_value(&construct_number(vm, global_object, *self).as_value());
        }
        if self.is_boolean() {
            return ObjectRef::from_value(&construct_boolean_from_immediate_boolean(vm, global_object, *self).as_value());
        }
        if self.is_string() {
            // `JSString::toObject`: `StringObject::create(vm, globalObject->stringObjectStructure(), this)`.
            let object = StringObject::create_with_string(vm, global_object.string_object_structure(), self.as_js_string());
            return ObjectRef::from_value(&object.as_value());
        }
        // `Symbol::toObject(globalObject)`: `SymbolObject::create(vm, globalObject->symbolObjectStructure(), this)`.
        if self.is_cell() {
            if let Some(symbol) = Symbol::from_cell_id(self.as_cell()) {
                let object = crate::runtime::symbol_object::symbol_to_object(global_object, &symbol);
                return ObjectRef::from_value(&object.as_value());
            }
        }
        // `JSBigInt::toObject(globalObject)`: `BigIntObject::create(vm, globalObject, this)`.
        if self.is_big_int() {
            let object = crate::runtime::bigint_object::BigIntObject::create(vm, global_object, *self);
            return ObjectRef::from_value(&object.as_value());
        }
        panic!(
            "toObject de valor que não é primitivo nem objeto (célula {:?})",
            self.is_cell().then(|| cell_registry::cell_type(self.as_cell()))
        )
    }

    /// `toPropertyKey(globalObject)`: `None` com a exceção pendente.
    pub fn to_property_key(&self, global_object: &JSGlobalObject) -> Option<Identifier> {
        let vm = global_object.vm();
        if self.is_string() {
            return Some(Identifier::from_string(vm, &self.as_js_string().value()));
        }
        let primitive = self.to_primitive_preferred(PreferredPrimitiveType::PreferString);
        if primitive.is_empty() {
            return None;
        }
        if primitive.is_cell() {
            if let Some(symbol) = Symbol::from_cell_id(primitive.as_cell()) {
                return Some(Identifier::from_private_name(&symbol.private_name()));
            }
        }
        let string = primitive.to_string(vm);
        if vm.exception().is_some() {
            return None;
        }
        Some(Identifier::from_string(vm, &string.value()))
    }

    /// `JSValue::synthesizePrototype(globalObject)` sem o ramo de `undefined`/`null`: o protótipo do
    /// invólucro de um primitivo (`None` se o valor não é um primitivo com invólucro).
    pub fn primitive_wrapper_prototype(&self, global_object: &JSGlobalObject) -> Option<JSValue> {
        let structure = if self.is_string() {
            global_object.string_object_structure()
        } else if self.is_number() {
            global_object.number_object_structure()
        } else if self.is_boolean() {
            global_object.boolean_object_structure()
        } else if self.is_symbol() {
            global_object.symbol_object_structure()
        } else if self.is_big_int() {
            global_object.big_int_object_structure()
        } else {
            return None;
        };
        Some(structure.stored_prototype())
    }

    /// `getPrototype(globalObject)` de `JSValue` (`JSValue::synthesizePrototype` para primitivo, que
    /// lança `createNotAnObjectError` em `undefined`/`null`): `Err(Pending)` com a exceção pendente.
    pub fn get_prototype(&self, global_object: &JSGlobalObject) -> Result<JSValue, Thrown> {
        if self.is_object() {
            return self.as_object().get_prototype(global_object);
        }
        if let Some(prototype) = self.primitive_wrapper_prototype(global_object) {
            return Ok(prototype);
        }
        debug_assert!(self.is_undefined_or_null());
        let mut scope = ThrowScope::new(global_object.vm());
        throw_exception(global_object, &mut scope, create_not_an_object_error(global_object, *self));
        Err(Thrown::Pending)
    }
}

/// `throwVMTypeError(globalObject, scope)` (mensagem `Type error`) e `throwVMTypeError(globalObject,
/// scope, message)`: lança e devolve o `EncodedJSValue` nulo.
pub fn throw_vm_type_error(global_object: &JSGlobalObject, message: Option<&str>) -> EncodedJSValue {
    let mut scope = ThrowScope::new(global_object.vm());
    let message = WtfString::from_utf8(message.unwrap_or("Type error").as_bytes());
    throw_vm_exception(global_object, &mut scope, create_type_error(global_object, &message)).encode()
}

/// `throwVMError(globalObject, scope, error)`: lança `error` e devolve o `EncodedJSValue` nulo.
pub fn throw_error_object(global_object: &JSGlobalObject, error: JSObjectHandle) -> EncodedJSValue {
    let mut scope = ThrowScope::new(global_object.vm());
    throw_vm_exception(global_object, &mut scope, error).encode()
}

/// O mesmo lançamento de `throw_error_object`, para quem devolve o marcador `Thrown::Pending`.
pub fn throw_error_pending(global_object: &JSGlobalObject, error: JSObjectHandle) -> Thrown {
    throw_error_object(global_object, error);
    Thrown::Pending
}

/// O `PutError` que as operações de `JSObject` devolvem no lugar de lançar, como exceção pendente (o
/// que o `shouldThrow = true` do C++ deixa no `ThrowScope`).
pub fn throw_put_error(global_object: &JSGlobalObject, error: PutError) {
    let mut scope = ThrowScope::new(global_object.vm());
    match error {
        PutError::TypeError(message) => {
            let message = WtfString::from_utf8(message.as_bytes());
            throw_exception(global_object, &mut scope, create_type_error(global_object, &message));
        }
        PutError::StackOverflow => {
            throw_stack_overflow_error(global_object, &mut scope);
        }
        PutError::OutOfMemory => {
            throw_out_of_memory_error(global_object, &mut scope);
        }
        PutError::RangeError(message) => {
            let message = WtfString::from_utf8(message.as_bytes());
            throw_exception(global_object, &mut scope, crate::runtime::error::create_range_error(global_object, &message));
        }
        PutError::Pending => {}
        PutError::Unported(what) => panic!("{what}"),
    }
}

/// `JSObject::defaultHasInstance(globalObject, value, proto)`: `None` com a exceção pendente.
pub fn default_has_instance(global_object: &JSGlobalObject, value: JSValue, proto: JSValue) -> Option<bool> {
    if !value.is_object() {
        return Some(false);
    }

    if !proto.is_object() {
        let mut scope = ThrowScope::new(global_object.vm());
        let message = WtfString::from_utf8(b"instanceof called on an object with an invalid prototype property.");
        throw_exception(global_object, &mut scope, create_type_error(global_object, &message));
        return None;
    }

    let mut object = value.as_object();
    loop {
        let object_value = match object.get_prototype(global_object) {
            Ok(object_value) => object_value,
            Err(thrown) => {
                throw_thrown(global_object, thrown);
                return None;
            }
        };
        if !object_value.is_object() {
            return Some(false);
        }
        object = object_value.as_object();
        if proto.as_cell() == object.as_value().as_cell() {
            return Some(true);
        }
    }
}
