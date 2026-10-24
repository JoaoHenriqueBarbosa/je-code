//! Porte de `src/vs/workbench/services/workspaces/common/workspaceTrust.ts`: o `WorkspaceTrustEnablementService`, o
//! `WorkspaceTrustManagementService` (quais pastas e workspaces são confiáveis, a confiança por autoridade remota, a
//! confiança no workspace vazio guardada no `Memento`) e o `WorkspaceTrustRequestService` (os pedidos de confiança em
//! arquivos, recursos e no workspace).
//!
//! Divergências:
//! - O `extends Disposable` vira o campo `base` ([`Disposable`]); `this._store` é o campo `store`.
//! - As dependências por injeção (`@IConfigurationService`...) entram numa struct de partes
//!   ([`WorkspaceTrustManagementServiceParts`] e [`WorkspaceTrustEnablementServiceParts`]), e `registerSingleton` não tem
//!   equivalente: quem monta o workbench cria o serviço com o `new`.
//! - As funções `async` do original são [`async_fn`]; o que o original lança dentro delas é o `Err` que rejeita a
//!   promessa. O `promiseWithResolvers()` é o [`Promise::new`].
//! - As propriedades do original viram métodos (veja `je_platform::workspace_trust`); o setter privado `isTrusted` é
//!   [`WorkspaceTrustManagementService::set_trusted_state`].
//! - **Estado serializado.** `JSON.stringify(this._trustStateInfo)` escreve os cinco componentes do URI e o `$mid` (o
//!   `toJSON` do original também escreveria o `fsPath` e o `external` já calculados; aqui eles ficam de fora, para a
//!   comparação "o armazenamento mudou?" não depender de qual lado já calculou o cache). Uma entrada lida do
//!   armazenamento cujo `uri` não é um objeto é descartada (no original ela quebraria no primeiro uso).
//! - `isWeb` é [`Platform::detect`]; `isEqualAuthority` é o do [`ExtUri`](je_base::resources::ExtUri) do serviço de
//!   identidade de URIs (o mesmo `equalsIgnoreCase`).
//! - O `LinkedList` de participantes é iterado de forma preguiçosa, como o `for ... of` do original (um participante
//!   adicionado durante a transição entra no laço).
//! - `Event.once(...)(handler)` do `completeWorkspaceTrustRequest` é [`Event::once`], sem guardar a inscrição.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, ObjectRef, Truthy};
use je_base::errors::{Error, on_unexpected_error};
use je_base::event::{Emitter, Event};
use je_base::js_string::index_of;
use je_base::json::{self, Object, Value};
use je_base::json_formatter::json_stringify;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::linked_list::LinkedList;
use je_base::map::ResourceMap;
use je_base::network::schemas;
use je_base::platform::Platform;
use je_base::promise::{Promise, Resolver};
use je_base::uri::{Field, URI, UriChange, UriComponents, UriState};
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::i_file_service::IFileService;
use je_platform::js::from_json;
use je_platform::remote::remote_authority_resolver::{IRemoteAuthorityResolverService, ResolverResult};
use je_platform::remote::remote_hosts::get_remote_authority;
use je_platform::resources::host_platform;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::virtual_workspace::is_virtual_resource;
use je_platform::agent_host::common::agent_host_uri::AGENT_HOST_SCHEME;
use je_platform::workspace::{
    AnyWorkspaceIdentifier, IWorkspace, IWorkspaceContextService, WorkbenchState, WorkspaceFolder, is_saved_workspace, is_temporary_workspace, to_workspace_identifier,
};
use je_platform::workspace_trust::{
    IWorkspaceTrustEnablementService, IWorkspaceTrustManagementService, IWorkspaceTrustRequestService, IWorkspaceTrustTransitionParticipant, ResourceTrustRequestOptions,
    WorkspaceTrustInfo, WorkspaceTrustRequestOptions, WorkspaceTrustUriInfo, WorkspaceTrustUriResponse,
};
use je_storage::StorageValue;

use crate::common::memento::Memento;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;

pub const WORKSPACE_TRUST_ENABLED: &str = "security.workspace.trust.enabled";
pub const WORKSPACE_TRUST_STARTUP_PROMPT: &str = "security.workspace.trust.startupPrompt";
pub const WORKSPACE_TRUST_BANNER: &str = "security.workspace.trust.banner";
pub const WORKSPACE_TRUST_UNTRUSTED_FILES: &str = "security.workspace.trust.untrustedFiles";
pub const WORKSPACE_TRUST_EMPTY_WINDOW: &str = "security.workspace.trust.emptyWindow";
pub const WORKSPACE_TRUST_EXTENSION_SUPPORT: &str = "extensions.supportUntrustedWorkspaces";
pub const WORKSPACE_TRUST_STORAGE_KEY: &str = "content.trust.model.key";

/// `CanonicalWorkspace`.
pub struct CanonicalWorkspace {
    original_workspace: Rc<dyn IWorkspace>,
    canonical_folder_uris: Vec<URI>,
    canonical_configuration: Option<URI>,
}

impl CanonicalWorkspace {
    pub fn new(original_workspace: Rc<dyn IWorkspace>, canonical_folder_uris: Vec<URI>, canonical_configuration: Option<URI>) -> Self {
        Self { original_workspace, canonical_folder_uris, canonical_configuration }
    }
}

impl IWorkspace for CanonicalWorkspace {
    fn id(&self) -> String {
        self.original_workspace.id()
    }

    fn folders(&self) -> Vec<WorkspaceFolder> {
        self.original_workspace
            .folders()
            .into_iter()
            .enumerate()
            .map(|(index, folder)| WorkspaceFolder { index: folder.index, name: folder.name, uri: self.canonical_folder_uris[index].clone(), raw: folder.raw })
            .collect()
    }

    fn transient(&self) -> bool {
        self.original_workspace.transient()
    }

    fn configuration(&self) -> Option<URI> {
        self.canonical_configuration.clone().or_else(|| self.original_workspace.configuration())
    }

    fn name(&self) -> Option<String> {
        self.original_workspace.name()
    }
}

/// As dependências do [`WorkspaceTrustEnablementService`].
pub struct WorkspaceTrustEnablementServiceParts {
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub environment_service: Rc<dyn IWorkbenchEnvironmentService>,
}

/// `WorkspaceTrustEnablementService`.
pub struct WorkspaceTrustEnablementService {
    base: Rc<Disposable>,
    configuration_service: Rc<dyn IConfigurationService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
}

