//! Porte de `src/vs/workbench/contrib/customEditor/browser/customEditorInput.ts`: a entrada de editor de um editor
//! personalizado (`CustomEditorInput`), que se resolve contra o modelo do documento (`ICustomEditorModel`) e leva a navegação
//! de um editor de texto personalizado.
//!
//! Divergências:
//! - **Herança.** `CustomEditorInput extends LazilyResolvedWebviewEditorInput extends WebviewInput` é um [`WebviewInputKind`]
//!   ([`CustomEditorInput`]) que guarda o [`LazilyResolvedWebviewEditorInput`] e a entrada é o [`WebviewInput`] que o
//!   construtor [`CustomEditorInput::new`] devolve (ver a nota de herança de [`WebviewInput`]); `instanceof CustomEditorInput`
//!   é `webview_input.kind_as::<CustomEditorInput>()`. Os membros que são da classe, e não do contrato `EditorInput`
//!   (`navigation`, `onDidChangeNavigation`, `backupId`, `untitledDocumentData`, `oldResource`, `undo`, `redo`, `onMove`),
//!   são métodos da struct.
//! - **Injeção.** Os serviços decorados do construtor (`IThemeService`, `IWebviewWorkbenchService`, `IInstantiationService`,
//!   `ILabelService`, `ICustomEditorService`, `IFileDialogService`, `IUndoRedoService`, `IFileService`,
//!   `IFilesConfigurationService`, `IEditorGroupsService`, `IWorkbenchLayoutService`, `ICustomEditorLabelService`) são a
//!   [`CustomEditorInputServices`]; o `IInstantiationService` de `copy()` e de `create` é a própria struct de serviços (com
//!   o `IUntitledTextEditorService` e o `IWebviewService` do `invokeFunction`). O `IThemeService` é o
//!   [`IWebviewThemeService`], o recorte de tema que `WebviewInput` lê (o `IThemeService` não foi portado).
//! - **Fora: `claim`, `canMove` e `doCanMove`**, que dependem do contêiner DOM do webview (`webview.container` e o `CodeWindow`
//!   de destino, recorte da Fase 6) e do `IWorkbenchLayoutService`. Sem eles, a entrada usa o `canMove` padrão (`true`).
//! - **`Selection.liftSelection(selection)`** é a identidade (o [`Selection`] já é a seleção lógica), e o `getSelection`
//!   devolve a [`TextEditorPaneSelection`].
//! - `applyOptions(options: ITextEditorOptions, token)` recebe as [`TextEditorOptions`] (as de `EditorOptions` com a seleção);
//!   `options.selection.endLineNumber ?? startLineNumber` e `endColumn ?? startColumn` são os `Option::unwrap_or`.
//! - `resolve(): Promise<null>` é `Promise<Option<Rc<dyn IEditorModel>>>` (sempre `None`), e `assertReturnsDefined` lança o
//!   erro do original (`Assertion Failed: argument is undefined or null`) como promessa rejeitada.
//! - `isEqual` é o `ExtUri::is_equal`; `basename(getUriLabel(...))` é o `basename` do `path` do processo hospedeiro; os
//!   caches de rótulo (`_shortDescription`...) são `RefCell<Option<String>>`. `readonly onDidChangeNavigation` é
//!   [`CustomEditorInput::on_did_change_navigation`].

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::network::schemas;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::dialogs::dialogs::IFileDialogService;
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::opener::editor_types::{EditorOptions, EditorOverride};
use je_platform::resources::ext_uri;
use je_platform::undo_redo::undo_redo::IUndoRedoService;
use je_editor::range::Range;
use je_editor::selection::{Selection, SelectionDirection};

