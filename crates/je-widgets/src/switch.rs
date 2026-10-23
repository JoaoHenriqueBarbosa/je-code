//! Porte de `src/vs/base/browser/ui/toggle/switch.ts` e `switch.css`: o [`Switch`], a chave em forma de
//! pílula para uma opção que vale assim que é virada (ligar um plugin, deixar o modelo ser escolhido
//! sozinho).
//!
//! Como o DOM e o CSS viram [`je_ui`]:
//!
//! - O `<button class="monaco-switch">` é um nó (`Role::Switch`) com as classes `monaco-switch` e `checked`, e o
//!   `.monaco-switch-thumb` um nó filho. Trilho de `28px` por `16px` (`--vscode-spacing-size280` e `size160`),
//!   borda de `1px` e raio de pílula (`--vscode-cornerRadius-circle`).
//! - O polegar preenche a altura interna do trilho (`width: 100cqh`, `14px`) e a borda transparente dele
//!   (`1px`, com `background-clip: padding-box`) deixa um disco de `12px`; marcado, `left` vai para
//!   `calc(100% - 100cqh)`. A cor do trilho desmarcado é `color-mix(in srgb, descriptionForeground 36%,
//!   transparent)`, ou seja, a mesma cor com o alfa multiplicado por `0.36`.
//! - `:disabled` é `opacity: 0.5` e `cursor: default`; `:focus-visible` é o contorno de `1px` com
//!   `outline-offset: 2px` (`--vscode-spacing-size20`). `transition`: instantâneo (a árvore não anima).
//! - O `<button>` nativo ativa com Enter e Space; aqui o `keydown` de Enter ou Space faz o mesmo que o clique.
//!
//! O original não tem um tipo de estilos: as cores são variáveis CSS (`descriptionForeground`,
//! `button.background`, `button.foreground`, `focusBorder`). [`SwitchStyles`] e [`default_switch_styles`]
//! seguem o padrão do resto do módulo. O `.hc-black .monaco-switch` (borda `contrastBorder` em alto
//! contraste) fica de fora.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event as BaseEvent};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, IDisposable};
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Border, Cursor, EventKind, NodeId, Role, Ui};

use crate::button::{Attributes, OutlineSpec, update_outline};
use crate::common::registered_emitter;
use crate::dom::{StandardKeyboardEvent, create_el};
use crate::hover_api::{HoverContent, HoverStyle, HoverTarget, IHoverOptions, get_base_layer_hover_delegate};

/// `--vscode-spacing-size280`.
const WIDTH: f64 = 28.0;
/// `--vscode-spacing-size160`.
const HEIGHT: f64 = 16.0;
/// `--vscode-strokeThickness`.
const STROKE: f64 = 1.0;

/// As cores que o `switch.css` lê por variável.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SwitchStyles {
    /// `--vscode-descriptionForeground`: o trilho desmarcado é essa cor a 36%.
    pub description_foreground: Option<Rgba>,
    /// `--vscode-button-background`: o trilho marcado.
    pub button_background: Option<Rgba>,
    /// `--vscode-button-foreground`: o polegar.
    pub button_foreground: Option<Rgba>,
    pub focus_border: Option<Rgba>,
}

/// As chaves de tema que o `switch.css` referencia.
pub fn default_switch_styles(theme: &ColorTheme) -> SwitchStyles {
    SwitchStyles {
        description_foreground: theme.get_color("descriptionForeground"),
        button_background: theme.get_color("button.background"),
        button_foreground: theme.get_color("button.foreground"),
        focus_border: theme.get_color("focusBorder"),
    }
}

/// `ISwitchOptions`.
#[derive(Clone, Debug, Default)]
pub struct SwitchOptions {
    pub checked: bool,
    /// Accessible name. Also used as the hover title unless `title` is given.
    pub aria_label: String,
    /// Hover title. Defaults to `aria_label`.
    pub title: Option<String>,
    pub disabled: bool,
}

