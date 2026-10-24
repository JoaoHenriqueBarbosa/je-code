//! `Intl.DateTimeFormat` sem ICU (`IntlDateTimeFormat.cpp`, `IntlDateTimeFormatPrototype.cpp`,
//! `IntlDateTimeFormatConstructor.cpp`): a leitura das opções (`ToDateTimeOptions`), a resolução do
//! padrão de data e hora e o formatador, com os fusos pelo `jiff` (a tzdata embutida de
//! `ul_common::time::zone`, a mesma de `process_time_zone.rs`).
//!
//! LOCALES cobertas: os 65 de `intl_date_time_data` e `intl_calendar_patterns` (medidos no bun). O gregoriano
//! lê o padrão de `intl_calendar_patterns::pattern(locale, "gregory", chave)`, como os outros calendários, e
//! os nomes (mês, dia, era, AM/PM, período do dia) de `intl_date_time_data`; sem a chave na tabela valem os
//! padrões avulsos de `intl_date_time_data` e, por último, os de `en` e `pt` daqui (para as combinações de
//! campos de uso comum). Locale fora dos 65 cai nesse último caminho.
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê:
//!
//! - O espaço estreito U+202F que o ICU 72 põe antes de `AM`/`PM` vira espaço comum, como a
//!   `replaceNarrowNoBreakSpaceOrThinSpaceWithNormalSpace` do C++ faz em `format()`.
//! - `formatRange` e `formatRangeToParts` (`range.rs`) não têm o `DateIntervalFormat` do ICU: o maior
//!   campo diferente (era, ano, mês, dia, AM/PM, hora, minuto, segundo) escolhe o padrão de intervalo do
//!   CLDR de `en` e `pt` só para as combinações comuns (datas numéricas, com mês por extenso e
//!   `weekday`, `hour` e `hour`+`minute`); as outras caem no `intervalFormatFallback`
//!   (`{0} - {1}`), com as datas inteiras.
//! - Os objetos `Temporal` (`temporal.rs`: `HandleDateTimeValue`, os formatadores por tipo de
//!   `getTemporalFormatter` e o `toLocaleString` dos protótipos) formatam em `format`, `formatToParts`,
//!   `formatRange` e `formatRangeToParts`. A conferência de calendário compara o da célula com o do
//!   formatador (`calendarMatchesICU`): `PlainDate` e `PlainDateTime` com `iso8601` formatam com qualquer
//!   calendário; `PlainYearMonth` e `PlainMonthDay` exigem o mesmo calendário (como o C++).
//! - Calendários: `gregory`, `iso8601` (que o ICU formata como o gregoriano para a data civil) e os de
//!   `intl_calendar` (icu_calendar: buddhist, chinese, coptic, dangi, ethiopic, ethioaa, hebrew, indian,
//!   islamic, islamic-civil, islamic-tbla, islamic-umalqura, japanese, persian, roc), com a ordem dos campos do
//!   gregoriano do locale; o resto pedido cai em `gregory`. Sistema de numeração: o do `icu_decimal`
//!   (`numberingSystem`, `-u-nu-` e o padrão do locale), aplicado às partes numéricas.
//! - Os nomes de fuso (`timeZoneName`) vêm de uma tabela dos fusos de uso comum; o resto sai pelo
//!   deslocamento (`GMT-03:00`, `GMT-3`), que é o que o ICU escreve sem nome no CLDR.
//! - O ano anterior a 1 sai como o ano da era (1 - ano), sem o campo de era se ele não foi pedido.
//! - Um instante fora da faixa do `jiff` (anos de -9999 a 9999) usa o deslocamento UTC.

use jiff::tz::{Offset, TimeZone, TimeZoneDatabase};
use ul_common::time::Civil;

use crate::custom_getter;
use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_object::JS_NON_FINAL_OBJECT_S_INFO;
use crate::runtime::lookup::{HashTable, HashTableValue};
use crate::runtime::property_name::PropertyName;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::icu_number;
use crate::runtime::intl_calendar::{self, NativeCalendar, NativeDate};
use crate::runtime::intl_calendar_patterns;
use crate::runtime::intl_calendar_range;
use crate::runtime::intl_date_time_data;
use crate::runtime::intl_locale_data::{Language, ResolvedLocale};
use crate::runtime::intl_number_format::is_unicode_locale_identifier_type;
use crate::runtime::process_time_zone::zone_offset_at;
use crate::runtime::intl_support::{
    bound_function, call_instance, coerce_options_to_object, construct_instance, get_property, new_object, number_option, number_value,
    option_bool, option_enum, option_string, parts_array, put, intl_format_prototype_values, range_parts_array,
    read_locale_matcher, resolve_locale_from, str_value, to_number_checked, to_rust_string, with_instance, IntlClass,
    IntlEnum, RangePart,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::{js_boolean, JSValue};
use crate::wtf::date_math::time_clip;

crate::intl_enum!(TextWidth { Narrow => "narrow", Short => "short", Long => "long" });
crate::intl_enum!(Digits2 { TwoDigit => "2-digit", Numeric => "numeric" });
crate::intl_enum!(Month {
    TwoDigit => "2-digit", Numeric => "numeric", Narrow => "narrow", Short => "short", Long => "long"
});
crate::intl_enum!(StyleWidth { Full => "full", Long => "long", Medium => "medium", Short => "short" });
crate::intl_enum!(HourCycle { H11 => "h11", H12 => "h12", H23 => "h23", H24 => "h24" });
crate::intl_enum!(TimeZoneName {
    Short => "short", Long => "long", ShortOffset => "shortOffset", LongOffset => "longOffset",
    ShortGeneric => "shortGeneric", LongGeneric => "longGeneric"
});

/// `RequiredComponent` de `initializeDateTimeFormat`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Required {
    Any,
    Date,
    Time,
}

/// `Defaults` de `initializeDateTimeFormat`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Defaults {
    All,
    Date,
    Time,
    /// `Defaults::ZonedDateTime`: como `All`, mais `timeZoneName: "short"` quando nada foi pedido.
    ZonedDateTime,
}

/// Os campos pedidos (e, depois de [`resolve_fields`], os do padrão).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Fields {
    weekday: Option<TextWidth>,
    era: Option<TextWidth>,
    year: Option<Digits2>,
    month: Option<Month>,
    day: Option<Digits2>,
    day_period: Option<TextWidth>,
    hour: Option<Digits2>,
    minute: Option<Digits2>,
    second: Option<Digits2>,
    fractional_second_digits: u32,
    time_zone_name: Option<TimeZoneName>,
}

impl Fields {
    fn has_date(&self) -> bool {
        self.era.is_some() || self.has_date_without_era()
    }

    /// `weekday`, `year`, `month` ou `day`: o que o `needDefaults` olha (a era sozinha não conta).
    fn has_date_without_era(&self) -> bool {
        self.weekday.is_some() || self.year.is_some() || self.month.is_some() || self.day.is_some()
    }

    fn has_time(&self) -> bool {
        self.day_period.is_some()
            || self.hour.is_some()
            || self.minute.is_some()
            || self.second.is_some()
            || self.fractional_second_digits > 0
    }

    fn any(&self) -> bool {
        self.has_date() || self.has_time() || self.time_zone_name.is_some()
    }
}

/// O estado de um `IntlDateTimeFormat`.
#[derive(Clone)]
struct DateTimeFormatState {
    locale: String,
    language: Language,
    zone: TimeZone,
    /// O `timeZone` do `resolvedOptions` (`America/Sao_Paulo`, `UTC`, `+03:00`).
    zone_name: String,
    hour_cycle: Option<HourCycle>,
    fields: Fields,
    date_style: Option<StyleWidth>,
    time_style: Option<StyleWidth>,
    /// O `m_userSkeleton`: os campos pedidos antes dos padrões (`era` e `timeZoneName` incluídos).
    user_fields: Fields,
    /// `m_anyPresent`: algum de `weekday`, `year`, `month`, `day`, `dayPeriod`, `hour`, `minute`,
    /// `second` ou `fractionalSecondDigits` foi pedido.
    any_present: bool,
    /// O ciclo de horas resolvido (existe mesmo sem `hour`, para os formatadores de `Temporal`).
    cycle: HourCycle,
    /// O calendário é `iso8601` (senão `gregory`).
    iso_calendar: bool,
    /// O calendário não gregoriano convertido pelo `icu_calendar` (o padrão é o gregoriano).
    native: NativeCalendar,
    /// O `numberingSystem` resolvido (vazio: `latn`).
    numbering: String,
}

// ---------------------------------------------------------------------------------------------
// Nomes
// ---------------------------------------------------------------------------------------------

const EN_MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const EN_MONTHS_SHORT: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const EN_WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const EN_WEEKDAYS_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const EN_WEEKDAYS_NARROW: [&str; 7] = ["S", "M", "T", "W", "T", "F", "S"];
const PT_MONTHS: [&str; 12] = [
    "janeiro", "fevereiro", "mar\u{e7}o", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro",
];
const PT_MONTHS_SHORT: [&str; 12] =
    ["jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.", "dez."];
const PT_WEEKDAYS: [&str; 7] = [
    "domingo",
    "segunda-feira",
    "ter\u{e7}a-feira",
    "quarta-feira",
    "quinta-feira",
    "sexta-feira",
    "s\u{e1}bado",
];
const PT_WEEKDAYS_SHORT: [&str; 7] = ["dom.", "seg.", "ter.", "qua.", "qui.", "sex.", "s\u{e1}b."];
const PT_WEEKDAYS_NARROW: [&str; 7] = ["D", "S", "T", "Q", "Q", "S", "S"];
const MONTHS_NARROW: [&str; 12] = ["J", "F", "M", "A", "M", "J", "J", "A", "S", "O", "N", "D"];

fn month_name(language: Language, width: TextWidth, month0: usize) -> &'static str {
    match (language, width) {
        (_, TextWidth::Narrow) => MONTHS_NARROW[month0],
        (Language::English, TextWidth::Short) => EN_MONTHS_SHORT[month0],
        (Language::English, TextWidth::Long) => EN_MONTHS[month0],
        (Language::Portuguese, TextWidth::Short) => PT_MONTHS_SHORT[month0],
        (Language::Portuguese, TextWidth::Long) => PT_MONTHS[month0],
    }
}

