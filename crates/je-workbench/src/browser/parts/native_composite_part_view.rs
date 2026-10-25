//! A vista nativa das partes de composites: a implementação, sobre a árvore de widgets (`je-ui`), da
//! [`ICompositePartView`] (`compositePart.ts`) e da [`IPaneCompositePartView`] (`paneCompositePart.ts`), com o CSS de
//! `part.css`, `compositepart.css` e `paneCompositePart.css` que posiciona o título, o cabeçalho, o conteúdo e o rodapé.
//!
//! Divergências:
//! - **Elemento e nó.** Cada [`ViewElement`] que a vista cria carrega o `NodeId` do nó dele ([`node_element`]); o pai, as classes e o
//!   estilo que a parte lê e escreve ficam no `ViewElement`, e `append_child`, `prepend_child` e `remove_child` movem também o nó.
//!   As classes que o CSS usa para escolher uma regra (`has-composite-bar`, `empty`, `sidebar`, `panel`, `auxiliarybar`) são lidas
//!   do `ViewElement` pelo layout.
//! - **Layout em vez de CSS.** `part.css` dá ao título 35px de altura, `padding: 0 8px` e três filhos em linha (`.title-label` com
//!   `padding-left: 12px`, `.title-actions` com `padding-left: 8px` e `.global-actions`); `compositepart.css` e as folhas de cada
//!   parte decidem quem cresce. A distribuição é [`flex_layout`] (base, crescimento, encolhimento e máximo de cada filho), com
//!   os valores de cada regra: com a barra de composites no título o rótulo some, a barra cresce e as ações têm a largura natural
//!   (`flex: inherit`); sem ela as ações crescem (`flex: 1`) e o rótulo da barra auxiliar fica em `flex: 0 0 auto` com
//!   `max-width: 40%`. O `padding-left` das ações é o deslocamento do nó.
//! - **Título.** O `h2` do rótulo é o texto de `11px` da fonte da UI; `text-transform: uppercase` das barras laterais é a
//!   conversão do texto exibido (o `textContent` guardado é o original), e `text-overflow: ellipsis` é o [`ellipsize`] à largura
//!   do nó. O `clientWidth` do `h2` é a largura do nó e o `scrollWidth` é a largura do texto inteiro.
//! - **Itens de ação.** O `createActionViewItem` do original não existe: a `ToolBar` de `je-widgets` desenha cada ação.
//! - **`window.devicePixelRatio`** é a leitura `device_pixel_ratio` que quem monta a vista entrega.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::actions::ActionRef;
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_platform::actions::{IMenuService, MenuId};
use je_platform::commands::ICommandService;
use je_platform::contextkey::IContextKeyService;
use je_platform::contextview::context_view::{AnchorTarget, IContextMenuService};
use je_platform::contextview::view_element::ViewElement;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::telemetry::ITelemetryService;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::color::Color;
use je_ui::{Content, EventKind, NodeId, Role, Ui};
use je_widgets::action_bar::ActionsOrientation;
use je_widgets::action_view_items::{IActionViewItem, IActionViewItemOptions};
use je_widgets::dom::{create_detached_el, create_el};
use je_widgets::hover_api::{HoverPlacement, IManagedHover, ManagedHoverContentOrFactory, get_base_layer_hover_delegate, get_default_hover_delegate};
use je_widgets::progress_bar::ProgressBar;
use je_widgets::text::{WORKBENCH_LINE_HEIGHT, ellipsize, measure_text, ui_font};
use je_widgets::toolbar::IToolBarOptions;

use crate::browser::parts::composite_part::{
    CompositeToolBarArgs, ICompositePartView, ICompositeProgressBar, ICompositeToolBar, ITitleLabelView, IToolBarItems,
};
use crate::browser::parts::pane_composite_bar::ContentElement;
use crate::browser::parts::pane_composite_part::{GlobalToolBarArgs, IPaneCompositePartView, PaneCompositeLayout};
use crate::platform::actions::toolbar::{
    HiddenItemStrategy, IMenuWorkbenchToolBarOptions, IWorkbenchToolBarOptions, MenuWorkbenchToolBar, WorkbenchToolBar,
};
use crate::services::contextview::native_context_view_service::{native_node, node_element};
use crate::services::layout::layout_service::LayoutContainerId;

