//! Porte do subconjunto puro de `runtime/temporal/core/DurationArithmetic.{h,cpp}` que `Temporal.Duration`
//! usa sem `relativeTo`: `durationSign`, `absDuration`, `largestSubduration`, `timeDurationFromComponents`,
//! `splitTimeDuration`, `plainTimeFromSubdayNs`, `totalTimeDuration`, `toInternalDuration`,
//! `toInternalDurationRecord`, `toInternalDurationRecordWith24HourDays`, `toDateDurationRecordWithoutTime`,
//! `add24HourDaysToTimeDuration` e `temporalDurationFromInternal`.
//!
//! Fora desta fatia, por dependerem de `ZonedDateTime` e do calendário não ISO: nada de `DurationArithmetic.cpp`
//! que o calendário ISO e o fuso (`TimeZoneICUBridge`) já dão. O `timeZone` de `nudgeToCalendarUnit`,
//! `bubbleRelativeDuration` e `roundRelativeDuration` é `Option<&TimeZone>` (`nullptr` é `None`: o instante de uma
//! data e hora é o `getUTCEpochNanoseconds`), e `nudgeToZonedTime` é `nudge_to_zoned_time`.
//! `getUTCEpochNanoseconds`, `adjustDateDurationRecord`, `nudgeToCalendarUnit`, `nudgeToZonedTime`,
//! `nudgeToDayOrTime`, `bubbleRelativeDuration`, `roundRelativeDuration` e as structs `NudgeResult` e `Nudged` (as
//! de `Temporal.PlainDate.prototype.until` e `since`) estão no fim do arquivo.
//!
//! DIVERGÊNCIAS: `Int128` é `i128`, `CheckedInt128` é aritmética saturante (o C++ só faz `ASSERT` de que não
//! estoura: os chamadores passam durações já validadas por `isValidDuration`); `negateDuration` é o `-d` do
//! `Duration`, usado direto pelos chamadores (sem função de repasse). `nudgeToCalendarUnit`, `bubbleRelativeDuration`
//! e `roundRelativeDuration` recebem o `CalendarID` e somam e medem pelo despacho único de `temporal_calendar_icu.rs`
//! (`calendar_date_add`, `calendar_date_until`); os chamadores de `PlainDateTime`, `ZonedDateTime` e `Duration` ainda
//! passam o ISO. `round_relative_duration` fica
//! sem fuso (o `timeZone` `nullptr` do C++) e `round_relative_duration_in_time_zone` é a versão completa.

use crate::runtime::date_constructor::{make_date, make_day, make_time};
use crate::runtime::fraction_to_double::{fraction_to_double, fraction_to_double_by_double};
use crate::runtime::iso8601::{checked_cast_double_to_int128, is_valid_duration, Duration, ExactTime, InternalDuration, PlainDate, PlainTime};
use crate::runtime::math_common::is_integer;
use crate::runtime::temporal_calendar::CalendarID;
use crate::runtime::temporal_calendar_icu::{calendar_date_add, calendar_date_until};
use crate::runtime::temporal_core_iso_date::add_days_to_iso_date;
use crate::runtime::temporal_core_rounding::{apply_unsigned_rounding_mode, round_number_to_increment_i128};
use crate::runtime::temporal_core_types::{range_error, TemporalResult};
use crate::runtime::temporal_object::{
    get_unsigned_rounding_mode, is_calendar_unit, length_in_nanoseconds, RoundingMode, TemporalDisambiguation, TemporalOverflow, TemporalUnit,
};
use crate::runtime::temporal_time_zone::{get_epoch_nanoseconds_for, TimeZone};
use crate::wtf::date_math::date_to_days_from_1970;

/// `durationSign(d)`: o sinal do primeiro campo não nulo, de `years` a `nanoseconds`.
pub fn duration_sign(d: &Duration) -> i32 {
    let signs = [
        d.years().signum() as i32,
        d.months().signum() as i32,
        d.weeks().signum() as i32,
        d.days().signum() as i32,
        d.hours().signum() as i32,
        d.minutes().signum() as i32,
        d.seconds().signum() as i32,
        d.milliseconds().signum() as i32,
        d.microseconds().signum() as i32,
        d.nanoseconds().signum() as i32,
    ];
    signs.into_iter().find(|sign| *sign != 0).unwrap_or(0)
}

/// `absDuration(d)`.
pub fn abs_duration(d: &Duration) -> Duration {
    Duration::new(
        d.years().abs(),
        d.months().abs(),
        d.weeks().abs(),
        d.days().abs(),
        d.hours().abs(),
        d.minutes().abs(),
        d.seconds().abs(),
        d.milliseconds().abs(),
        d.microseconds().abs(),
        d.nanoseconds().abs(),
    )
}

/// `largestSubduration(d)` (`DefaultTemporalLargestUnit`): a maior unidade não nula, ou `Nanosecond`.
pub fn largest_subduration(d: &Duration) -> TemporalUnit {
    let mut index = 0;
    while index < TemporalUnit::ALL.len() - 1 && d.field(TemporalUnit::ALL[index]) == 0.0 {
        index += 1;
    }
    TemporalUnit::ALL[index]
}

/// `timeDurationFromComponents(hours, minutes, seconds, milliseconds, microseconds, nanoseconds)`:
/// os campos de tempo em nanossegundos totais.
pub fn time_duration_from_components(
    hours: f64,
    minutes: f64,
    seconds: f64,
    milliseconds: f64,
    microseconds: f64,
    nanoseconds: f64,
) -> i128 {
    let cast = |value: f64| checked_cast_double_to_int128(value).unwrap_or(0);
    let min = cast(minutes).saturating_add(cast(hours).saturating_mul(60));
    let sec = cast(seconds).saturating_add(min.saturating_mul(60));
    let millis = cast(milliseconds).saturating_add(sec.saturating_mul(1000));
    let micros = cast(microseconds).saturating_add(millis.saturating_mul(1000));
    let nanos = cast(nanoseconds).saturating_add(micros.saturating_mul(1000));
    debug_assert!(nanos.abs() <= InternalDuration::MAX_TIME_DURATION);
    nanos
}