use super::custom_editor::{CustomEditorModelReference, ICustomEditorService};
use super::custom_text_editor_navigation::ICustomTextEditorNavigation;
use crate::browser::parts::editor::text_editor::TextEditorPaneSelection;
use crate::common::editor::editor_input::{CanMove, EditorInput, matches_by_default};
use crate::common::editor::editor_model::IEditorModel;
use crate::common::editor::{
    EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef, GroupIdentifier, IEditorPaneSelection, MoveResult, RevertOptions, SaveOptions, TextEditorOptions,
    UntypedEditorInput, Verbosity,
};
use crate::contrib::webview::browser_webview::{IOverlayWebview, IWebviewService, WebviewInitInfo, WebviewOptions};
use crate::contrib::webview_panel::webview_editor_input::{IWebviewThemeService, WebviewIconPath, WebviewInput, WebviewInputInitInfo, WebviewInputKind};
use crate::contrib::webview_panel::webview_workbench_service::{IWebviewWorkbenchService, LazilyResolvedWebviewEditorInput};
use crate::services::editor::custom_editor_label_service::ICustomEditorLabelService;
use crate::services::editor::editor_groups_service::IEditorGroupsService;
use crate::services::files_configuration::files_configuration_service::{IFilesConfigurationService, Readonly};
use crate::services::untitled::untitled_text_editor_service::IUntitledTextEditorService;
use crate::services::textmodel_resolver::resolver_service::ITextEditorModel;

/// `CustomEditorInputInitInfo`.
#[derive(Clone)]
pub struct CustomEditorInputInitInfo {
    pub resource: URI,
    pub view_type: String,
    pub webview_title: Option<String>,
    pub preferred_name: Option<String>,
    pub icon_path: Option<WebviewIconPath>,
}

/// O `options` do construtor: `{ startsDirty?, backupId?, untitledDocumentData?, oldResource? }`.
#[derive(Clone, Default)]
pub struct CustomEditorInputOptions {
    pub starts_dirty: Option<bool>,
    pub backup_id: Option<String>,
    pub untitled_document_data: Option<VSBuffer>,
    pub old_resource: Option<URI>,
}

/// O `options` de `create`: `{ customClasses?, oldResource? }`.
#[derive(Clone, Default)]
pub struct CustomEditorInputCreateOptions {
    pub custom_classes: Option<String>,
    pub old_resource: Option<URI>,
}

/// Os serviços que o construtor e o `create` do `CustomEditorInput` injetam (ver a nota do módulo).
pub struct CustomEditorInputServices {
    pub theme_service: Rc<dyn IWebviewThemeService>,
    pub webview_workbench_service: Rc<dyn IWebviewWorkbenchService>,
    pub label_service: Rc<dyn ILabelService>,
    pub custom_editor_service: Rc<dyn ICustomEditorService>,
    pub file_dialog_service: Rc<dyn IFileDialogService>,
    pub undo_redo_service: Rc<dyn IUndoRedoService>,
    pub file_service: Rc<dyn IFileService>,
    pub files_configuration_service: Rc<dyn IFilesConfigurationService>,
    pub editor_groups_service: Rc<dyn IEditorGroupsService>,
    pub custom_editor_label_service: Rc<dyn ICustomEditorLabelService>,
    pub untitled_text_editor_service: Rc<dyn IUntitledTextEditorService>,
    pub webview_service: Rc<dyn IWebviewService>,
}

/// `basename(labelService.getUriLabel(resource))`.
pub(crate) fn basename_of_label(label: &str) -> String {
    PathModule::native(&ProcessEnvironment).basename(label, None)
}

pub struct CustomEditorInput {
    lazy: LazilyResolvedWebviewEditorInput,
    services: Rc<CustomEditorInputServices>,

    on_did_change_navigation: Rc<Emitter<()>>,
    navigation: RefCell<Option<Rc<dyn ICustomTextEditorNavigation>>>,

    editor_resource: URI,
    pub old_resource: Option<URI>,
    default_dirty_state: Cell<Option<bool>>,

    editor_name: RefCell<Option<String>>,

    backup_id: Option<String>,

    untitled_document_data: Option<VSBuffer>,

    model_ref: RefCell<Option<CustomEditorModelReference>>,

    move_handler: RefCell<Option<Rc<dyn Fn(URI)>>>,