fn weekday_name(language: Language, width: TextWidth, weekday: usize) -> &'static str {
    match (language, width) {
        (Language::English, TextWidth::Narrow) => EN_WEEKDAYS_NARROW[weekday],
        (Language::English, TextWidth::Short) => EN_WEEKDAYS_SHORT[weekday],
        (Language::English, TextWidth::Long) => EN_WEEKDAYS[weekday],
        (Language::Portuguese, TextWidth::Narrow) => PT_WEEKDAYS_NARROW[weekday],
        (Language::Portuguese, TextWidth::Short) => PT_WEEKDAYS_SHORT[weekday],
        (Language::Portuguese, TextWidth::Long) => PT_WEEKDAYS[weekday],
    }
}

fn era_name(language: Language, width: TextWidth, before_common_era: bool) -> &'static str {
    match (language, width, before_common_era) {
        (Language::English, TextWidth::Narrow, false) => "A",
        (Language::English, TextWidth::Narrow, true) => "B",
        (Language::English, TextWidth::Short, false) => "AD",
        (Language::English, TextWidth::Short, true) => "BC",
        (Language::English, TextWidth::Long, false) => "Anno Domini",
        (Language::English, TextWidth::Long, true) => "Before Christ",
        (Language::Portuguese, TextWidth::Long, false) => "depois de Cristo",
        (Language::Portuguese, TextWidth::Long, true) => "antes de Cristo",
        (Language::Portuguese, _, false) => "d.C.",
        (Language::Portuguese, _, true) => "a.C.",
    }
}

/// `dayPeriod` (`B`): o período do dia flexível do CLDR.
fn day_period_name(language: Language, width: TextWidth, hour: i64, minute: i64, second: i64) -> &'static str {
    let exact_noon = hour == 12 && minute == 0 && second == 0;
    match language {
        Language::English => {
            if exact_noon {
                return "noon";
            }
            match hour {
                6..=11 => "in the morning",
                12..=17 => "in the afternoon",
                18..=20 => "in the evening",
                _ => "at night",
            }
        }
        Language::Portuguese => {
            if exact_noon {
                return if width == TextWidth::Narrow { "m" } else { "meio-dia" };
            }
            match hour {
                0..=5 => "da madrugada",
                6..=11 => "da manh\u{e3}",
                12..=18 => "da tarde",
                _ => "da noite",
            }
        }
    }
}

/// `GMT-3`, `GMT+5:30`, `GMT+0` (o deslocamento zero também leva o sinal, como no ICU do bun): o formato GMT do
/// locale quando há dados dele (`غرينتش+5:30`, `UTC−3`, `GMT +5:30`), senão o do `en`.
fn short_offset_name(state: &DateTimeFormatState, offset_seconds: i32) -> String {
    localized_offset_name(state, false, offset_seconds).unwrap_or_else(|| {
        let sign = if offset_seconds < 0 { '-' } else { '+' };
        let total = offset_seconds.unsigned_abs();
        let (hours, minutes) = (total / 3600, total % 3600 / 60);
        if minutes == 0 { format!("GMT{sign}{hours}") } else { format!("GMT{sign}{hours}:{minutes:02}") }
    })
}

/// `GMT-03:00`, `GMT+00:00`: o `longOffset`, no formato GMT do locale quando há dados dele.
fn long_offset_name(state: &DateTimeFormatState, offset_seconds: i32) -> String {
    localized_offset_name(state, true, offset_seconds).unwrap_or_else(|| {
        let sign = if offset_seconds < 0 { '-' } else { '+' };
        let total = offset_seconds.unsigned_abs();
        format!("GMT{sign}{:02}:{:02}", total / 3600, total % 3600 / 60)
    })
}

/// O formato GMT localizado de `intl_date_time_data` (dígitos ASCII; `parts_with_fields` os converte para o
/// `numberingSystem` do formatador, como o ICU faz com `غرينتش+١٣`).
fn localized_offset_name(state: &DateTimeFormatState, long: bool, offset_seconds: i32) -> Option<String> {
    intl_date_time_data::locale_data_for(state.locale.split("-u-").next().unwrap_or(""))?.gmt_offset_name(long, offset_seconds)
}


/// O texto do `timeZoneName` para o fuso e o instante.
fn time_zone_name_text(
    state: &DateTimeFormatState,
    kind: TimeZoneName,
    offset_seconds: i32,
    is_dst: bool,
) -> String {
    let iana = state.zone.iana_name().or(if state.zone_name == "UTC" { Some("UTC") } else { None });
    let base_locale = state.locale.split("-u-").next().unwrap_or("");
    let data = intl_date_time_data::locale_data_for(base_locale)
        .or_else(|| intl_date_time_data::locale_data_for(if state.language == Language::Portuguese { "pt" } else { "en" }));
    // A tabela do locale só guarda o nome que difere do GMT localizado (`tz|fuso|estilo|w` e `s`).
    if let Some(name) = iana.zip(data).and_then(|(iana, data)| data.zone_name(iana, kind.as_str(), is_dst)) {
        return name.to_string();
    }
    match kind {
        TimeZoneName::ShortOffset => short_offset_name(state, offset_seconds),
        TimeZoneName::Long | TimeZoneName::LongGeneric | TimeZoneName::LongOffset => long_offset_name(state, offset_seconds),
        // O deslocamento zero sem nome próprio é o `Etc/GMT`: `GMT` em en e es, `GMT+0` em de (medido por locale).
        TimeZoneName::Short | TimeZoneName::ShortGeneric if offset_seconds == 0 => data
            .and_then(|data| data.gmt_zero_short_name())
            .map_or_else(|| "GMT".to_string(), str::to_string),
        TimeZoneName::Short | TimeZoneName::ShortGeneric => short_offset_name(state, offset_seconds),
    }
}

// ---------------------------------------------------------------------------------------------
// Padrão
// ---------------------------------------------------------------------------------------------

/// Os campos de `dateStyle` e `timeStyle`.
fn fields_for_styles(language: Language, date_style: Option<StyleWidth>, time_style: Option<StyleWidth>) -> Fields {
    let mut fields = Fields::default();
    match date_style {
        Some(StyleWidth::Full) => {
            fields.weekday = Some(TextWidth::Long);
            fields.month = Some(Month::Long);
            fields.day = Some(Digits2::Numeric);
            fields.year = Some(Digits2::Numeric);
        }
        Some(StyleWidth::Long) => {
            fields.month = Some(Month::Long);
            fields.day = Some(Digits2::Numeric);
            fields.year = Some(Digits2::Numeric);
        }
        Some(StyleWidth::Medium) => {
            fields.month = Some(Month::Short);
            fields.day = Some(Digits2::Numeric);
            fields.year = Some(Digits2::Numeric);
        }
        Some(StyleWidth::Short) => {
            fields.month = Some(Month::Numeric);
            fields.day = Some(Digits2::Numeric);
            fields.year = Some(if language == Language::English { Digits2::TwoDigit } else { Digits2::Numeric });
        }
        None => {}
    }
    match time_style {
        Some(width) => {
            fields.hour = Some(Digits2::Numeric);
            fields.minute = Some(Digits2::TwoDigit);
            if width != StyleWidth::Short {
                fields.second = Some(Digits2::TwoDigit);
            }
            match width {
                StyleWidth::Full => fields.time_zone_name = Some(TimeZoneName::Long),
                StyleWidth::Long => fields.time_zone_name = Some(TimeZoneName::Short),
                _ => {}
            }
        }
        None => {}
    }
    fields
}

/// O que o `DateTimePatternGenerator` faz com os campos pedidos: minutos e segundos de dois dígitos
/// quando acompanham outro campo de tempo, relógio de 24 horas com dois dígitos, e o português com o
/// dia e o mês numéricos de dois dígitos.
fn resolve_fields(language: Language, cycle: HourCycle, mut fields: Fields) -> Fields {
    if fields.hour.is_some() && matches!(cycle, HourCycle::H23 | HourCycle::H24) {
        fields.hour = Some(Digits2::TwoDigit);
    }
    if fields.minute.is_some() && (fields.hour.is_some() || fields.second.is_some()) {
        fields.minute = Some(Digits2::TwoDigit);
    }
    if fields.second.is_some() && fields.minute.is_some() {
        fields.second = Some(Digits2::TwoDigit);
    }
    if language == Language::Portuguese {
        let numeric_month = matches!(fields.month, Some(Month::Numeric | Month::TwoDigit));
        if numeric_month && (fields.day.is_some() || fields.year.is_some()) {
            fields.month = Some(Month::TwoDigit);
        }
        if numeric_month && fields.day.is_some() {
            fields.day = Some(Digits2::TwoDigit);
        }
    }
    fields
}

// ---------------------------------------------------------------------------------------------
// Formatação
// ---------------------------------------------------------------------------------------------

type Part = (String, String);

fn literal(text: &str) -> Part {
    ("literal".to_string(), text.to_string())
}

fn two_digits(value: i64) -> String {
    format!("{value:02}")
}

fn digits(value: i64, width: Digits2) -> String {
    match width {
        Digits2::TwoDigit => two_digits(value),
        Digits2::Numeric => value.to_string(),
    }
}

/// A data civil e o fuso do instante.
struct Moment {
    civil: Civil,
    milliseconds: i64,
    offset_seconds: i32,
    is_dst: bool,
}

/// O deslocamento do fuso no instante, com as regras do ICU para fora da faixa do `jiff` (as mesmas do `Date`).
fn zone_offset(zone: &TimeZone, epoch_milliseconds: i64) -> (i32, bool) {
    zone_offset_at(zone, epoch_milliseconds as f64).unwrap_or((0, false))
}

