//! Porte do ramo ISO de `runtime/temporal/core/CalendarFields.{h,cpp}` que `Temporal.PlainDate` usa:
//! `CalendarFieldsIn`, `TimeFieldsIn`, `ResolvedCalendarDate`, `ResolveType`, `dateFromFields`,
//! `isoDateToFields`, `calendarMergeFields` e `plainDateWith`, mais os de `PlainYearMonth` e `PlainMonthDay`:
//! `yearMonthFromFields`, `monthDayFromFields`, `plainYearMonthWith`, `plainMonthDayWith`, `differenceYearMonth`,
//! e `plainYearMonthAdd`.
//!
//! Fora desta fatia: `plainYearMonthFromISODate`, `plainMonthDayFromISODate` e `plainYearMonthToPlainDate` (só têm
//! chamador com calendário não ISO: o ramo ISO de `toPlainYearMonth`, `toPlainMonthDay`, `toPlainDate` e `from` não
//! passa por elas)
//! e todo o ramo de calendário não ISO (`nonISOResolveFields`, `nonISOMonthDayToISOReferenceDate`,
//! `checkLunisolarMonthConsistency`, `nonISOFieldKeysToIgnore`), que depende do `CalendarICUBridge` (Intl).
//!
//! O calendário não ISO passa por `temporal_calendar_icu.rs` (o `CalendarICUBridge` do porte, sobre o
//! `icu_calendar`).

use crate::runtime::iso8601::{is_date_time_within_limits, is_year_month_within_limits, Duration, PlainDate};
use crate::runtime::temporal_calendar::{calendar_is_iso, CalendarID, ISO8601_CALENDAR_ID};
use crate::runtime::temporal_calendar_icu::{
    calendar_date_add, calendar_date_until, calendar_iso_date_to_fields, date_from_calendar_fields, month_day_reference_date,
};
use crate::runtime::temporal_core_iso_date::regulate_iso_date;
use crate::runtime::temporal_core_types::{range_error, TemporalError, TemporalErrorKind, TemporalResult};
use crate::runtime::temporal_object::{ParsedMonthCode, TemporalOverflow, TemporalUnit};
use crate::wtf::date_math::date_to_days_from_1970;

/// `isoMonthDayReferenceLeapYear`: o ano de referência (bissexto, para o 29 de fevereiro existir) de um
/// `PlainMonthDay` ISO.
pub const ISO_MONTH_DAY_REFERENCE_LEAP_YEAR: i32 = 1972;

/// `ROUGH_YEAR_RANGE` de `fields.rs` do temporal_rs: `-300000..300000`.
const SAFE_YEAR_MIN: i32 = -300000;
const SAFE_YEAR_MAX: i32 = 300000;

/// `struct CalendarFieldsIn`: os campos de calendário de entrada (`temporal_rs CalendarFields`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CalendarFieldsIn {
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub month_code: Option<ParsedMonthCode>,
    pub day: Option<u8>,
    pub era: Option<String>,
    pub era_year: Option<i32>,
}

/// `struct TimeFieldsIn`: os campos de hora lidos de um objeto (`double`, ainda sem regular).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TimeFieldsIn {
    pub hour: Option<f64>,
    pub minute: Option<f64>,
    pub second: Option<f64>,
    pub millisecond: Option<f64>,
    pub microsecond: Option<f64>,
    pub nanosecond: Option<f64>,
}

/// `struct ResolvedCalendarDate`: a data ISO e o calendário.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedCalendarDate {
    pub iso_date: PlainDate,
    pub calendar_id: CalendarID,
}

/// `enum class ResolveType` (e o `ISOResolveType` idêntico do `.cpp`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolveType {
    Date,
    YearMonth,
    MonthDay,
}

/// `TemporalCore::typeError(msg)`.
fn type_error(message: &str) -> TemporalError {
    TemporalError { kind: TemporalErrorKind::TypeError, message: message.to_string() }
}

/// `checkYearRange(fields)` (`check_year_in_safe_arithmetical_range` do temporal_rs).
fn check_year_range(fields: &CalendarFieldsIn) -> TemporalResult<()> {
    if fields.year.is_some_and(|year| !(SAFE_YEAR_MIN..=SAFE_YEAR_MAX).contains(&year)) {
        return Err(range_error("Date is not within representable range"));
    }
    if fields.era_year.is_some_and(|era_year| !(SAFE_YEAR_MIN..=SAFE_YEAR_MAX).contains(&era_year)) {
        return Err(range_error("eraYear is not within representable range"));
    }
    Ok(())
}