    short_description: RefCell<Option<String>>,
    medium_description: RefCell<Option<String>>,
    long_description: RefCell<Option<String>>,
    short_title: RefCell<Option<String>>,
    medium_title: RefCell<Option<String>>,
    long_title: RefCell<Option<String>>,
}

impl CustomEditorInput {
    /// `CustomEditorInput.typeId`.
    pub const TYPE_ID: &'static str = "workbench.editors.webviewEditor";

    /// `static create(instantiationService, init, group, options)`.
    pub fn create(
        services: &Rc<CustomEditorInputServices>,
        init: CustomEditorInputInitInfo,
        group: Option<GroupIdentifier>,
        options: Option<CustomEditorInputCreateOptions>,
    ) -> Rc<WebviewInput> {
        // If it's an untitled file we must populate the untitledDocumentData
        let untitled_text_model = services.untitled_text_editor_service.get(&init.resource);
        let untitled_string = untitled_text_model.and_then(|model| model.text_editor_model()).map(|model| model.get_value().to_string_lossy());
        let untitled_document_data = untitled_string.filter(|value| !value.is_empty()).map(|value| VSBuffer::from_string(&value));

        let options = options.unwrap_or_default();
        let webview = services.webview_service.create_webview_overlay(WebviewInitInfo {
            provided_view_type: Some(init.view_type.clone()),
            title: init.webview_title.clone(),
            options: WebviewOptions { custom_classes: options.custom_classes.clone(), ..WebviewOptions::default() },
            content_options: Default::default(),
            extension: None,
            origin: None,
        });
        let input = Self::new(
            init,
            webview,
            CustomEditorInputOptions { untitled_document_data, old_resource: options.old_resource, ..CustomEditorInputOptions::default() },
            services.clone(),
        );
        if let Some(group) = group {
            input.update_group(group);
        }
        input
    }

    /// `new CustomEditorInput(init, webview, options, ...services)`.
    pub fn new(init: CustomEditorInputInitInfo, webview: Rc<dyn IOverlayWebview>, options: CustomEditorInputOptions, services: Rc<CustomEditorInputServices>) -> Rc<WebviewInput> {
        let on_did_change_navigation = Emitter::new();
        let kind = Self {
            lazy: LazilyResolvedWebviewEditorInput::new(services.webview_workbench_service.clone()),
            services: services.clone(),
            on_did_change_navigation,
            navigation: RefCell::new(None),
            editor_resource: init.resource.clone(),
            old_resource: options.old_resource,
            default_dirty_state: Cell::new(options.starts_dirty),
            editor_name: RefCell::new(None),
            backup_id: options.backup_id,
            untitled_document_data: options.untitled_document_data,
            model_ref: RefCell::new(None),
            move_handler: RefCell::new(None),
            short_description: RefCell::new(None),
            medium_description: RefCell::new(None),
            long_description: RefCell::new(None),
            short_title: RefCell::new(None),
            medium_title: RefCell::new(None),
            long_title: RefCell::new(None),
        };
        WebviewInput::with_kind(
            WebviewInputInitInfo {
                provided_id: Some(init.view_type.clone()),
                view_type: init.view_type,
                name: init.preferred_name.unwrap_or_default(),
                icon_path: init.icon_path,
            },
            webview,
            services.theme_service.clone(),
            Rc::new(kind),
        )
    }

