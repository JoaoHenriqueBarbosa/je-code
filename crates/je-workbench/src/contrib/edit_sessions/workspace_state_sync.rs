//! Porte de `src/vs/workbench/contrib/editSessions/common/workspaceStateSync.ts`: o sincronizador do estado de workspace
//! (`workspaceState`) que acompanha uma edit session. `sync` grava no servidor do Cloud Changes o armazenamento de escopo de
//! workspace e de alvo de usuário, junto da identidade de cada pasta, e `apply` lê esse estado de volta, troca os URIs das pastas
//! da outra máquina pelos das pastas daqui e o grava no armazenamento deste workspace.
//!
//! Divergências:
//! - **Injeção.** Os serviços decorados do construtor são os campos de [`WorkspaceStateSynchroniserServices`]; `profile`,
//!   `collection`, o `userDataSyncStoreService` e o `logService` (os quatro primeiros parâmetros) seguem como argumentos de
//!   [`WorkspaceStateSynchroniser::new`]. O `NullBackupStoreService` e o `NullEnablementService` são os mesmos do original.
//! - **Herança.** `extends AbstractSynchroniser` é o `Deref` para a [`AbstractSynchroniser`] (como o `GlobalStateSynchroniser`), e
//!   os métodos abstratos são o [`SynchroniserImpl`]. O `sync()` e o `apply()` do original, sem argumentos, são métodos próprios que
//!   escondem os da base (`IUserDataSynchroniser::sync` e `apply`, que o porte da base implementa com outra assinatura); a base não
//!   chama nenhum dos dois, então a substituição do original (`override`) não tem outro efeito.
//! - Os métodos que o original deixa lançar `new Error('Method not implemented.')` devolvem a promessa rejeitada com esse erro
//!   (o lançamento síncrono vira a rejeição, como em qualquer chamada dentro de um `async`).
//! - **`IWorkspaceState`.** `{ folders, storage, version }` é montado como objeto JSON, com as chaves nessa ordem; `stringify` e
//!   `parse` são os de `marshalling`. Uma pasta sem `resourceUri` ou `workspaceFolderIdentity` de texto entra com o texto
//!   `undefined`, que é o que o JavaScript faz ao usar o valor como texto ou como chave; um estado sem `folders` em lista, ou sem
//!   `storage` em objeto, rejeita com `TypeError`, que é o que o `for...of` e o `Object.keys` do original lançam.
//! - **Valor armazenado.** `parse(storage[key])` seguido de `replaceUris(value)` e `storeAll` grava o valor revivido: um objeto ou
//!   lista é gravado como o `stringify` de `marshalling` o escreve (o que o armazenamento faz com um objeto), e um valor que o
//!   `replaceUris` rejeita (um texto não vazio) cai no `catch` e é gravado como veio.

use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationTokenSource;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::json::{Object, Value};
use je_base::lifecycle::IDisposable;
use je_base::marshalling::{self, Marshalled};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::host_platform;
use je_platform::storage::{StorageEntry, StorageScope, StorageService, StorageTarget, WillSaveStateReason};
use je_platform::telemetry::ITelemetryService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::user_data_profile::UserDataProfile;
use je_platform::user_data_sync::abstract_synchronizer::{
    AbstractSynchroniser, IAcceptResult, IMergeResult, IResourcePreview, ISyncResourcePreview, SynchroniserImpl, SynchroniserServices,
};
use je_platform::user_data_sync::user_data_sync::{
    IRemoteUserData, IResourceRefHandle, IUserDataSyncConfiguration, IUserDataSyncEnablementService, IUserDataSyncLocalStoreService, IUserDataSyncLogService,
    IUserDataSyncResource, IUserDataSyncResourcePreview, IUserDataSyncStoreService, IWorkspaceStateFolder, ServerResource, SyncResource as SettingsSyncResource,
};
use je_storage::StorageValue;

use super::common::{EditSession, EditSessionContent, IEditSessionsStorageService, SyncResource};
use crate::services::workspaces::common::workspace_identity_service::IWorkspaceIdentityService;

/// `protected override version: number = 1`.
const WORKSPACE_STATE_DATA_VERSION: f64 = 1.0;

/// O `new Error('Method not implemented.')`.
fn not_implemented() -> Error {
    Error::new("Method not implemented.")
}

/// `NullBackupStoreService`.
struct NullBackupStoreService;

impl IUserDataSyncLocalStoreService for NullBackupStoreService {
    fn write_resource(&self, _resource: ServerResource, _content: &str, _c_time: f64, _collection: Option<&str>, _root: Option<&URI>) -> Promise<()> {
        Promise::resolved(())
    }

