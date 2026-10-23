//! Porte de `src/vs/editor/common/config/fontInfo.ts`.
//!
//! Decisões de tradução:
//! - As constantes que o original calcula ao carregar o módulo com `platform.isMacintosh` e
//!   `platform.isWindows` viram funções, que consultam a plataforma detectada uma vez por thread
//!   ([`Platform::detect`]).
//! - `FontInfo extends BareFontInfo` vira composição com `Deref<Target = BareFontInfo>`, e o objeto de
//!   opções do construtor é dividido em [`BareFontInfoOptions`] (os campos da base) e
//!   [`FontInfoOptions`] (a base mais as medidas).
//! - Números do JavaScript são `f64`. `lineHeight | 0` trunca para inteiro de 32 bits.
//!
//! Fora do porte, por dependerem de `EditorOption` e `IValidatedEditorOptions` (ainda inexistentes):
//! `IValidatedEditorOptions`, `BareFontInfo.createFromValidatedSettings` e
//! `BareFontInfo.createFromRawSettings`.

use std::ops::Deref;

use je_base::platform::Platform;

use super::editor_zoom;

/// Plataforma consultada pelas constantes deste módulo: `(is_macintosh, is_windows)`.
fn platform_flags() -> (bool, bool) {
    thread_local! {
        static FLAGS: (bool, bool) = {
            let platform = Platform::detect();
            (platform.is_macintosh, platform.is_windows)
        };
    }
    FLAGS.with(|flags| *flags)
}

/// `GOLDEN_LINE_HEIGHT_RATIO`: determinada por observação empírica (`isMacintosh ? 1.5 : 1.35`).
pub fn golden_line_height_ratio() -> f64 {
    if platform_flags().0 { 1.5 } else { 1.35 }
}

/// `MINIMUM_LINE_HEIGHT`.
pub const MINIMUM_LINE_HEIGHT: f64 = 8.0;

/// `FONT_VARIATION_OFF`.
pub const FONT_VARIATION_OFF: &str = "normal";

/// `FONT_VARIATION_TRANSLATE`.
pub const FONT_VARIATION_TRANSLATE: &str = "translate";

/// `DEFAULT_WINDOWS_FONT_FAMILY`.
pub const DEFAULT_WINDOWS_FONT_FAMILY: &str = "Consolas, 'Courier New', monospace";

/// `DEFAULT_MAC_FONT_FAMILY`.
pub const DEFAULT_MAC_FONT_FAMILY: &str = "Menlo, Monaco, 'Courier New', monospace";

/// `DEFAULT_LINUX_FONT_FAMILY`.
pub const DEFAULT_LINUX_FONT_FAMILY: &str = "'Droid Sans Mono', monospace";

/// `SERIALIZED_FONT_INFO_VERSION`: muda sempre que os membros de [`FontInfo`] mudam.
pub const SERIALIZED_FONT_INFO_VERSION: f64 = 2.0;

/// `EDITOR_FONT_DEFAULTS`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorFontDefaults {
    pub font_family: &'static str,
    pub font_weight: &'static str,
    pub font_size: f64,
    pub line_height: f64,
    pub letter_spacing: f64,
}

/// `EDITOR_FONT_DEFAULTS`, conforme a plataforma detectada.
pub fn editor_font_defaults() -> EditorFontDefaults {
    let (is_macintosh, is_windows) = platform_flags();
    EditorFontDefaults {
        font_family: if is_macintosh {
            DEFAULT_MAC_FONT_FAMILY
        } else if is_windows {
            DEFAULT_WINDOWS_FONT_FAMILY
        } else {
            DEFAULT_LINUX_FONT_FAMILY
        },
        font_weight: "normal",
        font_size: if is_macintosh { 12.0 } else { 14.0 },
        line_height: 0.0,
        letter_spacing: 0.0,
    }
}

/// `Math.round`: arredonda para cima nos meios (`-2.5` vira `-2`).
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// `parseInt(text, 10)`: `NaN` quando não há dígito.
fn parse_int_base10(text: &str) -> f64 {
    let text = text.trim_start();
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let digits: Vec<u8> = digits.bytes().take_while(u8::is_ascii_digit).collect();
    if digits.is_empty() {
        return f64::NAN;
    }
    let value = digits.iter().fold(0.0, |acc, d| acc * 10.0 + f64::from(d - b'0'));
    if negative && value != 0.0 { -value } else { value }
}

