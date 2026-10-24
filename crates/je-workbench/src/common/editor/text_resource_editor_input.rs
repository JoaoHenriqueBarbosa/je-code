//! Porte de `src/vs/workbench/common/editor/textResourceEditorInput.ts`: o `AbstractTextResourceEditorInput` (a base das
//! entradas que abrem em editores de texto) e o `TextResourceEditorInput` (uma entrada somente leitura cujo conteúdo é
//! um recurso que aponta para um modelo do editor de código que já existe).
//!
//! Divergências:
//! - **Herança.** `AbstractTextResourceEditorInput extends AbstractResourceEditorInput` guarda a
//!   [`AbstractResourceEditorInput`] (campo `resource_input`) e dá `Deref` para ela, o que herda os campos e os métodos
//!   que ela traz. `TextResourceEditorInput extends AbstractTextResourceEditorInput` faz o mesmo. Os métodos de
//!   [`EditorInput`] que a classe não sobrescreve vêm da macro [`inherit_abstract_text_resource_editor_input`]
//!   (que acrescenta `save`, `saveAs` e `revert` à de [`AbstractResourceEditorInput`]).
//! - **Injeção.** Os argumentos estáticos de `TextResourceEditorInput` são `(resource, name, description,
//!   preferredLanguageId, preferredContents)` ([`Injectable::Args`]) e os serviços decorados são a tupla `Deps`, na ordem
//!   do original (`ITextModelService`, `ITextFileService`, `IEditorService`, `IFileService`, `ILabelService`,
//!   `IFilesConfigurationService`, `ITextResourceConfigurationService`, `ICustomEditorLabelService`).
//! - **`save(group, options?: ITextFileSaveOptions)`.** O contrato de [`EditorInput::save`] recebe as [`SaveOptions`]
//!   (o `ISaveOptions` que o `ITextFileSaveOptions` estende), e os campos extras do serviço de arquivos ficam
//!   `undefined`, como numa chamada sem eles. O `group` de `doSave` é só lido pelo original para passar adiante e não
//!   muda nada: não entra na assinatura de [`AbstractTextResourceEditorInput::do_save`].
//! - **`resolve(): Promise<ITextEditorModel>`** é o `resolve` do [`EditorInput`], que devolve o modelo como
//!   [`IEditorModel`]. O `instanceof TextResourceEditorModel` é o `downcast_ref` por [`IEditorModel::as_any`]; o
//!   `cachedModel` guarda o modelo como [`ITextEditorModel`] (sempre um `TextResourceEditorModel`) e o
//!   `setLanguageId` o consulta por `downcast_ref`.
//! - **`typeof preferredContents === 'string'`** é `Option::is_some` (a string vazia conta).
//! - **Continuação do `resolve`.** A continuação guarda o `Rc` da entrada (o `this` do original), que vive até a promessa
//!   se resolver.

use std::any::Any;
use std::cell::RefCell;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::errors::Error;
use je_base::lifecycle::{IDisposable, id_of, id_of_ref};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::text_model::create_text_buffer_factory;
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::ILabelService;
use je_platform::resources::ext_uri;
use je_editor::text_resource_configuration_service::ITextResourceConfigurationService;

use crate::common::editor::editor_input::{EditorInput, matches_by_default};
use crate::common::editor::editor_model::IEditorModel;
use crate::common::editor::resource_editor_input::{AbstractResourceEditorInput, ResourceEditorInputServices};
use crate::common::editor::text_resource_editor_model::TextResourceEditorModel;
use crate::common::editor::{
    DEFAULT_EDITOR_ASSOCIATION_ID, EditorInputOrUntyped, GroupIdentifier, RevertOptions, SaveOptions, UntypedEditorInput, Verbosity, is_resource_editor_input,
};
use crate::services::editor::custom_editor_label_service::ICustomEditorLabelService;
use crate::services::editor::editor_service::IEditorService;
use crate::services::files_configuration::files_configuration_service::IFilesConfigurationService;
use crate::services::textfile::textfiles::{ILanguageSupport, ITextFileService, TextFileSaveAsOptions, TextFileSaveOptions};
use crate::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelService, TextModelReference};

/// `AbstractTextResourceEditorInput`: the base class for all editor inputs that open in text editors.
pub struct AbstractTextResourceEditorInput {
    resource_input: AbstractResourceEditorInput,

    /// `editorService` (protected).
    pub editor_service: Rc<dyn IEditorService>,
    /// `textFileService` (protected).
    pub text_file_service: Rc<dyn ITextFileService>,
}

impl Deref for AbstractTextResourceEditorInput {
    type Target = AbstractResourceEditorInput;

    fn deref(&self) -> &AbstractResourceEditorInput {
        &self.resource_input
    }
}

impl AbstractTextResourceEditorInput {
    /// `constructor(resource, preferredResource, editorService, textFileService, labelService, fileService,
    /// filesConfigurationService, textResourceConfigurationService, customEditorLabelService)`.
    pub fn new(
        this: Weak<dyn EditorInput>,
        resource: URI,
        preferred_resource: Option<URI>,
        editor_service: Rc<dyn IEditorService>,
        text_file_service: Rc<dyn ITextFileService>,
        services: ResourceEditorInputServices,
    ) -> Self {
        Self { resource_input: AbstractResourceEditorInput::new(this, resource, preferred_resource, services), editor_service, text_file_service }
    }

