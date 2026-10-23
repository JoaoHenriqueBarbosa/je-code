//! As regras de cor de token de um tema (`ColorThemeData.tokenColors` do VS Code) e a busca da regra que
//! pinta um escopo (`findMatchingThemeRule` do `TMHelper.ts`, usada pelo "Inspect TM Scopes" e pelo
//! `ThemeDocument` das goldens de colorização).
//!
//! Divergência: o `ColorThemeData` completo (cores do workbench, semantic token colors, `tokenFontMap`) mora
//! no `je-theme`; aqui só entra a parte que o tokenizador TextMate consome.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::jsonc;

/// `ColorThemeType`, só o que o `ColorThemeData` usa para os padrões de editor e de token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeType {
    Light,
    Dark,
    HcDark,
    HcLight,
}

impl ThemeType {
    /// `uiTheme` do `package.json` (`vs`, `vs-dark`, `hc-black`, `hc-light`).
    pub fn from_ui_theme(ui_theme: &str) -> Self {
        match ui_theme {
            "vs" => Self::Light,
            "hc-black" => Self::HcDark,
            "hc-light" => Self::HcLight,
            _ => Self::Dark,
        }
    }

    fn editor_foreground(self) -> &'static str {
        match self {
            Self::Light => "#333333",
            Self::Dark => "#BBBBBB",
            Self::HcDark => "#FFFFFF",
            Self::HcLight => "#292929",
        }
    }

    fn editor_background(self) -> &'static str {
        match self {
            Self::Light | Self::HcLight => "#FFFFFF",
            Self::Dark => "#1E1E1E",
            Self::HcDark => "#000000",
        }
    }

    /// `defaultThemeColors` de tokens (`token.info-token`...).
    fn default_token_rules(self) -> [(&'static str, &'static str); 4] {
        let (info, warn, error, debug) = match self {
            Self::Light | Self::HcLight => ("#316bcd", "#cd9731", "#cd3131", "#800080"),
            Self::Dark => ("#6796e6", "#cd9731", "#f44747", "#b267e6"),
            Self::HcDark => ("#6796e6", "#008000", "#FF0000", "#b267e6"),
        };
        [("token.info-token", info), ("token.warn-token", warn), ("token.error-token", error), ("token.debug-token", debug)]
    }
}

/// `normalizeColor` do `colorThemeData.ts`: `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA` em maiúsculas.
fn normalize_color(color: &str) -> Option<String> {
    let digits = color.strip_prefix('#')?;
    if ![3, 4, 6, 8].contains(&digits.len()) || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = String::from("#");
    for c in digits.chars() {
        out.push(c.to_ascii_uppercase());
        if digits.len() <= 4 {
            out.push(c.to_ascii_uppercase());
        }
    }
    if out.len() == 9 && out.ends_with("FF") {
        out.truncate(7);
    }
    Some(out)
}

#[derive(Clone)]
struct Rule {
    scope: Option<Value>,
    foreground: Option<String>,
    background: Option<String>,
    font_style: Option<String>,
}

/// As regras de cor de token de um tema.
#[derive(Clone)]
pub struct TokenTheme {
    /// O identificador do tema (o nome do arquivo sem extensão).
    pub id: String,
    label: String,
    rules: Vec<Rule>,
    cache: HashMap<String, Option<String>>,
}

#[derive(Default)]
struct Loaded {
    colors: BTreeMap<String, String>,
    token_colors: Vec<Value>,
}

/// `loadColorTheme`: segue os `include` e junta as cores e as regras (a do arquivo incluído vem antes).
fn load_file(path: &Path, result: &mut Loaded) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let value = jsonc::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if let Some(include) = value["include"].as_str() {
        load_file(&path.parent().unwrap_or(Path::new(".")).join(include), result)?;
    }
    for (id, color) in value["colors"].as_object().into_iter().flatten() {
        match color.as_str() {
            Some("default") => {
                result.colors.remove(id);
            }
            Some(c) => {
                result.colors.insert(id.clone(), c.to_owned());
            }
            None => {}
        }
    }
    match &value["tokenColors"] {
        Value::Array(rules) => result.token_colors.extend(rules.iter().cloned()),
        Value::Null => {}
        other => return Err(format!("{}: tokenColors fora do formato suportado: {other}", path.display())),
    }
    Ok(())
}

fn truthy_scope(scope: &Value) -> bool {
    !matches!(scope, Value::Null) && scope.as_str() != Some("")
}

impl TokenTheme {
    /// Lê o tema de `extension_dir` descrito por uma entrada de `contributes.themes` (`path` e `uiTheme`).
    pub fn load(extension_dir: &Path, entry: &Value) -> Result<Self, String> {
        let relative = entry["path"].as_str().ok_or("tema sem path")?;
        let path = extension_dir.join(relative);
        let stem = path.file_stem().and_then(|s| s.to_str()).ok_or("path de tema inválido")?.to_owned();
        Self::load_file(&path, ThemeType::from_ui_theme(entry["uiTheme"].as_str().unwrap_or("vs-dark")), &stem)
    }

