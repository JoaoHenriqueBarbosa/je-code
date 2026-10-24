//! Porte de `src/vs/workbench/browser/parts/editor/breadcrumbsModel.ts`: o modelo das migalhas de um recurso
//! ([`BreadcrumbsModel`]), que junta o caminho do arquivo (a partir da pasta do workspace ou do `home` de um formatador
//! de rótulo) com os símbolos do outline do editor, conforme `breadcrumbs.filePath` e `breadcrumbs.symbolPath`.
//!
//! Notas de tradução:
//! - **Injeção.** Os seis parâmetros decorados do construtor são a struct [`BreadcrumbsModelServices`]; `resource` e
//!   `editor` seguem posicionais. `readonly editor: IEditorPane | undefined` é `Option<Rc<dyn IEditorPane>>`.
//! - **`getElements(): ReadonlyArray<FileElement | OutlineElement2>`** é `Vec<BreadcrumbsElement>`, uma enumeração das
//!   duas formas. O `element: IOutline<unknown> | unknown` de `OutlineElement2` é o [`OutlineElementValue`].
//! - **`onDidUpdate: Event<this>`** é `Event<Rc<BreadcrumbsModel>>`: o modelo dispara a si mesmo. O `fire(this)` do fim
//!   do construtor roda depois que o `Rc` existe (ninguém pode ter ouvido antes).
//! - **`_initFilePathInfo`** é a função [`init_file_path_info`] (o construtor a chama antes de o modelo existir, e o
//!   `_updateFileInfo` a reaproveita).
//! - **`_bindToEditor`**: o `.then(...).catch(...)` encadeado é um [`Promise::then_result`] com os dois ramos, e o
//!   `CancellationTokenSource` é descartado com cancelamento pelo `toDisposable` que o guarda no
//!   `_outlineDisposables`.
//! - **`isEqual(a, b)`** de `resources.ts` é o [`ext_uri`] sem viés; o `this._uriIdentityService.extUri.isEqual` é o
//!   `ExtUri` do serviço de identidade de URIs.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::cancellation::CancellationTokenSource;
use je_base::errors::on_unexpected_error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable, MutableDisposable, to_disposable};
use je_base::network::{UriOrString, matches_some_scheme, schemas};
use je_base::uri::{Field, URI, UriChange};
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::files::FileKind;
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::resources::ext_uri;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::{IWorkspaceContextService, WorkbenchState, WorkspaceFolder};

use super::breadcrumbs::{BreadcrumbsConfig, BreadcrumbsConfigKeys, ShowPath};
use crate::common::editor::IEditorPane;
use crate::services::outline::outline::{IOutline, IOutlineService, OutlineElement, OutlineTarget};
use crate::services::workspaces::workspace_folder_label_service::IWorkspaceFolderLabelService;

/// `FileElement`.
#[derive(Clone, Debug)]
pub struct FileElement {
    pub uri: URI,
    pub kind: FileKind,
    pub label: Option<String>,
}

impl FileElement {
    pub fn new(uri: URI, kind: FileKind, label: Option<String>) -> Self {
        Self { uri, kind, label }
    }

    /// `equals(other)`: o mesmo recurso e o mesmo rótulo.
    pub fn equals(&self, other: &FileElement) -> bool {
        ext_uri().is_equal(Some(&self.uri), Some(&other.uri), false) && self.label == other.label
    }
}

/// O `element: IOutline<unknown> | unknown` de `OutlineElement2`.
#[derive(Clone)]
pub enum OutlineElementValue {
    /// O outline inteiro (quando o outline não tem elementos de migalha mas também não está vazio).
    Outline(Rc<dyn IOutline>),
    /// Um elemento do outline.
    Element(OutlineElement),
}

/// `OutlineElement2`.
#[derive(Clone)]
pub struct OutlineElement2 {
    pub element: OutlineElementValue,
    pub outline: Rc<dyn IOutline>,
}

/// `FileElement | OutlineElement2`.
#[derive(Clone)]
pub enum BreadcrumbsElement {
    File(FileElement),
    Outline(OutlineElement2),
}

/// `FileInfo`.
struct FileInfo {
    path: Vec<FileElement>,
    folder: Option<WorkspaceFolder>,
    home: Option<URI>,
}

/// Os serviços que o `BreadcrumbsModel` recebe por injeção.
#[derive(Clone)]
pub struct BreadcrumbsModelServices {
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub workspace_service: Rc<dyn IWorkspaceContextService>,
    pub workspace_folder_label_service: Rc<dyn IWorkspaceFolderLabelService>,
    pub outline_service: Rc<dyn IOutlineService>,
    pub label_service: Rc<dyn ILabelService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
}

/// `uri.with({ query: null, fragment: null })`.
fn without_query_and_fragment(uri: &URI) -> URI {
    uri.with(&UriChange { query: Field::Clear, fragment: Field::Clear, ..UriChange::default() })
}

