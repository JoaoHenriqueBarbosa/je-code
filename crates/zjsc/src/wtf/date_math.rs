//! Porte de `wtf/DateMath.h` e `DateMath.cpp`: a aritmética de calendário do ECMAScript (dias desde a
//! época, ano, mês, dia da semana), os parsers de data (`parseES5Date` e o `parseDate` de RFC 822/2822
//! com as extensões do JavaScript) e as tabelas de nomes.
//!
//! DIVERGÊNCIAS:
//!
//! - `calculateLocalTimeOffset`, `calculateUTCOffset`, `calculateDSTOffset`, `equivalentYearForDST`,
//!   `initializeDates`, `parseDate(span)` (a sobrecarga que consulta o `localtime_r`), `isTimeZoneValid`,
//!   `setTimeZoneOverride` e `getTimeZoneOverride` ficam fora: dependem de `libc`/ICU e só o WebCore
//!   os chama. O `JSDateMath` do JSC calcula o deslocamento local pelo `DateCache`
//!   (`runtime/js_date_math.rs`), que recebe o fuso por um `TimeZoneSource`.
//! - `makeRFC2822DateString` fica fora (só o WebCore usa).
//! - `jsCurrentTime()` lê o relógio do pseudo-processo (`ul_common::time::realtime_nanos`); o sandbox pode trocar a fonte do
//!   tempo no `JSGlobalObject::jsDateNow` (o `overridenDateNow` do Bun já a envolve).
//! - Os parsers trabalham sobre os bytes UTF-8 da string (o C++ passa o UTF-8 como se fosse Latin1) e
//!   usam `i64` onde o C++ usa `long` (LP64) e `i32` onde usa `int`.

/// `enum class TimeType : uint8_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeType {
    UTCTime = 0,
    LocalTime = 1,
}

/// `struct LocalTimeOffset`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalTimeOffset {
    pub is_dst: bool,
    /// Deslocamento combinado (UTC mais horário de verão), em milissegundos.
    pub offset: i32,
}

impl LocalTimeOffset {
    pub const fn new(is_dst: bool, offset: i32) -> LocalTimeOffset {
        LocalTimeOffset { is_dst, offset }
    }
}

/// `WTF::weekdayName`: começa na segunda-feira.
pub const WEEKDAY_NAME: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
/// `WTF::monthName`.
pub const MONTH_NAME: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
/// `WTF::monthFullName`.
pub const MONTH_FULL_NAME: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// `WTF::firstDayOfMonth`: o dia do ano do primeiro dia de cada mês, sem e com ano bissexto.
pub const FIRST_DAY_OF_MONTH: [[i32; 12]; 2] = [
    [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334],
    [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335],
];

