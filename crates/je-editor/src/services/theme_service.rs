//! Porte parcial de `src/vs/platform/theme/common/themeService.ts`: o `IColorTheme` e o `IThemeService` no
//! que a geração das regras de decoração lê (`getColorTheme` e `onDidColorThemeChange`).
//!
//! Diferenças do original:
//! - `IColorTheme` traz `getColor(color, useDefault?)`, `semanticHighlighting` e `getTokenStyleMetadata`; `type`,
//!   `label`, `defines`, `tokenColorMap` e `tokenFontMap` não têm leitor aqui. Os dois últimos (que a coloração
//!   semântica lê) têm implementação padrão ("desligada" e "sem estilo"), que um tema com regras semânticas
//!   sobrescreve; o [`ColorTheme`] concreto do `je-theme` ainda não as sobrescreve.
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

/// `ITokenStyle`: o estilo de um token semântico, com a cor como índice do `tokenColorMap`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ITokenStyle {
    pub foreground: Option<u32>,
    pub bold: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub italic: Option<bool>,
}

/// `IColorTheme` (ver as diferenças no topo do módulo).
pub trait IColorTheme {
    /// `getColor(color, useDefault?)`.
    fn get_color(&self, color: &str, use_default: Option<bool>) -> Option<Color>;

    /// `semanticHighlighting`: o tema liga a coloração semântica. O padrão é desligada, como a de um tema sem
    /// regras semânticas.
    fn semantic_highlighting(&self) -> bool {
        false
    }

    /// `getTokenStyleMetadata(type, modifiers, modelLanguage)`: o estilo do tema para um token semântico, ou `None`
    /// (`undefined`) quando nenhuma regra casa. O padrão é `None`.
    fn get_token_style_metadata(&self, _token_type: &str, _modifiers: &[String], _model_language: &str) -> Option<ITokenStyle> {
        None
    }
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
