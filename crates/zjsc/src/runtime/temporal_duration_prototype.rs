//! Porte de `runtime/TemporalDurationPrototype.{h,cpp}`: `Temporal.Duration.prototype` (um
//! `JSNonFinalObject` com o `ClassInfo` `"Temporal.Duration"`): `with`, `negated`, `abs`, `add`, `subtract`,
//! `round`, `total`, `toString`, `toJSON`, `toLocaleString`, `valueOf`, os acessores `years` a `nanoseconds`,
//! `sign` e `blank` (`DontEnum|ReadOnly|CustomAccessor`) e `@@toStringTag`.
//!
//! `toLocaleString` delega ao `IntlDurationFormat` (`intl_duration_format::to_locale_string`).

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_duration_format::to_locale_string;
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{custom_getter_entry, native_entry};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_slot::GetValueFunc;
use crate::runtime::intl_support::str_value;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::property_attribute::{CUSTOM_ACCESSOR, DONT_ENUM, FUNCTION, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::temporal_core_duration::abs_duration;
use crate::runtime::temporal_duration::{create_temporal_duration, TemporalDuration, TemporalDurationRef};
use crate::runtime::temporal_object::{AddOrSubtract, TemporalUnit};
use crate::runtime::vm::VM;
use crate::custom_getter;

/// `const ClassInfo TemporalDurationPrototype::s_info` (`"Temporal.Duration"`).
pub static TEMPORAL_DURATION_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "Temporal.Duration",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&DURATION_PROTOTYPE_TABLE), inherits_js_type_range: None,
};

/// `durationPrototypeTableValues`, na ordem do `@begin`.
static DURATION_PROTOTYPE_TABLE_VALUES: [HashTableValue; 23] = [
    native_entry("with", temporal_duration_prototype_func_with, 1),
    native_entry("negated", temporal_duration_prototype_func_negated, 0),
    native_entry("abs", temporal_duration_prototype_func_abs, 0),
    native_entry("add", temporal_duration_prototype_func_add, 1),
    native_entry("subtract", temporal_duration_prototype_func_subtract, 1),
    native_entry("round", temporal_duration_prototype_func_round, 1),
    native_entry("total", temporal_duration_prototype_func_total, 1),
    native_entry("toString", temporal_duration_prototype_func_to_string, 0),
    native_entry("toJSON", temporal_duration_prototype_func_to_json, 0),
    native_entry("toLocaleString", temporal_duration_prototype_func_to_locale_string, 0),
    native_entry("valueOf", temporal_duration_prototype_func_value_of, 0),
    custom_getter_entry("years", temporal_duration_prototype_getter_years),
    custom_getter_entry("months", temporal_duration_prototype_getter_months),
    custom_getter_entry("weeks", temporal_duration_prototype_getter_weeks),
    custom_getter_entry("days", temporal_duration_prototype_getter_days),
    custom_getter_entry("hours", temporal_duration_prototype_getter_hours),
    custom_getter_entry("minutes", temporal_duration_prototype_getter_minutes),
    custom_getter_entry("seconds", temporal_duration_prototype_getter_seconds),
    custom_getter_entry("milliseconds", temporal_duration_prototype_getter_milliseconds),
    custom_getter_entry("microseconds", temporal_duration_prototype_getter_microseconds),
    custom_getter_entry("nanoseconds", temporal_duration_prototype_getter_nanoseconds),
    custom_getter_entry("sign", temporal_duration_prototype_getter_sign),
    custom_getter_entry("blank", temporal_duration_prototype_getter_blank),
];

/// `durationPrototypeTable`.
static DURATION_PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &DURATION_PROTOTYPE_TABLE_VALUES };

/// O `dynamicDowncast<TemporalDuration>(callFrame->thisValue())` com o `TypeError` de marca de cada método.
fn this_duration(this_value: JSValue, member: &str) -> Result<TemporalDurationRef, Thrown> {
    TemporalDuration::from_value(&this_value)
        .ok_or_else(|| Thrown::TypeError(format!("Temporal.Duration.prototype.{member} called on value that's not a Duration")))
}

/// `temporalDurationPrototypeFuncWith`: https://tc39.es/proposal-temporal/#sec-temporal.duration.prototype.with
fn temporal_duration_prototype_func_with_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let duration = this_duration(call.this_value(), "with")?;

    // Passo 3.a: `ToTemporalPartialDurationRecord` passo 1 lança `TypeError` para quem não é objeto.
    let duration_like = call.argument(0);
    if !duration_like.is_object() {
        return Err(Thrown::type_error("First argument to Temporal.Duration.prototype.with must be an object"));
    }

    // Passos 3 a 23: `with()` mescla o parcial nos campos existentes. Passo 24: `CreateTemporalDuration`.
    let result = duration.with(global_object, duration_like)?;
    Ok(create_temporal_duration(global_object, result, None)?.as_value())
}

