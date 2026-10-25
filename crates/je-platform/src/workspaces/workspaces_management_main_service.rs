//! Porte de `src/vs/platform/workspaces/electron-main/workspacesManagementMainService.ts`: o lado "main" dos workspaces, que
//! cria, resolve e apaga os workspaces sem título (`untitledWorkspacesHome`), guarda a lista deles, e troca a janela para
//! um workspace salvo (`enterWorkspace`), migrando o backup e apagando o workspace sem título de onde ela saiu.
//!
//! Não há processo principal do Electron no je-code, mas o papel dele existe no processo único; por isso o serviço é o
//! mesmo, sem IPC.
//!
//! Decisões de tradução:
//! - **`IWorkspacesManagementMainService`** é o trait [`IWorkspacesManagementMainService`]; as sobrecargas do original que
//!   devolvem `Promise` devolvem [`Promise`] (um trait não tem `async fn` compatível com objeto).
//! - **`@IEnvironmentMainService`**: o serviço só lê `untitledWorkspacesHome` e `workspaceStorageHome`, que entram como
//!   argumentos do construtor ([`WorkspacesManagementMainService::new`]); quem monta o serviço passa os valores do
//!   `EnvironmentService`. No original o teste sobrescreve o getter `untitledWorkspacesHome`; aqui ele escolhe o valor no
//!   construtor.
//! - **`@IUserDataProfilesMainService`**: o serviço só chama `unsetWorkspace`, que é o do [`UserDataProfilesService`] (o
//!   `UserDataProfilesMainService` do original é o mesmo serviço do `common`).
//! - **`ICodeWindow`** não tem porte (o `window.ts` de `electron-main` depende de `BrowserWindow`); [`CodeWindow`] é o
//!   contrato com os membros que os consumidores portados leem (este serviço, o `windowsFinder.ts` e o
//!   `windowsStateHandler.ts`), e [`NativeWindowConfiguration`] o `INativeWindowConfiguration` com os campos que eles
//!   leem ou gravam. `window.win` (o `BrowserWindow`, que some quando a janela é destruída) é
//!   [`CodeWindow::has_native_window`], e `window.win?.isSimpleFullScreen()` é [`CodeWindow::is_simple_full_screen`].
//! - **`findWindowOnWorkspaceOrFolder`** é a de [`crate::windows::windows_finder`] (o porte de `windowsFinder.ts`).
//! - **`doResolveLocalWorkspace`** tem duas sobrecargas no original (síncrona e assíncrona); só a assíncrona é chamada
//!   (`resolveLocalWorkspace`), e a leitura de arquivo daqui é síncrona (ver a divergência de execução em [`pfs`]), então o
//!   corpo é [`WorkspacesManagementMainService::do_resolve_local_workspace`], uma função comum.
//! - **O diálogo do `showMessageBox`** perde o argumento de janela (ver [`crate::dialogs::dialog_main_service`]).
//! - **O erro de workspace inválido** vem de [`do_parse_stored_workspace`] (a cópia do `common`, que no original difere
//!   só em `path.toString(true)` na mensagem: o parâmetro `skip_encoding_in_message` escolhe).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::json_formatter;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::resources::{ExtUri, original_fs_path};
use je_base::uri::{Field, URI, UriChange};
use je_instantiation::service_id;

use crate::backup::WorkspaceBackupInfo;
use crate::backup::backup_main_service::IBackupMainService;
use crate::dialogs::dialog_main_service::{IDialogMainService, MessageBoxOptions, MessageBoxType};
use crate::files::pfs::{self, FsError, RimRafMode};
use crate::log::{LogServiceRef, Logger};
use crate::resources::{ext_uri_biased_ignore_path_case, fs_path, join_path};
use crate::user_data_profile::UserDataProfilesService;
use crate::window::electron_main::{Rectangle, WindowUiState};
use crate::windows::windows_finder::find_window_on_workspace_or_folder;
use crate::workspace::{
    AnyWorkspaceIdentifier, ResolvedWorkspace, UNTITLED_WORKSPACE_NAME, WorkspaceIdentifier, has_workspace_file_extension_uri, is_untitled_workspace,
};
use crate::workspaces::node::workspaces::{create_empty_workspace_identifier, get_workspace_identifier};
use crate::workspaces::workspaces::{
    EnterWorkspaceResult, StoredWorkspace, UntitledWorkspaceInfo, WorkspaceFolderCreationData, do_parse_stored_workspace, get_stored_workspace_folder,
    stored_workspace_to_value, to_workspace_folders,
};

