//! Porte do contrato `IDialogMainService` de `src/vs/platform/dialogs/electron-main/dialogMainService.ts`, só com o que o
//! `WorkspacesManagementMainService` usa: o `showMessageBox`.
//!
//! Não há processo principal do Electron no je-code, mas o papel dele existe no processo único; a implementação
//! (`DialogMainService`, o `doPick` e as filas por janela) fica por conta das interfaces nativas, que desenham o diálogo.
//!
//! Decisões de tradução:
//! - **`electron.MessageBoxOptions`** é [`MessageBoxOptions`] com os campos que o serviço de workspaces preenche (`type`,
//!   `buttons`, `message` e `detail`); **`electron.MessageBoxReturnValue`** é [`MessageBoxReturnValue`].
//! - **O argumento `window?: electron.BrowserWindow`** não existe: no original ele é a janela em foco
//!   (`electron.BrowserWindow.getFocusedWindow()`), e no processo único quem desenha o diálogo já sabe qual é.
//! - **Os demais métodos** (`pickFileFolder`, `pickFolder`, `pickFile`, `pickWorkspace`, `showSaveDialog` e
//!   `showOpenDialog`) entram junto com quem os consumir.

use je_base::promise::Promise;
use je_instantiation::service_id;

/// `electron.MessageBoxOptions.type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageBoxType {
    None,
    Info,
    Error,
    Question,
    Warning,
}

/// `electron.MessageBoxOptions`, só com o que os chamadores preenchem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageBoxOptions {
    pub message_box_type: Option<MessageBoxType>,
    pub buttons: Vec<String>,
    pub message: String,
    pub detail: Option<String>,
}

/// `electron.MessageBoxReturnValue`: o índice do botão apertado e o estado da caixa de seleção.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageBoxReturnValue {
    pub response: usize,
    pub checkbox_checked: bool,
}

/// `IDialogMainService`.
pub trait IDialogMainService {
    fn show_message_box(&self, options: MessageBoxOptions) -> Promise<MessageBoxReturnValue>;
}

service_id!(pub I_DIALOG_MAIN_SERVICE: dyn IDialogMainService = "dialogMainService");
