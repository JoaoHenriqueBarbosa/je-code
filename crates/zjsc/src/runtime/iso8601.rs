//! Porte do núcleo de `runtime/ISO8601.{h,cpp}` que não depende de Intl: `Duration`, `ExactTime`,
//! `InternalDuration`, `PlainTime`, `PlainDate`, `PlainDateTime`, `PlainYearMonth`, `PlainMonthDay`,
//! `parseDuration` (`ParseTemporalDurationString`), `isValidDuration`, `checkedCastDoubleToInt128` e o
//! calendário ISO (`dayOfWeek`, `dayOfYear`, `daysInMonth`, `isValidISODate`).
//!
//! Fora desta fatia (listados em `wip-notes/temporal-plan.md`): `parseISODateTime` e os tokenizadores,
//! `parseTimeZoneName`/`parseTemporalTimeZoneString` (precisam de `intlResolveTimeZoneID`), as funções
//! `temporal*ToString`, `weekOfYear`/`yearOfWeek`, `monthCode`/`parseMonthCode`, `roundTimeDuration`,
//! `ExactTime::{difference, round, now, fromISOPartsAndOffset}` e `isDateTimeWithinLimits`.
//!
//! DIVERGÊNCIAS: `Int128` é `i128`; os campos de bits de `PlainTime` e `PlainDate` são campos comuns, com o
//! mesmo invariante (ano fora do limite vira `OUT_OF_RANGE_YEAR`); `Checked<Int128, RecordOverflow>` é o
//! `checked_*` do Rust, que devolve `None` no estouro; o `parseDuration` trabalha sobre `&[u16]` (o
//! `StringView` do C++ lê Latin1 ou UTF-16 e o resultado é o mesmo).

use crate::runtime::temporal_object::TemporalUnit;
use crate::wtf::date_math::{day_in_year, days_from_year_month, is_leap_year, week_day};

pub const MAX_YEAR: i32 = 275760;
pub const MIN_YEAR: i32 = -271821;
pub const OUT_OF_RANGE_YEAR: i32 = MIN_YEAR - 1;

pub const MONTHS_PER_YEAR: i32 = 12;
pub const DAYS_PER_WEEK: i32 = 7;

/// `static constexpr Int128 durationNanosecondsLimit`: `2^53 * 10^9`, o limite do passo 8 de `IsValidDuration`.
const DURATION_NANOSECONDS_LIMIT: i128 = (1i128 << 53) * 1_000_000_000;

/// `subsecondOutOfRangeSentinel`: guardado por `Duration::setField` quando `checkedCastDoubleToInt128` estoura;
/// qualquer valor `>= durationNanosecondsLimit` faz `isValidDuration` devolver falso.
const SUBSECOND_OUT_OF_RANGE_SENTINEL: i128 = DURATION_NANOSECONDS_LIMIT;

/// `ISO8601::Duration`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Duration {
    years: i64,
    months: i64,
    weeks: i64,
    days: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    milliseconds: i64,
    microseconds: i128,
    nanoseconds: i128,
}

impl Duration {
    /// `Duration::doubleToInt64Saturating(v)`: fora da faixa satura (os sentinelas sempre falham em
    /// `isValidDuration`); `NaN` cai no ramo do máximo, como no C++ (`!(v < max)`).
    pub fn double_to_int64_saturating(v: f64) -> i64 {
        if v.is_nan() || !(v < -(i64::MIN as f64)) {
            return i64::MAX;
        }
        if !(v > i64::MIN as f64) {
            return i64::MIN;
        }
        v.trunc() as i64
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        years: i64,
        months: i64,
        weeks: i64,
        days: i64,
        hours: i64,
        minutes: i64,
        seconds: i64,
        milliseconds: i64,
        microseconds: i128,
        nanoseconds: i128,
    ) -> Duration {
        Duration { years, months, weeks, days, hours, minutes, seconds, milliseconds, microseconds, nanoseconds }
    }

    pub fn years(&self) -> i64 {
        self.years
    }
    pub fn months(&self) -> i64 {
        self.months
    }
    pub fn weeks(&self) -> i64 {
        self.weeks
    }
    pub fn days(&self) -> i64 {
        self.days
    }
    pub fn hours(&self) -> i64 {
        self.hours
    }
    pub fn minutes(&self) -> i64 {
        self.minutes
    }
    pub fn seconds(&self) -> i64 {
        self.seconds
    }
    pub fn milliseconds(&self) -> i64 {
        self.milliseconds
    }
    pub fn microseconds(&self) -> i128 {
        self.microseconds
    }
    pub fn nanoseconds(&self) -> i128 {
        self.nanoseconds
    }

    /// `operator[](TemporalUnit)`: o campo como `double`, a leitura que a camada JS usa.
    pub fn field(&self, unit: TemporalUnit) -> f64 {
        match unit {
            TemporalUnit::Year => self.years as f64,
            TemporalUnit::Month => self.months as f64,
            TemporalUnit::Week => self.weeks as f64,
            TemporalUnit::Day => self.days as f64,
            TemporalUnit::Hour => self.hours as f64,
            TemporalUnit::Minute => self.minutes as f64,
            TemporalUnit::Second => self.seconds as f64,
            TemporalUnit::Millisecond => self.milliseconds as f64,
            TemporalUnit::Microsecond => self.microseconds as f64,
            TemporalUnit::Nanosecond => self.nanoseconds as f64,
        }
    }

    /// `setField(TemporalUnit, double)`: converte o número do JS para o armazenamento tipado do campo.
    pub fn set_field(&mut self, unit: TemporalUnit, v: f64) {
        match unit {
            TemporalUnit::Year => self.years = Duration::double_to_int64_saturating(v),
            TemporalUnit::Month => self.months = Duration::double_to_int64_saturating(v),
            TemporalUnit::Week => self.weeks = Duration::double_to_int64_saturating(v),
            TemporalUnit::Day => self.days = Duration::double_to_int64_saturating(v),
            TemporalUnit::Hour => self.hours = Duration::double_to_int64_saturating(v),
            TemporalUnit::Minute => self.minutes = Duration::double_to_int64_saturating(v),
            TemporalUnit::Second => self.seconds = Duration::double_to_int64_saturating(v),
            TemporalUnit::Millisecond => self.milliseconds = Duration::double_to_int64_saturating(v),
            TemporalUnit::Microsecond => {
                self.microseconds = checked_cast_double_to_int128(v).unwrap_or(SUBSECOND_OUT_OF_RANGE_SENTINEL)
            }
            TemporalUnit::Nanosecond => {
                self.nanoseconds = checked_cast_double_to_int128(v).unwrap_or(SUBSECOND_OUT_OF_RANGE_SENTINEL)
            }
        }
    }

    pub fn set_years(&mut self, v: i64) {
        self.years = v;
    }
    pub fn set_months(&mut self, v: i64) {
        self.months = v;
    }
    pub fn set_weeks(&mut self, v: i64) {
        self.weeks = v;
    }
    pub fn set_days(&mut self, v: i64) {
        self.days = v;
    }
    pub fn set_hours(&mut self, v: i64) {
        self.hours = v;
    }
    pub fn set_minutes(&mut self, v: i64) {
        self.minutes = v;
    }
    pub fn set_seconds(&mut self, v: i64) {
        self.seconds = v;
    }
    pub fn set_milliseconds(&mut self, v: i64) {
        self.milliseconds = v;
    }
    pub fn set_microseconds(&mut self, v: i128) {
        self.microseconds = v;
    }
    pub fn set_nanoseconds(&mut self, v: i128) {
        self.nanoseconds = v;
    }

    /// `Duration::clear()`.
    pub fn clear(&mut self) {
        *self = Duration::default();
    }

    /// `Duration::totalNanoseconds<unit>()`: soma de `days` até `unit` em nanossegundos, `None` no estouro de
    /// `Int128`. `unit` tem de ser `Day` ou menor (`ASSERT(unit >= TemporalUnit::Day)`).
    pub fn total_nanoseconds(&self, unit: TemporalUnit) -> Option<i128> {
        debug_assert!(unit >= TemporalUnit::Day);
        let mut result: i128 = 0;
        let terms: [(TemporalUnit, i128, i128); 7] = [
            (TemporalUnit::Day, i128::from(self.days), ExactTime::NS_PER_DAY),
            (TemporalUnit::Hour, i128::from(self.hours), ExactTime::NS_PER_HOUR),
            (TemporalUnit::Minute, i128::from(self.minutes), ExactTime::NS_PER_MINUTE),
            (TemporalUnit::Second, i128::from(self.seconds), ExactTime::NS_PER_SECOND),
            (TemporalUnit::Millisecond, i128::from(self.milliseconds), ExactTime::NS_PER_MILLISECOND),
            (TemporalUnit::Microsecond, self.microseconds, ExactTime::NS_PER_MICROSECOND),
            (TemporalUnit::Nanosecond, self.nanoseconds, 1),
        ];
        for (term_unit, value, factor) in terms {
            if unit >= term_unit {
                result = result.checked_add(value.checked_mul(factor)?)?;
            }
        }
        Some(result)
    }

    /// `Duration::totalNanoseconds<unit>()` do C++: a soma de `unit` e de todos os campos MENORES (`if constexpr
    /// (unit <= TemporalUnit::Second)` soma segundos, milissegundos, microssegundos e nanossegundos), `None` no
    /// estouro de `Int128`. O `Intl.DurationFormat` usa com `Second`, `Millisecond` e `Microsecond`.
    pub fn total_nanoseconds_from(&self, unit: TemporalUnit) -> Option<i128> {
        debug_assert!(unit >= TemporalUnit::Day);
        let mut result: i128 = 0;
        let terms: [(TemporalUnit, i128, i128); 7] = [
            (TemporalUnit::Day, i128::from(self.days), ExactTime::NS_PER_DAY),
            (TemporalUnit::Hour, i128::from(self.hours), ExactTime::NS_PER_HOUR),
            (TemporalUnit::Minute, i128::from(self.minutes), ExactTime::NS_PER_MINUTE),
            (TemporalUnit::Second, i128::from(self.seconds), ExactTime::NS_PER_SECOND),
            (TemporalUnit::Millisecond, i128::from(self.milliseconds), ExactTime::NS_PER_MILLISECOND),
            (TemporalUnit::Microsecond, self.microseconds, ExactTime::NS_PER_MICROSECOND),
            (TemporalUnit::Nanosecond, self.nanoseconds, 1),
        ];
        for (term_unit, value, factor) in terms {
            if unit <= term_unit {
                result = result.checked_add(value.checked_mul(factor)?)?;
            }
        }
        Some(result)
    }
}

impl std::ops::Neg for Duration {
    type Output = Duration;

    /// `Duration::operator-()`.
    fn neg(self) -> Duration {
        Duration::new(
            -self.years,
            -self.months,
            -self.weeks,
            -self.days,
            -self.hours,
            -self.minutes,
            -self.seconds,
            -self.milliseconds,
            -self.microseconds,
            -self.nanoseconds,
        )
    }
}

/// `ISO8601::ExactTime`: nanossegundos desde a época, como `Int128`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExactTime {
    epoch_nanoseconds: i128,
}

impl ExactTime {
    pub const DAY_RANGE_SECONDS: i128 = 86400_00000000;
    pub const NS_PER_MICROSECOND: i128 = 1000;
    pub const NS_PER_MILLISECOND: i128 = 1_000_000;
    pub const NS_PER_SECOND: i128 = 1_000_000_000;
    pub const NS_PER_MINUTE: i128 = ExactTime::NS_PER_SECOND * 60;
    pub const NS_PER_HOUR: i128 = ExactTime::NS_PER_MINUTE * 60;
    pub const NS_PER_DAY: i128 = ExactTime::NS_PER_HOUR * 24;
    pub const MIN_VALUE: i128 = -ExactTime::DAY_RANGE_SECONDS * ExactTime::NS_PER_SECOND;
    pub const MAX_VALUE: i128 = ExactTime::DAY_RANGE_SECONDS * ExactTime::NS_PER_SECOND;

    pub const fn new(epoch_nanoseconds: i128) -> ExactTime {
        ExactTime { epoch_nanoseconds }
    }

    /// `fromEpochMilliseconds(int64_t)`.
    pub const fn from_epoch_milliseconds(epoch_milliseconds: i64) -> ExactTime {
        ExactTime::new(epoch_milliseconds as i128 * ExactTime::NS_PER_MILLISECOND)
    }

