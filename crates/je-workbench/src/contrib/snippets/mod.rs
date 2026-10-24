//! Porte de `src/vs/workbench/contrib/snippets/browser`: o serviço de snippets do workbench (arquivos de snippet do
//! usuário, do workspace e das extensões) e o provedor de sugestões que os oferece no editor.
//!
//! - [`snippets`]: o contrato `ISnippetsService` (`snippets.ts`).
//! - [`snippets_file`]: `Snippet`, `SnippetFile` e `SnippetSource` (`snippetsFile.ts`).
//! - [`snippets_service`]: `SnippetsService` e o ponto de extensão `snippets` (`snippetsService.ts`).
//! - [`snippet_completion_provider`]: `SnippetCompletion` e `SnippetCompletionProvider`
//!   (`snippetCompletionProvider.ts`).

pub mod snippet_completion_provider;
pub mod snippets;
pub mod snippets_file;
pub mod snippets_service;
