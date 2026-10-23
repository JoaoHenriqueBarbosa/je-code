//! Porte de `src/vs/base/browser/ui/toggle/toggle.ts` e `toggle.css`: o [`Toggle`] (o botão de dois
//! estados dos campos de busca, `monaco-custom-toggle`), o [`Checkbox`] e o [`TriStateCheckbox`]
//! (`monaco-checkbox`), com [`ToggleStyles`], [`CheckboxStyles`], [`unthemed_toggle_styles`],
//! [`default_toggle_styles`] e [`default_checkbox_styles`] (o `defaultStyles.ts`).
//!
//! Como o DOM e o CSS viram [`je_ui`]:
//!
//! - O `<div class="monaco-custom-toggle ...">` é um nó com as mesmas classes (`checked`, `disabled`,
//!   `monaco-checkbox`, `codicon codicon-check`, as do `actionClassName`). O ícone (a pseudo-regra `::before`
//!   do `.codicon`) é o `Content::Codicon` do próprio nó, centrado como o glifo de `16px/1`.
//! - `20px` por `20px`, borda de `1px` (`--vscode-strokeThickness`), raio `3px`, padding `1px` e `overflow:
//!   hidden`; o `.monaco-checkbox` é `18px`, sem padding. `float: left` e a `margin-left` são do dono: a
//!   posição é [`Toggle::set_position`] e [`Toggle::width`] devolve a largura que o original reserva.
//! - `:hover` é o `inputOption.hoverBackground`, `.disabled` é `opacity: 0.4` mais `pointer-events: none`, e
//!   `:focus-visible` (só no que não é checkbox) troca a borda pela de foco; o checkbox ganha o contorno global
//!   de foco, `outline-offset: -1px`. As cores que o CSS lê por variável (`inputOption.hoverBackground`,
//!   `focusBorder` e o `color` herdado) são campos de [`ToggleStyles`] e [`CheckboxStyles`].
//! - `tabIndex` e `aria-*` ficam no widget ([`Toggle::tab_index`], [`Toggle::attribute`]).
//!
//! Fica de fora, por enquanto: o `CheckboxActionViewItem`, o `ToggleActionViewItem` e o
//! `createToggleActionViewItemProvider` (dependem do `BaseActionViewItem` do actionbar: onda 2), o toque
//! (`ignoreGesture`), a borda tracejada do `:focus-visible` (a árvore só tem borda sólida) e o
//! `.hc-black`/`.hc-light` (o tema de alto contraste troca o fundo e a borda do hover).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event as BaseEvent};
use je_base::html_content::IMarkdownString;
use je_base::icon_labels::{get_codicon_aria_label, strip_icons};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Border, Content, Cursor, EventKind, NodeId, PointerEvents, Role, Ui};

use crate::button::{Attributes, INHERITED_FOREGROUND, OutlineSpec, update_outline};
use crate::common::registered_emitter;
use crate::dom::{StandardKeyboardEvent, create_detached_el};
use crate::hover_api::{HoverContent, HoverStyle, HoverTarget, IHoverLifecycleOptions, IHoverOptions, get_base_layer_hover_delegate};

/// `IToggleStyles`, com os campos a mais que o CSS lê por variável: `input_option_hover_background`
/// (`--vscode-inputOption-hoverBackground`), `focus_border` (`--vscode-focusBorder`) e `inherited_foreground`
/// (o `color: inherit` do toggle desmarcado).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToggleStyles {
    pub input_active_option_border: Option<Rgba>,
    pub input_active_option_foreground: Option<Rgba>,
    pub input_active_option_background: Option<Rgba>,
    pub input_option_hover_background: Option<Rgba>,
    pub focus_border: Option<Rgba>,
    pub inherited_foreground: Option<Rgba>,
}

