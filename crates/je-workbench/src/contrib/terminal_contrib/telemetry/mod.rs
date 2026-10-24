//! Porte parcial de `src/vs/workbench/contrib/terminalContrib/telemetry/browser`: a telemetria das configurações de perfil e a
//! contribuição de telemetria dos terminais.
//!
//! Fora do porte (e por quê): o registro `registerWorkbenchContribution2` (`terminal.telemetry.contribution.ts`), que depende do
//! registro de contribuições do workbench; quem monta o workbench cria a contribuição.

pub mod terminal_profile_configuration_telemetry;
pub mod terminal_telemetry;
