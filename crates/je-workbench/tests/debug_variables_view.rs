//! Porte de `src/vs/workbench/contrib/debug/test/browser/variablesView.test.ts` (suíte `Debug - Variable Debug View`), caso a caso e na mesma ordem.
//!
//! NENHUM caso foi portado (sem `#[test]`): `variable expressions with display type` e `variable expressions` chamam
//! `VariablesRenderer.renderElement` e `disposeTemplate` (`browser/variablesView.ts`) sobre elementos do DOM (`textContent`
//! do `value`, do `type` e do `label.element`, com o `HighlightedLabel`) e o `DebugExpressionRenderer`, com o
//! `IHoverService` e a configuração `debug.showVariableTypes`. O porte não tem esses renderizadores de DOM (a UI é nativa);
//! o modelo que eles leem já está portado e testado em `debug_model.rs` e `debug_call_stack.rs`.
