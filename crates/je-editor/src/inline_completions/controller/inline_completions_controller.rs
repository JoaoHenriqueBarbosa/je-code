//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/controller/inlineCompletionsController.ts`: o
//! `InlineCompletionsController`, a contribuição do editor que cria o `InlineCompletionsModel` e a `InlineSuggestionsView`,
//! decide quando disparar e quando parar as sugestões em linha (digitar, colar, comandos, mudança de seleção, perda de
//! foco, vários editores), liga as chaves de contexto, toca o sinal de acessibilidade e lê a sugestão para leitores de tela.
//!
//! Notas de tradução:
//! - `@IInstantiationService` (que criava o modelo, a view e o `TextModelChangeRecorder`) e os demais `@I...` do construtor
//!   são o [`InlineCompletionsControllerServices`]. O `ICommandService`, o `IAccessibilityService`, o `IKeybindingService` e
//!   o `IContextKeyService` do editor vêm do `InlineEditsViewServices` que a view já recebe, e o
//!   `IConfigurationService`, o `ILanguageConfigurationService` e o `ITextModelService` vêm do
//!   `InlineCompletionsSourceServices` que o modelo recebe: cada serviço tem um só lugar. O `IContextKeyService` é o do
//!   próprio editor (o escopo que o `IInstantiationService` do editor injeta);
//! - `createHotClass(this)`, `InlineCompletionsController.hot`, `hotClassGetOriginalInstance` e `wrapInHotClass1` (recarga
//!   a quente do desenvolvimento do VS Code) não existem: `get` devolve a contribuição como ela é;
//! - `static _instances = new Set<InlineCompletionsController>()` é um `thread_local` de `Weak`, na ordem de criação; as
//!   iterações pegam uma cópia dos controllers vivos antes de agir (um deles pode mexer na lista);
//! - o controller é um `Rc<Self>` criado por `Rc::new_cyclic`: os callbacks que o original deixa para o
//!   `ObservableSuggestWidgetAdapter` (`handleSuggestAccepted` e o pré-selecionador) leem o `model` do controller, então
//!   guardam só um `Weak`. Os `autorun`, ouvintes e ligações de chaves de contexto do construtor rodam em
//!   [`InlineCompletionsController::init`], depois que o `Rc` existe, na ordem do original;
//! - `derivedDisposable<InlineCompletionsModel | undefined>` do `model` e `derived(reader => reader.store.add(createInstance(
//!   InlineSuggestionsView...)))` da `_view` são `derived_with_store`: o valor é descartado na próxima computação ou quando o
//!   observável fica sem observadores;
//! - `this._contextKeyService.getContext(this.editor.getDomNode())` é o `get_context(None)` do serviço do próprio editor (o
//!   editor do porte não tem nó DOM; o serviço injetado já é o escopado nele);
//! - `alert(...)` de `base/browser/ui/aria` é o `IAccessibilityService::alert`;
//! - `runOnChangeWithStore(derived(... return {}), async ...)` é um `run_on_change_with_store` sobre um `Rc<()>` novo a cada
//!   cálculo (o `{}` do original nunca é igual ao anterior); o `async` é um [`async_fn`], e o `await` que rejeita por
//!   cancelamento (o `store` descartado) encerra a função em silêncio, como o `CancellationError` ignorado do original;
//! - `runOnChange(editorObs.selections, (_, _, changes) => ...)` recebe as mudanças como [`EditorChange`] (o evento de
//!   cursor dentro de um `Rc<dyn Any>`);
//! - `NextMarkerAction.ID` e as outras três ações do `gotoError` ainda não têm porte: os ids entram como constantes deste
//!   arquivo, com o valor do original;
//! - `FadeoutDecoration`, a lista de editores de diff e o `openCodeEditor` do `ICodeEditorService` entram pelo
//!   [`IInlineCompletionsCodeEditorService`]: o `FadeoutDecoration` anima a opacidade por uma variável de estilo do contêiner
//!   (`--animation-opacity`), que é do pintor, e o editor de diff não existe no porte. O [`InlineCompletionsControllerHost`]
//!   implementa o [`InlineCompletionsEditorHost`] do modelo: o que depende do controller
//!   (`getInlineCompletionsController(editor)?.model.get()`) e do editor focado ou ativo ele resolve, o resto vai a esse
//!   serviço;
//! - o `InlineCompletionsController` implementa o [`IInlineCompletionsHoverController`] do participante de hover
//!   (`shouldShowHoverAt` e `shouldShowHoverAtViewZone`) e o [`IInlineCompletionsController`] do `SuggestModel`, que lê só o
//!   `status`, o `state` e o `stop` do modelo (o [`InlineCompletionsHost`] é o `IInlineCompletionsHost` dele). Os
//!   observáveis `status` e `state` dessa visão do modelo são montados uma vez por modelo, e o `suggest_model` do controller
//!   é mantido observado, para o modelo ter a mesma identidade a cada leitura.
//!
//! O que fica fora deste arquivo: a `InlineEditsOnboardingExperience` (`inlineEditsNewUsers.ts`) não é criada pelo controller,
//! e sim pela `InlineSuggestionsView` (é uma peça da view, ligada pelo contrato anotado no módulo dela), e as ações e o
//! registro de hover de `inlineCompletions.contribution.ts` e `controller/commands.ts` (próximas fatias).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, timeout_with_token};
use je_base::cancellation::cancel_on_dispose;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::observable::{
    IObservable, ITransaction, Observable, ObservableExt, ObservableSignal, StrictEquals, autorun, const_observable, derived,
    derived_observable_with_cache, derived_with_store, observable_from_event, observable_signal, observable_value, run_on_change,
    run_on_change_with_store, transaction, wait_for_state,
};
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_platform::accessibility::IAccessibilityService;
use je_platform::commands::{CommandEvent, ICommandService};
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::{IContextKeyChangeEvent, IContextKeyService};
use je_platform::keybinding::IKeybindingService;
use je_platform::log::LoggerService;
use je_platform::resources::host_environment;

