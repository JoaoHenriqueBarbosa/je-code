//! A detecção do layout de teclado do sistema, sem `native-keymap`, sem C e sem `unsafe`.
//!
//! - Linux: as variáveis `XKB_DEFAULT_*` do ambiente e, na falta delas, o `/etc/default/keyboard` (`XKBMODEL`,
//!   `XKBLAYOUT`, `XKBVARIANT`, `XKBOPTIONS`), a mesma fonte que o `setxkbmap` e o `localectl` usam para o padrão do
//!   sistema. O `group` é sempre `0`.
//! - Windows: o primeiro item de `HKCU\Keyboard Layout\Preload` (o identificador KLID, como `00000409`), pelo `reg.exe`.
//! - macOS: o `AppleCurrentKeyboardLayoutInputSourceID` do `com.apple.HIToolbox`, pelo `defaults`.
//!
//! As funções `parse_*` e `build_*` são puras e testadas; [`detect_keyboard_layout`] só lê o ambiente.
//! Divergência: o `native-keymap` entrega o nome localizado do layout (`text` no Windows, `lang` e `localizedName`
//! no Mac); aqui o `text` do Windows é o próprio KLID e o `lang` do Mac fica vazio.

use std::collections::HashMap;

use super::{KeyboardLayoutInfo, KeyboardLayoutKind, LinuxKeyboardLayoutInfo, MacKeyboardLayoutInfo, WindowsKeyboardLayoutInfo};

/// As variáveis `XKBMODEL=pc105` do `/etc/default/keyboard` (aspas opcionais; comentários e linhas vazias ignorados).
pub fn parse_etc_default_keyboard(text: &str) -> HashMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_owned(), value.trim().trim_matches(['"', '\'']).to_owned()))
        })
        .collect()
}

/// O layout do Linux a partir de um mapa de variáveis (`XKBMODEL`, `XKBLAYOUT`, `XKBVARIANT`, `XKBOPTIONS`).
/// `XKBLAYOUT` pode listar vários (`us,br`): o primeiro é o ativo, como o grupo `0`.
pub fn build_linux_layout(variables: &HashMap<String, String>) -> Option<KeyboardLayoutInfo> {
    let get = |key: &str| variables.get(key).cloned().unwrap_or_default();
    let layout = get("XKBLAYOUT").split(',').next().unwrap_or_default().trim().to_owned();
    if layout.is_empty() {
        return None;
    }
    let variant = get("XKBVARIANT").split(',').next().unwrap_or_default().trim().to_owned();
    Some(KeyboardLayoutInfo {
        kind: KeyboardLayoutKind::Linux(LinuxKeyboardLayoutInfo {
            model: get("XKBMODEL"),
            group: 0,
            layout: layout.clone(),
            variant: variant.clone(),
            options: get("XKBOPTIONS"),
            rules: "evdev".to_owned(),
        }),
        is_user_keyboard_layout: None,
        is_us_standard: (layout == "us" && variant.is_empty()).then_some(true),
    })
}

/// O KLID da saída de `reg query "HKCU\Keyboard Layout\Preload" /v 1` (`    1    REG_SZ    00000409`).
pub fn parse_windows_preload(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let (_name, kind, value) = (parts.next()?, parts.next()?, parts.next()?);
        (kind == "REG_SZ" && value.len() == 8 && value.chars().all(|c| c.is_ascii_hexdigit())).then(|| value.to_ascii_lowercase())
    })
}

/// O layout do Windows a partir do KLID.
pub fn build_windows_layout(klid: &str) -> KeyboardLayoutInfo {
    KeyboardLayoutInfo {
        kind: KeyboardLayoutKind::Windows(WindowsKeyboardLayoutInfo { name: klid.to_owned(), id: String::new(), text: klid.to_owned() }),
        is_user_keyboard_layout: None,
        is_us_standard: (klid == "00000409").then_some(true),
    }
}

