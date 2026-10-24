//! Porte parcial de `src/vs/workbench/services/extensions/common/extensions.ts`: o contrato `IExtensionService` reduzido
//! ao que os serviços do workbench portados leem dele.
//!
//! Divergências:
//! - **Contrato reduzido** ao que os serviços portados leem: `getExtension(id)` (o `configurationResolverService`, para
//!   `${extensionInstallFolder:id}`), `onDidRegisterExtensions` e `whenInstalledExtensionsRegistered` (o
//!   `EditorResolverService` e o `TextFileEditorModel`). O restante (o ciclo de vida do host de extensões,
//!   `activateById`...) entra com o porte do `ExtensionService`.
//! - **`IExtensionDescription`** devolve só o `extensionLocation`, que é o campo que esses serviços leem; o descritor
//!   inteiro (manifesto, identificador, `isBuiltin`...) é o `ExtensionDescription` do `je-extensions`, que guarda o local
//!   como texto e exige o manifesto.
//! - **`extensions`** (o `readonly extensions: IExtensionDescription[]` do original, lido pelo `KeybindingsEditorModel`)
//!   devolve o `ExtensionDescription` do `je-extensions`, o descritor inteiro, e não o `IExtensionDescription` reduzido
//!   acima. Quem implementa [`IExtensionService`] precisa implementá-lo (o dublê `TestExtensionService` devolve a lista
//!   vazia, como o `extensions = []` do original).

use je_base::event::Event;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::scanner::ExtensionDescription;
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

    /// `onDidRegisterExtensions`: fired when extensions have been registered in the extension host.
    fn on_did_register_extensions(&self) -> Event<()>;

    /// `whenInstalledExtensionsRegistered()`: a promise that resolves when all installed extensions are registered.
    fn when_installed_extensions_registered(&self) -> Promise<bool>;

    /// `extensions`: the descriptions of the extensions registered so far.
    fn extensions(&self) -> Vec<ExtensionDescription>;
}

service_id!(pub I_EXTENSION_SERVICE: dyn IExtensionService = "extensionService");
