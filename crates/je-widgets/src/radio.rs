//! Porte de `src/vs/base/browser/ui/radio/radio.ts` e `radio.css`: o [`Radio`], um grupo de botões em que
//! um só está ativo (`role="radiogroup"`), em duas aparências: a junta (as bordas se emendam) e a
//! segmentada (`className: 'segmented'`, retângulos arredondados com um indicador de seleção).
//!
//! Como o DOM e o CSS viram [`je_ui`]:
//!
//! - O `.monaco-custom-radio` é um nó com as classes do original (`segmented`, `multi-row`,
//!   `selection-indicator-ready`, `animate-selection`); cada opção é um [`Button`] (`.monaco-button`) com as
//!   classes `active` e `previous-active`; o `.monaco-radio-selection` e o `.monaco-radio-row-break` são nós.
//! - A folha de estilo `.monaco-custom-radio > .monaco-button ...` é um [`crate::button::ButtonCss`] instalado
//!   em cada botão ([`Radio::style_button`]): raios, bordas por lado, cores, fonte, padding e altura.
//! - O `display: flex` do grupo (com `gap`, `flex: 1 1 0`, `min-width: auto`, `flex-wrap` nas linhas que
//!   começam com `startsRow`) é [`Radio::layout`], sobre [`crate::button::flex`]; o `ResizeObserver` do
//!   original é uma chamada a [`Radio::layout`] depois de trocar a largura ([`Radio::set_width`]). O reservar
//!   a largura do rótulo em negrito (`[data-label]::after`) é o `reserve_weight` da caixa do botão.
//! - As variáveis `--vscode-spacing-*`, `cornerRadius-*`, `fontSize-label2`, `fontWeight-*` e
//!   `strokeThickness` são [`RadioMetrics`] (os valores que o teste do original escreve no `cssText`); as
//!   cores que o CSS lê por variável (`foreground`, `toolbar.hoverBackground`, `contrastBorder`,
//!   `contrastActiveBorder`, `focusBorder`) são campos de [`RadioStyles`].
//! - O arredondar `offsetLeft`/`offsetTop`/`offsetWidth`/`offsetHeight` do botão ativo é o retângulo do nó
//!   do botão, e o `transform`/`width`/`height` do indicador é o retângulo do nó do indicador.
//!
//! Fica de fora, por enquanto: a animação do indicador (`transition`, a escala de `:active`, a Web
//! Animations API do `whenSelectionAnimationSettles`: a árvore não anima, então a espera termina na hora), as
//! regras de alto contraste do `.hc-black`/`.hc-light` e o `:focus-visible` (o contorno aparece em todo
//! foco).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event as BaseEvent};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, none, to_disposable};
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Border, EventKind, NodeId, Role, Ui};

use crate::button::flex::{FlexItem, resolve_flexible_lengths};
use crate::button::{Button,ButtonBox, ButtonCss, ButtonOptions, ButtonStyles, ButtonTitle, IButton};
use crate::common::{registered_emitter, throw};
use crate::dom::{StandardKeyboardEvent, create_detached_el, create_el};
use crate::hover_api::{IHoverDelegate, create_instant_hover_delegate};
use crate::text::UI_FONT_SIZE;

/// `IRadioStyles`, com os campos a mais que o CSS lê por variável.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RadioStyles {
    pub active_foreground: Option<Rgba>,
    pub active_background: Option<Rgba>,
    pub active_border: Option<Rgba>,
    pub inactive_foreground: Option<Rgba>,
    pub inactive_background: Option<Rgba>,
    pub inactive_hover_background: Option<Rgba>,
    pub inactive_border: Option<Rgba>,
    /// `--vscode-foreground`.
    pub foreground: Option<Rgba>,
    /// `--vscode-toolbar-hoverBackground`.
    pub toolbar_hover_background: Option<Rgba>,
    /// `--vscode-contrastBorder` (só nos temas de alto contraste).
    pub contrast_border: Option<Rgba>,
    /// `--vscode-contrastActiveBorder` (só nos temas de alto contraste).
    pub contrast_active_border: Option<Rgba>,
    pub focus_border: Option<Rgba>,
}

