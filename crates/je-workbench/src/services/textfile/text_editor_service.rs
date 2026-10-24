//! Porte de `src/vs/workbench/services/textfile/common/textEditorService.ts`: o contrato `ITextEditorService` e a classe
//! [`TextEditorService`], que cria entradas de editor de texto a partir de uma entrada sem tipo (arquivo, sem título e
//! recurso virtual), guarda cada entrada por recurso e registra o editor de texto como editor padrão do resolvedor.
//!
//! Divergências:
//! - **Sobrecargas.** As duas sobrecargas de `createTextEditor` e de `resolveTextEditor` (`IUntypedEditorInput` devolve
//!   `EditorInput` e `IUntypedFileEditorInput` devolve `IFileEditorInput`) são um método só por operação: o
//!   `IFileEditorInput` ainda não foi portado, então o contrato devolve a [`EditorInputRef`].
//! - **`fileEditorFactory`.** O `IFileEditorFactory` (`getFileEditorFactory().createFileEditor(...)`) não foi portado (ver
//!   `editor_factory_registry`): a única implementação dele (`FileEditorInputFactory`, de `files.contribution.ts`) é
//!   `instantiationService.createInstance(FileEditorInput, resource, preferredResource, preferredName, preferredDescription,
//!   preferredEncoding, preferredLanguageId, preferredContents)`, que [`TextEditorService::create_file_editor`] faz direto.
//! - **Entradas do cache.** O `TextResourceEditorInput | IFileEditorInput | UntitledTextEditorInput` do cache é a
//!   [`EditorInputRef`]; o `instanceof` do `cachedFn` é o `downcast_ref` por [`EditorInput::as_any`] (sem título, depois
//!   recurso de texto, e o resto é arquivo).
//! - **Serviço do editor.** `createInstance(DiffEditorInput, ...)` e `createInstance(SideBySideEditorInput, ...)` injetam o
//!   `IEditorService`: as duas classes recebem o serviço por argumento, que o `TextEditorService` lê do serviço de
//!   instanciação na hora (o `IEditorService` depende do `ITextEditorService`, então ele não pode entrar no construtor).
//! - **Registro padrão.** As fábricas do editor padrão guardam um `Weak` do serviço (o resolvedor as guarda, e um `Rc`
//!   formaria um ciclo).
//! - **Monitor de vazamentos.** O `new Error().stack` do `leakId` não existe: o identificador é só
//!   `scheme#typeId#editorId`, e o `FileEditorInputLeakError` leva o identificador mais frequente na mensagem, no lugar da
//!   pilha. O `LEAK_REPORTED` estático é um `thread_local`.
//! - **`Event.once`.** O ouvinte de `onWillDispose` fica registrado no `Disposable` do serviço.
//! - **Erro final.** O `throw new Error('ITextEditorService: Unable to create texteditor from ...')` só vale para um recurso
//!   que não é `URI`, o que o tipo da entrada sem tipo não permite (sem recurso é sem título); o ramo não existe.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::errors::{Error, on_unexpected_error};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::ResourceMap;
use je_base::network::schemas;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_instantiation::{Injectable, InstantiationService, InstantiationType, register_singleton, service_id, service_impl};
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::ext_uri;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::common::editor::diff_editor_input::DiffEditorInput;
use crate::common::editor::side_by_side_editor_input::SideBySideEditorInput;
use crate::common::editor::text_resource_editor_input::TextResourceEditorInput;
use crate::common::editor::{EditorInputRef, EditorInputWithOptions, UntypedEditorInput, default_editor_association};
use crate::contrib::files::browser::editors::file_editor_input::FileEditorInput;
use crate::services::editor::editor_groups_service::IEditorGroup;
use crate::services::editor::editor_resolver_service::{
    EditorInputFactoryFunction, EditorInputFactoryObject, GlobPattern, IEditorResolverService, RegisteredEditorOptions, RegisteredEditorPriority,
    RegisteredEditorRegistrationInfo,
};
use crate::services::editor::editor_service::{IEditorService, I_EDITOR_SERVICE};
use crate::services::untitled::untitled_text_editor_input::UntitledTextEditorInput;
use crate::services::untitled::untitled_text_editor_service::{AssociatedResource, IUntitledTextEditorService, UntitledTextEditorOptions};