/// O `padding` horizontal do `.title` (`part.css`).
const TITLE_PADDING: f64 = 8.0;
/// O `padding-left` do `.title-label` (`part.css`).
const TITLE_LABEL_PADDING_LEFT: f64 = 12.0;
/// O `padding-left` do `.title-actions` (`compositepart.css`).
const TITLE_ACTIONS_PADDING_LEFT: f64 = 8.0;
/// O `padding` horizontal do `.header-or-footer` (`paneCompositePart.css`).
const HEADER_FOOTER_PADDING: f64 = 4.0;
/// O `top` do `.monaco-progress-container` da área de conteúdo (`part.css`).
const PROGRESS_TOP: f64 = 33.0;
/// O tamanho da fonte do `h2` do título (`part.css`).
const TITLE_FONT_SIZE: f32 = 11.0;
/// O tamanho da fonte da área de conteúdo (`part.css`).
const CONTENT_FONT_SIZE: f32 = 13.0;
/// O `margin` da mensagem de painel vazio (`paneCompositePart.css`).
const EMPTY_PANE_MESSAGE_MARGIN: f64 = 12.0;

/// Um filho de uma linha `display: flex`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlexItem {
    /// `flex-basis` mais os preenchimentos que o item não deixa encolher.
    pub basis: f64,
    /// `flex-grow`.
    pub grow: f64,
    /// `flex-shrink`.
    pub shrink: f64,
    /// `min-width`.
    pub min: f64,
    /// `max-width`.
    pub max: f64,
}

/// A largura que cada filho de uma linha `display: flex` de largura `available` ocupa: o espaço que sobra vai para os que crescem
/// (proporcional ao `flex-grow`), o que falta sai dos que encolhem (proporcional ao `flex-shrink` vezes a base), e os limites
/// `min` e `max` valem sobre o resultado (o espaço que um limite devolve é redistribuído, como no algoritmo do CSS).
pub fn flex_layout(items: &[FlexItem], available: f64) -> Vec<f64> {
    let mut sizes: Vec<f64> = items.iter().map(|item| item.basis.clamp(item.min, item.max.max(item.min))).collect();
    let mut frozen: Vec<bool> = items.iter().map(|item| item.grow == 0.0 && item.shrink == 0.0).collect();

    for _ in 0..items.len() + 1 {
        let free = available - sizes.iter().sum::<f64>();
        if free.abs() < 1e-9 {
            break;
        }
        let growing = free > 0.0;
        let total: f64 = items
            .iter()
            .zip(&frozen)
            .map(|(item, frozen)| if *frozen { 0.0 } else if growing { item.grow } else { item.shrink * item.basis })
            .sum();
        if total <= 0.0 {
            break;
        }

        let mut violated = false;
        for (index, item) in items.iter().enumerate() {
            if frozen[index] {
                continue;
            }
            let weight = if growing { item.grow } else { item.shrink * item.basis };
            let target = sizes[index] + free * weight / total;
            let clamped = target.clamp(item.min, item.max.max(item.min));
            if (clamped - target).abs() > 1e-9 {
                violated = true;
                frozen[index] = true;
            }
            sizes[index] = clamped;
        }
        if !violated {
            break;
        }
    }

    sizes
}

/// O texto do `h2` do título e a vista dele.
struct TitleLabel {
    node: NodeId,
    text: String,
    uppercase: bool,
    color: Option<Rgba>,
    default_color: Rc<dyn Fn() -> Rgba>,
    max_width: f64,
}

/// O `ITitleLabelView` do `h2` do rótulo.
struct NativeTitleLabelView {
    ui: Ui,
    label: Rc<RefCell<TitleLabel>>,
    element: Rc<ViewElement>,
    hover: Rc<dyn IManagedHover>,
}

impl NativeTitleLabelView {
    fn render(ui: &Ui, label: &TitleLabel) {
        let displayed = if label.uppercase { label.text.to_uppercase() } else { label.text.clone() };
        let style = ui_font(TITLE_FONT_SIZE, 400);
        let shown = if label.max_width.is_finite() { ellipsize(&displayed, &style, label.max_width) } else { displayed };
        let color = label.color.unwrap_or_else(|| (label.default_color)());
        ui.tree_mut().style_mut(label.node).content = Content::Text { text: shown, style, color, line_height: 35.0 };
    }
}