/// `_initFilePathInfo(uri)`: o original sempre a chama com `this.resource`, que também é o recurso da comparação com a
/// pasta do workspace mais abaixo.
fn init_file_path_info(services: &BreadcrumbsModelServices, resource: &URI) -> FileInfo {
    let uri = resource;
    if matches_some_scheme(UriOrString::Uri(uri), &[schemas::UNTITLED, schemas::DATA]) {
        return FileInfo { folder: None, path: Vec::new(), home: None };
    }

    let mut info = FileInfo { folder: services.workspace_service.get_workspace_folder(uri), path: Vec::new(), home: services.label_service.get_uri_home(uri) };

    let uri_identity_ext_uri = services.uri_identity_service.ext_uri();
    let mut uri_prefix = uri.clone();
    while uri_prefix.path().to_string_lossy() != "/" {
        let at_folder = info.folder.as_ref().is_some_and(|folder| ext_uri().is_equal(Some(&folder.uri), Some(&uri_prefix), false));
        let at_home = info.home.as_ref().is_some_and(|home| uri_identity_ext_uri.is_equal(Some(home), Some(&without_query_and_fragment(&uri_prefix)), false));
        if at_folder || at_home {
            break;
        }
        let kind = if info.path.is_empty() { FileKind::File } else { FileKind::Folder };
        info.path.insert(0, FileElement::new(uri_prefix.clone(), kind, None));
        let prev_path_length = uri_prefix.path().units().len();
        uri_prefix = ext_uri().dirname(&uri_prefix);
        if uri_prefix.path().units().len() == prev_path_length {
            break;
        }
    }

    if let Some(home) = &info.home {
        let (scheme, authority) = (home.scheme().to_string_lossy(), home.authority().to_string_lossy());
        let separator = services.label_service.get_separator(&scheme, Some(authority.as_str()));
        let label = services.label_service.get_uri_label(home, &UriLabelOptions::default());
        let labels: Vec<&str> = label.split(separator.as_str()).filter(|part| !part.is_empty()).collect();
        for (index, part) in labels.iter().enumerate().rev() {
            let kind = if index == 0 { FileKind::RootFolder } else { FileKind::Folder };
            info.path.insert(0, FileElement::new(home.clone(), kind, Some((*part).to_owned())));
        }
    }

    if let Some(folder) = &info.folder {
        if services.workspace_service.get_workbench_state() == WorkbenchState::Workspace {
            let folder_count = services.workspace_service.get_workspace().folders().len();
            if folder_count > 1 || ext_uri().is_equal(Some(&folder.uri), Some(resource), false) {
                info.path.insert(0, FileElement::new(folder.uri.clone(), FileKind::RootFolder, services.workspace_folder_label_service.get_workspace_folder_label(folder, None)));
            }
        }
    }
    info
}

/// `BreadcrumbsModel`.
pub struct BreadcrumbsModel {
    this: Weak<Self>,
    pub resource: URI,
    pub editor: Option<Rc<dyn IEditorPane>>,
    services: BreadcrumbsModelServices,
    disposables: Rc<DisposableStore>,
    file_info: RefCell<FileInfo>,
    cfg_file_path: Rc<BreadcrumbsConfig<ShowPath>>,
    cfg_symbol_path: Rc<BreadcrumbsConfig<ShowPath>>,
    current_outline: Rc<MutableDisposable<dyn IOutline>>,
    outline_disposables: Rc<DisposableStore>,
    on_did_update: Rc<Emitter<Rc<BreadcrumbsModel>>>,
}

impl BreadcrumbsModel {
    pub fn new(resource: URI, editor: Option<Rc<dyn IEditorPane>>, services: BreadcrumbsModelServices) -> Rc<Self> {
        let cfg_file_path = BreadcrumbsConfigKeys::FILE_PATH.bind_to(services.configuration_service.clone());
        let cfg_symbol_path = BreadcrumbsConfigKeys::SYMBOL_PATH.bind_to(services.configuration_service.clone());
        let disposables = DisposableStore::new();

        let model = Rc::new_cyclic(|this: &Weak<Self>| {
            let fire_did_update = |this: &Weak<Self>| {
                let this = this.clone();
                move |_: &()| {
                    if let Some(model) = this.upgrade() {
                        model.fire_did_update();
                    }
                }
            };
            cfg_file_path.on_did_change().listen_in(fire_did_update(this), &disposables);
            cfg_symbol_path.on_did_change().listen_in(fire_did_update(this), &disposables);

            let weak = this.clone();
            services.workspace_service.on_did_change_workspace_folders().listen_in(
                move |_| {
                    if let Some(model) = weak.upgrade() {
                        model.update_file_info();
                    }
                },
                &disposables,
            );

            let weak = this.clone();
            let scheme = resource.scheme().to_string_lossy();
            services.label_service.on_did_change_formatters().listen_in(
                move |event| {
                    if event.scheme == scheme && let Some(model) = weak.upgrade() {
                        model.update_file_info();
                    }
                },
                &disposables,
            );

            let file_info = init_file_path_info(&services, &resource);
            Self {
                this: this.clone(),
                resource: resource.clone(),
                editor: editor.clone(),
                services: services.clone(),
                disposables: disposables.clone(),
                file_info: RefCell::new(file_info),
                cfg_file_path: cfg_file_path.clone(),
                cfg_symbol_path: cfg_symbol_path.clone(),
                current_outline: MutableDisposable::new(),
                outline_disposables: DisposableStore::new(),
                on_did_update: Emitter::new(),
            }
        });

        if let Some(editor) = &editor {
            model.bind_to_editor(editor.clone());

            let (weak, bound_editor) = (Rc::downgrade(&model), editor.clone());
            services.outline_service.on_did_change().listen_in(
                move |_| {
                    if let Some(model) = weak.upgrade() {
                        model.bind_to_editor(bound_editor.clone());
                    }
                },
                &model.disposables,
            );

            let (weak, bound_editor) = (Rc::downgrade(&model), editor.clone());
            editor.on_did_change_control().listen_in(
                move |_| {
                    if let Some(model) = weak.upgrade() {
                        model.bind_to_editor(bound_editor.clone());
                    }
                },
                &model.disposables,
            );
        }
        model.fire_did_update();
        model
    }

