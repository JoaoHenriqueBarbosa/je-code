//! Casos que o `vs/base` do VS Code não tem para o `platform.ts` (que não tem teste próprio) e para o que
//! o `labels.ts`, o `keybindingParser.ts` e o `keybindingLabels.ts` fazem fora do que o `labels.test.ts` e
//! o `keyCodes.test.ts` cobrem. Não é porte de teste do VS Code. Os resultados esperados de `untildify` e
//! dos mnemônicos foram conferidos rodando as expressões regulares do original no Node.

use je_base::key_codes::{KeyCode, ScanCode};
use je_base::keybinding_labels::ModifierLabelProvider;
use je_base::keybinding_parser::KeybindingParser;
use je_base::keybindings::{Chord, KeyCodeChord, Keybinding, ScanCodeChord};
use je_base::labels;
use je_base::path::Platform as PathFlavor;
use je_base::platform::{self, language, NativeEnvironment, OperatingSystem, Platform, PlatformKind, WebEnvironment};

fn native(process_platform: &str, vars: &[(&str, &str)]) -> Platform {
    let vars: Vec<(String, String)> = vars.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect();
    let env = move |name: &str| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    Platform::from_native(&NativeEnvironment { platform: process_platform, env: &env, is_electron: false })
}

fn web(user_agent: &str, max_touch_points: Option<u32>, language: &str) -> Platform {
    Platform::from_web(&WebEnvironment { user_agent: user_agent.into(), max_touch_points, language: language.into(), ..WebEnvironment::default() })
}

#[test]
fn native_platforms() {
    let win = native("win32", &[]);
    assert!(win.is_windows && win.is_native && !win.is_web);
    assert_eq!((win.os, win.kind, win.path_flavor()), (OperatingSystem::Windows, PlatformKind::Windows, PathFlavor::Win32));

    let mac = native("darwin", &[]);
    assert_eq!((mac.os, mac.kind, mac.path_flavor()), (OperatingSystem::Macintosh, PlatformKind::Mac, PathFlavor::Posix));

    let linux = native("linux", &[("SNAP", "x"), ("SNAP_REVISION", "7"), ("GITHUB_WORKSPACE", "/w")]);
    assert_eq!((linux.os, linux.kind), (OperatingSystem::Linux, PlatformKind::Linux));
    assert!(linux.is_linux_snap && linux.is_ci);
    assert!(!native("linux", &[("SNAP", "x")]).is_linux_snap);
    assert!(!native("linux", &[("CI", "")]).is_ci);

    // sistema não reconhecido: `OS` cai em Linux e `platform` em Web, como no original
    let other = native("freebsd", &[]);
    assert_eq!((other.os, other.kind), (OperatingSystem::Linux, PlatformKind::Web));
    assert_eq!(PlatformKind::Mac.name(), "Mac");
}

#[test]
fn nls_config_from_environment() {
    let default = native("linux", &[]);
    assert_eq!((default.language.as_str(), default.locale.as_deref(), default.platform_locale.as_deref()), ("en", Some("en"), Some("en")));

    let config = r#"{"userLocale":"pt-br","osLocale":"en-us","resolvedLanguage":"pt-br","languagePack":{"translationsConfigFile":"/t.json"}}"#;
    let p = native("linux", &[("VSCODE_NLS_CONFIG", config)]);
    assert_eq!(p.language, "pt-br");
    assert_eq!(p.locale.as_deref(), Some("pt-br"));
    assert_eq!(p.platform_locale.as_deref(), Some("en-us"));
    assert_eq!(p.translations_config_file.as_deref(), Some("/t.json"));

    // JSON inválido ou `null`: o `try/catch` do original deixa tudo como estava
    assert_eq!(native("linux", &[("VSCODE_NLS_CONFIG", "{")]), default);
    assert_eq!(native("linux", &[("VSCODE_NLS_CONFIG", "null")]), default);

    // sem `resolvedLanguage`, a língua cai no padrão, mas o locale vira `undefined`
    let p = native("linux", &[("VSCODE_NLS_CONFIG", "{}")]);
    assert_eq!((p.language.as_str(), p.locale, p.platform_locale), ("en", None, None));
}

