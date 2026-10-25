//! Porte de `src/vs/editor/test/browser/widget/multiDiffEditorWidget.test.ts` (suíte `MultiDiffEditorWidget`), caso a caso e
//! na mesma ordem. Só o primeiro caso está portado: os outros nove dependem do `MultiDiffEditorWidget`, do `DiffEditorWidget`
//! e do `MultiDiffEditorViewModel`, que ainda não existem em `je-editor` (ver o relatório do lote).
//!
//! Notas de tradução:
//! - O `deepStrictEqual` sobre objetos vira a comparação de tuplas de campos.

use je_editor::multi_diff_editor_options::{
    DiffEditorOptionsOverride, DiffEditorVariant, HorizontalInsets, IMultiDiffEditorVariantConfiguration, MULTI_DIFF_EDITOR_VARIANTS, MultiDiffEditorVariant,
    get_multi_diff_editor_variant_configuration,
};

#[test]
fn uses_closed_variant_configurations() {
    let variants: Vec<&str> = MULTI_DIFF_EDITOR_VARIANTS.iter().map(|variant| variant.as_str()).collect();
    let hide_original = Some(DiffEditorOptionsOverride { hide_original_line_numbers: Some(true) });
    assert_eq!(
        (
            variants,
            get_multi_diff_editor_variant_configuration(MultiDiffEditorVariant::Cards),
            get_multi_diff_editor_variant_configuration(MultiDiffEditorVariant::NoCardsNonCompact),
            get_multi_diff_editor_variant_configuration(MultiDiffEditorVariant::NoCards),
        ),
        (
            vec!["cards", "noCards", "noCardsNonCompact"],
            IMultiDiffEditorVariantConfiguration {
                diff_editor_variant: Some(DiffEditorVariant::Compact),
                class_names: &["multiDiffEditor-compact", "multiDiffEditor-card"],
                horizontal_insets: HorizontalInsets { left: 9, right: 9 },
                header_height: 40,
                content_bottom_padding: 0,
                header_click_to_collapse: true,
                diff_editor_options: hide_original,
            },
            IMultiDiffEditorVariantConfiguration {
                diff_editor_variant: None,
                class_names: &["multiDiffEditor-standard"],
                horizontal_insets: HorizontalInsets { left: 9, right: 9 },
                header_height: 40,
                content_bottom_padding: 0,
                header_click_to_collapse: false,
                diff_editor_options: None,
            },
            IMultiDiffEditorVariantConfiguration {
                diff_editor_variant: None,
                class_names: &["multiDiffEditor-compact"],
                horizontal_insets: HorizontalInsets { left: 0, right: 0 },
                header_height: 32,
                content_bottom_padding: 8,
                header_click_to_collapse: true,
                diff_editor_options: hide_original,
            },
        )
    );
}
