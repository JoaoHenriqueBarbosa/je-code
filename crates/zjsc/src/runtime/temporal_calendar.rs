//! Porte de `runtime/TemporalCalendar.{h,cpp}` (só o calendário ISO 8601) e das partes de
//! `TemporalObject.cpp`, `IntlObject.cpp` e `CalendarICUBridge.h` que o calendário usa: `CalendarID`, a lista
//! `intlAvailableCalendars`, `isBuiltinCalendar`, `calendarIsISO`, `calendarHasEras`,
//! `getTemporalCalendarIdentifierWithISODefault`, `toTemporalCalendarIdentifier`,
//! `temporalShowCalendarName`, `parseMonthCode` (a de `JSGlobalObject`) e `readCalendarFieldsFromObject`.
//!
//! DIVERGÊNCIAS:
//! - A aritmética e os campos dos 15 calendários não ISO (`gregory`, `hebrew`, ...) são de
//!   `temporal_calendar_icu.rs` (icu4x); as células carregam o `CalendarID` e nenhuma criação recusa calendário.
//! - `CalendarID` é o índice na lista ordenada por `codePointCompare`, como no C++ (`iso8601` é o 12).
//! - `getTemporalCalendarIdentifierWithISODefault` e `toTemporalCalendarIdentifier` conhecem as células
//!   `PlainDate` e `PlainDateTime` entre as classes com `[[Calendar]]`; cada porte novo (`PlainYearMonth`,
//!   `PlainMonthDay`, `ZonedDateTime`) acrescenta a sua.
//! - `readZonedDateTimeFieldsFromObject` espera `ZonedDateTime` e entra com ela. `interpretTemporalDateTimeFields`
//!   está aqui (`interpret_temporal_date_time_fields`), sem o `globalObject`: o `?` converte o `TemporalError`.
//! - `calendarDateAdd`, `isoDateAdd` e `calendarDateUntil` do `.cpp` só embrulham o núcleo com o lançamento do
//!   erro; o `?` com `From<TemporalError> for Thrown` faz isso nos chamadores, sem função de repasse.