/// `calendarResolveFields(calendarId, fields, type)` (`CalendarResolveFields`), o ramo ISO:
/// https://tc39.es/proposal-temporal/#sec-temporal-calendarresolvefields
fn calendar_resolve_fields(calendar_id: CalendarID, fields: &mut CalendarFieldsIn, resolve_type: ResolveType) -> TemporalResult<()> {
    assert!(calendar_is_iso(calendar_id), "NonISOResolveFields: calendário não ISO depende do CalendarICUBridge");

    // Passos 1.a a 1.d: `needsYear` e `needsDay`.
    let needs_year = resolve_type == ResolveType::Date || resolve_type == ResolveType::YearMonth;
    let needs_day = resolve_type == ResolveType::Date || resolve_type == ResolveType::MonthDay;
    // Passo 1.e: `needsYear` e `year` ausente.
    if needs_year && fields.year.is_none() {
        return Err(type_error("year property must be present"));
    }
    // Passo 1.f: `needsDay` e `day` ausente.
    if needs_day && fields.day.is_none() {
        return Err(type_error("day property must be present"));
    }
    // Passo 1.g: `month` e `monthCode` ausentes.
    if fields.month.is_none() && fields.month_code.is_none() {
        return Err(type_error("month or monthCode property must be present"));
    }
    // Passo 1.h: validação do `monthCode` e consistência com `month`.
    if let Some(month_code) = fields.month_code {
        if month_code.is_leap_month {
            return Err(range_error("iso8601 calendar does not have leap months"));
        }
        if !(1..=12).contains(&month_code.month_number) {
            return Err(range_error("month must be 1-12 for iso8601 calendar"));
        }
        let code_month = u32::from(month_code.month_number);
        if fields.month.is_some_and(|month| month != code_month) {
            return Err(range_error("month does not match monthCode"));
        }
        fields.month = Some(code_month);
    }
    Ok(())
}

/// `calendarDateToISO(calendarId, fields, overflow, type)` (`CalendarDateToISO`), o ramo ISO:
/// https://tc39.es/proposal-temporal/#sec-temporal-calendardatetoiso
fn calendar_date_to_iso(
    calendar_id: CalendarID,
    fields: &CalendarFieldsIn,
    overflow: TemporalOverflow,
    resolve_type: ResolveType,
) -> TemporalResult<PlainDate> {
    assert!(calendar_is_iso(calendar_id), "NonISOCalendarDateToISO: calendário não ISO depende do CalendarICUBridge");

    // Passo 1.a: `year`, `month` e `day` não são `~unset~` (o `day` só não conta para ano-mês).
    let year = fields.year.expect("CalendarResolveFields garante year");
    let month = fields.month.expect("CalendarResolveFields garante month");
    let day = if resolve_type == ResolveType::YearMonth { 1 } else { fields.day.expect("CalendarResolveFields garante day") };

    // Passo 1.b: `RegulateISODate(year, month, day, overflow)`, o `month` preso em `int32_t`.
    regulate_iso_date(year, i32::try_from(month).unwrap_or(i32::MAX), i64::from(day), overflow)
}

/// `dateFromFields(calendarId, fields, overflow)` (`CalendarDateFromFields`):
/// https://tc39.es/proposal-temporal/#sec-temporal-calendardatefromfields
/// Implementa os passos 1 a 4; o `PrepareCalendarFields` é de quem chama.
pub fn date_from_fields(calendar_id: CalendarID, fields: &CalendarFieldsIn, overflow: TemporalOverflow) -> TemporalResult<ResolvedCalendarDate> {
    let mut resolved = fields.clone();
    check_year_range(&resolved)?;

    // Passos 1 e 2: `CalendarResolveFields` e `CalendarDateToISO`; o calendário não ISO é do icu4x.
    let result = if calendar_is_iso(calendar_id) {
        calendar_resolve_fields(calendar_id, &mut resolved, ResolveType::Date)?;
        let iso_date = calendar_date_to_iso(calendar_id, &resolved, overflow, ResolveType::Date)?;
        ResolvedCalendarDate { iso_date, calendar_id: ISO8601_CALENDAR_ID }
    } else {
        date_from_calendar_fields(calendar_id, &resolved, overflow)?
    };
    let iso_date = result.iso_date;

    // Passo 3: `ISODateWithinLimits(result)` falso é `RangeError`.
    if !is_date_time_within_limits(iso_date.year(), iso_date.month(), iso_date.day(), 12, 0, 0, 0, 0, 0) {
        return Err(range_error("Date is not within representable range"));
    }

    // Passo 4.
    Ok(result)
}

