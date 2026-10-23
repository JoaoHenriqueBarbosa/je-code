//! Porte de `src/vs/platform/workspaces/common/workspaces.ts`: o contrato do `IWorkspacesService`, os recentes, as
//! utilidades do arquivo `.code-workspace` (pastas armazenadas, reescrita para outro local) e a serialização dos
//! recentes.
//!
//! Representação: `IStoredWorkspaceFolder` é o [`RawWorkspaceFolder`] (caminho `path` ou `uri`); os recentes
//! guardados (`RecentlyOpenedStorageData`, `object`) são [`Dynamic`].

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::extpath;
use je_base::json::{self, Value};
use je_base::json_edit::{self, EditSegment};
use je_base::json_formatter::FormattingOptions;
use je_base::labels::normalize_drive_letter;
use je_base::network::schemas;
use je_base::path::PathModule;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_instantiation::service_id;

use crate::backup::BackupInfo;
use crate::log::LogServiceRef;
use crate::resources::host_environment;
use crate::workspace::{RawWorkspaceFolder, WorkspaceFolder, WorkspaceIdentifier};

/// `IRecentWorkspace`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentWorkspace {
    pub workspace: WorkspaceIdentifier,
    pub label: Option<String>,
    pub remote_authority: Option<String>,
}

/// `IRecentFolder`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentFolder {
    pub folder_uri: URI,
    pub label: Option<String>,
    pub remote_authority: Option<String>,
}

/// `IRecentFile`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentFile {
    pub file_uri: URI,
    pub label: Option<String>,
    pub remote_authority: Option<String>,
}

/// `IRecentWorkspace | IRecentFolder`: o que `IRecentlyOpened.workspaces` guarda.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecentWorkspaceOrFolder {
    Workspace(RecentWorkspace),
    Folder(RecentFolder),
}

/// `IRecentlyOpened`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecentlyOpened {
    pub workspaces: Vec<RecentWorkspaceOrFolder>,
    pub files: Vec<RecentFile>,
}

/// `IRecent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recent {
    Workspace(RecentWorkspace),
    Folder(RecentFolder),
    File(RecentFile),
}

/// `isRecentWorkspace`.
pub fn is_recent_workspace(curr: &Recent) -> bool {
    matches!(curr, Recent::Workspace(_))
}

/// `isRecentFolder`.
pub fn is_recent_folder(curr: &Recent) -> bool {
    matches!(curr, Recent::Folder(_))
}

/// `isRecentFile`.
pub fn is_recent_file(curr: &Recent) -> bool {
    matches!(curr, Recent::File(_))
}

/// `IWorkspaceFolderCreationData`.
#[derive(Clone, Debug)]
pub struct WorkspaceFolderCreationData {
    pub uri: URI,
    pub name: Option<String>,
}

/// `IUntitledWorkspaceInfo`.
#[derive(Clone, Debug)]
pub struct UntitledWorkspaceInfo {
    pub workspace: WorkspaceIdentifier,
    pub remote_authority: Option<String>,
}

/// `IEnterWorkspaceResult`.
#[derive(Clone, Debug)]
pub struct EnterWorkspaceResult {
    pub workspace: WorkspaceIdentifier,
    pub backup_path: Option<String>,
}

/// `IStoredWorkspace`.
#[derive(Clone, Debug)]
pub struct StoredWorkspace {
    pub remote_authority: Option<String>,
    pub transient: Option<bool>,
    pub folders: Vec<RawWorkspaceFolder>,
}

/// `IWorkspacesService`.
pub trait IWorkspacesService {
    // Workspaces Management
    fn enter_workspace(&self, workspace_uri: &URI) -> Promise<Option<EnterWorkspaceResultCell>>;
    fn create_untitled_workspace(&self, folders: Vec<WorkspaceFolderCreationData>, remote_authority: Option<String>) -> Promise<WorkspaceIdentifierCell>;
    fn delete_untitled_workspace(&self, workspace: &WorkspaceIdentifier) -> Promise<()>;
    fn get_workspace_identifier(&self, workspace_uri: &URI) -> Promise<WorkspaceIdentifierCell>;

