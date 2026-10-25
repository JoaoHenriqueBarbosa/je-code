//! Porte de `src/vs/workbench/contrib/comments/browser/commentsTreeViewer.ts`: a árvore do painel de comentários (`CommentsList`),
//! os renderers de recurso (`ResourceWithCommentsRenderer`) e de comentário (`CommentNodeRenderer`), os menus
//! (`CommentsMenus`), o delegado de alturas, o filtro (`Filter`) e o provedor de identidade.
//!
//! Divergências:
//! - **Elementos.** `CommentsModel | ResourceWithCommentThreads | CommentNode` é o enum [`CommentsElement`], que compara e calcula o
//!   hash pela identidade do objeto (o `===` do JavaScript). A árvore é uma `WorkbenchObjectTree`, então o elemento da árvore é
//!   `Option<CommentsElement>` (o `null` é a raiz); o filtro, o provedor de acessibilidade e o de identidade que o original põe nas
//!   opções entram pelos adaptadores de `Option` deste arquivo.
//! - **Injeção.** Os parâmetros decorados dos construtores (`@IConfigurationService`, `@IHoverService`, `@IThemeService`,
//!   `@IMenuService`, `@IContextKeyService`, `@IListService`, `@IInstantiationService`, `@IContextMenuService`,
//!   `@IKeybindingService`) são as structs [`CommentsTreeServices`] (renderers) e [`CommentsListServices`] (a árvore). O hover é o
//!   de base de `je-widgets` (o serviço de hover do porte é global). `createActionViewItem.bind(undefined, instantiationService)` é o
//!   [`IActionViewItemProvider`] de `CommentsTreeServices`.
//! - **`CommentsList extends WorkbenchObjectTree`** é composição com `Deref` para a árvore, como o `SettingsTree`.
//! - **DOM e CSS.** Os renderers montam nós do `je-ui` com as mesmas classes (`resource-container`, `separator`, `owner`,
//!   `comment-thread-container`, `comment-metadata-container`, `comment-metadata`, `icon`, `user`, `timestamp-container`,
//!   `relevance`, `text`, `range`, `actions`, `comment-snippet-container`, `count`, `reply-detail`). As regras de `panel.css`
//!   (linhas flex de 22px, `padding-right` de 5px nos itens, o selo `relevance` de raio 2px, a opacidade 0,8, o recuo de 16px do
//!   resumo das respostas, as ações escondidas até o ponteiro, a seleção ou o foco, `overflow: hidden` com reticências) viram o
//!   `layout` do template, que mede os nós e os posiciona (o `min-width: fit-content` é o item que não encolhe, e `flex-shrink: 1` o
//!   encolhimento proporcional de [`shrink_to_fit`]). O `layout` roda ao desenhar o elemento e quando o ponteiro entra ou sai; o
//!   estado `selected` e `focused` da linha só é relido nele (a seleção não dispara um novo).
//! - **Texto da pré-visualização.** `innerText = body` de um corpo em texto vira um [`HighlightedLabel`] com as quebras de linha
//!   tiradas: no `.text` (um contêiner flex) cada trecho de texto é um item anônimo, e as linhas ficam lado a lado.
//! - **Corpo em Markdown.** `renderMarkdown(commentBody, {}, span)` é `render_markdown` mais o [`MarkdownView`] (a tradução nativa do
//!   Markdown renderizado de `settings_markdown_view`). `getRenderedComment` (imagens viram "Image: alt", títulos viram texto, réguas
//!   iniciais saem) e a poda dos filhos depois do primeiro são as funções sobre o [`Element`] sanitizado. A linha do parágrafo é de
//!   22px (o `line-height` herdado de `.comment-metadata-container`).
//! - **Cor do ícone.** A variável `--comment-view-thread-state-color` é a cor do codicon do nó. Sem estado na thread o ícone usa a
//!   cor do texto (o original deixaria a cor da linha anterior no template reaproveitado).
//! - **`getCommentThreadWidgetStateColor`** só repassa para `getCommentThreadStateIconColor`; a chamada é direta.
//! - **Data do comentário.** `new Date(comment.timestamp)` é `try_parse_date`; um texto que não é data vira "sem data".
//! - **Menu de contexto.** `treeEvent.anchor` é o nó do alvo do evento (ou o da árvore); `event.preventDefault()` e
//!   `stopPropagation()` não existem (a árvore nativa não tem menu do navegador). `getActionViewItem` e `getActionsContext` do
//!   delegado viajam em [`ContextMenuExtras`]. O `new ActionViewItem(action, action, ...)` do original usa a ação como contexto,
//!   que aqui é `None`. O `MarshalledCommentThreadInternal` leva a thread como objeto hospedado em `thread`.
//! - **Filtro.** Para a raiz (`CommentsModel`), que a árvore nunca filtra, `filter` devolve visível (o original cairia em
//!   `filterCommentNode`). O resultado é o [`TreeFilterResult`] (`false` e `true` são `Bool`, `TreeVisibility` é `Visibility`), e os
//!   três desfechos por texto são o `text_filter_result` do painel de problemas.
//! - **`ICommentsListOptions extends IWorkbenchAsyncDataTreeOptions`** guarda só o que o construtor lê:
//!   `accessibilityProvider`, `overrideStyles`, `filter` e `sorter` ([`CommentsListOptions`]).

use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::actions::ActionRef;
use je_base::browser::html_parser::{Element, Node};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error};
use je_base::filters::Match;
use je_base::keybindings::ResolvedKeybinding;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, to_disposable};
use je_base::markdown::{MarkdownRenderOptions, render_markdown};
use je_base::marshalling_ids::MarshalledId;
use je_base::nls::{NlsArg, localize, localize_args};
use je_base::themables::ThemeIcon;
use je_base::JsString;
use je_editor::languages::{CanReply, Comment, CommentBody, CommentState, CommentThread, CommentThreadApplicability, CommentThreadState};
use je_editor::range::Range;
use je_editor::services::theme_service::IThemeService;
use je_platform::actions::menu_entry_action_view_item::{PrimaryAndSecondaryActions, get_context_menu_actions};
use je_platform::actions::menu_ids;
use je_platform::actions::{IMenuService, MenuActionOptions, MenuId};
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::contextview::context_view::{AnchorTarget, ContextMenuRequest, IContextMenuDelegate, IContextMenuService};
use je_platform::contextview::view_element::ViewElement;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::resources::ext_uri;
use je_render::Rgba;
use je_theme::to_rgba8;
use je_ui::{Content, EventKind, NodeId, Role, Ui};
use je_widgets::action_bar::{ActionBar, IActionBarOptions, IActionOptions, IActionViewItemProvider};
use je_widgets::action_view_items::{ActionBarStyles, ActionViewItem, IActionViewItem, IActionViewItemOptions};
use je_widgets::dom::create_el;
use je_widgets::highlighted_label::{HighlightedLabel, HighlightedLabelStyles};
use je_widgets::list::list_styles::IListStyles;
use je_widgets::list::list_types::{IIdentityProvider, IListAccessibilityProvider, IListVirtualDelegate, TemplateData};
use je_widgets::text::{measure_text, ui_font};
use je_widgets::tree::abstract_tree::{AbstractTreeOptions, AbstractTreeOptionsUpdate, ExpandOnlyOnTwistieClick};
use je_widgets::tree::object_tree::ObjectTreeOptions;
use je_widgets::tree::object_tree_model::ObjectTreeModel;
use je_widgets::tree::tree_types::{
    ITreeFilter, ITreeModel, ITreeRenderer, ITreeSorter, TreeElementRenderDetails, TreeFilterResult, TreeMouseEvent, TreeNodeApi, TreeVisibility, VisibilityValue,
};

