//! Porte de `runtime/TemporalZonedDateTimePrototype.{h,cpp}`: `Temporal.ZonedDateTime.prototype` (um
//! `JSNonFinalObject` com o `ClassInfo` `"Temporal.ZonedDateTime"`): `with`, `withPlainTime`, `withTimeZone`,
//! `withCalendar`, `add`, `subtract`, `until`, `since`, `round`, `startOfDay`, `getTimeZoneTransition`, `equals`,
//! `toInstant`, `toPlainDateTime`, `toPlainDate`, `toPlainTime`, `toString`, `toJSON`, `toLocaleString`,
//! `valueOf`, os 28 acessores (`DontEnum|ReadOnly|CustomAccessor`) e `@@toStringTag`.
//!
//! DIVERGÊNCIAS:
//! - Os acessores de data despacham por `calendar_id` da célula (`calendar_fields`): `era`, `eraYear`, `monthCode`,
//!   `weekOfYear` e `yearOfWeek` seguem o calendário (os dois últimos `undefined` fora do ISO, o ramo de
//!   `calendarIsISO` do C++).
//! - `toLocaleString` delega ao `IntlDateTimeFormat` com `ZonedDateTime` (`intl_date_time_format/temporal.rs`):
//!   o fuso do próprio objeto, `timeZoneName: "short"` por padrão, e `options.timeZone` é `TypeError`.
//! - `round` e `getTimeZoneTransition` decodificam o texto direto, sem o objeto intermediário `{ smallestUnit }` /
//!   `{ direction }` que o C++ monta (a leitura da propriedade é a mesma, sem efeito observável).
//! - `until` e `since` são o template `differenceTemporalZonedDateTime<op>` do C++: um corpo só, com a operação
//!   como argumento.
//! - `valueOf` e os `TypeError` de marca seguem o texto do C++.

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{put_to_string_tag};
use crate::runtime::lookup::{HashTable, HashTableValue};
use crate::runtime::lookup::{custom_getter_entry, native_entry};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::intl_date_time_format::temporal::zoned_date_time_to_locale_string;
use crate::runtime::intl_support::{get_options_object, get_property, option_enum, str_value, to_rust_string};
use crate::runtime::iso8601::{
    day_of_week, day_of_year, format_time_zone_offset_string, week_of_year, year_of_week, ExactTime, PlainDateTime, DAYS_PER_WEEK,
};
use crate::runtime::js_big_int::{ImplResult, JSBigInt};
use crate::runtime::js_big_int_ops::impl_result_value;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, js_null, js_number, JSValue};
use crate::runtime::temporal_calendar_icu::calendar_fields;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::temporal_calendar::{temporal_show_calendar_name, to_temporal_calendar_identifier};
use crate::runtime::temporal_core_calendar_fields::{calendar_merge_fields, iso_date_to_fields, ResolveType, TimeFieldsIn};
use crate::runtime::temporal_core_duration::temporal_duration_from_internal;
use crate::runtime::temporal_core_iso_date::{add_days_to_iso_date, round_iso_date_time};
use crate::runtime::temporal_core_rounding::{maximum_rounding_increment, round_number_to_increment_i128, validate_temporal_rounding_increment};
use crate::runtime::temporal_core_types::TransitionDirection;
use crate::runtime::temporal_core_zoned_date_time::{
    difference_zoned_date_time_with_rounding, get_start_of_day, interpret_iso_date_time_offset, MatchBehaviour, UseStartOfDay,
};
use crate::runtime::temporal_duration::{create_temporal_duration, TemporalDuration};
use crate::runtime::temporal_instant::TemporalInstant;
use crate::runtime::temporal_object::{
    extract_difference_options, is_partial_temporal_object, length_in_nanoseconds, temporal_fractional_second_digits, temporal_rounding_increment,
    temporal_rounding_mode, temporal_unit_type, temporal_unit_valued, to_seconds_string_precision_record, to_temporal_disambiguation,
    to_temporal_offset, to_temporal_overflow, to_temporal_overflow_value, validate_temporal_unit_value, AllowedUnit, DifferenceOperation,
    Inclusivity, OffsetBehaviour, RoundingMode, TemporalDisambiguation, TemporalOffsetDisambiguation, TemporalUnit, TemporalUnitDefault, UnitGroup,
    UnitOption,
};
use crate::runtime::temporal_plain_date::TemporalPlainDate;
use crate::runtime::temporal_plain_date_time::TemporalPlainDateTime;
use crate::runtime::temporal_plain_time::TemporalPlainTime;
use crate::runtime::temporal_time_zone::{get_time_zone_transition, time_zone_equals};
use crate::runtime::temporal_time_zone::add_zoned_date_time;
use crate::runtime::temporal_zoned_date_time::{
    create_temporal_zoned_date_time, read_zoned_date_time_fields_from_object, to_temporal_time_zone_identifier, zoned_date_time_to_string,
    ShowOffsetOption, ShowTimeZoneNameOption, TemporalZonedDateTime, TemporalZonedDateTimeRef, ZonedDateTimeFieldMode,
};
use crate::runtime::vm::VM;

/// `const ClassInfo TemporalZonedDateTimePrototype::s_info` (`"Temporal.ZonedDateTime"`).
pub static TEMPORAL_ZONED_DATE_TIME_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "Temporal.ZonedDateTime",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&PROTOTYPE_TABLE), inherits_js_type_range: None,
};

/// O `dynamicDowncast<TemporalZonedDateTime>(thisValue)` com o `TypeError` de marca de cada membro.
fn this_zoned_date_time(this_value: JSValue, member: &str) -> Result<TemporalZonedDateTimeRef, Thrown> {
    TemporalZonedDateTime::from_value(&this_value)
        .ok_or_else(|| Thrown::TypeError(format!("Temporal.ZonedDateTime.prototype.{member} called on value that's not a ZonedDateTime")))
}

/// `createTemporalZonedDateTime(globalObject, exactTime, zdt->timeZone(), zdt->calendarID())`: um `ZonedDateTime` com
/// o instante dado e o fuso e o calendário do receptor.
fn with_exact_time(global_object: &JSGlobalObject, zoned_date_time: &TemporalZonedDateTime, exact_time: ExactTime) -> HostResult {
    Ok(create_temporal_zoned_date_time(global_object, exact_time, zoned_date_time.time_zone().clone(), zoned_date_time.calendar_id(), None)?
        .as_value())
}

