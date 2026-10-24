//! Porte de `src/vs/workbench/api/test/common/extHostCommentThreads.test.ts` (suíte `ExtHostCommentThreads`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado em [`run_test`]; os casos são `async` e rodam em
//!   `run_with_faked_timers`. O `ExtHostDocuments` do original é `mock<ExtHostDocuments>()` (nada construído); aqui é o
//!   real, criado ANTES do guarda (o `setup` do original roda antes de o rastreador existir), para os `DisposableStore`
//!   dele não contarem como vazamento. Nenhum caso o usa.
//! - `new TestRPCProtocol()` e `rpc.set(...)` são o [`TestMainContext`] de `support` (com o `sync()` dele). Os proxies
//!   respondem na própria chamada (no original a chamada remota só chega à thread principal numa macrotarefa), então
//!   `threads`, `controllers` e `deletedThreads` já estão preenchidos antes do `await rpc.sync()`; os casos mantêm o
//!   `sync` onde o original o tem.
//! - `new class extends mock<MainThreadCommentsShape>()` com os cinco `override` é [`MockMainThreadComments`]: os demais
//!   métodos falham com `panic!`, como o método ausente do objeto vazio lançaria `TypeError`. O mesmo vale para
//!   [`MockMainThreadCommands`] (`$registerCommand` e `$unregisterCommand`) e [`MockExtHostTelemetry`].
//! - `{ commentControlHandle, commentThreadHandle }` de `threads` é o [`MarshalledCommentThread`], e
//!   `{ $mid: MarshalledId.CommentThread, ...threads[0] }` é [`MarshalledCommentThread::to_dynamic`].
//! - `assert.strictEqual(result, thread)` compara a identidade do [`Dynamic`] que o comando devolve com
//!   [`ExtHostCommentThread::to_dynamic`] (o `thread.value`); `(await ...) as vscode.CommentThread` e `.dispose()` são
//!   [`ExtHostCommentThread::from_dynamic`].
//! - `store.add(commands.registerCommand(false, 'comments.identity', value => value))` registra o comando que devolve o
//!   primeiro argumento.
//! - `WeakRef<vscode.Comment>` é `Weak<Comment>`. O oráculo só roda o último caso com `--expose-gc` (senão chama
//!   `this.skip()`); em Rust a coleta é o descarte determinístico do último `Rc`, então o caso roda sempre, sem o
//!   `globalThis.gc`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::future::Future;
use std::rc::{Rc, Weak};

use common::{assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout, LeakGuard};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::path::Platform;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_commands::{CommandCallback, ExtHostCommands};
use je_workbench::api::common::ext_host_comments::{create_ext_host_comments, ExtHostCommentController, ExtHostCommentThread, ExtHostComments};
use je_workbench::api::common::ext_host_document_data::IExtHostDocumentSaveDelegate;
use je_workbench::api::common::ext_host_documents::ExtHostDocuments;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_protocol::{
    CommentAuthorInformation, CommentChanges, CommentProviderFeatures, CommentThreadChanges, CommentThreadRevealOptions, CommentingRangeResourceHint,
    CreateDocumentOptions, ExtHostCommentsShape, ExtHostCommandsShape, IExtHostTelemetry, IMainContext, MainContext, MainThreadCommandsShape,
    MainThreadCommentsShape, MainThreadDocumentsShape, OpenDocumentOptions,
};
use je_workbench::api::common::ext_host_protocol::CommandArgs;
use je_workbench::api::common::ext_host_types::{Comment, CommentBody, CommentData, CommentMode, Range};
use je_workbench::common::comments::MarshalledCommentThread;
use je_workbench::services::extensions::extensions::null_extension_description;
use support::ext_host_workspace_support::TestMainContext;

// ---- os dublês --------------------------------------------------------------------------------------

/// `new class extends mock<MainThreadCommentsShape>() { ... }`: `$registerCommentController`, `$unregisterCommentController`,
/// `$createCommentThread`, `$updateCommentThread` e `$deleteCommentThread`.
struct MockMainThreadComments {
    controllers: Rc<RefCell<Vec<u32>>>,
    threads: Rc<RefCell<Vec<MarshalledCommentThread>>>,
    deleted_threads: Rc<RefCell<Vec<u32>>>,
}

impl IDisposable for MockMainThreadComments {
    fn dispose(&self) {}
}

impl MainThreadCommentsShape for MockMainThreadComments {
    fn rpc_register_comment_controller(&self, handle: u32, _id: &str, _label: &str, _extension_id: &str) {
        self.controllers.borrow_mut().push(handle);
    }

    fn rpc_unregister_comment_controller(&self, _handle: u32) {}

    fn rpc_update_comment_controller_features(&self, _handle: u32, _features: CommentProviderFeatures) {
        panic!("$updateCommentControllerFeatures is not a function")
    }

