//! Dublês comuns dos testes das partes do shell (`statusbarPart.test.ts` e `titlebarPart.test.ts`): o `TestHostService` com foco e
//! janela ativa, e o `TestContextMenuService`.
#![allow(dead_code)]

use std::cell::Cell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_platform::contextview::context_view::{ContextMenuRequest, IContextMenuService};
use je_workbench::browser::parts::part_support::TargetWindow;
use je_workbench::services::host::host::{IHostService, ReloadOptions};
use je_workbench::services::layout::layout_service::MAIN_WINDOW;

/// `TestHostService`: `setFocus(focus)` troca o foco e dispara `onDidChangeFocus`; `onDidChangeActiveWindow` é o emissor
/// `active_window`, que os casos disparam (`override readonly onDidChangeActiveWindow = activeWindowEmitter.event`).
pub struct TestHostService {
    has_focus: Cell<bool>,
    on_did_change_focus: Rc<Emitter<bool>>,
    pub active_window: Rc<Emitter<u32>>,
}

impl TestHostService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { has_focus: Cell::new(true), on_did_change_focus: Emitter::new(), active_window: Emitter::new() })
    }

    /// `setFocus(focus)`.
    pub fn set_focus(&self, focus: bool) {
        self.has_focus.set(focus);
        self.on_did_change_focus.fire(focus);
    }
}

impl IHostService for TestHostService {
    fn reload(&self, _options: Option<ReloadOptions>) -> Promise<()> {
        Promise::resolved(())
    }

    fn restart(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn has_focus(&self) -> bool {
        self.has_focus.get()
    }

    fn on_did_change_focus(&self) -> Event<bool> {
        self.on_did_change_focus.event()
    }

    fn on_did_change_active_window(&self) -> Event<u32> {
        self.active_window.event()
    }
}

impl IDisposable for TestHostService {
    fn dispose(&self) {
        self.on_did_change_focus.dispose();
        self.active_window.dispose();
    }
}

/// `TestContextMenuService`: nenhum caso abre um menu.
pub struct TestContextMenuService;

impl IContextMenuService for TestContextMenuService {
    fn on_did_show_context_menu(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_hide_context_menu(&self) -> Event<()> {
        Event::none()
    }

    fn show_context_menu(&self, _request: ContextMenuRequest) {}
}

/// A janela principal (`mainWindow`) cujo `document.hasFocus()` é a leitura dada.
pub fn main_window(has_focus: Rc<dyn Fn() -> bool>) -> TargetWindow {
    TargetWindow { id: MAIN_WINDOW, has_focus }
}