/// `unthemedToggleStyles`.
pub fn unthemed_toggle_styles() -> ToggleStyles {
    ToggleStyles {
        input_active_option_border: Some(Rgba::new(0x00, 0x7A, 0xCC, 0x00)),
        input_active_option_foreground: Some(Rgba::opaque(0xFF, 0xFF, 0xFF)),
        input_active_option_background: Some(Rgba::new(0x0E, 0x63, 0x9C, 0x50)),
        ..ToggleStyles::default()
    }
}

/// `defaultToggleStyles` de `defaultStyles.ts`.
pub fn default_toggle_styles(theme: &ColorTheme) -> ToggleStyles {
    ToggleStyles {
        input_active_option_border: theme.get_color("inputOption.activeBorder"),
        input_active_option_foreground: theme.get_color("inputOption.activeForeground"),
        input_active_option_background: theme.get_color("inputOption.activeBackground"),
        input_option_hover_background: theme.get_color("inputOption.hoverBackground"),
        focus_border: theme.get_color("focusBorder"),
        inherited_foreground: theme.get_color("foreground"),
    }
}

/// `ICheckboxStyles`, com `focus_border` e `inherited_foreground` a mais.
#[derive(Clone, Default)]
pub struct CheckboxStyles {
    pub checkbox_background: Option<Rgba>,
    pub checkbox_border: Option<Rgba>,
    pub checkbox_foreground: Option<Rgba>,
    pub checkbox_disabled_background: Option<Rgba>,
    pub checkbox_disabled_foreground: Option<Rgba>,
    pub size: Option<f64>,
    pub hover_lifecycle_options: Option<IHoverLifecycleOptions>,
    pub focus_border: Option<Rgba>,
    pub inherited_foreground: Option<Rgba>,
}

/// `defaultCheckboxStyles` de `defaultStyles.ts`.
pub fn default_checkbox_styles(theme: &ColorTheme) -> CheckboxStyles {
    CheckboxStyles {
        checkbox_background: theme.get_color("checkbox.background"),
        checkbox_border: theme.get_color("checkbox.border"),
        checkbox_foreground: theme.get_color("checkbox.foreground"),
        checkbox_disabled_background: theme.get_color("checkbox.disabled.background"),
        checkbox_disabled_foreground: theme.get_color("checkbox.disabled.foreground"),
        size: None,
        hover_lifecycle_options: None,
        focus_border: theme.get_color("focusBorder"),
        inherited_foreground: theme.get_color("foreground"),
    }
}

/// `string | IMarkdownString | HTMLElement`: o título (e o hover) do toggle.
#[derive(Clone, Debug)]
pub enum ToggleTitle {
    Text(String),
    Markdown(IMarkdownString),
    /// Um elemento, com o `textContent` dele (o `je-ui` não lê texto dos filhos).
    Node(NodeId, String),
}

