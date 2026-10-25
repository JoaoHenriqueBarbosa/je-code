//! Porte de `src/vs/workbench/services/host/browser/host.ts`, só com o que os serviços portados chamam: o `IHostService`
//! com `reload`, `restart`, `hasFocus`, `onDidChangeFocus`, `onDidChangeActiveWindow` e `onDidChangeFullScreen`.
//!
//! Não entram os demais membros (`focus`, `openWindow`, `toggleFullScreen`, `close`...):
//! dependem de tipos de janela que não foram portados. Cada um entra junto com o tipo de que depende.

use je_base::event::Event;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

/// O `{ disableExtensions?: boolean }` de `IReloadOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReloadOptions {
    pub disable_extensions: Option<bool>,
}

/// `IWindowOpenable` (`IWorkspaceToOpen | IFolderToOpen | IFileToOpen`), com o `label` opcional de `IBaseWindowOpenable`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowOpenable {
    /// `IWorkspaceToOpen`.
    Workspace { workspace_uri: URI, label: Option<String> },
    /// `IFolderToOpen`.
    Folder { folder_uri: URI, label: Option<String> },
    /// `IFileToOpen`.
    File { file_uri: URI, label: Option<String> },
}

/// `IOpenWindowOptions` (com o `IBaseOpenWindowsOptions` de que herda).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenWindowOptions {
    /// Whether to reuse the window or open a new one (`forceReuseWindow`).
    pub force_reuse_window: Option<bool>,
    /// The remote authority to use when windows are opened with either no workspace (empty window) or a workspace that is
    /// neither `file://` nor `vscode-remote://`. `Some(None)` é o `null` do original (janela local); `None` é o `undefined`.
    pub remote_authority: Option<Option<String>>,
    pub force_profile: Option<String>,
    pub force_temp_profile: Option<bool>,
    pub force_new_window: Option<bool>,
    pub prefer_new_window: Option<bool>,
    pub no_recent_entry: Option<bool>,
    pub add_mode: Option<bool>,
    pub remove_mode: Option<bool>,
    pub diff_mode: Option<bool>,
    pub merge_mode: Option<bool>,
    pub goto_line_mode: Option<bool>,
    pub wait_marker_file_uri: Option<URI>,
    pub chat_session_to_open: Option<URI>,
}

/// `IHostService`, reduzido a `reload`, `restart` e `openWindow`.
pub trait IHostService {
    /// `openWindow(toOpen, options?)`: opens the provided windowable list. Tem implementação padrão (lança o erro
    /// `Method not implemented.` do dublê do original) para não quebrar quem implementa só o recorte portado.
    fn open_window(&self, _to_open: Vec<WindowOpenable>, _options: Option<OpenWindowOptions>) -> Promise<()> {
        je_base::errors::throw(je_base::errors::Error::new("Method not implemented."))
    }

    /// `reload(options?)`.
    fn reload(&self, options: Option<ReloadOptions>) -> Promise<()>;

    /// `restart()`: reinicia a aplicação.
    fn restart(&self) -> Promise<()>;

    /// `hasFocus`: se a janela tem o foco. O padrão (a janela tem foco) é o dos dublês que não observam o foco.
    fn has_focus(&self) -> bool {
        true
    }

    /// `onDidChangeFocus`: dispara com `true` quando a janela ganha o foco e com `false` quando o perde. O padrão
    /// nunca dispara, como o `Event.None` dos dublês.
    fn on_did_change_focus(&self) -> Event<bool> {
        Event::none()
    }

    /// `onDidChangeActiveWindow`: dispara com o identificador da janela que passou a ser a ativa. O padrão nunca dispara,
    /// como o `Event.None` dos dublês.
    fn on_did_change_active_window(&self) -> Event<u32> {
        Event::none()
    }

    /// `onDidChangeFullScreen`: dispara quando a janela `window_id` entra ou sai da tela cheia. O padrão nunca dispara, como o
    /// `Event.None` dos dublês.
    fn on_did_change_full_screen(&self) -> Event<FullScreenChangeEvent> {
        Event::none()
    }
}

/// O `{ windowId: number; fullscreen: boolean }` de `IHostService.onDidChangeFullScreen`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FullScreenChangeEvent {
    pub window_id: u32,
    pub fullscreen: bool,
}

service_id!(pub I_HOST_SERVICE: dyn IHostService = "hostService");