/// `ITextEditorService`.
pub trait ITextEditorService {
    /// `createTextEditor(input)`. @deprecated this method should not be used, rather consider using `IEditorResolverService`
    /// instead with `DEFAULT_EDITOR_ASSOCIATION.id`.
    fn create_text_editor(&self, input: UntypedEditorInput) -> EditorInputRef;

    /// A way to create text editor inputs from an untyped editor input. Depending on the passed in input this will be:
    /// - a `IFileEditorInput` for file resources
    /// - a `UntitledEditorInput` for untitled resources
    /// - a `TextResourceEditorInput` for virtual resources
    fn resolve_text_editor(&self, input: UntypedEditorInput) -> Promise<EditorInputRef>;
}

service_id!(pub I_TEXT_EDITOR_SERVICE: dyn ITextEditorService = "textEditorService");

/// `LEAK_TRACKING_THRESHOLD`.
const LEAK_TRACKING_THRESHOLD: usize = 256;

/// `LEAK_REPORTING_THRESHOLD`.
const LEAK_REPORTING_THRESHOLD: usize = 2 * LEAK_TRACKING_THRESHOLD;

thread_local! {
    /// `TextEditorService.LEAK_REPORTED`.
    static LEAK_REPORTED: Cell<bool> = const { Cell::new(false) };
}

/// `TextEditorService`.
pub struct TextEditorService {
    this: Weak<TextEditorService>,
    base: Rc<Disposable>,

    /// `editorInputCache`.
    editor_input_cache: RefCell<ResourceMap<EditorInputRef>>,
    /// `mapLeakToCounter`.
    map_leak_to_counter: RefCell<HashMap<String, usize>>,

    untitled_text_editor_service: Rc<dyn IUntitledTextEditorService>,
    instantiation_service: Rc<InstantiationService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    file_service: Rc<dyn IFileService>,
    editor_resolver_service: Rc<dyn IEditorResolverService>,
}

impl Injectable for TextEditorService {
    type Args = ();
    type Deps = (
        Rc<dyn IUntitledTextEditorService>,
        Rc<InstantiationService>,
        Rc<dyn IUriIdentityService>,
        Rc<dyn IFileService>,
        Rc<dyn IEditorResolverService>,
    );

    fn create(
        (): Self::Args,
        (untitled_text_editor_service, instantiation_service, uri_identity_service, file_service, editor_resolver_service): Self::Deps,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base: Disposable::new(),
            editor_input_cache: RefCell::new(ResourceMap::new()),
            map_leak_to_counter: RefCell::new(HashMap::new()),
            untitled_text_editor_service,
            instantiation_service,
            uri_identity_service,
            file_service,
            editor_resolver_service,
        });

        // Register the default editor to the editor resolver
        // service so that it shows up in the editors picker
        this.register_default_editor();

        this
    }
}

service_impl!(TextEditorService => dyn ITextEditorService, disposable);

/// `registerSingleton(ITextEditorService, TextEditorService, InstantiationType.Eager)`: do not change
/// (https://github.com/microsoft/vscode/issues/137675).
pub fn register() {
    register_singleton::<TextEditorService>(I_TEXT_EDITOR_SERVICE, InstantiationType::Eager);
}

