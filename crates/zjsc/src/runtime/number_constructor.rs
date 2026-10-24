//! Porte de `runtime/NumberConstructor.h`, `NumberConstructorInlines.h` e `NumberConstructor.cpp`: o
//! construtor `Number` (no C++ um `JSFunction` sobre um `NativeExecutable` com `call` e `construct`
//! próprios), as constantes e `isInteger`, `isFinite`, `isNaN` e `isSafeInteger`.
//!
//! DIVERGÊNCIAS e lacunas:
//!
//! - A tabela estática (`numberConstructorTable`: `isFinite`, `isNaN`, `isSafeInteger`) é preguiçosa, como no
//!   JSC (`NUMBER_CONSTRUCTOR_S_INFO`, flag `HasStaticPropertyTable`). Eager, como no `finishCreation`:
//!   `prototype`, as constantes, `parseInt`, `parseFloat` e `isInteger`.
//! - `JSBigInt::toNumber(numeric)` é `JSBigInt::to_number` (`toNumberHeap`, sem `BigInt32`).
//! - `parseInt` e `parseFloat` são lidos do global (`parse_int_function()`), que `add_global_functions`
//!   já tem de ter criado.

use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::js_big_int::JSBigInt;
use crate::runtime::js_function::{put_direct_native_function_without_transition, JSFunction, JSFunctionRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::{js_boolean, js_number, js_number_i32, JSValue};
use crate::runtime::js_wrapper_object::JSWrapperObject;
use crate::runtime::math_common::{is_integer, is_safe_integer, max_safe_integer, min_safe_integer};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_function::JS_FUNCTION_S_INFO;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{native_entry_with_intrinsic};
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, FUNCTION, READ_ONLY};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::property_name::PropertyName;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `NumberConstructor::isIntegerImpl(value)`.
pub fn is_integer_impl(value: JSValue) -> bool {
    value.is_int32() || (value.is_double() && is_integer(value.as_double()))
}

// ECMA 15.7.1
fn construct_number_constructor_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let mut n = 0.0;
    if call.argument_count() > 0 {
        let numeric = call.argument(0).to_numeric();
        if numeric.is_empty() {
            return Err(Thrown::Pending);
        }
        n = if numeric.is_number() { numeric.as_number() } else { JSBigInt::to_number(numeric) };
    }

    let structure =
        get_derived_structure_in_realm(global_object, call.new_target(), call.callee(), |realm| realm.number_object_structure())?;

    let object = JSWrapperObject::create(vm, structure);
    object.set_internal_value(js_number(n));
    Ok(object.as_value())
}

// ECMA 15.7.2
fn call_number_constructor_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() == 0 {
        return Ok(js_number_i32(0));
    }
    let numeric = call.argument(0).to_numeric();
    if numeric.is_empty() {
        return Err(Thrown::Pending);
    }
    if numeric.is_number() {
        return Ok(numeric);
    }
    // `numeric.isBigInt()`: `jsNumber(JSBigInt::toNumber(numeric))`.
    Ok(js_number(JSBigInt::to_number(numeric)))
}

// ECMA-262 20.1.2.3
fn number_constructor_func_is_integer(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_boolean(is_integer_impl(call.argument(0))))
}

// ECMA-262 20.1.2.5
fn number_constructor_func_is_safe_integer(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    if argument.is_int32() {
        return Ok(js_boolean(true));
    }
    if !argument.is_double() {
        return Ok(js_boolean(false));
    }
    Ok(js_boolean(is_safe_integer(argument.as_double())))
}

fn number_constructor_func_is_nan(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    if !argument.is_number() {
        return Ok(js_boolean(false));
    }
    Ok(js_boolean(argument.as_number().is_nan()))
}

fn number_constructor_func_is_finite(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    if !argument.is_number() {
        return Ok(js_boolean(false));
    }
    Ok(js_boolean(argument.as_number().is_finite()))
}

crate::host_function!(construct_number_constructor, construct_number_constructor_body);
crate::host_function!(call_number_constructor, call_number_constructor_body);
crate::host_function!(number_constructor_host_is_integer, number_constructor_func_is_integer);
crate::host_function!(number_constructor_host_is_safe_integer, number_constructor_func_is_safe_integer);
crate::host_function!(number_constructor_host_is_nan, number_constructor_func_is_nan);
crate::host_function!(number_constructor_host_is_finite, number_constructor_func_is_finite);

/// `const ClassInfo NumberConstructor::s_info` (`"Function"`, base `JSFunction`, `&numberConstructorTable`).
pub static NUMBER_CONSTRUCTOR_S_INFO: ClassInfo = ClassInfo {
    class_name: "Function",
    parent_class: Some(&JS_FUNCTION_S_INFO),
    static_prop_hash_table: Some(&NUMBER_CONSTRUCTOR_TABLE),
    inherits_js_type_range: None,
};

