//! O `TimeZoneSource` do `DateCache` sobre o fuso do processo: `ul_common::time::zone` (jiff, a
//! tzdata embutida) resolve a variável `TZ` como o glibc do Debian (`TZ` do ambiente do processo, vazio
//! ou `UTC` é UTC, `:` inicial ignorado, caminho absoluto ou nome em `/usr/share/zoneinfo`, regra
//! POSIX; valor que não resolve vira UTC), e este adaptador responde ao que o `DateCache` pergunta no
//! lugar do `UCalendar` do ICU.
//!
//! DIVERGÊNCIAS:
//!
//! - O fuso vale o `TZ` do momento da pergunta: `set_time_zone_spec_override` (o gancho do hospedeiro, como
//!   o `Emulation.setTimezoneOverride` do CDP) incrementa `generation()`, e a fonte resolve de novo na
//!   próxima pergunta. O `VM::date_cache` nota a troca e chama `DateCache::clear_for_time_zone_change`;
//!   o `DateInstance` carimba o desmembrado local com a geração e o refaz quando ela muda (no JSC é a
//!   varredura dos `DateInstance` vivos). A troca de `process.env.TZ` do Bun ainda não chama o gancho.
//! - Instante fora da faixa do jiff (anos de -9999 a 9999) usa o instante equivalente
//!   (`equivalentTime`, ECMA 262 15.9.1.9): mesmo dia da semana e mesma bissextilidade, então a regra
//!   POSIX de horário de verão do futuro dá o mesmo deslocamento que o ICU daria.
//! - `time_zone_display_name` pede o nome longo em inglês do CLDR ("Brasilia Standard Time"), que o
//!   jiff não tem: vem da tabela de metafusos de `time_zone_names.rs`, e o fuso que ela não lista sai
//!   no formato GMT localizado do ICU (`GMT-03:00`). FALTA o CLDR inteiro para igualar o Debian em
//!   todo fuso.
//! - Gancho de teste: `set_time_zone_spec_override` fixa o `TZ` da thread, porque o motor sozinho
//!   (sem pseudo-processo instalado) resolve UTC.

use std::cell::{Cell, RefCell};

use jiff::tz::{AmbiguousOffset, TimeZone};
use jiff::Timestamp;

use crate::runtime::js_date_math::TimeZoneSource;
use crate::runtime::time_zone_names;
use crate::wtf::date_math::{equivalent_time, int64_milliseconds as i64ms, LocalTimeOffset};

/// O fuso do processo (`ul_common::time::zone::local()`).
pub struct ProcessTimeZone {
    zone: RefCell<TimeZone>,
    /// A geração de `TIME_ZONE_GENERATION` em que `zone` foi resolvido.
    resolved: Cell<u64>,
    /// Um fuso fixo (`with_zone`): não relê o override.
    fixed: bool,
}

