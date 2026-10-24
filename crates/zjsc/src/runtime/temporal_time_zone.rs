//! Porte de `runtime/JSCTimeZone.h` (`TimeZone`, `TimeZoneID`), do trecho de `IntlObject.cpp` que resolve
//! identificadores (`intlResolveTimeZoneID`, `intlPrimaryTimeZoneID`, `utcTimeZoneID`) e de
//! `runtime/temporal/core/TimeZoneICUBridge.{h,cpp}`: `exactTimeToLocalDateAndTime`, `timeZoneEquals`,
//! `getOffsetNanosecondsFor`, `getISODateTimeFor`, `getPossibleEpochNanosecondsFor`, `getTimeZoneTransition`,
//! `getEpochNanosecondsFor` e `addZonedDateTime`.
//!
//! DIVERGÊNCIAS (o ICU do C++ é o `jiff` com a tzdata embutida, a mesma do `ProcessTimeZone`):
//!
//! - `TimeZoneID` é um `TimeZoneId`: o nome IANA com a caixa normalizada (o `[[Identifier]]` da spec, que
//!   preserva o alias, como `Asia/Calcutta`) mais o fuso do `jiff`. O C++ indexa a lista ordenada do ICU
//!   (primários primeiro, aliases depois); aqui a lista é a do `jiff`, que tem os mesmos nomes de ligação da
//!   tzdata mas não a separação de "primário" do CLDR. Dois `TimeZoneId` são iguais pelo nome, como o `==` do C++.
//! - `timeZoneEquals` pergunta pelo identificador primário (`intlPrimaryTimeZoneID`). O `jiff` não diz qual
//!   nome é o primário de uma ligação (`Backward`); a conta aqui é: o nome sem caixa, os sinônimos de UTC
//!   (`Etc/UTC`, `Etc/GMT`, `GMT`, `Zulu`...) valem `UTC` e uma tabela de ligações da tzdata (`Asia/Calcutta`,
//!   `US/Eastern`...) leva o alias ao primário. Alias que a tabela não lista conta como fuso próprio: FALTA a
//!   lista de primários do CLDR para igualar o ICU em todo identificador.
//! - O deslocamento fora da faixa do `jiff` (anos de -9999 a 9999) usa o instante equivalente
//!   (`equivalentTime`, ECMA 262 15.9.1.9), o que dá o deslocamento da regra POSIX final, como o ICU faria.
//! - `getTimeZoneTransition` só enxerga as transições que o `jiff` enumera (as da tabela e a regra POSIX até o
//!   ano 9999); depois disso responde `None`, onde o ICU seguiria a regra para sempre.
//! - `addZonedDateTime` e as funções de calendário só têm o ramo ISO (ver `temporal_calendar.rs`), de modo que o
//!   `CalendarID` não é parâmetro.

use std::fmt;
use std::rc::Rc;

use jiff::tz::{AmbiguousOffset, TimeZone as JiffTimeZone, TimeZoneDatabase};
use jiff::Timestamp;

use crate::runtime::iso8601::{
    format_time_zone_offset_string, is_date_time_within_limits, Duration, ExactTime, PlainDate, PlainDateTime, PlainTime,
};
use crate::runtime::temporal_core_duration::{get_utc_epoch_nanoseconds, plain_time_from_subday_ns, to_internal_duration};
use crate::runtime::temporal_calendar::CalendarID;
use crate::runtime::temporal_calendar_icu::calendar_date_add;
use crate::runtime::temporal_core_iso_date::add_days_to_iso_date;
use crate::runtime::temporal_core_types::{range_error, TemporalResult, TransitionDirection};
use crate::runtime::temporal_object::{TemporalDisambiguation, TemporalOverflow};
use crate::wtf::date_math::equivalent_time;

/// `epochNanosecondsOutOfRange`.
const EPOCH_NANOSECONDS_OUT_OF_RANGE: &str = "Epoch nanoseconds out of valid Temporal range";

const NS_PER_SECOND: i128 = ExactTime::NS_PER_SECOND;

/// `TimeZoneID` (`unsigned` no C++): o fuso IANA resolvido, com o identificador como a spec o guarda.
#[derive(Clone, Debug)]
pub struct TimeZoneId {
    /// `intlTimeZoneIDToString(id)`: o nome com a caixa normalizada, preservando o alias.
    name: Rc<str>,
    zone: JiffTimeZone,
}

impl TimeZoneId {
    /// `intlTimeZoneIDToString(id)`.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl PartialEq for TimeZoneId {
    fn eq(&self, other: &TimeZoneId) -> bool {
        self.name == other.name
    }
}

