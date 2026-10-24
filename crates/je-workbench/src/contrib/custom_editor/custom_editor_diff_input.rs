//! Porte de `src/vs/workbench/contrib/customEditor/browser/customEditorDiffInput.ts`: as entradas de editor da comparação de
//! editores personalizados, `CustomEditorDiffInput` (a comparação em linha, com um webview só) e
//! `CustomEditorSideBySideDiffInput` (a comparação lado a lado, um webview por lado).
//!
//! Divergências:
//! - **Herança.** As duas são [`WebviewInputKind`] (ver a nota de herança de [`WebviewInput`]) que guardam o
//!   [`LazilyResolvedWebviewEditorInput`]; `new CustomEditorDiffInput(init, webview, ...)` é [`CustomEditorDiffInput::new`] e o
//!   `instanceof` é `webview_input.kind_as::<CustomEditorDiffInput>()`.
//! - **Injeção.** `@IThemeService`, `@IWebviewWorkbenchService`, `@IInstantiationService`, `@ICustomEditorService`,
//!   `@IFilesConfigurationService`, `@IFileDialogService` e `@IUndoRedoService` são a [`CustomEditorDiffInputServices`] (que
//!   também traz o `IWebviewService` do `invokeFunction` de `create`); o `IThemeService` é o [`IWebviewThemeService`].
//! - **DRY.** O estado do documento modificado (`_modelRef`, `isDirty`, `isReadonly`, `save`, `saveAs`, `revert` e o trecho de
//!   `resolve` que retém o modelo) é igual nas duas entradas; [`ModifiedDocument`] o guarda uma vez e cada entrada passa a
//!   diferença (o que `save` e `saveAs` devolvem quando o alvo muda, e se a entrada retém o modelo).
//! - `MutableDisposable<IReference<...>>` é a referência registrada no armazém da entrada (descartada com ela).
//! - `isEditorInput(otherInput)` é o `Typed` de [`EditorInputOrUntyped`], e `isResourceDiffEditorInput` e
//!   `isResourceEditorInput` são os métodos de [`UntypedEditorInput`].
//! - **Fora:** as `IEditorGroup` de `create` são o identificador do grupo (`input.updateGroup(group.id)`).

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::js_string::JsString;
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::dialogs::dialogs::IFileDialogService;
use je_platform::opener::editor_types::{EditorOptions, EditorOverride};
use je_platform::resources::ext_uri;
use je_platform::undo_redo::undo_redo::IUndoRedoService;

use super::custom_editor::{CustomEditorModelReference, ICustomEditorModel, ICustomEditorService};
use crate::common::editor::editor_input::{EditorInput, UntypedEditorOptions};
use crate::common::editor::editor_model::IEditorModel;
use crate::common::editor::{
    EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef, GroupIdentifier, RevertOptions, SaveOptions, TextEditorOptions, UntypedEditorInput, Verbosity,
};
use crate::contrib::webview::browser_webview::{IOverlayWebview, IWebviewService, WebviewInitInfo};
use crate::contrib::webview_panel::webview_editor_input::{IWebviewThemeService, WebviewIconPath, WebviewInput, WebviewInputInitInfo, WebviewInputKind};
use crate::contrib::webview_panel::webview_workbench_service::{IWebviewWorkbenchService, LazilyResolvedWebviewEditorInput};
use crate::services::files_configuration::files_configuration_service::{IFilesConfigurationService, Readonly};

/// `CustomEditorDiffInputInitInfo`.
#[derive(Clone)]
pub struct CustomEditorDiffInputInitInfo {
    pub original_resource: URI,
    pub modified_resource: URI,
    pub view_type: String,
    pub label: Option<String>,
    pub description: Option<String>,
    pub icon_path: Option<WebviewIconPath>,
}

/// `CustomEditorSideBySideDiffSide`: `'original' | 'modified'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomEditorSideBySideDiffSide {
    Original,
    Modified,
}

