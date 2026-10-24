//! Porte de `src/vs/workbench/contrib/notebook/browser/contrib/find/findMatchDecorationModel.ts`: a classe `FindMatchDecorationModel` do
//! editor de notebook, que decora as ocorrências de busca e a atual (as decorações do editor de cada célula e as da régua de visão geral).
//! O contrato que a busca em arquivos lê dela é o trait de `find_match_decoration_model.rs`.
//!
//! Divergências:
//! - A classe se chama [`NotebookFindMatchDecorationModel`] (o nome do original é o do trait do módulo irmão) e trabalha sobre o
//!   `CellViewModel` concreto.
//! - `INotebookEditor` é o trait [`IFindDecorationsEditor`], só com o que a classe lê. `changeModelDecorations(callback)` recebe o
//!   acessor como [`IModelDecorationsChangeAccessor`] e o `callback` é `FnMut` (o editor pode não o chamar, como o `return null` do dublê).
//! - A decoração de célula da régua de visão geral (`overviewRuler: { color, modelRanges, includeOutput, position }`) não existe no
//!   [`INotebookCellDecorationOptions`] do porte (a régua é UI nativa): o `deltaCellDecorations` é chamado com as mesmas entradas e
//!   opções vazias, o que preserva a contagem e a vida das decorações.
//! - `FindDecorations._CURRENT_FIND_MATCH_DECORATION` e `_FIND_MATCH_DECORATION` são os acessores de `FindDecorations`.
//! - `async` é a [`Promise`] devolvida, e o construtor devolve `Rc<Self>` (os fechos guardam um `Weak`).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_editor::find::find_decorations::FindDecorations;
use je_editor::model::ModelDeltaDecoration;
use je_editor::range::Range;

use crate::contrib::notebook::browser::contrib::find::find_model::CellFindMatchModel;
use crate::contrib::notebook::browser::view_model::base_cell_view_model::INotebookCellDecorationOptions;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::{
    CellViewModel, ICellModelDecorations, ICellModelDeltaDecorations, ModelDecorationsChangeAccessor, NotebookDeltaDecoration,
};

/// `IModelDecorationsChangeAccessor`.
pub trait IModelDecorationsChangeAccessor {
    fn delta_decorations(&self, old_decorations: &[ICellModelDecorations], new_decorations: &[ICellModelDeltaDecorations]) -> Vec<ICellModelDecorations>;
}

impl IModelDecorationsChangeAccessor for ModelDecorationsChangeAccessor<'_> {
    fn delta_decorations(&self, old_decorations: &[ICellModelDecorations], new_decorations: &[ICellModelDeltaDecorations]) -> Vec<ICellModelDecorations> {
        ModelDecorationsChangeAccessor::delta_decorations(self, old_decorations, new_decorations)
    }
}

/// O que o `FindMatchDecorationModel` lê do `INotebookEditor`.
pub trait IFindDecorationsEditor {
    /// `changeModelDecorations(callback)`.
    fn change_model_decorations(&self, callback: &mut dyn FnMut(&dyn IModelDecorationsChangeAccessor));
    /// `deltaCellDecorations(oldDecorations, newDecorations)`.
    fn delta_cell_decorations(&self, old_decorations: &[String], new_decorations: Vec<NotebookDeltaDecoration>) -> Vec<String>;
    /// `findHighlightCurrent(matchIndex, ownerID)`: o deslocamento da saída, ou `None`.
    fn find_highlight_current(&self, match_index: usize, owner_id: &str) -> Promise<Option<f64>>;
    /// `findUnHighlightCurrent(matchIndex, ownerID)`.
    fn find_un_highlight_current(&self, match_index: usize, owner_id: &str);
    /// `findStop(ownerID)`.
    fn find_stop(&self, owner_id: &str);
}

/// `{ kind: 'input'; decorations: ICellModelDecorations[] } | { kind: 'output'; index: number }`.
#[derive(Clone, Debug)]
pub enum CurrentMatchDecorations {
    Input { decorations: Vec<ICellModelDecorations> },
    Output { index: usize },
}

/// `FindMatchDecorationModel`.
pub struct NotebookFindMatchDecorationModel {
    this: Weak<NotebookFindMatchDecorationModel>,
    base: Rc<Disposable>,
    notebook_editor: Rc<dyn IFindDecorationsEditor>,
    owner_id: String,
    all_matches_decorations: RefCell<Vec<ICellModelDecorations>>,
    current_match_cell_decorations: RefCell<Vec<String>>,
    all_matches_cell_decorations: RefCell<Vec<String>>,
    current_match_decorations: RefCell<Option<CurrentMatchDecorations>>,
}

/// Uma decoração de célula da régua, sem as opções de régua (ver as divergências do módulo).
fn ruler_decoration(handle: i64) -> NotebookDeltaDecoration {
    NotebookDeltaDecoration::Cell { handle, options: INotebookCellDecorationOptions::default() }
}

