//! Porte de `src/vs/workbench/browser/parts/editor/editorHeaderControl.ts`: o cabeçalho do grupo de editor, com as migalhas
//! (a [`BreadcrumbsControlFactory`]) e as três barras de ações dos menus do grupo (primária, secundária e de layout), e a regra
//! que decide se o cabeçalho aparece e quanta altura ele ocupa.
//!
//! Divergências de tradução:
//! - **DOM como nós da `Ui`.** `$('.a.b')` e `append` são `create_el` com as classes; `reset(container)` é o `clear_node`;
//!   `style.display = 'none'` é `style.visible = false` do nó (o `display: none` da `je-ui`); `classList.toggle('hidden', ...)` é
//!   a classe `hidden` junto com o `visible`, como o [`BreadcrumbsControl`] faz. A raiz do cabeçalho é criada antes do contêiner
//!   das ações e das migalhas, na ordem final de irmãos do original (migalhas e depois ações, que o original anexa por último).
//! - **`flex`, `gridTemplateColumns` e `marginLeft`.** São regras de layout do CSS que a árvore nativa não tem (o layout dos
//!   contêineres é por retângulo): o cabeçalho decide só o que o resto do porte lê, a visibilidade de cada contêiner e a altura.
//! - **`IInstantiationService` e o serviço escopado do painel.** O original cria as barras no `scopedInstantiationService` do painel
//!   de editor ativo (que tem as chaves de contexto do painel) ou no serviço do grupo; aqui os serviços chegam em
//!   [`EditorHeaderControlServices`] e as barras usam o serviço de chaves de contexto do grupo.
//! - **`MutableDisposable` das barras.** `headerActions` é um `DisposableStore` que se esvazia a cada `renderActions`, e cada barra
//!   entra nele ao ser criada (o original só atribui o `DisposableStore` no fim, o que é equivalente porque o evento de mudança
//!   dos itens do menu é assíncrono).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::nls::localize;
use je_platform::actions::{IMenuService, MenuActionOptions, MenuId};
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::telemetry::ITelemetryService;
use je_render::kurbo::Rect;
use je_ui::{NodeId, Role};
use je_widgets::dom::{clear_node, create_el};
use je_widgets::toolbar::IToolBarOptions;

use super::breadcrumbs_control::{BreadcrumbsControl, BreadcrumbsControlFactory, BreadcrumbsControlOptions, BreadcrumbsControlServices};
use super::editor::{IEditorGroupMenuIds, IEditorGroupView, IEditorGroupsView, ReserveHeaderSpace};
use crate::api::common::ext_host_values::ApiValue;
use crate::common::editor::{EditorInputOrUntyped, EditorResourceAccessor, EditorResourceAccessorOptions, SideBySideEditor};
use crate::platform::actions::{IMenuWorkbenchToolBarOptions, IToolBarRenderOptions, IWorkbenchToolBarOptions, MenuWorkbenchToolBar, PrimaryGroup};
use crate::services::editor::editor_groups_service::{ShowTabs, TabHeight};

/// Os serviços que o construtor do `EditorHeaderControl` recebe por injeção: os das migalhas (de onde vêm também a `Ui`, as
/// chaves de contexto, o menu de contexto e os comandos) e os das barras de ações.
#[derive(Clone)]
pub struct EditorHeaderControlServices {
    pub breadcrumbs: BreadcrumbsControlServices,
    pub menu_service: Rc<dyn IMenuService>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
}

type Toolbar = RefCell<Option<Rc<MenuWorkbenchToolBar>>>;

/// `EditorHeaderControl`.
pub struct EditorHeaderControl {
    this: Weak<EditorHeaderControl>,
    services: EditorHeaderControlServices,
    group_view: Rc<dyn IEditorGroupView>,
    groups_view: Rc<dyn IEditorGroupsView>,
    menu_ids: Option<IEditorGroupMenuIds>,
    reserve_header_space: Option<ReserveHeaderSpace>,

    header_container: Option<NodeId>,
    actions_container: Option<NodeId>,
    primary_actions_container: Option<NodeId>,
    secondary_actions_container: Option<NodeId>,
    layout_actions_container: Option<NodeId>,
    primary_actions_toolbar: Toolbar,
    secondary_actions_toolbar: Toolbar,
    layout_actions_toolbar: Toolbar,
    header_actions: Rc<DisposableStore>,
    breadcrumbs_container: Option<NodeId>,
    breadcrumbs_control_factory: RefCell<Option<Rc<BreadcrumbsControlFactory>>>,
    breadcrumbs_visible: Cell<bool>,
    visible: Cell<bool>,
    disposables: Rc<DisposableStore>,
}

