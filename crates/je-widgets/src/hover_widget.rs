//! Porte de `src/vs/base/browser/ui/hover/hoverWidget.ts` (a caixa, [`HoverWidget`]; a ação da barra de
//! status, [`HoverAction`]; [`ClickAction`], [`KeyDownAction`]; [`get_hover_accessible_view_hint`]) e do
//! `HoverWidget` posicionado de `src/vs/platform/hover/browser/hoverWidget.ts` ([`WorkbenchHoverWidget`], o que
//! calcula onde a dica aparece em volta do alvo, com ponteiro, e a barra de ações), com o delegado de
//! contexto do `hoverService.ts` ([`HoverContextViewDelegate`]) e um [`IHoverDelegate`] concreto
//! ([`DefaultHoverDelegate`]) que mostra a dica numa [`ContextView`](crate::context_view::ContextView).
//! Os contratos (`IHoverOptions`, `HoverPosition`, `IHoverDelegate`...) são os de [`crate::hover_api`].
//!
//! # API para quem consome
//!
//! - [`HoverWidget::new`]`(ui, fade_in)`: `container_dom_node` (`.monaco-hover`, papel `Tooltip`),
//!   `contents_dom_node` (`.monaco-hover-content`) e o `scrollbar`; o dono dimensiona os dois e chama
//!   [`HoverWidget::on_contents_changed`] (o `scanDomNode`).
//! - [`WorkbenchHoverWidget::new`]`(ui, options, styles, keybinding_label)` monta a dica de um
//!   [`IHoverOptions`]; `render(container)` a põe no contêiner de uma visão de contexto e `layout()` calcula
//!   [`WorkbenchHoverWidget::x`], [`WorkbenchHoverWidget::y`] e [`WorkbenchHoverWidget::anchor`] (o
//!   `anchorPosition` da visão). É um [`IHoverWidget`] (`dispose` esconde).
//! - [`HoverContextViewDelegate`] liga uma `WorkbenchHoverWidget` a uma `ContextView`;
//!   [`DefaultHoverDelegate`] é um [`IHoverDelegate`] que faz isso sozinho (a dica some quando o mouse sai do
//!   alvo e da dica).
//!
//! Como o DOM e o CSS viram [`je_ui`]:
//!
//! - As classes são as do DOM (`monaco-hover`, `workbench-hover`, `compact`, `with-pointer`, `fade-in`,
//!   `right-aligned`, `locked`, `hover-row`, `markdown-hover`, `status-bar`, `hover-contents`, `actions`,
//!   `action-container`, `disabled`). As regras de `hoverWidget.css` e `hover.css` com medida viram código
//!   do widget: `padding: 4px 8px` (`2px 8px` no `compact`), `line-height: 19px` e fonte de 13px (12px no
//!   `compact`), `max-width: 700px` na dica e `500px` no conteúdo, borda de 1px, raio de 5px (3px com
//!   ponteiro), barra de status de 22px, `margin: 8px 0` entre parágrafos. Cores por [`HoverStyles`]
//!   (`editorHoverWidget.*`, `textLink.*`, `focusBorder`).
//! - **Texto.** O conteúdo de texto (`pre-wrap`) e o markdown são quebrados em linhas por palavra com a medida de
//!   `text::measure_text`. O markdown é mostrado como texto simples por parágrafo (TODO(integração): o
//!   `IMarkdownRendererService` e o código, links e imagens). Conteúdo de nó (`HoverContent::Node`) é do dono:
//!   ele dimensiona o nó (sem o `padding`, a classe `html-hover-contents`).
//! - **Ponteiro.** O losango girado de `.workbench-hover-pointer:after` vira um V de linhas de 1px (o `je-ui` não
//!   gira nós), com o miolo na cor do fundo.
//! - A rolagem do conteúdo é o `ScrollableElement` do `je-widgets`; `contents_dom_node` anda com a rolagem.
//!
//! Fica de fora: a animação `fadein` (instantâneo); o `ResizeObserver` do conteúdo de nó (o dono chama
//! `layout()`/`on_contents_changed()` quando o nó muda); o foco preso (`trapFocus`: não há ordem de tabulação);
//! `blur` da janela; `row-reverse` das ações com `right-aligned`; `hr`, código e o `box-shadow`;
//! `getHoverAccessibleViewHint` sem o `IAccessibilityService` (a configuração é quem chama decide).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::TimeoutTimer;
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::key_codes::KeyCode;
use je_base::layout::{AnchorAlignment, AnchorPosition};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, none, to_disposable};
use je_base::nls::localize;
use je_base::scrollable::INewScrollDimensions;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_text::TextStyle;
use je_theme::ColorTheme;
use je_ui::{Border, Content, Cursor, EventKind, NodeId, PointerEvents, Role, Ui};

use crate::common::registered_emitter;
use crate::context_view::ContextView;
use crate::context_view_api::{Anchor, ContextViewDOMPosition, IContextViewDelegate};
use crate::dom::{StandardKeyboardEvent, create_el, create_detached_el, window_size};
use crate::hover_api::{HoverContent, HoverDelay, HoverPlacement, HoverPosition, HoverStyle, HoverTarget, IHoverAction, IHoverDelegate, IHoverDelegateOptions, IHoverOptions, IHoverTarget, IHoverWidget};
use crate::sash::is_macintosh;
use crate::scrollbar::scrollable_element::AbstractScrollableElement;
use crate::scrollbar::scrollable_element_options::ScrollableElementCreationOptions;
use crate::text::{measure_text, set_node_text, ui_font};

/// `Constants.PointerSize`.
const POINTER_SIZE: f64 = 3.0;
/// `Constants.HoverBorderWidth`.
const HOVER_BORDER_WIDTH: f64 = 2.0;
/// `Constants.HoverWindowEdgeMargin`.
const HOVER_WINDOW_EDGE_MARGIN: f64 = 2.0;
/// `.monaco-hover.workbench-hover { max-width: 700px }`.
const HOVER_MAX_WIDTH: f64 = 700.0;
/// `--vscode-hover-maxWidth` padrão (`.markdown-hover > .hover-contents`).
const CONTENT_MAX_WIDTH: f64 = 500.0;
/// `.monaco-hover.workbench-hover { line-height: 19px }`.
const LINE_HEIGHT: f64 = 19.0;

/// As cores do hover (`editorHoverWidget.*`, `textLink.*`, `focusBorder`) lidas do tema.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HoverStyles {
    pub background: Option<Rgba>,
    pub border: Option<Rgba>,
    pub foreground: Option<Rgba>,
    pub status_bar_background: Option<Rgba>,
    pub link_foreground: Option<Rgba>,
    pub link_active_foreground: Option<Rgba>,
    pub focus_border: Option<Rgba>,
}

/// Os estilos do hover com as cores resolvidas no tema.
pub fn default_hover_styles(theme: &ColorTheme) -> HoverStyles {
    HoverStyles {
        background: theme.get_color("editorHoverWidget.background"),
        border: theme.get_color("editorHoverWidget.border"),
        foreground: theme.get_color("editorHoverWidget.foreground"),
        status_bar_background: theme.get_color("editorHoverWidget.statusBarBackground"),
        link_foreground: theme.get_color("textLink.foreground"),
        link_active_foreground: theme.get_color("textLink.activeForeground"),
        focus_border: theme.get_color("focusBorder"),
    }
}

/// A cor da borda dos separadores de linha (`hoverBorder.transparent(0.5)` do participante de tema).
fn half_transparent(color: Rgba) -> Rgba {
    Rgba { a: (f32::from(color.a) * 0.5).round() as u8, ..color }
}

/// O `HoverWidget` da base: a caixa e a área de conteúdo rolável.
pub struct HoverWidget {
    base: Rc<Disposable>,
    ui: Ui,
    container_dom_node: NodeId,
    contents_dom_node: NodeId,
    scrollbar: Rc<AbstractScrollableElement>,
}