impl Eq for TimeZoneId {}

/// `class TimeZone`: um fuso nomeado (`TimeZone::fromID`) ou um deslocamento fixo (`TimeZone::fromUTCOffset`,
/// em nanossegundos).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimeZone {
    Id(TimeZoneId),
    UtcOffset(i64),
}

impl TimeZone {
    /// `TimeZone()`: `UTC`.
    pub fn utc() -> TimeZone {
        TimeZone::Id(utc_time_zone_id())
    }

    /// `isUTCOffset()`.
    pub fn is_utc_offset(&self) -> bool {
        matches!(self, TimeZone::UtcOffset(_))
    }
}

/// `TimeZone::toString()`: o identificador, ou `+HH:MM[:SS[.fff...]]` para o deslocamento.
impl fmt::Display for TimeZone {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimeZone::Id(id) => formatter.write_str(id.name()),
            TimeZone::UtcOffset(offset) => formatter.write_str(&format_time_zone_offset_string(*offset)),
        }
    }
}

/// `intlResolveTimeZoneID(name)`: qualquer nome aceito (sem diferenciar caixa; primários, ligações como
/// `Asia/Calcutta` e sinônimos de UTC) vira o `TimeZoneId` com o identificador normalizado. `None` se não é um
/// fuso IANA conhecido.
pub fn intl_resolve_time_zone_id(name: &[u8]) -> Option<TimeZoneId> {
    let name = std::str::from_utf8(name).ok()?;
    if !name.is_ascii() {
        return None;
    }
    let zone = TimeZoneDatabase::bundled().get(name).ok()?;
    let canonical: Rc<str> = Rc::from(zone.iana_name()?);
    Some(TimeZoneId { name: canonical, zone })
}

/// `utcTimeZoneID()`.
pub fn utc_time_zone_id() -> TimeZoneId {
    intl_resolve_time_zone_id(b"UTC").expect("o jiff embute o fuso UTC")
}

/// Os identificadores que o ECMA 402 normaliza para `UTC` (e as ligações da tzdata para `Etc/UTC` e `Etc/GMT`).
const UTC_EQUIVALENTS: [&str; 18] = [
    "utc",
    "etc/utc",
    "etc/uct",
    "etc/universal",
    "etc/zulu",
    "uct",
    "universal",
    "zulu",
    "gmt",
    "etc/gmt",
    "etc/gmt0",
    "etc/gmt+0",
    "etc/gmt-0",
    "etc/greenwich",
    "gmt0",
    "gmt+0",
    "gmt-0",
    "greenwich",
];

/// As ligações da tzdata (`backward`) mais usadas: `(alias, primário)`, em minúsculas.
const BACKWARD_LINKS: [(&str, &str); 44] = [
    ("asia/calcutta", "asia/kolkata"),
    ("asia/saigon", "asia/ho_chi_minh"),
    ("asia/katmandu", "asia/kathmandu"),
    ("asia/rangoon", "asia/yangon"),
    ("asia/dacca", "asia/dhaka"),
    ("asia/thimbu", "asia/thimphu"),
    ("asia/ulan_bator", "asia/ulaanbaatar"),
    ("asia/macao", "asia/macau"),
    ("america/buenos_aires", "america/argentina/buenos_aires"),
    ("america/cordoba", "america/argentina/cordoba"),
    ("america/indianapolis", "america/indiana/indianapolis"),
    ("america/louisville", "america/kentucky/louisville"),
    ("europe/kiev", "europe/kyiv"),
    ("atlantic/faeroe", "atlantic/faroe"),
    ("pacific/ponape", "pacific/pohnpei"),
    ("pacific/truk", "pacific/chuuk"),
    ("us/eastern", "america/new_york"),
    ("us/central", "america/chicago"),
    ("us/mountain", "america/denver"),
    ("us/pacific", "america/los_angeles"),
    ("us/alaska", "america/anchorage"),
    ("us/hawaii", "pacific/honolulu"),
    ("us/arizona", "america/phoenix"),
    ("navajo", "america/denver"),
    ("canada/eastern", "america/toronto"),
    ("canada/pacific", "america/vancouver"),
    ("brazil/east", "america/sao_paulo"),
    ("japan", "asia/tokyo"),
    ("singapore", "asia/singapore"),
    ("israel", "asia/jerusalem"),
    ("hongkong", "asia/hong_kong"),
    ("gb", "europe/london"),
    ("eire", "europe/dublin"),
    ("nz", "pacific/auckland"),
    ("prc", "asia/shanghai"),
    ("rok", "asia/seoul"),
    ("turkey", "europe/istanbul"),
    ("egypt", "africa/cairo"),
    ("iran", "asia/tehran"),
    ("cuba", "america/havana"),
    ("jamaica", "america/jamaica"),
    ("poland", "europe/warsaw"),
    ("portugal", "europe/lisbon"),
    ("iceland", "atlantic/reykjavik"),
];

