//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellStatusPart.ts`: `CellEditorStatusBar`, a barra de status sob o
//! editor da célula, com os itens contribuídos à esquerda e à direita (`CellStatusBarItem`), o clique que foca o editor e o modo de
//! foco da célula.
//!
//! Divergências:
//! - **DOM é a árvore do `je-ui`.** Os contêineres e cada item são nós; `tabIndex`, `aria-label` e `role` são atributos do nó,
//!   `classList.toggle` é `toggle_class`, `style.width` é a largura do `bounds` do contêiner, `style.opacity` e
//!   `style.backgroundColor` são os campos do `NodeStyle`. `style.color` não existe na árvore (a cor é da pintura do texto): ela vira
//!   a cor do estilo do [`SimpleIconLabel`] do item, e o rótulo é refeito quando o texto ou essa cor mudam.
//! - **`style.maxWidth`** é `max_width` do item: a largura do contêiner passa a ser a do conteúdo, no máximo `max_width`, com o
//!   recorte (`clip`) que o `overflow: hidden` dá. O posicionamento dos itens ao longo do contêiner (o `display: flex` do CSS) é do
//!   `cellRenderer.ts`.
//! - **Serviços injetados chegam como argumentos** ([`CellStatusBarServices`]). O `IThemeService` é o contrato estreito
//!   [`ICellStatusBarTheme`]: `onDidColorThemeChange` e a resolução de um `string | ThemeColor` para a cor (`resolveColor` do
//!   original, que devolve texto CSS; aqui a cor já resolvida, e `None` é o texto vazio). O `IHoverService` é o serviço de hover da
//!   camada de base (`get_base_layer_hover_delegate`), o mesmo que o `SimpleIconLabel` usa, e o `IConfigurationService` só lê
//!   `workbench.hover.delay`.
//! - **`INotebookEditorDelegate` e `ICodeEditor` são contratos estreitos** ([`ICellEditorStatusBarEditor`] e
//!   [`ICellStatusBarCodeEditor`]). `hasModel()` mais o estreitamento de tipo do contexto é `active_notebook_editor()`;
//!   `getTargetAtClientPoint(x, y)?.position` é `get_target_at_client_point`, que devolve só a posição.
//! - **O contexto da célula** (`INotebookCellActionContext & { $mid }`) é o [`INotebookCellActionContext`]; ao virar argumento de
//!   comando é [`INotebookActionContext::to_dynamic`](crate::contrib::notebook::browser::controller::INotebookActionContext::to_dynamic).
//! - **`e.event`** do [`IClickTarget`] é o [`PointerData`](je_ui::PointerData) do clique (coordenadas da janela, que são o
//!   `clientX` e o `clientY`). `e.target`, `parentElement` e `classList.contains` são o alvo do evento, o pai do nó e `has_class`.
//! - **`StandardKeyboardEvent.equals(KeyCode.Space)`** é a tecla sem modificadores.
//! - **`dispose()` sobrescrito** (que descarta as listas de itens) é um descarte registrado na parte.
//! - **`Date.now()`** do delegado de hover é `Instant`; `await executeCommand` com `try/catch` é o `then_result` que manda o erro
//!   para o `INotificationService`.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use je_base::dynamic::Dynamic;
use je_base::error_message::to_error_message_of;
use je_base::event::{Emitter, Event};
use je_base::icon_labels::strip_icons;
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_editor::position::Position;
use je_platform::commands::ICommandService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::notification::notification::{INotificationService, NotificationMessage};
use je_platform::telemetry::ITelemetryService;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Event as UiEvent, EventKind, NodeId, Role, Ui};
use je_widgets::dom::{StandardKeyboardEvent, create_el, is_ancestor_of_active_element};
use je_widgets::highlighted_label::HighlightedLabelStyles;
use je_widgets::hover_api::{
    HoverContent, HoverDelay, HoverPlacement, HoverPosition, IHoverAppearanceOptions, IHoverDelegate, IHoverDelegateOptions, IHoverOptions, IHoverPositionOptions, IHoverWidget, ManagedHoverContent,
    ManagedHoverContentOrFactory, get_base_layer_hover_delegate,
};
use je_widgets::simple_icon_label::SimpleIconLabel;

