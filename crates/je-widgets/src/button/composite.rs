//! `ButtonWithDescription`, `ButtonWithDropdown` e `ButtonBar` de `button.ts`, sobre o [`Button`].
//!
//! O `ButtonWithDropdown` junta o botão principal, o separador e o botão do menu (`display: flex` do
//! `.monaco-button-dropdown`); o layout é a resolução de comprimentos flexíveis de [`super::flex`]. O
//! menu de contexto e o provedor de ações (`IContextMenuProvider`, `IActionProvider`) são contratos do
//! `contextmenu.ts` e do `dropdown.ts`; ficam definidos aqui no mínimo que o botão usa
//! (`TODO(integração)`: mover para os módulos deles quando entrarem).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::actions::{ActionRef, IActionRunner, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::nls;
use je_base::promise::MaybePromise;
use je_base::key_codes::KeyCode;
use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Border, EventKind, NodeId, Role, Ui};

use super::flex::{FlexItem, resolve_flexible_lengths};
use super::{Button,ButtonBox, ButtonEvent, ButtonLabel, ButtonOptions, ButtonTitle, IButton, IButtonWithDescription, LabelPart, INHERITED_FOREGROUND, layout_row, rebuild_label_parts};
use crate::common::registered_emitter;
use crate::context_view_api::Anchor;
use crate::dom::{StandardKeyboardEvent, create_el};
use crate::hover_api::create_instant_hover_delegate;
use crate::text::{natural_line_height, ui_font};

/// `IActionProvider` de `dropdown.ts`.
pub trait IActionProvider {
    fn get_actions(&self) -> Vec<ActionRef>;
}

/// `readonly IAction[] | IActionProvider`.
#[derive(Clone)]
pub enum ButtonDropdownActions {
    Actions(Vec<ActionRef>),
    Provider(Rc<dyn IActionProvider>),
}

impl ButtonDropdownActions {
    fn get(&self) -> Vec<ActionRef> {
        match self {
            Self::Actions(actions) => actions.clone(),
            Self::Provider(provider) => provider.get_actions(),
        }
    }
}

/// O mínimo de `IContextMenuDelegate` que o botão usa.
#[derive(Clone)]
pub struct ContextMenuDelegate {
    pub get_anchor: Rc<dyn Fn() -> Anchor>,
    pub get_actions: Rc<dyn Fn() -> Vec<ActionRef>>,
    pub action_runner: Option<Rc<dyn IActionRunner>>,
    pub on_hide: Rc<dyn Fn()>,
    /// As camadas maiores ficam por cima (`dropdownLayer`).
    pub layer: Option<i32>,
}

/// `IContextMenuProvider`.
pub trait IContextMenuProvider {
    fn show_context_menu(&self, delegate: ContextMenuDelegate);
}

/// `IButtonWithDropdownOptions`.
#[derive(Clone)]
pub struct ButtonWithDropdownOptions {
    pub button: ButtonOptions,
    pub context_menu_provider: Rc<dyn IContextMenuProvider>,
    pub actions: ButtonDropdownActions,
    pub action_runner: Option<Rc<dyn IActionRunner>>,
    pub add_primary_action_to_dropdown: Option<bool>,
    pub dropdown_layer: Option<i32>,
}

/// Repete para os botões que embrulham um [`Button`] (`ButtonWithDropdown`, `ButtonWithDescription`) o que
/// o `IButton` deles delega ao botão principal sem mudar nada.
macro_rules! forward_to_primary {
    ($field:ident) => {
        fn set_icon(&self, icon: &ThemeIcon) {
            self.$field.set_icon(icon);
        }
        fn set_checked(&self, value: bool) {
            self.$field.set_checked(value);
        }
        fn checked(&self) -> bool {
            self.$field.checked()
        }
        fn set_title(&self, title: &str) {
            self.$field.set_title(title);
        }
        fn set_aria_label(&self, aria_label: &str) {
            self.$field.set_aria_label(aria_label);
        }
        fn focus(&self) {
            self.$field.focus();
        }
    };
}