impl From<&str> for ToggleTitle {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

/// `IToggleOpts`.
#[derive(Clone)]
pub struct ToggleOpts {
    pub styles: ToggleStyles,
    pub action_class_name: Option<String>,
    pub icon: Option<ThemeIcon>,
    pub title: ToggleTitle,
    pub is_checked: bool,
    pub not_focusable: bool,
    pub hover_lifecycle_options: Option<IHoverLifecycleOptions>,
}

impl ToggleOpts {
    pub fn new(title: impl Into<ToggleTitle>, is_checked: bool, styles: ToggleStyles) -> Self {
        Self { styles, action_class_name: None, icon: None, title: title.into(), is_checked, not_focusable: false, hover_lifecycle_options: None }
    }
}

/// O que o `style` do `domNode` guarda e a folha de estilo não decide: o `Checkbox` o reescreve.
#[derive(Clone, Copy, Debug, Default)]
struct InlineStyle {
    /// `None` é `inherit` (ou a string vazia).
    color: Option<Rgba>,
    background: Option<Rgba>,
    border_color: Option<Rgba>,
    /// `width` e `height`.
    size: Option<f64>,
    font_size: Option<f64>,
}

/// O toggle.
pub struct Toggle {
    base: Rc<Disposable>,
    weak_self: Weak<Toggle>,
    ui: Ui,
    opts: ToggleOpts,
    dom_node: NodeId,
    title: Rc<RefCell<ToggleTitle>>,
    icon: RefCell<Option<ThemeIcon>>,
    /// As classes que o `actionClassName` trouxe, para achar o glifo (`codicon-*`) quando não há `icon`.
    action_classes: Vec<String>,
    checked: Cell<bool>,
    tab_index: Cell<i32>,
    attributes: Attributes,
    inline: Cell<InlineStyle>,
    /// A variável `--monaco-custom-toggle-border-color`.
    border_color_variable: Cell<Option<Rgba>>,
    pointer_over: Cell<bool>,
    focused: Cell<bool>,
    outline: Cell<Option<NodeId>>,
    on_change: Rc<Emitter<bool>>,
    on_key_down: Rc<Emitter<StandardKeyboardEvent>>,
    disposed: Cell<bool>,
}

impl Toggle {
    pub fn new(ui: &Ui, opts: ToggleOpts) -> Rc<Toggle> {
        let base = Disposable::new();
        let mut classes = vec!["monaco-custom-toggle".to_owned()];
        let icon = opts.icon.clone();
        if let Some(icon) = &icon {
            classes.extend(icon.as_class_name_array());
        }
        let action_classes: Vec<String> = opts.action_class_name.iter().flat_map(|c| c.split(' ').filter(|s| !s.is_empty()).map(str::to_owned)).collect();
        classes.extend(action_classes.iter().cloned());
        if opts.is_checked {
            classes.push("checked".to_owned());
        }
        let dom_node = create_detached_el(&mut ui.tree_mut(), Role::Checkbox, &classes.join(" "));
        let title = Rc::new(RefCell::new(opts.title.clone()));
        {
            let title = title.clone();
            let options: Rc<dyn Fn() -> IHoverOptions> = Rc::new(move || {
                let content = match &*title.borrow() {
                    ToggleTitle::Text(text) => HoverContent::Text(strip_icons(&text.encode_utf16().collect::<Vec<_>>()).to_string_lossy()),
                    ToggleTitle::Markdown(markdown) => HoverContent::Markdown(markdown.clone()),
                    ToggleTitle::Node(node, _) => HoverContent::Node(*node),
                };
                IHoverOptions { style: Some(HoverStyle::Pointer), ..IHoverOptions::new(content, HoverTarget::Node(dom_node)) }
            });
            base.register(get_base_layer_hover_delegate().setup_delayed_hover(dom_node, options, opts.hover_lifecycle_options.clone()));
        }
        let this = Rc::new_cyclic(|weak| Toggle {
            weak_self: weak.clone(),
            ui: ui.clone(),
            dom_node,
            title,
            icon: RefCell::new(icon),
            action_classes,
            checked: Cell::new(opts.is_checked),
            tab_index: Cell::new(if opts.not_focusable { -1 } else { 0 }),
            attributes: Attributes::default(),
            inline: Cell::new(InlineStyle::default()),
            border_color_variable: Cell::new(None),
            pointer_over: Cell::new(false),
            focused: Cell::new(false),
            outline: Cell::new(None),
            on_change: registered_emitter(&base),
            on_key_down: registered_emitter(&base),
            disposed: Cell::new(false),
            opts: opts.clone(),
            base,
        });
        this.attributes.set("aria-checked", this.checked.get().to_string());
        this.set_title(opts.title);
        this.apply_styles();

        this.listen(EventKind::Click, |toggle, e| {
            if toggle.enabled() {
                toggle.set_checked(!toggle.checked.get());
                toggle.on_change.fire(false);
                e.stop(true);
            }
        });
        this.listen(EventKind::KeyDown, |toggle, e| {
            if !toggle.enabled() {
                return;
            }
            let Some(event) = StandardKeyboardEvent::from_event(e) else { return };
            if event.key_code == KeyCode::Space || event.key_code == KeyCode::Enter {
                toggle.set_checked(!toggle.checked.get());
                toggle.on_change.fire(true);
                e.stop(true);
                return;
            }
            toggle.on_key_down.fire(event);
        });
        this.listen(EventKind::PointerEnter, |toggle, _| {
            toggle.pointer_over.set(true);
            toggle.refresh();
        });
        this.listen(EventKind::PointerLeave, |toggle, _| {
            toggle.pointer_over.set(false);
            toggle.refresh();
        });
        this.listen(EventKind::Focus, |toggle, _| {
            toggle.focused.set(true);
            toggle.refresh();
        });
        this.listen(EventKind::Blur, |toggle, _| {
            toggle.focused.set(false);
            toggle.refresh();
        });
        this.refresh();
        this
    }

