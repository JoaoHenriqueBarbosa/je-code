//! Porte de `src/vs/workbench/services/untitled/common/untitledTextEditorInput.ts`: o `UntitledTextEditorInput`, a entrada
//! de editor para buffers de texto sem título. Repassa o nome, a sujeira e o encoding do modelo sem título, resolve o
//! modelo por uma referência do `ITextModelService` e se descarta quando o modelo é revertido.
//!
//! Divergências:
//! - **Herança.** `UntitledTextEditorInput extends AbstractTextResourceEditorInput` guarda a
//!   [`AbstractTextResourceEditorInput`] (campo `input`); os métodos de [`EditorInput`] que a classe não sobrescreve vêm
//!   da macro [`inherit_abstract_text_resource_editor_input`](crate::inherit_abstract_text_resource_editor_input), e o
//!   `super.getDescription(verbosity)`, o `super.getTitle(verbosity)` e o `super.matches(other)` são as chamadas à base
//!   e a [`matches_by_default`].
//! - **Injeção.** O modelo (`protected model`) é o argumento estático ([`Injectable::Args`]) e os serviços decorados são
//!   a tupla `Deps`, na ordem do construtor. O modelo é lido por [`UntitledTextEditorInput::model`] (o `getModel()` que a
//!   subclasse de teste do original expõe) e troca quando o serviço cria outro modelo para o mesmo recurso.
//! - **`resolve(): Promise<IUntitledTextEditorModel>`.** É [`UntitledTextEditorInput::resolve_untitled`]; o
//!   [`EditorInput::resolve`] devolve o mesmo modelo como [`IEditorModel`].
//! - **`IReference<IUntitledTextEditorModel>`** é a [`TextModelReference`] do `ITextModelService`: o original só a guarda
//!   para descartá-la, e o modelo que a referência segura é o do `resolve`.
//! - **`IEncodingSupport` e `ILanguageSupport`.** Os traits [`IEncodingSupport`] e [`ILanguageSupport`]; `getLanguageId`,
//!   `hasLanguageSetExplicitly` e `hasAssociatedFilePath` são métodos próprios.
//! - **`toUntyped`.** O `IUntitledTextResourceEditorInput` é o [`UntypedEditorInput`] (com `forceUntitled`, `encoding`,
//!   `languageId`, `contents` e `options.viewState`), e `typeof options?.preserveViewState === 'number'` é
//!   `Option::is_some`.
//! - **Escutas.** As escutas seguram a entrada por `Weak`, porque o `Rc` dela guarda os emissores onde as escutas vivem.

use std::any::Any;
use std::cell::RefCell;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::lifecycle::{DisposableStore, IDisposable, id_of, id_of_ref};
use je_base::promise::Promise;
use je_base::resources::to_local_resource;
use je_instantiation::Injectable;
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::ILabelService;
use je_platform::resources::ext_uri;
use je_editor::text_resource_configuration_service::ITextResourceConfigurationService;

use crate::common::editor::editor_input::{EditorInput, UntypedEditorOptions, matches_by_default};
use crate::common::editor::editor_model::IEditorModel;
use crate::common::editor::resource_editor_input::ResourceEditorInputServices;
use crate::common::editor::text_resource_editor_input::AbstractTextResourceEditorInput;
use crate::common::editor::{
    DEFAULT_EDITOR_ASSOCIATION_ID, EditorInputOrUntyped, TextEditorOptions, UntypedEditorInput, Verbosity, find_view_state_for_editor, is_untitled_resource_editor_input,
};
use crate::services::editor::custom_editor_label_service::ICustomEditorLabelService;
use crate::services::editor::editor_service::IEditorService;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::files_configuration::files_configuration_service::IFilesConfigurationService;
use crate::services::path::path_service::IPathService;
use crate::services::textfile::textfiles::{EncodingMode, IEncodingSupport, ILanguageSupport, ITextFileService};
use crate::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelService, TextModelReference};
use crate::services::untitled::untitled_text_editor_model::IUntitledTextEditorModel;
use crate::services::untitled::untitled_text_editor_service::IUntitledTextEditorService;
use crate::services::working_copy::working_copy::IWorkingCopy;

/// `UntitledTextEditorInput`: an editor input to be used for untitled text buffers.
pub struct UntitledTextEditorInput {
    this: Weak<UntitledTextEditorInput>,
    input: AbstractTextResourceEditorInput,

    /// `model` (protected).
    model: RefCell<Rc<dyn IUntitledTextEditorModel>>,
    model_resolve: RefCell<Option<Promise<()>>>,
    model_disposables: Rc<DisposableStore>,
    cached_untitled_text_editor_model_reference: RefCell<Option<TextModelReference>>,

    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    path_service: Rc<dyn IPathService>,
    text_model_service: Rc<dyn ITextModelService>,
}

impl Deref for UntitledTextEditorInput {
    type Target = AbstractTextResourceEditorInput;

    fn deref(&self) -> &AbstractTextResourceEditorInput {
        &self.input
    }
}

