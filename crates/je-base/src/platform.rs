//! Porte de `src/vs/base/common/platform.ts`.
//!
//! O original decide tudo ao carregar o módulo, lendo `process`, `navigator` e `globalThis`, e exporta
//! constantes (`isWindows`, `isMacintosh`, `OS`, `language`, `userAgent`...) que o resto do `vs/base`
//! consulta dentro de funções que, fora isso, seriam puras. Aqui isso é dado: um [`Platform`] é
//! detectado uma vez na partida ([`Platform::detect`]) e passado a quem precisa, e quem só quer testar
//! um sistema monta o seu com [`Platform::from_native`] ou [`Platform::from_web`], sem tocar o ambiente.
//!
//! Relação com o resto do crate: o [`crate::path::Platform`] é só o sabor de caminho (`Posix` ou
//! `Win32`, o `process.platform === 'win32'` do `path`), e é isso que o `uri` e o `resources` recebem.
//! O [`OperatingSystem`] daqui é o `OperatingSystem` do `platform.ts` (Windows, macOS, Linux), e
//! [`OperatingSystem::path_flavor`] / [`Platform::path_flavor`] fazem a ponte entre os dois, sem renomear
//! nada do que já existe.
//!
//! Não portados, por serem do laço de eventos do JavaScript e sem sentido em Rust: `setTimeout0` e
//! `setTimeout0IsFaster`.

use crate::json::{self, Value};
use crate::path;

pub const LANGUAGE_DEFAULT: &str = "en";

/// `OperatingSystem`: o sistema para o qual os atalhos e os caminhos são formatados.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OperatingSystem {
    Windows = 1,
    Macintosh = 2,
    Linux = 3,
}

impl OperatingSystem {
    /// O sabor de caminho do sistema: `Win32` no Windows, `Posix` nos outros.
    pub fn path_flavor(self) -> path::Platform {
        if self == OperatingSystem::Windows { path::Platform::Win32 } else { path::Platform::Posix }
    }
}

/// O `enum Platform` do original (`Web`, `Mac`, `Linux`, `Windows`), que fica com outro nome aqui porque
/// [`Platform`] é o dado detectado.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlatformKind {
    Web,
    Mac,
    Linux,
    Windows,
}

impl PlatformKind {
    /// `PlatformToString`.
    pub fn name(self) -> &'static str {
        match self {
            PlatformKind::Web => "Web",
            PlatformKind::Mac => "Mac",
            PlatformKind::Linux => "Linux",
            PlatformKind::Windows => "Windows",
        }
    }
}

/// O que o ramo nativo do original lê de `process`: `platform` (`win32`, `darwin`, `linux`...) e as
/// variáveis de ambiente.
pub struct NativeEnvironment<'a> {
    /// `process.platform`.
    pub platform: &'a str,
    /// `process.env[name]`.
    pub env: &'a dyn Fn(&str) -> Option<String>,
    /// `process.versions.electron` é uma string.
    pub is_electron: bool,
}

/// O que o ramo web do original lê de `navigator` e de `globalThis`.
#[derive(Clone, Debug, Default)]
pub struct WebEnvironment {
    /// `navigator.userAgent`.
    pub user_agent: String,
    /// `navigator.maxTouchPoints`.
    pub max_touch_points: Option<u32>,
    /// `navigator.language`.
    pub language: String,
    /// `nls.getNLSLanguage()`.
    pub nls_language: Option<String>,
    /// `typeof importScripts === 'function'`.
    pub is_web_worker: bool,
    /// `globalThis.origin`, lido só dentro de um web worker.
    pub web_worker_origin: Option<String>,
}

/// As constantes do `platform.ts`, detectadas uma vez.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Platform {
    pub is_windows: bool,
    pub is_macintosh: bool,
    pub is_linux: bool,
    pub is_linux_snap: bool,
    pub is_chrome_os: bool,
    pub is_native: bool,
    pub is_electron: bool,
    pub is_web: bool,
    pub is_web_worker: bool,
    pub web_worker_origin: Option<String>,
    pub is_ios: bool,
    pub is_mobile: bool,
    /// Se roda em CI (GitHub Actions, Azure Pipelines).
    pub is_ci: bool,
    /// O sistema operacional (`OS`): macOS e iOS contam como `Macintosh`, o que não é Windows é `Linux`.
    pub os: OperatingSystem,
    /// `platform`: `Web` só quando nada do resto foi reconhecido.
    pub kind: PlatformKind,
    /// A língua da interface, em minúsculas (`zh-tw`, `de`).
    pub language: String,
    /// Desktop: o locale do SO ou o de `--locale`/`argv.json`. Web: igual a `platform_locale`.
    pub locale: Option<String>,
    /// O locale do SO ou do navegador, sempre, em minúsculas.
    pub platform_locale: Option<String>,
    /// As traduções disponíveis por language packs.
    pub translations_config_file: Option<String>,
    pub user_agent: Option<String>,
}

