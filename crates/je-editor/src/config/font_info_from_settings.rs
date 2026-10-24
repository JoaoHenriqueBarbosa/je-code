//! Porte de `src/vs/editor/common/config/fontInfoFromSettings.ts`: a `BareFontInfo` montada a partir das opções
//! validadas do editor.
//!
//! Fora do porte: `createBareFontInfoFromRawSettings`, que valida as sete opções soltas com
//! `EditorOptions.fontFamily.validate(...)` e companhia, inclusive `EditorOptions.fontLigatures2` (a opção
//! interna de `fontLigatures` que só existe para essa função, sem `EditorOption` no registro do porte). Quem
//! chama é a medição de fonte do DOM, que no je-code é a [`EditorEnvironment`](super::editor_configuration_impl::EditorEnvironment).

use crate::config::editor_configuration_impl::ValidatedEditorOptions;
use crate::config::editor_options::font_ligatures::FONT_LIGATURES;
use crate::config::editor_options::font_size::FONT_SIZE;
use crate::config::editor_options::font_variations::FONT_VARIATIONS;
use crate::config::editor_options::font_weight::FONT_WEIGHT;
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::config::editor_options::registry::{FONT_FAMILY, LETTER_SPACING};
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