    fn listen(&self, kind: EventKind, handler: impl Fn(&Rc<Toggle>, &mut je_ui::Event) + 'static) {
        let weak = self.weak_self.clone();
        self.base.register(self.ui.listen(self.dom_node, kind, false, move |e| {
            if let Some(toggle) = weak.upgrade() {
                handler(&toggle, e);
            }
        }));
    }

    pub fn dom_node(&self) -> NodeId {
        self.dom_node
    }

    pub fn ui(&self) -> &Ui {
        &self.ui
    }

    /// `onChange`: dispara com `true` quando a troca veio do teclado.
    pub fn on_change(&self) -> BaseEvent<bool> {
        self.on_change.event()
    }

    pub fn on_key_down(&self) -> BaseEvent<StandardKeyboardEvent> {
        self.on_key_down.event()
    }

    pub fn enabled(&self) -> bool {
        self.attributes.get("aria-disabled").as_deref() != Some("true")
    }

    pub fn focus(&self) {
        self.ui.focus(self.dom_node);
    }

    pub fn checked(&self) -> bool {
        self.checked.get()
    }

    /// `set checked`: não dispara `onChange`.
    pub fn set_checked(&self, checked: bool) {
        self.checked.set(checked);
        self.attributes.set("aria-checked", checked.to_string());
        self.ui.tree_mut().toggle_class(self.dom_node, "checked", checked);
        self.apply_styles();
        self.refresh();
    }

    pub fn set_icon(&self, icon: Option<ThemeIcon>) {
        {
            let mut tree = self.ui.tree_mut();
            if let Some(old) = &*self.icon.borrow() {
                for class in old.as_class_name_array() {
                    tree.remove_class(self.dom_node, &class);
                }
            }
            if let Some(new) = &icon {
                for class in new.as_class_name_array() {
                    tree.add_class(self.dom_node, &class);
                }
            }
        }
        *self.icon.borrow_mut() = icon;
        self.refresh();
    }

    /// O que o toggle reserva na linha: `2` de margem, `2` de borda, `2` de padding e `16` do ícone.
    pub fn width(&self) -> f64 {
        2.0 + 2.0 + 2.0 + 16.0
    }

    /// `applyStyles`.
    fn apply_styles(&self) {
        let checked = self.checked.get();
        let styles = &self.opts.styles;
        self.border_color_variable.set(if checked { styles.input_active_option_border } else { None });
        let mut inline = self.inline.get();
        inline.color = if checked { styles.input_active_option_foreground } else { None };
        inline.background = if checked { styles.input_active_option_background } else { None };
        self.inline.set(inline);
    }

    /// O `domNode.style` que o `Checkbox` escreve (cor, fundo, cor da borda, tamanho e fonte).
    pub(crate) fn set_inline_style(&self, color: Option<Rgba>, background: Option<Rgba>, border_color: Option<Rgba>, size: f64, font_size: f64) {
        self.inline.set(InlineStyle { color, background, border_color, size: Some(size), font_size: Some(font_size) });
        self.refresh();
    }

    pub fn enable(&self) {
        self.attributes.set("aria-disabled", "false");
        self.ui.tree_mut().remove_class(self.dom_node, "disabled");
        if !self.opts.not_focusable {
            self.tab_index.set(0);
        }
        self.refresh();
    }