/// `isoDateToFields(calendarId, isoDate, type)`: https://tc39.es/proposal-temporal/#sec-temporal-isodatetofields
/// O ramo ISO é a identidade: o código do mês é o mês ISO, nunca bissexto.
pub fn iso_date_to_fields(calendar_id: CalendarID, iso_date: PlainDate, resolve_type: ResolveType) -> TemporalResult<CalendarFieldsIn> {
    if !calendar_is_iso(calendar_id) {
        return Ok(calendar_iso_date_to_fields(calendar_id, iso_date, resolve_type));
    }

    // Passo 1: todos os campos `~unset~`.
    let mut fields = CalendarFieldsIn::default();
    // Passo 3.
    fields.month_code = Some(ParsedMonthCode { month_number: iso_date.month(), is_leap_month: false });
    // Passo 4.
    if resolve_type == ResolveType::MonthDay || resolve_type == ResolveType::Date {
        fields.day = Some(iso_date.day());
    }
    // Passo 5.
    if resolve_type == ResolveType::YearMonth || resolve_type == ResolveType::Date {
        fields.year = Some(iso_date.year());
    }
    // Passo 6.
    Ok(fields)
}

/// As seis chaves de data de `CalendarFieldsIn` (a coluna "Enumeration Key" da tabela Calendar Fields Record).
/// O `OptionSet` do C++ é aqui uma tupla de seis booleanos na ordem `era`, `eraYear`, `year`, `month`,
/// `monthCode`, `day`.
type CalendarFieldKeys = [bool; 6];

const KEY_ERA: usize = 0;
const KEY_ERA_YEAR: usize = 1;
const KEY_YEAR: usize = 2;
const KEY_MONTH: usize = 3;
const KEY_MONTH_CODE: usize = 4;
const KEY_DAY: usize = 5;

/// `calendarFieldKeysPresent(fields)`: https://tc39.es/proposal-temporal/#sec-temporal-calendarfieldkeyspresent
fn calendar_field_keys_present(fields: &CalendarFieldsIn) -> CalendarFieldKeys {
    [
        fields.era.is_some(),
        fields.era_year.is_some(),
        fields.year.is_some(),
        fields.month.is_some(),
        fields.month_code.is_some(),
        fields.day.is_some(),
    ]
}

/// `calendarFieldKeysToIgnore(calendarId, keys)`: `month`/`monthCode` (e, fora do ISO, `era`/`eraYear`/`year`) contam em par.
/// https://tc39.es/proposal-temporal/#sec-temporal-calendarfieldkeystoignore
fn calendar_field_keys_to_ignore(calendar_id: CalendarID, keys: CalendarFieldKeys) -> CalendarFieldKeys {
    // Passo 1.a e 1.b.i: cópia de `keys`.
    let mut ignored_keys = keys;
    // Calendário não ISO: `era`, `eraYear` e `year` são três codificações do mesmo campo.
    if !calendar_is_iso(calendar_id) && (keys[KEY_ERA] || keys[KEY_ERA_YEAR] || keys[KEY_YEAR]) {
        ignored_keys[KEY_ERA] = true;
        ignored_keys[KEY_ERA_YEAR] = true;
        ignored_keys[KEY_YEAR] = true;
    }
    // Passo 1.b.ii e 1.b.iii: `month` e `monthCode` são duas codificações do mesmo campo.
    if keys[KEY_MONTH] {
        ignored_keys[KEY_MONTH_CODE] = true;
    }
    if keys[KEY_MONTH_CODE] {
        ignored_keys[KEY_MONTH] = true;
    }
    // Passo 1.d.
    ignored_keys
}

