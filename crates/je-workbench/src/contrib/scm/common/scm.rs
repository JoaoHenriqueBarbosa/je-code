//! Porte parcial de `src/vs/workbench/contrib/scm/common/scm.ts` e do `ISCMHistoryProvider` de `history.ts`: só o recorte de
//! contratos que o `ScmMultiDiffSourceResolver` e o `ScmHistoryItemResolver` leem.
//!
//! Divergências:
//! - Ficam de fora tudo o que o recorte não lê: `ISCMInput`, `ISCMViewService`, os contadores, a contribuição de ações e os
//!   demais membros de `ISCMProvider`, `ISCMResource` e `ISCMRepository`.
//! - `ISCMService.getRepository(idOrResource: string | URI)` é [`ISCMService::get_repository`] só com o id: a sobrecarga por
//!   `URI` (que procura o repositório pela raiz) não é lida pelo recorte.
//! - `ISCMProvider.rootUri?: URI` é `Option<URI>`; `historyProvider: IObservable<ISCMHistoryProvider | undefined>` é um
//!   `IObservable<Option<Rc<dyn ISCMHistoryProvider>>>` (a igualdade do observável é a identidade do `Rc`).
//! - `ISCMResource.multiDiffEditorOriginalUri` e `multiDiffEditorModifiedUri` são `Option<URI>`.
//! - `ISCMHistoryProvider.provideHistoryItemChanges` devolve `Promise<Option<Vec<..>>>` (o `undefined` do original é `None`).

use std::rc::Rc;

use je_base::event::Event;
use je_base::observable::base::IObservable;
use je_base::promise::Promise;
use je_base::uri::URI;

use super::history::ISCMHistoryItemChange;

/// `ISCMHistoryProvider` (só `provideHistoryItemChanges`).
pub trait ISCMHistoryProvider {
    fn provide_history_item_changes(&self, history_item_id: &str, history_item_parent_id: Option<&str>) -> Promise<Option<Vec<ISCMHistoryItemChange>>>;
}

/// `ISCMResource`.
pub trait ISCMResource {
    fn source_uri(&self) -> URI;
    fn multi_diff_editor_original_uri(&self) -> Option<URI> {
        None
    }
    fn multi_diff_editor_modified_uri(&self) -> Option<URI> {
        None
    }
}

/// `ISCMResourceGroup`.
pub trait ISCMResourceGroup {
    fn id(&self) -> String;
    fn label(&self) -> String;
    fn resources(&self) -> Vec<Rc<dyn ISCMResource>>;
    fn on_did_change_resources(&self) -> Event<()>;
}

/// `ISCMProvider`.
pub trait ISCMProvider {
    fn id(&self) -> String;
    fn provider_id(&self) -> String;
    fn root_uri(&self) -> Option<URI>;
    fn groups(&self) -> Vec<Rc<dyn ISCMResourceGroup>>;
    fn on_did_change_resource_groups(&self) -> Event<()>;
    fn history_provider(&self) -> IObservable<Option<Rc<dyn ISCMHistoryProvider>>>;
}

/// `ISCMRepository`.
pub trait ISCMRepository {
    fn id(&self) -> String;
    fn provider(&self) -> Rc<dyn ISCMProvider>;
}

/// `ISCMService`.
pub trait ISCMService {
    fn on_did_add_repository(&self) -> Event<Rc<dyn ISCMRepository>>;
    fn repositories(&self) -> Vec<Rc<dyn ISCMRepository>>;
    fn repository_count(&self) -> usize {
        self.repositories().len()
    }
    fn get_repository(&self, id: &str) -> Option<Rc<dyn ISCMRepository>>;
}
