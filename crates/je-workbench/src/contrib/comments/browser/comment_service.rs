//! Porte de `src/vs/workbench/contrib/comments/browser/commentService.ts`: o serviço de comentários (`ICommentService` e
//! `CommentService`), os eventos, o contrato dos controladores (`ICommentController`), as informações de comentários por
//! documento e por notebook (`ICommentInfo`, `INotebookCommentInfo`), o registro de controladores e o estado
//! `commentsEnabled` e `continueOnComments`.
//!
//! Divergências:
//! - **Construtor.** O `IInstantiationService` só servia a `createInstance(CommentMenus)`, que precisa do `IMenuService`; o
//!   serviço recebe o `IMenuService` direto. O `CommentsModel` que o original cria com `new` é criado em [`CommentService::new`].
//! - **`CommentThread<IRange | ICellRange>`.** A interface do serviço aceita a thread de documento ou de notebook; a
//!   implementação do original só lê threads de documento. A união é o enum [`AnyCommentThread`] onde o serviço apenas repassa a
//!   thread (`setActiveEditingCommentThread`, `setCurrentCommentThread`, `toggleReaction`, `setActiveCommentAndThread` e os eventos
//!   dos dois primeiros). `setWorkspaceComments`, que o modelo lê, recebe as threads de documento.
//!   `{ thread, comment? }` é o [`CommentAndThread`].
//! - **`async`.** As funções `async` devolvem [`Promise`]; os `await` em sequência de `setActiveCommentAndThread` são o
//!   encadeamento de `then_promise`. `Promise.all` é [`Promise::all`], e `.catch(_ => null)` é `catch` devolvendo `None`.
//! - **`removeContinueOnComment`.** O objeto de argumento é a struct [`ContinueOnCommentKey`]; `isReply?` é `Option<bool>`.
//!   O estado `_continueOnComments` fica num `Rc<RefCell<..>>` compartilhado com as promessas de `getDocumentComments`.
//! - **`storageService.getObject(CONTINUE_ON_COMMENTS)`.** O armazenamento devolve `json::Value` e o URI (que o `JSON.parse` do
//!   original deixa como objeto simples) é reconstruído a partir dos componentes; uma entrada malformada é ignorada. O que o
//!   original grava com `store(.., commentsToSave, ..)` sai pelo mesmo caminho inverso (`uri_to_value`).
//! - **Identidade.** Controladores e provedores comparam por endereço (`===`); `Set<IContinueOnCommentProvider>` é uma lista
//!   sem duplicatas.
//! - `ICommentThreadChangedEvent`, `INotebookCommentThreadChangedEvent` e `ICommentInfo` estendem um tipo base: cada uma é uma
//!   struct com o tipo base num campo e `Deref` para ele.
//! - `new Error('Not supported')` de `toggleReaction` é a promessa rejeitada (a função é `async`).

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ops::{Deref, DerefMut};
use std::rc::{Rc, Weak};

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{DebounceOptions, Emitter, Event};
use je_base::json::{Object, Value};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::map::OrderedMap;
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::symbols::Delay;
use je_base::uri::{URI, UriComponents};
use je_editor::languages::{
    Comment, CommentInfo, CommentOptions, CommentReaction, CommentThread, CommentThreadChangedEvent, CommentingRangeResourceHint, CommentingRanges, PendingComment,
    PendingCommentThread,
};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::services_model::IModelService;
use je_instantiation::service_id;
use je_platform::actions::IMenuService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService};
use je_platform::log::{LogService, Logger};
use je_platform::storage::storage::{StorageScope, StorageService, StorageTarget, StorageValueChangeEvent};
use je_storage::StorageValue;
use je_storage::marshalling::uri_to_value;

use crate::contrib::comments::browser::comment_menus::CommentMenus;
use crate::contrib::comments::browser::comments_model::{CommentsModel, ICommentsModel};
use crate::contrib::comments::common::comment_context_keys;
use crate::contrib::comments::common::comment_model::ICommentThreadChangedEvent;
use crate::contrib::comments::common::comments_configuration::{COMMENTS_SECTION, ICommentsConfiguration};
use crate::contrib::notebook::common::notebook_range::ICellRange;
use crate::contrib::scm::common::scm_service::uri_from_value;
use crate::services::layout::layout_service::IWorkbenchLayoutService;

/// `CONTINUE_ON_COMMENTS`.
const CONTINUE_ON_COMMENTS: &str = "comments.continueOnComments";

/// `_continueOnComments`: `uniqueOwner -> PendingCommentThread[]`.
type ContinueOnComments = RefCell<OrderedMap<String, Vec<PendingCommentThread>>>;

/// `CommentThread<IRange | ICellRange>`.
#[derive(Clone)]
pub enum AnyCommentThread {
    Document(Rc<dyn CommentThread<Range>>),
    Notebook(Rc<dyn CommentThread<ICellRange>>),
}

impl From<Rc<dyn CommentThread<Range>>> for AnyCommentThread {
    fn from(thread: Rc<dyn CommentThread<Range>>) -> Self {
        Self::Document(thread)
    }
}

impl From<Rc<dyn CommentThread<ICellRange>>> for AnyCommentThread {
    fn from(thread: Rc<dyn CommentThread<ICellRange>>) -> Self {
        Self::Notebook(thread)
    }
}

/// `{ thread: CommentThread; comment?: Comment }`.
#[derive(Clone)]
pub struct CommentAndThread {
    pub thread: AnyCommentThread,
    pub comment: Option<Comment>,
}

