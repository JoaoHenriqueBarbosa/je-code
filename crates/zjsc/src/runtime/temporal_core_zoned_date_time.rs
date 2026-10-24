//! Porte de `runtime/temporal/core/ZonedDateTimeCore.{h,cpp}`: `interpretISODateTimeOffset`, `getStartOfDay` e
//! `differenceZonedDateTimeWithRounding` (com `differenceZonedDateTime`), mais os enums `MatchBehaviour` e
//! `UseStartOfDay` do `.h`.
//!
//! DIVERGÊNCIA: só o calendário ISO está portado (ver `temporal_calendar.rs`), então `differenceZonedDateTime*`
//! não recebem o `CalendarID` e a diferença de datas é `diffISODate`.

use crate::runtime::iso8601::{Duration, ExactTime, InternalDuration, PlainDate, PlainDateTime, PlainTime};
use crate::runtime::temporal_calendar::CalendarID;
use crate::runtime::temporal_calendar_icu::calendar_date_until;
use crate::runtime::temporal_core_duration::{
    get_utc_epoch_nanoseconds, round_relative_duration_in_time_zone, time_duration_from_components,
};
use crate::runtime::temporal_core_iso_date::{add_days_to_iso_date, iso_date_compare};
use crate::runtime::temporal_core_rounding::round_number_to_increment_i128;
use crate::runtime::temporal_core_types::{range_error, TemporalResult, TransitionDirection};
use crate::runtime::temporal_object::{
    length_in_nanoseconds, OffsetBehaviour, RoundingMode, TemporalDisambiguation, TemporalOffsetDisambiguation, TemporalUnit,
};
use crate::runtime::temporal_time_zone::{
    get_epoch_nanoseconds_for, get_iso_date_time_for, get_possible_epoch_nanoseconds_for, get_time_zone_transition, TimeZone,
};
use crate::wtf::date_math::date_to_days_from_1970;

/// `enum class MatchBehaviour : bool`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchBehaviour {
    MatchMinutes,
    MatchExactly,
}

/// `enum class UseStartOfDay : bool`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseStartOfDay {
    No,
    Yes,
}