/// A chave.
pub struct Switch {
    base: Rc<Disposable>,
    weak_self: Weak<Switch>,
    ui: Ui,
    styles: SwitchStyles,
    dom_node: NodeId,
    thumb: NodeId,
    checked: Cell<bool>,
    disabled: Cell<bool>,
    title: Rc<RefCell<String>>,
    attributes: Attributes,
    focused: Cell<bool>,
    outline: Cell<Option<NodeId>>,
    on_change: Rc<Emitter<bool>>,
    disposed: Cell<bool>,
}

impl Switch {
    pub fn new(ui: &Ui, options: SwitchOptions, styles: SwitchStyles) -> Rc<Switch> {
        let base = Disposable::new();
        let (dom_node, thumb) = {
            let mut tree = ui.tree_mut();
            let dom_node = tree.create_detached(Role::Switch);
            tree.add_class(dom_node, "monaco-switch");
            let thumb = create_el(&mut tree, dom_node, Role::Presentation, "monaco-switch-thumb");
            (dom_node, thumb)
        };
        let title = Rc::new(RefCell::new(options.title.clone().unwrap_or_else(|| options.aria_label.clone())));
        {
            let title = title.clone();
            let hover_options: Rc<dyn Fn() -> IHoverOptions> =
                Rc::new(move || IHoverOptions { style: Some(HoverStyle::Pointer), ..IHoverOptions::new(HoverContent::Text(title.borrow().clone()), HoverTarget::Node(dom_node)) });
            base.register(get_base_layer_hover_delegate().setup_delayed_hover(dom_node, hover_options, None));
        }
        let this = Rc::new_cyclic(|weak| Switch {
            weak_self: weak.clone(),
            ui: ui.clone(),
            styles,
            dom_node,
            thumb,
            checked: Cell::new(options.checked),
            disabled: Cell::new(false),
            title,
            attributes: Attributes::default(),
            focused: Cell::new(false),
            outline: Cell::new(None),
            on_change: registered_emitter(&base),
            disposed: Cell::new(false),
            base,
        });
        this.attributes.set("type", "button");
        this.set_aria_label(&options.aria_label, options.title.as_deref());
        this.set_disabled(options.disabled);
        this.apply_state();

        this.listen(EventKind::Click, |switch, e| {
            e.stop(true);
            switch.flip();
        });
        this.listen(EventKind::KeyDown, |switch, e| {
            let Some(event) = StandardKeyboardEvent::from_event(e) else { return };
            if event.key_code == KeyCode::Enter || event.key_code == KeyCode::Space {
                e.stop(true);
                switch.flip();
            }
        });
        this.listen(EventKind::Focus, |switch, _| {
            switch.focused.set(true);
            switch.refresh();
        });
        this.listen(EventKind::Blur, |switch, _| {
            switch.focused.set(false);
            switch.refresh();
        });
        this
    }

    fn listen(&self, kind: EventKind, handler: impl Fn(&Rc<Switch>, &mut je_ui::Event) + 'static) {
        let weak = self.weak_self.clone();
        self.base.register(self.ui.listen(self.dom_node, kind, false, move |e| {
            if let Some(switch) = weak.upgrade() {
                handler(&switch, e);
            }
        }));
    }

    fn flip(&self) {
        if self.disabled.get() {
            return;
        }
        self.checked.set(!self.checked.get());
        self.apply_state();
        self.on_change.fire(self.checked.get());
    }

    pub fn dom_node(&self) -> NodeId {
        self.dom_node
    }

    /// Fires with the new state when the user flips the switch, not when it is set in code.
    pub fn on_change(&self) -> BaseEvent<bool> {
        self.on_change.event()
    }

    pub fn checked(&self) -> bool {
        self.checked.get()
    }

