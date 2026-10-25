//! Porte de `src/vs/base/browser/ui/button/button.ts` e `button.css`: o [`Button`] (a caixa de texto, ícone
//! ou os dois, com hover, foco e desabilitado), o [`ButtonWithDescription`], o [`ButtonWithDropdown`]
//! (dois botões e um separador) e a [`ButtonBar`] (navegação por setas entre botões), mais
//! [`ButtonStyles`], [`unthemed_button_styles`] e [`default_button_styles`] (o `defaultButtonStyles` de
//! `defaultStyles.ts`).
//!
//! Como o DOM e o CSS viram [`je_ui`]:
//!
//! - O `<a class="monaco-button">` é um nó com as mesmas classes (`monaco-button`, `monaco-text-button`,
//!   `secondary`, `small`, `disabled`, `checked`, `monaco-dropdown-button`, `codicon-*`), lidas com
//!   [`je_ui::Tree::has_class`]. Os filhos de texto (`<span>`) e de ícone (`<span class="codicon ...">`) do
//!   rótulo são nós filhos, na mesma ordem.
//! - Não há motor de layout: [`Button::layout`] faz o `display: flex; justify-content: center;
//!   align-items: center` do `.monaco-text-button` (padding, borda, `line-height`, margem de `0.2em` dos
//!   ícones) e calcula a altura. A largura é `width: 100%` (o pai define) ou a que o dono fixa com
//!   [`Button::set_width`]; a posição é do dono ([`Button::set_position`]).
//! - As regras com `:hover`, `:focus` e `.disabled` viram [`Button::refresh`], chamada pelos ouvintes de
//!   `PointerEnter`/`PointerLeave` (o `mouseover`/`mouseout` do original) e do rastreador de foco, e pelas
//!   trocas de estado. O contorno de foco (`outline: 1px solid var(--vscode-focusBorder)`, regra global do
//!   workbench) é um nó filho; a cor vem de [`ButtonStyles::focus_border`] (campo a mais, o CSS lê a
//!   variável direto). `opacity: 0.4` do `.disabled` é `NodeStyle::opacity`.
//! - O que outro widget escreve na folha de estilo sobre o botão (o `.monaco-custom-radio > .monaco-button`,
//!   o `.monaco-button-dropdown > .monaco-button`) é um [`ButtonCss`] instalado com [`Button::set_css`]:
//!   uma função que ajusta a [`ButtonBox`] calculada a partir das classes, antes dos estilos "inline" que
//!   as opções do botão escrevem (os que o original põe em `element.style`).
//! - `aria-*` e `data-label` ficam no nó ([`Button::attribute`], [`je_ui::Tree::set_attribute`]; o
//!   `aria-label` é o rótulo do nó); o `tabIndex` fica no botão ([`Button::tab_index`]).
//!
//! Fica de fora, por enquanto: o toque (`Gesture`, `TouchEventType.Tap`), o rótulo em Markdown com HTML
//! (o `renderMarkdown` e o `safeSetInnerHtml` são `TODO(integração)`: o valor vira texto, com ícones) e o
//! `ButtonWithIcon`, que depende desse renderizador para quebrar linha.

mod composite;
pub(crate) mod flex;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event as BaseEvent};
use je_base::html_content::IMarkdownString;
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::themables::ThemeIcon;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Border, Content, Cursor, Event, EventKind, KeyData, NodeId, PointerData, PointerEvents, Role, Tree, Ui};

pub use composite::{
    ButtonBar, ButtonBarAlignment, ButtonDropdownActions, ButtonWithDescription, ButtonWithDropdown, ButtonWithDropdownOptions, ContextMenuDelegate, IActionProvider, IContextMenuProvider,
};

use crate::common::registered_emitter;
use crate::dom::{FocusTracker, StandardKeyboardEvent, create_el};
use crate::hover_api::{HoverPlacement, IHoverDelegate, IManagedHover, ManagedHoverContentOrFactory, get_base_layer_hover_delegate, get_default_hover_delegate};
use crate::text::{UI_FONT_SIZE, measure_text, ui_font};

/// O `color` herdado quando nem o botão nem o pai declaram um.
pub(crate) const INHERITED_FOREGROUND: Rgba = Rgba::opaque(0xCC, 0xCC, 0xCC);

/// O `font-size` do `.codicon` (`font: normal normal normal 16px/1 codicon`).
pub(crate) const CODICON_SIZE: f64 = 16.0;
/// A margem direita da vaga à frente do rótulo (`margin: 0 var(--vscode-spacing-size40) 0 0`, `4px`).
const LEADING_ICON_GAP: f64 = 4.0;

/// `IButtonStyles`, com `focus_border` a mais (o `--vscode-focusBorder` que a regra global de foco lê).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ButtonStyles {
    pub button_background: Option<Rgba>,
    pub button_hover_background: Option<Rgba>,
    pub button_foreground: Option<Rgba>,
    pub button_separator: Option<Rgba>,
    pub button_secondary_background: Option<Rgba>,
    pub button_secondary_hover_background: Option<Rgba>,
    pub button_secondary_foreground: Option<Rgba>,
    pub button_secondary_border: Option<Rgba>,
    pub button_border: Option<Rgba>,
    pub focus_border: Option<Rgba>,
}

/// `unthemedButtonStyles`.
pub fn unthemed_button_styles() -> ButtonStyles {
    ButtonStyles {
        button_background: Some(Rgba::opaque(0x0E, 0x63, 0x9C)),
        button_hover_background: Some(Rgba::opaque(0x00, 0x6B, 0xB3)),
        button_separator: Some(Rgba::opaque(0xFF, 0xFF, 0xFF)),
        button_foreground: Some(Rgba::opaque(0xFF, 0xFF, 0xFF)),
        ..ButtonStyles::default()
    }
}

