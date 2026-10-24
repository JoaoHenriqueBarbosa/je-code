//! Porte de `src/vs/editor/contrib/find/browser/findController.ts`: o controlador da busca (`CommonFindController` e a
//! parte lógica de `FindController`), `getSelectionSearchString`, as opções de início (`IFindStartOptions`) e as ações
//! e comandos da busca com as chamadas de registro do fim do arquivo.
//!
//! Representação:
//! - `CommonFindController extends Disposable implements IEditorContribution` é a struct [`CommonFindController`] (um
//!   [`Disposable`] mais o estado num `RefCell`), criada por [`CommonFindController::new`] e entregue como `Rc`. O editor
//!   é um [`Weak`] do widget. `FindController extends CommonFindController` (que só acrescenta o widget DOM
//!   `FindWidget`, o `FindOptionsWidget` e os históricos, que são especificação) é [`FindController`], que devolve o
//!   mesmo tipo com o `_start` do `FindController` instalado ([`find_controller_start`]).
//! - O `_start` é virtual no original (`TestFindController` o sobrescreve e chama `super._start`): [`StartOverride`]
//!   é o sobrescrito, instalado por [`CommonFindController::set_start_override`]; o `super._start` é
//!   [`CommonFindController::start_base`], e [`CommonFindController::start`] (o `start` público, que chama `_start`)
//!   despacha para um ou para o outro. O `async` do `_start` é a [`Promise`]: o que o original roda antes do primeiro
//!   `await` roda de forma síncrona dentro da chamada, e o resto numa microtarefa.
//! - `@IContextKeyService`, `@IStorageService`, `@IClipboardService` e `@INotificationService` do construtor chegam em
//!   [`CommonFindControllerServices`] (o `IHoverService` só serve ao widget). O `IClipboardService` e o `IQuickInputService`
//!   ainda não têm porte: [`IClipboardService`] (só `readFindText`/`writeFindText`) e [`IQuickInputService`] (só a caixa de
//!   entrada) são os recortes que a busca usa, com os ids [`I_CLIPBOARD_SERVICE`] e [`I_QUICK_INPUT_SERVICE`] (o mesmo
//!   expediente de `ICodeEditorLookup`). O `alert` de `aria.js` é o `aria_alert` dos serviços.
//! - `StartFindAction`, `NextMatchFindAction`, `PreviousMatchFindAction` e `StartFindReplaceAction`
//!   (`registerMultiEditorAction(new MultiEditorAction(...))` mais `addImplementation`) são as instâncias por thread de
//!   [`start_find_action`] e companhia, criadas e registradas no primeiro acesso (o que o carregamento do módulo faz).
//!   As demais ações são structs que implementam [`EditorAction`]. A classe abstrata `SelectionMatchFindAction` é
//!   [`SelectionMatchFindAction`], que guarda o `_run` como função, e as duas subclasses são os construtores
//!   [`SelectionMatchFindAction::next`] e [`SelectionMatchFindAction::previous`]; `MatchFindAction` idem.
//! - `EditorCommand.bindToContribution<CommonFindController>(CommonFindController.get)` é [`bind_to_contribution`].
//! - `searchScope` chega e sai como `Vec<Range>` (uma `Selection` vale pelo seu `Range`).
//! - `getSelectionSearchString(editor, seed = 'single', nonEmpty = false)` recebe os dois parâmetros opcionais como
//!   `Option`.
//!
//! Divergências:
//! - O `FindWidget` e o `FindOptionsWidget` são a especificação (UI nativa, sem DOM): o `_start` de `FindController` não
//!   foca o widget e `highlightFindOptions` não tem o que realçar. `saveViewState` e `restoreViewState` guardam o estado
//!   do widget e por isso não existem.
//! - O `registerEditorContribution(CommonFindController.ID, FindController, ...)` precisa dos serviços que o widget do
//!   editor ainda não carrega: [`register_find_controller`] os recebe.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::Delayer;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::Event;
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::strings::escape_reg_exp_characters;
use je_base::symbols::Delay;
use je_platform::action::localize2;
use je_platform::actions::menu_ids;
use je_platform::commands::{CommandArgument, CommandDescription, CommandMetadata};
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression, IContextKey, IContextKeyService};
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings, PlatformKeybindings};
use je_platform::notification::{I_NOTIFICATION_SERVICE, INotificationService, Notification, NotificationMessage, Severity};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};

use super::find_model::{
    FindModelBoundToEditorModel, context_find_input_focused, context_find_widget_visible, context_replace_input_focused, find_ids,
    toggle_case_sensitive_keybinding, toggle_preserve_case_keybinding, toggle_regex_keybinding, toggle_search_scope_keybinding,
    toggle_whole_word_keybinding,
};
use super::find_state::{FindReplaceState, FindReplaceStateChangedEvent, INewFindReplaceState};
use crate::code_editor_widget::{CodeEditorWidget, EditorContributionInstantiation, ICodeEditor, IEditorContribution};
use crate::config::editor_options::find::{AutoFindInSelection, FIND, SeedSearchStringFromSelection};
use crate::config::editor_options::registry::READ_ONLY;
use crate::core_commands::key;
use crate::cursor_common::CursorSimpleModel;
use crate::editor_extensions::{
    ActionLabel, ActionOptions, CommandImplementationResult, CommandKeybindingsOptions, CommandMenuOptions, CommandOptions,
    ContributionCommandOptions, EditorAction, EditorActionBase, EditorControllerCommand, MultiEditorAction, ServicesAccessor,
    bind_to_contribution, register_editor_action, register_editor_command, register_editor_contribution,
    register_multi_editor_action,
};
use crate::model::{DecorationColor, ModelDecorationOptionsInit, ModelDecorationOverviewRulerInit, ModelDeltaDecoration, OverviewRulerLane};
use crate::range::Range;
use crate::text_model::ModelDecorationOptions;

/// `SEARCH_STRING_MAX_LENGTH`.
const SEARCH_STRING_MAX_LENGTH: usize = 524288;

/// O id da cor `editorOverviewRuler.rangeHighlightForeground` (`overviewRulerRangeHighlight`).
const OVERVIEW_RULER_RANGE_HIGHLIGHT: &str = "editorOverviewRuler.rangeHighlightForeground";

/// `NLS_NO_RESULTS` (`findWidget.ts`).
fn nls_no_results() -> String {
    localize("label.noResults", "No results", &[])
}

// #region Serviços (recortes)

pub use crate::services::clipboard_service::{I_CLIPBOARD_SERVICE, IClipboardService};

/// A fatia de `IInputBox` (`quickInput.ts`) que o `Go to Match...` usa.
pub trait IInputBox: IDisposable {
    /// `inputBox.placeholder = ...`.
    fn set_placeholder(&self, placeholder: &str);
    /// `inputBox.validationMessage = ...` (`None` é `undefined`).
    fn set_validation_message(&self, message: Option<&str>);
    /// `inputBox.value`.
    fn value(&self) -> String;
    /// `onDidChangeValue`.
    fn on_did_change_value(&self) -> Event<String>;
    /// `onDidAccept`.
    fn on_did_accept(&self) -> Event<()>;
    /// `onDidHide`.
    fn on_did_hide(&self) -> Event<()>;
    /// `show()`.
    fn show(&self);
    /// `hide()`.
    fn hide(&self);
}

/// A fatia de `IQuickInputService` que a busca usa.
pub trait IQuickInputService {
    /// `createInputBox()`.
    fn create_input_box(&self) -> Rc<dyn IInputBox>;
}

je_instantiation::service_id!(pub I_QUICK_INPUT_SERVICE: dyn IQuickInputService = "quickInputService");

/// Os serviços que o construtor de `CommonFindController` recebe por injeção.
#[derive(Clone)]
pub struct CommonFindControllerServices {
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub storage_service: Rc<dyn StorageService>,
    pub clipboard_service: Rc<dyn IClipboardService>,
    pub notification_service: Rc<dyn INotificationService>,
    /// `alert(message)` de `aria.js`.
    pub aria_alert: Rc<dyn Fn(&str)>,
}

