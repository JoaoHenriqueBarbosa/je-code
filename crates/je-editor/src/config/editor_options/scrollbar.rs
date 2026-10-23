//! Porte de `EditorScrollbar`, `IEditorScrollbarOptions` e `InternalEditorScrollbarOptions`
//! (`editorOptions.ts`, linhas 4097 a 4303).

use je_base::dynamic::{Dynamic, Truthy};
use je_base::scrollable::ScrollbarVisibility;

use crate::config::editor_options::{boolean, clamped_int, strings_array, EditorOption, EditorOptionImpl, Opt};

/// `InternalEditorScrollbarOptions`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InternalEditorScrollbarOptions {
    pub arrow_size: f64,
    pub vertical: ScrollbarVisibility,
    pub horizontal: ScrollbarVisibility,
    pub use_shadows: bool,
    pub vertical_has_arrows: bool,
    pub horizontal_has_arrows: bool,
    pub handle_mouse_wheel: bool,
    pub always_consume_mouse_wheel: bool,
    pub horizontal_scrollbar_size: f64,
    pub horizontal_slider_size: f64,
    pub vertical_scrollbar_size: f64,
    pub vertical_slider_size: f64,
    pub scroll_by_page: bool,
    pub ignore_horizontal_scrollbar_in_content_height: bool,
}

/// Chave tipada da opção `scrollbar`.
pub const SCROLLBAR: Opt<InternalEditorScrollbarOptions> = Opt::new(EditorOption::Scrollbar);

/// `_scrollbarVisibilityFromString`: o que não é string dá o padrão; `hidden` e `visible` têm o seu
/// valor e qualquer outra string vira `Auto`.
fn scrollbar_visibility_from_string(visibility: &Dynamic, default_value: ScrollbarVisibility) -> ScrollbarVisibility {
    let Dynamic::String(visibility) = visibility else {
        return default_value;
    };
    if *visibility == "hidden" {
        ScrollbarVisibility::Hidden
    } else if *visibility == "visible" {
        ScrollbarVisibility::Visible
    } else {
        ScrollbarVisibility::Auto
    }
}

/// `{ type, default, description }`.
fn entry(type_name: &str, default: Dynamic, description: &str) -> Dynamic {
    Dynamic::object([
        ("type", Dynamic::string(type_name)),
        ("default", default),
        ("description", Dynamic::string(description)),
    ])
}

/// `{ type: 'string', enum: ['auto', 'visible', 'hidden'], enumDescriptions, default: 'auto', description }`.
fn visibility_entry(enum_descriptions: &[&str], description: &str) -> Dynamic {
    Dynamic::object([
        ("type", Dynamic::string("string")),
        ("enum", strings_array(&["auto", "visible", "hidden"])),
        ("enumDescriptions", strings_array(enum_descriptions)),
        ("default", Dynamic::string("auto")),
        ("description", Dynamic::string(description)),
    ])
}

/// `EditorScrollbar`.
pub struct EditorScrollbar {
    default_value: InternalEditorScrollbarOptions,
    schema: Dynamic,
}

impl EditorScrollbar {
    pub fn new() -> Self {
        let defaults = InternalEditorScrollbarOptions {
            vertical: ScrollbarVisibility::Auto,
            horizontal: ScrollbarVisibility::Auto,
            arrow_size: 11.0,
            use_shadows: true,
            vertical_has_arrows: false,
            horizontal_has_arrows: false,
            horizontal_scrollbar_size: 12.0,
            horizontal_slider_size: 12.0,
            vertical_scrollbar_size: 14.0,
            vertical_slider_size: 14.0,
            handle_mouse_wheel: true,
            always_consume_mouse_wheel: true,
            scroll_by_page: false,
            ignore_horizontal_scrollbar_in_content_height: false,
        };
        let schema = Dynamic::object([
            (
                "editor.scrollbar.vertical",
                visibility_entry(
                    &[
                        "The vertical scrollbar will be visible only when necessary.",
                        "The vertical scrollbar will always be visible.",
                        "The vertical scrollbar will always be hidden.",
                    ],
                    "Controls the visibility of the vertical scrollbar.",
                ),
            ),
            (
                "editor.scrollbar.horizontal",
                visibility_entry(
                    &[
                        "The horizontal scrollbar will be visible only when necessary.",
                        "The horizontal scrollbar will always be visible.",
                        "The horizontal scrollbar will always be hidden.",
                    ],
                    "Controls the visibility of the horizontal scrollbar.",
                ),
            ),
            (
                "editor.scrollbar.verticalScrollbarSize",
                entry("number", Dynamic::Number(defaults.vertical_scrollbar_size), "The width of the vertical scrollbar."),
            ),
            (
                "editor.scrollbar.horizontalScrollbarSize",
                entry("number", Dynamic::Number(defaults.horizontal_scrollbar_size), "The height of the horizontal scrollbar."),
            ),
            (
                "editor.scrollbar.scrollByPage",
                entry(
                    "boolean",
                    Dynamic::Bool(defaults.scroll_by_page),
                    "Controls whether clicks scroll by page or jump to click position.",
                ),
            ),
            (
                "editor.scrollbar.ignoreHorizontalScrollbarInContentHeight",
                entry(
                    "boolean",
                    Dynamic::Bool(defaults.ignore_horizontal_scrollbar_in_content_height),
                    "When set, the horizontal scrollbar will not increase the size of the editor's content.",
                ),
            ),
        ]);
        Self { default_value: defaults, schema }
    }
}

