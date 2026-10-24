//! Porte de `src/vs/workbench/api/browser/mainThreadNotebookDocuments.ts`: o `MainThreadNotebookDocuments`, o lado do workbench
//! do RPC `ExtHostNotebookDocuments`: repassa ao host de extensões as mudanças de conteúdo, o estado de sujo e os salvamentos dos
//! notebooks, e atende `$tryCreateNotebook`, `$tryOpenNotebook` e `$trySaveNotebook`.
//!
//! Divergências:
//! - **Injeção.** O construtor `(extHostContext, @INotebookEditorModelResolverService, @IUriIdentityService)` é
//!   [`MainThreadNotebookDocuments::new`] com os três argumentos na ordem do original (o `IExtHostContext` é o [`IMainContext`]).
//!   O `NotebookTextModel` é o `Rc<NotebookTextModel>` concreto.
//! - **`BoundModelReferenceCollection`.** É o de `main_thread_documents`, com a [`DisposableRef`] da referência (`IReference<unknown>`
//!   só tem o `dispose` lido).
//! - **`IReference` do resolvedor.** É o [`Reference`](je_base::lifecycle::Reference) (o descarte é de chamada única e a cópia o
//!   compartilha), e o `Event.once(notebook.onWillDispose)(() => ref.dispose())` é um [`Event::once`] cujo ouvinte guarda uma cópia
//!   da referência. O `onWillDispose` de `$tryOpenNotebook` (sem `once`) é um ouvinte comum.
//! - **Eventos de mudança.** Os eventos `ChangeCellMime`, `ChangeDocumentMetadata`, `Initialize` e `Unknown` não vão ao host por
//!   esta via (o `switch` do original não os trata); o de metadados do documento só decide se o `newMetadata` vai junto. Cada
//!   evento vira o [`NotebookRawContentEventDto`] do protocolo (a união discriminada é o `enum`).
//! - **Métodos `async`** são [`async_fn`], que roda de forma síncrona até o primeiro `await`, como o original.
//! - `isDirty(textModel.uri)` é lido no momento do evento (o comentário do original sobre o primeiro ouvinte vale igual).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::map::ResourceMap;
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::api::browser::main_thread_documents::{BoundModelReferenceCollection, BoundModelReferenceLimits};
use crate::api::browser::main_thread_notebook_dto::{from_notebook_data_dto, to_notebook_cell_dto, to_notebook_output_dto, to_notebook_output_item_dto};
use crate::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostNotebookDocumentsShape, IMainContext, MainThreadNotebookDocumentsShape, NotebookCellsChangedEventDto, NotebookCreateOptions,
    NotebookRawContentEventDto, SerializableObjectWithBuffers,
};
use crate::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use crate::contrib::notebook::common::notebook_common::NotebookRawContentChange;
use crate::contrib::notebook::common::notebook_editor_model_resolver_service::{INotebookEditorModelResolverService, IUntitledNotebookResource};

/// `MainThreadNotebookDocuments`.
pub struct MainThreadNotebookDocuments {
    base: Rc<Disposable>,
    proxy: Rc<dyn ExtHostNotebookDocumentsShape>,
    document_event_listeners_mapping: RefCell<ResourceMap<Rc<DisposableStore>>>,
    model_reference_collection: Rc<BoundModelReferenceCollection>,
    notebook_editor_model_resolver_service: Rc<dyn INotebookEditorModelResolverService>,
}

impl MainThreadNotebookDocuments {
    pub fn new(
        ext_host_context: &Rc<dyn IMainContext>,
        notebook_editor_model_resolver_service: Rc<dyn INotebookEditorModelResolverService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
    ) -> Rc<Self> {
        let proxy = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_NOTEBOOK_DOCUMENTS);
        let model_reference_collection = Rc::new(BoundModelReferenceCollection::new(uri_identity_service.ext_uri(), BoundModelReferenceLimits::default()));

