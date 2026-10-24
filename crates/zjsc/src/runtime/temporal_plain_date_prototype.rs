//! Porte de `runtime/TemporalPlainDatePrototype.{h,cpp}`: `Temporal.PlainDate.prototype` (um `JSNonFinalObject`
//! com o `ClassInfo` `"Temporal.PlainDate"`): `toPlainMonthDay`, `toPlainYearMonth`, `withCalendar`, `add`, `subtract`, `with`, `until`, `since`,
//! `equals`, `toPlainDateTime`, `toString`, `toJSON`, `toLocaleString`, `valueOf`, os acessores `calendarId` a
//! `eraYear` (`DontEnum|ReadOnly|CustomAccessor`) e `@@toStringTag`.
//!
//! DIVERGÊNCIAS:
//! - `toLocaleString` delega ao `IntlDateTimeFormat` com `PlainDate` (`intl_date_time_format/temporal.rs`).
//! - Os acessores despacham pelo calendário da célula (`calendar_fields`); `weekOfYear` e `yearOfWeek` são
//!   `undefined` fora do ISO (`calendarWeekOfYear`), `dayOfWeek` é o ISO em todo calendário.
//! - O `toString` de `calendarName` monta o texto aqui, como o C++ (`TemporalDateToString`).

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{put_to_string_tag};
use crate::runtime::lookup::{HashTable, HashTableValue};
use crate::runtime::lookup::{custom_getter_entry, native_entry};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_date_time_format::temporal::to_locale_string as intl_to_locale_string;
use crate::runtime::intl_date_time_format::{Defaults, Required};
use crate::runtime::intl_support::{get_options_object, get_property, str_value};
use crate::runtime::iso8601::{day_of_week, temporal_date_to_string, week_of_year, year_of_week, Duration, DAYS_PER_WEEK};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, js_number, js_undefined, JSValue};
use crate::runtime::temporal_calendar_icu::{calendar_date_add, calendar_fields};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::temporal_calendar::{
    calendar_id_to_string, calendar_is_iso, read_calendar_fields_from_object, temporal_show_calendar_name,
    to_temporal_calendar_identifier, CalendarNameOption, FieldSetType,
};
use crate::runtime::temporal_core_calendar_fields::plain_date_with;
use crate::runtime::temporal_core_duration::to_date_duration_record_without_time;
use crate::runtime::temporal_duration::{create_temporal_duration, TemporalDuration};
use crate::runtime::temporal_object::{is_partial_temporal_object, to_temporal_overflow_value, TemporalOverflow};
use crate::runtime::temporal_plain_date::{create_temporal_date, TemporalPlainDate, TemporalPlainDateRef};
use crate::runtime::temporal_plain_date_time::create_temporal_date_time;
use crate::runtime::temporal_plain_month_day::create_temporal_month_day;
use crate::runtime::temporal_plain_year_month::create_temporal_year_month;
use crate::runtime::temporal_core_calendar_fields::{iso_date_to_fields, month_day_from_fields, year_month_from_fields, ResolveType};
use crate::runtime::temporal_plain_time::TemporalPlainTime;
use crate::runtime::temporal_core_zoned_date_time::get_start_of_day;
use crate::runtime::temporal_object::TemporalDisambiguation;
use crate::runtime::temporal_time_zone::get_epoch_nanoseconds_for;
use crate::runtime::temporal_zoned_date_time::{create_temporal_zoned_date_time, to_temporal_time_zone_identifier, TemporalZonedDateTime};
use crate::runtime::vm::VM;

/// `const ClassInfo TemporalPlainDatePrototype::s_info` (`"Temporal.PlainDate"`).
pub static TEMPORAL_PLAIN_DATE_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "Temporal.PlainDate",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&PROTOTYPE_TABLE), inherits_js_type_range: None,
};

/// O `dynamicDowncast<TemporalPlainDate>(callFrame->thisValue())` com o `TypeError` de marca de cada membro.
fn this_plain_date(this_value: JSValue, member: &str) -> Result<TemporalPlainDateRef, Thrown> {
    TemporalPlainDate::from_value(&this_value)
        .ok_or_else(|| Thrown::TypeError(format!("Temporal.PlainDate.prototype.{member} called on value that's not a PlainDate")))
}

