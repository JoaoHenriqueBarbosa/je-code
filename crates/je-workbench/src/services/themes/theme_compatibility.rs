//! Porte de `vs/workbench/services/themes/common/themeCompatibility.ts`.
//!
//! Converte as configurações de tema do formato antigo do TextMate (regras sem `scope`, com
//! chaves como `background`, `caret`, `ansiRed`) para as cores do workbench.

use indexmap::IndexMap;
use je_theme::color::Color;
use serde_json::Value;

/// `ITextMateThemingRule.scope`: `string | string[]`.
#[derive(Clone, Debug, PartialEq)]
pub enum TextMateScope {
    One(String),
    Many(Vec<String>),
}

impl TextMateScope {
    /// A veracidade (`!rule.scope`) em JavaScript: a string vazia é falsa, o vetor (mesmo vazio) é verdadeiro.
    pub(crate) fn is_truthy(&self) -> bool {
        match self {
            TextMateScope::One(scope) => !scope.is_empty(),
            TextMateScope::Many(_) => true,
        }
    }
}

/// `ITextMateThemingRule`. As `settings` ficam como mapa com ordem de inserção porque o formato
/// antigo carrega chaves arbitrárias (`selection`, `caret`, `ansiRed`...) além de `foreground`,
/// `background` e `fontStyle`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TextMateThemingRule {
    pub name: Option<String>,
    pub scope: Option<TextMateScope>,
    pub settings: Option<IndexMap<String, Value>>,
}

/// `IColorMap`: id da cor para a cor.
pub type ColorMap = IndexMap<String, Color>;

/// O `result` de `convertSettings`: as regras convertidas e as cores extraídas.
#[derive(Clone, Debug, Default)]
pub struct ConvertedSettings {
    pub text_mate_rules: Vec<TextMateThemingRule>,
    pub colors: ColorMap,
}

/// Os ids de cor de cada chave de configuração (`settingToColorIdMapping`), na ordem de registro.
fn setting_color_ids(setting_id: &str) -> Vec<String> {
    const ANSI_COLOR_MAP: [&str; 16] = [
        "ansiBlack",
        "ansiRed",
        "ansiGreen",
        "ansiYellow",
        "ansiBlue",
        "ansiMagenta",
        "ansiCyan",
        "ansiWhite",
        "ansiBrightBlack",
        "ansiBrightRed",
        "ansiBrightGreen",
        "ansiBrightYellow",
        "ansiBrightBlue",
        "ansiBrightMagenta",
        "ansiBrightCyan",
        "ansiBrightWhite",
    ];
    let ids: &[&str] = match setting_id {
        "background" => &["editor.background"],
        "foreground" => &["editor.foreground"],
        "selection" => &["editor.selectionBackground"],
        "inactiveSelection" => &["editor.inactiveSelectionBackground"],
        "selectionHighlightColor" => &["editor.selectionHighlightBackground"],
        "findMatchHighlight" => &[
            "editor.findMatchHighlightBackground",
            "peekViewResult.matchHighlightBackground",
        ],
        "currentFindMatchHighlight" => &["editor.findMatchBackground"],
        "hoverHighlight" => &["editor.hoverHighlightBackground"],
        // Inlinados no original para evitar dependências de editor/contrib.
        "wordHighlight" => &["editor.wordHighlightBackground"],
        "wordHighlightStrong" => &["editor.wordHighlightStrongBackground"],
        "findRangeHighlight" => &["editor.findRangeHighlightBackground"],
        "referenceHighlight" => &["peekViewEditor.matchHighlightBackground"],
        "lineHighlight" => &["editor.lineHighlightBackground"],
        "rangeHighlight" => &["editor.rangeHighlightBackground"],
        "caret" => &["editorCursor.foreground"],
        "invisibles" => &["editorWhitespace.foreground"],
        "guide" => &["editorIndentGuide.background1"],
        "activeGuide" => &["editorIndentGuide.activeBackground1"],
        other => {
            return if ANSI_COLOR_MAP.contains(&other) {
                vec![format!("terminal.{other}")]
            } else {
                Vec::new()
            };
        }
    };
    ids.iter().map(|id| (*id).to_string()).collect()
}

/// `convertSettings`: acrescenta a `result` as regras antigas e as cores que elas definem.
pub fn convert_settings(old_settings: Vec<TextMateThemingRule>, result: &mut ConvertedSettings) {
    for mut rule in old_settings {
        if !rule.scope.as_ref().is_some_and(TextMateScope::is_truthy) {
            match rule.settings.as_mut() {
                None => rule.settings = Some(IndexMap::new()),
                Some(settings) => {
                    for (key, value) in settings.iter() {
                        let mappings = setting_color_ids(key);
                        if !mappings.is_empty() {
                            if let Value::String(color_hex) = value {
                                let color = Color::from_hex(color_hex);
                                for color_id in mappings {
                                    result.colors.insert(color_id, color.clone());
                                }
                            }
                        }
                    }
                    settings.retain(|key, _| matches!(key.as_str(), "foreground" | "background" | "fontStyle"));
                }
            }
        }
        result.text_mate_rules.push(rule);
    }
}