/// O layout do Mac a partir do `AppleCurrentKeyboardLayoutInputSourceID` (`com.apple.keylayout.US`).
pub fn build_mac_layout(input_source_id: &str) -> Option<KeyboardLayoutInfo> {
    let id = input_source_id.trim();
    if id.is_empty() {
        return None;
    }
    Some(KeyboardLayoutInfo {
        kind: KeyboardLayoutKind::Mac(MacKeyboardLayoutInfo { id: id.to_owned(), lang: String::new(), localized_name: None }),
        is_user_keyboard_layout: None,
        is_us_standard: (id == "com.apple.keylayout.US").then_some(true),
    })
}

/// O layout do sistema onde o processo roda; `None` se não foi possível lê-lo.
#[cfg(not(target_arch = "wasm32"))]
pub fn detect_keyboard_layout() -> Option<KeyboardLayoutInfo> {
    use std::process::Command;
    let command_output = |program: &str, args: &[&str]| {
        let output = Command::new(program).args(args).output().ok().filter(|o| o.status.success())?;
        String::from_utf8(output.stdout).ok()
    };
    if cfg!(target_os = "linux") {
        let mut variables: HashMap<String, String> = std::fs::read_to_string("/etc/default/keyboard").map(|t| parse_etc_default_keyboard(&t)).unwrap_or_default();
        for (env, key) in [("XKB_DEFAULT_MODEL", "XKBMODEL"), ("XKB_DEFAULT_LAYOUT", "XKBLAYOUT"), ("XKB_DEFAULT_VARIANT", "XKBVARIANT"), ("XKB_DEFAULT_OPTIONS", "XKBOPTIONS")] {
            if let Ok(value) = std::env::var(env) {
                variables.insert(key.to_owned(), value);
            }
        }
        build_linux_layout(&variables)
    } else if cfg!(windows) {
        let output = command_output("reg", &["query", r"HKCU\Keyboard Layout\Preload", "/v", "1"])?;
        parse_windows_preload(&output).map(|klid| build_windows_layout(&klid))
    } else if cfg!(target_os = "macos") {
        let output = command_output("defaults", &["read", "com.apple.HIToolbox", "AppleCurrentKeyboardLayoutInputSourceID"])?;
        build_mac_layout(&output)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyboard_layout::{are_keyboard_layouts_equal, get_keyboard_layout_id, parse_keyboard_layout_description};

    #[test]
    fn parses_etc_default_keyboard() {
        let text = "# KEYBOARD CONFIGURATION FILE\nXKBMODEL=\"pc105\"\nXKBLAYOUT=\"br,us\"\nXKBVARIANT=\"\"\nXKBOPTIONS=\"\"\n";
        let layout = build_linux_layout(&parse_etc_default_keyboard(text)).unwrap();
        let KeyboardLayoutKind::Linux(linux) = &layout.kind else { panic!("devia ser Linux") };
        assert_eq!((linux.model.as_str(), linux.layout.as_str(), linux.group), ("pc105", "br", 0));
        assert_eq!(layout.is_us_standard, None);
        assert_eq!(get_keyboard_layout_id(&layout), "br");
        assert_eq!(parse_keyboard_layout_description(Some(&layout)).label, "br");
    }

    #[test]
    fn us_layout_is_us_standard_and_empty_layout_is_none() {
        let us = build_linux_layout(&parse_etc_default_keyboard("XKBLAYOUT=us\nXKBMODEL=pc105")).unwrap();
        assert_eq!(us.is_us_standard, Some(true));
        assert!(build_linux_layout(&HashMap::new()).is_none());
    }

    #[test]
    fn parses_windows_preload_and_mac_input_source() {
        let klid = parse_windows_preload("\nHKEY_CURRENT_USER\\Keyboard Layout\\Preload\n    1    REG_SZ    00000416\n").unwrap();
        assert_eq!(klid, "00000416");
        let windows = build_windows_layout(&klid);
        assert_eq!(get_keyboard_layout_id(&windows), "00000416");
        let mac = build_mac_layout("com.apple.keylayout.ABC-QWERTZ\n").unwrap();
        assert_eq!(parse_keyboard_layout_description(Some(&mac)).label, "ABC QWERTZ");
        assert!(are_keyboard_layouts_equal(Some(&mac), Some(&mac.clone())));
        assert!(!are_keyboard_layouts_equal(Some(&mac), Some(&windows)));
    }
}