/// `calendarMergeFields(calendarId, fields, additionalFields)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-calendarmergefields
pub fn calendar_merge_fields(calendar_id: CalendarID, fields: &CalendarFieldsIn, additional_fields: &CalendarFieldsIn) -> CalendarFieldsIn {
    // Passo 1: `additionalKeys`. Passo 2: `overriddenKeys`.
    let additional_keys = calendar_field_keys_present(additional_fields);
    let overridden_keys = calendar_field_keys_to_ignore(calendar_id, additional_keys);

    // Passos 3 a 5: para cada chave, o campo de `fields` se não foi sobrescrito, e o adicional por cima.
    let merge_key = |key: usize| overridden_keys[key];
    let mut merged = CalendarFieldsIn::default();
    if fields.era.is_some() && !merge_key(KEY_ERA) {
        merged.era.clone_from(&fields.era);
    }
    if additional_fields.era.is_some() {
        merged.era.clone_from(&additional_fields.era);
    }
    if fields.era_year.is_some() && !merge_key(KEY_ERA_YEAR) {
        merged.era_year = fields.era_year;
    }
    if additional_fields.era_year.is_some() {
        merged.era_year = additional_fields.era_year;
    }
    if fields.year.is_some() && !merge_key(KEY_YEAR) {
        merged.year = fields.year;
    }
    if additional_fields.year.is_some() {
        merged.year = additional_fields.year;
    }
    if fields.month.is_some() && !merge_key(KEY_MONTH) {
        merged.month = fields.month;
    }
    if additional_fields.month.is_some() {
        merged.month = additional_fields.month;
    }
    if fields.month_code.is_some() && !merge_key(KEY_MONTH_CODE) {
        merged.month_code = fields.month_code;
    }
    if additional_fields.month_code.is_some() {
        merged.month_code = additional_fields.month_code;
    }
    if fields.day.is_some() && !merge_key(KEY_DAY) {
        merged.day = fields.day;
    }
    if additional_fields.day.is_some() {
        merged.day = additional_fields.day;
    }

    // Passo 6.
    merged
}

/// `plainDateWith(calendarId, currentISODate, partialFields, overflow)`:
/// https://tc39.es/proposal-temporal/#sec-temporal.plaindate.prototype.with
/// Implementa os passos 5 (`ISODateToFields`), 7 (`CalendarMergeFields`) e 10 (`CalendarDateFromFields`).
pub fn plain_date_with(
    calendar_id: CalendarID,
    current_iso_date: PlainDate,
    partial_fields: &CalendarFieldsIn,
    overflow: TemporalOverflow,
) -> TemporalResult<ResolvedCalendarDate> {
    let fields = iso_date_to_fields(calendar_id, current_iso_date, ResolveType::Date)?;
    date_from_fields(calendar_id, &calendar_merge_fields(calendar_id, &fields, partial_fields), overflow)
}

/// `yearMonthFromFields(calendarId, fields, overflow)` (`CalendarYearMonthFromFields`), o ramo ISO:
/// https://tc39.es/proposal-temporal/#sec-temporal-calendaryearmonthfromfields
/// A data guardada tem sempre o dia 1.
pub fn year_month_from_fields(calendar_id: CalendarID, fields: &CalendarFieldsIn, overflow: TemporalOverflow) -> TemporalResult<ResolvedCalendarDate> {
    // Passo 1: `fields.[[Day]]` vira 1.
    let mut resolved = fields.clone();
    resolved.day = Some(1);
    check_year_range(&resolved)?;

    // Passos 2 e 3: `CalendarResolveFields` e `CalendarDateToISO`; o calendário não ISO é do icu4x e guarda o
    // primeiro dia do mês do calendário (a data ISO equivalente).
    let iso_date = if calendar_is_iso(calendar_id) {
        calendar_resolve_fields(calendar_id, &mut resolved, ResolveType::YearMonth)?;
        calendar_date_to_iso(calendar_id, &resolved, overflow, ResolveType::YearMonth)?
    } else {
        date_from_calendar_fields(calendar_id, &resolved, overflow)?.iso_date
    };

    // Passo 4: `ISOYearMonthWithinLimits(result)` falso é `RangeError`.
    if !is_year_month_within_limits(iso_date.year(), i32::from(iso_date.month())) {
        return Err(range_error("YearMonth is not within representable range"));
    }

    // Passo 5.
    Ok(ResolvedCalendarDate { iso_date, calendar_id })
}