/// O `.monaco-button-dropdown > .monaco-button.monaco-text-button` (o botão principal).
fn dropdown_primary_css(button: &Button, b: &mut ButtonBox) {
    if button.ui().tree().has_class(button.element(), "monaco-text-button") {
        b.border_widths[1] = 0.0;
        b.radii = [4.0, 0.0, 0.0, 4.0];
        b.outline_offset = -1.0;
    }
}

/// O `.monaco-button-dropdown > .monaco-button.monaco-dropdown-button` (o botão do menu).
fn dropdown_arrow_css(button: &Button, b: &mut ButtonBox) {
    let styles = button.styles();
    let secondary = button.ui().tree().has_class(button.element(), "secondary");
    b.padding = [0.0, 4.0, 0.0, 4.0];
    b.border_widths = [1.0, 1.0, 1.0, 0.0];
    b.border_color = if secondary { styles.button_secondary_border.or(styles.button_border) } else { styles.button_border };
    b.radii = [0.0, 4.0, 4.0, 0.0];
    b.cursor_pointer = true;
    b.outline_offset = -1.0;
}

/// O botão com menu.
pub struct ButtonWithDropdown {
    base: Rc<Disposable>,
    ui: Ui,
    element: NodeId,
    pub primary_button: Rc<Button>,
    action: ActionRef,
    separator_container: NodeId,
    separator: NodeId,
    pub dropdown_button: Rc<Button>,
    options: ButtonWithDropdownOptions,
    width: Cell<Option<f64>>,
    on_did_click: Rc<Emitter<Option<ButtonEvent>>>,
    disposed: Cell<bool>,
}

impl ButtonWithDropdown {
    pub fn new(ui: &Ui, container: NodeId, mut options: ButtonWithDropdownOptions) -> Rc<ButtonWithDropdown> {
        let base = Disposable::new();
        let element = {
            let mut tree = ui.tree_mut();
            create_el(&mut tree, container, Role::Group, "monaco-button-dropdown")
        };
        if options.button.hover_delegate.is_none() {
            let delegate = create_instant_hover_delegate();
            base.register(delegate.clone());
            options.button.hover_delegate = Some(delegate);
        }
        let primary_button = Button::new(ui, element, options.button.clone());
        base.register(primary_button.clone());
        primary_button.set_css(Some(Rc::new(dropdown_primary_css)));
        let on_did_click = registered_emitter::<Option<ButtonEvent>>(&base);
        {
            let on_did_click = on_did_click.clone();
            base.register(primary_button.on_did_click().listen(move |e| on_did_click.fire(*e)));
        }
        let action = {
            let on_did_click = on_did_click.clone();
            to_action(ToActionProps {
                id: "primaryAction".to_owned(),
                label: primary_button.label().plain_text(),
                tooltip: None,
                enabled: None,
                checked: None,
                class: None,
                alt: None,
                run: Rc::new(move |_| {
                    on_did_click.fire(None);
                    MaybePromise::Value(Dynamic::Undefined)
                }),
            })
        };
        let (separator_container, separator) = {
            let mut tree = ui.tree_mut();
            let separator_container = create_el(&mut tree, element, Role::Presentation, "monaco-button-dropdown-separator");
            let separator = create_el(&mut tree, separator_container, Role::Presentation, "");
            (separator_container, separator)
        };
        let dropdown_options = ButtonOptions { title: ButtonTitle::Text(nls::localize("button dropdown more actions", "More Actions...", &[])), support_icons: true, ..options.button.clone() };
        let dropdown_button = Button::new(ui, element, dropdown_options);
        base.register(dropdown_button.clone());
        dropdown_button.set_attribute("aria-haspopup", "true");
        dropdown_button.set_attribute("aria-expanded", "false");
        ui.tree_mut().add_class(dropdown_button.element(), "monaco-dropdown-button");
        dropdown_button.set_icon(&ThemeIcon::from_id("drop-down-button"));
        dropdown_button.set_css(Some(Rc::new(dropdown_arrow_css)));
        let this = Rc::new(ButtonWithDropdown {
            base,
            ui: ui.clone(),
            element,
            primary_button,
            action,
            separator_container,
            separator,
            dropdown_button,
            options,
            width: Cell::new(None),
            on_did_click,
            disposed: Cell::new(false),
        });
        let weak = Rc::downgrade(&this);
        this.base.register(this.dropdown_button.on_did_click().listen(move |_| {
            let Some(this) = weak.upgrade() else { return };
            let actions = this.options.actions.get();
            let (primary, add_primary) = (this.action.clone(), this.options.add_primary_action_to_dropdown != Some(false));
            let (anchor, hide) = (this.dropdown_button.clone(), this.dropdown_button.clone());
            this.options.context_menu_provider.show_context_menu(ContextMenuDelegate {
                get_anchor: Rc::new(move || Anchor::Node(anchor.element())),
                get_actions: Rc::new(move || if add_primary { std::iter::once(primary.clone()).chain(actions.iter().cloned()).collect() } else { actions.clone() }),
                action_runner: this.options.action_runner.clone(),
                on_hide: Rc::new(move || hide.set_attribute("aria-expanded", "false")),
                layer: this.options.dropdown_layer,
            });
            this.dropdown_button.set_attribute("aria-expanded", "true");
        }));
        this.apply_separator_style();
        this
    }

