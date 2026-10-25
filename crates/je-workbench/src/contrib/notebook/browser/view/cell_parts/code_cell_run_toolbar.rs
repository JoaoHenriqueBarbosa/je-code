//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/codeCellRunToolbar.ts`: `RunToolbar`, a barra com o botão de
//! executar da célula de código (e, se o menu secundário tiver ações, a seta com as demais opções de execução), e
//! `getCodeCellExecutionContextKeyService`, o serviço de chaves de contexto de onde as dicas de atalho do botão são resolvidas.
//!
//! Divergências:
//! - **Serviços injetados chegam como argumentos** ([`CellToolbarServices`], o mesmo conjunto das barras de `cellToolbars.ts`). O
//!   `readonly contextKeyService` do construtor é o `context_key_service` desse conjunto, que é o serviço de onde os menus são criados.
//!   O `IInstantiationService.createInstance(DropdownWithPrimaryActionViewItem, ...)` é a construção direta.
//! - `INotebookEditorDelegate` é o contrato estreito [`IRunToolbarEditor`]. `hasModel()` mais o estreitamento de tipo é
//!   `active_notebook_editor()` (ver `cell_toolbars.rs`).
//! - `cellContainer` e `runButtonContainer` são nós da árvore do `je-ui`; `cellContainer.classList.toggle(...)` é a classe do nó, e o
//!   `runButtonContainer` entra em `registerCellToolbarStickyScroll` como [`StickyTopNode`] (o substituto do `style.top`).
//! - `document.createElement('div')` do serviço de chaves escopado é um nó solto da árvore, como alvo ([`UiNodeTarget`]).
//! - **Chaves de contexto.** `EditorContextKeys.editorTextFocus`, `focus` e `textInputFocus` não são `RawContextKey` no porte do editor
//!   (são só as expressões `has(...)`), então a chave é criada no serviço pelo nome (`editorTextFocus`, `editorFocus`, `textInputFocus`),
//!   o que `bindTo` faz por baixo. As demais vêm de `cell_context_keys.rs` e de `je_platform::contextkey::contextkeys`.
//! - `/^inline/.test(g)` é `starts_with("inline")` (ver `get_cell_toolbar_actions`). O `getCellToolbarActions` do original é método de
//!   instância; aqui é a função de `cell_toolbars.rs`, a mesma que `BetweenCellToolbar` e `CellTitleToolbarPart` usam.
//! - `primary ? [primary] : []` do `updateActions`: a barra recebe a primeira ação primária, no máximo uma.

use std::rc::Rc;

use je_base::actions::{Action, ActionRef};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::nls::localize;
use je_platform::actions::{IMenu, MenuId, MenuItemAction};
use je_platform::contextkey::IContextKeyService;
use je_platform::contextkey::contextkey::IScopedContextKeyService;
use je_platform::contextkey::contextkeys::input_focused_context;
use je_ui::{NodeId, Role, Ui};
use je_widgets::action_bar::IActionViewItemProvider;
use je_widgets::action_view_items::{IActionViewItem, IActionViewItemOptions};
use je_widgets::dom::create_detached_el;
use je_widgets::dropdown_action_view_item::{IKeybindingProvider, MenuActions};
use je_widgets::toolbar::{IToolBarOptions, ToolBar};

use crate::contrib::notebook::browser::controller::{IActiveNotebookEditor, INotebookCellActionContext};
use crate::contrib::notebook::browser::notebook_options::NotebookOptions;
use crate::contrib::notebook::browser::view::cell_part::{CellPart, CellPartState};
use crate::contrib::notebook::browser::view::cell_parts::cell_context_keys::{NotebookCellExecutionStateContext, notebook_cell_execution_state, notebook_cell_list_focused, notebook_cell_type, notebook_editor_focused};
use crate::contrib::notebook::browser::view::cell_parts::cell_editor_overflow_widgets::UiNodeTarget;
use crate::contrib::notebook::browser::view::cell_parts::cell_toolbar_sticky_scroll::{ICellToolbarStickyScrollEditor, StickyTopNode, register_cell_toolbar_sticky_scroll};
use crate::contrib::notebook::browser::view::cell_parts::cell_toolbars::{CellToolbarServices, get_cell_toolbar_actions};
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::platform::actions::dropdown_with_primary_action_view_item::{DropdownWithPrimaryActionViewItem, IDropdownWithPrimaryActionViewItemOptions};
use crate::platform::actions::menu_entry_action_view_item::MenuEntryServices;
use crate::platform::contextview::ContextMenuServiceProvider;

