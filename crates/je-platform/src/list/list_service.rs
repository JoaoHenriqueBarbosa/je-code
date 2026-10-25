//! Porte parcial de `src/vs/platform/list/browser/listService.ts`: o contrato [`IListService`] e a lista que ele
//! devolve, no recorte que `getResourceForCommand` lê (a lista focada por último e o elemento em foco dela).
//!
//! Divergências:
//! - **`lastFocusedList`.** O original devolve `List | PagedList | ITree | AsyncDataTree | ...`, e o chamador faz
//!   `list?.getHTMLElement()`, `isActiveElement(element)` e `list instanceof List` ou `AsyncDataTree` para ler o foco. Sem
//!   DOM e sem as listas portadas, a lista é o [`IListWidget`]: [`IListWidget::is_active_element`] é o
//!   `element && isActiveElement(element)` e [`IListWidget::get_focused_element`] é o primeiro elemento de
//!   `getFocusedElements()` (`List`) ou de `getFocus()` (`AsyncDataTree`), ou `None` para um tipo de lista que o
//!   chamador não lê. O elemento viaja como [`Any`], e quem o lê o reconhece com `downcast_ref` (o `instanceof`).
//! - **Fora do recorte.** `register`, `lastFocusedList` como evento (`onDidChangeFocus`), as listas do workbench
//!   (`WorkbenchList` e companhia) e as opções de configuração entram com as listas.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, combined_disposable, to_disposable};
use je_instantiation::service_id;

use crate::contextkey::contextkey::{ContextKeyExpr, ContextKeyExpression, ContextKeyMeta, IContextKey, RawContextKey};
use crate::contextkey::contextkeys::{INPUT_FOCUSED_CONTEXT_KEY, raw_keys};

/// A lista que o [`IListService`] guarda (ver a nota do módulo).
pub trait IListWidget {
    /// `const element = list?.getHTMLElement(); element && isActiveElement(element)`: o elemento da lista existe e tem o
    /// foco do documento.
    fn is_active_element(&self) -> bool;

    /// O primeiro elemento em foco (`getFocusedElements()[0]` de uma `List`, `getFocus()[0]` de uma `AsyncDataTree`).
    fn get_focused_element(&self) -> Option<Rc<dyn Any>>;

    /// O `list instanceof WorkbenchList`: a lista como [`IWorkbenchList`], ou `None` para um tipo de lista que não é.
    fn as_workbench_list(self: Rc<Self>) -> Option<Rc<dyn IWorkbenchList>> {
        None
    }

    /// `tree instanceof WorkbenchDataTree || tree instanceof WorkbenchAsyncDataTree` seguido de `tree.getInput()`: `None` para uma
    /// lista que não é uma dessas árvores (o padrão), `Some(entrada)` para elas, com a entrada (`None` se ainda não há).
    fn workbench_data_tree_input(&self) -> Option<Option<Rc<dyn Any>>> {
        None
    }

    /// `widget.onDidFocus`: dispara quando a lista recebe o foco. O padrão nunca dispara (uma lista que não é registrada).
    fn on_did_focus(&self) -> Event<()> {
        Event::none()
    }

    /// `widget.onDidDispose`.
    fn on_did_dispose(&self) -> Event<()> {
        Event::none()
    }

    /// `getHTMLElement().classList.add/remove('last-focused')`: marca a lista como a focada por último.
    fn set_last_focused(&self, _last_focused: bool) {}
}

/// O recorte de `WorkbenchList` (`platform/list/browser/listService.ts`) que a visão acessível das notificações usa:
/// `indexOf`, `domFocus`, `setFocus`, `focusNext` e `focusPrevious`.
pub trait IWorkbenchList {
    /// `indexOf(element)`: o índice do elemento (comparado por identidade), ou `-1`.
    fn index_of(&self, element: &Rc<dyn Any>) -> isize;

    /// `domFocus()`.
    fn dom_focus(&self);

