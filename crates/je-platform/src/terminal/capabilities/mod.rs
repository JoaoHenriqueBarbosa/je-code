//! Porte de `src/vs/platform/terminal/common/capabilities` do VS Code.
//!
//! Portado: as interfaces e o enum de capacidades ([`capabilities`]), o armazenamento de capacidades com o seu
//! multiplexador ([`terminal_capability_store`]) e as capacidades (`CommandDetection`, `CwdDetection`,
//! `BufferMark`, `PartialCommandDetection`, `ShellEnvDetection`, `PromptTypeDetection` e `NaiveCwdDetection`).

#[allow(clippy::module_inception)] // `capabilities.rs` é o porte de `capabilities.ts`, como no original
pub mod capabilities;
pub mod terminal_capability_store;

pub mod shell_env_detection_capability;

pub mod command_detection;

pub mod buffer_mark_capability;
pub mod command_detection_capability;
pub mod cwd_detection_capability;
pub mod naive_cwd_detection_capability;
pub mod partial_command_detection_capability;
pub mod prompt_type_detection_capability;
