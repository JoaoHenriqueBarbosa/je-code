//! Porte de `src/vs/platform/userDataSync/common/abstractSynchronizer.ts`: a base de todos os sincronizadores
//! de recursos (`AbstractSynchroniser`, `AbstractFileSynchroniser`, `AbstractJsonFileSynchroniser`) e o
//! `AbstractInitializer`.
//!
//! Decisões de tradução:
//! - **Classes abstratas.** O Rust não tem herança: o que o original deixa abstrato em `AbstractSynchroniser`
//!   (`version`, `generateSyncPreview`, `getMergeResult`, `getAcceptResult`, `applyResult`, `hasRemoteChanged`,
//!   `hasLocalData`, `resolveContent`) é o trait [`SynchroniserImpl`]; a base é a struct
//!   [`AbstractSynchroniser`], genérica no tipo que o implementa, e o sincronizador concreto guarda um
//!   `Rc<AbstractSynchroniser<Self>>`, ligado com [`AbstractSynchroniser::bind`], e dá `Deref` para ele
//!   (o que o original herda). `AbstractFileSynchroniser` e `AbstractJsonFileSynchroniser` são camadas de
//!   `Deref` do mesmo jeito. `AbstractInitializer` usa o trait [`InitializerImpl`].
//! - **Tipos do subtipo.** O original passa o preview e o resultado de aceitar como os tipos base, e cada
//!   sincronizador os lê com tipos mais ricos (`IUserDataProfilesManifestResourcePreview`, o resultado com
//!   `local` e `remote`). Aqui [`SynchroniserImpl::Preview`] e [`SynchroniserImpl::Accept`] são os campos
//!   a mais, carregados em `extra` por [`IResourcePreview`], [`IAcceptResult`] e [`IMergeResult`]. O
//!   `acceptResult` dos recursos sem mudança não tem campos do subtipo no original; [`Default`] é o valor
//!   vazio de [`SynchroniserImpl::Accept`].
//! - **Métodos `async`** devolvem [`Promise`] e rodam com `async_fn`: o corpo roda já até o primeiro `await`,
//!   como no JavaScript. `try { } finally { }` é o `Result` guardado e o fim executado antes de devolvê-lo.
//!   Os métodos `public` de [`IUserDataSynchroniser`] (`sync`, `accept`, `apply`, `replace`...) são os do
//!   trait, implementado pela base, então quem os chama precisa do trait no escopo.
//! - **Os serviços injetados** (`@IFileService` e companhia) chegam num [`SynchroniserServices`]; o
//!   `IUriIdentityService.extUri` é o [`ExtUri`] dele.
//! - **JSON.** `JSON.parse` é [`parse_json`] (sem comentários nem vírgula final, como o do motor); o que o
//!   original guarda como objeto sem tipo (o estado da última sincronização, `ISyncData`, `IRemoteUserData`) é
//!   lido e escrito como [`Value`] na mesma ordem de chaves. O `version` do estado, que o original deixa
//!   `string | undefined`, só é lido quando é texto.
//! - `logService.error(error, retrial)` é `error_args` com o erro como mensagem e o número como argumento.
//! - `parseSyncData` de `AbstractSynchroniser` nunca devolve falso (lança), então o `if (!syncData)` de
//!   `replace` não existe aqui; o de `AbstractInitializer.initialize` existe porque lá ele devolve `undefined`.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::arrays::equals;
use je_base::async_::{ThrottledDelayer, async_fn, create_cancelable_promise};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::json::{self, Object, ParseOptions, Value};
use je_base::json_formatter::{FormattingOptions, json_stringify};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::parts::request::IHeaders;
use je_base::promise::{CancelablePromise, Promise};
use je_base::resources::ExtUri;
use je_base::runtime::date_now;
use je_base::strings::uppercase_first_letter;
use je_base::uri::{Field, URI, UriChange};
use je_storage::StorageValue;

use super::user_data_sync::IResourcePreview as IBaseResourcePreview;
use super::user_data_sync::{
    Change, IRemoteUserData, ISyncData, IUserData, IUserDataSyncConfiguration, IUserDataSyncEnablementService, IUserDataSyncLocalStoreService, IUserDataSyncLogService, IUserDataSyncResource,
    IUserDataSyncResourceConflicts, IUserDataSyncResourceInitializer, IUserDataSyncResourcePreview, IUserDataSyncStoreService, IUserDataSyncUtilService, IUserDataSynchroniser, MergeState,
    NON_EXISTING_RESOURCE_REF, PREVIEW_DIR_NAME, RefOrUserData, ServerResource, SyncResource, SyncStatus, USER_DATA_SYNC_CONFIGURATION_SCOPE, USER_DATA_SYNC_SCHEME, UserDataSyncError,
    UserDataSyncErrorCode, get_last_sync_resource_uri, get_path_segments,
};
use crate::configuration::service::IConfigurationService;
use crate::environment::environment::EnvironmentService;
use crate::external_services::service_machine_id::get_service_machine_id;
use crate::files::file_service::{FileContentSource, FileService};
use crate::files::files::{CreateFileOptions, FileContent, FileOperationError, FileOperationResult, PartialFileDeleteOptions, WriteFileOptions, to_file_operation_result};
use crate::log::{LogArg, LogMessage, Logger};
use crate::storage::{StorageScope, StorageService, StorageTarget};
use crate::telemetry::ITelemetryService;
use crate::user_data_profile::{IUserDataProfilesService, UserDataProfile};

// region: JSON

/// `JSON.parse(text)`: o valor, ou um `SyntaxError`. Sem comentários e sem vírgula final, como o do motor.
pub fn parse_json(text: &str) -> Result<Value, Error> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut errors = Vec::new();
    let value = json::parse(&units, &mut errors, ParseOptions { disallow_comments: true, allow_trailing_comma: false, allow_empty_content: false });
    match value {
        Some(value) if errors.is_empty() => Ok(value),
        _ => Err(Error::named("SyntaxError", format!("Unexpected token in JSON: {text}"))),
    }
}

/// `JSON.stringify(value)`.
pub fn stringify_json(value: &Value) -> String {
    json_stringify(value, None)
}

fn json_string(text: &str) -> Value {
    Value::String(JsString::from(text))
}

