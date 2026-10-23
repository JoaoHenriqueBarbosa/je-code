//! Porte de `src/vs/base/common/date.ts`: tempo de expiração, `fromNow`, `fromNowByDay`,
//! `getDurationString`, `toLocalISOString` e o `safeIntl`.
//!
//! Um `Date` do JavaScript é o valor de tempo em milissegundos (`f64`, como `getTime()`). Onde o original
//! usa o fuso local (`setHours`, `getFullYear`...), a função recebe o fuso explicitamente
//! ([`jiff::tz::TimeZone`]); [`local_time_zone`] devolve o do sistema, o do `new Date()`. O `Date.now()` é
//! [`runtime::date_now`], que acompanha o relógio virtual dos testes.
//!
//! `safeIntl` cobre `Collator` e `Locale`, que o porte já tem ([`crate::collator`] e a `Locale` do ICU4X).
//! Pendente: `DateTimeFormat` (precisa do `icu_datetime`), `Segmenter` e `NumberFormat`, que nenhum teste
//! de `vs/base/common` exercita.

use std::rc::Rc;

use icu_locale_core::Locale;
use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::collator::{Collator, CollatorOptions};
use crate::dynamic::number_to_js_string;
use crate::lazy::Lazy;
use crate::nls::localize;
use crate::platform::LANGUAGE_DEFAULT;
use crate::runtime;

const MINUTE: f64 = 60.0;
const HOUR: f64 = MINUTE * 60.0;
const DAY: f64 = HOUR * 24.0;
const WEEK: f64 = DAY * 7.0;
const MONTH: f64 = DAY * 30.0;
const YEAR: f64 = DAY * 365.0;

/// O fuso local do sistema (o do `new Date()`).
pub fn local_time_zone() -> TimeZone {
    TimeZone::system()
}

/// `Math.round`: arredonda para o inteiro mais próximo, com o meio indo para cima.
fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

fn now_ms() -> f64 {
    runtime::date_now() as f64
}

/// Returns the absolute expiration time for a relative lifetime in seconds.
pub fn get_expiration_time(expires_in: Option<f64>, now: Option<f64>) -> Option<f64> {
    expires_in.map(|expires_in| now.unwrap_or_else(now_ms) + expires_in * 1000.0)
}

/// Returns whether a known expiration time has passed.
pub fn is_expired(expires_at: Option<f64>, now: Option<f64>) -> bool {
    expires_at.is_some_and(|expires_at| expires_at <= now.unwrap_or_else(now_ms))
}

/// Returns the positive number of whole seconds until `expiresAt`, rounded up.
pub fn get_remaining_time_in_seconds(expires_at: Option<f64>, now: Option<f64>) -> Option<f64> {
    let expires_at = expires_at?;
    let remaining = ((expires_at - now.unwrap_or_else(now_ms)) / 1000.0).ceil();
    (remaining > 0.0).then_some(remaining)
}

