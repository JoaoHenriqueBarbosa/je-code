//! Peças comuns aos arquivos de opção deste diretório.
//!
//! - [`string_enum!`]: a união de strings do TypeScript (`'never' | 'always' | 'selection'`) como `enum`,
//!   com `as_str`, `from_text` e `validate` (`stringSet` seguido da conversão para a variante).
//! - `testing`: ambiente, `FontInfo` e opções calculadas para os testes das opções.

/// Declara um `enum` que representa uma união de strings do TypeScript.
///
/// `validate(input, default)` é `stringSet` com a lista de valores do `enum` e depois a conversão para a
/// variante: valor que não é string, ou que não está na lista, cai no padrão.
macro_rules! string_enum {
    ($(#[$meta:meta])* pub enum $name:ident { $($(#[$variant_meta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            $($(#[$variant_meta])* $variant),+
        }

        impl $name {
            /// As strings aceitas, na ordem do original.
            pub const ALLOWED: &'static [&'static str] = &[$($text),+];

            /// A string do original para esta variante.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            /// A variante da string, ou `None` se ela não é uma das aceitas.
            pub fn from_text(text: &str) -> Option<Self> {
                match text {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }

            /// `stringSet(input, default, ALLOWED)` seguido da conversão para a variante.
            pub fn validate(input: &je_base::dynamic::Dynamic, default_value: Self) -> Self {
                let text = $crate::config::editor_options::string_set(input, default_value.as_str(), Self::ALLOWED, None);
                Self::from_text(&text).unwrap_or(default_value)
            }
        }
    };
}


/// Montagem de ambiente e de opções calculadas para os testes das opções.
#[cfg(test)]
pub(crate) mod testing {
    use std::any::Any;
    use std::rc::Rc;

    use crate::config::editor_options::{
        AccessibilitySupport, ComputedEditorOptions, EditorOption, EnvironmentalOptions, InputMode,
    };
    use crate::config::font_info::{BareFontInfoOptions, FontInfo, FontInfoOptions};

    /// Um `FontInfo` de teste.
    pub fn font_info(is_monospace: bool, font_size: f64, font_variation_settings: &str) -> FontInfo {
        FontInfo::new(
            FontInfoOptions {
                bare: BareFontInfoOptions {
                    pixel_ratio: 1.0,
                    font_family: "Fira Code".into(),
                    font_weight: "normal".into(),
                    font_size,
                    font_feature_settings: "normal".into(),
                    font_variation_settings: font_variation_settings.into(),
                    line_height: 19.0,
                    letter_spacing: 0.0,
                },
                is_monospace,
                typical_halfwidth_character_width: 8.0,
                typical_fullwidth_character_width: 16.0,
                can_use_halfwidth_rightwards_arrow: true,
                space_width: 8.0,
                middot_width: 8.0,
                wsmiddot_width: 8.0,
                max_digit_width: 8.0,
            },
            true,
        )
    }

    /// Um ambiente de teste com a fonte dada (`pixel_ratio` 1, sem leitor de tela, inserção).
    pub fn environment(font_info: FontInfo) -> EnvironmentalOptions {
        EnvironmentalOptions {
            memory: None,
            outer_width: 0.0,
            outer_height: 0.0,
            font_info,
            extra_editor_class_name: String::new(),
            is_dominated_by_long_lines: false,
            view_line_count: 1.0,
            line_numbers_digit_count: 1.0,
            empty_selection_clipboard: true,
            pixel_ratio: 1.0,
            tab_focus_mode: false,
            input_mode: InputMode::Insert,
            accessibility_support: AccessibilitySupport::Unknown,
            glyph_margin_decoration_lane_count: 1.0,
            edit_context_supported: false,
        }
    }

    /// Guarda um valor como `Rc<dyn Any>`, o formato de `ComputedEditorOptions`.
    pub fn value<T: 'static>(value: T) -> Rc<dyn Any> {
        Rc::new(value)
    }

    /// Opções calculadas em que só os ids dados têm valor (os demais guardam `()`).
    pub fn options_with(overrides: Vec<(EditorOption, Rc<dyn Any>)>) -> ComputedEditorOptions {
        let mut values: Vec<Rc<dyn Any>> = EditorOption::ALL.iter().map(|_| value(())).collect();
        for (id, new_value) in overrides {
            values[id as usize] = new_value;
        }
        ComputedEditorOptions::new(values)
    }
}
