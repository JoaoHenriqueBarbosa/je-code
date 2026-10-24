//! Porte de `src/vs/workbench/contrib/snippets/browser/snippets.ts`: o contrato `ISnippetsService` e as opções de
//! `getSnippets`.
//!
//! Notas de tradução:
//! - `Snippet` e `SnippetFile` são objetos com identidade no original (o provedor de sugestões usa `Set<Snippet>` e o
//!   serviço devolve as mesmas instâncias a cada chamada), então o contrato os entrega como `Rc`.
//! - `Promise<Iterable<SnippetFile>>` é `Promise<Vec<Rc<SnippetFile>>>`.
//! - Os parâmetros opcionais (`resourceUri?`, `opt?`) são `Option`; `languageId: string | undefined` é
//!   `Option<&str>`.
//! - `isEnabled`, `updateEnablement` e `updateUsageTimestamp` recebem o snippet por referência.

use std::rc::Rc;

use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

use super::snippets_file::{Snippet, SnippetFile};

/// `ISnippetGetOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ISnippetGetOptions {
    pub include_disabled_snippets: Option<bool>,
    pub include_no_prefix_snippets: Option<bool>,
    pub no_recency_sort: Option<bool>,
    pub file_template_snippets: Option<bool>,
}

/// `ISnippetsService`.
pub trait ISnippetsService {
    fn get_snippet_files(&self) -> Promise<Vec<Rc<SnippetFile>>>;

    fn is_enabled(&self, snippet: &Snippet) -> bool;

    fn update_enablement(&self, snippet: &Snippet, enabled: bool);

    fn update_usage_timestamp(&self, snippet: &Snippet);

    fn get_snippets(&self, language_id: Option<&str>, resource_uri: Option<&URI>, opt: Option<&ISnippetGetOptions>) -> Promise<Vec<Rc<Snippet>>>;

    fn get_snippets_sync(&self, language_id: &str, resource_uri: Option<&URI>, opt: Option<&ISnippetGetOptions>) -> Vec<Rc<Snippet>>;
}

service_id!(pub I_SNIPPETS_SERVICE: dyn ISnippetsService = "snippetService");