/// `temporalZonedDateTimePrototypeFuncStartOfDay`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.startofday
fn start_of_day_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let zoned_date_time = this_zoned_date_time(call.this_value(), "startOfDay")?;
    // Passos 3 a 5: `isoDateTime = GetISODateTimeFor(timeZone, epochNanoseconds)`.
    let PlainDateTime { date, .. } = zoned_date_time.get_local_date_time()?;
    // Passo 6: `epochNanoseconds = ? GetStartOfDay(timeZone, isoDateTime.[[ISODate]])`.
    let start = get_start_of_day(zoned_date_time.time_zone(), date)?;
    // Passo 7: `CreateTemporalZonedDateTime(epochNanoseconds, timeZone, calendar)`.
    with_exact_time(global_object, &zoned_date_time, start)
}

/// `temporalZonedDateTimePrototypeFuncWithPlainTime`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.withplaintime
fn with_plain_time_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let zoned_date_time = this_zoned_date_time(call.this_value(), "withPlainTime")?;
    // Passos 3 a 5: `isoDateTime = GetISODateTimeFor(timeZone, zonedDateTime.[[EpochNanoseconds]])`.
    let PlainDateTime { date, .. } = zoned_date_time.get_local_date_time()?;

    let time_argument = call.argument(0);
    let result = if time_argument.is_undefined() {
        // Passo 6.a: `epochNs = ? GetStartOfDay(timeZone, isoDateTime.[[ISODate]])`.
        get_start_of_day(zoned_date_time.time_zone(), date)?
    } else {
        // Passo 7.a: `plainTime = ? ToTemporalTime(plainTimeLike)`.
        let plain_time = TemporalPlainTime::from(global_object, time_argument, JSValue::Undefined)?;
        // Passos 7.b e 7.c: `GetEpochNanosecondsFor(timeZone, resultISODateTime, ~compatible~)`.
        crate::runtime::temporal_time_zone::get_epoch_nanoseconds_for(
            zoned_date_time.time_zone(),
            date,
            plain_time.plain_time(),
            TemporalDisambiguation::Compatible,
        )?
    };

    // Passo 8: `CreateTemporalZonedDateTime(epochNs, timeZone, calendar)`.
    with_exact_time(global_object, &zoned_date_time, result)
}

/// `addDurationToZonedDateTime(globalObject, scope, zdt, duration, optionsArg)` (`AddDurationToZonedDateTime`):
/// https://tc39.es/proposal-temporal/#sec-temporal-adddurationtozoneddatetime
/// Quem chama faz os passos 1 e 2 (`ToTemporalDuration` e a negação de `subtract`).
fn add_duration_to_zoned_date_time(
    global_object: &JSGlobalObject,
    zoned_date_time: &TemporalZonedDateTime,
    duration: crate::runtime::iso8601::Duration,
    options_arg: JSValue,
) -> HostResult {
    // Passos 3 e 4: `GetOptionsObject` e `GetTemporalOverflowOption`.
    let overflow = to_temporal_overflow_value(global_object, options_arg)?;
    // Passos 5 a 8: `AddZonedDateTime(epochNs, timeZone, calendar, duration, overflow)`.
    let result = add_zoned_date_time(zoned_date_time.exact_time(), zoned_date_time.time_zone(), zoned_date_time.calendar_id(), &duration, overflow)?;
    // Passo 9.
    with_exact_time(global_object, zoned_date_time, result)
}

/// `temporalZonedDateTimePrototypeFuncAdd`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.add
fn add_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let zoned_date_time = this_zoned_date_time(call.this_value(), "add")?;
    // Passo 3: `AddDurationToZonedDateTime(~add~, zonedDateTime, temporalDurationLike, options)`.
    let duration = TemporalDuration::to_temporal_duration_record(global_object, call.argument(0))?;
    add_duration_to_zoned_date_time(global_object, &zoned_date_time, duration, call.argument(1))
}

/// `temporalZonedDateTimePrototypeFuncSubtract`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.subtract
fn subtract_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let zoned_date_time = this_zoned_date_time(call.this_value(), "subtract")?;
    // Passo 3: `AddDurationToZonedDateTime(~subtract~, ...)`; o passo 2 interno nega a duração.
    let duration = TemporalDuration::to_temporal_duration_record(global_object, call.argument(0))?;
    add_duration_to_zoned_date_time(global_object, &zoned_date_time, -duration, call.argument(1))
}

/// `differenceTemporalZonedDateTime<op>(globalObject, scope, zdt, otherArg, optionsArg)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-differencetemporalzoneddatetime
fn difference_temporal_zoned_date_time(
    global_object: &JSGlobalObject,
    zoned_date_time: &TemporalZonedDateTime,
    other_arg: JSValue,
    options_arg: JSValue,
    operation: DifferenceOperation,
) -> HostResult {
    // Passo 1: `other = ? ToTemporalZonedDateTime(other)`.
    let other = TemporalZonedDateTime::from(global_object, other_arg, JSValue::Undefined)?;

    // Passo 2: `CalendarEquals` falso é `RangeError`.
    if zoned_date_time.calendar_id() != other.calendar_id() {
        return Err(Thrown::range_error("cannot compute difference between ZonedDateTimes with different calendars"));
    }

    // Passos 3 e 4: `GetOptionsObject(options)` e `GetDifferenceSettings(op, resolvedOptions, ~datetime~, « »,
    // ~nanosecond~, ~hour~)`.
    let (smallest_unit, largest_unit, rounding_mode, increment) =
        extract_difference_options(global_object, options_arg, UnitGroup::DateTime, TemporalUnit::Nanosecond, TemporalUnit::Hour, operation)?;

    // Passo 5 (o atalho de `DifferenceInstant` para unidade de tempo) fica com a diferença do núcleo.
    // Passo 7: `TimeZoneEquals` falso com unidade de dia ou maior é `RangeError`.
    if largest_unit <= TemporalUnit::Day && !time_zone_equals(zoned_date_time.time_zone(), other.time_zone()) {
        return Err(Thrown::range_error("cannot compute day-or-larger difference between ZonedDateTimes with different time zones"));
    }

    // Passos 8 e 9: `DifferenceZonedDateTimeWithRounding`.
    let internal_duration = difference_zoned_date_time_with_rounding(
        zoned_date_time.calendar_id(),
        zoned_date_time.exact_time(),
        other.exact_time(),
        zoned_date_time.time_zone(),
        largest_unit,
        smallest_unit,
        rounding_mode,
        increment,
    )?;

    // Passo 10: `TemporalDurationFromInternal(internalDuration, ~hour~)`; o atalho do passo 5 foi dispensado, então a
    // unidade de tempo maior entra aqui (o resultado é o da spec).
    let duration_largest_unit = if largest_unit <= TemporalUnit::Day { TemporalUnit::Hour } else { largest_unit };
    let mut result = temporal_duration_from_internal(&internal_duration, duration_largest_unit)?;

    // Passo 11: `since` nega o resultado.
    if operation == DifferenceOperation::Since {
        result = -result;
    }

    // Passo 12.
    Ok(create_temporal_duration(global_object, result, None)?.as_value())
}

