//! Porte de `GuideOptions`, `IGuidesOptions`, `InternalGuidesOptions` e `primitiveSet`
//! (`editorOptions.ts`, linhas 4861 a 4985).
//!
//! `boolean | 'active'` e `boolean | 'always'` são os enums gerados por `bool_or_keyword!`.
//! `primitiveSet(value, default, [true, false, keyword])` aceita só os três valores, comparados
//! como o `indexOf` (igualdade estrita): o texto `'true'` não é o booleano `true`.

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{boolean, EditorOption, EditorOptionImpl, Opt};

/// Declara `boolean | '<keyword>'` com o `primitiveSet` e a conversão para o esquema.
macro_rules! bool_or_keyword {
    ($(#[$meta:meta])* pub enum $name:ident { $variant:ident => $keyword:literal }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            Bool(bool),
            $variant,
        }

        impl $name {
            /// A string do original para a variante sem booleano.
            pub const KEYWORD: &'static str = $keyword;

            /// `primitiveSet(value, defaultValue, [true, false, keyword])`: só os três valores
            /// passam; o resto dá o padrão.
            pub fn primitive_set(value: &Dynamic, default_value: Self) -> Self {
                match value {
                    Dynamic::Bool(value) => Self::Bool(*value),
                    Dynamic::String(text) if *text == $keyword => Self::$variant,
                    _ => default_value,
                }
            }

            /// O valor como aparece no esquema (`true`, `false` ou a string).
            pub fn to_dynamic(self) -> Dynamic {
                match self {
                    Self::Bool(value) => Dynamic::Bool(value),
                    Self::$variant => Dynamic::string($keyword),
                }
            }
        }
    };
}

bool_or_keyword! {
    /// `boolean | 'active'`.
    pub enum BoolOrActive { Active => "active" }
}

bool_or_keyword! {
    /// `boolean | 'always'`.
    pub enum BoolOrAlways { Always => "always" }
}

/// `InternalGuidesOptions` (`Readonly<Required<IGuidesOptions>>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InternalGuidesOptions {
    /// Desenha as guias de pares de colchetes (padrão `false`).
    pub bracket_pairs: BoolOrActive,
    /// Desenha as guias horizontais dos pares de colchetes (padrão `'active'`).
    pub bracket_pairs_horizontal: BoolOrActive,
    /// Destaca o par de colchetes ativo (padrão `true`).
    pub highlight_active_bracket_pair: bool,
    /// Desenha as guias de indentação (padrão `true`).
    pub indentation: bool,
    /// Destaca a guia de indentação ativa (padrão `true`).
    pub highlight_active_indentation: BoolOrAlways,
}

/// Chave tipada da opção `guides`.
pub const GUIDES: Opt<InternalGuidesOptions> = Opt::new(EditorOption::Guides);

/// `{ type: ['boolean', 'string'], enum: [true, keyword, false], enumDescriptions, default, description }`.
fn tri_state_entry(keyword: &str, default: Dynamic, enum_descriptions: [&str; 3], description: &str) -> Dynamic {
    Dynamic::object([
        ("type", Dynamic::array([Dynamic::string("boolean"), Dynamic::string("string")])),
        ("enum", Dynamic::array([Dynamic::Bool(true), Dynamic::string(keyword), Dynamic::Bool(false)])),
        ("enumDescriptions", Dynamic::array(enum_descriptions.map(Dynamic::string))),
        ("default", default),
        ("description", Dynamic::string(description)),
    ])
}

/// `{ type: 'boolean', default, description }`.
fn boolean_entry(default: bool, description: &str) -> Dynamic {
    Dynamic::object([
        ("type", Dynamic::string("boolean")),
        ("default", Dynamic::Bool(default)),
        ("description", Dynamic::string(description)),
    ])
}

/// `GuideOptions`.
pub struct GuideOptions {
    default_value: InternalGuidesOptions,
    schema: Dynamic,
}

