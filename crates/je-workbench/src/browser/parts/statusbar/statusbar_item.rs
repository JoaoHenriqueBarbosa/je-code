//! Porte de `src/vs/workbench/browser/parts/statusbar/statusbarItem.ts`: o [`StatusbarEntryItem`], o item da barra
//! de status que mostra o texto, a dica de ferramenta (hover gerenciável), o comando, a cor e o tipo de uma
//! [`StatusbarEntry`].
//!
//! Divergências:
//! - **DOM.** Os elementos são [`ViewElement`] (classes, estilo, pai, `hide`/`show`). O que o original faz com o DOM
//!   e o `ViewElement` não modela entra pelo trait [`StatusbarItemView`], passado ao construtor: os ouvintes de
//!   `pointerdown`, `click`, `Tap` (toque) e `keydown` do `labelContainer` ([`StatusbarItemEventKind`] e
//!   [`StatusbarItemEvent`], com o `keyCode` já na codificação do VS Code, como o `StandardKeyboardEvent`), os
//!   atributos (`role`, `tabIndex`, `aria-label`), o `appendChild(entry.content)`, o alvo de gestos de toque
//!   (`Gesture.addTarget`) e o desenho do rótulo com ícones.
//! - **Rótulo.** `StatusBarCodiconLabel` é a estrutura privada de mesmo nome: guarda o estado do progresso e do texto e
//!   entrega o desenho a [`StatusbarItemView::render_label`], com o ícone de progresso identificado por uma geração
//!   que muda a cada `showProgress` novo (o original cria um elemento novo nesse momento e o mantém estável entre
//!   textos). O `currentText` do original nunca é atualizado pelo `set text`, então reativar o progresso redesenha
//!   com texto vazio até o próximo texto; o porte mantém o comportamento.
//! - **Serviços.** `ICommandService`, `INotificationService`, `ITelemetryService` e `IThemeService` entram como
//!   objetos de [`StatusbarItemServices`]; o `IHoverService` é o [`HoverService`] concreto, e o `@IInstantiationService`
//!   do original é o próprio `new`.
//! - **`executeCommand` assíncrono.** O comando é pedido ao serviço na hora (como o original, que só espera depois) e o
//!   erro da promessa vira `notificationService.error(toErrorMessage(error))`.
//! - **Identidade.** Os `!==` entre objetos do original (`command`, `tooltip`, `ThemeColor`) são a identidade dos `Rc`
//!   (ver [`StatusbarCommand::is_same`], [`StatusbarColor::is_same`]).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::error_message::to_error_message_of;
use je_base::html_content::markdown_string_equal;
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, MutableDisposable};
use je_editor::services::theme_service::IThemeService;
use je_platform::commands::commands::ICommandService;
use je_platform::contextview::view_element::ViewElement;
use je_platform::hover::hover_service::{
    HoverContent, HoverService, IHoverAction, IHoverDelegate, IManagedHover, IManagedHoverOptions, ManagedHoverContent, ManagedHoverContentOrFactory, ManagedHoverTooltip,
};
use je_platform::notification::notification::{INotificationService, NotificationMessage};
use je_platform::resources::ext_uri;
use je_platform::telemetry::telemetry::ITelemetryService;

use crate::services::progress::progress_service::StatusbarShowProgress;
use crate::services::statusbar::statusbar::{
    STATUSBAR_ENTRY_KINDS, StatusbarColor, StatusbarCommand, StatusbarEntry, StatusbarEntryKind, StatusbarTooltip, TooltipContent, TooltipWithCommands, is_same_color, is_same_command,
    is_tooltip_with_commands,
};

/// Os eventos de DOM que o item ouve no `labelContainer`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusbarItemEventKind {
    PointerDown,
    Click,
    /// `TouchEventType.Tap` (`Gesture`).
    Tap,
    KeyDown,
}

/// `PointerEvent`, `MouseEvent`, o evento `Tap` e `KeyboardEvent` no que o item lê.
pub struct StatusbarItemEvent {
    /// `button`.
    pub button: i32,
    /// `pointerId` quando o evento é um `PointerEvent` (`isPointerEvent(event)`); `None` nos demais.
    pub pointer_id: Option<i32>,
    /// `detail` (zero no `click()` programático).
    pub detail: i32,
    /// `StandardKeyboardEvent.keyCode`.
    pub key_code: KeyCode,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    default_prevented: Cell<bool>,
}

