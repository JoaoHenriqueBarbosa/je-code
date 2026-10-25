//! Porte da `WorkbenchTable` de `src/vs/platform/list/browser/listService.ts` (com o `IWorkbenchTableOptions`, o
//! `IWorkbenchTableOptionsUpdate` e o `TableResourceNavigator`): a [`Table`] do `je-widgets` com o contexto de chaves
//! escopado, o registro no `ListService`, os estilos do tema, a configuração `workbench.list.*` e o `onDidOpen`.
//!
//! As árvores do workbench (`WorkbenchObjectTree` e companhia) vivem em [`crate::browser::workbench_tree`], que também guarda o
//! que as duas famílias compartilham (`createScopedContextKeyService`, `createScrollObserver`, as leituras da configuração).
//!
//! Divergências (as mesmas das árvores do workbench, ver `workbench_tree`):
//! - **Herança.** `WorkbenchTable<TRow> extends Table<TRow>` é um [`WorkbenchTable`] sobre a [`Table`] (`Deref` para ela). O
//!   `updateOptions` sobrescrito é [`WorkbenchTable::update_options`].
//! - **Injeção.** Os serviços do `@IContextKeyService`, `@IListService`, `@IConfigurationService` e o `defaultListStyles` são os
//!   campos de [`WorkbenchTreeServices`] (os mesmos das árvores). O `@IInstantiationService` e o `@IKeybindingService` do
//!   `toWorkbenchListOptions` não têm função aqui.
//! - **`toWorkbenchListOptions`.** O `smoothScrolling`, o `mouseWheelScrollSensitivity`, o `fastScrollSensitivity` e o
//!   `scrollByPage` são lidos da configuração para as opções de rolagem da lista. O `keyboardNavigationDelegate`, o
//!   `keyboardNavigationEventFilter`, o `multipleSelectionController` (e por isso o `useAltAsMultipleSelectionModifier`, que é só
//!   lido e mantido atualizado) e o `horizontalScrolling` não existem nas opções da lista do porte.
//! - **`IListWidget`.** Como no original, o serviço pergunta se o `getHTMLElement()` da tabela (o `.monaco-table`) é o elemento
//!   ativo. Quem tem o foco é o `.monaco-list` dela, então a pergunta é falsa, como no VS Code.
//! - **`TableResourceNavigator`.** O [`ResourceNavigator`] recebe os eventos da tabela como `TreeEvent` e `TreeMouseEvent` (o
//!   alvo do mouse é sempre `Unknown`: a lista não tem twistie), e o `getSelectedElement()` é o primeiro elemento selecionado.

use std::any::Any;
use std::cell::Cell;
use std::ops::Deref;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{Disposable, IDisposable};
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService, IScopedContextKeyService};
use je_platform::list::list_service::{
    IListWidget, workbench_list_double_selection, workbench_list_has_selection_or_focus, workbench_list_multi_selection, workbench_list_selection_navigation,
    workbench_list_supports_multi_select_context_key,
};
use je_ui::{NodeId, Ui};
use je_widgets::list::list_styles::IListStyles;
use je_widgets::list::list_types::{ListEvent, ListMouseEvent};
use je_widgets::table::table_types::{ITableColumn, ITableRenderer, ITableVirtualDelegate};
use je_widgets::table::table_widget::{ITableOptionsUpdate, Table, TableOptions};
use je_widgets::tree::tree_types::{TreeEvent, TreeMouseEvent, TreeMouseEventTarget};

use crate::browser::resource_navigator::{IOpenEvent, IResourceNavigatorOptions, ResourceNavigator};
use crate::browser::workbench_tree::{
    FAST_SCROLL_SENSITIVITY_KEY, LIST_SMOOTH_SCROLLING, MOUSE_WHEEL_SCROLL_SENSITIVITY_KEY, MULTI_SELECT_MODIFIER_SETTING_KEY, SCROLL_BY_PAGE_KEY, WorkbenchTreeServices, config_bool,
    config_number, create_scoped_context_key_service, create_scroll_observer, use_alt_as_multiple_selection_modifier,
};

