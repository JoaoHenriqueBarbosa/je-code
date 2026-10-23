//! Porte de `src/vs/platform/notification/common`: os tipos e o contrato do serviço de notificações (sem interface).

pub mod notification_legacy;
pub mod notification_message;
pub mod notification_telemetry;
#[allow(clippy::module_inception)] // `notification.rs` é o porte de `notification.ts`, como no original
pub mod notification;

pub use notification::*;