/// `defaultButtonStyles` de `defaultStyles.ts`: as chaves de tema que o original referencia por variável CSS.
pub fn default_button_styles(theme: &ColorTheme) -> ButtonStyles {
    ButtonStyles {
        button_foreground: theme.get_color("button.foreground"),
        button_separator: theme.get_color("button.separator"),
        button_background: theme.get_color("button.background"),
        button_hover_background: theme.get_color("button.hoverBackground"),
        button_secondary_foreground: theme.get_color("button.secondaryForeground"),
        button_secondary_background: theme.get_color("button.secondaryBackground"),
        button_secondary_hover_background: theme.get_color("button.secondaryHoverBackground"),
        button_secondary_border: theme.get_color("button.secondaryBorder"),
        button_border: theme.get_color("button.border"),
        focus_border: theme.get_color("focusBorder"),
    }
}

/// O `title?: boolean | string` de `IButtonOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ButtonTitle {
    /// `undefined` ou `false`.
    #[default]
    None,
    /// `true`: o título é o texto do rótulo.
    Auto,
    Text(String),
}

/// `IButtonOptions` (com os `Partial<IButtonStyles>` em [`ButtonOptions::styles`]).
#[derive(Clone, Default)]
pub struct ButtonOptions {
    pub styles: ButtonStyles,
    pub title: ButtonTitle,
    /// Cai para `title` se não definido.
    pub aria_label: Option<String>,
    pub support_icons: bool,
    pub support_short_label: bool,
    pub secondary: bool,
    pub small: bool,
    pub hover_delegate: Option<Rc<dyn IHoverDelegate>>,
    pub disabled: bool,
}

/// `string | IMarkdownString`.
#[derive(Clone, Debug)]
pub enum ButtonLabel {
    Text(String),
    Markdown(IMarkdownString),
}

impl ButtonLabel {
    /// `renderAsPlaintext(value)`.
    // TODO(integração): o texto simples de um Markdown vem do `renderAsPlaintext` quando o renderizador existir.
    pub fn plain_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Markdown(markdown) => markdown.value.to_string_lossy(),
        }
    }
}

impl PartialEq for ButtonLabel {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Markdown(a), Self::Markdown(b)) => a.value.units() == b.value.units(),
            _ => false,
        }
    }
}

impl From<&str> for ButtonLabel {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<String> for ButtonLabel {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

/// O evento (`Event | undefined`) que o `onDidClick` entrega: o ponteiro de um clique ou a tecla de Enter e
/// Space; `None` quando o clique é sintético (o item principal do menu do `ButtonWithDropdown`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ButtonEvent {
    pub pointer: Option<PointerData>,
    pub key: Option<KeyData>,
}

impl ButtonEvent {
    fn from_event(e: &Event) -> Self {
        Self { pointer: e.pointer, key: e.key }
    }
}

/// `IButton`.
pub trait IButton: IDisposable {
    fn element(&self) -> NodeId;
    fn on_did_click(&self) -> BaseEvent<Option<ButtonEvent>>;
    fn set_label(&self, value: ButtonLabel);
    fn set_icon(&self, icon: &ThemeIcon);
    fn set_enabled(&self, value: bool);
    fn enabled(&self) -> bool;
    fn set_checked(&self, value: bool);
    fn checked(&self) -> bool;
    fn set_title(&self, title: &str);
    fn set_aria_label(&self, aria_label: &str);
    fn focus(&self);
    fn has_focus(&self) -> bool;
}

/// `IButtonWithDescription`.
pub trait IButtonWithDescription: IButton {
    fn set_description(&self, value: &str);
}

/// Um trecho do rótulo: um `<span>` de texto ou um `<span class="codicon ...">`.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelPart {
    pub node: NodeId,
    pub kind: LabelPartKind,
    /// O texto do trecho, ou o id do ícone (com o modificador, `sync~spin`).
    pub text: String,
    /// O `data-label` de um trecho de texto (o `Radio` segmentado o escreve).
    pub data_label: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelPartKind {
    Text,
    Icon,
}

/// O que `renderLabelWithIcons` devolve: texto e ícones (`$(id)`), com `\$(id)` escapado virando texto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LabelSegment {
    Text(String),
    Icon(String),
}

/// `renderLabelWithIcons` (sem os elementos): os trechos de `text`.
// TODO(integração): trocar pelo `icon_labels::render_label_with_icons` quando o módulo de `iconLabel` entrar.
pub(crate) fn render_label_with_icons(text: &str) -> Vec<LabelSegment> {
    let chars: Vec<char> = text.chars().collect();
    let mut segments = Vec::new();
    let (mut text_start, mut i) = (0, 0);
    while i + 1 < chars.len() {
        if chars[i] == '$' && chars[i + 1] == '(' && let Some((name, end)) = parse_icon_name(&chars, i + 2) {
            let escaped = i > text_start && chars[i - 1] == '\\';
            let match_start = if escaped { i - 1 } else { i };
            if text_start < match_start {
                segments.push(LabelSegment::Text(chars[text_start..match_start].iter().collect()));
            }
            segments.push(if escaped { LabelSegment::Text(format!("$({name})")) } else { LabelSegment::Icon(name) });
            text_start = end;
            i = end;
        } else {
            i += 1;
        }
    }
    if text_start < chars.len() {
        segments.push(LabelSegment::Text(chars[text_start..].iter().collect()));
    }
    segments
}

/// `[A-Za-z0-9-]+(~[A-Za-z]+)?\)` a partir de `start`: o nome do ícone e a posição depois do `)`.
fn parse_icon_name(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut end = start;
    while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '-') {
        end += 1;
    }
    if end == start {
        return None;
    }
    if end < chars.len() && chars[end] == '~' {
        let modifier_start = end + 1;
        let mut modifier_end = modifier_start;
        while modifier_end < chars.len() && chars[modifier_end].is_ascii_alphabetic() {
            modifier_end += 1;
        }
        if modifier_end > modifier_start {
            end = modifier_end;
        }
    }
    (end < chars.len() && chars[end] == ')').then(|| (chars[start..end].iter().collect(), end + 1))
}