/// O que o serviço lê e grava de `INativeWindowConfiguration`.
#[derive(Clone, Debug, Default)]
pub struct NativeWindowConfiguration {
    pub extension_development_path: Option<Vec<String>>,
    pub extension_tests_path: Option<String>,
    pub backup_path: Option<String>,
    pub workspace: Option<AnyWorkspaceIdentifier>,
}

/// `ICodeWindow`, com os membros que os consumidores portados leem (ver [`crate::user_data_profile::user_data_profiles_handler::ProfilesWindow`]
/// para o resto do contrato).
pub trait CodeWindow {
    /// `window.win`: o `BrowserWindow` existe (a janela não foi destruída).
    fn has_native_window(&self) -> bool;

    /// `window.win?.isSimpleFullScreen()`: `false` quando o `BrowserWindow` não existe.
    fn is_simple_full_screen(&self) -> bool;

    /// `window.isReady`.
    fn is_ready(&self) -> bool;

    /// `window.id`.
    fn id(&self) -> i32;

    /// `window.backupPath`.
    fn backup_path(&self) -> Option<String>;

    /// `window.isExtensionDevelopmentHost`.
    fn is_extension_development_host(&self) -> bool;

    /// `window.isExtensionTestHost`.
    fn is_extension_test_host(&self) -> bool;

    /// `window.getBounds()`.
    fn get_bounds(&self) -> Rectangle;

    /// `window.serializeWindowState()`.
    fn serialize_window_state(&self) -> WindowUiState;

    /// `window.openedWorkspace`.
    fn opened_workspace(&self) -> Option<AnyWorkspaceIdentifier>;

    /// `window.config`: a configuração viva da janela (o serviço a atualiza ao entrar no workspace).
    fn config(&self) -> Option<Rc<RefCell<NativeWindowConfiguration>>>;

    /// `window.remoteAuthority`.
    fn remote_authority(&self) -> Option<String>;

    /// `window.focus()`.
    fn focus(&self);
}

/// `IWorkspaceEnteredEvent`.
#[derive(Clone)]
pub struct WorkspaceEnteredEvent {
    pub window: Rc<dyn CodeWindow>,
    pub workspace: WorkspaceIdentifier,
}

/// `IWorkspacesManagementMainService`.
pub trait IWorkspacesManagementMainService {
    fn on_did_delete_untitled_workspace(&self) -> Event<WorkspaceIdentifier>;
    fn on_did_enter_workspace(&self) -> Event<WorkspaceEnteredEvent>;

    fn enter_workspace(&self, into_window: Rc<dyn CodeWindow>, opened_windows: Vec<Rc<dyn CodeWindow>>, path: &URI) -> Promise<Option<EnterWorkspaceResult>>;

    fn create_untitled_workspace(&self, folders: Vec<WorkspaceFolderCreationData>, remote_authority: Option<String>) -> Promise<WorkspaceIdentifier>;

    fn delete_untitled_workspace(&self, workspace: &WorkspaceIdentifier) -> Promise<()>;

    fn get_untitled_workspaces(&self) -> Vec<UntitledWorkspaceInfo>;
    fn is_untitled_workspace(&self, workspace: &WorkspaceIdentifier) -> bool;

    fn resolve_local_workspace(&self, path: &URI) -> Promise<Option<ResolvedWorkspace>>;

    fn get_workspace_identifier(&self, workspace_path: &URI) -> Promise<WorkspaceIdentifier>;
}

service_id!(pub I_WORKSPACES_MANAGEMENT_MAIN_SERVICE: dyn IWorkspacesManagementMainService = "workspacesManagementMainService");

/// `join(a, b)` do módulo `path` da plataforma.
fn join(first: &str, second: &str) -> String {
    PathModule::native(&ProcessEnvironment).join(&[first, second])
}

/// `dirname(path)` do módulo `path` da plataforma.
fn dirname(path: &str) -> String {
    PathModule::native(&ProcessEnvironment).dirname(path)
}

/// `uri.with({ scheme: Schemas.file }).fsPath`.
fn file_scheme_fs_path(uri: &URI) -> String {
    fs_path(&uri.with(&UriChange { scheme: Field::from(schemas::FILE), ..Default::default() }))
}

/// `fs.promises.readFile(path, 'utf8')`: o texto UTF-8 do arquivo (bytes inválidos viram o caractere de substituição).
fn read_utf8(path: &str) -> Result<String, FsError> {
    let file = pfs::open_for_read(path)?;
    Ok(String::from_utf8_lossy(&pfs::read_to_end(&file)?).into_owned())
}