impl Injectable for UntitledTextEditorInput {
    /// `(model)`.
    type Args = (Rc<dyn IUntitledTextEditorModel>,);
    type Deps = (
        Rc<dyn ITextFileService>,
        Rc<dyn ILabelService>,
        Rc<dyn IEditorService>,
        Rc<dyn IFileService>,
        Rc<dyn IWorkbenchEnvironmentService>,
        Rc<dyn IPathService>,
        Rc<dyn IFilesConfigurationService>,
        Rc<dyn ITextModelService>,
        Rc<dyn ITextResourceConfigurationService>,
        Rc<dyn ICustomEditorLabelService>,
    );

    fn create(
        (model,): Self::Args,
        (
            text_file_service,
            label_service,
            editor_service,
            file_service,
            environment_service,
            path_service,
            files_configuration_service,
            text_model_service,
            text_resource_configuration_service,
            custom_editor_label_service,
        ): Self::Deps,
    ) -> Rc<Self> {
        let untitled = text_file_service.untitled();
        let resource = model.resource();

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            input: AbstractTextResourceEditorInput::new(
                this.clone(),
                resource,
                None,
                editor_service,
                text_file_service,
                ResourceEditorInputServices {
                    label_service,
                    file_service,
                    files_configuration_service,
                    text_resource_configuration_service,
                    custom_editor_label_service,
                },
            ),
            model: RefCell::new(model.clone()),
            model_resolve: RefCell::new(None),
            model_disposables: DisposableStore::new(),
            cached_untitled_text_editor_model_reference: RefCell::new(None),
            environment_service,
            path_service,
            text_model_service,
        });

        this.input.base().register(this.model_disposables.clone());
        this.register_model_listeners(&model);

        let weak_input = this.this.clone();
        this.input.base().register(untitled.on_did_create().listen(move |model| {
            if let Some(this) = weak_input.upgrade() {
                this.on_did_create_untitled_model(model);
            }
        }));

        this
    }
}

impl UntitledTextEditorInput {
    pub const ID: &'static str = "workbench.editors.untitledEditorInput";

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("a entrada de editor já foi liberada")
    }

    /// `model` (protected).
    pub fn model(&self) -> Rc<dyn IUntitledTextEditorModel> {
        self.model.borrow().clone()
    }

    fn register_model_listeners(&self, model: &Rc<dyn IUntitledTextEditorModel>) {
        self.model_disposables.clear();

        // re-emit some events from the model
        let this = self.this.clone();
        self.model_disposables.add(model.on_did_change_dirty().listen(move |_| {
            if let Some(this) = this.upgrade() {
                this.input.base().fire_did_change_dirty();
            }
        }));

        let this = self.this.clone();
        self.model_disposables.add(model.on_did_change_name().listen(move |_| {
            if let Some(this) = this.upgrade() {
                this.input.base().fire_did_change_label();
            }
        }));

        // a reverted untitled text editor model renders this input disposed
        let this = self.this.clone();
        self.model_disposables.add(model.on_did_revert().listen(move |_| {
            if let Some(this) = this.upgrade() {
                this.dispose();
            }
        }));
    }

    fn on_did_create_untitled_model(&self, model: &Rc<dyn IUntitledTextEditorModel>) {
        let current = self.model();
        if ext_uri().is_equal(Some(&model.resource()), Some(&current.resource()), false) && id_of(model) != id_of(&current) {
            // Ensure that we keep our model up to date with
            // the actual model from the service so that we
            // never get out of sync with the truth.

            *self.model.borrow_mut() = model.clone();
            self.register_model_listeners(model);
        }
    }

    /// `hasLanguageSetExplicitly`.
    pub fn has_language_set_explicitly(&self) -> bool {
        self.model().has_language_set_explicitly()
    }

    /// `hasAssociatedFilePath`.
    pub fn has_associated_file_path(&self) -> bool {
        self.model().has_associated_file_path()
    }

    /// `getLanguageId()`.
    pub fn get_language_id(&self) -> Option<String> {
        self.model().get_language_id()
    }

    /// `resolve(): Promise<IUntitledTextEditorModel>`.
    pub fn resolve_untitled(&self) -> Promise<Rc<dyn IUntitledTextEditorModel>> {
        let this = self.rc();

        let existing_model_resolve = self.model_resolve.borrow().clone();
        let model_resolve = existing_model_resolve.unwrap_or_else(|| {
            let acquiring = this.clone();
            let model_resolve = async_fn(async move {
                // Acquire a model reference
                let reference = acquiring.text_model_service.create_model_reference(&acquiring.input.resource).await?;
                *acquiring.cached_untitled_text_editor_model_reference.borrow_mut() = Some(reference);
                Ok(())
            });
            *self.model_resolve.borrow_mut() = Some(model_resolve.clone());
            model_resolve
        });

        async_fn(async move {
            model_resolve.await?;

            // It is possible that this input was disposed before the model
            // finished resolving. As such, we need to make sure to dispose
            // the model reference to not leak it.
            if EditorInput::is_disposed(this.as_ref()) {
                this.dispose_model_reference();
            }

            Ok(this.model())
        })
    }

    fn dispose_model_reference(&self) {
        let reference = self.cached_untitled_text_editor_model_reference.borrow_mut().take();
        if let Some(reference) = reference {
            reference.dispose();
        }
    }
}

