//! Porte de `runtime/ObjectPrototype.h`, `ObjectPrototypeInlines.h` e `ObjectPrototype.cpp`: o
//! `Object.prototype` (um `JSNonFinalObject` sem campos próprios, com `IsImmutablePrototypeExoticObject`).
//!
//! LACUNAS, e por quê:
//! - `objectProtoToStringFunction` é um `LazyProperty` do `JSGlobalObject`; aqui a `JSFunction` é criada em
//!   `finishCreation` (a primeira propriedade, como no C++) e guardada em `object_proto_to_string_function`.
//!   O acessor `__proto__` não é do `finishCreation`: entra no `JSGlobalObject::init`
//!   (`add_underscore_proto_accessor`).
//! - `HasOwnPropertyCache` e o `cacheSpecialProperty` do `objectPrototypeToString` não existem: a consulta é
//!   direta (os caches só evitam a consulta, sem efeito observável).
//! - Ler `toString` por acessor (`PropertySlot::getValue` com getter) depende da reentrada no
//!   interpretador pelo `PropertySlot`.
//! - Receptor que é primitivo (`toObject` precisa dos objetos-invólucro) cai na lacuna de
//!   `host_function_support.rs`; `JSFunction` é alcançada pelo `ObjectRef` (propriedades preguiçosas
//!   materializadas por `js_function_reify.rs`).

