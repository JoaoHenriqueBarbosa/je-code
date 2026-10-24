//! As funções nativas de `Date.prototype` (`DatePrototype.cpp`, os `JSC_DEFINE_HOST_FUNCTION`
//! `dateProtoFunc*`) e o `finishCreation` do protótipo: `toUTCString` e `toGMTString` (a mesma função),
//! `[Symbol.toPrimitive]` e a tabela `datePrototypeTable`. Os algoritmos moram em `date_prototype.rs`;
//! aqui ficam a casca de cada função (ler `this` e os argumentos, converter com código do usuário,
//! empacotar o resultado) e o `Date.prototype[Symbol.toPrimitive]` e o `toJSON`, que chamam código do
//! usuário.
//!
//! DIVERGÊNCIAS:
//!
//! - `datePrototypeTable` fica no `ClassInfo` (`DATE_PROTOTYPE_TABLE`) e a `Structure` leva
//!   `HasStaticPropertyTable`: as 44 funções reificam no primeiro acesso, como no C++. Eager, na ordem do
//!   `finishCreation`: `toUTCString`, `toGMTString`, `toTemporalInstant` e depois `constructor` e
//!   `@@toPrimitive`.
//! - `toTemporalInstant` só existe com `Options::useTemporal()` e é criado depois da tabela
//!   (`DatePrototype::finishCreation`).
//! - `toLocaleString`, `toLocaleDateString` e `toLocaleTimeString` passam pelo `IntlDateTimeFormat`
//!   de `intl_date_time_format.rs` (locales cobertas: en e pt-BR). O espaço antes de `AM`/`PM` é o
//!   espaço comum, como o `format()` do C++ o deixa depois de trocar o U+202F do ICU.
//! - `this` de `toPrimitive` e `toJSON` passa por `toThis(strict)` (`to_this_strict`).
//!   `toJSON` com `this` primitivo que não é `undefined`, `null` nem símbolo responde `Unported`
//!   (o `toObject` de número, booleano e string precisa dos objetos-invólucro).

use crate::host_function;
use crate::runtime::call_data::{call, get_call_data};
use crate::runtime::date_instance::{DateInstance, DateInstanceRef};
use crate::runtime::date_prototype::{
    self as algorithm, format_date_instance, set_new_value_from_date_args, set_new_value_from_time_args, this_date_instance,
    to_json_returns_null, DateError, DateField, DatePrototype, DateTimeFormat,
};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::js_object::JSObject;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::thrown_from_llint;
use crate::runtime::intl_date_time_format::{to_locale_string as intl_to_locale_string, Defaults, Required};
use crate::runtime::iso8601::ExactTime;
use crate::runtime::js_date_math::DateCache;
use crate::runtime::js_function::{call_host_function_as_constructor, put_direct_native_function_without_transition, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObjectRef;
use crate::runtime::object_to_primitive::{object_to_primitive, ordinary_to_primitive, PreferredPrimitiveType};
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_number, purify_nan, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_ENUM, FUNCTION, READ_ONLY};
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{native_entry_with_intrinsic};
use crate::runtime::property_name::PropertyName;
use crate::runtime::temporal_instant::TemporalInstant;
use crate::runtime::vm::VM;
use crate::wtf::date_math::TimeType;
use crate::wtf::text::wtf_string::String as WtfString;

// ---------------------------------------------------------------------------------------------
// Auxiliares
// ---------------------------------------------------------------------------------------------

/// O `Thrown` do que `date_prototype.rs` lança.
fn thrown_from_date_error(error: DateError) -> Thrown {
    match error {
        DateError::TypeError(message) => Thrown::type_error(message),
        DateError::RangeError(message) => Thrown::range_error(message),
    }
}

/// `dynamicDowncast<DateInstance>(thisValue)` ou o `throwVMTypeError(globalObject, scope)`.
fn this_date(call: &HostCall) -> Result<DateInstanceRef, Thrown> {
    this_date_instance(call.this_value()).map_err(thrown_from_date_error)
}

/// `jsNumber(double)`, com o `NaN` sempre na forma pura.
pub(crate) fn number_value(value: f64) -> JSValue {
    js_number(purify_nan(value))
}

/// `jsNontrivialString(vm, text)`.
pub(crate) fn string_value(vm: &VM, text: &str) -> JSValue {
    JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(text.as_bytes())))
}

fn time_type(utc: bool) -> TimeType {
    if utc { TimeType::UTCTime } else { TimeType::LocalTime }
}

