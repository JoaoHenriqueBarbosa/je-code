//! Porte de `runtime/TemporalPlainYearMonthPrototype.{h,cpp}`: `Temporal.PlainYearMonth.prototype` (um
//! `JSNonFinalObject` com o `ClassInfo` `"Temporal.PlainYearMonth"`): `add`, `subtract`, `until`, `since`,
//! `toPlainDate`, `toString`, `toJSON`, `toLocaleString`, `with`, `equals`, `valueOf`, os acessores `calendarId` a
//! `eraYear` (`DontEnum|ReadOnly|CustomAccessor`) e `@@toStringTag`.
//!
//! DIVERGÊNCIAS:
//! - `toLocaleString` delega ao `IntlDateTimeFormat` com `PlainYearMonth` (`intl_date_time_format/temporal.rs`).
//! - Os acessores só têm o ramo do calendário ISO: toda célula é ISO (ver `temporal_calendar.rs`), então `era` e
//!   `eraYear` são sempre `undefined`.
//! - `toPlainDate` só tem o ramo ISO (`regulateISODate` com `~constrain~`); o de calendário não ISO é do
//!   `CalendarICUBridge`.
//! - O acessor `calendarId` usa a mensagem de marca `...prototype.calendar called on...`, como o C++.

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{put_to_string_tag};
use crate::runtime::lookup::{HashTable, HashTableValue};
use crate::runtime::lookup::{custom_getter_entry, native_entry};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_date_time_format::temporal::to_locale_string as intl_to_locale_string;
use crate::runtime::intl_date_time_format::{Defaults, Required};
use crate::runtime::intl_support::{get_property, str_value};
use crate::runtime::iso8601::{is_date_time_within_limits, Duration, InternalDuration, PlainTime, PlainYearMonth};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::temporal_calendar::{calendar_id_to_string, calendar_is_iso, read_calendar_fields_from_object, FieldSetType};
use crate::runtime::temporal_core_calendar_fields::{
    difference_year_month, plain_year_month_add, plain_year_month_to_plain_date, plain_year_month_with,
};
use crate::runtime::temporal_core_duration::{get_utc_epoch_nanoseconds, round_relative_duration, temporal_duration_from_internal};
use crate::runtime::temporal_core_iso_date::{iso_date_compare, regulate_iso_date};
use crate::runtime::temporal_duration::{create_temporal_duration, TemporalDuration};
use crate::runtime::temporal_object::{
    extract_difference_options, is_partial_temporal_object, to_integer_with_truncation, to_temporal_overflow_value, DifferenceOperation,
    TemporalOverflow, TemporalUnit, UnitGroup,
};
use crate::runtime::temporal_plain_date::create_temporal_date;
use crate::runtime::temporal_plain_year_month::{create_temporal_year_month, TemporalPlainYearMonth, TemporalPlainYearMonthRef};
use crate::runtime::vm::VM;

/// `const ClassInfo TemporalPlainYearMonthPrototype::s_info` (`"Temporal.PlainYearMonth"`).
pub static TEMPORAL_PLAIN_YEAR_MONTH_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "Temporal.PlainYearMonth",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&PROTOTYPE_TABLE), inherits_js_type_range: None,
};

/// O `dynamicDowncast<TemporalPlainYearMonth>(callFrame->thisValue())` com o `TypeError` de marca de cada membro.
fn this_year_month(this_value: JSValue, member: &str) -> Result<TemporalPlainYearMonthRef, Thrown> {
    TemporalPlainYearMonth::from_value(&this_value)
        .ok_or_else(|| Thrown::TypeError(format!("Temporal.PlainYearMonth.prototype.{member} called on value that's not a PlainYearMonth")))
}