    // Workspaces History
    fn on_did_change_recently_opened(&self) -> Event<()>;
    fn add_recently_opened(&self, recents: Vec<Recent>) -> Promise<()>;
    fn remove_recently_opened(&self, workspaces: Vec<URI>) -> Promise<()>;
    fn clear_recently_opened(&self) -> Promise<()>;
    fn get_recently_opened(&self) -> Promise<RecentlyOpenedCell>;

    // Dirty Workspaces
    fn get_dirty_workspaces(&self) -> Promise<Vec<BackupInfo>>;
}

/// As promessas de [`Promise`] exigem `Clone`: estes aliases nomeiam os valores já `Clone` das respostas.
pub type EnterWorkspaceResultCell = EnterWorkspaceResult;
pub type WorkspaceIdentifierCell = WorkspaceIdentifier;
pub type RecentlyOpenedCell = RecentlyOpened;

service_id!(pub I_WORKSPACES_SERVICE: dyn IWorkspacesService = "workspacesService");

fn is_windows() -> bool {
    Platform::detect().is_windows
}

// #region Workspace File Utilities

/// `isStoredWorkspaceFolder`: o `RawWorkspaceFolder` já é um dos dois formatos (o guarda existe para valores JSON).
pub fn is_stored_workspace_folder(value: &Value) -> bool {
    stored_workspace_folder_from_value(value).is_some()
}

fn name_of(value: &json::Object) -> Result<Option<String>, ()> {
    match value.get("name") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.to_string_lossy())),
        Some(Value::Bool(false)) => Ok(None),
        Some(Value::Number(n)) if *n == 0.0 => Ok(None),
        Some(_) => Err(()),
    }
}

/// `isRawFileWorkspaceFolder || isRawUriWorkspaceFolder`, devolvendo a pasta tipada.
pub fn stored_workspace_folder_from_value(value: &Value) -> Option<RawWorkspaceFolder> {
    let Value::Object(object) = value else { return None };
    if let Some(Value::String(path)) = object.get("path")
        && let Ok(name) = name_of(object)
    {
        return Some(RawWorkspaceFolder::Path { path: path.to_string_lossy(), name });
    }
    if let Some(Value::String(uri)) = object.get("uri")
        && let Ok(name) = name_of(object)
    {
        return Some(RawWorkspaceFolder::Uri { uri: uri.to_string_lossy(), name });
    }
    None
}

/// Dada a pasta e a pasta de configuração do workspace, calcula o `IStoredWorkspaceFolder` por caminho relativo,
/// absoluto ou URI.
///
/// - `force_absolute`: mantém o caminho absoluto.
/// - `folder_name`: o nome da pasta no workspace.
/// - `target_config_folder_uri`: a pasta onde o arquivo de workspace vive.
pub fn get_stored_workspace_folder(
    folder_uri: &URI,
    force_absolute: bool,
    folder_name: Option<&str>,
    target_config_folder_uri: &URI,
    ext_uri: &ExtUri,
) -> RawWorkspaceFolder {
    let name = folder_name.map(str::to_string);
    let full_uri = |uri: &URI| RawWorkspaceFolder::Uri { uri: uri.to_string(true).to_string_lossy(), name: name.clone() };

    // Scheme mismatch: use full absolute URI as `uri`
    if folder_uri.scheme() != target_config_folder_uri.scheme() {
        return full_uri(folder_uri);
    }

    // Always prefer a relative path if possible unless prevented to make the workspace file shareable with other users
    let mut folder_path: Option<String> = if force_absolute { None } else { ext_uri.relative_path(target_config_folder_uri, folder_uri) };

    if let Some(path) = folder_path.as_mut() {
        if path.is_empty() {
            *path = ".".to_string();
        } else if is_windows() {
            *path = massage_path_for_windows(path);
        }
    }
    // We could not resolve a relative path
    else {
        // Local file: use `fsPath`
        if folder_uri.scheme() == schemas::FILE {
            let mut path = folder_uri.fs_path(Platform::detect().path_flavor()).to_string_lossy();
            if is_windows() {
                path = massage_path_for_windows(&path);
            }
            folder_path = Some(path);
        }
        // Different authority: use full absolute URI
        else if !ext_uri.is_equal_authority(folder_uri.authority(), target_config_folder_uri.authority()) {
            return full_uri(folder_uri);
        }
        // Non-local file: use `path` of URI
        else {
            folder_path = Some(folder_uri.path().to_string_lossy());
        }
    }

    RawWorkspaceFolder::Path { path: folder_path.unwrap_or_default(), name }
}