    /// Os estilos "inline" do separador: a borda, o fundo e a cor do traço.
    fn apply_separator_style(&self) {
        let button = &self.options.button;
        let styles = button.styles;
        let border = if button.secondary { styles.button_secondary_border } else { styles.button_border };
        let background = if button.secondary { styles.button_secondary_background } else { styles.button_background };
        let disabled = !IButton::enabled(self);
        let mut tree = self.ui.tree_mut();
        let style = tree.style_mut(self.separator_container);
        style.background = background;
        style.border = border.map(|color| Border { width: 1.0, color });
        style.border_widths = border.map(|_| [1.0, 0.0, 1.0, 0.0]);
        style.opacity = if disabled { 0.4 } else { 1.0 };
        tree.style_mut(self.separator).background = styles.button_separator;
        tree.style_mut(self.element).cursor = Some(if disabled { je_ui::Cursor::Default } else { je_ui::Cursor::Pointer });
    }

    pub fn dropdown_button_element(&self) -> NodeId {
        self.dropdown_button.element()
    }

    /// `style.width` do `.monaco-button-dropdown`: `None` é a soma do conteúdo.
    pub fn set_width(&self, width: Option<f64>) {
        self.width.set(width);
        self.layout();
    }

    /// `style.left` e `style.top`.
    pub fn set_position(&self, x: f64, y: f64) {
        let mut tree = self.ui.tree_mut();
        let r = tree.bounds(self.element);
        tree.set_bounds(self.element, Rect::new(x, y, x + r.width(), y + r.height()));
    }

