//! Porte de `runtime/TemporalPlainDateTimePrototype.{h,cpp}`: `Temporal.PlainDateTime.prototype` (um
//! `JSNonFinalObject` com o `ClassInfo` `"Temporal.PlainDateTime"`): `add`, `subtract`, `until`, `since`, `with`,
//! `withCalendar`, `withPlainTime`, `round`, `equals`, `toPlainDate`, `toPlainTime`, `toString`, `toJSON`,
//! `toLocaleString`, `valueOf`, os acessores `calendarId` a `eraYear` (`DontEnum|ReadOnly|CustomAccessor`) e
//! `@@toStringTag`.
//!
//! DIVERGÊNCIAS:
//! - `toLocaleString` delega ao `IntlDateTimeFormat` com `PlainDateTime` (`intl_date_time_format/temporal.rs`).
//! - Os acessores despacham pelo calendário da célula (`calendar_fields`); `weekOfYear` e `yearOfWeek` são
//!   `undefined` fora do ISO (`calendarWeekOfYear`), `dayOfWeek` é o ISO em todo calendário.
//! - `add` soma a data com `calendar_date_add`; `until` e `since` passam o calendário da célula ao núcleo.
//! - `until` e `since` chamam `difference_temporal_plain_date_time` com a operação, no lugar do template do C++.

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{put_to_string_tag};
use crate::runtime::lookup::{HashTable, HashTableValue};
use crate::runtime::lookup::{custom_getter_entry, native_entry};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_date_time_format::temporal::to_locale_string as intl_to_locale_string;
use crate::runtime::intl_date_time_format::{Defaults, Required};
use crate::runtime::intl_support::{get_options_object, str_value, to_rust_string};
use crate::runtime::iso8601::{day_of_week, week_of_year, year_of_week, Duration, DAYS_PER_WEEK};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, js_number, js_undefined, JSValue};
use crate::runtime::temporal_calendar_icu::{calendar_date_add, calendar_fields};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::temporal_calendar::{
    calendar_id_to_string, calendar_is_iso, interpret_temporal_date_time_fields, read_calendar_fields_from_object, to_temporal_calendar_identifier,
    FieldSetType,
};
use crate::runtime::temporal_core_calendar_fields::{calendar_merge_fields, iso_date_to_fields, ResolveType, TimeFieldsIn};
use crate::runtime::temporal_core_iso_date::round_iso_date_time;
use crate::runtime::temporal_core_rounding::{maximum_rounding_increment, validate_temporal_rounding_increment};
use crate::runtime::temporal_duration::{create_temporal_duration, TemporalDuration};
use crate::runtime::temporal_object::{
    is_calendar_unit, is_partial_temporal_object, length_in_nanoseconds, temporal_rounding_increment, temporal_rounding_mode,
    temporal_unit_type, temporal_unit_valued, to_temporal_overflow_value, validate_temporal_unit_value, AllowedUnit, DifferenceOperation,
    Inclusivity, RoundingMode, TemporalUnit, TemporalUnitDefault, UnitGroup, UnitOption, to_temporal_disambiguation,
};
use crate::runtime::temporal_time_zone::get_epoch_nanoseconds_for;
use crate::runtime::temporal_zoned_date_time::{create_temporal_zoned_date_time, to_temporal_time_zone_identifier};
use crate::runtime::temporal_plain_date::TemporalPlainDate;
use crate::runtime::temporal_plain_date_time::{create_temporal_date_time, TemporalPlainDateTime, TemporalPlainDateTimeRef};
use crate::runtime::temporal_plain_time::{validate_and_create_time_record, TemporalPlainTime};
use crate::runtime::vm::VM;

/// `const ClassInfo TemporalPlainDateTimePrototype::s_info` (`"Temporal.PlainDateTime"`).
pub static TEMPORAL_PLAIN_DATE_TIME_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "Temporal.PlainDateTime",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&PROTOTYPE_TABLE), inherits_js_type_range: None,
};