    /// `epochMilliseconds()`: trunca em direção a zero.
    pub fn epoch_milliseconds(&self) -> i64 {
        (self.epoch_nanoseconds / ExactTime::NS_PER_MILLISECOND) as i64
    }

    /// `floorEpochMilliseconds()`.
    pub fn floor_epoch_milliseconds(&self) -> i64 {
        self.epoch_nanoseconds.div_euclid(ExactTime::NS_PER_MILLISECOND) as i64
    }

    pub const fn epoch_nanoseconds(&self) -> i128 {
        self.epoch_nanoseconds
    }

    /// `nanosecondsFraction()`: o resto por segundo, com o sinal do `%` do C++.
    pub fn nanoseconds_fraction(&self) -> i32 {
        (self.epoch_nanoseconds % ExactTime::NS_PER_SECOND) as i32
    }

    /// `asString()`: o inteiro em decimal.
    pub fn as_string(&self) -> String {
        self.epoch_nanoseconds.to_string()
    }

    /// `IsValidEpochNanoseconds`.
    pub const fn is_valid(&self) -> bool {
        self.epoch_nanoseconds >= ExactTime::MIN_VALUE && self.epoch_nanoseconds <= ExactTime::MAX_VALUE
    }

    /// `ExactTime::add(Duration)`: sem campos de calendário (`ASSERT`s do C++); `None` no estouro ou fora da
    /// faixa válida.
    pub fn add(&self, duration: &Duration) -> Option<ExactTime> {
        debug_assert!(duration.years() == 0 && duration.months() == 0 && duration.weeks() == 0 && duration.days() == 0);
        let terms: [(i128, i128); 6] = [
            (i128::from(duration.hours()), ExactTime::NS_PER_HOUR),
            (i128::from(duration.minutes()), ExactTime::NS_PER_MINUTE),
            (i128::from(duration.seconds()), ExactTime::NS_PER_SECOND),
            (i128::from(duration.milliseconds()), ExactTime::NS_PER_MILLISECOND),
            (duration.microseconds(), ExactTime::NS_PER_MICROSECOND),
            (duration.nanoseconds(), 1),
        ];
        let mut result = self.epoch_nanoseconds;
        for (value, factor) in terms {
            result = result.checked_add(value.checked_mul(factor)?)?;
        }
        let result = ExactTime::new(result);
        result.is_valid().then_some(result)
    }

    /// `fromISOPartsAndOffset(y, mon, d, h, min, s, ms, micros, ns, offset)`: a data e a hora no fuso de
    /// `offset` (nanossegundos) como instante.
    #[allow(clippy::too_many_arguments)]
    pub fn from_iso_parts_and_offset(
        year: i32,
        month: u8,
        day: u8,
        hour: u32,
        minute: u32,
        second: u32,
        millisecond: u32,
        microsecond: u32,
        nanosecond: u32,
        offset: i64,
    ) -> ExactTime {
        debug_assert!((1..=12).contains(&month) && (1..=31).contains(&day));
        debug_assert!(hour <= 23 && minute <= 59 && second <= 59 && millisecond <= 999 && microsecond <= 999 && nanosecond <= 999);
        let date_days = crate::wtf::date_math::date_to_days_from_1970(year, i32::from(month) - 1, i32::from(day)) as i128;
        let utc_nanoseconds = date_days * ExactTime::NS_PER_DAY
            + i128::from(hour) * ExactTime::NS_PER_HOUR
            + i128::from(minute) * ExactTime::NS_PER_MINUTE
            + i128::from(second) * ExactTime::NS_PER_SECOND
            + i128::from(millisecond) * ExactTime::NS_PER_MILLISECOND
            + i128::from(microsecond) * ExactTime::NS_PER_MICROSECOND
            + i128::from(nanosecond);
        ExactTime::new(utc_nanoseconds - i128::from(offset))
    }

    /// `ExactTime::now()`: `WTF::currentTimeInNanoseconds()`, o mesmo relógio de `Date.now` (`jsCurrentTime`)
    /// com a resolução inteira.
    pub fn now() -> ExactTime {
        ExactTime::new(crate::wtf::date_math::current_time_in_nanoseconds())
    }

    /// `ExactTime::round(globalObject, increment, unit, roundingMode)` (os passos 10 a 17 de
    /// `Temporal.Instant.prototype.round`): valida o incremento contra o dia e arredonda como se positivo.
    pub fn round(
        &self,
        increment: u32,
        unit: TemporalUnit,
        rounding_mode: crate::runtime::temporal_object::RoundingMode,
    ) -> crate::runtime::temporal_core_types::TemporalResult<ExactTime> {
        use crate::runtime::temporal_object::{length_in_nanoseconds, Inclusivity};
        let maximum: i128 = match unit {
            TemporalUnit::Hour => 24,
            TemporalUnit::Minute => 24 * 60,
            TemporalUnit::Second => 24 * 60 * 60,
            TemporalUnit::Millisecond => 24 * 60 * 60 * 1000,
            TemporalUnit::Microsecond => 24 * 60 * 60 * 1000 * 1000,
            TemporalUnit::Nanosecond => ExactTime::NS_PER_DAY,
            _ => unreachable!("ExactTime::round com unidade de calendário"),
        };
        crate::runtime::temporal_core_rounding::validate_temporal_rounding_increment(
            f64::from(increment),
            Some(maximum as f64),
            Inclusivity::Inclusive,
        )?;
        // `roundTemporalInstant(ns, increment, unit, roundingMode)`.
        let increment_ns = i128::from(increment) * length_in_nanoseconds(unit);
        Ok(ExactTime::new(crate::runtime::temporal_core_rounding::round_number_to_increment_as_if_positive(
            self.epoch_nanoseconds,
            increment_ns,
            rounding_mode,
        )))
    }

    /// `ExactTime::difference(globalObject, other, roundingIncrement, smallestUnit, roundingMode)`
    /// (`DifferenceInstant`): https://tc39.es/proposal-temporal/#sec-temporal-differenceinstant
    pub fn difference(
        &self,
        other: ExactTime,
        rounding_increment: u32,
        smallest_unit: TemporalUnit,
        rounding_mode: crate::runtime::temporal_object::RoundingMode,
    ) -> crate::runtime::temporal_core_types::TemporalResult<InternalDuration> {
        let time_duration = other.epoch_nanoseconds - self.epoch_nanoseconds;
        let time_duration = round_time_duration(time_duration, rounding_increment, smallest_unit, rounding_mode)?;
        Ok(InternalDuration::new(Duration::default(), time_duration))
    }
}

/// `ISO8601::InternalDuration`: a parte de data (campos de tempo ignorados) mais a soma dos campos de tempo
/// em nanossegundos.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InternalDuration {
    date_duration: Duration,
    time: i128,
}

impl InternalDuration {
    /// `maxTimeDuration = 2**53 * 10**9 - 1`.
    pub const MAX_TIME_DURATION: i128 = 9_007_199_254_740_992 * ExactTime::NS_PER_SECOND - 1;

    pub fn new(date_duration: Duration, time: i128) -> InternalDuration {
        InternalDuration { date_duration, time }
    }

    pub fn time_duration_sign(&self) -> i32 {
        self.time.signum() as i32
    }

    pub fn time(&self) -> i128 {
        self.time
    }

    pub fn date_duration(&self) -> Duration {
        self.date_duration
    }
}

/// `ISO8601::PlainTime`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlainTime {
    hour: u8,
    minute: u8,
    second: u8,
    millisecond: u16,
    microsecond: u16,
    nanosecond: u16,
}

impl PlainTime {
    pub const fn new(hour: u32, minute: u32, second: u32, millisecond: u32, microsecond: u32, nanosecond: u32) -> PlainTime {
        PlainTime {
            hour: hour as u8,
            minute: minute as u8,
            second: second as u8,
            millisecond: (millisecond & 0x3ff) as u16,
            microsecond: (microsecond & 0x3ff) as u16,
            nanosecond: (nanosecond & 0x3ff) as u16,
        }
    }

    pub fn hour(&self) -> u32 {
        u32::from(self.hour)
    }
    pub fn minute(&self) -> u32 {
        u32::from(self.minute)
    }
    pub fn second(&self) -> u32 {
        u32::from(self.second)
    }
    pub fn millisecond(&self) -> u32 {
        u32::from(self.millisecond)
    }
    pub fn microsecond(&self) -> u32 {
        u32::from(self.microsecond)
    }
    pub fn nanosecond(&self) -> u32 {
        u32::from(self.nanosecond)
    }
}

/// `isYearWithinLimits`.
pub const fn is_year_within_limits(year: i64) -> bool {
    year >= MIN_YEAR as i64 && year <= MAX_YEAR as i64
}

/// `isYearMonthWithinLimits`.
pub const fn is_year_month_within_limits(year: i32, month: i32) -> bool {
    if !is_year_within_limits(year as i64) {
        return false;
    }
    if year == MIN_YEAR && month < 4 {
        return false;
    }
    if year == MAX_YEAR && month > 9 {
        return false;
    }
    true
}

/// `ISO8601::PlainDate`: o ano é o do limite ou `OUT_OF_RANGE_YEAR` (o invariante do construtor do C++).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlainDate {
    year: i32,
    month: u8,
    day: u8,
}

impl Default for PlainDate {
    fn default() -> PlainDate {
        PlainDate { year: 0, month: 1, day: 1 }
    }
}

impl PlainDate {
    /// `PlainDate(int64_t year, unsigned month, unsigned day)`.
    pub const fn new(year: i64, month: u32, day: u32) -> PlainDate {
        let year = if is_year_within_limits(year) { year as i32 } else { OUT_OF_RANGE_YEAR };
        PlainDate { year, month: (month & 0x1f) as u8, day: (day & 0x3f) as u8 }
    }

    /// `PlainDate::sentinel()`.
    pub const fn sentinel() -> PlainDate {
        PlainDate::new(OUT_OF_RANGE_YEAR as i64, 1, 1)
    }

    pub fn year(&self) -> i32 {
        self.year
    }
    pub fn month(&self) -> u8 {
        self.month
    }
    pub fn day(&self) -> u8 {
        self.day
    }
}

/// `ISO8601::PlainDateTime`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlainDateTime {
    pub date: PlainDate,
    pub time: PlainTime,
}

/// `ISO8601::PlainYearMonth`: guarda uma `PlainDate` com dia 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlainYearMonth {
    iso_plain_date: PlainDate,
}

impl Default for PlainYearMonth {
    fn default() -> PlainYearMonth {
        PlainYearMonth { iso_plain_date: PlainDate::new(0, 1, 1) }
    }
}

impl PlainYearMonth {
    pub const fn new(year: i32, month: u32) -> PlainYearMonth {
        PlainYearMonth { iso_plain_date: PlainDate::new(year as i64, month, 1) }
    }

    pub const fn from_date(date: PlainDate) -> PlainYearMonth {
        PlainYearMonth { iso_plain_date: date }
    }

    pub fn year(&self) -> i32 {
        self.iso_plain_date.year()
    }
    pub fn month(&self) -> u8 {
        self.iso_plain_date.month()
    }
    pub fn iso_plain_date(&self) -> &PlainDate {
        &self.iso_plain_date
    }
}

/// `ISO8601::PlainMonthDay`: guarda uma `PlainDate` de ano de referência 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlainMonthDay {
    iso_plain_date: PlainDate,
}

impl Default for PlainMonthDay {
    fn default() -> PlainMonthDay {
        PlainMonthDay { iso_plain_date: PlainDate::new(0, 1, 1) }
    }
}

impl PlainMonthDay {
    pub const fn new(month: u32, day: i32) -> PlainMonthDay {
        PlainMonthDay { iso_plain_date: PlainDate::new(2, month, day as u32) }
    }

    pub const fn from_date(date: PlainDate) -> PlainMonthDay {
        PlainMonthDay { iso_plain_date: date }
    }

    pub fn month(&self) -> u8 {
        self.iso_plain_date.month()
    }
    pub fn day(&self) -> u32 {
        u32::from(self.iso_plain_date.day())
    }
    pub fn iso_plain_date(&self) -> &PlainDate {
        &self.iso_plain_date
    }
}