    pub fn disable(&self) {
        self.attributes.set("aria-disabled", "true");
        self.ui.tree_mut().add_class(self.dom_node, "disabled");
        if !self.opts.not_focusable {
            self.tab_index.set(-1);
        }
        self.refresh();
    }

    /// `setTitle`: o título do hover e o `aria-label`.
    pub fn set_title(&self, new_title: ToggleTitle) {
        let aria_label = match &new_title {
            ToggleTitle::Text(text) => text.clone(),
            ToggleTitle::Markdown(markdown) => markdown.value.to_string_lossy(),
            ToggleTitle::Node(_, text) => text.clone(),
        };
        let label = get_codicon_aria_label(Some(&aria_label.encode_utf16().collect::<Vec<_>>())).to_string_lossy();
        *self.title.borrow_mut() = new_title;
        self.attributes.set("aria-label", label.clone());
        self.ui.tree_mut().set_label(self.dom_node, label);
    }

    pub fn set_visible(&self, visible: bool) {
        self.ui.tree_mut().style_mut(self.dom_node).visible = visible;
    }

    pub fn visible(&self) -> bool {
        self.ui.tree().style(self.dom_node).visible
    }

    pub fn tab_index(&self) -> i32 {
        self.tab_index.get()
    }

    /// `domNode.tabIndex = ...`: o dono do toggle (uma barra de ferramentas) cuida do foco.
    pub fn set_tab_index(&self, value: i32) {
        self.tab_index.set(value);
    }

    pub fn attribute(&self, name: &str) -> Option<String> {
        self.attributes.get(name)
    }

    /// `style.left` e `style.top` (o `float: left` e a `margin-left` são do dono).
    pub fn set_position(&self, x: f64, y: f64) {
        let mut tree = self.ui.tree_mut();
        let r = tree.bounds(self.dom_node);
        tree.set_bounds(self.dom_node, Rect::new(x, y, x + r.width(), y + r.height()));
    }

    fn glyph(&self) -> Option<String> {
        let from_icon = self.icon.borrow().as_ref().map(|i| i.as_class_name_array());
        from_icon.into_iter().flatten().chain(self.action_classes.iter().cloned()).find_map(|c| c.strip_prefix("codicon-").filter(|id| !id.starts_with("modifier-")).map(str::to_owned))
    }

    /// As regras de `toggle.css` e os estilos "inline", aplicados ao nó.
    pub fn refresh(&self) {
        if self.disposed.get() {
            return;
        }
        let (checkbox, disabled) = {
            let tree = self.ui.tree();
            (tree.has_class(self.dom_node, "monaco-checkbox"), tree.has_class(self.dom_node, "disabled"))
        };
        let inline = self.inline.get();
        let styles = &self.opts.styles;
        let size = inline.size.unwrap_or(if checkbox { 18.0 } else { 20.0 });
        let mut border_color = if checkbox { inline.border_color } else { self.border_color_variable.get() };
        let hover_background = (self.pointer_over.get() && !disabled).then_some(styles.input_option_hover_background).flatten();
        // `:focus-visible` troca a borda (tracejada no original) pela de foco no que não é checkbox.
        let focus_ring = self.focused.get() && !disabled;
        if focus_ring && !checkbox && let Some(color) = styles.focus_border {
            border_color = Some(color);
        }
        let color = inline.color.or(styles.inherited_foreground).unwrap_or(INHERITED_FOREGROUND);
        let glyph = self.glyph();
        let glyph_size = inline.font_size.unwrap_or(16.0);
        let outline = (focus_ring && checkbox).then_some(styles.focus_border).flatten();
        {
            let mut tree = self.ui.tree_mut();
            let r = tree.bounds(self.dom_node);
            tree.set_bounds(self.dom_node, Rect::new(r.x0, r.y0, r.x0 + size, r.y0 + size));
            let style = tree.style_mut(self.dom_node);
            style.background = inline.background.or(hover_background);
            style.border = border_color.map(|color| Border { width: 1.0, color });
            style.radius = 3.0;
            style.clip = true;
            style.opacity = if disabled { 0.4 } else { 1.0 };
            style.cursor = Some(if disabled { Cursor::Default } else { Cursor::Pointer });
            style.pointer_events = if disabled { PointerEvents::None } else { PointerEvents::Inherit };
            style.content = glyph.map_or(Content::None, |id| Content::Codicon { id, size: glyph_size as f32, color });
            update_outline(&mut tree, self.dom_node, &self.outline, outline.map(|color| OutlineSpec { color, offset: -1.0, radius: 3.0 }));
        }
    }
}

impl IDisposable for Toggle {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.base.dispose();
        self.ui.remove(self.dom_node);
    }

