//! Porte de `src/vs/workbench/common/editor/resourceEditorInput.ts`: o `AbstractResourceEditorInput`, a classe base de
//! todas as entradas de editor que abrem recursos (nome, descrição e título a partir do recurso preferido, capacidades
//! conforme o provider do arquivo e a configuração de somente leitura, e os limites de arquivo grande).
//!
//! Divergências:
//! - **Herança.** `AbstractResourceEditorInput extends EditorInput` é a struct [`AbstractResourceEditorInput`], que guarda
//!   a [`EditorInputBase`] e o estado da classe, com os métodos que a classe sobrescreve como métodos próprios. Uma
//!   subclasse a guarda num campo e herda os métodos de [`EditorInput`] com a macro
//!   [`inherit_abstract_resource_editor_input`](crate::inherit_abstract_resource_editor_input), que lista só o que ela
//!   não sobrescreve. O `dispose` é [`AbstractResourceEditorInput::dispose`] (o `super.dispose()`).
//! - **Injeção.** Os parâmetros decorados do construtor (`labelService`, `fileService`, `filesConfigurationService`,
//!   `textResourceConfigurationService` e `customEditorLabelService`) são a struct [`ResourceEditorInputServices`], que
//!   a subclasse monta a partir do `Deps` dela. O `this` é o `Weak` da entrada concreta, vindo do `Rc::new_cyclic` dela.
//! - **Escutas.** Os eventos de formatador, de registro e de capacidades do provider, do rótulo customizado e da
//!   configuração de somente leitura limpam os rótulos e avisam a entrada pelo `Weak`; o estado que eles limpam (os
//!   rótulos em cache e o recurso preferido) é compartilhado por `Rc`, porque as escutas vivem além do `&self`.
//! - **`IFileReadLimits` e `IFileLimitedEditorInputOptions`** (de `files.ts` e `editor.ts`) são [`FileReadLimits`] (da
//!   plataforma, `je_platform::files::files`) e [`FileLimitedEditorInputOptions`]; `size` é o número de bytes (`u64`, como o `limitSize` das opções de leitura),
//!   e o produto `valor * ByteSize.MB` do limite configurado é truncado para bytes inteiros.
//! - **Rótulos em cache.** `typeof this._name !== 'string'` é `Option::is_none` (os campos só guardam texto).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::uri::URI;
use je_platform::configuration::{ConfigurationValue, is_configured};
use je_platform::files::files::{FileReadLimits, byte_size, get_large_file_confirmation_limit};
use je_platform::files::i_file_service::IFileService;
use je_platform::js::to_number;
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::resources::ext_uri;
use je_editor::text_resource_configuration_service::ITextResourceConfigurationService;

use crate::common::editor::editor_input::{EditorInput, EditorInputBase};
use crate::common::editor::{EditorInputCapabilities, Verbosity};
use crate::services::editor::custom_editor_label_service::ICustomEditorLabelService;
use crate::services::files_configuration::files_configuration_service::{IFilesConfigurationService, Readonly};

/// `IFileLimitedEditorInputOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileLimitedEditorInputOptions {
    /// If provided, the size of the file will be checked against the limits and an error will be thrown if the file
    /// exceeds them.
    pub limits: Option<FileReadLimits>,
}

/// Os serviços que o construtor do `AbstractResourceEditorInput` recebe por injeção.
#[derive(Clone)]
pub struct ResourceEditorInputServices {
    pub label_service: Rc<dyn ILabelService>,
    pub file_service: Rc<dyn IFileService>,
    pub files_configuration_service: Rc<dyn IFilesConfigurationService>,
    pub text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
    pub custom_editor_label_service: Rc<dyn ICustomEditorLabelService>,
}

/// Os rótulos em cache (`_name`, `_shortDescription`, `_mediumDescription`, `_longDescription`, `_shortTitle`,
/// `_mediumTitle` e `_longTitle`).
#[derive(Default)]
struct LabelCache {
    name: Option<String>,
    short_description: Option<String>,
    medium_description: Option<String>,
    long_description: Option<String>,
    short_title: Option<String>,
    medium_title: Option<String>,
    long_title: Option<String>,
}