use crate::browser::labels::{IResourceLabel, ResourceLabels};
use crate::browser::workbench_tree::{IWorkbenchTreeOptions, WorkbenchObjectTree, WorkbenchTreeServices};
use crate::contrib::comments::common::comment_model::{CommentNode, ResourceWithCommentThreads};
use crate::contrib::markers::markers_tree_viewer::{
    CODICON_SIZE, LINE_HEIGHT, create_resource_label, place, row_selected_or_focused, set_resource_label_file, setup_mouse_hover, text_filter_result, units,
};
use crate::contrib::preferences::settings_markdown_view::{MarkdownView, MarkdownViewStyles, OwnedMarkdownEnvironment};
use crate::contrib::update::update_utils::try_parse_date;
use crate::platform::contextview::context_menu_handler::{ContextMenuExtras, native_extras};

use super::comment_colors::get_comment_thread_state_icon_color;
use super::comments_filter_options::FilterOptions;
use super::comments_model::CommentsModel;
use super::timestamp::TimestampWidget;

pub const COMMENTS_VIEW_ID: &str = "workbench.panel.comments";
pub const COMMENTS_VIEW_STORAGE_ID: &str = "Comments";

/// `COMMENTS_VIEW_TITLE`.
pub fn comments_view_title() -> je_platform::action::LocalizedString {
    je_platform::action::localize2("comments.view.title", "Comments")
}

/// O `padding-right: 5px` dos itens da linha (`.count`, `.user`, `.timestamp`, `.separator`, `.range *`...).
const ITEM_PADDING_RIGHT: f64 = 5.0;

/// O `.icon` (`padding-top: 4px`; `padding-right: 5px`) com o codicon de 16px.
const ICON_PADDING_TOP: f64 = 4.0;
const ICON_BOX_WIDTH: f64 = CODICON_SIZE + ITEM_PADDING_RIGHT;

/// `margin-right: 5px` de `.comment-metadata-container` e `.comment-snippet-container`.
const ROW_MARGIN_RIGHT: f64 = 5.0;

/// `padding-left: 16px` de `.comment-snippet-container`.
const SNIPPET_PADDING_LEFT: f64 = 16.0;

/// O selo `.relevance`: `padding: 0 4px 1px`, `margin: 4px 4px 3px 0`, `line-height: 14px`.
const RELEVANCE_PADDING_X: f64 = 4.0;
const RELEVANCE_PADDING_BOTTOM: f64 = 1.0;
const RELEVANCE_MARGIN_TOP: f64 = 4.0;
const RELEVANCE_MARGIN_RIGHT: f64 = 4.0;
const RELEVANCE_LINE_HEIGHT: f64 = 14.0;
const RELEVANCE_RADIUS: f64 = 2.0;

/// `font-size: 0.9em`.
const SMALL_FONT_SCALE: f32 = 0.9;

/// `opacity: 0.8`.
const DIMMED_OPACITY: f32 = 0.8;

/// `padding-right: 5px` de `.monaco-icon-label` na árvore de comentários.
const ICON_LABEL_PADDING_RIGHT: f64 = 5.0;

/// `·`.
const SEPARATOR: &str = "\u{00b7}";

// ---------------------------------------------------------------------------------------------------------------------
// Elementos da árvore
// ---------------------------------------------------------------------------------------------------------------------

/// `CommentsModel | ResourceWithCommentThreads | CommentNode`.
#[derive(Clone)]
pub enum CommentsElement {
    Model(Rc<CommentsModel>),
    Resource(Rc<ResourceWithCommentThreads>),
    Comment(Rc<CommentNode>),
}

/// A identidade do `===` do JavaScript: dois elementos são iguais quando são o mesmo objeto.
impl PartialEq for CommentsElement {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Model(a), Self::Model(b)) => Rc::ptr_eq(a, b),
            (Self::Resource(a), Self::Resource(b)) => Rc::ptr_eq(a, b),
            (Self::Comment(a), Self::Comment(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for CommentsElement {}

impl Hash for CommentsElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Model(model) => {
                0u8.hash(state);
                Rc::as_ptr(model).hash(state);
            }
            Self::Resource(resource) => {
                1u8.hash(state);
                Rc::as_ptr(resource).hash(state);
            }
            Self::Comment(comment) => {
                2u8.hash(state);
                Rc::as_ptr(comment).hash(state);
            }
        }
    }
}

/// O modelo da árvore do painel (`WorkbenchObjectTree<CommentsModel | ResourceWithCommentThreads | CommentNode, any>`).
pub type CommentsTreeModel = ObjectTreeModel<CommentsElement, FilterData>;

/// O nó que os renderers recebem.
pub type CommentsTreeNode = <CommentsTreeModel as ITreeModel>::Node;

/// `ICommentsListOptions`.
#[derive(Clone, Default)]
pub struct CommentsListOptions {
    pub accessibility_provider: Option<Rc<dyn IListAccessibilityProvider<CommentsElement>>>,
    pub override_styles: Option<IListStyles>,
    pub filter: Option<Rc<dyn ITreeFilter<CommentsElement, FilterData>>>,
    pub sorter: Option<Rc<dyn ITreeSorter<CommentsElement>>>,
}

// ---------------------------------------------------------------------------------------------------------------------
// Estilos e serviços
// ---------------------------------------------------------------------------------------------------------------------

/// Os estilos que o original deixa para `panel.css` e para os `defaultXStyles`.
#[derive(Clone)]
pub struct CommentsTreeStyles {
    /// Cor e fonte do texto da linha (`font-size` de 13px; a altura de linha é ajustada aqui).
    pub label: HighlightedLabelStyles,
    /// `--vscode-badge-background` e `--vscode-badge-foreground`: o selo `relevance`.
    pub badge_background: Option<Rgba>,
    pub badge_foreground: Option<Rgba>,
    /// As cores do Markdown do resumo do comentário (`textLink.*` e `textPreformat.*`).
    pub markdown: MarkdownViewStyles,
    /// As cores das barras de ações.
    pub action_bar: Option<Rc<ActionBarStyles>>,
}

impl CommentsTreeStyles {
    /// O texto da linha na fonte de `scale` vezes a base, com a altura de linha de 22px.
    fn text(&self, scale: f32) -> HighlightedLabelStyles {
        HighlightedLabelStyles { font_size: self.label.font_size * scale, line_height: Some(LINE_HEIGHT), ..self.label }
    }

    /// O texto do selo `relevance`: `font-size: 0.9em` e `line-height: 14px`.
    fn relevance(&self) -> HighlightedLabelStyles {
        HighlightedLabelStyles {
            foreground: self.badge_foreground.unwrap_or(self.label.foreground),
            font_size: self.label.font_size * SMALL_FONT_SCALE,
            line_height: Some(RELEVANCE_LINE_HEIGHT),
            ..self.label
        }
    }
}

/// Os serviços que os renderers leem.
#[derive(Clone)]
pub struct CommentsTreeServices {
    pub ui: Ui,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub theme_service: Rc<dyn IThemeService>,
    /// O ambiente do `renderMarkdown` (a plataforma ainda não tem o `IMarkdownRendererService`).
    pub markdown_environment: Rc<OwnedMarkdownEnvironment>,
    /// `createActionViewItem.bind(undefined, instantiationService)`.
    pub action_view_item_provider: IActionViewItemProvider,
    pub styles: CommentsTreeStyles,
}

/// Os serviços da [`CommentsList`].
#[derive(Clone)]
pub struct CommentsListServices {
    /// `IContextKeyService`, `IListService`, `IConfigurationService` e `defaultListStyles` da `WorkbenchObjectTree`.
    pub tree: WorkbenchTreeServices,
    pub renderers: CommentsTreeServices,
    pub menu_service: Rc<dyn IMenuService>,
    pub context_menu_service: Rc<dyn IContextMenuService>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
}

// ---------------------------------------------------------------------------------------------------------------------
// Auxiliares
// ---------------------------------------------------------------------------------------------------------------------

/// O texto de `typeof body === 'string' ? body : body.value`.
fn comment_body_text(comment: &Comment) -> String {
    match &comment.body {
        CommentBody::Text(text) => text.clone(),
        CommentBody::Markdown(markdown) => markdown.value.to_string_lossy(),
    }
}