/// `addDurationToYearMonth<op>` (`AddDurationToYearMonth`): https://tc39.es/proposal-temporal/#sec-temporal-adddurationtoyearmonth
fn add_duration_to_year_month(global_object: &JSGlobalObject, call: &HostCall, subtract: bool) -> HostResult {
    let member = if subtract { "subtract" } else { "add" };
    let year_month = this_year_month(call.this_value(), member)?;

    // Passo 1: `duration = ? ToTemporalDuration(temporalDurationLike)`.
    let mut duration = TemporalDuration::to_temporal_duration_record(global_object, call.argument(0))?;
    // Passo 2: `subtract` nega.
    if subtract {
        duration = -duration;
    }

    // Passos 4 e 5: `GetOptionsObject` e `GetTemporalOverflowOption`.
    let overflow = to_temporal_overflow_value(global_object, call.argument(1))?;

    // Passos 6 e 7: semanas, dias ou tempo diferente de zero é `RangeError`.
    if duration.weeks() != 0
        || duration.days() != 0
        || duration.hours() != 0
        || duration.minutes() != 0
        || duration.seconds() != 0
        || duration.milliseconds() != 0
        || duration.microseconds() != 0
        || duration.nanoseconds() != 0
    {
        return Err(Thrown::range_error("Duration must not have units below months for PlainYearMonth arithmetic"));
    }

    // Passos 8 a 14 (`plainYearMonthAdd`) e passo 15: `CreateTemporalYearMonth(isoDate, calendar)`.
    let result = plain_year_month_add(year_month.calendar_id(), *year_month.plain_year_month().iso_plain_date(), &duration, overflow)?;
    Ok(TemporalPlainYearMonth::create(
        global_object.vm(),
        &global_object.plain_year_month_structure(),
        PlainYearMonth::from_date(result.iso_date),
        year_month.calendar_id(),
    )
    .as_value())
}

/// `temporalPlainYearMonthPrototypeFuncAdd`: https://tc39.es/proposal-temporal/#sec-temporal.plainyearmonth.prototype.add
fn add_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    add_duration_to_year_month(global_object, call, false)
}

/// `temporalPlainYearMonthPrototypeFuncSubtract`: https://tc39.es/proposal-temporal/#sec-temporal.plainyearmonth.prototype.subtract
fn subtract_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    add_duration_to_year_month(global_object, call, true)
}

/// `temporalPlainYearMonthPrototypeFuncWith`: https://tc39.es/proposal-temporal/#sec-temporal.plainyearmonth.prototype.with
fn with_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let year_month = this_year_month(call.this_value(), "with")?;

    // Passo 3: `IsPartialTemporalObject(temporalYearMonthLike)` falso é `TypeError`.
    let temporal_year_month_like = call.argument(0);
    if !is_partial_temporal_object(global_object, temporal_year_month_like)? {
        return Err(Thrown::type_error("First argument to Temporal.PlainYearMonth.prototype.with must be a partial Temporal object"));
    }

    // Passo 4: `calendar = plainYearMonth.[[Calendar]]`, guardado no receptor.
    let calendar_id = year_month.calendar_id();

    // Passo 6: `PrepareCalendarFields(calendar, temporalYearMonthLike, «year, month, monthCode», «», ~partial~)`.
    let partial_fields = read_calendar_fields_from_object(global_object, temporal_year_month_like, calendar_id, FieldSetType::YearMonth, None)?;
    // `~partial~` lança `TypeError` se nenhum dos campos pedidos veio com valor.
    if partial_fields.year.is_none()
        && partial_fields.month.is_none()
        && partial_fields.month_code.is_none()
        && partial_fields.era.is_none()
        && partial_fields.era_year.is_none()
    {
        return Err(Thrown::type_error("Object must contain at least one Temporal date property"));
    }

    // Passos 8 e 9: `GetOptionsObject` e `GetTemporalOverflowOption`.
    let overflow = to_temporal_overflow_value(global_object, call.argument(1))?;

    // Passos 5, 7 e 10: `ISODateToFields`, `CalendarMergeFields` e `CalendarYearMonthFromFields`.
    let resolved = plain_year_month_with(calendar_id, *year_month.plain_year_month().iso_plain_date(), &partial_fields, overflow)?;

    // Passo 11: `CreateTemporalYearMonth(isoDate, calendar)`.
    Ok(create_temporal_year_month(global_object, resolved.iso_date, calendar_id, None)?.as_value())
}

