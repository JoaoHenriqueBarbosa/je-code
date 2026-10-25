//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsView.ts`: `InlineEditsView`, o
//! orquestrador das views de inline edits. Ele escolhe, para cada edição, a forma de mostrá-la (lado a lado, exclusão,
//! substituição de palavras ou de linhas, inserção, dica colapsada, view personalizada, salto), monta o estado de
//! interface compartilhado (`_uiState`) e cria as nove views da pasta `inline_edits_views/` mais a inserção e a dica de
//! longa distância.
//!
//! Escopo desta fatia: o arquivo inteiro (820 linhas) entrou de uma vez, não só a primeira metade. O `_uiState` do
//! construtor chama `_determineRenderState`, `_determineView`, `_willRenderAboveCursor`, `getViewData` e os auxiliares do
//! fim do arquivo, e o clique chama `_viewHasBeenShownLongerThan`; separar a metade deixaria a primeira sem compilar.
//!
//! O que falta (não está neste arquivo):
//! - `InlineEditsInsertionView` (`inlineEditsInsertionView.ts`) e `InlineEditsLongDistanceHint`
//!   (`longDistanceHint/inlineEditsLongDistanceHint.ts`) ainda não têm porte. Eles entram aqui pelos contratos
//!   [`IInsertionView`] e [`IInlineEditsViewHandle`], criados pela [`IInlineEditsViewFactory`]. Quando os portes existirem,
//!   o controller (4c-6) fornece uma fábrica que os instancia e implementa os dois traits;
//! - o gutter indicator (`components/`) não é criado por este arquivo no original; quem o cria é a
//!   `InlineSuggestionsView` (4c-5). Aqui só entram `SimpleInlineSuggestModel` e `InlineSuggestionGutterMenuData`, que vão
//!   para o estado da dica de longa distância ([`LongDistanceViewState`]);
//! - o `inlineEditsViewProducer` (4c-4, segunda parte) é o que cria o `ModelPerInlineEdit` observável que esta view lê.
//!
//! Notas de tradução:
//! - `@IInstantiationService` (que criava o `TextModel` de prévia, as views, o `InlineEditsLongDistanceHint` e os serviços
//!   de cada view) é o [`InlineEditsViewServices`]: os serviços que as views pedem e a [`IInlineEditsViewFactory`], que cria
//!   o `TextModel` de prévia e as duas views sem porte. As views portadas são criadas direto;
//! - a classe tinha campos inicializados fora de ordem, e o `_constructorDone` era o remendo (o `TODO: remove and use
//!   correct initialization order` do original). Aqui a ordem é a de dependência (`_uiState` não precisa de nenhuma view),
//!   então ele não existe. O estado mutável que o `_uiState` usa (`_previousView`, `_currentInlineEditCache`) e os métodos
//!   que o calculam moram no `ViewLogic`, compartilhado por `Rc` entre o `derived` e o tratador de clique, em vez de
//!   `this`;
//! - `ReturnType<typeof ..._determineView>` e `_determineRenderState` são [`InlineCompletionViewKind`] e
//!   [`InlineEditsRenderState`]: a união discriminada `{ kind, ...campos, viewData }` é o enum [`RenderDetail`] mais o
//!   `viewData`. O `state` de `_uiState` (que o TS tipa como opcional só pelo `ReturnType`) nunca falta;
//! - `this._useCodeShifting` e `this._renderSideBySide` (`getOption(inlineSuggest).map(...)`) são lidos direto da opção
//!   `inlineSuggest` dentro de cada derivado, porque os enums das opções não têm igualdade estrita para o `map`. A única
//!   diferença é que um derivado reexecuta quando qualquer campo da opção muda, não só o campo lido;
//! - `StrictEquals` de [`InlineCompletionEditorType`] e [`InlineSuggestHint`] (o `===` do original): o tipo do editor
//!   compara por valor; a dica compara por valor também, já que cada leitura de `displayLocation` clona o dado;
//! - a identidade de `alternativeAction` (o `===` que o `WordReplacementsViewData.equals` usa) é a do `Rc`. O original
//!   guarda um único objeto; aqui a ação é clonada a cada leitura do item, então o `ViewLogic` reaproveita o `Rc` anterior
//!   enquanto a identidade da sugestão, o rótulo e o id do comando forem os mesmos;
//! - o `_wordReplacementViews` (`mapObservableArrayCached` com a chave padrão: o próprio objeto) usa o endereço do `Rc` do
//!   item como chave, e o derivado com `equalsFn: arrayC(thisC())` mantém o vetor anterior quando todos os itens são
//!   iguais por `equals`, como no original;
//! - `reader.store.add(createInstance(InlineEditsLongDistanceHint, ...))` é o `reader.store().add(...)` do derivado;
//! - `clickEvent.event.preventDefault()` não existe: o `MouseEvent` do crate é um dado, não o evento do navegador, então
//!   não há o que cancelar daqui. Quem pinta e entrega o clique sabe que a view o aceitou (o `accept` roda no mesmo
//!   tratador) e decide o `preventDefault` do evento nativo;
//! - o `div.minScrollHeightViewZone` da zona de altura mínima é o [`MinScrollHeightNode`], que o pintor reconhece;
//! - `this._simpleModel.read(reader)!` e `this._inlineSuggestInfo.read(reader)!` do estado da dica de longa distância: sem
//!   um dos dois o estado é `None` (a dica não aparece), em vez de passar `undefined` adiante;
//! - `ITextModel.edit(textEdit)` do `_previewTextModel` é um `push_edit_operations` sem estado do cursor (a definição do
//!   original: `pushEditOperations(null, replacements.map(...), null)`); o `TextModel.edit` ainda não tem porte;
//! - `Date.now()` é [`date_now`].

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::event::{self, Event};
use je_base::js_string::{char_code_at, is_js_whitespace, starts_with_at, trim};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::{
    DerivedOptions, IObservable, IReader, IReaderWithStore, Observable, ObservableExt, StrictEquals, autorun, derived, derived_opts,
    map_observable_array_cached_by,
};
use je_base::runtime::date_now;
use je_platform::accessibility::IAccessibilityService;
use je_platform::contextkey::IContextKeyService;
use je_platform::keybinding::IKeybindingService;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::inline_suggest::{INLINE_SUGGEST, InlineEditsAllowCodeShifting, InlineEditsRenderSideBySide, InternalInlineSuggestOptions};
use crate::cursor_common::CursorSimpleModel;
use crate::diff::range_mapping::{DetailedLineRangeMapping, RangeMapping, line_range_mapping_from_range_mappings};
use crate::edit_operation::SingleEditOperation;
use crate::edits::string_edit::{BaseStringEdit, StringEdit};
use crate::edits::text_edit::TextReplacement;
use crate::editor_browser::{IViewZone, IViewZoneChangeAccessor};
use crate::embedded_code_editor_widget::IEmbeddedCodeEditorFactory;
use crate::inline_completions::inline_suggest_alternative_action::InlineSuggestAlternativeAction;
use crate::inline_completions::inline_suggestion_item::{InlineSuggestHint, InlineSuggestionAction, InlineSuggestionIdentity, InlineSuggestionItemBase};
use crate::inline_completions::long_distance_hint::{LongDistanceHint, LongDistanceViewState};
use crate::inline_completions::provide_inline_completions::InlineCompletionEditorType;
use crate::inline_completions::text_model_value_reference::TextModelValueReference;
use crate::language_service::ILanguageService;
use crate::line_range::LineRange;
use crate::model::{BracketPairColorizationOptions, TextModelCreationOptions};
use crate::observable_code_editor::{ObservableCodeEditor, observable_code_editor};
use crate::offset_range::OffsetRange;
use crate::position::Position;
use crate::position_offset_transformer_with_text_model::get_position_offset_transformer_from_text_model;
use crate::position_to_offset::{PositionOffsetTransformerBase, TextLength};
use crate::range::Range;
use crate::services::theme_service::IThemeService;
use crate::text::abstract_text::{AbstractText, StringText};
use crate::text_model::TextModel;

