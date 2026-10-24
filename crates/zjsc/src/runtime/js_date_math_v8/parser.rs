//! Porte da segunda metade de `runtime/JSDateMath-v8.cpp` (linhas 491 a 1175): os compositores
//! (`DayComposer`, `TimeComposer`, `TimeZoneComposer`), `ReadMilliseconds`, `DateParser::Parse`,
//! `ParseES5DateTime`, `TimeClip`, `MakeDay`, `MakeTime`, `MakeDate` e `ParseDateTimeString`.
//!
//! O scanner (`KeywordTable`, `DateToken`, `InputReader`, `DateStringTokenizer`) vive em
//! `scanner.rs`; a API usada aqui já foi conciliada com ele. Os símbolos (`':'`, `'-'`, ...) são
//! `u8` (`b':'`), e o `IsSymbol(char)` do C++ é `is_symbol_char`. `ParseES5DateTime` é genérico no
//! tipo de caractere do leitor (`C: AsciiChar`); o `Parse` lê os bytes UTF-8, então usa `u8`.
//!
//! DIVERGÊNCIAS:
//!
//! - `DateParser::Parse` recebe um `isolate` nulo no C++; aqui o parâmetro some.
//! - `Smi::IsValid` com `JSVALUE64` (`kSmiValueSize` 32): o ano já é um `i32`, então a checagem
//!   `value == static_cast<int32_t>(value)` é sempre verdadeira; é mantida para ficar igual.
//! - A saída do `DateParser` é um `[f64; OUTPUT_SIZE]`, indexado pelas constantes `YEAR`..`UTC_OFFSET`.
//! - `TimeClip`, `MakeDay`, `MakeTime` e `MakeDate` ficam neste arquivo porque só o
//!   `ParseDateTimeString` os usa; `js_date_math.rs` chama `time_clip` direto.

use super::scanner::{DateStringTokenizer, DateToken, InputReader, KeywordType, K_MAX_SIGNIFICANT_DIGITS, K_NONE};
use crate::wtf::ascii_ctype::AsciiChar;

// ---------------------------------------------------------------------------------------------
// Utilidades (JSDateMath-v8.cpp:31-79)
// ---------------------------------------------------------------------------------------------

/// `kSmiMinValue` / `kSmiMaxValue` / `kMaxValue` com `kSmiValueSize` 32 (JSDateMath-v8.cpp:41-43).
const K_MAX_VALUE: i32 = i32::MAX;

/// `Smi::IsValid` (JSDateMath-v8.cpp:71), ramo `JSVALUE64`.
fn smi_is_valid(value: i64) -> bool {
    value == value as i32 as i64
}

/// `DateParser::Between` (JSDateMath-v8.cpp:113).
fn between(x: i32, lo: i32, hi: i32) -> bool {
    (x.wrapping_sub(lo) as u32) <= (hi.wrapping_sub(lo) as u32)
}

// Índices do array de saída (`DateParser`, JSDateMath-v8.cpp:84).
const YEAR: usize = 0;
const MONTH: usize = 1;
const DAY: usize = 2;
const HOUR: usize = 3;
const MINUTE: usize = 4;
const SECOND: usize = 5;
const MILLISECOND: usize = 6;
const UTC_OFFSET: usize = 7;
const OUTPUT_SIZE: usize = 8;

// ---------------------------------------------------------------------------------------------
// TimeZoneComposer (JSDateMath-v8.cpp:375 e 583)
// ---------------------------------------------------------------------------------------------

struct TimeZoneComposer {
    sign: i32,
    hour: i32,
    minute: i32,
}

impl TimeZoneComposer {
    /// Construtor (JSDateMath-v8.cpp:377).
    fn new() -> Self {
        TimeZoneComposer { sign: K_NONE, hour: K_NONE, minute: K_NONE }
    }