use crate::code_action::platform::{AccessibilitySignal, IAccessibilitySignalService};
use crate::code_editor_widget::{CodeEditorWidget, EditorContributionInstantiation, ICodeEditor, IEditorContribution};
use crate::config::editor_options::inline_suggest::INLINE_SUGGEST;
use crate::config::editor_options::registry::{INLINE_COMPLETIONS_ACCESSIBILITY_VERBOSE, SCREEN_READER_ANNOUNCE_INLINE_SUGGESTION};
use crate::core_commands::core_editing_commands;
use crate::cursor_common::CursorSimpleModel;
use crate::cursor_events::{CursorChangeReason, ICursorSelectionChangedEvent};
use crate::edits::text_edit::TextReplacement;
use crate::editor_extensions::{EditorCommand, ICodeEditorLookup, ServicesAccessor, register_editor_contribution};
use crate::embedded_code_editor_widget::get_outer_editor;
use crate::find::find_model::find_ids;
use crate::inline_completions::change_recorder::{ChangeReasonLogger, TextModelChangeRecorder};
use crate::inline_completions::controller::command_ids::{INLINE_SUGGEST_COMMIT_ID, JUMP_TO_NEXT_INLINE_EDIT_ID};
use crate::inline_completions::controller::common::{get_inline_completions_controller, set_inline_completions_controller_getter};
use crate::inline_completions::controller::inline_completion_context_keys::inline_completion_context_keys;
use crate::inline_completions::hints_widget::hover_participant::IInlineCompletionsHoverController;
use crate::inline_completions::hints_widget::inline_completions_hints_widget::InlineSuggestionHintsContentWidget;
use crate::inline_completions::inline_completions_model::{
    GhostTextModelState, IDefaultAccountService, IInlineCompletionsService, InlineCompletionsEditorHost, InlineCompletionsModel,
    InlineCompletionsModelServices, InlineCompletionsModelState, InlineEditModelState, ModelStatus, StopReason, SuggestItemInfo,
    TriggerOptions,
};
use crate::inline_completions::inline_completions_source::InlineCompletionsSourceServices;
use crate::inline_completions::inline_suggestion_item::{InlineSuggestionAction, InlineSuggestionItemBase};
use crate::inline_completions::suggest_widget_adapter::{ObservableSuggestWidgetAdapter, SuggestWidgetHost};
use crate::inline_completions::utils::ObservableContextKeyService;
use crate::inline_completions::view::inline_edits::inline_edits_view::InlineEditsViewServices;
use crate::inline_completions::view::inline_suggestions_view::InlineSuggestionsView;
use crate::language_feature_debounce::{DebounceConfig, ILanguageFeatureDebounceService};
use crate::language_feature_registry::AnyLanguageFeatureRegistry;
use crate::language_features_service::LanguageFeaturesService;
use crate::lines_operations::InsertLineAction;
use crate::multicursor::listen_weak;
use crate::observable_code_editor::{EditorChange, ObservableCodeEditor, observable_code_editor};
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::snippet::snippet_controller2::SnippetController2;
use crate::suggest::suggest_model::{
    IInlineCompletionsController, IInlineCompletionsHost, IInlineCompletionsModel, InlineCompletionsModelState as SuggestModelState,
    InlineCompletionsStatus, InlineCompletionsStopReason,
};
use crate::text_model::TextModel;
use crate::trigger_inline_edit_commands_registry::TriggerInlineEditCommandsRegistry;

/// A chave de contexto do ditado do editor.
const EDITOR_DICTATION_IN_PROGRESS: &str = "editorDictation.inProgress";
/// A chave de contexto da visão acessível aberta.
const ACCESSIBLE_VIEW_IS_SHOWN: &str = "accessibleViewIsShown";
/// O comando que abre a visão acessível.
const ACCESSIBLE_VIEW_COMMAND_ID: &str = "editor.action.accessibleView";
/// A configuração do detalhamento da acessibilidade das sugestões em linha.
const ACCESSIBILITY_VERBOSITY_SETTING: &str = "accessibility.verbosity.inlineCompletions";
/// A configuração escondida que mantém a sugestão ao perder o foco (útil para depurar).
const KEEP_ON_BLUR_SETTING: &str = "editor.inlineSuggest.keepOnBlur";
/// O comando do suggest widget que aceita o item selecionado.
const ACCEPT_SELECTED_SUGGESTION_ID: &str = "acceptSelectedSuggestion";
/// `NextMarkerAction.ID` (`gotoError.ts`).
const NEXT_MARKER_ACTION_ID: &str = "editor.action.marker.next";
/// `PrevMarkerAction.ID` (`gotoError.ts`).
const PREV_MARKER_ACTION_ID: &str = "editor.action.marker.prev";
/// `NextMarkerInFilesAction.ID` (`gotoError.ts`).
const NEXT_MARKER_IN_FILES_ACTION_ID: &str = "editor.action.marker.nextInFiles";
/// `PrevMarkerInFilesAction.ID` (`gotoError.ts`).
const PREV_MARKER_IN_FILES_ACTION_ID: &str = "editor.action.marker.prevInFiles";

