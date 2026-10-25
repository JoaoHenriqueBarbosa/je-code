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
use std::rc::Rc;

use je_base::errors::Error;
use je_instantiation::service_id;

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

service_id!(pub I_LIST_SERVICE: dyn IListService = "listService");
