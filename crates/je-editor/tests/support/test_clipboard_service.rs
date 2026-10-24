//! `TestClipboardService` de `src/vs/platform/clipboard/test/common/testClipboardService.ts`: guarda o texto
//! escrito, sem tocar na área de transferência do sistema. Só implementa o recorte do `IClipboardService` que o
//! porte tem (`writeText` e `readText`).

use std::cell::RefCell;

use je_base::promise::Promise;
use je_editor::services::clipboard_service::IClipboardService;

#[derive(Default)]
pub struct TestClipboardService {
    text: RefCell<Option<String>>,
}

impl TestClipboardService {
    pub fn new() -> Self {
        Self::default()
    }
}

impl IClipboardService for TestClipboardService {
    fn write_text(&self, text: &str) -> Promise<()> {
        *self.text.borrow_mut() = Some(text.to_string());
        Promise::resolved(())
    }

    fn read_text(&self) -> Promise<String> {
        Promise::resolved(self.text.borrow().clone().unwrap_or_default())
    }
}
