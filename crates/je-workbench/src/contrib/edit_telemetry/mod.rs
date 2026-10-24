//! Porte de `src/vs/workbench/contrib/editTelemetry`: a telemetria de quem produziu cada trecho do texto (usuário,
//! sugestão inline, chat, IDE, edição externa). Esta fatia traz a base que o `editSourceTrackingImpl.ts` usa: o
//! espaço de trabalho observável, os documentos com edições anotadas, o rastreador de edições, o rastreador de ARC e
//! o serviço de números aleatórios.

pub mod browser;
pub mod common;
