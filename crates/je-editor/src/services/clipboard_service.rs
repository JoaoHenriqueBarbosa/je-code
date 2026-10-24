//! Porte parcial de `src/vs/platform/clipboard/common/clipboardService.ts`: o `IClipboardService`.
//!
//! Só `writeText` e `readText`, os dois membros que o hover lê. O resto do contrato (`readImage`,
//! `readFindText`, `writeFindText`, `writeResources`, `readResources`, `hasResources`, `triggerPaste`) entra com o
//! porte da plataforma de área de transferência. O `type?: string` (seleção do X11) fica de fora pelo mesmo motivo.

use je_base::promise::Promise;

/// `IClipboardService` (recorte mínimo, ver o topo do módulo).
pub trait IClipboardService {
    /// `writeText(text)`: grava o texto na área de transferência.
    fn write_text(&self, text: &str) -> Promise<()>;

    /// `readText()`: o texto da área de transferência (vazio quando não há).
    fn read_text(&self) -> Promise<String>;
}