use crate::runtime::host_call::{pending_or, Thrown};
use crate::runtime::intl_support::{get_property, option_enum};
use crate::runtime::iso8601::{parse_iso_date_time, parse_month_code, Duration, PlainDateTime, TemporalProduction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::object_to_primitive::PreferredPrimitiveType;
use crate::runtime::temporal_core_calendar_fields::{date_from_fields, CalendarFieldsIn, TimeFieldsIn};
use crate::runtime::temporal_object::{
    string_units, throw_range_error_with_units, to_integer_with_truncation, ParsedMonthCode, TemporalOverflow, TemporalUnit,
};
use crate::runtime::temporal_plain_date::TemporalPlainDate;
use crate::runtime::temporal_plain_date_time::TemporalPlainDateTime;
use crate::runtime::temporal_plain_month_day::TemporalPlainMonthDay;
use crate::runtime::temporal_plain_time::TemporalPlainTime;
use crate::runtime::temporal_plain_year_month::TemporalPlainYearMonth;
use crate::runtime::temporal_zoned_date_time::TemporalZonedDateTime;
use crate::wtf::option_set::OptionSet;

/// `typedef unsigned CalendarID` (`IntlObject.h`): o índice em [`AVAILABLE_CALENDARS`]. Não confundir com o
/// `CalendarId` de `iso8601.rs`, que é o texto do `[u-ca=...]` lido de uma cadeia.
pub type CalendarID = u8;

/// `intlAvailableCalendars()`: a tabela "Calendar Type" de proposal-intl-era-monthcode (os quinze de
/// `FOR_EACH_CACHED_CALENDAR_ID` e `iso8601`), ordenada como `Array.prototype.sort` sem comparador (ponto de
/// código).
pub const AVAILABLE_CALENDARS: [&str; 16] = [
    "buddhist",
    "chinese",
    "coptic",
    "dangi",
    "ethioaa",
    "ethiopic",
    "gregory",
    "hebrew",
    "indian",
    "islamic-civil",
    "islamic-tbla",
    "islamic-umalqura",
    "iso8601",
    "japanese",
    "persian",
    "roc",
];

const fn bytes_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut index = 0;
    while index < a.len() {
        if a[index] != b[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// O `CalendarID` de um nome canônico de [`AVAILABLE_CALENDARS`] (o `xxxCalendarID()` do C++).
const fn calendar_index(name: &str) -> CalendarID {
    let mut index = 0;
    while index < AVAILABLE_CALENDARS.len() {
        if bytes_equal(AVAILABLE_CALENDARS[index].as_bytes(), name.as_bytes()) {
            return index as CalendarID;
        }
        index += 1;
    }
    panic!("calendário fora de AVAILABLE_CALENDARS")
}

/// `iso8601CalendarID()`.
pub const ISO8601_CALENDAR_ID: CalendarID = calendar_index("iso8601");

/// `calendarIDToString(id)`.
pub fn calendar_id_to_string(id: CalendarID) -> &'static str {
    AVAILABLE_CALENDARS[usize::from(id)]
}

/// `calendarIsISO(id)`.
pub fn calendar_is_iso(id: CalendarID) -> bool {
    id == ISO8601_CALENDAR_ID
}

/// `calendarHasEras(id)`: os treze calendários com `era` e `eraYear` em Temporal.
pub fn calendar_has_eras(id: CalendarID) -> bool {
    [
        "buddhist",
        "coptic",
        "ethioaa",
        "ethiopic",
        "gregory",
        "hebrew",
        "indian",
        "islamic-civil",
        "islamic-tbla",
        "islamic-umalqura",
        "japanese",
        "persian",
        "roc",
    ]
    .into_iter()
    .any(|name| id == calendar_index(name))
}

/// `isBuiltinCalendar(string)`: a busca na `intlAvailableCalendarIndex()`, sem diferenciar maiúsculas de ASCII, com
/// os apelidos legados do CLDR (`islamicc` e `ethiopic-amete-alem`) apontando para o identificador canônico.
pub fn is_builtin_calendar(identifier: &[u16]) -> Option<CalendarID> {
    let ascii: Option<Vec<u8>> = identifier
        .iter()
        .map(|&unit| u8::try_from(unit).ok().filter(u8::is_ascii).map(|byte| byte.to_ascii_lowercase()))
        .collect();
    let ascii = ascii?;

    if let Some(index) = AVAILABLE_CALENDARS.iter().position(|name| name.as_bytes() == ascii.as_slice()) {
        return Some(index as CalendarID);
    }
    match ascii.as_slice() {
        b"islamicc" => Some(calendar_index("islamic-civil")),
        b"ethiopic-amete-alem" => Some(calendar_index("ethioaa")),
        _ => None,
    }
}

/// `getTemporalCalendarIdentifierWithISODefault(globalObject, item)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-gettemporalcalendarslotvaluewithisodefault
pub fn get_temporal_calendar_identifier_with_iso_default(global_object: &JSGlobalObject, item: JSValue) -> Result<CalendarID, Thrown> {
    // Passo 1: objeto com `[[InitializedTemporal*]]` devolve o `[[Calendar]]` dele.
    if let Some(plain_date) = TemporalPlainDate::from_value(&item) {
        return Ok(plain_date.calendar_id());
    }
    if let Some(plain_date_time) = TemporalPlainDateTime::from_value(&item) {
        return Ok(plain_date_time.calendar_id());
    }
    if let Some(year_month) = TemporalPlainYearMonth::from_value(&item) {
        return Ok(year_month.calendar_id());
    }
    if let Some(month_day) = TemporalPlainMonthDay::from_value(&item) {
        return Ok(month_day.calendar_id());
    }
    if let Some(zoned_date_time) = TemporalZonedDateTime::from_value(&item) {
        return Ok(zoned_date_time.calendar_id());
    }

    // Passos 2 e 3: `Get(item, "calendar")`; `undefined` é `"iso8601"`.
    let calendar_like = get_property(global_object, item, "calendar")?;
    if calendar_like.is_undefined() {
        return Ok(ISO8601_CALENDAR_ID);
    }
    // Passo 4: `ToTemporalCalendarIdentifier(calendarLike)`.
    to_temporal_calendar_identifier(global_object, calendar_like)
}

/// `toTemporalCalendarIdentifier(globalObject, calendarLike)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-totemporalcalendaridentifier
pub fn to_temporal_calendar_identifier(global_object: &JSGlobalObject, calendar_like: JSValue) -> Result<CalendarID, Thrown> {
    // Passo 1: objeto com `[[InitializedTemporal*]]` devolve o `[[Calendar]]` dele.
    if let Some(plain_date) = TemporalPlainDate::from_value(&calendar_like) {
        return Ok(plain_date.calendar_id());
    }
    if let Some(plain_date_time) = TemporalPlainDateTime::from_value(&calendar_like) {
        return Ok(plain_date_time.calendar_id());
    }
    if let Some(year_month) = TemporalPlainYearMonth::from_value(&calendar_like) {
        return Ok(year_month.calendar_id());
    }
    if let Some(month_day) = TemporalPlainMonthDay::from_value(&calendar_like) {
        return Ok(month_day.calendar_id());
    }
    if let Some(zoned_date_time) = TemporalZonedDateTime::from_value(&calendar_like) {
        return Ok(zoned_date_time.calendar_id());
    }

    // Passo 2: quem não é `String` é `TypeError`.
    if !calendar_like.is_string() {
        return Err(Thrown::type_error("calendar must be a string or Temporal object"));
    }

    let calendar_string = string_units(global_object, calendar_like)?;

    // Atalho: o nome de calendário nu (`"iso8601"`, `"hebrew"`, ...) pula `ParseTemporalCalendarString`, que
    // passaria o texto pela mesma busca de `isBuiltinCalendar`.
    if let Some(calendar_id) = is_builtin_calendar(&calendar_string) {
        return Ok(calendar_id);
    }

    // Passo 3: `ParseTemporalCalendarString(string)`: `ParseISODateTime` com as seis produções (os passos 3 a 5
    // do `ParseTemporalCalendarString` viram `CanonicalizeCalendar` abaixo: o que não é nome de calendário nem
    // cadeia de data de Temporal nunca passa pela canonicalização).
    let allowed = OptionSet::new(&[
        TemporalProduction::DateTimeZoned,
        TemporalProduction::DateTimeUnzoned,
        TemporalProduction::Instant,
        TemporalProduction::YearMonth,
        TemporalProduction::MonthDay,
        TemporalProduction::Time,
    ]);
    let Some(parsed) = parse_iso_date_time(&calendar_string, allowed) else {
        return Err(throw_range_error_with_units(global_object, "invalid calendar identifier: ", &calendar_string, ""));
    };

    // Passos 2.a a 2.c do `ParseTemporalCalendarString`: o calendário em minúsculas, ou `"iso8601"`.
    let identifier: Vec<u16> = match &parsed.calendar {
        Some(calendar) => calendar.to_ascii_lowercase().into_iter().map(u16::from).collect(),
        None => "iso8601".encode_utf16().collect(),
    };

    // Passo 4: `CanonicalizeCalendar(identifier)`.
    if let Some(calendar_id) = is_builtin_calendar(&identifier) {
        return Ok(calendar_id);
    }
    Err(throw_range_error_with_units(global_object, "invalid calendar identifier: ", &identifier, ""))
}

crate::intl_enum! {
    /// Os valores de `calendarName`, o texto que `temporalShowCalendarName` devolve.
    CalendarNameOption { Auto => "auto", Always => "always", Never => "never", Critical => "critical" }
}

/// `temporalShowCalendarName(globalObject, options)` (`GetTemporalShowCalendarNameOption`):
/// https://tc39.es/proposal-temporal/#sec-temporal-gettemporalshowcalendarnameoption
pub fn temporal_show_calendar_name(global_object: &JSGlobalObject, options: Option<JSValue>) -> Result<CalendarNameOption, Thrown> {
    let calendar_name = option_enum::<CalendarNameOption>(
        global_object,
        options,
        "calendarName",
        "calendarName must be \"auto\", \"always\", \"never\", or \"critical\"",
    )?;
    Ok(calendar_name.unwrap_or(CalendarNameOption::Auto))
}

/// O `parseMonthCode(globalObject, argument)` de `TemporalCalendar.cpp` (`ParseMonthCode`):
/// https://tc39.es/proposal-temporal/#sec-temporal-parsemonthcode
pub fn parse_month_code_value(global_object: &JSGlobalObject, argument: JSValue) -> Result<ParsedMonthCode, Thrown> {
    // Passo 1: `ToPrimitive(argument, ~string~)`.
    let primitive = pending_or(global_object, argument.to_primitive_preferred(PreferredPrimitiveType::PreferString))?;
    // Passo 2: quem não é `String` é `TypeError`.
    if !primitive.is_string() {
        return Err(Thrown::type_error("monthCode must be a string"));
    }
    let month_code = string_units(global_object, primitive)?;
    // Passos 3 a 8: a gramática de `MonthCode`; `None` é o `RangeError`.
    parse_month_code(&month_code).ok_or_else(|| Thrown::range_error("Invalid monthCode"))
}

/// `enum class FieldSetType`: o conjunto de campos que `PrepareCalendarFields` lê.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldSetType {
    Date,
    YearMonth,
    MonthDay,
    DateTime,
}

/// `readCalendarFieldsFromObject<type>(globalObject, bag, calendarId, timeFieldsOut)` (`PrepareCalendarFields`,
/// `CalendarFields::from_prop_bag` do temporal_rs): lê os campos em ordem alfabética (`day`, `era`, `eraYear`,
/// `hour`, `microsecond`, `millisecond`, `minute`, `month`, `monthCode`, `nanosecond`, `second`, `year`).
/// https://tc39.es/proposal-temporal/#sec-temporal-preparecalendarfields
///
/// `time_fields_out` é obrigatório (e só é lido) com `FieldSetType::DateTime`.
pub fn read_calendar_fields_from_object(
    global_object: &JSGlobalObject,
    bag: JSValue,
    calendar_id: CalendarID,
    set_type: FieldSetType,
    mut time_fields_out: Option<&mut TimeFieldsIn>,
) -> Result<CalendarFieldsIn, Thrown> {
    let mut fields = CalendarFieldsIn::default();

    // `readTimeField`: o campo de hora, `toIntegerWithTruncation` e finito.
    let read_time_field = |value: JSValue| -> Result<Option<f64>, Thrown> {
        if value.is_undefined() {
            return Ok(None);
        }
        let number = to_integer_with_truncation(global_object, value)?;
        if !number.is_finite() {
            return Err(Thrown::range_error("Temporal time properties must be finite"));
        }
        Ok(Some(number))
    };

    // `day` (não lido para ano-mês, pela spec).
    if set_type != FieldSetType::YearMonth {
        let day_property = get_property(global_object, bag, "day")?;
        if !day_property.is_undefined() {
            let day = to_integer_with_truncation(global_object, day_property)?;
            if !(day > 0.0 && day.is_finite()) {
                return Err(Thrown::range_error("day must be positive and finite"));
            }
            // `clampTo<uint8_t>`: a conversão do Rust satura.
            fields.day = Some(day as u8);
        }
    }

    // `era` e `eraYear` (só em calendário com eras).
    if calendar_has_eras(calendar_id) {
        let era_property = get_property(global_object, bag, "era")?;
        if !era_property.is_undefined() {
            fields.era = Some(crate::runtime::intl_support::to_rust_string(global_object, era_property)?);
        }
        let era_year_property = get_property(global_object, bag, "eraYear")?;
        if !era_year_property.is_undefined() {
            let era_year = to_integer_with_truncation(global_object, era_year_property)?;
            if !era_year.is_finite() {
                return Err(Thrown::range_error("eraYear must be finite"));
            }
            fields.era_year = Some(era_year as i32);
        }
    }

    // `hour`, `microsecond`, `millisecond` e `minute` (só `DateTime`; antes de `month` na ordem alfabética).
    if set_type == FieldSetType::DateTime {
        let time_fields = time_fields_out.as_deref_mut().expect("FieldSetType::DateTime exige timeFieldsOut");
        time_fields.hour = read_time_field(get_property(global_object, bag, "hour")?)?;
        time_fields.microsecond = read_time_field(get_property(global_object, bag, "microsecond")?)?;
        time_fields.millisecond = read_time_field(get_property(global_object, bag, "millisecond")?)?;
        time_fields.minute = read_time_field(get_property(global_object, bag, "minute")?)?;
    }

    // `month`.
    let month_property = get_property(global_object, bag, "month")?;
    if !month_property.is_undefined() {
        let month = to_integer_with_truncation(global_object, month_property)?;
        if !month.is_finite() || month < 1.0 {
            return Err(Thrown::range_error("month must be positive and finite"));
        }
        // `clampTo<uint32_t>`.
        fields.month = Some(month as u32);
    }

    // `monthCode` (`~to-month-code~`: `ParseMonthCode`).
    let month_code_property = get_property(global_object, bag, "monthCode")?;
    if !month_code_property.is_undefined() {
        fields.month_code = Some(parse_month_code_value(global_object, month_code_property)?);
    }

    // `nanosecond` e `second` (só `DateTime`; antes de `year`).
    if set_type == FieldSetType::DateTime {
        let time_fields = time_fields_out.as_deref_mut().expect("FieldSetType::DateTime exige timeFieldsOut");
        time_fields.nanosecond = read_time_field(get_property(global_object, bag, "nanosecond")?)?;
        time_fields.second = read_time_field(get_property(global_object, bag, "second")?)?;
    }

    // `year`.
    let year_property = get_property(global_object, bag, "year")?;
    if !year_property.is_undefined() {
        let year = to_integer_with_truncation(global_object, year_property)?;
        if !year.is_finite() {
            return Err(Thrown::range_error("year must be finite"));
        }
        // `clampTo<int32_t>`.
        fields.year = Some(year as i32);
    }

    Ok(fields)
}

/// `interpretTemporalDateTimeFields(globalObject, calendarId, dateFields, timeFields, overflow)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-interprettemporaldatetimefields
pub fn interpret_temporal_date_time_fields(
    calendar_id: CalendarID,
    date_fields: &CalendarFieldsIn,
    time_fields: &TimeFieldsIn,
    overflow: TemporalOverflow,
) -> Result<PlainDateTime, Thrown> {
    // Passo 1: `isoDate = ? CalendarDateFromFields(calendar, fields, overflow)`.
    let date_result = date_from_fields(calendar_id, date_fields, overflow)?;

    // Passo 2: `time = ? RegulateTime(...)`, com os campos ausentes em zero.
    let mut time_duration = Duration::default();
    for (unit, value) in [
        (TemporalUnit::Hour, time_fields.hour),
        (TemporalUnit::Minute, time_fields.minute),
        (TemporalUnit::Second, time_fields.second),
        (TemporalUnit::Millisecond, time_fields.millisecond),
        (TemporalUnit::Microsecond, time_fields.microsecond),
        (TemporalUnit::Nanosecond, time_fields.nanosecond),
    ] {
        time_duration.set_field(unit, value.unwrap_or(0.0));
    }
    let time = TemporalPlainTime::regulate_time(&time_duration, overflow)?;

    // Passo 3: `CombineISODateAndTimeRecord(isoDate, time)`.
    Ok(PlainDateTime { date: date_result.iso_date, time })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn iso8601_is_index_twelve() {
        assert_eq!(ISO8601_CALENDAR_ID, 12);
        assert_eq!(calendar_id_to_string(ISO8601_CALENDAR_ID), "iso8601");
        assert!(AVAILABLE_CALENDARS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn builtin_calendars_ignore_ascii_case_and_follow_aliases() {
        assert_eq!(is_builtin_calendar(&units("ISO8601")), Some(ISO8601_CALENDAR_ID));
        assert_eq!(is_builtin_calendar(&units("Gregory")), Some(calendar_index("gregory")));
        assert_eq!(is_builtin_calendar(&units("islamicc")), Some(calendar_index("islamic-civil")));
        assert_eq!(is_builtin_calendar(&units("ethiopic-amete-alem")), Some(calendar_index("ethioaa")));
        assert_eq!(is_builtin_calendar(&units("iso8601 ")), None);
        assert_eq!(is_builtin_calendar(&units("isoç")), None);
    }

    #[test]
    fn calendar_eras_flags() {
        assert!(calendar_has_eras(calendar_index("japanese")));
        assert!(!calendar_has_eras(calendar_index("chinese")));
        assert!(!calendar_has_eras(ISO8601_CALENDAR_ID));
    }
}