/// O `dynamicDowncast<TemporalPlainDateTime>(callFrame->thisValue())` com o `TypeError` de marca de cada membro.
fn this_plain_date_time(this_value: JSValue, member: &str) -> Result<TemporalPlainDateTimeRef, Thrown> {
    TemporalPlainDateTime::from_value(&this_value)
        .ok_or_else(|| Thrown::TypeError(format!("Temporal.PlainDateTime.prototype.{member} called on value that's not a PlainDateTime")))
}

/// `addDurationToPlainDateTime(globalObject, scope, plainDateTime, duration, optionsArg)` (`AddDurationToDateTime`):
/// https://tc39.es/proposal-temporal/#sec-temporal-adddurationtodatetime
/// Quem chama faz os passos 1 e 2 (`ToTemporalDuration` e a negação de `subtract`).
fn add_duration_to_plain_date_time(
    global_object: &JSGlobalObject,
    plain_date_time: &TemporalPlainDateTime,
    duration: Duration,
    options_arg: JSValue,
) -> HostResult {
    // Passos 4 e 5: `GetOptionsObject` e `GetTemporalOverflowOption`.
    let overflow = to_temporal_overflow_value(global_object, options_arg)?;

    // Passo 6: `timeResult = AddTime(time, internalDuration.[[Time]])`; o excesso de dias fica em `[[Days]]`.
    let balanced_time_duration = TemporalPlainTime::add_time(plain_date_time.plain_time(), &duration);
    let plain_time = validate_and_create_time_record(&balanced_time_duration)?;

    // Passo 7: `dateDuration = AdjustDateDurationRecord(internalDuration.[[Date]], timeResult.[[Days]])`.
    let date_duration = Duration::new(
        duration.years(),
        duration.months(),
        duration.weeks(),
        duration.days() + balanced_time_duration.days(),
        0,
        0,
        0,
        0,
        0,
        0,
    );

    // Passo 8: `addedDate = ? CalendarDateAdd(calendar, isoDate, dateDuration, overflow)`.
    let plain_date = calendar_date_add(plain_date_time.calendar_id(), plain_date_time.plain_date(), &date_duration, overflow)?;

    // Passos 9 e 10: `CombineISODateAndTimeRecord` e `CreateTemporalDateTime`.
    Ok(create_temporal_date_time(global_object, plain_date, plain_time, plain_date_time.calendar_id(), None)?.as_value())
}

/// `temporalPlainDateTimePrototypeFuncAdd`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.prototype.add
fn add_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date_time = this_plain_date_time(call.this_value(), "add")?;
    // Passo 3: `AddDurationToDateTime(~add~, ...)`.
    let duration = TemporalDuration::to_temporal_duration_record(global_object, call.argument(0))?;
    add_duration_to_plain_date_time(global_object, &plain_date_time, duration, call.argument(1))
}

/// `temporalPlainDateTimePrototypeFuncSubtract`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.prototype.subtract
fn subtract_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date_time = this_plain_date_time(call.this_value(), "subtract")?;
    // Passo 3: `AddDurationToDateTime(~subtract~, ...)`; `-duration` é `CreateNegatedTemporalDuration`.
    let duration = TemporalDuration::to_temporal_duration_record(global_object, call.argument(0))?;
    add_duration_to_plain_date_time(global_object, &plain_date_time, -duration, call.argument(1))
}

/// `until` e `since`: `DifferenceTemporalPlainDateTime(operation, this, other, options)`.
fn difference(global_object: &JSGlobalObject, call: &HostCall, operation: DifferenceOperation) -> HostResult {
    // Passos 1 e 2: marca.
    let member = if operation == DifferenceOperation::Since { "since" } else { "until" };
    let plain_date_time = this_plain_date_time(call.this_value(), member)?;
    // Passo 1 de `DifferenceTemporalPlainDateTime`: `ToTemporalDateTime(other)`.
    let other = TemporalPlainDateTime::from(global_object, call.argument(0), JSValue::Undefined)?;
    let result = plain_date_time.difference_temporal_plain_date_time(global_object, operation, &other, call.argument(1))?;
    Ok(create_temporal_duration(global_object, result, None)?.as_value())
}

