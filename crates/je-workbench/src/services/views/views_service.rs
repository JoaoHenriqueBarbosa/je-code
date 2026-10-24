//! Porte parcial de `src/vs/workbench/services/views/common/viewsService.ts`: o contrato [`IViewsService`] que o
//! `TimelineService` usa para abrir a view de timeline.
//!
//! Divergências:
//! - Só `openView` entra: o resto da interface (`onDidChangeViewVisibility`, `isViewVisible`, `getActiveViewWithId`,
//!   `getViewProgressIndicator`...) pertence à camada de UI (`IView`, `IViewPaneContainer`), que ainda não foi
//!   portada. `openView<T extends IView>(id, focus)` devolve `Promise<T | null>`; como o `IView` não existe e
//!   nenhum chamador do porte lê a view devolvida, o retorno é `Promise<()>`, que assenta quando a view abriu.

use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::promise::Promise;

use crate::contrib::files::browser_files::ExplorerReveal;

/// O membro que o objeto do original não define: chamá-lo é um `TypeError`.
fn not_a_function(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// O que o `fileCommands.ts` usa de uma `ViewPane` aberta ou achada por id: `setExpanded` e `focus`.
pub trait IViewPane {
    /// `setExpanded(expanded)`: se o estado mudou.
    fn set_expanded(&self, expanded: bool) -> bool;

    /// `focus()`.
    fn focus(&self);
}

/// A `ExplorerView` que `openView<ExplorerView>` devolve: a `ViewPane` com a propriedade `autoReveal`
/// (`boolean | 'force' | 'focusNoScroll'`).
pub trait IExplorerViewPane: IViewPane {
    /// `autoReveal`.
    fn auto_reveal(&self) -> ExplorerReveal;

    /// `autoReveal = value`.
    fn set_auto_reveal(&self, value: ExplorerReveal);
}

/// `IViewsService`.
pub trait IViewsService {
    /// `openView(id, focus)`.
    fn open_view(&self, id: &str, focus: bool) -> Promise<()>;

    /// `openView<ExplorerView>(id, focus)`: a view aberta, ou `None` (o `null` do original). Corpo padrão que lança
    /// `TypeError: openView is not a function` (o dublê que só define os outros membros).
    fn open_explorer_view(&self, _id: &str, _focus: bool) -> Promise<Option<Rc<dyn IExplorerViewPane>>> {
        not_a_function("openView")
    }

    /// `getViewWithId(id)`. Corpo padrão que lança `TypeError: getViewWithId is not a function`.
    fn get_view_with_id(&self, _id: &str) -> Option<Rc<dyn IViewPane>> {
        not_a_function("getViewWithId")
    }
}

je_instantiation::service_id!(pub I_VIEWS_SERVICE: dyn IViewsService = "viewsService");
