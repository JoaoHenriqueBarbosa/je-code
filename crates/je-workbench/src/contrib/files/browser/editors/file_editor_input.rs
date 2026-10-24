//! Porte de `src/vs/workbench/contrib/files/browser/editors/fileEditorInput.ts`: o `FileEditorInput`, a entrada de editor
//! dos recursos do sistema de arquivos. Liga a entrada ao `TextFileEditorModel` do recurso (sujeira, somente leitura,
//! codificação e linguagem), guarda as preferências de nome, descrição, codificação, linguagem e conteúdo, e resolve o
//! modelo de texto por uma referência do `ITextModelService` (ou o `BinaryEditorModel`, para arquivos binários).
//!
//! Divergências:
//! - **Herança.** `FileEditorInput extends AbstractTextResourceEditorInput` guarda a [`AbstractTextResourceEditorInput`]
//!   (campo `input`) e dá `Deref` para ela; os métodos de [`EditorInput`] que a classe não sobrescreve vêm da macro
//!   [`inherit_abstract_text_resource_editor_input`](crate::inherit_abstract_text_resource_editor_input), e o
//!   `super.getTitle(verbosity)`, o `super.getDescription(verbosity)`, o `super.getName()` e o `super.matches(other)` são
//!   as chamadas à base e a [`matches_by_default`]. O `super.isSaving()` (o `EditorInput.isSaving`) é `false`.
//! - **Injeção.** Os argumentos estáticos são `(resource, preferredResource, preferredName, preferredDescription,
//!   preferredEncoding, preferredLanguageId, preferredContents)` ([`Injectable::Args`]) e os serviços decorados são a
//!   tupla `Deps`, na ordem do construtor (`IInstantiationService` é o [`InstantiationService`] concreto).
//! - **`IFileEditorInput`.** O contrato ainda não foi portado: a entrada implementa [`IEncodingSupport`] e
//!   [`ILanguageSupport`], e `setPreferredResource` e `preferredResource` vêm da base e de [`EditorInput`].
//!   `IFileEditorInputOptions` (as opções de texto mais os limites) é o [`FileLimitedEditorInputOptions`], porque `resolve`
//!   só lê `limits`.
//! - **`resolve(options?): Promise<ITextFileEditorModel | BinaryEditorModel>`.** É [`FileEditorInput::resolve_file`]; o
//!   [`EditorInput::resolve`] devolve o mesmo modelo como [`IEditorModel`]. O `ITextFileEditorModel` que a referência
//!   guarda e o [`BinaryEditorModel`] distinguem-se pelo `downcast_ref` de [`IEditorModel::as_any`], o `instanceof` do
//!   original.
//! - **`IReference<ITextFileEditorModel>`** é a [`TextModelReference`] do `ITextModelService`: o original a guarda para
//!   descartá-la e lê o modelo dela.
//! - **`const enum ForceOpenAs`** é o `enum` [`ForceOpenAs`].
//! - **`typeof preferredContents === 'string'`** é `Option::is_some` (a string vazia conta); os demais `if (preferredX)` do
//!   construtor são os de texto não vazio.
//! - **`instanceof FileEditorInput`** é o `downcast_ref` por [`EditorInput::as_any`].
//! - **`toUntyped`.** O `IUntypedFileEditorInput` é o [`UntypedEditorInput`] (com `forceFile`, `encoding`, `languageId`,
//!   `contents` e `options.viewState`), e `typeof options?.preserveViewState === 'number'` é `Option::is_some`.
//! - **`rename`.** O `IMoveResult` é o [`MoveResult`], com o `viewState` dentro das opções do editor sem tipo.
//! - **Escutas.** As escutas seguram a entrada por `Weak`, porque o `Rc` dela guarda os emissores onde as escutas vivem.
//! - **Continuação do `resolve`.** A continuação guarda o `Rc` da entrada (o `this` do original), que vive até a promessa se
//!   resolver.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::lifecycle::{DisposableStore, IDisposable, id_of, id_of_ref};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::text_model::create_text_buffer_factory;
use je_editor::text_resource_configuration_service::ITextResourceConfigurationService;
use je_instantiation::{Injectable, InstantiationService};
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::ILabelService;
use je_platform::resources::ext_uri;

