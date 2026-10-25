//! Porte de `src/vs/workbench/contrib/files/browser/views/explorerViewer.ts`, fatia 2b (parte do renderer): o
//! `FilesRenderer` com o rótulo do item, a edição em linha do nome (`renderInputBox` sobre o
//! [`InlineEditInputBox`]) e o `IListAccessibilityProvider`.
//!
//! Divergências:
//! - **DOM.** O `templateData.container` é o [`NodeId`] do contêiner da linha; `display: flex|none` do rótulo é
//!   `hide`/`show` de `je-widgets::dom`; a variável CSS `--vscode-explorer-align-offset-margin-left` e o `force-twistie`
//!   no twistie da árvore são apresentação do widget da árvore e não existem aqui (a classe `align-nest-icon-with-parent-icon`
//!   segue como classe extra do rótulo).
//! - **`FuzzyScore`.** O filtro da fatia 2a usa `()` como dado de filtro (o `FilesFilter` não calcula pontuação), então
//!   `matches` do rótulo fica vazio, como em [`super::explorer_viewer_data`].
//! - **`IExplorerFileContribution`.** O `explorerFileContribRegistry` (contribuições por linha) ainda não foi portado:
//!   `contribs` não existe no `templateData`.
//! - **Tema de ícones.** `themeService.getFileIconTheme()` entra como o provedor [`FileIconThemeInfo`], porque o
//!   `IThemeService` do porte ainda não expõe o tema de ícones de arquivo.
//! - **`scheduleAtNextAnimationFrame`.** É o `setTimeout(0)` do `runtime`: não há quadro de animação de documento.
//! - **Distintivo de destaque.** O `CountBadge` vai no próprio elemento do rótulo (o `lastElementChild` do original é o
//!   contêiner interno do `IconLabel`) e usa os estilos de [`FilesRendererServices::badge_styles`].
//! - **`ILabelService.getSeparator`.** O separador sai de [`LabelSeparator`].

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::Truthy;
use je_base::errors::{self, Error};
use je_base::event::{Event, EventMultiplexer};
use je_base::lifecycle::{ById, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::runtime;
use je_base::key_codes::KeyCode;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextview::context_view::IContextMenuService;
use je_platform::files::files::FileKind;
use je_platform::label::label::ILabelService;
use je_platform::resources::{ext_uri, join_path};
use je_platform::workspace::{IWorkspaceContextService, WorkbenchState};
use je_ui::{NodeId, Ui};
use je_widgets::context_view_api::IContextViewProvider;
use je_widgets::count_badge::{CountBadge, ICountBadgeOptions, ICountBadgeStyles};
use je_widgets::dom::{hide, show};
use je_widgets::icon_label::{IIconLabelValueOptions, IconLabelText};
use je_widgets::input_box::{InputBoxStyles, Range};
use je_widgets::tree::compressed_object_tree_model::{CompressedRef, CompressibleObjectTreeModel};
use je_widgets::tree::compressible_object_tree::ICompressibleTreeRenderer;
use je_widgets::tree::index_tree_model::TreeNode;
use je_widgets::tree::tree_types::{ITreeModel, ITreeRenderer, TreeElementRenderDetails, TreeNodeApi};
use je_widgets::list::list_types::TemplateData;

use super::explorer_viewer::{CompressedNavigationController, ICompressedNavigationController, IExplorerFindHighlightTree};
use crate::browser::labels::{FileDecorations, IFileLabelOptions, IResourceLabel, IResourceLabelOptions, IResourceLabelProps, ResourceLabelResource, ResourceLabels};
use crate::browser::parts::views::inline_edit_input_box::{InlineEditInputBox, InlineEditInputBoxOptions};
use crate::contrib::files::browser_files::{EditableData, IExplorerService};
use crate::contrib::files::explorer_model::ExplorerItem;

type Model = CompressibleObjectTreeModel<Rc<ExplorerItem>>;
type CompressedNode = TreeNode<Option<CompressedRef<Rc<ExplorerItem>>>>;

/// `FilesRenderer.ID`.
pub const FILES_RENDERER_TEMPLATE_ID: &str = "file";

/// O que o renderer lê de `themeService.getFileIconTheme()`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileIconThemeInfo {
    pub has_file_icons: bool,
    pub has_folder_icons: bool,
    pub hides_explorer_arrows: bool,
}