/// `{ relative: true }`, as opções de `getUriLabel` dos rótulos relativos ao workspace.
fn relative_label_options() -> UriLabelOptions {
    UriLabelOptions { relative: Some(true), ..UriLabelOptions::default() }
}

/// Qual dos rótulos em cache um getter lê.
#[derive(Clone, Copy)]
enum Label {
    ShortDescription,
    MediumDescription,
    LongDescription,
    ShortTitle,
    MediumTitle,
    LongTitle,
}

impl LabelCache {
    fn slot(&mut self, label: Label) -> &mut Option<String> {
        match label {
            Label::ShortDescription => &mut self.short_description,
            Label::MediumDescription => &mut self.medium_description,
            Label::LongDescription => &mut self.long_description,
            Label::ShortTitle => &mut self.short_title,
            Label::MediumTitle => &mut self.medium_title,
            Label::LongTitle => &mut self.long_title,
        }
    }
}

/// `AbstractResourceEditorInput`: the base class for all editor inputs that open resources.
pub struct AbstractResourceEditorInput {
    base: EditorInputBase,

    /// `resource`.
    pub resource: URI,

    preferred_resource: Rc<RefCell<URI>>,
    labels: Rc<RefCell<LabelCache>>,

    /// `labelService` (protected).
    pub label_service: Rc<dyn ILabelService>,
    /// `fileService` (protected).
    pub file_service: Rc<dyn IFileService>,
    /// `filesConfigurationService` (protected).
    pub files_configuration_service: Rc<dyn IFilesConfigurationService>,
    /// `textResourceConfigurationService` (protected).
    pub text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
    /// `customEditorLabelService` (protected).
    pub custom_editor_label_service: Rc<dyn ICustomEditorLabelService>,
}

impl AbstractResourceEditorInput {
    /// `constructor(resource, preferredResource, labelService, fileService, filesConfigurationService,
    /// textResourceConfigurationService, customEditorLabelService)`. `this` é o `Weak` da entrada concreta.
    pub fn new(this: std::rc::Weak<dyn EditorInput>, resource: URI, preferred_resource: Option<URI>, services: ResourceEditorInputServices) -> Self {
        let ResourceEditorInputServices {
            label_service,
            file_service,
            files_configuration_service,
            text_resource_configuration_service,
            custom_editor_label_service,
        } = services;

        let input = Self {
            base: EditorInputBase::new(this.clone()),
            preferred_resource: Rc::new(RefCell::new(preferred_resource.unwrap_or_else(|| resource.clone()))),
            resource,
            labels: Rc::new(RefCell::new(LabelCache::default())),
            label_service,
            file_service,
            files_configuration_service,
            text_resource_configuration_service,
            custom_editor_label_service,
        };

        input.register_listeners(&this);

        input
    }

    fn register_listeners(&self, this: &std::rc::Weak<dyn EditorInput>) {
        // Clear our labels on certain label related events
        let on_label_event: Rc<dyn Fn(&str)> = {
            let (this, labels, preferred_resource) = (this.clone(), self.labels.clone(), self.preferred_resource.clone());
            Rc::new(move |scheme: &str| {
                if preferred_resource.borrow().scheme().to_string_lossy() == scheme
                    && let Some(input) = this.upgrade()
                {
                    Self::update_label_of(&labels, input.base());
                }
            })
        };

        let on_formatters_event = on_label_event.clone();
        self.base.register(self.label_service.on_did_change_formatters().listen(move |e| on_formatters_event(&e.scheme)));
        let on_registrations_event = on_label_event.clone();
        self.base
            .register(self.file_service.on_did_change_file_system_provider_registrations().listen(move |e| on_registrations_event(&e.scheme)));
        let on_capabilities_event = on_label_event;
        self.base.register(self.file_service.on_did_change_file_system_provider_capabilities().listen(move |e| on_capabilities_event(&e.scheme)));

        let (label_input, labels) = (this.clone(), self.labels.clone());
        self.base.register(self.custom_editor_label_service.on_did_change().listen(move |_| {
            if let Some(input) = label_input.upgrade() {
                Self::update_label_of(&labels, input.base());
            }
        }));

        let readonly_input = this.clone();
        self.base.register(self.files_configuration_service.on_did_change_readonly().listen(move |_| {
            if let Some(input) = readonly_input.upgrade() {
                input.base().fire_did_change_capabilities();
            }
        }));
    }

