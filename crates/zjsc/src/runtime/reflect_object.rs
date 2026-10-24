//! Porte de `runtime/ReflectObject.{h,cpp}` e de `builtins/ReflectObject.js`: o objeto `Reflect` (um
//! `JSNonFinalObject` com o `ClassInfo` `"Reflect"`) e as treze funções.
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê:
//! - `apply`, `deleteProperty`, `get` e `has` são `JSBuiltin` (JS embutido de `builtins/ReflectObject.js`)
//!   e entram aqui pelo `BuiltinCodeIndex` (`ReflectObject{Apply,DeleteProperty,Get,Has}Code`), como no C++:
//!   `Function.prototype.toString` dá `[native code]` e o frame aparece na pilha como `get (native:1:11)`.
//! - A tabela estática (`ReflectObject.lut.h`) é `REFLECT_OBJECT_TABLE`, reificada no primeiro acesso
//!   (`lookup.rs`); o `finishCreation` só põe o `@@toStringTag`, como no C++.
//! - O `JSObject` do porte não tem a tabela de métodos virtual (`methodTable()->get/put/defineOwnProperty/
//!   deleteProperty/preventExtensions`): as operações abaixo passam pelo despacho de `proxy_object.rs`
//!   (`object_set`, `object_define_own_property`, `object_is_extensible`, `object_prevent_extensions`,
//!   `JSObject::get_prototype`, `object_set_prototype`), que consulta o `Proxy` e usa o `JSObject` comum
//!   (com `JSArray` à mão) nos demais casos. `get`, `has` e `deleteProperty` não passam por aqui: são o JS
//!   embutido, que usa os intrínsecos privados. `getOwnPropertyDescriptor` e `ownKeys` vão pelo
//!   `object_constructor.rs`, que consulta o `Proxy`.
//! - Consulta de propriedade própria de `JSFunction` (`name`, `length`, `prototype`) passa pelo
//!   `ObjectRef::for_property_lookup` e pelo `getOwnPropertySlot` de função, ligados em `proxy_object.rs` e
//!   `object_constructor.rs`.
//! - `defineProperty` sobre nome de índice e `preventExtensions` de objeto com propriedades indexadas são
//!   `Unported` no `JSObject` (`defineOwnIndexedProperty`, `enterDictionaryIndexingMode`).