impl CommonFindControllerServices {
    /// Os serviços lidos do acessor (o que a injeção de dependência faz no construtor do original).
    pub fn from_accessor(accessor: &ServicesAccessor, aria_alert: Rc<dyn Fn(&str)>) -> Self {
        Self {
            context_key_service: accessor.get(je_platform::contextkey::I_CONTEXT_KEY_SERVICE),
            storage_service: accessor.get(je_platform::storage::I_STORAGE_SERVICE),
            clipboard_service: accessor.get(I_CLIPBOARD_SERVICE),
            notification_service: accessor.get(I_NOTIFICATION_SERVICE),
            aria_alert,
        }
    }
}

// #endregion

/// O modo de `seedSearchStringFromSelection` de `getSelectionSearchString` (`'single' | 'multiple'`) e de
/// `IFindStartOptions` (`'none' | 'single' | 'multiple'`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedSearchStringMode {
    None,
    Single,
    Multiple,
}

/// `getSelectionSearchString(editor, seedSearchStringFromSelection = 'single', seedSearchStringFromNonEmptySelection = false)`.
pub fn get_selection_search_string(
    editor: &CodeEditorWidget,
    seed_search_string_from_selection: Option<SeedSearchStringMode>,
    seed_search_string_from_non_empty_selection: Option<bool>,
) -> Option<JsString> {
    let seed_search_string_from_selection = seed_search_string_from_selection.unwrap_or(SeedSearchStringMode::Single);
    let seed_search_string_from_non_empty_selection = seed_search_string_from_non_empty_selection.unwrap_or(false);
    let model = editor.get_model()?;

    let selection = editor.get_selection()?;
    // if selection spans multiple lines, default search string to empty

    if (seed_search_string_from_selection == SeedSearchStringMode::Single && selection.start_line_number == selection.end_line_number)
        || seed_search_string_from_selection == SeedSearchStringMode::Multiple
    {
        if selection.is_empty() {
            let word_at_position = editor.get_configured_word_at_position(selection.get_start_position());
            if let Some(word_at_position) = word_at_position
                && !seed_search_string_from_non_empty_selection
            {
                return Some(word_at_position.word);
            }
        } else if model.get_value_length_in_range(&selection) < SEARCH_STRING_MAX_LENGTH {
            return Some(model.get_value_in_range(&selection));
        }
    }

    None
}

/// `FindStartFocusAction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindStartFocusAction {
    NoFocusChange,
    FocusFindInput,
    FocusReplaceInput,
}

/// `IFindStartOptions`.
#[derive(Clone, Copy, Debug)]
pub struct IFindStartOptions {
    pub force_reveal_replace: bool,
    pub seed_search_string_from_selection: SeedSearchStringMode,
    pub seed_search_string_from_non_empty_selection: bool,
    pub seed_search_string_from_global_clipboard: bool,
    pub should_focus: FindStartFocusAction,
    pub should_animate: bool,
    pub update_search_scope: bool,
    pub r#loop: bool,
}

/// `IFindStartArguments`.
#[derive(Clone, Debug, Default)]
pub struct IFindStartArguments {
    pub search_string: Option<JsString>,
    pub replace_string: Option<JsString>,
    pub is_regex: Option<bool>,
    pub match_whole_word: Option<bool>,
    pub is_case_sensitive: Option<bool>,
    pub preserve_case: Option<bool>,
    pub find_in_selection: Option<bool>,
}

impl IFindStartArguments {
    /// Lê o objeto de argumentos da ação (`undefined` quando o argumento não é um objeto).
    pub fn from_dynamic(args: &Dynamic) -> Option<Self> {
        if !args.is_truthy() {
            return None;
        }
        let text = |name: &str| match args.get_str(name) {
            Dynamic::String(value) => Some(value),
            _ => None,
        };
        let flag = |name: &str| match args.get_str(name) {
            Dynamic::Bool(value) => Some(value),
            _ => None,
        };
        Some(Self {
            search_string: text("searchString"),
            replace_string: text("replaceString"),
            is_regex: flag("isRegex"),
            match_whole_word: flag("matchWholeWord"),
            is_case_sensitive: flag("isCaseSensitive"),
            preserve_case: flag("preserveCase"),
            find_in_selection: flag("findInSelection"),
        })
    }
}

/// O `_start` sobrescrito de uma subclasse: recebe o controlador, as opções e o estado novo, e chama
/// [`CommonFindController::start_base`] (o `super._start`).
pub type StartOverride = Rc<dyn Fn(&Rc<CommonFindController>, IFindStartOptions, Option<INewFindReplaceState>) -> Promise<()>>;

/// `CommonFindController`.
pub struct CommonFindController {
    base: Rc<Disposable>,
    editor: Weak<CodeEditorWidget>,
    find_widget_visible: Rc<dyn IContextKey>,
    state: Rc<FindReplaceState>,
    update_history_delayer: RefCell<Rc<Delayer<()>>>,
    model: RefCell<Option<Rc<FindModelBoundToEditorModel>>>,
    services: CommonFindControllerServices,
    start_override: RefCell<Option<StartOverride>>,
}

impl CommonFindController {
    pub const ID: &'static str = "editor.contrib.findController";

    /// `CommonFindController.get(editor)`.
    pub fn get(editor: &Rc<CodeEditorWidget>) -> Option<Rc<CommonFindController>> {
        editor.get_contribution_as::<CommonFindController>(Self::ID)
    }

    pub fn new(editor: &Rc<CodeEditorWidget>, services: CommonFindControllerServices) -> Rc<Self> {
        let base = Disposable::new();
        let find_widget_visible = context_find_widget_visible().bind_to(&*services.context_key_service);

        let update_history_delayer = Rc::new(Delayer::new(Delay::Millis(500)));
        base.register(update_history_delayer.clone());
        let state = FindReplaceState::new();
        base.register(state.clone());

        let this = Rc::new(Self {
            base,
            editor: Rc::downgrade(editor),
            find_widget_visible,
            state,
            update_history_delayer: RefCell::new(update_history_delayer),
            model: RefCell::new(None),
            services,
            start_override: RefCell::new(None),
        });
        this.load_query_state();
        this.base.register(super::listen_weak(this.state.on_find_replace_state_change(), &this, |this, e| this.on_state_changed(e)));

        this.base.register(super::listen_weak(editor.on_did_change_model(), &this, |this, _| this.on_did_change_model()));
        this
    }

    /// `get editor()`.
    pub fn editor(&self) -> Rc<CodeEditorWidget> {
        self.editor.upgrade().expect("o editor vive enquanto o controlador da busca existe")
    }

    /// `this._updateHistoryDelayer = new Delayer<void>(...)`: o `_updateHistoryDelayer` é protegido e as subclasses o trocam.
    pub fn set_update_history_delayer(&self, delayer: Rc<Delayer<()>>) {
        *self.update_history_delayer.borrow_mut() = delayer;
    }

    /// Instala o `_start` de uma subclasse (ver o topo do módulo).
    pub fn set_start_override(&self, start_override: StartOverride) {
        *self.start_override.borrow_mut() = Some(start_override);
    }

    /// O `this._editor.onDidChangeModel` do construtor.
    fn on_did_change_model(self: &Rc<Self>) {
        let editor = self.editor();
        let should_restart_find = editor.get_model().is_some() && self.state.is_revealed();

        self.dispose_model();

        self.state.change(
            INewFindReplaceState {
                search_scope: Some(None),
                match_case: Some(self.stored_flag("editor.matchCase", false)),
                whole_word: Some(self.stored_flag("editor.wholeWord", false)),
                is_regex: Some(self.stored_flag("editor.isRegex", false)),
                preserve_case: Some(self.stored_flag("editor.preserveCase", false)),
                ..INewFindReplaceState::default()
            },
            false,
            None,
        );

        if should_restart_find {
            let _ = self.start(
                IFindStartOptions {
                    force_reveal_replace: false,
                    seed_search_string_from_selection: SeedSearchStringMode::None,
                    seed_search_string_from_non_empty_selection: false,
                    seed_search_string_from_global_clipboard: false,
                    should_focus: FindStartFocusAction::NoFocusChange,
                    should_animate: false,
                    update_search_scope: false,
                    r#loop: editor.get_option(FIND).r#loop,
                },
                None,
            );
        }
    }

