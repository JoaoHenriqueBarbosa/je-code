//! Porte de `runtime/SymbolConstructor.h`, `SymbolConstructorInlines.h` e `SymbolConstructor.cpp`: o
//! construtor `Symbol` (um `InternalFunction`), com `Symbol(description)`, `Symbol.for`, `Symbol.keyFor` e os
//! símbolos conhecidos (`Symbol.iterator`...).
//!
//! `symbolConstructorTable` (`for`, `keyFor`) é preguiçosa como no JSC (`SYMBOL_CONSTRUCTOR_S_INFO`, flag
//! `HasStaticPropertyTable`): as duas reificam no primeiro acesso. Eager, como no `finishCreation`: `length`,
//! `name`, `prototype` e os símbolos conhecidos.
//!
//! DIVERGÊNCIAS:
//! - `description.toString(globalObject)` usa o `JSValue::to_string` do porte (que chama `to_primitive` de objeto);
//!   a exceção pendente vira `Err(Thrown::Pending)` em `Symbol(desc)` e `Symbol.for(key)`.
//! - `constructSymbol` lança `createNotAConstructorError(callee)`; o callee vem do quadro nativo.

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::current_realm::has_pending_exception;
use crate::runtime::exception_helpers::create_not_a_constructor_error;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::internal_function::{InternalFunction, InternalFunctionRef, PropertyAdditionMode, INTERNAL_FUNCTION_S_INFO};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::{js_undefined, JSValue};
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{native_entry};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, FUNCTION, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::symbol::Symbol;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::vm::VM;
use crate::wtf::text::string_impl::StringImpl;

/// `const ClassInfo SymbolConstructor::s_info` (`"Function"`, `&symbolConstructorTable`).
pub static SYMBOL_CONSTRUCTOR_S_INFO: ClassInfo = ClassInfo {
    class_name: "Function",
    parent_class: Some(&INTERNAL_FUNCTION_S_INFO),
    static_prop_hash_table: Some(&SYMBOL_CONSTRUCTOR_TABLE),
    inherits_js_type_range: None,
};

/// `const ASCIILiteral SymbolKeyForTypeError`.
const SYMBOL_KEY_FOR_TYPE_ERROR: &str = "Symbol.keyFor requires that the first argument be a symbol";

/// `callSymbol`: `Symbol(description)`.
fn call_symbol_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let description = call.argument(0);
    if description.is_undefined() {
        return Ok(Symbol::create(vm).to_primitive());
    }

    let string = description.to_string(vm);
    if has_pending_exception() {
        return Err(Thrown::Pending);
    }
    let value = string.value();
    Ok(Symbol::create_with_description_and_string(vm, &value, string).to_primitive())
}

/// `constructSymbol`: `throwVMError(globalObject, scope, createNotAConstructorError(globalObject, callee))`.
fn construct_symbol_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let error = create_not_a_constructor_error(global_object, JSValue::from_cell(call.callee()));
    let mut scope = ThrowScope::new(global_object.vm());
    throw_exception(global_object, &mut scope, error);
    Err(Thrown::Pending)
}

/// `symbolConstructorFor`: `Symbol.for(key)`.
fn symbol_constructor_for_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let key_string = call.argument(0).to_string(vm);
    if has_pending_exception() {
        return Err(Thrown::Pending);
    }
    let key = key_string.value();
    let key_impl = key.impl_().cloned().unwrap_or_else(StringImpl::empty);
    let registered = vm.symbol_registry().symbol_for_key(&key_impl);
    Ok(Symbol::create_with_registered_uid(vm, &registered).to_primitive())
}

