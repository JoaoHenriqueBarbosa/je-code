//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/model/inlineCompletionsModel.ts`: o modelo observável das
//! sugestões em linha (estado, sugestão selecionada, aceitar, aceitar em parte, saltar, rejeitar) sobre o
//! `InlineCompletionsSource`, os observáveis de `je_base` e o `CodeEditorWidget`.
//!
//! Notas de tradução:
//! - `Disposable` (classe base) é o `Rc<Disposable>` guardado em `base`; `this._store` é o mesmo `base`
//!   (`register`), e o `FadeoutDecoration` aceito vive num `DisposableStore` à parte que a base descarta;
//! - o modelo é um `Rc<Self>` criado por `Rc::new_cyclic`: os métodos que o original chama em `this` de dentro de
//!   callbacks (`trigger` ao mudar o provedor, por exemplo) pegam um `Weak` do próprio modelo. Os `async` do original
//!   (`trigger`, `accept`, `next`...) são [`async_fn`] sobre [`Promise`]; o `try/finally` do `accept` é o corpo numa
//!   expressão `async` seguida do bloco de limpeza, que roda no sucesso e no erro;
//! - `derivedHandleChanges` com `changeSummary` vira [`derived_handle_changes`] com um resumo de campos `Cell` (o
//!   rastreador recebe `&S`); os `Signal` são [`ObservableSignal`] e os que levam dado (`_fetchSpecificProviderSignal`)
//!   carregam o [`ProviderChange`] como mudança;
//! - a igualdade `===` de objetos do JavaScript vira a identidade da sugestão (a `InlineSuggestionIdentity` por
//!   endereço mais o `hash` da ação), porque os itens deste porte são valores clonáveis: um item refeito por `withEdit`
//!   com outra ação é outro objeto, como no original ([`same_item`]). As implementações de [`StrictEquals`] dos tipos do
//!   editor que os observáveis precisam (`Position`, `GhostTextMode`...) ficam na seção "Igualdade estrita";
//! - o `observableCodeEditor(editor)` ainda não tem porte: os observáveis que o modelo lê dele (`getOption`, `versionId`,
//!   `selections`, `cursorLineNumber`, `inComposition`, `scrollTop`) são montados aqui com `observable_from_event` sobre
//!   os eventos do `CodeEditorWidget`; o `openedPeekWidgets` (que vem do `ReferencesController`) é injetado;
//! - colaboradores sem porte entram como traits injetados no [`InlineCompletionsModelServices`]: o
//!   `IInlineCompletionsService` ([`IInlineCompletionsService`]), o `IDefaultAccountService` ([`IDefaultAccountService`])
//!   e a parte do `ICodeEditorService`, do `getInlineCompletionsController` e da animação CSS `FadeoutDecoration` que o
//!   modelo usa ([`InlineCompletionsEditorHost`]). O `SuggestItemInfo` é [`SuggestItemInfo`] (a fábrica `fromSuggestion`
//!   é do `SuggestWidgetAdaptor`, que fica com a camada do controller);
//! - `candidateInlineCompletions` do `_computeAugmentation` sempre sai do estado do suggest widget, porque o estado
//!   (`InlineCompletionsState`) nunca é indefinido (o ramo `[selectedInlineCompletion]` do original é inalcançável);
//! - `selectedInlineCompletionIndex` mantém o teste `this._selectedInlineCompletionId === undefined` do original, que
//!   compara o observável e não o valor (nunca é verdadeiro), então a busca por identificador sempre roda;
//! - `Date.now()` é [`date_now`] (milissegundos); a expressão regular de palavra do `acceptNextWord` roda com o
//!   `lastIndex` zerado (sem a flag `g`, o `exec` do JavaScript ignora o `lastIndex`).

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::errors::{Error, on_unexpected_external_error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::dynamic::Dynamic;
use je_base::network::schemas;
use je_base::observable::base::{AnyObservable, IObservable, IReader, ITransaction, Observable};
use je_base::observable::change_tracker::{FnChangeTracker, IChangeContext};
use je_base::observable::debug_name::DebugNameOptions;
use je_base::observable::deps::EqualityComparer;
use je_base::observable::observables::derived::{DerivedHandleChangesOptions, DerivedOptions, derived, derived_handle_changes, derived_opts};
use je_base::observable::observables::observable_from_event::{FromEventOptions, observable_from_event, observable_from_event_opts};
use je_base::observable::observables::observable_signal::{ObservableSignal, observable_signal};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::utils::utils::{map_observable_array_cached_by, recompute_initially_and_on_change};
use je_base::observable::{ObservableExt, SettableExt, StrictEquals, autorun, observable_value, subtransaction, transaction};
use je_base::promise::Promise;
use je_base::runtime::date_now;
use je_base::uri::{URI, UriComponents};
use je_base::JsString;
use je_platform::accessibility::IAccessibilityService;
use je_platform::commands::ICommandService;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::inline_suggest::{INLINE_SUGGEST, InlineSuggestMode, InternalInlineSuggestOptions, ShowOnSuggestConflict};
use crate::config::editor_options::suggest::{InternalSuggestOptions, SUGGEST, SuggestPreviewMode};
use crate::config::editor_options::Opt;
use crate::cursor_columns::visible_column_from_column;
use crate::cursor_common::CursorSimpleModel;
use crate::edit_operation::SingleEditOperation;
use crate::editor_common::ScrollType;
use crate::edits::string_edit::{BaseStringReplacement, StringReplacement};
use crate::edits::text_edit::{TextEdit, TextReplacement};
use crate::inline_completions::compute_ghost_text::{
    GhostTextMode, compute_ghost_text, single_text_edit_augments, single_text_remove_common_prefix,
};
use crate::inline_completions::ghost_text::{
    GhostText, GhostTextOrReplacement, ghost_text_or_replacement_equals, ghost_texts_or_replacements_equal,
};
use crate::inline_completions::inline_completion_view_data::{InlineCompletionViewData, InlineCompletionViewKind};
use crate::inline_completions::inline_completions_source::{InlineCompletionsSource, InlineCompletionsSourceServices, InlineCompletionsState};
use crate::inline_completions::inline_suggestion_item::{
    InlineCompletionItem, InlineEditItem, InlineSuggestionAction, InlineSuggestionItem, InlineSuggestionItemBase,
};
use crate::inline_completions::provide_inline_completions::{
    InlineCompletionContextWithoutUuid, InlineCompletionEditorType, InlineSuggestRequestInfo, InlineSuggestSku, PartialAcceptance,
};
use crate::inline_completions::typing_speed::TypingInterval;
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::language_feature_debounce::IFeatureDebounceInformation;
use crate::language_features_service::LanguageFeaturesService;
use crate::languages::{
    CompletionItemKind, IInlineCompletionChangeHint, InlineCompletion, InlineCompletionCommand, InlineCompletionEndOfLifeReason,
    InlineCompletionTriggerKind, InlineCompletionsProvider, PartialAcceptInfo, PartialAcceptTriggerKind, SelectedSuggestionInfo,
};
use crate::line_range::LineRange;
use crate::model::EndOfLinePreference;
use crate::position::Position;
use crate::position_offset_transformer_with_text_model::get_position_offset_transformer_from_text_model;
use crate::position_to_offset::{PositionOffsetTransformerBase, TextLength};
use crate::range::Range;
use crate::selection::Selection;
use crate::snippet::snippet_controller2::{PartialSnippetInsertOptions, SnippetController2};
use crate::text::abstract_text::AbstractText;
use crate::text_model::TextModel;
use crate::text_model_edit_source::{InlineCompletionAcceptData, InlineCompletionPartialAcceptData, TextModelEditSource};
use crate::text_model_events::ModelContentChangedEvent;
use crate::text_model_text::TextModelText;

// #region Igualdade estrita

macro_rules! strict_equals_by_discriminant {
    ($($t:ty),* $(,)?) => {$(
        impl StrictEquals for $t {
            fn strict_equals(&self, other: &Self) -> bool {
                std::mem::discriminant(self) == std::mem::discriminant(other)
            }
        }
    )*};
}

strict_equals_by_discriminant!(GhostTextMode, ShowOnSuggestConflict, InlineCompletionTriggerKind, ModelStatus);

impl StrictEquals for Position {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

impl StrictEquals for InlineSuggestSku {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

/// O `===` de dois itens de sugestão: a mesma identidade e a mesma ação (ver as notas do módulo).
fn same_item<T: InlineSuggestionItemBase>(a: &T, b: &T) -> bool {
    Rc::ptr_eq(&a.base().identity, &b.base().identity) && a.hash() == b.hash()
}

impl StrictEquals for InlineCompletionItem {
    fn strict_equals(&self, other: &Self) -> bool {
        same_item(self, other)
    }
}

impl StrictEquals for InlineEditItem {
    fn strict_equals(&self, other: &Self) -> bool {
        same_item(self, other)
    }
}

// #endregion

// #region Colaboradores injetados

/// `IInlineCompletionsService`, só com o que o modelo usa.
pub trait IInlineCompletionsService {
    /// `onDidChangeIsSnoozing`.
    fn on_did_change_is_snoozing(&self) -> Event<bool>;
    /// `isSnoozing()`.
    fn is_snoozing(&self) -> bool;
    /// `reportNewCompletion(requestUuid)`.
    fn report_new_completion(&self, request_uuid: &str);
}

/// `IDefaultAccount['entitlementsData']`, só com os campos que `skuFromAccount` lê.
#[derive(Clone, Debug, Default)]
pub struct DefaultAccountEntitlements {
    pub access_type_sku: Option<String>,
    pub copilot_plan: Option<String>,
}

/// `IDefaultAccount`, só com o que o modelo usa.
#[derive(Clone, Debug, Default)]
pub struct DefaultAccount {
    pub entitlements_data: Option<DefaultAccountEntitlements>,
}

/// `IDefaultAccountService`, só com o que o modelo usa.
pub trait IDefaultAccountService {
    /// `getDefaultAccount()`.
    fn get_default_account(&self) -> Promise<Option<DefaultAccount>>;
    /// `onDidChangeDefaultAccount`.
    fn on_did_change_default_account(&self) -> Event<Option<DefaultAccount>>;
}

/// O que o modelo pede ao `ICodeEditorService`, ao `getInlineCompletionsController` (a camada do controller) e à
/// animação `edits-fadeout-decoration` (um estilo, não pintura nativa).
pub trait InlineCompletionsEditorHost {
    /// `listDiffEditors().some(d => d.getOriginalEditor().getId() === id || d.getModifiedEditor().getId() === id)`.
    fn is_in_diff_editor(&self, editor_id: &str) -> bool;

    /// `(getFocusedCodeEditor() || getActiveCodeEditor()) === editor`.
    fn is_focused_or_active_editor(&self, editor: &Rc<CodeEditorWidget>) -> bool;

    /// `openCodeEditor({ resource }, sourceEditor)`.
    fn open_code_editor(&self, resource: URI, source_editor: &Rc<CodeEditorWidget>) -> Promise<Option<Rc<CodeEditorWidget>>>;

    /// `getInlineCompletionsController(editor)?.model.get()`.
    fn inline_completions_model_of(&self, editor: &Rc<CodeEditorWidget>) -> Option<Rc<InlineCompletionsModel>>;

    /// O `FadeoutDecoration`: realça `ranges` e some sozinho em 1 segundo. `on_finished` avisa o fim, para o modelo soltar
    /// o descartável.
    fn start_edits_fadeout(&self, editor: &Rc<CodeEditorWidget>, ranges: Vec<Range>, on_finished: Rc<dyn Fn()>) -> DisposableRef;
}

/// Os serviços que o construtor recebe por injeção (os `@I...` do original e o que o `product` e o `observableCodeEditor`
/// davam).
pub struct InlineCompletionsModelServices {
    pub command_service: Rc<dyn ICommandService>,
    pub language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    pub accessibility_service: Rc<dyn IAccessibilityService>,
    pub language_features_service: Rc<LanguageFeaturesService>,
    pub inline_completions_service: Rc<dyn IInlineCompletionsService>,
    pub default_account_service: Rc<dyn IDefaultAccountService>,
    pub host: Rc<dyn InlineCompletionsEditorHost>,
    /// `editorObs.openedPeekWidgets`.
    pub opened_peek_widgets: IObservable<i32>,
    /// `SnippetController2.get(editor)?.isInSnippetObservable`; sem o controller, `constObservable(false)`.
    pub is_in_snippet_mode: IObservable<bool>,
    /// `product.defaultChatAgent?.completionsEnablementSetting`.
    pub completions_enablement_setting: Option<String>,
    /// Os serviços do `InlineCompletionsSource`.
    pub source_services: InlineCompletionsSourceServices,
}

// #endregion

/// `SuggestItemInfo` (`suggestWidgetAdapter.ts`).
#[derive(Clone, Debug)]
pub struct SuggestItemInfo {
    pub range: Range,
    pub insert_text: JsString,
    pub completion_item_kind: CompletionItemKind,
    pub is_snippet_text: bool,
    pub list_incomplete: bool,
}

impl SuggestItemInfo {
    pub fn new(range: Range, insert_text: JsString, completion_item_kind: CompletionItemKind, is_snippet_text: bool, list_incomplete: bool) -> Self {
        Self { range, insert_text, completion_item_kind, is_snippet_text, list_incomplete }
    }

    pub fn equals(&self, other: &SuggestItemInfo) -> bool {
        self.range == other.range
            && self.insert_text == other.insert_text
            && std::mem::discriminant(&self.completion_item_kind) == std::mem::discriminant(&other.completion_item_kind)
            && self.is_snippet_text == other.is_snippet_text
    }

    /// `toSelectedSuggestionInfo()`.
    pub fn to_selected_suggestion_info(&self) -> SelectedSuggestionInfo {
        SelectedSuggestionInfo::new(self.range, self.insert_text.to_string_lossy(), self.completion_item_kind, self.is_snippet_text)
    }

    /// `getSingleTextEdit()`.
    pub fn get_single_text_edit(&self) -> TextReplacement {
        TextReplacement::new(self.range, self.insert_text.clone())
    }
}

/// O valor de `state` quando há uma sugestão de texto fantasma.
#[derive(Clone)]
pub struct GhostTextModelState {
    pub edits: Vec<TextReplacement>,
    pub primary_ghost_text: GhostTextOrReplacement,
    pub ghost_texts: Vec<GhostTextOrReplacement>,
    pub suggest_item: Option<Rc<SuggestItemInfo>>,
    pub inline_suggestion: Option<InlineCompletionItem>,
}

/// O valor de `state` quando há uma edição em linha.
#[derive(Clone)]
pub struct InlineEditModelState {
    pub edits: Vec<TextReplacement>,
    pub inline_suggestion: InlineEditItem,
    pub cursor_at_inline_edit: IObservable<bool>,
    pub next_edit_uri: Option<URI>,
}

/// O valor de `state`: `{ kind: 'ghostText', ... } | { kind: 'inlineEdit', ... }`.
#[derive(Clone)]
pub enum InlineCompletionsModelState {
    GhostText(GhostTextModelState),
    InlineEdit(InlineEditModelState),
}

impl InlineCompletionsModelState {
    /// `edits`.
    pub fn edits(&self) -> &[TextReplacement] {
        match self {
            Self::GhostText(s) => &s.edits,
            Self::InlineEdit(s) => &s.edits,
        }
    }

    /// `inlineSuggestion` (indefinido num estado de texto fantasma vindo só do suggest widget).
    pub fn inline_suggestion(&self) -> Option<InlineSuggestionItem> {
        match self {
            Self::GhostText(s) => s.inline_suggestion.clone().map(InlineSuggestionItem::Completion),
            Self::InlineEdit(s) => Some(InlineSuggestionItem::Edit(s.inline_suggestion.clone())),
        }
    }

    /// O `equalsFn` do `state`.
    fn equals(a: &Option<Rc<Self>>, b: &Option<Rc<Self>>) -> bool {
        let (Some(a), Some(b)) = (a, b) else {
            return a.is_none() && b.is_none();
        };
        match (&**a, &**b) {
            (Self::GhostText(a), Self::GhostText(b)) => {
                ghost_texts_or_replacements_equal(Some(&a.ghost_texts), Some(&b.ghost_texts))
                    && match (&a.inline_suggestion, &b.inline_suggestion) {
                        (None, None) => true,
                        (Some(x), Some(y)) => same_item(x, y),
                        _ => false,
                    }
                    && match (&a.suggest_item, &b.suggest_item) {
                        (None, None) => true,
                        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
                        _ => false,
                    }
            }
            (Self::InlineEdit(a), Self::InlineEdit(b)) => same_item(&a.inline_suggestion, &b.inline_suggestion),
            _ => false,
        }
    }
}

/// O resultado de `status`: `'loading' | 'ghostText' | 'inlineEdit' | 'noSuggestion'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelStatus {
    Loading,
    GhostText,
    InlineEdit,
    NoSuggestion,
}

/// A mudança do `_fetchSpecificProviderSignal`: `{ provider, changeHint? }`.
#[derive(Clone)]
pub struct ProviderChange {
    pub provider: Rc<dyn InlineCompletionsProvider>,
    pub change_hint: Option<IInlineCompletionChangeHint>,
}

/// As opções de `trigger`.
#[derive(Clone, Default)]
pub struct TriggerOptions {
    pub only_fetch_inline_edits: bool,
    pub no_delay: bool,
    pub provider: Option<Rc<dyn InlineCompletionsProvider>>,
    pub explicit: bool,
    pub change_hint: Option<IInlineCompletionChangeHint>,
}

/// `{ documentValue, inlineCompletion }` de `extractReproSample`.
pub struct Repro {
    pub document_value: JsString,
    pub inline_completion: Option<Rc<InlineCompletion>>,
}

/// `VersionIdChangeReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionIdChangeReason {
    Undo,
    Redo,
    AcceptWord,
    Other,
}

impl VersionIdChangeReason {
    /// `_preserveCurrentCompletionReasons`.
    fn preserves_current_completion(self) -> bool {
        matches!(self, Self::Redo | Self::Undo | Self::AcceptWord)
    }
}

/// `_lastShownInlineCompletionInfo`.
struct LastShownInfo {
    alternate_text_model_version_id: i64,
    /// Already freed!
    inline_completion: InlineSuggestionItem,
}

/// `_lastAcceptedInlineCompletionInfo`.
struct LastAcceptedInfo {
    text_model_version_id_after: i64,
    /// Already freed!
    inline_completion: InlineSuggestionItem,
}

/// O `changeSummary` do `_fetchInlineCompletionsPromise`.
struct FetchChangeSummary {
    dont_refetch: Cell<bool>,
    preserve_current_completion: Cell<bool>,
    inline_completion_trigger_kind: Cell<InlineCompletionTriggerKind>,
    only_request_inline_edits: Cell<bool>,
    should_debounce: Cell<bool>,
    provider: RefCell<Option<Rc<dyn InlineCompletionsProvider>>>,
    change_hint: RefCell<Option<IInlineCompletionChangeHint>>,
    text_change: Cell<bool>,
    change_reason: RefCell<String>,
}

impl FetchChangeSummary {
    fn new() -> Self {
        Self {
            dont_refetch: Cell::new(false),
            preserve_current_completion: Cell::new(false),
            inline_completion_trigger_kind: Cell::new(InlineCompletionTriggerKind::Automatic),
            only_request_inline_edits: Cell::new(false),
            should_debounce: Cell::new(true),
            provider: RefCell::new(None),
            change_hint: RefCell::new(None),
            text_change: Cell::new(false),
            change_reason: RefCell::new(String::new()),
        }
    }
}

/// O valor do `_fetchInlineCompletionsPromise` (`Promise<boolean> | undefined`), com a igualdade de identidade.
#[derive(Clone)]
struct FetchResult(Option<Promise<bool>>);

impl StrictEquals for FetchResult {
    fn strict_equals(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => a.ptr_eq(b),
            _ => false,
        }
    }
}