    fn dispose_model(&self) {
        let model = self.model.borrow_mut().take();
        if let Some(model) = model {
            model.dispose();
        }
    }

    fn on_state_changed(&self, e: &FindReplaceStateChangedEvent) {
        self.save_query_state(e);

        if e.is_revealed {
            if self.state.is_revealed() {
                self.find_widget_visible.set(Dynamic::Bool(true));
            } else {
                self.find_widget_visible.reset();
                self.dispose_model();
            }
        }
        if e.search_string {
            self.set_global_buffer_term(&self.state.search_string());
        }
    }

    /// `this._storageService.getBoolean(key, StorageScope.WORKSPACE, fallback)`.
    fn stored_flag(&self, key: &str, fallback: bool) -> bool {
        self.services.storage_service.get_boolean(key, StorageScope::Workspace, Some(fallback)).unwrap_or(fallback)
    }

    fn save_query_state(&self, e: &FindReplaceStateChangedEvent) {
        let storage = &self.services.storage_service;
        if e.is_regex {
            storage.store("editor.isRegex", self.state.actual_is_regex().into(), StorageScope::Workspace, StorageTarget::Machine);
        }
        if e.whole_word {
            storage.store("editor.wholeWord", self.state.actual_whole_word().into(), StorageScope::Workspace, StorageTarget::Machine);
        }
        if e.match_case {
            storage.store("editor.matchCase", self.state.actual_match_case().into(), StorageScope::Workspace, StorageTarget::Machine);
        }
        if e.preserve_case {
            storage.store("editor.preserveCase", self.state.actual_preserve_case().into(), StorageScope::Workspace, StorageTarget::Machine);
        }
    }

    fn load_query_state(&self) {
        self.state.change(
            INewFindReplaceState {
                match_case: Some(self.stored_flag("editor.matchCase", self.state.match_case())),
                whole_word: Some(self.stored_flag("editor.wholeWord", self.state.whole_word())),
                is_regex: Some(self.stored_flag("editor.isRegex", self.state.is_regex())),
                preserve_case: Some(self.stored_flag("editor.preserveCase", self.state.preserve_case())),
                ..INewFindReplaceState::default()
            },
            false,
            None,
        );
    }

    pub fn is_find_input_focused(&self) -> bool {
        context_find_input_focused().get_value(&*self.services.context_key_service).is_truthy()
    }

    /// Returns whether the Replace input was the last focused input in the find widget. Returns false by default;
    /// overridden in FindController.
    pub fn was_replace_input_last_focused(&self) -> bool {
        false
    }

    /// Focuses the last focused element in the find widget. Implemented by FindController; base implementation does
    /// nothing.
    pub fn focus_last_element(&self) {
        // Base implementation - overridden in FindController
    }

    pub fn get_state(&self) -> Rc<FindReplaceState> {
        self.state.clone()
    }

    pub fn close_find_widget(&self) {
        self.state.change(
            INewFindReplaceState { is_revealed: Some(false), search_scope: Some(None), ..INewFindReplaceState::default() },
            false,
            None,
        );
        self.editor().focus();
    }

    pub fn toggle_case_sensitive(&self) {
        self.state.change(INewFindReplaceState { match_case: Some(!self.state.match_case()), ..INewFindReplaceState::default() }, false, None);
        if !self.state.is_revealed() {
            self.highlight_find_options(false);
        }
    }

    pub fn toggle_whole_words(&self) {
        self.state.change(INewFindReplaceState { whole_word: Some(!self.state.whole_word()), ..INewFindReplaceState::default() }, false, None);
        if !self.state.is_revealed() {
            self.highlight_find_options(false);
        }
    }

    pub fn toggle_regex(&self) {
        self.state.change(INewFindReplaceState { is_regex: Some(!self.state.is_regex()), ..INewFindReplaceState::default() }, false, None);
        if !self.state.is_revealed() {
            self.highlight_find_options(false);
        }
    }

    pub fn toggle_preserve_case(&self) {
        self.state.change(
            INewFindReplaceState { preserve_case: Some(!self.state.preserve_case()), ..INewFindReplaceState::default() },
            false,
            None,
        );
        if !self.state.is_revealed() {
            self.highlight_find_options(false);
        }
    }

    pub fn toggle_search_scope(&self) {
        if self.state.search_scope().is_some() {
            self.state.change(INewFindReplaceState { search_scope: Some(None), ..INewFindReplaceState::default() }, true, None);
        } else {
            let editor = self.editor();
            if let (Some(model), Some(selections)) = (editor.get_model(), editor.get_selections()) {
                let selections: Vec<Range> = selections
                    .into_iter()
                    .filter_map(|selection| {
                        let mut selection = selection;
                        if selection.end_column == 1 && selection.end_line_number > selection.start_line_number {
                            selection =
                                selection.set_end_position(selection.end_line_number - 1, model.get_line_max_column(selection.end_line_number - 1));
                        }
                        if !selection.is_empty() { Some(*selection) } else { None }
                    })
                    .collect();

                if !selections.is_empty() {
                    self.state.change(INewFindReplaceState { search_scope: Some(Some(selections)), ..INewFindReplaceState::default() }, true, None);
                }
            }
        }
    }

    pub fn set_search_string(&self, search_string: JsString) {
        let search_string = if self.state.is_regex() { escape_reg_exp_characters(search_string.units()) } else { search_string };
        self.state.change(INewFindReplaceState { search_string: Some(search_string), ..INewFindReplaceState::default() }, false, None);
    }

    /// `highlightFindOptions(ignoreWhenVisible = false)`: sobrescrito pelo `FindController` para realçar as opções no
    /// widget, que é especificação (ver o topo do módulo).
    pub fn highlight_find_options(&self, _ignore_when_visible: bool) {
        // overwritten in subclass
    }

    /// O `_start` da classe-base: o `super._start(opts, newState)` das subclasses.
    pub fn start_base(self: &Rc<Self>, opts: IFindStartOptions, new_state: Option<INewFindReplaceState>) -> Promise<()> {
        self.dispose_model();

        let editor = self.editor();
        if !editor.has_model() {
            // cannot do anything with an editor that doesn't have a model...
            return Promise::resolved(());
        }

        let mut state_changes = INewFindReplaceState { is_revealed: Some(true), ..new_state.unwrap_or_default() };

        match opts.seed_search_string_from_selection {
            SeedSearchStringMode::Single => {
                let selection_search_string = get_selection_search_string(
                    &editor,
                    Some(opts.seed_search_string_from_selection),
                    Some(opts.seed_search_string_from_non_empty_selection),
                )
                .filter(|search_string| !search_string.is_empty());
                if let Some(selection_search_string) = selection_search_string {
                    if self.state.is_regex() {
                        state_changes.search_string = Some(escape_reg_exp_characters(selection_search_string.units()));
                    } else {
                        state_changes.search_string = Some(selection_search_string);
                    }
                }
            }
            SeedSearchStringMode::Multiple if !opts.update_search_scope => {
                let selection_search_string = get_selection_search_string(&editor, Some(opts.seed_search_string_from_selection), None)
                    .filter(|search_string| !search_string.is_empty());
                if let Some(selection_search_string) = selection_search_string {
                    state_changes.search_string = Some(selection_search_string);
                }
            }
            _ => {}
        }

        if state_changes.search_string.as_ref().is_none_or(|search_string| search_string.is_empty()) && opts.seed_search_string_from_global_clipboard {
            let this = self.clone();
            return self.get_global_buffer_term().then_promise(move |selection_search_string| {
                if !this.editor().has_model() {
                    // the editor has lost its model in the meantime
                    return Promise::resolved(());
                }

                if !selection_search_string.is_empty() {
                    state_changes.search_string = Some(selection_search_string);
                }

                this.finish_start(opts, state_changes);
                Promise::resolved(())
            });
        }

        self.finish_start(opts, state_changes);
        Promise::resolved(())
    }

