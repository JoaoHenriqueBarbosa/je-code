//! Porte de `src/vs/workbench/contrib/multiDiffEditor/browser/multiDiffSourceResolverService.ts`: o serviço que escolhe o resolvedor
//! da fonte de um editor de diff de vários arquivos (`IMultiDiffSourceResolverService` e `MultiDiffSourceResolverService`), o
//! `MultiDiffEditorItem` (o `NotebookMultiDiffEditorItem` o estende) e os contratos `IMultiDiffSourceResolver` e
//! `IResolvedMultiDiffSource` que os resolvedores do SCM implementam.
//!
//! Divergências:
//! - `IResolvedMultiDiffSource.resources: IValueWithChangeEvent<readonly MultiDiffEditorItem[]>` é um
//!   [`MultiDiffResources`], cujo valor é um `Rc<Vec<MultiDiffEditorItem>>` (a lista é imutável e compartilhada, e a identidade
//!   do `Rc` faz o papel da identidade do array nos observáveis). `label?: IValueWithChangeEvent<string>` é
//!   [`IResolvedMultiDiffSource::label`] (`None` por padrão) e `contextKeys` é [`IResolvedMultiDiffSource::context_keys`].
//! - `contextKeys?: Record<string, ContextKeyValue>` é uma lista de pares `(chave, Dynamic)` na ordem de inserção; o
//!   `ContextKeyValue` é um [`Dynamic`] (como em `EditorGroupContextKeyProvider`).
//! - `BugIndicatingError('Invalid arguments')` do construtor é o `errors::throw` com o erro de bug, e o `Duplicate resolver` do
//!   `registerResolver` também.
//! - `_resolvers = new Set<IMultiDiffSourceResolver>()` é uma lista em ordem de inserção, comparada por identidade (`Rc::ptr_eq`),
//!   compartilhada com o descarte do registro (que não prende o serviço).
//! - `resolve(uri)` devolve `Promise<IResolvedMultiDiffSource | undefined>`; a promessa do resolvedor (`Promise<IResolvedMultiDiffSource>`)
//!   é adaptada para `Option` com `then(Some)`, e a ausência de resolvedor é `Promise.resolve(undefined)`.
//! - O `registerSingleton(IMultiDiffSourceResolverService, ...)` mora em `multiDiffEditor.contribution.ts` (não portado); o
//!   serviço é [`Injectable`] e implementa [`IMultiDiffSourceResolverService`], pronto para esse registro.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{self, Error};
use je_base::event::ValueWithChangeEventLike;
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::{Injectable, service_id, service_impl};

/// `IValueWithChangeEvent<readonly MultiDiffEditorItem[]>`.
pub type MultiDiffResources = Rc<dyn ValueWithChangeEventLike<Rc<Vec<MultiDiffEditorItem>>>>;

/// `IMultiDiffSourceResolverService`.
pub trait IMultiDiffSourceResolverService {
    fn register_resolver(&self, resolver: Rc<dyn IMultiDiffSourceResolver>) -> DisposableRef;

    fn resolve(&self, uri: &URI) -> Promise<Option<Rc<dyn IResolvedMultiDiffSource>>>;
}

service_id!(pub I_MULTI_DIFF_SOURCE_RESOLVER_SERVICE: dyn IMultiDiffSourceResolverService = "multiDiffSourceResolverService");

/// `IResolvedMultiDiffSource`.
pub trait IResolvedMultiDiffSource {
    fn resources(&self) -> MultiDiffResources;

    /// `label?: IValueWithChangeEvent<string>`.
    fn label(&self) -> Option<Rc<dyn ValueWithChangeEventLike<String>>> {
        None
    }

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

/// `MultiDiffSourceResolverService`.
#[derive(Default)]
pub struct MultiDiffSourceResolverService {
    resolvers: Rc<RefCell<Vec<Rc<dyn IMultiDiffSourceResolver>>>>,
}

impl Injectable for MultiDiffSourceResolverService {
    type Args = ();
    type Deps = ();

    fn create(_: (), _: ()) -> Rc<Self> {
        Rc::new(Self::default())
    }
}

service_impl!(MultiDiffSourceResolverService => dyn IMultiDiffSourceResolverService);

impl IMultiDiffSourceResolverService for MultiDiffSourceResolverService {
    fn register_resolver(&self, resolver: Rc<dyn IMultiDiffSourceResolver>) -> DisposableRef {
        // throw on duplicate
        if self.resolvers.borrow().iter().any(|registered| Rc::ptr_eq(registered, &resolver)) {
            errors::throw(Error::bug_indicating(Some("Duplicate resolver")));
        }
        self.resolvers.borrow_mut().push(resolver.clone());
        let resolvers = self.resolvers.clone();
        to_disposable(move || resolvers.borrow_mut().retain(|registered| !Rc::ptr_eq(registered, &resolver)))
    }

    fn resolve(&self, uri: &URI) -> Promise<Option<Rc<dyn IResolvedMultiDiffSource>>> {
        let resolvers: Vec<Rc<dyn IMultiDiffSourceResolver>> = self.resolvers.borrow().clone();
        for resolver in resolvers {
            if resolver.can_handle_uri(uri) {
                return resolver.resolve_diff_source(uri).then(Some);
            }
        }
        Promise::resolved(None)
    }
}