const FORMAT_DATE: u8 = DateTimeFormat::Date as u8;
const FORMAT_TIME: u8 = DateTimeFormat::Time as u8;
const FORMAT_DATE_AND_TIME: u8 = DateTimeFormat::DateAndTime as u8;

fn date_time_format(kind: u8) -> DateTimeFormat {
    match kind {
        FORMAT_DATE => DateTimeFormat::Date,
        FORMAT_TIME => DateTimeFormat::Time,
        _ => DateTimeFormat::DateAndTime,
    }
}

/// Os campos que um getter lê, na ordem dos índices `FIELD_*`.
const FIELD_FULL_YEAR: usize = 0;
const FIELD_MONTH: usize = 1;
const FIELD_DATE: usize = 2;
const FIELD_DAY: usize = 3;
const FIELD_HOURS: usize = 4;
const FIELD_MINUTES: usize = 5;
const FIELD_SECONDS: usize = 6;
const DATE_FIELDS: [DateField; 7] = [
    DateField::FullYear,
    DateField::Month,
    DateField::Date,
    DateField::Day,
    DateField::Hours,
    DateField::Minutes,
    DateField::Seconds,
];

// ---------------------------------------------------------------------------------------------
// Corpos
// ---------------------------------------------------------------------------------------------

/// `dateProtoFuncGetFullYear`, `GetMonth`, `GetDate`, `GetDay`, `GetHours`, `GetMinutes`, `GetSeconds` e
/// as versões UTC.
fn get_field<const FIELD: usize, const UTC: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = this_date(call)?;
    Ok(number_value(algorithm::date_proto_func_get_field(&this, global_object.vm().date_cache(), DATE_FIELDS[FIELD], UTC)))
}

/// `dateProtoFuncGetTime` (e `valueOf`).
fn get_time(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = this_date(call)?;
    Ok(number_value(algorithm::date_proto_func_get_time(&this)))
}

/// `dateProtoFuncGetMilliSeconds` e `dateProtoFuncGetUTCMilliseconds` (o mesmo corpo).
fn get_milliseconds(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = this_date(call)?;
    Ok(number_value(algorithm::date_proto_func_get_milliseconds(&this)))
}

/// `dateProtoFuncGetTimezoneOffset`.
fn get_timezone_offset(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = this_date(call)?;
    Ok(number_value(algorithm::date_proto_func_get_timezone_offset(&this, global_object.vm().date_cache())))
}

/// `dateProtoFuncGetYear`.
fn get_year(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = this_date(call)?;
    Ok(number_value(algorithm::date_proto_func_get_year(&this, global_object.vm().date_cache())))
}

/// `dateProtoFuncSetTime`: o `TypeError` de `this` vem antes da conversão do argumento.
fn set_time(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = this_date(call)?;
    let number = pending_or(global_object, call.argument(0).to_number())?;
    Ok(number_value(algorithm::date_proto_func_set_time(&this, number)))
}

/// `setNewValueFromTimeArgs` e `setNewValueFromDateArgs`: o mesmo contrato, `convert(i)` é o
/// `toIntegerPreserveNaN` do argumento `i`.
type SetterAlgorithm =
    fn(&DateInstance, &DateCache, usize, usize, TimeType, &mut dyn FnMut(usize) -> Result<f64, Thrown>) -> Result<f64, Thrown>;

/// O que `dateProtoFuncSet*` têm em comum: `this`, a conversão dos argumentos com código do usuário e o
/// algoritmo de `date_prototype.rs`.
fn set_from_args(global_object: &JSGlobalObject, call: &HostCall, apply: SetterAlgorithm, num_args_to_use: usize, utc: bool) -> HostResult {
    let this = this_date(call)?;
    let mut convert = |index: usize| to_integer_preserve_nan(global_object, call.argument(index));
    let result = apply(&this, global_object.vm().date_cache(), call.argument_count(), num_args_to_use, time_type(utc), &mut convert)?;
    Ok(number_value(result))
}

/// `dateProtoFuncSetMilliSeconds`, `SetSeconds`, `SetMinutes`, `SetHours` e as versões UTC.
fn set_from_time_args<const NUM_ARGS: usize, const UTC: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    set_from_args(global_object, call, set_new_value_from_time_args::<Thrown>, NUM_ARGS, UTC)
}

/// `dateProtoFuncSetDate`, `SetMonth`, `SetFullYear` e as versões UTC.
fn set_from_date_args<const NUM_ARGS: usize, const UTC: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    set_from_args(global_object, call, set_new_value_from_date_args::<Thrown>, NUM_ARGS, UTC)
}

