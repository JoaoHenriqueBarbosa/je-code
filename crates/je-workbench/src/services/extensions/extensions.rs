//! Porte parcial de `src/vs/workbench/services/extensions/common/extensions.ts`: o contrato `IExtensionService` reduzido
//! ao que os serviços do workbench portados leem dele.
//!
//! Divergências:
//! - **`getExtension(id)`** é o único método lido (pelo `configurationResolverService`, para `${extensionInstallFolder:id}`).
//!   O restante do serviço (o ciclo de vida do host de extensões, os eventos, `activateById`...) entra com o porte do
//!   `ExtensionService`.
//! - **`IExtensionDescription`** devolve só o `extensionLocation`, que é o campo que esses serviços leem; o descritor
//!   inteiro (manifesto, identificador, `isBuiltin`...) é o `ExtensionDescription` do `je-extensions`, que guarda o local
//!   como texto e exige o manifesto.

use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

/// `IExtensionDescription`, reduzido ao `extensionLocation`.
#[derive(Clone, Debug)]
pub struct IExtensionDescription {
    pub extension_location: URI,
}

/// `IExtensionService`.
pub trait IExtensionService {
    /// Returns the description of the extension with the given id, or `None` when it is not installed.
    fn get_extension(&self, id: &str) -> Promise<Option<IExtensionDescription>>;

    /// `whenInstalledExtensionsRegistered()`: resolves once the installed extensions are registered. Sem o registro de
    /// extensões no porte, o padrão é já resolvido.
    fn when_installed_extensions_registered(&self) -> Promise<bool> {
        Promise::resolved(true)
    }
}

service_id!(pub I_EXTENSION_SERVICE: dyn IExtensionService = "extensionService");
