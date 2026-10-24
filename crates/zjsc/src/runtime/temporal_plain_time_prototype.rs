//! Porte de `runtime/TemporalPlainTimePrototype.{h,cpp}`: `Temporal.PlainTime.prototype` (um `JSNonFinalObject`
//! com o `ClassInfo` `"Temporal.PlainTime"`): `add`, `subtract`, `with`, `until`, `since`, `round`, `equals`,
//! `toString`, `toJSON`, `toLocaleString`, `valueOf`, os acessores `hour` a `nanosecond`
//! (`DontEnum|ReadOnly|CustomAccessor`) e `@@toStringTag`.
//!
//! DIVERGÊNCIA: nenhuma em `toLocaleString`: ele delega ao `IntlDateTimeFormat` com `PlainTime`
//! (`intl_date_time_format/temporal.rs`, `CreateDateTimeFormat` com `~time~` e `FormatDateTime`).

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{put_to_string_tag};
use crate::runtime::lookup::{HashTable, HashTableValue};
use crate::runtime::lookup::{custom_getter_entry, native_entry};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_date_time_format::temporal::to_locale_string as intl_to_locale_string;
use crate::runtime::intl_date_time_format::{Defaults, Required};
use crate::runtime::intl_support::{get_options_object, str_value};
use crate::runtime::iso8601::Duration;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, js_number, js_undefined, JSValue};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::temporal_duration::{create_temporal_duration, TemporalDuration};
use crate::runtime::temporal_object::{
    is_partial_temporal_object, to_temporal_overflow, AddOrSubtract, DifferenceOperation, TemporalUnit,
};
use crate::runtime::temporal_plain_time::{validate_and_create_time_record, TemporalPlainTime, TemporalPlainTimeRef};
use crate::runtime::vm::VM;

/// `const ClassInfo TemporalPlainTimePrototype::s_info` (`"Temporal.PlainTime"`).
pub static TEMPORAL_PLAIN_TIME_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "Temporal.PlainTime",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&PROTOTYPE_TABLE), inherits_js_type_range: None,
};

/// O `dynamicDowncast<TemporalPlainTime>(callFrame->thisValue())` com o `TypeError` de marca de cada membro.
fn this_plain_time(this_value: JSValue, member: &str) -> Result<TemporalPlainTimeRef, Thrown> {
    TemporalPlainTime::from_value(&this_value)
        .ok_or_else(|| Thrown::TypeError(format!("Temporal.PlainTime.prototype.{member} called on value that's not a PlainTime")))
}

/// `AddDurationToTime(operation, this, temporalDurationLike)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-adddurationtotime
fn add_or_subtract(global_object: &JSGlobalObject, call: &HostCall, operation: AddOrSubtract) -> HostResult {
    // Passos 1 e 2 de `add` e `subtract`: marca.
    let member = if operation == AddOrSubtract::Add { "add" } else { "subtract" };
    let plain_time = this_plain_time(call.this_value(), member)?;

    // Passo 1 de `AddDurationToTime`: `ToTemporalDuration`. Passo 2: `subtract` nega (`CreateNegatedTemporalDuration`).
    let mut duration = TemporalDuration::to_temporal_duration_record(global_object, call.argument(0))?;
    if operation == AddOrSubtract::Subtract {
        duration = -duration;
    }

    // Passos 3 a 5: `AddTime` ignora os campos de data; `! CreateTemporalTime(result)`.
    let result = validate_and_create_time_record(&TemporalPlainTime::add_time(plain_time.plain_time(), &duration))?;
    Ok(TemporalPlainTime::create(global_object.vm(), &global_object.plain_time_structure(), result).as_value())
}

fn temporal_plain_time_prototype_func_add_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    add_or_subtract(global_object, call, AddOrSubtract::Add)
}

fn temporal_plain_time_prototype_func_subtract_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    add_or_subtract(global_object, call, AddOrSubtract::Subtract)
}