/// `temporalPlainDatePrototypeFuncToPlainMonthDay`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.toplainmonthday
fn to_plain_month_day_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date = this_plain_date(call.this_value(), "toPlainMonthDay")?;
    let calendar_id = plain_date.calendar_id();
    // Passos 3 a 5: `ISODateToFields(calendar, isoDate, ~month-day~)` e `CalendarMonthDayFromFields(..., ~constrain~)`
    // (no ISO, o ano de referência 1972; nos outros, a data de referência do calendário).
    let fields = iso_date_to_fields(calendar_id, plain_date.plain_date(), ResolveType::MonthDay)?;
    let resolved = month_day_from_fields(calendar_id, &fields, TemporalOverflow::Constrain)?;
    // Passo 6: `CreateTemporalMonthDay`.
    Ok(create_temporal_month_day(global_object, resolved.iso_date, calendar_id, None)?.as_value())
}

/// `temporalPlainDatePrototypeFuncToPlainYearMonth`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.toplainyearmonth
fn to_plain_year_month_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date = this_plain_date(call.this_value(), "toPlainYearMonth")?;
    let calendar_id = plain_date.calendar_id();
    // Passos 3 a 5: `ISODateToFields(calendar, isoDate, ~year-month~)` e `CalendarYearMonthFromFields(..., ~constrain~)`.
    let fields = iso_date_to_fields(calendar_id, plain_date.plain_date(), ResolveType::YearMonth)?;
    let resolved = year_month_from_fields(calendar_id, &fields, TemporalOverflow::Constrain)?;
    // Passo 6: `CreateTemporalYearMonth`.
    Ok(create_temporal_year_month(global_object, resolved.iso_date, calendar_id, None)?.as_value())
}

/// `temporalPlainDatePrototypeFuncWithCalendar`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.withcalendar
fn with_calendar_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca (`thisValue.toThis(strict)` é a identidade para objeto e para primitivo).
    let plain_date = this_plain_date(call.this_value(), "withCalendar")?;

    // Passo 3: `calendar = ? ToTemporalCalendarIdentifier(calendarLike)`.
    let new_calendar_id = to_temporal_calendar_identifier(global_object, call.argument(0))?;

    // Passo 4: `CreateTemporalDate(this.[[ISODate]], calendar)`; o C++ cria direto, sem repetir `ISODateWithinLimits`
    // (a data já está dentro).
    Ok(TemporalPlainDate::create(global_object.vm(), &global_object.plain_date_structure(), plain_date.plain_date(), new_calendar_id).as_value())
}

/// `addDurationToPlainDate(globalObject, scope, plainDate, duration, optionsArg)` (`AddDurationToDate`):
/// https://tc39.es/proposal-temporal/#sec-temporal-adddurationtodate
/// Quem chama faz os passos 1 a 3 (calendário, `ToTemporalDuration` e a negação de `subtract`).
fn add_duration_to_plain_date(
    global_object: &JSGlobalObject,
    plain_date: &TemporalPlainDate,
    duration: Duration,
    options_arg: JSValue,
) -> HostResult {
    // Passo 4: `dateDuration = ToDateDurationRecordWithoutTime`, que dobra 24 horas em um dia.
    let date_duration = to_date_duration_record_without_time(&duration)?;

    // Passos 5 e 6: `GetOptionsObject` e `GetTemporalOverflowOption`.
    let overflow = to_temporal_overflow_value(global_object, options_arg)?;

    // Passo 7: `result = ? CalendarDateAdd(calendar, isoDate, dateDuration, overflow)`.
    let result = calendar_date_add(plain_date.calendar_id(), plain_date.plain_date(), &date_duration, overflow)?;

    // Passo 8: `CreateTemporalDate(result, calendar)`.
    Ok(create_temporal_date(global_object, result, plain_date.calendar_id(), None)?.as_value())
}

