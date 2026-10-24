//! Porte da parte de leitura de `CalendarICUBridge` (`temporal/core/CalendarICUBridge.{h,cpp}`) para `PlainDate`:
//! a conversão de uma data ISO para o calendário e os getters `year`, `month`, `monthCode`, `day`, `era`,
//! `eraYear`, `daysInMonth`, `daysInYear`, `monthsInYear` e `inLeapYear`. O ICU é o `icu_calendar` (icu4x, dados
//! compilados), o mesmo que `intl_calendar.rs` usa para o `Intl.DateTimeFormat`.
//!
//! Medido no bun 1.4.2 (`PlainDate.from('2024-03-15').withCalendar(...)`):
//! - `year` é o ano aritmético (`extended_year` do icu4x): 5784 no hebraico, 2019 no japonês (era `reiwa`,
//!   `eraYear` 1), 0 no `roc` em 1911 (era `broc`, `eraYear` 1), 0 no gregoriano em `0000-06-01` (era `bce`,
//!   `eraYear` 1); nos cíclicos (`chinese`, `dangi`) é o ano ISO relacionado e `era` e `eraYear` são `undefined`.
//! - `month` é a posição no ano (hebraico 5784 tem 13: Adar II é `month` 7 com `monthCode` `M06`); `monthCode` do
//!   mês bissexto leva `L` (`M05L`, `M02L`).
//! - Japonês anterior a Meiji: `era` `ce`, `eraYear` igual a `year` (`1868-01-01` dá `ce` 1868).
//!
//! `calendar_fields` é o ponto único de despacho dos getters de calendário (`PlainDate`, `PlainDateTime`,
//! `ZonedDateTime`): o calendário ISO usa a aritmética de `iso8601.rs`, os outros o ICU. `date_from_calendar_fields`
//! é o `dateFromFields` dos calendários não ISO (`Date::try_from_fields` do icu4x), com as mensagens medidas no bun.
//!
//! `calendar_date_add` e `calendar_date_until` são o despacho único de `CalendarDateAdd` e `CalendarDateUntil`: o
//! calendário ISO e os de estrutura gregoriana (`gregory`, `buddhist`, `roc`, `japanese`) usam a aritmética ISO de
//! `temporal_core_iso_date.rs`, os demais (hebraico, chinês, dangi, islâmicos, persa, copta, etíope, indiano) o
//! `Date::try_added_with_options` e `Date::try_until_with_options` do icu4x (anos, depois meses, depois dias, como
//! o `NonISODateAdd`). Mensagens de `reject` do bun: `day is out of range for the resulting month (overflow:
//! reject)`, `month code does not exist in the target year (overflow: reject)`, `Failed to perform ICU calendar
//! arithmetic`; `chinese` e `dangi` fora de ±10000 caem na aritmética ISO.
//!
//! DIVERGÊNCIAS: sem teste unitário nem compilação; `calendarDateUntil` com `smallestUnit` ou `roundingIncrement`
//! passa por `RoundRelativeDuration` com o `CalendarID` (`temporal_core_duration.rs`).

use icu_calendar::{AnyCalendar, AnyCalendarKind, Date};

use icu_calendar::error::{DateAddError, DateFromFieldsError};
use icu_calendar::options::{DateAddOptions, DateDifferenceOptions, DateDurationUnit, DateFromFieldsOptions, Overflow};
use icu_calendar::types::{DateDuration, DateFields};
use icu_calendar::Iso;

use crate::runtime::intl_support::str_value;
use crate::runtime::iso8601::{days_in_month, is_date_time_within_limits, month_code, parse_month_code, Duration, PlainDate};
use crate::runtime::js_value::{js_number, js_undefined, JSValue};
use crate::runtime::vm::VM;
use crate::runtime::temporal_calendar::{calendar_has_eras, calendar_id_to_string, calendar_is_iso, CalendarID};
use crate::runtime::temporal_core_calendar_fields::{CalendarFieldsIn, ResolveType, ResolvedCalendarDate};
use crate::runtime::temporal_core_iso_date::{diff_iso_date, iso_date_add, iso_date_compare, OUT_OF_RANGE};
use crate::runtime::temporal_core_types::{range_error, TemporalError, TemporalErrorKind, TemporalResult};
use crate::runtime::temporal_object::{TemporalOverflow, TemporalUnit};
use crate::wtf::date_math::is_leap_year;

