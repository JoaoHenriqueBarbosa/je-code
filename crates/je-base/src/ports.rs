//! Porte de `src/vs/base/common/ports.ts`.

use crate::random::random_f64;

/// `randomPort()`: uma porta entre 1025 e 65535 (exclusive no topo, como o `Math.floor` do original).
pub fn random_port() -> u32 {
    const MIN: f64 = 1025.0;
    const MAX: f64 = 65535.0;
    (MIN + ((MAX - MIN) * random_f64()).floor()) as u32
}