/// `temporalPlainDatePrototypeFuncAdd`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.add
fn add_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date = this_plain_date(call.this_value(), "add")?;
    // Passo 3: `AddDurationToDate(~add~, this, temporalDurationLike, options)`; o passo 2 interno é `ToTemporalDuration`.
    let duration = TemporalDuration::to_temporal_duration_record(global_object, call.argument(0))?;
    add_duration_to_plain_date(global_object, &plain_date, duration, call.argument(1))
}

/// `temporalPlainDatePrototypeFuncSubtract`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.subtract
fn subtract_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date = this_plain_date(call.this_value(), "subtract")?;
    // Passo 3: `AddDurationToDate(~subtract~, ...)`; `-duration` é `CreateNegatedTemporalDuration`.
    let duration = TemporalDuration::to_temporal_duration_record(global_object, call.argument(0))?;
    add_duration_to_plain_date(global_object, &plain_date, -duration, call.argument(1))
}

/// `temporalPlainDatePrototypeFuncWith`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.with
fn with_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: `this` e marca.
    let plain_date = this_plain_date(call.this_value(), "with")?;

    // Passo 3: `IsPartialTemporalObject(temporalDateLike)` falso é `TypeError`.
    let temporal_date_like = call.argument(0);
    if !is_partial_temporal_object(global_object, temporal_date_like)? {
        return Err(Thrown::type_error("First argument to Temporal.PlainDate.prototype.with must be a partial Temporal object"));
    }

    // Passo 4: `calendar = plainDate.[[Calendar]]`, guardado no receptor.
    let calendar_id = plain_date.calendar_id();

    // Passo 6: `partialDate = ? PrepareCalendarFields(calendar, temporalDateLike, «year,month,monthCode,day», «», ~partial~)`.
    // O calendário vem do receptor: o passo 3 já recusou uma propriedade `calendar`.
    let partial_fields = read_calendar_fields_from_object(global_object, temporal_date_like, calendar_id, FieldSetType::Date, None)?;
    // `~partial~` lança `TypeError` se nenhum dos campos pedidos veio com valor.
    if partial_fields.day.is_none()
        && partial_fields.era.is_none()
        && partial_fields.era_year.is_none()
        && partial_fields.month.is_none()
        && partial_fields.month_code.is_none()
        && partial_fields.year.is_none()
    {
        return Err(Thrown::type_error("Object must contain at least one Temporal date property"));
    }

    // Passos 8 e 9: `GetOptionsObject(options)` e `GetTemporalOverflowOption(resolvedOptions)`.
    let overflow = to_temporal_overflow_value(global_object, call.argument(1))?;

    // Passos 5, 7 e 10: `ISODateToFields`, `CalendarMergeFields` e `CalendarDateFromFields`, em `plainDateWith`.
    let resolved = plain_date_with(calendar_id, plain_date.plain_date(), &partial_fields, overflow)?;

    // Passo 11: `CreateTemporalDate(isoDate, calendar)`.
    Ok(create_temporal_date(global_object, resolved.iso_date, calendar_id, None)?.as_value())
}

/// `temporalPlainDatePrototypeFuncUntil`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.until
fn until_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date = this_plain_date(call.this_value(), "until")?;
    // Passo 3: `DifferenceTemporalPlainDate(~until~, this, other, options)`; o passo 1 interno é `ToTemporalDate(other)`.
    let other = TemporalPlainDate::from(global_object, call.argument(0), JSValue::Undefined)?;
    let result = plain_date.until(global_object, &other, call.argument(1))?;
    Ok(create_temporal_duration(global_object, result, None)?.as_value())
}

/// `temporalPlainDatePrototypeFuncSince`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.since
fn since_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date = this_plain_date(call.this_value(), "since")?;
    // Passo 3: `DifferenceTemporalPlainDate(~since~, this, other, options)`.
    let other = TemporalPlainDate::from(global_object, call.argument(0), JSValue::Undefined)?;
    let result = plain_date.since(global_object, &other, call.argument(1))?;
    Ok(create_temporal_duration(global_object, result, None)?.as_value())
}