/// `checkedCastDoubleToInt128(double)`: o `__fixdfti` do compiler-rt; `None` quando o valor não cabe.
pub fn checked_cast_double_to_int128(n: f64) -> Option<i128> {
    const SIGNIFICAND_BITS: i32 = 52;
    const EXPONENT_BITS: i32 = 11;
    const EXPONENT_BIAS: i32 = 1023;
    const IMPLICIT_BIT: u64 = 1 << SIGNIFICAND_BITS;
    const SIGNIFICAND_MASK: u64 = IMPLICIT_BIT - 1;
    const SIGN_MASK: u64 = 1 << (SIGNIFICAND_BITS + EXPONENT_BITS);
    const ABS_MASK: u64 = SIGN_MASK - 1;

    let bits = n.to_bits();
    let n_abs = bits & ABS_MASK;
    let sign: i128 = if bits & SIGN_MASK != 0 { -1 } else { 1 };
    let exponent = (n_abs >> SIGNIFICAND_BITS) as i32 - EXPONENT_BIAS;
    let significand = (n_abs & SIGNIFICAND_MASK) | IMPLICIT_BIT;

    if exponent < 0 {
        return Some(0);
    }
    if exponent >= 128 {
        return None;
    }
    let mut result = significand as i128;
    if exponent < SIGNIFICAND_BITS {
        result >>= SIGNIFICAND_BITS - exponent;
    } else {
        result <<= exponent - SIGNIFICAND_BITS;
    }
    Some(result * sign)
}

/// `isValidDuration(const Duration&)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-isvalidduration
pub fn is_valid_duration(duration: &Duration) -> bool {
    let fields = [
        duration.years(),
        duration.months(),
        duration.weeks(),
        duration.days(),
        duration.hours(),
        duration.minutes(),
        duration.seconds(),
        duration.milliseconds(),
    ];
    let wide = [duration.microseconds(), duration.nanoseconds()];
    // Passos 1 e 2: todos os campos não nulos têm o mesmo sinal.
    let mut sign = 0i32;
    let signs = fields.iter().map(|v| v.signum() as i32).chain(wide.iter().map(|v| v.signum() as i32));
    for field_sign in signs {
        if field_sign != 0 {
            if sign != 0 && sign != field_sign {
                return false;
            }
            sign = field_sign;
        }
    }

    // Passos 3 a 5: `abs(years|months|weeks) >= 2^32` falha (comparação de faixa, sem `abs(INT64_MIN)`).
    let limit: i64 = 1 << 32;
    if duration.years() >= limit
        || duration.years() <= -limit
        || duration.months() >= limit
        || duration.months() <= -limit
        || duration.weeks() >= limit
        || duration.weeks() <= -limit
    {
        return false;
    }

    // Passos 6 a 8: `None` de `totalNanoseconds` é estouro de `Int128`, muito além do limite de 2^53.
    match duration.total_nanoseconds(TemporalUnit::Nanosecond) {
        Some(total) => total < DURATION_NANOSECONDS_LIMIT && total > -DURATION_NANOSECONDS_LIMIT,
        None => false,
    }
}

/// `roundTimeDuration(globalObject, timeDuration, increment, unit, roundingMode)` (com o
/// `roundTimeDurationToIncrement` estático): https://tc39.es/proposal-temporal/#sec-temporal-roundtimeduration
/// O erro é o `RangeError` que o C++ lança em `globalObject`.
pub fn round_time_duration(
    time_duration: i128,
    increment: u32,
    unit: TemporalUnit,
    rounding_mode: crate::runtime::temporal_object::RoundingMode,
) -> crate::runtime::temporal_core_types::TemporalResult<i128> {
    let divisor = crate::runtime::temporal_object::length_in_nanoseconds(unit);
    let rounded = crate::runtime::temporal_core_rounding::round_number_to_increment_i128(
        time_duration,
        divisor * i128::from(increment),
        rounding_mode,
    );
    if rounded.abs() > InternalDuration::MAX_TIME_DURATION {
        return Err(crate::runtime::temporal_core_types::range_error("Rounded time duration exceeds maximum"));
    }
    Ok(rounded)
}

/// `formatTimeZoneOffsetString(offset)`: `+HH:MM`, com `:SS` se há segundos e `.fff` se há fração (os zeros
/// à direita saem).
pub fn format_time_zone_offset_string(offset: i64) -> String {
    let negative = offset < 0;
    // O deslocamento é bem mais estreito que o intervalo de `i64`.
    let offset = offset.abs();
    let ns_per_second = ExactTime::NS_PER_SECOND as i64;
    let nanoseconds = offset % ns_per_second;
    let seconds = (offset / ns_per_second) % 60;
    let minutes = (offset / ExactTime::NS_PER_MINUTE as i64) % 60;
    let hours = offset / ExactTime::NS_PER_HOUR as i64;
    let sign = if negative { '-' } else { '+' };

    if nanoseconds != 0 {
        let fraction = format!("{nanoseconds:09}");
        return format!("{sign}{hours:02}:{minutes:02}:{seconds:02}.{}", fraction.trim_end_matches('0'));
    }
    if seconds != 0 {
        return format!("{sign}{hours:02}:{minutes:02}:{seconds:02}");
    }
    format!("{sign}{hours:02}:{minutes:02}")
}

const DAYS_IN_MONTHS: [[u8; 12]; 2] =
    [[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31], [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]];

/// `daysInMonth(int32_t year, uint8_t month)`: https://tc39.es/proposal-temporal/#sec-temporal-isodaysinmonth
pub fn days_in_month(year: i32, month: u8) -> u8 {
    DAYS_IN_MONTHS[usize::from(is_leap_year(year))][usize::from(month) - 1]
}

/// `daysInMonth(uint8_t month)`: o mês de um ano bissexto.
pub fn days_in_month_of_leap_year(month: u8) -> u8 {
    DAYS_IN_MONTHS[1][usize::from(month) - 1]
}

/// `dayOfWeek(PlainDate)`: de 1 (segunda) a 7 (domingo).
pub fn day_of_week(plain_date: PlainDate) -> u8 {
    let days = days_from_year_month(plain_date.year(), i32::from(plain_date.month()) - 1) + (i32::from(plain_date.day()) - 1);
    let week_day = week_day(days);
    if week_day == 0 {
        7
    } else {
        week_day as u8
    }
}

/// `dayOfYear(PlainDate)`: começa em 1 (1/1 é 1).
pub fn day_of_year(plain_date: PlainDate) -> u16 {
    (day_in_year(plain_date.year(), i32::from(plain_date.month()) - 1, i32::from(plain_date.day())) + 1) as u16
}

/// `isValidISODate(double year, double month, double day)`. As conversões de `double` para `int32_t` e
/// `uint8_t` são as implícitas do C++ (aqui `as`, que satura).
pub fn is_valid_iso_date(year: f64, month: f64, day: f64) -> bool {
    if !(1.0..=12.0).contains(&month) {
        return false;
    }
    let days_in_month = days_in_month(year as i32, month as u8);
    !(day < 1.0 || day > f64::from(days_in_month))
}

/// `handleFraction` (`DurationHandleFractions`): https://tc39.es/proposal-temporal/#sec-temporal-durationhandlefractions
fn handle_fraction(duration: &mut Duration, factor: i64, fraction_digits: &[u16], fraction_type: TemporalUnit) {
    debug_assert!(!fraction_digits.is_empty() && fraction_digits.len() <= 9);
    debug_assert!(matches!(fraction_type, TemporalUnit::Hour | TemporalUnit::Minute | TemporalUnit::Second));

    let mut padded = [b'0' as u16; 9];
    padded[..fraction_digits.len()].copy_from_slice(fraction_digits);
    let parsed = padded.iter().fold(0i64, |acc, &c| acc * 10 + i64::from(c - b'0' as u16));

    let mut fraction = factor * parsed;
    if fraction == 0 {
        return;
    }

    const DIVISOR: i64 = 1_000_000_000;
    if fraction_type == TemporalUnit::Hour {
        fraction *= 60;
        duration.set_minutes(fraction / DIVISOR);
        fraction %= DIVISOR;
        if fraction == 0 {
            return;
        }
    }

    if fraction_type != TemporalUnit::Second {
        fraction *= 60;
        duration.set_seconds(fraction / DIVISOR);
        fraction %= DIVISOR;
        if fraction == 0 {
            return;
        }
    }

    const NS_PER_MILLISECOND: i64 = 1_000_000;
    const NS_PER_MICROSECOND: i64 = 1_000;
    duration.set_milliseconds(fraction / NS_PER_MILLISECOND);
    duration.set_microseconds(i128::from(fraction % NS_PER_MILLISECOND / NS_PER_MICROSECOND));
    duration.set_nanoseconds(i128::from(fraction % NS_PER_MICROSECOND));
}

fn is_digit(c: u16) -> bool {
    (b'0' as u16..=b'9' as u16).contains(&c)
}

fn to_ascii_upper(c: u16) -> u16 {
    if (b'a' as u16..=b'z' as u16).contains(&c) {
        c - 0x20
    } else {
        c
    }
}

/// O inteiro decimal `digits` como `double` (`parseInt(digits, 10)`: arredondamento correto).
fn parse_decimal_digits(digits: &[u16]) -> f64 {
    let text: String = digits.iter().map(|&c| char::from(c as u8)).collect();
    text.parse::<f64>().unwrap_or(f64::INFINITY)
}

/// Quantos dígitos decimais começam em `buffer[pos]` (pelo menos 1, o chamador já viu um).
fn count_digits(buffer: &[u16], pos: usize) -> usize {
    buffer[pos..].iter().take_while(|&&c| is_digit(c)).count()
}

/// `parseDuration(StringView)` (`ParseTemporalDurationString`):
/// https://tc39.es/proposal-temporal/#sec-temporal-parsetemporaldurationstring
///
/// Cadeias como `-P1Y2M3W4DT5H6M7.123456789S`: maiúsculas ou minúsculas, sinal `+`/`-`, separador `.` ou `,`,
/// `T` só quando há parte de tempo, inteiros de qualquer tamanho e fração de no máximo 9 dígitos; horas e
/// minutos podem ter fração, só na última parte.
pub fn parse_duration(buffer: &[u16]) -> Option<Duration> {
    if buffer.len() < 3 {
        return None;
    }
    let mut result = Duration::default();
    let mut pos = 0usize;

    let mut factor: i64 = 1;
    if buffer[pos] == b'+' as u16 {
        pos += 1;
    } else if buffer[pos] == b'-' as u16 {
        factor = -1;
        pos += 1;
    }

    if to_ascii_upper(buffer[pos]) != b'P' as u16 {
        return None;
    }
    pos += 1;

    let mut date_part_index = 0u32;
    while date_part_index < 4 && pos < buffer.len() && is_digit(buffer[pos]) {
        let digits = count_digits(buffer, pos);
        let integer = factor as f64 * parse_decimal_digits(&buffer[pos..pos + digits]);
        pos += digits;
        if pos >= buffer.len() {
            return None;
        }

        match to_ascii_upper(buffer[pos]) as u8 as char {
            'Y' if buffer[pos] < 0x80 => {
                if date_part_index != 0 {
                    return None;
                }
                result.set_field(TemporalUnit::Year, integer);
                date_part_index = 1;
            }
            'M' if buffer[pos] < 0x80 => {
                if date_part_index >= 2 {
                    return None;
                }
                result.set_field(TemporalUnit::Month, integer);
                date_part_index = 2;
            }
            'W' if buffer[pos] < 0x80 => {
                if date_part_index >= 3 {
                    return None;
                }
                result.set_field(TemporalUnit::Week, integer);
                date_part_index = 3;
            }
            'D' if buffer[pos] < 0x80 => {
                result.set_field(TemporalUnit::Day, integer);
                date_part_index = 4;
            }
            _ => return None,
        }
        pos += 1;
    }

    if pos >= buffer.len() {
        return Some(result);
    }

    if buffer.len() - pos < 3 || to_ascii_upper(buffer[pos]) != b'T' as u16 {
        return None;
    }
    pos += 1;

    let mut time_part_index = 0u32;
    while time_part_index < 3 && pos < buffer.len() && is_digit(buffer[pos]) {
        let digits = count_digits(buffer, pos);
        let integer = factor as f64 * parse_decimal_digits(&buffer[pos..pos + digits]);
        pos += digits;
        if pos >= buffer.len() {
            return None;
        }

        let mut fractional_part: &[u16] = &[];
        if buffer[pos] == b'.' as u16 || buffer[pos] == b',' as u16 {
            pos += 1;
            let digits = count_digits(buffer, pos);
            if digits == 0 || digits > 9 {
                return None;
            }
            fractional_part = &buffer[pos..pos + digits];
            pos += digits;
            if pos >= buffer.len() {
                return None;
            }
        }

        let designator = if buffer[pos] < 0x80 { to_ascii_upper(buffer[pos]) as u8 as char } else { '\0' };
        match designator {
            'H' => {
                if time_part_index != 0 {
                    return None;
                }
                result.set_field(TemporalUnit::Hour, integer);
                if fractional_part.is_empty() {
                    time_part_index = 1;
                } else {
                    handle_fraction(&mut result, factor, fractional_part, TemporalUnit::Hour);
                    time_part_index = 3;
                }
            }
            'M' => {
                if time_part_index >= 2 {
                    return None;
                }
                result.set_field(TemporalUnit::Minute, integer);
                if fractional_part.is_empty() {
                    time_part_index = 2;
                } else {
                    handle_fraction(&mut result, factor, fractional_part, TemporalUnit::Minute);
                    time_part_index = 3;
                }
            }
            'S' => {
                result.set_field(TemporalUnit::Second, integer);
                if !fractional_part.is_empty() {
                    handle_fraction(&mut result, factor, fractional_part, TemporalUnit::Second);
                }
                time_part_index = 3;
            }
            _ => return None,
        }
        pos += 1;
    }

    if pos < buffer.len() {
        return None;
    }
    Some(result)
}