    /// `Set` (JSDateMath-v8.cpp:383).
    fn set(&mut self, offset_in_hours: i32) {
        self.sign = if offset_in_hours < 0 { -1 } else { 1 };
        self.hour = offset_in_hours * self.sign;
        self.minute = 0;
    }

    /// `SetSign` (JSDateMath-v8.cpp:389).
    fn set_sign(&mut self, sign: i32) {
        self.sign = if sign < 0 { -1 } else { 1 };
    }

    /// `SetAbsoluteHour` (JSDateMath-v8.cpp:390).
    fn set_absolute_hour(&mut self, hour: i32) {
        self.hour = hour;
    }

    /// `SetAbsoluteMinute` (JSDateMath-v8.cpp:391).
    fn set_absolute_minute(&mut self, minute: i32) {
        self.minute = minute;
    }

    /// `IsExpecting` (JSDateMath-v8.cpp:392).
    fn is_expecting(&self, n: i32) -> bool {
        self.hour != K_NONE && self.minute == K_NONE && TimeComposer::is_minute(n)
    }

    /// `IsUTC` (JSDateMath-v8.cpp:396).
    fn is_utc(&self) -> bool {
        self.hour == 0 && self.minute == 0
    }

    /// `IsEmpty` (JSDateMath-v8.cpp:398).
    fn is_empty(&self) -> bool {
        self.hour == K_NONE
    }

    /// `Write` (JSDateMath-v8.cpp:583).
    fn write(&mut self, output: &mut [f64; OUTPUT_SIZE]) -> bool {
        if self.sign != K_NONE {
            if self.hour == K_NONE {
                self.hour = 0;
            }
            if self.minute == K_NONE {
                self.minute = 0;
            }
            // Avoid signed integer overflow (undefined behavior) by doing unsigned
            // arithmetic.
            let total_seconds_unsigned = (self.hour as u32).wrapping_mul(3600).wrapping_add((self.minute as u32).wrapping_mul(60));
            if total_seconds_unsigned > K_MAX_VALUE as u32 {
                return false;
            }
            let mut total_seconds = total_seconds_unsigned as i32;
            if self.sign < 0 {
                total_seconds = -total_seconds;
            }
            debug_assert!(smi_is_valid(total_seconds as i64));
            output[UTC_OFFSET] = total_seconds as f64;
        } else {
            output[UTC_OFFSET] = f64::NAN;
        }
        true
    }
}

// ---------------------------------------------------------------------------------------------
// TimeComposer (JSDateMath-v8.cpp:406 e 550)
// ---------------------------------------------------------------------------------------------

struct TimeComposer {
    comp: [i32; Self::SIZE],
    index: usize,
    hour_offset: i32,
}

impl TimeComposer {
    /// `kSize` (JSDateMath-v8.cpp:441).
    const SIZE: usize = 4;

    /// Construtor (JSDateMath-v8.cpp:408).
    fn new() -> Self {
        TimeComposer { comp: [0; Self::SIZE], index: 0, hour_offset: K_NONE }
    }

    /// `IsEmpty` (JSDateMath-v8.cpp:413).
    fn is_empty(&self) -> bool {
        self.index == 0
    }

    /// `IsExpecting` (JSDateMath-v8.cpp:414).
    fn is_expecting(&self, n: i32) -> bool {
        (self.index == 1 && Self::is_minute(n)) || (self.index == 2 && Self::is_second(n)) || (self.index == 3 && Self::is_millisecond(n))
    }

    /// `Add` (JSDateMath-v8.cpp:418).
    fn add(&mut self, n: i32) -> bool {
        if self.index < Self::SIZE {
            self.comp[self.index] = n;
            self.index += 1;
            true
        } else {
            false
        }
    }

    /// `AddFinal` (JSDateMath-v8.cpp:422).
    fn add_final(&mut self, n: i32) -> bool {
        if !self.add(n) {
            return false;
        }
        while self.index < Self::SIZE {
            self.comp[self.index] = 0;
            self.index += 1;
        }
        true
    }

