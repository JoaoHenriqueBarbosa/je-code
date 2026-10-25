//! Porte de `src/vs/workbench/browser/parts/editor/breadcrumbsPicker.ts`: o seletor que as migalhas abrem sob uma
//! migalha, com a árvore de arquivos do workspace ([`BreadcrumbsFilePicker`]) ou a de símbolos do outline
//! ([`BreadcrumbsOutlinePicker`]).
//!
//! Notas de tradução:
//! - **`abstract class BreadcrumbsPicker<TInput, TElement>`** é a struct [`BreadcrumbsPicker`] (o que as duas classes
//!   compartilham) com o trait [`IBreadcrumbsPickerKind`] nos quatro métodos abstratos (`_createTree`, `_setInput`,
//!   `_previewElement`, `_revealElement`). `BreadcrumbsFilePicker` e `BreadcrumbsOutlinePicker` são construtores que devolvem
//!   o `Rc<BreadcrumbsPicker>` com o seu `kind`. O override de `restoreViewState` do outline é
//!   [`IBreadcrumbsPickerKind::on_restore_view_state`].
//! - **`type Tree<I, E> = WorkbenchDataTree | WorkbenchAsyncDataTree`** é o [`PickerTree`]: o que a base lê de qualquer das duas
//!   árvores (os três eventos, a altura do conteúdo, `layout`, `domFocus`, `dispose`) com os elementos como
//!   [`PickerTreeElement`]. A `WorkbenchXTree` é a árvore de `je-widgets` mais o [`ResourceNavigator`] (o `onDidOpen`); o
//!   contexto, o registro no `IListService` e a leitura de estilos do `WorkbenchTreeInternals` não entram.
//! - **`SelectEvent`** (interface exportada e sem uso no arquivo) não é portado.
//! - **`IListAccessibilityProvider`** (`FileAccessibilityProvider`) não tem campo nas opções das árvores de `je-widgets`, então não
//!   é portado; os rótulos de acessibilidade dos itens ficam com o `IconLabel`.
//! - **`overrideStyles: { listBackground }`** é um `tree.style(IListStyles)` aplicado uma vez, na criação (o
//!   `WorkbenchTreeInternals` o reaplicaria a cada troca de tema; o seletor vive pouco).
//! - **`IFileIconTheme`** é o `IFileIconTheme` de `notebook_outline.rs`, e `getFileIconTheme()`/`onDidFileIconThemeChange` (que o
//!   `IThemeService` daqui não traz) chegam por `file_icon_theme` e `on_did_file_icon_theme_change` em
//!   [`BreadcrumbsPickerServices`].
//! - **`IWorkspace | URI`** da entrada da árvore de arquivos é o [`FilePickerInput`]; `IWorkspaceFolder | IFileStat` é o
//!   [`FileTreeElement`] (igualdade e hash pelo id do `FileIdentityProvider`, o URI em texto).
//! - **`IOutline<unknown>` como entrada** da árvore do outline é o `Rc<dyn IOutline>`, e o `unknown` dos elementos é o
//!   [`OutlineTreeElement`] (ver `outline.rs`).
//! - **O contêiner da seta.** O `.arrow` do CSS (largura zero e três bordas transparentes) é um triângulo; o `je-ui` não tem
//!   primitiva de triângulo, então o nó `.arrow` recebe a geometria e a classe (a cor entra em [`BreadcrumbsPicker::arrow_color`])
//!   e a pintura fica para quando a primitiva existir. O `box-shadow` (`--vscode-shadow-lg`) e o `padding-top: 2px` do contêiner
//!   da árvore também não têm campo no `NodeStyle`.
//! - **`ILayoutInfo.inputHeight`** (sempre 0 e sem leitor) não é portado.
//! - **`setTimeout(() => this._tree.dispose(), 0)`** é um `disposable_timeout` de 0 ms.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use je_base::comparers::compare_file_names;
use je_base::dynamic::Dynamic;
use je_base::errors::on_unexpected_error;
use je_base::event::{Emitter, Event};
use je_base::filters::{FuzzyScore, create_matches};
use je_base::glob::{self, Expression, Glob, GlobOptions, ParsedExpression, ParsedGlob};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, MutableDisposable, none};
use je_base::nls::localize;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_base::async_::disposable_timeout;
use je_editor::text_resource_configuration_service::ITextResourceConfigurationService;
use je_editor::services::theme_service::IThemeService;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::files::{FileKind, FileStat, FileSystemProviderCapabilities};
use je_platform::files::i_file_service::IFileService;
use je_platform::opener::editor_types::EditorOptions;
use je_platform::resources::ext_uri;
use je_platform::workspace::{IWorkspaceContextService, Workspace, WorkspaceFolder};
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::to_rgba8;
use je_ui::{NodeId, Role, Ui};
use je_widgets::dom::create_el;
use je_widgets::icon_label::{IIconLabelCreationOptions, IIconLabelValueOptions};
use je_widgets::list::list_styles::IListStyles;
use je_widgets::list::list_types::{BrowserEvent, IIdentityProvider, IKeyboardNavigationLabelProvider, IListVirtualDelegate, TemplateData};
use je_widgets::tree::abstract_tree::ExpandOnlyOnTwistieClick;
use je_widgets::tree::async_data_tree::{AsyncDataTree, ObjectBackend};
use je_widgets::tree::async_data_tree_node::{AsyncDataTreeNodeWrapper, AsyncDataTreeOptions, IAsyncDataTreeRenderer};
use je_widgets::tree::data_tree::DataTree;
use je_widgets::tree::tree_types::{IAsyncDataSource, ITreeFilter, ITreeSorter, InputOrElement, TreeElementRenderDetails, TreeEvent, TreeFilterResult, TreeNodeApi, TreeVisibility};

