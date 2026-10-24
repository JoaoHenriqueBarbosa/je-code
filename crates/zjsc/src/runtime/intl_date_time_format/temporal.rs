//! Os objetos `Temporal` no `Intl.DateTimeFormat` (`IntlDateTimeFormatPrototype.cpp`:
//! `isTemporalObject`, `sameTemporalType`, `handleDateTimeValue`; `IntlDateTimeFormat.cpp`:
//! `getTemporalFormatter`, `computeAdjustDateTimeStyleFormat`, `computeGetDateTimeFormat`, `format`,
//! `formatToParts`, `partitionDateTimeRangePattern`) e o `toLocaleString` dos protótipos `Temporal`.
//!
//! Sem ICU, o `[[TemporalXxxFormat]]` de cada tipo é o estado do formatador com os campos que o tipo
//! aceita (`allowedFieldsForKind` com `dateStyle` e `timeStyle`, `requiredFieldsForKind` sem eles) e,
//! para os tipos sem fuso (`isPlain`), o fuso GMT. `null` no C++ (o tipo não tem campo aplicável) é
//! `None` aqui, e vira o `TypeError` "DateTimeFormat has no fields applicable to this Temporal type".
//!
//! DIVERGÊNCIAS:
//!
//! - A conferência de calendário do `handleDateTimeValue` compara o identificador da célula com o do
//!   formatador (`calendarMatchesICU`); só `iso8601` em `PlainDate` e `PlainDateTime` é isento.
//! - O `era` de ano anterior a 1 e os padrões de cada locale são os de `intl_date_time_format.rs`.
//! - O instante de um tipo sem fuso é o dos campos como se fossem UTC, em milissegundos truncados em
//!   direção a zero (`ExactTime::epochMilliseconds`), como o C++ faz antes de dar o valor ao ICU.

use super::*;
use crate::runtime::iso8601::ExactTime;
use crate::runtime::temporal_calendar::{calendar_id_to_string, calendar_is_iso, CalendarID, ISO8601_CALENDAR_ID};
use crate::runtime::temporal_instant::TemporalInstant;
use crate::runtime::temporal_plain_date::TemporalPlainDate;
use crate::runtime::temporal_plain_date_time::TemporalPlainDateTime;
use crate::runtime::temporal_plain_month_day::TemporalPlainMonthDay;
use crate::runtime::temporal_plain_time::TemporalPlainTime;
use crate::runtime::temporal_plain_year_month::TemporalPlainYearMonth;
use crate::runtime::temporal_zoned_date_time::TemporalZonedDateTime;

const NO_APPLICABLE_FIELDS: &str = "DateTimeFormat has no fields applicable to this Temporal type";

/// `IntlDateTimeFormat::TemporalFieldKind` (sem o `None`, que é o `Number`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TemporalKind {
    PlainDate,
    PlainDateTime,
    PlainTime,
    PlainYearMonth,
    PlainMonthDay,
    Instant,
    ZonedDateTime,
}

impl TemporalKind {
    /// `isTemporalObject` e `sameTemporalType` juntos: o tipo do objeto, se é um `Temporal`.
    pub fn of(value: JSValue) -> Option<TemporalKind> {
        if TemporalInstant::from_value(&value).is_some() {
            Some(TemporalKind::Instant)
        } else if TemporalPlainDate::from_value(&value).is_some() {
            Some(TemporalKind::PlainDate)
        } else if TemporalPlainDateTime::from_value(&value).is_some() {
            Some(TemporalKind::PlainDateTime)
        } else if TemporalPlainTime::from_value(&value).is_some() {
            Some(TemporalKind::PlainTime)
        } else if TemporalPlainYearMonth::from_value(&value).is_some() {
            Some(TemporalKind::PlainYearMonth)
        } else if TemporalPlainMonthDay::from_value(&value).is_some() {
            Some(TemporalKind::PlainMonthDay)
        } else if TemporalZonedDateTime::from_value(&value).is_some() {
            Some(TemporalKind::ZonedDateTime)
        } else {
            None
        }
    }