    fn constructor_name(&self) -> &'static str {
        "Toggle"
    }
}

/// O que `BaseCheckbox` (o `abstract class`) guarda e faz igual no `Checkbox` e no `TriStateCheckbox`.
struct BaseCheckbox {
    base: Rc<Disposable>,
    toggle: Rc<Toggle>,
    styles: CheckboxStyles,
    on_change: Rc<Emitter<bool>>,
}

impl BaseCheckbox {
    const CLASS_NAME: &'static str = "monaco-checkbox";

    fn new(toggle: Rc<Toggle>, styles: CheckboxStyles) -> Self {
        let base = Disposable::new();
        base.register(toggle.clone());
        let on_change = registered_emitter::<bool>(&base);
        Self { base, toggle, styles, on_change }
    }

    fn apply_styles(&self, enabled: Option<bool>) {
        let enabled = enabled.unwrap_or_else(|| self.toggle.enabled());
        let styles = &self.styles;
        let size = styles.size.unwrap_or(18.0);
        let size = if size == 0.0 { 18.0 } else { size };
        self.toggle.set_inline_style(
            if enabled { styles.checkbox_foreground } else { styles.checkbox_disabled_foreground },
            if enabled { styles.checkbox_background } else { styles.checkbox_disabled_background },
            if enabled { styles.checkbox_border } else { styles.checkbox_disabled_background },
            size,
            size - 2.0,
        );
    }
}

/// Repete para o `Checkbox` e o `TriStateCheckbox` o que o `BaseCheckbox` do original faz igual.
macro_rules! base_checkbox_methods {
    () => {
        pub fn dom_node(&self) -> NodeId {
            self.core.toggle.dom_node()
        }

        /// `onChange`: dispara com `true` quando a troca veio do teclado.
        pub fn on_change(&self) -> BaseEvent<bool> {
            self.core.on_change.event()
        }

        pub fn enabled(&self) -> bool {
            self.core.toggle.enabled()
        }

        pub fn focus(&self) {
            self.core.toggle.focus();
        }

        pub fn has_focus(&self) -> bool {
            self.core.toggle.ui().focused() == Some(self.core.toggle.dom_node())
        }

        pub fn enable(&self) {
            self.core.toggle.enable();
            self.refresh_styles(Some(true));
        }

        pub fn disable(&self) {
            self.core.toggle.disable();
            self.refresh_styles(Some(false));
        }

        pub fn set_title(&self, new_title: &str) {
            self.core.toggle.set_title(ToggleTitle::from(new_title));
        }

        /// `style.left` e `style.top`.
        pub fn set_position(&self, x: f64, y: f64) {
            self.core.toggle.set_position(x, y);
        }

        pub fn toggle(&self) -> &Rc<Toggle> {
            &self.core.toggle
        }
    };
}

/// O checkbox.
pub struct Checkbox {
    core: BaseCheckbox,
}