    fn get_all_resource_refs(&self, _resource: ServerResource, _collection: Option<&str>, _root: Option<&URI>) -> Promise<Vec<IResourceRefHandle>> {
        Promise::resolved(Vec::new())
    }

    fn resolve_resource_content(&self, _resource: ServerResource, _ref: &str, _collection: Option<&str>, _root: Option<&URI>) -> Promise<Option<String>> {
        Promise::resolved(None)
    }
}

/// `NullEnablementService`.
struct NullEnablementService {
    on_did_change_enablement: Rc<Emitter<bool>>,
    on_did_change_resource_enablement: Rc<Emitter<(SettingsSyncResource, bool)>>,
}

impl NullEnablementService {
    fn new() -> Self {
        Self { on_did_change_enablement: Emitter::new(), on_did_change_resource_enablement: Emitter::new() }
    }
}

impl IUserDataSyncEnablementService for NullEnablementService {
    fn on_did_change_enablement(&self) -> Event<bool> {
        self.on_did_change_enablement.event()
    }

    fn is_enabled(&self) -> bool {
        true
    }

    fn can_toggle_enablement(&self) -> bool {
        true
    }

    fn set_enablement(&self, _enabled: bool) {}

    fn on_did_change_resource_enablement(&self) -> Event<(SettingsSyncResource, bool)> {
        self.on_did_change_resource_enablement.event()
    }

    fn is_resource_enabled(&self, _resource: SettingsSyncResource, _default_value: Option<bool>) -> bool {
        true
    }

    fn set_resource_enablement(&self, _resource: SettingsSyncResource, _enabled: bool) {}

    fn get_resource_sync_state_version(&self, _resource: SettingsSyncResource) -> Option<String> {
        None
    }

    fn is_resource_enablement_configured(&self, _resource: SettingsSyncResource) -> bool {
        false
    }
}

/// Os serviços que o `WorkspaceStateSynchroniser` recebe por injeção (`@IFileService` e companhia).
#[derive(Clone)]
pub struct WorkspaceStateSynchroniserServices {
    pub file_service: Rc<dyn IFileService>,
    pub environment_service: Rc<dyn EnvironmentService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub storage_service: Rc<dyn StorageService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
    pub workspace_identity_service: Rc<dyn IWorkspaceIdentityService>,
    pub edit_sessions_storage_service: Rc<dyn IEditSessionsStorageService>,
}

/// `WorkspaceStateSynchroniser`.
pub struct WorkspaceStateSynchroniser {
    pub base: Rc<AbstractSynchroniser<WorkspaceStateSynchroniser>>,
    workspace_identity_service: Rc<dyn IWorkspaceIdentityService>,
    edit_sessions_storage_service: Rc<dyn IEditSessionsStorageService>,
}

impl std::ops::Deref for WorkspaceStateSynchroniser {
    type Target = AbstractSynchroniser<WorkspaceStateSynchroniser>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl IDisposable for WorkspaceStateSynchroniser {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O `!value` do JavaScript sobre um valor revivido.
fn is_falsy(value: &Marshalled) -> bool {
    match value {
        Marshalled::Undefined | Marshalled::Null => true,
        Marshalled::Bool(flag) => !flag,
        Marshalled::Number(number) => *number == 0.0 || number.is_nan(),
        Marshalled::String(text) => text.is_empty(),
        _ => false,
    }
}

/// `value[name]` de um objeto revivido.
fn property<'a>(value: &'a Marshalled, name: &str) -> Option<&'a Marshalled> {
    match value {
        Marshalled::Object(entries) => entries.iter().find(|(key, _)| key == name).map(|(_, value)| value),
        _ => None,
    }
}

/// O texto de um valor que o original usa como texto: o próprio texto, ou `undefined`.
fn text_or_undefined(value: Option<&Marshalled>) -> String {
    match value {
        Some(Marshalled::String(text)) => text.to_string_lossy(),
        _ => "undefined".to_string(),
    }
}

/// `remoteWorkspaceState.folders`.
fn incoming_folders(remote_workspace_state: &Marshalled) -> Result<Vec<IWorkspaceStateFolder>, Error> {
    match property(remote_workspace_state, "folders") {
        Some(Marshalled::Array(items)) => Ok(items
            .iter()
            .map(|item| IWorkspaceStateFolder {
                resource_uri: text_or_undefined(property(item, "resourceUri")),
                workspace_folder_identity: text_or_undefined(property(item, "workspaceFolderIdentity")),
            })
            .collect()),
        _ => Err(Error::named("TypeError", "remoteWorkspaceState.folders is not iterable")),
    }
}

