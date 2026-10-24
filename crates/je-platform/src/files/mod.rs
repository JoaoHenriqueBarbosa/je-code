//! Porte de `src/vs/platform/files` do VS Code: o serviço de arquivos e os providers.

pub mod disk;
pub mod disk_provider_base;
pub mod file_service;
#[allow(clippy::module_inception)] // `files.rs` é o porte de `files.ts`, como no original
pub mod files;
pub mod i_file_service;
pub mod in_memory;
pub mod io;
pub mod node_watcher;
pub mod pfs;
pub mod watcher;

pub use crate::log::LogServiceRef;