use crate::common::editor::binary_editor_model::BinaryEditorModel;
use crate::common::editor::editor_input::{EditorInput, UntypedEditorOptions, matches_by_default};
use crate::common::editor::editor_model::IEditorModel;
use crate::common::editor::resource_editor_input::{FileLimitedEditorInputOptions, ResourceEditorInputServices};
use crate::common::editor::text_resource_editor_input::AbstractTextResourceEditorInput;
use crate::common::editor::{
    DEFAULT_EDITOR_ASSOCIATION_ID, EditorInputCapabilities, EditorInputOrUntyped, GroupIdentifier, IEditorDescriptor, MoveResult, TextEditorOptions, UntypedEditorInput, Verbosity,
    find_view_state_for_editor, is_resource_editor_input,
};
use crate::contrib::files::common_files::{BINARY_FILE_EDITOR_ID, FILE_EDITOR_INPUT_ID, TEXT_FILE_EDITOR_ID};
use crate::services::editor::custom_editor_label_service::ICustomEditorLabelService;
use crate::services::editor::editor_service::IEditorService;
use crate::services::files_configuration::files_configuration_service::{IFilesConfigurationService, Readonly, ResourceOrEditor};
use crate::services::path::path_service::IPathService;
use crate::services::textfile::textfiles::{
    EncodingMode, IEncodingSupport, ILanguageSupport, ITextFileEditorModel, ITextFileService, TextBufferFactoryRef, TextFileEditorModelResolveOrCreateOptions, TextFileEditorModelState,
    TextFileReloadOptions, TextFileResolveOptions, TextFileResolveReason, TextFileOperationResult, text_file_operation_result_of,
};
use crate::services::textmodel_resolver::resolver_service::{ITextModelService, TextModelReference};
use crate::services::working_copy::working_copy::IWorkingCopy;

/// `ForceOpenAs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ForceOpenAs {
    None,
    Text,
    Binary,
}

/// `createTextBufferFactory(text)`.
fn text_buffer_factory(text: &str) -> TextBufferFactoryRef {
    TextBufferFactoryRef::new(create_text_buffer_factory(JsString::from(text).units()))
}

/// Texto não vazio: a verdade de `string` no JavaScript.
fn is_truthy(text: &Option<String>) -> bool {
    text.as_deref().is_some_and(|text| !text.is_empty())
}

/// A file editor input is the input type for the file editor of file system resources.
pub struct FileEditorInput {
    this: Weak<FileEditorInput>,
    input: AbstractTextResourceEditorInput,

    preferred_name: RefCell<Option<String>>,
    preferred_description: RefCell<Option<String>>,
    preferred_encoding: RefCell<Option<String>>,
    preferred_language_id: RefCell<Option<String>>,
    preferred_contents: RefCell<Option<String>>,

    force_open_as: Cell<ForceOpenAs>,

    model: RefCell<Option<Rc<dyn ITextFileEditorModel>>>,
    cached_text_file_model_reference: RefCell<Option<TextModelReference>>,

    model_listeners: Rc<DisposableStore>,

    instantiation_service: Rc<InstantiationService>,
    text_model_service: Rc<dyn ITextModelService>,
    path_service: Rc<dyn IPathService>,
}

impl Deref for FileEditorInput {
    type Target = AbstractTextResourceEditorInput;

    fn deref(&self) -> &AbstractTextResourceEditorInput {
        &self.input
    }
}