/// `temporalDurationPrototypeFuncNegated`: https://tc39.es/proposal-temporal/#sec-temporal.duration.prototype.negated
fn temporal_duration_prototype_func_negated_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let duration = this_duration(call.this_value(), "negated")?;

    // Passo 3: `CreateNegatedTemporalDuration(duration)`. A negação preserva `IsValidDuration`, então não
    // passa por `createTemporalDuration`.
    Ok(TemporalDuration::create(global_object.vm(), &global_object.duration_structure(), -duration.duration()).as_value())
}

/// `temporalDurationPrototypeFuncAbs`: https://tc39.es/proposal-temporal/#sec-temporal.duration.prototype.abs
fn temporal_duration_prototype_func_abs_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let duration = this_duration(call.this_value(), "abs")?;

    // Passo 3: `CreateTemporalDuration(abs(years), ..., abs(nanoseconds))`. Todo campo não negativo é mais
    // estrito que `IsValidDuration`, então não passa por `createTemporalDuration`.
    Ok(TemporalDuration::create(global_object.vm(), &global_object.duration_structure(), abs_duration(&duration.duration())).as_value())
}

/// `add` e `subtract`: https://tc39.es/proposal-temporal/#sec-temporal.duration.prototype.add
fn add_or_subtract(global_object: &JSGlobalObject, call: &HostCall, operation: AddOrSubtract) -> HostResult {
    let member = if operation == AddOrSubtract::Add { "add" } else { "subtract" };
    let duration = this_duration(call.this_value(), member)?;

    // Passo 3: `AddDurations(operation, duration, other)`.
    let result = duration.add_durations(global_object, operation, call.argument(0))?;
    Ok(create_temporal_duration(global_object, result, None)?.as_value())
}

fn temporal_duration_prototype_func_add_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    add_or_subtract(global_object, call, AddOrSubtract::Add)
}

fn temporal_duration_prototype_func_subtract_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    add_or_subtract(global_object, call, AddOrSubtract::Subtract)
}

/// `temporalDurationPrototypeFuncRound`: https://tc39.es/proposal-temporal/#sec-temporal.duration.prototype.round
fn temporal_duration_prototype_func_round_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: marca.
    let duration = this_duration(call.this_value(), "round")?;

    // Passo 2: `roundTo` `undefined` é `TypeError`.
    let options = call.argument(0);
    if options.is_undefined() {
        return Err(Thrown::type_error("Temporal.Duration.prototype.round requires an options argument"));
    }

    // Passos 3 a 13: `GetDifferenceSettings` e `TemporalDurationRound`, depois `CreateTemporalDuration`.
    let result = duration.round(global_object, options)?;
    Ok(create_temporal_duration(global_object, result, None)?.as_value())
}

/// `temporalDurationPrototypeFuncTotal`: https://tc39.es/proposal-temporal/#sec-temporal.duration.prototype.total
fn temporal_duration_prototype_func_total_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passo 1: marca.
    let duration = this_duration(call.this_value(), "total")?;

    // Passo 2: `totalOf` `undefined` é `TypeError`.
    let options = call.argument(0);
    if options.is_undefined() {
        return Err(Thrown::type_error("Temporal.Duration.prototype.total requires an options argument"));
    }

    // Passos 3 a 10: `GetDifferenceSettings` e `TemporalDurationTotal`.
    Ok(js_number(duration.total(global_object, options)?))
}

/// `temporalDurationPrototypeFuncToString`: https://tc39.es/proposal-temporal/#sec-temporal.duration.prototype.tostring
fn temporal_duration_prototype_func_to_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let duration = this_duration(call.this_value(), "toString")?;

    // Passos 3 a 18: as opções, `RoundTimeDuration` e `TemporalDurationToString`.
    Ok(str_value(global_object.vm(), &duration.to_string_with_options(global_object, call.argument(0))?))
}

/// `temporalDurationPrototypeFuncToJSON`: https://tc39.es/proposal-temporal/#sec-temporal.duration.prototype.tojson
fn temporal_duration_prototype_func_to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let duration = this_duration(call.this_value(), "toJSON")?;

    // Passo 3: `TemporalDurationToString(duration, "auto")`, sem ler opções.
    Ok(str_value(global_object.vm(), &duration.to_string()))
}

/// `temporalDurationPrototypeFuncToLocaleString`: https://tc39.es/proposal-temporal/#sup-temporal.duration.prototype.tolocalestring
/// Passo 3 (ECMA-402): um `IntlDurationFormat` descartável com `locales` e `options`, depois `format`.
fn temporal_duration_prototype_func_to_locale_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let duration = this_duration(call.this_value(), "toLocaleString")?;
    let text = to_locale_string(global_object, call.argument(0), call.argument(1), &duration.duration())?;
    Ok(str_value(global_object.vm(), &text))
}