impl HoverWidget {
    /// `new HoverWidget(fadeIn)`.
    pub fn new(ui: &Ui, fade_in: bool) -> Rc<Self> {
        let base = Disposable::new();
        let (container_dom_node, contents_dom_node) = {
            let mut tree = ui.tree_mut();
            let container = create_detached_el(&mut tree, Role::Tooltip, "monaco-hover");
            tree.toggle_class(container, "fade-in", fade_in);
            tree.style_mut(container).clip = true;
            tree.style_mut(container).cursor = Some(Cursor::Default);
            let contents = create_detached_el(&mut tree, Role::Generic, "monaco-hover-content");
            (container, contents)
        };
        let scrollbar = AbstractScrollableElement::scrollable_element(ui, contents_dom_node, ScrollableElementCreationOptions { consume_mouse_wheel_if_scrollbar_is_needed: Some(true), ..Default::default() });
        base.register(scrollbar.clone());
        ui.tree_mut().append_child(container_dom_node, scrollbar.get_dom_node());
        let this = Rc::new(Self { base, ui: ui.clone(), container_dom_node, contents_dom_node, scrollbar });
        let weak = Rc::downgrade(&this);
        this.base.register(this.scrollbar.on_scroll().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.ui.tree_mut().set_bounds(this.contents_dom_node, Rect::new(-e.scroll_left, -e.scroll_top, -e.scroll_left + e.scroll_width, -e.scroll_top + e.scroll_height));
            }
        }));
        this
    }

    /// `containerDomNode`.
    pub fn container_dom_node(&self) -> NodeId {
        self.container_dom_node
    }

    /// `contentsDomNode`: o dono o dimensiona com o tamanho total do conteúdo.
    pub fn contents_dom_node(&self) -> NodeId {
        self.contents_dom_node
    }

    pub fn scrollbar(&self) -> &Rc<AbstractScrollableElement> {
        &self.scrollbar
    }

    /// `onContentsChanged()` (o `scrollbar.scanDomNode()`): a janela rolável vale a área visível do
    /// contêiner (o dono já dimensionou o nó rolável) e o conteúdo, o tamanho do `contents_dom_node`.
    pub fn on_contents_changed(&self) {
        let (visible, contents) = {
            let tree = self.ui.tree();
            (tree.bounds(self.scrollbar.get_dom_node()), tree.bounds(self.contents_dom_node))
        };
        self.scrollbar.set_scroll_dimensions(&INewScrollDimensions { width: Some(visible.width()), scroll_width: Some(contents.width()), height: Some(visible.height()), scroll_height: Some(contents.height()) });
    }
}

impl IDisposable for HoverWidget {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `new ClickAction(container, run)`: o clique no contêiner roda `run` (parando o evento).
pub struct ClickAction {
    base: Rc<Disposable>,
}

impl ClickAction {
    pub fn new(ui: &Ui, container: NodeId, run: Rc<dyn Fn(NodeId)>) -> Rc<Self> {
        let base = Disposable::new();
        base.register(ui.listen(container, EventKind::Click, false, move |e| {
            e.stop(true);
            run(container);
        }));
        Rc::new(Self { base })
    }
}

impl IDisposable for ClickAction {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `new KeyDownAction(container, run, keyCodes)`: as teclas rodam `run` (parando o evento).
pub struct KeyDownAction {
    base: Rc<Disposable>,
}

impl KeyDownAction {
    pub fn new(ui: &Ui, container: NodeId, run: Rc<dyn Fn(NodeId)>, key_codes: Vec<KeyCode>) -> Rc<Self> {
        let base = Disposable::new();
        base.register(ui.listen(container, EventKind::KeyDown, false, move |e| {
            let Some(event) = StandardKeyboardEvent::from_event(e) else { return };
            if key_codes.iter().any(|key| event.equals(key.0 as u32)) {
                e.stop(true);
                run(container);
            }
        }));
        Rc::new(Self { base })
    }
}

impl IDisposable for KeyDownAction {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `getHoverAccessibleViewHint(shouldHaveHint, keybinding)`.
pub fn get_hover_accessible_view_hint(should_have_hint: bool, keybinding: Option<&str>) -> String {
    match (should_have_hint, keybinding.filter(|k| !k.is_empty())) {
        (true, Some(keybinding)) => localize("acessibleViewHint", "Inspect this in the accessible view with {0}.", &[keybinding]),
        (true, None) => localize("acessibleViewHintNoKbOpen", "Inspect this in the accessible view via the command Open Accessible View which is currently not triggerable via keybinding.", &[]),
        (false, _) => String::new(),
    }
}

/// O `{ label, iconClass?, run, commandId }` do `HoverAction.render`.
pub type HoverActionOptions = IHoverAction;

/// A fonte da barra de status do hover (`font-size: 12px; line-height: 22px`).
fn status_font() -> TextStyle {
    ui_font(12.0, 400)
}

/// `.hover-row.status-bar { line-height: 22px }`.
const STATUS_LINE_HEIGHT: f64 = 22.0;
/// `.action-container { margin-right: 16px }`.
const ACTION_MARGIN_RIGHT: f64 = 16.0;
/// `.icon { padding-right: 4px }`.
const ACTION_ICON_PADDING: f64 = 4.0;

/// O `linkHandler` das opções: recebe o endereço do link clicado.
pub type LinkHandler = Rc<dyn Fn(&str)>;

/// O `lookupKeybinding(commandId)?.getLabel()` do serviço de atalhos.
pub type KeybindingLookup<'a> = &'a dyn Fn(&str) -> Option<String>;

/// `HoverAction`: o link de ação da barra de status (ícone opcional e rótulo, com o atalho).
pub struct HoverAction {
    base: Rc<Disposable>,
    ui: Ui,
    pub action_label: String,
    pub action_keybinding_label: Option<String>,
    pub action_rendered_label: String,
    pub action_container: NodeId,
}

impl HoverAction {
    /// `HoverAction.render(parent, actionOptions, keybindingLabel)`: acrescenta o `.action-container` à
    /// direita dos que já estão em `parent` (o `display: flex` da `.actions`, com `margin-right: 16px`).
    pub fn render(ui: &Ui, parent: NodeId, action_options: &HoverActionOptions, keybinding_label: Option<&str>, styles: &HoverStyles) -> Rc<Self> {
        let base = Disposable::new();
        let action_rendered_label = keybinding_label.map_or_else(|| action_options.label.clone(), |k| format!("{} ({k})", action_options.label));
        let font = status_font();
        let link = styles.link_foreground.unwrap_or(Rgba::new(0x3b, 0x8e, 0xea, 255));
        let (container, icon_width) = {
            let mut tree = ui.tree_mut();
            let left = tree.children(parent).last().map_or(0.0, |last| tree.bounds(*last).x1 + ACTION_MARGIN_RIGHT);
            let container = create_el(&mut tree, parent, Role::Generic, "action-container");
            tree.style_mut(container).cursor = Some(Cursor::Pointer);
            tree.style_mut(container).clip = true;
            let action = create_el(&mut tree, container, Role::Button, "action");
            let mut x = 0.0;
            if let Some(icon_class) = action_options.icon_class.as_deref().filter(|c| !c.is_empty()) {
                let icon = create_el(&mut tree, action, Role::Generic, &format!("icon {icon_class}"));
                let id = icon_class.split_whitespace().find_map(|c| c.strip_prefix("codicon-")).unwrap_or_default().to_owned();
                tree.style_mut(icon).content = Content::Codicon { id, size: 12.0, color: link };
                tree.set_bounds(icon, Rect::new(0.0, 0.0, 12.0, STATUS_LINE_HEIGHT));
                x = 12.0 + ACTION_ICON_PADDING;
            }
            let label = create_el(&mut tree, action, Role::Generic, "");
            let width = set_node_text(&mut tree, label, &action_rendered_label, &font, link, STATUS_LINE_HEIGHT);
            tree.set_bounds(label, Rect::new(x, 0.0, x + width, STATUS_LINE_HEIGHT));
            tree.set_bounds(action, Rect::new(0.0, 0.0, x + width, STATUS_LINE_HEIGHT));
            tree.set_bounds(container, Rect::new(left, 0.0, left + x + width, STATUS_LINE_HEIGHT));
            (container, x)
        };
        let _ = icon_width;
        let run = action_options.run.clone();
        base.register(ClickAction::new(ui, container, run.clone()));
        base.register(KeyDownAction::new(ui, container, run, vec![KeyCode::Enter, KeyCode::Space]));
        let this = Rc::new(Self { base, ui: ui.clone(), action_label: action_options.label.clone(), action_keybinding_label: keybinding_label.map(str::to_owned), action_rendered_label, action_container: container });
        this.set_enabled(true);
        this
    }

