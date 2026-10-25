//! Porte de `src/vs/editor/contrib/inlineCompletions`: as partes sem dependência do editor observável.
//!
//! Plano das fatias (ordem de dependência, de `browser/model/`):
//! - fatia 0 (pronta): a camada de base. Os tipos `languages.InlineCompletion*` e o registro
//!   `inline_completions_provider` do `LanguageFeaturesService` (em `languages.rs` e `language_features_service.rs`),
//!   `inline_completion_is_visible`, `inline_suggest_alternative_action`, `inline_completion_view_data`
//!   (`InlineCompletionViewKind` e `InlineCompletionViewData`), `utils` (`ErrorResult` e `get_readonly_empty_array`),
//!   `fix_brackets_in_line` (em `bracket_pairs_text_model_part/fix_brackets.rs`), `TextModelText`
//!   (`text_model_text.rs`) e `getPositionOffsetTransformerFromTextModel`
//!   (`position_offset_transformer_with_text_model.rs`). `singleTextEditHelpers` já vive em `compute_ghost_text`;
//! - fatia 1 (em andamento): `text_model_value_reference` e `typing_speed` (prontos), depois
//!   `provide_inline_completions` e `inline_suggestion_item`, que agora têm toda a base da fatia 0;
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
pub mod inline_completion_is_visible;
pub mod inline_completion_view_data;
pub mod inline_suggest_alternative_action;
pub mod long_distance_hint;
pub mod long_distance_widget_placement;
pub mod text_model_value_reference;
pub mod towers_layout;
pub mod typing_speed;
pub mod utils;
