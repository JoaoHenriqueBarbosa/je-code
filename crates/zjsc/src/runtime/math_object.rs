//! Porte de `runtime/MathObject.h` e `MathObject.cpp`: o objeto `Math`, um `JSNonFinalObject` sem
//! campos próprios (registrado como `CellEntry::Object`), as oito constantes, o `@@toStringTag` e as
//! funções nativas.
//!
//! DIVERGÊNCIAS e lacunas:
//!
//! - `sinh`, `cosh`, `tanh`, `expm1`, `log1p`, `cbrt`, `log10`, `asinh`, `acosh` e `atanh` vêm do porte
//!   do glibc 2.41 em `glibc_hyper.rs`. O `hypot` de dois argumentos vem de `glibc_hypot.rs` (`e_hypot.c`).
//! - `hypot` com três argumentos reproduz o `std::hypot(x, y, z)` do libc++ recente (escala pelo maior
//!   valor), o ramo `#else` do C++; o ramo de `_LIBCPP_VERSION < 200100` (`hypotl`) não existe.
//! - `Math.sumPrecise` usa o `PreciseSum` de `wtf/precise_sum.rs` (um acumulador exato único no lugar de
//!   `XsumSmall`/`XsumLarge`) e o `forEachInIterable` genérico de `iterator_operations.rs`.
//! - `Math.random` usa o `WeakRandom` do global (`weakRandomNumber()`), semeado por
//!   `cryptographically_random_number` (ver `wtf/weak_random.rs`); `Options::forceWeakRandomSeed` não
//!   existe.
//! - As conversões `toNumber`/`toInt32`/`toUInt32` de `JSValue` ainda não lançam e não cobrem objeto
//!   (ver `js_value_conversions.rs`); quando cobrirem, os corpos abaixo passam a propagar o erro.