use super::components::gutter_indicator_view::{InlineSuggestionGutterMenuData, SimpleInlineSuggestModel};
use super::inline_edit_with_changes::InlineEditWithChanges;
use super::inline_edits_model::ModelPerInlineEdit;
use super::inline_edits_view_interface::{IInlineEditsView, InlineCompletionViewData, InlineCompletionViewKind, InlineEditClickEvent, InlineEditTabAction};
use super::inline_edits_views::inline_edits_collapsed_view::InlineEditsCollapsedView;
use super::inline_edits_views::inline_edits_custom_view::InlineEditsCustomView;
use super::inline_edits_views::inline_edits_deletion_view::{DeletionViewUiState, InlineEditsDeletionView};
use super::inline_edits_views::inline_edits_line_replacement_view::{InlineEditsLineReplacementView, LineReplacementEdit, Replacement};
use super::inline_edits_views::inline_edits_side_by_side_view::{ISideBySideViewHost, InlineEditsSideBySideView, SideBySideUiState};
use super::inline_edits_views::inline_edits_word_replacement_view::{InlineEditsWordReplacementView, WordReplacementsViewData};
use super::inline_edits_views::jump_to_view::{JumpToView, JumpToViewData, JumpToViewOptions, JumpToViewStyle};
use super::inline_edits_views::original_editor_inline_diff_view::{OriginalEditorInlineDiffView, OriginalEditorInlineDiffViewMode, OriginalEditorInlineDiffViewState};
use super::utils::utils::{apply_edit_to_modified_range_mappings, create_reindent_edit};

// #region Igualdade estrita

impl StrictEquals for InlineCompletionEditorType {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

impl StrictEquals for InlineSuggestHint {
    fn strict_equals(&self, other: &Self) -> bool {
        self.range == other.range && self.content == other.content && self.style == other.style
    }
}

// #endregion

/// O `_uiState.state` do original, sem o `viewData`: a união discriminada pelo `kind`.
#[derive(Clone)]
pub enum RenderDetail {
    JumpTo { position: Position },
    InsertionInline,
    SideBySide,
    Collapsed,
    Custom { display_location: Option<InlineSuggestHint> },
    Deletion { original_range: LineRange, deletions: Vec<Range> },
    InsertionMultiLine { line_number: i32, column: i32, text: JsString },
    WordReplacements { replacements: Vec<TextReplacement>, alternative_action: Option<Rc<InlineSuggestAlternativeAction>> },
    LineReplacement(LineReplacementEdit),
}

impl RenderDetail {
    /// O `kind` da união.
    pub fn kind(&self) -> InlineCompletionViewKind {
        match self {
            Self::JumpTo { .. } => InlineCompletionViewKind::JumpTo,
            Self::InsertionInline => InlineCompletionViewKind::InsertionInline,
            Self::SideBySide => InlineCompletionViewKind::SideBySide,
            Self::Collapsed => InlineCompletionViewKind::Collapsed,
            Self::Custom { .. } => InlineCompletionViewKind::Custom,
            Self::Deletion { .. } => InlineCompletionViewKind::Deletion,
            Self::InsertionMultiLine { .. } => InlineCompletionViewKind::InsertionMultiLine,
            Self::WordReplacements { .. } => InlineCompletionViewKind::WordReplacements,
            Self::LineReplacement(_) => InlineCompletionViewKind::LineReplacement,
        }
    }
}

/// O retorno de `_determineRenderState`: `{ kind, ...campos, viewData }`.
#[derive(Clone)]
pub struct InlineEditsRenderState {
    pub detail: RenderDetail,
    pub view_data: InlineCompletionViewData,
}

/// O `_uiState` do original.
pub struct InlineEditsUiState {
    pub state: InlineEditsRenderState,
    pub diff: Vec<DetailedLineRangeMapping>,
    pub edit: Rc<InlineEditWithChanges>,
    pub new_text: JsString,
    pub new_text_line_count: i32,
    pub editor_type: InlineCompletionEditorType,
    pub long_distance_hint: Option<LongDistanceHint>,
    pub next_cursor_position: Option<Position>,
    pub target: Rc<TextModelValueReference>,
}

/// O objeto anônimo que o construtor da `InlineEditsInsertionView` recebe: `{ lineNumber, startColumn, text, editorType }`.
#[derive(Clone, Debug, PartialEq)]
pub struct InsertionViewInput {
    pub line_number: i32,
    pub start_column: i32,
    pub text: JsString,
    pub editor_type: InlineCompletionEditorType,
}

/// O que a `InlineEditsView` exige de uma view que ela só conhece pelo contrato: ser uma `IInlineEditsView` descartável.
pub trait IInlineEditsViewHandle: IInlineEditsView + IDisposable {}

impl<T: IInlineEditsView + IDisposable> IInlineEditsViewHandle for T {}

/// O que a `InlineEditsInsertionView` expõe à `InlineEditsView`: `originalLines` e `startLineOffset`.
pub trait IInsertionView: IInlineEditsViewHandle {
    /// `originalLines`: o intervalo de linhas originais que a inserção cobre (`None` sem inserção).
    fn original_lines(&self) -> IObservable<Option<LineRange>>;

    /// `startLineOffset`: o `topOffset` do corte vertical.
    fn start_line_offset(&self) -> IObservable<f64>;
}

/// O que o `IInstantiationService` criava para a `InlineEditsView` e que ainda não tem porte (ver as notas do módulo).
pub trait IInlineEditsViewFactory {
    /// `createInstance(TextModel, '', languageId, options, null)`: o modelo de prévia, vazio.
    fn create_preview_text_model(&self, language_id: &str, options: TextModelCreationOptions) -> Rc<TextModel>;

    /// `createInstance(InlineEditsInsertionView, editor, input, tabAction)`.
    fn create_insertion_view(
        &self,
        editor: Rc<CodeEditorWidget>,
        input: IObservable<Option<Rc<InsertionViewInput>>>,
        tab_action: IObservable<InlineEditTabAction>,
    ) -> Rc<dyn IInsertionView>;

    /// `createInstance(InlineEditsLongDistanceHint, editor, viewState, previewTextModel, tabAction)`.
    fn create_long_distance_hint(
        &self,
        editor: Rc<CodeEditorWidget>,
        view_state: IObservable<Option<Rc<LongDistanceViewState>>>,
        preview_text_model: Rc<TextModel>,
        tab_action: IObservable<InlineEditTabAction>,
    ) -> Rc<dyn IInlineEditsViewHandle>;
}

/// Os serviços injetados que as views da `InlineEditsView` pedem (ver as notas do módulo).
pub struct InlineEditsViewServices {
    pub theme_service: Rc<dyn IThemeService>,
    pub language_service: Rc<dyn ILanguageService>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub accessibility_service: Rc<dyn IAccessibilityService>,
    pub side_by_side_host: Rc<dyn ISideBySideViewHost>,
    pub embedded_code_editor_factory: Rc<dyn IEmbeddedCodeEditorFactory>,
    pub views: Rc<dyn IInlineEditsViewFactory>,
}

/// O `dom_node` da view zone `div.minScrollHeightViewZone`: o pintor o reconhece e reserva a altura.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MinScrollHeightNode;

/// A view zone que garante a altura mínima de rolagem (`addZone` do autorun do construtor).
struct MinScrollHeightViewZone {
    after_line_number: i32,
    height_in_px: f64,
}

impl IViewZone for MinScrollHeightViewZone {
    fn after_line_number(&self) -> i32 {
        self.after_line_number
    }

