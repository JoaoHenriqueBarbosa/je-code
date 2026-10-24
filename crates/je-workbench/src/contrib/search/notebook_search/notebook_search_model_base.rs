//! Porte de `src/vs/workbench/contrib/search/browser/notebookSearch/notebookSearchModelBase.ts`: os contratos do arquivo
//! de notebook (`INotebookFileInstanceMatch`), do casamento dentro de uma célula (`IMatchInNotebook`) e da célula
//! (`ICellMatch`) na árvore de resultados.
//!
//! Divergências:
//! - `isNotebookFileMatch(obj)` e `isIMatchInNotebook(obj)` (predicados estruturais) são
//!   [`ISearchTreeFileMatch::as_notebook_file_match`] e [`ISearchTreeMatch::as_match_in_notebook`], que devolvem o objeto
//!   como o trait mais específico; [`is_notebook_file_match`] e [`is_i_match_in_notebook`] são os atalhos com o nome do original.
//! - `IMatchInNotebook.parent()` (que estreita o tipo de retorno de `ISearchTreeMatch.parent()`) é
//!   [`IMatchInNotebook::notebook_parent`].
//! - `Map<number, string>` do contexto é um [`OrderedMap`]; `unbindNotebookEditorWidget(editor)` aceita `None` (a
//!   implementação do original tem o parâmetro opcional).

use std::rc::Rc;

use je_base::js_string::JsString;
use je_base::map::OrderedMap;
use je_base::promise::Promise;

use crate::contrib::notebook::browser::notebook_browser::ICellViewModel;
use crate::contrib::notebook::browser::notebook_editor_widget::NotebookEditorWidget;
use crate::contrib::search::notebook_search::search_notebook_helpers::INotebookCellMatch;
use crate::contrib::search::search_tree_model::search_tree_common::{ISearchTreeFileMatch, ISearchTreeMatch};
use crate::services::search::search::TextSearchMatch;

/// `INotebookFileInstanceMatch`.
pub trait INotebookFileInstanceMatch: ISearchTreeFileMatch {
    fn bind_notebook_editor_widget(&self, editor: Rc<dyn NotebookEditorWidget>);
    fn update_matches_for_editor_widget(&self) -> Promise<()>;
    fn unbind_notebook_editor_widget(&self, editor: Option<&Rc<dyn NotebookEditorWidget>>);
    fn update_notebook_highlights(&self);
    fn get_cell_match(&self, cell_id: &str) -> Option<Rc<dyn ICellMatch>>;
    fn add_cell_match(&self, raw_cell: &INotebookCellMatch);
    fn show_match(&self, match_: &Rc<dyn IMatchInNotebook>) -> Promise<()>;
    fn cell_matches(&self) -> Vec<Rc<dyn ICellMatch>>;
}

/// `isNotebookFileMatch`.
pub fn is_notebook_file_match(obj: &Rc<dyn ISearchTreeFileMatch>) -> Option<Rc<dyn INotebookFileInstanceMatch>> {
    obj.as_notebook_file_match()
}

/// `IMatchInNotebook`.
pub trait IMatchInNotebook: ISearchTreeMatch {
    /// `parent()` (o pai visível na árvore de busca).
    fn notebook_parent(&self) -> Rc<dyn INotebookFileInstanceMatch>;
    fn cell_parent(&self) -> Rc<dyn ICellMatch>;
    fn is_webview_match(&self) -> bool;
    fn cell_index(&self) -> usize;
    fn webview_index(&self) -> Option<usize>;
    fn cell(&self) -> Option<Rc<dyn ICellViewModel>>;
}

/// `isIMatchInNotebook`.
pub fn is_i_match_in_notebook(obj: &Rc<dyn ISearchTreeMatch>) -> Option<Rc<dyn IMatchInNotebook>> {
    obj.as_match_in_notebook()
}

/// `ICellMatch`.
pub trait ICellMatch {
    fn has_cell_view_model(&self) -> bool;
    fn context(&self) -> OrderedMap<i64, JsString>;
    fn matches(&self) -> Vec<Rc<dyn IMatchInNotebook>>;
    fn content_matches(&self) -> Vec<Rc<dyn IMatchInNotebook>>;
    fn webview_matches(&self) -> Vec<Rc<dyn IMatchInNotebook>>;
    fn remove(&self, matches: &[Rc<dyn IMatchInNotebook>]);
    fn clear_all_matches(&self);
    fn add_content_matches(&self, text_search_matches: &[TextSearchMatch]);
    fn add_context(&self, text_search_matches: &[TextSearchMatch]);
    fn add_webview_matches(&self, text_search_matches: &[TextSearchMatch]);
    fn set_cell_model(&self, cell: Rc<dyn ICellViewModel>);
    fn parent(&self) -> Rc<dyn INotebookFileInstanceMatch>;
    fn id(&self) -> String;
    fn cell_index(&self) -> usize;
    fn cell(&self) -> Option<Rc<dyn ICellViewModel>>;
}
