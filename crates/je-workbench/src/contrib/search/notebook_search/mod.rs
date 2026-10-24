//! Porte de `src/vs/workbench/contrib/search/browser/notebookSearch`: os resultados de busca dentro de notebooks.
//!
//! Fica de fora `notebookSearchService.ts` e `notebookSearchContributions.ts`, que dependem do serviço de notebooks e da
//! busca em notebooks fechados e não entram na árvore de resultados.

pub mod notebook_search_model;
pub mod notebook_search_model_base;
pub mod search_notebook_helpers;
