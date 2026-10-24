//! Porte de `src/vs/workbench/services/environment/common/environmentService.ts`: o `IWorkbenchEnvironmentService`,
//! o serviço de ambiente que só existe na camada do workbench.
//!
//! Notas de tradução:
//! - `IWorkbenchEnvironmentService extends IEnvironmentService` é o trait [`IWorkbenchEnvironmentService`] com o
//!   [`EnvironmentService`] como supertrait. O `refineServiceDecorator` reaproveita o identificador do
//!   `IEnvironmentService` (`environmentService`), e [`I_WORKBENCH_ENVIRONMENT_SERVICE`] também.
//! - `filesToOpenOrCreate`, `filesToDiff` e `filesToMerge` (`IPath[] | undefined`) são métodos com padrão `None`, e o
//!   [`IPath`] do porte só tem o `fileUri` (o resto de `platform/window/common/window.ts`, com o `ITextEditorOptions`,
//!   entra quando alguém o ler).
//! - Não entra, porque o tipo de que depende não foi portado: `webviewExternalEndpoint` (o `IProductService` do porte
//!   não traz `webviewContentExternalBaseUrlTemplate`). Entra quando o contrato de que depende for portado.

use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::environment::environment::EnvironmentService;
use je_platform::window::IPath;

/// `IWorkbenchEnvironmentService`: a workbench specific environment service that is only present in workbench layer.
///
/// NOTE: KEEP THIS INTERFACE AS SMALL AS POSSIBLE. AS SUCH: PUT NON-WEB PROPERTIES INTO THE NATIVE WORKBENCH
/// ENVIRONMENT SERVICE
pub trait IWorkbenchEnvironmentService: EnvironmentService {
    // --- Paths
    fn log_file(&self) -> URI;
    fn window_logs_path(&self) -> URI;
    fn ext_host_logs_path(&self) -> URI;

    // --- Extensions
    fn extension_enabled_proposed_api(&self) -> Option<Vec<String>>;

    // --- Config
    fn remote_authority(&self) -> Option<String>;
    fn skip_release_notes(&self) -> bool;
    fn skip_welcome(&self) -> bool;
    fn disable_workspace_trust(&self) -> bool;
    fn is_sessions_window(&self) -> bool;

    /// `filesToOpenOrCreate` (`IPath[] | undefined`, só o `fileUri`).
    fn files_to_open_or_create(&self) -> Option<Vec<IPath>> {
        None
    }
    /// `filesToDiff` (`IPath[] | undefined`, só o `fileUri`).
    fn files_to_diff(&self) -> Option<Vec<IPath>> {
        None
    }
    /// `filesToMerge` (`IPath[] | undefined`, só o `fileUri`).
    fn files_to_merge(&self) -> Option<Vec<IPath>> {
        None
    }

    // --- Development
    fn debug_renderer(&self) -> bool;
    fn log_extension_host_communication(&self) -> Option<bool>;
    fn enable_smoke_test_driver(&self) -> Option<bool>;
    fn prof_duration_markers(&self) -> Option<Vec<String>>;
}

service_id!(pub I_WORKBENCH_ENVIRONMENT_SERVICE: dyn IWorkbenchEnvironmentService = "environmentService");
