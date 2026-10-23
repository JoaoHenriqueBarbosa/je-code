//! Porte de `src/vs/base/common/desktopEnvironmentInfo.ts`: o ambiente de desktop do Linux a partir das
//! variáveis de ambiente (`XDG_CURRENT_DESKTOP`, `DESKTOP_SESSION`...).

use crate::js_string::{self, JsString};
use crate::process::ProcessEnvironment;

/// `DesktopEnvironment`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopEnvironment {
    Unknown,
    Cinnamon,
    Deepin,
    Gnome,
    Kde3,
    Kde4,
    Kde5,
    Kde6,
    Pantheon,
    Unity,
    Xfce,
    Ukui,
    Lxqt,
}

impl DesktopEnvironment {
    /// O valor de texto do `enum` do original (`'GNOME'`, `'KDE5'`...).
    pub fn name(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::Cinnamon => "CINNAMON",
            Self::Deepin => "DEEPIN",
            Self::Gnome => "GNOME",
            Self::Kde3 => "KDE3",
            Self::Kde4 => "KDE4",
            Self::Kde5 => "KDE5",
            Self::Kde6 => "KDE6",
            Self::Pantheon => "PANTHEON",
            Self::Unity => "UNITY",
            Self::Xfce => "XFCE",
            Self::Ukui => "UKUI",
            Self::Lxqt => "LXQT",
        }
    }
}

const XDG_CURRENT_DESKTOP: &str = "XDG_CURRENT_DESKTOP";
const KDE_SESSION: &str = "KDE_SESSION_VERSION";

/// `if (env[name])`: presente e não vazio.
fn truthy<'a>(env: &'a ProcessEnvironment, name: &str) -> Option<&'a str> {
    env.get(name).map(String::as_str).filter(|v| !v.is_empty())
}

/// `getDesktopEnvironment()`, sobre o `env` dado.
pub fn get_desktop_environment(env: &ProcessEnvironment) -> DesktopEnvironment {
    if let Some(xdg_current_desktop) = truthy(env, XDG_CURRENT_DESKTOP) {
        let units: Vec<u16> = xdg_current_desktop.encode_utf16().collect();
        let values = units
            .split(|&u| u == u16::from(b':'))
            .map(|part| JsString::from_units(js_string::trim(part).to_vec()).to_string_lossy())
            .filter(|value| !value.is_empty());
        for value in values {
            match value.as_str() {
                "Unity" => {
                    if truthy(env, "DESKTOP_SESSION").is_some_and(|s| s.contains("gnome-fallback")) {
                        return DesktopEnvironment::Gnome;
                    }
                    return DesktopEnvironment::Unity;
                }
                "Deepin" => return DesktopEnvironment::Deepin,
                "GNOME" => return DesktopEnvironment::Gnome,
                "X-Cinnamon" => return DesktopEnvironment::Cinnamon,
                "KDE" => {
                    return match env.get(KDE_SESSION).map(String::as_str) {
                        Some("5") => DesktopEnvironment::Kde5,
                        Some("6") => DesktopEnvironment::Kde6,
                        _ => DesktopEnvironment::Kde4,
                    };
                }
                "Pantheon" => return DesktopEnvironment::Pantheon,
                "XFCE" => return DesktopEnvironment::Xfce,
                "UKUI" => return DesktopEnvironment::Ukui,
                "LXQt" => return DesktopEnvironment::Lxqt,
                _ => {}
            }
        }
    }

    if let Some(desktop_session) = truthy(env, "DESKTOP_SESSION") {
        match desktop_session {
            "deepin" => return DesktopEnvironment::Deepin,
            "gnome" | "mate" => return DesktopEnvironment::Gnome,
            "kde4" | "kde-plasma" => return DesktopEnvironment::Kde4,
            "kde" => {
                return if env.contains_key(KDE_SESSION) { DesktopEnvironment::Kde4 } else { DesktopEnvironment::Kde3 };
            }
            "xfce" | "xubuntu" => return DesktopEnvironment::Xfce,
            "ukui" => return DesktopEnvironment::Ukui,
            _ => {}
        }
    }

    if env.contains_key("GNOME_DESKTOP_SESSION_ID") {
        return DesktopEnvironment::Gnome;
    }
    if env.contains_key("KDE_FULL_SESSION") {
        return if env.contains_key(KDE_SESSION) { DesktopEnvironment::Kde4 } else { DesktopEnvironment::Kde3 };
    }

    DesktopEnvironment::Unknown
}