// ---------------------------------------------------------------------------------------------
// ParseISODateTime e os tokenizadores
// ---------------------------------------------------------------------------------------------
//
// DIVERGÊNCIAS: o `StringParsingBuffer<CharacterType>` do C++ (Latin1 ou UTF-16) é um cursor sobre `&[u16]`;
// ler além do fim devolve 0 em vez de ser comportamento indefinido (o C++ só lê depois de conferir
// `lengthRemaining()`, então o valor nunca é observado). `Vector<Latin1Character>` é `Vec<u8>`: os
// trechos guardados (nome de fuso, calendário) já foram conferidos como ASCII. `parseTimeZoneName` e
// `parseTemporalTimeZoneString` seguem fora: dependem de `intlResolveTimeZoneID`.

use crate::wtf::option_set::{OptionSet, OptionSetFlag};

/// `enum class SubMinutePrecision : bool`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubMinutePrecision {
    No,
    Yes,
}

/// `enum class TemporalProduction : uint8_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporalProduction {
    /// `TemporalInstantString`.
    Instant,
    /// `TemporalDateTimeString[+Zoned]`.
    DateTimeZoned,
    /// `TemporalDateTimeString[~Zoned]`.
    DateTimeUnzoned,
    /// `TemporalYearMonthString`.
    YearMonth,
    /// `TemporalMonthDayString`.
    MonthDay,
    /// `TemporalTimeString`.
    Time,
}

impl OptionSetFlag for TemporalProduction {
    type Mask = u8;
    const NONE: u8 = 0;
    const ALL: &'static [TemporalProduction] = &[
        TemporalProduction::Instant,
        TemporalProduction::DateTimeZoned,
        TemporalProduction::DateTimeUnzoned,
        TemporalProduction::YearMonth,
        TemporalProduction::MonthDay,
        TemporalProduction::Time,
    ];

    fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// `TemporalProductionSet`.
pub type TemporalProductionSet = OptionSet<TemporalProduction>;

/// `CalendarID` (`RFC9557Value`): o identificador de calendário do `[u-ca=...]`, em ASCII.
pub type CalendarId = Vec<u8>;

/// `minCalendarLength` e `maxCalendarLength`.
const MIN_CALENDAR_LENGTH: usize = 3;
const MAX_CALENDAR_LENGTH: usize = 8;

/// O `Variant<Vector<Latin1Character>, int64_t>` de `[[TimeZoneAnnotation]]`: o nome IANA ou o deslocamento
/// em nanossegundos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimeZoneNameOrOffset {
    Name(Vec<u8>),
    Offset(i64),
}

/// `ISO8601::ISOStringTimeZoneParseRecord`: https://tc39.es/proposal-temporal/#sec-temporal-iso-string-time-zone-parse-records
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ISOStringTimeZoneParseRecord {
    /// `m_z`.
    pub z: bool,
    /// `m_offset`.
    pub offset: Option<i64>,
    /// `m_nameOrOffset`; um nome vazio é `~empty~`.
    pub name_or_offset: TimeZoneNameOrOffset,
    /// `m_offsetHasSubMinutePrecision`.
    pub offset_has_sub_minute_precision: bool,
}

/// `ISO8601::TimeZoneIdentifierParseRecord`: `name` vazio é `~empty~` e `offset_minutes` `None` também.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeZoneIdentifierParseRecord {
    pub name: Vec<u8>,
    pub offset_minutes: Option<i64>,
}

/// `ISO8601::ParsedISODateTime`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedISODateTime {
    pub date: Option<PlainDate>,
    pub time: Option<PlainTime>,
    pub time_zone: Option<ISOStringTimeZoneParseRecord>,
    pub calendar: Option<CalendarId>,
    pub matched: TemporalProduction,
    /// Verdadeiro quando o objetivo casado foi a forma curta (`AnnotatedYearMonth` sem `DateDay` ou
    /// `AnnotatedMonthDay` sem `DateYear`).
    pub is_short_form: bool,
}

/// `StringParsingBuffer`: um cursor sobre as unidades UTF-16 da cadeia.
#[derive(Clone, Copy)]
struct ParsingBuffer<'a> {
    data: &'a [u16],
    position: usize,
}

impl<'a> ParsingBuffer<'a> {
    fn new(data: &'a [u16]) -> ParsingBuffer<'a> {
        ParsingBuffer { data, position: 0 }
    }

    fn at_end(&self) -> bool {
        self.position >= self.data.len()
    }

    fn length_remaining(&self) -> usize {
        self.data.len().saturating_sub(self.position)
    }

    /// `operator*`.
    fn current(&self) -> u16 {
        self.at(0)
    }

    /// `operator[](index)`, relativo à posição.
    fn at(&self, index: usize) -> u16 {
        self.data.get(self.position + index).copied().unwrap_or(0)
    }

    fn advance(&mut self) {
        self.advance_by(1);
    }

    fn advance_by(&mut self, count: usize) {
        self.position = (self.position + count).min(self.data.len());
    }

    /// `span()`: o que falta ler.
    fn span(&self) -> &'a [u16] {
        &self.data[self.position.min(self.data.len())..]
    }

    /// `consume(count)`: devolve os próximos `count` e avança.
    fn consume(&mut self, count: usize) -> &'a [u16] {
        let span = &self.span()[..count];
        self.advance_by(count);
        span
    }
}

fn is_between(c: u16, low: u8, high: u8) -> bool {
    c >= u16::from(low) && c <= u16::from(high)
}

fn is_ascii_lower(c: u16) -> bool {
    is_between(c, b'a', b'z')
}

fn is_ascii_alpha(c: u16) -> bool {
    is_ascii_lower(c) || is_between(c, b'A', b'Z')
}

/// O valor de um dígito ASCII já conferido.
fn digit_value(c: u16) -> u32 {
    u32::from(c - u16::from(b'0'))
}

/// `parseDecimalInt32(characters)`: só dígitos ASCII, conferidos pelo chamador.
fn parse_decimal_int32(characters: &[u16]) -> i32 {
    characters.iter().fold(0i32, |result, &c| result * 10 + digit_value(c) as i32)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Second60Mode {
    Accept,
    Reject,
}

/// `parseTimeSpec(buffer, second60Mode, subMinutePrecision, outHasSeconds)`:
/// https://tc39.es/proposal-temporal/#prod-TimeSpec. `TimeSecond` 60 vira 59 (passo 18.b de `ParseISODateTime`).
fn parse_time_spec(
    buffer: &mut ParsingBuffer,
    second60_mode: Second60Mode,
    sub_minute_precision: SubMinutePrecision,
    out_has_seconds: Option<&mut bool>,
) -> Option<PlainTime> {
    if buffer.length_remaining() < 2 {
        return None;
    }

    let first_hour_character = buffer.current();
    if !is_between(first_hour_character, b'0', b'2') {
        return None;
    }
    buffer.advance();
    let second_hour_character = buffer.current();
    if !is_digit(second_hour_character) {
        return None;
    }
    let hour = digit_value(second_hour_character) + 10 * digit_value(first_hour_character);
    if hour >= 24 {
        return None;
    }
    buffer.advance();

    if buffer.at_end() {
        return Some(PlainTime::new(hour, 0, 0, 0, 0, 0));
    }

    let mut split_by_colon = false;
    if buffer.current() == u16::from(b':') {
        split_by_colon = true;
        buffer.advance();
    } else if !is_between(buffer.current(), b'0', b'5') {
        return Some(PlainTime::new(hour, 0, 0, 0, 0, 0));
    }

    if buffer.length_remaining() < 2 {
        return None;
    }
    let first_minute_character = buffer.current();
    if !is_between(first_minute_character, b'0', b'5') {
        return None;
    }
    buffer.advance();
    let second_minute_character = buffer.current();
    if !is_digit(second_minute_character) {
        return None;
    }
    let minute = digit_value(second_minute_character) + 10 * digit_value(first_minute_character);
    debug_assert!(minute < 60);
    buffer.advance();

    if buffer.at_end() {
        return Some(PlainTime::new(hour, minute, 0, 0, 0, 0));
    }

    if split_by_colon {
        if buffer.current() == u16::from(b':') {
            buffer.advance();
        } else {
            return Some(PlainTime::new(hour, minute, 0, 0, 0, 0));
        }
    } else {
        let highest_second_digit = if second60_mode == Second60Mode::Accept { b'6' } else { b'5' };
        if !is_between(buffer.current(), b'0', highest_second_digit) {
            return Some(PlainTime::new(hour, minute, 0, 0, 0, 0));
        }
    }

    if sub_minute_precision == SubMinutePrecision::No {
        return None;
    }

    if let Some(has_seconds) = out_has_seconds {
        *has_seconds = true;
    }
    if buffer.length_remaining() < 2 {
        return None;
    }
    let first_second_character = buffer.current();
    let second;
    if is_between(first_second_character, b'0', b'5') {
        buffer.advance();
        let second_second_character = buffer.current();
        if !is_digit(second_second_character) {
            return None;
        }
        second = digit_value(second_second_character) + 10 * digit_value(first_second_character);
        debug_assert!(second < 60);
        buffer.advance();
    } else if second60_mode == Second60Mode::Accept && first_second_character == u16::from(b'6') {
        buffer.advance();
        if buffer.current() != u16::from(b'0') {
            return None;
        }
        second = 59;
        buffer.advance();
    } else {
        return None;
    }

    if buffer.at_end() {
        return Some(PlainTime::new(hour, minute, second, 0, 0, 0));
    }

    if buffer.current() != u16::from(b'.') && buffer.current() != u16::from(b',') {
        return Some(PlainTime::new(hour, minute, second, 0, 0, 0));
    }
    buffer.advance();

    let max_count = buffer.length_remaining().min(9);
    let digits = (0..max_count).take_while(|&index| is_digit(buffer.at(index))).count();
    if digits == 0 {
        return None;
    }

    let mut padded = [u16::from(b'0'); 9];
    for (index, slot) in padded.iter_mut().enumerate().take(digits) {
        *slot = buffer.at(index);
    }
    buffer.advance_by(digits);

    let millisecond = parse_decimal_int32(&padded[0..3]) as u32;
    let microsecond = parse_decimal_int32(&padded[3..6]) as u32;
    let nanosecond = parse_decimal_int32(&padded[6..9]) as u32;
    Some(PlainTime::new(hour, minute, second, millisecond, microsecond, nanosecond))
}

/// `parseUTCOffset(buffer, subMinutePrecision, outHasSubMinutePrecision)`: `UTCOffset[SubMinutePrecision]`, que é
/// `ASCIISign TimeSpec`. O intervalo vai de -23:59:59.999999999 a +23:59:59.999999999 e cabe em `i64`.
fn parse_utc_offset_in_buffer(
    buffer: &mut ParsingBuffer,
    sub_minute_precision: SubMinutePrecision,
    out_has_sub_minute_precision: Option<&mut bool>,
) -> Option<i64> {
    if buffer.length_remaining() < 3 {
        return None;
    }

    let mut factor: i64 = 1;
    if buffer.current() == u16::from(b'+') {
        buffer.advance();
    } else if buffer.current() == u16::from(b'-') {
        factor = -1;
        buffer.advance();
    } else {
        return None;
    }

    let mut has_seconds = false;
    let plain_time = parse_time_spec(buffer, Second60Mode::Reject, sub_minute_precision, Some(&mut has_seconds))?;

    if let Some(flag) = out_has_sub_minute_precision {
        *flag = has_seconds;
    }

    let nanoseconds = ExactTime::NS_PER_HOUR as i64 * i64::from(plain_time.hour())
        + ExactTime::NS_PER_MINUTE as i64 * i64::from(plain_time.minute())
        + ExactTime::NS_PER_SECOND as i64 * i64::from(plain_time.second())
        + 1_000_000 * i64::from(plain_time.millisecond())
        + 1_000 * i64::from(plain_time.microsecond())
        + i64::from(plain_time.nanosecond());
    Some(nanoseconds * factor)
}

/// `parseUTCOffset(StringView, SubMinutePrecision)`: a cadeia inteira tem de ser um `UTCOffset`.
pub fn parse_utc_offset(string: &[u16], sub_minute_precision: SubMinutePrecision) -> Option<i64> {
    let mut buffer = ParsingBuffer::new(string);
    let result = parse_utc_offset_in_buffer(&mut buffer, sub_minute_precision, None);
    if !buffer.at_end() {
        return None;
    }
    result
}

/// `canBeRFC9557Annotation(buffer)`: `[`, a marca crítica `!` opcional, uma chave válida e o `=`.
/// https://tc39.es/proposal-temporal/#prod-Annotation
fn can_be_rfc9557_annotation(buffer: &ParsingBuffer) -> bool {
    let length = buffer.length_remaining();
    // `[`, `=`, `]`, a chave e o valor: pelo menos 5.
    if length < 5 {
        return false;
    }
    if buffer.current() != u16::from(b'[') {
        return false;
    }
    let mut index = 1;
    if buffer.at(index) == u16::from(b'!') {
        index += 1;
    }
    if !is_ascii_lower(buffer.at(index)) && buffer.at(index) != u16::from(b'_') {
        return false;
    }
    index += 1;
    while index < length {
        let character = buffer.at(index);
        if character == u16::from(b'=') {
            return true;
        }
        if is_ascii_lower(character) || is_digit(character) || character == u16::from(b'-') || character == u16::from(b'_') {
            index += 1;
        } else {
            return false;
        }
    }
    false
}

/// `canBeTimeZone(buffer, character)`: `Z`, um sinal ou um `[` que não seja anotação RFC 9557.
fn can_be_time_zone(buffer: &ParsingBuffer, character: u16) -> bool {
    match u8::try_from(character) {
        Ok(b'z' | b'Z' | b'+' | b'-') => true,
        Ok(b'[') => !can_be_rfc9557_annotation(buffer),
        _ => false,
    }
}

/// `isTZLeadingChar`: https://tc39.es/proposal-temporal/#prod-TZLeadingChar
fn is_tz_leading_char(character: u16) -> bool {
    is_ascii_alpha(character) || character == u16::from(b'.') || character == u16::from(b'_')
}

/// `isTZChar`: https://tc39.es/proposal-temporal/#prod-TZChar
fn is_tz_char(character: u16) -> bool {
    is_tz_leading_char(character) || is_digit(character) || character == u16::from(b'-') || character == u16::from(b'+')
}

/// `parseTimeZoneIANAName(buffer)`: componentes separados por `/`, cada um um `TZLeadingChar` e depois
/// `TZChar`s. Não consome nada em caso de falha.
fn parse_time_zone_iana_name<'a>(buffer: &mut ParsingBuffer<'a>) -> Option<&'a [u16]> {
    let mut length = 0;
    let mut at_component_start = true;
    while length < buffer.length_remaining() {
        let character = buffer.at(length);
        if character == u16::from(b'/') {
            if at_component_start {
                break;
            }
            at_component_start = true;
        } else if if at_component_start { is_tz_leading_char(character) } else { is_tz_char(character) } {
            at_component_start = false;
        } else {
            break;
        }
        length += 1;
    }
    if at_component_start {
        return None;
    }
    Some(buffer.consume(length))
}