/// `temporalPlainDateTimePrototypeFuncUntil`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.prototype.until
fn until_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    difference(global_object, call, DifferenceOperation::Until)
}

/// `temporalPlainDateTimePrototypeFuncSince`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.prototype.since
fn since_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    difference(global_object, call, DifferenceOperation::Since)
}

/// `temporalPlainDateTimePrototypeFuncWith`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.prototype.with
fn with_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date_time = this_plain_date_time(call.this_value(), "with")?;

    // Passo 3: `IsPartialTemporalObject(temporalDateTimeLike)` falso é `TypeError`.
    let fields = call.argument(0);
    if !is_partial_temporal_object(global_object, fields)? {
        return Err(Thrown::type_error("First argument to Temporal.PlainDateTime.prototype.with must be a partial Temporal object"));
    }

    // Passo 4: `calendar = plainDateTime.[[Calendar]]`.
    let calendar_id = plain_date_time.calendar_id();

    // Passo 12: `PrepareCalendarFields`, todos os campos numa passada em ordem alfabética.
    let mut partial_time = TimeFieldsIn::default();
    let partial_date = read_calendar_fields_from_object(global_object, fields, calendar_id, FieldSetType::DateTime, Some(&mut partial_time))?;

    // `~partial~` lança `TypeError` se nenhum dos campos pedidos veio com valor.
    let any_field_set = partial_date.day.is_some()
        || partial_date.era.is_some()
        || partial_date.era_year.is_some()
        || partial_date.month.is_some()
        || partial_date.month_code.is_some()
        || partial_date.year.is_some()
        || partial_time.hour.is_some()
        || partial_time.minute.is_some()
        || partial_time.second.is_some()
        || partial_time.millisecond.is_some()
        || partial_time.microsecond.is_some()
        || partial_time.nanosecond.is_some();
    if !any_field_set {
        return Err(Thrown::type_error("at least one field must be provided"));
    }

    // Passos 14 e 15: `GetOptionsObject` e `GetTemporalOverflowOption`.
    let overflow = to_temporal_overflow_value(global_object, call.argument(1))?;

    // Passo 5: `fields = ISODateToFields(calendar, isoDate, ~date~)`. Passo 13: `CalendarMergeFields`.
    let date_fields = iso_date_to_fields(calendar_id, plain_date_time.plain_date(), ResolveType::Date)?;
    let merged_date = calendar_merge_fields(calendar_id, &date_fields, &partial_date);

    // Passos 6 a 11, fundidos no 13: cada campo de hora é o parcial, se existe, senão o do `this`.
    let current = plain_date_time.plain_time();
    let merged_time = TimeFieldsIn {
        hour: Some(partial_time.hour.unwrap_or(f64::from(current.hour()))),
        minute: Some(partial_time.minute.unwrap_or(f64::from(current.minute()))),
        second: Some(partial_time.second.unwrap_or(f64::from(current.second()))),
        millisecond: Some(partial_time.millisecond.unwrap_or(f64::from(current.millisecond()))),
        microsecond: Some(partial_time.microsecond.unwrap_or(f64::from(current.microsecond()))),
        nanosecond: Some(partial_time.nanosecond.unwrap_or(f64::from(current.nanosecond()))),
    };

    // Passo 16: `InterpretTemporalDateTimeFields(calendar, fields, overflow)`.
    let result = interpret_temporal_date_time_fields(calendar_id, &merged_date, &merged_time, overflow)?;

    // Passo 17: `CreateTemporalDateTime(result, calendar)`.
    Ok(create_temporal_date_time(global_object, result.date, result.time, calendar_id, None)?.as_value())
}