use super::breadcrumbs::{BreadcrumbsConfig, BreadcrumbsConfigKeys, SymbolSortOrder};
use super::breadcrumbs_model::{BreadcrumbsElement, FileElement, OutlineElement2, OutlineElementValue};
use crate::browser::labels::{FileDecorations, IFileLabelOptions, IResourceLabel, IResourceLabelOptions, ResourceLabels, ResourceLabelsServices, default_labels_container};
use crate::browser::resource_navigator::{IOpenEvent, IResourceNavigatorOptions, ResourceNavigator};
use crate::contrib::notebook::browser::contrib::outline::notebook_outline::IFileIconTheme;
use crate::services::editor::editor_groups_service::PreferredGroup;
use crate::services::editor::editor_service::IEditorService;
use crate::services::outline::outline::{IOutline, IOutlineComparator, OutlineTreeElement};

/// `breadcrumbsPickerBackground`.
const BREADCRUMBS_PICKER_BACKGROUND: &str = "breadcrumbPicker.background";
/// `widgetBorder`.
const WIDGET_BORDER: &str = "widget.border";

/// `ILayoutInfo`.
#[derive(Clone, Copy, Debug)]
struct LayoutInfo {
    max_height: f64,
    width: f64,
    arrow_size: f64,
    arrow_offset: f64,
}

/// O `unknown` dos elementos que a base lê das árvores (o alvo de um evento de abertura ou de foco).
#[derive(Clone)]
pub enum PickerTreeElement {
    File(FileTreeElement),
    Outline(OutlineTreeElement),
}

/// `IWorkspace | URI`: a entrada da árvore de arquivos.
#[derive(Clone)]
pub enum FilePickerInput {
    Workspace(Rc<Workspace>),
    Uri(URI),
}

impl FilePickerInput {
    /// `FileIdentityProvider.getId` para a entrada.
    fn id(&self) -> String {
        match self {
            Self::Workspace(workspace) => workspace.id(),
            Self::Uri(uri) => uri_text(uri),
        }
    }
}

impl PartialEq for FilePickerInput {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for FilePickerInput {}

impl Hash for FilePickerInput {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

/// `IWorkspaceFolder | IFileStat`.
#[derive(Clone)]
pub enum FileTreeElement {
    Folder(WorkspaceFolder),
    Stat(FileStat),
}

impl FileTreeElement {
    /// `FileIdentityProvider.getId`.
    fn id(&self) -> String {
        match self {
            Self::Folder(folder) => uri_text(&folder.uri),
            Self::Stat(stat) => uri_text(&stat.resource),
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Folder(folder) => &folder.name,
            Self::Stat(stat) => &stat.name,
        }
    }

    /// `(element as IFileStat).isDirectory`: `None` é o `undefined` de uma pasta do workspace.
    fn is_directory(&self) -> Option<bool> {
        match self {
            Self::Folder(_) => None,
            Self::Stat(stat) => Some(stat.is_directory),
        }
    }
}

impl PartialEq for FileTreeElement {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for FileTreeElement {}

impl Hash for FileTreeElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

fn uri_text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

/// O `fakeEvent` (`new UIEvent('fakeEvent')`) que o seletor passa a `setFocus`: um evento que não é de teclado.
fn fake_event() -> BrowserEvent {
    BrowserEvent { modifiers: Default::default(), button: Default::default(), target: None, click_count: 0, is_keyboard: false, force_event: false }
}

/// Os serviços que os seletores recebem por injeção.
#[derive(Clone)]
pub struct BreadcrumbsPickerServices {
    pub ui: Ui,
    pub theme_service: Rc<dyn IThemeService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
    pub workspace_service: Rc<dyn IWorkspaceContextService>,
    pub editor_service: Rc<dyn IEditorService>,
    pub file_service: Rc<dyn IFileService>,
    pub resource_labels: Rc<ResourceLabelsServices>,
    /// `themeService.getFileIconTheme()`.
    pub file_icon_theme: Rc<dyn Fn() -> IFileIconTheme>,
    /// `themeService.onDidFileIconThemeChange`.
    pub on_did_file_icon_theme_change: Event<IFileIconTheme>,
}

/// O que a base lê de uma árvore (ver as notas do módulo sobre o `Tree<I, E>`).
pub struct PickerTree {
    on_did_open: Event<IOpenEvent<PickerTreeElement>>,
    on_did_change_focus: Event<Option<PickerTreeElement>>,
    on_did_change_content_height: Event<f64>,
    content_height: Box<dyn Fn() -> f64>,
    layout: Box<dyn Fn(f64, f64)>,
    dom_focus: Box<dyn Fn()>,
    dispose: Box<dyn Fn()>,
    /// As escutas e o navegador de recursos que valem enquanto a árvore.
    _keep_alive: Rc<DisposableStore>,
}

impl PickerTree {
    fn content_height(&self) -> f64 {
        (self.content_height)()
    }

    fn layout(&self, height: f64, width: f64) {
        (self.layout)(height, width);
    }

    fn dom_focus(&self) {
        (self.dom_focus)();
    }

    fn dispose(&self) {
        (self.dispose)();
        self._keep_alive.dispose();
    }
}

/// Os métodos abstratos de `BreadcrumbsPicker`.
pub trait IBreadcrumbsPickerKind {
    /// `_createTree(container, input)`.
    fn create_tree(&self, picker: &BreadcrumbsPicker, container: NodeId, input: &BreadcrumbsElement) -> PickerTree;