/// A caixa calculada de um botão: o que as regras de CSS, os estilos "inline" e a folha de estilo de quem
/// o contém decidem juntos.
#[derive(Clone, Debug, PartialEq)]
pub struct ButtonBox {
    /// Cima, direita, baixo, esquerda.
    pub padding: [f64; 4],
    /// Cima, direita, baixo, esquerda.
    pub border_widths: [f64; 4],
    pub border_color: Option<Rgba>,
    /// Superior esquerdo, superior direito, inferior direito, inferior esquerdo.
    pub radii: [f64; 4],
    pub background: Option<Rgba>,
    pub foreground: Option<Rgba>,
    pub font_size: f64,
    pub font_weight: u16,
    pub line_height: f64,
    /// `height` explícita (a do `Radio` segmentado, a do botão com rótulo curto, ou a esticada pelo pai).
    pub height: Option<f64>,
    /// A margem horizontal dos ícones, em `em` do ícone (`margin: 0 0.2em`).
    pub icon_margin_em: f64,
    /// O peso cuja largura os trechos com `data-label` reservam (`[data-label]::after`).
    pub reserve_weight: Option<u16>,
    pub cursor_pointer: bool,
    /// `outline-offset` do contorno de foco: `2px` no `.monaco-text-button:focus`, `-1px` no resto.
    pub outline_offset: f64,
}

impl ButtonBox {
    const ZERO: ButtonBox = ButtonBox {
        padding: [0.0; 4],
        border_widths: [0.0; 4],
        border_color: None,
        radii: [0.0; 4],
        background: None,
        foreground: None,
        font_size: UI_FONT_SIZE as f64,
        font_weight: 400,
        line_height: 16.0,
        height: None,
        icon_margin_em: 0.2,
        reserve_weight: None,
        cursor_pointer: false,
        outline_offset: -1.0,
    };

    /// O `.monaco-text-button`.
    fn text_button() -> Self {
        Self { padding: [4.0, 8.0, 4.0, 8.0], border_widths: [1.0; 4], radii: [4.0; 4], font_size: 12.0, cursor_pointer: true, outline_offset: 2.0, ..Self::ZERO }
    }

    pub fn horizontal_extra(&self) -> f64 {
        self.padding[1] + self.padding[3] + self.border_widths[1] + self.border_widths[3]
    }

    pub fn vertical_extra(&self) -> f64 {
        self.padding[0] + self.padding[2] + self.border_widths[0] + self.border_widths[2]
    }
}

/// A folha de estilo de quem contém o botão: ajusta a [`ButtonBox`] já calculada. Recebe o botão para ler
/// o estado (`:hover`, classes).
pub type ButtonCss = Rc<dyn Fn(&Button, &mut ButtonBox)>;

/// Os estilos "inline" (`element.style`): `None` é a string vazia, que deixa a folha de estilo valer.
#[derive(Clone, Copy, Debug, Default)]
struct InlineStyle {
    background: Option<Rgba>,
    foreground: Option<Rgba>,
    border: Option<Rgba>,
}

/// Quem decide a largura do botão.
#[derive(Clone, Copy, Debug, PartialEq)]
enum WidthMode {
    /// `width: 100%` (o `.monaco-text-button`) ou a intrínseca, se não for um botão de texto.
    Css,
    Fixed(f64),
}

/// O botão.
pub struct Button {
    base: Rc<Disposable>,
    weak_self: Weak<Button>,
    ui: Ui,
    options: RefCell<ButtonOptions>,
    element: NodeId,
    label: RefCell<ButtonLabel>,
    label_element: Option<NodeId>,
    label_short_element: Option<NodeId>,
    parts: RefCell<Vec<LabelPart>>,
    short_parts: RefCell<Vec<LabelPart>>,
    icon_classes: RefCell<Vec<String>>,
    hover: RefCell<Option<Rc<dyn IManagedHover>>>,
    hover_style_applied: Cell<bool>,
    pointer_over: Cell<bool>,
    focused: Cell<bool>,
    tab_index: Cell<i32>,
    inline: Cell<InlineStyle>,
    css: RefCell<Option<ButtonCss>>,
    width_mode: Cell<WidthMode>,
    forced_height: Cell<Option<f64>>,
    outline: Cell<Option<NodeId>>,
    /// A vaga à frente do rótulo (`.monaco-button-leading-icon`): o ícone ou o spinner que o dono pendurou.
    leading: Cell<Option<NodeId>>,
    on_did_click: Rc<Emitter<Option<ButtonEvent>>>,
    on_did_escape: Rc<Emitter<Option<ButtonEvent>>>,
    focus_tracker: Rc<FocusTracker>,
    disposed: Cell<bool>,
}