/// `temporalZonedDateTimePrototypeFuncUntil`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.until
fn until_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let zoned_date_time = this_zoned_date_time(call.this_value(), "until")?;
    difference_temporal_zoned_date_time(global_object, &zoned_date_time, call.argument(0), call.argument(1), DifferenceOperation::Until)
}

/// `temporalZonedDateTimePrototypeFuncSince`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.since
fn since_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let zoned_date_time = this_zoned_date_time(call.this_value(), "since")?;
    difference_temporal_zoned_date_time(global_object, &zoned_date_time, call.argument(0), call.argument(1), DifferenceOperation::Since)
}

/// `temporalZonedDateTimePrototypeFuncRound`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.round
fn round_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let zoned_date_time = this_zoned_date_time(call.this_value(), "round")?;

    // Passos 3 a 5: `roundTo` `undefined` (e qualquer não objeto que não seja `String`) é `TypeError`; `String` é
    // `{ smallestUnit: <string> }`; senão `GetOptionsObject`.
    let round_to = call.argument(0);
    let mut options: Option<JSValue> = None;
    let mut smallest: Option<UnitOption> = None;
    if round_to.is_string() {
        let text = to_rust_string(global_object, round_to)?;
        smallest = Some(if text == "auto" {
            UnitOption::Auto
        } else {
            UnitOption::Unit(temporal_unit_type(&text).ok_or_else(|| Thrown::range_error("invalid Temporal unit"))?)
        });
    } else if round_to.is_object() {
        options = Some(round_to);
    } else {
        return Err(Thrown::type_error("Temporal.ZonedDateTime.prototype.round requires a smallestUnit option string or options object"));
    }

    // Passos 6 e 7: `GetRoundingIncrementOption(roundTo)`, as opções em ordem alfabética.
    let rounding_increment = temporal_rounding_increment(global_object, options)?;
    // Passo 8: `GetRoundingModeOption(roundTo, ~half-expand~)`.
    let rounding_mode = temporal_rounding_mode(global_object, options, RoundingMode::HalfExpand)?;
    // Passo 9: `GetTemporalUnitValuedOption(roundTo, "smallestUnit", ~required~)`.
    let smallest_maybe_auto = match smallest {
        Some(unit) => unit,
        None => temporal_unit_valued(global_object, options, "smallestUnit", TemporalUnitDefault::Required)?,
    };
    // Passo 10: `ValidateTemporalUnitValue(smallestUnit, ~time~, « ~day~ »)`.
    validate_temporal_unit_value(smallest_maybe_auto, UnitGroup::Time, AllowedUnit::Day, "smallestUnit")?;
    let UnitOption::Unit(smallest_unit) = smallest_maybe_auto else {
        unreachable!("smallestUnit é obrigatório e `auto` não passa pela validação");
    };

    // Passos 11 a 13: `day` é máximo 1 inclusivo; as outras, `MaximumTemporalDurationRoundingIncrement` exclusivo.
    if smallest_unit == TemporalUnit::Day {
        validate_temporal_rounding_increment(rounding_increment, Some(1.0), Inclusivity::Inclusive)?;
    } else {
        validate_temporal_rounding_increment(
            rounding_increment,
            maximum_rounding_increment(smallest_unit).map(f64::from),
            Inclusivity::Exclusive,
        )?;
    }

    // Passo 14: `nanosecond` com incremento 1 não arredonda, mas devolve uma instância nova.
    if smallest_unit == TemporalUnit::Nanosecond && rounding_increment == 1.0 {
        return with_exact_time(global_object, &zoned_date_time, zoned_date_time.exact_time());
    }

    // Passos 15 a 18: `isoDateTime = GetISODateTimeFor(timeZone, thisNs)`.
    let PlainDateTime { date, time } = zoned_date_time.get_local_date_time()?;
    let epoch_ns = zoned_date_time.exact_time().epoch_nanoseconds();
    let time_zone = zoned_date_time.time_zone();

    let result_ns = if smallest_unit == TemporalUnit::Day {
        // Passos 19.a a 19.e: `dateStart`, `dateEnd = AddDaysToISODate(dateStart, 1)` e os `GetStartOfDay` das duas.
        let next_date = add_days_to_iso_date(date, 1);
        let start_ns = get_start_of_day(time_zone, date)?.epoch_nanoseconds();
        let next_ns = get_start_of_day(time_zone, next_date)?.epoch_nanoseconds();
        // Passo 19.g: `dayLengthNs = endNs - startNs`.
        let day_length = next_ns - start_ns;
        if day_length == 0 || epoch_ns < start_ns {
            return Err(Thrown::range_error("Rounding result is outside the supported range of Temporal.ZonedDateTime"));
        }
        // Correção do polyfill (#3312): uma transição para trás que cruza a meia-noite deixa `epochNs >= nextNs`
        // numa data válida; limita a `nextNs - 1` para o arredondamento ficar dentro do dia.
        let epoch_ns = if epoch_ns >= next_ns { next_ns - 1 } else { epoch_ns };
        // Passos 19.h a 19.j: `dayProgressNs`, `roundedDayNs` e `epochNanoseconds = startNs + roundedDayNs`.
        let rounded_offset = round_number_to_increment_i128(epoch_ns - start_ns, day_length, rounding_mode);
        ExactTime::new(start_ns + rounded_offset)
    } else {
        // Passo 20.a: `roundResult = RoundISODateTime(isoDateTime, roundingIncrement, smallestUnit, roundingMode)`.
        let increment_ns = length_in_nanoseconds(smallest_unit) * (rounding_increment.trunc() as i128);
        let current_offset = zoned_date_time.get_offset_nanoseconds()?;
        let (rounded_date, rounded_time) = round_iso_date_time(date, time, increment_ns, smallest_unit, rounding_mode);
        // Passos 20.b e 20.c: `InterpretISODateTimeOffset(..., ~option~, offsetNanoseconds, ..., ~compatible~, ~prefer~, ~match-exactly~)`.
        interpret_iso_date_time_offset(
            rounded_date,
            rounded_time,
            UseStartOfDay::No,
            OffsetBehaviour::Option,
            TemporalOffsetDisambiguation::Prefer,
            current_offset,
            MatchBehaviour::MatchExactly,
            time_zone,
            TemporalDisambiguation::Compatible,
        )?
    };

    // Passo 21.
    with_exact_time(global_object, &zoned_date_time, result_ns)
}

