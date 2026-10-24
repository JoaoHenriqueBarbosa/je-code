//! Porte de `src/vs/workbench/contrib/scm`: só o contrato do `IQuickDiffModelService` que a camada de API lê
//! (`browser/quickDiffModel.ts`), em [`browser::quick_diff_model`].

pub mod browser;