/// `temporalPlainDateTimePrototypeFuncWithCalendar`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.prototype.withcalendar
fn with_calendar_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date_time = this_plain_date_time(call.this_value(), "withCalendar")?;

    // Passo 3: `calendar = ? ToTemporalCalendarIdentifier(calendarLike)`.
    let new_calendar_id = to_temporal_calendar_identifier(global_object, call.argument(0))?;

    // Passo 4: `CreateTemporalDateTime(this.[[ISODateTime]], calendar)`; o C++ cria direto, sem repetir o limite.
    Ok(TemporalPlainDateTime::create(
        global_object.vm(),
        &global_object.plain_date_time_structure(),
        plain_date_time.plain_date(),
        plain_date_time.plain_time(),
        new_calendar_id,
    )
    .as_value())
}

/// `temporalPlainDateTimePrototypeFuncWithPlainTime`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.prototype.withplaintime
fn with_plain_time_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date_time = this_plain_date_time(call.this_value(), "withPlainTime")?;

    // Passo 3: `ToTimeRecordOrMidnight(plainTimeLike)`; `undefined` é a meia-noite.
    let plain_time_like = call.argument(0);
    let plain_time = if plain_time_like.is_undefined() {
        Default::default()
    } else {
        TemporalPlainTime::from(global_object, plain_time_like, JSValue::Undefined)?.plain_time()
    };

    // Passos 4 e 5: `CombineISODateAndTimeRecord` e `CreateTemporalDateTime`.
    Ok(create_temporal_date_time(global_object, plain_date_time.plain_date(), plain_time, plain_date_time.calendar_id(), None)?.as_value())
}

/// `temporalPlainDateTimePrototypeFuncRound`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.prototype.round
fn round_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date_time = this_plain_date_time(call.this_value(), "round")?;

    // Passo 3: `roundTo` `undefined` é `TypeError`.
    let round_to = call.argument(0);
    if round_to.is_undefined() {
        return Err(Thrown::type_error("Temporal.PlainDateTime.prototype.round requires a roundTo option"));
    }

    let mut options: Option<JSValue> = None;
    let mut smallest: Option<TemporalUnit> = None;
    if round_to.is_string() {
        // Passo 4: `roundTo` `String` é `{ smallestUnit: <string> }`, decodificado direto.
        let string = to_rust_string(global_object, round_to)?;
        let Some(unit) = temporal_unit_type(&string) else {
            return Err(Thrown::range_error("smallestUnit is an invalid Temporal unit"));
        };
        smallest = Some(unit);
    } else {
        // Passo 5: `GetOptionsObject(roundTo)`.
        options = get_options_object(round_to)?;
    }

    // Passos 6 e 7: as opções em ordem alfabética; `GetRoundingIncrementOption`.
    let rounding_increment = temporal_rounding_increment(global_object, options)?;
    // Passo 8: `GetRoundingModeOption(roundTo, ~half-expand~)`.
    let rounding_mode = temporal_rounding_mode(global_object, options, RoundingMode::HalfExpand)?;

    let smallest_unit = match smallest {
        None => {
            // Passos 9 e 10: `GetTemporalUnitValuedOption(roundTo, "smallestUnit", ~required~)` e
            // `ValidateTemporalUnitValue(smallestUnit, ~time~, « ~day~ »)`.
            let smallest_maybe_auto = temporal_unit_valued(global_object, options, "smallestUnit", TemporalUnitDefault::Required)?;
            validate_temporal_unit_value(smallest_maybe_auto, UnitGroup::Time, AllowedUnit::Day, "smallestUnit")?;
            let UnitOption::Unit(unit) = smallest_maybe_auto else {
                unreachable!("smallestUnit é obrigatório e `auto` não passa pela validação");
            };
            unit
        }
        Some(unit) => {
            // Passo 10 (caminho da `String`): ano, mês e semana são recusados; dia e as de tempo passam.
            if is_calendar_unit(unit) {
                return Err(Thrown::range_error("smallestUnit is a disallowed unit"));
            }
            unit
        }
    };

    // Passos 11 e 12: `day` é máximo 1 inclusivo; as outras, `MaximumTemporalDurationRoundingIncrement` exclusivo.
    let (maximum, inclusivity) = if smallest_unit == TemporalUnit::Day {
        (1.0, Inclusivity::Inclusive)
    } else {
        (maximum_rounding_increment(smallest_unit).map_or(1.0, f64::from), Inclusivity::Exclusive)
    };
    // Passo 13: `ValidateTemporalRoundingIncrement(roundingIncrement, maximum, inclusive)`.
    validate_temporal_rounding_increment(rounding_increment, Some(maximum), inclusivity)?;

    // Passos 14 e 15: `RoundISODateTime(isoDateTime, roundingIncrement, smallestUnit, roundingMode)`.
    let increment_ns = length_in_nanoseconds(smallest_unit) * (rounding_increment as i128);
    let (rounded_date, rounded_time) =
        round_iso_date_time(plain_date_time.plain_date(), plain_date_time.plain_time(), increment_ns, smallest_unit, rounding_mode);

    // Passo 16: `CreateTemporalDateTime(result, calendar)`.
    Ok(create_temporal_date_time(global_object, rounded_date, rounded_time, plain_date_time.calendar_id(), None)?.as_value())
}