    /// O trecho de `_start` que vem depois do `await` da área de transferência.
    fn finish_start(&self, opts: IFindStartOptions, state_changes: INewFindReplaceState) {
        let mut state_changes = state_changes;
        let editor = self.editor();

        // Overwrite isReplaceRevealed
        if opts.force_reveal_replace || state_changes.is_replace_revealed == Some(true) {
            state_changes.is_replace_revealed = Some(true);
        } else if !self.find_widget_visible.get().is_truthy() {
            state_changes.is_replace_revealed = Some(false);
        }

        if opts.update_search_scope
            && let Some(current_selections) = editor.get_selections()
            && current_selections.iter().any(|selection| !selection.is_empty())
        {
            state_changes.search_scope = Some(Some(current_selections.iter().map(|selection| **selection).collect()));
        }

        state_changes.r#loop = Some(opts.r#loop);

        self.state.change(state_changes, false, None);

        if self.model.borrow().is_none() {
            let model = FindModelBoundToEditorModel::new(&editor, self.state.clone());
            *self.model.borrow_mut() = Some(model);
        }
    }

    /// `start(opts, newState?)`: chama o `_start`, que a subclasse pode ter sobrescrito.
    pub fn start(self: &Rc<Self>, opts: IFindStartOptions, new_state: Option<INewFindReplaceState>) -> Promise<()> {
        let start_override = self.start_override.borrow().clone();
        match start_override {
            Some(start_override) => start_override(self, opts, new_state),
            None => self.start_base(opts, new_state),
        }
    }

    /// O modelo de busca atual (a ação só roda quando ele existe).
    fn current_model(&self) -> Option<Rc<FindModelBoundToEditorModel>> {
        self.model.borrow().clone()
    }

    pub fn move_to_next_match(&self) -> bool {
        match self.current_model() {
            Some(model) => {
                model.move_to_next_match();
                true
            }
            None => false,
        }
    }

    pub fn move_to_prev_match(&self) -> bool {
        match self.current_model() {
            Some(model) => {
                model.move_to_prev_match();
                true
            }
            None => false,
        }
    }

    pub fn go_to_match(&self, index: usize) -> bool {
        match self.current_model() {
            Some(model) => {
                model.move_to_match(index);
                true
            }
            None => false,
        }
    }

    pub fn replace(&self) -> bool {
        match self.current_model() {
            Some(model) => {
                model.replace();
                true
            }
            None => false,
        }
    }

    pub fn replace_all(&self) -> bool {
        let Some(model) = self.current_model() else {
            return false;
        };
        if self.editor().get_model().is_some_and(|text_model| text_model.is_too_large_for_heap_operation()) {
            self.services.notification_service.warn(vec![NotificationMessage::Text(localize(
                "too.large.for.replaceall",
                "The file is too large to perform a replace all operation.",
                &[],
            ))]);
            return false;
        }
        model.replace_all();
        true
    }

    pub fn select_all_matches(&self) -> bool {
        match self.current_model() {
            Some(model) => {
                model.select_all_matches();
                self.editor().focus();
                true
            }
            None => false,
        }
    }

    pub fn get_global_buffer_term(&self) -> Promise<JsString> {
        let editor = self.editor();
        if editor.get_option(FIND).global_find_clipboard
            && editor.get_model().is_some_and(|model| !model.is_too_large_for_syncing())
        {
            return self.services.clipboard_service.read_find_text();
        }
        Promise::resolved(JsString::new())
    }

    pub fn set_global_buffer_term(&self, text: &JsString) {
        let editor = self.editor();
        if editor.get_option(FIND).global_find_clipboard
            && editor.get_model().is_some_and(|model| !model.is_too_large_for_syncing())
        {
            // intentionally not awaited
            let _ = self.services.clipboard_service.write_find_text(text);
        }
    }

    /// O `start` de [`match_find_action`] e de `MatchFindAction`, que reabre a busca para navegar.
    fn start_for_match_navigation(self: &Rc<Self>) -> Promise<()> {
        let find = *self.editor().get_option(FIND);
        self.start(
            IFindStartOptions {
                force_reveal_replace: false,
                seed_search_string_from_selection: if self.state.search_string().is_empty()
                    && find.seed_search_string_from_selection != SeedSearchStringFromSelection::Never
                {
                    SeedSearchStringMode::Single
                } else {
                    SeedSearchStringMode::None
                },
                seed_search_string_from_non_empty_selection: find.seed_search_string_from_selection == SeedSearchStringFromSelection::Selection,
                seed_search_string_from_global_clipboard: true,
                should_focus: FindStartFocusAction::NoFocusChange,
                should_animate: true,
                update_search_scope: false,
                r#loop: find.r#loop,
            },
            None,
        )
    }
}

impl IDisposable for CommonFindController {
    fn dispose(&self) {
        self.dispose_model();
        self.base.dispose();
    }
}

impl IEditorContribution for CommonFindController {}

/// `FindController extends CommonFindController`: o que o `_start` dele faz sem o widget (ver o topo do módulo).
pub struct FindController;

/// O `_start` de `FindController`: liga a busca na seleção conforme `autoFindInSelection`.
pub fn find_controller_start(
    controller: &Rc<CommonFindController>,
    opts: IFindStartOptions,
    new_state: Option<INewFindReplaceState>,
) -> Promise<()> {
    let mut opts = opts;
    let editor = controller.editor();
    let selection = editor.get_selection();

    let update_search_scope = match editor.get_option(FIND).auto_find_in_selection {
        AutoFindInSelection::Always => true,
        AutoFindInSelection::Never => false,
        AutoFindInSelection::Multiline => selection.is_some_and(|selection| selection.start_line_number != selection.end_line_number),
    };

    opts.update_search_scope = opts.update_search_scope || update_search_scope;

    controller.start_base(opts, new_state)
}

impl FindController {
    /// `new FindController(editor, ...)`.
    pub fn new(editor: &Rc<CodeEditorWidget>, services: CommonFindControllerServices) -> Rc<CommonFindController> {
        let controller = CommonFindController::new(editor, services);
        controller.set_start_override(Rc::new(find_controller_start));
        controller
    }
}

/// `new IContextKeyService`-agnóstico `EditorContextKeys.focus || editorIsOpen`: a precondição de iniciar a busca.
fn find_precondition() -> Option<ContextKeyExpression> {
    ContextKeyExpr::or([ContextKeyExpr::has("editorFocus"), ContextKeyExpr::has("editorIsOpen")])
}

/// `{ primary: KeyMod.CtrlCmd | KeyCode.X }`.
fn ctrl_cmd_keybinding(key_code: KeyCode) -> Keybindings {
    Keybindings { primary: Some(key(KeyMod::CTRL_CMD, key_code)), ..Keybindings::default() }
}

/// O `kbOpts` de peso `EditorContrib` e sem `kbExpr`, com um atalho principal.
fn contrib_keybinding(kb_expr: Option<ContextKeyExpression>, keybindings: Keybindings) -> CommandKeybindingsOptions {
    CommandKeybindingsOptions::new(KeybindingWeight::EDITOR_CONTRIB, kb_expr, keybindings)
}

/// `EditorContextKeys.focus`.
fn editor_focus() -> ContextKeyExpression {
    ContextKeyExpr::has("editorFocus")
}

