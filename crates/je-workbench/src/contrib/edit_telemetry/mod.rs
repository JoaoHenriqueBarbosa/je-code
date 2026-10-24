//! Porte de `src/vs/workbench/contrib/editTelemetry`: a telemetria de quem produziu cada trecho do texto (usuário,
//! sugestão inline, chat, IDE, edição externa). A primeira fatia trouxe a base: o espaço de trabalho observável, os
//! documentos com edições anotadas, o rastreador de edições, o rastreador de ARC e o serviço de números aleatórios. A
//! segunda traz o `editSourceTrackingImpl.ts` e o que ele usa: os documentos anotados, o `ScmAdapter` (como serviço), os
//! remetentes de ARC, o serviço de telemetria de edições de IA (só o contrato) e os contratos do serviço de marcadores do
//! agent host.

pub mod browser;
pub mod common;