impl Injectable for FileEditorInput {
    /// `(resource, preferredResource, preferredName, preferredDescription, preferredEncoding, preferredLanguageId,
    /// preferredContents)`.
    type Args = (URI, Option<URI>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>);
    type Deps = (
        Rc<InstantiationService>,
        Rc<dyn ITextFileService>,
        Rc<dyn ITextModelService>,
        Rc<dyn ILabelService>,
        Rc<dyn IFileService>,
        Rc<dyn IFilesConfigurationService>,
        Rc<dyn IEditorService>,
        Rc<dyn IPathService>,
        Rc<dyn ITextResourceConfigurationService>,
        Rc<dyn ICustomEditorLabelService>,
    );

    fn create(
        (resource, preferred_resource, preferred_name, preferred_description, preferred_encoding, preferred_language_id, preferred_contents): Self::Args,
        (
            instantiation_service,
            text_file_service,
            text_model_service,
            label_service,
            file_service,
            files_configuration_service,
            editor_service,
            path_service,
            text_resource_configuration_service,
            custom_editor_label_service,
        ): Self::Deps,
    ) -> Rc<Self> {
        let files = text_file_service.files();
        let model = files.get(&resource);

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            input: AbstractTextResourceEditorInput::new(
                this.clone(),
                resource,
                preferred_resource,
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
            preferred_name: RefCell::new(None),
            preferred_description: RefCell::new(None),
            preferred_encoding: RefCell::new(None),
            preferred_language_id: RefCell::new(None),
            preferred_contents: RefCell::new(None),
            force_open_as: Cell::new(ForceOpenAs::None),
            model: RefCell::new(model.clone()),
            cached_text_file_model_reference: RefCell::new(None),
            model_listeners: DisposableStore::new(),
            instantiation_service,
            text_model_service,
            path_service,
        });

        this.input.base().register(this.model_listeners.clone());

        if let Some(preferred_name) = preferred_name.filter(|name| !name.is_empty()) {
            this.set_preferred_name(&preferred_name);
        }

        if let Some(preferred_description) = preferred_description.filter(|description| !description.is_empty()) {
            this.set_preferred_description(&preferred_description);
        }

        if let Some(preferred_encoding) = preferred_encoding.filter(|encoding| !encoding.is_empty()) {
            this.set_preferred_encoding(&preferred_encoding);
        }

        if let Some(preferred_language_id) = preferred_language_id.filter(|language_id| !language_id.is_empty()) {
            this.set_preferred_language_id(&preferred_language_id);
        }

        if let Some(preferred_contents) = preferred_contents {
            this.set_preferred_contents(&preferred_contents);
        }

        // Attach to model that matches our resource once created
        let weak_input = this.this.clone();
        this.input.base().register(files.on_did_create().listen(move |model| {
            if let Some(this) = weak_input.upgrade() {
                this.on_did_create_text_file_model(model);
            }
        }));

        // If a file model already exists, make sure to wire it in
        if let Some(model) = model {
            this.register_model_listeners(&model);
        }

        this
    }
}

