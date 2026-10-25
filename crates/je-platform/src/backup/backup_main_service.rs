//! Porte de `src/vs/platform/backup/electron-main/backupMainService.ts` e `backup.ts`: o lado "main" do backup, que
//! registra e limpa os backups de workspaces, pastas e janelas vazias (o hot exit) e guarda o índice deles no estado
//! (`backupWorkspaces`).
//!
//! Não há processo principal do Electron no je-code, mas o papel dele existe no processo único; por isso o serviço é o
//! mesmo, sem janelas nem IPC.
//!
//! Decisões de tradução:
//! - **`IBackupMainService`** é o trait [`IBackupMainService`]; a sobrecarga `registerWorkspaceBackup(info, migrateFrom)`
//!   (que devolve `Promise<string>`) é [`IBackupMainService::register_workspace_backup_migrating`].
//! - **`@IEnvironmentMainService`**: o serviço só lê `backupHome`, que entra como argumento do construtor
//!   ([`BackupMainService::new`]); [`backup_home_of`] calcula o valor do `EnvironmentMainService.backupHome`
//!   (`join(userDataPath, 'Backups')`) a partir do ambiente nativo. No original o teste sobrescreve o campo protegido
//!   `backupHome`; aqui ele escolhe o valor no construtor.
//! - **Membros `protected`** (`getWorkspaceBackups`, `getFolderBackups`, `getFolderHash`) são `pub`: o teste os chama
//!   pela subclasse `TestBackupMainService`, que em Rust não existe.
//! - **`async`**: `initialize` é uma `async fn`; os métodos do trait que o original declara como `Promise` devolvem
//!   [`Promise`] (um trait não tem `async fn` compatível com objeto). O `Promises.readdir` e o `Promises.exists` do
//!   [`pfs`] são síncronos (ver a divergência de execução lá), então `doHasBackups` é uma função comum.
//! - **O estado** (`ISerializedBackupWorkspaces`) chega como [`Dynamic`], porque o JSON do disco pode ter qualquer forma:
//!   a leitura repete a tolerância do original (campo ausente ou que não é array vale lista vazia, um elemento `null`
//!   derruba a leitura dos workspaces ou pastas, um valor que não é string vira o texto do JavaScript, como o
//!   `URI.parse(undefined)`).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::extpath;
use je_base::network::schemas;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_instantiation::service_id;

use super::{BackupInfo, EmptyWindowBackupInfo, FolderBackupInfo, WorkspaceBackupInfo};
use crate::configuration::service::IConfigurationService;
use crate::environment::environment::NativeEnvironmentServiceApi;
use crate::files::files::HotExitConfiguration;
use crate::files::pfs::{self, RenameRetry, RimRafMode};
use crate::js::to_js_string;
use crate::log::{LogServiceRef, Logger};
use crate::resources::{ext_uri_biased_ignore_path_case, fs_path};
use crate::state::{StateReadService, StateService};
use crate::workspace::WorkspaceIdentifier;
use crate::workspaces::node::md5::md5_hex;
use crate::workspaces::node::workspaces::create_empty_workspace_identifier;

/// `IBackupMainService`.
pub trait IBackupMainService {
    fn is_hot_exit_enabled(&self) -> bool;

    fn get_empty_window_backups(&self) -> Vec<EmptyWindowBackupInfo>;

    /// `registerWorkspaceBackup(workspaceInfo)`: devolve o caminho do backup.
    fn register_workspace_backup(&self, workspace_info: WorkspaceBackupInfo) -> String;

    /// `registerWorkspaceBackup(workspaceInfo, migrateFrom)`: move os dados de `migrate_from` para o backup do workspace.
    fn register_workspace_backup_migrating(&self, workspace_info: WorkspaceBackupInfo, migrate_from: &str) -> Promise<String>;

    fn register_folder_backup(&self, folder_info: FolderBackupInfo) -> String;

    fn register_empty_window_backup(&self, empty_window_info: EmptyWindowBackupInfo) -> String;

    /// All folders or workspaces that are known to have backups stored. This call is long running because it checks for
    /// each backup location if any backups are stored.
    fn get_dirty_workspaces(&self) -> Promise<Vec<BackupInfo>>;
}