use crate::runtime::class_info::ClassInfo;
use crate::runtime::glibc_hyper;
use crate::runtime::glibc_hypot;
use ul_common::libm;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::for_each_in_iterable;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_number, js_number_i32, JSValue};
use crate::runtime::math_common::{self, math};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::precise_sum::PreciseSum;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo MathObject::s_info`.
pub static MATH_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "Math", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// As operações puras do `Math` que têm mais que uma chamada da biblioteca padrão.
pub mod operations {
    use super::math_common::{js_round, math};

    /// `mathProtoFuncHypot`, depois da conversão dos argumentos com `toNumber`.
    pub fn hypot(arguments: &[f64]) -> f64 {
        match *arguments {
            [] => 0.0,
            [arg0] => arg0.abs(),
            [arg0, arg1] => {
                if arg0.is_infinite() || arg1.is_infinite() {
                    return f64::INFINITY;
                }
                super::glibc_hypot::hypot(arg0, arg1)
            }
            [arg0, arg1, arg2] => {
                if arg0.is_infinite() || arg1.is_infinite() || arg2.is_infinite() {
                    return f64::INFINITY;
                }
                if arg0.is_nan() || arg1.is_nan() || arg2.is_nan() {
                    return f64::NAN;
                }
                // `std::hypot(x, y, z)`: escala pelo maior valor absoluto.
                let max = math::f_max(arg0.abs(), math::f_max(arg1.abs(), arg2.abs()));
                if max == 0.0 {
                    return 0.0;
                }
                let (x, y, z) = (arg0 / max, arg1 / max, arg2 / max);
                max * (x * x + y * y + z * z).sqrt()
            }
            _ => hypot_many(arguments),
        }
    }

    /// O laço para quatro argumentos ou mais.
    fn hypot_many(arguments: &[f64]) -> f64 {
        let mut has_infinity = false;
        let mut has_nan = false;
        let mut max_abs = 0.0f64;
        let mut sum = 0.0f64;
        for &argument in arguments {
            if argument.is_infinite() {
                has_infinity = true;
                continue;
            }
            if argument.is_nan() {
                has_nan = true;
                continue;
            }

            let abs_argument = argument.abs();
            if max_abs < abs_argument {
                let scaled_argument = max_abs / abs_argument;
                sum = sum.mul_add(scaled_argument * scaled_argument, 1.0);
                max_abs = abs_argument;
            } else if max_abs != 0.0 {
                let scaled_argument = abs_argument / max_abs;
                sum = scaled_argument.mul_add(scaled_argument, sum);
            }
        }

        if has_infinity {
            return f64::INFINITY;
        }
        if has_nan {
            return f64::NAN;
        }
        // when maxAbs is 0, that means all numbers are 0s. Thus, early return with 0.0.
        if max_abs == 0.0 {
            return 0.0;
        }

        sum.sqrt() * max_abs
    }

    /// `mathProtoFuncSign`.
    pub fn sign(arg: f64) -> f64 {
        if arg.is_nan() {
            return f64::NAN;
        }
        if arg == 0.0 {
            return arg;
        }
        if arg.is_sign_negative() { -1.0 } else { 1.0 }
    }

    /// `Float16 { value }` convertido de volta para `double` (`mathProtoFuncF16Round`): o arredondamento
    /// do `double` para o binary16 (5 bits de expoente, 10 de mantissa) pelo vizinho mais próximo, com
    /// empate para o par; `|x| >= 65520` (o meio entre o maior finito, 65504, e o próximo) vira infinito.
    pub fn f16_round(value: f64) -> f64 {
        if value.is_nan() {
            return f64::NAN;
        }
        if value.is_infinite() || value == 0.0 {
            return value;
        }

        let magnitude = value.abs();
        if magnitude >= 65520.0 {
            return f64::INFINITY.copysign(value);
        }

        // O menor expoente normal do binary16 é -14; abaixo dele o espaçamento fica em 2^-24.
        let exponent = (((magnitude.to_bits() >> 52) & 0x7ff) as i32 - 1023).max(-14);
        let ulp = f64::from_bits(((exponent - 10 + 1023) as u64) << 52);
        ((magnitude / ulp).round_ties_even() * ulp).copysign(value)
    }
}

/// Define o `JSC_DEFINE_HOST_FUNCTION` `$host` cujo corpo é `$body` (`fn(&JSGlobalObject, &HostCall)`):
/// o corpo recebe a `HostCall` em `$call`.
macro_rules! math_function {
    ($host:ident, $body:ident, |$call:ident| $result:expr) => {
        fn $body(_global_object: &JSGlobalObject, $call: &HostCall) -> HostResult {
            $result
        }
        crate::host_function!(pub $host, $body);
    };
}

/// `mathProtoFuncX` de um argumento: `toNumber`, a operação sobre `double` e o `jsDoubleNumber`
/// (`double_number`) ou `jsNumber` (`js_number`) do resultado.
macro_rules! unary_math_function {
    ($host:ident, $body:ident, $make:path, $operation:expr) => {
        math_function!($host, $body, |call| Ok($make(($operation)(call.argument(0).to_number()))));
    };
}

unary_math_function!(math_proto_func_abs, abs_body, js_number, |x: f64| x.abs());
unary_math_function!(math_proto_func_acos, acos_body, JSValue::double_number, super::glibc_asin::acos);
unary_math_function!(math_proto_func_asin, asin_body, JSValue::double_number, super::glibc_asin::asin);
unary_math_function!(math_proto_func_atan, atan_body, JSValue::double_number, super::glibc_atan::atan);
unary_math_function!(math_proto_func_acosh, acosh_body, JSValue::double_number, glibc_hyper::acosh);
unary_math_function!(math_proto_func_asinh, asinh_body, JSValue::double_number, glibc_hyper::asinh);
unary_math_function!(math_proto_func_atanh, atanh_body, JSValue::double_number, glibc_hyper::atanh);
unary_math_function!(math_proto_func_cbrt, cbrt_body, JSValue::double_number, glibc_hyper::cbrt);
unary_math_function!(math_proto_func_ceil, ceil_body, js_number, f64::ceil);
unary_math_function!(math_proto_func_cos, cos_body, JSValue::double_number, super::glibc_trig::cos);
unary_math_function!(math_proto_func_cosh, cosh_body, JSValue::double_number, glibc_hyper::cosh);
unary_math_function!(math_proto_func_exp, exp_body, JSValue::double_number, libm::exp);
unary_math_function!(math_proto_func_expm1, expm1_body, JSValue::double_number, glibc_hyper::expm1);
unary_math_function!(math_proto_func_floor, floor_body, js_number, f64::floor);
unary_math_function!(math_proto_func_fround, fround_body, JSValue::double_number, |x: f64| x as f32 as f64);
unary_math_function!(math_proto_func_log, log_body, JSValue::double_number, libm::log);
unary_math_function!(math_proto_func_log10, log10_body, JSValue::double_number, glibc_hyper::log10);
unary_math_function!(math_proto_func_log1p, log1p_body, JSValue::double_number, glibc_hyper::log1p);
unary_math_function!(math_proto_func_log2, log2_body, JSValue::double_number, libm::log2);
unary_math_function!(math_proto_func_round, round_body, js_number, math_common::js_round);
unary_math_function!(math_proto_func_sign, sign_body, js_number, operations::sign);
unary_math_function!(math_proto_func_sin, sin_body, JSValue::double_number, super::glibc_trig::sin);
unary_math_function!(math_proto_func_sinh, sinh_body, JSValue::double_number, glibc_hyper::sinh);
unary_math_function!(math_proto_func_sqrt, sqrt_body, JSValue::double_number, f64::sqrt);
unary_math_function!(math_proto_func_tan, tan_body, JSValue::double_number, super::glibc_tan::tan);
unary_math_function!(math_proto_func_tanh, tanh_body, JSValue::double_number, glibc_hyper::tanh);
// `toIntegerPreserveNaN`: `trunc(toNumber(x))`, com `-0` preservado.
unary_math_function!(math_proto_func_trunc, trunc_body, js_number, f64::trunc);
unary_math_function!(math_proto_func_f16_round, f16_round_body, JSValue::double_number, operations::f16_round);

math_function!(math_proto_func_atan2, atan2_body, |call| {
    let arg0 = call.argument(0).to_number();
    let arg1 = call.argument(1).to_number();
    Ok(JSValue::double_number(super::glibc_atan::atan2(arg0, arg1)))
});

math_function!(math_proto_func_clz32, clz32_body, |call| {
    let value = call.argument(0).to_uint32();
    Ok(js_number_i32(value.leading_zeros() as i32))
});

math_function!(math_proto_func_hypot, hypot_body, |call| {
    let arguments: Vec<f64> = (0..call.argument_count()).map(|index| call.argument(index).to_number()).collect();
    Ok(JSValue::double_number(operations::hypot(&arguments)))
});

math_function!(math_proto_func_max, max_body, |call| {
    if call.argument_count() == 0 {
        return Ok(js_number(f64::NEG_INFINITY));
    }
    let mut result = call.argument(0).to_number();
    for index in 1..call.argument_count() {
        result = math::js_max_double(result, call.argument(index).to_number());
    }
    Ok(js_number(result))
});

math_function!(math_proto_func_min, min_body, |call| {
    if call.argument_count() == 0 {
        return Ok(js_number(f64::INFINITY));
    }
    let mut result = call.argument(0).to_number();
    for index in 1..call.argument_count() {
        result = math::js_min_double(result, call.argument(index).to_number());
    }
    Ok(js_number(result))
});

// ECMA 15.8.2.1.13
math_function!(math_proto_func_pow, pow_body, |call| {
    let arg = call.argument(0).to_number();
    let arg2 = call.argument(1).to_number();
    Ok(js_number(math_common::operation_math_pow(arg, arg2)))
});

math_function!(math_proto_func_imul, imul_body, |call| {
    let left = call.argument(0).to_int32();
    let right = call.argument(1).to_int32();
    Ok(js_number_i32(left.wrapping_mul(right)))
});

fn random_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(JSValue::double_number(global_object.weak_random_number()))
}
crate::host_function!(math_proto_func_random, random_body);

/// `mathProtoFuncSumPrecise`: o `PreciseSum` sobre o iterável. O C++ escolhe `XsumSmall` ou `XsumLarge`
/// pelo comprimento do `JSArray`; o acumulador do porte é um só (ver `wtf/precise_sum.rs`).
fn sum_precise_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let iterable = call.argument(0);
    if iterable.is_undefined_or_null() {
        return Err(Thrown::type_error("Math.sumPrecise requires first argument not be null or undefined"));
    }

    let mut sum = PreciseSum::new();
    let mut count: u64 = 0;
    for_each_in_iterable(global_object, iterable, |value| {
        if count >= math_common::max_safe_integer_as_uint64() {
            return Err(Thrown::range_error("Math.sumPrecise exceeded maximum iterations"));
        }
        if !value.is_number() {
            return Err(Thrown::type_error("Math.sumPrecise was passed a non-number"));
        }
        sum.add(value.as_number());
        count += 1;
        Ok(())
    })?;
    Ok(js_number(sum.compute()))
}
crate::host_function!(math_proto_func_sum_precise, sum_precise_body);

/// `class MathObject final : public JSNonFinalObject`.
pub struct MathObject;

impl MathObject {
    /// `StructureFlags = Base::StructureFlags` (nenhuma flag).
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, MathObject::STRUCTURE_FLAGS),
            &MATH_OBJECT_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: o `MathObject(vm, structure)` e o `finishCreation`. O objeto
    /// é registrado como `CellEntry::Object`, porque a classe não tem campos próprios.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let object = JSObject::allocate(vm, structure);
        MathObject::finish_creation(&object, vm, global_object);
        object
    }

    /// `finishCreation(vm, globalObject)`.
    fn finish_creation(object: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        object.finish_creation(vm);

        let constant_attributes = DONT_DELETE | DONT_ENUM | READ_ONLY;
        let exp_one = libm::exp(1.0);
        let constants: [(&str, f64); 8] = [
            ("E", exp_one),
            ("LN2", libm::log(2.0)),
            ("LN10", libm::log(10.0)),
            ("LOG2E", libm::log2(exp_one)),
            ("LOG10E", glibc_hyper::log10(exp_one)),
            ("PI", std::f64::consts::PI),
            ("SQRT1_2", 0.5f64.sqrt()),
            ("SQRT2", 2.0f64.sqrt()),
        ];
        for (name, value) in constants {
            let identifier = Identifier::from_string(vm, &WtfString::from_latin1(name.as_bytes()));
            object.put_direct(vm, &PropertyName::from_identifier(&identifier), js_number(value), constant_attributes);
        }
        // JSC_TO_STRING_TAG_WITHOUT_TRANSITION()
        let class_name = js_string(vm, &WtfString::from_latin1(MATH_OBJECT_S_INFO.class_name.as_bytes()));
        object.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol),
            JSValue::from_js_string(class_name),
            DONT_ENUM | READ_ONLY,
        );

        let functions: [(&str, u32, NativeFunction, Intrinsic); 37] = [
            ("abs", 1, math_proto_func_abs, Intrinsic::AbsIntrinsic),
            ("acos", 1, math_proto_func_acos, Intrinsic::ACosIntrinsic),
            ("asin", 1, math_proto_func_asin, Intrinsic::ASinIntrinsic),
            ("atan", 1, math_proto_func_atan, Intrinsic::ATanIntrinsic),
            ("acosh", 1, math_proto_func_acosh, Intrinsic::ACoshIntrinsic),
            ("asinh", 1, math_proto_func_asinh, Intrinsic::ASinhIntrinsic),
            ("atanh", 1, math_proto_func_atanh, Intrinsic::ATanhIntrinsic),
            ("atan2", 2, math_proto_func_atan2, Intrinsic::NoIntrinsic),
            ("cbrt", 1, math_proto_func_cbrt, Intrinsic::CbrtIntrinsic),
            ("ceil", 1, math_proto_func_ceil, Intrinsic::CeilIntrinsic),
            ("clz32", 1, math_proto_func_clz32, Intrinsic::Clz32Intrinsic),
            ("cos", 1, math_proto_func_cos, Intrinsic::CosIntrinsic),
            ("cosh", 1, math_proto_func_cosh, Intrinsic::CoshIntrinsic),
            ("exp", 1, math_proto_func_exp, Intrinsic::ExpIntrinsic),
            ("expm1", 1, math_proto_func_expm1, Intrinsic::Expm1Intrinsic),
            ("floor", 1, math_proto_func_floor, Intrinsic::FloorIntrinsic),
            ("fround", 1, math_proto_func_fround, Intrinsic::FRoundIntrinsic),
            ("hypot", 2, math_proto_func_hypot, Intrinsic::NoIntrinsic),
            ("log", 1, math_proto_func_log, Intrinsic::LogIntrinsic),
            ("log10", 1, math_proto_func_log10, Intrinsic::Log10Intrinsic),
            ("log1p", 1, math_proto_func_log1p, Intrinsic::Log1pIntrinsic),
            ("log2", 1, math_proto_func_log2, Intrinsic::Log2Intrinsic),
            ("max", 2, math_proto_func_max, Intrinsic::MaxIntrinsic),
            ("min", 2, math_proto_func_min, Intrinsic::MinIntrinsic),
            ("pow", 2, math_proto_func_pow, Intrinsic::PowIntrinsic),
            ("random", 0, math_proto_func_random, Intrinsic::RandomIntrinsic),
            ("round", 1, math_proto_func_round, Intrinsic::RoundIntrinsic),
            ("sign", 1, math_proto_func_sign, Intrinsic::NoIntrinsic),
            ("sin", 1, math_proto_func_sin, Intrinsic::SinIntrinsic),
            ("sinh", 1, math_proto_func_sinh, Intrinsic::SinhIntrinsic),
            ("sqrt", 1, math_proto_func_sqrt, Intrinsic::SqrtIntrinsic),
            ("tan", 1, math_proto_func_tan, Intrinsic::TanIntrinsic),
            ("tanh", 1, math_proto_func_tanh, Intrinsic::TanhIntrinsic),
            ("trunc", 1, math_proto_func_trunc, Intrinsic::TruncIntrinsic),
            ("imul", 2, math_proto_func_imul, Intrinsic::IMulIntrinsic),
            ("f16round", 1, math_proto_func_f16_round, Intrinsic::F16RoundIntrinsic),
            ("sumPrecise", 1, math_proto_func_sum_precise, Intrinsic::NoIntrinsic),
        ];
        for (name, length, function, intrinsic) in functions {
            let identifier = Identifier::from_string(vm, &WtfString::from_latin1(name.as_bytes()));
            put_direct_native_function_without_transition(
                vm,
                global_object,
                object,
                &identifier,
                length,
                function,
                ImplementationVisibility::Public,
                intrinsic,
                DONT_ENUM,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::operations::{f16_round, hypot, sign};
    use crate::runtime::math_common::js_round;

    #[test]
    fn hypot_by_argument_count() {
        assert_eq!(hypot(&[]), 0.0);
        assert_eq!(hypot(&[-3.0]), 3.0);
        assert_eq!(hypot(&[3.0, 4.0]), 5.0);
        assert_eq!(hypot(&[f64::NAN, f64::INFINITY]), f64::INFINITY);
        assert!((hypot(&[2.0, 3.0, 6.0]) - 7.0).abs() < 1e-12);
        assert!(hypot(&[1.0, f64::NAN, 2.0]).is_nan());
        assert_eq!(hypot(&[0.0, 0.0, 0.0]), 0.0);
        assert_eq!(hypot(&[1.0, 1.0, 1.0, 1.0]), 2.0);
        assert!(hypot(&[1.0, 1.0, f64::NAN, 1.0]).is_nan());
        assert_eq!(hypot(&[1.0, f64::INFINITY, f64::NAN, 1.0]), f64::INFINITY);
        assert_eq!(hypot(&[0.0, 0.0, 0.0, 0.0]), 0.0);
        assert_eq!(hypot(&[1e300, 1e300, 1e300, 1e300]), 2e300);
    }

    #[test]
    fn sign_keeps_zero_and_nan() {
        assert!(sign(f64::NAN).is_nan());
        assert!(sign(-0.0).is_sign_negative());
        assert_eq!(sign(0.0), 0.0);
        assert_eq!(sign(-5.0), -1.0);
        assert_eq!(sign(7.5), 1.0);
        assert_eq!(sign(f64::NEG_INFINITY), -1.0);
    }

    #[test]
    fn round_is_round_half_up() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(0.49999999999999994), 0.0);
    }

    #[test]
    fn f16_round_cases() {
        assert_eq!(f16_round(1.0), 1.0);
        assert_eq!(f16_round(1.337), 1.3369140625);
        assert_eq!(f16_round(65504.0), 65504.0);
        assert_eq!(f16_round(65519.9), 65504.0);
        assert_eq!(f16_round(65520.0), f64::INFINITY);
        assert_eq!(f16_round(-65520.0), f64::NEG_INFINITY);
        assert_eq!(f16_round(5.960464477539063e-8), 5.960464477539063e-8);
        assert_eq!(f16_round(2.9802322387695312e-8), 0.0);
        assert!(f16_round(-1e-10).is_sign_negative());
        assert!(f16_round(f64::NAN).is_nan());
        assert!(f16_round(-0.0).is_sign_negative());
        // empate para o par: 2049 fica entre 2048 e 2050 (espaçamento 2 acima de 2048).
        assert_eq!(f16_round(2049.0), 2048.0);
        assert_eq!(f16_round(2051.0), 2052.0);
    }

    #[test]
    fn f16_round_ties_and_subnormals() {
        // Menor normal do binary16 (2^-14) e o maior subnormal (1023 * 2^-24) são exatos.
        assert_eq!(f16_round(6.103515625e-5), 6.103515625e-5);
        assert_eq!(f16_round(6.097555160522461e-5), 6.097555160522461e-5);
        // Empates no meio de dois binary16 vizinhos: vai para a mantissa par.
        assert_eq!(f16_round(1.00048828125), 1.0);
        assert_eq!(f16_round(1.00146484375), 1.001953125);
        assert_eq!(f16_round(1.0009765625), 1.0009765625);
        // O vizinho acima do maior subnormal é o menor normal; o empate 1023.5 * 2^-24 vai para o par (1024).
        assert_eq!(f16_round(1023.5 * 5.960464477539063e-8), 6.103515625e-5);
        assert_eq!(f16_round(0.1), 0.0999755859375);
        assert_eq!(f16_round(-65519.0), -65504.0);
        assert_eq!(f16_round(f64::INFINITY), f64::INFINITY);
        assert_eq!(f16_round(f64::NEG_INFINITY), f64::NEG_INFINITY);
    }

    #[test]
    fn round_keeps_negative_zero_and_halves() {
        assert!(js_round(-0.5_f64).is_sign_negative());
        assert!(js_round(-0.2_f64).is_sign_negative());
        assert_eq!(js_round(0.5), 1.0);
        assert_eq!(js_round(1.5), 2.0);
        assert_eq!(js_round(-1.5), -1.0);
        assert_eq!(js_round(-2.6), -3.0);
        assert!(js_round(f64::NAN).is_nan());
        assert_eq!(js_round(f64::INFINITY), f64::INFINITY);
        assert_eq!(js_round(4503599627370497.0), 4503599627370497.0);
    }

    #[test]
    fn hypot_three_arguments_does_not_overflow() {
        let big = hypot(&[3e200, 4e200, 0.0]);
        assert!(big.is_finite() && ((big - 5e200) / 5e200).abs() < 1e-15, "{big}");
        assert_eq!(hypot(&[f64::NEG_INFINITY, f64::NAN, 1.0]), f64::INFINITY);
        assert_eq!(hypot(&[-0.0, -0.0]), 0.0);
        assert!(hypot(&[f64::NAN, 1.0]).is_nan());
    }
}
