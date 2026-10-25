//! Os padrões de um tema por tipo (`ThemeType`) e a normalização de cores de `colorThemeData.ts`, numa definição
//! só para quem lê um tema: o `ColorThemeData` do workbench (`je-workbench`) e as regras de token do tokenizador
//! TextMate (`je-languages`).
//!
//! - `normalizeColor` (para uma string) e `defaultThemeColors` (as regras `token.*-token`) são do
//!   `colorThemeData.ts`.
//! - `editor_foreground` e `editor_background` são os padrões `editor.foreground` e `editor.background` do
//!   `colorRegistry.ts` para o tipo do tema, em maiúsculas já normalizadas.
//! - `from_ui_theme` e `ui_theme` são o `uiTheme` do `package.json` (`vs`, `vs-dark`, `hc-black`, `hc-light`), que
//!   é também o `themeTypeSelector` do `themeService.ts`.

use crate::ThemeType;

impl ThemeType {
    /// `uiTheme` do `package.json` (`vs`, `vs-dark`, `hc-black`, `hc-light`); o que não for conhecido é escuro.
    pub fn from_ui_theme(ui_theme: &str) -> Self {
        match ui_theme {
            "vs" => Self::Light,
            "hc-black" => Self::HighContrastDark,
            "hc-light" => Self::HighContrastLight,
            _ => Self::Dark,
        }
    }

    /// O inverso de [`ThemeType::from_ui_theme`]: `getThemeTypeSelector`.
    pub fn ui_theme(self) -> &'static str {
        match self {
            Self::Light => "vs",
            Self::Dark => "vs-dark",
            Self::HighContrastDark => "hc-black",
            Self::HighContrastLight => "hc-light",
        }
    }

    /// O padrão de `editor.foreground` para o tipo do tema.
    pub fn editor_foreground(self) -> &'static str {
        match self {
            Self::Light => "#333333",
            Self::Dark => "#BBBBBB",
            Self::HighContrastDark => "#FFFFFF",
            Self::HighContrastLight => "#292929",
        }
    }

    /// O padrão de `editor.background` para o tipo do tema.
    pub fn editor_background(self) -> &'static str {
        match self {
            Self::Light | Self::HighContrastLight => "#FFFFFF",
            Self::Dark => "#1E1E1E",
            Self::HighContrastDark => "#000000",
        }
    }

    /// `defaultThemeColors[baseTheme]`: as regras `token.*-token` aplicadas quando o tema não tem
    /// `token.info-token`, como pares (escopo, cor de primeiro plano).
    pub fn default_token_rules(self) -> [(&'static str, &'static str); 4] {
        let (info, warn, error, debug) = match self {
            Self::Light | Self::HighContrastLight => ("#316bcd", "#cd9731", "#cd3131", "#800080"),
            Self::Dark => ("#6796e6", "#cd9731", "#f44747", "#b267e6"),
            Self::HighContrastDark => ("#6796e6", "#008000", "#FF0000", "#b267e6"),
        };
        [("token.info-token", info), ("token.warn-token", warn), ("token.error-token", error), ("token.debug-token", debug)]
    }
}

/// `normalizeColor` para uma string: `#RGB`, `#RGBA`, `#RRGGBB` ou `#RRGGBBAA` em maiúsculas; o alfa `FF` cai.
pub fn normalize_color(color: &str) -> Option<String> {
    let digits = color.strip_prefix('#')?;
    let count = digits.chars().count();
    if ![3, 4, 6, 8].contains(&count) {
        return None;
    }
    let mut result = String::from("#");
    for c in digits.chars() {
        if !c.is_ascii_hexdigit() {
            return None;
        }
        let upper = c.to_ascii_uppercase();
        result.push(upper);
        if count <= 4 {
            result.push(upper);
        }
    }
    if result.len() == 9 && result.ends_with("FF") {
        result.truncate(7);
    }
    Some(result)
}