/// `timeDurationFromComponents` sobre os campos de tempo de uma `Duration` (a chamada que `toInternalDuration`,
/// `toInternalDurationRecord` e `toInternalDurationRecordWith24HourDays` repetem).
fn time_duration_of(d: &Duration) -> i128 {
    time_duration_from_components(
        d.hours() as f64,
        d.minutes() as f64,
        d.seconds() as f64,
        d.milliseconds() as f64,
        d.microseconds() as f64,
        d.nanoseconds() as f64,
    )
}

/// `splitTimeDuration(timeDuration)`: `(overflowDays, subdayNs)` por divisão em piso.
pub fn split_time_duration(time_duration: i128) -> (i64, i128) {
    let ns_per_day = ExactTime::NS_PER_DAY;
    let mut overflow_days = (time_duration / ns_per_day) as i64;
    let mut remainder = time_duration % ns_per_day;
    if remainder < 0 {
        remainder += ns_per_day;
        overflow_days -= 1;
    }
    (overflow_days, remainder)
}

/// `plainTimeFromSubdayNs(ns)`: `ns` está em `[0, nsPerDay)`.
pub fn plain_time_from_subday_ns(ns: i128) -> PlainTime {
    debug_assert!((0..ExactTime::NS_PER_DAY).contains(&ns));
    let nanosecond = (ns % 1000) as u32;
    let mut remaining = ns / 1000;
    let microsecond = (remaining % 1000) as u32;
    remaining /= 1000;
    let millisecond = (remaining % 1000) as u32;
    remaining /= 1000;
    let second = (remaining % 60) as u32;
    remaining /= 60;
    let minute = (remaining % 60) as u32;
    let hour = (remaining / 60) as u32;
    PlainTime::new(hour, minute, second, millisecond, microsecond, nanosecond)
}

/// `totalTimeDuration(timeDuration, unit)`: https://tc39.es/proposal-temporal/#sec-temporal-totaltimeduration
pub fn total_time_duration(time_duration: i128, unit: TemporalUnit) -> f64 {
    let unit_length = length_in_nanoseconds(unit);
    // Todo "Length in Nanoseconds" da tabela 21 é no máximo `nsPerDay` = 8.64e13 < 2^53. O `timeDuration`
    // pode passar da faixa de inteiros seguros: `fractionToDouble` faz a emulação em software que o passo exige.
    fraction_to_double_by_double(time_duration, unit_length as f64)
}

/// `toInternalDuration(d)`: os campos de tempo viram `[[Time]]`, sem normalizar os dias.
pub fn to_internal_duration(d: &Duration) -> InternalDuration {
    InternalDuration::new(*d, time_duration_of(d))
}

/// `toInternalDurationRecord(d)`: https://tc39.es/proposal-temporal/#sec-temporal-tointernaldurationrecord
/// Os dias ficam em `[[Date]]` e só `hours` a `nanoseconds` viram `[[Time]]`.
pub fn to_internal_duration_record(d: &Duration) -> InternalDuration {
    let date_duration = Duration::new(d.years(), d.months(), d.weeks(), d.days(), 0, 0, 0, 0, 0, 0);
    InternalDuration::new(date_duration, time_duration_of(d))
}

/// `add24HourDaysToTimeDuration(d, days)`: https://tc39.es/proposal-temporal/#sec-temporal-add24hourdaystonormalizedtimeduration
/// Erro se o resultado passa de `maxTimeDuration`.
pub fn add_24_hour_days_to_time_duration(d: i128, days: f64) -> TemporalResult<i128> {
    let out_of_range = || range_error("Total time in duration is out of range");
    let days_in_nanoseconds = checked_cast_double_to_int128(days)
        .and_then(|days| days.checked_mul(ExactTime::NS_PER_DAY))
        .ok_or_else(out_of_range)?;
    let result = d.checked_add(days_in_nanoseconds).ok_or_else(out_of_range)?;
    if result.abs() > InternalDuration::MAX_TIME_DURATION {
        return Err(out_of_range());
    }
    Ok(result)
}

/// `toInternalDurationRecordWith24HourDays(d)`: os dias entram no `[[Time]]` e `[[Date]]` fica só com
/// `years`, `months` e `weeks`.
pub fn to_internal_duration_record_with_24_hour_days(d: &Duration) -> TemporalResult<InternalDuration> {
    let time_duration = add_24_hour_days_to_time_duration(time_duration_of(d), d.days() as f64)?;
    let date_duration = Duration::new(d.years(), d.months(), d.weeks(), 0, 0, 0, 0, 0, 0, 0);
    Ok(InternalDuration::new(date_duration, time_duration))
}

/// `toDateDurationRecordWithoutTime(duration)`: tira os campos de tempo e dobra os dias na parte de data.
pub fn to_date_duration_record_without_time(duration: &Duration) -> TemporalResult<Duration> {
    let internal_duration = to_internal_duration_record_with_24_hour_days(duration)?;
    let days = internal_duration.time() / ExactTime::NS_PER_DAY;
    let date_duration = internal_duration.date_duration();
    Ok(Duration::new(date_duration.years(), date_duration.months(), date_duration.weeks(), days as i64, 0, 0, 0, 0, 0, 0))
}

