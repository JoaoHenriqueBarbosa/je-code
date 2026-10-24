//! Porte parcial de `src/vs/workbench/services/extensions/browser/extensionService.ts`: o `BrowserExtensionHostKindPicker`,
//! que decide em qual host de extensão (web worker local ou remoto) cada extensão roda no navegador.
//!
//! Fora do porte: o `ExtensionService` (o serviço do navegador, que estende o `AbstractExtensionService`, ainda não
//! portado: o gerenciador de hosts de extensão, o `IExtensionHost` e o restante de `common/abstractExtensionService.ts`).
//!
//! Divergências:
//! - `ILogService` é o [`LogServiceRef`] (o `@ILogService` do construtor).
//! - `ExtensionIdentifier` é o valor do identificador (`extensionId.value`), como no `IExtensionHostKindPicker` do porte.
//! - `ExtensionHostKind | null` é `Option<ExtensionHostKind>`.

use je_extensions::manifest::ExtensionKind;
use je_platform::log::{LogServiceRef, Logger};

use crate::services::extensions::extension_host_kind::{
    ExtensionHostKind, ExtensionRunningPreference, IExtensionHostKindPicker, extension_host_kind_to_string, extension_running_preference_to_string,
};

/// `BrowserExtensionHostKindPicker`.
pub struct BrowserExtensionHostKindPicker {
    log_service: LogServiceRef,
}

impl BrowserExtensionHostKindPicker {
    pub fn new(log_service: LogServiceRef) -> Self {
        Self { log_service }
    }

    /// `static pickRunningLocation(extensionKinds, isInstalledLocally, isInstalledRemotely, preference)`.
    pub fn pick_running_location(
        extension_kinds: &[ExtensionKind],
        is_installed_locally: bool,
        is_installed_remotely: bool,
        preference: ExtensionRunningPreference,
    ) -> Option<ExtensionHostKind> {
        let mut result: Vec<ExtensionHostKind> = Vec::new();
        let mut can_run_remotely = false;
        for extension_kind in extension_kinds {
            if *extension_kind == ExtensionKind::Ui && is_installed_remotely {
                // ui extensions run remotely if possible (but only as a last resort)
                if preference == ExtensionRunningPreference::Remote {
                    return Some(ExtensionHostKind::Remote);
                } else {
                    can_run_remotely = true;
                }
            }
            if *extension_kind == ExtensionKind::Workspace && is_installed_remotely {
                // workspace extensions run remotely if possible
                if preference == ExtensionRunningPreference::None || preference == ExtensionRunningPreference::Remote {
                    return Some(ExtensionHostKind::Remote);
                } else {
                    result.push(ExtensionHostKind::Remote);
                }
            }
            if *extension_kind == ExtensionKind::Web && (is_installed_locally || is_installed_remotely) {
                // web worker extensions run in the local web worker if possible
                if preference == ExtensionRunningPreference::None || preference == ExtensionRunningPreference::Local {
                    return Some(ExtensionHostKind::LocalWebWorker);
                } else {
                    result.push(ExtensionHostKind::LocalWebWorker);
                }
            }
        }
        if can_run_remotely {
            result.push(ExtensionHostKind::Remote);
        }
        result.first().copied()
    }
}

/// `extensionKinds.join(', ')`.
fn join_kinds(extension_kinds: &[ExtensionKind]) -> String {
    extension_kinds
        .iter()
        .map(|kind| match kind {
            ExtensionKind::Ui => "ui",
            ExtensionKind::Workspace => "workspace",
            ExtensionKind::Web => "web",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl IExtensionHostKindPicker for BrowserExtensionHostKindPicker {
    fn pick_extension_host_kind(
        &self,
        extension_id: &str,
        extension_kinds: &[ExtensionKind],
        is_installed_locally: bool,
        is_installed_remotely: bool,
        preference: ExtensionRunningPreference,
    ) -> Option<ExtensionHostKind> {
        let result = Self::pick_running_location(extension_kinds, is_installed_locally, is_installed_remotely, preference);
        self.log_service.trace(&format!(
            "pickRunningLocation for {extension_id}, extension kinds: [{}], isInstalledLocally: {is_installed_locally}, isInstalledRemotely: {is_installed_remotely}, preference: {} => {}",
            join_kinds(extension_kinds),
            extension_running_preference_to_string(preference),
            extension_host_kind_to_string(result)
        ));
        result
    }
}