impl GuideOptions {
    pub fn new() -> Self {
        let defaults = InternalGuidesOptions {
            bracket_pairs: BoolOrActive::Bool(false),
            bracket_pairs_horizontal: BoolOrActive::Active,
            highlight_active_bracket_pair: true,
            indentation: true,
            highlight_active_indentation: BoolOrAlways::Bool(true),
        };
        let schema = Dynamic::object([
            (
                "editor.guides.bracketPairs",
                tri_state_entry(
                    BoolOrActive::KEYWORD,
                    defaults.bracket_pairs.to_dynamic(),
                    [
                        "Enables bracket pair guides.",
                        "Enables bracket pair guides only for the active bracket pair.",
                        "Disables bracket pair guides.",
                    ],
                    "Controls whether bracket pair guides are enabled or not.",
                ),
            ),
            (
                "editor.guides.bracketPairsHorizontal",
                tri_state_entry(
                    BoolOrActive::KEYWORD,
                    defaults.bracket_pairs_horizontal.to_dynamic(),
                    [
                        "Enables horizontal guides as addition to vertical bracket pair guides.",
                        "Enables horizontal guides only for the active bracket pair.",
                        "Disables horizontal bracket pair guides.",
                    ],
                    "Controls whether horizontal bracket pair guides are enabled or not.",
                ),
            ),
            (
                "editor.guides.highlightActiveBracketPair",
                boolean_entry(defaults.highlight_active_bracket_pair, "Controls whether the editor should highlight the active bracket pair."),
            ),
            ("editor.guides.indentation", boolean_entry(defaults.indentation, "Controls whether the editor should render indent guides.")),
            (
                "editor.guides.highlightActiveIndentation",
                tri_state_entry(
                    BoolOrAlways::KEYWORD,
                    defaults.highlight_active_indentation.to_dynamic(),
                    [
                        "Highlights the active indent guide.",
                        "Highlights the active indent guide even if bracket guides are highlighted.",
                        "Do not highlight the active indent guide.",
                    ],
                    "Controls whether the editor should highlight the active indent guide.",
                ),
            ),
        ]);
        Self { default_value: defaults, schema }
    }
}

impl Default for GuideOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for GuideOptions {
    type Value = InternalGuidesOptions;

    fn id(&self) -> EditorOption {
        EditorOption::Guides
    }

    fn name(&self) -> &str {
        "guides"
    }

