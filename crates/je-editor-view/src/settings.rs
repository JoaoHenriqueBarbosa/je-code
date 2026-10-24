//! As configurações do usuário que o editor obedece, lidas do `settings.json` do VS Code como o
//! `TextResourceConfigurationService` e o `ModelService` as entregam ao editor: as chaves `editor.*` e `files.*`
//! gerais, com as do bloco da linguagem (`"[rust]": { ... }`, ou `"[rust][toml]"`) por cima, e cada chave aceita
//! nas três formas do arquivo (`"editor.minimap.enabled": false`, `"editor.minimap": { "enabled": false }` e
//! `"editor": { "minimap": { "enabled": false } }`).
//!
//! [`EditorSettings::resolve`] devolve os valores já validados como o `EditorOptions` do Monaco valida (limites,
//! padrões e enumerações) e, em [`EditorSettings::editor_options`], o objeto `editor.*` inteiro para o
//! `updateOptions` do `CodeEditorWidget` (as opções que só o núcleo lê, como `autoClosingBrackets`, passam por ali).

use je_base::dynamic::Dynamic;
use je_editor::config::font_info::BareFontInfo;
use serde_json::{Map, Value};

use crate::folding::WordWrap;
use crate::layout::{DEFAULT_FONT_FAMILY, DEFAULT_FONT_SIZE};

/// `editor.lineNumbers`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineNumbers {
    On,
    Off,
    /// A distância até a linha do cursor; a do cursor mostra o número, alinhado à esquerda.
    Relative,
    /// A linha do cursor, as múltiplas de 10 e a última.
    Interval,
}

/// `editor.renderWhitespace`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderWhitespace {
    None,
    /// Só o que não é um espaço solto entre palavras (tabulações, sequências, recuo e fim de linha).
    Boundary,
    Selection,
    Trailing,
    All,
}

/// `editor.cursorStyle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorStyle {
    Line,
    Block,
    Underline,
    LineThin,
    BlockOutline,
    UnderlineThin,
}

/// `editor.cursorBlinking`: `smooth`, `phase` e `expand` são animações de opacidade do mesmo período do `blink`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorBlinking {
    Blink,
    Smooth,
    Phase,
    Expand,
    Solid,
}

/// `editor.renderLineHighlight`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderLineHighlight {
    None,
    Gutter,
    Line,
    All,
}

impl RenderLineHighlight {
    pub fn content(self) -> bool {
        matches!(self, Self::Line | Self::All)
    }

    pub fn gutter(self) -> bool {
        matches!(self, Self::Gutter | Self::All)
    }
}

/// Uma régua de `editor.rulers`: a coluna e a cor própria (`{ "column": 80, "color": "#ff0000" }`).
#[derive(Clone, Debug, PartialEq)]
pub struct Ruler {
    pub column: f64,
    pub color: Option<String>,
}

/// As configurações resolvidas para um editor de uma linguagem.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorSettings {
    pub font_family: String,
    pub font_size: f32,
    pub font_weight: u16,
    /// A altura de linha em px já calculada (`0` vira a razão áurea do tamanho, abaixo de 8 é múltiplo do tamanho).
    pub line_height: f64,
    pub tab_size: i32,
    /// `editor.indentSize`: `None` é `"tabSize"`.
    pub indent_size: Option<i32>,
    pub insert_spaces: bool,
    pub detect_indentation: bool,
    pub word_wrap: WordWrap,
    pub word_wrap_column: i32,
    pub line_numbers: LineNumbers,
    pub minimap: bool,
    pub render_whitespace: RenderWhitespace,
    pub cursor_style: CursorStyle,
    pub cursor_blinking: CursorBlinking,
    pub render_line_highlight: RenderLineHighlight,
    pub rulers: Vec<Ruler>,
    pub indent_guides: bool,
    pub bracket_pair_colorization: bool,
    pub insert_final_newline: bool,
    pub trim_trailing_whitespace: bool,
    /// `files.trimTrailingWhitespaceInRegexAndStrings`.
    pub trim_in_regex_and_strings: bool,
    pub trim_final_newlines: bool,
    /// As chaves `editor.*` como objeto (sem o prefixo), com as da linguagem por cima.
    editor: Map<String, Value>,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self::resolve(&Map::new(), "")
    }
}

/// As chaves de um bloco de linguagem (`[rust]`, `[rust][toml]`) que valem para `language_id`.
fn language_blocks<'a>(settings: &'a Map<String, Value>, language_id: &str) -> Vec<&'a Map<String, Value>> {
    settings
        .iter()
        .filter(|(key, _)| key.starts_with('['))
        .filter(|(key, _)| key.trim_start_matches('[').trim_end_matches(']').split("][").any(|id| id == language_id))
        .filter_map(|(_, value)| value.as_object())
        .collect()
}