    /// `setEnabled(enabled)`: desabilitado perde o ponteiro e fica a 40% de opacidade.
    pub fn set_enabled(&self, enabled: bool) {
        let mut tree = self.ui.tree_mut();
        tree.toggle_class(self.action_container, "disabled", !enabled);
        let style = tree.style_mut(self.action_container);
        style.pointer_events = if enabled { PointerEvents::Inherit } else { PointerEvents::None };
        style.opacity = if enabled { 1.0 } else { 0.4 };
        style.cursor = Some(if enabled { Cursor::Pointer } else { Cursor::Default });
    }

    /// A largura que o contêiner ocupa em fila (sem a margem da direita).
    pub fn width(&self) -> f64 {
        self.ui.tree().bounds(self.action_container).width()
    }
}

impl IDisposable for HoverAction {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O retângulo de um alvo em coordenadas da janela.
#[derive(Clone, Copy, Debug)]
struct TargetRect {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
    width: f64,
    height: f64,
    center_x: f64,
    center_y: f64,
}

/// `ElementHoverTarget`.
struct ElementHoverTarget(NodeId);

impl IHoverTarget for ElementHoverTarget {
    fn target_elements(&self) -> Vec<NodeId> {
        vec![self.0]
    }
}

/// Um parágrafo do conteúdo já quebrado em linhas.
struct Block {
    lines: Vec<String>,
}

/// `white-space: pre-wrap`: quebra `text` em linhas de no máximo `max_width`, por palavra, respeitando as quebras.
pub(crate) fn wrap_text(text: &str, style: &TextStyle, max_width: f64) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_inclusive(' ') {
            let candidate = format!("{line}{word}");
            if line.is_empty() || measure_text(candidate.trim_end(), style) <= max_width {
                line = candidate;
            } else {
                lines.push(std::mem::take(&mut line).trim_end().to_owned());
                line = word.to_owned();
            }
        }
        lines.push(line.trim_end().to_owned());
    }
    lines
}

/// `CompositeMouseTracker`: sabe se o mouse está sobre algum dos nós, com um atraso para ir de um ao outro.
struct CompositeMouseTracker {
    base: Rc<Disposable>,
    ui: Ui,
    elements: RefCell<Vec<NodeId>>,
    is_mouse_in: Cell<bool>,
    suppress_next_mouse_out: Cell<bool>,
    timer: Rc<TimeoutTimer>,
    on_mouse_out: Rc<Emitter<()>>,
    event_debounce_delay: u64,
    weak_self: Weak<CompositeMouseTracker>,
}

impl CompositeMouseTracker {
    fn new(ui: &Ui, elements: Vec<NodeId>) -> Rc<Self> {
        let base = Disposable::new();
        let timer = TimeoutTimer::new();
        base.register(timer.clone());
        let on_mouse_out = registered_emitter::<()>(&base);
        let this = Rc::new_cyclic(|weak| Self { base, ui: ui.clone(), elements: RefCell::new(Vec::new()), is_mouse_in: Cell::new(true), suppress_next_mouse_out: Cell::new(false), timer, on_mouse_out, event_debounce_delay: 200, weak_self: weak.clone() });
        for element in elements {
            this.add_element(element);
        }
        this
    }

    fn add_element(&self, element: NodeId) -> DisposableRef {
        if self.elements.borrow().contains(&element) {
            return none();
        }
        self.elements.borrow_mut().push(element);
        let (enter, leave) = (self.weak_self.clone(), self.weak_self.clone());
        let enter_listener = self.ui.listen(element, EventKind::PointerEnter, false, move |_| {
            if let Some(this) = enter.upgrade() {
                this.is_mouse_in.set(true);
                this.suppress_next_mouse_out.set(false);
                this.timer.cancel();
            }
        });
        let leave_listener = self.ui.listen(element, EventKind::PointerLeave, false, move |_| {
            if let Some(this) = leave.upgrade() {
                this.on_target_mouse_leave();
            }
        });
        let weak = self.weak_self.clone();
        let removal = to_disposable(move || {
            if let Some(this) = weak.upgrade() {
                this.elements.borrow_mut().retain(|e| *e != element);
            }
        });
        let all: DisposableRef = je_base::lifecycle::combined_disposable(vec![enter_listener, leave_listener, removal]);
        self.base.register(all.clone());
        all
    }

    fn on_target_mouse_leave(&self) {
        self.is_mouse_in.set(false);
        // Avalia de forma assíncrona se o mouse continua fora, para os outros alvos terem a chance de disparar o `enter`.
        let weak = self.weak_self.clone();
        self.timer.cancel_and_set(
            move || {
                if let Some(this) = weak.upgrade()
                    && !this.is_mouse_in.get()
                    && !this.suppress_next_mouse_out.get()
                {
                    this.on_mouse_out.fire(());
                }
            },
            self.event_debounce_delay,
        );
    }

