//! Porte de `src/vs/workbench/api/test/common/extHostComments.test.ts` (suíte `ExtHostComments`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado antes do caso; o caso é `async` e roda em
//!   `run_with_faked_timers`. O `ExtHostDocuments` (`{} as ExtHostDocuments` no original, nada construído) é o real,
//!   criado ANTES do guarda, como em `ext_host_comment_threads.rs`. Nenhum método dele é chamado.
//! - `new TestRPCProtocol()` e `rpcProtocol.set(...)` são o `TestMainContext` de `support`.
//! - `new class extends mock<MainThreadCommentsShape>()` com os quatro `override` é o [`MockMainThreadComments`]: os demais
//!   métodos falham com `panic!`, como o método ausente do objeto vazio lançaria `TypeError`.
//! - `new class extends mock<ExtHostCommands>()` com só `registerArgumentProcessor` não tem equivalente de objeto vazio no
//!   porte (`ExtHostCommands` é uma struct, e `create_ext_host_comments` recebe `&ExtHostCommands`): é o `ExtHostCommands`
//!   real, sobre um `MainThreadCommands` cujos métodos falham e um `IExtHostTelemetry` que falha. O único método de
//!   `ExtHostCommands` que o caso usa é `register_argument_processor`, o mesmo do original.
//! - O `extension` (`{ ...nullExtensionDescription, identifier, name, displayName, extensionLocation }`) é o
//!   `null_extension_description()` com os campos trocados; `extensionLocation: URI.file('/extension')` é a URI como texto
//!   (`extension_location` é `String` no porte).
//! - `controllerHandle = -1` inicial é `Option<u32>` (`None` até o registro); `createdThreadCount++` é um `Cell<u32>`.
//! - `controller.dispose()` e `$createCommentThreadTemplate(handle, URI.file('/file'), undefined)` são
//!   `ExtHostCommentController::dispose` e `ExtHostCommentsShape::rpc_create_comment_thread_template`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::path::Platform;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_commands::ExtHostCommands;
use je_workbench::api::common::ext_host_comments::create_ext_host_comments;
use je_workbench::api::common::ext_host_document_data::IExtHostDocumentSaveDelegate;
use je_workbench::api::common::ext_host_documents::ExtHostDocuments;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_protocol::{
    CommandArgs, CommentChanges, CommentProviderFeatures, CommentThreadChanges, CommentThreadRevealOptions, CommentingRangeResourceHint, CreateDocumentOptions,
    ExtHostCommentsShape, IExtHostTelemetry, IMainContext, MainContext, MainThreadCommandsShape, MainThreadCommentsShape, MainThreadDocumentsShape,
    OpenDocumentOptions,
};
use je_workbench::services::extensions::extensions::null_extension_description;
use support::ext_host_workspace_support::TestMainContext;

/// `new class extends mock<MainThreadCommentsShape>() { ... }`: `$registerCommentController`, `$unregisterCommentController`,
/// `$createCommentThread` (que conta) e `$deleteCommentThread`.
struct MockMainThreadComments {
    controller_handle: Rc<RefCell<Option<u32>>>,
    created_thread_count: Rc<Cell<u32>>,
}

impl IDisposable for MockMainThreadComments {
    fn dispose(&self) {}
}

impl MainThreadCommentsShape for MockMainThreadComments {
    fn rpc_register_comment_controller(&self, handle: u32, _id: &str, _label: &str, _extension_id: &str) {
        *self.controller_handle.borrow_mut() = Some(handle);
    }

    fn rpc_unregister_comment_controller(&self, _handle: u32) {}

    fn rpc_update_comment_controller_features(&self, _handle: u32, _features: CommentProviderFeatures) {
        panic!("$updateCommentControllerFeatures is not a function")
    }

    #[allow(clippy::too_many_arguments)]
    fn rpc_create_comment_thread(
        &self,
        _comment_control_handle: u32,
        _comment_thread_handle: u32,
        _thread_id: &str,
        _resource: &URI,
        _range: Option<je_editor::range::Range>,
        _comments: Vec<CommentChanges>,
        _extension_id: &ExtensionIdentifier,
        _is_template: bool,
        _editor_id: Option<&str>,
    ) {
        self.created_thread_count.set(self.created_thread_count.get() + 1);
    }

    fn rpc_update_comment_thread(&self, _handle: u32, _comment_thread_handle: u32, _thread_id: &str, _resource: &URI, _changes: CommentThreadChanges) {
        panic!("$updateCommentThread is not a function")
    }