/// `Object.keys(remoteWorkspaceState.storage)` e os valores, na ordem do objeto.
fn incoming_storage(remote_workspace_state: &Marshalled) -> Result<Vec<(String, Marshalled)>, Error> {
    match property(remote_workspace_state, "storage") {
        Some(Marshalled::Object(entries)) => Ok(entries.iter().map(|(key, value)| (key.to_string_lossy(), value.clone())).collect()),
        _ => Err(Error::named("TypeError", "Cannot convert undefined or null to object")),
    }
}

/// O valor que `storeAll` recebe (ver as divergências do módulo).
fn to_storage_value(value: &Marshalled) -> StorageValue {
    match value {
        Marshalled::Undefined => StorageValue::Undefined,
        Marshalled::Null => StorageValue::Null,
        Marshalled::Bool(flag) => StorageValue::Bool(*flag),
        Marshalled::Number(number) => StorageValue::Number(*number),
        Marshalled::String(text) => StorageValue::String(text.to_string_lossy()),
        other => StorageValue::String(marshalling::stringify(other).to_string_lossy()),
    }
}

impl WorkspaceStateSynchroniser {
    pub fn new(
        profile: UserDataProfile,
        collection: Option<String>,
        user_data_sync_store_service: Rc<dyn IUserDataSyncStoreService>,
        log_service: IUserDataSyncLogService,
        services: WorkspaceStateSynchroniserServices,
    ) -> Rc<Self> {
        let WorkspaceStateSynchroniserServices {
            file_service,
            environment_service,
            telemetry_service,
            configuration_service,
            storage_service,
            uri_identity_service,
            workspace_identity_service,
            edit_sessions_storage_service,
        } = services;

        let base = AbstractSynchroniser::<Self>::new(
            IUserDataSyncResource { sync_resource: SettingsSyncResource::WorkspaceState, profile },
            collection,
            SynchroniserServices {
                file_service,
                environment_service,
                storage_service,
                user_data_sync_store_service,
                user_data_sync_local_store_service: Rc::new(NullBackupStoreService),
                user_data_sync_enablement_service: Rc::new(NullEnablementService::new()),
                telemetry_service,
                log_service,
                configuration_service,
                ext_uri: uri_identity_service.ext_uri(),
            },
        );
        let this = Rc::new(Self { base: base.clone(), workspace_identity_service, edit_sessions_storage_service });
        base.bind(Rc::downgrade(&this));
        this
    }

    /// `sync()`: grava o estado de workspace no servidor. Sempre `null` (sem pré-visualização).
    pub fn sync(&self) -> Promise<Option<IUserDataSyncResourcePreview>> {
        let workspace_identity_service = self.workspace_identity_service.clone();
        let storage_service = self.services.storage_service.clone();
        let edit_sessions_storage_service = self.edit_sessions_storage_service.clone();
        async_fn(async move {
            let cancellation_token_source = CancellationTokenSource::new(None);
            let folders = workspace_identity_service.get_workspace_state_folders(cancellation_token_source.token()).await?;
            if folders.is_empty() {
                return Ok(None);
            }

            // Ensure we have latest state by sending out onWillSaveState event
            storage_service.flush(WillSaveStateReason::None).await?;

            let keys = storage_service.keys(StorageScope::Workspace, StorageTarget::User);
            if keys.is_empty() {
                return Ok(None);
            }

            let mut contributed_data = Object::new();
            for key in &keys {
                if let Some(data) = storage_service.get(key, StorageScope::Workspace, None).filter(|data| !data.is_empty()) {
                    contributed_data.insert(JsString::from(key.as_str()), Value::String(JsString::from(data.as_str())));
                }
            }

            let folder_values = folders
                .iter()
                .map(|folder| {
                    let mut object = Object::new();
                    object.insert(JsString::from("resourceUri"), Value::String(JsString::from(folder.resource_uri.as_str())));
                    object.insert(JsString::from("workspaceFolderIdentity"), Value::String(JsString::from(folder.workspace_folder_identity.as_str())));
                    Value::Object(object)
                })
                .collect();
            let mut content = Object::new();
            content.insert(JsString::from("folders"), Value::Array(folder_values));
            content.insert(JsString::from("storage"), Value::Object(contributed_data));
            content.insert(JsString::from("version"), Value::Number(WORKSPACE_STATE_DATA_VERSION));

            let text = marshalling::stringify(&marshalling::from_json(&Value::Object(content))).to_string_lossy();
            edit_sessions_storage_service.write(SyncResource::WorkspaceState, EditSessionContent::Text(text)).await?;
            Ok(None)
        })
    }

