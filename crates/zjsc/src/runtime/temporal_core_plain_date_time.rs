//! Porte de `runtime/temporal/core/PlainDateTimeCore.{h,cpp}`: `compareISODateTime`,
//! `differencePlainDateTimeWithRounding` e `differenceTemporalPlainDateTime`, e o `isDateTimeWithinLimits` sobre
//! um par data e hora (`iso_date_time_within_limits`), que o `.cpp` de `TemporalPlainDateTime` escreve com os nove
//! campos soltos.
//!
//! DIVERGÊNCIA: as funções não recebem o `CalendarID`: só o calendário ISO está portado (ver
//! `temporal_calendar.rs`), então o ramo não ISO de `differencePlainDateTimeWithRounding` (que repete a lógica de
//! `diffISODateTime` com `calendarDateUntil`) não existe, e `roundRelativeDuration` é o sem fuso e sem calendário.

use crate::runtime::iso8601::{is_date_time_within_limits, Duration, InternalDuration, PlainDate, PlainTime};
use crate::runtime::temporal_calendar::CalendarID;
use crate::runtime::temporal_core_duration::{
    get_utc_epoch_nanoseconds, nudge_to_calendar_unit, round_relative_duration, temporal_duration_from_internal, total_time_duration,
};
use crate::runtime::temporal_core_iso_date::{diff_iso_date_time, iso_date_compare};
use crate::runtime::temporal_core_types::{range_error, TemporalResult};
use crate::runtime::temporal_object::{DifferenceOperation, RoundingMode, TemporalUnit};
use crate::runtime::temporal_plain_time::TemporalPlainTime;

/// `ISO8601::isDateTimeWithinLimits(date.year(), ..., time.nanosecond())`: `ISODateTimeWithinLimits`.
pub fn iso_date_time_within_limits(date: PlainDate, time: PlainTime) -> bool {
    is_date_time_within_limits(
        date.year(),
        date.month(),
        date.day(),
        time.hour(),
        time.minute(),
        time.second(),
        time.millisecond(),
        time.microsecond(),
        time.nanosecond(),
    )
}

/// `compareISODateTime(d1, t1, d2, t2)` (`CompareISODateTime`):
/// https://tc39.es/proposal-temporal/#sec-temporal-compareisodatetime
pub fn compare_iso_date_time(d1: PlainDate, t1: PlainTime, d2: PlainDate, t2: PlainTime) -> i32 {
    // Passos 1 e 2: `CompareISODate`, se diferente de zero é o resultado.
    let date_result = iso_date_compare(d1, d2);
    if date_result != 0 {
        return date_result;
    }
    // Passo 3: `CompareTimeRecord`.
    TemporalPlainTime::compare(t1, t2)
}

/// `differencePlainDateTimeWithRounding(thisDate, thisTime, otherDate, otherTime, largestUnit, smallestUnit,
/// roundingMode, increment)` (`DifferencePlainDateTimeWithRounding`):
/// https://tc39.es/proposal-temporal/#sec-temporal-differenceplaindatetimewithrounding
#[allow(clippy::too_many_arguments)]
pub fn difference_plain_date_time_with_rounding(
    calendar_id: CalendarID,
    this_date: PlainDate,
    this_time: PlainTime,
    other_date: PlainDate,
    other_time: PlainTime,
    largest_unit: TemporalUnit,
    smallest_unit: TemporalUnit,
    rounding_mode: RoundingMode,
    increment: f64,
) -> TemporalResult<InternalDuration> {
    // Passo 1: `CompareISODateTime = 0` devolve a duração interna zero.
    if this_date == other_date && this_time == other_time {
        return Ok(InternalDuration::default());
    }

    // Passo 2: um dos extremos fora de `ISODateTimeWithinLimits` é `RangeError`.
    if !iso_date_time_within_limits(this_date, this_time) || !iso_date_time_within_limits(other_date, other_time) {
        return Err(range_error("date-time is outside the representable range for Temporal"));
    }

    // Passo 3: `diff = DifferenceISODateTime(isoDateTime1, isoDateTime2, calendar, largestUnit)`.
    let mut diff = diff_iso_date_time(calendar_id, this_date, this_time, other_date, other_time, largest_unit)?;

    // Passo 4: `smallestUnit` nanossegundo com incremento 1 devolve `diff`.
    if smallest_unit == TemporalUnit::Nanosecond && increment == 1.0 {
        return Ok(diff);
    }

    // Passos 5 e 6: `originEpochNs` e `destEpochNs` (`GetUTCEpochNanoseconds`).
    let origin_epoch_ns = get_utc_epoch_nanoseconds(this_date, this_time);
    let dest_epoch_ns = get_utc_epoch_nanoseconds(other_date, other_time);

    // Passo 7: `RoundRelativeDuration`, que arredonda `diff` no lugar.
    round_relative_duration(
        calendar_id,
        &mut diff,
        origin_epoch_ns,
        dest_epoch_ns,
        this_date,
        this_time,
        largest_unit,
        increment,
        smallest_unit,
        rounding_mode,
    )?;
    Ok(diff)
}