impl Button {
    pub fn new(ui: &Ui, container: NodeId, options: ButtonOptions) -> Rc<Button> {
        let base = Disposable::new();
        let (element, label_element, label_short_element) = {
            let mut tree = ui.tree_mut();
            let element = create_el(&mut tree, container, Role::Button, "monaco-button");
            tree.toggle_class(element, "secondary", options.secondary);
            tree.toggle_class(element, "small", options.small);
            let (mut label_short, mut label) = (None, None);
            if options.support_short_label {
                label_short = Some(create_el(&mut tree, element, Role::Generic, "monaco-button-label-short"));
                label = Some(create_el(&mut tree, element, Role::Generic, "monaco-button-label"));
                tree.add_class(element, "monaco-text-button-with-short-label");
            }
            (element, label, label_short)
        };
        let focus_tracker = FocusTracker::new(ui, element);
        base.register(focus_tracker.clone());
        let this = Rc::new_cyclic(|weak| Button {
            weak_self: weak.clone(),
            ui: ui.clone(),
            element,
            label: RefCell::new(ButtonLabel::Text(String::new())),
            label_element,
            label_short_element,
            parts: RefCell::new(Vec::new()),
            short_parts: RefCell::new(Vec::new()),
            icon_classes: RefCell::new(Vec::new()),
            hover: RefCell::new(None),
            hover_style_applied: Cell::new(false),
            pointer_over: Cell::new(false),
            focused: Cell::new(false),
            tab_index: Cell::new(0),
            inline: Cell::new(InlineStyle::default()),
            css: RefCell::new(None),
            width_mode: Cell::new(WidthMode::Css),
            forced_height: Cell::new(None),
            outline: Cell::new(None),
            leading: Cell::new(None),
            on_did_click: registered_emitter(&base),
            on_did_escape: registered_emitter(&base),
            focus_tracker,
            disposed: Cell::new(false),
            options: RefCell::new(options.clone()),
            base,
        });

        this.update_inline(false);
        if let ButtonTitle::Text(title) = &options.title {
            this.set_title(title);
        }
        if let Some(aria_label) = &options.aria_label {
            this.set_attribute("aria-label", aria_label);
        }
        this.set_enabled(!options.disabled);

        this.listen(EventKind::Click, |button, e| {
            if !button.enabled() {
                e.stop(false);
                return;
            }
            button.on_did_click.fire(Some(ButtonEvent::from_event(e)));
        });
        this.listen(EventKind::KeyDown, |button, e| {
            let Some(event) = StandardKeyboardEvent::from_event(e) else { return };
            let mut handled = false;
            if button.enabled() && (event.equals(KeyCode::Enter.bits()) || event.equals(KeyCode::Space.bits())) {
                button.on_did_click.fire(Some(ButtonEvent::from_event(e)));
                handled = true;
            } else if event.equals(KeyCode::Escape.bits()) {
                button.on_did_escape.fire(Some(ButtonEvent::from_event(e)));
                if button.has_focus() {
                    button.ui.blur();
                }
                handled = true;
            }
            if handled {
                e.stop(true);
            }
        });
        this.listen(EventKind::PointerEnter, |button, _| {
            button.pointer_over.set(true);
            if button.enabled() {
                button.update_styles(true);
            } else {
                button.refresh();
            }
        });
        this.listen(EventKind::PointerLeave, |button, _| {
            button.pointer_over.set(false);
            button.update_styles(false);
        });
        // Also set hover background when button is focused for feedback.
        let weak = this.weak_self.clone();
        this.base.register(this.focus_tracker.on_did_focus().listen(move |_| {
            if let Some(button) = weak.upgrade() {
                button.focused.set(true);
                if button.enabled() {
                    button.update_styles(true);
                } else {
                    button.refresh();
                }
            }
        }));
        let weak = this.weak_self.clone();
        this.base.register(this.focus_tracker.on_did_blur().listen(move |_| {
            if let Some(button) = weak.upgrade() {
                button.focused.set(false);
                if button.enabled() {
                    button.update_styles(false);
                } else {
                    button.refresh();
                }
            }
        }));
        this.refresh();
        this
    }

    fn listen(&self, kind: EventKind, handler: impl Fn(&Rc<Button>, &mut Event) + 'static) {
        let weak = self.weak_self.clone();
        self.base.register(self.ui.listen(self.element, kind, false, move |e| {
            if let Some(button) = weak.upgrade() {
                handler(&button, e);
            }
        }));
    }

    pub fn ui(&self) -> &Ui {
        &self.ui
    }

    pub fn on_did_escape(&self) -> BaseEvent<Option<ButtonEvent>> {
        self.on_did_escape.event()
    }

    pub fn label(&self) -> ButtonLabel {
        self.label.borrow().clone()
    }

    /// O `textContent` do botão: o texto dos trechos de rótulo, em ordem.
    pub fn text_content(&self) -> String {
        let texts = |parts: &RefCell<Vec<LabelPart>>| parts.borrow().iter().filter(|p| p.kind == LabelPartKind::Text).map(|p| p.text.clone()).collect::<String>();
        texts(&self.short_parts) + &texts(&self.parts)
    }

    /// Os trechos do rótulo (os filhos do elemento do rótulo), em ordem.
    pub fn label_parts(&self) -> Vec<LabelPart> {
        self.parts.borrow().clone()
    }

    /// Escreve o `data-label` do trecho de texto `index` (contando só os trechos de texto), ou o apaga.
    pub fn set_data_label(&self, index: usize, value: Option<&str>) {
        if let Some(part) = self.parts.borrow_mut().iter_mut().filter(|p| p.kind == LabelPartKind::Text).nth(index) {
            part.data_label = value.map(str::to_owned);
        }
    }

    /// `labelShort`.
    pub fn set_label_short(&self, value: &str) {
        let Some(container) = self.label_short_element else { return };
        let support_icons = self.options.borrow().support_icons;
        rebuild_label_parts(&self.ui, container, &self.short_parts, value, support_icons);
        self.refresh();
    }

    /// `set label`.
    pub fn set_label(&self, value: ButtonLabel) {
        if *self.label.borrow() == value {
            return;
        }
        let (support_icons, title) = {
            let options = self.options.borrow();
            (options.support_icons, options.title.clone())
        };
        self.ui.tree_mut().add_class(self.element, "monaco-text-button");
        let container = self.label_element.unwrap_or(self.element);
        // TODO(integração): um valor Markdown vira texto; o `renderMarkdown` do original entra com o renderizador.
        let text = value.plain_text();
        rebuild_label_parts(&self.ui, container, &self.parts, &text, support_icons || matches!(value, ButtonLabel::Markdown(_)));
        let title = match title {
            ButtonTitle::Text(text) => text,
            ButtonTitle::Auto => value.plain_text(),
            ButtonTitle::None => String::new(),
        };
        self.set_title(&title);
        self.set_aria_label_from_options();
        *self.label.borrow_mut() = value;
        self.refresh();
    }

    fn set_aria_label_from_options(&self) {
        let options = self.options.borrow();
        if let Some(aria_label) = &options.aria_label {
            self.set_attribute("aria-label", aria_label);
        } else if let ButtonTitle::Text(title) = &options.title {
            self.set_attribute("aria-label", title);
        }
    }

    /// `set icon`.
    pub fn set_icon(&self, icon: &ThemeIcon) {
        self.set_aria_label_from_options();
        {
            let mut tree = self.ui.tree_mut();
            for class in self.icon_classes.borrow_mut().drain(..) {
                tree.remove_class(self.element, &class);
            }
            let classes = icon.as_class_name_array();
            for class in &classes {
                tree.add_class(self.element, class);
            }
            *self.icon_classes.borrow_mut() = classes;
        }
        self.refresh();
    }

