//! Porte de `src/vs/workbench/api/browser/mainThreadWebviewPanels.ts`: o lado do workbench do RPC `ExtHostWebviewPanels`, que
//! abre os painéis de webview das extensões, guarda a correspondência entre handles e entradas de editor, acompanha o
//! estado de visibilidade dos painéis e restaura (`$registerSerializer`) os painéis salvos, com a localização da extensão
//! atualizada.
//!
//! Divergências:
//! - **Injeção.** O construtor `(context, mainThreadWebviews, @IConfigurationService, @IEditorGroupsService, @IEditorService,
//!   @IExtensionService, @IStorageService, @IWebviewWorkbenchService, @IUriIdentityService)` é [`MainThreadWebviewPanels::new`],
//!   com [`MainThreadWebviewPanelsServices`] para os serviços; o `MainThreadWebviews` entra como [`IMainThreadWebviews`].
//!   `@extHostNamedCustomer` é o registro do `extHostCustomers.ts`, que não foi portado.
//! - **`WebviewInputStore`** é um mapa de handle para entrada e outro, pela identidade da entrada, de volta ao handle.
//! - **`$deserializeWebviewPanel`.** O `initData` leva o `webviewOptions` do webview (um `WebviewContentOptions`, com
//!   `allowScripts`/`allowForms`) onde o protocolo declara `IWebviewContentOptions` (`enableScripts`/`enableForms`): o original
//!   passa o objeto como está (a tipagem estrutural do TypeScript deixa), e o porte tipado o converte campo a campo com
//!   [`webview_content_options_to_dto`], que copia `allowScripts` para `enableScripts` e `allowForms` para `enableForms`.
//! - **Erros.** `$registerSerializer`, `$unregisterSerializer` e o `Webview already registered` do `addWebview` lançam no
//!   original e não devolvem nada: o porte os levanta com [`errors::throw`] (o lançamento síncrono); dentro do
//!   `resolveWebview` do serializador, que é `async`, o erro é a promessa rejeitada, e o `catch` do original o mostra com
//!   [`errors::on_unexpected_error`]. O `console.error` do estado ilegível é um `eprintln!`.
//! - **`updateExtensionLocation`.** `IExtensionDescription` (o do porte, só com o `extensionLocation`) não leva o
//!   `identifier`: o `id` da extensão atualizada é o `id` guardado na entrada, que é o identificador pedido ao
//!   `getExtension` (o mesmo, pela igualdade de [`ExtensionIdentifier`]). A `URI.joinPath` é a do [`ExtUri`] em uso.
//! - **`DiffEditorInput`.** `input instanceof DiffEditorInput` é [`EditorInput::diff_editors`] (a modificada, `primary`, e a
//!   original, `secondary`), e a identidade de entradas (`===`) é a de [`id_of`].
//! - **Grupos.** `PreferredGroup` é o [`GroupRef`]; `ACTIVE_GROUP` e `SIDE_GROUP` são os identificadores especiais dele.
//! - **`webviewInputs` e `addWebviewInput`** são o trait [`IMainThreadWebviewPanels`] (o que o `MainThreadCustomEditors` lê
//!   do objeto, e o que o teste dele substitui pelo `mock<MainThreadWebviewPanels>()`).
//!
//! [`ExtUri`]: je_base::resources::ExtUri

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event;
use je_base::json;
use je_base::lifecycle::{Disposable, DisposableMap, DisposableRef, IDisposable, id_of};
use je_base::uuid::generate_uuid;
use je_platform::configuration::service::IConfigurationService;
use je_platform::storage::StorageService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::api::browser::main_thread_webviews::{
    IMainThreadWebviews, MainThreadWebviewOptions, revive_webview_content_options, revive_webview_extension,
};
use crate::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostWebviewPanelsShape, IMainContext, MainThreadWebviewPanelsShape, WebviewContentOptions as WebviewContentOptionsDto,
    WebviewDeserializeInitData, WebviewExtensionDescription as WebviewExtensionDescriptionDto, WebviewIconPathDto, WebviewInitData,
    WebviewPanelOptions as WebviewPanelOptionsDto, WebviewPanelShowOptions, WebviewPanelViewState, WebviewPanelViewStateData, WebviewSerializerOptions,
};
use crate::common::editor::editor_input::EditorInput;
use crate::common::editor::{EditorInputOrUntyped, EditorInputRef};
use crate::contrib::webview::browser_webview::{
    ExtensionKeyedWebviewOriginStore, IWebview, WebviewContentOptions, WebviewExtensionDescription, WebviewInitInfo, WebviewOptions,
};
use crate::contrib::webview_panel::webview_editor_input::{WebviewIconPath, WebviewInput};
use crate::contrib::webview_panel::webview_workbench_service::{IWebViewShowOptions, IWebviewWorkbenchService, WebviewResolver};
use crate::services::editor::editor_group_column::editor_group_to_column;
use crate::services::editor::editor_groups_service::{
    ACTIVE_GROUP, FindGroupScope, GroupLocation, GroupRef, GroupsOrder, IEditorGroup, IEditorGroupsService, SIDE_GROUP,
    preferred_side_by_side_group_direction,
};
use crate::services::editor::editor_service::IEditorService;
use crate::services::extensions::extensions::IExtensionService;

