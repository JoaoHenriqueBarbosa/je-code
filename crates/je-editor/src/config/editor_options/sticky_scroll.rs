//! Porte de `EditorStickyScroll` (`editorOptions.ts`, linhas 3171 a 3245).

use crate::config::editor_options::option_support::{bool_input, number_input, object_input, string_input};
use crate::config::editor_options::{
    boolean, clamped_int, string_set, ConfigurationPropertySchema, EditorOption, EditorOptionImpl, Opt, OptionInput, OptionSchema, SchemaValue,
};

/// Chave tipada de `stickyScroll`.
pub const STICKY_SCROLL: Opt<EditorStickyScrollOptions> = Opt::new(EditorOption::StickyScroll);

/// `'outlineModel' | 'foldingProviderModel' | 'indentationModel'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StickyScrollDefaultModel {
    OutlineModel,
    FoldingProviderModel,
    IndentationModel,
}

impl StickyScrollDefaultModel {
    pub const fn as_str(self) -> &'static str {
        match self {
            StickyScrollDefaultModel::OutlineModel => "outlineModel",
            StickyScrollDefaultModel::FoldingProviderModel => "foldingProviderModel",
            StickyScrollDefaultModel::IndentationModel => "indentationModel",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "outlineModel" => Some(StickyScrollDefaultModel::OutlineModel),
            "foldingProviderModel" => Some(StickyScrollDefaultModel::FoldingProviderModel),
            "indentationModel" => Some(StickyScrollDefaultModel::IndentationModel),
            _ => None,
        }
    }
}

const ALLOWED_MODELS: [&str; 3] = ["outlineModel", "foldingProviderModel", "indentationModel"];

/// `IEditorStickyScrollOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IEditorStickyScrollOptions {
    pub enabled: Option<bool>,
    pub max_line_count: Option<f64>,
    pub default_model: Option<String>,
    pub scroll_with_editor: Option<bool>,
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
    schema: OptionSchema,
}

impl EditorStickyScroll {
    pub fn new() -> Self {
        let defaults = EditorStickyScrollOptions {
            enabled: true,
            max_line_count: 5.0,
            default_model: StickyScrollDefaultModel::OutlineModel,
            scroll_with_editor: true,
        };
        let schema = OptionSchema::Map(vec![
            (
                "editor.stickyScroll.enabled".to_string(),
                ConfigurationPropertySchema {
                    type_: Some("boolean".to_string()),
                    default: Some(SchemaValue::Bool(defaults.enabled)),
                    description: Some("Shows the nested current scopes during the scroll at the top of the editor.".to_string()),
                    ..Default::default()
                },
            ),
            (
                "editor.stickyScroll.maxLineCount".to_string(),
                ConfigurationPropertySchema {
                    type_: Some("number".to_string()),
                    default: Some(SchemaValue::Number(defaults.max_line_count)),
                    minimum: Some(1.0),
                    maximum: Some(20.0),
                    description: Some("Defines the maximum number of sticky lines to show.".to_string()),
                    ..Default::default()
                },
            ),
            (
                "editor.stickyScroll.defaultModel".to_string(),
                ConfigurationPropertySchema {
                    type_: Some("string".to_string()),
                    enum_values: Some(ALLOWED_MODELS.iter().map(|s| (*s).to_string()).collect()),
                    default: Some(SchemaValue::String(defaults.default_model.as_str().to_string())),
                    description: Some(
                        "Defines the model to use for determining which lines to stick. If the outline model does not exist, it will fall back on the folding provider model which falls back on the indentation model. This order is respected in all three cases."
                            .to_string(),
                    ),
                    ..Default::default()
                },
            ),
            (
                "editor.stickyScroll.scrollWithEditor".to_string(),
                ConfigurationPropertySchema {
                    type_: Some("boolean".to_string()),
                    default: Some(SchemaValue::Bool(defaults.scroll_with_editor)),
                    description: Some("Enable scrolling of Sticky Scroll with the editor's horizontal scrollbar.".to_string()),
                    ..Default::default()
                },
            ),
        ]);
        Self { default_value: defaults, schema }
    }