thread_local! {
    static START_FIND_ACTION: Rc<MultiEditorAction> = create_start_find_action();
    static NEXT_MATCH_FIND_ACTION: Rc<MultiEditorAction> = create_next_match_find_action();
    static PREVIOUS_MATCH_FIND_ACTION: Rc<MultiEditorAction> = create_previous_match_find_action();
    static START_FIND_REPLACE_ACTION: Rc<MultiEditorAction> = create_start_find_replace_action();
    /// O carregamento do módulo acontece uma vez; o registro é por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// `StartFindAction`.
pub fn start_find_action() -> Rc<MultiEditorAction> {
    START_FIND_ACTION.with(Rc::clone)
}

/// `NextMatchFindAction`.
pub fn next_match_find_action() -> Rc<MultiEditorAction> {
    NEXT_MATCH_FIND_ACTION.with(Rc::clone)
}

/// `PreviousMatchFindAction`.
pub fn previous_match_find_action() -> Rc<MultiEditorAction> {
    PREVIOUS_MATCH_FIND_ACTION.with(Rc::clone)
}

/// `StartFindReplaceAction`.
pub fn start_find_replace_action() -> Rc<MultiEditorAction> {
    START_FIND_REPLACE_ACTION.with(Rc::clone)
}

fn create_start_find_action() -> Rc<MultiEditorAction> {
    let action = register_multi_editor_action(Rc::new(MultiEditorAction::new(ActionOptions {
        command: CommandOptions {
            precondition: find_precondition(),
            kb_opts: vec![contrib_keybinding(None, ctrl_cmd_keybinding(KeyCode::KeyF))],
            menu_opts: vec![CommandMenuOptions::new(menu_ids::menubar_edit_menu(), "3_find", 1.0, localize("miFind", "&&Find", &[]))],
            ..CommandOptions::new(find_ids::START_FIND_ACTION)
        },
        context_menu_opts: Vec::new(),
        label: ActionLabel::Localized { label: localize2("startFindAction", "Find"), alias: None },
    })));

    action.add_implementation(
        0,
        Rc::new(|_accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic| {
            let Some(controller) = CommonFindController::get(editor) else {
                return CommandImplementationResult::Bool(false);
            };
            let find = *editor.get_option(FIND);
            CommandImplementationResult::Promise(controller.start(
                IFindStartOptions {
                    force_reveal_replace: false,
                    seed_search_string_from_selection: if find.seed_search_string_from_selection != SeedSearchStringFromSelection::Never {
                        SeedSearchStringMode::Single
                    } else {
                        SeedSearchStringMode::None
                    },
                    seed_search_string_from_non_empty_selection: find.seed_search_string_from_selection == SeedSearchStringFromSelection::Selection,
                    seed_search_string_from_global_clipboard: find.global_find_clipboard,
                    should_focus: FindStartFocusAction::FocusFindInput,
                    should_animate: true,
                    update_search_scope: false,
                    r#loop: find.r#loop,
                },
                None,
            ))
        }),
    );
    action
}

/// `findArgDescription`.
fn find_arg_description() -> CommandMetadata {
    let string_property = || Dynamic::object([("type", Dynamic::string("string"))]);
    let boolean_property = || Dynamic::object([("type", Dynamic::string("boolean"))]);
    let schema = Dynamic::object([(
        "properties",
        Dynamic::object([
            ("searchString", string_property()),
            ("replaceString", string_property()),
            ("isRegex", boolean_property()),
            ("matchWholeWord", boolean_property()),
            ("isCaseSensitive", boolean_property()),
            ("preserveCase", boolean_property()),
            ("findInSelection", boolean_property()),
        ]),
    )]);
    CommandMetadata {
        description: CommandDescription::Plain("Open a new In-Editor Find Widget.".to_owned()),
        args: Some(vec![CommandArgument { schema: Some(schema), ..CommandArgument::new("Open a new In-Editor Find Widget args") }]),
        returns: None,
    }
}

/// `StartFindWithArgsAction`.
pub struct StartFindWithArgsAction {
    action: EditorActionBase,
}

impl StartFindWithArgsAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(ActionOptions {
                command: CommandOptions {
                    kb_opts: vec![contrib_keybinding(None, Keybindings { primary: Some(0), ..Keybindings::default() })],
                    metadata: Some(find_arg_description()),
                    ..CommandOptions::new(find_ids::START_FIND_WITH_ARGS)
                },
                context_menu_opts: Vec::new(),
                label: ActionLabel::Localized { label: localize2("startFindWithArgsAction", "Find with Arguments"), alias: None },
            }),
        }
    }
}

impl Default for StartFindWithArgsAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for StartFindWithArgsAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()> {
        let Some(controller) = CommonFindController::get(editor) else {
            return Promise::resolved(());
        };
        let args = IFindStartArguments::from_dynamic(args);
        let new_state = match &args {
            Some(args) => INewFindReplaceState {
                search_string: args.search_string.clone(),
                replace_string: args.replace_string.clone(),
                is_replace_revealed: Some(args.replace_string.is_some()),
                is_regex: args.is_regex,
                whole_word: args.match_whole_word,
                match_case: args.is_case_sensitive,
                preserve_case: args.preserve_case,
                ..INewFindReplaceState::default()
            },
            None => INewFindReplaceState::default(),
        };

        let find = *editor.get_option(FIND);
        let started = controller.start(
            IFindStartOptions {
                force_reveal_replace: false,
                seed_search_string_from_selection: if controller.get_state().search_string().is_empty()
                    && find.seed_search_string_from_selection != SeedSearchStringFromSelection::Never
                {
                    SeedSearchStringMode::Single
                } else {
                    SeedSearchStringMode::None
                },
                seed_search_string_from_non_empty_selection: find.seed_search_string_from_selection == SeedSearchStringFromSelection::Selection,
                seed_search_string_from_global_clipboard: true,
                should_focus: FindStartFocusAction::FocusFindInput,
                should_animate: true,
                update_search_scope: args.as_ref().and_then(|args| args.find_in_selection).unwrap_or(false),
                r#loop: find.r#loop,
            },
            Some(new_state),
        );
        started.then(move |()| {
            controller.set_global_buffer_term(&controller.get_state().search_string());
        })
    }
}

/// `StartFindWithSelectionAction`.
pub struct StartFindWithSelectionAction {
    action: EditorActionBase,
}

impl StartFindWithSelectionAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(ActionOptions {
                command: CommandOptions {
                    kb_opts: vec![contrib_keybinding(
                        None,
                        Keybindings {
                            primary: Some(0),
                            mac: Some(PlatformKeybindings { primary: Some(key(KeyMod::CTRL_CMD, KeyCode::KeyE)), secondary: None }),
                            ..Keybindings::default()
                        },
                    )],
                    ..CommandOptions::new(find_ids::START_FIND_WITH_SELECTION)
                },
                context_menu_opts: Vec::new(),
                label: ActionLabel::Localized { label: localize2("startFindWithSelectionAction", "Find with Selection"), alias: None },
            }),
        }
    }
}

impl Default for StartFindWithSelectionAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for StartFindWithSelectionAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let Some(controller) = CommonFindController::get(editor) else {
            return Promise::resolved(());
        };
        controller
            .start(
                IFindStartOptions {
                    force_reveal_replace: false,
                    seed_search_string_from_selection: SeedSearchStringMode::Multiple,
                    seed_search_string_from_non_empty_selection: false,
                    seed_search_string_from_global_clipboard: false,
                    should_focus: FindStartFocusAction::NoFocusChange,
                    should_animate: true,
                    update_search_scope: false,
                    r#loop: editor.get_option(FIND).r#loop,
                },
                None,
            )
            .then(move |()| {
                controller.set_global_buffer_term(&controller.get_state().search_string());
            })
    }
}

/// `abstract class MatchFindAction`: o `_run` abstrato é a função `run_match`.
pub struct MatchFindAction {
    action: EditorActionBase,
    run_match: fn(&CommonFindController) -> bool,
}

impl MatchFindAction {
    pub fn new(opts: ActionOptions, run_match: fn(&CommonFindController) -> bool) -> Self {
        Self { action: EditorActionBase::new(opts), run_match }
    }
}

