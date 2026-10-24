//! Porte de `runtime/DateConstructor.h`, `DateConstructorInlines.h` e `DateConstructor.cpp`: o
//! construtor `Date`, `makeDay`/`makeDate`/`makeTime` (ECMA 262 21.4.1), e os algoritmos de
//! `new Date(...)`, `Date()`, `Date.parse`, `Date.UTC` e `Date.now`.
//!
//! DIVERGÊNCIAS (a criação do construtor e as cascas nativas ficam em `date_constructor_natives.rs`):
//!
//! - `DateConstructor::create`/`finishCreation` (o `InternalFunction` com `callDate` e
//!   `constructWithDateConstructor`, `length` 7, `name` `Date`, e `prototype` com
//!   `DontEnum|DontDelete|ReadOnly`, sem transição) mora lá; aqui ficam `create_structure`, o `ClassInfo`
//!   e as constantes dessa criação.
//! - Cada `JSC_DEFINE_HOST_FUNCTION` vira uma casca fina:
//!   `callDate`: `format_date_time_now`;
//!   `dateParse`: `toWTFString(argument(0))` e `date_parse`;
//!   `dateNow`: `js_date_now(overridenDateNow)`, e `DateNowIntrinsic` na tabela (`DATE_CONSTRUCTOR_TABLE`,
//!   em `date_constructor_natives.rs`);
//!   `dateUTC`: `milliseconds_from_components(..., TimeType::UTCTime)`;
//!   `constructWithDateConstructor`: `construct_date`, que devolve o valor de tempo, e a casca cria o
//!   `DateInstance` (`DateInstance::create`, que faz o `timeClip`) com a `Structure` do `newTarget`
//!   (`JSC_GET_DERIVED_STRUCTURE`) ou `globalObject->dateStructure()`.
//! - A conversão dos argumentos (`toNumber`, `toPrimitive`, que podem chamar código do usuário) entra
//!   como `convert(i)`: `args.at(i).toNumber(globalObject)`, com `undefined` (NaN) para o argumento
//!   ausente (`Date.UTC()` sem argumentos lê o `argument(0)`). `construct_date` com um argumento recebe
//!   `OneArgument`, o que a casca já resolveu: um `DateInstance` (o valor interno dele), uma string
//!   primitiva (vai ao parser) ou o número do `toNumber` do primitivo.
//! - `jsDateNow()` do `JSGlobalObject` envolve o `overridenDateNow` do Bun: `js_date_now` recebe esse
//!   valor (NaN é "sem substituição").
//! - O `DateCache` vem por parâmetro (o C++ o pega do `VM`, que ainda não tem o campo `dateCache`).

use crate::runtime::class_info::ClassInfo;
use crate::runtime::date_prototype::{format_date_time, DateTimeFormat};
use crate::runtime::date_constructor_natives::DATE_CONSTRUCTOR_TABLE;
use crate::runtime::internal_function::{InternalFunction, INTERNAL_FUNCTION_S_INFO};
use crate::runtime::js_date_math::{DateCache, UseSharedCache};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::PutError;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::JSValue;
use crate::runtime::math_common::to_int32;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::date_math::{
    date_to_days_from_1970, js_current_time, time_clip, TimeType, MS_PER_DAY, MS_PER_HOUR, MS_PER_MINUTE, MS_PER_SECOND,
};
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo DateConstructor::s_info` (`"Function"`, base `InternalFunction`, `&dateConstructorTable`).
/// A tabela mora em `date_constructor_natives.rs`, junto das funções nativas que ela referencia.
pub static DATE_CONSTRUCTOR_S_INFO: ClassInfo = ClassInfo {
    class_name: "Function",
    parent_class: Some(&INTERNAL_FUNCTION_S_INFO),
    static_prop_hash_table: Some(&DATE_CONSTRUCTOR_TABLE),
    inherits_js_type_range: None,
};

/// O `length` do construtor (`finishCreation(vm, 7, ...)`).
pub const DATE_CONSTRUCTOR_LENGTH: u32 = 7;

/// O nome do construtor (`vm.propertyNames->Date`).
pub const DATE_CONSTRUCTOR_NAME: &str = "Date";

/// Os atributos de `prototype` no `finishCreation`: `DontEnum | DontDelete | ReadOnly`.
pub const PROTOTYPE_ATTRIBUTES: u32 = DONT_ENUM | DONT_DELETE | READ_ONLY;

/// `class DateConstructor final : public InternalFunction`.
pub struct DateConstructor;

impl DateConstructor {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = InternalFunction::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)` (DateConstructorInlines.h).
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::InternalFunctionType, DateConstructor::STRUCTURE_FLAGS),
            &DATE_CONSTRUCTOR_S_INFO,
        )
    }
}

