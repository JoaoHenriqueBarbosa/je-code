//! Porte de `src/vs/workbench/contrib/accessibility/browser/accessibleView.ts`: a visão acessível (`AccessibleView`), o
//! serviço que a cria sob demanda (`AccessibleViewService`) e o seletor de símbolos (`AccessibleViewSymbolQuickPick`).
//!
//! Notas de tradução:
//! - **Injeção por decorador (`@IOpenerService ...`).** O construtor recebe os serviços reunidos em
//!   [`AccessibleViewServices`], na ordem do original. `IChatCodeBlockContextProviderService` (o chat foi cortado do porte) e
//!   `WorkbenchToolBar` (sem equivalente nativo pronto) ficam como os contratos mínimos [`IChatCodeBlockContextProviderService`] e
//!   [`IAccessibleViewToolbar`], que o host implementa.
//! - **`CodeEditorWidget`.** Quem monta o serviço entrega o editor já criado em `services.editor`, com as opções do original
//!   (`getSimpleEditorOptions` mais `lineDecorationsWidth: 6`, `readOnly: true`, `wordWrap: 'off'`...) e sem as contribuições
//!   `CodeActionController`, `FloatingEditorClickMenu` e `FloatingEditorToolbar`, porque a seleção de contribuições
//!   (`EditorExtensionsRegistry.getEditorContributions().filter(...)`) mora na criação do widget. Como o widget não tem `View`, as
//!   teclas chegam por `CodeEditorWidget::fire_key_down` e o `getDomNode()` sempre existe.
//! - **DOM.** O contêiner `.accessible-view` e o contêiner do context view são [`ViewElement`]; `classList.add`, `appendChild` e
//!   o tamanho aplicado por `layout` são os métodos dele. `getWindow(...).document.hasFocus()` não existe (a janela nativa só
//!   recebe tecla quando tem foco), então o ramo "outra janela com foco" de `hide` não existe, e `e.preventDefault()` e
//!   `e.stopPropagation()` também não (o evento de teclado do porte não os tem).
//! - **`_getTextModel(...).then(...)`.** É um [`Promise`] resolvido, e o `then` roda como micro tarefa, como o original.
//! - **`provider.options.configureKeybindingItems = ...`.** O `options` do provedor é imutável no porte; os dois conjuntos de
//!   itens de atalho que `_updateContent` calcula ficam no próprio [`AccessibleView`] e são os que `configureKeybindings` lê.
//! - **Métodos privados que o teste alcança por `as unknown as`.** `_updateToolbar`, `_currentProvider` e `_updateLastProvider`
//!   são [`AccessibleView::update_toolbar`], [`AccessibleView::current_provider`] e [`AccessibleView::update_last_provider`],
//!   públicos.
//! - **`getFlatActionBarActions(menu.getActions({}))`.** Usa `get_flat_context_menu_actions` (a mesma lista achatada, com os
//!   separadores entre grupos).
//! - **`alert(...)` de `aria`.** É `IAccessibilityService::alert`.
//! - **`ICodeBlockActionContext.chatSessionResource`.** O contrato da plataforma não o tem (o chat foi cortado).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::actions::ActionRef;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::Event;
use je_base::key_codes::KeyCode;
use je_base::layout::IAnchor;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable, to_disposable};
use je_base::marked::{self, Token};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::runtime::queue_microtask;
use je_base::themables::ThemeIcon;
use je_base::uri::{URI, UriComponents};
use je_editor::code_action::platform::{AccessibilitySignal, IAccessibilitySignalService};
use je_editor::code_editor_widget::{CodeEditorWidget, ICodeEditor, SelectionOrRange};
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, TextSource};
use je_platform::accessibility::IAccessibilityService;
use je_platform::accessibility::accessible_view::{
    AccessibleContentProvider, AccessibleContentProviderArgs, AccessibleViewContentProvider, AccessibleViewPosition, AccessibleViewProviderId, AccessibleViewType, ExtensionContentProvider,
    ExtensionContentProviderArgs, IAccessibleViewOptions, IAccessibleViewService, IAccessibleViewSymbol, ICodeBlockActionContext, IPosition, NavigationType,
};
use je_platform::actions::menu_entry_action_view_item::get_flat_context_menu_actions;
use je_platform::actions::menu_ids::accessible_view as accessible_view_menu_id;
use je_platform::actions::{IMenu, IMenuService, MenuActionOptions};
use je_platform::commands::commands::ICommandService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::{IContextKey, IContextKeyService};
use je_platform::contextview::context_view::{AnchorTarget, HideData, IContextViewDelegate, IContextViewService};
use je_platform::contextview::view_element::ViewElement;
use je_platform::keybinding::keybinding::{IKeybindingService, KeyboardEvent};
use je_platform::keybinding::keybinding_resolver::ResolutionResult;
use je_platform::opener::opener::{IOpenerService, OpenResource};
use je_platform::quickinput::quick_input::{IQuickInputService, IQuickPick, IQuickPickItem, QuickPickInput};
use je_platform::storage::storage::{StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;

use super::accessibility_configuration::{
    ACCESSIBLE_VIEW_CLOSE_ON_KEY_PRESS_SETTING, ACCESSIBLE_VIEW_SHOWN_STORAGE_PREFIX, HIDE_ACCESSIBLE_VIEW_SETTING, accessibility_command_id, accessibility_help_is_shown,
    accessible_view_contains_code_blocks, accessible_view_current_provider_id, accessible_view_go_to_symbol_supported, accessible_view_has_assigned_keybindings,
    accessible_view_has_unassigned_keybindings, accessible_view_in_code_block, accessible_view_is_shown, accessible_view_on_last_line, accessible_view_supports_navigation,
    accessible_view_verbosity_enabled,
};
use super::accessible_view_keybinding_resolver::resolve_content_and_keybinding_items;
use crate::services::layout::layout_service::ILayoutService;

/// `DIMENSIONS`.
const MAX_WIDTH: f64 = 900.0;
const WIDTH_RATIO: f64 = 0.75;
const MAX_HEIGHT_RATIO: f64 = 0.6;

/// `Codicon.primitiveSquare.id`.
const CODICON_PRIMITIVE_SQUARE: &str = "primitive-square";

/// `AccesibleViewContentProvider` é [`AccessibleViewContentProvider`] (o `export type` do arquivo).
pub type AccesibleViewContentProvider = AccessibleViewContentProvider;

/// `ICodeBlock`.
#[derive(Clone)]
struct CodeBlock {
    start_line: i32,
    end_line: i32,
    code: String,
    language_id: Option<String>,
}

// region: contratos que o host implementa

/// O `WorkbenchToolBar` da visão acessível: o que `AccessibleView` lê e escreve nele.
pub trait IAccessibleViewToolbar {
    /// `toolbar.context = { viewId: 'accessibleView' }`.
    fn set_context(&self, context: Dynamic);
    /// `toolbar.setAriaLabel(label)`.
    fn set_aria_label(&self, label: &str);
    /// `toolbar.setActions(actions)`.
    fn set_actions(&self, actions: Vec<ActionRef>);
    /// `isActiveElement(toolbar.getElement())`: o foco está na barra.
    fn is_focused(&self) -> bool;
    /// O `KEY_DOWN` do elemento da barra.
    fn on_key_down(&self) -> Event<KeyboardEvent>;
}

/// `IChatCodeBlockContextProviderService`: o registro do provedor do bloco de código em foco.
pub trait IChatCodeBlockContextProviderService {
    /// `registerProvider(provider, id)`.
    fn register_provider(&self, provider: Rc<dyn ICodeBlockContextProvider>, id: &str) -> DisposableRef;
}

/// `ICodeBlockContextProvider`.
pub trait ICodeBlockContextProvider {
    fn get_code_block_context(&self) -> Option<ICodeBlockActionContext>;
}

/// Os serviços do construtor de `AccessibleView`, na ordem do original, mais o editor e a barra (ver as notas do módulo).
pub struct AccessibleViewServices {
    pub opener_service: Rc<dyn IOpenerService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub model_service: Rc<dyn IModelService>,
    pub context_view_service: Rc<dyn IContextViewService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub accessibility_service: Rc<dyn IAccessibilityService>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub layout_service: Rc<dyn ILayoutService>,
    pub menu_service: Rc<dyn IMenuService>,
    pub command_service: Rc<dyn ICommandService>,
    pub code_block_context_provider_service: Rc<dyn IChatCodeBlockContextProviderService>,
    pub storage_service: Rc<dyn StorageService>,
    pub quick_input_service: Rc<dyn IQuickInputService>,
    pub accessibility_signal_service: Rc<dyn IAccessibilitySignalService>,
    /// O `_editorWidget` criado com as opções da visão acessível.
    pub editor: Rc<CodeEditorWidget>,
    /// O `_toolbar`.
    pub toolbar: Rc<dyn IAccessibleViewToolbar>,
}

// endregion

// region: o que as duas classes de provedor têm em comum

fn provider_id(provider: &AccessibleViewContentProvider) -> String {
    match provider {
        AccessibleViewContentProvider::Content(content) => content.id.as_str().to_owned(),
        AccessibleViewContentProvider::Extension(extension) => extension.id.clone(),
    }
}

fn provider_options(provider: &AccessibleViewContentProvider) -> &IAccessibleViewOptions {
    match provider {
        AccessibleViewContentProvider::Content(content) => &content.options,
        AccessibleViewContentProvider::Extension(extension) => &extension.options,
    }
}

fn provider_provide_content(provider: &AccessibleViewContentProvider) -> String {
    match provider {
        AccessibleViewContentProvider::Content(content) => content.provide_content(),
        AccessibleViewContentProvider::Extension(extension) => (extension.provide_content)(),
    }
}

fn provider_on_close(provider: &AccessibleViewContentProvider) {
    match provider {
        AccessibleViewContentProvider::Content(content) => (content.on_close)(),
        AccessibleViewContentProvider::Extension(extension) => (extension.on_close)(),
    }
}

/// `provider.onOpen?.()`: o `ExtensionContentProvider` não devolve recursos.
fn provider_on_open(provider: &AccessibleViewContentProvider) -> Option<DisposableRef> {
    match provider {
        AccessibleViewContentProvider::Content(content) => content.on_open.as_ref().and_then(|on_open| on_open()),
        AccessibleViewContentProvider::Extension(extension) => {
            if let Some(on_open) = &extension.on_open {
                on_open();
            }
            None
        }
    }
}

fn provider_provide_next_content(provider: &AccessibleViewContentProvider) -> Option<String> {
    match provider {
        AccessibleViewContentProvider::Content(content) => content.provide_next_content.as_ref().and_then(|provide| provide()),
        AccessibleViewContentProvider::Extension(extension) => extension.provide_next_content.as_ref().and_then(|provide| provide()),
    }
}

fn provider_provide_previous_content(provider: &AccessibleViewContentProvider) -> Option<String> {
    match provider {
        AccessibleViewContentProvider::Content(content) => content.provide_previous_content.as_ref().and_then(|provide| provide()),
        AccessibleViewContentProvider::Extension(extension) => extension.provide_previous_content.as_ref().and_then(|provide| provide()),
    }
}

fn provider_supports_navigation(provider: &AccessibleViewContentProvider) -> bool {
    match provider {
        AccessibleViewContentProvider::Content(content) => content.provide_next_content.is_some() || content.provide_previous_content.is_some(),
        AccessibleViewContentProvider::Extension(extension) => extension.provide_next_content.is_some() || extension.provide_previous_content.is_some(),
    }
}

fn provider_on_did_change_content(provider: &AccessibleViewContentProvider) -> Option<Event<()>> {
    match provider {
        AccessibleViewContentProvider::Content(content) => content.on_did_change_content.clone(),
        AccessibleViewContentProvider::Extension(extension) => extension.on_did_change_content.clone(),
    }
}

fn provider_actions(provider: &AccessibleViewContentProvider) -> Option<Vec<ActionRef>> {
    match provider {
        AccessibleViewContentProvider::Content(content) => content.actions(),
        AccessibleViewContentProvider::Extension(extension) => extension.actions.clone(),
    }
}

fn provider_dispose(provider: &AccessibleViewContentProvider) {
    match provider {
        AccessibleViewContentProvider::Content(content) => content.dispose(),
        AccessibleViewContentProvider::Extension(extension) => extension.dispose(),
    }
}

fn is_extension_provider(provider: &AccessibleViewContentProvider) -> bool {
    matches!(provider, AccessibleViewContentProvider::Extension(_))
}

fn is_chat_provider_id(id: &str) -> bool {
    id == AccessibleViewProviderId::PanelChat.as_str() || id == AccessibleViewProviderId::QuickChat.as_str()
}

/// `provider.actions.length`, `provider.actions` truthy and non empty.
fn has_provided_actions(provider: &AccessibleViewContentProvider) -> bool {
    provider_actions(provider).is_some_and(|actions| !actions.is_empty())
}

fn to_editor_position(position: IPosition) -> Position {
    Position::new(position.line_number as i32, position.column as i32)
}

fn to_platform_position(position: Position) -> IPosition {
    IPosition { line_number: position.line_number.max(0) as u32, column: position.column.max(0) as u32 }
}

// endregion

/// `AccessibleView`.
pub struct AccessibleView {
    me: Weak<AccessibleView>,
    base: Rc<Disposable>,
    services: AccessibleViewServices,

    accessibility_help_is_shown: Rc<dyn IContextKey>,
    on_last_line: Rc<dyn IContextKey>,
    accessible_view_is_shown: Rc<dyn IContextKey>,
    accessible_view_supports_navigation: Rc<dyn IContextKey>,
    accessible_view_verbosity_enabled: Rc<dyn IContextKey>,
    accessible_view_go_to_symbol_supported: Rc<dyn IContextKey>,
    accessible_view_current_provider_id: Rc<dyn IContextKey>,
    accessible_view_in_code_block: Rc<dyn IContextKey>,
    accessible_view_contains_code_blocks: Rc<dyn IContextKey>,
    has_unassigned_keybindings: Rc<dyn IContextKey>,
    has_assigned_keybindings: Rc<dyn IContextKey>,

    code_blocks: RefCell<Option<Vec<CodeBlock>>>,
    is_in_quick_pick: Cell<bool>,
    container: Rc<ViewElement>,
    title: RefCell<String>,
    toolbar_menu: Rc<MutableDisposable<dyn IMenu>>,
    /// Listeners tied to the provider of the current [`AccessibleView::show`] call.
    show_disposables: Rc<MutableDisposable<DisposableStore>>,
    /// Listeners tied to the most recent render.
    render_disposables: Rc<MutableDisposable<DisposableStore>>,
    last_provider_listener: Rc<MutableDisposable<dyn IDisposable>>,
    help_clear_listener: Rc<MutableDisposable<dyn IDisposable>>,
    code_block_context_provider_registration: Rc<MutableDisposable<dyn IDisposable>>,
    configure_keybindings_disposables: Rc<MutableDisposable<DisposableStore>>,

    /// `provider.options.configureKeybindingItems` e `configuredKeybindingItems` (ver as notas do módulo).
    configure_keybinding_items: RefCell<Option<Vec<IQuickPickItem>>>,
    configured_keybinding_items: RefCell<Option<Vec<IQuickPickItem>>>,

    current_provider: RefCell<Option<AccessibleViewContentProvider>>,
    current_content: RefCell<Option<String>>,

    last_provider: RefCell<Option<AccessibleViewContentProvider>>,
    last_provider_position: RefCell<HashMap<String, Position>>,

    view_container: RefCell<Option<Rc<ViewElement>>>,
}

impl AccessibleView {
    pub fn new(services: AccessibleViewServices) -> Rc<Self> {
        let key_service = services.context_key_service.clone();
        let container = ViewElement::with_classes(&["accessible-view"]);
        if services.configuration_service.get_value(Some(HIDE_ACCESSIBLE_VIEW_SETTING), None).is_truthy() {
            container.class_list_add("hide");
        }
        services.toolbar.set_context(Dynamic::object([("viewId", Dynamic::from("accessibleView"))]));

        let base = Disposable::new();
        let toolbar_menu: Rc<MutableDisposable<dyn IMenu>> = MutableDisposable::new();
        let show_disposables = MutableDisposable::new();
        let render_disposables = MutableDisposable::new();
        let last_provider_listener: Rc<MutableDisposable<dyn IDisposable>> = MutableDisposable::new();
        let help_clear_listener: Rc<MutableDisposable<dyn IDisposable>> = MutableDisposable::new();
        let code_block_context_provider_registration: Rc<MutableDisposable<dyn IDisposable>> = MutableDisposable::new();
        let configure_keybindings_disposables = MutableDisposable::new();
        base.register(toolbar_menu.clone());
        base.register(show_disposables.clone());
        base.register(render_disposables.clone());
        base.register(last_provider_listener.clone());
        base.register(help_clear_listener.clone());
        base.register(code_block_context_provider_registration.clone());
        base.register(configure_keybindings_disposables.clone());
        base.register(services.editor.clone());

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            accessibility_help_is_shown: accessibility_help_is_shown().bind_to(&*key_service),
            accessible_view_is_shown: accessible_view_is_shown().bind_to(&*key_service),
            accessible_view_supports_navigation: accessible_view_supports_navigation().bind_to(&*key_service),
            accessible_view_verbosity_enabled: accessible_view_verbosity_enabled().bind_to(&*key_service),
            accessible_view_go_to_symbol_supported: accessible_view_go_to_symbol_supported().bind_to(&*key_service),
            accessible_view_current_provider_id: accessible_view_current_provider_id().bind_to(&*key_service),
            accessible_view_in_code_block: accessible_view_in_code_block().bind_to(&*key_service),
            accessible_view_contains_code_blocks: accessible_view_contains_code_blocks().bind_to(&*key_service),
            on_last_line: accessible_view_on_last_line().bind_to(&*key_service),
            has_unassigned_keybindings: accessible_view_has_unassigned_keybindings().bind_to(&*key_service),
            has_assigned_keybindings: accessible_view_has_assigned_keybindings().bind_to(&*key_service),
            services,
            code_blocks: RefCell::new(None),
            is_in_quick_pick: Cell::new(false),
            container,
            title: RefCell::new(String::new()),
            toolbar_menu,
            show_disposables,
            render_disposables,
            last_provider_listener,
            help_clear_listener,
            code_block_context_provider_registration,
            configure_keybindings_disposables,
            configure_keybinding_items: RefCell::new(None),
            configured_keybinding_items: RefCell::new(None),
            current_provider: RefCell::new(None),
            current_content: RefCell::new(None),
            last_provider: RefCell::new(None),
            last_provider_position: RefCell::new(HashMap::new()),
            view_container: RefCell::new(None),
        });
        this.register_listeners();
        this
    }

    /// Os `this._register(...)` do fim do construtor.
    fn register_listeners(&self) {
        let services = &self.services;
        let me = self.me.clone();
        self.base.register(services.accessibility_service.on_did_change_screen_reader_optimized().listen({
            let me = me.clone();
            move |_| {
                let Some(view) = me.upgrade() else { return };
                if let Some(provider) = view.current_provider() {
                    if view.accessibility_help_is_shown.get().is_truthy() {
                        view.show(Some(provider), None, false, None);
                    }
                }
            }
        }));
        self.base.register(services.configuration_service.on_did_change_configuration().listen({
            let me = me.clone();
            move |event| {
                let Some(view) = me.upgrade() else { return };
                if let Some(provider) = view.current_provider() {
                    if let AccessibleViewContentProvider::Content(content) = &provider {
                        if event.affects_configuration(content.verbosity_setting_key.as_str(), None) {
                            if view.accessibility_help_is_shown.get().is_truthy() {
                                view.show(Some(provider.clone()), None, false, None);
                            }
                            view.accessible_view_verbosity_enabled.set(view.services.configuration_service.get_value(Some(content.verbosity_setting_key.as_str()), None));
                            view.update_toolbar(content.actions(), Some(content.options.view_type));
                        }
                    }
                }
                if event.affects_configuration(HIDE_ACCESSIBLE_VIEW_SETTING, None) {
                    if view.services.configuration_service.get_value(Some(HIDE_ACCESSIBLE_VIEW_SETTING), None).is_truthy() {
                        view.container.class_list_add("hide");
                    } else {
                        view.container.class_list_remove("hide");
                    }
                }
            }
        }));
        self.base.register(services.editor.on_did_dispose().listen({
            let me = me.clone();
            move |_| {
                if let Some(view) = me.upgrade() {
                    view.reset_context_keys();
                }
            }
        }));
        self.base.register(services.editor.on_did_change_cursor_position().listen(move |_| {
            let Some(view) = me.upgrade() else { return };
            let line_count = view.services.editor.get_model().map(|model| model.get_line_count());
            view.on_last_line.set(Dynamic::from(view.services.editor.get_position().map(|position| position.line_number) == line_count));
            let cursor_position = view.services.editor.get_position().map(|position| position.line_number);
            if let (Some(code_blocks), Some(cursor_position)) = (view.code_blocks.borrow().as_ref(), cursor_position) {
                let in_code_block = code_blocks.iter().any(|block| block.start_line <= cursor_position && block.end_line >= cursor_position);
                view.accessible_view_in_code_block.set(Dynamic::from(in_code_block));
            }
            view.play_diff_signals();
        }));
    }

    /// `editorWidget`.
    pub fn editor_widget(&self) -> &Rc<CodeEditorWidget> {
        &self.services.editor
    }

    /// `title.textContent`.
    pub fn title(&self) -> String {
        self.title.borrow().clone()
    }

    /// O contêiner `.accessible-view`.
    pub fn container(&self) -> &Rc<ViewElement> {
        &self.container
    }

    /// `_currentProvider`.
    pub fn current_provider(&self) -> Option<AccessibleViewContentProvider> {
        self.current_provider.borrow().clone()
    }

    fn play_diff_signals(&self) {
        let Some(current) = self.current_provider() else { return };
        let id = provider_id(&current);
        if id != AccessibleViewProviderId::DiffEditor.as_str() && id != AccessibleViewProviderId::InlineCompletions.as_str() {
            return;
        }
        let (Some(position), Some(model)) = (self.services.editor.get_position(), self.services.editor.get_model()) else { return };
        let line_content = model.get_line_content(position.line_number).to_string_lossy();
        if line_content.starts_with('+') {
            self.services.accessibility_signal_service.play_signal(AccessibilitySignal::DiffLineInserted);
        } else if line_content.starts_with('-') {
            self.services.accessibility_signal_service.play_signal(AccessibilitySignal::DiffLineDeleted);
        }
    }

    fn reset_context_keys(&self) {
        self.accessibility_help_is_shown.reset();
        self.accessible_view_is_shown.reset();
        self.accessible_view_supports_navigation.reset();
        self.accessible_view_verbosity_enabled.reset();
        self.accessible_view_go_to_symbol_supported.reset();
        self.accessible_view_current_provider_id.reset();
        self.has_assigned_keybindings.reset();
        self.has_unassigned_keybindings.reset();
    }

    /// `getPosition(id)`.
    pub fn get_position(&self, id: Option<AccessibleViewProviderId>) -> Option<Position> {
        let id = id?;
        let last_provider = self.last_provider.borrow().clone()?;
        if provider_id(&last_provider) != id.as_str() {
            return None;
        }
        self.services.editor.get_position()
    }

    /// `setPosition(position, reveal, select)`.
    pub fn set_position(&self, position: Position, reveal: bool, select: bool) {
        let editor = &self.services.editor;
        editor.set_position(position, None);
        if reveal {
            editor.reveal_position(position, None);
        }
        if select {
            let line_length = editor.get_model().map_or(0, |model| model.get_line_length(position.line_number));
            if line_length > 0 {
                editor.set_selection(SelectionOrRange::Range(Range::new(position.line_number, 1, position.line_number, line_length as i32 + 1)), None);
            }
        }
    }

    /// `getCodeBlockContext()`.
    pub fn get_code_block_context(&self) -> Option<ICodeBlockActionContext> {
        let position = self.services.editor.get_position();
        let code_blocks = self.code_blocks.borrow();
        let code_blocks = code_blocks.as_ref().filter(|blocks| !blocks.is_empty())?;
        let position = position?;
        let code_block_index = code_blocks.iter().position(|block| block.start_line <= position.line_number && block.end_line >= position.line_number)?;
        let code_block = &code_blocks[code_block_index];
        Some(ICodeBlockActionContext { code: code_block.code.clone(), language_id: code_block.language_id.clone(), code_block_index, element: Rc::new(()) })
    }

    /// `navigateToCodeBlock(type)`.
    pub fn navigate_to_code_block(&self, navigation: NavigationType) {
        let Some(position) = self.services.editor.get_position() else { return };
        let code_block = {
            let code_blocks = self.code_blocks.borrow();
            let Some(code_blocks) = code_blocks.as_ref().filter(|blocks| !blocks.is_empty()) else { return };
            match navigation {
                NavigationType::Previous => code_blocks.iter().rev().find(|block| block.end_line < position.line_number).cloned(),
                NavigationType::Next => code_blocks.iter().find(|block| block.start_line > position.line_number).cloned(),
            }
        };
        let Some(code_block) = code_block else { return };
        self.set_position(Position::new(code_block.start_line, 1), true, false);
    }

    /// `showLastProvider(id)`.
    pub fn show_last_provider(&self, id: AccessibleViewProviderId) {
        let Some(last_provider) = self.last_provider.borrow().clone() else { return };
        if provider_options(&last_provider).id != Some(id) {
            return;
        }
        self.rc().show(Some(last_provider), None, false, None);
    }

    /// `getAccessibilityStatus()`: `(providerId, isInCodeBlock, onLastLine)`.
    pub fn get_accessibility_status(&self) -> (Option<String>, bool, bool) {
        (self.current_provider().map(|provider| provider_id(&provider)), self.accessible_view_in_code_block.get().is_truthy(), self.on_last_line.get().is_truthy())
    }

    fn rc(&self) -> Rc<AccessibleView> {
        self.me.upgrade().expect("a visão acessível está viva enquanto um método dela roda")
    }

    /// `show(provider?, symbol?, showAccessibleViewHelp?, position?)`.
    pub fn show(self: &Rc<Self>, provider: Option<AccessibleViewContentProvider>, symbol: Option<IAccessibleViewSymbol>, show_accessible_view_help: bool, position: Option<IPosition>) {
        let provider = match provider.or_else(|| self.current_provider()) {
            Some(provider) => provider,
            None => return,
        };
        let show_disposables = DisposableStore::new();
        self.show_disposables.set(Some(show_disposables.clone()));
        if let Some(on_open_disposable) = provider_on_open(&provider) {
            show_disposables.add(on_open_disposable);
        }
        let delegate: Rc<dyn IContextViewDelegate> =
            Rc::new(AccessibleViewDelegate { view: Rc::downgrade(self), provider: provider.clone(), show_accessible_view_help, show_disposables: show_disposables.clone() });
        self.services.context_view_service.show_context_view(delegate, None, false);

        if let Some(position) = position {
            // Context view takes time to show up, so we need to wait for it to show up before we can set the position
            let view = self.clone();
            queue_microtask(move || {
                let line_number = position.line_number as i32;
                let column = position.column as i32;
                view.services.editor.reveal_line(line_number, None);
                view.services.editor.set_selection(SelectionOrRange::Range(Range::new(line_number, column, line_number, column)), None);
            });
        }

        if let (Some(symbol), Some(current)) = (symbol, self.current_provider()) {
            self.show_symbol(&current, &symbol);
        }
        if provider_options(&provider).id.is_some() {
            // only cache a provider with an ID so that it will eventually be cleared.
            self.set_last_provider(Some(provider.clone()));
        }
        if is_chat_provider_id(&provider_id(&provider)) && self.code_block_context_provider_registration.value().is_none() {
            let registration = self
                .services
                .code_block_context_provider_service
                .register_provider(Rc::new(AccessibleViewCodeBlockProvider { view: Rc::downgrade(self) }), "accessibleView");
            self.code_block_context_provider_registration.set(Some(registration));
        }
        if is_extension_provider(&provider) {
            self.services.storage_service.store(
                &format!("{ACCESSIBLE_VIEW_SHOWN_STORAGE_PREFIX}{}", provider_id(&provider)),
                StorageValue::from(true),
                StorageScope::Application,
                StorageTarget::User,
            );
        }
        // `showSymbol` above can re-enter `show`, which releases the listeners of this call
        if let Some(on_did_change_content) = provider_on_did_change_content(&provider) {
            if !show_disposables.is_disposed() {
                let view = Rc::downgrade(self);
                let provider = provider.clone();
                show_disposables.add(on_did_change_content.listen(move |_| {
                    let Some(view) = view.upgrade() else { return };
                    let container = view.view_container.borrow().clone();
                    if let Some(container) = container {
                        view.render(&provider, &container, show_accessible_view_help, None);
                    }
                }));
            }
        }
    }

    /// `_setLastProvider(provider)`.
    fn set_last_provider(&self, provider: Option<AccessibleViewContentProvider>) {
        *self.last_provider.borrow_mut() = provider.clone();
        let listener = match &provider {
            Some(AccessibleViewContentProvider::Content(content)) => content.on_did_request_clear_last_provider.as_ref().map(|event| {
                let view = self.me.clone();
                event.listen(move |id| {
                    let Some(view) = view.upgrade() else { return };
                    let last_provider_id = view.last_provider.borrow().as_ref().and_then(|last| provider_options(last).id);
                    if last_provider_id == Some(*id) {
                        view.set_last_provider(None);
                    }
                    view.last_provider_position.borrow_mut().remove(id.as_str());
                })
            }),
            _ => None,
        };
        self.last_provider_listener.set(listener);
    }

    /// `previous()`.
    pub fn previous(&self) {
        let current = self.current_provider();
        let new_content = current.as_ref().and_then(provider_provide_previous_content).filter(|content| !content.is_empty());
        let container = self.view_container.borrow().clone();
        if let (Some(current), Some(container), Some(new_content)) = (current, container, new_content) {
            self.rc().render(&current, &container, false, Some(new_content));
        }
    }

    /// `next()`.
    pub fn next(&self) {
        let current = self.current_provider();
        let new_content = current.as_ref().and_then(provider_provide_next_content).filter(|content| !content.is_empty());
        let container = self.view_container.borrow().clone();
        if let (Some(current), Some(container), Some(new_content)) = (current, container, new_content) {
            self.rc().render(&current, &container, false, Some(new_content));
        }
    }

    fn verbosity_enabled(&self) -> bool {
        let Some(current) = self.current_provider() else { return false };
        match &current {
            AccessibleViewContentProvider::Content(content) => matches!(self.services.configuration_service.get_value(Some(content.verbosity_setting_key.as_str()), None), Dynamic::Bool(true)),
            AccessibleViewContentProvider::Extension(extension) => {
                self.services.storage_service.get_boolean(&format!("{ACCESSIBLE_VIEW_SHOWN_STORAGE_PREFIX}{}", extension.id), StorageScope::Application, Some(false)).unwrap_or(false)
            }
        }
    }

    /// `goToSymbol()`.
    pub fn go_to_symbol(self: &Rc<Self>) {
        let Some(current) = self.current_provider() else { return };
        self.is_in_quick_pick.set(true);
        AccessibleViewSymbolQuickPick { accessible_view: self.clone(), quick_input_service: self.services.quick_input_service.clone() }.show(&current);
    }

    /// `calculateCodeBlocks(markdown?)`.
    pub fn calculate_code_blocks(&self, markdown: Option<&str>) {
        let Some(markdown) = markdown.filter(|markdown| !markdown.is_empty()) else { return };
        let Some(current) = self.current_provider() else { return };
        if !is_chat_provider_id(&provider_id(&current)) {
            return;
        }
        if provider_options(&current).language.as_deref().is_some_and(|language| !language.is_empty() && language != "markdown") {
            // Symbols haven't been provided and we cannot parse this language
            return;
        }
        let mut code_blocks = Vec::new();
        let lines: Vec<&str> = markdown.split('\n').collect();
        let mut in_block = false;
        let mut start_line = 0usize;
        let mut language_id: Option<String> = None;
        for (index, line) in lines.iter().enumerate() {
            if !in_block && line.starts_with("```") {
                in_block = true;
                start_line = index + 1;
                language_id = Some(line[3..].trim().to_owned());
            } else if in_block && line.ends_with("```") {
                in_block = false;
                let end_line = index;
                let code = lines[start_line..end_line].join("\n");
                code_blocks.push(CodeBlock { start_line: start_line as i32, end_line: end_line as i32, code, language_id: language_id.clone() });
            }
        }
        self.accessible_view_contains_code_blocks.set(Dynamic::from(!code_blocks.is_empty()));
        *self.code_blocks.borrow_mut() = Some(code_blocks);
    }

    /// `getSymbols()`.
    pub fn get_symbols(&self) -> Option<Vec<IAccessibleViewSymbol>> {
        let provider = self.current_provider()?;
        let current_content = self.current_content.borrow().clone().filter(|content| !content.is_empty())?;
        let provided = match &provider {
            AccessibleViewContentProvider::Content(content) => content.get_symbols.as_ref().map(|get_symbols| get_symbols()).unwrap_or_default(),
            AccessibleViewContentProvider::Extension(_) => Vec::new(),
        };
        if !provided.is_empty() {
            return Some(provided);
        }
        if provider_options(&provider).language.as_deref().is_some_and(|language| !language.is_empty() && language != "markdown") {
            // Symbols haven't been provided and we cannot parse this language
            return None;
        }
        let tokens = marked::lexer(JsString::from(current_content.as_str()).units()).ok()?;
        let mut symbols = provided;
        Self::convert_tokens_to_symbols(&tokens.tokens, &mut symbols);
        (!symbols.is_empty()).then_some(symbols)
    }

    /// `openHelpLink()`.
    pub fn open_help_link(&self) {
        let Some(current) = self.current_provider() else { return };
        let Some(read_more_url) = provider_options(&current).read_more_url.clone() else { return };
        let _ = self.services.opener_service.open(&OpenResource::Uri(URI::parse(read_more_url.as_str(), false)), None);
    }

    /// `configureKeybindings(unassigned)`.
    pub fn configure_keybindings(self: &Rc<Self>, unassigned: bool) {
        self.is_in_quick_pick.set(true);
        let provider = self.update_last_provider();
        let items = if unassigned { self.configure_keybinding_items.borrow().clone() } else { self.configured_keybinding_items.borrow().clone() };
        let Some(items) = items else { return };
        let disposables = DisposableStore::new();
        self.configure_keybindings_disposables.set(Some(disposables.clone()));
        let quick_pick: Rc<dyn IQuickPick> = self.services.quick_input_service.create_quick_pick(None);
        disposables.add(quick_pick.clone());
        quick_pick.set_items(items.into_iter().map(QuickPickInput::Item).collect());
        quick_pick.set_title(Some(localize("keybindings", "Configure keybindings", &[])));
        quick_pick.set_placeholder(Some(localize("selectKeybinding", "Select a command ID to configure a keybinding for it", &[])));
        quick_pick.show();
        disposables.add(quick_pick.on_did_accept().listen({
            let quick_pick = quick_pick.clone();
            let command_service = self.services.command_service.clone();
            move |_| {
                match quick_pick.selected_items().into_iter().next() {
                    Some(item) => {
                        let id = item.base.id.map_or(Dynamic::Undefined, Dynamic::from);
                        let quick_pick = quick_pick.clone();
                        command_service.execute_command("workbench.action.openGlobalKeybindings", vec![id]).finally(move || quick_pick.dispose());
                    }
                    None => quick_pick.dispose(),
                }
            }
        }));
        disposables.add(quick_pick.on_did_hide().listen({
            let quick_pick = quick_pick.clone();
            let view = Rc::downgrade(self);
            let disposables = disposables.clone();
            move |_| {
                let Some(view) = view.upgrade() else { return };
                if quick_pick.selected_items().is_empty() {
                    if let Some(provider) = &provider {
                        view.show(Some(provider.clone()), None, false, None);
                    }
                }
                disposables.dispose();
                view.is_in_quick_pick.set(false);
            }
        }));
    }

    /// `_convertTokensToSymbols(tokens, symbols)`.
    fn convert_tokens_to_symbols(tokens: &[Token], symbols: &mut Vec<IAccessibleViewSymbol>) {
        let mut first_list_item: Option<String> = None;
        for token in tokens {
            let (token_type, label): (&str, Option<String>) = match token {
                Token::Heading(heading) => ("heading", Some(heading.text.to_string_lossy())),
                Token::Paragraph(paragraph) => ("paragraph", Some(paragraph.text.to_string_lossy())),
                Token::Code(code) => ("code", Some(code.text.to_string_lossy())),
                Token::List(list) => {
                    let item_texts: Vec<String> = list.items.iter().filter_map(|item| if let Token::ListItem(item) = item { Some(item.text.to_string_lossy()) } else { None }).collect();
                    match item_texts.first() {
                        Some(first) => {
                            first_list_item = Some(format!("- {first}"));
                            ("list", Some(item_texts.join(", ")))
                        }
                        None => ("list", None),
                    }
                }
                Token::Space(_) => ("space", None),
                _ => ("", None),
            };
            if let Some(label) = label.filter(|label| !label.is_empty()) {
                let mut item = IQuickPickItem::default();
                item.base.label = JsString::from(localize("symbolLabel", "({0}) {1}", &[token_type, &label]));
                item.base.aria_label = Some(localize("symbolLabelAria", "({0}) {1}", &[token_type, &label]));
                symbols.push(IAccessibleViewSymbol { item, markdown_to_parse: Some(label), first_list_item: first_list_item.take(), line_number: None, end_line_number: None });
            }
        }
    }

    /// `showSymbol(provider, symbol)`.
    pub fn show_symbol(self: &Rc<Self>, provider: &AccessibleViewContentProvider, symbol: &IAccessibleViewSymbol) {
        let Some(current_content) = self.current_content.borrow().clone().filter(|content| !content.is_empty()) else { return };
        let mut line_number: Option<u32> = symbol.line_number;
        let markdown_to_parse = symbol.markdown_to_parse.clone();
        if line_number.is_none() && markdown_to_parse.is_none() {
            // No symbols provided and we cannot parse this language
            return;
        }
        if line_number.is_none() {
            if let Some(markdown_to_parse) = markdown_to_parse.filter(|markdown| !markdown.is_empty()) {
                // Note that this scales poorly, thus isn't used for worst case scenarios like the terminal, for which a line number will always be provided.
                // Parse the markdown to find the line number
                let first_line = markdown_to_parse.split('\n').next().unwrap_or_default().to_owned();
                let index = current_content.split('\n').position(|line| line.contains(first_line.as_str()) || symbol.first_list_item.as_deref().is_some_and(|item| !item.is_empty() && line.contains(item)));
                if let Some(index) = index {
                    line_number = Some(index as u32 + 1);
                }
            }
        }
        let Some(line_number) = line_number else { return };
        self.is_in_quick_pick.set(false);
        self.show(Some(provider.clone()), None, false, Some(IPosition { line_number, column: 1 }));
        self.update_context_keys(provider, true);
    }

    /// `disableHint()`.
    pub fn disable_hint(&self) {
        let Some(AccessibleViewContentProvider::Content(content)) = self.current_provider() else { return };
        let _ = self.services.configuration_service.update_value(&content.verbosity_setting_key, Dynamic::from(false), None, None);
        self.services.accessibility_service.alert(&localize("disableAccessibilityHelp", "{0} accessibility verbosity is now disabled", &[content.verbosity_setting_key.as_str()]));
    }

    fn update_context_keys(&self, provider: &AccessibleViewContentProvider, shown: bool) {
        if provider_options(provider).view_type == AccessibleViewType::Help {
            self.accessibility_help_is_shown.set(Dynamic::from(shown));
            self.accessible_view_is_shown.reset();
        } else {
            self.accessible_view_is_shown.set(Dynamic::from(shown));
            self.accessibility_help_is_shown.reset();
        }
        self.accessible_view_supports_navigation.set(Dynamic::from(provider_supports_navigation(provider)));
        self.accessible_view_verbosity_enabled.set(Dynamic::from(self.verbosity_enabled()));
        let go_to_symbol_supported = self.go_to_symbols_supported() && self.get_symbols().is_some_and(|symbols| !symbols.is_empty());
        self.accessible_view_go_to_symbol_supported.set(Dynamic::from(go_to_symbol_supported));
    }

    /// `_getStableUri(providerId)`.
    fn get_stable_uri(provider_id: &str) -> URI {
        URI::from(
            &UriComponents {
                scheme: Some(JsString::from(je_base::network::schemas::ACCESSIBLE_VIEW)),
                path: Some(JsString::from(format!("accessible-view-{provider_id}"))),
                ..UriComponents::default()
            },
            false,
        )
    }

    /// `_updateContent(provider, updatedContent?)`.
    fn update_content(&self, provider: &AccessibleViewContentProvider, updated_content: Option<String>) {
        let mut content = updated_content.unwrap_or_else(|| provider_provide_content(provider));
        if provider_options(provider).view_type == AccessibleViewType::View {
            *self.current_content.borrow_mut() = Some(content);
            self.has_unassigned_keybindings.reset();
            self.has_assigned_keybindings.reset();
            return;
        }
        let read_more_link_hint = self.read_more_hint(provider);
        let disable_help_hint = self.disable_verbosity_hint(provider);
        let screen_reader_mode_hint = self.screen_reader_mode_hint(provider);
        let exit_this_dialog_hint = self.exit_dialog_hint(provider);
        let mut configure_kb_hint = String::new();
        let mut configure_assigned_kb_hint = String::new();
        let resolved_content =
            resolve_content_and_keybinding_items(&*self.services.keybinding_service, Some(&format!("{screen_reader_mode_hint}{content}{read_more_link_hint}{disable_help_hint}{exit_this_dialog_hint}")));
        if let Some(resolved_content) = resolved_content {
            content = resolved_content.content;
            if let Some(items) = resolved_content.configure_keybinding_items {
                *self.configure_keybinding_items.borrow_mut() = Some(items);
                self.has_unassigned_keybindings.set(Dynamic::from(true));
                configure_kb_hint = self.configure_unassigned_kb_hint();
            } else {
                self.has_assigned_keybindings.reset();
            }
            if let Some(items) = resolved_content.configured_keybinding_items {
                *self.configured_keybinding_items.borrow_mut() = Some(items);
                self.has_assigned_keybindings.set(Dynamic::from(true));
                configure_assigned_kb_hint = self.configure_assigned_kb_hint();
            } else {
                self.has_assigned_keybindings.reset();
            }
        }
        *self.current_content.borrow_mut() = Some(format!("{content}{configure_kb_hint}{configure_assigned_kb_hint}"));
    }

    /// `_render(provider, container, showAccessibleViewHelp?, updatedContent?)`.
    fn render(self: &Rc<Self>, provider: &AccessibleViewContentProvider, container: &Rc<ViewElement>, show_accessible_view_help: bool, updated_content: Option<String>) {
        let editor = self.services.editor.clone();
        let id = provider_id(provider);
        let is_same_provider = self.current_provider().is_some_and(|current| provider_id(&current) == id);
        let previous_position = if is_same_provider { editor.get_position() } else { None };
        let previous_scroll_top = if is_same_provider { Some(editor.get_scroll_top()) } else { None };
        *self.current_provider.borrow_mut() = Some(provider.clone());
        self.accessible_view_current_provider_id.set(Dynamic::from(id.as_str()));
        let verbose = self.verbosity_enabled();
        self.update_content(provider, updated_content);
        let current_content = self.current_content.borrow().clone();
        self.calculate_code_blocks(current_content.as_deref());
        self.update_context_keys(provider, true);
        let widget_is_focused = editor.has_text_focus() || editor.has_widget_focus();
        let stable_uri = Self::get_stable_uri(&id);
        let view = self.clone();
        let rendered_provider = provider.clone();
        let rendered_container = container.clone();
        self.get_text_model(&stable_uri).then(move |model| {
            let Some(model) = model else { return };
            let provider = rendered_provider;
            let options = provider_options(&provider).clone();
            let editor = view.services.editor.clone();
            // Update the content of the existing model instead of creating a new one
            // This preserves the cursor position when content changes
            let current_content = view.current_content.borrow().clone().unwrap_or_default();
            if model.get_value().to_string_lossy() != current_content {
                model.set_value(TextSource::from(current_content.as_str()));
            }
            if !editor.get_model().is_some_and(|current| Rc::ptr_eq(&current, &model)) {
                editor.set_model(Some(model.clone()));
            }
            model.set_language(options.language.as_deref().unwrap_or("markdown"), None);
            view.container.set_parent(Some(rendered_container.clone()));
            let mut actions_hint = String::new();
            let has_actions = view.accessible_view_supports_navigation.get().is_truthy()
                || view.accessible_view_verbosity_enabled.get().is_truthy()
                || view.accessible_view_go_to_symbol_supported.get().is_truthy()
                || has_provided_actions(&provider);
            if verbose && !show_accessible_view_help && has_actions {
                actions_hint = if options.position.is_some() {
                    localize("ariaAccessibleViewActionsBottom", "Explore actions such as disabling this hint (Shift+Tab), use Escape to exit this dialog.", &[])
                } else {
                    localize("ariaAccessibleViewActions", "Explore actions such as disabling this hint (Shift+Tab).", &[])
                };
            }
            let mut aria_label = if options.view_type == AccessibleViewType::Help { localize("accessibility-help", "Accessibility Help", &[]) } else { localize("accessible-view", "Accessible View", &[]) };
            *view.title.borrow_mut() = aria_label.clone();
            if !actions_hint.is_empty() && options.view_type == AccessibleViewType::View {
                aria_label = localize("accessible-view-hint", "Accessible View, {0}", &[actions_hint.as_str()]);
            } else if !actions_hint.is_empty() {
                aria_label = localize("accessibility-help-hint", "Accessibility Help, {0}", &[actions_hint.as_str()]);
            }
            if cfg!(windows) && widget_is_focused {
                // prevent the screen reader on windows from reading
                // the aria label again when it's refocused
                aria_label = String::new();
            }
            editor.update_options(Some(&Dynamic::object([("ariaLabel", Dynamic::from(aria_label))])));
            editor.focus();
            let saved_position = view.last_provider_position.borrow().get(&provider_id(&provider)).copied();
            let line_count = editor.get_model().map(|model| model.get_line_count());
            if let Some(position_option) = options.position {
                let current_position = editor.get_position();
                let is_default_position = current_position.is_some_and(|position| position.line_number == 1 && position.column == 1);
                let preserved_position = match position_option {
                    AccessibleViewPosition::InitialBottomPreserve => previous_position.or(saved_position),
                    AccessibleViewPosition::InitialBottom if !is_same_provider => saved_position,
                    _ => None,
                };
                match preserved_position.filter(|position| position.line_number <= line_count.unwrap_or(0)) {
                    Some(preserved_position) => {
                        editor.set_position(preserved_position, None);
                        // When always preserving the cursor position, keep the current scroll
                        // position on content updates instead of revealing the cursor, which
                        // would cause the view to jump while the user is scrolling.
                        match previous_scroll_top {
                            Some(scroll_top) if position_option == AccessibleViewPosition::InitialBottomPreserve => editor.set_scroll_top(scroll_top, None),
                            _ => editor.reveal_line(preserved_position.line_number, None),
                        }
                    }
                    None => {
                        if matches!(position_option, AccessibleViewPosition::Bottom | AccessibleViewPosition::InitialBottomPreserve)
                            || (position_option == AccessibleViewPosition::InitialBottom && is_default_position)
                        {
                            if let Some(last_line) = line_count.filter(|last_line| *last_line > 0) {
                                let position = Position::new(last_line, 1);
                                editor.set_position(position, None);
                                editor.reveal_line(position.line_number, None);
                            }
                        }
                    }
                }
            } else if let Some(previous_position) = previous_position {
                editor.set_position(previous_position, None);
            } else if let Some(saved_position) = saved_position {
                // Restore the saved position for this provider if available (e.g., after close and reopen)
                // Only restore if the saved position is still valid within the current content
                if saved_position.line_number <= line_count.unwrap_or(0) {
                    editor.set_position(saved_position, None);
                    editor.reveal_position(saved_position, None);
                }
            }
        });
        self.update_toolbar(provider_actions(provider), Some(provider_options(provider).view_type));

        let disposable_store = DisposableStore::new();
        self.render_disposables.set(Some(disposable_store.clone()));
        let hiding_provider = provider.clone();
        disposable_store.add(editor.on_key_down().listen({
            let view = Rc::downgrade(self);
            let provider = provider.clone();
            move |event| {
                let Some(view) = view.upgrade() else { return };
                if event.key_code == KeyCode::Enter {
                    let _ = view.services.command_service.execute_command("editor.action.openLink", Vec::new());
                } else if event.key_code == KeyCode::Escape || should_hide(event, &*view.services.keybinding_service, &*view.services.configuration_service) {
                    view.hide(&provider);
                } else if event.key_code == KeyCode::KeyH {
                    if let Some(url) = provider_options(&provider).read_more_url.clone() {
                        view.services.accessibility_service.alert(&localize("openingDocs", "Opening the Accessibility documentation page.", &[]));
                        let _ = view.services.opener_service.open(&OpenResource::Uri(URI::parse(url.as_str(), false)), None);
                    }
                }
                if let AccessibleViewContentProvider::Content(content) = &provider {
                    if let Some(on_key_down) = &content.on_key_down {
                        on_key_down(event);
                    }
                }
            }
        }));
        disposable_store.add(self.services.toolbar.on_key_down().listen({
            let view = Rc::downgrade(self);
            move |event| {
                if event.key_code == KeyCode::Escape {
                    if let Some(view) = view.upgrade() {
                        view.hide(&hiding_provider);
                    }
                }
            }
        }));
        disposable_store.add(editor.on_did_blur_editor_widget().listen({
            let view = Rc::downgrade(self);
            let provider = provider.clone();
            move |_| {
                let Some(view) = view.upgrade() else { return };
                if !view.services.toolbar.is_focused() {
                    view.hide(&provider);
                }
            }
        }));
        disposable_store.add(editor.on_did_content_size_change().listen({
            let view = Rc::downgrade(self);
            move |_| {
                if let Some(view) = view.upgrade() {
                    view.layout();
                }
            }
        }));
        let view = Rc::downgrade(self);
        disposable_store.add(self.services.layout_service.on_did_layout_active_container().listen(move |_| {
            if let Some(view) = view.upgrade() {
                view.layout();
            }
        }));
    }

    /// O `hide` de `_render`: fecha a visão, salvando a posição do cursor.
    fn hide(self: &Rc<Self>, provider: &AccessibleViewContentProvider) {
        if !self.is_in_quick_pick.get() {
            provider_on_close(provider);
        }
        self.services.context_view_service.hide_context_view(None);
        if self.is_in_quick_pick.get() {
            return;
        }
        self.update_context_keys(provider, false);
        // Save the cursor position for this provider so it can be restored on reopen
        if let Some(current_position) = self.services.editor.get_position() {
            self.last_provider_position.borrow_mut().insert(provider_id(provider), current_position);
        }
        self.set_last_provider(None);
        *self.current_content.borrow_mut() = None;
        let current = self.current_provider.borrow_mut().take();
        if let Some(current) = current {
            provider_dispose(&current);
        }
    }

    /// `_updateToolbar(providedActions?, type?)`.
    pub fn update_toolbar(&self, provided_actions: Option<Vec<ActionRef>>, view_type: Option<AccessibleViewType>) {
        self.services.toolbar.set_aria_label(&if view_type == Some(AccessibleViewType::Help) {
            localize("accessibleHelpToolbar", "Accessibility Help", &[])
        } else {
            localize("accessibleViewToolbar", "Accessible View", &[])
        });
        let toolbar_menu = self.services.menu_service.create_menu(&accessible_view_menu_id(), self.services.context_key_service.clone(), None);
        self.toolbar_menu.set(Some(toolbar_menu.clone()));
        let menu_actions = get_flat_context_menu_actions(&toolbar_menu.get_actions(Some(&MenuActionOptions::default())), None);
        match provided_actions {
            Some(provided_actions) => {
                let primitive_square = ThemeIcon { id: CODICON_PRIMITIVE_SQUARE.to_owned(), color: None }.as_class_name();
                for provided_action in &provided_actions {
                    if provided_action.class().is_none_or(|class| class.is_empty()) {
                        provided_action.set_class(Some(primitive_square.clone()));
                    }
                    provided_action.set_checked(None);
                }
                self.services.toolbar.set_actions(provided_actions.into_iter().chain(menu_actions).collect());
            }
            None => self.services.toolbar.set_actions(menu_actions),
        }
    }

    /// `_layout()`.
    fn layout(&self) {
        let dimension = self.services.layout_service.active_container_dimension();
        let max_height = if dimension.height != 0.0 { dimension.height * MAX_HEIGHT_RATIO } else { dimension.height };
        let height = max_height.min(self.services.editor.get_content_height());
        let width = (dimension.width * WIDTH_RATIO).min(MAX_WIDTH);
        self.container.set_size(width, height);
    }

    /// `_getTextModel(resource)`.
    fn get_text_model(&self, resource: &URI) -> Promise<Option<Rc<TextModel>>> {
        if let Some(existing) = self.services.model_service.get_model(resource) {
            if !existing.is_disposed() {
                return Promise::resolved(Some(existing));
            }
        }
        // Create an empty model - content will be set via setValue() to preserve cursor position
        Promise::resolved(Some(self.services.model_service.create_model(TextSource::from(""), None, Some(resource), false)))
    }

    /// `_goToSymbolsSupported()`.
    fn go_to_symbols_supported(&self) -> bool {
        let Some(current) = self.current_provider() else { return false };
        let options = provider_options(&current);
        options.view_type == AccessibleViewType::Help
            || options.language.as_deref() == Some("markdown")
            || options.language.is_none()
            || matches!(&current, AccessibleViewContentProvider::Content(content) if content.get_symbols.is_some())
    }

    /// `_updateLastProvider()`.
    pub fn update_last_provider(&self) -> Option<AccessibleViewContentProvider> {
        let provider = self.current_provider()?;
        Some(match &provider {
            AccessibleViewContentProvider::Content(content) => AccessibleViewContentProvider::Content(AccessibleContentProvider::new(AccessibleContentProviderArgs {
                id: content.id,
                options: content.options.clone(),
                provide_content: content.provide_content.clone(),
                on_close: content.on_close.clone(),
                verbosity_setting_key: content.verbosity_setting_key.clone(),
                on_open: content.on_open.clone(),
                actions: content.actions(),
                provide_next_content: content.provide_next_content.clone(),
                provide_previous_content: content.provide_previous_content.clone(),
                on_did_change_content: content.on_did_change_content.clone(),
                on_key_down: content.on_key_down.clone(),
                get_symbols: content.get_symbols.clone(),
                on_did_request_clear_last_provider: content.on_did_request_clear_last_provider.clone(),
            })),
            AccessibleViewContentProvider::Extension(extension) => AccessibleViewContentProvider::Extension(ExtensionContentProvider::new(ExtensionContentProviderArgs {
                id: extension.id.clone(),
                options: extension.options.clone(),
                provide_content: extension.provide_content.clone(),
                on_close: extension.on_close.clone(),
                on_open: extension.on_open.clone(),
                provide_next_content: extension.provide_next_content.clone(),
                provide_previous_content: extension.provide_previous_content.clone(),
                actions: extension.actions.clone(),
                on_did_change_content: extension.on_did_change_content.clone(),
            })),
        })
    }

    /// `showAccessibleViewHelp()`.
    pub fn show_accessible_view_help(self: &Rc<Self>) {
        let Some(last_provider) = self.update_last_provider() else { return };
        // The provider can request to be cleared (e.g. its terminal was killed) while help is open
        let clear_requested = Rc::new(Cell::new(false));
        let help_clear_listener = match &last_provider {
            AccessibleViewContentProvider::Content(content) => content.on_did_request_clear_last_provider.as_ref().map(|event| {
                let clear_requested = clear_requested.clone();
                let last_provider_id = content.options.id;
                event.listen(move |id| {
                    if last_provider_id == Some(*id) {
                        clear_requested.set(true);
                    }
                })
            }),
            AccessibleViewContentProvider::Extension(_) => None,
        };
        self.help_clear_listener.set(help_clear_listener);
        let restore_last_provider: Rc<dyn Fn()> = {
            let view = Rc::downgrade(self);
            let last_provider = last_provider.clone();
            Rc::new(move || {
                let Some(view) = view.upgrade() else { return };
                view.help_clear_listener.clear();
                view.services.context_view_service.hide_context_view(None);
                if clear_requested.get() {
                    provider_dispose(&last_provider);
                    return;
                }
                // HACK: Delay to allow the context view to hide #207638
                let last_provider = last_provider.clone();
                queue_microtask(move || view.show(Some(last_provider), None, false, None));
            })
        };
        let custom_help = provider_options(&last_provider).custom_help.clone();
        let provide_help_content: Rc<dyn Fn() -> String> = {
            let view = Rc::downgrade(self);
            Rc::new(move || match &custom_help {
                Some(custom_help) => custom_help(),
                None => view.upgrade().map(|view| view.accessible_view_help_dialog_content(view.go_to_symbols_supported())).unwrap_or_default(),
            })
        };
        let help_provider = match &last_provider {
            AccessibleViewContentProvider::Content(content) => AccessibleViewContentProvider::Content(AccessibleContentProvider::new(AccessibleContentProviderArgs::new(
                content.id,
                IAccessibleViewOptions::new(AccessibleViewType::Help),
                provide_help_content,
                restore_last_provider,
                &content.verbosity_setting_key,
            ))),
            AccessibleViewContentProvider::Extension(extension) => AccessibleViewContentProvider::Extension(ExtensionContentProvider::new(ExtensionContentProviderArgs {
                id: extension.id.clone(),
                options: IAccessibleViewOptions::new(AccessibleViewType::Help),
                provide_content: provide_help_content,
                on_close: restore_last_provider,
                on_open: None,
                provide_next_content: None,
                provide_previous_content: None,
                actions: None,
                on_did_change_content: None,
            })),
        };
        self.services.context_view_service.hide_context_view(None);
        // HACK: Delay to allow the context view to hide #186514
        let view = self.clone();
        queue_microtask(move || view.show(Some(help_provider), None, true, None));
    }

    /// `_accessibleViewHelpDialogContent(providerHasSymbols?)`.
    fn accessible_view_help_dialog_content(&self, provider_has_symbols: bool) -> String {
        let navigation_hint = self.navigation_hint();
        let go_to_symbol_hint = self.go_to_symbol_hint(provider_has_symbols);
        let toolbar_hint = localize("toolbar", "Navigate to the toolbar (Shift+Tab).", &[]);
        let chat_hints = self.get_chat_hints();

        let mut hint = localize("intro", "In the accessible view, you can:\n", &[]);
        if !navigation_hint.is_empty() {
            hint.push_str(&format!(" - {navigation_hint}\n"));
        }
        if let Some(go_to_symbol_hint) = go_to_symbol_hint {
            hint.push_str(&format!(" - {go_to_symbol_hint}\n"));
        }
        if !toolbar_hint.is_empty() {
            hint.push_str(&format!(" - {toolbar_hint}\n"));
        }
        if self.current_provider().is_some_and(|provider| provider_id(&provider) == AccessibleViewProviderId::Notification.as_str()) {
            hint.push_str(&format!(
                " - {}\n",
                localize("notificationLinks", "If a notification contains links, they are available in the toolbar. Use Left and Right Arrow to find a link and Enter to open it.", &[])
            ));
        }
        if let Some(chat_hints) = chat_hints {
            hint.push_str(&chat_hints);
        }
        hint
    }

    /// `_getChatHints()`.
    fn get_chat_hints(&self) -> Option<String> {
        let current = self.current_provider()?;
        if !is_chat_provider_id(&provider_id(&current)) {
            return None;
        }
        Some(
            [
                localize("insertAtCursor", " - Insert the code block at the cursor{0}.", &["<keybinding:workbench.action.chat.insertCodeBlock>"]),
                localize("insertIntoNewFile", " - Insert the code block into a new file{0}.", &["<keybinding:workbench.action.chat.insertIntoNewFile>"]),
                localize("runInTerminal", " - Run the code block in the terminal{0}.\n", &["<keybinding:workbench.action.chat.runInTerminal>"]),
            ]
            .join("\n"),
        )
    }

    fn navigation_hint(&self) -> String {
        localize(
            "accessibleViewNextPreviousHint",
            "Show the next item{0} or previous item{1}.",
            &[&format!("<keybinding:{}>", accessibility_command_id::SHOW_NEXT), &format!("<keybinding:{}>", accessibility_command_id::SHOW_PREVIOUS)],
        )
    }

    fn disable_verbosity_hint(&self, provider: &AccessibleViewContentProvider) -> String {
        if provider_options(provider).view_type == AccessibleViewType::Help && self.verbosity_enabled() {
            return localize("acessibleViewDisableHint", "\nDisable accessibility verbosity for this feature{0}.", &[&format!("<keybinding:{}>", accessibility_command_id::DISABLE_VERBOSITY_HINT)]);
        }
        String::new()
    }

    fn go_to_symbol_hint(&self, provider_has_symbols: bool) -> Option<String> {
        if !provider_has_symbols {
            return None;
        }
        Some(localize("goToSymbolHint", "Go to a symbol{0}.", &[&format!("<keybinding:{}>", accessibility_command_id::GO_TO_SYMBOL)]))
    }

    /// O `lookupKeybinding(commandId)?.getAriaLabel()`.
    fn aria_label_of(&self, command_id: &str) -> Option<String> {
        self.services.keybinding_service.lookup_keybinding(command_id, None, false).and_then(|keybinding| keybinding.get_aria_label()).filter(|label| !label.is_empty())
    }

    fn configure_unassigned_kb_hint(&self) -> String {
        let keybinding_to_configure_quick_pick = match self.aria_label_of(accessibility_command_id::ACCESSIBILITY_HELP_CONFIGURE_KEYBINDINGS) {
            Some(configure_kb) => format!("({configure_kb})"),
            None => "by assigning a keybinding to the command Accessibility Help Configure Unassigned Keybindings.".to_owned(),
        };
        localize("configureKb", "\nConfigure keybindings for commands that lack them {0}.", &[keybinding_to_configure_quick_pick.as_str()])
    }

    fn configure_assigned_kb_hint(&self) -> String {
        let keybinding_to_configure_quick_pick = match self.aria_label_of(accessibility_command_id::ACCESSIBILITY_HELP_CONFIGURE_ASSIGNED_KEYBINDINGS) {
            Some(configure_kb) => format!("({configure_kb})"),
            None => "by assigning a keybinding to the command Accessibility Help Configure Assigned Keybindings.".to_owned(),
        };
        localize("configureKbAssigned", "\nConfigure keybindings for commands that already have assignments {0}.", &[keybinding_to_configure_quick_pick.as_str()])
    }

    fn screen_reader_mode_hint(&self, provider: &AccessibleViewContentProvider) -> String {
        let accessibility_support = self.services.accessibility_service.is_screen_reader_optimized();
        let turn_on_message = if cfg!(target_os = "macos") {
            localize("changeConfigToOnMac", "Configure the application to be optimized for usage with a Screen Reader (Command+E).", &[])
        } else {
            localize("changeConfigToOnWinLinux", "Configure the application to be optimized for usage with a Screen Reader (Control+E).", &[])
        };
        let mut screen_reader_mode_hint = String::new();
        if accessibility_support && provider_id(provider) == AccessibleViewProviderId::Editor.as_str() {
            screen_reader_mode_hint = localize("auto_on", "The application is configured to be optimized for usage with a Screen Reader.", &[]);
            screen_reader_mode_hint.push('\n');
        } else if !accessibility_support {
            screen_reader_mode_hint = format!("{}\n{turn_on_message}", localize("auto_off", "The application is configured to never be optimized for usage with a Screen Reader.", &[]));
            screen_reader_mode_hint.push('\n');
        }
        screen_reader_mode_hint
    }

    fn exit_dialog_hint(&self, provider: &AccessibleViewContentProvider) -> String {
        if self.verbosity_enabled() && provider_options(provider).position.is_none() { localize("exit", "\nExit this dialog (Escape).", &[]) } else { String::new() }
    }

    fn read_more_hint(&self, provider: &AccessibleViewContentProvider) -> String {
        if provider_options(provider).read_more_url.is_some() {
            localize(
                "openDoc",
                "\nOpen a browser window with more information related to accessibility{0}.",
                &[&format!("<keybinding:{}>", accessibility_command_id::ACCESSIBILITY_HELP_OPEN_HELP_LINK)],
            )
        } else {
            String::new()
        }
    }
}

