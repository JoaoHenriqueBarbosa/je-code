//! Porte de `runtime/JSDateMath.h` e `JSDateMath.cpp`: o `DateCache` (deslocamento local com o cache de
//! horário de verão do V8, desmembramento de um instante em data e hora, parse de data com cache da
//! última string) e o `PlainGregorianDateTime`, a data desmembrada compactada em 64 bits.
//!
//! DIVERGÊNCIAS:
//!
//! - O fuso vem de um `TimeZoneSource` no lugar do `UCalendar` do ICU (`OpaqueICUTimeZone`,
//!   `ucal_getTimeZoneOffsetFromLocal`, `ucal_getTimeZoneDisplayName`). Dois métodos devolvem o
//!   deslocamento combinado (UTC mais horário de verão) de um instante UTC e de um horário local; este
//!   último resolve lacunas e sobreposições como o `UCAL_TZ_LOCAL_FORMER` do ICU (vale o deslocamento
//!   de antes da transição). `UtcTimeZone` é o fuso padrão (o sandbox sem `TZ`).
//! - LIGAÇÃO COM O FUSO DO PROCESSO: `process_time_zone.rs` implementa o `TimeZoneSource` sobre
//!   `ul_common::time::zone` (jiff), e o `VM` o entrega ao `DateCache` (`VM::date_cache`). O nome longo
//!   do fuso (`time_zone_display_name`) pede o CLDR ("Brasilia Standard Time", "Coordinated Universal
//!   Time"), que o jiff não tem; vem da tabela de `time_zone_names.rs`.
//! - `listenForTimeZoneChangeNotifications`, `lastTimeZoneID` e `retrieveTimeZoneInformation` (o
//!   cache global do fuso do host) viram `TimeZoneSource::generation()`: a fonte incrementa o número
//!   quando o `TZ` muda, e `has_time_zone_change` compara com o do último `clear_for_time_zone_change`.
//! - `Options::useV8DateParser()` é verdadeiro por padrão, como no Bun (`ZigGlobalObject.cpp:316`);
//!   `parse_date` usa o parser V8 (`js_date_math_v8`, porte de `JSDateMath-v8.cpp`) e cai em
//!   `parseES5Date` e `parseDate` do WTF só com a opção desligada.
//! - O `BrokenDownDateCache` indexa por um hash simples dos bits do `double` no lugar do
//!   `FloatHash<double>`; o cache só acelera, a escolha da entrada não é observável.
//! - Os caches ficam em `Cell`/`RefCell`: o `DateCache` é compartilhado pelo `VM` por `&`.

use std::cell::{Cell, RefCell};

use crate::runtime::js_date_math_v8;
use crate::runtime::js_object::PutError;
use crate::runtime::options_list::Options;
use crate::wtf::date_math::{
    self, equivalent_time, int64_milliseconds as i64ms, ms_to_days, time_in_day, week_day, year_month_day_from_days, LocalTimeOffset,
    TimeType,
};
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::wtf_string::{String as WtfString, UTF8ConversionError};

/// `minECMAScriptTime`.
pub const MIN_ECMASCRIPT_TIME: f64 = -8.64E15;

// ---------------------------------------------------------------------------------------------
// PlainGregorianDateTime
// ---------------------------------------------------------------------------------------------

const YEAR_WIDTH: u32 = 21;
const MONTH_WIDTH: u32 = 4;
const MONTH_DAY_WIDTH: u32 = 5;
const WEEK_DAY_WIDTH: u32 = 3;
const HOUR_WIDTH: u32 = 5;
const MINUTE_WIDTH: u32 = 6;
const SECOND_WIDTH: u32 = 6;
const UTC_OFFSET_IN_MINUTE_WIDTH: u32 = 13;
const IS_DST_WIDTH: u32 = 1;

const YEAR_MASK: u64 = (1 << YEAR_WIDTH) - 1;
const MONTH_MASK: u64 = (1 << MONTH_WIDTH) - 1;
const MONTH_DAY_MASK: u64 = (1 << MONTH_DAY_WIDTH) - 1;
const WEEK_DAY_MASK: u64 = (1 << WEEK_DAY_WIDTH) - 1;
const HOUR_MASK: u64 = (1 << HOUR_WIDTH) - 1;
const MINUTE_MASK: u64 = (1 << MINUTE_WIDTH) - 1;
const SECOND_MASK: u64 = (1 << SECOND_WIDTH) - 1;
const UTC_OFFSET_IN_MINUTE_MASK: u64 = (1 << UTC_OFFSET_IN_MINUTE_WIDTH) - 1;
const IS_DST_MASK: u64 = (1 << IS_DST_WIDTH) - 1;

