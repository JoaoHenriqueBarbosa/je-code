//! Porte de `src/vs/workbench/services/themes/common/colorThemeData.ts`: o `ColorThemeData`, o tema de cor do
//! workbench com as cores do arquivo (`colorMap`), as customizações do usuário (`customColorMap`), as cores
//! transientes (`transientColorMap`), as regras de token do TextMate (`tokenColors`) e a persistência
//! (`toStorage`, `fromStorageData`).
//!
//! Reaproveitado, não duplicado:
//! - O mapa de cores e a resolução (`getColor`, `defines`, `getDefault`, `getColorCustomization`,
//!   `setTransientColors`) são do [`je_theme::ColorTheme`], que ganhou `unloaded`, `colors`, `has_transient_colors`
//!   e `get_color_customization`.
//! - `convertSettings` e `ITextMateThemingRule` vêm de `theme_compatibility`.
//! - `normalizeColor` (para string), `defaultThemeColors` e o `getThemeTypeSelector` são de
//!   `je_theme::theme_defaults`, que `je-languages/src/token_theme.rs` também usa.
//!
//! Fora deste porte (dependem de módulos ainda sem porte):
//! - Os semantic tokens: `semanticTokenRules`, `customSemanticTokenRules`, `getTokenStyle`, `resolveTokenStyleValue`,
//!   `getTokenStyleMetadata`, `getTokenStylingRuleScope`, `setCustomSemanticTokenColors`, `readSemanticTokenRule` e
//!   o `customSemanticHighlighting` do getter `semanticHighlighting`, todos do `tokenClassificationRegistry`.
//! - `resolveScopes`, `findMetadata`, `getTokenColorIndex`, `tokenColorMap`, `getTokenFontIndex` e `tokenFontMap`
//!   (usam `TokenStyle`, `getTokenStylingDefaultRules` e `IFontTokenOptions`).
//! - O carregamento de arquivo (`ensureLoaded`, `reload`, `_loadColorTheme`, `_loadSyntaxTokens`), que usa o
//!   `IExtensionResourceLoaderService`, e `fromExtensionTheme` (`IThemeExtensionPoint`, `ExtensionData`, `URI`),
//!   assim como `location`, `extensionData` e `setCustomizations` (`ThemeConfiguration`).
//!
//! Divergências:
//! - A ordem das chaves do `colorMap` gravado no armazenamento é a alfabética (o `ColorTheme` guarda o mapa sem
//!   ordem de inserção), para o texto gravado ser determinístico.
//! - `semanticTokenRules` é gravado como `[]` e ignorado na leitura, porque o tipo não existe ainda.
//! - `getBaseTheme` sem cores transientes devolve uma cópia (o original devolve o próprio objeto).
//! - `tokenColors` devolve o `Rc` do cache, para o chamador comparar a identidade (o `!==` do original) com
//!   `Rc::ptr_eq`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;
use je_theme::color::Color;
use je_theme::color::format::css::format_hex_a;
use je_theme::theme_defaults::normalize_color;
use je_theme::{ColorTheme, ThemeType};
use serde_json::{Map, Value};

use super::theme_compatibility::{TextMateScope, TextMateThemingRule};

/// `THEME_SCOPE_OPEN_PAREN`, `THEME_SCOPE_CLOSE_PAREN` e `THEME_SCOPE_WILDCARD` do `workbenchThemeService.ts`.
const THEME_SCOPE_OPEN_PAREN: char = '[';
const THEME_SCOPE_CLOSE_PAREN: char = ']';
const THEME_SCOPE_WILDCARD: char = '*';

const EDITOR_FOREGROUND: &str = "editor.foreground";
const EDITOR_BACKGROUND: &str = "editor.background";

/// `tokenGroupToScopesMap`, na ordem de declaração.
const TOKEN_GROUP_TO_SCOPES: [(&str, &[&str]); 7] = [
    ("comments", &["comment", "punctuation.definition.comment"]),
    ("strings", &["string", "meta.embedded.assembly"]),
    ("keywords", &["keyword - keyword.operator", "keyword.control", "storage", "storage.type"]),
    ("numbers", &["constant.numeric"]),
    ("types", &["entity.name.type", "entity.name.class", "support.type", "support.class"]),
    ("functions", &["entity.name.function", "support.function"]),
    ("variables", &["variable", "entity.name.variable"]),
];

