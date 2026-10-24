//! Porte de `src/vs/platform/theme/common/tokenClassificationRegistry.ts`: o registro dos tipos e modificadores de
//! semantic tokens (`registerTokenType`, `registerTokenModifier`), as regras de estilo padrão
//! (`getTokenStylingDefaultRules`), o `TokenStyle`, o `SemanticTokenRule`, o `parseClassifierString` e o esquema
//! JSON `tokenStylingSchema`.
//!
//! Divergências:
//! - `IJSONSchema` é um `serde_json::Value` (a mesma representação do restante do porte). `getTokenStylingSchema`
//!   devolve uma cópia montada na hora, não o objeto vivo, porque o registro guarda as propriedades à parte.
//! - O registro é um valor compartilhável (`Arc<Mutex<_>>`) e o `TokenSelector` guarda uma referência a ele, como o
//!   `match` do original captura o `this`. A linha `Registry.add(...)` (registro de plataforma) não existe aqui:
//!   o singleton é [`get_token_classification_registry`].
//! - Ficam de fora o `onDidChangeSchema` (o original declara o emissor e nunca o dispara) e o registro do esquema no
//!   `jsonContributionRegistry` com o `RunOnceScheduler` que reage a esse evento, que dependem desse emissor.
//!   A constante [`TOKEN_STYLING_SCHEMA_ID`] fica para quem registrar o esquema.
//! - `TokenStyleData` é um apelido de [`TokenStyle`] (os dois têm os mesmos cinco campos), por isso não há
//!   `TokenStyle.fromData`. `TokenStyle.is` e `SemanticTokenRule.is` (`instanceof` e duck typing) não existem: o tipo
//!   estático já diz.
//! - `TokenStyleFunction` recebe o [`ColorThemeContext`] no lugar do `IColorTheme`.
//! - A hierarquia de um tipo interrompe a subida se o `superType` já está nela; o original não termina num ciclo.
//! - `registerTokenType` e `registerTokenModifier` devolvem `Result` no lugar de lançar `Error`.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, OnceLock, PoisonError};

use je_base::collator::locale_compare;
use je_base::nls::localize;
use serde_json::{Map, Value, json};

use crate::color::Color;
use crate::color::format::css::format_hex_a;
use crate::registry::ColorThemeContext;

const TOKEN_TYPE_WILDCARD: &str = "*";
const TOKEN_CLASSIFIER_LANGUAGE_SEPARATOR: char = ':';
const CLASSIFIER_MODIFIER_SEPARATOR: char = '.';

/// `idPattern`.
const ID_PATTERN: &str = r"\w+[-_\w+]*";

/// `typeAndModifierIdPattern`.
pub static TYPE_AND_MODIFIER_ID_PATTERN: LazyLock<String> = LazyLock::new(|| format!("^{ID_PATTERN}$"));

/// `selectorPattern`.
static SELECTOR_PATTERN: LazyLock<String> = LazyLock::new(|| {
    format!(
        r"^({ID_PATTERN}|\*)(\{CLASSIFIER_MODIFIER_SEPARATOR}{ID_PATTERN})*({TOKEN_CLASSIFIER_LANGUAGE_SEPARATOR}{ID_PATTERN})?$"
    )
});

/// `fontStylePattern`.
const FONT_STYLE_PATTERN: &str = r"^(\s*(italic|bold|underline|strikethrough))*\s*$";

/// `tokenStylingSchemaId`.
pub const TOKEN_STYLING_SCHEMA_ID: &str = "vscode://schemas/token-styling";

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

/// `id.match(typeAndModifierIdPattern)`: `\w+[-_\w+]*` de ponta a ponta, isto é, um caractere de palavra
/// (`[A-Za-z0-9_]`) seguido de palavras, `-` ou `+`.
fn matches_id_pattern(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|first| first.is_ascii_alphanumeric() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+'))
}

/// O erro de `registerTokenType` e `registerTokenModifier` (o `Error` que o original lança).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenClassificationError {
    InvalidTokenTypeId,
    InvalidTokenSuperTypeId,
    InvalidTokenModifierId,
}