    /// `apply()`: lê o estado de workspace do servidor e o grava no armazenamento deste workspace. Sempre `null`.
    pub fn apply(&self) -> Promise<Option<ISyncResourcePreview<(), ()>>> {
        let log_service = self.services.log_service.clone();
        let workspace_identity_service = self.workspace_identity_service.clone();
        let storage_service = self.services.storage_service.clone();
        let edit_sessions_storage_service = self.edit_sessions_storage_service.clone();
        async_fn(async move {
            let payload = edit_sessions_storage_service.last_read_resources().borrow().get(&SyncResource::EditSessions).map(|resource| resource.content.clone());
            let workspace_state_id = match payload.filter(|payload| !payload.is_empty()) {
                Some(payload) => EditSession::parse(&payload)?.workspace_state_id,
                None => None,
            };

            let Some(resource) = edit_sessions_storage_service.read(SyncResource::WorkspaceState, workspace_state_id).await? else {
                return Ok(None);
            };

            let content_units: Vec<u16> = resource.content.encode_utf16().collect();
            let remote_workspace_state = errors::catch(|| marshalling::parse(&content_units, host_platform()))?;
            if is_falsy(&remote_workspace_state) {
                log_service.info("Skipping initializing workspace state because remote workspace state does not exist.");
                return Ok(None);
            }

            // Evaluate whether storage is applicable for current workspace
            let cancellation_token_source = CancellationTokenSource::new(None);
            let replace_uris = workspace_identity_service.matches(incoming_folders(&remote_workspace_state)?, cancellation_token_source.token()).await?;
            let Some(replace_uris) = replace_uris else {
                log_service.info("Skipping initializing workspace state because remote workspace state does not match current workspace.");
                return Ok(None);
            };

            let storage = incoming_storage(&remote_workspace_state)?;
            if !storage.is_empty() {
                // Initialize storage with remote storage
                let mut storage_entries: Vec<StorageEntry> = Vec::new();
                for (key, stored) in &storage {
                    // Deserialize the stored state and run URI conversion on it; keep the value as it came when that fails
                    let converted = match stored {
                        Marshalled::String(text) => {
                            let units = text.units().to_vec();
                            errors::catch(|| replace_uris(marshalling::parse(&units, host_platform()))).ok()
                        }
                        _ => None,
                    };
                    let value = to_storage_value(converted.as_ref().unwrap_or(stored));
                    storage_entries.push(StorageEntry { key: key.clone(), value, scope: StorageScope::Workspace, target: StorageTarget::User });
                }
                storage_service.store_all(&storage_entries, true);
            }

            let _ = edit_sessions_storage_service.delete(SyncResource::WorkspaceState, Some(resource.r#ref.clone()));
            Ok(None)
        })
    }
}

impl SynchroniserImpl for WorkspaceStateSynchroniser {
    type Preview = ();
    type Accept = ();

    fn version(&self) -> f64 {
        WORKSPACE_STATE_DATA_VERSION
    }

    // O original deixa o resto da `AbstractSynchronizer` por implementar (`apply_result` e os métodos de pré-visualização).
    fn apply_result(
        self: Rc<Self>,
        _remote_user_data: IRemoteUserData,
        _last_sync_user_data: Option<IRemoteUserData>,
        _result: Vec<(IResourcePreview<()>, IAcceptResult<()>)>,
        _force: bool,
    ) -> Promise<()> {
        Promise::rejected(not_implemented())
    }

    fn generate_sync_preview(
        self: Rc<Self>,
        _remote_user_data: IRemoteUserData,
        _last_sync_user_data: Option<IRemoteUserData>,
        _is_remote_data_from_current_machine: bool,
        _user_data_sync_configuration: IUserDataSyncConfiguration,
        _token: je_base::cancellation::CancellationToken,
    ) -> Promise<Vec<IResourcePreview<()>>> {
        Promise::resolved(Vec::new())
    }

    fn get_merge_result(self: Rc<Self>, _resource_preview: IResourcePreview<()>, _token: je_base::cancellation::CancellationToken) -> Promise<IMergeResult<()>> {
        Promise::rejected(not_implemented())
    }

    fn get_accept_result(
        self: Rc<Self>,
        _resource_preview: IResourcePreview<()>,
        _resource: URI,
        _content: Option<Option<String>>,
        _token: je_base::cancellation::CancellationToken,
    ) -> Promise<IAcceptResult<()>> {
        Promise::rejected(not_implemented())
    }

    fn has_remote_changed(self: Rc<Self>, _last_sync_user_data: IRemoteUserData) -> Promise<bool> {
        Promise::resolved(true)
    }

    fn has_local_data(self: Rc<Self>) -> Promise<bool> {
        Promise::resolved(false)
    }

    fn resolve_content(self: Rc<Self>, _uri: URI) -> Promise<Option<String>> {
        Promise::resolved(None)
    }
}