    /// Sets the state without firing `onChange`.
    pub fn set_checked(&self, checked: bool) {
        if self.checked.get() != checked {
            self.checked.set(checked);
            self.apply_state();
        }
    }

    pub fn disabled(&self) -> bool {
        self.disabled.get()
    }

    pub fn set_disabled(&self, disabled: bool) {
        self.disabled.set(disabled);
        self.refresh();
    }

    /// O `tabIndex` de um `<button>`: `0`, ou `-1` quando desabilitado (não recebe foco).
    pub fn tab_index(&self) -> i32 {
        if self.disabled.get() { -1 } else { 0 }
    }

    pub fn attribute(&self, name: &str) -> Option<String> {
        self.attributes.get(name)
    }

    /// `setAriaLabel(ariaLabel, title = ariaLabel)`.
    pub fn set_aria_label(&self, aria_label: &str, title: Option<&str>) {
        self.attributes.set("aria-label", aria_label);
        self.ui.tree_mut().set_label(self.dom_node, aria_label);
        *self.title.borrow_mut() = title.unwrap_or(aria_label).to_owned();
    }

    /// A largura e a altura do trilho.
    pub fn size(&self) -> (f64, f64) {
        (WIDTH, HEIGHT)
    }

    /// `style.left` e `style.top`.
    pub fn set_position(&self, x: f64, y: f64) {
        self.ui.tree_mut().set_bounds(self.dom_node, Rect::new(x, y, x + WIDTH, y + HEIGHT));
        self.refresh();
    }

    fn apply_state(&self) {
        self.attributes.set("aria-checked", self.checked.get().to_string());
        self.ui.tree_mut().toggle_class(self.dom_node, "checked", self.checked.get());
        self.refresh();
    }

    /// As regras de `switch.css`.
    fn refresh(&self) {
        if self.disposed.get() {
            return;
        }
        let checked = self.checked.get();
        let track = if checked { self.styles.button_background } else { self.styles.description_foreground.map(|c| Rgba { a: (f32::from(c.a) * 0.36).round() as u8, ..c }) };
        // O polegar tem `100cqh` de largura, a altura interna do trilho, e a borda transparente dele fica
        // dentro: o disco é o miolo, a `STROKE` de cada lado.
        let inner = HEIGHT - 2.0 * STROKE;
        let thumb_left = if checked { WIDTH - 2.0 * STROKE - inner } else { 0.0 };
        let disabled = self.disabled.get();
        let outline = (self.focused.get() && !disabled).then_some(self.styles.focus_border).flatten();
        let mut tree = self.ui.tree_mut();
        let r = tree.bounds(self.dom_node);
        tree.set_bounds(self.dom_node, Rect::new(r.x0, r.y0, r.x0 + WIDTH, r.y0 + HEIGHT));
        let style = tree.style_mut(self.dom_node);
        style.background = track;
        style.border = (checked && self.styles.button_background.is_some()).then(|| Border { width: STROKE, color: self.styles.button_background.unwrap_or_default() });
        style.radius = HEIGHT / 2.0;
        style.opacity = if disabled { 0.5 } else { 1.0 };
        style.cursor = Some(if disabled { Cursor::Default } else { Cursor::Pointer });
        tree.set_bounds(self.thumb, Rect::new(STROKE + thumb_left + STROKE, STROKE + STROKE, STROKE + thumb_left + inner - STROKE, STROKE + inner - STROKE));
        let thumb = tree.style_mut(self.thumb);
        thumb.background = self.styles.button_foreground;
        thumb.radius = (inner - 2.0 * STROKE) / 2.0;
        update_outline(&mut tree, self.dom_node, &self.outline, outline.map(|color| OutlineSpec { color, offset: 2.0, radius: HEIGHT / 2.0 }));
    }
}

impl IDisposable for Switch {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.base.dispose();
        self.ui.remove(self.dom_node);
    }

    fn constructor_name(&self) -> &'static str {
        "Switch"
    }
}