impl ITitleLabelView for NativeTitleLabelView {
    fn set_text(&self, text: &str) {
        {
            let mut label = self.label.borrow_mut();
            label.text = text.to_owned();
        }
        let label = self.label.borrow();
        Self::render(&self.ui, &label);
    }

    fn update_hover(&self, content: &str) {
        self.hover.update(ManagedHoverContentOrFactory::from(content), None);
    }

    fn set_color(&self, color: Option<&Color>) {
        self.label.borrow_mut().color = color.map(je_theme::to_rgba8);
        let label = self.label.borrow();
        Self::render(&self.ui, &label);
    }
}

impl IDisposable for NativeTitleLabelView {
    fn dispose(&self) {
        self.hover.dispose();
        self.element.set_parent(None);
    }
}

/// As leituras que a vista precisa de quem a monta.
#[derive(Clone)]
pub struct NativeCompositePartViewServices {
    pub menu_service: Rc<dyn IMenuService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub context_menu_service: Rc<dyn IContextMenuService>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub command_service: Rc<dyn ICommandService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    /// O nó do contêiner do workbench (`layoutService.getContainer(window)`).
    pub container_node: Rc<dyn Fn(LayoutContainerId) -> Option<NodeId>>,
    /// `window.devicePixelRatio`.
    pub device_pixel_ratio: Rc<dyn Fn() -> f64>,
    /// A cor do texto herdada (`color` do contêiner da parte).
    pub foreground: Rc<dyn Fn() -> Rgba>,
}

/// A vista nativa de uma parte de composites.
pub struct NativeCompositePartView {
    ui: Ui,
    services: NativeCompositePartViewServices,
    uppercase_title: bool,
    title_label: RefCell<Option<Rc<RefCell<TitleLabel>>>>,
    tool_bar: RefCell<Option<Rc<dyn IToolBarItems>>>,
    global_tool_bar: RefCell<Option<Rc<dyn IToolBarItems>>>,
}

impl NativeCompositePartView {
    /// `uppercase_title` é o `text-transform: uppercase` do `h2` (a barra lateral e a barra auxiliar).
    pub fn new(ui: &Ui, services: NativeCompositePartViewServices, uppercase_title: bool) -> Rc<Self> {
        Rc::new(Self { ui: ui.clone(), services, uppercase_title, title_label: RefCell::new(None), tool_bar: RefCell::new(None), global_tool_bar: RefCell::new(None) })
    }

    /// O texto do título como o `textContent` do `h2` o guarda (sem a caixa alta do CSS).
    pub fn title_text(&self) -> String {
        self.title_label.borrow().as_ref().map(|label| label.borrow().text.clone()).unwrap_or_default()
    }

    /// O nó do `h2` do título.
    pub fn title_node(&self) -> Option<NodeId> {
        self.title_label.borrow().as_ref().map(|label| label.borrow().node)
    }

    /// A barra de ferramentas global (`.global-actions`), depois do `createTitleArea`.
    pub fn global_tool_bar(&self) -> Option<Rc<dyn IToolBarItems>> {
        self.global_tool_bar.borrow().clone()
    }

    fn node_of(element: &Rc<ViewElement>) -> Option<NodeId> {
        native_node(element)
    }

    fn place(tree: &mut je_ui::Tree, node: NodeId, x: f64, y: f64, width: f64, height: f64) {
        tree.set_bounds(node, Rect::new(x, y, x + width.max(0.0), y + height.max(0.0)));
    }

    /// Os filhos do nó que têm a classe, na ordem do DOM.
    fn child_with_class(tree: &je_ui::Tree, parent: NodeId, class_name: &str) -> Option<NodeId> {
        tree.children_with_class(parent, class_name).first().copied()
    }

    fn part_has_class(layout: &PaneCompositeLayout, class_name: &str) -> bool {
        layout.part_element.class_list_contains(class_name)
    }