impl IDisposable for TextEditorService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl TextEditorService {
    fn register_default_editor(&self) {
        // Uma fábrica por forma de entrada: o `createTextEditor` do serviço com a entrada dada.
        let factory = |weak: Weak<TextEditorService>| -> EditorInputFactoryFunction {
            Rc::new(move |editor: &UntypedEditorInput, _group: &Rc<dyn IEditorGroup>| {
                let this = weak.upgrade().expect("o serviço está vivo enquanto o resolvedor o usa");

                MaybePromise::Value(EditorInputWithOptions { editor: this.create_text_input(editor), options: None })
            })
        };

        let default_editor = default_editor_association();
        self.base.register(self.editor_resolver_service.register_editor(
            GlobPattern::from("*"),
            RegisteredEditorRegistrationInfo {
                id: default_editor.id.to_string(),
                label: default_editor.display_name,
                detail: Some(default_editor.provider_display_name),
                priority: RegisteredEditorPriority::Builtin.into(),
            },
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject {
                create_editor_input: Some(factory(self.this.clone())),
                create_untitled_editor_input: Some(factory(self.this.clone())),
                create_diff_editor_input: Some(factory(self.this.clone())),
                ..EditorInputFactoryObject::default()
            },
        ));
    }

    /// `this.instantiationService.createInstance(...)` das classes que injetam o `IEditorService`.
    fn editor_service(&self) -> Rc<dyn IEditorService> {
        self.instantiation_service.invoke_function(|accessor| accessor.get(I_EDITOR_SERVICE))
    }

    /// O corpo de `createTextEditor`, que chama a si mesmo para as partes de diff, lado a lado e merge.
    fn create_text_input(&self, input: &UntypedEditorInput) -> EditorInputRef {
        // Merge Editor Not Supported (we fallback to showing the result only)
        if input.is_resource_merge_editor_input() {
            return self.create_text_input(input.result.as_deref().expect("o merge tem `result`"));
        }

        // Diff Editor Support
        if input.is_resource_diff_editor_input() {
            let original = self.create_text_input(input.original.as_deref().expect("o diff tem `original`"));
            let modified = self.create_text_input(input.modified.as_deref().expect("o diff tem `modified`"));

            return DiffEditorInput::new(input.label.as_deref(), input.description.as_deref(), original, modified, None, self.editor_service());
        }

        // Side by Side Editor Support
        if input.is_resource_side_by_side_editor_input() {
            let primary = self.create_text_input(input.primary.as_deref().expect("o lado a lado tem `primary`"));
            let secondary = self.create_text_input(input.secondary.as_deref().expect("o lado a lado tem `secondary`"));

            return SideBySideEditorInput::new(input.label.as_deref(), input.description.as_deref(), secondary, primary, self.editor_service());
        }

        // Untitled text file support
        let Some(resource) = input.resource.clone().filter(|_| input.force_untitled != Some(true)).filter(|resource| *resource.scheme() != schemas::UNTITLED)
        else {
            return self.create_untitled_editor(input);
        };

        // Text File/Resource Editor Support
        self.create_text_resource_editor(input, resource)
    }

    /// O ramo `forceUntitled || !resource || scheme === untitled` de `createTextEditor`.
    fn create_untitled_editor(&self, input: &UntypedEditorInput) -> EditorInputRef {
        let mut untitled_options = UntitledTextEditorOptions { language_id: input.language_id.clone(), initial_value: input.contents.clone(), encoding: input.encoding.clone(), ..UntitledTextEditorOptions::default() };

        // Untitled resource: use as hint for an existing untitled editor
        match &input.resource {
            Some(resource) if *resource.scheme() == schemas::UNTITLED => untitled_options.untitled_resource = Some(resource.clone()),

            // Other resource: use as hint for associated filepath
            other => untitled_options.associated_resource = other.as_ref().map(AssociatedResource::from),
        }
        let untitled_model = self.untitled_text_editor_service.create(Some(untitled_options));

        let instantiation_service = self.instantiation_service.clone();
        self.create_or_get_cached(&untitled_model.resource(), move || instantiation_service.create_instance::<UntitledTextEditorInput>((untitled_model,)), |_| {})
    }