/// `temporalZonedDateTimePrototypeFuncGetTimeZoneTransition`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.gettimezonetransition
fn get_time_zone_transition_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let zoned_date_time = this_zoned_date_time(call.this_value(), "getTimeZoneTransition")?;

    // Passos 4 a 6: `directionParam` `undefined` é `TypeError`; `String` é `{ direction: <string> }`; senão
    // `GetOptionsObject` e a propriedade `direction`.
    let direction_argument = call.argument(0);
    let direction_string = if direction_argument.is_string() {
        to_rust_string(global_object, direction_argument)?
    } else if direction_argument.is_object() {
        let direction = get_property(global_object, direction_argument, "direction")?;
        if direction.is_undefined() {
            return Err(Thrown::range_error("Temporal.ZonedDateTime.prototype.getTimeZoneTransition requires a 'direction' option"));
        }
        to_rust_string(global_object, direction)?
    } else if direction_argument.is_undefined() {
        return Err(Thrown::type_error("Temporal.ZonedDateTime.prototype.getTimeZoneTransition requires a 'direction' option"));
    } else {
        return Err(Thrown::type_error("Temporal.ZonedDateTime.prototype.getTimeZoneTransition requires an options object or string"));
    };

    // Passo 7: `direction = ? GetDirectionOption(directionParam)`.
    let direction = match direction_string.as_str() {
        "next" => TransitionDirection::Next,
        "previous" => TransitionDirection::Previous,
        _ => return Err(Thrown::range_error("direction must be \"next\" or \"previous\"")),
    };

    // Passos 8 a 10: `null` para deslocamento fixo; `GetNamedTimeZoneNextTransition` ou `...PreviousTransition`.
    let transition = get_time_zone_transition(zoned_date_time.time_zone(), zoned_date_time.exact_time(), direction)?;

    // Passos 11 e 12: sem transição é `null`; senão `CreateTemporalZonedDateTime(transition, timeZone, calendar)`.
    match transition {
        None => Ok(js_null()),
        Some(transition) => with_exact_time(global_object, &zoned_date_time, transition),
    }
}

/// `temporalZonedDateTimePrototypeFuncWith`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.with
fn with_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let zoned_date_time = this_zoned_date_time(call.this_value(), "with")?;

    // Passo 3: `IsPartialTemporalObject(temporalZonedDateTimeLike)` falso é `TypeError`.
    let fields_argument = call.argument(0);
    if !is_partial_temporal_object(global_object, fields_argument)? {
        return Err(Thrown::type_error("Temporal.ZonedDateTime.prototype.with requires a partial Temporal object of field overrides"));
    }

    // Passos 4 a 8: `calendar`, `offsetNanoseconds` e `isoDateTime` do receptor.
    let calendar_id = zoned_date_time.calendar_id();
    let PlainDateTime { date: current_date, time: current_time } = zoned_date_time.get_local_date_time()?;
    let current_offset_ns = zoned_date_time.get_offset_nanoseconds()?;

    // Passo 9: `fields = ISODateToFields(calendar, isoDateTime.[[ISODate]], ~date~)`.
    let date_fields = iso_date_to_fields(calendar_id, current_date, ResolveType::Date)?;

    // Passo 17: `partialZonedDateTime = ? PrepareCalendarFields(..., ~partial~)`.
    let partial = read_zoned_date_time_fields_from_object(global_object, fields_argument, calendar_id, ZonedDateTimeFieldMode::Partial)?;

    // Passo 18: `fields = CalendarMergeFields(calendar, fields, partialZonedDateTime)`.
    let merged = calendar_merge_fields(calendar_id, &date_fields, &partial.date_fields);

    // Passo 19: `resolvedOptions = ? GetOptionsObject(options)`.
    let options = get_options_object(call.argument(1))?;
    // Passos 20 a 22: `disambiguation`, `offset` (`~prefer~`) e `overflow`, em ordem alfabética.
    let disambiguation = to_temporal_disambiguation(global_object, options)?;
    let offset_opt = to_temporal_offset(global_object, options, TemporalOffsetDisambiguation::Prefer)?;
    let overflow = to_temporal_overflow(global_object, options)?;

    // Passos 10 a 16: os campos de hora do receptor, sobrescritos pelos dados.
    let time_fields = TimeFieldsIn {
        hour: Some(partial.time_fields.hour.unwrap_or(f64::from(current_time.hour()))),
        minute: Some(partial.time_fields.minute.unwrap_or(f64::from(current_time.minute()))),
        second: Some(partial.time_fields.second.unwrap_or(f64::from(current_time.second()))),
        millisecond: Some(partial.time_fields.millisecond.unwrap_or(f64::from(current_time.millisecond()))),
        microsecond: Some(partial.time_fields.microsecond.unwrap_or(f64::from(current_time.microsecond()))),
        nanosecond: Some(partial.time_fields.nanosecond.unwrap_or(f64::from(current_time.nanosecond()))),
    };

    // Passo 23: `dateTimeResult = ? InterpretTemporalDateTimeFields(calendar, fields, overflow)`.
    let PlainDateTime { date: new_date, time: new_time } =
        crate::runtime::temporal_calendar::interpret_temporal_date_time_fields(calendar_id, &merged, &time_fields, overflow)?;

    // Passo 24: o `offset` do dado, ou o do receptor (formatar e reler um deslocamento intacto é a identidade).
    let given_offset_ns = partial.offset_ns.unwrap_or(current_offset_ns);

    // Passo 25: `InterpretISODateTimeOffset(..., ~option~, newOffsetNanoseconds, timeZone, disambiguation, offset,
    // ~match-exactly~)`.
    let epoch_ns = interpret_iso_date_time_offset(
        new_date,
        new_time,
        UseStartOfDay::No,
        OffsetBehaviour::Option,
        offset_opt,
        given_offset_ns,
        MatchBehaviour::MatchExactly,
        zoned_date_time.time_zone(),
        disambiguation,
    )?;

    // Passo 26.
    with_exact_time(global_object, &zoned_date_time, epoch_ns)
}