impl Default for EditorScrollbar {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorScrollbar {
    type Value = InternalEditorScrollbarOptions;

    fn id(&self) -> EditorOption {
        EditorOption::Scrollbar
    }

    fn name(&self) -> &str {
        "scrollbar"
    }

    fn default_value(&self) -> InternalEditorScrollbarOptions {
        self.default_value
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> InternalEditorScrollbarOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value;
        }
        let defaults = &self.default_value;
        let horizontal_scrollbar_size =
            clamped_int(&input.get_str("horizontalScrollbarSize"), defaults.horizontal_scrollbar_size, 0.0, 1000.0);
        let vertical_scrollbar_size = clamped_int(&input.get_str("verticalScrollbarSize"), defaults.vertical_scrollbar_size, 0.0, 1000.0);
        InternalEditorScrollbarOptions {
            arrow_size: clamped_int(&input.get_str("arrowSize"), defaults.arrow_size, 0.0, 1000.0),
            vertical: scrollbar_visibility_from_string(&input.get_str("vertical"), defaults.vertical),
            horizontal: scrollbar_visibility_from_string(&input.get_str("horizontal"), defaults.horizontal),
            use_shadows: boolean(&input.get_str("useShadows"), defaults.use_shadows),
            vertical_has_arrows: boolean(&input.get_str("verticalHasArrows"), defaults.vertical_has_arrows),
            horizontal_has_arrows: boolean(&input.get_str("horizontalHasArrows"), defaults.horizontal_has_arrows),
            handle_mouse_wheel: boolean(&input.get_str("handleMouseWheel"), defaults.handle_mouse_wheel),
            always_consume_mouse_wheel: boolean(&input.get_str("alwaysConsumeMouseWheel"), defaults.always_consume_mouse_wheel),
            horizontal_scrollbar_size,
            // O padrão do slider é o tamanho já validado da barra.
            horizontal_slider_size: clamped_int(&input.get_str("horizontalSliderSize"), horizontal_scrollbar_size, 0.0, 1000.0),
            vertical_scrollbar_size,
            vertical_slider_size: clamped_int(&input.get_str("verticalSliderSize"), vertical_scrollbar_size, 0.0, 1000.0),
            scroll_by_page: boolean(&input.get_str("scrollByPage"), defaults.scroll_by_page),
            ignore_horizontal_scrollbar_in_content_height: boolean(
                &input.get_str("ignoreHorizontalScrollbarInContentHeight"),
                defaults.ignore_horizontal_scrollbar_in_content_height,
            ),
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

    fn validate(entries: Vec<(&str, Dynamic)>) -> InternalEditorScrollbarOptions {
        EditorOptionImpl::validate(&EditorScrollbar::new(), &Dynamic::object(entries))
    }

    fn defaults() -> InternalEditorScrollbarOptions {
        EditorOptionImpl::default_value(&EditorScrollbar::new())
    }

    #[test]
    fn defaults_match_the_original() {
        let defaults = defaults();
        assert_eq!((defaults.vertical, defaults.horizontal), (ScrollbarVisibility::Auto, ScrollbarVisibility::Auto));
        assert_eq!(defaults.arrow_size, 11.0);
        assert_eq!((defaults.horizontal_scrollbar_size, defaults.horizontal_slider_size), (12.0, 12.0));
        assert_eq!((defaults.vertical_scrollbar_size, defaults.vertical_slider_size), (14.0, 14.0));
        assert!(defaults.use_shadows && defaults.handle_mouse_wheel && defaults.always_consume_mouse_wheel);
        assert!(!defaults.vertical_has_arrows && !defaults.horizontal_has_arrows);
        assert!(!defaults.scroll_by_page && !defaults.ignore_horizontal_scrollbar_in_content_height);
        assert_eq!(validate(vec![]), defaults);
        let option = EditorScrollbar::new();
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), num(1.0), text("scrollbar")] {
            assert_eq!(EditorOptionImpl::validate(&option, &input), defaults);
        }
    }

