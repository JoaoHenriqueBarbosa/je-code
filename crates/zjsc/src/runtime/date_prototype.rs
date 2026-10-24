//! Porte de `runtime/DatePrototype.h`, `DatePrototypeInlines.h` e `DatePrototype.cpp`, mais a
//! `formatDateTime` de `DateConversion.cpp`: o `Date.prototype`, os algoritmos de cada método (getters,
//! setters, `toString` e família, `toISOString`) escritos sobre o `DateInstance`, e a tabela estática
//! de `datePrototypeTable`.
//!
//! DIVERGÊNCIAS (as funções nativas e o `finishCreation` ficam em `date_prototype_natives.rs`):
//!
//! - Cada `JSC_DEFINE_HOST_FUNCTION` é uma casca de três passos, lá: `this_date_instance(thisValue)` (o
//!   `TypeError` de `this` que não é `Date` vem antes de qualquer conversão de argumento), a conversão
//!   dos argumentos e a chamada da função daqui. Nos setters a conversão entra por um parâmetro
//!   `convert(i)` (`argument(i).toIntegerPreserveNaN(globalObject)`, que pode chamar `valueOf` do
//!   usuário e lançar): o C++ lê o estado do `Date` antes de converter, e um `valueOf` que mexe no
//!   próprio `Date` precisa enxergar essa ordem. Para `applyToNumberToOtherwiseIgnoredArguments` o mesmo
//!   `convert` serve (o `toNumber` tem o mesmo efeito, e o resultado é descartado).
//! - `DateError` é o que as funções lançam: `TypeError` (`throwVMTypeError`, mensagem `Type error` por
//!   padrão) e `RangeError` (`toISOString` com data inválida, `createRangeError(globalObject, "Invalid
//!   Date")`). Os setters são genéricos no erro `E` do `convert`, e a casca mistura `DateError` e o
//!   erro dela.
//! - `[Symbol.toPrimitive]` e `toJSON` chamam código do usuário e moram em `date_prototype_natives.rs`
//!   (sobre `object_to_primitive.rs`); `to_json_returns_null` é a parte pura do `toJSON`.
//!   `toLocale*String` passam pelo `intl_date_time_format.rs`; `toTemporalInstant` (fora da tabela, só com
//!   `Options::useTemporal()`) mora em `date_prototype_natives.rs`.

use crate::runtime::class_info::ClassInfo;
use crate::runtime::date_prototype_natives::DATE_PROTOTYPE_TABLE;
use crate::runtime::js_date_math::{DateCache, PlainGregorianDateTime, UseSharedCache};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::date_instance::{DateInstance, DateInstanceRef};
use crate::runtime::js_value::JSValue;
use crate::runtime::math_common::to_int32;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::date_math::{
    ms_to_year, time_clip, TimeType, MAX_ECMASCRIPT_TIME, MONTH_NAME, MS_PER_DAY, MS_PER_HOUR, MS_PER_MINUTE, MS_PER_SECOND, WEEKDAY_NAME,
};

/// `const ClassInfo DatePrototype::s_info` (`"Object"`, base `JSNonFinalObject`, com a tabela estática).
pub static DATE_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo {
        class_name: "Object",
        parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
        static_prop_hash_table: Some(&DATE_PROTOTYPE_TABLE),
        inherits_js_type_range: None,
    };

