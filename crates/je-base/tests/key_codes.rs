//! Porte de `src/vs/base/test/common/keyCodes.test.ts` (suíte `keyCodes`), caso a caso e na mesma ordem.
//!
//! O `ensureNoDisposablesAreLeakedInTestSuite()` do original não tem o que conferir aqui: nenhum caso
//! cria descartável. O `OperatingSystem` do teste "WINDOWS & LINUX" é o laço `forEach` do original.

use je_base::key_codes::{
    event_key_code, immutable_code_to_key_code, immutable_key_code_to_code, key_chord, native_windows_key_code_to_key_code, KeyCode, KeyCodeUtils, KeyMod, ScanCode,
    ScanCodeUtils,
};
use je_base::keybindings::{decode_keybinding, Chord, KeyCodeChord, Keybinding};
use je_base::platform::OperatingSystem;

fn test_binary_encoding(expected: Option<Keybinding>, k: u32, os: OperatingSystem) {
    assert_eq!(decode_keybinding(k, os).unwrap(), expected);
}

fn chord(ctrl: bool, shift: bool, alt: bool, meta: bool, key: KeyCode) -> KeyCodeChord {
    KeyCodeChord::new(ctrl, shift, alt, meta, key)
}

fn keybinding(chords: &[KeyCodeChord]) -> Keybinding {
    Keybinding::new(chords.iter().copied().map(Chord::KeyCode).collect()).unwrap()
}

#[test]
fn mapping_for_minus() {
    // [147, 83, 0, ScanCode.Minus, 'Minus', KeyCode.US_MINUS, '-', 189, 'VK_OEM_MINUS', '-', 'OEM_MINUS'],
    assert_eq!(event_key_code(189), Some(KeyCode::Minus));
    assert_eq!(native_windows_key_code_to_key_code("VK_OEM_MINUS"), Some(KeyCode::Minus));
    assert_eq!(ScanCodeUtils::lower_case_to_enum("minus"), ScanCode::Minus);
    assert_eq!(ScanCodeUtils::to_enum("Minus"), ScanCode::Minus);
    assert_eq!(ScanCodeUtils::to_string(ScanCode::Minus), "Minus");
    assert_eq!(immutable_code_to_key_code(ScanCode::Minus), Some(KeyCode::DependsOnKbLayout));
    assert_eq!(immutable_key_code_to_code(KeyCode::Minus), Some(ScanCode::DependsOnKbLayout));
    assert_eq!(KeyCodeUtils::to_string(KeyCode::Minus), Some("-"));
    assert_eq!(KeyCodeUtils::from_string("-"), KeyCode::Minus);
    assert_eq!(KeyCodeUtils::to_user_settings_us(KeyCode::Minus), Some("-"));
    assert_eq!(KeyCodeUtils::to_user_settings_general(KeyCode::Minus), Some("OEM_MINUS"));
    assert_eq!(KeyCodeUtils::from_user_settings("-"), KeyCode::Minus);
    assert_eq!(KeyCodeUtils::from_user_settings("OEM_MINUS"), KeyCode::Minus);
    assert_eq!(KeyCodeUtils::from_user_settings("oem_minus"), KeyCode::Minus);
}

#[test]
fn mapping_for_space() {
    // [21, 10, 1, ScanCode.Space, 'Space', KeyCode.Space, 'Space', 32, 'VK_SPACE', empty, empty],
    assert_eq!(event_key_code(32), Some(KeyCode::Space));
    assert_eq!(native_windows_key_code_to_key_code("VK_SPACE"), Some(KeyCode::Space));
    assert_eq!(ScanCodeUtils::lower_case_to_enum("space"), ScanCode::Space);
    assert_eq!(ScanCodeUtils::to_enum("Space"), ScanCode::Space);
    assert_eq!(ScanCodeUtils::to_string(ScanCode::Space), "Space");
    assert_eq!(immutable_code_to_key_code(ScanCode::Space), Some(KeyCode::Space));
    assert_eq!(immutable_key_code_to_code(KeyCode::Space), Some(ScanCode::Space));
    assert_eq!(KeyCodeUtils::to_string(KeyCode::Space), Some("Space"));
    assert_eq!(KeyCodeUtils::from_string("Space"), KeyCode::Space);
    assert_eq!(KeyCodeUtils::to_user_settings_us(KeyCode::Space), Some("Space"));
    assert_eq!(KeyCodeUtils::to_user_settings_general(KeyCode::Space), Some("Space"));
    assert_eq!(KeyCodeUtils::from_user_settings("Space"), KeyCode::Space);
    assert_eq!(KeyCodeUtils::from_user_settings("space"), KeyCode::Space);
}

