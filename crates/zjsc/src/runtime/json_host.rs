//! `impl JsonHost for JSGlobalObject`: o que o `LiteralParser`, o `Stringifier` e o `Walker` de
//! `JSONObject.cpp` pedem do motor (ver `literal_parser.rs` e `json_object.rs`), ligado às operações que
//! o motor já tem: `JSFinalObject`, `JSArray`, `ObjectRef`, `get_call_data`/`call`, `own_property_keys`,
//! as caixas de primitivo e o `JSRawJSONObject`.
//!
//! Convenção de erro: toda operação do motor deixa a exceção pendente no `VM` (ou devolve `PutError`,
//! que `throw_put_error` torna pendente); o `JsonHost` devolve `Thrown::Value` com o valor lançado, e
//! `take_pending` é a única ponte entre os dois (limpa a exceção pendente, como o `NakedPtr<Exception>`
//! de `call(..., returnedException)`). `throw_json_error` relança o valor no fim do `JSON.parse`.
//!
//! DIVERGÊNCIAS:
//!
//! - `create_data_property` de índice usa `put_by_index` (o `putDirectIndex` e o `defineOwnIndexedProperty`
//!   ainda não existem no porte): igual ao C++ para objeto novo e para índice já presente; só diverge
//!   se a cadeia de protótipos tiver acessor de índice, que o `[[Set]]` chamaria e o `createDataProperty`
//!   não. O de nome usa o `defineOwnProperty` ordinário com todos os atributos ligados, como a
//!   especificação.
//! - `get` de base primitiva procura a propriedade no protótipo da classe (`String`, `Number`, `Boolean`,
//!   `Symbol`, `BigInt`) com o primitivo como `this`.
//! - `set_underscore_proto` faz o `put` de `__proto__` pelo caminho geral, que alcança o acessor do
//!   `Object.prototype` (ver `js_global_object_init.rs`).

use crate::runtime::array_constructor::{is_array, IsArrayCaller};
use crate::runtime::array_prototype::throw_array_error;
use crate::runtime::bigint_object::BigIntObject;
use crate::runtime::boolean_object::BooleanObject;
use crate::runtime::delete_property_slot::DeletePropertySlot;
use crate::runtime::enumeration_mode::{DontEnumPropertiesMode, PropertyNameMode};
use crate::runtime::host_call::throw_thrown;
use crate::runtime::host_function_support::{throw_put_error, ObjectRef};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_array::{construct_array, JSArray};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, PutError};
use crate::runtime::js_type::JSType;
use crate::runtime::sparse_array_value_map::PutDirectIndexMode;
use crate::runtime::js_promise_host::Thrown;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::js_wrapper_object::JSWrapperObject;
use crate::runtime::literal_parser::{BoxedPrimitiveKind, JsonHost, JsonKey};
use crate::runtime::number_object::NumberObject;
use crate::runtime::object_constructor::own_property_keys;
use crate::runtime::object_to_primitive::call_function;
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::put_property_slot::{PutContext, PutPropertySlot};
use crate::runtime::string_object::StringObject;
use crate::runtime::js_raw_json_object::JSRawJSONObject;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `2^53 - 1` (`maxSafeInteger`), o teto do `toLength`.
const MAX_SAFE_INTEGER: f64 = 9007199254740991.0;

/// A exceção pendente no `VM` como `Thrown` (limpando-a): o valor lançado, ou a terminação.
fn take_pending(global_object: &JSGlobalObject) -> Thrown {
    let vm = global_object.vm();
    let exception = vm.exception().expect("operação do host falhou sem exceção pendente no VM");
    let thrown = if vm.is_termination_exception(&exception) { Thrown::Termination } else { Thrown::Value(exception.value()) };
    vm.clear_exception();
    thrown
}

/// `RETURN_IF_EXCEPTION(scope, ...)` depois de uma operação que deixa a exceção pendente.
fn pending_or<T>(global_object: &JSGlobalObject, value: T) -> Result<T, Thrown> {
    if global_object.vm().exception().is_some() { Err(take_pending(global_object)) } else { Ok(value) }
}

