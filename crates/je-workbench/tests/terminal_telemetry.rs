//! Porte de `src/vs/workbench/contrib/terminalContrib/telemetry/test/browser/terminalTelemetry.test.ts` (suíte `TerminalTelemetryContribution`), caso a caso e na mesma ordem.
//!
//! O único caso não foi portado, e a `TerminalTelemetryContribution` (`terminalTelemetry.ts`) também não existe na produção.
//! Caso de fora e o motivo exato:
//! - `cancels the shell integration timeout when the terminal or contribution is disposed`: a contribuição é construída com o
//!   `ILifecycleService`, o `ITerminalService` (`onDidCreateInstance`, `onAnyInstanceShellTypeChanged`), o `ITerminalEditorService` e
//!   reage ao `ITerminalInstance` (`processReady`, `onDisposed`, `shellLaunchConfig`, `capabilities`, `target`, `sessionId`...),
//!   além de substituir a API global de tempo (`pushGlobalTimeApi`). Os contratos `ITerminalService`, `ITerminalEditorService` e
//!   `ITerminalInstance` não têm dublê parcial possível em Rust (o original usa `upcastPartial`) e a contribuição depende deles
//!   por inteiro; não existem no porte como contratos usáveis pela contribuição.