/// O prefixo de `WebviewViewTypeTransformer('mainThreadWebview-')`.
const WEBVIEW_PANEL_VIEW_TYPE_PREFIX: &str = "mainThreadWebview-";

/// `WebviewViewTypeTransformer.fromExternal(viewType)`.
fn view_type_from_external(view_type: &str) -> String {
    format!("{WEBVIEW_PANEL_VIEW_TYPE_PREFIX}{view_type}")
}

/// `WebviewViewTypeTransformer.toExternal(viewType)`.
fn view_type_to_external(view_type: &str) -> Option<String> {
    view_type.strip_prefix(WEBVIEW_PANEL_VIEW_TYPE_PREFIX).map(str::to_owned)
}

/// Bi-directional map between webview handles and inputs (`WebviewInputStore`).
#[derive(Default)]
struct WebviewInputStore {
    handles_to_inputs: HashMap<String, Rc<WebviewInput>>,
    inputs_to_handles: HashMap<usize, String>,
}

impl WebviewInputStore {
    fn add(&mut self, handle: &str, input: Rc<WebviewInput>) {
        self.inputs_to_handles.insert(id_of(&input), handle.to_owned());
        self.handles_to_inputs.insert(handle.to_owned(), input);
    }

    fn get_handle_for_input(&self, input: &EditorInputRef) -> Option<String> {
        self.inputs_to_handles.get(&id_of(input)).cloned()
    }

    fn get_input_for_handle(&self, handle: &str) -> Option<Rc<WebviewInput>> {
        self.handles_to_inputs.get(handle).cloned()
    }

    fn delete(&mut self, handle: &str) {
        if let Some(input) = self.handles_to_inputs.remove(handle) {
            self.inputs_to_handles.remove(&id_of(&input));
        }
    }

    fn size(&self) -> usize {
        self.handles_to_inputs.len()
    }
}

/// Os serviços que o construtor do `MainThreadWebviewPanels` injeta.
pub struct MainThreadWebviewPanelsServices {
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub editor_group_service: Rc<dyn IEditorGroupsService>,
    pub editor_service: Rc<dyn IEditorService>,
    pub extension_service: Rc<dyn IExtensionService>,
    pub storage_service: Rc<dyn StorageService>,
    pub webview_workbench_service: Rc<dyn IWebviewWorkbenchService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
}

/// O que o `MainThreadCustomEditors` lê do `MainThreadWebviewPanels` (`webviewInputs` e `addWebviewInput`): o contrato que o
/// teste substitui pelo `mock<MainThreadWebviewPanels>()`.
pub trait IMainThreadWebviewPanels {
    /// `webviewInputs`.
    fn webview_inputs(&self) -> Vec<Rc<WebviewInput>>;

    /// `addWebviewInput(handle, input, options)`.
    fn add_webview_input(&self, handle: &str, input: Rc<WebviewInput>, options: MainThreadWebviewOptions);
}

pub struct MainThreadWebviewPanels {
    this: Weak<MainThreadWebviewPanels>,
    base: Rc<Disposable>,

    proxy: Rc<dyn ExtHostWebviewPanelsShape>,

    webview_inputs: RefCell<WebviewInputStore>,

    revivers: Rc<DisposableMap<String, dyn IDisposable>>,

    webview_origin_store: ExtensionKeyedWebviewOriginStore,

    main_thread_webviews: Rc<dyn IMainThreadWebviews>,
    configuration_service: Rc<dyn IConfigurationService>,
    editor_group_service: Rc<dyn IEditorGroupsService>,
    editor_service: Rc<dyn IEditorService>,
    extension_service: Rc<dyn IExtensionService>,
    webview_workbench_service: Rc<dyn IWebviewWorkbenchService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
}