/// `temporalDurationPrototypeFuncValueOf`: https://tc39.es/proposal-temporal/#sec-temporal.duration.prototype.valueof
fn temporal_duration_prototype_func_value_of_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    // Passo 1: `Duration` não tem valor primitivo; use `Temporal.Duration.compare`.
    Err(Thrown::type_error("Temporal.Duration.prototype.valueOf must not be called. To compare Duration values, use Temporal.Duration.compare"))
}

host_function!(temporal_duration_prototype_func_with, temporal_duration_prototype_func_with_body);
host_function!(temporal_duration_prototype_func_negated, temporal_duration_prototype_func_negated_body);
host_function!(temporal_duration_prototype_func_abs, temporal_duration_prototype_func_abs_body);
host_function!(temporal_duration_prototype_func_add, temporal_duration_prototype_func_add_body);
host_function!(temporal_duration_prototype_func_subtract, temporal_duration_prototype_func_subtract_body);
host_function!(temporal_duration_prototype_func_round, temporal_duration_prototype_func_round_body);
host_function!(temporal_duration_prototype_func_total, temporal_duration_prototype_func_total_body);
host_function!(temporal_duration_prototype_func_to_string, temporal_duration_prototype_func_to_string_body);
host_function!(temporal_duration_prototype_func_to_json, temporal_duration_prototype_func_to_json_body);
host_function!(temporal_duration_prototype_func_to_locale_string, temporal_duration_prototype_func_to_locale_string_body);
host_function!(temporal_duration_prototype_func_value_of, temporal_duration_prototype_func_value_of_body);

/// `JSC_DEFINE_TEMPORAL_DURATION_UNIT_GETTER(name, capitalizedName)`: `Return 𝔽(duration.[[<Unit>]])`, com o
/// `TypeError` de marca. https://tc39.es/proposal-temporal/#sec-get-temporal.duration.prototype.years
macro_rules! duration_unit_getter {
    ($host:ident, $body:ident, $unit:expr, $name:literal) => {
        fn $body(_global_object: &JSGlobalObject, this_value: JSValue, _property_name: &PropertyName) -> HostResult {
            Ok(js_number(this_duration(this_value, $name)?.duration().field($unit)))
        }
        custom_getter!($host, $body);
    };
}

duration_unit_getter!(temporal_duration_prototype_getter_years, years_body, TemporalUnit::Year, "years");
duration_unit_getter!(temporal_duration_prototype_getter_months, months_body, TemporalUnit::Month, "months");
duration_unit_getter!(temporal_duration_prototype_getter_weeks, weeks_body, TemporalUnit::Week, "weeks");
duration_unit_getter!(temporal_duration_prototype_getter_days, days_body, TemporalUnit::Day, "days");
duration_unit_getter!(temporal_duration_prototype_getter_hours, hours_body, TemporalUnit::Hour, "hours");
duration_unit_getter!(temporal_duration_prototype_getter_minutes, minutes_body, TemporalUnit::Minute, "minutes");
duration_unit_getter!(temporal_duration_prototype_getter_seconds, seconds_body, TemporalUnit::Second, "seconds");
duration_unit_getter!(temporal_duration_prototype_getter_milliseconds, milliseconds_body, TemporalUnit::Millisecond, "milliseconds");
duration_unit_getter!(temporal_duration_prototype_getter_microseconds, microseconds_body, TemporalUnit::Microsecond, "microseconds");
duration_unit_getter!(temporal_duration_prototype_getter_nanoseconds, nanoseconds_body, TemporalUnit::Nanosecond, "nanoseconds");

/// `temporalDurationPrototypeGetterSign`: https://tc39.es/proposal-temporal/#sec-get-temporal.duration.prototype.sign
fn sign_body(_global_object: &JSGlobalObject, this_value: JSValue, _property_name: &PropertyName) -> HostResult {
    // Passo 3: `𝔽(! DurationSign(...))`.
    Ok(js_number(this_duration(this_value, "sign")?.sign()))
}
custom_getter!(temporal_duration_prototype_getter_sign, sign_body);

/// `temporalDurationPrototypeGetterBlank`: https://tc39.es/proposal-temporal/#sec-get-temporal.duration.prototype.blank
fn blank_body(_global_object: &JSGlobalObject, this_value: JSValue, _property_name: &PropertyName) -> HostResult {
    // Passos 3 a 5: `DurationSign == 0`.
    Ok(js_boolean(this_duration(this_value, "blank")?.sign() == 0))
}
custom_getter!(temporal_duration_prototype_getter_blank, blank_body);

/// `class TemporalDurationPrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct TemporalDurationPrototype;

impl TemporalDurationPrototype {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, TemporalDurationPrototype::STRUCTURE_FLAGS),
            &TEMPORAL_DURATION_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, structure)`: `TemporalDurationPrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        TemporalDurationPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm)`: só o `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`; os métodos e acessores de
    /// `durationPrototypeTable` reificam no primeiro acesso.
    fn finish_creation(prototype: &JSObject, vm: &VM, _global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);

        put_to_string_tag(vm, prototype, TEMPORAL_DURATION_PROTOTYPE_S_INFO.class_name);
    }
}
