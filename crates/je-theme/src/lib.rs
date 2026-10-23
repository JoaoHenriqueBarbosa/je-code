//! Leitura de color themes do VS Code (`contributes.themes` das extensões), no formato de
//! `extensions/theme-defaults/themes/*.json`: JSON com comentários, `include` relativo que é
//! carregado antes (o tema que inclui sobrescreve), e cores `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`.
//!
//! Porta o que `src/vs/workbench/services/themes/common/colorThemeData.ts` faz ao carregar
//! (`_loadColorTheme`) e ao resolver uma cor (`getColor`, `defines`, `getDefault`): a cor do tema
//! (ou da customização) se existe, senão o padrão do registro para o tipo do tema.
//!
//! - [`color`]: a classe `Color` de `src/vs/base/common/color.ts`.
//! - [`registry`]: `colorUtils.ts` e `colorRegistry.ts` (o registro e a resolução de valores).
//! - [`registered_colors`]: as cores registradas pelo VS Code (gerado).

pub mod color;
#[rustfmt::skip]
pub mod registered_colors;
pub mod registry;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use je_base::json::{self, Value};
use je_render::Rgba;

use crate::color::Color;
use crate::registry::{ColorThemeContext, DEFAULT_COLOR_CONFIG_VALUE, color_registry};

/// Tipo do tema (`ColorScheme`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeType {
    Dark,
    Light,
    HighContrastDark,
    HighContrastLight,
}

/// Um tema carregado.
#[derive(Clone, Debug)]
pub struct ColorTheme {
    pub name: String,
    pub theme_type: ThemeType,
    /// `colorMap`: as cores do arquivo do tema (com os `include`).
    colors: HashMap<String, Color>,
    /// `customColorMap`: as customizações do usuário (`workbench.colorCustomizations`).
    custom_colors: HashMap<String, CustomColor>,
    /// `transientColorMap`: cores só de execução, que vencem todas as outras.
    transient_colors: Option<HashMap<String, Color>>,
}

/// O valor de uma customização de cor: `"default"` (usar o padrão do registro) ou uma cor.
#[derive(Clone, Debug, PartialEq)]
pub enum CustomColor {
    Default,
    Color(Color),
}

#[derive(Debug)]
pub enum ThemeError {
    Io(PathBuf, std::io::Error),
    Parse(PathBuf, String),
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThemeError::Io(p, e) => write!(f, "{}: {e}", p.display()),
            ThemeError::Parse(p, e) => write!(f, "{}: {e}", p.display()),
        }
    }
}

impl std::error::Error for ThemeError {}

/// A cor como o `Rgba` de 8 bits do renderizador: o alfa é `Math.round(a * 255)`, como em
/// `Color.Format.CSS.formatHexA`.
pub fn to_rgba8(color: &Color) -> Rgba {
    let rgba = color.rgba();
    Rgba::new(rgba.r, rgba.g, rgba.b, color.alpha_byte())
}

fn read_json(path: &Path) -> Result<Value, ThemeError> {
    let text = std::fs::read_to_string(path).map_err(|e| ThemeError::Io(path.to_owned(), e))?;
    let mut errors = Vec::new();
    let value = json::parse_str(&text, &mut errors);
    match (value, errors.first()) {
        (Some(v), None) => Ok(v),
        (_, Some(e)) => Err(ThemeError::Parse(
            path.to_owned(),
            je_base::json_error_messages::parse_error_message(e.error),
        )),
        (None, None) => Err(ThemeError::Parse(path.to_owned(), "vazio".into())),
    }
}

impl ColorTheme {
    /// Carrega o tema em `path`, resolvendo a cadeia de `include`.
    ///
    /// O tipo vem do `type` do JSON (`dark` se ausente). No VS Code o tipo vem do `uiTheme` do
    /// `package.json` da extensão: use [`ColorTheme::with_type`] quando ele for conhecido.
    pub fn load(path: &Path) -> Result<Self, ThemeError> {
        let mut colors = HashMap::new();
        let mut name = String::new();
        let mut theme_type = None;
        Self::load_into(path, &mut colors, &mut name, &mut theme_type)?;
        Ok(Self {
            name,
            theme_type: theme_type.unwrap_or(ThemeType::Dark),
            colors,
            custom_colors: HashMap::new(),
            transient_colors: None,
        })
    }

    /// Troca o tipo do tema (o `uiTheme` da contribuição).
    pub fn with_type(mut self, theme_type: ThemeType) -> Self {
        self.theme_type = theme_type;
        self
    }