impl FileIconThemeInfo {
    /// `theme.hasFileIcons && (theme.hidesExplorerArrows || !theme.hasFolderIcons)`: o tema não combina com o aninhamento
    /// e o ícone do filho aninhado precisa ser realinhado com o do pai.
    fn is_unhappy_with_nesting(self) -> bool {
        self.has_file_icons && (self.hides_explorer_arrows || !self.has_folder_icons)
    }
}

/// O que a edição em linha precisa do `IContextViewService`, do `IContextMenuService` e do tema.
#[derive(Clone)]
pub struct InlineEditServices {
    pub context_view_provider: Option<Rc<dyn IContextViewProvider>>,
    pub context_menu_service: Rc<dyn IContextMenuService>,
    /// `defaultInputBoxStyles`, lido a cada edição (o tema pode mudar).
    pub input_box_styles: Rc<dyn Fn() -> InputBoxStyles>,
}

/// Os argumentos e os serviços injetados do `FilesRenderer`.
#[derive(Clone)]
pub struct FilesRendererServices {
    pub ui: Ui,
    pub labels: Rc<ResourceLabels>,
    pub highlight_tree: Rc<dyn IExplorerFindHighlightTree>,
    /// `updateWidth`.
    pub update_width: Rc<dyn Fn(&Rc<ExplorerItem>)>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub explorer_service: Rc<dyn IExplorerService>,
    pub label_service: Rc<dyn ILabelService>,
    pub context_service: Rc<dyn IWorkspaceContextService>,
    pub file_icon_theme: Rc<dyn Fn() -> FileIconThemeInfo>,
    pub inline_edit: InlineEditServices,
    /// `{ ...defaultCountBadgeStyles, badgeBackground: listFilterMatchHighlight, badgeBorder: listFilterMatchHighlightBorder }`.
    pub badge_styles: ICountBadgeStyles,
}

/// `IFileTemplateData`.
pub struct FileTemplateData {
    pub template_disposables: Rc<DisposableStore>,
    pub element_disposables: Rc<DisposableStore>,
    pub label: Rc<dyn IResourceLabel>,
    pub container: NodeId,
    /// `currentContext`, compartilhado com o ouvinte de `label.onDidRender`.
    pub current_context: Rc<RefCell<Option<Rc<ExplorerItem>>>>,
}

/// `FilesRenderer`.
pub struct FilesRenderer {
    services: FilesRendererServices,
    /// `config.explorer.decorations`.
    decorations: Rc<RefCell<FileDecorations>>,
    config_listener: DisposableRef,
    compressed_navigation_controllers: Rc<RefCell<HashMap<ById<ExplorerItem>, Vec<Rc<CompressedNavigationController>>>>>,
    on_did_change_active_descendant: Rc<EventMultiplexer<()>>,
}

/// `explorer.decorations` da configuração.
fn read_decorations(configuration_service: &dyn IConfigurationService) -> FileDecorations {
    let decorations = configuration_service.get_value(None, None).get_str("explorer").get_str("decorations");
    FileDecorations { colors: decorations.get_str("colors").is_truthy(), badges: decorations.get_str("badges").is_truthy() }
}

fn file_kind_of(stat: &ExplorerItem) -> FileKind {
    if stat.is_root() {
        FileKind::RootFolder
    } else if stat.is_directory() {
        FileKind::Folder
    } else {
        FileKind::File
    }
}

