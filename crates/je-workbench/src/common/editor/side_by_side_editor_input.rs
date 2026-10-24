//! Porte de `src/vs/workbench/common/editor/sideBySideEditorInput.ts`: a entrada de editor lado a lado, com um lado
//! primário e um secundário.
//!
//! Divergências:
//! - **Injeção.** O `@IEditorService` do construtor é um `Rc<dyn IEditorService>` (o contrato reduzido de
//!   `services::editor::editor_service`).
//! - **Herança.** A [`DiffEditorInput`](crate::common::editor::diff_editor_input::DiffEditorInput) herda desta classe: ela
//!   guarda uma [`SideBySideEditorInput`] criada por [`SideBySideEditorInput::new_inner`] (o `super(...)`), cujo `this` é o
//!   objeto de fora. Os `this.x()` virtuais do original (o `this.getName()` do `super.getTitle`, o `this.dispose()` dos
//!   listeners, o `this` devolvido por `save`) vão pelo `Weak` da base, que aponta para o objeto de fora.
//! - **`getPreferredTitle`** é protegido: aqui é [`SideBySideEditorInput::get_preferred_title`] (`pub(crate)`), que o diff usa.
//! - **`super.x()` do `EditorInput`** são as implementações padrão do trait, reescritas no lugar (um trait não permite
//!   chamar o padrão que o `impl` sobrescreveu): `getDescription` devolve `None`, `getLabelExtraClasses` uma lista vazia,
//!   `getTitle` o `getName()` e `getAriaLabel` o `getTitle(SHORT)`, ambos virtuais.
//! - **`save`/`rename`** devolvem promessas, e as continuações são `'static`: o que elas leem da entrada (o `this`, os
//!   lados, o nome e a descrição preferidos, o serviço) é copiado antes, em [`SideBySideSnapshot`].
//!
//! Fora deste módulo (e por quê): `AbstractSideBySideEditorInputSerializer` e `SideBySideEditorInputSerializer` (dependem do
//! `IInstantiationService`, do `IEditorSerializer` e do `Registry` de plataforma, que ainda não foram portados).

use std::any::Any;
use std::rc::{Rc, Weak};

use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::event;
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;

use crate::common::editor::editor_input::{EditorInput, EditorInputBase, UntypedEditorOptions};
use crate::common::editor::{
    EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef, GroupIdentifier, MoveResult, RevertOptions, SaveOptions, TextEditorOptions,
    UntypedEditorInput, Verbosity, find_view_state_for_editor,
};
use crate::services::editor::editor_service::IEditorService;
use crate::services::files_configuration::files_configuration_service::Readonly;

/// `if (name)`: um texto é verdadeiro quando existe e não é vazio.
fn truthy(text: &Option<String>) -> Option<&String> {
    text.as_ref().filter(|text| !text.is_empty())
}

/// `{ ...first, ...second }` sobre os descritores de telemetria.
fn merge_descriptors(first: &Dynamic, second: &Dynamic) -> Dynamic {
    let merged = ObjectRef::new();
    for (key, value) in first.own_entries().into_iter().chain(second.own_entries()) {
        merged.set(&key, value);
    }
    Dynamic::Object(merged)
}

/// O que as continuações de `save` e `rename` leem da entrada (elas são `'static`).
#[derive(Clone)]
struct SideBySideSnapshot {
    this: EditorInputRef,
    primary: EditorInputRef,
    preferred_name: Option<String>,
    preferred_description: Option<String>,
    has_identical_sides: bool,
    editor_service: Rc<dyn IEditorService>,
}

impl SideBySideSnapshot {
    /// `new SideBySideEditorInput(this.preferredName, this.preferredDescription, editor, editor, this.editorService)`.
    fn identical_sides_input(&self, editor: &EditorInputRef) -> EditorInputRef {
        SideBySideEditorInput::new(
            self.preferred_name.as_deref(),
            self.preferred_description.as_deref(),
            editor.clone(),
            editor.clone(),
            self.editor_service.clone(),
        )
    }

    /// `{ label: this.preferredName, description: this.preferredDescription, primary: editor, secondary: editor }`.
    fn identical_sides_untyped(&self, editor: &UntypedEditorInput) -> UntypedEditorInput {
        UntypedEditorInput {
            label: self.preferred_name.clone(),
            description: self.preferred_description.clone(),
            primary: Some(Box::new(editor.clone())),
            secondary: Some(Box::new(editor.clone())),
            ..UntypedEditorInput::default()
        }
    }