/// `dateProtoFuncSetYear`.
fn set_year(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = this_date(call)?;
    let mut convert = |index: usize| to_integer_preserve_nan(global_object, call.argument(index));
    let result = algorithm::date_proto_func_set_year(&this, global_object.vm().date_cache(), call.argument_count(), &mut convert)?;
    Ok(number_value(result))
}

/// `formateDateInstance`: `toString`, `toUTCString`, `toDateString` e `toTimeString`.
fn format_this<const FORMAT: u8, const UTC: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let this = this_date(call)?;
    Ok(string_value(vm, &format_date_instance(&this, vm.date_cache(), date_time_format(FORMAT), UTC)))
}

/// `dateProtoFuncToISOString`.
fn to_iso_string(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let this = this_date(call)?;
    let text = algorithm::date_proto_func_to_iso_string(&this, vm.date_cache()).map_err(thrown_from_date_error)?;
    Ok(string_value(vm, &text))
}

/// `toPreferredPrimitiveType(globalObject, value)`: a `hint` de `@@toPrimitive`.
fn to_preferred_primitive_type(value: JSValue) -> Result<PreferredPrimitiveType, Thrown> {
    if !value.is_string() {
        return Err(Thrown::type_error("Primitive hint is not a string."));
    }

    let hint = value.as_js_string().value();
    let matches = |text: &[u8]| hint == WtfString::from_latin1(text);
    if matches(b"default") {
        return Ok(PreferredPrimitiveType::NoPreference);
    }
    if matches(b"number") {
        return Ok(PreferredPrimitiveType::PreferNumber);
    }
    if matches(b"string") {
        return Ok(PreferredPrimitiveType::PreferString);
    }
    Err(Thrown::type_error("Expected primitive hint to match one of 'default', 'number', 'string'."))
}

/// `JSValue::toIntegerPreserveNaN(globalObject)`: o `trunc` do `toNumber`, e `NaN` continua `NaN`.
fn to_integer_preserve_nan(global_object: &JSGlobalObject, value: JSValue) -> Result<f64, Thrown> {
    if value.is_int32() {
        return Ok(value.as_int32() as f64);
    }
    Ok(pending_or(global_object, value.to_number())?.trunc())
}

/// `dateProtoFuncToPrimitiveSymbol`.
fn to_primitive_symbol(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_value = crate::runtime::proxy_object::to_this_strict(call.this_value());
    if !this_value.is_object() {
        return Err(Thrown::type_error("Date.prototype[Symbol.toPrimitive] expected |this| to be an object."));
    }

    if call.argument_count() == 0 {
        return Err(Thrown::type_error("Date.prototype[Symbol.toPrimitive] expected a first argument."));
    }

    let mut preferred_type = to_preferred_primitive_type(call.argument(0))?;
    if preferred_type == PreferredPrimitiveType::NoPreference {
        preferred_type = PreferredPrimitiveType::PreferString;
    }
    ordinary_to_primitive(global_object, &this_value.as_object(), preferred_type).ok_or(Thrown::Pending)
}

/// `thisValue.toObject(globalObject)` de `dateProtoFuncToJSON`.
fn this_to_object(global_object: &JSGlobalObject, this_value: JSValue) -> Result<ObjectRef, Thrown> {
    this_value.to_object(global_object).ok_or(Thrown::Pending)
}

/// `dateProtoFuncToJSON`.
fn to_json(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let object = this_to_object(global_object, crate::runtime::proxy_object::to_this_strict(call.this_value()))?;

    let time_value = object_to_primitive(global_object, &object, PreferredPrimitiveType::PreferNumber).ok_or(Thrown::Pending)?;
    if to_json_returns_null(time_value) {
        return Ok(JSValue::null());
    }

    let to_iso_value = object.get(global_object, &PropertyName::from_identifier(&vm.property_names.to_iso_string));
    if vm.exception().is_some() {
        return Err(Thrown::Pending);
    }

    let call_data = get_call_data(to_iso_value);
    if call_data.is_none() {
        return Err(Thrown::type_error("toISOString is not a function"));
    }

    crate::runtime::call_data::call(global_object, to_iso_value, &call_data, object.as_value(), &[]).map_err(thrown_from_llint)
}