service_id!(pub I_BACKUP_MAIN_SERVICE: dyn IBackupMainService = "backupMainService");

/// `EnvironmentMainService.backupHome`: `join(this.userDataPath, 'Backups')`.
pub fn backup_home_of(environment: &dyn NativeEnvironmentServiceApi) -> String {
    join(&environment.user_data_path(), "Backups")
}

/// `join(a, b)` do módulo `path` da plataforma.
fn join(first: &str, second: &str) -> String {
    PathModule::native(&ProcessEnvironment).join(&[first, second])
}

fn is_linux() -> bool {
    cfg!(target_os = "linux")
}

/// `value[key]` com a regra do JavaScript: `undefined` e `null` lançam `TypeError`, o resto sem a propriedade vale `undefined`.
fn read_property(value: &Dynamic, key: &str) -> Result<Dynamic, Error> {
    match value {
        Dynamic::Undefined | Dynamic::Null => Err(Error::new(format!(
            "Cannot read properties of {} (reading '{key}')",
            if value.is_null() { "null" } else { "undefined" }
        ))),
        Dynamic::Object(_) => Ok(value.get_str(key)),
        _ => Ok(Dynamic::Undefined),
    }
}

/// `value?.[key]`.
fn property(value: &Dynamic, key: &str) -> Dynamic {
    read_property(value, key).unwrap_or(Dynamic::Undefined)
}

/// O `remoteAuthority` de um item serializado, quando é uma string.
fn remote_authority_of(item: &Dynamic) -> Option<String> {
    match property(item, "remoteAuthority") {
        Dynamic::String(authority) => Some(authority.to_string_lossy()),
        _ => None,
    }
}

/// O item serializado de um workspace antes da validação: o `id` pode não ser string (então o formato é inválido).
struct SerializedWorkspace {
    id: Option<String>,
    config_path: URI,
    remote_authority: Option<String>,
}

/// `deserializeWorkspaceInfos`: qualquer falha de leitura (o `catch` do original) deixa a lista vazia.
fn deserialize_workspace_infos(serialized: &Dynamic) -> Vec<SerializedWorkspace> {
    let read = || -> Result<Vec<SerializedWorkspace>, Error> {
        let Dynamic::Array(items) = property(serialized, "workspaces") else {
            return Ok(Vec::new());
        };
        items
            .to_vec()
            .iter()
            .map(|workspace| -> Result<SerializedWorkspace, Error> {
                let id = read_property(workspace, "id")?;
                let config_uri_path = read_property(workspace, "configURIPath")?;
                Ok(SerializedWorkspace {
                    id: match id {
                        Dynamic::String(id) => Some(id.to_string_lossy()),
                        _ => None,
                    },
                    config_path: URI::parse(to_js_string(&config_uri_path), false),
                    remote_authority: remote_authority_of(workspace),
                })
            })
            .collect()
    };
    read().unwrap_or_default()
}

/// `deserializeFolderInfos`.
fn deserialize_folder_infos(serialized: &Dynamic) -> Vec<FolderBackupInfo> {
    let read = || -> Result<Vec<FolderBackupInfo>, Error> {
        let Dynamic::Array(items) = property(serialized, "folders") else {
            return Ok(Vec::new());
        };
        items
            .to_vec()
            .iter()
            .map(|folder| -> Result<FolderBackupInfo, Error> {
                let folder_uri = read_property(folder, "folderUri")?;
                Ok(FolderBackupInfo { folder_uri: URI::parse(to_js_string(&folder_uri), false), remote_authority: remote_authority_of(folder) })
            })
            .collect()
    };
    read().unwrap_or_default()
}