/// Os caracteres de um trecho já conferido como ASCII, em bytes.
fn ascii_bytes(span: &[u16]) -> Vec<u8> {
    span.iter().map(|&c| c as u8).collect()
}

/// `parseTimeZoneIdentifier(buffer)`: https://tc39.es/proposal-temporal/#sec-parsetimezoneidentifier
fn parse_time_zone_identifier(buffer: &mut ParsingBuffer) -> Option<TimeZoneIdentifierParseRecord> {
    // Passo 3: as duas alternativas são disjuntas e o nome IANA não consome nada em caso de falha.
    if let Some(name) = parse_time_zone_iana_name(buffer) {
        return Some(TimeZoneIdentifierParseRecord { name: ascii_bytes(name), offset_minutes: None });
    }

    // Passos 4 a 6: `UTCOffset[~SubMinutePrecision]`.
    let offset_nanoseconds = parse_utc_offset_in_buffer(buffer, SubMinutePrecision::No, None)?;

    // Passos 7 e 8: sem precisão de subminuto a divisão é exata.
    Some(TimeZoneIdentifierParseRecord { name: Vec::new(), offset_minutes: Some(offset_nanoseconds / ExactTime::NS_PER_MINUTE as i64) })
}

/// `parseTimeZoneIdentifier(StringView)` (`ParseTimeZoneIdentifier`): a cadeia inteira tem de ser um
/// `TimeZoneIdentifier`. O nome IANA sai como está, sem conferir a lista de fusos disponíveis (a nota do passo 3.b).
pub fn parse_time_zone_identifier_string(identifier: &[u16]) -> Option<TimeZoneIdentifierParseRecord> {
    let mut buffer = ParsingBuffer::new(identifier);
    let result = parse_time_zone_identifier(&mut buffer)?;
    buffer.at_end().then_some(result)
}

/// `parseTemporalTimeZoneString(StringView)` (`ParseTemporalTimeZoneString`):
/// https://tc39.es/proposal-temporal/#sec-temporal-parsetemporaltimezonestring
/// `None` é o `RangeError` (o chamador escolhe a mensagem).
pub fn parse_temporal_time_zone_string(time_zone_string: &[u16]) -> Option<TimeZoneIdentifierParseRecord> {
    // Passos 1 e 2: `TimeZoneIdentifier` direto.
    if let Some(parse) = parse_time_zone_identifier_string(time_zone_string) {
        return Some(parse);
    }

    // Passo 3: `ParseISODateTime` com seis objetivos.
    let mut allowed = TemporalProductionSet::from(TemporalProduction::DateTimeZoned);
    for production in [
        TemporalProduction::DateTimeUnzoned,
        TemporalProduction::Instant,
        TemporalProduction::Time,
        TemporalProduction::MonthDay,
        TemporalProduction::YearMonth,
    ] {
        allowed = allowed | TemporalProductionSet::from(production);
    }
    let parsed = parse_iso_date_time(time_zone_string, allowed)?;
    // Passo 4: `timeZoneResult`.
    let time_zone_result = parsed.time_zone?;

    // Passo 5: a anotação, deslocamento ou nome.
    match &time_zone_result.name_or_offset {
        TimeZoneNameOrOffset::Offset(offset_nanoseconds) => {
            return Some(TimeZoneIdentifierParseRecord { name: Vec::new(), offset_minutes: Some(offset_nanoseconds / ExactTime::NS_PER_MINUTE as i64) });
        }
        TimeZoneNameOrOffset::Name(name) if !name.is_empty() => {
            return Some(TimeZoneIdentifierParseRecord { name: name.clone(), offset_minutes: None });
        }
        TimeZoneNameOrOffset::Name(_) => {}
    }

    // Passo 6: `Z` é `UTC`.
    if time_zone_result.z {
        return Some(TimeZoneIdentifierParseRecord { name: b"UTC".to_vec(), offset_minutes: None });
    }

    // Passo 7: o deslocamento da cadeia, sem precisão de subminuto.
    if let Some(offset) = time_zone_result.offset {
        if time_zone_result.offset_has_sub_minute_precision {
            return None;
        }
        return Some(TimeZoneIdentifierParseRecord { name: Vec::new(), offset_minutes: Some(offset / ExactTime::NS_PER_MINUTE as i64) });
    }

    // Passo 8: `RangeError`.
    None
}

/// `parseTimeZoneAnnotation(buffer)`: `[ !? TimeZoneIdentifier ]`.
fn parse_time_zone_annotation(buffer: &mut ParsingBuffer) -> Option<TimeZoneNameOrOffset> {
    if buffer.length_remaining() < 3 {
        return None;
    }

    if buffer.current() != u16::from(b'[') {
        return None;
    }
    buffer.advance();

    if buffer.current() == u16::from(b'!') {
        buffer.advance();
    }

    // O identificador para sozinho em `]`: não é `TZChar` nem parte de `UTCOffset`.
    let identifier = parse_time_zone_identifier(buffer)?;

    if buffer.at_end() || buffer.current() != u16::from(b']') {
        return None;
    }
    buffer.advance();

    // `[[OffsetMinutes]]` é em minutos; o guardado é o deslocamento em nanossegundos.
    match identifier.offset_minutes {
        Some(minutes) => Some(TimeZoneNameOrOffset::Offset(minutes * ExactTime::NS_PER_MINUTE as i64)),
        None => Some(TimeZoneNameOrOffset::Name(identifier.name)),
    }
}

/// `RFC9557Key`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rfc9557Key {
    Calendar,
    Other,
}

/// `RFC9557Annotation`: a marca crítica, a chave e o valor (só o de `u-ca` é guardado).
struct Rfc9557Annotation {
    critical: bool,
    key: Rfc9557Key,
    value: CalendarId,
}

/// `parseOneRFC9557Annotation(buffer)`: uma anotação `[!key=value]`; só `u-ca` é conhecida.
fn parse_one_rfc9557_annotation(buffer: &mut ParsingBuffer) -> Option<Rfc9557Annotation> {
    if !can_be_rfc9557_annotation(buffer) {
        return None;
    }
    let critical = buffer.at(1) == u16::from(b'!');
    // Pula `[` ou `[!`.
    buffer.advance_by(if critical { 2 } else { 1 });

    // A chave.
    let mut key_length = 0;
    while buffer.at(key_length) != u16::from(b'=') {
        key_length += 1;
    }
    if key_length == 0 {
        return None;
    }
    let key = &buffer.span()[..key_length];
    let is_calendar_key = key == "u-ca".encode_utf16().collect::<Vec<u16>>().as_slice();
    buffer.advance_by(key_length);

    if buffer.at_end() {
        return None;
    }

    // Consome o `=`.
    buffer.advance();

    let mut index = 0;
    while index < buffer.length_remaining() {
        let character = buffer.at(index);
        if character == u16::from(b']') {
            break;
        }
        if !is_ascii_alpha(character) && !is_digit(character) && character != u16::from(b'-') {
            return None;
        }
        index += 1;
    }
    if index == 0 {
        return None;
    }
    let name_length = index;

    if !is_calendar_key {
        // Anotação desconhecida: consome o resto dela.
        buffer.advance_by(name_length);
        if buffer.at_end() || buffer.current() != u16::from(b']') {
            return None;
        }
        buffer.advance();
        return Some(Rfc9557Annotation { critical, key: Rfc9557Key::Other, value: Vec::new() });
    }

    let is_valid_component = |start: usize, end: usize| {
        let component_length = end - start;
        (MIN_CALENDAR_LENGTH..=MAX_CALENDAR_LENGTH).contains(&component_length)
    };

    let mut current_name_component_start_index = 0;
    let mut is_leading_character_in_name_component = true;
    for index in 0..name_length {
        let character = buffer.at(index);
        if is_leading_character_in_name_component {
            if !(is_ascii_alpha(character) || is_digit(character)) {
                return None;
            }
            current_name_component_start_index = index;
            is_leading_character_in_name_component = false;
            continue;
        }

        if character == u16::from(b'-') {
            if !is_valid_component(current_name_component_start_index, index) {
                return None;
            }
            is_leading_character_in_name_component = true;
            continue;
        }

        if !(is_ascii_alpha(character) || is_digit(character)) {
            return None;
        }
    }
    if is_leading_character_in_name_component {
        return None;
    }
    if !is_valid_component(current_name_component_start_index, name_length) {
        return None;
    }

    let value = ascii_bytes(buffer.consume(name_length));

    if buffer.at_end() {
        return None;
    }
    if buffer.current() != u16::from(b']') {
        return None;
    }
    buffer.advance();
    Some(Rfc9557Annotation { critical, key: Rfc9557Key::Calendar, value })
}