    /// `setFocus(indexes)`: lança (devolve o erro) para um índice fora da lista.
    fn set_focus(&self, indexes: &[isize]) -> Result<(), Error>;

    /// `focusNext()`.
    fn focus_next(&self);

    /// `focusPrevious()`.
    fn focus_previous(&self);
}

/// `IListService`.
pub trait IListService {
    /// `lastFocusedList`: the last focused list, or `None` if there is none.
    fn last_focused_list(&self) -> Option<Rc<dyn IListWidget>>;
}

/// `IRegisteredList`.
struct RegisteredList {
    widget: Rc<dyn IListWidget>,
    /// `extraContextKeys`: guardadas como no original (o serviço só as retém).
    #[allow(dead_code)]
    extra_context_keys: Vec<Rc<dyn IContextKey>>,
}

/// A identidade do objeto (o ponteiro de dados, sem a tabela virtual): o `===` do original.
fn same_widget(a: &Rc<dyn IListWidget>, b: &Rc<dyn IListWidget>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `ListService`: o registro das listas do workbench e a última com foco.
#[derive(Default)]
pub struct ListService {
    lists: RefCell<Vec<Rc<RegisteredList>>>,
    last_focused_widget: RefCell<Option<Rc<dyn IListWidget>>>,
}

impl ListService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    fn set_last_focused_list(&self, widget: Option<Rc<dyn IListWidget>>) {
        let previous = self.last_focused_widget.borrow().clone();
        let unchanged = match (&previous, &widget) {
            (Some(a), Some(b)) => same_widget(a, b),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }

        if let Some(previous) = &previous {
            previous.set_last_focused(false);
        }
        *self.last_focused_widget.borrow_mut() = widget.clone();
        if let Some(widget) = &widget {
            widget.set_last_focused(true);
        }
    }

    /// `register(widget, extraContextKeys)`: lança se a mesma lista for registrada duas vezes.
    pub fn register(self: &Rc<Self>, widget: Rc<dyn IListWidget>, extra_context_keys: Vec<Rc<dyn IContextKey>>) -> DisposableRef {
        if self.lists.borrow().iter().any(|list| same_widget(&list.widget, &widget)) {
            je_base::errors::throw(Error::new("Cannot register the same widget multiple times"));
        }

        // Keep in our lists list
        let registered_list = Rc::new(RegisteredList { widget: widget.clone(), extra_context_keys });
        self.lists.borrow_mut().push(registered_list.clone());

        // Check for currently being focused
        if widget.is_active_element() {
            self.set_last_focused_list(Some(widget.clone()));
        }

        let on_focus = {
            let (this, widget) = (self.clone(), widget.clone());
            widget.clone().on_did_focus().listen(move |_| this.set_last_focused_list(Some(widget.clone())))
        };
        let on_unregister = {
            let (this, registered_list) = (self.clone(), registered_list.clone());
            to_disposable(move || {
                let mut lists = this.lists.borrow_mut();
                if let Some(index) = lists.iter().position(|list| Rc::ptr_eq(list, &registered_list)) {
                    lists.remove(index);
                }
            })
        };
        let on_dispose = {
            let (this, registered_list) = (self.clone(), registered_list);
            widget.on_did_dispose().listen(move |_| {
                this.lists.borrow_mut().retain(|list| !Rc::ptr_eq(list, &registered_list));
                let is_last_focused = this.last_focused_widget.borrow().as_ref().is_some_and(|last| same_widget(last, &registered_list.widget));
                if is_last_focused {
                    this.set_last_focused_list(None);
                }
            })
        };

        combined_disposable(vec![on_focus, on_unregister, on_dispose])
    }
}

impl IListService for ListService {
    fn last_focused_list(&self) -> Option<Rc<dyn IListWidget>> {
        self.last_focused_widget.borrow().clone()
    }
}

