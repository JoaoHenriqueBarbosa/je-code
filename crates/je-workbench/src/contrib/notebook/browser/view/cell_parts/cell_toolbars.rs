//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellToolbars.ts`: `BetweenCellToolbar` (a barra de inserção
//! entre as células) e `CellTitleToolbarPart` (a barra de título da célula, com a barra de exclusão ao lado). As duas são partes de
//! sobreposição ([`CellOverlayPart`](crate::contrib::notebook::browser::view::cell_part::CellOverlayPart)).
//!
//! Divergências:
//! - **Serviços injetados chegam como argumentos.** `IInstantiationService`, `IContextMenuService`, `IContextKeyService`,
//!   `IMenuService` e `IKeybindingService` são o [`CellToolbarServices`]; o `WorkbenchToolBar` recebe também o `ICommandService` e o
//!   `ITelemetryService` que o `createInstance` injetava nele.
//! - **`INotebookEditorDelegate` são contratos estreitos por parte** ([`IBetweenCellToolbarEditor`] e [`ICellTitleToolbarEditor`]).
//!   `hasModel()` mais o estreitamento de tipo que o original faz antes de montar o contexto é `active_notebook_editor()`: `Some`
//!   quando o editor tem modelo, e o `Rc<dyn IActiveNotebookEditor>` é o `notebookEditor` do contexto.
//! - **O contexto da barra** (`{ ui, cell, notebookEditor, source, $mid }`) é [`INotebookActionContext::to_dynamic`].
//! - **`createActionViewItem` e `MenuEntryActionViewItem`.** Não existem numa UI de nós nativos (ver `MenuWorkbenchToolBar`): o
//!   `ToolBar` de `je-widgets` desenha cada ação, então o `actionViewItemProvider` que cai no `MenuEntryActionViewItem` (alinhamento
//!   diferente de `center`) não é passado. O `CodiconActionViewItem` (`cellActionView.ts`) é construído direto, só quando o
//!   alinhamento é `center`; nos demais casos o provedor devolve `None` e a barra desenha a ação.
//! - **`style.transform = translateY(...)`** do contêiner inferior é o [`IndicatorNode`] (o mesmo substituto de `FastDomNode` de
//!   `cellFocusIndicator.ts`), e o `style.top` do contêiner da barra de título é o [`StickyTopNode`].
//! - **`_titleToolbarContainer`** do construtor do `BetweenCellToolbar` não é usado pelo original e não é parâmetro aqui.
//! - **`ICssClassDelegate.toggle(className, force?)`** perdeu o `?`: toda chamada da parte passa o `force`.
//! - **`CellTitleToolbarPart` implementa o contrato** [`ICellTitleToolbar`] de `cellFocusIndicator.ts` (`onDidUpdateActions` e
//!   `hasActions`), que é a forma como o `CellFocusIndicator` a enxerga. O modelo (`CellTitleToolbarModel.actions`) guarda as ações
//!   do momento da criação e não é atualizado pelas mudanças do menu, como no original.
//! - **`disposableTimeout(..., this._store)`** usa o `timeouts` da parte, registrado nela.
//! - **`getCellToolbarActions`**: `/^inline/.test(g)` é `starts_with("inline")`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::ActionRef;
use je_base::async_::disposable_timeout;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::DisposableStore;
use je_platform::actions::menu_entry_action_view_item::{PrimaryAndSecondaryActions, get_action_bar_actions};
use je_platform::actions::{IMenu, IMenuService, MenuActionOptions, MenuId, MenuItemAction};
use je_platform::commands::ICommandService;
use je_platform::contextkey::IContextKeyService;
use je_platform::contextview::context_view::IContextMenuService;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::telemetry::ITelemetryService;
use je_ui::{NodeId, Ui};
use je_widgets::action_bar::IActionViewItemProvider;
use je_widgets::action_view_items::{IActionViewItem, IActionViewItemOptions};
use je_widgets::dom::is_ancestor_of_active_element;
use je_widgets::hover_api::{IHoverDelegate, create_instant_hover_delegate};
use je_widgets::toolbar::{IToolBarOptions, ToolBar};