    /// `_setInput(element)`.
    fn set_input(&self, picker: &BreadcrumbsPicker, element: &BreadcrumbsElement) -> Promise<()>;

    /// `_previewElement(element)`.
    fn preview_element(&self, element: Option<&PickerTreeElement>) -> DisposableRef;

    /// `_revealElement(element, options, sideBySide)`.
    fn reveal_element(&self, picker: &BreadcrumbsPicker, element: &PickerTreeElement, options: EditorOptions, side_by_side: bool) -> Promise<bool>;

    /// O que `restoreViewState` faz além de limpar a prévia (o override do `BreadcrumbsOutlinePicker`).
    fn on_restore_view_state(&self) {}
}

/// `BreadcrumbsPicker`.
pub struct BreadcrumbsPicker {
    services: Rc<BreadcrumbsPickerServices>,
    kind: Rc<dyn IBreadcrumbsPickerKind>,
    disposables: Rc<DisposableStore>,
    dom_node: NodeId,
    arrow: Cell<Option<NodeId>>,
    arrow_color: Cell<Option<Rgba>>,
    tree_container: Cell<Option<NodeId>>,
    tree: RefCell<Option<Rc<PickerTree>>>,
    layout_info: Cell<Option<LayoutInfo>>,
    on_will_pick_element: Rc<Emitter<bool>>,
    preview_disposables: Rc<MutableDisposable<dyn IDisposable>>,
    resource: URI,
}

impl BreadcrumbsPicker {
    fn new(services: Rc<BreadcrumbsPickerServices>, parent: NodeId, resource: URI, kind: Rc<dyn IBreadcrumbsPickerKind>) -> Rc<Self> {
        let dom_node = create_el(&mut services.ui.tree_mut(), parent, Role::Generic, "monaco-breadcrumbs-picker show-file-icons");
        Rc::new(Self {
            services,
            kind,
            disposables: DisposableStore::new(),
            dom_node,
            arrow: Cell::new(None),
            arrow_color: Cell::new(None),
            tree_container: Cell::new(None),
            tree: RefCell::new(None),
            layout_info: Cell::new(None),
            on_will_pick_element: Emitter::new(),
            preview_disposables: MutableDisposable::new(),
            resource,
        })
    }

    /// O recurso da migalha a que o seletor pertence.
    pub fn resource(&self) -> &URI {
        &self.resource
    }

    /// O elemento raiz do seletor (`_domNode`).
    pub fn element(&self) -> NodeId {
        self.dom_node
    }

    /// A cor da seta (`borderColor: transparent transparent <cor>`), ver as notas do módulo.
    pub fn arrow_color(&self) -> Option<Rgba> {
        self.arrow_color.get()
    }

    /// `onWillPickElement`.
    pub fn on_will_pick_element(&self) -> Event<bool> {
        self.on_will_pick_element.event()
    }

    /// `dispose()`.
    pub fn dispose(&self) {
        self.disposables.dispose();
        self.preview_disposables.dispose();
        self.on_will_pick_element.dispose();
        self.services.ui.tree_mut().remove(self.dom_node);
        // tree cannot be disposed while being opened...
        if let Some(tree) = self.tree.borrow_mut().take() {
            disposable_timeout(move || tree.dispose(), 0, None);
        }
    }

    /// `show(input, maxHeight, width, arrowSize, arrowOffset)`.
    pub fn show(self: &Rc<Self>, input: BreadcrumbsElement, max_height: f64, width: f64, arrow_size: f64, arrow_offset: f64) -> Promise<()> {
        let theme = self.services.theme_service.get_color_theme();
        let color = theme.get_color(BREADCRUMBS_PICKER_BACKGROUND, None).map(|color| to_rgba8(&color));
        let border = theme.get_color(WIDGET_BORDER, None).map(|color| to_rgba8(&color));

        let tree_container = {
            let mut tree = self.services.ui.tree_mut();
            let arrow = create_el(&mut tree, self.dom_node, Role::Generic, "arrow");
            self.arrow.set(Some(arrow));
            self.arrow_color.set(color);

            let tree_container = create_el(&mut tree, self.dom_node, Role::Generic, "");
            let style = tree.style_mut(tree_container);
            style.background = color;
            style.radius = 3.0;
            style.border = border.map(|color| je_ui::Border { width: 1.0, color });
            tree_container
        };
        self.tree_container.set(Some(tree_container));

        self.layout_info.set(Some(LayoutInfo { max_height, width, arrow_size, arrow_offset }));
        let tree = Rc::new(self.kind.create_tree(self, tree_container, &input));
        *self.tree.borrow_mut() = Some(tree.clone());

        let this = Rc::downgrade(self);
        self.disposables.add(tree.on_did_open.listen({
            let this = this.clone();
            move |e: &IOpenEvent<PickerTreeElement>| {
                let Some(picker) = this.upgrade() else { return };
                let options = EditorOptions { preserve_focus: Some(false), ..e.editor_options.clone() };
                // `didReveal` não muda nada no original: a abertura já aconteceu (ou não) dentro de `_revealElement`.
                let _ = picker.kind.reveal_element(&picker, &e.element, options, e.side_by_side);
            }
        }));
        self.disposables.add(tree.on_did_change_focus.listen({
            let this = this.clone();
            move |element: &Option<PickerTreeElement>| {
                let Some(picker) = this.upgrade() else { return };
                picker.preview_disposables.set(Some(picker.kind.preview_element(element.as_ref())));
            }
        }));
        self.disposables.add(tree.on_did_change_content_height.listen({
            let this = this.clone();
            move |_| {
                if let Some(picker) = this.upgrade() {
                    picker.layout();
                }
            }
        }));

        // `this._domNode.focus()`: o foco do nó é da árvore do `je-ui`, que o seletor recebe do contexto que o abre.
        let picker = self.clone();
        self.kind.set_input(self, &input).then_result(move |result| {
            match result {
                Ok(()) => picker.layout(),
                Err(error) => on_unexpected_error(&error),
            }
            Ok(())
        })
    }