    /// `save(group, options?)`.
    pub fn save(&self, group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        // If this is neither an `untitled` resource, nor a resource
        // we can handle with the file service, we can only "Save As..."
        if *self.resource.scheme() != schemas::UNTITLED && !self.file_service.has_provider(&self.resource) {
            return self.save_as(group, options);
        }

        // Normal save
        self.do_save(options, false)
    }

    /// `saveAs(group, options?)`.
    pub fn save_as(&self, _group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        self.do_save(options, true)
    }

    fn do_save(&self, options: Option<SaveOptions>, save_as: bool) -> Promise<Option<EditorInputOrUntyped>> {
        // Save / Save As
        let target = if save_as {
            let options = TextFileSaveAsOptions {
                suggested_target: Some(self.preferred_resource()),
                ..TextFileSaveAsOptions::from_save_options(options.unwrap_or_default())
            };
            self.text_file_service.save_as(&self.resource, None, Some(options))
        } else {
            self.text_file_service.save(&self.resource, options.map(|base| TextFileSaveOptions { base, ..TextFileSaveOptions::default() }))
        };

        target.then(|target| {
            // save cancelled
            target.map(|target| EditorInputOrUntyped::Untyped(UntypedEditorInput::from_resource(target)))
        })
    }

    /// `revert(group, options?)`.
    pub fn revert(&self, _group: GroupIdentifier, options: Option<RevertOptions>) -> Promise<()> {
        self.text_file_service.revert(&self.resource, options)
    }
}

/// Gera, para uma subclasse do `AbstractTextResourceEditorInput` que guarda a base no campo dado, os métodos de
/// [`EditorInput`] que a classe herda sem sobrescrever: `save`, `save_as` e `revert` e os da macro
/// [`inherit_abstract_resource_editor_input`](crate::inherit_abstract_resource_editor_input).
#[macro_export]
macro_rules! inherit_abstract_text_resource_editor_input {
    (@method $field:ident, save) => {
        fn save(
            &self,
            group: $crate::common::editor::GroupIdentifier,
            options: Option<$crate::common::editor::SaveOptions>,
        ) -> ::je_base::promise::Promise<Option<$crate::common::editor::EditorInputOrUntyped>> {
            self.$field.save(group, options)
        }
    };
    (@method $field:ident, save_as) => {
        fn save_as(
            &self,
            group: $crate::common::editor::GroupIdentifier,
            options: Option<$crate::common::editor::SaveOptions>,
        ) -> ::je_base::promise::Promise<Option<$crate::common::editor::EditorInputOrUntyped>> {
            self.$field.save_as(group, options)
        }
    };
    (@method $field:ident, revert) => {
        fn revert(&self, group: $crate::common::editor::GroupIdentifier, options: Option<$crate::common::editor::RevertOptions>) -> ::je_base::promise::Promise<()> {
            self.$field.revert(group, options)
        }
    };
    (@method $field:ident, $other:ident) => {
        $crate::inherit_abstract_resource_editor_input!(@method $field, $other);
    };
    ($field:ident; $($method:ident),* $(,)?) => {
        $( $crate::inherit_abstract_text_resource_editor_input!(@method $field, $method); )*
    };
}

/// `TextResourceEditorInput`: a read-only text editor input whos contents are made of the provided resource that points
/// to an existing code editor model.
pub struct TextResourceEditorInput {
    this: Weak<TextResourceEditorInput>,
    input: AbstractTextResourceEditorInput,

    name: RefCell<Option<String>>,
    description: RefCell<Option<String>>,
    preferred_language_id: RefCell<Option<String>>,
    preferred_contents: RefCell<Option<String>>,

    cached_model: RefCell<Option<Rc<dyn ITextEditorModel>>>,
    model_reference: RefCell<Option<Promise<TextModelReference>>>,

    text_model_service: Rc<dyn ITextModelService>,
}

impl Deref for TextResourceEditorInput {
    type Target = AbstractTextResourceEditorInput;

    fn deref(&self) -> &AbstractTextResourceEditorInput {
        &self.input
    }
}

impl je_instantiation::Injectable for TextResourceEditorInput {
    type Args = (URI, Option<String>, Option<String>, Option<String>, Option<String>);
    type Deps = (
        Rc<dyn ITextModelService>,
        Rc<dyn ITextFileService>,
        Rc<dyn IEditorService>,
        Rc<dyn IFileService>,
        Rc<dyn ILabelService>,
        Rc<dyn IFilesConfigurationService>,
        Rc<dyn ITextResourceConfigurationService>,
        Rc<dyn ICustomEditorLabelService>,
    );

