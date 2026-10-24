//! Porte de `src/vs/workbench/contrib/search/common/notebookSearch.ts`: o contrato `INotebookSearchService`.
//!
//! Divergências: `ResourceSet` é o [`SharedResourceSet`](crate::services::search::search::SharedResourceSet), e o
//! `{ openFilesToScan, completeData, allScannedFiles }` é o [`NotebookSearchResult`].

use je_base::cancellation::CancellationToken;
use je_base::promise::Promise;
use je_instantiation::service_id;

use crate::services::search::search::{ISearchComplete, ITextQuery, SearchProgressCallback, SharedResourceSet};

/// O retorno de `INotebookSearchService.notebookSearch`.
pub struct NotebookSearchResult {
    pub open_files_to_scan: SharedResourceSet,
    pub complete_data: Promise<ISearchComplete>,
    pub all_scanned_files: Promise<SharedResourceSet>,
}

/// `INotebookSearchService`.
pub trait INotebookSearchService {
    fn notebook_search(
        &self,
        query: &ITextQuery,
        token: Option<CancellationToken>,
        search_instance_id: &str,
        on_progress: Option<SearchProgressCallback>,
    ) -> NotebookSearchResult;
}

service_id!(pub I_NOTEBOOK_SEARCH_SERVICE: dyn INotebookSearchService = "notebookSearchService");