fn massage_path_for_windows(folder_path: &str) -> String {
    // Drive letter should be upper case
    let folder_path = normalize_drive_letter(folder_path, true);

    // Always prefer slash over backslash unless we deal with UNC paths where backslash is mandatory.
    if !extpath::is_unc(&folder_path, host_environment().as_ref()) {
        return extpath::to_slashes(&folder_path);
    }
    folder_path
}

/// `toWorkspaceFolders`.
pub fn to_workspace_folders(configured_folders: &[RawWorkspaceFolder], workspace_config_file: &URI, ext_uri: &ExtUri) -> Vec<WorkspaceFolder> {
    let mut result: Vec<WorkspaceFolder> = Vec::new();
    let mut seen: std::collections::HashSet<JsString> = std::collections::HashSet::new();
    let relative_to = ext_uri.dirname(workspace_config_file);
    for configured_folder in configured_folders {
        let uri: Option<URI> = match configured_folder {
            RawWorkspaceFolder::Path { path, .. } => {
                if path.is_empty() {
                    None
                } else {
                    Some(ext_uri.resolve_path(&relative_to, path))
                }
            }
            RawWorkspaceFolder::Uri { uri, .. } => match errors::catch(|| URI::parse(uri.as_str(), false)) {
                Ok(parsed) => {
                    if parsed.path().units().first() != Some(&(b'/' as u16)) {
                        // this makes sure all workspace folder are absolute
                        let absolute = format!("/{}", parsed.path().to_string_lossy());
                        Some(parsed.with(&je_base::uri::UriChange { path: je_base::uri::Field::Set(JsString::from(absolute)), ..Default::default() }))
                    } else {
                        Some(parsed)
                    }
                }
                Err(error) => {
                    eprintln!("{error}"); // ignore
                    None
                }
            },
        };

        if let Some(uri) = uri {
            // remove duplicates
            let comparison_key = ext_uri.get_comparison_key(&uri, false);
            if seen.insert(comparison_key) {
                let configured_name = configured_folder.name().filter(|n| !n.is_empty());
                let name = configured_name.map_or_else(|| ext_uri.basename_or_authority(&uri), str::to_string);
                let index = result.len();
                result.push(WorkspaceFolder::new(uri, name, index, Some(configured_folder.clone())));
            }
        }
    }
    result
}

fn folder_to_value(folder: &RawWorkspaceFolder) -> Value {
    let mut object = json::Object::new();
    let (key, text, name) = match folder {
        RawWorkspaceFolder::Path { path, name } => ("path", path, name),
        RawWorkspaceFolder::Uri { uri, name } => ("uri", uri, name),
    };
    if let Some(name) = name {
        object.insert(JsString::from("name"), Value::String(JsString::from(name.as_str())));
    }
    object.insert(JsString::from(key), Value::String(JsString::from(text.as_str())));
    Value::Object(object)
}