    /// `SetHourOffset` (JSDateMath-v8.cpp:430).
    fn set_hour_offset(&mut self, n: i32) {
        self.hour_offset = n;
    }

    /// `IsMinute` (JSDateMath-v8.cpp:433).
    fn is_minute(x: i32) -> bool {
        between(x, 0, 59)
    }

    /// `IsHour` (JSDateMath-v8.cpp:434).
    fn is_hour(x: i32) -> bool {
        between(x, 0, 23)
    }

    /// `IsSecond` (JSDateMath-v8.cpp:435).
    fn is_second(x: i32) -> bool {
        between(x, 0, 59)
    }

    /// `IsHour12` (JSDateMath-v8.cpp:438).
    fn is_hour12(x: i32) -> bool {
        between(x, 0, 12)
    }

    /// `IsMillisecond` (JSDateMath-v8.cpp:439).
    fn is_millisecond(x: i32) -> bool {
        between(x, 0, 999)
    }

    /// `Write` (JSDateMath-v8.cpp:550).
    fn write(&mut self, output: &mut [f64; OUTPUT_SIZE]) -> bool {
        // All time slots default to 0
        while self.index < Self::SIZE {
            self.comp[self.index] = 0;
            self.index += 1;
        }

        if self.hour_offset != K_NONE {
            if !Self::is_hour12(self.comp[0]) {
                return false;
            }
            self.comp[0] %= 12;
            self.comp[0] += self.hour_offset;
        }

        let hour = self.comp[0];
        let minute = self.comp[1];
        let second = self.comp[2];
        let millisecond = self.comp[3];

        if !Self::is_hour(hour) || !Self::is_minute(minute) || !Self::is_second(second) || !Self::is_millisecond(millisecond) {
            // A 24th hour is allowed if minutes, seconds, and milliseconds are 0
            if hour != 24 || minute != 0 || second != 0 || millisecond != 0 {
                return false;
            }
        }

        output[HOUR] = hour as f64;
        output[MINUTE] = minute as f64;
        output[SECOND] = second as f64;
        output[MILLISECOND] = millisecond as f64;
        true
    }
}

// ---------------------------------------------------------------------------------------------
// DayComposer (JSDateMath-v8.cpp:447 e 492)
// ---------------------------------------------------------------------------------------------

struct DayComposer {
    comp: [i32; Self::SIZE],
    index: usize,
    named_month: i32,
    /// If set, ensures that data is always parsed in year-month-date order.
    is_iso_date: bool,
}

impl DayComposer {
    /// `kSize` (JSDateMath-v8.cpp:472).
    const SIZE: usize = 3;

    /// Construtor (JSDateMath-v8.cpp:449).
    fn new() -> Self {
        DayComposer { comp: [0; Self::SIZE], index: 0, named_month: K_NONE, is_iso_date: false }
    }

    /// `IsEmpty` (JSDateMath-v8.cpp:455).
    fn is_empty(&self) -> bool {
        self.index == 0
    }

    /// `Add` (JSDateMath-v8.cpp:456).
    fn add(&mut self, n: i32) -> bool {
        if self.index < Self::SIZE {
            self.comp[self.index] = n;
            self.index += 1;
            true
        } else {
            false
        }
    }

    /// `SetNamedMonth` (JSDateMath-v8.cpp:465).
    fn set_named_month(&mut self, n: i32) {
        self.named_month = n;
    }

    /// `set_iso_date` (JSDateMath-v8.cpp:467).
    fn set_iso_date(&mut self) {
        self.is_iso_date = true;
    }

    /// `IsMonth` (JSDateMath-v8.cpp:468).
    fn is_month(x: i32) -> bool {
        between(x, 1, 12)
    }

    /// `IsDay` (JSDateMath-v8.cpp:469).
    fn is_day(x: i32) -> bool {
        between(x, 1, 31)
    }