use crate::contrib::notebook::browser::controller::{INotebookCellActionContext, IActiveNotebookEditor};
use crate::contrib::notebook::browser::notebook_options::NotebookOptions;
use crate::contrib::notebook::browser::view::cell_part::{CellPart, CellPartState, common_layout_info};
use crate::contrib::notebook::browser::view::cell_parts::cell_widgets::{ClickTargetType, IClickTarget};
use crate::contrib::notebook::browser::view_model::cell_view_model_types::CellFocusMode;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_common::{CellStatusbarAlignment, INotebookCellStatusBarItem, StringOrCommand, StringOrMarkdown, StringOrThemeColor};

/// O que `CellEditorStatusBar` chama do `INotebookEditorDelegate`.
pub trait ICellEditorStatusBarEditor {
    /// `hasModel()` com o estreitamento para o editor ativo (ver as divergências do módulo).
    fn active_notebook_editor(&self) -> Option<Rc<dyn IActiveNotebookEditor>>;
    /// `notebookOptions`.
    fn notebook_options(&self) -> Rc<NotebookOptions>;
    /// `hasEditorFocus()`.
    fn has_editor_focus(&self) -> bool;
    /// `hasWebviewFocus()`.
    fn has_webview_focus(&self) -> bool;
    /// `getActiveCell()`.
    fn get_active_cell(&self) -> Option<CellViewModel>;
    /// `onDidChangeActiveCell`.
    fn on_did_change_active_cell(&self) -> Event<()>;
}

/// O que `CellEditorStatusBar` e `CellStatusBarItem` chamam do `ICodeEditor`.
pub trait ICellStatusBarCodeEditor {
    /// `hasWidgetFocus()`.
    fn has_widget_focus(&self) -> bool;
    /// `onDidFocusEditorWidget`.
    fn on_did_focus_editor_widget(&self) -> Event<()>;
    /// `onDidBlurEditorWidget`.
    fn on_did_blur_editor_widget(&self) -> Event<()>;
    /// `getTargetAtClientPoint(clientX, clientY)?.position`.
    fn get_target_at_client_point(&self, client_x: f64, client_y: f64) -> Option<Position>;
    /// `setPosition(position)`.
    fn set_position(&self, position: Position);
    /// `focus()`.
    fn focus(&self);
}

/// O que `CellStatusBarItem` chama do `IThemeService` (ver as divergências do módulo).
pub trait ICellStatusBarTheme {
    /// `onDidColorThemeChange`.
    fn on_did_color_theme_change(&self) -> Event<()>;
    /// `resolveColor(color)`: `None` é o texto vazio (sem cor).
    fn resolve_color(&self, color: &StringOrThemeColor) -> Option<Rgba>;
}

/// Os serviços que o `createInstance` do original injetava na barra e nos itens.
pub struct CellStatusBarServices {
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub command_service: Rc<dyn ICommandService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub theme: Rc<dyn ICellStatusBarTheme>,
    /// O estilo do texto de um item sem cor própria (a cor herdada e a fonte).
    pub label_styles: HighlightedLabelStyles,
}

/// O `IHoverDelegate` anônimo do construtor: mostra o hover instantâneo acima do item e some o atraso se um hover acabou de fechar.
struct CellStatusBarHoverDelegate {
    configuration_service: Rc<dyn IConfigurationService>,
    last_hover_hide_time: Cell<Option<Instant>>,
}

impl IHoverDelegate for CellStatusBarHoverDelegate {
    fn show_hover(&self, mut options: IHoverDelegateOptions, _focus: bool) -> Option<Rc<dyn IHoverWidget>> {
        options.hover_position = Some(HoverPosition::Above);

        let mut hover_options = IHoverOptions::new(options.content.clone(), options.target.clone());
        hover_options.container = options.container;
        hover_options.actions = options.managed.actions.clone();
        hover_options.additional_classes = options.managed.additional_classes.clone();
        hover_options.link_handler = options.managed.link_handler.clone();
        hover_options.trap_focus = options.managed.trap_focus;
        hover_options.on_did_show = options.managed.on_did_show.clone();
        hover_options.position = Some(IHoverPositionOptions { hover_position: options.hover_position, anchor_alignment: options.managed.anchor_alignment, ..IHoverPositionOptions::default() });
        hover_options.appearance = Some(IHoverAppearanceOptions {
            show_pointer: options.show_pointer,
            show_hover_hint: options.managed.show_hover_hint,
            skip_fade_in_animation: options.skip_fade_in_animation,
            ..IHoverAppearanceOptions::default()
        });
        get_base_layer_hover_delegate().show_instant_hover(hover_options, false)
    }