/// `differencePlainDateTimeWithTotal(plainDate, target, unit)` (`DifferencePlainDateTimeWithTotal`) de
/// `TemporalDuration.cpp`: https://tc39.es/proposal-temporal/#sec-temporal-differenceplaindatetimewithtotal
/// O extremo inicial é a meia-noite de `plain_date` (o `relativeTo` do `Duration`, sem fuso, então `~day~` cai no
/// caminho de nanossegundos).
pub fn difference_plain_date_time_with_total(
    calendar_id: CalendarID,
    plain_date: PlainDate,
    target_date: PlainDate,
    target_time: PlainTime,
    unit: TemporalUnit,
) -> TemporalResult<f64> {
    let midnight = PlainTime::default();
    // Passos 5 e 6: os instantes dos dois extremos.
    let origin_epoch_ns = get_utc_epoch_nanoseconds(plain_date, midnight);
    let dest_epoch_ns = get_utc_epoch_nanoseconds(target_date, target_time);

    // Passo 1: extremos iguais dão 0.
    if origin_epoch_ns == dest_epoch_ns {
        return Ok(0.0);
    }

    // Passo 2: um dos extremos fora de `ISODateTimeWithinLimits` é `RangeError`.
    if !iso_date_time_within_limits(plain_date, midnight) || !iso_date_time_within_limits(target_date, target_time) {
        return Err(range_error("date time is out of range of ECMAScript representation"));
    }

    // Passo 7: unidade de dia ou de tempo é só a diferença em nanossegundos.
    if unit >= TemporalUnit::Day {
        return Ok(total_time_duration(dest_epoch_ns - origin_epoch_ns, unit));
    }

    let diff = diff_iso_date_time(calendar_id, plain_date, midnight, target_date, target_time, unit)?;
    let sign = if dest_epoch_ns < origin_epoch_ns { -1 } else { 1 };
    let nudged = nudge_to_calendar_unit(
        calendar_id,
        sign,
        &diff,
        origin_epoch_ns,
        dest_epoch_ns,
        plain_date,
        midnight,
        1.0,
        unit,
        RoundingMode::Trunc,
        None,
    )?;
    Ok(nudged.total)
}