/// `IResourceCommentThreadEvent`.
#[derive(Clone)]
pub struct IResourceCommentThreadEvent {
    pub resource: URI,
    pub comment_infos: Vec<ICommentInfo>,
}

/// `ICommentInfo<T>`: o `CommentInfo<T>` com o dono (`uniqueOwner`) e o rótulo do controlador.
#[derive(Clone)]
pub struct ICommentInfo<T: Clone + 'static = Range> {
    pub info: CommentInfo<T>,
    pub unique_owner: String,
    pub label: Option<String>,
}

impl<T: Clone + 'static> Deref for ICommentInfo<T> {
    type Target = CommentInfo<T>;

    fn deref(&self) -> &Self::Target {
        &self.info
    }
}

impl<T: Clone + 'static> DerefMut for ICommentInfo<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.info
    }
}

/// `INotebookCommentInfo`.
#[derive(Clone)]
pub struct INotebookCommentInfo {
    pub extension_id: Option<String>,
    pub threads: Vec<Rc<dyn CommentThread<ICellRange>>>,
    pub unique_owner: String,
    pub label: Option<String>,
}

/// `IWorkspaceCommentThreadsEvent`.
#[derive(Clone)]
pub struct IWorkspaceCommentThreadsEvent {
    pub owner_id: String,
    pub owner_label: String,
    pub comment_threads: Vec<Rc<dyn CommentThread<Range>>>,
}

/// `INotebookCommentThreadChangedEvent`: o evento base com o dono.
#[derive(Clone)]
pub struct INotebookCommentThreadChangedEvent {
    pub event: CommentThreadChangedEvent<ICellRange>,
    pub unique_owner: String,
}

impl Deref for INotebookCommentThreadChangedEvent {
    type Target = CommentThreadChangedEvent<ICellRange>;

    fn deref(&self) -> &Self::Target {
        &self.event
    }
}

/// `{ uniqueOwner: string }` de `onDidUpdateCommentingRanges`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommentingRangesUpdate {
    pub unique_owner: String,
}

/// `{ range: Range; commentingRangesInfo: CommentingRanges }` de `onDidChangeActiveCommentingRange`.
#[derive(Clone)]
pub struct ActiveCommentingRange {
    pub range: Range,
    pub commenting_ranges_info: CommentingRanges,
}

/// `ICommentController.features`.
#[derive(Clone, Debug, Default)]
pub struct CommentControllerFeatures {
    pub reaction_group: Option<Vec<CommentReaction>>,
    pub reaction_handler: Option<bool>,
    pub options: Option<CommentOptions>,
}

/// `ICommentController`.
pub trait ICommentController {
    fn id(&self) -> String;
    fn label(&self) -> String;
    fn features(&self) -> CommentControllerFeatures;
    fn options(&self) -> Option<CommentOptions>;
    fn context_value(&self) -> Option<String>;
    fn owner(&self) -> String;
    /// `activeComment`.
    fn active_comment(&self) -> Option<CommentAndThread>;
    fn create_comment_thread_template(&self, resource: UriComponents, range: Option<Range>, editor_id: Option<String>) -> Promise<()>;
    fn update_comment_thread_template(&self, thread_handle: i64, range: Range) -> Promise<()>;
    fn delete_comment_thread_main(&self, comment_thread_id: &str);
    fn toggle_reaction(&self, uri: &URI, thread: &AnyCommentThread, comment: &Comment, reaction: &CommentReaction, token: CancellationToken) -> Promise<()>;
    fn get_document_comments(&self, resource: &URI, token: CancellationToken) -> Promise<ICommentInfo<Range>>;
    fn get_notebook_comments(&self, resource: &URI, token: CancellationToken) -> Promise<INotebookCommentInfo>;
    fn set_active_comment_and_thread(&self, comment_info: Option<CommentAndThread>) -> Promise<()>;
}

/// `IContinueOnCommentProvider`.
pub trait IContinueOnCommentProvider {
    fn provide_continue_on_comments(&self) -> Vec<PendingCommentThread>;
}

/// O argumento de `removeContinueOnComment`: `{ range, uri, uniqueOwner, isReply? }`.
#[derive(Clone, Debug)]
pub struct ContinueOnCommentKey {
    pub range: Option<Range>,
    pub uri: URI,
    pub unique_owner: String,
    pub is_reply: Option<bool>,
}