/// `window.openedWorkspace`, quando é um `IWorkspaceIdentifier` (`isWorkspaceIdentifier`).
fn opened_workspace_identifier(window: &Rc<dyn CodeWindow>) -> Option<WorkspaceIdentifier> {
    match window.opened_workspace() {
        Some(AnyWorkspaceIdentifier::Workspace(workspace)) => Some(workspace),
        _ => None,
    }
}

/// `WorkspacesManagementMainService`.
pub struct WorkspacesManagementMainService {
    this: Weak<WorkspacesManagementMainService>,
    base: Rc<Disposable>,
    on_did_delete_untitled_workspace: Rc<Emitter<WorkspaceIdentifier>>,
    on_did_enter_workspace: Rc<Emitter<WorkspaceEnteredEvent>>,

    untitled_workspaces_home: URI, // local URI that contains all untitled workspaces
    workspace_storage_home: URI,
    untitled_workspaces: RefCell<Vec<UntitledWorkspaceInfo>>,

    log_service: LogServiceRef,
    user_data_profiles_main_service: Rc<UserDataProfilesService>,
    backup_main_service: Rc<dyn IBackupMainService>,
    dialog_main_service: Rc<dyn IDialogMainService>,

    // Comparer for paths and resources that ignores path casing on Windows/macOS and respects it on Linux
    ext_uri: ExtUri,
}

