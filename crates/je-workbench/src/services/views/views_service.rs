//! Porte parcial de `src/vs/workbench/services/views/common/viewsService.ts`: o contrato [`IViewsService`] que o
//! `TimelineService` e o `OutputService` usam para abrir e consultar views.
//!
//! Divergências:
//! - Só entram `openView`, `getViewWithId`, `getActiveViewWithId`, `onDidChangeViewVisibility` e `isViewVisible`: o resto da
//!   interface (`getViewProgressIndicator`...) pertence à camada de UI (`IView`, `IViewPaneContainer`), que ainda não foi
//!   portada. `openView<T extends IView>(id, focus)` devolve `Promise<T | null>`; como o `IView` não existe, o retorno genérico
//!   é um método por view que algum chamador lê (`open_explorer_view`, `open_output_view`), e o `openView` sem tipo devolve
//!   `Promise<()>`, que assenta quando a view abriu. `getActiveViewWithId<OutputViewPane>` é `get_active_output_view_with_id`.
//! - **`OutputViewPane`** (`outputView.ts`) não foi portado: [`IOutputViewPane`] é o que o `OutputService` lê dele
//!   (`showChannel`). `onDidChangeViewVisibility` emite o [`ViewVisibilityChangeEvent`] (`{ id, visible }`).

use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::promise::Promise;

use crate::contrib::files::browser_files::ExplorerReveal;
use crate::services::output::output::IOutputChannel;

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

/// `OutputViewPane`, só com o que o `OutputService` lê: `showChannel(channel, preserveFocus)`.
pub trait IOutputViewPane {
    /// `showChannel(channel, preserveFocus)`.
    fn show_channel(&self, channel: Rc<dyn IOutputChannel>, preserve_focus: bool);
}

/// O `{ id, visible }` de `onDidChangeViewVisibility`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewVisibilityChangeEvent {
    pub id: String,
    pub visible: bool,
}

/// `IViewsService`.
pub trait IViewsService {
    /// `openView(id, focus)`.
    fn open_view(&self, id: &str, focus: bool) -> Promise<()>;

    /// `openView(id, focus)` conferindo o resultado: se a view abriu (`true`) ou se o original devolveria `null`
    /// (`false`). Corpo padrão que lança `TypeError: openView is not a function`.
    fn try_open_view(&self, _id: &str, _focus: bool) -> Promise<bool> {
        not_a_function("openView")
    }

    /// `openViewContainer(id, focus)`: se o contêiner abriu (`true`) ou se o original devolveria `null` (`false`). Corpo
    /// padrão que lança `TypeError: openViewContainer is not a function`.
    fn open_view_container(&self, _id: &str, _focus: bool) -> Promise<bool> {
        not_a_function("openViewContainer")
    }

    /// `openView<ExplorerView>(id, focus)`: a view aberta, ou `None` (o `null` do original). Corpo padrão que lança
    /// `TypeError: openView is not a function` (o dublê que só define os outros membros).
    fn open_explorer_view(&self, _id: &str, _focus: bool) -> Promise<Option<Rc<dyn IExplorerViewPane>>> {
        not_a_function("openView")
    }

    /// `openView<OutputViewPane>(id, focus)`: a view aberta, ou `None` (o `null` do original). Corpo padrão que lança
    /// `TypeError: openView is not a function`.
    fn open_output_view(&self, _id: &str, _focus: bool) -> Promise<Option<Rc<dyn IOutputViewPane>>> {
        not_a_function("openView")
    }

    /// `getViewWithId(id)`. Corpo padrão que lança `TypeError: getViewWithId is not a function`.
    fn get_view_with_id(&self, _id: &str) -> Option<Rc<dyn IViewPane>> {
        not_a_function("getViewWithId")
    }

    /// `getActiveViewWithId<OutputViewPane>(id)`. Corpo padrão que lança `TypeError: getActiveViewWithId is not a function`.
    fn get_active_output_view_with_id(&self, _id: &str) -> Option<Rc<dyn IOutputViewPane>> {
        not_a_function("getActiveViewWithId")
    }

    /// `onDidChangeViewVisibility`. Corpo padrão que lança `TypeError: onDidChangeViewVisibility is not a function`.
    fn on_did_change_view_visibility(&self) -> Event<ViewVisibilityChangeEvent> {
        not_a_function("onDidChangeViewVisibility")
    }

    /// `isViewVisible(id)`. Corpo padrão que lança `TypeError: isViewVisible is not a function`.
    fn is_view_visible(&self, _id: &str) -> bool {
        not_a_function("isViewVisible")
    }
}

je_instantiation::service_id!(pub I_VIEWS_SERVICE: dyn IViewsService = "viewsService");
