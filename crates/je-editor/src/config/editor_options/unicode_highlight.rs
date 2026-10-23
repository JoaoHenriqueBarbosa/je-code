//! Porte de `UnicodeHighlight`, `IUnicodeHighlightOptions` e `InternalUnicodeHighlightOptions`
//! (`editorOptions.ts`, linhas 4307 a 4497).
//!
//! `Record<string, true>` (`allowedCharacters` e `allowedLocales`) é o [`TrueRecord`]: as chaves na
//! ordem de propriedades do JavaScript, sem repetição. A igualdade dele é a de `objects.equals`
//! (mesmo conjunto de chaves, sem importar a ordem), que é o que o `applyUpdate` consulta.

use je_base::dynamic::{Dynamic, Truthy};
use je_base::JsString;

use crate::config::editor_options::{
    apply_update, boolean, strings_array, ApplyUpdateResult, EditorOption, EditorOptionImpl, Opt,
};

/// `inUntrustedWorkspace`: o valor de texto de `InUntrustedWorkspace`.
pub const IN_UNTRUSTED_WORKSPACE: &str = "inUntrustedWorkspace";

/// `unicodeHighlightConfigKeys`.
pub struct UnicodeHighlightConfigKeys {
    pub allowed_characters: &'static str,
    pub invisible_characters: &'static str,
    pub non_basic_ascii: &'static str,
    pub ambiguous_characters: &'static str,
    pub include_comments: &'static str,
    pub include_strings: &'static str,
    pub allowed_locales: &'static str,
}

/// `unicodeHighlightConfigKeys`.
pub const UNICODE_HIGHLIGHT_CONFIG_KEYS: UnicodeHighlightConfigKeys = UnicodeHighlightConfigKeys {
    allowed_characters: "editor.unicodeHighlight.allowedCharacters",
    invisible_characters: "editor.unicodeHighlight.invisibleCharacters",
    non_basic_ascii: "editor.unicodeHighlight.nonBasicASCII",
    ambiguous_characters: "editor.unicodeHighlight.ambiguousCharacters",
    include_comments: "editor.unicodeHighlight.includeComments",
    include_strings: "editor.unicodeHighlight.includeStrings",
    allowed_locales: "editor.unicodeHighlight.allowedLocales",
};

/// `boolean | InUntrustedWorkspace`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOrInUntrustedWorkspace {
    Bool(bool),
    InUntrustedWorkspace,
}

impl BoolOrInUntrustedWorkspace {
    /// `primitiveSet<boolean | InUntrustedWorkspace>(value, default, [true, false, inUntrustedWorkspace])`:
    /// só os três valores passam, comparados como o `indexOf` (igualdade estrita); o resto dá o padrão.
    fn primitive_set(value: &Dynamic, default_value: Self) -> Self {
        match value {
            Dynamic::Bool(value) => Self::Bool(*value),
            Dynamic::String(text) if *text == IN_UNTRUSTED_WORKSPACE => Self::InUntrustedWorkspace,
            _ => default_value,
        }
    }

    /// O valor como aparece no esquema (`true`, `false` ou `'inUntrustedWorkspace'`).
    fn to_dynamic(self) -> Dynamic {
        match self {
            Self::Bool(value) => Dynamic::Bool(value),
            Self::InUntrustedWorkspace => Dynamic::string(IN_UNTRUSTED_WORKSPACE),
        }
    }
}

/// `Record<string, true>`.
#[derive(Clone, Debug)]
pub struct TrueRecord {
    keys: Vec<JsString>,
}

impl TrueRecord {
    /// O registro com as chaves dadas; repetidas contam uma vez e a ordem é a de propriedades do
    /// JavaScript (índices de array primeiro).
    pub fn from_keys<K: Into<JsString>>(keys: impl IntoIterator<Item = K>) -> Self {
        let object = Dynamic::object(keys.into_iter().map(|key| (key, Dynamic::Bool(true))));
        Self { keys: object.own_keys() }
    }