/// `temporalPlainDateTimePrototypeFuncEquals`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.prototype.equals
fn equals_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date_time = this_plain_date_time(call.this_value(), "equals")?;
    // Passo 3: `other = ? ToTemporalDateTime(other)`.
    let other = TemporalPlainDateTime::from(global_object, call.argument(0), JSValue::Undefined)?;
    // Passo 4: `CompareISODateTime` diferente de 0 é falso.
    if plain_date_time.plain_date() != other.plain_date() || plain_date_time.plain_time() != other.plain_time() {
        return Ok(js_boolean(false));
    }
    // Passo 5: `CalendarEquals`.
    Ok(js_boolean(plain_date_time.calendar_id() == other.calendar_id()))
}

/// `temporalPlainDateTimePrototypeFuncToPlainDate`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.prototype.toplaindate
fn to_plain_date_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let plain_date_time = this_plain_date_time(call.this_value(), "toPlainDate")?;
    Ok(TemporalPlainDate::create(
        global_object.vm(),
        &global_object.plain_date_structure(),
        plain_date_time.plain_date(),
        plain_date_time.calendar_id(),
    )
    .as_value())
}

/// `temporalPlainDateTimePrototypeFuncToPlainTime`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.prototype.toplaintime
fn to_plain_time_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let plain_date_time = this_plain_date_time(call.this_value(), "toPlainTime")?;
    Ok(TemporalPlainTime::create(global_object.vm(), &global_object.plain_time_structure(), plain_date_time.plain_time()).as_value())
}

/// `temporalPlainDateTimePrototypeFuncToZonedDateTime`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.prototype.tozoneddatetime
fn to_zoned_date_time_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date_time = this_plain_date_time(call.this_value(), "toZonedDateTime")?;
    // Passo 3: `timeZone = ? ToTemporalTimeZoneIdentifier(temporalTimeZoneLike)`.
    let time_zone = to_temporal_time_zone_identifier(global_object, call.argument(0))?;
    // Passos 4 e 5: `GetOptionsObject(options)` e `GetTemporalDisambiguationOption`.
    let options = get_options_object(call.argument(1))?;
    let disambiguation = to_temporal_disambiguation(global_object, options)?;
    // Passo 6: `epochNs = ? GetEpochNanosecondsFor(timeZone, dateTime.[[ISODateTime]], disambiguation)`.
    let exact_time = get_epoch_nanoseconds_for(&time_zone, plain_date_time.plain_date(), plain_date_time.plain_time(), disambiguation)?;
    // Passo 7: `CreateTemporalZonedDateTime(epochNs, timeZone, calendar)`.
    Ok(create_temporal_zoned_date_time(global_object, exact_time, time_zone, plain_date_time.calendar_id(), None)?.as_value())
}