/// `parseCalendar(buffer)`: `Annotations`, com as regras de calendário múltiplo e de marca crítica do passo 4
/// de `ParseISODateTime`. Devolve os calendários vistos, em ordem.
fn parse_calendar(buffer: &mut ParsingBuffer) -> Option<Vec<CalendarId>> {
    if !can_be_rfc9557_annotation(buffer) {
        return None;
    }

    let mut result: Vec<CalendarId> = Vec::new();
    let mut calendar_was_critical = false;
    while can_be_rfc9557_annotation(buffer) {
        let annotation = parse_one_rfc9557_annotation(buffer)?;
        if annotation.key == Rfc9557Key::Calendar {
            result.push(annotation.value);
        }
        if annotation.critical {
            // Passo 4.a.ii.2.d.i: anotação desconhecida com a marca crítica.
            if annotation.key != Rfc9557Key::Calendar {
                return None;
            }
            // Passo 4.a.ii.2.c.ii: vários calendários com a marca crítica.
            if result.len() == 1 {
                calendar_was_critical = true;
            } else {
                return None;
            }
        }
        if calendar_was_critical && result.len() > 1 {
            return None;
        }
    }
    Some(result)
}

/// `parseDateYear(buffer)`: `DateYear`, de quatro dígitos ou com sinal e seis; `-000000` não existe.
fn parse_date_year(buffer: &mut ParsingBuffer) -> Option<i32> {
    if buffer.at_end() {
        return None;
    }
    let mut extended = false;
    let mut factor = 1;
    if buffer.current() == u16::from(b'+') {
        buffer.advance();
        extended = true;
    } else if buffer.current() == u16::from(b'-') {
        buffer.advance();
        extended = true;
        factor = -1;
    }
    let digits = if extended { 6 } else { 4 };
    if buffer.length_remaining() < digits {
        return None;
    }
    if !(0..digits).all(|index| is_digit(buffer.at(index))) {
        return None;
    }
    let year = parse_decimal_int32(&buffer.span()[..digits]) * factor;
    if year == 0 && factor < 0 {
        return None;
    }
    buffer.advance_by(digits);
    Some(year)
}

/// `parseDateMonth(buffer)`.
fn parse_date_month(buffer: &mut ParsingBuffer) -> Option<u32> {
    if buffer.length_remaining() < 2 {
        return None;
    }
    let c1 = buffer.current();
    if c1 != u16::from(b'0') && c1 != u16::from(b'1') {
        return None;
    }
    let c2 = buffer.at(1);
    if !is_digit(c2) {
        return None;
    }
    let month = digit_value(c2) + 10 * digit_value(c1);
    if month == 0 || month > 12 {
        return None;
    }
    buffer.advance_by(2);
    Some(month)
}

/// `parseDateDay(buffer, year, month)`.
fn parse_date_day(buffer: &mut ParsingBuffer, year: i32, month: u32) -> Option<u32> {
    if buffer.length_remaining() < 2 {
        return None;
    }
    let c1 = buffer.current();
    if !is_between(c1, b'0', b'3') {
        return None;
    }
    let c2 = buffer.at(1);
    if !is_digit(c2) {
        return None;
    }
    let day = digit_value(c2) + 10 * digit_value(c1);
    if day == 0 || day > u32::from(days_in_month(year, month as u8)) {
        return None;
    }
    buffer.advance_by(2);
    Some(day)
}

/// `parseDate(buffer)`: `YYYY-MM-DD` ou `YYYYMMDD`, com os dois separadores no mesmo modo.
fn parse_date(buffer: &mut ParsingBuffer) -> Option<PlainDate> {
    let year = parse_date_year(buffer)?;
    let mut extended = false;
    if !buffer.at_end() && buffer.current() == u16::from(b'-') {
        extended = true;
        buffer.advance();
    }
    let month = parse_date_month(buffer)?;
    if extended {
        if buffer.at_end() || buffer.current() != u16::from(b'-') {
            return None;
        }
        buffer.advance();
    } else if !buffer.at_end() && buffer.current() == u16::from(b'-') {
        // Compacto e estendido misturados são proibidos.
        return None;
    }
    let day = parse_date_day(buffer, year, month)?;
    Some(PlainDate::new(i64::from(year), month, day))
}

/// `parseDateSpecYearMonth(buffer)`: `YYYY-MM` ou `YYYYMM`.
fn parse_date_spec_year_month(buffer: &mut ParsingBuffer) -> Option<PlainDate> {
    let year = parse_date_year(buffer)?;
    if !buffer.at_end() && buffer.current() == u16::from(b'-') {
        buffer.advance();
    }
    let month = parse_date_month(buffer)?;
    Some(PlainDate::new(i64::from(year), month, 1))
}

/// `parseDateSpecMonthDay(buffer)`: `--MM-DD`, `--MMDD`, `MM-DD` ou `MMDD` (ano de referência 1972).
fn parse_date_spec_month_day(buffer: &mut ParsingBuffer) -> Option<PlainDate> {
    if buffer.length_remaining() >= 2 && buffer.at(0) == u16::from(b'-') && buffer.at(1) == u16::from(b'-') {
        buffer.advance_by(2);
    }
    let month = parse_date_month(buffer)?;
    if !buffer.at_end() && buffer.current() == u16::from(b'-') {
        buffer.advance();
    }
    let day = parse_date_day(buffer, 1972, month)?;
    Some(PlainDate::new(1972, month, day))
}

/// `ISO8601ParseTokens`: o que um tokenizador achou, antes de `ParseISODateTime` montar o registro.
struct ISO8601ParseTokens {
    year: Option<i32>,
    month: Option<u32>,
    day: Option<u32>,
    time: Option<PlainTime>,
    has_utc_designator: bool,
    utc_offset_ns: Option<i64>,
    offset_has_sub_minute_precision: bool,
    /// O `std::monostate` do C++ é `None`.
    time_zone_annotation: Option<TimeZoneNameOrOffset>,
    calendar: Option<CalendarId>,
    matched_goal: TemporalProduction,
}

impl ISO8601ParseTokens {
    /// O `ISO8601ParseTokens tokens;` do C++; `matchedGoal` começa com `{}` e todo tokenizador o atribui
    /// antes de devolver.
    fn new() -> ISO8601ParseTokens {
        ISO8601ParseTokens {
            year: None,
            month: None,
            day: None,
            time: None,
            has_utc_designator: false,
            utc_offset_ns: None,
            offset_has_sub_minute_precision: false,
            time_zone_annotation: None,
            calendar: None,
            matched_goal: TemporalProduction::Instant,
        }
    }

    /// Os campos de `Date` que `parseDate` achou.
    fn set_date(&mut self, date: PlainDate) {
        self.year = Some(date.year());
        self.month = Some(u32::from(date.month()));
        self.day = Some(u32::from(date.day()));
    }

    /// Um `DateTimeUTCOffset` com sinal: `parseUTCOffset` com `SubMinutePrecision::Yes`.
    fn parse_numeric_offset(&mut self, buffer: &mut ParsingBuffer) -> Option<()> {
        let mut sub_minute = false;
        let offset = parse_utc_offset_in_buffer(buffer, SubMinutePrecision::Yes, Some(&mut sub_minute))?;
        self.utc_offset_ns = Some(offset);
        self.offset_has_sub_minute_precision = sub_minute;
        Some(())
    }
}

/// `parseTrailingTokens(buffer, tokens, annotationRequired)`: `TimeZoneAnnotation? Annotations?`;
/// `annotation_required` impõe `[+Zoned]`.
fn parse_trailing_tokens(buffer: &mut ParsingBuffer, tokens: &mut ISO8601ParseTokens, annotation_required: bool) -> bool {
    if !buffer.at_end() && buffer.current() == u16::from(b'[') && can_be_time_zone(buffer, buffer.current()) {
        let Some(annotation) = parse_time_zone_annotation(buffer) else {
            return false;
        };
        tokens.time_zone_annotation = Some(annotation);
    } else if annotation_required {
        return false;
    }

    if !buffer.at_end() && can_be_rfc9557_annotation(buffer) {
        // Passo 4.a.ii.(1) e (2): o laço de anotações e as regras da marca crítica.
        let Some(calendars) = parse_calendar(buffer) else {
            return false;
        };
        if let Some(calendar) = calendars.into_iter().next() {
            tokens.calendar = Some(calendar);
        }
    }
    true
}

/// `tokenizeTemporalInstantString(buffer)`:
/// `Date DateTimeSeparator Time DateTimeUTCOffset[+Z] TimeZoneAnnotation? Annotations?`.
fn tokenize_temporal_instant_string(buffer: &mut ParsingBuffer) -> Option<ISO8601ParseTokens> {
    let mut tokens = ISO8601ParseTokens::new();

    let date = parse_date(buffer)?;
    tokens.set_date(date);

    // `DateTimeSeparator` é obrigatório.
    if buffer.at_end() || !matches!(u8::try_from(buffer.current()), Ok(b'T' | b't' | b' ')) {
        return None;
    }
    buffer.advance();

    tokens.time = Some(parse_time_spec(buffer, Second60Mode::Accept, SubMinutePrecision::Yes, None)?);

    // `DateTimeUTCOffset[+Z]` é obrigatório pela gramática: designador UTC ou deslocamento.
    if buffer.at_end() {
        return None;
    }
    if matches!(u8::try_from(buffer.current()), Ok(b'Z' | b'z')) {
        tokens.has_utc_designator = true;
        buffer.advance();
    } else if matches!(u8::try_from(buffer.current()), Ok(b'+' | b'-')) {
        tokens.parse_numeric_offset(buffer)?;
    } else {
        return None;
    }

    if !parse_trailing_tokens(buffer, &mut tokens, false) {
        return None;
    }
    tokens.matched_goal = TemporalProduction::Instant;
    Some(tokens)
}

/// `tokenizeTemporalDateTimeString(buffer, zoned, timeRequired)`: `AnnotatedDateTime[?Zoned, ?TimeRequired]`;
/// `time_required` proíbe a alternativa só com `Date`.
fn tokenize_temporal_date_time_string(buffer: &mut ParsingBuffer, zoned: bool, time_required: bool) -> Option<ISO8601ParseTokens> {
    let mut tokens = ISO8601ParseTokens::new();

    let date = parse_date(buffer)?;
    tokens.set_date(date);

    if !buffer.at_end() && matches!(u8::try_from(buffer.current()), Ok(b'T' | b't' | b' ')) {
        buffer.advance();
        tokens.time = Some(parse_time_spec(buffer, Second60Mode::Accept, SubMinutePrecision::Yes, None)?);
        if !buffer.at_end() {
            if matches!(u8::try_from(buffer.current()), Ok(b'Z' | b'z')) {
                // `[~Zoned]` proíbe o `Z`.
                if !zoned {
                    return None;
                }
                tokens.has_utc_designator = true;
                buffer.advance();
            } else if matches!(u8::try_from(buffer.current()), Ok(b'+' | b'-')) {
                tokens.parse_numeric_offset(buffer)?;
            }
        }
    } else if time_required {
        return None;
    }

    if !parse_trailing_tokens(buffer, &mut tokens, zoned) {
        return None;
    }
    tokens.matched_goal = if zoned { TemporalProduction::DateTimeZoned } else { TemporalProduction::DateTimeUnzoned };
    Some(tokens)
}

/// `tokenizeTemporalYearMonthString(buffer)`: `AnnotatedYearMonth | AnnotatedDateTime[~Zoned, ~TimeRequired]`.
fn tokenize_temporal_year_month_string(buffer: &mut ParsingBuffer) -> Option<ISO8601ParseTokens> {
    let restore_point = *buffer;
    if let Some(date) = parse_date_spec_year_month(buffer) {
        let mut tokens = ISO8601ParseTokens::new();
        tokens.year = Some(date.year());
        tokens.month = Some(u32::from(date.month()));
        // `day` fica vazio de propósito: `DateSpecYearMonth` não tem `DateDay`.
        if parse_trailing_tokens(buffer, &mut tokens, false) && buffer.at_end() {
            tokens.matched_goal = TemporalProduction::YearMonth;
            return Some(tokens);
        }
    }
    // Recua para `AnnotatedDateTime[~Zoned, ~TimeRequired]`.
    *buffer = restore_point;
    let mut tokens = tokenize_temporal_date_time_string(buffer, false, false)?;
    tokens.matched_goal = TemporalProduction::YearMonth;
    Some(tokens)
}