/// `intlPrimaryTimeZoneID(id)`, como o nome minúsculo do primário (ver as DIVERGÊNCIAS do módulo).
fn primary_time_zone_name(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if UTC_EQUIVALENTS.contains(&lower.as_str()) {
        return "utc".to_string();
    }
    match BACKWARD_LINKS.iter().find(|(alias, _)| *alias == lower) {
        Some((_, primary)) => (*primary).to_string(),
        None => lower,
    }
}

/// `timeZoneEquals(a, b)` (`TimeZoneEquals`): https://tc39.es/proposal-temporal/#sec-temporal-timezoneequals
pub fn time_zone_equals(a: &TimeZone, b: &TimeZone) -> bool {
    // Passo 1: iguais.
    if a == b {
        return true;
    }
    // Passos 2 a 4: só dois nomes de fuso podem ser o mesmo fuso sob nomes diferentes.
    match (a, b) {
        (TimeZone::Id(one), TimeZone::Id(two)) => primary_time_zone_name(one.name()) == primary_time_zone_name(two.name()),
        _ => false,
    }
}

/// O `Timestamp` do `jiff` para o instante em segundos; fora da faixa dele, o do instante equivalente.
fn jiff_timestamp(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds)
        .or_else(|_| Timestamp::from_second(equivalent_time(seconds.saturating_mul(1000)).div_euclid(1000)))
        .unwrap_or(Timestamp::UNIX_EPOCH)
}

/// O deslocamento do fuso (UCAL_ZONE_OFFSET mais UCAL_DST_OFFSET) em segundos no instante `seconds`.
fn offset_seconds_at(zone: &JiffTimeZone, seconds: i64) -> i64 {
    i64::from(zone.to_offset_info(jiff_timestamp(seconds)).offset().seconds())
}

/// Os deslocamentos `UCAL_TZ_LOCAL_FORMER` e `UCAL_TZ_LOCAL_LATTER` (em segundos) para a hora local lida como se
/// fosse UTC: iguais longe de uma transição; `(antes, depois)` com `depois > antes` na lacuna e `depois < antes`
/// na sobreposição.
fn local_offsets_seconds(zone: &JiffTimeZone, local_seconds: i64) -> (i64, i64) {
    let civil = jiff_timestamp(local_seconds).to_zoned(JiffTimeZone::UTC).datetime();
    match zone.to_ambiguous_timestamp(civil).offset() {
        AmbiguousOffset::Unambiguous { offset } => (i64::from(offset.seconds()), i64::from(offset.seconds())),
        AmbiguousOffset::Gap { before, after } | AmbiguousOffset::Fold { before, after } => {
            (i64::from(before.seconds()), i64::from(after.seconds()))
        }
    }
}

/// `exactTimeToLocalDateAndTime(exactTime, offsetNs)` (`GetISOPartsFromEpoch(ℝ(epochNs) + offsetNs)`): a data e a
/// hora do instante num fuso de deslocamento `offset_ns`.
pub fn exact_time_to_local_date_and_time(exact_time: ExactTime, offset_ns: i64) -> PlainDateTime {
    let local = exact_time.epoch_nanoseconds() + i128::from(offset_ns);
    let days = local.div_euclid(ExactTime::NS_PER_DAY);
    let mut rest = local.rem_euclid(ExactTime::NS_PER_DAY);
    let (year, month, day) = ul_common::time::civil_from_days(days as i64);
    let mut take = |weight: i128| {
        let value = (rest / weight) as u32;
        rest %= weight;
        value
    };
    let hour = take(ExactTime::NS_PER_HOUR);
    let minute = take(ExactTime::NS_PER_MINUTE);
    let second = take(NS_PER_SECOND);
    let millisecond = take(ExactTime::NS_PER_MILLISECOND);
    let microsecond = take(ExactTime::NS_PER_MICROSECOND);
    PlainDateTime {
        date: PlainDate::new(year, month as u32, day as u32),
        time: PlainTime::new(hour, minute, second, millisecond, microsecond, rest as u32),
    }
}