use crate::host_function;
use crate::runtime::call_data::{construct, get_construct_data};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::js_type_info::HAS_STATIC_PROPERTY_TABLE;
use crate::runtime::lookup::{reify_static_property, HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{builtin_entry, native_entry_with_intrinsic};
use crate::runtime::property_attribute::{BUILTIN, DONT_ENUM, FUNCTION};
use crate::runtime::enumeration_mode::{DontEnumPropertiesMode, PropertyNameMode};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::identifier::Identifier;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::thrown_from_llint;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::object_constructor::{
    object_constructor_get_own_property_descriptor_of, own_property_keys, to_property_descriptor,
};
use crate::runtime::property_name::PropertyName;
use crate::runtime::proxy_object::{
    list_from_array_like, object_define_own_property, object_is_extensible, object_prevent_extensions, object_set, object_set_prototype,
};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo ReflectObject::s_info`.
pub static REFLECT_OBJECT_S_INFO: ClassInfo = ClassInfo {
    class_name: "Reflect",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&REFLECT_OBJECT_TABLE),
    inherits_js_type_range: None,
};

/// `reflectObjectTableValues` de `ReflectObject.lut.h`, na ordem do `@begin`.
static REFLECT_OBJECT_TABLE_VALUES: [HashTableValue; 13] = [
    builtin_entry("apply", BuiltinCodeIndex::ReflectObjectApplyCode, 3),
    native_entry_with_intrinsic("construct", reflect_object_construct, 2, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("defineProperty", reflect_object_define_property, 3, Intrinsic::NoIntrinsic),
    builtin_entry("deleteProperty", BuiltinCodeIndex::ReflectObjectDeletePropertyCode, 2),
    builtin_entry("get", BuiltinCodeIndex::ReflectObjectGetCode, 2),
    native_entry_with_intrinsic("getOwnPropertyDescriptor", reflect_object_get_own_property_descriptor, 2, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("getPrototypeOf", reflect_object_get_prototype_of, 1, Intrinsic::ReflectGetPrototypeOfIntrinsic),
    builtin_entry("has", BuiltinCodeIndex::ReflectObjectHasCode, 2),
    native_entry_with_intrinsic("isExtensible", reflect_object_is_extensible, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("ownKeys", reflect_object_own_keys, 1, Intrinsic::ReflectOwnKeysIntrinsic),
    native_entry_with_intrinsic("preventExtensions", reflect_object_prevent_extensions, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("set", reflect_object_set, 3, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("setPrototypeOf", reflect_object_set_prototype_of, 2, Intrinsic::NoIntrinsic),
];

/// `reflectObjectTable`.
static REFLECT_OBJECT_TABLE: HashTable = HashTable { class_for_this: None, values: &REFLECT_OBJECT_TABLE_VALUES };

/// `ReflectOwnKeysNonObjectArgumentError` (`ReflectObject.h`).
pub const REFLECT_OWN_KEYS_NON_OBJECT_ARGUMENT_ERROR: &str = "Reflect.ownKeys requires the first argument be an object";

// ---------------------------------------------------------------------------------------------
// Operações internas de objeto (o que `methodTable()` despacharia).
// ---------------------------------------------------------------------------------------------

/// `isObject` com a mensagem de `TypeError` do C++ quando não é.
fn require_object(value: JSValue, message: &str) -> Result<ObjectRef, Thrown> {
    if !value.is_object() {
        return Err(Thrown::type_error(message));
    }
    Ok(value.as_object())
}

/// `JSValue::toPropertyKey(globalObject)` como `PropertyName`.
fn to_property_name(global_object: &JSGlobalObject, value: JSValue) -> Result<PropertyName, Thrown> {
    let identifier: Identifier = value.to_property_key(global_object).ok_or(Thrown::Pending)?;
    Ok(PropertyName::from_identifier(&identifier))
}

// ---------------------------------------------------------------------------------------------
// As funções.
// ---------------------------------------------------------------------------------------------

/// `reflectObjectConstruct`.
fn reflect_object_construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    const NOT_A_CONSTRUCTOR: &str = "Reflect.construct requires the first argument be a constructor";

    let target = call.argument(0);
    if !target.is_object() {
        return Err(Thrown::type_error(NOT_A_CONSTRUCTOR));
    }
    let construct_data = get_construct_data(target);
    if construct_data.is_none() {
        return Err(Thrown::type_error(NOT_A_CONSTRUCTOR));
    }

    let mut new_target = target;
    if call.argument_count() >= 3 {
        new_target = call.argument(2);
        if !new_target.is_object() || get_construct_data(new_target).is_none() {
            return Err(Thrown::type_error("Reflect.construct requires the third argument be a constructor if present"));
        }
    }

    let arguments_object = require_object(call.argument(1), "Reflect.construct requires the second argument be an object")?;
    let arguments = list_from_array_like(global_object, &arguments_object)?;
    construct(global_object, target, &construct_data, &arguments, new_target).map_err(thrown_from_llint)
}

/// `reflectObjectDefineProperty`: não lança quando o `[[DefineOwnProperty]]` falha (`shouldThrow = false`).
fn reflect_object_define_property_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let target = require_object(call.argument(0), "Reflect.defineProperty requires the first argument be an object")?;
    let property_name = to_property_name(global_object, call.argument(1))?;
    let descriptor = to_property_descriptor(global_object, call.argument(2))?;
    Ok(js_boolean(object_define_own_property(global_object, &target, &property_name, &descriptor, false)?))
}

/// `reflectObjectGetOwnPropertyDescriptor`.
fn reflect_object_get_own_property_descriptor_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let target = require_object(call.argument(0), "Reflect.getOwnPropertyDescriptor requires the first argument be an object")?;
    let key = call.argument(1).to_property_key(global_object).ok_or(Thrown::Pending)?;
    object_constructor_get_own_property_descriptor_of(global_object, &target, &key)
}

/// `reflectObjectGetPrototypeOf`.
fn reflect_object_get_prototype_of_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let target = require_object(call.argument(0), "Reflect.getPrototypeOf requires the first argument be an object")?;
    target.get_prototype(global_object)
}

/// `reflectObjectIsExtensible`.
fn reflect_object_is_extensible_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let target = require_object(call.argument(0), "Reflect.isExtensible requires the first argument be an object")?;
    Ok(js_boolean(object_is_extensible(global_object, &target)?))
}

/// `reflectObjectOwnKeys`.
fn reflect_object_own_keys_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let target = require_object(call.argument(0), REFLECT_OWN_KEYS_NON_OBJECT_ARGUMENT_ERROR)?;
    let keys = own_property_keys(global_object, &target, PropertyNameMode::StringsAndSymbols, DontEnumPropertiesMode::Include)?;
    Ok(keys.as_value())
}

/// `reflectObjectPreventExtensions`.
fn reflect_object_prevent_extensions_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let target = require_object(call.argument(0), "Reflect.preventExtensions requires the first argument be an object")?;
    Ok(js_boolean(object_prevent_extensions(global_object, &target)?))
}

/// `reflectObjectSet`: não lança erro de somente leitura em modo estrito (`shouldThrowIfCantSet = false`).
fn reflect_object_set_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let target = require_object(call.argument(0), "Reflect.set requires the first argument be an object")?;
    let property_name = to_property_name(global_object, call.argument(1))?;
    let receiver = if call.argument_count() >= 4 { call.argument(3) } else { call.argument(0) };
    Ok(js_boolean(object_set(global_object, &target, &property_name, call.argument(2), receiver, false)?))
}

/// `reflectObjectSetPrototypeOf`: `setPrototype(vm, globalObject, proto, shouldThrowIfCantSet = false)`.
fn reflect_object_set_prototype_of_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let target = require_object(call.argument(0), "Reflect.setPrototypeOf requires the first argument be an object")?;
    let proto = call.argument(1);
    if !proto.is_object() && !proto.is_null() {
        return Err(Thrown::type_error("Reflect.setPrototypeOf requires the second argument be either an object or null"));
    }
    Ok(js_boolean(object_set_prototype(global_object, &target, proto, false)?))
}

host_function!(reflect_object_construct, reflect_object_construct_body);
host_function!(reflect_object_define_property, reflect_object_define_property_body);
host_function!(reflect_object_get_own_property_descriptor, reflect_object_get_own_property_descriptor_body);
host_function!(reflect_object_get_prototype_of, reflect_object_get_prototype_of_body);
host_function!(reflect_object_is_extensible, reflect_object_is_extensible_body);
host_function!(pub reflect_object_own_keys, reflect_object_own_keys_body);
host_function!(reflect_object_prevent_extensions, reflect_object_prevent_extensions_body);
host_function!(reflect_object_set, reflect_object_set_body);
host_function!(reflect_object_set_prototype_of, reflect_object_set_prototype_of_body);

/// `class ReflectObject final : public JSNonFinalObject`: sem campos próprios.
pub struct ReflectObject;

impl ReflectObject {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, ReflectObject::STRUCTURE_FLAGS),
            &REFLECT_OBJECT_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `ReflectObject(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let object = JSObject::allocate(vm, structure);
        ReflectObject::finish_creation(&object, vm, global_object);
        object
    }

    /// `finishCreation(vm, globalObject)`: só o `@@toStringTag`; as treze entradas de `reflectObjectTable`
    /// são reificadas no primeiro acesso.
    fn finish_creation(object: &JSObject, vm: &VM, _global_object: &JSGlobalObject) {
        object.finish_creation(vm);
        put_to_string_tag(vm, object, REFLECT_OBJECT_S_INFO.class_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::executable::ExecutableBaseRef;
    use std::rc::Rc;

    /// `apply`, `deleteProperty`, `get` e `has` entram como builtins JS (nome público do C++, `length` do
    /// número de parâmetros do `ReflectObject.js`, sem intrínseco, `DontEnum`); as demais continuam nativas,
    /// com o intrínseco da tabela.
    #[test]
    fn installs_js_builtins_and_natives_with_name_length_and_intrinsic() {
        let global = JSGlobalObject::init(&Rc::new(VM::new()));
        let vm = global.vm();
        let structure = ReflectObject::create_structure(vm, &global, global.object_prototype().as_value());
        let reflect = ReflectObject::create(vm, &global, &structure);

        // (nome, length, é builtin JS, intrínseco)
        let expected: [(&str, u32, bool, Intrinsic); 13] = [
            ("apply", 3, true, Intrinsic::NoIntrinsic),
            ("construct", 2, false, Intrinsic::NoIntrinsic),
            ("defineProperty", 3, false, Intrinsic::NoIntrinsic),
            ("deleteProperty", 2, true, Intrinsic::NoIntrinsic),
            ("get", 2, true, Intrinsic::NoIntrinsic),
            ("getOwnPropertyDescriptor", 2, false, Intrinsic::NoIntrinsic),
            ("getPrototypeOf", 1, false, Intrinsic::ReflectGetPrototypeOfIntrinsic),
            ("has", 2, true, Intrinsic::NoIntrinsic),
            ("isExtensible", 1, false, Intrinsic::NoIntrinsic),
            ("ownKeys", 1, false, Intrinsic::ReflectOwnKeysIntrinsic),
            ("preventExtensions", 1, false, Intrinsic::NoIntrinsic),
            ("set", 3, false, Intrinsic::NoIntrinsic),
            ("setPrototypeOf", 2, false, Intrinsic::NoIntrinsic),
        ];
        for (name, length, is_builtin, intrinsic) in expected {
            let identifier = Identifier::from_span(vm, name.as_bytes());
            assert!(reify_static_property(vm, &reflect, REFLECT_OBJECT_TABLE.entry(name).unwrap()), "reificar {name}");
            let value = reflect.get_direct_by_name(vm, &PropertyName::from_identifier(&identifier));
            let function = value.as_js_function().unwrap_or_else(|| panic!("{name} não é uma JSFunction"));
            assert_eq!(!function.is_host_function(), is_builtin, "{name}: builtin JS ou nativa");
            let installed_intrinsic = match function.executable() {
                ExecutableBaseRef::Script(script) => {
                    assert_eq!(function.js_executable().borrow().parameter_count(), length, "length de {name}");
                    script.intrinsic()
                }
                ExecutableBaseRef::Native(native) => native.borrow().intrinsic(),
            };
            assert_eq!(installed_intrinsic, intrinsic, "intrínseco de {name}");
            if is_builtin {
                assert_eq!(&function.js_executable().borrow().name(), &identifier, "nome de {name}");
            }
        }
    }
}