    fn height_in_px(&self) -> Option<f64> {
        Some(self.height_in_px)
    }

    fn dom_node(&self) -> Rc<dyn Any> {
        Rc::new(MinScrollHeightNode)
    }
}

/// O `_previousView` do original.
struct PreviousView {
    id: String,
    view: InlineCompletionViewKind,
    editor_width: f64,
    timestamp: f64,
    uri: JsString,
}

/// O `_currentInlineEditCache` do original.
struct CurrentInlineEditCache {
    inline_suggestion_identity: Rc<InlineSuggestionIdentity>,
    first_cursor_line_number: i32,
}

/// `{ originalRange, modifiedRange, original, modified }` do `stringChanges`.
struct StringChange {
    original_range: Range,
    modified_range: Range,
    original: JsString,
    modified: JsString,
}

/// O estado e os cálculos que o `_uiState` usa (ver as notas do módulo).
struct ViewLogic {
    editor: Rc<CodeEditorWidget>,
    editor_obs: Rc<ObservableCodeEditor>,
    model: IObservable<Option<Rc<ModelPerInlineEdit>>>,
    show_collapsed: IObservable<bool>,
    inline_suggest_options: IObservable<Rc<InternalInlineSuggestOptions>>,
    preview_text_model: Rc<TextModel>,
    previous_view: RefCell<Option<PreviousView>>,
    current_inline_edit_cache: RefCell<Option<CurrentInlineEditCache>>,
    alternative_action_cache: RefCell<Option<(Rc<InlineSuggestionIdentity>, Rc<InlineSuggestAlternativeAction>)>>,
}

impl ViewLogic {
    /// `this._editorObs.model.get()!.uri.toString()`.
    fn current_model_uri(&self) -> JsString {
        self.editor_obs.model.get().expect("o editor tem modelo").uri().to_string(false)
    }

    /// `_getCacheId(model)`.
    fn get_cache_id(model: &ModelPerInlineEdit) -> String {
        model.inline_edit.inline_completion.base().identity.id.clone()
    }

    /// `_getLongDistanceHintState(model, reader)`.
    fn get_long_distance_hint_state(&self, model: &ModelPerInlineEdit, reader: &dyn IReader) -> Option<LongDistanceHint> {
        let inline_edit = &model.inline_edit;
        let identity = Rc::clone(&inline_edit.inline_completion.base().identity);
        if identity.jumped_to().read(reader) {
            return None;
        }
        inline_edit.action.as_ref()?;
        let editor_model = self.editor_obs.model.read(reader);
        let targets_editor = editor_model.as_ref().is_some_and(|editor_model| inline_edit.original_text.targets(editor_model));
        if !targets_editor {
            return Some(LongDistanceHint { is_visible: true, line_number: inline_edit.cursor_position.line_number });
        }

        let first_cursor_line_number = {
            let mut cache = self.current_inline_edit_cache.borrow_mut();
            if cache.as_ref().is_none_or(|cache| !Rc::ptr_eq(&cache.inline_suggestion_identity, &identity)) {
                *cache = Some(CurrentInlineEditCache {
                    inline_suggestion_identity: identity,
                    first_cursor_line_number: inline_edit.cursor_position.line_number,
                });
            }
            cache.as_ref().expect("preenchido acima").first_cursor_line_number
        };
        Some(LongDistanceHint { line_number: first_cursor_line_number, is_visible: !model.in_view_port.read(reader) })
    }

    /// O `derived` de `_uiState`.
    fn compute_ui_state(&self, reader: &dyn IReader) -> Option<Rc<InlineEditsUiState>> {
        let model = self.model.read(reader)?;
        let text_model = self.editor_obs.model.read(reader)?;

        let inline_edit = Rc::clone(&model.inline_edit);
        let original_text: Rc<dyn AbstractText> = inline_edit.original_text.clone();
        let mut diff: Vec<DetailedLineRangeMapping>;
        let mut mappings: Vec<RangeMapping>;
        let mut new_text: Rc<dyn AbstractText>;

        if let Some(edit) = &inline_edit.edit {
            mappings = RangeMapping::from_edit(edit);
            new_text = Rc::new(StringText::new(edit.apply(&*original_text)));
            diff = line_range_mapping_from_range_mappings(&mappings, &*original_text, &*new_text, false);
        } else {
            mappings = Vec::new();
            diff = Vec::new();
            new_text = Rc::clone(&original_text);
        }

        let Some(mut state) = self.determine_render_state(&model, reader, &diff, &*new_text) else {
            let previous = self.previous_view.borrow().as_ref().map_or("undefined", |previous| previous.view.as_str());
            errors::on_unexpected_error(&Error::new(format!("unable to determine view: tried to render {previous}")));
            return None;
        };

        let long_distance_hint = self.get_long_distance_hint_state(&model, reader);

        if let Some(hint) = long_distance_hint.filter(|hint| hint.is_visible) {
            state.view_data.set_long_distance_view_data(hint.line_number, inline_edit.line_edit().line_range.start_line_number);
        }

        state.view_data.is_for_another_document = Some(!inline_edit.original_text.targets(&text_model));

        if state.detail.kind() == InlineCompletionViewKind::SideBySide {
            let indentation_adjustment_edit =
                create_reindent_edit(&new_text.get_value(), inline_edit.modified_line_range(), text_model.get_options().tab_size as i32);
            new_text = Rc::new(StringText::new(indentation_adjustment_edit.apply_to_string(new_text.get_value())));

            mappings = apply_edit_to_modified_range_mappings(&mappings, &indentation_adjustment_edit);
            diff = line_range_mapping_from_range_mappings(&mappings, &*original_text, &*new_text, false);
        }

        self.preview_text_model.set_language(&text_model.get_language_id(), None);

        let previous_new_text = self.preview_text_model.get_value();
        let new_value = new_text.get_value();
        if previous_new_text != new_value {
            self.preview_text_model.set_eol(text_model.get_end_of_line_sequence());
            let update_old_value_edit = StringEdit::replace(OffsetRange::new(0, previous_new_text.len() as i32), new_value.clone());
            let update_old_value_edit_small = update_old_value_edit.remove_common_suffix_prefix(previous_new_text.units());

            let text_edit = get_position_offset_transformer_from_text_model(&self.preview_text_model).get_text_edit(&update_old_value_edit_small);
            let operations: Vec<SingleEditOperation> = text_edit.replacements.iter().map(TextReplacement::to_single_edit_operation).collect();
            self.preview_text_model.push_edit_operations(None, &operations, None, None, None);
        }

        if self.show_collapsed.read(reader) {
            state = InlineEditsRenderState { detail: RenderDetail::Collapsed, view_data: state.view_data };
        }

        model.handle_inline_edit_shown_next_frame(state.detail.kind(), state.view_data.clone());

        let next_cursor_position = match &inline_edit.action {
            Some(InlineSuggestionAction::JumpTo(jump_to)) => Some(jump_to.position),
            _ => None,
        };

        Some(Rc::new(InlineEditsUiState {
            state,
            diff,
            edit: Rc::clone(&inline_edit),
            new_text: new_value,
            new_text_line_count: inline_edit.modified_line_range().length(),
            editor_type: model.editor_type,
            long_distance_hint,
            next_cursor_position,
            target: Rc::clone(&inline_edit.inline_completion.base().original_text_ref),
        }))
    }