    /// `registerListeners()` (e o `_register` do `onDidChangeNavigation`).
    fn register_listeners(&self, input: &Rc<WebviewInput>) {
        let base = EditorInput::base(&**input);
        base.register(self.on_did_change_navigation.clone());

        // Clear our labels on certain label related events
        let on_label_event: Rc<dyn Fn(&str)> = {
            let weak = Rc::downgrade(input);
            Rc::new(move |scheme: &str| {
                if let Some(input) = weak.upgrade()
                    && let Some(this) = input.kind_as::<CustomEditorInput>()
                    && scheme == this.editor_resource.scheme().to_string_lossy()
                {
                    this.update_label(&input);
                }
            })
        };
        base.register(self.services.label_service.on_did_change_formatters().listen({
            let on_label_event = on_label_event.clone();
            move |e| on_label_event(&e.scheme)
        }));
        base.register(self.services.file_service.on_did_change_file_system_provider_registrations().listen({
            let on_label_event = on_label_event.clone();
            move |e| on_label_event(&e.scheme)
        }));
        base.register(self.services.file_service.on_did_change_file_system_provider_capabilities().listen(move |e| on_label_event(&e.scheme)));

        let weak = Rc::downgrade(input);
        base.register(self.services.custom_editor_label_service.on_did_change().listen(move |_| {
            if let Some(input) = weak.upgrade()
                && let Some(this) = input.kind_as::<CustomEditorInput>()
            {
                this.update_label(&input);
            }
        }));
        let weak = Rc::downgrade(input);
        base.register(self.services.files_configuration_service.on_did_change_readonly().listen(move |_| {
            if let Some(input) = weak.upgrade() {
                EditorInput::base(&*input).fire_did_change_capabilities();
            }
        }));
    }

    fn update_label(&self, input: &WebviewInput) {
        // Clear any cached labels from before
        *self.editor_name.borrow_mut() = None;
        *self.short_description.borrow_mut() = None;
        *self.medium_description.borrow_mut() = None;
        *self.long_description.borrow_mut() = None;
        *self.short_title.borrow_mut() = None;
        *self.medium_title.borrow_mut() = None;
        *self.long_title.borrow_mut() = None;

        // Trigger recompute of label
        EditorInput::base(input).fire_did_change_label();
    }

    /// `navigation` (getter).
    pub fn navigation(&self) -> Option<Rc<dyn ICustomTextEditorNavigation>> {
        self.navigation.borrow().clone()
    }

    /// `navigation` (setter): avisa `onDidChangeNavigation` quando o valor muda.
    pub fn set_navigation(&self, value: Option<Rc<dyn ICustomTextEditorNavigation>>) {
        let changed = match (&*self.navigation.borrow(), &value) {
            (None, None) => false,
            (Some(current), Some(next)) => !Rc::ptr_eq(current, next),
            _ => true,
        };
        if changed {
            *self.navigation.borrow_mut() = value;
            self.on_did_change_navigation.fire(());
        }
    }

    /// `onDidChangeNavigation`.
    pub fn on_did_change_navigation(&self) -> Event<()> {
        self.on_did_change_navigation.event()
    }

    /// `resource`.
    pub fn editor_resource(&self) -> &URI {
        &self.editor_resource
    }

    /// `backupId`.
    pub fn backup_id(&self) -> Option<String> {
        match &*self.model_ref.borrow() {
            Some(model_ref) => model_ref.object.backup_id(),
            None => self.backup_id.clone(),
        }
    }

    /// `untitledDocumentData`.
    pub fn untitled_document_data(&self) -> Option<VSBuffer> {
        self.untitled_document_data.clone()
    }

    /// `undo()`: `assertReturnsDefined(this._modelRef)` lança quando o modelo ainda não foi resolvido.
    pub fn undo(&self) -> Promise<()> {
        self.assert_model_ref();
        self.services.undo_redo_service.undo(&self.editor_resource)
    }

    /// `redo()`.
    pub fn redo(&self) -> Promise<()> {
        self.assert_model_ref();
        self.services.undo_redo_service.redo(&self.editor_resource)
    }

    fn assert_model_ref(&self) {
        if self.model_ref.borrow().is_none() {
            je_base::errors::throw(Error::new("Assertion Failed: argument is undefined or null"));
        }
    }

    /// `onMove(handler)`.
    pub fn on_move(&self, handler: Rc<dyn Fn(URI)>) {
        *self.move_handler.borrow_mut() = Some(handler);
    }

    fn ext_equal(a: &URI, b: &URI) -> bool {
        ext_uri().is_equal(Some(a), Some(b), false)
    }

    fn is_readonly_value(&self) -> Readonly {
        match &*self.model_ref.borrow() {
            None => self.services.files_configuration_service.is_readonly(&self.editor_resource, None),
            Some(model_ref) => model_ref.object.is_readonly(),
        }
    }