const YEAR_OFFSET: u32 = 64 - YEAR_WIDTH;
const MONTH_OFFSET: u32 = YEAR_OFFSET - MONTH_WIDTH;
const MONTH_DAY_OFFSET: u32 = MONTH_OFFSET - MONTH_DAY_WIDTH;
const WEEK_DAY_OFFSET: u32 = MONTH_DAY_OFFSET - WEEK_DAY_WIDTH;
const HOUR_OFFSET: u32 = WEEK_DAY_OFFSET - HOUR_WIDTH;
const MINUTE_OFFSET: u32 = HOUR_OFFSET - MINUTE_WIDTH;
const SECOND_OFFSET: u32 = MINUTE_OFFSET - SECOND_WIDTH;
const UTC_OFFSET_IN_MINUTE_OFFSET: u32 = SECOND_OFFSET - UTC_OFFSET_IN_MINUTE_WIDTH;
const IS_DST_OFFSET: u32 = UTC_OFFSET_IN_MINUTE_OFFSET - IS_DST_WIDTH;
const _: () = assert!(IS_DST_OFFSET == 0);

/// A data desmembrada de um `Date`, em hora local ou em UTC, em 64 bits. Milissegundos e menores não
/// entram: saem do valor de tempo do `Date`. O payload zerado quer dizer "ainda não calculado"; o
/// `monthDay` nunca é 0 numa data de verdade, então nenhum valor válido colide com ele.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlainGregorianDateTime {
    payload: u64,
}

impl PlainGregorianDateTime {
    pub const MIN_YEAR: i32 = -271821;
    pub const MAX_YEAR: i32 = 275760;

    /// `PlainGregorianDateTime(year, month, monthDay, weekDay, hour, minute, second, utcOffsetInMinute, isDST)`.
    pub fn new(
        year: i32,
        month: i32,
        month_day: i32,
        week_day: i32,
        hour: i32,
        minute: i32,
        second: i32,
        utc_offset_in_minute: i32,
        is_dst: bool,
    ) -> PlainGregorianDateTime {
        let payload = (((year as u32 as u64) & YEAR_MASK) << YEAR_OFFSET)
            | ((month as u64) << MONTH_OFFSET)
            | ((month_day as u64) << MONTH_DAY_OFFSET)
            | ((week_day as u64) << WEEK_DAY_OFFSET)
            | ((hour as u64) << HOUR_OFFSET)
            | ((minute as u64) << MINUTE_OFFSET)
            | ((second as u64) << SECOND_OFFSET)
            | (((utc_offset_in_minute as u32 as u64) & UTC_OFFSET_IN_MINUTE_MASK) << UTC_OFFSET_IN_MINUTE_OFFSET)
            | ((is_dst as u64) << IS_DST_OFFSET);
        PlainGregorianDateTime { payload }
    }

    /// Com sinal e nos bits mais altos: o deslocamento aritmético estende o sinal.
    pub fn year(&self) -> i32 {
        ((self.payload as i64) >> YEAR_OFFSET) as i32
    }

    pub fn month(&self) -> i32 {
        ((self.payload >> MONTH_OFFSET) & MONTH_MASK) as i32
    }

    pub fn month_day(&self) -> i32 {
        ((self.payload >> MONTH_DAY_OFFSET) & MONTH_DAY_MASK) as i32
    }

    pub fn week_day(&self) -> i32 {
        ((self.payload >> WEEK_DAY_OFFSET) & WEEK_DAY_MASK) as i32
    }

    pub fn hour(&self) -> i32 {
        ((self.payload >> HOUR_OFFSET) & HOUR_MASK) as i32
    }

    pub fn minute(&self) -> i32 {
        ((self.payload >> MINUTE_OFFSET) & MINUTE_MASK) as i32
    }

    pub fn second(&self) -> i32 {
        ((self.payload >> SECOND_OFFSET) & SECOND_MASK) as i32
    }

    pub fn utc_offset_in_minute(&self) -> i32 {
        let value = (self.payload << (64 - UTC_OFFSET_IN_MINUTE_WIDTH - UTC_OFFSET_IN_MINUTE_OFFSET)) as i64;
        (value >> (64 - UTC_OFFSET_IN_MINUTE_WIDTH)) as i32
    }

    pub fn is_dst(&self) -> bool {
        self.payload & IS_DST_MASK != 0
    }

    /// `explicit operator bool`: o `monthDay` nunca é zero numa data real, então só esses bits dizem se
    /// o valor guarda uma. Os demais bits distinguem "nunca calculado" (payload zerado) de "calculado
    /// para um valor de tempo que mudou" (o marcador de obsoleto).
    pub fn is_valid(&self) -> bool {
        self.payload & (MONTH_DAY_MASK << MONTH_DAY_OFFSET) != 0
    }

    /// `staleMarker()`.
    pub const fn stale_marker() -> PlainGregorianDateTime {
        PlainGregorianDateTime { payload: 1 }
    }

