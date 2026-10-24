//! Porte parcial de `src/vs/workbench/services/extensions/common/workspaceContains.ts`: só `checkGlobFileExists`, que o
//! `MainThreadWorkspace.$checkExists` chama dentro de `invokeFunction`.
//!
//! Divergências:
//! - Fora do porte (entram junto de `extHostExtensionService.ts`, o único consumidor): `IExtensionActivationHost`,
//!   `IExtensionActivationResult`, `checkActivateWorkspaceContainsExtension` e os auxiliares `_activateIfFileName` e
//!   `_activateIfGlobPatterns`.
//! - `UriComponents` é a [`URI`] (já revivida pela camada de RPC). `toWorkspaceFolder(resource)` é o
//!   `to_workspace_folder` da plataforma com o `basenameOrAuthority` como nome.
//! - `ServicesAccessor` e `IInstantiationService` são os de `je-instantiation`; o `IInstantiationService` é o próprio
//!   [`InstantiationService`](je_instantiation::InstantiationService).

use je_base::cancellation::CancellationToken;
use je_base::errors::is_cancellation_error;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::{I_INSTANTIATION_SERVICE, ServicesAccessor};
use je_platform::resources::ext_uri;
use je_platform::workspace::to_workspace_folder;

use crate::services::search::query_builder::{ICommonQueryBuilderOptions, IFileQueryBuilderOptions, QueryBuilder, SearchPathPatternBuilder, WorkspaceFolderOrUri};
use crate::services::search::search::I_SEARCH_SERVICE;

/// `checkGlobFileExists(accessor, folders, includes, token)`.
pub fn check_glob_file_exists(accessor: &ServicesAccessor, folders: &[URI], includes: Vec<String>, token: CancellationToken) -> Promise<bool> {
    let instantiation_service = accessor.get(I_INSTANTIATION_SERVICE);
    let search_service = accessor.get(I_SEARCH_SERVICE);
    let query_builder = instantiation_service.create_instance::<QueryBuilder>(());
    let workspace_folders: Vec<WorkspaceFolderOrUri> =
        folders.iter().map(|folder| WorkspaceFolderOrUri::Folder(to_workspace_folder(folder, ext_uri().basename_or_authority(folder)))).collect();
    let query = query_builder.file(
        &workspace_folders,
        &IFileQueryBuilderOptions {
            common: ICommonQueryBuilderOptions { reason: Some("checkExists".to_string()), include_pattern: Some(SearchPathPatternBuilder::List(includes)), ..Default::default() },
            exists: Some(true),
            ..Default::default()
        },
    );

    search_service.file_search(&query, Some(token)).then_result(|result| match result {
        Ok(result) => Ok(result.limit_hit.unwrap_or(false)),
        Err(err) if is_cancellation_error(&err) => Ok(false),
        Err(err) => Err(err),
    })
}
