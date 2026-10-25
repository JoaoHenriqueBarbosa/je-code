//! Porte de `src/vs/workbench/services/themes/browser/colorThemeCss.ts`: o CSS de um tema de cor (as variáveis
//! `--vscode-*` de cores e tamanhos mais as regras dos participantes de tema), uma função pura.
//!
//! Notas de tradução:
//! - O CSS gerado **não é usado na pintura**: os widgets nativos pintam o valor resolvido do tema. O texto serve à
//!   especificação, e os fixtures o interpretam num teste (`test/browser/component_fixtures/fixture_css.rs`).
//! - O parâmetro `environmentService` (repassado aos participantes) não existe, porque os participantes portados não
//!   o leem (ver `platform/theme/theme_service.rs`); `themingParticipants` e o serviço eram opcionais juntos, e aqui o
//!   chamador passa a lista (vazia quando não há participantes).
//! - `IColorTheme` para o registro de tamanhos (`resolveDefaultSize`) é o [`ColorThemeContext`] do `ColorThemeData`.

use je_theme::ThemeType;
use je_theme::color::Color;
use je_theme::registry::{ColorThemeContext, as_css_variable_name, color_registry};
use je_theme::size_utils::{get_size_registry, size_value_to_css};

use super::color_theme_data::ColorThemeData;
use crate::platform::theme::theme_service::{CssRuleSet, ICssStyleCollector, ThemingParticipant};

/// `CSSValue`: o CSS gerado.
pub struct CssValue {
    pub code: String,
}

impl ColorThemeContext for ColorThemeData {
    fn theme_type(&self) -> ThemeType {
        ColorThemeData::theme_type(self)
    }

    fn get_color(&self, id: &str, use_default: bool) -> Option<Color> {
        ColorThemeData::get_color(self, id, use_default)
    }

    fn defines(&self, id: &str) -> bool {
        ColorThemeData::defines(self, id)
    }
}

/// `generateColorThemeCSS(theme, scopeSelector, themingParticipants)`: a regra base, as regras dos participantes, e as
/// variáveis de cor e de tamanho do tema sob o `scope_selector`.
pub fn generate_color_theme_css(theme: &ColorThemeData, scope_selector: &str, theming_participants: &[ThemingParticipant]) -> CssValue {
    let mut collector = CssRuleSet::default();

    // Base rule
    collector.add_rule(&format!("{scope_selector} {{ forced-color-adjust: none; }}"));

    // Theming participant rules
    for participant in theming_participants {
        (**participant)(theme, &mut collector);
    }

    // Color CSS variables
    let mut variables: Vec<String> = Vec::new();
    for item in color_registry().get_colors() {
        if let Some(color) = ColorThemeData::get_color(theme, item.id, true) {
            variables.push(format!("{}: {};", as_css_variable_name(item.id), color));
        }
    }

    // Size CSS variables
    let size_registry = get_size_registry();
    for item in size_registry.get_sizes() {
        if let Some(size_value) = size_registry.resolve_default_size(&item.id, theme) {
            variables.push(format!("{}: {};", as_css_variable_name(&item.id), size_value_to_css(&size_value)));
        }
    }

    collector.add_rule(&format!("{scope_selector} {{ {} }}", variables.join("\n")));

    CssValue { code: collector.rules().join("\n") }
}
