//! Porte de `EditorStickyScroll` (`editorOptions.ts`, linhas 3171 a 3245).

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{boolean, clamped_int, strings_array, EditorOption, EditorOptionImpl, Opt};

/// Chave tipada de `stickyScroll`.
pub const STICKY_SCROLL: Opt<EditorStickyScrollOptions> = Opt::new(EditorOption::StickyScroll);

string_enum! {
    /// `'outlineModel' | 'foldingProviderModel' | 'indentationModel'`.
    pub enum StickyScrollDefaultModel {
        OutlineModel => "outlineModel",
        FoldingProviderModel => "foldingProviderModel",
        IndentationModel => "indentationModel",
    }
}

/// `EditorStickyScrollOptions` (`Readonly<Required<IEditorStickyScrollOptions>>`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorStickyScrollOptions {
    pub enabled: bool,
    pub max_line_count: f64,
    pub default_model: StickyScrollDefaultModel,
    pub scroll_with_editor: bool,
}

/// `EditorStickyScroll`.
pub struct EditorStickyScroll {
    default_value: EditorStickyScrollOptions,
    schema: Dynamic,
}

impl EditorStickyScroll {
    pub fn new() -> Self {
        let defaults = EditorStickyScrollOptions {
            enabled: true,
            max_line_count: 5.0,
            default_model: StickyScrollDefaultModel::OutlineModel,
            scroll_with_editor: true,
        };
        let schema = Dynamic::object([
            (
                "editor.stickyScroll.enabled",
                Dynamic::object([
                    ("type", Dynamic::string("boolean")),
                    ("default", defaults.enabled.into()),
                    ("description", Dynamic::string("Shows the nested current scopes during the scroll at the top of the editor.")),
                ]),
            ),
            (
                "editor.stickyScroll.maxLineCount",
                Dynamic::object([
                    ("type", Dynamic::string("number")),
                    ("default", defaults.max_line_count.into()),
                    ("minimum", Dynamic::Number(1.0)),
                    ("maximum", Dynamic::Number(20.0)),
                    ("description", Dynamic::string("Defines the maximum number of sticky lines to show.")),
                ]),
            ),
            (
                "editor.stickyScroll.defaultModel",
                Dynamic::object([
                    ("type", Dynamic::string("string")),
                    ("enum", strings_array(StickyScrollDefaultModel::ALLOWED)),
                    ("default", Dynamic::string(defaults.default_model.as_str())),
                    (
                        "description",
                        Dynamic::string("Defines the model to use for determining which lines to stick. If the outline model does not exist, it will fall back on the folding provider model which falls back on the indentation model. This order is respected in all three cases."),
                    ),
                ]),
            ),
            (
                "editor.stickyScroll.scrollWithEditor",
                Dynamic::object([
                    ("type", Dynamic::string("boolean")),
                    ("default", defaults.scroll_with_editor.into()),
                    ("description", Dynamic::string("Enable scrolling of Sticky Scroll with the editor's horizontal scrollbar.")),
                ]),
            ),
        ]);
        Self { default_value: defaults, schema }
    }
}

impl Default for EditorStickyScroll {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorStickyScroll {
    type Value = EditorStickyScrollOptions;

    fn id(&self) -> EditorOption {
        EditorOption::StickyScroll
    }

    fn name(&self) -> &str {
        "stickyScroll"
    }

    fn default_value(&self) -> EditorStickyScrollOptions {
        self.default_value
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> EditorStickyScrollOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value;
        }
        let defaults = &self.default_value;
        EditorStickyScrollOptions {
            enabled: boolean(&input.get_str("enabled"), defaults.enabled),
            max_line_count: clamped_int(&input.get_str("maxLineCount"), defaults.max_line_count, 1.0, 20.0),
            default_model: StickyScrollDefaultModel::validate(&input.get_str("defaultModel"), defaults.default_model),
            scroll_with_editor: boolean(&input.get_str("scrollWithEditor"), defaults.scroll_with_editor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Dynamic {
        Dynamic::string(s)
    }

    fn num(n: f64) -> Dynamic {
        Dynamic::Number(n)
    }

    #[test]
    fn defaults() {
        let value = EditorStickyScroll::new().default_value();
        assert!(value.enabled && value.scroll_with_editor);
        assert_eq!(value.max_line_count, 5.0);
        assert_eq!(value.default_model, StickyScrollDefaultModel::OutlineModel);
    }

    #[test]
    fn non_objects_and_empty_objects_give_the_default() {
        let option = EditorStickyScroll::new();
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(false), num(3.0), text("x")] {
            assert_eq!(option.validate(&input), option.default_value());
        }
        assert_eq!(option.validate(&Dynamic::object::<&str>([])), option.default_value());
    }

    #[test]
    fn fields_are_validated_independently() {
        let option = EditorStickyScroll::new();
        let input = Dynamic::object([
            ("enabled", Dynamic::Bool(false)),
            ("maxLineCount", num(7.9)),
            ("defaultModel", text("indentationModel")),
            ("scrollWithEditor", text("false")),
        ]);
        let value = option.validate(&input);
        assert!(!value.enabled && !value.scroll_with_editor);
        assert_eq!(value.max_line_count, 7.0);
        assert_eq!(value.default_model, StickyScrollDefaultModel::IndentationModel);
    }

    #[test]
    fn max_line_count_is_clamped_between_1_and_20() {
        let option = EditorStickyScroll::new();
        let with = |n: Dynamic| option.validate(&Dynamic::object([("maxLineCount", n)])).max_line_count;
        assert_eq!(with(num(0.0)), 1.0);
        assert_eq!(with(num(-4.0)), 1.0);
        assert_eq!(with(num(21.0)), 20.0);
        assert_eq!(with(num(20.0)), 20.0);
        assert_eq!(with(num(f64::NAN)), 5.0);
        assert_eq!(with(text("12px")), 12.0);
        assert_eq!(with(text("abc")), 5.0);
        assert_eq!(with(Dynamic::Undefined), 5.0);
    }

    #[test]
    fn unknown_default_model_falls_back() {
        let option = EditorStickyScroll::new();
        let input = Dynamic::object([("defaultModel", text("other"))]);
        assert_eq!(option.validate(&input).default_model, StickyScrollDefaultModel::OutlineModel);
    }

    #[test]
    fn schema_keeps_the_original_keys_and_order() {
        let option = EditorStickyScroll::new();
        let schema = option.schema().expect("sem esquema");
        assert_eq!(
            schema.own_keys(),
            vec!["editor.stickyScroll.enabled", "editor.stickyScroll.maxLineCount", "editor.stickyScroll.defaultModel", "editor.stickyScroll.scrollWithEditor"]
        );
        let max = schema.get_str("editor.stickyScroll.maxLineCount");
        assert_eq!(max.own_keys(), vec!["type", "default", "minimum", "maximum", "description"]);
        assert_eq!((max.get_str("minimum"), max.get_str("maximum")), (num(1.0), num(20.0)));
        let model = schema.get_str("editor.stickyScroll.defaultModel");
        assert_eq!(model.own_keys(), vec!["type", "enum", "default", "description"]);
    }
}