/// `CustomEditorSideBySideDiffInputInitInfo`.
#[derive(Clone)]
pub struct CustomEditorSideBySideDiffInputInitInfo {
    pub diff: CustomEditorDiffInputInitInfo,
    pub diff_id: String,
    pub side: CustomEditorSideBySideDiffSide,
}

/// Os serviços que os construtores e os `create` das entradas de comparação injetam (ver a nota do módulo).
pub struct CustomEditorDiffInputServices {
    pub theme_service: Rc<dyn IWebviewThemeService>,
    pub webview_workbench_service: Rc<dyn IWebviewWorkbenchService>,
    pub custom_editor_service: Rc<dyn ICustomEditorService>,
    pub files_configuration_service: Rc<dyn IFilesConfigurationService>,
    pub file_dialog_service: Rc<dyn IFileDialogService>,
    pub undo_redo_service: Rc<dyn IUndoRedoService>,
    pub webview_service: Rc<dyn IWebviewService>,
}

fn is_equal(a: &URI, b: &URI) -> bool {
    ext_uri().is_equal(Some(a), Some(b), false)
}

fn basename(resource: &URI) -> String {
    ext_uri().basename(resource, None)
}

/// `getCustomEditorSideBySideDiffInputResource(init)`.
fn side_by_side_resource(init: &CustomEditorSideBySideDiffInputInitInfo) -> &URI {
    match init.side {
        CustomEditorSideBySideDiffSide::Original => &init.diff.original_resource,
        CustomEditorSideBySideDiffSide::Modified => &init.diff.modified_resource,
    }
}

/// O que as duas entradas guardam do documento modificado (ver a nota do módulo).
struct ModifiedDocument {
    model_ref: RefCell<Option<CustomEditorModelReference>>,
    modified_resource: URI,
    services: Rc<CustomEditorDiffInputServices>,
}

impl ModifiedDocument {
    fn new(modified_resource: URI, services: Rc<CustomEditorDiffInputServices>) -> Self {
        Self { model_ref: RefCell::new(None), modified_resource, services }
    }

    fn model(&self) -> Option<Rc<dyn ICustomEditorModel>> {
        self.model_ref.borrow().as_ref().map(|model_ref| model_ref.object.clone())
    }

    fn is_readonly(&self) -> Readonly {
        match self.model() {
            None => self.services.files_configuration_service.is_readonly(&self.modified_resource, None),
            Some(model) => model.is_readonly(),
        }
    }

    fn is_dirty(&self) -> bool {
        self.model().is_some_and(|model| model.is_dirty())
    }

    /// `save(groupId, options)`: `to_untyped` é o que se devolve quando o alvo é outro recurso.
    fn save(&self, input: &WebviewInput, options: Option<SaveOptions>, to_untyped: impl Fn(URI) -> UntypedEditorInput + 'static) -> Promise<Option<EditorInputOrUntyped>> {
        let Some(model) = self.model() else {
            return Promise::resolved(None);
        };
        let modified_resource = self.modified_resource.clone();
        let input: EditorInputRef = input.rc();
        model.save_custom_editor(options).then(move |target| {
            let target = target?;
            if !is_equal(&target, &modified_resource) {
                return Some(EditorInputOrUntyped::Untyped(to_untyped(target)));
            }
            Some(EditorInputOrUntyped::Typed(input))
        })
    }

    /// `saveAs(groupId, options)`.
    fn save_as(&self, options: Option<SaveOptions>, to_untyped: impl Fn(URI) -> UntypedEditorInput + 'static) -> Promise<Option<EditorInputOrUntyped>> {
        let Some(model) = self.model() else {
            return Promise::resolved(None);
        };
        let modified_resource = self.modified_resource.clone();
        let file_dialog_service = self.services.file_dialog_service.clone();
        async_fn(async move {
            let target = file_dialog_service.pick_file_to_save(&modified_resource, options.as_ref().and_then(|options| options.available_file_systems.clone())).await?;
            let Some(target) = target else {
                return Ok(None);
            };

            if !model.save_custom_editor_as(&modified_resource, &target, options).await? {
                return Ok(None);
            }

            Ok(Some(EditorInputOrUntyped::Untyped(to_untyped(target))))
        })
    }