fn env_flag(env: &dyn Fn(&str) -> Option<String>, name: &str) -> bool {
    env(name).is_some_and(|v| !v.is_empty())
}

impl Platform {
    fn finish(mut self) -> Self {
        self.kind = if self.is_macintosh {
            PlatformKind::Mac
        } else if self.is_windows {
            PlatformKind::Windows
        } else if self.is_linux {
            PlatformKind::Linux
        } else {
            PlatformKind::Web
        };
        self.os = if self.is_macintosh || self.is_ios {
            OperatingSystem::Macintosh
        } else if self.is_windows {
            OperatingSystem::Windows
        } else {
            OperatingSystem::Linux
        };
        self
    }

    fn blank() -> Self {
        Self {
            is_windows: false,
            is_macintosh: false,
            is_linux: false,
            is_linux_snap: false,
            is_chrome_os: false,
            is_native: false,
            is_electron: false,
            is_web: false,
            is_web_worker: false,
            web_worker_origin: None,
            is_ios: false,
            is_mobile: false,
            is_ci: false,
            os: OperatingSystem::Linux,
            kind: PlatformKind::Web,
            language: LANGUAGE_DEFAULT.to_string(),
            locale: None,
            platform_locale: Some(LANGUAGE_DEFAULT.to_string()),
            translations_config_file: None,
            user_agent: None,
        }
    }

    /// O ramo nativo do original (processo com `process`): sistema por `process.platform`, CI e
    /// `VSCODE_NLS_CONFIG` pelas variáveis de ambiente.
    pub fn from_native(native: &NativeEnvironment<'_>) -> Self {
        let env = native.env;
        let mut p = Self::blank();
        p.is_windows = native.platform == "win32";
        p.is_macintosh = native.platform == "darwin";
        p.is_linux = native.platform == "linux";
        p.is_linux_snap = p.is_linux && env_flag(env, "SNAP") && env_flag(env, "SNAP_REVISION");
        p.is_electron = native.is_electron;
        p.is_ci = env_flag(env, "CI") || env_flag(env, "BUILD_ARTIFACTSTAGINGDIRECTORY") || env_flag(env, "GITHUB_WORKSPACE");
        p.locale = Some(LANGUAGE_DEFAULT.to_string());
        p.language = LANGUAGE_DEFAULT.to_string();
        if let Some(raw) = env("VSCODE_NLS_CONFIG").filter(|v| !v.is_empty()) {
            p.apply_nls_config(&raw);
        }
        p.is_native = true;
        p.finish()
    }

    /// Lê o `VSCODE_NLS_CONFIG` como o `try { JSON.parse(...) } catch {}` do original: texto que não é
    /// JSON, ou `null`, deixa tudo como estava.
    fn apply_nls_config(&mut self, raw: &str) {
        let mut errors = Vec::new();
        let Some(value) = json::parse_str(raw, &mut errors).filter(|_| errors.is_empty()) else { return };
        let (user_locale, os_locale, resolved_language, translations) = match &value {
            Value::Null => return,
            Value::Object(o) => {
                let text = |key: &str| match o.get(key) {
                    Some(Value::String(s)) => Some(s.to_string_lossy()),
                    _ => None,
                };
                let translations = match o.get("languagePack") {
                    Some(Value::Object(pack)) => match pack.get("translationsConfigFile") {
                        Some(Value::String(s)) => Some(s.to_string_lossy()),
                        _ => None,
                    },
                    _ => None,
                };
                (text("userLocale"), text("osLocale"), text("resolvedLanguage"), translations)
            }
            _ => (None, None, None, None),
        };
        self.locale = user_locale;
        self.platform_locale = os_locale;
        self.language = resolved_language.filter(|l| !l.is_empty()).unwrap_or_else(|| LANGUAGE_DEFAULT.to_string());
        self.translations_config_file = translations;
    }