impl Checkbox {
    pub fn new(ui: &Ui, title: &str, is_checked: bool, styles: CheckboxStyles) -> Rc<Checkbox> {
        let toggle_styles = ToggleStyles { focus_border: styles.focus_border, inherited_foreground: styles.inherited_foreground, ..unthemed_toggle_styles() };
        let toggle = Toggle::new(
            ui,
            ToggleOpts { action_class_name: Some(BaseCheckbox::CLASS_NAME.to_owned()), icon: Some(ThemeIcon::from_id("check")), hover_lifecycle_options: styles.hover_lifecycle_options.clone(), ..ToggleOpts::new(title, is_checked, toggle_styles) },
        );
        let this = Rc::new(Checkbox { core: BaseCheckbox::new(toggle, styles) });
        this.refresh_styles(None);
        let weak = Rc::downgrade(&this);
        this.core.base.register(this.core.toggle.on_change().listen(move |keyboard| {
            if let Some(checkbox) = weak.upgrade() {
                checkbox.refresh_styles(None);
                checkbox.core.on_change.fire(*keyboard);
            }
        }));
        this
    }

    base_checkbox_methods!();

    pub fn checked(&self) -> bool {
        self.core.toggle.checked()
    }

    pub fn set_checked(&self, new_is_checked: bool) {
        self.core.toggle.set_checked(new_is_checked);
        self.refresh_styles(None);
    }

    /// O `applyStyles` do `Checkbox`: o ícone de marcado e depois os estilos do `BaseCheckbox`.
    fn refresh_styles(&self, enabled: Option<bool>) {
        self.core.toggle.set_icon(self.core.toggle.checked().then(|| ThemeIcon::from_id("check")));
        self.core.apply_styles(enabled);
    }
}

impl IDisposable for Checkbox {
    fn dispose(&self) {
        self.core.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "Checkbox"
    }
}

/// `boolean | 'mixed'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckState {
    Unchecked,
    Checked,
    Mixed,
}

impl CheckState {
    fn icon(self) -> Option<ThemeIcon> {
        match self {
            Self::Checked => Some(ThemeIcon::from_id("check")),
            Self::Mixed => Some(ThemeIcon::from_id("dash")),
            Self::Unchecked => None,
        }
    }
}

/// O checkbox de três estados.
pub struct TriStateCheckbox {
    core: BaseCheckbox,
    state: Cell<CheckState>,
}

impl TriStateCheckbox {
    pub fn new(ui: &Ui, title: &str, state: CheckState, styles: CheckboxStyles) -> Rc<TriStateCheckbox> {
        let toggle_styles = ToggleStyles { focus_border: styles.focus_border, inherited_foreground: styles.inherited_foreground, ..unthemed_toggle_styles() };
        let toggle = Toggle::new(
            ui,
            ToggleOpts { action_class_name: Some(BaseCheckbox::CLASS_NAME.to_owned()), icon: state.icon(), hover_lifecycle_options: styles.hover_lifecycle_options.clone(), ..ToggleOpts::new(title, state == CheckState::Checked, toggle_styles) },
        );
        let this = Rc::new(TriStateCheckbox { core: BaseCheckbox::new(toggle, styles), state: Cell::new(state) });
        this.refresh_styles(None);
        let weak = Rc::downgrade(&this);
        this.core.base.register(this.core.toggle.on_change().listen(move |keyboard| {
            if let Some(checkbox) = weak.upgrade() {
                checkbox.state.set(if checkbox.core.toggle.checked() { CheckState::Checked } else { CheckState::Unchecked });
                checkbox.refresh_styles(None);
                checkbox.core.on_change.fire(*keyboard);
            }
        }));
        this
    }

    base_checkbox_methods!();

    pub fn checked(&self) -> CheckState {
        self.state.get()
    }

    pub fn set_checked(&self, new_state: CheckState) {
        if self.state.get() != new_state {
            self.state.set(new_state);
            self.core.toggle.set_checked(new_state == CheckState::Checked);
            self.refresh_styles(None);
        }
    }

    fn refresh_styles(&self, enabled: Option<bool>) {
        self.core.toggle.set_icon(self.state.get().icon());
        self.core.apply_styles(enabled);
    }
}

impl IDisposable for TriStateCheckbox {
    fn dispose(&self) {
        self.core.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "TriStateCheckbox"
    }
}