impl StatusbarItemEvent {
    pub fn new() -> Self {
        Self { button: 0, pointer_id: None, detail: 0, key_code: KeyCode::Unknown, ctrl_key: false, shift_key: false, alt_key: false, meta_key: false, default_prevented: Cell::new(false) }
    }

    /// `preventDefault()`.
    pub fn prevent_default(&self) {
        self.default_prevented.set(true);
    }

    /// `defaultPrevented`.
    pub fn default_prevented(&self) -> bool {
        self.default_prevented.get()
    }

    /// `StandardKeyboardEvent.equals(keyCode)`: a tecla sem modificadores.
    pub fn equals(&self, key_code: KeyCode) -> bool {
        self.key_code == key_code && !self.ctrl_key && !self.shift_key && !self.alt_key && !self.meta_key
    }
}

impl Default for StatusbarItemEvent {
    fn default() -> Self {
        Self::new()
    }
}

/// O ícone de progresso do rótulo: `syncing` ou `spinningLoading`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusbarProgressIconKind {
    Syncing,
    SpinningLoading,
}

/// O ícone de progresso do rótulo, com a geração que identifica o elemento criado por `renderIcon`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusbarProgressIcon {
    pub kind: StatusbarProgressIconKind,
    pub generation: u64,
}

/// Tudo que o item faz com o DOM além do que o [`ViewElement`] guarda.
pub trait StatusbarItemView {
    /// `addDisposableListener(element, type, handler)`.
    fn add_listener(&self, element: &Rc<ViewElement>, kind: StatusbarItemEventKind, handler: Rc<dyn Fn(&StatusbarItemEvent)>) -> DisposableRef;

    /// `Gesture.addTarget(element)`.
    fn add_touch_target(&self, element: &Rc<ViewElement>) -> DisposableRef;

    /// `element.setAttribute(name, value)`.
    fn set_attribute(&self, element: &Rc<ViewElement>, name: &str, value: &str);

    /// `container.appendChild(content)`.
    fn append_content(&self, container: &Rc<ViewElement>, content: &Rc<ViewElement>);

    /// Desenha o rótulo: com `progress`, o ícone de progresso vem primeiro (o mesmo enquanto a geração não muda) e depois o
    /// `text` com ícones `$(id)`; sem `progress`, só o `text`.
    fn render_label(&self, label_container: &Rc<ViewElement>, progress: Option<StatusbarProgressIcon>, text: &str);
}

/// Os serviços de que o item depende.
pub struct StatusbarItemServices {
    pub command_service: Rc<dyn ICommandService>,
    pub hover_service: Rc<HoverService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub theme_service: Rc<dyn IThemeService>,
    pub view: Rc<dyn StatusbarItemView>,
}

type Slot = Rc<MutableDisposable<dyn IDisposable>>;

fn new_slot() -> Slot {
    MutableDisposable::<dyn IDisposable>::new()
}

/// `StatusbarEntryItem`.
pub struct StatusbarEntryItem {
    this: Weak<StatusbarEntryItem>,
    base: Rc<Disposable>,
    services: StatusbarItemServices,
    container: Rc<ViewElement>,
    hover_delegate: Rc<dyn IHoverDelegate>,
    label: StatusbarCodiconLabel,

    entry: RefCell<Option<StatusbarEntry>>,

    foreground_listener: Slot,
    background_listener: Slot,

    command_mouse_listener: Slot,
    command_pointer_listener: Slot,
    command_touch_listener: Slot,
    command_keyboard_listener: Slot,

    hover: RefCell<Option<Rc<dyn IManagedHover>>>,

    pub label_container: Rc<ViewElement>,
    pub beak_container: Rc<ViewElement>,
}

