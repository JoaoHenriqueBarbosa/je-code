//! Porte de `src/vs/workbench/common/editor/editorInput.ts`: o `EditorInput`, a classe abstrata de que todas as entradas
//! de editor herdam (mais o `AbstractEditorInput` de `common/editor.ts`, a marca de `isEditorInput`).
//!
//! Divergências:
//! - **Herança.** `EditorInput` é o trait [`EditorInput`] (com a implementação padrão de cada método da classe) e o estado
//!   comum (o `Disposable` base, os emissores `_onDidChangeDirty`, `_onDidChangeLabel`, `_onDidChangeCapabilities` e
//!   `_onWillDispose`) é a struct [`EditorInputBase`], que cada entrada concreta guarda e devolve por
//!   [`EditorInput::base`]. O que uma subclasse sobrescreve é o método do trait; o `super.x()` é a chamada ao mesmo método
//!   do tipo de que ela "herda" (guardado como campo) ou à lógica compartilhada da base.
//! - **`this`.** Os métodos que devolvem a própria entrada (`save`, `saveAs`, `copy`) precisam do `Rc` dela: a base guarda
//!   um `Weak` para o objeto, criado por `Rc::new_cyclic` no construtor da entrada concreta ([`EditorInputBase::new`]).
//! - **`Disposable`.** A base usa um `DisposableStore` (o `_store` do original), porque o `isDisposed()` lê
//!   `this._store.isDisposed`. O `dispose()` da entrada concreta chama [`EditorInputBase::dispose`] (o `super.dispose()`).
//! - **`typeId`** é `editor_type_id` (o `type_id` colidiria com o `Any::type_id`). **`resolve`** devolve o modelo como
//!   [`IEditorModel`] (todo modelo resolvido de editor herda de `EditorModel`), ou `None` (o `null` do original).
//! - **`getTelemetryDescriptor`** (`{ [key: string]: unknown }`) é um [`Dynamic`] objeto. **`canMove`** (`true | string`) é
//!   [`CanMove`]. **`isReadonly`** (`boolean | IMarkdownString`) é o [`Readonly`] do serviço de configuração de arquivos.
//! - **Formas estruturais.** As checagens `isSideBySideEditorInput`, `isDiffEditorInput`, `isEditorInputWithPreferredResource`
//!   e `isEditorInputWithDiffResources` olham propriedades que só algumas entradas têm; aqui são os métodos
//!   [`EditorInput::side_by_side_editors`], [`EditorInput::diff_editors`], [`EditorInput::preferred_resource`] e
//!   [`EditorInput::diff_resources`], que devolvem `None` por padrão.

use std::any::Any;
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, DisposableStore, id_of, id_of_ref};
use je_base::lifecycle::IDisposable as _;
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_platform::dialogs::ConfirmResult;
use je_platform::opener::editor_types::EditorOverride;
use je_platform::resources::ext_uri;

use crate::common::editor::editor_model::IEditorModel;
use crate::common::editor::{
    EditorIdentifier, EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef, EditorResourceAccessor, GroupIdentifier, IEditorDescriptor, MoveResult,
    RevertOptions, SaveOptions, UntypedEditorInput, Verbosity,
};
use crate::services::files_configuration::files_configuration_service::Readonly;

/// `IEditorCloseHandler`.
pub trait IEditorCloseHandler {
    /// If `true`, will call into the `confirm` method to ask for confirmation before closing the editor.
    fn show_confirm(&self) -> bool;

    /// Allows an editor to control what should happen when the editor (or a list of editor of the same kind) is being
    /// closed.
    ///
    /// By default a file specific dialog will open if the editor is dirty and not in the process of saving.
    ///
    /// If the editor is not dealing with files or another condition should be used besides dirty state, this method should
    /// be implemented to show a different dialog.
    ///
    /// `editors`: all editors of the same kind that are being closed. Should be used to show a combined dialog.
    fn confirm(&self, editors: &[EditorIdentifier]) -> Promise<ConfirmResult>;
}

/// `IUntypedEditorOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UntypedEditorOptions {
    /// Implementations should try to preserve as much view state as possible from the typed input based on the group the
    /// editor is opened.
    pub preserve_view_state: Option<GroupIdentifier>,
    /// Implementations should preserve the original resource of the typed input and not alter it.
    pub preserve_resource: Option<bool>,
}