use crate::interpreter::call_frame::NativeCallFrame;
use crate::runtime::array_constructor::{is_array, IsArrayCaller};
use crate::runtime::array_prototype::throw_array_error;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::{finish, Thrown};
use crate::runtime::host_function_support::{throw_put_error, throw_vm_type_error, ObjectRef};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, put_direct_native_function_without_transition, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::{js_string, JSStringRef};
use crate::runtime::js_string_builder::js_make_nontrivial_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, IS_IMMUTABLE_PROTOTYPE_EXOTIC_OBJECT};
use crate::runtime::js_value::{js_boolean, js_undefined, EncodedJSValue, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::object_to_primitive::call_function;
use crate::runtime::property_attribute::{CUSTOM_ACCESSOR, DONT_ENUM};
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::proxy_object::to_this_strict;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo ObjectPrototype::s_info`.
pub static OBJECT_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `callFrame->thisValue().toThis(globalObject, ECMAMode::strict()).toObject(globalObject)`: `None` com a
/// exceção pendente.
fn this_object(global_object: &JSGlobalObject, call_frame: &NativeCallFrame<'_>) -> Option<ObjectRef> {
    to_this_strict(call_frame.this_value()).to_object(global_object)
}

/// `objectProtoFuncValueOf`.
pub(crate) fn object_proto_func_value_of(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    match this_object(global_object, call_frame) {
        Some(value_obj) => value_obj.as_value().encode(),
        None => JSValue::empty().encode(),
    }
}

/// `objectPrototypeHasOwnProperty(globalObject, thisObject, propertyName)`.
pub fn object_prototype_has_own_property(global_object: &JSGlobalObject, this_object: &ObjectRef, property_name: &PropertyName) -> bool {
    this_object.has_own_property(global_object, property_name)
}

/// `objectProtoFuncHasOwnProperty`.
fn object_proto_func_has_own_property(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let global_object: &JSGlobalObject = global_object;
    let Some(property_key) = call_frame.argument(0).to_property_key(global_object) else {
        return JSValue::empty().encode();
    };
    let Some(this_object) = this_object(global_object, call_frame) else {
        return JSValue::empty().encode();
    };
    let property_name = PropertyName::from_identifier(&property_key);
    finish(global_object, own_property_descriptor(global_object, &this_object, &property_name).map(|found| js_boolean(found.is_some())))
}

/// O descritor próprio de `this_object`, com o trap `getOwnPropertyDescriptor` quando é um `Proxy`
/// (`hasOwnProperty` e `propertyIsEnumerable` do JSC chamam `getOwnPropertyDescriptor` do método virtual).
fn own_property_descriptor(
    global_object: &JSGlobalObject,
    this_object: &ObjectRef,
    property_name: &PropertyName,
) -> Result<Option<PropertyDescriptor>, Thrown> {
    let lookup = this_object.for_property_lookup(global_object, property_name).ok_or(Thrown::Pending)?;
    crate::runtime::object_constructor::own_descriptor(global_object, lookup, property_name)
}

/// `objectProtoFuncIsPrototypeOf`.
fn object_proto_func_is_prototype_of(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let candidate = call_frame.argument(0);
    if !candidate.is_object() {
        return js_boolean(false).encode();
    }

    let Some(this_obj) = this_object(global_object, call_frame) else {
        return JSValue::empty().encode();
    };

    let this_cell = this_obj.as_value().as_cell();
    let mut object = candidate.as_object();
    finish(global_object, loop {
        let v = match object.get_prototype(global_object) {
            Ok(v) => v,
            Err(thrown) => break Err(thrown),
        };
        if !v.is_object() {
            break Ok(js_boolean(false));
        }
        if v.as_cell() == this_cell {
            break Ok(js_boolean(true));
        }
        object = v.as_object();
    })
}

/// Qual dos dois acessores `__defineGetter__`/`__defineSetter__`/`__lookupGetter__`/`__lookupSetter__`
/// trata (o C++ repete o corpo; aqui a diferença é este parâmetro).
#[derive(Clone, Copy)]
enum AccessorKind {
    Getter,
    Setter,
}

/// `objectProtoFuncDefineGetter` e `objectProtoFuncDefineSetter`.
fn define_accessor(global_object: &JSGlobalObject, call_frame: &NativeCallFrame<'_>, kind: AccessorKind) -> EncodedJSValue {
    let Some(this_object) = this_object(global_object, call_frame) else {
        return JSValue::empty().encode();
    };

    let accessor = call_frame.argument(1);
    if !accessor.is_callable() {
        let message = match kind {
            AccessorKind::Getter => "invalid getter usage",
            AccessorKind::Setter => "invalid setter usage",
        };
        return throw_vm_type_error(global_object, Some(message));
    }

    let Some(property_key) = call_frame.argument(0).to_property_key(global_object) else {
        return JSValue::empty().encode();
    };

    let mut descriptor = PropertyDescriptor::default();
    match kind {
        AccessorKind::Getter => descriptor.set_getter(accessor),
        AccessorKind::Setter => descriptor.set_setter(accessor),
    }
    descriptor.set_enumerable(true);
    descriptor.set_configurable(true);

    let should_throw = true;
    let property_name = PropertyName::from_identifier(&property_key);
    // `thisObject->methodTable()->defineOwnProperty`: o `Array` tem o `[[DefineOwnProperty]]` próprio, que
    // checa `length` não gravável antes de "não extensível" (mensagem "non-writable length property").
    if let Some(array) = crate::runtime::js_array::JSArray::from_value_by_class(&this_object.as_value()) {
        if let Err(error) = array.define_own_property(global_object.vm(), &property_name, &descriptor, should_throw) {
            throw_array_error(global_object, error);
        }
        return js_undefined().encode();
    }
    let result = this_object.define_own_property(
        global_object,
        &property_name,
        &descriptor,
        should_throw,
    );
    if let Err(error) = result {
        throw_put_error(global_object, error);
    }

    js_undefined().encode()
}

/// `objectProtoFuncDefineGetter`.
fn object_proto_func_define_getter(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    define_accessor(global_object, call_frame, AccessorKind::Getter)
}

/// `objectProtoFuncDefineSetter`.
fn object_proto_func_define_setter(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    define_accessor(global_object, call_frame, AccessorKind::Setter)
}

/// `objectProtoFuncLookupGetter` e `objectProtoFuncLookupSetter`.
fn lookup_accessor(global_object: &JSGlobalObject, call_frame: &NativeCallFrame<'_>, kind: AccessorKind) -> EncodedJSValue {
    let vm = global_object.vm();
    let Some(this_object) = this_object(global_object, call_frame) else {
        return JSValue::empty().encode();
    };

    let Some(property_key) = call_frame.argument(0).to_property_key(global_object) else {
        return JSValue::empty().encode();
    };
    let property_name = PropertyName::from_identifier(&property_key);

    let mut slot = PropertySlot::new(this_object.as_value(), InternalMethodType::GetOwnProperty);
    let has_property = this_object.get_property_slot(global_object, &property_name, &mut slot);
    if has_property {
        if slot.is_accessor() {
            let getter_setter = slot.getter_setter();
            return match kind {
                AccessorKind::Getter => getter_setter.getter_value_or_undefined(),
                AccessorKind::Setter => getter_setter.setter_value_or_undefined(),
            }
            .encode();
        }
        if slot.attributes() & CUSTOM_ACCESSOR != 0 {
            let slot_base = slot.slot_base().and_then(|cell_id| JSObject::from_cell_id(cell_id));
            let slot_base = slot_base.expect("slot.slotBase() de CustomAccessor");
            let mut descriptor = PropertyDescriptor::default();
            if slot_base.get_own_property_descriptor(vm, &property_name, &mut descriptor) {
                let (present, value) = match kind {
                    AccessorKind::Getter => (descriptor.getter_present(), descriptor.getter()),
                    AccessorKind::Setter => (descriptor.setter_present(), descriptor.setter()),
                };
                return if present { value.encode() } else { js_undefined().encode() };
            }
        }
    }

    js_undefined().encode()
}

/// `objectProtoFuncLookupGetter`.
fn object_proto_func_lookup_getter(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    lookup_accessor(global_object, call_frame, AccessorKind::Getter)
}

/// `objectProtoFuncLookupSetter`.
fn object_proto_func_lookup_setter(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    lookup_accessor(global_object, call_frame, AccessorKind::Setter)
}

/// `objectProtoFuncPropertyIsEnumerable`.
fn object_proto_func_property_is_enumerable(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let global_object: &JSGlobalObject = global_object;
    let Some(property_key) = call_frame.argument(0).to_property_key(global_object) else {
        return JSValue::empty().encode();
    };

    let Some(this_object) = this_object(global_object, call_frame) else {
        return JSValue::empty().encode();
    };

    let property_name = PropertyName::from_identifier(&property_key);
    finish(
        global_object,
        own_property_descriptor(global_object, &this_object, &property_name)
            .map(|found| js_boolean(found.is_some_and(|descriptor| descriptor.enumerable()))),
    )
}

/// `objectProtoFuncToLocaleString` (15.2.4.3 Object.prototype.toLocaleString()). LACUNA: o `call` final
/// (ver o cabeçalho do módulo).
fn object_proto_func_to_locale_string(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let global_object: &JSGlobalObject = global_object;
    let vm = global_object.vm();

    // 1. Let V be the this value.
    let this_value = call_frame.this_value();

    // 2. Invoke(V, "toString")

    // Let O be the result of calling ToObject passing the this value as the argument.
    let Some(object) = this_object(global_object, call_frame) else {
        return JSValue::empty().encode();
    };

    // Let toString be the O.[[Get]]("toString", V)
    let mut slot = PropertySlot::new(this_value, InternalMethodType::Get);
    let to_string_name = PropertyName::from_identifier(&vm.property_names.to_string);
    let has_property = object.get_property_slot(global_object, &to_string_name, &mut slot);
    let to_string = if has_property { slot.get_value_for(&to_string_name) } else { js_undefined() };
    if global_object.vm().exception().is_some() {
        return JSValue::empty().encode();
    }

    // If IsCallable(toString) is false, throw a TypeError exception.
    if !to_string.is_callable() {
        return throw_vm_type_error(global_object, None);
    }

    // Return the result of calling the [[Call]] internal method of toString passing the this value and no arguments.
    match call_function(global_object, to_string, this_value, &[]) {
        Some(result) => result.encode(),
        None => JSValue::empty().encode(),
    }
}

/// `inferBuiltinTag(globalObject, object)` (`ObjectPrototypeInlines.h`): o `ASCIILiteral` do tag nativo, ou
/// `None` com a exceção pendente. O `JSString*` comum (`vm.smallStrings.objectXString()`) não existe: o
/// resultado final é montado do mesmo jeito para todos os tags, com o mesmo texto.
fn infer_builtin_tag(global_object: &JSGlobalObject, object: &ObjectRef) -> Option<&'static str> {
    Some(match object.type_() {
        JSType::ArrayType | JSType::DerivedArrayType => "Array",
        JSType::DirectArgumentsType | JSType::ScopedArgumentsType | JSType::ClonedArgumentsType => "Arguments",
        JSType::JSFunctionType | JSType::InternalFunctionType => "Function",
        JSType::ErrorInstanceType => "Error",
        JSType::JSDateType => "Date",
        JSType::RegExpObjectType => "RegExp",
        JSType::BooleanObjectType => "Boolean",
        JSType::NumberObjectType => "Number",
        JSType::StringObjectType | JSType::DerivedStringObjectType => "String",
        JSType::FinalObjectType => "Object",
        _ => {
            let object_is_array = match is_array(&object.as_value(), IsArrayCaller::ObjectPrototypeToString) {
                Ok(value) => value,
                Err(error) => {
                    throw_array_error(global_object, error);
                    return None;
                }
            };
            if object_is_array {
                "Array"
            } else if object.as_value().is_callable() {
                "Function"
            } else {
                "Object"
            }
        }
    })
}

/// `objectPrototypeToStringSlow(globalObject, thisObject)`, sem o `cacheSpecialProperty` (cache da
/// `Structure` sem efeito observável, como o `cachedSpecialProperty` de `object_to_primitive.rs`).
fn object_prototype_to_string_slow(global_object: &JSGlobalObject, this_object: &ObjectRef) -> Option<JSStringRef> {
    let vm = global_object.vm();

    let tag = infer_builtin_tag(global_object, this_object)?;

    let mut slot = PropertySlot::new(this_object.as_value(), InternalMethodType::Get);
    let to_string_tag_name = PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol);
    let has_property = this_object.get_property_slot(global_object, &to_string_tag_name, &mut slot);
    let mut js_tag = None;
    if has_property {
        let tag_value = slot.get_value_for(&to_string_tag_name);
        if vm.exception().is_some() {
            return None;
        }
        if tag_value.is_string() {
            js_tag = Some(tag_value.as_js_string().value());
        }
    }

    let tag_text = js_tag.unwrap_or_else(|| WtfString::from_latin1(tag.as_bytes()));
    js_make_nontrivial_string(global_object, &[&"[object ", &tag_text, &"]"])
}

/// `objectPrototypeToString(globalObject, thisValue)`: `None` com a exceção pendente.
pub fn object_prototype_to_string(global_object: &JSGlobalObject, this_value: JSValue) -> Option<JSStringRef> {
    let vm = global_object.vm();
    if this_value.is_undefined() {
        return Some(js_string(vm, &WtfString::from_latin1(b"[object Undefined]")));
    }
    if this_value.is_null() {
        return Some(js_string(vm, &WtfString::from_latin1(b"[object Null]")));
    }

    let this_object = this_value.to_object(global_object)?;
    object_prototype_to_string_slow(global_object, &this_object)
}

/// `objectProtoFuncToString`.
fn object_proto_func_to_string(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    match object_prototype_to_string(global_object, to_this_strict(call_frame.this_value())) {
        Some(string) => JSValue::from_js_string(string).encode(),
        None => JSValue::empty().encode(),
    }
}

impl JSGlobalObject {
    /// `objectProtoToStringFunction()` (o valor da `JSFunction`).
    pub fn object_proto_to_string_function(&self) -> JSValue {
        self.object_proto_to_string_function.borrow().expect("JSGlobalObject sem objectProtoToStringFunction")
    }
}

/// `class ObjectPrototype : public JSNonFinalObject`.
pub struct ObjectPrototype;

impl ObjectPrototype {
    /// `StructureFlags = Base::StructureFlags | IsImmutablePrototypeExoticObject`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | IS_IMMUTABLE_PROTOTYPE_EXOTIC_OBJECT;

    /// `createStructure(vm, globalObject, prototype)` (`ObjectPrototypeInlines.h`).
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, ObjectPrototype::STRUCTURE_FLAGS),
            &OBJECT_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `ObjectPrototype(vm, structure)` e `finishCreation`. O objeto
    /// é registrado como `CellEntry::Object`, porque a classe não tem campos próprios.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        ObjectPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`: `toString` (o `objectProtoToStringFunction()` do global, criado aqui
    /// porque o porte não tem `LazyProperty`) e as demais funções, na ordem do C++.
    fn finish_creation(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);

        let names = &vm.property_names;
        let to_string_function = JSFunction::create_native(
            vm,
            global_object,
            0,
            names.to_string.string().string(),
            object_proto_func_to_string,
            ImplementationVisibility::Public,
            Intrinsic::ObjectToStringIntrinsic,
            call_host_function_as_constructor,
        );
        *global_object.object_proto_to_string_function.borrow_mut() = Some(to_string_function.as_value());
        prototype.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&names.to_string),
            to_string_function.as_value(),
            DONT_ENUM,
        );

        let functions: [(&Identifier, u32, NativeFunction, Intrinsic); 9] = [
            (&names.to_locale_string, 0, object_proto_func_to_locale_string, Intrinsic::NoIntrinsic),
            (&names.value_of, 0, object_proto_func_value_of, Intrinsic::NoIntrinsic),
            (&names.has_own_property, 1, object_proto_func_has_own_property, Intrinsic::HasOwnPropertyIntrinsic),
            (&names.property_is_enumerable, 1, object_proto_func_property_is_enumerable, Intrinsic::NoIntrinsic),
            (&names.is_prototype_of, 1, object_proto_func_is_prototype_of, Intrinsic::NoIntrinsic),
            (&names._define_getter_, 2, object_proto_func_define_getter, Intrinsic::NoIntrinsic),
            (&names._define_setter_, 2, object_proto_func_define_setter, Intrinsic::NoIntrinsic),
            (&names._lookup_getter_, 1, object_proto_func_lookup_getter, Intrinsic::NoIntrinsic),
            (&names._lookup_setter_, 1, object_proto_func_lookup_setter, Intrinsic::NoIntrinsic),
        ];
        for (name, length, function, intrinsic) in functions {
            put_direct_native_function_without_transition(
                vm,
                global_object,
                prototype,
                name,
                length,
                function,
                ImplementationVisibility::Public,
                intrinsic,
                DONT_ENUM,
            );
        }
    }
}