/// O que `RunToolbar` chama do `INotebookEditorDelegate` (e do `INotebookEditor` que o `registerCellToolbarStickyScroll` recebe).
pub trait IRunToolbarEditor: ICellToolbarStickyScrollEditor {
    /// `hasModel()` com o estreitamento para o editor ativo (ver as divergências do módulo).
    fn active_notebook_editor(&self) -> Option<Rc<dyn IActiveNotebookEditor>>;
    /// `notebookOptions`.
    fn notebook_options(&self) -> Rc<NotebookOptions>;
}

/// `RunToolbar`.
pub struct RunToolbar {
    state: CellPartState,
    notebook_editor: Rc<dyn IRunToolbarEditor>,
    run_button_container: Rc<StickyTopNode>,
    toolbar: Rc<ToolBar>,
}

impl RunToolbar {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ui: &Ui,
        notebook_editor: Rc<dyn IRunToolbarEditor>,
        cell_container: NodeId,
        run_button_container: NodeId,
        primary_menu_id: MenuId,
        secondary_menu_id: MenuId,
        services: CellToolbarServices,
    ) -> Rc<Self> {
        let state = CellPartState::new();

        let primary_menu = services.menu_service.create_menu(&primary_menu_id, services.context_key_service.clone(), None);
        state.register(primary_menu.clone());
        let secondary_menu = services.menu_service.create_menu(&secondary_menu_id, services.context_key_service.clone(), None);
        state.register(secondary_menu.clone());
        let run_button_container = StickyTopNode::new(ui, run_button_container);
        let toolbar = Self::create_run_cell_toolbar(ui, &state, &run_button_container, cell_container, &primary_menu, &secondary_menu, &services);

        let update_actions: Rc<dyn Fn()> = {
            let (primary_menu, toolbar) = (primary_menu.clone(), toolbar.clone());
            Rc::new(move || {
                let actions = get_cell_toolbar_actions(&*primary_menu);
                // Only allow one primary action
                let primary: Vec<ActionRef> = actions.primary.first().cloned().into_iter().collect();
                toolbar.set_actions(&primary, &[]);
            })
        };
        update_actions();
        {
            let update_actions = update_actions.clone();
            state.register(primary_menu.on_did_change().listen(move |_| update_actions()));
        }
        {
            let update_actions = update_actions.clone();
            state.register(secondary_menu.on_did_change().listen(move |_| update_actions()));
        }
        state.register(notebook_editor.notebook_options().on_did_change_options().listen(move |_| update_actions()));

        Rc::new(Self { state, notebook_editor, run_button_container, toolbar })
    }

    /// `createRunCellToolbar(container, cellContainer, contextKeyService)`.
    fn create_run_cell_toolbar(
        ui: &Ui,
        state: &CellPartState,
        run_button_container: &Rc<StickyTopNode>,
        cell_container: NodeId,
        primary_menu: &Rc<dyn IMenu>,
        secondary_menu: &Rc<dyn IMenu>,
        services: &CellToolbarServices,
    ) -> Rc<ToolBar> {
        let action_view_item_disposables = DisposableStore::new();
        state.register(action_view_item_disposables.clone());
        let dropdown_action = Action::new("notebook.moreRunActions", &localize("notebook.moreRunActionsLabel", "More...", &[]), "codicon-chevron-down", true, None);
        state.register(dropdown_action.clone());

        let execution_context_key_service = get_code_cell_execution_context_key_service(ui, &*services.context_key_service);
        {
            let execution_context_key_service = execution_context_key_service.clone();
            state.register(to_disposable(move || execution_context_key_service.dispose()));
        }
        let keybinding_provider: IKeybindingProvider = {
            let keybinding_service = services.keybinding_service.clone();
            Rc::new(move |action: &ActionRef| {
                let context: &dyn IContextKeyService = &*execution_context_key_service;
                keybinding_service.lookup_keybinding(&action.id(), Some(context), false)
            })
        };

        let context_menu_provider = ContextMenuServiceProvider::new(services.context_menu_service.clone());
        let entry_services = MenuEntryServices { keybinding_service: services.keybinding_service.clone(), context_key_service: services.context_key_service.clone() };
        let action_view_item_provider: IActionViewItemProvider = {
            let (ui, primary_menu, secondary_menu) = (ui.clone(), primary_menu.clone(), secondary_menu.clone());
            let keybinding_provider = keybinding_provider.clone();
            let context_menu_provider = context_menu_provider.clone();
            Rc::new(move |_action: &ActionRef, options: &IActionViewItemOptions| {
                action_view_item_disposables.clear();

                let primary = get_cell_toolbar_actions(&*primary_menu).primary.first().cloned()?;
                if !primary.as_any().is::<MenuItemAction>() {
                    return None;
                }

                let secondary = get_cell_toolbar_actions(&*secondary_menu).secondary;
                if secondary.is_empty() {
                    return None;
                }

                let item = DropdownWithPrimaryActionViewItem::new(
                    &ui,
                    primary,
                    dropdown_action.clone(),
                    MenuActions::Actions(secondary),
                    "notebook-cell-run-toolbar",
                    Some(IDropdownWithPrimaryActionViewItemOptions { hover_delegate: options.hover_delegate.clone(), get_key_binding: Some(keybinding_provider.clone()), ..IDropdownWithPrimaryActionViewItemOptions::default() }),
                    context_menu_provider.clone(),
                    entry_services.clone(),
                );
                {
                    let ui = ui.clone();
                    action_view_item_disposables.add(item.on_did_change_dropdown_visibility().listen(move |visible| {
                        ui.tree_mut().toggle_class(cell_container, "cell-run-toolbar-dropdown-active", *visible);
                    }));
                }

                let item: Rc<dyn IActionViewItem> = item;
                Some(item)
            })
        };

        let toolbar = ToolBar::new(
            ui,
            run_button_container.node(),
            context_menu_provider,
            IToolBarOptions {
                get_key_binding: Some(keybinding_provider),
                action_view_item_provider: Some(action_view_item_provider),
                render_dropdown_as_child_element: true,
                ..IToolBarOptions::default()
            },
        );
        state.register(toolbar.clone());
        toolbar
    }
}

