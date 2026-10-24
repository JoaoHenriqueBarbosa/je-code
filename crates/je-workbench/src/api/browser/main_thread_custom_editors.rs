//! Porte de `src/vs/workbench/api/browser/mainThreadCustomEditors.ts`: o lado do workbench do RPC `ExtHostCustomEditors`, que
//! registra os provedores de editor personalizado (binário e de texto), resolve as entradas de editor pelo host de extensões,
//! guarda o modelo do documento (`MainThreadCustomEditorModel`, uma working copy), a navegação dos editores de texto e a
//! resolução das comparações.
//!
//! Divergências:
//! - **Injeção.** O construtor `(context, mainThreadWebview, mainThreadWebviewPanels, @IExtensionService, @IStorageService,
//!   @IWorkingCopyService, @IWorkingCopyFileService, @ICustomEditorService, @IEditorGroupsService, @IEditorService,
//!   @IInstantiationService, @IWebviewWorkbenchService, @IUriIdentityService, @IUntitledTextEditorService, @IOutlineService)` é
//!   [`MainThreadCustomEditors::new`] com [`MainThreadCustomEditorsServices`]; o `IInstantiationService`, que só cria o modelo
//!   de texto e o `MainThreadCustomEditorModel`, é o [`CustomEditorModelServices`] (os serviços desses dois). O
//!   `MainThreadWebviews` entra como [`IMainThreadWebviews`] e o `MainThreadWebviewPanels` como [`IMainThreadWebviewPanels`].
//!   `@extHostNamedCustomer` é o registro do `extHostCustomers.ts`, que não foi portado.
//! - **Herança de entradas.** `webviewInput instanceof CustomEditorInput | CustomEditorDiffInput |
//!   CustomEditorSideBySideDiffInput` é [`CustomEditorWebviewInput::of`] (o `kind_as` de [`WebviewInput`]); o `WeakMap` de
//!   handles é a lista de pares `Weak<WebviewInput>` e handle.
//! - **Erros do RPC.** Os `$onDidEdit` e `$onContentChange` são `async` no original e rejeitam quando o modelo não existe; o
//!   protocolo do porte devolve `()`, então a rejeição vai para o `onUnexpectedError`. `$registerTextEditorProvider`,
//!   `$registerCustomEditorProvider` e `$unregisterEditorProvider` lançam pelo [`errors::throw`].
//! - **`workingCopy instanceof MainThreadCustomEditorModel`** (o provedor de working copies do `registerWorkingCopyProvider`)
//!   é a identidade do `Rc` entre as working copies registradas e os modelos que este objeto criou (a lista de `Weak`).
//! - **`MainThreadCustomEditorModel extends ResourceWorkingCopy`** guarda o [`ResourceWorkingCopy`] (o `Disposable` que ele traz
//!   não é acessível para `_register`, então o modelo tem o seu); `implements ICustomEditorModel` e `IWorkingCopy` são os dois
//!   traits (que têm métodos de mesmo nome, `isDirty`, `revert`..., e por isso o corpo mora num método próprio ou no trait de
//!   working copy). `HotExitState` é o enum com um número de identidade no estado pendente (a comparação `===`).
//! - **`workingCopy.meta as CustomDocumentBackupData`** é o [`Dynamic`] do backup, do qual só se lê o `backupId`
//!   ([`CustomDocumentBackupData::backup_id_of`]). `IUndoRedoService` é o recorte de `je-platform`
//!   ([`IUndoRedoService`]).
//! - `DeferredPromise.p` é o campo `promise`; `IReference` é a [`Reference`]. `MutableDisposable`, `DisposableMap` e
//!   `DisposableStore` são os de `je-base`, e `onUnexpectedError(error)` é [`errors::on_unexpected_error`].
//! - `ISelection` do `$onDidChangeCustomTextEditorSelection` é o [`Selection`].
//! - O fechamento `webviewInput.onMove(async newResource => ...)` é um `Rc<dyn Fn(URI)>` que dispara a promessa e reporta o
//!   erro dela como inesperado.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, create_cancelable_promise, race_cancellation};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error, is_cancellation_error, on_unexpected_error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableMap, DisposableRef, DisposableStore, IDisposable, id_of, to_disposable};
use je_base::marshalling::{Marshalled, stringify};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::{CancelablePromise, DeferredPromise, Promise};
use je_base::resources::to_local_resource;
use je_base::strings::multibyte_aware_btoa;
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_base::uuid::generate_uuid;
use je_base::js_string::JsString;
use je_editor::selection::Selection;
use je_platform::dialogs::dialogs::IFileDialogService;
use je_platform::files::files::FileOperation;
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::path::path_service::IPathService;
use je_platform::resources::ext_uri;
use je_platform::storage::StorageService;
use je_platform::undo_redo::undo_redo::{IUndoRedoService, ResourceUndoRedoElement};
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::api::browser::main_thread_custom_text_editor_navigation::MainThreadCustomTextEditorNavigation;
use crate::api::browser::main_thread_webview_panels::{IMainThreadWebviewPanels, revive_webview_options, webview_content_options_to_dto, webview_options_to_panel_options_dto};
use crate::api::browser::main_thread_webviews::{IMainThreadWebviews, MainThreadWebviewOptions, revive_webview_extension};
use crate::api::common::ext_host_protocol::{
    CreateCustomDocumentResult, CustomEditorDiffInitData, CustomEditorProviderCapabilities, CustomEditorResolveInitData, CustomEditorSideBySideDiffInitData,
    CustomEditorSideBySideDiffWebviewHandles, EditorGroupColumn, ExtHostContext, ExtHostCustomEditorsShape, IMainContext, MainThreadCustomEditorsShape,
    WebviewExtensionDescription as WebviewExtensionDescriptionDto, WebviewPanelOptions as WebviewPanelOptionsDto,
};
use crate::common::editor::editor_input::EditorInput;
use crate::common::editor::{EditorInputOrUntyped, RevertOptions, SaveOptions};
use crate::contrib::custom_editor::custom_editor::{
    CustomEditorCapabilities, CustomEditorModelReference, ICustomEditorModel, ICustomEditorService,
};
use crate::contrib::custom_editor::custom_editor_diff_input::{CustomEditorDiffInput, CustomEditorSideBySideDiffInput, CustomEditorSideBySideDiffSide};
use crate::contrib::custom_editor::custom_editor_input::CustomEditorInput;
use crate::contrib::custom_editor::custom_editor_input_factory::{
    CustomDocumentBackupData, CustomDocumentBackupExtension, CustomDocumentBackupWebview,
};
use crate::contrib::custom_editor::custom_text_editor_model::{CustomTextEditorModel, CustomTextEditorModelServices};
use crate::contrib::webview::browser_webview::{ExtensionKeyedWebviewOriginStore, IWebview, WebviewExtensionDescription};
use crate::contrib::webview_panel::webview_editor_input::WebviewInput;
use crate::contrib::webview_panel::webview_workbench_service::{IWebviewWorkbenchService, WebviewResolver};
use crate::services::editor::editor_group_column::editor_group_to_column;
use crate::services::editor::editor_groups_service::{GroupRef, IEditorGroupsService};
use crate::services::editor::editor_service::IEditorService;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::extensions::extensions::IExtensionService;
use crate::services::files_configuration::files_configuration_service::Readonly;
use crate::services::outline::outline::IOutlineService;
use crate::services::textfile::textfiles::ITextFileService;
use crate::services::textmodel_resolver::resolver_service::ITextModelService;
use crate::services::untitled::untitled_text_editor_service::IUntitledTextEditorService;
use crate::services::working_copy::resource_working_copy::{IResourceWorkingCopy, ResourceWorkingCopy};
use crate::services::working_copy::working_copy::{IWorkingCopy, NO_TYPE_ID, WorkingCopyBackup, WorkingCopyCapabilities, WorkingCopySaveEvent};
use crate::services::working_copy::working_copy_service::IWorkingCopyService;
use crate::services::textfile::collaborators::{IWorkingCopyFileService, WorkingCopyFileEvent};

/// `CustomEditorModelType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CustomEditorModelType {
    Custom,
    Text,
}

