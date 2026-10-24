//! Porte de `src/vs/workbench/api/browser/mainThreadWorkspace.ts`: o lado do workbench do RPC `ExtHostWorkspace`
//! (pastas do workspace, busca de arquivos e de texto, salvar, rede, confiança, identidades de sessão de edição, URIs
//! canônicas e codificações).
//!
//! Divergências:
//! - **Injeção.** O construtor `(extHostContext, @ISearchService, ...)` é o [`Injectable`]: o argumento estático é o
//!   `IExtHostContext` (aqui o [`IMainContext`], que dá o `getProxy`) e as quinze dependências decoradas são a tupla
//!   `Deps`, na ordem dos parâmetros. `IInstantiationService` é o próprio [`InstantiationService`].
//! - **`@extHostNamedCustomer(MainContext.MainThreadWorkspace)`** é o registro do cliente no `extHostCustomers.ts`, que
//!   não foi portado: a classe é criada por `create_instance`, que é o que o registro faz.
//! - **`implements MainThreadWorkspaceShape`** é o trait de mesmo nome do protocolo; `UriComponents` já chegam revividas
//!   como [`URI`], então `URI.revive(...)` e `revive(options)` não existem.
//! - **`$startFileSearch` e `$startTextSearch`** recebem `IFileQueryBuilderOptions` e `ITextQueryBuilderOptions` do
//!   `QueryBuilder` portado. O `query._reason = 'startTextSearch'` é o `reason` da parte comum da consulta. O
//!   `(<IFileMatch>p).results` (verdadeiro quando o item tem `results`) é "o item é um `IFileMatch` com `results`".
//! - **`activeCancelTokens`** é lido só pelo `dispose` (como no original, nada o preenche), então é um mapa de
//!   [`CancellationTokenSource`] indexado por `u64`.
//! - **Métodos `async`** (`$save`, `$isResourceTrusted` e os provedores de identidade) são [`async_fn`] e
//!   [`async_fn_maybe`], para manter as rodadas de microtarefa do original.
//! - **`isNative`** é o `is_native` de [`Platform::detect`].
//! - **`workspace.configuration || undefined`**: `Option<URI>` já é o `undefined`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::{async_fn, async_fn_maybe};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::is_cancellation_error;
use je_base::lifecycle::{Disposable, DisposableMap, DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_instantiation::{Injectable, InstantiationService};
use je_platform::canonical_uri::{ICanonicalUriProvider, ICanonicalUriService};
use je_platform::edit_sessions::{EditSessionIdentityMatch, IEditSessionIdentityProvider, IEditSessionIdentityService};
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::{ILabelService, WorkspaceLabelTarget};
use je_platform::notification::notification::{INotificationService, NotificationMessage, StatusMessageOptions};
use je_platform::request::request::{AuthInfo, Credentials, IRequestService};
use je_platform::workspace::{IWorkspace, IWorkspaceContextService, Workspace, WorkspaceFolder, is_untitled_workspace};
use je_platform::workspace_trust::{IWorkspaceTrustManagementService, IWorkspaceTrustRequestService, ResourceTrustRequestOptions, WorkspaceTrustRequestOptions};
use je_platform::workspaces::workspaces::WorkspaceFolderCreationData;

use crate::api::common::ext_host_protocol::{
    EncodingOptionDto, ExtHostContext, ExtHostWorkspaceShape, IMainContext, IRawFileMatch2, ITextSearchComplete, IWorkspaceData, MainThreadWorkspaceShape, ResolvedDecodingDto,
    ResolvedEncodingDto, SaveOptionsDto, WorkspaceFolderDataDto, WorkspaceFolderToAdd,
};
use crate::common::editor::{EditorInputOrUntyped, EditorResourceAccessor, EditorResourceAccessorOptions, SaveOptions, SaveReason, SideBySideEditor};
use crate::services::editor::editor_service::{FindEditorOptions, IEditorService, SaveAllEditorsOptions, SaveEditorsOptions, SaveEditorsResult, IncludeUntitled};
use crate::services::extensions::workspace_contains::check_glob_file_exists;
use crate::services::search::query_builder::{IFileQueryBuilderOptions, ITextQueryBuilderOptions, QueryBuilder, WorkspaceFolderOrUri};
use crate::services::search::search::{ISearchProgressItem, ISearchService, PatternInfo, SearchProgressCallback};
use crate::services::textfile::textfiles::{ITextFileService, ReadTextFileEncodingOptions, WriteTextFileOptions};
use crate::services::workspaces::workspace_editing::IWorkspaceEditingService;

/// `MainThreadWorkspace`.
pub struct MainThreadWorkspace {
    base: Rc<Disposable>,
    active_cancel_tokens: RefCell<HashMap<u64, CancellationTokenSource>>,
    proxy: Rc<dyn ExtHostWorkspaceShape>,
    query_builder: Rc<QueryBuilder>,
    search_service: Rc<dyn ISearchService>,
    context_service: Rc<dyn IWorkspaceContextService>,
    edit_session_identity_service: Rc<dyn IEditSessionIdentityService>,
    canonical_uri_service: Rc<dyn ICanonicalUriService>,
    editor_service: Rc<dyn IEditorService>,
    workspace_editing_service: Rc<dyn IWorkspaceEditingService>,
    notification_service: Rc<dyn INotificationService>,
    request_service: Rc<dyn IRequestService>,
    instantiation_service: Rc<InstantiationService>,
    label_service: Rc<dyn ILabelService>,
    environment_service: Rc<dyn EnvironmentService>,
    workspace_trust_management_service: Rc<dyn IWorkspaceTrustManagementService>,
    workspace_trust_request_service: Rc<dyn IWorkspaceTrustRequestService>,
    text_file_service: Rc<dyn ITextFileService>,
    registered_edit_session_providers: Rc<DisposableMap<u64, dyn IDisposable>>,
    registered_canonical_uri_providers: Rc<DisposableMap<u64, dyn IDisposable>>,
}

impl Injectable for MainThreadWorkspace {
    type Args = Rc<dyn IMainContext>;
    type Deps = (
        Rc<dyn ISearchService>,
        Rc<dyn IWorkspaceContextService>,
        Rc<dyn IEditSessionIdentityService>,
        Rc<dyn ICanonicalUriService>,
        Rc<dyn IEditorService>,
        Rc<dyn IWorkspaceEditingService>,
        Rc<dyn INotificationService>,
        Rc<dyn IRequestService>,
        Rc<InstantiationService>,
        Rc<dyn ILabelService>,
        Rc<dyn EnvironmentService>,
        Rc<dyn IFileService>,
        Rc<dyn IWorkspaceTrustManagementService>,
        Rc<dyn IWorkspaceTrustRequestService>,
        Rc<dyn ITextFileService>,
    );

    fn create(
        ext_host_context: Self::Args,
        (
            search_service,
            context_service,
            edit_session_identity_service,
            canonical_uri_service,
            editor_service,
            workspace_editing_service,
            notification_service,
            request_service,
            instantiation_service,
            label_service,
            environment_service,
            file_service,
            workspace_trust_management_service,
            workspace_trust_request_service,
            text_file_service,
        ): Self::Deps,
    ) -> Rc<Self> {
        let query_builder = instantiation_service.create_instance::<QueryBuilder>(());
        let proxy = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_WORKSPACE);
        let base = Disposable::new();
        let registered_edit_session_providers = DisposableMap::<u64, dyn IDisposable>::new();
        base.register(registered_edit_session_providers.clone());
        let registered_canonical_uri_providers = DisposableMap::<u64, dyn IDisposable>::new();
        base.register(registered_canonical_uri_providers.clone());

        let this = Rc::new(Self {
            base,
            active_cancel_tokens: RefCell::new(HashMap::new()),
            proxy,
            query_builder,
            search_service,
            context_service,
            edit_session_identity_service,
            canonical_uri_service,
            editor_service,
            workspace_editing_service,
            notification_service,
            request_service,
            instantiation_service,
            label_service,
            environment_service,
            workspace_trust_management_service,
            workspace_trust_request_service,
            text_file_service,
            registered_edit_session_providers,
            registered_canonical_uri_providers,
        });

        let workspace = this.context_service.get_workspace();
        // The workspace file is provided be a unknown file system provider. It might come
        // from the extension host. So initialize now knowing that `rootPath` is undefined.
        let configuration = workspace.configuration();
        if configuration.as_ref().is_some_and(|configuration| !Platform::detect().is_native && !file_service.has_provider(configuration)) {
            this.proxy.rpc_initialize_workspace(this.get_workspace_data(&workspace), this.is_workspace_trusted());
        } else {
            let weak = Rc::downgrade(&this);
            this.context_service.get_complete_workspace().then(move |workspace| {
                if let Some(this) = weak.upgrade() {
                    this.proxy.rpc_initialize_workspace(this.get_workspace_data(&workspace), this.is_workspace_trusted());
                }
            });
        }

        let weak = Rc::downgrade(&this);
        this.base.register(this.context_service.on_did_change_workspace_folders().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_workspace();
                }
            }
        }));
        this.base.register(this.context_service.on_did_change_workbench_state().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_workspace();
                }
            }
        }));
        this.base.register(this.workspace_trust_management_service.on_did_change_trust().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_grant_workspace_trust();
                }
            }
        }));
        this.base.register(this.workspace_trust_management_service.on_did_change_trusted_folders().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_workspace_trusted_folders();
            }
        }));
        this
    }
}