    /// O id do ícone do próprio elemento (a classe `codicon-*`), se houver.
    fn element_icon(&self) -> Option<String> {
        self.icon_classes.borrow().iter().find_map(|c| c.strip_prefix("codicon-").filter(|id| !id.starts_with("modifier-")).map(str::to_owned))
    }

    pub fn set_enabled(&self, value: bool) {
        self.ui.tree_mut().toggle_class(self.element, "disabled", !value);
        self.set_attribute("aria-disabled", (!value).to_string());
        if value {
            self.tab_index.set(0);
        } else if self.hover_style_applied.get() {
            // Disabled buttons never show hover feedback, and the blur handler only resets it while
            // enabled, so clear any hover style applied before disabling.
            self.update_styles(false);
            return;
        }
        self.refresh();
    }

    pub fn set_secondary(&self, value: bool) {
        self.ui.tree_mut().toggle_class(self.element, "secondary", value);
        self.options.borrow_mut().secondary = value;
        self.update_styles(false);
    }

    pub fn set_checked(&self, value: bool) {
        self.ui.tree_mut().toggle_class(self.element, "checked", value);
        self.set_attribute("aria-pressed", value.to_string());
    }

    pub fn set_title(&self, title: &str) {
        let existing = self.hover.borrow().clone();
        if existing.is_none() && !title.is_empty() {
            let delegate = self.options.borrow().hover_delegate.clone().unwrap_or_else(|| get_default_hover_delegate(HoverPlacement::Element));
            let hover = get_base_layer_hover_delegate().setup_managed_hover(delegate, self.element, ManagedHoverContentOrFactory::from(title), None);
            self.base.register(hover.clone());
            *self.hover.borrow_mut() = Some(hover);
        } else if let Some(hover) = existing {
            hover.update(ManagedHoverContentOrFactory::from(title), None);
        }
    }

    pub fn set_aria_label(&self, aria_label: &str) {
        self.set_attribute("aria-label", aria_label);
    }

    pub fn focus(&self) {
        self.ui.focus(self.element);
    }

    pub fn has_focus(&self) -> bool {
        self.ui.focused() == Some(self.element)
    }

    /// Os estilos das opções do botão.
    pub fn styles(&self) -> ButtonStyles {
        self.options.borrow().styles
    }

    /// `tabIndex`.
    pub fn tab_index(&self) -> i32 {
        self.tab_index.get()
    }

    pub fn set_tab_index(&self, value: i32) {
        self.tab_index.set(value);
    }

    /// `getAttribute`.
    pub fn attribute(&self, name: &str) -> Option<String> {
        self.ui.tree().attribute(self.element, name).map(str::to_owned)
    }

    /// `setAttribute`: o atributo vive no nó (o `aria-label` é o rótulo de acessibilidade dele).
    pub fn set_attribute(&self, name: &str, value: impl Into<String>) {
        self.ui.tree_mut().set_attribute(self.element, name, value);
    }

    /// O ponteiro está sobre o botão (o `:hover` do CSS).
    pub fn is_hovered(&self) -> bool {
        self.pointer_over.get()
    }

    /// O `:focus` do CSS.
    pub fn is_focused(&self) -> bool {
        self.focused.get()
    }

    /// Instala a folha de estilo de quem contém o botão (ver [`ButtonCss`]) e reaplica os estilos.
    pub fn set_css(&self, css: Option<ButtonCss>) {
        *self.css.borrow_mut() = css;
        self.refresh();
    }

    /// `style.left`/`style.top`: a posição do botão no pai.
    pub fn set_position(&self, x: f64, y: f64) {
        let mut tree = self.ui.tree_mut();
        let r = tree.bounds(self.element);
        tree.set_bounds(self.element, Rect::new(x, y, x + r.width(), y + r.height()));
    }

    /// `style.width` (`None` volta ao da folha de estilo: `100%` num botão de texto, o intrínseco nos outros).
    pub fn set_width(&self, width: Option<f64>) {
        self.width_mode.set(width.map_or(WidthMode::Css, WidthMode::Fixed));
        self.layout();
    }

    /// `style.height` (o que o `align-items: stretch` do pai faz); `None` volta à altura do conteúdo.
    pub fn set_height(&self, height: Option<f64>) {
        self.forced_height.set(height);
        self.layout();
    }

    /// A caixa de borda do botão: largura e altura.
    pub fn size(&self) -> (f64, f64) {
        let r = self.ui.tree().bounds(self.element);
        (r.width(), r.height())
    }

    /// A largura do conteúdo mais padding e borda: o `min-content` do botão (os rótulos não quebram).
    pub fn min_content_width(&self) -> f64 {
        let b = self.compute_box();
        self.content_width(&b) + b.horizontal_extra()
    }

    /// `updateStyles(hover)`.
    fn update_styles(&self, hover: bool) {
        self.update_inline(hover);
        self.hover_style_applied.set(hover);
        self.refresh();
    }

    fn update_inline(&self, hover: bool) {
        let options = self.options.borrow();
        let styles = &options.styles;
        let inline = if options.secondary {
            InlineStyle {
                background: if hover { styles.button_secondary_hover_background } else { styles.button_secondary_background },
                foreground: styles.button_secondary_foreground,
                border: styles.button_secondary_border,
            }
        } else {
            InlineStyle {
                background: if hover { styles.button_hover_background } else { styles.button_background },
                foreground: styles.button_foreground,
                border: styles.button_border,
            }
        };
        self.inline.set(inline);
    }