    /// `Object.keys(record)`.
    pub fn keys(&self) -> &[JsString] {
        &self.keys
    }

    pub fn contains(&self, key: &JsString) -> bool {
        self.keys.contains(key)
    }

    /// O registro como objeto `Dynamic` (`{ [key]: true }`).
    pub fn to_dynamic(&self) -> Dynamic {
        Dynamic::object(self.keys.iter().map(|key| (key.clone(), Dynamic::Bool(true))))
    }
}

/// `objects.equals`: as mesmas chaves, em qualquer ordem.
impl PartialEq for TrueRecord {
    fn eq(&self, other: &Self) -> bool {
        self.keys.len() == other.keys.len() && self.keys.iter().all(|key| other.contains(key))
    }
}

/// `InternalUnicodeHighlightOptions` (`Required<Readonly<IUnicodeHighlightOptions>>`).
#[derive(Clone, Debug, PartialEq)]
pub struct InternalUnicodeHighlightOptions {
    /// Destaca todo caractere fora do ASCII básico (U+0020 a U+007E, tab, line feed e carriage return).
    pub non_basic_ascii: BoolOrInUntrustedWorkspace,
    /// Destaca caracteres que só ocupam espaço ou não têm largura.
    pub invisible_characters: bool,
    /// Destaca caracteres que se confundem com ASCII básico, menos os comuns no idioma do usuário.
    pub ambiguous_characters: bool,
    /// Também destaca dentro de comentários.
    pub include_comments: BoolOrInUntrustedWorkspace,
    /// Também destaca dentro de strings.
    pub include_strings: BoolOrInUntrustedWorkspace,
    /// Caracteres permitidos, que não são destacados.
    pub allowed_characters: TrueRecord,
    /// Idiomas permitidos (e `_os`, `_vscode`): os caracteres comuns neles não são destacados.
    pub allowed_locales: TrueRecord,
}

/// Chave tipada da opção `unicodeHighlighting`.
pub const UNICODE_HIGHLIGHTING: Opt<InternalUnicodeHighlightOptions> = Opt::new(EditorOption::UnicodeHighlighting);

/// `{ restricted: true, type: ['boolean', 'string'], enum: [true, false, inUntrustedWorkspace], default, description }`.
fn trust_entry(default: BoolOrInUntrustedWorkspace, description: &str) -> Dynamic {
    Dynamic::object([
        ("restricted", Dynamic::Bool(true)),
        ("type", strings_array(&["boolean", "string"])),
        (
            "enum",
            Dynamic::array([
                Dynamic::Bool(true),
                Dynamic::Bool(false),
                Dynamic::string(IN_UNTRUSTED_WORKSPACE),
            ]),
        ),
        ("default", default.to_dynamic()),
        ("description", Dynamic::string(description)),
    ])
}

/// `{ restricted: true, type: 'boolean', default, description }`.
fn boolean_entry(default: bool, description: &str) -> Dynamic {
    Dynamic::object([
        ("restricted", Dynamic::Bool(true)),
        ("type", Dynamic::string("boolean")),
        ("default", Dynamic::Bool(default)),
        ("description", Dynamic::string(description)),
    ])
}

/// `{ type: 'boolean' }`, o `additionalProperties` dos dois registros.
fn additional_boolean_properties() -> Dynamic {
    Dynamic::object([("type", Dynamic::string("boolean"))])
}

/// `UnicodeHighlight`.
pub struct UnicodeHighlight {
    default_value: InternalUnicodeHighlightOptions,
    schema: Dynamic,
}