    /// `hasNeverBeenComputed()`.
    pub fn has_never_been_computed(&self) -> bool {
        self.payload == 0
    }

    pub fn payload(&self) -> u64 {
        self.payload
    }
}

// ---------------------------------------------------------------------------------------------
// Fonte do fuso
// ---------------------------------------------------------------------------------------------

/// O que o `DateCache` pede ao fuso do processo no lugar do `UCalendar`.
pub trait TimeZoneSource {
    /// Deslocamento combinado (UTC mais horário de verão) no instante UTC `utc_ms`
    /// (`UCAL_ZONE_OFFSET` mais `UCAL_DST_OFFSET`). `None` é a falha do ICU.
    fn utc_offset(&self, utc_ms: f64) -> Option<LocalTimeOffset>;

    /// Deslocamento combinado de um horário local `local_ms`, com lacunas e sobreposições resolvidas pelo
    /// deslocamento de antes da transição (`UCAL_TZ_LOCAL_FORMER`).
    fn local_offset(&self, local_ms: f64) -> Option<LocalTimeOffset>;

    /// `ucal_getTimeZoneDisplayName(UCAL_STANDARD ou UCAL_DST)`: o campo entre parênteses do
    /// `toString()`. Vazio quando o fuso não tem nome.
    fn time_zone_display_name(&self, is_dst: bool) -> String;

    /// `WTF::lastTimeZoneID()`: muda quando o fuso do processo muda.
    fn generation(&self) -> u64 {
        0
    }
}

/// UTC sem horário de verão: o fuso do sandbox sem `TZ`.
#[derive(Clone, Copy, Debug, Default)]
pub struct UtcTimeZone;

impl TimeZoneSource for UtcTimeZone {
    fn utc_offset(&self, _utc_ms: f64) -> Option<LocalTimeOffset> {
        Some(LocalTimeOffset::new(false, 0))
    }

    fn local_offset(&self, _local_ms: f64) -> Option<LocalTimeOffset> {
        Some(LocalTimeOffset::new(false, 0))
    }

    fn time_zone_display_name(&self, _is_dst: bool) -> String {
        "Coordinated Universal Time".to_string()
    }
}

// ---------------------------------------------------------------------------------------------
// DateCache
// ---------------------------------------------------------------------------------------------

/// `struct LocalTimeOffsetCache`.
#[derive(Clone, Copy, Debug)]
struct LocalTimeOffsetCache {
    offset: LocalTimeOffset,
    start: i64,
    end: i64,
    epoch: u64,
}

impl Default for LocalTimeOffsetCache {
    fn default() -> LocalTimeOffsetCache {
        LocalTimeOffsetCache {
            offset: LocalTimeOffset::default(),
            start: i64ms::MAX_ECMASCRIPT_TIME,
            end: i64ms::MIN_ECMASCRIPT_TIME,
            epoch: 0,
        }
    }
}

impl LocalTimeOffsetCache {
    fn is_empty(&self) -> bool {
        self.start > self.end
    }
}

const DST_CACHE_SIZE: usize = 32;
/// The implementation relies on the fact that no time zones have more than one daylight savings
/// offset change per 19 days. In Egypt in 2010 they decided to suspend DST during Ramadan. This led to
/// a short interval where DST is in effect from September 10 to September 30.
const DEFAULT_DST_DELTA_IN_MILLISECONDS: i64 = 19 * i64ms::SECONDS_PER_DAY * 1000;

/// `class DateCache::DSTCache`: `m_before` e `m_after` são índices de `entries`.
struct DstCache {
    epoch: u64,
    entries: [LocalTimeOffsetCache; DST_CACHE_SIZE],
    before: usize,
    after: usize,
}

impl DstCache {
    fn new() -> DstCache {
        DstCache { epoch: 0, entries: [LocalTimeOffsetCache::default(); DST_CACHE_SIZE], before: 0, after: 1 }
    }

    fn bump_epoch(&mut self) -> u64 {
        self.epoch += 1;
        self.epoch
    }

    fn reset(&mut self) {
        self.entries = [LocalTimeOffsetCache::default(); DST_CACHE_SIZE];
        self.before = 0;
        self.after = 1;
        self.epoch = 0;
    }

    fn least_recently_used(&mut self, exclude: Option<usize>) -> usize {
        let mut result: Option<usize> = None;
        for index in 0..DST_CACHE_SIZE {
            if Some(index) == exclude {
                continue;
            }
            match result {
                None => result = Some(index),
                Some(current) => {
                    if self.entries[current].epoch > self.entries[index].epoch {
                        result = Some(index);
                    }
                }
            }
        }
        let result = result.expect("DSTCache sem entradas");
        self.entries[result] = LocalTimeOffsetCache::default();
        result
    }

