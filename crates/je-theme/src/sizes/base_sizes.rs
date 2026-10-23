//! Porte de `src/vs/platform/theme/common/sizes/baseSizes.ts`: os tamanhos base do tema (fontes,
//! pesos, raios de canto, espessura de traço e a escala de espaçamento).
//!
//! No original cada `export const x = registerSize(...)` registra no singleton ao carregar o módulo e
//! guarda o id devolvido. Aqui o id é uma constante (`BODY_FONT_SIZE`...) e o registro é a tabela
//! [`BASE_SIZES`], aplicada por [`register_base_sizes`] quando o singleton nasce
//! ([`crate::size_utils::SizeRegistry::with_base_sizes`]). A chave de `nls.localize` da mensagem de
//! depreciação é sempre a da descrição mais `.deprecated`.

use je_base::nls;

use crate::size_utils::{SizeRegistry, SizeUnit, size_for_all_themes};

// ------ Font Sizes

/// Obsoleto: use [`FONT_SIZE_BODY1`].
pub const BODY_FONT_SIZE: &str = "bodyFontSize";
/// Obsoleto: use [`FONT_SIZE_LABEL1`].
pub const BODY_FONT_SIZE_SMALL: &str = "bodyFontSize.small";
/// Obsoleto: use [`FONT_SIZE_BODY2`].
pub const BODY_FONT_SIZE_X_SMALL: &str = "bodyFontSize.xSmall";

// ------ Font ramp
//
// A generic font-size ramp for headings, body text and labels. "Strong" variants
// are NOT separate size tokens: reuse the matching size token paired with
// `fontWeight.semiBold` (600). Regular text pairs with `fontWeight.regular` (400).

pub const FONT_SIZE_HEADING1: &str = "fontSize.heading1";
pub const FONT_SIZE_HEADING2: &str = "fontSize.heading2";
pub const FONT_SIZE_HEADING3: &str = "fontSize.heading3";
pub const FONT_SIZE_BODY1: &str = "fontSize.body1";
pub const FONT_SIZE_BODY2: &str = "fontSize.body2";
pub const FONT_SIZE_LABEL1: &str = "fontSize.label1";
pub const FONT_SIZE_LABEL2: &str = "fontSize.label2";
pub const FONT_SIZE_LABEL3: &str = "fontSize.label3";

// ------ Font weights
//
// A two-weight ramp (regular/semiBold). "Strong" emphasis reuses the matching
// font-size token paired with `fontWeight.semiBold`.

pub const FONT_WEIGHT_REGULAR: &str = "fontWeight.regular";
pub const FONT_WEIGHT_SEMI_BOLD: &str = "fontWeight.semiBold";

pub const CODICON_FONT_SIZE: &str = "codiconFontSize";
pub const CODICON_FONT_SIZE_COMPACT: &str = "codiconFontSize.compact";

// ------ Corner Radii

pub const CORNER_RADIUS_MEDIUM: &str = "cornerRadius.medium";
pub const CORNER_RADIUS_X_SMALL: &str = "cornerRadius.xSmall";
pub const CORNER_RADIUS_SMALL: &str = "cornerRadius.small";
pub const CORNER_RADIUS_LARGE: &str = "cornerRadius.large";
pub const CORNER_RADIUS_X_LARGE: &str = "cornerRadius.xLarge";
pub const CORNER_RADIUS_CIRCLE: &str = "cornerRadius.circle";

// ------ Stroke Thickness

pub const STROKE_THICKNESS: &str = "strokeThickness";

// ------ Spacing ramp
//
// A fixed ramp of spacing tokens used for padding, margins and gaps. Numeric tokens
// encode the value in tenths of a pixel (e.g. `size200` is 20px). `sizeNone`
// represents 0px, matching the design system's spacing ramp.

