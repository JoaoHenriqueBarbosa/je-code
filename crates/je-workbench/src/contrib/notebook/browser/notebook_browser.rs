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

use std::any::Any;
use std::rc::Rc;

use je_base::js_string::JsString;
use je_base::promise::Promise;
use je_editor::model::FindMatch;
use je_editor::text_model::TextModel;

use crate::contrib::notebook::common::notebook_common::{CellKind, TextBufferRef};

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