    /// `saveResultToEditor`.
    fn save_result_to_editor(&self, primary_save_result: Option<EditorInputOrUntyped>) -> Option<EditorInputOrUntyped> {
        let primary_save_result = primary_save_result?;
        if !self.has_identical_sides {
            return Some(primary_save_result);
        }

        if self.primary.matches(&primary_save_result) {
            return Some(EditorInputOrUntyped::Typed(self.this.clone()));
        }

        match &primary_save_result {
            EditorInputOrUntyped::Typed(editor) => Some(EditorInputOrUntyped::Typed(self.identical_sides_input(editor))),
            EditorInputOrUntyped::Untyped(untyped) if !untyped.is_compound() => Some(EditorInputOrUntyped::Untyped(self.identical_sides_untyped(untyped))),
            EditorInputOrUntyped::Untyped(_) => None,
        }
    }

    /// `{ ...renameResult.options, viewState: findViewStateForEditor(this, group, this.editorService) }`.
    fn options_with_view_state(&self, options: Option<TextEditorOptions>, group: GroupIdentifier) -> TextEditorOptions {
        let mut options = options.unwrap_or_default();
        options.view_state = find_view_state_for_editor(&self.this, group, &*self.editor_service);
        options
    }

    /// O resultado de `rename` da lado primário, refeito como lado a lado.
    fn rename_result_to_editor(&self, group: GroupIdentifier, rename_result: Option<MoveResult>) -> Option<MoveResult> {
        let rename_result = rename_result?;

        // Build a side-by-side result from the rename result
        match &rename_result.editor {
            EditorInputOrUntyped::Typed(editor) => Some(MoveResult {
                editor: EditorInputOrUntyped::Typed(self.identical_sides_input(editor)),
                options: Some(self.options_with_view_state(rename_result.options.clone(), group)),
            }),
            EditorInputOrUntyped::Untyped(untyped) if untyped.is_resource_editor_input() => {
                let mut editor = self.identical_sides_untyped(untyped);
                editor.options = Some(self.options_with_view_state(rename_result.options.clone(), group));

                Some(MoveResult { editor: EditorInputOrUntyped::Untyped(editor), options: None })
            }
            EditorInputOrUntyped::Untyped(_) => None,
        }
    }
}

/// `SideBySideEditorInput`: side by side editor inputs that have a primary and secondary side.
pub struct SideBySideEditorInput {
    base: EditorInputBase,
    preferred_name: Option<String>,
    preferred_description: Option<String>,
    secondary: EditorInputRef,
    primary: EditorInputRef,
    editor_service: Rc<dyn IEditorService>,
    has_identical_sides: bool,
}

impl SideBySideEditorInput {
    pub const ID: &'static str = "workbench.editorinputs.sidebysideEditorInput";