/// `differenceTemporalPlainYearMonth<op>`: https://tc39.es/proposal-temporal/#sec-temporal-differencetemporalplainyearmonth
fn difference_temporal_plain_year_month(global_object: &JSGlobalObject, call: &HostCall, operation: DifferenceOperation) -> HostResult {
    let member = if operation == DifferenceOperation::Until { "until" } else { "since" };
    // Passos 1 e 2 do membro: marca.
    let year_month = this_year_month(call.this_value(), member)?;

    // Passo 1: `other = ? ToTemporalYearMonth(other)`.
    let other = TemporalPlainYearMonth::from(global_object, call.argument(0), JSValue::Undefined)?;

    // Passo 2: `calendar = yearMonth.[[Calendar]]`. Passo 3: `CalendarEquals` falso é `RangeError`.
    let calendar_id = year_month.calendar_id();
    if calendar_id != other.calendar_id() {
        return Err(Thrown::range_error("cannot compute difference between year-months with different calendars"));
    }

    // Passos 4 e 5: `GetOptionsObject` e `GetDifferenceSettings(op, resolvedOptions, ~date~, «week, day», ~month~, ~year~)`.
    let (smallest_unit, largest_unit, rounding_mode, increment) =
        extract_difference_options(global_object, call.argument(1), UnitGroup::Date, TemporalUnit::Month, TemporalUnit::Year, operation)?;
    // As unidades `«week, day»` que a chamada acima não recusa.
    if largest_unit == TemporalUnit::Week || largest_unit == TemporalUnit::Day {
        return Err(Thrown::range_error("largestUnit must be one of year, years, month, months"));
    }
    if smallest_unit == TemporalUnit::Week || smallest_unit == TemporalUnit::Day {
        return Err(Thrown::range_error("smallestUnit must be one of year, years, month, months"));
    }

    let this_iso_date = *year_month.plain_year_month().iso_plain_date();
    let other_iso_date = *other.plain_year_month().iso_plain_date();

    // Passo 6: `CompareISODate = 0` devolve a duração zero.
    if iso_date_compare(this_iso_date, other_iso_date) == 0 {
        return Ok(create_temporal_duration(global_object, Duration::default(), None)?.as_value());
    }

    // As duas pontas precisam estar na faixa de data e hora antes da aritmética de calendário.
    let within_limits = |date: crate::runtime::iso8601::PlainDate| is_date_time_within_limits(date.year(), date.month(), date.day(), 12, 0, 0, 0, 0, 0);
    if !within_limits(this_iso_date) || !within_limits(other_iso_date) {
        return Err(Thrown::range_error("date/time value is outside of supported range"));
    }

    // Passos 7 a 13: o dia 1 nas duas pontas e `CalendarDateUntil`.
    let date_difference = difference_year_month(calendar_id, this_iso_date, other_iso_date, largest_unit)?;

    // Passos 14 e 15: `AdjustDateDurationRecord(dateDifference, 0, 0)` e `CombineDateAndTimeDuration(..., 0)`.
    let mut duration =
        InternalDuration::new(Duration::new(date_difference.years(), date_difference.months(), 0, 0, 0, 0, 0, 0, 0, 0), 0);

    // Passo 16: `smallestUnit` diferente de `~month~` ou `increment` diferente de 1: `RoundRelativeDuration`.
    if smallest_unit != TemporalUnit::Month || increment != 1.0 {
        let origin_epoch_ns = get_utc_epoch_nanoseconds(this_iso_date, PlainTime::default());
        let dest_epoch_ns = get_utc_epoch_nanoseconds(other_iso_date, PlainTime::default());
        round_relative_duration(
            calendar_id,
            &mut duration,
            origin_epoch_ns,
            dest_epoch_ns,
            this_iso_date,
            PlainTime::default(),
            largest_unit,
            increment,
            smallest_unit,
            rounding_mode,
        )?;
    }

    // Passo 17: `result = ! TemporalDurationFromInternal(duration, ~day~)`.
    let mut result = temporal_duration_from_internal(&duration, TemporalUnit::Day)?;
    // Passo 18: `since` nega.
    if operation == DifferenceOperation::Since {
        result = -result;
    }
    // Passo 19.
    Ok(create_temporal_duration(global_object, result, None)?.as_value())
}

