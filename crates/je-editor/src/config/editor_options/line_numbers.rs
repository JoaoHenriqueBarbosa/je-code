//! Porte de `EditorRenderLineNumbersOption`, `RenderLineNumbersType` e `LineNumbersType`
//! (`editorOptions.ts`, linhas 3916 a 3979).
//!
//! A função de `lineNumbers` do original vira [`RenderFn`], um `Rc<dyn Fn>` que compara por identidade
//! (como o `===` do JavaScript).

use std::fmt;
use std::rc::Rc;

use crate::config::editor_options::{ConfigurationPropertySchema, EditorOption, EditorOptionImpl, Opt, OptionInput, OptionSchema, SchemaValue};

/// Chave tipada de `lineNumbers`.
pub const LINE_NUMBERS: Opt<InternalEditorRenderLineNumbersOptions> = Opt::new(EditorOption::LineNumbers);

/// `(lineNumber: number) => string`.
#[derive(Clone)]
pub struct RenderFn(Rc<dyn Fn(f64) -> String>);

impl RenderFn {
    pub fn new(render: impl Fn(f64) -> String + 'static) -> Self {
        RenderFn(Rc::new(render))
    }

    pub fn call(&self, line_number: f64) -> String {
        (self.0)(line_number)
    }
}

impl PartialEq for RenderFn {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for RenderFn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RenderFn(..)")
    }
}

/// `LineNumbersType`: `'on' | 'off' | 'relative' | 'interval' | ((lineNumber: number) => string)`.
/// Qualquer texto fora dos três nomes (e qualquer valor que não seja função nem `undefined`) vale `Off`.
#[derive(Clone, Debug, PartialEq)]
pub enum LineNumbersType {
    On,
    Off,
    Relative,
    Interval,
    Function(RenderFn),
}

impl LineNumbersType {
    /// Lê o valor dinâmico: `None` é `undefined` (vale o padrão); o resto segue a cadeia de `if` do
    /// original (`'interval'`, `'relative'`, `'on'`, senão `Off`).
    pub fn from_input(input: &OptionInput) -> Option<Self> {
        match input {
            OptionInput::Undefined => None,
            OptionInput::String(text) => Some(match text.as_str() {
                "interval" => LineNumbersType::Interval,
                "relative" => LineNumbersType::Relative,
                "on" => LineNumbersType::On,
                _ => LineNumbersType::Off,
            }),
            _ => Some(LineNumbersType::Off),
        }
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
    schema: OptionSchema,
}

impl EditorRenderLineNumbersOption {
    pub fn new() -> Self {
        let schema = OptionSchema::Single(ConfigurationPropertySchema {
            type_: Some("string".to_string()),
            enum_values: Some(["off", "on", "relative", "interval"].iter().map(|s| (*s).to_string()).collect()),
            default: Some(SchemaValue::String("on".to_string())),
            description: Some("Controls the display of line numbers.".to_string()),
            ..Default::default()
        });
        Self { schema }
    }

    /// `validate` com a entrada tipada; `None` é `undefined`.
    pub fn validate_line_numbers(&self, line_numbers: Option<&LineNumbersType>) -> InternalEditorRenderLineNumbersOptions {
        let default = self.default_value();
        let mut render_type = default.render_type;
        let mut render_fn = default.render_fn;

        if let Some(line_numbers) = line_numbers {
            match line_numbers {
                LineNumbersType::Function(function) => {
                    render_type = RenderLineNumbersType::Custom;
                    render_fn = Some(function.clone());
                }
                LineNumbersType::Interval => render_type = RenderLineNumbersType::Interval,
                LineNumbersType::Relative => render_type = RenderLineNumbersType::Relative,
                LineNumbersType::On => render_type = RenderLineNumbersType::On,
                LineNumbersType::Off => render_type = RenderLineNumbersType::Off,
            }
        }

        InternalEditorRenderLineNumbersOptions { render_type, render_fn }
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

    fn schema(&self) -> Option<&OptionSchema> {
        Some(&self.schema)
    }

    fn validate(&self, input: &OptionInput) -> InternalEditorRenderLineNumbersOptions {
        self.validate_line_numbers(LineNumbersType::from_input(input).as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_type(input: OptionInput) -> RenderLineNumbersType {
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
        assert_eq!(render_type(OptionInput::Undefined), RenderLineNumbersType::On);
    }

    #[test]
    fn known_strings_map_to_their_type() {
        assert_eq!(render_type(OptionInput::string("on")), RenderLineNumbersType::On);
        assert_eq!(render_type(OptionInput::string("off")), RenderLineNumbersType::Off);
        assert_eq!(render_type(OptionInput::string("relative")), RenderLineNumbersType::Relative);
        assert_eq!(render_type(OptionInput::string("interval")), RenderLineNumbersType::Interval);
    }

    #[test]
    fn anything_else_is_off() {
        assert_eq!(render_type(OptionInput::string("bogus")), RenderLineNumbersType::Off);
        assert_eq!(render_type(OptionInput::string("ON")), RenderLineNumbersType::Off);
        assert_eq!(render_type(OptionInput::Null), RenderLineNumbersType::Off);
        assert_eq!(render_type(OptionInput::Bool(true)), RenderLineNumbersType::Off);
        assert_eq!(render_type(OptionInput::Number(1.0)), RenderLineNumbersType::Off);
        assert_eq!(render_type(OptionInput::Other), RenderLineNumbersType::Off);
    }

    #[test]
    fn string_values_never_set_a_render_fn() {
        let value = EditorRenderLineNumbersOption::new().validate(&OptionInput::string("relative"));
        assert!(value.render_fn.is_none());
    }

    #[test]
    fn functions_become_custom_and_keep_identity() {
        let option = EditorRenderLineNumbersOption::new();
        let render = RenderFn::new(|n| format!("L{n}"));
        let value = option.validate_line_numbers(Some(&LineNumbersType::Function(render.clone())));
        assert_eq!(value.render_type, RenderLineNumbersType::Custom);
        let stored = value.render_fn.expect("função guardada");
        assert_eq!(stored.call(7.0), "L7");
        assert_eq!(stored, render);
        assert_ne!(stored, RenderFn::new(|n| format!("L{n}")));
    }

    #[test]
    fn schema_is_a_single_enum() {
        let option = EditorRenderLineNumbersOption::new();
        let Some(OptionSchema::Single(schema)) = option.schema() else { panic!("sem esquema") };
        assert_eq!(schema.type_.as_deref(), Some("string"));
        assert_eq!(schema.default, Some(SchemaValue::String("on".to_string())));
        assert_eq!(schema.enum_values.as_ref().map(Vec::len), Some(4));
    }
}