    /// `isPlain`: sem fuso, com o GMT para a conta do instante.
    fn is_plain(self) -> bool {
        !matches!(self, TemporalKind::Instant | TemporalKind::ZonedDateTime)
    }
}

/// Os grupos de campos que um tipo aceita (`dateFields`, `timeFields`, `yearMonthFields`...).
#[derive(Clone, Copy)]
struct Mask {
    weekday: bool,
    era: bool,
    year: bool,
    month: bool,
    day: bool,
    /// `dayPeriod`, `hour`, `minute`, `second` e `fractionalSecondDigits`.
    time: bool,
}

const fn mask(weekday: bool, era: bool, year: bool, month: bool, day: bool, time: bool) -> Mask {
    Mask { weekday, era, year, month, day, time }
}

/// `allowedFieldsForKind`: o que o `AdjustDateTimeStyleFormat` deixa passar (com `era`).
fn allowed_mask(kind: TemporalKind) -> Mask {
    match kind {
        TemporalKind::PlainDate => mask(true, true, true, true, true, false),
        TemporalKind::PlainDateTime => mask(true, true, true, true, true, true),
        TemporalKind::PlainTime => mask(false, false, false, false, false, true),
        TemporalKind::PlainYearMonth => mask(false, true, true, true, false, false),
        TemporalKind::PlainMonthDay => mask(false, false, false, true, true, false),
        TemporalKind::Instant | TemporalKind::ZonedDateTime => mask(false, false, false, false, false, false),
    }
}

/// `requiredFieldsForKind`: o que satisfaz o `GetDateTimeFormat` (sem `era`).
fn required_mask(kind: TemporalKind) -> Mask {
    let allowed = allowed_mask(kind);
    Mask { era: false, ..allowed }
}

/// Os campos de `fields` que `mask` aceita.
fn pick(fields: &Fields, mask: Mask) -> Fields {
    let mut picked = Fields::default();
    if mask.weekday {
        picked.weekday = fields.weekday;
    }
    if mask.era {
        picked.era = fields.era;
    }
    if mask.year {
        picked.year = fields.year;
    }
    if mask.month {
        picked.month = fields.month;
    }
    if mask.day {
        picked.day = fields.day;
    }
    if mask.time {
        picked.day_period = fields.day_period;
        picked.hour = fields.hour;
        picked.minute = fields.minute;
        picked.second = fields.second;
        picked.fractional_second_digits = fields.fractional_second_digits;
    }
    picked
}

/// `computeAdjustDateTimeStyleFormat` (e o `Instant` e os `null` de `computeTemporalFormatter` com
/// `dateStyle` ou `timeStyle`).
fn styled_fields(state: &DateTimeFormatState, kind: TemporalKind) -> Option<Fields> {
    match kind {
        // O formatador base já tem o estilo e o fuso.
        TemporalKind::Instant => return Some(state.fields),
        TemporalKind::PlainDate | TemporalKind::PlainYearMonth | TemporalKind::PlainMonthDay if state.date_style.is_none() => {
            return None;
        }
        TemporalKind::PlainTime if state.time_style.is_none() => return None,
        _ => {}
    }
    // Passos 1 a 3: sem campo em conflito (o fuso conflita com os tipos sem fuso) vale o formatador base.
    let kept = pick(&state.fields, allowed_mask(kind));
    if kept == state.fields {
        return Some(state.fields);
    }
    // Passos 5 a 9: o melhor padrão para os campos aceitos, ou `null` se não sobrou nenhum.
    if !kept.any() {
        return None;
    }
    Some(resolve_fields(state.language, state.cycle, kept))
}

