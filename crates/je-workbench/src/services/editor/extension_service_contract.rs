//! Contrato reduzido do `IExtensionService` (`src/vs/workbench/services/extensions/common/extensions.ts`) que o
//! `EditorResolverService` lê: `onDidRegisterExtensions` e `whenInstalledExtensionsRegistered`.
//!
//! Divergências:
//! - O [`extensions`](crate::services::extensions::extensions) já existente é reduzido ao `getExtension` (o que o
//!   resolvedor de variáveis de configuração lê) e tem dublês de teste que o implementam, então estes dois membros não
//!   entram lá. O identificador [`I_EXTENSION_SERVICE`] tem o mesmo nome do original (`extensionService`) mas é de outro
//!   tipo de serviço. Quando o `ExtensionService` for portado, os dois contratos viram um só.
//! - **`whenInstalledExtensionsRegistered(): Promise<boolean>`** devolve `Promise<bool>`.

use je_base::event::Event;
use je_base::promise::Promise;
use je_instantiation::service_id;

/// `IExtensionService`, reduzido ao registro das extensões instaladas.
pub trait IExtensionService {
    /// `onDidRegisterExtensions`: fired when extensions have been registered in the extension host.
    fn on_did_register_extensions(&self) -> Event<()>;

    /// `whenInstalledExtensionsRegistered()`: a promise that resolves when all installed extensions are registered.
    fn when_installed_extensions_registered(&self) -> Promise<bool>;
}

service_id!(pub I_EXTENSION_SERVICE: dyn IExtensionService = "extensionService");