/// Um `PutError` de `JSObject` como `Thrown` (`Unported` é `panic!`, em `throw_put_error`).
fn put_error(global_object: &JSGlobalObject, error: PutError) -> Thrown {
    throw_put_error(global_object, error);
    take_pending(global_object)
}

/// O `PropertyName` da chave (índice vira o nome numérico, que o `JSObject` reconhece como índice).
fn property_name(vm: &VM, key: &JsonKey) -> PropertyName {
    let identifier = match key {
        JsonKey::Index(index) => Identifier::from_u32(vm, *index),
        JsonKey::Name(name) => Identifier::from_string(vm, name),
    };
    PropertyName::from_identifier(&identifier)
}

/// O valor que o `ObjectRef` representa; todo valor que `is_object` aceita tem `ObjectRef`.
fn object_of(value: JSValue) -> ObjectRef {
    ObjectRef::from_value(&value).expect("JsonHost recebeu um objeto que o registro não expõe")
}


impl JsonHost for JSGlobalObject {
    fn vm(&self) -> &VM {
        JSGlobalObject::vm(self)
    }

    fn new_object(&self) -> Result<JSValue, Thrown> {
        Ok(JSFinalObject::create(self.vm(), &self.object_structure_for_object_constructor()).as_value())
    }

    fn new_array(&self, elements: &[JSValue]) -> Result<JSValue, Thrown> {
        Ok(construct_array(self.vm(), &self.array_structure(), elements).as_value())
    }

    fn create_data_property(&self, object: JSValue, key: &JsonKey, value: JSValue) -> Result<bool, Thrown> {
        let vm = self.vm();
        let object = object_of(object);
        let result = match key {
            // `Walker::walk`: o array usa `putDirectIndex(..., PutDirectIndexShouldNotThrow)`, que define a
            // propriedade própria sem consultar setters da cadeia de protótipos (`put_by_index` consultaria).
            JsonKey::Index(index) if object.type_() == JSType::ArrayType => {
                object.put_direct_index(vm, *index, value, 0, PutDirectIndexMode::PutDirectIndexShouldNotThrow)
            }
            // Objeto com chave de índice: `createDataProperty`, o `[[DefineOwnProperty]]` ordinário.
            JsonKey::Index(_) | JsonKey::Name(_) => {
                let descriptor = PropertyDescriptor::new(value, 0);
                object.define_own_property(self, &property_name(vm, key), &descriptor, false)
            }
        };
        result.map_err(|error| put_error(self, error))
    }

    fn set_underscore_proto(&self, object: JSValue, value: JSValue) -> Result<(), Thrown> {
        let vm = self.vm();
        let target = object_of(object);
        let name = PropertyName::from_identifier(&vm.property_names.underscore_proto);
        let mut slot = PutPropertySlot::new(object, false, PutContext::UnknownContext, false);
        target.put(vm, &name, value, &mut slot).map(|_| ()).map_err(|error| put_error(self, error))
    }

    fn is_object(&self, value: JSValue) -> bool {
        value.is_object()
    }

    fn is_callable(&self, value: JSValue) -> bool {
        value.is_callable()
    }

    fn call(&self, function: JSValue, this_value: JSValue, arguments: &[JSValue]) -> Result<JSValue, Thrown> {
        call_function(self, function, this_value, arguments).ok_or_else(|| take_pending(self))
    }

    fn get(&self, base: JSValue, key: &JsonKey) -> Result<JSValue, Thrown> {
        // Objeto (inclusive Proxy): o `[[Get]]` genérico, que dispara o trap `get` por índice como no
        // `Holder::appendNextProperty` do JSC (`object->get(globalObject, index)`).
        if base.is_object() {
            let value = object_of(base).get(self, &property_name(self.vm(), key));
            return pending_or(self, value);
        }
        let holder = if base.is_object() { base } else { base.primitive_wrapper_prototype(self).unwrap_or_else(JSValue::undefined) };
        let Some(holder) = ObjectRef::from_value(&holder) else {
            return Ok(JSValue::undefined());
        };
        let mut slot = PropertySlot::new(base, InternalMethodType::Get);
        let name = property_name(self.vm(), key);
        let found = holder.get_property_slot(self, &name, &mut slot);
        pending_or(self, ())?;
        if !found {
            return Ok(JSValue::undefined());
        }
        pending_or(self, slot.get_value_for(&name))
    }