    /// O layout do `.title`: a linha `display: flex` de `part.css`.
    fn layout_title(&self, layout: &PaneCompositeLayout, title: &Rc<ViewElement>, width: f64, height: f64, actions_width: f64, global_width: f64) {
        let Some(title_node) = Self::node_of(title) else { return };
        let has_composite_bar = title.class_list_contains("has-composite-bar");
        let auxiliary = Self::part_has_class(layout, "auxiliarybar");
        let inner = (width - TITLE_PADDING * 2.0).max(0.0);

        let (bar, label, actions, global) = {
            let tree = self.ui.tree();
            (
                Self::child_with_class(&tree, title_node, "composite-bar-container"),
                Self::child_with_class(&tree, title_node, "title-label"),
                Self::child_with_class(&tree, title_node, "title-actions"),
                Self::child_with_class(&tree, title_node, "global-actions"),
            )
        };

        let label_text_width = self
            .title_label
            .borrow()
            .as_ref()
            .map(|label| {
                let label = label.borrow();
                let shown = if label.uppercase { label.text.to_uppercase() } else { label.text.clone() };
                measure_text(&shown, &ui_font(TITLE_FONT_SIZE, 400))
            })
            .unwrap_or(0.0);

        // Os filhos presentes, na ordem do DOM, com as regras de `flex` de cada um.
        let mut present: Vec<(NodeId, FlexItem, &'static str)> = Vec::new();
        if let Some(bar) = bar.filter(|_| has_composite_bar) {
            present.push((bar, FlexItem { basis: 0.0, grow: 1.0, shrink: 1.0, min: 0.0, max: f64::INFINITY }, "bar"));
        }
        if let Some(label) = label {
            if !has_composite_bar {
                let basis = label_text_width + TITLE_LABEL_PADDING_LEFT;
                let item = if auxiliary {
                    FlexItem { basis, grow: 0.0, shrink: 0.0, min: 0.0, max: inner * 0.4 }
                } else {
                    FlexItem { basis, grow: 0.0, shrink: 1.0, min: 0.0, max: f64::INFINITY }
                };
                present.push((label, item, "label"));
            }
        }
        if let Some(actions) = actions {
            let natural = actions_width + TITLE_ACTIONS_PADDING_LEFT;
            let item = if has_composite_bar {
                FlexItem { basis: natural, grow: 0.0, shrink: 1.0, min: TITLE_ACTIONS_PADDING_LEFT, max: f64::INFINITY }
            } else {
                FlexItem { basis: TITLE_ACTIONS_PADDING_LEFT, grow: 1.0, shrink: 1.0, min: TITLE_ACTIONS_PADDING_LEFT, max: f64::INFINITY }
            };
            present.push((actions, item, "actions"));
        }
        if let Some(global) = global {
            present.push((global, FlexItem { basis: global_width, grow: 0.0, shrink: 0.0, min: global_width, max: global_width }, "global"));
        }

        let items: Vec<FlexItem> = present.iter().map(|(_, item, _)| *item).collect();
        let sizes = flex_layout(&items, inner);

        let mut tree = self.ui.tree_mut();
        tree.style_mut(title_node).visible = true;
        if let Some(label) = label {
            tree.style_mut(label).visible = !has_composite_bar;
        }
        let mut x = TITLE_PADDING;
        for ((node, _, kind), size) in present.iter().zip(sizes.iter().copied()) {
            match *kind {
                // O `padding-left` das ações é o deslocamento do nó.
                "actions" => Self::place(&mut tree, *node, x + TITLE_ACTIONS_PADDING_LEFT, 0.0, size - TITLE_ACTIONS_PADDING_LEFT, height),
                _ => Self::place(&mut tree, *node, x, 0.0, size, height),
            }
            x += size;
        }

        // O `h2` ocupa o rótulo depois do `padding-left`; o texto é cortado à largura do nó.
        let label_size = present.iter().position(|(node, _, kind)| *kind == "label" && Some(*node) == label).map(|index| sizes[index]);
        if let (Some(label_size), Some(title_label)) = (label_size, self.title_label.borrow().clone()) {
            let h2_width = (label_size - TITLE_LABEL_PADDING_LEFT).max(0.0);
            let node = {
                let mut label = title_label.borrow_mut();
                label.max_width = h2_width;
                label.node
            };
            Self::place(&mut tree, node, TITLE_LABEL_PADDING_LEFT, 0.0, h2_width, height);
            drop(tree);
            NativeTitleLabelView::render(&self.ui, &title_label.borrow());
        }
    }

    /// O layout do `.header-or-footer`: o contêiner da barra de composites ocupa a área menos o preenchimento.
    fn layout_header_or_footer(&self, area: &Rc<ViewElement>, y: f64, width: f64, height: f64) {
        let Some(node) = Self::node_of(area) else { return };
        let mut tree = self.ui.tree_mut();
        Self::place(&mut tree, node, 0.0, y, width, height);
        tree.style_mut(node).visible = true;
        if let Some(bar) = Self::child_with_class(&tree, node, "composite-bar-container") {
            Self::place(&mut tree, bar, HEADER_FOOTER_PADDING, 0.0, width - HEADER_FOOTER_PADDING * 2.0, height);
        }
    }

    /// O layout da área de conteúdo: os composites ocupam tudo (`height: 100%`) e a mensagem de painel vazio fica no centro.
    fn layout_content(&self, layout: &PaneCompositeLayout, content: &Rc<ViewElement>, y: f64) {
        let Some(node) = Self::node_of(content) else { return };
        let size = layout.result.content_size;
        let empty = Self::part_has_class(layout, "empty");

        let mut tree = self.ui.tree_mut();
        Self::place(&mut tree, node, 0.0, y, size.width, size.height);
        tree.style_mut(node).visible = true;

        for child in tree.children(node).to_vec() {
            if tree.has_class(child, "composite") {
                Self::place(&mut tree, child, 0.0, 0.0, size.width, size.height);
            } else if tree.has_class(child, "monaco-progress-container") {
                tree.set_bounds(child, Rect::new(0.0, PROGRESS_TOP, 0.0, PROGRESS_TOP + 2.0));
                tree.style_mut(child).z_index = 5;
            }
        }

        if let Some(area) = layout.empty_pane_message_area.as_ref().and_then(Self::node_of) {
            tree.style_mut(area).visible = empty;
            Self::place(&mut tree, area, 0.0, 0.0, size.width, size.height);
            if let Some(message) = Self::child_with_class(&tree, area, "empty-pane-message") {
                let style = ui_font(CONTENT_FONT_SIZE, 400);
                let text_width = match &tree.style(message).content {
                    Content::Text { text, .. } => measure_text(text, &style),
                    _ => 0.0,
                };
                let box_width = (text_width + EMPTY_PANE_MESSAGE_MARGIN * 2.0).min(size.width);
                let box_height = WORKBENCH_LINE_HEIGHT + EMPTY_PANE_MESSAGE_MARGIN * 2.0;
                Self::place(&mut tree, message, ((size.width - box_width) / 2.0).max(0.0), ((size.height - box_height) / 2.0).max(0.0), box_width, box_height);
            }
        }
    }
}

impl ICompositePartView for NativeCompositePartView {
    fn create_element(&self, parent: Option<&Rc<ViewElement>>, tag: &str, class_name: &str) -> Rc<ViewElement> {
        let role = if tag == "h2" { Role::Heading } else { Role::Generic };
        let node = {
            let mut tree = self.ui.tree_mut();
            match parent.and_then(Self::node_of) {
                Some(parent_node) => create_el(&mut tree, parent_node, role, class_name),
                None => create_detached_el(&mut tree, role, class_name),
            }
        };

        let element = node_element(node);
        element.set_class_name(class_name);
        if let Some(parent) = parent {
            element.set_parent(Some(parent.clone()));
        }
        element
    }