    /// `_determineView(model, reader, diff, newText)`.
    fn determine_view(&self, model: &ModelPerInlineEdit, reader: &dyn IReader, diff: &[DetailedLineRangeMapping], new_text: &dyn AbstractText) -> InlineCompletionViewKind {
        // Check if we can use the previous view if it is the same InlineCompletion as previously shown
        let inline_edit = &model.inline_edit;
        let (can_use_cache, reconsider_view_editor_width_change, previous_view) = {
            let previous = self.previous_view.borrow();
            let can_use_cache = previous
                .as_ref()
                .is_some_and(|previous| previous.id == Self::get_cache_id(model) && previous.uri == self.current_model_uri());
            let editor_width = self.editor_obs.layout_info_width.read(reader);
            let reconsider = previous.as_ref().is_some_and(|previous| {
                previous.editor_width != editor_width && matches!(previous.view, InlineCompletionViewKind::SideBySide | InlineCompletionViewKind::LineReplacement)
            });
            (can_use_cache, reconsider, previous.as_ref().map(|previous| previous.view))
        };

        if can_use_cache && !reconsider_view_editor_width_change {
            return previous_view.expect("o cache só vale com a view anterior");
        }

        let action = inline_edit.inline_completion.action();
        if let Some(InlineSuggestionAction::Edit(edit)) = &action {
            if edit.alternative_action.is_some() {
                return InlineCompletionViewKind::WordReplacements;
            }
        }

        let target_uri = inline_edit.inline_completion.base().original_text_ref.uri();
        if let Some(editor_model) = self.editor_obs.model.read(reader) {
            if target_uri.to_string(false) != editor_model.uri().to_string(false) {
                return InlineCompletionViewKind::Custom;
            }
        }

        if model.display_location.is_some() && !inline_edit.inline_completion.base().identity.jumped_to().read(reader) {
            return InlineCompletionViewKind::Custom;
        }

        // Determine the view based on the edit / diff

        let options = self.inline_suggest_options.read(reader);
        let allow_code_shifting = options.edits.allow_code_shifting;
        let num_original_lines = inline_edit.original_line_range().length();
        let num_modified_lines = inline_edit.modified_line_range().length();
        let inner = inner_changes(diff);
        let is_single_inner_edit = inner.len() == 1;
        let original_text: &dyn AbstractText = &*inline_edit.original_text;

        if model.editor_type != InlineCompletionEditorType::DiffEditor {
            if is_single_inner_edit && allow_code_shifting != InlineEditsAllowCodeShifting::Never && is_single_line_insertion(diff) {
                if is_single_line_insertion_after_position(diff, Some(&inline_edit.cursor_position)) {
                    return InlineCompletionViewKind::InsertionInline;
                }

                // If we have a single line insertion before the cursor position, we do not want to move the cursor by inserting
                // the suggestion inline. Use a line replacement view instead. Do not use word replacement view.
                return InlineCompletionViewKind::LineReplacement;
            }

            if is_deletion(&inner, original_text, new_text) {
                return InlineCompletionViewKind::Deletion;
            }

            if is_single_multi_line_insertion(diff) && allow_code_shifting == InlineEditsAllowCodeShifting::Always {
                return InlineCompletionViewKind::InsertionMultiLine;
            }

            let max_length = InlineEditsWordReplacementView::MAX_LENGTH as i32;
            let all_inner_changes_not_too_long = inner
                .iter()
                .all(|m| TextLength::of_range(&m.original_range).column_count < max_length && TextLength::of_range(&m.modified_range).column_count < max_length);
            if all_inner_changes_not_too_long && is_single_inner_edit && num_original_lines == 1 && num_modified_lines == 1 {
                // Do not show indentation changes with word replacement view
                let modified_text: Vec<JsString> = inner.iter().map(|m| new_text.get_value_of_range(&m.modified_range)).collect();
                let original_text_values: Vec<JsString> = inner.iter().map(|m| original_text.get_value_of_range(&m.original_range)).collect();
                if !modified_text.iter().any(includes_tab) && !original_text_values.iter().any(includes_tab) {
                    // Make sure there is no insertion, even if we grow them
                    let insertions: Vec<TextReplacement> = inner.iter().map(|m| TextReplacement::new(m.original_range, "")).collect();
                    if !inner.iter().any(|m| m.original_range.is_empty())
                        || !grow_edits_until_whitespace(&insertions, original_text)
                            .iter()
                            .any(|e| e.range.is_empty() && TextLength::of_range(&e.range).column_count < max_length)
                    {
                        return InlineCompletionViewKind::WordReplacements;
                    }
                }
            }
        }

        if num_original_lines > 0 && num_modified_lines > 0 {
            if num_original_lines == 1 && num_modified_lines == 1 && model.editor_type != InlineCompletionEditorType::DiffEditor
            /* prefer side by side in diff editor */
            {
                return InlineCompletionViewKind::LineReplacement;
            }

            if options.edits.render_side_by_side != InlineEditsRenderSideBySide::Never
                && InlineEditsSideBySideView::fits_inside_viewport(&self.editor, &self.preview_text_model, inline_edit, reader)
            {
                return InlineCompletionViewKind::SideBySide;
            }

            return InlineCompletionViewKind::LineReplacement;
        }

        if model.editor_type == InlineCompletionEditorType::DiffEditor {
            if is_deletion(&inner, original_text, new_text) {
                return InlineCompletionViewKind::Deletion;
            }

            if is_single_multi_line_insertion(diff) && allow_code_shifting == InlineEditsAllowCodeShifting::Always {
                return InlineCompletionViewKind::InsertionMultiLine;
            }
        }

        InlineCompletionViewKind::SideBySide
    }

    /// `_determineRenderState(model, reader, diff, newText)`.
    fn determine_render_state(
        &self,
        model: &ModelPerInlineEdit,
        reader: &dyn IReader,
        diff: &[DetailedLineRangeMapping],
        new_text: &dyn AbstractText,
    ) -> Option<InlineEditsRenderState> {
        let inline_edit = &model.inline_edit;
        if let Some(InlineSuggestionAction::JumpTo(jump_to)) = &inline_edit.action {
            return Some(InlineEditsRenderState { detail: RenderDetail::JumpTo { position: jump_to.position }, view_data: create_empty_view_data() });
        }

        let mut view = self.determine_view(model, reader, diff, new_text);
        if self.will_render_above_cursor(reader, inline_edit, view)
            && matches!(view, InlineCompletionViewKind::LineReplacement | InlineCompletionViewKind::WordReplacements)
        {
            view = InlineCompletionViewKind::SideBySide;
        }
        *self.previous_view.borrow_mut() = Some(PreviousView {
            id: Self::get_cache_id(model),
            view,
            editor_width: self.editor.get_layout_info().width,
            timestamp: date_now() as f64,
            uri: self.current_model_uri(),
        });

        let inner = inner_changes(diff);
        let text_model = self.editor.get_model().expect("o editor tem modelo");
        let string_changes: Vec<StringChange> = inner
            .iter()
            .map(|m| StringChange {
                original_range: m.original_range,
                modified_range: m.modified_range,
                original: inline_edit.original_text.get_value_of_range(&m.original_range),
                modified: new_text.get_value_of_range(&m.modified_range),
            })
            .collect();

        let view_data = get_view_data(inline_edit, &string_changes, &text_model);
        let state = |detail: RenderDetail| Some(InlineEditsRenderState { detail, view_data: view_data.clone() });

        match view {
            InlineCompletionViewKind::InsertionInline => return state(RenderDetail::InsertionInline),
            InlineCompletionViewKind::SideBySide => return state(RenderDetail::SideBySide),
            InlineCompletionViewKind::Collapsed => return state(RenderDetail::Collapsed),
            InlineCompletionViewKind::Custom => return state(RenderDetail::Custom { display_location: model.display_location.clone() }),
            _ => {}
        }

        if view == InlineCompletionViewKind::Deletion {
            return state(RenderDetail::Deletion {
                original_range: inline_edit.original_line_range(),
                deletions: inner.iter().map(|m| m.original_range).collect(),
            });
        }

        if view == InlineCompletionViewKind::InsertionMultiLine {
            let change = inner[0];
            return state(RenderDetail::InsertionMultiLine {
                line_number: change.original_range.start_line_number,
                column: change.original_range.start_column,
                text: new_text.get_value_of_range(&change.modified_range),
            });
        }

        let replacements: Vec<TextReplacement> = string_changes.iter().map(|m| TextReplacement::new(m.original_range, m.modified.clone())).collect();
        if replacements.is_empty() {
            return None;
        }

        if view == InlineCompletionViewKind::WordReplacements {
            let original_text: &dyn AbstractText = &*inline_edit.original_text;
            let mut grown_edits = grow_edits_to_entire_word(&replacements, original_text);
            if grown_edits.iter().any(|e| e.range.is_empty()) {
                grown_edits = grow_edits_until_whitespace(&replacements, original_text);
            }

            return state(RenderDetail::WordReplacements { replacements: grown_edits, alternative_action: self.stable_alternative_action(inline_edit) });
        }

        if view == InlineCompletionViewKind::LineReplacement {
            let modified_range = inline_edit.modified_line_range();
            return state(RenderDetail::LineReplacement(LineReplacementEdit {
                original_range: inline_edit.original_line_range(),
                modified_range,
                modified_lines: modified_range.map_to_line_array(|line| new_text.get_line_at(line)),
                replacements: inner.iter().map(|m| Replacement { original_range: m.original_range, modified_range: m.modified_range }).collect(),
            }));
        }

        None
    }

