//! Porte parcial de `src/vs/workbench/contrib/terminalContrib/telemetry/browser`: a telemetria das configurações de perfil.
//!
//! Fora do porte (e por quê): a `TerminalTelemetryContribution` (`terminalTelemetry.ts`) e o registro dela
//! (`terminal.telemetry.contribution.ts`), que dependem do `ITerminalService`, do `ITerminalEditorService`, do
//! `ITerminalInstance` e do `registerWorkbenchContribution2` (`contrib/terminal/browser`, inexistentes no porte).

pub mod terminal_profile_configuration_telemetry;