    /// O ramo `Text File/Resource Editor Support` de `createTextEditor`.
    fn create_text_resource_editor(&self, input: &UntypedEditorInput, resource: URI) -> EditorInputRef {
        // Derive the label from the path if not provided explicitly
        let label = input.label.clone().filter(|label| !label.is_empty()).unwrap_or_else(|| ext_uri().basename(&resource, None));

        // We keep track of the preferred resource this input is to be created
        // with but it may be different from the canonical resource (see below)
        let preferred_resource = resource;

        // From this moment on, only operate on the canonical resource
        // to ensure we reduce the chance of opening the same resource
        // with different resource forms (e.g. path casing on Windows)
        let canonical_resource = self.uri_identity_service.as_canonical_uri(&preferred_resource);

        let factory = {
            let (instantiation_service, file_service) = (self.instantiation_service.clone(), self.file_service.clone());
            let (input, canonical_resource, preferred_resource) = (input.clone(), canonical_resource.clone(), preferred_resource.clone());
            move || -> EditorInputRef {
                // File
                if input.force_file == Some(true) || file_service.has_provider(&canonical_resource) {
                    return Self::create_file_editor(
                        &instantiation_service,
                        canonical_resource,
                        preferred_resource,
                        input.label.clone(),
                        input.description.clone(),
                        input.encoding.clone(),
                        input.language_id.clone(),
                        input.contents.clone(),
                    );
                }

                // Resource
                instantiation_service.create_instance::<TextResourceEditorInput>((
                    canonical_resource,
                    input.label.clone(),
                    input.description.clone(),
                    input.language_id.clone(),
                    input.contents.clone(),
                ))
            }
        };

        let cached_fn = {
            let input = input.clone();
            move |cached_input: &EditorInputRef| {
                // Untitled
                if cached_input.as_any().is::<UntitledTextEditorInput>() {
                    return;
                }

                match cached_input.as_any().downcast_ref::<TextResourceEditorInput>() {
                    // Resources
                    Some(cached_input) => {
                        if !label.is_empty() {
                            cached_input.set_name(&label);
                        }

                        if let Some(description) = input.description.as_deref().filter(|description| !description.is_empty()) {
                            cached_input.set_description(description);
                        }

                        if let Some(language_id) = input.language_id.as_deref().filter(|language_id| !language_id.is_empty()) {
                            cached_input.set_preferred_language_id(language_id);
                        }

                        if let Some(contents) = input.contents.as_deref() {
                            cached_input.set_preferred_contents(contents);
                        }
                    }

                    // Files
                    None => {
                        let cached_input = cached_input.as_any().downcast_ref::<FileEditorInput>().expect("a entrada do cache é de arquivo, de recurso ou sem título");
                        cached_input.set_preferred_resource(&preferred_resource);

                        if let Some(label) = input.label.as_deref().filter(|label| !label.is_empty()) {
                            cached_input.set_preferred_name(label);
                        }

                        if let Some(description) = input.description.as_deref().filter(|description| !description.is_empty()) {
                            cached_input.set_preferred_description(description);
                        }

                        if let Some(encoding) = input.encoding.as_deref().filter(|encoding| !encoding.is_empty()) {
                            cached_input.set_preferred_encoding(encoding);
                        }

                        if let Some(language_id) = input.language_id.as_deref().filter(|language_id| !language_id.is_empty()) {
                            cached_input.set_preferred_language_id(language_id);
                        }

                        if let Some(contents) = input.contents.as_deref() {
                            cached_input.set_preferred_contents(contents);
                        }
                    }
                }
            }
        };

        self.create_or_get_cached(&canonical_resource, factory, cached_fn)
    }

    /// `fileEditorFactory.createFileEditor(resource, preferredResource, preferredName, preferredDescription,
    /// preferredEncoding, preferredLanguageId, preferredContents, instantiationService)`.
    #[allow(clippy::too_many_arguments)]
    fn create_file_editor(
        instantiation_service: &InstantiationService,
        resource: URI,
        preferred_resource: URI,
        preferred_name: Option<String>,
        preferred_description: Option<String>,
        preferred_encoding: Option<String>,
        preferred_language_id: Option<String>,
        preferred_contents: Option<String>,
    ) -> EditorInputRef {
        instantiation_service.create_instance::<FileEditorInput>((
            resource,
            Some(preferred_resource),
            preferred_name,
            preferred_description,
            preferred_encoding,
            preferred_language_id,
            preferred_contents,
        ))
    }

