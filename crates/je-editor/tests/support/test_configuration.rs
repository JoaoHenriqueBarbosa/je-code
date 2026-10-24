//! Porte de `src/vs/editor/test/browser/config/testConfiguration.ts`: o `TestConfiguration`, uma
//! `EditorConfiguration` com o ambiente e a fonte fixos (a fonte `mockFont`, monoespaçada, com largura 10).
//!
//! Divergências:
//! - `TestConfiguration extends EditorConfiguration` e sobrescreve `_readEnvConfiguration` e `_readFontInfo`. Aqui
//!   os dois métodos são o [`TestEnvironment`], a implementação de `EditorEnvironment` que a
//!   `EditorConfiguration` recebe, e [`TestConfiguration::new`] monta a configuração (`super(false,
//!   MenuId.EditorContext, opts, null, new TestAccessibilityService())`).
//! - O `TestAccessibilityService` só entra pelo evento `onDidChangeScreenReaderOptimized` (que é `Event.None`), e a
//!   medição de fonte e a razão de pixels não mudam nunca: os três eventos de [`TestEnvironment`] são `Event::none()`.
//! - O `ElementSizeObserver(null, options.dimension)` não tem elemento de referência: só vale o `dimension` das
//!   opções (`{ width, height }`), se houver.
//! - `TestCodeEditorCreationOptions` é o objeto [`Dynamic`] das opções; o `envConfig` é lido dele a cada cálculo,
//!   com o `??` do original (`undefined` e `null` caem no padrão). O `accessibilitySupport` é o número do enum
//!   (`0` desconhecido, `1` desabilitado, `2` habilitado).

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_editor::config::editor_configuration_impl::{EditorConfiguration, EditorEnvironment, EnvConfiguration};
use je_editor::config::editor_options::font_ligatures::EditorFontLigatures;
use je_editor::config::editor_options::font_variations::EditorFontVariations;
use je_editor::config::editor_options::AccessibilitySupport;
use je_editor::config::element_size_observer::ElementSizeObserver;
use je_editor::config::font_info::{BareFontInfo, BareFontInfoOptions, FontInfo, FontInfoOptions};
use je_editor::core_2d::dimension::Dimension;
use je_platform::actions::menu_ids::editor_context;

/// `envConfig?.key`: `None` se o `envConfig` ou a chave são `undefined` ou `null`.
fn env_config_value(raw_options: &Dynamic, key: &str) -> Option<Dynamic> {
    let env_config = raw_options.get_str("envConfig");
    if env_config.is_nullish() {
        return None;
    }
    let value = env_config.get_str(key);
    (!value.is_nullish()).then_some(value)
}

fn env_config_number(raw_options: &Dynamic, key: &str, default: f64) -> f64 {
    match env_config_value(raw_options, key) {
        Some(Dynamic::Number(number)) => number,
        _ => default,
    }
}

/// O ambiente do `TestConfiguration` (`_readEnvConfiguration` e `_readFontInfo`).
pub struct TestEnvironment;

impl EditorEnvironment for TestEnvironment {
    fn read_env_configuration(&self, raw_options: &Dynamic, _container_observer: &ElementSizeObserver) -> EnvConfiguration {
        EnvConfiguration {
            extra_editor_class_name: match env_config_value(raw_options, "extraEditorClassName") {
                Some(Dynamic::String(text)) => text.to_string_lossy(),
                _ => String::new(),
            },
            outer_width: env_config_number(raw_options, "outerWidth", 100.0),
            outer_height: env_config_number(raw_options, "outerHeight", 100.0),
            empty_selection_clipboard: match env_config_value(raw_options, "emptySelectionClipboard") {
                Some(Dynamic::Bool(flag)) => flag,
                _ => true,
            },
            pixel_ratio: env_config_number(raw_options, "pixelRatio", 1.0),
            accessibility_support: match env_config_value(raw_options, "accessibilitySupport") {
                Some(Dynamic::Number(value)) if value == 1.0 => AccessibilitySupport::Disabled,
                Some(Dynamic::Number(value)) if value == 2.0 => AccessibilitySupport::Enabled,
                _ => AccessibilitySupport::Unknown,
            },
            edit_context_supported: true,
        }
    }

    fn read_font_info(&self, _styling: &BareFontInfo) -> FontInfo {
        FontInfo::new(
            FontInfoOptions {
                bare: BareFontInfoOptions {
                    pixel_ratio: 1.0,
                    font_family: "mockFont".to_owned(),
                    font_weight: "normal".to_owned(),
                    font_size: 14.0,
                    font_feature_settings: EditorFontLigatures::OFF.to_owned(),
                    font_variation_settings: EditorFontVariations::OFF.to_owned(),
                    line_height: 19.0,
                    letter_spacing: 1.5,
                },
                is_monospace: true,
                typical_halfwidth_character_width: 10.0,
                typical_fullwidth_character_width: 20.0,
                can_use_halfwidth_rightwards_arrow: true,
                space_width: 10.0,
                middot_width: 10.0,
                wsmiddot_width: 10.0,
                max_digit_width: 10.0,
            },
            true,
        )
    }

    fn on_did_change_font_measurements(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_pixel_ratio(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_screen_reader_optimized(&self) -> Event<()> {
        Event::none()
    }
}

/// `TestConfiguration`.
pub struct TestConfiguration;

impl TestConfiguration {
    /// `new TestConfiguration(opts)`.
    pub fn new(opts: &Dynamic) -> Rc<EditorConfiguration> {
        Self::with_environment(opts, Rc::new(TestEnvironment))
    }

    /// `new TestConfiguration(opts)` de uma subclasse que sobrescreve `_readEnvConfiguration` e/ou `_readFontInfo`:
    /// o `environment` é o que as sobrescritas fazem.
    pub fn with_environment(opts: &Dynamic, environment: Rc<dyn EditorEnvironment>) -> Rc<EditorConfiguration> {
        let dimension = opts.get_str("dimension");
        let dimension = if dimension.is_nullish() {
            None
        } else {
            match (dimension.get_str("width"), dimension.get_str("height")) {
                (Dynamic::Number(width), Dynamic::Number(height)) => Some(Dimension { width, height }),
                _ => None,
            }
        };
        EditorConfiguration::new(false, editor_context(), opts, ElementSizeObserver::new(None, dimension), environment)
    }
}