impl IDisposable for AccessibleView {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O objeto literal `delegate: IContextViewDelegate` de `show`.
struct AccessibleViewDelegate {
    view: Weak<AccessibleView>,
    provider: AccessibleViewContentProvider,
    show_accessible_view_help: bool,
    show_disposables: Rc<DisposableStore>,
}

impl IContextViewDelegate for AccessibleViewDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        let Some(view) = self.view.upgrade() else {
            return AnchorTarget::Anchor(IAnchor { x: 0.0, y: 0.0, width: None, height: None });
        };
        let dimension = view.services.layout_service.active_container_dimension();
        let width = (dimension.width * WIDTH_RATIO).min(MAX_WIDTH);
        AnchorTarget::Anchor(IAnchor {
            x: (dimension.width / 2.0) - (width / 2.0),
            y: view.services.layout_service.active_container_offset().quick_pick_top,
            width: None,
            height: None,
        })
    }

    fn render(&self, container: &Rc<ViewElement>) -> Option<DisposableRef> {
        let view = self.view.upgrade()?;
        *view.view_container.borrow_mut() = Some(container.clone());
        container.class_list_add("accessible-view-container");
        view.render(&self.provider, container, self.show_accessible_view_help, None);
        let render_disposables = view.render_disposables.clone();
        Some(to_disposable(move || render_disposables.clear()))
    }

    fn on_hide(&self, _data: Option<HideData>) {
        self.show_disposables.dispose();
        let Some(view) = self.view.upgrade() else { return };
        view.toolbar_menu.clear();
        if !self.show_accessible_view_help {
            // Save cursor position before disposing so it can be restored on reopen
            let current = view.current_provider.borrow_mut().take();
            if let Some(current) = current {
                if let Some(current_position) = view.services.editor.get_position() {
                    view.last_provider_position.borrow_mut().insert(provider_id(&current), current_position);
                }
                provider_dispose(&current);
            }
            view.reset_context_keys();
        }
    }
}

