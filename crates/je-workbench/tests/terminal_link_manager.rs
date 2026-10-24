//! Porte de `src/vs/workbench/contrib/terminalContrib/links/test/browser/terminalLinkManager.test.ts` (suíte `TerminalLinkManager`), caso a caso e na mesma ordem.
//!
//! Nenhum dos 9 casos foi portado, e o `TerminalLinkManager` (`terminalLinkManager.ts`) também não existe na produção. Lista de
//! casos de fora e o motivo exato:
//! - `registerExternalLinkProvider > should not leak disposables if the link manager is already disposed`,
//!   `getLinks and open recent link > should return no links`, `should return word links in order`, `should return web links in
//!   order` e `should return file links in order`: constroem o `TerminalLinkManager`, cujo construtor registra os detectores no
//!   emulador por `xterm.registerLinkProvider` (o `ITerminal` do porte só tem `cols` e `buffer.active`), liga o
//!   `xterm.options.linkHandler` (ativar, `hover` e `leave`) e cria os `TerminalLinkDetectorAdapter`, `TerminalLink` e
//!   `TerminalExternalLinkDetector`, além de depender do `ITunnelService`, do `INotificationService`, do `ITelemetryService` e do
//!   `ITerminalConfigurationService`. Essas peças (adaptador de detector, link do terminal, provedor de links do emulador) não
//!   existem no porte.
//! - `OSC 8 hover > should cancel delayed tooltip when leave happens before hover delay`, `should dismiss shown tooltip on scroll` e
//!   `should dismiss shown tooltip on render`: além do acima, usam `linkHandler.hover/leave`, `xterm.onScroll`, `xterm.onRender`,
//!   `xterm._core._renderService.dimensions` e o `MouseEvent` do DOM, sem equivalente no `ITerminal`/`IXtermRaw` do porte.
//! - `link hover invalidation > replacing or invalidating a link hover disposes the previous hover and its invalidation
//!   listener`: usa o `TerminalWidgetManager`, o `TerminalHover` (widget de hover do terminal) e o `IHoverService`, inexistentes
//!   no porte.
