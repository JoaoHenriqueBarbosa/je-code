//! Porte parcial de `src/vs/workbench/contrib/multiDiffEditor/browser/multiDiffSourceResolverService.ts`: só o
//! `MultiDiffEditorItem`, o item de um editor de diff de vários arquivos (o `NotebookMultiDiffEditorItem` o estende). O
//! `IMultiDiffSourceResolverService` e o `MultiDiffSourceResolverService` não são portados.
//!
//! Divergências:
//! - `contextKeys?: Record<string, ContextKeyValue>` é uma lista de pares `(chave, Dynamic)` na ordem de inserção; o
//!   `ContextKeyValue` é um [`Dynamic`] (como em `EditorGroupContextKeyProvider`).
//! - `BugIndicatingError('Invalid arguments')` do construtor é o `errors::throw` com o erro de bug.

use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{self, Error};
use je_base::uri::URI;

/// `MultiDiffEditorItem`.
#[derive(Clone, Debug)]
pub struct MultiDiffEditorItem {
    pub original_uri: Option<URI>,
    pub modified_uri: Option<URI>,
    pub go_to_file_uri: Option<URI>,
    pub go_to_file_editor_title: Option<String>,
    pub context_keys: Option<Vec<(String, Dynamic)>>,
}

impl MultiDiffEditorItem {
    pub fn new(
        original_uri: Option<URI>,
        modified_uri: Option<URI>,
        go_to_file_uri: Option<URI>,
        go_to_file_editor_title: Option<String>,
        context_keys: Option<Vec<(String, Dynamic)>>,
    ) -> Self {
        if original_uri.is_none() && modified_uri.is_none() {
            errors::throw(Error::bug_indicating(Some("Invalid arguments")));
        }
        Self { original_uri, modified_uri, go_to_file_uri, go_to_file_editor_title, context_keys }
    }

    /// `getKey()`: `JSON.stringify([modifiedUri?.toString(), originalUri?.toString()])`.
    pub fn get_key(&self) -> String {
        let to_dynamic = |uri: &Option<URI>| uri.as_ref().map_or(Dynamic::Null, |uri| Dynamic::string(&uri.to_string(false).to_string_lossy()));
        json_stringify(&Dynamic::array([to_dynamic(&self.modified_uri), to_dynamic(&self.original_uri)])).unwrap_or_default()
    }
}