    /// `_layout()`.
    fn layout(&self) {
        let (Some(info), Some(arrow), Some(tree_container), Some(tree)) = (self.layout_info.get(), self.arrow.get(), self.tree_container.get(), self.tree.borrow().clone()) else {
            return;
        };

        let header_height = 2.0 * info.arrow_size;
        let tree_height = (info.max_height - header_height).min(tree.content_height());
        let total_height = tree_height + header_height;

        {
            let mut ui_tree = self.services.ui.tree_mut();
            let origin = ui_tree.bounds(self.dom_node);
            ui_tree.set_bounds(self.dom_node, Rect::new(origin.x0, origin.y0, origin.x0 + info.width, origin.y0 + total_height));
            ui_tree.set_bounds(arrow, Rect::new(info.arrow_offset, -2.0 * info.arrow_size, info.arrow_offset + 2.0 * info.arrow_size, 0.0));
            ui_tree.set_bounds(tree_container, Rect::new(0.0, 0.0, info.width, tree_height));
        }
        tree.layout(tree_height, info.width);
    }

    /// `restoreViewState()`.
    pub fn restore_view_state(&self) {
        self.preview_disposables.clear();
        self.kind.on_restore_view_state();
    }
}

/// A `Tree` como o [`PickerTree`], a partir dos pedaços tipados de cada árvore.
struct PickerTreeParts<E: Clone + 'static> {
    on_did_change_selection: Event<TreeEvent<E>>,
    on_pointer: Event<je_widgets::tree::tree_types::TreeMouseEvent<E>>,
    on_mouse_dbl_click: Event<je_widgets::tree::tree_types::TreeMouseEvent<E>>,
    on_did_change_focus: Event<TreeEvent<E>>,
    get_selected_element: Box<dyn Fn() -> Option<E>>,
}

impl<E: Clone + 'static> PickerTreeParts<E> {
    /// A árvore do `Workbench*DataTree`: o navegador de recursos sobre os eventos e o mapa dos elementos para o
    /// [`PickerTreeElement`] (`None` é a raiz sem elemento).
    #[allow(clippy::too_many_arguments)]
    fn into_picker_tree(
        self,
        configuration_service: Rc<dyn IConfigurationService>,
        to_element: impl Fn(&E) -> Option<PickerTreeElement> + Clone + 'static,
        on_did_change_content_height: Event<f64>,
        content_height: impl Fn() -> f64 + 'static,
        layout: impl Fn(f64, f64) + 'static,
        dom_focus: impl Fn() + 'static,
        dispose: impl Fn() + 'static,
    ) -> PickerTree {
        let keep_alive = DisposableStore::new();
        let navigator = ResourceNavigator::new(
            self.on_did_change_selection,
            self.on_pointer,
            self.on_mouse_dbl_click,
            self.get_selected_element,
            Some(IResourceNavigatorOptions { configuration_service: Some(configuration_service), open_on_single_click: None }),
        );
        keep_alive.add_rc(navigator.clone());

        let open_emitter = Emitter::new();
        let map_open = to_element.clone();
        keep_alive.add(navigator.on_did_open().listen({
            let open_emitter = open_emitter.clone();
            move |e: &IOpenEvent<E>| {
                if let Some(element) = map_open(&e.element) {
                    open_emitter.fire(IOpenEvent { editor_options: e.editor_options.clone(), side_by_side: e.side_by_side, element, browser_event: e.browser_event });
                }
            }
        }));
        keep_alive.add_rc(open_emitter.clone());

        let on_did_change_focus = self.on_did_change_focus.map(move |e: &TreeEvent<E>| e.elements.first().and_then(&to_element), Some(&keep_alive));

        PickerTree {
            on_did_open: open_emitter.event(),
            on_did_change_focus,
            on_did_change_content_height,
            content_height: Box::new(content_height),
            layout: Box::new(layout),
            dom_focus: Box::new(dom_focus),
            dispose: Box::new(dispose),
            _keep_alive: keep_alive,
        }
    }
}

//#region - Files

/// `FileVirtualDelegate`.
struct FileVirtualDelegate;

impl IListVirtualDelegate<FileTreeElement> for FileVirtualDelegate {
    fn get_height(&self, _element: &FileTreeElement) -> f64 {
        22.0
    }

    fn get_template_id(&self, _element: &FileTreeElement) -> String {
        "FileStat".to_owned()
    }
}

/// `FileIdentityProvider`.
struct FileIdentityProvider;

impl IIdentityProvider<FileTreeElement> for FileIdentityProvider {
    fn get_id(&self, element: &FileTreeElement) -> String {
        element.id()
    }
}

/// `FileDataSource`.
struct FileDataSource {
    file_service: Rc<dyn IFileService>,
}

impl IAsyncDataSource<FilePickerInput, FileTreeElement> for FileDataSource {
    fn has_children(&self, element: &InputOrElement<FilePickerInput, FileTreeElement>) -> bool {
        match element {
            InputOrElement::Input(_) | InputOrElement::Element(FileTreeElement::Folder(_)) => true,
            InputOrElement::Element(FileTreeElement::Stat(stat)) => stat.is_directory,
        }
    }