/// `{ getCodeBlockContext: () => this.getCodeBlockContext() }`.
struct AccessibleViewCodeBlockProvider {
    view: Weak<AccessibleView>,
}

impl ICodeBlockContextProvider for AccessibleViewCodeBlockProvider {
    fn get_code_block_context(&self) -> Option<ICodeBlockActionContext> {
        self.view.upgrade().and_then(|view| view.get_code_block_context())
    }
}

/// `AccessibleViewService`.
pub struct AccessibleViewService {
    base: Rc<Disposable>,
    accessible_view: RefCell<Option<Rc<AccessibleView>>>,
    /// `this._instantiationService.createInstance(AccessibleView)`.
    create_accessible_view: Rc<dyn Fn() -> Rc<AccessibleView>>,
    configuration_service: Rc<dyn IConfigurationService>,
    keybinding_service: Rc<dyn IKeybindingService>,
}

impl AccessibleViewService {
    pub fn new(create_accessible_view: Rc<dyn Fn() -> Rc<AccessibleView>>, configuration_service: Rc<dyn IConfigurationService>, keybinding_service: Rc<dyn IKeybindingService>) -> Rc<Self> {
        Rc::new(Self { base: Disposable::new(), accessible_view: RefCell::new(None), create_accessible_view, configuration_service, keybinding_service })
    }