/// `temporalPlainDatePrototypeFuncEquals`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.equals
fn equals_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date = this_plain_date(call.this_value(), "equals")?;
    // Passo 3: `other = ? ToTemporalDate(other)`.
    let other = TemporalPlainDate::from(global_object, call.argument(0), JSValue::Undefined)?;
    // Passo 4: `CompareISODate` diferente de 0 é falso (o `!=` do `PlainDate` é esse teste).
    if plain_date.plain_date() != other.plain_date() {
        return Ok(js_boolean(false));
    }
    // Passos 5 e 6: `CalendarEquals`.
    Ok(js_boolean(plain_date.calendar_id() == other.calendar_id()))
}

/// `temporalPlainDatePrototypeFuncToPlainDateTime`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.toplaindatetime
fn to_plain_date_time_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date = this_plain_date(call.this_value(), "toPlainDateTime")?;

    // Passos 3 e 4: `ToTemporalTime(temporalTime)`; `undefined` é a meia-noite.
    let item_value = call.argument(0);
    let plain_time = if item_value.is_undefined() {
        Default::default()
    } else {
        TemporalPlainTime::from(global_object, item_value, JSValue::Undefined)?.plain_time()
    };

    // Passos 5 e 6: `CombineISODateAndTimeRecord` e `CreateTemporalDateTime`.
    Ok(create_temporal_date_time(global_object, plain_date.plain_date(), plain_time, plain_date.calendar_id(), None)?.as_value())
}

/// `temporalPlainDatePrototypeFuncToZonedDateTime`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.tozoneddatetime
fn to_zoned_date_time_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date = this_plain_date(call.this_value(), "toZonedDateTime")?;

    // Passos 3 a 5: `item` objeto lê `timeZone` e `plainTime`; senão `item` é o fuso.
    let item = call.argument(0);
    let (time_zone, temporal_time) = if item.is_object() && TemporalZonedDateTime::from_value(&item).is_none() {
        let time_zone_like = get_property(global_object, item, "timeZone")?;
        if time_zone_like.is_undefined() {
            (to_temporal_time_zone_identifier(global_object, item)?, JSValue::Undefined)
        } else {
            let time_zone = to_temporal_time_zone_identifier(global_object, time_zone_like)?;
            (time_zone, get_property(global_object, item, "plainTime")?)
        }
    } else {
        (to_temporal_time_zone_identifier(global_object, item)?, JSValue::Undefined)
    };

    // Passos 6 a 9: sem hora, `GetStartOfDay`; com hora, `GetEpochNanosecondsFor(..., ~compatible~)`.
    let exact_time = if temporal_time.is_undefined() {
        get_start_of_day(&time_zone, plain_date.plain_date())?
    } else {
        let plain_time = TemporalPlainTime::from(global_object, temporal_time, JSValue::Undefined)?;
        get_epoch_nanoseconds_for(&time_zone, plain_date.plain_date(), plain_time.plain_time(), TemporalDisambiguation::Compatible)?
    };

    // Passo 10: `CreateTemporalZonedDateTime(epochNs, timeZone, calendar)`.
    Ok(create_temporal_zoned_date_time(global_object, exact_time, time_zone, plain_date.calendar_id(), None)?.as_value())
}

/// `temporalPlainDatePrototypeFuncToString`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.tostring
fn to_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    // Passos 1 e 2: marca.
    let plain_date = this_plain_date(call.this_value(), "toString")?;

    // Passo 3: `resolvedOptions = ? GetOptionsObject(options)`.
    let options = get_options_object(call.argument(0))?;
    let Some(options) = options else {
        return Ok(str_value(vm, &plain_date.to_string()));
    };

    // Passo 4: `showCalendar = ? GetTemporalShowCalendarNameOption(resolvedOptions)`.
    let show_calendar = temporal_show_calendar_name(global_object, Some(options))?;

    // Passo 5: `TemporalDateToString(plainDate, showCalendar)`: `never` sem anotação, `always` com `[u-ca=<id>]`,
    // `critical` com `[!u-ca=<id>]` e `auto` só se o calendário não é ISO.
    let base = temporal_date_to_string(plain_date.plain_date());
    let calendar_id = calendar_id_to_string(plain_date.calendar_id());
    let result = match show_calendar {
        CalendarNameOption::Never => base,
        CalendarNameOption::Always => format!("{base}[u-ca={calendar_id}]"),
        CalendarNameOption::Critical => format!("{base}[!u-ca={calendar_id}]"),
        CalendarNameOption::Auto if !calendar_is_iso(plain_date.calendar_id()) => format!("{base}[u-ca={calendar_id}]"),
        CalendarNameOption::Auto => base,
    };
    Ok(str_value(vm, &result))
}