    fn get_children(&self, element: &InputOrElement<FilePickerInput, FileTreeElement>) -> MaybePromise<Vec<FileTreeElement>> {
        let uri = match element {
            InputOrElement::Input(FilePickerInput::Workspace(workspace)) => return MaybePromise::Value(workspace.folders().into_iter().map(FileTreeElement::Folder).collect()),
            InputOrElement::Input(FilePickerInput::Uri(uri)) => uri.clone(),
            InputOrElement::Element(FileTreeElement::Folder(folder)) => folder.uri.clone(),
            InputOrElement::Element(FileTreeElement::Stat(stat)) => stat.resource.clone(),
        };
        MaybePromise::Promise(self.file_service.resolve(&uri, None).then(|stat| stat.children.unwrap_or_default().into_iter().map(FileTreeElement::Stat).collect()))
    }
}

/// O `templateData` do `FileRenderer`: o `IResourceLabel`.
struct FileTemplate(Rc<dyn IResourceLabel>);

/// `FileRenderer`.
struct FileRenderer {
    labels: Rc<ResourceLabels>,
    config_service: Rc<dyn IConfigurationService>,
}

impl IAsyncDataTreeRenderer<FilePickerInput, FileTreeElement, FuzzyScore> for FileRenderer {
    fn template_id(&self) -> String {
        "FileStat".to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        Rc::new(FileTemplate(self.labels.create(container, Some(IIconLabelCreationOptions { support_highlights: true, ..IIconLabelCreationOptions::default() }))))
    }