    fn revert(&self, options: Option<RevertOptions>) -> Promise<()> {
        match self.model() {
            Some(model) => model.revert(options),
            None => Promise::resolved(()),
        }
    }

    /// O trecho de `resolve()` depois do `super.resolve()`: retém o modelo do documento modificado (quando `retain`).
    fn resolve(self: &Rc<Self>, input: &Rc<WebviewInput>, retain: bool) -> Promise<()> {
        let (this, input) = (self.clone(), input.clone());
        async_fn(async move {
            if EditorInput::is_disposed(&*input) {
                return Ok(());
            }

            if retain && this.model_ref.borrow().is_none() {
                let model_ref = this.services.custom_editor_service.models().try_retain(&this.modified_resource, &input.view_type);
                if let Some(model_ref) = model_ref {
                    let old_capabilities = EditorInput::capabilities(&*input);
                    let retained_model_ref = model_ref.await?;
                    if EditorInput::is_disposed(&*input) {
                        retained_model_ref.dispose();
                        return Ok(());
                    }
                    let base = EditorInput::base(&*input);
                    base.register(Rc::new(retained_model_ref.clone()));
                    *this.model_ref.borrow_mut() = Some(retained_model_ref.clone());
                    let weak = Rc::downgrade(&input);
                    base.register(retained_model_ref.object.on_did_change_dirty().listen(move |_| {
                        if let Some(input) = weak.upgrade() {
                            EditorInput::base(&*input).fire_did_change_dirty();
                        }
                    }));
                    let weak = Rc::downgrade(&input);
                    base.register(retained_model_ref.object.on_did_change_readonly().listen(move |_| {
                        if let Some(input) = weak.upgrade() {
                            EditorInput::base(&*input).fire_did_change_capabilities();
                        }
                    }));
                    if EditorInput::is_dirty(&*input) {
                        base.fire_did_change_dirty();
                    }
                    if EditorInput::capabilities(&*input) != old_capabilities {
                        base.fire_did_change_capabilities();
                    }
                }
            }

            Ok(())
        })
    }
}

/// O `readonly` e o aviso do `onDidChangeReadonly` de `IFilesConfigurationService`, que as duas entradas registram no
/// construtor (`this._register(this.filesConfigurationService.onDidChangeReadonly(() => this._onDidChangeCapabilities.fire()))`).
fn register_readonly_listener(services: &CustomEditorDiffInputServices, input: &Rc<WebviewInput>) {
    let weak = Rc::downgrade(input);
    EditorInput::base(&**input).register(services.files_configuration_service.on_did_change_readonly().listen(move |_| {
        if let Some(input) = weak.upgrade() {
            EditorInput::base(&*input).fire_did_change_capabilities();
        }
    }));
}

/// `capabilities` das duas entradas.
fn diff_capabilities(readonly: &Readonly) -> EditorInputCapabilities {
    let mut capabilities = EditorInputCapabilities::SINGLETON | EditorInputCapabilities::CAN_DROP_INTO_EDITOR;
    if readonly.is_readonly() {
        capabilities |= EditorInputCapabilities::READONLY;
    }
    capabilities
}

/// `getTitle(verbosity)` das duas entradas: `localize('{0} ({1})', name, description)` quando há descrição.
fn title_with_description(name: String, description: Option<String>, key: &str) -> String {
    match description.filter(|description| !description.is_empty()) {
        Some(description) => localize(key, "{0} ({1})", &[name.as_str(), description.as_str()]),
        None => name,
    }
}