/// `String.prototype.lastIndexOf('.')` em unidades UTF-16 (`-1` quando não há ponto).
fn last_dot_index(value: &str) -> i64 {
    let units: Vec<u16> = value.encode_utf16().collect();
    units.iter().rposition(|unit| *unit == u16::from(b'.')).map_or(-1, |index| index as i64)
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

impl FilesRenderer {
    /// `constructor(container, labels, highlightTree, updateWidth, ...)`.
    pub fn new(services: FilesRendererServices) -> Rc<Self> {
        let decorations = Rc::new(RefCell::new(read_decorations(services.configuration_service.as_ref())));
        let config_listener = services.configuration_service.on_did_change_configuration().listen({
            let (decorations, configuration_service) = (decorations.clone(), services.configuration_service.clone());
            move |event| {
                if event.affects_configuration("explorer", None) {
                    *decorations.borrow_mut() = read_decorations(configuration_service.as_ref());
                }
            }
        });

        Rc::new(Self {
            services,
            decorations,
            config_listener,
            compressed_navigation_controllers: Rc::new(RefCell::new(HashMap::new())),
            on_did_change_active_descendant: EventMultiplexer::new(),
        })
    }

    /// `onDidChangeActiveDescendant`.
    pub fn on_did_change_active_descendant(&self) -> Event<()> {
        self.on_did_change_active_descendant.event()
    }

    /// `getWidgetAriaLabel`.
    pub fn get_widget_aria_label(&self) -> String {
        localize("treeAriaLabel", "Files Explorer", &[])
    }

    /// `getCompressedNavigationController`.
    pub fn get_compressed_navigation_controller(&self, stat: &Rc<ExplorerItem>) -> Option<Vec<Rc<CompressedNavigationController>>> {
        self.compressed_navigation_controllers.borrow().get(&ById(stat.clone())).cloned()
    }

    // IAccessibilityProvider

    /// `getAriaLabel`.
    pub fn get_aria_label(&self, element: &Rc<ExplorerItem>) -> String {
        element.name()
    }

    /// `getAriaLevel`: We need to compute aria level on our own since children of compact folders will otherwise have an
    /// incorrect level (#107235).
    pub fn get_aria_level(&self, element: &Rc<ExplorerItem>) -> usize {
        let mut depth = 0;
        let mut parent = element.parent();
        while let Some(current) = parent {
            parent = current.parent();
            depth += 1;
        }

        if self.services.context_service.get_workbench_state() == WorkbenchState::Workspace {
            depth += 1;
        }

        depth
    }

    /// `getActiveDescendantId`.
    pub fn get_active_descendant_id(&self, stat: &Rc<ExplorerItem>) -> Option<String> {
        self.compressed_navigation_controllers.borrow().get(&ById(stat.clone())).and_then(|controllers| controllers.first()).map(|controller| controller.current_id())
    }

    fn template<'a>(template_data: &'a TemplateData) -> &'a FileTemplateData {
        template_data.downcast_ref::<FileTemplateData>().expect("o template do FilesRenderer")
    }

    fn render_stat(&self, stat: &Rc<ExplorerItem>, label: IconLabelText, dom_id: Option<String>, template_data: &FileTemplateData) {
        show(&mut self.services.ui.tree_mut(), template_data.label.element());
        let mut extra_classes = vec!["explorer-item".to_owned()];
        if self.services.explorer_service.is_cut(stat) {
            extra_classes.push("cut".to_owned());
        }

        // Offset nested children unless folders have both chevrons and icons, otherwise alignment breaks
        let realign_nested_children = stat.nested_parent().is_some() && (self.services.file_icon_theme)().is_unhappy_with_nesting();
        if realign_nested_children {
            extra_classes.push("align-nest-icon-with-parent-icon".to_owned());
        }

        let resource = stat.resource();
        let separator = self.services.label_service.get_separator(&resource.scheme().to_string_lossy(), Some(resource.authority().to_string_lossy().as_str())).as_str();
        template_data.label.set_resource(
            IResourceLabelProps { resource: Some(ResourceLabelResource::Uri(resource)), name: Some(label), ..IResourceLabelProps::default() },
            Some(IResourceLabelOptions {
                file_kind: Some(file_kind_of(stat)),
                file_decorations: Some(*self.decorations.borrow()),
                icon_label: IIconLabelValueOptions { extra_classes, matches: None, separator: Some(separator.to_owned()), dom_id, ..IIconLabelValueOptions::default() },
                ..IResourceLabelOptions::default()
            }),
        );

        let highlight_results = if stat.is_directory() { self.services.highlight_tree.get(stat) } else { 0 };
        let label_element = template_data.label.element();
        if highlight_results > 0 {
            let badge = CountBadge::new(&self.services.ui, label_element, &ICountBadgeOptions::default(), self.services.badge_styles);
            badge.set_count(highlight_results as i64);
            badge.set_title_format(&localize("explorerHighlightFolderBadgeTitle", "Directory contains {0} matches", &[&highlight_results.to_string()]));
            template_data.element_disposables.add(badge);
        }
        self.services.ui.tree_mut().toggle_class(label_element, "highlight-badge", highlight_results > 0);
    }

    /// `renderInputBox`.
    fn render_input_box(&self, container: NodeId, stat: &Rc<ExplorerItem>, editable_data: EditableData) -> Rc<DisposableStore> {
        let disposables = DisposableStore::new();
        let ui = self.services.ui.clone();

        // Use a file label only for the icon next to the input box
        let label = self.services.labels.create(container, None);
        disposables.add(Rc::new(label.clone()));
        disposables.add(to_disposable({
            let (ui, element) = (ui.clone(), label.element());
            move || ui.tree_mut().remove(element)
        }));

        let mut extra_classes = vec!["explorer-item".to_owned(), "explorer-item-edited".to_owned()];
        if stat.nested_parent().is_some() && (self.services.file_icon_theme)().is_unhappy_with_nesting() {
            extra_classes.push("align-nest-icon-with-parent-icon".to_owned());
        }
        let label_options = IFileLabelOptions {
            hide_path: true,
            hide_label: true,
            resource: IResourceLabelOptions {
                file_kind: Some(file_kind_of(stat)),
                icon_label: IIconLabelValueOptions { extra_classes, ..IIconLabelValueOptions::default() },
                ..IResourceLabelOptions::default()
            },
            ..IFileLabelOptions::default()
        };

        let name = stat.name();
        let stat_resource = stat.resource();
        let parent = if name.is_empty() { stat_resource } else { ext_uri().dirname(&stat_resource) };
        let value = name;

        // Use icon for ' ' if name is empty.
        let file_for = {
            let parent = parent.clone();
            move |value: &str| join_path(&parent, &[if value.is_empty() { " " } else { value }])
        };
        label.set_file(&file_for(&value), Some(label_options.clone()));

        // hack: hide label
        {
            let mut tree = ui.tree_mut();
            if let Some(first) = tree.children(label.element()).first().copied() {
                hide(&mut tree, first);
            }
        }

        // Input field for name
        let last_dot = last_dot_index(&value);
        let is_directory = stat.is_directory();
        let on_finish = editable_data.on_finish.clone();
        let finish_disposables = disposables.clone();
        let editable = EditableData {
            on_finish: Rc::new(move |value, success| {
                // Remove the file icon together with the input box
                finish_disposables.dispose();
                on_finish(value, success)
            }),
            ..editable_data
        };
        let selection_end = if last_dot > 0 && !is_directory { last_dot as usize } else { utf16_len(&value) };
        let inline_edit = InlineEditInputBox::new(
            &ui,
            label.element(),
            self.services.inline_edit.context_view_provider.clone(),
            self.services.inline_edit.context_menu_service.clone(),
            (self.services.inline_edit.input_box_styles)(),
            editable,
            InlineEditInputBoxOptions {
                aria_label: localize("fileInputAriaLabel", "Type file name. Press Enter to confirm or Escape to cancel.", &[]),
                value: Some(value),
                selection: Some(Range { start: 0, end: selection_end }),
            },
        );
        disposables.add(inline_edit.clone());
        let input_box = inline_edit.input_box().clone();

        let current_selection_state = Rc::new(RefCell::new("prefix"));

        disposables.add(input_box.on_did_change().listen({
            let (label, label_options, file_for) = (label.clone(), label_options, file_for);
            move |value| label.set_file(&file_for(value), Some(label_options.clone())) // update label icon while typing!
        }));
        disposables.add(input_box.on_key_down().listen({
            let input_box = input_box.clone();
            move |event| {
                if !event.equals(KeyCode::F2.bits()) {
                    return;
                }

                let value = input_box.value();
                let dot_index = last_dot_index(&value);
                if is_directory || dot_index == -1 {
                    return;
                }

                let mut state = current_selection_state.borrow_mut();
                match *state {
                    "prefix" => {
                        *state = "all";
                        input_box.select(Some(Range { start: 0, end: utf16_len(&value) }));
                    }
                    "all" => {
                        *state = "suffix";
                        input_box.select(Some(Range { start: dot_index as usize + 1, end: utf16_len(&value) }));
                    }
                    _ => {
                        *state = "prefix";
                        input_box.select(Some(Range { start: 0, end: dot_index as usize }));
                    }
                }
            }
        }));

        disposables
    }

}