    fn render_element(&self, node: &Rc<AsyncDataTreeNodeWrapper<FilePickerInput, FileTreeElement, FuzzyScore>>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let Some(template) = template_data.downcast_ref::<FileTemplate>() else { return };
        let file_decorations = match self.config_service.get_value(Some("explorer.decorations"), None) {
            decorations @ Dynamic::Object(_) => {
                let flag = |key: &str| matches!(decorations.get_str(key), Dynamic::Bool(true));
                Some(FileDecorations { colors: flag("colors"), badges: flag("badges") })
            }
            _ => None,
        };
        let InputOrElement::Element(element) = node.element() else { return };
        let (resource, file_kind) = match &element {
            FileTreeElement::Folder(folder) => (folder.uri.clone(), FileKind::RootFolder),
            FileTreeElement::Stat(stat) => (stat.resource.clone(), if stat.is_directory { FileKind::Folder } else { FileKind::File }),
        };
        template.0.set_file(
            &resource,
            Some(IFileLabelOptions {
                resource: IResourceLabelOptions {
                    icon_label: IIconLabelValueOptions { matches: Some(create_matches(node.filter_data().as_deref())), extra_classes: vec!["picker-item".to_owned()], ..IIconLabelValueOptions::default() },
                    file_kind: Some(file_kind),
                    file_decorations,
                    ..IResourceLabelOptions::default()
                },
                hide_path: true,
                ..IFileLabelOptions::default()
            }),
        );
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        if let Some(template) = template_data.downcast_ref::<FileTemplate>() {
            template.0.dispose();
        }
    }
}

/// `FileNavigationLabelProvider`.
struct FileNavigationLabelProvider;

impl IKeyboardNavigationLabelProvider<FileTreeElement> for FileNavigationLabelProvider {
    fn get_keyboard_navigation_label(&self, element: &FileTreeElement) -> Vec<Option<String>> {
        vec![Some(element.name().to_owned())]
    }
}

/// O `update` de `FileFilter`: reconstrói as expressões de exclusão de cada pasta do workspace.
fn update_file_excludes(
    workspace_service: &dyn IWorkspaceContextService,
    config: &BreadcrumbsConfig<Dynamic>,
    file_service: &dyn IFileService,
    cached_expressions: &RefCell<HashMap<String, ParsedExpression>>,
) {
    let posix = PathModule::posix(&ProcessEnvironment);
    for folder in workspace_service.get_workspace().folders() {
        let overrides = ConfigurationOverrides { resource: Some(folder.uri.clone()), ..ConfigurationOverrides::default() };
        let Some(excludes_config @ Dynamic::Object(_)) = config.get_value(Some(&overrides)) else {
            continue;
        };
        // adjust patterns to be absolute in case they aren't
        // free floating (**/)
        let mut adjusted_config = Expression::new();
        for (pattern, value) in excludes_config.entries() {
            let Dynamic::Bool(flag) = value else { continue };
            let pattern = pattern.to_string_lossy();
            let folder_path = folder.uri.path().to_string_lossy();
            let pattern_abs = if !pattern.starts_with("**/") { posix.join(&[folder_path.as_str(), pattern.as_str()]) } else { pattern };
            adjusted_config.insert(pattern_abs, flag);
        }
        let ignore_case = !file_service.has_capability(&folder.uri, FileSystemProviderCapabilities::PATH_CASE_SENSITIVE);
        if let ParsedGlob::Expression(expression) = glob::parse(Glob::Expression(&adjusted_config), &GlobOptions { ignore_case: Some(ignore_case), ..GlobOptions::default() }) {
            cached_expressions.borrow_mut().insert(uri_text(&folder.uri), expression);
        }
    }
}

/// `FileFilter`.
struct FileFilter {
    workspace_service: Rc<dyn IWorkspaceContextService>,
    cached_expressions: Rc<RefCell<HashMap<String, ParsedExpression>>>,
    disposables: Rc<DisposableStore>,
}

impl FileFilter {
    fn new(workspace_service: Rc<dyn IWorkspaceContextService>, config_service: Rc<dyn IConfigurationService>, file_service: Rc<dyn IFileService>) -> Rc<Self> {
        let config = BreadcrumbsConfigKeys::FILE_EXCLUDES.bind_to(config_service);
        let cached_expressions: Rc<RefCell<HashMap<String, ParsedExpression>>> = Rc::default();
        let update = {
            let (workspace_service, config, cached_expressions) = (workspace_service.clone(), config.clone(), cached_expressions.clone());
            Rc::new(move || update_file_excludes(workspace_service.as_ref(), &config, file_service.as_ref(), &cached_expressions))
        };
        update();

        let disposables = DisposableStore::new();
        disposables.add(config.clone());
        let on_change = update.clone();
        disposables.add(config.on_did_change().listen(move |_| on_change()));
        let on_change = update;
        disposables.add(workspace_service.on_did_change_workspace_folders().listen(move |_| on_change()));
        Rc::new(Self { workspace_service, cached_expressions, disposables })
    }
}

impl IDisposable for FileFilter {
    fn dispose(&self) {
        self.disposables.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "FileFilter"
    }
}

impl ITreeFilter<FileTreeElement, FuzzyScore> for FileFilter {
    fn filter(&self, element: &FileTreeElement, _parent_visibility: TreeVisibility) -> TreeFilterResult<FuzzyScore> {
        let FileTreeElement::Stat(stat) = element else {
            // not a file
            return TreeFilterResult::Bool(true);
        };
        let folder = self.workspace_service.get_workspace_folder(&stat.resource);
        let cached = self.cached_expressions.borrow();
        let Some((folder, expression)) = folder.and_then(|folder| cached.get(&uri_text(&folder.uri)).map(|expression| (folder, expression))) else {
            // no folder or no filer
            return TreeFilterResult::Bool(true);
        };

        let posix = PathModule::posix(&ProcessEnvironment);
        let relative = posix.relative(&folder.uri.path().to_string_lossy(), &stat.resource.path().to_string_lossy());
        let basename = ext_uri().basename(&stat.resource, None);
        TreeFilterResult::Bool(expression.matches(&relative, Some(basename.as_str()), None).matched().is_none())
    }
}

/// `FileSorter`.
pub struct FileSorter;

impl ITreeSorter<FileTreeElement> for FileSorter {
    fn compare(&self, a: &FileTreeElement, b: &FileTreeElement) -> i32 {
        if let (FileTreeElement::Folder(a), FileTreeElement::Folder(b)) = (a, b) {
            return a.index as i32 - b.index as i32;
        }
        if a.is_directory() == b.is_directory() {
            // same type -> compare on names
            let (name_a, name_b): (Vec<u16>, Vec<u16>) = (a.name().encode_utf16().collect(), b.name().encode_utf16().collect());
            compare_file_names(Some(&name_a), Some(&name_b))
        } else if a.is_directory() == Some(true) {
            -1
        } else {
            1
        }
    }
}

/// `AsyncDataTree` de arquivos.
type FileAsyncTree = AsyncDataTree<FilePickerInput, FileTreeElement, FuzzyScore, ObjectBackend>;

/// O que `BreadcrumbsFilePicker` implementa dos métodos abstratos.
struct FilePickerKind {
    services: Rc<BreadcrumbsPickerServices>,
    tree: RefCell<Option<Rc<FileAsyncTree>>>,
}

impl FilePickerKind {
    fn tree(&self) -> Rc<FileAsyncTree> {
        self.tree.borrow().clone().expect("a árvore é criada em `show`, antes de `_setInput`")
    }
}

impl IBreadcrumbsPickerKind for FilePickerKind {
    fn create_tree(&self, picker: &BreadcrumbsPicker, container: NodeId, _input: &BreadcrumbsElement) -> PickerTree {
        let services = &self.services;
        let tree_container = picker.tree_container.get().expect("o contêiner da árvore é criado antes de `_createTree`");

        // tree icon theme specials
        {
            let mut ui_tree = services.ui.tree_mut();
            ui_tree.add_class(tree_container, "file-icon-themable-tree");
            ui_tree.add_class(tree_container, "show-file-icons");
        }
        let apply_file_icon_theme = {
            let ui = services.ui.clone();
            move |file_icon_theme: &IFileIconTheme| {
                let mut ui_tree = ui.tree_mut();
                ui_tree.toggle_class(tree_container, "align-icons-and-twisties", file_icon_theme.has_file_icons && !file_icon_theme.has_folder_icons);
                ui_tree.toggle_class(tree_container, "hide-arrows", file_icon_theme.hides_explorer_arrows);
            }
        };
        picker.disposables.add(services.on_did_file_icon_theme_change.listen({
            let apply = apply_file_icon_theme.clone();
            move |file_icon_theme| apply(file_icon_theme)
        }));
        apply_file_icon_theme(&(services.file_icon_theme)());

        let labels = ResourceLabels::new(default_labels_container() /* TODO@Jo visibility propagation */, services.resource_labels.clone());
        picker.disposables.add_rc(labels.clone());

        let filter = FileFilter::new(services.workspace_service.clone(), services.configuration_service.clone(), services.file_service.clone());
        picker.disposables.add_rc(filter.clone());

        let mut options = AsyncDataTreeOptions::<FileTreeElement, FuzzyScore>::default();
        options.update.multiple_selection_support = Some(false);
        options.update.show_not_found_message = Some(false);
        options.sorter = Some(Rc::new(FileSorter));
        options.filter = Some(filter);
        options.identity_provider = Some(Rc::new(FileIdentityProvider));
        options.keyboard_navigation_label_provider = Some(Rc::new(FileNavigationLabelProvider));

        let tree = AsyncDataTree::new(
            "BreadcrumbsFilePicker",
            &services.ui,
            container,
            Rc::new(FileVirtualDelegate),
            vec![Rc::new(FileRenderer { labels, config_service: services.configuration_service.clone() })],
            Rc::new(FileDataSource { file_service: services.file_service.clone() }),
            options,
        );
        let background = services.theme_service.get_color_theme().get_color(BREADCRUMBS_PICKER_BACKGROUND, None).map(|color| to_rgba8(&color));
        tree.style(&IListStyles { list_background: background, ..IListStyles::default() });
        *self.tree.borrow_mut() = Some(tree.clone());

        let parts = PickerTreeParts {
            on_did_change_selection: tree.on_did_change_selection(),
            on_pointer: tree.on_pointer(),
            on_mouse_dbl_click: tree.on_mouse_dbl_click(),
            on_did_change_focus: tree.on_did_change_focus(),
            get_selected_element: Box::new({
                let tree = tree.clone();
                move || tree.get_selection().first().cloned()
            }),
        };
        parts.into_picker_tree(
            services.configuration_service.clone(),
            |element: &FileTreeElement| Some(PickerTreeElement::File(element.clone())),
            tree.on_did_change_content_height(),
            {
                let tree = tree.clone();
                move || tree.content_height()
            },
            {
                let tree = tree.clone();
                move |height, width| tree.layout(Some(height), Some(width))
            },
            {
                let tree = tree.clone();
                move || tree.dom_focus()
            },
            move || tree.dispose(),
        )
    }