/// Os campos de [`BareFontInfo`] (o objeto de opções do construtor protegido do original).
#[derive(Clone, Debug, PartialEq)]
pub struct BareFontInfoOptions {
    pub pixel_ratio: f64,
    pub font_family: String,
    pub font_weight: String,
    pub font_size: f64,
    pub font_feature_settings: String,
    pub font_variation_settings: String,
    pub line_height: f64,
    pub letter_spacing: f64,
}

/// `BareFontInfo`.
#[derive(Clone, Debug, PartialEq)]
pub struct BareFontInfo {
    pub pixel_ratio: f64,
    pub font_family: String,
    pub font_weight: String,
    pub font_size: f64,
    pub font_feature_settings: String,
    pub font_variation_settings: String,
    pub line_height: f64,
    pub letter_spacing: f64,
}

impl BareFontInfo {
    /// `BareFontInfo._create(...)`.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        font_family: &str,
        font_weight: &str,
        mut font_size: f64,
        font_feature_settings: &str,
        font_variation_settings: &str,
        mut line_height: f64,
        letter_spacing: f64,
        pixel_ratio: f64,
        ignore_editor_zoom: bool,
    ) -> BareFontInfo {
        if line_height == 0.0 {
            line_height = golden_line_height_ratio() * font_size;
        } else if line_height < MINIMUM_LINE_HEIGHT {
            // Valores pequenos demais para serem alturas de linha em pixels estão em ems.
            line_height *= font_size;
        }

        // Impõe inteiro e mínimo.
        line_height = js_round(line_height);
        if line_height < MINIMUM_LINE_HEIGHT {
            line_height = MINIMUM_LINE_HEIGHT;
        }

        let editor_zoom_level_multiplier = 1.0 + if ignore_editor_zoom { 0.0 } else { editor_zoom::get_zoom_level() * 0.1 };
        font_size *= editor_zoom_level_multiplier;
        line_height *= editor_zoom_level_multiplier;

        let mut font_weight = font_weight.to_owned();
        let mut font_variation_settings = font_variation_settings.to_owned();
        if font_variation_settings == FONT_VARIATION_TRANSLATE {
            if font_weight == "normal" || font_weight == "bold" {
                font_variation_settings = FONT_VARIATION_OFF.to_owned();
            } else {
                let font_weight_as_number = parse_int_base10(&font_weight);
                font_variation_settings = format!("'wght' {font_weight_as_number}");
                font_weight = "normal".to_owned();
            }
        }

        BareFontInfo::new(BareFontInfoOptions {
            pixel_ratio,
            font_family: font_family.to_owned(),
            font_weight,
            font_size,
            font_feature_settings: font_feature_settings.to_owned(),
            font_variation_settings,
            line_height,
            letter_spacing,
        })
    }

    /// O construtor protegido do original: `lineHeight | 0` trunca para inteiro de 32 bits.
    pub(crate) fn new(opts: BareFontInfoOptions) -> BareFontInfo {
        BareFontInfo {
            pixel_ratio: opts.pixel_ratio,
            font_family: opts.font_family,
            font_weight: opts.font_weight,
            font_size: opts.font_size,
            font_feature_settings: opts.font_feature_settings,
            font_variation_settings: opts.font_variation_settings,
            line_height: f64::from(opts.line_height as i32),
            letter_spacing: opts.letter_spacing,
        }
    }

    /// `getId()`.
    pub fn get_id(&self) -> String {
        format!(
            "{}-{}-{}-{}-{}-{}-{}-{}",
            self.pixel_ratio,
            self.font_family,
            self.font_weight,
            self.font_size,
            self.font_feature_settings,
            self.font_variation_settings,
            self.line_height,
            self.letter_spacing
        )
    }

    /// `getMassagedFontFamily()`: a família entre aspas (se preciso) seguida da família padrão de reserva.
    pub fn get_massaged_font_family(&self) -> String {
        let fallback_font_family = editor_font_defaults().font_family;
        let font_family = Self::wrap_in_quotes(&self.font_family);
        if !fallback_font_family.is_empty() && self.font_family != fallback_font_family {
            return format!("{font_family}, {fallback_font_family}");
        }
        font_family
    }

    fn wrap_in_quotes(font_family: &str) -> String {
        if font_family.contains([',', '"', '\'']) {
            // Parece que a família já está escapada.
            return font_family.to_owned();
        }
        if font_family.contains(['+', ' ']) {
            // Uma família com `+` ou espaço vai entre aspas.
            return format!("\"{font_family}\"");
        }
        font_family.to_owned()
    }
}