/// `differenceTemporalPlainDateTime(op, thisDate, thisTime, otherDate, otherTime, smallestUnit, largestUnit,
/// roundingMode, increment)` (`DifferenceTemporalPlainDateTime`, passos 5 a 9):
/// https://tc39.es/proposal-temporal/#sec-temporal-differencetemporalplaindatetime
/// Os passos 1 a 4 (`ToTemporalDateTime`, `CalendarEquals`, `GetOptionsObject` e `GetDifferenceSettings`) são da
/// camada JS.
#[allow(clippy::too_many_arguments)]
pub fn difference_temporal_plain_date_time(
    calendar_id: CalendarID,
    operation: DifferenceOperation,
    this_date: PlainDate,
    this_time: PlainTime,
    other_date: PlainDate,
    other_time: PlainTime,
    smallest_unit: TemporalUnit,
    largest_unit: TemporalUnit,
    rounding_mode: RoundingMode,
    increment: f64,
) -> TemporalResult<Duration> {
    debug_assert!(largest_unit <= smallest_unit);
    debug_assert!(increment >= 1.0);

    // Passo 5: `CompareISODateTime = 0` devolve a duração zero.
    if this_date == other_date && this_time == other_time {
        return Ok(Duration::default());
    }

    // Passo 6: `internalDuration = DifferencePlainDateTimeWithRounding(...)`, ciente do calendário da célula.
    let internal_duration = difference_plain_date_time_with_rounding(
        calendar_id,
        this_date,
        this_time,
        other_date,
        other_time,
        largest_unit,
        smallest_unit,
        rounding_mode,
        increment,
    )?;

    // Passo 7: `result = TemporalDurationFromInternal(internalDuration, largestUnit)`.
    let mut result = temporal_duration_from_internal(&internal_duration, largest_unit)?;

    // Passo 8: `since` nega o resultado. Passo 9.
    if operation == DifferenceOperation::Since {
        result = -result;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::temporal_calendar::ISO8601_CALENDAR_ID;

    fn time(hour: u32, minute: u32, second: u32, millisecond: u32, microsecond: u32, nanosecond: u32) -> PlainTime {
        PlainTime::new(hour, minute, second, millisecond, microsecond, nanosecond)
    }

    #[test]
    fn compares_date_then_time() {
        let date = PlainDate::new(2020, 1, 1);
        assert_eq!(compare_iso_date_time(date, time(23, 0, 0, 0, 0, 0), PlainDate::new(2020, 1, 2), time(0, 0, 0, 0, 0, 0)), -1);
        assert_eq!(compare_iso_date_time(date, time(1, 0, 0, 0, 0, 1), date, time(1, 0, 0, 0, 0, 0)), 1);
        assert_eq!(compare_iso_date_time(date, time(1, 2, 3, 4, 5, 6), date, time(1, 2, 3, 4, 5, 6)), 0);
    }

    #[test]
    fn date_time_limits_have_one_day_of_slack() {
        let midnight = time(0, 0, 0, 0, 0, 0);
        // O limite é aberto (`ns <= min - 1 dia`): -271821-04-19T00:00 já está fora (o bun lança
        // RangeError em `new Temporal.PlainDateTime(-271821, 4, 19)`), um nanossegundo depois passa.
        assert!(!iso_date_time_within_limits(PlainDate::new(-271821, 4, 19), midnight));
        assert!(iso_date_time_within_limits(PlainDate::new(-271821, 4, 19), time(0, 0, 0, 0, 0, 1)));
        assert!(iso_date_time_within_limits(PlainDate::new(-271821, 4, 20), midnight));
        assert!(!iso_date_time_within_limits(PlainDate::new(-271821, 4, 18), time(23, 59, 59, 999, 999, 999)));
        assert!(iso_date_time_within_limits(PlainDate::new(275760, 9, 13), time(23, 59, 59, 999, 999, 999)));
        assert!(!iso_date_time_within_limits(PlainDate::new(275760, 9, 14), midnight));
        assert!(!iso_date_time_within_limits(PlainDate::sentinel(), midnight));
    }

    #[test]
    fn difference_balances_into_the_largest_unit() {
        let one_date = PlainDate::new(2020, 1, 1);
        let two_date = PlainDate::new(2020, 1, 2);
        let until = difference_temporal_plain_date_time(
            ISO8601_CALENDAR_ID,
            DifferenceOperation::Until,
            one_date,
            time(12, 0, 0, 0, 0, 0),
            two_date,
            time(10, 30, 0, 0, 0, 0),
            TemporalUnit::Nanosecond,
            TemporalUnit::Day,
            RoundingMode::Trunc,
            1.0,
        )
        .unwrap();
        // 22h30 de diferença: o ajuste de sinal recua o dia (passo 7 de `DifferenceISODateTime`).
        assert_eq!((until.days(), until.hours(), until.minutes()), (0, 22, 30));
        let since = difference_temporal_plain_date_time(
            ISO8601_CALENDAR_ID,
            DifferenceOperation::Since,
            one_date,
            time(12, 0, 0, 0, 0, 0),
            two_date,
            time(10, 30, 0, 0, 0, 0),
            TemporalUnit::Nanosecond,
            TemporalUnit::Day,
            RoundingMode::Trunc,
            1.0,
        )
        .unwrap();
        assert_eq!((since.days(), since.hours(), since.minutes()), (0, -22, -30));
        let zero = difference_temporal_plain_date_time(
            ISO8601_CALENDAR_ID,
            DifferenceOperation::Until,
            one_date,
            time(1, 0, 0, 0, 0, 0),
            one_date,
            time(1, 0, 0, 0, 0, 0),
            TemporalUnit::Nanosecond,
            TemporalUnit::Day,
            RoundingMode::Trunc,
            1.0,
        )
        .unwrap();
        assert_eq!(zero, Duration::default());
    }

    #[test]
    fn difference_rounds_to_the_smallest_unit() {
        let result = difference_temporal_plain_date_time(
            ISO8601_CALENDAR_ID,
            DifferenceOperation::Until,
            PlainDate::new(2020, 1, 1),
            time(0, 0, 0, 0, 0, 0),
            PlainDate::new(2020, 1, 1),
            time(5, 40, 0, 0, 0, 0),
            TemporalUnit::Hour,
            TemporalUnit::Hour,
            RoundingMode::HalfExpand,
            1.0,
        )
        .unwrap();
        assert_eq!(result.hours(), 6);
        let day_larger = difference_temporal_plain_date_time(
            ISO8601_CALENDAR_ID,
            DifferenceOperation::Until,
            PlainDate::new(2020, 1, 1),
            time(0, 0, 0, 0, 0, 0),
            PlainDate::new(2020, 1, 3),
            time(13, 0, 0, 0, 0, 0),
            TemporalUnit::Day,
            TemporalUnit::Day,
            RoundingMode::HalfExpand,
            1.0,
        )
        .unwrap();
        assert_eq!(day_larger.days(), 3);
    }

    #[test]
    fn difference_in_hours_collapses_days() {
        let result = difference_temporal_plain_date_time(
            ISO8601_CALENDAR_ID,
            DifferenceOperation::Until,
            PlainDate::new(2020, 1, 1),
            time(0, 0, 0, 0, 0, 0),
            PlainDate::new(2020, 1, 3),
            time(1, 0, 0, 0, 0, 0),
            TemporalUnit::Nanosecond,
            TemporalUnit::Hour,
            RoundingMode::Trunc,
            1.0,
        )
        .unwrap();
        assert_eq!((result.days(), result.hours()), (0, 49));
    }

    #[test]
    fn total_is_fractional_in_the_requested_unit() {
        let start = PlainDate::new(2020, 1, 1);
        let midnight = time(0, 0, 0, 0, 0, 0);
        // Mesmo instante: zero, sem passar pelas conferências de faixa.
        let iso = ISO8601_CALENDAR_ID;
        assert_eq!(difference_plain_date_time_with_total(iso, start, start, midnight, TemporalUnit::Month).unwrap(), 0.0);
        // 45 dias e 12 horas: unidade de dia e de tempo são só nanossegundos.
        let target = PlainDate::new(2020, 2, 15);
        assert_eq!(difference_plain_date_time_with_total(iso, start, target, time(12, 0, 0, 0, 0, 0), TemporalUnit::Day).unwrap(), 45.5);
        assert_eq!(difference_plain_date_time_with_total(iso, start, target, midnight, TemporalUnit::Hour).unwrap(), 45.0 * 24.0);
        // Unidade de calendário: 1 mês e 14 dias, e fevereiro de 2020 tem 29 dias.
        let months = difference_plain_date_time_with_total(iso, start, target, midnight, TemporalUnit::Month).unwrap();
        assert!((months - (1.0 + 14.0 / 29.0)).abs() < 1e-12, "months = {months}");
        // Alvo para trás: o sinal é negativo.
        let backwards = difference_plain_date_time_with_total(iso, target, start, midnight, TemporalUnit::Month).unwrap();
        assert!(backwards < 0.0);
        // Extremo fora da faixa é `RangeError`.
        assert!(difference_plain_date_time_with_total(iso, start, PlainDate::new(-271821, 4, 18), midnight, TemporalUnit::Day).is_err());
    }

    #[test]
    fn out_of_range_endpoint_is_a_range_error() {
        let error = difference_plain_date_time_with_rounding(
            ISO8601_CALENDAR_ID,
            PlainDate::new(2020, 1, 1),
            time(0, 0, 0, 0, 0, 0),
            PlainDate::new(-271821, 4, 18),
            time(0, 0, 0, 0, 0, 0),
            TemporalUnit::Day,
            TemporalUnit::Nanosecond,
            RoundingMode::Trunc,
            1.0,
        );
        assert!(error.is_err());
    }
}