/// `symbolConstructorKeyFor`: `Symbol.keyFor(symbol)`.
fn symbol_constructor_key_for_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let symbol_value = call.argument(0);
    let symbol = match symbol_value {
        JSValue::Cell(cell_id) => Symbol::from_cell_id(cell_id),
        _ => None,
    }
    .ok_or_else(|| Thrown::type_error(SYMBOL_KEY_FOR_TYPE_ERROR))?;

    if symbol.uid().symbol_registry().is_none() {
        return Ok(js_undefined());
    }
    debug_assert!(!symbol.uid().is_null_symbol());
    Ok(match symbol.description(global_object.vm()) {
        Some(description) => JSValue::from_js_string(description),
        None => js_undefined(),
    })
}

host_function!(call_symbol, call_symbol_body);
host_function!(pub construct_symbol, construct_symbol_body);
host_function!(symbol_constructor_for, symbol_constructor_for_body);
host_function!(symbol_constructor_key_for, symbol_constructor_key_for_body);

/// `symbolConstructorTableValues` de `SymbolConstructor.lut.h`, na ordem do `@begin`.
static SYMBOL_CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 2] = [
    native_entry("for", symbol_constructor_for, 1),
    native_entry("keyFor", symbol_constructor_key_for, 1),
];

/// `symbolConstructorTable`.
static SYMBOL_CONSTRUCTOR_TABLE: HashTable = HashTable { class_for_this: None, values: &SYMBOL_CONSTRUCTOR_TABLE_VALUES };

/// `class SymbolConstructor final : public InternalFunction`: sem campos próprios, é o `InternalFunction`.
pub struct SymbolConstructor;

impl SymbolConstructor {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = InternalFunction::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)` (`SymbolConstructorInlines.h`).
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::InternalFunctionType, SymbolConstructor::STRUCTURE_FLAGS),
            &SYMBOL_CONSTRUCTOR_S_INFO,
        )
    }

    /// `create(vm, structure, prototype)`: `SymbolConstructor(vm, structure)` e `finishCreation(vm, prototype)`.
    pub fn create(vm: &VM, _global_object: &JSGlobalObject, structure: StructureRef, prototype: &JSObject) -> InternalFunctionRef {
        let constructor = InternalFunction::new(vm, structure, call_symbol, Some(construct_symbol));
        // `for` e `keyFor` (`symbolConstructorTable`) não nascem aqui: reificam no primeiro acesso.
        constructor.finish_creation(vm, 0, vm.property_names.symbol.string().string(), PropertyAdditionMode::WithoutStructureTransition);
        constructor.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.prototype),
            prototype.as_value(),
            DONT_ENUM | DONT_DELETE | READ_ONLY,
        );

        // `JSC_COMMON_PRIVATE_IDENTIFIERS_EACH_WELL_KNOWN_SYMBOL(INITIALIZE_WELL_KNOWN_SYMBOLS)`.
        let names = &vm.property_names;
        let well_known_symbols: [(&[u8], &Identifier); 15] = [
            (b"hasInstance", &names.has_instance_symbol),
            (b"isConcatSpreadable", &names.is_concat_spreadable_symbol),
            (b"asyncIterator", &names.async_iterator_symbol),
            (b"iterator", &names.iterator_symbol),
            (b"match", &names.match_symbol),
            (b"matchAll", &names.match_all_symbol),
            (b"replace", &names.replace_symbol),
            (b"search", &names.search_symbol),
            (b"species", &names.species_symbol),
            (b"split", &names.split_symbol),
            (b"toPrimitive", &names.to_primitive_symbol),
            (b"toStringTag", &names.to_string_tag_symbol),
            (b"unscopables", &names.unscopables_symbol),
            (b"dispose", &names.dispose_symbol),
            (b"asyncDispose", &names.async_dispose_symbol),
        ];
        for (name, symbol_identifier) in well_known_symbols {
            let key = symbol_identifier.impl_().expect("símbolo conhecido sem StringImpl");
            constructor.put_direct(
                vm,
                &PropertyName::from_identifier(&Identifier::from_span(vm, name)),
                Symbol::for_key(vm, &key).to_primitive(),
                DONT_ENUM | DONT_DELETE | READ_ONLY,
            );
        }

        constructor
    }
}