/// `computeGetDateTimeFormat` sem `dateStyle` nem `timeStyle`.
fn unstyled_fields(state: &DateTimeFormatState, kind: TemporalKind) -> Option<Fields> {
    let user = &state.user_fields;
    if kind == TemporalKind::Instant {
        // `inherit = ~all~`: com algum campo pedido o formatador base já é o resultado.
        if state.any_present {
            return Some(state.fields);
        }
        let mut fields = *user;
        fields.year = Some(Digits2::Numeric);
        fields.month = Some(Month::Numeric);
        fields.day = Some(Digits2::Numeric);
        fields.hour = Some(Digits2::Numeric);
        fields.minute = Some(Digits2::Numeric);
        fields.second = Some(Digits2::Numeric);
        return Some(resolve_fields(state.language, state.cycle, fields));
    }
    // `inherit = ~relevant~`: o `era` (se o tipo o aceita) e os campos obrigatórios do tipo.
    let mut fields = pick(user, required_mask(kind));
    fields.era = pick(user, allowed_mask(kind)).era;
    if !(fields.has_date_without_era() || fields.has_time()) {
        // Passo 17a: algo foi pedido, mas nada que este tipo use.
        if state.any_present {
            return None;
        }
        // Passo 17b: os campos padrão do tipo (`defaultOptions`).
        let (year, month, day, time) = match kind {
            TemporalKind::PlainTime => (false, false, false, true),
            TemporalKind::PlainDate => (true, true, true, false),
            TemporalKind::PlainDateTime => (true, true, true, true),
            TemporalKind::PlainYearMonth => (true, true, false, false),
            TemporalKind::PlainMonthDay => (false, true, true, false),
            TemporalKind::Instant | TemporalKind::ZonedDateTime => unreachable!("tratados antes"),
        };
        if year {
            fields.year = Some(Digits2::Numeric);
        }
        if month {
            fields.month = Some(Month::Numeric);
        }
        if day {
            fields.day = Some(Digits2::Numeric);
        }
        if time {
            fields.hour = Some(Digits2::Numeric);
            fields.minute = Some(Digits2::Numeric);
            fields.second = Some(Digits2::Numeric);
        }
    }
    Some(resolve_fields(state.language, state.cycle, fields))
}

/// `getTemporalFormatter(kind)` como um estado de formatador: os campos do tipo e, nos tipos sem fuso,
/// o GMT. `None` é o formatador `null`.
fn temporal_state(state: &DateTimeFormatState, kind: TemporalKind) -> Option<DateTimeFormatState> {
    debug_assert!(kind != TemporalKind::ZonedDateTime);
    let fields = if state.date_style.is_some() || state.time_style.is_some() {
        styled_fields(state, kind)?
    } else {
        unstyled_fields(state, kind)?
    };
    let mut derived = state.clone();
    derived.hour_cycle = fields.hour.map(|_| state.cycle);
    derived.fields = fields;
    if kind.is_plain() {
        derived.zone = TimeZone::UTC;
        derived.zone_name = "UTC".to_string();
    }
    Some(derived)
}

/// O resultado de `handleDateTimeValue` para um objeto `Temporal`: o tipo, o valor em milissegundos e o
/// formatador do tipo.
pub(super) struct TemporalRecord {
    ms: i64,
    state: DateTimeFormatState,
}

/// `ISO8601::ExactTime::fromISOPartsAndOffset(...).epochMilliseconds()` dos campos como UTC.
fn epoch_milliseconds_of(year: i32, month: u8, day: u8, time: (u32, u32, u32, u32, u32, u32)) -> i64 {
    ExactTime::from_iso_parts_and_offset(year, month, day, time.0, time.1, time.2, time.3, time.4, time.5, 0).epoch_milliseconds()
}

