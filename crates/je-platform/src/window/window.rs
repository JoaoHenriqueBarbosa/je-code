//! Porte de `src/vs/platform/window/common/window.ts`, só o que o workbench já usa.
//!
//! Do arquivo original entram `DEFAULT_CUSTOM_TITLEBAR_HEIGHT` (o teste de posição das notificações
//! calcula o deslocamento do topo com ela) e o `IPath` reduzido ao `fileUri` (o que o serviço de confiança no
//! workspace lê dos arquivos de abertura; `workspace`, `label`, `type`, `openOnlyIfExists`, `exists`, `editorOptions`,
//! `selection`, `remoteAuthority`, `editorOverrideId`, `diffMode` e `mergeMode` entram junto com quem os consumir).
//! Os demais tipos (`IWindowSettings`, `IOpenWindowOptions`...) entram junto com quem os consumir.

use je_base::uri::URI;

/// `DEFAULT_CUSTOM_TITLEBAR_HEIGHT`: altura da barra de título customizada, em pixels, incluindo o espaço da
/// central de comandos.
pub const DEFAULT_CUSTOM_TITLEBAR_HEIGHT: u32 = 35;

/// `IPath` (só o `fileUri`): o arquivo a abrir, `None` quando o caminho é de uma pasta ou de um workspace.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IPath {
    pub file_uri: Option<URI>,
}