/// `{ inlineCompletions, inlineEdit }` do `_inlineSuggestionItems`.
struct SuggestionItems {
    inline_completions: Vec<InlineCompletionItem>,
    inline_edit: Option<InlineEditItem>,
}

/// `InlineCompletionsModel`.
pub struct InlineCompletionsModel {
    base: Rc<Disposable>,
    me: Weak<InlineCompletionsModel>,
    pub text_model: Rc<TextModel>,
    editor: Rc<CodeEditorWidget>,
    command_service: Rc<dyn ICommandService>,
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    accessibility_service: Rc<dyn IAccessibilityService>,
    language_features_service: Rc<LanguageFeaturesService>,
    inline_completions_service: Rc<dyn IInlineCompletionsService>,
    host: Rc<dyn InlineCompletionsEditorHost>,
    fadeouts: Rc<DisposableStore>,

    selected_suggest_item: IObservable<Option<Rc<SuggestItemInfo>>>,
    pub text_model_version_id: IObservable<Option<i64>>,
    positions: IObservable<Rc<Vec<Position>>>,
    enabled: IObservable<bool>,

    source: Rc<InlineCompletionsSource>,
    is_active: Rc<ObservableValue<bool, ()>>,
    only_request_inline_edits_signal: Rc<ObservableSignal<()>>,
    force_update_explicitly_signal: Rc<ObservableSignal<()>>,
    no_delay_signal: Rc<ObservableSignal<()>>,
    fetch_specific_provider_signal: Rc<ObservableSignal<ProviderChange>>,
    pub dont_refetch_signal: Rc<ObservableSignal<()>>,
    selected_inline_completion_id: Rc<ObservableValue<Option<String>, ()>>,
    jumped_to_id: Rc<ObservableValue<Option<String>, ()>>,
    in_accept_flow_value: Rc<ObservableValue<bool, ()>>,

    is_accepting_partially: Rc<Cell<bool>>,
    on_did_accept: Rc<Emitter<()>>,
    last_shown_inline_completion_info: Rc<RefCell<Option<LastShownInfo>>>,
    last_accepted_inline_completion_info: Rc<RefCell<Option<LastAcceptedInfo>>>,

    trigger_command_on_provider_change: IObservable<bool>,

    did_undo_inline_edits: IObservable<bool>,
    fetch_inline_completions_promise: IObservable<FetchResult>,
    inline_completion_items: IObservable<Rc<Vec<InlineCompletionItem>>>,

    pub primary_position: IObservable<Position>,
    pub all_positions: IObservable<Rc<Vec<Position>>>,
    pub selected_inline_completion_index: IObservable<usize>,
    pub selected_inline_completion: IObservable<Option<InlineCompletionItem>>,
    pub active_commands: IObservable<Rc<Vec<InlineCompletionCommand>>>,
    pub last_trigger_kind: IObservable<Option<InlineCompletionTriggerKind>>,
    pub inline_completions_count: IObservable<Option<i32>>,
    pub state: IObservable<Option<Rc<InlineCompletionsModelState>>>,
    pub status: IObservable<ModelStatus>,
    pub inline_completion_state: IObservable<Option<Rc<InlineCompletionsModelState>>>,
    pub inline_edit_state: IObservable<Option<Rc<InlineCompletionsModelState>>>,
    pub inline_edit_available: IObservable<bool>,
    pub warning: IObservable<Option<Rc<crate::languages::InlineCompletionWarning>>>,
    pub ghost_texts: IObservable<Option<Rc<Vec<GhostTextOrReplacement>>>>,
    pub primary_ghost_text: IObservable<Option<Rc<GhostTextOrReplacement>>>,
    pub show_collapsed: IObservable<bool>,
    pub tab_should_jump_to_inline_edit: IObservable<bool>,
    pub tab_should_accept_inline_edit: IObservable<bool>,
    /// `inAcceptFlow`.
    pub in_accept_flow: IObservable<bool>,
    pub is_in_diff_editor: bool,
    pub editor_type: InlineCompletionEditorType,
}

/// O `editorObs.getOption(opt)`: o valor da opção, que muda com `onDidChangeConfiguration`. O original devolve o mesmo
/// objeto enquanto a opção não muda; aqui a igualdade é a de valor.
fn option_observable<V: Clone + PartialEq + 'static>(editor: &Rc<CodeEditorWidget>, opt: impl Fn() -> Opt<V> + 'static) -> IObservable<Rc<V>> {
    let editor_for_value = Rc::clone(editor);
    observable_from_event_opts(
        FromEventOptions {
            equals_fn: Some(Rc::new(|a: &Rc<V>, b: &Rc<V>| **a == **b) as EqualityComparer<Rc<V>>),
            ..FromEventOptions::default()
        },
        editor.on_did_change_configuration(),
        move |_| Rc::new(editor_for_value.get_option(opt()).clone()),
    )
}

fn suggest_preview_mode_to_ghost(mode: &SuggestPreviewMode) -> GhostTextMode {
    match mode {
        SuggestPreviewMode::Prefix => GhostTextMode::Prefix,
        SuggestPreviewMode::Subword => GhostTextMode::Subword,
        SuggestPreviewMode::SubwordSmart => GhostTextMode::SubwordSmart,
    }
}

fn inline_suggest_mode_to_ghost(mode: &InlineSuggestMode) -> GhostTextMode {
    match mode {
        InlineSuggestMode::Prefix => GhostTextMode::Prefix,
        InlineSuggestMode::Subword => GhostTextMode::Subword,
        InlineSuggestMode::SubwordSmart => GhostTextMode::SubwordSmart,
    }
}

/// `ghostText.isEmpty()` de qualquer das duas variantes.
fn ghost_is_empty(ghost: &GhostTextOrReplacement) -> bool {
    match ghost {
        GhostTextOrReplacement::GhostText(g) => g.is_empty(),
        GhostTextOrReplacement::Replacement(r) => r.is_empty(),
    }
}

/// `ghostText.lineNumber` de qualquer das duas variantes.
fn ghost_line_number(ghost: &GhostTextOrReplacement) -> i32 {
    match ghost {
        GhostTextOrReplacement::GhostText(g) => g.line_number,
        GhostTextOrReplacement::Replacement(r) => r.line_number,
    }
}