/// `normalizeColor` para uma `Color`: passa por `Color.Format.CSS.formatHexA(color, true)`.
fn normalize_color_value(color: &Color) -> Option<String> {
    normalize_color(&format_hex_a(color, true))
}

/// A veracidade (`if (value)`) de um valor JSON em JavaScript.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `ITextMateThemingRule` a partir do JSON (uma regra do tema ou de `textMateRules`). `None` se não for um objeto.
fn rule_from_json(value: &Value) -> Option<TextMateThemingRule> {
    let object = value.as_object()?;
    let scope = match object.get("scope") {
        Some(Value::String(scope)) => Some(TextMateScope::One(scope.clone())),
        Some(Value::Array(scopes)) => {
            Some(TextMateScope::Many(scopes.iter().filter_map(|scope| scope.as_str().map(str::to_owned)).collect()))
        }
        _ => None,
    };
    let settings = object.get("settings").and_then(Value::as_object).map(|settings| {
        settings.iter().map(|(key, value)| (key.clone(), value.clone())).collect::<IndexMap<String, Value>>()
    });
    Some(TextMateThemingRule { name: object.get("name").and_then(Value::as_str).map(str::to_owned), scope, settings })
}

/// A regra como é gravada no armazenamento: só `settings` e `scope` (os nomes não persistem).
fn rule_to_json(rule: &TextMateThemingRule) -> Value {
    let mut object = Map::new();
    if let Some(settings) = &rule.settings {
        object.insert("settings".into(), Value::Object(settings.iter().map(|(k, v)| (k.clone(), v.clone())).collect()));
    }
    match &rule.scope {
        Some(TextMateScope::One(scope)) => {
            object.insert("scope".into(), Value::String(scope.clone()));
        }
        Some(TextMateScope::Many(scopes)) => {
            object.insert("scope".into(), Value::Array(scopes.iter().cloned().map(Value::String).collect()));
        }
        None => {}
    }
    Value::Object(object)
}