/// `temporalPlainTimePrototypeFuncWith`: https://tc39.es/proposal-temporal/#sec-temporal.plaintime.prototype.with
fn temporal_plain_time_prototype_func_with_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let plain_time = this_plain_time(call.this_value(), "with")?;

    let temporal_time_like = call.argument(0);
    if !temporal_time_like.is_object() {
        return Err(Thrown::type_error("First argument to Temporal.PlainTime.prototype.with must be an object"));
    }

    // Passo 3: `IsPartialTemporalObject(temporalTimeLike)` falso é `TypeError`.
    if !is_partial_temporal_object(global_object, temporal_time_like)? {
        return Err(Thrown::type_error("argument must be a partial Temporal object"));
    }

    // Passo 4: `ToTemporalTimeRecord(temporalTimeLike, ~partial~)`.
    let partial_time = TemporalPlainTime::to_partial_time(global_object, temporal_time_like, false)?;

    // Passo 17: `GetOptionsObject(options)`. Passo 18: `GetTemporalOverflowOption`.
    let options = get_options_object(call.argument(1))?;
    let overflow = to_temporal_overflow(global_object, options)?;

    // Passos 5 a 16 (por campo): o campo parcial, quando existe, senão o do `this`.
    let current = plain_time.plain_time();
    let current_fields = [current.hour(), current.minute(), current.second(), current.millisecond(), current.microsecond(), current.nanosecond()];
    let mut duration = Duration::default();
    for (index, unit) in
        [TemporalUnit::Hour, TemporalUnit::Minute, TemporalUnit::Second, TemporalUnit::Millisecond, TemporalUnit::Microsecond, TemporalUnit::Nanosecond]
            .into_iter()
            .enumerate()
    {
        duration.set_field(unit, partial_time[index].unwrap_or(f64::from(current_fields[index])));
    }

    // Passo 19: `RegulateTime`. Passo 20: `! CreateTemporalTime(result)`.
    let result = TemporalPlainTime::regulate_time(&duration, overflow)?;
    Ok(TemporalPlainTime::create(global_object.vm(), &global_object.plain_time_structure(), result).as_value())
}

/// `until` e `since`: `DifferenceTemporalPlainTime(operation, this, other, options)`.
fn difference(global_object: &JSGlobalObject, call: &HostCall, operation: DifferenceOperation) -> HostResult {
    let member = if operation == DifferenceOperation::Since { "since" } else { "until" };
    let plain_time = this_plain_time(call.this_value(), member)?;

    // Passo 1 de `DifferenceTemporalPlainTime`: `ToTemporalTime(other)`.
    let other = TemporalPlainTime::from(global_object, call.argument(0), js_undefined())?;

    let result = plain_time.difference_temporal_plain_time(operation, global_object, &other, call.argument(1))?;
    Ok(create_temporal_duration(global_object, result, None)?.as_value())
}

/// `temporalPlainTimePrototypeFuncUntil`: https://tc39.es/proposal-temporal/#sec-temporal.plaintime.prototype.until
fn temporal_plain_time_prototype_func_until_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    difference(global_object, call, DifferenceOperation::Until)
}

/// `temporalPlainTimePrototypeFuncSince`: https://tc39.es/proposal-temporal/#sec-temporal.plaintime.prototype.since
fn temporal_plain_time_prototype_func_since_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    difference(global_object, call, DifferenceOperation::Since)
}

/// `temporalPlainTimePrototypeFuncRound`: https://tc39.es/proposal-temporal/#sec-temporal.plaintime.prototype.round
fn temporal_plain_time_prototype_func_round_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let plain_time = this_plain_time(call.this_value(), "round")?;

    // Passo 3: `roundTo` `undefined` é `TypeError`.
    let options = call.argument(0);
    if options.is_undefined() {
        return Err(Thrown::type_error("Temporal.PlainTime.prototype.round requires an options argument"));
    }

    let rounded = plain_time.round(global_object, options)?;
    Ok(TemporalPlainTime::create(global_object.vm(), &global_object.plain_time_structure(), rounded).as_value())
}