/// Os campos de [`FontInfo`]: os da base mais as medidas (o objeto de opções do construtor).
#[derive(Clone, Debug, PartialEq)]
pub struct FontInfoOptions {
    pub bare: BareFontInfoOptions,
    pub is_monospace: bool,
    pub typical_halfwidth_character_width: f64,
    pub typical_fullwidth_character_width: f64,
    pub can_use_halfwidth_rightwards_arrow: bool,
    pub space_width: f64,
    pub middot_width: f64,
    pub wsmiddot_width: f64,
    pub max_digit_width: f64,
}

/// `FontInfo`: o [`BareFontInfo`] (por `Deref`) mais as medidas dos caracteres.
#[derive(Clone, Debug, PartialEq)]
pub struct FontInfo {
    bare: BareFontInfo,
    pub version: f64,
    pub is_trusted: bool,
    pub is_monospace: bool,
    pub typical_halfwidth_character_width: f64,
    pub typical_fullwidth_character_width: f64,
    pub can_use_halfwidth_rightwards_arrow: bool,
    pub space_width: f64,
    pub middot_width: f64,
    pub wsmiddot_width: f64,
    pub max_digit_width: f64,
}

impl Deref for FontInfo {
    type Target = BareFontInfo;

    fn deref(&self) -> &BareFontInfo {
        &self.bare
    }
}

impl FontInfo {
    pub fn new(opts: FontInfoOptions, is_trusted: bool) -> FontInfo {
        FontInfo {
            bare: BareFontInfo::new(opts.bare),
            version: SERIALIZED_FONT_INFO_VERSION,
            is_trusted,
            is_monospace: opts.is_monospace,
            typical_halfwidth_character_width: opts.typical_halfwidth_character_width,
            typical_fullwidth_character_width: opts.typical_fullwidth_character_width,
            can_use_halfwidth_rightwards_arrow: opts.can_use_halfwidth_rightwards_arrow,
            space_width: opts.space_width,
            middot_width: opts.middot_width,
            wsmiddot_width: opts.wsmiddot_width,
            max_digit_width: opts.max_digit_width,
        }
    }