/// `IWorkbenchTableOptionsUpdate`: o `ITableOptionsUpdate` com o `overrideStyles` e o `multipleSelectionSupport` (do
/// `IListOptionsUpdate`, que não está nas opções de rolagem da lista).
#[derive(Clone, Default)]
pub struct IWorkbenchTableOptionsUpdate {
    pub table: ITableOptionsUpdate,
    pub multiple_selection_support: Option<bool>,
    pub override_styles: Option<IListStyles>,
}

/// `IWorkbenchTableOptions<T>`: o `ITableOptions`, o `IResourceNavigatorOptions`, o `selectionNavigation` e o `overrideStyles`.
#[derive(Clone)]
pub struct IWorkbenchTableOptions<T> {
    pub table: TableOptions<T>,
    pub resource_navigator: IResourceNavigatorOptions,
    pub selection_navigation: bool,
    pub override_styles: Option<IListStyles>,
}

impl<T> Default for IWorkbenchTableOptions<T> {
    fn default() -> Self {
        Self { table: TableOptions::default(), resource_navigator: IResourceNavigatorOptions::default(), selection_navigation: false, override_styles: None }
    }
}

/// A tabela como o [`ListService`](je_platform::list::list_service::ListService) a guarda (`WorkbenchListWidget`).
struct WorkbenchTableListWidget<TRow: Clone + 'static> {
    ui: Ui,
    table: Rc<Table<TRow, TRow>>,
}

impl<TRow: Clone + 'static> IListWidget for WorkbenchTableListWidget<TRow> {
    fn is_active_element(&self) -> bool {
        self.ui.focused() == Some(self.table.get_html_element())
    }

    fn get_focused_element(&self) -> Option<Rc<dyn Any>> {
        self.table.get_focused_elements().into_iter().next().map(|element| Rc::new(element) as Rc<dyn Any>)
    }

    fn on_did_focus(&self) -> Event<()> {
        self.table.on_did_focus()
    }

    fn on_did_dispose(&self) -> Event<()> {
        self.table.on_did_dispose()
    }

    fn set_last_focused(&self, last_focused: bool) {
        self.ui.tree_mut().toggle_class(self.table.get_html_element(), "last-focused", last_focused);
    }
}

/// `WorkbenchTable<TRow>`.
pub struct WorkbenchTable<TRow: Clone + 'static> {
    base: Rc<Disposable>,
    table: Rc<Table<TRow, TRow>>,
    /// `contextKeyService`.
    pub context_key_service: Rc<dyn IScopedContextKeyService>,
    list_supports_multi_select: Rc<dyn IContextKey>,
    default_list_styles: Rc<dyn Fn() -> IListStyles>,
    use_alt_as_multiple_selection_modifier: Rc<Cell<bool>>,
    navigator: Rc<ResourceNavigator<TRow>>,
}

impl<TRow: Clone + 'static> Deref for WorkbenchTable<TRow> {
    type Target = Table<TRow, TRow>;

    fn deref(&self) -> &Table<TRow, TRow> {
        &self.table
    }
}

