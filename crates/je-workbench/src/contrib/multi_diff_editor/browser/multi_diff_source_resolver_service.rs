//! Porte parcial de `src/vs/workbench/contrib/multiDiffEditor/browser/multiDiffSourceResolverService.ts`: o
//! `MultiDiffEditorItem`, o item de um editor de diff de vários arquivos (o `NotebookMultiDiffEditorItem` o estende), e os
//! contratos `IMultiDiffSourceResolver` e `IResolvedMultiDiffSource` que os resolvedores do SCM implementam. O
//! `IMultiDiffSourceResolverService` e o `MultiDiffSourceResolverService` não são portados.
//!
//! Divergências:
//! - `IResolvedMultiDiffSource.resources: IValueWithChangeEvent<readonly MultiDiffEditorItem[]>` é um
//!   [`MultiDiffResources`], cujo valor é um `Rc<Vec<MultiDiffEditorItem>>` (a lista é imutável e compartilhada, e a identidade
//!   do `Rc` faz o papel da identidade do array nos observáveis). `contextKeys` é [`IResolvedMultiDiffSource::context_keys`].
//! - `contextKeys?: Record<string, ContextKeyValue>` é uma lista de pares `(chave, Dynamic)` na ordem de inserção; o
//!   `ContextKeyValue` é um [`Dynamic`] (como em `EditorGroupContextKeyProvider`).
//! - `BugIndicatingError('Invalid arguments')` do construtor é o `errors::throw` com o erro de bug.

use std::rc::Rc;

use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{self, Error};
use je_base::event::ValueWithChangeEventLike;
use je_base::promise::Promise;
use je_base::uri::URI;

/// `IValueWithChangeEvent<readonly MultiDiffEditorItem[]>`.
pub type MultiDiffResources = Rc<dyn ValueWithChangeEventLike<Rc<Vec<MultiDiffEditorItem>>>>;

/// `IResolvedMultiDiffSource`.
pub trait IResolvedMultiDiffSource {
    fn resources(&self) -> MultiDiffResources;

    /// `contextKeys?: Record<string, ContextKeyValue>`, na ordem de inserção.
    fn context_keys(&self) -> Option<Vec<(String, Dynamic)>> {
        None
    }
}

/// `IMultiDiffSourceResolver`.
pub trait IMultiDiffSourceResolver {
    fn can_handle_uri(&self, uri: &URI) -> bool;
    fn resolve_diff_source(&self, uri: &URI) -> Promise<Rc<dyn IResolvedMultiDiffSource>>;
}

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
