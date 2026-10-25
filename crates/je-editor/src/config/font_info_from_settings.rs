//! Porte de `src/vs/editor/common/config/fontInfoFromSettings.ts`: a `BareFontInfo` montada a partir das opções
//! validadas do editor ([`create_bare_font_info_from_validated_settings`]) ou a partir das configurações cruas
//! ([`create_bare_font_info_from_raw_settings`]).
//!
//! Divergências:
//! - O objeto `opts` de `createBareFontInfoFromRawSettings` (`{ fontFamily?, fontWeight?, ... }`, com campos
//!   `unknown`) é o [`Dynamic`] da configuração `editor`, como o serviço de configuração o entrega; cada campo ausente
//!   é `undefined` e cai no padrão da opção, como no original.
//! - `EditorOptions.<opção>.validate(...)` é a validação da opção correspondente do registro
//!   ([`editor_options_registry`]). `EditorOptions.fontLigatures2`, a segunda instância de `EditorFontLigatures` que o
//!   original registra à parte (sem entrada no `EditorOption`), é uma instância nova de [`EditorFontLigatures`], que não
//!   guarda estado.
//! - O valor padrão de `ignoreEditorZoom` (`false`) é argumento explícito.

use je_base::dynamic::Dynamic;

use crate::config::editor_configuration_impl::ValidatedEditorOptions;
use crate::config::editor_options::font_ligatures::{EditorFontLigatures, FONT_LIGATURES};
use crate::config::editor_options::font_size::FONT_SIZE;
use crate::config::editor_options::font_variations::FONT_VARIATIONS;
use crate::config::editor_options::font_weight::FONT_WEIGHT;
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::config::editor_options::registry::{editor_options_registry, FONT_FAMILY, LETTER_SPACING};
use crate::config::editor_options::{EditorOption, EditorOptionImpl};
use crate::config::font_info::BareFontInfo;

/// `createBareFontInfoFromValidatedSettings(options, pixelRatio, ignoreEditorZoom)`.
pub fn create_bare_font_info_from_validated_settings(
    options: &ValidatedEditorOptions,
    pixel_ratio: f64,
    ignore_editor_zoom: bool,
) -> BareFontInfo {
    BareFontInfo::create(
        options.get(FONT_FAMILY),
        options.get(FONT_WEIGHT),
        *options.get(FONT_SIZE),
        options.get(FONT_LIGATURES),
        options.get(FONT_VARIATIONS),
        *options.get(LINE_HEIGHT),
        *options.get(LETTER_SPACING),
        pixel_ratio,
        ignore_editor_zoom,
    )
}

/// `EditorOptions.<opção>.validate(input)` de uma opção do registro, no tipo `T` do valor dela.
fn validate_registered_option<T: Clone + 'static>(option: EditorOption, input: &Dynamic) -> T {
    editor_options_registry()[option as usize]
        .validate(input)
        .downcast_ref::<T>()
        .cloned()
        .expect("o valor validado da opção tem o tipo do registro")
}

/// `createBareFontInfoFromRawSettings(opts, pixelRatio, ignoreEditorZoom)`: `opts` é o objeto com as sete opções de fonte
/// (a configuração `editor`).
pub fn create_bare_font_info_from_raw_settings(opts: &Dynamic, pixel_ratio: f64, ignore_editor_zoom: bool) -> BareFontInfo {
    let font_family: String = validate_registered_option(EditorOption::FontFamily, &opts.get_str("fontFamily"));
    let font_weight: String = validate_registered_option(EditorOption::FontWeight, &opts.get_str("fontWeight"));
    let font_size: f64 = validate_registered_option(EditorOption::FontSize, &opts.get_str("fontSize"));
    let font_feature_settings = EditorOptionImpl::validate(&EditorFontLigatures::new(), &opts.get_str("fontLigatures"));
    let font_variation_settings: String = validate_registered_option(EditorOption::FontVariations, &opts.get_str("fontVariations"));
    let line_height: f64 = validate_registered_option(EditorOption::LineHeight, &opts.get_str("lineHeight"));
    let letter_spacing: f64 = validate_registered_option(EditorOption::LetterSpacing, &opts.get_str("letterSpacing"));
    BareFontInfo::create(
        &font_family,
        &font_weight,
        font_size,
        &font_feature_settings,
        &font_variation_settings,
        line_height,
        letter_spacing,
        pixel_ratio,
        ignore_editor_zoom,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_settings_use_the_option_validation() {
        let opts = Dynamic::object([
            ("fontFamily", Dynamic::string("Fira Code")),
            ("fontWeight", Dynamic::string("bold")),
            ("fontSize", Dynamic::Number(20.0)),
            ("fontLigatures", Dynamic::Bool(true)),
            ("lineHeight", Dynamic::Number(30.0)),
            ("letterSpacing", Dynamic::Number(1.0)),
        ]);
        let info = create_bare_font_info_from_raw_settings(&opts, 1.0, true);
        assert_eq!(info.line_height, 30.0);
        assert_eq!(info.font_size, 20.0);
        assert_eq!(info.font_weight, "bold");
        assert_eq!(info.font_feature_settings, EditorFontLigatures::ON);
        assert_eq!(info.letter_spacing, 1.0);
    }

    #[test]
    fn missing_settings_fall_back_to_the_defaults() {
        let info = create_bare_font_info_from_raw_settings(&Dynamic::object::<&str>([]), 1.0, true);
        assert!(info.line_height > 0.0);
        assert_eq!(info.font_weight, "normal");
        assert_eq!(info.font_feature_settings, EditorFontLigatures::OFF);
    }
}