    fn prepend_child(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>) {
        child.set_parent(Some(parent.clone()));
        if let (Some(parent_node), Some(child_node)) = (Self::node_of(parent), Self::node_of(child)) {
            let mut tree = self.ui.tree_mut();
            let first = tree.children(parent_node).first().copied();
            tree.insert_before(parent_node, child_node, first);
        }
    }

    fn append_child(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>) {
        child.set_parent(Some(parent.clone()));
        if let (Some(parent_node), Some(child_node)) = (Self::node_of(parent), Self::node_of(child)) {
            self.ui.tree_mut().append_child(parent_node, child_node);
        }
    }

    fn remove_child(&self, child: &Rc<ViewElement>) {
        child.set_parent(None);
        if let Some(child_node) = Self::node_of(child) {
            self.ui.tree_mut().detach(child_node);
        }
    }

    fn create_title_label_view(&self, title_label: &Rc<ViewElement>) -> Rc<dyn ITitleLabelView> {
        let node = Self::node_of(title_label).expect("o h2 do título é um nó nativo");
        let label = Rc::new(RefCell::new(TitleLabel {
            node,
            text: String::new(),
            uppercase: self.uppercase_title,
            color: None,
            default_color: self.services.foreground.clone(),
            max_width: f64::INFINITY,
        }));

        // `clientWidth` e `scrollWidth` do `h2` (o refluxo do DOM).
        let (ui, width_label) = (self.ui.clone(), label.clone());
        title_label.set_client_width_source(Rc::new(move || ui.tree().bounds(node).width()));
        title_label.set_scroll_width_source(Rc::new(move || {
            let label = width_label.borrow();
            let shown = if label.uppercase { label.text.to_uppercase() } else { label.text.clone() };
            measure_text(&shown, &ui_font(TITLE_FONT_SIZE, 400))
        }));

        // `setupManagedHover(getDefaultHoverDelegate('mouse'), titleLabel, '')`
        let hover = get_base_layer_hover_delegate().setup_managed_hover(get_default_hover_delegate(HoverPlacement::Mouse), node, ManagedHoverContentOrFactory::from(""), None);

        *self.title_label.borrow_mut() = Some(label.clone());
        let view = Rc::new(NativeTitleLabelView { ui: self.ui.clone(), label, element: title_label.clone(), hover });
        NativeTitleLabelView::render(&view.ui, &view.label.borrow());
        view
    }