    fn probe(&mut self, milliseconds_from_epoch: i64) -> (usize, usize) {
        let mut before: Option<usize> = None;
        let mut after: Option<usize> = None;
        for index in 0..DST_CACHE_SIZE {
            let cache = self.entries[index];
            if cache.start <= milliseconds_from_epoch {
                if before.is_none_or(|b| self.entries[b].start < cache.start) {
                    before = Some(index);
                }
            } else if milliseconds_from_epoch < cache.end && after.is_none_or(|a| self.entries[a].end > cache.end) {
                after = Some(index);
            }
        }

        let before = match before {
            Some(index) => index,
            None => {
                if self.entries[self.before].is_empty() {
                    self.before
                } else {
                    self.least_recently_used(after)
                }
            }
        };
        let after = match after {
            Some(index) => index,
            None => {
                if self.entries[self.after].is_empty() && before != self.after {
                    self.after
                } else {
                    self.least_recently_used(Some(before))
                }
            }
        };

        self.before = before;
        self.after = after;
        (before, after)
    }

    fn extend_the_after_cache(&mut self, milliseconds_from_epoch: i64, offset: LocalTimeOffset) {
        let after = self.after;
        if self.entries[after].offset == offset
            && self.entries[after].start - DEFAULT_DST_DELTA_IN_MILLISECONDS <= milliseconds_from_epoch
            && milliseconds_from_epoch <= self.entries[after].end
        {
            // Extend the m_after cache.
            self.entries[after].start = milliseconds_from_epoch;
        } else {
            // The m_after cache is either invalid or starts too late.
            if !self.entries[after].is_empty() {
                // If the m_after cache is valid, replace it with a new cache.
                self.after = self.least_recently_used(Some(self.before));
            }
            let after = self.after;
            self.entries[after].start = milliseconds_from_epoch;
            self.entries[after].end = milliseconds_from_epoch;
            self.entries[after].offset = offset;
            self.entries[after].epoch = self.bump_epoch();
        }
    }

    fn local_time_offset(&mut self, source: &dyn TimeZoneSource, milliseconds_from_epoch: i64, input_time_type: TimeType) -> LocalTimeOffset {
        let mut milliseconds_from_epoch = milliseconds_from_epoch;
        if !(milliseconds_from_epoch >= i64ms::MIN_ECMASCRIPT_TIME && milliseconds_from_epoch <= i64ms::MAX_ECMASCRIPT_TIME) {
            // Adjust to equivalent time.
            milliseconds_from_epoch = equivalent_time(milliseconds_from_epoch);
        }

        if self.epoch > u32::MAX as u64 {
            self.reset();
        }

        // If the time fits in the cached interval in the last cache hit, return the cached offset.
        let before = self.before;
        if self.entries[before].start <= milliseconds_from_epoch && milliseconds_from_epoch <= self.entries[before].end {
            self.entries[before].epoch = self.bump_epoch();
            return self.entries[before].offset;
        }

        self.probe(milliseconds_from_epoch);

        let before = self.before;
        debug_assert!(self.entries[before].is_empty() || self.entries[before].start <= milliseconds_from_epoch);
        debug_assert!(self.entries[self.after].is_empty() || milliseconds_from_epoch < self.entries[self.after].start);

        if self.entries[before].is_empty() {
            // Cache miss! Compute the DST offset for the time and shrink the cache interval to only
            // contain the time. This allows fast repeated DST offset computations for the same time.
            let offset = calculate_local_time_offset(source, milliseconds_from_epoch as f64, input_time_type);
            self.entries[before].offset = offset;
            self.entries[before].start = milliseconds_from_epoch;
            self.entries[before].end = milliseconds_from_epoch;
            self.entries[before].epoch = self.bump_epoch();
            return offset;
        }

        // Cache hit! If the time fits in the cached interval, return the cached offset.
        if milliseconds_from_epoch <= self.entries[before].end {
            self.entries[before].epoch = self.bump_epoch();
            return self.entries[before].offset;
        }

        if (milliseconds_from_epoch - DEFAULT_DST_DELTA_IN_MILLISECONDS) > self.entries[before].end {
            let offset = calculate_local_time_offset(source, milliseconds_from_epoch as f64, input_time_type);
            self.extend_the_after_cache(milliseconds_from_epoch, offset);
            std::mem::swap(&mut self.before, &mut self.after);
            return offset;
        }

        self.entries[before].epoch = self.bump_epoch();

        // Check if m_after is invalid or starts too late. Note that start of invalid caches is
        // maxECMAScriptTime.
        let new_after_start = if self.entries[before].end < i64ms::MAX_ECMASCRIPT_TIME - DEFAULT_DST_DELTA_IN_MILLISECONDS {
            self.entries[before].end + DEFAULT_DST_DELTA_IN_MILLISECONDS
        } else {
            i64ms::MAX_ECMASCRIPT_TIME
        };
        if new_after_start <= self.entries[self.after].start {
            let offset = calculate_local_time_offset(source, new_after_start as f64, input_time_type);
            self.extend_the_after_cache(new_after_start, offset);
        } else {
            // Update the usage counter of m_after since it is going to be used.
            debug_assert!(!self.entries[self.after].is_empty());
            let after = self.after;
            self.entries[after].epoch = self.bump_epoch();
        }

        // Now the millisecondsFromEpoch is between m_before->end and m_after->start. Only one daylight
        // savings offset change can occur in this interval.
        let before = self.before;
        let after = self.after;
        if self.entries[before].offset == self.entries[after].offset {
            // Merge two caches if they have the same offset.
            self.entries[before].end = self.entries[after].end;
            self.entries[after] = LocalTimeOffsetCache::default();
            return self.entries[before].offset;
        }

        // Binary search for daylight savings offset change point, but give up if we don't find it in
        // five iterations.
        for i in (0..=4).rev() {
            let before = self.before;
            let after = self.after;
            let delta = self.entries[after].start - self.entries[before].end;
            let middle = if i == 0 { milliseconds_from_epoch } else { self.entries[before].end + delta / 2 };
            let offset = calculate_local_time_offset(source, middle as f64, input_time_type);
            if self.entries[before].offset == offset {
                self.entries[before].end = middle;
                if milliseconds_from_epoch <= self.entries[before].end {
                    return offset;
                }
            } else {
                debug_assert!(self.entries[after].offset == offset);
                self.entries[after].start = middle;
                if milliseconds_from_epoch >= self.entries[after].start {
                    // This swap helps the optimistic fast check in subsequent invocations.
                    std::mem::swap(&mut self.before, &mut self.after);
                    return offset;
                }
            }
        }

        LocalTimeOffset::default()
    }
}

