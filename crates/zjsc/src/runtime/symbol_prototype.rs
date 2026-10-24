//! Porte de `runtime/SymbolPrototype.h`, `SymbolPrototypeInlines.h` e `SymbolPrototype.cpp`: o
//! `Symbol.prototype` (um `JSNonFinalObject` comum, "not one of the symbol wrapper object instance").
//!
//! A tabela estática (`symbolPrototypeTable`: `description` `DontEnum|ReadOnly|CustomAccessor`, `toString`,
//! `valueOf`) fica no `ClassInfo` e a `Structure` leva `HasStaticPropertyTable`: as três reificam no primeiro acesso.
//! O `Integrity::auditStructureID` do C++ é só auditoria de memória e não existe aqui.

use crate::custom_getter;
use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::{js_undefined, JSValue};
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{custom_getter_entry, native_entry_with_intrinsic};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{CUSTOM_ACCESSOR, DONT_ENUM, FUNCTION, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::symbol::{Symbol, SymbolRef};
use crate::runtime::symbol_object::SymbolObject;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo SymbolPrototype::s_info`.
pub static SYMBOL_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo {
        class_name: "Symbol",
        parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
        static_prop_hash_table: Some(&SYMBOL_PROTOTYPE_TABLE),
        inherits_js_type_range: None,
    };

const SYMBOL_DESCRIPTION_TYPE_ERROR: &str = "Symbol.prototype.description requires that |this| be a symbol or a symbol object";
const SYMBOL_TO_STRING_TYPE_ERROR: &str = "Symbol.prototype.toString requires that |this| be a symbol or a symbol object";
const SYMBOL_VALUE_OF_TYPE_ERROR: &str = "Symbol.prototype.valueOf requires that |this| be a symbol or a symbol object";

/// `tryExtractSymbol(thisValue)`: o símbolo primitivo ou o valor interno de um `SymbolObject`.
pub fn try_extract_symbol(this_value: JSValue) -> Option<SymbolRef> {
    if let JSValue::Cell(cell_id) = this_value {
        if let Some(symbol) = Symbol::from_cell_id(cell_id) {
            return Some(symbol);
        }
    }
    SymbolObject::from_value(&this_value).map(|object| SymbolObject::internal_value(&object))
}

/// `symbolProtoGetterDescription`.
fn symbol_proto_description(global_object: &JSGlobalObject, this_value: JSValue, _property_name: &PropertyName) -> HostResult {
    let symbol = try_extract_symbol(this_value).ok_or_else(|| Thrown::type_error(SYMBOL_DESCRIPTION_TYPE_ERROR))?;
    Ok(match symbol.description(global_object.vm()) {
        Some(string) => JSValue::from_js_string(string),
        None => js_undefined(),
    })
}

custom_getter!(symbol_proto_getter_description, symbol_proto_description);

/// `symbolProtoFuncToString`.
fn symbol_proto_to_string(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let symbol = try_extract_symbol(call.this_value()).ok_or_else(|| Thrown::type_error(SYMBOL_TO_STRING_TYPE_ERROR))?;
    let string = symbol.to_string(global_object.vm()).ok_or(Thrown::OutOfMemory)?;
    Ok(JSValue::from_js_string(string))
}

/// `symbolProtoFuncValueOf` (também o `[Symbol.toPrimitive]`).
fn symbol_proto_value_of(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let symbol = try_extract_symbol(call.this_value()).ok_or_else(|| Thrown::type_error(SYMBOL_VALUE_OF_TYPE_ERROR))?;
    Ok(symbol.to_primitive())
}

host_function!(symbol_proto_func_to_string, symbol_proto_to_string);
host_function!(symbol_proto_func_value_of, symbol_proto_value_of);

/// `symbolPrototypeTableValues` de `SymbolPrototype.lut.h`, na ordem do `@begin`.
static SYMBOL_PROTOTYPE_TABLE_VALUES: [HashTableValue; 3] = [
    custom_getter_entry("description", symbol_proto_getter_description),
    native_entry_with_intrinsic("toString", symbol_proto_func_to_string, 0, Intrinsic::SymbolPrototypeToStringIntrinsic),
    native_entry_with_intrinsic("valueOf", symbol_proto_func_value_of, 0, Intrinsic::NoIntrinsic),
];

/// `symbolPrototypeTable`.
static SYMBOL_PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &SYMBOL_PROTOTYPE_TABLE_VALUES };

/// `class SymbolPrototype final : public JSNonFinalObject`.
pub struct SymbolPrototype;

impl SymbolPrototype {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)` (`SymbolPrototypeInlines.h`).
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, SymbolPrototype::STRUCTURE_FLAGS),
            &SYMBOL_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `SymbolPrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        SymbolPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`: a ordem de propriedades é a que o bun mede (`description`, `toString`,
    /// `valueOf`, `constructor`, `[Symbol.toPrimitive]`, `[Symbol.toStringTag]`): aqui entram as da tabela estática;
    /// `constructor` e depois `install_symbol_keyed_properties` vêm do `init` do global.
    fn finish_creation(prototype: &JSObject, vm: &VM, _global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);
        prototype.structure().set_may_be_prototype(true);
        // `description`, `toString` e `valueOf` (`symbolPrototypeTable`) não nascem aqui: reificam no primeiro acesso.
    }

    /// As propriedades com chave Symbol (`[Symbol.toPrimitive]` e `[Symbol.toStringTag]`), que no bun vêm depois
    /// de `constructor`: o `init` do global chama isto logo após gravar `constructor`.
    pub fn install_symbol_keyed_properties(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        let to_primitive_function = JSFunction::create_native(
            vm,
            global_object,
            1,
            &WtfString::from_latin1(b"[Symbol.toPrimitive]"),
            symbol_proto_func_value_of,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            call_host_function_as_constructor,
        );
        prototype.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.to_primitive_symbol),
            to_primitive_function.as_value(),
            DONT_ENUM | READ_ONLY,
        );
        // `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
        prototype.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol),
            JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(SYMBOL_PROTOTYPE_S_INFO.class_name.as_bytes()))),
            DONT_ENUM | READ_ONLY,
        );
    }
}
