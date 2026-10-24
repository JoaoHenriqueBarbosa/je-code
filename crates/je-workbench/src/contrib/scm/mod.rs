//! Porte de `src/vs/workbench/contrib/scm`: o contrato do `IQuickDiffModelService` que a camada de API lê
//! (`browser/quickDiffModel.ts`), em [`browser::quick_diff_model`], e o grafo do histórico (`browser/scmHistory.ts` e os
//! tipos de `common/history.ts`), em [`browser::scm_history`] e [`common::history`].

pub mod browser;
pub mod common;