impl WorkspaceTrustEnablementService {
    pub fn new(parts: WorkspaceTrustEnablementServiceParts) -> Rc<Self> {
        Rc::new(Self { base: Disposable::new(), configuration_service: parts.configuration_service, environment_service: parts.environment_service })
    }
}

impl IDisposable for WorkspaceTrustEnablementService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkspaceTrustEnablementService for WorkspaceTrustEnablementService {
    fn is_workspace_trust_enabled(&self) -> bool {
        if self.environment_service.disable_workspace_trust() {
            return false;
        }

        self.configuration_service.get_value(Some(WORKSPACE_TRUST_ENABLED), None).is_truthy()
    }
}

/// `{ readonly scheme: string; readonly authority: string }` de `_trustedAuthorities`.
struct TrustedAuthority {
    scheme: String,
    authority: String,
}

/// As dependências do [`WorkspaceTrustManagementService`].
pub struct WorkspaceTrustManagementServiceParts {
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub remote_authority_resolver_service: Rc<dyn IRemoteAuthorityResolverService>,
    pub storage_service: Rc<dyn StorageService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
    pub environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    pub workspace_service: Rc<dyn IWorkspaceContextService>,
    pub workspace_trust_enablement_service: Rc<dyn IWorkspaceTrustEnablementService>,
    pub file_service: Rc<dyn IFileService>,
}

/// `WorkspaceTrustManagementService`.
pub struct WorkspaceTrustManagementService {
    base: Rc<Disposable>,
    store: Rc<DisposableStore>,
    this: Weak<Self>,

    workspace_resolved_promise: Promise<()>,
    workspace_resolved_resolve: Resolver<()>,
    workspace_trust_initialized_promise: Promise<()>,
    workspace_trust_initialized_resolve: Resolver<()>,

    on_did_change_trust: Rc<Emitter<bool>>,
    on_did_change_trusted_folders: Rc<Emitter<()>>,

    canonical_startup_files: RefCell<Vec<URI>>,
    canonical_workspace: RefCell<Rc<dyn IWorkspace>>,
    canonical_uris_resolved: Cell<bool>,

    is_trusted: Cell<bool>,
    trust_state_info: RefCell<WorkspaceTrustInfo>,
    remote_authority: RefCell<Option<ResolverResult>>,
    trusted_authorities: Rc<RefCell<Vec<Rc<TrustedAuthority>>>>,

    stored_trust_state: WorkspaceTrustMemento,
    trust_transition_manager: Rc<WorkspaceTrustTransitionManager>,

    configuration_service: Rc<dyn IConfigurationService>,
    remote_authority_resolver_service: Rc<dyn IRemoteAuthorityResolverService>,
    storage_service: Rc<dyn StorageService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    workspace_service: Rc<dyn IWorkspaceContextService>,
    workspace_trust_enablement_service: Rc<dyn IWorkspaceTrustEnablementService>,
    file_service: Rc<dyn IFileService>,
}

impl WorkspaceTrustManagementService {
    pub fn new(parts: WorkspaceTrustManagementServiceParts) -> Rc<Self> {
        let WorkspaceTrustManagementServiceParts {
            configuration_service,
            remote_authority_resolver_service,
            storage_service,
            uri_identity_service,
            environment_service,
            workspace_service,
            workspace_trust_enablement_service,
            file_service,
        } = parts;

        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());
        let on_did_change_trust = Emitter::new();
        base.register(on_did_change_trust.clone());
        let on_did_change_trusted_folders = Emitter::new();
        base.register(on_did_change_trusted_folders.clone());

        let (workspace_resolved_promise, workspace_resolved_resolve) = Promise::new();
        let (workspace_trust_initialized_promise, workspace_trust_initialized_resolve) = Promise::new();

        let is_empty = Self::compute_is_empty_workspace(workspace_service.as_ref());
        let stored_trust_state = WorkspaceTrustMemento::new(if Platform::detect().is_web && is_empty { None } else { Some(storage_service.clone()) });
        let trust_transition_manager = WorkspaceTrustTransitionManager::new();
        base.register(trust_transition_manager.clone());
        let trusted_authorities: Rc<RefCell<Vec<Rc<TrustedAuthority>>>> = Rc::new(RefCell::new(Vec::new()));
        base.register(to_disposable({
            let trusted_authorities = trusted_authorities.clone();
            move || trusted_authorities.borrow_mut().clear()
        }));

        let canonical_workspace: Rc<dyn IWorkspace> = workspace_service.get_workspace();

        let this = Rc::new_cyclic(|this| Self {
            base,
            store,
            this: this.clone(),
            workspace_resolved_promise,
            workspace_resolved_resolve,
            workspace_trust_initialized_promise,
            workspace_trust_initialized_resolve,
            on_did_change_trust,
            on_did_change_trusted_folders,
            canonical_startup_files: RefCell::new(Vec::new()),
            canonical_workspace: RefCell::new(canonical_workspace),
            canonical_uris_resolved: Cell::new(false),
            is_trusted: Cell::new(false),
            trust_state_info: RefCell::new(WorkspaceTrustInfo::default()),
            remote_authority: RefCell::new(None),
            trusted_authorities,
            stored_trust_state,
            trust_transition_manager,
            configuration_service,
            remote_authority_resolver_service,
            storage_service,
            uri_identity_service,
            environment_service,
            workspace_service,
            workspace_trust_enablement_service,
            file_service,
        });

        *this.trust_state_info.borrow_mut() = this.load_trust_info();
        this.is_trusted.set(this.calculate_workspace_trust());

        this.initialize_workspace_trust();
        this.register_listeners();
        this
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    //#region initialize

    fn initialize_workspace_trust(&self) {
        // Resolve canonical Uris
        let this = self.rc();
        let canonical = async_fn(async move {
            let outcome: Result<(), Error> = async {
                this.resolve_canonical_uris().await?;
                this.canonical_uris_resolved.set(true);
                this.update_workspace_trust(None).await
            }
            .await;
            this.workspace_resolved_resolve.resolve(());

            if this.environment_service.remote_authority().is_none() {
                this.workspace_trust_initialized_resolve.resolve(());
            }
            outcome
        });
        drop(canonical);

        // Remote - resolve remote authority
        if let Some(remote_authority) = self.environment_service.remote_authority() {
            let this = self.rc();
            let remote = async_fn(async move {
                let outcome: Result<(), Error> = async {
                    let result = this.remote_authority_resolver_service.resolve_authority(&remote_authority).await?;
                    *this.remote_authority.borrow_mut() = Some(result);
                    this.file_service.activate_provider(schemas::VSCODE_REMOTE).await?;
                    this.update_workspace_trust(None).await
                }
                .await;
                this.workspace_trust_initialized_resolve.resolve(());
                outcome
            });
            drop(remote);
        }

        // Empty workspace - save initial state to memento
        if self.is_empty_workspace() {
            let this = self.rc();
            drop(self.workspace_trust_initialized_promise.then(move |()| {
                if this.stored_trust_state.is_empty_workspace_trusted().is_none() {
                    this.stored_trust_state.set_is_empty_workspace_trusted(Some(this.is_workspace_trusted()));
                }
            }));
        }
    }