/// `temporalPlainYearMonthPrototypeFuncUntil`: https://tc39.es/proposal-temporal/#sec-temporal.plainyearmonth.prototype.until
fn until_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    difference_temporal_plain_year_month(global_object, call, DifferenceOperation::Until)
}

/// `temporalPlainYearMonthPrototypeFuncSince`: https://tc39.es/proposal-temporal/#sec-temporal.plainyearmonth.prototype.since
fn since_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    difference_temporal_plain_year_month(global_object, call, DifferenceOperation::Since)
}

/// `temporalPlainYearMonthPrototypeFuncEquals`: https://tc39.es/proposal-temporal/#sec-temporal.plainyearmonth.prototype.equals
fn equals_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let year_month = this_year_month(call.this_value(), "equals")?;
    // Passo 3: `other = ? ToTemporalYearMonth(other)`.
    let other = TemporalPlainYearMonth::from(global_object, call.argument(0), JSValue::Undefined)?;
    // Passo 4: `CompareISODate` diferente de 0 é falso.
    if year_month.plain_year_month() != other.plain_year_month() {
        return Ok(js_boolean(false));
    }
    // Passo 5: `CalendarEquals`.
    Ok(js_boolean(year_month.calendar_id() == other.calendar_id()))
}

/// `temporalPlainYearMonthPrototypeFuncToPlainDate`: https://tc39.es/proposal-temporal/#sec-temporal.plainyearmonth.prototype.toplaindate
fn to_plain_date_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let year_month = this_year_month(call.this_value(), "toPlainDate")?;

    // Passo 3: `item` que não é objeto é `TypeError`.
    let item = call.argument(0);
    if !item.is_object() {
        return Err(Thrown::type_error("Temporal.PlainYearMonth.prototype.toPlainDate: item is not an object"));
    }

    // Passos 5 a 7: o ano e o mês vêm do receptor e o único campo lido é `day` (`~to-positive-integer-with-truncation~`:
    // `RangeError` para valor não finito ou menor ou igual a zero).
    let day_property = get_property(global_object, item, "day")?;
    if day_property.is_undefined() {
        return Err(Thrown::type_error("Temporal.PlainYearMonth.prototype.toPlainDate: item does not have a day field"));
    }
    let day = to_integer_with_truncation(global_object, day_property)?;
    if !day.is_finite() {
        return Err(Thrown::range_error("day property must be finite"));
    }
    if day <= 0.0 {
        return Err(Thrown::range_error("day property must be a positive integer"));
    }
    // `clampTo<uint8_t>`.
    let day = day.min(255.0) as u8;

    // Passo 8: `CalendarDateFromFields(calendar, merged, ~constrain~)` e passo 9: `CreateTemporalDate(isoDate, calendar)`.
    if !calendar_is_iso(year_month.calendar_id()) {
        let resolved = plain_year_month_to_plain_date(year_month.calendar_id(), *year_month.plain_year_month().iso_plain_date(), day)?;
        return Ok(create_temporal_date(global_object, resolved.iso_date, resolved.calendar_id, None)?.as_value());
    }
    // O ramo ISO: `RegulateISODate`.
    let plain_date = regulate_iso_date(year_month.year(), i32::from(year_month.month()), i64::from(day), TemporalOverflow::Constrain)
        .map_err(|_| Thrown::range_error("Temporal.PlainYearMonth.prototype.toPlainDate: date is invalid"))?;
    Ok(create_temporal_date(global_object, plain_date, year_month.calendar_id(), None)?.as_value())
}

/// `temporalPlainYearMonthPrototypeFuncToString`: https://tc39.es/proposal-temporal/#sec-temporal.plainyearmonth.prototype.tostring
fn to_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let year_month = this_year_month(call.this_value(), "toString")?;
    // Passos 3 a 5: `GetOptionsObject`, `GetTemporalShowCalendarNameOption` e `TemporalYearMonthToString`.
    Ok(str_value(global_object.vm(), &year_month.to_string_with_options(global_object, call.argument(0))?))
}