/// `defaultRadioStyles` de `defaultStyles.ts`, mais as cores que o `radio.css` lê direto.
pub fn default_radio_styles(theme: &ColorTheme) -> RadioStyles {
    RadioStyles {
        active_foreground: theme.get_color("radio.activeForeground"),
        active_background: theme.get_color("radio.activeBackground"),
        active_border: theme.get_color("radio.activeBorder"),
        inactive_foreground: theme.get_color("radio.inactiveForeground"),
        inactive_background: theme.get_color("radio.inactiveBackground"),
        inactive_border: theme.get_color("radio.inactiveBorder"),
        inactive_hover_background: theme.get_color("radio.inactiveHoverBackground"),
        foreground: theme.get_color("foreground"),
        toolbar_hover_background: theme.get_color("toolbar.hoverBackground"),
        contrast_border: theme.get_color("contrastBorder"),
        contrast_active_border: theme.get_color("contrastActiveBorder"),
        focus_border: theme.get_color("focusBorder"),
    }
}

/// As variáveis de design (`--vscode-spacing-*`, `cornerRadius-*`, `fontSize-label2`, `fontWeight-*`,
/// `strokeThickness`), com os valores do teste do original.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RadioMetrics {
    pub spacing_none: f64,
    pub spacing_20: f64,
    pub spacing_60: f64,
    pub spacing_240: f64,
    pub stroke_thickness: f64,
    pub corner_radius_small: f64,
    pub corner_radius_medium: f64,
    pub font_size_label2: f64,
    pub font_weight_regular: u16,
    pub font_weight_semi_bold: u16,
}

impl Default for RadioMetrics {
    fn default() -> Self {
        Self {
            spacing_none: 0.0,
            spacing_20: 2.0,
            spacing_60: 6.0,
            spacing_240: 24.0,
            stroke_thickness: 1.0,
            corner_radius_small: 4.0,
            corner_radius_medium: 6.0,
            font_size_label2: 11.0,
            font_weight_regular: 400,
            font_weight_semi_bold: 600,
        }
    }
}

/// `IRadioOptionItem`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RadioOptionItem {
    pub text: String,
    pub tooltip: Option<String>,
    /// Accessible name. Defaults to `tooltip ?? text`. Set it when `text` is icon-only.
    pub aria_label: Option<String>,
    pub is_active: bool,
    pub disabled: bool,
    /// Starts a new row of a segmented radio, for items that don't fit beside the ones before them.
    pub starts_row: bool,
}

impl RadioOptionItem {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), ..Self::default() }
    }
}

/// O `arrowKeyBehavior`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArrowKeyBehavior {
    /// Moves focus and selects, matching the ARIA radiogroup pattern.
    #[default]
    Select,
    /// Only moves focus, and Enter or Space selects.
    Focus,
}

/// `IRadioOptions`, com `styles` e `metrics` a mais.
#[derive(Clone, Default)]
pub struct RadioOptions {
    pub items: Vec<RadioOptionItem>,
    pub active_icon: Option<ThemeIcon>,
    pub hover_delegate: Option<Rc<dyn IHoverDelegate>>,
    /// Accessible name of the radio group.
    pub aria_label: Option<String>,
    /// Extra class added to the group, e.g. `segmented` for rounded-rectangle segments.
    pub class_name: Option<String>,
    pub arrow_key_behavior: ArrowKeyBehavior,
    pub styles: RadioStyles,
    pub metrics: RadioMetrics,
}

/// O retângulo do indicador de seleção (`transform`, `width` e `height`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionBounds {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// Um item de uma linha do contêiner flex: um botão (pelo índice) ou uma quebra de linha.
enum Slot {
    Button(usize),
    Break(NodeId),
}

/// A opção: o botão, o item e o que o descarta.
struct RadioButton {
    button: Rc<Button>,
    item: RadioOptionItem,
    row_break: Option<NodeId>,
    store: Rc<DisposableStore>,
}

/// O grupo de rádio.
pub struct Radio {
    base: Rc<Disposable>,
    weak_self: Weak<Radio>,
    ui: Ui,
    dom_node: NodeId,
    hover_delegate: Rc<dyn IHoverDelegate>,
    arrow_key_behavior: ArrowKeyBehavior,
    styles: RadioStyles,
    metrics: RadioMetrics,
    segmented: bool,
    items: RefCell<Vec<RadioOptionItem>>,
    active_item: Cell<Option<usize>>,
    buttons: RefCell<Vec<RadioButton>>,
    selection_indicator: Option<NodeId>,
    selection_indicator_bounds: Cell<Option<SelectionBounds>>,
    width: Cell<Option<f64>>,
    on_did_select: Rc<Emitter<usize>>,
    on_did_activate: Rc<Emitter<usize>>,
    disposed: Cell<bool>,
}