    /// `equals(other)`: não compara `pixelRatio`, `version`, `isTrusted` nem `isMonospace`, como o original.
    pub fn equals(&self, other: &FontInfo) -> bool {
        self.font_family == other.font_family
            && self.font_weight == other.font_weight
            && self.font_size == other.font_size
            && self.font_feature_settings == other.font_feature_settings
            && self.font_variation_settings == other.font_variation_settings
            && self.line_height == other.line_height
            && self.letter_spacing == other.letter_spacing
            && self.typical_halfwidth_character_width == other.typical_halfwidth_character_width
            && self.typical_fullwidth_character_width == other.typical_fullwidth_character_width
            && self.can_use_halfwidth_rightwards_arrow == other.can_use_halfwidth_rightwards_arrow
            && self.space_width == other.space_width
            && self.middot_width == other.middot_width
            && self.wsmiddot_width == other.wsmiddot_width
            && self.max_digit_width == other.max_digit_width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create(font_weight: &str, font_size: f64, variation: &str, line_height: f64, ignore_zoom: bool) -> BareFontInfo {
        BareFontInfo::create("Fira Code", font_weight, font_size, "", variation, line_height, 0.0, 1.0, ignore_zoom)
    }

    fn font_info(tweak: impl FnOnce(&mut FontInfoOptions)) -> FontInfo {
        let mut opts = FontInfoOptions {
            bare: BareFontInfoOptions {
                pixel_ratio: 1.0,
                font_family: "Fira Code".into(),
                font_weight: "normal".into(),
                font_size: 14.0,
                font_feature_settings: "normal".into(),
                font_variation_settings: "normal".into(),
                line_height: 19.0,
                letter_spacing: 0.0,
            },
            is_monospace: true,
            typical_halfwidth_character_width: 8.0,
            typical_fullwidth_character_width: 16.0,
            can_use_halfwidth_rightwards_arrow: true,
            space_width: 8.0,
            middot_width: 8.0,
            wsmiddot_width: 8.0,
            max_digit_width: 8.0,
        };
        tweak(&mut opts);
        FontInfo::new(opts, true)
    }

    #[test]
    fn zero_line_height_uses_golden_ratio() {
        let info = create("normal", 20.0, "normal", 0.0, true);
        assert_eq!(info.line_height, js_round(golden_line_height_ratio() * 20.0));
    }

    #[test]
    fn small_line_height_is_in_ems_and_has_a_floor() {
        assert_eq!(create("normal", 20.0, "normal", 1.5, true).line_height, 30.0);
        assert_eq!(create("normal", 4.0, "normal", 1.0, true).line_height, 8.0);
        assert_eq!(create("normal", 14.0, "normal", 12.0, true).line_height, 12.0);
    }

    #[test]
    fn zoom_scales_font_size_and_line_height() {
        editor_zoom::set_zoom_level(2.0);
        let zoomed = create("normal", 10.0, "normal", 20.0, false);
        let ignored = create("normal", 10.0, "normal", 20.0, true);
        editor_zoom::set_zoom_level(0.0);
        assert!((zoomed.font_size - 12.0).abs() < 1e-9);
        assert_eq!(zoomed.line_height, 24.0);
        assert_eq!(ignored.font_size, 10.0);
        assert_eq!(ignored.line_height, 20.0);
    }

    #[test]
    fn variation_translate() {
        let off = create("bold", 14.0, FONT_VARIATION_TRANSLATE, 20.0, true);
        assert_eq!(off.font_variation_settings, "normal");
        assert_eq!(off.font_weight, "bold");

        let numeric = create("450", 14.0, FONT_VARIATION_TRANSLATE, 20.0, true);
        assert_eq!(numeric.font_variation_settings, "'wght' 450");
        assert_eq!(numeric.font_weight, "normal");

        let nan = create("abc", 14.0, FONT_VARIATION_TRANSLATE, 20.0, true);
        assert_eq!(nan.font_variation_settings, "'wght' NaN");
    }

    #[test]
    fn get_id_joins_all_fields() {
        let info = create("normal", 14.0, "normal", 20.0, true);
        assert_eq!(info.get_id(), "1-Fira Code-normal-14--normal-20-0");
    }

    #[test]
    fn massaged_font_family_quotes_and_appends_fallback() {
        let fallback = editor_font_defaults().font_family;
        let info = create("normal", 14.0, "normal", 20.0, true);
        assert_eq!(info.get_massaged_font_family(), format!("\"Fira Code\", {fallback}"));

        let mut escaped = info.clone();
        escaped.font_family = "'Fira Code', monospace".into();
        assert_eq!(escaped.get_massaged_font_family(), format!("'Fira Code', monospace, {fallback}"));

        let mut plain = info.clone();
        plain.font_family = "Menlo".into();
        assert_eq!(plain.get_massaged_font_family(), format!("Menlo, {fallback}"));

        let mut default_family = info;
        default_family.font_family = fallback.into();
        assert_eq!(default_family.get_massaged_font_family(), fallback);
    }

    #[test]
    fn constructor_truncates_line_height() {
        assert_eq!(font_info(|o| o.bare.line_height = 19.9).line_height, 19.0);
    }

    #[test]
    fn equals_ignores_pixel_ratio_trust_and_monospace() {
        let base = font_info(|_| {});
        let other = font_info(|o| {
            o.bare.pixel_ratio = 2.0;
            o.is_monospace = false;
        });
        assert!(base.equals(&other));
        assert_eq!(base.version, SERIALIZED_FONT_INFO_VERSION);
        assert!(base.is_trusted);
    }

    #[test]
    fn equals_detects_differences() {
        let base = font_info(|_| {});
        assert!(!base.equals(&font_info(|o| o.bare.font_size = 15.0)));
        assert!(!base.equals(&font_info(|o| o.space_width = 9.0)));
        assert!(!base.equals(&font_info(|o| o.can_use_halfwidth_rightwards_arrow = false)));
        assert!(!base.equals(&font_info(|o| o.bare.font_family = "Other".into())));
    }

    #[test]
    fn parse_int_base10_follows_js() {
        assert_eq!(parse_int_base10("  42px"), 42.0);
        assert_eq!(parse_int_base10("-7"), -7.0);
        assert!(parse_int_base10("x").is_nan());
    }
}