/// `ICommentService`.
pub trait ICommentService {
    fn on_did_set_resource_comment_infos(&self) -> Event<IResourceCommentThreadEvent>;
    fn on_did_set_all_comment_threads(&self) -> Event<IWorkspaceCommentThreadsEvent>;
    fn on_did_update_comment_threads(&self) -> Event<ICommentThreadChangedEvent>;
    fn on_did_update_notebook_comment_threads(&self) -> Event<INotebookCommentThreadChangedEvent>;
    fn on_did_change_active_editing_comment_thread(&self) -> Event<Option<AnyCommentThread>>;
    fn on_did_change_current_comment_thread(&self) -> Event<Option<AnyCommentThread>>;
    fn on_did_update_commenting_ranges(&self) -> Event<CommentingRangesUpdate>;
    fn on_did_change_active_commenting_range(&self) -> Event<ActiveCommentingRange>;
    fn on_did_set_data_provider(&self) -> Event<()>;
    fn on_did_delete_data_provider(&self) -> Event<Option<String>>;
    fn on_did_change_commenting_enabled(&self) -> Event<bool>;
    fn on_resource_has_commenting_ranges(&self) -> Event<()>;
    fn is_commenting_enabled(&self) -> bool;
    fn comments_model(&self) -> Rc<dyn ICommentsModel>;
    /// `lastActiveCommentcontroller`.
    fn last_active_comment_controller(&self) -> Option<Rc<dyn ICommentController>>;
    fn set_document_comments(&self, resource: &URI, comment_infos: Vec<ICommentInfo>);
    fn set_workspace_comments(&self, unique_owner: &str, comments_by_resource: Vec<Rc<dyn CommentThread<Range>>>);
    fn remove_workspace_comments(&self, unique_owner: &str);
    fn register_comment_controller(&self, unique_owner: &str, comment_control: Rc<dyn ICommentController>);
    fn unregister_comment_controller(&self, unique_owner: Option<&str>);
    fn get_comment_controller(&self, unique_owner: &str) -> Option<Rc<dyn ICommentController>>;
    fn create_comment_thread_template(&self, unique_owner: &str, resource: &URI, range: Option<Range>, editor_id: Option<String>) -> Promise<()>;
    fn update_comment_thread_template(&self, unique_owner: &str, thread_handle: i64, range: Range) -> Promise<()>;
    fn get_comment_menus(&self, unique_owner: &str) -> Rc<CommentMenus>;
    fn update_comments(&self, owner_id: &str, event: CommentThreadChangedEvent<Range>);
    fn update_notebook_comments(&self, owner_id: &str, event: CommentThreadChangedEvent<ICellRange>);
    fn dispose_comment_thread(&self, owner_id: &str, thread_id: &str);
    fn get_document_comments(&self, resource: &URI) -> Promise<Vec<Option<ICommentInfo>>>;
    fn get_notebook_comments(&self, resource: &URI) -> Promise<Vec<Option<INotebookCommentInfo>>>;
    fn update_commenting_ranges(&self, owner_id: &str, resource_hints: Option<&CommentingRangeResourceHint>);
    fn has_reaction_handler(&self, unique_owner: &str) -> bool;
    fn toggle_reaction(&self, unique_owner: &str, resource: &URI, thread: &AnyCommentThread, comment: &Comment, reaction: &CommentReaction) -> Promise<()>;
    fn set_active_editing_comment_thread(&self, comment_thread: Option<AnyCommentThread>);
    fn set_current_comment_thread(&self, comment_thread: Option<AnyCommentThread>);
    fn set_active_comment_and_thread(&self, unique_owner: &str, comment_info: Option<CommentAndThread>) -> Promise<()>;
    fn enable_commenting(&self, enable: bool);
    fn register_continue_on_comment_provider(&self, provider: Rc<dyn IContinueOnCommentProvider>) -> DisposableRef;
    fn remove_continue_on_comment(&self, pending_comment: &ContinueOnCommentKey) -> Option<PendingCommentThread>;
    fn resource_has_commenting_ranges(&self, resource: &URI) -> bool;
}

service_id!(pub I_COMMENT_SERVICE: dyn ICommentService = "commentService");