    /// O `model.inlineEdit.action?.alternativeAction` com identidade estável (ver as notas do módulo).
    fn stable_alternative_action(&self, inline_edit: &InlineEditWithChanges) -> Option<Rc<InlineSuggestAlternativeAction>> {
        let action = match &inline_edit.action {
            Some(InlineSuggestionAction::Edit(edit)) => edit.alternative_action.clone()?,
            _ => return None,
        };
        let identity = &inline_edit.inline_completion.base().identity;
        let mut cache = self.alternative_action_cache.borrow_mut();
        if let Some((cached_identity, cached)) = cache.as_ref() {
            if Rc::ptr_eq(cached_identity, identity) && cached.label == action.label && cached.command.id == action.command.id {
                return Some(Rc::clone(cached));
            }
        }
        let stable = Rc::new(action);
        *cache = Some((Rc::clone(identity), Rc::clone(&stable)));
        Some(stable)
    }

    /// `_willRenderAboveCursor(reader, inlineEdit, view)`.
    fn will_render_above_cursor(&self, reader: &dyn IReader, inline_edit: &InlineEditWithChanges, view: InlineCompletionViewKind) -> bool {
        if self.inline_suggest_options.read(reader).edits.allow_code_shifting == InlineEditsAllowCodeShifting::Always {
            return false;
        }

        let original_line_range = inline_edit.original_line_range();
        let modified_line_range = inline_edit.modified_line_range();
        for cursor_position in &inline_edit.multi_cursor_positions {
            if view == InlineCompletionViewKind::WordReplacements && cursor_position.line_number == original_line_range.start_line_number + 1 {
                return true;
            }

            if view == InlineCompletionViewKind::LineReplacement
                && cursor_position.line_number >= original_line_range.end_line_number_exclusive
                && cursor_position.line_number < modified_line_range.end_line_number_exclusive + modified_line_range.length()
            {
                return true;
            }
        }

        false
    }

    /// `_viewHasBeenShownLongerThan(durationMs)`.
    fn view_has_been_shown_longer_than(&self, duration_ms: f64) -> bool {
        let view_creation_time = self.previous_view.borrow().as_ref().map(|previous| previous.timestamp).filter(|timestamp| *timestamp != 0.0);
        let Some(view_creation_time) = view_creation_time else {
            errors::throw(Error::bug_indicating(Some("viewHasBeenShownLongThan called before a view has been shown")));
        };

        let current_time = date_now() as f64;
        (current_time - view_creation_time) >= duration_ms
    }
}

/// `InlineEditsView`.
pub struct InlineEditsView {
    base: Rc<Disposable>,

    pub side_by_side: Rc<InlineEditsSideBySideView>,
    pub deletion: Rc<InlineEditsDeletionView>,
    pub insertion: Rc<dyn IInsertionView>,
    pub inline_collapsed_view: Rc<InlineEditsCollapsedView>,
    pub custom_view: Rc<InlineEditsCustomView>,
    pub long_distance_hint: IObservable<Option<Rc<dyn IInlineEditsViewHandle>>>,
    pub inline_diff_view: Rc<OriginalEditorInlineDiffView>,
    pub word_replacement_views: IObservable<Rc<Vec<Rc<InlineEditsWordReplacementView>>>>,
    pub line_replacement_view: Rc<InlineEditsLineReplacementView>,
    pub jump_to_view: Rc<JumpToView>,

    /// O `_previewTextModel`.
    pub preview_text_model: Rc<TextModel>,
    /// O `_uiState`: o dado que as views e o pintor leem.
    pub ui_state: IObservable<Option<Rc<InlineEditsUiState>>>,