/// `comment.timestamp ? new Date(comment.timestamp) : undefined`.
fn comment_date(comment: &Comment) -> Option<f64> {
    comment.timestamp.as_deref().filter(|timestamp| !timestamp.is_empty()).and_then(|timestamp| try_parse_date(Some(timestamp)))
}

/// O codicon de um id (`Codicon.commentDraft`...).
fn codicon(id: &str) -> ThemeIcon {
    ThemeIcon::from_id(id)
}

/// A largura que o texto do rótulo teria sem corte.
fn natural_width(label: &HighlightedLabel) -> f64 {
    label.set_max_width(None);
    label.width()
}

/// Corta o rótulo em `max_width` (reticências) e o posiciona em (`x`, `y`) com a largura que sobrou.
fn place_label(ui: &Ui, label: &HighlightedLabel, x: f64, y: f64, max_width: f64) {
    let max_width = max_width.max(0.0);
    label.set_max_width(Some(max_width));
    let width = label.width().min(max_width);
    place(&mut ui.tree_mut(), label.element(), x, y, width, label.line_height());
}

/// Um rótulo de texto filho de `parent`, com a classe e a opacidade do CSS.
fn create_label(ui: &Ui, parent: NodeId, class_name: &str, styles: HighlightedLabelStyles, opacity: f32) -> Rc<HighlightedLabel> {
    let label = HighlightedLabel::new(ui, parent, None, styles);
    let mut tree = ui.tree_mut();
    tree.add_class(label.element(), class_name);
    if opacity < 1.0 {
        tree.style_mut(label.element()).opacity = opacity;
    }
    label
}

/// Mostra ou esconde um nó (o `display: none` do CSS).
fn set_visible(ui: &Ui, node: NodeId, visible: bool) {
    ui.tree_mut().style_mut(node).visible = visible;
}

/// Um item de uma linha flex: a largura base e se encolhe (`min-width: fit-content` é o que não encolhe).
struct FlexItem {
    width: f64,
    shrinkable: bool,
}

impl FlexItem {
    fn fixed(width: f64) -> Self {
        Self { width, shrinkable: false }
    }

    fn shrinkable(width: f64) -> Self {
        Self { width, shrinkable: true }
    }
}

/// O `flex-shrink: 1` de uma linha: o que passa de `available` sai dos itens que encolhem, em proporção à largura de cada um, sem
/// passar de zero (o que sobrar de um item que zerou vai para os demais).
fn shrink_to_fit(items: &mut [FlexItem], available: f64) {
    for _ in 0..items.len() {
        let overflow = items.iter().map(|item| item.width).sum::<f64>() - available;
        if overflow <= 0.0 {
            return;
        }
        let weight: f64 = items.iter().filter(|item| item.shrinkable && item.width > 0.0).map(|item| item.width).sum();
        if weight <= 0.0 {
            return;
        }
        for item in items.iter_mut().filter(|item| item.shrinkable && item.width > 0.0) {
            item.width = (item.width - overflow * item.width / weight).max(0.0);
        }
    }
}

/// `{ commentControlHandle, commentThreadHandle, $mid: MarshalledId.CommentThread }`, com a thread do modelo em `thread` quando
/// `with_thread` (o `MarshalledCommentThreadInternal`).
fn marshalled_comment_thread(node: &CommentNode, with_thread: bool) -> Dynamic {
    let mut entries = vec![
        ("commentControlHandle", Dynamic::from(node.controller_handle as f64)),
        ("commentThreadHandle", Dynamic::from(node.thread_handle as f64)),
        ("$mid", Dynamic::from(MarshalledId::CommentThread as i32)),
    ];
    if with_thread {
        entries.push(("thread", Dynamic::Object(ObjectRef::with_host(Rc::new(node.thread.clone())))));
    }
    Dynamic::object(entries)
}