    /// `Write` (JSDateMath-v8.cpp:492).
    fn write(&mut self, output: &mut [f64; OUTPUT_SIZE]) -> bool {
        if self.index < 1 {
            return false;
        }
        // Day and month defaults to 1.
        while self.index < Self::SIZE {
            self.comp[self.index] = 1;
            self.index += 1;
        }

        let mut year: i32 = 0; // Default year is 0 (=> 2000) for KJS compatibility.
        let month: i32;
        let day: i32;

        if self.named_month == K_NONE {
            // `index` já vale 3 aqui (o laço acima preenche até `kSize`), como no C++.
            if self.is_iso_date || (self.index == 3 && !Self::is_day(self.comp[0])) {
                // YMD
                year = self.comp[0];
                month = self.comp[1];
                day = self.comp[2];
            } else {
                // MD(Y)
                month = self.comp[0];
                day = self.comp[1];
                if self.index == 3 {
                    year = self.comp[2];
                }
            }
        } else {
            month = self.named_month;
            if self.index == 1 {
                // MD or DM
                day = self.comp[0];
            } else if !Self::is_day(self.comp[0]) {
                // YMD, MYD, or YDM
                year = self.comp[0];
                day = self.comp[1];
            } else {
                // DMY, MDY, or DYM
                day = self.comp[0];
                year = self.comp[1];
            }
        }

        if !self.is_iso_date {
            if between(year, 0, 49) {
                year += 2000;
            } else if between(year, 50, 99) {
                year += 1900;
            }
        }

        if !smi_is_valid(year as i64) || !Self::is_month(month) || !Self::is_day(day) {
            return false;
        }

        output[YEAR] = year as f64;
        output[MONTH] = (month - 1) as f64; // 0-based
        output[DAY] = day as f64;
        true
    }
}

// ---------------------------------------------------------------------------------------------
// ReadMilliseconds (JSDateMath-v8.cpp:658)
// ---------------------------------------------------------------------------------------------

/// `DateParser::ReadMilliseconds`.
fn read_milliseconds(token: DateToken) -> i32 {
    // Read first three significant digits of the original numeral,
    // as inferred from the value and the number of digits.
    // I.e., use the number of digits to see if there were
    // leading zeros.
    let mut number = token.number();
    let mut length = token.length();
    if length < 3 {
        // Less than three digits. Multiply to put most significant digit
        // in hundreds position.
        if length == 1 {
            number *= 100;
        } else if length == 2 {
            number *= 10;
        }
    } else if length > 3 {
        if length > K_MAX_SIGNIFICANT_DIGITS {
            length = K_MAX_SIGNIFICANT_DIGITS;
        }
        // More than three digits. Divide by 10^(length - 3) to get three
        // most significant digits.
        let mut factor: i32 = 1;
        loop {
            debug_assert!(factor <= 100_000_000); // factor won't overflow.
            factor *= 10;
            length -= 1;
            if length <= 3 {
                break;
            }
        }
        number /= factor;
    }
    number
}

// ---------------------------------------------------------------------------------------------
// DateParser::Parse (JSDateMath-v8.cpp:692)
// ---------------------------------------------------------------------------------------------

