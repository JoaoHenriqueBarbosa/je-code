//! Porte de `src/vs/workbench/api/test/common/extHostCustomEditors.test.ts` (suíte `ExtHostCustomEditors document
//! disposal`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; os casos `async`
//!   rodam em `run_with_faked_timers`. O `ExtHostDocuments` do original é `mock<ExtHostDocuments>()` (nada construído);
//!   aqui é o real, criado ANTES do guarda (o `setup` roda antes de o rastreador existir), para os `DisposableStore`
//!   dele não contarem como vazamento.
//! - `SingleProxyRPCProtocol(proxy)` devolve o mesmo objeto a qualquer proxy; o [`IMainContext`] do porte entrega o
//!   proxy pela forma pedida, então o [`TestRpcProtocol`] registra o dublê de `MainThreadCustomEditors` e o de
//!   `MainThreadDocuments` (que falha se for chamado).
//! - `new class extends mock<MainThreadCustomEditorsShape>()` com os três `override` vira [`MainThreadCustomEditors`]:
//!   os três métodos respondem e os demais falham (`panic!`), como o método ausente do objeto vazio lançaria
//!   `TypeError`. `mock<ExtHostWebviews>()` e `mock<ExtHostWebviewPanels>()` são o `ExtHostWebviews` e o
//!   `ExtHostWebviewPanels` reais, sobre o `MockMainThreadWebviewManager` de `support/webview_support.rs` (todo método
//!   falha): nenhum método é chamado nestes casos.
//! - O `CustomDocument` literal (`{ uri, dispose }`, com `uri` fixo ou um getter) é o [`TestDocument`], com a `uri` num
//!   `RefCell` (o getter do último caso) e o `dispose` num fecho que devolve [`Result`] (o lançamento do original).
//! - Os provedores literais são [`CustomProvider`] (`{ openCustomDocument: () => document, resolveCustomEditor: () => {} }`) e
//!   [`TextProvider`] (`{ resolveCustomTextEditor: () => {} }`), escolhidos por [`EditorProvider::Custom`] e
//!   [`EditorProvider::Text`] no lugar do `typeof resolveCustomTextEditor === 'function'` do original.
//! - `assert.rejects(promise, error)` é a comparação do erro da promessa rejeitada com o `error` esperado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/webview_support.rs"]
mod webview_support;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::path::Platform;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::selection::Selection as EditorSelection;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_custom_editors::{
    CustomDocument, CustomEditorRegistrationOptions, CustomDocumentOpenContext, CustomReadonlyEditorProvider, CustomTextEditorProvider, EditorProvider,
    ExtHostCustomEditors,
};
use je_workbench::api::common::ext_host_document_data::{IExtHostDocumentSaveDelegate, TextDocument};
use je_workbench::api::common::ext_host_documents::ExtHostDocuments;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_protocol::{
    CreateDocumentOptions, CreateCustomDocumentResult, CustomEditorProviderCapabilities, ExtHostCustomEditorsShape, IMainContext, MainContext,
    MainThreadCustomEditorsShape, MainThreadDocumentsShape, OpenDocumentOptions, WebviewExtensionDescription, WebviewPanelOptions,
};
use je_workbench::api::common::ext_host_types::Disposable;
use je_workbench::api::common::ext_host_webview::ExtHostWebviews;
use je_workbench::api::common::ext_host_webview_panels::{ExtHostWebviewPanel, ExtHostWebviewPanels};
use je_workbench::contrib::webview::webview_common::WebviewRemoteInfo;
use je_workbench::services::extensions::extensions::null_extension_description;
use webview_support::{MockMainThreadWebviewManager, NullApiDeprecationService};

/// `TestRPCProtocol`: os proxies são os objetos registrados por `set`.
#[derive(Default)]
struct TestRpcProtocol {
    objects: RefCell<HashMap<&'static str, Rc<dyn Any>>>,
}

