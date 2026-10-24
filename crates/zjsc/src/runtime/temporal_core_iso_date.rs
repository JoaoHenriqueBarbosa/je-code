//! Porte do subconjunto de datas de `runtime/temporal/core/ISOArithmetic.{h,cpp}` (e do `calendarDateAdd`
//! de `CalendarArithmetic.cpp`, que só repassa a `isoDateAdd`): `balanceISOYearMonth`, `addDaysToISODate`,
//! `regulateISODate`, `isoDateAdd`, `isoDateCompare` e `diffISODate` (o ramo `iso8601` de
//! `CalendarDateUntil`), mais `diffISODateTime` e `roundISODateTime` (de `PlainDateTime`). O `isoTimeCompare` é o
//! `TemporalPlainTime::compare`, que já existia (a mesma regra, sem segunda cópia).
//!
//! DIVERGÊNCIA: `calendarDateAdd(calendarId, ...)` e `calendarDateUntil(calendarId, ...)` despacham por calendário;
//! o despacho único está em `temporal_calendar_icu.rs` (`calendar_date_add`, `calendar_date_until`) e cai em
//! `isoDateAdd` e `diffISODate` daqui para `iso8601` e calendários de estrutura gregoriana. Os chamadores que só
//! existem para o calendário ISO (`PlainDateTime`, `Duration`, `ZonedDateTime`) usam as funções daqui direto.

use crate::runtime::date_constructor::{make_date, make_day};
use crate::runtime::iso8601::{
    days_in_month, is_date_time_within_limits, is_year_within_limits, Duration, ExactTime, InternalDuration, PlainDate, PlainTime, PlainYearMonth,
    DAYS_PER_WEEK, MONTHS_PER_YEAR, OUT_OF_RANGE_YEAR,
};
use crate::runtime::temporal_calendar::CalendarID;
use crate::runtime::temporal_calendar_icu::calendar_date_until;
use crate::runtime::temporal_core_rounding::round_number_to_increment_i128;
use crate::runtime::temporal_core_types::{range_error, TemporalResult};
use crate::runtime::temporal_object::{RoundingMode, TemporalOverflow, TemporalUnit};
use crate::runtime::temporal_plain_time::difference_time;
use crate::wtf::date_math::{ms_to_days_f64, year_month_day_from_days};

/// `balanceISOYearMonth(year, month)`: https://tc39.es/proposal-temporal/#sec-temporal-balanceisoyearmonth
pub fn balance_iso_year_month(year: i64, month: i64) -> PlainYearMonth {
    // Passos 1 e 2: `year + floor((month - 1) / 12)` e `((month - 1) modulo 12) + 1`; a divisão e o resto
    // euclidianos são o piso e o módulo da spec (o C++ corrige a divisão que trunca para zero).
    let month_zero_based = month - 1;
    let year = year + month_zero_based.div_euclid(i64::from(MONTHS_PER_YEAR));
    let month = month_zero_based.rem_euclid(i64::from(MONTHS_PER_YEAR)) + 1;
    // Passo 3.
    PlainYearMonth::from_date(PlainDate::new(year, month as u32, 1))
}

/// `addDaysToISODate(date, days)`: https://tc39.es/proposal-temporal/#sec-temporal-adddaystoisodate
/// Fora da faixa devolve `PlainDate::sentinel()`.
pub fn add_days_to_iso_date(date: PlainDate, days: i64) -> PlainDate {
    let year = date.year();
    let month = i32::from(date.month());
    if year == OUT_OF_RANGE_YEAR {
        return PlainDate::sentinel();
    }

    // O invariante do mês de `PlainDate` (posto por `regulateISODate` e `balanceISOYearMonth` em todo ponto
    // construído a partir das entradas públicas) é `[1, 12]`.
    debug_assert!((1..=12).contains(&month));

    let new_day = i64::from(date.day()) + days;

    // O novo dia do mês cai em `[1, daysInMonth(year, month)]`: nenhum transbordo para outro mês ou ano, então
    // pula a ida e volta por dias desde 1970.
    if new_day >= 1 && new_day <= 31 && new_day <= i64::from(days_in_month(year, month as u8)) {
        return PlainDate::new(i64::from(year), month as u32, new_day as u32);
    }

    // Passo 1: `ISODateToEpochDays(year, month - 1, day)` (o `makeDay` do WTF recebe o mês de base zero).
    let epoch_days = make_day(f64::from(year), f64::from(month - 1), new_day as f64);
    // Passo 2: `EpochDaysToEpochMs(epochDays, 0)`.
    let ms = make_date(epoch_days, 0.0);
    let days_to_use = ms_to_days_f64(ms);
    // `yearMonthDayFromDays` soma um deslocamento de ~1.47e8 dias, então o domínio dela é mais estreito que o
    // `int32_t` do parâmetro. As datas reais ficam a menos de 1e8 dias da época, nada de válido é excluído.
    const MAX_SAFE_EPOCH_DAYS: f64 = 1e9;
    if days_to_use.abs() > MAX_SAFE_EPOCH_DAYS {
        return PlainDate::sentinel();
    }
    // Passo 3: `CreateISODateRecord(EpochTimeToEpochYear(ms), EpochTimeToMonthInYear(ms) + 1, EpochTimeToDate(ms))`.
    let (y, m, d) = year_month_day_from_days(days_to_use as i32);
    if !is_year_within_limits(i64::from(y)) {
        return PlainDate::sentinel();
    }
    PlainDate::new(i64::from(y), (m + 1) as u32, d as u32)
}