#[test]
fn web_platforms() {
    let win = web("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/120.0 Safari/537.36 Edg/120.0", None, "PT-BR");
    assert!(win.is_windows && win.is_web && !win.is_native);
    assert_eq!(win.os, OperatingSystem::Windows);
    assert_eq!(win.locale.as_deref(), Some("pt-br"));
    assert_eq!(win.platform_locale.as_deref(), Some("pt-br"));
    assert!(win.is_chrome() && win.is_edge() && !win.is_safari() && !win.is_firefox());

    // Mac com tela de toque é iPad: conta como iOS, e o `OS` continua Macintosh
    let ipad = web("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 Safari/605.1.15", Some(5), "en-US");
    assert!(ipad.is_macintosh && ipad.is_ios && ipad.is_safari());
    assert!(!web("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", Some(0), "en").is_ios);

    // Android é Linux e móvel; CrOS também é Linux
    let android = web("Mozilla/5.0 (Linux; Android 14) Mobile Safari", None, "en");
    assert!(android.is_linux && android.is_mobile && android.is_android());
    assert!(web("Mozilla/5.0 (X11; CrOS x86_64)", None, "en").is_chrome_os);

    // sem nenhum sistema reconhecido: Web, `OS` Linux
    let nothing = Platform::from_web(&WebEnvironment::default());
    assert_eq!((nothing.kind, nothing.os, nothing.is_web), (PlatformKind::Web, OperatingSystem::Linux, true));
    assert_eq!(nothing.language, "en");

    let worker = Platform::from_web(&WebEnvironment { is_web_worker: true, web_worker_origin: Some("https://x".into()), nls_language: Some("de".into()), ..WebEnvironment::default() });
    assert!(worker.is_web_worker);
    assert_eq!((worker.web_worker_origin.as_deref(), worker.language.as_str()), (Some("https://x"), "de"));
}

#[test]
fn detect_matches_the_host() {
    let detected = Platform::detect();
    if cfg!(target_os = "linux") {
        assert!(detected.is_linux && detected.is_native);
        assert_eq!(detected.path_flavor(), PathFlavor::Posix);
    } else if cfg!(windows) {
        assert!(detected.is_windows);
        assert_eq!(detected.path_flavor(), PathFlavor::Win32);
    } else if cfg!(target_os = "macos") {
        assert!(detected.is_macintosh);
    }
}

#[test]
fn language_helpers_and_versions() {
    assert!(language::is_default("en") && !language::is_default("en-gb"));
    assert!(language::is_default_variant("en") && language::is_default_variant("en-gb") && language::is_default_variant("en-"));
    assert!(!language::is_default_variant("de") && !language::is_default_variant("e") && !language::is_default_variant("enx"));

    assert!(platform::is_tahoe_or_newer("25.0.0") && platform::is_tahoe_or_newer("26"));
    assert!(!platform::is_tahoe_or_newer("24.6.0") && !platform::is_tahoe_or_newer("abc"));
    assert!(platform::is_little_endian() == cfg!(target_endian = "little"));
}

fn chords(input: &str) -> Vec<Chord> {
    KeybindingParser::parse_keybinding(input).map(|k| k.chords).unwrap_or_default()
}

fn key(ctrl: bool, shift: bool, alt: bool, meta: bool, key_code: KeyCode) -> Chord {
    Chord::KeyCode(KeyCodeChord::new(ctrl, shift, alt, meta, key_code))
}