    fn create(
        (resource, name, description, preferred_language_id, preferred_contents): Self::Args,
        (
            text_model_service,
            text_file_service,
            editor_service,
            file_service,
            label_service,
            files_configuration_service,
            text_resource_configuration_service,
            custom_editor_label_service,
        ): Self::Deps,
    ) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
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
            name: RefCell::new(name),
            description: RefCell::new(description),
            preferred_language_id: RefCell::new(preferred_language_id),
            preferred_contents: RefCell::new(preferred_contents),
            cached_model: RefCell::new(None),
            model_reference: RefCell::new(None),
            text_model_service,
        })
    }
}

impl TextResourceEditorInput {
    pub const ID: &'static str = "workbench.editors.resourceEditorInput";

    /// `setName(name)`.
    pub fn set_name(&self, name: &str) {
        if self.name.borrow().as_deref() != Some(name) {
            *self.name.borrow_mut() = Some(name.to_string());

            self.base().fire_did_change_label();
        }
    }

    /// `setDescription(description)`.
    pub fn set_description(&self, description: &str) {
        if self.description.borrow().as_deref() != Some(description) {
            *self.description.borrow_mut() = Some(description.to_string());

            self.base().fire_did_change_label();
        }
    }

    /// `setPreferredLanguageId(languageId)`.
    pub fn set_preferred_language_id(&self, language_id: &str) {
        *self.preferred_language_id.borrow_mut() = Some(language_id.to_string());
    }

    /// `setPreferredContents(contents)`.
    pub fn set_preferred_contents(&self, contents: &str) {
        *self.preferred_contents.borrow_mut() = Some(contents.to_string());
    }
}

impl ILanguageSupport for TextResourceEditorInput {
    fn set_language_id(&self, language_id: &str, source: Option<&str>) {
        self.set_preferred_language_id(language_id);

        let cached_model = self.cached_model.borrow().clone();
        if let Some(cached_model) = cached_model
            && let Some(cached_model) = cached_model.as_any().downcast_ref::<TextResourceEditorModel>()
        {
            cached_model.set_language_id(language_id, source);
        }
    }
}

impl IDisposable for TextResourceEditorInput {
    fn dispose(&self) {
        let model_reference = self.model_reference.borrow_mut().take();
        if let Some(model_reference) = model_reference {
            model_reference.then(|reference| reference.dispose());
        }

        *self.cached_model.borrow_mut() = None;

        self.input.dispose();
    }
}

impl EditorInput for TextResourceEditorInput {
    crate::inherit_abstract_text_resource_editor_input!(input; base, resource, preferred_resource, capabilities, get_title, is_readonly, save, save_as, revert);

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
        let name = self.name.borrow().clone();
        match name {
            Some(name) if !name.is_empty() => name,
            _ => self.input.get_name(),
        }
    }

    fn get_description(&self, _verbosity: Option<Verbosity>) -> Option<String> {
        self.description.borrow().clone()
    }

    fn resolve(&self) -> Promise<Option<Rc<dyn IEditorModel>>> {
        // Unset preferred contents and language after resolving
        // once to prevent these properties to stick. We still
        // want the user to change the language in the editor
        // and want to show updated contents (if any) in future
        // `resolve` calls.
        let preferred_contents = self.preferred_contents.borrow_mut().take();
        let preferred_language_id = self.preferred_language_id.borrow_mut().take();

        let existing_model_reference = self.model_reference.borrow().clone();
        let model_reference = existing_model_reference.unwrap_or_else(|| {
            let model_reference = self.text_model_service.create_model_reference(&self.resource);
            *self.model_reference.borrow_mut() = Some(model_reference.clone());
            model_reference
        });

        let this = self.this.upgrade().expect("a entrada de editor já foi liberada");
        model_reference.then_promise(move |reference| {
            // Ensure the resolved model is of expected type
            let model = reference.object.clone();
            let Some(text_resource_model) = model.as_any().downcast_ref::<TextResourceEditorModel>() else {
                reference.dispose();
                *this.model_reference.borrow_mut() = None;

                return Promise::rejected(Error::new(format!("Unexpected model for TextResourceEditorInput: {}", this.resource.to_string(false).to_string_lossy())));
            };

            *this.cached_model.borrow_mut() = Some(model.clone());

            // Set contents and language if preferred
            if preferred_contents.is_some() || preferred_language_id.is_some() {
                let contents_factory = preferred_contents.map(|contents| create_text_buffer_factory(JsString::from(contents).units()));
                text_resource_model.update_text_editor_model(contents_factory, preferred_language_id.as_deref(), None);
            }

            let resolved: Rc<dyn IEditorModel> = model;
            Promise::resolved(Some(resolved))
        })
    }

    fn matches(&self, other_input: &EditorInputOrUntyped) -> bool {
        if let EditorInputOrUntyped::Typed(other) = other_input {
            if id_of_ref(self) == id_of(other) {
                return true;
            }

            if let Some(other) = other.as_any().downcast_ref::<TextResourceEditorInput>() {
                return ext_uri().is_equal(Some(&other.resource), Some(&self.resource), false);
            }
        }

        if is_resource_editor_input(Some(other_input)) {
            return matches_by_default(self, other_input);
        }

        false
    }
}