impl FileEditorInput {
    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("a entrada de editor já foi liberada")
    }

    /// `model` (o modelo de arquivo de texto que a entrada acompanha, se já existe).
    fn current_model(&self) -> Option<Rc<dyn ITextFileEditorModel>> {
        self.model.borrow().clone()
    }

    fn on_did_create_text_file_model(&self, model: &Rc<dyn ITextFileEditorModel>) {
        // Once the text file model is created, we keep it inside
        // the input to be able to implement some methods properly
        if ext_uri().is_equal(Some(&model.resource()), Some(&self.input.resource), false) {
            *self.model.borrow_mut() = Some(model.clone());

            self.register_model_listeners(model);
        }
    }

    fn register_model_listeners(&self, model: &Rc<dyn ITextFileEditorModel>) {
        // Clear any old
        self.model_listeners.clear();

        // re-emit some events from the model
        let this = self.this.clone();
        self.model_listeners.add(model.on_did_change_dirty().listen(move |_| {
            if let Some(this) = this.upgrade() {
                this.input.base().fire_did_change_dirty();
            }
        }));

        let this = self.this.clone();
        self.model_listeners.add(model.on_did_change_readonly().listen(move |_| {
            if let Some(this) = this.upgrade() {
                this.input.base().fire_did_change_capabilities();
            }
        }));

        // important: treat save errors as potential dirty change because
        // a file that is in save conflict or error will report dirty even
        // if auto save is turned on.
        let this = self.this.clone();
        self.model_listeners.add(model.on_did_save_error().listen(move |_| {
            if let Some(this) = this.upgrade() {
                this.input.base().fire_did_change_dirty();
            }
        }));

        // remove model association once it gets disposed
        let this = self.this.clone();
        self.model_listeners.add(model.on_will_dispose().once().listen(move |_| {
            if let Some(this) = this.upgrade() {
                this.model_listeners.clear();
                *this.model.borrow_mut() = None;
            }
        }));
    }

    /// `setPreferredName(name)`.
    pub fn set_preferred_name(&self, name: &str) {
        if !self.allow_label_override() {
            return; // block for specific schemes we consider to be owning
        }

        if self.preferred_name.borrow().as_deref() != Some(name) {
            *self.preferred_name.borrow_mut() = Some(name.to_string());

            self.input.base().fire_did_change_label();
        }
    }

    fn allow_label_override(&self) -> bool {
        let scheme = self.input.resource.scheme().to_string_lossy();

        scheme != self.path_service.default_uri_scheme() && scheme != schemas::VSCODE_USER_DATA && scheme != schemas::FILE && scheme != schemas::VSCODE_REMOTE
    }

    /// `getPreferredName()`.
    pub fn get_preferred_name(&self) -> Option<String> {
        self.preferred_name.borrow().clone()
    }

    /// `setPreferredDescription(description)`.
    pub fn set_preferred_description(&self, description: &str) {
        if !self.allow_label_override() {
            return; // block for specific schemes we consider to be owning
        }

        if self.preferred_description.borrow().as_deref() != Some(description) {
            *self.preferred_description.borrow_mut() = Some(description.to_string());

            self.input.base().fire_did_change_label();
        }
    }

    /// `getPreferredDescription()`.
    pub fn get_preferred_description(&self) -> Option<String> {
        self.preferred_description.borrow().clone()
    }

    /// `getPreferredTitle()` (protected).
    fn get_preferred_title(&self) -> Option<String> {
        let preferred_name = self.preferred_name.borrow().clone();
        let preferred_description = self.preferred_description.borrow().clone();

        if is_truthy(&preferred_name) && is_truthy(&preferred_description) {
            return Some(format!("{} {}", preferred_name.unwrap_or_default(), preferred_description.unwrap_or_default()));
        }

        if is_truthy(&preferred_name) || is_truthy(&preferred_description) {
            return preferred_name.or(preferred_description);
        }

        None
    }

    /// `getPreferredEncoding()`.
    pub fn get_preferred_encoding(&self) -> Option<String> {
        self.preferred_encoding.borrow().clone()
    }

    /// `setPreferredEncoding(encoding)`.
    pub fn set_preferred_encoding(&self, encoding: &str) {
        *self.preferred_encoding.borrow_mut() = Some(encoding.to_string());

        // encoding is a good hint to open the file as text
        self.set_force_open_as_text();
    }

    /// `getLanguageId()`.
    pub fn get_language_id(&self) -> Option<String> {
        if let Some(model) = self.current_model() {
            return model.get_language_id();
        }

        self.preferred_language_id.borrow().clone()
    }

    /// `getPreferredLanguageId()`.
    pub fn get_preferred_language_id(&self) -> Option<String> {
        self.preferred_language_id.borrow().clone()
    }

    /// `setPreferredLanguageId(languageId)`.
    pub fn set_preferred_language_id(&self, language_id: &str) {
        *self.preferred_language_id.borrow_mut() = Some(language_id.to_string());

        // languages are a good hint to open the file as text
        self.set_force_open_as_text();
    }

    /// `setPreferredContents(contents)`.
    pub fn set_preferred_contents(&self, contents: &str) {
        *self.preferred_contents.borrow_mut() = Some(contents.to_string());

        // contents is a good hint to open the file as text
        self.set_force_open_as_text();
    }

    /// `setForceOpenAsText()`.
    pub fn set_force_open_as_text(&self) {
        self.force_open_as.set(ForceOpenAs::Text);
    }

    /// `setForceOpenAsBinary()`.
    pub fn set_force_open_as_binary(&self) {
        self.force_open_as.set(ForceOpenAs::Binary);
    }

    /// `isResolved()`.
    pub fn is_resolved(&self) -> bool {
        self.current_model().is_some()
    }

    /// `resolve(options?): Promise<ITextFileEditorModel | BinaryEditorModel>`.
    pub fn resolve_file(&self, options: Option<FileLimitedEditorInputOptions>) -> Promise<Rc<dyn IEditorModel>> {
        // Resolve as binary
        if self.force_open_as.get() == ForceOpenAs::Binary {
            return self.do_resolve_as_binary();
        }

        // Resolve as text
        self.do_resolve_as_text(options)
    }

    fn do_resolve_as_text(&self, options: Option<FileLimitedEditorInputOptions>) -> Promise<Rc<dyn IEditorModel>> {
        let this = self.rc();

        // Unset preferred contents after having applied it once
        // to prevent this property to stick. We still want future
        // `resolve` calls to fetch the contents from disk.
        let preferred_contents = self.preferred_contents.borrow_mut().take();

        // Resolve resource via text file service and only allow
        // to open binary files if we are instructed so
        let resolve_options = TextFileEditorModelResolveOrCreateOptions {
            resolve: TextFileResolveOptions {
                contents: preferred_contents.map(|contents| text_buffer_factory(&contents)),
                force_read_from_file: None,
                allow_binary: Some(self.force_open_as.get() == ForceOpenAs::Text),
                reason: Some(TextFileResolveReason::Editor),
                limits: self.input.ensure_limits(options.as_ref()),
            },
            language_id: self.preferred_language_id.borrow().clone(),
            encoding: self.preferred_encoding.borrow().clone(),
            // trigger a reload of the model if it exists already but do not wait to show the model
            reload: Some(TextFileReloadOptions { async_: true }),
        };
        let resolve = self.input.text_file_service.files().resolve(&self.input.resource, Some(resolve_options));

        async_fn(async move {
            match Self::resolve_text_model(this.clone(), resolve).await {
                Ok(model) => Ok(model),
                Err(error) => {
                    // Handle binary files with binary model
                    if text_file_operation_result_of(&error) == Some(TextFileOperationResult::FileIsBinary) {
                        return this.do_resolve_as_binary().await;
                    }

                    // Bubble any other error up
                    Err(error)
                }
            }
        })
    }

    /// O corpo do `try` de `doResolveAsText` depois da chamada de `files.resolve`.
    async fn resolve_text_model(this: Rc<Self>, resolve: Promise<Rc<dyn ITextFileEditorModel>>) -> Result<Rc<dyn IEditorModel>, Error> {
        resolve.await?;

        // This is a bit ugly, because we first resolve the model and then resolve a model reference. the reason being that binary
        // or very large files do not resolve to a text file model but should be opened as binary files without text. First calling into
        // resolve() ensures we are not creating model references for these kind of resources.
        // In addition we have a bit of payload to take into account (encoding, reload) that the text resolver does not handle yet.
        let has_reference = this.cached_text_file_model_reference.borrow().is_some();
        if !has_reference {
            let reference = this.text_model_service.create_model_reference(&this.input.resource).await?;
            *this.cached_text_file_model_reference.borrow_mut() = Some(reference);
        }

        let model: Rc<dyn IEditorModel> = this
            .cached_text_file_model_reference
            .borrow()
            .as_ref()
            .expect("a referência do modelo foi guardada logo acima")
            .object
            .clone();

        // It is possible that this input was disposed before the model
        // finished resolving. As such, we need to make sure to dispose
        // the model reference to not leak it.
        if this.input.base().is_disposed() {
            this.dispose_model_reference();
        }

        Ok(model)
    }

    fn do_resolve_as_binary(&self) -> Promise<Rc<dyn IEditorModel>> {
        let model = self.instantiation_service.create_instance::<BinaryEditorModel>((self.input.preferred_resource(), EditorInput::get_name(self)));
        let resolved = IEditorModel::resolve(&*model);
        let model: Rc<dyn IEditorModel> = model;

        async_fn(async move {
            resolved.await?;

            Ok(model)
        })
    }

    fn dispose_model_reference(&self) {
        let reference = self.cached_text_file_model_reference.borrow_mut().take();
        if let Some(reference) = reference {
            reference.dispose();
        }
    }
}