/// O `AnyCalendarKind` de um nome canônico de `AVAILABLE_CALENDARS`; `None` para `iso8601`.
fn calendar_kind(id: CalendarID) -> Option<AnyCalendarKind> {
    Some(match calendar_id_to_string(id) {
        "buddhist" => AnyCalendarKind::Buddhist,
        "chinese" => AnyCalendarKind::Chinese,
        "coptic" => AnyCalendarKind::Coptic,
        "dangi" => AnyCalendarKind::Dangi,
        "ethioaa" => AnyCalendarKind::EthiopianAmeteAlem,
        "ethiopic" => AnyCalendarKind::Ethiopian,
        "gregory" => AnyCalendarKind::Gregorian,
        "hebrew" => AnyCalendarKind::Hebrew,
        "indian" => AnyCalendarKind::Indian,
        "islamic-civil" => AnyCalendarKind::HijriTabularTypeIIFriday,
        "islamic-tbla" => AnyCalendarKind::HijriTabularTypeIIThursday,
        "islamic-umalqura" => AnyCalendarKind::HijriUmmAlQura,
        "japanese" => AnyCalendarKind::Japanese,
        "persian" => AnyCalendarKind::Persian,
        "roc" => AnyCalendarKind::Roc,
        _ => return None,
    })
}

/// A data ISO `iso` no calendário `kind`; `None` para uma data que o icu4x não representa.
fn iso_to_calendar_date(kind: AnyCalendarKind, iso: &PlainDate) -> Option<Date<AnyCalendar>> {
    Some(icu_iso_date(iso)?.to_calendar(AnyCalendar::new(kind)))
}

/// A data ISO no icu4x, por `Date::try_from_fields` (que vale em toda a faixa do icu4x, de -999999 a 999999) e não por
/// `Date::try_new_iso`, que só aceita os anos de -9999 a 9999: os limites do Temporal (-271821 a 275760) ficam fora dele.
fn icu_iso_date(iso: &PlainDate) -> Option<Date<Iso>> {
    let mut fields = DateFields::default();
    fields.extended_year = Some(iso.year());
    fields.ordinal_month = Some(iso.month());
    fields.day = Some(iso.day());
    Date::try_from_fields(fields, DateFromFieldsOptions::default(), Iso).ok()
}

/// A data ISO equivalente a uma data de calendário.
fn calendar_date_to_iso_date(date: &Date<AnyCalendar>) -> PlainDate {
    let iso = date.to_calendar(Iso);
    PlainDate::new(i64::from(iso.year().extended_year()), u32::from(iso.month().ordinal), u32::from(iso.day_of_month().0))
}

/// `calendarUsesISODateArithmetic` sem o `iso8601`: `gregory` e os de estrutura gregoriana somam e subtraem como o ISO.
fn calendar_uses_iso_arithmetic(kind: AnyCalendarKind) -> bool {
    matches!(kind, AnyCalendarKind::Gregorian | AnyCalendarKind::Buddhist | AnyCalendarKind::Roc | AnyCalendarKind::Japanese)
}

/// `calendarUsesISOFallbackForExtremeYear`: `chinese` e `dangi` fora de ±10000 (a faixa bem comportada do icu4x).
fn calendar_uses_iso_fallback_for_extreme_year(kind: AnyCalendarKind, iso_year: i32) -> bool {
    matches!(kind, AnyCalendarKind::Chinese | AnyCalendarKind::Dangi) && iso_year.abs() > 10000
}