/// `DateCache::calculateLocalTimeOffset(milliseconds, inputTimeType)`. Falha do fuso devolve
/// `{ false, 0 }`: o cálculo da parte sem fuso falha de qualquer jeito depois.
fn calculate_local_time_offset(source: &dyn TimeZoneSource, milliseconds_from_epoch: f64, input_time_type: TimeType) -> LocalTimeOffset {
    let result = if input_time_type != TimeType::LocalTime {
        source.utc_offset(milliseconds_from_epoch)
    } else {
        source.local_offset(milliseconds_from_epoch)
    };
    result.unwrap_or(LocalTimeOffset::new(false, 0))
}

/// `struct DateCache::YearMonthDayCache`.
#[derive(Clone, Copy, Debug)]
struct YearMonthDayCache {
    days: i32,
    year: i32,
    month: i32,
    day: i32,
}

const BROKEN_DOWN_CACHE_SIZE: usize = 8;

/// `class DateCache::BrokenDownDateCache`: a forma desmembrada de um valor de tempo, entre `Date`s.
#[derive(Clone, Copy)]
struct BrokenDownEntry {
    key: f64,
    value: PlainGregorianDateTime,
}

struct BrokenDownDateCache {
    entries: [BrokenDownEntry; BROKEN_DOWN_CACHE_SIZE],
}

impl BrokenDownDateCache {
    fn new() -> BrokenDownDateCache {
        BrokenDownDateCache { entries: [BrokenDownEntry { key: f64::NAN, value: PlainGregorianDateTime::default() }; BROKEN_DOWN_CACHE_SIZE] }
    }

    fn slot(milliseconds_from_epoch: f64) -> usize {
        let bits = milliseconds_from_epoch.to_bits();
        ((bits ^ (bits >> 32) ^ (bits >> 17)) as usize) & (BROKEN_DOWN_CACHE_SIZE - 1)
    }

    fn get(&self, milliseconds_from_epoch: f64) -> PlainGregorianDateTime {
        let entry = &self.entries[BrokenDownDateCache::slot(milliseconds_from_epoch)];
        if entry.key != milliseconds_from_epoch {
            return PlainGregorianDateTime::default();
        }
        entry.value
    }

    fn set(&mut self, milliseconds_from_epoch: f64, value: PlainGregorianDateTime) {
        self.entries[BrokenDownDateCache::slot(milliseconds_from_epoch)] = BrokenDownEntry { key: milliseconds_from_epoch, value };
    }

    fn reset(&mut self) {
        *self = BrokenDownDateCache::new();
    }
}

/// `enum class UseSharedCache : bool`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseSharedCache {
    No,
    Yes,
}