pub struct CustomEditorDiffInput {
    lazy: LazilyResolvedWebviewEditorInput,
    services: Rc<CustomEditorDiffInputServices>,
    init: CustomEditorDiffInputInitInfo,
    document: Rc<ModifiedDocument>,
}

impl CustomEditorDiffInput {
    /// `CustomEditorDiffInput.typeId`.
    pub const TYPE_ID: &'static str = "workbench.editors.customDiffEditor";

    /// `static create(instantiationService, init, group)`.
    pub fn create(services: &Rc<CustomEditorDiffInputServices>, init: CustomEditorDiffInputInitInfo, group: Option<GroupIdentifier>) -> Rc<WebviewInput> {
        let webview = services.webview_service.create_webview_overlay(WebviewInitInfo {
            provided_view_type: Some(init.view_type.clone()),
            title: init.label.clone(),
            ..WebviewInitInfo::default()
        });

        let input = Self::new(init, webview, services.clone());
        if let Some(group) = group {
            input.update_group(group);
        }

        input
    }

    /// `new CustomEditorDiffInput(init, webview, ...services)`.
    pub fn new(init: CustomEditorDiffInputInitInfo, webview: Rc<dyn IOverlayWebview>, services: Rc<CustomEditorDiffInputServices>) -> Rc<WebviewInput> {
        let kind = Self {
            lazy: LazilyResolvedWebviewEditorInput::new(services.webview_workbench_service.clone()),
            services: services.clone(),
            document: Rc::new(ModifiedDocument::new(init.modified_resource.clone(), services.clone())),
            init: init.clone(),
        };
        WebviewInput::with_kind(
            WebviewInputInitInfo {
                provided_id: Some(init.view_type.clone()),
                view_type: init.view_type,
                name: init.label.unwrap_or_default(),
                icon_path: init.icon_path,
            },
            webview,
            services.theme_service.clone(),
            Rc::new(kind),
        )
    }

    /// `originalResource`.
    pub fn original_resource(&self) -> &URI {
        &self.init.original_resource
    }

    /// `modifiedResource`.
    pub fn modified_resource(&self) -> &URI {
        &self.init.modified_resource
    }

    /// `undo()`.
    pub fn undo(&self) -> Promise<()> {
        self.services.undo_redo_service.undo(&self.init.modified_resource)
    }

    /// `redo()`.
    pub fn redo(&self) -> Promise<()> {
        self.services.undo_redo_service.redo(&self.init.modified_resource)
    }

    /// `toUntypedWithModifiedResource(modifiedResource)`.
    fn to_untyped_with_modified_resource(init: &CustomEditorDiffInputInitInfo, modified_resource: URI) -> UntypedEditorInput {
        UntypedEditorInput {
            original: Some(Box::new(UntypedEditorInput::from_resource(init.original_resource.clone()))),
            modified: Some(Box::new(UntypedEditorInput::from_resource(modified_resource))),
            label: init.label.clone(),
            description: init.description.clone(),
            options: Some(TextEditorOptions {
                base: EditorOptions { override_: Some(EditorOverride::Id(JsString::from(init.view_type.as_str()))), ..EditorOptions::default() },
                ..TextEditorOptions::default()
            }),
            ..UntypedEditorInput::default()
        }
    }
}

impl WebviewInputKind for CustomEditorDiffInput {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn attach(&self, input: &Rc<WebviewInput>) {
        register_readonly_listener(&self.services, input);
    }

    fn editor_type_id(&self, _input: &WebviewInput) -> String {
        Self::TYPE_ID.to_owned()
    }

    fn editor_id(&self, input: &WebviewInput) -> Option<String> {
        Some(input.view_type.clone())
    }

    fn capabilities(&self, _input: &WebviewInput) -> EditorInputCapabilities {
        diff_capabilities(&self.document.is_readonly())
    }

    fn resource(&self, _input: &WebviewInput) -> Option<URI> {
        Some(self.init.modified_resource.clone())
    }