/// Um item de `ISerializedBackupWorkspaces`: os campos mais o `remoteAuthority`, quando é verdadeiro (`if (remoteAuthority)`).
fn serialized_item(mut entries: Vec<(&'static str, Dynamic)>, remote_authority: &Option<String>) -> Dynamic {
    if let Some(remote_authority) = remote_authority.as_deref().filter(|authority| !authority.is_empty()) {
        entries.push(("remoteAuthority", Dynamic::string(remote_authority)));
    }
    Dynamic::object(entries)
}

/// `BackupMainService`.
pub struct BackupMainService {
    this: Weak<BackupMainService>,
    backup_home: String,
    configuration_service: Rc<dyn IConfigurationService>,
    log_service: LogServiceRef,
    state_service: Rc<dyn StateService>,

    workspaces: RefCell<Vec<WorkspaceBackupInfo>>,
    folders: RefCell<Vec<FolderBackupInfo>>,
    empty_windows: RefCell<Vec<EmptyWindowBackupInfo>>,

    // Comparers for paths and resources that will
    // - ignore path casing on Windows/macOS
    // - respect path casing on Linux
    backup_uri_comparer: ExtUri,
}

impl BackupMainService {
    const BACKUP_WORKSPACES_METADATA_STORAGE_KEY: &'static str = "backupWorkspaces";

    pub fn new(
        backup_home: String,
        configuration_service: Rc<dyn IConfigurationService>,
        log_service: LogServiceRef,
        state_service: Rc<dyn StateService>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            backup_home,
            configuration_service,
            log_service,
            state_service,
            workspaces: RefCell::new(Vec::new()),
            folders: RefCell::new(Vec::new()),
            empty_windows: RefCell::new(Vec::new()),
            backup_uri_comparer: ext_uri_biased_ignore_path_case(),
        })
    }

    /// `backupHome`.
    pub fn backup_home(&self) -> &str {
        &self.backup_home
    }

    /// `this`, para as promessas que sobrevivem à chamada.
    fn shared(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto um método dele roda")
    }

    /// `backupPathComparer.isEqual(pathA, pathB)`: ignora a caixa fora do Linux.
    fn is_equal_backup_path(path_a: &str, path_b: &str) -> bool {
        extpath::is_equal(path_a, path_b, !is_linux())
    }

    pub async fn initialize(&self) -> Result<(), Error> {
        // read backup workspaces
        let stored = self.state_service.get_item(Self::BACKUP_WORKSPACES_METADATA_STORAGE_KEY, Dynamic::Undefined);
        let serialized_backup_workspaces = if stored.is_nullish() {
            Dynamic::object([
                ("workspaces", Dynamic::array([])),
                ("folders", Dynamic::array([])),
                ("emptyWindows", Dynamic::array([])),
            ])
        } else {
            stored
        };

        // validate empty workspaces backups first
        let empty_windows = self.validate_empty_workspaces(&property(&serialized_backup_workspaces, "emptyWindows")).await?;
        *self.empty_windows.borrow_mut() = empty_windows;

        // validate workspace backups
        let workspaces = self.validate_workspaces(deserialize_workspace_infos(&serialized_backup_workspaces)).await;
        *self.workspaces.borrow_mut() = workspaces;

        // validate folder backups
        let folders = self.validate_folders(deserialize_folder_infos(&serialized_backup_workspaces)).await;
        *self.folders.borrow_mut() = folders;

        // store metadata in case some workspaces or folders have been removed
        self.store_workspaces_metadata();
        Ok(())
    }

    pub fn get_workspace_backups(&self) -> Vec<WorkspaceBackupInfo> {
        if self.is_hot_exit_on_exit_and_window_close() {
            // Only non-folder windows are restored on main process launch when
            // hot exit is configured as onExitAndWindowClose.
            return Vec::new();
        }

        self.workspaces.borrow().clone() // return a copy
    }

    pub fn get_folder_backups(&self) -> Vec<FolderBackupInfo> {
        if self.is_hot_exit_on_exit_and_window_close() {
            // Only non-folder windows are restored on main process launch when
            // hot exit is configured as onExitAndWindowClose.
            return Vec::new();
        }

        self.folders.borrow().clone() // return a copy
    }

    fn is_hot_exit_on_exit_and_window_close(&self) -> bool {
        self.get_hot_exit_config() == HotExitConfiguration::ON_EXIT_AND_WINDOW_CLOSE
    }

    /// `config?.files?.hotExit || HotExitConfiguration.ON_EXIT`.
    fn get_hot_exit_config(&self) -> String {
        let config = self.configuration_service.get_value(None, None);
        let hot_exit = property(&property(&config, "files"), "hotExit");

        if hot_exit.is_truthy() { to_js_string(&hot_exit).to_string_lossy() } else { HotExitConfiguration::ON_EXIT.to_string() }
    }

    async fn move_backup_folder(&self, backup_path: &str, move_from_path: &str) {
        // Target exists: make sure to convert existing backups to empty window backups
        if pfs::exists(backup_path) {
            self.convert_to_empty_window_backup(backup_path).await;
        }

        // When we have data to migrate from, move it over to the target location
        if pfs::exists(move_from_path)
            && let Err(error) = pfs::rename(move_from_path, backup_path, RenameRetry::Disabled /* no retry */).await
        {
            self.log_service.error(&format!("Backup: Could not move backup folder to new location: {error}"));
        }
    }

    async fn validate_workspaces(&self, root_workspaces: Vec<SerializedWorkspace>) -> Vec<WorkspaceBackupInfo> {
        let mut seen_ids: HashSet<String> = HashSet::new();
        let mut result: Vec<WorkspaceBackupInfo> = Vec::new();

        // Validate Workspaces
        for workspace_info in root_workspaces {
            let Some(id) = workspace_info.id else {
                return Vec::new(); // wrong format, skip all entries
            };
            let workspace = WorkspaceIdentifier { id, config_path: workspace_info.config_path };

            if seen_ids.insert(workspace.id.clone()) {
                let backup_path = join(&self.backup_home, &workspace.id);
                let has_backups = self.do_has_backups(&backup_path);

                // If the workspace has no backups, ignore it
                if has_backups {
                    if workspace.config_path.scheme() != schemas::FILE || pfs::exists(&fs_path(&workspace.config_path)) {
                        result.push(WorkspaceBackupInfo { workspace, remote_authority: workspace_info.remote_authority });
                    } else {
                        // If the workspace has backups, but the target workspace is missing, convert backups to empty ones
                        self.convert_to_empty_window_backup(&backup_path).await;
                    }
                } else {
                    self.delete_stale_backup(&backup_path).await;
                }
            }
        }

        result
    }

    async fn validate_folders(&self, folder_workspaces: Vec<FolderBackupInfo>) -> Vec<FolderBackupInfo> {
        let mut result: Vec<FolderBackupInfo> = Vec::new();
        let mut seen_ids: HashSet<Vec<u16>> = HashSet::new();
        for folder_info in folder_workspaces {
            let folder_uri = folder_info.folder_uri.clone();
            let key = self.backup_uri_comparer.get_comparison_key(&folder_uri, false);
            if seen_ids.insert(key.units().to_vec()) {
                let backup_path = join(&self.backup_home, &Self::get_folder_hash(&folder_info));
                let has_backups = self.do_has_backups(&backup_path);

                // If the folder has no backups, ignore it
                if has_backups {
                    if folder_uri.scheme() != schemas::FILE || pfs::exists(&fs_path(&folder_uri)) {
                        result.push(folder_info);
                    } else {
                        // If the folder has backups, but the target workspace is missing, convert backups to empty ones
                        self.convert_to_empty_window_backup(&backup_path).await;
                    }
                } else {
                    self.delete_stale_backup(&backup_path).await;
                }
            }
        }

        result
    }

    async fn validate_empty_workspaces(&self, empty_workspaces: &Dynamic) -> Result<Vec<EmptyWindowBackupInfo>, Error> {
        let Dynamic::Array(empty_workspaces) = empty_workspaces else {
            return Ok(Vec::new());
        };

        let mut result: Vec<EmptyWindowBackupInfo> = Vec::new();
        let mut seen_ids: HashSet<String> = HashSet::new();

        // Validate Empty Windows
        for backup_info in empty_workspaces.to_vec() {
            let Dynamic::String(backup_folder) = read_property(&backup_info, "backupFolder")? else {
                return Ok(Vec::new());
            };
            let backup_folder = backup_folder.to_string_lossy();

            if seen_ids.insert(backup_folder.clone()) {
                let backup_path = join(&self.backup_home, &backup_folder);
                if self.do_has_backups(&backup_path) {
                    result.push(EmptyWindowBackupInfo { backup_folder, remote_authority: remote_authority_of(&backup_info) });
                } else {
                    self.delete_stale_backup(&backup_path).await;
                }
            }
        }

        Ok(result)
    }

    async fn delete_stale_backup(&self, backup_path: &str) {
        if let Err(error) = pfs::rm(backup_path, RimRafMode::Move, None).await {
            self.log_service.error(&format!("Backup: Could not delete stale backup: {error}"));
        }
    }

    fn prepare_new_empty_window_backup(&self) -> EmptyWindowBackupInfo {
        // We are asked to prepare a new empty window backup folder.
        // Empty windows backup folders are derived from a workspace
        // identifier, so we generate a new empty workspace identifier
        // until we found a unique one.

        let mut empty_workspace_identifier = create_empty_workspace_identifier();
        while self
            .empty_windows
            .borrow()
            .iter()
            .any(|empty_window| !empty_window.backup_folder.is_empty() && Self::is_equal_backup_path(&empty_window.backup_folder, &empty_workspace_identifier.id))
        {
            empty_workspace_identifier = create_empty_workspace_identifier();
        }

        EmptyWindowBackupInfo { backup_folder: empty_workspace_identifier.id, remote_authority: None }
    }

    async fn convert_to_empty_window_backup(&self, backup_path: &str) -> bool {
        let new_empty_window_backup_info = self.prepare_new_empty_window_backup();

        // Rename backupPath to new empty window backup path
        let new_empty_window_backup_path = join(&self.backup_home, &new_empty_window_backup_info.backup_folder);
        if let Err(error) = pfs::rename(backup_path, &new_empty_window_backup_path, RenameRetry::Disabled /* no retry */).await {
            self.log_service.error(&format!("Backup: Could not rename backup folder: {error}"));
            return false;
        }
        self.empty_windows.borrow_mut().push(new_empty_window_backup_info);

        true
    }

    async fn collect_dirty_workspaces(&self) -> Vec<BackupInfo> {
        let mut dirty_workspaces: Vec<BackupInfo> = Vec::new();

        // Workspaces with backups
        let workspaces = self.workspaces.borrow().clone();
        for workspace in workspaces {
            let workspace = BackupInfo::Workspace(workspace);
            if self.has_backups(&workspace) {
                dirty_workspaces.push(workspace);
            }
        }

        // Folders with backups
        let folders = self.folders.borrow().clone();
        for folder in folders {
            let folder = BackupInfo::Folder(folder);
            if self.has_backups(&folder) {
                dirty_workspaces.push(folder);
            }
        }

        dirty_workspaces
    }

    /// `hasBackups`: o ramo de janela vazia do original nunca é chamado (só `getDirtyWorkspaces` a usa), então aqui o
    /// argumento é o workspace ou a pasta.
    fn has_backups(&self, backup_location: &BackupInfo) -> bool {
        let backup_path = match backup_location {
            // Folder
            BackupInfo::Folder(folder) => join(&self.backup_home, &Self::get_folder_hash(folder)),
            // Workspace
            BackupInfo::Workspace(workspace) => join(&self.backup_home, &workspace.workspace.id),
        };

        self.do_has_backups(&backup_path)
    }

    fn do_has_backups(&self, backup_path: &str) -> bool {
        // backup path does not exist: `readdir` falha
        let Ok(backup_schemas) = pfs::readdir_names(backup_path) else {
            return false;
        };

        for backup_schema in backup_schemas {
            // invalid folder: `readdir` falha
            if let Ok(backup_schema_children) = pfs::readdir_names(&join(backup_path, &backup_schema))
                && !backup_schema_children.is_empty()
            {
                return true;
            }
        }

        false
    }

    fn store_workspaces_metadata(&self) {
        let workspaces: Vec<Dynamic> = self
            .workspaces
            .borrow()
            .iter()
            .map(|WorkspaceBackupInfo { workspace, remote_authority }| {
                serialized_item(
                    vec![("id", Dynamic::string(&workspace.id)), ("configURIPath", Dynamic::String(workspace.config_path.to_string(false)))],
                    remote_authority,
                )
            })
            .collect();
        let folders: Vec<Dynamic> = self
            .folders
            .borrow()
            .iter()
            .map(|FolderBackupInfo { folder_uri, remote_authority }| {
                serialized_item(vec![("folderUri", Dynamic::String(folder_uri.to_string(false)))], remote_authority)
            })
            .collect();
        let empty_windows: Vec<Dynamic> = self
            .empty_windows
            .borrow()
            .iter()
            .map(|EmptyWindowBackupInfo { backup_folder, remote_authority }| serialized_item(vec![("backupFolder", Dynamic::string(backup_folder))], remote_authority))
            .collect();

        self.state_service.set_item(
            Self::BACKUP_WORKSPACES_METADATA_STORAGE_KEY,
            Dynamic::object([
                ("workspaces", Dynamic::array(workspaces)),
                ("folders", Dynamic::array(folders)),
                ("emptyWindows", Dynamic::array(empty_windows)),
            ]),
        );
    }

    pub fn get_folder_hash(folder: &FolderBackupInfo) -> String {
        let folder_uri = &folder.folder_uri;

        let key = if folder_uri.scheme() == schemas::FILE {
            let folder_path = fs_path(folder_uri);
            if is_linux() { folder_path } else { folder_path.to_lowercase() } // for backward compatibility, use the fspath as key
        } else {
            folder_uri.to_string(false).to_string_lossy().to_lowercase()
        };

        md5_hex(key.as_bytes()) // Using MD5 to convert a file path to a fixed length
    }
}