    fn view(&self) -> Option<Rc<AccessibleView>> {
        self.accessible_view.borrow().clone()
    }
}

impl IDisposable for AccessibleViewService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IAccessibleViewService for AccessibleViewService {
    fn show(&self, provider: AccessibleViewContentProvider, position: Option<IPosition>) {
        let view = match self.view() {
            Some(view) => view,
            None => {
                let view = (self.create_accessible_view)();
                self.base.register(view.clone());
                *self.accessible_view.borrow_mut() = Some(view.clone());
                view
            }
        };
        view.show(Some(provider), None, false, position);
    }

    fn configure_keybindings(&self, unassigned: bool) {
        if let Some(view) = self.view() {
            view.configure_keybindings(unassigned);
        }
    }

    fn open_help_link(&self) {
        if let Some(view) = self.view() {
            view.open_help_link();
        }
    }

    fn show_last_provider(&self, id: AccessibleViewProviderId) {
        if let Some(view) = self.view() {
            view.show_last_provider(id);
        }
    }

    fn next(&self) {
        if let Some(view) = self.view() {
            view.next();
        }
    }

    fn previous(&self) {
        if let Some(view) = self.view() {
            view.previous();
        }
    }

    fn go_to_symbol(&self) {
        if let Some(view) = self.view() {
            view.go_to_symbol();
        }
    }

