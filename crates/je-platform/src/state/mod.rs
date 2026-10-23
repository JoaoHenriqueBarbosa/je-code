//! Porte de `src/vs/platform/state/node`: o estado da aplicação num arquivo JSON (`storage.json`).

#[allow(clippy::module_inception)] // `state.rs` é o porte de `state.ts`, como no original
pub mod state;
pub mod state_service;

pub use state::{StateReadService, StateService};
pub use state_service::{FileStorage, SaveStrategy, StateReadonlyService, StateServiceImpl};