/// `uri.toString()`.
fn uri_key(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

/// `commentController !== this._lastActiveCommentController`: o `===` entre controladores é a identidade do objeto.
fn same_controller(a: &Rc<dyn ICommentController>, b: &Rc<dyn ICommentController>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `Set<IContinueOnCommentProvider>`: a identidade do provedor.
fn same_provider(a: &Rc<dyn IContinueOnCommentProvider>, b: &Rc<dyn IContinueOnCommentProvider>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `this._defaultCommentingEnablement`: `!!configurationService.getValue<ICommentsConfiguration | undefined>(COMMENTS_SECTION)?.visible`.
fn default_commenting_enablement(configuration_service: &dyn IConfigurationService) -> bool {
    ICommentsConfiguration::from_dynamic(&configuration_service.get_value(Some(COMMENTS_SECTION), None)).is_some_and(|configuration| configuration.visible)
}

/// `removeContinueOnComment(pendingComment)`.
fn remove_continue_on_comment_from(continue_on_comments: &ContinueOnComments, pending_comment: &ContinueOnCommentKey) -> Option<PendingCommentThread> {
    let mut map = continue_on_comments.borrow_mut();
    let pending_comments = map.get_mut(&pending_comment.unique_owner)?;
    let uri = uri_key(&pending_comment.uri);
    let comment_index = pending_comments.iter().position(|comment| {
        uri_key(&comment.uri) == uri && comment.range == pending_comment.range && pending_comment.is_reply.is_none_or(|is_reply| comment.is_reply == is_reply)
    })?;
    Some(pending_comments.remove(comment_index))
}

/// `_addContinueOnComments(pendingComments, map)`: devolve os donos que mudaram.
fn add_continue_on_comments(pending_comments: &[PendingCommentThread], map: &mut OrderedMap<String, Vec<PendingCommentThread>>) -> Vec<String> {
    let mut changed_owners: Vec<String> = Vec::new();
    let mut mark_changed = |owner: &str| {
        if !changed_owners.iter().any(|changed| changed == owner) {
            changed_owners.push(owner.to_owned());
        }
    };
    for pending_comment in pending_comments {
        match map.get_mut(&pending_comment.unique_owner) {
            None => {
                map.insert(pending_comment.unique_owner.clone(), vec![pending_comment.clone()]);
                mark_changed(&pending_comment.unique_owner);
            }
            Some(comments_for_owner) => {
                let uri = uri_key(&pending_comment.uri);
                if comments_for_owner.iter().all(|comment| uri_key(&comment.uri) != uri || comment.range != pending_comment.range) {
                    comments_for_owner.push(pending_comment.clone());
                    mark_changed(&pending_comment.unique_owner);
                }
            }
        }
    }
    changed_owners
}

/// O `IRange` do JSON do armazenamento.
fn range_to_value(range: &Range) -> Value {
    Value::Object(
        [
            ("startLineNumber", range.start_line_number),
            ("startColumn", range.start_column),
            ("endLineNumber", range.end_line_number),
            ("endColumn", range.end_column),
        ]
        .into_iter()
        .map(|(key, value)| (key, Value::Number(f64::from(value))))
        .collect::<Object>(),
    )
}

fn number_field(object: &Object, key: &str) -> Option<i32> {
    match object.get(key) {
        Some(Value::Number(number)) => Some(*number as i32),
        _ => None,
    }
}

fn text_field(object: &Object, key: &str) -> Option<String> {
    match object.get(key) {
        Some(Value::String(text)) => Some(text.to_string_lossy()),
        _ => None,
    }
}

fn range_from_value(value: &Value) -> Option<Range> {
    let Value::Object(object) = value else { return None };
    Some(Range {
        start_line_number: number_field(object, "startLineNumber")?,
        start_column: number_field(object, "startColumn")?,
        end_line_number: number_field(object, "endLineNumber")?,
        end_column: number_field(object, "endColumn")?,
    })
}

/// A forma de `JSON.stringify(PendingCommentThread)`.
fn pending_comment_thread_to_value(thread: &PendingCommentThread) -> Value {
    let mut object = Object::new();
    if let Some(range) = &thread.range {
        object.insert("range".into(), range_to_value(range));
    }
    object.insert("uri".into(), uri_to_value(&thread.uri));
    object.insert("uniqueOwner".into(), Value::from(thread.unique_owner.as_str()));
    object.insert("isReply".into(), Value::Bool(thread.is_reply));

    let mut cursor = Object::new();
    cursor.insert("lineNumber".into(), Value::Number(f64::from(thread.comment.cursor.line_number)));
    cursor.insert("column".into(), Value::Number(f64::from(thread.comment.cursor.column)));
    let mut comment = Object::new();
    comment.insert("body".into(), Value::from(thread.comment.body.as_str()));
    comment.insert("cursor".into(), Value::Object(cursor));
    object.insert("comment".into(), Value::Object(comment));

    Value::Object(object)
}

/// O `PendingCommentThread` do JSON do armazenamento; uma entrada malformada é `None`.
fn pending_comment_thread_from_value(value: &Value) -> Option<PendingCommentThread> {
    let Value::Object(object) = value else { return None };
    let range = match object.get("range") {
        None | Some(Value::Null) => None,
        Some(range) => Some(range_from_value(range)?),
    };
    let Some(Value::Object(comment)) = object.get("comment") else { return None };
    let Some(Value::Object(cursor)) = comment.get("cursor") else { return None };
    Some(PendingCommentThread {
        range,
        uri: uri_from_value(object.get("uri")?)?,
        unique_owner: text_field(object, "uniqueOwner")?,
        is_reply: matches!(object.get("isReply"), Some(Value::Bool(true))),
        comment: PendingComment {
            body: text_field(comment, "body")?,
            cursor: Position { line_number: number_field(cursor, "lineNumber")?, column: number_field(cursor, "column")? },
        },
    })
}

/// Um emissor que morre junto com o `base`.
fn register_emitter<T: 'static>(base: &Disposable) -> Rc<Emitter<T>> {
    let emitter = Emitter::new();
    base.register(emitter.clone());
    emitter
}

/// `CommentService`.
pub struct CommentService {
    me: Weak<CommentService>,
    base: Rc<Disposable>,

    menu_service: Rc<dyn IMenuService>,
    layout_service: Rc<dyn IWorkbenchLayoutService>,
    configuration_service: Rc<dyn IConfigurationService>,
    storage_service: Rc<dyn StorageService>,
    log_service: Rc<dyn LogService>,
    model_service: Rc<dyn IModelService>,

    on_did_set_data_provider: Rc<Emitter<()>>,
    on_did_delete_data_provider: Rc<Emitter<Option<String>>>,
    on_did_set_resource_comment_infos: Rc<Emitter<IResourceCommentThreadEvent>>,
    on_did_set_all_comment_threads: Rc<Emitter<IWorkspaceCommentThreadsEvent>>,
    on_did_update_comment_threads: Rc<Emitter<ICommentThreadChangedEvent>>,
    on_did_update_notebook_comment_threads: Rc<Emitter<INotebookCommentThreadChangedEvent>>,
    on_did_update_commenting_ranges: Rc<Emitter<CommentingRangesUpdate>>,
    on_did_change_active_editing_comment_thread: Rc<Emitter<Option<AnyCommentThread>>>,
    on_did_change_current_comment_thread: Rc<Emitter<Option<AnyCommentThread>>>,
    on_did_change_commenting_enabled: Rc<Emitter<bool>>,
    on_resource_has_commenting_ranges: Rc<Emitter<()>>,
    on_did_change_active_commenting_range: Rc<Emitter<ActiveCommentingRange>>,

    comment_controls: RefCell<OrderedMap<String, Rc<dyn ICommentController>>>,
    comment_menus: RefCell<OrderedMap<String, Rc<CommentMenus>>>,
    is_commenting_enabled: Cell<bool>,
    workspace_has_commenting: Rc<dyn IContextKey>,
    commenting_enabled: Rc<dyn IContextKey>,

    /// `uniqueOwner -> PendingCommentThread[]`
    continue_on_comments: Rc<ContinueOnComments>,
    continue_on_comment_providers: Rc<RefCell<Vec<Rc<dyn IContinueOnCommentProvider>>>>,

    comments_model: Rc<CommentsModel>,

    /// URIs
    commenting_range_resources: Rc<RefCell<HashSet<String>>>,
    /// schemes
    commenting_range_resource_hint_schemes: RefCell<HashSet<String>>,

    last_active_comment_controller: Rc<RefCell<Option<Rc<dyn ICommentController>>>>,
}

impl CommentService {
    pub fn new(
        menu_service: Rc<dyn IMenuService>,
        layout_service: Rc<dyn IWorkbenchLayoutService>,
        configuration_service: Rc<dyn IConfigurationService>,
        context_key_service: Rc<dyn IContextKeyService>,
        storage_service: Rc<dyn StorageService>,
        log_service: Rc<dyn LogService>,
        model_service: Rc<dyn IModelService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let comments_model = CommentsModel::new();
        base.register(comments_model.clone());

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            // `_handleConfiguration`: `this._isCommentingEnabled = this._defaultCommentingEnablement`.
            is_commenting_enabled: Cell::new(default_commenting_enablement(&*configuration_service)),
            workspace_has_commenting: comment_context_keys::workspace_has_commenting().bind_to(&*context_key_service),
            commenting_enabled: comment_context_keys::commenting_enabled().bind_to(&*context_key_service),
            on_did_set_data_provider: register_emitter(&base),
            on_did_delete_data_provider: register_emitter(&base),
            on_did_set_resource_comment_infos: register_emitter(&base),
            on_did_set_all_comment_threads: register_emitter(&base),
            on_did_update_comment_threads: register_emitter(&base),
            on_did_update_notebook_comment_threads: register_emitter(&base),
            on_did_update_commenting_ranges: register_emitter(&base),
            on_did_change_active_editing_comment_thread: register_emitter(&base),
            on_did_change_current_comment_thread: register_emitter(&base),
            on_did_change_commenting_enabled: register_emitter(&base),
            on_resource_has_commenting_ranges: register_emitter(&base),
            on_did_change_active_commenting_range: register_emitter(&base),
            base,
            menu_service,
            layout_service,
            configuration_service,
            storage_service,
            log_service,
            model_service,
            comment_controls: RefCell::new(OrderedMap::new()),
            comment_menus: RefCell::new(OrderedMap::new()),
            continue_on_comments: Rc::new(RefCell::new(OrderedMap::new())),
            continue_on_comment_providers: Rc::new(RefCell::new(Vec::new())),
            comments_model,
            commenting_range_resources: Rc::new(RefCell::new(HashSet::new())),
            commenting_range_resource_hint_schemes: RefCell::new(HashSet::new()),
            last_active_comment_controller: Rc::new(RefCell::new(None)),
        });
        this.handle_configuration();
        this.handle_zen_mode();
        this.handle_continue_on_comments();
        this.handle_model_added();
        this
    }

    /// `_handleConfiguration()`: o valor inicial entra no construtor; aqui fica o ouvinte.
    fn handle_configuration(&self) {
        let me = self.me.clone();
        self.base.register(self.configuration_service.on_did_change_configuration().listen(move |event| {
            if event.affects_configuration("comments.visible", None) {
                if let Some(this) = me.upgrade() {
                    let enable = default_commenting_enablement(&*this.configuration_service);
                    this.enable_commenting(enable);
                }
            }
        }));
    }

    /// `_handleZenMode()`.
    fn handle_zen_mode(&self) {
        let pre_zen_mode_value = Rc::new(Cell::new(self.is_commenting_enabled.get()));
        let me = self.me.clone();
        self.base.register(self.layout_service.on_did_change_zen_mode().listen(move |entered| {
            let Some(this) = me.upgrade() else { return };
            if *entered {
                pre_zen_mode_value.set(this.is_commenting_enabled.get());
                this.enable_commenting(false);
            } else {
                this.enable_commenting(pre_zen_mode_value.get());
            }
        }));
    }

    /// O restante do construtor que trata o `continueOnComments`: o ouvinte do armazenamento (com `debounce` de 500 ms) e o de
    /// `onWillSaveState`.
    fn handle_continue_on_comments(&self) {
        let storage_listener = DisposableStore::new();
        self.base.register(storage_listener.clone());

        let storage_event = self.storage_service.on_did_change_value(StorageScope::Workspace, Some(CONTINUE_ON_COMMENTS), &storage_listener).debounce(
            |last: Option<StorageValueChangeEvent>, event: &StorageValueChangeEvent| match last {
                Some(last) if last.external => last,
                _ => event.clone(),
            },
            DebounceOptions { delay: Delay::Millis(500), ..DebounceOptions::default() },
            Some(&*storage_listener),
        );
        let me = self.me.clone();
        storage_listener.add(storage_event.listen(move |change| {
            if !change.external {
                return;
            }
            if let Some(this) = me.upgrade() {
                this.restore_continue_on_comments();
            }
        }));

        let me = self.me.clone();
        self.base.register(self.storage_service.on_will_save_state().listen(move |_| {
            let Some(this) = me.upgrade() else { return };
            let mut map = OrderedMap::new();
            let providers: Vec<Rc<dyn IContinueOnCommentProvider>> = this.continue_on_comment_providers.borrow().clone();
            for provider in providers {
                let pending_comments = provider.provide_continue_on_comments();
                add_continue_on_comments(&pending_comments, &mut map);
            }
            this.save_continue_on_comments(&map);
        }));
    }

    /// O corpo do ouvinte do armazenamento: lê os comentários gravados por outra janela e atualiza o modelo dos donos que mudaram.
    fn restore_continue_on_comments(&self) {
        let Some(Value::Array(entries)) = self.storage_service.get_object(CONTINUE_ON_COMMENTS, StorageScope::Workspace, None) else {
            return;
        };
        let comments_to_restore: Vec<PendingCommentThread> = entries.iter().filter_map(pending_comment_thread_from_value).collect();
        let uris: Vec<String> = comments_to_restore.iter().map(|thread| uri_key(&thread.uri)).collect();
        self.log_service.debug(&format!("Comments: URIs of continue on comments from storage {}.", uris.join(", ")));

        let changed_owners = add_continue_on_comments(&comments_to_restore, &mut self.continue_on_comments.borrow_mut());
        for unique_owner in changed_owners {
            let Some(control) = self.comment_controls.borrow().get(&unique_owner).cloned() else {
                continue;
            };
            let pending = self.continue_on_comments.borrow().get(&unique_owner).cloned().unwrap_or_default();
            self.update_model_threads(ICommentThreadChangedEvent {
                event: CommentThreadChangedEvent { pending, added: Vec::new(), removed: Vec::new(), changed: Vec::new() },
                unique_owner,
                owner: control.owner(),
                owner_label: control.label(),
            });
        }
    }

    /// O ouvinte de `modelService.onModelAdded`.
    fn handle_model_added(&self) {
        let me = self.me.clone();
        self.base.register(self.model_service.on_model_added().listen(move |model| {
            let uri = model.uri();
            // Excluded schemes
            if uri.scheme().to_string_lossy() == schemas::VSCODE_SOURCE_CONTROL {
                return;
            }
            let Some(this) = me.upgrade() else { return };
            // Allows comment providers to cause their commenting ranges to be prefetched by opening text documents in the background.
            let known = this.commenting_range_resources.borrow().contains(&uri_key(&uri));
            if !known {
                // O original não espera a promessa.
                let _ = ICommentService::get_document_comments(&*this, &uri);
            }
        }));
    }

    /// `setModelThreads(ownerId, owner, ownerLabel, commentThreads)`.
    fn set_model_threads(&self, owner_id: &str, owner: &str, owner_label: &str, comment_threads: Vec<Rc<dyn CommentThread<Range>>>) {
        self.comments_model.set_comment_threads(owner_id, owner, owner_label, &comment_threads);
        self.on_did_set_all_comment_threads.fire(IWorkspaceCommentThreadsEvent { owner_id: owner_id.to_owned(), owner_label: owner_label.to_owned(), comment_threads });
    }

    /// `updateModelThreads(event)`.
    fn update_model_threads(&self, event: ICommentThreadChangedEvent) {
        self.comments_model.update_comment_threads(&event);
        self.on_did_update_comment_threads.fire(event);
    }

    /// `_saveContinueOnComments(map)`.
    fn save_continue_on_comments(&self, map: &OrderedMap<String, Vec<PendingCommentThread>>) {
        let comments_to_save: Vec<PendingCommentThread> = map.values().flatten().cloned().collect();
        let uris: Vec<String> = comments_to_save.iter().map(|thread| uri_key(&thread.uri)).collect();
        self.log_service.debug(&format!("Comments: URIs of continue on comments to add to storage {}.", uris.join(", ")));
        self.storage_service.store(
            CONTINUE_ON_COMMENTS,
            StorageValue::Object(Value::Array(comments_to_save.iter().map(pending_comment_thread_to_value).collect())),
            StorageScope::Workspace,
            StorageTarget::User,
        );
    }

    /// O controlador registrado para o dono.
    fn controller(&self, unique_owner: &str) -> Option<Rc<dyn ICommentController>> {
        self.comment_controls.borrow().get(&unique_owner.to_owned()).cloned()
    }
}