impl Radio {
    pub fn new(ui: &Ui, opts: RadioOptions) -> Rc<Radio> {
        let base = Disposable::new();
        let hover_delegate: Rc<dyn IHoverDelegate> = match &opts.hover_delegate {
            Some(delegate) => delegate.clone(),
            None => {
                let delegate = create_instant_hover_delegate();
                base.register(delegate.clone());
                delegate
            }
        };
        let mut class_name = "monaco-custom-radio".to_owned();
        if let Some(extra) = &opts.class_name {
            class_name.push(' ');
            class_name.push_str(extra);
        }
        let segmented = class_name.split_whitespace().any(|c| c == "segmented");
        let (dom_node, selection_indicator) = {
            let mut tree = ui.tree_mut();
            let dom_node = create_detached_el(&mut tree, Role::RadioGroup, &class_name);
            let indicator = segmented.then(|| {
                let node = create_el(&mut tree, dom_node, Role::Presentation, "monaco-radio-selection");
                tree.style_mut(node).visible = false;
                tree.set_attribute(node, "aria-hidden", "true");
                node
            });
            (dom_node, indicator)
        };
        let this = Rc::new_cyclic(|weak| Radio {
            weak_self: weak.clone(),
            ui: ui.clone(),
            dom_node,
            hover_delegate,
            arrow_key_behavior: opts.arrow_key_behavior,
            styles: opts.styles,
            metrics: opts.metrics,
            segmented,
            items: RefCell::new(Vec::new()),
            active_item: Cell::new(None),
            buttons: RefCell::new(Vec::new()),
            selection_indicator,
            selection_indicator_bounds: Cell::new(None),
            width: Cell::new(None),
            on_did_select: registered_emitter(&base),
            on_did_activate: registered_emitter(&base),
            disposed: Cell::new(false),
            base,
        });
        if let Some(label) = &opts.aria_label {
            this.ui.tree_mut().set_attribute(dom_node, "aria-label", label.clone());
        }
        this.set_items(opts.items);
        this
    }

    pub fn dom_node(&self) -> NodeId {
        self.dom_node
    }

    pub fn attribute(&self, name: &str) -> Option<String> {
        self.ui.tree().attribute(self.dom_node, name).map(str::to_owned)
    }

    pub fn on_did_select(&self) -> BaseEvent<usize> {
        self.on_did_select.event()
    }

    /// Fires on click, Enter, or Space, even when the item is already selected.
    pub fn on_did_activate(&self) -> BaseEvent<usize> {
        self.on_did_activate.event()
    }

    /// The option buttons in item order, for callers that need to measure or decorate them.
    pub fn option_elements(&self) -> Vec<NodeId> {
        self.buttons.borrow().iter().map(|b| b.button.element()).collect()
    }

    /// Os botões das opções, na ordem dos itens.
    pub fn option_buttons(&self) -> Vec<Rc<Button>> {
        self.buttons.borrow().iter().map(|b| b.button.clone()).collect()
    }

    /// O nó `.monaco-radio-selection` (só no segmentado).
    pub fn selection_indicator(&self) -> Option<NodeId> {
        self.selection_indicator
    }

    /// Um atributo do indicador (o `aria-hidden` que o construtor escreve).
    pub fn selection_indicator_attribute(&self, name: &str) -> Option<String> {
        self.selection_indicator.and_then(|node| self.ui.tree().attribute(node, name).map(str::to_owned))
    }

    /// O retângulo em que o indicador está (o `transform`, `width` e `height` do original).
    pub fn selection_indicator_bounds(&self) -> Option<SelectionBounds> {
        self.selection_indicator_bounds.get()
    }

    /// Shows `text` on an option until the returned disposable puts the item's own label back.
    pub fn override_option_label(&self, index: usize, text: &str) -> DisposableRef {
        let (button, item_text) = {
            let buttons = self.buttons.borrow();
            let Some(entry) = buttons.get(index) else { return none() };
            (entry.button.clone(), entry.item.text.clone())
        };
        self.set_button_label(&button, text);
        let weak = self.weak_self.clone();
        to_disposable(move || {
            if let Some(radio) = weak.upgrade() {
                radio.set_button_label(&button, &item_text);
            }
        })
    }