/// `DateParser::Parse`: preenche `out` com `[ano, mês, dia, hora, minuto, segundo, milissegundo,
/// deslocamento UTC em segundos ou NaN]` e devolve `true`, ou devolve `false` se a string não é uma data.
fn date_parser_parse(str: &[u8], out: &mut [f64; OUTPUT_SIZE]) -> bool {
    let mut input = InputReader::new(str);
    let mut scanner = DateStringTokenizer::new(&mut input);
    let mut tz = TimeZoneComposer::new();
    let mut time = TimeComposer::new();
    let mut day = DayComposer::new();

    // Specification:
    // Accept ES5 ISO 8601 date-time-strings or legacy dates compatible
    // with Safari.
    // (A especificação completa está em JSDateMath-v8.cpp:700-747.)

    // First try getting as far as possible with as ES5 Date Time String.
    let next_unhandled_token = parse_es5_date_time(&mut scanner, &mut day, &mut time, &mut tz);
    if next_unhandled_token.is_invalid() {
        return false;
    }
    let mut has_read_number = !day.is_empty();
    // If there's anything left, continue with the legacy parser.
    let mut token = next_unhandled_token;
    while !token.is_end_of_input() {
        if token.is_number() {
            has_read_number = true;
            let n = token.number();
            if scanner.skip_symbol(b':') {
                if scanner.skip_symbol(b':') {
                    // n + "::"
                    if !time.is_empty() {
                        return false;
                    }
                    time.add(n);
                    time.add(0);
                } else {
                    // n + ":"
                    if !time.add(n) {
                        return false;
                    }
                    if scanner.peek().is_symbol_char(b'.') {
                        scanner.next();
                    }
                }
            } else if scanner.skip_symbol(b'.') && time.is_expecting(n) {
                time.add(n);
                if !scanner.peek().is_number() {
                    return false;
                }
                let ms = read_milliseconds(scanner.next());
                if ms < 0 {
                    return false;
                }
                time.add_final(ms);
            } else if tz.is_expecting(n) {
                tz.set_absolute_minute(n);
            } else if time.is_expecting(n) {
                time.add_final(n);
                // Require end, white space, "Z", "+" or "-" immediately after
                // finalizing time.
                let peek = scanner.peek();
                if !peek.is_end_of_input() && !peek.is_white_space() && !peek.is_keyword_z() && !peek.is_ascii_sign() {
                    return false;
                }
            } else {
                if !day.add(n) {
                    return false;
                }
                scanner.skip_symbol(b'-');
            }
        } else if token.is_keyword() {
            // Parse a "word" (sequence of chars. >= 'A').
            let keyword_type = token.keyword_type();
            let value = token.keyword_value();
            if keyword_type == KeywordType::AmPm && !time.is_empty() {
                time.set_hour_offset(value);
            } else if keyword_type == KeywordType::MonthName {
                day.set_named_month(value);
                scanner.skip_symbol(b'-');
            } else if keyword_type == KeywordType::TimeZoneName && has_read_number {
                tz.set(value);
            } else {
                // Garbage words are illegal if a number has been read.
                if has_read_number {
                    return false;
                }
                // The first number has to be separated from garbage words by
                // whitespace or other separators.
                if scanner.peek().is_number() {
                    return false;
                }
            }
        } else if token.is_ascii_sign() && (tz.is_utc() || !time.is_empty()) {
            // Parse UTC offset (only after UTC or time).
            tz.set_sign(token.ascii_sign());
            // The following number may be empty.
            let mut n: i32 = 0;
            let mut length: i32 = 0;
            if scanner.peek().is_number() {
                let next_token = scanner.next();
                length = next_token.length();
                n = next_token.number();
            }
            has_read_number = true;

            if scanner.peek().is_symbol_char(b':') {
                tz.set_absolute_hour(n);
                // TODO(littledan): Use minutes as part of timezone?
                tz.set_absolute_minute(K_NONE);
            } else if length == 2 || length == 1 {
                // Handle time zones like GMT-8
                tz.set_absolute_hour(n);
                tz.set_absolute_minute(0);
            } else if length == 4 || length == 3 {
                // Looks like the hhmm format
                tz.set_absolute_hour(n / 100);
                tz.set_absolute_minute(n % 100);
            } else {
                // No need to accept time zones like GMT-12345
                return false;
            }
        } else if (token.is_ascii_sign() || token.is_symbol_char(b')')) && has_read_number {
            // Extra sign or ')' is illegal if a number has been read.
            return false;
        } else {
            // Ignore other characters and whitespace.
        }
        token = scanner.next();
    }

    // `&&` em curto-circuito, na mesma ordem do C++ (JSDateMath-v8.cpp:856).
    day.write(out) && time.write(out) && tz.write(out)
}

// ---------------------------------------------------------------------------------------------
// ParseES5DateTime (JSDateMath-v8.cpp:932)
// ---------------------------------------------------------------------------------------------

