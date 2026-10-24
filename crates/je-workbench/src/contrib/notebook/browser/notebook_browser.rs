//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/notebookBrowser.ts`: só o contrato do que a árvore de
//! resultados de busca lê da célula e do editor de notebook (`ICellViewModel`, `CellEditState`, `CellWebviewFindMatch`,
//! `CellFindMatchWithIndex`) e as opções de busca do `INotebookFindOptions`.
//!
//! Divergências:
//! - O widget de notebook ainda não foi portado: `ICellViewModel` é o trait com os membros que `notebookSearchModel.ts` e
//!   `searchNotebookHelpers.ts` leem (`id`, `cellKind`, `textBuffer`, `resolveTextModel`, `getEditState` e
//!   `updateEditState`). `as_any` existe para o `instanceof` do original (`this._cell instanceof CellSearchModel`).
//! - `textBuffer` é o [`TextBufferRef`] compartilhado da célula e `resolveTextModel()` devolve `Promise<Rc<TextModel>>`.
//! - `INotebookFindOptions` (declarado em `notebookEditorWidget.ts`/`findModel.ts` no original, sempre com todos os campos
//!   opcionais) mora aqui, ao lado dos tipos de resultado de busca que o consomem.
//! - `INotebookEditor` é o contrato do editor com o que o lado do workbench do host de extensões lê e dirige
//!   (`mainThreadNotebookEditors.ts` e `mainThreadNotebookDocumentsAndEditors.ts`): seleção, foco, intervalos visíveis,
//!   revelar célula, `viewType` (`getViewModel().viewType`) e os eventos de modelo e de foco. Estende o
//!   [`NotebookEditorWidget`] (a leitura da busca): `getId`, `textModel`, `hasModel`. O que o widget faz com o foco e a rolagem é
//!   UI e fica com a fatia do editor.
//! - `getNotebookEditorFromEditorPane(editorPane)` é [`get_notebook_editor_from_editor_pane`], que pergunta ao painel
//!   ([`IEditorPane::notebook_editor`]) o controle de notebook que as três ramificações do original devolvem.
//! - `INotebookEditorOptions` (que estende `ITextEditorOptions`) é [`INotebookEditorOptions`] com os campos que o host de
//!   extensões preenche (`cellSelections`, `label`); o [`TextEditorOptions`] não tem os campos de notebook, então
//!   [`INotebookEditorOptions::to_text_editor_options`] leva `cellSelections` no `viewState` como o objeto `{ cellSelections }`.

use std::any::Any;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::js_string::JsString;
use je_base::promise::Promise;
use je_editor::model::FindMatch;
use je_editor::text_model::TextModel;

use crate::common::editor::{IEditorPane, TextEditorOptions};
use crate::contrib::notebook::browser::notebook_editor_widget::NotebookEditorWidget;
use crate::contrib::notebook::common::notebook_common::{CellKind, INotebookTextModel, TextBufferRef};
use crate::contrib::notebook::common::notebook_range::ICellRange;

/// `CellEditState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellEditState {
    /// Default state. For markup cells, this is the renderer view. For code cells, the browser editor is focused.
    Preview = 0,
    /// Eding mode. Source for markup or code is rendered in the editors and the state is editable.
    Editing = 1,
}

/// `ICellViewModel`, só com os membros que a busca lê.
pub trait ICellViewModel {
    fn id(&self) -> String;
    fn cell_kind(&self) -> CellKind;
    fn text_buffer(&self) -> TextBufferRef;
    fn resolve_text_model(&self) -> Promise<Rc<TextModel>>;
    fn get_edit_state(&self) -> CellEditState;
    fn update_edit_state(&self, new_state: CellEditState, source: &str);
    fn as_any(&self) -> &dyn Any;
}

/// O `{ start, end }` de `searchPreviewInfo.range`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchPreviewRange {
    pub start: i64,
    pub end: i64,
}

/// `searchPreviewInfo` de `CellWebviewFindMatch`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchPreviewInfo {
    pub line: JsString,
    pub range: SearchPreviewRange,
}

/// `CellWebviewFindMatch`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellWebviewFindMatch {
    pub index: usize,
    pub search_preview_info: Option<SearchPreviewInfo>,
}