/// `regulateISODate(year, month, day, overflow)`: https://tc39.es/proposal-temporal/#sec-temporal-regulateisodate
pub fn regulate_iso_date(year: i32, month: i32, day: i64, overflow: TemporalOverflow) -> TemporalResult<PlainDate> {
    let (month, day) = if overflow == TemporalOverflow::Constrain {
        // Passo 1: `month` e `day` presos em `[1, 12]` e `[1, ISODaysInMonth(year, month)]`.
        let month = month.clamp(1, 12);
        let max_day = i64::from(days_in_month(year, month as u8));
        (month, day.clamp(1, max_day))
    } else {
        // Passo 2: `~reject~` e `IsValidISODate` falso é `RangeError`.
        if !(1..=12).contains(&month) || day < 1 || day > i64::from(days_in_month(year, month as u8)) {
            return Err(range_error("date time is out of range of ECMAScript representation"));
        }
        (month, day)
    };
    // Passo 3.
    Ok(PlainDate::new(i64::from(year), month as u32, day as u32))
}

/// A mensagem do `RangeError` de `ISODateWithinLimits` falso em `CalendarDateAdd` (a de `calendarDateAdd` não ISO é igual).
pub const OUT_OF_RANGE: &str = "date time is out of range of ECMAScript representation";

/// `isoDateAdd(plainDate, duration, overflow)` (o ramo `iso8601` de `CalendarDateAdd`):
/// https://tc39.es/proposal-temporal/#sec-temporal-calendardateadd
pub fn iso_date_add(plain_date: PlainDate, duration: &Duration, overflow: TemporalOverflow) -> TemporalResult<PlainDate> {
    // Passo 1.a: `BalanceISOYearMonth(isoDate.[[Year]] + duration.[[Years]], isoDate.[[Month]] + duration.[[Months]])`.
    let intermediate = balance_iso_year_month(
        i64::from(plain_date.year()) + duration.years(),
        i64::from(plain_date.month()) + duration.months(),
    );
    // Passo 1.b: `RegulateISODate(intermediate.[[Year]], intermediate.[[Month]], isoDate.[[Day]], overflow)`.
    let intermediate =
        regulate_iso_date(intermediate.year(), i32::from(intermediate.month()), i64::from(plain_date.day()), overflow)
            .map_err(|_| range_error(OUT_OF_RANGE))?;
    // Passos 1.c e 1.d: `days = duration.[[Days]] + 7 × duration.[[Weeks]]` e `AddDaysToISODate(intermediate, days)`.
    let days = duration.days() + i64::from(DAYS_PER_WEEK) * duration.weeks();
    let result = add_days_to_iso_date(intermediate, days);
    // Passo 3: `ISODateWithinLimits(result)` falso é `RangeError`.
    if !is_date_time_within_limits(result.year(), result.month(), result.day(), 12, 0, 0, 0, 0, 0) {
        return Err(range_error(OUT_OF_RANGE));
    }
    // Passo 4.
    Ok(result)
}

/// `isoDateCompare(d1, d2)` (`CompareISODate`): https://tc39.es/proposal-temporal/#sec-temporal-compareisodate
pub fn iso_date_compare(d1: PlainDate, d2: PlainDate) -> i32 {
    match (d1.year(), d1.month(), d1.day()).cmp(&(d2.year(), d2.month(), d2.day())) {
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
    }
}