    fn placement(&self) -> Option<HoverPlacement> {
        Some(HoverPlacement::Element)
    }

    fn delay(&self) -> HoverDelay {
        let recently_hidden = self.last_hover_hide_time.get().is_some_and(|hidden_at| hidden_at.elapsed() < Duration::from_millis(200));
        if recently_hidden {
            // show instantly when a hover was recently shown
            return HoverDelay::Fixed(0.0);
        }

        match self.configuration_service.get_value(Some("workbench.hover.delay"), None) {
            Dynamic::Number(delay) => HoverDelay::Fixed(delay),
            _ => HoverDelay::Fixed(0.0),
        }
    }

    fn on_did_hide_hover(&self) {
        self.last_hover_hide_time.set(Some(Instant::now()));
    }
}

/// `CellEditorStatusBar`.
pub struct CellEditorStatusBar {
    state: CellPartState,
    me: Weak<CellEditorStatusBar>,
    ui: Ui,
    notebook_editor: Rc<dyn ICellEditorStatusBarEditor>,
    cell_container: NodeId,
    editor: Option<Rc<dyn ICellStatusBarCodeEditor>>,
    services: Rc<CellStatusBarServices>,
    pub status_bar_container: NodeId,
    left_items_container: NodeId,
    right_items_container: NodeId,
    items_disposable: Rc<DisposableStore>,
    left_items: Rc<RefCell<Vec<Rc<CellStatusBarItem>>>>,
    right_items: Rc<RefCell<Vec<Rc<CellStatusBarItem>>>>,
    width: Cell<f64>,
    current_context: RefCell<Option<INotebookCellActionContext>>,
    on_did_click: Rc<Emitter<IClickTarget>>,
    hover_delegate: Rc<dyn IHoverDelegate>,
}

impl CellEditorStatusBar {
    pub fn new(
        ui: &Ui,
        notebook_editor: Rc<dyn ICellEditorStatusBarEditor>,
        cell_container: NodeId,
        editor_part: NodeId,
        editor: Option<Rc<dyn ICellStatusBarCodeEditor>>,
        services: Rc<CellStatusBarServices>,
    ) -> Rc<Self> {
        let (status_bar_container, status_left_container, status_right_container, left_items_container, right_items_container) = {
            let mut tree = ui.tree_mut();
            let status_bar_container = create_el(&mut tree, editor_part, Role::Generic, "cell-statusbar-container");
            tree.set_attribute(status_bar_container, "tabIndex", "-1");
            let status_left_container = create_el(&mut tree, status_bar_container, Role::Generic, "cell-status-left");
            let status_right_container = create_el(&mut tree, status_bar_container, Role::Generic, "cell-status-right");
            let left_items_container = create_el(&mut tree, status_left_container, Role::Generic, "cell-contributed-items cell-contributed-items-left");
            let right_items_container = create_el(&mut tree, status_right_container, Role::Generic, "cell-contributed-items cell-contributed-items-right");
            (status_bar_container, status_left_container, status_right_container, left_items_container, right_items_container)
        };

        let state = CellPartState::new();
        let items_disposable = DisposableStore::new();
        state.register(items_disposable.clone());
        let on_did_click = Emitter::new();
        state.register(on_did_click.clone());

        let left_items: Rc<RefCell<Vec<Rc<CellStatusBarItem>>>> = Rc::new(RefCell::new(Vec::new()));
        let right_items: Rc<RefCell<Vec<Rc<CellStatusBarItem>>>> = Rc::new(RefCell::new(Vec::new()));
        state.register(to_disposable({
            let (left_items, right_items) = (left_items.clone(), right_items.clone());
            move || {
                for items in [&left_items, &right_items] {
                    let items = std::mem::take(&mut *items.borrow_mut());
                    for item in items {
                        item.dispose();
                    }
                }
            }
        }));

        let hover_delegate: Rc<dyn IHoverDelegate> = Rc::new(CellStatusBarHoverDelegate { configuration_service: services.configuration_service.clone(), last_hover_hide_time: Cell::new(None) });

        Rc::new_cyclic(|me: &Weak<Self>| {
            {
                let me = me.clone();
                state.register(services.theme.on_did_color_theme_change().listen(move |_| {
                    let Some(part) = me.upgrade() else { return };
                    let context = part.current_context.borrow().clone();
                    if let Some(context) = context {
                        part.update_context(context);
                    }
                }));
            }

            {
                let (me, ui) = (me.clone(), ui.clone());
                state.register(ui.clone().listen(status_bar_container, EventKind::Click, false, move |e: &mut UiEvent| {
                    let Some(part) = me.upgrade() else { return };
                    let Some(pointer) = e.pointer else { return };
                    let target = e.target;
                    let type_ = if target == status_left_container || target == status_right_container || target == status_bar_container {
                        // hit on empty space
                        ClickTargetType::Container
                    } else {
                        let tree = ui.tree();
                        let has_command = |node: NodeId| tree.has_class(node, "cell-status-item-has-command");
                        let item_has_command = has_command(target) || tree.parent(target).is_some_and(has_command);
                        if item_has_command { ClickTargetType::ContributedCommandItem } else { ClickTargetType::ContributedTextItem } // text
                    };
                    part.on_did_click.fire(IClickTarget { type_, event: pointer });
                }));
            }

            Self {
                state,
                me: me.clone(),
                ui: ui.clone(),
                notebook_editor,
                cell_container,
                editor,
                services,
                status_bar_container,
                left_items_container,
                right_items_container,
                items_disposable,
                left_items,
                right_items,
                width: Cell::new(0.0),
                current_context: RefCell::new(None),
                on_did_click,
                hover_delegate,
            }
        })
    }

