//! Porte de `runtime/BigIntConstructor.h`, `BigIntConstructorInlines.h` e `BigIntConstructor.cpp`: o
//! construtor `BigInt` (no C++ um `InternalFunction` com `call` e `construct` próprios), `asUintN`,
//! `asIntN` e, com `Options::useBigIntMathMethods()`, `abs`, `cbrt`, `max`, `min`, `pow`, `sign` e
//! `sqrt`.
//!
//! DIVERGÊNCIAS e lacunas:
//!
//! - A tabela estática (`bigIntConstructorTable`: `asUintN`, `asIntN`, `HasStaticPropertyTable`) é preguiçosa
//!   como no JSC: reifica no primeiro acesso (`lookup.rs`), e `Reflect.ownKeys` a lista antes de `length`.
//! - Sem `BigInt32` (`USE(BIGINT32)` é 0): todo BigInt é célula, e as ramificações `isBigInt32()` não
//!   existem.
//! - `constructBigIntConstructor` lança `createNotAConstructorError(callee)`, o mesmo de
//!   `callHostFunctionAsConstructor`, que é o `construct` do `NativeExecutable`.
//! - A exceção de uma conversão fica pendente no `VM` e a função devolve `Thrown::Pending`.

use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_big_int::{BigIntError, ComparisonResult, ImplResult, JSBigInt};
use crate::runtime::js_big_int_ops::{
    big_int_of, big_int_unary_op, compare_big_int, impl_result_value, make_big_int_from_double, make_big_int_from_i64,
    to_big_int,
};
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::internal_function::{InternalFunction, InternalFunctionRef, PropertyAdditionMode, INTERNAL_FUNCTION_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{native_entry};
use crate::runtime::structure::Structure;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::math_common::is_integer;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::object_to_primitive::PreferredPrimitiveType;
use crate::runtime::operations_bitwise::js_pow;
use crate::runtime::options_list::Options;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, FUNCTION, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// O valor de um resultado de conversão ou de operação: o `JSValue` vazio é a exceção pendente no `VM`.
fn or_pending(value: JSValue) -> HostResult {
    if value.is_empty() {
        return Err(Thrown::Pending);
    }
    Ok(value)
}

// The `BigInt(value)` call.
fn call_big_int_constructor_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = call.argument(0);
    let primitive = or_pending(value.to_primitive_preferred(PreferredPrimitiveType::PreferNumber))?;

    if primitive.is_int32() {
        return or_pending(make_big_int_from_i64(i64::from(primitive.as_int32())));
    }

    if primitive.is_double() {
        let number = primitive.as_double();
        if !is_integer(number) {
            // A mensagem do V8 traz o número (medido no Chromium 154).
            if crate::runtime::stack_frame::v8_stack_format() {
                return Err(Thrown::range_error(&format!(
                    "The number {} cannot be converted to a BigInt because it is not an integer",
                    crate::runtime::v8_stack::number_text(number)
                )));
            }
            return Err(Thrown::range_error("Not an integer"));
        }
        return or_pending(make_big_int_from_double(number));
    }

    or_pending(to_big_int(primitive))
}

/// O `bigInt` e o `numberOfBits` de `asUintN` e `asIntN`, e a conta feita por `operation`.
fn as_n_bits(
    global_object: &JSGlobalObject,
    call: &HostCall,
    operation: fn(u64, &JSBigInt) -> Result<ImplResult, BigIntError>,
) -> HostResult {
    let number_of_bits = call.argument(0).to_index("number of bits")?;
    // `RETURN_IF_EXCEPTION(scope, { })`: o `toIntegerOrInfinity` do argumento deixa a exceção pendente
    // (um `Symbol`, o `valueOf` que lança) e o `toBigInt` do segundo argumento não pode rodar.
    if global_object.vm().exception().is_some() {
        return Err(Thrown::Pending);
    }

    let big_int = or_pending(to_big_int(call.argument(1)))?;

    debug_assert!(big_int.is_big_int());
    let big_int = big_int_of(big_int).expect("toBigInt devolveu um valor que não é BigInt");
    or_pending(impl_result_value(operation(number_of_bits, &big_int)))
}

fn big_int_constructor_func_as_uint_n(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    as_n_bits(global_object, call, JSBigInt::as_uint_n)
}

fn big_int_constructor_func_as_int_n(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    as_n_bits(global_object, call, JSBigInt::as_int_n)
}

