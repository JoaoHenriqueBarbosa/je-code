//! Porte de `src/vs/platform/theme/test/common/sizeRegistry.test.ts` (suíte "Size Registry"), caso a caso
//! na ordem do original. O `ensureNoDisposablesAreLeakedInTestSuite` não tem par: o registro não guarda
//! disposables além do emitter do próprio singleton. Cada teste roda numa thread com o seu registro
//! (singleton `thread_local`), já com os tamanhos base registrados.

use je_theme::size_registry::{
    BODY_FONT_SIZE, BODY_FONT_SIZE_SMALL, CODICON_FONT_SIZE, CORNER_RADIUS_LARGE,
    CORNER_RADIUS_MEDIUM, CORNER_RADIUS_SMALL, STROKE_THICKNESS, SizeDefaults, SizeUnit, SizeValue,
    as_css_variable, as_css_variable_name, get_size_registry, register_size, size,
    size_for_all_themes, size_value_to_css,
};

fn same_size_for_all_themes(value: f64) -> SizeDefaults {
    SizeDefaults {
        dark: Some(size(value, SizeUnit::Px)),
        light: Some(size(value, SizeUnit::Px)),
        hc_dark: Some(size(value, SizeUnit::Px)),
        hc_light: Some(size(value, SizeUnit::Px)),
    }
}

#[test]
fn register_size_should_register_a_size_token() {
    let id = register_size(
        "test.size",
        Some(same_size_for_all_themes(10.0).into()),
        "Test size",
        None,
    );
    assert_eq!(id, "test.size");

    let sizes = get_size_registry().get_sizes();
    let test_size = sizes.iter().find(|s| s.id == "test.size");
    let test_size = test_size.expect("test.size deveria estar registrado");
    assert_eq!(test_size.description, "Test size");

    get_size_registry().deregister_size("test.size");
}

#[test]
fn size_value_to_css_should_convert_size_value_to_css_string() {
    assert_eq!(size_value_to_css(&size(10.0, SizeUnit::Px)), "10px");
    assert_eq!(size_value_to_css(&size(1.5, SizeUnit::Rem)), "1.5rem");
    assert_eq!(size_value_to_css(&size(100.0, SizeUnit::Percent)), "100%");
    assert_eq!(size_value_to_css(&size(1.2, SizeUnit::Em)), "1.2em");
}

#[test]
fn as_css_variable_name_should_convert_identifier_to_css_variable_name() {
    assert_eq!(as_css_variable_name("fontSize"), "--vscode-fontSize");
    assert_eq!(
        as_css_variable_name("corner.radius"),
        "--vscode-corner-radius"
    );
    assert_eq!(
        as_css_variable_name("font.size.large"),
        "--vscode-font-size-large"
    );
}

#[test]
fn as_css_variable_should_create_css_variable_reference() {
    assert_eq!(as_css_variable("fontSize"), "var(--vscode-fontSize)");
    assert_eq!(
        as_css_variable("cornerRadius"),
        "var(--vscode-cornerRadius)"
    );
}

#[test]
fn deregister_size_should_remove_a_size_token() {
    register_size(
        "test.remove",
        Some(same_size_for_all_themes(5.0).into()),
        "Test remove",
        None,
    );

    let sizes = get_size_registry().get_sizes();
    assert!(sizes.iter().any(|s| s.id == "test.remove"));

    get_size_registry().deregister_size("test.remove");

    let sizes = get_size_registry().get_sizes();
    assert!(!sizes.iter().any(|s| s.id == "test.remove"));
}

#[test]
fn size_tokens_should_be_available() {
    let sizes = get_size_registry().get_sizes();

    // Check that base sizes are registered
    let registered = |id: &str| sizes.iter().any(|s| s.id == id);
    assert!(registered(BODY_FONT_SIZE), "bodyFontSize should be registered");
    assert!(
        registered(BODY_FONT_SIZE_SMALL),
        "bodyFontSizeSmall should be registered"
    );
    assert!(
        registered(CODICON_FONT_SIZE),
        "codiconFontSize should be registered"
    );
    assert!(
        registered(CORNER_RADIUS_MEDIUM),
        "cornerRadius.medium should be registered"
    );
    assert!(
        registered(CORNER_RADIUS_SMALL),
        "cornerRadius.small should be registered"
    );
    assert!(
        registered(CORNER_RADIUS_LARGE),
        "cornerRadius.large should be registered"
    );
    assert!(
        registered(STROKE_THICKNESS),
        "strokeThickness should be registered"
    );
}

#[test]
fn size_for_all_themes_should_create_same_value_for_all_themes() {
    let size_defaults = size_for_all_themes(10.0, SizeUnit::Px);
    let expected = Some(SizeValue {
        value: 10.0,
        unit: SizeUnit::Px,
    });
    assert_eq!(size_defaults.light, expected);
    assert_eq!(size_defaults.dark, expected);
    assert_eq!(size_defaults.hc_dark, expected);
    assert_eq!(size_defaults.hc_light, expected);
}

#[test]
fn register_size_should_work_with_size_for_all_themes() {
    let id = register_size(
        "test.allThemes",
        Some(size_for_all_themes(5.0, SizeUnit::Rem).into()),
        "Test all themes",
        None,
    );
    assert_eq!(id, "test.allThemes");

    let sizes = get_size_registry().get_sizes();
    assert!(sizes.iter().any(|s| s.id == "test.allThemes"));

    get_size_registry().deregister_size("test.allThemes");
}
