//! Porte de `src/vs/workbench/contrib/terminal/browser/xterm/decorationAddon.ts`: o addon do xterm que desenha as decorações
//! dos comandos e das marcas do buffer (o ícone na margem e a marca na régua), com o tooltip, o menu de contexto e as ações de
//! cada comando.
//!
//! Decisões de tradução:
//! - **`ITerminalAddon`.** O addon recebe o emulador em [`DecorationAddon::activate`]; o emulador é o
//!   [`IDecorationAddonTerminal`]: o contrato sem tela da plataforma com `registerDecoration` e os elementos de decoração de
//!   comando abertos (`terminal.element?.querySelectorAll(DecorationSelector.CommandDecoration)`).
//! - **Injeção.** Os parâmetros decorados do construtor viram [`DecorationAddonServices`], na ordem do original. O
//!   `IContextMenuService`, o `IHoverService` e o `IAccessibilitySignalService` (que no porte só conhece sinais do editor) não
//!   existem em outro lugar do porte, e entram aqui como os contratos mínimos que o addon lê ([`IContextMenuService`],
//!   [`IHoverService`] e [`IDecorationAccessibilitySignalService`]).
//! - **Fora do porte: o chat.** `_resource`, `IChatContextPickService`, `IChatWidgetService`, `IInstantiationService`,
//!   `_createAttachToChatAction` e o `TerminalContext` pertencem à camada de IA e agentes, que o dono tirou do objetivo (recorte
//!   de escopo de 2026-10-23). Sem widgets de chat, `chatIsEnabled` é sempre falso e a ação "Attach To Chat" nunca existe, que é o
//!   que o original faz sem chat; por isso o construtor também não recebe o `resource`.
//! - **`IDecoration`, `HTMLElement` e `dom.addDisposableListener`.** São [`IXtermDecoration`], [`IDecorationElement`] e
//!   [`IDecorationElement::add_disposable_listener`] (ver `xterm_decoration.rs`). O `element.classList` de `_updateClasses`
//!   tira todas as classes antes de pôr as novas (a intenção do laço do original, que percorre a lista viva enquanto remove).
//! - **`IAction`.** As ações são `toAction` ([`to_action`]), com a mesma `tooltip` do original (a de "Go To Recent Directory" é a
//!   do `labelRunRecent`, como lá). O `Separator` é o de `je_base::actions`.
//! - **`async` dos tratadores de clique.** `_getCommandActions` é `async` sem nenhum `await` dentro: aqui devolve a lista, e o
//!   tratador a entrega ao menu numa promessa resolvida (o mesmo salto de microtarefa do original).
//! - **`new Promise<boolean>(r => prompt(...))`.** É `Promise::with_executor`, e as escolhas do aviso resolvem `true` ou `false`.
//! - **`e.includes(gutterIcon)`.** Os itens do quick pick são valores, não objetos compartilhados: a identidade é o rótulo, que
//!   é único entre os dois itens.
//! - **`throw new Error(...)`.** É o `Err` de [`Error`] do `registerCommandDecoration` e de `registerMarkDecoration`; nos
//!   tratadores de evento, o erro vai para `on_unexpected_error`, como o JavaScript faz com uma exceção num ouvinte. O texto do
//!   `JSON.stringify(command)` traz só `command` e `timestamp` do comando.
//! - **`_decorations` (`Map<number, ...>`).** É um `IndexMap` pelo `id` do marcador, e [`DecorationAddon::has_decoration`] é a
//!   leitura que o teste faz por reflexão.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::actions::{ActionRef, Separator, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, on_unexpected_error};
use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, DisposableMap, DisposableRef, DisposableStore, IDisposable, dispose_all, to_disposable};
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_editor::services::clipboard_service::IClipboardService;
use je_editor::services::theme_service::IThemeService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::notification::notification::{
    BasePromptChoice, INotificationService, NotificationMessage, PromptChoice, PromptChoiceKind, Severity,
};
use je_platform::opener::opener::{IOpenerService, OpenResource};
use je_platform::quickinput::quick_input::{IQuickInputService, IQuickItem, IQuickPickItem, QuickPickInput, QuickPickOk};
use je_platform::terminal::capabilities::capabilities::{
    CommandInvalidationReason, ICommandDetectionCapability, IMarkProperties, ITerminalCapabilityStore, ITerminalCommand, TerminalCapability, TerminalCapabilityImpl,
};
use je_platform::terminal::terminal_settings::TerminalSettingId;
use je_platform::commands::commands::ICommandService;

use super::decoration_styles::{
    DecorationSelector, get_terminal_command_decoration_state, get_terminal_decoration_hover_content, terminal_decoration_mark, update_layout,
};
use super::xterm_decoration::{
    DecorationEvent, DecorationEventType, IDecorationElement, IDecorationHost, IDecorationOptions, IDecorationOverviewRulerOptions, IXtermDecoration,
    OverviewRulerPosition,
};
use crate::contrib::terminal::common::terminal_color_registry::{
    TERMINAL_COMMAND_DECORATION_DEFAULT_BACKGROUND_COLOR, TERMINAL_COMMAND_DECORATION_ERROR_BACKGROUND_COLOR, TERMINAL_COMMAND_DECORATION_SUCCESS_BACKGROUND_COLOR,
};

/// O sinal de acessibilidade de comando terminado (`AccessibilitySignal.terminalCommandFailed` e `terminalCommandSucceeded`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalCommandSignal {
    Failed,
    Succeeded,
}

/// `IAccessibilitySignalService`, só com `playSignal` dos sinais de comando do terminal.
pub trait IDecorationAccessibilitySignalService {
    fn play_signal(&self, signal: TerminalCommandSignal);
}