impl IDisposable for MainThreadWorkspace {
    fn dispose(&self) {
        self.base.dispose();

        for token_source in self.active_cancel_tokens.borrow().values() {
            token_source.cancel();
        }
    }
}

/// `{ ... }` de `EditSessionIdentityProvider`: o provedor que repassa as chamadas ao extension host.
struct ExtHostEditSessionIdentityProvider {
    scheme: String,
    proxy: Rc<dyn ExtHostWorkspaceShape>,
}

impl IEditSessionIdentityProvider for ExtHostEditSessionIdentityProvider {
    fn scheme(&self) -> String {
        self.scheme.clone()
    }

    fn get_edit_session_identifier(&self, workspace_folder: &WorkspaceFolder, token: CancellationToken) -> Promise<Option<String>> {
        let proxy = self.proxy.clone();
        let uri = workspace_folder.uri.clone();
        async_fn_maybe(async move { Ok(MaybePromise::Promise(proxy.rpc_get_edit_session_identifier(uri, token))) })
    }

    fn provide_edit_session_identity_match(
        &self,
        workspace_folder: &WorkspaceFolder,
        identity1: &str,
        identity2: &str,
        token: CancellationToken,
    ) -> Promise<Option<EditSessionIdentityMatch>> {
        let proxy = self.proxy.clone();
        let uri = workspace_folder.uri.clone();
        let (identity1, identity2) = (identity1.to_string(), identity2.to_string());
        async_fn_maybe(async move { Ok(MaybePromise::Promise(proxy.rpc_provide_edit_session_identity_match(uri, &identity1, &identity2, token))) })
    }
}