    /// `suppressPendingMouseOut()`.
    fn suppress_pending_mouse_out(&self) {
        if !self.is_mouse_in.get() {
            self.suppress_next_mouse_out.set(true);
        }
    }
}

impl IDisposable for CompositeMouseTracker {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O `HoverWidget` do workbench.
pub struct WorkbenchHoverWidget {
    base: Rc<Disposable>,
    weak_self: Weak<WorkbenchHoverWidget>,
    ui: Ui,
    styles: HoverStyles,
    hover: Rc<HoverWidget>,
    hover_container: NodeId,
    hover_pointer: Option<NodeId>,
    target: Rc<dyn IHoverTarget>,
    clipping_element: Option<NodeId>,
    anchor_alignment: AnchorAlignment,
    link_handler: Option<LinkHandler>,
    compact: bool,
    html_content: Option<NodeId>,
    blocks: Vec<Block>,
    status_rows: Vec<StatusRow>,
    is_disposed: Cell<bool>,
    hover_position: Cell<HoverPosition>,
    force_position: bool,
    x: Cell<f64>,
    y: Cell<f64>,
    is_locked: Cell<bool>,
    max_height_ratio_relative_to_window: f64,
    mouse_tracker: Rc<CompositeMouseTracker>,
    lock_mouse_tracker: Rc<CompositeMouseTracker>,
    /// `style.maxWidth` do contêiner (as adaptações de posição o estreitam).
    max_width_override: Cell<Option<f64>>,
    /// `style.maxHeight` do contêiner, de `adjustHoverMaxHeight`.
    max_height: Cell<Option<f64>>,
    /// O padding do contêiner do ponteiro: `(esquerda, cima, direita, baixo)`.
    container_padding: Cell<(f64, f64, f64, f64)>,
    box_size: Cell<(f64, f64)>,
    on_dispose: Rc<Emitter<()>>,
    on_request_layout: Rc<Emitter<()>>,
    actions: RefCell<Vec<Rc<HoverAction>>>,
    contents_element: NodeId,
}

/// Uma linha `.hover-row.status-bar`: o nó e o tamanho do que tem dentro.
struct StatusRow {
    node: NodeId,
    width: f64,
}

impl WorkbenchHoverWidget {
    /// `new HoverWidget(options, ...)`. `keybinding_label(commandId)` é o `lookupKeybinding(...).getLabel()`.
    pub fn new(ui: &Ui, mut options: IHoverOptions, styles: HoverStyles, keybinding_label: Option<KeybindingLookup<'_>>) -> Rc<Self> {
        let base = Disposable::new();
        let target: Rc<dyn IHoverTarget> = match &options.target {
            HoverTarget::Target(target) => target.clone(),
            HoverTarget::Node(node) => Rc::new(ElementHoverTarget(*node)),
        };
        let clipping_element = options.position.as_ref().and_then(|p| p.clipping_element);
        let anchor_alignment = options.position.as_ref().and_then(|p| p.anchor_alignment).unwrap_or(AnchorAlignment::Left);

        let mut appearance = options.appearance.unwrap_or_default();
        match options.style {
            Some(HoverStyle::Pointer) => {
                appearance.compact.get_or_insert(true);
                appearance.show_pointer.get_or_insert(true);
            }
            Some(HoverStyle::Mouse) => {
                appearance.compact.get_or_insert(true);
            }
            None => {}
        }
        options.appearance = Some(appearance);
        let compact = appearance.compact == Some(true);
        let show_pointer = appearance.show_pointer == Some(true);

        let hover = HoverWidget::new(ui, appearance.skip_fade_in_animation != Some(true));
        base.register(hover.clone());
        let hover_container = {
            let mut tree = ui.tree_mut();
            let container = create_detached_el(&mut tree, Role::Generic, "workbench-hover-container");
            let box_node = hover.container_dom_node();
            tree.add_class(box_node, "workbench-hover");
            if compact {
                tree.add_class(box_node, "compact");
            }
            if show_pointer {
                tree.add_class(box_node, "with-pointer");
            }
            for class in &options.additional_classes {
                tree.add_class(box_node, class);
            }
            let style = tree.style_mut(box_node);
            style.background = styles.background;
            style.border = styles.border.map(|color| Border { width: 1.0, color });
            style.radius = if show_pointer { 3.0 } else { 5.0 };
            style.z_index = 40;
            container
        };
        let hover_pointer = show_pointer.then(|| {
            let mut tree = ui.tree_mut();
            let pointer = create_el(&mut tree, hover_container, Role::Generic, "workbench-hover-pointer");
            tree.style_mut(pointer).pointer_events = PointerEvents::None;
            tree.style_mut(pointer).z_index = 41;
            pointer
        });
        ui.tree_mut().append_child(hover_container, hover.container_dom_node());

        let max_height_ratio = appearance.max_height_ratio.filter(|r| *r > 0.0 && *r <= 1.0).unwrap_or(0.5);
        // Por padrão a dica vai acima quando a posição não foi dada
        let hover_position = options.position.as_ref().and_then(|p| p.hover_position).unwrap_or(HoverPosition::Above);

        // Não deixa o mousedown sair do widget, senão o `preventDefault` impediria selecionar texto.
        base.register(ui.listen(hover.container_dom_node(), EventKind::PointerDown, false, |e| e.stop_propagation()));
        // Esconde a dica com Escape
        let weak_for_keys: Rc<RefCell<Weak<WorkbenchHoverWidget>>> = Rc::new(RefCell::new(Weak::new()));
        {
            let weak = weak_for_keys.clone();
            base.register(ui.listen(hover.container_dom_node(), EventKind::KeyDown, false, move |e| {
                if StandardKeyboardEvent::from_event(e).is_some_and(|k| k.equals(KeyCode::Escape.0 as u32))
                    && let Some(this) = weak.borrow().upgrade()
                {
                    this.dispose();
                }
            }));
        }

        // Linhas de conteúdo
        let row_element;
        let contents_element;
        let mut blocks = Vec::new();
        let mut html_content = None;
        {
            let mut tree = ui.tree_mut();
            row_element = create_el(&mut tree, hover.contents_dom_node(), Role::Generic, "hover-row markdown-hover");
            contents_element = create_el(&mut tree, row_element, Role::Generic, "hover-contents");
            match &options.content {
                HoverContent::Text(_) | HoverContent::Markdown(_) => {}
                HoverContent::Node(node) => {
                    tree.add_class(contents_element, "html-hover-contents");
                    tree.append_child(contents_element, *node);
                    html_content = Some(*node);
                }
            }
        }
        match &options.content {
            HoverContent::Text(text) => blocks.push(Block { lines: vec![text.clone()] }),
            HoverContent::Markdown(markdown) => {
                // TODO(integração): o `IMarkdownRendererService` renderiza o markdown de verdade; aqui cada parágrafo é texto.
                blocks.extend(markdown.value.to_string().split("\n\n").filter(|p| !p.trim().is_empty()).map(|p| Block { lines: vec![p.trim().replace('\n', " ")] }));
            }
            HoverContent::Node(_) => {}
        }

        // Barra de status com as ações
        let mut status_rows = Vec::new();
        let mut actions = Vec::new();
        if !options.actions.is_empty() {
            let (row, actions_element) = {
                let mut tree = ui.tree_mut();
                let row = create_el(&mut tree, hover.container_dom_node(), Role::Generic, "hover-row status-bar");
                let actions_element = create_el(&mut tree, row, Role::Generic, "actions");
                tree.style_mut(actions_element).background = styles.status_bar_background;
                (row, actions_element)
            };
            let mut total = 0.0;
            for action in &options.actions {
                let label = keybinding_label.and_then(|lookup| lookup(&action.command_id));
                let weak = weak_for_keys.clone();
                let run = action.run.clone();
                let rendered = HoverAction::render(
                    ui,
                    actions_element,
                    &IHoverAction {
                        label: action.label.clone(),
                        command_id: action.command_id.clone(),
                        icon_class: action.icon_class.clone(),
                        run: Rc::new(move |e| {
                            run(e);
                            if let Some(this) = weak.borrow().upgrade() {
                                this.dispose();
                            }
                        }),
                    },
                    label.as_deref(),
                    &styles,
                );
                total = ui.tree().bounds(rendered.action_container).x1 + ACTION_MARGIN_RIGHT;
                base.register(rendered.clone());
                actions.push(rendered);
            }
            status_rows.push(StatusRow { node: row, width: total + 8.0 });
        }

        // Decide se a dica some ao passar o mouse sobre ela
        let hide_on_hover = if !options.actions.is_empty() {
            // Com ações, exige o mouse sobre a dica para alcançá-las
            false
        } else if let Some(hide) = options.persistence.as_ref().and_then(|p| p.hide_on_hover) {
            hide
        } else {
            // Sem valor: vale para texto, e para markdown sem link (teste ingênuo de `](` e `</a>`)
            match &options.content {
                HoverContent::Text(_) => true,
                HoverContent::Markdown(markdown) => {
                    let value = markdown.value.to_string();
                    !value.contains("](") && !value.contains("</a>")
                }
                HoverContent::Node(_) => false,
            }
        };

        // A dica de como alcançar o conteúdo com o mouse
        if appearance.show_hover_hint == Some(true) {
            let mut tree = ui.tree_mut();
            let row = create_el(&mut tree, hover.container_dom_node(), Role::Generic, "hover-row status-bar");
            let info = create_el(&mut tree, row, Role::Generic, "info");
            let text = localize("hoverhint", "Hold {0} key to mouse over", &[if is_macintosh() { "Option" } else { "Alt" }]);
            let font = status_font();
            let width = set_node_text(&mut tree, info, &text, &font, styles.foreground.unwrap_or(Rgba::new(204, 204, 204, 255)), STATUS_LINE_HEIGHT);
            tree.set_bounds(info, Rect::new(8.0, 0.0, 8.0 + width, STATUS_LINE_HEIGHT));
            status_rows.push(StatusRow { node: row, width: width + 16.0 });
        }

        let mut tracker_targets = target.target_elements();
        if !hide_on_hover {
            tracker_targets.push(hover_container);
        }
        let mouse_tracker = CompositeMouseTracker::new(ui, tracker_targets);
        base.register(mouse_tracker.clone());
        // Outro rastreador quando `hide_on_hover`, para seguir o mouse na dica também travada.
        let lock_mouse_tracker = if hide_on_hover {
            let mut targets = target.target_elements();
            targets.push(hover_container);
            let tracker = CompositeMouseTracker::new(ui, targets);
            base.register(tracker.clone());
            tracker
        } else {
            mouse_tracker.clone()
        };

        let on_dispose = registered_emitter::<()>(&base);
        let on_request_layout = registered_emitter::<()>(&base);
        let this = Rc::new_cyclic(|weak| WorkbenchHoverWidget {
            base,
            weak_self: weak.clone(),
            ui: ui.clone(),
            styles,
            hover,
            hover_container,
            hover_pointer,
            target,
            clipping_element,
            anchor_alignment,
            link_handler: options.link_handler.clone(),
            compact,
            html_content,
            blocks,
            status_rows,
            is_disposed: Cell::new(false),
            hover_position: Cell::new(hover_position),
            force_position: options.position.as_ref().and_then(|p| p.force_position) == Some(true),
            x: Cell::new(0.0),
            y: Cell::new(0.0),
            is_locked: Cell::new(false),
            max_height_ratio_relative_to_window: max_height_ratio,
            mouse_tracker: mouse_tracker.clone(),
            lock_mouse_tracker: lock_mouse_tracker.clone(),
            max_width_override: Cell::new(None),
            max_height: Cell::new(None),
            container_padding: Cell::new((0.0, 0.0, 0.0, 0.0)),
            box_size: Cell::new((0.0, 0.0)),
            on_dispose,
            on_request_layout,
            actions: RefCell::new(actions),
            contents_element,
        });
        *weak_for_keys.borrow_mut() = Rc::downgrade(&this);
        for tracker in [mouse_tracker, lock_mouse_tracker] {
            let weak = this.weak_self.clone();
            this.base.register(tracker.on_mouse_out.event().listen(move |_| {
                if let Some(this) = weak.upgrade()
                    && !this.is_locked.get()
                {
                    this.dispose();
                }
            }));
        }
        this
    }