/// O que as funções de `Date.prototype` lançam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateError {
    /// `throwVMTypeError(globalObject, scope)`: a mensagem padrão é `Type error`.
    TypeError(&'static str),
    /// `throwVMError(globalObject, scope, createRangeError(globalObject, message))`.
    RangeError(&'static str),
}

/// A mensagem padrão de `throwVMTypeError(globalObject, scope)`.
pub const DEFAULT_TYPE_ERROR_MESSAGE: &str = "Type error";

/// `class DatePrototype final : public JSNonFinalObject`.
pub struct DatePrototype;

impl DatePrototype {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)` (DatePrototypeInlines.h).
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::ObjectType, DatePrototype::STRUCTURE_FLAGS),
            &DATE_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: o construtor e o `finishCreation` da base (as funções
    /// nativas esperam a ligação).
    pub fn create(vm: &VM, _global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        prototype.finish_creation(vm);
        prototype
    }
}

// ---------------------------------------------------------------------------------------------
// A tabela estática
// ---------------------------------------------------------------------------------------------

/// `datePrototypeTable` mora em `date_prototype_natives.rs` (referencia as cascas nativas) e é apontada
/// por `DATE_PROTOTYPE_S_INFO`.

// ---------------------------------------------------------------------------------------------
// DateConversion: formatDateTime
// ---------------------------------------------------------------------------------------------

/// `enum class DateTimeFormat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateTimeFormat {
    Date = 1,
    Time = 2,
    DateAndTime = 3,
}

/// `appendNumber<width>(builder, value)`: o sinal, se negativo, e o módulo com zeros à esquerda.
fn append_number(builder: &mut String, width: usize, value: i32) {
    let mut value = value;
    if value < 0 {
        builder.push('-');
        value = value.wrapping_neg();
    }
    let digits = value.to_string();
    for _ in digits.len()..width {
        builder.push('0');
    }
    builder.push_str(&digits);
}

/// `formatDateTime(t, format, asUTCVariant, dateCache)`.
pub fn format_date_time(t: PlainGregorianDateTime, format: DateTimeFormat, as_utc_variant: bool, date_cache: &DateCache) -> String {
    let append_date = (format as i32) & (DateTimeFormat::Date as i32) != 0;
    let append_time = (format as i32) & (DateTimeFormat::Time as i32) != 0;

    let mut builder = String::new();

    if append_date {
        builder.push_str(WEEKDAY_NAME[((t.week_day() + 6) % 7) as usize]);

        if as_utc_variant {
            builder.push_str(", ");
            append_number(&mut builder, 2, t.month_day());
            builder.push(' ');
            builder.push_str(MONTH_NAME[t.month() as usize]);
        } else {
            builder.push(' ');
            builder.push_str(MONTH_NAME[t.month() as usize]);
            builder.push(' ');
            append_number(&mut builder, 2, t.month_day());
        }
        builder.push(' ');
        append_number(&mut builder, 4, t.year());
    }

    if append_date && append_time {
        builder.push(' ');
    }

    if append_time {
        append_number(&mut builder, 2, t.hour());
        builder.push(':');
        append_number(&mut builder, 2, t.minute());
        builder.push(':');
        append_number(&mut builder, 2, t.second());
        builder.push_str(" GMT");

        if !as_utc_variant {
            let offset = t.utc_offset_in_minute().abs();
            builder.push(if t.utc_offset_in_minute() < 0 { '-' } else { '+' });
            append_number(&mut builder, 2, offset / 60);
            append_number(&mut builder, 2, offset % 60);
            let time_zone_name = date_cache.time_zone_display_name(t.is_dst());
            if !time_zone_name.is_empty() {
                builder.push_str(" (");
                builder.push_str(&time_zone_name);
                builder.push(')');
            }
        }
    }

    builder
}

// ---------------------------------------------------------------------------------------------
// Algoritmos
// ---------------------------------------------------------------------------------------------

/// `dynamicDowncast<DateInstance>(thisValue)` ou o `throwVMTypeError` que todas as funções fazem.
pub fn this_date_instance(this_value: JSValue) -> Result<DateInstanceRef, DateError> {
    DateInstance::from_value(&this_value).ok_or(DateError::TypeError(DEFAULT_TYPE_ERROR_MESSAGE))
}

/// `formateDateInstance(globalObject, callFrame, format, asUTCVariant)`: `toString`, `toUTCString`,
/// `toDateString` e `toTimeString`.
pub fn format_date_instance(this: &DateInstance, cache: &DateCache, format: DateTimeFormat, as_utc_variant: bool) -> String {
    let gregorian_date_time = if as_utc_variant { this.gregorian_date_time_utc(cache) } else { this.gregorian_date_time(cache) };
    if !gregorian_date_time.is_valid() {
        return "Invalid Date".to_string();
    }

    format_date_time(gregorian_date_time, format, as_utc_variant, cache)
}

/// `dateProtoFuncToISOString`.
pub fn date_proto_func_to_iso_string(this: &DateInstance, cache: &DateCache) -> Result<String, DateError> {
    if !this.internal_number().is_finite() {
        return Err(DateError::RangeError("Invalid Date"));
    }

    let gregorian_date_time = this.gregorian_date_time_utc(cache);
    if !gregorian_date_time.is_valid() {
        return Ok("Invalid Date".to_string());
    }

    // https://tc39.es/ecma262/#sec-date-time-string-format

    // If the year is outside the bounds of 0 and 9999 inclusive we want to use the extended year
    // format (ES 15.9.1.15.1).
    let mut ms = (this.internal_number() % MS_PER_SECOND) as i32;
    if ms < 0 {
        ms += MS_PER_SECOND as i32;
    }

    let mut year = gregorian_date_time.year();
    let month = gregorian_date_time.month() + 1;
    let day = gregorian_date_time.month_day();
    let hour = gregorian_date_time.hour();
    let minute = gregorian_date_time.minute();
    let second = gregorian_date_time.second();

    let mut prefix = "";
    let mut year_digits = 4;
    if !(0..=9999).contains(&year) {
        prefix = if year < 0 { "-" } else { "+" };
        year_digits = 6;
        year = year.abs();
    }

    Ok(format!("{prefix}{year:0year_digits$}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{ms:03}Z"))
}

/// A parte pura de `dateProtoFuncToJSON`: o `timeValue` numérico não finito devolve `null`.
pub fn to_json_returns_null(time_value: JSValue) -> bool {
    time_value.is_number() && !time_value.as_number().is_finite()
}

/// Qual campo de uma data desmembrada um getter lê.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateField {
    FullYear,
    Month,
    Date,
    Day,
    Hours,
    Minutes,
    Seconds,
}