impl fmt::Display for TokenClassificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidTokenTypeId => "Invalid token type id.",
            Self::InvalidTokenSuperTypeId => "Invalid token super type id.",
            Self::InvalidTokenModifierId => "Invalid token modifier id.",
        })
    }
}

impl std::error::Error for TokenClassificationError {}

/// `TokenTypeOrModifierContribution`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenTypeOrModifierContribution {
    pub num: u64,
    pub id: String,
    pub super_type: Option<String>,
    pub description: String,
    pub deprecation_message: Option<String>,
}

/// `TokenStyle` e `TokenStyleData`: a cor e os quatro estilos de fonte de um token (`None` é `undefined`).
#[derive(Clone, Debug, Default)]
pub struct TokenStyle {
    pub foreground: Option<Color>,
    pub bold: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub italic: Option<bool>,
}

/// `TokenStyleData`, ver as divergências do módulo.
pub type TokenStyleData = TokenStyle;

impl TokenStyle {
    /// `TokenStyle.toJSONObject`.
    pub fn to_json_object(&self) -> Value {
        json!({
            "_foreground": self.foreground.as_ref().map(|color| format_hex_a(color, true)),
            "_bold": self.bold,
            "_underline": self.underline,
            "_italic": self.italic,
            "_strikethrough": self.strikethrough,
        })
    }

    /// `TokenStyle.fromJSONObject`: `None` se o valor é falso (`if (obj)`).
    pub fn from_json_object(obj: &Value) -> Option<TokenStyle> {
        if !is_truthy(obj) {
            return None;
        }
        let bool_or_undef = |key: &str| obj.get(key).and_then(Value::as_bool);
        Some(TokenStyle {
            foreground: obj.get("_foreground").and_then(Value::as_str).map(Color::from_hex),
            bold: bool_or_undef("_bold"),
            underline: bool_or_undef("_underline"),
            strikethrough: bool_or_undef("_strikethrough"),
            italic: bool_or_undef("_italic"),
        })
    }

    /// `TokenStyle.equals`, com `undefined` representado por `None`.
    pub fn equals(s1: Option<&TokenStyle>, s2: Option<&TokenStyle>) -> bool {
        match (s1, s2) {
            (None, None) => true,
            (Some(s1), Some(s2)) => {
                std::ptr::eq(s1, s2)
                    || (Color::equals_opt(s1.foreground.as_ref(), s2.foreground.as_ref())
                        && s1.bold == s2.bold
                        && s1.underline == s2.underline
                        && s1.strikethrough == s2.strikethrough
                        && s1.italic == s2.italic)
            }
            _ => false,
        }
    }

    /// `TokenStyle.fromSettings`: o `fontStyle`, se presente, zera os quatro estilos e liga os que ele cita (as
    /// palavras casadas em qualquer posição, como o `/italic|bold|underline|strikethrough/g` do original).
    pub fn from_settings(
        foreground: Option<&str>,
        font_style: Option<&str>,
        mut bold: Option<bool>,
        mut underline: Option<bool>,
        mut strikethrough: Option<bool>,
        mut italic: Option<bool>,
    ) -> TokenStyle {
        let foreground = foreground.map(Color::from_hex);
        if let Some(font_style) = font_style {
            bold = Some(false);
            italic = Some(false);
            underline = Some(false);
            strikethrough = Some(false);
            let mut rest = font_style;
            while !rest.is_empty() {
                let keyword = ["italic", "bold", "underline", "strikethrough"].into_iter().find(|keyword| rest.starts_with(*keyword));
                match keyword {
                    Some(keyword) => {
                        match keyword {
                            "bold" => bold = Some(true),
                            "italic" => italic = Some(true),
                            "underline" => underline = Some(true),
                            _ => strikethrough = Some(true),
                        }
                        rest = &rest[keyword.len()..];
                    }
                    None => {
                        let mut chars = rest.chars();
                        chars.next();
                        rest = chars.as_str();
                    }
                }
            }
        }
        TokenStyle { foreground, bold, underline, strikethrough, italic }
    }
}

/// `ProbeScope`.
pub type ProbeScope = Vec<String>;

/// `TokenStyleFunction`.
pub type TokenStyleFunction = dyn Fn(&dyn ColorThemeContext) -> Option<TokenStyle>;

