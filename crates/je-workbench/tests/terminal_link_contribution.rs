//! Porte de `src/vs/workbench/contrib/terminalContrib/links/test/browser/terminalLinkContribution.test.ts` (suíte `TerminalLinkContribution`), caso a caso e na mesma ordem.
//!
//! Nenhum dos 2 casos foi portado, e a `TerminalLinkContribution` (`terminalLinkContribution.ts`) também não existe na produção.
//! Casos de fora e o motivo exato:
//! - `does not register external link provider listeners for detached terminals` e `registers and disposes external link provider
//!   listeners for regular terminals`: instanciam a `TerminalLinkContribution` com o `ITerminalContributionContext` (e o
//!   `IDetachedCompatibleTerminalContributionContext`), o `ITerminalContribution.xtermReady`, o `TerminalWidgetManager`, o
//!   `TerminalLinkManager` (substituído por `stubInstance`) e o `IXtermTerminal.raw`. O `ITerminalContribution` e o
//!   `registerTerminalContribution` de `contrib/terminal/browser`, o `TerminalWidgetManager` e o `TerminalLinkManager` não
//!   existem no porte. O contrato `ITerminalLinkProviderService` (`links.rs`) existe, mas a contribuição que o consome não.
