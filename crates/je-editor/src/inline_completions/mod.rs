//! Porte de `src/vs/editor/contrib/inlineCompletions`: as partes sem dependência do editor observável.
//!
//! Plano das fatias (ordem de dependência, de `browser/model/`):
//! - fatia 1 (em andamento): `text_model_value_reference` e `typing_speed` (prontos), depois
//!   `provide_inline_completions` e `inline_suggestion_item`. Essas duas dependem da camada de base da fatia 0:
//!   os tipos `languages.InlineCompletion*` (`InlineCompletionsProvider`, `InlineCompletions`,
//!   `InlineCompletionContext`, `InlineCompletionEndOfLifeReason`, `LifetimeSummary`, `PartialAcceptInfo`) que
//!   ainda não existem em `languages.rs`, `inlineCompletionIsVisible` com `singleTextEditHelpers`,
//!   `InlineSuggestAlternativeAction`, `InlineCompletionViewData` e `InlineCompletionViewKind` de
//!   `inlineEditsViewInterface`, `ErrorResult` e `getReadonlyEmptyArray` de `utils.ts`, `fixBracketsInLine`,
//!   `TextModelText` e `getPositionOffsetTransformerFromTextModel`;
//! - fatia 2: `inlineCompletionsSource` (fila de requisições, debounce e `InlineCompletionsState` sobre
//!   `provide_inline_completions`);
//! - fatia 3: `inlineCompletionsModel` (estado observável, aceitar, aceitar parcial, saltar, rejeitar, sugestão
//!   selecionada) sobre a fonte e os itens;
//! - fatia 4: o controller (`inlineCompletionsController`) com os utilitários de teste (`GhostTextContext`,
//!   `withAsyncTestCodeEditorAndInlineCompletionsModel`, provedor de teste) e os 28 casos de `inlineCompletions`
//!   do oráculo.

pub mod compute_ghost_text;
pub mod edit_kind;
pub mod ghost_text;
pub mod graph;
pub mod long_distance_hint;
pub mod long_distance_widget_placement;
pub mod text_model_value_reference;
pub mod towers_layout;
pub mod typing_speed;