impl WorkspacesManagementMainService {
    /// `untitled_workspaces_home` e `workspace_storage_home` são os do `EnvironmentService`.
    pub fn new(
        untitled_workspaces_home: URI,
        workspace_storage_home: URI,
        log_service: LogServiceRef,
        user_data_profiles_main_service: Rc<UserDataProfilesService>,
        backup_main_service: Rc<dyn IBackupMainService>,
        dialog_main_service: Rc<dyn IDialogMainService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_delete_untitled_workspace = Emitter::new();
        let on_did_enter_workspace = Emitter::new();
        base.register(on_did_delete_untitled_workspace.clone());
        base.register(on_did_enter_workspace.clone());
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            on_did_delete_untitled_workspace,
            on_did_enter_workspace,
            untitled_workspaces_home,
            workspace_storage_home,
            untitled_workspaces: RefCell::new(Vec::new()),
            log_service,
            user_data_profiles_main_service,
            backup_main_service,
            dialog_main_service,
            ext_uri: ext_uri_biased_ignore_path_case(),
        })
    }

    /// `this`, para as promessas que sobrevivem à chamada.
    fn shared(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto um método dele roda")
    }

    pub async fn initialize(&self) {
        // Reset
        self.untitled_workspaces.borrow_mut().clear();

        // Resolve untitled workspaces
        match pfs::readdir_names(&file_scheme_fs_path(&self.untitled_workspaces_home)) {
            Ok(folders) => {
                let untitled_workspace_paths: Vec<URI> =
                    folders.iter().map(|folder| join_path(&self.untitled_workspaces_home, &[folder.as_str(), UNTITLED_WORKSPACE_NAME])).collect();
                for untitled_workspace_path in untitled_workspace_paths {
                    let workspace = get_workspace_identifier(&untitled_workspace_path, &Platform::detect());
                    match self.do_resolve_local_workspace(&untitled_workspace_path) {
                        None => self.do_delete_untitled_workspace_and_notify(&workspace).await,
                        Some(resolved_workspace) => {
                            self.untitled_workspaces.borrow_mut().push(UntitledWorkspaceInfo { workspace, remote_authority: resolved_workspace.remote_authority })
                        }
                    }
                }
            }
            Err(error) => {
                if error.code != Some("ENOENT") {
                    self.log_service.warn(&format!("Unable to read folders in {} ({error}).", self.untitled_workspaces_home.to_string(false).to_string_lossy()));
                }
            }
        }
    }

    fn do_resolve_local_workspace(&self, uri: &URI) -> Option<ResolvedWorkspace> {
        if !self.is_workspace_path(uri) {
            return None; // does not look like a valid workspace config file
        }

        if uri.scheme() != schemas::FILE {
            return None;
        }

        let contents = read_utf8(&fs_path(uri)).ok()?; // invalid workspace
        self.do_resolve_workspace(uri, &contents)
    }

    fn is_workspace_path(&self, uri: &URI) -> bool {
        is_untitled_workspace(uri, &self.untitled_workspaces_home) || has_workspace_file_extension_uri(uri)
    }

    fn do_resolve_workspace(&self, path: &URI, contents: &str) -> Option<ResolvedWorkspace> {
        match do_parse_stored_workspace(path, contents, true) {
            Ok(workspace) => {
                let workspace_identifier = get_workspace_identifier(path, &Platform::detect());
                Some(ResolvedWorkspace {
                    folders: to_workspace_folders(&workspace.folders, &workspace_identifier.config_path, &self.ext_uri),
                    identifier: workspace_identifier,
                    remote_authority: workspace.remote_authority,
                    transient: workspace.transient,
                })
            }
            Err(error) => {
                self.log_service.warn(&error.to_string());
                None
            }
        }
    }

    fn new_untitled_workspace(&self, folders: &[WorkspaceFolderCreationData], remote_authority: Option<String>) -> (WorkspaceIdentifier, StoredWorkspace) {
        let random_id = create_empty_workspace_identifier().id;
        let untitled_workspace_config_folder = join_path(&self.untitled_workspaces_home, &[random_id.as_str()]);
        let untitled_workspace_config_path = join_path(&untitled_workspace_config_folder, &[UNTITLED_WORKSPACE_NAME]);

        let stored_workspace_folder = folders
            .iter()
            .map(|folder| get_stored_workspace_folder(&folder.uri, true, folder.name.as_deref(), &untitled_workspace_config_folder, &self.ext_uri))
            .collect();

        (
            get_workspace_identifier(&untitled_workspace_config_path, &Platform::detect()),
            StoredWorkspace { folders: stored_workspace_folder, remote_authority, transient: None },
        )
    }

    /// `deleteUntitledWorkspace`: apaga do disco, solta o workspace dos perfis e avisa.
    async fn do_delete_untitled_workspace_and_notify(&self, workspace: &WorkspaceIdentifier) {
        if !self.is_untitled_workspace(workspace) {
            return; // only supported for untitled workspaces
        }

        // Delete from disk
        self.do_delete_untitled_workspace(workspace).await;

        // unset workspace from profiles
        self.user_data_profiles_main_service.unset_workspace(&AnyWorkspaceIdentifier::Workspace(workspace.clone()), false);

        // Event
        self.on_did_delete_untitled_workspace.fire(workspace.clone());
    }

    async fn do_delete_untitled_workspace(&self, workspace: &WorkspaceIdentifier) {
        let config_path = original_fs_path(&workspace.config_path, Platform::detect().path_flavor()).to_string_lossy();
        let result: Result<(), Error> = async {
            // Delete Workspace
            pfs::rm(&dirname(&config_path), RimRafMode::Unlink, None).await?;

            // Mark Workspace Storage to be deleted
            let workspace_storage_path = join(&file_scheme_fs_path(&self.workspace_storage_home), &workspace.id);
            if pfs::exists(&workspace_storage_path) {
                pfs::write_file(&join(&workspace_storage_path, "obsolete"), "", None).await?;
            }

            // Remove from list
            self.untitled_workspaces.borrow_mut().retain(|untitled_workspace| untitled_workspace.workspace.id != workspace.id);
            Ok(())
        }
        .await;

        if let Err(error) = result {
            self.log_service.warn(&format!("Unable to delete untitled workspace {config_path} ({error})."));
        }
    }

    async fn is_valid_target_workspace_path(&self, window: &Rc<dyn CodeWindow>, windows: &[Rc<dyn CodeWindow>], workspace_path: Option<&URI>) -> Result<bool, Error> {
        let Some(workspace_path) = workspace_path else {
            return Ok(true);
        };

        if let Some(opened_workspace) = opened_workspace_identifier(window)
            && self.ext_uri.is_equal(Some(&opened_workspace.config_path), Some(workspace_path), false)
        {
            return Ok(false); // window is already opened on a workspace with that path
        }

        // Prevent overwriting a workspace that is currently opened in another window
        if find_window_on_workspace_or_folder(windows, workspace_path, &self.ext_uri).is_some() {
            let workspace_name = self.ext_uri.basename(workspace_path, None);
            self.dialog_main_service
                .show_message_box(MessageBoxOptions {
                    message_box_type: Some(MessageBoxType::Info),
                    buttons: vec![localize("ok", "&&OK", &[])],
                    message: localize("workspaceOpenedMessage", "Unable to save workspace '{0}'", &[workspace_name.as_str()]),
                    detail: Some(localize("workspaceOpenedDetail", "The workspace is already opened in another window. Please close that window first and then try again.", &[])),
                })
                .await?;

            return Ok(false);
        }

        Ok(true) // OK
    }

    async fn do_enter_workspace(&self, window: &Rc<dyn CodeWindow>, workspace: WorkspaceIdentifier) -> Result<Option<EnterWorkspaceResult>, Error> {
        let Some(config) = window.config() else {
            return Ok(None);
        };

        window.focus();

        // Register window for backups and migrate current backups over
        let (extension_development_path, current_backup_path) = {
            let config = config.borrow();
            (config.extension_development_path.clone(), config.backup_path.clone())
        };
        let mut backup_path: Option<String> = None;
        if extension_development_path.is_none() {
            let workspace_info = WorkspaceBackupInfo { workspace: workspace.clone(), remote_authority: window.remote_authority() };
            backup_path = Some(match current_backup_path.filter(|path| !path.is_empty()) {
                Some(migrate_from) => self.backup_main_service.register_workspace_backup_migrating(workspace_info, &migrate_from).await?,
                None => self.backup_main_service.register_workspace_backup(workspace_info),
            });
        }

        // if the window was opened on an untitled workspace, delete it.
        if let Some(opened_workspace) = opened_workspace_identifier(window)
            && self.is_untitled_workspace(&opened_workspace)
        {
            self.do_delete_untitled_workspace_and_notify(&opened_workspace).await;
        }

        // Update window configuration properly based on transition to workspace
        {
            let mut config = config.borrow_mut();
            config.workspace = Some(AnyWorkspaceIdentifier::Workspace(workspace.clone()));
            config.backup_path = backup_path.clone();
        }

        Ok(Some(EnterWorkspaceResult { workspace, backup_path }))
    }
}