impl EditorHeaderControl {
    pub const DEFAULT_HEIGHT: f64 = 32.0;
    pub const COMPACT_HEIGHT: f64 = 28.0;

    /// `new EditorHeaderControl(parent, groupView, groupsView, menuIds, showHeader, reserveHeaderSpace)`.
    pub fn new(
        parent: NodeId,
        group_view: Rc<dyn IEditorGroupView>,
        groups_view: Rc<dyn IEditorGroupsView>,
        menu_ids: Option<IEditorGroupMenuIds>,
        show_header: bool,
        reserve_header_space: Option<ReserveHeaderSpace>,
        services: EditorHeaderControlServices,
    ) -> Rc<Self> {
        let needs_breadcrumbs = show_header || groups_view.part_options().show_tabs != ShowTabs::Single;
        let (header_container, breadcrumbs_container, actions) = {
            let mut tree = services.breadcrumbs.ui.tree_mut();
            let mut breadcrumbs_parent = parent;
            let mut header_container = None;
            if show_header {
                let header = create_el(&mut tree, parent, Role::Generic, "editor-group-header breadcrumbs-in-header");
                breadcrumbs_parent = header;
                header_container = Some(header);
            }
            let breadcrumbs_container = needs_breadcrumbs.then(|| create_el(&mut tree, breadcrumbs_parent, Role::Generic, "breadcrumbs-below-tabs"));
            let actions = header_container.map(|header| {
                let actions_container = create_el(&mut tree, header, Role::Generic, "editor-group-header-actions");
                let primary = create_el(&mut tree, actions_container, Role::Generic, "editor-group-header-primary-actions");
                let secondary = create_el(&mut tree, actions_container, Role::Generic, "editor-group-header-secondary-actions");
                let layout = create_el(&mut tree, actions_container, Role::Generic, "editor-group-header-layout-actions");
                (actions_container, primary, secondary, layout)
            });
            (header_container, breadcrumbs_container, actions)
        };

        let disposables = DisposableStore::new();
        let header_actions = DisposableStore::new();
        disposables.add(header_actions.clone());
        if let Some(header) = header_container {
            let ui = services.breadcrumbs.ui.clone();
            disposables.add(to_disposable(move || ui.remove(header)));
        }

        let control = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            services,
            group_view,
            groups_view,
            menu_ids,
            reserve_header_space,
            header_container,
            actions_container: actions.map(|(actions_container, ..)| actions_container),
            primary_actions_container: actions.map(|(_, primary, ..)| primary),
            secondary_actions_container: actions.map(|(_, _, secondary, _)| secondary),
            layout_actions_container: actions.map(|(_, _, _, layout)| layout),
            primary_actions_toolbar: RefCell::new(None),
            secondary_actions_toolbar: RefCell::new(None),
            layout_actions_toolbar: RefCell::new(None),
            header_actions,
            breadcrumbs_container,
            breadcrumbs_control_factory: RefCell::new(None),
            breadcrumbs_visible: Cell::new(false),
            visible: Cell::new(false),
            disposables,
        });

        let weak = control.this.clone();
        if control.header_container.is_some() {
            let weak = weak.clone();
            control.disposables.add(control.group_view.on_did_active_editor_change().listen(move |_| {
                if let Some(control) = weak.upgrade() {
                    control.render_actions(true);
                }
            }));
        }

        if let Some(container) = control.breadcrumbs_container {
            let factory = BreadcrumbsControlFactory::new(
                container,
                control.group_view.clone(),
                BreadcrumbsControlOptions {
                    show_file_icons: true,
                    show_symbol_icons: true,
                    show_decoration_colors: false,
                    show_placeholder: true,
                    drag_editor: false,
                    widget_styles: None,
                    show_editor_type_picker: true,
                },
                control.services.breadcrumbs.clone(),
            );
            control.disposables.add(to_disposable({
                let factory = factory.clone();
                move || factory.dispose()
            }));
            control.disposables.add(factory.on_did_enablement_change().listen({
                let weak = weak.clone();
                move |_| {
                    if let Some(control) = weak.upgrade() {
                        control.update_breadcrumbs_visibility(true);
                    }
                }
            }));
            control.disposables.add(factory.on_did_visibility_change().listen({
                let weak = weak.clone();
                move |_| {
                    if let Some(control) = weak.upgrade() {
                        control.update_breadcrumbs_visibility(true);
                    }
                }
            }));
            *control.breadcrumbs_control_factory.borrow_mut() = Some(factory);
            control.update_breadcrumbs_visibility(false);
        }

        if control.header_container.is_some() {
            control.render_actions(false);
        }

        control
    }

    /// `breadcrumbsControl`: `this.breadcrumbsControlFactory?.control`.
    fn breadcrumbs_control(&self) -> Option<Rc<BreadcrumbsControl>> {
        let factory = self.breadcrumbs_control_factory.borrow().clone();
        factory.and_then(|factory| factory.control())
    }

    /// `height`.
    pub fn height(&self) -> f64 {
        if self.header_container.is_some() {
            if !self.visible.get() {
                return 0.0;
            }
            return if self.groups_view.part_options().tab_height == TabHeight::Compact { Self::COMPACT_HEIGHT } else { Self::DEFAULT_HEIGHT };
        }
        match self.breadcrumbs_control() {
            Some(control) if !control.is_hidden() => BreadcrumbsControl::HEIGHT,
            _ => 0.0,
        }
    }

    /// `handleEditorsChange(changed)`.
    pub fn handle_editors_change(&self, changed: bool) {
        let Some(control) = self.breadcrumbs_control() else { return };
        if changed {
            control.update();
        } else {
            control.reveal_last();
        }
    }

    /// `layout(width)`.
    pub fn layout(&self, width: f64) {
        self.update_visibility(false);
        if let Some(header) = self.header_container {
            self.set_width(header, width.max(0.0));
        }
        let control = self.breadcrumbs_control();
        if let (Some(control), Some(container)) = (control, self.breadcrumbs_container)
            && !control.is_hidden()
        {
            let breadcrumbs_width = if self.header_container.is_some() {
                self.services.breadcrumbs.ui.tree().bounds(control.dom_node()).width().max(0.0)
            } else {
                let breadcrumbs_width = width.max(0.0);
                self.set_width(container, breadcrumbs_width);
                breadcrumbs_width
            };
            control.layout(Some((breadcrumbs_width, BreadcrumbsControl::HEIGHT)));
        }
    }

    /// `element.style.width = `${width}px``.
    fn set_width(&self, node: NodeId, width: f64) {
        let mut tree = self.services.breadcrumbs.ui.tree_mut();
        let bounds = tree.bounds(node);
        tree.set_bounds(node, Rect::new(bounds.x0, bounds.y0, bounds.x0 + width, bounds.y1));
    }

    /// `updateBreadcrumbsVisibility(relayout)`.
    fn update_breadcrumbs_visibility(&self, relayout: bool) {
        let visible = self.breadcrumbs_control().is_some_and(|control| !control.is_hidden());
        self.breadcrumbs_visible.set(visible);
        if let Some(container) = self.breadcrumbs_container {
            let mut tree = self.services.breadcrumbs.ui.tree_mut();
            tree.toggle_class(container, "hidden", !visible);
            tree.style_mut(container).visible = visible;
        }
        self.update_visibility(relayout);
    }

    /// `updateVisibility(relayout)`.
    fn update_visibility(&self, relayout: bool) {
        let (Some(header), Some(actions), Some(primary), Some(secondary), Some(layout)) = (
            self.header_container,
            self.actions_container,
            self.primary_actions_container,
            self.secondary_actions_container,
            self.layout_actions_container,
        ) else {
            if relayout {
                self.group_view.relayout();
            }
            return;
        };

        let has_items = |toolbar: &Toolbar| toolbar.borrow().as_ref().map_or(0, |toolbar| toolbar.get_items_length()) > 0;
        let has_primary_actions = has_items(&self.primary_actions_toolbar);
        let has_secondary_actions = has_items(&self.secondary_actions_toolbar);
        let has_layout_actions = has_items(&self.layout_actions_toolbar);
        let has_menu_actions = has_primary_actions || has_secondary_actions || has_layout_actions;
        let reserved = self.reserve_header_space.as_ref().is_some_and(|reserve| reserve(self.group_view.active_editor().as_ref()));
        self.visible.set(self.breadcrumbs_visible.get() || has_menu_actions || reserved);
        {
            let mut tree = self.services.breadcrumbs.ui.tree_mut();
            tree.style_mut(primary).visible = has_primary_actions;
            tree.style_mut(secondary).visible = has_secondary_actions;
            tree.style_mut(layout).visible = has_layout_actions;
            tree.style_mut(actions).visible = has_menu_actions;
            tree.style_mut(header).visible = self.visible.get();
        }
        if relayout {
            self.group_view.relayout();
        }
    }

    /// `new MenuWorkbenchToolBar(container, menuId, createToolbarOptions(ariaLabel, trailingSeparator))`, guardada em
    /// `headerActions` com o ouvinte de `onDidChangeMenuItems` (`on_change`).
    fn create_toolbar(&self, container: NodeId, menu_id: &MenuId, aria_label: String, trailing_separator: bool, on_change: fn(&EditorHeaderControl)) -> Rc<MenuWorkbenchToolBar> {
        let services = &self.services;
        let active_editor = self.group_view.active_editor().map(EditorInputOrUntyped::Typed);
        let uri_options = EditorResourceAccessorOptions { support_side_by_side: Some(SideBySideEditor::Primary), filter_by_scheme: None };
        let arg = EditorResourceAccessor::get_original_uri(active_editor.as_ref(), Some(&uri_options)).map(|uri| uri.to_dynamic());
        let options = IMenuWorkbenchToolBarOptions {
            base: IWorkbenchToolBarOptions {
                tool_bar: IToolBarOptions { aria_label: Some(aria_label), trailing_separator, highlight_toggled_items: true, ..IToolBarOptions::default() },
                menu_options: Some(MenuActionOptions { arg, should_forward_args: true, ..MenuActionOptions::default() }),
                ..IWorkbenchToolBarOptions::default()
            },
            toolbar_options: Some(IToolBarRenderOptions {
                primary_group: Some(PrimaryGroup::Predicate(Rc::new(|group: &str| !group.starts_with("secondary/")))),
                use_separators_in_primary_actions: true,
            }),
            event_debounce_delay: None,
        };
        let toolbar = MenuWorkbenchToolBar::new(
            &services.breadcrumbs.ui,
            container,
            menu_id,
            Some(options),
            services.menu_service.clone(),
            services.breadcrumbs.context_key_service.clone(),
            services.breadcrumbs.context_menu_service.clone(),
            services.keybinding_service.clone(),
            services.breadcrumbs.command_service.clone(),
            services.telemetry_service.clone(),
        );
        self.header_actions.add_rc(toolbar.clone());
        let weak = self.this.clone();
        self.header_actions.add(toolbar.on_did_change_menu_items().listen(move |_| {
            if let Some(control) = weak.upgrade() {
                on_change(&control);
            }
        }));
        toolbar
    }

    /// `renderActions(relayout)`.
    fn render_actions(&self, relayout: bool) {
        let (Some(primary_container), Some(secondary_container), Some(layout_container)) =
            (self.primary_actions_container, self.secondary_actions_container, self.layout_actions_container)
        else {
            return;
        };
        self.header_actions.clear();
        *self.primary_actions_toolbar.borrow_mut() = None;
        *self.secondary_actions_toolbar.borrow_mut() = None;
        *self.layout_actions_toolbar.borrow_mut() = None;
        {
            let mut tree = self.services.breadcrumbs.ui.tree_mut();
            clear_node(&mut tree, primary_container);
            clear_node(&mut tree, secondary_container);
            clear_node(&mut tree, layout_container);
        }

        let header_primary_menu_id = self.menu_ids.as_ref().and_then(|ids| ids.header_primary.clone());
        let header_secondary_menu_id = self.menu_ids.as_ref().and_then(|ids| ids.header_secondary.clone());
        let header_layout_menu_id = self.menu_ids.as_ref().and_then(|ids| ids.header_layout.clone());
        if header_primary_menu_id.is_none() && header_secondary_menu_id.is_none() && header_layout_menu_id.is_none() {
            self.update_visibility(relayout);
            return;
        }

        if let Some(menu_id) = header_primary_menu_id {
            let aria_label = localize("ariaLabelEditorHeaderPrimaryActions", "Editor primary actions", &[]);
            let toolbar = self.create_toolbar(primary_container, &menu_id, aria_label, false, |control| control.update_visibility(true));
            *self.primary_actions_toolbar.borrow_mut() = Some(toolbar);
        }
        if let Some(menu_id) = header_layout_menu_id {
            let aria_label = localize("ariaLabelEditorHeaderLayoutActions", "Editor layout actions", &[]);
            let toolbar = self.create_toolbar(layout_container, &menu_id, aria_label, false, |control| control.render_actions(true));
            *self.layout_actions_toolbar.borrow_mut() = Some(toolbar);
        }
        if let Some(menu_id) = header_secondary_menu_id {
            let has_layout_actions = self.layout_actions_toolbar.borrow().as_ref().map_or(0, |toolbar| toolbar.get_items_length()) > 0;
            let aria_label = localize("ariaLabelEditorHeaderActions", "Editor actions", &[]);
            let toolbar = self.create_toolbar(secondary_container, &menu_id, aria_label, has_layout_actions, |control| control.update_visibility(true));
            *self.secondary_actions_toolbar.borrow_mut() = Some(toolbar);
        }
        self.update_visibility(relayout);
    }
}

impl IDisposable for EditorHeaderControl {
    fn dispose(&self) {
        self.disposables.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "EditorHeaderControl"
    }
}