/// `interpretISODateTimeOffset(date, time, useStartOfDay, offsetBehaviour, offsetOpt, inlineOffsetNs,
/// matchBehaviour, timeZone, disambiguation)` (`InterpretISODateTimeOffset`):
/// https://tc39.es/proposal-temporal/#sec-temporal-interpretisodatetimeoffset
#[allow(clippy::too_many_arguments)]
pub fn interpret_iso_date_time_offset(
    date: PlainDate,
    time: PlainTime,
    use_start_of_day: UseStartOfDay,
    offset_behaviour: OffsetBehaviour,
    offset_opt: TemporalOffsetDisambiguation,
    inline_offset_ns: i64,
    match_behaviour: MatchBehaviour,
    time_zone: &TimeZone,
    disambiguation: TemporalDisambiguation,
) -> TemporalResult<ExactTime> {
    // Passo 1: `time` é `~start-of-day~`: `GetStartOfDay`.
    if use_start_of_day == UseStartOfDay::Yes {
        debug_assert!(offset_behaviour == OffsetBehaviour::Wall);
        debug_assert!(inline_offset_ns == 0);
        return get_start_of_day(time_zone, date);
    }

    // Passo 3: `wall`, ou `option` com `ignore`.
    if offset_behaviour == OffsetBehaviour::Wall || (offset_behaviour == OffsetBehaviour::Option && offset_opt == TemporalOffsetDisambiguation::Ignore) {
        return get_epoch_nanoseconds_for(time_zone, date, time, disambiguation);
    }

    // Passo 4: `exact`, ou `option` com `use`: `UTC(date, time) - offset`.
    if offset_behaviour == OffsetBehaviour::Exact || (offset_behaviour == OffsetBehaviour::Option && offset_opt == TemporalOffsetDisambiguation::Use) {
        let result = ExactTime::new(get_utc_epoch_nanoseconds(date, time) - i128::from(inline_offset_ns));
        if !result.is_valid() {
            return Err(range_error("date/time offset combination is outside the supported range for Temporal.ZonedDateTime"));
        }
        return Ok(result);
    }

    // Passos 5 e 6.
    debug_assert!(offset_behaviour == OffsetBehaviour::Option);
    debug_assert!(offset_opt == TemporalOffsetDisambiguation::Prefer || offset_opt == TemporalOffsetDisambiguation::Reject);
    // Passo 7: `CheckISODaysRange(isoDate)`.
    if date_to_days_from_1970(date.year(), i32::from(date.month()) - 1, i32::from(date.day())).abs() > 1e8 {
        return Err(range_error("wall-clock date is outside the representable range for Temporal.ZonedDateTime"));
    }

    // Passo 8: `utcEpochNanoseconds = GetUTCEpochNanoseconds(isoDateTime)`.
    let utc_epoch_ns = get_utc_epoch_nanoseconds(date, time);

    // Passo 9: `possibleEpochNs = GetPossibleEpochNanoseconds(timeZone, isoDateTime)`.
    let possible = get_possible_epoch_nanoseconds_for(time_zone, date, time)?;

    // Passo 10: cada candidato.
    let inline_offset = i128::from(inline_offset_ns);
    for candidate in possible.candidates() {
        // Passos 10.a e 10.b: `candidateOffset = utcEpochNanoseconds - candidate`.
        let candidate_offset = utc_epoch_ns - candidate.epoch_nanoseconds();
        if candidate_offset == inline_offset {
            return Ok(*candidate);
        }
        // Passo 10.c: `match-minutes` compara o deslocamento arredondado ao minuto.
        if match_behaviour == MatchBehaviour::MatchMinutes
            && round_number_to_increment_i128(candidate_offset, ExactTime::NS_PER_MINUTE, RoundingMode::HalfExpand) == inline_offset
        {
            return Ok(*candidate);
        }
    }

    // Passo 11: `reject` é `RangeError`.
    if offset_opt == TemporalOffsetDisambiguation::Reject {
        return Err(range_error("offset does not agree with timezone for the given date/time"));
    }

    // Passo 12: `DisambiguatePossibleEpochNanoseconds` (o `getEpochNanosecondsFor` recalcula os candidatos).
    get_epoch_nanoseconds_for(time_zone, date, time, disambiguation)
}

/// `getStartOfDay(timeZone, date)` (`GetStartOfDay`): https://tc39.es/proposal-temporal/#sec-temporal-getstartofday
pub fn get_start_of_day(time_zone: &TimeZone, date: PlainDate) -> TemporalResult<ExactTime> {
    // Passos 1 e 2: `possibleEpochNs` da meia-noite.
    let midnight = PlainTime::default();
    let possible = get_possible_epoch_nanoseconds_for(time_zone, date, midnight)?;
    // Passo 3: algum candidato, o primeiro.
    if let Some(candidate) = possible.candidates().first() {
        // O início do dia precisa caber na faixa (a meia-noite de +275760-09-13 em America/Vancouver passa do máximo).
        if !candidate.is_valid() {
            return Err(range_error("start of day is outside the representable range of Temporal.ZonedDateTime"));
        }
        return Ok(*candidate);
    }
    // Passos 4 e 5: a lacuna só ocorre em fuso nomeado; o primeiro instante depois dela é a próxima transição a
    // partir do último instante antes dela.
    debug_assert!(!time_zone.is_utc_offset());
    let before_gap = get_epoch_nanoseconds_for(time_zone, date, midnight, TemporalDisambiguation::Earlier)?;
    // Passos 6 e 7.
    match get_time_zone_transition(time_zone, before_gap, TransitionDirection::Next)? {
        Some(transition) => Ok(transition),
        None => Err(range_error("no start of day: time zone has no future transitions")),
    }
}

