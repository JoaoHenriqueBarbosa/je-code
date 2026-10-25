//! Porte de `src/vs/workbench/common/editor/diffEditorInput.ts`: a entrada base do diff editor, feita de duas entradas de
//! editor, a versão original e a modificada.
//!
//! Divergências:
//! - **Herança.** `DiffEditorInput extends SideBySideEditorInput`: a struct guarda a [`SideBySideEditorInput`] (`side`),
//!   criada por [`SideBySideEditorInput::new_inner`] com o `Weak` do diff (o `super(...)`). O que o diff não sobrescreve é
//!   delegado à `side` (a macro `inherit_from_side_by_side`, que gera os métodos que só repassam à superclasse).
//! - **`readonly original`/`modified`** são [`DiffEditorInput::original`] e [`DiffEditorInput::modified`].
//! - **Rótulos.** `shorten` é o de `je_base::labels` com o separador de caminho nativo.
//!
//! - **`modifiedEditorModel instanceof BaseTextEditorModel`.** É [`IEditorModel::into_text_editor_model`]: os modelos que herdam
//!   da `BaseTextEditorModel` devolvem a si mesmos como `ITextEditorModel`, e o `TextDiffEditorModel` é criado sobre eles.
//!
//! Fora deste módulo (e por quê): `DiffEditorInputSerializer` (depende do `IInstantiationService`, do `IEditorSerializer` e
//! do `Registry` de plataforma).

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::labels::shorten;
use je_base::lifecycle::{IDisposable, id_of, id_of_ref};
use je_base::nls::localize;
use je_base::path::PathModule;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::resources::host_environment;

use crate::common::editor::diff_editor_model::DiffEditorModel;
use crate::common::editor::editor_input::{EditorInput, EditorInputBase, UntypedEditorOptions};
use crate::common::editor::editor_model::IEditorModel;
use crate::common::editor::side_by_side_editor_input::SideBySideEditorInput;
use crate::common::editor::text_diff_editor_model::TextDiffEditorModel;
use crate::common::editor::{
    BINARY_DIFF_EDITOR_ID, EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef, GroupIdentifier, IEditorDescriptor, MoveResult, RevertOptions,
    SaveOptions, TEXT_DIFF_EDITOR_ID, UntypedEditorInput, Verbosity,
};
use crate::services::editor::editor_service::IEditorService;
use crate::services::files_configuration::files_configuration_service::Readonly;

/// `IDiffEditorInputLabels`.
struct DiffEditorInputLabels {
    name: String,

    short_description: Option<String>,
    medium_description: Option<String>,
    long_description: Option<String>,

    force_description: bool,

    short_title: Option<String>,
    medium_title: Option<String>,
    long_title: Option<String>,
}

/// `computeLabel(originalLabel, modifiedLabel, separator = ' - ')`.
fn compute_label(original_label: Option<&str>, modified_label: Option<&str>, separator: &str) -> Option<String> {
    let (original_label, modified_label) = (original_label.filter(|label| !label.is_empty())?, modified_label.filter(|label| !label.is_empty())?);

    if original_label == modified_label {
        return Some(modified_label.to_string());
    }

    Some(format!("{original_label}{separator}{modified_label}"))
}