/// `tokenizeTemporalMonthDayString(buffer)`: `AnnotatedMonthDay | AnnotatedDateTime[~Zoned, ~TimeRequired]`.
fn tokenize_temporal_month_day_string(buffer: &mut ParsingBuffer) -> Option<ISO8601ParseTokens> {
    let restore_point = *buffer;
    if let Some(date) = parse_date_spec_month_day(buffer) {
        let mut tokens = ISO8601ParseTokens::new();
        // `year` fica vazio de propósito: `DateSpecMonthDay` não tem `DateYear`.
        tokens.month = Some(u32::from(date.month()));
        tokens.day = Some(u32::from(date.day()));
        if parse_trailing_tokens(buffer, &mut tokens, false) && buffer.at_end() {
            tokens.matched_goal = TemporalProduction::MonthDay;
            return Some(tokens);
        }
    }
    *buffer = restore_point;
    let mut tokens = tokenize_temporal_date_time_string(buffer, false, false)?;
    tokens.matched_goal = TemporalProduction::MonthDay;
    Some(tokens)
}

/// `tokenizeTemporalAnnotatedTime(buffer)`:
/// `TimeDesignator? Time DateTimeUTCOffset[~Z]? TimeZoneAnnotation? Annotations?`.
fn tokenize_temporal_annotated_time(buffer: &mut ParsingBuffer) -> Option<ISO8601ParseTokens> {
    if !buffer.at_end() && matches!(u8::try_from(buffer.current()), Ok(b'T' | b't')) {
        buffer.advance();
    }
    let time = parse_time_spec(buffer, Second60Mode::Accept, SubMinutePrecision::Yes, None)?;

    let mut tokens = ISO8601ParseTokens::new();
    tokens.time = Some(time);

    if !buffer.at_end() {
        // `DateTimeUTCOffset[~Z]` proíbe o `Z`.
        if matches!(u8::try_from(buffer.current()), Ok(b'Z' | b'z')) {
            return None;
        }
        if matches!(u8::try_from(buffer.current()), Ok(b'+' | b'-')) {
            tokens.parse_numeric_offset(buffer)?;
        }
    }
    if !parse_trailing_tokens(buffer, &mut tokens, false) {
        return None;
    }
    tokens.matched_goal = TemporalProduction::Time;
    Some(tokens)
}

/// `isAmbiguousAnnotatedTime(buffer)`: o erro antecipado de `AnnotatedTime` (um `Time` sem `TimeDesignator`
/// que também seria um `DateSpecMonthDay` ou `DateSpecYearMonth`).
/// https://tc39.es/proposal-temporal/#sec-temporal-iso8601grammar-static-semantics-early-errors
fn is_ambiguous_annotated_time(buffer: &ParsingBuffer) -> bool {
    if buffer.at_end() {
        return false;
    }

    // As regras valem só para a alternativa sem `TimeDesignator`.
    if matches!(u8::try_from(buffer.current()), Ok(b'T' | b't')) {
        return false;
    }

    // Passo 1: o trecho `Time DateTimeUTCOffset[~Z]` vai até o `[` da anotação de fuso.
    let length = buffer.length_remaining();
    let prefix_length = (0..length).take_while(|&index| buffer.at(index) != u16::from(b'[')).count();
    let prefix = &buffer.span()[..prefix_length];

    // Passo 2: `ParseText(prefix, DateSpecMonthDay)` é erro de sintaxe se casar.
    let mut month_day_buffer = ParsingBuffer::new(prefix);
    if parse_date_spec_month_day(&mut month_day_buffer).is_some() && month_day_buffer.at_end() {
        return true;
    }

    // Passo 3: o mesmo com `DateSpecYearMonth`.
    let mut year_month_buffer = ParsingBuffer::new(prefix);
    parse_date_spec_year_month(&mut year_month_buffer).is_some() && year_month_buffer.at_end()
}

/// O `tryGoal` de `ParseISODateTime`: roda o tokenizador na cadeia inteira (tem de consumi-la toda), a
/// menos que um objetivo anterior já tenha casado.
fn try_goal(string: &[u16], parse_result: &mut Option<ISO8601ParseTokens>, tokenizer: impl FnOnce(&mut ParsingBuffer) -> Option<ISO8601ParseTokens>) {
    if parse_result.is_some() {
        return;
    }
    let mut buffer = ParsingBuffer::new(string);
    *parse_result = tokenizer(&mut buffer).filter(|_| buffer.at_end());
}

/// `parseISODateTime(string, allowed)` (`ParseISODateTime`):
/// https://tc39.es/proposal-temporal/#sec-temporal-parseisodatetime
/// `None` é o `RangeError` (o chamador escolhe a mensagem do tipo). Os objetivos entram do mais específico
/// ao menos, para o `matched` refletir a produção mais estreita.
pub fn parse_iso_date_time(string: &[u16], allowed: TemporalProductionSet) -> Option<ParsedISODateTime> {
    // Passos 1 a 3: `parseResult` vazio; o calendário fica em `parseResult.calendar`.
    let mut parse_result: Option<ISO8601ParseTokens> = None;

    // Passo 4: cada objetivo, com as regras de anotação e de forma curta dos passos 4.a.ii.(1) a (4).
    if allowed.contains(TemporalProduction::Instant) {
        try_goal(string, &mut parse_result, tokenize_temporal_instant_string);
    }
    if allowed.contains(TemporalProduction::DateTimeZoned) {
        try_goal(string, &mut parse_result, |buffer| tokenize_temporal_date_time_string(buffer, true, false));
    }
    if allowed.contains(TemporalProduction::YearMonth) {
        try_goal(string, &mut parse_result, tokenize_temporal_year_month_string);
    }
    if allowed.contains(TemporalProduction::MonthDay) {
        try_goal(string, &mut parse_result, tokenize_temporal_month_day_string);
    }
    if allowed.contains(TemporalProduction::Time) && parse_result.is_none() {
        // `TemporalTimeString` é `AnnotatedTime | AnnotatedDateTime[~Zoned, +TimeRequired]`.
        try_goal(string, &mut parse_result, tokenize_temporal_annotated_time);
        if parse_result.is_some() {
            // `AnnotatedTime`: erros antecipados.
            if is_ambiguous_annotated_time(&ParsingBuffer::new(string)) {
                parse_result = None;
            }
        }
        if parse_result.is_none() {
            try_goal(string, &mut parse_result, |buffer| {
                let mut tokens = tokenize_temporal_date_time_string(buffer, false, true)?;
                tokens.matched_goal = TemporalProduction::Time;
                Some(tokens)
            });
        }
    }
    if allowed.contains(TemporalProduction::DateTimeUnzoned) {
        try_goal(string, &mut parse_result, |buffer| tokenize_temporal_date_time_string(buffer, false, false));
    }

    // Passos 4.a.ii.(3) e (4): `YearMonth` sem `DateDay` e `MonthDay` sem `DateYear` exigem calendário
    // `iso8601` ou nenhum; qualquer outro é `None`.
    let mut is_short_form = false;
    if let Some(result) = &parse_result {
        if (result.matched_goal == TemporalProduction::YearMonth && result.day.is_none())
            || (result.matched_goal == TemporalProduction::MonthDay && result.year.is_none())
        {
            is_short_form = true;
        }
        if is_short_form {
            if let Some(calendar) = &result.calendar {
                if !calendar.eq_ignore_ascii_case(b"iso8601") {
                    return None;
                }
            }
        }
    }

    // Passo 5: sem resultado, `RangeError`.
    let parse_result = parse_result?;

    // Passos 6 a 20: os valores já saem extraídos pelos tokenizadores (o `TimeSecond` 60 já virou 59 e a
    // fração já foi completada com zeros).
    let year_mv = parse_result.year.unwrap_or(0);
    let month_mv = parse_result.month.unwrap_or(1);
    let day_mv = parse_result.day.unwrap_or(1);

    // Passo 21: `parseDate` conferiu o dia contra o ano lido antes de `PlainDate` limitá-lo a
    // `OUT_OF_RANGE_YEAR`, que não é necessariamente bissexto.
    debug_assert!(year_mv == OUT_OF_RANGE_YEAR || is_valid_iso_date(f64::from(year_mv), f64::from(month_mv), f64::from(day_mv)));

    // Passos 24 a 27: o registro de fuso, quando há `Z`, deslocamento ou anotação.
    let any_time_zone_information = parse_result.has_utc_designator || parse_result.utc_offset_ns.is_some() || parse_result.time_zone_annotation.is_some();
    let time_zone = any_time_zone_information.then(|| ISOStringTimeZoneParseRecord {
        z: parse_result.has_utc_designator,
        offset: parse_result.utc_offset_ns,
        name_or_offset: parse_result.time_zone_annotation.clone().unwrap_or(TimeZoneNameOrOffset::Name(Vec::new())),
        offset_has_sub_minute_precision: parse_result.offset_has_sub_minute_precision,
    });

    // Passos 22, 23, 28 e 29: `time` ausente é `~start-of-day~`; `date` ausente só no objetivo `Time` sem mês.
    let date_bearing_goal = parse_result.matched_goal != TemporalProduction::Time || parse_result.month.is_some();
    let date = date_bearing_goal.then(|| PlainDate::new(i64::from(year_mv), month_mv, day_mv));

    Some(ParsedISODateTime {
        date,
        time: parse_result.time,
        time_zone,
        calendar: parse_result.calendar,
        matched: parse_result.matched_goal,
        is_short_form,
    })
}

/// `temporalTimeToString(plainTime, precision)`: https://tc39.es/proposal-temporal/#sec-temporal-timerecordtostring
/// `HH:MM` com `Precision::Minute`; senão `HH:MM:SS` e a fração (`auto` tira os zeros à direita, `Fixed` dá os
/// dígitos pedidos).
pub fn temporal_time_to_string(plain_time: PlainTime, precision: (crate::runtime::temporal_object::Precision, u32)) -> String {
    use crate::runtime::temporal_object::{format_seconds_string_fraction, Precision};

    let (precision_type, precision_value) = precision;
    debug_assert!(precision_type == Precision::Auto || precision_value < 10);
    let mut result = format!("{:02}:{:02}", plain_time.hour(), plain_time.minute());
    if precision_type == Precision::Minute {
        return result;
    }

    let fraction_nanoseconds = plain_time.millisecond() * 1_000_000 + plain_time.microsecond() * 1_000 + plain_time.nanosecond();
    result.push_str(&format!(":{:02}", plain_time.second()));
    format_seconds_string_fraction(&mut result, fraction_nanoseconds, precision);
    result
}

// ---------------------------------------------------------------------------------------------
// Auxiliares de data que `Temporal.PlainDate` usa (semana ISO, texto, código de mês, limites)
// ---------------------------------------------------------------------------------------------

/// `weekOfYear(PlainDate)`: o número da semana ISO (a semana 1 é a que tem a primeira quinta-feira do ano).
/// https://en.wikipedia.org/wiki/ISO_week_date#Algorithms
pub fn week_of_year(plain_date: PlainDate) -> u8 {
    let ordinal = i32::from(day_of_year(plain_date));
    let weekday = i32::from(day_of_week(plain_date));

    let week = (ordinal - weekday + 10) / 7;
    if week <= 0 {
        // A última semana do ano anterior, de 52 ou 53 semanas
        // (https://en.wikipedia.org/wiki/ISO_week_date#Weeks_per_year): ano que termina na quinta-feira (o
        // 1/1 deste ano é sexta) ou bissexto que termina na sexta (o 1/1 deste ano é sábado).
        let weekday_of_january_first = day_of_week(PlainDate::new(i64::from(plain_date.year()), 1, 1));
        if weekday_of_january_first == 5 {
            return 53;
        }
        if weekday_of_january_first == 6 && is_leap_year(plain_date.year() - 1) {
            return 53;
        }
        return 52;
    }

    if week == 53 {
        // A semana 53 que cai na semana 1 do ano seguinte.
        if (crate::wtf::date_math::days_in_year(plain_date.year()) - ordinal) < (4 - weekday) {
            return 1;
        }
    }

    week as u8
}

