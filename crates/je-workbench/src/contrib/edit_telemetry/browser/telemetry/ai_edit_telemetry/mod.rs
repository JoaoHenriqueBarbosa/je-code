//! `editTelemetry/browser/telemetry/aiEditTelemetry`.
//!
//! Fica de fora o `AiEditTelemetryServiceImpl` (`aiEditTelemetryServiceImpl.ts`): ele cria o
//! `DataChannelForwardingTelemetryService`, que depende do `IDataChannelService` (`platform/dataChannel`), ainda não
//! portado.

pub mod ai_edit_telemetry_service;