/// `differenceZonedDateTime(ns1, ns2, timeZone, largestUnit)` (`DifferenceZonedDateTime`):
/// https://tc39.es/proposal-temporal/#sec-temporal-differencezoneddatetime
fn difference_zoned_date_time(
    calendar_id: CalendarID,
    ns1: ExactTime,
    ns2: ExactTime,
    time_zone: &TimeZone,
    largest_unit: TemporalUnit,
) -> TemporalResult<InternalDuration> {
    let ns_a = ns1.epoch_nanoseconds();
    let ns_b = ns2.epoch_nanoseconds();

    // Passo 1: iguais, a duração zero.
    if ns_a == ns_b {
        return Ok(InternalDuration::new(Default::default(), 0));
    }

    // Passos 2 e 3: `GetISODateTimeFor` das duas pontas.
    let PlainDateTime { date: start_date, time: start_time } = get_iso_date_time_for(time_zone, ns1)?;
    let PlainDateTime { date: end_date, time: end_time } = get_iso_date_time_for(time_zone, ns2)?;

    // Passo 4: o mesmo dia local, só a diferença de instantes.
    if iso_date_compare(start_date, end_date) == 0 {
        return Ok(InternalDuration::new(Default::default(), ns_b - ns_a));
    }

    // Passos 5 a 7: `sign`, `maxDayCorrection` e `dayCorrection`.
    let sign: i64 = if ns_b - ns_a < 0 { 1 } else { -1 };
    let max_day_correction = if sign == -1 { 2 } else { 1 };
    let mut day_correction = 0;
    // Passo 8: `timeDuration = DifferenceTime(startTime, endTime)`.
    let time_diff = time_duration_from_components(
        f64::from(end_time.hour()) - f64::from(start_time.hour()),
        f64::from(end_time.minute()) - f64::from(start_time.minute()),
        f64::from(end_time.second()) - f64::from(start_time.second()),
        f64::from(end_time.millisecond()) - f64::from(start_time.millisecond()),
        f64::from(end_time.microsecond()) - f64::from(start_time.microsecond()),
        f64::from(end_time.nanosecond()) - f64::from(start_time.nanosecond()),
    );
    // Passo 9: `TimeDurationSign(timeDuration) = sign` conta um dia de correção.
    if i64::from(time_diff.signum() as i32) == sign {
        day_correction += 1;
    }

    // Passos 10 e 11: tenta as correções até o sinal da diferença de tempo deixar de ser `sign`.
    let mut intermediate_date = end_date;
    let mut adjusted_time_diff = time_diff;
    let mut success = false;
    while day_correction <= max_day_correction && !success {
        // Passos 11.a a 11.c.
        intermediate_date = add_days_to_iso_date(end_date, i64::from(day_correction) * sign);
        let intermediate_ns = get_epoch_nanoseconds_for(time_zone, intermediate_date, start_time, TemporalDisambiguation::Compatible)?;
        // Passos 11.d e 11.e.
        adjusted_time_diff = ns_b - intermediate_ns.epoch_nanoseconds();
        // Passo 11.f.
        if i64::from(adjusted_time_diff.signum() as i32) != sign {
            success = true;
        }
        // Passo 11.g.
        day_correction += 1;
    }
    // Passo 12.
    debug_assert!(success);

    // Passo 13: `dateLargestUnit = LargerOfTwoTemporalUnits(largestUnit, ~day~)`.
    let date_largest_unit = if largest_unit > TemporalUnit::Day { TemporalUnit::Day } else { largest_unit };
    // Passo 14: `dateDifference = CalendarDateUntil(calendar, startDate, intermediateDate, dateLargestUnit)`.
    let date_difference = calendar_date_until(calendar_id, start_date, intermediate_date, date_largest_unit)?;

    // Passo 15: `CombineDateAndTimeDuration(dateDifference, timeDuration)`.
    let date_part = Duration::new(
        date_difference.years(),
        date_difference.months(),
        date_difference.weeks(),
        date_difference.days(),
        0,
        0,
        0,
        0,
        0,
        0,
    );
    Ok(InternalDuration::new(date_part, adjusted_time_diff))
}