/// `dateProtoFuncGetFullYear` e as irmãs `getMonth`, `getDate`, `getDay`, `getHours`, `getMinutes`,
/// `getSeconds` e as versões UTC (`as_utc`): NaN se a data é inválida.
pub fn date_proto_func_get_field(this: &DateInstance, cache: &DateCache, field: DateField, as_utc: bool) -> f64 {
    let gregorian_date_time = if as_utc { this.gregorian_date_time_utc(cache) } else { this.gregorian_date_time(cache) };
    if !gregorian_date_time.is_valid() {
        return f64::NAN;
    }
    let value = match field {
        DateField::FullYear => gregorian_date_time.year(),
        DateField::Month => gregorian_date_time.month(),
        DateField::Date => gregorian_date_time.month_day(),
        DateField::Day => gregorian_date_time.week_day(),
        DateField::Hours => gregorian_date_time.hour(),
        DateField::Minutes => gregorian_date_time.minute(),
        DateField::Seconds => gregorian_date_time.second(),
    };
    value as f64
}

/// `dateProtoFuncGetTime` (e `valueOf`).
pub fn date_proto_func_get_time(this: &DateInstance) -> f64 {
    this.internal_number()
}

/// `dateProtoFuncGetMilliSeconds` e `dateProtoFuncGetUTCMilliseconds` (o mesmo corpo).
pub fn date_proto_func_get_milliseconds(this: &DateInstance) -> f64 {
    let milli = this.internal_number();
    if milli.is_nan() {
        return f64::NAN;
    }

    let secs = (milli / MS_PER_SECOND).floor();
    let ms = milli - secs * MS_PER_SECOND;
    // Since timeClip makes internalNumber integer milliseconds, this result is always int32_t.
    (ms as i32) as f64
}

/// `dateProtoFuncGetTimezoneOffset`.
pub fn date_proto_func_get_timezone_offset(this: &DateInstance, cache: &DateCache) -> f64 {
    let gregorian_date_time = this.gregorian_date_time(cache);
    if !gregorian_date_time.is_valid() {
        return f64::NAN;
    }
    (-gregorian_date_time.utc_offset_in_minute()) as f64
}

/// `dateProtoFuncGetYear`: o ano completo menos 1900.
pub fn date_proto_func_get_year(this: &DateInstance, cache: &DateCache) -> f64 {
    let gregorian_date_time = this.gregorian_date_time(cache);
    if !gregorian_date_time.is_valid() {
        return f64::NAN;
    }

    // NOTE: IE returns the full year even in getYear.
    (gregorian_date_time.year() - 1900) as f64
}