use crate::contrib::notebook::browser::controller::{IActiveNotebookEditor, INotebookCellActionContext};
use crate::contrib::notebook::browser::notebook_options::NotebookOptions;
use crate::contrib::notebook::browser::view::cell_part::{CellPart, CellPartState, common_layout_info};
use crate::contrib::notebook::browser::view::cell_parts::cell_action_view::CodiconActionViewItem;
use crate::contrib::notebook::browser::view::cell_parts::cell_focus_indicator::{ICellTitleToolbar, IndicatorNode};
use crate::contrib::notebook::browser::view::cell_parts::cell_toolbar_sticky_scroll::{ICellToolbarStickyScrollEditor, StickyScrollOptions, StickyTopNode, register_cell_toolbar_sticky_scroll};
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::platform::actions::menu_entry_action_view_item::{IMenuEntryActionViewItemOptions, MenuEntryServices};
use crate::platform::actions::toolbar::{IWorkbenchToolBarOptions, WorkbenchToolBar};
use crate::platform::contextview::ContextMenuServiceProvider;

/// Os serviços que as duas partes recebem do `createInstance` do original.
#[derive(Clone)]
pub struct CellToolbarServices {
    pub menu_service: Rc<dyn IMenuService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub context_menu_service: Rc<dyn IContextMenuService>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub command_service: Rc<dyn ICommandService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
}

/// `getCellToolbarActions(menu)`.
pub(crate) fn get_cell_toolbar_actions(menu: &dyn IMenu) -> PrimaryAndSecondaryActions {
    let options = MenuActionOptions { should_forward_args: true, ..MenuActionOptions::default() };
    get_action_bar_actions(&menu.get_actions(Some(&options)), &|group: &str| group.starts_with("inline"), false)
}

/// O que `BetweenCellToolbar` chama do `INotebookEditorDelegate`.
pub trait IBetweenCellToolbarEditor {
    /// `hasModel()` com o estreitamento para o editor ativo (ver as divergências do módulo).
    fn active_notebook_editor(&self) -> Option<Rc<dyn IActiveNotebookEditor>>;
    /// `notebookOptions`.
    fn notebook_options(&self) -> Rc<NotebookOptions>;
    /// `creationOptions.menuIds.cellInsertToolbar`.
    fn cell_insert_toolbar_menu_id(&self) -> MenuId;
}

/// `BetweenCellToolbar`.
pub struct BetweenCellToolbar {
    state: CellPartState,
    ui: Ui,
    notebook_editor: Rc<dyn IBetweenCellToolbarEditor>,
    bottom_cell_toolbar_container: Rc<IndicatorNode>,
    services: CellToolbarServices,
    between_cell_toolbar: RefCell<Option<Rc<ToolBar>>>,
}

impl BetweenCellToolbar {
    pub fn new(ui: &Ui, notebook_editor: Rc<dyn IBetweenCellToolbarEditor>, bottom_cell_toolbar_container: Rc<IndicatorNode>, services: CellToolbarServices) -> Rc<Self> {
        Rc::new(Self { state: CellPartState::new(), ui: ui.clone(), notebook_editor, bottom_cell_toolbar_container, services, between_cell_toolbar: RefCell::new(None) })
    }

    fn initialize(&self) -> Rc<ToolBar> {
        if let Some(between_cell_toolbar) = self.between_cell_toolbar.borrow().clone() {
            return between_cell_toolbar;
        }

        let action_view_item_provider: IActionViewItemProvider = {
            let notebook_editor = self.notebook_editor.clone();
            let ui = self.ui.clone();
            let entry_services = MenuEntryServices { keybinding_service: self.services.keybinding_service.clone(), context_key_service: self.services.context_key_service.clone() };
            Rc::new(move |action: &ActionRef, options: &IActionViewItemOptions| {
                if action.as_any().is::<MenuItemAction>() && notebook_editor.notebook_options().get_display_options().insert_toolbar_alignment == "center" {
                    let item: Rc<dyn IActionViewItem> = CodiconActionViewItem::new(
                        &ui,
                        action.clone(),
                        Some(IMenuEntryActionViewItemOptions { hover_delegate: options.hover_delegate.clone(), ..IMenuEntryActionViewItemOptions::default() }),
                        entry_services.clone(),
                    );
                    return Some(item);
                }

                None
            })
        };
        let between_cell_toolbar = ToolBar::new(
            &self.ui,
            self.bottom_cell_toolbar_container.node(),
            ContextMenuServiceProvider::new(self.services.context_menu_service.clone()),
            IToolBarOptions { action_view_item_provider: Some(action_view_item_provider), ..IToolBarOptions::default() },
        );
        self.state.register(between_cell_toolbar.clone());

        *self.between_cell_toolbar.borrow_mut() = Some(between_cell_toolbar.clone());
        let menu = self.services.menu_service.create_menu(&self.notebook_editor.cell_insert_toolbar_menu_id(), self.services.context_key_service.clone(), None);
        self.state.register(menu.clone());
        let update_actions: Rc<dyn Fn()> = {
            let (menu, between_cell_toolbar) = (menu.clone(), between_cell_toolbar.clone());
            Rc::new(move || {
                let actions = get_cell_toolbar_actions(&*menu);
                between_cell_toolbar.set_actions(&actions.primary, &actions.secondary);
            })
        };

        {
            let update_actions = update_actions.clone();
            self.state.register(menu.on_did_change().listen(move |_| update_actions()));
        }
        {
            let update_actions = update_actions.clone();
            self.state.register(self.notebook_editor.notebook_options().on_did_change_options().listen(move |e| {
                if e.insert_toolbar_alignment {
                    update_actions();
                }
            }));
        }

        update_actions();

        between_cell_toolbar
    }
}

