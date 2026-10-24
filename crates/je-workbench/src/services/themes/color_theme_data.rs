//! Porte de `src/vs/workbench/services/themes/common/colorThemeData.ts`: o `ColorThemeData`, o tema de cor do
//! workbench com as cores do arquivo (`colorMap`), as customizações do usuário (`customColorMap`), as cores
//! transientes (`transientColorMap`), as regras de token do TextMate (`tokenColors`), as regras de semantic tokens
//! (`semanticTokenRules`, `customSemanticTokenRules`), a resolução de estilo de token (`getTokenStyle`,
//! `resolveScopes`, `getTokenStyleMetadata`, os índices de cor e de fonte), o carregamento do arquivo do tema e a
//! persistência (`toStorage`, `fromStorageData`).
//!
//! Reaproveitado, não duplicado:
//! - O mapa de cores e a resolução (`getColor`, `defines`, `getDefault`, `getColorCustomization`,
//!   `setTransientColors`) são do [`je_theme::ColorTheme`], que ganhou `unloaded`, `colors`, `set_colors`,
//!   `has_transient_colors` e `get_color_customization`.
//! - `convertSettings` e `ITextMateThemingRule` vêm de `theme_compatibility`; `createMatchers` de
//!   `text_mate_scope_matcher`; o parser de `.tmTheme` de `plist_parser`.
//! - `TokenStyle`, `SemanticTokenRule`, `TokenStyleValue`, `parseClassifierString` e o registro de classificação
//!   são de `je_theme::token_classification_registry`.
//! - `normalizeColor` (para string), `defaultThemeColors` e o `getThemeTypeSelector` são de
//!   `je_theme::theme_defaults`, que `je-languages/src/token_theme.rs` também usa.
//! - `MetadataConsts`, `FontStyle`, `ColorId`, `toStandardTokenType` e `IFontTokenOptions` são de `je-editor`.
//! - O contrato `IExtensionResourceLoaderService` é o de `contrib::snippets::snippets_file` (migra para a
//!   plataforma quando o serviço for portado).
//!
//! Divergências:
//! - A ordem das chaves do `colorMap` gravado no armazenamento é a alfabética (o `ColorTheme` guarda o mapa sem
//!   ordem de inserção), para o texto gravado ser determinístico.
//! - `getBaseTheme` sem cores transientes devolve uma cópia (o original devolve o próprio objeto).
//! - `tokenColors` devolve o `Rc` do cache, para o chamador comparar a identidade (o `!==` do original) com
//!   `Rc::ptr_eq`.
//! - `ensureLoaded`, `reload` e `load` são `async fn` que esperam a `Promise` do carregador; o recurso é lido
//!   pelo [`IExtensionResourceLoaderService`] recebido como argumento (no original, o serviço é o parâmetro).
//! - `tokenColorMap` e `tokenFontMap` devolvem `Vec<Option<_>>`: o índice 0 é o buraco do array esparso do original
//!   (o primeiro id é 1). O `TokenFontIndex.get` (que busca a fonte por identidade de objeto, e por isso nunca
//!   acha uma fonte recém-criada) não existe; o `console.log` de `TokenColorIndex.get` para cor ausente também não.
//! - `getTokenStyle` e `getTokenStyleMetadata` recebem as `definitions` como `Option<&mut ...>` (o original usa um
//!   objeto de saída com padrão `{}`); as definições guardam cópias da regra, dos escopos ou do valor.
//! - `getTokenStylingRuleScope` compara a identidade da regra com `std::ptr::eq` sobre as listas do próprio tema
//!   ([`ColorThemeData::semantic_token_rules`] e [`ColorThemeData::custom_semantic_token_rules`]).
//! - `parseTokenSelector` nunca lança (seletor inválido vira `$invalid`), então o `catch` de `readSemanticTokenRules`
//!   e a rejeição `error.invalidformat.semanticTokenColors` do carregamento não têm como acontecer.
//! - Valores de `colors`, `tokenColors` e `semanticTokenColors` que não são objeto ou array (os `for ... in` do
//!   original sobre texto ou sobre os índices de um array) são ignorados; itens de `tokenColors` que não são objeto
//!   são descartados.
//! - O JSON do tema é lido com o `parse` de `je_base::json` (comentários e vírgula final aceitos, como o
//!   `Json.parse` do original) e convertido para `serde_json::Value`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::errors::Error;
use je_base::json::{self as js_json, NodeType, ParseOptions};
use je_base::json_error_messages::parse_error_message;
use je_base::nls::localize;
use je_base::path::{Environment, PathModule, ProcessEnvironment};
use je_base::uri::URI;
use je_editor::encoded_token_attributes::{color_id, font_style, metadata_consts};
use je_editor::token_theme::{IFontTokenOptions, to_standard_token_type};
use je_platform::resources::ext_uri;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;
use je_theme::color::Color;
use je_theme::color::format::css::format_hex_a;
use je_theme::registry::DEFAULT_COLOR_CONFIG_VALUE;
use je_theme::theme_defaults::normalize_color;
use je_theme::token_classification_registry::{
    ProbeScope, SemanticTokenRule, TokenStyle, TokenStyleDefaults, TokenStyleValue, get_token_classification_registry,
    parse_classifier_string,
};
use je_theme::{ColorTheme, ThemeType};
use serde_json::{Map, Value};

