//! Porte parcial de `src/vs/platform/clipboard/common/clipboardService.ts`: o `IClipboardService`.
//!
//! `writeText`, `readText` (o que o hover lê), `readFindText` e `writeFindText` (a área de transferência de busca do
//! macOS, que a busca do editor lê) e `writeResources`, `readResources` e `hasResources` (o que o explorador de arquivos
//! lê no recortar, copiar e colar). O resto do contrato (`readImage`, `triggerPaste`) entra com o porte da plataforma de
//! área de transferência. O `type?: string` (seleção do X11) fica de
//! fora pelo mesmo motivo.

use je_base::JsString;
use je_base::promise::Promise;
use je_base::uri::URI;

/// `IClipboardService` (recorte mínimo, ver o topo do módulo).
pub trait IClipboardService {
    /// `writeText(text)`: grava o texto na área de transferência.
    fn write_text(&self, text: &str) -> Promise<()>;

    /// `readText()`: o texto da área de transferência (vazio quando não há).
    fn read_text(&self) -> Promise<String>;

    /// `readFindText()`: reads text from the system find pasteboard.
    fn read_find_text(&self) -> Promise<JsString>;

    /// `writeFindText(text)`: writes text to the system find pasteboard.
    fn write_find_text(&self, text: &JsString) -> Promise<()>;

    /// `writeResources(resources)`: writes resources to the system clipboard.
    fn write_resources(&self, resources: Vec<URI>) -> Promise<()>;

    /// `readResources()`: reads resources from the system clipboard.
    fn read_resources(&self) -> Promise<Vec<URI>>;

    /// `hasResources()`: find out if resources are copied to the clipboard.
    fn has_resources(&self) -> Promise<bool>;
}

je_instantiation::service_id!(pub I_CLIPBOARD_SERVICE: dyn IClipboardService = "clipboardService");

