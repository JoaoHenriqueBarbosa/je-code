//! Porte de `src/vs/platform/theme/common/sizeRegistry.ts`: o ponto de entrada que reexporta
//! [`crate::size_utils`] e os tamanhos base, para quem o usa saber que todos eles estão registrados.

pub use crate::size_utils::*;

// Make sure all size files are exported
pub use crate::sizes::base_sizes::*;
