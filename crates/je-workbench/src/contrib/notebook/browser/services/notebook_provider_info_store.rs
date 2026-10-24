//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/services/notebookServiceImpl.ts`: o `NotebookProviderInfoStore`, que guarda
//! os provedores de notebook contribuídos (por extensão ou do cache do `Memento`), os registra no `IEditorResolverService` (um editor
//! para o arquivo e outro, exclusivo, para as células) e diz quais provedores atendem um recurso. O `NotebookService` e o
//! `NotebookOutputRendererInfoStore` do mesmo arquivo ficam para outras fatias.
//!
//! Divergências:
//! - `NotebookEditorInput`, `NotebookDiffEditorInput`, `NotebookMultiDiffEditorInput`, `MergeEditorInput` e o
//!   `INotebookEditorModelResolverService` não têm porte: a criação das entradas e a leitura do índice da célula são o trait
//!   [`INotebookEditorInputFactory`], que quem monta o workbench implementa. As fábricas registradas no resolvedor fazem o restante do
//!   original (a leitura do URI da célula, as opções, o `Event.once(onWillDispose)` fica com a implementação do trait).
//! - `notebooksExtensionPoint.setHandler(...)` é [`NotebookProviderInfoStore::setup_handler`], chamado por quem liga o ponto de extensão;
//!   `INotebookEditorContribution` (o formato do ponto de extensão) é [`NotebookContribution`].
//! - `cellOptions` de `INotebookEditorOptions` (as opções da célula aberta) não existe no [`INotebookEditorOptions`] do porte: as opções
//!   de saída levam `cellSelections` e o `viewState` indefinido.
//! - `INotebookExclusiveDocumentFilter`/`canHandleDiff`: o `RegisteredEditorOptions` do porte só tem `canSupportResource`; a escolha dos
//!   dois fábricas de diff pelo `canHandleDiff` é feita em cada registro com as duas ouvintes de configuração e de leitor de tela.
//! - O `Memento` guarda objetos JavaScript: cada [`NotebookProviderInfo`] é gravado como `{ extension, id, displayName, priority,
//!   providerDisplayName, _selectors }` e relido pelo `NotebookProviderInfo::from_selectors`.
//! - `Symbol.iterator` é [`NotebookProviderInfoStore::iter`].
//! - O construtor devolve `Rc<Self>`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::network::schemas;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::accessibility::IAccessibilityService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::i_file_service::IFileService;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::common::editor::{EditorInputRef, EditorInputWithOptions, UntypedEditorInput};
use crate::common::memento::Memento;
use crate::contrib::notebook::browser::notebook_browser::INotebookEditorOptions;
use crate::contrib::notebook::common::notebook_common::{CellUri, NotebookEditorPriority, NotebookFilenamePattern, NotebookSetting};
use crate::contrib::notebook::common::notebook_provider::{NotebookEditorDescriptor, NotebookEditorDescriptorSelector, NotebookProviderInfo};
use crate::services::editor::editor_resolver_service::{
    EditorInputFactoryObject, GlobPattern, IEditorResolverService, RegisteredEditorOptions, RegisteredEditorPriority, RegisteredEditorRegistrationInfo,
    RegisteredEditorRegistrationPriority,
};
use crate::services::extensions::extensions::IExtensionService;
use crate::services::extensions::extensions_registry::ExtensionPointUser;

const CUSTOM_EDITORS_STORAGE_ID: &str = "notebookEditors";
const CUSTOM_EDITORS_ENTRY_ID: &str = "editors";

/// `INotebookEditorContribution` do ponto de extensão `notebooks`.
#[derive(Clone, Debug, Default)]
pub struct NotebookContribution {
    pub r#type: String,
    pub display_name: String,
    pub selector: Vec<NotebookEditorDescriptorSelector>,
    pub priority: Option<String>,
}