impl IDisposable for FilesRenderer {
    fn dispose(&self) {
        self.config_listener.dispose();
    }
}

impl ITreeRenderer<Model> for FilesRenderer {
    fn template_id(&self) -> String {
        FILES_RENDERER_TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let template_disposables = DisposableStore::new();
        let label = self.services.labels.create(container, Some(je_widgets::icon_label::IIconLabelCreationOptions { support_highlights: true, ..Default::default() }));
        template_disposables.add(Rc::new(label.clone()));
        let current_context: Rc<RefCell<Option<Rc<ExplorerItem>>>> = Rc::new(RefCell::new(None));

        template_disposables.add(label.on_did_render().listen({
            let (current_context, update_width) = (current_context.clone(), self.services.update_width.clone());
            move |_| {
                // schedule this on the next animation frame to avoid rendering reentry
                let (current_context, update_width) = (current_context.clone(), update_width.clone());
                runtime::set_timeout(
                    move || {
                        // noop when the element is no longer in the tree: no update of width necessary
                        if let Some(context) = current_context.borrow().clone() {
                            update_width(&context);
                        }
                    },
                    0,
                );
            }
        }));

        let element_disposables = template_disposables.add_rc(DisposableStore::new());
        Rc::new(FileTemplateData { template_disposables, element_disposables, label, container, current_context }) as Rc<dyn Any>
    }