/// `dateProtoFuncSetTime`: `milli` é o `timeClip` do `toNumber` do argumento (o `TypeError` de `this`
/// vem antes da conversão, na casca).
pub fn date_proto_func_set_time(this: &DateInstance, number: f64) -> f64 {
    let milli = time_clip(number);
    this.set_internal_number(milli);
    milli
}

/// `struct BrokenDownDate`: os campos de que um setter monta o novo valor de tempo. Não é um
/// `PlainGregorianDateTime` (empacotado e conferido): aqui cabe o ano ou o mês fora da faixa que o
/// chamador passou, antes do `timeClip`.
#[derive(Clone, Copy, Debug, Default)]
struct BrokenDownDate {
    year: i32,
    month: i32,
    month_day: i32,
    hour: i32,
    minute: i32,
    second: i32,
}

impl BrokenDownDate {
    fn from_plain(t: PlainGregorianDateTime) -> BrokenDownDate {
        BrokenDownDate {
            year: t.year(),
            month: t.month(),
            month_day: t.month_day(),
            hour: t.hour(),
            minute: t.minute(),
            second: t.second(),
        }
    }
}

/// `applyToNumberToOtherwiseIgnoredArguments`: converte (e descarta) os primeiros `max_args` argumentos.
fn apply_to_number_to_otherwise_ignored_arguments<E>(
    argument_count: usize,
    max_args: usize,
    convert: &mut dyn FnMut(usize) -> Result<f64, E>,
) -> Result<(), E> {
    for index in 0..argument_count.min(max_args) {
        convert(index)?;
    }
    Ok(())
}

/// `fillStructuresUsingTimeArgs`: converte os argumentos de `f([hour,] [min,] [sec,] [ms])` em
/// milissegundos, atualizando `ms` e `t`. Devolve se o resultado é finito.
fn fill_structures_using_time_args<E>(
    argument_count: usize,
    max_args: usize,
    convert: &mut dyn FnMut(usize) -> Result<f64, E>,
    ms: &mut f64,
    t: &mut BrokenDownDate,
) -> Result<bool, E> {
    let mut milliseconds = 0.0;
    let mut idx = 0;
    let num_args = argument_count.min(max_args);

    // hours
    if max_args >= 4 && idx < num_args {
        t.hour = 0;
        let hours = convert(idx)?;
        idx += 1;
        milliseconds += hours * MS_PER_HOUR;
    }

    // minutes
    if max_args >= 3 && idx < num_args {
        t.minute = 0;
        let minutes = convert(idx)?;
        idx += 1;
        milliseconds += minutes * MS_PER_MINUTE;
    }

    // seconds
    if max_args >= 2 && idx < num_args {
        t.second = 0;
        let seconds = convert(idx)?;
        idx += 1;
        milliseconds += seconds * MS_PER_SECOND;
    }

    // milliseconds
    if idx < num_args {
        let millis = convert(idx)?;
        milliseconds += millis;
    } else {
        milliseconds += *ms;
    }

    *ms = milliseconds;
    Ok(milliseconds.is_finite())
}

/// `fillStructuresUsingDateArgs`: converte os argumentos de `f([years,] [months,] [days])` em ano, mês e
/// milissegundos, atualizando `ms` e `t`. Devolve se o resultado é representável.
fn fill_structures_using_date_args<E>(
    argument_count: usize,
    max_args: usize,
    convert: &mut dyn FnMut(usize) -> Result<f64, E>,
    ms: &mut f64,
    t: &mut BrokenDownDate,
) -> Result<bool, E> {
    let mut ok = true;
    let mut idx = 0;
    let num_args = argument_count.min(max_args);
    let max_year = ms_to_year(MAX_ECMASCRIPT_TIME) as f64;

    // years
    if max_args >= 3 && idx < num_args {
        let years = convert(idx)?;
        idx += 1;

        // The broken-down date represents `years` as `int`. Therefore, if the `years` exceeds the
        // maximum representable `int`, date calculations may produce incorrect results. The
        // condition, `abs(years) <= msToYear(maxECMAScriptTime)`, is used as a safeguard before
        // `timeClip(double)`.
        ok = ok && years.is_finite() && years.abs() <= max_year;
        t.year = to_int32(years);
    }
    // months
    if max_args >= 2 && idx < num_args {
        let months = convert(idx)?;
        idx += 1;
        let years = months / 12.0;
        ok = ok && months.is_finite() && years.abs() <= max_year;
        t.month = to_int32(months);
    }
    // days
    if idx < num_args {
        let days = convert(idx)?;
        ok = ok && days.is_finite();
        t.month_day = 0;
        *ms += days * MS_PER_DAY;
    }

    Ok(ok)
}