/// `ghostText.parts` de qualquer das duas variantes.
fn ghost_parts(ghost: &GhostTextOrReplacement) -> &[crate::inline_completions::ghost_text::GhostTextPart] {
    match ghost {
        GhostTextOrReplacement::GhostText(g) => &g.parts,
        GhostTextOrReplacement::Replacement(r) => &r.parts,
    }
}

/// `URI.from(<any>value)` sobre o argumento do comando.
fn uri_from_dynamic(value: &Dynamic) -> Option<URI> {
    let component = |key: &str| match value.get_str(key) {
        Dynamic::String(s) => Some(s),
        _ => None,
    };
    let components = UriComponents {
        scheme: component("scheme"),
        authority: component("authority"),
        path: component("path"),
        query: component("query"),
        fragment: component("fragment"),
    };
    components.scheme.as_ref()?;
    Some(URI::from(&components, false))
}

/// `skuFromAccount(account)`.
fn sku_from_account(account: Option<&DefaultAccount>) -> Option<InlineSuggestSku> {
    let entitlements = account?.entitlements_data.as_ref()?;
    match (&entitlements.access_type_sku, &entitlements.copilot_plan) {
        (Some(kind), Some(plan)) if !kind.is_empty() && !plan.is_empty() => {
            Some(InlineSuggestSku { kind: kind.clone(), plan: plan.clone() })
        }
        _ => None,
    }
}

/// O `\s` do JavaScript.
fn is_js_whitespace(unit: u16) -> bool {
    matches!(unit, 0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF)
}

/// `getEndPositionsAfterApplying(edits)` (`utils.ts`).
pub fn get_end_positions_after_applying(edits: &[TextReplacement]) -> Vec<Position> {
    get_modified_ranges_after_applying(edits).iter().map(Range::get_end_position).collect()
}

/// `getModifiedRangesAfterApplying(edits)` (`utils.ts`): os ranges novos na ordem original das edições.
pub fn get_modified_ranges_after_applying(edits: &[TextReplacement]) -> Vec<Range> {
    let mut order: Vec<usize> = (0..edits.len()).collect();
    order.sort_by_key(|&i| {
        let r = &edits[i].range;
        (r.start_line_number, r.start_column, r.end_line_number, r.end_column)
    });
    let sorted: Vec<TextReplacement> = order.iter().map(|&i| edits[i].clone()).collect();
    let sorted_new_ranges = TextEdit::new(sorted).get_new_ranges();
    let mut result = vec![Range::new(1, 1, 1, 1); edits.len()];
    for (sorted_index, &original_index) in order.iter().enumerate() {
        result[original_index] = sorted_new_ranges[sorted_index];
    }
    result
}

/// `removeTextReplacementCommonSuffixPrefix(edits, textModel)` (`utils.ts`).
pub fn remove_text_replacement_common_suffix_prefix(edits: &[TextReplacement], text_model: &Rc<TextModel>) -> Vec<TextReplacement> {
    let transformer = get_position_offset_transformer_from_text_model(text_model);
    let text = text_model.get_value();
    edits
        .iter()
        .map(|edit| {
            let replacement: StringReplacement = transformer.get_string_replacement(edit);
            transformer.get_text_replacement(&replacement.remove_common_suffix_prefix(text.units()))
        })
        .collect()
}

/// `getSecondaryEdits(textModel, positions, primaryTextRepl)`: as edições dos cursores secundários que repetem a do
/// primário.
pub fn get_secondary_edits(text_model: &Rc<TextModel>, positions: &[Position], primary_text_repl: &TextReplacement) -> Vec<TextReplacement> {
    if positions.len() == 1 {
        // No secondary cursor positions
        return Vec::new();
    }
    let text = TextModelText::new(Rc::clone(text_model));
    let text_transformer = text.get_transformer();
    let primary_offset = text_transformer.get_offset(positions[0]);
    let secondary_offsets: Vec<i32> = positions[1..].iter().map(|pos| text_transformer.get_offset(*pos)).collect();

    let primary_text_repl = primary_text_repl.remove_common_prefix_and_suffix(&text);
    let primary_string_repl = text_transformer.get_string_replacement(&primary_text_repl);

    let delta_from_offset_to_range_start = primary_string_repl.replace_range.start - primary_offset;
    let primary_context_range = primary_string_repl.replace_range.join(crate::offset_range::OffsetRange::empty_at(primary_offset));
    let primary_context_value = text.get_value_of_offset_range(primary_context_range);

    secondary_offsets
        .into_iter()
        .filter_map(|secondary_offset| {
            let new_range_start = secondary_offset + delta_from_offset_to_range_start;
            let new_range_end = new_range_start + primary_string_repl.replace_range.length();
            let range = crate::offset_range::OffsetRange::new(new_range_start, new_range_end);

            let context_range = range.join(crate::offset_range::OffsetRange::empty_at(secondary_offset));
            let context_value = text.get_value_of_offset_range(context_range);
            if context_value != primary_context_value {
                return None;
            }

            let string_repl = StringReplacement::new(range, primary_string_repl.new_text.clone());
            Some(text_transformer.get_text_replacement(&string_repl))
        })
        .collect()
}

/// `isSuggestionInViewport(editor, suggestion, reader)`: `scroll_top` é o `observableCodeEditor(editor).scrollTop`.
pub fn is_suggestion_in_viewport(
    editor: &Rc<CodeEditorWidget>,
    scroll_top: &IObservable<f64>,
    suggestion: &InlineSuggestionItem,
    reader: Option<&dyn IReader>,
) -> bool {
    let target_range = suggestion.target_range();

    // TODO make getVisibleRanges reactive! (comentário do original)
    scroll_top.read_opt(reader);
    let visible_ranges = editor.get_visible_ranges();

    let (Some(first), Some(last)) = (visible_ranges.first(), visible_ranges.last()) else {
        return false;
    };
    let viewport_range = Range::new(first.start_line_number, first.start_column, last.end_line_number, last.end_column);
    viewport_range.contains_range(&target_range)
}

impl InlineCompletionsModel {
    /// O construtor. `is_suppressed` é o `_isSuppressed: () => boolean`.
    pub fn new(
        text_model: Rc<TextModel>,
        selected_suggest_item: IObservable<Option<Rc<SuggestItemInfo>>>,
        text_model_version_id: IObservable<Option<i64>>,
        positions: IObservable<Rc<Vec<Position>>>,
        debounce_value: Rc<dyn IFeatureDebounceInformation>,
        enabled: IObservable<bool>,
        is_suppressed: Rc<dyn Fn() -> bool>,
        editor: Rc<CodeEditorWidget>,
        services: InlineCompletionsModelServices,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let InlineCompletionsModelServices {
            command_service,
            language_configuration_service,
            accessibility_service,
            language_features_service,
            inline_completions_service,
            default_account_service,
            host,
            opened_peek_widgets,
            is_in_snippet_mode,
            completions_enablement_setting,
            source_services,
        } = services;

        let is_active = observable_value::<bool, ()>("isActive", false);
        let only_request_inline_edits_signal = observable_signal::<()>("onlyRequestInlineEditsSignal");
        let force_update_explicitly_signal = observable_signal::<()>("forceUpdateExplicitlySignal");
        let no_delay_signal = observable_signal::<()>("noDelaySignal");
        let fetch_specific_provider_signal = observable_signal::<ProviderChange>("fetchSpecificProviderSignal");
        let dont_refetch_signal = observable_signal::<()>("dontRefetchSignal");
        let selected_inline_completion_id = observable_value::<Option<String>, ()>("selectedInlineCompletionId", None);
        let sku = observable_value::<Option<InlineSuggestSku>, ()>("sku", None);
        let jumped_to_id = observable_value::<Option<String>, ()>("jumpedToId", None);
        let in_accept_flow_value = observable_value::<bool, ()>("inAcceptFlow", false);
        let in_accept_flow: IObservable<bool> = in_accept_flow_value.to_observable();
        let is_accepting_partially = Rc::new(Cell::new(false));
        let last_shown: Rc<RefCell<Option<LastShownInfo>>> = Rc::default();
        let last_accepted: Rc<RefCell<Option<LastAcceptedInfo>>> = Rc::default();

        let primary_position = {
            let positions = Rc::clone(&positions);
            derived(move |reader| positions.read(reader).first().copied().unwrap_or(Position::new(1, 1)))
        };
        let all_positions = {
            let positions = Rc::clone(&positions);
            derived(move |reader| positions.read(reader))
        };

        let source = InlineCompletionsSource::new(
            Rc::clone(&text_model),
            Rc::clone(&text_model_version_id),
            debounce_value,
            Rc::clone(&primary_position),
            completions_enablement_setting,
            source_services,
        );
        base.register(Rc::clone(&source) as DisposableRef);
        let last_trigger_kind = {
            let inline_completions = Rc::clone(&source.inline_completions);
            inline_completions.map(|v, _| v.request.as_ref().map(|r| r.context.trigger_kind))
        };

        // Os observáveis que o `observableCodeEditor(editor)` dava.
        let suggest = option_observable::<InternalSuggestOptions>(&editor, || SUGGEST);
        let suggest_preview_enabled = suggest.map(|v, _| v.preview);
        let suggest_preview_mode = suggest.map(|v, _| suggest_preview_mode_to_ghost(&v.preview_mode));
        let inline_suggest = option_observable::<InternalInlineSuggestOptions>(&editor, || INLINE_SUGGEST);
        let inline_suggest_mode = inline_suggest.map(|v, _| inline_suggest_mode_to_ghost(&v.mode));
        let suppressed_inline_completion_group_ids =
            inline_suggest.map(|v, _| Rc::new(v.experimental.suppress_inline_suggestions.split(',').map(str::to_owned).collect::<HashSet<String>>()));
        let inline_edits_enabled = inline_suggest.map(|v, _| v.edits.enabled);
        let inline_edits_show_collapsed_enabled = inline_suggest.map(|s, _| s.edits.show_collapsed);
        let trigger_command_on_provider_change = inline_suggest.map(|s, _| s.trigger_command_on_provider_change);
        let min_show_delay = inline_suggest.map(|s, _| s.min_show_delay);
        let show_on_suggest_conflict = inline_suggest.map(|s, _| s.experimental.show_on_suggest_conflict);
        let suppress_in_snippet_mode = inline_suggest.map(|s, _| s.suppress_in_snippet_mode);

        let editor_version_id: IObservable<Option<i64>> = {
            let editor = Rc::clone(&editor);
            observable_from_event(editor.on_did_change_model_content(), move |_| editor.get_model().map(|m| m.get_version_id()))
        };
        let scroll_top: IObservable<f64> = {
            let editor = Rc::clone(&editor);
            observable_from_event(editor.on_did_scroll_change(), move |_| editor.get_scroll_top())
        };
        let selections: IObservable<Option<Rc<Vec<Selection>>>> = {
            let editor = Rc::clone(&editor);
            observable_from_event(editor.on_did_change_cursor_selection(), move |_| editor.get_selections().map(Rc::new))
        };
        let cursor_line_number: IObservable<Option<i32>> = {
            let editor = Rc::clone(&editor);
            observable_from_event(editor.on_did_change_cursor_position(), move |_| editor.get_position().map(|p| p.line_number))
        };
        let in_composition = observable_value::<bool, ()>("inComposition", false);
        {
            let (start_target, end_target) = (Rc::clone(&in_composition), Rc::clone(&in_composition));
            base.register(editor.on_did_composition_start().listen(move |_| start_target.set(true, None)));
            base.register(editor.on_did_composition_end().listen(move |_| end_target.set(false, None)));
        }

        // `defaultAccountService.getDefaultAccount().then(createDisposableCb(...))` e o evento de troca de conta.
        let alive = Rc::new(Cell::new(true));
        {
            let (alive_for_account, sku_for_account) = (Rc::clone(&alive), Rc::clone(&sku));
            let _ = default_account_service.get_default_account().then(move |account| {
                if alive_for_account.get() {
                    sku_for_account.set(sku_from_account(account.as_ref()), None);
                }
            });
            base.register(to_disposable(move || alive.set(false)));
            let sku_for_change = Rc::clone(&sku);
            base.register(
                default_account_service
                    .on_did_change_default_account()
                    .listen(move |account| sku_for_change.set(sku_from_account(account.as_ref()), None)),
            );
        }

        let typing = TypingInterval::new(&text_model);
        base.register(Rc::clone(&typing) as DisposableRef);

        // Determine editor type
        let is_notebook = text_model.uri().scheme().to_string_lossy() == schemas::VSCODE_NOTEBOOK_CELL;
        let is_in_diff_editor = host.is_in_diff_editor(&editor.get_id());
        let editor_type = if is_notebook {
            InlineCompletionEditorType::Notebook
        } else if is_in_diff_editor {
            InlineCompletionEditorType::DiffEditor
        } else {
            InlineCompletionEditorType::TextEditor
        };

        // `_didUndoInlineEdits`
        let did_undo_inline_edits = {
            let version_any = text_model_version_id.to_any_observable();
            let tracker = FnChangeTracker::<Cell<bool>>::new(
                |_| Cell::new(false),
                move |ctx: &dyn IChangeContext, did_undo: &Cell<bool>| {
                    did_undo.set(ctx.did_change(&*version_any) && ctx.change::<ModelContentChangedEvent>().is_some_and(|e| e.is_undoing));
                    true
                },
            );
            let (version, last_accepted) = (Rc::clone(&text_model_version_id), Rc::clone(&last_accepted));
            derived_handle_changes::<bool, (), Cell<bool>>(
                DerivedHandleChangesOptions { debug: DebugNameOptions::named("didUndoInlineEdits"), change_tracker: tracker, equality_comparer: None },
                move |reader, did_undo| {
                    let version_id = version.read(reader);
                    let should_reset = match (version_id, last_accepted.borrow().as_ref()) {
                        (Some(version_id), Some(info)) => {
                            info.text_model_version_id_after == version_id - 1 && info.inline_completion.is_inline_edit() && did_undo.get()
                        }
                        _ => false,
                    };
                    if should_reset {
                        *last_accepted.borrow_mut() = None;
                    }
                    should_reset
                },
            )
        };

        // `_inlineSuggestionItems` e derivados.
        let inline_suggestion_items: IObservable<Option<Rc<SuggestionItems>>> = {
            let (source, primary_position, text_model) = (Rc::clone(&source), Rc::clone(&primary_position), Rc::clone(&text_model));
            derived_opts(DerivedOptions::default(), move |reader| {
                // O estado (`InlineCompletionsState`) nunca é indefinido: o vazio dá listas vazias.
                let c = source.inline_completions.read(reader);
                let cursor_position = primary_position.read(reader);
                let mut inline_edit: Option<InlineEditItem> = None;
                let mut visible_completions: Vec<InlineCompletionItem> = Vec::new();
                for completion in &c.inline_completions {
                    match completion {
                        InlineSuggestionItem::Completion(completion) => {
                            if completion.is_visible(&text_model, cursor_position) {
                                visible_completions.push(completion.clone());
                            }
                        }
                        InlineSuggestionItem::Edit(edit) => inline_edit = Some(edit.clone()),
                    }
                }

                if !visible_completions.is_empty() {
                    // Don't show the inline edit if there is a visible completion
                    inline_edit = None;
                }

                Some(Rc::new(SuggestionItems { inline_completions: visible_completions, inline_edit }))
            })
        };
        let inline_completion_items: IObservable<Rc<Vec<InlineCompletionItem>>> = {
            let items = Rc::clone(&inline_suggestion_items);
            derived_opts(
                DerivedOptions {
                    equals_fn: Some(Rc::new(|a: &Rc<Vec<InlineCompletionItem>>, b: &Rc<Vec<InlineCompletionItem>>| {
                        a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| same_item(x, y))
                    }) as EqualityComparer<Rc<Vec<InlineCompletionItem>>>),
                    ..DerivedOptions::default()
                },
                move |reader| Rc::new(items.read(reader).map(|c| c.inline_completions.clone()).unwrap_or_default()),
            )
        };