    fn render_element(&self, node: &Rc<<Model as ITreeModel>::Node>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let template = Self::template(template_data);
        let stat = node.element().expect("elemento da árvore");
        *template.current_context.borrow_mut() = Some(stat.clone());

        let editable_data = self.services.explorer_service.get_editable_data(&stat);
        self.services.ui.tree_mut().remove_class(template.label.element(), "compressed");

        match editable_data {
            // File Label
            None => self.render_stat(&stat, IconLabelText::Single(stat.name()), None, template),
            // Input Box
            Some(editable_data) => {
                hide(&mut self.services.ui.tree_mut(), template.label.element());
                template.element_disposables.add(self.render_input_box(template.container, &stat, editable_data));
            }
        }
    }

    fn dispose_element(&self, _node: &Rc<<Model as ITreeModel>::Node>, _index: i64, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let template = Self::template(template_data);
        *template.current_context.borrow_mut() = None;
        template.element_disposables.clear();
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        Self::template(template_data).template_disposables.dispose();
    }
}

impl ICompressibleTreeRenderer<Rc<ExplorerItem>, ()> for FilesRenderer {
    fn render_compressed_elements(&self, node: &Rc<CompressedNode>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let template = Self::template(template_data);
        let elements = node.element.as_ref().expect("nó comprimido").elements.clone();
        let stat = elements.last().expect("nó comprimido sem elementos").clone();
        *template.current_context.borrow_mut() = Some(stat.clone());

        let editable: Vec<&Rc<ExplorerItem>> = elements.iter().filter(|e| self.services.explorer_service.is_editable(Some(*e))).collect();
        let editable_data = editable.first().and_then(|e| self.services.explorer_service.get_editable_data(e));

        let Some(editable_data) = editable_data else {
            // File Label
            let label_element = template.label.element();
            {
                let mut tree = self.services.ui.tree_mut();
                tree.add_class(label_element, "compressed");
            }

            let id = format!("compressed-explorer_{}", CompressedNavigationController::next_id());
            let labels: Vec<String> = elements.iter().map(|e| e.name()).collect();
            // A pontuação fuzzy não existe neste porte (ver o topo do módulo), então não há deslocamento a ajustar.
            self.render_stat(&stat, IconLabelText::Multiple(labels), Some(id.clone()), template);

            let compressed_navigation_controller = CompressedNavigationController::new(id, elements.clone(), node.depth, node.collapsed.get());
            template.element_disposables.add(compressed_navigation_controller.clone());

            self.compressed_navigation_controllers.borrow_mut().entry(ById(stat.clone())).or_default().push(compressed_navigation_controller.clone());

            // accessibility
            template.element_disposables.add(self.on_did_change_active_descendant.add(compressed_navigation_controller.on_did_change()));

            template.element_disposables.add(to_disposable({
                let (controllers, stat, controller) = (self.compressed_navigation_controllers.clone(), stat, compressed_navigation_controller);
                move || {
                    let mut controllers = controllers.borrow_mut();
                    let key = ById(stat);
                    let Some(node_controllers) = controllers.get_mut(&key) else {
                        errors::throw(Error::new("Disposing unknown navigation controller"))
                    };
                    let Some(rendered_index) = node_controllers.iter().position(|candidate| Rc::ptr_eq(candidate, &controller)) else {
                        errors::throw(Error::new("Disposing unknown navigation controller"))
                    };

                    if node_controllers.len() == 1 {
                        controllers.remove(&key);
                    } else {
                        node_controllers.remove(rendered_index);
                    }
                }
            }));
            return;
        };

        // Input Box
        let mut tree = self.services.ui.tree_mut();
        tree.remove_class(template.label.element(), "compressed");
        hide(&mut tree, template.label.element());
        drop(tree);
        template.element_disposables.add(self.render_input_box(template.container, editable[0], editable_data));
    }

    fn dispose_compressed_elements(&self, _node: &Rc<CompressedNode>, _index: i64, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let template = Self::template(template_data);
        *template.current_context.borrow_mut() = None;
        template.element_disposables.clear();
    }
}