/// `differenceZonedDateTimeWithRounding(ns1, ns2, timeZone, largestUnit, smallestUnit, roundingMode, increment)`
/// (`DifferenceZonedDateTimeWithRounding`): https://tc39.es/proposal-temporal/#sec-temporal-differencezoneddatetimewithrounding
#[allow(clippy::too_many_arguments)]
pub fn difference_zoned_date_time_with_rounding(
    calendar_id: CalendarID,
    ns1: ExactTime,
    ns2: ExactTime,
    time_zone: &TimeZone,
    largest_unit: TemporalUnit,
    smallest_unit: TemporalUnit,
    rounding_mode: RoundingMode,
    increment: f64,
) -> TemporalResult<InternalDuration> {
    let ns_a = ns1.epoch_nanoseconds();
    let ns_b = ns2.epoch_nanoseconds();

    // Passo 1: `largestUnit` de tempo: `DifferenceInstant`, só aritmética de nanossegundos.
    if largest_unit > TemporalUnit::Day {
        let mut difference = InternalDuration::new(Default::default(), ns_b - ns_a);
        // Passo 3: sem arredondar quando `smallestUnit` é `nanosecond` e o incremento é 1.
        if smallest_unit != TemporalUnit::Nanosecond || increment != 1.0 {
            let rounded_time = round_number_to_increment_i128(
                difference.time(),
                length_in_nanoseconds(smallest_unit) * (increment.trunc() as i128),
                rounding_mode,
            );
            difference = InternalDuration::new(Default::default(), rounded_time);
        }
        return Ok(difference);
    }

    // Passo 2: `difference = DifferenceZonedDateTime(ns1, ns2, timeZone, calendar, largestUnit)`.
    let mut difference = difference_zoned_date_time(calendar_id, ns1, ns2, time_zone, largest_unit)?;

    // Passo 3: sem arredondar.
    if smallest_unit == TemporalUnit::Nanosecond && increment == 1.0 {
        return Ok(difference);
    }

    // Passo 4: `dateTime = GetISODateTimeFor(timeZone, ns1)`.
    let PlainDateTime { date: start_date, time: start_time } = get_iso_date_time_for(time_zone, ns1)?;

    // Passo 5: `RoundRelativeDuration(difference, ns1, ns2, dateTime, timeZone, calendar, largestUnit, ...)`, no lugar.
    round_relative_duration_in_time_zone(
        calendar_id,
        &mut difference,
        ns_a,
        ns_b,
        start_date,
        start_time,
        largest_unit,
        increment,
        smallest_unit,
        rounding_mode,
        Some(time_zone),
    )?;
    Ok(difference)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::temporal_calendar::ISO8601_CALENDAR_ID;
    use crate::runtime::temporal_time_zone::intl_resolve_time_zone_id;

    fn zone(name: &str) -> TimeZone {
        TimeZone::Id(intl_resolve_time_zone_id(name.as_bytes()).expect("fuso conhecido"))
    }

    fn utc_instant(date: PlainDate, time: PlainTime) -> ExactTime {
        ExactTime::new(get_utc_epoch_nanoseconds(date, time))
    }

    #[test]
    fn interprets_offsets_in_every_behaviour() {
        let new_york = zone("America/New_York");
        let date = PlainDate::new(2024, 7, 1);
        let time = PlainTime::new(12, 0, 0, 0, 0, 0);
        let noon_utc = utc_instant(date, PlainTime::new(16, 0, 0, 0, 0, 0));
        let hour = 3_600_000_000_000;
        // `exact`: `Z`, o deslocamento é 0.
        assert_eq!(
            interpret_iso_date_time_offset(
                date,
                time,
                UseStartOfDay::No,
                OffsetBehaviour::Exact,
                TemporalOffsetDisambiguation::Reject,
                0,
                MatchBehaviour::MatchMinutes,
                &new_york,
                TemporalDisambiguation::Compatible
            ),
            Ok(utc_instant(date, time))
        );
        // `option` com o deslocamento certo e com `reject`.
        let option = |offset: i64, offset_opt| {
            interpret_iso_date_time_offset(
                date,
                time,
                UseStartOfDay::No,
                OffsetBehaviour::Option,
                offset_opt,
                offset,
                MatchBehaviour::MatchExactly,
                &new_york,
                TemporalDisambiguation::Compatible,
            )
        };
        assert_eq!(option(-4 * hour, TemporalOffsetDisambiguation::Reject), Ok(noon_utc));
        assert!(option(-5 * hour, TemporalOffsetDisambiguation::Reject).is_err());
        // `prefer` com o deslocamento errado cai no fuso; `use` honra o deslocamento; `ignore` o descarta.
        assert_eq!(option(-5 * hour, TemporalOffsetDisambiguation::Prefer), Ok(noon_utc));
        assert_eq!(option(-5 * hour, TemporalOffsetDisambiguation::Use), Ok(utc_instant(date, PlainTime::new(17, 0, 0, 0, 0, 0))));
        assert_eq!(option(-5 * hour, TemporalOffsetDisambiguation::Ignore), Ok(noon_utc));
    }

    #[test]
    fn match_minutes_rounds_sub_minute_offsets() {
        // Um deslocamento fixo de +00:19:32 (como o de Amsterdã antes de 1937): a cadeia traz `+00:20`.
        let zone = TimeZone::UtcOffset(19 * 60_000_000_000 + 32_000_000_000);
        let date = PlainDate::new(1900, 1, 1);
        let time = PlainTime::new(0, 0, 0, 0, 0, 0);
        let twenty_minutes = 20 * 60_000_000_000;
        let interpret = |match_behaviour| {
            interpret_iso_date_time_offset(
                date,
                time,
                UseStartOfDay::No,
                OffsetBehaviour::Option,
                TemporalOffsetDisambiguation::Reject,
                twenty_minutes,
                match_behaviour,
                &zone,
                TemporalDisambiguation::Compatible,
            )
        };
        assert!(interpret(MatchBehaviour::MatchMinutes).is_ok());
        assert!(interpret(MatchBehaviour::MatchExactly).is_err());
    }

    #[test]
    fn start_of_day_skips_the_gap() {
        // Em São Paulo, 2018-11-04 começou à 01:00 (o relógio pulou a meia-noite).
        let sao_paulo = zone("America/Sao_Paulo");
        let start = get_start_of_day(&sao_paulo, PlainDate::new(2018, 11, 4)).unwrap();
        assert_eq!(start, utc_instant(PlainDate::new(2018, 11, 4), PlainTime::new(3, 0, 0, 0, 0, 0)));
        // Dia comum.
        let start = get_start_of_day(&sao_paulo, PlainDate::new(2018, 11, 5)).unwrap();
        assert_eq!(start, utc_instant(PlainDate::new(2018, 11, 5), PlainTime::new(2, 0, 0, 0, 0, 0)));
    }

    #[test]
    fn differences_follow_the_local_calendar() {
        let new_york = zone("America/New_York");
        // 2024-03-09T12:00 EST até 2024-03-10T12:00 EDT: um dia de calendário, só 23 horas.
        let one = utc_instant(PlainDate::new(2024, 3, 9), PlainTime::new(17, 0, 0, 0, 0, 0));
        let two = utc_instant(PlainDate::new(2024, 3, 10), PlainTime::new(16, 0, 0, 0, 0, 0));
        let days = difference_zoned_date_time_with_rounding(ISO8601_CALENDAR_ID, one, two, &new_york, TemporalUnit::Day, TemporalUnit::Nanosecond, RoundingMode::Trunc, 1.0)
            .unwrap();
        assert_eq!(days.date_duration(), Duration::new(0, 0, 0, 1, 0, 0, 0, 0, 0, 0));
        assert_eq!(days.time(), 0);
        let hours = difference_zoned_date_time_with_rounding(ISO8601_CALENDAR_ID, one, two, &new_york, TemporalUnit::Hour, TemporalUnit::Nanosecond, RoundingMode::Trunc, 1.0)
            .unwrap();
        assert_eq!(hours.time(), 23 * 3_600_000_000_000);
        // Arredondar para horas inteiras.
        let rounded = difference_zoned_date_time_with_rounding(ISO8601_CALENDAR_ID, one, two, &new_york, TemporalUnit::Hour, TemporalUnit::Hour, RoundingMode::HalfExpand, 1.0)
            .unwrap();
        assert_eq!(rounded.time(), 23 * 3_600_000_000_000);
    }
}