/// O provedor de URI canônica que repassa a chamada ao extension host.
struct ExtHostCanonicalUriProvider {
    scheme: String,
    proxy: Rc<dyn ExtHostWorkspaceShape>,
}

impl ICanonicalUriProvider for ExtHostCanonicalUriProvider {
    fn scheme(&self) -> String {
        self.scheme.clone()
    }

    fn provide_canonical_uri(&self, uri: &URI, target_scheme: &str, token: CancellationToken) -> Promise<Option<URI>> {
        let proxy = self.proxy.clone();
        let (uri, target_scheme) = (uri.clone(), target_scheme.to_string());
        async_fn(async move {
            let result = proxy.rpc_provide_canonical_uri(uri, &target_scheme, token).await?;
            Ok(result)
        })
    }
}

impl MainThreadWorkspace {
    // --- workspace ---

    fn get_status_message(extension_name: &str, add_count: usize, remove_count: usize) -> String {
        let wants_to_add = add_count > 0;
        let wants_to_delete = remove_count > 0;

        // Add Folders
        if wants_to_add && !wants_to_delete {
            if add_count == 1 {
                localize("folderStatusMessageAddSingleFolder", "Extension '{0}' added 1 folder to the workspace", &[extension_name])
            } else {
                localize("folderStatusMessageAddMultipleFolders", "Extension '{0}' added {1} folders to the workspace", &[extension_name, &add_count.to_string()])
            }
        }
        // Delete Folders
        else if wants_to_delete && !wants_to_add {
            if remove_count == 1 {
                localize("folderStatusMessageRemoveSingleFolder", "Extension '{0}' removed 1 folder from the workspace", &[extension_name])
            } else {
                localize("folderStatusMessageRemoveMultipleFolders", "Extension '{0}' removed {1} folders from the workspace", &[extension_name, &remove_count.to_string()])
            }
        }
        // Change Folders
        else {
            localize("folderStatusChangeFolder", "Extension '{0}' changed folders of the workspace", &[extension_name])
        }
    }