/// `ThemeIcon | URI`: o ícone que representa uma entrada de editor.
#[derive(Clone, Debug)]
pub enum EditorInputIcon {
    Theme(ThemeIcon),
    Uri(URI),
}

/// `true | string`: o resultado de `canMove`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanMove {
    /// `true`: the editor can be moved to the target group.
    Yes,
    /// A message to show to the user because the editor cannot be moved.
    No(String),
}

/// O estado comum de toda entrada de editor: o `Disposable` base e os emissores que `EditorInput` declara.
pub struct EditorInputBase {
    this: Weak<dyn EditorInput>,
    store: Rc<DisposableStore>,
    on_did_change_dirty: Rc<Emitter<()>>,
    on_did_change_label: Rc<Emitter<()>>,
    on_did_change_capabilities: Rc<Emitter<()>>,
    on_will_dispose: Rc<Emitter<()>>,
}

impl EditorInputBase {
    /// `constructor()`: `this` é o `Weak` da entrada concreta, vindo do `Rc::new_cyclic` dela.
    pub fn new(this: Weak<dyn EditorInput>) -> Self {
        let store = DisposableStore::new();
        let on_did_change_dirty = Emitter::new();
        let on_did_change_label = Emitter::new();
        let on_did_change_capabilities = Emitter::new();
        let on_will_dispose = Emitter::new();
        store.add(on_did_change_dirty.clone());
        store.add(on_did_change_label.clone());
        store.add(on_did_change_capabilities.clone());
        store.add(on_will_dispose.clone());
        Self { this, store, on_did_change_dirty, on_did_change_label, on_did_change_capabilities, on_will_dispose }
    }

    /// A própria entrada (o `this` do original).
    pub fn this(&self) -> EditorInputRef {
        self.this.upgrade().expect("a entrada de editor já foi liberada")
    }

    /// `_register(o)`.
    pub fn register(&self, o: DisposableRef) -> DisposableRef {
        self.store.add(o)
    }

    /// `this._onDidChangeDirty.fire()`.
    pub fn fire_did_change_dirty(&self) {
        self.on_did_change_dirty.fire(());
    }

    /// `this._onDidChangeLabel.fire()`.
    pub fn fire_did_change_label(&self) {
        self.on_did_change_label.fire(());
    }

    /// `this._onDidChangeCapabilities.fire()`.
    pub fn fire_did_change_capabilities(&self) {
        self.on_did_change_capabilities.fire(());
    }

    /// `isDisposed()`: `this._store.isDisposed`.
    pub fn is_disposed(&self) -> bool {
        self.store.is_disposed()
    }

    /// `dispose()` de `EditorInput`: avisa `onWillDispose` (uma vez) e descarta o que a entrada registrou.
    pub fn dispose(&self) {
        if !self.is_disposed() {
            self.on_will_dispose.fire(());
        }

        self.store.dispose();
    }
}

/// O corpo de `EditorInput.matches`: o `super.matches(other)` de quem sobrescreve o método (um trait não deixa chamar o
/// padrão que o `impl` sobrescreveu).
pub fn matches_by_default<T: EditorInput + ?Sized>(input: &T, other_input: &EditorInputOrUntyped) -> bool {
    match other_input {
        // Typed inputs: via === check
        EditorInputOrUntyped::Typed(other) => id_of_ref(input) == id_of(other),

        // Untyped inputs: go into properties
        EditorInputOrUntyped::Untyped(untyped) => {
            let other_input_editor_id = untyped.options.as_ref().and_then(|options| options.base.override_.as_ref());

            // If the overrides are both defined and don't match that means they're separate inputs
            if let (Some(own_editor_id), Some(other_input_editor_id)) = (input.editor_id(), other_input_editor_id)
                && !matches!(other_input_editor_id, EditorOverride::Id(id) if id.to_string_lossy() == own_editor_id)
            {
                return false;
            }

            ext_uri().is_equal(input.resource().as_ref(), EditorResourceAccessor::get_canonical_uri(Some(other_input), None).as_ref(), false)
        }
    }
}

/// `EditorInput`: editor inputs are lightweight objects that can be passed to the workbench API to open inside the editor
/// part. Each editor input is mapped to an editor that is capable of opening it through the Platform facade.
pub trait EditorInput: je_base::lifecycle::IDisposable {
    /// O estado comum (ver [`EditorInputBase`]).
    fn base(&self) -> &EditorInputBase;