    /// Lê o arquivo de tema em `path`.
    pub fn load_file(path: &Path, kind: ThemeType, id: &str) -> Result<Self, String> {
        let mut loaded = Loaded::default();
        load_file(path, &mut loaded)?;

        let color_of = |id: &str, default: &str| normalize_color(loaded.colors.get(id).map_or(default, String::as_str));
        let mut rules = vec![Rule {
            scope: None,
            foreground: color_of("editor.foreground", kind.editor_foreground()),
            background: color_of("editor.background", kind.editor_background()),
            font_style: None,
        }];
        let has_default_tokens =
            loaded.token_colors.iter().any(|r| r["scope"].as_str() == Some("token.info-token") && !r["settings"].is_null());
        let mut add_rule = |scope: &Value, settings: &Value| {
            if !truthy_scope(scope) || settings.is_null() {
                return;
            }
            let color = |key: &str| settings[key].as_str().and_then(normalize_color);
            rules.push(Rule {
                scope: Some(scope.clone()),
                foreground: color("foreground"),
                background: color("background"),
                font_style: settings["fontStyle"].as_str().map(str::to_owned),
            });
        };
        for rule in &loaded.token_colors {
            add_rule(&rule["scope"], &rule["settings"]);
        }
        if !has_default_tokens {
            for (scope, foreground) in kind.default_token_rules() {
                add_rule(&json!(scope), &json!({ "foreground": foreground }));
            }
        }
        Ok(Self { id: id.to_owned(), label: id.to_owned(), rules, cache: HashMap::new() })
    }

    /// O tema no formato que o `vscode-textmate` recebe (`IRawTheme`).
    pub fn raw_theme(&self) -> Value {
        let settings: Vec<Value> = self
            .rules
            .iter()
            .map(|r| {
                let mut style = json!({ "foreground": r.foreground, "background": r.background });
                if let Some(f) = &r.font_style {
                    style["fontStyle"] = json!(f);
                }
                match &r.scope {
                    Some(scope) => json!({ "scope": scope, "settings": style }),
                    None => json!({ "settings": style }),
                }
            })
            .collect();
        json!({ "name": self.label, "settings": settings })
    }

    /// `ThemeDocument.explainTokenColor`: o seletor da regra vencedora e a cor, em maiúsculas.
    pub fn explain(&mut self, scopes: &str, color: &str) -> String {
        if !self.cache.contains_key(scopes) {
            let found = self.find_matching_rule(&scopes.split(' ').collect::<Vec<_>>());
            self.cache.insert(scopes.to_owned(), found);
        }
        let selector = self.cache[scopes].as_deref().unwrap_or("default");
        format!("{selector}: {}", color.to_uppercase())
    }

    /// `findMatchingThemeRule(theme, scopes)`: o `rawSelector` da regra que vence.
    pub fn find_matching_rule(&self, scopes: &[&str]) -> Option<String> {
        (0..scopes.len()).rev().find_map(|i| self.find_for_scope(scopes[i], &scopes[..i]))
    }

    fn find_for_scope(&self, scope: &str, parents: &[&str]) -> Option<String> {
        let mut best: Option<ThemeRule> = None;
        // Loop backwards, to ensure the last most specific rule wins
        for rule in self.rules.iter().rev().filter(|r| r.foreground.as_deref().is_some_and(|f| !f.is_empty())) {
            let selectors: Vec<String> = match &rule.scope {
                Some(Value::String(s)) => s.split(',').map(|p| p.trim().to_owned()).collect(),
                Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
                _ => continue,
            };
            for raw in selectors {
                let candidate = ThemeRule::new(raw);
                if candidate.matches(scope, parents) && candidate.is_more_specific(best.as_ref()) {
                    best = Some(candidate);
                }
            }
        }
        best.map(|b| b.raw_selector)
    }
}

/// `ThemeRule` do `TMHelper.ts`.
struct ThemeRule {
    raw_selector: String,
    scope: String,
    parent_scopes: Vec<String>,
}

impl ThemeRule {
    fn new(raw_selector: String) -> Self {
        let mut pieces: Vec<String> = raw_selector.split(' ').map(str::to_owned).collect();
        let scope = pieces.pop().unwrap_or_default();
        Self { raw_selector, scope, parent_scopes: pieces }
    }

    fn matches_one(selector_scope: &str, scope: &str) -> bool {
        selector_scope == scope || scope.strip_prefix(selector_scope).is_some_and(|rest| rest.starts_with('.'))
    }

    fn matches(&self, scope: &str, parent_scopes: &[&str]) -> bool {
        if !Self::matches_one(&self.scope, scope) {
            return false;
        }
        let mut selector_parent_index = self.parent_scopes.len() as isize - 1;
        let mut parent_scope_index = parent_scopes.len() as isize - 1;
        while selector_parent_index >= 0 && parent_scope_index >= 0 {
            if Self::matches_one(&self.parent_scopes[selector_parent_index as usize], parent_scopes[parent_scope_index as usize]) {
                selector_parent_index -= 1;
            }
            parent_scope_index -= 1;
        }
        selector_parent_index == -1
    }

    /// `isMoreSpecific(other)`, com os comprimentos em unidades UTF-16 como o `String.length` do original.
    fn is_more_specific(&self, other: Option<&Self>) -> bool {
        let len = |s: &str| s.encode_utf16().count() as i64;
        let Some(b) = other else { return true };
        if len(&self.scope) != len(&b.scope) {
            return len(&self.scope) > len(&b.scope);
        }
        if self.parent_scopes.len() != b.parent_scopes.len() {
            return self.parent_scopes.len() > b.parent_scopes.len();
        }
        for (x, y) in self.parent_scopes.iter().zip(&b.parent_scopes) {
            if len(x) != len(y) {
                return len(x) > len(y);
            }
        }
        false
    }
}