    //#endregion

    //#region private interface

    fn register_listeners(&self) {
        let this = self.this.clone();
        self.workspace_service.on_did_change_workspace_folders().listen_in(
            move |_| {
                if let Some(this) = this.upgrade() {
                    drop(this.update_workspace_trust(None));
                }
            },
            &self.store,
        );

        let this = self.this.clone();
        self.storage_service.on_did_change_value(StorageScope::ApplicationShared, Some(WORKSPACE_TRUST_STORAGE_KEY), &self.store).listen_in(
            move |_| {
                let Some(this) = this.upgrade() else { return };
                /* This will only execute if storage was changed by a user action in a separate window */
                let loaded = this.load_trust_info();
                if trust_info_to_json(&this.trust_state_info.borrow()) != trust_info_to_json(&loaded) {
                    *this.trust_state_info.borrow_mut() = loaded;
                    this.on_did_change_trusted_folders.fire(());

                    drop(this.update_workspace_trust(None));
                }
            },
            &self.store,
        );
    }

    fn get_canonical_uri_inner(&self, uri: &URI) -> Promise<URI> {
        let this = self.rc();
        let uri = uri.clone();
        async_fn(async move {
            let mut canonical_uri = uri.clone();
            if this.environment_service.remote_authority().is_some() && uri.scheme() == schemas::VSCODE_REMOTE {
                canonical_uri = this.remote_authority_resolver_service.get_canonical_uri(&uri).await?;
            } else if uri.scheme() == "vscode-vfs"
                && let Some(index) = index_of(uri.authority().units(), &[u16::from(b'+')], 0)
            {
                canonical_uri = uri.with(&UriChange { authority: Field::Set(uri.authority().slice(0, index)), ..UriChange::default() });
            }

            // ignore query and fragent section of uris always
            Ok(canonical_uri.with(&UriChange { query: Field::Clear, fragment: Field::Clear, ..UriChange::default() }))
        })
    }