impl UnicodeHighlight {
    pub fn new() -> Self {
        let defaults = InternalUnicodeHighlightOptions {
            non_basic_ascii: BoolOrInUntrustedWorkspace::InUntrustedWorkspace,
            invisible_characters: true,
            ambiguous_characters: true,
            include_comments: BoolOrInUntrustedWorkspace::InUntrustedWorkspace,
            include_strings: BoolOrInUntrustedWorkspace::Bool(true),
            allowed_characters: TrueRecord::from_keys::<&str>([]),
            allowed_locales: TrueRecord::from_keys(["_os", "_vscode"]),
        };
        let keys = &UNICODE_HIGHLIGHT_CONFIG_KEYS;
        let schema = Dynamic::object([
            (
                keys.non_basic_ascii,
                trust_entry(
                    defaults.non_basic_ascii,
                    "Controls whether all non-basic ASCII characters are highlighted. Only characters between U+0020 and U+007E, tab, line-feed and carriage-return are considered basic ASCII.",
                ),
            ),
            (
                keys.invisible_characters,
                boolean_entry(
                    defaults.invisible_characters,
                    "Controls whether characters that just reserve space or have no width at all are highlighted.",
                ),
            ),
            (
                keys.ambiguous_characters,
                boolean_entry(
                    defaults.ambiguous_characters,
                    "Controls whether characters are highlighted that can be confused with basic ASCII characters, except those that are common in the current user locale.",
                ),
            ),
            (
                keys.include_comments,
                trust_entry(
                    defaults.include_comments,
                    "Controls whether characters in comments should also be subject to Unicode highlighting.",
                ),
            ),
            (
                keys.include_strings,
                trust_entry(
                    defaults.include_strings,
                    "Controls whether characters in strings should also be subject to Unicode highlighting.",
                ),
            ),
            (
                keys.allowed_characters,
                Dynamic::object([
                    ("restricted", Dynamic::Bool(true)),
                    ("type", Dynamic::string("object")),
                    ("default", defaults.allowed_characters.to_dynamic()),
                    ("description", Dynamic::string("Defines allowed characters that are not being highlighted.")),
                    ("additionalProperties", additional_boolean_properties()),
                ]),
            ),
            (
                keys.allowed_locales,
                Dynamic::object([
                    ("restricted", Dynamic::Bool(true)),
                    ("type", Dynamic::string("object")),
                    ("additionalProperties", additional_boolean_properties()),
                    ("default", defaults.allowed_locales.to_dynamic()),
                    (
                        "description",
                        Dynamic::string("Unicode characters that are common in allowed locales are not being highlighted."),
                    ),
                ]),
            ),
        ]);
        Self { default_value: defaults, schema }
    }

    /// `validateBooleanMap`: só entram as chaves cujo valor é exatamente `true`; o que não é objeto
    /// dá o padrão.
    fn validate_boolean_map(map: &Dynamic, default_value: &TrueRecord) -> TrueRecord {
        if map.type_of() != "object" || !map.is_truthy() {
            return default_value.clone();
        }
        TrueRecord::from_keys(
            map.own_entries()
                .into_iter()
                .filter(|(_, value)| matches!(value, Dynamic::Bool(true)))
                .map(|(key, _)| key),
        )
    }
}

impl Default for UnicodeHighlight {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for UnicodeHighlight {
    type Value = InternalUnicodeHighlightOptions;

    fn id(&self) -> EditorOption {
        EditorOption::UnicodeHighlighting
    }

    fn name(&self) -> &str {
        "unicodeHighlight"
    }