    fn load_into(
        path: &Path,
        colors: &mut HashMap<String, Color>,
        name: &mut String,
        theme_type: &mut Option<ThemeType>,
    ) -> Result<(), ThemeError> {
        let Value::Object(root) = read_json(path)? else {
            return Err(ThemeError::Parse(
                path.to_owned(),
                "o tema não é um objeto".into(),
            ));
        };
        if let Some(Value::String(include)) = root.get("include") {
            let base = path
                .parent()
                .unwrap_or(Path::new("."))
                .join(include.to_string_lossy());
            Self::load_into(&base, colors, name, theme_type)?;
        }
        if let Some(Value::String(n)) = root.get("name") {
            *name = n.to_string_lossy();
        }
        if let Some(Value::String(t)) = root.get("type") {
            *theme_type = match t.to_string_lossy().as_str() {
                "light" => Some(ThemeType::Light),
                "hc" | "hcDark" => Some(ThemeType::HighContrastDark),
                "hcLight" => Some(ThemeType::HighContrastLight),
                _ => Some(ThemeType::Dark),
            };
        }
        // `_loadColorTheme`: `"default"` apaga a cor herdada, string vira `Color.fromHex` (hexadecimal
        // inválido vira vermelho) e qualquer outro valor, `null` inclusive, é ignorado.
        if let Some(Value::Object(map)) = root.get("colors") {
            for (key, value) in map.iter() {
                let Value::String(text) = value else { continue };
                let (key, text) = (key.to_string_lossy(), text.to_string_lossy());
                if text == DEFAULT_COLOR_CONFIG_VALUE {
                    colors.remove(&key);
                } else {
                    colors.insert(key, Color::from_hex(&text));
                }
            }
        }
        Ok(())
    }

    /// `setCustomColors`: substitui as customizações (`workbench.colorCustomizations`). O valor
    /// `"default"` manda usar o padrão do registro, qualquer outro texto é `Color.fromHex`.
    pub fn set_custom_colors<I, K, V>(&mut self, colors: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: AsRef<str>,
    {
        self.custom_colors = colors
            .into_iter()
            .map(|(id, value)| {
                let value = value.as_ref();
                let custom = if value == DEFAULT_COLOR_CONFIG_VALUE {
                    CustomColor::Default
                } else {
                    CustomColor::Color(Color::from_hex(value))
                };
                (id.into(), custom)
            })
            .collect();
    }

    /// `setTransientColors`: cores só de execução, que vencem tema e customizações.
    pub fn set_transient_colors(&mut self, colors: Option<HashMap<String, Color>>) {
        self.transient_colors = colors;
    }

    /// A cor definida pelo arquivo do tema, sem customização nem padrão do registro.
    pub fn color(&self, id: &str) -> Option<Rgba> {
        self.colors.get(id).map(to_rgba8)
    }

    /// `getColor(colorId, useDefault)`: transiente, depois customização, depois a cor do tema (esta só
    /// se a customização não pede `"default"`), por fim o padrão do registro se `use_default`.
    pub fn resolve_color(&self, id: &str, use_default: bool) -> Option<Color> {
        if let Some(color) = self.transient_colors.as_ref().and_then(|map| map.get(id)) {
            return Some(color.clone());
        }
        match self.custom_colors.get(id) {
            Some(CustomColor::Color(color)) => return Some(color.clone()),
            None => {
                if let Some(color) = self.colors.get(id) {
                    return Some(color.clone());
                }
            }
            Some(CustomColor::Default) => {}
        }
        if use_default {
            self.get_default(id)
        } else {
            None
        }
    }

    /// `getColor(colorId)` com padrão, como o renderizador a usa.
    pub fn get_color(&self, id: &str) -> Option<Rgba> {
        self.resolve_color(id, true).as_ref().map(to_rgba8)
    }

    /// `getDefault`: o padrão do registro para o tipo deste tema.
    pub fn get_default(&self, id: &str) -> Option<Color> {
        color_registry().resolve_default_color(id, self)
    }

    /// `defines`: o tema, as customizações ou as cores transientes definem a cor (o padrão do registro não conta).
    pub fn defines(&self, id: &str) -> bool {
        if self
            .transient_colors
            .as_ref()
            .is_some_and(|map| map.contains_key(id))
        {
            return true;
        }
        match self.custom_colors.get(id) {
            Some(CustomColor::Color(_)) => true,
            Some(CustomColor::Default) => false,
            None => self.colors.contains_key(id),
        }
    }
}

impl ColorThemeContext for ColorTheme {
    fn theme_type(&self) -> ThemeType {
        self.theme_type
    }

    fn get_color(&self, id: &str, use_default: bool) -> Option<Color> {
        self.resolve_color(id, use_default)
    }

    fn defines(&self, id: &str) -> bool {
        ColorTheme::defines(self, id)
    }
}

// Registro de tamanhos do tema (`sizeUtils.ts`, `sizeRegistry.ts`, `sizes/`).
pub mod size_utils;
pub mod size_registry;
pub mod sizes;
