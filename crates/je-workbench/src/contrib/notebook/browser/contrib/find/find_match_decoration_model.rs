//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/contrib/find/findMatchDecorationModel.ts`: o contrato do
//! gerenciador de decorações das ocorrências de busca de um notebook, só com os membros que `notebookSearchModel.ts`
//! lê (`stopWebviewFind`, `setAllFindMatchesDecorations`, `clearCurrentFindMatchDecoration` e os dois
//! `highlightCurrentFindMatchDecoration*`).
//!
//! Divergências:
//! - A classe é o trait [`FindMatchDecorationModel`], criado pelo widget (ver
//!   [`NotebookEditorWidget::create_find_match_decoration_model`](crate::contrib::notebook::browser::notebook_editor_widget::NotebookEditorWidget)).
//! - O deslocamento devolvido (`number | null`) é `Option<f64>`.

use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_editor::range::Range;

use crate::contrib::notebook::browser::notebook_browser::{CellFindMatchWithIndex, ICellViewModel};

/// `FindMatchDecorationModel`.
pub trait FindMatchDecorationModel: IDisposable {
    fn stop_webview_find(&self);
    fn set_all_find_matches_decorations(&self, cell_find_matches: &[CellFindMatchWithIndex]);
    fn clear_current_find_match_decoration(&self);
    fn highlight_current_find_match_decoration_in_cell(&self, cell: &Rc<dyn ICellViewModel>, cell_range: &Range) -> Promise<Option<f64>>;
    fn highlight_current_find_match_decoration_in_webview(&self, cell: &Rc<dyn ICellViewModel>, index: usize) -> Promise<Option<f64>>;
}