    fn default_value(&self) -> InternalUnicodeHighlightOptions {
        self.default_value.clone()
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    /// `allowedCharacters` e `allowedLocales` são tratados de forma atômica: se mudaram, o registro
    /// novo substitui o antigo inteiro (os objetos do original são sempre verdadeiros, então a
    /// condição `update.allowedX && value` é só `value` existir).
    fn apply_update(
        &self,
        value: Option<&InternalUnicodeHighlightOptions>,
        update: InternalUnicodeHighlightOptions,
    ) -> ApplyUpdateResult<InternalUnicodeHighlightOptions> {
        let mut did_change = false;
        let mut value = value.cloned();
        if let Some(current) = value.as_mut() {
            if current.allowed_characters != update.allowed_characters {
                current.allowed_characters = update.allowed_characters.clone();
                did_change = true;
            }
            if current.allowed_locales != update.allowed_locales {
                current.allowed_locales = update.allowed_locales.clone();
                did_change = true;
            }
        }

        let result = apply_update(value.as_ref(), update);
        if did_change {
            return ApplyUpdateResult { new_value: result.new_value, did_change: true };
        }
        result
    }

    fn validate(&self, input: &Dynamic) -> InternalUnicodeHighlightOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value.clone();
        }
        let defaults = &self.default_value;
        // O original passa `inUntrustedWorkspace` como padrão dos três campos, inclusive `includeStrings`.
        let untrusted = BoolOrInUntrustedWorkspace::InUntrustedWorkspace;
        InternalUnicodeHighlightOptions {
            non_basic_ascii: BoolOrInUntrustedWorkspace::primitive_set(&input.get_str("nonBasicASCII"), untrusted),
            invisible_characters: boolean(&input.get_str("invisibleCharacters"), defaults.invisible_characters),
            ambiguous_characters: boolean(&input.get_str("ambiguousCharacters"), defaults.ambiguous_characters),
            include_comments: BoolOrInUntrustedWorkspace::primitive_set(&input.get_str("includeComments"), untrusted),
            include_strings: BoolOrInUntrustedWorkspace::primitive_set(&input.get_str("includeStrings"), untrusted),
            allowed_characters: Self::validate_boolean_map(&input.get_str("allowedCharacters"), &defaults.allowed_characters),
            allowed_locales: Self::validate_boolean_map(&input.get_str("allowedLocales"), &defaults.allowed_locales),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BoolOrInUntrustedWorkspace::{Bool, InUntrustedWorkspace};
    use super::*;

    fn text(value: &str) -> Dynamic {
        Dynamic::string(value)
    }

    fn num(value: f64) -> Dynamic {
        Dynamic::Number(value)
    }

    fn validate(entries: Vec<(&str, Dynamic)>) -> InternalUnicodeHighlightOptions {
        EditorOptionImpl::validate(&UnicodeHighlight::new(), &Dynamic::object(entries))
    }

    fn defaults() -> InternalUnicodeHighlightOptions {
        EditorOptionImpl::default_value(&UnicodeHighlight::new())
    }

    fn record(keys: &[&str]) -> TrueRecord {
        TrueRecord::from_keys(keys.iter().copied())
    }

    fn key_list(record: &TrueRecord) -> Vec<String> {
        record.keys().iter().map(JsString::to_string_lossy).collect()
    }

    fn with_records(characters: &[&str], locales: &[&str]) -> InternalUnicodeHighlightOptions {
        InternalUnicodeHighlightOptions {
            allowed_characters: record(characters),
            allowed_locales: record(locales),
            ..defaults()
        }
    }

    fn flags(entries: Vec<(&str, bool)>) -> Dynamic {
        Dynamic::object(entries.into_iter().map(|(key, value)| (key, Dynamic::Bool(value))))
    }

    #[test]
    fn defaults_match_the_original() {
        let defaults = defaults();
        assert_eq!(defaults.non_basic_ascii, InUntrustedWorkspace);
        assert_eq!(defaults.include_comments, InUntrustedWorkspace);
        assert_eq!(defaults.include_strings, Bool(true));
        assert!(defaults.invisible_characters && defaults.ambiguous_characters);
        assert!(defaults.allowed_characters.keys().is_empty());
        assert_eq!(key_list(&defaults.allowed_locales), vec!["_os", "_vscode"]);
        // `includeStrings` padrão é `true`, mas `validate` sem campo cai em `inUntrustedWorkspace`.
        let validated = validate(vec![]);
        assert_eq!(validated.include_strings, InUntrustedWorkspace);
        assert_eq!(validated.allowed_locales, defaults.allowed_locales);
        assert_eq!(validated.allowed_characters, defaults.allowed_characters);
        let option = UnicodeHighlight::new();
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), num(1.0), text("x"), num(0.0)] {
            assert_eq!(EditorOptionImpl::validate(&option, &input), defaults);
        }
    }