    fn resolve_canonical_uris(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            // Open editors
            let mut files_to_open = Vec::new();
            if let Some(files) = this.environment_service.files_to_open_or_create() {
                files_to_open.extend(files);
            }

            if let Some(files) = this.environment_service.files_to_diff() {
                files_to_open.extend(files);
            }

            if let Some(files) = this.environment_service.files_to_merge() {
                files_to_open.extend(files);
            }

            if !files_to_open.is_empty() {
                let files_to_open_or_create_uris: Vec<URI> = files_to_open.into_iter().filter_map(|file| file.file_uri).collect();
                let canonical_files_to_open = Promise::all(files_to_open_or_create_uris.iter().map(|uri| this.get_canonical_uri_inner(uri)).collect()).await?;

                let ext_uri = this.uri_identity_service.ext_uri();
                let mut startup_files = this.canonical_startup_files.borrow_mut();
                // `filter` roda sobre a lista antes de qualquer `push`, como o `push(...filter(...))` do original.
                let additions: Vec<URI> = canonical_files_to_open.into_iter().filter(|uri| startup_files.iter().all(|other| !ext_uri.is_equal(Some(uri), Some(other), false))).collect();
                startup_files.extend(additions);
            }

            // Workspace
            let workspace = this.workspace_service.get_workspace();
            let workspace_uris: Vec<URI> = workspace.folders().iter().map(|folder| folder.uri.clone()).collect();
            let canonical_workspace_folders = Promise::all(workspace_uris.iter().map(|uri| this.get_canonical_uri_inner(uri)).collect()).await?;

            let mut canonical_workspace_configuration = this.workspace_service.get_workspace().configuration();
            if let Some(configuration) = &canonical_workspace_configuration
                && is_saved_workspace(configuration, &this.environment_service.untitled_workspaces_home())
            {
                canonical_workspace_configuration = Some(this.get_canonical_uri_inner(configuration).await?);
            }

            let original: Rc<dyn IWorkspace> = this.workspace_service.get_workspace();
            *this.canonical_workspace.borrow_mut() = Rc::new(CanonicalWorkspace::new(original, canonical_workspace_folders, canonical_workspace_configuration));
            Ok(())
        })
    }

    fn load_trust_info(&self) -> WorkspaceTrustInfo {
        let info_as_string = self.storage_service.get(WORKSPACE_TRUST_STORAGE_KEY, StorageScope::ApplicationShared, None);

        let mut uri_trust_info = Vec::new();
        if let Some(text) = info_as_string.filter(|text| !text.is_empty()) {
            let mut errors = Vec::new();
            // try { result = JSON.parse(infoAsString); } catch { }
            if let Some(value) = json::parse_str(&text, &mut errors).filter(|_| errors.is_empty())
                && let Dynamic::Object(result) = from_json(&value)
                && let Some(Dynamic::Array(items)) = result.get(&JsString::from("uriTrustInfo"))
            {
                for index in 0..items.len() {
                    let item = items.get(index);
                    let Dynamic::Object(item) = item else { continue };
                    let Some(Dynamic::Object(uri)) = item.get(&JsString::from("uri")) else { continue };
                    let trusted = item.get(&JsString::from("trusted")).is_some_and(|trusted| trusted.is_truthy());
                    // `result.uriTrustInfo.filter(info => info.trusted)`
                    if trusted {
                        uri_trust_info.push(WorkspaceTrustUriInfo { uri: revive_uri(&uri), trusted: true });
                    }
                }
            }
        }

        WorkspaceTrustInfo { uri_trust_info }
    }

    fn save_trust_info(&self) -> Promise<()> {
        self.storage_service.store(
            WORKSPACE_TRUST_STORAGE_KEY,
            StorageValue::String(trust_info_to_json(&self.trust_state_info.borrow())),
            StorageScope::ApplicationShared,
            StorageTarget::Machine,
        );
        self.on_did_change_trusted_folders.fire(());

        self.update_workspace_trust(None)
    }

    fn get_workspace_uris(&self) -> Vec<URI> {
        let canonical_workspace = self.canonical_workspace.borrow().clone();
        let mut workspace_uris: Vec<URI> = canonical_workspace.folders().iter().map(|folder| folder.uri.clone()).collect();
        if let Some(configuration) = canonical_workspace.configuration()
            && is_saved_workspace(&configuration, &self.environment_service.untitled_workspaces_home())
        {
            workspace_uris.push(configuration);
        }

        workspace_uris
    }

    fn calculate_workspace_trust(&self) -> bool {
        // Feature is disabled
        if !self.workspace_trust_enablement_service.is_workspace_trust_enabled() {
            return true;
        }

        // Canonical Uris not yet resolved
        if !self.canonical_uris_resolved.get() {
            return false;
        }

        // Remote - resolver explicitly sets workspace trust to TRUE
        if self.environment_service.remote_authority().is_some()
            && let Some(true) = self.remote_authority.borrow().as_ref().and_then(|remote| remote.options.as_ref()).and_then(|options| options.is_trusted)
        {
            return true;
        }

        // Empty workspace - use memento, open ediors, or user setting
        if self.is_empty_workspace() {
            // Use memento if present
            if let Some(trusted) = self.stored_trust_state.is_empty_workspace_trusted() {
                return trusted;
            }

            // Startup files
            let startup_files = self.canonical_startup_files.borrow().clone();
            if !startup_files.is_empty() {
                return self.get_uris_trust(&startup_files);
            }

            // User setting
            return self.configuration_service.get_value(Some(WORKSPACE_TRUST_EMPTY_WINDOW), None).is_truthy();
        }

        self.get_uris_trust(&self.get_workspace_uris())
    }

    fn update_workspace_trust(&self, trusted: Option<bool>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            if !this.workspace_trust_enablement_service.is_workspace_trust_enabled() {
                return Ok(());
            }

            let trusted = match trusted {
                Some(trusted) => trusted,
                None => {
                    this.resolve_canonical_uris().await?;
                    this.calculate_workspace_trust()
                }
            };

            if this.is_workspace_trusted() == trusted {
                return Ok(());
            }

            // Update workspace trust
            this.set_trusted_state(trusted);

            // Run workspace trust transition participants
            this.trust_transition_manager.participate(trusted).await?;

            // Fire workspace trust change event
            this.on_did_change_trust.fire(trusted);
            Ok(())
        })
    }

    fn get_uris_trust(&self, uris: &[URI]) -> bool {
        for uri in uris {
            let trusted = self.do_get_uri_trust_info(uri).trusted;

            if !trusted {
                return trusted;
            }
        }

        true
    }

    fn do_get_uri_trust_info(&self, uri: &URI) -> WorkspaceTrustUriInfo {
        // Return trusted when workspace trust is disabled
        if !self.workspace_trust_enablement_service.is_workspace_trust_enabled() {
            return WorkspaceTrustUriInfo { trusted: true, uri: uri.clone() };
        }

        let ext_uri = self.uri_identity_service.ext_uri();

        // Agent sessions workspace file is always trusted
        if ext_uri.is_equal(Some(uri), Some(&self.environment_service.agent_sessions_workspace()), false) {
            return WorkspaceTrustUriInfo { trusted: true, uri: uri.clone() };
        }

        if self.is_trusted_virtual_resource(uri) {
            return WorkspaceTrustUriInfo { trusted: true, uri: uri.clone() };
        }

        if self.is_trusted_by_remote(uri) {
            return WorkspaceTrustUriInfo { trusted: true, uri: uri.clone() };
        }

        let mut result_state = false;
        let mut max_length: i64 = -1;

        let mut result_uri = uri.clone();

        for trust_info in &self.trust_state_info.borrow().uri_trust_info {
            if ext_uri.is_equal_or_parent(uri, &trust_info.uri, false, false) {
                let fs_path_length = trust_info.uri.fs_path(host_platform()).len() as i64;
                if fs_path_length > max_length {
                    max_length = fs_path_length;
                    result_state = trust_info.trusted;
                    result_uri = trust_info.uri.clone();
                }
            }
        }

        WorkspaceTrustUriInfo { trusted: result_state, uri: result_uri }
    }

    fn do_set_uris_trust(&self, uris: Vec<URI>, trusted: bool) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let mut changed = false;
            let ext_uri = this.uri_identity_service.ext_uri();

            for uri in uris {
                if trusted {
                    if this.is_trusted_virtual_resource(&uri) {
                        continue;
                    }

                    if this.is_trusted_by_remote(&uri) {
                        continue;
                    }

                    let found = this.trust_state_info.borrow().uri_trust_info.iter().any(|trust_info| ext_uri.is_equal(Some(&trust_info.uri), Some(&uri), false));
                    if !found {
                        this.trust_state_info.borrow_mut().uri_trust_info.push(WorkspaceTrustUriInfo { uri, trusted: true });
                        changed = true;
                    }
                } else {
                    let previous_length = this.trust_state_info.borrow().uri_trust_info.len();
                    this.trust_state_info.borrow_mut().uri_trust_info.retain(|trust_info| !ext_uri.is_equal(Some(&trust_info.uri), Some(&uri), false));
                    if previous_length != this.trust_state_info.borrow().uri_trust_info.len() {
                        changed = true;
                    }
                }
            }

            if changed {
                this.save_trust_info().await?;
            }
            Ok(())
        })
    }

    fn compute_is_empty_workspace(workspace_service: &dyn IWorkspaceContextService) -> bool {
        if workspace_service.get_workbench_state() == WorkbenchState::Empty {
            return true;
        }

        let workspace = workspace_service.get_workspace();
        is_temporary_workspace(workspace.configuration().as_ref()) && workspace.folders().is_empty()
    }

    fn is_empty_workspace(&self) -> bool {
        Self::compute_is_empty_workspace(self.workspace_service.as_ref())
    }

    fn is_trusted_virtual_resource(&self, uri: &URI) -> bool {
        // `vscode-vfs` (e.g. GitHub Repositories) and `vscode-agent-host`
        // (remote agent host folders) represent real, writable resources where
        // code can run or files can change, so they must go through normal
        // workspace trust rather than being auto-trusted as virtual resources.
        is_virtual_resource(uri) && uri.scheme() != "vscode-vfs" && uri.scheme() != AGENT_HOST_SCHEME
    }

    fn is_trusted_by_remote(&self, uri: &URI) -> bool {
        let ext_uri = self.uri_identity_service.ext_uri();
        for trusted_authority in self.trusted_authorities.borrow().iter() {
            if uri.scheme() == trusted_authority.scheme.as_str() && ext_uri.is_equal_authority(uri.authority(), &JsString::from(trusted_authority.authority.as_str())) {
                return true;
            }
        }

        if self.environment_service.remote_authority().is_none() {
            return false;
        }

        let remote_authority = self.remote_authority.borrow();
        let Some(remote_authority) = remote_authority.as_ref() else { return false };

        get_remote_authority(uri).is_some_and(|authority| ext_uri.is_equal_authority(&authority, &JsString::from(remote_authority.authority.authority.as_str())))
            && remote_authority.options.as_ref().and_then(|options| options.is_trusted).unwrap_or(false)
    }

    /// `set isTrusted(value)`.
    fn set_trusted_state(&self, value: bool) {
        self.is_trusted.set(value);

        // Reset acceptsOutOfWorkspaceFiles
        if !value {
            self.stored_trust_state.set_accepts_out_of_workspace_files(false);
        }

        // Empty workspace - save memento
        if self.is_empty_workspace() {
            self.stored_trust_state.set_is_empty_workspace_trusted(Some(value));
        }
    }

    //#endregion
}