/// `dateProtoFuncToLocaleString`, `ToLocaleDateString` e `ToLocaleTimeString`: o
/// `IntlDateTimeFormat` de `intl_date_time_format.rs` com `locales` e `options`
/// (`RequiredComponent`/`Defaults`: `Any`/`All`, `Date`/`Date` e `Time`/`Time`).
fn to_locale_string<const FORMAT: u8>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let this = this_date(call)?;

    if this.internal_number().is_nan() {
        return Ok(string_value(vm, "Invalid Date"));
    }

    let (required, defaults) = match date_time_format(FORMAT) {
        DateTimeFormat::Date => (Required::Date, Defaults::Date),
        DateTimeFormat::Time => (Required::Time, Defaults::Time),
        DateTimeFormat::DateAndTime => (Required::Any, Defaults::All),
    };
    let text = intl_to_locale_string(global_object, call.argument(0), call.argument(1), required, defaults, this.internal_number())?;
    Ok(string_value(vm, &text))
}

// ---------------------------------------------------------------------------------------------
// As funções nativas
// ---------------------------------------------------------------------------------------------

host_function!(date_proto_func_get_full_year, get_field::<FIELD_FULL_YEAR, false>);
host_function!(date_proto_func_get_utc_full_year, get_field::<FIELD_FULL_YEAR, true>);
host_function!(date_proto_func_get_month, get_field::<FIELD_MONTH, false>);
host_function!(date_proto_func_get_utc_month, get_field::<FIELD_MONTH, true>);
host_function!(date_proto_func_get_date, get_field::<FIELD_DATE, false>);
host_function!(date_proto_func_get_utc_date, get_field::<FIELD_DATE, true>);
host_function!(date_proto_func_get_day, get_field::<FIELD_DAY, false>);
host_function!(date_proto_func_get_utc_day, get_field::<FIELD_DAY, true>);
host_function!(date_proto_func_get_hours, get_field::<FIELD_HOURS, false>);
host_function!(date_proto_func_get_utc_hours, get_field::<FIELD_HOURS, true>);
host_function!(date_proto_func_get_minutes, get_field::<FIELD_MINUTES, false>);
host_function!(date_proto_func_get_utc_minutes, get_field::<FIELD_MINUTES, true>);
host_function!(date_proto_func_get_seconds, get_field::<FIELD_SECONDS, false>);
host_function!(date_proto_func_get_utc_seconds, get_field::<FIELD_SECONDS, true>);
host_function!(date_proto_func_get_time, get_time);
host_function!(date_proto_func_get_milli_seconds, get_milliseconds);
host_function!(date_proto_func_get_utc_milliseconds, get_milliseconds);
host_function!(date_proto_func_get_timezone_offset, get_timezone_offset);
host_function!(date_proto_func_get_year, get_year);
host_function!(date_proto_func_set_time, set_time);
host_function!(date_proto_func_set_milli_seconds, set_from_time_args::<1, false>);
host_function!(date_proto_func_set_utc_milliseconds, set_from_time_args::<1, true>);
host_function!(date_proto_func_set_seconds, set_from_time_args::<2, false>);
host_function!(date_proto_func_set_utc_seconds, set_from_time_args::<2, true>);
host_function!(date_proto_func_set_minutes, set_from_time_args::<3, false>);
host_function!(date_proto_func_set_utc_minutes, set_from_time_args::<3, true>);
host_function!(date_proto_func_set_hours, set_from_time_args::<4, false>);
host_function!(date_proto_func_set_utc_hours, set_from_time_args::<4, true>);
host_function!(date_proto_func_set_date, set_from_date_args::<1, false>);
host_function!(date_proto_func_set_utc_date, set_from_date_args::<1, true>);
host_function!(date_proto_func_set_month, set_from_date_args::<2, false>);
host_function!(date_proto_func_set_utc_month, set_from_date_args::<2, true>);
host_function!(date_proto_func_set_full_year, set_from_date_args::<3, false>);
host_function!(date_proto_func_set_utc_full_year, set_from_date_args::<3, true>);
host_function!(date_proto_func_set_year, set_year);
host_function!(date_proto_func_to_date_string, format_this::<FORMAT_DATE, false>);
host_function!(date_proto_func_to_primitive_symbol, to_primitive_symbol);
host_function!(date_proto_func_to_string, format_this::<FORMAT_DATE_AND_TIME, false>);
host_function!(date_proto_func_to_time_string, format_this::<FORMAT_TIME, false>);
host_function!(date_proto_func_to_utc_string, format_this::<FORMAT_DATE_AND_TIME, true>);
host_function!(date_proto_func_to_iso_string, to_iso_string);
host_function!(date_proto_func_to_json, to_json);
host_function!(date_proto_func_to_locale_string, to_locale_string::<FORMAT_DATE_AND_TIME>);
host_function!(date_proto_func_to_locale_date_string, to_locale_string::<FORMAT_DATE>);
host_function!(date_proto_func_to_locale_time_string, to_locale_string::<FORMAT_TIME>);
host_function!(date_proto_func_to_temporal_instant, to_temporal_instant);