impl MainThreadWebviewPanels {
    pub fn new(context: &dyn IMainContext, main_thread_webviews: Rc<dyn IMainThreadWebviews>, services: MainThreadWebviewPanelsServices) -> Rc<Self> {
        let base = Disposable::new();
        let revivers: Rc<DisposableMap<String, dyn IDisposable>> = DisposableMap::new();
        base.register(revivers.clone());

        let MainThreadWebviewPanelsServices {
            configuration_service,
            editor_group_service,
            editor_service,
            extension_service,
            storage_service,
            webview_workbench_service,
            uri_identity_service,
        } = services;

        let panels = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            proxy: context.get_proxy(&ExtHostContext::EXT_HOST_WEBVIEW_PANELS),
            webview_inputs: RefCell::new(WebviewInputStore::default()),
            revivers,
            webview_origin_store: ExtensionKeyedWebviewOriginStore::new("mainThreadWebviewPanel.origins", storage_service),
            main_thread_webviews,
            configuration_service,
            editor_group_service: editor_group_service.clone(),
            editor_service: editor_service.clone(),
            extension_service: extension_service.clone(),
            webview_workbench_service: webview_workbench_service.clone(),
            uri_identity_service,
        });

        let this = panels.this.clone();
        let editors_changed = event::any(vec![
            editor_service.on_did_active_editor_change(),
            editor_service.on_did_visible_editors_change().signal(),
            editor_group_service.on_did_add_group().signal(),
            editor_group_service.on_did_remove_group().signal(),
            editor_group_service.on_did_move_group().signal(),
        ]);
        panels.base.register(editors_changed.listen(move |_| {
            if let Some(this) = this.upgrade() {
                let active_editor = this.active_editor_input();
                this.update_webview_view_states(active_editor);
            }
        }));

        let this = panels.this.clone();
        panels.base.register(webview_workbench_service.on_did_change_active_webview_editor().listen(move |input| {
            if let Some(this) = this.upgrade() {
                this.update_webview_view_states(input.clone().map(|input| input as EditorInputRef));
            }
        }));

        // This reviver's only job is to activate extensions.
        // This should trigger the real reviver to be registered from the extension host side.
        panels.base.register(webview_workbench_service.register_resolver(Rc::new(ActivationResolver { extension_service })));

        panels
    }

    /// `this._editorService.activeEditor`, só a entrada tipada.
    fn active_editor_input(&self) -> Option<EditorInputRef> {
        match self.editor_service.active_editor() {
            Some(EditorInputOrUntyped::Typed(input)) => Some(input),
            _ => None,
        }
    }
}

impl IMainThreadWebviewPanels for MainThreadWebviewPanels {
    /// `webviewInputs`.
    fn webview_inputs(&self) -> Vec<Rc<WebviewInput>> {
        self.webview_inputs.borrow().handles_to_inputs.values().cloned().collect()
    }

    /// `addWebviewInput(handle, input, options)`: lança (`Webview already registered`) quando o handle já está registrado.
    fn add_webview_input(&self, handle: &str, input: Rc<WebviewInput>, options: MainThreadWebviewOptions) {
        if let Err(error) = self.try_add_webview_input(handle, input, options) {
            errors::throw(error);
        }
    }
}

impl MainThreadWebviewPanels {
    fn try_add_webview_input(&self, handle: &str, input: Rc<WebviewInput>, options: MainThreadWebviewOptions) -> Result<(), Error> {
        self.webview_inputs.borrow_mut().add(handle, input.clone());
        let webview: Rc<dyn IWebview> = input.webview();
        self.main_thread_webviews.add_webview(handle, webview, options)?;

        let subscription: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let listener_subscription = subscription.clone();
        let this = self.this.clone();
        let handle = handle.to_owned();
        let listener = input.webview().on_did_dispose().listen(move |_| {
            let subscription = listener_subscription.borrow_mut().take();
            if let Some(subscription) = subscription {
                subscription.dispose();
            }

            if let Some(panels) = this.upgrade() {
                let this = this.clone();
                let handle = handle.clone();
                panels.proxy.rpc_on_did_dispose_webview_panel(&handle).finally(move || {
                    if let Some(panels) = this.upgrade() {
                        panels.webview_inputs.borrow_mut().delete(&handle);
                    }
                });
            }
        });
        *subscription.borrow_mut() = Some(listener);
        Ok(())
    }