impl CellPart for RunToolbar {
    fn state(&self) -> &CellPartState {
        &self.state
    }

    fn did_render_cell(&self, element: &CellViewModel) {
        let sticky_scroll_editor: Rc<dyn ICellToolbarStickyScrollEditor> = self.notebook_editor.clone();
        self.state.cell_disposables().add(register_cell_toolbar_sticky_scroll(sticky_scroll_editor, element.clone(), self.run_button_container.clone(), None));

        if let Some(notebook_editor) = self.notebook_editor.active_notebook_editor() {
            let context = INotebookCellActionContext::new(notebook_editor).with_cell(element.clone()).with_ui(true);
            self.toolbar.set_context(Some(context.to_dynamic(None)));
        }
    }
}

impl_cell_content_part!(RunToolbar, "RunToolbar");

/// `getCodeCellExecutionContextKeyService(contextKeyService)`: um serviço de chaves escopado, num nó solto, com as chaves que um
/// editor de célula de código focado e ocioso teria.
pub fn get_code_cell_execution_context_key_service(ui: &Ui, context_key_service: &dyn IContextKeyService) -> Rc<dyn IScopedContextKeyService> {
    // Create a fake ContextKeyService, and look up the keybindings within this context.
    let node = create_detached_el(&mut ui.tree_mut(), Role::Generic, "");
    let execution_context_key_service = context_key_service.create_scoped(UiNodeTarget::new(ui, node));
    let key_service: &dyn IContextKeyService = &*execution_context_key_service;
    input_focused_context().bind_to(key_service).set(Dynamic::from(true));
    key_service.create_key("editorTextFocus", Dynamic::from(false)).set(Dynamic::from(true));
    key_service.create_key("editorFocus", Dynamic::from(false)).set(Dynamic::from(true));
    key_service.create_key("textInputFocus", Dynamic::from(false)).set(Dynamic::from(true));
    notebook_cell_execution_state().bind_to(key_service).set(Dynamic::from(NotebookCellExecutionStateContext::Idle.as_str()));
    notebook_cell_list_focused().bind_to(key_service).set(Dynamic::from(true));
    notebook_editor_focused().bind_to(key_service).set(Dynamic::from(true));
    notebook_cell_type().bind_to(key_service).set(Dynamic::from("code"));

    execution_context_key_service
}
