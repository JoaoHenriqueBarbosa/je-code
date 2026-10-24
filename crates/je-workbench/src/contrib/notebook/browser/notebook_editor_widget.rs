//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/notebookEditorWidget.ts`: só o contrato do
//! `NotebookEditorWidget` que a árvore de resultados de busca lê (`searchResult.ts` e `notebookSearchModel.ts`).
//!
//! Divergências:
//! - A classe é o trait [`NotebookEditorWidget`]; o `widget instanceof NotebookEditorWidget` de `SearchResultImpl` é
//!   sempre verdadeiro para quem implementa o trait.
//! - `new FindMatchDecorationModel(widget, searchInstanceID)` (que o `NotebookCompatibleFileMatch` chama) é o método
//!   [`NotebookEditorWidget::create_find_match_decoration_model`], porque o gerenciador de decorações pertence ao widget.
//! - `textModel?: NotebookTextModel` é `text_model()` com `Option<Rc<dyn INotebookTextModel>>`; `hasModel()` é
//!   `has_model()`. `onWillChangeModel` e `onDidAttachViewModel` são os eventos de mesmo nome.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::promise::Promise;
use je_editor::range::Range;

use super::contrib::find::find_match_decoration_model::FindMatchDecorationModel;
use super::notebook_browser::{CellFindMatchWithIndex, ICellViewModel, INotebookFindOptions};
use crate::contrib::notebook::common::notebook_common::INotebookTextModel;

/// `NotebookEditorWidget`, só com os membros que a busca lê.
pub trait NotebookEditorWidget {
    fn get_id(&self) -> String;
    fn text_model(&self) -> Option<Rc<dyn INotebookTextModel>>;
    fn has_model(&self) -> bool;
    fn on_will_change_model(&self) -> Event<Option<Rc<dyn INotebookTextModel>>>;
    fn on_did_attach_view_model(&self) -> Event<()>;
    fn get_cell_index(&self, cell: &Rc<dyn ICellViewModel>) -> Option<usize>;
    fn reveal_cell_offset_in_center(&self, cell: &Rc<dyn ICellViewModel>, offset: f64);
    fn set_cell_editor_selection(&self, cell: &Rc<dyn ICellViewModel>, range: &Range);
    fn reveal_range_in_center_if_outside_viewport_async(&self, cell: &Rc<dyn ICellViewModel>, range: &Range) -> Promise<()>;
    #[allow(clippy::too_many_arguments)]
    fn find(
        &self,
        query: &str,
        options: &INotebookFindOptions,
        token: CancellationToken,
        skip_wrapping: bool,
        skip_reveal: bool,
        search_instance_id: &str,
    ) -> Promise<Vec<CellFindMatchWithIndex>>;
    fn create_find_match_decoration_model(&self, search_instance_id: &str) -> Rc<dyn FindMatchDecorationModel>;
}
