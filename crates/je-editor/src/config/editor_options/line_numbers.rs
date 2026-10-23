//! Porte de `EditorRenderLineNumbersOption`, `RenderLineNumbersType` e `LineNumbersType`
//! (`editorOptions.ts`, linhas 3916 a 3979).
//!
//! A função de `lineNumbers` do original é [`RenderFn`], que guarda a função do [`Dynamic`] e compara por
//! identidade (como o `===` do JavaScript).

use std::fmt;

use je_base::dynamic::{Dynamic, FunctionRef};

use crate::config::editor_options::{strings_array, EditorOption, EditorOptionImpl, Opt};

/// Chave tipada de `lineNumbers`.
pub const LINE_NUMBERS: Opt<InternalEditorRenderLineNumbersOptions> = Opt::new(EditorOption::LineNumbers);

/// `(lineNumber: number) => string`.
#[derive(Clone)]
pub struct RenderFn(FunctionRef);

impl RenderFn {
    pub fn new(function: FunctionRef) -> Self {
        RenderFn(function)
    }

    pub fn function(&self) -> &FunctionRef {
        &self.0
    }

    /// `renderFn(lineNumber)`.
    pub fn call(&self, line_number: f64) -> Dynamic {
        self.0.call(&[Dynamic::Number(line_number)])
    }
}

impl PartialEq for RenderFn {
    fn eq(&self, other: &Self) -> bool {
        self.0.same(&other.0)
    }
}

impl fmt::Debug for RenderFn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RenderFn({})", self.0.name())
    }
}

/// `RenderLineNumbersType` (valores numéricos do original).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderLineNumbersType {
    Off = 0,
    On = 1,
    Relative = 2,
    Interval = 3,
    Custom = 4,
}

/// `InternalEditorRenderLineNumbersOptions`.
#[derive(Clone, Debug, PartialEq)]
pub struct InternalEditorRenderLineNumbersOptions {
    pub render_type: RenderLineNumbersType,
    pub render_fn: Option<RenderFn>,
}

/// `EditorRenderLineNumbersOption`.
pub struct EditorRenderLineNumbersOption {
    schema: Dynamic,
}

impl EditorRenderLineNumbersOption {
    pub fn new() -> Self {
        let schema = Dynamic::object([
            ("type", Dynamic::string("string")),
            ("enum", strings_array(&["off", "on", "relative", "interval"])),
            (
                "enumDescriptions",
                Dynamic::array([
                    Dynamic::string("Line numbers are not rendered."),
                    Dynamic::string("Line numbers are rendered as absolute number."),
                    Dynamic::string("Line numbers are rendered as distance in lines to cursor position."),
                    Dynamic::string("Line numbers are rendered every 10 lines."),
                ]),
            ),
            ("default", Dynamic::string("on")),
            ("description", Dynamic::string("Controls the display of line numbers.")),
        ]);
        Self { schema }
    }
}

impl Default for EditorRenderLineNumbersOption {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorRenderLineNumbersOption {
    type Value = InternalEditorRenderLineNumbersOptions;

    fn id(&self) -> EditorOption {
        EditorOption::LineNumbers
    }

    fn name(&self) -> &str {
        "lineNumbers"
    }