impl IMainContext for TestRpcProtocol {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        self.objects.borrow().get(sid).cloned().unwrap_or_else(|| panic!("o proxy '{sid}' não foi registrado"))
    }

    fn set_erased(&self, sid: &'static str, value: Rc<dyn Any>) {
        self.objects.borrow_mut().insert(sid, value);
    }
}

/// O `MainThreadDocuments` que nenhum método do caso chama.
struct UnusedMainThreadDocuments;

impl IDisposable for UnusedMainThreadDocuments {
    fn dispose(&self) {}
}

impl IExtHostDocumentSaveDelegate for UnusedMainThreadDocuments {
    fn rpc_try_save_document(&self, _uri: &URI) -> Promise<bool> {
        panic!("o proxy não deveria ser chamado");
    }
}

impl MainThreadDocumentsShape for UnusedMainThreadDocuments {
    fn rpc_try_create_document(&self, _options: Option<CreateDocumentOptions>) -> Promise<URI> {
        panic!("o proxy não deveria ser chamado");
    }

    fn rpc_try_open_document(&self, _uri: &URI, _options: Option<OpenDocumentOptions>) -> Promise<URI> {
        panic!("o proxy não deveria ser chamado");
    }
}

/// `new class extends mock<MainThreadCustomEditorsShape>()` com `$registerCustomEditorProvider`,
/// `$registerTextEditorProvider` e `$unregisterEditorProvider` sobrescritos.
struct MainThreadCustomEditors;

impl IDisposable for MainThreadCustomEditors {
    fn dispose(&self) {}
}

impl MainThreadCustomEditorsShape for MainThreadCustomEditors {
    fn rpc_on_did_change_custom_text_editor_selection(&self, _handle: &str, _selection: Option<EditorSelection>) {
        panic!("o proxy não deveria ser chamado");
    }

    fn rpc_register_text_editor_provider(
        &self,
        _extension: WebviewExtensionDescription,
        _view_type: &str,
        _options: WebviewPanelOptions,
        _capabilities: CustomEditorProviderCapabilities,
        _serialize_buffers_for_post_message: bool,
    ) {
    }

    fn rpc_register_custom_editor_provider(
        &self,
        _extension: WebviewExtensionDescription,
        _view_type: &str,
        _options: WebviewPanelOptions,
        _capabilities: CustomEditorProviderCapabilities,
        _supports_multiple_editors_per_document: bool,
        _serialize_buffers_for_post_message: bool,
    ) {
    }

    fn rpc_unregister_editor_provider(&self, _view_type: &str) {}

    fn rpc_on_did_edit(&self, _resource: &URI, _view_type: &str, _edit_id: u32, _label: Option<&str>) {
        panic!("o proxy não deveria ser chamado");
    }

    fn rpc_on_content_change(&self, _resource: &URI, _view_type: &str) {
        panic!("o proxy não deveria ser chamado");
    }
}

/// O `setup`: `new ExtHostCustomEditors(SingleProxyRPCProtocol(proxy), documents, undefined, webviews, panels)`. Os
/// `ExtHostWebviews` e `ExtHostWebviewPanels` (o `mock<...>()` do original) são os reais, sobre um
/// `MainThreadWebviewManager` cujo método não é chamado em nenhum caso.
fn setup() -> ExtHostCustomEditors {
    let rpc: Rc<dyn IMainContext> = Rc::new(TestRpcProtocol::default());
    let proxy: Rc<dyn MainThreadCustomEditorsShape> = Rc::new(MainThreadCustomEditors);
    rpc.set(&MainContext::MAIN_THREAD_CUSTOM_EDITORS, proxy);
    let documents_proxy: Rc<dyn MainThreadDocumentsShape> = Rc::new(UnusedMainThreadDocuments);
    rpc.set(&MainContext::MAIN_THREAD_DOCUMENTS, documents_proxy);
    Rc::new(MockMainThreadWebviewManager::default()).register_in(&*rpc);

    let documents_and_editors = Rc::new(ExtHostDocumentsAndEditors::new(rpc.clone(), NullLogService::shared()));
    let documents = ExtHostDocuments::new(&*rpc, documents_and_editors);
    let webviews = ExtHostWebviews::new(&*rpc, WebviewRemoteInfo { is_remote: false, authority: None }, None, NullLogService::shared(), Rc::new(NullApiDeprecationService));
    let panels = ExtHostWebviewPanels::new(&*rpc, webviews.clone(), None);
    ExtHostCustomEditors::new(&*rpc, documents, None, webviews, panels)
}