    fn is_dirty_value(&self) -> bool {
        match &*self.model_ref.borrow() {
            None => self.default_dirty_state.get().unwrap_or(false),
            Some(model_ref) => model_ref.object.is_dirty(),
        }
    }

    fn capabilities_value(&self, input: &WebviewInput) -> EditorInputCapabilities {
        let mut capabilities = EditorInputCapabilities::NONE;

        capabilities |= EditorInputCapabilities::CAN_DROP_INTO_EDITOR;

        let supports_multiple_editors_per_document = self
            .services
            .custom_editor_service
            .get_custom_editor_capabilities(&input.view_type)
            .and_then(|capabilities| capabilities.supports_multiple_editors_per_document)
            .unwrap_or(false);
        if !supports_multiple_editors_per_document {
            capabilities |= EditorInputCapabilities::SINGLETON;
        }

        if self.is_readonly_value().is_readonly() {
            capabilities |= EditorInputCapabilities::READONLY;
        }

        if *self.editor_resource.scheme() == schemas::UNTITLED {
            capabilities |= EditorInputCapabilities::UNTITLED;
        }

        capabilities
    }

    fn uri_label(&self, resource: &URI, relative: bool) -> String {
        self.services.label_service.get_uri_label(resource, &UriLabelOptions { relative: relative.then_some(true), ..UriLabelOptions::default() })
    }

    fn cached(cache: &RefCell<Option<String>>, compute: impl FnOnce() -> String) -> String {
        if let Some(value) = &*cache.borrow() {
            return value.clone();
        }
        let value = compute();
        *cache.borrow_mut() = Some(value.clone());
        value
    }

    fn name_value(&self, input: &WebviewInput) -> String {
        let custom_title = input.get_webview_title().filter(|title| !title.is_empty());
        if let Some(custom_title) = custom_title {
            return custom_title;
        }

        Self::cached(&self.editor_name, || {
            self.services
                .custom_editor_label_service
                .get_name(&self.editor_resource)
                .unwrap_or_else(|| basename_of_label(&self.uri_label(&self.editor_resource, false)))
        })
    }

    fn other_custom_editor(other: &EditorInputOrUntyped) -> Option<(&WebviewInput, &CustomEditorInput)> {
        match other {
            EditorInputOrUntyped::Typed(other) => {
                let other = other.as_any().downcast_ref::<WebviewInput>()?;
                Some((other, other.kind_as::<CustomEditorInput>()?))
            }
            EditorInputOrUntyped::Untyped(_) => None,
        }
    }
}

impl WebviewInputKind for CustomEditorInput {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn attach(&self, input: &Rc<WebviewInput>) {
        self.register_listeners(input);
    }

    fn editor_type_id(&self, _input: &WebviewInput) -> String {
        Self::TYPE_ID.to_owned()
    }

    fn editor_id(&self, input: &WebviewInput) -> Option<String> {
        Some(input.view_type.clone())
    }

    fn capabilities(&self, input: &WebviewInput) -> EditorInputCapabilities {
        self.capabilities_value(input)
    }

    fn resource(&self, _input: &WebviewInput) -> Option<URI> {
        Some(self.editor_resource.clone())
    }

    fn get_name(&self, input: &WebviewInput) -> String {
        self.name_value(input)
    }

    fn get_description(&self, _input: &WebviewInput, verbosity: Option<Verbosity>) -> Option<String> {
        let dirname = ext_uri().dirname(&self.editor_resource);
        Some(match verbosity.unwrap_or(Verbosity::Medium) {
            Verbosity::Short => Self::cached(&self.short_description, || self.services.label_service.get_uri_basename_label(&dirname)),
            Verbosity::Long => Self::cached(&self.long_description, || self.uri_label(&dirname, false)),
            Verbosity::Medium => Self::cached(&self.medium_description, || self.uri_label(&dirname, true)),
        })
    }