    fn on_did_change_workspace(&self) {
        self.proxy.rpc_accept_workspace_data(self.get_workspace_data(&self.context_service.get_workspace()));
    }

    fn get_workspace_data(&self, workspace: &Rc<Workspace>) -> Option<IWorkspaceData> {
        if !self.context_service.has_workspace_data() {
            return None;
        }
        let configuration = workspace.configuration();
        let is_untitled = match &configuration {
            Some(configuration) => is_untitled_workspace(configuration, &self.environment_service.untitled_workspaces_home()),
            None => false,
        };
        let full: Rc<dyn IWorkspace> = workspace.clone();
        Some(IWorkspaceData {
            configuration,
            is_untitled: Some(is_untitled),
            folders: workspace.folders().into_iter().map(|folder| WorkspaceFolderDataDto { uri: folder.uri, name: folder.name, index: folder.index }).collect(),
            id: workspace.id(),
            name: self.label_service.get_workspace_label(&WorkspaceLabelTarget::Full(full), None),
            transient: Some(workspace.transient()),
        })
    }

    // --- save & edit resources ---

    fn save_result_to_uris(result: &SaveEditorsResult) -> Vec<URI> {
        if !result.success {
            return Vec::new();
        }

        let options = EditorResourceAccessorOptions { support_side_by_side: Some(SideBySideEditor::Primary), filter_by_scheme: None };
        result.editors.iter().filter_map(|editor: &EditorInputOrUntyped| EditorResourceAccessor::get_canonical_uri(Some(editor), Some(&options))).collect()
    }

    // --- trust ---

    fn is_workspace_trusted(&self) -> bool {
        self.workspace_trust_management_service.is_workspace_trusted()
    }

    fn on_did_grant_workspace_trust(&self) {
        self.proxy.rpc_on_did_grant_workspace_trust();
    }

    fn on_did_change_workspace_trusted_folders(&self) {
        self.proxy.rpc_on_did_change_workspace_trusted_folders();
    }
}

impl MainThreadWorkspaceShape for MainThreadWorkspace {
    fn rpc_update_workspace_folders(&self, extension_name: &str, index: f64, delete_count: f64, folders_to_add: Vec<WorkspaceFolderToAdd>) -> Promise<()> {
        let workspace_folders_to_add: Vec<WorkspaceFolderCreationData> =
            folders_to_add.into_iter().map(|folder| WorkspaceFolderCreationData { uri: folder.uri, name: folder.name }).collect();

        // Indicate in status message
        drop(self.notification_service.status(
            NotificationMessage::Text(Self::get_status_message(extension_name, workspace_folders_to_add.len(), delete_count.max(0.0) as usize)),
            Some(StatusMessageOptions { show_after: None, hide_after: Some(10 * 1000 /* 10s */) }),
        ));

        self.workspace_editing_service.update_folders(index, Some(delete_count), Some(workspace_folders_to_add), Some(true))
    }