pub const SPACING_NONE: &str = "spacing.sizeNone";
pub const SPACING_SIZE20: &str = "spacing.size20";
pub const SPACING_SIZE40: &str = "spacing.size40";
pub const SPACING_SIZE60: &str = "spacing.size60";
pub const SPACING_SIZE80: &str = "spacing.size80";
pub const SPACING_SIZE100: &str = "spacing.size100";
pub const SPACING_SIZE120: &str = "spacing.size120";
pub const SPACING_SIZE160: &str = "spacing.size160";
pub const SPACING_SIZE200: &str = "spacing.size200";
pub const SPACING_SIZE240: &str = "spacing.size240";
pub const SPACING_SIZE280: &str = "spacing.size280";
pub const SPACING_SIZE320: &str = "spacing.size320";
pub const SPACING_SIZE360: &str = "spacing.size360";
pub const SPACING_SIZE400: &str = "spacing.size400";

/// Uma chamada de `registerSize(id, sizeForAllThemes(value, unit), localize(key, description), ...)`.
struct BaseSize {
    id: &'static str,
    value: f64,
    unit: SizeUnit,
    /// A chave de `nls.localize` da descrição.
    key: &'static str,
    description: &'static str,
    /// A mensagem de depreciação, localizada com a chave `{key}.deprecated`.
    deprecation_message: Option<&'static str>,
}

const fn px(
    id: &'static str,
    value: f64,
    key: &'static str,
    description: &'static str,
) -> BaseSize {
    BaseSize {
        id,
        value,
        unit: SizeUnit::Px,
        key,
        description,
        deprecation_message: None,
    }
}

const fn deprecated(base: BaseSize, message: &'static str) -> BaseSize {
    BaseSize {
        deprecation_message: Some(message),
        ..base
    }
}