/// `CellFindMatchWithIndex`.
#[derive(Clone)]
pub struct CellFindMatchWithIndex {
    pub index: usize,
    pub cell: Rc<dyn ICellViewModel>,
    pub content_matches: Vec<FindMatch>,
    pub webview_matches: Vec<CellWebviewFindMatch>,
}

/// `INotebookFindOptions`: as opções de `NotebookEditorWidget.find`.
#[derive(Clone, Debug, Default)]
pub struct INotebookFindOptions {
    pub regex: Option<bool>,
    pub whole_word: Option<bool>,
    pub case_sensitive: Option<bool>,
    pub word_separators: Option<String>,
    pub include_markup_input: Option<bool>,
    pub include_markup_preview: Option<bool>,
    pub include_code_input: Option<bool>,
    pub include_output: Option<bool>,
}

/// `INotebookEditor`, só com o que o lado do workbench do host de extensões lê e dirige (ver as divergências do módulo).
pub trait INotebookEditor: NotebookEditorWidget {
    /// `onDidChangeVisibleRanges`.
    fn on_did_change_visible_ranges(&self) -> Event<()>;
    /// `visibleRanges`.
    fn visible_ranges(&self) -> Vec<ICellRange>;
    /// `onDidChangeSelection`.
    fn on_did_change_selection(&self) -> Event<()>;
    /// `getSelections()`.
    fn get_selections(&self) -> Vec<ICellRange>;
    /// `setSelections(selections)`.
    fn set_selections(&self, selections: Vec<ICellRange>);
    /// `getFocus()`.
    fn get_focus(&self) -> ICellRange;
    /// `setFocus(focus)`.
    fn set_focus(&self, focus: ICellRange);
    /// `onDidChangeModel`.
    fn on_did_change_model(&self) -> Event<Option<Rc<dyn INotebookTextModel>>>;
    /// `onDidFocusWidget`.
    fn on_did_focus_widget(&self) -> Event<()>;
    /// `getViewModel().viewType`.
    fn view_type(&self) -> String;
    /// `getLength()`.
    fn get_length(&self) -> usize;
    /// `cellAt(index)`.
    fn cell_at(&self, index: usize) -> Option<Rc<dyn ICellViewModel>>;
    /// `revealCellRangeInView(range)`.
    fn reveal_cell_range_in_view(&self, range: ICellRange);
    /// `revealInCenter(cell)`.
    fn reveal_in_center(&self, cell: &Rc<dyn ICellViewModel>);
    /// `revealInCenterIfOutsideViewport(cell)`.
    fn reveal_in_center_if_outside_viewport(&self, cell: &Rc<dyn ICellViewModel>) -> Promise<()>;
    /// `revealInViewAtTop(cell)`.
    fn reveal_in_view_at_top(&self, cell: &Rc<dyn ICellViewModel>);
}

/// `getNotebookEditorFromEditorPane(editorPane?)`.
pub fn get_notebook_editor_from_editor_pane(editor_pane: Option<&Rc<dyn IEditorPane>>) -> Option<Rc<dyn INotebookEditor>> {
    editor_pane?.notebook_editor()
}

/// `INotebookEditorOptions`: os campos que o host de extensões preenche (ver as divergências do módulo).
#[derive(Clone, Default)]
pub struct INotebookEditorOptions {
    /// Os `ITextEditorOptions` de que herda.
    pub base: TextEditorOptions,
    /// `cellSelections`.
    pub cell_selections: Option<Vec<ICellRange>>,
    /// `label`.
    pub label: Option<String>,
}

impl INotebookEditorOptions {
    /// As opções como o [`TextEditorOptions`] da entrada sem tipo: `cellSelections` vai no `viewState` (o objeto
    /// `{ cellSelections: [{ start, end }] }`), e o `label` é o da própria entrada (devolvido à parte).
    pub fn to_text_editor_options(&self) -> TextEditorOptions {
        let mut options = self.base.clone();
        if let Some(cell_selections) = &self.cell_selections {
            let ranges = cell_selections
                .iter()
                .map(|range| Dynamic::object([("start", Dynamic::Number(range.start as f64)), ("end", Dynamic::Number(range.end as f64))]));
            options.base.view_state = Some(Dynamic::object([("cellSelections", Dynamic::array(ranges))]));
        }
        options
    }
}