    #[allow(clippy::too_many_arguments)]
    fn rpc_create_comment_thread(
        &self,
        comment_control_handle: u32,
        comment_thread_handle: u32,
        _thread_id: &str,
        _resource: &URI,
        _range: Option<je_editor::range::Range>,
        _comments: Vec<CommentChanges>,
        _extension_id: &ExtensionIdentifier,
        _is_template: bool,
        _editor_id: Option<&str>,
    ) {
        self.threads.borrow_mut().push(MarshalledCommentThread { comment_control_handle, comment_thread_handle });
    }

    fn rpc_update_comment_thread(&self, _handle: u32, _comment_thread_handle: u32, _thread_id: &str, _resource: &URI, _changes: CommentThreadChanges) {}

    fn rpc_delete_comment_thread(&self, _controller: u32, thread: u32) {
        self.deleted_threads.borrow_mut().push(thread);
    }

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

/// `new class extends mock<MainThreadCommandsShape>() { override $registerCommand(): void { } override $unregisterCommand(): void { } }`.
struct MockMainThreadCommands;

impl IDisposable for MockMainThreadCommands {
    fn dispose(&self) {}
}

impl MainThreadCommandsShape for MockMainThreadCommands {
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

/// `new class extends mock<IExtHostTelemetry>() { }`: nenhum caso chega a `onExtensionError`.
struct MockExtHostTelemetry;

impl IExtHostTelemetry for MockExtHostTelemetry {
    fn on_extension_error(&self, _extension: &ExtensionIdentifier, _error: &Error) -> bool {
        panic!("onExtensionError is not a function")
    }
}

/// O `MainThreadDocuments` que o `ExtHostDocuments` real pede e que nenhum caso chama.
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

// ---- o ambiente de cada caso -----------------------------------------------------------------------

/// O que `createComments()` devolve.
#[derive(Clone)]
struct Env {
    rpc: Rc<TestMainContext>,
    commands: Rc<ExtHostCommands>,
    comments: Rc<ExtHostComments>,
    controller: Rc<ExtHostCommentController>,
    controllers: Rc<RefCell<Vec<u32>>>,
    threads: Rc<RefCell<Vec<MarshalledCommentThread>>>,
    deleted_threads: Rc<RefCell<Vec<u32>>>,
}

impl Env {
    /// `createComments()`.
    fn new(guard: &LeakGuard, rpc: Rc<TestMainContext>, documents: Rc<ExtHostDocuments>) -> Env {
        let threads = Rc::new(RefCell::new(Vec::new()));
        let deleted_threads = Rc::new(RefCell::new(Vec::new()));
        let controllers = Rc::new(RefCell::new(Vec::new()));
        rpc.set(
            &MainContext::MAIN_THREAD_COMMENTS,
            Rc::new(MockMainThreadComments { controllers: controllers.clone(), threads: threads.clone(), deleted_threads: deleted_threads.clone() })
                as Rc<dyn MainThreadCommentsShape>,
        );
        rpc.set(&MainContext::MAIN_THREAD_COMMANDS, Rc::new(MockMainThreadCommands) as Rc<dyn MainThreadCommandsShape>);
        let commands = ExtHostCommands::new(&*rpc, NullLogService::shared(), Rc::new(MockExtHostTelemetry));
        let identity: CommandCallback = Rc::new(|args: &[Dynamic]| MaybePromise::Value(args[0].clone()));
        guard.add_rc(Rc::new(commands.register_command(false, "comments.identity", identity, None, None).expect("registerCommand")));
        let comments = create_ext_host_comments(&*rpc, &commands, documents);
        let controller = guard.add_rc(comments.create_comment_controller(&null_extension_description(), "test.comments", "Comments"));
        Env { rpc, commands, comments, controller, controllers, threads, deleted_threads }
    }

    /// `controller.createCommentThread(URI.file(path), new Range(...), [])`.
    fn create_thread(&self, path: &str, range: Range, comments: Vec<Rc<Comment>>) -> Rc<ExtHostCommentThread> {
        self.controller.create_comment_thread(uri(path), Some(range), comments).expect("createCommentThread")
    }

    /// `rpc.sync()`.
    async fn sync(&self) {
        self.rpc.sync().await.expect("sync");
    }

    /// `commands.$executeContributedCommand('comments.identity', argument)`.
    async fn execute_identity(&self, argument: &Dynamic) -> Dynamic {
        self.commands.rpc_execute_contributed_command("comments.identity", vec![argument.clone()]).await.expect("executeContributedCommand")
    }

    /// `threads[index]`.
    fn thread_key(&self, index: usize) -> MarshalledCommentThread {
        self.threads.borrow()[index]
    }

    fn deleted_threads(&self) -> Vec<u32> {
        self.deleted_threads.borrow().clone()
    }
}

/// O corpo de um caso: o `setup`, o caso e o `teardown`.
fn run_test<Fut: Future<Output = ()> + 'static>(body: impl FnOnce(Env) -> Fut + 'static) {
    let rpc = Rc::new(TestMainContext::new());
    rpc.set(&MainContext::MAIN_THREAD_DOCUMENTS, Rc::new(UnusedMainThreadDocuments) as Rc<dyn MainThreadDocumentsShape>);
    let context: Rc<dyn IMainContext> = rpc.clone();
    let documents_and_editors = Rc::new(ExtHostDocumentsAndEditors::new(context, NullLogService::shared()));
    let documents = ExtHostDocuments::new(&*rpc, documents_and_editors);

    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard, rpc, documents);
    run_with_faked_timers(async move {
        body(env).await;
    });
}

