//! Parte de `wtf/Seconds.h` que o `MonotonicTime` e o `Parser` usam (duração em segundos como `f64`).

use std::ops::Sub;

#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Seconds {
    value: f64,
}

impl Seconds {
    pub const fn new(value: f64) -> Seconds {
        Seconds { value }
    }

    pub fn value(self) -> f64 {
        self.value
    }

    pub fn seconds(self) -> f64 {
        self.value
    }

    pub fn milliseconds(self) -> f64 {
        self.value * 1000.0
    }

    pub fn microseconds(self) -> f64 {
        self.value * 1_000_000.0
    }

    pub fn nanoseconds(self) -> f64 {
        self.value * 1_000_000_000.0
    }
}

impl Sub for Seconds {
    type Output = Seconds;

    fn sub(self, other: Seconds) -> Seconds {
        Seconds { value: self.value - other.value }
    }
}