/// `temporalPlainDateTimePrototypeFuncToString`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.prototype.tostring
fn to_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let plain_date_time = this_plain_date_time(call.this_value(), "toString")?;
    Ok(str_value(global_object.vm(), &plain_date_time.to_string_with_options(global_object, call.argument(0))?))
}

/// `temporalPlainDateTimePrototypeFuncToJSON`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.prototype.tojson
fn to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let plain_date_time = this_plain_date_time(call.this_value(), "toJSON")?;
    Ok(str_value(global_object.vm(), &plain_date_time.to_string()))
}

/// `temporalPlainDateTimePrototypeFuncToLocaleString`: https://tc39.es/proposal-temporal/#sup-temporal.plaindatetime.prototype.tolocalestring
fn to_locale_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    this_plain_date_time(call.this_value(), "toLocaleString")?;
    let text = intl_to_locale_string(global_object, call.argument(0), call.argument(1), Required::Any, Defaults::All, call.this_value())?;
    Ok(str_value(global_object.vm(), &text))
}

/// `temporalPlainDateTimePrototypeFuncValueOf`: https://tc39.es/proposal-temporal/#sec-temporal.plaindatetime.prototype.valueof
fn value_of_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error(
        "Temporal.PlainDateTime.prototype.valueOf must not be called. To compare PlainDateTime values, use Temporal.PlainDateTime.compare",
    ))
}

host_function!(temporal_plain_date_time_prototype_func_add, add_body);
host_function!(temporal_plain_date_time_prototype_func_subtract, subtract_body);
host_function!(temporal_plain_date_time_prototype_func_until, until_body);
host_function!(temporal_plain_date_time_prototype_func_since, since_body);
host_function!(temporal_plain_date_time_prototype_func_with, with_body);
host_function!(temporal_plain_date_time_prototype_func_with_calendar, with_calendar_body);
host_function!(temporal_plain_date_time_prototype_func_with_plain_time, with_plain_time_body);
host_function!(temporal_plain_date_time_prototype_func_round, round_body);
host_function!(temporal_plain_date_time_prototype_func_equals, equals_body);
host_function!(temporal_plain_date_time_prototype_func_to_plain_date, to_plain_date_body);
host_function!(temporal_plain_date_time_prototype_func_to_plain_time, to_plain_time_body);
host_function!(temporal_plain_date_time_prototype_func_to_zoned_date_time, to_zoned_date_time_body);
host_function!(temporal_plain_date_time_prototype_func_to_string, to_string_body);
host_function!(temporal_plain_date_time_prototype_func_to_json, to_json_body);
host_function!(temporal_plain_date_time_prototype_func_to_locale_string, to_locale_string_body);
host_function!(temporal_plain_date_time_prototype_func_value_of, value_of_body);

// `JSC_DEFINE_CUSTOM_GETTER(temporalPlainDateTimePrototypeGetterX, ...)` é o `temporal_getter!` de `temporal_object.rs`.

