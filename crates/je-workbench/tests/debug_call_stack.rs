//! Porte de `src/vs/workbench/contrib/debug/test/browser/callStack.test.ts` (suíte `Debug - CallStack`), caso a caso e na mesma ordem.
//!
//! NENHUM caso foi portado (sem `#[test]`), todos de fora pelo mesmo motivo: o modelo de debug não existe no porte.
//! `crates/je-workbench/src/contrib/debug/` só tem `debug.rs` (interfaces mínimas `IDebugSession`/`IThread`/`IStackFrame`
//! como traits somente de leitura), `debug_source.rs` (`Source`), `debug_utils.rs` e `debug_protocol.rs`. Faltam, e todos os
//! casos dependem deles: `common/debugModel.ts` (`DebugModel`, `Thread`, `StackFrame`, 2160 linhas), `browser/debugSession.ts`
//! (`DebugSession`, 1692 linhas, que `createTestSession` instancia com 20 serviços), `browser/rawDebugSession.ts`
//! (`RawDebugSession`, que o `MockRawSession` de `test/common/mockDebug.ts` imita), `browser/callStackView.ts`
//! (`getContext`, `getContextForContributedActions`, `getSpecificSourceName`, `getThreadContextOverlay`,
//! `getSessionContextOverlay`), `browser/callStackEditorContribution.ts` (`createDecorationsForStackFrame`, que precisa do
//! `IModelDeltaDecoration` e dos ícones `debugStackframe`) e `getStackFrameThreadAndSessionToFocus` de `browser/debugService.ts`.
//! Portar só uma fatia (por exemplo o `Thread` sem a sessão) deixaria os casos infiéis, porque `Thread.fetchCallStack`,
//! `model.rawUpdate` e `model.addSession` conversam com a `DebugSession` e com o `stackTrace` da sessão bruta.
//!
//! Os 13 casos: `threads simple`, `threads multiple with allThreadsStopped`, `allThreadsStopped in multiple events`,
//! `threads multiple without allThreadsStopped`, `stack frame get specific source name`, `stack frame toString()`,
//! `debug child sessions are added in correct order`, `replacing an inactive root session removes its child sessions`,
//! `adding a concurrent root session preserves inactive children of an active root session`, `decorations`, `contexts`,
//! `item-level stack depth contexts`, `focusStackFrameThreadAndSession`.
