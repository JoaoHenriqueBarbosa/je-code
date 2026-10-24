//! Porte de `src/vs/workbench/services/extensions/common/workspaceContains.ts`: `checkActivateWorkspaceContainsExtension`
//! (o `AbstractExtensionService` e o host de extensões ativam por `workspaceContains:`) e `checkGlobFileExists`, que o
//! `MainThreadWorkspace.$checkExists` chama dentro de `invokeFunction`.
//!
//! Divergências:
//! - `IExtensionActivationHost` é o trait [`IExtensionActivationHost`] (um objeto literal no original).
//! - `UriComponents` é a [`URI`] (já revivida pela camada de RPC). `toWorkspaceFolder(resource)` é o
//!   `to_workspace_folder` da plataforma com o `basenameOrAuthority` como nome.
//! - `ServicesAccessor` e `IInstantiationService` são os de `je-instantiation`; o `IInstantiationService` é o próprio
//!   [`InstantiationService`](je_instantiation::InstantiationService).
//! - `Promise.all([...]).then(...)` que rejeita deixa a promessa de `checkActivateWorkspaceContainsExtension` pendente, como
//!   no original (o `resolve(undefined)` só roda quando todas assentam com sucesso).

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::{is_cancellation_error, on_unexpected_error};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_extensions::scanner::ExtensionDescription;
use je_instantiation::{I_INSTANTIATION_SERVICE, ServicesAccessor};
use je_platform::log::{LogServiceRef, Logger};
use je_platform::resources::{ext_uri, join_path};
use je_platform::workspace::to_workspace_folder;

use crate::services::search::query_builder::{ICommonQueryBuilderOptions, IFileQueryBuilderOptions, QueryBuilder, SearchPathPatternBuilder, WorkspaceFolderOrUri};
use crate::services::search::search::I_SEARCH_SERVICE;

const WORKSPACE_CONTAINS_TIMEOUT: u64 = 7000;

/// `IExtensionActivationHost`.
pub trait IExtensionActivationHost {
    fn log_service(&self) -> LogServiceRef;
    fn folders(&self) -> Vec<URI>;
    fn force_using_search(&self) -> bool;

    fn exists(&self, uri: &URI) -> Promise<bool>;
    fn check_exists(&self, folders: &[URI], includes: Vec<String>, token: CancellationToken) -> Promise<bool>;
}

/// `IExtensionActivationResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IExtensionActivationResult {
    pub activation_event: String,
}

/// `checkActivateWorkspaceContainsExtension(host, desc)`.
pub fn check_activate_workspace_contains_extension(host: Rc<dyn IExtensionActivationHost>, desc: &ExtensionDescription) -> Promise<Option<IExtensionActivationResult>> {
    let Some(activation_events) = &desc.manifest.activation_events else {
        return Promise::resolved(None);
    };

    let mut file_names: Vec<String> = Vec::new();
    let mut glob_patterns: Vec<String> = Vec::new();

    for activation_event in activation_events {
        if let Some(file_name_or_glob) = activation_event.strip_prefix("workspaceContains:") {
            if file_name_or_glob.contains('*') || file_name_or_glob.contains('?') || host.force_using_search() {
                glob_patterns.push(file_name_or_glob.to_owned());
            } else {
                file_names.push(file_name_or_glob.to_owned());
            }
        }
    }

    if file_names.is_empty() && glob_patterns.is_empty() {
        return Promise::resolved(None);
    }

    let (promise, resolver) = Promise::<Option<IExtensionActivationResult>>::new();
    let activate_resolver = resolver.clone();
    let activate: Rc<dyn Fn(&str)> = Rc::new(move |activation_event: &str| activate_resolver.resolve(Some(IExtensionActivationResult { activation_event: activation_event.to_owned() })));

    let file_name_promise =
        Promise::all(file_names.into_iter().map(|file_name| activate_if_file_name(host.clone(), file_name, activate.clone())).collect()).then(|_| ());
    let glob_pattern_promise = activate_if_glob_patterns(host, desc.identifier.value.clone(), glob_patterns, activate);

    Promise::all(vec![file_name_promise, glob_pattern_promise]).then(move |_| {
        // when all are done, resolve with undefined (relevant only if it was not activated so far)
        resolver.resolve(None);
    });

    promise
}

fn activate_if_file_name(host: Rc<dyn IExtensionActivationHost>, file_name: String, activate: Rc<dyn Fn(&str)>) -> Promise<()> {
    async_fn(async move {
        // find exact path
        for uri in host.folders() {
            if host.exists(&join_path(&uri, &[file_name.as_str()])).await? {
                // the file was found
                activate(&format!("workspaceContains:{file_name}"));
                return Ok(());
            }
        }
        Ok(())
    })
}

fn activate_if_glob_patterns(host: Rc<dyn IExtensionActivationHost>, extension_id: String, glob_patterns: Vec<String>, activate: Rc<dyn Fn(&str)>) -> Promise<()> {
    async_fn(async move {
        if glob_patterns.is_empty() {
            return Ok(());
        }

        let token_source = CancellationTokenSource::new(None);
        let search_p = host.check_exists(&host.folders(), glob_patterns.clone(), token_source.token());

        let timer = {
            let token_source = token_source.clone();
            let host = host.clone();
            let extension_id = extension_id.clone();
            let glob_patterns = glob_patterns.clone();
            runtime::set_timeout(
                move || {
                    token_source.cancel();
                    host.log_service().info(&format!(
                        "Not activating extension '{extension_id}': Timed out while searching for 'workspaceContains' pattern {}",
                        glob_patterns.join(",")
                    ));
                },
                WORKSPACE_CONTAINS_TIMEOUT,
            )
        };

        let mut exists = false;
        match search_p.await {
            Ok(value) => exists = value,
            Err(err) => {
                if !is_cancellation_error(&err) {
                    on_unexpected_error(&err);
                }
            }
        }

        token_source.dispose_with(false);
        runtime::clear_timeout(timer);

        if exists {
            // a file was found matching one of the glob patterns
            activate(&format!("workspaceContains:{}", glob_patterns.join(",")));
        }
        Ok(())
    })
}

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