fn current_gregorian(this: &DateInstance, cache: &DateCache, input_time_type: TimeType) -> PlainGregorianDateTime {
    if input_time_type == TimeType::UTCTime { this.gregorian_date_time_utc(cache) } else { this.gregorian_date_time(cache) }
}

/// `setNewValueFromTimeArgs`: `setMilliseconds`/`setSeconds`/`setMinutes`/`setHours` e as versões UTC
/// (`num_args_to_use` 1 a 4). `convert(i)` é o `toIntegerPreserveNaN` do argumento `i`.
pub fn set_new_value_from_time_args<E>(
    this: &DateInstance,
    cache: &DateCache,
    argument_count: usize,
    num_args_to_use: usize,
    input_time_type: TimeType,
    convert: &mut dyn FnMut(usize) -> Result<f64, E>,
) -> Result<f64, E> {
    if argument_count == 0 {
        this.set_internal_number(f64::NAN);
        return Ok(f64::NAN);
    }

    let milli = this.internal_number();
    if milli.is_nan() {
        apply_to_number_to_otherwise_ignored_arguments(argument_count, num_args_to_use, convert)?;
        if this.internal_number().is_nan() {
            this.set_internal_number(f64::NAN);
        }
        return Ok(f64::NAN);
    }

    let secs = (milli / MS_PER_SECOND).floor();
    let mut ms = milli - secs * MS_PER_SECOND;

    let other = current_gregorian(this, cache, input_time_type);
    if !other.is_valid() {
        apply_to_number_to_otherwise_ignored_arguments(argument_count, num_args_to_use, convert)?;
        return Ok(f64::NAN);
    }

    let mut gregorian_date_time = BrokenDownDate::from_plain(other);
    let success = fill_structures_using_time_args(argument_count, num_args_to_use, convert, &mut ms, &mut gregorian_date_time)?;
    if !success {
        this.set_internal_number(f64::NAN);
        return Ok(f64::NAN);
    }

    let new_utc_date = cache.gregorian_date_time_to_ms(
        gregorian_date_time.year,
        gregorian_date_time.month,
        gregorian_date_time.month_day,
        gregorian_date_time.hour,
        gregorian_date_time.minute,
        gregorian_date_time.second,
        ms,
        input_time_type,
    );
    let result = time_clip(new_utc_date);
    this.set_internal_number(result);
    Ok(result)
}

/// `setNewValueFromDateArgs`: `setDate`/`setMonth`/`setFullYear` e as versões UTC (`num_args_to_use` 1 a 3).
pub fn set_new_value_from_date_args<E>(
    this: &DateInstance,
    cache: &DateCache,
    argument_count: usize,
    num_args_to_use: usize,
    input_time_type: TimeType,
    convert: &mut dyn FnMut(usize) -> Result<f64, E>,
) -> Result<f64, E> {
    if argument_count == 0 {
        this.set_internal_number(f64::NAN);
        return Ok(f64::NAN);
    }

    let milli = this.internal_number();
    let mut ms = 0.0;

    let mut gregorian_date_time;
    if num_args_to_use == 3 && milli.is_nan() {
        gregorian_date_time = BrokenDownDate::from_plain(cache.ms_to_gregorian_date_time(0.0, TimeType::UTCTime, UseSharedCache::Yes));
    } else {
        ms = milli - (milli / MS_PER_SECOND).floor() * MS_PER_SECOND;
        let other = current_gregorian(this, cache, input_time_type);
        if !other.is_valid() {
            apply_to_number_to_otherwise_ignored_arguments(argument_count, num_args_to_use, convert)?;
            return Ok(f64::NAN);
        }
        gregorian_date_time = BrokenDownDate::from_plain(other);
    }

    let success = fill_structures_using_date_args(argument_count, num_args_to_use, convert, &mut ms, &mut gregorian_date_time)?;
    if !success {
        this.set_internal_number(f64::NAN);
        return Ok(f64::NAN);
    }

    let new_utc_date = cache.gregorian_date_time_to_ms(
        gregorian_date_time.year,
        gregorian_date_time.month,
        gregorian_date_time.month_day,
        gregorian_date_time.hour,
        gregorian_date_time.minute,
        gregorian_date_time.second,
        ms,
        input_time_type,
    );
    let result = time_clip(new_utc_date);
    this.set_internal_number(result);
    Ok(result)
}