use super::plist_parser;
use super::text_mate_scope_matcher::{MatchesName, Matcher, create_matchers};
use super::theme_compatibility::{ConvertedSettings, TextMateScope, TextMateThemingRule, convert_settings};
use super::theme_configuration::ThemeConfiguration;
use super::workbench_theme_service::{
    ExtensionData, IThemeExtensionPoint, IWorkbenchTheme, THEME_SCOPE_CLOSE_PAREN, THEME_SCOPE_OPEN_PAREN, THEME_SCOPE_WILDCARD,
};
use crate::contrib::snippets::snippets_file::IExtensionResourceLoaderService;

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
pub(super) fn is_truthy(value: &Value) -> bool {
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

/// `TokenStyleDefinition`: de onde veio uma propriedade do estilo de um token (uma regra semântica, os escopos
/// sondados ou o valor padrão do registro).
#[derive(Clone, Debug)]
pub enum TokenStyleDefinition {
    Rule(SemanticTokenRule),
    ProbeScopes(Vec<ProbeScope>),
    Value(TokenStyleValue),
}

/// `TokenStyleDefinitions`: a definição de cada propriedade de `TokenStyleData`.
#[derive(Clone, Debug, Default)]
pub struct TokenStyleDefinitions {
    pub foreground: Option<TokenStyleDefinition>,
    pub bold: Option<TokenStyleDefinition>,
    pub underline: Option<TokenStyleDefinition>,
    pub strikethrough: Option<TokenStyleDefinition>,
    pub italic: Option<TokenStyleDefinition>,
}

/// `TextMateThemingRuleDefinitions`: a regra do TextMate que definiu cada propriedade e o escopo que casou.
#[derive(Clone, Debug, Default)]
pub struct TextMateThemingRuleDefinitions {
    pub foreground: Option<TextMateThemingRule>,
    pub bold: Option<TextMateThemingRule>,
    pub underline: Option<TextMateThemingRule>,
    pub strikethrough: Option<TextMateThemingRule>,
    pub italic: Option<TextMateThemingRule>,
    pub scope: Option<ProbeScope>,
}

/// `ITokenStyle` (de `themeService.ts`): o estilo de um token com a cor como índice do `tokenColorMap`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ITokenStyle {
    pub foreground: Option<u32>,
    pub bold: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub italic: Option<bool>,
}

/// O resultado de `getTokenStylingRuleScope`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenStylingRuleScope {
    Setting,
    Theme,
}

/// O tema de cor do workbench.
#[derive(Clone)]
pub struct ColorThemeData {
    id: String,
    pub label: String,
    pub settings_id: String,
    pub description: Option<String>,
    pub is_loaded: bool,
    /// Só definido para tema de extensão do registro, não para o restaurado do armazenamento.
    pub location: Option<URI>,
    pub watch: Option<bool>,
    /// A extensão que contribuiu o tema (`extensionData`).
    pub extension_data: Option<ExtensionData>,

    theme_semantic_highlighting: Option<bool>,
    custom_semantic_highlighting: Option<bool>,
    custom_semantic_highlighting_deprecated: Option<bool>,

    theme_token_colors: Vec<TextMateThemingRule>,
    custom_token_colors: Vec<TextMateThemingRule>,
    /// `colorMap`, `customColorMap` e `transientColorMap`.
    colors: ColorTheme,

    semantic_token_rules: Vec<SemanticTokenRule>,
    custom_semantic_token_rules: Vec<SemanticTokenRule>,

    /// `themeTokenScopeMatchers` e `customTokenScopeMatchers`, criados sob demanda.
    theme_token_scope_matchers: RefCell<Option<Rc<Vec<Matcher<ProbeScope>>>>>,
    custom_token_scope_matchers: RefCell<Option<Rc<Vec<Matcher<ProbeScope>>>>>,

    /// `textMateThemingRules`, `tokenColorIndex` e `tokenFontIndex`, criados sob demanda.
    text_mate_theming_rules: RefCell<Option<Rc<Vec<TextMateThemingRule>>>>,
    token_color_index: RefCell<Option<Rc<TokenColorIndex>>>,
    token_font_index: RefCell<Option<Rc<TokenFontIndex>>>,
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
            location: None,
            watch: None,
            extension_data: None,
            theme_semantic_highlighting: None,
            custom_semantic_highlighting: None,
            custom_semantic_highlighting_deprecated: None,
            theme_token_colors: Vec::new(),
            custom_token_colors: Vec::new(),
            colors: ColorTheme::unloaded(label, theme_type_of_id(id), HashMap::new()),
            semantic_token_rules: Vec::new(),
            custom_semantic_token_rules: Vec::new(),
            theme_token_scope_matchers: RefCell::new(None),
            custom_token_scope_matchers: RefCell::new(None),
            text_mate_theming_rules: RefCell::new(None),
            token_color_index: RefCell::new(None),
            token_font_index: RefCell::new(None),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// `classNames`: o `id` separado por espaços.
    pub fn class_names(&self) -> Vec<&str> {
        self.id.split(' ').collect()
    }

    /// `semanticHighlighting`: o `enabled` do usuário vence o `semanticHighlighting` deprecado do usuário, que vence
    /// o do tema.
    pub fn semantic_highlighting(&self) -> bool {
        self.custom_semantic_highlighting
            .or(self.custom_semantic_highlighting_deprecated)
            .or(self.theme_semantic_highlighting)
            .unwrap_or(false)
    }

    /// As regras de semantic tokens do arquivo do tema (`semanticTokenRules`).
    pub fn semantic_token_rules(&self) -> &[SemanticTokenRule] {
        &self.semantic_token_rules
    }