impl IDisposable for WorkspaceTrustManagementService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkspaceTrustManagementService for WorkspaceTrustManagementService {
    fn on_did_change_trust(&self) -> Event<bool> {
        self.on_did_change_trust.event()
    }

    fn on_did_change_trusted_folders(&self) -> Event<()> {
        self.on_did_change_trusted_folders.event()
    }

    fn workspace_resolved(&self) -> Promise<()> {
        self.workspace_resolved_promise.clone()
    }

    fn workspace_trust_initialized(&self) -> Promise<()> {
        self.workspace_trust_initialized_promise.clone()
    }

    fn accepts_out_of_workspace_files(&self) -> bool {
        self.stored_trust_state.accepts_out_of_workspace_files()
    }

    fn set_accepts_out_of_workspace_files(&self, value: bool) {
        self.stored_trust_state.set_accepts_out_of_workspace_files(value);
    }

    fn is_workspace_trusted(&self) -> bool {
        self.is_trusted.get()
    }

    fn is_workspace_trust_forced(&self) -> bool {
        // Remote - remote authority explicitly sets workspace trust
        if self.environment_service.remote_authority().is_some()
            && self.remote_authority.borrow().as_ref().and_then(|remote| remote.options.as_ref()).and_then(|options| options.is_trusted).is_some()
        {
            return true;
        }

        // All workspace uris are trusted automatically
        let workspace_uris: Vec<URI> = self.get_workspace_uris().into_iter().filter(|uri| !self.is_trusted_virtual_resource(uri) && !self.is_trusted_by_remote(uri)).collect();
        workspace_uris.is_empty()
    }

    fn can_set_parent_folder_trust(&self) -> bool {
        let canonical_workspace = self.canonical_workspace.borrow().clone();
        let AnyWorkspaceIdentifier::SingleFolder(workspace_identifier) = to_workspace_identifier(canonical_workspace.as_ref()) else {
            return false;
        };

        if workspace_identifier.uri.scheme() != schemas::FILE && workspace_identifier.uri.scheme() != schemas::VSCODE_REMOTE {
            return false;
        }

        let ext_uri = self.uri_identity_service.ext_uri();
        let parent_folder = ext_uri.dirname(&workspace_identifier.uri);
        !ext_uri.is_equal(Some(&workspace_identifier.uri), Some(&parent_folder), false)
    }

    fn set_parent_folder_trust(&self, trusted: bool) -> Promise<()> {
        if self.can_set_parent_folder_trust() {
            let canonical_workspace = self.canonical_workspace.borrow().clone();
            if let AnyWorkspaceIdentifier::SingleFolder(workspace_identifier) = to_workspace_identifier(canonical_workspace.as_ref()) {
                let parent_folder = self.uri_identity_service.ext_uri().dirname(&workspace_identifier.uri);
                return self.set_uris_trust(&[parent_folder], trusted);
            }
        }
        Promise::resolved(())
    }

    fn can_set_workspace_trust(&self) -> bool {
        // Remote - remote authority not yet resolved, or remote authority explicitly sets workspace trust
        if self.environment_service.remote_authority().is_some() {
            let remote_authority = self.remote_authority.borrow();
            let is_trusted = remote_authority.as_ref().and_then(|remote| remote.options.as_ref()).and_then(|options| options.is_trusted);
            if remote_authority.is_none() || is_trusted.is_some() {
                return false;
            }
        }

        // Empty workspace
        if self.is_empty_workspace() {
            return true;
        }

        // All workspace uris are trusted automatically
        let workspace_uris: Vec<URI> = self.get_workspace_uris().into_iter().filter(|uri| !self.is_trusted_virtual_resource(uri) && !self.is_trusted_by_remote(uri)).collect();
        if workspace_uris.is_empty() {
            return false;
        }

        // Untrusted workspace
        if !self.is_workspace_trusted() {
            return true;
        }

        // Trusted workspaces
        // Can only untrusted in the single folder scenario
        let canonical_workspace = self.canonical_workspace.borrow().clone();
        let AnyWorkspaceIdentifier::SingleFolder(workspace_identifier) = to_workspace_identifier(canonical_workspace.as_ref()) else {
            return false;
        };

        // Can only be untrusted in certain schemes
        if workspace_identifier.uri.scheme() != schemas::FILE && workspace_identifier.uri.scheme() != "vscode-vfs" {
            return false;
        }

        let ext_uri = self.uri_identity_service.ext_uri();

        // If the current folder isn't trusted directly, return false
        let trust_info = self.do_get_uri_trust_info(&workspace_identifier.uri);
        if !trust_info.trusted || !ext_uri.is_equal(Some(&workspace_identifier.uri), Some(&trust_info.uri), false) {
            return false;
        }

        // Check if the parent is also trusted
        if self.can_set_parent_folder_trust() {
            let parent_folder = ext_uri.dirname(&workspace_identifier.uri);
            let parent_path_trust_info = self.do_get_uri_trust_info(&parent_folder);
            if parent_path_trust_info.trusted {
                return false;
            }
        }

        true
    }

    fn set_workspace_trust(&self, trusted: bool) -> Promise<()> {
        // Empty workspace
        if self.is_empty_workspace() {
            return self.update_workspace_trust(Some(trusted));
        }

        let workspace_folders = self.get_workspace_uris();
        self.set_uris_trust(&workspace_folders, trusted)
    }

