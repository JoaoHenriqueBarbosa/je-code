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
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::promise::Promise;
use je_editor::range::Range;

use super::contrib::find::find_match_decoration_model::FindMatchDecorationModel;
use super::notebook_browser::{CellFindMatchWithIndex, ICellViewModel, INotebookFindOptions};
use crate::contrib::notebook::common::notebook_common::INotebookTextModel;

/// `{ message: unknown }` de `INotebookEditor.onDidReceiveMessage` (o `forRenderer?` não é lido pelo `MainThreadNotebookKernels`).
#[derive(Clone, Debug)]
pub struct INotebookWebviewMessage {
    pub message: Dynamic,
}

/// `NotebookEditorWidget`, só com os membros que a busca e o `MainThreadNotebookKernels` leem.
pub trait NotebookEditorWidget {
    fn get_id(&self) -> String;
    fn text_model(&self) -> Option<Rc<dyn INotebookTextModel>>;
    fn has_model(&self) -> bool;

    /// `onDidReceiveMessage`: a mensagem que um renderizador mandou. Corpo padrão: um editor sem webview não recebe.
    fn on_did_receive_message(&self) -> Event<INotebookWebviewMessage> {
        Event::none()
    }

    /// `postMessage(message)`: manda a mensagem aos renderizadores. Corpo padrão: um editor sem webview a descarta.
    fn post_message(&self, _message: Dynamic) {}

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

    /// O widget como `INotebookEditor` (`NotebookEditorWidget implements INotebookEditor`): quem implementa o contrato
    /// completo devolve a si mesmo. O padrão é `None`, o widget que só serve à leitura da busca. É o que permite ao
    /// `INotebookEditorService`, que entrega widgets, alimentar o lado do workbench do host de extensões.
    fn as_notebook_editor(self: Rc<Self>) -> Option<Rc<dyn crate::contrib::notebook::browser::notebook_browser::INotebookEditor>> {
        None
    }

    /// `onWillHide()`: avisa o widget de que o painel dele vai sumir. Corpo padrão: um widget sem painel não tem o que fazer.
    fn on_will_hide(&self) {}

    /// `getDomNode()`: o nó do widget na árvore de elementos, que o serviço de widgets retira depois de descartá-lo. Corpo
    /// padrão: um widget sem DOM devolve um nó solto, cujo `remove` não faz nada.
    fn get_dom_node(&self) -> Rc<dyn NotebookWidgetDomNode> {
        Rc::new(DetachedDomNode)
    }

    /// `dispose()`: descarta o widget. Tem outro nome para não colidir com o `IDisposable::dispose` de quem implementa os
    /// dois traits. Corpo padrão: nada a descartar.
    fn dispose_widget(&self) {}
}

/// O nó (`HTMLElement`) que `NotebookEditorWidget.getDomNode()` devolve, no que o serviço de widgets usa: `remove()`.
pub trait NotebookWidgetDomNode {
    fn remove(&self);
}

/// O nó de um widget sem DOM.
struct DetachedDomNode;

impl NotebookWidgetDomNode for DetachedDomNode {
    fn remove(&self) {}
}