    /// `onDidUpdate`.
    pub fn on_did_update(&self) -> Event<Rc<BreadcrumbsModel>> {
        self.on_did_update.event()
    }

    /// `this._onDidUpdate.fire(this)`.
    fn fire_did_update(&self) {
        if let Some(model) = self.this.upgrade() {
            self.on_did_update.fire(model);
        }
    }

    /// `isRelative()`.
    pub fn is_relative(&self) -> bool {
        let file_info = self.file_info.borrow();
        file_info.folder.is_some() || file_info.home.is_some()
    }

    /// `getElements()`.
    pub fn get_elements(&self) -> Vec<BreadcrumbsElement> {
        let mut result: Vec<BreadcrumbsElement> = Vec::new();

        // file path elements
        let file_info = self.file_info.borrow();
        match self.cfg_file_path.get_value(None) {
            Some(ShowPath::On) => result.extend(file_info.path.iter().cloned().map(BreadcrumbsElement::File)),
            Some(ShowPath::Last) => result.extend(file_info.path.last().cloned().map(BreadcrumbsElement::File)),
            _ => {}
        }

        let symbol_path = self.cfg_symbol_path.get_value(None);
        if symbol_path == Some(ShowPath::Off) {
            return result;
        }

        let Some(outline) = self.current_outline.value() else {
            return result;
        };

        let breadcrumbs_elements = outline.config().breadcrumbs_data_source.get_breadcrumb_elements();
        let start = if symbol_path == Some(ShowPath::Last) && !breadcrumbs_elements.is_empty() { breadcrumbs_elements.len() - 1 } else { 0 };
        for breadcrumb in &breadcrumbs_elements[start..] {
            result.push(BreadcrumbsElement::Outline(OutlineElement2 { element: OutlineElementValue::Element(breadcrumb.element.clone()), outline: outline.clone() }));
        }

        if breadcrumbs_elements.is_empty() && !outline.is_empty() {
            result.push(BreadcrumbsElement::Outline(OutlineElement2 { element: OutlineElementValue::Outline(outline.clone()), outline: outline.clone() }));
        }

        result
    }

    /// `_updateFileInfo()`.
    fn update_file_info(&self) {
        let file_info = init_file_path_info(&self.services, &self.resource);
        *self.file_info.borrow_mut() = file_info;
        self.fire_did_update();
    }

    /// `_bindToEditor(editor)`.
    fn bind_to_editor(&self, editor: Rc<dyn IEditorPane>) {
        let new_cts = Rc::new(CancellationTokenSource::new(None));
        self.current_outline.clear();
        self.outline_disposables.clear();
        let cts_to_dispose = new_cts.clone();
        self.outline_disposables.add(to_disposable(move || cts_to_dispose.dispose_with(true)));

        let token = new_cts.token();
        let weak = self.this.clone();
        self.services.outline_service.create_outline(editor, OutlineTarget::BREADCRUMBS, token.clone()).then_result(move |result| {
            let Some(model) = weak.upgrade() else {
                return Ok(());
            };
            match result {
                Ok(mut outline) => {
                    if token.is_cancellation_requested() {
                        // cancelled: dispose new outline and reset
                        if let Some(outline) = &outline {
                            outline.dispose();
                        }
                        outline = None;
                    }
                    model.current_outline.set(outline.clone());
                    model.fire_did_update();
                    if let Some(outline) = outline {
                        let weak = model.this.clone();
                        model.outline_disposables.add(outline.on_did_change().listen(move |_| {
                            if let Some(model) = weak.upgrade() {
                                model.fire_did_update();
                            }
                        }));
                    }
                }
                Err(error) => {
                    model.fire_did_update();
                    on_unexpected_error(&error);
                }
            }
            Ok(())
        });
    }
}

impl IDisposable for BreadcrumbsModel {
    fn dispose(&self) {
        self.disposables.dispose();
        self.cfg_file_path.dispose();
        self.cfg_symbol_path.dispose();
        self.current_outline.dispose();
        self.outline_disposables.dispose();
        self.on_did_update.dispose();
    }
}