thread_local! {
    /// `InlineCompletionsController._instances`.
    static INSTANCES: RefCell<Vec<Weak<InlineCompletionsController>>> = const { RefCell::new(Vec::new()) };
    /// O registro da contribuição acontece uma vez por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// Os controllers vivos, na ordem de criação (uma cópia: quem itera pode mexer na lista).
fn instances() -> Vec<Rc<InlineCompletionsController>> {
    INSTANCES.with(|instances| instances.borrow().iter().filter_map(Weak::upgrade).collect())
}

/// `isEqual(a, b)` de `resources.ts` (o `ExtUri` sem viés).
fn uris_equal(a: Option<&URI>, b: Option<&URI>) -> bool {
    ExtUri::unbiased(host_environment()).is_equal(a, b, false)
}

/// `getContext(editor.getDomNode())?.getValue('editorDictation.inProgress') === true`.
fn is_editor_dictation_in_progress(context_key_service: &dyn IContextKeyService) -> bool {
    matches!(context_key_service.get_context(None).get_value(EDITOR_DICTATION_IN_PROGRESS), Dynamic::Bool(true))
}

/// O valor `boolean | undefined` de uma chave de contexto.
fn optional_bool(value: Option<bool>) -> Dynamic {
    value.map_or(Dynamic::Undefined, Dynamic::Bool)
}

/// O estado quando é uma sugestão de texto fantasma (`state.kind === 'ghostText'`).
fn as_ghost_text(state: &Option<Rc<InlineCompletionsModelState>>) -> Option<&GhostTextModelState> {
    match state.as_deref() {
        Some(InlineCompletionsModelState::GhostText(state)) => Some(state),
        _ => None,
    }
}

/// O estado quando é uma edição em linha (`state.kind === 'inlineEdit'`).
fn as_inline_edit(state: &Option<Rc<InlineCompletionsModelState>>) -> Option<&InlineEditModelState> {
    match state.as_deref() {
        Some(InlineCompletionsModelState::InlineEdit(state)) => Some(state),
        _ => None,
    }
}

// #region Igualdade estrita

impl StrictEquals for InlineCompletionsStatus {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

impl StrictEquals for SuggestModelState {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

// #endregion

// #region Serviços injetados

/// O que o controller pede ao `ICodeEditorService` e à animação do `FadeoutDecoration` e que não é dele (ver as notas do
/// módulo).
pub trait IInlineCompletionsCodeEditorService {
    /// `listDiffEditors().some(d => d.getOriginalEditor().getId() === id || d.getModifiedEditor().getId() === id)`.
    fn is_in_diff_editor(&self, editor_id: &str) -> bool;

    /// `openCodeEditor({ resource }, sourceEditor)`.
    fn open_code_editor(&self, resource: URI, source_editor: &Rc<CodeEditorWidget>) -> Promise<Option<Rc<CodeEditorWidget>>>;

    /// O `FadeoutDecoration`: realça `ranges` e some sozinho em 1 segundo. `on_finished` avisa o fim, para o modelo soltar o
    /// descartável.
    fn start_edits_fadeout(&self, editor: &Rc<CodeEditorWidget>, ranges: Vec<Range>, on_finished: Rc<dyn Fn()>) -> DisposableRef;
}

/// Os serviços que o construtor do controller recebe por injeção (ver as notas do módulo).
pub struct InlineCompletionsControllerServices {
    /// Os serviços da view; o controller lê deles o comando, a acessibilidade, o atalho e as chaves de contexto do editor.
    pub view_services: InlineEditsViewServices,
    /// Os serviços da fonte de sugestões de cada modelo; o controller lê deles a configuração.
    pub source_services: InlineCompletionsSourceServices,
    /// `@ILanguageFeatureDebounceService`.
    pub debounce_service: Rc<dyn ILanguageFeatureDebounceService>,
    /// `@ILanguageFeaturesService`.
    pub language_features_service: Rc<LanguageFeaturesService>,
    /// `@IAccessibilitySignalService`.
    pub accessibility_signal_service: Rc<dyn IAccessibilitySignalService>,
    /// `@IInlineCompletionsService`, para o modelo.
    pub inline_completions_service: Rc<dyn IInlineCompletionsService>,
    /// `@IDefaultAccountService`, para o modelo.
    pub default_account_service: Rc<dyn IDefaultAccountService>,
    /// `product.defaultChatAgent?.completionsEnablementSetting`, para o modelo.
    pub completions_enablement_setting: Option<String>,
    /// O que o `ObservableSuggestWidgetAdapter` pede ao `SuggestController` e depende do `SuggestWidget`.
    pub suggest_widget_host: Rc<dyn SuggestWidgetHost>,
    /// O que o [`InlineCompletionsControllerHost`] entrega ao serviço de editores.
    pub code_editor_service: Rc<dyn IInlineCompletionsCodeEditorService>,
    /// `getFocusedCodeEditor()` e `getActiveCodeEditor()` do `ICodeEditorService`.
    pub code_editor_lookup: Rc<dyn ICodeEditorLookup>,
    /// `@ILoggerService`, para o `TextModelChangeRecorder`.
    pub logger_service: Rc<dyn LoggerService>,
    /// `createInstance(StructuredLogger.cast<...>(), key)` do `TextModelChangeRecorder`: um logger novo a cada chamada.
    pub create_change_reason_logger: Rc<dyn Fn() -> Rc<dyn ChangeReasonLogger>>,
}

/// O [`InlineCompletionsEditorHost`] do modelo (ver as notas do módulo).
pub struct InlineCompletionsControllerHost {
    code_editor_service: Rc<dyn IInlineCompletionsCodeEditorService>,
    code_editor_lookup: Rc<dyn ICodeEditorLookup>,
}

impl InlineCompletionsControllerHost {
    pub fn new(code_editor_service: Rc<dyn IInlineCompletionsCodeEditorService>, code_editor_lookup: Rc<dyn ICodeEditorLookup>) -> Self {
        Self { code_editor_service, code_editor_lookup }
    }
}

impl InlineCompletionsEditorHost for InlineCompletionsControllerHost {
    fn is_in_diff_editor(&self, editor_id: &str) -> bool {
        self.code_editor_service.is_in_diff_editor(editor_id)
    }

    /// `(getFocusedCodeEditor() || getActiveCodeEditor()) === editor`.
    fn is_focused_or_active_editor(&self, editor: &Rc<CodeEditorWidget>) -> bool {
        self.code_editor_lookup
            .get_focused_code_editor()
            .or_else(|| self.code_editor_lookup.get_active_code_editor())
            .is_some_and(|current| Rc::ptr_eq(&current, editor))
    }

    fn open_code_editor(&self, resource: URI, source_editor: &Rc<CodeEditorWidget>) -> Promise<Option<Rc<CodeEditorWidget>>> {
        self.code_editor_service.open_code_editor(resource, source_editor)
    }

    /// `getInlineCompletionsController(editor)?.model.get()`.
    fn inline_completions_model_of(&self, editor: &Rc<CodeEditorWidget>) -> Option<Rc<InlineCompletionsModel>> {
        get_inline_completions_controller::<InlineCompletionsController>(editor)?.model.get()
    }

    fn start_edits_fadeout(&self, editor: &Rc<CodeEditorWidget>, ranges: Vec<Range>, on_finished: Rc<dyn Fn()>) -> DisposableRef {
        self.code_editor_service.start_edits_fadeout(editor, ranges, on_finished)
    }
}

// #endregion

// #region O que o SuggestModel lê do controller

/// O `InlineCompletionsModel` como o `SuggestModel` o lê: `status`, `state` e `stop`.
struct SuggestModelView {
    model: Rc<InlineCompletionsModel>,
    status: IObservable<InlineCompletionsStatus>,
    state: IObservable<Option<SuggestModelState>>,
}

impl SuggestModelView {
    fn new(model: Rc<InlineCompletionsModel>) -> Rc<dyn IInlineCompletionsModel> {
        let status = model.status.map(|status, _| match status {
            ModelStatus::Loading => InlineCompletionsStatus::Loading,
            ModelStatus::GhostText => InlineCompletionsStatus::GhostText,
            ModelStatus::InlineEdit => InlineCompletionsStatus::InlineEdit,
            ModelStatus::NoSuggestion => InlineCompletionsStatus::NoSuggestion,
        });
        let state = model
            .state
            .map(|state, _| state.map(|state| SuggestModelState { has_inline_suggestion: state.inline_suggestion().is_some() }));
        Rc::new(Self { model, status, state })
    }
}

impl IInlineCompletionsModel for SuggestModelView {
    fn status(&self) -> IObservable<InlineCompletionsStatus> {
        Rc::clone(&self.status)
    }

    fn state(&self) -> IObservable<Option<SuggestModelState>> {
        Rc::clone(&self.state)
    }

    fn stop(&self, reason: InlineCompletionsStopReason) {
        self.model.stop(
            match reason {
                InlineCompletionsStopReason::ExplicitCancel => StopReason::ExplicitCancel,
                InlineCompletionsStopReason::Automatic => StopReason::Automatic,
            },
            None,
        );
    }
}

/// O `IInlineCompletionsHost` do `SuggestModel`: se há provedor para o modelo de texto e o controller do editor.
pub struct InlineCompletionsHost {
    language_features_service: Rc<LanguageFeaturesService>,
}

impl InlineCompletionsHost {
    pub fn new(language_features_service: Rc<LanguageFeaturesService>) -> Self {
        Self { language_features_service }
    }
}

impl IInlineCompletionsHost for InlineCompletionsHost {
    /// `languageFeaturesService.inlineCompletionsProvider.has(model)`.
    fn has_provider(&self, model: &TextModel) -> bool {
        self.language_features_service.inline_completions_provider.has(model)
    }

    /// `getInlineCompletionsController(editor)`.
    fn get_controller(&self, editor: &Rc<CodeEditorWidget>) -> Option<Rc<dyn IInlineCompletionsController>> {
        InlineCompletionsController::get(editor).map(|controller| controller as Rc<dyn IInlineCompletionsController>)
    }
}

// #endregion

/// `InlineCompletionsController`.
pub struct InlineCompletionsController {
    me: Weak<Self>,
    base: Rc<Disposable>,
    /// `editor`.
    pub editor: Rc<CodeEditorWidget>,
    editor_obs: Rc<ObservableCodeEditor>,

    context_key_service: Rc<dyn IContextKeyService>,
    configuration_service: Rc<dyn IConfigurationService>,
    command_service: Rc<dyn ICommandService>,
    accessibility_signal_service: Rc<dyn IAccessibilitySignalService>,
    accessibility_service: Rc<dyn IAccessibilityService>,
    keybinding_service: Rc<dyn IKeybindingService>,

    suggest_widget_adapter: Rc<ObservableSuggestWidgetAdapter>,
    enabled_in_config: IObservable<bool>,
    focus_is_in_editor_or_menu: IObservable<bool>,
    cursor_is_in_indentation: IObservable<bool>,
    hide_inline_edit_on_selection_change: IObservable<bool>,

    /// `model`.
    pub model: IObservable<Option<Rc<InlineCompletionsModel>>>,
    suggest_model: IObservable<Option<Rc<dyn IInlineCompletionsModel>>>,
    accessibility_signal_trigger: Rc<ObservableSignal<()>>,
    view: IObservable<Rc<InlineSuggestionsView>>,
}

impl InlineCompletionsController {
    /// `InlineCompletionsController.ID`.
    pub const ID: &'static str = "editor.contrib.inlineCompletionsController";

    /// `InlineCompletionsController.getInFocusedEditorOrParent(accessor)`: o controller do editor com foco ou do editor
    /// externo (se o focado é um editor embutido).
    pub fn get_in_focused_editor_or_parent(accessor: &ServicesAccessor) -> Option<Rc<InlineCompletionsController>> {
        let outer_editor = get_outer_editor(accessor)?;
        Self::get(&outer_editor)
    }

    /// `InlineCompletionsController.get(editor)`.
    pub fn get(editor: &Rc<CodeEditorWidget>) -> Option<Rc<InlineCompletionsController>> {
        editor.get_contribution_as::<InlineCompletionsController>(Self::ID)
    }

    pub fn new(editor: &Rc<CodeEditorWidget>, services: InlineCompletionsControllerServices) -> Rc<Self> {
        let InlineCompletionsControllerServices {
            view_services,
            source_services,
            debounce_service,
            language_features_service,
            accessibility_signal_service,
            inline_completions_service,
            default_account_service,
            completions_enablement_setting,
            suggest_widget_host,
            code_editor_service,
            code_editor_lookup,
            logger_service,
            create_change_reason_logger,
        } = services;

        let base = Disposable::new();
        let editor_obs = observable_code_editor(editor);
        let context_key_service = Rc::clone(&view_services.context_key_service);
        let configuration_service = Rc::clone(&source_services.configuration_service);
        let command_service = Rc::clone(&view_services.command_service);
        let accessibility_service = Rc::clone(&view_services.accessibility_service);
        let keybinding_service = Rc::clone(&view_services.keybinding_service);

        let positions: IObservable<Rc<Vec<Position>>> = {
            let selections = Rc::clone(&editor_obs.selections);
            derived(move |reader| {
                Rc::new(selections.read(reader).map_or_else(
                    || vec![Position::new(1, 1)],
                    |selections| selections.iter().map(|selection| selection.get_end_position()).collect(),
                ))
            })
        };

        let enabled_in_config: IObservable<bool> = {
            let editor = Rc::clone(editor);
            observable_from_event(editor.on_did_change_configuration(), move |_| editor.get_option(INLINE_SUGGEST).enabled)
        };

        let focus_is_in_menu = observable_value::<bool, ()>("focusIsInMenu", false);
        let focus_is_in_editor_or_menu: IObservable<bool> = {
            let (editor_obs, focus_is_in_menu) = (Rc::clone(&editor_obs), focus_is_in_menu.to_observable());
            derived(move |reader| {
                let editor_has_focus = editor_obs.is_focused.read(reader);
                let menu_has_focus = focus_is_in_menu.read(reader);
                editor_has_focus || menu_has_focus
            })
        };

        let cursor_is_in_indentation: IObservable<bool> = {
            let editor_obs = Rc::clone(&editor_obs);
            derived(move |reader| {
                let Some(cursor_pos) = editor_obs.cursor_position.read(reader) else {
                    return false;
                };
                let Some(model) = editor_obs.model.read(reader) else {
                    return false;
                };
                editor_obs.version_id.read(reader);
                let indent_max_column = model.get_line_indent_column(cursor_pos.line_number);
                cursor_pos.column <= indent_max_column
            })
        };

        let debounce_registry: Rc<dyn AnyLanguageFeatureRegistry> = language_features_service.inline_completions_provider.clone();
        let debounce_value = debounce_service.for_feature(
            debounce_registry,
            "InlineCompletionsDebounce",
            Some(DebounceConfig { min: Some(50.0), max: Some(50.0), key: None }),
        );

        let host: Rc<dyn InlineCompletionsEditorHost> = Rc::new(InlineCompletionsControllerHost::new(code_editor_service, code_editor_lookup));

        // Os serviços de cada modelo: `createInstance(InlineCompletionsModel, ...)` cria um modelo a cada troca do modelo de texto.
        let model_services: Rc<dyn Fn() -> InlineCompletionsModelServices> = {
            let (editor, editor_obs) = (Rc::clone(editor), Rc::clone(&editor_obs));
            let (command_service, accessibility_service) = (Rc::clone(&command_service), Rc::clone(&accessibility_service));
            Rc::new(move || InlineCompletionsModelServices {
                command_service: Rc::clone(&command_service),
                language_configuration_service: Rc::clone(&source_services.language_configuration_service),
                accessibility_service: Rc::clone(&accessibility_service),
                language_features_service: Rc::clone(&language_features_service),
                inline_completions_service: Rc::clone(&inline_completions_service),
                default_account_service: Rc::clone(&default_account_service),
                host: Rc::clone(&host),
                opened_peek_widgets: editor_obs.opened_peek_widgets.to_observable(),
                is_in_snippet_mode: SnippetController2::get(&editor)
                    .map_or_else(|| const_observable(false), |controller| controller.is_in_snippet_observable()),
                completions_enablement_setting: completions_enablement_setting.clone(),
                source_services: source_services.clone(),
            })
        };

        let this = Rc::new_cyclic(|me: &Weak<Self>| {
            let suggest_widget_adapter = ObservableSuggestWidgetAdapter::new(
                &editor_obs,
                suggest_widget_host,
                {
                    let me = me.clone();
                    Rc::new(move |item: &SuggestItemInfo| {
                        if let Some(model) = me.upgrade().and_then(|this| this.model.get()) {
                            model.handle_suggest_accepted(item);
                        }
                    })
                },
                {
                    let me = me.clone();
                    Rc::new(move || -> Option<TextReplacement> {
                        let model = me.upgrade()?.model.get()?;
                        model.selected_inline_completion.get().map(|completion| completion.get_single_text_edit())
                    })
                },
            );
            base.register(suggest_widget_adapter.clone());

            let model: IObservable<Option<Rc<InlineCompletionsModel>>> = {
                let (editor, editor_obs, context_key_service) = (Rc::clone(editor), Rc::clone(&editor_obs), Rc::clone(&context_key_service));
                let (selected_item, enabled) = (Rc::clone(&suggest_widget_adapter.selected_item), Rc::clone(&enabled_in_config));
                derived_with_store(move |reader, store| {
                    if editor_obs.is_readonly.read(reader) {
                        return None;
                    }
                    let text_model = editor_obs.model.read(reader)?;

                    let is_suppressed: Rc<dyn Fn() -> bool> = {
                        let context_key_service = Rc::clone(&context_key_service);
                        Rc::new(move || is_editor_dictation_in_progress(&*context_key_service))
                    };
                    Some(store.add_rc(InlineCompletionsModel::new(
                        text_model,
                        Rc::clone(&selected_item),
                        Rc::clone(&editor_obs.version_id),
                        Rc::clone(&positions),
                        Rc::clone(&debounce_value),
                        Rc::clone(&enabled),
                        is_suppressed,
                        Rc::clone(&editor),
                        model_services(),
                    )))
                })
            };

            let suggest_model: IObservable<Option<Rc<dyn IInlineCompletionsModel>>> =
                model.map(|model, _| model.map(SuggestModelView::new));

            let view: IObservable<Rc<InlineSuggestionsView>> = {
                let (editor, model, focus_is_in_menu) = (Rc::clone(editor), Rc::clone(&model), Rc::clone(&focus_is_in_menu));
                derived_with_store(move |_reader, store| {
                    store.add_rc(InlineSuggestionsView::new(
                        Rc::clone(&editor),
                        Rc::clone(&model),
                        Rc::clone(&focus_is_in_menu),
                        view_services.clone(),
                    ))
                })
            };

            Self {
                me: me.clone(),
                base: Rc::clone(&base),
                editor: Rc::clone(editor),
                editor_obs: Rc::clone(&editor_obs),
                context_key_service,
                configuration_service,
                command_service,
                accessibility_signal_service,
                accessibility_service,
                keybinding_service,
                suggest_widget_adapter,
                enabled_in_config,
                focus_is_in_editor_or_menu,
                cursor_is_in_indentation,
                hide_inline_edit_on_selection_change: editor_obs.get_option(INLINE_SUGGEST).map(|_, _| true),
                model,
                suggest_model,
                accessibility_signal_trigger: observable_signal::<()>("playAccessibilitySignal"),
                view,
            }
        });
        this.init();

        this.base.register(TextModelChangeRecorder::new(editor, &*logger_service, create_change_reason_logger()));

        this
    }

    /// O corpo do construtor depois dos campos: os ouvintes, os `autorun` e as chaves de contexto, na ordem do original.
    fn init(self: &Rc<Self>) {
        let base = &self.base;

        base.register(listen_weak(&self.me, self.context_key_service.on_did_change_context(), {
            let keys: HashSet<String> = HashSet::from([EDITOR_DICTATION_IN_PROGRESS.to_owned()]);
            move |this: &Self, e: &Rc<dyn IContextKeyChangeEvent>| {
                if e.affects_some(&keys)
                    && this.is_editor_dictation_in_progress()
                    && let Some(model) = this.model.get()
                {
                    model.stop(StopReason::Automatic, None);
                }
            }
        }));

        let _ = self.model.recompute_initially_and_on_change(base.store(), None);
        let _ = self.view.recompute_initially_and_on_change(base.store(), None);
        let _ = self.suggest_model.keep_observed(base.store());

        INSTANCES.with(|instances| instances.borrow_mut().push(self.me.clone()));
        let me = self.me.clone();
        base.register(to_disposable(move || INSTANCES.with(|instances| instances.borrow_mut().retain(|other| !Weak::ptr_eq(other, &me)))));

        self.reject_other_controllers();
        self.register_triggers();
        self.handle_focus();
        self.handle_suggest_widget();
        self.handle_accessibility();
        self.bind_context_keys();
    }

    /// Os dois `autorun` que cancelam as sugestões dos outros editores quando esta começa, e escondem a edição que aponta
    /// para este editor quando ela foi aceita.
    fn reject_other_controllers(self: &Rc<Self>) {
        let me = self.me.clone();
        self.base.register(autorun(move |reader| {
            // Cancel all other inline completions when a new one starts
            let Some(this) = me.upgrade() else {
                return;
            };
            let Some(model) = this.model.read(reader) else {
                return;
            };
            let Some(state) = model.state.read(reader) else {
                return;
            };
            if !this.focus_is_in_editor_or_menu.get() {
                return;
            }

            // This controller is in focus, hence reject others.
            // However if we display a NES that relates to another edit then trigger NES on that related controller
            let next_edit_uri = match &*state {
                InlineCompletionsModelState::InlineEdit(state) => state.next_edit_uri.clone(),
                InlineCompletionsModelState::GhostText(_) => None,
            };
            for ctrl in instances() {
                if Rc::ptr_eq(&ctrl, &this) {
                    continue;
                }
                let ctrl_uri = ctrl.editor.get_model().map(|model| model.uri());
                if next_edit_uri.is_some() && uris_equal(next_edit_uri.as_ref(), ctrl_uri.as_ref()) {
                    // The next edit in other edito is related to this controller, trigger it.
                    if let Some(model) = ctrl.model.get() {
                        let _ = model.trigger(None, TriggerOptions::default());
                    }
                } else {
                    ctrl.reject();
                }
            }
        }));

        let me = self.me.clone();
        self.base.register(autorun(move |reader| {
            let Some(this) = me.upgrade() else {
                return;
            };
            let model = this.model.read(reader);
            let uri = this.editor.get_model().map(|model| model.uri());
            let (Some(model), Some(uri)) = (model, uri) else {
                return;
            };

            // This NES was accepted, its possible there is an NES that points to this editor.
            // I.e. there's an NES that reads `Go To Next Edit`,
            // If there is one that points to this editor, then we need to hide that as this NES was accepted.
            let me = this.me.clone();
            reader.store().add(model.on_did_accept().listen(move |_| {
                let Some(this) = me.upgrade() else {
                    return;
                };
                for ctrl in instances() {
                    if Rc::ptr_eq(&ctrl, &this) {
                        continue;
                    }
                    // Find the nes from another editor that points to this.
                    let state = ctrl.model.get().and_then(|model| model.state.get());
                    if as_inline_edit(&state).is_some_and(|state| uris_equal(state.next_edit_uri.as_ref(), Some(&uri)))
                        && let Some(model) = ctrl.model.get()
                    {
                        model.stop(StopReason::Automatic, None);
                    }
                }
            }));
        }));
    }

    /// O disparo ao digitar, colar, executar certos comandos e ao mudar a seleção por um gesto explícito.
    fn register_triggers(self: &Rc<Self>) {
        self.base.register(self.trigger_on_signal(&self.editor_obs.on_did_type));
        self.base.register(self.trigger_on_signal(&self.editor_obs.on_did_paste));

        // These commands don't trigger onDidType.
        let core_commands = core_editing_commands::instance();
        let trigger_commands: HashSet<String> = [
            core_commands.tab.base().id.clone(),
            core_commands.delete_left.base().id.clone(),
            core_commands.delete_right.base().id.clone(),
            INLINE_SUGGEST_COMMIT_ID.to_owned(),
            ACCEPT_SELECTED_SUGGESTION_ID.to_owned(),
            InsertLineAction::AFTER_ID.to_owned(),
            InsertLineAction::BEFORE_ID.to_owned(),
            find_ids::NEXT_MATCH_FIND_ACTION.to_owned(),
            NEXT_MARKER_ACTION_ID.to_owned(),
            PREV_MARKER_ACTION_ID.to_owned(),
            NEXT_MARKER_IN_FILES_ACTION_ID.to_owned(),
            PREV_MARKER_IN_FILES_ACTION_ID.to_owned(),
        ]
        .into_iter()
        .chain(TriggerInlineEditCommandsRegistry::get_registered_commands())
        .collect();
        self.base.register(listen_weak(&self.me, self.command_service.on_did_execute_command(), move |this: &Self, e: &CommandEvent| {
            if trigger_commands.contains(&e.command_id)
                && this.editor.has_text_focus()
                && this.enabled_in_config.get()
                && !this.is_editor_dictation_in_progress()
            {
                let no_delay = e.command_id == INLINE_SUGGEST_COMMIT_ID;
                this.editor_obs.force_update_with(|tx| {
                    /* @description onDidExecuteCommand */
                    if let Some(model) = this.model.get() {
                        let _ = model.trigger(Some(tx), TriggerOptions { no_delay, ..TriggerOptions::default() });
                    }
                });
            }
        }));

        let me = self.me.clone();
        self.base.register(run_on_change::<Option<Rc<Vec<Selection>>>, EditorChange>(
            Rc::clone(&self.editor_obs.selections),
            move |_value, _previous, changes| {
                let Some(this) = me.upgrade() else {
                    return;
                };
                let is_explicit = changes.iter().any(|change| {
                    (**change)
                        .downcast_ref::<ICursorSelectionChangedEvent>()
                        .is_some_and(|event| event.reason == CursorChangeReason::Explicit || event.source == "api")
                });
                if !is_explicit {
                    return;
                }
                let state = this.model.get().and_then(|model| model.state.get());
                if !this.hide_inline_edit_on_selection_change.get() && as_inline_edit(&state).is_some() {
                    return;
                }
                let Some(model) = this.model.get() else {
                    return;
                };
                if as_ghost_text(&model.state.get()).is_some() {
                    model.stop(StopReason::Automatic, None);
                }
            },
        ));
    }

    /// O disparo de um sinal do editor (`onDidType` e `onDidPaste`): `runOnChange(signal, () => { if (enabled &&
    /// !dictation) model.get()?.trigger() })`.
    fn trigger_on_signal<C: Clone + 'static>(&self, signal: &Rc<ObservableSignal<C>>) -> DisposableRef {
        let me = self.me.clone();
        run_on_change::<(), C>(signal.to_observable(), move |_value, _previous, _changes| {
            if let Some(this) = me.upgrade()
                && this.enabled_in_config.get()
                && !this.is_editor_dictation_in_progress()
                && let Some(model) = this.model.get()
            {
                let _ = model.trigger(None, TriggerOptions::default());
            }
        })
    }

    /// O `autorun` do foco: com foco, para os outros editores; sem foco, esconde a sugestão (salvo as exceções).
    fn handle_focus(self: &Rc<Self>) {
        let me = self.me.clone();
        self.base.register(autorun(move |reader| {
            let Some(this) = me.upgrade() else {
                return;
            };
            let is_focused = this.focus_is_in_editor_or_menu.read(reader);
            let model = this.model.get();
            if is_focused {
                // If this model already has an NES for another editor, then leave as is
                // Else stop other models.
                let state = model.as_ref().and_then(|model| model.state.get());
                if as_inline_edit(&state).is_none_or(|state| state.next_edit_uri.is_none()) {
                    transaction(|tx| {
                        for ctrl in instances() {
                            if !Rc::ptr_eq(&ctrl, &this)
                                && let Some(model) = ctrl.model.get()
                            {
                                model.stop(StopReason::Automatic, Some(tx));
                            }
                        }
                    });
                }
                return;
            }

            // This is a hidden setting very useful for debugging
            if this.context_key_service.get_context_key_value(ACCESSIBLE_VIEW_IS_SHOWN).is_truthy()
                || this.configuration_service.get_value(Some(KEEP_ON_BLUR_SETTING), None).is_truthy()
                || this.editor.get_option(INLINE_SUGGEST).keep_on_blur
                || InlineSuggestionHintsContentWidget::drop_down_visible()
            {
                return;
            }

            let Some(model) = model else {
                return;
            };
            let explicit_request = model
                .state
                .get()
                .and_then(|state| state.inline_suggestion())
                .is_some_and(|inline_suggestion| inline_suggestion.is_from_explicit_request());
            if explicit_request && model.inline_edit_available.get() {
                // dont hide inline edits on blur when requested explicitly
                return;
            }

            transaction(|tx| {
                /* @description InlineCompletionsController.onDidBlurEditorWidget */
                model.stop(StopReason::Automatic, Some(tx));
            });
        }));
    }

    /// O `forceRenderingAbove` do suggest widget, enquanto há uma sugestão em linha de duas linhas ou mais.
    fn handle_suggest_widget(self: &Rc<Self>) {
        let me = self.me.clone();
        self.base.register(autorun(move |reader| {
            /* @description InlineCompletionsController.forceRenderingAbove */
            let Some(this) = me.upgrade() else {
                return;
            };
            let state = this.model.read(reader).and_then(|model| model.inline_completion_state.read(reader));
            match as_ghost_text(&state) {
                Some(state) if state.suggest_item.is_some() => {
                    if state.primary_ghost_text.line_count() >= 2 {
                        this.suggest_widget_adapter.suggest_widget_adaptor.force_rendering_above();
                    }
                }
                _ => this.suggest_widget_adapter.suggest_widget_adaptor.stop_force_rendering_above(),
            }
        }));
        let adapter = Rc::clone(&self.suggest_widget_adapter);
        self.base.register(to_disposable(move || adapter.suggest_widget_adaptor.stop_force_rendering_above()));
    }

    /// O sinal de acessibilidade e a leitura da sugestão, mais a opção de detalhamento da acessibilidade.
    fn handle_accessibility(self: &Rc<Self>) {
        let current_inline_completion_by_semantic_id: IObservable<Option<String>> = {
            let (model, adapter) = (Rc::clone(&self.model), Rc::clone(&self.suggest_widget_adapter));
            derived_observable_with_cache::<Option<String>>(None, move |reader, last| {
                let state = model.read(reader).and_then(|model| model.state.read(reader));
                if adapter.selected_item.get().is_some() {
                    return last.cloned().flatten();
                }
                state.and_then(|state| state.inline_suggestion()).map(|inline_suggestion| inline_suggestion.semantic_id())
            })
        };
        let trigger: IObservable<Rc<()>> = {
            let signal = self.accessibility_signal_trigger.to_observable();
            derived(move |reader| {
                signal.read(reader);
                current_inline_completion_by_semantic_id.read(reader);
                Rc::new(())
            })
        };
        let me = self.me.clone();
        self.base.register(run_on_change_with_store::<Rc<()>, ()>(trigger, move |_value, _previous, _deltas, store| {
            /* @description InlineCompletionsController.playAccessibilitySignalAndReadSuggestion */
            let Some(this) = me.upgrade() else {
                return;
            };
            let model = this.model.get();
            if model.as_ref().and_then(|model| model.state.get()).is_none() {
                return;
            }

            let (timeout_token, wait_token) = (cancel_on_dispose(store), cancel_on_dispose(store));
            let selected_item = Rc::clone(&this.suggest_widget_adapter.selected_item);
            let me = me.clone();
            let _ = async_fn(async move {
                if timeout_with_token(50, &timeout_token).await.is_err() {
                    return Ok(());
                }
                if wait_for_state(selected_item, |item| item.is_none(), None, Some(wait_token)).await.is_err() {
                    return Ok(());
                }

                let Some(this) = me.upgrade() else {
                    return Ok(());
                };
                let model = this.model.get();
                let state = model.as_ref().and_then(|model| model.state.get());
                if let (Some(model), Some(state)) = (model, state) {
                    this.announce_suggestion(&model, &state);
                }
                Ok(())
            });
        }));

        // TODO@hediet
        self.base.register(listen_weak(&self.me, self.configuration_service.on_did_change_configuration(), |this: &Self, e| {
            if e.affects_configuration(ACCESSIBILITY_VERBOSITY_SETTING, None) {
                this.update_accessibility_verbosity();
            }
        }));
        self.update_accessibility_verbosity();
    }

    /// `editor.updateOptions({ inlineCompletionsAccessibilityVerbose: getValue('accessibility.verbosity.inlineCompletions') })`.
    fn update_accessibility_verbosity(&self) {
        let verbose = self.configuration_service.get_value(Some(ACCESSIBILITY_VERBOSITY_SETTING), None);
        self.editor.update_options(Some(&Dynamic::object([("inlineCompletionsAccessibilityVerbose", verbose)])));
    }

    /// O sinal de acessibilidade da sugestão e, com `screenReaderAnnounceInlineSuggestion`, o texto para o leitor de tela.
    fn announce_suggestion(&self, model: &Rc<InlineCompletionsModel>, state: &InlineCompletionsModelState) {
        let announce = *self.editor.get_option(SCREEN_READER_ANNOUNCE_INLINE_SUGGESTION);
        match state {
            InlineCompletionsModelState::GhostText(ghost_text_state) => {
                let ghost_text = &ghost_text_state.primary_ghost_text;
                let line_text = CursorSimpleModel::get_line_content(&*model.text_model, ghost_text.line_number());
                self.accessibility_signal_service.play_signal(AccessibilitySignal::InlineSuggestion);

                if announce {
                    self.provide_screen_reader_update(&ghost_text.render_for_screen_reader(line_text.units()).to_string_lossy());
                }
            }
            InlineCompletionsModelState::InlineEdit(inline_edit_state) => {
                self.accessibility_signal_service.play_signal(AccessibilitySignal::NextEditSuggestion);

                if announce {
                    let line_number = inline_edit_state.inline_suggestion.target_range().start_line_number.to_string();
                    let content = if model.tab_should_accept_inline_edit.get() {
                        match self.aria_label_of(INLINE_SUGGEST_COMMIT_ID) {
                            Some(keybinding) => localize(
                                "nextEditSuggestionAcceptWithKb",
                                "Next edit suggestion available on line {0}, press {1} to accept",
                                &[&line_number, &keybinding],
                            ),
                            None => localize(
                                "nextEditSuggestionAcceptNoKb",
                                "Next edit suggestion available on line {0}, accept it to apply",
                                &[&line_number],
                            ),
                        }
                    } else if model.tab_should_jump_to_inline_edit.get() {
                        match self.aria_label_of(JUMP_TO_NEXT_INLINE_EDIT_ID) {
                            Some(keybinding) => localize(
                                "nextEditSuggestionJumpWithKb",
                                "Next edit suggestion available on line {0}, press {1} to jump",
                                &[&line_number, &keybinding],
                            ),
                            None => localize("nextEditSuggestionJumpNoKb", "Next edit suggestion available on line {0}", &[&line_number]),
                        }
                    } else {
                        localize("nextEditSuggestionNoAction", "Next edit suggestion available on line {0}", &[&line_number])
                    };
                    self.provide_screen_reader_update(&content);
                }
            }
        }
    }

    /// `this._keybindingService.lookupKeybinding(commandId)?.getAriaLabel()`, sem o rótulo vazio.
    fn aria_label_of(&self, command_id: &str) -> Option<String> {
        self.keybinding_service
            .lookup_keybinding(command_id, None, false)
            .and_then(|keybinding| keybinding.get_aria_label())
            .filter(|label| !label.is_empty())
    }

    /// `_isEditorDictationInProgress()`.
    fn is_editor_dictation_in_progress(&self) -> bool {
        is_editor_dictation_in_progress(&*self.context_key_service)
    }

    /// `playAccessibilitySignal(tx)`.
    pub fn play_accessibility_signal(&self, tx: &dyn ITransaction) {
        self.accessibility_signal_trigger.trigger(Some(tx), ());
    }

    /// `_provideScreenReaderUpdate(content)`.
    fn provide_screen_reader_update(&self, content: &str) {
        let accessible_view_showing = self.context_key_service.get_context_key_value(ACCESSIBLE_VIEW_IS_SHOWN).is_truthy();
        let accessible_view_keybinding = self.keybinding_service.lookup_keybinding(ACCESSIBLE_VIEW_COMMAND_ID, None, false);
        let hint = match accessible_view_keybinding {
            Some(keybinding) if !accessible_view_showing && *self.editor.get_option(INLINE_COMPLETIONS_ACCESSIBILITY_VERBOSE) => Some(
                localize(
                    "showAccessibleViewHint",
                    "Inspect this in the accessible view ({0})",
                    &[&keybinding.get_aria_label().unwrap_or_default()],
                ),
            ),
            _ => None,
        };
        self.accessibility_service.alert(&match hint {
            Some(hint) => format!("{content}, {hint}"),
            None => content.to_owned(),
        });
    }

    /// `reject()`: cancela a sugestão deste editor e, se ele tem o foco, as dos outros sem foco.
    pub fn reject(&self) {
        transaction(|tx| {
            let Some(model) = self.model.get() else {
                return;
            };
            model.stop(StopReason::ExplicitCancel, Some(tx));
            // Only if this controller is in focus can we cancel others.
            if self.focus_is_in_editor_or_menu.get() {
                for ctrl in instances() {
                    if !std::ptr::eq(Rc::as_ptr(&ctrl), self)
                        && !ctrl.focus_is_in_editor_or_menu.get()
                        && let Some(model) = ctrl.model.get()
                    {
                        model.stop(StopReason::Automatic, Some(tx));
                    }
                }
            }
        });
    }

    /// `jump()`.
    pub fn jump(&self) {
        if let Some(model) = self.model.get() {
            model.jump();
        }
    }

    /// As ligações do construtor com o `ObservableContextKeyService`.
    fn bind_context_keys(self: &Rc<Self>) {
        let keys = inline_completion_context_keys();
        let context_keys = ObservableContextKeyService::new(Rc::clone(&self.context_key_service));
        let base = &self.base;
        let model = &self.model;
        let editor_obs = &self.editor_obs;

        base.register(context_keys.bind_observable(&keys.cursor_in_indentation, &self.cursor_is_in_indentation));
        base.register(context_keys.bind(&keys.has_selection, {
            let editor_obs = Rc::clone(editor_obs);
            move |reader| Dynamic::Bool(!editor_obs.cursor_selection.read(reader).is_some_and(|selection| selection.is_empty()))
        }));
        base.register(context_keys.bind(&keys.cursor_at_inline_edit, {
            let model = Rc::clone(model);
            move |reader| {
                let state = model.read(reader).and_then(|model| model.inline_edit_state.read(reader));
                optional_bool(as_inline_edit(&state).map(|state| state.cursor_at_inline_edit.read(reader)))
            }
        }));
        base.register(context_keys.bind(&keys.tab_should_accept_inline_edit, {
            let model = Rc::clone(model);
            move |reader| Dynamic::Bool(model.read(reader).is_some_and(|model| model.tab_should_accept_inline_edit.read(reader)))
        }));
        base.register(context_keys.bind(&keys.tab_should_jump_to_inline_edit, {
            let model = Rc::clone(model);
            move |reader| Dynamic::Bool(model.read(reader).is_some_and(|model| model.tab_should_jump_to_inline_edit.read(reader)))
        }));
        base.register(context_keys.bind(&keys.inline_edit_visible, {
            let model = Rc::clone(model);
            move |reader| Dynamic::Bool(model.read(reader).and_then(|model| model.inline_edit_state.read(reader)).is_some())
        }));
        base.register(context_keys.bind(&keys.inline_suggestion_has_indentation, {
            let model = Rc::clone(model);
            move |reader| optional_bool(model.read(reader).map(|model| model.get_indentation_info(reader).0))
        }));
        base.register(context_keys.bind(&keys.inline_suggestion_has_indentation_less_than_tab_size, {
            let model = Rc::clone(model);
            move |reader| optional_bool(model.read(reader).map(|model| model.get_indentation_info(reader).1))
        }));
        base.register(context_keys.bind(&keys.suppress_suggestions, {
            let model = Rc::clone(model);
            move |reader| {
                let state = model.read(reader).and_then(|model| model.inline_completion_state.read(reader));
                optional_bool(
                    as_ghost_text(&state)
                        .and_then(|state| state.inline_suggestion.as_ref())
                        .and_then(|inline_suggestion| inline_suggestion.source().inline_suggestions.suppress_suggestions),
                )
            }
        }));
        base.register(context_keys.bind(&keys.inline_suggestion_alternative_action_visible, {
            let model = Rc::clone(model);
            move |reader| {
                let state = model.read(reader).and_then(|model| model.inline_edit_state.read(reader));
                match as_inline_edit(&state).and_then(|state| state.inline_suggestion.action()) {
                    None => Dynamic::Undefined,
                    Some(InlineSuggestionAction::Edit(action)) => Dynamic::Bool(action.alternative_action.is_some()),
                    Some(_) => Dynamic::Bool(false),
                }
            }
        }));
        base.register(context_keys.bind(&keys.inline_suggestion_visible, {
            let model = Rc::clone(model);
            move |reader| {
                let state = model.read(reader).and_then(|model| model.inline_completion_state.read(reader));
                Dynamic::Bool(as_ghost_text(&state).is_some_and(|state| state.inline_suggestion.is_some() && !state.primary_ghost_text.is_empty()))
            }
        }));

        let first_ghost_text_pos: IObservable<Option<Position>> = {
            let model = Rc::clone(model);
            derived(move |reader| -> Option<Position> {
                let state = model.read(reader).and_then(|model| model.inline_completion_state.read(reader));
                let primary_ghost_text = &as_ghost_text(&state)?.primary_ghost_text;
                if primary_ghost_text.is_empty() {
                    return None;
                }
                Some(Position::new(primary_ghost_text.line_number(), primary_ghost_text.parts().first()?.column))
            })
        };
        base.register(context_keys.bind(&keys.cursor_before_ghost_text, {
            let editor_obs = Rc::clone(editor_obs);
            move |reader| {
                let Some(first_part_pos) = first_ghost_text_pos.read(reader) else {
                    return Dynamic::Bool(false);
                };
                let Some(cursor_pos) = editor_obs.cursor_position.read(reader) else {
                    return Dynamic::Bool(false);
                };
                Dynamic::Bool(first_part_pos == cursor_pos)
            }
        }));
    }
}

impl IInlineCompletionsHoverController for InlineCompletionsController {
    fn model(&self) -> IObservable<Option<Rc<InlineCompletionsModel>>> {
        Rc::clone(&self.model)
    }

    /// `shouldShowHoverAt(range)`.
    fn should_show_hover_at(&self, range: Range) -> bool {
        let Some(ghost_text) = self.model.get().and_then(|model| model.primary_ghost_text.get()) else {
            return false;
        };
        ghost_text.parts().iter().any(|part| range.contains_position(&Position::new(ghost_text.line_number(), part.column)))
    }

    /// `shouldShowHoverAtViewZone(viewZoneId)`.
    fn should_show_hover_at_view_zone(&self, view_zone_id: &str) -> bool {
        self.view.get().should_show_hover_at_view_zone(view_zone_id)
    }
}

impl IInlineCompletionsController for InlineCompletionsController {
    fn model(&self) -> IObservable<Option<Rc<dyn IInlineCompletionsModel>>> {
        Rc::clone(&self.suggest_model)
    }
}

impl IDisposable for InlineCompletionsController {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IEditorContribution for InlineCompletionsController {}

/// O `setInlineCompletionsControllerGetter((editor) => InlineCompletionsController.get(editor))` do topo do arquivo.
pub fn install_inline_completions_controller_getter() {
    set_inline_completions_controller_getter(|editor| InlineCompletionsController::get(editor).map(|controller| controller as Rc<dyn Any>));
}

/// O `registerEditorContribution(InlineCompletionsController.ID, ..., EditorContributionInstantiation.Eventually)` de
/// `inlineCompletions.contribution.ts`, mais o getter do topo de `inlineCompletionsController.ts`. `services` entrega os
/// [`InlineCompletionsControllerServices`] de cada editor (o `createInstance` do original). É idempotente: o registro
/// acontece uma vez por thread.
pub fn register_inline_completions_controller(services: Rc<dyn Fn(&Rc<CodeEditorWidget>) -> InlineCompletionsControllerServices>) {
    if REGISTERED.replace(true) {
        return;
    }
    install_inline_completions_controller_getter();
    register_editor_contribution(
        InlineCompletionsController::ID,
        Rc::new(move |editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> { InlineCompletionsController::new(editor, services(editor)) }),
        EditorContributionInstantiation::Eventually,
    );
}