/// `dateProtoFuncSetYear` (Annex B.2.5). `convert(0)` é o `toIntegerPreserveNaN` do primeiro argumento.
pub fn date_proto_func_set_year<E>(
    this: &DateInstance,
    cache: &DateCache,
    argument_count: usize,
    convert: &mut dyn FnMut(usize) -> Result<f64, E>,
) -> Result<f64, E> {
    if argument_count == 0 {
        this.set_internal_number(f64::NAN);
        return Ok(f64::NAN);
    }

    let milli = this.internal_number();
    let mut ms = 0.0;

    let mut gregorian_date_time;
    if milli.is_nan() {
        // Based on ECMA 262 B.2.5 (setYear) the time must be reset to +0 if it is NaN.
        gregorian_date_time = BrokenDownDate::from_plain(cache.ms_to_gregorian_date_time(0.0, TimeType::UTCTime, UseSharedCache::Yes));
    } else {
        let secs = (milli / MS_PER_SECOND).floor();
        ms = milli - secs * MS_PER_SECOND;
        gregorian_date_time = BrokenDownDate::from_plain(this.gregorian_date_time(cache));
    }

    let year = convert(0)?;
    if !year.is_finite() || year.abs() > ms_to_year(MAX_ECMASCRIPT_TIME) as f64 {
        this.set_internal_number(f64::NAN);
        return Ok(f64::NAN);
    }

    gregorian_date_time.year = to_int32(if (0.0..=99.0).contains(&year) { year + 1900.0 } else { year });
    let time_in_milliseconds = cache.gregorian_date_time_to_ms(
        gregorian_date_time.year,
        gregorian_date_time.month,
        gregorian_date_time.month_day,
        gregorian_date_time.hour,
        gregorian_date_time.minute,
        gregorian_date_time.second,
        ms,
        TimeType::LocalTime,
    );
    let result = time_clip(time_in_milliseconds);
    this.set_internal_number(result);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_date_math::UtcTimeZone;

    fn utc_plain(ms: f64) -> (DateCache, PlainGregorianDateTime) {
        let cache = DateCache::new(Box::new(UtcTimeZone));
        let t = cache.ms_to_gregorian_date_time(ms, TimeType::UTCTime, UseSharedCache::No);
        (cache, t)
    }

    #[test]
    fn formats_like_node() {
        let (cache, t) = utc_plain(0.0);
        assert_eq!(format_date_time(t, DateTimeFormat::DateAndTime, true, &cache), "Thu, 01 Jan 1970 00:00:00 GMT");
        let local = cache.ms_to_gregorian_date_time(0.0, TimeType::LocalTime, UseSharedCache::No);
        assert_eq!(
            format_date_time(local, DateTimeFormat::DateAndTime, false, &cache),
            "Thu Jan 01 1970 00:00:00 GMT+0000 (Coordinated Universal Time)"
        );
        assert_eq!(format_date_time(local, DateTimeFormat::Date, false, &cache), "Thu Jan 01 1970");
        assert_eq!(format_date_time(t, DateTimeFormat::Time, true, &cache), "00:00:00 GMT");
    }

    #[test]
    fn negative_year_is_padded() {
        let (cache, t) = utc_plain(-62198755200000.0); // -000001-01-01
        assert_eq!(t.year(), -1);
        assert_eq!(format_date_time(t, DateTimeFormat::Date, true, &cache), "Fri, 01 Jan -0001");
    }
}