/// `temporalDurationFromInternal(internalDuration, largestUnit)`: a `InternalDuration` de volta em `Duration`.
pub fn temporal_duration_from_internal(internal_duration: &InternalDuration, largest_unit: TemporalUnit) -> TemporalResult<Duration> {
    let sign = i128::from(internal_duration.time_duration_sign());
    let mut nanoseconds = internal_duration.time().abs();
    let mut microseconds = 0i128;
    let mut milliseconds = 0i128;
    let mut seconds = 0i128;
    let mut minutes = 0i128;
    let mut hours = 0i128;
    let mut days = 0i128;

    // Cada unidade maior que a menor pedida absorve o quociente da anterior.
    if largest_unit <= TemporalUnit::Microsecond {
        microseconds = nanoseconds / 1000;
        nanoseconds %= 1000;
    }
    if largest_unit <= TemporalUnit::Millisecond {
        milliseconds = microseconds / 1000;
        microseconds %= 1000;
    }
    if largest_unit <= TemporalUnit::Second {
        seconds = milliseconds / 1000;
        milliseconds %= 1000;
    }
    if largest_unit <= TemporalUnit::Minute {
        minutes = seconds / 60;
        seconds %= 60;
    }
    if largest_unit <= TemporalUnit::Hour {
        hours = minutes / 60;
        minutes %= 60;
    }
    if largest_unit <= TemporalUnit::Day {
        days = hours / 24;
        hours %= 24;
    }

    // ℝ(𝔽(x)) do passo 12 de `CreateTemporalDuration`: passa por `float64` para que os `Int128` sem
    // representação exata estourem `nanosecondsLimit` e `isValidDuration` os rejeite. `milliseconds` satura em
    // `int64_t` porque `Int128 -> double` ainda pode passar dele (largestUnit milissegundo, entrada fora da faixa).
    let date_duration = internal_duration.date_duration();
    let result = Duration::new(
        date_duration.years(),
        date_duration.months(),
        date_duration.weeks(),
        date_duration.days().wrapping_add((days * sign) as i64),
        (hours * sign) as i64,
        (minutes * sign) as i64,
        (seconds * sign) as i64,
        Duration::double_to_int64_saturating((milliseconds * sign) as f64),
        ((microseconds * sign) as f64) as i128,
        ((nanoseconds * sign) as f64) as i128,
    );
    // Passo de `CreateTemporalDuration`: se `IsValidDuration` é falso, erro.
    if !is_valid_duration(&result) {
        return Err(range_error("Duration is outside the representable range"));
    }
    Ok(result)
}

/// `getUTCEpochNanoseconds(date, time)` (`IsoDateTime::as_nanoseconds` do temporal_rs, caminho UTC): a data e a
/// hora como instante, como se fossem UTC.
pub fn get_utc_epoch_nanoseconds(date: PlainDate, time: PlainTime) -> i128 {
    let day_ms = make_day(f64::from(date.year()), f64::from(date.month()) - 1.0, f64::from(date.day()));
    let time_ms = make_time(f64::from(time.hour()), f64::from(time.minute()), f64::from(time.second()), f64::from(time.millisecond()));
    let ms = make_date(day_ms, time_ms);
    debug_assert!(is_integer(ms));
    (ms as i128) * ExactTime::NS_PER_MILLISECOND
        + i128::from(time.microsecond()) * ExactTime::NS_PER_MICROSECOND
        + i128::from(time.nanosecond())
}

/// `dateDurationSign(d)`: https://tc39.es/proposal-temporal/#sec-temporal-datedurationsign (só `years`, `months`,
/// `weeks` e `days`).
fn date_duration_sign(d: &Duration) -> i32 {
    [d.years(), d.months(), d.weeks(), d.days()].into_iter().find(|field| *field != 0).map_or(0, |field| field.signum() as i32)
}

/// `InternalDuration::sign()`: o sinal da parte de data e, sendo zero, o do tempo.
pub fn internal_duration_sign(internal_duration: &InternalDuration) -> i32 {
    let sign = date_duration_sign(&internal_duration.date_duration());
    if sign != 0 {
        return sign;
    }
    internal_duration.time_duration_sign()
}

/// `adjustDateDurationRecord(dateDuration, days, weeks, months)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-adjustdatedurationrecord
/// Uma duração de data com `days`, e `weeks` e `months` se dados (senão os de `dateDuration`).
pub fn adjust_date_duration_record(date_duration: &Duration, days: i64, weeks: Option<i64>, months: Option<i64>) -> TemporalResult<Duration> {
    // Passos 1 e 2: `weeks` e `months` ausentes são os de `dateDuration`.
    let years = date_duration.years();
    let months = months.unwrap_or_else(|| date_duration.months());
    let weeks = weeks.unwrap_or_else(|| date_duration.weeks());
    // Passo 3: `CreateDateDurationRecord(dateDuration.[[Years]], months, weeks, days)`.
    let result = Duration::new(years, months, weeks, days, 0, 0, 0, 0, 0, 0);

    // Pula `isValidDuration` quando a conferência rápida dos campos dados basta.
    const FIELD_LIMIT: i64 = 1 << 32;
    const MAX_DAYS: i64 = (1 << 53) / 86400;
    let fast_path = years > -FIELD_LIMIT
        && years < FIELD_LIMIT
        && months > -FIELD_LIMIT
        && months < FIELD_LIMIT
        && weeks > -FIELD_LIMIT
        && weeks < FIELD_LIMIT
        && days > -MAX_DAYS
        && days < MAX_DAYS;
    if fast_path {
        let mut sign = 0;
        let mut check = |value: i64| {
            if sign == 0 && value != 0 {
                sign = if value > 0 { 1 } else { -1 };
            }
            !((value < 0 && sign > 0) || (value > 0 && sign < 0))
        };
        if check(years) && check(months) && check(weeks) && check(days) {
            return Ok(result);
        }
    }

    if !is_valid_duration(&result) {
        return Err(range_error("Temporal.Duration properties must be valid and of consistent sign"));
    }
    Ok(result)
}

/// `struct NudgeResult` (`NudgeResultRecord` do temporal_rs): a duração arredondada, o instante e se a unidade
/// de calendário expandiu.
#[derive(Clone, Copy, Debug, Default)]
pub struct NudgeResult {
    pub duration: InternalDuration,
    pub nudged_epoch_ns: i128,
    pub did_expand_calendar_unit: bool,
}

/// `struct Nudged` (`NudgedRecord` do temporal_rs): o `NudgeResult` e o total fracionário.
#[derive(Clone, Copy, Debug, Default)]
pub struct Nudged {
    pub nudge_result: NudgeResult,
    pub total: f64,
}

/// `struct NudgeWindow`: os limites inferior e superior (instante e duração) de um passo de `nudge` de calendário.
struct NudgeWindow {
    r1: i64,
    r2: i64,
    start_epoch_ns: i128,
    end_epoch_ns: i128,
    start_duration: Duration,
    end_duration: Duration,
}