    fn default_value(&self) -> InternalGuidesOptions {
        self.default_value
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> InternalGuidesOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value;
        }
        let defaults = &self.default_value;
        InternalGuidesOptions {
            bracket_pairs: BoolOrActive::primitive_set(&input.get_str("bracketPairs"), defaults.bracket_pairs),
            bracket_pairs_horizontal: BoolOrActive::primitive_set(&input.get_str("bracketPairsHorizontal"), defaults.bracket_pairs_horizontal),
            highlight_active_bracket_pair: boolean(&input.get_str("highlightActiveBracketPair"), defaults.highlight_active_bracket_pair),
            indentation: boolean(&input.get_str("indentation"), defaults.indentation),
            highlight_active_indentation: BoolOrAlways::primitive_set(&input.get_str("highlightActiveIndentation"), defaults.highlight_active_indentation),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Dynamic {
        Dynamic::string(value)
    }

    fn validate(entries: Vec<(&str, Dynamic)>) -> InternalGuidesOptions {
        EditorOptionImpl::validate(&GuideOptions::new(), &Dynamic::object(entries))
    }

    fn defaults() -> InternalGuidesOptions {
        EditorOptionImpl::default_value(&GuideOptions::new())
    }

    #[test]
    fn defaults_match_the_original() {
        let defaults = defaults();
        assert_eq!(defaults.bracket_pairs, BoolOrActive::Bool(false));
        assert_eq!(defaults.bracket_pairs_horizontal, BoolOrActive::Active);
        assert!(defaults.highlight_active_bracket_pair && defaults.indentation);
        assert_eq!(defaults.highlight_active_indentation, BoolOrAlways::Bool(true));
        assert_eq!(validate(vec![]), defaults);
        let option = GuideOptions::new();
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), Dynamic::Number(1.0), text("x"), text("")] {
            assert_eq!(EditorOptionImpl::validate(&option, &input), defaults);
        }
    }

    #[test]
    fn bracket_pairs_accept_booleans_and_active_only() {
        let pairs = |value: Dynamic| validate(vec![("bracketPairs", value)]).bracket_pairs;
        assert_eq!(pairs(Dynamic::Bool(true)), BoolOrActive::Bool(true));
        assert_eq!(pairs(Dynamic::Bool(false)), BoolOrActive::Bool(false));
        assert_eq!(pairs(text("active")), BoolOrActive::Active);
        // O texto `'true'` e `'always'` não estão na lista: padrão `false`.
        assert_eq!(pairs(text("true")), BoolOrActive::Bool(false));
        assert_eq!(pairs(text("always")), BoolOrActive::Bool(false));
        assert_eq!(pairs(Dynamic::Number(1.0)), BoolOrActive::Bool(false));
        assert_eq!(pairs(Dynamic::Null), BoolOrActive::Bool(false));

        let horizontal = |value: Dynamic| validate(vec![("bracketPairsHorizontal", value)]).bracket_pairs_horizontal;
        assert_eq!(horizontal(Dynamic::Bool(true)), BoolOrActive::Bool(true));
        assert_eq!(horizontal(Dynamic::Bool(false)), BoolOrActive::Bool(false));
        assert_eq!(horizontal(text("active")), BoolOrActive::Active);
        // Inválido cai no padrão `'active'`.
        assert_eq!(horizontal(text("false")), BoolOrActive::Active);
        assert_eq!(horizontal(Dynamic::Number(0.0)), BoolOrActive::Active);
    }

    #[test]
    fn highlight_active_indentation_accepts_booleans_and_always_only() {
        let indentation = |value: Dynamic| validate(vec![("highlightActiveIndentation", value)]).highlight_active_indentation;
        assert_eq!(indentation(Dynamic::Bool(false)), BoolOrAlways::Bool(false));
        assert_eq!(indentation(Dynamic::Bool(true)), BoolOrAlways::Bool(true));
        assert_eq!(indentation(text("always")), BoolOrAlways::Always);
        assert_eq!(indentation(text("active")), BoolOrAlways::Bool(true));
        assert_eq!(indentation(text("false")), BoolOrAlways::Bool(true));
        assert_eq!(indentation(Dynamic::Null), BoolOrAlways::Bool(true));
    }

    #[test]
    fn plain_booleans_use_the_boolean_helper() {
        let value = validate(vec![("highlightActiveBracketPair", text("false")), ("indentation", Dynamic::Number(0.0))]);
        assert!(!value.highlight_active_bracket_pair && !value.indentation);
        let value = validate(vec![("highlightActiveBracketPair", Dynamic::Null)]);
        assert!(!value.highlight_active_bracket_pair && value.indentation);
        let value = validate(vec![("indentation", Dynamic::Bool(true)), ("highlightActiveBracketPair", Dynamic::Bool(true))]);
        assert!(value.highlight_active_bracket_pair && value.indentation);
    }

    #[test]
    fn schema_keeps_the_original_keys_and_order() {
        let option = GuideOptions::new();
        let schema = EditorOptionImpl::schema(&option).expect("sem esquema");
        assert_eq!(
            schema.own_keys(),
            vec![
                "editor.guides.bracketPairs",
                "editor.guides.bracketPairsHorizontal",
                "editor.guides.highlightActiveBracketPair",
                "editor.guides.indentation",
                "editor.guides.highlightActiveIndentation",
            ]
        );
        let pairs = schema.get_str("editor.guides.bracketPairs");
        assert_eq!(pairs.own_keys(), vec!["type", "enum", "enumDescriptions", "default", "description"]);
        assert_eq!(pairs.get_str("type").get_str("length"), Dynamic::Number(2.0));
        assert_eq!(pairs.get_str("enum").get_str("0"), Dynamic::Bool(true));
        assert_eq!(pairs.get_str("enum").get_str("1"), text("active"));
        assert_eq!(pairs.get_str("enum").get_str("2"), Dynamic::Bool(false));
        assert_eq!(pairs.get_str("default"), Dynamic::Bool(false));
        let horizontal = schema.get_str("editor.guides.bracketPairsHorizontal");
        assert_eq!(horizontal.get_str("default"), text("active"));
        assert_eq!(horizontal.get_str("enumDescriptions").get_str("length"), Dynamic::Number(3.0));
        let flag = schema.get_str("editor.guides.indentation");
        assert_eq!(flag.own_keys(), vec!["type", "default", "description"]);
        assert_eq!(flag.get_str("default"), Dynamic::Bool(true));
        let highlight = schema.get_str("editor.guides.highlightActiveIndentation");
        assert_eq!(highlight.own_keys(), vec!["type", "enum", "enumDescriptions", "default", "description"]);
        assert_eq!(highlight.get_str("enum").get_str("1"), text("always"));
        assert_eq!(highlight.get_str("default"), Dynamic::Bool(true));
    }
}