/// `class DateCache`.
pub struct DateCache {
    source: Box<dyn TimeZoneSource>,
    caches: RefCell<[DstCache; 2]>,
    year_month_day_cache: Cell<Option<YearMonthDayCache>>,
    cached_date_string: RefCell<WtfString>,
    cached_date_string_value: Cell<f64>,
    broken_down_date_caches: RefCell<[BrokenDownDateCache; 2]>,
    cached_time_zone_id: Cell<u64>,
    may_have_cached_local_gregorian_date_time: Cell<bool>,
    time_zone_standard_display_name_cache: RefCell<Option<String>>,
    time_zone_dst_display_name_cache: RefCell<Option<String>>,
}

impl DateCache {
    /// `DateCache()` com o fuso dado.
    pub fn new(source: Box<dyn TimeZoneSource>) -> DateCache {
        let generation = source.generation();
        DateCache {
            source,
            caches: RefCell::new([DstCache::new(), DstCache::new()]),
            year_month_day_cache: Cell::new(None),
            cached_date_string: RefCell::new(WtfString::default()),
            cached_date_string_value: Cell::new(f64::NAN),
            broken_down_date_caches: RefCell::new([BrokenDownDateCache::new(), BrokenDownDateCache::new()]),
            cached_time_zone_id: Cell::new(generation),
            may_have_cached_local_gregorian_date_time: Cell::new(false),
            time_zone_standard_display_name_cache: RefCell::new(None),
            time_zone_dst_display_name_cache: RefCell::new(None),
        }
    }

    /// Um `DateCache` em UTC.
    pub fn utc() -> DateCache {
        DateCache::new(Box::new(UtcTimeZone))
    }

    /// A geração do fuso de que os caches (e os desmembrados locais dos `DateInstance`) valem.
    pub fn time_zone_generation(&self) -> u64 {
        self.cached_time_zone_id.get()
    }

    /// `hasTimeZoneChange()`.
    pub fn has_time_zone_change(&self) -> bool {
        self.cached_time_zone_id.get() != self.source.generation()
    }

    /// `clearForTimeZoneChange()`. A varredura dos `DateInstance` vivos
    /// (`take_may_have_cached_local_gregorian_date_time` e `invalidate_cached_local_gregorian_date_time`)
    /// é de quem tem o heap.
    pub fn clear_for_time_zone_change(&self) {
        for cache in self.caches.borrow_mut().iter_mut() {
            cache.reset();
        }
        for cache in self.broken_down_date_caches.borrow_mut().iter_mut() {
            cache.reset();
        }
        self.year_month_day_cache.set(None);
        *self.cached_date_string.borrow_mut() = WtfString::default();
        self.cached_date_string_value.set(f64::NAN);
        *self.time_zone_standard_display_name_cache.borrow_mut() = None;
        *self.time_zone_dst_display_name_cache.borrow_mut() = None;
        self.cached_time_zone_id.set(self.source.generation());
    }

    /// `noteCachedLocalGregorianDateTime()`.
    pub fn note_cached_local_gregorian_date_time(&self) {
        self.may_have_cached_local_gregorian_date_time.set(true);
    }

    /// `takeMayHaveCachedLocalGregorianDateTime()`.
    pub fn take_may_have_cached_local_gregorian_date_time(&self) -> bool {
        self.may_have_cached_local_gregorian_date_time.replace(false)
    }

    /// `timeZoneDisplayName(isDST)`: vazio quando a fonte não tem nome.
    pub fn time_zone_display_name(&self, is_dst: bool) -> String {
        let cache = if is_dst { &self.time_zone_dst_display_name_cache } else { &self.time_zone_standard_display_name_cache };
        cache.borrow_mut().get_or_insert_with(|| self.source.time_zone_display_name(is_dst)).clone()
    }

    /// `localTimeOffset(millisecondsFromEpoch, inputTimeType)`.
    pub fn local_time_offset(&self, milliseconds_from_epoch: i64, input_time_type: TimeType) -> LocalTimeOffset {
        self.caches.borrow_mut()[input_time_type as usize].local_time_offset(&*self.source, milliseconds_from_epoch, input_time_type)
    }

    /// `gregorianDateTimeToMS(year, month, monthDay, hour, minute, second, milliseconds, inputTimeType)`.
    pub fn gregorian_date_time_to_ms(
        &self,
        year: i32,
        month: i32,
        month_day: i32,
        hour: i32,
        minute: i32,
        second: i32,
        milliseconds: f64,
        input_time_type: TimeType,
    ) -> f64 {
        let day = date_math::date_to_days_from_1970(year, month, month_day);
        let ms = date_math::time_to_ms(hour as f64, minute as f64, second as f64, milliseconds);
        let local_time_result = (day * date_math::MS_PER_DAY) + ms;

        if input_time_type == TimeType::LocalTime && local_time_result.is_finite() {
            return local_time_result - self.local_time_offset(local_time_result as i64, input_time_type).offset as f64;
        }
        local_time_result
    }