    fn get_open_aria_hint(&self, verbosity_setting_key: &str) -> Option<String> {
        if !self.configuration_service.get_value(Some(verbosity_setting_key), None).is_truthy() {
            return None;
        }
        let keybinding = self
            .keybinding_service
            .lookup_keybinding(accessibility_command_id::OPEN_ACCESSIBLE_VIEW, None, false)
            .and_then(|keybinding| keybinding.get_aria_label())
            .filter(|label| !label.is_empty());
        Some(match keybinding {
            Some(keybinding) => localize("acessibleViewHint", "Inspect this in the accessible view with {0}", &[keybinding.as_str()]),
            None => localize("acessibleViewHintNoKbEither", "Inspect this in the accessible view via the command Open Accessible View which is currently not triggerable via keybinding.", &[]),
        })
    }

    fn disable_hint(&self) {
        if let Some(view) = self.view() {
            view.disable_hint();
        }
    }

    fn show_accessible_view_help(&self) {
        if let Some(view) = self.view() {
            view.show_accessible_view_help();
        }
    }

    fn get_position(&self, id: AccessibleViewProviderId) -> Option<IPosition> {
        self.view().and_then(|view| view.get_position(Some(id))).map(to_platform_position)
    }

    fn get_last_position(&self) -> Option<IPosition> {
        let view = self.view()?;
        let last_line = view.editor_widget().get_model().map(|model| model.get_line_count())?;
        (last_line > 0).then(|| to_platform_position(Position::new(last_line, 1)))
    }