// https://tc39.es/proposal-bigint-math/#sec-bigint.abs
fn big_int_constructor_abs(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let big_int_value = call.argument(0);
    let Some(big_int) = big_int_of(big_int_value) else {
        return Err(Thrown::type_error("BigInt.abs requires the argument to be a BigInt"));
    };

    if big_int.sign() {
        return or_pending(big_int_unary_op(big_int_value, JSBigInt::unary_minus));
    }
    Ok(big_int_value)
}

// https://tc39.es/proposal-bigint-math/#sec-bigint.cbrt
fn big_int_constructor_cbrt(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let big_int_value = call.argument(0);
    if !big_int_value.is_big_int() {
        return Err(Thrown::type_error("BigInt.cbrt requires the argument to be a BigInt"));
    }

    or_pending(big_int_unary_op(big_int_value, JSBigInt::cbrt))
}

/// `BigInt.max` e `BigInt.min`: o argumento que `replaces` diz ser o novo resultado quando comparado ao
/// atual. `name` é o nome do método na mensagem do `TypeError`.
fn big_int_constructor_extremum(call: &HostCall, name: &str, replaces: ComparisonResult) -> HostResult {
    let error_message = format!("BigInt.{name} requires every argument to be a BigInt");
    let mut result = call.argument(0);
    if !result.is_big_int() {
        return Err(Thrown::type_error(&error_message));
    }

    for index in 1..call.argument_count() {
        let value = call.argument(index);
        if !value.is_big_int() {
            return Err(Thrown::type_error(&error_message));
        }

        if compare_big_int(value, result) == replaces {
            result = value;
        }
    }

    Ok(result)
}

// https://tc39.es/proposal-bigint-math/#sec-bigint.max
fn big_int_constructor_max(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    big_int_constructor_extremum(call, "max", ComparisonResult::GreaterThan)
}

// https://tc39.es/proposal-bigint-math/#sec-bigint.min
fn big_int_constructor_min(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    big_int_constructor_extremum(call, "min", ComparisonResult::LessThan)
}

// https://tc39.es/proposal-bigint-math/#sec-bigint.pow
fn big_int_constructor_pow(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let base = call.argument(0);
    if !base.is_big_int() {
        return Err(Thrown::type_error("BigInt.pow requires the first argument to be a BigInt"));
    }

    let exponent = call.argument(1);
    if !exponent.is_big_int() {
        return Err(Thrown::type_error("BigInt.pow requires the second argument to be a BigInt"));
    }

    or_pending(js_pow(base, exponent))
}

// https://tc39.es/proposal-bigint-math/#sec-bigint.sign
fn big_int_constructor_sign(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let big_int_value = call.argument(0);
    let Some(big_int) = big_int_of(big_int_value) else {
        return Err(Thrown::type_error("BigInt.sign requires the argument to be a BigInt"));
    };

    if big_int.is_zero() {
        return Ok(big_int_value);
    }
    or_pending(make_big_int_from_i64(if big_int.sign() { -1 } else { 1 }))
}

// https://tc39.es/proposal-bigint-math/#sec-bigint.sqrt
fn big_int_constructor_sqrt(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let big_int_value = call.argument(0);
    let Some(big_int) = big_int_of(big_int_value) else {
        return Err(Thrown::type_error("BigInt.sqrt requires the argument to be a positive BigInt"));
    };

    if big_int.sign() {
        return Err(Thrown::range_error("BigInt.sqrt requires the argument to be a positive BigInt"));
    }

    or_pending(big_int_unary_op(big_int_value, JSBigInt::sqrt))
}

crate::host_function!(call_big_int_constructor, call_big_int_constructor_body);
crate::host_function!(big_int_constructor_host_as_uint_n, big_int_constructor_func_as_uint_n);
crate::host_function!(big_int_constructor_host_as_int_n, big_int_constructor_func_as_int_n);
crate::host_function!(big_int_constructor_host_abs, big_int_constructor_abs);
crate::host_function!(big_int_constructor_host_cbrt, big_int_constructor_cbrt);
crate::host_function!(big_int_constructor_host_max, big_int_constructor_max);
crate::host_function!(big_int_constructor_host_min, big_int_constructor_min);
crate::host_function!(big_int_constructor_host_pow, big_int_constructor_pow);
crate::host_function!(big_int_constructor_host_sign, big_int_constructor_sign);
crate::host_function!(big_int_constructor_host_sqrt, big_int_constructor_sqrt);