/// Uma unidade de `fromNow`: até onde ela vale, por quanto divide os segundos, o nome na chave de
/// tradução e as palavras (abreviada e completa, no singular e no plural). Os dias não têm forma
/// abreviada, então não têm o sufixo `.fullWord` na chave.
struct Unit {
    limit: f64,
    divisor: f64,
    name: &'static str,
    short: [&'static str; 2],
    full: [&'static str; 2],
    has_full_word_key: bool,
}

const UNITS: [Unit; 7] = [
    Unit { limit: MINUTE, divisor: 1.0, name: "seconds", short: ["sec", "secs"], full: ["second", "seconds"], has_full_word_key: true },
    Unit { limit: HOUR, divisor: MINUTE, name: "minutes", short: ["min", "mins"], full: ["minute", "minutes"], has_full_word_key: true },
    Unit { limit: DAY, divisor: HOUR, name: "hours", short: ["hr", "hrs"], full: ["hour", "hours"], has_full_word_key: true },
    Unit { limit: WEEK, divisor: DAY, name: "days", short: ["day", "days"], full: ["day", "days"], has_full_word_key: false },
    Unit { limit: MONTH, divisor: WEEK, name: "weeks", short: ["wk", "wks"], full: ["week", "weeks"], has_full_word_key: true },
    Unit { limit: YEAR, divisor: MONTH, name: "months", short: ["mo", "mos"], full: ["month", "months"], has_full_word_key: true },
    Unit { limit: f64::INFINITY, divisor: YEAR, name: "years", short: ["yr", "yrs"], full: ["year", "years"], has_full_word_key: true },
];

impl Unit {
    fn localize(&self, value: f64, append_ago_label: bool, use_full_time_words: bool) -> String {
        let plural = usize::from(value != 1.0);
        let full = use_full_time_words && self.has_full_word_key;
        let word = if use_full_time_words { self.full[plural] } else { self.short[plural] };
        let key = format!(
            "date.fromNow.{}.{}{}{}",
            self.name,
            if plural == 0 { "singular" } else { "plural" },
            if append_ago_label { ".ago" } else { "" },
            if full { ".fullWord" } else { "" },
        );
        let message = format!("{{0}} {word}{}", if append_ago_label { " ago" } else { "" });
        localize(&key, &message, &[&number_to_js_string(value)])
    }
}

/// Create a localized difference of the time between now and the specified date.
///
/// `date` é `None` para o `undefined` do original. `append_ago_label` adiciona o " ago",
/// `use_full_time_words` usa palavras inteiras (seconds) no lugar das abreviadas (secs) e
/// `disallow_now` impede o texto "now" para diferenças abaixo de 30 segundos.
pub fn from_now(date: Option<f64>, append_ago_label: bool, use_full_time_words: bool, disallow_now: bool) -> String {
    let Some(date) = date else {
        return localize("date.fromNow.unknown", "unknown", &[]);
    };

    let seconds = js_round((now_ms() - date) / 1000.0);
    if seconds < -30.0 {
        let relative = from_now(Some(now_ms() + seconds * 1000.0), false, use_full_time_words, disallow_now);
        return localize("date.fromNow.in", "in {0}", &[&relative]);
    }

    if !disallow_now && seconds < 30.0 {
        return localize("date.fromNow.now", "now", &[]);
    }

    let unit = UNITS.iter().find(|unit| seconds < unit.limit).expect("a última unidade não tem limite");
    // Os segundos usam o valor sem arredondar de novo; as demais unidades arredondam a divisão.
    let value = if unit.divisor == 1.0 { seconds } else { js_round(seconds / unit.divisor) };
    unit.localize(value, append_ago_label, use_full_time_words)
}

/// `fromNowByDay`: "Today" e "Yesterday" (no fuso `zone`) e, antes disso, [`from_now`].
pub fn from_now_by_day(date: f64, append_ago_label: bool, use_full_time_words: bool, zone: &TimeZone) -> String {
    let now = Timestamp::from_millisecond(now_ms() as i64).expect("Date.now() cabe no intervalo do jiff");
    let today_midnight = now.to_zoned(zone.clone()).start_of_day().expect("início do dia existe");
    let yesterday_midnight = today_midnight
        .yesterday()
        .and_then(|day| day.start_of_day())
        .expect("início do dia anterior existe");

    if date > today_midnight.timestamp().as_millisecond() as f64 {
        return localize("today", "Today", &[]);
    }

    if date > yesterday_midnight.timestamp().as_millisecond() as f64 {
        return localize("yesterday", "Yesterday", &[]);
    }

    from_now(Some(date), append_ago_label, use_full_time_words, false)
}

/// Gets a readable duration with intelligent/lossy precision. For example "40ms" or "3.040s")
/// `ms` é a duração em milissegundos e `use_full_time_words` escolhe palavras inteiras (seconds) no lugar
/// das abreviadas (secs).
pub fn get_duration_string(ms: f64, use_full_time_words: bool) -> String {
    let seconds = (ms / 1000.0).abs();
    let (key, message, value) = if seconds < 1.0 {
        if use_full_time_words {
            ("duration.ms.full", "{0} milliseconds", ms)
        } else {
            ("duration.ms", "{0}ms", ms)
        }
    } else if seconds < MINUTE {
        let value = js_round(ms) / 1000.0;
        if use_full_time_words { ("duration.s.full", "{0} seconds", value) } else { ("duration.s", "{0}s", value) }
    } else if seconds < HOUR {
        let value = js_round(ms / (1000.0 * MINUTE));
        if use_full_time_words { ("duration.m.full", "{0} minutes", value) } else { ("duration.m", "{0} mins", value) }
    } else if seconds < DAY {
        let value = js_round(ms / (1000.0 * HOUR));
        if use_full_time_words { ("duration.h.full", "{0} hours", value) } else { ("duration.h", "{0} hrs", value) }
    } else {
        ("duration.d", "{0} days", js_round(ms / (1000.0 * DAY)))
    };
    localize(key, message, &[&number_to_js_string(value)])
}

/// `toLocalISOString`: a data no fuso `zone` no formato ISO, com o `Z` final do original (que não é
/// UTC; é o formato que o VS Code usa nos nomes de log).
pub fn to_local_iso_string(date: f64, zone: &TimeZone) -> String {
    let zoned = Timestamp::from_millisecond(date as i64).expect("valor de tempo válido").to_zoned(zone.clone());
    format!(
        "{}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        zoned.year(),
        zoned.month(),
        zoned.day(),
        zoned.hour(),
        zoned.minute(),
        zoned.second(),
        zoned.millisecond(),
    )
}

/// `safeIntl`: o `Intl` que cai na locale padrão quando a tag é inválida em vez de lançar `RangeError`.
pub mod safe_intl {
    use super::*;

    /// `safeIntl.Collator(locales, options)`. A queda para a locale padrão de uma tag inválida é do
    /// próprio [`Collator::new`].
    pub fn collator(locale: Option<&str>, options: CollatorOptions) -> Lazy<Rc<Collator>> {
        let locale = locale.map(str::to_owned);
        Lazy::new(move || Rc::new(Collator::new(locale.as_deref(), options)))
    }

    /// `safeIntl.Locale(tag)`. Uma tag que a `Intl.Locale` rejeita (inclusive com `_` no lugar do `-`,
    /// que o ICU4X aceitaria) vira a locale de [`LANGUAGE_DEFAULT`].
    pub fn locale(tag: &str) -> Lazy<Locale> {
        let tag = tag.to_owned();
        Lazy::new(move || {
            let parsed = if tag.contains('_') { None } else { tag.parse::<Locale>().ok() };
            parsed.unwrap_or_else(|| LANGUAGE_DEFAULT.parse().expect("a locale padrão é uma tag válida"))
        })
    }

    /// `Intl.Locale#baseName`: idioma, script, região e variantes, sem extensões.
    pub fn base_name(locale: &Locale) -> String {
        locale.id.to_string()
    }
}