    /// `localTimeToMS(milliseconds, inputTimeType)`.
    pub fn local_time_to_ms(&self, milliseconds: f64, input_time_type: TimeType) -> f64 {
        if input_time_type == TimeType::LocalTime && milliseconds.is_finite() {
            return milliseconds - self.local_time_offset(milliseconds as i64, input_time_type).offset as f64;
        }
        milliseconds
    }

    /// `yearMonthDayFromDaysWithCache(days)`.
    fn year_month_day_from_days_with_cache(&self, days: i32) -> (i32, i32, i32) {
        if let Some(cached) = self.year_month_day_cache.get() {
            // Check conservatively if the given 'days' has the same year and month as the cached 'days'.
            let new_day = cached.day + (days - cached.days);
            if (1..=28).contains(&new_day) {
                self.year_month_day_cache.set(Some(YearMonthDayCache { days, year: cached.year, month: cached.month, day: new_day }));
                return (cached.year, cached.month, new_day);
            }
        }
        let (year, month, day) = year_month_day_from_days(days);
        self.year_month_day_cache.set(Some(YearMonthDayCache { days, year, month, day }));
        (year, month, day)
    }

    /// `computeGregorianDateTime(millisecondsFromEpoch, outputTimeType)`: a entrada é UTC.
    fn compute_gregorian_date_time(&self, milliseconds_from_epoch: f64, output_time_type: TimeType) -> PlainGregorianDateTime {
        let mut milliseconds_from_epoch = milliseconds_from_epoch;
        let mut local_time = LocalTimeOffset::default();
        if output_time_type == TimeType::LocalTime && milliseconds_from_epoch.is_finite() {
            local_time = self.local_time_offset(milliseconds_from_epoch as i64, TimeType::UTCTime);
            milliseconds_from_epoch += local_time.offset as f64;
        }
        if !milliseconds_from_epoch.is_finite() {
            return PlainGregorianDateTime::default();
        }

        let time_clipped = milliseconds_from_epoch as i64;
        let days = ms_to_days(time_clipped);
        let time_in_day_ms = time_in_day(time_clipped, days);
        let (year, month, day) = self.year_month_day_from_days_with_cache(days);
        let hour = time_in_day_ms / (60 * 60 * 1000);
        let minute = (time_in_day_ms / (60 * 1000)) % 60;
        let second = (time_in_day_ms / 1000) % 60;
        PlainGregorianDateTime::new(
            year,
            month,
            day,
            week_day(days),
            hour,
            minute,
            second,
            (local_time.offset as i64 / i64ms::MS_PER_MINUTE) as i32,
            local_time.is_dst,
        )
    }

    /// `msToGregorianDateTime(millisecondsFromEpoch, outputTimeType, useSharedCache)`.
    pub fn ms_to_gregorian_date_time(
        &self,
        milliseconds_from_epoch: f64,
        output_time_type: TimeType,
        use_shared_cache: UseSharedCache,
    ) -> PlainGregorianDateTime {
        if use_shared_cache == UseSharedCache::No {
            return self.compute_gregorian_date_time(milliseconds_from_epoch, output_time_type);
        }

        let cached = self.broken_down_date_caches.borrow()[output_time_type as usize].get(milliseconds_from_epoch);
        if cached.is_valid() {
            return cached;
        }

        let result = self.compute_gregorian_date_time(milliseconds_from_epoch, output_time_type);
        if result.is_valid() {
            self.broken_down_date_caches.borrow_mut()[output_time_type as usize].set(milliseconds_from_epoch, result);
        }
        result
    }

