//! Porte de `src/vs/workbench/contrib/editTelemetry/browser/helpers/annotatedDocuments.ts`.
//!
//! Representação:
//! - `IObservable<readonly AnnotatedDocument[]>` é `IObservable<Rc<Vec<Rc<AnnotatedDocument>>>>` (o array é um
//!   objeto do JavaScript, comparado por identidade, como o `Rc`);
//! - `createInstance(UriVisibilityProvider)` do construtor do `AnnotatedDocuments` é a busca do serviço
//!   [`I_URI_VISIBILITY_PROVIDER`] (o ponto em que os testes do original trocam a classe por
//!   `stubInstance(UriVisibilityProvider, ...)`) e, se ele não estiver registrado, a criação do
//!   [`UriVisibilityProvider`] de verdade;
//! - o `mapObservableArrayCached` sobre `IObservableDocument[]` usa a identidade do documento como chave (o item do
//!   original é o próprio objeto), e sobre os grupos usa o `id` do grupo.

use std::collections::HashMap;
use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::{
    IObservable, IReader, Observable, ObservableExt, derived, derived_observable_with_cache, map_observable_array_cached_by, observable_from_event,
    observable_signal_from_event, recompute_initially_and_on_change,
};
use je_base::uri::URI;
use je_instantiation::{Injectable, InstantiationService, service_id};

use super::document_with_annotated_edits::{CombineStreamedChanges, DocumentWithSourceAnnotatedEdits, EditSourceData, IDocumentWithAnnotatedEdits, MinimizeEditsProcessor};
use super::observable_workspace::{IObservableDocument, ObservableWorkspace};
use crate::common::editor::{EditorInputOrUntyped, EditorResourceAccessor};
use crate::services::editor::editor_groups_service::{IEditorGroup, IEditorGroupsService};

/// `IAnnotatedDocuments`.
pub trait IAnnotatedDocuments {
    fn documents(&self) -> IObservable<Rc<Vec<Rc<AnnotatedDocument>>>>;
}

/// `AnnotatedDocuments`.
pub struct AnnotatedDocuments {
    base: Rc<Disposable>,
    documents: IObservable<Rc<Vec<Rc<AnnotatedDocument>>>>,
}

impl Injectable for AnnotatedDocuments {
    type Args = Rc<dyn ObservableWorkspace>;
    type Deps = (Rc<InstantiationService>,);

    fn create(workspace: Self::Args, (instantiation_service,): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();

        let uri_visibility_provider: Rc<dyn IUriVisibilityProvider> = instantiation_service
            .invoke_function(|accessor| accessor.try_get(I_URI_VISIBILITY_PROVIDER))
            .unwrap_or_else(|| instantiation_service.create_instance::<UriVisibilityProvider>(()));

        // `workspace.documents` como um array de documentos (o `Rc<[..]>` do porte não é o tipo que o mapa em cache lê).
        let documents_as_vec: IObservable<Rc<Vec<Rc<dyn IObservableDocument>>>> = workspace.documents().map(|docs, _| Rc::new(docs.iter().cloned().collect::<Vec<_>>()));

        let states = map_observable_array_cached_by(
            None,
            documents_as_vec,
            move |doc: &Rc<dyn IObservableDocument>, store| {
                let provider = uri_visibility_provider.clone();
                let uri = doc.uri().clone();
                let doc_is_visible: IObservable<bool> = derived(move |reader| provider.is_visible(&uri, reader));
                let visible_for_cache = doc_is_visible.clone();
                let was_ever_visible: IObservable<bool> =
                    derived_observable_with_cache::<bool>(None, move |reader, last_val| last_val.copied().unwrap_or(false) || visible_for_cache.read(reader));
                let doc = doc.clone();
                let store = store.clone();
                let insta = instantiation_service.clone();
                was_ever_visible.map(move |v, _| {
                    if v {
                        Some(store.add_rc(insta.create_instance::<AnnotatedDocument>((doc.clone(), doc_is_visible.clone()))))
                    } else {
                        None
                    }
                })
            },
            |doc| Rc::as_ptr(doc).cast::<()>() as usize,
        );

        let documents: IObservable<Rc<Vec<Rc<AnnotatedDocument>>>> =
            states.map(|vals, reader| Rc::new(vals.iter().filter_map(|v| v.read(reader)).collect::<Vec<_>>()));

        base.register(recompute_initially_and_on_change(&documents, None));

        Rc::new(Self { base, documents })
    }
}