    fn set_position(&self, position: IPosition, reveal: Option<bool>, select: Option<bool>) {
        if let Some(view) = self.view() {
            view.set_position(to_editor_position(position), reveal.unwrap_or(false), select.unwrap_or(false));
        }
    }

    fn get_code_block_context(&self) -> Option<ICodeBlockActionContext> {
        self.view().and_then(|view| view.get_code_block_context())
    }

    fn navigate_to_code_block(&self, navigation: NavigationType) {
        if let Some(view) = self.view() {
            view.navigate_to_code_block(navigation);
        }
    }
}

/// `AccessibleViewSymbolQuickPick`.
struct AccessibleViewSymbolQuickPick {
    accessible_view: Rc<AccessibleView>,
    quick_input_service: Rc<dyn IQuickInputService>,
}

impl AccessibleViewSymbolQuickPick {
    fn show(&self, provider: &AccessibleViewContentProvider) {
        let disposables = DisposableStore::new();
        let quick_pick: Rc<dyn IQuickPick> = self.quick_input_service.create_quick_pick(None);
        disposables.add(quick_pick.clone());
        quick_pick.set_placeholder(Some(localize("accessibleViewSymbolQuickPickPlaceholder", "Type to search symbols", &[])));
        quick_pick.set_title(Some(localize("accessibleViewSymbolQuickPickTitle", "Go to Symbol Accessible View", &[])));
        let Some(symbols) = self.accessible_view.get_symbols() else { return };
        // O item escolhido volta como `IQuickPickItem`: o índice no `id` o liga ao símbolo de origem.
        let picks: Vec<QuickPickInput> = symbols
            .iter()
            .enumerate()
            .map(|(index, symbol)| {
                let mut item = symbol.item.clone();
                item.base.id = Some(index.to_string());
                QuickPickInput::Item(item)
            })
            .collect();
        quick_pick.set_can_select_many(false);
        quick_pick.set_items(picks);
        quick_pick.show();
        disposables.add(quick_pick.on_did_accept().listen({
            let quick_pick = quick_pick.clone();
            let accessible_view = self.accessible_view.clone();
            let provider = provider.clone();
            move |_| {
                let selected_index = quick_pick.selected_items().first().and_then(|item| item.base.id.as_deref().and_then(|id| id.parse::<usize>().ok()));
                if let Some(symbol) = selected_index.and_then(|index| symbols.get(index)) {
                    accessible_view.show_symbol(&provider, symbol);
                }
                quick_pick.hide();
            }
        }));
        disposables.add(quick_pick.on_did_hide().listen({
            let quick_pick = quick_pick.clone();
            let accessible_view = self.accessible_view.clone();
            let provider = provider.clone();
            let disposables = disposables.clone();
            move |_| {
                if quick_pick.selected_items().is_empty() {
                    // this was escaped, so refocus the accessible view
                    accessible_view.show(Some(provider.clone()), None, false, None);
                }
                disposables.dispose();
            }
        }));
    }
}