/// `DateParser::ParseES5DateTime`: tenta ler um ES5 Date Time String. Devolve o próximo token para o
/// parser legado continuar, `EndOfInput` se terminou, ou `Invalid` se falhou de vez.
fn parse_es5_date_time<C: AsciiChar>(
    scanner: &mut DateStringTokenizer<'_, '_, C>,
    day: &mut DayComposer,
    time: &mut TimeComposer,
    tz: &mut TimeZoneComposer,
) -> DateToken {
    debug_assert!(day.is_empty());
    debug_assert!(time.is_empty());
    debug_assert!(tz.is_empty());

    // Parse mandatory date string: [('-'|'+')yy]yyyy[':'MM[':'DD]]
    if scanner.peek().is_ascii_sign() {
        // Keep the sign token, so we can pass it back to the legacy
        // parser if we don't use it.
        let sign_token = scanner.next();
        if !scanner.peek().is_fixed_length_number(6) {
            return sign_token;
        }
        let sign = sign_token.ascii_sign();
        let year = scanner.next().number();
        if sign < 0 && year == 0 {
            return sign_token;
        }
        day.add(sign * year);
    } else if scanner.peek().is_fixed_length_number(4) {
        day.add(scanner.next().number());
    } else {
        return scanner.next();
    }
    if scanner.skip_symbol(b'-') {
        if !scanner.peek().is_fixed_length_number(2) || !DayComposer::is_month(scanner.peek().number()) {
            return scanner.next();
        }
        day.add(scanner.next().number());
        if scanner.skip_symbol(b'-') {
            if !scanner.peek().is_fixed_length_number(2) || !DayComposer::is_day(scanner.peek().number()) {
                return scanner.next();
            }
            day.add(scanner.next().number());
        }
    }
    // Check for optional time string: 'T'HH':'mm[':'ss['.'sss]]Z
    if !scanner.peek().is_keyword_type(KeywordType::TimeSeparator) {
        if !scanner.peek().is_end_of_input() {
            return scanner.next();
        }
    } else {
        // ES5 Date Time String time part is present.
        scanner.next();
        if !scanner.peek().is_fixed_length_number(2) || !between(scanner.peek().number(), 0, 24) {
            return DateToken::invalid();
        }
        // Allow 24:00[:00[.000]], but no other time starting with 24.
        let hour_is_24 = scanner.peek().number() == 24;
        time.add(scanner.next().number());
        if !scanner.skip_symbol(b':') {
            return DateToken::invalid();
        }
        if !scanner.peek().is_fixed_length_number(2) || !TimeComposer::is_minute(scanner.peek().number()) || (hour_is_24 && scanner.peek().number() > 0) {
            return DateToken::invalid();
        }
        time.add(scanner.next().number());
        if scanner.skip_symbol(b':') {
            if !scanner.peek().is_fixed_length_number(2) || !TimeComposer::is_second(scanner.peek().number()) || (hour_is_24 && scanner.peek().number() > 0) {
                return DateToken::invalid();
            }
            time.add(scanner.next().number());
            if scanner.skip_symbol(b'.') {
                if !scanner.peek().is_number() || (hour_is_24 && scanner.peek().number() > 0) {
                    return DateToken::invalid();
                }
                // Allow more or less than the mandated three digits.
                time.add(read_milliseconds(scanner.next()));
            }
        }
        // Check for optional timezone designation: 'Z' | ('+'|'-')hh':'mm
        if scanner.peek().is_keyword_z() {
            scanner.next();
            tz.set(0);
        } else if scanner.peek().is_symbol_char(b'+') || scanner.peek().is_symbol_char(b'-') {
            tz.set_sign(if scanner.next().symbol() == b'+' { 1 } else { -1 });
            if scanner.peek().is_fixed_length_number(4) {
                // hhmm extension syntax.
                let hourmin = scanner.next().number();
                let hour = hourmin / 100;
                let min = hourmin % 100;
                if !TimeComposer::is_hour(hour) || !TimeComposer::is_minute(min) {
                    return DateToken::invalid();
                }
                tz.set_absolute_hour(hour);
                tz.set_absolute_minute(min);
            } else {
                // hh:mm standard syntax.
                if !scanner.peek().is_fixed_length_number(2) || !TimeComposer::is_hour(scanner.peek().number()) {
                    return DateToken::invalid();
                }
                tz.set_absolute_hour(scanner.next().number());
                if !scanner.skip_symbol(b':') {
                    return DateToken::invalid();
                }
                if !scanner.peek().is_fixed_length_number(2) || !TimeComposer::is_minute(scanner.peek().number()) {
                    return DateToken::invalid();
                }
                tz.set_absolute_minute(scanner.next().number());
            }
        }
        if !scanner.peek().is_end_of_input() {
            return DateToken::invalid();
        }
    }
    // Successfully parsed ES5 Date Time String.
    // ES#sec-date-time-string-format Date Time String Format
    // "When the time zone offset is absent, date-only forms are interpreted
    //  as a UTC time and date-time forms are interpreted as a local time."
    if tz.is_empty() && time.is_empty() {
        tz.set(0);
    }
    day.set_iso_date();
    DateToken::end_of_input()
}