    fn create_or_get_cached(&self, resource: &URI, factory_fn: impl FnOnce() -> EditorInputRef, cached_fn: impl FnOnce(&EditorInputRef)) -> EditorInputRef {
        // Return early if already cached
        let cached = self.editor_input_cache.borrow().get(resource).cloned();
        if let Some(input) = cached {
            cached_fn(&input);

            return input;
        }

        // Otherwise create and add to cache
        let input = factory_fn();
        self.editor_input_cache.borrow_mut().set(resource.clone(), input.clone());

        // Track Leaks
        let leak_id = self.track_leaks(&input);

        let weak = self.this.clone();
        let resource = resource.clone();
        self.base.register(input.on_will_dispose().once().listen(move |_| {
            let Some(this) = weak.upgrade() else { return };

            // Remove from cache
            this.editor_input_cache.borrow_mut().delete(&resource);

            // Untrack Leaks
            if let Some(leak_id) = &leak_id {
                this.untrack_leaks(leak_id);
            }
        }));

        input
    }

    //#region Leak Monitoring

    fn track_leaks(&self, input: &EditorInputRef) -> Option<String> {
        let cache_size = self.editor_input_cache.borrow().size();
        if LEAK_REPORTED.with(Cell::get) || cache_size < LEAK_TRACKING_THRESHOLD {
            return None;
        }

        let scheme = input.resource().map(|resource| resource.scheme().to_string_lossy()).unwrap_or_default();
        let type_id = Some(input.editor_type_id()).filter(|type_id| !type_id.is_empty()).unwrap_or_else(|| "<no typeId>".to_string());
        let editor_id = input.editor_id().filter(|editor_id| !editor_id.is_empty()).unwrap_or_else(|| "<no editorId>".to_string());
        let leak_id = format!("{scheme}#{type_id}#{editor_id}\n");

        {
            let mut counters = self.map_leak_to_counter.borrow_mut();
            let leak_counter = counters.get(&leak_id).copied().unwrap_or(0) + 1;
            counters.insert(leak_id.clone(), leak_counter);
        }

        if cache_size > LEAK_REPORTING_THRESHOLD {
            LEAK_REPORTED.with(|reported| reported.set(true));

            let (top_leak, top_count) = {
                let counters = self.map_leak_to_counter.borrow();
                let mut top: Option<(String, usize)> = None;
                for (key, value) in counters.iter() {
                    match &top {
                        Some((_, top_count)) if value <= top_count => {}
                        _ => top = Some((key.clone(), *value)),
                    }
                }

                top.expect("o contador do vazamento atual existe")
            };

            let message = format!("Potential text editor input LEAK detected, having {cache_size} text editor inputs already. Most frequent owner ({top_count})");
            on_unexpected_error(&Error::new(format!("FileEditorInputLeakError: {message}\n{top_leak}")));
        }

        Some(leak_id)
    }

    fn untrack_leaks(&self, leak_id: &str) {
        let mut counters = self.map_leak_to_counter.borrow_mut();
        let stack_counter = counters.get(leak_id).copied().unwrap_or(1) - 1;
        counters.insert(leak_id.to_string(), stack_counter);

        if stack_counter == 0 {
            counters.remove(leak_id);
        }
    }

    //#endregion
}

impl ITextEditorService for TextEditorService {
    fn create_text_editor(&self, input: UntypedEditorInput) -> EditorInputRef {
        self.create_text_input(&input)
    }

    fn resolve_text_editor(&self, input: UntypedEditorInput) -> Promise<EditorInputRef> {
        Promise::resolved(self.create_text_input(&input))
    }
}