    fn get_title(&self, input: &WebviewInput, verbosity: Option<Verbosity>) -> String {
        let custom_title = input.get_webview_title().filter(|title| !title.is_empty());
        if let Some(custom_title) = custom_title {
            return custom_title;
        }

        match verbosity {
            Some(Verbosity::Short) => Self::cached(&self.short_title, || self.name_value(input)),
            Some(Verbosity::Long) => Self::cached(&self.long_title, || self.uri_label(&self.editor_resource, false)),
            Some(Verbosity::Medium) | None => Self::cached(&self.medium_title, || self.uri_label(&self.editor_resource, true)),
        }
    }

    fn matches(&self, input: &WebviewInput, other: &EditorInputOrUntyped) -> bool {
        if matches_by_default(input, other) {
            return true;
        }
        match Self::other_custom_editor(other) {
            Some((other_input, other_custom)) => {
                std::ptr::eq(input, other_input) || (input.view_type == other_input.view_type && Self::ext_equal(&self.editor_resource, &other_custom.editor_resource))
            }
            None => false,
        }
    }

    fn copy(&self, input: &WebviewInput) -> EditorInputRef {
        CustomEditorInput::create(
            &self.services,
            CustomEditorInputInitInfo {
                resource: self.editor_resource.clone(),
                view_type: input.view_type.clone(),
                webview_title: input.get_webview_title(),
                preferred_name: None,
                icon_path: input.icon_path(),
            },
            input.group(),
            Some(CustomEditorInputCreateOptions { custom_classes: input.webview().options().custom_classes, old_resource: None }),
        )
    }

    fn is_readonly(&self, _input: &WebviewInput) -> Readonly {
        self.is_readonly_value()
    }

    fn is_dirty(&self, _input: &WebviewInput) -> bool {
        self.is_dirty_value()
    }

    fn save(&self, input: &WebviewInput, _group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        let Some(model) = self.model_ref.borrow().as_ref().map(|model_ref| model_ref.object.clone()) else {
            return Promise::resolved(None);
        };
        let resource = self.editor_resource.clone();
        let input: EditorInputRef = input.rc();
        model.save_custom_editor(options).then(move |target| {
            let target = target?; // save cancelled

            // Different URIs == untyped input returned to allow resolver to possibly resolve to a different editor type
            if !Self::ext_equal(&target, &resource) {
                return Some(EditorInputOrUntyped::Untyped(UntypedEditorInput::from_resource(target)));
            }

            Some(EditorInputOrUntyped::Typed(input))
        })
    }

    fn save_as(&self, input: &WebviewInput, group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        let Some(model) = self.model_ref.borrow().as_ref().map(|model_ref| model_ref.object.clone()) else {
            return Promise::resolved(None);
        };
        let dialog_path = self.editor_resource.clone();
        let file_dialog_service = self.services.file_dialog_service.clone();
        let input = input.rc();
        async_fn(async move {
            let target = file_dialog_service.pick_file_to_save(&dialog_path, options.as_ref().and_then(|options| options.available_file_systems.clone())).await?;
            let Some(target) = target else {
                return Ok(None); // save cancelled
            };

            if !model.save_custom_editor_as(&dialog_path, &target, options).await? {
                return Ok(None);
            }

            Ok(EditorInput::rename(&*input, group, &target).await?.map(|moved| moved.editor))
        })
    }

    fn revert(&self, input: &WebviewInput, _group: GroupIdentifier, options: Option<RevertOptions>) -> Promise<()> {
        let model = self.model_ref.borrow().as_ref().map(|model_ref| model_ref.object.clone());
        if let Some(model) = model {
            return model.revert(options);
        }
        self.default_dirty_state.set(Some(false));
        EditorInput::base(input).fire_did_change_dirty();
        Promise::resolved(())
    }