    fn default_value(&self) -> InternalEditorRenderLineNumbersOptions {
        InternalEditorRenderLineNumbersOptions { render_type: RenderLineNumbersType::On, render_fn: None }
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, line_numbers: &Dynamic) -> InternalEditorRenderLineNumbersOptions {
        let default = self.default_value();
        let mut render_type = default.render_type;
        let mut render_fn = default.render_fn;

        if !line_numbers.is_undefined() {
            match line_numbers {
                Dynamic::Function(function) => {
                    render_type = RenderLineNumbersType::Custom;
                    render_fn = Some(RenderFn::new(function.clone()));
                }
                Dynamic::String(text) if *text == "interval" => render_type = RenderLineNumbersType::Interval,
                Dynamic::String(text) if *text == "relative" => render_type = RenderLineNumbersType::Relative,
                Dynamic::String(text) if *text == "on" => render_type = RenderLineNumbersType::On,
                _ => render_type = RenderLineNumbersType::Off,
            }
        }

        InternalEditorRenderLineNumbersOptions { render_type, render_fn }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_type(input: Dynamic) -> RenderLineNumbersType {
        EditorRenderLineNumbersOption::new().validate(&input).render_type
    }

    #[test]
    fn default_is_on_without_function() {
        let value = EditorRenderLineNumbersOption::new().default_value();
        assert_eq!(value.render_type, RenderLineNumbersType::On);
        assert!(value.render_fn.is_none());
    }

    #[test]
    fn numeric_values_of_the_render_type_match_the_original() {
        assert_eq!(RenderLineNumbersType::Off as u8, 0);
        assert_eq!(RenderLineNumbersType::On as u8, 1);
        assert_eq!(RenderLineNumbersType::Relative as u8, 2);
        assert_eq!(RenderLineNumbersType::Interval as u8, 3);
        assert_eq!(RenderLineNumbersType::Custom as u8, 4);
    }

    #[test]
    fn undefined_keeps_the_default() {
        assert_eq!(render_type(Dynamic::Undefined), RenderLineNumbersType::On);
    }

    #[test]
    fn known_strings_map_to_their_type() {
        assert_eq!(render_type(Dynamic::string("on")), RenderLineNumbersType::On);
        assert_eq!(render_type(Dynamic::string("off")), RenderLineNumbersType::Off);
        assert_eq!(render_type(Dynamic::string("relative")), RenderLineNumbersType::Relative);
        assert_eq!(render_type(Dynamic::string("interval")), RenderLineNumbersType::Interval);
    }

    #[test]
    fn anything_else_is_off() {
        assert_eq!(render_type(Dynamic::string("bogus")), RenderLineNumbersType::Off);
        assert_eq!(render_type(Dynamic::string("ON")), RenderLineNumbersType::Off);
        assert_eq!(render_type(Dynamic::Null), RenderLineNumbersType::Off);
        assert_eq!(render_type(Dynamic::Bool(true)), RenderLineNumbersType::Off);
        assert_eq!(render_type(Dynamic::Number(1.0)), RenderLineNumbersType::Off);
        assert_eq!(render_type(Dynamic::array([])), RenderLineNumbersType::Off);
        assert_eq!(render_type(Dynamic::object::<&str>([])), RenderLineNumbersType::Off);
    }

    #[test]
    fn string_values_never_set_a_render_fn() {
        let value = EditorRenderLineNumbersOption::new().validate(&Dynamic::string("relative"));
        assert!(value.render_fn.is_none());
    }

    #[test]
    fn functions_become_custom_and_keep_identity() {
        let option = EditorRenderLineNumbersOption::new();
        let function = FunctionRef::native("renderLine", 1, |args| match args.first() {
            Some(Dynamic::Number(n)) => Dynamic::string(&format!("L{n}")),
            _ => Dynamic::Undefined,
        });
        let value = option.validate(&Dynamic::Function(function.clone()));
        assert_eq!(value.render_type, RenderLineNumbersType::Custom);
        let stored = value.render_fn.expect("função guardada");
        assert_eq!(stored.call(7.0), Dynamic::string("L7"));
        assert_eq!(stored, RenderFn::new(function));
        let other = FunctionRef::native("renderLine", 1, |_| Dynamic::Undefined);
        assert_ne!(stored, RenderFn::new(other));
    }

    #[test]
    fn schema_keeps_the_original_keys_and_order() {
        let option = EditorRenderLineNumbersOption::new();
        let schema = option.schema().expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["type", "enum", "enumDescriptions", "default", "description"]);
        assert_eq!(schema.get_str("default"), Dynamic::string("on"));
        assert_eq!(schema.get_str("enum").get_str("length"), Dynamic::Number(4.0));
        assert_eq!(schema.get_str("enumDescriptions").get_str("length"), Dynamic::Number(4.0));
    }
}
