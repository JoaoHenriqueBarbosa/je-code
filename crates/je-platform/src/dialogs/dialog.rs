//! Porte de `src/vs/platform/dialogs/electron-browser/dialog.ts`: o texto do diálogo "Sobre" (`createNativeAboutDialogDetails`).
//!
//! Decisões de tradução:
//! - **`process.versions`** (do `sandbox/electron-browser/globals.ts`) é [`ProcessVersions`], que a interface nativa preenche com
//!   o que ela conhece; a versão que não existe vira o texto `undefined`, como a chave ausente do objeto vira no `localize`.
//! - **`isLinuxSnap`** vem do [`Platform`] que a função recebe (o `host`).
//! - **`new Date(productService.date)`** é o `Timestamp` do `jiff`, que lê a data ISO 8601 do `product.json`; uma data que não
//!   se lê vira `NaN`, como no `Date` do JavaScript.
//! - **O `fromNow`** é [`je_base::date::from_now`].

use jiff::Timestamp;
use je_base::date::from_now;
use je_base::nls::localize;
use je_base::platform::Platform;

use crate::native::OsProperties;
use crate::product::IProductService;

/// `process.versions`, só com as chaves que o diálogo "Sobre" lê.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessVersions {
    pub electron: Option<String>,
    pub microsoft_build: Option<String>,
    pub chrome: Option<String>,
    pub node: Option<String>,
    pub v8: Option<String>,
}

/// O resultado de [`create_native_about_dialog_details`]: `{ title, details, detailsToCopy }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeAboutDialogDetails {
    pub title: String,
    pub details: String,
    pub details_to_copy: String,
}

/// Tira o marcador de pré-lançamento (`-canary` ou `-unstable`) logo depois da versão `N.N.N`, quando ele termina em `.`,
/// `+`, `-` ou no fim do texto: `/^(?<core>\d+\.\d+\.\d+)-(?:canary|unstable)(?=[.+-]|$)/` trocado por `core`.
fn strip_prerelease_marker(version: &str) -> String {
    let bytes = version.as_bytes();
    let mut index = 0;
    for part in 0..3 {
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == start {
            return version.to_owned();
        }
        if part < 2 {
            if bytes.get(index) != Some(&b'.') {
                return version.to_owned();
            }
            index += 1;
        }
    }

    let (core, rest) = version.split_at(index);
    let Some(after_dash) = rest.strip_prefix('-') else {
        return version.to_owned();
    };
    for marker in ["canary", "unstable"] {
        if let Some(tail) = after_dash.strip_prefix(marker)
            && (tail.is_empty() || tail.starts_with(['.', '+', '-']))
        {
            return format!("{core}{tail}");
        }
    }

    version.to_owned()
}

/// `formatCopilotVersion`.
fn format_copilot_version(version: Option<&str>) -> String {
    let Some(version) = version else {
        return "Unknown".to_owned();
    };

    let stripped = strip_prerelease_marker(version);
    let formatted = stripped.strip_suffix(".unsigned").unwrap_or(&stripped);
    if formatted.is_empty() { "Unknown".to_owned() } else { formatted.to_owned() }
}

/// `new Date(date).getTime()`.
fn date_to_time_value(date: &str) -> f64 {
    date.parse::<Timestamp>().map_or(f64::NAN, |timestamp| timestamp.as_millisecond() as f64)
}

pub fn create_native_about_dialog_details(product_service: &dyn IProductService, os_props: &OsProperties, process_versions: &ProcessVersions, host: &Platform) -> NativeAboutDialogDetails {
    let mut version = product_service.version().to_owned();
    if let Some(target) = product_service.target().filter(|target| !target.is_empty()) {
        version = format!("{version} ({target} setup)");
    } else if product_service.darwin_universal_asset_id().is_some_and(|asset_id| !asset_id.is_empty()) {
        version = format!("{version} (Universal)");
    }

    let copilot_versions = product_service.copilot_versions();
    let copilot_runtime_version = format_copilot_version(copilot_versions.map(|versions| versions.runtime.as_str()));
    let copilot_sdk_version = format_copilot_version(copilot_versions.map(|versions| versions.sdk.as_str()));

    let version_of = |value: &Option<String>| value.clone().unwrap_or_else(|| "undefined".to_owned());

    let get_details = |use_ago: bool| -> String {
        let date = match product_service.date().filter(|date| !date.is_empty()) {
            Some(date) if use_ago => format!("{date} ({})", from_now(Some(date_to_time_value(date)), true, false, false)),
            Some(date) => date.to_owned(),
            None => "Unknown".to_owned(),
        };
        let os = format!("{} {} {}{}", os_props.os_type, os_props.arch, os_props.release, if host.is_linux_snap { " snap" } else { "" });

        localize(
            "aboutDetail",
            "Version: {0}\nCommit: {1}\nDate: {2}\nElectron: {3}\nElectronBuildId: {4}\nChromium: {5}\nNode.js: {6}\nV8: {7}\n@github/copilot: {8}\n@github/copilot-sdk: {9}\nOS: {10}",
            &[
                &version,
                product_service.commit().filter(|commit| !commit.is_empty()).unwrap_or("Unknown"),
                &date,
                &version_of(&process_versions.electron),
                &version_of(&process_versions.microsoft_build),
                &version_of(&process_versions.chrome),
                &version_of(&process_versions.node),
                &version_of(&process_versions.v8),
                &copilot_runtime_version,
                &copilot_sdk_version,
                &os,
            ],
        )
    };

    let details = get_details(true);
    let details_to_copy = get_details(false);

    NativeAboutDialogDetails { title: product_service.name_long().to_owned(), details, details_to_copy }
}