/// O que o armazenamento de provedores pede para criar as entradas de editor (ver as divergências do módulo).
pub trait INotebookEditorInputFactory {
    /// `NotebookEditorInput.getOrCreate(instantiationService, notebookUri, preferredResource, viewType)`.
    fn create_notebook_editor_input(&self, notebook_uri: &URI, preferred_resource: Option<&URI>, view_type: &str) -> EditorInputRef;
    /// As entradas de diff: `NotebookDiffEditorInput.create` ou, com `notebook.experimental.enableNewDiffEditor`,
    /// `NotebookMultiDiffEditorInput.create`.
    fn create_diff_editor_input(&self, modified: &URI, label: Option<&str>, description: Option<&str>, original: &URI, view_type: &str, multi_diff: bool) -> EditorInputRef;
    /// A entrada de mesclagem (`MergeEditorInput`) para a entrada sem tipo.
    fn create_merge_editor_input(&self, merge_editor: &UntypedEditorInput) -> EditorInputRef;
    /// A entrada de um notebook sem título (`resolve({ untitledResource })`, com o `ref.dispose()` no `onWillDispose` do notebook).
    fn create_untitled_notebook_editor_input(&self, resource: &URI, view_type: &str) -> Promise<EditorInputRef>;
    /// O índice da célula com a alça, no notebook (`0` quando não existe).
    fn find_cell_index(&self, notebook_uri: &URI, handle: i64) -> Promise<usize>;
}

/// `NotebookProviderInfoStore`.
pub struct NotebookProviderInfoStore {
    this: Weak<NotebookProviderInfoStore>,
    base: Rc<DisposableStore>,
    memento: Memento,
    handled: Cell<bool>,
    contributed_editors: RefCell<Vec<(String, Rc<NotebookProviderInfo>)>>,
    contributed_editor_disposables: Rc<DisposableStore>,
    editor_resolver_service: Rc<dyn IEditorResolverService>,
    configuration_service: Rc<dyn IConfigurationService>,
    accessibility_service: Rc<dyn IAccessibilityService>,
    file_service: Rc<dyn IFileService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    input_factory: Rc<dyn INotebookEditorInputFactory>,
}

fn priority_to_text(priority: RegisteredEditorPriority) -> &'static str {
    priority.as_str()
}

fn priority_from_text(text: &str) -> RegisteredEditorPriority {
    match text {
        "builtin" => RegisteredEditorPriority::Builtin,
        "option" => RegisteredEditorPriority::Option,
        "exclusive" => RegisteredEditorPriority::Exclusive,
        "explicit" => RegisteredEditorPriority::Explicit,
        _ => RegisteredEditorPriority::Default,
    }
}

fn dynamic_string(object: &ObjectRef, key: &str) -> Option<String> {
    match object.get(&JsString::from(key)) {
        Some(Dynamic::String(value)) => Some(value.to_string_lossy()),
        _ => None,
    }
}

/// O `NotebookProviderInfo` como objeto do `Memento`.
fn info_to_dynamic(info: &NotebookProviderInfo) -> Dynamic {
    let selectors = info.selectors().into_iter().map(|selector| match selector {
        NotebookFilenamePattern::Pattern(pattern) => Dynamic::object([("include", Dynamic::string(&pattern)), ("exclude", Dynamic::string(""))]),
        NotebookFilenamePattern::Relative(pattern) => Dynamic::object([("include", Dynamic::string(&pattern.pattern)), ("exclude", Dynamic::string(""))]),
        NotebookFilenamePattern::Exclusive(filter) => {
            let text = |item: &Option<je_base::glob::PatternItem>| match item {
                Some(je_base::glob::PatternItem::Str(text)) => Dynamic::string(text),
                Some(je_base::glob::PatternItem::Relative(relative)) => Dynamic::string(&relative.pattern),
                None => Dynamic::Undefined,
            };
            Dynamic::object([("include", text(&filter.include)), ("exclude", text(&filter.exclude))])
        }
    });
    Dynamic::object([
        ("extension", info.extension.as_ref().map_or(Dynamic::Undefined, |extension| Dynamic::object([("value", Dynamic::string(&extension.value))]))),
        ("id", Dynamic::string(&info.id)),
        ("displayName", Dynamic::string(&info.display_name)),
        ("priority", Dynamic::string(priority_to_text(info.priority))),
        ("providerDisplayName", Dynamic::string(&info.provider_display_name)),
        ("_selectors", Dynamic::array(selectors)),
    ])
}