    fn get_uri_trust_info(&self, uri: &URI) -> Promise<WorkspaceTrustUriInfo> {
        // Return trusted when workspace trust is disabled
        if !self.workspace_trust_enablement_service.is_workspace_trust_enabled() {
            return Promise::resolved(WorkspaceTrustUriInfo { trusted: true, uri: uri.clone() });
        }

        // Uri is trusted automatically by the remote
        if self.is_trusted_by_remote(uri) {
            return Promise::resolved(WorkspaceTrustUriInfo { trusted: true, uri: uri.clone() });
        }

        let this = self.rc();
        let uri = uri.clone();
        async_fn(async move {
            let canonical_uri = this.get_canonical_uri_inner(&uri).await?;
            Ok(this.do_get_uri_trust_info(&canonical_uri))
        })
    }

    fn set_uris_trust(&self, uris: &[URI], trusted: bool) -> Promise<()> {
        let this = self.rc();
        let uris = uris.to_vec();
        async_fn(async move {
            let canonical_uris = Promise::all(uris.iter().map(|uri| this.get_canonical_uri_inner(uri)).collect()).await?;
            this.do_set_uris_trust(canonical_uris, trusted).await
        })
    }

    fn register_trusted_authority(&self, scheme: &str, authority: &str) -> DisposableRef {
        let entry = Rc::new(TrustedAuthority { scheme: scheme.to_owned(), authority: authority.to_owned() });
        self.trusted_authorities.borrow_mut().push(entry.clone());
        let this = self.this.clone();
        let update_trust = move || {
            if let Some(this) = this.upgrade() {
                this.on_did_change_trusted_folders.fire(());
                let promise = this.update_workspace_trust(None);
                drop(promise.catch(|error| on_unexpected_error(&error)));
            }
        };
        update_trust();
        let trusted_authorities = self.trusted_authorities.clone();
        to_disposable(move || {
            let mut authorities = trusted_authorities.borrow_mut();
            let previous_length = authorities.len();
            authorities.retain(|candidate| !Rc::ptr_eq(candidate, &entry));
            let deleted = authorities.len() != previous_length;
            drop(authorities);
            if deleted {
                update_trust();
            }
        })
    }

    fn get_trusted_uris(&self) -> Vec<URI> {
        self.trust_state_info.borrow().uri_trust_info.iter().map(|info| info.uri.clone()).collect()
    }

    fn set_trusted_uris(&self, uris: &[URI]) -> Promise<()> {
        let this = self.rc();
        let uris = uris.to_vec();
        async_fn(async move {
            this.trust_state_info.borrow_mut().uri_trust_info = Vec::new();
            let ext_uri = this.uri_identity_service.ext_uri();
            for uri in uris {
                let canonical_uri = this.get_canonical_uri_inner(&uri).await?;
                let clean_uri = ext_uri.remove_trailing_path_separator(&canonical_uri, None);
                let added = this.trust_state_info.borrow().uri_trust_info.iter().any(|added_uri| ext_uri.is_equal(Some(&added_uri.uri), Some(&clean_uri), false));

                if added {
                    continue;
                }

                this.trust_state_info.borrow_mut().uri_trust_info.push(WorkspaceTrustUriInfo { trusted: true, uri: clean_uri });
            }

            this.save_trust_info().await
        })
    }

    fn add_workspace_trust_transition_participant(&self, participant: Rc<dyn IWorkspaceTrustTransitionParticipant>) -> DisposableRef {
        self.trust_transition_manager.add_workspace_trust_transition_participant(participant)
    }
}

/// `new URI` a partir do objeto serializado (`URI.revive`, sem validação).
fn revive_uri(value: &ObjectRef) -> URI {
    let part = |name: &str| match value.get(&JsString::from(name)) {
        Some(Dynamic::String(text)) => Some(text),
        _ => None,
    };
    URI::revive(
        &UriState::from(UriComponents { scheme: part("scheme"), authority: part("authority"), path: part("path"), query: part("query"), fragment: part("fragment") }),
        host_platform(),
    )
}

/// `JSON.stringify(trustInfo)` (veja a nota do módulo sobre o que o URI escreve).
fn trust_info_to_json(info: &WorkspaceTrustInfo) -> String {
    let items = info
        .uri_trust_info
        .iter()
        .map(|entry| {
            let mut uri = Object::new();
            uri.insert(JsString::from("$mid"), Value::Number(1.0));
            for (name, part) in [("path", entry.uri.path()), ("scheme", entry.uri.scheme()), ("authority", entry.uri.authority()), ("query", entry.uri.query()), ("fragment", entry.uri.fragment())] {
                if !part.is_empty() {
                    uri.insert(JsString::from(name), Value::String(part.clone()));
                }
            }
            let mut item = Object::new();
            item.insert(JsString::from("uri"), Value::Object(uri));
            item.insert(JsString::from("trusted"), Value::Bool(entry.trusted));
            Value::Object(item)
        })
        .collect();
    let mut root = Object::new();
    root.insert(JsString::from("uriTrustInfo"), Value::Array(items));
    json_stringify(&Value::Object(root), None)
}

/// `WorkspaceTrustRequestService`.
pub struct WorkspaceTrustRequestService {
    base: Rc<Disposable>,

    open_files_trust_request_promise: RefCell<Option<Promise<WorkspaceTrustUriResponse>>>,
    open_files_trust_request_resolver: RefCell<Option<Resolver<WorkspaceTrustUriResponse>>>,

    resources_trust_request_promises: RefCell<ResourceMap<Promise<Option<bool>>>>,
    resources_trust_request_resolvers: RefCell<ResourceMap<Resolver<Option<bool>>>>,

    workspace_trust_request_promise: RefCell<Option<Promise<Option<bool>>>>,
    workspace_trust_request_resolver: RefCell<Option<Resolver<Option<bool>>>>,

    on_did_initiate_open_files_trust_request: Rc<Emitter<()>>,
    on_did_initiate_resources_trust_request: Rc<Emitter<ResourceTrustRequestOptions>>,
    on_did_initiate_workspace_trust_request: Rc<Emitter<Option<WorkspaceTrustRequestOptions>>>,
    on_did_initiate_workspace_trust_request_on_startup: Rc<Emitter<()>>,

    this: Weak<Self>,
    configuration_service: Rc<dyn IConfigurationService>,
    workspace_trust_management_service: Rc<dyn IWorkspaceTrustManagementService>,
}