/// O horário de verão do ponto de vista do ICU, que escolhe a coluna `w` ou `s` dos nomes de fuso. O tzdb do
/// sistema usa DST negativo (Europe/Dublin: o GMT de inverno é `isdst=1` e o IST de verão é `isdst=0`); o ICU
/// trata o deslocamento maior como o de verão. Se o instante com `isdst` oposto, a meio ano de distância, tem
/// deslocamento maior que o do instante marcado como DST, o DST do tzdb é negativo e a marca se inverte.
fn icu_is_dst(zone: &TimeZone, epoch_milliseconds: i64, offset_seconds: i32, is_dst: bool) -> bool {
    const HALF_YEAR_MILLISECONDS: i64 = 182 * 86_400_000;
    let opposite = [-HALF_YEAR_MILLISECONDS, HALF_YEAR_MILLISECONDS]
        .into_iter()
        .map(|delta| zone_offset(zone, epoch_milliseconds + delta))
        .find(|&(_, other_dst)| other_dst != is_dst);
    match opposite {
        Some((other_offset, _)) => {
            let (dst_offset, standard_offset) = if is_dst { (offset_seconds, other_offset) } else { (other_offset, offset_seconds) };
            if dst_offset < standard_offset { !is_dst } else { is_dst }
        }
        None => is_dst,
    }
}

fn moment_at(zone: &TimeZone, epoch_milliseconds: i64) -> Moment {
    let (offset_seconds, jiff_dst) = zone_offset(zone, epoch_milliseconds);
    let is_dst = icu_is_dst(zone, epoch_milliseconds, offset_seconds, jiff_dst);
    let local_seconds = epoch_milliseconds.div_euclid(1000) + i64::from(offset_seconds);
    Moment {
        civil: Civil::from_secs(local_seconds),
        milliseconds: epoch_milliseconds.rem_euclid(1000),
        offset_seconds,
        is_dst,
    }
}

/// As partes da data (gregoriana).
fn date_parts(language: Language, fields: &Fields, moment: &Moment) -> Vec<Part> {
    let civil = &moment.civil;
    let before_common_era = civil.year < 1;
    let year_value = if before_common_era { 1 - civil.year } else { civil.year };
    let day_value = civil.mday;
    let year_type = "year";
    let year_text = |width: Digits2| -> String {
        match width {
            Digits2::TwoDigit => two_digits(year_value.rem_euclid(100)),
            Digits2::Numeric => year_value.to_string(),
        }
    };
    let month_part = |width: Month| -> String {
        match width {
            Month::Narrow => month_name(language, TextWidth::Narrow, civil.mon as usize - 1).to_string(),
            Month::Short => month_name(language, TextWidth::Short, civil.mon as usize - 1).to_string(),
            Month::Long => month_name(language, TextWidth::Long, civil.mon as usize - 1).to_string(),
            Month::TwoDigit => two_digits(civil.mon),
            Month::Numeric => civil.mon.to_string(),
        }
    };

    let mut parts: Vec<Part> = Vec::new();
    let others = fields.era.is_some() || fields.year.is_some() || fields.month.is_some() || fields.day.is_some();
    if let Some(width) = fields.weekday {
        parts.push(("weekday".to_string(), weekday_name(language, width, civil.wday as usize).to_string()));
        if others {
            parts.push(literal(", "));
        }
    }

    let text_month = matches!(fields.month, Some(Month::Narrow | Month::Short | Month::Long));
    if text_month {
        let month = fields.month.expect("mês de texto");
        match language {
            Language::English => {
                parts.push(("month".to_string(), month_part(month)));
                if let Some(width) = fields.day {
                    parts.push(literal(" "));
                    parts.push(("day".to_string(), digits(day_value, width)));
                }
                if let Some(width) = fields.year {
                    parts.push(literal(if fields.day.is_some() { ", " } else { " " }));
                    parts.push((year_type.to_string(), year_text(width)));
                }
            }
            Language::Portuguese => {
                if let Some(width) = fields.day {
                    parts.push(("day".to_string(), digits(day_value, width)));
                    parts.push(literal(" de "));
                }
                parts.push(("month".to_string(), month_part(month)));
                if let Some(width) = fields.year {
                    parts.push(literal(" de "));
                    parts.push((year_type.to_string(), year_text(width)));
                }
            }
        }
    } else {
        let mut numeric: Vec<Part> = Vec::new();
        let day = fields.day.map(|width| ("day".to_string(), digits(day_value, width)));
        let month = fields.month.map(|width| ("month".to_string(), month_part(width)));
        let year = fields.year.map(|width| (year_type.to_string(), year_text(width)));
        let ordered = match language {
            Language::English => [month, day, year],
            Language::Portuguese => [day, month, year],
        };
        for part in ordered.into_iter().flatten() {
            if !numeric.is_empty() {
                numeric.push(literal("/"));
            }
            numeric.push(part);
        }
        parts.extend(numeric);
    }

    if let Some(width) = fields.era {
        if parts.last().is_some_and(|part| part.0 != "literal") {
            parts.push(literal(" "));
        }
        parts.push(("era".to_string(), era_name(language, width, before_common_era).to_string()));
    }
    parts
}

/// O mês estreito do `iso8601` por locale (medido no bun): `ja` e `zh` dão o número, `ko` o número com `월`,
/// `ar` a letra árabe, `it` e `es` as letras próprias; o resto usa a inicial inglesa.
fn iso_narrow_month(locale: &str, language: Language, width: TextWidth, month: u32) -> String {
    const IT: [&str; 12] = ["G", "F", "M", "A", "M", "G", "L", "A", "S", "O", "N", "D"];
    const AR: [&str; 12] = [
        "\u{64a}", "\u{641}", "\u{645}", "\u{623}", "\u{648}", "\u{646}", "\u{644}", "\u{63a}", "\u{633}", "\u{643}", "\u{628}", "\u{62f}",
    ];
    let index = month as usize - 1;
    let tag = locale.split('-').next().unwrap_or("");
    match tag {
        "ja" | "zh" => month.to_string(),
        "ko" => format!("{month}\u{c6d4}"),
        "ar" => AR[index].to_string(),
        "it" => IT[index].to_string(),
        "es" if month == 1 => "E".to_string(),
        // O resto é o nome estreito isolado do locale (medido no bun nos 65 locales); sem dados vale a inicial inglesa.
        _ => match intl_date_time_data::locale_data_for(locale) {
            Some(data) => data.standalone_month_name("narrow", index).to_string(),
            None => month_name(language, width, index).to_string(),
        },
    }
}

/// A era com um único campo de data, `month` ou `day`, sem ano nem dia da semana, no calendário `iso8601` (medido no bun
/// em 65 locales). A era não tem nome (o texto vazio deixa só o espaço do padrão `G ...`) e o ICU monta o resto com o
/// `appendItems` da raiz: o campo vem como `" (rótulo: valor)"` ou, sem rótulo, como `" valor"`. O rótulo aparece conforme a
/// largura da era e a do campo, igual em todo locale: era `short` sempre; era `narrow` só com o mês `narrow`; era `long` com
/// o mês de dois dígitos, `narrow` e `long` e com o dia de dois dígitos. O mês `long` é vazio (`" (month: )"` ou `" "`) e o
/// `short` é o nome isolado do locale. Dentro de um intervalo entre meses diferentes o rótulo vale para todo mês que tem
/// texto (`force_label`), medido em `formatRange`. `None` fora desse molde.
fn iso_era_field_parts(state: &DateTimeFormatState, fields: &Fields, moment: &Moment, force_label: bool) -> Option<Vec<Part>> {
    let era = fields.era?;
    if state.date_style.is_some() || fields.weekday.is_some() || fields.year.is_some() || fields.month.is_some() == fields.day.is_some() {
        return None;
    }
    let (locale, language) = (state.locale.as_str(), state.language);
    let civil = &moment.civil;
    let (month_label, day_label) = crate::runtime::intl_iso_field_labels_data::labels(locale);
    let (kind, label, text, labeled) = match (fields.month, fields.day) {
        (Some(month), _) => {
            let text = match month {
                Month::Numeric => civil.mon.to_string(),
                Month::TwoDigit => two_digits(civil.mon),
                Month::Narrow => iso_narrow_month(locale, language, TextWidth::Narrow, civil.mon as u32),
                Month::Short => match crate::runtime::intl_iso_field_labels_data::short_month_override(locale, civil.mon as usize - 1) {
                    Some(name) => name.to_string(),
                    None => match intl_date_time_data::locale_data_for(locale) {
                        Some(data) => data.standalone_month_name("short", civil.mon as usize - 1).to_string(),
                        None => month_name(language, TextWidth::Short, civil.mon as usize - 1).to_string(),
                    },
                },
                Month::Long => String::new(),
            };
            let labeled = match era {
                TextWidth::Short => true,
                TextWidth::Narrow => month == Month::Narrow,
                TextWidth::Long => matches!(month, Month::TwoDigit | Month::Narrow | Month::Long),
            };
            ("month", month_label, text, labeled)
        }
        (None, Some(day)) => {
            let labeled = match era {
                TextWidth::Short => true,
                TextWidth::Narrow => false,
                TextWidth::Long => day == Digits2::TwoDigit,
            };
            ("day", day_label, digits(civil.mday, day), labeled)
        }
        (None, None) => return None,
    };
    let labeled = labeled || (force_label && !text.is_empty());
    let mut parts: Vec<Part> = Vec::new();
    match (labeled, text.is_empty()) {
        (true, true) => parts.push(literal(&format!(" ({label}: )"))),
        (true, false) => {
            parts.push(literal(&format!(" ({label}: ")));
            parts.push((kind.to_string(), text));
            parts.push(literal(")"));
        }
        (false, true) => parts.push(literal(" ")),
        (false, false) => {
            parts.push(literal(" "));
            parts.push((kind.to_string(), text));
        }
    }
    Some(parts)
}

