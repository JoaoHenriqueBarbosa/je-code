//! Porte de `src/vs/platform/telemetry/common` (a parte local): o contrato do serviço, o serviço nulo, os
//! utilitários de validação e de limpeza de dados pessoais.
//!
//! Nada é enviado a servidor algum: não há `TelemetryService` com appenders de rede, nem `1dsAppender`
//! (Application Insights da Microsoft), nem canal remoto. O que fica de fora está listado em `docs/conformance.md`.

#[allow(clippy::module_inception)] // `telemetry.rs` é o porte de `telemetry.ts`, como no original
pub mod telemetry;
pub mod telemetry_log_appender;
pub mod telemetry_service;
pub mod telemetry_utils;

pub use telemetry::*;
pub use telemetry_log_appender::*;
pub use telemetry_service::*;
pub use telemetry_utils::*;