/// `temporalPlainDatePrototypeFuncToJSON`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.tojson
fn to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Passos 1 e 2: marca.
    let plain_date = this_plain_date(call.this_value(), "toJSON")?;
    // Passo 3: `TemporalDateToString(this, ~auto~)`.
    Ok(str_value(global_object.vm(), &plain_date.to_string()))
}

/// `temporalPlainDatePrototypeFuncToLocaleString`: https://tc39.es/proposal-temporal/#sup-temporal.plaindate.prototype.tolocalestring
/// Passo 3 (ECMA-402): `CreateDateTimeFormat(%Intl.DateTimeFormat%, locales, options, ~date~, ~date~)` e o
/// `FormatDateTime` do `PlainDate`.
fn to_locale_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    this_plain_date(call.this_value(), "toLocaleString")?;
    let text = intl_to_locale_string(global_object, call.argument(0), call.argument(1), Required::Date, Defaults::Date, call.this_value())?;
    Ok(str_value(global_object.vm(), &text))
}

/// `temporalPlainDatePrototypeFuncValueOf`: https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.valueof
fn value_of_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    // Passo 1: `PlainDate` não tem valor primitivo; use `Temporal.PlainDate.compare`.
    Err(Thrown::type_error("Temporal.PlainDate.prototype.valueOf must not be called. To compare PlainDate values, use Temporal.PlainDate.compare"))
}

host_function!(temporal_plain_date_prototype_func_to_plain_month_day, to_plain_month_day_body);
host_function!(temporal_plain_date_prototype_func_to_plain_year_month, to_plain_year_month_body);
host_function!(temporal_plain_date_prototype_func_with_calendar, with_calendar_body);
host_function!(temporal_plain_date_prototype_func_add, add_body);
host_function!(temporal_plain_date_prototype_func_subtract, subtract_body);
host_function!(temporal_plain_date_prototype_func_with, with_body);
host_function!(temporal_plain_date_prototype_func_until, until_body);
host_function!(temporal_plain_date_prototype_func_since, since_body);
host_function!(temporal_plain_date_prototype_func_equals, equals_body);
host_function!(temporal_plain_date_prototype_func_to_plain_date_time, to_plain_date_time_body);
host_function!(temporal_plain_date_prototype_func_to_zoned_date_time, to_zoned_date_time_body);
host_function!(temporal_plain_date_prototype_func_to_string, to_string_body);
host_function!(temporal_plain_date_prototype_func_to_json, to_json_body);
host_function!(temporal_plain_date_prototype_func_to_locale_string, to_locale_string_body);
host_function!(temporal_plain_date_prototype_func_value_of, value_of_body);

// `JSC_DEFINE_CUSTOM_GETTER(temporalPlainDatePrototypeGetterX, ...)` é o `temporal_getter!` de `temporal_object.rs`.

