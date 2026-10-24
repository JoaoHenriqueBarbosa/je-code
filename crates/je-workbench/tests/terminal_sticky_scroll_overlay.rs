//! Porte de `src/vs/workbench/contrib/terminalContrib/stickyScroll/test/browser/terminalStickyScrollOverlay.test.ts` (suíte `TerminalStickyScrollOverlay`), caso a caso e na mesma ordem.
//!
//! Nenhum caso foi portado, e o `TerminalStickyScrollOverlay` (`terminalStickyScrollOverlay.ts`) também não existe na produção.
//! Casos de fora e o motivo exato (a suíte roda uma vez em `Classic UI` e outra em `Modern UI`, dez casos de cada, e todos
//! afirmam geometria do DOM):
//! - `positions the header on its first show`, `keeps the header aligned after scrolling`, `keeps the header aligned after a
//!   layout without a row count change`, `pushes a multiline header out by cell heights at the next command`, `cancels the delayed
//!   first show when scrolling back to the prompt`, `keeps the header hidden when laying out the alternate buffer`, `cancels a
//!   pending show in the alternate buffer and restores it on return`, `keeps the header aligned in the <location> terminal`
//!   (`side`, `editor`, `fixed` e `upper split`), `keeps the header aligned when toggling Modern UI` e `keeps fractional font and
//!   zoom geometry within one CSS pixel`: cada um mede `getBoundingClientRect().top` de um elemento `.xterm-screen` dentro de um
//!   `SplitView` DOM com CSS (`terminal.css`, `xterm.css`, `padding.css`), `offsetParent`, `classList` e `style.zoom`, sobre um
//!   `Terminal` do xterm.js aberto no DOM (`raw.open(host)`, `raw.dimensions.css.cell`, `raw.screenElement`). A UI do je-code é
//!   uma árvore de widgets nativa sem DOM nem CSS no caminho de pintura, então nenhum caso tem uma parte "só lógica": a asserção
//!   de cada um é o posicionamento do elemento DOM. Não há lógica sem DOM isolável nesse teste.
