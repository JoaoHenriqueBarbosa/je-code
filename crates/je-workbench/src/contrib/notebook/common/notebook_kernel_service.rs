//! Porte parcial de `src/vs/workbench/contrib/notebook/common/notebookKernelService.ts`: só o contrato
//! `INotebookTextModelLike` de que `notebookCommon.ts` depende (`INotebookTextModel` o estende). O resto do arquivo
//! (`INotebookKernel`, `INotebookKernelService`, as ações de origem e o histórico de kernels) fica para a fatia do
//! serviço de kernels.
//!
//! Divergência: `INotebookTextModelLike` é uma interface estrutural (`{ uri: URI; notebookType: string }`); aqui é um
//! trait, e `INotebookTextModel` o tem como supertrait.

use je_base::uri::URI;

/// `INotebookTextModelLike`.
pub trait INotebookTextModelLike {
    fn uri(&self) -> URI;
    fn notebook_type(&self) -> String;
}