/// O `{ getAnchor, getActions }` de `showContextMenu`.
pub struct ContextMenuDelegate {
    pub get_anchor: Rc<dyn Fn() -> Rc<dyn IDecorationElement>>,
    pub get_actions: Rc<dyn Fn() -> Vec<ActionRef>>,
}

/// `IContextMenuService`, só com `showContextMenu`.
pub trait IContextMenuService {
    fn show_context_menu(&self, delegate: ContextMenuDelegate);
}

/// O `{ content: new MarkdownString(...) }` de `setupDelayedHover`: o conteúdo é o texto em markdown.
pub struct DelayedHoverOptions {
    pub content: String,
}

/// `IHoverService`, só com `setupDelayedHover`.
pub trait IHoverService {
    fn setup_delayed_hover(&self, element: Rc<dyn IDecorationElement>, options: Rc<dyn Fn() -> DelayedHoverOptions>) -> DisposableRef;
}

/// `IDecorationAddon` (`platform/terminal/common/terminal.ts`).
pub trait IDecorationAddon {
    fn register_menu_items(&self, command: Rc<dyn ITerminalCommand>, items: Vec<ActionRef>) -> DisposableRef;
}

/// O `Terminal` do xterm no que o [`DecorationAddon`] lê.
pub trait IDecorationAddonTerminal: IDecorationHost {
    /// `terminal.element?.querySelectorAll(DecorationSelector.CommandDecoration)`: vazio quando o terminal não foi aberto.
    fn command_decoration_elements(&self) -> Vec<Rc<dyn IDecorationElement>>;
}

/// Os serviços que o construtor recebe injetados (ver o cabeçalho).
pub struct DecorationAddonServices {
    pub clipboard_service: Rc<dyn IClipboardService>,
    pub context_menu_service: Rc<dyn IContextMenuService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub theme_service: Rc<dyn IThemeService>,
    pub opener_service: Rc<dyn IOpenerService>,
    pub quick_input_service: Rc<dyn IQuickInputService>,
    pub command_service: Rc<dyn ICommandService>,
    pub accessibility_signal_service: Rc<dyn IDecorationAccessibilitySignalService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub hover_service: Rc<dyn IHoverService>,
}

/// `{ command: ITerminalCommand; noNewLine?: boolean }`.
#[derive(Clone)]
pub struct DecorationRunCommandRequest {
    pub command: Rc<dyn ITerminalCommand>,
    pub no_new_line: Option<bool>,
}

/// `{ command: ITerminalCommand }`.
#[derive(Clone)]
pub struct DecorationCopyAsHtmlRequest {
    pub command: Rc<dyn ITerminalCommand>,
}

/// `IDisposableDecoration`.
struct IDisposableDecoration {
    decoration: Rc<dyn IXtermDecoration>,
    disposables: Vec<DisposableRef>,
    command: Option<Rc<dyn ITerminalCommand>>,
    mark_properties: Option<IMarkProperties>,
}