impl ICommentService for CommentService {
    fn on_did_set_resource_comment_infos(&self) -> Event<IResourceCommentThreadEvent> {
        self.on_did_set_resource_comment_infos.event()
    }

    fn on_did_set_all_comment_threads(&self) -> Event<IWorkspaceCommentThreadsEvent> {
        self.on_did_set_all_comment_threads.event()
    }

    fn on_did_update_comment_threads(&self) -> Event<ICommentThreadChangedEvent> {
        self.on_did_update_comment_threads.event()
    }

    fn on_did_update_notebook_comment_threads(&self) -> Event<INotebookCommentThreadChangedEvent> {
        self.on_did_update_notebook_comment_threads.event()
    }

    fn on_did_change_active_editing_comment_thread(&self) -> Event<Option<AnyCommentThread>> {
        self.on_did_change_active_editing_comment_thread.event()
    }

    fn on_did_change_current_comment_thread(&self) -> Event<Option<AnyCommentThread>> {
        self.on_did_change_current_comment_thread.event()
    }

    fn on_did_update_commenting_ranges(&self) -> Event<CommentingRangesUpdate> {
        self.on_did_update_commenting_ranges.event()
    }

    fn on_did_change_active_commenting_range(&self) -> Event<ActiveCommentingRange> {
        self.on_did_change_active_commenting_range.event()
    }