    pub fn is_disposed(&self) -> bool {
        self.is_disposed.get()
    }

    /// `isMouseIn`.
    pub fn is_mouse_in(&self) -> bool {
        self.lock_mouse_tracker.is_mouse_in.get()
    }

    /// `domNode`: o `.monaco-hover`.
    pub fn dom_node(&self) -> NodeId {
        self.hover.container_dom_node()
    }

    /// O `.workbench-hover-container`, filho do contêiner da visão de contexto depois de `render`.
    pub fn hover_container(&self) -> NodeId {
        self.hover_container
    }

    pub fn on_dispose(&self) -> BaseEvent<()> {
        self.on_dispose.event()
    }

    pub fn on_request_layout(&self) -> BaseEvent<()> {
        self.on_request_layout.event()
    }

    /// `anchor`: o `AnchorPosition` da visão de contexto.
    pub fn anchor(&self) -> AnchorPosition {
        if self.hover_position.get() == HoverPosition::Below { AnchorPosition::Below } else { AnchorPosition::Above }
    }

    pub fn x(&self) -> f64 {
        self.x.get()
    }

    pub fn y(&self) -> f64 {
        self.y.get()
    }

    /// A posição atual (pode virar outra depois de `layout`, se não couber).
    pub fn hover_position(&self) -> HoverPosition {
        self.hover_position.get()
    }

    /// `isLocked`.
    pub fn is_locked(&self) -> bool {
        self.is_locked.get()
    }

    /// `set isLocked`: segurar Alt trava a dica (ela não some e dá para passar o mouse).
    pub fn set_is_locked(&self, locked: bool) {
        if self.is_locked.replace(locked) != locked {
            self.ui.tree_mut().toggle_class(self.hover_container, "locked", locked);
        }
    }

    /// `addMouseTrackingElement(element)`: o mouse sobre o nó vale como estar na dica (dicas aninhadas).
    pub fn add_mouse_tracking_element(&self, element: NodeId) -> DisposableRef {
        self.lock_mouse_tracker.add_element(element)
    }

    pub fn link_handler(&self) -> Option<LinkHandler> {
        self.link_handler.clone()
    }

    pub fn focus(&self) {
        self.ui.focus(self.hover.container_dom_node());
    }

    pub fn hide(&self) {
        self.dispose();
    }

    /// `render(container)`: põe a dica no contêiner e calcula a posição.
    pub fn render(&self, container: NodeId) {
        self.ui.tree_mut().append_child(container, self.hover_container);
        self.layout();
    }

    fn font(&self) -> TextStyle {
        ui_font(if self.compact { 12.0 } else { 13.0 }, 400)
    }

    /// O tamanho da caixa `.monaco-hover` (com a borda) e o do conteúdo, com `max-width`/`max-height` em vigor:
    /// `(largura, altura, altura do conteúdo, altura fora do conteúdo)`.
    fn measure(&self) -> (f64, f64, f64, f64) {
        let font = self.font();
        let max_box_width = self.max_width_override.get().map_or(HOVER_MAX_WIDTH, |w| w.min(HOVER_MAX_WIDTH));
        let padding_y = if self.compact { 4.0 } else { 8.0 };
        let (content_width, content_height) = if let Some(node) = self.html_content {
            let r = self.ui.tree().bounds(node);
            (r.width(), r.height())
        } else {
            let max_text = (max_box_width - HOVER_BORDER_WIDTH - 16.0).clamp(0.0, CONTENT_MAX_WIDTH);
            let mut width = 0.0_f64;
            let mut height = 0.0;
            for (i, block) in self.blocks.iter().enumerate() {
                let lines: Vec<String> = block.lines.iter().flat_map(|l| wrap_text(l, &font, max_text)).collect();
                // A largura de ajuste ao conteúdo (`shrink-to-fit`) é `min(max-content, disponível)`: um parágrafo que
                // quebra ocupa a largura toda, não a da sua linha mais larga depois da quebra (medido, H26).
                width = block.lines.iter().flat_map(|l| l.split('\n')).map(|l| measure_text(l.trim_end(), &font).min(max_text)).fold(width, f64::max);
                height += lines.len() as f64 * LINE_HEIGHT + if i > 0 { 8.0 } else { 0.0 };
            }
            (width + 16.0, height + padding_y)
        };
        let status_width = self.status_rows.iter().map(|r| r.width).fold(0.0, f64::max);
        let status_height: f64 = self.status_rows.iter().map(|_| STATUS_LINE_HEIGHT + if self.styles.border.is_some() { 1.0 } else { 0.0 }).sum();
        let box_width = (content_width.max(status_width) + HOVER_BORDER_WIDTH).min(max_box_width);
        let outside = HOVER_BORDER_WIDTH + status_height;
        let contents_height = match self.max_height.get() {
            Some(max) => content_height.min((max - outside).max(0.0)),
            None => content_height,
        };
        (box_width, contents_height + outside, content_height, outside)
    }