    fn get_target_group_from_show_options(&self, show_options: &WebviewPanelShowOptions) -> GroupRef {
        let active = GroupRef::Id(ACTIVE_GROUP);
        let Some(view_column) = show_options.view_column else {
            return active;
        };
        if view_column == ACTIVE_GROUP as f64 || (self.editor_group_service.count() == 1 && self.editor_group_service.active_group().is_empty()) {
            return active;
        }

        if view_column == SIDE_GROUP as f64 {
            return GroupRef::Id(SIDE_GROUP);
        }

        if view_column >= 0.0 {
            // First check to see if an existing group exists
            let groups = self.editor_group_service.get_groups(GroupsOrder::GridAppearance);
            let group_in_column = if view_column.fract() == 0.0 { groups.get(view_column as usize) } else { None };
            if let Some(group_in_column) = group_in_column {
                return GroupRef::Id(group_in_column.id());
            }

            // We are dealing with an unknown group and therefore need a new group.
            // Note that the new group's id may not match the one requested. We only allow
            // creating a single new group, so if someone passes in `showOptions.viewColumn = 99`
            // and there are two editor groups open, we simply create a third editor group instead
            // of creating all the groups up to 99.
            let new_group = self.editor_group_service.find_group(&FindGroupScope { location: Some(GroupLocation::Last), ..FindGroupScope::default() });
            if let Some(new_group) = new_group {
                let direction = preferred_side_by_side_group_direction(&*self.configuration_service);
                return GroupRef::Group(self.editor_group_service.add_group(Some(GroupRef::Group(new_group)), direction));
            }
        }

        active
    }

    /// `updateExtensionLocation(webviewInput)`.
    fn update_extension_location(this: Rc<Self>, webview_input: Rc<WebviewInput>) -> je_base::promise::Promise<()> {
        async_fn(async move {
            let Some(old_extension) = webview_input.extension() else {
                return Ok(());
            };
            let Some(old_location) = old_extension.location.clone() else {
                return Ok(());
            };

            let extension = this.extension_service.get_extension(&old_extension.id.value).await?;
            let ext_uri = this.uri_identity_service.ext_uri();
            let Some(extension) = extension else {
                return Ok(());
            };
            if ext_uri.is_equal(Some(&old_location), Some(&extension.extension_location), false) {
                return Ok(());
            }

            let webview = webview_input.webview();
            let old_location = ext_uri.remove_trailing_path_separator(&ext_uri.normalize_path(&old_location), None);
            let mut content_options = webview.content_options();
            content_options.local_resource_roots = content_options.local_resource_roots.map(|roots| {
                roots
                    .into_iter()
                    .map(|root| {
                        let normalized_root = ext_uri.normalize_path(&root);
                        if !ext_uri.is_equal_or_parent(&normalized_root, &old_location, false, false) {
                            return root;
                        }
                        let path = normalized_root.path();
                        let relative = path.slice(old_location.path().len().min(path.len()), path.len()).to_string_lossy();
                        ext_uri.join_path(&extension.extension_location, &[relative.as_str()])
                    })
                    .collect()
            });
            webview.set_content_options(content_options);
            webview.set_extension(Some(WebviewExtensionDescription { id: old_extension.id.clone(), location: Some(extension.extension_location.clone()) }));
            Ok(())
        })
    }

    fn update_webview_view_states(&self, active_editor_input: Option<EditorInputRef>) {
        if self.webview_inputs.borrow().size() == 0 {
            return;
        }

        let mut view_states: WebviewPanelViewStateData = Vec::new();

        let mut update_view_states_for_input = |group: &Rc<dyn IEditorGroup>, top_level_input: &EditorInputRef, editor_input: &EditorInputRef| {
            let Some(webview_input) = editor_input.as_any().downcast_ref::<WebviewInput>() else {
                return;
            };

            webview_input.update_group(group.id());

            let handle = self.webview_inputs.borrow().get_handle_for_input(editor_input);
            if let Some(handle) = handle {
                view_states.push((
                    handle,
                    WebviewPanelViewState {
                        visible: group.active_editor().is_some_and(|active| id_of(top_level_input) == id_of(&active)),
                        active: active_editor_input.as_ref().is_some_and(|active| id_of(editor_input) == id_of(active)),
                        position: editor_group_to_column(&*self.editor_group_service, &GroupRef::Id(group.id())),
                    },
                ));
            }
        };

        for group in self.editor_group_service.groups() {
            for input in group.editors() {
                if let Some((primary, secondary)) = input.diff_editors() {
                    update_view_states_for_input(&group, &input, &primary);
                    update_view_states_for_input(&group, &input, &secondary);
                } else {
                    update_view_states_for_input(&group, &input, &input);
                }
            }
        }

        if !view_states.is_empty() {
            self.proxy.rpc_on_did_change_webview_panel_view_states(view_states);
        }
    }