impl IBackupMainService for BackupMainService {
    fn is_hot_exit_enabled(&self) -> bool {
        self.get_hot_exit_config() != HotExitConfiguration::OFF
    }

    fn get_empty_window_backups(&self) -> Vec<EmptyWindowBackupInfo> {
        self.empty_windows.borrow().clone() // return a copy
    }

    fn register_workspace_backup(&self, workspace_info: WorkspaceBackupInfo) -> String {
        let is_known = self.workspaces.borrow().iter().any(|workspace| workspace_info.workspace.id == workspace.workspace.id);
        let backup_path = join(&self.backup_home, &workspace_info.workspace.id);
        if !is_known {
            self.workspaces.borrow_mut().push(workspace_info);
            self.store_workspaces_metadata();
        }

        backup_path
    }

    fn register_workspace_backup_migrating(&self, workspace_info: WorkspaceBackupInfo, migrate_from: &str) -> Promise<String> {
        let backup_path = self.register_workspace_backup(workspace_info);

        if migrate_from.is_empty() {
            return Promise::resolved(backup_path);
        }

        let this = self.shared();
        let migrate_from = migrate_from.to_string();
        async_fn(async move {
            this.move_backup_folder(&backup_path, &migrate_from).await;
            Ok(backup_path)
        })
    }

