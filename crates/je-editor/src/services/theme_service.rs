//! Porte parcial de `src/vs/platform/theme/common/themeService.ts`: o `IColorTheme` e o `IThemeService` no
//! que a geração das regras de decoração lê (`getColorTheme` e `onDidColorThemeChange`).
//!
//! Diferenças do original:
//! - `IColorTheme` traz só `getColor(color, useDefault?)`. `type`, `label`, `semanticHighlighting`, `defines`,
//!   `getTokenStyleMetadata`, `tokenColorMap` e `tokenFontMap` não têm leitor aqui.
//! - O `IThemeService` de `view_model_impl.rs` tem só o evento, sobre o `ColorTheme` concreto do `je-theme`.
//!   Este contrato usa o tema como trait, porque o `TestColorTheme` dos testes (um mapa de cores em texto) não
//!   é um `ColorTheme` carregado de arquivo. `getFileIconTheme`, `getProductIconTheme` e os eventos de ícone
//!   ficam de fora.
//! - O [`ColorTheme`] concreto implementa o [`IColorTheme`]: `getColor(id)` sem `useDefault` usa o padrão do
//!   registro (só um `useDefault` falso o dispensa), como o `ColorThemeData`.

use std::rc::Rc;

use je_base::event::Event;
use je_theme::ColorTheme;
use je_theme::color::Color;

/// `IColorTheme` (ver as diferenças no topo do módulo).
pub trait IColorTheme {
    /// `getColor(color, useDefault?)`.
    fn get_color(&self, color: &str, use_default: Option<bool>) -> Option<Color>;
}

impl IColorTheme for ColorTheme {
    fn get_color(&self, color: &str, use_default: Option<bool>) -> Option<Color> {
        self.resolve_color(color, use_default.unwrap_or(true))
    }
}

/// `IThemeService` (ver as diferenças no topo do módulo).
pub trait IThemeService {
    /// `getColorTheme()`.
    fn get_color_theme(&self) -> Rc<dyn IColorTheme>;

    /// `onDidColorThemeChange`.
    fn on_did_color_theme_change(&self) -> Event<Rc<dyn IColorTheme>>;
}