/// A conferência de calendário do `handleDateTimeValue` (`validateCalendar`): o `iso8601` só é isento em
/// `PlainDate` e `PlainDateTime`; nos outros, e em qualquer calendário não ISO, o identificador da célula tem de
/// ser o do formatador (`calendarMatchesICU`).
fn validate_calendar(state: &DateTimeFormatState, kind: TemporalKind, calendar_id: CalendarID) -> Result<(), Thrown> {
    let exempt = matches!(kind, TemporalKind::PlainDate | TemporalKind::PlainDateTime);
    if calendar_is_iso(calendar_id) && exempt {
        return Ok(());
    }
    let formatter_calendar = if state.iso_calendar {
        "iso8601"
    } else if state.native.is_native() {
        state.native.name()
    } else {
        "gregory"
    };
    if calendar_id_to_string(calendar_id) == formatter_calendar {
        Ok(())
    } else {
        Err(Thrown::range_error("Temporal object's calendar does not match DateTimeFormat calendar"))
    }
}

/// `handleDateTimeValue` de um objeto `Temporal`; `None` se `value` não é um (o chamador faz o
/// `HandleDateTimeOthers`).
pub(super) fn handle_date_time_value(state: &DateTimeFormatState, value: JSValue) -> Result<Option<TemporalRecord>, Thrown> {
    let Some(kind) = TemporalKind::of(value) else { return Ok(None) };
    let ms = match kind {
        TemporalKind::PlainDate => {
            let cell = TemporalPlainDate::from_value(&value).expect("o tipo acabou de ser conferido");
            let date = cell.plain_date();
            validate_calendar(state, kind, cell.calendar_id())?;
            // `CombineISODateAndTimeRecord(isoDate, NoonTimeRecord())`.
            epoch_milliseconds_of(date.year(), date.month(), date.day(), (12, 0, 0, 0, 0, 0))
        }
        TemporalKind::PlainYearMonth => {
            let cell = TemporalPlainYearMonth::from_value(&value).expect("o tipo acabou de ser conferido");
            validate_calendar(state, kind, cell.calendar_id())?;
            let year_month = cell.plain_year_month();
            let iso = year_month.iso_plain_date();
            epoch_milliseconds_of(iso.year(), iso.month(), iso.day(), (12, 0, 0, 0, 0, 0))
        }
        TemporalKind::PlainMonthDay => {
            let cell = TemporalPlainMonthDay::from_value(&value).expect("o tipo acabou de ser conferido");
            validate_calendar(state, kind, cell.calendar_id())?;
            let month_day = cell.plain_month_day();
            let iso = month_day.iso_plain_date();
            epoch_milliseconds_of(iso.year(), iso.month(), iso.day(), (12, 0, 0, 0, 0, 0))
        }
        TemporalKind::PlainTime => {
            let time = TemporalPlainTime::from_value(&value).expect("o tipo acabou de ser conferido").plain_time();
            epoch_milliseconds_of(
                1970,
                1,
                1,
                (time.hour(), time.minute(), time.second(), time.millisecond(), time.microsecond(), time.nanosecond()),
            )
        }
        TemporalKind::PlainDateTime => {
            let cell = TemporalPlainDateTime::from_value(&value).expect("o tipo acabou de ser conferido");
            validate_calendar(state, kind, cell.calendar_id())?;
            let (date, time) = (cell.plain_date(), cell.plain_time());
            epoch_milliseconds_of(
                date.year(),
                date.month(),
                date.day(),
                (time.hour(), time.minute(), time.second(), time.millisecond(), time.microsecond(), time.nanosecond()),
            )
        }
        TemporalKind::Instant => {
            TemporalInstant::from_value(&value).expect("o tipo acabou de ser conferido").exact_time().epoch_milliseconds()
        }
        TemporalKind::ZonedDateTime => {
            return Err(Thrown::type_error(
                "Temporal.ZonedDateTime is not supported in Intl.DateTimeFormat; use toLocaleString() or convert to PlainDateTime first",
            ));
        }
    };
    let derived = temporal_state(state, kind).ok_or_else(|| Thrown::type_error(NO_APPLICABLE_FIELDS))?;
    Ok(Some(TemporalRecord { ms, state: derived }))
}

