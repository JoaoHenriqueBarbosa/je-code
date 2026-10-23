//! Porte de `EditorHover` e `IEditorHoverOptions` (`editorOptions.ts`, linhas 2352 a 2467).

use je_base::dynamic::{Dynamic, Truthy};
use je_base::platform::Platform;

use crate::config::editor_options::{boolean, clamped_int, strings_array, EditorOption, EditorOptionImpl, Opt};

string_enum! {
    /// `'on' | 'off' | 'onKeyboardModifier'`.
    pub enum HoverEnabled {
        On => "on",
        Off => "off",
        OnKeyboardModifier => "onKeyboardModifier",
    }
}

/// `EditorHoverOptions` (`Readonly<Required<IEditorHoverOptions>>`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorHoverOptions {
    /// Liga o hover.
    pub enabled: HoverEnabled,
    /// Atraso em milissegundos para mostrar o hover.
    pub delay: f64,
    /// O hover é fixo: dá para clicar nele e selecionar o conteúdo.
    pub sticky: bool,
    /// Quanto tempo o hover continua visível depois de o mouse sair dele (exige `sticky`).
    pub hiding_delay: f64,
    /// O hover aparece acima da linha, se couber.
    pub above: bool,
    /// Mostra os hovers de aviso de linha longa (tokenização pulada, renderização pausada).
    pub show_long_line_warning: bool,
}

/// Chave tipada da opção `hover`.
pub const HOVER: Opt<EditorHoverOptions> = Opt::new(EditorOption::Hover);

/// `EditorHover`.
pub struct EditorHover {
    default_value: EditorHoverOptions,
    schema: Dynamic,
}

impl EditorHover {
    pub fn new() -> Self {
        let defaults = EditorHoverOptions {
            enabled: HoverEnabled::On,
            delay: 300.0,
            sticky: true,
            hiding_delay: 300.0,
            above: true,
            show_long_line_warning: true,
        };
        let modifier = if Platform::detect().is_macintosh { "Command" } else { "Control" };
        let on_keyboard_modifier = format!(
            "Hover is shown when holding `{modifier}` or `Alt` (the opposite modifier of `#editor.multiCursorModifier#`)"
        );
        let schema = Dynamic::object([
            (
                "editor.hover.enabled",
                Dynamic::object([
                    ("type", Dynamic::string("string")),
                    ("enum", strings_array(HoverEnabled::ALLOWED)),
                    ("default", Dynamic::string(defaults.enabled.as_str())),
                    (
                        "markdownEnumDescriptions",
                        strings_array(&["Hover is enabled.", "Hover is disabled.", on_keyboard_modifier.as_str()]),
                    ),
                    ("description", Dynamic::string("Controls whether the hover is shown.")),
                    ("keywords", strings_array(&["hint", "info", "tooltip"])),
                ]),
            ),
            (
                "editor.hover.delay",
                Dynamic::object([
                    ("type", Dynamic::string("number")),
                    ("default", Dynamic::Number(defaults.delay)),
                    ("minimum", Dynamic::Number(0.0)),
                    ("maximum", Dynamic::Number(10000.0)),
                    ("description", Dynamic::string("Controls the delay in milliseconds after which the hover is shown.")),
                ]),
            ),
            (
                "editor.hover.sticky",
                Dynamic::object([
                    ("type", Dynamic::string("boolean")),
                    ("default", Dynamic::Bool(defaults.sticky)),
                    ("description", Dynamic::string("Controls whether the hover should remain visible when mouse is moved over it.")),
                ]),
            ),
            (
                "editor.hover.hidingDelay",
                Dynamic::object([
                    ("type", Dynamic::string("integer")),
                    ("minimum", Dynamic::Number(0.0)),
                    ("default", Dynamic::Number(defaults.hiding_delay)),
                    (
                        "markdownDescription",
                        Dynamic::string(
                            "Controls the delay in milliseconds after which the hover is hidden. Requires `#editor.hover.sticky#` to be enabled.",
                        ),
                    ),
                ]),
            ),
            (
                "editor.hover.above",
                Dynamic::object([
                    ("type", Dynamic::string("boolean")),
                    ("default", Dynamic::Bool(defaults.above)),
                    ("description", Dynamic::string("Prefer showing hovers above the line, if there's space.")),
                ]),
            ),
            (
                "editor.hover.showLongLineWarning",
                Dynamic::object([
                    ("type", Dynamic::string("boolean")),
                    ("default", Dynamic::Bool(defaults.show_long_line_warning)),
                    (
                        "description",
                        Dynamic::string(
                            "Controls whether long line warning hovers are shown, such as when tokenization is skipped or rendering is paused.",
                        ),
                    ),
                ]),
            ),
        ]);
        Self { default_value: defaults, schema }
    }
}

impl Default for EditorHover {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorHover {
    type Value = EditorHoverOptions;

    fn id(&self) -> EditorOption {
        EditorOption::Hover
    }

    fn name(&self) -> &str {
        "hover"
    }