#[test]
fn keybinding_parser() {
    assert_eq!(KeybindingParser::parse_keybinding(""), None);
    assert_eq!(chords("ctrl+k ctrl+s"), vec![key(true, false, false, false, KeyCode::KeyK), key(true, false, false, false, KeyCode::KeyS)]);
    assert_eq!(chords("  alt-left  "), vec![key(false, false, true, false, KeyCode::LeftArrow)]);
    assert_eq!(chords("cmd+win-meta+x"), vec![key(false, false, false, true, KeyCode::KeyX)]);
    assert_eq!(chords("CTRL+SHIFT+ALT+META+F12 a"), vec![key(true, true, true, true, KeyCode::F12), key(false, false, false, false, KeyCode::KeyA)]);
    // um modificador sem tecla vira `Unknown`, como no original
    assert_eq!(chords("ctrl+"), vec![key(true, false, false, false, KeyCode::Unknown)]);
    assert_eq!(chords("oem_minus"), vec![key(false, false, false, false, KeyCode::Minus)]);
    // `[código]` é um `ScanCodeChord`; sem nome conhecido é `ScanCode::None`
    assert_eq!(chords("Ctrl-Shift+[KeyA]"), vec![Chord::ScanCode(ScanCodeChord::new(true, true, false, false, ScanCode::KeyA))]);
    assert_eq!(chords("shift+[nope]"), vec![Chord::ScanCode(ScanCodeChord::new(false, true, false, false, ScanCode::None))]);
    // `[]` e `[a]]` não casam a expressão do original, viram nomes de tecla desconhecidos
    assert_eq!(chords("shift+[]"), vec![key(false, true, false, false, KeyCode::Unknown)]);
    assert_eq!(chords("[a]]"), vec![key(false, false, false, false, KeyCode::Unknown)]);
}

#[test]
fn modifier_label_providers() {
    let chord = KeyCodeChord::new(true, true, true, true, KeyCode::KeyK);
    let label = |provider: &ModifierLabelProvider, os| provider.to_label(os, &[chord, chord], |_| Some("K".to_string())).unwrap();
    use OperatingSystem::{Linux, Macintosh, Windows};

    let ui = ModifierLabelProvider::ui();
    assert_eq!(label(&ui, Macintosh), "\u{2303}⇧⌥⌘K \u{2303}⇧⌥⌘K");
    assert_eq!(label(&ui, Windows), "Ctrl+Shift+Alt+Windows+K Ctrl+Shift+Alt+Windows+K");
    assert_eq!(label(&ui, Linux), "Ctrl+Shift+Alt+Super+K Ctrl+Shift+Alt+Super+K");

    let aria = ModifierLabelProvider::aria();
    assert_eq!(label(&aria, Macintosh), "Control+Shift+Option+Command+K Control+Shift+Option+Command+K");
    assert_eq!(label(&aria, Windows), "Control+Shift+Alt+Windows+K Control+Shift+Alt+Windows+K");
    assert_eq!(label(&aria, Linux), "Control+Shift+Alt+Super+K Control+Shift+Alt+Super+K");

    let electron = ModifierLabelProvider::electron_accelerator();
    assert_eq!(label(&electron, Macintosh), "Ctrl+Shift+Alt+Cmd+K Ctrl+Shift+Alt+Cmd+K");
    assert_eq!(label(&electron, Windows), "Ctrl+Shift+Alt+Super+K Ctrl+Shift+Alt+Super+K");
    assert_eq!(label(&electron, Linux), label(&electron, Windows));

    let settings = ModifierLabelProvider::user_settings();
    assert_eq!(label(&settings, Macintosh), "ctrl+shift+alt+cmd+K ctrl+shift+alt+cmd+K");
    assert_eq!(label(&settings, Windows), "ctrl+shift+alt+win+K ctrl+shift+alt+win+K");
    assert_eq!(label(&settings, Linux), "ctrl+shift+alt+meta+K ctrl+shift+alt+meta+K");

    // sem acordes, ou com um acorde sem rótulo de tecla, não há rótulo; tecla vazia deixa só os modificadores
    assert_eq!(ui.to_label::<KeyCodeChord>(Linux, &[], |_| Some(String::new())), None);
    assert_eq!(ui.to_label(Linux, &[chord], |_| None), None);
    assert_eq!(ui.to_label(Linux, &[chord], |_| Some(String::new())).as_deref(), Some("Ctrl+Shift+Alt+Super"));
}

#[test]
fn keybinding_hash_equality_and_errors() {
    let a = Keybinding::new(vec![key(true, false, false, false, KeyCode::KeyK), Chord::ScanCode(ScanCodeChord::new(false, true, false, true, ScanCode::KeyA))]).unwrap();
    assert_eq!(a.get_hash_code(), format!("K1000{};S0101{}", KeyCode::KeyK.0, ScanCode::KeyA.0));
    assert!(a.equals(Some(&a.clone())) && !a.equals(None));
    assert!(Keybinding::new(vec![]).is_err());
    assert_eq!(Keybinding::new(vec![]).unwrap_err().message, "Illegal argument: chords");
    assert!(KeyCodeChord::new(true, false, false, false, KeyCode::Ctrl).is_duplicate_modifier_case());
    assert!(ScanCodeChord::new(false, false, true, false, ScanCode::AltRight).is_duplicate_modifier_case());
    assert!(KeyCodeChord::new(false, false, false, false, KeyCode::Unknown).is_modifier_key());
}