    /// `onDidClick`.
    pub fn on_did_click(&self) -> Event<IClickTarget> {
        self.on_did_click.event()
    }

    fn get_max_item_width(&self) -> f64 {
        self.width.get() / 2.0
    }

    fn focus_is_inside_status_bar(&self) -> bool {
        is_ancestor_of_active_element(&self.ui, self.status_bar_container)
    }

    fn update_focus_mode_for_editor_event(&self, element: &CellViewModel, editor: &dyn ICellStatusBarCodeEditor) {
        if editor.has_widget_focus() || self.focus_is_inside_status_bar() {
            element.base().set_focus_mode(CellFocusMode::Editor);
        } else {
            let current_mode = element.base().focus_mode();
            if current_mode == CellFocusMode::ChatInput {
                element.base().set_focus_mode(CellFocusMode::ChatInput);
            } else if current_mode == CellFocusMode::Output && self.notebook_editor.has_webview_focus() {
                element.base().set_focus_mode(CellFocusMode::Output);
            } else {
                element.base().set_focus_mode(CellFocusMode::Container);
            }
        }
    }

    pub fn update_context(&self, context: INotebookCellActionContext) {
        *self.current_context.borrow_mut() = Some(context.clone());
        self.items_disposable.clear();

        let Some(cell) = context.cell.clone() else {
            return;
        };

        {
            let me = self.me.clone();
            let on_did_change_layout = match &cell {
                CellViewModel::Code(cell) => cell.on_did_change_layout(),
                CellViewModel::Markup(cell) => cell.on_did_change_layout(),
            };
            self.items_disposable.add(on_did_change_layout.listen(move |_| {
                let Some(part) = me.upgrade() else { return };
                let current_cell = part.current_context.borrow().as_ref().and_then(|context| context.cell.clone());
                if let Some(current_cell) = current_cell {
                    part.update_internal_layout_now(&current_cell);
                }
            }));
        }
        {
            let me = self.me.clone();
            self.items_disposable.add(cell.base().on_did_change_cell_status_bar_items().listen(move |_| {
                if let Some(part) = me.upgrade() {
                    part.update_rendered_items();
                }
            }));
        }
        {
            let me = self.me.clone();
            self.items_disposable.add(self.notebook_editor.on_did_change_active_cell().listen(move |_| {
                if let Some(part) = me.upgrade() {
                    part.update_active_cell();
                }
            }));
        }
        self.update_internal_layout_now(&cell);
        self.update_active_cell();
        self.update_rendered_items();
    }

    fn update_active_cell(&self) {
        let current_cell = self.current_context.borrow().as_ref().and_then(|context| context.cell.clone());
        let is_active_cell = match (self.notebook_editor.get_active_cell(), current_cell) {
            (Some(active), Some(current)) => active.ptr_eq(&current),
            (None, None) => true,
            _ => false,
        };
        self.ui.tree_mut().toggle_class(self.status_bar_container, "is-active-cell", is_active_cell);
    }