fn info_from_dynamic(value: &Dynamic) -> Option<NotebookProviderInfo> {
    let Dynamic::Object(object) = value else { return None };
    let extension = match object.get(&JsString::from("extension")) {
        Some(Dynamic::Object(extension)) => dynamic_string(&extension, "value").map(|value| ExtensionIdentifier::new(&value)),
        _ => None,
    };
    let selectors = match object.get(&JsString::from("_selectors")) {
        Some(Dynamic::Array(selectors)) => (0..selectors.len())
            .filter_map(|index| match selectors.get(index) {
                Dynamic::Object(selector) => Some(NotebookFilenamePattern::Exclusive(crate::contrib::notebook::common::notebook_common::INotebookExclusiveDocumentFilter {
                    include: dynamic_string(&selector, "include").map(|text| je_base::glob::PatternItem::Str(text)),
                    exclude: Some(je_base::glob::PatternItem::Str(dynamic_string(&selector, "exclude").unwrap_or_default())),
                })),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let descriptor = NotebookEditorDescriptor {
        extension,
        id: dynamic_string(object, "id")?,
        display_name: dynamic_string(object, "displayName").unwrap_or_default(),
        selectors: Vec::new(),
        priority: priority_from_text(&dynamic_string(object, "priority").unwrap_or_default()),
        provider_display_name: dynamic_string(object, "providerDisplayName").unwrap_or_default(),
    };
    Some(NotebookProviderInfo::from_selectors(descriptor, selectors))
}

impl NotebookProviderInfoStore {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        storage_service: Rc<dyn StorageService>,
        extension_service: Rc<dyn IExtensionService>,
        editor_resolver_service: Rc<dyn IEditorResolverService>,
        configuration_service: Rc<dyn IConfigurationService>,
        accessibility_service: Rc<dyn IAccessibilityService>,
        file_service: Rc<dyn IFileService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        input_factory: Rc<dyn INotebookEditorInputFactory>,
    ) -> Rc<Self> {
        let base = DisposableStore::new();
        let contributed_editor_disposables = DisposableStore::new();
        base.add_rc(contributed_editor_disposables.clone());
        let memento = Memento::new(CUSTOM_EDITORS_STORAGE_ID, storage_service);

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            memento,
            handled: Cell::new(false),
            contributed_editors: RefCell::new(Vec::new()),
            contributed_editor_disposables,
            editor_resolver_service,
            configuration_service,
            accessibility_service,
            file_service,
            uri_identity_service,
            input_factory,
        });

        let memento_object = this.memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
        // Process the notebook contributions but buffer changes from the resolver
        let store = this.clone();
        let cached = memento_object.get(&JsString::from(CUSTOM_EDITORS_ENTRY_ID));
        this.editor_resolver_service.buffer_change_events(&move || {
            if let Some(Dynamic::Array(entries)) = &cached {
                for index in 0..entries.len() {
                    if let Some(info) = info_from_dynamic(&entries.get(index)) {
                        let _ = store.add(Rc::new(info), false);
                    }
                }
            }
        });

        let weak = this.this.clone();
        this.base.add(extension_service.on_did_register_extensions().listen(move |_| {
            let Some(this) = weak.upgrade() else { return };
            if !this.handled.get() {
                // there is no extension point registered for notebook content provider
                // clear the memento and cache
                this.clear_all();
                let memento_object = this.memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
                memento_object.set(&JsString::from(CUSTOM_EDITORS_ENTRY_ID), Dynamic::array(Vec::new()));
                this.memento.save_memento();
            }
        }));

        this
    }

    /// O tratador do ponto de extensão `notebooks` (`_setupHandler(extensions)`).
    pub fn setup_handler(&self, extensions: &[ExtensionPointUser<Vec<NotebookContribution>>]) {
        self.handled.set(true);
        let builtins: Vec<Rc<NotebookProviderInfo>> = self.contributed_editors.borrow().iter().filter(|(_, info)| info.extension.is_none()).map(|(_, info)| info.clone()).collect();
        self.clear_all();

        let mut builtin_providers_from_cache: Vec<(String, DisposableRef)> = Vec::new();
        for builtin in &builtins {
            builtin_providers_from_cache.push((builtin.id.clone(), self.add(builtin.clone(), true)));
        }

        for extension in extensions {
            for notebook_contribution in &extension.value {
                if notebook_contribution.r#type.is_empty() {
                    extension.collector.error("Notebook does not specify type-property");
                    continue;
                }

                let existing = self.get(&notebook_contribution.r#type);

                if existing.is_some() {
                    let is_cached_builtin = builtins.iter().any(|builtin| builtin.id == notebook_contribution.r#type);
                    if existing.is_some_and(|existing| existing.extension.is_none()) && extension.description.is_builtin && is_cached_builtin {
                        // we are registering an extension which is using the same view type which is already cached
                        if let Some((_, registration)) = builtin_providers_from_cache.iter().find(|(id, _)| *id == notebook_contribution.r#type) {
                            registration.dispose();
                        }
                    } else {
                        extension.collector.error(&format!("Notebook type '{}' already used", notebook_contribution.r#type));
                        continue;
                    }
                }

                let _ = self.add(
                    Rc::new(NotebookProviderInfo::new(NotebookEditorDescriptor {
                        extension: Some(extension.description.identifier.clone()),
                        id: notebook_contribution.r#type.clone(),
                        display_name: notebook_contribution.display_name.clone(),
                        selectors: notebook_contribution.selector.clone(),
                        priority: Self::convert_priority(notebook_contribution.priority.as_deref()),
                        provider_display_name: extension.description.display_name.clone().unwrap_or_else(|| extension.description.identifier.value.clone()),
                    })),
                    true,
                );
            }
        }

        self.save_editors_memento();
    }

    fn save_editors_memento(&self) {
        let memento_object = self.memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
        let entries: Vec<Dynamic> = self.contributed_editors.borrow().iter().map(|(_, info)| info_to_dynamic(info)).collect();
        memento_object.set(&JsString::from(CUSTOM_EDITORS_ENTRY_ID), Dynamic::array(entries));
        self.memento.save_memento();
    }

    pub fn clear_editor_cache(&self) {
        let memento_object = self.memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
        memento_object.set(&JsString::from(CUSTOM_EDITORS_ENTRY_ID), Dynamic::array(Vec::new()));
        self.memento.save_memento();
    }

    fn convert_priority(priority: Option<&str>) -> RegisteredEditorPriority {
        match priority {
            None | Some("") => RegisteredEditorPriority::Default,
            Some(priority) if priority == NotebookEditorPriority::Default.as_str() => RegisteredEditorPriority::Default,
            Some(_) => RegisteredEditorPriority::Option,
        }
    }

    fn can_handle_diff(configuration_service: &dyn IConfigurationService, accessibility_service: &dyn IAccessibilityService) -> bool {
        matches!(configuration_service.get_value(Some(NotebookSetting::TEXT_DIFF_EDITOR_PREVIEW), None), Dynamic::Bool(true)) && !accessibility_service.is_screen_reader_optimized()
    }

    fn register_contribution_point(&self, notebook_provider_info: &Rc<NotebookProviderInfo>) -> DisposableRef {
        let disposables = DisposableStore::new();

        for selector in notebook_provider_info.selectors() {
            let glob_pattern = match &selector {
                NotebookFilenamePattern::Exclusive(filter) => match &filter.include {
                    Some(je_base::glob::PatternItem::Str(pattern)) => GlobPattern::Pattern(pattern.clone()),
                    Some(je_base::glob::PatternItem::Relative(pattern)) => GlobPattern::Relative(Rc::new(pattern.clone())),
                    None => GlobPattern::Pattern(String::new()),
                },
                NotebookFilenamePattern::Pattern(pattern) => GlobPattern::Pattern(pattern.clone()),
                NotebookFilenamePattern::Relative(pattern) => GlobPattern::Relative(Rc::new(pattern.clone())),
            };
            let notebook_editor_info = RegisteredEditorRegistrationInfo {
                id: notebook_provider_info.id.clone(),
                label: notebook_provider_info.display_name.clone(),
                detail: Some(notebook_provider_info.provider_display_name.clone()),
                priority: RegisteredEditorRegistrationPriority::Single(notebook_provider_info.priority),
            };
            let file_service = self.file_service.clone();
            let notebook_editor_options = RegisteredEditorOptions {
                single_per_resource: None,
                can_support_resource: Some(Rc::new(move |resource: &URI| {
                    if resource.scheme() == schemas::VSCODE_NOTEBOOK_CELL_OUTPUT {
                        return resource.query().to_string_lossy().split('&').any(|pair| pair == "openIn=notebook");
                    }
                    let scheme = resource.scheme().to_string_lossy();
                    scheme == schemas::UNTITLED || scheme == schemas::VSCODE_NOTEBOOK_CELL || file_service.has_provider(resource)
                })),
            };

            let notebook_editor_input_factory = self.notebook_editor_input_factory(notebook_provider_info.id.clone());
            let notebook_untitled_editor_factory = {
                let input_factory = self.input_factory.clone();
                let view_type = notebook_provider_info.id.clone();
                Rc::new(move |input: &UntypedEditorInput, _group: &Rc<dyn crate::services::editor::editor_groups_service::IEditorGroup>| {
                    let resource = input.resource.clone().expect("o recurso sem título");
                    let options = input.options.clone();
                    MaybePromise::Promise(input_factory.create_untitled_notebook_editor_input(&resource, &view_type).then(move |editor| EditorInputWithOptions { editor, options }))
                })
            };
            let notebook_diff_editor_input_factory = {
                let input_factory = self.input_factory.clone();
                let configuration_service = self.configuration_service.clone();
                let view_type = notebook_provider_info.id.clone();
                Rc::new(move |input: &UntypedEditorInput, _group: &Rc<dyn crate::services::editor::editor_groups_service::IEditorGroup>| {
                    let modified = input.modified.as_ref().and_then(|modified| modified.resource.clone()).expect("o recurso modificado");
                    let original = input.original.as_ref().and_then(|original| original.resource.clone()).expect("o recurso original");
                    let multi_diff = matches!(configuration_service.get_value(Some("notebook.experimental.enableNewDiffEditor"), None), Dynamic::Bool(true));
                    MaybePromise::Value(EditorInputWithOptions {
                        editor: input_factory.create_diff_editor_input(&modified, input.label.as_deref(), input.description.as_deref(), &original, &view_type, multi_diff),
                        options: None,
                    })
                })
            };
            let merge_editor_input_factory = {
                let input_factory = self.input_factory.clone();
                Rc::new(move |input: &UntypedEditorInput, _group: &Rc<dyn crate::services::editor::editor_groups_service::IEditorGroup>| {
                    MaybePromise::Value(EditorInputWithOptions { editor: input_factory.create_merge_editor_input(input), options: None })
                })
            };

            let notebook_factory_object = Rc::new(RefCell::new(EditorInputFactoryObject {
                create_editor_input: Some(notebook_editor_input_factory.clone()),
                create_diff_editor_input: Some(notebook_diff_editor_input_factory.clone()),
                create_untitled_editor_input: Some(notebook_untitled_editor_factory),
                create_merge_editor_input: Some(merge_editor_input_factory),
                ..Default::default()
            }));
            let notebook_cell_factory_object = Rc::new(RefCell::new(EditorInputFactoryObject {
                create_editor_input: Some(notebook_editor_input_factory),
                create_diff_editor_input: Some(notebook_diff_editor_input_factory.clone()),
                ..Default::default()
            }));

            // TODO @lramos15 find a better way to toggle handling diff editors than needing these listeners for every registration
            // This is a lot of event listeners especially if there are many notebooks
            let toggle_diff = {
                let (configuration_service, accessibility_service) = (self.configuration_service.clone(), self.accessibility_service.clone());
                let (notebook_factory_object, notebook_cell_factory_object) = (notebook_factory_object.clone(), notebook_cell_factory_object.clone());
                let diff_factory = notebook_diff_editor_input_factory.clone();
                Rc::new(move || {
                    let can_handle_diff = Self::can_handle_diff(&*configuration_service, &*accessibility_service);
                    let factory = if can_handle_diff { Some(diff_factory.clone()) } else { None };
                    notebook_factory_object.borrow_mut().create_diff_editor_input = factory.clone();
                    notebook_cell_factory_object.borrow_mut().create_diff_editor_input = factory;
                })
            };
            {
                let toggle_diff = toggle_diff.clone();
                disposables.add(self.configuration_service.on_did_change_configuration().listen(move |e| {
                    if e.affects_configuration(NotebookSetting::TEXT_DIFF_EDITOR_PREVIEW, None) {
                        toggle_diff();
                    }
                }));
            }
            disposables.add(self.accessibility_service.on_did_change_screen_reader_optimized().listen(move |_| toggle_diff()));

            // Register the notebook editor
            disposables.add(self.editor_resolver_service.register_editor(
                glob_pattern.clone(),
                notebook_editor_info.clone(),
                notebook_editor_options.clone(),
                notebook_factory_object.borrow().clone(),
            ));
            // Then register the schema handler as exclusive for that notebook
            let pattern_text = match &glob_pattern {
                GlobPattern::Pattern(pattern) => pattern.clone(),
                GlobPattern::Relative(pattern) => pattern.pattern.clone(),
            };
            disposables.add(self.editor_resolver_service.register_editor(
                GlobPattern::Pattern(format!("{}:/**/{}", schemas::VSCODE_NOTEBOOK_CELL, pattern_text)),
                RegisteredEditorRegistrationInfo { priority: RegisteredEditorRegistrationPriority::Single(RegisteredEditorPriority::Exclusive), ..notebook_editor_info },
                notebook_editor_options,
                notebook_cell_factory_object.borrow().clone(),
            ));
        }

        let disposables_ref = disposables;
        to_disposable(move || disposables_ref.dispose())
    }

    /// `notebookEditorInputFactory`.
    fn notebook_editor_input_factory(&self, view_type: String) -> crate::services::editor::editor_resolver_service::EditorInputFactoryFunction {
        let input_factory = self.input_factory.clone();
        let uri_identity_service = self.uri_identity_service.clone();
        Rc::new(move |input: &UntypedEditorInput, _group| {
            let resource = input.resource.clone().expect("o recurso da entrada");
            let options = input.options.clone();
            let is_output = resource.scheme() == schemas::VSCODE_NOTEBOOK_CELL_OUTPUT;

            let (notebook_uri, cell_resource, handle) = if is_output {
                let output_uri_data = CellUri::parse_cell_output_uri(&resource);
                match output_uri_data {
                    Some(data) if data.cell_handle.is_some() => {
                        let handle = data.cell_handle.expect("a alça da célula");
                        (uri_identity_service.as_canonical_uri(&data.notebook), Some(CellUri::generate(&data.notebook, handle)), Some(handle))
                    }
                    _ => errors::throw(Error::new("Invalid cell output uri")),
                }
            } else {
                match CellUri::parse(&resource) {
                    // resource is a notebook cell
                    Some(data) => (uri_identity_service.as_canonical_uri(&data.notebook), Some(resource.clone()), Some(data.handle)),
                    None => (uri_identity_service.as_canonical_uri(&resource), None, None),
                }
            };

            let preferred_resource = cell_resource.clone();
            let (input_factory_for_editor, view_type) = (input_factory.clone(), view_type.clone());
            let build = move |notebook_options: Option<crate::common::editor::TextEditorOptions>| {
                let editor = input_factory_for_editor.create_notebook_editor_input(&notebook_uri, preferred_resource.as_ref(), &view_type);
                EditorInputWithOptions { editor, options: notebook_options }
            };

            let mut notebook_options = options.unwrap_or_default();
            notebook_options.base.view_state = None;
            if is_output {
                let Some(handle) = handle else { errors::throw(Error::new("Invalid cell handle")) };
                let cell_index = input_factory.find_cell_index(&uri_identity_service.as_canonical_uri(&CellUri::parse(&cell_resource.clone().expect("o URI da célula")).expect("o URI da célula").notebook), handle);
                return MaybePromise::Promise(cell_index.then(move |cell_index| {
                    let selections = vec![crate::contrib::notebook::common::notebook_range::ICellRange { start: cell_index as i64, end: cell_index as i64 + 1 }];
                    let output_options = INotebookEditorOptions { base: notebook_options, cell_selections: Some(selections), label: None };
                    build(Some(output_options.to_text_editor_options()))
                }));
            }
            MaybePromise::Value(build(Some(notebook_options)))
        })
    }

    fn clear_all(&self) {
        self.contributed_editors.borrow_mut().clear();
        self.contributed_editor_disposables.clear();
    }

    pub fn get(&self, view_type: &str) -> Option<Rc<NotebookProviderInfo>> {
        self.contributed_editors.borrow().iter().find(|(id, _)| id == view_type).map(|(_, info)| info.clone())
    }

    pub fn add(&self, info: Rc<NotebookProviderInfo>, save_memento: bool) -> DisposableRef {
        if self.contributed_editors.borrow().iter().any(|(id, _)| *id == info.id) {
            errors::throw(Error::new(format!("notebook type '{}' ALREADY EXISTS", info.id)));
        }
        self.contributed_editors.borrow_mut().push((info.id.clone(), info.clone()));
        let mut editor_registration: Option<DisposableRef> = None;

        // built-in notebook providers contribute their own editors
        if info.extension.is_some() {
            let registration = self.register_contribution_point(&info);
            self.contributed_editor_disposables.add(registration.clone());
            editor_registration = Some(registration);
        }

        if save_memento {
            self.save_editors_memento();
        }

        let weak = self.this.clone();
        let id = info.id.clone();
        let registration = to_disposable(move || {
            let Some(this) = weak.upgrade() else { return };
            this.save_editors_memento();
            if let Some(editor_registration) = &editor_registration {
                editor_registration.dispose();
            }
            this.contributed_editors.borrow_mut().retain(|(candidate, _)| *candidate != id);
        });
        self.base.add(registration)
    }

    pub fn get_contributed_notebook(&self, resource: &URI) -> Vec<Rc<NotebookProviderInfo>> {
        let result: Vec<Rc<NotebookProviderInfo>> = self.contributed_editors.borrow().iter().filter(|(_, info)| info.matches(resource)).map(|(_, info)| info.clone()).collect();
        if result.is_empty() && resource.scheme() == schemas::UNTITLED {
            // untitled resource and no path-specific match => all providers apply
            return self.iter();
        }
        result
    }

    /// `[Symbol.iterator]()`.
    pub fn iter(&self) -> Vec<Rc<NotebookProviderInfo>> {
        self.contributed_editors.borrow().iter().map(|(_, info)| info.clone()).collect()
    }
}

impl IDisposable for NotebookProviderInfoStore {
    fn dispose(&self) {
        self.clear_all();
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookProviderInfoStore"
    }
}