/// `TokenStyleValue`: um estilo literal ou uma `TokenClassificationString`.
#[derive(Clone, Debug)]
pub enum TokenStyleValue {
    Style(TokenStyle),
    Classification(String),
}

/// `TokenStyleDefaults`.
#[derive(Clone, Debug, Default)]
pub struct TokenStyleDefaults {
    pub scopes_to_probe: Option<Vec<ProbeScope>>,
    pub light: Option<TokenStyleValue>,
    pub dark: Option<TokenStyleValue>,
    pub hc_dark: Option<TokenStyleValue>,
    pub hc_light: Option<TokenStyleValue>,
}

/// O seletor já interpretado de um `TokenSelector` válido.
#[derive(Clone, Debug)]
struct SelectorMatcher {
    registry: Arc<Mutex<State>>,
    token_type: String,
    modifiers: Vec<String>,
    language: Option<String>,
}

/// `TokenSelector`.
#[derive(Clone, Debug)]
pub struct TokenSelector {
    pub id: String,
    /// `None` no seletor inválido (`$invalid`), que não casa com nada.
    matcher: Option<SelectorMatcher>,
}

impl TokenSelector {
    /// `match(type, modifiers, language)`: a pontuação, ou -1 se o seletor não casa.
    pub fn matches<M: AsRef<str>>(&self, token_type: &str, modifiers: &[M], language: &str) -> i32 {
        let Some(matcher) = &self.matcher else { return -1 };
        let mut score = 0;
        if let Some(selector_language) = &matcher.language {
            if selector_language != language {
                return -1;
            }
            score += 10;
        }
        if matcher.token_type != TOKEN_TYPE_WILDCARD {
            let level = lock(&matcher.registry).type_hierarchy(token_type).iter().position(|id| *id == matcher.token_type);
            let Some(level) = level else { return -1 };
            score += 100 - level as i32;
        }
        // all selector modifiers must be present
        for selector_modifier in &matcher.modifiers {
            if !modifiers.iter().any(|modifier| modifier.as_ref() == selector_modifier) {
                return -1;
            }
        }
        score + matcher.modifiers.len() as i32 * 100
    }
}

/// `SemanticTokenDefaultRule`.
#[derive(Clone, Debug)]
pub struct SemanticTokenDefaultRule {
    pub selector: TokenSelector,
    pub defaults: TokenStyleDefaults,
}

/// `SemanticTokenRule`.
#[derive(Clone, Debug)]
pub struct SemanticTokenRule {
    pub style: TokenStyle,
    pub selector: TokenSelector,
}

impl SemanticTokenRule {
    /// `SemanticTokenRule.fromJSONObject`.
    pub fn from_json_object(registry: &TokenClassificationRegistry, o: &Value) -> Option<SemanticTokenRule> {
        let selector = o.get("_selector").and_then(Value::as_str)?;
        let style = TokenStyle::from_json_object(o.get("_style")?)?;
        Some(SemanticTokenRule { selector: registry.parse_token_selector(selector, None), style })
    }

    /// `SemanticTokenRule.toJSONObject`.
    pub fn to_json_object(&self) -> Value {
        json!({ "_selector": self.selector.id, "_style": self.style.to_json_object() })
    }

    /// `SemanticTokenRule.equals`, com `undefined` representado por `None`.
    pub fn equals(r1: Option<&SemanticTokenRule>, r2: Option<&SemanticTokenRule>) -> bool {
        match (r1, r2) {
            (None, None) => true,
            (Some(r1), Some(r2)) => {
                std::ptr::eq(r1, r2)
                    || (r1.selector.id == r2.selector.id && TokenStyle::equals(Some(&r1.style), Some(&r2.style)))
            }
            _ => false,
        }
    }
}

/// O resultado de [`parse_classifier_string`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassifierString {
    pub token_type: String,
    pub modifiers: Vec<String>,
    pub language: Option<String>,
}

