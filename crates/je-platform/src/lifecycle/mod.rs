//! Porte de `src/vs/platform/lifecycle`: no lado Node, o ciclo de vida do shared process
//! ([`shared_process_lifecycle_service`]).

#[cfg(not(target_arch = "wasm32"))]
pub mod shared_process_lifecycle_service;