/// `monthDayToISOReferenceDate(calendarId, resolved, fields, overflow)` (`CalendarMonthDayToISOReferenceDate`),
/// o ramo ISO: https://tc39.es/proposal-temporal/#sec-temporal-calendarmonthdaytoisoreferencedate
fn month_day_to_iso_reference_date(calendar_id: CalendarID, resolved: &CalendarFieldsIn, overflow: TemporalOverflow) -> TemporalResult<PlainDate> {
    assert!(calendar_is_iso(calendar_id), "NonISOMonthDayToISOReferenceDate: calendário não ISO depende do CalendarICUBridge");

    // Passo 1.a.
    let month = resolved.month.expect("CalendarResolveFields garante month");
    let day = resolved.day.expect("CalendarResolveFields garante day");
    // Passos 1.b e 1.c: o ano só decide se o 29 de fevereiro existe; `RegulateISODate` não confere a faixa dele.
    let year = resolved.year.unwrap_or(ISO_MONTH_DAY_REFERENCE_LEAP_YEAR);
    // Passo 1.d.
    let regulated = regulate_iso_date(year, i32::try_from(month).unwrap_or(i32::MAX), i64::from(day), overflow)?;
    // Passo 1.e: o ano guardado é sempre o de referência, nunca o de quem chamou.
    Ok(PlainDate::new(i64::from(ISO_MONTH_DAY_REFERENCE_LEAP_YEAR), u32::from(regulated.month()), u32::from(regulated.day())))
}

/// `monthDayFromFields(calendarId, fields, overflow)` (`CalendarMonthDayFromFields`), o ramo ISO:
/// https://tc39.es/proposal-temporal/#sec-temporal-calendarmonthdayfromfields
pub fn month_day_from_fields(calendar_id: CalendarID, fields: &CalendarFieldsIn, overflow: TemporalOverflow) -> TemporalResult<ResolvedCalendarDate> {
    // Passos 1 e 2: `CalendarResolveFields` e `CalendarMonthDayToISOReferenceDate`.
    let iso_date = if calendar_is_iso(calendar_id) {
        let mut resolved = fields.clone();
        calendar_resolve_fields(calendar_id, &mut resolved, ResolveType::MonthDay)?;
        month_day_to_iso_reference_date(calendar_id, &resolved, overflow)?
    } else {
        month_day_reference_date(calendar_id, fields, overflow)?
    };

    // Passo 3: `ISODateWithinLimits(result)` é invariante (o ano de referência está sempre dentro).
    debug_assert!(is_date_time_within_limits(iso_date.year(), iso_date.month(), iso_date.day(), 12, 0, 0, 0, 0, 0));

    // Passo 4.
    Ok(ResolvedCalendarDate { iso_date, calendar_id })
}

/// `plainYearMonthFromISODate(calendarId, fullISODate)` (passos 12 e 14 de `ToTemporalYearMonth`, o ramo da
/// `String`): `~constrain~` seja qual for o `overflow`.
pub fn plain_year_month_from_iso_date(calendar_id: CalendarID, full_iso_date: PlainDate) -> TemporalResult<ResolvedCalendarDate> {
    let fields = iso_date_to_fields(calendar_id, full_iso_date, ResolveType::YearMonth)?;
    year_month_from_fields(calendar_id, &fields, TemporalOverflow::Constrain)
}

/// `plainMonthDayFromISODate(calendarId, fullISODate, overflow)` (passos 13 e 15 de `ToTemporalMonthDay`, o ramo
/// da `String`).
pub fn plain_month_day_from_iso_date(calendar_id: CalendarID, full_iso_date: PlainDate, overflow: TemporalOverflow) -> TemporalResult<ResolvedCalendarDate> {
    let fields = iso_date_to_fields(calendar_id, full_iso_date, ResolveType::MonthDay)?;
    month_day_from_fields(calendar_id, &fields, overflow)
}

/// `plainYearMonthWith(calendarId, currentISODate, partialFields, overflow)`:
/// https://tc39.es/proposal-temporal/#sec-temporal.plainyearmonth.prototype.with
/// Implementa os passos 5 (`ISODateToFields`), 7 (`CalendarMergeFields`) e 10 (`CalendarYearMonthFromFields`).
pub fn plain_year_month_with(
    calendar_id: CalendarID,
    current_iso_date: PlainDate,
    partial_fields: &CalendarFieldsIn,
    overflow: TemporalOverflow,
) -> TemporalResult<ResolvedCalendarDate> {
    let fields = iso_date_to_fields(calendar_id, current_iso_date, ResolveType::YearMonth)?;
    year_month_from_fields(calendar_id, &calendar_merge_fields(calendar_id, &fields, partial_fields), overflow)
}