    fn create_progress_bar(&self, container: &Rc<ViewElement>) -> Rc<dyn ICompositeProgressBar> {
        let container_node = Self::node_of(container).expect("a área de conteúdo é um nó nativo");
        let progress_bar = ProgressBar::new(&self.ui, container_node, None);
        let element = progress_bar.get_container();
        {
            let mut tree = self.ui.tree_mut();
            tree.set_bounds(element, Rect::new(0.0, PROGRESS_TOP, 0.0, PROGRESS_TOP + 2.0));
            tree.style_mut(element).z_index = 5;
        }
        progress_bar
    }

    fn create_tool_bar(&self, args: CompositeToolBarArgs) -> Rc<dyn ICompositeToolBar> {
        let container = Self::node_of(&args.container).expect("o .title-actions é um nó nativo");
        let tool_bar = WorkbenchToolBar::new(
            &self.ui,
            container,
            Some(IWorkbenchToolBarOptions {
                tool_bar: IToolBarOptions {
                    orientation: Some(ActionsOrientation::Horizontal),
                    action_view_item_provider: Some(args.action_view_item_provider),
                    anchor_alignment_provider: Some(args.anchor_alignment_provider),
                    toggle_menu_title: Some(args.toggle_menu_title),
                    hover_delegate: Some(args.hover_delegate),
                    trailing_separator: args.trailing_separator,
                    responsive_behavior: args.responsive_behavior,
                    ..IToolBarOptions::default()
                },
                telemetry_source: Some(args.telemetry_source),
                ..IWorkbenchToolBarOptions::default()
            }),
            self.services.menu_service.clone(),
            self.services.context_key_service.clone(),
            self.services.context_menu_service.clone(),
            self.services.keybinding_service.clone(),
            self.services.command_service.clone(),
            self.services.telemetry_service.clone(),
        );

        let items: Rc<dyn IToolBarItems> = tool_bar.clone();
        *self.tool_bar.borrow_mut() = Some(items);
        tool_bar
    }

    fn create_action_view_item(&self, _action: &ActionRef, _options: &IActionViewItemOptions) -> Option<Rc<dyn IActionViewItem>> {
        None
    }
}

impl IPaneCompositePartView for NativeCompositePartView {
    fn set_text_content(&self, element: &Rc<ViewElement>, text: &str) {
        let Some(node) = Self::node_of(element) else { return };
        let color = (self.services.foreground)();
        self.ui.tree_mut().style_mut(node).content = Content::Text { text: text.to_owned(), style: ui_font(CONTENT_FONT_SIZE, 400), color, line_height: WORKBENCH_LINE_HEIGHT };
    }