/// `epochNanosecondsForDateAndTime(date, time, timeZone)`: o instante de `date` e `time`, `getUTCEpochNanoseconds`
/// sem fuso e `GetEpochNanosecondsFor(..., ~compatible~)` com fuso.
fn epoch_nanoseconds_for_date_and_time(date: PlainDate, time: PlainTime, time_zone: Option<&TimeZone>) -> TemporalResult<i128> {
    match time_zone {
        None => Ok(get_utc_epoch_nanoseconds(date, time)),
        Some(time_zone) => Ok(get_epoch_nanoseconds_for(time_zone, date, time, TemporalDisambiguation::Compatible)?.epoch_nanoseconds()),
    }
}

/// O instante de `date` na hora `iso_time` e a conferência de faixa de `dateToDaysFrom1970` (`1e8` dias).
fn epoch_nanoseconds_of_date_within_range(date: PlainDate, iso_time: PlainTime, time_zone: Option<&TimeZone>) -> TemporalResult<i128> {
    let day_count = date_to_days_from_1970(date.year(), i32::from(date.month()) - 1, i32::from(date.day()));
    if day_count.abs() > 1e8 {
        return Err(range_error("date is outside the representable range"));
    }
    epoch_nanoseconds_for_date_and_time(date, iso_time, time_zone)
}