/// `key.match(themeScopeRegex)` com `themeScopeRegex = /\[(.+?)\]/g`: os trechos `[...]` com ao menos um caractere
/// dentro (o `.` não casa com quebra de linha), cada um com os colchetes.
fn theme_scope_matches(key: &str) -> Vec<String> {
    let chars: Vec<char> = key.chars().collect();
    let is_line_terminator = |c: char| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}');
    let mut matches = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == THEME_SCOPE_OPEN_PAREN {
            let mut j = i + 1;
            let mut end = None;
            while j < chars.len() && !is_line_terminator(chars[j]) {
                if j >= i + 2 && chars[j] == THEME_SCOPE_CLOSE_PAREN {
                    end = Some(j);
                    break;
                }
                j += 1;
            }
            if let Some(end) = end {
                matches.push(chars[i..=end].iter().collect::<String>());
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    matches
}

/// O tema de cor do workbench (parte portada, ver o cabeçalho do módulo).
#[derive(Clone)]
pub struct ColorThemeData {
    id: String,
    pub label: String,
    pub settings_id: String,
    pub description: Option<String>,
    pub is_loaded: bool,
    pub watch: Option<bool>,

    theme_semantic_highlighting: Option<bool>,
    custom_semantic_highlighting_deprecated: Option<bool>,

    theme_token_colors: Vec<TextMateThemingRule>,
    custom_token_colors: Vec<TextMateThemingRule>,
    /// `colorMap`, `customColorMap` e `transientColorMap`.
    colors: ColorTheme,

    /// `textMateThemingRules`, criado sob demanda.
    text_mate_theming_rules: RefCell<Option<Rc<Vec<TextMateThemingRule>>>>,
}

/// O `type` do tema pelo primeiro nome de classe do `id` (`themeTypeSelector`).
fn theme_type_of_id(id: &str) -> ThemeType {
    ThemeType::from_ui_theme(id.split(' ').next().unwrap_or(""))
}

impl ColorThemeData {
    pub const STORAGE_KEY: &'static str = "colorThemeData";

    fn new(id: &str, label: &str, settings_id: &str) -> Self {
        Self {
            id: id.to_owned(),
            label: label.to_owned(),
            settings_id: settings_id.to_owned(),
            description: None,
            is_loaded: false,
            watch: None,
            theme_semantic_highlighting: None,
            custom_semantic_highlighting_deprecated: None,
            theme_token_colors: Vec::new(),
            custom_token_colors: Vec::new(),
            colors: ColorTheme::unloaded(label, theme_type_of_id(id), HashMap::new()),
            text_mate_theming_rules: RefCell::new(None),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// `classNames`: o `id` separado por espaços.
    pub fn class_names(&self) -> Vec<&str> {
        self.id.split(' ').collect()
    }

    /// `semanticHighlighting`, sem o `customSemanticHighlighting` (de `setCustomSemanticTokenColors`, sem porte):
    /// o `semanticHighlighting` deprecado do usuário vence o do tema.
    pub fn semantic_highlighting(&self) -> bool {
        self.custom_semantic_highlighting_deprecated.or(self.theme_semantic_highlighting).unwrap_or(false)
    }

    /// `themeTypeSelector`: a primeira classe do `id` (`vs`, `vs-dark`, `hc-black`, `hc-light`).
    pub fn theme_type_selector(&self) -> &str {
        self.class_names()[0]
    }

    /// `type` (o `ColorScheme`).
    pub fn theme_type(&self) -> ThemeType {
        theme_type_of_id(&self.id)
    }

    /// `tokenColors`: a regra padrão (sem escopo) é sempre a primeira, depois as regras do tema, as do usuário
    /// e, sem `token.info-token`, as cores `token.*-token` do tipo do tema.
    pub fn token_colors(&self) -> Rc<Vec<TextMateThemingRule>> {
        if let Some(rules) = self.text_mate_theming_rules.borrow().as_ref() {
            return rules.clone();
        }
        let mut result: Vec<TextMateThemingRule> = Vec::new();

        // the default rule (scope empty) is always the first rule. Ignore all other default rules.
        let foreground = self.get_color(EDITOR_FOREGROUND, true).or_else(|| self.get_default(EDITOR_FOREGROUND));
        let background = self.get_color(EDITOR_BACKGROUND, true).or_else(|| self.get_default(EDITOR_BACKGROUND));
        let mut settings = IndexMap::new();
        if let Some(foreground) = foreground.as_ref().and_then(normalize_color_value) {
            settings.insert("foreground".to_owned(), Value::String(foreground));
        }
        if let Some(background) = background.as_ref().and_then(normalize_color_value) {
            settings.insert("background".to_owned(), Value::String(background));
        }
        result.push(TextMateThemingRule { name: None, scope: None, settings: Some(settings) });

        let mut has_default_tokens = false;
        for rule in &self.theme_token_colors {
            add_rule(&mut result, &mut has_default_tokens, rule);
        }
        // Add the custom colors after the theme colors
        // so that they will override them
        for rule in &self.custom_token_colors {
            add_rule(&mut result, &mut has_default_tokens, rule);
        }
        if !has_default_tokens {
            for (scope, foreground) in self.theme_type().default_token_rules() {
                let mut settings = IndexMap::new();
                settings.insert("foreground".to_owned(), Value::String(foreground.to_owned()));
                let rule = TextMateThemingRule {
                    name: None,
                    scope: Some(TextMateScope::One(scope.to_owned())),
                    settings: Some(settings),
                };
                add_rule(&mut result, &mut has_default_tokens, &rule);
            }
        }
        let result = Rc::new(result);
        *self.text_mate_theming_rules.borrow_mut() = Some(result.clone());
        result
    }

    /// `getColor(colorId, useDefault)`.
    pub fn get_color(&self, color_id: &str, use_default: bool) -> Option<Color> {
        self.colors.resolve_color(color_id, use_default)
    }

    /// `getDefault`.
    pub fn get_default(&self, color_id: &str) -> Option<Color> {
        self.colors.get_default(color_id)
    }

    /// `defines`.
    pub fn defines(&self, color_id: &str) -> bool {
        self.colors.defines(color_id)
    }

    /// `getColorCustomization`.
    pub fn get_color_customization(&self, color_id: &str) -> Option<Color> {
        self.colors.get_color_customization(color_id).cloned()
    }

    /// Replaces runtime-only colors; these are deliberately excluded from theme storage.
    pub fn set_transient_colors(&mut self, colors: Option<HashMap<String, Color>>) {
        if self.colors.has_transient_colors() || colors.is_some() {
            self.colors.set_transient_colors(colors);
            self.clear_caches();
        }
    }

    /// `getBaseTheme`: o tema sem as cores transientes (as do início, sem a tinta).
    pub fn get_base_theme(&self) -> ColorThemeData {
        let mut theme = self.clone();
        theme.set_transient_colors(None);
        theme
    }

    /// `setCustomColors`: as customizações de cor, sem tema e depois as específicas do tema (estas vencem).
    pub fn set_custom_colors(&mut self, colors: &Map<String, Value>) {
        let mut custom: IndexMap<String, String> = IndexMap::new();
        Self::overwrite_custom_colors(&mut custom, colors);
        if let Some(theme_specific) = self.get_theme_specific_colors(colors) {
            Self::overwrite_custom_colors(&mut custom, &theme_specific);
        }
        self.colors.set_custom_colors(custom);
        self.clear_caches();
    }

    fn overwrite_custom_colors(custom: &mut IndexMap<String, String>, colors: &Map<String, Value>) {
        for (id, value) in colors {
            if let Value::String(text) = value {
                custom.insert(id.clone(), text.clone());
            }
        }
    }

    /// `setCustomTokenColors`.
    pub fn set_custom_token_colors(&mut self, custom_token_colors: &Map<String, Value>) {
        self.custom_token_colors = Vec::new();
        self.custom_semantic_highlighting_deprecated = None;

        // first add the non-theme specific settings
        self.add_custom_token_colors(custom_token_colors);

        // append theme specific settings. Last rules will win.
        if let Some(theme_specific) = self.get_theme_specific_colors(custom_token_colors) {
            self.add_custom_token_colors(&theme_specific);
        }
        self.clear_caches();
    }

    fn add_custom_token_colors(&mut self, custom_token_colors: &Map<String, Value>) {
        // Put the general customizations such as comments, strings, etc. first so that
        // they can be overridden by specific customizations like "string.interpolated"
        for (group, scopes) in TOKEN_GROUP_TO_SCOPES {
            let Some(value) = custom_token_colors.get(group).filter(|value| is_truthy(value)) else { continue };
            let settings: IndexMap<String, Value> = match value {
                Value::String(foreground) => {
                    IndexMap::from([("foreground".to_owned(), Value::String(foreground.clone()))])
                }
                Value::Object(settings) => settings.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                _ => continue,
            };
            for scope in scopes {
                self.custom_token_colors.push(TextMateThemingRule {
                    name: None,
                    scope: Some(TextMateScope::One((*scope).to_owned())),
                    settings: Some(settings.clone()),
                });
            }
        }

        // specific customizations
        if let Some(Value::Array(rules)) = custom_token_colors.get("textMateRules") {
            for rule in rules.iter().filter_map(rule_from_json) {
                if rule.scope.as_ref().is_some_and(TextMateScope::is_truthy) && rule.settings.is_some() {
                    self.custom_token_colors.push(rule);
                }
            }
        }
        if let Some(Value::Bool(flag)) = custom_token_colors.get("semanticHighlighting") {
            self.custom_semantic_highlighting_deprecated = Some(*flag);
        }
    }

    /// `isThemeScope`: a chave é `[...]`.
    pub fn is_theme_scope(&self, key: &str) -> bool {
        key.starts_with(THEME_SCOPE_OPEN_PAREN) && key.ends_with(THEME_SCOPE_CLOSE_PAREN)
    }

    /// `isThemeScopeMatch`: o id do escopo (com `*` no começo e/ou no fim como curinga) casa com o `settingsId`.
    pub fn is_theme_scope_match(&self, theme_id: &str) -> bool {
        let chars: Vec<char> = theme_id.chars().collect();
        let (Some(first), Some(last)) = (chars.first().copied(), chars.last().copied()) else {
            return theme_id == self.settings_id;
        };
        let prefix: String = chars[..chars.len() - 1].iter().collect();
        let infix: String = chars.get(1..chars.len() - 1).unwrap_or(&[]).iter().collect();
        let suffix: String = chars[1..].iter().collect();
        theme_id == self.settings_id
            || (self.settings_id.contains(&infix) && first == THEME_SCOPE_WILDCARD && last == THEME_SCOPE_WILDCARD)
            || (self.settings_id.starts_with(&prefix) && last == THEME_SCOPE_WILDCARD)
            || (self.settings_id.ends_with(&suffix) && first == THEME_SCOPE_WILDCARD)
    }

    /// `getThemeSpecificColors`: junta as customizações das chaves `[tema]` que casam com este tema.
    pub fn get_theme_specific_colors(&self, colors: &Map<String, Value>) -> Option<Map<String, Value>> {
        let mut theme_specific_colors: Option<Map<String, Value>> = None;
        for (key, scoped_colors) in colors {
            let Value::Object(scoped_colors) = scoped_colors else { continue };
            if !self.is_theme_scope(key) {
                continue;
            }
            for theme_scope in theme_scope_matches(key) {
                let inner: Vec<char> = theme_scope.chars().collect();
                let theme_id: String = inner[1..inner.len() - 1].iter().collect();
                if !self.is_theme_scope_match(&theme_id) {
                    continue;
                }
                let merged = theme_specific_colors.get_or_insert_with(Map::new);
                for (subkey, override_colors) in scoped_colors {
                    match (merged.get(subkey), override_colors) {
                        (Some(Value::Array(original)), Value::Array(overriding)) => {
                            let joined = original.iter().chain(overriding).cloned().collect();
                            merged.insert(subkey.clone(), Value::Array(joined));
                        }
                        (_, overriding) if is_truthy(overriding) => {
                            merged.insert(subkey.clone(), overriding.clone());
                        }
                        _ => {}
                    }
                }
            }
        }
        theme_specific_colors
    }

    /// `clearCaches`, só com os caches que existem neste porte.
    pub fn clear_caches(&mut self) {
        *self.text_mate_theming_rules.borrow_mut() = None;
    }

    /// `toStorage`: grava o tema no perfil, sem as customizações (vêm das configurações) e sem as cores transientes.
    pub fn to_storage(&self, storage_service: &dyn StorageService) {
        let mut entries: Vec<(&String, String)> =
            self.colors.colors().iter().map(|(key, color)| (key, format_hex_a(color, true))).collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        let color_map_data: Map<String, Value> =
            entries.into_iter().map(|(key, hex)| (key.clone(), Value::String(hex))).collect();

        // no need to persist custom colors, they will be taken from the settings
        let mut value = Map::new();
        value.insert("id".into(), Value::String(self.id.clone()));
        value.insert("label".into(), Value::String(self.label.clone()));
        value.insert("settingsId".into(), Value::String(self.settings_id.clone()));
        // don't persist names
        value.insert("themeTokenColors".into(), Value::Array(self.theme_token_colors.iter().map(rule_to_json).collect()));
        value.insert("semanticTokenRules".into(), Value::Array(Vec::new()));
        if let Some(flag) = self.theme_semantic_highlighting {
            value.insert("themeSemanticHighlighting".into(), Value::Bool(flag));
        }
        value.insert("colorMap".into(), Value::Object(color_map_data));
        if let Some(watch) = self.watch {
            value.insert("watch".into(), Value::Bool(watch));
        }

        // roam persisted color theme colors. Don't enable for icons as they contain references to fonts and images.
        storage_service.store(
            Self::STORAGE_KEY,
            StorageValue::String(Value::Object(value).to_string()),
            StorageScope::Profile,
            StorageTarget::User,
        );
    }

    /// `createUnloadedThemeForThemeType`.
    pub fn create_unloaded_theme_for_theme_type(theme_type: ThemeType, color_map: &[(&str, &str)]) -> ColorThemeData {
        Self::create_unloaded_theme(theme_type.ui_theme(), color_map)
    }

    /// `createUnloadedTheme`.
    pub fn create_unloaded_theme(id: &str, color_map: &[(&str, &str)]) -> ColorThemeData {
        let mut theme_data = Self::new(id, "", &format!("__{id}"));
        theme_data.is_loaded = false;
        theme_data.theme_token_colors = Vec::new();
        theme_data.watch = Some(false);
        theme_data.colors = ColorTheme::unloaded(
            "",
            theme_type_of_id(id),
            color_map.iter().map(|(color_id, hex)| ((*color_id).to_owned(), Color::from_hex(hex))).collect(),
        );
        theme_data
    }

    /// `createLoadedEmptyTheme`.
    pub fn create_loaded_empty_theme(id: &str, settings_id: &str) -> ColorThemeData {
        let mut theme_data = Self::new(id, "", settings_id);
        theme_data.is_loaded = true;
        theme_data.theme_token_colors = Vec::new();
        theme_data.watch = Some(false);
        theme_data
    }

    /// `fromStorageData`: o tema gravado por `to_storage`, ou `None` se não há, não é válido ou falta `id` ou
    /// `settingsId`.
    pub fn from_storage_data(storage_service: &dyn StorageService) -> Option<ColorThemeData> {
        let input = storage_service.get(Self::STORAGE_KEY, StorageScope::Profile, None).filter(|input| !input.is_empty())?;
        let data: Value = serde_json::from_str(&input).ok()?;
        let data = data.as_object()?;

        let text = |key: &str| data.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
        let (id, label, settings_id) = (text("id"), text("label"), text("settingsId"));
        if id.is_empty() || settings_id.is_empty() {
            return None;
        }
        let mut theme = ColorThemeData::new(&id, &label, &settings_id);
        let mut color_map: HashMap<String, Color> = HashMap::new();
        for (key, value) in data {
            match key.as_str() {
                "colorMap" => {
                    for (color_id, hex) in value.as_object().into_iter().flatten() {
                        color_map.insert(color_id.clone(), Color::from_hex(hex.as_str().unwrap_or("")));
                    }
                }
                "themeTokenColors" => {
                    theme.theme_token_colors = value.as_array().into_iter().flatten().filter_map(rule_from_json).collect();
                }
                "watch" => theme.watch = value.as_bool(),
                "themeSemanticHighlighting" => theme.theme_semantic_highlighting = value.as_bool(),
                // semanticTokenRules: sem porte; location: no longer restore; extensionData: sem porte
                _ => {}
            }
        }
        theme.colors = ColorTheme::unloaded(label, theme_type_of_id(&id), color_map);
        Some(theme)
    }
}

/// O `addRule` interno de `tokenColors`: normaliza as cores e marca se o tema define `token.info-token`.
fn add_rule(result: &mut Vec<TextMateThemingRule>, has_default_tokens: &mut bool, rule: &TextMateThemingRule) {
    let (Some(scope), Some(rule_settings)) = (&rule.scope, &rule.settings) else { return };
    if !scope.is_truthy() {
        return;
    }
    if matches!(scope, TextMateScope::One(scope) if scope == "token.info-token") {
        *has_default_tokens = true;
    }
    let mut settings = IndexMap::new();
    for key in ["foreground", "background"] {
        if let Some(color) = rule_settings.get(key).and_then(Value::as_str).and_then(normalize_color) {
            settings.insert(key.to_owned(), Value::String(color));
        }
    }
    for key in ["fontStyle", "fontSize", "fontFamily", "lineHeight"] {
        if let Some(value) = rule_settings.get(key).filter(|value| !value.is_null()) {
            settings.insert(key.to_owned(), value.clone());
        }
    }
    result.push(TextMateThemingRule { name: None, scope: Some(scope.clone()), settings: Some(settings) });
}
