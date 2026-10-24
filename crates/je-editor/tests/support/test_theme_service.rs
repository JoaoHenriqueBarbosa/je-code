//! Porte de `src/vs/platform/theme/test/common/testThemeService.ts`: o `TestThemeService`.
//!
//! Divergências:
//! - O [`IThemeService`] do porte (`view_model_impl.rs`) traz só `onDidColorThemeChange`, o que o `ViewModel` lê.
//!   `getColorTheme`, `getProductIconTheme`, `getFileIconTheme` e os eventos de ícone não têm porte, então o
//!   `TestColorTheme` (o tema que o construtor recebe e `getColorTheme` devolve) também não existe aqui.
//! - `onThemeChange(theme)` dispara `onDidColorThemeChange` com o tema recebido, como o original.

use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_editor::view_model_impl::IThemeService;
use je_theme::ColorTheme;

pub struct TestThemeService {
    on_theme_change_emitter: Rc<Emitter<Rc<ColorTheme>>>,
}

impl TestThemeService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { on_theme_change_emitter: Emitter::new() })
    }

    /// `onThemeChange(theme)`.
    pub fn on_theme_change(&self, theme: Rc<ColorTheme>) {
        self.on_theme_change_emitter.fire(theme);
    }
}

impl IThemeService for TestThemeService {
    fn on_did_color_theme_change(&self) -> Event<Rc<ColorTheme>> {
        self.on_theme_change_emitter.event()
    }
}

impl IDisposable for TestThemeService {
    fn dispose(&self) {
        self.on_theme_change_emitter.dispose();
    }
}