    fn update_rendered_items(&self) {
        let Some(context) = self.current_context.borrow().clone() else {
            return;
        };
        let Some(cell) = context.cell.clone() else {
            return;
        };
        let mut items = cell.base().get_cell_status_bar_items();
        items.sort_by(|item_a, item_b| item_b.priority.unwrap_or(0.0).partial_cmp(&item_a.priority.unwrap_or(0.0)).unwrap_or(Ordering::Equal));

        let max_item_width = self.get_max_item_width();
        let new_left_items: Vec<INotebookCellStatusBarItem> = items.iter().filter(|item| item.alignment == CellStatusbarAlignment::Left).cloned().collect();
        let mut new_right_items: Vec<INotebookCellStatusBarItem> = items.iter().filter(|item| item.alignment == CellStatusbarAlignment::Right).cloned().collect();
        new_right_items.reverse();

        self.update_items(&self.left_items, new_left_items, self.left_items_container, &context, max_item_width);
        self.update_items(&self.right_items, new_right_items, self.right_items_container, &context, max_item_width);
    }

    fn update_items(&self, rendered_items: &RefCell<Vec<Rc<CellStatusBarItem>>>, new_items: Vec<INotebookCellStatusBarItem>, container: NodeId, context: &INotebookCellActionContext, max_item_width: f64) {
        let deleted: Vec<Rc<CellStatusBarItem>> = {
            let mut rendered_items = rendered_items.borrow_mut();
            if rendered_items.len() > new_items.len() { rendered_items.split_off(new_items.len()) } else { Vec::new() }
        };
        for deleted_item in deleted {
            self.ui.tree_mut().remove(deleted_item.container);
            deleted_item.dispose();
        }

        for (i, new_item) in new_items.into_iter().enumerate() {
            let existing_item = rendered_items.borrow().get(i).cloned();
            match existing_item {
                Some(existing_item) => existing_item.update_item(new_item, Some(max_item_width)),
                None => {
                    let item = CellStatusBarItem::new(&self.ui, container, context.clone(), self.hover_delegate.clone(), self.editor.clone(), self.services.clone(), new_item, Some(max_item_width));
                    rendered_items.borrow_mut().push(item);
                }
            }
        }
    }
}

impl CellPart for CellEditorStatusBar {
    fn state(&self) -> &CellPartState {
        &self.state
    }

    fn did_render_cell(&self, element: &CellViewModel) {
        if let Some(notebook_editor) = self.notebook_editor.active_notebook_editor() {
            let context = INotebookCellActionContext::new(notebook_editor).with_cell(element.clone()).with_ui(true);
            self.update_context(context);
        }

        let Some(editor) = self.editor.clone() else {
            return;
        };

        // Focus Mode
        {
            let (me, element, editor) = (self.me.clone(), element.clone(), editor.clone());
            self.state.cell_disposables().add(editor.clone().on_did_focus_editor_widget().listen(move |_| {
                if let Some(part) = me.upgrade() {
                    part.update_focus_mode_for_editor_event(&element, &*editor);
                }
            }));
        }
        {
            let (me, element, editor) = (self.me.clone(), element.clone(), editor.clone());
            self.state.cell_disposables().add(editor.clone().on_did_blur_editor_widget().listen(move |_| {
                let Some(part) = me.upgrade() else { return };
                // this is for a special case:
                // users click the status bar empty space, which we will then focus the editor
                // so we don't want to update the focus state too eagerly, it will be updated with onDidFocusEditorWidget
                if part.notebook_editor.has_editor_focus() && !part.focus_is_inside_status_bar() {
                    part.update_focus_mode_for_editor_event(&element, &*editor);
                }
            }));
        }

        // Mouse click handlers
        {
            let me = self.me.clone();
            self.state.cell_disposables().add(self.on_did_click().listen(move |e: &IClickTarget| {
                let Some(part) = me.upgrade() else { return };
                let Some(current_cell) = part.state.current_cell() else { return };
                if current_cell.as_code().is_some() && e.type_ != ClickTargetType::ContributedCommandItem {
                    let statusbar_height = part.notebook_editor.notebook_options().compute_editor_statusbar_height(&current_cell.base().internal_metadata(), &current_cell.uri());
                    if let Some(position) = editor.get_target_at_client_point(e.event.x, e.event.y - statusbar_height) {
                        editor.set_position(position);
                        editor.focus();
                    }
                }
            }));
        }
    }