        // forward dirty and save events
        let base = Disposable::new();
        base.register(notebook_editor_model_resolver_service.on_did_change_dirty().listen({
            let proxy = proxy.clone();
            move |model| proxy.rpc_accept_dirty_state_changed(&model.resource(), model.is_dirty())
        }));
        base.register(notebook_editor_model_resolver_service.on_did_save_notebook().listen({
            let proxy = proxy.clone();
            move |resource| proxy.rpc_accept_model_saved(resource)
        }));

        // when a conflict is going to happen RELEASE references that are held by extensions
        base.register(notebook_editor_model_resolver_service.on_will_fail_with_conflict().listen({
            let model_reference_collection = model_reference_collection.clone();
            move |event| model_reference_collection.remove(&event.data.resource)
        }));

        Rc::new(Self {
            base,
            proxy,
            document_event_listeners_mapping: RefCell::new(ResourceMap::new()),
            model_reference_collection,
            notebook_editor_model_resolver_service,
        })
    }

    /// `handleNotebooksAdded(notebooks)`.
    pub fn handle_notebooks_added(&self, notebooks: &[Rc<NotebookTextModel>]) {
        for text_model in notebooks {
            let disposable_store = DisposableStore::new();
            let (proxy, resolver, model) = (self.proxy.clone(), self.notebook_editor_model_resolver_service.clone(), text_model.clone());
            disposable_store.add(text_model.on_did_change_content().listen(move |event| {
                let mut event_dto = NotebookCellsChangedEventDto { version_id: event.version_id, raw_events: Vec::new() };

                for raw_event in &event.raw_events {
                    match &raw_event.event {
                        NotebookRawContentChange::ModelChange(change) => {
                            event_dto.raw_events.push(NotebookRawContentEventDto::ModelChange {
                                changes: change
                                    .changes
                                    .iter()
                                    .map(|(start, delete_count, cells)| (*start, *delete_count, cells.iter().map(|cell| to_notebook_cell_dto(&**cell)).collect()))
                                    .collect(),
                            });
                        }
                        NotebookRawContentChange::Move(change) => {
                            event_dto.raw_events.push(NotebookRawContentEventDto::Move { index: change.index, length: change.length, new_idx: change.new_idx });
                        }
                        NotebookRawContentChange::Output(change) => {
                            event_dto.raw_events.push(NotebookRawContentEventDto::Output { index: change.index, outputs: change.outputs.iter().map(to_notebook_output_dto).collect() });
                        }
                        NotebookRawContentChange::OutputItem(change) => {
                            event_dto.raw_events.push(NotebookRawContentEventDto::OutputItem {
                                index: change.index,
                                output_id: change.output_id.clone(),
                                output_items: change.output_items.iter().map(to_notebook_output_item_dto).collect(),
                                append: change.append,
                            });
                        }
                        NotebookRawContentChange::ChangeCellLanguage(change) => {
                            event_dto.raw_events.push(NotebookRawContentEventDto::ChangeCellLanguage { index: change.index, language: change.language.clone() });
                        }
                        NotebookRawContentChange::ChangeCellContent(change) => {
                            event_dto.raw_events.push(NotebookRawContentEventDto::ChangeCellContent { index: change.index });
                        }
                        NotebookRawContentChange::ChangeCellMetadata(change) => {
                            event_dto.raw_events.push(NotebookRawContentEventDto::ChangeCellMetadata { index: change.index, metadata: change.metadata.clone() });
                        }
                        NotebookRawContentChange::ChangeCellInternalMetadata(change) => {
                            event_dto.raw_events.push(NotebookRawContentEventDto::ChangeCellInternalMetadata {
                                index: change.index,
                                internal_metadata: change.internal_metadata.clone(),
                            });
                        }
                        NotebookRawContentChange::Initialize(_)
                        | NotebookRawContentChange::ChangeDocumentMetadata(_)
                        | NotebookRawContentChange::ChangeCellMime(_)
                        | NotebookRawContentChange::Unknown(_) => {}
                    }
                }

                let has_document_metadata_change_event = event.raw_events.iter().any(|raw_event| matches!(raw_event.event, NotebookRawContentChange::ChangeDocumentMetadata(_)));

                // using the model resolver service to know if the model is dirty or not.
                // assuming this is the first listener it can mean that at first the model
                // is marked as dirty and that another event is fired
                proxy.rpc_accept_model_changed(
                    &model.uri(),
                    SerializableObjectWithBuffers::new(event_dto),
                    resolver.is_dirty(&model.uri()),
                    has_document_metadata_change_event.then(|| model.metadata()),
                );
            }));

            self.document_event_listeners_mapping.borrow_mut().set(text_model.uri(), disposable_store);
        }
    }

    /// `handleNotebooksRemoved(uris)`.
    pub fn handle_notebooks_removed(&self, uris: &[URI]) {
        for uri in uris {
            let listeners = self.document_event_listeners_mapping.borrow().get(uri).cloned();
            if let Some(listeners) = listeners {
                listeners.dispose();
            }
            self.document_event_listeners_mapping.borrow_mut().delete(uri);
        }
    }
}