    fn set_input(&self, _picker: &BreadcrumbsPicker, element: &BreadcrumbsElement) -> Promise<()> {
        let BreadcrumbsElement::File(FileElement { uri, kind, .. }) = element else {
            return Promise::resolved(());
        };
        let input = if *kind == FileKind::RootFolder { FilePickerInput::Workspace(self.services.workspace_service.get_workspace()) } else { FilePickerInput::Uri(ext_uri().dirname(uri)) };

        let tree = self.tree();
        let uri = uri.clone();
        tree.set_input(input, None).then({
            let tree = tree.clone();
            move |()| {
                let mut focus_element = None;
                for child in tree.get_node(None).children() {
                    let InputOrElement::Element(element) = child.element() else { continue };
                    let matches = match &element {
                        FileTreeElement::Folder(folder) => ext_uri().is_equal(Some(&folder.uri), Some(&uri), false),
                        FileTreeElement::Stat(stat) => ext_uri().is_equal(Some(&stat.resource), Some(&uri), false),
                    };
                    if matches {
                        focus_element = Some(element);
                        break;
                    }
                }
                if let Some(focus_element) = focus_element {
                    tree.reveal(&focus_element, Some(0.5));
                    tree.set_focus(&[focus_element], Some(fake_event()));
                }
                tree.dom_focus();
            }
        })
    }

    fn preview_element(&self, _element: Option<&PickerTreeElement>) -> DisposableRef {
        none()
    }