    /// A caixa calculada: as regras de `button.css` pelas classes, a folha de estilo de quem contém o botão e,
    /// por cima, os estilos "inline".
    pub fn compute_box(&self) -> ButtonBox {
        let (text_button, small, short, secondary, enabled) = {
            let tree = self.ui.tree();
            (tree.has_class(self.element, "monaco-text-button"), tree.has_class(self.element, "small"), self.label_element.is_some(), tree.has_class(self.element, "secondary"), !tree.has_class(self.element, "disabled"))
        };
        let styles = self.options.borrow().styles;
        let mut b = if text_button { ButtonBox::text_button() } else { ButtonBox::ZERO };
        if text_button {
            b.border_color = if secondary { styles.button_secondary_border.or(styles.button_border) } else { styles.button_border };
            if small {
                b = ButtonBox { padding: [3.0, 6.0, 3.0, 6.0], line_height: 14.0, font_size: 11.0, ..b };
            }
            if short {
                b = ButtonBox { padding: [0.0, 4.0, 0.0, 4.0], height: Some(28.0), ..b };
            }
        }
        let css = self.css.borrow().clone();
        if let Some(css) = css {
            css(self, &mut b);
        }
        let inline = self.inline.get();
        if inline.background.is_some() {
            b.background = inline.background;
        }
        if inline.foreground.is_some() {
            b.foreground = inline.foreground;
        }
        if inline.border.is_some() {
            b.border_color = inline.border;
            b.border_widths = [1.0; 4];
        }
        if let Some(height) = self.forced_height.get() {
            b.height = Some(height);
        }
        b.cursor_pointer = b.cursor_pointer && enabled;
        b
    }

    /// Reaplica os estilos e o layout: o que o navegador faz sozinho a cada mudança de classe ou de estado.
    pub fn refresh(&self) {
        if self.disposed.get() {
            return;
        }
        let b = self.compute_box();
        self.apply_style(&b);
        self.layout_with(&b);
    }

    /// Refaz o layout dos filhos e a altura.
    pub fn layout(&self) {
        let b = self.compute_box();
        self.layout_with(&b);
    }

    fn apply_style(&self, b: &ButtonBox) {
        let disabled = self.ui.tree().has_class(self.element, "disabled");
        let icon = self.element_icon();
        let foreground = b.foreground.unwrap_or(INHERITED_FOREGROUND);
        let offset = b.outline_offset;
        let mut tree = self.ui.tree_mut();
        let uniform = b.border_widths.iter().all(|w| *w == b.border_widths[0]);
        let radius_uniform = b.radii.iter().all(|r| *r == b.radii[0]);
        let style = tree.style_mut(self.element);
        style.background = b.background;
        style.border = b.border_color.filter(|_| b.border_widths.iter().any(|w| *w > 0.0)).map(|color| Border { width: b.border_widths.iter().copied().fold(0.0, f64::max), color });
        style.border_widths = (!uniform).then_some(b.border_widths);
        style.radius = b.radii[0];
        style.corner_radii = (!radius_uniform).then_some(b.radii);
        style.opacity = if disabled { 0.4 } else { 1.0 };
        style.cursor = Some(if b.cursor_pointer { Cursor::Pointer } else { Cursor::Default });
        style.clip = self.label_element.is_some();
        style.content = match icon {
            Some(id) if self.parts.borrow().is_empty() => Content::Codicon { id, size: CODICON_SIZE as f32, color: foreground },
            _ => Content::None,
        };
        let outline = self.focused.get().then_some(self.options.borrow().styles.focus_border).flatten();
        let radius = b.radii.iter().copied().fold(0.0, f64::max);
        update_outline(&mut tree, self.element, &self.outline, outline.map(|color| OutlineSpec { color, offset, radius }));
    }

    /// A largura do conteúdo: a soma dos trechos (e do glifo do próprio elemento).
    fn content_width(&self, b: &ButtonBox) -> f64 {
        let leading = self.leading_cell();
        if self.label_element.is_some() {
            return leading + row_items(&self.parts.borrow(), b).iter().map(|i| i.cell_width).sum::<f64>();
        }
        let parts = row_items(&self.parts.borrow(), b);
        if parts.is_empty() && leading == 0.0 && self.element_icon().is_some() {
            return CODICON_SIZE;
        }
        leading + parts.iter().map(|i| i.cell_width).sum::<f64>()
    }

    /// Põe `node` na vaga à frente do rótulo (o `button.prepend(leading)` do `WorkbenchButtonBar`): o nó vira o
    /// primeiro filho do botão, e o layout reserva `16px` mais a margem de `--vscode-spacing-size40` (`4px`) até o
    /// rótulo, ou só os `16px` com a classe `monaco-button-leading-icon-only`. Chamar de novo depois de reescrever o
    /// rótulo (que no DOM apaga os filhos) o devolve à frente. `None` esvazia a vaga.
    pub fn set_leading(&self, node: Option<NodeId>) {
        self.leading.set(node);
        if let Some(node) = node {
            let mut tree = self.ui.tree_mut();
            let first = tree.children(self.element).first().copied();
            if first != Some(node) {
                tree.insert_before(self.element, node, first);
            }
        }
        self.refresh();
    }

    /// O espaço que a vaga à frente ocupa na fileira (zero sem ela).
    fn leading_cell(&self) -> f64 {
        let Some(node) = self.leading.get() else { return 0.0 };
        let gap = if self.ui.tree().has_class(node, "monaco-button-leading-icon-only") { 0.0 } else { LEADING_ICON_GAP };
        CODICON_SIZE + gap
    }

    /// O `display: flex` do botão com a vaga à frente: ela e os trechos centrados juntos em `inner`.
    fn layout_leading_row(&self, parts: &[LabelPart], b: &ButtonBox, inner: Rect) {
        let leading_cell = self.leading_cell();
        let row = Rect::new(inner.x0 + leading_cell, inner.y0, inner.x1, inner.y1);
        layout_row(&self.ui, parts, b, row);
        let Some(node) = self.leading.get() else { return };
        let total = leading_cell + row_items(parts, b).iter().map(|i| i.cell_width).sum::<f64>();
        let x = inner.x0 + (inner.width() - total) / 2.0;
        let y = inner.y0 + (inner.height() - CODICON_SIZE) / 2.0;
        self.ui.tree_mut().set_bounds(node, Rect::new(x, y, x + CODICON_SIZE, y + CODICON_SIZE));
    }