impl StatusbarEntryItem {
    pub fn new(services: StatusbarItemServices, container: Rc<ViewElement>, entry: StatusbarEntry, hover_delegate: Rc<dyn IHoverDelegate>) -> Rc<Self> {
        let base = Disposable::new();

        // Label Container
        let label_container = ViewElement::with_classes(&["statusbar-item-label"]);
        label_container.set_parent(Some(container.clone()));
        services.view.set_attribute(&label_container, "role", "button");
        // allows screen readers to read title, but still prevents tab focus.
        services.view.set_attribute(&label_container, "tabIndex", "-1");
        base.register(services.view.add_touch_target(&label_container)); // enable touch

        // Label (with support for progress)
        let label = StatusbarCodiconLabel::new(services.view.clone(), label_container.clone());

        // Beak Container
        let beak_container = ViewElement::with_classes(&["status-bar-item-beak-container"]);
        beak_container.set_parent(Some(container.clone()));

        if let Some(content) = &entry.content {
            services.view.append_content(&container, content);
        }

        let register_slot = |slot: &Slot| {
            base.register(slot.clone());
        };
        let foreground_listener = new_slot();
        let background_listener = new_slot();
        let command_mouse_listener = new_slot();
        let command_pointer_listener = new_slot();
        let command_touch_listener = new_slot();
        let command_keyboard_listener = new_slot();
        for slot in [&foreground_listener, &background_listener, &command_mouse_listener, &command_pointer_listener, &command_touch_listener, &command_keyboard_listener] {
            register_slot(slot);
        }

        let item = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            services,
            container,
            hover_delegate,
            label,
            entry: RefCell::new(None),
            foreground_listener,
            background_listener,
            command_mouse_listener,
            command_pointer_listener,
            command_touch_listener,
            command_keyboard_listener,
            hover: RefCell::new(None),
            label_container,
            beak_container,
        });

        item.update(entry);

        item
    }

    /// `get name()`.
    pub fn name(&self) -> String {
        self.entry.borrow().as_ref().expect("a entrada do item foi definida").name.clone()
    }

    /// `get hasCommand()`.
    pub fn has_command(&self) -> bool {
        self.entry.borrow().as_ref().is_some_and(|entry| entry.command.is_some())
    }

    pub fn update(&self, entry: StatusbarEntry) {
        let previous = self.entry.borrow().clone();
        let previous = previous.as_ref();

        // Update: Progress
        self.label.set_show_progress(entry.show_progress.unwrap_or(StatusbarShowProgress::Flag(false)));

        // Update: Text
        if previous.is_none_or(|previous| entry.text != previous.text) {
            self.label.set_text(&entry.text);

            if entry.text.is_empty() {
                self.label_container.hide();
            } else {
                self.label_container.show();
            }
        }

        // Update: ARIA label
        //
        // Set the aria label on both elements so screen readers would read
        // the correct thing without duplication #96210

        if previous.is_none_or(|previous| entry.aria_label != previous.aria_label) {
            self.services.view.set_attribute(&self.container, "aria-label", &entry.aria_label);
            self.services.view.set_attribute(&self.label_container, "aria-label", &entry.aria_label);
        }

        if previous.is_none_or(|previous| entry.role != previous.role) {
            let role = entry.role.as_deref().filter(|role| !role.is_empty()).unwrap_or("button");
            self.services.view.set_attribute(&self.label_container, "role", role);
        }

        // Update: Hover
        if previous.is_none_or(|previous| !is_equal_tooltip(previous.tooltip.as_ref(), entry.tooltip.as_ref())) {
            let mut hover_options: Option<IManagedHoverOptions> = None;
            let hover_tooltip: Option<TooltipContent>;
            match entry.tooltip.as_ref().and_then(is_tooltip_with_commands) {
                Some(with_commands) => {
                    hover_tooltip = Some(with_commands.content.clone());
                    hover_options = Some(IManagedHoverOptions {
                        on_did_show: with_commands.on_did_show.clone(),
                        actions: self.hover_actions(with_commands),
                        ..IManagedHoverOptions::default()
                    });
                }
                None => {
                    hover_tooltip = match &entry.tooltip {
                        Some(StatusbarTooltip::Content(content)) => Some(content.clone()),
                        _ => None,
                    };
                }
            }

            let hover_contents = ManagedHoverContentOrFactory::Content(to_managed_hover_content(hover_tooltip));
            let existing = self.hover.borrow().clone();
            match existing {
                Some(hover) => {
                    let _ = hover.update(hover_contents, hover_options);
                }
                None => {
                    let hover = self.services.hover_service.setup_managed_hover(self.hover_delegate.clone(), &self.container, hover_contents, hover_options);
                    self.base.register(hover.clone());
                    *self.hover.borrow_mut() = Some(hover);
                }
            }
        }

        // Update: Command
        if previous.is_none_or(|previous| !is_same_command(entry.command.as_ref(), previous.command.as_ref())) {
            self.command_mouse_listener.clear();
            self.command_pointer_listener.clear();
            self.command_touch_listener.clear();
            self.command_keyboard_listener.clear();

            let command = entry.command.clone();
            let has_hover = self.hover.borrow().is_some();
            let is_tooltip_command = matches!(command, Some(StatusbarCommand::ShowTooltip | StatusbarCommand::ToggleTooltip));
            match command {
                Some(command) if !is_tooltip_command || has_hover => {
                    self.listen_to_command(command);

                    self.container.class_list_add("has-command");
                    self.label_container.class_list_remove("disabled");
                }
                _ => {
                    self.container.class_list_remove("has-command");
                    self.label_container.class_list_add("disabled");
                }
            }
        }

        // Update: Beak
        if previous.is_none_or(|previous| entry.show_beak != previous.show_beak) {
            if entry.show_beak.unwrap_or(false) {
                self.container.class_list_add("has-beak");
            } else {
                self.container.class_list_remove("has-beak");
            }
        }

        let has_background_color = entry.background_color.as_ref().is_some_and(StatusbarColor::is_truthy) || entry.kind.is_some_and(|kind| kind != StatusbarEntryKind::Standard);

        // Update: Kind
        if previous.is_none_or(|previous| entry.kind != previous.kind) {
            for kind in STATUSBAR_ENTRY_KINDS {
                self.container.class_list_remove(&format!("{}-kind", kind.as_str()));
            }

            if let Some(kind) = entry.kind.filter(|kind| *kind != StatusbarEntryKind::Standard) {
                self.container.class_list_add(&format!("{}-kind", kind.as_str()));
            }

            toggle_class(&self.container, "has-background-color", has_background_color);
        }

        // Update: Foreground
        if previous.is_none_or(|previous| !is_same_color(entry.color.as_ref(), previous.color.as_ref())) {
            self.apply_color(&self.label_container, entry.color.as_ref(), false);
        }

        // Update: Background
        if previous.is_none_or(|previous| !is_same_color(entry.background_color.as_ref(), previous.background_color.as_ref())) {
            toggle_class(&self.container, "has-background-color", has_background_color);
            self.apply_color(&self.container, entry.background_color.as_ref(), true);
        }

        // Remember for next round
        *self.entry.borrow_mut() = Some(entry);
    }

    /// As ações do hover de uma dica com comandos: `commandId`, `label` e `run: () => this.executeCommand(command)`.
    fn hover_actions(&self, with_commands: &TooltipWithCommands) -> Vec<IHoverAction> {
        with_commands
            .commands
            .iter()
            .map(|command| {
                let weak = self.this.clone();
                let run_command = command.clone();
                IHoverAction {
                    command_id: command.id(),
                    label: command.title(),
                    icon_class: None,
                    run: Rc::new(move |_| {
                        if let Some(item) = weak.upgrade() {
                            item.execute_command(&run_command, None);
                        }
                    }),
                }
            })
            .collect()
    }

    /// Os ouvintes do comando: `pointerdown` (só no `ToggleTooltipCommand`), `click`, `Tap` e `keydown`.
    fn listen_to_command(&self, command: StatusbarCommand) {
        let view = &self.services.view;

        // Mouse-down dismisses the hover before click, so capture pinned state on pointer-down.
        let tooltip_pointer_down: Rc<Cell<Option<(i32, bool)>>> = Rc::new(Cell::new(None));
        if matches!(command, StatusbarCommand::ToggleTooltip) {
            let state = tooltip_pointer_down.clone();
            let hover_service = self.services.hover_service.clone();
            let container = self.container.clone();
            self.command_pointer_listener.set(Some(view.add_listener(
                &self.label_container,
                StatusbarItemEventKind::PointerDown,
                Rc::new(move |e| {
                    state.set(if e.button == 0 { Some((e.pointer_id.unwrap_or(0), hover_service.get_sticky_hover(&container).is_some())) } else { None });
                }),
            )));
        }

        let weak = self.this.clone();
        let execute_command_command = command.clone();
        let state = tooltip_pointer_down;
        let execute_command: Rc<dyn Fn(&StatusbarItemEvent)> = Rc::new(move |event| {
            let was_pinned = match (state.get(), event.pointer_id) {
                (Some((pointer_id, was_pinned)), Some(event_pointer_id)) if event.detail > 0 && event_pointer_id == pointer_id => Some(was_pinned),
                _ => None,
            };
            state.set(None);
            if let Some(item) = weak.upgrade() {
                item.execute_command(&execute_command_command, was_pinned);
            }
        });
        self.command_mouse_listener.set(Some(view.add_listener(&self.label_container, StatusbarItemEventKind::Click, execute_command.clone())));
        self.command_touch_listener.set(Some(view.add_listener(&self.label_container, StatusbarItemEventKind::Tap, execute_command)));

        let weak = self.this.clone();
        self.command_keyboard_listener.set(Some(view.add_listener(
            &self.label_container,
            StatusbarItemEventKind::KeyDown,
            Rc::new(move |event| {
                let Some(item) = weak.upgrade() else {
                    return;
                };
                if event.equals(KeyCode::Space) || event.equals(KeyCode::Enter) {
                    event.prevent_default();

                    item.execute_command(&command, None);
                } else if event.equals(KeyCode::Escape) || event.equals(KeyCode::LeftArrow) || event.equals(KeyCode::RightArrow) {
                    event.prevent_default();

                    let hover = item.hover.borrow().clone();
                    if let Some(hover) = hover {
                        hover.hide();
                    }
                }
            }),
        )));
    }

    fn execute_command(&self, command: &StatusbarCommand, tooltip_was_pinned_on_pointer_down: Option<bool>) {
        // Custom commands from us: Show or toggle tooltip
        if matches!(command, StatusbarCommand::ShowTooltip | StatusbarCommand::ToggleTooltip) {
            let is_toggle = matches!(command, StatusbarCommand::ToggleTooltip);
            let sticky_hover = if is_toggle { self.services.hover_service.get_sticky_hover(&self.container) } else { None };
            let hover = self.hover.borrow().clone();
            if is_toggle && tooltip_was_pinned_on_pointer_down.unwrap_or(sticky_hover.is_some()) {
                if let Some(hover) = hover {
                    hover.hide();
                }
                if let Some(sticky_hover) = sticky_hover {
                    sticky_hover.dispose();
                }
            } else if let Some(hover) = hover {
                hover.show(true /* focus */);
            }
        }
        // Any other command is going through command service
        else {
            let id = command.id();
            let args = command.arguments();

            self.services
                .telemetry_service
                .public_log2("workbenchActionExecuted", Some(Dynamic::object([("id", Dynamic::string(&id)), ("from", Dynamic::string("status bar"))])));
            let notification_service = self.services.notification_service.clone();
            let _ = self.services.command_service.execute_command(&id, args).then_result(move |result| {
                if let Err(error) = result {
                    notification_service.error(vec![NotificationMessage::Text(to_error_message_of(&error, false))]);
                }
                Ok(())
            });
        }
    }

    fn apply_color(&self, container: &Rc<ViewElement>, color: Option<&StatusbarColor>, is_background: bool) {
        let property = if is_background { "background-color" } else { "color" };
        let set_color = |element: &Rc<ViewElement>, value: Option<String>| match value {
            Some(value) if !value.is_empty() => element.style_set_property(property, &value),
            _ => element.style_remove_property(property),
        };

        let slot = if is_background { &self.background_listener } else { &self.foreground_listener };
        slot.clear();

        let mut color_result: Option<String> = None;
        if let Some(color) = color.filter(|color| color.is_truthy()) {
            match color {
                StatusbarColor::Theme(theme_color) => {
                    color_result = self.services.theme_service.get_color_theme().get_color(&theme_color.id, None).map(|value| value.to_string());

                    let element = container.clone();
                    let id = theme_color.id.clone();
                    let listener = self.services.theme_service.on_did_color_theme_change().listen(move |theme| {
                        let color_value = theme.get_color(&id, None).map(|value| value.to_string());
                        match color_value {
                            Some(value) if !value.is_empty() => element.style_set_property(property, &value),
                            _ => element.style_remove_property(property),
                        }
                    });
                    slot.set(Some(listener));
                }
                StatusbarColor::Css(css) => color_result = Some(css.clone()),
            }
        }

        set_color(container, color_result);
    }
}