/// `const ClassInfo BigIntConstructor::s_info`.
pub static BIG_INT_CONSTRUCTOR_S_INFO: ClassInfo = ClassInfo {
    class_name: "Function",
    parent_class: Some(&INTERNAL_FUNCTION_S_INFO),
    static_prop_hash_table: Some(&BIG_INT_CONSTRUCTOR_TABLE),
    inherits_js_type_range: None,
};

/// `bigIntConstructorTableValues` de `BigIntConstructor.lut.h`, na ordem do `@begin`.
static BIG_INT_CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 2] = [
    native_entry("asUintN", big_int_constructor_host_as_uint_n, 2),
    native_entry("asIntN", big_int_constructor_host_as_int_n, 2),
];

/// `bigIntConstructorTable`.
static BIG_INT_CONSTRUCTOR_TABLE: HashTable = HashTable { class_for_this: None, values: &BIG_INT_CONSTRUCTOR_TABLE_VALUES };

/// `class BigIntConstructor final : public InternalFunction`: espaço de nomes de `create`.
pub struct BigIntConstructor;

impl BigIntConstructor {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = InternalFunction::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `create(vm, structure, bigIntPrototype)`: o `NativeExecutable` com `callBigIntConstructor` e o
    /// `construct` que lança (comprimento 1, nome `BigInt`) e o `finishCreation`.
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        function_prototype: JSValue,
        big_int_prototype: &JSObject,
    ) -> InternalFunctionRef {
        // `BigIntConstructor::createStructure`: o `ClassInfo` do `InternalFunction`, como no `Array`.
        let structure = Structure::create(
            vm,
            Some(global_object),
            function_prototype,
            TypeInfo::new(JSType::InternalFunctionType, BigIntConstructor::STRUCTURE_FLAGS),
            &BIG_INT_CONSTRUCTOR_S_INFO,
        );
        // `constructBigIntConstructor` é `createNotAConstructorError(callee)`, o mesmo corpo de `constructSymbol`; com
        // `construct` presente o `BigInt` é um construtor para `isConstructor` (`Reflect.construct(BigInt, [1], Object)`
        // lança "function is not a constructor" na chamada, não "requires the first argument be a constructor").
        let constructor = InternalFunction::new(
            vm,
            structure,
            call_big_int_constructor,
            Some(crate::runtime::symbol_constructor::construct_symbol),
        );
        BigIntConstructor::finish_creation(&constructor, vm, global_object, big_int_prototype);
        constructor
    }

    /// `finishCreation(vm, bigIntPrototype)`: `length`, `name`, `prototype` e, com a opção, os métodos de Math.
    /// `asUintN` e `asIntN` (`bigIntConstructorTable`) não nascem aqui: reificam no primeiro acesso, e
    /// `Reflect.ownKeys` os lista antes de `length` porque os nomes da tabela vêm primeiro.
    fn finish_creation(constructor: &InternalFunction, vm: &VM, global_object: &JSGlobalObject, big_int_prototype: &JSObject) {
        // Com `Options::useBigIntMathMethods()`, na ordem do `finishCreation`.
        let math: [(&str, u32, NativeFunction); 7] = [
            ("abs", 1, big_int_constructor_host_abs),
            ("cbrt", 1, big_int_constructor_host_cbrt),
            ("max", 2, big_int_constructor_host_max),
            ("min", 2, big_int_constructor_host_min),
            ("pow", 2, big_int_constructor_host_pow),
            ("sign", 1, big_int_constructor_host_sign),
            ("sqrt", 1, big_int_constructor_host_sqrt),
        ];
        let math_methods: &[(&str, u32, NativeFunction)] = if Options::use_big_int_math_methods() { &math } else { &[] };
        constructor.finish_creation(vm, 1, &WtfString::from_latin1(b"BigInt"), PropertyAdditionMode::WithoutStructureTransition);
        constructor.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.prototype),
            big_int_prototype.as_value(),
            DONT_ENUM | DONT_DELETE | READ_ONLY,
        );
        for (name, length, function) in math_methods {
            put_direct_native_function_without_transition(
                vm,
                global_object,
                constructor,
                &Identifier::from_span(vm, name.as_bytes()),
                *length,
                *function,
                ImplementationVisibility::Public,
                Intrinsic::NoIntrinsic,
                DONT_ENUM,
            );
        }
    }
}