    fn layout_with(&self, b: &ButtonBox) {
        let current = self.ui.tree().bounds(self.element);
        let (text_button, short) = (self.ui.tree().has_class(self.element, "monaco-text-button"), self.label_element.is_some());
        let parts = self.parts.borrow().clone();
        let short_parts = self.short_parts.borrow().clone();
        let content_height = if short {
            0.0
        } else {
            let items = row_items(&parts, b);
            let glyph = if items.is_empty() && self.element_icon().is_some() { CODICON_SIZE } else { 0.0 };
            items.iter().map(|i| i.height).fold(glyph, f64::max)
        };
        let height = b.height.unwrap_or(if short { 28.0 } else { content_height + b.vertical_extra() });
        let (fill, width) = match self.width_mode.get() {
            WidthMode::Fixed(w) => (false, w),
            WidthMode::Css if text_button => (true, current.width()),
            WidthMode::Css => (false, self.content_width(b) + b.horizontal_extra()),
        };
        {
            let mut tree = self.ui.tree_mut();
            tree.set_fill(self.element, fill, false);
            tree.set_bounds(self.element, Rect::new(current.x0, current.y0, current.x0 + width, current.y0 + height));
        }
        let width = self.ui.tree().bounds(self.element).width();
        let (left, top) = (b.border_widths[3] + b.padding[3], b.border_widths[0] + b.padding[0]);
        let inner = Rect::new(left, top, width - b.border_widths[1] - b.padding[1], height - b.border_widths[2] - b.padding[2]);
        if let (true, Some(label), Some(label_short)) = (short, self.label_element, self.label_short_element) {
            self.layout_short_label(b, inner, label, label_short, &parts, &short_parts);
        } else {
            self.layout_leading_row(&parts, b, inner);
        }
        // O contorno acompanha a caixa.
        let outline = self.outline.get();
        if let Some(outline) = outline {
            let expand = b.outline_offset + 1.0;
            let mut tree = self.ui.tree_mut();
            tree.set_bounds(outline, Rect::new(-expand, -expand, width + expand, height + expand));
        }
    }

    /// O `.monaco-text-button-with-short-label`: o rótulo ocupa a linha inteira (`flex-basis: 100%`); se a
    /// maior palavra dele não cabe, a linha quebra e o rótulo curto (que cresce) fica na primeira linha, e o
    /// rótulo vai para uma segunda, cortada pelos 28px de altura.
    fn layout_short_label(&self, b: &ButtonBox, inner: Rect, label: NodeId, label_short: NodeId, parts: &[LabelPart], short_parts: &[LabelPart]) {
        let div = ButtonBox { line_height: 18.0, font_weight: 400, ..b.clone() };
        let min_content: f64 = row_items(parts, &div).iter().zip(parts).map(|(item, part)| match part.kind {
            LabelPartKind::Text => part.text.split_whitespace().map(|w| measure_text(w, &ui_font(div.font_size as f32, 400))).fold(0.0, f64::max),
            LabelPartKind::Icon => item.cell_width,
        }).sum();
        let fits = min_content <= inner.width();
        let div_height = 26.0;
        {
            let mut tree = self.ui.tree_mut();
            tree.style_mut(label).visible = fits;
            tree.style_mut(label_short).visible = !fits;
            tree.set_bounds(label, Rect::new(inner.x0, inner.y0, inner.x1, inner.y0 + div_height));
            tree.set_bounds(label_short, Rect::new(inner.x0, inner.y0, if fits { inner.x0 } else { inner.x1 }, inner.y0 + div_height));
        }
        let area = |w: f64| Rect::new(0.0, 0.0, w, div_height);
        layout_row(&self.ui, parts, &div, area(inner.width()));
        layout_row(&self.ui, short_parts, &div, area(if fits { 0.0 } else { inner.width() }));
    }

    fn dispose_once(&self) -> bool {
        !self.disposed.replace(true)
    }
}

/// Um item da fileira de rótulo.
struct RowItem {
    part: LabelPart,
    cell_width: f64,
    node_width: f64,
    offset: f64,
    height: f64,
}

/// `reset(container, ...elements)`: troca os trechos de rótulo de `container` pelos de `text`.
pub(crate) fn rebuild_label_parts(ui: &Ui, container: NodeId, parts: &RefCell<Vec<LabelPart>>, text: &str, support_icons: bool) {
    let old: Vec<LabelPart> = parts.borrow_mut().drain(..).collect();
    for part in old {
        ui.remove(part.node);
    }
    let segments = if support_icons { render_label_with_icons(text) } else { vec![LabelSegment::Text(text.to_owned())] };
    let mut created = Vec::new();
    let mut tree = ui.tree_mut();
    for segment in segments {
        match segment {
            LabelSegment::Text(raw) => {
                // Ignore empty segment. Without icons, `textContent` keeps the text as is.
                let segment = if support_icons { raw.trim().to_owned() } else { raw };
                if segment.is_empty() {
                    continue;
                }
                let node = tree.create(container, Role::Generic);
                created.push(LabelPart { node, kind: LabelPartKind::Text, text: segment, data_label: None });
            }
            LabelSegment::Icon(id) => {
                let node = tree.create(container, Role::Generic);
                tree.set_class_name(node, &ThemeIcon::from_id(id.clone()).as_class_name());
                created.push(LabelPart { node, kind: LabelPartKind::Icon, text: id, data_label: None });
            }
        }
    }
    *parts.borrow_mut() = created;
}

