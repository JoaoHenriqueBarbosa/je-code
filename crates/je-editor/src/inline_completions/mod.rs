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
//!   está no modelo; a fábrica `fromSuggestion` e o `ObservableSuggestWidgetAdapter` entram na fatia 4a;
//! - fatia 4a (pronta): os auxiliares do controller. `change_recorder` (`TextModelChangeRecorder`, com o
//!   `StructuredLogger` injetado), `suggest_widget_adapter` (`SuggestWidgetAdaptor`, `SuggestItemInfo.fromSuggestion` e
//!   `ObservableSuggestWidgetAdapter`; o que depende do `SuggestWidget` entra como o `SuggestWidgetHost`), o restante de
//!   `utils.ts` (`addPositions`, `subtractPositions`, `substringPos`, `convertItemsToStableObservables` e
//!   `ObservableContextKeyService`, que usa o `bindContextKey` de `je_platform`) e `controller/` (`commandIds`, `common` e
//!   `inlineCompletionContextKeys`);
//! - fatia 4b (ghost text pronto): `view/ghost_text_view` (`GhostTextView`, `AdditionalLinesWidget`, as decorações
//!   inline e o realce). A `View` do editor ainda não tem porte: as view zones e o `onMouseUp` chegam pelo
//!   `IGhostTextViewHost`. A `InlineSuggestionsView`, que junta ghost text, hints e inline edits, vem depois;
//! - fatia 4c: as views e o controller. Em ordem de dependência:
//!   - 4c-1 (pronta): a base do inline edits, em `view/inline_edits/` (`inlineEditsViewInterface`, `inlineEditWithChanges`,
//!     `inlineEditsModel`, `theme` e `utils/` com `utils` e `flexBoxLayout`; o `towersLayout` e o `scrollToReveal` do
//!     `longDistanceHint` seguem em `towers_layout` e `long_distance_hint`), mais o `Rect` de `core_2d` e o
//!     `getNewLineRanges` e o `toLineEdit` de `edits/line_edit`;
//!   - 4c-2 (pronta): as views de inline edits (`inlineEditsViews/*`): inserção de palavra, colapsada, exclusão, `jumpToView`,
//!     `inlineEditsCustomView`, `originalEditorInlineDiffView`, substituição de palavras, substituição de linhas, lado a
//!     lado, inserção de várias linhas (`inline_edits_insertion_view`, que cria o `GhostTextView` direto) e `longDistanceHint/`
//!     (`long_distance_hint/`: a dica `InlineEditsLongDistanceHint` e o `LongDistancePreviewEditor`). Todas em
//!     `inline_edits_views/`, com o `NonOverflowOverlayWidget` que elas compartilham; o lado a lado trouxe o
//!     `EmbeddedCodeEditorWidget`, em `embedded_code_editor_widget`. A trilha de símbolos e o arquivo alvo da dica de
//!     longa distância (a fábrica de breadcrumbs do `HideUnchangedRegionsFeature`, `basename` e `getIconClasses`) entram pelo
//!     `ILongDistanceHintHost`;
//!   - 4c-3 (pronta): o gutter indicator, em `view/inline_edits/components/`: `gutterIndicatorView`
//!     (`InlineEditsGutterIndicator`, `InlineEditsGutterIndicatorData`, `InlineSuggestionGutterMenuData`,
//!     `SimpleInlineSuggestModel`, o posicionamento na margem, o hover e o pulso) e `gutterIndicatorMenu`
//!     (`GutterIndicatorMenuContent`, o cabeçalho, as ações, os atalhos e o rodapé como dado em `GutterMenuContentVisual`).
//!     O `GutterIndicatorMenuHost` implementa o `IGutterIndicatorHost` do indicador; o hover e o rastreador de foco
//!     (`showInstantHover` e `createFocusTracker`) entram pelo `IGutterMenuHoverPresenter`, fechado pelo pintor nativo e pelo
//!     controller (4c-6); a altura do sticky scroll o indicador lê do `StickyScrollController` do editor;
//!   - 4c-4 (pronta): a `InlineEditsView`, em `view/inline_edits/inline_edits_view` (o arquivo inteiro: o `_uiState`,
//!     `_determineView`, `_determineRenderState`, as views portadas, o `JumpToView` e o clique), com o `ILongDistanceHint` e
//!     o `ILongDistanceViewState` em `long_distance_hint`, e o producer (`inline_edits_view_producer`:
//!     `InlineEditsViewAndDiffProducer`, que cria o `ModelPerInlineEdit` observável que a view lê). A inserção e a dica de
//!     longa distância são criadas direto, como no original; o `InlineEditsViewServices` leva os serviços que as views pedem
//!     e a `IPreviewTextModelFactory` (o `TextModel` de prévia, que o `IInstantiationService` criava);
//!   - 4c-5 (pronta, menos uma peça): a `InlineSuggestionsView` (`view/inline_suggestions_view`: junta o `GhostTextView`, o
//!     `InlineEditsViewAndDiffProducer`, o indicador da margem e o hints widget) está pronta. O hints widget
//!     (`hints_widget/`: `InlineCompletionsHintsWidget`, `InlineSuggestionHintsContentWidget`, `StatusBarViewItem`, o modelo da
//!     `CustomizedMenuWorkbenchToolBar` sobre o `IMenuService` e `get_action_bar_actions` do `je-platform`, e o
//!     `InlineCompletionsHoverParticipant`) é criado direto pela view, com os serviços de `InlineEditsViewServices`
//!     (acrescidos de `command_service` e `menu_service`). Falta, ligada por contrato anotado no `//!` da view, a
//!     `InlineEditsOnboardingExperience` (`inlineEditsNewUsers`), que depende do armazenamento e da configuração do usuário
//!     (o controller não a cria: é a view que a cria, ao fim do construtor);
//!   - 4c-6 (pronta): o controller (`controller/inline_completions_controller`: `InlineCompletionsController`, que cria o
//!     modelo e a view, registra os disparos, o foco, o sinal de acessibilidade e as chaves de contexto, e se registra como
//!     contribuição do editor em `register_inline_completions_controller`). Implementa o `InlineCompletionsEditorHost` do
//!     modelo (`InlineCompletionsControllerHost`, que entrega o serviço de editores ao `IInlineCompletionsCodeEditorService`
//!     injetado), o `IInlineCompletionsHoverController` do participante de hover, e o `IInlineCompletionsController` e o
//!     `IInlineCompletionsHost` do `SuggestModel`. Ficam para quem pinta e para o registro da contribuição: o
//!     `IInlineCompletionsHoverHost` do participante (o `getWarningWidgetContext` é do pintor), o `FadeoutDecoration`, a
//!     lista de editores de diff, o `HoverParticipantRegistry.register` e as ações de `controller/commands.ts`;
//! - fatia 4d (próxima): os utilitários de teste de `test/browser/utils.ts` (`GhostTextContext`,
//!   `withAsyncTestCodeEditorAndInlineCompletionsModel`, provedor de teste) e os 28 casos de `inlineCompletions.test.ts`
//!   do oráculo.

pub mod change_recorder;
pub mod compute_ghost_text;
pub mod controller;
pub mod edit_kind;
pub mod ghost_text;
pub mod graph;
pub mod hints_widget;
pub mod inline_completion_is_visible;
pub mod inline_completion_view_data;
pub mod inline_completions_model;
pub mod inline_completions_source;
pub mod inline_suggest_alternative_action;
pub mod inline_suggestion_item;
pub mod long_distance_hint;
pub mod long_distance_widget_placement;
pub mod provide_inline_completions;
pub mod suggest_widget_adapter;
pub mod text_model_value_reference;
pub mod towers_layout;
pub mod typing_speed;
pub mod utils;
pub mod view;