impl<TRow: Clone + 'static> WorkbenchTable<TRow> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        user: &str,
        ui: &Ui,
        container: NodeId,
        delegate: Rc<dyn ITableVirtualDelegate<TRow>>,
        columns: Vec<ITableColumn<TRow, TRow>>,
        renderers: Vec<Rc<dyn ITableRenderer<TRow>>>,
        mut options: IWorkbenchTableOptions<TRow>,
        services: &WorkbenchTreeServices,
    ) -> Rc<Self> {
        // toWorkbenchListOptions + `{ keyboardSupport: false, ...workbenchListOptions, horizontalScrolling }`
        let configuration_service = services.configuration_service.as_ref();
        let multiple_selection_support = options.table.list.multiple_selection_support;
        {
            let update = &mut options.table.list.view.update;
            update.smooth_scrolling = Some(config_bool(configuration_service, LIST_SMOOTH_SCROLLING));
            update.mouse_wheel_scroll_sensitivity = config_number(configuration_service, MOUSE_WHEEL_SCROLL_SENSITIVITY_KEY);
            update.fast_scroll_sensitivity = config_number(configuration_service, FAST_SCROLL_SENSITIVITY_KEY);
            update.scroll_by_page = Some(config_bool(configuration_service, SCROLL_BY_PAGE_KEY));
        }
        if options.table.list.keyboard_support.is_none() {
            options.table.list.keyboard_support = Some(false);
        }

        let table = Table::new(user, ui, container, delegate, columns, renderers, options.table);
        let base = Disposable::new();

        let context_key_service = create_scoped_context_key_service(&services.context_key_service);
        let keys: &dyn IContextKeyService = &*context_key_service;

        {
            let (top, height, render) = (Rc::downgrade(&table), Rc::downgrade(&table), Rc::downgrade(&table));
            base.register(create_scroll_observer(
                keys,
                move || top.upgrade().map_or(0.0, |table| table.scroll_top()),
                move || height.upgrade().map_or(0.0, |table| table.scroll_height()),
                move || render.upgrade().map_or(0.0, |table| table.render_height()),
                &table.on_did_scroll().signal(),
            ));
        }

        let list_supports_multi_select = workbench_list_supports_multi_select_context_key().bind_to(keys);
        list_supports_multi_select.set(Dynamic::Bool(multiple_selection_support != Some(false)));

        let list_selection_navigation = workbench_list_selection_navigation().bind_to(keys);
        list_selection_navigation.set(Dynamic::Bool(options.selection_navigation));

        let list_has_selection_or_focus = workbench_list_has_selection_or_focus().bind_to(keys);
        let list_double_selection = workbench_list_double_selection().bind_to(keys);
        let list_multi_selection = workbench_list_multi_selection().bind_to(keys);

        let use_alt = Rc::new(Cell::new(use_alt_as_multiple_selection_modifier(configuration_service)));

        base.register(context_key_service.clone());
        base.register(services.list_service.register(Rc::new(WorkbenchTableListWidget { ui: ui.clone(), table: table.clone() }), Vec::new()));

        let this = Rc::new(Self {
            base: base.clone(),
            table: table.clone(),
            context_key_service: context_key_service.clone(),
            list_supports_multi_select,
            default_list_styles: services.default_list_styles.clone(),
            use_alt_as_multiple_selection_modifier: use_alt.clone(),
            navigator: ResourceNavigator::new(
                table.on_did_change_selection().map(|event: &ListEvent<TRow>| TreeEvent { elements: event.elements.clone(), browser_event: event.browser_event }, None),
                table.on_pointer().map(Self::to_tree_mouse_event, None),
                table.on_mouse_dbl_click().map(Self::to_tree_mouse_event, None),
                {
                    let weak = Rc::downgrade(&table);
                    move || weak.upgrade().and_then(|table| table.get_selected_elements().into_iter().next())
                },
                Some(IResourceNavigatorOptions {
                    configuration_service: options.resource_navigator.configuration_service.clone().or_else(|| Some(services.configuration_service.clone())),
                    open_on_single_click: options.resource_navigator.open_on_single_click,
                }),
            ),
        });
        base.register(this.navigator.clone());

        this.update_styles(options.override_styles.as_ref());

        {
            let (weak, keys_service) = (Rc::downgrade(&table), context_key_service.clone());
            let (has_selection_or_focus, multi_selection, double_selection) = (list_has_selection_or_focus.clone(), list_multi_selection, list_double_selection);
            base.register(table.on_did_change_selection().listen(move |_| {
                let Some(table) = weak.upgrade() else { return };
                let selection = table.get_selection();
                let focus = table.get_focus();

                keys_service.buffer_change_events(&|| {
                    has_selection_or_focus.set(Dynamic::Bool(!selection.is_empty() || !focus.is_empty()));
                    multi_selection.set(Dynamic::Bool(selection.len() > 1));
                    double_selection.set(Dynamic::Bool(selection.len() == 2));
                });
            }));
        }
        {
            let weak = Rc::downgrade(&table);
            base.register(table.on_did_change_focus().listen(move |_| {
                let Some(table) = weak.upgrade() else { return };
                list_has_selection_or_focus.set(Dynamic::Bool(!table.get_selection().is_empty() || !table.get_focus().is_empty()));
            }));
        }
        {
            let (service, weak) = (services.configuration_service.clone(), Rc::downgrade(&table));
            base.register(services.configuration_service.on_did_change_configuration().listen(move |e| {
                let service = service.as_ref();
                if e.affects_configuration(MULTI_SELECT_MODIFIER_SETTING_KEY, None) {
                    use_alt.set(use_alt_as_multiple_selection_modifier(service));
                }

                let mut update = ITableOptionsUpdate::default();
                let mut changed = false;

                if e.affects_configuration(SCROLL_BY_PAGE_KEY, None) {
                    update.scroll_by_page = Some(config_bool(service, SCROLL_BY_PAGE_KEY));
                    changed = true;
                }
                if e.affects_configuration(LIST_SMOOTH_SCROLLING, None) {
                    update.smooth_scrolling = Some(config_bool(service, LIST_SMOOTH_SCROLLING));
                    changed = true;
                }
                if e.affects_configuration(MOUSE_WHEEL_SCROLL_SENSITIVITY_KEY, None) {
                    update.mouse_wheel_scroll_sensitivity = config_number(service, MOUSE_WHEEL_SCROLL_SENSITIVITY_KEY);
                    changed = true;
                }
                if e.affects_configuration(FAST_SCROLL_SENSITIVITY_KEY, None) {
                    update.fast_scroll_sensitivity = config_number(service, FAST_SCROLL_SENSITIVITY_KEY);
                    changed = true;
                }
                if changed && let Some(table) = weak.upgrade() {
                    table.update_options(&update);
                }
            }));
        }

        this
    }

    /// O `onPointer` e o `onMouseDblClick` da tabela como o `ResourceNavigator` os lê.
    fn to_tree_mouse_event(event: &ListMouseEvent<TRow>) -> TreeMouseEvent<TRow> {
        TreeMouseEvent { browser_event: event.browser_event, is_handled_by_list: event.is_handled_by_list.clone(), element: event.element.clone(), target: TreeMouseEventTarget::Unknown }
    }

    /// `onDidOpen`.
    pub fn on_did_open(&self) -> Event<IOpenEvent<TRow>> {
        self.navigator.on_did_open()
    }

    /// `useAltAsMultipleSelectionModifier`.
    pub fn use_alt_as_multiple_selection_modifier(&self) -> bool {
        self.use_alt_as_multiple_selection_modifier.get()
    }

    /// `updateOptions(options)`.
    pub fn update_options(&self, options: &IWorkbenchTableOptionsUpdate) {
        self.table.update_options(&options.table);

        if options.override_styles.is_some() {
            self.update_styles(options.override_styles.as_ref());
        }

        if let Some(support) = options.multiple_selection_support {
            self.list_supports_multi_select.set(Dynamic::Bool(support));
        }
    }

    /// `updateStyles(styles)`: `this.style(styles ? getListStyles(styles) : defaultListStyles)`.
    fn update_styles(&self, styles: Option<&IListStyles>) {
        self.table.style(&styles.cloned().unwrap_or_else(|| (self.default_list_styles)()));
    }

    /// A tabela por baixo, para quem precisa do `Rc`.
    pub fn widget(&self) -> &Rc<Table<TRow, TRow>> {
        &self.table
    }
}

impl<TRow: Clone + 'static> IDisposable for WorkbenchTable<TRow> {
    fn dispose(&self) {
        self.base.dispose();
        self.table.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "WorkbenchTable"
    }
}