fn string_property(object: &Object, name: &str) -> Option<String> {
    match object.get(name) {
        Some(Value::String(text)) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// O `!!value` do JavaScript sobre um valor JSON.
fn is_truthy_value(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => *number != 0.0 && !number.is_nan(),
        Value::String(text) => !text.to_string_lossy().is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `isRemoteUserData`: `ref` texto não vazio e `syncData` nulo ou um [`ISyncData`].
pub fn is_remote_user_data(thing: &Value) -> bool {
    let Value::Object(object) = thing else { return false };
    let has_ref = matches!(object.get("ref"), Some(Value::String(r#ref)) if !r#ref.to_string_lossy().is_empty());
    has_ref
        && match object.get("syncData") {
            Some(Value::Null) => true,
            Some(sync_data) => is_sync_data(sync_data),
            None => false,
        }
}

/// `isSyncData`: `version` número e `content` texto, sozinhos (compatibilidade) ou com `machineId` texto.
pub fn is_sync_data(thing: &Value) -> bool {
    let Value::Object(object) = thing else { return false };
    if matches!(object.get("version"), Some(Value::Number(_))) && matches!(object.get("content"), Some(Value::String(_))) {
        // backward compatibility
        if object.len() == 2 {
            return true;
        }
        if object.len() == 3 && matches!(object.get("machineId"), Some(Value::String(_))) {
            return true;
        }
    }
    false
}

impl ISyncData {
    /// O `ISyncData` que o valor descreve, se [`is_sync_data`] o aceita.
    pub fn from_json(value: &Value) -> Option<Self> {
        if !is_sync_data(value) {
            return None;
        }
        let Value::Object(object) = value else { return None };
        let Some(Value::Number(version)) = object.get("version") else { return None };
        Some(Self { version: *version, machine_id: string_property(object, "machineId"), content: string_property(object, "content")? })
    }

    /// `{ version, machineId?, content }`, na ordem em que o original escreve.
    pub fn to_json(&self) -> Value {
        let mut object = Object::new();
        object.insert(JsString::from("version"), Value::Number(self.version));
        if let Some(machine_id) = &self.machine_id {
            object.insert(JsString::from("machineId"), json_string(machine_id));
        }
        object.insert(JsString::from("content"), json_string(&self.content));
        Value::Object(object)
    }
}

impl IRemoteUserData {
    /// O `IRemoteUserData` que o valor descreve, se [`is_remote_user_data`] o aceita. As propriedades além de
    /// `ref` e `syncData` ficam em `extras`.
    pub fn from_json(value: &Value) -> Option<Self> {
        if !is_remote_user_data(value) {
            return None;
        }
        let Value::Object(object) = value else { return None };
        let sync_data = match object.get("syncData") {
            Some(Value::Null) => None,
            Some(sync_data) => Some(ISyncData::from_json(sync_data)?),
            None => return None,
        };
        let extras = object.iter().filter(|(key, _)| *key != "ref" && *key != "syncData").map(|(key, value)| (key.to_string_lossy(), value.clone())).collect();
        Some(Self { r#ref: string_property(object, "ref")?, sync_data, extras })
    }

    /// `{ ref, ...extras, syncData }`: a ordem do espalhamento do original.
    pub fn to_json(&self) -> Value {
        let mut object = Object::new();
        object.insert(JsString::from("ref"), json_string(&self.r#ref));
        for (key, value) in &self.extras {
            object.insert(JsString::from(key.as_str()), value.clone());
        }
        object.insert(JsString::from("syncData"), self.sync_data.as_ref().map_or(Value::Null, ISyncData::to_json));
        Value::Object(object)
    }
}

/// `ILastSyncUserDataState`: `ref`, `version` e o que `updateLastSyncUserData` recebeu em `additionalProps`.
/// `entries` são todas as propriedades menos `ref`, na ordem em que foram gravadas.
struct LastSyncUserDataState {
    r#ref: String,
    entries: Vec<(String, Value)>,
}

impl LastSyncUserDataState {
    fn from_json(value: &Value) -> Self {
        let Value::Object(object) = value else { return Self { r#ref: String::new(), entries: Vec::new() } };
        Self {
            r#ref: string_property(object, "ref").unwrap_or_default(),
            entries: object.iter().filter(|(key, _)| *key != "ref").map(|(key, value)| (key.to_string_lossy(), value.clone())).collect(),
        }
    }

    fn version(&self) -> Option<String> {
        self.entries.iter().find(|(key, _)| key == "version").and_then(|(_, value)| match value {
            Value::String(text) => Some(text.to_string_lossy()),
            _ => None,
        })
    }
}

/// O estado a gravar: `{ ref, version, ...additionalProps }` (o `version` ausente não aparece, como o
/// `undefined` do `JSON.stringify`).
fn last_sync_user_data_state_json(r#ref: &str, version: Option<&str>, additional_props: &[(String, Value)]) -> String {
    let mut object = Object::new();
    object.insert(JsString::from("ref"), json_string(r#ref));
    if let Some(version) = version {
        object.insert(JsString::from("version"), json_string(version));
    }
    for (key, value) in additional_props {
        object.insert(JsString::from(key.as_str()), value.clone());
    }
    stringify_json(&Value::Object(object))
}

// endregion

// region: tipos

/// `resource.with({ scheme: USER_DATA_SYNC_SCHEME, authority })`: a URI virtual (`base`, `local`, `remote`,
/// `accepted`) de um recurso do preview.
pub fn with_sync_authority(resource: &URI, authority: &str) -> URI {
    resource.with(&UriChange { scheme: Field::from(USER_DATA_SYNC_SCHEME), authority: Field::from(authority), ..UriChange::default() })
}

/// `getSyncResourceLogLabel`.
pub fn get_sync_resource_log_label(sync_resource: SyncResource, profile: &UserDataProfile) -> String {
    let name: Vec<u16> = sync_resource.as_str().encode_utf16().collect();
    let label = uppercase_first_letter(&name).to_string_lossy();
    if profile.is_default { label } else { format!("{label} ({})", profile.name) }
}

/// `IResourcePreview` (o de `abstractSynchronizer.ts`): o que o sincronizador gera. `extra` são os campos que
/// o subtipo acrescenta (ver [`SynchroniserImpl::Preview`]).
#[derive(Clone, Debug, PartialEq)]
pub struct IResourcePreview<P> {
    pub base_resource: URI,
    pub base_content: Option<String>,

    pub remote_resource: URI,
    pub remote_content: Option<String>,
    pub remote_change: Change,

    pub local_resource: URI,
    pub local_content: Option<String>,
    pub local_change: Change,

    pub preview_resource: URI,
    pub accepted_resource: URI,

    pub extra: P,
}

/// `IAcceptResult`. `extra` são os campos que o subtipo acrescenta (ver [`SynchroniserImpl::Accept`]).
#[derive(Clone, Debug, PartialEq)]
pub struct IAcceptResult<A> {
    pub content: Option<String>,
    pub local_change: Change,
    pub remote_change: Change,
    pub extra: A,
}

/// `IMergeResult`: um [`IAcceptResult`] com `hasConflicts`.
#[derive(Clone, Debug, PartialEq)]
pub struct IMergeResult<A> {
    pub result: IAcceptResult<A>,
    pub has_conflicts: bool,
}

impl<A> Deref for IMergeResult<A> {
    type Target = IAcceptResult<A>;

    fn deref(&self) -> &IAcceptResult<A> {
        &self.result
    }
}

/// `IEditableResourcePreview`: o preview com o estado de merge e o resultado de aceitar, que mudam durante a
/// sincronização. Dá `Deref` (e `DerefMut`) para o [`IResourcePreview`], cujos `localChange` e `remoteChange`
/// o original sobrescreve.
#[derive(Clone, Debug, PartialEq)]
pub struct IEditableResourcePreview<P, A> {
    pub preview: IResourcePreview<P>,
    pub merge_state: MergeState,
    pub accept_result: Option<IAcceptResult<A>>,
}

impl<P, A> Deref for IEditableResourcePreview<P, A> {
    type Target = IResourcePreview<P>;

    fn deref(&self) -> &IResourcePreview<P> {
        &self.preview
    }
}

impl<P, A> DerefMut for IEditableResourcePreview<P, A> {
    fn deref_mut(&mut self) -> &mut IResourcePreview<P> {
        &mut self.preview
    }
}

impl<P, A> IEditableResourcePreview<P, A> {
    /// O `IResourcePreview` de `userDataSync.ts`: a visão que o resto do sistema tem do preview.
    pub fn to_base(&self) -> IBaseResourcePreview {
        IBaseResourcePreview {
            base_resource: self.preview.base_resource.clone(),
            remote_resource: self.preview.remote_resource.clone(),
            local_resource: self.preview.local_resource.clone(),
            preview_resource: self.preview.preview_resource.clone(),
            accepted_resource: self.preview.accepted_resource.clone(),
            local_change: self.preview.local_change,
            remote_change: self.preview.remote_change,
            merge_state: self.merge_state,
        }
    }
}

/// `ISyncResourcePreview`.
#[derive(Clone, Debug, PartialEq)]
pub struct ISyncResourcePreview<P, A> {
    pub sync_resource: SyncResource,
    pub profile: UserDataProfile,
    pub is_last_sync_from_current_machine: bool,
    pub remote_user_data: IRemoteUserData,
    pub last_sync_user_data: Option<IRemoteUserData>,
    pub resource_previews: Vec<IEditableResourcePreview<P, A>>,
}

impl<P, A> ISyncResourcePreview<P, A> {
    /// O `IUserDataSyncResourcePreview` de `userDataSync.ts`.
    pub fn to_base(&self) -> IUserDataSyncResourcePreview {
        IUserDataSyncResourcePreview {
            sync_resource: self.sync_resource,
            profile: self.profile.clone(),
            is_last_sync_from_current_machine: self.is_last_sync_from_current_machine,
            resource_previews: self.resource_previews.iter().map(IEditableResourcePreview::to_base).collect(),
        }
    }
}

/// `SyncStrategy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncStrategy {
    /// Merge the local and remote data without applying.
    Preview,
    /// Merge the local and remote data and apply.
    Merge,
    /// Pull the remote data or push the local data.
    PullOrPush,
}

impl SyncStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Merge => "merge",
            Self::PullOrPush => "pull-push",
        }
    }
}

/// Os serviços que o `AbstractSynchroniser` recebe por injeção (`@IFileService`, `@IEnvironmentService`...) e o
/// `extUri` do `IUriIdentityService`.
#[derive(Clone)]
pub struct SynchroniserServices {
    pub file_service: Rc<FileService>,
    pub environment_service: Rc<dyn EnvironmentService>,
    pub storage_service: Rc<dyn StorageService>,
    pub user_data_sync_store_service: Rc<dyn IUserDataSyncStoreService>,
    pub user_data_sync_local_store_service: Rc<dyn IUserDataSyncLocalStoreService>,
    pub user_data_sync_enablement_service: Rc<dyn IUserDataSyncEnablementService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub log_service: IUserDataSyncLogService,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub ext_uri: ExtUri,
}

/// O que `AbstractSynchroniser` deixa abstrato.
///
/// Os métodos recebem `self: Rc<Self>` porque as continuações `async` precisam do sincronizador vivo.
pub trait SynchroniserImpl: 'static {
    /// Os campos que o preview deste sincronizador acrescenta ao [`IResourcePreview`].
    type Preview: Clone + 'static;
    /// Os campos que o resultado deste sincronizador acrescenta ao [`IAcceptResult`]. [`Default`] é o valor
    /// do recurso sem mudança.
    type Accept: Clone + Default + 'static;

    /// `version`.
    fn version(&self) -> f64;

    fn generate_sync_preview(
        self: Rc<Self>,
        remote_user_data: IRemoteUserData,
        last_sync_user_data: Option<IRemoteUserData>,
        is_remote_data_from_current_machine: bool,
        user_data_sync_configuration: IUserDataSyncConfiguration,
        token: CancellationToken,
    ) -> Promise<Vec<IResourcePreview<Self::Preview>>>;

    fn get_merge_result(self: Rc<Self>, resource_preview: IResourcePreview<Self::Preview>, token: CancellationToken) -> Promise<IMergeResult<Self::Accept>>;

    /// `content`: `None` é `undefined`, `Some(None)` é `null`.
    fn get_accept_result(self: Rc<Self>, resource_preview: IResourcePreview<Self::Preview>, resource: URI, content: Option<Option<String>>, token: CancellationToken) -> Promise<IAcceptResult<Self::Accept>>;

    fn apply_result(
        self: Rc<Self>,
        remote_user_data: IRemoteUserData,
        last_sync_user_data: Option<IRemoteUserData>,
        result: Vec<(IResourcePreview<Self::Preview>, IAcceptResult<Self::Accept>)>,
        force: bool,
    ) -> Promise<()>;

    fn has_remote_changed(self: Rc<Self>, last_sync_user_data: IRemoteUserData) -> Promise<bool>;

    fn has_local_data(self: Rc<Self>) -> Promise<bool>;

    fn resolve_content(self: Rc<Self>, uri: URI) -> Promise<Option<String>>;
}

type Preview<I> = ISyncResourcePreview<<I as SynchroniserImpl>::Preview, <I as SynchroniserImpl>::Accept>;
type Editable<I> = IEditableResourcePreview<<I as SynchroniserImpl>::Preview, <I as SynchroniserImpl>::Accept>;
type UpdateResourcePreview<I> = Box<dyn FnOnce(Editable<I>) -> Promise<Editable<I>>>;

struct State<I: SynchroniserImpl> {
    sync_preview_promise: Option<CancelablePromise<Preview<I>>>,
    status: SyncStatus,
    conflicts: Vec<IBaseResourcePreview>,
    sync_headers: IHeaders,
}

/// Espera a promessa do preview, se há: `return this.syncPreviewPromise` de um método `async`.
async fn await_optional<T: Clone + 'static>(promise: Option<CancelablePromise<T>>) -> Result<Option<T>, Error> {
    match promise {
        Some(promise) => Ok(Some(promise.await?)),
        None => Ok(None),
    }
}

// endregion

/// `AbstractSynchroniser`.
pub struct AbstractSynchroniser<I: SynchroniserImpl> {
    me: Weak<Self>,
    base: Rc<Disposable>,
    imp: RefCell<Option<Weak<I>>>,

    pub sync_resource: IUserDataSyncResource,
    pub collection: Option<String>,
    pub services: SynchroniserServices,

    pub sync_folder: URI,
    pub sync_preview_folder: URI,
    pub last_sync_resource: URI,
    last_sync_user_data_state_key: String,
    pub sync_resource_log_label: String,
    pub resource: SyncResource,
    pub current_machine_id_promise: Promise<String>,

    state: RefCell<State<I>>,
    on_did_change_status: Rc<Emitter<SyncStatus>>,
    on_did_change_conflicts: Rc<Emitter<IUserDataSyncResourceConflicts>>,
    local_change_trigger_throttler: Rc<ThrottledDelayer<()>>,
    on_did_change_local: Rc<Emitter<()>>,
}

impl<I: SynchroniserImpl> AbstractSynchroniser<I> {
    pub fn new(sync_resource: IUserDataSyncResource, collection: Option<String>, services: SynchroniserServices) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_status = Emitter::new();
        base.register(on_did_change_status.clone());
        let on_did_change_conflicts = Emitter::new();
        base.register(on_did_change_conflicts.clone());
        let local_change_trigger_throttler = Rc::new(ThrottledDelayer::<()>::new(50));
        base.register(local_change_trigger_throttler.clone());
        let on_did_change_local = Emitter::new();
        base.register(on_did_change_local.clone());

        let resource = sync_resource.sync_resource;
        let profile_collection = if sync_resource.profile.is_default { None } else { Some(sync_resource.profile.id.clone()) };
        let ext_uri = &services.ext_uri;
        let segments = get_path_segments(profile_collection.as_deref(), &[resource.as_str()]);
        let sync_folder = ext_uri.join_path(&services.environment_service.user_data_sync_home(), &segments.iter().map(String::as_str).collect::<Vec<_>>());
        let sync_preview_folder = ext_uri.join_path(&sync_folder, &[PREVIEW_DIR_NAME]);
        let last_sync_resource = get_last_sync_resource_uri(profile_collection.as_deref(), resource, services.environment_service.as_ref(), ext_uri);
        let current_machine_id_promise = get_service_machine_id(services.environment_service.as_ref(), &services.file_service, Some(services.storage_service.clone()));
        let last_sync_user_data_state_key = format!("{}{}.lastSyncUserData", collection.as_deref().filter(|collection| !collection.is_empty()).map_or(String::new(), |collection| format!("{collection}.")), resource.as_str());
        let sync_resource_log_label = get_sync_resource_log_label(resource, &sync_resource.profile);

        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            imp: RefCell::new(None),
            sync_resource,
            collection,
            services,
            sync_folder,
            sync_preview_folder,
            last_sync_resource,
            last_sync_user_data_state_key,
            sync_resource_log_label,
            resource,
            current_machine_id_promise,
            state: RefCell::new(State { sync_preview_promise: None, status: SyncStatus::Idle, conflicts: Vec::new(), sync_headers: IHeaders::new() }),
            on_did_change_status,
            on_did_change_conflicts,
            local_change_trigger_throttler,
            on_did_change_local,
        })
    }

    /// Liga o sincronizador concreto, o que o `super(...)` do construtor e o `this` fazem no original.
    pub fn bind(&self, imp: Weak<I>) {
        *self.imp.borrow_mut() = Some(imp);
    }

    /// `_register(o)`.
    pub fn register(&self, o: DisposableRef) -> DisposableRef {
        self.base.register(o)
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o sincronizador está vivo enquanto é usado")
    }

    fn imp(&self) -> Rc<I> {
        self.imp.borrow().as_ref().and_then(Weak::upgrade).expect("o sincronizador concreto foi ligado e está vivo")
    }

    /// `this.extUri.isEqual(a, b)`.
    fn is_same_resource(&self, a: &URI, b: &URI) -> bool {
        self.services.ext_uri.is_equal(Some(a), Some(b), false)
    }

    fn log_error(&self, error: &Error, args: &[LogArg]) {
        self.services.log_service.error_args(&LogMessage::from(error), args);
    }

    fn resource_lower_case(&self) -> String {
        self.resource.as_str().to_lowercase()
    }

    pub fn sync_headers(&self) -> IHeaders {
        self.state.borrow().sync_headers.clone()
    }

    /// `syncPreviewPromise`.
    fn sync_preview_promise(&self) -> Option<CancelablePromise<Preview<I>>> {
        self.state.borrow().sync_preview_promise.clone()
    }

    /// `triggerLocalChange`.
    pub fn trigger_local_change(&self) {
        let this = self.rc();
        let _ = self.local_change_trigger_throttler.trigger(move |_token| this.do_trigger_local_change(), None);
    }

    fn do_trigger_local_change(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let status = this.state.borrow().status;
            // Sync again if current status is in conflicts
            if status == SyncStatus::HasConflicts {
                this.services.log_service.info(&format!("{}: In conflicts state and local change detected. Syncing again...", this.sync_resource_log_label));
                let preview = this.sync_preview_promise().expect("em conflito há um preview").await?;
                this.state.borrow_mut().sync_preview_promise = None;
                let status = this.perform_sync(preview.remote_user_data.clone(), preview.last_sync_user_data.clone(), SyncStrategy::Merge, this.get_user_data_sync_configuration()).await?;
                this.set_status(status);
            }
            // Check if local change causes remote change
            else {
                this.services.log_service.trace(&format!("{}: Checking for local changes...", this.sync_resource_log_label));
                let last_sync_user_data = this.get_last_sync_user_data().await?;
                let has_remote_changed = match last_sync_user_data {
                    Some(last_sync_user_data) => this.imp().has_remote_changed(last_sync_user_data).await?,
                    None => true,
                };
                if has_remote_changed {
                    this.on_did_change_local.fire(());
                }
            }
            Ok(())
        })
    }

    /// `setStatus`.
    pub fn set_status(&self, status: SyncStatus) {
        let changed = {
            let mut state = self.state.borrow_mut();
            let changed = state.status != status;
            state.status = status;
            changed
        };
        if changed {
            self.on_did_change_status.fire(status);
        }
    }

    /// `getUserDataSyncConfiguration`.
    pub fn get_user_data_sync_configuration(&self) -> IUserDataSyncConfiguration {
        IUserDataSyncConfiguration::from_dynamic(&self.services.configuration_service.get_value(Some(USER_DATA_SYNC_CONFIGURATION_SCOPE), None))
    }

    fn is_remote_data_from_current_machine(&self, remote_user_data: &IRemoteUserData) -> Promise<bool> {
        let machine_id = self.current_machine_id_promise.clone();
        let remote_machine_id = remote_user_data.sync_data.as_ref().and_then(|sync_data| sync_data.machine_id.clone());
        async_fn(async move {
            let machine_id = machine_id.await?;
            Ok(remote_machine_id.is_some_and(|remote_machine_id| !remote_machine_id.is_empty() && remote_machine_id == machine_id))
        })
    }

    /// `getLatestRemoteUserData`: `None` é o `null` (o recurso não existe no servidor).
    pub fn get_latest_remote_user_data(&self, ref_or_latest_data: Option<RefOrUserData>, last_sync_user_data: Option<IRemoteUserData>) -> Promise<IRemoteUserData> {
        let this = self.rc();
        async_fn(async move {
            match ref_or_latest_data {
                None => Ok(IRemoteUserData::new(NON_EXISTING_RESOURCE_REF, None)),
                Some(RefOrUserData::UserData(user_data)) => this.to_remote_user_data(user_data),
                Some(RefOrUserData::Ref(r#ref)) => {
                    // Last time synced resource and latest resource on server are same
                    if let Some(last_sync_user_data) = last_sync_user_data.as_ref().filter(|last| last.r#ref == r#ref) {
                        return Ok(last_sync_user_data.clone());
                    }
                    this.get_remote_user_data(last_sync_user_data).await
                }
            }
        })
    }

    fn perform_sync(&self, remote_user_data: IRemoteUserData, last_sync_user_data: Option<IRemoteUserData>, strategy: SyncStrategy, user_data_sync_configuration: IUserDataSyncConfiguration) -> Promise<SyncStatus> {
        let this = self.rc();
        async_fn(async move {
            let version = this.imp().version();
            if let Some(sync_data) = remote_user_data.sync_data.as_ref().filter(|sync_data| sync_data.version > version) {
                return Err(UserDataSyncError::new(
                    localize(
                        "incompatible",
                        "Cannot sync {0} as its local version {1} is not compatible with its remote version {2}",
                        &[this.resource.as_str(), &version.to_string(), &sync_data.version.to_string()],
                    ),
                    UserDataSyncErrorCode::IncompatibleLocalContent,
                    Some(this.resource),
                )
                .into_error());
            }

            let error = match this.do_sync(remote_user_data.clone(), last_sync_user_data.clone(), strategy, user_data_sync_configuration.clone()).await {
                Ok(status) => return Ok(status),
                Err(error) => error,
            };
            if let Some(sync_error) = UserDataSyncError::from_error(&error) {
                match sync_error.code {
                    UserDataSyncErrorCode::LocalPreconditionFailed => {
                        // Rejected as there is a new local version. Syncing again...
                        this.services.log_service.info(&format!(
                            "{}: Failed to synchronize {} as there is a new local version available. Synchronizing again...",
                            this.sync_resource_log_label, this.sync_resource_log_label
                        ));
                        return this.perform_sync(remote_user_data, last_sync_user_data, strategy, user_data_sync_configuration).await;
                    }

                    UserDataSyncErrorCode::Conflict | UserDataSyncErrorCode::PreconditionFailed => {
                        // Rejected as there is a new remote version. Syncing again...
                        this.services.log_service.info(&format!("{}: Failed to synchronize as there is a new remote version available. Synchronizing again...", this.sync_resource_log_label));

                        // Avoid cache and get latest remote user data - https://github.com/microsoft/vscode/issues/90624
                        let remote_user_data = this.get_remote_user_data(None).await?;

                        // Get the latest last sync user data. Because multiple parallel syncs (in Web) could share same last sync data
                        // and one of them successfully updated remote and last sync state.
                        let last_sync_user_data = this.get_last_sync_user_data().await?;

                        return this.perform_sync(remote_user_data, last_sync_user_data, SyncStrategy::Merge, user_data_sync_configuration).await;
                    }
                    _ => {}
                }
            }
            Err(error)
        })
    }

    fn do_sync(&self, remote_user_data: IRemoteUserData, last_sync_user_data: Option<IRemoteUserData>, strategy: SyncStrategy, user_data_sync_configuration: IUserDataSyncConfiguration) -> Promise<SyncStatus> {
        let this = self.rc();
        async_fn(async move {
            let result = Self::do_sync_body(this.clone(), remote_user_data, last_sync_user_data, strategy, user_data_sync_configuration).await;
            if result.is_err() {
                // reset preview on error
                this.state.borrow_mut().sync_preview_promise = None;
            }
            result
        })
    }

    async fn do_sync_body(
        this: Rc<Self>,
        remote_user_data: IRemoteUserData,
        last_sync_user_data: Option<IRemoteUserData>,
        strategy: SyncStrategy,
        user_data_sync_configuration: IUserDataSyncConfiguration,
    ) -> Result<SyncStatus, Error> {
        let is_remote_data_from_current_machine = this.is_remote_data_from_current_machine(&remote_user_data).await?;
        let accept_remote = !is_remote_data_from_current_machine && last_sync_user_data.is_none() && this.get_stored_last_sync_user_data_state_content().is_some();
        let merge = strategy == SyncStrategy::Preview || (strategy == SyncStrategy::Merge && !accept_remote);
        let apply = strategy == SyncStrategy::Merge || strategy == SyncStrategy::PullOrPush;

        // generate or use existing preview
        let has_preview = this.sync_preview_promise().is_some();
        if !has_preview {
            let generator = this.clone();
            let (remote, last, configuration) = (remote_user_data.clone(), last_sync_user_data.clone(), user_data_sync_configuration);
            let promise = create_cancelable_promise(move |token| generator.do_generate_sync_resource_preview(remote, last, is_remote_data_from_current_machine, merge, configuration, token));
            this.state.borrow_mut().sync_preview_promise = Some(promise);
        }

        let mut preview = this.sync_preview_promise().expect("o preview existe ou acabou de ser criado").await?;

        if strategy == SyncStrategy::Merge && accept_remote {
            this.services.log_service.info(&format!("{}: Accepting remote because it was synced before and the last sync data is not available.", this.sync_resource_log_label));
            for resource_preview in preview.resource_previews.clone() {
                preview = this.accept_preview(&resource_preview.remote_resource, None).await?.unwrap_or(preview);
            }
        } else if strategy == SyncStrategy::PullOrPush {
            for resource_preview in preview.resource_previews.clone() {
                if resource_preview.merge_state == MergeState::Accepted {
                    continue;
                }
                let same_ref = last_sync_user_data.as_ref().is_some_and(|last| last.r#ref == remote_user_data.r#ref);
                let accepted = if same_ref || is_remote_data_from_current_machine {
                    this.accept_preview(&resource_preview.local_resource, None).await?
                } else {
                    this.accept_preview(&resource_preview.remote_resource, None).await?
                };
                preview = accepted.unwrap_or(preview);
            }
        }

        this.update_conflicts(&preview.resource_previews);
        if preview.resource_previews.iter().any(|resource_preview| resource_preview.merge_state == MergeState::Conflict) {
            return Ok(SyncStatus::HasConflicts);
        }

        if apply {
            return this.do_apply(false).await;
        }

        Ok(SyncStatus::Syncing)
    }

    /// `accept`, com o preview completo que `doSync` usa. O `accept` do [`IUserDataSynchroniser`] devolve a
    /// visão base dele.
    fn accept_preview(&self, resource: &URI, content: Option<Option<String>>) -> Promise<Option<Preview<I>>> {
        let this = self.rc();
        let resource = resource.clone();
        async_fn(async move {
            let imp = this.imp();
            let accepted = resource.clone();
            this.update_sync_resource_preview(
                &resource,
                Box::new(move |mut resource_preview: Editable<I>| {
                    async_fn(async move {
                        let accept_result = imp.get_accept_result(resource_preview.preview.clone(), accepted, content, CancellationToken::None).await?;
                        resource_preview.local_change = accept_result.local_change;
                        resource_preview.remote_change = accept_result.remote_change;
                        resource_preview.accept_result = Some(accept_result);
                        resource_preview.merge_state = MergeState::Accepted;
                        Ok(resource_preview)
                    })
                }),
            )
            .await?;
            await_optional(this.sync_preview_promise()).await
        })
    }

    /// `discard`.
    pub fn discard(&self, resource: &URI) -> Promise<Option<Preview<I>>> {
        let this = self.rc();
        let resource = resource.clone();
        async_fn(async move {
            let (imp, file_service) = (this.imp(), this.services.file_service.clone());
            this.update_sync_resource_preview(
                &resource,
                Box::new(move |mut resource_preview: Editable<I>| {
                    async_fn(async move {
                        let merge_result = imp.get_merge_result(resource_preview.preview.clone(), CancellationToken::None).await?;
                        let content = merge_result.content.clone().unwrap_or_default();
                        file_service.write_file(&resource_preview.preview_resource, FileContentSource::from(content.as_str()), None).await?;
                        resource_preview.accept_result = None;
                        resource_preview.merge_state = MergeState::Preview;
                        resource_preview.local_change = merge_result.local_change;
                        resource_preview.remote_change = merge_result.remote_change;
                        Ok(resource_preview)
                    })
                }),
            )
            .await?;
            await_optional(this.sync_preview_promise()).await
        })
    }

    fn update_sync_resource_preview(&self, resource: &URI, update_resource_preview: UpdateResourcePreview<I>) -> Promise<()> {
        let this = self.rc();
        let resource = resource.clone();
        async_fn(async move {
            let Some(sync_preview_promise) = this.sync_preview_promise() else { return Ok(()) };
            let mut preview = sync_preview_promise.await?;
            let index = preview.resource_previews.iter().position(|resource_preview| {
                this.is_same_resource(&resource_preview.local_resource, &resource)
                    || this.is_same_resource(&resource_preview.remote_resource, &resource)
                    || this.is_same_resource(&resource_preview.preview_resource, &resource)
            });
            let Some(index) = index else { return Ok(()) };

            let current = preview.clone();
            let updated = create_cancelable_promise(move |_token| {
                async_fn(async move {
                    let mut resource_previews = current.resource_previews.clone();
                    resource_previews[index] = update_resource_preview(resource_previews[index].clone()).await?;
                    Ok(ISyncResourcePreview { resource_previews, ..current })
                })
            });
            this.state.borrow_mut().sync_preview_promise = Some(updated.clone());

            preview = updated.await?;
            this.update_conflicts(&preview.resource_previews);
            if preview.resource_previews.iter().any(|resource_preview| resource_preview.merge_state == MergeState::Conflict) {
                this.set_status(SyncStatus::HasConflicts);
            } else {
                this.set_status(SyncStatus::Syncing);
            }
            Ok(())
        })
    }

    fn do_apply(&self, force: bool) -> Promise<SyncStatus> {
        let this = self.rc();
        async_fn(async move {
            let Some(sync_preview_promise) = this.sync_preview_promise() else { return Ok(SyncStatus::Idle) };

            let preview = sync_preview_promise.await?;

            // check for conflicts
            if preview.resource_previews.iter().any(|resource_preview| resource_preview.merge_state == MergeState::Conflict) {
                return Ok(SyncStatus::HasConflicts);
            }

            // check if all are accepted
            if preview.resource_previews.iter().any(|resource_preview| resource_preview.merge_state != MergeState::Accepted) {
                return Ok(SyncStatus::Syncing);
            }

            // apply preview
            let result = preview
                .resource_previews
                .iter()
                .map(|resource_preview| (resource_preview.preview.clone(), resource_preview.accept_result.clone().expect("o preview aceito tem o resultado de aceitar")))
                .collect();
            this.imp().apply_result(preview.remote_user_data.clone(), preview.last_sync_user_data.clone(), result, force).await?;

            // reset preview
            this.state.borrow_mut().sync_preview_promise = None;

            // reset preview folder
            this.clear_preview_folder().await?;

            Ok(SyncStatus::Idle)
        })
    }

    fn clear_preview_folder(&self) -> Promise<()> {
        let file_service = self.services.file_service.clone();
        let folder = self.sync_preview_folder.clone();
        async_fn(async move {
            // Ignore
            let _ = file_service.del(&folder, Some(PartialFileDeleteOptions { recursive: Some(true), ..PartialFileDeleteOptions::default() })).await;
            Ok(())
        })
    }

    fn update_conflicts(&self, resource_previews: &[Editable<I>]) {
        let conflicts: Vec<IBaseResourcePreview> = resource_previews.iter().filter(|resource_preview| resource_preview.merge_state == MergeState::Conflict).map(IEditableResourcePreview::to_base).collect();
        let changed = !equals(Some(self.state.borrow().conflicts.as_slice()), Some(conflicts.as_slice()), |a, b| self.is_same_resource(&a.preview_resource, &b.preview_resource));
        if changed {
            self.state.borrow_mut().conflicts = conflicts;
            self.on_did_change_conflicts.fire(self.conflicts());
        }
    }

    /// `resolvePreviewContent`.
    pub fn resolve_preview_content(&self, uri: &URI) -> Promise<Option<String>> {
        let this = self.rc();
        let uri = uri.clone();
        async_fn(async move {
            let sync_preview = await_optional(this.sync_preview_promise()).await?;
            if let Some(sync_preview) = sync_preview {
                for resource_preview in &sync_preview.resource_previews {
                    if this.is_same_resource(&resource_preview.accepted_resource, &uri) {
                        return Ok(resource_preview.accept_result.as_ref().and_then(|accept_result| accept_result.content.clone()));
                    }
                    if this.is_same_resource(&resource_preview.remote_resource, &uri) {
                        return Ok(resource_preview.remote_content.clone());
                    }
                    if this.is_same_resource(&resource_preview.local_resource, &uri) {
                        return Ok(resource_preview.local_content.clone());
                    }
                    if this.is_same_resource(&resource_preview.base_resource, &uri) {
                        return Ok(resource_preview.base_content.clone());
                    }
                }
            }
            Ok(None)
        })
    }

    fn do_generate_sync_resource_preview(
        &self,
        remote_user_data: IRemoteUserData,
        last_sync_user_data: Option<IRemoteUserData>,
        is_remote_data_from_current_machine: bool,
        merge: bool,
        user_data_sync_configuration: IUserDataSyncConfiguration,
        token: CancellationToken,
    ) -> Promise<Preview<I>> {
        let this = self.rc();
        async_fn(async move {
            let imp = this.imp();
            let resource_preview_results =
                imp.clone().generate_sync_preview(remote_user_data.clone(), last_sync_user_data.clone(), is_remote_data_from_current_machine, user_data_sync_configuration, token.clone()).await?;

            let mut resource_previews: Vec<Editable<I>> = Vec::new();
            for resource_preview_result in resource_preview_results {
                let accepted_resource = with_sync_authority(&resource_preview_result.preview_resource, "accepted");

                /* No change -> Accept */
                if resource_preview_result.local_change == Change::None && resource_preview_result.remote_change == Change::None {
                    resource_previews.push(IEditableResourcePreview {
                        preview: IResourcePreview { accepted_resource, ..resource_preview_result },
                        accept_result: Some(IAcceptResult { content: None, local_change: Change::None, remote_change: Change::None, extra: Default::default() }),
                        merge_state: MergeState::Accepted,
                    });
                }
                /* Changed -> Apply ? (Merge ? Conflict | Accept) : Preview */
                else {
                    /* Merge */
                    let merge_result = if merge { Some(imp.clone().get_merge_result(resource_preview_result.clone(), token.clone()).await?) } else { None };
                    if token.is_cancellation_requested() {
                        break;
                    }
                    let merged_content = merge_result.as_ref().and_then(|merge_result| merge_result.content.clone()).unwrap_or_default();
                    this.services.file_service.write_file(&resource_preview_result.preview_resource, FileContentSource::from(merged_content.as_str()), None).await?;

                    /* Conflict | Accept */
                    let accept_result = match merge_result.as_ref().filter(|merge_result| !merge_result.has_conflicts) {
                        /* Accept if merged and there are no conflicts */
                        Some(_) => Some(imp.clone().get_accept_result(resource_preview_result.clone(), resource_preview_result.preview_resource.clone(), None, token.clone()).await?),
                        None => None,
                    };

                    let has_conflicts = merge_result.as_ref().is_some_and(|merge_result| merge_result.has_conflicts);
                    let merge_state = if has_conflicts {
                        MergeState::Conflict
                    } else if accept_result.is_some() {
                        MergeState::Accepted
                    } else {
                        MergeState::Preview
                    };
                    let (local_change, remote_change) = match (&accept_result, &merge_result) {
                        (Some(accept_result), _) => (accept_result.local_change, accept_result.remote_change),
                        (None, Some(merge_result)) => (merge_result.local_change, merge_result.remote_change),
                        (None, None) => (resource_preview_result.local_change, resource_preview_result.remote_change),
                    };
                    resource_previews.push(IEditableResourcePreview { preview: IResourcePreview { local_change, remote_change, ..resource_preview_result }, accept_result, merge_state });
                }
            }

            Ok(ISyncResourcePreview {
                sync_resource: this.resource,
                profile: this.sync_resource.profile.clone(),
                remote_user_data,
                last_sync_user_data,
                resource_previews,
                is_last_sync_from_current_machine: is_remote_data_from_current_machine,
            })
        })
    }

    /// `getLastSyncUserData`.
    pub fn get_last_sync_user_data(&self) -> Promise<Option<IRemoteUserData>> {
        let this = self.rc();
        async_fn(async move {
            let Some(stored_last_sync_user_data_state_content) = this.get_stored_last_sync_user_data_state_content().filter(|content| !content.is_empty()) else {
                // Last Sync Data state does not exist
                this.services.log_service.info(&format!("{}: Last sync data state does not exist.", this.sync_resource_log_label));
                return Ok(None);
            };

            let last_sync_user_data_state = LastSyncUserDataState::from_json(&parse_json(&stored_last_sync_user_data_state_content)?);
            let resource_sync_state_version = this.services.user_data_sync_enablement_service.get_resource_sync_state_version(this.resource);
            let state_version = last_sync_user_data_state.version();
            let has_sync_resource_state_version_changed = state_version.as_deref().is_some_and(|version| !version.is_empty())
                && resource_sync_state_version.as_deref().is_some_and(|version| !version.is_empty())
                && state_version != resource_sync_state_version;
            if has_sync_resource_state_version_changed {
                this.services.log_service.info(&format!(
                    "{}: Reset last sync state because last sync state version {} is not compatible with current sync state version {}.",
                    this.sync_resource_log_label,
                    state_version.unwrap_or_default(),
                    resource_sync_state_version.unwrap_or_default()
                ));
                this.reset_local().await?;
                return Ok(None);
            }

            // `None` é `undefined`; `Some(None)` é `null` (o recurso não existe)
            let mut sync_data: Option<Option<ISyncData>> = None;

            // Get Last Sync Data from Local
            let mut retrial = 1;
            loop {
                if sync_data.is_some() {
                    break;
                }
                let retry = retrial < 6; /* Retry 5 times */
                retrial += 1;
                if !retry {
                    break;
                }
                match this.read_last_sync_stored_remote_user_data().await {
                    Ok(last_sync_stored_remote_user_data) => {
                        if let Some(last_sync_stored_remote_user_data) = last_sync_stored_remote_user_data {
                            if last_sync_stored_remote_user_data.r#ref == last_sync_user_data_state.r#ref {
                                sync_data = Some(last_sync_stored_remote_user_data.sync_data);
                            } else {
                                this.services.log_service.info(&format!("{}: Last sync data stored locally is not same as the last sync state.", this.sync_resource_log_label));
                            }
                        }
                        break;
                    }
                    Err(error) => {
                        if FileOperationError::from_error(&error).is_some_and(|file_error| file_error.file_operation_result == FileOperationResult::FileNotFound) {
                            this.services.log_service.info(&format!("{}: Last sync resource does not exist locally.", this.sync_resource_log_label));
                            break;
                        } else if UserDataSyncError::from_error(&error).is_some() {
                            return Err(error);
                        } else {
                            // log and retry
                            this.log_error(&error, &[LogArg::from(retrial)]);
                        }
                    }
                }
            }

            // Get Last Sync Data from Remote
            if sync_data.is_none() {
                let remote = async {
                    let content = this
                        .services
                        .user_data_sync_store_service
                        .resolve_resource_content(ServerResource::from(this.resource), &last_sync_user_data_state.r#ref, this.collection.as_deref(), Some(this.sync_headers()))
                        .await?;
                    let sync_data = match content {
                        None => None,
                        Some(content) => Some(this.parse_sync_data(&content)?),
                    };
                    this.write_last_sync_stored_remote_user_data(&IRemoteUserData::new(last_sync_user_data_state.r#ref.clone(), sync_data.clone())).await?;
                    Ok::<Option<ISyncData>, Error>(sync_data)
                }
                .await;
                match remote {
                    Ok(remote_sync_data) => sync_data = Some(remote_sync_data),
                    Err(error) => {
                        if UserDataSyncError::from_error(&error).is_some_and(|sync_error| sync_error.code == UserDataSyncErrorCode::NotFound) {
                            this.services.log_service.info(&format!("{}: Last sync resource does not exist remotely.", this.sync_resource_log_label));
                        } else {
                            return Err(error);
                        }
                    }
                }
            }

            // Last Sync Data Not Found
            let Some(sync_data) = sync_data else { return Ok(None) };

            Ok(Some(IRemoteUserData { r#ref: last_sync_user_data_state.r#ref, sync_data, extras: last_sync_user_data_state.entries }))
        })
    }

    /// `updateLastSyncUserData`: grava o estado da última sincronização (no armazenamento) e os dados remotos
    /// (no arquivo). `additional_props` não pode ter `ref` nem `version` verdadeiros.
    pub fn update_last_sync_user_data(&self, last_sync_remote_user_data: IRemoteUserData, additional_props: Vec<(String, Value)>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            if additional_props.iter().any(|(key, value)| (key == "ref" || key == "version") && is_truthy_value(value)) {
                return Err(Error::new("Cannot have core properties as additional"));
            }

            let version = this.services.user_data_sync_enablement_service.get_resource_sync_state_version(this.resource);
            let last_sync_user_data_state = last_sync_user_data_state_json(&last_sync_remote_user_data.r#ref, version.as_deref(), &additional_props);

            this.services.storage_service.store(&this.last_sync_user_data_state_key, StorageValue::from(last_sync_user_data_state.as_str()), StorageScope::Application, StorageTarget::Machine);
            this.write_last_sync_stored_remote_user_data(&last_sync_remote_user_data).await
        })
    }

    fn get_stored_last_sync_user_data_state_content(&self) -> Option<String> {
        self.services.storage_service.get(&self.last_sync_user_data_state_key, StorageScope::Application, None)
    }

    fn read_last_sync_stored_remote_user_data(&self) -> Promise<Option<IRemoteUserData>> {
        let this = self.rc();
        async_fn(async move {
            let content = this.services.file_service.read_file(&this.last_sync_resource, None, None).await?.value.to_string();
            let parsed = if content.is_empty() { Ok(None) } else { parse_json(&content).map(Some) };
            match parsed {
                Ok(Some(last_sync_stored_remote_user_data)) => Ok(IRemoteUserData::from_json(&last_sync_stored_remote_user_data)),
                Ok(None) => Ok(None),
                Err(error) => {
                    this.log_error(&error, &[]);
                    Ok(None)
                }
            }
        })
    }

    fn write_last_sync_stored_remote_user_data(&self, last_sync_remote_user_data: &IRemoteUserData) -> Promise<()> {
        let file_service = self.services.file_service.clone();
        let resource = self.last_sync_resource.clone();
        let content = stringify_json(&last_sync_remote_user_data.to_json());
        async_fn(async move { file_service.write_file(&resource, FileContentSource::from(content.as_str()), None).await.map(|_| ()) })
    }

    /// `getRemoteUserData`.
    pub fn get_remote_user_data(&self, last_sync_data: Option<IRemoteUserData>) -> Promise<IRemoteUserData> {
        let this = self.rc();
        async_fn(async move {
            let user_data = this.get_user_data(last_sync_data).await?;
            this.to_remote_user_data(user_data)
        })
    }

    fn to_remote_user_data(&self, IUserData { r#ref, content }: IUserData) -> Result<IRemoteUserData, Error> {
        let sync_data = match content {
            Some(content) => Some(self.parse_sync_data(&content)?),
            None => None,
        };
        Ok(IRemoteUserData::new(r#ref, sync_data))
    }

    /// `parseSyncData`.
    pub fn parse_sync_data(&self, content: &str) -> Result<ISyncData, Error> {
        match parse_json(content) {
            Ok(value) => {
                if let Some(sync_data) = ISyncData::from_json(&value) {
                    return Ok(sync_data);
                }
            }
            Err(error) => self.log_error(&error, &[]),
        }
        Err(UserDataSyncError::new(
            localize("incompatible sync data", "Cannot parse sync data as it is not compatible with the current version.", &[]),
            UserDataSyncErrorCode::IncompatibleRemoteContent,
            Some(self.resource),
        )
        .into_error())
    }

    fn get_user_data(&self, last_sync_data: Option<IRemoteUserData>) -> Promise<IUserData> {
        let this = self.rc();
        async_fn(async move {
            let last_sync_user_data = last_sync_data.map(|last_sync_data| IUserData {
                r#ref: last_sync_data.r#ref.clone(),
                content: last_sync_data.sync_data.as_ref().map(|sync_data| stringify_json(&sync_data.to_json())),
            });
            this.services.user_data_sync_store_service.read_resource(ServerResource::from(this.resource), last_sync_user_data, this.collection.as_deref(), Some(this.sync_headers())).await
        })
    }

    /// `updateRemoteUserData`.
    pub fn update_remote_user_data(&self, content: String, r#ref: Option<String>) -> Promise<IRemoteUserData> {
        let this = self.rc();
        async_fn(async move {
            let machine_id = this.current_machine_id_promise.clone().await?;
            let sync_data = ISyncData { version: this.imp().version(), machine_id: Some(machine_id), content };
            let written = this
                .services
                .user_data_sync_store_service
                .write_resource(ServerResource::from(this.resource), &stringify_json(&sync_data.to_json()), r#ref.as_deref(), this.collection.as_deref(), Some(this.sync_headers()))
                .await;
            match written {
                Ok(r#ref) => Ok(IRemoteUserData::new(r#ref, Some(sync_data))),
                Err(error) => match UserDataSyncError::from_error(&error) {
                    Some(sync_error) if sync_error.code == UserDataSyncErrorCode::TooLarge => Err(UserDataSyncError::new(sync_error.message, sync_error.code, Some(this.resource)).into_error()),
                    _ => Err(error),
                },
            }
        })
    }

    /// `backupLocal`.
    pub fn backup_local(&self, content: &str) -> Promise<()> {
        let sync_data = ISyncData { version: self.imp().version(), machine_id: None, content: content.to_string() };
        let profile_id = if self.sync_resource.profile.is_default { None } else { Some(self.sync_resource.profile.id.clone()) };
        self.services.user_data_sync_local_store_service.write_resource(ServerResource::from(self.resource), &stringify_json(&sync_data.to_json()), date_now() as f64, profile_id.as_deref(), None)
    }
}

impl<I: SynchroniserImpl> IDisposable for AbstractSynchroniser<I> {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl<I: SynchroniserImpl> IUserDataSynchroniser for AbstractSynchroniser<I> {
    fn resource(&self) -> SyncResource {
        self.resource
    }

    fn status(&self) -> SyncStatus {
        self.state.borrow().status
    }

    fn on_did_change_status(&self) -> Event<SyncStatus> {
        self.on_did_change_status.event()
    }

    /// `{ ...this.syncResource, conflicts: this._conflicts }`.
    fn conflicts(&self) -> IUserDataSyncResourceConflicts {
        IUserDataSyncResourceConflicts { sync_resource: self.resource, profile: self.sync_resource.profile.clone(), conflicts: self.state.borrow().conflicts.clone() }
    }

    fn on_did_change_conflicts(&self) -> Event<IUserDataSyncResourceConflicts> {
        self.on_did_change_conflicts.event()
    }

    fn on_did_change_local(&self) -> Event<()> {
        self.on_did_change_local.event()
    }

    fn sync(&self, ref_or_user_data: Option<RefOrUserData>, preview: bool, user_data_sync_configuration: IUserDataSyncConfiguration, headers: IHeaders) -> Promise<Option<IUserDataSyncResourcePreview>> {
        let this = self.rc();
        async_fn(async move {
            this.state.borrow_mut().sync_headers = headers;
            let result = Self::sync_body(this.clone(), ref_or_user_data, preview, user_data_sync_configuration).await;
            this.state.borrow_mut().sync_headers = IHeaders::new();
            result
        })
    }

    fn accept(&self, resource: &URI, content: Option<Option<String>>) -> Promise<Option<IUserDataSyncResourcePreview>> {
        let preview = self.accept_preview(resource, content);
        async_fn(async move { Ok(preview.await?.map(|preview| preview.to_base())) })
    }

    fn apply(&self, force: bool, headers: IHeaders) -> Promise<Option<IUserDataSyncResourcePreview>> {
        let this = self.rc();
        async_fn(async move {
            this.state.borrow_mut().sync_headers = headers;
            let applied = this.do_apply(force).await;
            let result = match applied {
                Ok(status) => {
                    this.set_status(status);
                    await_optional(this.sync_preview_promise()).await
                }
                Err(error) => Err(error),
            };
            this.state.borrow_mut().sync_headers = IHeaders::new();
            Ok(result?.map(|preview| preview.to_base()))
        })
    }

    fn stop(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let status = this.state.borrow().status;
            if status == SyncStatus::Idle {
                return Ok(());
            }

            this.services.log_service.trace(&format!("{}: Stopping synchronizing {}.", this.sync_resource_log_label, this.resource_lower_case()));
            let sync_preview_promise = this.state.borrow_mut().sync_preview_promise.take();
            if let Some(sync_preview_promise) = sync_preview_promise {
                sync_preview_promise.cancel();
            }

            this.update_conflicts(&[]);
            this.clear_preview_folder().await?;

            this.set_status(SyncStatus::Idle);
            this.services.log_service.info(&format!("{}: Stopped synchronizing {}.", this.sync_resource_log_label, this.resource_lower_case()));
            Ok(())
        })
    }

    fn has_previously_synced(&self) -> Promise<bool> {
        let last_sync_data = self.get_last_sync_user_data();
        async_fn(async move {
            let last_sync_data = last_sync_data.await?;
            // `null` sync data implies resource is not synced
            Ok(last_sync_data.is_some_and(|last_sync_data| last_sync_data.sync_data.is_some()))
        })
    }

    fn has_local_data(&self) -> Promise<bool> {
        self.imp().has_local_data()
    }

    fn reset_local(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            this.services.storage_service.remove(&this.last_sync_user_data_state_key, StorageScope::Application);
            if let Err(error) = this.services.file_service.del(&this.last_sync_resource, None).await
                && to_file_operation_result(&error) != FileOperationResult::FileNotFound
            {
                this.log_error(&error, &[]);
            }
            Ok(())
        })
    }

    fn resolve_content(&self, resource: &URI) -> Promise<Option<String>> {
        self.imp().resolve_content(resource.clone())
    }

    fn replace(&self, content: &str) -> Promise<bool> {
        let this = self.rc();
        let content = content.to_string();
        async_fn(async move {
            let sync_data = this.parse_sync_data(&content)?;

            this.stop().await?;

            let result = Self::replace_body(this.clone(), sync_data).await;
            this.set_status(SyncStatus::Idle);
            result?;

            Ok(true)
        })
    }
}

impl<I: SynchroniserImpl> AbstractSynchroniser<I> {
    async fn sync_body(this: Rc<Self>, ref_or_user_data: Option<RefOrUserData>, preview: bool, user_data_sync_configuration: IUserDataSyncConfiguration) -> Result<Option<IUserDataSyncResourcePreview>, Error> {
        let status = this.state.borrow().status;
        if status == SyncStatus::HasConflicts {
            this.services.log_service.info(&format!("{}: Skipped synchronizing {} as there are conflicts.", this.sync_resource_log_label, this.resource_lower_case()));
            return Ok(await_optional(this.sync_preview_promise()).await?.map(|preview| preview.to_base()));
        }

        if status == SyncStatus::Syncing {
            this.services.log_service.info(&format!("{}: Skipped synchronizing {} as it is running already.", this.sync_resource_log_label, this.resource_lower_case()));
            return Ok(await_optional(this.sync_preview_promise()).await?.map(|preview| preview.to_base()));
        }

        this.services.log_service.trace(&format!("{}: Started synchronizing {}...", this.sync_resource_log_label, this.resource_lower_case()));
        this.set_status(SyncStatus::Syncing);

        let outcome = async {
            let last_sync_user_data = this.get_last_sync_user_data().await?;
            let remote_user_data = this.get_latest_remote_user_data(ref_or_user_data, last_sync_user_data.clone()).await?;
            let status = this
                .perform_sync(remote_user_data, last_sync_user_data, if preview { SyncStrategy::Preview } else { SyncStrategy::Merge }, user_data_sync_configuration)
                .await?;
            if status == SyncStatus::HasConflicts {
                this.services.log_service.info(&format!("{}: Detected conflicts while synchronizing {}.", this.sync_resource_log_label, this.resource_lower_case()));
            } else if status == SyncStatus::Idle {
                this.services.log_service.trace(&format!("{}: Finished synchronizing {}.", this.sync_resource_log_label, this.resource_lower_case()));
            }
            Ok::<_, Error>((status, this.sync_preview_promise()))
        }
        .await;
        // `finally`: o estado volta a `Idle` se algo lançou
        let final_status = outcome.as_ref().map_or(SyncStatus::Idle, |(status, _)| *status);
        this.set_status(final_status);
        let (_, sync_preview_promise) = outcome?;
        Ok(await_optional(sync_preview_promise).await?.map(|preview| preview.to_base()))
    }

    async fn replace_body(this: Rc<Self>, sync_data: ISyncData) -> Result<(), Error> {
        this.services.log_service.trace(&format!("{}: Started resetting {}...", this.sync_resource_log_label, this.resource_lower_case()));
        this.set_status(SyncStatus::Syncing);
        let imp = this.imp();
        let last_sync_user_data = this.get_last_sync_user_data().await?;
        let remote_user_data = this.get_latest_remote_user_data(None, last_sync_user_data.clone()).await?;
        let is_remote_data_from_current_machine = this.is_remote_data_from_current_machine(&remote_user_data).await?;

        /* use replace sync data */
        let resource_preview_results = imp
            .clone()
            .generate_sync_preview(
                IRemoteUserData::new(remote_user_data.r#ref.clone(), Some(sync_data)),
                last_sync_user_data.clone(),
                is_remote_data_from_current_machine,
                this.get_user_data_sync_configuration(),
                CancellationToken::None,
            )
            .await?;

        let mut resource_previews = Vec::new();
        for resource_preview_result in resource_preview_results {
            /* Accept remote resource */
            let accept_result = imp.clone().get_accept_result(resource_preview_result.clone(), resource_preview_result.remote_resource.clone(), None, CancellationToken::None).await?;
            /* compute remote change */
            let IAcceptResult { remote_change, .. } = imp
                .clone()
                .get_accept_result(resource_preview_result.clone(), resource_preview_result.preview_resource.clone(), Some(resource_preview_result.remote_content.clone()), CancellationToken::None)
                .await?;
            let remote_change = if remote_change != Change::None { remote_change } else { Change::Modified };
            resource_previews.push((resource_preview_result, IAcceptResult { remote_change, ..accept_result }));
        }

        imp.apply_result(remote_user_data, last_sync_user_data, resource_previews, false).await?;
        this.services.log_service.info(&format!("{}: Finished resetting {}.", this.sync_resource_log_label, this.resource_lower_case()));
        Ok(())
    }
}

// region: AbstractFileSynchroniser

/// `IFileResourcePreview`: o preview de um recurso que é um arquivo, com o conteúdo lido dele.
#[derive(Clone, Debug)]
pub struct IFileResourcePreview<P> {
    pub preview: IResourcePreview<P>,
    pub file_content: Option<FileContent>,
}

/// `AbstractFileSynchroniser`: um sincronizador de um recurso que é um arquivo. Dá `Deref` para a base.
pub struct AbstractFileSynchroniser<I: SynchroniserImpl> {
    base: Rc<AbstractSynchroniser<I>>,
    pub file: URI,
}

impl<I: SynchroniserImpl> Deref for AbstractFileSynchroniser<I> {
    type Target = AbstractSynchroniser<I>;

    fn deref(&self) -> &AbstractSynchroniser<I> {
        &self.base
    }
}

impl<I: SynchroniserImpl> AbstractFileSynchroniser<I> {
    pub fn new(file: URI, sync_resource: IUserDataSyncResource, collection: Option<String>, services: SynchroniserServices) -> Self {
        let file_service = services.file_service.clone();
        let base = AbstractSynchroniser::new(sync_resource, collection, services);
        base.register(file_service.watch(&base.services.ext_uri.dirname(&file), None));
        let (weak, watched) = (Rc::downgrade(&base), file.clone());
        base.register(file_service.on_did_files_change().listen(move |event| {
            if let Some(base) = weak.upgrade()
                && event.contains(&watched, &[])
            {
                base.trigger_local_change();
            }
        }));
        Self { base, file }
    }

    /// `getLocalFileContent`: `None` se o arquivo não pode ser lido.
    pub fn get_local_file_content(&self) -> Promise<Option<FileContent>> {
        let (file_service, file) = (self.services.file_service.clone(), self.file.clone());
        async_fn(async move { Ok(file_service.read_file(&file, None, None).await.ok()) })
    }

    /// `updateLocalFileContent`: grava o arquivo, só se ele não mudou desde `old_content` (a menos que `force`).
    pub fn update_local_file_content(&self, new_content: &str, old_content: Option<FileContent>, force: bool) -> Promise<()> {
        let (file_service, file, new_content) = (self.services.file_service.clone(), self.file.clone(), new_content.to_string());
        async_fn(async move {
            let written = match old_content {
                // file exists already
                Some(old_content) => {
                    let options = if force { None } else { Some(WriteFileOptions { mtime: Some(old_content.mtime()), etag: Some(old_content.etag().to_string()), ..WriteFileOptions::default() }) };
                    file_service.write_file(&file, FileContentSource::from(new_content.as_str()), options).await.map(|_| ())
                }
                // file does not exist
                None => file_service.create_file(&file, Some(FileContentSource::from(new_content.as_str())), Some(CreateFileOptions { overwrite: force })).await.map(|_| ()),
            };
            written.map_err(|error| match FileOperationError::from_error(&error) {
                Some(file_error) if matches!(file_error.file_operation_result, FileOperationResult::FileNotFound | FileOperationResult::FileModifiedSince) => {
                    UserDataSyncError::new(file_error.message, UserDataSyncErrorCode::LocalPreconditionFailed, None).into_error()
                }
                _ => error,
            })
        })
    }

    /// `deleteLocalFile`: um arquivo que já não existe não é erro.
    pub fn delete_local_file(&self) -> Promise<()> {
        let (file_service, file) = (self.services.file_service.clone(), self.file.clone());
        async_fn(async move {
            match file_service.del(&file, None).await {
                Err(error) if !FileOperationError::from_error(&error).is_some_and(|file_error| file_error.file_operation_result == FileOperationResult::FileNotFound) => Err(error),
                _ => Ok(()),
            }
        })
    }
}

impl<I: SynchroniserImpl> IDisposable for AbstractFileSynchroniser<I> {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// endregion

// region: AbstractJsonFileSynchroniser

/// `AbstractJsonFileSynchroniser`: um sincronizador de um arquivo JSON. Dá `Deref` para o de arquivo.
pub struct AbstractJsonFileSynchroniser<I: SynchroniserImpl> {
    file_synchroniser: AbstractFileSynchroniser<I>,
    pub user_data_sync_util_service: Rc<dyn IUserDataSyncUtilService>,
    formatting_options: RefCell<Option<Promise<FormattingOptions>>>,
}

impl<I: SynchroniserImpl> Deref for AbstractJsonFileSynchroniser<I> {
    type Target = AbstractFileSynchroniser<I>;

    fn deref(&self) -> &AbstractFileSynchroniser<I> {
        &self.file_synchroniser
    }
}

impl<I: SynchroniserImpl> AbstractJsonFileSynchroniser<I> {
    pub fn new(file: URI, sync_resource: IUserDataSyncResource, collection: Option<String>, services: SynchroniserServices, user_data_sync_util_service: Rc<dyn IUserDataSyncUtilService>) -> Self {
        Self { file_synchroniser: AbstractFileSynchroniser::new(file, sync_resource, collection, services), user_data_sync_util_service, formatting_options: RefCell::new(None) }
    }

    /// `hasErrors`: o conteúdo não é JSON válido, ou não é do tipo esperado (array ou objeto).
    pub fn has_errors(&self, content: &str, is_array: bool) -> bool {
        let units: Vec<u16> = content.encode_utf16().collect();
        let mut parse_errors = Vec::new();
        let result = json::parse(&units, &mut parse_errors, ParseOptions { disallow_comments: false, allow_trailing_comma: true, allow_empty_content: true });
        !parse_errors.is_empty() || result.is_some_and(|result| is_array != matches!(result, Value::Array(_)))
    }

    /// `getFormattingOptions`: resolvidas uma vez e guardadas.
    pub fn get_formatting_options(&self) -> Promise<FormattingOptions> {
        self.formatting_options.borrow_mut().get_or_insert_with(|| self.user_data_sync_util_service.resolve_formatting_options(Some(&self.file))).clone()
    }
}

impl<I: SynchroniserImpl> IDisposable for AbstractJsonFileSynchroniser<I> {
    fn dispose(&self) {
        self.file_synchroniser.dispose();
    }
}

// endregion

// region: AbstractInitializer

/// O que `AbstractInitializer` deixa abstrato: `doInitialize`.
pub trait InitializerImpl {
    fn do_initialize(&self, remote_user_data: IRemoteUserData) -> Promise<()>;
}

/// `AbstractInitializer`.
pub struct AbstractInitializer {
    me: Weak<Self>,
    imp: RefCell<Option<Weak<dyn InitializerImpl>>>,
    pub resource: SyncResource,
    pub user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
    pub environment_service: Rc<dyn EnvironmentService>,
    pub log_service: crate::log::LogServiceRef,
    pub file_service: Rc<FileService>,
    pub storage_service: Rc<dyn StorageService>,
    pub ext_uri: ExtUri,
    last_sync_resource: URI,
}

impl AbstractInitializer {
    pub fn new(
        resource: SyncResource,
        user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
        environment_service: Rc<dyn EnvironmentService>,
        log_service: crate::log::LogServiceRef,
        file_service: Rc<FileService>,
        storage_service: Rc<dyn StorageService>,
        ext_uri: ExtUri,
    ) -> Rc<Self> {
        let last_sync_resource = get_last_sync_resource_uri(None, resource, environment_service.as_ref(), &ext_uri);
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            imp: RefCell::new(None),
            resource,
            user_data_profiles_service,
            environment_service,
            log_service,
            file_service,
            storage_service,
            ext_uri,
            last_sync_resource,
        })
    }

    /// Liga o inicializador concreto.
    pub fn bind(&self, imp: Weak<dyn InitializerImpl>) {
        *self.imp.borrow_mut() = Some(imp);
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o inicializador está vivo enquanto é usado")
    }

    /// `parseSyncData`: `None` (e um log) se o conteúdo não é um [`ISyncData`].
    fn parse_sync_data(&self, content: &str) -> Option<ISyncData> {
        match parse_json(content) {
            Ok(value) => {
                if let Some(sync_data) = ISyncData::from_json(&value) {
                    return Some(sync_data);
                }
            }
            Err(error) => self.log_service.error_args(&LogMessage::from(&error), &[]),
        }
        self.log_service.info_args("Cannot parse sync data as it is not compatible with the current version.", &[LogArg::from(self.resource.as_str())]);
        None
    }

    /// `updateLastSyncUserData`.
    pub fn update_last_sync_user_data(&self, last_sync_remote_user_data: IRemoteUserData, additional_props: Vec<(String, Value)>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            if additional_props.iter().any(|(key, value)| (key == "ref" || key == "version") && is_truthy_value(value)) {
                return Err(Error::new("Cannot have core properties as additional"));
            }

            let last_sync_user_data_state = last_sync_user_data_state_json(&last_sync_remote_user_data.r#ref, None, &additional_props);

            this.storage_service.store(&format!("{}.lastSyncUserData", this.resource.as_str()), StorageValue::from(last_sync_user_data_state.as_str()), StorageScope::Application, StorageTarget::Machine);
            this.file_service.write_file(&this.last_sync_resource, FileContentSource::from(stringify_json(&last_sync_remote_user_data.to_json()).as_str()), None).await.map(|_| ())
        })
    }
}

impl IUserDataSyncResourceInitializer for AbstractInitializer {
    fn initialize(&self, IUserData { r#ref, content }: IUserData) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let Some(content) = content.filter(|content| !content.is_empty()) else {
                this.log_service.info_args("Remote content does not exist.", &[LogArg::from(this.resource.as_str())]);
                return Ok(());
            };

            let Some(sync_data) = this.parse_sync_data(&content) else { return Ok(()) };

            let imp = this.imp.borrow().as_ref().and_then(Weak::upgrade).expect("o inicializador concreto foi ligado e está vivo");
            if let Err(error) = imp.do_initialize(IRemoteUserData::new(r#ref, Some(sync_data))).await {
                this.log_service.error_args(&LogMessage::from(&error), &[]);
            }
            Ok(())
        })
    }
}

// endregion