/// As partes da data no calendário `iso8601`: os padrões do CLDR para `iso8601` são os mesmos em
/// qualquer locale (`y-MM`, `MM-dd`, `y-MM-dd`), medidos no bun (`temporal_locale_bun.tsv`). Com mês
/// por extenso ou abreviado o ICU perde o nome do mês e sobra o resto do padrão (`"2024 "`, `" 5"`);
/// o mês estreito aparece, por locale (`iso8601`: `iso_narrow_month`). O dia da semana vem depois da data
/// (`2024-01-05, Fri`, `2024  5, Friday`, `2024 Friday` sem dia; o nome é o do locale) e a era, que no `iso8601`
/// não tem nome (o ICU deixa o texto vazio), deixa só o espaço do padrão `G y...` à frente da data (`" 2024-01-05"`),
/// medidos no bun em `en-US`, `pt-BR`, `de`, `ja`, `fr` e `ko`. Sem medida no bun: mês de texto junto de outro campo
/// (ano primeiro, depois o mês, depois o dia, separados por espaço).
fn iso_date_parts(state: &DateTimeFormatState, fields: &Fields, moment: &Moment) -> Vec<Part> {
    if let Some(parts) = iso_era_field_parts(state, fields, moment, false) {
        return parts;
    }
    let (locale, language) = (state.locale.as_str(), state.language);
    let civil = &moment.civil;
    let year = fields.year.map(|width| {
        // O padrão `y-MM-dd` do `dateStyle: 'short'` não tem o ano de dois dígitos do gregoriano em inglês.
        let width = if state.date_style.is_some() { Digits2::Numeric } else { width };
        let text = match width {
            Digits2::TwoDigit => two_digits(civil.year.rem_euclid(100)),
            Digits2::Numeric => civil.year.to_string(),
        };
        ("year".to_string(), text)
    });
    let day = fields.day.map(|width| {
        // O padrão `MM-dd` tem o dia de dois dígitos; com o mês por extenso o dia segue o pedido (`" 5"`, medido no bun).
        let width = if matches!(fields.month, Some(Month::Numeric | Month::TwoDigit)) { Digits2::TwoDigit } else { width };
        ("day".to_string(), digits(civil.mday, width))
    });
    let text_width = match fields.month {
        Some(Month::Narrow) => Some(TextWidth::Narrow),
        Some(Month::Short) => Some(TextWidth::Short),
        Some(Month::Long) => Some(TextWidth::Long),
        _ => None,
    };
    let (month, separator) = match (fields.month, text_width) {
        (None, _) => (None, "-"),
        (Some(_), None) => (Some(("month".to_string(), two_digits(civil.mon))), "-"),
        // Medido no bun: no `iso8601` o ICU não tem nome de mês longo nem abreviado, e o mês some do resultado.
        (Some(_), Some(TextWidth::Long | TextWidth::Short)) => (Some(("month".to_string(), String::new())), " "),
        (Some(_), Some(width)) => (Some(("month".to_string(), iso_narrow_month(locale, language, width, civil.mon as u32))), " "),
    };
    let mut parts: Vec<Part> = Vec::new();
    for (index, part) in [year, month, day].into_iter().flatten().enumerate() {
        if index > 0 {
            parts.push(literal(separator));
        }
        if !(part.0 == "month" && part.1.is_empty()) {
            parts.push(part);
        }
    }
    if let Some(width) = fields.weekday {
        if !parts.is_empty() {
            parts.push(literal(if fields.day.is_some() { ", " } else { " " }));
        }
        let name = match intl_date_time_data::locale_data_for(locale) {
            Some(data) => data.weekday_name(width.as_str(), civil.wday as usize, false),
            None => weekday_name(language, width, civil.wday as usize),
        };
        parts.push(("weekday".to_string(), name.to_string()));
    }
    if fields.era.is_some() && !parts.is_empty() {
        parts.insert(0, literal(" "));
    }
    parts
}

/// A junção entre a data e a hora no `iso8601`, escolhida como o `DateTimePatternGenerator` faz: a longa com o mês
/// por extenso, a média com o abreviado, a curta nos demais. Medido no bun: `, ` em `en`, `pt` e `de` (a longa é ` at `,
/// ` às `, ` um `), ` ` em `fr` (a longa ` à `, a média `, `), ` ` em `ja` e `ko` em todas. Os outros locales seguem
/// `date_time_joiner`.
fn iso_date_time_joiner(state: &DateTimeFormatState, fields: &Fields) -> Part {
    let tag = state.locale.split(['-', '_']).next().unwrap_or("");
    let (long, medium) = (fields.month == Some(Month::Long), fields.month == Some(Month::Short));
    literal(match (tag, long, medium) {
        ("en", true, _) => " at ",
        ("pt", true, _) => " \u{e0}s ",
        ("de", true, _) => " um ",
        ("fr", true, _) => " \u{e0} ",
        ("ja" | "ko", _, _) | ("fr", false, false) => " ",
        ("en" | "pt" | "de" | "fr", _, _) => ", ",
        _ => return date_time_joiner(state, fields),
    })
}

/// A data, a junção e a hora do `iso8601`. A junção existe sempre que há campo de data, até quando a data sai vazia
/// (só a era, sem nome: `", 3:04 PM"` em `en`).
fn iso_calendar_parts(state: &DateTimeFormatState, fields: &Fields, moment: &Moment) -> Vec<Part> {
    let mut parts = iso_date_parts(state, fields, moment);
    let time = time_parts(state, fields, moment);
    if fields.has_date() && !time.is_empty() {
        parts.push(iso_date_time_joiner(state, fields));
    }
    parts.extend(time);
    parts
}

/// As partes da hora.
fn time_parts(state: &DateTimeFormatState, fields: &Fields, moment: &Moment) -> Vec<Part> {
    let civil = &moment.civil;
    let cycle = state.hour_cycle.unwrap_or(HourCycle::H23);
    let twelve = matches!(cycle, HourCycle::H11 | HourCycle::H12);
    let mut parts: Vec<Part> = Vec::new();
    let colon = |parts: &mut Vec<Part>| {
        if !parts.is_empty() {
            parts.push(literal(":"));
        }
    };

    if let Some(width) = fields.hour {
        let value = match cycle {
            HourCycle::H12 => if civil.hour % 12 == 0 { 12 } else { civil.hour % 12 },
            HourCycle::H11 => civil.hour % 12,
            HourCycle::H23 => civil.hour,
            HourCycle::H24 => if civil.hour == 0 { 24 } else { civil.hour },
        };
        parts.push(("hour".to_string(), digits(value, width)));
    }
    if let Some(width) = fields.minute {
        colon(&mut parts);
        parts.push(("minute".to_string(), digits(civil.min, width)));
    }
    if let Some(width) = fields.second {
        colon(&mut parts);
        parts.push(("second".to_string(), digits(civil.sec, width)));
    }
    if fields.fractional_second_digits > 0 {
        let fraction = format!("{:03}", moment.milliseconds);
        let fraction = &fraction[..fields.fractional_second_digits as usize];
        if fields.second.is_some() {
            parts.push(literal("."));
        }
        parts.push(("fractionalSecond".to_string(), fraction.to_string()));
    }

    // No ciclo de 24 horas o `dayPeriod` não aparece (pt-BR, fr, de e ja com `hour` e `dayPeriod: "long"` dão só `15:04`).
    if let Some(width) = fields.day_period.filter(|_| twelve || fields.hour.is_none()) {
        if !parts.is_empty() {
            parts.push(literal(" "));
        }
        parts.push(("dayPeriod".to_string(), day_period_name(state.language, width, civil.hour, civil.min, civil.sec).to_string()));
    } else if twelve && fields.hour.is_some() {
        parts.push(literal(" "));
        // O AM e o PM do locale (`午後`, `오후`) quando há dados; `en`, `pt`, `de` e `fr` dão `AM` e `PM`.
        let symbol = match intl_date_time_data::locale_data_for(&state.locale) {
            Some(data) => data.token("dayPeriod", "", false, civil, moment.milliseconds, cycle.as_str(), &|_| String::new()).1,
            None => (if civil.hour < 12 { "AM" } else { "PM" }).to_string(),
        };
        parts.push(("dayPeriod".to_string(), symbol));
    }

    if let Some(kind) = fields.time_zone_name {
        if !parts.is_empty() {
            parts.push(literal(" "));
        }
        let name = time_zone_name_text(state, kind, moment.offset_seconds, moment.is_dst);
        parts.push(("timeZoneName".to_string(), name));
    }
    parts
}

/// Acrescenta `campo=valor` à chave de skeleton (o texto que `scripts/gen-datetime-data.js` monta).
fn push_key(key: &mut String, name: &str, value: &str) {
    if !key.is_empty() {
        key.push(';');
    }
    key.push_str(name);
    key.push('=');
    key.push_str(value);
}

/// A chave de skeleton do formatador em `intl_date_time_data`: `dateStyle` e `timeStyle`, ou os campos
/// pedidos (os que o `Defaults` acrescentou entram como `numeric`). `None` quando o `dayPeriod` ou a
/// fração de segundo vieram de um padrão e não do pedido do usuário.
fn data_key(state: &DateTimeFormatState) -> Option<String> {
    let mut key = String::new();
    let has_hour = if state.date_style.is_some() || state.time_style.is_some() {
        if let Some(style) = state.date_style {
            push_key(&mut key, "dateStyle", style.as_str());
        }
        if let Some(style) = state.time_style {
            push_key(&mut key, "timeStyle", style.as_str());
        }
        state.time_style.is_some()
    } else {
        let (user, resolved) = (&state.user_fields, &state.fields);
        if (resolved.day_period.is_some() && user.day_period.is_none()) || resolved.fractional_second_digits != user.fractional_second_digits {
            return None;
        }
        let numeric = |requested: Option<Digits2>, wanted: Option<Digits2>| requested.or(wanted.map(|_| Digits2::Numeric));
        if let Some(width) = user.weekday.or(resolved.weekday) {
            push_key(&mut key, "weekday", width.as_str());
        }
        if let Some(width) = user.era.or(resolved.era) {
            push_key(&mut key, "era", width.as_str());
        }
        if let Some(width) = numeric(user.year, resolved.year) {
            push_key(&mut key, "year", width.as_str());
        }
        if let Some(width) = user.month.or(resolved.month.map(|_| Month::Numeric)) {
            push_key(&mut key, "month", width.as_str());
        }
        if let Some(width) = numeric(user.day, resolved.day) {
            push_key(&mut key, "day", width.as_str());
        }
        if let Some(width) = user.day_period.filter(|_| resolved.hour.is_none() || matches!(state.cycle, HourCycle::H11 | HourCycle::H12)) {
            push_key(&mut key, "dayPeriod", width.as_str());
        }
        for (name, requested, wanted) in
            [("hour", user.hour, resolved.hour), ("minute", user.minute, resolved.minute), ("second", user.second, resolved.second)]
        {
            if let Some(width) = numeric(requested, wanted) {
                push_key(&mut key, name, width.as_str());
            }
            if name == "second" && user.fractional_second_digits > 0 {
                push_key(&mut key, "fractionalSecondDigits", &user.fractional_second_digits.to_string());
            }
        }
        if let Some(kind) = user.time_zone_name.or(resolved.time_zone_name) {
            push_key(&mut key, "timeZoneName", kind.as_str());
        }
        resolved.hour.is_some()
    };
    if has_hour {
        key.push_str(if matches!(state.cycle, HourCycle::H11 | HourCycle::H12) { "|12" } else { "|24" });
    }
    Some(key)
}

