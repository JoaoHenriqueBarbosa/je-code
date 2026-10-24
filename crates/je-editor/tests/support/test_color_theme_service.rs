//! Porte de `src/vs/platform/theme/test/common/testThemeService.ts`: o `TestColorTheme` e o `TestThemeService` com o
//! tema de cores como trait (o `IThemeService` de `je_editor::services::theme_service`).
//!
//! Diferenças do original:
//! - `TestColorTheme` guarda só o mapa de cores em texto (`colors`); `label`, `type`, `semanticHighlighting`,
//!   `defines`, `getTokenStyleMetadata`, `tokenColorMap` e `tokenFontMap` não são lidos pelo contrato do porte.
//! - O `TestThemeService` não tem os temas de ícone de arquivo e de produto, que o `IThemeService` do porte não
//!   traz. Existe também o `TestThemeService` de `test_theme_service.rs`, do `IThemeService` mais antigo do
//!   `ViewModel` (só o evento, sobre o `ColorTheme` concreto).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_editor::services::theme_service::{IColorTheme, IThemeService};
use je_theme::color::Color;

pub struct TestColorTheme {
    colors: HashMap<String, String>,
}

impl TestColorTheme {
    /// `new TestColorTheme({ id: '#hex', ... })`.
    pub fn new(colors: &[(&str, &str)]) -> Rc<Self> {
        Rc::new(Self { colors: colors.iter().map(|(id, value)| (id.to_string(), value.to_string())).collect() })
    }
}

impl IColorTheme for TestColorTheme {
    fn get_color(&self, color: &str, _use_default: Option<bool>) -> Option<Color> {
        let value = self.colors.get(color)?;
        if value.is_empty() {
            return None;
        }
        Some(Color::from_hex(value))
    }
}

pub struct TestThemeService {
    color_theme: RefCell<Rc<dyn IColorTheme>>,
    on_theme_change: Rc<Emitter<Rc<dyn IColorTheme>>>,
}

impl TestThemeService {
    /// `new TestThemeService(theme)`.
    pub fn new(theme: Rc<dyn IColorTheme>) -> Rc<Self> {
        Rc::new(Self { color_theme: RefCell::new(theme), on_theme_change: Emitter::new() })
    }

    /// `new TestThemeService()`: com o `TestColorTheme` vazio.
    pub fn new_default() -> Rc<Self> {
        Self::new(TestColorTheme::new(&[]))
    }

    /// `setTheme(theme)`: troca o tema e avisa quem ouve.
    pub fn set_theme(&self, theme: Rc<dyn IColorTheme>) {
        *self.color_theme.borrow_mut() = theme.clone();
        self.on_theme_change.fire(theme);
    }
}

impl IThemeService for TestThemeService {
    fn get_color_theme(&self) -> Rc<dyn IColorTheme> {
        self.color_theme.borrow().clone()
    }

    fn on_did_color_theme_change(&self) -> Event<Rc<dyn IColorTheme>> {
        self.on_theme_change.event()
    }
}

impl IDisposable for TestThemeService {
    fn dispose(&self) {
        self.on_theme_change.dispose();
    }
}