impl IDisposable for MainThreadNotebookDocuments {
    fn dispose(&self) {
        self.base.dispose();
        self.model_reference_collection.dispose();
        let listeners: Vec<Rc<DisposableStore>> = self.document_event_listeners_mapping.borrow().values().cloned().collect();
        for listener in listeners {
            listener.dispose();
        }
    }
}

impl MainThreadNotebookDocumentsShape for MainThreadNotebookDocuments {
    fn rpc_try_create_notebook(&self, options: NotebookCreateOptions) -> Promise<URI> {
        let (resolver, proxy) = (self.notebook_editor_model_resolver_service.clone(), self.proxy.clone());
        async_fn(async move {
            if let Some(content) = options.content {
                let reference = resolver.resolve_untitled(&IUntitledNotebookResource { untitled_resource: None }, &options.view_type, None).await?;
                let notebook = reference.object.resolved_notebook();

                // untitled notebooks are disposed when they get saved. we should not hold a reference
                // to such a disposed notebook and therefore dispose the reference as well
                let release = reference.clone();
                let _ = notebook.on_will_dispose().once().listen(move |_| release.dispose());

                // untitled notebooks with content are dirty by default
                proxy.rpc_accept_dirty_state_changed(&reference.object.resource(), true);

                // apply content changes... slightly HACKY -> this triggers a change event
                let data = from_notebook_data_dto(&content);
                notebook.reset(data.cells, data.metadata, notebook.transient_options());
                Ok(notebook.uri())
            } else {
                // If we aren't adding content, we don't need to resolve the full editor model yet.
                // This will allow us to adjust settings when the editor is opened, e.g. scratchpad
                let notebook = resolver.create_untitled_notebook_text_model(&options.view_type).await?;
                Ok(notebook.uri())
            }
        })
    }

    fn rpc_try_open_notebook(&self, uri: &URI) -> Promise<URI> {
        let uri = uri.clone();
        let (resolver, model_reference_collection) = (self.notebook_editor_model_resolver_service.clone(), self.model_reference_collection.clone());
        async_fn(async move {
            let reference = resolver.resolve(&uri, None, None).await?;

            if uri.scheme() == schemas::UNTITLED {
                // untitled notebooks are disposed when they get saved. we should not hold a reference
                // to such a disposed notebook and therefore dispose the reference as well
                let release = reference.clone();
                let _ = reference.object.resolved_notebook().on_will_dispose().listen(move |_| release.dispose());
            }

            let held: DisposableRef = Rc::new(reference);
            model_reference_collection.add(uri.clone(), held, 0);
            Ok(uri)
        })
    }

    fn rpc_try_save_notebook(&self, uri: &URI) -> Promise<bool> {
        let uri = uri.clone();
        let resolver = self.notebook_editor_model_resolver_service.clone();
        async_fn(async move {
            let reference = resolver.resolve(&uri, None, None).await?;
            let save_result = reference.object.save(None).await?;
            reference.dispose();
            Ok(save_result)
        })
    }
}