/// As partes pelos padrões medidos no bun (`intl_date_time_data`), para as línguas que os têm.
fn locale_data_parts(state: &DateTimeFormatState, moment: &Moment) -> Option<Vec<Part>> {
    let data = intl_date_time_data::locale_data_for(&state.locale)?;
    let pattern = data.pattern(&data_key(state)?)?;
    let zone = |style: &str| zone_style_text(state, moment, style);
    Some(data.render(pattern, &moment.civil, moment.milliseconds, state.cycle.as_str(), &zone))
}

/// O nome do fuso no estilo `style` (`short`, `long`, `shortOffset`...), da tabela do locale ou calculado do deslocamento.
fn zone_style_text(state: &DateTimeFormatState, moment: &Moment, style: &str) -> String {
    let kind = match style {
        "long" => TimeZoneName::Long,
        "shortOffset" => TimeZoneName::ShortOffset,
        "longOffset" => TimeZoneName::LongOffset,
        "shortGeneric" => TimeZoneName::ShortGeneric,
        "longGeneric" => TimeZoneName::LongGeneric,
        _ => TimeZoneName::Short,
    };
    time_zone_name_text(state, kind, moment.offset_seconds, moment.is_dst)
}

/// As partes de `moment` para os campos `fields`: a data, a junção e a hora, com os dígitos do `numberingSystem`.
fn parts_with_fields(state: &DateTimeFormatState, fields: &Fields, moment: &Moment) -> Vec<Part> {
    let mut parts = calendar_parts(state, fields, moment);
    if !matches!(state.numbering.as_str(), "" | "latn") {
        if let Some(digits) = icu_number::digits_of(state.locale.split("-u-").next().unwrap_or(""), &state.numbering) {
            for (kind, text) in parts.iter_mut() {
                // `M02` (mês do calendário chinês) é nome, não número: os dígitos ficam ASCII.
                let month_name = kind == "month" && text.starts_with('M');
                // `timeZoneName` leva os dígitos do `numberingSystem` no deslocamento (`غرينتش+١٣`, `GMT+١٣`).
                if !month_name && !matches!(kind.as_str(), "literal" | "dayPeriod" | "era" | "weekday") {
                    *text = text.chars().map(|c| c.to_digit(10).map_or(c, |d| digits[d as usize])).collect();
                }
            }
        }
    }
    parts
}

/// O calendário padrão do locale quando nada o pede (medido no bun 1.4.2 nos 100 locales do porte): `th`
/// budista, `fa` e `ps` persa (`ps-PK` e `ur`-like seguem gregoriano); os demais, gregoriano.
fn default_calendar(locale: &str) -> Option<&'static str> {
    let mut subtags = locale.split("-u-").next().unwrap_or("").split('-');
    let language = subtags.next()?;
    let region = subtags.find(|tag| tag.len() == 2 && tag.chars().all(|c| c.is_ascii_alphabetic()));
    match (language, region) {
        ("th", _) => Some("buddhist"),
        ("fa", _) => Some("persian"),
        ("ps", Some("PK")) => None,
        ("ps", _) => Some("persian"),
        _ => None,
    }
}

/// As partes da data de um calendário não gregoriano pelo padrão que o ICU escolhe para o skeleton no locale
/// (`intl_calendar_patterns`, medido no bun), ou `None` quando a tabela não tem o skeleton (aí vale o gregoriano de
/// `date_parts`). O padrão vem do locale resolvido pela cadeia do ICU: `língua-REGIÃO`, a língua e, sem tabela
/// para ela, a língua dos dados do porte (`en` ou `pt`). Quando `fields` são os do formatador inteiro, o padrão
/// traz também a hora (a
/// chave é a de `data_key`) e o segundo valor é `true`: quem chama não junta a hora de novo. Com campos parciais
/// (as pontas de um `formatRange`) a chave é só a da data.
fn native_pattern_parts(
    state: &DateTimeFormatState,
    fields: &Fields,
    moment: &Moment,
    native: Option<&NativeDate>,
) -> Option<(Vec<Part>, bool)> {
    let native = native?;
    let data = intl_date_time_data::locale_data_for(state.locale.split("-u-").next().unwrap_or(""));
    let (pattern, whole) = table_pattern(state, fields, state.native.name(), data)?;
    let time = |kind: &str, argument: &str| {
        data.map(|data| {
            let zone = |style: &str| zone_style_text(state, moment, style);
            data.token(kind, argument, false, &moment.civil, moment.milliseconds, state.cycle.as_str(), &zone)
        })
    };
    let weekday = |width: &str, standalone: bool| -> String {
        let index = moment.civil.wday as usize;
        match data {
            Some(data) => data.weekday_name(width, index, standalone).to_string(),
            None => weekday_name(
                state.language,
                match width {
                    "short" => TextWidth::Short,
                    "narrow" => TextWidth::Narrow,
                    _ => TextWidth::Long,
                },
                index,
            )
            .to_string(),
        }
    };
    Some((intl_calendar::render_pattern(pattern, native, &weekday, &time), whole))
}

/// As partes da data gregoriana pelo mesmo padrão medido que os outros calendários (`gregory` em
/// `intl_calendar_patterns`), com os nomes de `intl_date_time_data` do locale. `None` sem dados do locale ou quando
/// a tabela não tem o skeleton (aí valem os padrões avulsos de `locale_data_parts` e, por fim, `date_parts`). O
/// segundo valor é como em [`native_pattern_parts`].
fn gregorian_pattern_parts(state: &DateTimeFormatState, fields: &Fields, moment: &Moment) -> Option<(Vec<Part>, bool)> {
    let data = intl_date_time_data::locale_data_for(state.locale.split("-u-").next().unwrap_or(""))?;
    let (pattern, whole) = table_pattern(state, fields, "gregory", Some(data))?;
    let zone = |style: &str| zone_style_text(state, moment, style);
    Some((data.render(pattern, &moment.civil, moment.milliseconds, state.cycle.as_str(), &zone), whole))
}

/// A cadeia de locales do ICU para as tabelas por calendário: `língua-REGIÃO`, a língua, e por fim a língua dos
/// dados do porte (o "root" do porte, a mesma que `Language::of_locale` usa para o resto do formatador).
fn icu_locale_chain(state: &DateTimeFormatState) -> Option<Vec<String>> {
    let tag = state.locale.split("-u-").next().unwrap_or("");
    let mut subtags = tag.split('-');
    let language = subtags.next()?;
    let region = subtags.find(|subtag| subtag.len() == 2 && subtag.bytes().all(|byte| byte.is_ascii_alphabetic()));
    let base = match state.language {
        Language::English => "en",
        Language::Portuguese => "pt",
    };
    let mut chain: Vec<String> = region.map(|region| format!("{language}-{}", region.to_ascii_uppercase())).into_iter().collect();
    chain.extend([language.to_string(), base.to_string()]);
    Some(chain)
}

/// O padrão de `intl_calendar_patterns` do calendário `calendar` para `fields` no locale do formatador, pela cadeia
/// do ICU (`língua-REGIÃO`, a língua e, por fim, a língua dos dados do porte), e se ele já traz a hora. `None`
/// quando `fields` não tem data ou a tabela não tem o skeleton.
fn table_pattern(
    state: &DateTimeFormatState,
    fields: &Fields,
    calendar: &str,
    data: Option<&intl_date_time_data::LocaleData>,
) -> Option<(&'static str, bool)> {
    let whole = *fields == state.fields;
    let has_date = state.date_style.is_some()
        || [fields.weekday.is_some(), fields.era.is_some(), fields.year.is_some(), fields.month.is_some(), fields.day.is_some()].contains(&true);
    if !has_date {
        return None;
    }
    let chain = icu_locale_chain(state)?;
    let lookup = |key: &str| chain.iter().find_map(|locale| intl_calendar_patterns::pattern(locale, calendar, key));
    // O padrão do formatador inteiro (data e hora); sem ele, o da data sozinha e a hora é juntada por quem chama.
    let whole_pattern = if whole {
        data_key(state).filter(|key| data.is_some() || !key.contains('|')).and_then(|key| lookup(&key))
    } else {
        None
    };
    match whole_pattern {
        Some(pattern) => Some((pattern, true)),
        None => Some((lookup(&date_key(state, fields)?)?, false)),
    }
}

/// A chave de skeleton só com a data de `fields` (ou o `dateStyle`); `None` com `dateStyle` e `timeStyle` juntos.
fn date_key(state: &DateTimeFormatState, fields: &Fields) -> Option<String> {
    let mut key = String::new();
    match (state.date_style, state.time_style) {
        (Some(style), _) => push_key(&mut key, "dateStyle", style.as_str()),
        (None, _) => {
            for (name, width) in [
                ("weekday", fields.weekday.map(|width| width.as_str())),
                ("era", fields.era.map(|width| width.as_str())),
                ("year", fields.year.map(|width| width.as_str())),
                ("month", fields.month.map(|width| width.as_str())),
                ("day", fields.day.map(|width| width.as_str())),
            ] {
                if let Some(width) = width {
                    push_key(&mut key, name, width);
                }
            }
        }
    }
    (!key.is_empty()).then_some(key)
}