    /// As regras de semantic tokens do usuário (`customSemanticTokenRules`).
    pub fn custom_semantic_token_rules(&self) -> &[SemanticTokenRule] {
        &self.custom_semantic_token_rules
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

    /// `rule.defaults[this.type]`: o valor padrão do registro para o tipo deste tema.
    fn defaults_for_type<'a>(&self, defaults: &'a TokenStyleDefaults) -> Option<&'a TokenStyleValue> {
        match self.theme_type() {
            ThemeType::Light => defaults.light.as_ref(),
            ThemeType::Dark => defaults.dark.as_ref(),
            ThemeType::HighContrastDark => defaults.hc_dark.as_ref(),
            ThemeType::HighContrastLight => defaults.hc_light.as_ref(),
        }
    }

    /// `getTokenStyle`: junta as regras semânticas do tema e do usuário (a de maior pontuação vence cada propriedade)
    /// e, para o que ficou indefinido, as regras padrão do registro.
    fn get_token_style(
        &self,
        token_type: &str,
        modifiers: &[String],
        language: &str,
        use_default: bool,
        mut definitions: Option<&mut TokenStyleDefinitions>,
    ) -> Option<TokenStyle> {
        const FOREGROUND: usize = 0;
        const BOLD: usize = 1;
        const UNDERLINE: usize = 2;
        const STRIKETHROUGH: usize = 3;
        const ITALIC: usize = 4;

        let mut result = TokenStyle::default();
        // foreground, bold, underline, strikethrough, italic, fontFamily, fontSize e lineHeight.
        let mut score = [-1_i64; 8];

        fn process_style(
            result: &mut TokenStyle,
            score: &mut [i64; 8],
            definitions: &mut Option<&mut TokenStyleDefinitions>,
            match_score: i64,
            style: &TokenStyle,
            definition: &dyn Fn() -> TokenStyleDefinition,
        ) {
            if style.foreground.is_some() && score[FOREGROUND] <= match_score {
                score[FOREGROUND] = match_score;
                result.foreground = style.foreground.clone();
                if let Some(definitions) = definitions.as_deref_mut() {
                    definitions.foreground = Some(definition());
                }
            }
            macro_rules! apply_flag {
                ($index:expr, $field:ident) => {
                    if let Some(info) = style.$field {
                        if score[$index] <= match_score {
                            score[$index] = match_score;
                            result.$field = Some(info);
                            if let Some(definitions) = definitions.as_deref_mut() {
                                definitions.$field = Some(definition());
                            }
                        }
                    }
                };
            }
            apply_flag!(BOLD, bold);
            apply_flag!(UNDERLINE, underline);
            apply_flag!(STRIKETHROUGH, strikethrough);
            apply_flag!(ITALIC, italic);
        }

        for rule in self.semantic_token_rules.iter().chain(&self.custom_semantic_token_rules) {
            let match_score = rule.selector.matches(token_type, modifiers, language);
            if match_score >= 0 {
                process_style(
                    &mut result,
                    &mut score,
                    &mut definitions,
                    i64::from(match_score),
                    &rule.style,
                    &|| TokenStyleDefinition::Rule(rule.clone()),
                );
            }
        }

        let mut has_undefined_style_property = false;
        for entry in &mut score {
            if *entry == -1 {
                has_undefined_style_property = true;
            } else {
                *entry = i64::MAX; // set it to the max, so it won't be replaced by a default
            }
        }
        if has_undefined_style_property {
            for rule in get_token_classification_registry().get_token_styling_default_rules() {
                let match_score = rule.selector.matches(token_type, modifiers, language);
                if match_score < 0 {
                    continue;
                }
                let match_score = i64::from(match_score);
                let mut style: Option<TokenStyle> = None;
                if let Some(scopes_to_probe) = &rule.defaults.scopes_to_probe {
                    style = self.resolve_scopes(scopes_to_probe, None);
                    if let Some(style) = &style {
                        process_style(&mut result, &mut score, &mut definitions, match_score, style, &|| {
                            TokenStyleDefinition::ProbeScopes(scopes_to_probe.clone())
                        });
                    }
                }
                if style.is_none() && use_default {
                    let token_style_value = self.defaults_for_type(&rule.defaults);
                    style = self.resolve_token_style_value(token_style_value);
                    if let Some(style) = &style {
                        process_style(&mut result, &mut score, &mut definitions, match_score, style, &|| {
                            TokenStyleDefinition::Value(
                                token_style_value.cloned().expect("o estilo só existe se o valor padrão existe"),
                            )
                        });
                    }
                }
            }
        }
        Some(result)
    }

    /// `resolveTokenStyleValue`: resolve um `TokenStyleValue` no contexto deste tema.
    pub fn resolve_token_style_value(&self, token_style_value: Option<&TokenStyleValue>) -> Option<TokenStyle> {
        match token_style_value? {
            TokenStyleValue::Classification(classifier) => {
                let parsed = parse_classifier_string(classifier, Some(""));
                self.get_token_style(&parsed.token_type, &parsed.modifiers, &parsed.language.unwrap_or_default(), true, None)
            }
            TokenStyleValue::Style(style) => Some(style.clone()),
        }
    }

    /// `getTokenColorIndex`: todas as cores que um token pode ter.
    pub fn get_token_color_index(&self) -> Rc<TokenColorIndex> {
        if let Some(index) = self.token_color_index.borrow().as_ref() {
            return index.clone();
        }
        let mut index = TokenColorIndex::default();
        for rule in self.token_colors().iter() {
            if let Some(settings) = &rule.settings {
                index.add(settings.get("foreground").and_then(Value::as_str));
                index.add(settings.get("background").and_then(Value::as_str));
            }
        }

        for rule in &self.semantic_token_rules {
            index.add_color(rule.style.foreground.as_ref());
        }
        for rule in get_token_classification_registry().get_token_styling_default_rules() {
            if let Some(TokenStyleValue::Style(style)) = self.defaults_for_type(&rule.defaults) {
                index.add_color(style.foreground.as_ref());
            }
        }
        for rule in &self.custom_semantic_token_rules {
            index.add_color(rule.style.foreground.as_ref());
        }

        let index = Rc::new(index);
        *self.token_color_index.borrow_mut() = Some(index.clone());
        index
    }

    /// `getTokenFontIndex`.
    pub fn get_token_font_index(&self) -> Rc<TokenFontIndex> {
        if let Some(index) = self.token_font_index.borrow().as_ref() {
            return index.clone();
        }
        let mut index = TokenFontIndex::default();
        for rule in self.token_colors().iter() {
            if let Some(settings) = &rule.settings {
                index.add(
                    settings.get("fontFamily").and_then(Value::as_str).map(str::to_owned),
                    settings.get("fontSize").and_then(Value::as_f64),
                    settings.get("lineHeight").and_then(Value::as_f64),
                );
            }
        }
        let index = Rc::new(index);
        *self.token_font_index.borrow_mut() = Some(index.clone());
        index
    }

    /// `tokenColorMap`.
    pub fn token_color_map(&self) -> Vec<Option<String>> {
        self.get_token_color_index().as_array()
    }

    /// `tokenFontMap`.
    pub fn token_font_map(&self) -> Vec<Option<IFontTokenOptions>> {
        self.get_token_font_index().as_array()
    }

    /// `getTokenStyleMetadata`.
    pub fn get_token_style_metadata(
        &self,
        type_with_language: &str,
        modifiers: &[String],
        default_language: &str,
        use_default: bool,
        definitions: Option<&mut TokenStyleDefinitions>,
    ) -> Option<ITokenStyle> {
        let classifier = parse_classifier_string(type_with_language, Some(default_language));
        let style = self.get_token_style(
            &classifier.token_type,
            modifiers,
            &classifier.language.unwrap_or_default(),
            use_default,
            definitions,
        )?;
        Some(ITokenStyle {
            foreground: Some(self.get_token_color_index().get_color(style.foreground.as_ref())),
            bold: style.bold,
            underline: style.underline,
            strikethrough: style.strikethrough,
            italic: style.italic,
        })
    }

    /// `getTokenStylingRuleScope`: de onde veio a regra, comparada por identidade (a regra tem de ser uma das listas
    /// do próprio tema).
    pub fn get_token_styling_rule_scope(&self, rule: &SemanticTokenRule) -> Option<TokenStylingRuleScope> {
        if self.custom_semantic_token_rules.iter().any(|candidate| std::ptr::eq(candidate, rule)) {
            return Some(TokenStylingRuleScope::Setting);
        }
        if self.semantic_token_rules.iter().any(|candidate| std::ptr::eq(candidate, rule)) {
            return Some(TokenStylingRuleScope::Theme);
        }
        None
    }

    /// Os comparadores das regras do arquivo do tema, criados na primeira vez.
    fn theme_scope_matchers(&self) -> Rc<Vec<Matcher<ProbeScope>>> {
        self.theme_token_scope_matchers
            .borrow_mut()
            .get_or_insert_with(|| Rc::new(self.theme_token_colors.iter().map(get_scope_matcher).collect()))
            .clone()
    }

    /// Os comparadores das regras do usuário, criados na primeira vez.
    fn custom_scope_matchers(&self) -> Rc<Vec<Matcher<ProbeScope>>> {
        self.custom_token_scope_matchers
            .borrow_mut()
            .get_or_insert_with(|| Rc::new(self.custom_token_colors.iter().map(get_scope_matcher).collect()))
            .clone()
    }

    /// `resolveScopes`: o estilo da primeira lista de escopos que casa com alguma regra do TextMate (a de maior
    /// pontuação vence a cor e o `fontStyle`; em empate, a última regra).
    pub fn resolve_scopes(
        &self,
        scopes: &[ProbeScope],
        mut definitions: Option<&mut TextMateThemingRuleDefinitions>,
    ) -> Option<TokenStyle> {
        let theme_matchers = self.theme_scope_matchers();
        let custom_matchers = self.custom_scope_matchers();

        for scope in scopes {
            let mut foreground: Option<String> = None;
            let mut font_style: Option<String> = None;
            let mut foreground_score = -1;
            let mut font_style_score = -1;
            let mut font_style_theming_rule: Option<&TextMateThemingRule> = None;
            let mut foreground_theming_rule: Option<&TextMateThemingRule> = None;

            for (matchers, theming_rules) in
                [(&*theme_matchers, &self.theme_token_colors), (&*custom_matchers, &self.custom_token_colors)]
            {
                for (matcher, theming_rule) in matchers.iter().zip(theming_rules) {
                    let score = matcher(scope);
                    if score < 0 {
                        continue;
                    }
                    let Some(settings) = &theming_rule.settings else { continue };
                    let rule_foreground = settings.get("foreground").and_then(Value::as_str).filter(|text| !text.is_empty());
                    if score >= foreground_score && rule_foreground.is_some() {
                        foreground = rule_foreground.map(str::to_owned);
                        foreground_score = score;
                        foreground_theming_rule = Some(theming_rule);
                    }
                    let rule_font_style = settings.get("fontStyle").and_then(Value::as_str);
                    if score >= font_style_score && rule_font_style.is_some() {
                        font_style = rule_font_style.map(str::to_owned);
                        font_style_score = score;
                        font_style_theming_rule = Some(theming_rule);
                    }
                }
            }
            if foreground.is_some() || font_style.is_some() {
                if let Some(definitions) = definitions.as_deref_mut() {
                    definitions.foreground = foreground_theming_rule.cloned();
                    definitions.bold = font_style_theming_rule.cloned();
                    definitions.italic = definitions.bold.clone();
                    definitions.underline = definitions.bold.clone();
                    definitions.strikethrough = definitions.bold.clone();
                    definitions.scope = Some(scope.clone());
                }
                return Some(TokenStyle::from_settings(foreground.as_deref(), font_style.as_deref(), None, None, None, None));
            }
        }
        None
    }

    /// `setCustomizations`: as customizações de cor, de token e de semantic token das configurações.
    pub fn set_customizations(&mut self, settings: &ThemeConfiguration) {
        self.set_custom_colors(&settings.color_customizations());
        self.set_custom_token_colors(&settings.token_color_customizations());
        self.set_custom_semantic_token_colors(settings.semantic_token_color_customizations().as_ref());
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
        self.clear_custom_caches();
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
        self.clear_custom_caches();
    }

    /// `setCustomSemanticTokenColors`: `enabled` e `rules` do usuário, depois os das chaves `[tema]` que casam com
    /// este tema (estes vencem).
    pub fn set_custom_semantic_token_colors(&mut self, semantic_token_colors: Option<&Map<String, Value>>) {
        self.custom_semantic_token_rules = Vec::new();
        self.custom_semantic_highlighting = None;

        if let Some(semantic_token_colors) = semantic_token_colors {
            self.custom_semantic_highlighting = semantic_token_colors.get("enabled").and_then(Value::as_bool);
            if let Some(rules) = semantic_token_colors.get("rules").filter(|rules| is_truthy(rules)) {
                self.read_semantic_token_rules(rules);
            }
            if let Some(theme_specific) = self.get_theme_specific_colors(semantic_token_colors) {
                if let Some(enabled) = theme_specific.get("enabled").and_then(Value::as_bool) {
                    self.custom_semantic_highlighting = Some(enabled);
                }
                if let Some(rules) = theme_specific.get("rules").filter(|rules| is_truthy(rules)) {
                    self.read_semantic_token_rules(rules);
                }
            }
        }

        self.clear_index_caches();
    }

    fn read_semantic_token_rules(&mut self, token_styling_rule_section: &Value) {
        let Value::Object(section) = token_styling_rule_section else { return };
        for (key, settings) in section {
            if !self.is_theme_scope(key) {
                // still do this test until experimental settings are gone
                if let Some(rule) = read_semantic_token_rule(key, settings) {
                    self.custom_semantic_token_rules.push(rule);
                }
            }
        }
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

    /// `clearCaches`.
    pub fn clear_caches(&mut self) {
        self.clear_index_caches();
        *self.theme_token_scope_matchers.borrow_mut() = None;
        *self.custom_token_scope_matchers.borrow_mut() = None;
    }

    /// Os caches que `setCustomColors` e `setCustomTokenColors` zeram: os índices, as regras do TextMate e os
    /// comparadores das regras do usuário (não os do arquivo do tema).
    fn clear_custom_caches(&mut self) {
        self.clear_index_caches();
        *self.custom_token_scope_matchers.borrow_mut() = None;
    }

    /// Os caches que `setCustomSemanticTokenColors` zera: `tokenColorIndex`, `tokenFontIndex` e `textMateThemingRules`.
    fn clear_index_caches(&mut self) {
        *self.token_color_index.borrow_mut() = None;
        *self.token_font_index.borrow_mut() = None;
        *self.text_mate_theming_rules.borrow_mut() = None;
    }

    /// `ensureLoaded`.
    pub async fn ensure_loaded(&mut self, extension_resource_loader_service: &dyn IExtensionResourceLoaderService) -> Result<(), Error> {
        if self.is_loaded { Ok(()) } else { self.load(extension_resource_loader_service).await }
    }

    /// `reload`.
    pub async fn reload(&mut self, extension_resource_loader_service: &dyn IExtensionResourceLoaderService) -> Result<(), Error> {
        self.load(extension_resource_loader_service).await
    }

    async fn load(&mut self, extension_resource_loader_service: &dyn IExtensionResourceLoaderService) -> Result<(), Error> {
        let Some(location) = self.location.clone() else { return Ok(()) };
        self.theme_token_colors = Vec::new();
        self.clear_caches();

        let mut result = LoadResult::default();
        load_color_theme(extension_resource_loader_service, &location, &mut result).await?;
        self.is_loaded = true;
        self.semantic_token_rules = result.semantic_token_rules;
        self.colors.set_colors(result.converted.colors.into_iter().collect());
        self.theme_token_colors = result.converted.text_mate_rules;
        self.theme_semantic_highlighting = result.semantic_highlighting;
        Ok(())
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
        value.insert(
            "semanticTokenRules".into(),
            Value::Array(self.semantic_token_rules.iter().map(SemanticTokenRule::to_json_object).collect()),
        );
        if let Some(extension_data) = ExtensionData::to_json_object(self.extension_data.as_ref()) {
            value.insert("extensionData".into(), extension_data);
        }
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

    /// `fromExtensionTheme`: o tema contribuído por uma extensão, ainda não carregado.
    pub fn from_extension_theme(theme: &IThemeExtensionPoint, color_theme_location: URI, extension_data: ExtensionData) -> ColorThemeData {
        let base_theme = theme.ui_theme.as_deref().filter(|ui_theme| !ui_theme.is_empty()).unwrap_or("vs-dark");
        let theme_selector = to_css_selector(&extension_data.extension_id, &theme.path);
        let id = format!("{base_theme} {theme_selector}");
        let label = match theme.label.as_deref().filter(|label| !label.is_empty()) {
            Some(label) => label.to_owned(),
            None => {
                let env = ProcessEnvironment;
                PathModule::native(&env as &dyn Environment).basename(&theme.path, None)
            }
        };
        let settings_id = if theme.id.is_empty() { label.clone() } else { theme.id.clone() };
        let mut theme_data = Self::new(&id, &label, &settings_id);
        theme_data.description = theme.description.clone();
        theme_data.watch = Some(theme.watch);
        theme_data.location = Some(color_theme_location);
        theme_data.extension_data = Some(extension_data);
        theme_data.is_loaded = false;
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
                "semanticTokenRules" => {
                    if let Value::Array(rules_data) = value {
                        let registry = get_token_classification_registry();
                        theme.semantic_token_rules.extend(
                            rules_data.iter().filter_map(|data| SemanticTokenRule::from_json_object(registry, data)),
                        );
                    }
                }
                "extensionData" => theme.extension_data = ExtensionData::from_json_object(value),
                // location: no longer restore
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

/// `noMatch`.
fn no_match(_scope: &ProbeScope) -> i32 {
    -1
}

/// `nameMatcher`: a pontuação do último identificador do seletor (o escopo mais profundo que o casa, vezes 0x10000,
/// mais o tamanho do identificador), ou -1 se algum identificador não casa com nenhum escopo.
fn name_matcher(identifiers: &[String], scopes: &ProbeScope) -> i32 {
    if scopes.len() < identifiers.len() {
        return -1;
    }

    let mut score: Option<i32> = None;
    let every = identifiers.iter().all(|identifier| {
        for i in (0..scopes.len()).rev() {
            if scopes_are_matching(&scopes[i], identifier) {
                score = Some((i as i32 + 1) * 0x10000 + identifier.encode_utf16().count() as i32);
                return true;
            }
        }
        false
    });
    match score {
        Some(score) if every => score,
        _ => -1,
    }
}

/// `scopesAreMatching`: o escopo é igual ao nome ou começa com ele seguido de `.`.
fn scopes_are_matching(this_scope_name: &str, scope_name: &str) -> bool {
    if this_scope_name.is_empty() {
        return false;
    }
    if this_scope_name == scope_name {
        return true;
    }
    this_scope_name.strip_prefix(scope_name).is_some_and(|rest| rest.starts_with('.'))
}

/// `getScopeMatcher`: o maior valor entre os comparadores do seletor da regra.
fn get_scope_matcher(rule: &TextMateThemingRule) -> Matcher<ProbeScope> {
    let Some(rule_scope) = rule.scope.as_ref().filter(|scope| scope.is_truthy()) else {
        return Box::new(no_match);
    };
    if rule.settings.is_none() {
        return Box::new(no_match);
    }
    let matches_name: MatchesName<ProbeScope> = Rc::new(name_matcher);
    let mut matchers = Vec::new();
    match rule_scope {
        TextMateScope::Many(scopes) => {
            for rs in scopes {
                create_matchers(rs, matches_name.clone(), &mut matchers);
            }
        }
        TextMateScope::One(scope) => create_matchers(scope, matches_name, &mut matchers),
    }

    if matchers.is_empty() {
        return Box::new(no_match);
    }
    Box::new(move |scope: &ProbeScope| {
        let mut max = (matchers[0].matcher)(scope);
        for matcher in &matchers[1..] {
            max = max.max((matcher.matcher)(scope));
        }
        max
    })
}

/// `readSemanticTokenRule`: uma cor (texto) ou um objeto com `foreground`, `fontStyle` e/ou os quatro estilos.
fn read_semantic_token_rule(selector_string: &str, settings: &Value) -> Option<SemanticTokenRule> {
    let selector = get_token_classification_registry().parse_token_selector(selector_string, None);
    let style = match settings {
        Value::String(foreground) => Some(TokenStyle::from_settings(Some(foreground.as_str()), None, None, None, None, None)),
        settings if is_semantic_token_colorization_setting(settings) => Some(TokenStyle::from_settings(
            settings.get("foreground").and_then(Value::as_str),
            settings.get("fontStyle").and_then(Value::as_str),
            settings.get("bold").and_then(Value::as_bool),
            settings.get("underline").and_then(Value::as_bool),
            settings.get("strikethrough").and_then(Value::as_bool),
            settings.get("italic").and_then(Value::as_bool),
        )),
        _ => None,
    };
    style.map(|style| SemanticTokenRule { selector, style })
}

/// `isSemanticTokenColorizationSetting`.
fn is_semantic_token_colorization_setting(style: &Value) -> bool {
    let is_string = |key: &str| style.get(key).is_some_and(Value::is_string);
    let is_boolean = |key: &str| style.get(key).is_some_and(Value::is_boolean);
    is_string("foreground")
        || is_string("fontStyle")
        || is_boolean("italic")
        || is_boolean("underline")
        || is_boolean("strikethrough")
        || is_boolean("bold")
}

/// `findMetadata`: o metadado codificado de um token (linguagem, tipo padrão, estilo de fonte, cor e colchetes
/// balanceados) para a lista de escopos capturados.
pub fn find_metadata(color_theme_data: &ColorThemeData, capture_names: &[String], language_id: u32, bracket: bool) -> u32 {
    let mut metadata = 0;

    metadata |= language_id << metadata_consts::LANGUAGEID_OFFSET;

    let mut definitions = TextMateThemingRuleDefinitions::default();
    let token_style = color_theme_data.resolve_scopes(&[capture_names.to_vec()], Some(&mut definitions));

    if let Some(last) = capture_names.last() {
        let standard_token = to_standard_token_type(last);
        metadata |= standard_token << metadata_consts::TOKEN_TYPE_OFFSET;
    }

    let font_style_of = |rule: &Option<TextMateThemingRule>| -> Option<String> {
        let settings = rule.as_ref()?.settings.as_ref()?;
        settings.get("fontStyle")?.as_str().filter(|text| !text.is_empty()).map(str::to_owned)
    };
    let rule_font_style = font_style_of(&definitions.foreground).or_else(|| font_style_of(&definitions.bold));
    let has_style = |name: &str| rule_font_style.as_deref().is_some_and(|text| text.contains(name));
    if has_style("italic") {
        metadata |= font_style::ITALIC | metadata_consts::ITALIC_MASK;
    }
    if has_style("bold") {
        metadata |= font_style::BOLD | metadata_consts::BOLD_MASK;
    }
    if has_style("underline") {
        metadata |= font_style::UNDERLINE | metadata_consts::UNDERLINE_MASK;
    }
    if has_style("strikethrough") {
        metadata |= font_style::STRIKETHROUGH | metadata_consts::STRIKETHROUGH_MASK;
    }

    let token_style_foreground = match token_style.as_ref().and_then(|style| style.foreground.as_ref()) {
        Some(foreground) => color_theme_data.get_token_color_index().get_color(Some(foreground)),
        None => color_id::DEFAULT_FOREGROUND,
    };
    metadata |= token_style_foreground << metadata_consts::FOREGROUND_OFFSET;

    if bracket {
        metadata |= metadata_consts::BALANCED_BRACKETS_MASK;
    }

    metadata
}

/// `TokenColorIndex`: numera as cores (normalizadas) dos tokens a partir de 1; o 0 é "sem cor".
#[derive(Debug, Default)]
pub struct TokenColorIndex {
    last_color_id: u32,
    id2color: Vec<Option<String>>,
    color2id: HashMap<String, u32>,
}

impl TokenColorIndex {
    /// `add` com uma cor em texto.
    pub fn add(&mut self, color: Option<&str>) -> u32 {
        let Some(color) = color.and_then(normalize_color) else { return 0 };
        if let Some(&value) = self.color2id.get(&color) {
            return value;
        }
        self.last_color_id += 1;
        let value = self.last_color_id;
        self.color2id.insert(color.clone(), value);
        if self.id2color.len() <= value as usize {
            self.id2color.resize(value as usize + 1, None);
        }
        self.id2color[value as usize] = Some(color);
        value
    }

    /// `add` com uma `Color`.
    pub fn add_color(&mut self, color: Option<&Color>) -> u32 {
        self.add(color.and_then(normalize_color_value).as_deref())
    }

    /// `get` com uma cor em texto: 0 se não é uma cor válida ou não está no índice.
    pub fn get(&self, color: Option<&str>) -> u32 {
        let Some(color) = color.and_then(normalize_color) else { return 0 };
        self.color2id.get(&color).copied().unwrap_or(0)
    }

    /// `get` com uma `Color`.
    pub fn get_color(&self, color: Option<&Color>) -> u32 {
        self.get(color.and_then(normalize_color_value).as_deref())
    }

    /// `asArray`: o índice 0 é o buraco do array esparso (vazio se nenhuma cor foi adicionada).
    pub fn as_array(&self) -> Vec<Option<String>> {
        self.id2color.clone()
    }
}

/// `TokenFontIndex`: numera as fontes dos tokens a partir de 1. Cada `add` cria uma fonte nova (o `Map` do original
/// é chaveado pela identidade do objeto, que nunca se repete).
#[derive(Debug, Default)]
pub struct TokenFontIndex {
    last_font_id: u32,
    id2font: Vec<Option<IFontTokenOptions>>,
}

impl TokenFontIndex {
    pub fn add(&mut self, font_family: Option<String>, font_size_multiplier: Option<f64>, line_height_multiplier: Option<f64>) -> u32 {
        self.last_font_id += 1;
        let value = self.last_font_id;
        if self.id2font.len() <= value as usize {
            self.id2font.resize(value as usize + 1, None);
        }
        self.id2font[value as usize] = Some(IFontTokenOptions { font_family, font_size_multiplier, line_height_multiplier });
        value
    }

    /// `asArray`: o índice 0 é o buraco do array esparso.
    pub fn as_array(&self) -> Vec<Option<IFontTokenOptions>> {
        self.id2font.clone()
    }
}

/// O `result` de `_loadColorTheme`: as regras e as cores (de `convertSettings`), as regras semânticas e o
/// `semanticHighlighting` (`false` no início; `None` é o `undefined` que `false || undefined` deixa).
struct LoadResult {
    converted: ConvertedSettings,
    semantic_token_rules: Vec<SemanticTokenRule>,
    semantic_highlighting: Option<bool>,
}

impl Default for LoadResult {
    fn default() -> Self {
        Self { converted: ConvertedSettings::default(), semantic_token_rules: Vec::new(), semantic_highlighting: Some(false) }
    }
}

/// `JSON.parse`-like: o valor de `je_base::json` como `serde_json::Value`.
pub(super) fn to_serde(value: &js_json::Value) -> Value {
    match value {
        js_json::Value::Null => Value::Null,
        js_json::Value::Bool(flag) => Value::Bool(*flag),
        js_json::Value::Number(number) => {
            if number.fract() == 0.0 && number.abs() < 9.007_199_254_740_992e15 {
                Value::from(*number as i64)
            } else {
                serde_json::Number::from_f64(*number).map_or(Value::Null, Value::Number)
            }
        }
        js_json::Value::String(text) => Value::String(text.to_string_lossy()),
        js_json::Value::Array(items) => Value::Array(items.iter().map(to_serde).collect()),
        js_json::Value::Object(object) => {
            Value::Object(object.iter().map(|(key, value)| (key.to_string_lossy(), to_serde(value))).collect())
        }
    }
}

/// `_loadColorTheme`: lê o arquivo do tema (JSON, seguindo `include`; ou `.tmTheme`) e acumula o resultado.
fn load_color_theme<'a>(
    extension_resource_loader_service: &'a dyn IExtensionResourceLoaderService,
    theme_location: &'a URI,
    result: &'a mut LoadResult,
) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'a>> {
    Box::pin(async move {
        let resources = ext_uri();
        if resources.extname(theme_location) != ".json" {
            return load_syntax_tokens(extension_resource_loader_service, theme_location, &mut result.converted).await;
        }

        let content = extension_resource_loader_service.read_extension_resource(theme_location).await?;
        let mut errors = Vec::new();
        let units: Vec<u16> = content.encode_utf16().collect();
        let parsed = js_json::parse(&units, &mut errors, ParseOptions::default());
        if !errors.is_empty() {
            let messages: Vec<String> = errors.iter().map(|e| parse_error_message(e.error)).collect();
            return Err(Error::new(localize(
                "error.cannotparsejson",
                "Problems parsing JSON theme file: {0}",
                &[&messages.join(", ")],
            )));
        }
        let content_value = match parsed.as_ref().map(|value| (js_json::node_type(value), value)) {
            Some((NodeType::Object, value)) => to_serde(value),
            _ => {
                return Err(Error::new(localize(
                    "error.invalidformat",
                    "Invalid format for JSON theme file: Object expected.",
                    &[],
                )));
            }
        };
        let location_text = theme_location.to_string(false).to_string_lossy();

        if let Some(include) = content_value.get("include").and_then(Value::as_str).filter(|include| !include.is_empty()) {
            let included = resources.join_path(&resources.dirname(theme_location), &[include]);
            load_color_theme(extension_resource_loader_service, &included, &mut *result).await?;
        }
        if let Some(Value::Array(settings)) = content_value.get("settings") {
            convert_settings(settings.iter().filter_map(rule_from_json).collect(), &mut result.converted);
            return Ok(());
        }
        // `result.semanticHighlighting || contentValue.semanticHighlighting`
        if result.semantic_highlighting != Some(true) {
            result.semantic_highlighting = content_value.get("semanticHighlighting").map(is_truthy);
        }
        if let Some(colors) = content_value.get("colors").filter(|colors| is_truthy(colors)) {
            if !matches!(colors, Value::Object(_) | Value::Array(_)) {
                return Err(Error::new(localize(
                    "error.invalidformat.colors",
                    "Problem parsing color theme file: {0}. Property 'colors' is not of type 'object'.",
                    &[&location_text],
                )));
            }
            // new JSON color themes format
            if let Value::Object(colors) = colors {
                for (color_id, color_val) in colors {
                    match color_val {
                        // ignore colors that are set to to default
                        Value::String(text) if text == DEFAULT_COLOR_CONFIG_VALUE => {
                            result.converted.colors.shift_remove(color_id);
                        }
                        Value::String(text) => {
                            result.converted.colors.insert(color_id.clone(), Color::from_hex(text));
                        }
                        _ => {}
                    }
                }
            }
        }
        if let Some(token_colors) = content_value.get("tokenColors").filter(|token_colors| is_truthy(token_colors)) {
            match token_colors {
                Value::Array(rules) => result.converted.text_mate_rules.extend(rules.iter().filter_map(rule_from_json)),
                Value::String(path) => {
                    let syntax_location = resources.join_path(&resources.dirname(theme_location), &[path]);
                    load_syntax_tokens(extension_resource_loader_service, &syntax_location, &mut result.converted).await?;
                }
                _ => {
                    return Err(Error::new(localize(
                        "error.invalidformat.tokenColors",
                        "Problem parsing color theme file: {0}. Property 'tokenColors' should be either an array specifying colors or a path to a TextMate theme file",
                        &[&location_text],
                    )));
                }
            }
        }
        if let Some(Value::Object(semantic_token_colors)) = content_value.get("semanticTokenColors") {
            for (key, settings) in semantic_token_colors {
                if let Some(rule) = read_semantic_token_rule(key, settings) {
                    result.semantic_token_rules.push(rule);
                }
            }
        }
        Ok(())
    })
}

/// `_loadSyntaxTokens`: lê um `.tmTheme` (plist) e converte as `settings`.
async fn load_syntax_tokens(
    extension_resource_loader_service: &dyn IExtensionResourceLoaderService,
    theme_location: &URI,
    result: &mut ConvertedSettings,
) -> Result<(), Error> {
    let content = extension_resource_loader_service.read_extension_resource(theme_location).await.map_err(|error| {
        Error::new(localize(
            "error.cannotload",
            "Problems loading tmTheme file {0}: {1}",
            &[&theme_location.to_string(false).to_string_lossy(), &error.message],
        ))
    })?;
    let content_value = plist_parser::parse(&content)
        .map_err(|error| Error::new(localize("error.cannotparse", "Problems parsing tmTheme file: {0}", &[&error.message])))?;
    let Some(Value::Array(settings)) = content_value.get("settings") else {
        return Err(Error::new(localize(
            "error.plist.invalidformat",
            "Problem parsing tmTheme file: {0}. 'settings' is not array.",
            &[],
        )));
    };
    convert_settings(settings.iter().filter_map(rule_from_json).collect(), result);
    Ok(())
}

/// `toCSSSelector`: o `extensionId` e o caminho do tema como uma classe de CSS (só `_`, letras, dígitos e `-`; um
/// começo com dígito ou `-` ganha `_` na frente).
fn to_css_selector(extension_id: &str, path: &str) -> String {
    let path = path.strip_prefix("./").unwrap_or(path);
    let selector: String = format!("{extension_id}-{path}")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' })
        .collect();
    if selector.starts_with(|c: char| c.is_ascii_digit() || c == '-') { format!("_{selector}") } else { selector }
}

impl IWorkbenchTheme for ColorThemeData {
    fn id(&self) -> &str {
        &self.id
    }
    fn label(&self) -> &str {
        &self.label
    }
    fn extension_data(&self) -> Option<&ExtensionData> {
        self.extension_data.as_ref()
    }
    fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
    fn settings_id(&self) -> Option<&str> {
        Some(&self.settings_id)
    }
}