/// `plainMonthDayWith(calendarId, currentISODate, partialFields, overflow)`:
/// https://tc39.es/proposal-temporal/#sec-temporal.plainmonthday.prototype.with
/// Implementa os passos 5 (`ISODateToFields`), 7 (`CalendarMergeFields`) e 10 (`CalendarMonthDayFromFields`).
pub fn plain_month_day_with(
    calendar_id: CalendarID,
    current_iso_date: PlainDate,
    partial_fields: &CalendarFieldsIn,
    overflow: TemporalOverflow,
) -> TemporalResult<ResolvedCalendarDate> {
    let fields = iso_date_to_fields(calendar_id, current_iso_date, ResolveType::MonthDay)?;
    month_day_from_fields(calendar_id, &calendar_merge_fields(calendar_id, &fields, partial_fields), overflow)
}

/// `differenceYearMonth(calendarId, thisISODate, otherISODate, largestUnit)` (passos 7 a 14 de
/// `DifferenceTemporalPlainYearMonth`), o ramo ISO:
/// https://tc39.es/proposal-temporal/#sec-temporal-differencetemporalplainyearmonth
pub fn difference_year_month(
    calendar_id: CalendarID,
    this_iso_date: PlainDate,
    other_iso_date: PlainDate,
    largest_unit: TemporalUnit,
) -> TemporalResult<Duration> {
    // Passos 7 e 8 (ISO): o dia 1 nas duas pontas e a conferência da faixa.
    if calendar_is_iso(calendar_id) {
        let this_date = PlainDate::new(i64::from(this_iso_date.year()), u32::from(this_iso_date.month()), 1);
        let other_date = PlainDate::new(i64::from(other_iso_date.year()), u32::from(other_iso_date.month()), 1);
        let out_of_range = |date: PlainDate| date_to_days_from_1970(date.year(), i32::from(date.month()) - 1, 1).abs() > 1e8;
        if out_of_range(this_date) || out_of_range(other_date) {
            return Err(range_error("date is outside the representable range for Temporal"));
        }
        // Passos 10 a 14: `CalendarDateUntil(thisDate, otherDate, largestUnit)`.
        return calendar_date_until(calendar_id, this_date, other_date, largest_unit);
    }

    // Não ISO, passos 7 a 9 de cada ponta: `ISODateToFields(~year-month~)`, `[[Day]]` igual a 1 e
    // `CalendarDateFromFields(~constrain~)` (o primeiro dia do mês do calendário).
    let first_of_month = |iso_date: PlainDate| -> TemporalResult<PlainDate> {
        let mut fields = iso_date_to_fields(calendar_id, iso_date, ResolveType::YearMonth)?;
        fields.day = Some(1);
        Ok(date_from_fields(calendar_id, &fields, TemporalOverflow::Constrain)?.iso_date)
    };
    // Passos 10 a 14: `CalendarDateUntil(thisDate, otherDate, largestUnit)`.
    calendar_date_until(calendar_id, first_of_month(this_iso_date)?, first_of_month(other_iso_date)?, largest_unit)
}

/// `plainYearMonthToPlainDate(calendarId, pymISODate, day)` (passos 5, 7 e 8 de `toPlainDate`):
/// https://tc39.es/proposal-temporal/#sec-temporal.plainyearmonth.prototype.toplaindate
pub fn plain_year_month_to_plain_date(calendar_id: CalendarID, pym_iso_date: PlainDate, day: u8) -> TemporalResult<ResolvedCalendarDate> {
    let fields = iso_date_to_fields(calendar_id, pym_iso_date, ResolveType::YearMonth)?;
    let input_fields = CalendarFieldsIn { day: Some(day), ..Default::default() };
    date_from_fields(calendar_id, &calendar_merge_fields(calendar_id, &fields, &input_fields), TemporalOverflow::Constrain)
}