// ---------------------------------------------------------------------------------------------
// makeDay, makeDate, makeTime
// ---------------------------------------------------------------------------------------------

/// `makeDay(year, month, date)`: https://tc39.es/ecma262/#sec-makeday
pub fn make_day(year: f64, month: f64, date: f64) -> f64 {
    let additional_years = (month / 12.0).floor();
    let ym = year + additional_years;
    if !ym.is_finite() {
        return f64::NAN;
    }
    let mm = month - additional_years * 12.0;
    let year_int32 = to_int32(ym);
    let month_int32 = to_int32(mm);
    if year_int32 as f64 != ym || month_int32 as f64 != mm {
        return f64::NAN;
    }
    let days = date_to_days_from_1970(year_int32, month_int32, 1);
    days + date - 1.0
}

/// `makeDate(day, time)`: https://tc39.es/ecma262/#sec-makedate. As operações de ponto flutuante não
/// são fundidas (o C++ desliga o `FP_CONTRACT`; o Rust nunca funde).
pub fn make_date(day: f64, time: f64) -> f64 {
    (day * MS_PER_DAY) + time
}

/// `makeTime(hour, min, sec, ms)`: https://tc39.es/ecma262/#sec-maketime
pub fn make_time(hour: f64, min: f64, sec: f64, ms: f64) -> f64 {
    (((hour * MS_PER_HOUR) + min * MS_PER_MINUTE) + sec * MS_PER_SECOND) + ms
}

// ---------------------------------------------------------------------------------------------
// Algoritmos
// ---------------------------------------------------------------------------------------------