/// Os campos de calendário de um `PlainDate` que os getters leem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarDateFields {
    pub year: i32,
    pub month: u8,
    pub month_code: String,
    pub day: u8,
    /// `None` nos calendários sem era (`chinese`, `dangi`).
    pub era: Option<String>,
    pub era_year: Option<i32>,
    pub days_in_month: u8,
    pub days_in_year: u16,
    pub months_in_year: u8,
    pub in_leap_year: bool,
    /// `calendarDayOfYear`: o dia (base 1) dentro do ano do calendário.
    pub day_of_year: u16,
}

/// Lê os campos de `iso` no calendário `id`. `None` para `iso8601` (o ramo ISO é de `iso8601.rs`) ou para uma data
/// que o icu4x não representa.
pub fn calendar_date_fields(id: CalendarID, iso: &PlainDate) -> Option<CalendarDateFields> {
    let kind = calendar_kind(id)?;
    let date = iso_to_calendar_date(kind, iso)?;
    let year_info = date.year();
    let era = year_info.era();
    Some(CalendarDateFields {
        year: year_info.extended_year(),
        month: date.month().ordinal,
        month_code: date.month().to_input().code().0.as_str().to_owned(),
        day: date.day_of_month().0,
        era: era.map(|era_year| era_year.era.as_str().to_owned()),
        era_year: era.map(|era_year| era_year.year),
        days_in_month: date.days_in_month(),
        days_in_year: date.days_in_year(),
        months_in_year: date.months_in_year(),
        in_leap_year: date.is_in_leap_year(),
        day_of_year: date.day_of_year().0,
    })
}

/// Os campos de `iso` no calendário ISO: o ramo ISO de `CalendarISOToDate` (sem era).
fn iso_calendar_date_fields(iso: &PlainDate) -> CalendarDateFields {
    let leap = is_leap_year(iso.year());
    CalendarDateFields {
        year: iso.year(),
        month: iso.month(),
        month_code: month_code(u32::from(iso.month())),
        day: iso.day(),
        era: None,
        era_year: None,
        days_in_month: days_in_month(iso.year(), iso.month()),
        days_in_year: if leap { 366 } else { 365 },
        months_in_year: 12,
        in_leap_year: leap,
        day_of_year: crate::runtime::iso8601::day_of_year(*iso),
    }
}

/// O despacho único de `CalendarISOToDate(calendar, isoDate)` para os getters de `PlainDate`, `PlainDateTime` e
/// `ZonedDateTime`: ISO direto, os demais pelo ICU.
pub fn calendar_fields(id: CalendarID, iso: &PlainDate) -> CalendarDateFields {
    if calendar_is_iso(id) {
        return iso_calendar_date_fields(iso);
    }
    calendar_date_fields(id, iso).expect("data dentro dos limites de Temporal é representável no icu4x")
}

/// O `RangeError` do bun para cada erro de `Date::try_from_fields` (medido no bun 1.4.2).
fn from_fields_error(error: DateFromFieldsError) -> TemporalError {
    range_error(match error {
        DateFromFieldsError::InvalidDay { .. } => "Day is out of range for the given month in this calendar",
        DateFromFieldsError::InvalidOrdinalMonth { .. } => "month is out of range for this calendar year",
        DateFromFieldsError::MonthCodeInvalidSyntax => "Invalid monthCode",
        DateFromFieldsError::MonthNotInCalendar => "monthCode is not valid for this calendar",
        DateFromFieldsError::MonthNotInYear => "monthCode does not exist in this calendar year",
        DateFromFieldsError::InvalidEra => "era is not valid for this calendar",
        DateFromFieldsError::InconsistentYear => "year is inconsistent with era and eraYear",
        DateFromFieldsError::InconsistentMonth => "month does not match monthCode",
        _ => "Date is not within representable range",
    })
}