impl IDisposable for StatusbarEntryItem {
    fn dispose(&self) {
        self.label.dispose();
        self.base.dispose();
    }
}

/// `classList.toggle(name, force)`.
fn toggle_class(element: &Rc<ViewElement>, name: &str, force: bool) {
    if force {
        element.class_list_add(name);
    } else {
        element.class_list_remove(name);
    }
}

/// O `TooltipContent` como o conteúdo do hover gerenciável: um markdown vira `{ markdown, markdownNotSupportedFallback: undefined }`.
fn to_managed_hover_content(tooltip: Option<TooltipContent>) -> ManagedHoverContent {
    match tooltip {
        None => ManagedHoverContent::Undefined,
        Some(TooltipContent::Text(text)) => ManagedHoverContent::Text(text),
        Some(TooltipContent::Markdown(markdown)) => ManagedHoverContent::Tooltip(ManagedHoverTooltip { markdown: Some(HoverContent::Markdown(markdown)), ..ManagedHoverTooltip::default() }),
        Some(TooltipContent::Element(element)) => {
            ManagedHoverContent::Tooltip(ManagedHoverTooltip { element: Some(Rc::new(move |_| element.clone())), ..ManagedHoverTooltip::default() })
        }
        Some(TooltipContent::Managed(tooltip)) => ManagedHoverContent::Tooltip((*tooltip).clone()),
    }
}