impl NotebookFindMatchDecorationModel {
    pub fn new(notebook_editor: Rc<dyn IFindDecorationsEditor>, owner_id: &str) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base: Disposable::new(),
            notebook_editor,
            owner_id: owner_id.to_string(),
            all_matches_decorations: RefCell::new(Vec::new()),
            current_match_cell_decorations: RefCell::new(Vec::new()),
            all_matches_cell_decorations: RefCell::new(Vec::new()),
            current_match_decorations: RefCell::new(None),
        })
    }

    pub fn current_match_decorations(&self) -> Option<CurrentMatchDecorations> {
        self.current_match_decorations.borrow().clone()
    }

    fn clear_decorations(&self) {
        self.clear_current_find_match_decoration();
        self.set_all_find_matches_decorations(&[]);
    }

    /// `highlightCurrentFindMatchDecorationInCell(cell, cellRange)`.
    pub fn highlight_current_find_match_decoration_in_cell(&self, cell: &CellViewModel, cell_range: &Range) -> Promise<Option<f64>> {
        self.clear_current_find_match_decoration();

        // match is an editor FindMatch, we update find match decoration in the editor
        // we will highlight the match in the webview
        self.notebook_editor.change_model_decorations(&mut |accessor| {
            let find_matches_options = FindDecorations::current_find_match_decoration();

            let decorations = vec![ModelDeltaDecoration { range: *cell_range, options: find_matches_options }];
            let delta_decoration = ICellModelDeltaDecorations { owner_id: cell.handle(), decorations };

            let previous = match &*self.current_match_decorations.borrow() {
                Some(CurrentMatchDecorations::Input { decorations }) => decorations.clone(),
                _ => Vec::new(),
            };
            let decorations = accessor.delta_decorations(&previous, &[delta_decoration]);
            *self.current_match_decorations.borrow_mut() = Some(CurrentMatchDecorations::Input { decorations });
        });

        let previous = self.current_match_cell_decorations.borrow().clone();
        *self.current_match_cell_decorations.borrow_mut() = self.notebook_editor.delta_cell_decorations(&previous, vec![ruler_decoration(cell.handle())]);

        Promise::resolved(None)
    }

    /// `highlightCurrentFindMatchDecorationInWebview(cell, index)`.
    pub fn highlight_current_find_match_decoration_in_webview(&self, cell: &CellViewModel, index: usize) -> Promise<Option<f64>> {
        self.clear_current_find_match_decoration();

        let weak = self.this.clone();
        let handle = cell.handle();
        let highlighted = self.notebook_editor.find_highlight_current(index, &self.owner_id);
        async_fn(async move {
            let offset = highlighted.await?;
            if let Some(this) = weak.upgrade() {
                *this.current_match_decorations.borrow_mut() = Some(CurrentMatchDecorations::Output { index });

                let previous = this.current_match_cell_decorations.borrow().clone();
                *this.current_match_cell_decorations.borrow_mut() = this.notebook_editor.delta_cell_decorations(&previous, vec![ruler_decoration(handle)]);
            }
            Ok(offset)
        })
    }

    /// `clearCurrentFindMatchDecoration()`.
    pub fn clear_current_find_match_decoration(&self) {
        let current = self.current_match_decorations.borrow().clone();
        match current {
            Some(CurrentMatchDecorations::Input { .. }) => {
                self.notebook_editor.change_model_decorations(&mut |accessor| {
                    let old = match &*self.current_match_decorations.borrow() {
                        Some(CurrentMatchDecorations::Input { decorations }) => decorations.clone(),
                        _ => Vec::new(),
                    };
                    accessor.delta_decorations(&old, &[]);
                    *self.current_match_decorations.borrow_mut() = None;
                });
            }
            Some(CurrentMatchDecorations::Output { index }) => {
                self.notebook_editor.find_un_highlight_current(index, &self.owner_id);
            }
            None => {}
        }

        let previous = self.current_match_cell_decorations.borrow().clone();
        *self.current_match_cell_decorations.borrow_mut() = self.notebook_editor.delta_cell_decorations(&previous, Vec::new());
    }

    /// `setAllFindMatchesDecorations(cellFindMatches)`.
    pub fn set_all_find_matches_decorations(&self, cell_find_matches: &[CellFindMatchModel<CellViewModel>]) {
        self.notebook_editor.change_model_decorations(&mut |accessor| {
            let find_matches_options = FindDecorations::find_match_decoration();

            let delta_decorations: Vec<ICellModelDeltaDecorations> = cell_find_matches
                .iter()
                .map(|cell_find_match| {
                    // Find matches
                    let new_find_matches_decorations: Vec<ModelDeltaDecoration> = cell_find_match
                        .content_matches
                        .iter()
                        .map(|content_match| ModelDeltaDecoration { range: content_match.range, options: find_matches_options.clone() })
                        .collect();

                    ICellModelDeltaDecorations { owner_id: cell_find_match.cell.handle(), decorations: new_find_matches_decorations }
                })
                .collect();

            let previous = self.all_matches_decorations.borrow().clone();
            *self.all_matches_decorations.borrow_mut() = accessor.delta_decorations(&previous, &delta_decorations);
        });

        let previous = self.all_matches_cell_decorations.borrow().clone();
        let new_decorations = cell_find_matches.iter().map(|cell_find_match| ruler_decoration(cell_find_match.cell.handle())).collect();
        *self.all_matches_cell_decorations.borrow_mut() = self.notebook_editor.delta_cell_decorations(&previous, new_decorations);
    }

    /// `stopWebviewFind()`.
    pub fn stop_webview_find(&self) {
        self.notebook_editor.find_stop(&self.owner_id);
    }
}

impl IDisposable for NotebookFindMatchDecorationModel {
    fn dispose(&self) {
        self.clear_decorations();
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "FindMatchDecorationModel"
    }
}