/// `getOffsetNanosecondsFor(timeZone, exactTime)` (`GetOffsetNanosecondsFor`):
/// https://tc39.es/proposal-temporal/#sec-temporal-getoffsetnanosecondsfor
pub fn get_offset_nanoseconds_for(time_zone: &TimeZone, exact_time: ExactTime) -> TemporalResult<i64> {
    match time_zone {
        // Passo 2: o deslocamento do identificador.
        TimeZone::UtcOffset(offset) => Ok(*offset),
        // Passo 3: `GetNamedTimeZoneOffsetNanoseconds`.
        TimeZone::Id(id) => {
            let seconds = exact_time.floor_epoch_milliseconds().div_euclid(1000);
            Ok(offset_seconds_at(&id.zone, seconds) * NS_PER_SECOND as i64)
        }
    }
}

/// `getISODateTimeFor(timeZone, epochNs)` (`GetISODateTimeFor`):
/// https://tc39.es/proposal-temporal/#sec-temporal-getisodatetimefor
pub fn get_iso_date_time_for(time_zone: &TimeZone, epoch_ns: ExactTime) -> TemporalResult<PlainDateTime> {
    // Passo 1: `offsetNs`. Passos 2 e 3: a data e a hora.
    let offset_ns = get_offset_nanoseconds_for(time_zone, epoch_ns)?;
    Ok(exact_time_to_local_date_and_time(epoch_ns, offset_ns))
}

/// `GapOffsets`: os deslocamentos (em nanossegundos) de antes e de depois da lacuna do horário de verão.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GapOffsets {
    pub before_ns: i64,
    pub after_ns: i64,
}

/// `PossibleEpochNanoseconds`: os 0, 1 ou 2 instantes de uma data e hora locais num fuso.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PossibleEpochNanoseconds {
    /// A hora local não existe (lacuna do horário de verão).
    Gap(GapOffsets),
    /// Um instante só.
    Single(ExactTime),
    /// Sobreposição: `[mais cedo, mais tarde]`.
    Fold([ExactTime; 2]),
}

impl PossibleEpochNanoseconds {
    /// `isGap(possible)`.
    pub fn is_gap(&self) -> bool {
        matches!(self, PossibleEpochNanoseconds::Gap(_))
    }

    /// `epochCandidates(possible)`: vazio na lacuna, 1 ou 2.
    pub fn candidates(&self) -> &[ExactTime] {
        match self {
            PossibleEpochNanoseconds::Gap(_) => &[],
            PossibleEpochNanoseconds::Single(exact_time) => std::slice::from_ref(exact_time),
            PossibleEpochNanoseconds::Fold(candidates) => candidates,
        }
    }
}

/// `getPossibleEpochNanosecondsFor(timeZone, date, time)` (`GetPossibleEpochNanoseconds`):
/// https://tc39.es/proposal-temporal/#sec-temporal-getpossibleepochnanoseconds
pub fn get_possible_epoch_nanoseconds_for(time_zone: &TimeZone, date: PlainDate, time: PlainTime) -> TemporalResult<PossibleEpochNanoseconds> {
    // O instante da hora local lida como UTC.
    let local_ns = get_utc_epoch_nanoseconds(date, time);

    let possible = match time_zone {
        // Passo 2: deslocamento fixo, um candidato só.
        TimeZone::UtcOffset(offset) => PossibleEpochNanoseconds::Single(ExactTime::new(local_ns - i128::from(*offset))),
        // Passo 3: `GetNamedTimeZoneEpochNanoseconds`: normal, lacuna ou sobreposição.
        TimeZone::Id(id) => {
            let local_seconds = local_ns.div_euclid(NS_PER_SECOND) as i64;
            let (before, after) = local_offsets_seconds(&id.zone, local_seconds);
            let instant = |offset_seconds: i64| ExactTime::new(local_ns - i128::from(offset_seconds) * NS_PER_SECOND);
            if before == after {
                PossibleEpochNanoseconds::Single(instant(before))
            } else if after > before {
                PossibleEpochNanoseconds::Gap(GapOffsets { before_ns: before * NS_PER_SECOND as i64, after_ns: after * NS_PER_SECOND as i64 })
            } else {
                PossibleEpochNanoseconds::Fold([instant(before), instant(after)])
            }
        }
    };

    // Passo 4: `IsValidEpochNanoseconds` falso em algum candidato é `RangeError`.
    if possible.candidates().iter().any(|candidate| !candidate.is_valid()) {
        return Err(range_error(EPOCH_NANOSECONDS_OUT_OF_RANGE));
    }
    // Passo 5.
    Ok(possible)
}

