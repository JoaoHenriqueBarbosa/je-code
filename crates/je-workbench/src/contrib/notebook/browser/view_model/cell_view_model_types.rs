//! Os tipos de `src/vs/workbench/contrib/notebook/browser/notebookBrowser.ts` que os view models de célula e o view model do
//! notebook usam (o que o porte parcial de `notebook_browser.rs` ainda não tem): `CellFocusMode`, `CellFoldingState`,
//! `CellLayoutState`, `CellLayoutContext`, os `*CellLayoutInfo` e `*CellLayoutChangeEvent`, `CursorAtBoundary`,
//! `INotebookViewCellsUpdateEvent`, `EditorFoldingStateDelegate` e `INotebookEditorViewState`.
//!
//! Divergências:
//! - `CellFindMatch.cell` é o [`CellViewModel`] (a única forma de `ICellViewModel` que existe na camada lógica).
//! - `IEditorCommentsOptions` e `ICodeEditorViewState` não existem como tipos do porte; o primeiro é um [`Dynamic`] (o valor lido
//!   da configuração) e o segundo é guardado como [`Dynamic`] opaco, já que o editor de código da célula é Fase 6.
//! - `fontInfo: FontInfo | null` é `Option<Rc<FontInfo>>`; os campos numéricos opcionais dos eventos de layout são `Option<f64>`
//!   e os booleanos opcionais são `bool` (ausente é falso), exceto onde o original distingue `undefined` de `0`.

use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_editor::config::font_info::FontInfo;
use je_editor::model::FindMatch;

use super::notebook_view_model_impl::CellViewModel;

/// `CellFocusMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellFocusMode {
    Container,
    Editor,
    Output,
    ChatInput,
}

/// `CellFoldingState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellFoldingState {
    None,
    Expanded,
    Collapsed,
}

/// `CellLayoutState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellLayoutState {
    Uninitialized,
    Estimated,
    FromCache,
    Measured,
}

/// `CellLayoutContext`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellLayoutContext {
    Fold,
}

/// `CursorAtBoundary`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorAtBoundary {
    None,
    Top,
    Bottom,
    Both,
}

/// `CursorAtLineBoundary`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorAtLineBoundary {
    None,
    Start,
    End,
    Both,
}

/// `CodeCellLayoutInfo`.
#[derive(Clone, Debug)]
pub struct CodeCellLayoutInfo {
    pub font_info: Option<Rc<FontInfo>>,
    pub editor_height: f64,
    pub editor_width: f64,
    pub chat_height: f64,
    pub status_bar_height: f64,
    pub comment_offset: f64,
    pub comment_height: f64,
    pub output_container_offset: f64,
    pub output_total_height: f64,
    pub output_show_more_container_height: f64,
    pub output_show_more_container_offset: f64,
    pub total_height: f64,
    pub code_indicator_height: f64,
    pub output_indicator_height: f64,
    pub bottom_toolbar_offset: f64,
    pub layout_state: CellLayoutState,
    pub estimated_has_horizontal_scrolling: bool,
    pub outline_width: f64,
    pub top_margin: f64,
    pub bottom_margin: f64,
}

/// `MarkupCellLayoutInfo`.
#[derive(Clone, Debug)]
pub struct MarkupCellLayoutInfo {
    pub chat_height: f64,
    pub editor_height: f64,
    pub preview_height: f64,
    pub font_info: Option<Rc<FontInfo>>,
    pub editor_width: f64,
    pub comment_offset: f64,
    pub comment_height: f64,
    pub bottom_toolbar_offset: f64,
    pub total_height: f64,
    pub layout_state: CellLayoutState,
    pub fold_hint_height: f64,
    pub status_bar_height: f64,
    pub outline_width: f64,
    pub bottom_margin: f64,
    pub top_margin: f64,
}

/// `CellLayoutChangeEvent`: o que `BaseCellViewModel.layoutChange` recebe (a união dos dois eventos concretos). `font` é o
/// `FontInfo` novo; `outer_width` é o `outerWidth` novo.
#[derive(Clone, Debug, Default)]
pub struct CellLayoutChangeEvent {
    pub font: Option<Rc<FontInfo>>,
    pub outer_width: Option<f64>,
    pub comment_height: bool,
    // `CodeCellLayoutChangeEvent`
    pub source: Option<String>,
    pub editor_height: bool,
    pub chat_height: bool,
    pub output_height: bool,
    pub output_show_more_container_height: Option<f64>,
    /// `totalHeight?: boolean` no evento de código e `totalHeight?: number` no de markup (`MarkupCellLayoutChangeEvent`).
    pub total_height_changed: bool,
    pub total_height: Option<f64>,
    pub context: Option<CellLayoutContext>,
}

/// `CellFindMatch`.
#[derive(Clone)]
pub struct CellFindMatch {
    pub cell: CellViewModel,
    pub content_matches: Vec<FindMatch>,
}

/// `INotebookViewCellsUpdateEvent<CellViewModel>`: as emendas (`[start, deleteCount, newCells]`) que mudaram as células de
/// view.
#[derive(Clone)]
pub struct INotebookViewCellsUpdateEvent {
    pub synchronous: bool,
    pub splices: Vec<(usize, usize, Vec<CellViewModel>)>,
}

/// `EditorFoldingStateDelegate`.
pub trait EditorFoldingStateDelegate {
    fn get_cell_index(&self, cell: &CellViewModel) -> i64;
    fn get_folding_state(&self, cell_index: i64) -> CellFoldingState;
    /// `getFoldingState(getCellIndex(this))` para a célula de `handle`: a célula markdown só conhece o próprio `handle` quando
    /// calcula a altura (o `this` do original é o `CellViewModel` envolvente, que a célula não guarda).
    fn get_folding_state_by_handle(&self, handle: i64) -> CellFoldingState;
}

/// `INotebookEditorViewState`. `editorViewStates` (o `ICodeEditorViewState` de cada célula) é guardado como [`Dynamic`] opaco.
#[derive(Clone, Debug, Default)]
pub struct INotebookEditorViewState {
    pub editing_cells: HashMap<usize, bool>,
    pub editor_view_states: HashMap<usize, Dynamic>,
    pub cell_total_heights: Option<HashMap<usize, f64>>,
    pub cell_line_number_states: HashMap<usize, String>,
    pub collapsed_input_cells: HashMap<usize, bool>,
    pub collapsed_output_cells: HashMap<usize, bool>,
}