    /// Escreve o tamanho e as posições dos nós a partir de `measure`, e devolve o tamanho da caixa.
    fn apply_box(&self) -> (f64, f64) {
        let (box_width, box_height, full_content_height, _) = self.measure();
        self.box_size.set((box_width, box_height));
        let font = self.font();
        let padding_y = if self.compact { 2.0 } else { 4.0 };
        let max_box_width = self.max_width_override.get().map_or(HOVER_MAX_WIDTH, |w| w.min(HOVER_MAX_WIDTH));
        let max_text = (max_box_width - HOVER_BORDER_WIDTH - 16.0).clamp(0.0, CONTENT_MAX_WIDTH);
        let inner_width = box_width - HOVER_BORDER_WIDTH;
        let color = self.styles.foreground.unwrap_or(Rgba::new(204, 204, 204, 255));
        let (pad_l, pad_t, _, _) = self.container_padding.get();
        let status_height: f64 = self.status_rows.iter().map(|_| STATUS_LINE_HEIGHT + if self.styles.border.is_some() { 1.0 } else { 0.0 }).sum();
        let visible_contents = box_height - HOVER_BORDER_WIDTH - status_height;
        let mut tree = self.ui.tree_mut();
        // Linhas de texto: um nó por linha dentro de `.hover-contents`.
        for child in tree.children(self.contents_element).to_vec() {
            if Some(child) != self.html_content {
                tree.remove(child);
            }
        }
        if self.html_content.is_none() {
            let mut y = padding_y;
            for (i, block) in self.blocks.iter().enumerate() {
                if i > 0 {
                    y += 8.0;
                }
                for line in block.lines.iter().flat_map(|l| wrap_text(l, &font, max_text)) {
                    let node = create_el(&mut tree, self.contents_element, Role::Generic, "hover-line");
                    let width = set_node_text(&mut tree, node, &line, &font, color, LINE_HEIGHT);
                    tree.set_bounds(node, Rect::new(8.0, y, 8.0 + width, y + LINE_HEIGHT));
                    y += LINE_HEIGHT;
                }
            }
            tree.set_bounds(self.contents_element, Rect::new(0.0, 0.0, inner_width, full_content_height));
        } else if let Some(node) = self.html_content {
            let size = tree.bounds(node);
            tree.set_bounds(node, Rect::new(0.0, 0.0, size.width(), size.height()));
            tree.set_bounds(self.contents_element, Rect::new(0.0, 0.0, inner_width, full_content_height));
        }
        let row = tree.parent(self.contents_element).expect("a linha de conteúdo");
        tree.set_bounds(row, Rect::new(0.0, 0.0, inner_width, full_content_height));
        tree.set_bounds(self.hover.contents_dom_node(), Rect::new(0.0, 0.0, inner_width, full_content_height));
        let scroll_node = self.hover.scrollbar().get_dom_node();
        // A borda de 1px do `.monaco-hover` fica por dentro da caixa no `je-ui`: o conteúdo entra deslocado dela
        // (medido contra o oráculo, H26: sem isso o texto saía 1px acima e à esquerda).
        let border = HOVER_BORDER_WIDTH / 2.0;
        tree.set_bounds(scroll_node, Rect::new(border, border, border + inner_width, border + visible_contents));
        // As linhas da barra de status, abaixo do conteúdo.
        let mut y = border + visible_contents;
        for (i, row) in self.status_rows.iter().enumerate() {
            let has_border = self.styles.border.is_some() && (i > 0 || self.html_content.is_some() || !self.blocks.is_empty());
            let height = STATUS_LINE_HEIGHT + if self.styles.border.is_some() { 1.0 } else { 0.0 };
            tree.set_bounds(row.node, Rect::new(border, y, border + inner_width, y + height));
            if has_border && let Some(border) = self.styles.border {
                tree.style_mut(row.node).border = None;
                let line = tree.children(row.node).iter().copied().find(|c| tree.has_class(*c, "row-border"));
                let line = line.unwrap_or_else(|| create_el(&mut tree, row.node, Role::Generic, "row-border"));
                tree.style_mut(line).background = Some(half_transparent(border));
                tree.set_bounds(line, Rect::new(0.0, 0.0, inner_width, 1.0));
            }
            for child in tree.children(row.node).to_vec() {
                if tree.has_class(child, "actions") {
                    tree.set_bounds(child, Rect::new(0.0, if self.styles.border.is_some() { 1.0 } else { 0.0 }, inner_width, height));
                    tree.style_mut(child).clip = true;
                    // `padding: 0 8px` nas ações: os contêineres entram deslocados.
                    for action in tree.children(child).to_vec() {
                        let r = tree.bounds(action);
                        let shifted = if r.x0 < 8.0 { r + je_render::kurbo::Vec2::new(8.0, 0.0) } else { r };
                        tree.set_bounds(action, shifted);
                    }
                } else if tree.has_class(child, "info") {
                    let r = tree.bounds(child);
                    tree.set_bounds(child, Rect::new(8.0, 1.0 * f64::from(u8::from(self.styles.border.is_some())), r.width() + 8.0, height));
                }
            }
            y += height;
        }
        let box_node = self.hover.container_dom_node();
        tree.set_bounds(box_node, Rect::new(pad_l, pad_t, pad_l + box_width, pad_t + box_height));
        let (pl, pt, pr, pb) = self.container_padding.get();
        tree.set_bounds(self.hover_container, Rect::new(-pl, -pt, -pl + pl + box_width + pr, -pt + pt + box_height + pb));
        drop(tree);
        self.hover.on_contents_changed();
        (box_width, box_height)
    }

    /// O retângulo do alvo em coordenadas da janela, já recortado pelo `clippingElement`.
    fn target_rect(&self) -> TargetRect {
        let first = self.target.target_elements()[0];
        let tree = self.ui.tree();
        let r = tree.absolute_bounds(first);
        let (mut top, mut right, mut bottom, mut left) = (r.y0, r.x1, r.y1, r.x0);
        if let Some(clip) = self.clipping_element {
            let c = tree.absolute_bounds(clip);
            let (v_top, v_right, v_bottom, v_left) = (top.max(c.y0), right.min(c.x1), bottom.min(c.y1), left.max(c.x0));
            // Mantém o alvo inteiro quando nada dele está visível
            if v_left <= v_right && v_top <= v_bottom {
                (top, right, bottom, left) = (v_top, v_right, v_bottom, v_left);
            }
        }
        let (width, height) = (right - left, bottom - top);
        TargetRect { left, right, top, bottom, width, height, center_x: left + width / 2.0, center_y: top + height / 2.0 }
    }

    /// `layout()`: calcula a posição da dica em volta do alvo (e a posição final, se a pedida não couber).
    pub fn layout(&self) {
        // O reposicionamento cancela os mouseout pendentes (o mouse pode voltar para dentro depois).
        self.mouse_tracker.suppress_pending_mouse_out();
        if !Rc::ptr_eq(&self.lock_mouse_tracker, &self.mouse_tracker) {
            self.lock_mouse_tracker.suppress_pending_mouse_out();
        }

        self.ui.tree_mut().remove_class(self.hover.container_dom_node(), "right-aligned");
        self.max_width_override.set(None);
        self.max_height.set(None);
        self.container_padding.set((0.0, 0.0, 0.0, 0.0));
        self.apply_box();

        let mut target = self.target_rect();
        // Ajustam a posição conforme o espaço.
        self.adjust_horizontal_hover_position(&target);
        self.adjust_vertical_hover_position(&target);
        // Limita a altura máxima da dica.
        self.adjust_hover_max_height(&target);

        // Desloca a dica quando há ponteiro, para alinhar com o alvo
        if self.hover_pointer.is_some() {
            let p = POINTER_SIZE;
            match self.hover_position.get() {
                HoverPosition::Right => {
                    target.left += p;
                    target.right += p;
                    self.container_padding.set((p, 0.0, 0.0, 0.0));
                }
                HoverPosition::Left => {
                    target.left -= p;
                    target.right -= p;
                    self.container_padding.set((0.0, 0.0, p, 0.0));
                }
                HoverPosition::Below => {
                    target.top += p;
                    target.bottom += p;
                    self.container_padding.set((0.0, p, 0.0, 0.0));
                }
                HoverPosition::Above => {
                    target.top -= p;
                    target.bottom -= p;
                    self.container_padding.set((0.0, 0.0, 0.0, p));
                }
            }
            target.center_x = target.left + target.width / 2.0;
            target.center_y = target.top + target.height / 2.0;
        }
        self.apply_box();

        self.compute_x_coordinate(&target);
        self.compute_y_coordinate(&target);

        if self.hover_pointer.is_some() {
            self.set_hover_pointer_position(&target);
        }
    }

    /// `clientWidth` da caixa: a largura sem as bordas.
    fn client_size(&self) -> (f64, f64) {
        let (w, h) = self.box_size.get();
        (w - HOVER_BORDER_WIDTH, h - HOVER_BORDER_WIDTH)
    }

    fn compute_x_coordinate(&self, target: &TargetRect) {
        let (client_width, _) = self.client_size();
        let hover_width = client_width + HOVER_BORDER_WIDTH;
        let document_client_left = 0.0;

        let mut x;
        if let Some(target_x) = self.target.x() {
            x = target_x;
        } else if self.hover_position.get() == HoverPosition::Right {
            x = target.right;
        } else if self.hover_position.get() == HoverPosition::Left {
            x = target.left - hover_width;
        } else {
            if self.anchor_alignment == AnchorAlignment::Right {
                x = target.right - hover_width;
            } else if self.hover_pointer.is_some() {
                x = target.center_x - client_width / 2.0;
            } else {
                x = target.left;
            }
            // A dica passa do limite direito da janela
            let document_client_width = window_size(&self.ui).0;
            if x + hover_width >= document_client_width {
                self.ui.tree_mut().add_class(self.hover.container_dom_node(), "right-aligned");
                x = (document_client_width - hover_width - HOVER_WINDOW_EDGE_MARGIN).max(document_client_left);
            }
        }
        // A dica passa do limite esquerdo da janela
        if x < document_client_left {
            x = target.left + HOVER_WINDOW_EDGE_MARGIN;
        }
        self.x.set(x);
    }