    // --- search ---

    fn rpc_start_file_search(&self, include_folder: Option<URI>, options: IFileQueryBuilderOptions, token: CancellationToken) -> Promise<Option<Vec<URI>>> {
        let workspace = self.context_service.get_workspace();

        let folders: Vec<WorkspaceFolderOrUri> = match include_folder {
            Some(include_folder) => vec![WorkspaceFolderOrUri::Uri(include_folder)],
            None => workspace.folders().into_iter().map(WorkspaceFolderOrUri::Folder).collect(),
        };
        let query = self.query_builder.file(&folders, &options);

        self.search_service.file_search(&query, Some(token)).then_result(|result| match result {
            Ok(result) => Ok(Some(result.results.iter().map(|m| m.resource.clone()).collect())),
            Err(err) if is_cancellation_error(&err) => Ok(None),
            Err(err) => Err(err),
        })
    }

    fn rpc_start_text_search(
        &self,
        pattern: PatternInfo,
        folder: Option<URI>,
        options: ITextQueryBuilderOptions,
        request_id: u64,
        token: CancellationToken,
    ) -> Promise<Option<ITextSearchComplete>> {
        let workspace = self.context_service.get_workspace();
        let folders: Vec<URI> = match folder {
            Some(folder) => vec![folder],
            None => workspace.folders().into_iter().map(|folder| folder.uri).collect(),
        };

        let mut query = self.query_builder.text(pattern, Some(folders.as_slice()), &options);
        query.common.reason = Some("startTextSearch".to_string());

        let proxy = self.proxy.clone();
        let on_progress: SearchProgressCallback = Rc::new(move |p: &ISearchProgressItem| {
            if let Some(file_match) = p.as_file_match()
                && file_match.results.is_some()
            {
                proxy.rpc_handle_text_search_result(IRawFileMatch2 { resource: file_match.resource.clone(), results: file_match.results.clone() }, request_id);
            }
        });

        self.search_service.text_search(&query, Some(token), Some(on_progress)).then_result(|result| match result {
            Ok(result) => Ok(Some(ITextSearchComplete { limit_hit: result.limit_hit, message: None })),
            Err(err) if is_cancellation_error(&err) => Ok(None),
            Err(err) => Err(err),
        })
    }

    fn rpc_check_exists(&self, folders: Vec<URI>, includes: Vec<String>, token: CancellationToken) -> Promise<bool> {
        self.instantiation_service.invoke_function(|accessor| check_glob_file_exists(accessor, &folders, includes, token))
    }

    // --- save & edit resources ---

    fn rpc_save(&self, uri: &URI, options: SaveOptionsDto) -> Promise<Option<URI>> {
        let uri = uri.clone();
        let editor_service = self.editor_service.clone();
        async_fn(async move {
            let editors = editor_service.find_editors(&uri, Some(FindEditorOptions { support_side_by_side: Some(SideBySideEditor::Primary), order: None }));
            let result = editor_service
                .save(
                    editors,
                    Some(SaveEditorsOptions {
                        base: SaveOptions { reason: Some(SaveReason::Explicit), force: Some(!options.save_as), ..Default::default() },
                        save_as: Some(options.save_as),
                    }),
                )
                .await?;

            Ok(Self::save_result_to_uris(&result).into_iter().next())
        })
    }

    fn rpc_save_all(&self, include_untitled: Option<bool>) -> Promise<bool> {
        self.editor_service
            .save_all(Some(SaveAllEditorsOptions { include_untitled: include_untitled.map(IncludeUntitled::Bool), ..Default::default() }))
            .then(|res| res.success)
    }