/// `computeNudgeWindow(sign, duration, originEpochNs, isoDateTime, timeZone, increment, unit, additionalShift)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-computenudgewindow
#[allow(clippy::too_many_arguments)]
fn compute_nudge_window(
    calendar_id: CalendarID,
    sign: i32,
    duration: &InternalDuration,
    origin_epoch_ns: i128,
    iso_date: PlainDate,
    iso_time: PlainTime,
    increment: f64,
    unit: TemporalUnit,
    additional_shift: bool,
    time_zone: Option<&TimeZone>,
) -> TemporalResult<NudgeWindow> {
    let date_duration = duration.date_duration();
    let shift = (increment as i64) * i64::from(sign);
    let increment_wide = increment as i128;
    let (r1, r2, start_duration, end_duration);

    // Passos 1 a 4: `r1`, `r2`, `startDuration` e `endDuration` conforme `unit`.
    match unit {
        TemporalUnit::Year => {
            // Passos 1.a a 1.f.
            let years = round_number_to_increment_i128(i128::from(date_duration.years()), increment_wide, RoundingMode::Trunc);
            r1 = years as i64 + if additional_shift { shift } else { 0 };
            r2 = r1 + shift;
            start_duration = Duration::new(r1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
            end_duration = Duration::new(r2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
        TemporalUnit::Month => {
            // Passos 2.a a 2.f.
            let months = round_number_to_increment_i128(i128::from(date_duration.months()), increment_wide, RoundingMode::Trunc);
            r1 = months as i64 + if additional_shift { shift } else { 0 };
            r2 = r1 + shift;
            start_duration = adjust_date_duration_record(&date_duration, 0, Some(0), Some(r1))?;
            end_duration = adjust_date_duration_record(&date_duration, 0, Some(0), Some(r2))?;
        }
        TemporalUnit::Week => {
            // Passo 3.a: `yearsMonths = AdjustDateDurationRecord(duration.[[Date]], 0, 0)`.
            let years_months = adjust_date_duration_record(&date_duration, 0, Some(0), None)?;
            // Passo 3.b: `weeksStart = CalendarDateAdd(calendar, isoDateTime.[[ISODate]], yearsMonths, ~constrain~)`.
            let weeks_start = calendar_date_add(calendar_id, iso_date, &years_months, TemporalOverflow::Constrain)?;
            // Passo 3.c: `weeksEnd = AddDaysToISODate(weeksStart, duration.[[Date]].[[Days]])`.
            let weeks_end = add_days_to_iso_date(weeks_start, date_duration.days());
            // Passo 3.d: `untilResult = CalendarDateUntil(calendar, weeksStart, weeksEnd, ~week~)`.
            let until_result = calendar_date_until(calendar_id, weeks_start, weeks_end, TemporalUnit::Week)?;
            // Passo 3.e: `weeks = RoundNumberToIncrement(duration.[[Date]].[[Weeks]] + untilResult.[[Weeks]], increment, ~trunc~)`.
            let weeks = round_number_to_increment_i128(
                i128::from(date_duration.weeks() + until_result.weeks()),
                increment_wide,
                RoundingMode::Trunc,
            );
            // Passos 3.f e 3.g.
            r1 = weeks as i64;
            r2 = r1 + shift;
            // Passos 3.h e 3.i.
            start_duration = adjust_date_duration_record(&date_duration, 0, Some(r1), None)?;
            end_duration = adjust_date_duration_record(&date_duration, 0, Some(r2), None)?;
        }
        _ => {
            // Passo 4.a: `unit` é `~day~`.
            debug_assert!(unit == TemporalUnit::Day);
            // Passos 4.b a 4.f.
            let days = round_number_to_increment_i128(i128::from(date_duration.days()), increment_wide, RoundingMode::Trunc);
            r1 = days as i64;
            r2 = r1 + shift;
            start_duration = adjust_date_duration_record(&date_duration, r1, None, None)?;
            end_duration = adjust_date_duration_record(&date_duration, r2, None, None)?;
        }
    }

    // Passos 5 e 6: `sign` 1 tem `r1 >= 0` e `r1 < r2`; `sign` -1 tem `r1 <= 0` e `r1 > r2`.
    debug_assert!(sign != 1 || (r1 >= 0 && r1 < r2));
    debug_assert!(sign != -1 || (r1 <= 0 && r1 > r2));

    // Passos 7 e 8: `startEpochNs` é a origem só se `startDuration` é toda zero; senão sai de `CalendarDateAdd`
    // (https://github.com/tc39/proposal-temporal/issues/3316).
    let start_epoch_ns = if start_duration.years() == 0 && start_duration.months() == 0 && start_duration.weeks() == 0 && start_duration.days() == 0 {
        origin_epoch_ns
    } else {
        let start = calendar_date_add(calendar_id, iso_date, &start_duration, TemporalOverflow::Constrain)?;
        epoch_nanoseconds_of_date_within_range(start, iso_time, time_zone)?
    };
    // Passo 9: `end = CalendarDateAdd(calendar, isoDateTime.[[ISODate]], endDuration, ~constrain~)`.
    let end = calendar_date_add(calendar_id, iso_date, &end_duration, TemporalOverflow::Constrain)?;
    // Passos 10 a 12: `endDateTime` e o `GetUTCEpochNanoseconds` (ou `GetEpochNanosecondsFor` com fuso).
    let end_epoch_ns = epoch_nanoseconds_of_date_within_range(end, iso_time, time_zone)?;

    // Passos 13 a 15: as durações combinadas com zero ficam para o chamador (só o caminho escolhido precisa).
    Ok(NudgeWindow { r1, r2, start_epoch_ns, end_epoch_ns, start_duration, end_duration })
}

/// `nudgeToCalendarUnit(sign, duration, originEpochNs, destEpochNs, isoDateTime, increment, unit, roundingMode,
/// timeZone)`: https://tc39.es/proposal-temporal/#sec-temporal-nudgetocalendarunit
#[allow(clippy::too_many_arguments)]
pub fn nudge_to_calendar_unit(
    calendar_id: CalendarID,
    sign: i32,
    duration: &InternalDuration,
    origin_epoch_ns: i128,
    dest_epoch_ns: i128,
    iso_date: PlainDate,
    iso_time: PlainTime,
    increment: f64,
    unit: TemporalUnit,
    rounding_mode: RoundingMode,
    time_zone: Option<&TimeZone>,
) -> TemporalResult<Nudged> {
    // Passos 1 e 2: `didExpandCalendarUnit` falso e `nudgeWindow = ComputeNudgeWindow(..., false)`.
    let mut did_expand_calendar_unit = false;
    let mut nudge_window =
        compute_nudge_window(calendar_id, sign, duration, origin_epoch_ns, iso_date, iso_time, increment, unit, false, time_zone)?;

    // Passos 3 a 6: o destino precisa estar na janela; senão repete com `additionalShift`.
    let in_bounds = if sign == 1 {
        nudge_window.start_epoch_ns <= dest_epoch_ns && dest_epoch_ns <= nudge_window.end_epoch_ns
    } else {
        nudge_window.end_epoch_ns <= dest_epoch_ns && dest_epoch_ns <= nudge_window.start_epoch_ns
    };
    if !in_bounds {
        nudge_window = compute_nudge_window(calendar_id, sign, duration, origin_epoch_ns, iso_date, iso_time, increment, unit, true, time_zone)?;
        // A spec afirma que os limites valem depois da repetição, mas a asserção é violável
        // (https://github.com/tc39/proposal-temporal/issues/3310).
        did_expand_calendar_unit = true;
    }

    // Passo 13: `startEpochNs` e `endEpochNs` iguais só ocorrem quando um fuso pula o dia inteiro (3310); o
    // `RangeError` segue o polyfill.
    if nudge_window.start_epoch_ns == nudge_window.end_epoch_ns {
        return Err(range_error("cannot round relative to a time zone transition that skips an entire day"));
    }
    // Passo 14: `progress = (destEpochNs - startEpochNs) / (endEpochNs - startEpochNs)`.
    let progress_numerator = dest_epoch_ns - nudge_window.start_epoch_ns;
    let progress_denominator = nudge_window.end_epoch_ns - nudge_window.start_epoch_ns;
    // Passo 15: `total = r1 + progress × increment × sign`, com a conta inteira antes da divisão final.
    let total_numerator = i128::from(nudge_window.r1) * progress_denominator + progress_numerator * i128::from(increment as i64) * i128::from(sign);
    let total = fraction_to_double(total_numerator, progress_denominator.abs()) * if progress_denominator < 0 { -1.0 } else { 1.0 };
    let progress = progress_numerator / progress_denominator;
    // Passo 17: `0 <= progress <= 1`.
    debug_assert!((0..=1).contains(&progress));
    // Passos 18 e 19: `isNegative` e `unsignedRoundingMode`.
    let unsigned_rounding_mode = get_unsigned_rounding_mode(rounding_mode, sign < 0);
    // Passos 20 e 21: `progress = 1` dá `abs(r2)`; senão `ApplyUnsignedRoundingMode` sobre o racional exato (a
    // comparação é de igualdade exata, não do `total` em `double`).
    let abs_r1 = i128::from(nudge_window.r1).abs();
    let abs_r2 = i128::from(nudge_window.r2).abs();
    let mut rounded_to_r2 = true;
    if progress != 1 {
        rounded_to_r2 =
            apply_unsigned_rounding_mode(total_numerator.abs(), progress_denominator.abs(), abs_r1, abs_r2, unsigned_rounding_mode) == abs_r2;
    }
    // Passos 22 e 23: `abs(r2)` expande a unidade e usa `endDuration` e `endEpochNs`; senão os de início.
    did_expand_calendar_unit |= rounded_to_r2;
    let result_duration = if rounded_to_r2 { nudge_window.end_duration } else { nudge_window.start_duration };
    let nudged_epoch_ns = if rounded_to_r2 { nudge_window.end_epoch_ns } else { nudge_window.start_epoch_ns };
    // Passo 24: o `Duration Nudge Result Record`, com `CombineDateAndTimeDuration(resultDuration, 0)`.
    let nudge_result = NudgeResult {
        duration: InternalDuration::new(result_duration, 0),
        nudged_epoch_ns,
        did_expand_calendar_unit,
    };
    // Passo 25.
    Ok(Nudged { nudge_result, total })
}

/// `nudgeToDayOrTime(duration, destEpochNs, largestUnit, increment, smallestUnit, roundingMode)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-nudgetodayortime
pub fn nudge_to_day_or_time(
    duration: &InternalDuration,
    dest_epoch_ns: i128,
    largest_unit: TemporalUnit,
    increment: f64,
    smallest_unit: TemporalUnit,
    rounding_mode: RoundingMode,
) -> TemporalResult<NudgeResult> {
    let date_duration = duration.date_duration();
    // Passo 1: `timeDuration = ! Add24HourDaysToTimeDuration(duration.[[Time]], duration.[[Date]].[[Days]])`.
    let time_duration = add_24_hour_days_to_time_duration(duration.time(), date_duration.days() as f64)?;
    // Passo 2: o comprimento de `smallestUnit` em nanossegundos.
    let unit_length = length_in_nanoseconds(smallest_unit);
    // Passo 3: `roundedTime = RoundTimeDurationToIncrement(timeDuration, unitLength × increment, roundingMode)`.
    let rounded_time = round_number_to_increment_i128(time_duration, unit_length * (increment.trunc() as i128), rounding_mode);
    // Passo 4: `diffTime = ! AddTimeDuration(roundedTime, -timeDuration)`.
    let diff_time = rounded_time - time_duration;
    // Passos 5 e 6: `wholeDays` e `roundedWholeDays`, o truncamento de `TotalTimeDuration(..., ~day~)`. A divisão
    // inteira trunca; um `double` arredondaria e, passando de 128 dias, as distinções dos passos 7 a 9 e 13
    // ficam abaixo de um ULP.
    let ns_per_day = length_in_nanoseconds(TemporalUnit::Day);
    let whole_days = time_duration / ns_per_day;
    let rounded_whole_days = rounded_time / ns_per_day;
    // Passos 7 a 9: `dayDelta`, o sinal dele e `didExpandDays`.
    let day_delta = rounded_whole_days - whole_days;
    let day_delta_sign = day_delta.signum() as i32;
    let did_expand_days = day_delta_sign == time_duration.signum() as i32;
    // Passo 10: `nudgedEpochNs = AddTimeDurationToEpochNanoseconds(diffTime, destEpochNs)`.
    let nudged_epoch_ns = diff_time + dest_epoch_ns;
    // Passos 11 e 12: `days` e `remainder`.
    let mut days = 0i64;
    let mut remainder = rounded_time;
    // Passo 13: `TemporalUnitCategory(largestUnit)` é `~date~`.
    if largest_unit <= TemporalUnit::Day {
        days = rounded_whole_days as i64;
        remainder = rounded_time - rounded_whole_days * ns_per_day;
    }
    // Passos 14 e 15: `dateDuration = ! AdjustDateDurationRecord(duration.[[Date]], days)` e
    // `CombineDateAndTimeDuration(dateDuration, remainder)`.
    let date_duration = adjust_date_duration_record(&date_duration, days, None, None)?;
    // Passo 16.
    Ok(NudgeResult { duration: InternalDuration::new(date_duration, remainder), nudged_epoch_ns, did_expand_calendar_unit: did_expand_days })
}

/// `nudgeToZonedTime(sign, duration, isoDateTime, timeZone, increment, unit, roundingMode)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-nudgetozonedtime
#[allow(clippy::too_many_arguments)]
pub fn nudge_to_zoned_time(
    calendar_id: CalendarID,
    sign: i32,
    duration: &InternalDuration,
    iso_date: PlainDate,
    iso_time: PlainTime,
    time_zone: &TimeZone,
    increment: f64,
    unit: TemporalUnit,
    rounding_mode: RoundingMode,
) -> TemporalResult<NudgeResult> {
    let date_duration = duration.date_duration();
    // Passo 1: `start = CalendarDateAdd(calendar, isoDateTime.[[ISODate]], duration.[[Date]], ~constrain~)`.
    let start = calendar_date_add(calendar_id, iso_date, &date_duration, TemporalOverflow::Constrain)?;
    // Passos 2 a 4: `startDateTime`, `endDate = AddDaysToISODate(start, sign)` e `endDateTime`.
    let end_date = add_days_to_iso_date(start, i64::from(sign));
    // Passos 5 e 6: `GetEpochNanosecondsFor(timeZone, ..., ~compatible~)` das duas pontas.
    let start_epoch_ns = epoch_nanoseconds_for_date_and_time(start, iso_time, Some(time_zone))?;
    let end_epoch_ns = epoch_nanoseconds_for_date_and_time(end_date, iso_time, Some(time_zone))?;
    // Passo 7: `daySpan = TimeDurationFromEpochNanosecondsDifference(endEpochNs, startEpochNs)`. O passo 8 é
    // violável (um fuso que pula o dia inteiro dá `daySpan` zero, tc39/proposal-temporal#3310) e só subtrai.
    let day_span = end_epoch_ns - start_epoch_ns;
    debug_assert!(day_span == 0 || day_span.signum() as i32 == sign);
    // Passo 9: o comprimento de `unit`.
    let unit_length = length_in_nanoseconds(unit);
    let rounding_increment_ns = unit_length * (increment.trunc() as i128);
    // Passo 10: `roundedTimeDuration = RoundTimeDurationToIncrement(duration.[[Time]], increment × unitLength, mode)`.
    let mut rounded_time_duration = round_number_to_increment_i128(duration.time(), rounding_increment_ns, rounding_mode);
    // Passo 11: `beyondDaySpan = AddTimeDuration(roundedTimeDuration, -daySpan)`.
    let beyond_day_span = rounded_time_duration - day_span;
    // Passos 12 e 13: arredondou para além do dia (`TimeDurationSign(beyondDaySpan) != -sign`) ou não.
    let did_round_beyond_day = beyond_day_span.signum() as i32 != -sign;
    let day_delta;
    let nudged_epoch_ns;
    if did_round_beyond_day {
        day_delta = i64::from(sign);
        rounded_time_duration = round_number_to_increment_i128(beyond_day_span, rounding_increment_ns, rounding_mode);
        nudged_epoch_ns = rounded_time_duration + end_epoch_ns;
    } else {
        day_delta = 0;
        nudged_epoch_ns = rounded_time_duration + start_epoch_ns;
    }
    // Passo 14: `dateDuration = ! AdjustDateDurationRecord(duration.[[Date]], duration.[[Date]].[[Days]] + dayDelta)`.
    let adjusted_date_duration = adjust_date_duration_record(&date_duration, date_duration.days() + day_delta, None, None)?;
    // Passos 15 e 16: `CombineDateAndTimeDuration` e o `Duration Nudge Result Record`.
    Ok(NudgeResult {
        duration: InternalDuration::new(adjusted_date_duration, rounded_time_duration),
        nudged_epoch_ns,
        did_expand_calendar_unit: did_round_beyond_day,
    })
}

/// `bubbleRelativeDuration(sign, duration, nudgedEpochNs, isoDateTime, timeZone, largestUnit, smallestUnit)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-bubblerelativeduration
#[allow(clippy::too_many_arguments)]
pub fn bubble_relative_duration(
    calendar_id: CalendarID,
    sign: i32,
    duration: InternalDuration,
    nudged_epoch_ns: i128,
    iso_date: PlainDate,
    iso_time: PlainTime,
    largest_unit: TemporalUnit,
    smallest_unit: TemporalUnit,
    time_zone: Option<&TimeZone>,
) -> TemporalResult<InternalDuration> {
    // Passo 1: `smallestUnit` igual a `largestUnit` devolve `duration`.
    if smallest_unit == largest_unit {
        return Ok(duration);
    }
    // Passos 2 a 4: os índices na tabela de unidades (a ordem do enum) e `unitIndex = smallestUnitIndex - 1`.
    let largest_unit_index = largest_unit as i32;
    let mut unit_index = smallest_unit as i32 - 1;
    // Passo 5: `done` falso.
    let mut done = false;
    let mut duration = duration;
    // Passo 6: repete enquanto `unitIndex >= largestUnitIndex` e `done` é falso.
    while unit_index >= largest_unit_index && !done {
        // Passo 6.a: a unidade da tabela em `unitIndex`.
        let unit = TemporalUnit::ALL[unit_index as usize];
        // Passo 6.b: `unit` não é `~week~`, ou `largestUnit` é `~week~`.
        if unit != TemporalUnit::Week || largest_unit == TemporalUnit::Week {
            let date_duration = duration.date_duration();
            let end_duration = match unit {
                // Passo 6.b.i.
                TemporalUnit::Year => Duration::new(date_duration.years() + i64::from(sign), 0, 0, 0, 0, 0, 0, 0, 0, 0),
                // Passo 6.b.ii.
                TemporalUnit::Month => adjust_date_duration_record(&date_duration, 0, Some(0), Some(date_duration.months() + i64::from(sign)))?,
                // Passo 6.b.iii: `~week~`.
                _ => adjust_date_duration_record(&date_duration, 0, Some(date_duration.weeks() + i64::from(sign)), None)?,
            };
            // Passos 6.b.iv a 6.b.vi: `end = CalendarDateAdd(...)`, `endDateTime` e o `GetUTCEpochNanoseconds` (ou o
            // `GetEpochNanosecondsFor` do fuso).
            let end = calendar_date_add(calendar_id, iso_date, &end_duration, TemporalOverflow::Constrain)?;
            let end_epoch_ns = epoch_nanoseconds_for_date_and_time(end, iso_time, time_zone)?;
            // Passos 6.b.vii e 6.b.viii: `beyondEnd` e o sinal dele.
            let beyond_end_sign = (nudged_epoch_ns - end_epoch_ns).signum() as i32;
            // Passo 6.b.ix: `beyondEndSign` diferente de `-sign` troca `duration`.
            if beyond_end_sign != -sign {
                duration = InternalDuration::new(end_duration, 0);
            } else {
                // Passo 6.b.x.
                done = true;
            }
        }
        // Passo 6.c.
        unit_index -= 1;
    }
    // Passo 7.
    Ok(duration)
}

/// `roundRelativeDuration(duration, originEpochNs, destEpochNs, isoDateTime, largestUnit, increment,
/// smallestUnit, roundingMode)` com `timeZone` `nullptr`: https://tc39.es/proposal-temporal/#sec-temporal-roundrelativeduration
/// Arredonda `duration` no lugar, sem fuso.
#[allow(clippy::too_many_arguments)]
pub fn round_relative_duration(
    calendar_id: CalendarID,
    duration: &mut InternalDuration,
    origin_epoch_ns: i128,
    dest_epoch_ns: i128,
    iso_date: PlainDate,
    iso_time: PlainTime,
    largest_unit: TemporalUnit,
    increment: f64,
    smallest_unit: TemporalUnit,
    rounding_mode: RoundingMode,
) -> TemporalResult<()> {
    round_relative_duration_in_time_zone(
        calendar_id,
        duration,
        origin_epoch_ns,
        dest_epoch_ns,
        iso_date,
        iso_time,
        largest_unit,
        increment,
        smallest_unit,
        rounding_mode,
        None,
    )
}

/// `roundRelativeDuration(duration, originEpochNs, destEpochNs, isoDateTime, timeZone, largestUnit, increment,
/// smallestUnit, roundingMode)`: https://tc39.es/proposal-temporal/#sec-temporal-roundrelativeduration
/// Arredonda `duration` no lugar; `time_zone` `None` é o `~unset~` dos passos 3 e 6.
#[allow(clippy::too_many_arguments)]
pub fn round_relative_duration_in_time_zone(
    calendar_id: CalendarID,
    duration: &mut InternalDuration,
    origin_epoch_ns: i128,
    dest_epoch_ns: i128,
    iso_date: PlainDate,
    iso_time: PlainTime,
    largest_unit: TemporalUnit,
    increment: f64,
    smallest_unit: TemporalUnit,
    rounding_mode: RoundingMode,
    time_zone: Option<&TimeZone>,
) -> TemporalResult<()> {
    // Passos 1 e 2: `irregularLengthUnit` é `IsCalendarUnit(smallestUnit)`. Passo 3: com `timeZone` e `~day~` também.
    let irregular_length_unit = is_calendar_unit(smallest_unit) || (time_zone.is_some() && smallest_unit == TemporalUnit::Day);
    // Passo 4: `sign` -1 se `InternalDurationSign(duration) < 0`, senão 1.
    let sign = if internal_duration_sign(duration) < 0 { -1 } else { 1 };

    let nudge_result = if irregular_length_unit {
        // Passo 5: `NudgeToCalendarUnit`, e o `nudgeResult` do registro (o `total` não serve a `round`, `until`
        // nem `since`).
        nudge_to_calendar_unit(
            calendar_id,
            sign,
            duration,
            origin_epoch_ns,
            dest_epoch_ns,
            iso_date,
            iso_time,
            increment,
            smallest_unit,
            rounding_mode,
            time_zone,
        )?
        .nudge_result
    } else if let Some(time_zone) = time_zone {
        // Passo 6: `NudgeToZonedTime`.
        nudge_to_zoned_time(calendar_id, sign, duration, iso_date, iso_time, time_zone, increment, smallest_unit, rounding_mode)?
    } else {
        // Passo 7: `NudgeToDayOrTime`.
        nudge_to_day_or_time(duration, dest_epoch_ns, largest_unit, increment, smallest_unit, rounding_mode)?
    };
    // Passo 8: `duration` é a do `nudgeResult`.
    *duration = nudge_result.duration;
    // Passo 9: `DidExpandCalendarUnit` e `smallestUnit` diferente de `~week~`.
    if nudge_result.did_expand_calendar_unit && smallest_unit != TemporalUnit::Week {
        // Passo 9.a: `startUnit = LargerOfTwoTemporalUnits(smallestUnit, ~day~)`.
        let start_unit = if smallest_unit <= TemporalUnit::Day { smallest_unit } else { TemporalUnit::Day };
        // Passo 9.b: `BubbleRelativeDuration`.
        *duration = bubble_relative_duration(
            calendar_id,
            sign,
            *duration,
            nudge_result.nudged_epoch_ns,
            iso_date,
            iso_time,
            largest_unit,
            start_unit,
            time_zone,
        )?;
    }
    // Passo 10.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::temporal_calendar::ISO8601_CALENDAR_ID;

    fn duration(days: i64, hours: i64, minutes: i64, seconds: i64, ms: i64, us: i128, ns: i128) -> Duration {
        Duration::new(0, 0, 0, days, hours, minutes, seconds, ms, us, ns)
    }

    #[test]
    fn sign_and_largest_unit() {
        assert_eq!(duration_sign(&duration(0, 0, -1, 0, 0, 0, 0)), -1);
        assert_eq!(duration_sign(&Duration::default()), 0);
        assert_eq!(largest_subduration(&duration(0, 0, 5, 0, 0, 0, 0)), TemporalUnit::Minute);
        assert_eq!(largest_subduration(&Duration::default()), TemporalUnit::Nanosecond);
    }

    #[test]
    fn internal_round_trip() {
        let d = duration(1, 2, 3, 4, 5, 6, 7);
        let internal = to_internal_duration_record_with_24_hour_days(&d).unwrap();
        assert_eq!(internal.time(), 86_400_000_000_000 + 2 * 3_600_000_000_000 + 3 * 60_000_000_000 + 4_005_006_007);
        let back = temporal_duration_from_internal(&internal, TemporalUnit::Day).unwrap();
        assert_eq!(back, d);
        let hours = temporal_duration_from_internal(&internal, TemporalUnit::Hour).unwrap();
        assert_eq!(hours, duration(0, 26, 3, 4, 5, 6, 7));
        let negated = temporal_duration_from_internal(&InternalDuration::new(Duration::default(), -internal.time()), TemporalUnit::Hour).unwrap();
        assert_eq!(negated, -duration(0, 26, 3, 4, 5, 6, 7));
    }

    #[test]
    fn splits_floor_days() {
        assert_eq!(split_time_duration(-1), (-1, ExactTime::NS_PER_DAY - 1));
        assert_eq!(split_time_duration(ExactTime::NS_PER_DAY + 5), (1, 5));
        assert_eq!(plain_time_from_subday_ns(3_723_004_005_006), PlainTime::new(1, 2, 3, 4, 5, 6));
    }

    #[test]
    fn totals() {
        assert_eq!(total_time_duration(90 * 60 * 1_000_000_000, TemporalUnit::Hour), 1.5);
        assert!(add_24_hour_days_to_time_duration(0, 1e12).is_err());
    }

    #[test]
    fn rounds_relative_duration_to_months() {
        let date = PlainDate::new(2020, 1, 1);
        let time = PlainTime::default();
        let origin = get_utc_epoch_nanoseconds(date, time);
        let dest = get_utc_epoch_nanoseconds(PlainDate::new(2020, 3, 20), time);
        let mut duration = InternalDuration::new(Duration::new(0, 2, 0, 19, 0, 0, 0, 0, 0, 0), 0);
        round_relative_duration(ISO8601_CALENDAR_ID, &mut duration, origin, dest, date, time, TemporalUnit::Month, 1.0, TemporalUnit::Month, RoundingMode::HalfExpand)
            .unwrap();
        assert_eq!((duration.date_duration().months(), duration.date_duration().days()), (3, 0));
        // `trunc` fica nos dois meses (`startDuration`, sem dias).
        let mut duration = InternalDuration::new(Duration::new(0, 2, 0, 19, 0, 0, 0, 0, 0, 0), 0);
        round_relative_duration(ISO8601_CALENDAR_ID, &mut duration, origin, dest, date, time, TemporalUnit::Month, 1.0, TemporalUnit::Month, RoundingMode::Trunc)
            .unwrap();
        assert_eq!(duration.date_duration().months(), 2);
    }

    #[test]
    fn utc_epoch_nanoseconds_of_the_epoch() {
        assert_eq!(get_utc_epoch_nanoseconds(PlainDate::new(1970, 1, 1), PlainTime::default()), 0);
        assert_eq!(get_utc_epoch_nanoseconds(PlainDate::new(1970, 1, 2), PlainTime::new(0, 0, 0, 0, 0, 1)), ExactTime::NS_PER_DAY + 1);
    }
}