    #[test]
    fn trust_fields_accept_booleans_and_the_marker_only() {
        let value = validate(vec![("nonBasicASCII", Dynamic::Bool(true)), ("includeComments", Dynamic::Bool(false)), ("includeStrings", Dynamic::Bool(false))]);
        assert_eq!((value.non_basic_ascii, value.include_comments, value.include_strings), (Bool(true), Bool(false), Bool(false)));
        let value = validate(vec![("nonBasicASCII", text("inUntrustedWorkspace")), ("includeStrings", text("inUntrustedWorkspace"))]);
        assert_eq!((value.non_basic_ascii, value.include_strings), (InUntrustedWorkspace, InUntrustedWorkspace));
        // O texto `'true'` não é o booleano `true` no `indexOf`: cai no padrão.
        let value = validate(vec![("nonBasicASCII", text("true")), ("includeComments", num(1.0)), ("includeStrings", Dynamic::Null)]);
        assert_eq!((value.non_basic_ascii, value.include_comments, value.include_strings), (InUntrustedWorkspace, InUntrustedWorkspace, InUntrustedWorkspace));
    }

    #[test]
    fn plain_booleans_use_the_boolean_helper() {
        let value = validate(vec![("invisibleCharacters", text("false")), ("ambiguousCharacters", num(0.0))]);
        assert!(!value.invisible_characters && !value.ambiguous_characters);
        let value = validate(vec![("invisibleCharacters", Dynamic::Null)]);
        assert!(!value.invisible_characters && value.ambiguous_characters);
    }

    #[test]
    fn boolean_maps_keep_only_true_values() {
        let value = validate(vec![
            ("allowedCharacters", flags(vec![("a", true), ("b", false), ("c", true)])),
            ("allowedLocales", Dynamic::object([("pt-BR", Dynamic::Bool(true)), ("x", text("true")), ("y", num(1.0))])),
        ]);
        assert_eq!(key_list(&value.allowed_characters), vec!["a", "c"]);
        assert_eq!(key_list(&value.allowed_locales), vec!["pt-BR"]);
        // Objeto vazio é válido e dá registro vazio (não o padrão).
        let value = validate(vec![("allowedLocales", Dynamic::object::<&str>([]))]);
        assert!(value.allowed_locales.keys().is_empty());
        // O que não é objeto (ou é `null`) dá o padrão do campo.
        for bad in [Dynamic::Null, num(1.0), text("a"), Dynamic::Bool(true)] {
            let value = validate(vec![("allowedCharacters", bad.clone()), ("allowedLocales", bad)]);
            assert_eq!(key_list(&value.allowed_locales), vec!["_os", "_vscode"]);
            assert!(value.allowed_characters.keys().is_empty());
        }
        // Um array é objeto: as chaves são os índices.
        let value = validate(vec![("allowedCharacters", Dynamic::array([Dynamic::Bool(true), Dynamic::Bool(false), Dynamic::Bool(true)]))]);
        assert_eq!(key_list(&value.allowed_characters), vec!["0", "2"]);
    }

    #[test]
    fn true_record_equality_ignores_order_and_duplicates() {
        assert_eq!(record(&["a", "b"]), record(&["b", "a"]));
        assert_eq!(record(&["a", "a"]), record(&["a"]));
        assert_ne!(record(&["a"]), record(&["a", "b"]));
        assert_ne!(record(&["a", "b"]), record(&["a", "c"]));
        assert_eq!(record(&[]), record(&[]));
        // Chaves que são índices de array vêm primeiro, em ordem numérica.
        assert_eq!(key_list(&record(&["b", "10", "2"])), vec!["2", "10", "b"]);
        assert_eq!(record(&["a"]).to_dynamic().own_keys(), vec!["a"]);
    }