    /// O `display: flex` do contêiner: o botão principal encolhe (`width: 100%`, `flex-shrink: 1`) até o
    /// separador (1px) e o botão do menu caberem; o contêiner estica a altura dos três.
    pub fn layout(&self) {
        self.primary_button.set_height(None);
        self.dropdown_button.set_height(None);
        let intrinsic = |b: &Rc<Button>| b.min_content_width();
        let (primary_min, arrow_width) = (intrinsic(&self.primary_button), intrinsic(&self.dropdown_button));
        let total = self.width.get().unwrap_or(primary_min + 1.0 + arrow_width);
        let primary_box = self.primary_button.compute_box();
        let items = [
            FlexItem { base: total, pad_border: primary_box.horizontal_extra(), grow: 0.0, shrink: 1.0, min: primary_min },
            FlexItem { base: 1.0, pad_border: 0.0, grow: 0.0, shrink: 1.0, min: 1.0 },
            FlexItem { base: arrow_width, pad_border: self.dropdown_button.compute_box().horizontal_extra(), grow: 0.0, shrink: 1.0, min: arrow_width },
        ];
        let widths = resolve_flexible_lengths(&items, total);
        let height = self.primary_button.size().1.max(self.dropdown_button.size().1);
        self.primary_button.set_height(Some(height));
        self.dropdown_button.set_height(Some(height));
        self.primary_button.set_position(0.0, 0.0);
        self.primary_button.set_width(Some(widths[0]));
        self.dropdown_button.set_position(widths[0] + widths[1], 0.0);
        self.dropdown_button.set_width(Some(widths[2]));
        let mut tree = self.ui.tree_mut();
        tree.set_bounds(self.separator_container, Rect::new(widths[0], 0.0, widths[0] + widths[1], height));
        // `padding: 4px 0` e a borda de cima e de baixo do contêiner; o traço ocupa o resto.
        let border = tree.style(self.separator_container).border_widths.map_or(0.0, |w| w[0]);
        tree.set_bounds(self.separator, Rect::new(0.0, border + 4.0, widths[1], height - border - 4.0));
        let origin = tree.bounds(self.element);
        tree.set_bounds(self.element, Rect::new(origin.x0, origin.y0, origin.x0 + total, origin.y0 + height));
    }
}

impl IButton for ButtonWithDropdown {
    fn element(&self) -> NodeId {
        self.element
    }

    fn on_did_click(&self) -> BaseEvent<Option<ButtonEvent>> {
        self.on_did_click.event()
    }

    fn set_label(&self, value: ButtonLabel) {
        let text = value.plain_text();
        self.primary_button.set_label(value);
        self.action.set_label(text);
    }

    fn set_enabled(&self, enabled: bool) {
        self.primary_button.set_enabled(enabled);
        self.dropdown_button.set_enabled(enabled);
        self.ui.tree_mut().toggle_class(self.element, "disabled", !enabled);
        self.apply_separator_style();
    }

    fn enabled(&self) -> bool {
        self.primary_button.enabled()
    }

    fn has_focus(&self) -> bool {
        self.primary_button.has_focus() || self.dropdown_button.has_focus()
    }

    forward_to_primary!(primary_button);
}

impl IDisposable for ButtonWithDropdown {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.base.dispose();
        self.ui.remove(self.element);
    }

    fn constructor_name(&self) -> &'static str {
        "ButtonWithDropdown"
    }
}

/// O botão com uma descrição em itálico embaixo (`.monaco-description-button`).
pub struct ButtonWithDescription {
    base: Rc<Disposable>,
    ui: Ui,
    options: ButtonOptions,
    element: NodeId,
    button: Rc<Button>,
    description_element: NodeId,
    description: RefCell<Vec<LabelPart>>,
    foreground: Cell<Option<Rgba>>,
    disposed: Cell<bool>,
}

impl ButtonWithDescription {
    pub fn new(ui: &Ui, container: NodeId, options: ButtonOptions) -> Rc<ButtonWithDescription> {
        let base = Disposable::new();
        let element = {
            let mut tree = ui.tree_mut();
            let element = tree.create_detached(Role::Group);
            tree.add_class(element, "monaco-description-button");
            element
        };
        let button = Button::new(ui, element, options.clone());
        base.register(button.clone());
        let description_element = {
            let mut tree = ui.tree_mut();
            let node = create_el(&mut tree, element, Role::Generic, "monaco-button-description");
            tree.append_child(container, element);
            node
        };
        Rc::new(ButtonWithDescription { base, ui: ui.clone(), options, element, button, description_element, description: RefCell::new(Vec::new()), foreground: Cell::new(None), disposed: Cell::new(false) })
    }

    pub fn button(&self) -> &Rc<Button> {
        &self.button
    }