    /// `constructor(preferredName, preferredDescription, secondary, primary, editorService)`.
    pub fn new(
        preferred_name: Option<&str>,
        preferred_description: Option<&str>,
        secondary: EditorInputRef,
        primary: EditorInputRef,
        editor_service: Rc<dyn IEditorService>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|this| Self::new_inner(this.clone(), preferred_name, preferred_description, secondary, primary, editor_service))
    }

    /// O construtor da classe: `this` é o `Weak` do objeto de fora (este, ou a subclasse que o guarda).
    pub(crate) fn new_inner(
        this: Weak<dyn EditorInput>,
        preferred_name: Option<&str>,
        preferred_description: Option<&str>,
        secondary: EditorInputRef,
        primary: EditorInputRef,
        editor_service: Rc<dyn IEditorService>,
    ) -> Self {
        let has_identical_sides = primary.matches(&EditorInputOrUntyped::Typed(secondary.clone()));

        let base = EditorInputBase::new(this.clone());
        Self::register_listeners(&base, &this, &primary, &secondary);

        Self {
            base,
            preferred_name: preferred_name.map(str::to_string),
            preferred_description: preferred_description.map(str::to_string),
            secondary,
            primary,
            editor_service,
            has_identical_sides,
        }
    }

    fn register_listeners(base: &EditorInputBase, this: &Weak<dyn EditorInput>, primary: &EditorInputRef, secondary: &EditorInputRef) {
        // When the primary or secondary input gets disposed, dispose this diff editor input
        let dispose_this = {
            let this = this.clone();
            move |_: &()| {
                if let Some(this) = this.upgrade()
                    && !this.is_disposed()
                {
                    this.dispose();
                }
            }
        };
        base.register(event::any(vec![primary.on_will_dispose(), secondary.on_will_dispose()]).once().listen(dispose_this));

        // Re-emit some events from the primary side to the outside
        base.register(primary.on_did_change_dirty().listen(Self::forwarder(this, EditorInputBase::fire_did_change_dirty)));

        // Re-emit some events from both sides to the outside
        base.register(primary.on_did_change_capabilities().listen(Self::forwarder(this, EditorInputBase::fire_did_change_capabilities)));
        base.register(secondary.on_did_change_capabilities().listen(Self::forwarder(this, EditorInputBase::fire_did_change_capabilities)));
        base.register(primary.on_did_change_label().listen(Self::forwarder(this, EditorInputBase::fire_did_change_label)));
        base.register(secondary.on_did_change_label().listen(Self::forwarder(this, EditorInputBase::fire_did_change_label)));
    }

    /// `() => this._onDidChange...Fire()`: o listener que reemite o evento pelo objeto de fora.
    fn forwarder(this: &Weak<dyn EditorInput>, fire: fn(&EditorInputBase)) -> impl Fn(&()) + 'static {
        let this = this.clone();
        move |_| {
            if let Some(this) = this.upgrade() {
                fire(this.base());
            }
        }
    }

    /// `secondary`: the secondary editor input is shown on the left hand side.
    pub fn secondary(&self) -> EditorInputRef {
        self.secondary.clone()
    }

    /// `primary`: the primary editor input is shown on the right hand side.
    pub fn primary(&self) -> EditorInputRef {
        self.primary.clone()
    }

    pub fn get_preferred_name(&self) -> Option<String> {
        self.preferred_name.clone()
    }

    pub fn get_preferred_description(&self) -> Option<String> {
        self.preferred_description.clone()
    }

    /// `getPreferredTitle()` (protegido).
    pub(crate) fn get_preferred_title(&self) -> Option<String> {
        if truthy(&self.preferred_name).is_some() && truthy(&self.preferred_description).is_some() {
            return Some(format!("{} {}", self.preferred_name.as_deref().unwrap_or_default(), self.preferred_description.as_deref().unwrap_or_default()));
        }

        if truthy(&self.preferred_name).is_some() || truthy(&self.preferred_description).is_some() {
            return self.preferred_name.clone().or_else(|| self.preferred_description.clone());
        }

        None
    }

    fn snapshot(&self) -> SideBySideSnapshot {
        SideBySideSnapshot {
            this: self.base.this(),
            primary: self.primary.clone(),
            preferred_name: self.preferred_name.clone(),
            preferred_description: self.preferred_description.clone(),
            has_identical_sides: self.has_identical_sides,
            editor_service: self.editor_service.clone(),
        }
    }
}