    /// `base`: o estado comum da entrada (para a subclasse devolver em `EditorInput::base`).
    pub fn base(&self) -> &EditorInputBase {
        &self.base
    }

    /// `capabilities`.
    pub fn capabilities(&self) -> EditorInputCapabilities {
        let mut capabilities = EditorInputCapabilities::CAN_SPLIT_IN_GROUP;

        if self.file_service.has_provider(&self.resource) {
            if self.files_configuration_service.is_readonly(&self.resource, None).is_readonly() {
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

    /// `preferredResource`.
    pub fn preferred_resource(&self) -> URI {
        self.preferred_resource.borrow().clone()
    }

    /// `updateLabel()` das escutas: limpa os rótulos em cache e dispara a mudança de rótulo.
    fn update_label_of(labels: &RefCell<LabelCache>, base: &EditorInputBase) {
        // Clear any cached labels from before
        *labels.borrow_mut() = LabelCache::default();

        // Trigger recompute of label
        base.fire_did_change_label();
    }

    fn update_label(&self) {
        Self::update_label_of(&self.labels, &self.base);
    }

    /// `setPreferredResource(preferredResource)`.
    pub fn set_preferred_resource(&self, preferred_resource: &URI) {
        let current = self.preferred_resource();
        if !ext_uri().is_equal(Some(preferred_resource), Some(&current), false) {
            *self.preferred_resource.borrow_mut() = preferred_resource.clone();

            self.update_label();
        }
    }

    /// `getName()`.
    pub fn get_name(&self) -> String {
        let cached = self.labels.borrow().name.clone();
        if let Some(name) = cached {
            return name;
        }

        let preferred_resource = self.preferred_resource();
        let name = self.custom_editor_label_service.get_name(&preferred_resource).unwrap_or_else(|| self.label_service.get_uri_basename_label(&preferred_resource));
        self.labels.borrow_mut().name = Some(name.clone());

        name
    }

    /// `getDescription(verbosity = Verbosity.MEDIUM)`.
    pub fn get_description(&self, verbosity: Option<Verbosity>) -> Option<String> {
        Some(match verbosity.unwrap_or(Verbosity::Medium) {
            Verbosity::Short => self.short_description(),
            Verbosity::Long => self.long_description(),
            Verbosity::Medium => self.medium_description(),
        })
    }

    /// Lê o rótulo em cache, ou o calcula e guarda (o `if (typeof this._x !== 'string')` dos getters).
    fn cached_label(&self, label: Label, compute: impl FnOnce() -> String) -> String {
        let cached = self.labels.borrow_mut().slot(label).clone();
        if let Some(cached) = cached {
            return cached;
        }

        let computed = compute();
        *self.labels.borrow_mut().slot(label) = Some(computed.clone());

        computed
    }

    fn dirname_of_preferred_resource(&self) -> URI {
        ext_uri().dirname(&self.preferred_resource())
    }

    fn short_description(&self) -> String {
        self.cached_label(Label::ShortDescription, || self.label_service.get_uri_basename_label(&self.dirname_of_preferred_resource()))
    }

    fn medium_description(&self) -> String {
        self.cached_label(Label::MediumDescription, || self.label_service.get_uri_label(&self.dirname_of_preferred_resource(), &relative_label_options()))
    }

    fn long_description(&self) -> String {
        self.cached_label(Label::LongDescription, || self.label_service.get_uri_label(&self.dirname_of_preferred_resource(), &UriLabelOptions::default()))
    }

    fn short_title(&self) -> String {
        self.cached_label(Label::ShortTitle, || self.get_name())
    }

    fn medium_title(&self) -> String {
        self.cached_label(Label::MediumTitle, || self.label_service.get_uri_label(&self.preferred_resource(), &relative_label_options()))
    }

    fn long_title(&self) -> String {
        self.cached_label(Label::LongTitle, || self.label_service.get_uri_label(&self.preferred_resource(), &UriLabelOptions::default()))
    }

    /// `getTitle(verbosity?)`.
    pub fn get_title(&self, verbosity: Option<Verbosity>) -> String {
        match verbosity {
            Some(Verbosity::Short) => self.short_title(),
            Some(Verbosity::Long) => self.long_title(),
            Some(Verbosity::Medium) | None => self.medium_title(),
        }
    }

    /// `isReadonly()`.
    pub fn is_readonly(&self) -> Readonly {
        self.files_configuration_service.is_readonly(&self.resource, None)
    }

    /// `ensureLimits(options?)` (protected).
    pub fn ensure_limits(&self, options: Option<&FileLimitedEditorInputOptions>) -> Option<FileReadLimits> {
        if let Some(limits) = options.and_then(|options| options.limits) {
            return Some(limits); // respect passed in limits if any
        }

        // We want to determine the large file configuration based on the best defaults
        // for the resource but also respecting user settings. We only apply user settings
        // if explicitly configured by the user. Otherwise we pick the best limit for the
        // resource scheme.

        let default_size_limit = get_large_file_confirmation_limit(Some(&self.resource));
        let mut configured_size_limit: Option<u64> = None;

        let configured_size_limit_mb = self.text_resource_configuration_service.inspect(Some(&self.resource), None, "workbench.editorLargeFileConfirmation");
        if is_configured(configured_size_limit_mb.as_ref()) {
            configured_size_limit = Some((to_number(&configured_size_limit_mb.value()) * byte_size::MB as f64) as u64); // normalize to MB
        }

        Some(FileReadLimits { size: Some(configured_size_limit.unwrap_or(default_size_limit)) })
    }

    /// `dispose()` de `EditorInput`.
    pub fn dispose(&self) {
        self.base.dispose();
    }
}

/// Gera, para uma subclasse do `AbstractResourceEditorInput` que guarda a base no campo dado, os métodos de
/// [`EditorInput`] que a classe herda sem sobrescrever. Quem usa a macro lista os que quer herdar (`base`, `resource`,
/// `preferred_resource`, `capabilities`, `get_name`, `get_description`, `get_title` e `is_readonly`) e escreve à mão os
/// que sobrescreve.
#[macro_export]
macro_rules! inherit_abstract_resource_editor_input {
    (@method $field:ident, base) => {
        fn base(&self) -> &$crate::common::editor::editor_input::EditorInputBase {
            self.$field.base()
        }
    };
    (@method $field:ident, resource) => {
        fn resource(&self) -> Option<::je_base::uri::URI> {
            Some(self.$field.resource.clone())
        }
    };
    (@method $field:ident, preferred_resource) => {
        fn preferred_resource(&self) -> Option<::je_base::uri::URI> {
            Some(self.$field.preferred_resource())
        }
    };
    (@method $field:ident, capabilities) => {
        fn capabilities(&self) -> $crate::common::editor::EditorInputCapabilities {
            self.$field.capabilities()
        }
    };
    (@method $field:ident, get_name) => {
        fn get_name(&self) -> String {
            self.$field.get_name()
        }
    };
    (@method $field:ident, get_description) => {
        fn get_description(&self, verbosity: Option<$crate::common::editor::Verbosity>) -> Option<String> {
            self.$field.get_description(verbosity)
        }
    };
    (@method $field:ident, get_title) => {
        fn get_title(&self, verbosity: Option<$crate::common::editor::Verbosity>) -> String {
            self.$field.get_title(verbosity)
        }
    };
    (@method $field:ident, is_readonly) => {
        fn is_readonly(&self) -> $crate::services::files_configuration::files_configuration_service::Readonly {
            self.$field.is_readonly()
        }
    };
    ($field:ident; $($method:ident),* $(,)?) => {
        $( $crate::inherit_abstract_resource_editor_input!(@method $field, $method); )*
    };
}