/// `shouldHide(event, keybindingService, configurationService)`.
fn should_hide(event: &KeyboardEvent, keybinding_service: &dyn IKeybindingService, configuration_service: &dyn IConfigurationService) -> bool {
    if !configuration_service.get_value(Some(ACCESSIBLE_VIEW_CLOSE_ON_KEY_PRESS_SETTING), None).is_truthy() {
        return false;
    }
    let resolve_result = keybinding_service.soft_dispatch(event, None);
    let is_valid_chord = matches!(resolve_result, ResolutionResult::MoreChordsNeeded);
    if keybinding_service.in_chord_mode() || is_valid_chord {
        return false;
    }
    should_handle_key(event) && !event.ctrl_key && !event.alt_key && !event.meta_key && !event.shift_key
}

/// `shouldHandleKey(event)`: `/^(Key[A-Z]|Digit[0-9]|Equal|Comma|Period|Slash|Quote|Backquote|Backslash|Minus|Semicolon|Space|Enter)$/`.
fn should_handle_key(event: &KeyboardEvent) -> bool {
    let Some(code) = event.code.as_deref() else { return false };
    if let Some(letter) = code.strip_prefix("Key") {
        return letter.len() == 1 && letter.chars().all(|character| character.is_ascii_uppercase());
    }
    if let Some(digit) = code.strip_prefix("Digit") {
        return digit.len() == 1 && digit.chars().all(|character| character.is_ascii_digit());
    }
    matches!(code, "Equal" | "Comma" | "Period" | "Slash" | "Quote" | "Backquote" | "Backslash" | "Minus" | "Semicolon" | "Space" | "Enter")
}