/// `CustomEditorWebviewInput = CustomEditorInput | CustomEditorDiffInput | CustomEditorSideBySideDiffInput`.
#[derive(Clone, Copy)]
enum CustomEditorWebviewInput<'a> {
    Input(&'a CustomEditorInput),
    Diff(&'a CustomEditorDiffInput),
    SideBySide(&'a CustomEditorSideBySideDiffInput),
}

impl<'a> CustomEditorWebviewInput<'a> {
    /// `webviewInput instanceof CustomEditorInput || ... CustomEditorDiffInput || ... CustomEditorSideBySideDiffInput`.
    fn of(input: &'a WebviewInput) -> Option<Self> {
        if let Some(custom) = input.kind_as::<CustomEditorInput>() {
            return Some(Self::Input(custom));
        }
        if let Some(diff) = input.kind_as::<CustomEditorDiffInput>() {
            return Some(Self::Diff(diff));
        }
        input.kind_as::<CustomEditorSideBySideDiffInput>().map(Self::SideBySide)
    }
}

fn is_equal(a: &URI, b: &URI) -> bool {
    ext_uri().is_equal(Some(a), Some(b), false)
}

/// `CustomEditorDiffInitData` (e o `initData` de `$resolveCustomEditor`): o `CustomEditorSideBySideDiffData`.
#[derive(Clone)]
struct CustomEditorSideBySideDiffData {
    handle: String,
    init_data: CustomEditorDiffInitData,
}

#[derive(Default)]
struct PendingSideBySideSides {
    original: Option<CustomEditorSideBySideDiffData>,
    modified: Option<CustomEditorSideBySideDiffData>,
    started: bool,
}

/// `PendingCustomEditorSideBySideDiffResolution`.
struct PendingCustomEditorSideBySideDiffResolution {
    sides: RefCell<PendingSideBySideSides>,
    promise: DeferredPromise<()>,
    cancellation: CancellationTokenSource,
    disposables: Rc<DisposableStore>,
}

/// O `{ selection, changed }` de `_pendingNavigation`.
#[derive(Default)]
struct PendingNavigation {
    selection: Option<Selection>,
    changed: bool,
}

/// Os serviços do `MainThreadCustomEditorModel` e do `CustomTextEditorModel` (o que o `IInstantiationService` injeta).
#[derive(Clone)]
pub struct CustomEditorModelServices {
    pub file_dialog_service: Rc<dyn IFileDialogService>,
    pub file_service: Rc<dyn IFileService>,
    pub label_service: Rc<dyn ILabelService>,
    pub undo_redo_service: Rc<dyn IUndoRedoService>,
    pub environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    pub path_service: Rc<dyn IPathService>,
    pub text_model_service: Rc<dyn ITextModelService>,
    pub text_file_service: Rc<dyn ITextFileService>,
}

/// Os serviços que o construtor do `MainThreadCustomEditors` injeta.
pub struct MainThreadCustomEditorsServices {
    pub extension_service: Rc<dyn IExtensionService>,
    pub storage_service: Rc<dyn StorageService>,
    pub working_copy_service: Rc<dyn IWorkingCopyService>,
    pub working_copy_file_service: Rc<dyn IWorkingCopyFileService>,
    pub custom_editor_service: Rc<dyn ICustomEditorService>,
    pub editor_group_service: Rc<dyn IEditorGroupsService>,
    pub editor_service: Rc<dyn IEditorService>,
    pub webview_workbench_service: Rc<dyn IWebviewWorkbenchService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
    pub untitled_text_editor_service: Rc<dyn IUntitledTextEditorService>,
    pub outline_service: Rc<dyn IOutlineService>,
    pub model: CustomEditorModelServices,
}

type GetEditors = Rc<dyn Fn() -> Vec<Rc<WebviewInput>>>;

pub struct MainThreadCustomEditors {
    this: Weak<MainThreadCustomEditors>,
    base: Rc<Disposable>,

    proxy_custom_editors: Rc<dyn ExtHostCustomEditorsShape>,

    editor_providers: Rc<DisposableMap<String, DisposableStore>>,
    navigation: Rc<DisposableMap<String, MainThreadCustomTextEditorNavigation>>,
    pending_navigation: RefCell<HashMap<String, Rc<RefCell<PendingNavigation>>>>,

    editor_rename_backups: RefCell<HashMap<String, Dynamic>>,
    pending_side_by_side_diff_resolutions: RefCell<HashMap<String, Rc<PendingCustomEditorSideBySideDiffResolution>>>,

    /// Webview handle of each resolved input. An input is resolved again if creating its document failed
    /// (o `WeakMap<CustomEditorWebviewInput, WebviewHandle>`).
    webview_handles: RefCell<Vec<(Weak<WebviewInput>, String)>>,

    /// Os modelos de documento personalizado criados (para o provedor de working copies).
    custom_models: Rc<RefCell<Vec<Weak<MainThreadCustomEditorModel>>>>,

    webview_origin_store: ExtensionKeyedWebviewOriginStore,

    main_thread_webview: Rc<dyn IMainThreadWebviews>,
    main_thread_webview_panels: Rc<dyn IMainThreadWebviewPanels>,

    custom_editor_service: Rc<dyn ICustomEditorService>,
    editor_group_service: Rc<dyn IEditorGroupsService>,
    editor_service: Rc<dyn IEditorService>,
    webview_workbench_service: Rc<dyn IWebviewWorkbenchService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    untitled_text_editor_service: Rc<dyn IUntitledTextEditorService>,
    outline_service: Rc<dyn IOutlineService>,
    working_copy_service: Rc<dyn IWorkingCopyService>,
    extension_service: Rc<dyn IExtensionService>,
    model_services: CustomEditorModelServices,
}

impl MainThreadCustomEditors {
    pub fn new(
        context: &dyn IMainContext,
        main_thread_webview: Rc<dyn IMainThreadWebviews>,
        main_thread_webview_panels: Rc<dyn IMainThreadWebviewPanels>,
        services: MainThreadCustomEditorsServices,
    ) -> Rc<Self> {
        let MainThreadCustomEditorsServices {
            extension_service,
            storage_service,
            working_copy_service,
            working_copy_file_service,
            custom_editor_service,
            editor_group_service,
            editor_service,
            webview_workbench_service,
            uri_identity_service,
            untitled_text_editor_service,
            outline_service,
            model,
        } = services;

        let base = Disposable::new();
        let editor_providers: Rc<DisposableMap<String, DisposableStore>> = DisposableMap::new();
        base.register(editor_providers.clone());
        let navigation: Rc<DisposableMap<String, MainThreadCustomTextEditorNavigation>> = DisposableMap::new();
        base.register(navigation.clone());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            proxy_custom_editors: context.get_proxy(&ExtHostContext::EXT_HOST_CUSTOM_EDITORS),
            editor_providers,
            navigation,
            pending_navigation: RefCell::new(HashMap::new()),
            editor_rename_backups: RefCell::new(HashMap::new()),
            pending_side_by_side_diff_resolutions: RefCell::new(HashMap::new()),
            webview_handles: RefCell::new(Vec::new()),
            custom_models: Rc::default(),
            webview_origin_store: ExtensionKeyedWebviewOriginStore::new("mainThreadCustomEditors.origins", storage_service),
            main_thread_webview,
            main_thread_webview_panels,
            custom_editor_service,
            editor_group_service,
            editor_service,
            webview_workbench_service: webview_workbench_service.clone(),
            uri_identity_service,
            untitled_text_editor_service,
            outline_service,
            working_copy_service: working_copy_service.clone(),
            extension_service: extension_service.clone(),
            model_services: model,
        });

        {
            let custom_models = this.custom_models.clone();
            this.base.register(working_copy_file_service.register_working_copy_provider(Rc::new(move |editor_resource: &URI| {
                let mut matched_working_copies: Vec<Rc<dyn IWorkingCopy>> = Vec::new();

                let models: Vec<Rc<MainThreadCustomEditorModel>> = custom_models.borrow().iter().filter_map(Weak::upgrade).collect();
                for working_copy in working_copy_service.working_copies() {
                    // `workingCopy instanceof MainThreadCustomEditorModel`
                    let model = models.iter().find(|model| id_of(&working_copy) == id_of(model));
                    if let Some(model) = model
                        && ext_uri().is_equal_or_parent(editor_resource, &model.editor_resource, false, false)
                    {
                        matched_working_copies.push(working_copy.clone());
                    }
                }
                matched_working_copies
            })));
        }

        // This reviver's only job is to activate custom editor extensions.
        this.base.register(webview_workbench_service.register_resolver(Rc::new(ActivationResolver { extension_service })));

        // Working copy operations
        {
            let weak = this.this.clone();
            this.base.register(working_copy_file_service.on_will_run_working_copy_file_operation().listen(move |e| {
                if let Some(this) = weak.upgrade() {
                    this.on_will_run_working_copy_file_operation(e);
                }
            }));
        }

        this
    }

    fn this(&self) -> Rc<Self> {
        self.this.upgrade().expect("o MainThreadCustomEditors está vivo enquanto um método dele roda")
    }

    fn register_editor_provider(
        &self,
        model_type: CustomEditorModelType,
        extension: WebviewExtensionDescription,
        view_type: &str,
        options: WebviewPanelOptionsDto,
        capabilities: CustomEditorProviderCapabilities,
        supports_multiple_editors_per_document: bool,
        serialize_buffers_for_post_message: bool,
    ) {
        if self.editor_providers.has(&view_type.to_owned()) {
            errors::throw(Error::new(format!("Provider for {view_type} already registered")));
        }

        let disposables = DisposableStore::new();

        disposables.add(self.custom_editor_service.register_custom_editor_capabilities(
            view_type,
            CustomEditorCapabilities {
                supports_multiple_editors_per_document: Some(supports_multiple_editors_per_document),
                is_text_editor: Some(model_type == CustomEditorModelType::Text),
                supports_inline_diff: capabilities.supports_inline_diff,
                supports_side_by_side_diff: capabilities.supports_side_by_side_diff,
            },
        ));

        disposables.add(self.webview_workbench_service.register_resolver(Rc::new(CustomEditorResolver {
            owner: self.this.clone(),
            model_type,
            extension,
            view_type: view_type.to_owned(),
            options,
            capabilities,
            serialize_buffers_for_post_message,
            provider_disposables: Rc::downgrade(&disposables),
        })));

        self.editor_providers.set(view_type.to_owned(), disposables, false);
    }

    fn get_or_add_webview_input(&self, webview_input: &Rc<WebviewInput>, serialize_buffers_for_post_message: bool) -> String {
        let existing = self
            .webview_handles
            .borrow()
            .iter()
            .find(|(input, _)| input.upgrade().is_some_and(|input| Rc::ptr_eq(&input, webview_input)))
            .map(|(_, handle)| handle.clone());
        if let Some(handle) = existing {
            return handle;
        }

        let handle = generate_uuid();
        self.webview_handles.borrow_mut().push((Rc::downgrade(webview_input), handle.clone()));
        self.main_thread_webview_panels.add_webview_input(&handle, webview_input.clone(), MainThreadWebviewOptions { serialize_buffers_for_post_message });
        handle
    }

    /// `asCanonicalUri` quando o modelo é de texto.
    fn canonical_resource(&self, model_type: CustomEditorModelType, resource: &URI) -> URI {
        if model_type == CustomEditorModelType::Text { self.uri_identity_service.as_canonical_uri(resource) } else { resource.clone() }
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_custom_text_editor_navigation(
        &self,
        input: &Rc<WebviewInput>,
        handle: &str,
        view_type: &str,
        resource: &URI,
        model: Rc<dyn ICustomEditorModel>,
        token: &CancellationToken,
        provider_disposables: Rc<DisposableStore>,
    ) -> Promise<()> {
        let Some(text_editor_model) = model.as_any().downcast_ref::<CustomTextEditorModel>().map(|model| model.text_editor_model()) else {
            return Promise::resolved(());
        };
        let this = self.this();
        let lifetime = Rc::new(CancellationTokenSource::new(Some(token)));
        let pending = Rc::new(RefCell::new(PendingNavigation::default()));
        let handle = handle.to_owned();
        let view_type = view_type.to_owned();
        let resource = resource.clone();
        let input = input.clone();
        this.pending_navigation.borrow_mut().insert(handle.clone(), pending.clone());

        let cleanup = DisposableStore::new();
        let clear: Rc<dyn Fn()> = {
            let cleanup = cleanup.clone();
            Rc::new(move || cleanup.dispose())
        };
        {
            let (this, lifetime, handle, input) = (this.clone(), lifetime.clone(), handle.clone(), input.clone());
            let (weak_cleanup, weak_providers) = (Rc::downgrade(&cleanup), Rc::downgrade(&provider_disposables));
            cleanup.add(to_disposable(move || {
                if let (Some(providers), Some(cleanup)) = (weak_providers.upgrade(), weak_cleanup.upgrade()) {
                    let cleanup: DisposableRef = cleanup;
                    providers.delete(&cleanup);
                }
                lifetime.dispose_with(true);
                this.pending_navigation.borrow_mut().remove(&handle);
                this.navigation.delete_and_dispose(&handle);
                if let Some(custom) = input.kind_as::<CustomEditorInput>()
                    && custom.navigation().is_some()
                {
                    custom.set_navigation(None);
                    this.outline_service.notify_outline_changed();
                }
            }));
        }
        {
            let clear = clear.clone();
            cleanup.add(EditorInput::on_will_dispose(&*input).listen(move |_| clear()));
            let clear = clear.clone();
            cleanup.add(input.webview().on_did_dispose().listen(move |_| clear()));
        }
        provider_disposables.add(cleanup);

        async_fn(async move {
            let resolved = this.proxy_custom_editors.rpc_resolve_custom_text_editor_navigation(&handle, &view_type, &resource, lifetime.token());
            let result = race_cancellation(&resolved, &lifetime.token()).await;
            match result {
                Ok(result) => {
                    let result = result.flatten();
                    if result.is_none() || lifetime.token().is_cancellation_requested() || EditorInput::is_disposed(&*input) {
                        this.proxy_custom_editors.rpc_dispose_custom_text_editor_navigation(&handle);
                        clear();
                    } else if let Some(result) = result {
                        let selection = {
                            let pending = pending.borrow();
                            if pending.changed { pending.selection } else { result.selection }
                        };
                        let focus: Rc<dyn Fn()> = {
                            let (weak_input, group_service) = (Rc::downgrade(&input), this.editor_group_service.clone());
                            Rc::new(move || {
                                let Some(input) = weak_input.upgrade() else {
                                    return;
                                };
                                let group = input.group().and_then(|group| group_service.get_group(group));
                                if let Some(group) = group
                                    && group.active_editor().is_some_and(|active| id_of(&active) == id_of(&input))
                                {
                                    group.focus();
                                }
                            })
                        };
                        let navigation = MainThreadCustomTextEditorNavigation::new(text_editor_model, selection, handle.clone(), this.proxy_custom_editors.clone(), focus);
                        this.navigation.set(handle.clone(), navigation.clone(), false);
                        if let Some(custom) = input.kind_as::<CustomEditorInput>() {
                            custom.set_navigation(Some(navigation));
                        }
                        this.outline_service.notify_outline_changed();
                    }
                }
                Err(error) => {
                    clear();
                    if !is_cancellation_error(&error) {
                        on_unexpected_error(&error);
                    }
                }
            }
            this.pending_navigation.borrow_mut().remove(&handle);
            Ok(())
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_custom_editor_side_by_side_diff(
        &self,
        model_type: CustomEditorModelType,
        input: &Rc<WebviewInput>,
        handle: &str,
        view_type: &str,
        init_data: CustomEditorDiffInitData,
        position: EditorGroupColumn,
        cancellation: &CancellationToken,
    ) -> Promise<()> {
        let Some(CustomEditorWebviewInput::SideBySide(side_by_side)) = CustomEditorWebviewInput::of(input) else {
            return Promise::resolved(());
        };
        let diff_id = side_by_side.diff_id().to_owned();
        let side = side_by_side.side();
        let original_resource = side_by_side.original_resource().clone();
        let modified_resource = side_by_side.modified_resource().clone();
        let this = self.this();

        let pending = {
            let existing = self.pending_side_by_side_diff_resolutions.borrow().get(&diff_id).cloned();
            existing.unwrap_or_else(|| {
                let pending = Rc::new(PendingCustomEditorSideBySideDiffResolution {
                    sides: RefCell::new(PendingSideBySideSides::default()),
                    promise: DeferredPromise::new(),
                    cancellation: CancellationTokenSource::new(None),
                    disposables: DisposableStore::new(),
                });
                self.pending_side_by_side_diff_resolutions.borrow_mut().insert(diff_id.clone(), pending.clone());
                pending
            })
        };

        let cleanup: Rc<dyn Fn()> = {
            let (this, diff_id, pending) = (this.clone(), diff_id.clone(), pending.clone());
            Rc::new(move || {
                this.pending_side_by_side_diff_resolutions.borrow_mut().remove(&diff_id);
                pending.disposables.dispose();
                pending.cancellation.dispose_with(false);
            })
        };

        {
            let (pending, cleanup) = (pending.clone(), cleanup.clone());
            pending.disposables.clone().add(cancellation.on_cancellation_requested().listen(move |_| {
                pending.cancellation.cancel();
                if !pending.sides.borrow().started {
                    let _ = pending.promise.cancel();
                    cleanup();
                }
            }));
        }

        let side_data = CustomEditorSideBySideDiffData { handle: handle.to_owned(), init_data };
        match side {
            CustomEditorSideBySideDiffSide::Original => pending.sides.borrow_mut().original = Some(side_data),
            CustomEditorSideBySideDiffSide::Modified => pending.sides.borrow_mut().modified = Some(side_data),
        }

        let ready = {
            let mut sides = pending.sides.borrow_mut();
            if sides.original.is_some() && sides.modified.is_some() && !sides.started {
                sides.started = true;
                Some((sides.original.clone().expect("original"), sides.modified.clone().expect("modified")))
            } else {
                None
            }
        };
        if let Some((original, modified)) = ready {
            let original_resource = self.canonical_resource(model_type, &original_resource);
            let modified_resource = self.canonical_resource(model_type, &modified_resource);
            let view_type = view_type.to_owned();
            let (proxy, pending_for_work) = (self.proxy_custom_editors.clone(), pending.clone());
            let work = async_fn(async move {
                let result = proxy
                    .rpc_resolve_custom_editor_side_by_side_diff(
                        &original_resource,
                        &modified_resource,
                        CustomEditorSideBySideDiffWebviewHandles { original: original.handle, modified: modified.handle },
                        &view_type,
                        CustomEditorSideBySideDiffInitData { original: original.init_data, modified: modified.init_data },
                        position,
                        pending_for_work.cancellation.token(),
                    )
                    .await;
                cleanup();
                result
            });
            let _ = pending.promise.settle_with(&work);
        }

        pending.promise.promise.clone()
    }

    #[allow(clippy::too_many_arguments)]
    fn get_or_create_custom_editor_model(
        &self,
        model_type: CustomEditorModelType,
        resource: &URI,
        view_type: &str,
        backup_id: Option<String>,
        cancellation: &CancellationToken,
    ) -> Promise<CustomEditorModelReference> {
        let models = self.custom_editor_service.models();
        if let Some(existing_model) = models.try_retain(resource, view_type) {
            return existing_model;
        }

        let model: Promise<Rc<dyn ICustomEditorModel>> = match model_type {
            CustomEditorModelType::Text => {
                let services = CustomTextEditorModelServices {
                    text_file_service: self.model_services.text_file_service.clone(),
                    label_service: self.model_services.label_service.clone(),
                    extension_service: self.extension_service.clone(),
                };
                CustomTextEditorModel::create(self.model_services.text_model_service.clone(), services, view_type, resource)
                    .then(|model| -> Rc<dyn ICustomEditorModel> { model })
            }
            CustomEditorModelType::Custom => {
                let panels = self.main_thread_webview_panels.clone();
                let resource_for_editors = resource.clone();
                let get_editors: GetEditors = Rc::new(move || {
                    panels
                        .webview_inputs()
                        .into_iter()
                        .filter(|editor| match CustomEditorWebviewInput::of(editor) {
                            Some(CustomEditorWebviewInput::Input(input)) => is_equal(input.editor_resource(), &resource_for_editors),
                            Some(CustomEditorWebviewInput::Diff(diff)) => {
                                is_equal(diff.original_resource(), &resource_for_editors) || is_equal(diff.modified_resource(), &resource_for_editors)
                            }
                            Some(CustomEditorWebviewInput::SideBySide(_)) => {
                                EditorInput::resource(&***editor).is_some_and(|editor_resource| is_equal(&editor_resource, &resource_for_editors))
                            }
                            None => false,
                        })
                        .collect()
                });
                let custom_models = self.custom_models.clone();
                MainThreadCustomEditorModel::create(
                    self.this(),
                    view_type,
                    resource,
                    backup_id,
                    get_editors,
                    cancellation.clone(),
                )
                .then(move |model| -> Rc<dyn ICustomEditorModel> {
                    custom_models.borrow_mut().push(Rc::downgrade(&model));
                    model
                })
            }
        };

        match errors::catch(|| models.add(resource, view_type, model)) {
            Ok(reference) => reference,
            Err(error) => Promise::rejected(error),
        }
    }

    /// `getCustomEditorModel(resourceComponents, viewType)`.
    fn get_custom_editor_model(&self, resource: &URI, view_type: &str) -> Promise<Rc<dyn ICustomEditorModel>> {
        self.custom_editor_service.models().get(resource, view_type).then_result(|result| {
            let model = result?.filter(|model| model.as_any().is::<MainThreadCustomEditorModel>());
            model.ok_or_else(|| Error::new("Could not find model for webview editor"))
        })
    }

    //#region Working Copy
    fn on_will_run_working_copy_file_operation(&self, e: &WorkingCopyFileEvent) {
        if e.operation != FileOperation::Move {
            return;
        }
        let this = self.this();
        let files = e.files.clone();
        e.wait_until(async_fn(async move {
            let mut models: Vec<Rc<dyn ICustomEditorModel>> = Vec::new();
            for file in files {
                if let Some(source) = file.source {
                    models.extend(this.custom_editor_service.models().get_all_models(&source).await?);
                }
            }
            for model in models {
                if let Some(model) = model.as_any().downcast_ref::<MainThreadCustomEditorModel>()
                    && model.dirty()
                {
                    let working_copy = model.backup(&CancellationToken::None).await?;
                    if let Some(meta) = working_copy.meta {
                        // The meta is always the custom document backup data
                        this.editor_rename_backups.borrow_mut().insert(model.editor_resource.to_string(false).to_string_lossy(), meta);
                    }
                }
            }
            Ok(())
        }));
    }
    //#endregion
}

impl IDisposable for MainThreadCustomEditors {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl MainThreadCustomEditorsShape for MainThreadCustomEditors {
    fn rpc_on_did_change_custom_text_editor_selection(&self, handle: &str, selection: Option<Selection>) {
        if let Some(navigation) = self.navigation.get(&handle.to_owned()) {
            navigation.update_selection(selection);
        }
        let pending = self.pending_navigation.borrow().get(handle).cloned();
        if let Some(pending) = pending {
            let mut pending = pending.borrow_mut();
            pending.changed = true;
            pending.selection = selection;
        }
    }

    fn rpc_register_text_editor_provider(
        &self,
        extension: WebviewExtensionDescriptionDto,
        view_type: &str,
        options: WebviewPanelOptionsDto,
        capabilities: CustomEditorProviderCapabilities,
        serialize_buffers_for_post_message: bool,
    ) {
        self.register_editor_provider(
            CustomEditorModelType::Text,
            revive_webview_extension(&extension),
            view_type,
            options,
            capabilities,
            true,
            serialize_buffers_for_post_message,
        );
    }

    fn rpc_register_custom_editor_provider(
        &self,
        extension: WebviewExtensionDescriptionDto,
        view_type: &str,
        options: WebviewPanelOptionsDto,
        capabilities: CustomEditorProviderCapabilities,
        supports_multiple_editors_per_document: bool,
        serialize_buffers_for_post_message: bool,
    ) {
        self.register_editor_provider(
            CustomEditorModelType::Custom,
            revive_webview_extension(&extension),
            view_type,
            options,
            capabilities,
            supports_multiple_editors_per_document,
            serialize_buffers_for_post_message,
        );
    }

    fn rpc_unregister_editor_provider(&self, view_type: &str) {
        if !self.editor_providers.has(&view_type.to_owned()) {
            errors::throw(Error::new(format!("No provider for {view_type} registered")));
        }

        self.editor_providers.delete_and_dispose(&view_type.to_owned());

        self.custom_editor_service.models().dispose_all_models_for_view(view_type);
    }

    fn rpc_on_did_edit(&self, resource: &URI, view_type: &str, edit_id: u32, label: Option<&str>) {
        let label = label.map(str::to_owned);
        let _ = self
            .get_custom_editor_model(resource, view_type)
            .then_result(move |result| {
                let model = result?;
                match model.as_any().downcast_ref::<MainThreadCustomEditorModel>() {
                    Some(model) => model.push_edit(edit_id, label.as_deref()),
                    None => Err(Error::new("Could not find model for webview editor")),
                }
            })
            .catch(|error| on_unexpected_error(&error));
    }

    fn rpc_on_content_change(&self, resource: &URI, view_type: &str) {
        let _ = self
            .get_custom_editor_model(resource, view_type)
            .then(|model| {
                if let Some(model) = model.as_any().downcast_ref::<MainThreadCustomEditorModel>() {
                    model.change_content();
                }
            })
            .catch(|error| on_unexpected_error(&error));
    }
}

/// O resolvedor que só ativa as extensões de editor personalizado (`canResolve` dispara `onCustomEditor:<viewType>` e devolve
/// `false`).
struct ActivationResolver {
    extension_service: Rc<dyn IExtensionService>,
}

impl WebviewResolver for ActivationResolver {
    fn can_resolve(&self, webview: &Rc<WebviewInput>) -> bool {
        if CustomEditorWebviewInput::of(webview).is_some() {
            let _ = self.extension_service.activate_by_event(&format!("onCustomEditor:{}", webview.view_type), None);
        }
        false
    }

    fn resolve_webview(&self, _webview: Rc<WebviewInput>, _token: CancellationToken) -> Promise<()> {
        errors::throw(Error::new("not implemented"))
    }
}

/// O resolvedor que `registerEditorProvider` registra para o tipo de visão.
struct CustomEditorResolver {
    owner: Weak<MainThreadCustomEditors>,
    model_type: CustomEditorModelType,
    extension: WebviewExtensionDescription,
    view_type: String,
    options: WebviewPanelOptionsDto,
    capabilities: CustomEditorProviderCapabilities,
    serialize_buffers_for_post_message: bool,
    provider_disposables: Weak<DisposableStore>,
}

impl WebviewResolver for CustomEditorResolver {
    fn can_resolve(&self, webview_input: &Rc<WebviewInput>) -> bool {
        CustomEditorWebviewInput::of(webview_input).is_some() && webview_input.view_type == self.view_type
    }

    fn resolve_webview(&self, webview_input: Rc<WebviewInput>, cancellation: CancellationToken) -> Promise<()> {
        let Some(this) = self.owner.upgrade() else {
            return Promise::resolved(());
        };
        let model_type = self.model_type;
        let extension = self.extension.clone();
        let view_type = self.view_type.clone();
        let options = self.options;
        let capabilities = self.capabilities;
        let serialize_buffers_for_post_message = self.serialize_buffers_for_post_message;
        let provider_disposables = self.provider_disposables.upgrade();

        async_fn(async move {
            let Some(kind) = CustomEditorWebviewInput::of(&webview_input) else {
                return Ok(());
            };

            let webview = webview_input.webview();
            webview.set_origin(&this.webview_origin_store.get_origin(&view_type, &extension.id));

            let handle = this.get_or_add_webview_input(&webview_input, serialize_buffers_for_post_message);
            webview.set_options(revive_webview_options(&options));
            webview.set_extension(Some(extension.clone()));

            let resource = match kind {
                CustomEditorWebviewInput::Diff(diff) => diff.modified_resource().clone(),
                _ => EditorInput::resource(&*webview_input).expect("a entrada de editor personalizado tem recurso"),
            };

            // If there's an old resource this was a move and we must resolve the backup at the same time as the webview
            // This is because the backup must be ready upon model creation, and the input resolve method comes after
            let mut backup_id: Option<String> = None;
            if let CustomEditorWebviewInput::Input(input) = kind {
                backup_id = input.backup_id();
                if let Some(old_resource) = &input.old_resource
                    && input.backup_id().is_none()
                {
                    let key = old_resource.to_string(false).to_string_lossy();
                    let backup = this.editor_rename_backups.borrow_mut().remove(&key);
                    backup_id = backup.and_then(|backup| CustomDocumentBackupData::backup_id_of(&backup));
                }
            }

            let mut model_ref: Option<CustomEditorModelReference> = None;
            let additional_model_refs = DisposableStore::new();
            let created: Result<(), Error> = async {
                model_ref = Some(this.get_or_create_custom_editor_model(model_type, &resource, &view_type, backup_id.clone(), &cancellation).await?);
                match kind {
                    CustomEditorWebviewInput::Diff(diff) if !is_equal(diff.original_resource(), &resource) => {
                        let additional = this.get_or_create_custom_editor_model(model_type, diff.original_resource(), &view_type, None, &cancellation).await?;
                        additional_model_refs.add(Rc::new(additional));
                    }
                    CustomEditorWebviewInput::SideBySide(side_by_side) if model_type == CustomEditorModelType::Text => {
                        let other_resource =
                            if side_by_side.side() == CustomEditorSideBySideDiffSide::Original { side_by_side.modified_resource() } else { side_by_side.original_resource() };
                        if !is_equal(other_resource, &resource) {
                            let additional = this.get_or_create_custom_editor_model(model_type, other_resource, &view_type, None, &cancellation).await?;
                            additional_model_refs.add(Rc::new(additional));
                        }
                    }
                    _ => {}
                }
                Ok(())
            }
            .await;
            if let Err(error) = created {
                additional_model_refs.dispose();
                if let Some(model_ref) = &model_ref {
                    model_ref.dispose();
                }
                // Let the editor show the error. Resolving again, for example with Try Again, creates the document again
                return Err(error);
            }

            let Some(model_ref) = model_ref else {
                additional_model_refs.dispose();
                return Ok(());
            };
            let resolved_model_ref = Rc::new(RefCell::new(model_ref));

            if cancellation.is_cancellation_requested() {
                additional_model_refs.dispose();
                resolved_model_ref.borrow().dispose();
                return Ok(());
            }

            let dispose_model_refs: Rc<dyn Fn()> = {
                let (additional_model_refs, resolved_model_ref) = (additional_model_refs.clone(), resolved_model_ref.clone());
                Rc::new(move || {
                    additional_model_refs.dispose();

                    // If the model is still dirty, make sure we have time to save it
                    let model = resolved_model_ref.borrow().object.clone();
                    if model.is_dirty() {
                        let sub: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
                        let (listener_sub, resolved_model_ref) = (sub.clone(), resolved_model_ref.clone());
                        let listener = model.on_did_change_dirty().listen(move |_| {
                            let model_ref = resolved_model_ref.borrow().clone();
                            if !model_ref.object.is_dirty() {
                                let sub = listener_sub.borrow_mut().take();
                                if let Some(sub) = sub {
                                    sub.dispose();
                                }
                                model_ref.dispose();
                            }
                        });
                        *sub.borrow_mut() = Some(listener);
                        return;
                    }

                    resolved_model_ref.borrow().dispose();
                })
            };

            let dispose_sub: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
            let input_dispose_sub: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
            let on_disposed: Rc<dyn Fn()> = {
                let (dispose_sub, input_dispose_sub, dispose_model_refs) = (dispose_sub.clone(), input_dispose_sub.clone(), dispose_model_refs.clone());
                Rc::new(move || {
                    for sub in [&dispose_sub, &input_dispose_sub] {
                        let sub = sub.borrow_mut().take();
                        if let Some(sub) = sub {
                            sub.dispose();
                        }
                    }
                    dispose_model_refs();
                })
            };
            {
                let on_disposed = on_disposed.clone();
                *dispose_sub.borrow_mut() = Some(webview.on_did_dispose().listen(move |_| on_disposed()));
            }
            // Also listen for when the input is disposed (e.g., during SaveAs when the webview is transferred to a new editor).
            // In this case, webview.onDidDispose won't fire because the webview is reused.
            *input_dispose_sub.borrow_mut() = Some(EditorInput::on_will_dispose(&*webview_input).listen(move |_| on_disposed()));

            if let CustomEditorWebviewInput::Input(input) = kind
                && capabilities.supports_move == Some(true)
            {
                let (this, resolved_model_ref, view_type, handle, model_type) = (this.clone(), resolved_model_ref.clone(), view_type.clone(), handle.clone(), model_type);
                input.on_move(Rc::new(move |new_resource: URI| {
                    let (this, resolved_model_ref, view_type, handle) = (this.clone(), resolved_model_ref.clone(), view_type.clone(), handle.clone());
                    let moved = async_fn(async move {
                        let old_model = resolved_model_ref.borrow().clone();
                        let new_model_ref = this.get_or_create_custom_editor_model(model_type, &new_resource, &view_type, None, &CancellationToken::None).await?;
                        *resolved_model_ref.borrow_mut() = new_model_ref;
                        let _ = this.proxy_custom_editors.rpc_on_move_custom_editor(&handle, &new_resource, &view_type).catch(|error| on_unexpected_error(&error));
                        old_model.dispose();
                        Ok(())
                    });
                    let _ = moved.catch(|error| on_unexpected_error(&error));
                }));
            }

            let resolved: Result<(), Error> = async {
                let init_data = CustomEditorResolveInitData {
                    title: EditorInput::get_title(&*webview_input, None),
                    content_options: webview_content_options_to_dto(&webview.content_options()),
                    options: webview_options_to_panel_options_dto(&webview.options()),
                    active: matches!(this.editor_service.active_editor(), Some(EditorInputOrUntyped::Typed(active)) if id_of(&active) == id_of(&webview_input)),
                };
                let position = editor_group_to_column(&*this.editor_group_service, &GroupRef::Id(webview_input.group().unwrap_or(0)));

                match kind {
                    CustomEditorWebviewInput::Diff(diff) => {
                        let original_resource = this.canonical_resource(model_type, diff.original_resource());
                        let modified_resource = this.canonical_resource(model_type, diff.modified_resource());
                        this.proxy_custom_editors
                            .rpc_resolve_custom_editor_inline_diff(&original_resource, &modified_resource, &handle, &view_type, init_data, position, cancellation.clone())
                            .await?;
                    }
                    CustomEditorWebviewInput::SideBySide(_) => {
                        this.resolve_custom_editor_side_by_side_diff(model_type, &webview_input, &handle, &view_type, init_data, position, &cancellation).await?;
                    }
                    CustomEditorWebviewInput::Input(_) => {
                        let actual_resource = this.canonical_resource(model_type, &resource);
                        this.proxy_custom_editors.rpc_resolve_custom_editor(&actual_resource, &handle, &view_type, init_data, position, cancellation.clone()).await?;
                        if model_type == CustomEditorModelType::Text
                            && capabilities.supports_navigation == Some(true)
                            && !cancellation.is_cancellation_requested()
                            && !EditorInput::is_disposed(&*webview_input)
                            && let Some(provider_disposables) = &provider_disposables
                        {
                            let model = resolved_model_ref.borrow().object.clone();
                            this.resolve_custom_text_editor_navigation(&webview_input, &handle, &view_type, &actual_resource, model, &cancellation, provider_disposables.clone())
                                .await?;
                        }
                    }
                }
                Ok(())
            }
            .await;
            if let Err(error) = resolved {
                on_unexpected_error(&error);
                webview.set_html(&this.main_thread_webview.get_webview_resolved_failed_content(&view_type));
                additional_model_refs.dispose();
                resolved_model_ref.borrow().dispose();
                return Ok(());
            }

            Ok(())
        })
    }
}

#[derive(Clone)]
enum HotExitState {
    Allowed,
    NotAllowed,
    /// `HotExitState.Pending`: o `id` é a identidade da instância (o `this._hotExitState === pendingState`).
    Pending { id: u64, operation: CancelablePromise<String> },
}

impl HotExitState {
    fn is_pending(&self, expected_id: u64) -> bool {
        matches!(self, HotExitState::Pending { id, .. } if *id == expected_id)
    }
}

pub struct MainThreadCustomEditorModel {
    this: Weak<MainThreadCustomEditorModel>,
    base: Rc<Disposable>,
    resource_working_copy: Rc<ResourceWorkingCopy>,

    from_backup: Cell<bool>,
    hot_exit_state: RefCell<HotExitState>,
    next_hot_exit_id: Cell<u64>,
    backup_id: RefCell<Option<String>>,

    current_edit_index: Cell<i64>,
    save_point: Cell<i64>,
    edits: RefCell<Vec<u32>>,
    is_dirty_from_content_change: Cell<bool>,

    ongoing_save: RefCell<Option<CancelablePromise<()>>>,

    on_did_change_dirty: Rc<Emitter<()>>,
    on_did_change_content: Rc<Emitter<()>>,
    on_did_save: Rc<Emitter<WorkingCopySaveEvent>>,

    proxy: Rc<dyn ExtHostCustomEditorsShape>,
    view_type: String,
    editor_resource: URI,
    editable: bool,
    get_editors: GetEditors,
    services: CustomEditorModelServices,
}

impl MainThreadCustomEditorModel {
    /// `create(instantiationService, proxy, viewType, resource, options, untitledTextEditorService, getEditors, cancellation)`.
    fn create(
        owner: Rc<MainThreadCustomEditors>,
        view_type: &str,
        resource: &URI,
        backup_id: Option<String>,
        get_editors: GetEditors,
        cancellation: CancellationToken,
    ) -> Promise<Rc<MainThreadCustomEditorModel>> {
        let view_type = view_type.to_owned();
        let resource = resource.clone();
        async_fn(async move {
            let editors = get_editors();
            let primary_custom_editor_input = editors.iter().find_map(|editor| editor.kind_as::<CustomEditorInput>());
            let untitled_document_data = primary_custom_editor_input.and_then(|input| input.untitled_document_data());
            let CreateCustomDocumentResult { editable } = owner
                .proxy_custom_editors
                .rpc_create_custom_document(&resource, &view_type, backup_id.clone(), untitled_document_data.clone(), cancellation)
                .await?;

            // Now that the extension has received the untitledDocumentData, revert
            // the untitled text model so it is no longer tracked as a separate dirty
            // working copy (avoids double-dirty prompts, see #125293).
            if untitled_document_data.is_some() && *resource.scheme() == schemas::UNTITLED
                && let Some(untitled_model) = owner.untitled_text_editor_service.get(&resource)
            {
                let _ = IWorkingCopy::revert(&*untitled_model, None).catch(|error| on_unexpected_error(&error));
            }

            Ok(Self::new(
                owner.proxy_custom_editors.clone(),
                &view_type,
                resource,
                backup_id.is_some(),
                editable,
                untitled_document_data.is_some(),
                get_editors,
                owner.model_services.clone(),
                owner.working_copy_service.clone(),
                owner.extension_service.clone(),
            ))
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        proxy: Rc<dyn ExtHostCustomEditorsShape>,
        view_type: &str,
        editor_resource: URI,
        from_backup: bool,
        editable: bool,
        start_dirty: bool,
        get_editors: GetEditors,
        services: CustomEditorModelServices,
        working_copy_service: Rc<dyn IWorkingCopyService>,
        extension_service: Rc<dyn IExtensionService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_dirty = Emitter::new();
        let on_did_change_content = Emitter::new();
        let on_did_save = Emitter::new();
        base.register(on_did_change_dirty.clone());
        base.register(on_did_change_content.clone());
        base.register(on_did_save.clone());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            resource_working_copy: ResourceWorkingCopy::new(Self::to_working_copy_resource(view_type, &editor_resource), services.file_service.clone()),
            from_backup: Cell::new(from_backup),
            hot_exit_state: RefCell::new(HotExitState::Allowed),
            next_hot_exit_id: Cell::new(0),
            backup_id: RefCell::new(None),
            current_edit_index: Cell::new(-1),
            save_point: Cell::new(-1),
            edits: RefCell::new(Vec::new()),
            // Normally means we're re-opening an untitled file (set this before registering the working copy
            // so that dirty state is correct when first queried).
            is_dirty_from_content_change: Cell::new(start_dirty),
            ongoing_save: RefCell::new(None),
            on_did_change_dirty,
            on_did_change_content,
            on_did_save,
            proxy,
            view_type: view_type.to_owned(),
            editor_resource,
            editable,
            get_editors,
            services,
        });

        if editable {
            let working_copy: Rc<dyn IWorkingCopy> = this.clone();
            this.base.register(working_copy_service.register_working_copy(working_copy));

            let weak = this.this.clone();
            this.base.register(extension_service.on_will_stop().listen(move |e| {
                let name = weak.upgrade().map(|model| IWorkingCopy::name(&*model)).unwrap_or_default();
                e.veto(
                    Promise::resolved(true),
                    &localize("vetoExtHostRestart", "An extension provided editor for '{0}' is still open that would close otherwise.", &[name.as_str()]),
                );
            }));
        }

        this
    }

    fn this(&self) -> Rc<Self> {
        self.this.upgrade().expect("o modelo está vivo enquanto um método dele roda")
    }

    //#region IWorkingCopy

    /// Make sure each custom editor has a unique resource for backup and edits (`toWorkingCopyResource`).
    fn to_working_copy_resource(view_type: &str, resource: &URI) -> URI {
        let authority: String = view_type.chars().map(|char| if char.is_ascii_alphanumeric() || char == '-' || char == '_' { char } else { '-' }).collect();
        let without_query_and_fragment = resource.with(&UriChange { query: Field::Clear, fragment: Field::Clear, ..UriChange::default() });
        let path = format!("/{}", multibyte_aware_btoa(without_query_and_fragment.to_string(true).units()));
        URI::from(
            &UriComponents {
                scheme: Some(JsString::from(schemas::VSCODE_CUSTOM_EDITOR)),
                authority: Some(JsString::from(authority)),
                path: Some(JsString::from(path)),
                query: Some(stringify(&Marshalled::Uri(resource.clone()))),
                ..UriComponents::default()
            },
            false,
        )
    }

    fn is_untitled(&self) -> bool {
        *self.editor_resource.scheme() == schemas::UNTITLED
    }

    /// `isDirty()`.
    pub fn dirty(&self) -> bool {
        if self.is_dirty_from_content_change.get() {
            return true;
        }
        if !self.edits.borrow().is_empty() {
            return self.save_point.get() != self.current_edit_index.get();
        }
        self.from_backup.get()
    }

    //#endregion

    /// `editorResource`.
    pub fn editor_resource(&self) -> &URI {
        &self.editor_resource
    }

    /// `pushEdit(editId, label)`: lança (`Document is not editable`) quando o documento não é editável.
    pub fn push_edit(&self, edit_id: u32, label: Option<&str>) -> Result<(), Error> {
        if !self.editable {
            return Err(Error::new("Document is not editable"));
        }

        self.change(|| {
            self.splice_edits(Some(edit_id));
            self.current_edit_index.set(self.edits.borrow().len() as i64 - 1);
        });

        let (undo_this, redo_this) = (self.this.clone(), self.this.clone());
        self.services.undo_redo_service.push_element(ResourceUndoRedoElement {
            resource: self.editor_resource.clone(),
            label: label.map_or_else(|| localize("defaultEditLabel", "Edit", &[]), str::to_owned),
            code: "undoredo.customEditorEdit".to_owned(),
            confirm_before_undo: None,
            undo: Rc::new(move || undo_this.upgrade().map_or_else(|| Promise::resolved(()), |model| model.undo())),
            redo: Rc::new(move || redo_this.upgrade().map_or_else(|| Promise::resolved(()), |model| model.redo())),
        });
        Ok(())
    }

    /// `changeContent()`.
    pub fn change_content(&self) {
        self.change(|| self.is_dirty_from_content_change.set(true));
    }

    fn undo(&self) -> Promise<()> {
        if !self.editable {
            return Promise::resolved(());
        }

        if self.current_edit_index.get() < 0 {
            // nothing to undo
            return Promise::resolved(());
        }

        let undone_edit = self.edits.borrow()[self.current_edit_index.get() as usize];
        self.change(|| self.current_edit_index.set(self.current_edit_index.get() - 1));
        self.proxy.rpc_undo(&self.editor_resource, &self.view_type, undone_edit, self.dirty())
    }

    fn redo(&self) -> Promise<()> {
        if !self.editable {
            return Promise::resolved(());
        }

        if self.current_edit_index.get() >= self.edits.borrow().len() as i64 - 1 {
            // nothing to redo
            return Promise::resolved(());
        }

        let redone_edit = self.edits.borrow()[(self.current_edit_index.get() + 1) as usize];
        self.change(|| self.current_edit_index.set(self.current_edit_index.get() + 1));
        self.proxy.rpc_redo(&self.editor_resource, &self.view_type, redone_edit, self.dirty())
    }

    fn splice_edits(&self, edit_to_insert: Option<u32>) {
        let removed_edits: Vec<u32> = {
            let mut edits = self.edits.borrow_mut();
            let start = ((self.current_edit_index.get() + 1) as usize).min(edits.len());
            let removed: Vec<u32> = edits.drain(start..).collect();
            if let Some(edit_to_insert) = edit_to_insert {
                edits.push(edit_to_insert);
            }
            removed
        };

        if !removed_edits.is_empty()
            && let Err(error) = self.proxy.rpc_dispose_edits(&self.editor_resource, &self.view_type, removed_edits)
        {
            on_unexpected_error(&error);
        }
    }

    fn change(&self, make_edit: impl FnOnce()) {
        let was_dirty = self.dirty();
        make_edit();
        self.on_did_change_content.fire(());

        if self.dirty() != was_dirty {
            self.on_did_change_dirty.fire(());
        }
    }

    /// `revert(options)`.
    fn revert_document(&self, options: Option<RevertOptions>) -> Promise<()> {
        if !self.editable {
            return Promise::resolved(());
        }

        if self.current_edit_index.get() == self.save_point.get() && !self.is_dirty_from_content_change.get() && !self.from_backup.get() {
            return Promise::resolved(());
        }

        if !options.is_some_and(|options| options.soft == Some(true)) {
            let _ = self.proxy.rpc_revert(&self.editor_resource, &self.view_type, CancellationToken::None).catch(|error| on_unexpected_error(&error));
        }

        self.change(|| {
            self.is_dirty_from_content_change.set(false);
            self.from_backup.set(false);
            self.current_edit_index.set(self.save_point.get());
            self.splice_edits(None);
        });
        Promise::resolved(())
    }

    fn mark_saved(&self) {
        self.change(|| {
            self.is_dirty_from_content_change.set(false);
            self.save_point.set(self.current_edit_index.get());
            self.from_backup.set(false);
        });
    }

    /// `save(options)`.
    fn save_document(&self, options: Option<SaveOptions>) -> Promise<bool> {
        let this = self.this();
        async_fn(async move {
            let result = this.save_custom_editor_document(options.clone()).await?.is_some();

            // Emit Save Event
            if result {
                this.on_did_save.fire(WorkingCopySaveEvent {
                    reason: options.as_ref().and_then(|options| options.reason.clone()),
                    source: options.as_ref().and_then(|options| options.source.clone()),
                });
            }

            Ok(result)
        })
    }

    /// `saveCustomEditor(options)`.
    fn save_custom_editor_document(&self, options: Option<SaveOptions>) -> Promise<Option<URI>> {
        if !self.editable {
            return Promise::resolved(None);
        }

        let this = self.this();
        async_fn(async move {
            if this.is_untitled() {
                let Some(target_uri) = this.suggest_untitled_save_path(options.as_ref()).await? else {
                    return Ok(None);
                };

                this.save_custom_editor_as_document(&this.editor_resource, &target_uri, options).await?;
                return Ok(Some(target_uri));
            }

            let proxy = this.proxy.clone();
            let (resource, view_type) = (this.editor_resource.clone(), this.view_type.clone());
            let save_promise = create_cancelable_promise(move |token| proxy.rpc_on_save(&resource, &view_type, token));
            let previous = this.ongoing_save.borrow_mut().take();
            if let Some(previous) = previous {
                previous.cancel();
            }
            *this.ongoing_save.borrow_mut() = Some(save_promise.clone());

            let result = (*save_promise).clone().await;
            // Make sure we are still doing the same save
            let is_current = this.ongoing_save.borrow().as_ref().is_some_and(|current| current.ptr_eq(&save_promise));
            if result.is_ok() && is_current {
                this.mark_saved();
            }
            if is_current {
                *this.ongoing_save.borrow_mut() = None;
            }
            result?;

            Ok(Some(this.editor_resource.clone()))
        })
    }

    fn suggest_untitled_save_path(&self, options: Option<&SaveOptions>) -> Promise<Option<URI>> {
        if !self.is_untitled() {
            errors::throw(Error::new("Resource is not untitled"));
        }

        let remote_authority = self.services.environment_service.remote_authority();
        let local_resource = to_local_resource(&self.editor_resource, remote_authority.as_deref(), &self.services.path_service.default_uri_scheme());

        self.services.file_dialog_service.pick_file_to_save(&local_resource, options.and_then(|options| options.available_file_systems.clone()))
    }

    /// `saveCustomEditorAs(resource, targetResource, options)`.
    fn save_custom_editor_as_document(&self, resource: &URI, target_resource: &URI, _options: Option<SaveOptions>) -> Promise<bool> {
        let this = self.this();
        let (resource, target_resource) = (resource.clone(), target_resource.clone());
        async_fn(async move {
            if this.editable {
                // A cancelação do `saveAs` não é tratada (como no original).
                let proxy = this.proxy.clone();
                let (editor_resource, view_type, target) = (this.editor_resource.clone(), this.view_type.clone(), target_resource.clone());
                let save_as = create_cancelable_promise(move |token| proxy.rpc_on_save_as(&editor_resource, &view_type, &target, token));
                (*save_as).clone().await?;
                this.mark_saved();
                Ok(true)
            } else {
                // Since the editor is readonly, just copy the file over
                this.services.file_service.copy(&resource, &target_resource, Some(false)).await?;
                Ok(true)
            }
        })
    }

    /// `canHotExit`.
    fn can_hot_exit_value(&self) -> bool {
        self.backup_id.borrow().is_some() && matches!(*self.hot_exit_state.borrow(), HotExitState::Allowed)
    }

    /// `backup(token)`.
    pub fn backup(&self, token: &CancellationToken) -> Promise<WorkingCopyBackup> {
        let this = self.this();
        let token = token.clone();
        async_fn(async move {
            let editors = (this.get_editors)();
            let Some(primary_editor) = editors.first() else {
                return Err(Error::new("No editors found for resource, cannot back up"));
            };

            let webview = primary_editor.webview();
            let mut backup_meta = CustomDocumentBackupData {
                view_type: this.view_type.clone(),
                editor_resource: this.editor_resource.clone(),
                custom_title: primary_editor.get_webview_title(),
                icon_path: primary_editor.icon_path(),
                backup_id: String::new(),
                extension: primary_editor
                    .extension()
                    .map(|extension| CustomDocumentBackupExtension { id: extension.id.value.clone(), location: extension.location.clone() }),
                webview: CustomDocumentBackupWebview { origin: Some(webview.origin()), options: webview.options(), state: webview.state() },
            };

            if !this.editable {
                return Ok(WorkingCopyBackup { meta: Some(backup_meta.to_dynamic()), content: None });
            }

            let previous = this.hot_exit_state.borrow().clone();
            if let HotExitState::Pending { operation, .. } = previous {
                operation.cancel();
            }

            let proxy = this.proxy.clone();
            let (resource_for_backup, view_type_for_backup) = (this.editor_resource.clone(), this.view_type.clone());
            let operation = create_cancelable_promise(move |token| proxy.rpc_backup(&resource_for_backup, &view_type_for_backup, token));
            let pending_id = this.next_hot_exit_id.get();
            this.next_hot_exit_id.set(pending_id + 1);
            *this.hot_exit_state.borrow_mut() = HotExitState::Pending { id: pending_id, operation: operation.clone() };

            let cancel_operation = operation.clone();
            let _ = token.on_cancellation_requested().listen(move |_| cancel_operation.cancel());

            let mut error_message = String::new();
            match (*operation).clone().await {
                Ok(backup_id) => {
                    // Make sure state has not changed in the meantime
                    if this.hot_exit_state.borrow().is_pending(pending_id) {
                        *this.hot_exit_state.borrow_mut() = HotExitState::Allowed;
                        backup_meta.backup_id = backup_id.clone();
                        *this.backup_id.borrow_mut() = Some(backup_id);
                    }
                }
                Err(error) => {
                    if is_cancellation_error(&error) {
                        // This is expected
                        return Err(error);
                    }

                    // Otherwise it could be a real error. Make sure state has not changed in the meantime.
                    if this.hot_exit_state.borrow().is_pending(pending_id) {
                        *this.hot_exit_state.borrow_mut() = HotExitState::NotAllowed;
                    }
                    if !error.message.is_empty() {
                        error_message = error.message;
                    }
                }
            }

            if matches!(*this.hot_exit_state.borrow(), HotExitState::Allowed) {
                return Ok(WorkingCopyBackup { meta: Some(backup_meta.to_dynamic()), content: None });
            }

            Err(Error::new(format!("Cannot backup in this state: {error_message}")))
        })
    }
}

impl IDisposable for MainThreadCustomEditorModel {
    fn dispose(&self) {
        if self.editable {
            self.services.undo_redo_service.remove_elements(&self.editor_resource);
        }

        let _ = self.proxy.rpc_dispose_custom_document(&self.editor_resource, &self.view_type).catch(|error| on_unexpected_error(&error));

        self.base.dispose();
        self.resource_working_copy.dispose();
    }
}

impl IResourceWorkingCopy for MainThreadCustomEditorModel {
    fn resource_working_copy(&self) -> &ResourceWorkingCopy {
        &self.resource_working_copy
    }
}

impl IWorkingCopy for MainThreadCustomEditorModel {
    /// `typeId`: o original deixa o `NO_TYPE_ID` (qualquer valor diferente da string vazia vira semente do backup, e trocá-lo
    /// exige um caminho para os backups que já existem sem ela).
    fn working_copy_type_id(&self) -> String {
        NO_TYPE_ID.to_owned()
    }

    fn resource(&self) -> URI {
        self.resource_working_copy.resource().clone()
    }

    fn name(&self) -> String {
        PathModule::native(&ProcessEnvironment).basename(&self.services.label_service.get_uri_label(&self.editor_resource, &UriLabelOptions::default()), None)
    }

    fn capabilities(&self) -> WorkingCopyCapabilities {
        if self.is_untitled() { WorkingCopyCapabilities::UNTITLED } else { WorkingCopyCapabilities::NONE }
    }

    fn on_did_change_dirty(&self) -> Event<()> {
        self.on_did_change_dirty.event()
    }

    fn on_did_change_content(&self) -> Event<()> {
        self.on_did_change_content.event()
    }

    fn on_did_save(&self) -> Event<WorkingCopySaveEvent> {
        self.on_did_save.event()
    }

    fn is_dirty(&self) -> bool {
        self.dirty()
    }

    fn is_modified(&self) -> bool {
        self.dirty()
    }

    fn backup(&self, token: &CancellationToken) -> Promise<WorkingCopyBackup> {
        MainThreadCustomEditorModel::backup(self, token)
    }

    fn save(&self, options: Option<SaveOptions>) -> Promise<bool> {
        self.save_document(options)
    }

    fn revert(&self, options: Option<RevertOptions>) -> Promise<()> {
        self.revert_document(options)
    }
}

impl ICustomEditorModel for MainThreadCustomEditorModel {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn view_type(&self) -> String {
        self.view_type.clone()
    }

    fn resource(&self) -> URI {
        self.resource_working_copy.resource().clone()
    }

    fn backup_id(&self) -> Option<String> {
        self.backup_id.borrow().clone()
    }

    fn can_hot_exit(&self) -> bool {
        self.can_hot_exit_value()
    }

    fn is_readonly(&self) -> Readonly {
        Readonly::Bool(!self.editable)
    }

    fn on_did_change_readonly(&self) -> Event<()> {
        Event::none()
    }

    fn is_orphaned(&self) -> bool {
        IResourceWorkingCopy::is_orphaned(self)
    }

    fn on_did_change_orphaned(&self) -> Event<()> {
        IResourceWorkingCopy::on_did_change_orphaned(self)
    }

    fn is_dirty(&self) -> bool {
        self.dirty()
    }

    fn on_did_change_dirty(&self) -> Event<()> {
        self.on_did_change_dirty.event()
    }

    fn revert(&self, options: Option<RevertOptions>) -> Promise<()> {
        self.revert_document(options)
    }

    fn save_custom_editor(&self, options: Option<SaveOptions>) -> Promise<Option<URI>> {
        self.save_custom_editor_document(options)
    }

    fn save_custom_editor_as(&self, resource: &URI, target_resource: &URI, options: Option<SaveOptions>) -> Promise<bool> {
        self.save_custom_editor_as_document(resource, target_resource, options)
    }
}