/// `parseClassifierString`: `type(.modifier)*(:language)?`, lido do fim para o começo (os modificadores saem na
/// ordem inversa à do texto).
pub fn parse_classifier_string(s: &str, default_language: Option<&str>) -> ClassifierString {
    let mut k = s.len();
    let mut language = default_language.map(str::to_owned);
    let mut modifiers = Vec::new();

    for (i, ch) in s.char_indices().rev() {
        if ch == TOKEN_CLASSIFIER_LANGUAGE_SEPARATOR || ch == CLASSIFIER_MODIFIER_SEPARATOR {
            let segment = &s[i + 1..k];
            k = i;
            if ch == TOKEN_CLASSIFIER_LANGUAGE_SEPARATOR {
                language = Some(segment.to_owned());
            } else {
                modifiers.push(segment.to_owned());
            }
        }
    }
    ClassifierString { token_type: s[..k].to_owned(), modifiers, language }
}

/// `getStylingSchemeEntry`.
fn get_styling_scheme_entry(description: Option<&str>, deprecation_message: Option<&str>) -> Value {
    let mut entry = Map::new();
    if let Some(description) = description {
        entry.insert("description".into(), description.into());
    }
    if let Some(deprecation_message) = deprecation_message {
        entry.insert("deprecationMessage".into(), deprecation_message.into());
    }
    entry.insert("defaultSnippets".into(), json!([{ "body": "${1:#ff0000}" }]));
    entry.insert(
        "anyOf".into(),
        json!([{ "type": "string", "format": "color-hex" }, { "$ref": "#/definitions/style" }]),
    );
    Value::Object(entry)
}