impl IEncodingSupport for FileEditorInput {
    fn get_encoding(&self) -> Option<String> {
        if let Some(model) = self.current_model() {
            return model.get_encoding();
        }

        self.preferred_encoding.borrow().clone()
    }

    fn set_encoding(&self, encoding: &str, mode: EncodingMode) -> Promise<()> {
        self.set_preferred_encoding(encoding);

        match self.current_model() {
            Some(model) => model.set_encoding(encoding, mode),
            None => Promise::resolved(()),
        }
    }
}

impl ILanguageSupport for FileEditorInput {
    fn set_language_id(&self, language_id: &str, source: Option<&str>) {
        self.set_preferred_language_id(language_id);

        if let Some(model) = self.current_model() {
            model.set_language_id(language_id, source);
        }
    }
}

impl IDisposable for FileEditorInput {
    fn dispose(&self) {
        // Model
        *self.model.borrow_mut() = None;

        // Model reference
        self.dispose_model_reference();

        self.input.dispose();
    }
}

impl EditorInput for FileEditorInput {
    crate::inherit_abstract_text_resource_editor_input!(input; base, resource, preferred_resource, save, save_as, revert);

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        FILE_EDITOR_INPUT_ID.to_string()
    }

    fn editor_id(&self) -> Option<String> {
        Some(DEFAULT_EDITOR_ASSOCIATION_ID.to_string())
    }

    fn capabilities(&self) -> EditorInputCapabilities {
        let mut capabilities = EditorInputCapabilities::CAN_SPLIT_IN_GROUP;

        if let Some(model) = self.current_model() {
            if model.is_readonly().is_readonly() {
                capabilities |= EditorInputCapabilities::READONLY;
            }
        } else if self.input.file_service.has_provider(&self.input.resource) {
            if self.input.files_configuration_service.is_readonly(&self.input.resource, None).is_readonly() {
                capabilities |= EditorInputCapabilities::READONLY;
            }
        } else {
            capabilities |= EditorInputCapabilities::UNTITLED;
        }

        if !capabilities.intersects(EditorInputCapabilities::READONLY) {
            capabilities |= EditorInputCapabilities::CAN_DROP_INTO_EDITOR;
        }

        capabilities
    }

    fn get_name(&self) -> String {
        match self.preferred_name.borrow().clone() {
            Some(name) if !name.is_empty() => name,
            _ => self.input.get_name(),
        }
    }

    fn is_readonly(&self) -> Readonly {
        match self.current_model() {
            Some(model) => model.is_readonly(),
            None => self.input.files_configuration_service.is_readonly(&self.input.resource, None),
        }
    }

    fn get_description(&self, verbosity: Option<Verbosity>) -> Option<String> {
        match self.preferred_description.borrow().clone() {
            Some(description) if !description.is_empty() => Some(description),
            _ => self.input.get_description(verbosity),
        }
    }

    fn get_title(&self, verbosity: Option<Verbosity>) -> String {
        let mut title = self.input.get_title(verbosity);

        if let Some(preferred_title) = self.get_preferred_title().filter(|preferred_title| !preferred_title.is_empty()) {
            title = format!("{preferred_title} ({title})");
        }

        title
    }

    fn is_dirty(&self) -> bool {
        self.current_model().is_some_and(|model| model.is_dirty())
    }

    fn is_saving(&self) -> bool {
        if let Some(model) = self.current_model()
            && (model.has_state(TextFileEditorModelState::Saved) || model.has_state(TextFileEditorModelState::Conflict) || model.has_state(TextFileEditorModelState::Error))
        {
            return false; // require the model to be dirty and not in conflict or error state
        }

        // Note: currently not checking for ModelState.PENDING_SAVE for a reason
        // because we currently miss an event for this state change on editors
        // and it could result in bad UX where an editor can be closed even though
        // it shows up as dirty and has not finished saving yet.

        if self.input.files_configuration_service.has_short_auto_save_delay(Some(&ResourceOrEditor::from_editor(&self.this()))) {
            return true; // a short auto save is configured, treat this as being saved
        }

        false // `super.isSaving()`
    }

    fn prefers_editor_pane(&self, editor_panes: &[Rc<dyn IEditorDescriptor>]) -> Option<Rc<dyn IEditorDescriptor>> {
        let preferred_type_id = if self.force_open_as.get() == ForceOpenAs::Binary { BINARY_FILE_EDITOR_ID } else { TEXT_FILE_EDITOR_ID };

        editor_panes.iter().find(|editor_pane| editor_pane.editor_type_id() == preferred_type_id).cloned()
    }

    fn resolve(&self) -> Promise<Option<Rc<dyn IEditorModel>>> {
        self.resolve_file(None).then(Some)
    }

    fn rename(&self, group: GroupIdentifier, target: &URI) -> Promise<Option<MoveResult>> {
        let mut options = TextEditorOptions::default();
        options.view_state = find_view_state_for_editor(&self.this(), group, self.input.editor_service.as_ref());

        Promise::resolved(Some(MoveResult {
            editor: EditorInputOrUntyped::Untyped(UntypedEditorInput {
                resource: Some(target.clone()),
                encoding: IEncodingSupport::get_encoding(self),
                options: Some(options),
                ..UntypedEditorInput::default()
            }),
            options: None,
        }))
    }

    fn to_untyped(&self, options: Option<&UntypedEditorOptions>) -> Option<UntypedEditorInput> {
        let mut untyped_input = UntypedEditorInput {
            resource: Some(self.input.preferred_resource()),
            force_file: Some(true),
            options: Some(TextEditorOptions::with_override_id(DEFAULT_EDITOR_ASSOCIATION_ID)),
            ..UntypedEditorInput::default()
        };

        if let Some(options) = options
            && let Some(preserve_view_state) = options.preserve_view_state
        {
            untyped_input.encoding = IEncodingSupport::get_encoding(self);
            untyped_input.language_id = self.get_language_id();
            untyped_input.contents = self
                .input
                .text_file_service
                .files()
                .get(&self.input.resource)
                .filter(|model| model.is_dirty())
                .and_then(|model| model.text_editor_model())
                .filter(|text_editor_model| !text_editor_model.is_too_large_for_heap_operation()) // only if dirty and not too large
                .map(|text_editor_model| text_editor_model.get_value().to_string_lossy());

            if let Some(untyped_options) = untyped_input.options.as_mut() {
                untyped_options.view_state = find_view_state_for_editor(&self.this(), preserve_view_state, self.input.editor_service.as_ref());
            }
        }

        Some(untyped_input)
    }

    fn matches(&self, other_input: &EditorInputOrUntyped) -> bool {
        if let EditorInputOrUntyped::Typed(other) = other_input {
            if id_of_ref(self) == id_of(other) {
                return true;
            }

            if let Some(other) = other.as_any().downcast_ref::<FileEditorInput>() {
                return ext_uri().is_equal(Some(&other.input.resource), Some(&self.input.resource), false);
            }
        }

        if is_resource_editor_input(Some(other_input)) {
            return matches_by_default(self, other_input);
        }

        false
    }
}