    pub fn description_element(&self) -> NodeId {
        self.description_element
    }

    /// O `color` herdado pela descrição.
    pub fn set_foreground(&self, color: Option<Rgba>) {
        self.foreground.set(color);
        self.layout(None);
    }

    /// `getAttribute` do elemento da descrição.
    pub fn attribute(&self, name: &str) -> Option<String> {
        self.ui.tree().attribute(self.element, name).map(str::to_owned)
    }

    /// `setAttribute` do elemento da descrição.
    pub fn set_attribute(&self, name: &str, value: impl Into<String>) {
        self.ui.tree_mut().set_attribute(self.element, name, value);
    }

    /// O texto da descrição (o `textContent`).
    pub fn description_text(&self) -> String {
        self.description.borrow().iter().filter(|p| p.kind == super::LabelPartKind::Text).map(|p| p.text.clone()).collect()
    }

    /// O `display: flex; flex-direction: column; align-items: center; margin: 4px 5px`: o botão ocupa a
    /// largura (100%) e a descrição, de largura própria (texto mais `padding: 4px 20px`), fica centrada.
    /// `outer` é a caixa de margem (`x`, `y`, largura); a largura `None` mantém a atual.
    pub fn layout(&self, outer: Option<(f64, f64, f64)>) {
        let current = self.ui.tree().bounds(self.element);
        let (x, y, outer_width) = outer.unwrap_or((current.x0 - 5.0, current.y0 - 4.0, current.width() + 10.0));
        let width = outer_width - 10.0;
        let description_style = ButtonBox { font_size: 11.0, line_height: natural_line_height(&ui_font(11.0, 400)), foreground: Some(self.foreground.get().unwrap_or(INHERITED_FOREGROUND)), ..description_box() };
        let desc_content: f64 = description_items_width(&self.description.borrow(), &description_style);
        let desc_height = description_style.line_height + 8.0;
        self.button.set_position(0.0, 0.0);
        self.button.set_width(Some(width));
        let button_height = self.button.size().1;
        let desc_width = (desc_content + 40.0).min(width);
        {
            let mut tree = self.ui.tree_mut();
            tree.set_bounds(self.description_element, Rect::new((width - desc_width) / 2.0, button_height, (width + desc_width) / 2.0, button_height + desc_height));
            tree.set_bounds(self.element, Rect::new(x + 5.0, y + 4.0, x + 5.0 + width, y + 4.0 + button_height + desc_height));
        }
        layout_row(&self.ui, &self.description.borrow(), &description_style, Rect::new(20.0, 4.0, desc_width - 20.0, 4.0 + description_style.line_height));
    }
}

/// O `.monaco-button-description` sem padding nem borda no `ButtonBox` (o dono do layout põe o `padding`).
fn description_box() -> ButtonBox {
    ButtonBox {
        padding: [0.0; 4],
        border_widths: [0.0; 4],
        border_color: None,
        radii: [0.0; 4],
        background: None,
        foreground: None,
        font_size: 11.0,
        font_weight: 400,
        line_height: 14.0,
        height: None,
        icon_margin_em: 0.2,
        reserve_weight: None,
        cursor_pointer: false,
        outline_offset: -1.0,
    }
}

fn description_items_width(parts: &[LabelPart], b: &ButtonBox) -> f64 {
    parts
        .iter()
        .map(|p| match p.kind {
            super::LabelPartKind::Text => crate::text::measure_text(&p.text, &ui_font(b.font_size as f32, b.font_weight)),
            super::LabelPartKind::Icon => super::CODICON_SIZE * (1.0 + 2.0 * b.icon_margin_em),
        })
        .sum()
}

impl IButton for ButtonWithDescription {
    fn element(&self) -> NodeId {
        self.element
    }

    fn on_did_click(&self) -> BaseEvent<Option<ButtonEvent>> {
        self.button.on_did_click()
    }

    fn set_label(&self, value: ButtonLabel) {
        self.button.set_label(value);
    }