    #[test]
    fn apply_update_replaces_changed_records_atomically() {
        let option = UnicodeHighlight::new();
        let current = with_records(&["a"], &["_os"]);

        // Registro de caracteres mudou: o novo substitui, e a mudança é registrada.
        let update = with_records(&["a", "b"], &["_os"]);
        let result = EditorOptionImpl::apply_update(&option, Some(&current), update.clone());
        assert!(result.did_change);
        assert_eq!(result.new_value, update);

        // Registro de idiomas mudou.
        let update = with_records(&["a"], &["_vscode"]);
        assert!(EditorOptionImpl::apply_update(&option, Some(&current), update).did_change);

        // Só a ordem das chaves mudou: igual para `objects.equals`, nada mudou.
        let before = with_records(&["a", "b"], &["_os", "_vscode"]);
        let reordered = with_records(&["b", "a"], &["_vscode", "_os"]);
        assert!(!EditorOptionImpl::apply_update(&option, Some(&before), reordered).did_change);

        // Tudo igual: sem mudança.
        assert!(!EditorOptionImpl::apply_update(&option, Some(&current), current.clone()).did_change);

        // Outro campo mudou e os registros não: cai na comparação do valor inteiro.
        let update = InternalUnicodeHighlightOptions { invisible_characters: false, ..current.clone() };
        assert!(EditorOptionImpl::apply_update(&option, Some(&current), update).did_change);

        // Sem valor anterior: só a comparação do valor inteiro, que muda.
        let result = EditorOptionImpl::apply_update(&option, None, current.clone());
        assert!(result.did_change);
        assert_eq!(result.new_value, current);
    }

    #[test]
    fn schema_keeps_the_original_shape_and_order() {
        let option = UnicodeHighlight::new();
        let schema = EditorOptionImpl::schema(&option).expect("sem esquema");
        assert_eq!(
            schema.own_keys(),
            vec![
                "editor.unicodeHighlight.nonBasicASCII",
                "editor.unicodeHighlight.invisibleCharacters",
                "editor.unicodeHighlight.ambiguousCharacters",
                "editor.unicodeHighlight.includeComments",
                "editor.unicodeHighlight.includeStrings",
                "editor.unicodeHighlight.allowedCharacters",
                "editor.unicodeHighlight.allowedLocales",
            ]
        );
        let non_basic = schema.get_str(UNICODE_HIGHLIGHT_CONFIG_KEYS.non_basic_ascii);
        assert_eq!(non_basic.own_keys(), vec!["restricted", "type", "enum", "default", "description"]);
        assert_eq!(non_basic.get_str("restricted"), Dynamic::Bool(true));
        assert_eq!(non_basic.get_str("type").get_str("length"), num(2.0));
        assert_eq!(non_basic.get_str("enum").get_str("2"), text("inUntrustedWorkspace"));
        assert_eq!(non_basic.get_str("default"), text("inUntrustedWorkspace"));
        assert_eq!(schema.get_str(UNICODE_HIGHLIGHT_CONFIG_KEYS.include_strings).get_str("default"), Dynamic::Bool(true));
        let invisible = schema.get_str(UNICODE_HIGHLIGHT_CONFIG_KEYS.invisible_characters);
        assert_eq!(invisible.own_keys(), vec!["restricted", "type", "default", "description"]);
        let characters = schema.get_str(UNICODE_HIGHLIGHT_CONFIG_KEYS.allowed_characters);
        assert_eq!(characters.own_keys(), vec!["restricted", "type", "default", "description", "additionalProperties"]);
        assert!(characters.get_str("default").own_keys().is_empty());
        assert_eq!(characters.get_str("additionalProperties").get_str("type"), text("boolean"));
        let locales = schema.get_str(UNICODE_HIGHLIGHT_CONFIG_KEYS.allowed_locales);
        assert_eq!(locales.own_keys(), vec!["restricted", "type", "additionalProperties", "default", "description"]);
        assert_eq!(locales.get_str("default").own_keys(), vec!["_os", "_vscode"]);
        assert_eq!(locales.get_str("default").get_str("_os"), Dynamic::Bool(true));
    }
}