/// `getTimeZoneTransition(timeZone, exactTime, direction)` (`GetNamedTimeZoneNextTransition` e
/// `GetNamedTimeZonePreviousTransition`): a próxima transição que muda o deslocamento depois do instante, ou a
/// anterior; `None` sem transição (e para deslocamento fixo).
pub fn get_time_zone_transition(time_zone: &TimeZone, exact_time: ExactTime, direction: TransitionDirection) -> TemporalResult<Option<ExactTime>> {
    let TimeZone::Id(id) = time_zone else { return Ok(None) };
    let zone = &id.zone;

    // O ponto de partida: o piso (próxima) ou o teto (anterior) em segundos, que é a granularidade das transições.
    let epoch_ns = exact_time.epoch_nanoseconds();
    let seconds = match direction {
        TransitionDirection::Next => epoch_ns.div_euclid(NS_PER_SECOND),
        TransitionDirection::Previous => (epoch_ns + NS_PER_SECOND - 1).div_euclid(NS_PER_SECOND),
    } as i64;
    let Ok(start) = Timestamp::from_second(seconds) else { return Ok(None) };

    // Até 20 vezes, para pular as transições que só mudam regra ou abreviação.
    let candidates: Box<dyn Iterator<Item = Timestamp> + '_> = match direction {
        TransitionDirection::Next => Box::new(zone.following(start).map(|transition| transition.timestamp())),
        TransitionDirection::Previous => Box::new(zone.preceding(start).map(|transition| transition.timestamp())),
    };
    for timestamp in candidates.take(20) {
        let transition_seconds = timestamp.as_second();
        let transition = ExactTime::new(i128::from(transition_seconds) * NS_PER_SECOND);
        if !transition.is_valid() {
            return Ok(None);
        }
        if offset_seconds_at(zone, transition_seconds - 1) != offset_seconds_at(zone, transition_seconds) {
            return Ok(Some(transition));
        }
    }
    Ok(None)
}

/// `disambiguatePossibleEpochNanoseconds(possible, timeZone, date, time, disambiguation)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-disambiguatepossibleepochnanoseconds
fn disambiguate_possible_epoch_nanoseconds(
    possible: &PossibleEpochNanoseconds,
    time_zone: &TimeZone,
    date: PlainDate,
    time: PlainTime,
    disambiguation: TemporalDisambiguation,
) -> TemporalResult<ExactTime> {
    match possible {
        // Passo 2: um candidato só.
        PossibleEpochNanoseconds::Single(exact_time) => Ok(*exact_time),
        // Passo 3: sobreposição.
        PossibleEpochNanoseconds::Fold(fold) => match disambiguation {
            TemporalDisambiguation::Reject => Err(range_error("ambiguous instant: use a 'disambiguation' option to resolve")),
            TemporalDisambiguation::Earlier | TemporalDisambiguation::Compatible => Ok(fold[0]),
            TemporalDisambiguation::Later => Ok(fold[1]),
        },
        // Passo 4: lacuna.
        PossibleEpochNanoseconds::Gap(gap) => {
            // Passo 5.
            if disambiguation == TemporalDisambiguation::Reject {
                return Err(range_error("nonexistent instant: local time does not exist in this time zone (DST gap)"));
            }
            // Passos 14 e 15: `nanoseconds = offsetAfter - offsetBefore`.
            let nanoseconds = i128::from(gap.after_ns - gap.before_ns);
            let is_earlier = disambiguation == TemporalDisambiguation::Earlier;
            // Passos 16.a a 16.d e 18 a 21: a hora local deslocada de `∓nanoseconds`.
            let time_of_day_ns = i128::from(time.hour()) * ExactTime::NS_PER_HOUR
                + i128::from(time.minute()) * ExactTime::NS_PER_MINUTE
                + i128::from(time.second()) * NS_PER_SECOND
                + i128::from(time.millisecond()) * ExactTime::NS_PER_MILLISECOND
                + i128::from(time.microsecond()) * ExactTime::NS_PER_MICROSECOND
                + i128::from(time.nanosecond());
            let shifted_ns = time_of_day_ns + if is_earlier { -nanoseconds } else { nanoseconds };
            let day_shift = shifted_ns.div_euclid(ExactTime::NS_PER_DAY);
            let shifted_time = plain_time_from_subday_ns(shifted_ns.rem_euclid(ExactTime::NS_PER_DAY));
            let shifted_date = add_days_to_iso_date(date, day_shift as i64);
            // Passos 16.e e 22: reentrar traz o `IsValidEpochNanoseconds` do passo 5 de `GetPossibleEpochNanoseconds`.
            let shifted_possible = get_possible_epoch_nanoseconds_for(time_zone, shifted_date, shifted_time)?;
            let candidates = shifted_possible.candidates();
            // Passos 16.f a 16.g e 23 a 25: a hora deslocada já passou da transição, então existe.
            match (candidates.first(), candidates.last()) {
                (Some(first), Some(last)) => Ok(if is_earlier { *first } else { *last }),
                _ => Err(range_error("nonexistent instant: local time does not exist in this time zone (DST gap)")),
            }
        }
    }
}