    fn rpc_delete_comment_thread(&self, _controller: u32, _thread: u32) {}

    fn rpc_update_commenting_ranges(&self, _handle: u32, _resource_hints: Option<CommentingRangeResourceHint>) {
        panic!("$updateCommentingRanges is not a function")
    }

    fn rpc_reveal_comment_thread(&self, _handle: u32, _comment_thread_handle: u32, _comment_unique_id_in_thread: u32, _options: CommentThreadRevealOptions) -> Promise<()> {
        panic!("$revealCommentThread is not a function")
    }

    fn rpc_hide_comment_thread(&self, _handle: u32, _comment_thread_handle: u32) {
        panic!("$hideCommentThread is not a function")
    }
}

/// O `MainThreadCommands` do `ExtHostCommands` real: nenhum método é chamado.
struct MockMainThreadCommands;

impl IDisposable for MockMainThreadCommands {
    fn dispose(&self) {}
}

impl MainThreadCommandsShape for MockMainThreadCommands {
    // O original usa um `ExtHostCommands` `mock`, que não registra nada; o real registra o comando delegante na construção, e o
    // `SingleProxyRPCProtocol(null)` do original aceita qualquer chamada: o dublê ignora as duas.
    fn rpc_register_command(&self, _id: &str) {}

    fn rpc_unregister_command(&self, _id: &str) {}

    fn rpc_fire_command_activation_event(&self, _id: &str) {
        panic!("$fireCommandActivationEvent is not a function")
    }

    fn rpc_execute_command(&self, _id: &str, _args: CommandArgs, _retry: bool) -> Promise<Dynamic> {
        panic!("$executeCommand is not a function")
    }

    fn rpc_get_commands(&self) -> Promise<Vec<String>> {
        panic!("$getCommands is not a function")
    }
}

/// O `IExtHostTelemetry` do `ExtHostCommands` real: nenhum caso chega a `onExtensionError`.
struct MockExtHostTelemetry;

impl IExtHostTelemetry for MockExtHostTelemetry {
    fn on_extension_error(&self, _extension: &ExtensionIdentifier, _error: &Error) -> bool {
        panic!("onExtensionError is not a function")
    }
}

/// O `MainThreadDocuments` que o `ExtHostDocuments` real pede e que o caso não chama.
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

#[test]
fn disposed_comment_controllers_are_removed_from_extension_host_bookkeeping() {
    let rpc = Rc::new(TestMainContext::new());
    rpc.set(&MainContext::MAIN_THREAD_DOCUMENTS, Rc::new(UnusedMainThreadDocuments) as Rc<dyn MainThreadDocumentsShape>);
    let context: Rc<dyn IMainContext> = rpc.clone();
    let documents_and_editors = Rc::new(ExtHostDocumentsAndEditors::new(context, NullLogService::shared()));
    let documents = ExtHostDocuments::new(&*rpc, documents_and_editors);

    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let controller_handle: Rc<RefCell<Option<u32>>> = Rc::new(RefCell::new(None));
        let created_thread_count = Rc::new(Cell::new(0));
        rpc.set(
            &MainContext::MAIN_THREAD_COMMENTS,
            Rc::new(MockMainThreadComments { controller_handle: controller_handle.clone(), created_thread_count: created_thread_count.clone() })
                as Rc<dyn MainThreadCommentsShape>,
        );
        rpc.set(&MainContext::MAIN_THREAD_COMMANDS, Rc::new(MockMainThreadCommands) as Rc<dyn MainThreadCommandsShape>);

        let commands = ExtHostCommands::new(&*rpc, NullLogService::shared(), Rc::new(MockExtHostTelemetry));
        let mut extension = null_extension_description();
        extension.identifier = ExtensionIdentifier::new("test.comments");
        extension.manifest.name = "comments".to_owned();
        extension.manifest.display_name = Some("Comments".to_owned());
        extension.extension_location = URI::file("/extension", Platform::Posix).to_string(false).to_string_lossy();
        let ext_host_comments = create_ext_host_comments(&*rpc, &commands, documents);
        let controller = ext_host_comments.create_comment_controller(&extension, "comments", "Comments");

        controller.dispose();
        let handle = controller_handle.borrow().expect("o controlador foi registrado");
        ext_host_comments.rpc_create_comment_thread_template(handle, &URI::file("/file", Platform::Posix), None, None).await.unwrap();

        assert_eq!(created_thread_count.get(), 0);
    });
}