impl WorkspaceTrustRequestService {
    pub fn new(configuration_service: Rc<dyn IConfigurationService>, workspace_trust_management_service: Rc<dyn IWorkspaceTrustManagementService>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_initiate_open_files_trust_request = Emitter::new();
        base.register(on_did_initiate_open_files_trust_request.clone());
        let on_did_initiate_resources_trust_request = Emitter::new();
        base.register(on_did_initiate_resources_trust_request.clone());
        let on_did_initiate_workspace_trust_request = Emitter::new();
        base.register(on_did_initiate_workspace_trust_request.clone());
        let on_did_initiate_workspace_trust_request_on_startup = Emitter::new();
        base.register(on_did_initiate_workspace_trust_request_on_startup.clone());
        Rc::new_cyclic(|this| Self {
            base,
            open_files_trust_request_promise: RefCell::new(None),
            open_files_trust_request_resolver: RefCell::new(None),
            resources_trust_request_promises: RefCell::new(ResourceMap::new()),
            resources_trust_request_resolvers: RefCell::new(ResourceMap::new()),
            workspace_trust_request_promise: RefCell::new(None),
            workspace_trust_request_resolver: RefCell::new(None),
            on_did_initiate_open_files_trust_request,
            on_did_initiate_resources_trust_request,
            on_did_initiate_workspace_trust_request,
            on_did_initiate_workspace_trust_request_on_startup,
            this: this.clone(),
            configuration_service,
            workspace_trust_management_service,
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    //#region Open file(s) trust request

    /// `get untrustedFilesSetting()`: `'prompt' | 'open' | 'newWindow'`.
    fn untrusted_files_setting(&self) -> Option<String> {
        match self.configuration_service.get_value(Some(WORKSPACE_TRUST_UNTRUSTED_FILES), None) {
            Dynamic::String(text) => Some(text.to_string_lossy()),
            _ => None,
        }
    }

    /// `set untrustedFilesSetting(value)`.
    fn set_untrusted_files_setting(&self, value: &str) {
        drop(self.configuration_service.update_value(WORKSPACE_TRUST_UNTRUSTED_FILES, Dynamic::string(value), None, None));
    }

    //#endregion

    //#region Workspace trust request

    fn resolve_workspace_trust_request(&self, trusted: Option<bool>) {
        let resolver = self.workspace_trust_request_resolver.borrow_mut().take();
        if let Some(resolver) = resolver {
            resolver.resolve(Some(trusted.unwrap_or_else(|| self.workspace_trust_management_service.is_workspace_trusted())));

            *self.workspace_trust_request_promise.borrow_mut() = None;
        }
    }

    //#endregion
}

impl IDisposable for WorkspaceTrustRequestService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkspaceTrustRequestService for WorkspaceTrustRequestService {
    fn on_did_initiate_open_files_trust_request(&self) -> Event<()> {
        self.on_did_initiate_open_files_trust_request.event()
    }

    fn on_did_initiate_workspace_trust_request(&self) -> Event<Option<WorkspaceTrustRequestOptions>> {
        self.on_did_initiate_workspace_trust_request.event()
    }

    fn on_did_initiate_workspace_trust_request_on_startup(&self) -> Event<()> {
        self.on_did_initiate_workspace_trust_request_on_startup.event()
    }

    fn on_did_initiate_resources_trust_request(&self) -> Event<ResourceTrustRequestOptions> {
        self.on_did_initiate_resources_trust_request.event()
    }

    fn complete_open_files_trust_request(&self, result: WorkspaceTrustUriResponse, save_response: Option<bool>) -> Promise<()> {
        let resolver = self.open_files_trust_request_resolver.borrow_mut().take();
        let Some(resolver) = resolver else {
            return Promise::resolved(());
        };

        // Set acceptsOutOfWorkspaceFiles
        if result == WorkspaceTrustUriResponse::Open {
            self.workspace_trust_management_service.set_accepts_out_of_workspace_files(true);
        }

        // Save response
        if save_response == Some(true) {
            if result == WorkspaceTrustUriResponse::Open {
                self.set_untrusted_files_setting("open");
            }

            if result == WorkspaceTrustUriResponse::OpenInNewWindow {
                self.set_untrusted_files_setting("newWindow");
            }
        }

        // Resolve promise
        resolver.resolve(result);

        *self.open_files_trust_request_promise.borrow_mut() = None;
        Promise::resolved(())
    }

    fn request_open_files_trust(&self, uris: &[URI]) -> Promise<WorkspaceTrustUriResponse> {
        let this = self.rc();
        let uris = uris.to_vec();
        async_fn(async move {
            // If workspace is untrusted, there is no conflict
            if !this.workspace_trust_management_service.is_workspace_trusted() {
                return Ok(WorkspaceTrustUriResponse::Open);
            }

            let open_files_trust_info = Promise::all(uris.iter().map(|uri| this.workspace_trust_management_service.get_uri_trust_info(uri)).collect()).await?;

            // If all uris are trusted, there is no conflict
            if open_files_trust_info.iter().all(|info| info.trusted) {
                return Ok(WorkspaceTrustUriResponse::Open);
            }

            // If user has setting, don't need to ask
            let setting = this.untrusted_files_setting();
            if setting.as_deref() != Some("prompt") {
                if setting.as_deref() == Some("newWindow") {
                    return Ok(WorkspaceTrustUriResponse::OpenInNewWindow);
                }

                if setting.as_deref() == Some("open") {
                    return Ok(WorkspaceTrustUriResponse::Open);
                }
            }

            // If we already asked the user, don't need to ask again
            if this.workspace_trust_management_service.accepts_out_of_workspace_files() {
                return Ok(WorkspaceTrustUriResponse::Open);
            }

            // Create/return a promise
            let existing = this.open_files_trust_request_promise.borrow().clone();
            if let Some(existing) = existing {
                return existing.await;
            }
            let (promise, resolver) = Promise::new();
            *this.open_files_trust_request_resolver.borrow_mut() = Some(resolver);
            *this.open_files_trust_request_promise.borrow_mut() = Some(promise.clone());

            this.on_did_initiate_open_files_trust_request.fire(());
            promise.await
        })
    }

    fn complete_resources_trust_request(&self, uri: &URI, result: WorkspaceTrustUriResponse) -> Promise<()> {
        let this = self.rc();
        let uri = uri.clone();
        async_fn(async move {
            let resolver = this.resources_trust_request_resolvers.borrow().get(&uri).cloned();
            let Some(resolver) = resolver else {
                return Ok(());
            };

            let trusted = result == WorkspaceTrustUriResponse::Open;
            this.workspace_trust_management_service.set_uris_trust(std::slice::from_ref(&uri), trusted).await?;

            resolver.resolve(Some(trusted));

            this.resources_trust_request_resolvers.borrow_mut().delete(&uri);
            this.resources_trust_request_promises.borrow_mut().delete(&uri);
            Ok(())
        })
    }

    fn request_resources_trust(&self, options: ResourceTrustRequestOptions) -> Promise<Option<bool>> {
        let this = self.rc();
        async_fn(async move {
            // Check if all resources are already trusted
            let resources_trust_info = this.workspace_trust_management_service.get_uri_trust_info(&options.uri).await?;
            if resources_trust_info.trusted {
                return Ok(Some(true));
            }

            // Return existing promise for this URI
            let existing_promise = this.resources_trust_request_promises.borrow().get(&options.uri).cloned();
            if let Some(existing_promise) = existing_promise {
                return existing_promise.await;
            }

            // Create a new promise for this URI
            let (promise, resolver) = Promise::new();
            this.resources_trust_request_resolvers.borrow_mut().set(options.uri.clone(), resolver);
            this.resources_trust_request_promises.borrow_mut().set(options.uri.clone(), promise.clone());
            this.on_did_initiate_resources_trust_request.fire(options);

            promise.await
        })
    }

    fn cancel_workspace_trust_request(&self) {
        let resolver = self.workspace_trust_request_resolver.borrow_mut().take();
        if let Some(resolver) = resolver {
            resolver.resolve(None);

            *self.workspace_trust_request_promise.borrow_mut() = None;
        }
    }

    fn complete_workspace_trust_request(&self, trusted: Option<bool>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            if trusted.is_none() || trusted == Some(this.workspace_trust_management_service.is_workspace_trusted()) {
                this.resolve_workspace_trust_request(trusted);
                return Ok(());
            }

            // Register one-time event handler to resolve the promise when workspace trust changed
            let weak = this.this.clone();
            this.workspace_trust_management_service.on_did_change_trust().once().listen(move |trusted| {
                if let Some(this) = weak.upgrade() {
                    this.resolve_workspace_trust_request(Some(*trusted));
                }
            });

            // Update storage, transition workspace state
            this.workspace_trust_management_service.set_workspace_trust(trusted.unwrap_or_default()).await
        })
    }

    fn request_workspace_trust(&self, options: Option<WorkspaceTrustRequestOptions>) -> Promise<Option<bool>> {
        let this = self.rc();
        async_fn(async move {
            // Trusted workspace
            if this.workspace_trust_management_service.is_workspace_trusted() {
                return Ok(Some(this.workspace_trust_management_service.is_workspace_trusted()));
            }

            // Modal request
            let existing = this.workspace_trust_request_promise.borrow().clone();
            if let Some(existing) = existing {
                // Return existing promise
                return existing.await;
            }
            // Create promise
            let (promise, resolver) = Promise::new();
            *this.workspace_trust_request_resolver.borrow_mut() = Some(resolver);
            *this.workspace_trust_request_promise.borrow_mut() = Some(promise.clone());

            this.on_did_initiate_workspace_trust_request.fire(options);
            promise.await
        })
    }

    fn request_workspace_trust_on_startup(&self) {
        if self.workspace_trust_request_promise.borrow().is_none() {
            // Create promise
            let (promise, resolver) = Promise::new();
            *self.workspace_trust_request_resolver.borrow_mut() = Some(resolver);
            *self.workspace_trust_request_promise.borrow_mut() = Some(promise);
        }

        self.on_did_initiate_workspace_trust_request_on_startup.fire(());
    }
}

/// `WorkspaceTrustTransitionManager`.
struct WorkspaceTrustTransitionManager {
    base: Rc<Disposable>,
    participants: LinkedList<Rc<dyn IWorkspaceTrustTransitionParticipant>>,
}

impl WorkspaceTrustTransitionManager {
    fn new() -> Rc<Self> {
        Rc::new(Self { base: Disposable::new(), participants: LinkedList::new() })
    }