/// `isoDateSurpasses(sign, y1, m1, d1, isoDate2)`: https://tc39.es/proposal-temporal/#sec-temporal-isodatesurpasses
fn iso_date_surpasses(sign: i32, y1: i32, m1: i32, d1: i32, iso_date2: PlainDate) -> bool {
    if y1 != iso_date2.year() {
        return sign * (y1 - iso_date2.year()) > 0;
    }
    if m1 != i32::from(iso_date2.month()) {
        return sign * (m1 - i32::from(iso_date2.month())) > 0;
    }
    if d1 != i32::from(iso_date2.day()) {
        return sign * (d1 - i32::from(iso_date2.day())) > 0;
    }
    false
}

/// `diffISODate(one, two, largestUnit)` (o ramo `iso8601` de `CalendarDateUntil`, passo 3):
/// https://tc39.es/proposal-temporal/#sec-temporal-calendardateuntil
pub fn diff_iso_date(one: PlainDate, two: PlainDate, largest_unit: TemporalUnit) -> Duration {
    // Passos 1, 2 e 3.a: `sign = -CompareISODate(one, two)`; zero devolve a duração zero.
    let sign = -iso_date_compare(one, two);
    if sign == 0 {
        return Duration::default();
    }

    // Passos 3.b e 3.d.
    let mut years: i32 = 0;
    let mut months: i32 = 0;

    // Passos 3.c e 3.e: a otimização do polyfill roda o laço de anos para `year` e para `month` e dobra em
    // meses depois.
    if largest_unit == TemporalUnit::Year || largest_unit == TemporalUnit::Month {
        // `candidateYears` começa em `two.year() - one.year()` em vez de `sign` (menos iterações).
        let mut candidate_years = two.year() - one.year();
        if candidate_years != 0 {
            candidate_years -= sign;
        }
        // Passo 3.c.ii: repete enquanto `ISODateSurpasses(sign, one, two, candidateYears, 0, 0, 0)` é falso.
        while !iso_date_surpasses(sign, one.year() + candidate_years, i32::from(one.month()), i32::from(one.day()), two) {
            years = candidate_years;
            candidate_years += sign;
        }

        // Passo 3.e.i e 3.e.ii: `candidateMonths` começa em `sign`.
        let mut candidate_months = sign;
        let mut intermediate = balance_iso_year_month(i64::from(one.year() + years), i64::from(one.month()) + i64::from(candidate_months));
        while !iso_date_surpasses(sign, intermediate.year(), i32::from(intermediate.month()), i32::from(one.day()), two) {
            months = candidate_months;
            candidate_months += sign;
            intermediate = balance_iso_year_month(i64::from(intermediate.year()), i64::from(intermediate.month()) + i64::from(sign));
        }

        // `largestUnit` mês: os anos entram nos meses.
        if largest_unit == TemporalUnit::Month {
            months += years * MONTHS_PER_YEAR;
            years = 0;
        }
    }

    // Os passos 3.f a 3.j (semanas e dias pelo `ISODateSurpasses`) viram a subtração de dias desde a época
    // (otimização do polyfill), sobre a data depois de somar anos e meses.
    let intermediate = balance_iso_year_month(i64::from(one.year() + years), i64::from(one.month()) + i64::from(months));
    let constrained = regulate_iso_date(intermediate.year(), i32::from(intermediate.month()), i64::from(one.day()), TemporalOverflow::Constrain)
        .expect("o modo constrain não falha");

    let mut weeks = 0.0f64;
    let mut days = make_day(f64::from(two.year()), f64::from(two.month()) - 1.0, f64::from(two.day()))
        - make_day(f64::from(constrained.year()), f64::from(constrained.month()) - 1.0, f64::from(constrained.day()));

    // Passo 3.g: `largestUnit` semana.
    if largest_unit == TemporalUnit::Week {
        weeks = (days.abs() / f64::from(DAYS_PER_WEEK)).trunc();
        days = ((days.trunc() as i128) % i128::from(DAYS_PER_WEEK)) as f64;
        if weeks != 0.0 {
            weeks *= f64::from(sign); // evita -0
        }
    }

    // Passo 3.k: `CreateDateDurationRecord(years, months, weeks, days)`.
    Duration::new(i64::from(years), i64::from(months), weeks as i64, days as i64, 0, 0, 0, 0, 0, 0)
}

