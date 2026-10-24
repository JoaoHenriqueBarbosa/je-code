//! Porte de `src/vs/workbench/contrib/terminal/test/common/terminalColorRegistry.test.ts` (suíte `Workbench - TerminalColorRegistry`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`registerColors()` e `Registry.as<IColorRegistry>(ThemeingExtensions.ColorContribution)`.** O registro de cores do
//!   `je-theme` já traz as cores `terminal.ansi*` registradas (ver as divergências de `terminal_color_registry.rs`):
//!   `color_registry()` é o `themingRegistry`.
//! - **`getMockTheme(type)`.** É o [`MockTheme`], que implementa `ColorThemeContext`: `getColor` resolve o padrão do registro
//!   no próprio tema, `defines` devolve verdadeiro sempre.
//! - **`ColorScheme`.** É o `ThemeType` do `je-theme`.
//! - **`Color.Format.CSS.formatHexA(color, true)`.** É `format_hex_a(&color, true)`.
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** Cada caso cria o guarda de vazamentos antes de tudo.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_theme::ThemeType;
use je_theme::color::Color;
use je_theme::color::format::css::format_hex_a;
use je_theme::registry::{ColorThemeContext, color_registry};
use je_workbench::contrib::terminal::common::terminal_color_registry::ANSI_COLOR_IDENTIFIERS;

struct MockTheme {
    theme_type: ThemeType,
}

fn get_mock_theme(theme_type: ThemeType) -> MockTheme {
    MockTheme { theme_type }
}

impl ColorThemeContext for MockTheme {
    fn theme_type(&self) -> ThemeType {
        self.theme_type
    }

    fn get_color(&self, id: &str, _use_default: bool) -> Option<Color> {
        color_registry().resolve_default_color(id, self)
    }

    fn defines(&self, _id: &str) -> bool {
        true
    }
}

fn hex_colors(theme: &MockTheme) -> Vec<String> {
    ANSI_COLOR_IDENTIFIERS.iter().map(|color_id| format_hex_a(&theme.get_color(color_id, true).expect("a cor ANSI tem padrão"), true)).collect()
}

#[test]
fn hc_colors() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = get_mock_theme(ThemeType::HighContrastDark);
    let colors = hex_colors(&theme);

    assert_eq!(
        colors,
        vec![
            "#000000", "#cd0000", "#00cd00", "#cdcd00", "#0000ee", "#cd00cd", "#00cdcd", "#e5e5e5", "#7f7f7f", "#ff0000", "#00ff00", "#ffff00", "#5c5cff", "#ff00ff",
            "#00ffff", "#ffffff"
        ],
        "The high contrast terminal colors should be used when the hc theme is active"
    );
}

#[test]
fn light_colors() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = get_mock_theme(ThemeType::Light);
    let colors = hex_colors(&theme);

    assert_eq!(
        colors,
        vec![
            "#000000", "#cd3131", "#107c10", "#949800", "#0451a5", "#bc05bc", "#0598bc", "#555555", "#666666", "#f14c4c", "#14ce14", "#b5ba00", "#3b8eea", "#d670d6",
            "#29b8db", "#a5a5a5"
        ],
        "The light terminal colors should be used when the light theme is active"
    );
}

#[test]
fn light_bright_colors_are_lighter_than_regular_colors() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = get_mock_theme(ThemeType::Light);
    let colors: Vec<Color> = ANSI_COLOR_IDENTIFIERS.iter().map(|color_id| theme.get_color(color_id, true).expect("a cor ANSI tem padrão")).collect();

    assert_eq!(
        colors[8..].iter().enumerate().map(|(index, bright_color)| bright_color.is_lighter_than(&colors[index])).collect::<Vec<_>>(),
        vec![true, true, true, true, true, true, true, true]
    );
}

#[test]
fn dark_colors() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let theme = get_mock_theme(ThemeType::Dark);
    let colors = hex_colors(&theme);

    assert_eq!(
        colors,
        vec![
            "#000000", "#cd3131", "#0dbc79", "#e5e510", "#2472c8", "#bc3fbc", "#11a8cd", "#e5e5e5", "#666666", "#f14c4c", "#23d18b", "#f5f543", "#3b8eea", "#d670d6",
            "#29b8db", "#e5e5e5"
        ],
        "The dark terminal colors should be used when a dark theme is active"
    );
}