    pub fn set_items(&self, items: Vec<RadioOptionItem>) {
        let old: Vec<RadioButton> = self.buttons.borrow_mut().drain(..).collect();
        for entry in old {
            entry.store.dispose();
            if let Some(row_break) = entry.row_break {
                self.ui.remove(row_break);
            }
        }
        self.selection_indicator_bounds.set(None);
        {
            let mut tree = self.ui.tree_mut();
            tree.remove_class(self.dom_node, "selection-indicator-ready");
            tree.remove_class(self.dom_node, "animate-selection");
            if let Some(indicator) = self.selection_indicator {
                tree.style_mut(indicator).visible = false;
            }
            let multi_row = self.segmented && items.iter().enumerate().any(|(index, item)| index > 0 && item.starts_row);
            tree.toggle_class(self.dom_node, "multi-row", multi_row);
        }
        self.active_item.set(if items.is_empty() { None } else { Some(items.iter().position(|item| item.is_active).unwrap_or(0)) });
        *self.items.borrow_mut() = items.clone();
        for (index, item) in items.into_iter().enumerate() {
            let entry = self.create_option(index, item);
            self.buttons.borrow_mut().push(entry);
        }
        self.update_buttons(false);
    }

    fn create_option(&self, index: usize, item: RadioOptionItem) -> RadioButton {
        let store = DisposableStore::new();
        let row_break = (self.segmented && index > 0 && item.starts_row).then(|| create_el(&mut self.ui.tree_mut(), self.dom_node, Role::Presentation, "monaco-radio-row-break"));
        let button = Button::new(
            &self.ui,
            self.dom_node,
            ButtonOptions {
                hover_delegate: Some(self.hover_delegate.clone()),
                title: item.tooltip.clone().map_or(ButtonTitle::None, ButtonTitle::Text),
                aria_label: item.aria_label.clone(),
                support_icons: true,
                styles: ButtonStyles { focus_border: self.styles.focus_border, ..ButtonStyles::default() },
                ..ButtonOptions::default()
            },
        );
        store.add_rc(button.clone());
        self.ui.tree_mut().set_role(button.element(), Role::Radio);
        let weak = self.weak_self.clone();
        let css: ButtonCss = Rc::new(move |button, b| {
            if let Some(radio) = weak.upgrade() {
                radio.style_button(index, button, b);
            }
        });
        button.set_css(Some(css));
        button.set_enabled(!item.disabled);
        let weak = self.weak_self.clone();
        store.add(button.on_did_click().listen(move |_| {
            if let Some(radio) = weak.upgrade() {
                radio.select_item(index);
                radio.on_did_activate.fire(index);
            }
        }));
        let weak = self.weak_self.clone();
        store.add(self.ui.listen(button.element(), EventKind::KeyDown, false, move |e| {
            let Some(event) = StandardKeyboardEvent::from_event(e) else { return };
            let delta = if event.equals(KeyCode::RightArrow.bits()) || event.equals(KeyCode::DownArrow.bits()) {
                1
            } else if event.equals(KeyCode::LeftArrow.bits()) || event.equals(KeyCode::UpArrow.bits()) {
                -1
            } else {
                0
            };
            if delta == 0 {
                return;
            }
            e.stop(true);
            if let Some(radio) = weak.upgrade() {
                radio.navigate(index, delta);
            }
        }));
        RadioButton { button, item, row_break, store }
    }

    /// `setActiveItem`: lança `Invalid Index` fora do intervalo.
    pub fn set_active_item(&self, index: isize) {
        let len = self.items.borrow().len();
        if index < 0 || index as usize >= len {
            throw("Invalid Index");
        }
        let index = index as usize;
        if self.selection_indicator_bounds.get().is_none() {
            self.layout_selection_indicator(false);
        }
        let changed = self.active_item.get() != Some(index);
        self.active_item.set(Some(index));
        self.update_buttons(changed);
    }

    /// Waits for selection motion. A árvore não anima: o indicador pula, e a espera termina na hora.
    pub fn when_selection_animation_settles(&self) -> Promise<()> {
        Promise::resolved(())
    }

    pub fn set_enabled(&self, enabled: bool) {
        for entry in self.buttons.borrow().iter() {
            entry.button.set_enabled(enabled);
        }
        self.update_buttons(false);
    }