/// Gera os métodos de `EditorInput` que o diff herda da `SideBySideEditorInput` sem sobrescrever.
macro_rules! inherit_from_side_by_side {
    ($($method:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        $(
            fn $method(&self, $($arg: $ty),*) -> $ret {
                self.side.$method($($arg),*)
            }
        )*
    };
}

/// `DiffEditorInput`: the base editor input for the diff editor. It is made up of two editor inputs, the original
/// version and the modified version.
pub struct DiffEditorInput {
    side: SideBySideEditorInput,
    original: EditorInputRef,
    modified: EditorInputRef,
    force_open_as_binary: Option<bool>,
    cached_model: Rc<RefCell<Option<Rc<dyn IEditorModel>>>>,
    labels: DiffEditorInputLabels,
}

impl DiffEditorInput {
    pub const ID: &'static str = "workbench.editors.diffEditorInput";

    /// `constructor(preferredName, preferredDescription, original, modified, forceOpenAsBinary, editorService)`.
    pub fn new(
        preferred_name: Option<&str>,
        preferred_description: Option<&str>,
        original: EditorInputRef,
        modified: EditorInputRef,
        force_open_as_binary: Option<bool>,
        editor_service: Rc<dyn IEditorService>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|this| {
            let side = SideBySideEditorInput::new_inner(this.clone(), preferred_name, preferred_description, original.clone(), modified.clone(), editor_service);
            let labels = Self::compute_labels(&side, &original, &modified);

            Self { side, original, modified, force_open_as_binary, cached_model: Rc::new(RefCell::new(None)), labels }
        })
    }

    /// `original`: the original (secondary) editor input is shown on the left hand side.
    pub fn original(&self) -> EditorInputRef {
        self.original.clone()
    }

    /// `modified`: the modified (primary) editor input is shown on the right hand side.
    pub fn modified(&self) -> EditorInputRef {
        self.modified.clone()
    }

    fn compute_labels(side: &SideBySideEditorInput, original: &EditorInputRef, modified: &EditorInputRef) -> DiffEditorInputLabels {
        let preferred_name = side.get_preferred_name().filter(|name| !name.is_empty());
        let preferred_description = side.get_preferred_description().filter(|description| !description.is_empty());

        // Name
        let name: String;
        let mut force_description = false;
        if let Some(preferred_name) = preferred_name {
            name = preferred_name;
        } else {
            let original_name = original.get_name();
            let modified_name = modified.get_name();

            name = localize("sideBySideLabels", "{0} ↔ {1}", &[original_name.as_str(), modified_name.as_str()]);

            // Enforce description when the names are identical
            force_description = original_name == modified_name;
        }

        // Description
        let short_description: Option<String>;
        let mut medium_description: Option<String> = None;
        let long_description: Option<String>;
        if let Some(preferred_description) = preferred_description {
            short_description = Some(preferred_description.clone());
            medium_description = Some(preferred_description.clone());
            long_description = Some(preferred_description);
        } else {
            let description = |editor: &EditorInputRef, verbosity: Verbosity| editor.get_description(Some(verbosity));

            short_description = compute_label(description(original, Verbosity::Short).as_deref(), description(modified, Verbosity::Short).as_deref(), " - ");
            long_description = compute_label(description(original, Verbosity::Long).as_deref(), description(modified, Verbosity::Long).as_deref(), " - ");

            // Medium Description: try to be verbose by computing a label that resembles the difference between the two
            let original_medium_description = description(original, Verbosity::Medium);
            let modified_medium_description = description(modified, Verbosity::Medium);
            let is_set = |description: &Option<String>| description.as_deref().is_some_and(|description| !description.is_empty());
            force_description = force_description
                && is_set(&original_medium_description)
                && is_set(&modified_medium_description)
                && original_medium_description != modified_medium_description;
            if let (Some(original_medium_description), Some(modified_medium_description)) = (&original_medium_description, &modified_medium_description)
                && (!original_medium_description.is_empty() || !modified_medium_description.is_empty()) // ...however never when both sides are empty strings
            {
                // we can only `shorten` when both sides are strings...
                let env = host_environment();
                let separator = PathModule::native(&*env).sep();
                let shortened = shorten(&[Some(original_medium_description.as_str()), Some(modified_medium_description.as_str())], separator);
                medium_description = compute_label(shortened[0].as_deref(), shortened[1].as_deref(), " - ");
            }
        }

        // Title
        let title = |verbosity: Verbosity| {
            compute_label(Some(original.get_title(Some(verbosity)).as_str()), Some(modified.get_title(Some(verbosity)).as_str()), " ↔ ")
        };
        let (mut short_title, mut medium_title, mut long_title) = (title(Verbosity::Short), title(Verbosity::Medium), title(Verbosity::Long));

        if let Some(preferred_title) = side.get_preferred_title().filter(|title| !title.is_empty()) {
            let with_preferred = |title: Option<String>| Some(format!("{preferred_title} ({})", title.unwrap_or_else(|| "undefined".to_string())));
            short_title = with_preferred(short_title);
            medium_title = with_preferred(medium_title);
            long_title = with_preferred(long_title);
        }

        DiffEditorInputLabels { name, short_description, medium_description, long_description, force_description, short_title, medium_title, long_title }
    }
}

impl IDisposable for DiffEditorInput {
    fn dispose(&self) {
        // Free the diff editor model but do not propagate the dispose() call to the two inputs. We never created the two
        // inputs (original and modified) so we can not dispose them without sideeffects.
        let cached_model = self.cached_model.borrow_mut().take();
        if let Some(cached_model) = cached_model {
            cached_model.dispose();
        }

        self.side.dispose();
    }
}

impl EditorInput for DiffEditorInput {
    fn base(&self) -> &EditorInputBase {
        self.side.base()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        Self::ID.to_string()
    }

    fn editor_id(&self) -> Option<String> {
        if self.modified.editor_id() == self.original.editor_id() { self.modified.editor_id() } else { None }
    }

    fn capabilities(&self) -> EditorInputCapabilities {
        let mut capabilities = self.side.capabilities();

        // Force description capability depends on labels
        if self.labels.force_description {
            capabilities |= EditorInputCapabilities::FORCE_DESCRIPTION;
        }

        capabilities
    }

