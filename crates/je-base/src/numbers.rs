//! Porte de `src/vs/base/common/numbers.ts`.
//!
//! Números são `f64` (o `number` do JavaScript). `Math.min`/`Math.max` propagam `NaN`, o que o
//! `f64::min`/`max` do Rust não faz, então [`clamp`] usa as versões do JS. `SlidingWindowAverage`
//! exige `size > 0` (o original com 0 produz `NaN`; aqui é pânico por divisão por zero).

use crate::assert::assert;
use crate::dynamic::number_to_js_string;
use crate::random::random_f64;

fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}

fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    js_min(js_max(value, min), max)
}

/// Formata uma contagem de tokens de forma compacta (`128K`, `1M`, `1.5M`).
pub fn format_token_count(count: f64) -> String {
    if count >= 1_000_000.0 {
        let value = count / 1_000_000.0;
        let floored = (value * 10.0).floor() / 10.0;
        if floored % 1.0 == 0.0 { format!("{floored:.0}M") } else { format!("{floored:.1}M") }
    } else if count > 900_000.0 {
        "1M".to_string()
    } else if count >= 1000.0 {
        format!("{}K", number_to_js_string((count / 1000.0).round()))
    } else {
        number_to_js_string(count)
    }
}

/// `index` módulo `modulo` sempre no intervalo `[0, modulo)` (o resto do JS pode ser negativo).
pub fn rot(index: f64, modulo: f64) -> f64 {
    (modulo + (index % modulo)) % modulo
}

#[derive(Debug, Default)]
pub struct Counter {
    next: u64,
}

impl Counter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_next(&mut self) -> u64 {
        let value = self.next;
        self.next += 1;
        value
    }
}

#[derive(Debug)]
pub struct MovingAverage {
    n: f64,
    val: f64,
}

impl Default for MovingAverage {
    fn default() -> Self {
        Self { n: 1.0, val: 0.0 }
    }
}

impl MovingAverage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, value: f64) -> f64 {
        self.val += (value - self.val) / self.n;
        self.n += 1.0;
        self.val
    }

    pub fn value(&self) -> f64 {
        self.val
    }
}

#[derive(Debug)]
pub struct SlidingWindowAverage {
    n: usize,
    val: f64,
    values: Vec<f64>,
    index: usize,
    sum: f64,
}

impl SlidingWindowAverage {
    pub fn new(size: usize) -> Self {
        Self { n: 0, val: 0.0, values: vec![0.0; size], index: 0, sum: 0.0 }
    }

    pub fn update(&mut self, value: f64) -> f64 {
        let old_value = self.values[self.index];
        self.values[self.index] = value;
        self.index = (self.index + 1) % self.values.len();
        self.sum -= old_value;
        self.sum += value;
        if self.n < self.values.len() {
            self.n += 1;
        }
        self.val = self.sum / self.n as f64;
        self.val
    }

    pub fn value(&self) -> f64 {
        self.val
    }
}

/// O ponto `(x, y)` está dentro do triângulo `(ax, ay)`, `(bx, by)`, `(cx, cy)`?
#[allow(clippy::too_many_arguments)]
pub fn is_point_within_triangle(x: f64, y: f64, ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64) -> bool {
    let v0x = cx - ax;
    let v0y = cy - ay;
    let v1x = bx - ax;
    let v1y = by - ay;
    let v2x = x - ax;
    let v2y = y - ay;

    let dot00 = v0x * v0x + v0y * v0y;
    let dot01 = v0x * v1x + v0y * v1y;
    let dot02 = v0x * v2x + v0y * v2y;
    let dot11 = v1x * v1x + v1y * v1y;
    let dot12 = v1x * v2x + v1y * v2y;

    let inv_denom = 1.0 / (dot00 * dot11 - dot01 * dot01);
    let u = (dot11 * dot02 - dot01 * dot12) * inv_denom;
    let v = (dot00 * dot12 - dot01 * dot02) * inv_denom;

    u >= 0.0 && v >= 0.0 && u + v < 1.0
}

/// Verdadeiro com probabilidade `p` (entre 0 e 1; fora disso lança).
pub fn random_chance(p: f64) -> bool {
    assert((0.0..=1.0).contains(&p), "p must be between 0 and 1");
    random_f64() < p
}