const VIEW_TYPE: &str = "custom-document-test";

/// `URI.file('/test/document')`.
fn resource() -> URI {
    URI::file("/test/document", Platform::Posix)
}

/// O `vscode.CustomDocument` literal: `uri` (fixo ou lido de um getter) e `dispose`.
struct TestDocument {
    uri: RefCell<URI>,
    on_dispose: Box<dyn Fn() -> Result<(), Error>>,
}

impl TestDocument {
    fn new(uri: URI, on_dispose: impl Fn() -> Result<(), Error> + 'static) -> Rc<Self> {
        Rc::new(Self { uri: RefCell::new(uri), on_dispose: Box::new(on_dispose) })
    }
}

impl CustomDocument for TestDocument {
    fn uri(&self) -> URI {
        self.uri.borrow().clone()
    }

    fn dispose(&self) -> Result<(), Error> {
        (self.on_dispose)()
    }
}

/// `{ openCustomDocument: () => document, resolveCustomEditor: () => { } }`.
struct CustomProvider {
    document: Rc<dyn CustomDocument>,
}

impl CustomReadonlyEditorProvider for CustomProvider {
    fn open_custom_document(&self, _uri: &URI, _open_context: CustomDocumentOpenContext, _token: CancellationToken) -> MaybePromise<Rc<dyn CustomDocument>> {
        MaybePromise::Value(self.document.clone())
    }

    fn resolve_custom_editor(&self, _document: Rc<dyn CustomDocument>, _panel: Rc<ExtHostWebviewPanel>, _token: CancellationToken) -> MaybePromise<()> {
        MaybePromise::Value(())
    }
}

/// `{ resolveCustomTextEditor: () => { } }`.
struct TextProvider;

impl CustomTextEditorProvider for TextProvider {
    fn resolve_custom_text_editor(&self, _document: TextDocument, _panel: Rc<ExtHostWebviewPanel>, _token: CancellationToken) -> MaybePromise<()> {
        MaybePromise::Value(())
    }
}

/// O `register(document)` do original: registra o provedor de `openCustomDocument: () => document`, guardando o
/// registro para o guarda descartar ao fim.
fn register(editors: &ExtHostCustomEditors, store: &Rc<DisposableStore>, document: Rc<dyn CustomDocument>) -> Rc<Disposable> {
    let registration = editors
        .register_custom_editor_provider(
            &null_extension_description(),
            VIEW_TYPE,
            EditorProvider::Custom(Rc::new(CustomProvider { document })),
            CustomEditorRegistrationOptions::default(),
        )
        .unwrap();
    store.add_rc(Rc::new(registration))
}

/// O `open()` do original.
fn open(editors: &ExtHostCustomEditors) -> Promise<CreateCustomDocumentResult> {
    editors.rpc_create_custom_document(&resource(), VIEW_TYPE, None, None, CancellationToken::None)
}

#[test]
fn disposes_the_document_after_its_provider_has_been_unregistered() {
    let editors = setup();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let disposed = Rc::new(Cell::new(0));
        let registration = register(&editors, &store, TestDocument::new(resource(), {
            let disposed = disposed.clone();
            move || {
                disposed.set(disposed.get() + 1);
                Ok(())
            }
        }));
        open(&editors).await.unwrap();
        registration.dispose();
        editors.rpc_dispose_custom_document(&resource(), VIEW_TYPE).await.unwrap();
        assert_eq!(disposed.get(), 1);
    });
}