/// A data (gregoriana ou do calendário pedido), a junção e a hora, em dígitos ASCII.
fn calendar_parts(state: &DateTimeFormatState, fields: &Fields, moment: &Moment) -> Vec<Part> {
    // Os padrões medidos são os do gregoriano; o `iso8601` tem os seus (`iso_calendar_parts`), da raiz do CLDR.
    if state.iso_calendar {
        return iso_calendar_parts(state, fields, moment);
    }
    let native_date = state.native.date(&state.locale, &moment.civil);
    // O gregoriano passa pela mesma tabela medida dos outros calendários (`gregory`); o que ela não tem cai nos
    // padrões avulsos de `intl_date_time_data` e, por fim, em `date_parts`.
    let mut gregorian_date = None;
    if native_date.is_none() {
        match gregorian_pattern_parts(state, fields, moment) {
            Some((parts, true)) => return parts,
            Some((parts, false)) => gregorian_date = Some(parts),
            None => {}
        }
    }
    if native_date.is_none() && *fields == state.fields {
        if let Some(parts) = locale_data_parts(state, moment) {
            return parts;
        }
    }
    let mut parts = if let Some(parts) = gregorian_date {
        parts
    } else {
        match native_pattern_parts(state, fields, moment, native_date.as_ref()) {
            Some((parts, true)) => return parts,
            Some((parts, false)) => parts,
            None => date_parts(state.language, fields, moment),
        }
    };
    let time = time_parts(state, fields, moment);
    if !parts.is_empty() && !time.is_empty() {
        // Medido no bun: com calendário não gregoriano `ja` junta com espaço e `fr` com ` à ` ou espaço, como no `iso8601`.
        let native_ja_fr = native_date.is_some() && matches!(state.locale.split(['-', '_']).next(), Some("ja" | "fr"));
        parts.push(if native_ja_fr { iso_date_time_joiner(state, fields) } else { date_time_joiner(state, fields) });
    }
    parts.extend(time);
    parts
}

/// O que liga a data à hora: ` at ` e ` às ` com a data por extenso, vírgula nas demais.
fn date_time_joiner(state: &DateTimeFormatState, fields: &Fields) -> Part {
    let long_date = matches!(state.date_style, Some(StyleWidth::Full | StyleWidth::Long))
        || (state.date_style.is_none() && fields.month == Some(Month::Long));
    literal(match (state.language, long_date) {
        (Language::English, true) => " at ",
        (Language::Portuguese, true) => " \u{e0}s ",
        _ => ", ",
    })
}

/// As partes do instante formatado (`udat_formatForFields`).
fn format_to_parts_at(state: &DateTimeFormatState, epoch_milliseconds: i64) -> Vec<Part> {
    parts_with_fields(state, &state.fields, &moment_at(&state.zone, epoch_milliseconds))
}

// ---------------------------------------------------------------------------------------------
// Inicialização
// ---------------------------------------------------------------------------------------------

/// `ISO8601::parseUTCOffsetInMinutes`: `+hh`, `+hhmm` ou `+hh:mm`, em segundos.
fn parse_utc_offset(text: &str) -> Option<i32> {
    let sign = match text.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let rest = &text[1..];
    let (hours, minutes) = match rest.len() {
        2 => (rest, "00"),
        4 => (&rest[..2], &rest[2..]),
        5 if rest.as_bytes()[2] == b':' => (&rest[..2], &rest[3..]),
        _ => return None,
    };
    if !hours.bytes().all(|byte| byte.is_ascii_digit()) || !minutes.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let (hours, minutes): (i32, i32) = (hours.parse().ok()?, minutes.parse().ok()?);
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(sign * (hours * 3600 + minutes * 60))
}

/// O fuso pedido em `timeZone`: o fuso do `jiff` e o nome do `resolvedOptions`.
fn resolve_time_zone(requested: &str) -> Result<(TimeZone, String), Thrown> {
    if let Some(seconds) = parse_utc_offset(requested) {
        let offset = Offset::from_seconds(seconds).map_err(|_| Thrown::range_error(&format!("invalid time zone: {requested}")))?;
        let sign = if seconds < 0 { '-' } else { '+' };
        let total = seconds.unsigned_abs();
        return Ok((TimeZone::fixed(offset), format!("{sign}{:02}:{:02}", total / 3600, total % 3600 / 60)));
    }
    // Só `UTC` (em qualquer caixa) vira `UTC`; o resto mantém o nome IANA pedido, sem
    // canonicalizar (medido no bun: `GMT`, `Etc/UTC`, `Etc/GMT`, `Zulu`, `Asia/Calcutta` e
    // `Europe/Kiev` voltam como pedidos, na caixa canônica).
    if requested.eq_ignore_ascii_case("utc") {
        return Ok((TimeZone::UTC, "UTC".to_string()));
    }
    match TimeZoneDatabase::bundled().get(requested) {
        Ok(zone) => {
            let name = zone.iana_name().unwrap_or(requested).to_string();
            Ok((zone, name))
        }
        Err(_) => {
            // A tzdata embutida pode não trazer os aliases de UTC (`GMT`, `Zulu`, `UCT`...).
            // O bun devolve o alias na caixa canônica (`GMT`, `Etc/UTC`, `Zulu`), não `UTC`.
            const UTC_ALIASES: [&str; 11] = [
                "Etc/UTC", "Etc/GMT", "GMT", "Etc/UCT", "UCT", "Etc/Zulu", "Zulu", "Etc/Universal", "Universal", "Etc/Greenwich",
                "Greenwich",
            ];
            match UTC_ALIASES.iter().find(|alias| alias.eq_ignore_ascii_case(requested)) {
                Some(alias) => Ok((TimeZone::UTC, (*alias).to_string())),
                None => Err(Thrown::range_error(&format!("invalid time zone: {requested}"))),
            }
        }
    }
}