    fn get_name(&self) -> String {
        self.labels.name.clone()
    }

    fn get_description(&self, verbosity: Option<Verbosity>) -> Option<String> {
        match verbosity.unwrap_or(Verbosity::Medium) {
            Verbosity::Short => self.labels.short_description.clone(),
            Verbosity::Long => self.labels.long_description.clone(),
            Verbosity::Medium => self.labels.medium_description.clone(),
        }
    }

    fn get_title(&self, verbosity: Option<Verbosity>) -> String {
        let title = match verbosity {
            Some(Verbosity::Short) => &self.labels.short_title,
            Some(Verbosity::Long) => &self.labels.long_title,
            Some(Verbosity::Medium) | None => &self.labels.medium_title,
        };

        title.clone().unwrap_or_default()
    }

    fn resolve(&self) -> Promise<Option<Rc<dyn IEditorModel>>> {
        // Create Model - we never reuse our cached model if refresh is true because we cannot decide for the inputs within
        // if the cached model can be reused or not. There may be inputs that need to be loaded again and thus we always
        // recreate the model and dispose the previous one - if any.
        let cached_model = self.cached_model.clone();

        // Join resolve call over two inputs and build diff editor model
        Promise::all(vec![self.original.resolve(), self.modified.resolve()]).then(move |models| {
            // If both are text models, return textdiffeditor model
            let text_models = models[0].clone().and_then(|model| model.into_text_editor_model()).zip(models[1].clone().and_then(|model| model.into_text_editor_model()));
            let resolved_model: Rc<dyn IEditorModel> = match text_models {
                Some((original_model, modified_model)) => TextDiffEditorModel::new(original_model, modified_model),

                // Otherwise return normal diff model
                None => DiffEditorModel::new(models[0].clone(), models[1].clone()),
            };

            let previous_model = cached_model.borrow_mut().replace(resolved_model.clone());
            if let Some(previous_model) = previous_model {
                previous_model.dispose();
            }

            Some(resolved_model)
        })
    }

    fn prefers_editor_pane(&self, editor_panes: &[Rc<dyn IEditorDescriptor>]) -> Option<Rc<dyn IEditorDescriptor>> {
        let wanted = if self.force_open_as_binary == Some(true) { BINARY_DIFF_EDITOR_ID } else { TEXT_DIFF_EDITOR_ID };

        editor_panes.iter().find(|editor_pane| editor_pane.editor_type_id() == wanted).cloned()
    }

    fn to_untyped(&self, options: Option<&UntypedEditorOptions>) -> Option<UntypedEditorInput> {
        let untyped = self.side.to_untyped(options)?;

        Some(UntypedEditorInput { modified: untyped.primary.clone(), original: untyped.secondary.clone(), ..untyped })
    }

    fn matches(&self, other_input: &EditorInputOrUntyped) -> bool {
        match other_input {
            EditorInputOrUntyped::Typed(other) => {
                if id_of_ref(self) == id_of(other) {
                    return true;
                }

                match other.as_any().downcast_ref::<DiffEditorInput>() {
                    Some(other) => {
                        self.modified.matches(&EditorInputOrUntyped::Typed(other.modified.clone()))
                            && self.original.matches(&EditorInputOrUntyped::Typed(other.original.clone()))
                            && other.force_open_as_binary == self.force_open_as_binary
                    }
                    None => false,
                }
            }
            EditorInputOrUntyped::Untyped(untyped) => match (&untyped.modified, &untyped.original) {
                (Some(other_modified), Some(other_original)) => {
                    self.modified.matches(&EditorInputOrUntyped::Untyped((**other_modified).clone()))
                        && self.original.matches(&EditorInputOrUntyped::Untyped((**other_original).clone()))
                }
                _ => false,
            },
        }
    }

    fn diff_editors(&self) -> Option<(EditorInputRef, EditorInputRef)> {
        Some((self.modified.clone(), self.original.clone()))
    }

    inherit_from_side_by_side! {
        resource() -> Option<URI>;
        get_label_extra_classes() -> Vec<String>;
        get_aria_label() -> String;
        get_telemetry_descriptor() -> Dynamic;
        is_dirty() -> bool;
        is_saving() -> bool;
        save(group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>>;
        save_as(group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>>;
        revert(group: GroupIdentifier, options: Option<RevertOptions>) -> Promise<()>;
        rename(group: GroupIdentifier, target: &URI) -> Promise<Option<MoveResult>>;
        is_readonly() -> Readonly;
        side_by_side_editors() -> Option<(EditorInputRef, EditorInputRef)>;
    }
}