thread_local! {
    /// O valor de `TZ` que vale nesta thread no lugar do ambiente do pseudo-processo (o gancho dos
    /// testes: o motor sozinho não tem processo instalado, e `local()` sem ele resolve UTC).
    static TIME_ZONE_SPEC_OVERRIDE: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Sobe a cada `set_time_zone_spec_override`: é o `generation()` das fontes de fuso do processo.
    static TIME_ZONE_GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// Fixa o `TZ` (no formato da variável) do fuso desta thread, ou volta ao ambiente com `None`. Vale já para
/// o código que roda depois da chamada, com a página viva: o `DateCache` do `VM` limpa os caches na próxima
/// leitura e o `Intl` relê o fuso a cada pergunta.
pub fn set_time_zone_spec_override(spec: Option<&str>) {
    TIME_ZONE_SPEC_OVERRIDE.with(|cell| *cell.borrow_mut() = spec.map(str::to_string));
    TIME_ZONE_GENERATION.with(|generation| generation.set(generation.get() + 1));
}

/// O fuso do processo, respeitando o gancho de teste de `TZ` (o mesmo que o `DateCache` usa e, por isso,
/// o padrão do `Intl.DateTimeFormat` e do `toLocaleString`).
pub fn process_zone() -> TimeZone {
    TIME_ZONE_SPEC_OVERRIDE
        .with(|cell| cell.borrow().as_deref().map(|spec| ul_common::time::zone::from_spec(spec.as_bytes())))
        .unwrap_or_else(ul_common::time::zone::local)
}

impl ProcessTimeZone {
    /// Resolve `TZ` agora.
    pub fn new() -> ProcessTimeZone {
        ProcessTimeZone { zone: RefCell::new(process_zone()), resolved: Cell::new(TIME_ZONE_GENERATION.with(Cell::get)), fixed: false }
    }

    /// Um fuso já resolvido (os testes).
    pub fn with_zone(zone: TimeZone) -> ProcessTimeZone {
        ProcessTimeZone { zone: RefCell::new(zone), resolved: Cell::new(0), fixed: true }
    }

    /// O fuso em vigor: resolve de novo quando o override mudou desde a última pergunta.
    fn zone(&self) -> TimeZone {
        let generation = self.generation();
        if !self.fixed && self.resolved.get() != generation {
            *self.zone.borrow_mut() = process_zone();
            self.resolved.set(generation);
        }
        self.zone.borrow().clone()
    }

    /// O identificador do fuso (`DateCache::defaultTimeZone().toString()`): o nome IANA, ou `UTC` quando o
    /// fuso não tem um (regra POSIX de `TZ`, caminho de arquivo sem nome da base).
    pub fn time_zone_id(&self) -> String {
        self.zone().iana_name().unwrap_or("UTC").to_string()
    }
}

impl Default for ProcessTimeZone {
    fn default() -> ProcessTimeZone {
        ProcessTimeZone::new()
    }
}

/// O `Timestamp` do instante, trazido para a faixa do jiff pelo instante equivalente se preciso.
fn timestamp_in_range(milliseconds: f64) -> Option<Timestamp> {
    if !milliseconds.is_finite() {
        return None;
    }
    let milliseconds = milliseconds as i64;
    // Antes da faixa do jiff o ICU usa o deslocamento inicial do fuso (o LMT, como -03:06:28 em São Paulo no
    // ano -271821), não o de um ano equivalente. Um dia de folga sobre o mínimo deixa o `local_offset` subtrair o
    // deslocamento sem sair da faixa. Depois da faixa vale a regra final, que o instante equivalente reproduz.
    let lowest = Timestamp::MIN.as_millisecond() + i64ms::MS_PER_DAY;
    if milliseconds < lowest {
        return Timestamp::from_millisecond(lowest).ok();
    }
    // Depois da faixa vale a regra final do fuso (o ICU não aplica as transições históricas). O instante
    // equivalente cai em 2008..2035, onde o São Paulo ainda tinha horário de verão; somar o ciclo de 28 anos
    // (10227 dias, sem virada de século até 2063) o leva a 2036..2063, depois do último dado histórico.
    const DAYS_IN_28_YEARS: i64 = 28 * 365 + 7;
    Timestamp::from_millisecond(milliseconds)
        .or_else(|_| Timestamp::from_millisecond(equivalent_time(milliseconds) + DAYS_IN_28_YEARS * i64ms::MS_PER_DAY))
        .ok()
}

/// O deslocamento em milissegundos cabe no `int` do `LocalTimeOffset`.
fn offset_in_milliseconds(seconds: i32) -> i32 {
    seconds * i64ms::MS_PER_SECOND as i32
}

/// Os segundos inteiros do instante, arredondados para baixo. O `Timestamp::as_second` do jiff trunca em direção
/// a zero: num instante anterior a 1970 com fração (`...:59.999`) ele daria o segundo seguinte, e o instante de
/// uma transição de horário de verão passaria para o lado errado dela. O ICU compara em milissegundos, que é o
/// mesmo que comparar o piso dos segundos com as transições, sempre em segundo inteiro.
fn floor_seconds(timestamp: Timestamp) -> i64 {
    timestamp.as_millisecond().div_euclid(i64ms::MS_PER_SECOND)
}

/// O deslocamento em segundos e o horário de verão do `zone` no instante `milliseconds`, pelas regras do ICU:
/// instantes fora da faixa do jiff valem o deslocamento inicial (antes) ou a regra final (depois). `None` só
/// para instante não finito.
pub fn zone_offset_at(zone: &TimeZone, milliseconds: f64) -> Option<(i32, bool)> {
    let timestamp = timestamp_in_range(milliseconds)?;
    let info = zone.to_offset_info(Timestamp::from_second(floor_seconds(timestamp)).ok()?);
    Some((info.offset().seconds(), info.dst().is_dst()))
}

impl TimeZoneSource for ProcessTimeZone {
    fn utc_offset(&self, utc_ms: f64) -> Option<LocalTimeOffset> {
        let (seconds, is_dst) = zone_offset_at(&self.zone(), utc_ms)?;
        Some(LocalTimeOffset::new(is_dst, offset_in_milliseconds(seconds)))
    }

    fn generation(&self) -> u64 {
        if self.fixed { 0 } else { TIME_ZONE_GENERATION.with(Cell::get) }
    }

    /// `UCAL_TZ_LOCAL_FORMER`: lacuna e sobreposição valem o deslocamento de antes da transição, que é
    /// o `compatible` do jiff nos dois casos.
    fn local_offset(&self, local_ms: f64) -> Option<LocalTimeOffset> {
        let as_if_utc = timestamp_in_range(local_ms)?;
        let zone = self.zone();
        let civil = as_if_utc.to_zoned(TimeZone::UTC).datetime();
        // O instante que o horário local teria com o deslocamento de ANTES da transição: na lacuna ele
        // cai antes da transição (`civil - after`), na sobreposição também (`civil - before`). O
        // `compatible` do jiff escolheria o instante de depois da transição na lacuna.
        let seconds = floor_seconds(as_if_utc);
        let former_seconds = match zone.to_ambiguous_timestamp(civil).offset() {
            AmbiguousOffset::Unambiguous { offset } => seconds - offset.seconds() as i64,
            AmbiguousOffset::Gap { after, .. } => seconds - after.seconds() as i64,
            AmbiguousOffset::Fold { before, .. } => seconds - before.seconds() as i64,
        };
        let info = zone.to_offset_info(Timestamp::from_second(former_seconds).ok()?);
        Some(LocalTimeOffset::new(info.dst().is_dst(), offset_in_milliseconds(info.offset().seconds())))
    }

    /// O nome longo do CLDR (`time_zone_names`); fuso sem nome na tabela (ou sem nome IANA) cai no
    /// formato GMT localizado do ICU, com o deslocamento padrão (ou o de verão) em vigor hoje.
    fn time_zone_display_name(&self, is_dst: bool) -> String {
        let zone = self.zone();
        if let Some(name) = zone.iana_name().and_then(|iana| time_zone_names::long_name(iana, is_dst)) {
            return name.to_string();
        }
        // 2024-01-15T12:00:00Z e 2024-07-15T12:00:00Z: um dos dois está fora do horário de verão.
        let samples = [1_705_320_000_i64, 1_721_044_800_i64].map(|seconds| {
            let info = zone.to_offset_info(Timestamp::from_second(seconds).unwrap_or(Timestamp::UNIX_EPOCH));
            (info.dst().is_dst(), info.offset().seconds())
        });
        let wanted = samples.iter().find(|(dst, _)| *dst == is_dst).or_else(|| samples.iter().find(|(dst, _)| !*dst));
        time_zone_names::localized_gmt(wanted.map_or(0, |(_, seconds)| *seconds))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_date_math::DateCache;
    use crate::wtf::date_math::{date_to_days_from_1970, TimeType};

    fn cache_for(name: &str) -> DateCache {
        let zone = ul_common::time::zone::from_spec(name.as_bytes());
        DateCache::new(Box::new(ProcessTimeZone::with_zone(zone)))
    }

    #[test]
    fn utc_has_the_icu_name() {
        let cache = cache_for("UTC");
        assert_eq!(cache.time_zone_display_name(false), "Coordinated Universal Time");
        assert_eq!(cache.local_time_offset(0, TimeType::UTCTime), LocalTimeOffset::new(false, 0));
    }

    #[test]
    fn sao_paulo_is_minus_three() {
        let cache = cache_for("America/Sao_Paulo");
        // 2026-01-15T12:00:00Z, sem horário de verão desde 2019.
        let offset = cache.local_time_offset(1_768_478_400_000, TimeType::UTCTime);
        assert_eq!(offset, LocalTimeOffset::new(false, -3 * 3_600_000));
        let local = cache.local_time_offset(1_768_478_400_000 - 3 * 3_600_000, TimeType::LocalTime);
        assert_eq!(local, LocalTimeOffset::new(false, -3 * 3_600_000));
    }

    #[test]
    fn new_york_gap_uses_the_offset_before_the_transition() {
        let cache = cache_for("America/New_York");
        // 2024-03-10T02:30 local não existe: vale o deslocamento de antes (-5h, horário padrão).
        let local_ms = 1_710_037_800_000_i64; // 2024-03-10T02:30:00 lido como se fosse UTC
        let offset = cache.local_time_offset(local_ms, TimeType::LocalTime);
        assert_eq!(offset, LocalTimeOffset::new(false, -5 * 3_600_000));
    }

    #[test]
    fn far_future_uses_the_equivalent_year() {
        let cache = cache_for("America/New_York");
        // Ano 20000, 15 de julho: horário de verão pela regra POSIX final.
        let utc_ms = (date_to_days_from_1970(20000, 6, 15) * crate::wtf::date_math::MS_PER_DAY) as i64;
        let offset = cache.local_time_offset(utc_ms, TimeType::UTCTime);
        assert!(offset.is_dst);
    }

    /// Avalia `source` no realm vivo e devolve o texto de `R`.
    fn read(global_object: &crate::runtime::js_global_object::JSGlobalObjectRef, source: &str) -> String {
        use crate::api::eval::{evaluate, program_source, read_global_result};
        let _ = evaluate(global_object, &program_source(source));
        let value = read_global_result(global_object, "R").unwrap_or_else(|_| panic!("leitura de R"));
        String::from_utf8_lossy(&value.to_wtf_string().utf8(crate::wtf::text::conversion_mode::ConversionMode::LenientConversion)).into_owned()
    }

    /// Medido no bun 1.4.2 com `TZ` trocado: a mesma `Date`, o `Intl` padrão e o `toString` seguem o fuso novo
    /// no realm que já estava vivo (o `Emulation.setTimezoneOverride` do CDP).
    #[test]
    fn override_changed_with_the_realm_alive_applies_at_once() {
        const SCRIPT: &str = "var d = globalThis.d || (globalThis.d = new Date(0)); \
            var R = [d.getHours(), d.getTimezoneOffset(), new Intl.DateTimeFormat().resolvedOptions().timeZone, d.toString(), \
            d.toLocaleString('en-US'), new Date(2026, 0, 15, 12).toISOString()].join('|')";
        crate::runtime::cell_registry::run_program(|| {
            set_time_zone_spec_override(Some("UTC"));
            let (vm, global_object) = crate::api::eval::new_global_object();
            let _finalizer = crate::api::eval::VmFinalizer::new(&vm);
            assert_eq!(
                read(&global_object, SCRIPT),
                "0|0|UTC|Thu Jan 01 1970 00:00:00 GMT+0000 (Coordinated Universal Time)|1/1/1970, 12:00:00 AM|2026-01-15T12:00:00.000Z"
            );
            set_time_zone_spec_override(Some("America/Sao_Paulo"));
            assert_eq!(
                read(&global_object, SCRIPT),
                "21|180|America/Sao_Paulo|Wed Dec 31 1969 21:00:00 GMT-0300 (Brasilia Standard Time)|12/31/1969, 9:00:00 PM|2026-01-15T15:00:00.000Z"
            );
            set_time_zone_spec_override(Some("Asia/Tokyo"));
            assert_eq!(
                read(&global_object, SCRIPT),
                "9|-540|Asia/Tokyo|Thu Jan 01 1970 09:00:00 GMT+0900 (Japan Standard Time)|1/1/1970, 9:00:00 AM|2026-01-15T03:00:00.000Z"
            );
            set_time_zone_spec_override(None);
        });
    }
}