    /// `parseDate(globalObject, vm, date)`. O `Err` é o `throwOutOfMemoryError` da conversão para UTF-8.
    pub fn parse_date(&self, date: &WtfString) -> Result<f64, PutError> {
        if *date == *self.cached_date_string.borrow() {
            return Ok(self.cached_date_string_value.get());
        }

        // V8's date parser (and useful web compat) treats every ECMAScript WhiteSpace code point as a
        // separator: TAB/VT/FF/SP, NBSP, BOM, and every Unicode Zs character including the
        // narrowNoBreakSpace that ICU >= 72 emits from toLocaleString. The parsers scan the UTF-8
        // encoding one byte at a time, so fold all of those into ASCII spaces here. Line terminators
        // (LS/PS) are intentionally left alone so they continue to reject, matching V8.
        let mut updated_string = date.clone();
        if !date.contains_only_ascii() {
            if date.is_8bit() {
                let replaced: Vec<u8> = date.span8().iter().map(|&c| if c == 0xA0 { b' ' } else { c }).collect();
                updated_string = WtfString::from_latin1(&replaced);
            } else {
                let replaced: Vec<u16> = date
                    .span16()
                    .iter()
                    .map(|&c| if crate::parser::lexer::Lexer::<u16>::is_white_space(c) { b' ' as u16 } else { c })
                    .collect();
                updated_string = WtfString::from_utf16(&replaced);
            }
        }

        let bytes = match updated_string.try_get_utf8(ConversionMode::LenientConversion) {
            Ok(bytes) => bytes,
            Err(error) => {
                if error == UTF8ConversionError::OutOfMemory {
                    return Err(PutError::OutOfMemory);
                }
                // https://tc39.github.io/ecma262/#sec-date-objects section 20.3.3.2 states that:
                // "Unrecognizable Strings or dates containing illegal element values in the format
                // String shall cause Date.parse to return NaN."
                return Ok(f64::NAN);
            }
        };

        // `parseDateImpl` (JSDateMath.cpp:439).
        let value = if Options::use_v8_date_parser() {
            let mut local = false;
            let mut value = js_date_math_v8::parser::parse_date_time_string(&bytes, &mut local);

            if local {
                value -= self.local_time_offset(value as i64, TimeType::LocalTime).offset as f64;
            }

            js_date_math_v8::parser::time_clip(value)
        } else {
            let (mut value, mut is_local_time) = date_math::parse_es5_date(&bytes);
            if value.is_nan() {
                (value, is_local_time) = date_math::parse_date(&bytes);
            }

            if is_local_time && value.is_finite() {
                value -= self.local_time_offset(value as i64, TimeType::LocalTime).offset as f64;
            }
            value
        };

        *self.cached_date_string.borrow_mut() = date.clone();
        self.cached_date_string_value.set(value);
        Ok(value)
    }
}

/// `isUTCEquivalent(timeZone)`.
pub fn is_utc_equivalent(time_zone: &str) -> bool {
    time_zone == "Etc/UTC" || time_zone == "Etc/GMT" || time_zone == "GMT"
}

/// `isNonIANA(timeZone)`: os nomes de três letras que o ICU conhece e a IANA não.
pub fn is_non_iana(time_zone: &str) -> bool {
    matches!(
        time_zone,
        "ACT" | "AET" | "AGT" | "ART" | "AST" | "BET" | "BST" | "CAT" | "CNT" | "CST" | "CTT" | "EAT" | "ECT" | "IET" | "IST" | "JST"
            | "MIT" | "NET" | "NST" | "PLT" | "PNT" | "PRT" | "PST" | "SST" | "VST"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UTC-3 fixo, sem horário de verão.
    struct Minus3;

    impl TimeZoneSource for Minus3 {
        fn utc_offset(&self, _utc_ms: f64) -> Option<LocalTimeOffset> {
            Some(LocalTimeOffset::new(false, -3 * 3_600_000))
        }

        fn local_offset(&self, _local_ms: f64) -> Option<LocalTimeOffset> {
            Some(LocalTimeOffset::new(false, -3 * 3_600_000))
        }

        fn time_zone_display_name(&self, _is_dst: bool) -> String {
            "Brasilia Standard Time".to_string()
        }
    }

    #[test]
    fn plain_gregorian_roundtrip() {
        let t = PlainGregorianDateTime::new(-271821, 3, 20, 6, 23, 59, 58, -180, true);
        assert_eq!((t.year(), t.month(), t.month_day(), t.week_day()), (-271821, 3, 20, 6));
        assert_eq!((t.hour(), t.minute(), t.second(), t.utc_offset_in_minute(), t.is_dst()), (23, 59, 58, -180, true));
        assert!(t.is_valid());
        assert!(!PlainGregorianDateTime::default().is_valid());
        assert!(!PlainGregorianDateTime::stale_marker().is_valid());
    }

    #[test]
    fn breaks_down_utc_and_local() {
        let cache = DateCache::new(Box::new(Minus3));
        let utc = cache.ms_to_gregorian_date_time(0.0, TimeType::UTCTime, UseSharedCache::No);
        assert_eq!((utc.year(), utc.month(), utc.month_day(), utc.hour(), utc.week_day()), (1970, 0, 1, 0, 4));
        let local = cache.ms_to_gregorian_date_time(0.0, TimeType::LocalTime, UseSharedCache::Yes);
        assert_eq!((local.year(), local.month(), local.month_day(), local.hour(), local.utc_offset_in_minute()), (1969, 11, 31, 21, -180));
        assert_eq!(cache.gregorian_date_time_to_ms(1970, 0, 1, 0, 0, 0, 0.0, TimeType::LocalTime), 3.0 * 3_600_000.0);
    }

    #[test]
    fn parses_with_local_offset() {
        let cache = DateCache::new(Box::new(Minus3));
        let value = cache.parse_date(&WtfString::from_latin1(b"1970-01-01T00:00:00")).unwrap();
        assert_eq!(value, 3.0 * 3_600_000.0);
        let value = cache.parse_date(&WtfString::from_latin1(b"1970-01-01T00:00:00Z")).unwrap();
        assert_eq!(value, 0.0);
    }
}