    #[test]
    fn visibility_strings() {
        let value = validate(vec![("vertical", text("hidden")), ("horizontal", text("visible"))]);
        assert_eq!((value.vertical, value.horizontal), (ScrollbarVisibility::Hidden, ScrollbarVisibility::Visible));
        // Qualquer outra string vira `Auto`, e o que não é string fica com o padrão.
        let value = validate(vec![("vertical", text("bogus")), ("horizontal", text("auto"))]);
        assert_eq!((value.vertical, value.horizontal), (ScrollbarVisibility::Auto, ScrollbarVisibility::Auto));
        let value = validate(vec![("vertical", num(2.0)), ("horizontal", Dynamic::Bool(true))]);
        assert_eq!((value.vertical, value.horizontal), (ScrollbarVisibility::Auto, ScrollbarVisibility::Auto));
        assert_eq!(scrollbar_visibility_from_string(&num(2.0), ScrollbarVisibility::Hidden), ScrollbarVisibility::Hidden);
        assert_eq!(scrollbar_visibility_from_string(&Dynamic::Undefined, ScrollbarVisibility::Visible), ScrollbarVisibility::Visible);
        assert_eq!(scrollbar_visibility_from_string(&text(""), ScrollbarVisibility::Visible), ScrollbarVisibility::Auto);
    }

    #[test]
    fn sizes_are_clamped_integers() {
        let value = validate(vec![("arrowSize", num(20.9)), ("verticalScrollbarSize", num(5000.0)), ("horizontalScrollbarSize", num(-3.0))]);
        assert_eq!(value.arrow_size, 20.0);
        assert_eq!(value.vertical_scrollbar_size, 1000.0);
        assert_eq!(value.horizontal_scrollbar_size, 0.0);
        assert_eq!(validate(vec![("arrowSize", text("abc"))]).arrow_size, 11.0);
        assert_eq!(validate(vec![("arrowSize", text("7px"))]).arrow_size, 7.0);
    }

    #[test]
    fn slider_sizes_default_to_the_validated_scrollbar_sizes() {
        let value = validate(vec![("verticalScrollbarSize", num(20.0)), ("horizontalScrollbarSize", num(9.0))]);
        assert_eq!((value.vertical_slider_size, value.horizontal_slider_size), (20.0, 9.0));
        let value = validate(vec![("verticalScrollbarSize", num(5000.0))]);
        assert_eq!(value.vertical_slider_size, 1000.0);
        let value = validate(vec![("verticalSliderSize", num(3.0)), ("horizontalSliderSize", num(4.0))]);
        assert_eq!((value.vertical_slider_size, value.horizontal_slider_size), (3.0, 4.0));
        assert_eq!((value.vertical_scrollbar_size, value.horizontal_scrollbar_size), (14.0, 12.0));
        let value = validate(vec![("verticalSliderSize", num(2000.0))]);
        assert_eq!(value.vertical_slider_size, 1000.0);
    }

    #[test]
    fn booleans_follow_the_boolean_helper() {
        let value = validate(vec![
            ("useShadows", text("false")),
            ("verticalHasArrows", Dynamic::Bool(true)),
            ("horizontalHasArrows", num(1.0)),
            ("handleMouseWheel", num(0.0)),
            ("alwaysConsumeMouseWheel", Dynamic::Null),
            ("scrollByPage", Dynamic::Bool(true)),
            ("ignoreHorizontalScrollbarInContentHeight", Dynamic::Bool(true)),
        ]);
        assert!(!value.use_shadows && value.vertical_has_arrows && value.horizontal_has_arrows);
        assert!(!value.handle_mouse_wheel && !value.always_consume_mouse_wheel);
        assert!(value.scroll_by_page && value.ignore_horizontal_scrollbar_in_content_height);
    }

    #[test]
    fn schema_keeps_the_original_shape_and_order() {
        let option = EditorScrollbar::new();
        let schema = EditorOptionImpl::schema(&option).expect("sem esquema");
        assert_eq!(
            schema.own_keys(),
            vec![
                "editor.scrollbar.vertical",
                "editor.scrollbar.horizontal",
                "editor.scrollbar.verticalScrollbarSize",
                "editor.scrollbar.horizontalScrollbarSize",
                "editor.scrollbar.scrollByPage",
                "editor.scrollbar.ignoreHorizontalScrollbarInContentHeight",
            ]
        );
        let vertical = schema.get_str("editor.scrollbar.vertical");
        assert_eq!(vertical.own_keys(), vec!["type", "enum", "enumDescriptions", "default", "description"]);
        assert_eq!(vertical.get_str("enum").get_str("length"), num(3.0));
        assert_eq!(vertical.get_str("enum").get_str("2"), text("hidden"));
        assert_eq!(vertical.get_str("default"), text("auto"));
        assert_eq!(vertical.get_str("enumDescriptions").get_str("2"), text("The vertical scrollbar will always be hidden."));
        let size = schema.get_str("editor.scrollbar.verticalScrollbarSize");
        assert_eq!(size.own_keys(), vec!["type", "default", "description"]);
        assert_eq!(size.get_str("default"), num(14.0));
        assert_eq!(schema.get_str("editor.scrollbar.horizontalScrollbarSize").get_str("default"), num(12.0));
        assert_eq!(schema.get_str("editor.scrollbar.scrollByPage").get_str("type"), text("boolean"));
    }
}