#[test]
fn removes_the_old_document_so_a_replacement_provider_can_reopen_its_resource() {
    let editors = setup();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let disposed: Rc<RefCell<Vec<&'static str>>> = Rc::default();
        let registration = register(&editors, &store, TestDocument::new(resource(), {
            let disposed = disposed.clone();
            move || {
                disposed.borrow_mut().push("old");
                Ok(())
            }
        }));
        open(&editors).await.unwrap();
        registration.dispose();
        editors.rpc_dispose_custom_document(&resource(), VIEW_TYPE).await.unwrap();
        register(&editors, &store, TestDocument::new(resource(), {
            let disposed = disposed.clone();
            move || {
                disposed.borrow_mut().push("new");
                Ok(())
            }
        }));
        open(&editors).await.unwrap();
        editors.rpc_dispose_custom_document(&resource(), VIEW_TYPE).await.unwrap();
        assert_eq!(*disposed.borrow(), vec!["old", "new"]);
    });
}

#[test]
fn does_not_require_a_replacement_provider_to_be_a_custom_document_provider() {
    let editors = setup();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let disposed = Rc::new(Cell::new(0));
        let registration = register(&editors, &store, TestDocument::new(resource(), {
            let disposed = disposed.clone();
            move || {
                disposed.set(disposed.get() + 1);
                Ok(())
            }
        }));
        open(&editors).await.unwrap();
        registration.dispose();
        let replacement = editors
            .register_custom_editor_provider(&null_extension_description(), VIEW_TYPE, EditorProvider::Text(Rc::new(TextProvider)), CustomEditorRegistrationOptions::default())
            .unwrap();
        store.add_rc(Rc::new(replacement));
        editors.rpc_dispose_custom_document(&resource(), VIEW_TYPE).await.unwrap();
        assert_eq!(disposed.get(), 1);
    });
}

#[test]
fn still_disposes_documents_while_their_provider_is_registered() {
    let editors = setup();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let disposed = Rc::new(Cell::new(0));
        register(&editors, &store, TestDocument::new(resource(), {
            let disposed = disposed.clone();
            move || {
                disposed.set(disposed.get() + 1);
                Ok(())
            }
        }));
        open(&editors).await.unwrap();
        editors.rpc_dispose_custom_document(&resource(), VIEW_TYPE).await.unwrap();
        assert_eq!(disposed.get(), 1);
    });
}

#[test]
fn removes_the_entry_before_invoking_a_throwing_document_dispose_callback() {
    let editors = setup();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let error = Error::new("expected document disposal failure");
        let registration = register(&editors, &store, TestDocument::new(resource(), {
            let error = error.clone();
            move || Err(error.clone())
        }));
        open(&editors).await.unwrap();
        registration.dispose();
        let rejection = editors.rpc_dispose_custom_document(&resource(), VIEW_TYPE).await;
        assert_eq!(rejection.unwrap_err(), error);
        register(&editors, &store, TestDocument::new(resource(), || Ok(())));
        open(&editors).await.unwrap();
        editors.rpc_dispose_custom_document(&resource(), VIEW_TYPE).await.unwrap();
    });
}

#[test]
fn uses_the_stored_resource_key_when_the_document_uri_has_changed() {
    let editors = setup();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let disposed = Rc::new(Cell::new(0));
        let document = TestDocument::new(resource(), {
            let disposed = disposed.clone();
            move || {
                disposed.set(disposed.get() + 1);
                Ok(())
            }
        });
        let registration = register(&editors, &store, document.clone());
        open(&editors).await.unwrap();
        *document.uri.borrow_mut() = URI::file("/test/renamed", Platform::Posix);
        registration.dispose();
        editors.rpc_dispose_custom_document(&resource(), VIEW_TYPE).await.unwrap();
        register(&editors, &store, TestDocument::new(resource(), {
            let disposed = disposed.clone();
            move || {
                disposed.set(disposed.get() + 1);
                Ok(())
            }
        }));
        open(&editors).await.unwrap();
        editors.rpc_dispose_custom_document(&resource(), VIEW_TYPE).await.unwrap();
        assert_eq!(disposed.get(), 2);
    });
}
