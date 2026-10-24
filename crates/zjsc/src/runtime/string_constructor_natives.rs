//! As cascas finas das funções nativas de `StringConstructor.cpp` (`stringFromCharCode`, `stringFromCodePoint`,
//! `stringRaw`, `callStringConstructor`, `constructWithStringConstructor`) e o `StringConstructor::create`
//! com a tabela `stringConstructorTable` (`fromCharCode` e `fromCodePoint` com `length` 1 e intrínseco, `raw`
//! sem). Os algoritmos puros estão em `string_constructor.rs`.
//!
//! DIVERGÊNCIAS:
//! - `StringConstructor` é um `JSFunction` sobre `NativeExecutable` (como o `NumberConstructor`), com a
//!   chamada e a construção do C++ e o `StringConstructorIntrinsic`.
//! - `String.raw` com `this`-objeto exige `toObject` do `template`: primitivos que não sejam
//!   `undefined`/`null` seguem a lacuna de `JSValue::to_object` (ver `host_function_support.rs`).
//!   O ramo rápido por `butterfly` de `JSArray` é o `getByIndex` genérico.

use crate::interpreter::call_frame::NativeCallFrame;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::throw_thrown;
use crate::runtime::host_function_support::throw_vm_type_error;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::js_function::{JSFunction, JSFunctionRef, JS_FUNCTION_S_INFO};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{native_entry_with_intrinsic};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::js_string::{js_empty_string, js_string};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, FUNCTION, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::string_constructor as algo;
use crate::runtime::string_object::{StringObject, StringObjectRef};
use crate::runtime::string_prototype::{concat, string_from_units, StringOpError};
use crate::runtime::string_prototype_natives::throw_string_op_error;
use crate::runtime::string_regexp_support::{to_string_value, to_wtf_string_value};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::symbol::Symbol;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo StringConstructor::s_info` (`"Function"`, base `InternalFunction`, `&stringConstructorTable`).
pub static STRING_CONSTRUCTOR_S_INFO: ClassInfo = ClassInfo {
    class_name: "Function",
    parent_class: Some(&JS_FUNCTION_S_INFO),
    static_prop_hash_table: Some(&STRING_CONSTRUCTOR_TABLE),
    inherits_js_type_range: None,
};

/// `stringConstructorTableValues` de `StringConstructor.lut.h`, na ordem do `@begin`.
static STRING_CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 3] = [
    native_entry_with_intrinsic("fromCharCode", string_from_char_code, 1, Intrinsic::FromCharCodeIntrinsic),
    native_entry_with_intrinsic("fromCodePoint", string_from_code_point, 1, Intrinsic::FromCodePointIntrinsic),
    native_entry_with_intrinsic("raw", string_raw, 1, Intrinsic::NoIntrinsic),
];

/// `stringConstructorTable`.
static STRING_CONSTRUCTOR_TABLE: HashTable = HashTable { class_for_this: None, values: &STRING_CONSTRUCTOR_TABLE_VALUES };

/// `value.toString(globalObject)` com o `RETURN_IF_EXCEPTION` (`None` com a exceção pendente: um `Symbol`,
/// um `toString` que lança). Um `StringObject` também passa pelo `toPrimitive`, como no C++.
fn to_js_string(global_object: &JSGlobalObject, value: &JSValue) -> Option<crate::runtime::js_string::JSStringRef> {
    to_string_value(global_object, *value).ok()
}

/// `stringFromCharCode`: um `toUInt32` por argumento, e o primeiro que lança interrompe os seguintes.
fn string_from_char_code(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let vm = global_object.vm();
    let mut units: Vec<u16> = Vec::new();
    for argument in call_frame.arguments_span() {
        units.push(argument.to_uint32() as u16);
        if vm.exception().is_some() {
            return JSValue::empty().encode();
        }
    }
    JSValue::from_js_string(js_string(vm, &string_from_units(&units))).encode()
}

/// `stringFromCodePoint`: cada argumento é convertido (`toNumber`) e conferido antes do seguinte, então o
/// `valueOf` de um argumento depois de um inválido não roda.
fn string_from_code_point(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let vm = global_object.vm();
    let mut units: Vec<u16> = Vec::new();
    for argument in call_frame.arguments_span() {
        let value = argument.to_number();
        if vm.exception().is_some() {
            return JSValue::empty().encode();
        }
        if let Err(error) = algo::push_code_point(&mut units, value) {
            throw_string_op_error(global_object, error);
            return JSValue::empty().encode();
        }
    }
    JSValue::from_js_string(js_string(vm, &string_from_units(&units))).encode()
}

/// `stringRaw` (https://tc39.es/ecma262/#sec-string.raw): o segmento `index`, a substituição `index` (só se o
/// segmento seguinte existe) e o próximo segmento, convertidos nessa ordem, como o laço do C++.
fn string_raw(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let vm = global_object.vm();
    let template_value = call_frame.argument(0);
    if template_value.is_undefined_or_null() {
        return throw_vm_type_error(global_object, Some("String.raw requires template not be null or undefined"));
    }
    let Some(cooked) = template_value.to_object(global_object) else {
        return JSValue::empty().encode();
    };
    let raw_value = cooked.get(global_object, &PropertyName::from_identifier(&vm.property_names.raw));
    if vm.exception().is_some() {
        return JSValue::empty().encode();
    }
    if raw_value.is_undefined_or_null() {
        return throw_vm_type_error(global_object, Some("String.raw requires template.raw not be null or undefined"));
    }
    let Some(raw) = raw_value.to_object(global_object) else {
        return JSValue::empty().encode();
    };
    let length_name = PropertyName::from_identifier(&vm.property_names.length);
    let length_value = raw.get(global_object, &length_name);
    if vm.exception().is_some() {
        return JSValue::empty().encode();
    }
    let Some(raw) = raw.for_property_lookup(global_object, &length_name) else {
        return JSValue::empty().encode();
    };
    let Ok(literal_count) = length_value.to_length_checked() else {
        return JSValue::empty().encode();
    };
    if literal_count == 0 {
        return JSValue::from_js_string(js_empty_string(vm)).encode();
    }
    // O C++ indexa por `uint64_t` e o `RopeBuilder` recusa o que passa de `JSString::MaxLength`
    // (`throwOutOfMemoryError`); o índice por nome (acima de `uint32_t`) não existe neste porte.
    let Ok(literal_count) = u32::try_from(literal_count) else {
        throw_string_op_error(global_object, StringOpError::OutOfMemory);
        return JSValue::empty().encode();
    };

    let arguments = call_frame.arguments_span();
    let substitution_count = arguments.len().saturating_sub(1);
    let mut parts: Vec<WtfString> = Vec::new();
    for index in 0..literal_count {
        let segment = raw.get_by_index(vm, index);
        if vm.exception().is_some() {
            return JSValue::empty().encode();
        }
        let Ok(segment_string) = to_wtf_string_value(global_object, segment) else {
            return JSValue::empty().encode();
        };
        parts.push(segment_string);

        if index + 1 == literal_count {
            break;
        }
        if (index as usize) < substitution_count {
            let Ok(substitution) = to_wtf_string_value(global_object, arguments[index as usize + 1]) else {
                return JSValue::empty().encode();
            };
            parts.push(substitution);
        }
    }
    match concat(&parts) {
        Ok(string) => JSValue::from_js_string(js_string(vm, &string)).encode(),
        Err(error) => {
            throw_string_op_error(global_object, error);
            JSValue::empty().encode()
        }
    }
}

/// `callStringConstructor`: `String(value)` (o `Symbol` vira a descrição `Symbol(...)`).
fn call_string_constructor(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let vm = global_object.vm();
    if call_frame.argument_count() == 0 {
        return JSValue::from_js_string(js_empty_string(vm)).encode();
    }
    let argument = call_frame.unchecked_argument(0);
    if argument.is_cell() {
        if let Some(symbol) = Symbol::from_cell_id(argument.as_cell()) {
            return match symbol.to_string(vm) {
                Some(string) => JSValue::from_js_string(string).encode(),
                None => JSValue::empty().encode(),
            };
        }
    }
    match to_js_string(global_object, &argument) {
        Some(string) => JSValue::from_js_string(string).encode(),
        None => JSValue::empty().encode(),
    }
}

/// `constructWithStringConstructor`: `new String(value)`.
fn construct_with_string_constructor(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let vm = global_object.vm();
    let structure = match get_derived_structure_in_realm(
        global_object,
        call_frame.this_value(),
        call_frame.js_callee(),
        |realm| realm.string_object_structure(),
    ) {
        Ok(structure) => structure,
        Err(thrown) => {
            throw_thrown(global_object, thrown);
            return JSValue::empty().encode();
        }
    };
    if call_frame.argument_count() == 0 {
        return StringObject::create(vm, structure).as_value().encode();
    }
    match to_js_string(global_object, &call_frame.unchecked_argument(0)) {
        Some(string) => StringObject::create_with_string(vm, structure, string).as_value().encode(),
        None => JSValue::empty().encode(),
    }
}

/// `class StringConstructor : public JSFunction`: espaço de nomes de `create`.
pub struct StringConstructor;

impl StringConstructor {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSFunction::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::JSFunctionType, StringConstructor::STRUCTURE_FLAGS),
            &STRING_CONSTRUCTOR_S_INFO,
        )
    }

    /// `create(vm, structure, stringPrototype)` e `finishCreation`: `length` 1, `name` "String",
    /// `prototype` (`ReadOnly|DontEnum|DontDelete`) e a tabela `stringConstructorTable`. Liga também o
    /// `constructor` do protótipo (`DontEnum`), o passo que o `init` do global faz depois.
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        string_prototype: &StringObjectRef,
    ) -> JSFunctionRef {
        // No JSC atual `StringConstructor` é um `JSFunction` sobre `NativeExecutable` com o
        // `StringConstructorIntrinsic` (como o `NumberConstructor`): `length` e `name` são reificados
        // preguiçosamente e saem de `getOwnSpecialPropertyNames`, antes dos nomes da tabela.
        let constructor = JSFunction::create_native_with_structure(
            vm,
            global_object,
            structure,
            1,
            &WtfString::from_latin1(b"String"),
            call_string_constructor,
            ImplementationVisibility::Public,
            Intrinsic::StringConstructorIntrinsic,
            construct_with_string_constructor,
        );
        // As entradas de `STRING_CONSTRUCTOR_TABLE` (lut) não nascem aqui: reificam no primeiro acesso.
        constructor.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.prototype),
            string_prototype.as_value(),
            READ_ONLY | DONT_ENUM | DONT_DELETE,
        );

        string_prototype.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&vm.property_names.constructor),
            constructor.as_value(),
            DONT_ENUM,
        );
        // `[Symbol.iterator]` por último, depois do `constructor` (ordem do bun).
        string_prototype.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&vm.property_names.iterator_symbol),
            global_object.string_proto_symbol_iterator_function(),
            DONT_ENUM,
        );
        constructor
    }
}