    /// Moves focus to the active item, for callers that rebuild the row after a selection.
    pub fn focus_active_item(&self) {
        if let Some(index) = self.active_item.get() {
            self.focus_item(index as isize);
        }
    }

    /// Moves focus to an item without selecting it.
    pub fn focus_item(&self, index: isize) {
        let buttons = self.option_buttons();
        if index < 0 || index as usize >= buttons.len() {
            throw("Invalid Index");
        }
        for (candidate, button) in buttons.iter().enumerate() {
            button.set_tab_index(if candidate == index as usize { 0 } else { -1 });
        }
        buttons[index as usize].focus();
    }

    fn select_item(&self, index: usize) {
        if index >= self.items.borrow().len() || self.active_item.get() == Some(index) {
            return;
        }
        self.set_active_item(index as isize);
        self.on_did_select.fire(index);
    }

    /// Moves to the next enabled item in `delta` direction, wrapping around the ends.
    fn navigate(&self, from: usize, delta: isize) {
        let count = self.items.borrow().len() as isize;
        for offset in 1..=count {
            let index = (((from as isize + delta * offset) % count) + count) % count;
            if self.items.borrow()[index as usize].disabled {
                continue;
            }
            if self.arrow_key_behavior == ArrowKeyBehavior::Select {
                self.select_item(index as usize);
            }
            self.focus_item(index);
            return;
        }
    }

    fn set_button_label(&self, button: &Rc<Button>, text: &str) {
        button.set_label(text.into());
        if self.segmented {
            let texts: Vec<String> = button.label_parts().into_iter().filter(|p| p.kind == crate::button::LabelPartKind::Text).map(|p| p.text).collect();
            for (index, text) in texts.iter().enumerate() {
                button.set_data_label(index, Some(text));
            }
        }
    }

    fn update_buttons(&self, animate: bool) {
        let mut is_active = false;
        let entries: Vec<(Rc<Button>, String)> = self.buttons.borrow().iter().map(|b| (b.button.clone(), b.item.text.clone())).collect();
        for (index, (button, text)) in entries.iter().enumerate() {
            let is_previous_active = is_active;
            is_active = self.active_item.get() == Some(index);
            {
                let mut tree = self.ui.tree_mut();
                tree.toggle_class(button.element(), "active", is_active);
                tree.toggle_class(button.element(), "previous-active", is_previous_active);
            }
            button.set_attribute("aria-checked", is_active.to_string());
            button.set_tab_index(if is_active { 0 } else { -1 });
            self.set_button_label(button, text);
            button.refresh();
        }
        self.layout_buttons();
        self.layout_selection_indicator(animate);
    }

    /// A folha de estilo do `.monaco-custom-radio > .monaco-button`, para o botão `index`.
    pub fn style_button(&self, index: usize, button: &Button, b: &mut ButtonBox) {
        let (active, previous_active, ready) = {
            let tree = self.ui.tree();
            (tree.has_class(button.element(), "active"), tree.has_class(button.element(), "previous-active"), tree.has_class(self.dom_node, "selection-indicator-ready"))
        };
        let hovered = button.is_hovered();
        let last = index + 1 == self.items.borrow().len();
        let (s, m) = (&self.styles, &self.metrics);
        // .monaco-custom-radio > .monaco-button
        b.radii = [0.0; 4];
        if !self.segmented && index == 0 {
            b.radii[0] = 3.0;
            b.radii[3] = 3.0;
        }
        if !self.segmented && last {
            b.radii[1] = 3.0;
            b.radii[2] = 3.0;
        }
        b.font_size = 0.9 * f64::from(UI_FONT_SIZE);
        b.line_height = b.font_size;
        b.padding[1] = 0.5 * b.font_size;
        b.padding[3] = 0.5 * b.font_size;
        if !self.segmented {
            if !active && !last {
                b.border_widths[1] = 0.0;
            }
            if previous_active {
                b.border_widths[3] = 0.0;
            }
        }
        // `color: var(--vscode-radio-inactiveForeground)` sem a variável herda a cor do pai.
        b.foreground = s.inactive_foreground.or(s.foreground);
        b.background = s.inactive_background;
        b.border_color = s.inactive_border;
        if hovered && !active {
            b.background = s.inactive_hover_background;
        }
        if active {
            b.foreground = s.active_foreground.or(s.foreground);
            b.background = s.active_background;
            b.border_color = s.active_border;
        }
        if self.segmented {
            // .monaco-custom-radio.segmented > .monaco-button
            b.height = Some(m.spacing_240);
            b.padding = [m.spacing_none, m.spacing_60, m.spacing_none, m.spacing_60];
            b.border_widths = [m.stroke_thickness; 4];
            b.border_color = None;
            b.radii = [m.corner_radius_small; 4];
            b.foreground = s.foreground;
            b.background = None;
            b.font_size = m.font_size_label2;
            b.line_height = b.font_size;
            b.font_weight = m.font_weight_regular;
            b.reserve_weight = Some(m.font_weight_semi_bold);
            b.outline_offset = -m.stroke_thickness;
            if hovered && !active {
                b.background = s.toolbar_hover_background;
            }
            if active {
                b.background = s.foreground.map(|c| Rgba { a: (f32::from(c.a) * 0.10).round() as u8, ..c });
                b.border_color = s.contrast_active_border;
                b.font_weight = m.font_weight_semi_bold;
                if ready {
                    b.background = None;
                    b.border_color = None;
                }
            }
        }
    }