impl CellPart for BetweenCellToolbar {
    fn state(&self) -> &CellPartState {
        &self.state
    }

    fn did_render_cell(&self, element: &CellViewModel) {
        let between_cell_toolbar = self.initialize();
        if let Some(notebook_editor) = self.notebook_editor.active_notebook_editor() {
            let context: INotebookCellActionContext = INotebookCellActionContext::new(notebook_editor).with_cell(element.clone()).with_ui(true);
            between_cell_toolbar.set_context(Some(context.to_dynamic(Some("insertToolbar"))));
        }
        self.update_internal_layout_now(element);
    }

    fn update_internal_layout_now(&self, element: &CellViewModel) {
        let bottom_toolbar_offset = common_layout_info(element).bottom_toolbar_offset;
        self.bottom_cell_toolbar_container.set_translate_y(bottom_toolbar_offset);
    }
}

impl_cell_overlay_part!(BetweenCellToolbar, "BetweenCellToolbar");

/// `ICssClassDelegate`.
pub trait ICssClassDelegate {
    fn toggle(&self, class_name: &str, force: bool);
}

/// O que `CellTitleToolbarPart` chama do `INotebookEditorDelegate` (e do `INotebookEditor` que o `registerCellToolbarStickyScroll`
/// recebe).
pub trait ICellTitleToolbarEditor: ICellToolbarStickyScrollEditor {
    /// `hasModel()` com o estreitamento para o editor ativo (ver as divergências do módulo).
    fn active_notebook_editor(&self) -> Option<Rc<dyn IActiveNotebookEditor>>;
    /// `focus()`.
    fn focus(&self);
}

/// `CellTitleToolbarModel`.
struct CellTitleToolbarModel {
    title_menu: Rc<dyn IMenu>,
    actions: PrimaryAndSecondaryActions,
    delete_menu: Rc<dyn IMenu>,
    delete_actions: PrimaryAndSecondaryActions,
}

/// A barra de título (um `WorkbenchToolBar`) ou a de exclusão (um `ToolBar`): as duas são o `ToolBar` do original.
enum TitleToolBar {
    Workbench(Rc<WorkbenchToolBar>),
    Plain(Rc<ToolBar>),
}

impl TitleToolBar {
    fn tool_bar(&self) -> &ToolBar {
        match self {
            TitleToolBar::Workbench(toolbar) => toolbar.tool_bar(),
            TitleToolBar::Plain(toolbar) => toolbar,
        }
    }

    /// `setActions(primary, secondary)`: a `WorkbenchToolBar` esconde itens do menu, então a barra do workbench passa por ela.
    fn set_actions(&self, primary: &[ActionRef], secondary: &[ActionRef]) {
        match self {
            TitleToolBar::Workbench(toolbar) => toolbar.set_actions(primary, secondary, None),
            TitleToolBar::Plain(toolbar) => toolbar.set_actions(primary, secondary),
        }
    }
}

/// `CellTitleToolbarView`.
struct CellTitleToolbarView {
    toolbar: Rc<TitleToolBar>,
    delete_toolbar: Rc<TitleToolBar>,
}

/// `CellTitleToolbarPart`.
pub struct CellTitleToolbarPart {
    state: CellPartState,
    me: Weak<CellTitleToolbarPart>,
    ui: Ui,
    toolbar_container: Rc<StickyTopNode>,
    root_class_delegate: Rc<dyn ICssClassDelegate>,
    toolbar_id: MenuId,
    delete_toolbar_id: MenuId,
    notebook_editor: Rc<dyn ICellTitleToolbarEditor>,
    services: CellToolbarServices,
    timeouts: Rc<DisposableStore>,
    model: RefCell<Option<Rc<CellTitleToolbarModel>>>,
    view: RefCell<Option<Rc<CellTitleToolbarView>>>,
    on_did_update_actions: Rc<Emitter<()>>,
}

