//! Porte de `src/vs/base/common/stopwatch.ts`.
//!
//! O original escolhe entre `performance.now()` (alta resolução, padrão) e `Date.now()`. Aqui os dois
//! relógios são o `Instant` monotônico (alta resolução, em milissegundos fracionários) e o
//! `Date.now()` do `runtime` (relógio do loop, que os testes controlam com timers falsos; milissegundos
//! inteiros).

use crate::runtime;

fn performance_now() -> f64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    // `performance.now()` conta a partir do início do processo.
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

fn date_now() -> f64 {
    runtime::date_now() as f64
}

/// `StopWatch`.
#[derive(Clone, Copy, Debug)]
pub struct StopWatch {
    start_time: f64,
    stop_time: f64,
    now: fn() -> f64,
}

impl StopWatch {
    /// `StopWatch.create(highResolution?)`.
    pub fn create(high_resolution: Option<bool>) -> Self {
        Self::new(high_resolution)
    }

    /// `new StopWatch(highResolution?)`: só `Some(false)` troca a alta resolução por `Date.now`.
    pub fn new(high_resolution: Option<bool>) -> Self {
        let now: fn() -> f64 = if high_resolution == Some(false) { date_now } else { performance_now };
        Self { start_time: now(), stop_time: -1.0, now }
    }

    pub fn stop(&mut self) {
        self.stop_time = (self.now)();
    }

    pub fn reset(&mut self) {
        self.start_time = (self.now)();
        self.stop_time = -1.0;
    }

    /// `elapsed`: milissegundos desde o início (ou até `stop`).
    pub fn elapsed(&self) -> f64 {
        if self.stop_time != -1.0 {
            return self.stop_time - self.start_time;
        }
        (self.now)() - self.start_time
    }
}