    fn reveal_element(&self, picker: &BreadcrumbsPicker, element: &PickerTreeElement, options: EditorOptions, side_by_side: bool) -> Promise<bool> {
        let PickerTreeElement::File(FileTreeElement::Stat(stat)) = element else {
            return Promise::resolved(false);
        };
        if !stat.is_file {
            return Promise::resolved(false);
        }

        picker.on_will_pick_element.fire(side_by_side);
        let input = crate::common::editor::UntypedEditorInput { resource: Some(stat.resource.clone()), options: Some(options.into()), ..Default::default() };
        self.services.editor_service.open_editor_in(input, side_by_side.then_some(PreferredGroup::Side)).then(|_| true)
    }
}

/// `BreadcrumbsFilePicker`.
pub struct BreadcrumbsFilePicker;

impl BreadcrumbsFilePicker {
    pub fn new(services: Rc<BreadcrumbsPickerServices>, parent: NodeId, resource: URI) -> Rc<BreadcrumbsPicker> {
        BreadcrumbsPicker::new(services.clone(), parent, resource, Rc::new(FilePickerKind { services, tree: RefCell::new(None) }))
    }
}

//#endregion

//#region - Outline

/// `OutlineTreeSorter`.
struct OutlineTreeSorter {
    comparator: Rc<dyn IOutlineComparator>,
    order: SymbolSortOrder,
}

impl OutlineTreeSorter {
    fn new(comparator: Rc<dyn IOutlineComparator>, uri: Option<&URI>, config_service: &dyn ITextResourceConfigurationService) -> Self {
        let order = <SymbolSortOrder as super::breadcrumbs::ConfigValue>::from_dynamic(&config_service.get_value(uri, Some("breadcrumbs.symbolSortOrder"))).unwrap_or(SymbolSortOrder::Position);
        Self { comparator, order }
    }
}

impl ITreeSorter<OutlineTreeElement> for OutlineTreeSorter {
    fn compare(&self, a: &OutlineTreeElement, b: &OutlineTreeElement) -> i32 {
        let result = match self.order {
            SymbolSortOrder::Name => self.comparator.compare_by_name(&a.0, &b.0),
            SymbolSortOrder::Type => self.comparator.compare_by_type(&a.0, &b.0),
            SymbolSortOrder::Position => self.comparator.compare_by_position(&a.0, &b.0),
        };
        if result < 0.0 {
            -1
        } else if result > 0.0 {
            1
        } else {
            0
        }
    }
}

type OutlineDataTree = DataTree<Rc<dyn IOutline>, OutlineTreeElement, FuzzyScore>;

/// O que `BreadcrumbsOutlinePicker` implementa dos métodos abstratos.
struct OutlinePickerKind {
    services: Rc<BreadcrumbsPickerServices>,
    tree: RefCell<Option<Rc<OutlineDataTree>>>,
    view_state: RefCell<Option<DisposableRef>>,
}

impl OutlinePickerKind {
    fn tree(&self) -> Rc<OutlineDataTree> {
        self.tree.borrow().clone().expect("a árvore é criada em `show`, antes de `_setInput`")
    }
}

impl IBreadcrumbsPickerKind for OutlinePickerKind {
    fn create_tree(&self, _picker: &BreadcrumbsPicker, container: NodeId, input: &BreadcrumbsElement) -> PickerTree {
        let services = &self.services;
        let BreadcrumbsElement::Outline(OutlineElement2 { outline, .. }) = input else {
            unreachable!("o seletor do outline só abre elementos do outline");
        };
        let config = outline.config().clone();

        let mut options = (config.options)();
        options.sorter = Some(Rc::new(OutlineTreeSorter::new(config.comparator.clone(), None, services.text_resource_configuration_service.as_ref())));
        options.tree.collapse_by_default = true;
        options.tree.update.expand_only_on_twistie_click = Some(ExpandOnlyOnTwistieClick::Always(true));
        options.tree.update.multiple_selection_support = Some(false);
        options.tree.update.show_not_found_message = Some(false);

        let tree = DataTree::new("BreadcrumbsOutlinePicker", &services.ui, container, config.delegate.clone(), config.renderers.clone(), config.tree_data_source.clone(), options);
        *self.tree.borrow_mut() = Some(tree.clone());

        let parts = PickerTreeParts {
            on_did_change_selection: tree.on_did_change_selection(),
            on_pointer: tree.on_pointer(),
            on_mouse_dbl_click: tree.on_mouse_dbl_click(),
            on_did_change_focus: tree.on_did_change_focus(),
            get_selected_element: Box::new({
                let tree = tree.clone();
                move || tree.get_selection().first().cloned()
            }),
        };
        // Os elementos da `DataTree` são `Option<T>`: `None` é a raiz, sem elemento para abrir ou pré-visualizar.
        parts.into_picker_tree(
            services.configuration_service.clone(),
            |element: &Option<OutlineTreeElement>| element.clone().map(PickerTreeElement::Outline),
            tree.on_did_change_content_height(),
            {
                let tree = tree.clone();
                move || tree.content_height()
            },
            {
                let tree = tree.clone();
                move |height, width| tree.layout(Some(height), Some(width))
            },
            {
                let tree = tree.clone();
                move || tree.dom_focus()
            },
            move || tree.dispose(),
        )
    }

    fn set_input(&self, _picker: &BreadcrumbsPicker, input: &BreadcrumbsElement) -> Promise<()> {
        let BreadcrumbsElement::Outline(OutlineElement2 { outline, element }) = input else {
            return Promise::resolved(());
        };

        *self.view_state.borrow_mut() = Some(outline.capture_view_state());

        let tree = self.tree();

        tree.set_input(Some(outline.clone()), None);
        if let OutlineElementValue::Element(element) = element {
            let element = Some(OutlineTreeElement(element.clone()));
            tree.reveal(&element, Some(0.5));
            tree.set_focus(&[element], Some(fake_event()));
        }
        tree.dom_focus();

        Promise::resolved(())
    }

    fn preview_element(&self, element: Option<&PickerTreeElement>) -> DisposableRef {
        let Some(PickerTreeElement::Outline(element)) = element else {
            return none();
        };
        match self.tree().get_input() {
            Some(outline) => outline.preview(&element.0),
            None => none(),
        }
    }

    fn reveal_element(&self, picker: &BreadcrumbsPicker, element: &PickerTreeElement, options: EditorOptions, side_by_side: bool) -> Promise<bool> {
        let PickerTreeElement::Outline(element) = element else {
            return Promise::resolved(false);
        };
        picker.on_will_pick_element.fire(side_by_side);
        let Some(outline) = self.tree().get_input() else {
            return Promise::resolved(false);
        };
        match outline.reveal(&element.0, &options, side_by_side, false) {
            MaybePromise::Value(()) => Promise::resolved(true),
            MaybePromise::Promise(promise) => promise.then(|()| true),
        }
    }

    fn on_restore_view_state(&self) {
        if let Some(view_state) = self.view_state.borrow().as_ref() {
            view_state.dispose();
        }
    }
}

/// `BreadcrumbsOutlinePicker`.
pub struct BreadcrumbsOutlinePicker;

impl BreadcrumbsOutlinePicker {
    pub fn new(services: Rc<BreadcrumbsPickerServices>, parent: NodeId, resource: URI) -> Rc<BreadcrumbsPicker> {
        BreadcrumbsPicker::new(services.clone(), parent, resource, Rc::new(OutlinePickerKind { services, tree: RefCell::new(None), view_state: RefCell::new(None) }))
    }
}

//#endregion

/// `localize('breadcrumbs', "Breadcrumbs")`: o rótulo de acessibilidade da árvore de arquivos.
pub fn file_picker_widget_aria_label() -> String {
    localize("breadcrumbs", "Breadcrumbs", &[])
}