/// Reescreve o conteúdo de um arquivo de workspace para ser salvo em outro local.
/// Dá erro se o arquivo não for um arquivo de workspace válido.
pub fn rewrite_workspace_file_for_new_location(
    raw_workspace_contents: &str,
    config_path_uri: &URI,
    is_from_untitled_workspace: bool,
    target_config_path_uri: &URI,
    ext_uri: &ExtUri,
) -> Result<String, Error> {
    let stored_workspace = do_parse_stored_workspace(config_path_uri, raw_workspace_contents)?;

    let source_config_folder = ext_uri.dirname(config_path_uri);
    let target_config_folder = ext_uri.dirname(target_config_path_uri);

    let host_env = host_environment();
    let path_module = PathModule::native(host_env.as_ref());
    let mut rewritten_folders: Vec<RawWorkspaceFolder> = Vec::new();
    for folder in &stored_workspace.folders {
        let folder_uri = match folder {
            RawWorkspaceFolder::Path { path, .. } => ext_uri.resolve_path(&source_config_folder, path),
            RawWorkspaceFolder::Uri { uri, .. } => URI::parse(uri.as_str(), false),
        };
        let absolute = if is_from_untitled_workspace {
            false // if it was an untitled workspace, try to make paths relative
        } else {
            // for existing workspaces, preserve whether a path was absolute or relative
            match folder {
                RawWorkspaceFolder::Path { path, .. } => path_module.is_absolute(path),
                RawWorkspaceFolder::Uri { .. } => true,
            }
        };
        rewritten_folders.push(get_stored_workspace_folder(&folder_uri, absolute, folder.name(), &target_config_folder, ext_uri));
    }

    // Preserve as much of the existing workspace as possible by using jsonEdit and only changing the folders portion.
    let platform = Platform::detect();
    let formatting_options = FormattingOptions {
        insert_spaces: Some(false),
        tab_size: Some(4),
        eol: Some(JsString::from(if platform.is_linux || platform.is_macintosh { "\n" } else { "\r\n" })),
    };
    let text: Vec<u16> = raw_workspace_contents.encode_utf16().collect();
    let edit_error = |e: json_edit::JsonEditError| Error::new(e.0);
    let folders_value = Value::Array(rewritten_folders.iter().map(folder_to_value).collect());
    let edits = json_edit::set_property(&text, &[EditSegment::from("folders")], Some(folders_value), &formatting_options, None).map_err(edit_error)?;
    let mut new_content = json_edit::apply_edits(&text, &edits).map_err(edit_error)?;

    let target_remote_authority = (target_config_path_uri.scheme() == schemas::VSCODE_REMOTE).then(|| target_config_path_uri.authority().to_string_lossy());
    let same_authority = match (&stored_workspace.remote_authority, &target_remote_authority) {
        (None, None) => true,
        (Some(a), Some(b)) => a.to_lowercase() == b.to_lowercase(),
        _ => false,
    };
    if same_authority {
        // unsaved remote workspaces have the remoteAuthority set. Remove it when no longer necessary.
        let edits = json_edit::remove_property(&new_content, &[EditSegment::from("remoteAuthority")], &formatting_options).map_err(edit_error)?;
        new_content = json_edit::apply_edits(&new_content, &edits).map_err(edit_error)?;
    }

    Ok(String::from_utf16_lossy(&new_content))
}

fn do_parse_stored_workspace(path: &URI, contents: &str) -> Result<StoredWorkspace, Error> {
    // Parse workspace file (fault tolerant parser)
    let mut errors_found = Vec::new();
    let parsed = json::parse_str(contents, &mut errors_found);

    // Filter out folders which do not have a path or uri set
    let Some(Value::Object(object)) = parsed else {
        return Err(Error::new(format!("{} looks like an invalid workspace file.", path.to_string(false).to_string_lossy())));
    };
    let Some(Value::Array(folders)) = object.get("folders") else {
        return Err(Error::new(format!("{} looks like an invalid workspace file.", path.to_string(false).to_string_lossy())));
    };
    let folders = folders.iter().filter_map(stored_workspace_folder_from_value).collect();
    let remote_authority = match object.get("remoteAuthority") {
        Some(Value::String(s)) => Some(s.to_string_lossy()),
        _ => None,
    };
    let transient = match object.get("transient") {
        Some(Value::Bool(b)) => Some(*b),
        _ => None,
    };
    Ok(StoredWorkspace { remote_authority, transient, folders })
}

// #endregion

// #region Workspace Storage

fn string_of(value: &Dynamic, key: &str) -> Option<String> {
    match value.get_str(key) {
        Dynamic::String(s) => Some(s.to_string_lossy()),
        _ => None,
    }
}

