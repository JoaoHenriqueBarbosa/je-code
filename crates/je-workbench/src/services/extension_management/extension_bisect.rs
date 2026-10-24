//! Recorte de `src/vs/workbench/services/extensionManagement/browser/extensionBisect.ts`: o contrato
//! `IExtensionBisectService`, só com o `isDisabledByBisect` que o `ExtensionEnablementService` consulta. O serviço
//! em si (a busca binária pela extensão que causa um problema, com os comandos e as notificações) entra com a
//! contribuição de bisect.

use je_instantiation::service_id;

use super::extension_management::IExtension;

/// `IExtensionBisectService`, reduzido a `isDisabledByBisect`.
pub trait IExtensionBisectService {
    /// Se a extensão está desabilitada pela busca binária em andamento.
    fn is_disabled_by_bisect(&self, extension: &IExtension) -> bool;
}

service_id!(pub I_EXTENSION_BISECT_SERVICE: dyn IExtensionBisectService = "IExtensionBisectService");