/// A ordem de `Object.keys`: as chaves que são índices de array (`"0"`, `"12"`) primeiro, em ordem numérica, e as
/// demais na ordem de inserção.
fn sort_as_object_keys(items: &mut [TokenTypeOrModifierContribution]) {
    let array_index = |key: &str| key.parse::<u32>().ok().filter(|index| *index != u32::MAX && index.to_string() == key);
    items.sort_by_key(|item| array_index(&item.id).map_or((1, 0), |index| (0, index)));
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// O estado do `TokenClassificationRegistry`. Os objetos do original (`tokenTypeById`, `tokenModifierById` e as
/// `properties` do esquema) são listas na ordem de inserção: re-registrar um id mantém a posição, como a atribuição
/// a uma chave existente.
#[derive(Debug)]
struct State {
    current_type_number: u64,
    current_modifier_bit: u64,
    token_type_by_id: Vec<TokenTypeOrModifierContribution>,
    token_modifier_by_id: Vec<TokenTypeOrModifierContribution>,
    token_styling_default_rules: Vec<SemanticTokenDefaultRule>,
    type_hierarchy: HashMap<String, Vec<String>>,
    /// `tokenStylingSchema.properties`.
    schema_properties: Vec<(String, Value)>,
}

fn upsert<T>(items: &mut Vec<T>, item: T, same: impl Fn(&T) -> bool) {
    match items.iter().position(same) {
        Some(index) => items[index] = item,
        None => items.push(item),
    }
}

impl State {
    fn set_schema_property(&mut self, id: String, entry: Value) {
        upsert(&mut self.schema_properties, (id.clone(), entry), |(key, _)| *key == id);
    }

    fn delete_schema_property(&mut self, id: &str) {
        self.schema_properties.retain(|(key, _)| key != id);
    }

    /// `getTypeHierarchy`: o tipo e a cadeia dos seus `superType`.
    fn type_hierarchy(&mut self, type_id: &str) -> &Vec<String> {
        if !self.type_hierarchy.contains_key(type_id) {
            let mut hierarchy = vec![type_id.to_owned()];
            let mut current = self.token_type_by_id.iter().find(|contribution| contribution.id == type_id);
            while let Some(super_type) =
                current.and_then(|contribution| contribution.super_type.as_deref()).filter(|id| !id.is_empty())
            {
                if hierarchy.iter().any(|id| id == super_type) {
                    break;
                }
                hierarchy.push(super_type.to_owned());
                current = self.token_type_by_id.iter().find(|contribution| contribution.id == super_type);
            }
            self.type_hierarchy.insert(type_id.to_owned(), hierarchy);
        }
        &self.type_hierarchy[type_id]
    }
}

/// `TokenClassificationRegistry`.
#[derive(Clone, Debug)]
pub struct TokenClassificationRegistry {
    state: Arc<Mutex<State>>,
}

impl Default for TokenClassificationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenClassificationRegistry {
    /// Um registro vazio.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                current_type_number: 0,
                current_modifier_bit: 1,
                token_type_by_id: Vec::new(),
                token_modifier_by_id: Vec::new(),
                token_styling_default_rules: Vec::new(),
                type_hierarchy: HashMap::new(),
                schema_properties: Vec::new(),
            })),
        }
    }

    /// `registerTokenType`.
    pub fn register_token_type(
        &self,
        id: &str,
        description: &str,
        super_type: Option<&str>,
        deprecation_message: Option<&str>,
    ) -> Result<(), TokenClassificationError> {
        if !matches_id_pattern(id) {
            return Err(TokenClassificationError::InvalidTokenTypeId);
        }
        if super_type.is_some_and(|super_type| !super_type.is_empty() && !matches_id_pattern(super_type)) {
            return Err(TokenClassificationError::InvalidTokenSuperTypeId);
        }

        let mut state = lock(&self.state);
        let num = state.current_type_number;
        state.current_type_number += 1;
        let contribution = TokenTypeOrModifierContribution {
            num,
            id: id.to_owned(),
            super_type: super_type.map(str::to_owned),
            description: description.to_owned(),
            deprecation_message: deprecation_message.map(str::to_owned),
        };
        upsert(&mut state.token_type_by_id, contribution, |existing| existing.id == id);

        state.set_schema_property(id.to_owned(), get_styling_scheme_entry(Some(description), deprecation_message));
        state.type_hierarchy = HashMap::new();
        Ok(())
    }

    /// `registerTokenModifier`.
    pub fn register_token_modifier(
        &self,
        id: &str,
        description: &str,
        deprecation_message: Option<&str>,
    ) -> Result<(), TokenClassificationError> {
        if !matches_id_pattern(id) {
            return Err(TokenClassificationError::InvalidTokenModifierId);
        }

        let mut state = lock(&self.state);
        let num = state.current_modifier_bit;
        state.current_modifier_bit = state.current_modifier_bit.saturating_mul(2);
        let contribution = TokenTypeOrModifierContribution {
            num,
            id: id.to_owned(),
            super_type: None,
            description: description.to_owned(),
            deprecation_message: deprecation_message.map(str::to_owned),
        };
        upsert(&mut state.token_modifier_by_id, contribution, |existing| existing.id == id);

        state.set_schema_property(format!("*.{id}"), get_styling_scheme_entry(Some(description), deprecation_message));
        Ok(())
    }

    /// `parseTokenSelector`: o seletor `(*|type)(.modifier)*`, de `language` se o texto não tem `:language`. Um
    /// seletor sem tipo é o `$invalid`, que não casa com nada.
    pub fn parse_token_selector(&self, selector_string: &str, language: Option<&str>) -> TokenSelector {
        let selector = parse_classifier_string(selector_string, language);

        if selector.token_type.is_empty() {
            return TokenSelector { id: "$invalid".to_owned(), matcher: None };
        }

        let mut modifiers = selector.modifiers;
        modifiers.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        let mut id = selector.token_type.clone();
        for modifier in &modifiers {
            id.push(CLASSIFIER_MODIFIER_SEPARATOR);
            id.push_str(modifier);
        }
        if let Some(language) = &selector.language {
            id.push(TOKEN_CLASSIFIER_LANGUAGE_SEPARATOR);
            id.push_str(language);
        }
        TokenSelector {
            id,
            matcher: Some(SelectorMatcher {
                registry: self.state.clone(),
                token_type: selector.token_type,
                modifiers,
                language: selector.language,
            }),
        }
    }

    /// `registerTokenStyleDefault`.
    pub fn register_token_style_default(&self, selector: TokenSelector, defaults: TokenStyleDefaults) {
        lock(&self.state).token_styling_default_rules.push(SemanticTokenDefaultRule { selector, defaults });
    }

    /// `deregisterTokenStyleDefault`.
    pub fn deregister_token_style_default(&self, selector: &TokenSelector) {
        lock(&self.state).token_styling_default_rules.retain(|rule| rule.selector.id != selector.id);
    }

    /// `deregisterTokenType`.
    pub fn deregister_token_type(&self, id: &str) {
        let mut state = lock(&self.state);
        state.token_type_by_id.retain(|contribution| contribution.id != id);
        state.delete_schema_property(id);
        state.type_hierarchy = HashMap::new();
    }

    /// `deregisterTokenModifier`.
    pub fn deregister_token_modifier(&self, id: &str) {
        let mut state = lock(&self.state);
        state.token_modifier_by_id.retain(|contribution| contribution.id != id);
        state.delete_schema_property(&format!("*.{id}"));
    }

    /// `getTokenTypes`.
    pub fn get_token_types(&self) -> Vec<TokenTypeOrModifierContribution> {
        let mut types = lock(&self.state).token_type_by_id.clone();
        sort_as_object_keys(&mut types);
        types
    }

    /// `getTokenModifiers`.
    pub fn get_token_modifiers(&self) -> Vec<TokenTypeOrModifierContribution> {
        let mut modifiers = lock(&self.state).token_modifier_by_id.clone();
        sort_as_object_keys(&mut modifiers);
        modifiers
    }

    /// `getTokenStylingSchema`: o `tokenStylingSchema` com as propriedades registradas até agora.
    pub fn get_token_styling_schema(&self) -> Value {
        let properties: Map<String, Value> = lock(&self.state).schema_properties.iter().cloned().collect();
        let mut pattern_properties = Map::new();
        pattern_properties.insert(SELECTOR_PATTERN.clone(), get_styling_scheme_entry(None, None));
        json!({
            "type": "object",
            "properties": properties,
            "patternProperties": pattern_properties,
            "additionalProperties": false,
            "definitions": { "style": style_definition() },
        })
    }

    /// `getTokenStylingDefaultRules`.
    pub fn get_token_styling_default_rules(&self) -> Vec<SemanticTokenDefaultRule> {
        lock(&self.state).token_styling_default_rules.clone()
    }
}