/// As partes de `value` para `format` e `formatToParts`: `undefined` é agora, um `Temporal` o do tipo,
/// o resto `ToNumber` e `TimeClip`.
pub(super) fn value_parts(
    global_object: &JSGlobalObject,
    state: &DateTimeFormatState,
    value: JSValue,
    method: &str,
) -> Result<Vec<Part>, Thrown> {
    if !value.is_undefined() {
        if let Some(record) = handle_date_time_value(state, value)? {
            return Ok(format_to_parts_at(&record.state, record.ms));
        }
    }
    let milliseconds = date_argument(global_object, value, method)?;
    Ok(format_to_parts_at(state, milliseconds))
}

/// Os dois extremos de `formatRange` e `formatRangeToParts` (`ToDateTimeFormattable` e
/// `partitionDateTimeRangePattern`): o que não é `Temporal` vira `Number` (o primeiro, depois o
/// segundo); `Temporal` exige o mesmo tipo nos dois; sem `Temporal`, `TimeClip` com `NaN` como
/// `RangeError`.
pub(super) fn range_parts(
    global_object: &JSGlobalObject,
    state: &DateTimeFormatState,
    start_value: JSValue,
    end_value: JSValue,
) -> Result<Vec<RangePart>, Thrown> {
    let (start_kind, end_kind) = (TemporalKind::of(start_value), TemporalKind::of(end_value));
    let start_number = if start_kind.is_none() { Some(to_number_checked(global_object, start_value)?) } else { None };
    let end_number = if end_kind.is_none() { Some(to_number_checked(global_object, end_value)?) } else { None };
    if start_kind.is_some() || end_kind.is_some() {
        if start_kind != end_kind {
            return Err(Thrown::type_error("formatRange requires both arguments to be the same Temporal type"));
        }
        let start = handle_date_time_value(state, start_value)?.expect("o tipo foi conferido");
        let end = handle_date_time_value(state, end_value)?.expect("o tipo foi conferido");
        return Ok(range::format_range_parts_at(&start.state, start.ms, end.ms));
    }
    let (start, end) = (time_clip(start_number.expect("sem Temporal")), time_clip(end_number.expect("sem Temporal")));
    if start.is_nan() || end.is_nan() {
        return Err(Thrown::range_error("Passed date is out of range"));
    }
    Ok(range::format_range_parts_at(state, start as i64, end as i64))
}

/// O `toLocaleString` de `PlainDate`, `PlainTime`, `PlainDateTime`, `PlainYearMonth`, `PlainMonthDay` e
/// `Instant`: `CreateDateTimeFormat(%Intl.DateTimeFormat%, locales, options, required, defaults)` e
/// `FormatDateTime(dateTimeFormat, this)`.
pub fn to_locale_string(
    global_object: &JSGlobalObject,
    locales: JSValue,
    options: JSValue,
    required: Required,
    defaults: Defaults,
    this_value: JSValue,
) -> Result<String, Thrown> {
    let state = initialize(global_object, locales, options, required, defaults, None)?;
    let record = handle_date_time_value(&state, this_value)?.expect("o chamador conferiu a marca");
    Ok(format_to_parts_at(&record.state, record.ms).into_iter().map(|(_, text)| text).collect())
}