raw_keys! {
    /// `RawWorkbenchListScrollAtBoundaryContextKey`: `'none' | 'top' | 'bottom' | 'both'`.
    raw_workbench_list_scroll_at_boundary_context_key: "listScrollAtBoundary", "none", ContextKeyMeta::None;
    /// `RawWorkbenchListFocusContextKey`.
    raw_workbench_list_focus_context_key: "listFocus", true, ContextKeyMeta::None;
    /// `WorkbenchTreeStickyScrollFocused`.
    workbench_tree_sticky_scroll_focused: "treestickyScrollFocused", false, ContextKeyMeta::None;
    /// `WorkbenchListSupportsMultiSelectContextKey`.
    workbench_list_supports_multi_select_context_key: "listSupportsMultiselect", true, ContextKeyMeta::None;
    /// `WorkbenchListHasSelectionOrFocus`.
    workbench_list_has_selection_or_focus: "listHasSelectionOrFocus", false, ContextKeyMeta::None;
    /// `WorkbenchListDoubleSelection`.
    workbench_list_double_selection: "listDoubleSelection", false, ContextKeyMeta::None;
    /// `WorkbenchListMultiSelection`.
    workbench_list_multi_selection: "listMultiSelection", false, ContextKeyMeta::None;
    /// `WorkbenchListSelectionNavigation`.
    workbench_list_selection_navigation: "listSelectionNavigation", false, ContextKeyMeta::None;
    /// `WorkbenchListSupportsFind`.
    workbench_list_supports_find: "listSupportsFind", true, ContextKeyMeta::None;
    /// `WorkbenchTreeElementCanCollapse`.
    workbench_tree_element_can_collapse: "treeElementCanCollapse", false, ContextKeyMeta::None;
    /// `WorkbenchTreeElementHasParent`.
    workbench_tree_element_has_parent: "treeElementHasParent", false, ContextKeyMeta::None;
    /// `WorkbenchTreeElementCanExpand`.
    workbench_tree_element_can_expand: "treeElementCanExpand", false, ContextKeyMeta::None;
    /// `WorkbenchTreeElementHasChild`.
    workbench_tree_element_has_child: "treeElementHasChild", false, ContextKeyMeta::None;
    /// `WorkbenchTreeFindOpen`.
    workbench_tree_find_open: "treeFindOpen", false, ContextKeyMeta::None;
}

/// `WorkbenchListTypeNavigationModeKey`.
pub const WORKBENCH_LIST_TYPE_NAVIGATION_MODE_KEY: &str = "listTypeNavigationMode";

/// `WorkbenchListAutomaticKeyboardNavigationLegacyKey` (obsoleta em favor da anterior).
pub const WORKBENCH_LIST_AUTOMATIC_KEYBOARD_NAVIGATION_LEGACY_KEY: &str = "listAutomaticKeyboardNavigation";

/// `WorkbenchListScrollAtTopContextKey`.
pub fn workbench_list_scroll_at_top_context_key() -> ContextKeyExpression {
    let key = raw_workbench_list_scroll_at_boundary_context_key();
    ContextKeyExpr::or([Some(key.is_equal_to("top")), Some(key.is_equal_to("both"))]).expect("a expressão tem termos")
}

/// `WorkbenchListScrollAtBottomContextKey`.
pub fn workbench_list_scroll_at_bottom_context_key() -> ContextKeyExpression {
    let key = raw_workbench_list_scroll_at_boundary_context_key();
    ContextKeyExpr::or([Some(key.is_equal_to("bottom")), Some(key.is_equal_to("both"))]).expect("a expressão tem termos")
}

/// `WorkbenchListFocusContextKey`: `listFocus && !inputFocus && !treestickyScrollFocused`.
pub fn workbench_list_focus_context_key() -> ContextKeyExpression {
    ContextKeyExpr::and([
        Some(raw_workbench_list_focus_context_key().expr()),
        Some(ContextKeyExpr::not(INPUT_FOCUSED_CONTEXT_KEY)),
        Some(workbench_tree_sticky_scroll_focused().to_negated()),
    ])
    .expect("a expressão tem termos")
}

service_id!(pub I_LIST_SERVICE: dyn IListService = "listService");