/// `dateFromFields(calendarId, fields, overflow)` para um calendário não ISO (`NonISOResolveFields` e
/// `NonISODateToISO`): os campos ausentes são `TypeError` (`year` só dispensado por `era` com `eraYear` em
/// calendário com eras), o resto é do icu4x. O resultado é a data ISO equivalente com o calendário preservado.
pub fn date_from_calendar_fields(calendar_id: CalendarID, fields: &CalendarFieldsIn, overflow: TemporalOverflow) -> TemporalResult<ResolvedCalendarDate> {
    let kind = calendar_kind(calendar_id).expect("calendário não ISO tem AnyCalendarKind");
    let type_error = |message: &str| TemporalError { kind: TemporalErrorKind::TypeError, message: message.to_string() };
    let era_fields = calendar_has_eras(calendar_id).then(|| fields.era.as_deref().zip(fields.era_year)).flatten();
    if fields.year.is_none() && era_fields.is_none() {
        return Err(type_error("year property must be present"));
    }
    if fields.day.is_none() {
        return Err(type_error("day property must be present"));
    }
    if fields.month.is_none() && fields.month_code.is_none() {
        return Err(type_error("month or monthCode property must be present"));
    }

    let month_code_text = fields.month_code.map(|code| format!("M{:02}{}", code.month_number, if code.is_leap_month { "L" } else { "" }));
    let mut icu_fields = DateFields::default();
    icu_fields.extended_year = fields.year;
    icu_fields.era = era_fields.map(|(era, _)| era.as_bytes());
    icu_fields.era_year = era_fields.map(|(_, era_year)| era_year);
    icu_fields.ordinal_month = fields.month.map(|month| u8::try_from(month).unwrap_or(u8::MAX));
    icu_fields.month_code = month_code_text.as_deref().map(str::as_bytes);
    icu_fields.day = fields.day;
    let mut options = DateFromFieldsOptions::default();
    options.overflow = Some(match overflow {
        TemporalOverflow::Constrain => Overflow::Constrain,
        TemporalOverflow::Reject => Overflow::Reject,
    });

    let date = Date::try_from_fields(icu_fields, options, AnyCalendar::new(kind)).map_err(from_fields_error)?;
    Ok(ResolvedCalendarDate { iso_date: calendar_date_to_iso_date(&date), calendar_id })
}

/// Quantos anos do calendário a busca da data de referência de `PlainMonthDay` recua (cobre o `M05L` hebraico e os
/// meses bissextos raros do chinês).
const MONTH_DAY_REFERENCE_YEARS: i32 = 400;

