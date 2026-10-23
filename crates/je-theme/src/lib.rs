//! Leitura de color themes do VS Code (`contributes.themes` das extensões), no formato de
//! `extensions/theme-defaults/themes/*.json`: JSON com comentários, `include` relativo que é
//! carregado antes (o tema que inclui sobrescreve), e cores `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`.
//!
//! Porta o que `src/vs/workbench/services/themes/common/colorThemeData.ts` faz ao carregar
//! (`_loadColorTheme`): cores do workbench e `tokenColors`. O registro de cores com os padrões por
//! tipo de tema (`colorRegistry.ts`) entra em seguida.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use je_base::json::{self, Value};
use je_render::Rgba;

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
    colors: HashMap<String, Rgba>,
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

/// `Color.fromHex` do `color.ts`: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`; o resto é inválido.
pub fn parse_hex(hex: &str) -> Option<Rgba> {
    let digits = hex.strip_prefix('#')?;
    let nibble = |i: usize| u8::from_str_radix(&digits[i..=i], 16).ok();
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    if !digits.is_ascii() {
        return None;
    }
    match digits.len() {
        3 => Some(Rgba::new(nibble(0)? * 17, nibble(1)? * 17, nibble(2)? * 17, 0xff)),
        4 => Some(Rgba::new(nibble(0)? * 17, nibble(1)? * 17, nibble(2)? * 17, nibble(3)? * 17)),
        6 => Some(Rgba::new(byte(0)?, byte(2)?, byte(4)?, 0xff)),
        8 => Some(Rgba::new(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
        _ => None,
    }
}

fn read_json(path: &Path) -> Result<Value, ThemeError> {
    let text = std::fs::read_to_string(path).map_err(|e| ThemeError::Io(path.to_owned(), e))?;
    let mut errors = Vec::new();
    let value = json::parse_str(&text, &mut errors);
    match (value, errors.first()) {
        (Some(v), None) => Ok(v),
        (_, Some(e)) => Err(ThemeError::Parse(path.to_owned(), je_base::json_error_messages::parse_error_message(e.error))),
        (None, None) => Err(ThemeError::Parse(path.to_owned(), "vazio".into())),
    }
}

impl ColorTheme {
    /// Carrega o tema em `path`, resolvendo a cadeia de `include`.
    pub fn load(path: &Path) -> Result<Self, ThemeError> {
        let mut colors = HashMap::new();
        let mut name = String::new();
        let mut theme_type = None;
        Self::load_into(path, &mut colors, &mut name, &mut theme_type)?;
        Ok(Self { name, theme_type: theme_type.unwrap_or(ThemeType::Dark), colors })
    }

    fn load_into(path: &Path, colors: &mut HashMap<String, Rgba>, name: &mut String, theme_type: &mut Option<ThemeType>) -> Result<(), ThemeError> {
        let Value::Object(root) = read_json(path)? else {
            return Err(ThemeError::Parse(path.to_owned(), "o tema não é um objeto".into()));
        };
        if let Some(Value::String(include)) = root.get("include") {
            let base = path.parent().unwrap_or(Path::new(".")).join(include.to_string_lossy());
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
        if let Some(Value::Object(map)) = root.get("colors") {
            for (key, value) in map.iter() {
                if let Value::String(hex) = value
                    && let Some(color) = parse_hex(&hex.to_string_lossy())
                {
                    colors.insert(key.to_string_lossy(), color);
                }
            }
        }
        Ok(())
    }

    /// A cor definida pelo tema, sem padrão do registro.
    pub fn color(&self, id: &str) -> Option<Rgba> {
        self.colors.get(id).copied()
    }
}