impl CellTitleToolbarPart {
    pub fn new(
        ui: &Ui,
        toolbar_container: NodeId,
        root_class_delegate: Rc<dyn ICssClassDelegate>,
        toolbar_id: MenuId,
        delete_toolbar_id: MenuId,
        notebook_editor: Rc<dyn ICellTitleToolbarEditor>,
        services: CellToolbarServices,
    ) -> Rc<Self> {
        let state = CellPartState::new();
        let on_did_update_actions = Emitter::new();
        state.register(on_did_update_actions.clone());
        let timeouts = DisposableStore::new();
        state.register(timeouts.clone());
        Rc::new_cyclic(|me: &Weak<Self>| Self {
            state,
            me: me.clone(),
            ui: ui.clone(),
            toolbar_container: StickyTopNode::new(ui, toolbar_container),
            root_class_delegate,
            toolbar_id,
            delete_toolbar_id,
            notebook_editor,
            services,
            timeouts,
            model: RefCell::new(None),
            view: RefCell::new(None),
            on_did_update_actions,
        })
    }

    fn initialize_model(&self) -> Rc<CellTitleToolbarModel> {
        if let Some(model) = self.model.borrow().clone() {
            return model;
        }

        let title_menu = self.services.menu_service.create_menu(&self.toolbar_id, self.services.context_key_service.clone(), None);
        self.state.register(title_menu.clone());
        let delete_menu = self.services.menu_service.create_menu(&self.delete_toolbar_id, self.services.context_key_service.clone(), None);
        self.state.register(delete_menu.clone());
        let actions = get_cell_toolbar_actions(&*title_menu);
        let delete_actions = get_cell_toolbar_actions(&*delete_menu);

        let model = Rc::new(CellTitleToolbarModel { title_menu, actions, delete_menu, delete_actions });
        *self.model.borrow_mut() = Some(model.clone());

        model
    }

    /// `createDeleteToolbar(accessor, container, hoverDelegate, 'cell-delete-toolbar')`.
    fn create_delete_toolbar(&self, hover_delegate: Rc<dyn IHoverDelegate>) -> Rc<ToolBar> {
        let keybinding_service = self.services.keybinding_service.clone();
        let toolbar = ToolBar::new(
            &self.ui,
            self.toolbar_container.node(),
            ContextMenuServiceProvider::new(self.services.context_menu_service.clone()),
            IToolBarOptions {
                get_key_binding: Some(Rc::new(move |action: &ActionRef| keybinding_service.lookup_keybinding(&action.id(), None, false))),
                hover_delegate: Some(hover_delegate),
                ..IToolBarOptions::default()
            },
        );

        self.ui.tree_mut().add_class(toolbar.get_element(), "cell-delete-toolbar");

        toolbar
    }

    fn initialize(&self, model: &CellTitleToolbarModel) -> Rc<CellTitleToolbarView> {
        if let Some(view) = self.view.borrow().clone() {
            return view;
        }
        let instant_hover_delegate = create_instant_hover_delegate();
        self.state.register(instant_hover_delegate.clone());
        let hover_delegate: Rc<dyn IHoverDelegate> = instant_hover_delegate;
        let toolbar = WorkbenchToolBar::new(
            &self.ui,
            self.toolbar_container.node(),
            Some(IWorkbenchToolBarOptions { tool_bar: IToolBarOptions { hover_delegate: Some(hover_delegate.clone()), ..IToolBarOptions::default() }, ..IWorkbenchToolBarOptions::default() }),
            self.services.menu_service.clone(),
            self.services.context_key_service.clone(),
            self.services.context_menu_service.clone(),
            self.services.keybinding_service.clone(),
            self.services.command_service.clone(),
            self.services.telemetry_service.clone(),
        );
        self.state.register(toolbar.clone());

        let delete_toolbar = self.create_delete_toolbar(hover_delegate);
        self.state.register(delete_toolbar.clone());
        if !model.delete_actions.primary.is_empty() || !model.delete_actions.secondary.is_empty() {
            delete_toolbar.set_actions(&model.delete_actions.primary, &model.delete_actions.secondary);
        }

        let view = Rc::new(CellTitleToolbarView { toolbar: Rc::new(TitleToolBar::Workbench(toolbar)), delete_toolbar: Rc::new(TitleToolBar::Plain(delete_toolbar)) });
        self.setup_change_listeners(&view.toolbar, &model.title_menu, &model.actions);
        self.setup_change_listeners(&view.delete_toolbar, &model.delete_menu, &model.delete_actions);
        *self.view.borrow_mut() = Some(view.clone());

        view
    }