/// `isEqualTooltip`.
fn is_equal_tooltip(tooltip: Option<&StatusbarTooltip>, other: Option<&StatusbarTooltip>) -> bool {
    let Some(tooltip) = tooltip else {
        return other.is_none();
    };

    match (tooltip, other) {
        (StatusbarTooltip::Content(TooltipContent::Markdown(a)), Some(StatusbarTooltip::Content(TooltipContent::Markdown(b)))) => markdown_string_equal(a, b, &ext_uri()),
        (StatusbarTooltip::Content(TooltipContent::Markdown(_)), _) => false,
        (StatusbarTooltip::Content(TooltipContent::Text(a)), Some(StatusbarTooltip::Content(TooltipContent::Text(b)))) => a == b,
        (StatusbarTooltip::Content(TooltipContent::Element(a)), Some(StatusbarTooltip::Content(TooltipContent::Element(b)))) => Rc::ptr_eq(a, b),
        (StatusbarTooltip::Content(TooltipContent::Managed(a)), Some(StatusbarTooltip::Content(TooltipContent::Managed(b)))) => Rc::ptr_eq(a, b),
        (StatusbarTooltip::WithCommands(a), Some(StatusbarTooltip::WithCommands(b))) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

/// `StatusBarCodiconLabel`.
struct StatusbarCodiconLabel {
    view: Rc<dyn StatusbarItemView>,
    container: Rc<ViewElement>,
    progress_icon: Cell<Option<StatusbarProgressIcon>>,
    icon_generation: Cell<u64>,
    current_text: RefCell<String>,
    current_show_progress: Cell<StatusbarShowProgress>,
}

impl StatusbarCodiconLabel {
    fn new(view: Rc<dyn StatusbarItemView>, container: Rc<ViewElement>) -> Self {
        Self {
            view,
            container,
            progress_icon: Cell::new(None),
            icon_generation: Cell::new(0),
            current_text: RefCell::new(String::new()),
            current_show_progress: Cell::new(StatusbarShowProgress::Flag(false)),
        }
    }

    fn set_show_progress(&self, show_progress: StatusbarShowProgress) {
        if self.current_show_progress.get() != show_progress {
            self.current_show_progress.set(show_progress);
            if show_progress != StatusbarShowProgress::Flag(false) {
                let generation = self.icon_generation.get() + 1;
                self.icon_generation.set(generation);
                let kind = if show_progress == StatusbarShowProgress::Syncing { StatusbarProgressIconKind::Syncing } else { StatusbarProgressIconKind::SpinningLoading };
                self.progress_icon.set(Some(StatusbarProgressIcon { kind, generation }));
            }
            let current_text = self.current_text.borrow().clone();
            self.set_text(&current_text);
        }
    }

    fn set_text(&self, text: &str) {
        // Progress: insert progress codicon as first element as needed
        // but keep it stable so that the animation does not reset
        let progress = if self.current_show_progress.get() != StatusbarShowProgress::Flag(false) { self.progress_icon.get() } else { None };
        match progress {
            Some(progress) => {
                // If we have text to show, add a space to separate from progress
                let text_content = if text.is_empty() { String::new() } else { format!("\u{A0}{text}") }; // prepend non-breaking space
                self.view.render_label(&self.container, Some(progress), &text_content);
            }
            // No Progress: no special handling
            None => self.view.render_label(&self.container, None, text),
        }
    }

    fn dispose(&self) {}
}