    fn update_internal_layout_now(&self, element: &CellViewModel) {
        // todo@rebornix layer breaker
        let statusbar_hidden = self.notebook_editor.notebook_options().compute_editor_statusbar_height(&element.base().internal_metadata(), &element.uri()) == 0.0;
        self.ui.tree_mut().toggle_class(self.cell_container, "cell-statusbar-hidden", statusbar_hidden);

        let width = common_layout_info(element).editor_width;
        if width == 0.0 || width.is_nan() {
            return;
        }

        self.width.set(width);
        {
            let mut tree = self.ui.tree_mut();
            let bounds = tree.bounds(self.status_bar_container);
            tree.set_bounds(self.status_bar_container, Rect::new(bounds.x0, bounds.y0, bounds.x0 + width, bounds.y1));
        }

        let max_item_width = self.get_max_item_width();
        for item in self.left_items.borrow().iter().chain(self.right_items.borrow().iter()) {
            item.set_max_width(max_item_width);
        }
    }
}

impl_cell_content_part!(CellEditorStatusBar, "CellEditorStatusBar");

/// `CellStatusBarItem`.
struct CellStatusBarItem {
    item_disposables: Rc<DisposableStore>,
    me: Weak<CellStatusBarItem>,
    ui: Ui,
    container: NodeId,
    context: INotebookCellActionContext,
    hover_delegate: Rc<dyn IHoverDelegate>,
    editor: Option<Rc<dyn ICellStatusBarCodeEditor>>,
    services: Rc<CellStatusBarServices>,
    max_width: Cell<Option<f64>>,
    foreground: Cell<Option<Rgba>>,
    current_item: RefCell<Option<INotebookCellStatusBarItem>>,
}

impl CellStatusBarItem {
    #[allow(clippy::too_many_arguments)]
    fn new(
        ui: &Ui,
        parent: NodeId,
        context: INotebookCellActionContext,
        hover_delegate: Rc<dyn IHoverDelegate>,
        editor: Option<Rc<dyn ICellStatusBarCodeEditor>>,
        services: Rc<CellStatusBarServices>,
        item_model: INotebookCellStatusBarItem,
        max_width: Option<f64>,
    ) -> Rc<Self> {
        let container = create_el(&mut ui.tree_mut(), parent, Role::Generic, "cell-status-item");
        let item = Rc::new_cyclic(|me: &Weak<Self>| Self {
            item_disposables: DisposableStore::new(),
            me: me.clone(),
            ui: ui.clone(),
            container,
            context,
            hover_delegate,
            editor,
            services,
            max_width: Cell::new(None),
            foreground: Cell::new(None),
            current_item: RefCell::new(None),
        });

        item.update_item(item_model, max_width);
        item
    }

    /// `set maxWidth(v)`.
    fn set_max_width(&self, max_width: f64) {
        self.max_width.set(Some(max_width));
        self.apply_size();
    }

    /// A largura do contêiner: a do conteúdo, no máximo `max_width` (o `max-width` com `overflow: hidden`).
    fn apply_size(&self) {
        let mut tree = self.ui.tree_mut();
        let content_width = tree.children(self.container).iter().map(|child| tree.bounds(*child).x1).fold(0.0, f64::max);
        let width = self.max_width.get().map_or(content_width, |max_width| content_width.min(max_width));
        let bounds = tree.bounds(self.container);
        tree.set_bounds(self.container, Rect::new(bounds.x0, bounds.y0, bounds.x0 + width, bounds.y1));
        tree.style_mut(self.container).clip = true;
    }