/// `temporalZonedDateTimePrototypeFuncWithCalendar`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.withcalendar
fn with_calendar_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let zoned_date_time = this_zoned_date_time(call.this_value(), "withCalendar")?;
    // Passo 3: `calendar = ? ToTemporalCalendarIdentifier(calendarLike)`.
    let calendar_id = to_temporal_calendar_identifier(global_object, call.argument(0))?;
    // Passo 4: `CreateTemporalZonedDateTime(epochNanoseconds, timeZone, calendar)`.
    Ok(create_temporal_zoned_date_time(global_object, zoned_date_time.exact_time(), zoned_date_time.time_zone().clone(), calendar_id, None)?
        .as_value())
}

/// `temporalZonedDateTimePrototypeFuncWithTimeZone`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.withtimezone
fn with_time_zone_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let zoned_date_time = this_zoned_date_time(call.this_value(), "withTimeZone")?;
    // Passo 3: `timeZone = ? ToTemporalTimeZoneIdentifier(timeZoneLike)`.
    let time_zone = to_temporal_time_zone_identifier(global_object, call.argument(0))?;
    // Passo 4: `CreateTemporalZonedDateTime(epochNanoseconds, timeZone, calendar)`.
    Ok(create_temporal_zoned_date_time(global_object, zoned_date_time.exact_time(), time_zone, zoned_date_time.calendar_id(), None)?.as_value())
}

/// `temporalZonedDateTimePrototypeFuncEquals`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.equals
fn equals_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let zoned_date_time = this_zoned_date_time(call.this_value(), "equals")?;
    // Passo 3: `other = ? ToTemporalZonedDateTime(other)`.
    let other = TemporalZonedDateTime::from(global_object, call.argument(0), JSValue::Undefined)?;
    // Passos 4 a 6: instante, `TimeZoneEquals` e `CalendarEquals`.
    Ok(js_boolean(
        zoned_date_time.exact_time() == other.exact_time()
            && time_zone_equals(zoned_date_time.time_zone(), other.time_zone())
            && zoned_date_time.calendar_id() == other.calendar_id(),
    ))
}

/// `temporalZonedDateTimePrototypeFuncToInstant`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.toinstant
fn to_instant_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let zoned_date_time = this_zoned_date_time(call.this_value(), "toInstant")?;
    // Passo 3: `CreateTemporalInstant(zonedDateTime.[[EpochNanoseconds]])`.
    Ok(TemporalInstant::create(global_object.vm(), global_object.instant_structure(), zoned_date_time.exact_time()).as_value())
}

/// `temporalZonedDateTimePrototypeFuncToPlainDate`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.toplaindate
fn to_plain_date_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let zoned_date_time = this_zoned_date_time(call.this_value(), "toPlainDate")?;
    // Passo 3: `GetISODateTimeFor`. Passo 4: `CreateTemporalDate(isoDateTime.[[ISODate]], calendar)`.
    let PlainDateTime { date, .. } = zoned_date_time.get_local_date_time()?;
    Ok(TemporalPlainDate::create(global_object.vm(), &global_object.plain_date_structure(), date, zoned_date_time.calendar_id()).as_value())
}

/// `temporalZonedDateTimePrototypeFuncToPlainTime`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.toplaintime
fn to_plain_time_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let zoned_date_time = this_zoned_date_time(call.this_value(), "toPlainTime")?;
    // Passo 3: `GetISODateTimeFor`. Passo 4: `CreateTemporalTime(isoDateTime.[[Time]])`.
    let PlainDateTime { time, .. } = zoned_date_time.get_local_date_time()?;
    Ok(TemporalPlainTime::create(global_object.vm(), &global_object.plain_time_structure(), time).as_value())
}

/// `temporalZonedDateTimePrototypeFuncToPlainDateTime`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.toplaindatetime
fn to_plain_date_time_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let zoned_date_time = this_zoned_date_time(call.this_value(), "toPlainDateTime")?;
    // Passo 3: `GetISODateTimeFor`. Passo 4: `CreateTemporalDateTime(isoDateTime, calendar)`.
    let PlainDateTime { date, time } = zoned_date_time.get_local_date_time()?;
    Ok(TemporalPlainDateTime::create(global_object.vm(), &global_object.plain_date_time_structure(), date, time, zoned_date_time.calendar_id())
        .as_value())
}

/// `temporalZonedDateTimePrototypeFuncToJSON`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.tojson
fn to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca. Passo 3: `TemporalZonedDateTimeToString(zonedDateTime, ~auto~, ~auto~, ~auto~, ~auto~)`.
    let zoned_date_time = this_zoned_date_time(call.this_value(), "toJSON")?;
    Ok(str_value(global_object.vm(), &zoned_date_time.to_string_default()?))
}

