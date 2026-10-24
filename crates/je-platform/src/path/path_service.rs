//! Porte de `src/vs/platform/path/common/pathService.ts`: o contrato `IPathService` (as propriedades de caminho que
//! valem para o ambiente alvo) e o `IResourcePathProvider`.
//!
//! Notas de tradução:
//! - **`IPath`** (o módulo `win32` ou `posix`) é o [`PathFlavor`], o sabor de caminho que o `PathModule` do
//!   `je-base` recebe para montar o módulo com o ambiente de quem chama.
//! - **Sobrecargas.** `userHome(options?)` com `preferLocal: true` devolve o `URI` na hora, e sem ele devolve uma
//!   promessa: são [`IPathService::local_user_home`] e [`IPathService::user_home`]. `hasValidBasename` tem a versão
//!   assíncrona (que resolve o sistema operacional) e a síncrona (que o recebe): [`IPathService::has_valid_basename`]
//!   e [`IPathService::has_valid_basename_for_os`].
//! - **Promessas.** `Promise<T>` é a [`Promise`] do `je-base`.

use std::rc::Rc;

use je_base::lifecycle::DisposableRef;
use je_base::path::Platform;
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

/// `IPath`: o sabor de caminho (`win32` ou `posix`).
pub type PathFlavor = Platform;

/// `IResourcePathProvider`.
pub trait IResourcePathProvider {
    fn get_operating_system(&self, resource: &URI) -> Promise<Option<OperatingSystem>>;
}

/// `IPathService`: provides access to path related properties that match the target environment.
pub trait IPathService {
    /// The correct path library to use for the ambient target environment.
    fn path(&self) -> Promise<PathFlavor>;

    /// Determines the best default URI scheme for the current workspace.
    fn default_uri_scheme(&self) -> String;

    /// Converts the given path to a file URI for the ambient target environment.
    fn file_uri(&self, path: &str) -> Promise<URI>;

    /// Resolves the user-home directory for the ambient target environment.
    fn user_home(&self) -> Promise<URI>;

    /// `userHome({ preferLocal: true })`: o home do usuário local, sem esperar o ambiente remoto.
    fn local_user_home(&self) -> URI;

    /// Figures out if the provided resource has a valid file name for the operating system the file is saved to.
    fn has_valid_basename(&self, resource: &URI, basename: Option<&str>) -> Promise<bool>;

    /// A versão síncrona de `hasValidBasename`, para um sistema operacional já conhecido.
    fn has_valid_basename_for_os(&self, resource: &URI, os: OperatingSystem, basename: Option<&str>) -> bool;

    /// Resolves the operating system whose path semantics apply to a resource.
    fn get_operating_system(&self, resource: &URI) -> Promise<Option<OperatingSystem>>;

    /// Resolves the path implementation whose semantics apply to a resource.
    fn get_path(&self, resource: &URI) -> Promise<Option<PathFlavor>>;

    /// Registers path semantics for a URI scheme.
    fn register_path_provider(&self, scheme: &str, provider: Rc<dyn IResourcePathProvider>) -> DisposableRef;

    /// @deprecated use `userHome` instead.
    fn resolved_user_home(&self) -> Option<URI>;
}

service_id!(pub I_PATH_SERVICE: dyn IPathService = "pathService");
