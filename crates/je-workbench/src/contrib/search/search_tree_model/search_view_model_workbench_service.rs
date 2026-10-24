//! Porte de `src/vs/workbench/contrib/search/browser/searchTreeModel/searchViewModelWorkbenchService.ts`: o contrato
//! `ISearchViewModelWorkbenchService`, que guarda o modelo de busca da visão.
//!
//! Divergências: `searchModel` (propriedade com `get` e `set`) é [`ISearchViewModelWorkbenchService::search_model`] e
//! [`ISearchViewModelWorkbenchService::set_search_model`].

use std::rc::Rc;

use je_instantiation::service_id;

use super::search_tree_common::ISearchModel;

/// `ISearchViewModelWorkbenchService`.
pub trait ISearchViewModelWorkbenchService {
    fn search_model(&self) -> Rc<dyn ISearchModel>;
    fn set_search_model(&self, search_model: Rc<dyn ISearchModel>);
}

service_id!(pub I_SEARCH_VIEW_MODEL_WORKBENCH_SERVICE: dyn ISearchViewModelWorkbenchService = "searchViewModelWorkbenchService");