/// `temporalPlainYearMonthPrototypeFuncToJSON`: https://tc39.es/proposal-temporal/#sec-temporal.plainyearmonth.prototype.tojson
fn to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca. Passo 3: `TemporalYearMonthToString(yearMonth, ~auto~)`.
    let year_month = this_year_month(call.this_value(), "toJSON")?;
    Ok(str_value(global_object.vm(), &year_month.to_string()))
}

/// `temporalPlainYearMonthPrototypeFuncToLocaleString`: https://tc39.es/proposal-temporal/#sup-temporal.plainyearmonth.prototype.tolocalestring
/// Passos 3 e 4 (ECMA-402): `CreateDateTimeFormat(%DateTimeFormat%, locales, options, ~date~, ~date~)` e
/// `FormatDateTime` do `PlainYearMonth`.
fn to_locale_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    this_year_month(call.this_value(), "toLocaleString")?;
    let text = intl_to_locale_string(global_object, call.argument(0), call.argument(1), Required::Date, Defaults::Date, call.this_value())?;
    Ok(str_value(global_object.vm(), &text))
}

/// `temporalPlainYearMonthPrototypeFuncValueOf`: https://tc39.es/proposal-temporal/#sec-temporal.plainyearmonth.prototype.valueof
fn value_of_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error(
        "Temporal.PlainYearMonth.prototype.valueOf must not be called. To compare PlainYearMonth values, use Temporal.PlainYearMonth.compare",
    ))
}

host_function!(temporal_plain_year_month_prototype_func_add, add_body);
host_function!(temporal_plain_year_month_prototype_func_subtract, subtract_body);
host_function!(temporal_plain_year_month_prototype_func_until, until_body);
host_function!(temporal_plain_year_month_prototype_func_since, since_body);
host_function!(temporal_plain_year_month_prototype_func_to_plain_date, to_plain_date_body);
host_function!(temporal_plain_year_month_prototype_func_to_string, to_string_body);
host_function!(temporal_plain_year_month_prototype_func_to_json, to_json_body);
host_function!(temporal_plain_year_month_prototype_func_to_locale_string, to_locale_string_body);
host_function!(temporal_plain_year_month_prototype_func_with, with_body);
host_function!(temporal_plain_year_month_prototype_func_equals, equals_body);
host_function!(temporal_plain_year_month_prototype_func_value_of, value_of_body);

