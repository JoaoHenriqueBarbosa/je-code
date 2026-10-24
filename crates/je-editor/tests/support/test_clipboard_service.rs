//! `TestClipboardService` de `src/vs/platform/clipboard/test/common/testClipboardService.ts`: guarda o texto e os
//! recursos escritos, sem tocar na área de transferência do sistema. Só implementa o recorte do `IClipboardService`
//! que o porte tem (`writeText`, `readText`, `writeResources`, `readResources` e `hasResources`).

use std::cell::RefCell;

use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::services::clipboard_service::IClipboardService;

#[derive(Default)]
pub struct TestClipboardService {
    text: RefCell<Option<String>>,
    resources: RefCell<Option<Vec<URI>>>,
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

    fn write_resources(&self, resources: Vec<URI>) -> Promise<()> {
        *self.resources.borrow_mut() = Some(resources);
        Promise::resolved(())
    }

    fn read_resources(&self) -> Promise<Vec<URI>> {
        Promise::resolved(self.resources.borrow().clone().unwrap_or_default())
    }

    fn has_resources(&self) -> Promise<bool> {
        Promise::resolved(self.resources.borrow().as_ref().is_some_and(|resources| !resources.is_empty()))
    }
}
