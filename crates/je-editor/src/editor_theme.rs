//! Porte de `src/vs/editor/common/editorTheme.ts`: `EditorTheme`, o tema de cores que o editor consulta e que
//! é trocado no lugar quando o tema do workbench muda.
//!
//! Divergências:
//! - `IColorTheme` (`platform/theme/common/themeService.ts`) é o `je_theme::ColorTheme`, em `Rc`, como em
//!   `ViewThemeChangedEvent` (`view_events.rs`). `ColorScheme` é o `je_theme::ThemeType`, `ColorIdentifier` é
//!   `&str` e `Color` é o `je_theme::color::Color`.
//! - O original guarda o tema numa propriedade mutável e o `ViewContext` compartilha o `EditorTheme`; aqui o
//!   tema fica numa `RefCell`, então `update` recebe `&self`.
//! - `type` (palavra reservada) vira [`EditorTheme::theme_type`] e `value` vira [`EditorTheme::value`].
//! - `getColor(color)` pergunta ao tema com o padrão do registro (`useDefault` verdadeiro, o padrão do
//!   original), por [`ColorTheme::resolve_color`].

use std::cell::RefCell;
use std::rc::Rc;

use je_theme::color::Color;
use je_theme::{ColorTheme, ThemeType};

/// `EditorTheme`.
pub struct EditorTheme {
    theme: RefCell<Rc<ColorTheme>>,
}

impl EditorTheme {
    pub fn new(theme: Rc<ColorTheme>) -> Self {
        Self { theme: RefCell::new(theme) }
    }

    /// `type`.
    pub fn theme_type(&self) -> ThemeType {
        self.theme.borrow().theme_type
    }

    /// `value`.
    pub fn value(&self) -> Rc<ColorTheme> {
        self.theme.borrow().clone()
    }

    pub fn update(&self, theme: Rc<ColorTheme>) {
        *self.theme.borrow_mut() = theme;
    }

    pub fn get_color(&self, color: &str) -> Option<Color> {
        self.theme.borrow().resolve_color(color, true)
    }
}
