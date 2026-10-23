//! Testes próprios de `desktop_environment_info.rs`, que o VS Code não testa. Não é porte de teste do VS Code.
//! As expectativas seguem o `switch` do `desktopEnvironmentInfo.ts`, ramo a ramo.

use je_base::desktop_environment_info::{DesktopEnvironment, get_desktop_environment};
use je_base::process::ProcessEnvironment;

fn detect(entries: &[(&str, &str)]) -> DesktopEnvironment {
    let env: ProcessEnvironment = entries.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    get_desktop_environment(&env)
}

#[test]
fn reads_xdg_current_desktop_first() {
    assert_eq!(detect(&[("XDG_CURRENT_DESKTOP", "ubuntu:GNOME")]), DesktopEnvironment::Gnome);
    assert_eq!(detect(&[("XDG_CURRENT_DESKTOP", " X-Cinnamon ")]), DesktopEnvironment::Cinnamon);
    assert_eq!(detect(&[("XDG_CURRENT_DESKTOP", "Unity")]), DesktopEnvironment::Unity);
    assert_eq!(detect(&[("XDG_CURRENT_DESKTOP", "Unity"), ("DESKTOP_SESSION", "gnome-fallback")]), DesktopEnvironment::Gnome);
    assert_eq!(detect(&[("XDG_CURRENT_DESKTOP", "KDE")]), DesktopEnvironment::Kde4);
    assert_eq!(detect(&[("XDG_CURRENT_DESKTOP", "KDE"), ("KDE_SESSION_VERSION", "5")]), DesktopEnvironment::Kde5);
    assert_eq!(detect(&[("XDG_CURRENT_DESKTOP", "KDE"), ("KDE_SESSION_VERSION", "6")]), DesktopEnvironment::Kde6);
    assert_eq!(detect(&[("XDG_CURRENT_DESKTOP", "LXQt")]), DesktopEnvironment::Lxqt);
    assert_eq!(detect(&[("XDG_CURRENT_DESKTOP", "Foo:XFCE")]), DesktopEnvironment::Xfce);
}

#[test]
fn falls_back_to_desktop_session_and_markers() {
    assert_eq!(detect(&[("DESKTOP_SESSION", "mate")]), DesktopEnvironment::Gnome);
    assert_eq!(detect(&[("DESKTOP_SESSION", "kde")]), DesktopEnvironment::Kde3);
    assert_eq!(detect(&[("DESKTOP_SESSION", "kde"), ("KDE_SESSION_VERSION", "")]), DesktopEnvironment::Kde4);
    assert_eq!(detect(&[("DESKTOP_SESSION", "xubuntu")]), DesktopEnvironment::Xfce);
    assert_eq!(detect(&[("GNOME_DESKTOP_SESSION_ID", "this-is-deprecated")]), DesktopEnvironment::Gnome);
    assert_eq!(detect(&[("KDE_FULL_SESSION", "true")]), DesktopEnvironment::Kde3);
    assert_eq!(detect(&[("KDE_FULL_SESSION", "true"), ("KDE_SESSION_VERSION", "5")]), DesktopEnvironment::Kde4);
    assert_eq!(detect(&[("XDG_CURRENT_DESKTOP", "Weird"), ("DESKTOP_SESSION", "ukui")]), DesktopEnvironment::Ukui);
    assert_eq!(detect(&[]), DesktopEnvironment::Unknown);
    assert_eq!(DesktopEnvironment::Kde6.name(), "KDE6");
}