impl IAnnotatedDocuments for AnnotatedDocuments {
    fn documents(&self) -> IObservable<Rc<Vec<Rc<AnnotatedDocument>>>> {
        self.documents.clone()
    }
}

impl IDisposable for AnnotatedDocuments {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O que o `AnnotatedDocuments` lê do `UriVisibilityProvider`.
pub trait IUriVisibilityProvider {
    /// `isVisible(uri, reader)`.
    fn is_visible(&self, uri: &URI, reader: &dyn IReader) -> bool;
}

service_id!(pub I_URI_VISIBILITY_PROVIDER: dyn IUriVisibilityProvider = "uriVisibilityProvider");

/// `UriVisibilityProvider`.
pub struct UriVisibilityProvider {
    visible_uris: IObservable<Rc<HashMap<String, URI>>>,
}

impl Injectable for UriVisibilityProvider {
    type Args = ();
    type Deps = (Rc<dyn IEditorGroupsService>,);

    fn create(_: (), (editor_groups_service,): Self::Deps) -> Rc<Self> {
        let on_did_add_group_signal = observable_signal_from_event("onDidAddGroup", editor_groups_service.on_did_add_group());
        let on_did_remove_group_signal = observable_signal_from_event("onDidRemoveGroup", editor_groups_service.on_did_remove_group());
        let groups_service = editor_groups_service.clone();
        let groups: IObservable<Rc<Vec<Rc<dyn IEditorGroup>>>> = derived(move |reader| {
            on_did_add_group_signal.read(reader);
            on_did_remove_group_signal.read(reader);
            Rc::new(groups_service.groups())
        });

        let visible_uris = map_observable_array_cached_by(
            None,
            groups,
            |g: &Rc<dyn IEditorGroup>, _store| {
                let group = g.clone();
                let editors = observable_from_event(g.on_did_model_change(), move |_| Rc::new(group.editors()));
                editors.map(|e, _| {
                    Rc::new(
                        e.iter()
                            .map(|editor| EditorResourceAccessor::get_canonical_uri(Some(&EditorInputOrUntyped::Typed(editor.clone())), None))
                            .collect::<Vec<Option<URI>>>(),
                    )
                })
            },
            |g| g.id(),
        )
        .map(|editors, reader| {
            let mut map = HashMap::new();
            for uris_obs in editors.iter() {
                for uri in uris_obs.read(reader).iter().flatten() {
                    map.insert(uri.to_string(false).to_string(), uri.clone());
                }
            }
            Rc::new(map)
        });

        Rc::new(Self { visible_uris })
    }
}

impl IUriVisibilityProvider for UriVisibilityProvider {
    fn is_visible(&self, uri: &URI, reader: &dyn IReader) -> bool {
        self.visible_uris.read(reader).contains_key(&uri.to_string(false).to_string())
    }
}

/// `AnnotatedDocument`.
pub struct AnnotatedDocument {
    base: Rc<Disposable>,
    pub document: Rc<dyn IObservableDocument>,
    pub is_visible: IObservable<bool>,
    pub document_with_annotations: Rc<dyn IDocumentWithAnnotatedEdits<EditSourceData>>,
}

impl Injectable for AnnotatedDocument {
    type Args = (Rc<dyn IObservableDocument>, IObservable<bool>);
    type Deps = (Rc<InstantiationService>,);

    fn create((document, is_visible): Self::Args, (instantiation_service,): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();

        let source_annotated = DocumentWithSourceAnnotatedEdits::new(&document);
        base.register(source_annotated.clone());
        let mut processed_doc: Rc<dyn IDocumentWithAnnotatedEdits<EditSourceData>> = source_annotated;
        // Combine streaming edits into one and make edit smaller
        let combined = instantiation_service.create_instance::<CombineStreamedChanges<EditSourceData>>(processed_doc);
        base.register(combined.clone());
        processed_doc = combined;
        // Remove common suffix and prefix from edits
        let minimized = MinimizeEditsProcessor::new(processed_doc);
        base.register(minimized.clone());
        processed_doc = minimized;

        Rc::new(Self { base, document, is_visible, document_with_annotations: processed_doc })
    }
}

impl IDisposable for AnnotatedDocument {
    fn dispose(&self) {
        self.base.dispose();
    }
}
