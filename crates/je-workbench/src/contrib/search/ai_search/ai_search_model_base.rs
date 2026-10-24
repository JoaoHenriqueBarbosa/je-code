//! Porte de `src/vs/workbench/contrib/search/browser/AISearch/aiSearchModelBase.ts`: o contrato do arquivo de resultado de
//! busca com IA (`ISearchTreeAIFileMatch`).
//!
//! Divergências: `isSearchTreeAIFileMatch(obj)` (que testa a existência de `getFullRange()` e se ele devolve um `Range`)
//! recebe o objeto já como [`ISearchTreeAIFileMatch`] e testa só o resultado de `get_full_range`.

use je_editor::range::Range;

use crate::contrib::search::search_tree_model::search_tree_common::ISearchTreeFileMatch;

/// `ISearchTreeAIFileMatch`.
pub trait ISearchTreeAIFileMatch: ISearchTreeFileMatch {
    fn get_full_range(&self) -> Option<Range>;
    fn rank(&self) -> i64;
}

/// `isSearchTreeAIFileMatch`.
pub fn is_search_tree_ai_file_match(obj: &dyn ISearchTreeAIFileMatch) -> bool {
    obj.get_full_range().is_some()
}