/// `millisecondsFromComponents(globalObject, args, timeType)`: `new Date(y, m, ...)` (hora local) e
/// `Date.UTC(...)`. `convert(i)` é o `toNumber` do argumento `i` (`undefined` se ausente).
pub fn milliseconds_from_components<E>(
    cache: &DateCache,
    argument_count: usize,
    time_type: TimeType,
    convert: &mut dyn FnMut(usize) -> Result<f64, E>,
) -> Result<f64, E> {
    // Initialize doubleArguments with default values.
    let mut double_arguments = [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    let mut has_non_finite = false;
    let number_of_used_arguments = argument_count.min(7).max(1);
    for i in 0..number_of_used_arguments {
        double_arguments[i] = convert(i)?;

        has_non_finite |= !double_arguments[i].is_finite();
        double_arguments[i] = (double_arguments[i] + 0.0).trunc();
    }

    if has_non_finite {
        return Ok(f64::NAN);
    }

    if (0.0..=99.0).contains(&double_arguments[0]) {
        double_arguments[0] += 1900.0;
    }

    let time = make_date(
        make_day(double_arguments[0], double_arguments[1], double_arguments[2]),
        make_time(double_arguments[3], double_arguments[4], double_arguments[5], double_arguments[6]),
    );
    Ok(time_clip(cache.local_time_to_ms(time, time_type)))
}

/// O único argumento de `new Date(value)` depois de resolvido pela casca.
#[derive(Clone, Debug)]
pub enum OneArgument {
    /// `dynamicDowncast<DateInstance>(arg0)`: o `internalNumber()` dele.
    DateInstance(f64),
    /// `toPrimitive` deu uma string: vai ao parser.
    String(WtfString),
    /// `toPrimitive` não deu string: o `toNumber` do primitivo.
    Number(f64),
}

/// O valor de tempo de `constructDate` (ECMA 15.9.3): o `DateInstance::create` aplica o `timeClip`.
/// `now` é o `jsDateNow()`; `one_argument` vale quando `argument_count == 1`; `convert` serve ao
/// caso de dois ou mais argumentos.
pub fn construct_date<E: From<PutError>>(
    cache: &DateCache,
    argument_count: usize,
    now: f64,
    one_argument: Option<OneArgument>,
    convert: &mut dyn FnMut(usize) -> Result<f64, E>,
) -> Result<f64, E> {
    if argument_count == 0 {
        // new Date() ECMA 15.9.3.3
        return Ok(now);
    }
    if argument_count == 1 {
        return Ok(match one_argument.expect("construct_date com um argumento exige OneArgument") {
            OneArgument::DateInstance(value) => value,
            OneArgument::String(string) => cache.parse_date(&string)?,
            OneArgument::Number(value) => value,
        });
    }
    milliseconds_from_components(cache, argument_count, TimeType::LocalTime, convert)
}

/// `callDate`: `Date()` sem `new`, o `toString()` do instante atual (`now`).
pub fn format_date_time_now(cache: &DateCache, now: f64) -> String {
    let ts = cache.ms_to_gregorian_date_time(now, TimeType::LocalTime, UseSharedCache::No);
    format_date_time(ts, DateTimeFormat::DateAndTime, false, cache)
}

/// `dateParse`: o `timeClip` do parse da string que a casca converteu com `toWTFString`.
pub fn date_parse(cache: &DateCache, date_str: &WtfString) -> Result<f64, PutError> {
    Ok(time_clip(cache.parse_date(date_str)?))
}

/// `JSGlobalObject::jsDateNow()`: o `overridenDateNow` do `bun test` (NaN é "sem substituição") ou o
/// relógio. Todo leitor do tempo atual passa por aqui.
pub fn js_date_now(overriden_date_now: f64) -> f64 {
    if overriden_date_now.is_nan() { js_current_time() } else { overriden_date_now }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_date_math::UtcTimeZone;

    fn cache() -> DateCache {
        DateCache::new(Box::new(UtcTimeZone))
    }

    fn components(args: &[f64], time_type: TimeType) -> f64 {
        let args = args.to_vec();
        let mut convert = |i: usize| -> Result<f64, ()> { Ok(args.get(i).copied().unwrap_or(f64::NAN)) };
        milliseconds_from_components(&cache(), args.len(), time_type, &mut convert).unwrap()
    }

    #[test]
    fn date_utc_components() {
        assert_eq!(components(&[1970.0, 0.0, 1.0], TimeType::UTCTime), 0.0);
        assert_eq!(components(&[70.0, 0.0], TimeType::UTCTime), 0.0);
        assert_eq!(components(&[2000.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], TimeType::UTCTime), 946_684_800_001.0);
        assert!(components(&[], TimeType::UTCTime).is_nan());
        assert!(components(&[f64::NAN], TimeType::UTCTime).is_nan());
        assert_eq!(components(&[1970.0, 12.0], TimeType::UTCTime), 31_536_000_000.0);
    }

    #[test]
    fn date_parse_clips() {
        let cache = cache();
        assert_eq!(date_parse(&cache, &WtfString::from_latin1(b"2000-01-01T00:00:00.001Z")).unwrap(), 946_684_800_001.0);
        assert!(date_parse(&cache, &WtfString::from_latin1(b"+275761-01-01T00:00:00Z")).unwrap().is_nan());
        assert!(date_parse(&cache, &WtfString::from_latin1(b"nope")).unwrap().is_nan());
    }

    #[test]
    fn call_date_formats() {
        assert_eq!(format_date_time_now(&cache(), 0.0), "Thu Jan 01 1970 00:00:00 GMT+0000 (Coordinated Universal Time)");
    }
}
