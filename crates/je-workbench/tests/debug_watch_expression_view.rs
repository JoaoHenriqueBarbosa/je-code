//! Porte de `src/vs/workbench/contrib/debug/test/browser/watchExpressionView.test.ts` (suíte `Debug - Watch Debug View`), caso a caso e na mesma ordem.
//!
//! NENHUM caso foi portado (sem `#[test]`), pelo mesmo motivo: `watch expressions with display type` e `watch expressions`
//! chamam `WatchExpressionsRenderer.renderElement` (`browser/watchExpressionsView.ts`) sobre elementos do DOM
//! (`textContent` do `value`, do `type` e do `label.element`, com o `HighlightedLabel`) e o `DebugExpressionRenderer`
//! (`browser/debugExpressionRenderer.ts`), com o `IHoverService` e a configuração `debug.showVariableTypes` por
//! `TestInstantiationService`. O porte não tem esses renderizadores de DOM (a UI é nativa); o modelo que eles leem (`Variable`,
//! `Scope`, `StackFrame`, `Thread`) já está portado e testado em `debug_model.rs` e `debug_call_stack.rs`.