    fn on_did_set_data_provider(&self) -> Event<()> {
        self.on_did_set_data_provider.event()
    }

    fn on_did_delete_data_provider(&self) -> Event<Option<String>> {
        self.on_did_delete_data_provider.event()
    }

    fn on_did_change_commenting_enabled(&self) -> Event<bool> {
        self.on_did_change_commenting_enabled.event()
    }

    fn on_resource_has_commenting_ranges(&self) -> Event<()> {
        self.on_resource_has_commenting_ranges.event()
    }

    fn is_commenting_enabled(&self) -> bool {
        self.is_commenting_enabled.get()
    }

    fn comments_model(&self) -> Rc<dyn ICommentsModel> {
        self.comments_model.clone()
    }

    fn last_active_comment_controller(&self) -> Option<Rc<dyn ICommentController>> {
        self.last_active_comment_controller.borrow().clone()
    }

    fn enable_commenting(&self, enable: bool) {
        if enable != self.is_commenting_enabled.get() {
            self.is_commenting_enabled.set(enable);
            self.commenting_enabled.set(Dynamic::Bool(enable));
            self.on_did_change_commenting_enabled.fire(enable);
        }
    }

    /// The current comment thread is the thread that has focus or is being hovered.
    fn set_current_comment_thread(&self, comment_thread: Option<AnyCommentThread>) {
        self.on_did_change_current_comment_thread.fire(comment_thread);
    }