// https://tc39.es/proposal-temporal/#sec-get-temporal.plainyearmonth.prototype.calendarid
// A mensagem de marca do C++ diz `calendar`, não `calendarId`.
crate::temporal_getter!(temporal_plain_year_month_prototype_getter_calendar_id, calendar_id_body, this_year_month, "calendar", |global, ym| str_value(
    global.vm(),
    calendar_id_to_string(ym.calendar_id())
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plainyearmonth.prototype.year
crate::temporal_getter!(temporal_plain_year_month_prototype_getter_year, year_body, this_year_month, "year", |_global, ym| js_number(ym.calendar_date_fields().year));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plainyearmonth.prototype.month
crate::temporal_getter!(temporal_plain_year_month_prototype_getter_month, month_body, this_year_month, "month", |_global, ym| js_number(ym.calendar_date_fields().month));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plainyearmonth.prototype.monthcode
crate::temporal_getter!(temporal_plain_year_month_prototype_getter_month_code, month_code_body, this_year_month, "monthCode", |global, ym| str_value(
    global.vm(),
    &ym.calendar_date_fields().month_code
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plainyearmonth.prototype.daysinmonth
crate::temporal_getter!(temporal_plain_year_month_prototype_getter_days_in_month, days_in_month_body, this_year_month, "daysInMonth", |_global, ym| js_number(
    ym.calendar_date_fields().days_in_month
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plainyearmonth.prototype.daysinyear
crate::temporal_getter!(temporal_plain_year_month_prototype_getter_days_in_year, days_in_year_body, this_year_month, "daysInYear", |_global, ym| js_number(
    ym.calendar_date_fields().days_in_year
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plainyearmonth.prototype.monthsinyear
crate::temporal_getter!(temporal_plain_year_month_prototype_getter_months_in_year, months_in_year_body, this_year_month, "monthsInYear", |_global, ym| js_number(ym.calendar_date_fields().months_in_year));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plainyearmonth.prototype.inleapyear
crate::temporal_getter!(temporal_plain_year_month_prototype_getter_in_leap_year, in_leap_year_body, this_year_month, "inLeapYear", |_global, ym| js_boolean(
    ym.calendar_date_fields().in_leap_year
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plainyearmonth.prototype.era
// `calendarEra` é `nullopt` nos calendários sem era (ISO, `chinese`, `dangi`): `undefined`.
crate::temporal_getter!(temporal_plain_year_month_prototype_getter_era, era_body, this_year_month, "era", |global, ym| ym.calendar_date_fields().era_value(global.vm()));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plainyearmonth.prototype.erayear
crate::temporal_getter!(temporal_plain_year_month_prototype_getter_era_year, era_year_body, this_year_month, "eraYear", |_global, ym| ym.calendar_date_fields().era_year_value());

/// `plain year monthPrototypeTableValues`, na ordem do `@begin`: os métodos (`DontEnum|Function`) e os acessores
/// (`DontEnum|ReadOnly|CustomAccessor`, sem setter).
static PROTOTYPE_TABLE_VALUES: [HashTableValue; 21] = [
    native_entry("add", temporal_plain_year_month_prototype_func_add, 1),
    native_entry("subtract", temporal_plain_year_month_prototype_func_subtract, 1),
    native_entry("until", temporal_plain_year_month_prototype_func_until, 1),
    native_entry("since", temporal_plain_year_month_prototype_func_since, 1),
    native_entry("toPlainDate", temporal_plain_year_month_prototype_func_to_plain_date, 1),
    native_entry("toString", temporal_plain_year_month_prototype_func_to_string, 0),
    native_entry("toJSON", temporal_plain_year_month_prototype_func_to_json, 0),
    native_entry("toLocaleString", temporal_plain_year_month_prototype_func_to_locale_string, 0),
    native_entry("with", temporal_plain_year_month_prototype_func_with, 1),
    native_entry("equals", temporal_plain_year_month_prototype_func_equals, 1),
    native_entry("valueOf", temporal_plain_year_month_prototype_func_value_of, 0),
    custom_getter_entry("calendarId", temporal_plain_year_month_prototype_getter_calendar_id),
    custom_getter_entry("year", temporal_plain_year_month_prototype_getter_year),
    custom_getter_entry("month", temporal_plain_year_month_prototype_getter_month),
    custom_getter_entry("monthCode", temporal_plain_year_month_prototype_getter_month_code),
    custom_getter_entry("daysInMonth", temporal_plain_year_month_prototype_getter_days_in_month),
    custom_getter_entry("daysInYear", temporal_plain_year_month_prototype_getter_days_in_year),
    custom_getter_entry("monthsInYear", temporal_plain_year_month_prototype_getter_months_in_year),
    custom_getter_entry("inLeapYear", temporal_plain_year_month_prototype_getter_in_leap_year),
    custom_getter_entry("era", temporal_plain_year_month_prototype_getter_era),
    custom_getter_entry("eraYear", temporal_plain_year_month_prototype_getter_era_year),
];

/// A tabela estática do protótipo.
static PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &PROTOTYPE_TABLE_VALUES };

/// `class TemporalPlainYearMonthPrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct TemporalPlainYearMonthPrototype;

impl TemporalPlainYearMonthPrototype {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`; sem `HasStaticPropertyTable` porque os
    /// membros são postos direto em `finishCreation`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | crate::runtime::js_type_info::HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, TemporalPlainYearMonthPrototype::STRUCTURE_FLAGS),
            &TEMPORAL_PLAIN_YEAR_MONTH_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: o construtor e o `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        TemporalPlainYearMonthPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm)` com `plainYearMonthPrototypeTable`: os métodos (`DontEnum|Function`, com o comprimento da
    /// tabela), os acessores (`DontEnum|ReadOnly|CustomAccessor`) e `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
    fn finish_creation(prototype: &JSObject, vm: &VM, _global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);

        put_to_string_tag(vm, prototype, TEMPORAL_PLAIN_YEAR_MONTH_PROTOTYPE_S_INFO.class_name);
    }
}