impl EditorAction for MatchFindAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let Some(controller) = CommonFindController::get(editor) else {
            return Promise::resolved(());
        };
        if (self.run_match)(&controller) {
            return Promise::resolved(());
        }
        let run_match = self.run_match;
        controller.start_for_match_navigation().then(move |()| {
            run_match(&controller);
        })
    }
}

/// O `runMatch` de `matchFindAction`: move, e se o cursor mudou de lugar (caiu numa ocorrência) fecha o widget quando o
/// `closeOnResult` pede.
fn run_match(controller: &Rc<CommonFindController>, next: bool, should_close_on_result: bool, was_find_widget_visible: bool) -> bool {
    let editor = controller.editor();
    let previous_selection = editor.get_selection();
    let result = if next { controller.move_to_next_match() } else { controller.move_to_prev_match() };

    let mut landed_on_match = false;
    if result {
        let current_selection = editor.get_selection();
        match (previous_selection, current_selection) {
            (None, Some(_)) => landed_on_match = true,
            (Some(previous_selection), Some(current_selection)) if previous_selection != current_selection => landed_on_match = true,
            _ => {}
        }
    }

    if landed_on_match {
        editor.push_undo_stop();
        if should_close_on_result && was_find_widget_visible && controller.is_find_input_focused() {
            controller.close_find_widget();
        }
        return true;
    }
    false
}

/// `matchFindAction(editor, next)`.
fn match_find_action(editor: &Rc<CodeEditorWidget>, next: bool) -> Promise<()> {
    let Some(controller) = CommonFindController::get(editor) else {
        return Promise::resolved(());
    };
    let should_close_on_result = editor.get_option(FIND).close_on_result;
    let was_find_widget_visible = controller.get_state().is_revealed();

    if !run_match(&controller, next, should_close_on_result, was_find_widget_visible) {
        let started = controller.start_for_match_navigation();
        return started.then(move |()| {
            if !run_match(&controller, next, should_close_on_result, was_find_widget_visible) {
                // Re-announce "no results" for screen readers on explicit navigation (#301126)
                let state = controller.get_state();
                if was_find_widget_visible && state.matches_count() == 0 && !state.search_string().is_empty() {
                    let message = localize(
                        "ariaSearchNoResult",
                        "{0} found for '{1}'",
                        &[nls_no_results().as_str(), state.search_string().to_string_lossy().as_str()],
                    );
                    (controller.services.aria_alert)(&message);
                }
            }
        });
    }
    Promise::resolved(())
}

fn create_next_match_find_action() -> Rc<MultiEditorAction> {
    let action = register_multi_editor_action(Rc::new(MultiEditorAction::new(ActionOptions {
        command: CommandOptions {
            kb_opts: vec![
                contrib_keybinding(
                    Some(editor_focus()),
                    Keybindings {
                        primary: Some(KeyCode::F3.bits()),
                        mac: Some(PlatformKeybindings {
                            primary: Some(key(KeyMod::CTRL_CMD, KeyCode::KeyG)),
                            secondary: Some(vec![KeyCode::F3.bits()]),
                        }),
                        ..Keybindings::default()
                    },
                ),
                contrib_keybinding(
                    ContextKeyExpr::and([Some(editor_focus()), Some(context_find_input_focused().expr())]),
                    Keybindings { primary: Some(KeyCode::Enter.bits()), ..Keybindings::default() },
                ),
            ],
            ..CommandOptions::new(find_ids::NEXT_MATCH_FIND_ACTION)
        },
        context_menu_opts: Vec::new(),
        label: ActionLabel::Localized { label: localize2("findNextMatchAction", "Find Next"), alias: None },
    })));

    action.add_implementation(
        0,
        Rc::new(|_accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic| {
            CommandImplementationResult::Promise(match_find_action(editor, true))
        }),
    );
    action
}

fn create_previous_match_find_action() -> Rc<MultiEditorAction> {
    let action = register_multi_editor_action(Rc::new(MultiEditorAction::new(ActionOptions {
        command: CommandOptions {
            kb_opts: vec![
                contrib_keybinding(
                    Some(editor_focus()),
                    Keybindings {
                        primary: Some(key(KeyMod::SHIFT, KeyCode::F3)),
                        mac: Some(PlatformKeybindings {
                            primary: Some(key(KeyMod::CTRL_CMD | KeyMod::SHIFT, KeyCode::KeyG)),
                            secondary: Some(vec![key(KeyMod::SHIFT, KeyCode::F3)]),
                        }),
                        ..Keybindings::default()
                    },
                ),
                contrib_keybinding(
                    ContextKeyExpr::and([Some(editor_focus()), Some(context_find_input_focused().expr())]),
                    Keybindings { primary: Some(key(KeyMod::SHIFT, KeyCode::Enter)), ..Keybindings::default() },
                ),
            ],
            ..CommandOptions::new(find_ids::PREVIOUS_MATCH_FIND_ACTION)
        },
        context_menu_opts: Vec::new(),
        label: ActionLabel::Localized { label: localize2("findPreviousMatchAction", "Find Previous"), alias: None },
    })));

    action.add_implementation(
        0,
        Rc::new(|_accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic| {
            CommandImplementationResult::Promise(match_find_action(editor, false))
        }),
    );
    action
}

/// `parseInt(value)`: `None` é o `NaN`.
fn parse_int(value: &str) -> Option<i64> {
    let trimmed = value.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let (sign, unsigned) = match trimmed.as_bytes().first() {
        Some(b'-') => (-1, &trimmed[1..]),
        Some(b'+') => (1, &trimmed[1..]),
        _ => (1, trimmed),
    };
    let (radix, digits) = match unsigned.strip_prefix("0x").or_else(|| unsigned.strip_prefix("0X")) {
        Some(rest) => (16, rest),
        None => (10, unsigned),
    };
    let end = digits.find(|c: char| !c.is_digit(radix)).unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    i64::from_str_radix(&digits[..end], radix).ok().map(|number| sign * number)
}

/// `MoveToMatchFindAction`.
pub struct MoveToMatchFindAction {
    action: EditorActionBase,
    highlight_decorations: Rc<RefCell<Vec<String>>>,
}

impl MoveToMatchFindAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(ActionOptions {
                command: CommandOptions {
                    precondition: Some(context_find_widget_visible().expr()),
                    ..CommandOptions::new(find_ids::GO_TO_MATCH_FIND_ACTION)
                },
                context_menu_opts: Vec::new(),
                label: ActionLabel::Localized { label: localize2("findMatchAction.goToMatch", "Go to Match..."), alias: None },
            }),
            highlight_decorations: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn clear_decorations(editor: &CodeEditorWidget, highlight_decorations: &RefCell<Vec<String>>) {
        let old = highlight_decorations.borrow().clone();
        if let Some(new) = editor.change_decorations(|change_accessor| change_accessor.delta_decorations(&old, &[])) {
            *highlight_decorations.borrow_mut() = new;
        }
    }

    fn add_decorations(editor: &CodeEditorWidget, highlight_decorations: &RefCell<Vec<String>>, range: Range) {
        let old = highlight_decorations.borrow().clone();
        let new = editor.change_decorations(|change_accessor| {
            change_accessor.delta_decorations(
                &old,
                &[
                    ModelDeltaDecoration {
                        range,
                        options: ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
                            description: "find-match-quick-access-range-highlight".to_owned(),
                            class_name: Some("rangeHighlight".to_owned()),
                            is_whole_line: true,
                            ..ModelDecorationOptionsInit::default()
                        }),
                    },
                    ModelDeltaDecoration {
                        range,
                        options: ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
                            description: "find-match-quick-access-range-highlight-overview".to_owned(),
                            overview_ruler: Some(ModelDecorationOverviewRulerInit {
                                color: Some(DecorationColor::Theme(OVERVIEW_RULER_RANGE_HIGHLIGHT.to_owned())),
                                dark_color: None,
                                position: Some(OverviewRulerLane::Full),
                            }),
                            ..ModelDecorationOptionsInit::default()
                        }),
                    },
                ],
            )
        });
        if let Some(new) = new {
            *highlight_decorations.borrow_mut() = new;
        }
    }
}

