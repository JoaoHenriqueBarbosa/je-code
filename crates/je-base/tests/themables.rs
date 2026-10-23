//! Casos próprios do porte de `themables.ts`: o arquivo não tem teste no original (só é exercitado pelas
//! suítes de `iconLabels` e `markdownString`).

use je_base::themables::{IconModifier, ThemeColor, ThemeIcon, theme_color_from_id};

#[test]
fn class_names_follow_the_icon_id() {
    assert_eq!(ThemeIcon::from_id("add").as_class_name(), "codicon codicon-add");
    assert_eq!(ThemeIcon::from_id("loading~spin").as_class_name(), "codicon codicon-loading codicon-modifier-spin");
    assert_eq!(ThemeIcon::from_id("git-merge").as_css_selector(), ".codicon.codicon-git-merge");
    // Um id inválido cai no ícone de erro.
    assert_eq!(ThemeIcon::from_id("not valid!").as_class_name(), "codicon codicon-error");
    assert_eq!(ThemeIcon::from_id("").as_class_name_array(), vec!["codicon", "codicon-error"]);
}

#[test]
fn from_string_modify_and_modifier() {
    assert_eq!(ThemeIcon::from_string("$(zap)"), Some(ThemeIcon::from_id("zap")));
    assert_eq!(ThemeIcon::from_string("$(loading~spin)"), Some(ThemeIcon::from_id("loading~spin")));
    assert_eq!(ThemeIcon::from_string("$(zap) "), None);
    assert_eq!(ThemeIcon::from_string("zap"), None);

    let spin = ThemeIcon::modify(&ThemeIcon::from_id("sync"), Some(IconModifier::Spin));
    assert_eq!(spin.id, "sync~spin");
    assert_eq!(ThemeIcon::get_modifier(&spin), Some("spin"));
    assert_eq!(ThemeIcon::modify(&spin, Some(IconModifier::Disabled)).id, "sync~disabled");
    assert_eq!(ThemeIcon::modify(&spin, None).id, "sync");
    assert_eq!(ThemeIcon::get_modifier(&ThemeIcon::from_id("sync")), None);
}

#[test]
fn equality_and_kind() {
    let mut a = ThemeIcon::from_id("file");
    let b = ThemeIcon::from_id("file");
    assert!(ThemeIcon::is_equal(&a, &b));
    a.color = Some(theme_color_from_id("red"));
    assert!(!ThemeIcon::is_equal(&a, &b));
    assert_eq!(a.color, Some(ThemeColor::new("red")));
    assert!(ThemeIcon::is_file(Some(&a)));
    assert!(!ThemeIcon::is_file(None));
    assert!(ThemeIcon::is_folder(Some(&ThemeIcon::from_id("folder"))));
    assert!(!ThemeIcon::is_folder(Some(&a)));
}