#[test]
fn untildify_and_tildify() {
    assert_eq!(labels::untildify("~", "/home/u"), "/home/u");
    assert_eq!(labels::untildify("~/a", "/home/u"), "/home/u/a");
    assert_eq!(labels::untildify("~\\a", "C:\\u"), "C:\\u\\a");
    assert_eq!(labels::untildify("~x", "/h"), "~x");
    assert_eq!(labels::untildify("a~", "/h"), "a~");
    // o segundo argumento do `replace` interpreta `$&`, `$$`, `$'` e `$n`
    assert_eq!(labels::untildify("~/a", "/h$&x"), "/h~/x/a");
    assert_eq!(labels::untildify("~/a", "/h$$"), "/h$/a");
    assert_eq!(labels::untildify("~/a", "/h$'"), "/ha/a");
    assert_eq!(labels::untildify("~/a", "/h$1$2$10"), "/h/$2/0/a");
    assert_eq!(labels::untildify("~/a", "/h$`z$"), "/hz$1a");

    let linux = native("linux", &[]);
    let windows = native("win32", &[]);
    assert_eq!(labels::tildify("/home/u/x", "/home/u", OperatingSystem::Linux, &linux), "~/x");
    assert_eq!(labels::tildify("/Home/u/x", "/home/u/", OperatingSystem::Linux, &linux), "/Home/u/x");
    assert_eq!(labels::tildify("/Home/u/x", "/home/u", OperatingSystem::Macintosh, &linux), "~/x");
    assert_eq!(labels::tildify("C:\\u\\x", "C:\\u", OperatingSystem::Windows, &windows), "C:\\u\\x");
    assert_eq!(labels::tildify("/home/user2/x", "/home/user", OperatingSystem::Linux, &linux), "/home/user2/x");
    assert_eq!(labels::normalize_drive_letter("c:\\a", true), "C:\\a");
    assert_eq!(labels::normalize_drive_letter("c:\\a", false), "c:\\a");
}

#[test]
fn mnemonic_menu_labels() {
    let (win, mac, linux) = (native("win32", &[]), native("darwin", &[]), native("linux", &[]));
    for (label, mac_label, other_label, swapped) in [
        ("&File", "&File", "&&File", "&&File"),
        ("&&File", "File", "File", "&File"),
        ("Save (&&S) & more", "Save  & more", "Save  && more", "Save (&S) && more"),
        ("a&&&b", "a&b", "a&&b", "a&&&b"),
        ("(&&é)x", "(é)x", "(é)x", "(&é)x"),
        ("x_(&&_)", "x_", "x_", "x_(&_)"),
    ] {
        assert_eq!(labels::mnemonic_menu_label(label, false, &mac), mac_label, "{label}");
        assert_eq!(labels::mnemonic_menu_label(label, true, &linux), other_label, "{label}");
        assert_eq!(labels::mnemonic_menu_label(label, false, &win), swapped, "{label}");
        assert_eq!(labels::mnemonic_menu_label(label, false, &linux), swapped, "{label}");
    }
    assert_eq!(labels::mnemonic_button_label_force_disabled("Do &&not (&&S)"), "Do not ");
    assert_eq!(labels::unmnemonic_label("a & b"), "a && b");
}

#[test]
fn split_recent_label() {
    let linux = native("linux", &[]);
    let split = |label| labels::split_recent_label(label, &linux);
    let recent = |name: &str, parent: &str| labels::RecentLabel { name: name.to_string(), parent_path: parent.to_string() };
    assert_eq!(split("/a/b/c"), recent("c", "/a/b"));
    assert_eq!(split("C:\\a\\b"), recent("b", "C:\\a"));
    assert_eq!(split("/a/b [SSH: host]"), recent("b [SSH: host]", "/a"));
    assert_eq!(split("/"), recent("/", ""));
}