/// `dateProtoFuncToTemporalInstant`: o `Temporal.Instant` do valor de tempo (milissegundos inteiros),
/// `RangeError` para `NaN` (`Invalid Date`).
fn to_temporal_instant(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = this_date(call)?;
    let epoch_milliseconds = this.internal_number();
    if !epoch_milliseconds.is_finite() || epoch_milliseconds.trunc() != epoch_milliseconds {
        return Err(Thrown::range_error("Invalid integer number of Epoch Millseconds"));
    }
    let exact_time = ExactTime::from_epoch_milliseconds(epoch_milliseconds as i64);
    Ok(TemporalInstant::create(global_object.vm(), global_object.instant_structure(), exact_time).as_value())
}

/// `datePrototypeTableValues`, na ordem do `@begin ... @end` de `DatePrototype.cpp`.
static DATE_PROTOTYPE_TABLE_VALUES: [HashTableValue; 44] = [
    native_entry_with_intrinsic("toString", date_proto_func_to_string, 0, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("toISOString", date_proto_func_to_iso_string, 0, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("toDateString", date_proto_func_to_date_string, 0, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("toTimeString", date_proto_func_to_time_string, 0, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("toLocaleString", date_proto_func_to_locale_string, 0, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("toLocaleDateString", date_proto_func_to_locale_date_string, 0, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("toLocaleTimeString", date_proto_func_to_locale_time_string, 0, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("valueOf", date_proto_func_get_time, 0, Intrinsic::DatePrototypeGetTimeIntrinsic),
    native_entry_with_intrinsic("getTime", date_proto_func_get_time, 0, Intrinsic::DatePrototypeGetTimeIntrinsic),
    native_entry_with_intrinsic("getFullYear", date_proto_func_get_full_year, 0, Intrinsic::DatePrototypeGetFullYearIntrinsic),
    native_entry_with_intrinsic("getUTCFullYear", date_proto_func_get_utc_full_year, 0, Intrinsic::DatePrototypeGetUTCFullYearIntrinsic),
    native_entry_with_intrinsic("getMonth", date_proto_func_get_month, 0, Intrinsic::DatePrototypeGetMonthIntrinsic),
    native_entry_with_intrinsic("getUTCMonth", date_proto_func_get_utc_month, 0, Intrinsic::DatePrototypeGetUTCMonthIntrinsic),
    native_entry_with_intrinsic("getDate", date_proto_func_get_date, 0, Intrinsic::DatePrototypeGetDateIntrinsic),
    native_entry_with_intrinsic("getUTCDate", date_proto_func_get_utc_date, 0, Intrinsic::DatePrototypeGetUTCDateIntrinsic),
    native_entry_with_intrinsic("getDay", date_proto_func_get_day, 0, Intrinsic::DatePrototypeGetDayIntrinsic),
    native_entry_with_intrinsic("getUTCDay", date_proto_func_get_utc_day, 0, Intrinsic::DatePrototypeGetUTCDayIntrinsic),
    native_entry_with_intrinsic("getHours", date_proto_func_get_hours, 0, Intrinsic::DatePrototypeGetHoursIntrinsic),
    native_entry_with_intrinsic("getUTCHours", date_proto_func_get_utc_hours, 0, Intrinsic::DatePrototypeGetUTCHoursIntrinsic),
    native_entry_with_intrinsic("getMinutes", date_proto_func_get_minutes, 0, Intrinsic::DatePrototypeGetMinutesIntrinsic),
    native_entry_with_intrinsic("getUTCMinutes", date_proto_func_get_utc_minutes, 0, Intrinsic::DatePrototypeGetUTCMinutesIntrinsic),
    native_entry_with_intrinsic("getSeconds", date_proto_func_get_seconds, 0, Intrinsic::DatePrototypeGetSecondsIntrinsic),
    native_entry_with_intrinsic("getUTCSeconds", date_proto_func_get_utc_seconds, 0, Intrinsic::DatePrototypeGetUTCSecondsIntrinsic),
    native_entry_with_intrinsic("getMilliseconds", date_proto_func_get_milli_seconds, 0, Intrinsic::DatePrototypeGetMillisecondsIntrinsic),
    native_entry_with_intrinsic("getUTCMilliseconds", date_proto_func_get_utc_milliseconds, 0, Intrinsic::DatePrototypeGetUTCMillisecondsIntrinsic),
    native_entry_with_intrinsic("getTimezoneOffset", date_proto_func_get_timezone_offset, 0, Intrinsic::DatePrototypeGetTimezoneOffsetIntrinsic),
    native_entry_with_intrinsic("getYear", date_proto_func_get_year, 0, Intrinsic::DatePrototypeGetYearIntrinsic),
    native_entry_with_intrinsic("setTime", date_proto_func_set_time, 1, Intrinsic::DatePrototypeSetTimeIntrinsic),
    native_entry_with_intrinsic("setMilliseconds", date_proto_func_set_milli_seconds, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("setUTCMilliseconds", date_proto_func_set_utc_milliseconds, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("setSeconds", date_proto_func_set_seconds, 2, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("setUTCSeconds", date_proto_func_set_utc_seconds, 2, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("setMinutes", date_proto_func_set_minutes, 3, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("setUTCMinutes", date_proto_func_set_utc_minutes, 3, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("setHours", date_proto_func_set_hours, 4, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("setUTCHours", date_proto_func_set_utc_hours, 4, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("setDate", date_proto_func_set_date, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("setUTCDate", date_proto_func_set_utc_date, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("setMonth", date_proto_func_set_month, 2, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("setUTCMonth", date_proto_func_set_utc_month, 2, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("setFullYear", date_proto_func_set_full_year, 3, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("setUTCFullYear", date_proto_func_set_utc_full_year, 3, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("setYear", date_proto_func_set_year, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("toJSON", date_proto_func_to_json, 1, Intrinsic::NoIntrinsic),
];

/// `datePrototypeTable`.
pub static DATE_PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &DATE_PROTOTYPE_TABLE_VALUES };

/// `[Symbol.toPrimitive]` do `Date.prototype` (`DontEnum|ReadOnly`): no golden vem depois do `constructor`,
/// por isso é instalado por `install_date` e não por `create_date_prototype`.
pub fn install_date_prototype_to_primitive(vm: &VM, global_object: &JSGlobalObject, prototype: &JSObject) {
    let to_primitive_function = JSFunction::create_native(
        vm,
        global_object,
        1,
        &WtfString::from_latin1(b"[Symbol.toPrimitive]"),
        date_proto_func_to_primitive_symbol,
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
}

/// `DatePrototype::create(vm, globalObject, structure)` com o `finishCreation` do C++ e a tabela
/// estática. O `constructor` entra depois, quando o `DateConstructor` existe.
pub fn create_date_prototype(vm: &VM, global_object: &JSGlobalObject, object_prototype: JSValue) -> JSObjectRef {
    let structure = DatePrototype::create_structure(vm, Some(global_object), object_prototype);
    let prototype = DatePrototype::create(vm, global_object, &structure);

    // A `datePrototypeTable` reifica no primeiro acesso (`DATE_PROTOTYPE_TABLE`). Ordem do golden
    // (`Reflect.ownKeys` do bun): a tabela primeiro, depois `toUTCString`/`toGMTString`,
    // `toTemporalInstant`, o `constructor` e por fim `[Symbol.toPrimitive]` (ver
    // `install_date_prototype_to_primitive`).

    // `toUTCString` e `toGMTString` são a mesma função.
    let to_utc_string_function = JSFunction::create_native(
        vm,
        global_object,
        0,
        &WtfString::from_latin1(b"toUTCString"),
        date_proto_func_to_utc_string,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    for name in [b"toUTCString".as_slice(), b"toGMTString".as_slice()] {
        prototype.put_direct(
            vm,
            &PropertyName::from_identifier(&Identifier::from_span(vm, name)),
            to_utc_string_function.as_value(),
            DONT_ENUM,
        );
    }

    // `if (Options::useTemporal())`: `toTemporalInstant` (`putDirectWithoutTransition`, `DontEnum`), depois da tabela.
    if crate::runtime::options::Options::use_temporal() {
        put_direct_native_function_without_transition(
            vm,
            global_object,
            &prototype,
            &Identifier::from_span(vm, b"toTemporalInstant"),
            0,
            date_proto_func_to_temporal_instant,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            DONT_ENUM,
        );
    }
    prototype
}