    /// `style.width` do grupo: `None` é a largura do conteúdo (`position: absolute` sem largura).
    pub fn set_width(&self, width: Option<f64>) {
        self.width.set(width);
        self.layout();
    }

    /// `style.left` e `style.top`.
    pub fn set_position(&self, x: f64, y: f64) {
        let mut tree = self.ui.tree_mut();
        let r = tree.bounds(self.dom_node);
        tree.set_bounds(self.dom_node, Rect::new(x, y, x + r.width(), y + r.height()));
    }

    /// O `ResizeObserver` do original: refaz o layout e o indicador depois de uma mudança de tamanho.
    pub fn layout(&self) {
        self.layout_buttons();
        self.layout_selection_indicator(false);
    }

    /// O `display: flex` do grupo.
    fn layout_buttons(&self) {
        let m = &self.metrics;
        let (border, padding, gap) = if self.segmented { (m.stroke_thickness, m.spacing_20, m.spacing_20) } else { (0.0, 0.0, 0.0) };
        let frame = 2.0 * (border + padding);
        let buttons = self.option_buttons();
        let row_breaks: Vec<Option<NodeId>> = self.buttons.borrow().iter().map(|b| b.row_break).collect();
        for button in &buttons {
            button.set_height(None);
            button.set_width(None);
        }
        let min_outer: Vec<f64> = buttons.iter().map(|b| b.min_content_width()).collect();
        let max_content = min_outer.iter().sum::<f64>() + gap * buttons.len().saturating_sub(1) as f64;
        let inner_width = self.width.get().map_or(max_content, |w| w - frame);
        let wraps = self.ui.tree().has_class(self.dom_node, "multi-row");
        let break_height = m.spacing_20;
        // As linhas do contêiner: uma quebra (`flex-basis: 100%`) fica sozinha numa linha.
        let mut lines: Vec<Vec<Slot>> = vec![Vec::new()];
        let mut used = 0.0;
        for (index, row_break) in row_breaks.iter().enumerate() {
            if let Some(node) = row_break {
                lines.push(vec![Slot::Break(*node)]);
                lines.push(Vec::new());
                used = 0.0;
            }
            let hypothetical = min_outer[index];
            let line = lines.last_mut().expect("sempre há uma linha");
            let needed = used + if line.is_empty() { 0.0 } else { gap } + hypothetical;
            if wraps && !line.is_empty() && needed > inner_width + 1e-9 {
                lines.push(vec![Slot::Button(index)]);
                used = hypothetical;
            } else {
                line.push(Slot::Button(index));
                used = needed;
            }
        }
        lines.retain(|l| !l.is_empty());
        let slot_height = |slot: &Slot| match slot {
            Slot::Button(index) => buttons[*index].size().1,
            Slot::Break(_) => break_height,
        };
        let mut y = border + padding;
        for line in &lines {
            let flex: Vec<FlexItem> = line
                .iter()
                .map(|slot| match slot {
                    Slot::Button(index) => {
                        let extra = buttons[*index].compute_box().horizontal_extra();
                        let base = if self.segmented { extra } else { inner_width };
                        FlexItem { base, pad_border: extra, grow: if self.segmented { 1.0 } else { 0.0 }, shrink: 1.0, min: min_outer[*index] }
                    }
                    Slot::Break(_) => FlexItem { base: inner_width, pad_border: 0.0, grow: 0.0, shrink: 1.0, min: 0.0 },
                })
                .collect();
            let available = inner_width - gap * line.len().saturating_sub(1) as f64;
            let widths = resolve_flexible_lengths(&flex, available);
            // `align-items: stretch` (junto) ou `center` (segmentado): a altura da linha é a maior.
            let natural: Vec<f64> = line.iter().map(slot_height).collect();
            let line_height = natural.iter().copied().fold(0.0, f64::max);
            let mut x = border + padding;
            for ((slot, width), height) in line.iter().zip(&widths).zip(&natural) {
                match slot {
                    Slot::Button(index) => {
                        let button = &buttons[*index];
                        let height = if self.segmented { *height } else { line_height };
                        button.set_height(if self.segmented { None } else { Some(height) });
                        button.set_position(x, y + (line_height - height) / 2.0);
                        button.set_width(Some(*width));
                    }
                    Slot::Break(node) => self.ui.tree_mut().set_bounds(*node, Rect::new(x, y, x + width, y + break_height)),
                }
                x += width + gap;
            }
            y += line_height;
        }
        let height = y + border + padding;
        let width = self.width.get().unwrap_or(max_content + frame);
        let mut tree = self.ui.tree_mut();
        let r = tree.bounds(self.dom_node);
        tree.set_bounds(self.dom_node, Rect::new(r.x0, r.y0, r.x0 + width, r.y0 + height));
        let style = tree.style_mut(self.dom_node);
        if self.segmented {
            let s = &self.styles;
            style.border = Some(Border { width: m.stroke_thickness, color: s.contrast_border.or(s.inactive_border).unwrap_or(Rgba::TRANSPARENT) });
            style.radius = m.corner_radius_medium;
            style.background = None;
        }
    }