impl IDisposable for WorkspacesManagementMainService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkspacesManagementMainService for WorkspacesManagementMainService {
    fn on_did_delete_untitled_workspace(&self) -> Event<WorkspaceIdentifier> {
        self.on_did_delete_untitled_workspace.event()
    }

    fn on_did_enter_workspace(&self) -> Event<WorkspaceEnteredEvent> {
        self.on_did_enter_workspace.event()
    }

    fn enter_workspace(&self, window: Rc<dyn CodeWindow>, windows: Vec<Rc<dyn CodeWindow>>, path: &URI) -> Promise<Option<EnterWorkspaceResult>> {
        let this = self.shared();
        let path = path.clone();
        async_fn(async move {
            if !window.has_native_window() || !window.is_ready() {
                return Ok(None); // return early if the window is not ready or disposed
            }

            let is_valid = this.is_valid_target_workspace_path(&window, &windows, Some(&path)).await?;
            if !is_valid {
                return Ok(None); // return early if the workspace is not valid
            }

            let Some(result) = this.do_enter_workspace(&window, get_workspace_identifier(&path, &Platform::detect())).await? else {
                return Ok(None);
            };

            // Emit as event
            this.on_did_enter_workspace.fire(WorkspaceEnteredEvent { window, workspace: result.workspace.clone() });

            Ok(Some(result))
        })
    }

    fn create_untitled_workspace(&self, folders: Vec<WorkspaceFolderCreationData>, remote_authority: Option<String>) -> Promise<WorkspaceIdentifier> {
        let this = self.shared();
        async_fn(async move {
            let (workspace, stored_workspace) = this.new_untitled_workspace(&folders, remote_authority.clone());
            let config_path = fs_path(&workspace.config_path);

            pfs::mkdir_recursive(&dirname(&config_path), None)?;
            pfs::write_file(&config_path, json_formatter::json_stringify(&stored_workspace_to_value(&stored_workspace), Some("\t")), None).await?;

            this.untitled_workspaces.borrow_mut().push(UntitledWorkspaceInfo { workspace: workspace.clone(), remote_authority });

            Ok(workspace)
        })
    }

    fn delete_untitled_workspace(&self, workspace: &WorkspaceIdentifier) -> Promise<()> {
        let this = self.shared();
        let workspace = workspace.clone();
        async_fn(async move {
            this.do_delete_untitled_workspace_and_notify(&workspace).await;
            Ok(())
        })
    }

    fn get_untitled_workspaces(&self) -> Vec<UntitledWorkspaceInfo> {
        self.untitled_workspaces.borrow().clone()
    }

    fn is_untitled_workspace(&self, workspace: &WorkspaceIdentifier) -> bool {
        is_untitled_workspace(&workspace.config_path, &self.untitled_workspaces_home)
    }

    fn resolve_local_workspace(&self, uri: &URI) -> Promise<Option<ResolvedWorkspace>> {
        Promise::resolved(self.do_resolve_local_workspace(uri))
    }

    fn get_workspace_identifier(&self, config_path: &URI) -> Promise<WorkspaceIdentifier> {
        Promise::resolved(get_workspace_identifier(config_path, &Platform::detect()))
    }
}