/// `URI.file(path)`.
fn uri(path: &str) -> URI {
    URI::file(path, Platform::Posix)
}

/// `new Range(startLine, startCharacter, endLine, endCharacter)`.
fn range(start_line: f64, start_character: f64, end_line: f64, end_character: f64) -> Range {
    Range::from_coordinates(start_line, start_character, end_line, end_character).expect("Range")
}

// ---- os casos --------------------------------------------------------------------------------------

#[test]
fn a_disposed_thread_no_longer_resolves_as_a_command_argument() {
    run_test(|env| async move {
        let thread = env.create_thread("/review.txt", range(0.0, 0.0, 0.0, 1.0), vec![]);
        env.sync().await;
        let argument = env.thread_key(0).to_dynamic();
        assert!(env.execute_identity(&argument).await.strict_equals(&thread.to_dynamic()));
        thread.dispose();
        assert_deep_strict_equal(&env.execute_identity(&argument).await, &argument);
    });
}

#[test]
fn repeated_disposal_deletes_a_thread_from_the_renderer_only_once() {
    run_test(|env| async move {
        let thread = env.create_thread("/review.txt", range(0.0, 0.0, 0.0, 1.0), vec![]);
        thread.dispose();
        thread.dispose();
        env.controller.dispose();
        env.sync().await;
        assert_eq!(env.deleted_threads(), vec![env.thread_key(0).comment_thread_handle]);
    });
}

#[test]
fn disposing_one_thread_preserves_command_arguments_for_another_live_thread() {
    run_test(|env| async move {
        let first = env.create_thread("/review.txt", range(0.0, 0.0, 0.0, 1.0), vec![]);
        let second = env.create_thread("/review.txt", range(1.0, 0.0, 1.0, 1.0), vec![]);
        env.sync().await;
        first.dispose();
        assert!(env.execute_identity(&env.thread_key(1).to_dynamic()).await.strict_equals(&second.to_dynamic()));
    });
}

#[test]
fn a_disposed_template_no_longer_resolves_as_a_command_argument() {
    run_test(|env| async move {
        env.sync().await;
        let controller_handle = env.controllers.borrow()[0];
        env.comments.rpc_create_comment_thread_template(controller_handle, &uri("/review.txt"), None, None).await.expect("createCommentThreadTemplate");
        env.sync().await;
        let argument = env.thread_key(0).to_dynamic();
        let thread = ExtHostCommentThread::from_dynamic(&env.execute_identity(&argument).await).expect("a thread vivo");
        thread.dispose();
        assert_deep_strict_equal(&env.execute_identity(&argument).await, &argument);
    });
}

#[test]
fn renderer_deletion_and_subsequent_api_disposal_release_the_same_thread_once() {
    run_test(|env| async move {
        let thread = env.create_thread("/review.txt", range(0.0, 0.0, 0.0, 1.0), vec![]);
        env.sync().await;
        let key = env.thread_key(0);
        env.comments.rpc_delete_comment_thread(key.comment_control_handle, key.comment_thread_handle);
        thread.dispose();
        env.sync().await;
        assert_eq!(env.deleted_threads(), vec![key.comment_thread_handle]);
    });
}

#[test]
fn controller_shutdown_disposes_every_remaining_thread() {
    run_test(|env| async move {
        env.create_thread("/first.txt", range(0.0, 0.0, 0.0, 1.0), vec![]);
        env.create_thread("/second.txt", range(0.0, 0.0, 0.0, 1.0), vec![]);
        env.controller.dispose();
        env.sync().await;
        let handles: Vec<u32> = env.threads.borrow().iter().map(|thread| thread.comment_thread_handle).collect();
        assert_eq!(env.deleted_threads(), handles);
    });
}

#[test]
fn a_live_controller_releases_the_comments_of_a_disposed_thread() {
    run_test(|env| async move {
        /// `function createAndDispose(): WeakRef<vscode.Comment>`.
        fn create_and_dispose(env: &Env) -> Weak<Comment> {
            let author = CommentAuthorInformation { name: "Reviewer".to_owned(), icon_path: None };
            let comment = Comment::new(CommentData::new(CommentBody::Text("Review comment".to_owned()), CommentMode::Preview, author));
            let thread = env.create_thread("/review.txt", range(0.0, 0.0, 0.0, 1.0), vec![comment.clone()]);
            thread.dispose();
            Rc::downgrade(&comment)
        }
        let comment = create_and_dispose(&env);
        env.sync().await;
        timeout(0).await.expect("timeout");
        assert!(comment.upgrade().is_none(), "Disposed thread comments are still retained");
    });
}