    /// Para o `instanceof` do original: o tipo concreto da entrada.
    fn as_any(&self) -> &dyn Any;

    /// `typeId`: unique type identifier for this input. Every editor input of the same class should share the same type
    /// identifier. The type identifier is used for example for serialising/deserialising editor inputs via the
    /// serialisers of the `EditorInputFactoryRegistry`.
    fn editor_type_id(&self) -> String;

    /// `resource`: returns the optional associated resource of this input.
    ///
    /// This resource should be unique for all editors of the same kind and input and is often used to identify the editor
    /// input among others.
    ///
    /// **Note:** DO NOT use this property for anything but identity checks. DO NOT use this property to present as label
    /// to the user. Please refer to `EditorResourceAccessor` documentation in that case.
    fn resource(&self) -> Option<URI>;

    /// A própria entrada como handle compartilhado.
    fn this(&self) -> EditorInputRef {
        self.base().this()
    }

    /// `onDidChangeDirty`: triggered when this input changes its dirty state.
    fn on_did_change_dirty(&self) -> Event<()> {
        self.base().on_did_change_dirty.event()
    }

    /// `onDidChangeLabel`: triggered when this input changes its label.
    fn on_did_change_label(&self) -> Event<()> {
        self.base().on_did_change_label.event()
    }

    /// `onDidChangeCapabilities`: triggered when this input changes its capabilities.
    fn on_did_change_capabilities(&self) -> Event<()> {
        self.base().on_did_change_capabilities.event()
    }

    /// `onWillDispose`: triggered when this input is about to be disposed.
    fn on_will_dispose(&self) -> Event<()> {
        self.base().on_will_dispose.event()
    }

    /// `closeHandler`: optional, subclasses can override to implement custom confirmation on close behavior.
    fn close_handler(&self) -> Option<Rc<dyn IEditorCloseHandler>> {
        None
    }

    /// `editorId`: identifies the type of editor this input represents. This ID is registered with the
    /// `EditorResolverService` to allow for resolving an untyped input to a typed one.
    fn editor_id(&self) -> Option<String> {
        None
    }

    /// `capabilities`: the capabilities of the input.
    fn capabilities(&self) -> EditorInputCapabilities {
        EditorInputCapabilities::READONLY
    }

    /// Figure out if the input has the provided capability.
    fn has_capability(&self, capability: EditorInputCapabilities) -> bool {
        if capability == EditorInputCapabilities::NONE {
            return self.capabilities() == EditorInputCapabilities::NONE;
        }

        self.capabilities().intersects(capability)
    }

    fn is_readonly(&self) -> Readonly {
        Readonly::Bool(self.has_capability(EditorInputCapabilities::READONLY))
    }

    /// Returns the display name of this input.
    fn get_name(&self) -> String {
        format!("Editor {}", self.editor_type_id())
    }

    /// Returns the display description of this input.
    fn get_description(&self, _verbosity: Option<Verbosity>) -> Option<String> {
        None
    }

    /// Returns the display title of this input.
    fn get_title(&self, _verbosity: Option<Verbosity>) -> String {
        self.get_name()
    }

    /// Returns the extra classes to apply to the label of this input.
    fn get_label_extra_classes(&self) -> Vec<String> {
        Vec::new()
    }

    /// Returns the aria label to be read out by a screen reader.
    fn get_aria_label(&self) -> String {
        self.get_title(Some(Verbosity::Short))
    }

    /// Returns the icon which represents this editor input. If `None`, the default icon will be used.
    fn get_icon(&self) -> Option<EditorInputIcon> {
        None
    }

    /// Returns a descriptor suitable for telemetry events. Subclasses should extend if they can contribute.
    fn get_telemetry_descriptor(&self) -> Dynamic {
        Dynamic::object([("typeId", Dynamic::string(&self.editor_type_id()))])
    }

    /// Returns if this input is dirty or not.
    fn is_dirty(&self) -> bool {
        false
    }

    /// Returns if the input has unsaved changes.
    fn is_modified(&self) -> bool {
        self.is_dirty()
    }

    /// Returns if this input is currently being saved or soon to be saved. Based on this assumption the editor may for
    /// example decide to not signal the dirty state to the user assuming that the save is scheduled to happen anyway.
    fn is_saving(&self) -> bool {
        false
    }

    /// Returns a type of `IDisposable` that represents the resolved input. Subclasses should override to provide a
    /// meaningful model or return `None` (`null`) if the editor does not require a model.
    fn resolve(&self) -> Promise<Option<Rc<dyn IEditorModel>>> {
        Promise::resolved(None)
    }