    fn create_global_tool_bar(&self, args: GlobalToolBarArgs) -> Rc<dyn IToolBarItems> {
        let container = Self::node_of(&args.container).expect("o .global-actions é um nó nativo");
        let tool_bar = MenuWorkbenchToolBar::new(
            &self.ui,
            container,
            &args.menu_id,
            Some(IMenuWorkbenchToolBarOptions {
                base: IWorkbenchToolBarOptions {
                    tool_bar: IToolBarOptions {
                        orientation: Some(ActionsOrientation::Horizontal),
                        action_view_item_provider: Some(args.action_view_item_provider),
                        anchor_alignment_provider: Some(args.anchor_alignment_provider),
                        toggle_menu_title: Some(args.toggle_menu_title),
                        hover_delegate: Some(args.hover_delegate),
                        highlight_toggled_items: true,
                        ..IToolBarOptions::default()
                    },
                    hidden_item_strategy: Some(HiddenItemStrategy::NoHide),
                    telemetry_source: Some(args.telemetry_source),
                    ..IWorkbenchToolBarOptions::default()
                },
                ..IMenuWorkbenchToolBarOptions::default()
            }),
            self.services.menu_service.clone(),
            self.services.context_key_service.clone(),
            self.services.context_menu_service.clone(),
            self.services.keybinding_service.clone(),
            self.services.command_service.clone(),
            self.services.telemetry_service.clone(),
        );

        let items: Rc<dyn IToolBarItems> = tool_bar;
        *self.global_tool_bar.borrow_mut() = Some(items.clone());
        items
    }

    fn on_context_menu(&self, element: &Rc<ViewElement>, handler: Rc<dyn Fn(AnchorTarget)>) -> DisposableRef {
        let Some(node) = Self::node_of(element) else { return to_disposable(|| {}) };
        self.ui.listen(node, EventKind::ContextMenu, false, move |event| {
            let (posx, posy) = event.pointer.map_or((0.0, 0.0), |pointer| (pointer.x, pointer.y));
            handler(AnchorTarget::Mouse { posx, posy });
        })
    }

    fn schedule_at_next_animation_frame(&self, runner: Box<dyn FnOnce()>) -> DisposableRef {
        self.ui.schedule_at_next_animation_frame(0, runner)
    }

    fn toggle_container_class(&self, container: LayoutContainerId, class_name: &str, force: bool) {
        if let Some(node) = (self.services.container_node)(container) {
            self.ui.tree_mut().toggle_class(node, class_name, force);
        }
    }

    fn get_floating_border_width(&self) -> f64 {
        // `computeScreenAwareSize(window, 1)`
        let device_pixel_ratio = (self.services.device_pixel_ratio)();
        let screen_px = device_pixel_ratio * 1.0;
        screen_px.floor().max(1.0) / device_pixel_ratio
    }

    fn content_element_for(&self, element: &Rc<ViewElement>) -> Rc<ContentElement> {
        let content = ContentElement::default();
        if let Some(node) = Self::node_of(element) {
            content.set_node(node);
        }
        Rc::new(content)
    }

    fn layout_part(&self, layout: &PaneCompositeLayout) {
        let result = layout.result;

        // As larguras dos itens das barras de ferramentas são lidas antes de tomar a árvore emprestada para escrever.
        let actions_width = self.tool_bar.borrow().as_ref().map_or(0.0, |tool_bar| tool_bar.get_items_width());
        let global_width = self.global_tool_bar.borrow().as_ref().map_or(0.0, |tool_bar| tool_bar.get_items_width());

        let mut y = 0.0;
        if let Some(header) = &layout.header_area {
            self.layout_header_or_footer(header, y, layout.width, result.header_size.height);
            y += result.header_size.height;
        }
        if let Some(title) = &layout.title_area {
            if let Some(node) = Self::node_of(title) {
                Self::place(&mut self.ui.tree_mut(), node, 0.0, y, layout.width, result.title_size.height);
            }
            self.layout_title(layout, title, layout.width, result.title_size.height, actions_width, global_width);
            y += result.title_size.height;
        }
        if let Some(content) = &layout.content_area {
            self.layout_content(layout, content, y);
            y += result.content_size.height;
        }
        if let Some(footer) = &layout.footer_area {
            self.layout_header_or_footer(footer, y, layout.width, result.footer_size.height);
        }
    }
}