    /// `layoutSelectionIndicator`.
    fn layout_selection_indicator(&self, animate: bool) {
        let Some(indicator) = self.selection_indicator else { return };
        let Some(button) = self.active_item.get().and_then(|i| self.buttons.borrow().get(i).map(|b| b.button.clone())) else { return };
        let r = self.ui.tree().bounds(button.element());
        let bounds = SelectionBounds { left: r.x0, top: r.y0, width: r.width(), height: r.height() };
        if bounds.width == 0.0 || bounds.height == 0.0 {
            return;
        }
        let previous = self.selection_indicator_bounds.get();
        if previous == Some(bounds) {
            return;
        }
        // Sliding diagonally between rows reads poorly, so a move to another row jumps.
        let was_ready = self.ui.tree().has_class(self.dom_node, "selection-indicator-ready");
        {
            let mut tree = self.ui.tree_mut();
            tree.toggle_class(self.dom_node, "animate-selection", animate && previous.is_some_and(|p| p.top == bounds.top));
            tree.add_class(self.dom_node, "selection-indicator-ready");
            tree.set_bounds(indicator, Rect::new(bounds.left, bounds.top, bounds.left + bounds.width, bounds.top + bounds.height));
        }
        self.selection_indicator_bounds.set(Some(bounds));
        self.style_indicator();
        if !was_ready {
            // As regras do `.active` mudam quando o indicador aparece.
            for entry in self.buttons.borrow().iter() {
                entry.button.refresh();
            }
        }
    }

    /// `.monaco-custom-radio.segmented > .monaco-radio-selection`.
    fn style_indicator(&self) {
        let Some(indicator) = self.selection_indicator else { return };
        let (s, m) = (&self.styles, &self.metrics);
        let mut tree = self.ui.tree_mut();
        let ready = tree.has_class(self.dom_node, "selection-indicator-ready");
        let style = tree.style_mut(indicator);
        style.visible = ready;
        style.background = s.foreground.map(|c| Rgba { a: (f32::from(c.a) * 0.10).round() as u8, ..c });
        style.border = Some(Border { width: m.stroke_thickness, color: s.contrast_active_border.unwrap_or(Rgba::TRANSPARENT) });
        style.radius = m.corner_radius_small;
        style.pointer_events = je_ui::PointerEvents::None;
    }
}

impl IDisposable for Radio {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        let entries: Vec<RadioButton> = self.buttons.borrow_mut().drain(..).collect();
        for entry in entries {
            entry.store.dispose();
        }
        self.base.dispose();
        self.ui.remove(self.dom_node);
    }

    fn constructor_name(&self) -> &'static str {
        "Radio"
    }
}