/// Os `defaultSnippets` do `fontStyle` depois do que limpa o estilo herdado.
const FONT_STYLE_SNIPPETS: [&str; 15] = [
    "italic",
    "bold",
    "underline",
    "strikethrough",
    "italic bold",
    "italic underline",
    "italic strikethrough",
    "bold underline",
    "bold strikethrough",
    "underline strikethrough",
    "italic bold underline",
    "italic bold strikethrough",
    "italic underline strikethrough",
    "bold underline strikethrough",
    "italic bold underline strikethrough",
];

/// `tokenStylingSchema.definitions.style`.
fn style_definition() -> Value {
    let none_label = localize("schema.token.fontStyle.none", "None (clear inherited style)", &[]);
    let mut font_style_snippets = vec![json!({ "label": none_label, "bodyText": "\"\"" })];
    font_style_snippets.extend(FONT_STYLE_SNIPPETS.iter().map(|body| json!({ "body": body })));
    json!({
        "type": "object",
        "description": localize("schema.token.settings", "Colors and styles for the token.", &[]),
        "properties": {
            "foreground": {
                "type": "string",
                "description": localize("schema.token.foreground", "Foreground color for the token.", &[]),
                "format": "color-hex",
                "default": "#ff0000",
            },
            "background": {
                "type": "string",
                "deprecationMessage": localize("schema.token.background.warning", "Token background colors are currently not supported.", &[]),
            },
            "fontStyle": {
                "type": "string",
                "description": localize("schema.token.fontStyle", "Sets the all font styles of the rule: 'italic', 'bold', 'underline' or 'strikethrough' or a combination. All styles that are not listed are unset. The empty string unsets all styles.", &[]),
                "pattern": FONT_STYLE_PATTERN,
                "patternErrorMessage": localize("schema.fontStyle.error", "Font style must be 'italic', 'bold', 'underline' or 'strikethrough' or a combination. The empty string unsets all styles.", &[]),
                "defaultSnippets": font_style_snippets,
            },
            "bold": {
                "type": "boolean",
                "description": localize("schema.token.bold", "Sets or unsets the font style to bold. Note, the presence of 'fontStyle' overrides this setting.", &[]),
            },
            "italic": {
                "type": "boolean",
                "description": localize("schema.token.italic", "Sets or unsets the font style to italic. Note, the presence of 'fontStyle' overrides this setting.", &[]),
            },
            "underline": {
                "type": "boolean",
                "description": localize("schema.token.underline", "Sets or unsets the font style to underline. Note, the presence of 'fontStyle' overrides this setting.", &[]),
            },
            "strikethrough": {
                "type": "boolean",
                "description": localize("schema.token.strikethrough", "Sets or unsets the font style to strikethrough. Note, the presence of 'fontStyle' overrides this setting.", &[]),
            },
        },
        "defaultSnippets": [{ "body": { "foreground": "${1:#FF0000}", "fontStyle": "${2:bold}" } }],
    })
}

