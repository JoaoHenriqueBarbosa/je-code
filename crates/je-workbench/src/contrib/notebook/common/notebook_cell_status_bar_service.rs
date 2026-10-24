//! Porte de `src/vs/workbench/contrib/notebook/common/notebookCellStatusBarService.ts`: o contrato do serviço que reúne os
//! provedores de itens da barra de status das células (`INotebookCellStatusBarService`). A implementação
//! (`notebookCellStatusBarServiceImpl.ts`) fica para a fatia dos serviços.
//!
//! Divergências: o provedor é o `Rc<dyn INotebookCellStatusBarItemProvider>` de `notebook_common`, e o índice da célula é
//! `usize`.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

use super::notebook_common::{INotebookCellStatusBarItemList, INotebookCellStatusBarItemProvider};

/// `INotebookCellStatusBarService`.
pub trait INotebookCellStatusBarService {
    fn on_did_change_providers(&self) -> Event<()>;
    fn on_did_change_items(&self) -> Event<()>;

    fn register_cell_status_bar_item_provider(&self, provider: Rc<dyn INotebookCellStatusBarItemProvider>) -> DisposableRef;

    fn get_status_bar_items_for_cell(&self, doc_uri: &URI, cell_index: usize, view_type: &str, token: &CancellationToken) -> Promise<Vec<INotebookCellStatusBarItemList>>;
}

service_id!(pub I_NOTEBOOK_CELL_STATUS_BAR_SERVICE: dyn INotebookCellStatusBarService = "notebookCellStatusBarService");