    fn try_get_webview_input(&self, handle: &str) -> Option<Rc<WebviewInput>> {
        self.webview_inputs.borrow().get_input_for_handle(handle)
    }
}

impl IDisposable for MainThreadWebviewPanels {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl MainThreadWebviewPanelsShape for MainThreadWebviewPanels {
    fn rpc_create_webview_panel(
        &self,
        extension_data: WebviewExtensionDescriptionDto,
        handle: &str,
        view_type: &str,
        init_data: WebviewInitData,
        show_options: WebviewPanelShowOptions,
    ) {
        let target_group = self.get_target_group_from_show_options(&show_options);
        let main_thread_show_options = IWebViewShowOptions { preserve_focus: Some(show_options.preserve_focus.unwrap_or(false)), group: Some(target_group) };

        let extension = revive_webview_extension(&extension_data);
        let origin = self.webview_origin_store.get_origin(view_type, &extension.id);

        let webview = self.webview_workbench_service.open_webview(
            WebviewInitInfo {
                origin: Some(origin),
                provided_view_type: Some(view_type.to_owned()),
                title: Some(init_data.title.clone()),
                options: revive_webview_options(&init_data.panel_options),
                content_options: revive_webview_content_options(&init_data.webview_options),
                extension: Some(extension),
            },
            &view_type_from_external(view_type),
            &init_data.title,
            None,
            main_thread_show_options,
        );

        self.add_webview_input(handle, webview, MainThreadWebviewOptions { serialize_buffers_for_post_message: init_data.serialize_buffers_for_post_message });
    }

    fn rpc_dispose_webview(&self, handle: &str) {
        let Some(webview) = self.try_get_webview_input(handle) else {
            return;
        };
        webview.dispose();
    }

    fn rpc_set_title(&self, handle: &str, value: &str) {
        if let Some(webview) = self.try_get_webview_input(handle) {
            webview.set_webview_title(value);
        }
    }

    fn rpc_set_icon_path(&self, handle: &str, value: Option<WebviewIconPathDto>) {
        if let Some(webview) = self.try_get_webview_input(handle) {
            webview.set_icon_path(revive_webview_icon(value));
        }
    }

    fn rpc_reveal(&self, handle: &str, show_options: WebviewPanelShowOptions) {
        let Some(webview) = self.try_get_webview_input(handle) else {
            return;
        };
        if webview.is_disposed() {
            return;
        }

        let target_group = self.get_target_group_from_show_options(&show_options);
        self.webview_workbench_service.reveal_webview(&webview, target_group, show_options.preserve_focus.unwrap_or(false));
    }

    fn rpc_register_serializer(&self, view_type: &str, options: WebviewSerializerOptions) {
        let key = view_type.to_owned();
        if self.revivers.has(&key) {
            errors::throw(Error::new(format!("Reviver for {view_type} already registered")));
        }

        let resolver = Rc::new(SerializerResolver { panels: self.this.clone(), view_type: key.clone(), options });
        self.revivers.set(key, self.webview_workbench_service.register_resolver(resolver), false);
    }

    fn rpc_unregister_serializer(&self, view_type: &str) {
        let key = view_type.to_owned();
        if !self.revivers.has(&key) {
            errors::throw(Error::new(format!("No reviver for {view_type} registered")));
        }

        self.revivers.delete_and_dispose(&key);
    }
}

/// O resolvedor que só ativa as extensões (`canResolve` dispara `onWebviewPanel:<viewType>` e devolve `false`).
struct ActivationResolver {
    extension_service: Rc<dyn IExtensionService>,
}

impl WebviewResolver for ActivationResolver {
    fn can_resolve(&self, webview: &Rc<WebviewInput>) -> bool {
        if let Some(view_type) = view_type_to_external(&webview.view_type) {
            self.extension_service.activate_by_event(&format!("onWebviewPanel:{view_type}"), None);
        }
        false
    }

