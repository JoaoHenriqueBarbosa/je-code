//! O `TimeClamper` do Blink (`third_party/blink/renderer/core/timing/time_clamper.cc`) e a conversão de instante
//! monotônico em `DOMHighResTimeStamp` de `Performance::MonotonicTimeToDOMHighResTimeStamp`. O JavaScriptCore e o
//! bun não arredondam o relógio; quem liga isto é o hospedeiro do navegador.
//!
//! - O instante (microssegundos inteiros, como o `base::TimeDelta`) cai no múltiplo de `resolution` de baixo ou no
//!   de cima: sobe quando os dígitos de baixo passam do limiar `MurmurHash3(múltiplo ^ segredo) % resolution`. O
//!   segredo é um `RandUint64` por processo, então o mesmo instante arredonda igual dentro da página e diferente
//!   entre páginas.
//! - A resolução é 100 µs, ou 5 µs com `crossOriginIsolated`.
//! - `performance.now()` e o `Event.timeStamp` clampam o instante monotônico e a origem em separado e subtraem os
//!   dois em milissegundos (`double`): como o instante é contado desde o boot, a subtração deixa o ruído de ponto
//!   flutuante que o Chromium mostra (`114.80000000447035`). O carimbo do quadro (rAF) clampa o delta direto e sai
//!   redondo (`115.1`). Medido no Chromium 151.

use std::cell::Cell;

/// A resolução sem isolamento de origem cruzada (`kCoarseResolutionMicroseconds`).
pub const COARSE_RESOLUTION_US: i64 = 100;
/// A resolução com `crossOriginIsolated` (`kFineResolutionMicroseconds`).
pub const FINE_RESOLUTION_US: i64 = 5;

#[derive(Clone, Copy)]
struct Clamper {
    secret: u64,
    resolution: i64,
    /// O instante monotônico (µs desde o boot) da origem do tempo da página.
    origin_ticks: i64,
    /// A época (µs) do instante zero do relógio monotônico (`GetUnixAtZeroMonotonic`).
    unix_at_zero: i64,
}

thread_local! {
    static CLAMPER: Cell<Option<Clamper>> = const { Cell::new(None) };
}

/// Liga o arredondamento do relógio da página.
pub fn set_time_clamper(secret: u64, resolution_us: i64, origin_ticks_us: i64, unix_at_zero_us: i64) {
    CLAMPER.with(|clamper| clamper.set(Some(Clamper { secret, resolution: resolution_us, origin_ticks: origin_ticks_us, unix_at_zero: unix_at_zero_us })));
}

/// Fim do programa (`cell_registry::reset_program_state`).
pub(crate) fn reset_for_program() {
    CLAMPER.with(|clamper| clamper.set(None));
}

/// `MurmurHash3` do `TimeClamper` (o finalizador `fmix64`).
fn murmur_hash3(mut value: u64) -> u64 {
    value ^= value >> 33;
    value = value.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    value ^= value >> 33;
    value = value.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    value ^= value >> 33;
    value
}

impl Clamper {
    /// `TimeClamper::ClampTimeResolution`, em microssegundos.
    fn clamp(&self, time: i64) -> i64 {
        let magnitude = time.unsigned_abs() as i64;
        let lower = magnitude % self.resolution;
        let mut clamped = magnitude - lower;
        let threshold = (murmur_hash3(clamped as u64 ^ self.secret) % self.resolution as u64) as i64;
        if lower >= threshold {
            clamped += self.resolution;
        }
        if time < 0 {
            -clamped
        } else {
            clamped
        }
    }

    /// `ClampTimeResolution(delta).InMillisecondsF()`.
    fn clamp_ms(&self, time: i64) -> f64 {
        self.clamp(time) as f64 / 1000.0
    }
}

fn micros(milliseconds: f64) -> i64 {
    (milliseconds * 1000.0).floor() as i64
}

/// `performance.now()` com o relógio arredondado, a partir dos milissegundos decorridos desde a origem; `None` com
/// o arredondamento desligado.
pub(crate) fn clamped_now_ms(elapsed_ms: f64) -> Option<f64> {
    let clamper = CLAMPER.with(Cell::get)?;
    let now = clamper.origin_ticks + micros(elapsed_ms);
    Some((clamper.clamp_ms(now) - clamper.clamp_ms(clamper.origin_ticks)).max(0.0))
}

/// `performance.timeOrigin` com o relógio arredondado: a época do zero monotônico mais a origem, cada um clampado.
pub(crate) fn clamped_time_origin_ms() -> Option<f64> {
    let clamper = CLAMPER.with(Cell::get)?;
    Some(clamper.clamp_ms(clamper.unix_at_zero) + clamper.clamp_ms(clamper.origin_ticks))
}

/// O carimbo de um instante contado desde a origem (o quadro do `requestAnimationFrame`): o delta clampado, sem a
/// subtração. O instante vem de uma grade em microssegundos inteiros, então arredonda (`16.666 * 3` é
/// `49.99799999999999`). Com o arredondamento desligado, o próprio valor.
pub fn clamp_delta_ms(elapsed_ms: f64) -> f64 {
    match CLAMPER.with(Cell::get) {
        Some(clamper) => clamper.clamp_ms((elapsed_ms * 1000.0).round() as i64),
        None => elapsed_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clamper(resolution: i64) -> Clamper {
        Clamper { secret: 0x0123_4567_89AB_CDEF, resolution, origin_ticks: 20_000_000_123, unix_at_zero: 1_700_000_000_000_000 }
    }

    #[test]
    fn clamps_to_the_resolution_and_never_moves_more_than_one_step() {
        for resolution in [COARSE_RESOLUTION_US, FINE_RESOLUTION_US] {
            let c = clamper(resolution);
            for time in (0..5000).map(|n| n * 7 + 1) {
                let clamped = c.clamp(time);
                assert_eq!(clamped % resolution, 0);
                assert!(clamped == time - time % resolution || clamped == time - time % resolution + resolution);
            }
            assert_eq!(c.clamp(-1234), -c.clamp(1234));
        }
    }

    /// O `now` é a diferença de dois instantes grandes clampados: o ruído de ponto flutuante aparece como no Chromium.
    #[test]
    fn now_subtracts_two_clamped_instants() {
        let c = clamper(COARSE_RESOLUTION_US);
        let now = c.clamp_ms(c.origin_ticks + 114_830) - c.clamp_ms(c.origin_ticks);
        assert!(((now * 10.0).round() / 10.0 - now).abs() < 1e-6);
        assert_ne!(now, (now * 10.0).round() / 10.0);
    }
}
