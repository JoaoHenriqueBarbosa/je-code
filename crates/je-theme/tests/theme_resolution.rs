//! A resolução de cor do `ColorThemeData.getColor` (`colorThemeData.ts`): tema, `include`, `"default"`,
//! `null`, hexadecimal inválido, customizações do usuário, cores transientes e o padrão do registro.

use std::collections::HashMap;
use std::path::PathBuf;

use je_render::Rgba;
use je_theme::color::Color;
use je_theme::registry::{
    ColorContribution, ColorRegistry, ColorValue, Defaults, as_css_variable_name,
};
use je_theme::{ColorTheme, ThemeType};

/// Escreve os temas num diretório temporário próprio do teste e carrega o último.
fn load(test: &str, files: &[(&str, &str)]) -> ColorTheme {
    let dir: PathBuf = std::env::temp_dir().join(format!("je-theme-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, content) in files {
        std::fs::write(dir.join(name), content).unwrap();
    }
    let theme = ColorTheme::load(&dir.join(files.last().unwrap().0)).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    theme
}

fn css(color: Option<Color>) -> Option<String> {
    color.map(|c| c.to_string())
}

#[test]
fn theme_color_wins_over_registry_default() {
    let theme = load(
        "wins",
        &[("t.json", r##"{ "colors": { "foreground": "#112233" } }"##)],
    );
    assert_eq!(
        css(theme.resolve_color("foreground", true)),
        Some("#112233".into())
    );
    assert_eq!(
        css(theme.resolve_color("foreground", false)),
        Some("#112233".into())
    );
    assert_eq!(
        theme.get_color("foreground"),
        Some(Rgba::new(0x11, 0x22, 0x33, 0xff))
    );
}

#[test]
fn undefined_color_falls_back_to_the_registry_default_for_the_theme_type() {
    let dark = load("dark", &[("t.json", "{}")]);
    assert_eq!(
        css(dark.resolve_color("foreground", true)),
        Some("#cccccc".into())
    );
    assert_eq!(dark.resolve_color("foreground", false), None);
    let light = load("light", &[("t.json", "{}")]).with_type(ThemeType::Light);
    assert_eq!(
        css(light.resolve_color("foreground", true)),
        Some("#616161".into())
    );
    let hc = load("hc", &[("t.json", "{}")]).with_type(ThemeType::HighContrastDark);
    assert_eq!(
        css(hc.resolve_color("foreground", true)),
        Some("#ffffff".into())
    );
}

#[test]
fn registry_defaults_resolve_references_and_transforms_in_the_theme_context() {
    // `descriptionForeground` (dark) é `transparent(foreground, 0.7)`: segue o `foreground` do tema.
    let theme = load(
        "transform",
        &[("t.json", r##"{ "colors": { "foreground": "#ffffff" } }"##)],
    );
    assert_eq!(
        css(theme.resolve_color("descriptionForeground", true)),
        Some("rgba(255, 255, 255, 0.7)".into())
    );
    assert_eq!(
        theme.get_color("descriptionForeground"),
        Some(Rgba::new(255, 255, 255, 179))
    );
}

#[test]
fn null_default_resolves_to_nothing() {
    let theme = load("null", &[("t.json", "{}")]);
    assert_eq!(theme.resolve_color("selection.background", true), None);
    assert_eq!(theme.resolve_color("not.a.registered.color", true), None);
}

#[test]
fn invalid_hex_in_a_theme_is_red_like_color_from_hex() {
    let theme = load(
        "invalid",
        &[(
            "t.json",
            r##"{ "colors": { "foreground": "not a color" } }"##,
        )],
    );
    assert_eq!(
        css(theme.resolve_color("foreground", true)),
        Some("#ff0000".into())
    );
}

#[test]
fn non_string_values_are_ignored_and_keep_the_included_color() {
    let theme = load(
        "nonstring",
        &[
            (
                "base.json",
                r##"{ "colors": { "foreground": "#010203", "focusBorder": "#040506" } }"##,
            ),
            (
                "t.json",
                r##"{ "include": "./base.json", "colors": { "foreground": null, "focusBorder": 5 } }"##,
            ),
        ],
    );
    assert_eq!(
        css(theme.resolve_color("foreground", false)),
        Some("#010203".into())
    );
    assert_eq!(
        css(theme.resolve_color("focusBorder", false)),
        Some("#040506".into())
    );
}

#[test]
fn default_in_the_including_theme_removes_the_included_color() {
    let theme = load(
        "default",
        &[
            (
                "base.json",
                r##"{ "colors": { "foreground": "#010203" } }"##,
            ),
            (
                "t.json",
                r##"{ "include": "./base.json", "colors": { "foreground": "default" } }"##,
            ),
        ],
    );
    assert!(!theme.defines("foreground"));
    assert_eq!(theme.resolve_color("foreground", false), None);
    assert_eq!(
        css(theme.resolve_color("foreground", true)),
        Some("#cccccc".into())
    );
}

#[test]
fn including_theme_overrides_included_theme() {
    let theme = load(
        "override",
        &[
            (
                "base.json",
                r##"{ "name": "Base", "colors": { "foreground": "#010203", "focusBorder": "#040506" } }"##,
            ),
            (
                "t.json",
                r##"{ "name": "Filho", "include": "./base.json", "colors": { "foreground": "#0a0b0c" } }"##,
            ),
        ],
    );
    assert_eq!(theme.name, "Filho");
    assert_eq!(
        css(theme.resolve_color("foreground", false)),
        Some("#0a0b0c".into())
    );
    assert_eq!(
        css(theme.resolve_color("focusBorder", false)),
        Some("#040506".into())
    );
}

#[test]
fn customizations_win_over_the_theme_and_default_asks_for_the_registry_default() {
    let mut theme = load(
        "custom",
        &[(
            "t.json",
            r##"{ "colors": { "foreground": "#010203", "focusBorder": "#040506" } }"##,
        )],
    );
    theme.set_custom_colors([("foreground", "#aabbcc"), ("focusBorder", "default")]);
    assert_eq!(
        css(theme.resolve_color("foreground", true)),
        Some("#aabbcc".into())
    );
    assert!(theme.defines("foreground"));
    // "default" ignora a cor do tema e cai no padrão do registro.
    assert!(!theme.defines("focusBorder"));
    assert_eq!(theme.resolve_color("focusBorder", false), None);
    assert_eq!(
        css(theme.resolve_color("focusBorder", true)),
        Some("#007fd4".into())
    );
    // Uma customização inválida vira vermelho, como `Color.fromHex`.
    theme.set_custom_colors([("foreground", "xyz")]);
    assert_eq!(
        css(theme.resolve_color("foreground", true)),
        Some("#ff0000".into())
    );
    // `setCustomColors` substitui as customizações anteriores.
    theme.set_custom_colors(Vec::<(String, String)>::new());
    assert_eq!(
        css(theme.resolve_color("foreground", true)),
        Some("#010203".into())
    );
}

#[test]
fn transient_colors_win_over_everything() {
    let mut theme = load(
        "transient",
        &[("t.json", r##"{ "colors": { "foreground": "#010203" } }"##)],
    );
    theme.set_custom_colors([("foreground", "#aabbcc")]);
    theme.set_transient_colors(Some(HashMap::from([(
        "foreground".to_owned(),
        Color::from_hex("#102030"),
    )])));
    assert_eq!(
        css(theme.resolve_color("foreground", true)),
        Some("#102030".into())
    );
    assert!(theme.defines("foreground"));
    theme.set_transient_colors(None);
    assert_eq!(
        css(theme.resolve_color("foreground", true)),
        Some("#aabbcc".into())
    );
}

#[test]
fn if_defined_then_else_follows_what_the_theme_defines() {
    // `list.focusHighlightForeground` (light): ifDefinedThenElse(list.activeSelectionBackground, list.highlightForeground, '#BBE7FF')
    let without = load("ifdef-without", &[("t.json", "{}")]).with_type(ThemeType::Light);
    assert_eq!(
        css(without.resolve_color("list.focusHighlightForeground", true)),
        Some("#bbe7ff".into())
    );
    let with = load(
        "ifdef-with",
        &[(
            "t.json",
            r##"{ "colors": { "list.activeSelectionBackground": "#000000" } }"##,
        )],
    )
    .with_type(ThemeType::Light);
    assert_eq!(
        css(with.resolve_color("list.focusHighlightForeground", true)),
        Some("#0066bf".into())
    );
}

#[test]
fn css_variable_name_replaces_dots_with_hyphens() {
    assert_eq!(
        as_css_variable_name("editorSuggestWidget.background"),
        "--vscode-editorSuggestWidget-background"
    );
    assert_eq!(as_css_variable_name("foreground"), "--vscode-foreground");
}

#[test]
fn registry_register_replace_update_and_deregister() {
    let contribution = |id, description| ColorContribution {
        id,
        description,
        defaults: Some(Defaults::Single(ColorValue::Hex("#112233"))),
        needs_transparency: false,
        deprecation_message: None,
    };
    let mut registry = ColorRegistry::new();
    registry.register_color(contribution("a", "primeira"));
    registry.register_color(contribution("b", "segunda"));
    registry.register_color(contribution("c", "terceira"));
    // Registrar de novo substitui sem mudar a posição.
    registry.register_color(contribution("a", "primeira de novo"));
    let ids: Vec<_> = registry
        .get_colors()
        .iter()
        .map(|c| (c.id, c.description))
        .collect();
    assert_eq!(
        ids,
        [
            ("a", "primeira de novo"),
            ("b", "segunda"),
            ("c", "terceira")
        ]
    );

    registry.update_default_color("b", None);
    assert_eq!(registry.get("b").unwrap().defaults, None);
    registry.update_default_color("inexistente", None);

    registry.deregister_color("a");
    assert!(registry.get("a").is_none());
    assert_eq!(registry.get("c").unwrap().description, "terceira");
    assert_eq!(registry.get_colors().len(), 2);
}
