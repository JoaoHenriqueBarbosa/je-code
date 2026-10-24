//! Porte parcial de `src/vs/workbench/services/views/common/viewsService.ts`: o contrato [`IViewsService`] que o
//! `TimelineService` usa para abrir a view de timeline.
//!
//! Divergências:
//! - Só `openView` entra: o resto da interface (`onDidChangeViewVisibility`, `isViewVisible`, `getActiveViewWithId`,
//!   `getViewProgressIndicator`...) pertence à camada de UI (`IView`, `IViewPaneContainer`), que ainda não foi
//!   portada. `openView<T extends IView>(id, focus)` devolve `Promise<T | null>`; como o `IView` não existe e
//!   nenhum chamador do porte lê a view devolvida, o retorno é `Promise<()>`, que assenta quando a view abriu.

use je_base::promise::Promise;

/// `IViewsService`.
pub trait IViewsService {
    /// `openView(id, focus)`.
    fn open_view(&self, id: &str, focus: bool) -> Promise<()>;
}

je_instantiation::service_id!(pub I_VIEWS_SERVICE: dyn IViewsService = "viewsService");