/// `temporalPlainTimePrototypeFuncEquals`: https://tc39.es/proposal-temporal/#sec-temporal.plaintime.prototype.equals
fn temporal_plain_time_prototype_func_equals_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let plain_time = this_plain_time(call.this_value(), "equals")?;

    // Passo 3: `other = ? ToTemporalTime(other)`.
    let other = TemporalPlainTime::from(global_object, call.argument(0), js_undefined())?;

    // Passos 4 e 5: `CompareTimeRecord = 0` é a igualdade dos seis campos.
    Ok(js_boolean(plain_time.plain_time() == other.plain_time()))
}

/// `temporalPlainTimePrototypeFuncToString`: https://tc39.es/proposal-temporal/#sec-temporal.plaintime.prototype.tostring
fn temporal_plain_time_prototype_func_to_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let plain_time = this_plain_time(call.this_value(), "toString")?;

    // Passos 3 a 12: as opções, `RoundTime` e `TimeRecordToString`.
    Ok(str_value(global_object.vm(), &plain_time.to_string_with_options(global_object, call.argument(0))?))
}

/// `temporalPlainTimePrototypeFuncToJSON`: https://tc39.es/proposal-temporal/#sec-temporal.plaintime.prototype.tojson
fn temporal_plain_time_prototype_func_to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let plain_time = this_plain_time(call.this_value(), "toJSON")?;

    // Passo 3: `TimeRecordToString(this.[[Time]], ~auto~)`.
    Ok(str_value(global_object.vm(), &plain_time.to_string()))
}

/// `temporalPlainTimePrototypeFuncToLocaleString`: https://tc39.es/proposal-temporal/#sup-temporal.plaintime.prototype.tolocalestring
fn temporal_plain_time_prototype_func_to_locale_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    this_plain_time(call.this_value(), "toLocaleString")?;
    let text = intl_to_locale_string(global_object, call.argument(0), call.argument(1), Required::Time, Defaults::Time, call.this_value())?;
    Ok(str_value(global_object.vm(), &text))
}

/// `temporalPlainTimePrototypeFuncValueOf`: https://tc39.es/proposal-temporal/#sec-temporal.plaintime.prototype.valueof
fn temporal_plain_time_prototype_func_value_of_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    // Passo 1: `PlainTime` não tem valor primitivo; use `Temporal.PlainTime.compare`.
    Err(Thrown::type_error("Temporal.PlainTime.prototype.valueOf must not be called. To compare PlainTime values, use Temporal.PlainTime.compare"))
}

host_function!(temporal_plain_time_prototype_func_add, temporal_plain_time_prototype_func_add_body);
host_function!(temporal_plain_time_prototype_func_subtract, temporal_plain_time_prototype_func_subtract_body);
host_function!(temporal_plain_time_prototype_func_with, temporal_plain_time_prototype_func_with_body);
host_function!(temporal_plain_time_prototype_func_until, temporal_plain_time_prototype_func_until_body);
host_function!(temporal_plain_time_prototype_func_since, temporal_plain_time_prototype_func_since_body);
host_function!(temporal_plain_time_prototype_func_round, temporal_plain_time_prototype_func_round_body);
host_function!(temporal_plain_time_prototype_func_equals, temporal_plain_time_prototype_func_equals_body);
host_function!(temporal_plain_time_prototype_func_to_string, temporal_plain_time_prototype_func_to_string_body);
host_function!(temporal_plain_time_prototype_func_to_json, temporal_plain_time_prototype_func_to_json_body);
host_function!(temporal_plain_time_prototype_func_to_locale_string, temporal_plain_time_prototype_func_to_locale_string_body);
host_function!(temporal_plain_time_prototype_func_value_of, temporal_plain_time_prototype_func_value_of_body);