    fn compute_y_coordinate(&self, target: &TargetRect) {
        let (_, client_height) = self.client_size();
        let mut y;
        if let Some(target_y) = self.target.y() {
            y = target_y;
        } else if self.hover_position.get() == HoverPosition::Above {
            y = target.top;
        } else if self.hover_position.get() == HoverPosition::Below {
            y = target.bottom - 2.0;
        } else if self.hover_pointer.is_some() {
            y = target.center_y + client_height / 2.0;
        } else {
            y = target.bottom;
        }
        // A dica embaixo passa da janela
        if y > window_size(&self.ui).1 {
            y = target.bottom;
        }
        self.y.set(y);
    }

    fn adjust_horizontal_hover_position(&self, target: &TargetRect) {
        // Não ajusta na horizontal se a coordenada x foi dada
        if self.target.x().is_some() {
            return;
        }
        let (client_width, _) = self.client_size();
        let position = self.hover_position.get();
        let window_width = window_size(&self.ui).0;

        if self.anchor_alignment == AnchorAlignment::Right && matches!(position, HoverPosition::Above | HoverPosition::Below) {
            let available_width = target.right - 0.0 - HOVER_WINDOW_EDGE_MARGIN - HOVER_BORDER_WIDTH;
            if client_width > available_width {
                self.max_width_override.set(Some(available_width.max(0.0)));
                self.apply_box();
            }
            return;
        }

        let hover_pointer_offset = if self.hover_pointer.is_some() { POINTER_SIZE } else { 0.0 };

        // Com a posição forçada, restringe a largura máxima
        if self.force_position {
            let padding = hover_pointer_offset + HOVER_BORDER_WIDTH;
            match position {
                HoverPosition::Right => self.max_width_override.set(Some(window_width - target.right - padding)),
                HoverPosition::Left => self.max_width_override.set(Some(target.left - padding)),
                _ => return,
            }
            self.apply_box();
            return;
        }

        // Dica à direita do alvo
        if position == HoverPosition::Right {
            let room_on_right = window_width - target.right;
            // A dica da direita passa da janela
            if room_on_right < client_width + hover_pointer_offset {
                let room_on_left = target.left;
                // Há espaço à esquerda: inverte a posição
                if room_on_left >= client_width + hover_pointer_offset {
                    self.hover_position.set(HoverPosition::Left);
                } else {
                    // A da esquerda também passaria da janela
                    self.hover_position.set(HoverPosition::Below);
                }
            }
        }
        // Dica à esquerda do alvo
        else if position == HoverPosition::Left {
            let room_on_left = target.left;
            // A dica da esquerda passa da janela
            if room_on_left < client_width + hover_pointer_offset {
                let room_on_right = window_width - target.right;
                // Há espaço à direita: inverte a posição
                if room_on_right >= client_width + hover_pointer_offset {
                    self.hover_position.set(HoverPosition::Right);
                } else {
                    // A da direita também passaria da janela
                    self.hover_position.set(HoverPosition::Below);
                }
            }
            // A dica da esquerda passa da janela
            if target.left - client_width - hover_pointer_offset <= 0.0 {
                self.hover_position.set(HoverPosition::Right);
            }
        }
    }

    fn adjust_vertical_hover_position(&self, target: &TargetRect) {
        // Não ajusta na vertical se a coordenada y foi dada ou a posição é forçada
        if self.target.y().is_some() || self.force_position {
            return;
        }
        let (_, client_height) = self.client_size();
        let offset_height = client_height + HOVER_BORDER_WIDTH;
        let hover_pointer_offset = if self.hover_pointer.is_some() { POINTER_SIZE } else { 0.0 };
        let window_height = window_size(&self.ui).1;
        match self.hover_position.get() {
            // Dica acima do alvo: passa do topo da janela
            HoverPosition::Above if target.top - client_height - hover_pointer_offset < 0.0 => self.hover_position.set(HoverPosition::Below),
            // Dica abaixo do alvo: passa do fim da janela
            HoverPosition::Below if target.bottom + offset_height + hover_pointer_offset > window_height => self.hover_position.set(HoverPosition::Above),
            _ => {}
        }
    }

    fn adjust_hover_max_height(&self, target: &TargetRect) {
        let window_height = window_size(&self.ui).1;
        let mut max_height = window_height * self.max_height_ratio_relative_to_window;
        // Com a posição forçada, restringe a altura máxima
        if self.force_position {
            let padding = (if self.hover_pointer.is_some() { POINTER_SIZE } else { 0.0 }) + HOVER_BORDER_WIDTH;
            match self.hover_position.get() {
                HoverPosition::Above => max_height = max_height.min(target.top - padding),
                HoverPosition::Below => max_height = max_height.min(window_height - target.bottom - padding),
                _ => {}
            }
        }
        self.max_height.set(Some(max_height));
        self.apply_box();
    }

    /// `setHoverPointerPosition(target)`: o V do ponteiro na borda que olha para o alvo.
    fn set_hover_pointer_position(&self, target: &TargetRect) {
        let Some(pointer) = self.hover_pointer else { return };
        let (box_width, box_height) = self.box_size.get();
        let (client_width, client_height) = self.client_size();
        let position = self.hover_position.get();
        // O centro do losango na borda da caixa que olha para o alvo, em coordenadas da caixa.
        let (cx, cy, outward) = match position {
            HoverPosition::Left | HoverPosition::Right => {
                // Se a dica é mais alta que o alvo, o ponteiro fica no centro do alvo; senão, no centro da dica.
                let top = if client_height > target.height { target.center_y - (self.y.get() - client_height) - POINTER_SIZE } else { client_height / 2.0 - POINTER_SIZE };
                (if position == HoverPosition::Left { box_width } else { 0.0 }, top + POINTER_SIZE, if position == HoverPosition::Left { Outward::Right } else { Outward::Left })
            }
            HoverPosition::Above | HoverPosition::Below => {
                // O ponteiro no centro da dica; se sair do alvo, no centro do alvo.
                let mut left = client_width / 2.0 - POINTER_SIZE;
                let pointer_x = self.x.get() + left;
                if pointer_x < target.left || pointer_x > target.right {
                    left = target.center_x - self.x.get() - POINTER_SIZE;
                }
                (left + POINTER_SIZE, if position == HoverPosition::Above { box_height } else { 0.0 }, if position == HoverPosition::Above { Outward::Down } else { Outward::Up })
            }
        };
        let (pad_l, pad_t, _, _) = self.container_padding.get();
        let class = match position {
            HoverPosition::Left => "right",
            HoverPosition::Right => "left",
            HoverPosition::Above => "bottom",
            HoverPosition::Below => "top",
        };
        let mut tree = self.ui.tree_mut();
        for c in ["top", "left", "right", "bottom"] {
            tree.remove_class(pointer, c);
        }
        tree.add_class(pointer, class);
        for child in tree.children(pointer).to_vec() {
            tree.remove(child);
        }
        let border = self.styles.border.or(self.styles.foreground).unwrap_or(Rgba::new(0x45, 0x45, 0x45, 255));
        let background = self.styles.background.unwrap_or(Rgba::new(0x25, 0x25, 0x26, 255));
        // O nó do ponteiro cobre o contêiner todo; o V é desenhado em coordenadas do contêiner (o contêiner começa no padding).
        tree.set_bounds(pointer, Rect::new(0.0, 0.0, pad_l + box_width + 16.0, pad_t + box_height + 16.0));
        let (ox, oy) = (pad_l + cx, pad_t + cy);
        let add = |tree: &mut je_ui::Tree, rect: Rect, color: Rgba| {
            let node = create_el(tree, pointer, Role::Generic, "workbench-hover-pointer-part");
            tree.set_bounds(node, rect);
            tree.style_mut(node).background = Some(color);
        };
        // A linha de 1px da borda da caixa, coberta entre os braços.
        add(&mut tree, outward.rect(ox, oy, -1.0, 0.0, 8.0), background);
        for k in 0..4 {
            let width = 8.0 - 2.0 * f64::from(k);
            add(&mut tree, outward.rect(ox, oy, f64::from(k), f64::from(k) + 1.0, width), border);
            if width > 2.0 {
                add(&mut tree, outward.rect(ox, oy, f64::from(k), f64::from(k) + 1.0, width - 2.0), background);
            }
        }
    }