    fn get_name(&self, _input: &WebviewInput) -> String {
        self.init.label.clone().unwrap_or_else(|| {
            localize("customEditorDiffLabel", "{0} - {1}", &[basename(&self.init.original_resource).as_str(), basename(&self.init.modified_resource).as_str()])
        })
    }

    fn get_description(&self, _input: &WebviewInput, _verbosity: Option<Verbosity>) -> Option<String> {
        self.init.description.clone()
    }

    fn get_title(&self, input: &WebviewInput, verbosity: Option<Verbosity>) -> String {
        title_with_description(self.get_name(input), self.get_description(input, verbosity), "customEditorDiffTitle")
    }

    fn is_readonly(&self, _input: &WebviewInput) -> Readonly {
        self.document.is_readonly()
    }

    fn is_dirty(&self, _input: &WebviewInput) -> bool {
        self.document.is_dirty()
    }

    fn matches(&self, input: &WebviewInput, other_input: &EditorInputOrUntyped) -> bool {
        match other_input {
            EditorInputOrUntyped::Typed(other) => {
                let Some(other) = other.as_any().downcast_ref::<WebviewInput>() else {
                    return false;
                };
                if std::ptr::eq(input, other) {
                    return true;
                }
                match other.kind_as::<CustomEditorDiffInput>() {
                    Some(other_diff) => {
                        input.view_type == other.view_type
                            && is_equal(&self.init.original_resource, &other_diff.init.original_resource)
                            && is_equal(&self.init.modified_resource, &other_diff.init.modified_resource)
                    }
                    None => false,
                }
            }
            EditorInputOrUntyped::Untyped(other) => {
                if !other.is_resource_diff_editor_input() {
                    return false;
                }
                let override_matches = matches!(
                    other.options.as_ref().and_then(|options| options.base.override_.as_ref()),
                    Some(EditorOverride::Id(id)) if id.to_string_lossy() == input.view_type
                );
                let resource_of = |side: &Option<Box<UntypedEditorInput>>| side.as_ref().and_then(|side| side.resource.clone());
                override_matches
                    && resource_of(&other.original).is_some_and(|resource| is_equal(&self.init.original_resource, &resource))
                    && resource_of(&other.modified).is_some_and(|resource| is_equal(&self.init.modified_resource, &resource))
            }
        }
    }

    fn copy(&self, _input: &WebviewInput) -> EditorInputRef {
        CustomEditorDiffInput::create(&self.services, self.init.clone(), None)
    }

    fn save(&self, input: &WebviewInput, _group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        let init = self.init.clone();
        self.document.save(input, options, move |target| Self::to_untyped_with_modified_resource(&init, target))
    }

    fn save_as(&self, _input: &WebviewInput, _group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        let init = self.init.clone();
        self.document.save_as(options, move |target| Self::to_untyped_with_modified_resource(&init, target))
    }

    fn revert(&self, _input: &WebviewInput, _group: GroupIdentifier, options: Option<RevertOptions>) -> Promise<()> {
        self.document.revert(options)
    }

    fn resolve(&self, input: &WebviewInput) -> Promise<Option<Rc<dyn IEditorModel>>> {
        let (input, document) = (input.rc(), self.document.clone());
        let lazy_resolved = self.lazy.resolve(&input);
        async_fn(async move {
            lazy_resolved.await?;
            document.resolve(&input, true).await?;
            Ok(None)
        })
    }

    fn to_untyped(&self, _input: &WebviewInput, _options: Option<&UntypedEditorOptions>) -> Option<UntypedEditorInput> {
        Some(Self::to_untyped_with_modified_resource(&self.init, self.init.modified_resource.clone()))
    }

    fn diff_resources(&self, _input: &WebviewInput) -> Option<(URI, URI)> {
        Some((self.init.original_resource.clone(), self.init.modified_resource.clone()))
    }