    pub display_range: IObservable<Option<LineRange>>,
    pub inline_edits_is_hovered: IObservable<bool>,
    pub gutter_indicator_offset: IObservable<f64>,
}

/// O `kind` do `_uiState.state` (`None` sem estado).
fn ui_kind(ui_state: &IObservable<Option<Rc<InlineEditsUiState>>>, reader: &dyn IReader) -> Option<InlineCompletionViewKind> {
    ui_state.read(reader).map(|state| state.state.detail.kind())
}

/// `this._model.map(m => m?.inlineEdit)`.
fn inline_edit_of(model: &IObservable<Option<Rc<ModelPerInlineEdit>>>) -> IObservable<Option<Rc<InlineEditWithChanges>>> {
    model.map(|model, _| model.map(|model| Rc::clone(&model.inline_edit)))
}

/// `this._uiState.map(s => s?.editorType ?? InlineCompletionEditorType.TextEditor)`.
fn editor_type_of(ui_state: &IObservable<Option<Rc<InlineEditsUiState>>>) -> IObservable<InlineCompletionEditorType> {
    ui_state.map(|state, _| state.map_or(InlineCompletionEditorType::TextEditor, |state| state.editor_type))
}

impl InlineEditsView {
    pub fn new(
        editor: Rc<CodeEditorWidget>,
        model: IObservable<Option<Rc<ModelPerInlineEdit>>>,
        simple_model: IObservable<Option<Rc<SimpleInlineSuggestModel>>>,
        inline_suggest_info: IObservable<Option<Rc<InlineSuggestionGutterMenuData>>>,
        show_collapsed: IObservable<bool>,
        services: InlineEditsViewServices,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let editor_obs = observable_code_editor(&editor);
        let inline_suggest_options = editor_obs.get_option(INLINE_SUGGEST);

        let tab_action: IObservable<InlineEditTabAction> = {
            let model = Rc::clone(&model);
            derived(move |reader| model.read(reader).map_or(InlineEditTabAction::Inactive, |model| model.tab_action.read(reader)))
        };

        let mut creation_options = TextModel::default_creation_options();
        creation_options.bracket_pair_colorization_options =
            BracketPairColorizationOptions { enabled: true, independent_color_pool_per_bracket_type: false };
        let preview_text_model =
            services.views.create_preview_text_model(&editor.get_model().expect("o editor tem modelo").get_language_id(), creation_options);
        base.register(preview_text_model.clone());

        let logic = Rc::new(ViewLogic {
            editor: Rc::clone(&editor),
            editor_obs: Rc::clone(&editor_obs),
            model: Rc::clone(&model),
            show_collapsed,
            inline_suggest_options: Rc::clone(&inline_suggest_options),
            preview_text_model: Rc::clone(&preview_text_model),
            previous_view: RefCell::new(None),
            current_inline_edit_cache: RefCell::new(None),
            alternative_action_cache: RefCell::new(None),
        });

        let ui_state: IObservable<Option<Rc<InlineEditsUiState>>> = {
            let logic = Rc::clone(&logic);
            derived(move |reader| logic.compute_ui_state(reader))
        };

        let side_by_side = InlineEditsSideBySideView::new(
            Rc::clone(&editor),
            inline_edit_of(&model),
            Rc::clone(&preview_text_model),
            ui_state.map(|state, _| {
                state.filter(|state| state.state.detail.kind() == InlineCompletionViewKind::SideBySide).map(|state| {
                    Rc::new(SideBySideUiState { new_text_line_count: state.new_text_line_count, editor_type: state.editor_type })
                })
            }),
            Rc::clone(&tab_action),
            Rc::clone(&services.theme_service),
            Rc::clone(&services.side_by_side_host),
            &*services.embedded_code_editor_factory,
        );
        base.register(side_by_side.clone());

        let deletion = InlineEditsDeletionView::new(
            Rc::clone(&editor),
            inline_edit_of(&model),
            ui_state.map(|state, _| {
                let state = state?;
                match &state.state.detail {
                    RenderDetail::Deletion { original_range, deletions } => {
                        Some(Rc::new(DeletionViewUiState { original_range: *original_range, deletions: deletions.clone(), editor_type: state.editor_type }))
                    }
                    _ => None,
                }
            }),
            Rc::clone(&tab_action),
        );
        base.register(deletion.clone());

        let insertion = services.views.create_insertion_view(
            Rc::clone(&editor),
            ui_state.map(|state, _| {
                let state = state?;
                match &state.state.detail {
                    RenderDetail::InsertionMultiLine { line_number, column, text } => Some(Rc::new(InsertionViewInput {
                        line_number: *line_number,
                        start_column: *column,
                        text: text.clone(),
                        editor_type: state.editor_type,
                    })),
                    _ => None,
                }
            }),
            Rc::clone(&tab_action),
        );
        base.register(insertion.clone());

        let inline_collapsed_view = InlineEditsCollapsedView::new(
            Rc::clone(&editor),
            {
                let ui_state = Rc::clone(&ui_state);
                model.map(move |model, reader| {
                    if ui_kind(&ui_state, reader) == Some(InlineCompletionViewKind::Collapsed) { model.map(|model| Rc::clone(&model.inline_edit)) } else { None }
                })
            },
            Rc::clone(&services.accessibility_service),
        );
        base.register(inline_collapsed_view.clone());

        let custom_view = InlineEditsCustomView::new(
            Rc::clone(&editor),
            {
                let ui_state = Rc::clone(&ui_state);
                model.map(move |model, reader| {
                    if ui_kind(&ui_state, reader) == Some(InlineCompletionViewKind::Custom) { model.and_then(|model| model.display_location.clone()) } else { None }
                })
            },
            Rc::clone(&tab_action),
            editor_type_of(&ui_state),
            Rc::clone(&services.theme_service),
            Rc::clone(&services.language_service),
        );
        base.register(custom_view.clone());

        let show_long_distance_hint = inline_suggest_options.map(|options, _| options.edits.show_long_distance_hint);
        let long_distance_hint: IObservable<Option<Rc<dyn IInlineEditsViewHandle>>> = {
            let (editor, preview_text_model, tab_action, ui_state, views) =
                (Rc::clone(&editor), Rc::clone(&preview_text_model), Rc::clone(&tab_action), Rc::clone(&ui_state), Rc::clone(&services.views));
            let (simple_model, inline_suggest_info) = (Rc::clone(&simple_model), Rc::clone(&inline_suggest_info));
            derived(move |reader| {
                if !show_long_distance_hint.read(reader) {
                    return None;
                }
                let view_state: IObservable<Option<Rc<LongDistanceViewState>>> = {
                    let (simple_model, inline_suggest_info) = (Rc::clone(&simple_model), Rc::clone(&inline_suggest_info));
                    ui_state.map(move |state, reader| {
                        let state = state?;
                        let hint = state.long_distance_hint?;
                        Some(Rc::new(LongDistanceViewState {
                            hint,
                            new_text_line_count: state.new_text_line_count,
                            edit: Rc::clone(&state.edit),
                            diff: state.diff.clone(),
                            editor_type: state.editor_type,
                            model: simple_model.read(reader)?,
                            inline_suggest_info: inline_suggest_info.read(reader)?,
                            next_cursor_position: state.next_cursor_position,
                            target: Rc::clone(&state.target),
                        }))
                    })
                };
                let view = views.create_long_distance_hint(Rc::clone(&editor), view_state, Rc::clone(&preview_text_model), Rc::clone(&tab_action));
                reader.store().add(view.clone());
                Some(view)
            })
            .recompute_initially_and_on_change(base.store(), None)
        };

        let inline_diff_view_state: IObservable<Option<Rc<OriginalEditorInlineDiffViewState>>> = {
            let side_by_side = Rc::clone(&side_by_side);
            ui_state.map(move |state, _| {
                let state = state?;
                let mode = match state.state.detail.kind() {
                    InlineCompletionViewKind::SideBySide => OriginalEditorInlineDiffViewMode::SideBySide,
                    InlineCompletionViewKind::Deletion => OriginalEditorInlineDiffViewMode::Deletion,
                    InlineCompletionViewKind::InsertionInline => OriginalEditorInlineDiffViewMode::InsertionInline,
                    InlineCompletionViewKind::LineReplacement => OriginalEditorInlineDiffViewMode::LineReplacement,
                    _ => return None,
                };
                Some(Rc::new(OriginalEditorInlineDiffViewState {
                    modified_text: Rc::new(StringText::new(state.new_text.clone())),
                    diff: state.diff.clone(),
                    mode,
                    modified_code_editor: Rc::clone(side_by_side.preview_editor().editor()),
                    editor_type: state.editor_type,
                }))
            })
        };
        let inline_diff_view = OriginalEditorInlineDiffView::new(Rc::clone(&editor), inline_diff_view_state, Rc::clone(&preview_text_model));
        base.register(inline_diff_view.clone());

        let jump_to_view = JumpToView::new(
            Rc::clone(&editor_obs),
            JumpToViewOptions { style: JumpToViewStyle::Label },
            ui_state.map(|state, _| {
                let state = state?;
                match &state.state.detail {
                    RenderDetail::JumpTo { position } => Some(Rc::new(JumpToViewData { jump_to_position: *position })),
                    _ => None,
                }
            }),
            Rc::clone(&services.theme_service),
            Rc::clone(&services.keybinding_service),
            Rc::clone(&services.context_key_service),
        );
        base.register(jump_to_view.clone());

        let word_replacements: IObservable<Rc<Vec<Rc<WordReplacementsViewData>>>> = {
            let ui_state = Rc::clone(&ui_state);
            derived_opts(
                DerivedOptions {
                    equals_fn: Some(Rc::new(|a: &Rc<Vec<Rc<WordReplacementsViewData>>>, b: &Rc<Vec<Rc<WordReplacementsViewData>>>| {
                        a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| a.equals(b))
                    })),
                    ..DerivedOptions::default()
                },
                move |reader| {
                    let Some(state) = ui_state.read(reader) else {
                        return Rc::new(Vec::new());
                    };
                    match &state.state.detail {
                        RenderDetail::WordReplacements { replacements, alternative_action } => Rc::new(
                            replacements
                                .iter()
                                .map(|replacement| Rc::new(WordReplacementsViewData::new(replacement.clone(), state.editor_type, alternative_action.clone())))
                                .collect(),
                        ),
                        _ => Rc::new(Vec::new()),
                    }
                },
            )
        };
        let word_replacement_views: IObservable<Rc<Vec<Rc<InlineEditsWordReplacementView>>>> = {
            let (editor_obs, tab_action) = (Rc::clone(&editor_obs), Rc::clone(&tab_action));
            let (language_service, theme_service, keybinding_service) =
                (Rc::clone(&services.language_service), Rc::clone(&services.theme_service), Rc::clone(&services.keybinding_service));
            map_observable_array_cached_by(
                None,
                word_replacements,
                move |view_data: &Rc<WordReplacementsViewData>, store| {
                    store.add_rc(InlineEditsWordReplacementView::new(
                        Rc::clone(&editor_obs),
                        (**view_data).clone(),
                        Rc::clone(&tab_action),
                        Rc::clone(&language_service),
                        Rc::clone(&theme_service),
                        Rc::clone(&keybinding_service),
                    ))
                },
                |view_data| Rc::as_ptr(view_data) as usize,
            )
        };
        let line_replacement_view = InlineEditsLineReplacementView::new(
            Rc::clone(&editor_obs),
            ui_state.map(|state, _| {
                let state = state?;
                match &state.state.detail {
                    RenderDetail::LineReplacement(edit) => Some(Rc::new(edit.clone())),
                    _ => None,
                }
            }),
            editor_type_of(&ui_state),
            Rc::clone(&tab_action),
            Rc::clone(&services.language_service),
            Rc::clone(&services.theme_service),
        );
        base.register(line_replacement_view.clone());