        let selected_inline_completion_index: IObservable<usize> = {
            let (id, items) = (Rc::clone(&selected_inline_completion_id), Rc::clone(&inline_completion_items));
            derived(move |reader| {
                let selected_inline_completion_id = id.read(reader);
                let filtered_completions = items.read(reader);
                let idx = filtered_completions.iter().position(|v| Some(v.semantic_id()) == selected_inline_completion_id);
                match idx {
                    Some(idx) => idx,
                    None => {
                        // Reset the selection so that the selection does not jump back when it appears again
                        id.set(None, None);
                        0
                    }
                }
            })
        };
        let selected_inline_completion: IObservable<Option<InlineCompletionItem>> = {
            let (items, index) = (Rc::clone(&inline_completion_items), Rc::clone(&selected_inline_completion_index));
            derived(move |reader| {
                let filtered_completions = items.read(reader);
                let idx = index.read(reader);
                filtered_completions.get(idx).cloned()
            })
        };
        let active_commands: IObservable<Rc<Vec<InlineCompletionCommand>>> = {
            let selected = Rc::clone(&selected_inline_completion);
            derived_opts(
                DerivedOptions {
                    // `arrayEqualsC`: os comandos comparam por identidade, que aqui é o conteúdo do `Debug`.
                    equals_fn: Some(Rc::new(|a: &Rc<Vec<InlineCompletionCommand>>, b: &Rc<Vec<InlineCompletionCommand>>| {
                        a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| format!("{x:?}") == format!("{y:?}"))
                    }) as EqualityComparer<Rc<Vec<InlineCompletionCommand>>>),
                    ..DerivedOptions::default()
                },
                move |reader| {
                    Rc::new(
                        selected
                            .read(reader)
                            .and_then(|item| item.source().inline_suggestions.commands.clone())
                            .unwrap_or_default(),
                    )
                },
            )
        };
        let inline_completions_count: IObservable<Option<i32>> = {
            let (kind, items) = (Rc::clone(&last_trigger_kind), Rc::clone(&inline_completion_items));
            derived(move |reader| {
                if kind.read(reader) == Some(InlineCompletionTriggerKind::Explicit) {
                    Some(items.read(reader).len() as i32)
                } else {
                    None
                }
            })
        };

        let has_visible_peek_widgets = derived({
            let peek = Rc::clone(&opened_peek_widgets);
            move |reader| peek.read(reader) > 0
        });

        let should_show_on_suggest_conflict: IObservable<bool> = {
            let (conflict, selected, suggest_item) =
                (Rc::clone(&show_on_suggest_conflict), Rc::clone(&selected_inline_completion), Rc::clone(&selected_suggest_item));
            derived(move |reader| {
                let show_on_suggest_conflict = conflict.read(reader);
                if !matches!(show_on_suggest_conflict, ShowOnSuggestConflict::Never) {
                    let has_inline_completion = selected.read(reader).is_some();
                    if has_inline_completion {
                        let Some(item) = suggest_item.read(reader) else {
                            return false;
                        };
                        if matches!(show_on_suggest_conflict, ShowOnSuggestConflict::WhenSuggestListIsIncomplete) {
                            return item.list_incomplete;
                        }
                        return true;
                    }
                }
                false
            })
        };

        // `state`
        let state: IObservable<Option<Rc<InlineCompletionsModelState>>> = {
            let text_model = Rc::clone(&text_model);
            let suppress_in_snippet_mode = Rc::clone(&suppress_in_snippet_mode);
            let is_in_snippet_mode = Rc::clone(&is_in_snippet_mode);
            let inline_suggestion_items = Rc::clone(&inline_suggestion_items);
            let has_visible_peek_widgets = Rc::clone(&has_visible_peek_widgets);
            let primary_position = Rc::clone(&primary_position);
            let selected_suggest_item = Rc::clone(&selected_suggest_item);
            let should_show_on_suggest_conflict = Rc::clone(&should_show_on_suggest_conflict);
            let suggest_preview_enabled = Rc::clone(&suggest_preview_enabled);
            let suggest_preview_mode = Rc::clone(&suggest_preview_mode);
            let inline_suggest_mode = Rc::clone(&inline_suggest_mode);
            let positions = Rc::clone(&positions);
            let is_active = Rc::clone(&is_active);
            let selected_inline_completion = Rc::clone(&selected_inline_completion);
            let source = Rc::clone(&source);
            derived_opts(
                DerivedOptions {
                    equals_fn: Some(Rc::new(|a: &Option<Rc<InlineCompletionsModelState>>, b: &Option<Rc<InlineCompletionsModelState>>| {
                        InlineCompletionsModelState::equals(a, b)
                    }) as EqualityComparer<Option<Rc<InlineCompletionsModelState>>>),
                    ..DerivedOptions::default()
                },
                move |reader| {
                    let model = &text_model;

                    if suppress_in_snippet_mode.read(reader) && is_in_snippet_mode.read(reader) {
                        return None;
                    }

                    let item = inline_suggestion_items.read(reader);
                    let inline_edit_result = item.as_ref().and_then(|i| i.inline_edit.clone());
                    if let Some(inline_edit_result) = inline_edit_result {
                        if has_visible_peek_widgets.read(reader) {
                            return None;
                        }
                        let target_range = inline_edit_result.target_range();
                        let cursor_at_inline_edit = primary_position.map(move |cursor_pos, _| {
                            LineRange::from_range_inclusive(&target_range).add_margin(1, 1).contains(cursor_pos.line_number)
                        });
                        let string_edit = match inline_edit_result.action() {
                            Some(InlineSuggestionAction::Edit(edit)) => Some(edit.string_edit),
                            _ => None,
                        };
                        let replacements = string_edit
                            .map(|e| TextEdit::from_string_edit(&e, &TextModelText::new(Rc::clone(model))).replacements)
                            .unwrap_or_default();

                        let command = inline_edit_result.command();
                        let mut next_edit_uri = match &command {
                            Some(c) if (c.id == "vscode.open" || c.id == "_workbench.open") && c.arguments.as_ref().is_some_and(|a| !a.is_empty()) => {
                                c.arguments.as_ref().and_then(|a| a.first()).and_then(uri_from_dynamic)
                            }
                            _ => None,
                        };
                        if !inline_edit_result.base().original_text_ref.targets(model) {
                            next_edit_uri = Some(inline_edit_result.base().original_text_ref.uri());
                        }
                        return Some(Rc::new(InlineCompletionsModelState::InlineEdit(InlineEditModelState {
                            edits: replacements,
                            inline_suggestion: inline_edit_result,
                            cursor_at_inline_edit,
                            next_edit_uri,
                        })));
                    }

                    let suggest_item = selected_suggest_item.read(reader);
                    let show_on_conflict = should_show_on_suggest_conflict.read(reader);
                    match suggest_item {
                        Some(suggest_item) if !show_on_conflict => {
                            let suggest_completion_edit = single_text_remove_common_prefix(&suggest_item.get_single_text_edit(), model, None);
                            let augmentation = compute_augmentation(model, &source, &suggest_completion_edit, Some(reader));

                            let is_suggestion_preview_enabled = suggest_preview_enabled.read(reader);
                            if !is_suggestion_preview_enabled && augmentation.is_none() {
                                return None;
                            }

                            let full_edit = augmentation.as_ref().map_or_else(|| suggest_completion_edit.clone(), |a| a.1.clone());
                            let full_edit_preview_length = augmentation
                                .as_ref()
                                .map_or(0, |a| a.1.text.len() as i32 - suggest_completion_edit.text.len() as i32);

                            let mode = suggest_preview_mode.read(reader);
                            let positions = positions.read(reader);
                            let (edits, ghost_texts) = ghost_texts_for(model, &positions, full_edit.clone(), mode, full_edit_preview_length);
                            let primary_ghost_text = ghost_texts
                                .first()
                                .cloned()
                                .unwrap_or_else(|| GhostTextOrReplacement::GhostText(GhostText::new(full_edit.range.end_line_number, Vec::new())));
                            Some(Rc::new(InlineCompletionsModelState::GhostText(GhostTextModelState {
                                edits,
                                primary_ghost_text,
                                ghost_texts,
                                inline_suggestion: augmentation.map(|a| a.0),
                                suggest_item: Some(suggest_item),
                            })))
                        }
                        _ => {
                            if !is_active.read(reader) {
                                return None;
                            }
                            let inline_suggestion = selected_inline_completion.read(reader)?;

                            let replacement = inline_suggestion.get_single_text_edit();
                            let mode = inline_suggest_mode.read(reader);
                            let positions = positions.read(reader);
                            let (edits, ghost_texts) = ghost_texts_for(model, &positions, replacement, mode, 0);
                            let primary_ghost_text = ghost_texts.first().cloned()?;
                            Some(Rc::new(InlineCompletionsModelState::GhostText(GhostTextModelState {
                                edits,
                                primary_ghost_text,
                                ghost_texts,
                                inline_suggestion: Some(inline_suggestion),
                                suggest_item: None,
                            })))
                        }
                    }
                },
            )
        };

        let status: IObservable<ModelStatus> = {
            let (loading, state) = (Rc::clone(&source.loading), Rc::clone(&state));
            derived(move |reader| {
                if loading.read(reader) {
                    return ModelStatus::Loading;
                }
                match state.read(reader).as_deref() {
                    Some(InlineCompletionsModelState::GhostText(_)) => ModelStatus::GhostText,
                    Some(InlineCompletionsModelState::InlineEdit(_)) => ModelStatus::InlineEdit,
                    None => ModelStatus::NoSuggestion,
                }
            })
        };
        let inline_completion_state: IObservable<Option<Rc<InlineCompletionsModelState>>> = {
            let (state, in_composition) = (Rc::clone(&state), in_composition.to_observable());
            derived(move |reader| {
                let s = state.read(reader)?;
                if !matches!(&*s, InlineCompletionsModelState::GhostText(_)) {
                    return None;
                }
                if in_composition.read(reader) {
                    return None;
                }
                Some(s)
            })
        };
        let inline_edit_state: IObservable<Option<Rc<InlineCompletionsModelState>>> = {
            let state = Rc::clone(&state);
            derived(move |reader| {
                let s = state.read(reader)?;
                matches!(&*s, InlineCompletionsModelState::InlineEdit(_)).then_some(s)
            })
        };
        let inline_edit_available = {
            let s = Rc::clone(&inline_edit_state);
            derived(move |reader| s.read(reader).is_some())
        };
        let warning = {
            let s = Rc::clone(&inline_completion_state);
            derived(move |reader| match s.read(reader).as_deref() {
                Some(InlineCompletionsModelState::GhostText(g)) => g.inline_suggestion.as_ref().and_then(|i| i.warning()).map(Rc::new),
                _ => None,
            })
        };
        let ghost_texts: IObservable<Option<Rc<Vec<GhostTextOrReplacement>>>> = {
            let s = Rc::clone(&inline_completion_state);
            derived_opts(
                DerivedOptions {
                    equals_fn: Some(Rc::new(|a: &Option<Rc<Vec<GhostTextOrReplacement>>>, b: &Option<Rc<Vec<GhostTextOrReplacement>>>| {
                        ghost_texts_or_replacements_equal(a.as_ref().map(|v| v.as_slice()), b.as_ref().map(|v| v.as_slice()))
                    }) as EqualityComparer<Option<Rc<Vec<GhostTextOrReplacement>>>>),
                    ..DerivedOptions::default()
                },
                move |reader| match s.read(reader).as_deref() {
                    Some(InlineCompletionsModelState::GhostText(g)) => Some(Rc::new(g.ghost_texts.clone())),
                    _ => None,
                },
            )
        };
        let primary_ghost_text: IObservable<Option<Rc<GhostTextOrReplacement>>> = {
            let s = Rc::clone(&inline_completion_state);
            derived_opts(
                DerivedOptions {
                    equals_fn: Some(Rc::new(|a: &Option<Rc<GhostTextOrReplacement>>, b: &Option<Rc<GhostTextOrReplacement>>| {
                        ghost_text_or_replacement_equals(a.as_deref(), b.as_deref())
                    }) as EqualityComparer<Option<Rc<GhostTextOrReplacement>>>),
                    ..DerivedOptions::default()
                },
                move |reader| match s.read(reader).as_deref() {
                    Some(InlineCompletionsModelState::GhostText(g)) => Some(Rc::new(g.primary_ghost_text.clone())),
                    _ => None,
                },
            )
        };

        let show_collapsed: IObservable<bool> = {
            let (state, version, enabled_collapsed, jumped, in_accept) = (
                Rc::clone(&state),
                Rc::clone(&text_model_version_id),
                Rc::clone(&inline_edits_show_collapsed_enabled),
                jumped_to_id.to_observable(),
                Rc::clone(&in_accept_flow),
            );
            derived(move |reader| {
                let Some(s) = state.read(reader) else { return false };
                let InlineCompletionsModelState::InlineEdit(s) = &*s else { return false };

                if s.inline_suggestion.base().hint.is_some() || matches!(s.inline_suggestion.action(), Some(InlineSuggestionAction::JumpTo(_))) {
                    return false;
                }

                let is_current_model_version = Some(s.inline_suggestion.updated_edit_model_version()) == version.read(reader);
                (enabled_collapsed.read(reader) || !is_current_model_version)
                    && jumped.read(reader).as_deref() != Some(s.inline_suggestion.semantic_id().as_str())
                    && !in_accept.read(reader)
            })
        };

        let tab_should_indent: IObservable<bool> = {
            let (in_accept, selections, text_model) = (Rc::clone(&in_accept_flow), Rc::clone(&selections), Rc::clone(&text_model));
            derived(move |reader| {
                if in_accept.read(reader) {
                    return false;
                }

                fn is_multi_line(range: &Range) -> bool {
                    range.start_line_number != range.end_line_number
                }

                fn get_non_indentation_range(model: &TextModel, line_number: i32) -> Range {
                    let column_start = model.get_line_indent_column(line_number);
                    let last_non_ws_column = model.get_line_last_non_whitespace_column(line_number);
                    let column_end = last_non_ws_column.max(column_start);
                    Range::new(line_number, column_start, line_number, column_end)
                }

                selections.read(reader).is_some_and(|selections| {
                    selections.iter().any(|s| {
                        if s.is_empty() {
                            text_model.get_line_length(s.start_line_number) == 0
                        } else {
                            is_multi_line(s) || s.contains_range(&get_non_indentation_range(&text_model, s.start_line_number))
                        }
                    })
                })
            })
        };

        let appeared_inside_viewport: IObservable<bool> = {
            let (state, editor, scroll_top) = (Rc::clone(&state), Rc::clone(&editor), Rc::clone(&scroll_top));
            derived(move |reader| {
                let Some(state) = state.read(reader) else { return false };
                match state.inline_suggestion() {
                    Some(suggestion) => is_suggestion_in_viewport(&editor, &scroll_top, &suggestion, Some(reader)),
                    None => false,
                }
            })
        };

        let tab_should_jump_to_inline_edit: IObservable<bool> = {
            let (tab_should_indent, inline_edit_state, show_collapsed, in_accept, appeared) = (
                Rc::clone(&tab_should_indent),
                Rc::clone(&inline_edit_state),
                Rc::clone(&show_collapsed),
                Rc::clone(&in_accept_flow),
                Rc::clone(&appeared_inside_viewport),
            );
            derived(move |reader| {
                if tab_should_indent.read(reader) {
                    return false;
                }

                let Some(s) = inline_edit_state.read(reader) else { return false };
                let InlineCompletionsModelState::InlineEdit(s) = &*s else { return false };

                if matches!(s.inline_suggestion.action(), Some(InlineSuggestionAction::JumpTo(_))) {
                    return true;
                }

                if show_collapsed.read(reader) {
                    return true;
                }

                if in_accept.read(reader) && appeared.read(reader) {
                    return false;
                }

                !s.cursor_at_inline_edit.read(reader)
            })
        };

        let tab_should_accept_inline_edit: IObservable<bool> = {
            let (tab_should_indent, inline_edit_state, show_collapsed, in_accept, appeared, cursor_line, jumped) = (
                Rc::clone(&tab_should_indent),
                Rc::clone(&inline_edit_state),
                Rc::clone(&show_collapsed),
                Rc::clone(&in_accept_flow),
                Rc::clone(&appeared_inside_viewport),
                Rc::clone(&cursor_line_number),
                jumped_to_id.to_observable(),
            );
            derived(move |reader| {
                let Some(s) = inline_edit_state.read(reader) else { return false };
                let InlineCompletionsModelState::InlineEdit(s) = &*s else { return false };
                if matches!(s.inline_suggestion.action(), Some(InlineSuggestionAction::JumpTo(_))) {
                    return false;
                }
                if show_collapsed.read(reader) {
                    return false;
                }
                if tab_should_indent.read(reader) {
                    return false;
                }
                if in_accept.read(reader) && appeared.read(reader) {
                    return true;
                }
                if Some(s.inline_suggestion.target_range().start_line_number) == cursor_line.read(reader) {
                    return true;
                }
                if jumped.read(reader).as_deref() == Some(s.inline_suggestion.semantic_id().as_str()) {
                    return true;
                }

                s.cursor_at_inline_edit.read(reader)
            })
        };

        // `_fetchInlineCompletionsPromise`
        let fetch_inline_completions_promise: IObservable<FetchResult> = {
            let version_any = text_model_version_id.to_any_observable();
            let force_any = force_update_explicitly_signal.to_any_observable();
            let dont_refetch_any = dont_refetch_signal.to_any_observable();
            let only_edits_any = only_request_inline_edits_signal.to_any_observable();
            let specific_provider_any = fetch_specific_provider_signal.to_any_observable();
            let is_accepting_partially_for_reason = Rc::clone(&is_accepting_partially);
            let tracker = FnChangeTracker::<FetchChangeSummary>::new(
                |_| FetchChangeSummary::new(),
                move |ctx: &dyn IChangeContext, summary: &FetchChangeSummary| {
                    /* @description fetch inline completions */
                    if ctx.did_change(&*version_any) {
                        let change = ctx.change::<ModelContentChangedEvent>();
                        let reason = match change {
                            Some(e) if e.is_undoing => VersionIdChangeReason::Undo,
                            Some(e) if e.is_redoing => VersionIdChangeReason::Redo,
                            _ if is_accepting_partially_for_reason.get() => VersionIdChangeReason::AcceptWord,
                            _ => VersionIdChangeReason::Other,
                        };
                        if reason.preserves_current_completion() {
                            summary.preserve_current_completion.set(true);
                        }
                        let detailed_reasons = change.map(|e| e.detailed_reasons.as_slice()).unwrap_or_default();
                        *summary.change_reason.borrow_mut() = detailed_reasons.first().map(|r| r.get_type()).unwrap_or_default();
                        summary.text_change.set(true);
                    } else if ctx.did_change(&*force_any) {
                        summary.preserve_current_completion.set(true);
                        summary.inline_completion_trigger_kind.set(InlineCompletionTriggerKind::Explicit);
                    } else if ctx.did_change(&*dont_refetch_any) {
                        summary.dont_refetch.set(true);
                    } else if ctx.did_change(&*only_edits_any) {
                        summary.only_request_inline_edits.set(true);
                    } else if ctx.did_change(&*specific_provider_any) {
                        let change = ctx.change::<ProviderChange>();
                        *summary.provider.borrow_mut() = change.map(|c| Rc::clone(&c.provider));
                        *summary.change_hint.borrow_mut() = change.and_then(|c| c.change_hint.clone());
                    }
                    true
                },
            );

            let fetch = FetchDependencies {
                source: Rc::clone(&source),
                text_model: Rc::clone(&text_model),
                text_model_version_id: Rc::clone(&text_model_version_id),
                enabled: Rc::clone(&enabled),
                is_suppressed: Rc::clone(&is_suppressed),
                selected_suggest_item: Rc::clone(&selected_suggest_item),
                is_active: Rc::clone(&is_active),
                inline_completions_service: Rc::clone(&inline_completions_service),
                should_show_on_suggest_conflict: Rc::clone(&should_show_on_suggest_conflict),
                did_undo_inline_edits: Rc::clone(&did_undo_inline_edits),
                typing: Rc::clone(&typing),
                sku: Rc::clone(&sku),
                inline_edits_enabled: Rc::clone(&inline_edits_enabled),
                in_accept_flow: Rc::clone(&in_accept_flow),
                min_show_delay: Rc::clone(&min_show_delay),
                last_shown: Rc::clone(&last_shown),
                selected_inline_completion: Rc::clone(&selected_inline_completion),
                inline_suggestion_items: Rc::clone(&inline_suggestion_items),
                jumped_to_id: jumped_to_id.to_observable(),
                language_features_service: Rc::clone(&language_features_service),
                suppressed_group_ids: Rc::clone(&suppressed_inline_completion_group_ids),
                editor_type,
                no_delay_signal: Rc::clone(&no_delay_signal),
                dont_refetch_signal: Rc::clone(&dont_refetch_signal),
                only_request_inline_edits_signal: Rc::clone(&only_request_inline_edits_signal),
                force_update_explicitly_signal: Rc::clone(&force_update_explicitly_signal),
                fetch_specific_provider_signal: Rc::clone(&fetch_specific_provider_signal),
            };
            derived_handle_changes::<FetchResult, (), FetchChangeSummary>(
                DerivedHandleChangesOptions { debug: DebugNameOptions::named("fetchInlineCompletionsPromise"), change_tracker: tracker, equality_comparer: None },
                move |reader, change_summary| FetchResult(fetch.run(reader, change_summary)),
            )
        };

        let model = Rc::new_cyclic(|me| InlineCompletionsModel {
            base,
            me: me.clone(),
            text_model,
            editor,
            command_service,
            language_configuration_service,
            accessibility_service,
            language_features_service,
            inline_completions_service,
            host,
            fadeouts: DisposableStore::new(),
            selected_suggest_item,
            text_model_version_id,
            positions,
            enabled,
            source,
            is_active,
            only_request_inline_edits_signal,
            force_update_explicitly_signal,
            no_delay_signal,
            fetch_specific_provider_signal,
            dont_refetch_signal,
            selected_inline_completion_id,
            jumped_to_id,
            in_accept_flow_value,
            is_accepting_partially,
            on_did_accept: Emitter::new(),
            last_shown_inline_completion_info: last_shown,
            last_accepted_inline_completion_info: last_accepted,
            trigger_command_on_provider_change,
            did_undo_inline_edits,
            fetch_inline_completions_promise,
            inline_completion_items,
            primary_position,
            all_positions,
            selected_inline_completion_index,
            selected_inline_completion,
            active_commands,
            last_trigger_kind,
            inline_completions_count,
            state,
            status,
            inline_completion_state,
            inline_edit_state,
            inline_edit_available,
            warning,
            ghost_texts,
            primary_ghost_text,
            show_collapsed,
            tab_should_jump_to_inline_edit,
            tab_should_accept_inline_edit,
            in_accept_flow,
            is_in_diff_editor,
            editor_type,
        });
        model.base.register(Rc::clone(&model.fadeouts) as DisposableRef);
        model.base.register(Rc::clone(&model.on_did_accept) as DisposableRef);
        model.wire(editor_version_id);
        model
    }

    /// O resto do construtor, que precisa do modelo já criado: os `autorun`, o `recomputeInitiallyAndOnChange` e a
    /// escuta dos provedores.
    fn wire(self: &Rc<Self>, editor_version_id: IObservable<Option<i64>>) {
        let base = &self.base;

        base.register(self.inline_completions_service.on_did_change_is_snoozing().listen({
            let weak = self.me.clone();
            move |is_snoozing| {
                if *is_snoozing && let Some(model) = weak.upgrade() {
                    model.stop(StopReason::Automatic, None);
                }
            }
        }));

        {
            let service = Rc::clone(&self.inline_completions_service);
            base.register(recompute_initially_and_on_change(
                &self.state,
                Some(Rc::new(move |s: &Option<Rc<InlineCompletionsModelState>>| {
                    if let Some(suggestion) = s.as_ref().and_then(|s| s.inline_suggestion()) {
                        service.report_new_completion(&suggestion.request_uuid());
                    }
                })),
            ));
        }

        base.register(recompute_initially_and_on_change(&self.fetch_inline_completions_promise, None));

        {
            let (in_accept_flow, editor_version_id) = (Rc::clone(&self.in_accept_flow_value), editor_version_id);
            base.register(autorun(move |reader| {
                editor_version_id.read(reader);
                in_accept_flow.set(false, None);
            }));
        }

        {
            let (state, jumped) = (Rc::clone(&self.state), Rc::clone(&self.jumped_to_id));
            base.register(autorun(move |reader| {
                let jump_to_reset = match state.read(reader).as_deref() {
                    None => true,
                    Some(InlineCompletionsModelState::InlineEdit(s)) => !s.cursor_at_inline_edit.read(reader),
                    Some(InlineCompletionsModelState::GhostText(_)) => false,
                };
                if jump_to_reset {
                    jumped.set(None, None);
                }
            }));
        }

        {
            let state = Rc::clone(&self.state);
            base.register(autorun(move |reader| {
                if let Some(inline_suggestion) = state.read(reader).and_then(|s| s.inline_suggestion()) {
                    inline_suggestion.add_performance_marker("activeSuggestion");
                }
            }));
        }

        {
            let inline_edit_semantic_id: IObservable<Option<String>> = {
                let inline_edit_state = Rc::clone(&self.inline_edit_state);
                derived(move |reader| match inline_edit_state.read(reader).as_deref() {
                    Some(InlineCompletionsModelState::InlineEdit(s)) => Some(s.inline_suggestion.semantic_id()),
                    _ => None,
                })
            };
            let weak = self.me.clone();
            base.register(autorun(move |reader| {
                let id = inline_edit_semantic_id.read(reader);
                if id.is_some()
                    && let Some(model) = weak.upgrade()
                {
                    model.editor.push_undo_stop();
                    if let Some(inline_suggestion) = model.state.get().and_then(|s| s.inline_suggestion()) {
                        *model.last_shown_inline_completion_info.borrow_mut() = Some(LastShownInfo {
                            alternate_text_model_version_id: model.text_model.get_alternative_version_id(),
                            inline_completion: inline_suggestion,
                        });
                    }
                }
            }));
        }

        // TODO: should use getAvailableProviders and update on _suppressedInlineCompletionGroupIds change (comentário do original)
        {
            let language_features_service = Rc::clone(&self.language_features_service);
            let text_model = Rc::clone(&self.text_model);
            let inline_completion_providers: IObservable<Rc<Vec<Rc<dyn InlineCompletionsProvider>>>> = observable_from_event(
                self.language_features_service.inline_completions_provider.on_did_change(),
                move |_| Rc::new(language_features_service.inline_completions_provider.all(&text_model)),
            );
            let weak = self.me.clone();
            let mapped = map_observable_array_cached_by(
                None,
                inline_completion_providers,
                move |provider: &Rc<dyn InlineCompletionsProvider>, store: &Rc<DisposableStore>| {
                    let Some(event) = provider.on_did_change_inline_completions() else {
                        return;
                    };
                    let (weak, provider) = (weak.clone(), Rc::clone(provider));
                    store.add(event.listen(move |change_hint| {
                        if let Some(model) = weak.upgrade() {
                            model.handle_provider_change(&provider, change_hint.clone());
                        }
                    }));
                },
                |provider| Rc::as_ptr(provider).cast::<()>() as usize,
            );
            base.register(recompute_initially_and_on_change(&mapped, None));
        }

        base.register(recompute_initially_and_on_change(&self.did_undo_inline_edits, None));
    }

    /// O callback de `provider.onDidChangeInlineCompletions`.
    fn handle_provider_change(self: &Rc<Self>, provider: &Rc<dyn InlineCompletionsProvider>, change_hint: Option<IInlineCompletionChangeHint>) {
        if !self.enabled.get() {
            return;
        }

        // Only update the active editor
        if !self.host.is_focused_or_active_editor(&self.editor) {
            return;
        }

        if self.trigger_command_on_provider_change.get() {
            // TODO@hediet remove this and always do the else branch. (comentário do original)
            let _ = self.trigger(None, TriggerOptions { only_fetch_inline_edits: true, ..TriggerOptions::default() });
            return;
        }

        // If there is an active suggestion from a different provider, we ignore the update
        // (`activeState.edits` é uma lista, sempre verdadeira; sem sugestão o provedor comparado é indefinido.)
        if let Some(active_state) = self.state.get() {
            let same_provider = active_state
                .inline_suggestion()
                .is_some_and(|s| std::ptr::eq(Rc::as_ptr(&s.source().provider).cast::<()>(), Rc::as_ptr(provider).cast::<()>()));
            if !same_provider {
                return;
            }
        }

        transaction(|tx: &dyn ITransaction| {
            self.fetch_specific_provider_signal.trigger(Some(tx), ProviderChange { provider: Rc::clone(provider), change_hint });
            let _ = self.trigger(Some(tx), TriggerOptions::default());
        });
    }

    /// `editor`.
    pub fn editor(&self) -> &Rc<CodeEditorWidget> {
        &self.editor
    }

    /// `isAcceptingPartially`.
    pub fn is_accepting_partially(&self) -> bool {
        self.is_accepting_partially.get()
    }

    /// `onDidAccept`.
    pub fn on_did_accept(&self) -> Event<()> {
        self.on_did_accept.event()
    }

    /// `debugGetSelectedSuggestItem()`.
    pub fn debug_get_selected_suggest_item(&self) -> IObservable<Option<Rc<SuggestItemInfo>>> {
        Rc::clone(&self.selected_suggest_item)
    }

    /// `getIndentationInfo(reader)`: `(startsWithIndentation, startsWithIndentationLessThanTabSize)`.
    pub fn get_indentation_info(&self, reader: &dyn IReader) -> (bool, bool) {
        let mut starts_with_indentation = false;
        let mut starts_with_indentation_less_than_tab_size = true;
        let ghost_text = self.primary_ghost_text.read(reader);
        if let Some(ghost_text) = ghost_text.as_deref()
            && let Some(first_part) = ghost_parts(ghost_text).first()
        {
            let column = first_part.column;
            let first_line = first_part.lines[0].line.clone();

            let indentation_end_column = self.text_model.get_line_indent_column(ghost_line_number(ghost_text));
            let in_indentation = column <= indentation_end_column;

            if in_indentation {
                let first_non_ws_idx = je_base::strings::first_non_whitespace_index(first_line.units())
                    .map_or(first_line.len() as i32 - 1, |i| i as i32);
                starts_with_indentation = first_non_ws_idx > 0;

                let tab_size = self.text_model.get_options().tab_size;
                let visible_column_indentation = visible_column_from_column(first_line.units(), first_non_ws_idx + 1, tab_size as i32);
                starts_with_indentation_less_than_tab_size = visible_column_indentation < tab_size as i32;
            }
        }
        (starts_with_indentation, starts_with_indentation_less_than_tab_size)
    }

    /// `trigger(tx, options)`: dispara a busca e espera por ela.
    pub fn trigger(self: &Rc<Self>, tx: Option<&dyn ITransaction>, options: TriggerOptions) -> Promise<()> {
        subtransaction(tx, |tx| {
            if options.only_fetch_inline_edits {
                self.only_request_inline_edits_signal.trigger(Some(tx), ());
            }
            if options.no_delay {
                self.no_delay_signal.trigger(Some(tx), ());
            }
            self.is_active.set(true, Some(tx));

            if options.explicit {
                self.in_accept_flow_value.set(true, Some(tx));
                self.force_update_explicitly_signal.trigger(Some(tx), ());
            }
            if let Some(provider) = &options.provider {
                self.fetch_specific_provider_signal
                    .trigger(Some(tx), ProviderChange { provider: Rc::clone(provider), change_hint: options.change_hint.clone() });
            }
        });
        let pending = self.fetch_inline_completions_promise.get().0;
        async_fn(async move {
            if let Some(pending) = pending {
                pending.await?;
            }
            Ok(())
        })
    }

    /// `triggerExplicitly(tx, onlyFetchInlineEdits = false)`.
    pub fn trigger_explicitly(self: &Rc<Self>, tx: Option<&dyn ITransaction>, only_fetch_inline_edits: bool) -> Promise<()> {
        self.trigger(tx, TriggerOptions { only_fetch_inline_edits, explicit: true, ..TriggerOptions::default() })
    }

    /// `stop(stopReason = 'automatic', tx)`.
    pub fn stop(&self, stop_reason: StopReason, tx: Option<&dyn ITransaction>) {
        subtransaction(tx, |tx| {
            if stop_reason == StopReason::ExplicitCancel
                && let Some(inline_completion) = self.state.get().and_then(|s| s.inline_suggestion())
            {
                inline_completion.report_end_of_life(InlineCompletionEndOfLifeReason::Rejected);
            }

            self.is_active.set(false, Some(tx));
            self.source.clear(tx);
        });
    }

    /// `extractReproSample()`.
    pub fn extract_repro_sample(&self) -> Repro {
        let value = self.text_model.get_value();
        let item = self.state.get().and_then(|s| s.inline_suggestion());
        Repro { document_value: value, inline_completion: item.map(|i| i.get_source_completion()) }
    }

    async fn delta_selected_inline_completion_index(self: Rc<Self>, delta: i32) -> Result<(), Error> {
        self.trigger_explicitly(None, false).await?;

        let completions = self.inline_completion_items.get();
        if !completions.is_empty() {
            let len = completions.len() as i32;
            let new_idx = (self.selected_inline_completion_index.get() as i32 + delta + len) % len;
            self.selected_inline_completion_id.set(Some(completions[new_idx as usize].semantic_id()), None);
        } else {
            self.selected_inline_completion_id.set(None, None);
        }
        Ok(())
    }

    /// `next()`.
    pub fn next(self: &Rc<Self>) -> Promise<()> {
        let this = Rc::clone(self);
        async_fn(async move { this.delta_selected_inline_completion_index(1).await })
    }

    /// `previous()`.
    pub fn previous(self: &Rc<Self>) -> Promise<()> {
        let this = Rc::clone(self);
        async_fn(async move { this.delta_selected_inline_completion_index(-1).await })
    }

    /// `_getMetadata(completion, languageId, type?)`.
    fn get_metadata(completion: &InlineSuggestionItem, language_id: &str, kind: Option<&str>) -> TextModelEditSource {
        let source = completion.source();
        let provider_id = source.provider.provider_id().cloned();
        let correlation_id = completion.get_source_completion().correlation_id.clone();
        match kind {
            Some(kind) => TextModelEditSource::inline_completion_partial_accept(&InlineCompletionPartialAcceptData {
                nes: completion.is_inline_edit(),
                request_uuid: completion.request_uuid(),
                provider_id,
                language_id: language_id.to_owned(),
                kind: kind.to_owned(),
                correlation_id,
            }),
            None => TextModelEditSource::inline_completion_accept(&InlineCompletionAcceptData {
                nes: completion.is_inline_edit(),
                request_uuid: completion.request_uuid(),
                correlation_id,
                provider_id,
                language_id: language_id.to_owned(),
            }),
        }
    }

    /// `accept(editor = this._editor, alternativeAction = false)`.
    pub fn accept(self: &Rc<Self>, editor: &Rc<CodeEditorWidget>, alternative_action: bool) -> Promise<()> {
        let (this, editor) = (Rc::clone(self), Rc::clone(editor));
        async_fn(async move {
            if !editor.get_model().is_some_and(|m| Rc::ptr_eq(&m, &this.text_model)) {
                return Err(Error::bug_indicating(None));
            }

            let completion: InlineSuggestionItem;
            let mut is_next_edit_uri = false;
            let state = this.state.get();
            let edits: Vec<TextReplacement>;
            let state_is_inline_edit: bool;
            match state.as_deref() {
                Some(InlineCompletionsModelState::GhostText(s)) => {
                    let Some(inline_suggestion) = &s.inline_suggestion else {
                        return Ok(());
                    };
                    if ghost_is_empty(&s.primary_ghost_text) {
                        return Ok(());
                    }
                    completion = InlineSuggestionItem::Completion(inline_suggestion.clone());
                    edits = s.edits.clone();
                    state_is_inline_edit = false;
                }
                Some(InlineCompletionsModelState::InlineEdit(s)) => {
                    completion = InlineSuggestionItem::Edit(s.inline_suggestion.clone());
                    is_next_edit_uri = s.next_edit_uri.is_some();
                    edits = s.edits.clone();
                    state_is_inline_edit = true;
                }
                None => return Ok(()),
            }

            // Make sure the completion list will not be disposed before the text change is sent.
            completion.add_ref();

            let result: Result<(), Error> = async {
                let mut follow_up_trigger = false;
                editor.push_undo_stop();

                if !completion.base().original_text_ref.targets(&this.text_model) {
                    // The edit targets a different document, open it and transplant the completion
                    let target_editor = this.host.open_code_editor(completion.base().original_text_ref.uri(), &this.editor).await?;
                    if let Some(target_editor) = target_editor {
                        let model = this.host.inline_completions_model_of(&target_editor);
                        target_editor.focus();
                        if let Some(model) = model {
                            model.transplant_completion(completion.clone());
                        }
                        target_editor.reveal_line_in_center(completion.target_range().start_line_number, None);
                    }
                } else if is_next_edit_uri {
                    // Do nothing
                } else if let Some(InlineSuggestionAction::Edit(action)) = completion.action() {
                    let language_id = this.text_model.get_language_id();
                    let additional_edits = additional_text_edits(&completion);
                    if alternative_action && let Some(alternative) = &action.alternative_action {
                        follow_up_trigger = true;
                        let alt_command = &alternative.command;
                        let outcome = this.command_service.execute_command(&alt_command.id, alt_command.arguments.clone().unwrap_or_default()).await;
                        if let Err(error) = outcome {
                            on_unexpected_external_error(&error);
                        }
                    } else if let Some(snippet_info) = &action.snippet_info {
                        let main_edit = TextReplacement::delete(action.text_replacement.range);
                        let all: Vec<TextReplacement> = std::iter::once(main_edit).chain(additional_edits).collect();
                        let edit = TextEdit::from_parallel_replacements_unsorted(&all);
                        editor.edit(&edit, Self::get_metadata(&completion, &language_id, None));

                        editor.set_position(snippet_info.range.get_start_position(), Some("inlineCompletionAccept"));
                        if let Some(controller) = SnippetController2::get(&editor) {
                            controller.insert(
                                snippet_info.snippet.clone(),
                                Some(PartialSnippetInsertOptions { undo_stop_before: Some(false), ..PartialSnippetInsertOptions::default() }),
                            );
                        }
                    } else {
                        // The cursor should move to the end of the edit, not the end of the range provided by the extension
                        // Inline Edit diffs (human readable) the suggestion from the extension so it already removes common suffix/prefix
                        // Inline Completions does diff the suggestion so it may contain common suffix
                        let minimal_edits = if state_is_inline_edit {
                            edits.clone()
                        } else {
                            remove_text_replacement_common_suffix_prefix(&edits, &this.text_model)
                        };
                        let selections: Vec<Selection> =
                            get_end_positions_after_applying(&minimal_edits).into_iter().map(|p| Selection::from_positions(p, p)).collect();

                        let all: Vec<TextReplacement> = edits.iter().cloned().chain(additional_edits).collect();
                        let edit = TextEdit::from_parallel_replacements_unsorted(&all);

                        editor.edit(&edit, Self::get_metadata(&completion, &language_id, None));

                        if completion.base().hint.is_none() {
                            // do not move the cursor when the completion is displayed in a different location
                            let selections_to_set: &[Selection] =
                                if state_is_inline_edit { &selections[selections.len().saturating_sub(1)..] } else { &selections };
                            editor.set_selections(selections_to_set, Some("inlineCompletionAccept"), None);
                        }

                        if state_is_inline_edit && !this.accessibility_service.is_motion_reduced() {
                            this.start_fadeout(edit.get_new_ranges());
                        }
                    }
                }

                this.on_did_accept.fire(());

                // Reset before invoking the command, as the command might cause a follow up trigger (which we don't want to reset).
                this.stop(StopReason::Automatic, None);

                if let Some(command) = completion.command() {
                    let outcome = this.command_service.execute_command(&command.id, command.arguments.clone().unwrap_or_default()).await;
                    if let Err(error) = outcome {
                        on_unexpected_external_error(&error);
                    }
                }

                // TODO: how can we make alternative actions to retrigger? (comentário do original)
                if follow_up_trigger {
                    let _ = this.trigger(None, TriggerOptions::default());
                }

                completion.report_end_of_life(InlineCompletionEndOfLifeReason::Accepted { alternative_action });
                Ok(())
            }
            .await;

            completion.remove_ref();
            this.in_accept_flow_value.set(true, None);
            *this.last_accepted_inline_completion_info.borrow_mut() =
                Some(LastAcceptedInfo { text_model_version_id_after: this.text_model.get_version_id(), inline_completion: completion });
            result
        })
    }

    /// O `FadeoutDecoration`: o descartável sai do `fadeouts` quando o hospedeiro avisa que a animação acabou.
    fn start_fadeout(&self, ranges: Vec<Range>) {
        let slot: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let (slot_for_finish, store) = (Rc::clone(&slot), Rc::clone(&self.fadeouts));
        let decoration = self.host.start_edits_fadeout(
            &self.editor,
            ranges,
            Rc::new(move || {
                if let Some(decoration) = slot_for_finish.borrow_mut().take() {
                    store.delete(&decoration);
                }
            }),
        );
        self.fadeouts.add(Rc::clone(&decoration));
        *slot.borrow_mut() = Some(decoration);
    }

    /// `acceptNextWord()`.
    pub fn accept_next_word(self: &Rc<Self>) -> Promise<()> {
        let this = Rc::clone(self);
        self.accept_next(
            "word",
            Box::new(move |pos: Position, text: &[u16]| {
                let lang_id = this.text_model.get_language_id_at_position(pos.line_number, pos.column);
                let config = this.language_configuration_service.get_language_configuration(&lang_id);
                let word_reg_exp = &config.word_definition;
                word_reg_exp.set_last_index(0);

                let m1 = word_reg_exp.exec(text);
                let mut accept_until_index_exclusive = match &m1 {
                    Some(m1) if m1.index() == 0 => m1.whole().len(),
                    Some(m1) => m1.index(),
                    None => text.len(),
                };
                word_reg_exp.set_last_index(0);

                // `/\s+/g.exec(text)`: a primeira sequência de espaços.
                if let Some(start) = text.iter().position(|u| is_js_whitespace(*u)) {
                    let length = text[start..].iter().take_while(|u| is_js_whitespace(**u)).count();
                    if start + length < accept_until_index_exclusive {
                        accept_until_index_exclusive = start + length;
                    }
                }
                accept_until_index_exclusive
            }),
            PartialAcceptTriggerKind::Word,
        )
    }

    /// `acceptNextLine()`.
    pub fn accept_next_line(self: &Rc<Self>) -> Promise<()> {
        self.accept_next(
            "line",
            Box::new(|_pos: Position, text: &[u16]| text.iter().position(|u| *u == u16::from(b'\n')).map_or(text.len(), |i| i + 1)),
            PartialAcceptTriggerKind::Line,
        )
    }

    /// `_acceptNext(editor, type, getAcceptUntilIndex, kind)`.
    fn accept_next(
        self: &Rc<Self>,
        kind_name: &'static str,
        get_accept_until_index: Box<dyn Fn(Position, &[u16]) -> usize>,
        kind: PartialAcceptTriggerKind,
    ) -> Promise<()> {
        let this = Rc::clone(self);
        let editor = Rc::clone(&self.editor);
        async_fn(async move {
            if !editor.get_model().is_some_and(|m| Rc::ptr_eq(&m, &this.text_model)) {
                return Err(Error::bug_indicating(None));
            }

            let state = this.inline_completion_state.get();
            let Some(InlineCompletionsModelState::GhostText(state)) = state.as_deref() else {
                return Ok(());
            };
            let Some(completion) = state.inline_suggestion.clone() else {
                return Ok(());
            };
            if ghost_is_empty(&state.primary_ghost_text) {
                return Ok(());
            }
            let ghost_text = &state.primary_ghost_text;

            if completion.snippet_info.is_some() {
                // not in WYSIWYG mode, partial commit might change completion, thus it is not supported
                return this.accept(&editor, false).await;
            }

            let parts = ghost_parts(ghost_text);
            let first_part = &parts[0];
            let ghost_text_pos = Position::new(ghost_line_number(ghost_text), first_part.column);
            let ghost_text_val = first_part.text.clone();
            let accept_until_index_exclusive = get_accept_until_index(ghost_text_pos, ghost_text_val.units());
            if accept_until_index_exclusive == ghost_text_val.len() && parts.len() == 1 {
                let _ = this.accept(&editor, false);
                return Ok(());
            }
            let partial_ghost_text_val = JsString::from(&ghost_text_val.units()[..accept_until_index_exclusive]);

            let positions = this.positions.get();
            let cursor_position = positions[0];
            let completion_item = InlineSuggestionItem::Completion(completion.clone());

            // Executing the edit might free the completion, so we have to hold a reference on it.
            completion_item.add_ref();
            let language_id = this.text_model.get_language_id();
            this.is_accepting_partially.set(true);
            {
                editor.push_undo_stop();
                let replace_range = Range::from_positions(cursor_position, ghost_text_pos);
                let mut new_text = editor.get_model().expect("modelo conferido acima").get_value_in_range(&replace_range);
                new_text.push_units(partial_ghost_text_val.units());
                let primary_edit = TextReplacement::new(replace_range, new_text);
                let mut edits = get_secondary_edits(&this.text_model, &positions, &primary_edit);
                edits.insert(0, primary_edit);
                let selections: Vec<Selection> =
                    get_end_positions_after_applying(&edits).into_iter().map(|p| Selection::from_positions(p, p)).collect();

                editor.edit(&TextEdit::from_parallel_replacements_unsorted(&edits), Self::get_metadata(&completion_item, &language_id, Some(kind_name)));
                editor.set_selections(&selections, Some("inlineCompletionPartialAccept"), None);
                if let Some(position) = editor.get_position() {
                    editor.reveal_position_in_center_if_outside_viewport(position, Some(ScrollType::Smooth));
                }
            }
            this.is_accepting_partially.set(false);

            let accepted_range = Range::from_positions(
                completion.edit_range().get_start_position(),
                TextLength::of_text_units(partial_ghost_text_val.units()).add_to_position(ghost_text_pos),
            );
            // This assumes that the inline completion and the model use the same EOL style.
            let text = editor.get_model().expect("modelo conferido acima").get_value_in_range_with(&accepted_range, EndOfLinePreference::Lf);
            let accepted_length = text.len() as i32;
            completion_item.report_partial_accept(
                accepted_length,
                PartialAcceptInfo { kind, accepted_length },
                PartialAcceptance {
                    characters: accept_until_index_exclusive as i32,
                    ratio: accept_until_index_exclusive as f64 / ghost_text_val.len() as f64,
                    count: 1,
                },
            );
            completion_item.remove_ref();
            Ok(())
        })
    }

    /// `handleSuggestAccepted(item)`.
    pub fn handle_suggest_accepted(&self, item: &SuggestItemInfo) {
        let item_edit = single_text_remove_common_prefix(&item.get_single_text_edit(), &self.text_model, None);
        let Some((completion, _)) = compute_augmentation(&self.text_model, &self.source, &item_edit, None) else {
            return;
        };

        // This assumes that the inline completion and the model use the same EOL style.
        let already_accepted_length = self.text_model.get_value_in_range_with(&completion.edit_range(), EndOfLinePreference::Lf).len() as i32;
        let accepted_length = already_accepted_length + item_edit.text.len() as i32;

        InlineSuggestionItem::Completion(completion).report_partial_accept(
            item_edit.text.len() as i32,
            PartialAcceptInfo { kind: PartialAcceptTriggerKind::Suggest, accepted_length },
            PartialAcceptance { characters: item_edit.text.len() as i32, count: 1, ratio: 1.0 },
        );
    }

    /// `jump()`.
    pub fn jump(self: &Rc<Self>) {
        let state = self.inline_edit_state.get();
        let Some(InlineCompletionsModelState::InlineEdit(s)) = state.as_deref() else {
            return;
        };

        let suggestion = InlineSuggestionItem::Edit(s.inline_suggestion.clone());

        if !suggestion.base().original_text_ref.targets(&self.text_model) {
            let _ = self.accept(&self.editor, false);
            return;
        }

        suggestion.add_ref();
        transaction(|tx: &dyn ITransaction| {
            let is_jump_to = matches!(suggestion.action(), Some(InlineSuggestionAction::JumpTo(_)));
            if is_jump_to {
                self.stop(StopReason::Automatic, Some(tx));
                suggestion.report_end_of_life(InlineCompletionEndOfLifeReason::Accepted { alternative_action: false });
            }

            self.jumped_to_id.set(Some(s.inline_suggestion.semantic_id()), Some(tx));
            self.dont_refetch_signal.trigger(Some(tx), ());
            let target_range = s.inline_suggestion.target_range();
            let target_position = target_range.get_start_position();
            self.editor.set_position(target_position, Some("inlineCompletions.jump"));

            // TODO: consider using view information to reveal it (comentário do original)
            let is_single_line_change = target_range.is_single_line()
                && (s.inline_suggestion.base().hint.is_some()
                    || matches!(suggestion.action(), Some(InlineSuggestionAction::Edit(a)) if !a.text_replacement.text.units().contains(&u16::from(b'\n'))));
            if is_single_line_change || is_jump_to {
                self.editor.reveal_position(target_position, Some(ScrollType::Smooth));
            } else {
                let reveal_range = Range::new(target_range.start_line_number - 1, 1, target_range.end_line_number + 1, 1);
                self.editor.reveal_range(reveal_range, Some(ScrollType::Smooth), None, None);
            }

            s.inline_suggestion.base().identity.set_jump_to(Some(tx));

            self.editor.focus();
        });
        suggestion.remove_ref();
    }

    /// `handleInlineSuggestionShown(inlineCompletion, viewKind, viewData, timeWhenShown)`.
    pub fn handle_inline_suggestion_shown(
        &self,
        inline_completion: &InlineSuggestionItem,
        view_kind: InlineCompletionViewKind,
        view_data: InlineCompletionViewData,
        time_when_shown: f64,
    ) -> Promise<()> {
        inline_completion.report_inline_edit_shown(&*self.command_service, view_kind, view_data, &self.text_model, time_when_shown)
    }

    /// Transplants an inline completion from another model to this one.
    /// Used for cross-file inline edits.
    pub fn transplant_completion(&self, item: InlineSuggestionItem) {
        // No explicit addRef needed: `seedWithCompletion` creates a new `InlineCompletionsState`
        // which calls `addRef` on every item it holds and pairs it with `removeRef` in dispose.
        transaction(|tx: &dyn ITransaction| {
            self.source.seed_with_completion(item, tx);
            self.is_active.set(true, Some(tx));
            self.in_accept_flow_value.set(true, Some(tx));
            self.dont_refetch_signal.trigger(Some(tx), ());
        });
    }
}