    fn resolve_webview(&self, _webview: Rc<WebviewInput>, _token: je_base::cancellation::CancellationToken) -> je_base::promise::Promise<()> {
        errors::throw(Error::new("not implemented"))
    }
}

/// O resolvedor que `$registerSerializer` registra: restaura o painel pelo serializador do host de extensões.
struct SerializerResolver {
    panels: Weak<MainThreadWebviewPanels>,
    view_type: String,
    options: WebviewSerializerOptions,
}

impl WebviewResolver for SerializerResolver {
    fn can_resolve(&self, webview_input: &Rc<WebviewInput>) -> bool {
        webview_input.view_type == view_type_from_external(&self.view_type)
    }

    fn resolve_webview(&self, webview_input: Rc<WebviewInput>, _token: je_base::cancellation::CancellationToken) -> je_base::promise::Promise<()> {
        let Some(panels) = self.panels.upgrade() else {
            return je_base::promise::Promise::resolved(());
        };
        let options = MainThreadWebviewOptions { serialize_buffers_for_post_message: self.options.serialize_buffers_for_post_message };
        async_fn(async move {
            let Some(view_type) = view_type_to_external(&webview_input.view_type) else {
                webview_input.webview().set_html(&panels.main_thread_webviews.get_webview_resolved_failed_content(&webview_input.view_type));
                return Ok(());
            };

            let handle = generate_uuid();

            panels.try_add_webview_input(&handle, webview_input.clone(), options)?;

            let mut state = Dynamic::Undefined;
            if let Some(serialized_state) = webview_input.webview().state().filter(|state| !state.is_empty()) {
                let mut parse_errors = Vec::new();
                match json::parse_str(&serialized_state, &mut parse_errors).filter(|_| parse_errors.is_empty()) {
                    Some(parsed) => state = Dynamic::from_json(&parsed),
                    None => eprintln!("Could not load webview state {serialized_state}"),
                }
            }

            let deserialized = async {
                MainThreadWebviewPanels::update_extension_location(panels.clone(), webview_input.clone()).await?;
                let webview = webview_input.webview();
                let active = panels.active_editor_input().is_some_and(|active| id_of(&active) == id_of(&webview_input));
                panels
                    .proxy
                    .rpc_deserialize_webview_panel(
                        &handle,
                        &view_type,
                        WebviewDeserializeInitData {
                            title: webview_input.get_title(None),
                            state,
                            panel_options: webview_options_to_panel_options_dto(&webview.options()),
                            webview_options: webview_content_options_to_dto(&webview.content_options()),
                            active,
                        },
                        editor_group_to_column(&*panels.editor_group_service, &GroupRef::Id(webview_input.group().unwrap_or(0))),
                    )
                    .await
            }
            .await;
            if let Err(error) = deserialized {
                errors::on_unexpected_error(&error);
                webview_input.webview().set_html(&panels.main_thread_webviews.get_webview_resolved_failed_content(&view_type));
            }
            Ok(())
        })
    }
}

/// `reviveWebviewIcon(value)`.
fn revive_webview_icon(value: Option<WebviewIconPathDto>) -> Option<WebviewIconPath> {
    match value? {
        WebviewIconPathDto::ThemeIcon(icon) => Some(WebviewIconPath::ThemeIcon(icon)),
        WebviewIconPathDto::LightDark { light, dark } => Some(WebviewIconPath::LightDark { light, dark }),
    }
}

/// `reviveWebviewOptions(panelOptions)`.
pub(crate) fn revive_webview_options(panel_options: &WebviewPanelOptionsDto) -> WebviewOptions {
    WebviewOptions {
        enable_find_widget: panel_options.enable_find_widget,
        retain_context_when_hidden: panel_options.retain_context_when_hidden,
        ..WebviewOptions::default()
    }
}

/// O `webview.options` (um `WebviewOptions`) lido como `IWebviewPanelOptions`: os dois campos que o protocolo declara.
pub(crate) fn webview_options_to_panel_options_dto(options: &WebviewOptions) -> WebviewPanelOptionsDto {
    WebviewPanelOptionsDto { enable_find_widget: options.enable_find_widget, retain_context_when_hidden: options.retain_context_when_hidden }
}

/// O `webview.contentOptions` lido como `IWebviewContentOptions` (ver a nota do módulo).
pub fn webview_content_options_to_dto(options: &WebviewContentOptions) -> WebviewContentOptionsDto {
    WebviewContentOptionsDto {
        enable_scripts: options.allow_scripts,
        enable_forms: options.allow_forms,
        enable_command_uris: options.enable_command_uris.clone(),
        local_resource_roots: options.local_resource_roots.clone(),
        port_mapping: options.port_mapping.clone(),
    }
}