    fn set_enabled(&self, enabled: bool) {
        self.button.set_enabled(enabled);
    }

    fn enabled(&self) -> bool {
        self.button.enabled()
    }

    fn has_focus(&self) -> bool {
        self.button.has_focus()
    }

    forward_to_primary!(button);
}

impl IButtonWithDescription for ButtonWithDescription {
    fn set_description(&self, value: &str) {
        rebuild_label_parts(&self.ui, self.description_element, &self.description, value, self.options.support_icons);
        self.layout(None);
    }
}

impl IDisposable for ButtonWithDescription {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.button.dispose();
        self.base.dispose();
        self.ui.remove(self.element);
    }

    fn constructor_name(&self) -> &'static str {
        "ButtonWithDescription"
    }
}

/// `ButtonBarAlignment`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonBarAlignment {
    #[default]
    Horizontal,
    Vertical,
}

/// A barra de botões: guarda os botões e move o foco entre eles com as setas.
pub struct ButtonBar {
    ui: Ui,
    container: NodeId,
    alignment: ButtonBarAlignment,
    buttons: Rc<RefCell<Vec<Rc<dyn IButton>>>>,
    button_store: Rc<DisposableStore>,
}

impl ButtonBar {
    pub fn new(ui: &Ui, container: NodeId, alignment: Option<ButtonBarAlignment>) -> Self {
        Self { ui: ui.clone(), container, alignment: alignment.unwrap_or_default(), buttons: Rc::new(RefCell::new(Vec::new())), button_store: DisposableStore::new() }
    }

    pub fn buttons(&self) -> Vec<Rc<dyn IButton>> {
        self.buttons.borrow().clone()
    }

    pub fn clear(&self) {
        self.button_store.clear();
        self.buttons.borrow_mut().clear();
    }

    pub fn add_button(&self, options: ButtonOptions) -> Rc<Button> {
        let button = self.button_store.add_rc(Button::new(&self.ui, self.container, options));
        self.push_button(button.clone());
        button
    }

    pub fn add_button_with_description(&self, options: ButtonOptions) -> Rc<ButtonWithDescription> {
        let button = self.button_store.add_rc(ButtonWithDescription::new(&self.ui, self.container, options));
        self.push_button(button.clone());
        button
    }

    pub fn add_button_with_dropdown(&self, options: ButtonWithDropdownOptions) -> Rc<ButtonWithDropdown> {
        let button = self.button_store.add_rc(ButtonWithDropdown::new(&self.ui, self.container, options));
        self.push_button(button.clone());
        button
    }

    fn push_button(&self, button: Rc<dyn IButton>) {
        let index = {
            let mut buttons = self.buttons.borrow_mut();
            buttons.push(button.clone());
            buttons.len() - 1
        };
        let (buttons, alignment) = (self.buttons.clone(), self.alignment);
        self.button_store.add(self.ui.listen(button.element(), EventKind::KeyDown, false, move |e| {
            let Some(event) = StandardKeyboardEvent::from_event(e) else { return };
            let (previous, next) = match alignment {
                ButtonBarAlignment::Vertical => (KeyCode::UpArrow, KeyCode::DownArrow),
                ButtonBarAlignment::Horizontal => (KeyCode::LeftArrow, KeyCode::RightArrow),
            };
            let buttons = buttons.borrow();
            let last = buttons.len() - 1;
            // Next / Previous Button
            let to_focus = if event.equals(previous.bits()) {
                Some(if index > 0 { index - 1 } else { last })
            } else if event.equals(next.bits()) {
                Some(if index == last { 0 } else { index + 1 })
            } else {
                None
            };
            if let Some(target) = to_focus {
                let target = buttons[target].clone();
                drop(buttons);
                target.focus();
                e.stop(true);
            }
        }));
    }
}

impl IDisposable for ButtonBar {
    fn dispose(&self) {
        self.button_store.dispose();
        self.buttons.borrow_mut().clear();
    }
}