/// `differenceISODateTime(isoDateTime1, isoDateTime2, calendar, largestUnit)` (`DifferenceISODateTime`):
/// https://tc39.es/proposal-temporal/#sec-temporal-differenceisodatetime
/// A parte de data é `CalendarDateUntil` (ISO cai em `diff_iso_date`).
pub fn diff_iso_date_time(
    calendar_id: CalendarID,
    d1: PlainDate,
    t1: PlainTime,
    d2: PlainDate,
    t2: PlainTime,
    largest_unit: TemporalUnit,
) -> TemporalResult<InternalDuration> {
    // Passo 3: `timeDuration = DifferenceTime(t1, t2)`. Passos 4 e 5: `timeSign` e `dateSign`.
    let mut time_duration = difference_time(t1, t2);
    let time_sign = time_duration.signum() as i32;
    let date_sign = iso_date_compare(d1, d2);

    // Passos 6 e 7: `timeSign = dateSign` (mesmo sinal, nenhum nulo) recua um dia na data e devolve o dia ao tempo.
    let mut adjusted_d2 = d2;
    if date_sign != 0 && time_sign != 0 && date_sign == time_sign {
        adjusted_d2 = add_days_to_iso_date(adjusted_d2, i64::from(time_sign));
        time_duration -= i128::from(time_sign) * ExactTime::NS_PER_DAY;
    }

    // Passos 8 e 9: `dateLargestUnit = LargerOfTwoTemporalUnits(~day~, largestUnit)` e `CalendarDateUntil`.
    let date_largest_unit = if largest_unit > TemporalUnit::Day { TemporalUnit::Day } else { largest_unit };
    let date_difference = calendar_date_until(calendar_id, d1, adjusted_d2, date_largest_unit)?;

    // Passo 10: `largestUnit` menor que dia leva os dias para o tempo.
    let mut remaining_days = date_difference.days();
    if largest_unit != date_largest_unit {
        time_duration += i128::from(date_difference.days()) * ExactTime::NS_PER_DAY;
        remaining_days = 0;
    }

    // Passo 11: `CombineDateAndTimeDuration(dateDifference, timeDuration)`.
    Ok(InternalDuration::new(
        Duration::new(date_difference.years(), date_difference.months(), date_difference.weeks(), remaining_days, 0, 0, 0, 0, 0, 0),
        time_duration,
    ))
}

/// `roundTime` (passos 1 a 6, só a quantidade) do `ISOArithmetic.cpp`: `(quantity, baseOffset)` em nanossegundos,
/// o tempo da unidade corrente para baixo e o das unidades maiores.
fn round_time_quantity(time: PlainTime, unit: TemporalUnit) -> (i128, i128) {
    let parts = [
        (time.hour(), ExactTime::NS_PER_HOUR),
        (time.minute(), ExactTime::NS_PER_MINUTE),
        (time.second(), ExactTime::NS_PER_SECOND),
        (time.millisecond(), ExactTime::NS_PER_MILLISECOND),
        (time.microsecond(), ExactTime::NS_PER_MICROSECOND),
        (time.nanosecond(), 1),
    ];
    // `day` e `hour` pegam o tempo todo desde a meia-noite; o `default` do C++ (nanossegundo) é o último campo.
    let first = match unit {
        TemporalUnit::Day | TemporalUnit::Hour => 0,
        TemporalUnit::Minute => 1,
        TemporalUnit::Second => 2,
        TemporalUnit::Millisecond => 3,
        TemporalUnit::Microsecond => 4,
        _ => 5,
    };
    let total = |slice: &[(u32, i128)]| slice.iter().map(|(value, weight)| i128::from(*value) * weight).sum::<i128>();
    (total(&parts[first..]), total(&parts[..first]))
}

