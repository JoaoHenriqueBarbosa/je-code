//! Porte de `src/vs/workbench/api/common/extHostFileSystemInfo.ts`: as capacidades dos provedores de sistema de
//! arquivos registrados por esquema, e o `ExtUri` que decide se o caso do caminho conta.
//!
//! Divergências:
//! - `IExtHostFileSystemInfo` (o decorator e a interface com `extUri`) é o próprio [`ExtHostFileSystemInfo`]: o
//!   trait de mesmo nome de `ext_host_workspace` só tem o `getCapabilities`, e este struct o implementa.
//! - `$acceptProviderInfos(uri, capabilities)` é `rpc_accept_provider_infos` do
//!   [`ExtHostFileSystemInfoShape`]; `number | null` é `Option<FileSystemProviderCapabilities>`.
//! - `Object.keys(Schemas)` são os NOMES das propriedades do espaço de nomes `Schemas` (`inMemory`, `vscodeRemote`...), e
//!   não os valores (`inmemory`, `vscode-remote`...): a lista é a do original, com os mesmos nomes.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_platform::files::files::FileSystemProviderCapabilities;
use je_platform::resources::host_environment;

use super::ext_host_protocol::ExtHostFileSystemInfoShape;
use super::ext_host_workspace::IExtHostFileSystemInfo;

/// `Object.keys(Schemas)`.
const SCHEMAS_KEYS: &[&str] = &[
    "inMemory",
    "vscode",
    "internal",
    "walkThrough",
    "walkThroughSnippet",
    "vscodeOnboardingSample",
    "http",
    "https",
    "file",
    "mailto",
    "untitled",
    "data",
    "command",
    "vscodeRemote",
    "vscodeRemoteResource",
    "vscodeManagedRemoteResource",
    "vscodeUserData",
    "vscodeCustomEditor",
    "vscodeNotebookCell",
    "vscodeNotebookCellMetadata",
    "vscodeNotebookCellMetadataDiff",
    "vscodeNotebookCellOutput",
    "vscodeNotebookCellOutputDiff",
    "vscodeNotebookMetadata",
    "vscodeInteractiveInput",
    "vscodeSettings",
    "vscodeWorkspaceTrust",
    "vscodeTerminal",
    "vscodeImageCarousel",
    "vscodeChatCodeBlock",
    "vscodeChatCodeCompareBlock",
    "vscodeChatEditor",
    "vscodeChatInput",
    "sessionsChatInput",
    "vscodeLocalChatSession",
    "vscodeChatResponseResource",
    "agentHostTerminal",
    "vscodeChatTerminalOutput",
    "webviewPanel",
    "vscodeWebview",
    "vscodeBrowser",
    "extension",
    "vscodeFileResource",
    "tmp",
    "vsls",
    "vscodeSourceControl",
    "commentsInput",
    "codeSetting",
    "outputChannel",
    "accessibleView",
    "chatEditingSnapshotScheme",
    "chatEditingModel",
    "copilotPr",
];

/// `ExtHostFileSystemInfo`.
pub struct ExtHostFileSystemInfo {
    system_schemes: HashSet<&'static str>,
    provider_info: Rc<RefCell<HashMap<String, FileSystemProviderCapabilities>>>,
    ext_uri: ExtUri,
}

impl Default for ExtHostFileSystemInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtHostFileSystemInfo {
    pub fn new() -> ExtHostFileSystemInfo {
        let provider_info: Rc<RefCell<HashMap<String, FileSystemProviderCapabilities>>> = Rc::new(RefCell::new(HashMap::new()));
        let info = provider_info.clone();
        let ext_uri = ExtUri::new(host_environment(), move |uri: &URI| {
            let capabilities = info.borrow().get(&uri.scheme().to_string_lossy()).copied();
            match capabilities {
                // default: not ignore
                None => false,
                // configured as case sensitive
                Some(capabilities) if capabilities.intersects(FileSystemProviderCapabilities::PATH_CASE_SENSITIVE) => false,
                Some(_) => true,
            }
        });
        ExtHostFileSystemInfo { system_schemes: SCHEMAS_KEYS.iter().copied().collect(), provider_info, ext_uri }
    }

    /// `extUri`.
    pub fn ext_uri(&self) -> &ExtUri {
        &self.ext_uri
    }

    pub fn is_free_scheme(&self, scheme: &str) -> bool {
        !self.provider_info.borrow().contains_key(scheme) && !self.system_schemes.contains(scheme)
    }
}

impl IExtHostFileSystemInfo for ExtHostFileSystemInfo {
    fn get_capabilities(&self, scheme: &str) -> Option<FileSystemProviderCapabilities> {
        self.provider_info.borrow().get(scheme).copied()
    }
}

impl ExtHostFileSystemInfoShape for ExtHostFileSystemInfo {
    fn rpc_accept_provider_infos(&self, uri: &URI, capabilities: Option<FileSystemProviderCapabilities>) {
        let scheme = uri.scheme().to_string_lossy();
        match capabilities {
            None => {
                self.provider_info.borrow_mut().remove(&scheme);
            }
            Some(capabilities) => {
                self.provider_info.borrow_mut().insert(scheme, capabilities);
            }
        }
    }
}