/// `numberConstructorTableValues` de `NumberConstructor.lut.h`, na ordem do `@begin`.
static NUMBER_CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 3] = [
    native_entry_with_intrinsic("isFinite", number_constructor_host_is_finite, 1, Intrinsic::NumberIsFiniteIntrinsic),
    native_entry_with_intrinsic("isNaN", number_constructor_host_is_nan, 1, Intrinsic::NumberIsNaNIntrinsic),
    native_entry_with_intrinsic("isSafeInteger", number_constructor_host_is_safe_integer, 1, Intrinsic::NumberIsSafeIntegerIntrinsic),
];

/// `numberConstructorTable`.
static NUMBER_CONSTRUCTOR_TABLE: HashTable = HashTable { class_for_this: None, values: &NUMBER_CONSTRUCTOR_TABLE_VALUES };

/// `class NumberConstructor final : public JSFunction`: espaço de nomes de `create`.
pub struct NumberConstructor;

impl NumberConstructor {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSFunction::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::JSFunctionType, NumberConstructor::STRUCTURE_FLAGS),
            &NUMBER_CONSTRUCTOR_S_INFO,
        )
    }

    /// `create(vm, structure, numberPrototype)`: o `NativeExecutable` com `callNumberConstructor` e
    /// `constructNumberConstructor` (comprimento 1, nome `Number`) e o `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, number_prototype: &JSWrapperObject) -> JSFunctionRef {
        let structure = NumberConstructor::create_structure(vm, global_object, global_object.function_prototype().as_value());
        let constructor = JSFunction::create_native_with_structure(
            vm,
            global_object,
            structure,
            1,
            vm.property_names.number.string().string(),
            call_number_constructor,
            ImplementationVisibility::Public,
            Intrinsic::NumberConstructorIntrinsic,
            construct_number_constructor,
        );
        // As entradas de `NUMBER_CONSTRUCTOR_TABLE` (lut) não nascem aqui: reificam no primeiro acesso.
        NumberConstructor::finish_creation(&constructor, vm, global_object, number_prototype);
        constructor
    }

    /// `finishCreation(vm, numberPrototype)`.
    fn finish_creation(constructor: &JSFunction, vm: &VM, global_object: &JSGlobalObject, number_prototype: &JSWrapperObject) {
        let constant_attributes = DONT_DELETE | DONT_ENUM | READ_ONLY;

        constructor.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.prototype),
            number_prototype.as_value(),
            DONT_ENUM | DONT_DELETE | READ_ONLY,
        );

        let constants: [(&str, f64); 7] = [
            ("EPSILON", f64::EPSILON),
            ("MAX_VALUE", 1.7976931348623157E+308),
            ("MIN_VALUE", 5E-324),
            ("MAX_SAFE_INTEGER", max_safe_integer()),
            ("MIN_SAFE_INTEGER", min_safe_integer()),
            ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
            ("POSITIVE_INFINITY", f64::INFINITY),
        ];
        for (name, value) in constants {
            let identifier = Identifier::from_string(vm, &WtfString::from_latin1(name.as_bytes()));
            constructor.put_direct(vm, &PropertyName::from_identifier(&identifier), JSValue::double_number(value), constant_attributes);
        }
        constructor.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.na_n), JSValue::nan(), constant_attributes);

        // `Number.parseInt` e `Number.parseFloat` são as mesmas funções das globais.
        constructor.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.parse_int),
            global_object.parse_int_function(),
            DONT_ENUM,
        );
        constructor.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.parse_float),
            global_object.parse_float_function(),
            DONT_ENUM,
        );

        // isInteger (`JSC_NATIVE_INTRINSIC_FUNCTION_WITHOUT_TRANSITION`), por último como no bun.
        NumberConstructor::install_function(
            vm,
            global_object,
            constructor,
            "isInteger",
            number_constructor_host_is_integer,
            Intrinsic::NumberIsIntegerIntrinsic,
        );
    }

    /// Instala `isInteger` (`JSC_NATIVE_INTRINSIC_FUNCTION_WITHOUT_TRANSITION`, comprimento 1).
    fn install_function(
        vm: &VM,
        global_object: &JSGlobalObject,
        constructor: &JSFunction,
        name: &str,
        function: NativeFunction,
        intrinsic: Intrinsic,
    ) {
        let identifier = Identifier::from_string(vm, &WtfString::from_latin1(name.as_bytes()));
        put_direct_native_function_without_transition(
            vm,
            global_object,
            constructor,
            &identifier,
            1,
            function,
            ImplementationVisibility::Public,
            intrinsic,
            DONT_ENUM,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_checks() {
        assert!(is_integer_impl(JSValue::Int32(5)));
        assert!(is_integer_impl(JSValue::Double(1e300)));
        assert!(!is_integer_impl(JSValue::Double(1.5)));
        assert!(!is_integer_impl(JSValue::Double(f64::NAN)));
        assert!(!is_integer_impl(JSValue::Double(f64::INFINITY)));
        assert!(!is_integer_impl(JSValue::Undefined));
        assert!(!is_integer_impl(JSValue::Bool(true)));
    }
}