/// `toString`: os tipos registrados, os sem ponto antes dos com ponto e cada grupo em ordem alfabética.
impl fmt::Display for TokenClassificationRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut types = lock(&self.state).token_type_by_id.clone();
        types.sort_by(|a, b| {
            let category = |id: &str| u8::from(id.contains('.'));
            category(&a.id).cmp(&category(&b.id)).then_with(|| locale_compare(&a.id, &b.id))
        });
        let lines: Vec<String> = types.iter().map(|t| format!("- `{}`: {}", t.id, t.description)).collect();
        f.write_str(&lines.join("\n"))
    }
}

/// Um tipo padrão: id, chave e texto de `nls.localize`, `scopesToProbe`, `superType` e `deprecationMessage`.
struct DefaultTokenType {
    id: &'static str,
    nls_key: &'static str,
    description: &'static str,
    scopes_to_probe: &'static [&'static [&'static str]],
    super_type: Option<&'static str>,
    deprecation_message: Option<&'static str>,
}

const fn default_type(
    id: &'static str,
    nls_key: &'static str,
    description: &'static str,
    scopes_to_probe: &'static [&'static [&'static str]],
) -> DefaultTokenType {
    DefaultTokenType { id, nls_key, description, scopes_to_probe, super_type: None, deprecation_message: None }
}

/// Os tipos de token padrão, na ordem de registro.
const DEFAULT_TOKEN_TYPES: [DefaultTokenType; 24] = [
    default_type("comment", "comment", "Style for comments.", &[&["comment"]]),
    default_type("string", "string", "Style for strings.", &[&["string"]]),
    default_type("keyword", "keyword", "Style for keywords.", &[&["keyword.control"]]),
    default_type("number", "number", "Style for numbers.", &[&["constant.numeric"]]),
    default_type("regexp", "regexp", "Style for expressions.", &[&["constant.regexp"]]),
    default_type("operator", "operator", "Style for operators.", &[&["keyword.operator"]]),
    default_type("namespace", "namespace", "Style for namespaces.", &[&["entity.name.namespace"]]),
    default_type("type", "type", "Style for types.", &[&["entity.name.type"], &["support.type"]]),
    default_type("struct", "struct", "Style for structs.", &[&["entity.name.type.struct"]]),
    default_type("class", "class", "Style for classes.", &[&["entity.name.type.class"], &["support.class"]]),
    default_type("interface", "interface", "Style for interfaces.", &[&["entity.name.type.interface"]]),
    default_type("enum", "enum", "Style for enums.", &[&["entity.name.type.enum"]]),
    default_type("typeParameter", "typeParameter", "Style for type parameters.", &[&["entity.name.type.parameter"]]),
    default_type("function", "function", "Style for functions", &[&["entity.name.function"], &["support.function"]]),
    DefaultTokenType {
        super_type: Some("method"),
        deprecation_message: Some("Deprecated use `method` instead"),
        ..default_type("member", "member", "Style for member functions", &[])
    },
    default_type(
        "method",
        "method",
        "Style for method (member functions)",
        &[&["entity.name.function.member"], &["support.function"]],
    ),
    default_type("macro", "macro", "Style for macros.", &[&["entity.name.function.preprocessor"]]),
    default_type("variable", "variable", "Style for variables.", &[&["variable.other.readwrite"], &["entity.name.variable"]]),
    default_type("parameter", "parameter", "Style for parameters.", &[&["variable.parameter"]]),
    default_type("property", "property", "Style for properties.", &[&["variable.other.property"]]),
    default_type("enumMember", "enumMember", "Style for enum members.", &[&["variable.other.enummember"]]),
    default_type("event", "event", "Style for events.", &[&["variable.other.event"]]),
    default_type(
        "decorator",
        "decorator",
        "Style for decorators & annotations.",
        &[&["entity.name.decorator"], &["entity.name.function"]],
    ),
    default_type("label", "labels", "Style for labels. ", &[]),
];

/// Os modificadores de token padrão: id e texto de `nls.localize` (a chave é o próprio id).
const DEFAULT_TOKEN_MODIFIERS: [(&str, &str); 8] = [
    ("declaration", "Style for all symbol declarations."),
    ("documentation", "Style to use for references in documentation."),
    ("static", "Style to use for symbols that are static."),
    ("abstract", "Style to use for symbols that are abstract."),
    ("deprecated", "Style to use for symbols that are deprecated."),
    ("modification", "Style to use for write accesses."),
    ("async", "Style to use for symbols that are async."),
    ("readonly", "Style to use for symbols that are read-only."),
];

/// Os `registerTokenStyleDefault(selector, scopesToProbe)` avulsos, depois dos modificadores.
const DEFAULT_TOKEN_STYLE_DEFAULTS: [(&str, &[&[&str]]); 11] = [
    ("variable.readonly", &[&["variable.other.constant"]]),
    ("property.readonly", &[&["variable.other.constant.property"]]),
    ("type.defaultLibrary", &[&["support.type"]]),
    ("class.defaultLibrary", &[&["support.class"]]),
    ("interface.defaultLibrary", &[&["support.class"]]),
    ("variable.defaultLibrary", &[&["support.variable"], &["support.other.variable"]]),
    ("variable.defaultLibrary.readonly", &[&["support.constant"]]),
    ("property.defaultLibrary", &[&["support.variable.property"]]),
    ("property.defaultLibrary.readonly", &[&["support.constant.property"]]),
    ("function.defaultLibrary", &[&["support.function"]]),
    ("member.defaultLibrary", &[&["support.function"]]),
];

/// `registerTokenStyleDefault(selectorString, scopesToProbe)` do `createDefaultTokenClassificationRegistry`.
fn register_default_style(registry: &TokenClassificationRegistry, selector_string: &str, scopes_to_probe: &[&[&str]]) {
    let selector = registry.parse_token_selector(selector_string, None);
    let scopes_to_probe: Vec<ProbeScope> =
        scopes_to_probe.iter().map(|scope| scope.iter().map(|s| (*s).to_owned()).collect::<ProbeScope>()).collect();
    registry.register_token_style_default(
        selector,
        TokenStyleDefaults { scopes_to_probe: Some(scopes_to_probe), ..TokenStyleDefaults::default() },
    );
}

/// `createDefaultTokenClassificationRegistry`.
fn create_default_token_classification_registry() -> TokenClassificationRegistry {
    let registry = TokenClassificationRegistry::new();

    // default token types
    for token_type in &DEFAULT_TOKEN_TYPES {
        let description = localize(token_type.nls_key, token_type.description, &[]);
        registry
            .register_token_type(token_type.id, &description, token_type.super_type, token_type.deprecation_message)
            .expect("os ids dos tipos de token padrão são válidos");
        register_default_style(&registry, token_type.id, token_type.scopes_to_probe);
    }

    // default token modifiers
    for (id, description) in DEFAULT_TOKEN_MODIFIERS {
        registry
            .register_token_modifier(id, &localize(id, description, &[]), None)
            .expect("os ids dos modificadores de token padrão são válidos");
    }

    for (selector, scopes_to_probe) in DEFAULT_TOKEN_STYLE_DEFAULTS {
        register_default_style(&registry, selector, scopes_to_probe);
    }
    registry
}

/// `getTokenClassificationRegistry`.
pub fn get_token_classification_registry() -> &'static TokenClassificationRegistry {
    static REGISTRY: OnceLock<TokenClassificationRegistry> = OnceLock::new();
    REGISTRY.get_or_init(create_default_token_classification_registry)
}