/// `IntlDateTimeFormat::initializeDateTimeFormat`. `forced_zone` é o `toLocaleStringTimeZone` do
/// `ZonedDateTime.prototype.toLocaleString`: o fuso do próprio objeto, e `options.timeZone` é `TypeError`.
fn initialize(
    global_object: &JSGlobalObject,
    locales: JSValue,
    options_value: JSValue,
    required: Required,
    defaults: Defaults,
    forced_zone: Option<&str>,
) -> Result<DateTimeFormatState, Thrown> {
    let resolved: ResolvedLocale = resolve_locale_from(global_object, locales, &["ca", "hc", "nu"])?;
    let options = coerce_options_to_object(global_object, options_value)?;
    read_locale_matcher(global_object, options)?;

    let calendar_option = option_string(global_object, options, "calendar", &[], "")?;
    if let Some(calendar) = &calendar_option {
        if !is_unicode_locale_identifier_type(calendar) {
            return Err(Thrown::range_error("calendar is not a well-formed calendar value"));
        }
    }
    let numbering_option = option_string(global_object, options, "numberingSystem", &[], "")?;
    if let Some(numbering_system) = &numbering_option {
        if !is_unicode_locale_identifier_type(numbering_system) {
            return Err(Thrown::range_error("numberingSystem is not a well-formed numbering system value"));
        }
    }
    let hour12 = option_bool(global_object, options, "hour12")?;
    let hour_cycle_option = option_enum::<HourCycle>(
        global_object,
        options,
        "hourCycle",
        "hourCycle must be \"h11\", \"h12\", \"h23\", or \"h24\"",
    )?;

    // A opção `calendar` ganha da extensão `-u-ca-`; `iso8601`, `gregory` e os calendários do `icu_calendar`
    // (`NativeCalendar`) têm dados, o resto cai no gregoriano.
    let requested_calendar: Option<String> = calendar_option.clone().or_else(|| resolved.keyword("ca").map(str::to_string));
    let iso_calendar = requested_calendar.as_deref().is_some_and(|calendar| calendar.eq_ignore_ascii_case("iso8601"));
    // `islamic-rgsa` o ICU reconhece sem ter dados (o `-u-ca-` fica no locale), e o formato cai no calendário padrão
    // do locale (`th` budista, `fa` persa), não no gregoriano.
    let requested_with_data = requested_calendar.as_deref().filter(|calendar| !NativeCalendar::is_recognized_without_data(calendar));
    let native = requested_with_data.or_else(|| default_calendar(&resolved.locale)).and_then(NativeCalendar::parse).unwrap_or_default();
    let mut honored: Vec<(&str, &str)> = Vec::new();
    // O `-u-nu-` e a opção `numberingSystem` valem quando o icu4x tem os dígitos; senão, o padrão do locale.
    let numbering_requested = numbering_option.as_deref().or_else(|| resolved.keyword("nu"));
    let numbering_supported = numbering_requested.filter(|name| icu_number::numbering_system_honored(&resolved.locale, name));
    if let Some(extension) = resolved.keyword("nu") {
        if numbering_supported == Some(extension) {
            honored.push(("nu", extension));
        }
    }
    let numbering = match numbering_supported {
        Some(name) => name.to_string(),
        None => icu_number::default_numbering_system(&resolved.locale).to_string(),
    };
    if let Some(extension) = resolved.keyword("ca") {
        let known = native.is_native()
            || iso_calendar
            || extension == "gregory"
            || NativeCalendar::is_recognized_without_data(extension);
        if known && requested_calendar.as_deref() == Some(extension) {
            honored.push(("ca", extension));
        }
    }
    let extension_cycle = if hour12.is_none() && hour_cycle_option.is_none() {
        resolved.keyword("hc").and_then(HourCycle::parse)
    } else {
        None
    };
    if let Some(cycle) = extension_cycle {
        honored.push(("hc", cycle.as_str()));
    }
    let locale = resolved.tag_with(&honored);

    let time_zone_value = match options {
        Some(options) => get_property(global_object, options, "timeZone")?,
        None => JSValue::undefined(),
    };
    let (zone, zone_name) = if let Some(forced) = forced_zone {
        // `toLocaleStringTimeZone`: o fuso do `ZonedDateTime`, e pedir outro é `TypeError`.
        if !time_zone_value.is_undefined() {
            return Err(Thrown::type_error(
                "ZonedDateTime.toLocaleString does not accept a timeZone option; the ZonedDateTime's time zone is used",
            ));
        }
        resolve_time_zone(forced)?
    } else if time_zone_value.is_undefined() {
        let zone = crate::runtime::process_time_zone::process_zone();
        let name = zone.iana_name().unwrap_or("UTC").to_string();
        (zone, name)
    } else {
        resolve_time_zone(&to_rust_string(global_object, time_zone_value)?)?
    };

    let width_message = |name: &str| format!("{name} must be \"narrow\", \"short\", or \"long\"");
    let digits_message = |name: &str| format!("{name} must be \"2-digit\" or \"numeric\"");
    let mut fields = Fields {
        weekday: option_enum::<TextWidth>(global_object, options, "weekday", &width_message("weekday"))?,
        era: option_enum::<TextWidth>(global_object, options, "era", &width_message("era"))?,
        year: option_enum::<Digits2>(global_object, options, "year", &digits_message("year"))?,
        month: option_enum::<Month>(
            global_object,
            options,
            "month",
            "month must be \"2-digit\", \"numeric\", \"narrow\", \"short\", or \"long\"",
        )?,
        day: option_enum::<Digits2>(global_object, options, "day", &digits_message("day"))?,
        day_period: option_enum::<TextWidth>(global_object, options, "dayPeriod", &width_message("dayPeriod"))?,
        hour: option_enum::<Digits2>(global_object, options, "hour", &digits_message("hour"))?,
        minute: option_enum::<Digits2>(global_object, options, "minute", &digits_message("minute"))?,
        second: option_enum::<Digits2>(global_object, options, "second", &digits_message("second"))?,
        fractional_second_digits: number_option(global_object, options, "fractionalSecondDigits", 1, 3)?.unwrap_or(0),
        time_zone_name: option_enum::<TimeZoneName>(
            global_object,
            options,
            "timeZoneName",
            "timeZoneName must be \"short\", \"long\", \"shortOffset\", \"longOffset\", \"shortGeneric\", or \"longGeneric\"",
        )?,
    };
    option_string(
        global_object,
        options,
        "formatMatcher",
        &["basic", "best fit"],
        "formatMatcher must be either \"basic\" or \"best fit\"",
    )?;
    let style_message = |name: &str| format!("{name} must be \"full\", \"long\", \"medium\", or \"short\"");
    let date_style = option_enum::<StyleWidth>(global_object, options, "dateStyle", &style_message("dateStyle"))?;
    let time_style = option_enum::<StyleWidth>(global_object, options, "timeStyle", &style_message("timeStyle"))?;

    let user_fields = fields;
    let any_present = fields.has_date_without_era() || fields.has_time();
    if date_style.is_some() || time_style.is_some() {
        if fields.any() {
            return Err(Thrown::type_error("dateStyle and timeStyle may not be used with other DateTimeFormat options"));
        }
        if required == Required::Date && time_style.is_some() {
            return Err(Thrown::type_error("timeStyle is specified while formatting date is requested"));
        }
        if required == Required::Time && date_style.is_some() {
            return Err(Thrown::type_error("dateStyle is specified while formatting time is requested"));
        }
        fields = fields_for_styles(resolved.language, date_style, time_style);
    } else {
        let mut need_defaults = true;
        if matches!(required, Required::Date | Required::Any) && fields.has_date_without_era() {
            need_defaults = false;
        }
        if matches!(required, Required::Time | Required::Any) && fields.has_time() {
            need_defaults = false;
        }
        if need_defaults && matches!(defaults, Defaults::Date | Defaults::All | Defaults::ZonedDateTime) {
            fields.year = Some(Digits2::Numeric);
            fields.month = Some(Month::Numeric);
            fields.day = Some(Digits2::Numeric);
        }
        if need_defaults && matches!(defaults, Defaults::Time | Defaults::All | Defaults::ZonedDateTime) {
            fields.hour = Some(Digits2::Numeric);
            fields.minute = Some(Digits2::Numeric);
            fields.second = Some(Digits2::Numeric);
        }
        if need_defaults && defaults == Defaults::ZonedDateTime && fields.time_zone_name.is_none() {
            fields.time_zone_name = Some(TimeZoneName::Short);
        }
    }

    // O ciclo de horas: `hour12` ganha de `hourCycle`, que ganha da extensão `-u-hc-`, que ganha do
    // padrão do locale (`h12` em inglês, `h23` em português).
    // Com dados medidos no bun (`intl_date_time_data`) o ciclo de `hour12` e o padrão vêm da tabela.
    let table = intl_date_time_data::locale_data_for(&locale);
    let table_cycle = |name: fn(&intl_date_time_data::LocaleData) -> &'static str| table.and_then(|data| HourCycle::parse(name(data)));
    let locale_default = table_cycle(|data| data.default_cycle)
        .unwrap_or(if resolved.language == Language::English { HourCycle::H12 } else { HourCycle::H23 });
    let cycle = match (hour12, hour_cycle_option, extension_cycle) {
        (Some(true), _, _) => table_cycle(|data| data.hour12_true_cycle).unwrap_or(HourCycle::H12),
        (Some(false), _, _) => table_cycle(|data| data.hour12_false_cycle).unwrap_or(HourCycle::H23),
        (None, Some(cycle), _) | (None, None, Some(cycle)) => cycle,
        (None, None, None) if iso_calendar && time_style.is_some() => HourCycle::H23,
        (None, None, None) => locale_default,
    };
    let mut fields = resolve_fields(resolved.language, cycle, fields);
    // Os padrões de `timeStyle` da raiz do `iso8601` são `HH:mm` e `hh:mm a`, em todo locale (medido no bun).
    if iso_calendar && time_style.is_some() && fields.hour.is_some() {
        fields.hour = Some(Digits2::TwoDigit);
    }
    let hour_cycle = fields.hour.map(|_| cycle);
    let mut state = DateTimeFormatState {
        locale,
        language: resolved.language,
        zone,
        zone_name,
        hour_cycle,
        fields,
        date_style,
        time_style,
        user_fields,
        any_present,
        cycle,
        iso_calendar,
        native,
        numbering,
    };
    apply_pattern_widths(&mut state);
    Ok(state)
}

/// A largura que o padrão do locale dá ao mês, ao dia e ao ano numéricos (`dd/MM/y` em fr e pt, `d.M.y` em de): é o que
/// `resolvedOptions` mostra (`month: "2-digit"`), e não `numeric`. Só vale no gregoriano, com a tabela medida do locale.
fn apply_pattern_widths(state: &mut DateTimeFormatState) {
    if state.native.is_native() || state.iso_calendar || state.date_style.is_some() || state.time_style.is_some() {
        return;
    }
    let Some(pattern) = intl_date_time_data::locale_data_for(&state.locale).and_then(|data| data_key(state).and_then(|key| data.pattern(&key))) else {
        return;
    };
    let width = |field: &str| -> Option<Digits2> {
        if pattern.contains(&format!("{{{field}:2-digit}}")) {
            Some(Digits2::TwoDigit)
        } else if pattern.contains(&format!("{{{field}:numeric}}")) {
            Some(Digits2::Numeric)
        } else {
            None
        }
    };
    if matches!(state.fields.month, Some(Month::Numeric | Month::TwoDigit)) {
        state.fields.month = width("month").map(|width| if width == Digits2::TwoDigit { Month::TwoDigit } else { Month::Numeric }).or(state.fields.month);
    }
    if state.fields.day.is_some() {
        state.fields.day = width("day").or(state.fields.day);
    }
}

/// `Date.prototype.toLocaleString` e irmãs: o texto de `time_value` com `locales` e `options`.
pub fn to_locale_string(
    global_object: &JSGlobalObject,
    locales: JSValue,
    options: JSValue,
    required: Required,
    defaults: Defaults,
    time_value: f64,
) -> Result<String, Thrown> {
    let state = initialize(global_object, locales, options, required, defaults, None)?;
    let value = time_clip(time_value);
    if value.is_nan() {
        return Err(Thrown::range_error("date value is not finite in DateTimeFormat format()"));
    }
    Ok(format_to_parts_at(&state, value as i64).into_iter().map(|(_, text)| text).collect())
}

/// O valor de tempo do argumento de `format`: `undefined` é agora, o resto `ToNumber`.
fn date_argument(global_object: &JSGlobalObject, value: JSValue, method: &str) -> Result<i64, Thrown> {
    let number = if value.is_undefined() {
        crate::runtime::date_constructor::js_date_now(f64::NAN)
    } else {
        to_number_checked(global_object, value)?
    };
    let clipped = time_clip(number);
    if clipped.is_nan() {
        return Err(Thrown::range_error(&format!("date value is not finite in DateTimeFormat {method}()")));
    }
    Ok(clipped as i64)
}

fn construct_date_time_format_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_instance(global_object, call, |global_object| {
        Ok(Box::new(initialize(global_object, call.argument(0), call.argument(1), Required::Any, Defaults::Date, None)?))
    })
}

/// `callDateTimeFormat`: sem `new`, sem ler `newTarget()`.
fn call_date_time_format_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    call_instance(global_object, call, |global_object| {
        Ok(Box::new(initialize(global_object, call.argument(0), call.argument(1), Required::Any, Defaults::Date, None)?))
    })
}

fn format_function_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<DateTimeFormatState, _>(
        call.this_value(),
        "Intl.DateTimeFormat.prototype.format called on value that's not a DateTimeFormat",
        |state, _| {
            let parts = temporal::value_parts(global_object, state, call.argument(0), "format")?;
            let text: String = parts.into_iter().map(|(_, text)| text).collect();
            Ok(str_value(global_object.vm(), &text))
        },
    )
}

fn format_getter_body(global_object: &JSGlobalObject, this_value: JSValue, _name: &PropertyName) -> HostResult {
    with_instance::<DateTimeFormatState, _>(
        this_value,
        "Intl.DateTimeFormat.prototype.format called on value that's not a DateTimeFormat",
        |_, instance| bound_function(global_object, instance, date_time_format_format, "format", 1),
    )
}

fn format_to_parts_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<DateTimeFormatState, _>(
        call.this_value(),
        "Intl.DateTimeFormat.prototype.formatToParts called on value that's not a DateTimeFormat",
        |state, _| {
            let parts = temporal::value_parts(global_object, state, call.argument(0), "formatToParts")?;
            Ok(parts_array(global_object, &parts))
        },
    )
}