    /// Saves the editor. The provided groupId helps implementors to e.g. preserve view state of the editor and re-open it
    /// in the correct group after saving.
    ///
    /// Returns the resulting editor input (typically the same) of this operation or `None` to indicate that the operation
    /// failed or was canceled.
    fn save(&self, _group: GroupIdentifier, _options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        Promise::resolved(Some(EditorInputOrUntyped::Typed(self.this())))
    }

    /// Saves the editor to a different location. The provided `group` helps implementors to e.g. preserve view state of
    /// the editor and re-open it in the correct group after saving.
    ///
    /// Returns the resulting editor input (typically a different one) of this operation or `None` to indicate that the
    /// operation failed or was canceled.
    fn save_as(&self, _group: GroupIdentifier, _options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        Promise::resolved(Some(EditorInputOrUntyped::Typed(self.this())))
    }

    /// Reverts this input from the provided group.
    fn revert(&self, _group: GroupIdentifier, _options: Option<RevertOptions>) -> Promise<()> {
        Promise::resolved(())
    }

    /// Called to determine how to handle a resource that is renamed that matches the editors resource (or is a child of).
    ///
    /// Implementors are free to not implement this method to signal no intent to participate. If an editor is returned
    /// though, it will replace the current one with that editor and optional options.
    fn rename(&self, _group: GroupIdentifier, _target: &URI) -> Promise<Option<MoveResult>> {
        Promise::resolved(None)
    }

    /// Returns a copy of the current editor input. Used when we can't just reuse the input.
    fn copy(&self) -> EditorInputRef {
        self.this()
    }

    /// Indicates if this editor can be moved to another group. By default editors can freely be moved around groups. If an
    /// editor cannot be moved, a message should be returned to show to the user.
    fn can_move(&self, _source_group: GroupIdentifier, _target_group: GroupIdentifier) -> CanMove {
        CanMove::Yes
    }

    /// Indicates if this editor can be reopened after being closed. By default editors can be reopened. Subclasses can
    /// override to prevent this.
    fn can_reopen(&self) -> bool {
        true
    }

    /// Returns if the other object matches this input.
    fn matches(&self, other_input: &EditorInputOrUntyped) -> bool {
        matches_by_default(self, other_input)
    }

    /// If a editor was registered onto multiple editor panes, this method will be asked to return the preferred one to use.
    ///
    /// `editor_panes`: a list of editor pane descriptors that are candidates for the editor to open in.
    fn prefers_editor_pane(&self, editor_panes: &[Rc<dyn IEditorDescriptor>]) -> Option<Rc<dyn IEditorDescriptor>> {
        editor_panes.first().cloned()
    }

    /// Returns a representation of this typed editor input as untyped resource editor input that e.g. can be used to
    /// serialize the editor input into a form that it can be restored.
    ///
    /// May return `None` if an untyped representation is not supported.
    fn to_untyped(&self, _options: Option<&UntypedEditorOptions>) -> Option<UntypedEditorInput> {
        None
    }

    /// Returns if this editor is disposed.
    fn is_disposed(&self) -> bool {
        self.base().is_disposed()
    }

    /// `primary` e `secondary` quando a entrada é lado a lado (`isSideBySideEditorInput`): the primary editor input is
    /// shown on the right hand side and the secondary on the left hand side.
    fn side_by_side_editors(&self) -> Option<(EditorInputRef, EditorInputRef)> {
        None
    }

    /// `modified` e `original` quando a entrada é de diff (`isDiffEditorInput`): the modified (primary) editor input is
    /// shown on the right hand side and the original (secondary) on the left hand side.
    fn diff_editors(&self) -> Option<(EditorInputRef, EditorInputRef)> {
        None
    }

    /// `preferredResource` (`EditorInputWithPreferredResource`): an editor may provide an additional preferred resource
    /// alongside the `resource` property. While the `resource` property serves as unique identifier of the editor that
    /// should be used whenever we compare to other editors, the `preferredResource` should be used in places where e.g.
    /// the resource is shown to the user.
    fn preferred_resource(&self) -> Option<URI> {
        None
    }

    /// `diffResources` (`IEditorInputWithDiffResources`): o `original` e o `modified` do diff.
    fn diff_resources(&self) -> Option<(URI, URI)> {
        None
    }
}