impl IDisposable for SideBySideEditorInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorInput for SideBySideEditorInput {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        Self::ID.to_string()
    }

    fn capabilities(&self) -> EditorInputCapabilities {
        // Use primary capabilities as main capabilities...
        let mut capabilities = self.primary.capabilities();

        // ...with the exception of `CanSplitInGroup` which is only relevant to single editors.
        capabilities = capabilities & !EditorInputCapabilities::CAN_SPLIT_IN_GROUP;

        // Trust: should be considered for both sides
        if self.secondary.has_capability(EditorInputCapabilities::REQUIRES_TRUST) {
            capabilities |= EditorInputCapabilities::REQUIRES_TRUST;
        }

        // Singleton: should be considered for both sides
        if self.secondary.has_capability(EditorInputCapabilities::SINGLETON) {
            capabilities |= EditorInputCapabilities::SINGLETON;
        }

        // ForceReveal: should be considered for both sides
        if self.secondary.has_capability(EditorInputCapabilities::FORCE_REVEAL) {
            capabilities |= EditorInputCapabilities::FORCE_REVEAL;
        }

        // Indicate we show more than one editor
        capabilities |= EditorInputCapabilities::MULTIPLE_EDITORS;

        capabilities
    }

    fn resource(&self) -> Option<URI> {
        if self.has_identical_sides {
            // pretend to be just primary side when being asked for a resource in case both sides are the same. this can
            // help when components want to identify this input among others (e.g. in history).
            return self.primary.resource();
        }

        None
    }

    fn get_name(&self) -> String {
        if let Some(preferred_name) = truthy(&self.preferred_name) {
            return preferred_name.clone();
        }

        if self.has_identical_sides {
            return self.primary.get_name(); // keep name concise when same editor is opened side by side
        }

        let (secondary_name, primary_name) = (self.secondary.get_name(), self.primary.get_name());

        localize("sideBySideLabels", "{0} - {1}", &[secondary_name.as_str(), primary_name.as_str()])
    }

    fn get_description(&self, verbosity: Option<Verbosity>) -> Option<String> {
        if let Some(preferred_description) = truthy(&self.preferred_description) {
            return Some(preferred_description.clone());
        }

        if self.has_identical_sides {
            return self.primary.get_description(verbosity);
        }

        None
    }

    fn get_title(&self, verbosity: Option<Verbosity>) -> String {
        let mut title = if self.has_identical_sides { self.primary.get_title(verbosity) } else { self.base.this().get_name() };

        if let Some(preferred_title) = self.get_preferred_title().filter(|title| !title.is_empty()) {
            title = format!("{preferred_title} ({title})");
        }

        title
    }

    fn get_label_extra_classes(&self) -> Vec<String> {
        if self.has_identical_sides {
            return self.primary.get_label_extra_classes();
        }

        Vec::new()
    }

    fn get_aria_label(&self) -> String {
        if self.has_identical_sides {
            return self.primary.get_aria_label();
        }

        self.base.this().get_title(Some(Verbosity::Short))
    }

    fn get_telemetry_descriptor(&self) -> Dynamic {
        let descriptor = self.primary.get_telemetry_descriptor();
        let own = Dynamic::object([("typeId", Dynamic::string(&self.base.this().editor_type_id()))]);

        merge_descriptors(&descriptor, &own)
    }

    fn is_dirty(&self) -> bool {
        self.primary.is_dirty()
    }

    fn is_saving(&self) -> bool {
        self.primary.is_saving()
    }

    fn save(&self, group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        let snapshot = self.snapshot();

        self.primary.save(group, options).then(move |primary_save_result| snapshot.save_result_to_editor(primary_save_result))
    }

    fn save_as(&self, group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        let snapshot = self.snapshot();

        self.primary.save_as(group, options).then(move |primary_save_result| snapshot.save_result_to_editor(primary_save_result))
    }

    fn revert(&self, group: GroupIdentifier, options: Option<RevertOptions>) -> Promise<()> {
        self.primary.revert(group, options)
    }

    fn rename(&self, group: GroupIdentifier, target: &URI) -> Promise<Option<MoveResult>> {
        if !self.has_identical_sides {
            return Promise::resolved(None); // currently only enabled when both sides are identical
        }

        // Forward rename to primary side
        let snapshot = self.snapshot();

        self.primary.rename(group, target).then(move |rename_result| snapshot.rename_result_to_editor(group, rename_result))
    }

    fn is_readonly(&self) -> Readonly {
        self.primary.is_readonly()
    }

    fn to_untyped(&self, options: Option<&UntypedEditorOptions>) -> Option<UntypedEditorInput> {
        let primary_resource_editor_input = self.primary.to_untyped(options)?;
        let secondary_resource_editor_input = self.secondary.to_untyped(options)?;

        // Prevent nested side by side editors which are unsupported
        if primary_resource_editor_input.is_compound() || secondary_resource_editor_input.is_compound() {
            return None;
        }

        let mut untyped_input = UntypedEditorInput {
            label: self.preferred_name.clone(),
            description: self.preferred_description.clone(),
            primary: Some(Box::new(primary_resource_editor_input)),
            secondary: Some(Box::new(secondary_resource_editor_input)),
            ..UntypedEditorInput::default()
        };

        if let Some(preserve_view_state) = options.and_then(|options| options.preserve_view_state) {
            let mut editor_options = TextEditorOptions::default();
            editor_options.view_state = find_view_state_for_editor(&self.base.this(), preserve_view_state, &*self.editor_service);
            untyped_input.options = Some(editor_options);
        }

        Some(untyped_input)
    }

    fn matches(&self, other_input: &EditorInputOrUntyped) -> bool {
        match other_input {
            EditorInputOrUntyped::Typed(other) => {
                if je_base::lifecycle::id_of_ref(self) == je_base::lifecycle::id_of(other) {
                    return true;
                }

                if other.diff_editors().is_some() {
                    return false; // prevent subclass from matching
                }

                match other.side_by_side_editors() {
                    Some((other_primary, other_secondary)) => {
                        self.primary.matches(&EditorInputOrUntyped::Typed(other_primary)) && self.secondary.matches(&EditorInputOrUntyped::Typed(other_secondary))
                    }
                    None => false,
                }
            }
            EditorInputOrUntyped::Untyped(untyped) => {
                if untyped.is_resource_diff_editor_input() {
                    return false; // prevent subclass from matching
                }

                match (&untyped.primary, &untyped.secondary) {
                    (Some(other_primary), Some(other_secondary)) => {
                        self.primary.matches(&EditorInputOrUntyped::Untyped((**other_primary).clone()))
                            && self.secondary.matches(&EditorInputOrUntyped::Untyped((**other_secondary).clone()))
                    }
                    _ => false,
                }
            }
        }
    }

    fn side_by_side_editors(&self) -> Option<(EditorInputRef, EditorInputRef)> {
        Some((self.primary.clone(), self.secondary.clone()))
    }
}