fn resolved_options_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<DateTimeFormatState, _>(
        call.this_value(),
        "Intl.DateTimeFormat.prototype.resolvedOptions called on value that's not a DateTimeFormat",
        |state, _| {
            let vm = global_object.vm();
            let options = new_object(global_object);
            put(global_object, &options, "locale", str_value(vm, &state.locale));
            put(
                global_object,
                &options,
                "calendar",
                str_value(vm, if state.iso_calendar { "iso8601" } else if state.native.is_native() { state.native.name() } else { "gregory" }),
            );
            let numbering = if state.numbering.is_empty() { "latn" } else { state.numbering.as_str() };
            put(global_object, &options, "numberingSystem", str_value(vm, numbering));
            put(global_object, &options, "timeZone", str_value(vm, &state.zone_name));
            if let Some(cycle) = state.hour_cycle {
                put(global_object, &options, "hourCycle", str_value(vm, cycle.as_str()));
                put(global_object, &options, "hour12", js_boolean(matches!(cycle, HourCycle::H11 | HourCycle::H12)));
            }
            if state.date_style.is_none() && state.time_style.is_none() {
                let fields = &state.fields;
                if let Some(width) = fields.weekday {
                    put(global_object, &options, "weekday", str_value(vm, width.as_str()));
                }
                if let Some(width) = fields.era {
                    put(global_object, &options, "era", str_value(vm, width.as_str()));
                }
                if let Some(width) = fields.year {
                    put(global_object, &options, "year", str_value(vm, width.as_str()));
                }
                if let Some(width) = fields.month {
                    put(global_object, &options, "month", str_value(vm, width.as_str()));
                }
                if let Some(width) = fields.day {
                    put(global_object, &options, "day", str_value(vm, width.as_str()));
                }
                // O `a` do padrão de 12 horas volta como `dayPeriod: "short"` (o `parse` do padrão em
                // `IntlDateTimeFormat.cpp` trata `a`, `b` e `B` como o campo `dayPeriod`).
                let twelve_hour_marker = matches!(state.hour_cycle, Some(HourCycle::H11 | HourCycle::H12)) && fields.hour.is_some();
                let day_period = fields.day_period.or(if twelve_hour_marker { Some(TextWidth::Short) } else { None });
                if let Some(width) = day_period {
                    put(global_object, &options, "dayPeriod", str_value(vm, width.as_str()));
                }
                if let Some(width) = fields.hour {
                    put(global_object, &options, "hour", str_value(vm, width.as_str()));
                }
                if let Some(width) = fields.minute {
                    put(global_object, &options, "minute", str_value(vm, width.as_str()));
                }
                if let Some(width) = fields.second {
                    put(global_object, &options, "second", str_value(vm, width.as_str()));
                }
                if fields.fractional_second_digits > 0 {
                    put(global_object, &options, "fractionalSecondDigits", number_value(fields.fractional_second_digits));
                }
                if let Some(kind) = fields.time_zone_name {
                    put(global_object, &options, "timeZoneName", str_value(vm, kind.as_str()));
                }
            } else {
                if let Some(style) = state.date_style {
                    put(global_object, &options, "dateStyle", str_value(vm, style.as_str()));
                }
                if let Some(style) = state.time_style {
                    put(global_object, &options, "timeStyle", str_value(vm, style.as_str()));
                }
            }
            Ok(options.as_value())
        },
    )
}

/// Os dois extremos de `formatRange` e `formatRangeToParts`: `undefined` é `TypeError`, o resto
/// `ToNumber` (o primeiro, depois o segundo) e `TimeClip` com `NaN` como `RangeError`.
fn range_parts_of(global_object: &JSGlobalObject, call: &HostCall, state: &DateTimeFormatState) -> Result<Vec<RangePart>, Thrown> {
    let (start_value, end_value) = (call.argument(0), call.argument(1));
    if start_value.is_undefined() || end_value.is_undefined() {
        return Err(Thrown::type_error("startDate or endDate is undefined"));
    }
    temporal::range_parts(global_object, state, start_value, end_value)
}

fn format_range_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<DateTimeFormatState, _>(
        call.this_value(),
        "Intl.DateTimeFormat.prototype.formatRange called on value that's not a DateTimeFormat",
        |state, _| {
            let text: String = range_parts_of(global_object, call, state)?.into_iter().map(|(_, text, _)| text).collect();
            Ok(str_value(global_object.vm(), &text))
        },
    )
}

fn format_range_to_parts_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<DateTimeFormatState, _>(
        call.this_value(),
        "Intl.DateTimeFormat.prototype.formatRangeToParts called on value that's not a DateTimeFormat",
        |state, _| Ok(range_parts_array(global_object, &range_parts_of(global_object, call, state)?)),
    )
}

host_function!(call_date_time_format, call_date_time_format_body);
host_function!(construct_date_time_format, construct_date_time_format_body);
host_function!(date_time_format_format, format_function_body);
custom_getter!(date_time_format_proto_format_getter, format_getter_body);
host_function!(date_time_format_proto_format_to_parts, format_to_parts_body);
host_function!(date_time_format_proto_format_range, format_range_body);
host_function!(date_time_format_proto_format_range_to_parts, format_range_to_parts_body);
host_function!(date_time_format_proto_resolved_options, resolved_options_body);

/// `dateTimeFormatPrototypeTableValues` de `IntlDateTimeFormatPrototype.lut.h`, na ordem do `@begin`.
static DATE_TIME_FORMAT_PROTOTYPE_TABLE_VALUES: [HashTableValue; 5] = intl_format_prototype_values(
    date_time_format_proto_format_getter,
    date_time_format_proto_format_range,
    date_time_format_proto_format_range_to_parts,
    date_time_format_proto_format_to_parts,
    date_time_format_proto_resolved_options,
);

/// `dateTimeFormatPrototypeTable`.
static DATE_TIME_FORMAT_PROTOTYPE_TABLE: HashTable =
    HashTable { class_for_this: None, values: &DATE_TIME_FORMAT_PROTOTYPE_TABLE_VALUES };

/// `IntlDateTimeFormatPrototype::s_info` (`"Intl.DateTimeFormat"`).
static DATE_TIME_FORMAT_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "Intl.DateTimeFormat",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&DATE_TIME_FORMAT_PROTOTYPE_TABLE),
    inherits_js_type_range: None,
};

/// `IntlDateTimeFormatConstructor` e `IntlDateTimeFormatPrototype` (`format`, `formatToParts`,
/// `formatRange`, `formatRangeToParts`, `resolvedOptions`, da tabela estática, reificadas no primeiro acesso).
/// O construtor funciona com e sem `new`.
pub fn install_date_time_format(global_object: &JSGlobalObject, intl: &JSObject) {
    let class = IntlClass {
        name: "DateTimeFormat",
        length: 0,
        has_supported_locales_of: true,
        call: call_date_time_format,
        construct: construct_date_time_format,
    };
    class.install_with_table(global_object, intl, &DATE_TIME_FORMAT_PROTOTYPE_S_INFO);
}

mod range;
pub mod temporal;

#[cfg(test)]
mod tests {
    use super::*;

    fn state(language: Language, fields: Fields, cycle: HourCycle, zone: TimeZone) -> DateTimeFormatState {
        let fields = resolve_fields(language, cycle, fields);
        DateTimeFormatState {
            locale: String::new(),
            language,
            zone,
            zone_name: "UTC".to_string(),
            hour_cycle: fields.hour.map(|_| cycle),
            fields,
            date_style: None,
            time_style: None,
            user_fields: Fields::default(),
            any_present: false,
            cycle,
            iso_calendar: false,
            native: NativeCalendar::default(),
            numbering: String::new(),
        }
    }

    fn text(state: &DateTimeFormatState, milliseconds: i64) -> String {
        format_to_parts_at(state, milliseconds).into_iter().map(|(_, text)| text).collect()
    }

    fn all_numeric() -> Fields {
        Fields {
            year: Some(Digits2::Numeric),
            month: Some(Month::Numeric),
            day: Some(Digits2::Numeric),
            hour: Some(Digits2::Numeric),
            minute: Some(Digits2::Numeric),
            second: Some(Digits2::Numeric),
            ..Fields::default()
        }
    }

    #[test]
    fn english_default_matches_the_icu_pattern() {
        let state = state(Language::English, all_numeric(), HourCycle::H12, TimeZone::UTC);
        assert_eq!(text(&state, 0), "1/1/1970, 12:00:00 AM");
        assert_eq!(text(&state, 1_700_000_000_000), "11/14/2023, 10:13:20 PM");
    }

    #[test]
    fn portuguese_uses_day_first_and_24_hours() {
        let state = state(Language::Portuguese, all_numeric(), HourCycle::H23, TimeZone::UTC);
        assert_eq!(text(&state, 1_700_000_000_000), "14/11/2023, 22:13:20");
    }

    #[test]
    fn text_months() {
        let fields = Fields {
            weekday: Some(TextWidth::Long),
            year: Some(Digits2::Numeric),
            month: Some(Month::Long),
            day: Some(Digits2::Numeric),
            ..Fields::default()
        };
        let english = state(Language::English, fields, HourCycle::H12, TimeZone::UTC);
        assert_eq!(text(&english, 1_700_000_000_000), "Tuesday, November 14, 2023");
        let portuguese = state(Language::Portuguese, fields, HourCycle::H23, TimeZone::UTC);
        assert_eq!(text(&portuguese, 1_700_000_000_000), "ter\u{e7}a-feira, 14 de novembro de 2023");
    }

    #[test]
    fn time_zone_shifts_the_civil_time() {
        let zone = TimeZoneDatabase::bundled().get("America/Sao_Paulo").unwrap();
        let state = state(Language::English, all_numeric(), HourCycle::H12, zone);
        assert_eq!(text(&state, 1_700_000_000_000), "11/14/2023, 7:13:20 PM");
    }

    #[test]
    fn offsets_parse() {
        assert_eq!(parse_utc_offset("+03:00"), Some(10800));
        assert_eq!(parse_utc_offset("-0330"), Some(-12600));
        assert_eq!(parse_utc_offset("+3"), None);
    }
}