/// Os itens de uma fileira de rótulo, com o tamanho da célula (a que ocupa na fileira) e o do nó.
fn row_items(parts: &[LabelPart], b: &ButtonBox) -> Vec<RowItem> {
    parts
        .iter()
        .map(|part| match part.kind {
            LabelPartKind::Text => {
                let style = ui_font(b.font_size as f32, b.font_weight);
                let width = measure_text(&part.text, &style);
                let cell_width = match (&part.data_label, b.reserve_weight) {
                    (Some(_), Some(weight)) => width.max(measure_text(&part.text, &ui_font(b.font_size as f32, weight))),
                    _ => width,
                };
                RowItem { part: part.clone(), cell_width, node_width: width, offset: (cell_width - width) / 2.0, height: b.line_height }
            }
            LabelPartKind::Icon => {
                let margin = b.icon_margin_em * CODICON_SIZE;
                RowItem { part: part.clone(), cell_width: CODICON_SIZE + 2.0 * margin, node_width: CODICON_SIZE, offset: margin, height: CODICON_SIZE }
            }
        })
        .collect()
}

/// O `display: flex; justify-content: center; align-items: center` de uma fileira de trechos dentro de
/// `area` (relativa ao nó que os contém).
pub(crate) fn layout_row(ui: &Ui, parts: &[LabelPart], b: &ButtonBox, area: Rect) {
    let items = row_items(parts, b);
    let total: f64 = items.iter().map(|i| i.cell_width).sum();
    let foreground = b.foreground.unwrap_or(INHERITED_FOREGROUND);
    let mut x = area.x0 + (area.width() - total) / 2.0;
    let mut tree = ui.tree_mut();
    for item in items {
        let y = area.y0 + (area.height() - item.height) / 2.0;
        let left = x + item.offset;
        tree.set_bounds(item.part.node, Rect::new(left, y, left + item.node_width, y + item.height));
        tree.style_mut(item.part.node).content = match item.part.kind {
            LabelPartKind::Text => Content::Text { text: item.part.text.clone(), style: ui_font(b.font_size as f32, b.font_weight), color: foreground, line_height: b.line_height },
            // O modificador (`~spin`) é uma classe à parte; o glifo é o do nome.
            LabelPartKind::Icon => Content::Codicon { id: item.part.text.split('~').next().unwrap_or_default().to_owned(), size: CODICON_SIZE as f32, color: foreground },
        };
        x += item.cell_width;
    }
}

impl IButton for Button {
    fn element(&self) -> NodeId {
        self.element
    }

    fn on_did_click(&self) -> BaseEvent<Option<ButtonEvent>> {
        self.on_did_click.event()
    }

    fn set_label(&self, value: ButtonLabel) {
        Button::set_label(self, value);
    }

    fn set_icon(&self, icon: &ThemeIcon) {
        Button::set_icon(self, icon);
    }

    fn set_enabled(&self, value: bool) {
        Button::set_enabled(self, value);
    }

    fn enabled(&self) -> bool {
        !self.ui.tree().has_class(self.element, "disabled")
    }

    fn set_checked(&self, value: bool) {
        Button::set_checked(self, value);
    }

    fn checked(&self) -> bool {
        self.ui.tree().has_class(self.element, "checked")
    }

    fn set_title(&self, title: &str) {
        Button::set_title(self, title);
    }

    fn set_aria_label(&self, aria_label: &str) {
        Button::set_aria_label(self, aria_label);
    }

    fn focus(&self) {
        Button::focus(self);
    }

    fn has_focus(&self) -> bool {
        Button::has_focus(self)
    }
}

impl IDisposable for Button {
    fn dispose(&self) {
        if !self.dispose_once() {
            return;
        }
        self.base.dispose();
        self.ui.remove(self.element);
    }

    fn constructor_name(&self) -> &'static str {
        "Button"
    }
}

/// O contorno de foco: `outline: 1px solid <cor>` com `outline-offset`.
pub(crate) struct OutlineSpec {
    pub color: Rgba,
    pub offset: f64,
    pub radius: f64,
}

/// Mostra ou esconde o nó de contorno de `element` (criado na primeira vez que aparece). O contorno de
/// `outline-offset` negativo cai sobre a borda do elemento, e a árvore pinta a borda do pai depois dos
/// filhos: nesse caso a cor do contorno é composta sobre a da borda (o mesmo pixel que o navegador pinta).
pub(crate) fn update_outline(tree: &mut Tree, element: NodeId, slot: &Cell<Option<NodeId>>, spec: Option<OutlineSpec>) {
    let hide = |tree: &mut Tree| {
        if let Some(node) = slot.get() {
            tree.style_mut(node).visible = false;
        }
    };
    let Some(spec) = spec else {
        hide(tree);
        return;
    };
    let style = tree.style(element);
    if spec.offset < 0.0 && style.border_widths.is_none() && let Some(border) = style.border.filter(|b| b.width >= 1.0) {
        tree.style_mut(element).border = Some(Border { color: composite_over(spec.color, border.color), ..border });
        hide(tree);
        return;
    }
    let node = slot.get().unwrap_or_else(|| {
        let node = create_el(tree, element, Role::Presentation, "outline");
        tree.style_mut(node).pointer_events = PointerEvents::None;
        slot.set(Some(node));
        node
    });
    let rect = tree.bounds(element);
    let expand = spec.offset + 1.0;
    tree.set_bounds(node, Rect::new(-expand, -expand, rect.width() + expand, rect.height() + expand));
    let style = tree.style_mut(node);
    style.visible = true;
    style.border = Some(Border { width: 1.0, color: spec.color });
    style.radius = if spec.radius > 0.0 { spec.radius + expand.max(0.0) } else { 0.0 };
}

/// `top` sobre `bottom` (o operador `over`, sem pré-multiplicação).
fn composite_over(top: Rgba, bottom: Rgba) -> Rgba {
    let (a_top, a_bottom) = (f32::from(top.a) / 255.0, f32::from(bottom.a) / 255.0);
    let alpha = a_top + a_bottom * (1.0 - a_top);
    if alpha <= 0.0 {
        return Rgba::TRANSPARENT;
    }
    let channel = |t: u8, b: u8| ((f32::from(t) * a_top + f32::from(b) * a_bottom * (1.0 - a_top)) / alpha).round() as u8;
    Rgba { r: channel(top.r, bottom.r), g: channel(top.g, bottom.g), b: channel(top.b, bottom.b), a: (alpha * 255.0).round() as u8 }
}
