//! Parte de `wtf/MonotonicTime.h` que o `Parser` usa: `now()` e a diferença entre dois instantes.
//!
//! O C++ lê `CLOCK_MONOTONIC`: aqui é o do pseudo-processo (`ul_common::time::monotonic_nanos`), em segundos.

use std::ops::Sub;
use crate::wtf::seconds::Seconds;

/// `class MonotonicTime`.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct MonotonicTime {
    value: f64,
}

impl MonotonicTime {
    pub fn now() -> MonotonicTime {
        MonotonicTime { value: ul_common::time::monotonic_nanos() as f64 / 1e9 }
    }

    pub fn seconds_since_epoch(self) -> Seconds {
        Seconds::new(self.value)
    }

    /// `explicit operator bool`.
    pub fn is_set(self) -> bool {
        self.value != 0.0
    }
}

impl Sub for MonotonicTime {
    type Output = Seconds;

    fn sub(self, other: MonotonicTime) -> Seconds {
        Seconds::new(self.value - other.value)
    }
}