impl Default for MoveToMatchFindAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for MoveToMatchFindAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let Some(controller) = CommonFindController::get(editor) else {
            return Promise::resolved(());
        };
        let accessor = accessor.expect("a ação Go to Match precisa do acessor de serviços");

        let matches_count = controller.get_state().matches_count();
        if matches_count < 1 {
            accessor.get(I_NOTIFICATION_SERVICE).notify(Notification::new(
                Severity::Warning,
                localize("findMatchAction.noResults", "No matches. Try searching for something else.", &[]),
            ));
            return Promise::resolved(());
        }

        let quick_input_service = accessor.get(I_QUICK_INPUT_SERVICE);
        let disposables = DisposableStore::new();
        let input_box = quick_input_service.create_input_box();
        disposables.add(input_box.clone());
        input_box.set_placeholder(&localize(
            "findMatchAction.inputPlaceHolder",
            "Type a number to go to a specific match (between 1 and {0})",
            &[&matches_count.to_string()],
        ));

        let to_find_match_index = {
            let controller = controller.clone();
            Rc::new(move |value: &str| -> Option<usize> {
                let index = parse_int(value)?;

                let match_count = controller.get_state().matches_count() as i64;
                if index > 0 && index <= match_count {
                    Some((index - 1) as usize) // zero based
                } else if index < 0 && index >= -match_count {
                    Some((match_count + index) as usize)
                } else {
                    None
                }
            })
        };
        let validation_message = {
            let controller = controller.clone();
            Rc::new(move || {
                localize(
                    "findMatchAction.inputValidationMessage",
                    "Please type a number between 1 and {0}",
                    &[&controller.get_state().matches_count().to_string()],
                )
            })
        };

        let highlight_decorations = self.highlight_decorations.clone();

        disposables.add(input_box.on_did_change_value().listen({
            let (controller, editor, input_box) = (controller.clone(), editor.clone(), input_box.clone());
            let (to_find_match_index, validation_message) = (to_find_match_index.clone(), validation_message.clone());
            let highlight_decorations = highlight_decorations.clone();
            move |value: &String| {
                // updatePickerAndEditor
                match to_find_match_index(value) {
                    Some(index) => {
                        // valid
                        input_box.set_validation_message(None);
                        controller.go_to_match(index);
                        if let Some(current_match) = controller.get_state().current_match() {
                            Self::add_decorations(&editor, &highlight_decorations, current_match);
                        }
                    }
                    None => {
                        input_box.set_validation_message(Some(&validation_message()));
                        Self::clear_decorations(&editor, &highlight_decorations);
                    }
                }
            }
        }));

        disposables.add(input_box.on_did_accept().listen({
            let (controller, input_box) = (controller.clone(), input_box.clone());
            move |_: &()| match to_find_match_index(&input_box.value()) {
                Some(index) => {
                    controller.go_to_match(index);
                    input_box.hide();
                }
                None => input_box.set_validation_message(Some(&validation_message())),
            }
        }));

        disposables.add(input_box.on_did_hide().listen({
            let (editor, disposables) = (editor.clone(), disposables.clone());
            move |_: &()| {
                Self::clear_decorations(&editor, &highlight_decorations);
                disposables.dispose();
            }
        }));

        input_box.show();
        Promise::resolved(())
    }
}

/// `abstract class SelectionMatchFindAction`: o `_run` abstrato é a função `run_match`. As subclasses são
/// [`SelectionMatchFindAction::next`] e [`SelectionMatchFindAction::previous`].
pub struct SelectionMatchFindAction {
    action: EditorActionBase,
    run_match: fn(&CommonFindController) -> bool,
}

impl SelectionMatchFindAction {
    /// `NextSelectionMatchFindAction`.
    pub fn next() -> Self {
        Self {
            action: EditorActionBase::new(ActionOptions {
                command: CommandOptions {
                    kb_opts: vec![contrib_keybinding(Some(editor_focus()), ctrl_cmd_keybinding(KeyCode::F3))],
                    ..CommandOptions::new(find_ids::NEXT_SELECTION_MATCH_FIND_ACTION)
                },
                context_menu_opts: Vec::new(),
                label: ActionLabel::Localized { label: localize2("nextSelectionMatchFindAction", "Find Next Selection"), alias: None },
            }),
            run_match: CommonFindController::move_to_next_match,
        }
    }

    /// `PreviousSelectionMatchFindAction`.
    pub fn previous() -> Self {
        Self {
            action: EditorActionBase::new(ActionOptions {
                command: CommandOptions {
                    kb_opts: vec![contrib_keybinding(
                        Some(editor_focus()),
                        Keybindings { primary: Some(key(KeyMod::CTRL_CMD | KeyMod::SHIFT, KeyCode::F3)), ..Keybindings::default() },
                    )],
                    ..CommandOptions::new(find_ids::PREVIOUS_SELECTION_MATCH_FIND_ACTION)
                },
                context_menu_opts: Vec::new(),
                label: ActionLabel::Localized { label: localize2("previousSelectionMatchFindAction", "Find Previous Selection"), alias: None },
            }),
            run_match: CommonFindController::move_to_prev_match,
        }
    }
}

impl EditorAction for SelectionMatchFindAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let Some(controller) = CommonFindController::get(editor) else {
            return Promise::resolved(());
        };

        let selection_search_string = get_selection_search_string(editor, Some(SeedSearchStringMode::Single), Some(false))
            .filter(|search_string| !search_string.is_empty());
        if let Some(selection_search_string) = selection_search_string {
            controller.set_search_string(selection_search_string);
        }
        if !(self.run_match)(&controller) {
            let run_match = self.run_match;
            return controller
                .start(
                    IFindStartOptions {
                        force_reveal_replace: false,
                        seed_search_string_from_selection: SeedSearchStringMode::None,
                        seed_search_string_from_non_empty_selection: false,
                        seed_search_string_from_global_clipboard: false,
                        should_focus: FindStartFocusAction::NoFocusChange,
                        should_animate: true,
                        update_search_scope: false,
                        r#loop: editor.get_option(FIND).r#loop,
                    },
                    None,
                )
                .then(move |()| {
                    run_match(&controller);
                });
        }
        Promise::resolved(())
    }
}