    /// Os `HoverAction` da barra de status.
    pub fn actions(&self) -> Vec<Rc<HoverAction>> {
        self.actions.borrow().clone()
    }

    /// `HoverContextViewDelegate` desta dica.
    pub fn context_view_delegate(self: &Rc<Self>, focus: bool, stack_depth: i32) -> Rc<HoverContextViewDelegate> {
        Rc::new(HoverContextViewDelegate { hover: self.clone(), focus, layer: stack_depth })
    }
}

/// O sentido em que o V do ponteiro aponta, para mapear `(u, v)` (para fora e lateral) em retângulos.
#[derive(Clone, Copy)]
enum Outward {
    Up,
    Down,
    Left,
    Right,
}

impl Outward {
    /// O retângulo de `u0..u1` para fora a partir de `(cx, cy)`, de largura lateral `width` centrada.
    fn rect(self, cx: f64, cy: f64, u0: f64, u1: f64, width: f64) -> Rect {
        let half = width / 2.0;
        match self {
            Self::Up => Rect::new(cx - half, cy - u1, cx + half, cy - u0),
            Self::Down => Rect::new(cx - half, cy + u0, cx + half, cy + u1),
            Self::Left => Rect::new(cx - u1, cy - half, cx - u0, cy + half),
            Self::Right => Rect::new(cx + u0, cy - half, cx + u1, cy + half),
        }
    }
}

impl IHoverWidget for WorkbenchHoverWidget {
    fn is_disposed(&self) -> bool {
        self.is_disposed.get()
    }
}

impl IDisposable for WorkbenchHoverWidget {
    fn dispose(&self) {
        if self.is_disposed.replace(true) {
            return;
        }
        self.on_dispose.fire(());
        // `this._target.dispose?.()`
        self.target.dispose();
        self.ui.remove(self.hover_container);
        self.base.dispose();
    }
}

/// `HoverContextViewDelegate`: liga a dica a uma [`ContextView`]: a âncora é o `(x, y)` calculado, a posição
/// é a `anchor` da dica e a camada vale a profundidade da pilha (dicas aninhadas ficam por cima).
pub struct HoverContextViewDelegate {
    hover: Rc<WorkbenchHoverWidget>,
    focus: bool,
    layer: i32,
}

impl HoverContextViewDelegate {
    /// O delegado de uma dica: `focus` põe o foco nela ao mostrar e `layer` é a profundidade na pilha de dicas.
    pub fn new(hover: Rc<WorkbenchHoverWidget>, focus: bool, layer: i32) -> Rc<Self> {
        Rc::new(Self { hover, focus, layer })
    }
}

impl IContextViewDelegate for HoverContextViewDelegate {
    fn get_anchor(&self) -> Anchor {
        Anchor::Rect(je_base::layout::IAnchor { x: self.hover.x(), y: self.hover.y(), width: None, height: None })
    }

    fn render(&self, container: NodeId) -> Option<DisposableRef> {
        self.hover.render(container);
        // O tamanho da visão é a caixa da dica (o contêiner tem margem negativa igual ao padding do ponteiro).
        let (w, h) = self.hover.box_size.get();
        self.hover.ui.tree_mut().set_bounds(container, Rect::new(0.0, 0.0, w, h));
        if self.focus {
            self.hover.focus();
        }
        Some(self.hover.clone())
    }

    fn layout(&self) {
        self.hover.layout();
        let (w, h) = self.hover.box_size.get();
        let mut tree = self.hover.ui.tree_mut();
        if let Some(view) = tree.parent(self.hover.hover_container) {
            let origin = tree.bounds(view);
            tree.set_bounds(view, Rect::new(origin.x0, origin.y0, origin.x0 + w, origin.y0 + h));
        }
    }

    fn anchor_position(&self) -> Option<AnchorPosition> {
        Some(self.hover.anchor())
    }

    fn layer(&self) -> i32 {
        self.layer
    }
}

/// Um [`IHoverDelegate`] concreto: mostra a dica numa [`ContextView`] própria dentro de `container`, uma por
/// vez, e a esconde quando o mouse sai do alvo e dela. `delay` é o atraso até a dica aparecer (o dono do
/// alvo é quem o espera, como no original), `placement` onde ela se ancora.
pub struct DefaultHoverDelegate {
    ui: Ui,
    container: NodeId,
    styles: HoverStyles,
    delay: HoverDelay,
    placement: Option<HoverPlacement>,
    current: RefCell<Option<Rc<ShownHover>>>,
}

/// A dica na tela com a visão de contexto que a hospeda.
struct ShownHover {
    hover: Rc<WorkbenchHoverWidget>,
    context_view: Rc<ContextView>,
}

impl IHoverWidget for ShownHover {
    fn is_disposed(&self) -> bool {
        self.hover.is_disposed()
    }
}

impl IDisposable for ShownHover {
    fn dispose(&self) {
        self.hover.dispose();
        self.context_view.dispose();
    }
}

impl DefaultHoverDelegate {
    pub fn new(ui: &Ui, container: NodeId, styles: HoverStyles, delay: HoverDelay, placement: Option<HoverPlacement>) -> Rc<Self> {
        Rc::new(Self { ui: ui.clone(), container, styles, delay, placement, current: RefCell::new(None) })
    }
}

impl IHoverDelegate for DefaultHoverDelegate {
    fn show_hover(&self, options: IHoverDelegateOptions, focus: bool) -> Option<Rc<dyn IHoverWidget>> {
        // Só uma dica por vez
        if let Some(previous) = self.current.borrow_mut().take() {
            previous.dispose();
        }
        let mut hover_options = IHoverOptions::new(options.content.clone(), options.target.clone());
        hover_options.container = options.container;
        hover_options.actions = options.managed.actions.clone();
        hover_options.additional_classes = options.managed.additional_classes.clone();
        hover_options.link_handler = options.managed.link_handler.clone();
        hover_options.trap_focus = options.managed.trap_focus;
        hover_options.position = Some(crate::hover_api::IHoverPositionOptions { hover_position: options.hover_position, anchor_alignment: options.managed.anchor_alignment, ..Default::default() });
        hover_options.appearance = Some(crate::hover_api::IHoverAppearanceOptions { show_pointer: options.show_pointer, show_hover_hint: options.managed.show_hover_hint, skip_fade_in_animation: options.skip_fade_in_animation, ..Default::default() });
        let hover = WorkbenchHoverWidget::new(&self.ui, hover_options, self.styles, None);
        let context_view = ContextView::new(&self.ui, options.container.unwrap_or(self.container), ContextViewDOMPosition::Absolute);
        context_view.show(hover.context_view_delegate(focus, 1));
        let shown = Rc::new(ShownHover { hover, context_view });
        // Quando a dica some (mouse fora, Escape, ação), a visão some junto.
        let weak = Rc::downgrade(&shown);
        shown.hover.base.register(shown.hover.on_dispose().listen(move |_| {
            if let Some(shown) = weak.upgrade() {
                shown.context_view.hide();
            }
        }));
        *self.current.borrow_mut() = Some(shown.clone());
        Some(shown)
    }

    fn delay(&self) -> HoverDelay {
        self.delay.clone()
    }

    fn placement(&self) -> Option<HoverPlacement> {
        self.placement
    }
}

impl IDisposable for DefaultHoverDelegate {
    fn dispose(&self) {
        if let Some(shown) = self.current.borrow_mut().take() {
            shown.dispose();
        }
    }
}