/// `plainYearMonthAdd(calendarId, currentISODate, duration, overflow)` (passos 9 a 14 de
/// `AddDurationToYearMonth`): https://tc39.es/proposal-temporal/#sec-temporal-adddurationtoyearmonth
pub fn plain_year_month_add(
    calendar_id: CalendarID,
    current_iso_date: PlainDate,
    duration: &Duration,
    overflow: TemporalOverflow,
) -> TemporalResult<ResolvedCalendarDate> {
    // Passos 9 e 10: `ISODateToFields(calendar, isoDate, ~year-month~)` e `fields.[[Day]]` igual a 1.
    let mut fields = iso_date_to_fields(calendar_id, current_iso_date, ResolveType::YearMonth)?;
    fields.day = Some(1);

    // Passo 11: `date = ? CalendarDateFromFields(calendar, fields, ~constrain~)`.
    let date = date_from_fields(calendar_id, &fields, TemporalOverflow::Constrain)?;

    // Passo 12: `addedDate = ? CalendarDateAdd(calendar, date, durationToAdd, overflow)` (no ISO o despacho cai no
    // `isoDateAdd` puro, que trata os anos de borda).
    let added_date = calendar_date_add(calendar_id, date.iso_date, duration, overflow)?;

    // Passo 13: `ISODateToFields(calendar, addedDate, ~year-month~)`.
    let added_fields = iso_date_to_fields(calendar_id, added_date, ResolveType::YearMonth)?;

    // Passo 14: `CalendarYearMonthFromFields(calendar, addedDateFields, overflow)`.
    year_month_from_fields(calendar_id, &added_fields, overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(year: i32, month: u32, day: u8) -> CalendarFieldsIn {
        CalendarFieldsIn { year: Some(year), month: Some(month), day: Some(day), ..Default::default() }
    }

    #[test]
    fn resolves_iso_dates() {
        let resolved = date_from_fields(ISO8601_CALENDAR_ID, &fields(2020, 2, 30), TemporalOverflow::Constrain).unwrap();
        assert_eq!((resolved.iso_date.month(), resolved.iso_date.day()), (2, 29));
        assert!(date_from_fields(ISO8601_CALENDAR_ID, &fields(2020, 2, 30), TemporalOverflow::Reject).is_err());
        let missing_year = CalendarFieldsIn { month: Some(1), day: Some(1), ..Default::default() };
        let error = date_from_fields(ISO8601_CALENDAR_ID, &missing_year, TemporalOverflow::Constrain).unwrap_err();
        assert_eq!(error.kind, TemporalErrorKind::TypeError);
        assert!(date_from_fields(ISO8601_CALENDAR_ID, &fields(290000, 1, 1), TemporalOverflow::Constrain).is_err());
    }

    #[test]
    fn month_code_must_match_month() {
        let mut bag = fields(2020, 3, 1);
        bag.month_code = Some(ParsedMonthCode { month_number: 4, is_leap_month: false });
        assert!(date_from_fields(ISO8601_CALENDAR_ID, &bag, TemporalOverflow::Constrain).is_err());
        bag.month = None;
        let resolved = date_from_fields(ISO8601_CALENDAR_ID, &bag, TemporalOverflow::Constrain).unwrap();
        assert_eq!(resolved.iso_date.month(), 4);
    }

    #[test]
    fn with_merges_month_and_month_code() {
        let partial = CalendarFieldsIn { month: Some(5), ..Default::default() };
        let resolved = plain_date_with(ISO8601_CALENDAR_ID, PlainDate::new(2020, 1, 31), &partial, TemporalOverflow::Constrain).unwrap();
        assert_eq!((resolved.iso_date.year(), resolved.iso_date.month(), resolved.iso_date.day()), (2020, 5, 31));
        let partial = CalendarFieldsIn { day: Some(15), ..Default::default() };
        let resolved = plain_date_with(ISO8601_CALENDAR_ID, PlainDate::new(2020, 1, 31), &partial, TemporalOverflow::Constrain).unwrap();
        assert_eq!(resolved.iso_date.day(), 15);
    }

    #[test]
    fn year_month_from_fields_stores_day_one() {
        let bag = CalendarFieldsIn { year: Some(2020), month: Some(2), day: Some(30), ..Default::default() };
        let resolved = year_month_from_fields(ISO8601_CALENDAR_ID, &bag, TemporalOverflow::Reject).unwrap();
        assert_eq!((resolved.iso_date.year(), resolved.iso_date.month(), resolved.iso_date.day()), (2020, 2, 1));
        let missing_year = CalendarFieldsIn { month: Some(2), ..Default::default() };
        let error = year_month_from_fields(ISO8601_CALENDAR_ID, &missing_year, TemporalOverflow::Constrain).unwrap_err();
        assert_eq!(error.kind, TemporalErrorKind::TypeError);
        // Fora de `ISOYearMonthWithinLimits`: abril de -271821 é o primeiro mês aceito.
        let early = CalendarFieldsIn { year: Some(-271821), month: Some(3), ..Default::default() };
        assert!(year_month_from_fields(ISO8601_CALENDAR_ID, &early, TemporalOverflow::Constrain).is_err());
        let first = CalendarFieldsIn { year: Some(-271821), month: Some(4), ..Default::default() };
        assert!(year_month_from_fields(ISO8601_CALENDAR_ID, &first, TemporalOverflow::Constrain).is_ok());
        let late = CalendarFieldsIn { year: Some(275760), month: Some(10), ..Default::default() };
        assert!(year_month_from_fields(ISO8601_CALENDAR_ID, &late, TemporalOverflow::Constrain).is_err());
    }

    #[test]
    fn month_day_from_fields_uses_reference_year() {
        let bag = CalendarFieldsIn { month: Some(2), day: Some(30), ..Default::default() };
        let resolved = month_day_from_fields(ISO8601_CALENDAR_ID, &bag, TemporalOverflow::Constrain).unwrap();
        assert_eq!((resolved.iso_date.year(), resolved.iso_date.month(), resolved.iso_date.day()), (1972, 2, 29));
        assert!(month_day_from_fields(ISO8601_CALENDAR_ID, &bag, TemporalOverflow::Reject).is_err());
        // O ano de quem chama só decide se 29 de fevereiro existe, e nunca é guardado.
        let non_leap = CalendarFieldsIn { year: Some(2021), month: Some(2), day: Some(29), ..Default::default() };
        let resolved = month_day_from_fields(ISO8601_CALENDAR_ID, &non_leap, TemporalOverflow::Constrain).unwrap();
        assert_eq!((resolved.iso_date.year(), resolved.iso_date.month(), resolved.iso_date.day()), (1972, 2, 28));
        assert!(month_day_from_fields(ISO8601_CALENDAR_ID, &non_leap, TemporalOverflow::Reject).is_err());
        let no_day = CalendarFieldsIn { month: Some(2), ..Default::default() };
        assert_eq!(month_day_from_fields(ISO8601_CALENDAR_ID, &no_day, TemporalOverflow::Constrain).unwrap_err().kind, TemporalErrorKind::TypeError);
    }

    #[test]
    fn year_month_and_month_day_with() {
        let partial = CalendarFieldsIn { month_code: Some(ParsedMonthCode { month_number: 7, is_leap_month: false }), ..Default::default() };
        let resolved = plain_year_month_with(ISO8601_CALENDAR_ID, PlainDate::new(2020, 1, 1), &partial, TemporalOverflow::Constrain).unwrap();
        assert_eq!((resolved.iso_date.year(), resolved.iso_date.month(), resolved.iso_date.day()), (2020, 7, 1));
        let partial = CalendarFieldsIn { day: Some(31), ..Default::default() };
        let resolved = plain_month_day_with(ISO8601_CALENDAR_ID, PlainDate::new(1972, 2, 10), &partial, TemporalOverflow::Constrain).unwrap();
        assert_eq!((resolved.iso_date.month(), resolved.iso_date.day()), (2, 29));
    }

    #[test]
    fn year_month_difference_and_add() {
        let years_months =
            difference_year_month(ISO8601_CALENDAR_ID, PlainDate::new(2019, 11, 1), PlainDate::new(2021, 2, 1), TemporalUnit::Year).unwrap();
        assert_eq!((years_months.years(), years_months.months()), (1, 3));
        let months_only =
            difference_year_month(ISO8601_CALENDAR_ID, PlainDate::new(2019, 11, 1), PlainDate::new(2021, 2, 1), TemporalUnit::Month).unwrap();
        assert_eq!((months_only.years(), months_only.months()), (0, 15));

        let one_month = Duration::new(0, 1, 0, 0, 0, 0, 0, 0, 0, 0);
        let added = plain_year_month_add(ISO8601_CALENDAR_ID, PlainDate::new(2020, 12, 1), &one_month, TemporalOverflow::Constrain).unwrap();
        assert_eq!((added.iso_date.year(), added.iso_date.month(), added.iso_date.day()), (2021, 1, 1));
    }
}