/// `string | undefined` de uma configuração.
fn config_string(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `!!command.exitCode`.
fn has_failed(exit_code: Option<f64>) -> bool {
    exit_code.is_some_and(|exit_code| exit_code != 0.0 && !exit_code.is_nan())
}

fn same_command(a: &Rc<dyn ITerminalCommand>, b: &Rc<dyn ITerminalCommand>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `{ class: undefined, tooltip, id, label, enabled: true, run }`.
fn make_action(id: &str, label: &str, tooltip: &str, run: impl Fn() -> MaybePromise<Dynamic> + 'static) -> ActionRef {
    to_action(ToActionProps {
        id: id.to_owned(),
        label: label.to_owned(),
        tooltip: Some(tooltip.to_owned()),
        enabled: Some(true),
        checked: None,
        class: None,
        alt: None,
        run: Rc::new(move |_| run()),
    })
}

/// O retorno de uma ação cuja promessa interna não tem valor.
fn finished(promise: Promise<()>) -> MaybePromise<Dynamic> {
    MaybePromise::Promise(promise.then(|()| Dynamic::Undefined))
}

/// O retorno de uma ação sem promessa.
fn undefined() -> MaybePromise<Dynamic> {
    MaybePromise::Value(Dynamic::Undefined)
}

pub struct DecorationAddon {
    base: Rc<Disposable>,
    this: Weak<Self>,
    capabilities: Rc<dyn ITerminalCapabilityStore>,
    services: DecorationAddonServices,

    terminal: RefCell<Option<Rc<dyn IDecorationAddonTerminal>>>,
    capability_disposables: Rc<DisposableMap<TerminalCapability, DisposableStore>>,
    decorations: RefCell<IndexMap<i64, Rc<IDisposableDecoration>>>,
    placeholder_decoration: RefCell<Option<Rc<dyn IXtermDecoration>>>,
    show_gutter_decorations: Cell<bool>,
    show_overview_ruler_decorations: Cell<bool>,
    registered_menu_items: RefCell<Vec<(Rc<dyn ITerminalCommand>, Vec<ActionRef>)>>,

    on_did_request_run_command: Rc<Emitter<DecorationRunCommandRequest>>,
    on_did_request_copy_as_html: Rc<Emitter<DecorationCopyAsHtmlRequest>>,
}

impl DecorationAddon {
    pub fn new(capabilities: Rc<dyn ITerminalCapabilityStore>, services: DecorationAddonServices) -> Rc<Self> {
        let base = Disposable::new();
        let capability_disposables: Rc<DisposableMap<TerminalCapability, DisposableStore>> = DisposableMap::new();
        base.register(capability_disposables.clone());
        let on_did_request_run_command: Rc<Emitter<DecorationRunCommandRequest>> = Emitter::new();
        base.register(on_did_request_run_command.clone());
        let on_did_request_copy_as_html: Rc<Emitter<DecorationCopyAsHtmlRequest>> = Emitter::new();
        base.register(on_did_request_copy_as_html.clone());
        let this = Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            capabilities,
            services,
            terminal: RefCell::new(None),
            capability_disposables,
            decorations: RefCell::new(IndexMap::new()),
            placeholder_decoration: RefCell::new(None),
            show_gutter_decorations: Cell::new(false),
            show_overview_ruler_decorations: Cell::new(false),
            registered_menu_items: RefCell::new(Vec::new()),
            on_did_request_run_command,
            on_did_request_copy_as_html,
        });
        this.register_listeners();
        this
    }

    /// O corpo do construtor do original, depois dos campos.
    fn register_listeners(&self) {
        let weak = self.this.clone();
        self.base.register(to_disposable(move || {
            if let Some(this) = weak.upgrade() {
                this.dispose_resources();
            }
        }));
        let weak = self.this.clone();
        self.base.register(self.services.configuration_service.on_did_change_configuration().listen(move |event| {
            let Some(this) = weak.upgrade() else { return };
            if event.affects_configuration(TerminalSettingId::FontSize.as_str(), None) || event.affects_configuration(TerminalSettingId::LineHeight.as_str(), None) {
                this.refresh_layouts();
            } else if event.affects_configuration("workbench.colorCustomizations", None) {
                this.refresh_styles(true);
            } else if event.affects_configuration(TerminalSettingId::ShellIntegrationDecorationsEnabled.as_str(), None) {
                this.remove_capability_disposables(TerminalCapability::CommandDetection);
                this.update_decoration_visibility();
            }
        }));
        let weak = self.this.clone();
        self.base.register(self.services.theme_service.on_did_color_theme_change().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.refresh_styles(true);
            }
        }));
        self.update_decoration_visibility();
        let weak = self.this.clone();
        self.base.register(self.capabilities.on_did_add_capability().listen(move |capability| {
            if let Some(this) = weak.upgrade() {
                this.create_capability_disposables(capability.id);
            }
        }));
        let weak = self.this.clone();
        self.base.register(self.capabilities.on_did_remove_capability().listen(move |capability| {
            if let Some(this) = weak.upgrade() {
                this.remove_capability_disposables(capability.id);
            }
        }));
    }

    /// `onDidRequestRunCommand`.
    pub fn on_did_request_run_command(&self) -> Event<DecorationRunCommandRequest> {
        self.on_did_request_run_command.event()
    }

    /// `onDidRequestCopyAsHtml`.
    pub fn on_did_request_copy_as_html(&self) -> Event<DecorationCopyAsHtmlRequest> {
        self.on_did_request_copy_as_html.event()
    }

    /// `this._decorations.has(markerId)`: a leitura que o teste faz do mapa privado.
    pub fn has_decoration(&self, marker_id: f64) -> bool {
        self.decorations.borrow().contains_key(&(marker_id as i64))
    }

    /// `this._decorations.get(markerId)?.decoration`: a decoração guardada para o marcador, que o teste compara por identidade.
    pub fn decoration(&self, marker_id: f64) -> Option<Rc<dyn IXtermDecoration>> {
        self.decorations.borrow().get(&(marker_id as i64)).map(|entry| entry.decoration.clone())
    }

    fn terminal(&self) -> Option<Rc<dyn IDecorationAddonTerminal>> {
        self.terminal.borrow().clone()
    }

    fn create_capability_disposables(&self, capability_id: TerminalCapability) {
        let Some(capability) = self.capabilities.get_capability(capability_id) else { return };
        if self.capability_disposables.has(&capability_id) {
            return;
        }
        let store = DisposableStore::new();
        match capability {
            TerminalCapabilityImpl::BufferMarkDetection(capability) => {
                let weak = self.this.clone();
                store.add(capability.on_mark_added().listen(move |mark| {
                    if let Some(this) = weak.upgrade()
                        && let Err(error) = this.register_mark_decoration(mark.clone())
                    {
                        on_unexpected_error(&error);
                    }
                }));
            }
            TerminalCapabilityImpl::CommandDetection(capability) => {
                for disposable in self.get_command_detection_listeners(&*capability) {
                    store.add(disposable);
                }
            }
            _ => {}
        }
        self.capability_disposables.set(capability_id, store, false);
    }

    fn remove_capability_disposables(&self, capability: TerminalCapability) {
        self.capability_disposables.delete_and_dispose(&capability);
    }

    pub fn register_mark_decoration(&self, mark: IMarkProperties) -> Result<Option<Rc<dyn IXtermDecoration>>, Error> {
        if self.terminal().is_none() || (!self.show_gutter_decorations.get() && !self.show_overview_ruler_decorations.get()) {
            return Ok(None);
        }
        if mark.hidden == Some(true) {
            return Ok(None);
        }
        self.register_command_decoration(None, None, Some(mark))
    }

    fn update_decoration_visibility(&self) {
        let show_decorations = config_string(
            &self.services.configuration_service.get_value(Some(TerminalSettingId::ShellIntegrationDecorationsEnabled.as_str()), None),
        );
        let show_decorations = show_decorations.as_deref();
        self.show_gutter_decorations.set(matches!(show_decorations, Some("both" | "gutter")));
        self.show_overview_ruler_decorations.set(matches!(show_decorations, Some("both" | "overviewRuler")));
        self.dispose_all_decorations();
        if self.show_gutter_decorations.get() || self.show_overview_ruler_decorations.get() {
            self.attach_to_command_capability();
            self.update_gutter_decoration_visibility();
        }
        let current_command = self.command_capability().and_then(|capability| capability.executing_command_object());
        if let Some(current_command) = current_command
            && let Err(error) = self.register_command_decoration(Some(current_command), Some(true), None)
        {
            on_unexpected_error(&error);
        }
    }

    fn command_capability(&self) -> Option<Rc<dyn ICommandDetectionCapability>> {
        match self.capabilities.get_capability(TerminalCapability::CommandDetection) {
            Some(TerminalCapabilityImpl::CommandDetection(capability)) => Some(capability),
            _ => None,
        }
    }

    fn dispose_all_decorations(&self) {
        let placeholder = self.placeholder_decoration.borrow().clone();
        if let Some(placeholder) = placeholder {
            placeholder.dispose();
        }
        let values: Vec<Rc<IDisposableDecoration>> = self.decorations.borrow().values().cloned().collect();
        for value in values {
            value.decoration.dispose();
            dispose_all(value.disposables.iter());
        }
    }

    fn update_gutter_decoration_visibility(&self) {
        let Some(terminal) = self.terminal() else { return };
        for element in terminal.command_decoration_elements() {
            self.update_command_decoration_visibility(&*element);
        }
    }

    fn update_command_decoration_visibility(&self, command_decoration_element: &dyn IDecorationElement) {
        if self.show_gutter_decorations.get() {
            command_decoration_element.class_list_remove(DecorationSelector::Hide.as_str());
        } else {
            command_decoration_element.class_list_add(&[DecorationSelector::Hide.as_str()]);
        }
    }

    pub fn refresh_layouts(&self) {
        let placeholder_element = self.placeholder_decoration.borrow().as_ref().and_then(|placeholder| placeholder.element());
        update_layout(&*self.services.configuration_service, placeholder_element.as_deref());
        let values: Vec<Rc<IDisposableDecoration>> = self.decorations.borrow().values().cloned().collect();
        for decoration in values {
            update_layout(&*self.services.configuration_service, decoration.decoration.element().as_deref());
        }
    }

    fn refresh_styles(&self, refresh_overview_ruler_colors: bool) {
        let values: Vec<Rc<IDisposableDecoration>> = self.decorations.borrow().values().cloned().collect();
        if refresh_overview_ruler_colors {
            for decoration in &values {
                let color = self.get_decoration_css_color(decoration.command.as_deref()).unwrap_or_default();
                decoration.decoration.set_overview_ruler_color(&color);
            }
        }
        let placeholder_element = self.placeholder_decoration.borrow().as_ref().and_then(|placeholder| placeholder.element());
        self.update_classes(placeholder_element.as_deref(), None, None);
        for decoration in &values {
            self.update_classes(decoration.decoration.element().as_deref(), decoration.command.as_ref(), decoration.mark_properties.as_ref());
        }
    }

    fn dispose_resources(&self) {
        for disposable in self.capability_disposables.values() {
            disposable.dispose();
        }
        self.clear_decorations();
    }

    fn clear_placeholder(&self) {
        let placeholder = self.placeholder_decoration.borrow_mut().take();
        if let Some(placeholder) = placeholder {
            placeholder.dispose();
        }
    }

    pub fn clear_decorations(&self) {
        let placeholder = self.placeholder_decoration.borrow().clone();
        if let Some(placeholder) = placeholder {
            placeholder.marker().dispose();
        }
        self.clear_placeholder();
        self.dispose_all_decorations();
        self.decorations.borrow_mut().clear();
    }

    fn attach_to_command_capability(&self) {
        if let Some(capability) = self.command_capability() {
            let disposables = self.get_command_detection_listeners(&*capability);
            let store = DisposableStore::new();
            for disposable in disposables {
                store.add(disposable);
            }
            self.capability_disposables.set(TerminalCapability::CommandDetection, store, false);
        }
    }

    fn get_command_detection_listeners(&self, capability: &dyn ICommandDetectionCapability) -> Vec<DisposableRef> {
        self.remove_capability_disposables(TerminalCapability::CommandDetection);

        let mut command_detection_listeners: Vec<DisposableRef> = Vec::new();
        // Command started
        if let Some(executing_command) = capability.executing_command_object()
            && executing_command.marker().is_some()
            && let Err(error) = self.register_command_decoration(Some(executing_command), Some(true), None)
        {
            on_unexpected_error(&error);
        }
        let weak = self.this.clone();
        command_detection_listeners.push(capability.on_command_started().listen(move |command| {
            if let Some(this) = weak.upgrade()
                && let Err(error) = this.register_command_decoration(Some(command.clone()), Some(true), None)
            {
                on_unexpected_error(&error);
            }
        }));
        // Command finished
        for command in capability.commands() {
            if let Err(error) = self.register_command_decoration(Some(command), None, None) {
                on_unexpected_error(&error);
            }
        }
        let weak = self.this.clone();
        command_detection_listeners.push(capability.on_command_finished().listen(move |command| {
            let Some(this) = weak.upgrade() else { return };
            let buffer = this.terminal().map(|terminal| terminal.buffer_active());
            let marker = command.prompt_start_marker();

            // Edge case: Handle case where tsc watch commands clears buffer, but decoration of that tsc command re-appears
            let should_register_decoration = command.exit_code().is_none()
                // Only register decoration if the cursor is at or below the promptStart marker.
                || matches!((&buffer, &marker), (Some(buffer), Some(marker)) if f64::from(buffer.base_y() + buffer.cursor_y()) >= marker.line());

            if should_register_decoration && let Err(error) = this.register_command_decoration(Some(command.clone()), None, None) {
                on_unexpected_error(&error);
            }

            if has_failed(command.exit_code()) {
                this.services.accessibility_signal_service.play_signal(TerminalCommandSignal::Failed);
            } else {
                this.services.accessibility_signal_service.play_signal(TerminalCommandSignal::Succeeded);
            }
        }));
        // Command invalidated
        let weak = self.this.clone();
        command_detection_listeners.push(capability.on_command_invalidated().listen(move |commands| {
            let Some(this) = weak.upgrade() else { return };
            for command in commands {
                let id = command.marker().map(|marker| marker.id() as i64).filter(|id| *id != 0);
                if let Some(id) = id {
                    let matched = this.decorations.borrow().get(&id).cloned();
                    if let Some(matched) = matched {
                        matched.decoration.dispose();
                        dispose_all(matched.disposables.iter());
                    }
                }
            }
        }));
        // Current command invalidated
        let weak = self.this.clone();
        command_detection_listeners.push(capability.on_current_command_invalidated().listen(move |request| {
            let Some(this) = weak.upgrade() else { return };
            match request.reason {
                CommandInvalidationReason::NoProblemsReported => {
                    let last_decoration = this.decorations.borrow().last().map(|(_, value)| value.clone());
                    if let Some(last_decoration) = last_decoration {
                        last_decoration.decoration.dispose();
                    }
                }
                CommandInvalidationReason::Windows => this.clear_placeholder(),
            }
        }));
        command_detection_listeners
    }

    /// `activate(terminal)`.
    pub fn activate(&self, terminal: Rc<dyn IDecorationAddonTerminal>) {
        *self.terminal.borrow_mut() = Some(terminal);
        self.attach_to_command_capability();
    }

    pub fn register_command_decoration(
        &self,
        command: Option<Rc<dyn ITerminalCommand>>,
        before_command_execution: Option<bool>,
        mark_properties: Option<IMarkProperties>,
    ) -> Result<Option<Rc<dyn IXtermDecoration>>, Error> {
        let before_command_execution = before_command_execution.unwrap_or(false);
        let Some(terminal) = self.terminal() else { return Ok(None) };
        if (before_command_execution && command.is_none()) || (!self.show_gutter_decorations.get() && !self.show_overview_ruler_decorations.get()) {
            return Ok(None);
        }
        let marker = command.as_ref().and_then(|command| command.marker()).or_else(|| mark_properties.as_ref().and_then(|properties| properties.marker.clone()));
        let Some(marker) = marker else {
            return Err(Error::new(format!("cannot add a decoration for a command {} with no marker", describe_command(command.as_deref()))));
        };
        self.clear_placeholder();
        let color = self.get_decoration_css_color(command.as_deref()).unwrap_or_default();
        let overview_ruler_options = self.show_overview_ruler_decorations.get().then(|| {
            if before_command_execution {
                IDecorationOverviewRulerOptions { color: color.clone(), position: Some(OverviewRulerPosition::Left) }
            } else {
                let position = if has_failed(command.as_ref().and_then(|command| command.exit_code())) { OverviewRulerPosition::Right } else { OverviewRulerPosition::Left };
                IDecorationOverviewRulerOptions { color: color.clone(), position: Some(position) }
            }
        });
        let Some(decoration) = terminal.register_decoration(IDecorationOptions { marker, x: None, width: None, overview_ruler_options }) else {
            return Ok(None);
        };
        if before_command_execution {
            *self.placeholder_decoration.borrow_mut() = Some(decoration.clone());
        }
        let weak = self.this.clone();
        let weak_decoration = Rc::downgrade(&decoration);
        // O retorno de `onRender` não é guardado no original: o xterm solta o ouvinte com a decoração.
        let _ = decoration.on_render().listen(move |element| {
            let (Some(this), Some(decoration)) = (weak.upgrade(), weak_decoration.upgrade()) else { return };
            if element.class_list_contains(DecorationSelector::OverviewRuler.as_str()) {
                return;
            }
            let marker_id = decoration.marker().id() as i64;
            if !this.decorations.borrow().contains_key(&marker_id) {
                let weak_for_dispose = this.this.clone();
                let _ = decoration.on_dispose().listen(move |_| {
                    let Some(this) = weak_for_dispose.upgrade() else { return };
                    let disposable_decoration = this.decorations.borrow().get(&marker_id).cloned();
                    if let Some(disposable_decoration) = disposable_decoration {
                        dispose_all(disposable_decoration.disposables.iter());
                        this.decorations.borrow_mut().shift_remove(&marker_id);
                    }
                });
                let entry = IDisposableDecoration {
                    decoration: decoration.clone(),
                    disposables: this.create_disposables(element, command.as_ref(), mark_properties.as_ref()),
                    command: command.clone(),
                    mark_properties: command.as_ref().and_then(|command| command.mark_properties()).or_else(|| mark_properties.clone()),
                };
                this.decorations.borrow_mut().insert(marker_id, Rc::new(entry));
            }
            let command_marker_line = command.as_ref().and_then(|command| command.marker()).map(|marker| marker.line());
            if !element.class_list_contains(DecorationSelector::Codicon.as_str()) || command_marker_line == Some(0.0) {
                // first render or buffer was cleared
                update_layout(&*this.services.configuration_service, Some(&**element));
                let properties = command.as_ref().and_then(|command| command.mark_properties()).or_else(|| mark_properties.clone());
                this.update_classes(Some(&**element), command.as_ref(), properties.as_ref());
            }
        });
        Ok(Some(decoration))
    }

    pub fn register_menu_items(&self, command: Rc<dyn ITerminalCommand>, items: Vec<ActionRef>) -> DisposableRef {
        {
            let mut registered = self.registered_menu_items.borrow_mut();
            match registered.iter_mut().find(|(registered_command, _)| same_command(registered_command, &command)) {
                Some((_, existing_items)) => existing_items.extend(items.iter().cloned()),
                None => registered.push((command.clone(), items.clone())),
            }
        }
        let weak = self.this.clone();
        to_disposable(move || {
            let Some(this) = weak.upgrade() else { return };
            let mut registered = this.registered_menu_items.borrow_mut();
            if let Some((_, command_items)) = registered.iter_mut().find(|(registered_command, _)| same_command(registered_command, &command)) {
                for item in &items {
                    if let Some(index) = command_items.iter().position(|command_item| std::ptr::addr_eq(Rc::as_ptr(command_item), Rc::as_ptr(item))) {
                        command_items.remove(index);
                    }
                }
            }
        })
    }

    fn create_disposables(&self, element: &Rc<dyn IDecorationElement>, command: Option<&Rc<dyn ITerminalCommand>>, mark_properties: Option<&IMarkProperties>) -> Vec<DisposableRef> {
        let command_mark_properties = command.and_then(|command| command.mark_properties());
        let Some(command) = command else { return Vec::new() };
        if command.exit_code().is_none() && command_mark_properties.is_none() {
            Vec::new()
        } else if command_mark_properties.is_some() || mark_properties.is_some() {
            vec![self.create_hover(element, Some(command), mark_properties.and_then(|properties| properties.hover_message.clone()))]
        } else {
            let mut disposables = self.create_context_menu(element, command);
            disposables.push(self.create_hover(element, Some(command), None));
            disposables
        }
    }

    fn create_hover(&self, element: &Rc<dyn IDecorationElement>, command: Option<&Rc<dyn ITerminalCommand>>, hover_message: Option<String>) -> DisposableRef {
        let command = command.cloned();
        self.services.hover_service.setup_delayed_hover(
            element.clone(),
            Rc::new(move || DelayedHoverOptions { content: get_terminal_decoration_hover_content(command.as_deref(), hover_message.as_deref(), Some(true)) }),
        )
    }

    fn update_classes(&self, element: Option<&dyn IDecorationElement>, command: Option<&Rc<dyn ITerminalCommand>>, mark_properties: Option<&IMarkProperties>) {
        let Some(element) = element else { return };
        for class in element.class_list() {
            element.class_list_remove(&class);
        }
        element.class_list_add(&[DecorationSelector::CommandDecoration.as_str(), DecorationSelector::Codicon.as_str(), DecorationSelector::XtermDecoration.as_str()]);

        if let Some(mark_properties) = mark_properties {
            element.class_list_add(&[DecorationSelector::DefaultColor.as_str()]);
            let icon_classes = terminal_decoration_mark().as_class_name_array();
            element.class_list_add(&icon_classes.iter().map(String::as_str).collect::<Vec<_>>());
            if mark_properties.hover_message.as_deref().is_none_or(str::is_empty) {
                //disable the mouse pointer
                element.class_list_add(&[DecorationSelector::Default.as_str()]);
            }
        } else {
            // command decoration
            let state = get_terminal_command_decoration_state(command.map(|command| &**command), None, None);
            self.update_command_decoration_visibility(element);
            for class_name in &state.class_names {
                element.class_list_add(&[class_name.as_str()]);
            }
            let icon_classes = state.icon.as_class_name_array();
            element.class_list_add(&icon_classes.iter().map(String::as_str).collect::<Vec<_>>());
        }
        element.remove_attribute("title");
        element.remove_attribute("aria-label");
    }

    fn create_context_menu(&self, element: &Rc<dyn IDecorationElement>, command: &Rc<dyn ITerminalCommand>) -> Vec<DisposableRef> {
        // When the xterm Decoration gets disposed of, its element gets removed from the dom
        // along with its listeners
        let mut listeners: Vec<DisposableRef> = Vec::new();
        listeners.push(element.add_disposable_listener(
            DecorationEventType::MouseDown,
            Rc::new(|event: &DecorationEvent| {
                event.stop_immediate_propagation();
            }),
        ));

        let (weak, element_for_click, command_for_click) = (self.this.clone(), element.clone(), command.clone());
        listeners.push(element.add_disposable_listener(
            DecorationEventType::Click,
            Rc::new(move |event: &DecorationEvent| {
                event.stop_immediate_propagation();
                let Some(this) = weak.upgrade() else { return };
                let actions = this.get_command_actions(&command_for_click);
                let (weak, element) = (weak.clone(), element_for_click.clone());
                let _ = Promise::resolved(actions).then(move |actions| {
                    if let Some(this) = weak.upgrade() {
                        this.services.context_menu_service.show_context_menu(ContextMenuDelegate {
                            get_anchor: Rc::new({
                                let element = element.clone();
                                move || element.clone()
                            }),
                            get_actions: Rc::new(move || actions.clone()),
                        });
                    }
                });
            }),
        ));

        let (weak, element_for_menu, command_for_menu) = (self.this.clone(), element.clone(), command.clone());
        listeners.push(element.add_disposable_listener(
            DecorationEventType::ContextMenu,
            Rc::new(move |event: &DecorationEvent| {
                event.stop_immediate_propagation();
                let Some(this) = weak.upgrade() else { return };
                let chat_actions = this.get_command_actions(&command_for_menu);
                let (weak, element) = (weak.clone(), element_for_menu.clone());
                let _ = Promise::resolved(chat_actions).then(move |chat_actions| {
                    if let Some(this) = weak.upgrade() {
                        let mut actions = this.get_context_menu_actions();
                        actions.extend(chat_actions);
                        this.services.context_menu_service.show_context_menu(ContextMenuDelegate {
                            get_anchor: Rc::new({
                                let element = element.clone();
                                move || element.clone()
                            }),
                            get_actions: Rc::new(move || actions.clone()),
                        });
                    }
                });
            }),
        ));
        listeners
    }

    fn get_context_menu_actions(&self) -> Vec<ActionRef> {
        let label = localize("workbench.action.terminal.toggleVisibility", "Toggle Visibility", &[]);
        let weak = self.this.clone();
        vec![make_action("terminal.toggleVisibility", &label, &label, move || {
            if let Some(this) = weak.upgrade() {
                this.show_toggle_visibility_quick_pick();
            }
            undefined()
        })]
    }

    fn get_command_actions(&self, command: &Rc<dyn ITerminalCommand>) -> Vec<ActionRef> {
        let mut actions: Vec<ActionRef> = Vec::new();
        let registered_menu_items = self
            .registered_menu_items
            .borrow()
            .iter()
            .find(|(registered_command, _)| same_command(registered_command, command))
            .map(|(_, items)| items.clone())
            .unwrap_or_default();
        if !registered_menu_items.is_empty() {
            actions.extend(registered_menu_items);
            actions.push(Separator::new());
        }

        if command.command() != "" {
            let label_run = localize("terminal.rerunCommand", "Rerun Command", &[]);
            let (weak, command_to_run) = (self.this.clone(), command.clone());
            actions.push(make_action("terminal.rerunCommand", &label_run, &label_run, move || {
                if command_to_run.command() == "" {
                    return undefined();
                }
                let Some(this) = weak.upgrade() else { return undefined() };
                let (weak, command) = (weak.clone(), command_to_run.clone());
                if command.is_trusted() {
                    this.on_did_request_run_command.fire(DecorationRunCommandRequest { command, no_new_line: None });
                    return undefined();
                }
                MaybePromise::Promise(this.prompt_to_run(&command).then(move |should_run| {
                    if should_run && let Some(this) = weak.upgrade() {
                        this.on_did_request_run_command.fire(DecorationRunCommandRequest { command: command.clone(), no_new_line: None });
                    }
                    Dynamic::Undefined
                }))
            }));
            // The second section is the clipboard section
            actions.push(Separator::new());
            let label_copy = localize("terminal.copyCommand", "Copy Command", &[]);
            let (weak, command_to_copy) = (self.this.clone(), command.clone());
            actions.push(make_action("terminal.copyCommand", &label_copy, &label_copy, move || match weak.upgrade() {
                Some(this) => finished(this.services.clipboard_service.write_text(&command_to_copy.command())),
                None => undefined(),
            }));
        }
        if command.has_output() {
            let label_copy_command_and_output = localize("terminal.copyCommandAndOutput", "Copy Command and Output", &[]);
            let (weak, command_to_copy) = (self.this.clone(), command.clone());
            actions.push(make_action("terminal.copyCommandAndOutput", &label_copy_command_and_output, &label_copy_command_and_output, move || {
                let (Some(this), Some(output)) = (weak.upgrade(), command_to_copy.get_output()) else { return undefined() };
                let command_line = command_to_copy.command();
                let prefix = if command_line != "" { format!("{command_line}\n") } else { String::new() };
                finished(this.services.clipboard_service.write_text(&format!("{prefix}{output}")))
            }));
            let label_text = localize("terminal.copyOutput", "Copy Output", &[]);
            let (weak, command_to_copy) = (self.this.clone(), command.clone());
            actions.push(make_action("terminal.copyOutput", &label_text, &label_text, move || {
                let (Some(this), Some(text)) = (weak.upgrade(), command_to_copy.get_output()) else { return undefined() };
                finished(this.services.clipboard_service.write_text(&text))
            }));
            let label_html = localize("terminal.copyOutputAsHtml", "Copy Output as HTML", &[]);
            let (weak, command_to_copy) = (self.this.clone(), command.clone());
            actions.push(make_action("terminal.copyOutputAsHtml", &label_html, &label_html, move || {
                if let Some(this) = weak.upgrade() {
                    this.on_did_request_copy_as_html.fire(DecorationCopyAsHtmlRequest { command: command_to_copy.clone() });
                }
                undefined()
            }));
        }
        if !actions.is_empty() {
            actions.push(Separator::new());
        }
        let label_run_recent = localize("workbench.action.terminal.runRecentCommand", "Run Recent Command", &[]);
        let weak = self.this.clone();
        actions.push(make_action("workbench.action.terminal.runRecentCommand", &label_run_recent, &label_run_recent, move || {
            match weak.upgrade() {
                Some(this) => MaybePromise::Promise(this.services.command_service.execute_command("workbench.action.terminal.runRecentCommand", Vec::new())),
                None => undefined(),
            }
        }));
        let label_go_to_recent = localize("workbench.action.terminal.goToRecentDirectory", "Go To Recent Directory", &[]);
        let weak = self.this.clone();
        actions.push(make_action("workbench.action.terminal.goToRecentDirectory", &label_go_to_recent, &label_run_recent, move || {
            match weak.upgrade() {
                Some(this) => MaybePromise::Promise(this.services.command_service.execute_command("workbench.action.terminal.goToRecentDirectory", Vec::new())),
                None => undefined(),
            }
        }));

        actions.push(Separator::new());

        let label_about = localize("terminal.learnShellIntegration", "Learn About Shell Integration", &[]);
        let weak = self.this.clone();
        actions.push(make_action("terminal.learnShellIntegration", &label_about, &label_about, move || match weak.upgrade() {
            Some(this) => {
                let opened = this.services.opener_service.open(&OpenResource::Text(JsString::from("https://code.visualstudio.com/docs/terminal/shell-integration")), None);
                MaybePromise::Promise(opened.then(|_| Dynamic::Undefined))
            }
            None => undefined(),
        }));
        actions
    }

    /// `new Promise<boolean>(r => this._notificationService.prompt(Severity.Info, ..., [yes, no]))`.
    fn prompt_to_run(&self, command: &Rc<dyn ITerminalCommand>) -> Promise<bool> {
        let message = localize("rerun", "Do you want to run the command: {0}", &[&command.command()]);
        let notification_service = self.services.notification_service.clone();
        Promise::with_executor(move |resolver| {
            let (resolve_yes, resolve_no) = (resolver.clone(), resolver);
            let choice = |label: String, run: Rc<dyn Fn()>| PromptChoiceKind::Plain(PromptChoice { base: BasePromptChoice { telemetry_id: None, label, keep_open: false, run }, is_secondary: false });
            notification_service.prompt(
                Severity::Info,
                NotificationMessage::Text(message),
                vec![
                    choice(localize("yes", "Yes", &[]), Rc::new(move || resolve_yes.resolve(true))),
                    choice(localize("no", "No", &[]), Rc::new(move || resolve_no.resolve(false))),
                ],
                None,
            );
        })
    }

    fn show_toggle_visibility_quick_pick(&self) {
        let quick_pick = self.services.quick_input_service.create_quick_pick(None);
        self.base.register(quick_pick.clone());
        quick_pick.set_hide_input(true);
        quick_pick.set_hide_check_all(true);
        quick_pick.set_can_select_many(true);
        quick_pick.set_title(Some(localize("toggleVisibility", "Toggle visibility", &[])));
        let config_value = config_string(&self.services.configuration_service.get_value(Some(TerminalSettingId::ShellIntegrationDecorationsEnabled.as_str()), None));
        let config_value = config_value.as_deref();
        let gutter_label = localize("gutter", "Gutter command decorations", &[]);
        let overview_ruler_label = localize("overviewRuler", "Overview ruler command decorations", &[]);
        let item = |label: &str, picked: bool| IQuickPickItem {
            base: IQuickItem { label: JsString::from(label), ..Default::default() },
            picked: Some(picked),
            ..Default::default()
        };
        let gutter_icon = item(&gutter_label, config_value != Some("never") && config_value != Some("overviewRuler"));
        let overview_ruler_icon = item(&overview_ruler_label, config_value != Some("never") && config_value != Some("gutter"));
        quick_pick.set_items(vec![QuickPickInput::Item(gutter_icon.clone()), QuickPickInput::Item(overview_ruler_icon.clone())]);
        let mut selected_items: Vec<IQuickPickItem> = Vec::new();
        if config_value != Some("never") {
            if config_value != Some("gutter") {
                selected_items.push(gutter_icon);
            }
            if config_value != Some("overviewRuler") {
                selected_items.push(overview_ruler_icon);
            }
        }
        quick_pick.set_selected_items(selected_items);
        let weak = self.this.clone();
        self.base.register(quick_pick.on_did_change_selection().listen(move |selected| {
            let Some(this) = weak.upgrade() else { return };
            let includes = |label: &str| selected.iter().any(|item| item.base.label.to_string_lossy() == label);
            let mut new_value = "never";
            if includes(&gutter_label) {
                new_value = if includes(&overview_ruler_label) { "both" } else { "gutter" };
            } else if includes(&overview_ruler_label) {
                new_value = "overviewRuler";
            }
            let _ = this.services.configuration_service.update_value(TerminalSettingId::ShellIntegrationDecorationsEnabled.as_str(), Dynamic::string(new_value), None, None);
        }));
        quick_pick.set_ok(QuickPickOk::Bool(false));
        quick_pick.show();
    }

    fn get_decoration_css_color(&self, command: Option<&dyn ITerminalCommand>) -> Option<String> {
        let color_id = match command.and_then(|command| command.exit_code()) {
            None => TERMINAL_COMMAND_DECORATION_DEFAULT_BACKGROUND_COLOR,
            Some(exit_code) if exit_code != 0.0 => TERMINAL_COMMAND_DECORATION_ERROR_BACKGROUND_COLOR,
            Some(_) => TERMINAL_COMMAND_DECORATION_SUCCESS_BACKGROUND_COLOR,
        };
        self.services.theme_service.get_color_theme().get_color(color_id, None).map(|color| color.to_string())
    }
}

impl IDisposable for DecorationAddon {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IDecorationAddon for DecorationAddon {
    fn register_menu_items(&self, command: Rc<dyn ITerminalCommand>, items: Vec<ActionRef>) -> DisposableRef {
        DecorationAddon::register_menu_items(self, command, items)
    }
}

/// O `JSON.stringify(command)` da mensagem de erro (ver o cabeçalho).
fn describe_command(command: Option<&dyn ITerminalCommand>) -> String {
    match command {
        Some(command) => format!("{{\"command\":{:?},\"timestamp\":{}}}", command.command(), je_base::dynamic::number_to_js_string(command.timestamp())),
        None => "undefined".to_owned(),
    }
}