crate::temporal_getter!(temporal_plain_date_time_prototype_getter_calendar_id, calendar_id_body, this_plain_date_time, "calendarId", |global, cell| str_value(
    global.vm(),
    calendar_id_to_string(cell.calendar_id())
));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_year, year_body, this_plain_date_time, "year", |_global, cell| js_number(calendar_fields(cell.calendar_id(), &cell.plain_date()).year));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_month, month_body, this_plain_date_time, "month", |_global, cell| js_number(calendar_fields(cell.calendar_id(), &cell.plain_date()).month));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_month_code, month_code_body, this_plain_date_time, "monthCode", |global, cell| str_value(
    global.vm(),
    &calendar_fields(cell.calendar_id(), &cell.plain_date()).month_code
));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_day, day_body, this_plain_date_time, "day", |_global, cell| js_number(calendar_fields(cell.calendar_id(), &cell.plain_date()).day));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_hour, hour_body, this_plain_date_time, "hour", |_global, cell| js_number(cell.plain_time().hour()));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_minute, minute_body, this_plain_date_time, "minute", |_global, cell| js_number(cell.plain_time().minute()));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_second, second_body, this_plain_date_time, "second", |_global, cell| js_number(cell.plain_time().second()));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_millisecond, millisecond_body, this_plain_date_time, "millisecond", |_global, cell| js_number(
    cell.plain_time().millisecond()
));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_microsecond, microsecond_body, this_plain_date_time, "microsecond", |_global, cell| js_number(
    cell.plain_time().microsecond()
));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_nanosecond, nanosecond_body, this_plain_date_time, "nanosecond", |_global, cell| js_number(
    cell.plain_time().nanosecond()
));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_day_of_week, day_of_week_body, this_plain_date_time, "dayOfWeek", |_global, cell| js_number(
    day_of_week(cell.plain_date())
));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_day_of_year, day_of_year_body, this_plain_date_time, "dayOfYear", |_global, cell| js_number(
    calendar_fields(cell.calendar_id(), &cell.plain_date()).day_of_year
));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_week_of_year, week_of_year_body, this_plain_date_time, "weekOfYear", |_global, cell| {
    // `calendarWeekOfYear`: fora do ISO, `undefined`.
    if calendar_is_iso(cell.calendar_id()) { js_number(week_of_year(cell.plain_date())) } else { js_undefined() }
});
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_year_of_week, year_of_week_body, this_plain_date_time, "yearOfWeek", |_global, cell| {
    if calendar_is_iso(cell.calendar_id()) { js_number(year_of_week(cell.plain_date())) } else { js_undefined() }
});
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_days_in_week, days_in_week_body, this_plain_date_time, "daysInWeek", |_global, _cell| js_number(
    DAYS_PER_WEEK
));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_days_in_month, days_in_month_body, this_plain_date_time, "daysInMonth", |_global, cell| js_number(
    calendar_fields(cell.calendar_id(), &cell.plain_date()).days_in_month
));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_days_in_year, days_in_year_body, this_plain_date_time, "daysInYear", |_global, cell| js_number(
    calendar_fields(cell.calendar_id(), &cell.plain_date()).days_in_year
));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_months_in_year, months_in_year_body, this_plain_date_time, "monthsInYear", |_global, cell| js_number(calendar_fields(cell.calendar_id(), &cell.plain_date()).months_in_year));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_in_leap_year, in_leap_year_body, this_plain_date_time, "inLeapYear", |_global, cell| js_boolean(
    calendar_fields(cell.calendar_id(), &cell.plain_date()).in_leap_year
));
// `CalendarISOToDate(calendar, isoDate).[[Era]]` e `[[EraYear]]`: `undefined` no calendário ISO.
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_era, era_body, this_plain_date_time, "era", |global, cell| calendar_fields(cell.calendar_id(), &cell.plain_date()).era_value(global.vm()));
crate::temporal_getter!(temporal_plain_date_time_prototype_getter_era_year, era_year_body, this_plain_date_time, "eraYear", |_global, cell| calendar_fields(cell.calendar_id(), &cell.plain_date()).era_year_value());