    /// O ramo web do original (`navigator`): sistema pelo `userAgent`.
    pub fn from_web(web: &WebEnvironment) -> Self {
        let ua = web.user_agent.as_str();
        let mut p = Self::blank();
        p.user_agent = Some(web.user_agent.clone());
        p.is_windows = ua.contains("Windows");
        p.is_macintosh = ua.contains("Macintosh");
        p.is_ios = (ua.contains("Macintosh") || ua.contains("iPad") || ua.contains("iPhone")) && web.max_touch_points.is_some_and(|n| n > 0);
        p.is_linux = ua.contains("Linux");
        p.is_chrome_os = ua.contains("CrOS");
        p.is_mobile = ua.contains("Mobi");
        p.is_web = true;
        p.is_web_worker = web.is_web_worker;
        p.web_worker_origin = if web.is_web_worker { web.web_worker_origin.clone() } else { None };
        p.language = web.nls_language.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| LANGUAGE_DEFAULT.to_string());
        let locale = web.language.to_lowercase();
        p.locale = Some(locale.clone());
        p.platform_locale = Some(locale);
        p.finish()
    }

    /// A plataforma do processo atual: em Windows, Linux e macOS, o ramo nativo lendo o ambiente real;
    /// em `wasm32`, o ramo web, sem `navigator` (o `WebEnvironment` vazio: nenhum sistema reconhecido,
    /// `OS` igual a `Linux`). Um host web que leia o `navigator` por conta própria monta o dado com
    /// [`Platform::from_web`].
    pub fn detect() -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            Self::from_web(&WebEnvironment::default())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let platform = match std::env::consts::OS {
                "windows" => "win32",
                "macos" => "darwin",
                other => other,
            };
            Self::from_native(&NativeEnvironment { platform, env: &|name| std::env::var(name).ok(), is_electron: false })
        }
    }

    /// O sabor de caminho do sistema (o que o `uri` e o `resources` recebem).
    pub fn path_flavor(&self) -> path::Platform {
        self.os.path_flavor()
    }

    fn user_agent_contains(&self, needle: &str) -> bool {
        self.user_agent.as_deref().is_some_and(|ua| ua.contains(needle))
    }

    pub fn is_chrome(&self) -> bool {
        self.user_agent_contains("Chrome")
    }

    pub fn is_firefox(&self) -> bool {
        self.user_agent_contains("Firefox")
    }

    pub fn is_safari(&self) -> bool {
        !self.is_chrome() && self.user_agent_contains("Safari")
    }

    pub fn is_edge(&self) -> bool {
        self.user_agent_contains("Edg/")
    }

    pub fn is_android(&self) -> bool {
        self.user_agent_contains("Android")
    }

    pub fn has_electron_user_agent(&self) -> bool {
        self.user_agent_contains("Electron")
    }
}

/// `Language`: o que o original faz com a língua da interface.
pub mod language {
    /// `Language.isDefaultVariant`: `en` ou `en-*`.
    pub fn is_default_variant(language: &str) -> bool {
        let units: Vec<u16> = language.encode_utf16().collect();
        if units.len() == 2 {
            language == "en"
        } else if units.len() >= 3 {
            units[0] == u16::from(b'e') && units[1] == u16::from(b'n') && units[2] == u16::from(b'-')
        } else {
            false
        }
    }

    /// `Language.isDefault`.
    pub fn is_default(language: &str) -> bool {
        language == "en"
    }
}

/// `isLittleEndian`: o original mede com um `Uint16Array`, aqui o alvo da compilação diz.
pub fn is_little_endian() -> bool {
    cfg!(target_endian = "little")
}

/// `parseFloat(string)`: o maior prefixo decimal do texto (depois dos espaços iniciais), `NaN` se não houver.
fn parse_float(text: &str) -> f64 {
    let units: Vec<u16> = text.encode_utf16().collect();
    let trimmed = crate::js_string::trim_start(&units);
    let text = String::from_utf16_lossy(trimmed);
    let bytes = text.as_bytes();
    let mut i = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    if text[i..].starts_with("Infinity") {
        return if bytes.first() == Some(&b'-') { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    let digits = |from: usize| bytes[from..].iter().take_while(|b| b.is_ascii_digit()).count();
    let int_digits = digits(i);
    i += int_digits;
    let mut frac_digits = 0;
    if bytes.get(i) == Some(&b'.') {
        frac_digits = digits(i + 1);
        if int_digits > 0 || frac_digits > 0 {
            i += 1 + frac_digits;
        }
    }
    if int_digits == 0 && frac_digits == 0 {
        return f64::NAN;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(bytes.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let exp_digits = digits(j);
        if exp_digits > 0 {
            i = j + exp_digits;
        }
    }
    text[..i].parse().unwrap_or(f64::NAN)
}

/// `isTahoeOrNewer`: o `osVersion` do Darwin do macOS Tahoe (26) começa em 25.
pub fn is_tahoe_or_newer(os_version: &str) -> bool {
    parse_float(os_version) >= 25.0
}
