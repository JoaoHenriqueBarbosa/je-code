//! Auxiliares compartilhados pelas opções de `editorOptions.ts` portadas neste diretório.
//!
//! O `validate` do original recebe `unknown` e lê campos de objetos (`input.enabled`). [`OptionInput`]
//! só enxerga escalares e marca objeto e array como `Other`, então cada opção com struct de entrada
//! tem um método tipado `validate_options` (ou `validate_input`) que recebe a interface `I...Options`
//! já montada. O `validate` do trait traduz o que o [`OptionInput`] consegue dizer: um `Other` vale como
//! objeto sem nenhum campo preenchido, o que dá o mesmo resultado do original para `{}`.
//! `// TODO(config)`: quando `OptionInput` virar `Dynamic`, o `validate` do trait lê os campos direto.

use crate::config::editor_options::OptionInput;

/// `Other` (objeto ou array) vira a entrada vazia `I::default()`; qualquer outro valor vira `None`,
/// que é o caso `!_input || typeof _input !== 'object'` do original.
pub(crate) fn object_input<I: Default>(input: &OptionInput) -> Option<I> {
    matches!(input, OptionInput::Other).then(I::default)
}

/// Campo numérico opcional de uma interface como `unknown` (`undefined` quando ausente).
pub(crate) fn number_input(value: Option<f64>) -> OptionInput {
    value.map_or(OptionInput::Undefined, OptionInput::Number)
}

/// Campo booleano opcional de uma interface como `unknown`.
pub(crate) fn bool_input(value: Option<bool>) -> OptionInput {
    value.map_or(OptionInput::Undefined, OptionInput::Bool)
}

/// Campo de texto opcional de uma interface como `unknown`.
pub(crate) fn string_input(value: Option<&str>) -> OptionInput {
    value.map_or(OptionInput::Undefined, OptionInput::string)
}

/// Campo `unknown` opcional: ausente é `undefined`.
pub(crate) fn input_or_undefined(value: Option<&OptionInput>) -> OptionInput {
    value.cloned().unwrap_or(OptionInput::Undefined)
}

#[cfg(test)]
pub(crate) use test_support::*;

#[cfg(test)]
mod test_support {
    use std::any::Any;
    use std::rc::Rc;

    use crate::config::editor_options::{AccessibilitySupport, ComputedEditorOptions, EditorOption, EnvironmentalOptions, InputMode};
    use crate::config::font_info::{BareFontInfoOptions, FontInfo, FontInfoOptions};

    pub fn test_env(pixel_ratio: f64) -> EnvironmentalOptions {
        EnvironmentalOptions {
            memory: None,
            outer_width: 0.0,
            outer_height: 0.0,
            extra_editor_class_name: String::new(),
            is_dominated_by_long_lines: false,
            view_line_count: 1.0,
            line_numbers_digit_count: 1.0,
            empty_selection_clipboard: true,
            pixel_ratio,
            tab_focus_mode: false,
            input_mode: InputMode::Insert,
            accessibility_support: AccessibilitySupport::Unknown,
            glyph_margin_decoration_lane_count: 1.0,
            edit_context_supported: false,
        }
    }

    /// Opções calculadas em que só `accessibilitySupport` tem valor útil.
    pub fn test_options(accessibility: AccessibilitySupport) -> ComputedEditorOptions {
        ComputedEditorOptions::new(
            EditorOption::ALL
                .iter()
                .map(|id| {
                    if *id == EditorOption::AccessibilitySupport {
                        Rc::new(accessibility) as Rc<dyn Any>
                    } else {
                        Rc::new(()) as Rc<dyn Any>
                    }
                })
                .collect(),
        )
    }

    pub fn test_font_info(line_height: f64, typical_halfwidth_character_width: f64) -> FontInfo {
        FontInfo::new(
            FontInfoOptions {
                bare: BareFontInfoOptions {
                    pixel_ratio: 1.0,
                    font_family: "Fira Code".into(),
                    font_weight: "normal".into(),
                    font_size: 14.0,
                    font_feature_settings: "normal".into(),
                    font_variation_settings: "normal".into(),
                    line_height,
                    letter_spacing: 0.0,
                },
                is_monospace: true,
                typical_halfwidth_character_width,
                typical_fullwidth_character_width: typical_halfwidth_character_width * 2.0,
                can_use_halfwidth_rightwards_arrow: true,
                space_width: typical_halfwidth_character_width,
                middot_width: typical_halfwidth_character_width,
                wsmiddot_width: typical_halfwidth_character_width,
                max_digit_width: typical_halfwidth_character_width,
            },
            true,
        )
    }
}