/// A última data ISO até 1972-12-31 (a referência de `PlainMonthDay`) em que o mês `month_code` tem o dia `day` no
/// calendário `kind`; o erro do icu4x do último ano tentado quando nenhum serve.
fn find_month_day_reference(kind: AnyCalendarKind, month_code_text: &str, day: u8, start_year: i32) -> Result<PlainDate, DateFromFieldsError> {
    let limit = PlainDate::new(1972, 12, 31);
    let mut last_error = DateFromFieldsError::MonthNotInYear;
    for year in (start_year - MONTH_DAY_REFERENCE_YEARS..=start_year).rev() {
        let mut icu_fields = DateFields::default();
        icu_fields.extended_year = Some(year);
        icu_fields.month_code = Some(month_code_text.as_bytes());
        icu_fields.day = Some(day);
        let mut options = DateFromFieldsOptions::default();
        options.overflow = Some(Overflow::Reject);
        match Date::try_from_fields(icu_fields, options, AnyCalendar::new(kind)) {
            Ok(date) => {
                let iso_date = calendar_date_to_iso_date(&date);
                if iso_date_compare(iso_date, limit) <= 0 {
                    return Ok(iso_date);
                }
            }
            Err(error @ (DateFromFieldsError::MonthNotInCalendar | DateFromFieldsError::MonthCodeInvalidSyntax)) => return Err(error),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

/// `monthDayToISOReferenceDate(calendarId, fields, overflow)` (`NonISOResolveFields` e
/// `NonISOMonthDayToISOReferenceDate`) para um calendário não ISO: sem `monthCode`, o `month` precisa de `year` (ou
/// `era` e `eraYear`) para virar código; o resultado é a última data ISO até 1972-12-31 com esse mês e dia. Com
/// `constrain`, o dia que nenhum ano comporta desce até o maior que algum ano tem (medido no bun: `M12` dia 31 do
/// `islamic-civil` dá `1971-02-26`, o dia 30).
pub fn month_day_reference_date(calendar_id: CalendarID, fields: &CalendarFieldsIn, overflow: TemporalOverflow) -> TemporalResult<PlainDate> {
    let kind = calendar_kind(calendar_id).expect("calendário não ISO tem AnyCalendarKind");
    let Some(requested_day) = fields.day else {
        return Err(TemporalError { kind: TemporalErrorKind::TypeError, message: "day property must be present".to_string() });
    };
    if fields.month.is_none() && fields.month_code.is_none() {
        return Err(TemporalError { kind: TemporalErrorKind::TypeError, message: "month or monthCode property must be present".to_string() });
    }
    let (month_code_text, mut day) = match fields.month_code {
        Some(code) => (format!("M{:02}{}", code.month_number, if code.is_leap_month { "L" } else { "" }), requested_day),
        None => {
            let date = date_from_calendar_fields(calendar_id, fields, overflow)?;
            let resolved = calendar_fields(calendar_id, &date.iso_date);
            (resolved.month_code, resolved.day)
        }
    };
    let start_year = calendar_fields(calendar_id, &PlainDate::new(1972, 12, 31)).year;
    loop {
        match find_month_day_reference(kind, &month_code_text, day, start_year) {
            Ok(iso_date) => return Ok(iso_date),
            Err(_) if overflow == TemporalOverflow::Constrain && day > 1 => day -= 1,
            Err(error) => return Err(from_fields_error(error)),
        }
    }
}

/// `isoDateToFields(calendarId, isoDate, type)` para um calendário não ISO: `year` (aritmético), `monthCode` e `day`,
/// sem `era` nem `eraYear` (o `with` que traz `year` descarta o par, o que traz só o par descarta `year`).
pub fn calendar_iso_date_to_fields(calendar_id: CalendarID, iso_date: PlainDate, resolve_type: ResolveType) -> CalendarFieldsIn {
    let fields = calendar_fields(calendar_id, &iso_date);
    let mut result = CalendarFieldsIn::default();
    result.month_code = parse_month_code(&fields.month_code.encode_utf16().collect::<Vec<u16>>());
    if resolve_type != ResolveType::YearMonth {
        result.day = Some(fields.day);
    }
    if resolve_type != ResolveType::MonthDay {
        result.year = Some(fields.year);
    }
    result
}

/// O `DateDuration` do icu4x para a parte de datas de `duration` (semanas viram dias); `None` fora do `i32`, o
/// limite do `ucal_add` do C++ (`duration is out of the representable range`).
fn icu_date_duration(duration: &Duration) -> Option<DateDuration> {
    let total_days = duration.days().checked_add(duration.weeks().checked_mul(7)?)?;
    let magnitude = |value: i64| u32::try_from(value.unsigned_abs()).ok();
    Some(DateDuration {
        is_negative: [duration.years(), duration.months(), total_days].iter().any(|&value| value < 0),
        years: magnitude(duration.years())?,
        months: magnitude(duration.months())?,
        weeks: 0,
        days: magnitude(total_days)?,
    })
}

/// `calendarDateAdd(calendarId, isoDate, duration, overflow)` (`CalendarDateAdd`):
/// https://tc39.es/proposal-temporal/#sec-temporal-calendardateadd
/// O despacho único: ISO e calendários de estrutura gregoriana pela aritmética ISO; duração só de dias e semanas
/// é independente de calendário; o resto é do icu4x.
pub fn calendar_date_add(calendar_id: CalendarID, iso_date: PlainDate, duration: &Duration, overflow: TemporalOverflow) -> TemporalResult<PlainDate> {
    let Some(kind) = calendar_kind(calendar_id) else { return iso_date_add(iso_date, duration, overflow) };
    if calendar_uses_iso_arithmetic(kind)
        || (duration.years() == 0 && duration.months() == 0)
        || calendar_uses_iso_fallback_for_extreme_year(kind, iso_date.year())
    {
        return iso_date_add(iso_date, duration, overflow);
    }
    let icu_duration = icu_date_duration(duration).ok_or_else(|| range_error("duration is out of the representable range"))?;
    let date = iso_to_calendar_date(kind, &iso_date).ok_or_else(|| range_error(OUT_OF_RANGE))?;
    let mut options = DateAddOptions::default();
    options.overflow = Some(match overflow {
        TemporalOverflow::Constrain => Overflow::Constrain,
        TemporalOverflow::Reject => Overflow::Reject,
    });
    let added = date.try_added_with_options(icu_duration, options).map_err(|error| {
        range_error(match error {
            DateAddError::InvalidDay { .. } => "day is out of range for the resulting month (overflow: reject)",
            DateAddError::MonthNotInYear => "month code does not exist in the target year (overflow: reject)",
            _ => "Failed to perform ICU calendar arithmetic",
        })
    })?;
    let result = calendar_date_to_iso_date(&added);
    if !is_date_time_within_limits(result.year(), result.month(), result.day(), 12, 0, 0, 0, 0, 0) {
        return Err(range_error(OUT_OF_RANGE));
    }
    Ok(result)
}

/// `calendarDateUntil(calendarId, one, two, largestUnit)` (`CalendarDateUntil`):
/// https://tc39.es/proposal-temporal/#sec-temporal-calendardateuntil
/// O despacho único: dia e semana são independentes de calendário; ano e mês nos calendários não gregorianos são do
/// icu4x (`NonISODateUntil`).
pub fn calendar_date_until(calendar_id: CalendarID, one: PlainDate, two: PlainDate, largest_unit: TemporalUnit) -> TemporalResult<Duration> {
    let kind = calendar_kind(calendar_id);
    let unit_is_calendar_dependent = matches!(largest_unit, TemporalUnit::Year | TemporalUnit::Month);
    let Some(kind) = kind.filter(|&kind| {
        unit_is_calendar_dependent
            && !calendar_uses_iso_arithmetic(kind)
            && !calendar_uses_iso_fallback_for_extreme_year(kind, one.year())
            && !calendar_uses_iso_fallback_for_extreme_year(kind, two.year())
    }) else {
        return Ok(diff_iso_date(one, two, largest_unit));
    };
    if iso_date_compare(one, two) == 0 {
        return Ok(Duration::default());
    }
    let (first, second) = (iso_to_calendar_date(kind, &one), iso_to_calendar_date(kind, &two));
    let (Some(first), Some(second)) = (first, second) else { return Err(range_error(OUT_OF_RANGE)) };
    let mut options = DateDifferenceOptions::default();
    options.largest_unit = Some(if largest_unit == TemporalUnit::Year { DateDurationUnit::Years } else { DateDurationUnit::Months });
    let difference = first.try_until_with_options(&second, options).map_err(|_| range_error("Failed to perform ICU calendar arithmetic"))?;
    let sign: i64 = if difference.is_negative { -1 } else { 1 };
    Ok(Duration::new(
        sign * i64::from(difference.years),
        sign * i64::from(difference.months),
        sign * i64::from(difference.weeks),
        sign * i64::from(difference.days),
        0,
        0,
        0,
        0,
        0,
        0,
    ))
}

impl CalendarDateFields {
    /// O getter `era`: `undefined` nos calendários sem era (inclusive `iso8601`).
    pub fn era_value(&self, vm: &VM) -> JSValue {
        self.era.as_deref().map_or_else(js_undefined, |era| str_value(vm, era))
    }

    /// O getter `eraYear`: `undefined` nos calendários sem era.
    pub fn era_year_value(&self) -> JSValue {
        self.era_year.map_or_else(js_undefined, js_number)
    }
}