    fn register_folder_backup(&self, folder_info: FolderBackupInfo) -> String {
        let is_known = self.folders.borrow().iter().any(|folder| self.backup_uri_comparer.is_equal(Some(&folder_info.folder_uri), Some(&folder.folder_uri), false));
        let backup_path = join(&self.backup_home, &Self::get_folder_hash(&folder_info));
        if !is_known {
            self.folders.borrow_mut().push(folder_info);
            self.store_workspaces_metadata();
        }

        backup_path
    }

    fn register_empty_window_backup(&self, empty_window_info: EmptyWindowBackupInfo) -> String {
        let is_known = self
            .empty_windows
            .borrow()
            .iter()
            .any(|empty_window| !empty_window.backup_folder.is_empty() && Self::is_equal_backup_path(&empty_window.backup_folder, &empty_window_info.backup_folder));
        let backup_path = join(&self.backup_home, &empty_window_info.backup_folder);
        if !is_known {
            self.empty_windows.borrow_mut().push(empty_window_info);
            self.store_workspaces_metadata();
        }

        backup_path
    }

    fn get_dirty_workspaces(&self) -> Promise<Vec<BackupInfo>> {
        let this = self.shared();
        async_fn(async move { Ok(this.collect_dirty_workspaces().await) })
    }
}