    fn update_item(&self, item: INotebookCellStatusBarItem, max_width: Option<f64>) {
        self.item_disposables.clear();

        let previous = self.current_item.borrow().clone();
        let foreground = item.color.as_ref().and_then(|color| self.services.theme.resolve_color(color));
        if previous.as_ref().is_none_or(|previous| previous.text != item.text) || foreground != self.foreground.get() {
            self.foreground.set(foreground);
            let mut styles = self.services.label_styles;
            if let Some(foreground) = foreground {
                styles.foreground = foreground;
            }
            let label = Rc::new(SimpleIconLabel::new(&self.ui, self.container, styles));
            label.set_text(&item.text.replace('\n', " "));
            self.item_disposables.add(label);
        }

        let background_color = item.background_color.as_ref().and_then(|color| self.services.theme.resolve_color(color));
        let opacity = item.opacity.as_deref().filter(|opacity| !opacity.is_empty()).and_then(|opacity| opacity.parse::<f32>().ok()).unwrap_or(1.0);
        {
            let mut tree = self.ui.tree_mut();
            let style = tree.style_mut(self.container);
            style.background = background_color;
            style.opacity = opacity;
            tree.toggle_class(self.container, "cell-status-item-show-when-active", item.only_show_when_active == Some(true));
        }

        if let Some(max_width) = max_width {
            self.max_width.set(Some(max_width));
        }
        self.apply_size();

        let (aria_label, role) = match &item.accessibility_information {
            Some(information) => (information.label.clone(), information.role.clone()),
            None if item.text.is_empty() => (String::new(), None),
            None => {
                let units: Vec<u16> = item.text.encode_utf16().collect();
                (strip_icons(&units).to_string_lossy().trim().to_owned(), None)
            }
        };

        {
            let mut tree = self.ui.tree_mut();
            tree.set_attribute(self.container, "aria-label", aria_label);
            tree.set_attribute(self.container, "role", role.unwrap_or_default());
        }

        let hover_content = match &item.tooltip {
            Some(StringOrMarkdown::String(tooltip)) if !tooltip.is_empty() => Some(ManagedHoverContent::Text(tooltip.clone())),
            Some(StringOrMarkdown::String(_)) | None => None,
            Some(StringOrMarkdown::Markdown(markdown)) => Some(ManagedHoverContent::Markdown { markdown: Some(HoverContent::Markdown(markdown.clone())), markdown_not_supported_fallback: None }),
        };
        if let Some(hover_content) = hover_content {
            let hover = get_base_layer_hover_delegate().setup_managed_hover(self.hover_delegate.clone(), self.container, ManagedHoverContentOrFactory::Content(hover_content), None);
            self.item_disposables.add(hover);
        }

        self.ui.tree_mut().toggle_class(self.container, "cell-status-item-has-command", item.command.is_some());
        if item.command.is_some() {
            self.ui.tree_mut().set_attribute(self.container, "tabIndex", "0");

            {
                let me = self.me.clone();
                self.item_disposables.add(self.ui.listen(self.container, EventKind::Click, false, move |_| {
                    if let Some(item) = me.upgrade() {
                        item.execute_command();
                    }
                }));
            }
            {
                let me = self.me.clone();
                self.item_disposables.add(self.ui.listen(self.container, EventKind::KeyDown, false, move |e: &mut UiEvent| {
                    let Some(item) = me.upgrade() else { return };
                    let is_activation_key = StandardKeyboardEvent::from_event(e)
                        .is_some_and(|event| !(event.ctrl_key || event.shift_key || event.alt_key || event.meta_key) && (event.key_code == KeyCode::Space || event.key_code == KeyCode::Enter));
                    if is_activation_key {
                        item.execute_command();
                    }
                }));
            }
        } else {
            self.ui.tree_mut().remove_attribute(self.container, "tabIndex");
        }

        *self.current_item.borrow_mut() = Some(item);
    }

    fn execute_command(&self) {
        let Some(command) = self.current_item.borrow().as_ref().and_then(|item| item.command.clone()) else {
            return;
        };

        let (id, mut args) = match command {
            StringOrCommand::String(id) => (id, Vec::new()),
            StringOrCommand::Command(command) => (command.id, command.arguments.unwrap_or_default()),
        };

        if args.is_empty() {
            args.push(self.context.to_dynamic(None));
        }

        self.services
            .telemetry_service
            .public_log2("workbenchActionExecuted", Some(Dynamic::object([("id", Dynamic::from(id.as_str())), ("from", Dynamic::from("cell status bar"))])));
        if let Some(editor) = &self.editor {
            editor.focus();
        }
        let notification_service = self.services.notification_service.clone();
        let _ = self.services.command_service.execute_command(&id, args).then_result(move |result| {
            if let Err(error) = result {
                notification_service.error(vec![NotificationMessage::Text(to_error_message_of(&error, false))]);
            }
            Ok(())
        });
    }
}

impl IDisposable for CellStatusBarItem {
    fn dispose(&self) {
        self.item_disposables.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "CellStatusBarItem"
    }
}