/// `getEpochNanosecondsFor(timeZone, date, time, disambiguation)` (`GetEpochNanosecondsFor`):
/// https://tc39.es/proposal-temporal/#sec-temporal-getepochnanosecondsfor
pub fn get_epoch_nanoseconds_for(
    time_zone: &TimeZone,
    date: PlainDate,
    time: PlainTime,
    disambiguation: TemporalDisambiguation,
) -> TemporalResult<ExactTime> {
    // Passo 1: `GetPossibleEpochNanoseconds`. Passo 2: `DisambiguatePossibleEpochNanoseconds`.
    let possible = get_possible_epoch_nanoseconds_for(time_zone, date, time)?;
    disambiguate_possible_epoch_nanoseconds(&possible, time_zone, date, time, disambiguation)
}

/// `addZonedDateTime(startEpochNs, timeZone, calendar, duration, overflow)` (`AddZonedDateTime`):
/// https://tc39.es/proposal-temporal/#sec-temporal-addzoneddatetime
pub fn add_zoned_date_time(
    start_epoch_ns: ExactTime,
    time_zone: &TimeZone,
    calendar_id: CalendarID,
    duration: &Duration,
    overflow: TemporalOverflow,
) -> TemporalResult<ExactTime> {
    // A parte de tempo em nanossegundos (a `AddInstant` dos passos 1 e 7).
    let norm = to_internal_duration(duration).time();
    let add_time_duration = |epoch_ns: ExactTime| {
        let result = ExactTime::new(epoch_ns.epoch_nanoseconds() + norm);
        if !result.is_valid() {
            return Err(range_error("Duration addition results in an out-of-range ZonedDateTime"));
        }
        Ok(result)
    };

    // Passo 1: sem componentes de data, é só `AddInstant`.
    if duration.years() == 0 && duration.months() == 0 && duration.weeks() == 0 && duration.days() == 0 {
        if norm == 0 {
            return Ok(start_epoch_ns);
        }
        return add_time_duration(start_epoch_ns);
    }

    // Passo 2: `isoDateTime = GetISODateTimeFor(timeZone, epochNanoseconds)`.
    let PlainDateTime { date, time } = get_iso_date_time_for(time_zone, start_epoch_ns)?;

    // Passo 3: `addedDate = CalendarDateAdd(calendar, isoDateTime.[[ISODate]], duration.[[Date]], overflow)`.
    let date_duration = Duration::new(duration.years(), duration.months(), duration.weeks(), duration.days(), 0, 0, 0, 0, 0, 0);
    let added_date = calendar_date_add(calendar_id, date, &date_duration, overflow)?;

    // Passos 4 e 5: `ISODateTimeWithinLimits(intermediateDateTime)` falso é `RangeError`.
    if !is_date_time_within_limits(
        added_date.year(),
        added_date.month(),
        added_date.day(),
        time.hour(),
        time.minute(),
        time.second(),
        time.millisecond(),
        time.microsecond(),
        time.nanosecond(),
    ) {
        return Err(range_error("intermediate datetime out of range"));
    }

    // Passo 6: `intermediateNs = GetEpochNanosecondsFor(timeZone, intermediateDateTime, ~compatible~)`.
    let intermediate = get_epoch_nanoseconds_for(time_zone, added_date, time, TemporalDisambiguation::Compatible)?;

    // Passo 7: `AddInstant(intermediateNs, duration.[[Time]])`.
    add_time_duration(intermediate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(name: &str) -> TimeZone {
        TimeZone::Id(intl_resolve_time_zone_id(name.as_bytes()).unwrap_or_else(|| panic!("fuso {name} desconhecido")))
    }

    fn utc_instant(date: PlainDate, time: PlainTime) -> ExactTime {
        ExactTime::new(get_utc_epoch_nanoseconds(date, time))
    }

    #[test]
    fn resolves_names_without_case_and_keeps_aliases() {
        assert_eq!(intl_resolve_time_zone_id(b"america/sao_paulo").unwrap().name(), "America/Sao_Paulo");
        assert_eq!(intl_resolve_time_zone_id(b"asia/calcutta").unwrap().name(), "Asia/Calcutta");
        assert_eq!(intl_resolve_time_zone_id(b"utc").unwrap().name(), "UTC");
        assert!(intl_resolve_time_zone_id(b"Not/AZone").is_none());
        assert!(intl_resolve_time_zone_id("Am\u{e9}rica/Sao_Paulo".as_bytes()).is_none());
    }

    #[test]
    fn displays_names_and_offsets() {
        assert_eq!(zone("America/New_York").to_string(), "America/New_York");
        assert_eq!(TimeZone::UtcOffset(-3 * 3_600_000_000_000).to_string(), "-03:00");
        assert_eq!(TimeZone::UtcOffset(5 * 3_600_000_000_000 + 30 * 60_000_000_000).to_string(), "+05:30");
    }

    #[test]
    fn equality_goes_through_primary_names() {
        assert!(time_zone_equals(&zone("Asia/Calcutta"), &zone("Asia/Kolkata")));
        assert!(time_zone_equals(&zone("UTC"), &zone("Etc/UTC")));
        assert!(time_zone_equals(&zone("GMT"), &zone("UTC")));
        assert!(!time_zone_equals(&zone("America/New_York"), &zone("America/Chicago")));
        assert!(!time_zone_equals(&zone("UTC"), &TimeZone::UtcOffset(0)));
        assert!(time_zone_equals(&TimeZone::UtcOffset(60_000_000_000), &TimeZone::UtcOffset(60_000_000_000)));
    }

    #[test]
    fn named_offsets_follow_the_tzdata() {
        let sao_paulo = zone("America/Sao_Paulo");
        let instant = utc_instant(PlainDate::new(2026, 1, 15), PlainTime::new(12, 0, 0, 0, 0, 0));
        assert_eq!(get_offset_nanoseconds_for(&sao_paulo, instant), Ok(-3 * 3_600_000_000_000));
        let local = get_iso_date_time_for(&sao_paulo, instant).unwrap();
        assert_eq!(local.date, PlainDate::new(2026, 1, 15));
        assert_eq!(local.time, PlainTime::new(9, 0, 0, 0, 0, 0));
        // Antes da época: o piso do dia.
        let before_epoch = exact_time_to_local_date_and_time(ExactTime::new(-1), 0);
        assert_eq!(before_epoch.date, PlainDate::new(1969, 12, 31));
        assert_eq!(before_epoch.time, PlainTime::new(23, 59, 59, 999, 999, 999));
    }

    #[test]
    fn far_future_uses_the_posix_rule() {
        let new_york = zone("America/New_York");
        // Julho do ano 20000: horário de verão.
        let instant = utc_instant(PlainDate::new(20000, 7, 15), PlainTime::new(12, 0, 0, 0, 0, 0));
        assert_eq!(get_offset_nanoseconds_for(&new_york, instant), Ok(-4 * 3_600_000_000_000));
    }

    #[test]
    fn spring_forward_gap_and_fall_back_fold() {
        let new_york = zone("America/New_York");
        let gap_time = PlainTime::new(2, 30, 0, 0, 0, 0);
        let gap_date = PlainDate::new(2024, 3, 10);
        let possible = get_possible_epoch_nanoseconds_for(&new_york, gap_date, gap_time).unwrap();
        assert_eq!(possible, PossibleEpochNanoseconds::Gap(GapOffsets { before_ns: -5 * 3_600_000_000_000, after_ns: -4 * 3_600_000_000_000 }));
        // `compatible` e `later` empurram para depois (03:30 EDT), `earlier` para antes (01:30 EST).
        let later = get_epoch_nanoseconds_for(&new_york, gap_date, gap_time, TemporalDisambiguation::Compatible).unwrap();
        assert_eq!(later, utc_instant(gap_date, PlainTime::new(7, 30, 0, 0, 0, 0)));
        let earlier = get_epoch_nanoseconds_for(&new_york, gap_date, gap_time, TemporalDisambiguation::Earlier).unwrap();
        assert_eq!(earlier, utc_instant(gap_date, PlainTime::new(6, 30, 0, 0, 0, 0)));
        assert!(get_epoch_nanoseconds_for(&new_york, gap_date, gap_time, TemporalDisambiguation::Reject).is_err());

        let fold_date = PlainDate::new(2024, 11, 3);
        let fold_time = PlainTime::new(1, 30, 0, 0, 0, 0);
        let possible = get_possible_epoch_nanoseconds_for(&new_york, fold_date, fold_time).unwrap();
        let first = utc_instant(fold_date, PlainTime::new(5, 30, 0, 0, 0, 0));
        let second = utc_instant(fold_date, PlainTime::new(6, 30, 0, 0, 0, 0));
        assert_eq!(possible, PossibleEpochNanoseconds::Fold([first, second]));
        assert_eq!(get_epoch_nanoseconds_for(&new_york, fold_date, fold_time, TemporalDisambiguation::Compatible), Ok(first));
        assert_eq!(get_epoch_nanoseconds_for(&new_york, fold_date, fold_time, TemporalDisambiguation::Later), Ok(second));
        assert!(get_epoch_nanoseconds_for(&new_york, fold_date, fold_time, TemporalDisambiguation::Reject).is_err());
    }

    #[test]
    fn fixed_offsets_have_one_candidate_and_no_transitions() {
        let offset = TimeZone::UtcOffset(-3 * 3_600_000_000_000);
        let date = PlainDate::new(2020, 6, 1);
        let time = PlainTime::new(12, 0, 0, 0, 0, 0);
        let possible = get_possible_epoch_nanoseconds_for(&offset, date, time).unwrap();
        assert_eq!(possible, PossibleEpochNanoseconds::Single(utc_instant(date, PlainTime::new(15, 0, 0, 0, 0, 0))));
        assert_eq!(get_time_zone_transition(&offset, ExactTime::new(0), TransitionDirection::Next), Ok(None));
        // Fora da faixa de Temporal.
        assert!(get_possible_epoch_nanoseconds_for(&offset, PlainDate::new(275760, 9, 13), PlainTime::new(23, 0, 0, 0, 0, 0)).is_err());
    }

    #[test]
    fn finds_the_neighbouring_transitions() {
        let new_york = zone("America/New_York");
        let start = utc_instant(PlainDate::new(2024, 1, 1), PlainTime::default());
        let next = get_time_zone_transition(&new_york, start, TransitionDirection::Next).unwrap().unwrap();
        assert_eq!(next, utc_instant(PlainDate::new(2024, 3, 10), PlainTime::new(7, 0, 0, 0, 0, 0)));
        let from = utc_instant(PlainDate::new(2024, 7, 1), PlainTime::default());
        let previous = get_time_zone_transition(&new_york, from, TransitionDirection::Previous).unwrap().unwrap();
        assert_eq!(previous, next);
        // A transição exata não é "depois" nem "antes" dela mesma.
        let after = get_time_zone_transition(&new_york, next, TransitionDirection::Next).unwrap().unwrap();
        assert_eq!(after, utc_instant(PlainDate::new(2024, 11, 3), PlainTime::new(6, 0, 0, 0, 0, 0)));
        let before = get_time_zone_transition(&new_york, after, TransitionDirection::Previous).unwrap().unwrap();
        assert_eq!(before, next);
    }

    #[test]
    fn adds_durations_across_the_gap() {
        let new_york = zone("America/New_York");
        let start = utc_instant(PlainDate::new(2024, 3, 9), PlainTime::new(7, 30, 0, 0, 0, 0)); // 02:30 EST
        let one_day = Duration::new(0, 0, 0, 1, 0, 0, 0, 0, 0, 0);
        // 02:30 do dia seguinte não existe: o `compatible` empurra para 03:30 EDT.
        let iso = crate::runtime::temporal_calendar::ISO8601_CALENDAR_ID;
        let result = add_zoned_date_time(start, &new_york, iso, &one_day, TemporalOverflow::Constrain).unwrap();
        assert_eq!(result, utc_instant(PlainDate::new(2024, 3, 10), PlainTime::new(7, 30, 0, 0, 0, 0)));
        // 24 horas são 24 horas exatas.
        let day_of_hours = Duration::new(0, 0, 0, 0, 24, 0, 0, 0, 0, 0);
        let result = add_zoned_date_time(start, &new_york, iso, &day_of_hours, TemporalOverflow::Constrain).unwrap();
        assert_eq!(result, utc_instant(PlainDate::new(2024, 3, 10), PlainTime::new(7, 30, 0, 0, 0, 0)));
        assert_eq!(add_zoned_date_time(start, &new_york, iso, &Duration::default(), TemporalOverflow::Constrain), Ok(start));
    }
}