/// `temporalZonedDateTimePrototypeFuncToString`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.tostring
fn to_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let zoned_date_time = this_zoned_date_time(call.this_value(), "toString")?;

    // Passo 3: `resolvedOptions = ? GetOptionsObject(options)`.
    let options = get_options_object(call.argument(0))?;

    // Passos 4 e 5: as opções em ordem alfabética; `showCalendar = ? GetTemporalShowCalendarNameOption(resolvedOptions)`.
    let show_calendar = temporal_show_calendar_name(global_object, options)?;
    // Passo 6: `digits = ? GetTemporalFractionalSecondDigitsOption(resolvedOptions)`.
    let digits = temporal_fractional_second_digits(global_object, options)?;
    // Passo 7: `showOffset = ? GetTemporalShowOffsetOption(resolvedOptions)`.
    let show_offset =
        option_enum::<ShowOffsetOption>(global_object, options, "offset", "offset must be \"auto\" or \"never\"")?.unwrap_or(ShowOffsetOption::Auto);
    // Passo 8: `roundingMode = ? GetRoundingModeOption(resolvedOptions, ~trunc~)`.
    let rounding_mode = temporal_rounding_mode(global_object, options, RoundingMode::Trunc)?;
    // Passo 9: `smallestUnit = ? GetTemporalUnitValuedOption(resolvedOptions, "smallestUnit", ~unset~)`.
    let smallest_unit_option = temporal_unit_valued(global_object, options, "smallestUnit", TemporalUnitDefault::Unset)?;
    // Passo 10: `showTimeZone = ? GetTemporalShowTimeZoneNameOption(resolvedOptions)`.
    let show_time_zone = option_enum::<ShowTimeZoneNameOption>(
        global_object,
        options,
        "timeZoneName",
        "timeZoneName must be \"auto\", \"never\", or \"critical\"",
    )?
    .unwrap_or(ShowTimeZoneNameOption::Auto);

    // Passo 11: `ValidateTemporalUnitValue(smallestUnit, ~time~)`.
    validate_temporal_unit_value(smallest_unit_option, UnitGroup::Time, AllowedUnit::None, "smallestUnit")?;
    let smallest_unit = match smallest_unit_option {
        UnitOption::Unit(unit) => Some(unit),
        UnitOption::Unset | UnitOption::Auto => None,
    };
    // Passo 12: `hour` é `RangeError`.
    if smallest_unit == Some(TemporalUnit::Hour) {
        return Err(Thrown::range_error("smallestUnit cannot be \"hour\" for ZonedDateTime.toString"));
    }

    // Passo 13: `precision = ToSecondsStringPrecisionRecord(smallestUnit, digits)`.
    let precision = to_seconds_string_precision_record(smallest_unit, digits);

    // Passo 14: `TemporalZonedDateTimeToString(...)`.
    let text = zoned_date_time_to_string(
        zoned_date_time.exact_time(),
        zoned_date_time.time_zone(),
        zoned_date_time.calendar_id(),
        precision,
        rounding_mode,
        show_offset,
        show_time_zone,
        show_calendar,
    )?;
    Ok(str_value(global_object.vm(), &text))
}

/// `temporalZonedDateTimePrototypeFuncToLocaleString`: https://tc39.es/proposal-temporal/#sup-temporal.zoneddatetime.prototype.tolocalestring
/// Passo 3 (ECMA-402): `CreateDateTimeFormat(%Intl.DateTimeFormat%, locales, options, ~any~, ~all~, timeZone)` e o
/// `FormatDateTime` do instante.
fn to_locale_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let zoned_date_time = this_zoned_date_time(call.this_value(), "toLocaleString")?;
    // Passo 5 e 6: o instante (`epochMilliseconds` do `ExactTime`, truncado) no fuso do objeto.
    let text = zoned_date_time_to_locale_string(
        global_object,
        call.argument(0),
        call.argument(1),
        &zoned_date_time.time_zone_id(),
        zoned_date_time.exact_time().epoch_milliseconds(),
    )?;
    Ok(str_value(global_object.vm(), &text))
}

/// `temporalZonedDateTimePrototypeFuncValueOf`: https://tc39.es/proposal-temporal/#sec-temporal.zoneddatetime.prototype.valueof
fn value_of_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error(
        "Temporal.ZonedDateTime.prototype.valueOf must not be called. To compare ZonedDateTime values, use Temporal.ZonedDateTime.compare",
    ))
}

host_function!(temporal_zoned_date_time_prototype_func_with, with_body);
host_function!(temporal_zoned_date_time_prototype_func_with_plain_time, with_plain_time_body);
host_function!(temporal_zoned_date_time_prototype_func_with_time_zone, with_time_zone_body);
host_function!(temporal_zoned_date_time_prototype_func_with_calendar, with_calendar_body);
host_function!(temporal_zoned_date_time_prototype_func_add, add_body);
host_function!(temporal_zoned_date_time_prototype_func_subtract, subtract_body);
host_function!(temporal_zoned_date_time_prototype_func_until, until_body);
host_function!(temporal_zoned_date_time_prototype_func_since, since_body);
host_function!(temporal_zoned_date_time_prototype_func_round, round_body);
host_function!(temporal_zoned_date_time_prototype_func_start_of_day, start_of_day_body);
host_function!(temporal_zoned_date_time_prototype_func_get_time_zone_transition, get_time_zone_transition_body);
host_function!(temporal_zoned_date_time_prototype_func_equals, equals_body);
host_function!(temporal_zoned_date_time_prototype_func_to_instant, to_instant_body);
host_function!(temporal_zoned_date_time_prototype_func_to_plain_date_time, to_plain_date_time_body);
host_function!(temporal_zoned_date_time_prototype_func_to_plain_date, to_plain_date_body);
host_function!(temporal_zoned_date_time_prototype_func_to_plain_time, to_plain_time_body);
host_function!(temporal_zoned_date_time_prototype_func_to_string, to_string_body);
host_function!(temporal_zoned_date_time_prototype_func_to_json, to_json_body);
host_function!(temporal_zoned_date_time_prototype_func_to_locale_string, to_locale_string_body);
host_function!(temporal_zoned_date_time_prototype_func_value_of, value_of_body);

/// `JSC_DEFINE_CUSTOM_GETTER(temporalZonedDateTimePrototypeGetterX, ...)` (`this_zoned_date_time` é a marca).
macro_rules! zoned_date_time_getter {
    ($host:ident, $body:ident, $name:literal, |$global:ident, $cell:ident| $value:expr) => {
        crate::temporal_getter!($host, $body, this_zoned_date_time, $name, |$global, $cell| $value);
    };
}

// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.epochnanoseconds
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_epoch_nanoseconds, epoch_nanoseconds_body, "epochNanoseconds", |global, zdt| {
    let value = impl_result_value(JSBigInt::create_from_i128(zdt.exact_time().epoch_nanoseconds()).map(ImplResult::Heap));
    pending_or(global, value)?
});
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.timezoneid
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_time_zone_id, time_zone_id_body, "timeZoneId", |global, zdt| str_value(
    global.vm(),
    &zdt.time_zone_id()
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.calendarid
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_calendar_id, calendar_id_body, "calendarId", |global, zdt| str_value(
    global.vm(),
    zdt.calendar_id_string()
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.year
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_year, year_body, "year", |_global, zdt| js_number(
    calendar_fields(zdt.calendar_id(), &zdt.get_local_date_time()?.date).year
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.month
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_month, month_body, "month", |_global, zdt| js_number(
    calendar_fields(zdt.calendar_id(), &zdt.get_local_date_time()?.date).month
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.monthcode
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_month_code, month_code_body, "monthCode", |global, zdt| str_value(
    global.vm(),
    &calendar_fields(zdt.calendar_id(), &zdt.get_local_date_time()?.date).month_code
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.day
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_day, day_body, "day", |_global, zdt| js_number(
    calendar_fields(zdt.calendar_id(), &zdt.get_local_date_time()?.date).day
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.hour
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_hour, hour_body, "hour", |_global, zdt| js_number(
    zdt.get_local_date_time()?.time.hour()
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.minute
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_minute, minute_body, "minute", |_global, zdt| js_number(
    zdt.get_local_date_time()?.time.minute()
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.second
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_second, second_body, "second", |_global, zdt| js_number(
    zdt.get_local_date_time()?.time.second()
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.millisecond
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_millisecond, millisecond_body, "millisecond", |_global, zdt| js_number(
    zdt.get_local_date_time()?.time.millisecond()
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.microsecond
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_microsecond, microsecond_body, "microsecond", |_global, zdt| js_number(
    zdt.get_local_date_time()?.time.microsecond()
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.nanosecond
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_nanosecond, nanosecond_body, "nanosecond", |_global, zdt| js_number(
    zdt.get_local_date_time()?.time.nanosecond()
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.offset
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_offset, offset_body, "offset", |global, zdt| str_value(
    global.vm(),
    &format_time_zone_offset_string(zdt.get_offset_nanoseconds()?)
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.offsetnanoseconds
zoned_date_time_getter!(
    temporal_zoned_date_time_prototype_getter_offset_nanoseconds,
    offset_nanoseconds_body,
    "offsetNanoseconds",
    |_global, zdt| js_number(zdt.get_offset_nanoseconds()? as f64)
);
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.dayofweek
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_day_of_week, day_of_week_body, "dayOfWeek", |_global, zdt| js_number(
    day_of_week(zdt.get_local_date_time()?.date)
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.dayofyear
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_day_of_year, day_of_year_body, "dayOfYear", |_global, zdt| js_number(
    day_of_year(zdt.get_local_date_time()?.date)
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.weekofyear
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_week_of_year, week_of_year_body, "weekOfYear", |_global, zdt| {
    // `calendarIsISO`: fora do ISO, `undefined`.
    if crate::runtime::temporal_calendar::calendar_is_iso(zdt.calendar_id()) {
        js_number(week_of_year(zdt.get_local_date_time()?.date))
    } else {
        crate::runtime::js_value::js_undefined()
    }
});
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.yearofweek
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_year_of_week, year_of_week_body, "yearOfWeek", |_global, zdt| {
    if crate::runtime::temporal_calendar::calendar_is_iso(zdt.calendar_id()) {
        js_number(year_of_week(zdt.get_local_date_time()?.date))
    } else {
        crate::runtime::js_value::js_undefined()
    }
});
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.hoursinday
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_hours_in_day, hours_in_day_body, "hoursInDay", |_global, zdt| {
    // Passos 3 a 5: `today = isoDateTime.[[ISODate]]`. Passo 6: `tomorrow = AddDaysToISODate(today, 1)`.
    let today = zdt.get_local_date_time()?.date;
    let tomorrow = add_days_to_iso_date(today, 1);
    // Passos 7 e 8: `GetStartOfDay` de hoje e de amanhã.
    let today_ns = get_start_of_day(zdt.time_zone(), today)?.epoch_nanoseconds();
    let tomorrow_ns = get_start_of_day(zdt.time_zone(), tomorrow)?.epoch_nanoseconds();
    // Passos 9 e 10: `TotalTimeDuration(diff, ~hour~)`.
    js_number((tomorrow_ns - today_ns) as f64 / ExactTime::NS_PER_HOUR as f64)
});
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.daysinweek
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_days_in_week, days_in_week_body, "daysInWeek", |_global, zdt| {
    // Passo 3: `GetISODateTimeFor` só pelo efeito (o `RangeError`); toda semana tem 7 dias.
    zdt.get_local_date_time()?;
    js_number(DAYS_PER_WEEK)
});
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.daysinmonth
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_days_in_month, days_in_month_body, "daysInMonth", |_global, zdt| {
    js_number(calendar_fields(zdt.calendar_id(), &zdt.get_local_date_time()?.date).days_in_month)
});
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.daysinyear
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_days_in_year, days_in_year_body, "daysInYear", |_global, zdt| js_number(
    calendar_fields(zdt.calendar_id(), &zdt.get_local_date_time()?.date).days_in_year
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.monthsinyear
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_months_in_year, months_in_year_body, "monthsInYear", |_global, zdt| js_number(calendar_fields(zdt.calendar_id(), &zdt.get_local_date_time()?.date).months_in_year));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.inleapyear
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_in_leap_year, in_leap_year_body, "inLeapYear", |_global, zdt| js_boolean(
    calendar_fields(zdt.calendar_id(), &zdt.get_local_date_time()?.date).in_leap_year
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.era
// `CalendarEra(calendar, isoDate)`: `undefined` no calendário ISO (`calendarHasEras` é falso); o `GetISODateTimeFor`
// do passo 3 ainda roda.
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_era, era_body, "era", |global, zdt| calendar_fields(zdt.calendar_id(), &zdt.get_local_date_time()?.date).era_value(global.vm()));
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.erayear
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_era_year, era_year_body, "eraYear", |_global, zdt| calendar_fields(zdt.calendar_id(), &zdt.get_local_date_time()?.date).era_year_value());
// https://tc39.es/proposal-temporal/#sec-get-temporal.zoneddatetime.prototype.epochmilliseconds
zoned_date_time_getter!(temporal_zoned_date_time_prototype_getter_epoch_milliseconds, epoch_milliseconds_body, "epochMilliseconds", |_global, zdt| {
    js_number(zdt.exact_time().floor_epoch_milliseconds() as f64)
});

/// `zoned date timePrototypeTableValues`, na ordem do `@begin`: os métodos (`DontEnum|Function`) e os acessores
/// (`DontEnum|ReadOnly|CustomAccessor`, sem setter).
static PROTOTYPE_TABLE_VALUES: [HashTableValue; 48] = [
    native_entry("with", temporal_zoned_date_time_prototype_func_with, 1),
    native_entry("withPlainTime", temporal_zoned_date_time_prototype_func_with_plain_time, 0),
    native_entry("withTimeZone", temporal_zoned_date_time_prototype_func_with_time_zone, 1),
    native_entry("withCalendar", temporal_zoned_date_time_prototype_func_with_calendar, 1),
    native_entry("add", temporal_zoned_date_time_prototype_func_add, 1),
    native_entry("subtract", temporal_zoned_date_time_prototype_func_subtract, 1),
    native_entry("until", temporal_zoned_date_time_prototype_func_until, 1),
    native_entry("since", temporal_zoned_date_time_prototype_func_since, 1),
    native_entry("round", temporal_zoned_date_time_prototype_func_round, 1),
    native_entry("startOfDay", temporal_zoned_date_time_prototype_func_start_of_day, 0),
    native_entry("getTimeZoneTransition", temporal_zoned_date_time_prototype_func_get_time_zone_transition, 1),
    native_entry("equals", temporal_zoned_date_time_prototype_func_equals, 1),
    native_entry("toInstant", temporal_zoned_date_time_prototype_func_to_instant, 0),
    native_entry("toPlainDateTime", temporal_zoned_date_time_prototype_func_to_plain_date_time, 0),
    native_entry("toPlainDate", temporal_zoned_date_time_prototype_func_to_plain_date, 0),
    native_entry("toPlainTime", temporal_zoned_date_time_prototype_func_to_plain_time, 0),
    native_entry("toString", temporal_zoned_date_time_prototype_func_to_string, 0),
    native_entry("toJSON", temporal_zoned_date_time_prototype_func_to_json, 0),
    native_entry("toLocaleString", temporal_zoned_date_time_prototype_func_to_locale_string, 0),
    native_entry("valueOf", temporal_zoned_date_time_prototype_func_value_of, 0),
    custom_getter_entry("epochNanoseconds", temporal_zoned_date_time_prototype_getter_epoch_nanoseconds),
    custom_getter_entry("timeZoneId", temporal_zoned_date_time_prototype_getter_time_zone_id),
    custom_getter_entry("calendarId", temporal_zoned_date_time_prototype_getter_calendar_id),
    custom_getter_entry("year", temporal_zoned_date_time_prototype_getter_year),
    custom_getter_entry("month", temporal_zoned_date_time_prototype_getter_month),
    custom_getter_entry("monthCode", temporal_zoned_date_time_prototype_getter_month_code),
    custom_getter_entry("day", temporal_zoned_date_time_prototype_getter_day),
    custom_getter_entry("hour", temporal_zoned_date_time_prototype_getter_hour),
    custom_getter_entry("minute", temporal_zoned_date_time_prototype_getter_minute),
    custom_getter_entry("second", temporal_zoned_date_time_prototype_getter_second),
    custom_getter_entry("millisecond", temporal_zoned_date_time_prototype_getter_millisecond),
    custom_getter_entry("microsecond", temporal_zoned_date_time_prototype_getter_microsecond),
    custom_getter_entry("nanosecond", temporal_zoned_date_time_prototype_getter_nanosecond),
    custom_getter_entry("offset", temporal_zoned_date_time_prototype_getter_offset),
    custom_getter_entry("offsetNanoseconds", temporal_zoned_date_time_prototype_getter_offset_nanoseconds),
    custom_getter_entry("dayOfWeek", temporal_zoned_date_time_prototype_getter_day_of_week),
    custom_getter_entry("dayOfYear", temporal_zoned_date_time_prototype_getter_day_of_year),
    custom_getter_entry("weekOfYear", temporal_zoned_date_time_prototype_getter_week_of_year),
    custom_getter_entry("yearOfWeek", temporal_zoned_date_time_prototype_getter_year_of_week),
    custom_getter_entry("hoursInDay", temporal_zoned_date_time_prototype_getter_hours_in_day),
    custom_getter_entry("daysInWeek", temporal_zoned_date_time_prototype_getter_days_in_week),
    custom_getter_entry("daysInMonth", temporal_zoned_date_time_prototype_getter_days_in_month),
    custom_getter_entry("daysInYear", temporal_zoned_date_time_prototype_getter_days_in_year),
    custom_getter_entry("monthsInYear", temporal_zoned_date_time_prototype_getter_months_in_year),
    custom_getter_entry("inLeapYear", temporal_zoned_date_time_prototype_getter_in_leap_year),
    custom_getter_entry("era", temporal_zoned_date_time_prototype_getter_era),
    custom_getter_entry("eraYear", temporal_zoned_date_time_prototype_getter_era_year),
    custom_getter_entry("epochMilliseconds", temporal_zoned_date_time_prototype_getter_epoch_milliseconds),
];

/// A tabela estática do protótipo.
static PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &PROTOTYPE_TABLE_VALUES };

/// `class TemporalZonedDateTimePrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct TemporalZonedDateTimePrototype;

impl TemporalZonedDateTimePrototype {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`; sem `HasStaticPropertyTable` porque os
    /// membros são postos direto em `finishCreation` (a tabela estática do C++ é essa mesma lista).
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | crate::runtime::js_type_info::HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, TemporalZonedDateTimePrototype::STRUCTURE_FLAGS),
            &TEMPORAL_ZONED_DATE_TIME_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `TemporalZonedDateTimePrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        TemporalZonedDateTimePrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm)` com `zonedDateTimePrototypeTable`: os métodos (`DontEnum|Function`, com o comprimento da
    /// tabela), os acessores (`DontEnum|ReadOnly|CustomAccessor`) e `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
    fn finish_creation(prototype: &JSObject, vm: &VM, _global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);

        put_to_string_tag(vm, prototype, TEMPORAL_ZONED_DATE_TIME_PROTOTYPE_S_INFO.class_name);
    }
}