    fn is_array(&self, value: JSValue) -> Result<bool, Thrown> {
        is_array(&value, IsArrayCaller::ArrayIsArray).map_err(|error| {
            throw_array_error(self, error);
            take_pending(self)
        })
    }

    fn length_of_array_like(&self, object: JSValue) -> Result<u64, Thrown> {
        if let Some(array) = JSArray::from_value(&object) {
            return Ok(u64::from(array.length()));
        }
        let length = object_of(object).get(self, &PropertyName::from_identifier(&self.vm().property_names.length));
        let length = pending_or(self, length)?;
        // `toLength`: `toIntegerOrInfinity` limitado a [0, 2^53 - 1].
        let number = pending_or(self, length.to_number())?;
        let integer = if number.is_nan() { 0.0 } else { number.trunc() };
        Ok(integer.clamp(0.0, MAX_SAFE_INTEGER) as u64)
    }

    fn own_enumerable_string_keys(&self, object: JSValue) -> Result<Vec<JsonKey>, Thrown> {
        let vm = self.vm();
        let keys = own_property_keys(self, &object_of(object), PropertyNameMode::Strings, DontEnumPropertiesMode::Exclude)
            .map_err(|thrown| {
                throw_thrown(self, thrown);
                take_pending(self)
            })?;
        Ok((0..keys.length()).map(|index| JsonKey::from_wtf_string(&keys.get_by_index(vm, index).as_js_string().value())).collect())
    }

    fn delete_property(&self, object: JSValue, key: &JsonKey) -> Result<(), Thrown> {
        let vm = self.vm();
        let object = object_of(object);
        let result = match key {
            JsonKey::Index(index) => object.delete_property_by_index(vm, *index),
            JsonKey::Name(_) => object.delete_property(vm, &property_name(vm, key), &mut DeletePropertySlot::default()),
        };
        result.map(|_| ()).map_err(|error| put_error(self, error))
    }

    fn boxed_primitive_kind(&self, object: JSValue) -> BoxedPrimitiveKind {
        if NumberObject::from_value(&object).is_some() {
            BoxedPrimitiveKind::Number
        } else if object.is_cell() && StringObject::from_cell_id(object.as_cell()).is_some() {
            BoxedPrimitiveKind::String
        } else if BooleanObject::from_value(&object).is_some() {
            BoxedPrimitiveKind::Boolean
        } else if BigIntObject::from_value(&object).is_some() {
            BoxedPrimitiveKind::BigInt
        } else {
            BoxedPrimitiveKind::None
        }
    }

    fn unwrap_boxed_primitive(&self, object: JSValue) -> Result<JSValue, Thrown> {
        match self.boxed_primitive_kind(object) {
            BoxedPrimitiveKind::Number => Ok(js_number(pending_or(self, object.to_number())?)),
            BoxedPrimitiveKind::String => pending_or(self, JSValue::from_js_string(object.to_string(self.vm()))),
            BoxedPrimitiveKind::Boolean | BoxedPrimitiveKind::BigInt => {
                Ok(JSWrapperObject::from_cell_id(object.as_cell()).expect("caixa de primitivo sem JSWrapperObject").internal_value())
            }
            BoxedPrimitiveKind::None => {
                unreachable!("unwrap_boxed_primitive só é chamado para caixa de Number, String, Boolean ou BigInt")
            }
        }
    }

    fn raw_json_text(&self, object: JSValue) -> Option<WtfString> {
        JSRawJSONObject::from_value(&object).map(|object| JSRawJSONObject::raw_json(&object).value())
    }

    fn to_string(&self, value: JSValue) -> Result<WtfString, Thrown> {
        pending_or(self, value.to_string(self.vm()).value())
    }
}