    fn setup_change_listeners(&self, toolbar: &Rc<TitleToolBar>, menu: &Rc<dyn IMenu>, init_actions: &PrimaryAndSecondaryActions) {
        // #103926
        let dropdown_is_visible = Rc::new(Cell::new(false));
        let deferred_update: Rc<RefCell<Option<Box<dyn FnOnce()>>>> = Rc::new(RefCell::new(None));

        self.update_actions(toolbar, init_actions);
        {
            let (me, menu, toolbar, dropdown_is_visible, deferred_update) = (self.me.clone(), menu.clone(), toolbar.clone(), dropdown_is_visible.clone(), deferred_update.clone());
            let on_did_change = menu.on_did_change();
            self.state.register(on_did_change.listen(move |_| {
                let Some(part) = me.upgrade() else { return };
                let actions = get_cell_toolbar_actions(&*menu);
                if dropdown_is_visible.get() {
                    let (me, toolbar) = (me.clone(), toolbar.clone());
                    *deferred_update.borrow_mut() = Some(Box::new(move || {
                        if let Some(part) = me.upgrade() {
                            part.update_actions(&toolbar, &actions);
                        }
                    }));
                    return;
                }

                part.update_actions(&toolbar, &actions);
            }));
        }
        self.root_class_delegate.toggle("cell-toolbar-dropdown-active", false);
        {
            let me = self.me.clone();
            self.state.register(toolbar.tool_bar().on_did_change_dropdown_visibility().listen(move |visible: &bool| {
                let Some(part) = me.upgrade() else { return };
                dropdown_is_visible.set(*visible);
                part.root_class_delegate.toggle("cell-toolbar-dropdown-active", *visible);

                if deferred_update.borrow().is_some() && !*visible {
                    // O original lê `deferredUpdate` só quando o temporizador dispara, depois de já tê-lo zerado na linha seguinte: a
                    // atualização adiada não chega a rodar (ela só é refeita na próxima mudança do menu). O porte mantém essa ordem.
                    let pending = deferred_update.clone();
                    disposable_timeout(
                        move || {
                            let deferred = pending.borrow_mut().take();
                            if let Some(deferred) = deferred {
                                deferred();
                            }
                        },
                        0,
                        Some(part.timeouts.clone()),
                    );

                    *deferred_update.borrow_mut() = None;
                }
            }));
        }
    }

    fn update_actions(&self, toolbar: &TitleToolBar, actions: &PrimaryAndSecondaryActions) {
        let had_focus = is_ancestor_of_active_element(&self.ui, toolbar.tool_bar().get_element());
        toolbar.set_actions(&actions.primary, &actions.secondary);
        if had_focus {
            self.notebook_editor.focus();
        }

        let has_actions = !actions.primary.is_empty() || !actions.secondary.is_empty();
        self.root_class_delegate.toggle("cell-has-toolbar-actions", has_actions);
        self.on_did_update_actions.fire(());
    }
}

impl CellPart for CellTitleToolbarPart {
    fn state(&self) -> &CellPartState {
        &self.state
    }

    fn prepare_render_cell(&self, _element: &CellViewModel) {
        self.initialize_model();
    }

    fn did_render_cell(&self, element: &CellViewModel) {
        let model = self.initialize_model();
        let view = self.initialize(&model);
        let sticky_scroll_editor: Rc<dyn ICellToolbarStickyScrollEditor> = self.notebook_editor.clone();
        self.state.cell_disposables().add(register_cell_toolbar_sticky_scroll(
            sticky_scroll_editor,
            element.clone(),
            self.toolbar_container.clone(),
            Some(StickyScrollOptions { extra_offset: Some(4.0), min: Some(-14.0) }),
        ));

        if let Some(notebook_editor) = self.notebook_editor.active_notebook_editor() {
            let toolbar_context = INotebookCellActionContext::new(notebook_editor).with_cell(element.clone()).with_ui(true).to_dynamic(Some("cellToolbar"));
            view.toolbar.tool_bar().set_context(Some(toolbar_context.clone()));
            view.delete_toolbar.tool_bar().set_context(Some(toolbar_context));
        }
    }
}

impl ICellTitleToolbar for CellTitleToolbarPart {
    /// `onDidUpdateActions`.
    fn on_did_update_actions(&self) -> Event<()> {
        self.on_did_update_actions.event()
    }

    /// `hasActions`.
    fn has_actions(&self) -> bool {
        let Some(model) = self.model.borrow().clone() else {
            return false;
        };

        model.actions.primary.len() + model.actions.secondary.len() + model.delete_actions.primary.len() + model.delete_actions.secondary.len() > 0
    }
}

impl_cell_overlay_part!(CellTitleToolbarPart, "CellTitleToolbarPart");