    fn on_dispose(&self, _input: &WebviewInput) {
        self.lazy.dispose();
    }

    fn on_transfer(&self, _input: &WebviewInput, other: &Rc<WebviewInput>) {
        if let Some(other) = other.kind_as::<CustomEditorDiffInput>() {
            self.lazy.transfer(&other.lazy);
        }
    }
}

pub struct CustomEditorSideBySideDiffInput {
    lazy: LazilyResolvedWebviewEditorInput,
    services: Rc<CustomEditorDiffInputServices>,
    init: CustomEditorSideBySideDiffInputInitInfo,
    document: Rc<ModifiedDocument>,
}

impl CustomEditorSideBySideDiffInput {
    /// `CustomEditorSideBySideDiffInput.typeId`.
    pub const TYPE_ID: &'static str = "workbench.editors.customSideBySideDiffEditor";

    /// `static create(instantiationService, init, group)`.
    pub fn create(services: &Rc<CustomEditorDiffInputServices>, init: CustomEditorSideBySideDiffInputInitInfo, group: Option<GroupIdentifier>) -> Rc<WebviewInput> {
        let webview = services.webview_service.create_webview_overlay(WebviewInitInfo {
            provided_view_type: Some(init.diff.view_type.clone()),
            title: Some(basename(side_by_side_resource(&init))),
            ..WebviewInitInfo::default()
        });

        let input = Self::new(init, webview, services.clone());
        if let Some(group) = group {
            input.update_group(group);
        }

        input
    }

    /// `new CustomEditorSideBySideDiffInput(init, webview, ...services)`.
    pub fn new(init: CustomEditorSideBySideDiffInputInitInfo, webview: Rc<dyn IOverlayWebview>, services: Rc<CustomEditorDiffInputServices>) -> Rc<WebviewInput> {
        let name = basename(side_by_side_resource(&init));
        let kind = Self {
            lazy: LazilyResolvedWebviewEditorInput::new(services.webview_workbench_service.clone()),
            services: services.clone(),
            document: Rc::new(ModifiedDocument::new(init.diff.modified_resource.clone(), services.clone())),
            init: init.clone(),
        };
        WebviewInput::with_kind(
            WebviewInputInitInfo { provided_id: Some(init.diff.view_type.clone()), view_type: init.diff.view_type, name, icon_path: init.diff.icon_path },
            webview,
            services.theme_service.clone(),
            Rc::new(kind),
        )
    }

    /// `resource`.
    fn resource_value(&self) -> &URI {
        side_by_side_resource(&self.init)
    }

    /// `originalResource`.
    pub fn original_resource(&self) -> &URI {
        &self.init.diff.original_resource
    }

    /// `modifiedResource`.
    pub fn modified_resource(&self) -> &URI {
        &self.init.diff.modified_resource
    }

    /// `side`.
    pub fn side(&self) -> CustomEditorSideBySideDiffSide {
        self.init.side
    }

    /// `diffId`.
    pub fn diff_id(&self) -> &str {
        &self.init.diff_id
    }

    /// `undo()`.
    pub fn undo(&self) -> Promise<()> {
        self.services.undo_redo_service.undo(&self.init.diff.modified_resource)
    }

    /// `redo()`.
    pub fn redo(&self) -> Promise<()> {
        self.services.undo_redo_service.redo(&self.init.diff.modified_resource)
    }

    fn is_readonly_value(&self) -> Readonly {
        if self.init.side == CustomEditorSideBySideDiffSide::Original {
            return Readonly::Bool(true);
        }
        self.document.is_readonly()
    }
}

impl WebviewInputKind for CustomEditorSideBySideDiffInput {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn attach(&self, input: &Rc<WebviewInput>) {
        register_readonly_listener(&self.services, input);
    }

    fn editor_type_id(&self, _input: &WebviewInput) -> String {
        Self::TYPE_ID.to_owned()
    }