/// `plain date timePrototypeTableValues`, na ordem do `@begin`: os métodos (`DontEnum|Function`) e os acessores
/// (`DontEnum|ReadOnly|CustomAccessor`, sem setter).
static PROTOTYPE_TABLE_VALUES: [HashTableValue; 38] = [
    native_entry("add", temporal_plain_date_time_prototype_func_add, 1),
    native_entry("subtract", temporal_plain_date_time_prototype_func_subtract, 1),
    native_entry("until", temporal_plain_date_time_prototype_func_until, 1),
    native_entry("since", temporal_plain_date_time_prototype_func_since, 1),
    native_entry("with", temporal_plain_date_time_prototype_func_with, 1),
    native_entry("withCalendar", temporal_plain_date_time_prototype_func_with_calendar, 1),
    native_entry("withPlainTime", temporal_plain_date_time_prototype_func_with_plain_time, 0),
    native_entry("round", temporal_plain_date_time_prototype_func_round, 1),
    native_entry("equals", temporal_plain_date_time_prototype_func_equals, 1),
    native_entry("toPlainDate", temporal_plain_date_time_prototype_func_to_plain_date, 0),
    native_entry("toPlainTime", temporal_plain_date_time_prototype_func_to_plain_time, 0),
    native_entry("toZonedDateTime", temporal_plain_date_time_prototype_func_to_zoned_date_time, 1),
    native_entry("toString", temporal_plain_date_time_prototype_func_to_string, 0),
    native_entry("toJSON", temporal_plain_date_time_prototype_func_to_json, 0),
    native_entry("toLocaleString", temporal_plain_date_time_prototype_func_to_locale_string, 0),
    native_entry("valueOf", temporal_plain_date_time_prototype_func_value_of, 0),
    custom_getter_entry("calendarId", temporal_plain_date_time_prototype_getter_calendar_id),
    custom_getter_entry("year", temporal_plain_date_time_prototype_getter_year),
    custom_getter_entry("month", temporal_plain_date_time_prototype_getter_month),
    custom_getter_entry("monthCode", temporal_plain_date_time_prototype_getter_month_code),
    custom_getter_entry("day", temporal_plain_date_time_prototype_getter_day),
    custom_getter_entry("hour", temporal_plain_date_time_prototype_getter_hour),
    custom_getter_entry("minute", temporal_plain_date_time_prototype_getter_minute),
    custom_getter_entry("second", temporal_plain_date_time_prototype_getter_second),
    custom_getter_entry("millisecond", temporal_plain_date_time_prototype_getter_millisecond),
    custom_getter_entry("microsecond", temporal_plain_date_time_prototype_getter_microsecond),
    custom_getter_entry("nanosecond", temporal_plain_date_time_prototype_getter_nanosecond),
    custom_getter_entry("dayOfWeek", temporal_plain_date_time_prototype_getter_day_of_week),
    custom_getter_entry("dayOfYear", temporal_plain_date_time_prototype_getter_day_of_year),
    custom_getter_entry("weekOfYear", temporal_plain_date_time_prototype_getter_week_of_year),
    custom_getter_entry("yearOfWeek", temporal_plain_date_time_prototype_getter_year_of_week),
    custom_getter_entry("daysInWeek", temporal_plain_date_time_prototype_getter_days_in_week),
    custom_getter_entry("daysInMonth", temporal_plain_date_time_prototype_getter_days_in_month),
    custom_getter_entry("daysInYear", temporal_plain_date_time_prototype_getter_days_in_year),
    custom_getter_entry("monthsInYear", temporal_plain_date_time_prototype_getter_months_in_year),
    custom_getter_entry("inLeapYear", temporal_plain_date_time_prototype_getter_in_leap_year),
    custom_getter_entry("era", temporal_plain_date_time_prototype_getter_era),
    custom_getter_entry("eraYear", temporal_plain_date_time_prototype_getter_era_year),
];

/// A tabela estática do protótipo.
static PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &PROTOTYPE_TABLE_VALUES };

/// `class TemporalPlainDateTimePrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct TemporalPlainDateTimePrototype;

impl TemporalPlainDateTimePrototype {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`; sem `HasStaticPropertyTable` porque os
    /// membros são postos direto em `finishCreation` (a tabela estática do C++ é essa mesma lista).
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | crate::runtime::js_type_info::HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, TemporalPlainDateTimePrototype::STRUCTURE_FLAGS),
            &TEMPORAL_PLAIN_DATE_TIME_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `TemporalPlainDateTimePrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        TemporalPlainDateTimePrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm)` com `plainDateTimePrototypeTable`: os métodos (`DontEnum|Function`, com o comprimento da
    /// tabela), os acessores (`DontEnum|ReadOnly|CustomAccessor`) e `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
    fn finish_creation(prototype: &JSObject, vm: &VM, _global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);

        put_to_string_tag(vm, prototype, TEMPORAL_PLAIN_DATE_TIME_PROTOTYPE_S_INFO.class_name);
    }
}