// ---------------------------------------------------------------------------------------------
// Aritmética de datas (JSDateMath-v8.cpp:1045-1147)
// ---------------------------------------------------------------------------------------------

// ES6 section 20.3.1.1 Time Values and Time Range (JSDateMath-v8.cpp:1045).
const K_MIN_YEAR: f64 = -1000000.0;
const K_MAX_YEAR: f64 = -K_MIN_YEAR;
const K_MIN_MONTH: f64 = -10000000.0;
const K_MAX_MONTH: f64 = -K_MIN_MONTH;

const K_MS_PER_DAY: f64 = 86400000.0;

const K_MS_PER_SECOND: f64 = 1000.0;
const K_MS_PER_MINUTE: f64 = 60000.0;
const K_MS_PER_HOUR: f64 = 3600000.0;
const K_MS_PER_MONTH: i64 = (K_MS_PER_DAY * 30.0) as i64;

/// The largest time that can be stored in JSDate (JSDateMath-v8.cpp:1058).
const K_MAX_TIME_IN_MS: i64 = 864000000_i64 * 10000000;

/// Conservative upper bound on time that can be stored in JSDate before UTC conversion
/// (JSDateMath-v8.cpp:1062).
const K_MAX_TIME_BEFORE_UTC_IN_MS: i64 = K_MAX_TIME_IN_MS + K_MS_PER_MONTH;

/// `DoubleToInteger` (JSDateMath-v8.cpp:1066), `#sec-tointegerorinfinity`.
fn double_to_integer(x: f64) -> f64 {
    // ToIntegerOrInfinity normalizes -0 to +0. Special case 0 for performance.
    if x.is_nan() || x == 0.0 {
        return 0.0;
    }
    if !x.is_finite() {
        return x;
    }
    // Add 0.0 in the truncation case to ensure this doesn't return -0.
    (if x > 0.0 { x.floor() } else { x.ceil() }) + 0.0
}

/// `TimeClip` (JSDateMath-v8.cpp:1079), ECMA 262 `#sec-timeclip`.
pub fn time_clip(time: f64) -> f64 {
    if -(K_MAX_TIME_IN_MS as f64) <= time && time <= K_MAX_TIME_IN_MS as f64 {
        return double_to_integer(time);
    }
    f64::NAN
}