    /// `validate` com a interface já montada; `None` é o caso `!_input || typeof _input !== 'object'`.
    pub fn validate_options(&self, input: Option<&IEditorStickyScrollOptions>) -> EditorStickyScrollOptions {
        let Some(input) = input else {
            return self.default_value;
        };
        let defaults = &self.default_value;
        let default_model = string_set(&string_input(input.default_model.as_deref()), defaults.default_model.as_str(), &ALLOWED_MODELS, None);
        EditorStickyScrollOptions {
            enabled: boolean(&bool_input(input.enabled), defaults.enabled),
            max_line_count: clamped_int(&number_input(input.max_line_count), defaults.max_line_count, 1.0, 20.0),
            default_model: StickyScrollDefaultModel::from_str(&default_model).unwrap_or(defaults.default_model),
            scroll_with_editor: boolean(&bool_input(input.scroll_with_editor), defaults.scroll_with_editor),
        }
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

    fn schema(&self) -> Option<&OptionSchema> {
        Some(&self.schema)
    }

    fn validate(&self, input: &OptionInput) -> EditorStickyScrollOptions {
        self.validate_options(object_input::<IEditorStickyScrollOptions>(input).as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let option = EditorStickyScroll::new();
        let value = option.default_value();
        assert!(value.enabled && value.scroll_with_editor);
        assert_eq!(value.max_line_count, 5.0);
        assert_eq!(value.default_model, StickyScrollDefaultModel::OutlineModel);
    }

    #[test]
    fn non_objects_and_empty_objects_give_the_default() {
        let option = EditorStickyScroll::new();
        assert_eq!(option.validate_options(None), option.default_value());
        for input in [OptionInput::Undefined, OptionInput::Null, OptionInput::Bool(false), OptionInput::Number(3.0), OptionInput::string("x")] {
            assert_eq!(option.validate(&input), option.default_value());
        }
        assert_eq!(option.validate(&OptionInput::Other), option.default_value());
    }

    #[test]
    fn fields_are_validated_independently() {
        let option = EditorStickyScroll::new();
        let input = IEditorStickyScrollOptions {
            enabled: Some(false),
            max_line_count: Some(7.9),
            default_model: Some("indentationModel".to_string()),
            scroll_with_editor: Some(false),
        };
        let value = option.validate_options(Some(&input));
        assert!(!value.enabled && !value.scroll_with_editor);
        assert_eq!(value.max_line_count, 7.0);
        assert_eq!(value.default_model, StickyScrollDefaultModel::IndentationModel);
    }

    #[test]
    fn max_line_count_is_clamped_between_1_and_20() {
        let option = EditorStickyScroll::new();
        let with = |n: f64| option.validate_options(Some(&IEditorStickyScrollOptions { max_line_count: Some(n), ..Default::default() })).max_line_count;
        assert_eq!(with(0.0), 1.0);
        assert_eq!(with(-4.0), 1.0);
        assert_eq!(with(21.0), 20.0);
        assert_eq!(with(20.0), 20.0);
        assert_eq!(with(f64::NAN), 5.0);
    }

    #[test]
    fn unknown_default_model_falls_back() {
        let option = EditorStickyScroll::new();
        let input = IEditorStickyScrollOptions { default_model: Some("other".to_string()), ..Default::default() };
        assert_eq!(option.validate_options(Some(&input)).default_model, StickyScrollDefaultModel::OutlineModel);
    }

    #[test]
    fn schema_has_four_keys() {
        let option = EditorStickyScroll::new();
        let Some(OptionSchema::Map(entries)) = option.schema() else { panic!("sem esquema") };
        let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            ["editor.stickyScroll.enabled", "editor.stickyScroll.maxLineCount", "editor.stickyScroll.defaultModel", "editor.stickyScroll.scrollWithEditor"]
        );
        assert_eq!((entries[1].1.minimum, entries[1].1.maximum), (Some(1.0), Some(20.0)));
    }
}