    fn editor_id(&self, input: &WebviewInput) -> Option<String> {
        Some(input.view_type.clone())
    }

    fn capabilities(&self, _input: &WebviewInput) -> EditorInputCapabilities {
        diff_capabilities(&self.is_readonly_value())
    }

    fn resource(&self, _input: &WebviewInput) -> Option<URI> {
        Some(self.resource_value().clone())
    }

    fn get_name(&self, _input: &WebviewInput) -> String {
        basename(self.resource_value())
    }

    fn get_description(&self, _input: &WebviewInput, _verbosity: Option<Verbosity>) -> Option<String> {
        self.init.diff.description.clone()
    }

    fn get_title(&self, input: &WebviewInput, verbosity: Option<Verbosity>) -> String {
        title_with_description(self.get_name(input), self.get_description(input, verbosity), "customEditorSideBySideDiffTitle")
    }

    fn is_readonly(&self, _input: &WebviewInput) -> Readonly {
        self.is_readonly_value()
    }

    fn is_dirty(&self, _input: &WebviewInput) -> bool {
        self.init.side == CustomEditorSideBySideDiffSide::Modified && self.document.is_dirty()
    }

    fn matches(&self, input: &WebviewInput, other_input: &EditorInputOrUntyped) -> bool {
        match other_input {
            EditorInputOrUntyped::Typed(other) => {
                let Some(other) = other.as_any().downcast_ref::<WebviewInput>() else {
                    return false;
                };
                if std::ptr::eq(input, other) {
                    return true;
                }
                match other.kind_as::<CustomEditorSideBySideDiffInput>() {
                    Some(other_side) => {
                        EditorInput::editor_id(input) == EditorInput::editor_id(other)
                            && self.init.side == other_side.init.side
                            && is_equal(&self.init.diff.original_resource, &other_side.init.diff.original_resource)
                            && is_equal(&self.init.diff.modified_resource, &other_side.init.diff.modified_resource)
                    }
                    None => false,
                }
            }
            EditorInputOrUntyped::Untyped(other) => {
                if !other.is_resource_editor_input() {
                    return false;
                }
                other.resource.as_ref().is_some_and(|resource| is_equal(self.resource_value(), resource))
            }
        }
    }

    fn copy(&self, _input: &WebviewInput) -> EditorInputRef {
        CustomEditorSideBySideDiffInput::create(&self.services, self.init.clone(), None)
    }

    fn save(&self, input: &WebviewInput, _group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        self.document.save(input, options, UntypedEditorInput::from_resource)
    }

    fn save_as(&self, _input: &WebviewInput, _group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        self.document.save_as(options, UntypedEditorInput::from_resource)
    }

    fn revert(&self, _input: &WebviewInput, _group: GroupIdentifier, options: Option<RevertOptions>) -> Promise<()> {
        self.document.revert(options)
    }

    fn resolve(&self, input: &WebviewInput) -> Promise<Option<Rc<dyn IEditorModel>>> {
        let (input, document) = (input.rc(), self.document.clone());
        let retain = self.init.side == CustomEditorSideBySideDiffSide::Modified;
        let lazy_resolved = self.lazy.resolve(&input);
        async_fn(async move {
            lazy_resolved.await?;
            document.resolve(&input, retain).await?;
            Ok(None)
        })
    }

    fn to_untyped(&self, _input: &WebviewInput, _options: Option<&UntypedEditorOptions>) -> Option<UntypedEditorInput> {
        Some(UntypedEditorInput::from_resource(self.resource_value().clone()))
    }

    fn on_dispose(&self, _input: &WebviewInput) {
        self.lazy.dispose();
    }

    fn on_transfer(&self, _input: &WebviewInput, other: &Rc<WebviewInput>) {
        if let Some(other) = other.kind_as::<CustomEditorSideBySideDiffInput>() {
            self.lazy.transfer(&other.lazy);
        }
    }
}