/// O `thread.canReply` (`boolean | CommentAuthorInformation`) como valor de chave de contexto.
fn can_reply_value(can_reply: &CanReply) -> Dynamic {
    match can_reply {
        CanReply::Bool(can_reply) => Dynamic::Bool(*can_reply),
        CanReply::Author(author) => Dynamic::object([("name", Dynamic::from(author.name.as_str()))]),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// CommentsModelVirtualDelegate
// ---------------------------------------------------------------------------------------------------------------------

/// `CommentsModelVirtualDelegate`.
struct CommentsModelVirtualDelegate;

impl CommentsModelVirtualDelegate {
    const RESOURCE_ID: &'static str = "resource-with-comments";
    const COMMENT_ID: &'static str = "comment-node";
}

impl IListVirtualDelegate<CommentsElement> for CommentsModelVirtualDelegate {
    fn get_height(&self, element: &CommentsElement) -> f64 {
        if let CommentsElement::Comment(comment) = element
            && comment.has_reply()
        {
            return 44.0;
        }
        22.0
    }

    fn get_template_id(&self, element: &CommentsElement) -> String {
        match element {
            CommentsElement::Resource(_) => Self::RESOURCE_ID,
            CommentsElement::Comment(_) => Self::COMMENT_ID,
            CommentsElement::Model(_) => "",
        }
        .to_owned()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// ResourceWithCommentsRenderer
// ---------------------------------------------------------------------------------------------------------------------

/// `IResourceTemplateData`.
struct ResourceTemplate {
    ui: Ui,
    container: NodeId,
    resource_container: NodeId,
    resource_label: Rc<dyn IResourceLabel>,
    separator: Rc<HighlightedLabel>,
    owner: Rc<HighlightedLabel>,
}

impl ResourceTemplate {
    /// `.resource-container { display: flex; overflow: hidden }`: o rótulo, o separador e o dono, que encolhe com reticências.
    fn layout(&self) {
        let ui = &self.ui;
        let (row_width, label_width) = {
            let tree = ui.tree();
            (tree.bounds(self.container).width(), tree.bounds(self.resource_label.element()).width())
        };
        let separator_visible = ui.tree().style(self.separator.element()).visible;
        let separator_width = if separator_visible { natural_width(&self.separator) + ITEM_PADDING_RIGHT } else { 0.0 };

        let mut x = 0.0;
        place(&mut ui.tree_mut(), self.resource_container, 0.0, 0.0, row_width, LINE_HEIGHT);
        place(&mut ui.tree_mut(), self.resource_label.element(), x, 0.0, label_width, LINE_HEIGHT);
        x += label_width + ICON_LABEL_PADDING_RIGHT;
        if separator_visible {
            place_label(ui, &self.separator, x, 0.0, separator_width - ITEM_PADDING_RIGHT);
            x += separator_width;
        }
        place_label(ui, &self.owner, x, 0.0, (row_width - x - ITEM_PADDING_RIGHT).max(0.0));
    }
}

/// `ResourceWithCommentsRenderer`.
pub struct ResourceWithCommentsRenderer {
    labels: Rc<ResourceLabels>,
    services: Rc<CommentsTreeServices>,
}

impl ResourceWithCommentsRenderer {
    pub fn new(labels: Rc<ResourceLabels>, services: Rc<CommentsTreeServices>) -> Self {
        Self { labels, services }
    }

    fn template(template_data: &TemplateData) -> &ResourceTemplate {
        template_data.downcast_ref::<ResourceTemplate>().expect("o template do ResourceWithCommentsRenderer")
    }
}

impl ITreeRenderer<CommentsTreeModel> for ResourceWithCommentsRenderer {
    fn template_id(&self) -> String {
        CommentsModelVirtualDelegate::RESOURCE_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let ui = self.services.ui.clone();
        let (resource_container, resource_label) = create_resource_label(&ui, &self.labels, container, "resource-container", None);
        ui.tree_mut().style_mut(resource_container).clip = true;
        let separator = create_label(&ui, resource_container, "separator", self.services.styles.text(1.0), DIMMED_OPACITY);
        let owner = create_label(&ui, resource_container, "owner", self.services.styles.text(SMALL_FONT_SCALE), DIMMED_OPACITY);

        Rc::new(ResourceTemplate { ui, container, resource_container, resource_label, separator, owner })
    }

    fn render_element(&self, node: &Rc<CommentsTreeNode>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let Some(CommentsElement::Resource(resource)) = node.element() else { return };
        let template = Self::template(template_data);

        set_resource_label_file(&*template.resource_label, &resource.resource, None);
        template.separator.set(Some(SEPARATOR), &[], "", false, false);

        let owner_label = resource.owner_label.borrow().clone().filter(|owner_label| !owner_label.is_empty());
        match owner_label {
            Some(owner_label) => {
                template.owner.set(Some(&owner_label), &[], "", false, false);
                set_visible(&template.ui, template.separator.element(), true);
            }
            None => {
                template.owner.set(Some(""), &[], "", false, false);
                set_visible(&template.ui, template.separator.element(), false);
            }
        }
        template.layout();
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        let template = Self::template(template_data);
        template.resource_label.dispose();
        template.separator.dispose();
        template.owner.dispose();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// CommentsMenus
// ---------------------------------------------------------------------------------------------------------------------

/// `CommentsMenus`.
pub struct CommentsMenus {
    menu_service: Rc<dyn IMenuService>,
    context_key_service: RefCell<Option<Rc<dyn IContextKeyService>>>,
}

impl CommentsMenus {
    pub fn new(menu_service: Rc<dyn IMenuService>) -> Rc<Self> {
        Rc::new(Self { menu_service, context_key_service: RefCell::new(None) })
    }

    /// `getResourceActions(element)`: o `{ actions }` do original é a lista das ações primárias.
    pub fn get_resource_actions(&self, element: &CommentNode) -> Vec<ActionRef> {
        self.get_actions(&menu_ids::comments_view_thread_actions(), element).primary
    }

    /// `getResourceContextActions(element)`.
    pub fn get_resource_context_actions(&self, element: &CommentNode) -> Vec<ActionRef> {
        self.get_actions(&menu_ids::comments_view_thread_actions(), element).secondary
    }

    /// `setContextKeyService(service)`.
    pub fn set_context_key_service(&self, service: Rc<dyn IContextKeyService>) {
        *self.context_key_service.borrow_mut() = Some(service);
    }

    fn get_actions(&self, menu_id: &MenuId, element: &CommentNode) -> PrimaryAndSecondaryActions {
        let Some(context_key_service) = self.context_key_service.borrow().clone() else {
            return PrimaryAndSecondaryActions::default();
        };

        let overlay: Vec<(String, Dynamic)> = vec![
            ("commentController".to_owned(), Dynamic::from(element.owner.as_str())),
            ("resourceScheme".to_owned(), Dynamic::from(element.resource.scheme().to_string_lossy())),
            ("commentThread".to_owned(), element.context_value.as_deref().map_or(Dynamic::Undefined, |value| Dynamic::from(value))),
            ("canReply".to_owned(), can_reply_value(&element.thread.can_reply())),
        ];
        let context_key_service = context_key_service.create_overlay(overlay);

        let menu = self.menu_service.get_menu_actions(menu_id, &*context_key_service, Some(&MenuActionOptions { should_forward_args: true, ..MenuActionOptions::default() }));
        get_context_menu_actions(&menu, Some("inline"))
    }
}

impl IDisposable for CommentsMenus {
    fn dispose(&self) {
        *self.context_key_service.borrow_mut() = None;
    }

    fn constructor_name(&self) -> &'static str {
        "CommentsMenus"
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// CommentNodeRenderer
// ---------------------------------------------------------------------------------------------------------------------

/// `ICommentThreadTemplateData`, com os nós da linha para o `layout`.
struct CommentNodeTemplate {
    this: Weak<CommentNodeTemplate>,
    ui: Ui,
    /// O contêiner da linha (`.monaco-tl-contents`).
    container: NodeId,
    thread_container: NodeId,
    metadata_container: NodeId,
    metadata: NodeId,
    icon: NodeId,
    user_names: Rc<HighlightedLabel>,
    timestamp_container: NodeId,
    timestamp: Rc<TimestampWidget>,
    relevance: NodeId,
    relevance_label: Rc<HighlightedLabel>,
    separator: Rc<HighlightedLabel>,
    comment_preview: NodeId,
    preview_label: Rc<HighlightedLabel>,
    range: Rc<HighlightedLabel>,
    actions: NodeId,
    action_bar: Rc<ActionBar>,
    snippet_container: NodeId,
    replies_icon: NodeId,
    count: Rc<HighlightedLabel>,
    last_reply_detail: Rc<HighlightedLabel>,
    replies_separator: Rc<HighlightedLabel>,
    replies_timestamp_container: NodeId,
    replies_timestamp: Rc<TimestampWidget>,
    disposables: Rc<DisposableStore>,
    element_disposables: Rc<DisposableStore>,
    /// O Markdown do resumo do comentário, quando o corpo é Markdown.
    markdown_view: RefCell<Option<Rc<MarkdownView>>>,
    /// A largura que o texto do Markdown teria numa linha só.
    preview_natural_width: Cell<f64>,
    has_reply: Cell<bool>,
    outdated: Cell<bool>,
    hovered: Cell<bool>,
}

impl CommentNodeTemplate {
    /// O ponteiro sobre a linha mostra as ações (`.monaco-list-row:hover .comment-metadata-container .actions`).
    fn attach_listeners(&self) {
        let (enter, leave) = (self.this.clone(), self.this.clone());
        self.disposables.add(self.ui.listen(self.container, EventKind::PointerEnter, false, move |_| {
            if let Some(this) = enter.upgrade() {
                this.hovered.set(true);
                this.layout();
            }
        }));
        self.disposables.add(self.ui.listen(self.container, EventKind::PointerLeave, false, move |_| {
            if let Some(this) = leave.upgrade() {
                this.hovered.set(false);
                this.layout();
            }
        }));
    }

    /// As regras de `panel.css` da linha: o resumo do comentário (`.comment-metadata-container`) e o das respostas
    /// (`.comment-snippet-container`, só com respostas).
    fn layout(&self) {
        let ui = self.ui.clone();
        let row_width = ui.tree().bounds(self.container).width();
        let active = self.hovered.get() || row_selected_or_focused(&ui.tree(), self.container);
        let inner_width = (row_width - ROW_MARGIN_RIGHT).max(0.0);
        let has_reply = self.has_reply.get();
        let outdated = self.outdated.get();

        // `.actions { display: none }` até o ponteiro, a seleção ou o foco
        let actions_width: f64 = (0..self.action_bar.length()).map(|index| self.action_bar.get_width(index)).sum();
        let actions_visible = active && actions_width > 0.0;
        let actions_width = if actions_visible { actions_width } else { 0.0 };
        let metadata_width = (inner_width - actions_width).max(0.0);

        let timestamp_visible = self.timestamp.is_visible();
        let has_range = !self.range.text().is_empty();
        let markdown_view = self.markdown_view.borrow().clone();
        let text_natural_width = if markdown_view.is_some() { self.preview_natural_width.get() + ITEM_PADDING_RIGHT } else { natural_width(&self.preview_label) };

        {
            let mut tree = ui.tree_mut();
            let thread_height = if has_reply { 2.0 * LINE_HEIGHT } else { LINE_HEIGHT };
            place(&mut tree, self.thread_container, 0.0, 0.0, row_width, thread_height);
            place(&mut tree, self.metadata_container, 0.0, 0.0, inner_width, LINE_HEIGHT);
            place(&mut tree, self.metadata, 0.0, 0.0, metadata_width, LINE_HEIGHT);
            place(&mut tree, self.actions, metadata_width, 0.0, actions_width, LINE_HEIGHT);
            tree.style_mut(self.actions).visible = actions_visible;
            tree.style_mut(self.relevance).visible = outdated;
            tree.style_mut(self.separator.element()).visible = !outdated;
            tree.style_mut(self.range.element()).visible = has_range;
            tree.style_mut(self.timestamp_container).visible = timestamp_visible;
            tree.style_mut(self.preview_label.element()).visible = markdown_view.is_none();
            tree.style_mut(self.snippet_container).visible = has_reply;
        }

        // `.comment-metadata { flex: 1; display: flex }`
        let mut items = [
            FlexItem::fixed(ICON_BOX_WIDTH),
            FlexItem::fixed(natural_width(&self.user_names) + ITEM_PADDING_RIGHT),
            FlexItem::shrinkable(if timestamp_visible { self.timestamp.natural_width() + ITEM_PADDING_RIGHT } else { 0.0 }),
            FlexItem::fixed(if outdated { natural_width(&self.relevance_label) + 2.0 * RELEVANCE_PADDING_X + RELEVANCE_MARGIN_RIGHT } else { 0.0 }),
            FlexItem::shrinkable(if outdated { 0.0 } else { natural_width(&self.separator) + ITEM_PADDING_RIGHT }),
            FlexItem::shrinkable(text_natural_width),
            FlexItem::fixed(if has_range { natural_width(&self.range) + ITEM_PADDING_RIGHT } else { 0.0 }),
        ];
        shrink_to_fit(&mut items, metadata_width);

        let mut x = 0.0;
        place(&mut ui.tree_mut(), self.icon, x, ICON_PADDING_TOP, CODICON_SIZE, CODICON_SIZE);
        x += items[0].width;

        place_label(&ui, &self.user_names, x, 0.0, items[1].width - ITEM_PADDING_RIGHT);
        x += items[1].width;

        place(&mut ui.tree_mut(), self.timestamp_container, x, 0.0, items[2].width, LINE_HEIGHT);
        self.timestamp.layout(items[2].width - ITEM_PADDING_RIGHT);
        x += items[2].width;

        if outdated {
            let badge_width = items[3].width - RELEVANCE_MARGIN_RIGHT;
            let mut tree = ui.tree_mut();
            place(&mut tree, self.relevance, x, RELEVANCE_MARGIN_TOP, badge_width, RELEVANCE_LINE_HEIGHT + RELEVANCE_PADDING_BOTTOM);
            place(&mut tree, self.relevance_label.element(), RELEVANCE_PADDING_X, 0.0, self.relevance_label.width(), RELEVANCE_LINE_HEIGHT);
        }
        x += items[3].width;

        if !outdated {
            place_label(&ui, &self.separator, x, 0.0, items[4].width - ITEM_PADDING_RIGHT);
        }
        x += items[4].width;

        let text_width = items[5].width;
        place(&mut ui.tree_mut(), self.comment_preview, x, 0.0, text_width, LINE_HEIGHT);
        match &markdown_view {
            Some(view) => {
                view.layout((text_width - ITEM_PADDING_RIGHT).max(0.0));
            }
            None => place_label(&ui, &self.preview_label, 0.0, 0.0, text_width),
        }
        x += text_width;

        if has_range {
            place_label(&ui, &self.range, x, 0.0, items[6].width - ITEM_PADDING_RIGHT);
        }

        if !has_reply {
            return;
        }

        // `.comment-snippet-container { display: flex; padding-left: 16px }`
        let replies_timestamp_visible = self.replies_timestamp.is_visible();
        let mut snippet_items = [
            FlexItem::fixed(ICON_BOX_WIDTH),
            FlexItem::fixed(natural_width(&self.count) + ITEM_PADDING_RIGHT),
            FlexItem::shrinkable(natural_width(&self.last_reply_detail) + ITEM_PADDING_RIGHT),
            FlexItem::shrinkable(natural_width(&self.replies_separator) + ITEM_PADDING_RIGHT),
            FlexItem::shrinkable(if replies_timestamp_visible { self.replies_timestamp.natural_width() + ITEM_PADDING_RIGHT } else { 0.0 }),
        ];
        shrink_to_fit(&mut snippet_items, (inner_width - SNIPPET_PADDING_LEFT).max(0.0));

        set_visible(&ui, self.replies_timestamp_container, replies_timestamp_visible);
        place(&mut ui.tree_mut(), self.snippet_container, 0.0, LINE_HEIGHT, inner_width, LINE_HEIGHT);
        let mut x = SNIPPET_PADDING_LEFT;
        place(&mut ui.tree_mut(), self.replies_icon, x, ICON_PADDING_TOP, CODICON_SIZE, CODICON_SIZE);
        x += snippet_items[0].width;
        place_label(&ui, &self.count, x, 0.0, snippet_items[1].width - ITEM_PADDING_RIGHT);
        x += snippet_items[1].width;
        place_label(&ui, &self.last_reply_detail, x, 0.0, snippet_items[2].width - ITEM_PADDING_RIGHT);
        x += snippet_items[2].width;
        place_label(&ui, &self.replies_separator, x, 0.0, snippet_items[3].width - ITEM_PADDING_RIGHT);
        x += snippet_items[3].width;
        place(&mut ui.tree_mut(), self.replies_timestamp_container, x, 0.0, snippet_items[4].width, LINE_HEIGHT);
        self.replies_timestamp.layout(snippet_items[4].width - ITEM_PADDING_RIGHT);
    }
}

/// `getRenderedComment`, sobre o Markdown já renderizado e sanitizado: as imagens viram o texto "Image: alt", os títulos viram texto
/// e as réguas iniciais saem.
fn clean_rendered_comment(element: &mut Element) {
    replace_images(element);
    replace_headings(element);
    remove_leading_rules(element);
}

/// O elemento que substitui uma imagem: `textDescription.textContent = image.alt ? "Image: {alt}" : "Image"`.
fn image_description(image: &Element) -> Element {
    let alt = image.get_attribute("alt").map(|alt| alt.to_string_lossy()).filter(|alt| !alt.is_empty());
    let text = match alt {
        Some(alt) => localize("imageWithLabel", "Image: {0}", &[&alt]),
        None => localize("image", "Image", &[]),
    };
    let mut description = Element::new("div", Vec::new());
    description.children.push(Node::Text(JsString::from(text)));
    description
}

fn replace_images(element: &mut Element) {
    for child in element.children.iter_mut() {
        let replacement = match child {
            Node::Element(inner) if inner.tag_name.eq_ignore_ascii_case("img") => Some(image_description(inner)),
            Node::Element(inner) => {
                replace_images(inner);
                None
            }
            _ => None,
        };
        if let Some(replacement) = replacement {
            *child = Node::Element(replacement);
        }
    }
}

fn is_heading(tag_name: &str) -> bool {
    matches!(tag_name.to_ascii_lowercase().as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

fn replace_headings(element: &mut Element) {
    for child in element.children.iter_mut() {
        let replacement = match child {
            Node::Element(inner) if is_heading(&inner.tag_name) => Some(Node::Text(inner.text_content())),
            Node::Element(inner) => {
                replace_headings(inner);
                None
            }
            _ => None,
        };
        if let Some(replacement) = replacement {
            *child = replacement;
        }
    }
}

/// `while (children.length > 1 && firstElementChild?.tagName === 'HR') removeChild(firstElementChild)`.
fn remove_leading_rules(element: &mut Element) {
    while element.children.iter().filter(|node| matches!(node, Node::Element(_))).count() > 1 {
        let Some(index) = element.children.iter().position(|node| matches!(node, Node::Element(_))) else { break };
        if !matches!(&element.children[index], Node::Element(first) if first.tag_name.eq_ignore_ascii_case("hr")) {
            break;
        }
        element.children.remove(index);
    }
}

/// `for (i = children.length - 1; i >= 1; i--) removeChild(children[i])`: só o primeiro elemento filho fica.
fn keep_first_element_child(element: &mut Element) {
    let mut seen = false;
    element.children.retain(|node| match node {
        Node::Element(_) => !std::mem::replace(&mut seen, true),
        _ => true,
    });
}

/// `CommentNodeRenderer`.
pub struct CommentNodeRenderer {
    services: Rc<CommentsTreeServices>,
    menus: Rc<CommentsMenus>,
}

impl CommentNodeRenderer {
    pub fn new(services: Rc<CommentsTreeServices>, menus: Rc<CommentsMenus>) -> Self {
        Self { services, menus }
    }

    fn template(template_data: &TemplateData) -> &Rc<CommentNodeTemplate> {
        template_data.downcast_ref::<Rc<CommentNodeTemplate>>().expect("o template do CommentNodeRenderer")
    }

    fn get_count_string(comment_count: usize) -> String {
        if comment_count > 2 {
            localize_args("commentsCountReplies", "{0} replies", &[NlsArg::Number((comment_count - 1) as f64)])
        } else if comment_count == 2 {
            localize("commentsCountReply", "1 reply", &[])
        } else {
            localize("commentCount", "1 comment", &[])
        }
    }

    /// `getIcon(threadState, hasDraft)`: o id do codicon.
    fn get_icon(thread_state: Option<CommentThreadState>, has_draft: bool) -> &'static str {
        // Priority: draft > unresolved > resolved
        if has_draft {
            "comment-draft"
        } else if thread_state == Some(CommentThreadState::Unresolved) {
            "comment-unresolved"
        } else {
            "comment"
        }
    }

    /// O corpo em Markdown como resumo: `getRenderedComment`, só o primeiro bloco, e o hover com o texto dele.
    fn render_markdown_preview(&self, template: &CommentNodeTemplate, markdown: &je_base::html_content::IMarkdownString) {
        let environment = &self.services.markdown_environment;
        let rendered = render_markdown(markdown, &MarkdownRenderOptions::default(), &environment.borrowed()).unwrap_or_else(|error| errors::throw(Error::new(error.to_string())));
        {
            let mut element = rendered.element.borrow_mut();
            clean_rendered_comment(&mut element);
            keep_first_element_child(&mut element);
        }
        let text_content = rendered.element.borrow().text_content().to_string_lossy();
        template
            .preview_natural_width
            .set(measure_text(&text_content, &ui_font(self.services.styles.label.font_size, self.services.styles.label.font_weight)));

        let styles = MarkdownViewStyles { line_height: LINE_HEIGHT, ..self.services.styles.markdown.clone() };
        let view = template.element_disposables.add_rc(MarkdownView::from_rendered(&self.services.ui, rendered, environment.clone(), &styles));
        self.services.ui.tree_mut().append_child(template.comment_preview, view.root());
        *template.markdown_view.borrow_mut() = Some(view);

        let hover = setup_mouse_hover(template.comment_preview, &text_content);
        template.element_disposables.add(to_disposable(move || hover.dispose()));
    }

    fn render_comment(&self, template: &CommentNodeTemplate, element: &Rc<CommentNode>) {
        let ui = &self.services.ui;
        template.action_bar.clear();
        template.element_disposables.clear();
        *template.markdown_view.borrow_mut() = None;

        let comment_count = element.replies.len() + 1;
        let outdated = element.thread_relevance == Some(CommentThreadApplicability::Outdated);
        template.outdated.set(outdated);
        let relevance_text = if outdated { localize("outdated", "Outdated", &[]) } else { String::new() };
        template.relevance_label.set(Some(&relevance_text), &[], "", false, false);

        // Check if any comment in the thread has draft state
        let has_draft = element.thread.comments().is_some_and(|comments| comments.iter().any(|comment| comment.state == Some(CommentState::Draft)));
        let icon_id = Self::get_icon(element.thread_state, has_draft);
        let color_theme = self.services.theme_service.get_color_theme();
        let icon_color = get_comment_thread_state_icon_color(element.thread_state, &*color_theme).as_ref().map_or(self.services.styles.label.foreground, to_rgba8);
        {
            let mut tree = ui.tree_mut();
            tree.set_class_name(template.icon, &format!("icon {}", codicon(icon_id).as_class_name()));
            tree.style_mut(template.icon).content = Content::Codicon { id: icon_id.to_owned(), size: CODICON_SIZE as f32, color: icon_color };
        }

        template.user_names.set(Some(&element.comment.user_name), &[], "", false, false);
        template.timestamp.set_timestamp(comment_date(&element.comment));

        template.preview_natural_width.set(0.0);
        match &element.comment.body {
            CommentBody::Text(body) => template.preview_label.set(Some(&body.replace(['\r', '\n'], "")), &[], "", false, false),
            CommentBody::Markdown(markdown) => {
                template.preview_label.set(Some(""), &[], "", false, false);
                self.render_markdown_preview(template, markdown);
            }
        }

        if let Some(range) = &element.range {
            template.range.set(Some(&Self::range_text(range)), &[], "", false, false);
        }

        let menu_actions = self.menus.get_resource_actions(element);
        template.action_bar.push(
            &menu_actions,
            IActionOptions {
                item: IActionViewItemOptions { icon: Some(true), label: Some(false), styles: self.services.styles.action_bar.clone(), ..IActionViewItemOptions::default() },
                index: None,
            },
        );
        template.action_bar.layout();
        template.action_bar.set_context(Some(marshalled_comment_thread(element, false)));

        template.has_reply.set(element.has_reply());
        if element.has_reply() {
            template.count.set(Some(&Self::get_count_string(comment_count)), &[], "", false, false);
            let last_comment = &element.replies[element.replies.len() - 1].comment;
            template.last_reply_detail.set(Some(&localize("lastReplyFrom", "Last reply from {0}", &[&last_comment.user_name])), &[], "", false, false);
            template.replies_timestamp.set_timestamp(comment_date(last_comment));
        }
        template.layout();
    }

    /// `[Ln {0}]` ou `[Ln {0}-{1}]`.
    fn range_text(range: &Range) -> String {
        if range.start_line_number == range.end_line_number {
            localize_args("commentLine", "[Ln {0}]", &[NlsArg::Number(f64::from(range.start_line_number))])
        } else {
            localize_args("commentRange", "[Ln {0}-{1}]", &[NlsArg::Number(f64::from(range.start_line_number)), NlsArg::Number(f64::from(range.end_line_number))])
        }
    }
}

impl ITreeRenderer<CommentsTreeModel> for CommentNodeRenderer {
    fn template_id(&self) -> String {
        CommentsModelVirtualDelegate::COMMENT_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let ui = self.services.ui.clone();
        let styles = &self.services.styles;
        let text = styles.text(1.0);
        let small = styles.text(SMALL_FONT_SCALE);
        let configuration_service = &self.services.configuration_service;
        let foreground = styles.label.foreground;

        let thread_container = create_el(&mut ui.tree_mut(), container, Role::Generic, "comment-thread-container");
        let metadata_container = create_el(&mut ui.tree_mut(), thread_container, Role::Generic, "comment-metadata-container");
        ui.tree_mut().style_mut(metadata_container).clip = true;
        let metadata = create_el(&mut ui.tree_mut(), metadata_container, Role::Generic, "comment-metadata");

        let icon = create_el(&mut ui.tree_mut(), metadata, Role::Image, "icon");
        let user_names = create_label(&ui, metadata, "user", text, 1.0);
        let timestamp_container = create_el(&mut ui.tree_mut(), metadata, Role::Generic, "timestamp-container");
        let timestamp = TimestampWidget::new(&ui, configuration_service.clone(), timestamp_container, text, None);
        ui.tree_mut().style_mut(timestamp.element()).opacity = DIMMED_OPACITY;

        let relevance = create_el(&mut ui.tree_mut(), metadata, Role::Generic, "relevance");
        {
            let mut tree = ui.tree_mut();
            let style = tree.style_mut(relevance);
            style.background = styles.badge_background;
            style.radius = RELEVANCE_RADIUS;
        }
        let relevance_label = HighlightedLabel::new(&ui, relevance, None, styles.relevance());
        let separator = create_label(&ui, metadata, "separator", text, DIMMED_OPACITY);
        separator.set(Some(SEPARATOR), &[], "", false, false);
        let comment_preview = create_el(&mut ui.tree_mut(), metadata, Role::Generic, "text");
        ui.tree_mut().style_mut(comment_preview).clip = true;
        let preview_label = HighlightedLabel::new(&ui, comment_preview, None, text);
        let range = create_label(&ui, metadata, "range", text, DIMMED_OPACITY);

        let actions = create_el(&mut ui.tree_mut(), metadata_container, Role::Generic, "actions");
        set_visible(&ui, actions, false);
        let action_bar = ActionBar::new(
            &ui,
            actions,
            IActionBarOptions { action_view_item_provider: Some(self.services.action_view_item_provider.clone()), styles: styles.action_bar.clone(), ..IActionBarOptions::default() },
        );

        let snippet_container = create_el(&mut ui.tree_mut(), thread_container, Role::Generic, "comment-snippet-container");
        ui.tree_mut().style_mut(snippet_container).clip = true;
        let replies_icon = create_el(&mut ui.tree_mut(), snippet_container, Role::Image, "icon");
        {
            let mut tree = ui.tree_mut();
            tree.set_class_name(replies_icon, &format!("icon {}", codicon("indent").as_class_name()));
            tree.style_mut(replies_icon).content = Content::Codicon { id: "indent".to_owned(), size: CODICON_SIZE as f32, color: foreground };
        }
        let count = create_label(&ui, snippet_container, "count", text, 1.0);
        let last_reply_detail = create_label(&ui, snippet_container, "reply-detail", small, DIMMED_OPACITY);
        let replies_separator = create_label(&ui, snippet_container, "separator", text, DIMMED_OPACITY);
        replies_separator.set(Some(SEPARATOR), &[], "", false, false);
        let replies_timestamp_container = create_el(&mut ui.tree_mut(), snippet_container, Role::Generic, "timestamp-container");
        let replies_timestamp = TimestampWidget::new(&ui, configuration_service.clone(), replies_timestamp_container, small, None);
        ui.tree_mut().style_mut(replies_timestamp.element()).opacity = DIMMED_OPACITY;

        let disposables = DisposableStore::new();
        disposables.add(timestamp.clone());
        disposables.add(replies_timestamp.clone());
        for label in [&user_names, &relevance_label, &separator, &preview_label, &range, &count, &last_reply_detail, &replies_separator] {
            disposables.add(label.clone());
        }

        let template = Rc::new_cyclic(|this| CommentNodeTemplate {
            this: this.clone(),
            ui,
            container,
            thread_container,
            metadata_container,
            metadata,
            icon,
            user_names,
            timestamp_container,
            timestamp,
            relevance,
            relevance_label,
            separator,
            comment_preview,
            preview_label,
            range,
            actions,
            action_bar,
            snippet_container,
            replies_icon,
            count,
            last_reply_detail,
            replies_separator,
            replies_timestamp_container,
            replies_timestamp,
            disposables,
            element_disposables: DisposableStore::new(),
            markdown_view: RefCell::new(None),
            preview_natural_width: Cell::new(0.0),
            has_reply: Cell::new(false),
            outdated: Cell::new(false),
            hovered: Cell::new(false),
        });
        template.attach_listeners();
        Rc::new(template)
    }

    fn render_element(&self, node: &Rc<CommentsTreeNode>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        if let Some(CommentsElement::Comment(element)) = node.element() {
            self.render_comment(Self::template(template_data), &element);
        }
    }

    fn dispose_element(&self, _node: &Rc<CommentsTreeNode>, _index: i64, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let template = Self::template(template_data);
        template.element_disposables.clear();
        *template.markdown_view.borrow_mut() = None;
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        let template = Self::template(template_data);
        template.disposables.dispose();
        template.element_disposables.dispose();
        template.action_bar.dispose();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Filter
// ---------------------------------------------------------------------------------------------------------------------

/// `FilterData`: `ResourceFilterData | CommentFilterData`.
#[derive(Clone, Debug)]
pub enum FilterData {
    Resource { uri_matches: Vec<Match> },
    Comment { text_matches: Vec<Match> },
}

/// `Filter`.
pub struct Filter {
    options: RefCell<Rc<FilterOptions>>,
}

impl Filter {
    pub fn new(options: Rc<FilterOptions>) -> Self {
        Self { options: RefCell::new(options) }
    }

    /// `public options`.
    pub fn options(&self) -> Rc<FilterOptions> {
        self.options.borrow().clone()
    }

    pub fn set_options(&self, options: Rc<FilterOptions>) {
        *self.options.borrow_mut() = options;
    }

    fn filter_resource_markers(options: &FilterOptions, resource_markers: &ResourceWithCommentThreads) -> TreeFilterResult<FilterData> {
        // Filter by text. Do not apply negated filters on resources instead use exclude patterns
        if !options.text_filter.text.is_empty() && !options.text_filter.negate {
            let uri_matches = (FilterOptions::FILTER)(&units(&options.text_filter.text), &units(&ext_uri().basename(&resource_markers.resource, None)));
            if let Some(uri_matches) = uri_matches {
                return TreeFilterResult::Data { visibility: VisibilityValue::Bool(true), data: FilterData::Resource { uri_matches } };
            }
        }

        TreeFilterResult::Visibility(TreeVisibility::Recurse)
    }

    fn filter_comment_node(options: &FilterOptions, comment: &CommentNode, parent_visibility: TreeVisibility) -> TreeFilterResult<FilterData> {
        let matches_resolved_state = comment.thread_state.is_none()
            || (options.show_resolved && comment.thread_state == Some(CommentThreadState::Resolved))
            || (options.show_unresolved && comment.thread_state == Some(CommentThreadState::Unresolved));

        if !matches_resolved_state {
            return TreeFilterResult::Bool(false);
        }

        if options.text_filter.text.is_empty() {
            return TreeFilterResult::Bool(true);
        }

        let text = units(&options.text_filter.text);
        let message_filter = |value: &str| (FilterOptions::MESSAGE_FILTER)(&text, &units(value));
        let text_matches =
            // Check body of comment for value
            message_filter(&comment_body_text(&comment.comment))
                // Check first user for value
                .or_else(|| message_filter(&comment.comment.user_name))
                // Check all replies for value
                .unwrap_or_else(|| {
                    comment
                        .replies
                        .iter()
                        .filter_map(|reply| {
                            // Check user for value
                            message_filter(&reply.comment.user_name)
                                // Check body of reply for value
                                .or_else(|| message_filter(&comment_body_text(&reply.comment)))
                        })
                        .flatten()
                        .collect()
                });

        text_filter_result(&options.text_filter, !text_matches.is_empty(), parent_visibility, || FilterData::Comment { text_matches })
    }
}

impl ITreeFilter<CommentsElement, FilterData> for Filter {
    fn filter(&self, element: &CommentsElement, parent_visibility: TreeVisibility) -> TreeFilterResult<FilterData> {
        let options = self.options();
        if options.filter.is_empty() && options.show_resolved && options.show_unresolved {
            return TreeFilterResult::Visibility(TreeVisibility::Visible);
        }

        match element {
            CommentsElement::Resource(resource) => Self::filter_resource_markers(&options, resource),
            CommentsElement::Comment(comment) => Self::filter_comment_node(&options, comment, parent_visibility),
            // a raiz nunca passa pelo filtro da árvore
            CommentsElement::Model(_) => TreeFilterResult::Visibility(TreeVisibility::Visible),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Adaptadores de `Option` e identidade
// ---------------------------------------------------------------------------------------------------------------------

/// O filtro do usuário sobre o elemento da árvore de objetos (`T | null`): a raiz nunca é filtrada.
struct OptionElementFilter(Rc<dyn ITreeFilter<CommentsElement, FilterData>>);

impl ITreeFilter<Option<CommentsElement>, FilterData> for OptionElementFilter {
    fn filter(&self, element: &Option<CommentsElement>, parent_visibility: TreeVisibility) -> TreeFilterResult<FilterData> {
        match element {
            Some(element) => self.0.filter(element, parent_visibility),
            None => TreeFilterResult::Bool(true),
        }
    }
}

/// O provedor de acessibilidade do usuário sobre o elemento da árvore de objetos: a raiz (`null`) não tem rótulo.
struct OptionAccessibilityProvider(Rc<dyn IListAccessibilityProvider<CommentsElement>>);

impl IListAccessibilityProvider<Option<CommentsElement>> for OptionAccessibilityProvider {
    fn get_aria_label(&self, element: &Option<CommentsElement>) -> Option<String> {
        element.as_ref().and_then(|element| self.0.get_aria_label(element))
    }

    fn get_widget_aria_label(&self) -> String {
        self.0.get_widget_aria_label()
    }

    fn get_widget_role(&self) -> Option<Role> {
        self.0.get_widget_role()
    }

    fn get_role(&self, element: &Option<CommentsElement>) -> Option<Role> {
        element.as_ref().and_then(|element| self.0.get_role(element))
    }
}

/// O `identityProvider` de `CommentsList`.
struct CommentsIdentityProvider;

impl IIdentityProvider<Option<CommentsElement>> for CommentsIdentityProvider {
    fn get_id(&self, element: &Option<CommentsElement>) -> String {
        match element {
            Some(CommentsElement::Model(_)) => "root".to_owned(),
            Some(CommentsElement::Resource(resource)) => format!("{}-{}", resource.unique_owner, resource.id),
            Some(CommentsElement::Comment(comment)) => format!(
                "{}-{}-{}-{}{}",
                comment.unique_owner,
                comment.resource.to_string(false).to_string_lossy(),
                comment.thread_id,
                comment.comment.unique_id_in_thread,
                if comment.is_root { "-root" } else { "" }
            ),
            None => String::new(),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// CommentsList
// ---------------------------------------------------------------------------------------------------------------------

/// O delegado do menu de contexto de um comentário.
struct CommentsContextMenuDelegate {
    anchor: AnchorTarget,
    actions: Vec<ActionRef>,
    tree: Weak<WorkbenchObjectTree<CommentsElement, FilterData>>,
    extras: ContextMenuExtras,
}

impl IContextMenuDelegate for CommentsContextMenuDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        self.anchor.clone()
    }

    fn get_actions(&self) -> Vec<ActionRef> {
        self.actions.clone()
    }

    fn on_hide(&self, was_cancelled: bool) {
        if was_cancelled && let Some(tree) = self.tree.upgrade() {
            tree.dom_focus();
        }
    }

    fn native_extras(&self) -> Option<Rc<dyn std::any::Any>> {
        native_extras(self.extras.clone())
    }
}

/// `CommentsList`.
pub struct CommentsList {
    base: Rc<Disposable>,
    ui: Ui,
    tree: Rc<WorkbenchObjectTree<CommentsElement, FilterData>>,
    menus: Rc<CommentsMenus>,
    context_menu_service: Rc<dyn IContextMenuService>,
    keybinding_service: Rc<dyn IKeybindingService>,
}

impl CommentsList {
    pub fn new(labels: Rc<ResourceLabels>, container: NodeId, options: CommentsListOptions, services: &CommentsListServices) -> Rc<Self> {
        let ui = services.renderers.ui.clone();
        let renderer_services = Rc::new(services.renderers.clone());

        let menus = CommentsMenus::new(services.menu_service.clone());
        menus.set_context_key_service(services.tree.context_key_service.clone());
        let resource_renderer: Rc<dyn ITreeRenderer<CommentsTreeModel>> = Rc::new(ResourceWithCommentsRenderer::new(labels, renderer_services.clone()));
        let comment_renderer: Rc<dyn ITreeRenderer<CommentsTreeModel>> = Rc::new(CommentNodeRenderer::new(renderer_services, menus.clone()));
        let renderers = vec![resource_renderer, comment_renderer];

        let tree = WorkbenchObjectTree::<CommentsElement, FilterData>::new(
            "CommentsTree",
            &ui,
            container,
            Rc::new(CommentsModelVirtualDelegate),
            renderers,
            ObjectTreeOptions {
                tree: AbstractTreeOptions {
                    accessibility_provider: options.accessibility_provider.map(|provider| Rc::new(OptionAccessibilityProvider(provider)) as Rc<dyn IListAccessibilityProvider<Option<CommentsElement>>>),
                    identity_provider: Some(Rc::new(CommentsIdentityProvider)),
                    collapse_by_default: false,
                    filter: options.filter.map(|filter| Rc::new(OptionElementFilter(filter)) as Rc<dyn ITreeFilter<Option<CommentsElement>, FilterData>>),
                    find_widget_enabled: Some(false),
                    update: AbstractTreeOptionsUpdate {
                        expand_only_on_twistie_click: Some(ExpandOnlyOnTwistieClick::Always(true)),
                        multiple_selection_support: Some(false),
                        ..AbstractTreeOptionsUpdate::default()
                    },
                    ..AbstractTreeOptions::default()
                },
                sorter: options.sorter,
            },
            IWorkbenchTreeOptions { override_styles: options.override_styles, ..IWorkbenchTreeOptions::default() },
            &services.tree,
        );

        let base = Disposable::new();
        base.register(menus.clone());
        let this = Rc::new(Self { base, ui, tree, menus, context_menu_service: services.context_menu_service.clone(), keybinding_service: services.keybinding_service.clone() });

        let weak = Rc::downgrade(&this);
        this.base.register(this.tree.on_context_menu().listen(move |tree_event| {
            if let Some(this) = weak.upgrade() {
                this.comments_on_context_menu(tree_event);
            }
        }));
        this
    }

    fn comments_on_context_menu(&self, tree_event: &TreeMouseEvent<Option<CommentsElement>>) {
        let Some(Some(CommentsElement::Comment(node))) = tree_event.element.clone() else { return };

        self.tree.set_focus(&[Some(CommentsElement::Comment(node.clone()))], None);
        let actions = self.menus.get_resource_context_actions(&node);
        if actions.is_empty() {
            return;
        }

        let anchor_node = tree_event.browser_event.target.unwrap_or_else(|| self.tree.get_html_element());
        let action_view_item_provider: IActionViewItemProvider = {
            let (ui, keybinding_service) = (self.ui.clone(), self.keybinding_service.clone());
            Rc::new(move |action: &ActionRef, _options: &IActionViewItemOptions| {
                let keybinding = keybinding_service.lookup_keybinding(&action.id(), None, false)?;
                let options = IActionViewItemOptions { label: Some(true), keybinding: keybinding.get_label(), ..IActionViewItemOptions::default() };
                Some(ActionViewItem::new(&ui, None, action.clone(), options) as Rc<dyn IActionViewItem>)
            })
        };
        let get_actions_context: Rc<dyn Fn() -> Option<Dynamic>> = {
            let node = node.clone();
            Rc::new(move || Some(marshalled_comment_thread(&node, true)))
        };

        self.context_menu_service.show_context_menu(ContextMenuRequest::Delegate(Rc::new(CommentsContextMenuDelegate {
            anchor: AnchorTarget::Element(ViewElement::from_native(Rc::new(anchor_node))),
            actions,
            tree: Rc::downgrade(&self.tree),
            extras: ContextMenuExtras { get_action_view_item: Some(action_view_item_provider), get_actions_context: Some(get_actions_context), get_key_binding: None },
        })));
    }

    /// `filterComments()`.
    pub fn filter_comments(&self) {
        self.tree.refilter();
    }

    /// `getVisibleItemCount()`.
    pub fn get_visible_item_count(&self) -> usize {
        let mut filtered = 0;
        let root = self.tree.get_root_node();

        for resource_node in root.children() {
            for comment_node in resource_node.children() {
                if comment_node.visible() && resource_node.visible() {
                    filtered += 1;
                }
            }
        }

        filtered
    }
}

impl Deref for CommentsList {
    type Target = WorkbenchObjectTree<CommentsElement, FilterData>;

    fn deref(&self) -> &Self::Target {
        &self.tree
    }
}

impl IDisposable for CommentsList {
    fn dispose(&self) {
        self.base.dispose();
        self.tree.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "CommentsList"
    }
}