fn create_start_find_replace_action() -> Rc<MultiEditorAction> {
    let action = register_multi_editor_action(Rc::new(MultiEditorAction::new(ActionOptions {
        command: CommandOptions {
            precondition: find_precondition(),
            kb_opts: vec![contrib_keybinding(
                None,
                Keybindings {
                    primary: Some(key(KeyMod::CTRL_CMD, KeyCode::KeyH)),
                    mac: Some(PlatformKeybindings { primary: Some(key(KeyMod::CTRL_CMD | KeyMod::ALT, KeyCode::KeyF)), secondary: None }),
                    ..Keybindings::default()
                },
            )],
            menu_opts: vec![CommandMenuOptions::new(menu_ids::menubar_edit_menu(), "3_find", 2.0, localize("miReplace", "&&Replace", &[]))],
            ..CommandOptions::new(find_ids::START_FIND_REPLACE_ACTION)
        },
        context_menu_opts: Vec::new(),
        label: ActionLabel::Localized { label: localize2("startReplace", "Replace"), alias: None },
    })));

    action.add_implementation(
        0,
        Rc::new(|_accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic| {
            if !editor.has_model() || *editor.get_option(READ_ONLY) {
                return CommandImplementationResult::Bool(false);
            }
            let Some(controller) = CommonFindController::get(editor) else {
                return CommandImplementationResult::Bool(false);
            };

            let current_selection = editor.get_selection().expect("o editor com modelo tem seleção");
            let find_input_focused = controller.is_find_input_focused();
            let find = *editor.get_option(FIND);
            // we only seed search string from selection when the current selection is single line and not empty,
            // + the find input is not focused
            let seed_search_string_from_selection = !current_selection.is_empty()
                && current_selection.start_line_number == current_selection.end_line_number
                && find.seed_search_string_from_selection != SeedSearchStringFromSelection::Never
                && !find_input_focused;
            // if the existing search string in find widget is empty and we don't seed search string from selection, it means
            // the Find Input is still empty, so we should focus the Find Input instead of Replace Input.
            //
            // findInputFocused true -> seedSearchStringFromSelection false, FocusReplaceInput
            // findInputFocused false, seedSearchStringFromSelection true FocusReplaceInput
            // findInputFocused false seedSearchStringFromSelection false FocusFindInput
            let should_focus = if find_input_focused || seed_search_string_from_selection {
                FindStartFocusAction::FocusReplaceInput
            } else {
                FindStartFocusAction::FocusFindInput
            };

            CommandImplementationResult::Promise(controller.start(
                IFindStartOptions {
                    force_reveal_replace: true,
                    seed_search_string_from_selection: if seed_search_string_from_selection {
                        SeedSearchStringMode::Single
                    } else {
                        SeedSearchStringMode::None
                    },
                    seed_search_string_from_non_empty_selection: find.seed_search_string_from_selection == SeedSearchStringFromSelection::Selection,
                    seed_search_string_from_global_clipboard: find.seed_search_string_from_selection != SeedSearchStringFromSelection::Never,
                    should_focus,
                    should_animate: true,
                    update_search_scope: false,
                    r#loop: find.r#loop,
                },
                None,
            ))
        }),
    );
    action
}

/// Um `new FindCommand({ id, precondition, handler, kbOpts })` de `registerEditorCommand`: o peso é `EditorContrib + 5`.
fn register_find_command(
    find_command: &impl Fn(ContributionCommandOptions<CommonFindController>) -> EditorControllerCommand<CommonFindController>,
    id: &str,
    precondition: Option<ContextKeyExpression>,
    kb_expr: Option<ContextKeyExpression>,
    keybindings: Keybindings,
    handler: fn(&CommonFindController),
) {
    register_editor_command(Rc::new(find_command(ContributionCommandOptions {
        command: CommandOptions {
            precondition,
            kb_opts: vec![CommandKeybindingsOptions::new(KeybindingWeight::EDITOR_CONTRIB + 5, kb_expr, keybindings)],
            ..CommandOptions::new(id)
        },
        handler: Rc::new(move |controller: &CommonFindController, _args: &Dynamic| handler(controller)),
    })));
}

/// As chamadas `registerEditorContribution(...)`, `registerEditorAction(...)` e `registerEditorCommand(...)` do fim do
/// arquivo (o que o carregamento do módulo faz no original). É idempotente: o registro acontece uma vez por thread.
pub fn register_find_controller(services: CommonFindControllerServices) {
    if REGISTERED.with(|registered| registered.replace(true)) {
        return;
    }
    register_editor_contribution(
        CommonFindController::ID,
        Rc::new(move |editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> { FindController::new(editor, services.clone()) }),
        EditorContributionInstantiation::Eager, // eager because it uses `saveViewState`/`restoreViewState`
    );

    start_find_action();
    next_match_find_action();
    previous_match_find_action();
    start_find_replace_action();
    register_editor_action(StartFindWithArgsAction::new);
    register_editor_action(StartFindWithSelectionAction::new);
    register_editor_action(MoveToMatchFindAction::new);
    register_editor_action(SelectionMatchFindAction::next);
    register_editor_action(SelectionMatchFindAction::previous);

    let find_command = bind_to_contribution(Rc::new(CommonFindController::get));
    let widget_visible = Some(context_find_widget_visible().expr());

    register_find_command(
        &find_command,
        find_ids::CLOSE_FIND_WIDGET_COMMAND,
        widget_visible.clone(),
        Some(editor_focus()),
        Keybindings {
            primary: Some(KeyCode::Escape.bits()),
            secondary: Some(vec![key(KeyMod::SHIFT, KeyCode::Escape)]),
            ..Keybindings::default()
        },
        |controller| controller.close_find_widget(),
    );
    register_find_command(
        &find_command,
        find_ids::TOGGLE_CASE_SENSITIVE_COMMAND,
        None,
        Some(editor_focus()),
        toggle_case_sensitive_keybinding(),
        |controller| controller.toggle_case_sensitive(),
    );
    register_find_command(
        &find_command,
        find_ids::TOGGLE_WHOLE_WORD_COMMAND,
        None,
        Some(editor_focus()),
        toggle_whole_word_keybinding(),
        |controller| controller.toggle_whole_words(),
    );
    register_find_command(
        &find_command,
        find_ids::TOGGLE_REGEX_COMMAND,
        None,
        Some(editor_focus()),
        toggle_regex_keybinding(),
        |controller| controller.toggle_regex(),
    );
    register_find_command(
        &find_command,
        find_ids::TOGGLE_SEARCH_SCOPE_COMMAND,
        None,
        Some(editor_focus()),
        toggle_search_scope_keybinding(),
        |controller| controller.toggle_search_scope(),
    );
    register_find_command(
        &find_command,
        find_ids::TOGGLE_PRESERVE_CASE_COMMAND,
        None,
        Some(editor_focus()),
        toggle_preserve_case_keybinding(),
        |controller| controller.toggle_preserve_case(),
    );
    register_find_command(
        &find_command,
        find_ids::REPLACE_ONE_ACTION,
        widget_visible.clone(),
        Some(editor_focus()),
        Keybindings { primary: Some(key(KeyMod::CTRL_CMD | KeyMod::SHIFT, KeyCode::Digit1)), ..Keybindings::default() },
        |controller| {
            controller.replace();
        },
    );
    register_find_command(
        &find_command,
        find_ids::REPLACE_ONE_ACTION,
        widget_visible.clone(),
        ContextKeyExpr::and([Some(editor_focus()), Some(context_replace_input_focused().expr())]),
        Keybindings { primary: Some(KeyCode::Enter.bits()), ..Keybindings::default() },
        |controller| {
            controller.replace();
        },
    );
    register_find_command(
        &find_command,
        find_ids::REPLACE_ALL_ACTION,
        widget_visible.clone(),
        Some(editor_focus()),
        Keybindings { primary: Some(key(KeyMod::CTRL_CMD | KeyMod::ALT, KeyCode::Enter)), ..Keybindings::default() },
        |controller| {
            controller.replace_all();
        },
    );
    register_find_command(
        &find_command,
        find_ids::REPLACE_ALL_ACTION,
        widget_visible.clone(),
        ContextKeyExpr::and([Some(editor_focus()), Some(context_replace_input_focused().expr())]),
        Keybindings {
            primary: None,
            mac: Some(PlatformKeybindings { primary: Some(key(KeyMod::CTRL_CMD, KeyCode::Enter)), secondary: None }),
            ..Keybindings::default()
        },
        |controller| {
            controller.replace_all();
        },
    );
    register_find_command(
        &find_command,
        find_ids::SELECT_ALL_MATCHES_ACTION,
        widget_visible,
        Some(editor_focus()),
        Keybindings { primary: Some(key(KeyMod::ALT, KeyCode::Enter)), ..Keybindings::default() },
        |controller| {
            controller.select_all_matches();
        },
    );
}

#[cfg(test)]
mod tests {
    use super::parse_int;

    #[test]
    fn parse_int_follows_the_javascript_function() {
        assert_eq!(parse_int("12"), Some(12));
        assert_eq!(parse_int("  -3abc"), Some(-3));
        assert_eq!(parse_int("+7"), Some(7));
        assert_eq!(parse_int("0x10"), Some(16));
        assert_eq!(parse_int("abc"), None);
        assert_eq!(parse_int(""), None);
        assert_eq!(parse_int("-"), None);
    }
}