/// `yearOfWeek(PlainDate)`: o campo `[[Year]]` de `ISOWeekOfYear`, o ano do calendário de semanas.
/// https://tc39.es/proposal-temporal/#sec-temporal-isoweekofyear
pub fn year_of_week(plain_date: PlainDate) -> i32 {
    let ordinal = i32::from(day_of_year(plain_date));
    let weekday = i32::from(day_of_week(plain_date));

    let week = (ordinal - weekday + 10) / 7;
    if week < 1 {
        return plain_date.year() - 1;
    }

    if week == 53 && (crate::wtf::date_math::days_in_year(plain_date.year()) - ordinal) < (4 - weekday) {
        return plain_date.year() + 1;
    }

    plain_date.year()
}

/// `temporalDateToString(year, month)` (estática): `YYYY-MM`, ou `+YYYYYY-MM`/`-YYYYYY-MM` fora de 0 a 9999.
fn temporal_year_month_digits(year: i32, month: i32) -> String {
    // Se vai ser impresso, está dentro do limite.
    debug_assert!(is_year_within_limits(i64::from(year)));

    let (prefix, year_digits, year) = if !(0..=9999).contains(&year) {
        (if year < 0 { "-" } else { "+" }, 6, year.abs())
    } else {
        ("", 4, year)
    };
    format!("{prefix}{year:0year_digits$}-{month:02}")
}

/// `temporalDateToString(PlainDate)`: https://tc39.es/proposal-temporal/#sec-temporal-isodatetostring
pub fn temporal_date_to_string(plain_date: PlainDate) -> String {
    format!("{}-{:02}", temporal_year_month_digits(plain_date.year(), i32::from(plain_date.month())), plain_date.day())
}

/// `temporalDateTimeToString(plainDate, plainTime, precision)`: a data, `T` e a hora (sem anotação de calendário).
/// https://tc39.es/proposal-temporal/#sec-temporal-isodatetimetostring
pub fn temporal_date_time_to_string(
    plain_date: PlainDate,
    plain_time: PlainTime,
    precision: (crate::runtime::temporal_object::Precision, u32),
) -> String {
    format!("{}T{}", temporal_date_to_string(plain_date), temporal_time_to_string(plain_time, precision))
}

/// A anotação de calendário que `temporalYearMonthToString` e `temporalMonthDayToString` repartem: a data ISO
/// completa mais `[u-ca=...]` (`always` e `auto` com calendário não ISO) ou `[!u-ca=...]` (`critical`), e o
/// texto curto do `short_form` quando não há anotação (`never`, ou `auto` no calendário ISO).
fn temporal_date_with_calendar_annotation(
    plain_date: PlainDate,
    short_form: impl FnOnce() -> String,
    calendar_name: &str,
    calendar_id: u8,
) -> String {
    use crate::runtime::temporal_calendar::{calendar_id_to_string, calendar_is_iso};

    let is_non_iso = !calendar_is_iso(calendar_id);
    let cal_id = calendar_id_to_string(calendar_id);
    match calendar_name {
        "never" if is_non_iso => temporal_date_to_string(plain_date),
        "never" => short_form(),
        "always" => format!("{}[u-ca={cal_id}]", temporal_date_to_string(plain_date)),
        "critical" => format!("{}[!u-ca={cal_id}]", temporal_date_to_string(plain_date)),
        _ if is_non_iso => format!("{}[u-ca={cal_id}]", temporal_date_to_string(plain_date)),
        _ => short_form(),
    }
}

/// `temporalYearMonthToString(plainYearMonth, calendarName, calendarId)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-temporalyearmonthtostring
pub fn temporal_year_month_to_string(plain_year_month: PlainYearMonth, calendar_name: &str, calendar_id: u8) -> String {
    temporal_date_with_calendar_annotation(
        *plain_year_month.iso_plain_date(),
        || temporal_year_month_digits(plain_year_month.year(), i32::from(plain_year_month.month())),
        calendar_name,
        calendar_id,
    )
}

/// `temporalMonthDayToString(plainMonthDay, calendarName, calendarId)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-temporalmonthdaytostring
pub fn temporal_month_day_to_string(plain_month_day: PlainMonthDay, calendar_name: &str, calendar_id: u8) -> String {
    temporal_date_with_calendar_annotation(
        *plain_month_day.iso_plain_date(),
        || format!("{:02}-{:02}", plain_month_day.month(), plain_month_day.day()),
        calendar_name,
        calendar_id,
    )
}

/// `monthCode(month)`: `M01` a `M12`.
pub fn month_code(month: u32) -> String {
    format!("M{month:02}")
}

/// `parseMonthCode(StringView)`: a gramática de https://tc39.es/proposal-temporal/#sec-temporal-parsemonthcode
/// (`M00L`, `M0` `NonZeroDigit` `L?`, `M` `NonZeroDigit` `DecimalDigit` `L?`). `None` é o `RangeError`.
pub fn parse_month_code(month_code: &[u16]) -> Option<crate::runtime::temporal_object::ParsedMonthCode> {
    let is_digit = |unit: u16| (u16::from(b'0')..=u16::from(b'9')).contains(&unit);
    if month_code.len() < 3
        || month_code.len() > 4
        || month_code[0] != u16::from(b'M')
        || !is_digit(month_code[1])
        || !is_digit(month_code[2])
    {
        return None;
    }
    if month_code.len() == 4 && month_code[3] != u16::from(b'L') {
        return None;
    }
    // `M00` só vale como `M00L`: `M00` sozinho não casa nenhuma das três alternativas.
    if month_code[1] == u16::from(b'0') && month_code[2] == u16::from(b'0') && month_code.len() == 3 {
        return None;
    }

    let is_leap_month = month_code.len() == 4;
    let month_number = ((month_code[1] - u16::from(b'0')) * 10 + (month_code[2] - u16::from(b'0'))) as u8;
    Some(crate::runtime::temporal_object::ParsedMonthCode { month_number, is_leap_month })
}

/// `isDateTimeWithinLimits(year, month, day, hour, minute, second, millisecond, microsecond, nanosecond)`:
/// https://tc39.es/proposal-temporal/#sec-temporal-isodatetimewithinlimits
#[allow(clippy::too_many_arguments)]
pub fn is_date_time_within_limits(
    year: i32,
    month: u8,
    day: u8,
    hour: u32,
    minute: u32,
    second: u32,
    millisecond: u32,
    microsecond: u32,
    nanosecond: u32,
) -> bool {
    // Os instantes extremos que Temporal admite são -271821-04-20T00:00:00Z e +275760-09-13T00:00:00Z, e
    // `isDateTimeWithinLimits` dá uma folga de um dia a cada ponta: só os anos de borda têm combinações fora
    // da faixa; os anos estritamente de dentro sempre passam.
    if year > MIN_YEAR && year < MAX_YEAR {
        return true;
    }

    let nanoseconds =
        ExactTime::from_iso_parts_and_offset(year, month, day, hour, minute, second, millisecond, microsecond, nanosecond, 0)
            .epoch_nanoseconds();
    if nanoseconds <= ExactTime::MIN_VALUE - ExactTime::NS_PER_DAY {
        return false;
    }
    if nanoseconds >= ExactTime::MAX_VALUE + ExactTime::NS_PER_DAY {
        return false;
    }
    true
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn parse_time(text: &str) -> Option<ParsedISODateTime> {
        parse_iso_date_time(&units(text), OptionSet::new(&[TemporalProduction::Time]))
    }

    #[test]
    fn parses_time_strings() {
        let parsed = parse_time("14:30:15.123456789").unwrap();
        assert_eq!(parsed.time, Some(PlainTime::new(14, 30, 15, 123, 456, 789)));
        assert_eq!(parsed.date, None);
        assert_eq!(parse_time("T0930").unwrap().time, Some(PlainTime::new(9, 30, 0, 0, 0, 0)));
        assert_eq!(parse_time("23:59:60").unwrap().time, Some(PlainTime::new(23, 59, 59, 0, 0, 0)));
        assert_eq!(parse_time("2021-01-01T14:30").unwrap().time, Some(PlainTime::new(14, 30, 0, 0, 0, 0)));
        assert_eq!(parse_time("2021-01-01T14:30").unwrap().date, Some(PlainDate::new(2021, 1, 1)));
    }

    #[test]
    fn rejects_malformed_and_ambiguous_time_strings() {
        for text in ["", "24:00", "14:60", "14:30:15.", "2021-01-01", "0101", "2021-12", "14:30Z", "14:30:00 "] {
            assert_eq!(parse_time(text), None, "{text}");
        }
    }

    #[test]
    fn parses_annotations_and_offsets() {
        let parsed = parse_time("14:30+01:00[Europe/Paris][u-ca=iso8601]").unwrap();
        assert_eq!(parsed.calendar, Some(b"iso8601".to_vec()));
        let time_zone = parsed.time_zone.unwrap();
        assert_eq!(time_zone.offset, Some(3_600_000_000_000));
        assert_eq!(time_zone.name_or_offset, TimeZoneNameOrOffset::Name(b"Europe/Paris".to_vec()));
        assert_eq!(parse_time("14:30[u-ca=iso8601][!u-ca=gregory]"), None);
        assert_eq!(parse_time("14:30[!foo=bar]"), None);
    }

    #[test]
    fn parses_instants_and_utc_offsets() {
        let instant = parse_iso_date_time(&units("2020-01-01T00:00:00Z"), OptionSet::new(&[TemporalProduction::Instant])).unwrap();
        assert!(instant.time_zone.unwrap().z);
        assert_eq!(parse_utc_offset(&units("-08:30"), SubMinutePrecision::No), Some(-30_600_000_000_000));
        assert_eq!(parse_utc_offset(&units("+01:00:30"), SubMinutePrecision::No), None);
    }

    #[test]
    fn formats_times() {
        use crate::runtime::temporal_object::Precision;
        let time = PlainTime::new(5, 6, 7, 80, 0, 0);
        assert_eq!(temporal_time_to_string(time, (Precision::Auto, 0)), "05:06:07.08");
        assert_eq!(temporal_time_to_string(time, (Precision::Fixed, 5)), "05:06:07.08000");
        assert_eq!(temporal_time_to_string(time, (Precision::Fixed, 0)), "05:06:07");
        assert_eq!(temporal_time_to_string(time, (Precision::Minute, 0)), "05:06");
        assert_eq!(temporal_time_to_string(PlainTime::new(0, 0, 0, 0, 0, 0), (Precision::Fixed, 9)), "00:00:00.000000000");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn parses_a_full_duration() {
        let d = parse_duration(&units("-P1Y2M3W4DT5H6M7.123456789S")).unwrap();
        assert_eq!((d.years(), d.months(), d.weeks(), d.days()), (-1, -2, -3, -4));
        assert_eq!((d.hours(), d.minutes(), d.seconds()), (-5, -6, -7));
        assert_eq!((d.milliseconds(), d.microseconds(), d.nanoseconds()), (-123, -456, -789));
        assert!(is_valid_duration(&d));
    }

    #[test]
    fn parses_fractional_hours_and_lowercase() {
        let d = parse_duration(&units("pt1,5h")).unwrap();
        assert_eq!((d.hours(), d.minutes()), (1, 30));
    }

    #[test]
    fn rejects_malformed_durations() {
        for text in ["P", "PT", "P1Y1Y", "P1D2M", "PT1.5H2M", "P1", "1Y", "PT1.S", "PT1H ", "P1.5D"] {
            assert_eq!(parse_duration(&units(text)), None, "{text}");
        }
    }

    #[test]
    fn calendar_helpers() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(day_of_week(PlainDate::new(2024, 1, 1)), 1);
        assert_eq!(day_of_year(PlainDate::new(2024, 12, 31)), 366);
        assert!(!is_valid_iso_date(2023.0, 2.0, 29.0));
        assert_eq!(PlainDate::new(300000, 1, 1).year(), OUT_OF_RANGE_YEAR);
    }

    #[test]
    fn exact_time_range_and_double_cast() {
        assert!(ExactTime::new(ExactTime::MAX_VALUE).is_valid());
        assert!(!ExactTime::new(ExactTime::MAX_VALUE + 1).is_valid());
        assert_eq!(checked_cast_double_to_int128(-1e30), Some(-1_000_000_000_000_000_019_884_624_838_656));
        assert_eq!(checked_cast_double_to_int128(0.5), Some(0));
        assert_eq!(checked_cast_double_to_int128(1e40), None);
    }
}