    fn default_value(&self) -> EditorHoverOptions {
        self.default_value
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> EditorHoverOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value;
        }
        let defaults = &self.default_value;
        EditorHoverOptions {
            enabled: HoverEnabled::validate(&input.get_str("enabled"), defaults.enabled),
            delay: clamped_int(&input.get_str("delay"), defaults.delay, 0.0, 10000.0),
            sticky: boolean(&input.get_str("sticky"), defaults.sticky),
            hiding_delay: clamped_int(&input.get_str("hidingDelay"), defaults.hiding_delay, 0.0, 600000.0),
            above: boolean(&input.get_str("above"), defaults.above),
            show_long_line_warning: boolean(&input.get_str("showLongLineWarning"), defaults.show_long_line_warning),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Dynamic {
        Dynamic::string(value)
    }

    fn num(value: f64) -> Dynamic {
        Dynamic::Number(value)
    }

    fn validate(entries: Vec<(&str, Dynamic)>) -> EditorHoverOptions {
        EditorOptionImpl::validate(&EditorHover::new(), &Dynamic::object(entries))
    }

    fn defaults() -> EditorHoverOptions {
        EditorOptionImpl::default_value(&EditorHover::new())
    }

    #[test]
    fn defaults_match_the_original() {
        let defaults = defaults();
        assert_eq!(defaults.enabled, HoverEnabled::On);
        assert_eq!((defaults.delay, defaults.hiding_delay), (300.0, 300.0));
        assert!(defaults.sticky && defaults.above && defaults.show_long_line_warning);
        assert_eq!(validate(vec![]), defaults);
        let option = EditorHover::new();
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), num(1.0), text("hover"), num(0.0)] {
            assert_eq!(EditorOptionImpl::validate(&option, &input), defaults);
        }
        // Um array é `typeof 'object'` e sem campos dá os padrões.
        assert_eq!(EditorOptionImpl::validate(&option, &Dynamic::array([])), defaults);
    }

    #[test]
    fn enabled_accepts_only_the_three_strings() {
        assert_eq!(validate(vec![("enabled", text("off"))]).enabled, HoverEnabled::Off);
        assert_eq!(validate(vec![("enabled", text("onKeyboardModifier"))]).enabled, HoverEnabled::OnKeyboardModifier);
        assert_eq!(validate(vec![("enabled", text("sometimes"))]).enabled, HoverEnabled::On);
        assert_eq!(validate(vec![("enabled", Dynamic::Bool(false))]).enabled, HoverEnabled::On);
        assert_eq!(validate(vec![("enabled", Dynamic::Null)]).enabled, HoverEnabled::On);
    }

    #[test]
    fn delays_are_clamped_integers() {
        assert_eq!(validate(vec![("delay", num(500.7))]).delay, 500.0);
        assert_eq!(validate(vec![("delay", text("250px"))]).delay, 250.0);
        assert_eq!(validate(vec![("delay", num(20000.0))]).delay, 10000.0);
        assert_eq!(validate(vec![("delay", num(-5.0))]).delay, 0.0);
        assert_eq!(validate(vec![("delay", text("abc"))]).delay, 300.0);
        // O limite do `hidingDelay` na validação (600000) é maior que o do `delay`.
        assert_eq!(validate(vec![("hidingDelay", num(20000.0))]).hiding_delay, 20000.0);
        assert_eq!(validate(vec![("hidingDelay", num(700000.0))]).hiding_delay, 600000.0);
        assert_eq!(validate(vec![("hidingDelay", num(-1.0))]).hiding_delay, 0.0);
        assert_eq!(validate(vec![("hidingDelay", Dynamic::Bool(true))]).hiding_delay, 300.0);
    }

    #[test]
    fn booleans_follow_the_boolean_helper() {
        let value = validate(vec![("sticky", text("false")), ("above", num(0.0)), ("showLongLineWarning", Dynamic::Bool(false))]);
        assert!(!value.sticky && !value.above && !value.show_long_line_warning);
        let value = validate(vec![("sticky", Dynamic::Null), ("above", text("x"))]);
        assert!(!value.sticky && value.above);
    }

    #[test]
    fn schema_keeps_the_original_shape_and_order() {
        let option = EditorHover::new();
        let schema = EditorOptionImpl::schema(&option).expect("sem esquema");
        assert_eq!(
            schema.own_keys(),
            vec![
                "editor.hover.enabled",
                "editor.hover.delay",
                "editor.hover.sticky",
                "editor.hover.hidingDelay",
                "editor.hover.above",
                "editor.hover.showLongLineWarning",
            ]
        );
        let enabled = schema.get_str("editor.hover.enabled");
        assert_eq!(enabled.own_keys(), vec!["type", "enum", "default", "markdownEnumDescriptions", "description", "keywords"]);
        assert_eq!(enabled.get_str("enum").get_str("length"), num(3.0));
        assert_eq!(enabled.get_str("default"), text("on"));
        let modifier = if Platform::detect().is_macintosh { "Command" } else { "Control" };
        assert_eq!(
            enabled.get_str("markdownEnumDescriptions").get_str("2"),
            text(&format!(
                "Hover is shown when holding `{modifier}` or `Alt` (the opposite modifier of `#editor.multiCursorModifier#`)"
            ))
        );
        let delay = schema.get_str("editor.hover.delay");
        assert_eq!(delay.own_keys(), vec!["type", "default", "minimum", "maximum", "description"]);
        assert_eq!((delay.get_str("minimum"), delay.get_str("maximum")), (num(0.0), num(10000.0)));
        let hiding = schema.get_str("editor.hover.hidingDelay");
        assert_eq!(hiding.own_keys(), vec!["type", "minimum", "default", "markdownDescription"]);
        assert_eq!(hiding.get_str("type"), text("integer"));
        assert_eq!(schema.get_str("editor.hover.sticky").own_keys(), vec!["type", "default", "description"]);
    }
}
