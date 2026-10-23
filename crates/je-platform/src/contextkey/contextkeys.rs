//! Porte de `src/vs/platform/contextkey/common/contextkeys.ts`: as chaves de contexto de plataforma.
//!
//! Divergência: no original as constantes são registradas ao carregar o módulo (e entram em
//! `RawContextKey.all()`); aqui cada chave é criada (e registrada) na primeira chamada da sua função.

use je_base::platform::Platform;

use super::contextkey::{ContextKeyMeta, RawContextKey};

thread_local! {
    static PLATFORM: Platform = Platform::detect();
}

macro_rules! raw_keys {
    ($($(#[$doc:meta])* $name:ident: $key:literal, $default:expr, $meta:expr;)*) => {$(
        $(#[$doc])*
        pub fn $name() -> RawContextKey {
            thread_local! { static KEY: RawContextKey = RawContextKey::new($key, $default, $meta); }
            KEY.with(Clone::clone)
        }
    )*};
}

fn platform<T>(f: impl FnOnce(&Platform) -> T) -> T {
    PLATFORM.with(f)
}

raw_keys! {
    is_mac_context: "isMac", platform(|p| p.is_macintosh), "Whether the operating system is macOS";
    is_linux_context: "isLinux", platform(|p| p.is_linux), "Whether the operating system is Linux";
    is_windows_context: "isWindows", platform(|p| p.is_windows), "Whether the operating system is Windows";
    is_chrome_os_context: "isChromeOS", platform(|p| p.is_chrome_os), "Whether the operating system is ChromeOS";
    is_web_context: "isWeb", platform(|p| p.is_web), "Whether the platform is a web browser";
    is_mac_native_context: "isMacNative", platform(|p| p.is_macintosh && !p.is_web), "Whether the operating system is macOS on a non-browser platform";
    is_ios_context: "isIOS", platform(|p| p.is_ios), "Whether the operating system is iOS";
    is_mobile_context: "isMobile", platform(|p| p.is_mobile), "Whether the platform is a mobile web browser";
    is_development_context: "isDevelopment", false, ContextKeyMeta::Hide;
    product_quality_context: "productQualityType", "", "Quality type of VS Code";
    input_focused_context: "inputFocus", false, "Whether keyboard focus is inside an input box";
}

/// `InputFocusedContextKey`.
pub const INPUT_FOCUSED_CONTEXT_KEY: &str = "inputFocus";