/// O `toLocaleString` de `ZonedDateTime`: `CreateDateTimeFormat(..., ~any~, ~all~, zonedDateTime.[[TimeZone]])`
/// (com `timeZoneName: "short"` quando nada foi pedido) e `FormatDateTime` do instante, no fuso do objeto.
pub fn zoned_date_time_to_locale_string(
    global_object: &JSGlobalObject,
    locales: JSValue,
    options: JSValue,
    time_zone_id: &str,
    epoch_milliseconds: i64,
) -> Result<String, Thrown> {
    let state = initialize(global_object, locales, options, Required::Any, Defaults::ZonedDateTime, Some(time_zone_id))?;
    Ok(format_to_parts_at(&state, epoch_milliseconds).into_iter().map(|(_, text)| text).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 14/11/2023 22:13:20 UTC.
    const BASE: i64 = 1_700_000_000_000;

    fn state(language: Language, user: Fields, cycle: HourCycle, base: Fields, zone: TimeZone) -> DateTimeFormatState {
        let fields = resolve_fields(language, cycle, base);
        DateTimeFormatState {
            locale: String::new(),
            language,
            zone,
            zone_name: "America/Sao_Paulo".to_string(),
            hour_cycle: fields.hour.map(|_| cycle),
            fields,
            date_style: None,
            time_style: None,
            user_fields: user,
            any_present: user.has_date_without_era() || user.has_time(),
            cycle,
            iso_calendar: false,
            native: crate::runtime::intl_calendar::NativeCalendar::default(),
            numbering: String::new(),
        }
    }

    fn numeric_date() -> Fields {
        Fields {
            year: Some(Digits2::Numeric),
            month: Some(Month::Numeric),
            day: Some(Digits2::Numeric),
            ..Fields::default()
        }
    }

    fn numeric_time() -> Fields {
        Fields {
            hour: Some(Digits2::Numeric),
            minute: Some(Digits2::Numeric),
            second: Some(Digits2::Numeric),
            ..Fields::default()
        }
    }

    /// O formatador do `new Intl.DateTimeFormat("en")`: nada pedido, padrão de data.
    fn default_constructor(language: Language) -> DateTimeFormatState {
        let cycle = if language == Language::English { HourCycle::H12 } else { HourCycle::H23 };
        state(language, Fields::default(), cycle, numeric_date(), jiff::tz::TimeZoneDatabase::bundled().get("America/Sao_Paulo").unwrap())
    }

    fn text(derived: &DateTimeFormatState, ms: i64) -> String {
        format_to_parts_at(derived, ms).into_iter().map(|(_, text)| text).collect()
    }

    fn of(base: &DateTimeFormatState, kind: TemporalKind, ms: i64) -> Option<String> {
        temporal_state(base, kind).map(|derived| text(&derived, ms))
    }

    #[test]
    fn plain_types_take_their_default_fields_and_ignore_the_zone() {
        let english = default_constructor(Language::English);
        // A hora de parede do `PlainDateTime` é a dos campos, não a de São Paulo.
        assert_eq!(of(&english, TemporalKind::PlainDateTime, BASE).unwrap(), "11/14/2023, 10:13:20 PM");
        assert_eq!(of(&english, TemporalKind::PlainDate, BASE).unwrap(), "11/14/2023");
        assert_eq!(of(&english, TemporalKind::PlainTime, BASE).unwrap(), "10:13:20 PM");
        // `PlainYearMonth` e `PlainMonthDay` só formatam com `calendar: "iso8601"` (senão `RangeError`),
        // e então o padrão é o do CLDR para `iso8601`, igual em todo locale.
        let mut iso = english.clone();
        iso.iso_calendar = true;
        assert_eq!(of(&iso, TemporalKind::PlainYearMonth, BASE).unwrap(), "2023-11");
        assert_eq!(of(&iso, TemporalKind::PlainMonthDay, BASE).unwrap(), "11-14");
        // O `Instant` usa o fuso do formatador e todos os campos padrão.
        assert_eq!(of(&english, TemporalKind::Instant, BASE).unwrap(), "11/14/2023, 7:13:20 PM");
    }

    #[test]
    fn portuguese_defaults_use_the_locale_pattern() {
        let portuguese = default_constructor(Language::Portuguese);
        assert_eq!(of(&portuguese, TemporalKind::PlainDate, BASE).unwrap(), "14/11/2023");
        assert_eq!(of(&portuguese, TemporalKind::PlainTime, BASE).unwrap(), "22:13:20");
        // No calendário `iso8601` o padrão não depende do locale.
        let mut iso = portuguese.clone();
        iso.iso_calendar = true;
        assert_eq!(of(&iso, TemporalKind::PlainMonthDay, BASE).unwrap(), "11-14");
        assert_eq!(of(&iso, TemporalKind::PlainYearMonth, BASE).unwrap(), "2023-11");
    }

    #[test]
    fn requested_fields_without_the_required_ones_have_no_format() {
        // `{ hour: "numeric" }`: um `PlainDate` não tem campo aplicável, um `PlainTime` tem.
        let user = Fields { hour: Some(Digits2::Numeric), ..Fields::default() };
        let base = state(Language::English, user, HourCycle::H12, user, TimeZone::UTC);
        assert_eq!(of(&base, TemporalKind::PlainDate, BASE), None);
        assert_eq!(of(&base, TemporalKind::PlainYearMonth, BASE), None);
        assert_eq!(of(&base, TemporalKind::PlainMonthDay, BASE), None);
        assert_eq!(of(&base, TemporalKind::PlainTime, BASE).unwrap(), "10 PM");
        assert_eq!(of(&base, TemporalKind::PlainDateTime, BASE).unwrap(), "10 PM");
        assert_eq!(of(&base, TemporalKind::Instant, BASE).unwrap(), "10 PM");
        // `{ year: "numeric" }`: o `PlainTime` fica sem formato.
        let user = Fields { year: Some(Digits2::Numeric), ..Fields::default() };
        let base = state(Language::English, user, HourCycle::H12, user, TimeZone::UTC);
        assert_eq!(of(&base, TemporalKind::PlainTime, BASE), None);
        assert_eq!(of(&base, TemporalKind::PlainDate, BASE).unwrap(), "2023");
    }

    #[test]
    fn era_is_kept_only_where_the_type_accepts_it() {
        let user = Fields { era: Some(TextWidth::Short), ..Fields::default() };
        let base = state(Language::English, user, HourCycle::H12, Fields { era: user.era, ..numeric_date() }, TimeZone::UTC);
        assert!(of(&base, TemporalKind::PlainDate, BASE).unwrap().starts_with("11/14/2023"));
        assert!(of(&base, TemporalKind::PlainDate, BASE).unwrap().contains("AD"));
        assert_eq!(of(&base, TemporalKind::PlainTime, BASE).unwrap(), "10:13:20 PM");
        // `PlainMonthDay` só formata com `calendar: "iso8601"` (no gregoriano o bun lança `RangeError`).
        let mut iso = base.clone();
        iso.iso_calendar = true;
        assert_eq!(of(&iso, TemporalKind::PlainMonthDay, BASE).unwrap(), "11-14");
    }

    #[test]
    fn time_zone_name_is_dropped_by_the_plain_types() {
        let user = Fields {
            hour: Some(Digits2::Numeric),
            minute: Some(Digits2::Numeric),
            time_zone_name: Some(TimeZoneName::Short),
            ..Fields::default()
        };
        let zone = jiff::tz::TimeZoneDatabase::bundled().get("America/Sao_Paulo").unwrap();
        let base = state(Language::English, user, HourCycle::H12, user, zone);
        assert_eq!(of(&base, TemporalKind::PlainTime, BASE).unwrap(), "10:13 PM");
        assert_eq!(of(&base, TemporalKind::Instant, BASE).unwrap(), "7:13 PM GMT-3");
    }

    #[test]
    fn styles_filter_the_fields_a_type_does_not_have() {
        let mut base = default_constructor(Language::English);
        base.date_style = Some(StyleWidth::Long);
        base.time_style = Some(StyleWidth::Short);
        base.fields = resolve_fields(Language::English, HourCycle::H12, fields_for_styles(Language::English, base.date_style, base.time_style));
        assert_eq!(of(&base, TemporalKind::PlainDateTime, BASE).unwrap(), "November 14, 2023 at 10:13 PM");
        assert_eq!(of(&base, TemporalKind::PlainDate, BASE).unwrap(), "November 14, 2023");
        assert_eq!(of(&base, TemporalKind::PlainTime, BASE).unwrap(), "10:13 PM");
        // `dateStyle: "long"` com `iso8601`: o ICU perde o nome do mês (golden do bun: `"2024 "`).
        let mut iso = base.clone();
        iso.iso_calendar = true;
        assert_eq!(of(&iso, TemporalKind::PlainYearMonth, BASE).unwrap(), "2023 ");
        // Só `dateStyle`: sem `timeStyle` o `PlainTime` não tem formato; só `timeStyle`, as datas não.
        let mut date_only = base.clone();
        date_only.time_style = None;
        date_only.fields = resolve_fields(Language::English, HourCycle::H12, fields_for_styles(Language::English, date_only.date_style, None));
        assert_eq!(of(&date_only, TemporalKind::PlainTime, BASE), None);
        let mut time_only = base.clone();
        time_only.date_style = None;
        time_only.fields = resolve_fields(Language::English, HourCycle::H12, fields_for_styles(Language::English, None, time_only.time_style));
        assert_eq!(of(&time_only, TemporalKind::PlainDate, BASE), None);
        assert_eq!(of(&time_only, TemporalKind::PlainMonthDay, BASE), None);
        assert_eq!(of(&time_only, TemporalKind::PlainDateTime, BASE).unwrap(), "10:13 PM");
    }

    #[test]
    fn time_zone_in_a_time_style_conflicts_with_the_plain_types() {
        let mut base = default_constructor(Language::English);
        base.time_style = Some(StyleWidth::Long);
        base.fields = resolve_fields(Language::English, HourCycle::H12, fields_for_styles(Language::English, None, base.time_style));
        assert_eq!(of(&base, TemporalKind::PlainTime, BASE).unwrap(), "10:13:20 PM");
        assert_eq!(of(&base, TemporalKind::Instant, BASE).unwrap(), "7:13:20 PM GMT-3");
    }

    #[test]
    fn calendar_check_exempts_only_date_and_date_time() {
        let mut base = default_constructor(Language::English);
        assert!(validate_calendar(&base, TemporalKind::PlainDate, ISO8601_CALENDAR_ID).is_ok());
        assert!(validate_calendar(&base, TemporalKind::PlainDateTime, ISO8601_CALENDAR_ID).is_ok());
        assert!(validate_calendar(&base, TemporalKind::PlainYearMonth, ISO8601_CALENDAR_ID).is_err());
        assert!(validate_calendar(&base, TemporalKind::PlainMonthDay, ISO8601_CALENDAR_ID).is_err());
        base.iso_calendar = true;
        assert!(validate_calendar(&base, TemporalKind::PlainYearMonth, ISO8601_CALENDAR_ID).is_ok());
    }

    #[test]
    fn plain_range_uses_the_gmt_wall_clock() {
        let english = default_constructor(Language::English);
        let derived = temporal_state(&english, TemporalKind::PlainDate).unwrap();
        let parts = range::format_range_parts_at(&derived, BASE, BASE + 2 * 86_400_000);
        let joined: String = parts.into_iter().map(|(_, text, _)| text).collect();
        assert_eq!(joined, "11/14/2023 \u{2013} 11/16/2023");
    }

    #[test]
    fn epoch_milliseconds_follow_the_c_plus_plus_truncation() {
        assert_eq!(epoch_milliseconds_of(1970, 1, 1, (0, 0, 1, 500, 0, 0)), 1500);
        assert_eq!(epoch_milliseconds_of(2023, 11, 14, (12, 0, 0, 0, 0, 0)), 1_699_963_200_000);
        // Antes de 1970 o ExactTime trunca em direção a zero.
        assert_eq!(epoch_milliseconds_of(1969, 12, 31, (23, 59, 59, 999, 999, 999)), 0);
    }
}