    fn add_workspace_trust_transition_participant(&self, participant: Rc<dyn IWorkspaceTrustTransitionParticipant>) -> DisposableRef {
        let remove = self.participants.push(participant);
        to_disposable(move || remove.remove())
    }

    fn participate(&self, trusted: bool) -> Promise<()> {
        let mut participants = self.participants.iter();
        async_fn(async move {
            for participant in &mut participants {
                participant.participate(trusted).await?;
            }
            Ok(())
        })
    }
}

impl IDisposable for WorkspaceTrustTransitionManager {
    fn dispose(&self) {
        self.participants.clear();
        self.base.dispose();
    }
}

/// `WorkspaceTrustMemento`.
struct WorkspaceTrustMemento {
    memento: Option<Memento>,
    memento_object: ObjectRef,
}

impl WorkspaceTrustMemento {
    const ACCEPTS_OUT_OF_WORKSPACE_FILES_KEY: &'static str = "acceptsOutOfWorkspaceFiles";
    const IS_EMPTY_WORKSPACE_TRUSTED_KEY: &'static str = "isEmptyWorkspaceTrusted";

    fn new(storage_service: Option<Rc<dyn StorageService>>) -> Self {
        match storage_service {
            Some(storage_service) => {
                let memento = Memento::new("workspaceTrust", storage_service);
                let memento_object = memento.get_memento(StorageScope::Workspace, StorageTarget::Machine);
                Self { memento: Some(memento), memento_object }
            }
            None => Self { memento: None, memento_object: ObjectRef::new() },
        }
    }

    fn accepts_out_of_workspace_files(&self) -> bool {
        match self.memento_object.get(&JsString::from(Self::ACCEPTS_OUT_OF_WORKSPACE_FILES_KEY)) {
            Some(Dynamic::Bool(value)) => value,
            _ => false,
        }
    }

    fn set_accepts_out_of_workspace_files(&self, value: bool) {
        self.memento_object.set(&JsString::from(Self::ACCEPTS_OUT_OF_WORKSPACE_FILES_KEY), Dynamic::Bool(value));

        if let Some(memento) = &self.memento {
            memento.save_memento();
        }
    }

    fn is_empty_workspace_trusted(&self) -> Option<bool> {
        match self.memento_object.get(&JsString::from(Self::IS_EMPTY_WORKSPACE_TRUSTED_KEY)) {
            Some(Dynamic::Bool(value)) => Some(value),
            _ => None,
        }
    }

    fn set_is_empty_workspace_trusted(&self, value: Option<bool>) {
        self.memento_object.set(&JsString::from(Self::IS_EMPTY_WORKSPACE_TRUSTED_KEY), value.map_or(Dynamic::Undefined, Dynamic::Bool));

        if let Some(memento) = &self.memento {
            memento.save_memento();
        }
    }
}