/// `MakeDay` (JSDateMath-v8.cpp:1088).
fn make_day(year: f64, month: f64, date: f64) -> f64 {
    if (K_MIN_YEAR <= year && year <= K_MAX_YEAR) && (K_MIN_MONTH <= month && month <= K_MAX_MONTH) && date.is_finite() {
        let mut y = year as i32;
        let mut m = month as i32;
        y += m / 12;
        m %= 12;
        if m < 0 {
            m += 12;
            y -= 1;
        }
        debug_assert!(0 <= m);
        debug_assert!(m < 12);

        // kYearDelta is an arbitrary number such that:
        // a) kYearDelta = -1 (mod 400)
        // b) year + kYearDelta > 0 for years in the range defined by
        //    ECMA 262 - 15.9.1.1, i.e. upto 100,000,000 days on either side of
        //    Jan 1 1970. This is required so that we don't run into integer
        //    division of negative numbers.
        // c) there shouldn't be an overflow for 32-bit integers in the following
        //    operations.
        const K_YEAR_DELTA: i32 = 399999;
        const K_BASE_DAY: i32 =
            365 * (1970 + K_YEAR_DELTA) + (1970 + K_YEAR_DELTA) / 4 - (1970 + K_YEAR_DELTA) / 100 + (1970 + K_YEAR_DELTA) / 400;
        let mut day_from_year =
            365 * (y + K_YEAR_DELTA) + (y + K_YEAR_DELTA) / 4 - (y + K_YEAR_DELTA) / 100 + (y + K_YEAR_DELTA) / 400 - K_BASE_DAY;
        if (y % 4 != 0) || (y % 100 == 0 && y % 400 != 0) {
            const K_DAY_FROM_MONTH: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
            day_from_year += K_DAY_FROM_MONTH[m as usize];
        } else {
            const K_DAY_FROM_MONTH: [i32; 12] = [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];
            day_from_year += K_DAY_FROM_MONTH[m as usize];
        }
        return (day_from_year - 1) as f64 + double_to_integer(date);
    }
    f64::NAN
}

/// `MakeTime` (JSDateMath-v8.cpp:1128).
fn make_time(hour: f64, min: f64, sec: f64, ms: f64) -> f64 {
    if hour.is_finite() && min.is_finite() && sec.is_finite() && ms.is_finite() {
        let h = double_to_integer(hour);
        let m = double_to_integer(min);
        let s = double_to_integer(sec);
        let milli = double_to_integer(ms);
        return h * K_MS_PER_HOUR + m * K_MS_PER_MINUTE + s * K_MS_PER_SECOND + milli;
    }
    f64::NAN
}

/// `MakeDate` (JSDateMath-v8.cpp:1141).
fn make_date(day: f64, time: f64) -> f64 {
    if day.is_finite() && time.is_finite() {
        return time + day * K_MS_PER_DAY;
    }
    f64::NAN
}

// ---------------------------------------------------------------------------------------------
// ParseDateTimeString (JSDateMath-v8.cpp:1150)
// ---------------------------------------------------------------------------------------------

/// `v8::ParseDateTimeString`: o instante em milissegundos desde a época, ou NaN. `local` vira
/// verdadeiro quando a string não traz fuso: o chamador (`DateCache::parse_date`) subtrai o
/// deslocamento local.
pub fn parse_date_time_string(str: &[u8], local: &mut bool) -> f64 {
    let mut out = [0.0_f64; OUTPUT_SIZE];

    if !date_parser_parse(str, &mut out) {
        return f64::NAN;
    }

    let day = make_day(out[YEAR], out[MONTH], out[DAY]);
    let time = make_time(out[HOUR], out[MINUTE], out[SECOND], out[MILLISECOND]);

    let mut date = make_date(day, time);

    if out[UTC_OFFSET].is_nan() {
        if date >= -(K_MAX_TIME_BEFORE_UTC_IN_MS as f64) && date <= K_MAX_TIME_BEFORE_UTC_IN_MS as f64 {
            // DIFF: Use JSC DateCache::DSTCache instead of v8's DateCache. Mark as local and handle
            // the offset in DateCache::ParseDate.
            *local = true;
        } else {
            return f64::NAN;
        }
    } else {
        date -= out[UTC_OFFSET] * 1000.0;
    }

    date
}