/// Os tamanhos base, na ordem de registro do `baseSizes.ts`.
const BASE_SIZES: &[BaseSize] = &[
    // ------ Font Sizes
    deprecated(
        px(
            BODY_FONT_SIZE,
            13.0,
            "bodyFontSize",
            "Base font size. This size is used if not overridden by a component.",
        ),
        "Deprecated: use `fontSize.body1` instead.",
    ),
    deprecated(
        px(
            BODY_FONT_SIZE_SMALL,
            12.0,
            "bodyFontSizeSmall",
            "Small font size for secondary content.",
        ),
        "Deprecated: use `fontSize.label1` instead.",
    ),
    deprecated(
        px(
            BODY_FONT_SIZE_X_SMALL,
            11.0,
            "bodyFontSizeXSmall",
            "Extra small font size for less prominent content.",
        ),
        "Deprecated: use `fontSize.body2` instead.",
    ),
    // ------ Font ramp
    px(
        FONT_SIZE_HEADING1,
        26.0,
        "fontSizeHeading1",
        "Heading 1 font size (largest heading).",
    ),
    px(
        FONT_SIZE_HEADING2,
        18.0,
        "fontSizeHeading2",
        "Heading 2 font size (title).",
    ),
    px(
        FONT_SIZE_HEADING3,
        13.0,
        "fontSizeHeading3",
        "Heading 3 font size (subtitle).",
    ),
    px(
        FONT_SIZE_BODY1,
        13.0,
        "fontSizeBody1",
        "Primary body font size.",
    ),
    px(
        FONT_SIZE_BODY2,
        11.0,
        "fontSizeBody2",
        "Secondary body font size.",
    ),
    px(
        FONT_SIZE_LABEL1,
        12.0,
        "fontSizeLabel1",
        "Label 1 font size (section title, tabs).",
    ),
    px(
        FONT_SIZE_LABEL2,
        11.0,
        "fontSizeLabel2",
        "Label 2 font size (metadata).",
    ),
    px(
        FONT_SIZE_LABEL3,
        10.0,
        "fontSizeLabel3",
        "Label 3 font size (badge).",
    ),
    // ------ Font weights
    BaseSize {
        unit: SizeUnit::Unitless,
        ..px(
            FONT_WEIGHT_REGULAR,
            400.0,
            "fontWeightRegular",
            "Regular font weight (400) for body, labels and metadata.",
        )
    },
    BaseSize {
        unit: SizeUnit::Unitless,
        ..px(
            FONT_WEIGHT_SEMI_BOLD,
            600.0,
            "fontWeightSemiBold",
            "SemiBold font weight (600) for headings and strong emphasis.",
        )
    },
    px(
        CODICON_FONT_SIZE,
        16.0,
        "codiconFontSize",
        "Base font size for codicons.",
    ),
    px(
        CODICON_FONT_SIZE_COMPACT,
        12.0,
        "codiconFontSizeCompact",
        "Compact font size for codicons.",
    ),
    // ------ Corner Radii
    px(
        CORNER_RADIUS_MEDIUM,
        6.0,
        "cornerRadiusMedium",
        "Base corner radius for UI elements.",
    ),
    px(
        CORNER_RADIUS_X_SMALL,
        2.0,
        "cornerRadiusXSmall",
        "Extra small corner radius for very compact UI elements.",
    ),
    px(
        CORNER_RADIUS_SMALL,
        4.0,
        "cornerRadiusSmall",
        "Small corner radius for compact UI elements.",
    ),
    px(
        CORNER_RADIUS_LARGE,
        8.0,
        "cornerRadiusLarge",
        "Large corner radius for prominent UI elements.",
    ),
    px(
        CORNER_RADIUS_X_LARGE,
        12.0,
        "cornerRadiusXLarge",
        "Extra large corner radius for very prominent UI elements.",
    ),
    px(
        CORNER_RADIUS_CIRCLE,
        9999.0,
        "cornerRadiusCircle",
        "Circular corner radius for fully rounded UI elements.",
    ),
    // ------ Stroke Thickness
    px(
        STROKE_THICKNESS,
        1.0,
        "strokeThickness",
        "Base stroke thickness for borders and outlines.",
    ),
    // ------ Spacing ramp
    px(SPACING_NONE, 0.0, "spacingNone", "No spacing (0px)."),
    px(SPACING_SIZE20, 2.0, "spacingSize20", "Spacing of 2px."),
    px(SPACING_SIZE40, 4.0, "spacingSize40", "Spacing of 4px."),
    px(SPACING_SIZE60, 6.0, "spacingSize60", "Spacing of 6px."),
    px(SPACING_SIZE80, 8.0, "spacingSize80", "Spacing of 8px."),
    px(SPACING_SIZE100, 10.0, "spacingSize100", "Spacing of 10px."),
    px(SPACING_SIZE120, 12.0, "spacingSize120", "Spacing of 12px."),
    px(SPACING_SIZE160, 16.0, "spacingSize160", "Spacing of 16px."),
    px(SPACING_SIZE200, 20.0, "spacingSize200", "Spacing of 20px."),
    px(SPACING_SIZE240, 24.0, "spacingSize240", "Spacing of 24px."),
    px(SPACING_SIZE280, 28.0, "spacingSize280", "Spacing of 28px."),
    px(SPACING_SIZE320, 32.0, "spacingSize320", "Spacing of 32px."),
    px(SPACING_SIZE360, 36.0, "spacingSize360", "Spacing of 36px."),
    px(SPACING_SIZE400, 40.0, "spacingSize400", "Spacing of 40px."),
];

/// Registra os tamanhos base em `registry`, na ordem do original.
pub fn register_base_sizes(registry: &SizeRegistry) {
    for base in BASE_SIZES {
        let deprecation_message = base.deprecation_message.map(|message| {
            nls::localize(&format!("{}.deprecated", base.key), message, &[])
        });
        registry.register_size(
            base.id,
            Some(size_for_all_themes(base.value, base.unit).into()),
            &nls::localize(base.key, base.description, &[]),
            deprecation_message.as_deref(),
        );
    }
}