    /// The active comment thread is the thread that is currently being edited.
    fn set_active_editing_comment_thread(&self, comment_thread: Option<AnyCommentThread>) {
        self.on_did_change_active_editing_comment_thread.fire(comment_thread);
    }

    fn set_active_comment_and_thread(&self, unique_owner: &str, comment_info: Option<CommentAndThread>) -> Promise<()> {
        let Some(comment_controller) = self.controller(unique_owner) else {
            return Promise::resolved(());
        };

        let last_active = self.last_active_comment_controller.clone();
        let previous = last_active.borrow().clone();
        let activate = {
            let comment_controller = comment_controller.clone();
            move || {
                *last_active.borrow_mut() = Some(comment_controller.clone());
                comment_controller.set_active_comment_and_thread(comment_info)
            }
        };

        match previous {
            Some(previous) if !same_controller(&previous, &comment_controller) => previous.set_active_comment_and_thread(None).then_promise(move |_| activate()),
            _ => activate(),
        }
    }

    fn set_document_comments(&self, resource: &URI, comment_infos: Vec<ICommentInfo>) {
        self.on_did_set_resource_comment_infos.fire(IResourceCommentThreadEvent { resource: resource.clone(), comment_infos });
    }

    fn set_workspace_comments(&self, unique_owner: &str, comments_by_resource: Vec<Rc<dyn CommentThread<Range>>>) {
        if !comments_by_resource.is_empty() {
            self.workspace_has_commenting.set(Dynamic::Bool(true));
        }
        if let Some(control) = self.controller(unique_owner) {
            self.set_model_threads(unique_owner, &control.owner(), &control.label(), comments_by_resource);
        }
    }

    fn remove_workspace_comments(&self, unique_owner: &str) {
        if let Some(control) = self.controller(unique_owner) {
            self.set_model_threads(unique_owner, &control.owner(), &control.label(), Vec::new());
        }
    }

    fn register_comment_controller(&self, unique_owner: &str, comment_control: Rc<dyn ICommentController>) {
        self.comment_controls.borrow_mut().insert(unique_owner.to_owned(), comment_control);
        self.on_did_set_data_provider.fire(());
    }

    fn unregister_comment_controller(&self, unique_owner: Option<&str>) {
        match unique_owner.filter(|owner| !owner.is_empty()) {
            Some(unique_owner) => {
                self.comment_controls.borrow_mut().remove(&unique_owner.to_owned());
            }
            None => self.comment_controls.borrow_mut().clear(),
        }
        self.comments_model.delete_comments_by_owner(unique_owner);
        self.on_did_delete_data_provider.fire(unique_owner.map(str::to_owned));
    }

    fn get_comment_controller(&self, unique_owner: &str) -> Option<Rc<dyn ICommentController>> {
        self.controller(unique_owner)
    }

    fn create_comment_thread_template(&self, unique_owner: &str, resource: &URI, range: Option<Range>, editor_id: Option<String>) -> Promise<()> {
        match self.controller(unique_owner) {
            Some(comment_controller) => comment_controller.create_comment_thread_template(resource.to_json().components, range, editor_id),
            None => Promise::resolved(()),
        }
    }

    fn update_comment_thread_template(&self, unique_owner: &str, thread_handle: i64, range: Range) -> Promise<()> {
        match self.controller(unique_owner) {
            Some(comment_controller) => comment_controller.update_comment_thread_template(thread_handle, range),
            None => Promise::resolved(()),
        }
    }

    fn dispose_comment_thread(&self, unique_owner: &str, thread_id: &str) {
        if let Some(controller) = self.get_comment_controller(unique_owner) {
            controller.delete_comment_thread_main(thread_id);
        }
    }

    fn get_comment_menus(&self, unique_owner: &str) -> Rc<CommentMenus> {
        let existing = self.comment_menus.borrow().get(&unique_owner.to_owned()).cloned();
        if let Some(existing) = existing {
            return existing;
        }

        let menu = Rc::new(CommentMenus::new(self.menu_service.clone()));
        self.comment_menus.borrow_mut().insert(unique_owner.to_owned(), menu.clone());
        menu
    }