    fn resolve(&self, input: &WebviewInput) -> Promise<Option<Rc<dyn IEditorModel>>> {
        let input = input.rc();
        async_fn(async move {
            let this = input.kind_as::<CustomEditorInput>().expect("a entrada é um CustomEditorInput");
            this.lazy.resolve(&input).await?;

            if EditorInput::is_disposed(&*input) {
                return Ok(None);
            }

            if this.model_ref.borrow().is_none() {
                let old_capabilities = EditorInput::capabilities(&*input);
                let retained = this.services.custom_editor_service.models().try_retain(&this.editor_resource, &input.view_type);
                let Some(retained) = retained else {
                    return Err(Error::new("Assertion Failed: argument is undefined or null"));
                };
                let model_ref = retained.await?;
                let base = EditorInput::base(&*input);
                base.register(Rc::new(model_ref.clone()));
                *this.model_ref.borrow_mut() = Some(model_ref.clone());
                let weak = Rc::downgrade(&input);
                base.register(model_ref.object.on_did_change_dirty().listen(move |_| {
                    if let Some(input) = weak.upgrade() {
                        EditorInput::base(&*input).fire_did_change_dirty();
                    }
                }));
                let weak = Rc::downgrade(&input);
                base.register(model_ref.object.on_did_change_readonly().listen(move |_| {
                    if let Some(input) = weak.upgrade() {
                        EditorInput::base(&*input).fire_did_change_capabilities();
                    }
                }));
                // If we're loading untitled file data we should ensure it's dirty
                if this.untitled_document_data.is_some() {
                    this.default_dirty_state.set(Some(true));
                }
                if EditorInput::is_dirty(&*input) {
                    base.fire_did_change_dirty();
                }
                if EditorInput::capabilities(&*input) != old_capabilities {
                    base.fire_did_change_capabilities();
                }
            }

            Ok(None)
        })
    }

    fn rename(&self, _input: &WebviewInput, _group: GroupIdentifier, new_resource: &URI) -> Promise<Option<MoveResult>> {
        // We return an untyped editor input which can then be resolved in the editor service
        Promise::resolved(Some(MoveResult { editor: EditorInputOrUntyped::Untyped(UntypedEditorInput::from_resource(new_resource.clone())), options: None }))
    }

    fn to_untyped(&self, input: &WebviewInput, _options: Option<&crate::common::editor::editor_input::UntypedEditorOptions>) -> Option<UntypedEditorInput> {
        Some(UntypedEditorInput {
            resource: Some(self.editor_resource.clone()),
            options: Some(TextEditorOptions {
                base: EditorOptions { override_: Some(EditorOverride::Id(JsString::from(input.view_type.as_str()))), ..EditorOptions::default() },
                ..TextEditorOptions::default()
            }),
            ..UntypedEditorInput::default()
        })
    }

    fn get_selection(&self, _input: &WebviewInput) -> Option<Rc<dyn IEditorPaneSelection>> {
        let selection = self.navigation().and_then(|navigation| navigation.selection())?;
        Some(Rc::new(TextEditorPaneSelection::new(selection)))
    }

    fn apply_options(&self, _input: &WebviewInput, options: &TextEditorOptions, token: CancellationToken) -> Promise<()> {
        let navigation = self.navigation();
        let selection = options.selection;
        match (navigation, selection) {
            (Some(navigation), Some(selection)) if !token.is_cancellation_requested() => {
                let start_line_number = selection.start_line_number as i32;
                let start_column = selection.start_column as i32;
                let range = Range::new(
                    start_line_number,
                    start_column,
                    selection.end_line_number.map_or(start_line_number, |value| value as i32),
                    selection.end_column.map_or(start_column, |value| value as i32),
                );
                navigation.reveal_range(range, Some(Selection::from_range(&range, SelectionDirection::Ltr)), options.preserve_focus.unwrap_or(false), token)
            }
            _ => Promise::resolved(()),
        }
    }

    fn on_dispose(&self, _input: &WebviewInput) {
        self.lazy.dispose();
    }

    fn on_transfer(&self, _input: &WebviewInput, other: &Rc<WebviewInput>) {
        if let Some(other) = other.kind_as::<CustomEditorInput>() {
            self.lazy.transfer(&other.lazy);
            *other.move_handler.borrow_mut() = self.move_handler.borrow_mut().take();
        }
    }
}
