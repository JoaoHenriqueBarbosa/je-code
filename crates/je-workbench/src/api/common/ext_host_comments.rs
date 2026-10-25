//! Porte de `src/vs/workbench/api/common/extHostComments.ts`: o lado da extensão dos comentários
//! (`vscode.comments.createCommentController`, `CommentController`, `CommentThread`), o processador de argumentos de
//! comando (`$mid` de controlador, thread, resposta, comentário) e as respostas ao protocolo (`$provideCommentingRanges`,
//! `$toggleReaction`, `$updateCommentThread`...).
//!
//! Divergências:
//! - O original define `ExtHostCommentsImpl`, `ExtHostCommentThread` e `ExtHostCommentController` dentro de
//!   `createExtHostComments`, fechando sobre o `proxy` e os contadores de handle (`handlePool` e `_handlePool` são
//!   estáticos da classe, ou seja, por chamada). Aqui o que as três fecham é o [`CommentsShared`] (o proxy e os dois
//!   contadores), e [`create_ext_host_comments`] devolve o [`ExtHostComments`].
//! - A thread e o controlador têm no original uma classe interna e um `value` (objeto literal congelado, o que a
//!   extensão vê). Aqui são um tipo só: [`ExtHostCommentThread`] e [`ExtHostCommentController`] (em `Rc`) são o objeto da
//!   API, os getters e setters são métodos, e `value` é [`ExtHostCommentThread::to_dynamic`] e
//!   [`ExtHostCommentController::to_dynamic`], o [`Dynamic`] que o processador de argumentos devolve (com identidade
//!   estável). O que a classe interna expõe a mais (`handle`, `commentHandle`, `threadId`, `getCommentByUniqueId`...)
//!   fica visível, e o `activeComment` que o `value` do controlador deixa comentado é [`ExtHostCommentController::active_comment`].
//! - Os getters e setters que chamam `checkProposedApiEnabled` (e as operações que lançam) devolvem [`Result`]; as
//!   funções `async` devolvem [`Promise`] (rejeitada onde o original lança). O `throw` de `convertToDTOComment` na
//!   construção de uma thread é o erro de [`ExtHostCommentController::create_comment_thread`]; dentro do `debounce` (que
//!   não tem a quem lançar) o erro vai para [`errors::on_unexpected_error`].
//! - `@debounce(100)` de `eventuallyUpdateCommentThread` é o [`Debounce`] de `je_base::decorators`; o fecho do `debounce` e
//!   o do ouvinte de `onDidUpdateCommentThread` guardam um [`Weak`] da thread (um `Rc` fecharia um ciclo, e a suíte
//!   confere com `Weak` que a thread descartada solta os comentários).
//! - `CommentThreadModification` (um objeto em que só a presença da chave importa: o valor é relido da thread) é um
//!   struct de booleanos. `Map<ProviderHandle, ExtHostCommentController>` e `Map<number, ExtHostCommentThread>` são
//!   [`BTreeMap`] por handle (os handles crescem, então a ordem é a de inserção do `Map`); `Map<vscode.Comment, number>`
//!   é uma lista de pares comparados por identidade de `Rc`.
//! - `_localDisposables` do controlador, que o original descarta sem proteção (descartar duas vezes cancela o registro
//!   duas vezes), é uma lista de fechos, para manter o comportamento; o da thread é uma lista de `DisposableRef`.
//! - O `text` de um argumento `CommentThreadNode` que não é texto (a thread principal sempre manda texto) lê como texto vazio.
//! - `ExtHostCommentThread#reveal` sem comentário na thread manda `0` como o id do comentário (o original manda `undefined`, e
//!   o id real começa em `1`).
//! - O `Range.to(range)` de `$createCommentThreadTemplate` e `$updateCommentThreadTemplate`, que lança para um `IRange`
//!   negativo, rejeita a promessa.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, await_maybe};
use je_base::cancellation::CancellationToken;
use je_base::decorators::Debounce;
use je_base::dynamic::{date_to_iso_string, Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::Emitter;
use je_base::lifecycle::{plain_disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::marshalling_ids::MarshalledId;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::languages::StringOrMarkdown;
use je_editor::range::Range as EditorRange;
use je_extensions::scanner::ExtensionDescription;

use super::ext_host_commands::{ArgumentProcessor, ExtHostCommands};
use super::ext_host_documents::ExtHostDocuments;
use super::ext_host_protocol::{
    ActiveCommentInfo, CommentAuthorInformation, CommentCanReply, CommentChanges, CommentOptions, CommentProviderFeatures, CommentReaction,
    CommentThreadChanges, CommentThreadRevealOptions, CommentingRangesDto, ExtHostCommentsShape, IMainContext, MainContext, MainThreadCommentsShape,
};
use super::ext_host_type_converters::{self as type_converters, path_or_uri_to_uri};
use super::ext_host_types::{
    self as types, Comment, CommentBody, CommentOrRevealOptions, CommentReactors, CommentThreadApplicability, CommentThreadCollapsibleState,
    CommentThreadFocus, CommentThreadState, CommentThreadStateValue, CommentingRangeProvider, CommentingRangesResult, PathOrUri,
};
use crate::common::comments::{handle_of, MarshalledCommentThread};
use crate::services::extensions::extensions::check_proposed_api_enabled;

/// `ReactionHandler`: `(comment, reaction) => Promise<void>`.
pub type ReactionHandler = Rc<dyn Fn(&Rc<Comment>, &types::CommentReaction) -> MaybePromise<()>>;

type ControllerMap = RefCell<BTreeMap<u32, Rc<ExtHostCommentController>>>;

/// O que as três classes do original fecham: o `proxy` e os contadores `handlePool` (controladores) e `_handlePool`
/// (threads).
struct CommentsShared {
    proxy: Rc<dyn MainThreadCommentsShape>,
    controller_handle_pool: Cell<u32>,
    thread_handle_pool: Cell<u32>,
}

/// `createExtHostComments(mainContext, commands, documents)`.
pub fn create_ext_host_comments(main_context: &dyn IMainContext, commands: &ExtHostCommands, documents: Rc<ExtHostDocuments>) -> Rc<ExtHostComments> {
    let controllers: Rc<ControllerMap> = Rc::new(RefCell::new(BTreeMap::new()));
    commands.register_argument_processor(Rc::new(CommentsArgumentProcessor { controllers: controllers.clone() }));
    Rc::new(ExtHostComments {
        shared: Rc::new(CommentsShared {
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_COMMENTS),
            controller_handle_pool: Cell::new(0),
            thread_handle_pool: Cell::new(0),
        }),
        documents,
        controllers,
    })
}

/// O `processArgument` que `ExtHostCommentsImpl` registra em `commands.registerArgumentProcessor`.
struct CommentsArgumentProcessor {
    controllers: Rc<ControllerMap>,
}

impl CommentsArgumentProcessor {
    fn controller(&self, handle: u32) -> Option<Rc<ExtHostCommentController>> {
        self.controllers.borrow().get(&handle).cloned()
    }

    /// `this._commentControllers.get(thread.commentControlHandle)?.getCommentThread(thread.commentThreadHandle)`: a thread
    /// de uma forma `MarshalledCommentThread` (o próprio argumento ou o `arg.thread`).
    fn thread_of(&self, marshalled: &Dynamic) -> Option<Rc<ExtHostCommentThread>> {
        let marshalled = MarshalledCommentThread::from_dynamic(marshalled)?;
        self.controller(marshalled.comment_control_handle)?.get_comment_thread(marshalled.comment_thread_handle)
    }

    /// `commentThread.getCommentByUniqueId(arg.commentUniqueId)` de um argumento `CommentNode` ou `CommentThreadNode`.
    fn comment_of(&self, arg: &Dynamic) -> Option<Rc<Comment>> {
        let thread = self.thread_of(&arg.get_str("thread"))?;
        thread.get_comment_by_unique_id(handle_of(&arg.get_str("commentUniqueId"))?)
    }
}

impl ArgumentProcessor for CommentsArgumentProcessor {
    fn process_argument(&self, arg: &Dynamic, _extension: Option<&ExtensionDescription>) -> Dynamic {
        if !arg.is_truthy() {
            return arg.clone();
        }
        let Dynamic::Number(mid) = arg.get_str("$mid") else {
            return arg.clone();
        };
        let Some(mid) = MarshalledId::from_i32(mid as i32).filter(|id| f64::from(*id as i32) == mid) else {
            return arg.clone();
        };
        let processed = match mid {
            MarshalledId::CommentController => handle_of(&arg.get_str("handle")).and_then(|handle| self.controller(handle)).map(|controller| controller.to_dynamic()),
            MarshalledId::CommentThread => self.thread_of(arg).map(|thread| thread.to_dynamic()),
            MarshalledId::CommentThreadReply | MarshalledId::CommentThreadInstance => self.thread_of(&arg.get_str("thread")).map(|thread| {
                if mid == MarshalledId::CommentThreadInstance {
                    return thread.to_dynamic();
                }
                Dynamic::object([("thread", thread.to_dynamic()), ("text", arg.get_str("text"))])
            }),
            MarshalledId::CommentNode => self.comment_of(arg).map(|comment| comment.to_dynamic()),
            MarshalledId::CommentThreadNode => self.comment_of(arg).map(|comment| {
                let body = match arg.get_str("text") {
                    Dynamic::String(text) => text.to_string_lossy(),
                    _ => String::new(),
                };
                let mut data = comment.data.borrow_mut();
                // If the old comment body was a markdown string, use a markdown string here too.
                data.body = match data.body {
                    CommentBody::Text(_) => CommentBody::Text(body),
                    CommentBody::Markdown(_) => CommentBody::Markdown(types::MarkdownString::new(Some(&body), false)),
                };
                drop(data);
                comment.to_dynamic()
            }),
            _ => None,
        };
        processed.unwrap_or_else(|| arg.clone())
    }
}

/// `ExtHostComments` (a interface `ExtHostComments` e o `ExtHostCommentsImpl` do original).
pub struct ExtHostComments {
    shared: Rc<CommentsShared>,
    documents: Rc<ExtHostDocuments>,
    controllers: Rc<ControllerMap>,
}

impl ExtHostComments {
    /// `createCommentController(extension, id, label)`.
    pub fn create_comment_controller(&self, extension: &ExtensionDescription, id: &str, label: &str) -> Rc<ExtHostCommentController> {
        let handle = self.shared.controller_handle_pool.get();
        self.shared.controller_handle_pool.set(handle + 1);
        let controllers = Rc::downgrade(&self.controllers);
        let comment_controller = ExtHostCommentController::new(self.shared.clone(), extension.clone(), handle, id, label, move || {
            if let Some(controllers) = controllers.upgrade() {
                controllers.borrow_mut().remove(&handle);
            }
        });
        self.controllers.borrow_mut().insert(comment_controller.handle, comment_controller.clone());
        comment_controller
    }

    fn controller(&self, handle: u32) -> Option<Rc<ExtHostCommentController>> {
        self.controllers.borrow().get(&handle).cloned()
    }
}

impl ExtHostCommentsShape for ExtHostComments {
    fn rpc_create_comment_thread_template(&self, comment_controller_handle: u32, uri: &URI, range: Option<EditorRange>, editor_id: Option<&str>) -> Promise<()> {
        let Some(comment_controller) = self.controller(comment_controller_handle) else {
            return Promise::resolved(());
        };
        match comment_controller.create_comment_thread_template(uri, range.as_ref(), editor_id) {
            Ok(_) => Promise::resolved(()),
            Err(error) => Promise::rejected(error),
        }
    }

    fn rpc_set_active_comment(&self, controller_handle: u32, comment_info: Option<ActiveCommentInfo>) -> Promise<()> {
        if let Some(comment_controller) = self.controller(controller_handle) {
            comment_controller.set_active_comment(comment_info);
        }
        Promise::resolved(())
    }

    fn rpc_update_comment_thread_template(&self, comment_controller_handle: u32, thread_handle: u32, range: EditorRange) -> Promise<()> {
        let Some(comment_controller) = self.controller(comment_controller_handle) else {
            return Promise::resolved(());
        };
        match comment_controller.update_comment_thread_template(thread_handle, &range) {
            Ok(()) => Promise::resolved(()),
            Err(error) => Promise::rejected(error),
        }
    }

    fn rpc_delete_comment_thread(&self, comment_controller_handle: u32, comment_thread_handle: u32) {
        if let Some(comment_controller) = self.controller(comment_controller_handle) {
            comment_controller.delete_comment_thread(comment_thread_handle);
        }
    }

    fn rpc_update_comment_thread(&self, comment_controller_handle: u32, comment_thread_handle: u32, changes: CommentThreadChanges) -> Promise<()> {
        if let Some(comment_controller) = self.controller(comment_controller_handle) {
            comment_controller.update_comment_thread(comment_thread_handle, &changes);
        }
        Promise::resolved(())
    }

    fn rpc_provide_commenting_ranges(&self, comment_controller_handle: u32, uri: &URI, token: CancellationToken) -> Promise<Option<CommentingRangesDto>> {
        let Some(comment_controller) = self.controller(comment_controller_handle).filter(|controller| controller.commenting_range_provider().is_some()) else {
            return Promise::resolved(None);
        };
        let documents = self.documents.clone();
        let uri = uri.clone();
        async_fn(async move {
            let document = documents.ensure_document_data(&uri, None).await?;
            let Some(provider) = comment_controller.commenting_range_provider() else {
                return Ok(None);
            };
            let ranges_result = await_maybe((provider.provide_commenting_ranges)(&document.document(), &token)).await?;
            Ok(ranges_result.map(|ranges_result| {
                let (ranges, file_comments) = match ranges_result {
                    CommentingRangesResult::Ranges(ranges) => (ranges, false),
                    CommentingRangesResult::Info(info) => (info.ranges.unwrap_or_default(), info.enable_file_comments.unwrap_or(false)),
                };
                CommentingRangesDto { ranges: ranges.iter().map(type_converters::Range::from).collect(), file_comments }
            }))
        })
    }

    fn rpc_toggle_reaction(&self, comment_controller_handle: u32, thread_handle: u32, _uri: &URI, comment: CommentChanges, reaction: CommentReaction) -> Promise<()> {
        let Some(comment_controller) = self.controller(comment_controller_handle).filter(|controller| controller.reaction_handler().is_some()) else {
            return Promise::resolved(());
        };
        async_fn(async move {
            let Some(comment_thread) = comment_controller.get_comment_thread(thread_handle) else {
                return Ok(());
            };
            let Some(vscode_comment) = comment_thread.get_comment_by_unique_id(comment.unique_id_in_thread) else {
                return Ok(());
            };
            if let Some(reaction_handler) = comment_controller.reaction_handler() {
                await_maybe(reaction_handler(&vscode_comment, &convert_from_reaction(&reaction))).await?;
            }
            Ok(())
        })
    }
}

/// `CommentThreadModification`: só a presença da chave importa (o valor é relido da thread).
#[derive(Clone, Copy, Default)]
struct CommentThreadModification {
    range: bool,
    label: bool,
    context_value: bool,
    comments: bool,
    collapsible_state: bool,
    can_reply: bool,
    state: bool,
    applicability: bool,
    is_template: bool,
}

/// `ExtHostCommentThread` (o que o original chama de `ExtHostCommentThread` e de `vscode.CommentThread2`).
pub struct ExtHostCommentThread {
    this: Weak<ExtHostCommentThread>,
    shared: Rc<CommentsShared>,
    handle: u32,
    comment_handle: Cell<u32>,
    modifications: Cell<CommentThreadModification>,
    id: RefCell<String>,
    uri: URI,
    range: RefCell<Option<types::Range>>,
    can_reply: RefCell<CommentCanReply>,
    label: RefCell<Option<String>>,
    context_value: RefCell<Option<String>>,
    comments: RefCell<Vec<Rc<Comment>>>,
    collapse_state: Cell<Option<CommentThreadCollapsibleState>>,
    state: Cell<Option<CommentThreadStateValue>>,
    local_disposables: Vec<DisposableRef>,
    is_disposed: Cell<bool>,
    comments_map: RefCell<Vec<(Rc<Comment>, u32)>>,
    accept_input_disposables: Rc<MutableDisposable<DisposableStore>>,
    on_did_update_comment_thread: Rc<Emitter<()>>,
    debounce: Debounce<(), ()>,
    controller_handle: u32,
    is_template: Cell<bool>,
    extension_description: ExtensionDescription,
    value: OnceCell<Dynamic>,
}

impl ExtHostCommentThread {
    #[allow(clippy::too_many_arguments)]
    fn new(
        shared: Rc<CommentsShared>,
        comment_controller_id: &str,
        comment_controller_handle: u32,
        id: Option<String>,
        uri: URI,
        range: Option<types::Range>,
        comments: Vec<Rc<Comment>>,
        extension_description: ExtensionDescription,
        is_template: bool,
        on_did_dispose: impl Fn(u32) + 'static,
        editor_id: Option<&str>,
    ) -> Result<Rc<ExtHostCommentThread>, Error> {
        let handle = shared.thread_handle_pool.get();
        shared.thread_handle_pool.set(handle + 1);
        let id = id.unwrap_or_else(|| format!("{comment_controller_id}.{handle}"));

        let accept_input_disposables = MutableDisposable::new();
        accept_input_disposables.set(Some(DisposableStore::new()));

        let thread = Rc::new_cyclic(|this: &Weak<ExtHostCommentThread>| {
            let on_did_update_comment_thread = Emitter::<()>::new();
            let debounced = this.clone();
            let updated = this.clone();
            let proxy = shared.proxy.clone();
            let local_disposables: Vec<DisposableRef> = vec![
                Rc::new(types::Disposable::new(move || on_did_dispose(handle))),
                on_did_update_comment_thread.event().listen(move |_: &()| {
                    if let Some(thread) = updated.upgrade() {
                        thread.eventually_update_comment_thread();
                    }
                }),
                plain_disposable(move || proxy.rpc_delete_comment_thread(comment_controller_handle, handle)),
            ];
            ExtHostCommentThread {
                this: this.clone(),
                shared: shared.clone(),
                handle,
                comment_handle: Cell::new(0),
                modifications: Cell::new(CommentThreadModification::default()),
                id: RefCell::new(id),
                uri,
                range: RefCell::new(range),
                can_reply: RefCell::new(CommentCanReply::Bool(true)),
                label: RefCell::new(None),
                context_value: RefCell::new(None),
                comments: RefCell::new(comments),
                collapse_state: Cell::new(None),
                state: Cell::new(None),
                local_disposables,
                is_disposed: Cell::new(false),
                comments_map: RefCell::new(Vec::new()),
                accept_input_disposables,
                on_did_update_comment_thread,
                debounce: Debounce::without_reducer(100, move |()| {
                    if let Some(thread) = debounced.upgrade() {
                        thread.update_comment_thread();
                    }
                }),
                controller_handle: comment_controller_handle,
                is_template: Cell::new(is_template),
                extension_description,
                value: OnceCell::new(),
            }
        });

        let dto_comments = thread.convert_comments()?;
        shared.proxy.rpc_create_comment_thread(
            comment_controller_handle,
            handle,
            &thread.id.borrow(),
            &thread.uri,
            type_converters::Range::from_optional(thread.range.borrow().as_ref()),
            dto_comments,
            &thread.extension_description.identifier,
            is_template,
            editor_id,
        );
        Ok(thread)
    }

    /// `value`: a thread como argumento de comando (o mesmo objeto em toda leitura).
    pub fn to_dynamic(&self) -> Dynamic {
        types::weak_host_dynamic(&self.value, &self.this)
    }

    /// A thread de um argumento criado por [`ExtHostCommentThread::to_dynamic`], se ainda estiver viva.
    pub fn from_dynamic(value: &Dynamic) -> Option<Rc<ExtHostCommentThread>> {
        types::host_object::<ExtHostCommentThread>(value)
    }

    pub fn handle(&self) -> u32 {
        self.handle
    }

    pub fn extension_description(&self) -> &ExtensionDescription {
        &self.extension_description
    }

    /// `threadId` e `id`.
    pub fn id(&self) -> String {
        self.id.borrow().clone()
    }

    pub fn set_thread_id(&self, id: &str) {
        *self.id.borrow_mut() = id.to_owned();
    }

    /// `resource` e `uri`.
    pub fn uri(&self) -> &URI {
        &self.uri
    }

    pub fn range(&self) -> Option<types::Range> {
        *self.range.borrow()
    }

    pub fn set_range(&self, range: Option<types::Range>) {
        let current = *self.range.borrow();
        let changed = match (range, current) {
            (Some(range), Some(current)) => !range.is_equal(&current),
            _ => true,
        };
        if changed {
            *self.range.borrow_mut() = range;
            self.modify(|modifications| modifications.range = true);
            self.on_did_update_comment_thread.fire(());
        }
    }

    pub fn can_reply(&self) -> CommentCanReply {
        self.can_reply.borrow().clone()
    }

    pub fn set_can_reply(&self, state: CommentCanReply) {
        let changed = !self.can_reply.borrow().same(&state);
        if changed {
            *self.can_reply.borrow_mut() = state;
            self.modify(|modifications| modifications.can_reply = true);
            self.on_did_update_comment_thread.fire(());
        }
    }

    pub fn label(&self) -> Option<String> {
        self.label.borrow().clone()
    }

    pub fn set_label(&self, label: Option<String>) {
        *self.label.borrow_mut() = label;
        self.modify(|modifications| modifications.label = true);
        self.on_did_update_comment_thread.fire(());
    }

    pub fn context_value(&self) -> Option<String> {
        self.context_value.borrow().clone()
    }

    pub fn set_context_value(&self, context: Option<String>) {
        *self.context_value.borrow_mut() = context;
        self.modify(|modifications| modifications.context_value = true);
        self.on_did_update_comment_thread.fire(());
    }

    pub fn comments(&self) -> Vec<Rc<Comment>> {
        self.comments.borrow().clone()
    }

    pub fn set_comments(&self, new_comments: Vec<Rc<Comment>>) {
        *self.comments.borrow_mut() = new_comments;
        self.modify(|modifications| modifications.comments = true);
        self.on_did_update_comment_thread.fire(());
    }

    pub fn collapsible_state(&self) -> Option<CommentThreadCollapsibleState> {
        self.collapse_state.get()
    }

    pub fn set_collapsible_state(&self, new_state: CommentThreadCollapsibleState) {
        if self.collapse_state.get() == Some(new_state) {
            return;
        }
        self.collapse_state.set(Some(new_state));
        self.modify(|modifications| modifications.collapsible_state = true);
        self.on_did_update_comment_thread.fire(());
    }

    pub fn state(&self) -> Option<CommentThreadStateValue> {
        self.state.get()
    }

    /// `set state(newState)`: a forma de objeto (`{ resolved, applicability }`) pede a proposta `commentThreadApplicability`.
    pub fn set_state(&self, new_state: CommentThreadStateValue) -> Result<(), Error> {
        self.state.set(Some(new_state));
        match new_state {
            CommentThreadStateValue::Info { .. } => {
                check_proposed_api_enabled(&self.extension_description, "commentThreadApplicability")?;
                self.modify(|modifications| {
                    modifications.state = true;
                    modifications.applicability = true;
                });
            }
            CommentThreadStateValue::State(_) => self.modify(|modifications| modifications.state = true),
        }
        self.on_did_update_comment_thread.fire(());
        Ok(())
    }

    pub fn is_disposed(&self) -> bool {
        self.is_disposed.get()
    }

    /// `onDidUpdateCommentThread`.
    pub fn on_did_update_comment_thread(&self) -> je_base::event::Event<()> {
        self.on_did_update_comment_thread.event()
    }

    fn modify(&self, change: impl FnOnce(&mut CommentThreadModification)) {
        let mut modifications = self.modifications.get();
        change(&mut modifications);
        self.modifications.set(modifications);
    }

    fn update_is_template(&self) {
        if self.is_template.get() {
            self.is_template.set(false);
            self.modify(|modifications| modifications.is_template = true);
        }
    }

    /// `eventuallyUpdateCommentThread()` (`@debounce(100)`): agenda a atualização para 100 ms depois da última chamada.
    pub fn eventually_update_comment_thread(&self) {
        self.debounce.call(());
    }

    /// O corpo de `eventuallyUpdateCommentThread`.
    fn update_comment_thread(&self) {
        if self.is_disposed.get() {
            return;
        }
        self.update_is_template();

        if self.accept_input_disposables.value().is_none() {
            self.accept_input_disposables.set(Some(DisposableStore::new()));
        }

        match self.format_modifications(self.modifications.get()) {
            Err(error) => errors::on_unexpected_error(&error),
            Ok(formatted_modifications) => {
                self.modifications.set(CommentThreadModification::default());
                self.shared.proxy.rpc_update_comment_thread(self.controller_handle, self.handle, &self.id.borrow(), &self.uri, formatted_modifications);
            }
        }
    }

    fn format_modifications(&self, modified: CommentThreadModification) -> Result<CommentThreadChanges, Error> {
        let mut formatted_modifications = CommentThreadChanges::default();
        if modified.range {
            formatted_modifications.range = Some(type_converters::Range::from_optional(self.range.borrow().as_ref()));
        }
        if modified.label {
            formatted_modifications.label = Some(self.label());
        }
        if modified.context_value {
            // null -> cleared contextValue
            // undefined -> no change
            formatted_modifications.context_value = Some(self.context_value());
        }
        if modified.comments {
            formatted_modifications.comments = Some(self.convert_comments()?);
        }
        if modified.collapsible_state {
            formatted_modifications.collapse_state = Some(convert_to_collapsible_state(self.collapse_state.get()));
        }
        if modified.can_reply {
            formatted_modifications.can_reply = Some(self.can_reply());
        }
        if modified.state {
            formatted_modifications.state = Some(convert_to_state(self.state.get()));
        }
        if modified.applicability {
            formatted_modifications.applicability = Some(convert_to_relevance(self.state.get()));
        }
        if modified.is_template {
            formatted_modifications.is_template = Some(self.is_template.get());
        }
        Ok(formatted_modifications)
    }

    /// `this._comments.map(cmt => convertToDTOComment(this, cmt, this._commentsMap, this.extensionDescription))`.
    fn convert_comments(&self) -> Result<Vec<CommentChanges>, Error> {
        let comments = self.comments.borrow().clone();
        comments.iter().map(|comment| self.convert_to_dto_comment(comment)).collect()
    }

    /// `convertToDTOComment`: dá ao comentário o id dele na thread (o primeiro uso o cria).
    fn convert_to_dto_comment(&self, vscode_comment: &Rc<Comment>) -> Result<CommentChanges, Error> {
        let known = self.comments_map.borrow().iter().find(|(comment, _)| Rc::ptr_eq(comment, vscode_comment)).map(|(_, id)| *id);
        let comment_unique_id = match known.filter(|id| *id != 0) {
            Some(id) => id,
            None => {
                let id = self.comment_handle.get() + 1;
                self.comment_handle.set(id);
                self.comments_map.borrow_mut().push((vscode_comment.clone(), id));
                id
            }
        };

        let data = vscode_comment.data.borrow();
        if data.state.is_some() {
            check_proposed_api_enabled(&self.extension_description, "commentsDraftState")?;
        }

        if data.reactions.as_ref().is_some_and(|reactions| reactions.iter().any(|reaction| reaction.reactors.is_some())) {
            check_proposed_api_enabled(&self.extension_description, "commentReactor")?;
        }

        Ok(CommentChanges {
            mode: Some(data.mode),
            context_value: data.context_value.clone(),
            unique_id_in_thread: comment_unique_id,
            body: match &data.body {
                CommentBody::Text(text) => StringOrMarkdown::String(text.clone()),
                CommentBody::Markdown(markdown) => StringOrMarkdown::Markdown(type_converters::MarkdownString::from_markdown(markdown)),
            },
            user_name: data.author.name.clone(),
            user_icon_path: data.author.icon_path.clone(),
            label: data.label.clone(),
            comment_reactions: data.reactions.as_ref().map(|reactions| reactions.iter().map(convert_to_reaction).collect()),
            state: data.state,
            timestamp: data.timestamp.and_then(date_to_iso_string),
        })
    }

    /// `getCommentByUniqueId(uniqueId)`.
    pub fn get_comment_by_unique_id(&self, unique_id: u32) -> Option<Rc<Comment>> {
        self.comments_map.borrow().iter().find(|(_, id)| *id == unique_id).map(|(comment, _)| comment.clone())
    }

    /// `reveal(commentOrOptions?, options?)` (proposta `commentReveal`).
    pub fn reveal(&self, comment_or_options: Option<CommentOrRevealOptions>, options: Option<types::CommentThreadRevealOptions>) -> Promise<()> {
        let Some(this) = self.this.upgrade() else {
            return Promise::rejected(Error::new("a thread de comentários foi descartada"));
        };
        async_fn(async move {
            check_proposed_api_enabled(&this.extension_description, "commentReveal")?;
            let (comment, options) = match comment_or_options {
                Some(CommentOrRevealOptions::Comment(comment)) => (Some(comment), options),
                Some(CommentOrRevealOptions::Options(reveal_options)) => (None, options.or(Some(reveal_options))),
                None => (None, options),
            };
            let unique_id_of = |comment: &Rc<Comment>| this.comments_map.borrow().iter().find(|(known, _)| Rc::ptr_eq(known, comment)).map(|(_, id)| *id);
            let comment_to_reveal = comment
                .as_ref()
                .and_then(unique_id_of)
                .or_else(|| this.comments.borrow().first().and_then(unique_id_of));
            let mut preserve_focus = true;
            let mut focus_reply = false;
            match options.and_then(|options| options.focus) {
                Some(CommentThreadFocus::Reply) => {
                    focus_reply = true;
                    preserve_focus = false;
                }
                Some(CommentThreadFocus::Comment) => preserve_focus = false,
                None => {}
            }
            this.shared
                .proxy
                .rpc_reveal_comment_thread(this.controller_handle, this.handle, comment_to_reveal.unwrap_or(0), CommentThreadRevealOptions { preserve_focus, focus_reply })
                .await
        })
    }

    /// `hide()`.
    pub fn hide(&self) -> Promise<()> {
        self.shared.proxy.rpc_hide_comment_thread(self.controller_handle, self.handle);
        Promise::resolved(())
    }
}

impl IDisposable for ExtHostCommentThread {
    fn dispose(&self) {
        if self.is_disposed.get() {
            return;
        }
        self.is_disposed.set(true);
        self.accept_input_disposables.dispose();
        self.on_did_update_comment_thread.dispose();
        for disposable in &self.local_disposables {
            disposable.dispose();
        }
    }
}

/// `ExtHostCommentController` (o que o original chama de `ExtHostCommentController` e de `vscode.CommentController`).
pub struct ExtHostCommentController {
    this: Weak<ExtHostCommentController>,
    shared: Rc<CommentsShared>,
    extension: ExtensionDescription,
    handle: u32,
    id: String,
    label: String,
    threads: RefCell<BTreeMap<u32, Rc<ExtHostCommentThread>>>,
    commenting_range_provider: RefCell<Option<CommentingRangeProvider>>,
    reaction_handler: RefCell<Option<ReactionHandler>>,
    options: RefCell<Option<CommentOptions>>,
    active_comment: RefCell<Option<Rc<Comment>>>,
    active_thread: RefCell<Option<Rc<ExtHostCommentThread>>>,
    local_disposables: Vec<Box<dyn Fn()>>,
    value: OnceCell<Dynamic>,
}

impl ExtHostCommentController {
    fn new(shared: Rc<CommentsShared>, extension: ExtensionDescription, handle: u32, id: &str, label: &str, on_did_dispose: impl Fn() + 'static) -> Rc<ExtHostCommentController> {
        shared.proxy.rpc_register_comment_controller(handle, id, label, &extension.identifier.value);
        Rc::new_cyclic(|this: &Weak<ExtHostCommentController>| {
            let proxy = shared.proxy.clone();
            ExtHostCommentController {
                this: this.clone(),
                shared,
                extension,
                handle,
                id: id.to_owned(),
                label: label.to_owned(),
                threads: RefCell::new(BTreeMap::new()),
                commenting_range_provider: RefCell::new(None),
                reaction_handler: RefCell::new(None),
                options: RefCell::new(None),
                active_comment: RefCell::new(None),
                active_thread: RefCell::new(None),
                local_disposables: vec![Box::new(move || proxy.rpc_unregister_comment_controller(handle)), Box::new(on_did_dispose)],
                value: OnceCell::new(),
            }
        })
    }

    /// `value`: o controlador como argumento de comando (o mesmo objeto em toda leitura).
    pub fn to_dynamic(&self) -> Dynamic {
        types::weak_host_dynamic(&self.value, &self.this)
    }

    /// O controlador de um argumento criado por [`ExtHostCommentController::to_dynamic`], se ainda estiver vivo.
    pub fn from_dynamic(value: &Dynamic) -> Option<Rc<ExtHostCommentController>> {
        types::host_object::<ExtHostCommentController>(value)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn handle(&self) -> u32 {
        self.handle
    }

    pub fn commenting_range_provider(&self) -> Option<CommentingRangeProvider> {
        self.commenting_range_provider.borrow().clone()
    }

    /// `set commentingRangeProvider(provider)`: o `resourceHints` pede a proposta `commentingRangeHint`.
    pub fn set_commenting_range_provider(&self, provider: Option<CommentingRangeProvider>) -> Result<(), Error> {
        let resource_hints = provider.as_ref().and_then(|provider| provider.resource_hints.clone());
        *self.commenting_range_provider.borrow_mut() = provider;
        if resource_hints.is_some() {
            check_proposed_api_enabled(&self.extension, "commentingRangeHint")?;
        }
        self.shared.proxy.rpc_update_commenting_ranges(self.handle, resource_hints);
        Ok(())
    }

    pub fn reaction_handler(&self) -> Option<ReactionHandler> {
        self.reaction_handler.borrow().clone()
    }

    pub fn set_reaction_handler(&self, handler: Option<ReactionHandler>) {
        let has_handler = handler.is_some();
        *self.reaction_handler.borrow_mut() = handler;
        self.shared.proxy.rpc_update_comment_controller_features(self.handle, CommentProviderFeatures { reaction_handler: Some(has_handler), ..CommentProviderFeatures::default() });
    }

    pub fn options(&self) -> Option<CommentOptions> {
        self.options.borrow().clone()
    }

    pub fn set_options(&self, options: Option<CommentOptions>) {
        *self.options.borrow_mut() = options.clone();
        self.shared.proxy.rpc_update_comment_controller_features(self.handle, CommentProviderFeatures { options, ..CommentProviderFeatures::default() });
    }

    /// `activeComment` (proposta `activeComment`).
    pub fn active_comment(&self) -> Result<Option<Rc<Comment>>, Error> {
        check_proposed_api_enabled(&self.extension, "activeComment")?;
        Ok(self.active_comment.borrow().clone())
    }

    /// `activeCommentThread` (proposta `activeComment`).
    pub fn active_comment_thread(&self) -> Result<Option<Rc<ExtHostCommentThread>>, Error> {
        check_proposed_api_enabled(&self.extension, "activeComment")?;
        Ok(self.active_thread.borrow().clone())
    }

    /// `createCommentThread(resource, range, comments)`.
    pub fn create_comment_thread(&self, resource: URI, range: Option<types::Range>, comments: Vec<Rc<Comment>>) -> Result<Rc<ExtHostCommentThread>, Error> {
        let comment_thread = ExtHostCommentThread::new(
            self.shared.clone(),
            &self.id,
            self.handle,
            None,
            resource,
            range,
            comments,
            self.extension.clone(),
            false,
            self.forget_thread(),
            None,
        )?;
        self.threads.borrow_mut().insert(comment_thread.handle, comment_thread.clone());
        Ok(comment_thread)
    }

    /// O `onDidDispose` de uma thread: tira a thread de `_threads`.
    fn forget_thread(&self) -> impl Fn(u32) + 'static {
        let this = self.this.clone();
        move |handle| {
            if let Some(controller) = this.upgrade() {
                controller.threads.borrow_mut().remove(&handle);
            }
        }
    }

    /// `$setActiveComment(commentInfo)`.
    fn set_active_comment(&self, comment_info: Option<ActiveCommentInfo>) {
        let Some(comment_info) = comment_info else {
            *self.active_comment.borrow_mut() = None;
            *self.active_thread.borrow_mut() = None;
            return;
        };
        if let Some(thread) = self.get_comment_thread(comment_info.comment_thread_handle) {
            *self.active_comment.borrow_mut() = comment_info.unique_id_in_thread.filter(|id| *id != 0).and_then(|id| thread.get_comment_by_unique_id(id));
            *self.active_thread.borrow_mut() = Some(thread);
        }
    }

    /// `$createCommentThreadTemplate(uriComponents, range, editorId)`.
    fn create_comment_thread_template(&self, uri: &URI, range: Option<&EditorRange>, editor_id: Option<&str>) -> Result<Rc<ExtHostCommentThread>, Error> {
        let comment_thread = ExtHostCommentThread::new(
            self.shared.clone(),
            &self.id,
            self.handle,
            None,
            uri.clone(),
            type_converters::Range::to_optional(range)?,
            Vec::new(),
            self.extension.clone(),
            true,
            self.forget_thread(),
            editor_id,
        )?;
        comment_thread.set_collapsible_state(CommentThreadCollapsibleState::Expanded);
        self.threads.borrow_mut().insert(comment_thread.handle, comment_thread.clone());
        Ok(comment_thread)
    }

    /// `$updateCommentThreadTemplate(threadHandle, range)`.
    fn update_comment_thread_template(&self, thread_handle: u32, range: &EditorRange) -> Result<(), Error> {
        if let Some(thread) = self.get_comment_thread(thread_handle) {
            thread.set_range(Some(type_converters::Range::to(range)?));
        }
        Ok(())
    }

    /// `$updateCommentThread(threadHandle, changes)`: só o `collapseState` volta da thread principal.
    fn update_comment_thread(&self, thread_handle: u32, changes: &CommentThreadChanges) {
        let Some(thread) = self.get_comment_thread(thread_handle) else {
            return;
        };
        if changes.collapse_state.is_some() {
            thread.set_collapsible_state(convert_to_collapsible_state(changes.collapse_state));
        }
    }

    /// `$deleteCommentThread(threadHandle)`.
    fn delete_comment_thread(&self, thread_handle: u32) {
        let thread = self.get_comment_thread(thread_handle);
        if let Some(thread) = thread {
            thread.dispose();
        }
        self.threads.borrow_mut().remove(&thread_handle);
    }

    /// `getCommentThread(handle)`.
    pub fn get_comment_thread(&self, handle: u32) -> Option<Rc<ExtHostCommentThread>> {
        self.threads.borrow().get(&handle).cloned()
    }
}

impl IDisposable for ExtHostCommentController {
    fn dispose(&self) {
        let threads: Vec<Rc<ExtHostCommentThread>> = self.threads.borrow().values().cloned().collect();
        for thread in threads {
            thread.dispose();
        }

        for disposable in &self.local_disposables {
            disposable();
        }
    }
}

/// `convertToReaction(reaction)`: os `reactors` que são autores viram os nomes.
fn convert_to_reaction(reaction: &types::CommentReaction) -> CommentReaction {
    CommentReaction {
        label: reaction.label.clone(),
        icon_path: reaction.icon_path.as_ref().filter(|path| !matches!(path, PathOrUri::Path(text) if text.is_empty())).map(path_or_uri_to_uri),
        count: Some(reaction.count),
        has_reacted: Some(reaction.author_has_reacted),
        can_edit: None,
        reactors: reaction.reactors.as_ref().map(|reactors| match reactors {
            CommentReactors::Names(names) => names.clone(),
            CommentReactors::Authors(authors) => authors.iter().map(|author| author.name.clone()).collect(),
        }),
    }
}

/// `convertFromReaction(reaction)`: os valores ausentes viram os padrões (`''`, `0`, `false`).
fn convert_from_reaction(reaction: &CommentReaction) -> types::CommentReaction {
    types::CommentReaction {
        label: Some(reaction.label.clone().unwrap_or_default()),
        count: reaction.count.unwrap_or(0.0),
        icon_path: Some(match &reaction.icon_path {
            Some(icon_path) => PathOrUri::Uri(icon_path.clone()),
            None => PathOrUri::Path(String::new()),
        }),
        author_has_reacted: reaction.has_reacted.unwrap_or(false),
        reactors: reaction
            .reactors
            .as_ref()
            .map(|reactors| CommentReactors::Authors(reactors.iter().map(|name| CommentAuthorInformation { name: name.clone(), icon_path: None }).collect())),
    }
}

/// `convertToCollapsibleState(kind)`: os enums da API e de `languages` têm os mesmos valores; sem valor, a thread
/// fica recolhida.
fn convert_to_collapsible_state(kind: Option<CommentThreadCollapsibleState>) -> CommentThreadCollapsibleState {
    kind.unwrap_or(CommentThreadCollapsibleState::Collapsed)
}

/// `convertToState(kind)`: o `resolved` do objeto, ou o próprio valor; sem valor, não resolvida.
fn convert_to_state(kind: Option<CommentThreadStateValue>) -> CommentThreadState {
    let resolved_kind = match kind {
        Some(CommentThreadStateValue::Info { resolved, .. }) => resolved,
        Some(CommentThreadStateValue::State(state)) => Some(state),
        None => None,
    };
    resolved_kind.unwrap_or(CommentThreadState::Unresolved)
}

/// `convertToRelevance(kind)`: o `applicability` do objeto; sem valor, atual.
fn convert_to_relevance(kind: Option<CommentThreadStateValue>) -> CommentThreadApplicability {
    match kind {
        Some(CommentThreadStateValue::Info { applicability: Some(applicability), .. }) => applicability,
        _ => CommentThreadApplicability::Current,
    }
}
