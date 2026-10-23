//! Porte de `WrappingIndent` e `WrappingIndentOption` (`editorOptions.ts`, linhas 5600 a 5666).

use je_base::dynamic::Dynamic;

use crate::config::editor_options::{strings_array, AccessibilitySupport, ComputedEditorOptions, EditorOption, EditorOptionImpl, EnvironmentalOptions, Opt};

/// Chave tipada de `wrappingIndent`.
pub const WRAPPING_INDENT: Opt<WrappingIndent> = Opt::new(EditorOption::WrappingIndent);

/// `WrappingIndent`: como indentar as linhas quebradas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrappingIndent {
    /// Sem indentação: as linhas quebradas começam na coluna 1.
    None = 0,
    /// Mesma indentação da linha de origem.
    Same = 1,
    /// Um nível a mais que a linha de origem.
    Indent = 2,
    /// Dois níveis a mais que a linha de origem.
    DeepIndent = 3,
}

impl WrappingIndent {
    /// As strings aceitas (`'none' | 'same' | 'indent' | 'deepIndent'`), na ordem do esquema.
    pub const ALLOWED: &'static [&'static str] = &["none", "same", "indent", "deepIndent"];

    /// A variante da string, ou `None` se não é uma das quatro.
    pub fn from_text(text: &str) -> Option<Self> {
        match text {
            "none" => Some(Self::None),
            "same" => Some(Self::Same),
            "indent" => Some(Self::Indent),
            "deepIndent" => Some(Self::DeepIndent),
            _ => None,
        }
    }
}

/// `WrappingIndentOption`.
pub struct WrappingIndentOption {
    schema: Dynamic,
}

impl WrappingIndentOption {
    pub fn new() -> Self {
        let property = Dynamic::object([
            ("type", Dynamic::string("string")),
            ("enum", strings_array(WrappingIndent::ALLOWED)),
            (
                "enumDescriptions",
                strings_array(&[
                    "No indentation. Wrapped lines begin at column 1.",
                    "Wrapped lines get the same indentation as the parent.",
                    "Wrapped lines get +1 indentation toward the parent.",
                    "Wrapped lines get +2 indentation toward the parent.",
                ]),
            ),
            ("description", Dynamic::string("Controls the indentation of wrapped lines.")),
            ("default", Dynamic::string("same")),
        ]);
        Self { schema: Dynamic::object([("editor.wrappingIndent", property)]) }
    }
}

impl Default for WrappingIndentOption {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for WrappingIndentOption {
    type Value = WrappingIndent;

    fn id(&self) -> EditorOption {
        EditorOption::WrappingIndent
    }

    fn name(&self) -> &str {
        "wrappingIndent"
    }

    fn default_value(&self) -> WrappingIndent {
        WrappingIndent::Same
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> WrappingIndent {
        // `switch (input)` compara por igualdade estrita: só strings casam.
        match input {
            Dynamic::String(text) => WrappingIndent::from_text(&text.to_string_lossy()).unwrap_or(WrappingIndent::Same),
            _ => WrappingIndent::Same,
        }
    }

    fn compute(&self, _env: &EnvironmentalOptions, options: &ComputedEditorOptions, value: &WrappingIndent) -> WrappingIndent {
        let accessibility_support = options.get(Opt::<AccessibilitySupport>::new(EditorOption::AccessibilitySupport));
        if *accessibility_support == AccessibilitySupport::Enabled {
            // Com leitor de tela confirmado, sem indentação, para os pontos de quebra do editor
            // coincidirem com os do textarea.
            return WrappingIndent::None;
        }
        *value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::editor_options::option_support::testing::{environment, font_info, options_with, value};

    fn text(s: &str) -> Dynamic {
        Dynamic::string(s)
    }

    #[test]
    fn numeric_values_follow_the_original_enum() {
        assert_eq!(
            [WrappingIndent::None as u8, WrappingIndent::Same as u8, WrappingIndent::Indent as u8, WrappingIndent::DeepIndent as u8],
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn validate_maps_the_four_strings_and_falls_back_to_same() {
        let option = WrappingIndentOption::new();
        assert_eq!(option.default_value(), WrappingIndent::Same);
        assert_eq!(option.validate(&text("none")), WrappingIndent::None);
        assert_eq!(option.validate(&text("same")), WrappingIndent::Same);
        assert_eq!(option.validate(&text("indent")), WrappingIndent::Indent);
        assert_eq!(option.validate(&text("deepIndent")), WrappingIndent::DeepIndent);
        assert_eq!(option.validate(&text("deepindent")), WrappingIndent::Same);
        assert_eq!(option.validate(&text("")), WrappingIndent::Same);
        assert_eq!(option.validate(&Dynamic::Number(2.0)), WrappingIndent::Same);
        assert_eq!(option.validate(&Dynamic::Undefined), WrappingIndent::Same);
        assert_eq!(option.validate(&Dynamic::Bool(true)), WrappingIndent::Same);
        assert_eq!(option.validate(&Dynamic::array([text("none")])), WrappingIndent::Same);
    }

    #[test]
    fn compute_drops_the_indent_only_with_a_screen_reader() {
        let option = WrappingIndentOption::new();
        let env = environment(font_info(true, 14.0, "normal"));
        for (support, expected) in [
            (AccessibilitySupport::Enabled, WrappingIndent::None),
            (AccessibilitySupport::Disabled, WrappingIndent::DeepIndent),
            (AccessibilitySupport::Unknown, WrappingIndent::DeepIndent),
        ] {
            let options = options_with(vec![(EditorOption::AccessibilitySupport, value(support))]);
            assert_eq!(option.compute(&env, &options, &WrappingIndent::DeepIndent), expected);
        }
    }

    #[test]
    fn schema_keeps_the_original_keys_and_order() {
        let option = WrappingIndentOption::new();
        let schema = option.schema().expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["editor.wrappingIndent"]);
        let property = schema.get_str("editor.wrappingIndent");
        assert_eq!(property.own_keys(), vec!["type", "enum", "enumDescriptions", "description", "default"]);
        assert_eq!(property.get_str("enum").get_str("length"), Dynamic::Number(4.0));
        assert_eq!(property.get_str("enumDescriptions").get_str("length"), Dynamic::Number(4.0));
        assert_eq!(property.get_str("default"), text("same"));
    }
}