// https://tc39.es/proposal-temporal/#sec-get-temporal.plaindate.prototype.calendarid
crate::temporal_getter!(temporal_plain_date_prototype_getter_calendar_id, calendar_id_body, this_plain_date, "calendarId", |global, date| str_value(
    global.vm(),
    calendar_id_to_string(date.calendar_id())
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plaindate.prototype.year
crate::temporal_getter!(temporal_plain_date_prototype_getter_year, year_body, this_plain_date, "year", |_global, date| js_number(calendar_fields(date.calendar_id(), &date.plain_date()).year));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plaindate.prototype.month
crate::temporal_getter!(temporal_plain_date_prototype_getter_month, month_body, this_plain_date, "month", |_global, date| js_number(calendar_fields(date.calendar_id(), &date.plain_date()).month));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plaindate.prototype.monthcode
crate::temporal_getter!(temporal_plain_date_prototype_getter_month_code, month_code_body, this_plain_date, "monthCode", |global, date| str_value(
    global.vm(),
    &calendar_fields(date.calendar_id(), &date.plain_date()).month_code
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plaindate.prototype.day
crate::temporal_getter!(temporal_plain_date_prototype_getter_day, day_body, this_plain_date, "day", |_global, date| js_number(calendar_fields(date.calendar_id(), &date.plain_date()).day));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plaindate.prototype.dayofweek
crate::temporal_getter!(temporal_plain_date_prototype_getter_day_of_week, day_of_week_body, this_plain_date, "dayOfWeek", |_global, date| js_number(day_of_week(
    date.plain_date()
)));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plaindate.prototype.dayofyear
crate::temporal_getter!(temporal_plain_date_prototype_getter_day_of_year, day_of_year_body, this_plain_date, "dayOfYear", |_global, date| js_number(calendar_fields(date.calendar_id(), &date.plain_date()).day_of_year));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plaindate.prototype.weekofyear
crate::temporal_getter!(temporal_plain_date_prototype_getter_week_of_year, week_of_year_body, this_plain_date, "weekOfYear", |_global, date| {
    // `calendarWeekOfYear`: fora do ISO, `undefined`.
    if calendar_is_iso(date.calendar_id()) { js_number(week_of_year(date.plain_date())) } else { js_undefined() }
});
// https://tc39.es/proposal-temporal/#sec-get-temporal.plaindate.prototype.yearofweek
crate::temporal_getter!(temporal_plain_date_prototype_getter_year_of_week, year_of_week_body, this_plain_date, "yearOfWeek", |_global, date| {
    if calendar_is_iso(date.calendar_id()) { js_number(year_of_week(date.plain_date())) } else { js_undefined() }
});
// https://tc39.es/proposal-temporal/#sec-get-temporal.plaindate.prototype.daysinweek
crate::temporal_getter!(temporal_plain_date_prototype_getter_days_in_week, days_in_week_body, this_plain_date, "daysInWeek", |_global, _date| js_number(DAYS_PER_WEEK));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plaindate.prototype.daysinmonth
crate::temporal_getter!(temporal_plain_date_prototype_getter_days_in_month, days_in_month_body, this_plain_date, "daysInMonth", |_global, date| js_number(
    calendar_fields(date.calendar_id(), &date.plain_date()).days_in_month
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plaindate.prototype.daysinyear
crate::temporal_getter!(temporal_plain_date_prototype_getter_days_in_year, days_in_year_body, this_plain_date, "daysInYear", |_global, date| js_number(
    calendar_fields(date.calendar_id(), &date.plain_date()).days_in_year
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plaindate.prototype.monthsinyear
crate::temporal_getter!(temporal_plain_date_prototype_getter_months_in_year, months_in_year_body, this_plain_date, "monthsInYear", |_global, date| js_number(calendar_fields(date.calendar_id(), &date.plain_date()).months_in_year));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plaindate.prototype.inleapyear
crate::temporal_getter!(temporal_plain_date_prototype_getter_in_leap_year, in_leap_year_body, this_plain_date, "inLeapYear", |_global, date| js_boolean(
    calendar_fields(date.calendar_id(), &date.plain_date()).in_leap_year
));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plaindate.prototype.era
// `CalendarISOToDate(calendar, isoDate).[[Era]]`: `undefined` sem era (`calendarHasEras` falso).
crate::temporal_getter!(temporal_plain_date_prototype_getter_era, era_body, this_plain_date, "era", |global, date| calendar_fields(date.calendar_id(), &date.plain_date()).era_value(global.vm()));
// https://tc39.es/proposal-temporal/#sec-get-temporal.plaindate.prototype.erayear
crate::temporal_getter!(temporal_plain_date_prototype_getter_era_year, era_year_body, this_plain_date, "eraYear", |_global, date| calendar_fields(date.calendar_id(), &date.plain_date()).era_year_value());

/// `plain datePrototypeTableValues`, na ordem do `@begin`: os métodos (`DontEnum|Function`) e os acessores
/// (`DontEnum|ReadOnly|CustomAccessor`, sem setter).
static PROTOTYPE_TABLE_VALUES: [HashTableValue; 31] = [
    native_entry("toPlainMonthDay", temporal_plain_date_prototype_func_to_plain_month_day, 0),
    native_entry("toPlainYearMonth", temporal_plain_date_prototype_func_to_plain_year_month, 0),
    native_entry("withCalendar", temporal_plain_date_prototype_func_with_calendar, 1),
    native_entry("add", temporal_plain_date_prototype_func_add, 1),
    native_entry("subtract", temporal_plain_date_prototype_func_subtract, 1),
    native_entry("with", temporal_plain_date_prototype_func_with, 1),
    native_entry("until", temporal_plain_date_prototype_func_until, 1),
    native_entry("since", temporal_plain_date_prototype_func_since, 1),
    native_entry("equals", temporal_plain_date_prototype_func_equals, 1),
    native_entry("toPlainDateTime", temporal_plain_date_prototype_func_to_plain_date_time, 0),
    native_entry("toZonedDateTime", temporal_plain_date_prototype_func_to_zoned_date_time, 1),
    native_entry("toString", temporal_plain_date_prototype_func_to_string, 0),
    native_entry("toJSON", temporal_plain_date_prototype_func_to_json, 0),
    native_entry("toLocaleString", temporal_plain_date_prototype_func_to_locale_string, 0),
    native_entry("valueOf", temporal_plain_date_prototype_func_value_of, 0),
    custom_getter_entry("calendarId", temporal_plain_date_prototype_getter_calendar_id),
    custom_getter_entry("year", temporal_plain_date_prototype_getter_year),
    custom_getter_entry("month", temporal_plain_date_prototype_getter_month),
    custom_getter_entry("monthCode", temporal_plain_date_prototype_getter_month_code),
    custom_getter_entry("day", temporal_plain_date_prototype_getter_day),
    custom_getter_entry("dayOfWeek", temporal_plain_date_prototype_getter_day_of_week),
    custom_getter_entry("dayOfYear", temporal_plain_date_prototype_getter_day_of_year),
    custom_getter_entry("weekOfYear", temporal_plain_date_prototype_getter_week_of_year),
    custom_getter_entry("yearOfWeek", temporal_plain_date_prototype_getter_year_of_week),
    custom_getter_entry("daysInWeek", temporal_plain_date_prototype_getter_days_in_week),
    custom_getter_entry("daysInMonth", temporal_plain_date_prototype_getter_days_in_month),
    custom_getter_entry("daysInYear", temporal_plain_date_prototype_getter_days_in_year),
    custom_getter_entry("monthsInYear", temporal_plain_date_prototype_getter_months_in_year),
    custom_getter_entry("inLeapYear", temporal_plain_date_prototype_getter_in_leap_year),
    custom_getter_entry("era", temporal_plain_date_prototype_getter_era),
    custom_getter_entry("eraYear", temporal_plain_date_prototype_getter_era_year),
];

/// A tabela estática do protótipo.
static PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &PROTOTYPE_TABLE_VALUES };

/// `class TemporalPlainDatePrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct TemporalPlainDatePrototype;

impl TemporalPlainDatePrototype {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`; sem `HasStaticPropertyTable` porque os
    /// membros são postos direto em `finishCreation` (a tabela estática do C++ é essa mesma lista).
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | crate::runtime::js_type_info::HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, TemporalPlainDatePrototype::STRUCTURE_FLAGS),
            &TEMPORAL_PLAIN_DATE_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, structure)`: `TemporalPlainDatePrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        TemporalPlainDatePrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm)` com `plainDatePrototypeTable`: os métodos (`DontEnum|Function`, com o comprimento da
    /// tabela), os acessores (`DontEnum|ReadOnly|CustomAccessor`) e `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
    fn finish_creation(prototype: &JSObject, vm: &VM, _global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);

        put_to_string_tag(vm, prototype, TEMPORAL_PLAIN_DATE_PROTOTYPE_S_INFO.class_name);
    }
}
