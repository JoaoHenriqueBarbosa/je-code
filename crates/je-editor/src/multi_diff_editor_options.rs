//! Porte de `src/vs/editor/browser/widget/multiDiffEditor/multiDiffEditorOptions.ts` e do tipo `MultiDiffEditorVariant` de
//! `editor/common/multiDiffEditor.ts`: a configuração fechada de cada variante do editor de diff de vários arquivos.
//!
//! Divergências:
//! - Os literais de texto do TypeScript (`'cards'`, `'compact'`) são enums fechados; [`MultiDiffEditorVariant::as_str`] devolve o
//!   texto original.
//! - `IDiffEditorOptions` (`editor/common/config/editorOptions.ts`) ainda não foi portado por inteiro: o recorte
//!   [`DiffEditorOptionsOverride`] leva só o campo que as variantes definem (`hideOriginalLineNumbers`).

/// `MultiDiffEditorVariant`: `'cards' | 'noCards' | 'noCardsNonCompact'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultiDiffEditorVariant {
    Cards,
    NoCards,
    NoCardsNonCompact,
}

impl MultiDiffEditorVariant {
    /// O texto da variante no original.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cards => "cards",
            Self::NoCards => "noCards",
            Self::NoCardsNonCompact => "noCardsNonCompact",
        }
    }
}

/// `multiDiffEditorVariants`.
pub const MULTI_DIFF_EDITOR_VARIANTS: [MultiDiffEditorVariant; 3] =
    [MultiDiffEditorVariant::Cards, MultiDiffEditorVariant::NoCards, MultiDiffEditorVariant::NoCardsNonCompact];

/// `DiffEditorVariant` (`diffEditorOptions.ts`): `'default' | 'compact'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffEditorVariant {
    Default,
    Compact,
}

/// Recorte de `IDiffEditorOptions` com os campos que as variantes definem.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiffEditorOptionsOverride {
    pub hide_original_line_numbers: Option<bool>,
}

/// `Readonly<{ left: number; right: number }>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HorizontalInsets {
    pub left: i32,
    pub right: i32,
}

/// `IMultiDiffEditorVariantConfiguration`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IMultiDiffEditorVariantConfiguration {
    pub diff_editor_variant: Option<DiffEditorVariant>,
    pub class_names: &'static [&'static str],
    pub horizontal_insets: HorizontalInsets,
    pub header_height: i32,
    pub content_bottom_padding: i32,
    pub header_click_to_collapse: bool,
    pub diff_editor_options: Option<DiffEditorOptionsOverride>,
}

/// `getMultiDiffEditorVariantConfiguration(variant)`.
pub fn get_multi_diff_editor_variant_configuration(variant: MultiDiffEditorVariant) -> IMultiDiffEditorVariantConfiguration {
    match variant {
        MultiDiffEditorVariant::NoCardsNonCompact => IMultiDiffEditorVariantConfiguration {
            diff_editor_variant: None,
            class_names: &["multiDiffEditor-standard"],
            horizontal_insets: HorizontalInsets { left: 9, right: 9 },
            header_height: 40,
            content_bottom_padding: 0,
            header_click_to_collapse: false,
            diff_editor_options: None,
        },
        MultiDiffEditorVariant::NoCards => IMultiDiffEditorVariantConfiguration {
            diff_editor_variant: None,
            class_names: &["multiDiffEditor-compact"],
            horizontal_insets: HorizontalInsets { left: 0, right: 0 },
            header_height: 32,
            content_bottom_padding: 8,
            header_click_to_collapse: true,
            diff_editor_options: Some(DiffEditorOptionsOverride { hide_original_line_numbers: Some(true) }),
        },
        MultiDiffEditorVariant::Cards => IMultiDiffEditorVariantConfiguration {
            diff_editor_variant: Some(DiffEditorVariant::Compact),
            class_names: &["multiDiffEditor-compact", "multiDiffEditor-card"],
            horizontal_insets: HorizontalInsets { left: 9, right: 9 },
            header_height: 40,
            content_bottom_padding: 0,
            header_click_to_collapse: true,
            diff_editor_options: Some(DiffEditorOptionsOverride { hide_original_line_numbers: Some(true) }),
        },
    }
}
