//! Porte de `src/vs/platform/otel` (telemetria OTLP): os tipos de span concluído e o decodificador do
//! corpo JSON do OTLP/HTTP. Só decodificação, sem rede.

pub mod otlp;
pub mod span_data;