impl IDisposable for InlineCompletionsModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O `stopReason` de `stop`: `'explicitCancel' | 'automatic'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    ExplicitCancel,
    Automatic,
}

/// `additionalTextEdits.map(e => new TextReplacement(Range.lift(e.range), e.text ?? ''))`.
fn additional_text_edits(completion: &InlineSuggestionItem) -> Vec<TextReplacement> {
    let edits: Vec<SingleEditOperation> = match completion {
        InlineSuggestionItem::Completion(c) => c.additional_text_edits.clone(),
        InlineSuggestionItem::Edit(e) => e.additional_text_edits(),
    };
    edits.into_iter().map(|e| TextReplacement::new(e.range, e.text.unwrap_or_default())).collect()
}

/// `_computeAugmentation(suggestCompletion, reader)`: a sugestão em linha que estende a do suggest widget, com a edição
/// já sem o prefixo comum.
fn compute_augmentation(
    model: &Rc<TextModel>,
    source: &Rc<InlineCompletionsSource>,
    suggest_completion: &TextReplacement,
    reader: Option<&dyn IReader>,
) -> Option<(InlineCompletionItem, TextReplacement)> {
    let suggest_widget_inline_completions = source.suggest_widget_inline_completions.read_opt(reader);
    for candidate in &suggest_widget_inline_completions.inline_completions {
        let InlineSuggestionItem::Completion(completion) = candidate else {
            continue;
        };
        let r = completion.get_single_text_edit();
        let r = single_text_remove_common_prefix(
            &r,
            model,
            Some(&Range::from_positions(r.range.get_start_position(), suggest_completion.range.get_end_position())),
        );
        if single_text_edit_augments(&r, suggest_completion) {
            return Some((completion.clone(), r));
        }
    }
    None
}

/// As edições válidas (a primária e as dos cursores secundários) e o texto fantasma de cada uma.
fn ghost_texts_for(
    model: &Rc<TextModel>,
    positions: &[Position],
    primary: TextReplacement,
    mode: GhostTextMode,
    preview_suffix_length: i32,
) -> (Vec<TextReplacement>, Vec<GhostTextOrReplacement>) {
    let mut all_potential_edits = vec![primary.clone()];
    all_potential_edits.extend(get_secondary_edits(model, positions, &primary));
    let mut edits = Vec::new();
    let mut ghost_texts = Vec::new();
    for (idx, edit) in all_potential_edits.into_iter().enumerate() {
        if let Some(ghost_text) = compute_ghost_text(&edit, model, mode, positions.get(idx).copied(), preview_suffix_length) {
            edits.push(edit);
            ghost_texts.push(GhostTextOrReplacement::GhostText(ghost_text));
        }
    }
    (edits, ghost_texts)
}

/// `getAvailableProviders(providers)`. O `excludesGroupIds` do original é uma versão simplificada, ainda proposta: tira os provedores de grupos suprimidos e os de grupos que outro provedor exclui.
fn available_providers(
    providers: &[Rc<dyn InlineCompletionsProvider>],
    suppressed_provider_group_ids: &HashSet<String>,
) -> Vec<Rc<dyn InlineCompletionsProvider>> {
    let unsuppressed_providers: Vec<&Rc<dyn InlineCompletionsProvider>> = providers
        .iter()
        .filter(|provider| !provider.group_id().is_some_and(|group_id| suppressed_provider_group_ids.contains(group_id)))
        .collect();

    let mut excluded_group_ids: HashSet<String> = HashSet::new();
    for provider in &unsuppressed_providers {
        if let Some(ids) = provider.excludes_group_ids() {
            excluded_group_ids.extend(ids.iter().cloned());
        }
    }

    unsuppressed_providers
        .into_iter()
        .filter(|provider| !provider.group_id().is_some_and(|group_id| excluded_group_ids.contains(group_id)))
        .cloned()
        .collect()
}

/// As dependências do corpo do `_fetchInlineCompletionsPromise`.
struct FetchDependencies {
    source: Rc<InlineCompletionsSource>,
    text_model: Rc<TextModel>,
    text_model_version_id: IObservable<Option<i64>>,
    enabled: IObservable<bool>,
    is_suppressed: Rc<dyn Fn() -> bool>,
    selected_suggest_item: IObservable<Option<Rc<SuggestItemInfo>>>,
    is_active: Rc<ObservableValue<bool, ()>>,
    inline_completions_service: Rc<dyn IInlineCompletionsService>,
    should_show_on_suggest_conflict: IObservable<bool>,
    did_undo_inline_edits: IObservable<bool>,
    typing: Rc<TypingInterval>,
    sku: Rc<ObservableValue<Option<InlineSuggestSku>, ()>>,
    inline_edits_enabled: IObservable<bool>,
    in_accept_flow: IObservable<bool>,
    min_show_delay: IObservable<f64>,
    last_shown: Rc<RefCell<Option<LastShownInfo>>>,
    selected_inline_completion: IObservable<Option<InlineCompletionItem>>,
    inline_suggestion_items: IObservable<Option<Rc<SuggestionItems>>>,
    jumped_to_id: IObservable<Option<String>>,
    language_features_service: Rc<LanguageFeaturesService>,
    suppressed_group_ids: IObservable<Rc<HashSet<String>>>,
    editor_type: InlineCompletionEditorType,
    no_delay_signal: Rc<ObservableSignal<()>>,
    dont_refetch_signal: Rc<ObservableSignal<()>>,
    only_request_inline_edits_signal: Rc<ObservableSignal<()>>,
    force_update_explicitly_signal: Rc<ObservableSignal<()>>,
    fetch_specific_provider_signal: Rc<ObservableSignal<ProviderChange>>,
}

impl FetchDependencies {
    /// O corpo do `derivedHandleChanges` de `_fetchInlineCompletionsPromise`.
    fn run(&self, reader: &dyn IReader, change_summary: &FetchChangeSummary) -> Option<Promise<bool>> {
        self.source.clear_operation_on_text_model_change.read(reader); // Make sure the clear operation runs before the fetch operation
        self.no_delay_signal.read(reader);
        self.dont_refetch_signal.read(reader);
        self.only_request_inline_edits_signal.read(reader);
        self.force_update_explicitly_signal.read(reader);
        self.fetch_specific_provider_signal.read(reader);
        let trigger_kind = change_summary.inline_completion_trigger_kind.get();
        let should_update = !(self.is_suppressed)()
            && ((self.enabled.read(reader) && self.selected_suggest_item.read(reader).is_some()) || self.is_active.read(reader))
            && (!self.inline_completions_service.is_snoozing() || trigger_kind == InlineCompletionTriggerKind::Explicit);
        if !should_update {
            self.source.cancel_update();
            return None;
        }

        self.text_model_version_id.read(reader); // Refetch on text change

        let suggest_item = if self.should_show_on_suggest_conflict.get() { None } else { self.selected_suggest_item.read(reader) };
        // O estado do suggest widget nunca é indefinido (ver as notas do módulo).
        if suggest_item.is_none() {
            self.source.seed_inline_completions_with_suggest_widget();
        }

        if change_summary.dont_refetch.get() {
            return Some(Promise::resolved(true));
        }

        if self.did_undo_inline_edits.read(reader) && trigger_kind != InlineCompletionTriggerKind::Explicit {
            transaction(|tx: &dyn ITransaction| {
                self.source.clear(tx);
            });
            return None;
        }

        let mut reason = String::new();
        if change_summary.provider.borrow().is_some() {
            reason.push_str("providerOnDidChange");
        } else if trigger_kind == InlineCompletionTriggerKind::Explicit {
            reason.push_str("explicit");
        }
        let change_reason = change_summary.change_reason.borrow().clone();
        if !change_reason.is_empty() {
            if reason.is_empty() {
                reason.push_str(&change_reason);
            } else {
                reason.push(':');
                reason.push_str(&change_reason);
            }
        }

        let typing_interval = self.typing.get_typing_interval();
        let start_time = date_now() as f64;
        let mut request_info = InlineSuggestRequestInfo {
            editor_type: self.editor_type,
            start_time,
            language_id: self.text_model.get_language_id(),
            reason,
            typing_interval: typing_interval.average_interval as f64,
            typing_interval_character_count: typing_interval.character_count as i32,
            available_providers: Vec::new(),
            sku: self.sku.get(),
        };

        let mut context = InlineCompletionContextWithoutUuid {
            trigger_kind,
            selected_suggestion_info: suggest_item.as_ref().map(|s| s.to_selected_suggestion_info()),
            user_prompt: None,
            request_uuid: String::new(),
            include_inline_completions: !change_summary.only_request_inline_edits.get(),
            include_inline_edits: self.inline_edits_enabled.read(reader),
            request_issued_date_time: request_info.start_time,
            earliest_shown_date_time: request_info.start_time
                + if trigger_kind == InlineCompletionTriggerKind::Explicit || self.in_accept_flow.get() { 0.0 } else { self.min_show_delay.get() },
            change_hint: change_summary.change_hint.borrow().clone(),
        };

        if context.trigger_kind == InlineCompletionTriggerKind::Automatic
            && change_summary.text_change.get()
            && let Some(last_shown) = self.last_shown.borrow().as_ref()
            && self.text_model.get_alternative_version_id() == last_shown.alternate_text_model_version_id
        {
            // When undoing back to a version where an inline edit/completion was shown,
            // we want to show an inline edit (or completion) again if it was originally an inline edit (or completion).
            context.include_inline_completions = !last_shown.inline_completion.is_inline_edit();
            context.include_inline_edits = last_shown.inline_completion.is_inline_edit();
        }

        let item_to_preserve_candidate: Option<InlineSuggestionItem> = self
            .selected_inline_completion
            .get()
            .map(InlineSuggestionItem::Completion)
            .or_else(|| self.inline_suggestion_items.get().and_then(|i| i.inline_edit.clone()).map(InlineSuggestionItem::Edit));
        let item_to_preserve = item_to_preserve_candidate
            .filter(|candidate| change_summary.preserve_current_completion.get() || candidate.forward_stable());
        let user_jumped_to_active_completion: IObservable<bool> = {
            let (jumped_to_id, items) = (Rc::clone(&self.jumped_to_id), Rc::clone(&self.inline_suggestion_items));
            jumped_to_id.map(move |jumped_to, _| {
                jumped_to.is_some_and(|jumped_to| {
                    items.get().and_then(|i| i.inline_edit.as_ref().map(|e| e.semantic_id())).as_deref() == Some(jumped_to.as_str())
                })
            })
        };

        let (providers, label) = match change_summary.provider.borrow().clone() {
            Some(provider) => {
                let label = format!("single:{}", provider.provider_id().map_or_else(|| "undefined".to_owned(), |id| id.to_string()));
                (vec![provider], Some(label))
            }
            None => (self.language_features_service.inline_completions_provider.all(&self.text_model), None), // TODO: should use inlineCompletionProviders (comentário do original)
        };
        let available = available_providers(&providers, &self.suppressed_group_ids.get());
        request_info.available_providers = available.iter().filter_map(|p| p.provider_id().cloned()).collect();

        Some(self.source.fetch(
            available,
            label,
            context,
            item_to_preserve.map(|i| Rc::clone(&i.base().identity)),
            change_summary.should_debounce.get(),
            user_jumped_to_active_completion,
            request_info,
        ))
    }
}