    fn update_comments(&self, owner_id: &str, event: CommentThreadChangedEvent<Range>) {
        if let Some(control) = self.controller(owner_id) {
            self.update_model_threads(ICommentThreadChangedEvent { event, unique_owner: owner_id.to_owned(), owner_label: control.label(), owner: control.owner() });
        }
    }

    fn update_notebook_comments(&self, owner_id: &str, event: CommentThreadChangedEvent<ICellRange>) {
        self.on_did_update_notebook_comment_threads.fire(INotebookCommentThreadChangedEvent { event, unique_owner: owner_id.to_owned() });
    }

    fn update_commenting_ranges(&self, owner_id: &str, resource_hints: Option<&CommentingRangeResourceHint>) {
        if let Some(resource_hints) = resource_hints {
            let mut schemes = self.commenting_range_resource_hint_schemes.borrow_mut();
            for scheme in &resource_hints.schemes {
                schemes.insert(scheme.clone());
            }
        }
        self.workspace_has_commenting.set(Dynamic::Bool(true));
        self.on_did_update_commenting_ranges.fire(CommentingRangesUpdate { unique_owner: owner_id.to_owned() });
    }

    fn toggle_reaction(&self, unique_owner: &str, resource: &URI, thread: &AnyCommentThread, comment: &Comment, reaction: &CommentReaction) -> Promise<()> {
        match self.controller(unique_owner) {
            Some(comment_controller) => comment_controller.toggle_reaction(resource, thread, comment, reaction, CancellationToken::None),
            None => Promise::rejected(Error::new("Not supported")),
        }
    }

    fn has_reaction_handler(&self, unique_owner: &str) -> bool {
        self.controller(unique_owner).is_some_and(|comment_provider| comment_provider.features().reaction_handler.unwrap_or(false))
    }

    fn get_document_comments(&self, resource: &URI) -> Promise<Vec<Option<ICommentInfo>>> {
        let controls: Vec<Rc<dyn ICommentController>> = self.comment_controls.borrow().values().cloned().collect();

        let mut comment_control_result: Vec<Promise<Option<ICommentInfo>>> = Vec::new();
        for control in controls {
            let continue_on_comments = self.continue_on_comments.clone();
            let resource = resource.clone();
            comment_control_result.push(
                control
                    .get_document_comments(&resource, CancellationToken::None)
                    .then(move |mut document_comments| {
                        // Check that there aren't any continue on comments in the provided comments
                        // This can happen because continue on comments are stored separately from local un-submitted comments.
                        for document_comment_thread in &document_comments.threads {
                            if document_comment_thread.comments().is_some_and(|comments| comments.is_empty()) {
                                if let Some(range) = document_comment_thread.range() {
                                    remove_continue_on_comment_from(
                                        &continue_on_comments,
                                        &ContinueOnCommentKey { range: Some(range), uri: resource.clone(), unique_owner: document_comments.unique_owner.clone(), is_reply: None },
                                    );
                                }
                            }
                        }
                        let resource_text = uri_key(&resource);
                        document_comments.pending_comment_threads = continue_on_comments
                            .borrow()
                            .get(&document_comments.unique_owner)
                            .map(|pending_comments| pending_comments.iter().filter(|pending_comment| uri_key(&pending_comment.uri) == resource_text).cloned().collect());
                        Some(document_comments)
                    })
                    .catch(|_| None),
            );
        }

        // `_updateResourcesWithCommentingRanges(resource, commentInfos)`
        let commenting_range_resources = self.commenting_range_resources.clone();
        let on_resource_has_commenting_ranges = self.on_resource_has_commenting_ranges.clone();
        let resource = resource.clone();
        Promise::all(comment_control_result).then(move |comment_infos| {
            let mut added_resources = false;
            for comments in comment_infos.iter().flatten() {
                if !comments.commenting_ranges.ranges.is_empty() || !comments.threads.is_empty() {
                    commenting_range_resources.borrow_mut().insert(uri_key(&resource));
                    added_resources = true;
                }
            }
            if added_resources {
                on_resource_has_commenting_ranges.fire(());
            }
            comment_infos
        })
    }

    fn get_notebook_comments(&self, resource: &URI) -> Promise<Vec<Option<INotebookCommentInfo>>> {
        let controls: Vec<Rc<dyn ICommentController>> = self.comment_controls.borrow().values().cloned().collect();
        let comment_control_result: Vec<Promise<Option<INotebookCommentInfo>>> =
            controls.iter().map(|control| control.get_notebook_comments(resource, CancellationToken::None).then(Some).catch(|_| None)).collect();

        Promise::all(comment_control_result)
    }

    fn register_continue_on_comment_provider(&self, provider: Rc<dyn IContinueOnCommentProvider>) -> DisposableRef {
        let providers = self.continue_on_comment_providers.clone();
        if !providers.borrow().iter().any(|registered| same_provider(registered, &provider)) {
            providers.borrow_mut().push(provider.clone());
        }
        to_disposable(move || providers.borrow_mut().retain(|registered| !same_provider(registered, &provider)))
    }

    fn remove_continue_on_comment(&self, pending_comment: &ContinueOnCommentKey) -> Option<PendingCommentThread> {
        remove_continue_on_comment_from(&self.continue_on_comments, pending_comment)
    }

    fn resource_has_commenting_ranges(&self, resource: &URI) -> bool {
        self.commenting_range_resource_hint_schemes.borrow().contains(&resource.scheme().to_string_lossy()) || self.commenting_range_resources.borrow().contains(&uri_key(resource))
    }
}

impl IDisposable for CommentService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