/// `roundISODateTime(date, time, incrementNs, unit, mode)` (`RoundISODateTime`):
/// https://tc39.es/proposal-temporal/#sec-temporal-roundisodatetime
/// Fora da faixa de datas o dia devolvido é `PlainDate::sentinel()` (quem chama confere `ISODateTimeWithinLimits`).
pub fn round_iso_date_time(date: PlainDate, time: PlainTime, increment_ns: i128, unit: TemporalUnit, mode: RoundingMode) -> (PlainDate, PlainTime) {
    // Passo 2: `RoundTime`, o tempo arredondado em nanossegundos desde a meia-noite.
    let (quantity, base_offset) = round_time_quantity(time, unit);
    let mut rounded_local_ns = base_offset + round_number_to_increment_i128(quantity, increment_ns, mode);

    // Passo 3: `AddDaysToISODate(isoDate, roundedTime.[[Days]])`, o excesso de um dia para um lado ou para o outro.
    let mut rounded_date = date;
    if rounded_local_ns < 0 {
        rounded_local_ns += ExactTime::NS_PER_DAY;
        rounded_date = add_days_to_iso_date(date, -1);
    } else if rounded_local_ns >= ExactTime::NS_PER_DAY {
        rounded_local_ns -= ExactTime::NS_PER_DAY;
        rounded_date = add_days_to_iso_date(date, 1);
    }

    // Passo 4: `CombineISODateAndTimeRecord(balanceResult, roundedTime)`.
    let mut rest = rounded_local_ns;
    let mut take = |weight: i128| {
        let value = (rest / weight) as u32;
        rest %= weight;
        value
    };
    let hour = take(ExactTime::NS_PER_HOUR);
    let minute = take(ExactTime::NS_PER_MINUTE);
    let second = take(ExactTime::NS_PER_SECOND);
    let millisecond = take(ExactTime::NS_PER_MILLISECOND);
    let microsecond = take(ExactTime::NS_PER_MICROSECOND);
    (rounded_date, PlainTime::new(hour, minute, second, millisecond, microsecond, rest as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balances_year_and_month() {
        let balanced = balance_iso_year_month(2020, 14);
        assert_eq!((balanced.year(), balanced.month()), (2021, 2));
        let balanced = balance_iso_year_month(2020, 0);
        assert_eq!((balanced.year(), balanced.month()), (2019, 12));
        let balanced = balance_iso_year_month(2020, -12);
        assert_eq!((balanced.year(), balanced.month()), (2018, 12));
    }

    #[test]
    fn adds_days_across_months_and_years() {
        let date = add_days_to_iso_date(PlainDate::new(2020, 12, 31), 1);
        assert_eq!((date.year(), date.month(), date.day()), (2021, 1, 1));
        let date = add_days_to_iso_date(PlainDate::new(2020, 3, 1), -1);
        assert_eq!((date.year(), date.month(), date.day()), (2020, 2, 29));
        assert_eq!(add_days_to_iso_date(PlainDate::new(275760, 9, 13), 400_000_000), PlainDate::sentinel());
    }

    #[test]
    fn regulates_dates() {
        let constrained = regulate_iso_date(2021, 2, 30, TemporalOverflow::Constrain).unwrap();
        assert_eq!((constrained.month(), constrained.day()), (2, 28));
        assert!(regulate_iso_date(2021, 2, 30, TemporalOverflow::Reject).is_err());
        let constrained = regulate_iso_date(2021, 13, 0, TemporalOverflow::Constrain).unwrap();
        assert_eq!((constrained.month(), constrained.day()), (12, 1));
    }

    #[test]
    fn adds_durations_with_constrain() {
        let duration = Duration::new(0, 1, 0, 0, 0, 0, 0, 0, 0, 0);
        let result = iso_date_add(PlainDate::new(2020, 1, 31), &duration, TemporalOverflow::Constrain).unwrap();
        assert_eq!((result.year(), result.month(), result.day()), (2020, 2, 29));
        assert!(iso_date_add(PlainDate::new(2020, 1, 31), &duration, TemporalOverflow::Reject).is_err());
    }

    #[test]
    fn diffs_date_times_with_sign_adjustment() {
        let midnight = PlainTime::default();
        // Mesmo dia, 1h30: sem ajuste.
        let iso = crate::runtime::temporal_calendar::ISO8601_CALENDAR_ID;
        let same_day =
            diff_iso_date_time(iso, PlainDate::new(2020, 1, 1), midnight, PlainDate::new(2020, 1, 1), PlainTime::new(1, 30, 0, 0, 0, 0), TemporalUnit::Day).unwrap();
        assert_eq!((same_day.date_duration().days(), same_day.time()), (0, 90 * 60 * ExactTime::NS_PER_SECOND));
        // 12:00 até o dia seguinte 10:00: um dia a menos, 22h de tempo.
        let next_day = diff_iso_date_time(
            iso,
            PlainDate::new(2020, 1, 1),
            PlainTime::new(12, 0, 0, 0, 0, 0),
            PlainDate::new(2020, 1, 2),
            PlainTime::new(10, 0, 0, 0, 0, 0),
            TemporalUnit::Day,
        )
        .unwrap();
        assert_eq!((next_day.date_duration().days(), next_day.time()), (0, 22 * ExactTime::NS_PER_HOUR));
        // Com `largestUnit` hora os dias vão para o tempo.
        let in_hours = diff_iso_date_time(iso, PlainDate::new(2020, 1, 1), midnight, PlainDate::new(2020, 1, 3), midnight, TemporalUnit::Hour).unwrap();
        assert_eq!((in_hours.date_duration().days(), in_hours.time()), (0, 48 * ExactTime::NS_PER_HOUR));
        // Para trás: o sinal do tempo igual ao da data devolve um dia (passo 7).
        let backwards =
            diff_iso_date_time(iso, PlainDate::new(2020, 1, 3), midnight, PlainDate::new(2020, 1, 1), PlainTime::new(1, 0, 0, 0, 0, 0), TemporalUnit::Day).unwrap();
        assert_eq!((backwards.date_duration().days(), backwards.time()), (-1, -23 * ExactTime::NS_PER_HOUR));
        // Para trás sem ajuste: o tempo e a data têm sinais opostos.
        let backwards =
            diff_iso_date_time(iso, PlainDate::new(2020, 1, 3), PlainTime::new(1, 0, 0, 0, 0, 0), PlainDate::new(2020, 1, 1), midnight, TemporalUnit::Day).unwrap();
        assert_eq!((backwards.date_duration().days(), backwards.time()), (-2, -ExactTime::NS_PER_HOUR));
    }

    #[test]
    fn rounds_date_times_across_midnight() {
        let minute = ExactTime::NS_PER_MINUTE;
        let (date, time) = round_iso_date_time(PlainDate::new(2020, 12, 31), PlainTime::new(23, 59, 40, 0, 0, 0), minute, TemporalUnit::Minute, RoundingMode::HalfExpand);
        assert_eq!((date.year(), date.month(), date.day()), (2021, 1, 1));
        assert_eq!(time, PlainTime::default());
        let (date, time) = round_iso_date_time(PlainDate::new(2020, 3, 1), PlainTime::new(10, 20, 30, 400, 500, 600), 5 * ExactTime::NS_PER_SECOND, TemporalUnit::Second, RoundingMode::Floor);
        assert_eq!((date.month(), date.day()), (3, 1));
        assert_eq!(time, PlainTime::new(10, 20, 30, 0, 0, 0));
        let (_, time) = round_iso_date_time(PlainDate::new(2020, 3, 1), PlainTime::new(10, 20, 30, 400, 500, 600), 1000, TemporalUnit::Nanosecond, RoundingMode::HalfExpand);
        assert_eq!(time, PlainTime::new(10, 20, 30, 400, 501, 0));
        // `day` arredonda o tempo todo desde a meia-noite, com incremento de um dia.
        let (date, time) = round_iso_date_time(PlainDate::new(2020, 2, 28), PlainTime::new(12, 0, 0, 0, 0, 0), ExactTime::NS_PER_DAY, TemporalUnit::Day, RoundingMode::HalfExpand);
        assert_eq!((date.month(), date.day(), time), (2, 29, PlainTime::default()));
        let (date, _) = round_iso_date_time(PlainDate::new(2020, 2, 28), PlainTime::new(11, 59, 59, 999, 999, 999), ExactTime::NS_PER_DAY, TemporalUnit::Day, RoundingMode::HalfExpand);
        assert_eq!((date.month(), date.day()), (2, 28));
        // Um dia depois do limite ainda é uma data (quem chama confere `ISODateTimeWithinLimits`).
        let (date, _) = round_iso_date_time(PlainDate::new(275760, 9, 13), PlainTime::new(23, 59, 59, 0, 0, 0), ExactTime::NS_PER_HOUR, TemporalUnit::Hour, RoundingMode::Ceil);
        assert_eq!((date.month(), date.day()), (9, 14));
    }

    #[test]
    fn diffs_dates() {
        let one = PlainDate::new(2020, 1, 31);
        let two = PlainDate::new(2021, 3, 1);
        let year = diff_iso_date(one, two, TemporalUnit::Year);
        assert_eq!((year.years(), year.months(), year.days()), (1, 1, 1));
        let month = diff_iso_date(one, two, TemporalUnit::Month);
        assert_eq!((month.years(), month.months(), month.days()), (0, 13, 1));
        let week = diff_iso_date(PlainDate::new(2020, 1, 1), PlainDate::new(2020, 1, 20), TemporalUnit::Week);
        assert_eq!((week.weeks(), week.days()), (2, 5));
        let backwards = diff_iso_date(two, one, TemporalUnit::Day);
        assert_eq!(backwards.days(), -395);
        assert_eq!(diff_iso_date(one, one, TemporalUnit::Year), Duration::default());
    }
}