impl IEncodingSupport for UntitledTextEditorInput {
    fn get_encoding(&self) -> Option<String> {
        self.model().get_encoding()
    }

    /// `setEncoding(encoding, mode)`: o `mode` é ignorado, só existe `EncodingMode::Encode`.
    fn set_encoding(&self, encoding: &str, _mode: EncodingMode) -> Promise<()> {
        self.model().set_encoding(encoding)
    }
}

impl ILanguageSupport for UntitledTextEditorInput {
    fn set_language_id(&self, language_id: &str, source: Option<&str>) {
        self.model().set_language_id(language_id, source);
    }
}

impl IDisposable for UntitledTextEditorInput {
    fn dispose(&self) {
        // Model
        *self.model_resolve.borrow_mut() = None;

        // Model reference
        self.dispose_model_reference();

        self.input.dispose();
    }
}

impl EditorInput for UntitledTextEditorInput {
    crate::inherit_abstract_text_resource_editor_input!(input; base, resource, preferred_resource, capabilities, is_readonly, save, save_as, revert);

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        Self::ID.to_string()
    }

    fn editor_id(&self) -> Option<String> {
        Some(DEFAULT_EDITOR_ASSOCIATION_ID.to_string())
    }

    fn get_name(&self) -> String {
        self.model().name()
    }

    fn get_description(&self, verbosity: Option<Verbosity>) -> Option<String> {
        // Without associated path: only use if name and description differ
        if !self.has_associated_file_path() {
            let description_candidate = self.input.resource.path().to_string_lossy();
            if description_candidate != self.get_name() {
                return Some(description_candidate);
            }

            return None;
        }

        // With associated path: delegate to parent
        self.input.get_description(verbosity)
    }

    fn get_title(&self, verbosity: Option<Verbosity>) -> String {
        // Without associated path: check if name and description differ to decide
        // if description should appear besides the name to distinguish better
        if !self.has_associated_file_path() {
            let name = self.get_name();
            let description = self.get_description(None);
            if let Some(description) = description.filter(|description| !description.is_empty() && *description != name) {
                return format!("{name} • {description}");
            }

            return name;
        }

        // With associated path: delegate to parent
        self.input.get_title(verbosity)
    }

    fn is_dirty(&self) -> bool {
        self.model().is_dirty()
    }

    fn resolve(&self) -> Promise<Option<Rc<dyn IEditorModel>>> {
        self.resolve_untitled().then(|model| {
            let model: Rc<dyn IEditorModel> = model;
            Some(model)
        })
    }

    fn to_untyped(&self, options: Option<&UntypedEditorOptions>) -> Option<UntypedEditorInput> {
        let model = self.model();
        let has_associated_file_path = model.has_associated_file_path();

        let resource = if has_associated_file_path {
            to_local_resource(&model.resource(), self.environment_service.remote_authority().as_deref(), &self.path_service.default_uri_scheme())
        } else {
            self.input.resource.clone()
        };

        let mut untyped_input = UntypedEditorInput {
            resource: Some(resource),
            force_untitled: Some(true),
            options: Some(TextEditorOptions::with_override_id(DEFAULT_EDITOR_ASSOCIATION_ID)),
            ..UntypedEditorInput::default()
        };

        if let Some(options) = options
            && let Some(preserve_view_state) = options.preserve_view_state
        {
            untyped_input.encoding = model.get_encoding();
            untyped_input.language_id = model.get_language_id();
            untyped_input.contents = if model.is_modified() { model.text_editor_model().map(|text_editor_model| text_editor_model.get_value().to_string_lossy()) } else { None };
            if let Some(untyped_options) = untyped_input.options.as_mut() {
                untyped_options.view_state = find_view_state_for_editor(&self.this(), preserve_view_state, self.input.editor_service.as_ref());
            }

            if untyped_input.contents.is_some() && !has_associated_file_path && options.preserve_resource != Some(true) {
                // Given how generic untitled resources in the system are, we
                // need to be careful not to set our resource into the untyped
                // editor if we want to transport contents too, because of
                // issue https://github.com/microsoft/vscode/issues/140898
                // The workaround is to simply remove the resource association
                // if we have contents and no associated resource.
                // In that case we can ensure that a new untitled resource is
                // being created and the contents can be restored properly.
                untyped_input.resource = None;
            }
        }

        Some(untyped_input)
    }

    fn matches(&self, other_input: &EditorInputOrUntyped) -> bool {
        if let EditorInputOrUntyped::Typed(other) = other_input {
            if id_of_ref(self) == id_of(other) {
                return true;
            }

            if let Some(other) = other.as_any().downcast_ref::<UntitledTextEditorInput>() {
                return ext_uri().is_equal(Some(&other.input.resource), Some(&self.input.resource), false);
            }
        }

        if is_untitled_resource_editor_input(Some(other_input)) {
            return matches_by_default(self, other_input);
        }

        false
    }
}
