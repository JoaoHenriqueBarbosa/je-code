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
//! - fatia 1 (pronta): `text_model_value_reference`, `typing_speed`, `provide_inline_completions` (consulta aos
//!   provedores, `InlineSuggestData` e `InlineSuggestionList`) e `inline_suggestion_item` (`InlineCompletionItem`,
//!   `InlineEditItem`, identidade e dica);
//! - fatia 2 (pronta): `inline_completions_source` (fila de requisições, debounce, `InlineCompletionsState` e telemetria
//!   de resposta vazia sobre `provide_inline_completions`), mais o `wait` do `utils`. O `RenameSymbolProcessor`, o
//!   `StructuredLogger` do `logFetch` e o `DataChannelForwardingTelemetryService` ainda não têm porte: entram como traits
//!   injetados (`InlineCompletionsRenameProcessor`, `InlineCompletionsFetchLogger`, `InlineCompletionsRequestTelemetry`)
//!   no `InlineCompletionsSourceServices`; os portes reais fecham essas pontas;
//! - fatia 3 (pronta): `inline_completions_model` (estado observável, aceitar, aceitar em parte, saltar, rejeitar,
//!   sugestão selecionada) sobre a fonte e os itens, mais `getSecondaryEdits`, `isSuggestionInViewport` e os helpers de
//!   `utils.ts` que faltavam (`getEndPositionsAfterApplying`, `removeTextReplacementCommonSuffixPrefix`). O
//!   `observableCodeEditor` ainda não tem porte: o modelo monta os observáveis do editor que lê e recebe injetados o
//!   `IInlineCompletionsService`, o `IDefaultAccountService`, o `openedPeekWidgets` e o `InlineCompletionsEditorHost`
//!   (diff editor, editor ativo, abrir editor, `getInlineCompletionsController`, animação de fade). O `SuggestItemInfo`
//!   está no modelo; a fábrica `fromSuggestion` e o `ObservableSuggestWidgetAdapter` entram com o controller;
//! - fatia 4: o controller (`inlineCompletionsController`), que implementa o `InlineCompletionsEditorHost` e os
//!   serviços injetados, com os utilitários de teste de `test/browser/utils.ts` (`GhostTextContext`,
//!   `withAsyncTestCodeEditorAndInlineCompletionsModel`, provedor de teste) e os 28 casos de `inlineCompletions.test.ts`
//!   do oráculo.

pub mod compute_ghost_text;
pub mod edit_kind;
pub mod ghost_text;
pub mod graph;
pub mod inline_completion_is_visible;
pub mod inline_completion_view_data;
pub mod inline_completions_model;
pub mod inline_completions_source;
pub mod inline_suggest_alternative_action;
pub mod inline_suggestion_item;
pub mod long_distance_hint;
pub mod long_distance_widget_placement;
pub mod provide_inline_completions;
pub mod text_model_value_reference;
pub mod towers_layout;
pub mod typing_speed;
pub mod utils;