        base.register(autorun({
            let (model, logic) = (Rc::clone(&model), Rc::clone(&logic));
            let (side_by_side, line_replacement_view, insertion, word_replacement_views, inline_diff_view, custom_view) = (
                Rc::clone(&side_by_side),
                Rc::clone(&line_replacement_view),
                Rc::clone(&insertion),
                Rc::clone(&word_replacement_views),
                Rc::clone(&inline_diff_view),
                Rc::clone(&custom_view),
            );
            move |reader| {
                let Some(model) = model.read(reader) else {
                    return;
                };
                let mut events: Vec<Event<InlineEditClickEvent>> = vec![side_by_side.on_did_click(), line_replacement_view.on_did_click(), insertion.on_did_click()];
                events.extend(word_replacement_views.read(reader).iter().map(|view| view.on_did_click()));
                events.push(inline_diff_view.on_did_click());
                events.push(custom_view.on_did_click());
                let logic = Rc::clone(&logic);
                reader.store().add(event::any(events).listen(move |click_event| {
                    if logic.view_has_been_shown_longer_than(350.0) {
                        model.accept(click_event.alternative_action);
                    }
                }));
            }
        }));

        word_replacement_views.recompute_initially_and_on_change(base.store(), None);

        let min_editor_scroll_height: IObservable<f64> = {
            let (word_replacement_views, line_replacement_view, custom_view) =
                (Rc::clone(&word_replacement_views), Rc::clone(&line_replacement_view), Rc::clone(&custom_view));
            derived(move |reader| {
                let line_replacement = line_replacement_view.min_editor_scroll_height().map_or(0.0, |height| height.read(reader));
                let custom = custom_view.min_editor_scroll_height().map_or(0.0, |height| height.read(reader));
                word_replacement_views
                    .read(reader)
                    .iter()
                    .map(|view| view.min_editor_scroll_height().map_or(0.0, |height| height.read(reader)))
                    .fold(f64::NEG_INFINITY, f64::max)
                    .max(line_replacement)
                    .max(custom)
            })
            .recompute_initially_and_on_change(base.store(), None)
        };

        let view_zone_id: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        base.register(autorun({
            let (editor, editor_obs) = (Rc::clone(&editor), Rc::clone(&editor_obs));
            move |reader| {
                let min_scroll_height = min_editor_scroll_height.read(reader);
                let Some(text_model) = editor_obs.model.read(reader) else {
                    return;
                };

                editor.change_view_zones(&mut |accessor: &mut dyn IViewZoneChangeAccessor| {
                    let scroll_height = editor.get_scroll_height();
                    let view_zone_height = min_scroll_height - scroll_height + 1.0; /* Add 1px so there is a small gap */

                    if view_zone_height != 0.0 {
                        if let Some(id) = view_zone_id.borrow_mut().take() {
                            accessor.remove_zone(&id);
                        }
                    }

                    if view_zone_height <= 0.0 {
                        return;
                    }

                    let id = accessor.add_zone(Rc::new(MinScrollHeightViewZone {
                        after_line_number: CursorSimpleModel::get_line_count(&*text_model),
                        height_in_px: view_zone_height,
                    }));
                    *view_zone_id.borrow_mut() = Some(id);
                });
            }
        }));

        let display_range: IObservable<Option<LineRange>> = {
            let (ui_state, editor_obs, insertion) = (Rc::clone(&ui_state), Rc::clone(&editor_obs), Rc::clone(&insertion));
            derived(move |reader| {
                let state = ui_state.read(reader)?;
                let editor_model = editor_obs.model.read(reader)?;
                if state.target.uri().to_string(false) != editor_model.uri().to_string(false) {
                    return None;
                }

                match &state.state.detail {
                    RenderDetail::Custom { display_location } => {
                        let Some(range) = display_location.as_ref().map(|display_location| display_location.range) else {
                            errors::throw(Error::bug_indicating(Some("custom view should have a range")));
                        };
                        Some(LineRange::new(range.start_line_number, range.end_line_number))
                    }
                    RenderDetail::InsertionMultiLine { .. } => insertion.original_lines().read(reader),
                    _ => Some(state.edit.display_range()),
                }
            })
        };

        let inline_edits_is_hovered: IObservable<bool> = {
            let (side_by_side, word_replacement_views, deletion, inline_diff_view, line_replacement_view, insertion, custom_view, long_distance_hint) = (
                Rc::clone(&side_by_side),
                Rc::clone(&word_replacement_views),
                Rc::clone(&deletion),
                Rc::clone(&inline_diff_view),
                Rc::clone(&line_replacement_view),
                Rc::clone(&insertion),
                Rc::clone(&custom_view),
                Rc::clone(&long_distance_hint),
            );
            derived(move |reader| {
                side_by_side.is_hovered().read(reader)
                    || word_replacement_views.read(reader).iter().any(|view| view.is_hovered().read(reader))
                    || deletion.is_hovered().read(reader)
                    || inline_diff_view.is_hovered().read(reader)
                    || line_replacement_view.is_hovered().read(reader)
                    || insertion.is_hovered().read(reader)
                    || custom_view.is_hovered().read(reader)
                    || long_distance_hint.map(|view, reader| view.is_some_and(|view| view.is_hovered().read(reader))).read(reader)
            })
        };

        let gutter_indicator_offset: IObservable<f64> = {
            let (ui_state, insertion) = (Rc::clone(&ui_state), Rc::clone(&insertion));
            derived(move |reader| {
                // TODO: have a better way to tell the gutter indicator view where the edit is inside a viewzone
                if ui_kind(&ui_state, reader) == Some(InlineCompletionViewKind::InsertionMultiLine) {
                    return insertion.start_line_offset().read(reader);
                }
                0.0
            })
        };

