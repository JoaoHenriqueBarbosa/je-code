//! Porte de `src/vs/editor/contrib/semanticTokens/common/semanticTokensConfig.ts`: a configuração
//! `editor.semanticHighlighting` e a decisão de colorir um modelo semanticamente.

use je_base::dynamic::Dynamic;
use je_base::JsString;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;

use crate::services::theme_service::IThemeService;
use crate::text_model::TextModel;

pub const SEMANTIC_HIGHLIGHTING_SETTING_ID: &str = "editor.semanticHighlighting";

/// `isSemanticColoringEnabled(model, themeService, configurationService)`.
pub fn is_semantic_coloring_enabled(model: &TextModel, theme_service: &dyn IThemeService, configuration_service: &dyn IConfigurationService) -> bool {
    let overrides = ConfigurationOverrides { resource: Some(model.uri()), override_identifier: Some(JsString::from(model.get_language_id().as_str())) };
    let options = configuration_service.get_value(Some(SEMANTIC_HIGHLIGHTING_SETTING_ID), Some(&overrides));
    // `IEditorSemanticHighlightingOptions | undefined`, depois `?.enabled`
    let setting = if options.is_nullish() { Dynamic::Undefined } else { options.get_str("enabled") };
    if let Dynamic::Bool(setting) = setting {
        return setting;
    }
    theme_service.get_color_theme().semantic_highlighting()
}