/// `restoreRecentlyOpened`: os recentes de `data` (o valor guardado), pulando as entradas inválidas.
pub fn restore_recently_opened(data: Option<&Dynamic>, log_service: &LogServiceRef) -> RecentlyOpened {
    let mut result = RecentlyOpened::default();
    let Some(data) = data.filter(|d| d.is_truthy_value()) else {
        return result;
    };
    let Dynamic::Array(entries) = data.get_str("entries") else {
        return result;
    };
    for entry in entries.to_vec() {
        let label = string_of(&entry, "label");
        let remote_authority = string_of(&entry, "remoteAuthority");
        let restored = errors::catch(|| {
            let workspace = entry.get_str("workspace");
            if matches!(workspace, Dynamic::Object(_)) && string_of(&workspace, "id").is_some() && string_of(&workspace, "configPath").is_some() {
                // isSerializedRecentWorkspace
                result.workspaces.push(RecentWorkspaceOrFolder::Workspace(RecentWorkspace {
                    label: label.clone(),
                    remote_authority: remote_authority.clone(),
                    workspace: WorkspaceIdentifier {
                        id: string_of(&workspace, "id").unwrap_or_default(),
                        config_path: URI::parse(string_of(&workspace, "configPath").unwrap_or_default().as_str(), false),
                    },
                }));
            } else if let Some(folder_uri) = string_of(&entry, "folderUri") {
                result.workspaces.push(RecentWorkspaceOrFolder::Folder(RecentFolder {
                    label: label.clone(),
                    remote_authority: remote_authority.clone(),
                    folder_uri: URI::parse(folder_uri.as_str(), false),
                }));
            } else if let Some(file_uri) = string_of(&entry, "fileUri") {
                result.files.push(RecentFile { label: label.clone(), remote_authority: remote_authority.clone(), file_uri: URI::parse(file_uri.as_str(), false) });
            }
        });
        if let Err(error) = restored {
            log_service.warn(&format!("Error restoring recent entry {:?}: {error}. Skip entry.", entry));
        }
    }
    result
}

/// `toStoreData`.
pub fn to_store_data(recents: &RecentlyOpened) -> Dynamic {
    let platform = Platform::detect().path_flavor();
    // Only store the label if it is provided and only if it differs from the path. This gives us a chance to render
    // the path better, e.g. use `~` for home.
    let store_label = |label: &Option<String>, uri: &URI| -> Dynamic {
        match label {
            Some(label) if !label.is_empty() && *label != uri.fs_path(platform).to_string_lossy() && *label != uri.path().to_string_lossy() => Dynamic::string(label),
            _ => Dynamic::Undefined,
        }
    };
    let optional = |value: &Option<String>| value.as_deref().map_or(Dynamic::Undefined, Dynamic::string);
    let mut entries: Vec<Dynamic> = Vec::new();
    for recent in &recents.workspaces {
        match recent {
            RecentWorkspaceOrFolder::Folder(folder) => entries.push(Dynamic::object([
                ("folderUri", Dynamic::String(folder.folder_uri.to_string(false))),
                ("label", store_label(&folder.label, &folder.folder_uri)),
                ("remoteAuthority", optional(&folder.remote_authority)),
            ])),
            RecentWorkspaceOrFolder::Workspace(workspace) => entries.push(Dynamic::object([
                (
                    "workspace",
                    Dynamic::object([("id", Dynamic::string(&workspace.workspace.id)), ("configPath", Dynamic::String(workspace.workspace.config_path.to_string(false)))]),
                ),
                ("label", store_label(&workspace.label, &workspace.workspace.config_path)),
                ("remoteAuthority", optional(&workspace.remote_authority)),
            ])),
        }
    }
    for recent in &recents.files {
        entries.push(Dynamic::object([
            ("fileUri", Dynamic::String(recent.file_uri.to_string(false))),
            ("label", store_label(&recent.label, &recent.file_uri)),
            ("remoteAuthority", optional(&recent.remote_authority)),
        ]));
    }
    Dynamic::object([("entries", Dynamic::array(entries))])
}

trait TruthyValue {
    fn is_truthy_value(&self) -> bool;
}

impl TruthyValue for Dynamic {
    fn is_truthy_value(&self) -> bool {
        je_base::dynamic::Truthy::is_truthy(self)
    }
}

// #endregion