// `temporalPlainTimePrototypeGetterHour` e irmãos: o campo do `[[Time]]`, com o `TypeError` de marca, pelo
// `temporal_getter!` de `temporal_object.rs`.
// https://tc39.es/proposal-temporal/#sec-get-temporal.plaintime.prototype.hour
crate::temporal_getter!(temporal_plain_time_prototype_getter_hour, hour_body, this_plain_time, "hour", |_global, time| js_number(time.plain_time().hour()));
crate::temporal_getter!(temporal_plain_time_prototype_getter_minute, minute_body, this_plain_time, "minute", |_global, time| js_number(time.plain_time().minute()));
crate::temporal_getter!(temporal_plain_time_prototype_getter_second, second_body, this_plain_time, "second", |_global, time| js_number(time.plain_time().second()));
crate::temporal_getter!(temporal_plain_time_prototype_getter_millisecond, millisecond_body, this_plain_time, "millisecond", |_global, time| js_number(time.plain_time().millisecond()));
crate::temporal_getter!(temporal_plain_time_prototype_getter_microsecond, microsecond_body, this_plain_time, "microsecond", |_global, time| js_number(time.plain_time().microsecond()));
crate::temporal_getter!(temporal_plain_time_prototype_getter_nanosecond, nanosecond_body, this_plain_time, "nanosecond", |_global, time| js_number(time.plain_time().nanosecond()));

/// `plain timePrototypeTableValues`, na ordem do `@begin`: os métodos (`DontEnum|Function`) e os acessores
/// (`DontEnum|ReadOnly|CustomAccessor`, sem setter).
static PROTOTYPE_TABLE_VALUES: [HashTableValue; 17] = [
    native_entry("add", temporal_plain_time_prototype_func_add, 1),
    native_entry("subtract", temporal_plain_time_prototype_func_subtract, 1),
    native_entry("with", temporal_plain_time_prototype_func_with, 1),
    native_entry("until", temporal_plain_time_prototype_func_until, 1),
    native_entry("since", temporal_plain_time_prototype_func_since, 1),
    native_entry("round", temporal_plain_time_prototype_func_round, 1),
    native_entry("equals", temporal_plain_time_prototype_func_equals, 1),
    native_entry("toString", temporal_plain_time_prototype_func_to_string, 0),
    native_entry("toJSON", temporal_plain_time_prototype_func_to_json, 0),
    native_entry("toLocaleString", temporal_plain_time_prototype_func_to_locale_string, 0),
    native_entry("valueOf", temporal_plain_time_prototype_func_value_of, 0),
    custom_getter_entry("hour", temporal_plain_time_prototype_getter_hour),
    custom_getter_entry("minute", temporal_plain_time_prototype_getter_minute),
    custom_getter_entry("second", temporal_plain_time_prototype_getter_second),
    custom_getter_entry("millisecond", temporal_plain_time_prototype_getter_millisecond),
    custom_getter_entry("microsecond", temporal_plain_time_prototype_getter_microsecond),
    custom_getter_entry("nanosecond", temporal_plain_time_prototype_getter_nanosecond),
];

/// A tabela estática do protótipo.
static PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &PROTOTYPE_TABLE_VALUES };

/// `class TemporalPlainTimePrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct TemporalPlainTimePrototype;

impl TemporalPlainTimePrototype {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`; sem `HasStaticPropertyTable` porque os
    /// membros são postos direto em `finishCreation` (a tabela estática do C++ é essa mesma lista).
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | crate::runtime::js_type_info::HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, TemporalPlainTimePrototype::STRUCTURE_FLAGS),
            &TEMPORAL_PLAIN_TIME_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `TemporalPlainTimePrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        TemporalPlainTimePrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm)` com `plainTimePrototypeTable`: os métodos (`DontEnum|Function`, com o comprimento
    /// da tabela), os acessores (`DontEnum|ReadOnly|CustomAccessor`) e `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
    fn finish_creation(prototype: &JSObject, vm: &VM, _global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);

        put_to_string_tag(vm, prototype, TEMPORAL_PLAIN_TIME_PROTOTYPE_S_INFO.class_name);
    }
}