    fn rpc_resolve_proxy(&self, url: &str) -> Promise<Option<String>> {
        self.request_service.resolve_proxy(url)
    }

    fn rpc_lookup_authorization(&self, auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        self.request_service.lookup_authorization(auth_info)
    }

    fn rpc_lookup_kerberos_authorization(&self, url: &str) -> Promise<Option<String>> {
        self.request_service.lookup_kerberos_authorization(url)
    }

    fn rpc_load_certificates(&self) -> Promise<Vec<String>> {
        self.request_service.load_certificates()
    }

    // --- trust ---

    fn rpc_request_resource_trust(&self, options: ResourceTrustRequestOptions) -> Promise<Option<bool>> {
        self.workspace_trust_request_service.request_resources_trust(options)
    }

    fn rpc_request_workspace_trust(&self, options: Option<WorkspaceTrustRequestOptions>) -> Promise<Option<bool>> {
        self.workspace_trust_request_service.request_workspace_trust(options)
    }

    fn rpc_is_resource_trusted(&self, resource: &URI) -> Promise<bool> {
        let uri = resource.clone();
        let trust_management_service = self.workspace_trust_management_service.clone();
        async_fn(async move {
            let trust_info = trust_management_service.get_uri_trust_info(&uri).await?;
            Ok(trust_info.trusted)
        })
    }

    // --- edit sessions ---

    fn rpc_register_edit_session_identity_provider(&self, handle: u64, scheme: &str) {
        let disposable: DisposableRef =
            self.edit_session_identity_service.register_edit_session_identity_provider(Rc::new(ExtHostEditSessionIdentityProvider { scheme: scheme.to_string(), proxy: self.proxy.clone() }));

        self.registered_edit_session_providers.set(handle, disposable, false);
    }

    fn rpc_unregister_edit_session_identity_provider(&self, handle: u64) {
        self.registered_edit_session_providers.delete_and_dispose(&handle);
    }

    // --- canonical uri identities ---

    fn rpc_register_canonical_uri_provider(&self, handle: u64, scheme: &str) {
        let disposable: DisposableRef =
            self.canonical_uri_service.register_canonical_uri_provider(Rc::new(ExtHostCanonicalUriProvider { scheme: scheme.to_string(), proxy: self.proxy.clone() }));

        self.registered_canonical_uri_providers.set(handle, disposable, false);
    }

    fn rpc_unregister_canonical_uri_provider(&self, handle: u64) {
        self.registered_canonical_uri_providers.delete_and_dispose(&handle);
    }

    // --- encodings

    fn rpc_resolve_decoding(&self, resource: Option<URI>, options: Option<EncodingOptionDto>) -> Promise<ResolvedDecodingDto> {
        let options = options.map(|options| ReadTextFileEncodingOptions { encoding: Some(options.encoding), ..Default::default() });
        self.text_file_service.resolve_decoding(resource.as_ref(), options).then(|decoding| ResolvedDecodingDto {
            preferred_encoding: decoding.preferred_encoding,
            guess_encoding: decoding.guess_encoding,
            candidate_guess_encodings: decoding.candidate_guess_encodings,
        })
    }

    fn rpc_validate_detected_encoding(&self, resource: Option<URI>, detected_encoding: &str, options: Option<EncodingOptionDto>) -> Promise<String> {
        let options = options.map(|options| ReadTextFileEncodingOptions { encoding: Some(options.encoding), ..Default::default() });
        self.text_file_service.validate_detected_encoding(resource.as_ref(), detected_encoding, options)
    }

    fn rpc_resolve_encoding(&self, resource: Option<URI>, options: Option<EncodingOptionDto>) -> Promise<ResolvedEncodingDto> {
        let options = options.map(|options| WriteTextFileOptions { encoding: Some(options.encoding), ..Default::default() });
        self.text_file_service.resolve_encoding(resource.as_ref(), options).then(|encoding| ResolvedEncodingDto { encoding: encoding.encoding, add_bom: encoding.add_bom })
    }
}