#[test]
fn mac_binary_encoding() {
    let test = |expected: Option<Keybinding>, k: u32| test_binary_encoding(expected, k, OperatingSystem::Macintosh);
    let enter = |ctrl, shift, alt, meta| Some(chord(ctrl, shift, alt, meta, KeyCode::Enter).to_keybinding());
    let e = KeyCode::Enter.bits();

    test(None, 0);
    test(enter(false, false, false, false), e);
    test(enter(true, false, false, false), KeyMod::WIN_CTRL | e);
    test(enter(false, false, true, false), KeyMod::ALT | e);
    test(enter(true, false, true, false), KeyMod::ALT | KeyMod::WIN_CTRL | e);
    test(enter(false, true, false, false), KeyMod::SHIFT | e);
    test(enter(true, true, false, false), KeyMod::SHIFT | KeyMod::WIN_CTRL | e);
    test(enter(false, true, true, false), KeyMod::SHIFT | KeyMod::ALT | e);
    test(enter(true, true, true, false), KeyMod::SHIFT | KeyMod::ALT | KeyMod::WIN_CTRL | e);
    test(enter(false, false, false, true), KeyMod::CTRL_CMD | e);
    test(enter(true, false, false, true), KeyMod::CTRL_CMD | KeyMod::WIN_CTRL | e);
    test(enter(false, false, true, true), KeyMod::CTRL_CMD | KeyMod::ALT | e);
    test(enter(true, false, true, true), KeyMod::CTRL_CMD | KeyMod::ALT | KeyMod::WIN_CTRL | e);
    test(enter(false, true, false, true), KeyMod::CTRL_CMD | KeyMod::SHIFT | e);
    test(enter(true, true, false, true), KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyMod::WIN_CTRL | e);
    test(enter(false, true, true, true), KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyMod::ALT | e);
    test(enter(true, true, true, true), KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyMod::ALT | KeyMod::WIN_CTRL | e);

    test(
        Some(keybinding(&[chord(false, false, false, false, KeyCode::Enter), chord(false, false, false, false, KeyCode::Tab)])),
        key_chord(KeyCode::Enter.bits(), KeyCode::Tab.bits()),
    );
    test(
        Some(keybinding(&[chord(false, false, false, true, KeyCode::KeyY), chord(false, false, false, false, KeyCode::KeyZ)])),
        key_chord(KeyMod::CTRL_CMD | KeyCode::KeyY.bits(), KeyCode::KeyZ.bits()),
    );
}

#[test]
fn windows_and_linux_binary_encoding() {
    for os in [OperatingSystem::Linux, OperatingSystem::Windows] {
        let test = |expected: Option<Keybinding>, k: u32| test_binary_encoding(expected, k, os);
        let enter = |ctrl, shift, alt, meta| Some(chord(ctrl, shift, alt, meta, KeyCode::Enter).to_keybinding());
        let e = KeyCode::Enter.bits();

        test(None, 0);
        test(enter(false, false, false, false), e);
        test(enter(false, false, false, true), KeyMod::WIN_CTRL | e);
        test(enter(false, false, true, false), KeyMod::ALT | e);
        test(enter(false, false, true, true), KeyMod::ALT | KeyMod::WIN_CTRL | e);
        test(enter(false, true, false, false), KeyMod::SHIFT | e);
        test(enter(false, true, false, true), KeyMod::SHIFT | KeyMod::WIN_CTRL | e);
        test(enter(false, true, true, false), KeyMod::SHIFT | KeyMod::ALT | e);
        test(enter(false, true, true, true), KeyMod::SHIFT | KeyMod::ALT | KeyMod::WIN_CTRL | e);
        test(enter(true, false, false, false), KeyMod::CTRL_CMD | e);
        test(enter(true, false, false, true), KeyMod::CTRL_CMD | KeyMod::WIN_CTRL | e);
        test(enter(true, false, true, false), KeyMod::CTRL_CMD | KeyMod::ALT | e);
        test(enter(true, false, true, true), KeyMod::CTRL_CMD | KeyMod::ALT | KeyMod::WIN_CTRL | e);
        test(enter(true, true, false, false), KeyMod::CTRL_CMD | KeyMod::SHIFT | e);
        test(enter(true, true, false, true), KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyMod::WIN_CTRL | e);
        test(enter(true, true, true, false), KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyMod::ALT | e);
        test(enter(true, true, true, true), KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyMod::ALT | KeyMod::WIN_CTRL | e);

        test(
            Some(keybinding(&[chord(false, false, false, false, KeyCode::Enter), chord(false, false, false, false, KeyCode::Tab)])),
            key_chord(KeyCode::Enter.bits(), KeyCode::Tab.bits()),
        );
        test(
            Some(keybinding(&[chord(true, false, false, false, KeyCode::KeyY), chord(false, false, false, false, KeyCode::KeyZ)])),
            key_chord(KeyMod::CTRL_CMD | KeyCode::KeyY.bits(), KeyCode::KeyZ.bits()),
        );
    }
}