/// O valor de `path` (`editor.minimap.enabled`) em `map`, em qualquer uma das formas do arquivo: a chave inteira,
/// um prefixo dela com o resto dentro do objeto, ou o objeto da seção.
fn lookup<'a>(map: &'a Map<String, Value>, path: &str) -> Option<&'a Value> {
    let parts: Vec<&str> = path.split('.').collect();
    for split in (1..=parts.len()).rev() {
        let Some(mut value) = map.get(&parts[..split].join(".")) else { continue };
        let mut found = true;
        for part in &parts[split..] {
            match value.get(part) {
                Some(inner) => value = inner,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if found {
            return Some(value);
        }
    }
    None
}

/// Junta `value` em `target` (objetos mesclam chave a chave, o resto troca), como a sobreposição das configurações.
fn merge(target: &mut Map<String, Value>, key: &str, value: &Value) {
    match (target.get_mut(key), value) {
        (Some(Value::Object(existing)), Value::Object(update)) => {
            for (k, v) in update {
                merge(existing, k, v);
            }
        }
        _ => {
            target.insert(key.to_owned(), value.clone());
        }
    }
}

/// As chaves de seção `section` (`editor`) de `map` como objeto sem o prefixo: `editor.a.b: 1` vira `{ a: { b: 1 } }`.
fn collect_section(map: &Map<String, Value>, section: &str, into: &mut Map<String, Value>) {
    let prefix = format!("{section}.");
    for (key, value) in map {
        if key == section {
            if let Value::Object(object) = value {
                for (k, v) in object {
                    merge(into, k, v);
                }
            }
            continue;
        }
        let Some(rest) = key.strip_prefix(&prefix) else { continue };
        let mut parts = rest.split('.').collect::<Vec<_>>();
        let last = parts.pop().unwrap_or_default();
        let mut nested = value.clone();
        nested = Value::Object(Map::from_iter([(last.to_owned(), nested)]));
        for part in parts.iter().rev() {
            nested = Value::Object(Map::from_iter([((*part).to_owned(), nested)]));
        }
        if let Value::Object(object) = nested {
            for (k, v) in &object {
                merge(into, k, v);
            }
        }
    }
}

fn as_bool(value: Option<&Value>, default: bool) -> bool {
    value.and_then(Value::as_bool).unwrap_or(default)
}

/// `EditorFloatOption.float`/`EditorIntOption.clampedInt`: número ou texto numérico, senão o padrão.
fn as_number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn clamped_int(value: Option<&Value>, default: i32, min: i32, max: i32) -> i32 {
    as_number(value).map_or(default, |n| (n.floor() as i64).clamp(i64::from(min), i64::from(max)) as i32)
}

fn as_str(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

/// `editor.fontWeight`: `normal` (400), `bold` (700) ou um número de 1 a 1000.
fn font_weight(value: Option<&Value>) -> u16 {
    match value {
        Some(Value::String(s)) if s == "bold" => 700,
        Some(Value::String(s)) if s == "normal" => 400,
        other => as_number(other).map_or(400, |n| n.clamp(1.0, 1000.0) as u16),
    }
}

impl EditorSettings {
    /// Resolve as configurações de `settings` (o objeto do `settings.json`) para um editor de `language_id`.
    pub fn resolve(settings: &Map<String, Value>, language_id: &str) -> Self {
        let blocks = language_blocks(settings, language_id);
        // O valor da linguagem vence o geral (o último bloco que casa vence os anteriores).
        let get = |path: &str| blocks.iter().rev().find_map(|block| lookup(block, path)).or_else(|| lookup(settings, path));
        let mut editor = Map::new();
        collect_section(settings, "editor", &mut editor);
        for block in &blocks {
            collect_section(block, "editor", &mut editor);
        }

        let font_family = as_str(get("editor.fontFamily")).filter(|f| !f.trim().is_empty()).unwrap_or(DEFAULT_FONT_FAMILY).to_owned();
        // `EditorFontSize.validate`: 0 é o padrão, o resto fica entre 6 e 100.
        let font_size = match as_number(get("editor.fontSize")) {
            Some(n) if n != 0.0 => n.clamp(6.0, 100.0) as f32,
            _ => DEFAULT_FONT_SIZE,
        };
        let weight = font_weight(get("editor.fontWeight"));
        let raw_line_height = as_number(get("editor.lineHeight")).map_or(0.0, |n| n.clamp(0.0, 150.0));
        let line_height = BareFontInfo::create(&font_family, "normal", f64::from(font_size), "", "", raw_line_height, 0.0, 1.0, true).line_height;
        let word_wrap = match as_str(get("editor.wordWrap")) {
            Some("on") => WordWrap::On,
            Some("wordWrapColumn") => WordWrap::WordWrapColumn,
            Some("bounded") => WordWrap::Bounded,
            _ => WordWrap::Off,
        };
        let line_numbers = match as_str(get("editor.lineNumbers")) {
            Some("off") => LineNumbers::Off,
            Some("relative") => LineNumbers::Relative,
            Some("interval") => LineNumbers::Interval,
            _ => LineNumbers::On,
        };
        let render_whitespace = match as_str(get("editor.renderWhitespace")) {
            Some("none") => RenderWhitespace::None,
            Some("boundary") => RenderWhitespace::Boundary,
            Some("trailing") => RenderWhitespace::Trailing,
            Some("all") => RenderWhitespace::All,
            _ => RenderWhitespace::Selection,
        };
        let cursor_style = match as_str(get("editor.cursorStyle")) {
            Some("block") => CursorStyle::Block,
            Some("underline") => CursorStyle::Underline,
            Some("line-thin") => CursorStyle::LineThin,
            Some("block-outline") => CursorStyle::BlockOutline,
            Some("underline-thin") => CursorStyle::UnderlineThin,
            _ => CursorStyle::Line,
        };
        let cursor_blinking = match as_str(get("editor.cursorBlinking")) {
            Some("smooth") => CursorBlinking::Smooth,
            Some("phase") => CursorBlinking::Phase,
            Some("expand") => CursorBlinking::Expand,
            Some("solid") => CursorBlinking::Solid,
            _ => CursorBlinking::Blink,
        };
        let render_line_highlight = match as_str(get("editor.renderLineHighlight")) {
            Some("none") => RenderLineHighlight::None,
            Some("gutter") => RenderLineHighlight::Gutter,
            Some("all") => RenderLineHighlight::All,
            _ => RenderLineHighlight::Line,
        };
        let rulers = get("editor.rulers")
            .and_then(Value::as_array)
            .map(|list| {
                let mut rulers: Vec<Ruler> = list
                    .iter()
                    .filter_map(|r| match r {
                        Value::Number(n) => n.as_f64().map(|column| Ruler { column, color: None }),
                        Value::Object(o) => as_number(o.get("column")).map(|column| Ruler { column, color: o.get("color").and_then(Value::as_str).map(str::to_owned) }),
                        _ => None,
                    })
                    .map(|r| Ruler { column: r.column.max(0.0), ..r })
                    .collect();
                // `EditorRulers.validate` ordena pela coluna.
                rulers.sort_by(|a, b| a.column.total_cmp(&b.column));
                rulers
            })
            .unwrap_or_default();
        // `renderIndentGuides` é a forma antiga (`migrateOptions` a leva para `guides.indentation`).
        let indent_guides = get("editor.guides.indentation").and_then(Value::as_bool).or_else(|| get("editor.renderIndentGuides").and_then(Value::as_bool)).unwrap_or(true);
        let indent_size = match get("editor.indentSize") {
            Some(Value::String(s)) if s == "tabSize" => None,
            other => as_number(other).map(|n| (n.floor() as i32).clamp(1, 100)),
        };
        Self {
            font_family,
            font_size,
            font_weight: weight,
            line_height,
            tab_size: clamped_int(get("editor.tabSize"), 4, 1, 100),
            indent_size,
            insert_spaces: as_bool(get("editor.insertSpaces"), true),
            detect_indentation: as_bool(get("editor.detectIndentation"), true),
            word_wrap,
            word_wrap_column: clamped_int(get("editor.wordWrapColumn"), 80, 1, 1 << 30),
            line_numbers,
            minimap: as_bool(get("editor.minimap.enabled"), true),
            render_whitespace,
            cursor_style,
            cursor_blinking,
            render_line_highlight,
            rulers,
            indent_guides,
            bracket_pair_colorization: as_bool(get("editor.bracketPairColorization.enabled"), true),
            insert_final_newline: as_bool(get("files.insertFinalNewline"), false),
            trim_trailing_whitespace: as_bool(get("files.trimTrailingWhitespace"), false),
            trim_in_regex_and_strings: as_bool(get("files.trimTrailingWhitespaceInRegexAndStrings"), true),
            trim_final_newlines: as_bool(get("files.trimFinalNewlines"), false),
            editor,
        }
    }

    /// As chaves de `editor.*` (sem o prefixo) que o usuário definiu, para saber quais saíram numa troca.
    pub(crate) fn editor_keys(&self) -> impl Iterator<Item = &String> {
        self.editor.keys()
    }

    /// O objeto do `updateOptions` do widget: as opções do usuário, com a fonte e a quebra resolvidas e as que a
    /// view fixa (`lineNumbersMinChars` 3 do editor de texto do workbench). `removed` são as chaves que estavam na
    /// configuração anterior e saíram: vão como `undefined`, o que devolve o padrão (`applyUpdate`).
    pub(crate) fn editor_options(&self, removed: &[String]) -> Dynamic {
        let mut options = self.editor.clone();
        options.insert("fontFamily".to_owned(), Value::from(self.font_family.clone()));
        options.insert("fontSize".to_owned(), Value::from(f64::from(self.font_size)));
        options.insert("lineHeight".to_owned(), Value::from(self.line_height));
        options.insert("wordWrap".to_owned(), Value::from(self.word_wrap.as_str()));
        options.entry("lineNumbersMinChars").or_insert(Value::from(3.0));
        options.insert("automaticLayout".to_owned(), Value::from(false));
        let text = serde_json::to_string(&Value::Object(options)).unwrap_or_else(|_| "{}".to_owned());
        let parsed = je_base::json::parse_str(&text, &mut Vec::new()).unwrap_or(je_base::json::Value::Object(Default::default()));
        let dynamic = Dynamic::from_json(&parsed);
        if let Dynamic::Object(object) = &dynamic {
            for key in removed {
                object.set(&je_base::JsString::from(key.as_str()), Dynamic::Undefined);
            }
        }
        dynamic
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings(value: Value) -> Map<String, Value> {
        value.as_object().expect("objeto").clone()
    }

    #[test]
    fn defaults_are_the_vs_code_ones() {
        let s = EditorSettings::default();
        assert_eq!((s.font_size, s.tab_size, s.insert_spaces, s.detect_indentation), (DEFAULT_FONT_SIZE, 4, true, true));
        assert_eq!(s.line_height, crate::layout::DEFAULT_LINE_HEIGHT, "lineHeight 0 é a razão áurea do tamanho");
        assert_eq!((s.line_numbers, s.render_whitespace, s.cursor_style, s.render_line_highlight), (LineNumbers::On, RenderWhitespace::Selection, CursorStyle::Line, RenderLineHighlight::Line));
        assert!(s.minimap && s.indent_guides && s.bracket_pair_colorization && s.rulers.is_empty());
        assert!(!s.insert_final_newline && !s.trim_trailing_whitespace);
    }

    #[test]
    fn the_three_forms_of_a_key_and_validation() {
        let s = EditorSettings::resolve(
            &settings(json!({
                "editor.fontSize": 200,
                "editor.lineHeight": 1.5,
                "editor.minimap": { "enabled": false },
                "editor": { "lineNumbers": "relative", "tabSize": "2" },
                "editor.rulers": [100, { "column": 80, "color": "#ff0000" }],
                "editor.fontWeight": "bold",
            })),
            "plaintext",
        );
        assert_eq!(s.font_size, 100.0, "o tamanho fica entre 6 e 100");
        assert_eq!(s.line_height, 150.0, "abaixo de 8 a altura é múltiplo do tamanho");
        assert!(!s.minimap);
        assert_eq!((s.line_numbers, s.tab_size, s.font_weight), (LineNumbers::Relative, 2, 700));
        assert_eq!(s.rulers, vec![Ruler { column: 80.0, color: Some("#ff0000".to_owned()) }, Ruler { column: 100.0, color: None }], "as réguas ficam em ordem");
        let options = s.editor_options(&[]);
        assert!(matches!(options.get_str("minimap").get_str("enabled"), Dynamic::Bool(false)), "as opções vão para o widget sem o prefixo");
    }

    #[test]
    fn language_blocks_override_the_general_values() {
        let user = settings(json!({
            "editor.tabSize": 4,
            "files.trimTrailingWhitespace": true,
            "[rust]": { "editor.tabSize": 2, "files.trimTrailingWhitespace": false },
            "[markdown][plaintext]": { "editor.wordWrap": "on" },
        }));
        let rust = EditorSettings::resolve(&user, "rust");
        assert_eq!((rust.tab_size, rust.trim_trailing_whitespace), (2, false));
        let text = EditorSettings::resolve(&user, "plaintext");
        assert_eq!((text.tab_size, text.trim_trailing_whitespace, text.word_wrap), (4, true, WordWrap::On), "um bloco com várias linguagens vale para cada uma");
        assert_eq!(EditorSettings::resolve(&user, "toml").word_wrap, WordWrap::Off);
    }
}
