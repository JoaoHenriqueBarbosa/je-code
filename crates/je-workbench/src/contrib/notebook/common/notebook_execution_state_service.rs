//! Porte parcial de `src/vs/workbench/contrib/notebook/common/notebookExecutionStateService.ts`: só o erro de execução
//! de célula (`ICellErrorStackFrame` e `ICellExecutionError`) de que `notebookCommon.ts` depende
//! (`NotebookCellInternalMetadata.error`). O resto do arquivo (`INotebookExecutionStateService`,
//! `INotebookCellExecution`, `INotebookExecution` e os eventos de mudança de execução) fica para a fatia do
//! serviço de estado de execução, que também traz `notebookExecutionService.ts`.
//!
//! Divergências:
//! - `IRange` e `Range` são o mesmo tipo no porte do editor ([`Range`]).
//! - `stack: string | ICellErrorStackFrame[] | undefined` é `Option<CellExecutionErrorStack>`.

use je_base::uri::UriComponents;
use je_editor::range::Range;

/// `ICellErrorStackFrame`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ICellErrorStackFrame {
    /// The location of this stack frame. This should be provided as a URI if the location of the call frame can be
    /// accessed by the editor.
    pub uri: Option<UriComponents>,

    pub location: Option<Range>,

    /// The name of the stack frame, typically a method or function name.
    pub label: String,
}

/// `string | ICellErrorStackFrame[]`: o valor definido de `ICellExecutionError.stack`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CellExecutionErrorStack {
    Text(String),
    Frames(Vec<ICellErrorStackFrame>),
}

/// `ICellExecutionError`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ICellExecutionError {
    pub name: String,
    pub message: String,
    pub stack: Option<CellExecutionErrorStack>,
    pub uri: UriComponents,
    pub location: Option<Range>,
}