/// `WTF::daysInMonths`.
pub const DAYS_IN_MONTHS: [i8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

pub const HOURS_PER_DAY: f64 = 24.0;
pub const MINUTES_PER_HOUR: f64 = 60.0;
pub const SECONDS_PER_MINUTE: f64 = 60.0;
pub const MS_PER_SECOND: f64 = 1000.0;
pub const MS_PER_MONTH: f64 = 2592000000.0;
pub const SECONDS_PER_HOUR: f64 = SECONDS_PER_MINUTE * MINUTES_PER_HOUR;
pub const SECONDS_PER_DAY: f64 = SECONDS_PER_HOUR * HOURS_PER_DAY;
pub const MS_PER_MINUTE: f64 = MS_PER_SECOND * SECONDS_PER_MINUTE;
pub const MS_PER_HOUR: f64 = MS_PER_SECOND * SECONDS_PER_HOUR;
pub const MS_PER_DAY: f64 = MS_PER_SECOND * SECONDS_PER_DAY;

/// `maxUnixTime`: 31/12/2037.
pub const MAX_UNIX_TIME: f64 = 2145859200.0;
/// `maxECMAScriptTime`: o ECMAScript não pede suporte além de 8.64E15 ms.
pub const MAX_ECMASCRIPT_TIME: f64 = 8.64E15;

/// `class Int64Milliseconds` (só as constantes; o valor é um `i64` solto).
pub mod int64_milliseconds {
    pub const HOURS_PER_DAY: i64 = 24;
    pub const MINUTES_PER_HOUR: i64 = 60;
    pub const SECONDS_PER_MINUTE: i64 = 60;
    pub const MS_PER_SECOND: i64 = 1000;
    pub const MS_PER_MONTH: i64 = 2592000000;
    pub const SECONDS_PER_HOUR: i64 = SECONDS_PER_MINUTE * MINUTES_PER_HOUR;
    pub const SECONDS_PER_DAY: i64 = SECONDS_PER_HOUR * HOURS_PER_DAY;
    pub const MS_PER_MINUTE: i64 = MS_PER_SECOND * SECONDS_PER_MINUTE;
    pub const MS_PER_HOUR: i64 = MS_PER_SECOND * SECONDS_PER_HOUR;
    pub const MS_PER_DAY: i64 = MS_PER_SECOND * SECONDS_PER_DAY;
    pub const MAX_ECMASCRIPT_TIME: i64 = 8_640_000_000_000_000;
    pub const MIN_ECMASCRIPT_TIME: i64 = -8_640_000_000_000_000;

    pub const DAYS_IN_4_YEARS: i64 = 4 * 365 + 1;
    pub const DAYS_IN_100_YEARS: i64 = 25 * DAYS_IN_4_YEARS - 1;
    pub const DAYS_IN_400_YEARS: i64 = 4 * DAYS_IN_100_YEARS + 1;
    pub const DAYS_1970_TO_2000: i64 = 30 * 365 + 7;
    pub const DAYS_OFFSET: i32 = (1000 * DAYS_IN_400_YEARS + 5 * DAYS_IN_400_YEARS - DAYS_1970_TO_2000) as i32;
    pub const YEARS_OFFSET: i32 = 400000;
}

use int64_milliseconds as i64ms;
use std::cell::Cell;

/// `jsCurrentTime()`: o instante atual em milissegundos inteiros desde a época, no relógio do pseudo-processo.
pub fn js_current_time() -> f64 {
    (current_time_in_nanoseconds() as f64 / 1_000_000.0).floor()
}

/// `WTF::currentTimeInNanoseconds()`: o instante atual em nanossegundos desde a época, do mesmo relógio de
/// `js_current_time`, sem o arredondamento para milissegundos. Com o relógio virtual ligado (o navegador sob
/// `--virtual-time-budget`), é a origem mais o tempo virtual do laço de eventos.
pub fn current_time_in_nanoseconds() -> i128 {
    match VIRTUAL_ORIGIN_NS.with(Cell::get) {
        Some(origin) => origin + (crate::runtime::waiter_list_manager::virtual_now() * 1_000_000.0) as i128,
        None => ul_common::time::realtime_nanos(),
    }
}

thread_local! {
    /// A época, em nanossegundos, do instante 0 do relógio virtual; `None` é o relógio do sistema.
    static VIRTUAL_ORIGIN_NS: Cell<Option<i128>> = const { Cell::new(None) };
}

/// Liga o relógio virtual do programa: `Date` e o `performance` passam a andar com o tempo virtual dos timers, a
/// partir de `origin_ms` (a época do início da página).
pub fn set_virtual_clock(origin_ms: f64) {
    VIRTUAL_ORIGIN_NS.with(|origin| origin.set(Some((origin_ms * 1_000_000.0) as i128)));
}

/// A origem do relógio virtual, em nanossegundos desde a época, quando ligado.
pub fn virtual_clock_origin_ns() -> Option<i128> {
    VIRTUAL_ORIGIN_NS.with(Cell::get)
}

/// Fim do programa: volta ao relógio do sistema.
pub(crate) fn reset_for_program() {
    let _ = VIRTUAL_ORIGIN_NS.try_with(|origin| origin.set(None));
}

/// `timeClip(t)`.
pub fn time_clip(t: f64) -> f64 {
    if t.abs() > MAX_ECMASCRIPT_TIME {
        return f64::NAN;
    }
    t.trunc() + 0.0
}

/// `daysFrom1970ToYear(year)`.
pub fn days_from_1970_to_year(year: i32) -> f64 {
    const LEAP_DAYS_BEFORE_1971_BY_4_RULE: i32 = 1970 / 4;
    const EXCLUDED_LEAP_DAYS_BEFORE_1971_BY_100_RULE: i32 = 1970 / 100;
    const LEAP_DAYS_BEFORE_1971_BY_400_RULE: i32 = 1970 / 400;

    let year_minus_one = year as f64 - 1.0;
    let years_to_add_by_4_rule = (year_minus_one / 4.0).floor() - LEAP_DAYS_BEFORE_1971_BY_4_RULE as f64;
    let years_to_exclude_by_100_rule = (year_minus_one / 100.0).floor() - EXCLUDED_LEAP_DAYS_BEFORE_1971_BY_100_RULE as f64;
    let years_to_add_by_400_rule = (year_minus_one / 400.0).floor() - LEAP_DAYS_BEFORE_1971_BY_400_RULE as f64;

    365.0 * (year as f64 - 1970.0) + years_to_add_by_4_rule - years_to_exclude_by_100_rule + years_to_add_by_400_rule
}

/// `isLeapYear(year)`.
pub fn is_leap_year(year: i32) -> bool {
    if year % 4 != 0 {
        return false;
    }
    if year % 400 == 0 {
        return true;
    }
    if year % 100 == 0 {
        return false;
    }
    true
}

/// `daysInYear(year)`.
pub fn days_in_year(year: i32) -> i32 {
    365 + is_leap_year(year) as i32
}

/// `msToDays(double)`.
pub fn ms_to_days_f64(ms: f64) -> f64 {
    (ms / MS_PER_DAY).floor()
}

/// `msToDays(Int64Milliseconds)`.
pub fn ms_to_days(ms: i64) -> i32 {
    let mut time = ms;
    if time < 0 {
        // O C++ subtrai em `int64_t` e, no x86, `static_cast<int64_t>` de um double fora da faixa já
        // entrega `INT64_MIN`: a conta dá a volta em vez de estourar.
        time = time.wrapping_sub(i64ms::MS_PER_DAY - 1);
    }
    (time / i64ms::MS_PER_DAY) as i32
}

/// `timeInDay(ms, days)`.
pub fn time_in_day(ms: i64, days: i32) -> i32 {
    ms.wrapping_sub((days as i64) * i64ms::MS_PER_DAY) as i32
}

/// `yearMonthDayFromDays(passedDays)`: `(ano, mês 0 a 11, dia 1 a 31)`.
pub fn year_month_day_from_days(passed_days: i32) -> (i32, i32, i32) {
    let mut days = passed_days;
    days = days.wrapping_add(i64ms::DAYS_OFFSET);
    let mut year = (400 * (days / i64ms::DAYS_IN_400_YEARS as i32)).wrapping_sub(i64ms::YEARS_OFFSET);
    days %= i64ms::DAYS_IN_400_YEARS as i32;

    days -= 1;
    let yd1 = days / i64ms::DAYS_IN_100_YEARS as i32;
    days %= i64ms::DAYS_IN_100_YEARS as i32;
    year += 100 * yd1;

    days += 1;
    let yd2 = days / i64ms::DAYS_IN_4_YEARS as i32;
    days %= i64ms::DAYS_IN_4_YEARS as i32;
    year += 4 * yd2;

    days -= 1;
    let yd3 = days / 365;
    days %= 365;
    year += yd3;

    let is_leap = (yd1 == 0 || yd2 != 0) && yd3 == 0;

    days += is_leap as i32;

    // Check if the date is after February.
    let mut month = 0;
    let mut day = 0;
    let feb_end = 31 + 28 + if is_leap { 1 } else { 0 };
    if days >= feb_end {
        days -= feb_end;
        // Find the date starting from March.
        for i in 2..12 {
            if days < DAYS_IN_MONTHS[i] as i32 {
                month = i as i32;
                day = days + 1;
                break;
            }
            days -= DAYS_IN_MONTHS[i] as i32;
        }
    } else if days < 31 {
        // Check January and February.
        month = 0;
        day = days + 1;
    } else {
        month = 1;
        day = days - 31 + 1;
    }

    (year, month, day)
}

/// `daysFromYearMonth(year, month)`.
pub fn days_from_year_month(year: i32, month: i32) -> i32 {
    let mut year = year.wrapping_add(month / 12);
    let mut month = month % 12;
    if month < 0 {
        year = year.wrapping_sub(1);
        month += 12;
    }

    debug_assert!((0..12).contains(&month));

    // yearDelta is an arbitrary number such that:
    // a) yearDelta = -1 (mod 400)
    // b) year + yearDelta > 0 for years in the range defined by ECMA 262 - 15.9.1.1, i.e. upto
    //    100,000,000 days on either side of Jan 1 1970. This is required so that we don't run into
    //    integer division of negative numbers.
    // c) there shouldn't be an overflow for 32-bit integers in the following operations.
    const YEAR_DELTA: i32 = 399999;
    const BASE_DAY: i32 = 365 * (1970 + YEAR_DELTA) + (1970 + YEAR_DELTA) / 4 - (1970 + YEAR_DELTA) / 100 + (1970 + YEAR_DELTA) / 400;

    let year1 = year.wrapping_add(YEAR_DELTA);
    let day_from_year = 365i32
        .wrapping_mul(year1)
        .wrapping_add(year1 / 4)
        .wrapping_sub(year1 / 100)
        .wrapping_add(year1 / 400)
        .wrapping_sub(BASE_DAY);

    if (year % 4 != 0) || (year % 100 == 0 && year % 400 != 0) {
        return day_from_year.wrapping_add(FIRST_DAY_OF_MONTH[0][month as usize]);
    }
    day_from_year.wrapping_add(FIRST_DAY_OF_MONTH[1][month as usize])
}

/// `dayInYear(year, month, day)`.
pub fn day_in_year(year: i32, month: i32, day: i32) -> i32 {
    FIRST_DAY_OF_MONTH[is_leap_year(year) as usize][month as usize].wrapping_add(day).wrapping_sub(1)
}

/// `dayInYear(ms, year)`.
pub fn day_in_year_of_ms(ms: f64, year: i32) -> i32 {
    let result = ms_to_days_f64(ms) - days_from_1970_to_year(year);
    if result.is_nan() { 0 } else { result as i32 }
}

/// `dateToDaysFrom1970(year, month, day)`: os dias de 1970-01-01 até a data.
pub fn date_to_days_from_1970(year: i32, month: i32, day: i32) -> f64 {
    let mut year = year.wrapping_add(month / 12);

    let mut month = month % 12;
    if month < 0 {
        month += 12;
        year = year.wrapping_sub(1);
    }

    let yearday = days_from_1970_to_year(year).floor();
    debug_assert!((year >= 1970 && yearday >= 0.0) || (year < 1970 && yearday < 0.0));
    yearday + day_in_year(year, month, day) as f64
}

/// `msToYear(ms)`.
pub fn ms_to_year(ms: f64) -> i32 {
    let mut ms_as_years = (ms / (MS_PER_DAY * 365.2425)).floor();
    if ms_as_years.is_nan() {
        ms_as_years = 0.0;
    }
    let approx_year = (ms_as_years + 1970.0) as i32;
    let ms_from_approx_year_to_1970 = MS_PER_DAY * days_from_1970_to_year(approx_year);
    if ms_from_approx_year_to_1970 > ms {
        return approx_year - 1;
    }
    if ms_from_approx_year_to_1970 + MS_PER_DAY * days_in_year(approx_year) as f64 <= ms {
        return approx_year + 1;
    }
    approx_year
}

/// `fmod` do C seguido do ajuste para o intervalo `[0, modulus)`.
fn positive_fmod(value: f64, modulus: f64) -> i32 {
    let mut result = value % modulus;
    if result < 0.0 {
        result += modulus;
    }
    result as i32
}

/// `msToMinutes(ms)`.
pub fn ms_to_minutes(ms: f64) -> i32 {
    positive_fmod((ms / MS_PER_MINUTE).floor(), MINUTES_PER_HOUR)
}

/// `msToHours(ms)`.
pub fn ms_to_hours(ms: f64) -> i32 {
    positive_fmod((ms / MS_PER_HOUR).floor(), HOURS_PER_DAY)
}

/// `msToSeconds(ms)`.
pub fn ms_to_seconds(ms: f64) -> i32 {
    positive_fmod((ms / MS_PER_SECOND).floor(), SECONDS_PER_MINUTE)
}

/// `weekDay(days)`: 0 é domingo. O (dias + 4) mod 7 sem desvio, na faixa inteira do `int32_t`
/// (Ben Joffe, "A faster way to calculate the day-of-the-week").
pub fn week_day(days: i32) -> i32 {
    const MULTIPLIER: u64 = 0x2492492493000000;
    const THURSDAY_OFFSET: u64 = 0x9400u64 << 48;
    ((days as i64 as u64).wrapping_mul(MULTIPLIER).wrapping_add(THURSDAY_OFFSET) >> 61) as i32
}

/// `monthFromDayInYear(dayInYear, leapYear)`.
pub fn month_from_day_in_year(day_in_year: i32, leap_year: bool) -> i32 {
    let d = day_in_year;
    let mut step = 31;
    if d < step {
        return 0;
    }
    step += if leap_year { 29 } else { 28 };
    if d < step {
        return 1;
    }
    for (month, days) in [(2, 31), (3, 30), (4, 31), (5, 30), (6, 31), (7, 31), (8, 30), (9, 31)] {
        step += days;
        if d < step {
            return month;
        }
    }
    if d < step + 30 {
        return 10;
    }
    11
}

/// `dayInMonthFromDayInYear(dayInYear, leapYear)`.
pub fn day_in_month_from_day_in_year(day_in_year: i32, leap_year: bool) -> i32 {
    let d = day_in_year;
    let mut step = 0;
    let mut next = 30;

    if d <= next {
        return d + 1;
    }
    let days_in_feb = if leap_year { 29 } else { 28 };
    for days_in_this_month in [days_in_feb, 31, 30, 31, 30, 31, 31, 30, 31, 30] {
        // `checkMonth`: avança o mês e vê se `d` cai nele.
        step = next;
        next += days_in_this_month;
        if d <= next {
            return d - step;
        }
    }
    step = next;
    d - step
}

/// `timeToMS(hour, min, sec, ms)`.
pub fn time_to_ms(hour: f64, min: f64, sec: f64, ms: f64) -> f64 {
    ((hour * MINUTES_PER_HOUR + min) * SECONDS_PER_MINUTE + sec) * MS_PER_SECOND + ms
}

/// `equivalentYear(year)`: um ano em `[2008, 2035]` com a mesma bissextilidade e o mesmo dia da semana
/// do primeiro dia (ECMA 262 15.9.1.9).
pub fn equivalent_year(year: i32) -> i32 {
    let week_day = week_day(days_from_year_month(year, 0));
    let recent_year = (if is_leap_year(year) { 1956 } else { 1967 }) + (week_day * 12) % 28;
    // Find the year in the range 2008..2037 that is equivalent mod 28.
    // Add 3*28 to give a positive argument to the modulus operator.
    2008 + (recent_year + 3 * 28 - 2008) % 28
}

/// `equivalentTime(ms)`: o instante equivalente (ECMA 262 15.9.1.9) para as chamadas de biblioteca que
/// não aceitam datas fora de um inteiro de 32 bits não negativo em segundos.
pub fn equivalent_time(ms: i64) -> i64 {
    let days = ms_to_days(ms);
    let time_within_day_ms = ms.wrapping_sub(days as i64 * i64ms::MS_PER_DAY) as i32;
    let (year, month, day) = year_month_day_from_days(days);
    let new_days = days_from_year_month(equivalent_year(year), month) + day - 1;
    new_days as i64 * i64ms::MS_PER_DAY + time_within_day_ms as i64
}

// ---------------------------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------------------------

fn ymdhmsto_milliseconds(year: i32, mon: i64, day: i64, hour: i64, minute: i64, second: i64, milliseconds: f64) -> f64 {
    let mday = FIRST_DAY_OF_MONTH[is_leap_year(year) as usize][(mon - 1) as usize] as i64;
    let ydays = days_from_1970_to_year(year);

    let date_milliseconds = milliseconds
        + second as f64 * MS_PER_SECOND
        + minute as f64 * (SECONDS_PER_MINUTE * MS_PER_SECOND)
        + hour as f64 * (SECONDS_PER_HOUR * MS_PER_SECOND)
        + ((mday + day - 1) as f64 + ydays) * (SECONDS_PER_DAY * MS_PER_SECOND);

    // Clamp to EcmaScript standard (ecma262/#sec-time-values-and-time-range) of
    //  +/- 100,000,000 days from 01 January, 1970.
    if !(-8640000000000000.0..=8640000000000000.0).contains(&date_milliseconds) {
        return f64::NAN;
    }

    date_milliseconds
}

/// Zonas conhecidas: o RFC 2822 manda tratar as obsoletas que não estão aqui como "-0000".
const KNOWN_ZONES: [(&str, i32); 10] = [
    ("ut", 0),
    ("gmt", 0),
    ("est", -300),
    ("edt", -240),
    ("cst", -360),
    ("cdt", -300),
    ("mst", -420),
    ("mdt", -360),
    ("pst", -480),
    ("pdt", -420),
];

use crate::wtf::ascii_ctype::is_unicode_compatible_ascii_whitespace as is_ws;

fn is_digit(ch: u8) -> bool {
    ch.is_ascii_digit()
}

fn skip_spaces_and_comments(s: &mut &[u8]) {
    let mut nesting = 0;
    while let Some(&ch) = s.first() {
        if !is_ws(ch) {
            if ch == b'(' {
                nesting += 1;
            } else if ch == b')' && nesting > 0 {
                nesting -= 1;
            } else if nesting == 0 {
                break;
            }
        }
        *s = &s[1..];
    }
}

/// `skipExactly(span, ch)`.
fn skip_exactly(s: &mut &[u8], ch: u8) -> bool {
    if s.first() == Some(&ch) {
        *s = &s[1..];
        return true;
    }
    false
}

/// `skipLettersExactlyIgnoringASCIICase(span, letters)`: o prefixo igual, sem distinguir caixa.
fn skip_letters_ignoring_case(s: &mut &[u8], letters: &str) -> bool {
    let letters = letters.as_bytes();
    if s.len() >= letters.len() && s[..letters.len()].eq_ignore_ascii_case(letters) {
        *s = &s[letters.len()..];
        return true;
    }
    false
}

/// `findMonth(monthStr)`: 0 a 11, ou -1.
fn find_month(month_str: &[u8]) -> i64 {
    if month_str.len() < 3 {
        return -1;
    }
    let needle = [month_str[0].to_ascii_lowercase(), month_str[1].to_ascii_lowercase(), month_str[2].to_ascii_lowercase()];
    let haystack = b"janfebmaraprmayjunjulaugsepoctnovdec";
    if let Some(index) = haystack.windows(3).position(|window| window == needle) {
        if index % 3 == 0 {
            return (index / 3) as i64;
        }
    }
    -1
}

/// `safeStringToInteger`: o `strtol` que o C++ emula com `std::from_chars` (pula espaços e um `+`
/// iniciais, aceita um `-`, base 10). Só avança `s` no sucesso.
fn safe_string_to_integer(s: &mut &[u8], validate: impl Fn(i64) -> bool) -> Option<i64> {
    let mut rest = *s;
    while let Some(&ch) = rest.first() {
        if !is_ws(ch) {
            break;
        }
        rest = &rest[1..];
    }
    if rest.first() == Some(&b'+') {
        rest = &rest[1..];
    }
    let negative = rest.first() == Some(&b'-');
    let digits_start = if negative { &rest[1..] } else { rest };
    let digit_count = digits_start.iter().take_while(|ch| is_digit(**ch)).count();
    if digit_count == 0 {
        return None;
    }
    let mut magnitude: i128 = 0;
    for &ch in &digits_start[..digit_count] {
        magnitude = magnitude * 10 + (ch - b'0') as i128;
        if magnitude > (1i128 << 64) {
            return None;
        }
    }
    let value = if negative { -magnitude } else { magnitude };
    if value < i64::MIN as i128 || value > i64::MAX as i128 {
        return None;
    }
    let value = value as i64;
    if !validate(value) {
        return None;
    }
    *s = &digits_start[digit_count..];
    Some(value)
}

fn parse_int(s: &mut &[u8]) -> Option<i32> {
    safe_string_to_integer(s, |value| value > i32::MIN as i64 && value < i32::MAX as i64).map(|value| value as i32)
}

fn parse_long(s: &mut &[u8]) -> Option<i64> {
    safe_string_to_integer(s, |value| value != i64::MIN && value != i64::MAX)
}

/// O que `parseES5DatePortion` e `parseES5TimePortion` consumiram, `postParsePosition - currentPosition`.
fn consumed(before: &[u8], after: &[u8]) -> usize {
    before.len() - after.len()
}

/// Parses a date with the format YYYY[-MM[-DD]]. Year parsing is lenient, allows any number of
/// digits, and +/-.
fn parse_es5_date_portion(current: &mut &[u8], year: &mut i32, month: &mut i64, day: &mut i64, is_single_digit: &mut bool) -> bool {
    // This is a bit more lenient on the year string than ES5 specifies: instead of restricting to 4
    // digits (or 6 digits with mandatory +/-), it accepts any integer value.
    let has_negative_year = current.first() == Some(&b'-');
    match parse_int(current) {
        Some(value) => *year = value,
        None => return false,
    }
    if *year == 0 && has_negative_year {
        return false;
    }

    // Check for presence of -MM portion.
    if !skip_exactly(current, b'-') {
        return true;
    }

    if current.first().is_none_or(|ch| !is_digit(*ch)) {
        return false;
    }
    let mut post = *current;
    match parse_long(&mut post) {
        Some(value) => *month = value,
        None => return false,
    }
    match consumed(current, post) {
        1 => *is_single_digit = true,
        2 => {}
        _ => return false,
    }
    *current = post;

    // Check for presence of -DD portion.
    if !skip_exactly(current, b'-') {
        return true;
    }

    if current.first().is_none_or(|ch| !is_digit(*ch)) {
        return false;
    }
    post = *current;
    match parse_long(&mut post) {
        Some(value) => *day = value,
        None => return false,
    }
    match consumed(current, post) {
        1 => *is_single_digit = true,
        2 => {}
        _ => return false,
    }
    *current = post;
    true
}

/// O resultado de `parseES5TimePortion`.
struct Es5Time {
    hours: i64,
    minutes: i64,
    seconds: i64,
    milliseconds: f64,
    is_local_time: bool,
    time_zone_seconds: i64,
}

/// Parses a time with the format HH:mm[:ss[.sss]][Z|(+|-)(00:00|0000|00)]. Fractional seconds
/// parsing is lenient, allows any number of digits.
fn parse_es5_time_portion(current: &mut &[u8], has_t_symbol: bool) -> Option<Es5Time> {
    let mut time = Es5Time { hours: 0, minutes: 0, seconds: 0, milliseconds: 0.0, is_local_time: false, time_zone_seconds: 0 };

    if current.first().is_none_or(|ch| !is_digit(*ch)) {
        return None;
    }

    let mut post = *current;
    time.hours = parse_long(&mut post)?;
    if post.first() != Some(&b':') || (has_t_symbol && consumed(current, post) != 2) {
        return None;
    }
    *current = &post[1..];

    if current.first().is_none_or(|ch| !is_digit(*ch)) {
        return None;
    }
    post = *current;
    time.minutes = parse_long(&mut post)?;
    if has_t_symbol && consumed(current, post) != 2 {
        return None;
    }
    *current = post;

    // Seconds are optional.
    if skip_exactly(current, b':') {
        if current.first().is_none_or(|ch| !is_digit(*ch)) {
            return None;
        }
        post = *current;
        time.seconds = parse_long(&mut post)?;
        if has_t_symbol && consumed(current, post) != 2 {
            return None;
        }
        if post.first() == Some(&b'.') {
            *current = &post[1..];

            // In ECMA-262-5 it's a bit unclear if '.' can be present without milliseconds, but a
            // reasonable interpretation guided by the given examples and RFC 3339 says "no". We
            // check the next character to avoid reading +/- timezone hours after an invalid decimal.
            if current.first().is_none_or(|ch| !is_digit(*ch)) {
                return None;
            }

            // We are more lenient than ES5 by accepting more or less than 3 fraction digits.
            post = *current;
            let frac_seconds = parse_long(&mut post)?;

            let num_frac_digits = consumed(current, post) as i64;
            time.milliseconds = frac_seconds as f64 * 10f64.powf((-num_frac_digits + 3) as f64);
        }
        *current = post;
    }

    if skip_exactly(current, b'Z') {
        return Some(time);
    }

    // Parse (+|-)(00:00|0000|00).
    let tz_negative;
    if skip_exactly(current, b'-') {
        tz_negative = true;
    } else if skip_exactly(current, b'+') {
        tz_negative = false;
    } else {
        time.is_local_time = true;
        return Some(time);
    }

    let tz_hours_abs;
    let mut tz_minutes = 0;

    if current.first().is_none_or(|ch| !is_digit(*ch)) {
        return None;
    }
    post = *current;
    let tz_hours = parse_long(&mut post)?;
    if post.first() != Some(&b':') {
        if !has_t_symbol && consumed(current, post) == 2 {
            // "00" case.
            tz_hours_abs = tz_hours.abs();
        } else if consumed(current, post) == 4 {
            // "0000" case.
            let abs = tz_hours.abs();
            tz_minutes = abs % 100;
            tz_hours_abs = abs / 100;
        } else {
            return None;
        }
    } else {
        // "00:00" case.
        if has_t_symbol && consumed(current, post) != 2 {
            return None;
        }
        tz_hours_abs = tz_hours.abs();
        *current = &post[1..]; // Skip ":".

        if current.first().is_none_or(|ch| !is_digit(*ch)) {
            return None;
        }
        post = *current;
        tz_minutes = parse_long(&mut post)?;
        if has_t_symbol && consumed(current, post) != 2 {
            return None;
        }
    }
    *current = post;

    if tz_hours_abs > 23 {
        return None;
    }
    if !(0..=59).contains(&tz_minutes) {
        return None;
    }

    time.time_zone_seconds = 60 * (tz_minutes + (60 * tz_hours_abs));
    if tz_negative {
        time.time_zone_seconds = -time.time_zone_seconds;
    }

    Some(time)
}

/// `parseES5Date(dateString, isLocalTime)`: o formato de ecma262/#sec-date-time-string-format
/// (`YYYY-MM-DDTHH:mm:ss[.sss]Z`), quase sempre estrito. Devolve o valor e `isLocalTime`.
pub fn parse_es5_date(date_string: &[u8]) -> (f64, bool) {
    let nan = f64::NAN;
    let mut date_string = date_string;
    let mut is_local_time = false;

    const DAYS_PER_MONTH: [i64; 12] = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

    // The year must be present, but the other fields may be omitted - see ES5.1 15.9.1.15.
    let mut year = 0;
    let mut month = 1;
    let mut day = 1;
    let mut time = Es5Time { hours: 0, minutes: 0, seconds: 0, milliseconds: 0.0, is_local_time: false, time_zone_seconds: 0 };
    let mut is_single_digit = false;

    // Parse the date YYYY[-MM[-DD]]
    if !parse_es5_date_portion(&mut date_string, &mut year, &mut month, &mut day, &mut is_single_digit) {
        return (nan, false);
    }
    // Look for a time portion.
    // Note: As of ES2016, when a UTC offset is missing, date-time forms are local time while
    // date-only forms are UTC.
    if let Some(&first) = date_string.first() {
        if first == b'T' || first == b't' || first == b' ' {
            let has_t_symbol = first == b'T' || first == b't';
            date_string = &date_string[1..];

            // when dataString does not follow ISO8601 format, return NaN
            if is_single_digit && has_t_symbol {
                return (nan, false);
            }

            // Parse the time HH:mm[:ss[.sss]][Z|(+|-)(00:00|0000|00)]
            match parse_es5_time_portion(&mut date_string, has_t_symbol) {
                Some(parsed) => time = parsed,
                None => return (nan, false),
            }
            is_local_time = time.is_local_time;
        }
    }
    // Check that we have parsed all characters in the string.
    if !date_string.is_empty() {
        return (nan, false);
    }

    if is_single_digit {
        is_local_time = true;
    }

    // A few of these checks could be done inline above, but since many of them are interrelated we
    // would be sacrificing readability to "optimize" the (presumably less common) failure path.
    if !(1..=12).contains(&month) {
        return (nan, is_local_time);
    }
    if day < 1 || day > DAYS_PER_MONTH[(month - 1) as usize] {
        return (nan, is_local_time);
    }
    if month == 2 && day > 28 && !is_leap_year(year) {
        return (nan, is_local_time);
    }
    if time.hours < 0 || time.hours > 24 {
        return (nan, is_local_time);
    }
    if time.hours == 24 && (time.minutes != 0 || time.seconds != 0) {
        return (nan, is_local_time);
    }
    if time.minutes < 0 || time.minutes > 59 {
        return (nan, is_local_time);
    }
    if time.seconds < 0 || time.seconds >= 61 {
        return (nan, is_local_time);
    }
    if time.seconds == 60 {
        // Discard leap seconds by clamping to the end of a minute.
        time.milliseconds = 0.0;
    }

    let value = ymdhmsto_milliseconds(year, month, day, time.hours, time.minutes, time.seconds, time.milliseconds)
        - (time.time_zone_seconds as f64 * MS_PER_SECOND);
    (value, is_local_time)
}

/// `parseDate(dateString, isLocalTime)`: o RFC 822/2822 com as extensões do JavaScript. Devolve o
/// valor e `isLocalTime` (verdadeiro quando a data não trazia fuso).
pub fn parse_date(date_string: &[u8]) -> (f64, bool) {
    let nan = f64::NAN;
    let mut is_local_time = true;
    let mut offset: i32 = 0;
    let mut date_string = date_string;

    // This parses a date in the form:
    //     Tuesday, 09-Nov-99 23:12:40 GMT
    // or
    //     Sat, 01-Jan-2000 08:00:00 GMT
    // or
    //     Sat, 01 Jan 2000 08:00:00 GMT
    // or
    //     01 Jan 99 22:00 +0100    (exceptions in rfc822/rfc2822)
    // ### non RFC formats, added for Javascript:
    //     [Wednesday] January 09 1999 23:12:40 GMT
    //     [Wednesday] January 09 23:12:40 GMT 1999
    //
    // We ignore the weekday.

    // Skip leading space
    skip_spaces_and_comments(&mut date_string);

    let mut month: i64 = -1;
    let mut word_start = date_string;
    // Check contents of first words if not number
    while let Some(&front) = date_string.first() {
        if is_digit(front) {
            break;
        }
        if is_ws(front) || front == b'(' {
            if consumed(word_start, date_string) >= 3 {
                month = find_month(word_start);
            }
            skip_spaces_and_comments(&mut date_string);
            word_start = date_string;
        } else {
            date_string = &date_string[1..];
        }
    }

    // Missing delimiter between month and day (like "January29")?
    if month == -1 && word_start.len() != date_string.len() {
        month = find_month(word_start);
    }

    skip_spaces_and_comments(&mut date_string);

    if date_string.is_empty() {
        return (nan, is_local_time);
    }

    // ' 09-Nov-99 23:12:40 GMT'
    let mut day = match parse_long(&mut date_string) {
        Some(value) => value,
        None => return (nan, is_local_time),
    };

    if day < 0 {
        return (nan, is_local_time);
    }

    let mut year: Option<i32> = None;
    if day > 31 {
        // ### where is the boundary and what happens below?
        if !skip_exactly(&mut date_string, b'/') {
            return (nan, is_local_time);
        }
        // looks like a YYYY/MM/DD date
        if date_string.is_empty() {
            return (nan, is_local_time);
        }
        if day <= i32::MIN as i64 || day >= i32::MAX as i64 {
            return (nan, is_local_time);
        }
        year = Some(day as i32);
        match parse_long(&mut date_string) {
            Some(value) => month = value,
            None => return (nan, is_local_time),
        }
        month -= 1;
        if !skip_exactly(&mut date_string, b'/') {
            return (nan, is_local_time);
        }
        if date_string.is_empty() {
            return (nan, is_local_time);
        }
        match parse_long(&mut date_string) {
            Some(value) => day = value,
            None => return (nan, is_local_time),
        }
    } else if date_string.first() == Some(&b'/') && month == -1 {
        date_string = &date_string[1..];
        // This looks like a MM/DD/YYYY date, not an RFC date.
        month = day - 1; // 0-based
        match parse_long(&mut date_string) {
            Some(value) => day = value,
            None => return (nan, is_local_time),
        }
        if !(1..=31).contains(&day) {
            return (nan, is_local_time);
        }
        skip_exactly(&mut date_string, b'/');
        if date_string.is_empty() {
            return (nan, is_local_time);
        }
    } else {
        skip_exactly(&mut date_string, b'-');

        skip_spaces_and_comments(&mut date_string);

        skip_exactly(&mut date_string, b',');

        if month == -1 {
            // not found yet
            month = find_month(date_string);
            if month == -1 {
                return (nan, is_local_time);
            }

            while let Some(&front) = date_string.first() {
                if front == b'-' || front == b',' || is_ws(front) {
                    break;
                }
                date_string = &date_string[1..];
            }

            let Some(&front) = date_string.first() else { return (nan, is_local_time) };

            // '-99 23:12:40 GMT'
            if front != b'-' && front != b'/' && front != b',' && !is_ws(front) {
                return (nan, is_local_time);
            }
            date_string = &date_string[1..];
        }
    }

    if !(0..=11).contains(&month) {
        return (nan, is_local_time);
    }

    let mut new_pos_str = date_string;
    // '99 23:12:40 GMT'
    if !date_string.is_empty() && year.is_none() {
        match parse_int(&mut new_pos_str) {
            Some(result) => year = Some(result),
            None => return (nan, is_local_time),
        }
    }

    // Don't fail if the time is missing.
    let mut hour: i64 = 0;
    let mut minute: i64 = 0;
    let mut second: i64 = 0;
    if new_pos_str.is_empty() {
        date_string = new_pos_str;
    } else {
        // ' 23:12:40 GMT'
        if !(is_ws(new_pos_str[0]) || new_pos_str[0] == b',') {
            if new_pos_str[0] != b':' {
                return (nan, is_local_time);
            }
            // There was no year; the number was the hour.
            year = None;
        } else {
            // in the normal case (we parsed the year), advance to the next number
            // ' at 23:12:40 GMT'
            if new_pos_str.len() >= 3
                && is_ws(new_pos_str[0])
                && new_pos_str[1].eq_ignore_ascii_case(&b'a')
                && new_pos_str[2].eq_ignore_ascii_case(&b't')
            {
                new_pos_str = &new_pos_str[3..];
            } else {
                new_pos_str = &new_pos_str[1..]; // space or comma
            }
            date_string = new_pos_str;
            skip_spaces_and_comments(&mut date_string);
        }

        new_pos_str = date_string;
        if let Some(value) = parse_long(&mut new_pos_str) {
            hour = value;
        }
        // Do not check for errno here since we want to continue even if errno was set because we
        // are still looking for the timezone!

        // Read a number? If not, this might be a timezone name.
        if new_pos_str.len() != date_string.len() {
            date_string = new_pos_str;

            if !(0..=23).contains(&hour) {
                return (nan, is_local_time);
            }

            if date_string.is_empty() {
                return (nan, is_local_time);
            }

            // ':12:40 GMT'
            if !skip_exactly(&mut date_string, b':') {
                return (nan, is_local_time);
            }

            match parse_long(&mut date_string) {
                Some(value) => minute = value,
                None => return (nan, is_local_time),
            }

            if !(0..=59).contains(&minute) {
                return (nan, is_local_time);
            }

            // ':40 GMT'
            if date_string.first().is_some_and(|ch| *ch != b':' && !is_ws(*ch)) {
                return (nan, is_local_time);
            }

            // seconds are optional in rfc822 + rfc2822
            if skip_exactly(&mut date_string, b':') {
                match parse_long(&mut date_string) {
                    Some(value) => second = value,
                    None => return (nan, is_local_time),
                }

                if !(0..=59).contains(&second) {
                    return (nan, is_local_time);
                }
            }

            skip_spaces_and_comments(&mut date_string);

            if skip_letters_ignoring_case(&mut date_string, "am") {
                if hour > 12 {
                    return (nan, is_local_time);
                }
                if hour == 12 {
                    hour = 0;
                }
                skip_spaces_and_comments(&mut date_string);
            } else if skip_letters_ignoring_case(&mut date_string, "pm") {
                if hour > 12 {
                    return (nan, is_local_time);
                }
                if hour != 12 {
                    hour += 12;
                }
                skip_spaces_and_comments(&mut date_string);
            }
        }
    }

    // The year may be after the time but before the time zone.
    if date_string.first().is_some_and(|ch| is_digit(*ch)) && year.is_none() {
        match parse_int(&mut date_string) {
            Some(result) => year = Some(result),
            None => return (nan, is_local_time),
        }
        skip_spaces_and_comments(&mut date_string);
    }

    // Don't fail if the time zone is missing. Some websites omit the time zone (4275206).
    if !date_string.is_empty() {
        if skip_letters_ignoring_case(&mut date_string, "gmt") || skip_letters_ignoring_case(&mut date_string, "utc") {
            is_local_time = false;
        }

        if matches!(date_string.first(), Some(b'+') | Some(b'-')) {
            let mut o = match parse_int(&mut date_string) {
                Some(value) => value,
                None => return (nan, is_local_time),
            };

            if !(-9959..=9959).contains(&o) {
                return (nan, is_local_time);
            }

            let sgn = if o < 0 { -1 } else { 1 };
            o = o.abs();
            if !skip_exactly(&mut date_string, b':') {
                if o >= 24 {
                    offset = ((o / 100) * 60 + (o % 100)) * sgn;
                } else {
                    offset = o * 60 * sgn;
                }
            } else {
                // GMT+05:00
                let o2 = match parse_int(&mut date_string) {
                    Some(value) => value,
                    None => return (nan, is_local_time),
                };
                offset = (o * 60 + o2) * sgn;
            }
            is_local_time = false;
        } else {
            for (tz_name, tz_offset) in KNOWN_ZONES {
                // Since the passed-in length is used for both strings, the following checks that
                // dateString has the time zone name as a prefix, not that it is equal.
                if skip_letters_ignoring_case(&mut date_string, tz_name) {
                    offset = tz_offset;
                    is_local_time = false;
                    break;
                }
            }
        }
    }

    skip_spaces_and_comments(&mut date_string);

    if !date_string.is_empty() && year.is_none() {
        match parse_int(&mut date_string) {
            Some(result) => year = Some(result),
            None => return (nan, is_local_time),
        }
        skip_spaces_and_comments(&mut date_string);
    }

    // Trailing garbage
    if !date_string.is_empty() {
        return (nan, is_local_time);
    }

    // Y2K: Handle 2 digit years.
    let year_value = match year {
        Some(mut year_value) => {
            if (0..100).contains(&year_value) {
                if year_value < 50 {
                    year_value += 2000;
                } else {
                    year_value += 1900;
                }
            }
            year_value
        }
        // We select 2000 as default value. This is because of the following reasons.
        // 1. Year 2000 was used for the initial value of the variable `year`. While it won't be
        //    posed to users in WebKit, V8 used this 2000 as its default value.
        // 2. It is a leap year. When using `new Date("Feb 29")`, we assume that people want to save
        //    month and day. Leap year can save user inputs if they is valid. If we use the current
        //    year instead, the current year may not be a leap year. In that case,
        //    `new Date("Feb 29").getMonth()` becomes 2 (March).
        None => 2000,
    };

    if day <= 0 || day > 31 {
        return (nan, is_local_time);
    }

    let value = ymdhmsto_milliseconds(year_value, month + 1, day, hour, minute, second, 0.0)
        - offset as f64 * (SECONDS_PER_MINUTE * MS_PER_SECOND);
    (value, is_local_time)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_roundtrip() {
        assert_eq!(date_to_days_from_1970(1970, 0, 1), 0.0);
        assert_eq!(date_to_days_from_1970(2000, 0, 1), 10957.0);
        assert_eq!(year_month_day_from_days(10957), (2000, 0, 1));
        assert_eq!(year_month_day_from_days(-1), (1969, 11, 31));
        assert_eq!(week_day(0), 4);
        assert_eq!(week_day(-1), 3);
        assert_eq!(ms_to_year(0.0), 1970);
        assert_eq!(ms_to_year(-1.0), 1969);
    }

    #[test]
    fn parses_iso() {
        assert_eq!(parse_es5_date(b"1970-01-01T00:00:00.000Z"), (0.0, false));
        assert_eq!(parse_es5_date(b"1970-01-01T00:00:01Z"), (1000.0, false));
        assert_eq!(parse_es5_date(b"1970-01-01"), (0.0, false));
        assert_eq!(parse_es5_date(b"1970-01-01T00:00"), (0.0, true));
        assert!(parse_es5_date(b"1970-13-01").0.is_nan());
        assert_eq!(parse_es5_date(b"1970-01-01T01:00:00+01:00"), (0.0, false));
    }

    #[test]
    fn parses_rfc() {
        assert_eq!(parse_date(b"Thu, 01 Jan 1970 00:00:00 GMT"), (0.0, false));
        assert_eq!(parse_date(b"Jan 1 1970 00:00:00 UTC"), (0.0, false));
        assert_eq!(parse_date(b"1/2/1970"), (86_400_000.0, true));
        assert_eq!(parse_date(b"01 Jan 70 01:00 +0100"), (0.0, false));
        assert!(parse_date(b"garbage").0.is_nan());
    }
}