        Rc::new(Self {
            base,
            side_by_side,
            deletion,
            insertion,
            inline_collapsed_view,
            custom_view,
            long_distance_hint,
            inline_diff_view,
            word_replacement_views,
            line_replacement_view,
            jump_to_view,
            preview_text_model,
            ui_state,
            display_range,
            inline_edits_is_hovered,
            gutter_indicator_offset,
        })
    }
}

impl IDisposable for InlineEditsView {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `diff.flatMap(d => d.innerChanges ?? [])`.
fn inner_changes(diff: &[DetailedLineRangeMapping]) -> Vec<RangeMapping> {
    diff.iter().flat_map(|d| d.inner_changes.iter().flatten()).copied().collect()
}

/// `v.includes('\t')`.
fn includes_tab(value: &JsString) -> bool {
    value.units().contains(&u16::from(b'\t'))
}

/// `createEmptyViewData`.
fn create_empty_view_data() -> InlineCompletionViewData {
    InlineCompletionViewData::new(-1, -1, -1, -1, -1, -1, -1, Some(true))
}

/// `getViewData(inlineEdit, stringChanges, textModel)`.
fn get_view_data(inline_edit: &InlineEditWithChanges, string_changes: &[StringChange], text_model: &TextModel) -> InlineCompletionViewData {
    let Some(edit) = &inline_edit.edit else {
        return create_empty_view_data();
    };

    let cursor_position = inline_edit.cursor_position;
    let line_edit = inline_edit.line_edit();
    let starts_with_eol = string_changes.first().is_some_and(|change| starts_with_at(change.modified.units(), text_model.get_eol().as_units(), 0));
    InlineCompletionViewData::new(
        edit.replacements.first().map_or(0, |replacement| replacement.range.get_start_position().column - cursor_position.column),
        line_edit.line_range.start_line_number - cursor_position.line_number
            + i32::from(starts_with_eol && line_edit.line_range.start_line_number >= cursor_position.line_number),
        line_edit.line_range.length(),
        line_edit.new_lines.len() as i32,
        string_changes.iter().map(|change| change.original.len() as i32).sum(),
        string_changes.iter().map(|change| change.modified.len() as i32).sum(),
        string_changes.len() as i32,
        Some(string_changes.iter().all(|change| change.original == string_changes[0].original && change.modified == string_changes[0].modified)),
    )
}

/// `isSingleLineInsertion(diff)`.
fn is_single_line_insertion(diff: &[DetailedLineRangeMapping]) -> bool {
    diff.iter().all(|m| m.inner_changes.iter().flatten().all(is_word_insertion))
}

/// `isWordInsertion(r)`.
fn is_word_insertion(r: &RangeMapping) -> bool {
    if !r.original_range.is_empty() {
        return false;
    }
    let is_insertion_within_line = r.modified_range.start_line_number == r.modified_range.end_line_number;
    if !is_insertion_within_line {
        return false;
    }
    true
}

/// `isSingleLineInsertionAfterPosition(diff, position)`.
fn is_single_line_insertion_after_position(diff: &[DetailedLineRangeMapping], position: Option<&Position>) -> bool {
    let Some(pos) = position else {
        return false;
    };

    if !is_single_line_insertion(diff) {
        return false;
    }

    diff.iter().all(|m| m.inner_changes.iter().flatten().all(|r| is_stable_word_insertion(pos, r)))
}

/// `isStableWordInsertion(r)`.
fn is_stable_word_insertion(pos: &Position, r: &RangeMapping) -> bool {
    let insert_position = r.original_range.get_start_position();
    if pos.is_before_or_equal(&insert_position) {
        return true;
    }
    if insert_position.line_number < pos.line_number {
        return true;
    }
    false
}

/// `isSingleMultiLineInsertion(diff)`.
fn is_single_multi_line_insertion(diff: &[DetailedLineRangeMapping]) -> bool {
    let inner = inner_changes(diff);
    if inner.len() != 1 {
        return false;
    }

    let change = &inner[0];
    if !change.original_range.is_empty() {
        return false;
    }

    if change.modified_range.start_line_number == change.modified_range.end_line_number {
        return false;
    }

    true
}

/// `isDeletion(inner, inlineEdit, newText)`: o `inlineEdit.originalText` chega como `original_text`.
fn is_deletion(inner: &[RangeMapping], original_text: &dyn AbstractText, new_text: &dyn AbstractText) -> bool {
    inner.iter().all(|m| {
        let original = original_text.get_value_of_range(&m.original_range);
        let modified = new_text.get_value_of_range(&m.modified_range);
        trim(modified.units()).is_empty() && !original.is_empty() && (original.len() > modified.len() || !trim(original.units()).is_empty())
    })
}

/// `growEditsToEntireWord(replacements, originalText)`: `/^[a-zA-Z]$/.test(char)`.
fn grow_edits_to_entire_word(replacements: &[TextReplacement], original_text: &dyn AbstractText) -> Vec<TextReplacement> {
    grow_edits(replacements, original_text, |unit| matches!(unit, 0x41..=0x5A | 0x61..=0x7A))
}

/// `growEditsUntilWhitespace(replacements, originalText)`: `!(/^\s$/.test(char))`.
fn grow_edits_until_whitespace(replacements: &[TextReplacement], original_text: &dyn AbstractText) -> Vec<TextReplacement> {
    grow_edits(replacements, original_text, |unit| !is_js_whitespace(unit))
}

/// `_growEdits(replacements, originalText, fn)`. O original ordena o vetor recebido no lugar; aqui a ordenação é de uma
/// cópia (quem chama nunca relê o vetor ordenado).
fn grow_edits(replacements: &[TextReplacement], original_text: &dyn AbstractText, include: fn(u16) -> bool) -> Vec<TextReplacement> {
    let mut result: Vec<TextReplacement> = Vec::new();

    let mut sorted = replacements.to_vec();
    sorted.sort_by(|a, b| Range::compare_ranges_using_starts(Some(&a.range), Some(&b.range)).cmp(&0));

    // `isIncluded(c)`: um índice fora da linha (o `undefined` do original) nunca entra.
    let is_included = |c: Option<u16>| c.is_some_and(include);

    for edit in sorted {
        let mut start_index = i64::from(edit.range.start_column) - 1;
        let mut end_index = i64::from(edit.range.end_column) - 2;
        let mut prefix: Vec<u16> = Vec::new();
        let mut suffix: Vec<u16> = Vec::new();
        let start_line_content = original_text.get_line_at(edit.range.start_line_number);
        let end_line_content = original_text.get_line_at(edit.range.end_line_number);

        if is_included(char_code_at(start_line_content.units(), start_index)) {
            // grow to the left
            while let Some(unit) = char_code_at(start_line_content.units(), start_index - 1).filter(|unit| include(*unit)) {
                prefix.insert(0, unit);
                start_index -= 1;
            }
        }

        if is_included(char_code_at(end_line_content.units(), end_index)) || end_index < start_index {
            // grow to the right
            while let Some(unit) = char_code_at(end_line_content.units(), end_index + 1).filter(|unit| include(*unit)) {
                suffix.push(unit);
                end_index += 1;
            }
        }

        // create new edit and merge together if they are touching
        let mut text = prefix;
        text.extend_from_slice(edit.text.units());
        text.extend(suffix);
        let mut new_edit = TextReplacement::new(
            Range::new(edit.range.start_line_number, (start_index + 1) as i32, edit.range.end_line_number, (end_index + 2) as i32),
            JsString::from_units(text),
        );
        if result.last().is_some_and(|last| Range::are_intersecting_or_touching(&last.range, &new_edit.range)) {
            let last = result.pop().expect("conferido acima");
            new_edit = TextReplacement::join_replacements(&[last, new_edit], original_text);
        }

        result.push(new_edit);
    }

    result
}
