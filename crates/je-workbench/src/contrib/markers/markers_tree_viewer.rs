//! Porte de `src/vs/workbench/contrib/markers/browser/markersTreeViewer.ts`: o provedor de acessibilidade, o delegado
//! de altura, os renderers (recurso, marcador e informação relacionada), o filtro e os modelos de visão
//! (`MarkerViewModel`, `MarkersViewModel`) da árvore do painel de problemas.
//!
//! O arquivo do original não traz fonte de dados nem a classe da árvore: isso mora em `markersView.ts` (a próxima
//! fatia). Aqui ficam os tipos [`MarkersTreeModel`] e [`MarkersTreeNode`], o modelo e o nó da `WorkbenchObjectTree`
//! que a visão cria com estes renderers.
//!
//! Notas de tradução:
//! - **Injeção.** Os parâmetros decorados dos construtores (`@ILabelService`, `@IOpenerService`, `@IHoverService`,
//!   `@IInstantiationService`, `@IModelService`, `@IEditorService`, `@ILanguageFeaturesService`, `@IContextKeyService`) são
//!   as structs [`MarkersTreeServices`] (renderers e desenhistas) e [`MarkerViewModelServices`] (modelos de visão). O
//!   `IHoverService.setupManagedHover` é o hover de base de `je-widgets` (o serviço de hover do porte é global).
//! - **DOM e CSS.** Os renderers montam nós do `je-ui` com as mesmas classes (`resource-label-container`,
//!   `count-badge-wrapper`, `marker-icon`, `marker-message-details-container`, `marker-message-line`, `marker-message`,
//!   `marker-source`, `marker-code`, `marker-line`, `multiline-actions`, `related-info-resource`...). As regras de
//!   `markers.css` (`display: flex`, `flex: 1`, margens de 6px, opacidade 0,7, `overflow: hidden` com reticências, ações
//!   escondidas até o ponteiro, a seleção ou o foco) viram o `layout` de cada desenhista, que mede os nós e os posiciona; o
//!   `layout` roda ao desenhar o elemento e quando o ponteiro entra ou sai. O `.markers-panel.wide` é a classe `wide` de um
//!   ancestral do nó. Divergências: o `text-decoration: underline` do código com link não existe no [`HighlightedLabel`], e
//!   o estado `selected` e `focused` da linha só é relido no `layout` (a seleção não dispara um novo).
//! - **`FileResourceMarkersRenderer extends ResourceMarkersRenderer {}`** (subclasse vazia) é o alias
//!   [`FileResourceMarkersRenderer`].
//! - **`ToggleMultilineActionViewItem extends ActionViewItem`** é composição ([`ActionViewItem::new_with_owner`], como o
//!   `QuickFixActionViewItem`): acrescenta o atributo `aria-expanded` ao elemento do item (`Tree::set_attribute`) no
//!   `render` e no `updateClass`, com `icon: true` como o original.
//! - **`Link` (`platform/opener/browser/link.ts`).** O porte ainda não tem o widget; o código com `target` desenha o `a.monaco-link`
//!   ([`Role::Link`]) com o rótulo dentro, a dica com o endereço e a abertura (clique, `Enter` ou `Space`) por
//!   `IOpenerService.open(href, { allowCommands: true })`, o que o `Link` faz.
//! - **Ícones registrados.** Os ícones `markers-view-multi-line-expanded` e `markers-view-multi-line-collapsed` são
//!   registrados no registro de ícones (`registerIcon` de `je_theme::icon_registry`, com `chevron-up` e `chevron-down` como
//!   padrão). As constantes de módulo do original são preguiçosas: o registro roda no primeiro uso, não no carregamento.
//! - **`IListAccessibilityProvider<MarkerElement | MarkerTableItem>`.** O provedor implementa o trait para os dois elementos
//!   (a árvore e a tabela); o `MarkerTableItem` é um `Marker` por composição (`base`).
//! - **`ITreeFilter`.** `Filter.options` (público e mutável) é um `RefCell<Rc<FilterOptions>>` com [`Filter::options`] e
//!   [`Filter::set_options`]. O resultado é o [`TreeFilterResult`]: `false` e `true` são `Bool`, `TreeVisibility` é
//!   `Visibility` e `{ visibility, data }` é `Data`. `FilterData` é um enum, que junta os três `interface` do original.
//!   Os valores "falsos" do JavaScript (`source` e `code` de texto vazio) são tratados como ausentes, como o original.
//! - **Elementos da árvore.** `MarkerElement` compara por identidade (ver `markers_model`), o `Map<ResourceMarkers, ...>` do
//!   `ResourceMarkersRenderer` é um mapa pelo ponteiro do `Rc`.
//! - **Promessas do `MarkerViewModel`.** `CancelablePromise<ITextModel>` e `CancelablePromise<CodeActionSet>` são
//!   [`CancelablePromise`] de `Rc<TextModel>` e de `Option<Rc<dyn CodeActionSet>>`; `this._register(actions)` registra o
//!   descarte do conjunto no `Disposable` do modelo de visão. `applyCodeAction` recebe os serviços de
//!   [`ApplyCodeActionServices`] e o token vazio.
//! - **`Delayer<void>(300)`** é o [`Delayer`] com `Delay::Millis(300)`; a promessa devolvida pelo `trigger` (que rejeita
//!   quando o disparo é substituído) tem o erro engolido.
//! - **`IContextKey<MarkersViewMode>`** guarda o texto do modo (`"tree"`, `"table"`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::actions::{Action, ActionRef, IAction, ToActionProps, to_action};
use je_base::async_::{Delayer, async_fn, create_cancelable_promise};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::filters::Match;
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, to_disposable};
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::{CancelablePromise, MaybePromise, Promise};
use je_base::symbols::Delay;
use je_base::themables::ThemeIcon;
use je_base::{JsString, nls::localize};
use je_editor::code_action::code_action::{ApplyCodeActionReason, ApplyCodeActionServices, apply_code_action, get_code_actions};
use je_editor::code_action::languages::CodeActionTriggerType;
use je_editor::code_action::platform::ActionSet;
use je_editor::code_action::types::{CodeActionFilter, CodeActionKind, CodeActionSet, CodeActionTrigger, CodeActionTriggerSource};
use je_editor::code_editor_widget::SelectionOrRange;
use je_editor::language_features_service::LanguageFeaturesService;
use je_editor::services_model::IModelService;
use je_editor::text_model::TextModel;
use je_platform::contextkey::{IContextKey, IContextKeyService};
use je_platform::contextview::context_view::IContextMenuService;
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::markers::marker_service::unsupported_schemas;
use je_platform::markers::markers::{MarkerCode, MarkerSeverity};
use je_platform::opener::editor_types::{EditorOptions, TextEditorSelection};
use je_platform::opener::opener::{AllowCommands, IOpenerService, OpenOptions, OpenResource};
use je_platform::progress::progress::Progress;
use je_platform::resources::ext_uri;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Content, EventKind, NodeId, Role, Tree, Ui};
use je_widgets::action_bar::{ActionBar, IActionBarOptions, IActionOptions, IActionViewItemProvider};
use je_theme::icon_registry::register_icon;
use je_widgets::action_view_items::{ActionBarStyles, ActionViewItem, BaseActionViewItem, IActionViewItem, IActionViewItemOptions, ItemSize};
use je_widgets::count_badge::{CountBadge, ICountBadgeOptions, ICountBadgeStyles};
use je_widgets::dom::{StandardKeyboardEvent, clear_node, create_el};
use je_widgets::highlighted_label::{HighlightedLabel, HighlightedLabelStyles, IHighlight};
use je_widgets::hover_api::{HoverPlacement, IManagedHover, ManagedHoverContentOrFactory, get_base_layer_hover_delegate, get_default_hover_delegate};
use je_widgets::icon_label::{IIconLabelCreationOptions, IIconLabelValueOptions};
use je_widgets::list::list_types::{IListAccessibilityProvider, IListVirtualDelegate, TemplateData};
use je_widgets::severity_icon::{SeverityIcon, SeverityIconStyles};
use je_widgets::text::{measure_text, ui_font};
use je_widgets::tree::object_tree_model::ObjectTreeModel;
use je_widgets::tree::tree_types::{ITreeFilter, ITreeModel, ITreeRenderer, TreeElementRenderDetails, TreeFilterResult, TreeNodeApi, TreeVisibility, VisibilityValue};

use crate::browser::labels::{IFileLabelOptions, IResourceLabel, IResourceLabelOptions, ResourceLabels};
use crate::common::editor::{TextEditorOptions, UntypedEditorInput};
use crate::services::editor::editor_service::IEditorService;

use super::markers_common::{MarkersViewMode, markers_view_mode_context_key};
use super::markers_filter_options::FilterOptions;
use super::markers_model::{Marker, MarkerElement, MarkerTableItem, RelatedInformation, ResourceMarkers};
use super::markers_view_actions::{QuickFixAction, QuickFixActionViewItem};
use super::messages::Messages;

/// O modelo da árvore do painel (`WorkbenchObjectTree<MarkerElement, FilterData>`).
pub type MarkersTreeModel = ObjectTreeModel<MarkerElement, FilterData>;

/// O nó que os renderers recebem.
pub type MarkersTreeNode = <MarkersTreeModel as ITreeModel>::Node;

/// A altura de uma linha (`line-height: 22px` de `.monaco-tl-contents`).
const LINE_HEIGHT: f64 = 22.0;

/// `padding-right: 10px` de `.monaco-tl-contents`.
const CONTENTS_PADDING_RIGHT: f64 = 10.0;

/// `margin-left: 6px` de `.marker-source`, `.marker-line` e `.multiline-actions`.
const DETAILS_MARGIN_LEFT: f64 = 6.0;

/// `margin: 0 4px` de `.actions .action-item`.
const ACTION_ITEM_MARGIN: f64 = 4.0;

/// `margin: 0 6px` do `.marker-icon` mais o codicon de 16px.
const MARKER_ICON_WIDTH: f64 = 28.0;

const CODICON_SIZE: f64 = 16.0;

/// `.count-badge-wrapper` com `.wide`: `margin-left: 10px`.
const WIDE_BADGE_MARGIN_LEFT: f64 = 10.0;

thread_local! {
    static EXPANDED_ICON: ThemeIcon = register_icon(
        "markers-view-multi-line-expanded",
        ThemeIcon::from_id("chevron-up"),
        &localize("expandedIcon", "Icon indicating that multiple lines are shown in the markers view.", &[]),
        None,
    );
    static COLLAPSED_ICON: ThemeIcon = register_icon(
        "markers-view-multi-line-collapsed",
        ThemeIcon::from_id("chevron-down"),
        &localize("collapsedIcon", "Icon indicating that multiple lines are collapsed in the markers view.", &[]),
        None,
    );
}

/// `expandedIcon`.
fn expanded_icon() -> ThemeIcon {
    EXPANDED_ICON.with(Clone::clone)
}

/// `collapsedIcon`.
fn collapsed_icon() -> ThemeIcon {
    COLLAPSED_ICON.with(Clone::clone)
}

const TOGGLE_MULTILINE_ACTION: &str = "problems.action.toggleMultiline";

/// `ToggleMultilineActionViewItem extends ActionViewItem`: acrescenta o atributo `aria-expanded` ao elemento do item,
/// verdadeiro quando a classe da ação é a do ícone expandido.
struct ToggleMultilineActionViewItem {
    inner: Rc<ActionViewItem>,
    ui: Ui,
    disposed: Cell<bool>,
}

impl ToggleMultilineActionViewItem {
    /// `new ToggleMultilineActionViewItem(undefined, action, options)`.
    fn new(ui: &Ui, action: ActionRef, options: IActionViewItemOptions) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| Self {
            inner: ActionViewItem::new_with_owner(ui, None, action, options, weak.clone() as Weak<dyn IActionViewItem>),
            ui: ui.clone(),
            disposed: Cell::new(false),
        })
    }

    /// `updateExpandedAttribute()`.
    fn update_expanded_attribute(&self) {
        let Some(element) = self.inner.base().element() else { return };
        let expanded = self.inner.base().action().class().as_deref() == Some(expanded_icon().as_class_name().as_str());
        self.ui.tree_mut().set_attribute(element, "aria-expanded", expanded.to_string());
    }
}

impl IActionViewItem for ToggleMultilineActionViewItem {
    fn base(&self) -> &BaseActionViewItem {
        self.inner.base()
    }

    fn render(&self, container: NodeId) {
        self.inner.render(container);
        self.update_expanded_attribute();
    }

    fn focus(&self, from_right: bool) {
        self.inner.focus(from_right);
    }

    fn is_focused(&self) -> bool {
        self.inner.is_focused()
    }

    fn blur(&self) {
        self.inner.blur();
    }

    fn set_focusable(&self, focusable: bool) {
        self.inner.set_focusable(focusable);
    }

    fn tab_stops(&self) -> usize {
        self.inner.tab_stops()
    }

    fn update_enabled(&self) {
        self.inner.update_enabled();
    }

    fn update_label(&self) {
        self.inner.update_label();
    }

    fn update_class(&self) {
        self.inner.update_class();
        self.update_expanded_attribute();
    }

    fn update_checked(&self) {
        self.inner.update_checked();
    }

    fn get_tooltip(&self) -> Option<String> {
        self.inner.get_tooltip()
    }

    fn update_aria_label(&self) {
        self.inner.update_aria_label();
    }

    fn size(&self, vertical: bool) -> ItemSize {
        self.inner.size(vertical)
    }

    fn layout(&self, vertical: bool, width: f64, height: f64) {
        self.inner.layout(vertical, width, height);
    }
}

impl IDisposable for ToggleMultilineActionViewItem {
    fn dispose(&self) {
        if !self.disposed.replace(true) {
            self.inner.dispose();
        }
    }

    fn constructor_name(&self) -> &'static str {
        "ToggleMultilineActionViewItem"
    }
}

/// Os estilos que o original deixa para a folha de estilo e para os `defaultXStyles`.
#[derive(Clone)]
pub struct MarkersTreeStyles {
    /// Cor, fonte e realce dos rótulos da árvore (`highlight_weight` e `line_height` são ajustados aqui).
    pub label: HighlightedLabelStyles,
    /// `defaultCountBadgeStyles`.
    pub count_badge: ICountBadgeStyles,
    /// As cores dos ícones de severidade (`problemsErrorIcon.foreground` e irmãs).
    pub severity_icon: SeverityIconStyles,
    /// `textLink.foreground`: a cor do código com link.
    pub text_link_foreground: Option<Rgba>,
    /// As cores das barras de ações.
    pub action_bar: Option<Rc<ActionBarStyles>>,
}

impl MarkersTreeStyles {
    /// O rótulo da árvore: `.highlight { font-weight: bold }` e `line-height: 22px` de `markers.css`.
    fn label_styles(&self) -> HighlightedLabelStyles {
        HighlightedLabelStyles { highlight_weight: 700, line_height: Some(LINE_HEIGHT), ..self.label }
    }
}

/// Os serviços que os renderers e os desenhistas leem.
#[derive(Clone)]
pub struct MarkersTreeServices {
    pub ui: Ui,
    pub label_service: Rc<dyn ILabelService>,
    pub opener_service: Rc<dyn IOpenerService>,
    pub context_menu_service: Rc<dyn IContextMenuService>,
    pub styles: MarkersTreeStyles,
}

/// Os serviços do [`MarkerViewModel`].
#[derive(Clone)]
pub struct MarkerViewModelServices {
    pub model_service: Rc<dyn IModelService>,
    pub editor_service: Rc<dyn IEditorService>,
    pub language_features_service: LanguageFeaturesService,
    pub apply_code_action_services: ApplyCodeActionServices,
}

// ---------------------------------------------------------------------------------------------------------------------
// Auxiliares de texto e de nós
// ---------------------------------------------------------------------------------------------------------------------

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// Os `IMatch[]` como os `IHighlight[]` do rótulo.
fn highlights(matches: &[Match]) -> Vec<IHighlight> {
    matches.iter().map(|m| IHighlight::new(m.start, m.end)).collect()
}

/// O módulo `path` do original (o da plataforma).
fn native_path() -> PathModule<'static> {
    PathModule::native(&ProcessEnvironment)
}

fn place(tree: &mut Tree, node: NodeId, x: f64, y: f64, width: f64, height: f64) {
    tree.set_bounds(node, Rect::new(x, y, x + width, y + height));
}

/// Um nó de texto de uma linha (os `(`, `)` e `:` e o `[Ln, Col]`); devolve o nó e a largura do texto.
fn text_node(ui: &Ui, parent: NodeId, class_name: &str, text: &str, styles: &HighlightedLabelStyles, color: Rgba) -> (NodeId, f64) {
    let font = ui_font(styles.font_size, styles.font_weight);
    let width = measure_text(text, &font);
    let mut tree = ui.tree_mut();
    let node = create_el(&mut tree, parent, Role::Generic, class_name);
    tree.style_mut(node).content = Content::Text { text: text.to_owned(), style: font, color, line_height: LINE_HEIGHT };
    (node, width)
}

/// O nó, ou um antepassado dele, tem a classe? (As classes de estado vivem em ancestrais: `wide`, `monaco-list-row`.)
fn ancestor_has_class(tree: &Tree, node: NodeId, class_name: &str) -> bool {
    let mut current = Some(node);
    while let Some(id) = current {
        if tree.has_class(id, class_name) {
            return true;
        }
        current = tree.parent(id);
    }
    false
}

/// `.monaco-list-row:hover`, `.selected` e `.focused`: a linha que contém o nó está selecionada ou com foco?
fn row_selected_or_focused(tree: &Tree, node: NodeId) -> bool {
    let mut current = Some(node);
    while let Some(id) = current {
        if tree.has_class(id, "monaco-list-row") {
            return tree.has_class(id, "selected") || tree.has_class(id, "focused");
        }
        current = tree.parent(id);
    }
    false
}

/// `hoverService.setupManagedHover(getDefaultHoverDelegate('mouse'), element, content)`.
fn setup_mouse_hover(node: NodeId, content: &str) -> Rc<dyn IManagedHover> {
    get_base_layer_hover_delegate().setup_managed_hover(get_default_hover_delegate(HoverPlacement::Mouse), node, ManagedHoverContentOrFactory::from(content), None)
}

/// O valor de texto de um `code` não vazio (o `code ?` do original é falso para a cadeia vazia).
fn code_value(code: &MarkerCode) -> Option<&str> {
    match code {
        MarkerCode::Text(text) => Some(text.as_str()).filter(|text| !text.is_empty()),
        MarkerCode::Target { value, .. } => Some(value.as_str()),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// MarkersWidgetAccessibilityProvider
// ---------------------------------------------------------------------------------------------------------------------

/// `MarkersWidgetAccessibilityProvider`.
pub struct MarkersWidgetAccessibilityProvider {
    label_service: Rc<dyn ILabelService>,
}

impl MarkersWidgetAccessibilityProvider {
    pub fn new(label_service: Rc<dyn ILabelService>) -> Self {
        Self { label_service }
    }

    fn widget_aria_label() -> String {
        localize("problemsView", "Problems View", &[])
    }
}

impl IListAccessibilityProvider<MarkerElement> for MarkersWidgetAccessibilityProvider {
    fn get_widget_aria_label(&self) -> String {
        Self::widget_aria_label()
    }

    fn get_aria_label(&self, element: &MarkerElement) -> Option<String> {
        match element {
            MarkerElement::ResourceMarkers(resource_markers) => {
                let label = self.label_service.get_uri_label(&resource_markers.resource, &UriLabelOptions { relative: Some(true), ..UriLabelOptions::default() });
                let path = if label.is_empty() { resource_markers.path.clone() } else { label };
                Some(Messages::markers_tree_aria_label_resource(resource_markers.markers().len(), &resource_markers.name, &native_path().dirname(&path)))
            }
            MarkerElement::Marker(marker) => Some(Messages::markers_tree_aria_label_marker(marker)),
            MarkerElement::RelatedInformation(related_information) => Some(Messages::markers_tree_aria_label_related_information(&related_information.raw)),
        }
    }
}

impl IListAccessibilityProvider<Rc<MarkerTableItem>> for MarkersWidgetAccessibilityProvider {
    fn get_widget_aria_label(&self) -> String {
        Self::widget_aria_label()
    }

    fn get_aria_label(&self, element: &Rc<MarkerTableItem>) -> Option<String> {
        Some(Messages::markers_tree_aria_label_marker(&element.base))
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// VirtualDelegate
// ---------------------------------------------------------------------------------------------------------------------

/// `const enum TemplateId`.
mod template_id {
    pub const RESOURCE_MARKERS: &str = "rm";
    pub const MARKER: &str = "m";
    pub const RELATED_INFORMATION: &str = "ri";
}

/// `VirtualDelegate`.
pub struct VirtualDelegate {
    markers_view_state: Rc<MarkersViewModel>,
}

impl VirtualDelegate {
    /// `static LINE_HEIGHT`.
    pub const LINE_HEIGHT: f64 = LINE_HEIGHT;

    pub fn new(markers_view_state: Rc<MarkersViewModel>) -> Self {
        Self { markers_view_state }
    }
}

impl IListVirtualDelegate<MarkerElement> for VirtualDelegate {
    fn get_height(&self, element: &MarkerElement) -> f64 {
        if let MarkerElement::Marker(marker) = element {
            let view_model = self.markers_view_state.get_view_model(marker);
            let no_of_lines = if view_model.is_none_or(|view_model| view_model.multiline()) { marker.lines().len() } else { 1 };
            return no_of_lines as f64 * Self::LINE_HEIGHT;
        }
        Self::LINE_HEIGHT
    }

    fn get_template_id(&self, element: &MarkerElement) -> String {
        match element {
            MarkerElement::ResourceMarkers(_) => template_id::RESOURCE_MARKERS,
            MarkerElement::Marker(_) => template_id::MARKER,
            MarkerElement::RelatedInformation(_) => template_id::RELATED_INFORMATION,
        }
        .to_owned()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// FilterData
// ---------------------------------------------------------------------------------------------------------------------

/// `FilterData`: `ResourceMarkersFilterData | MarkerFilterData | RelatedInformationFilterData`.
#[derive(Clone, Debug)]
pub enum FilterData {
    ResourceMarkers { uri_matches: Vec<Match> },
    Marker { line_matches: Vec<Vec<Match>>, source_matches: Vec<Match>, code_matches: Vec<Match> },
    RelatedInformation { uri_matches: Vec<Match>, message_matches: Vec<Match> },
}

// ---------------------------------------------------------------------------------------------------------------------
// ResourceMarkersRenderer
// ---------------------------------------------------------------------------------------------------------------------

/// `IResourceMarkersTemplateData` com o nó da linha, para o `layout`.
struct ResourceMarkersTemplate {
    ui: Ui,
    container: NodeId,
    resource_label_container: NodeId,
    resource_label: Rc<dyn IResourceLabel>,
    badge_wrapper: NodeId,
    count: Rc<CountBadge>,
}

impl ResourceMarkersTemplate {
    /// `.resource-label-container { flex: 1 }` (ou o tamanho do conteúdo, com `.wide`) e o `.count-badge-wrapper`
    /// (com `margin-left: 10px` no modo `.wide`).
    fn layout_row(&self) {
        let (row_width, wide, label_natural_width, badge_width, badge_height) = {
            let tree = self.ui.tree();
            let badge = tree.bounds(self.count.element());
            (
                tree.bounds(self.container).width(),
                ancestor_has_class(&tree, self.container, "wide"),
                tree.bounds(self.resource_label.element()).width(),
                badge.width(),
                badge.height(),
            )
        };
        let content_width = (row_width - CONTENTS_PADDING_RIGHT).max(0.0);
        let (label_width, badge_x) = if wide {
            let label_width = label_natural_width.min((content_width - badge_width - WIDE_BADGE_MARGIN_LEFT).max(0.0));
            (label_width, label_width + WIDE_BADGE_MARGIN_LEFT)
        } else {
            let badge_x = (content_width - badge_width).max(0.0);
            (badge_x, badge_x)
        };
        let mut tree = self.ui.tree_mut();
        place(&mut tree, self.resource_label_container, 0.0, 0.0, label_width, LINE_HEIGHT);
        place(&mut tree, self.badge_wrapper, badge_x, 0.0, badge_width, LINE_HEIGHT);
        place(&mut tree, self.count.element(), 0.0, ((LINE_HEIGHT - badge_height) / 2.0).max(0.0), badge_width, badge_height);
    }
}

/// O ponteiro do `ResourceMarkers`: a chave do `Map<ResourceMarkers, ...>` do original (a identidade do objeto).
fn resource_markers_key(resource_markers: &Rc<ResourceMarkers>) -> usize {
    Rc::as_ptr(resource_markers) as usize
}

/// `ResourceMarkersRenderer`.
pub struct ResourceMarkersRenderer {
    ui: Ui,
    labels: Rc<ResourceLabels>,
    styles: MarkersTreeStyles,
    rendered_nodes: Rc<RefCell<HashMap<usize, Vec<Rc<ResourceMarkersTemplate>>>>>,
    disposables: Rc<DisposableStore>,
}

/// `FileResourceMarkersRenderer extends ResourceMarkersRenderer {}`.
pub type FileResourceMarkersRenderer = ResourceMarkersRenderer;

impl ResourceMarkersRenderer {
    pub fn new(ui: &Ui, labels: Rc<ResourceLabels>, styles: MarkersTreeStyles, on_did_change_render_node_count: Event<Rc<MarkersTreeNode>>) -> Rc<Self> {
        let rendered_nodes: Rc<RefCell<HashMap<usize, Vec<Rc<ResourceMarkersTemplate>>>>> = Rc::default();
        let disposables = DisposableStore::new();
        let nodes = rendered_nodes.clone();
        disposables.add(on_did_change_render_node_count.listen(move |node| Self::on_did_change_render_node_count(&nodes, node)));
        Rc::new(Self { ui: ui.clone(), labels, styles, rendered_nodes, disposables })
    }

    fn on_did_change_render_node_count(rendered_nodes: &Rc<RefCell<HashMap<usize, Vec<Rc<ResourceMarkersTemplate>>>>>, node: &Rc<MarkersTreeNode>) {
        let Some(MarkerElement::ResourceMarkers(resource_markers)) = node.element() else { return };
        let node_renders = rendered_nodes.borrow().get(&resource_markers_key(&resource_markers)).cloned();
        let Some(node_renders) = node_renders else { return };
        for node_render in &node_renders {
            Self::update_count(node, node_render);
        }
    }

    fn update_count(node: &Rc<MarkersTreeNode>, template: &ResourceMarkersTemplate) {
        template.count.set_count(node.children().iter().filter(|child| child.visible()).count() as i64);
        template.layout_row();
    }

    fn template(template_data: &TemplateData) -> &Rc<ResourceMarkersTemplate> {
        template_data.downcast_ref::<Rc<ResourceMarkersTemplate>>().expect("o template do ResourceMarkersRenderer")
    }
}

impl IDisposable for ResourceMarkersRenderer {
    fn dispose(&self) {
        self.disposables.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ResourceMarkersRenderer"
    }
}

impl ITreeRenderer<MarkersTreeModel> for ResourceMarkersRenderer {
    fn template_id(&self) -> String {
        template_id::RESOURCE_MARKERS.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let resource_label_container = create_el(&mut self.ui.tree_mut(), container, Role::Generic, "resource-label-container");
        let resource_label = self.labels.create(resource_label_container, Some(IIconLabelCreationOptions { support_highlights: true, ..IIconLabelCreationOptions::default() }));

        let badge_wrapper = create_el(&mut self.ui.tree_mut(), container, Role::Generic, "count-badge-wrapper");
        let count = CountBadge::new(&self.ui, badge_wrapper, &ICountBadgeOptions::default(), self.styles.count_badge);

        Rc::new(Rc::new(ResourceMarkersTemplate { ui: self.ui.clone(), container, resource_label_container, resource_label, badge_wrapper, count }))
    }

    fn render_element(&self, node: &Rc<MarkersTreeNode>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let Some(MarkerElement::ResourceMarkers(resource_markers)) = node.element() else { return };
        let template = Self::template(template_data);
        let uri_matches = match node.filter_data() {
            Some(FilterData::ResourceMarkers { uri_matches }) => uri_matches,
            _ => Vec::new(),
        };

        template.resource_label.set_file(
            &resource_markers.resource,
            Some(IFileLabelOptions {
                resource: IResourceLabelOptions {
                    icon_label: IIconLabelValueOptions { matches: Some(uri_matches), ..IIconLabelValueOptions::default() },
                    ..IResourceLabelOptions::default()
                },
                ..IFileLabelOptions::default()
            }),
        );

        Self::update_count(node, template);
        self.rendered_nodes.borrow_mut().entry(resource_markers_key(&resource_markers)).or_default().push(template.clone());
    }

    fn dispose_element(&self, node: &Rc<MarkersTreeNode>, _index: i64, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let Some(MarkerElement::ResourceMarkers(resource_markers)) = node.element() else { return };
        let template = Self::template(template_data);
        let key = resource_markers_key(&resource_markers);
        let mut rendered_nodes = self.rendered_nodes.borrow_mut();
        let node_renders = rendered_nodes.entry(key).or_default();
        let Some(node_render_index) = node_renders.iter().position(|node_render| Rc::ptr_eq(node_render, template)) else {
            errors::throw(Error::new("Disposing unknown resource marker".to_owned()));
        };

        if node_renders.len() == 1 {
            rendered_nodes.remove(&key);
        } else {
            node_renders.remove(node_render_index);
        }
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        let template = Self::template(template_data);
        template.resource_label.dispose();
        template.count.dispose();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// MarkerRenderer e MarkerWidget
// ---------------------------------------------------------------------------------------------------------------------

/// Um rótulo e o nó que o contém.
struct LabelItem {
    node: NodeId,
    label: Rc<HighlightedLabel>,
}

/// O código do marcador: `(`, o rótulo e `)`, dentro de `.marker-code` (e de um `a.monaco-link`, se tem `target`).
struct CodeItem {
    /// O nó que entra na linha: o `a.monaco-link` ou o próprio `.marker-code`.
    node: NodeId,
    /// `.marker-code`.
    container: NodeId,
    open: NodeId,
    close: NodeId,
    open_width: f64,
    close_width: f64,
    label: Rc<HighlightedLabel>,
}

impl CodeItem {
    fn width(&self) -> f64 {
        self.open_width + self.label.width() + self.close_width
    }
}

/// Uma linha da mensagem (`.marker-message-line`).
struct MessageLine {
    line: NodeId,
    message: NodeId,
    label: Rc<HighlightedLabel>,
}

/// A primeira linha (`.details-container`): a origem, o código, `[Ln, Col]` e o botão de várias linhas.
struct Details {
    source: Option<LabelItem>,
    code: Option<CodeItem>,
    line_col: NodeId,
    line_col_width: f64,
    multiline_actions: NodeId,
    multiline_bar: Rc<ActionBar>,
}

/// `MarkerWidget`.
struct MarkerWidget {
    this: Weak<MarkerWidget>,
    base: Rc<Disposable>,
    services: Rc<MarkersTreeServices>,
    markers_view_model: Rc<MarkersViewModel>,
    parent: NodeId,
    action_bar: Rc<ActionBar>,
    actions_container: NodeId,
    icon: NodeId,
    icon_container: NodeId,
    message_and_details_container: NodeId,
    message_and_details_container_hover: Rc<dyn IManagedHover>,
    quick_fix_item: Rc<RefCell<Option<Rc<QuickFixActionViewItem>>>>,
    disposables: Rc<DisposableStore>,
    lines: RefCell<Vec<MessageLine>>,
    details: RefCell<Option<Details>>,
    hovered: Cell<bool>,
}

impl MarkerWidget {
    fn new(parent: NodeId, markers_view_model: Rc<MarkersViewModel>, services: Rc<MarkersTreeServices>) -> Rc<Self> {
        let ui = services.ui.clone();
        let quick_fix_item: Rc<RefCell<Option<Rc<QuickFixActionViewItem>>>> = Rc::default();

        let actions_container = create_el(&mut ui.tree_mut(), parent, Role::Generic, "actions");
        let provider: IActionViewItemProvider = {
            let (ui, context_menu_service, slot) = (ui.clone(), services.context_menu_service.clone(), quick_fix_item.clone());
            Rc::new(move |action: &ActionRef, options: &IActionViewItemOptions| {
                if action.id() != QuickFixAction::ID {
                    return None;
                }
                let item = QuickFixActionViewItem::new(&ui, action.clone(), options.clone(), context_menu_service.clone());
                *slot.borrow_mut() = Some(item.clone());
                Some(item as Rc<dyn IActionViewItem>)
            })
        };
        let action_bar = ActionBar::new(&ui, actions_container, IActionBarOptions { action_view_item_provider: Some(provider), styles: services.styles.action_bar.clone(), ..IActionBarOptions::default() });

        // wrap the icon in a container that get the icon color as foreground color. That way, if the
        // list view does not have a specific color for the icon (=the color variable is invalid) it
        // falls back to the foreground color of container (inherit)
        let (icon_container, icon, message_and_details_container) = {
            let mut tree = ui.tree_mut();
            let icon_container = create_el(&mut tree, parent, Role::Generic, "");
            let icon = create_el(&mut tree, icon_container, Role::Image, "");
            let message_and_details_container = create_el(&mut tree, parent, Role::Generic, "marker-message-details-container");
            tree.style_mut(message_and_details_container).clip = true;
            (icon_container, icon, message_and_details_container)
        };
        let message_and_details_container_hover = setup_mouse_hover(message_and_details_container, "");

        let base = Disposable::new();
        base.register(action_bar.clone());
        {
            let hover = message_and_details_container_hover.clone();
            base.register(to_disposable(move || hover.dispose()));
        }
        let disposables = DisposableStore::new();
        base.register(disposables.clone());

        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            services,
            markers_view_model,
            parent,
            action_bar,
            actions_container,
            icon,
            icon_container,
            message_and_details_container,
            message_and_details_container_hover,
            quick_fix_item,
            disposables,
            lines: RefCell::new(Vec::new()),
            details: RefCell::new(None),
            hovered: Cell::new(false),
        })
    }

    fn render(&self, element: &Rc<Marker>, filter_data: Option<&FilterData>) {
        let ui = self.services.ui.clone();
        self.action_bar.clear();
        self.disposables.clear();
        self.lines.borrow_mut().clear();
        *self.details.borrow_mut() = None;
        self.hovered.set(false);
        clear_node(&mut ui.tree_mut(), self.message_and_details_container);

        let severity = element.marker.severity.to_severity();
        {
            let mut tree = ui.tree_mut();
            tree.set_class_name(self.icon_container, &format!("marker-icon {}", severity.map_or("", |severity| severity.as_str())));
            match severity {
                Some(severity) => {
                    tree.set_class_name(self.icon, &format!("codicon {}", SeverityIcon::class_name(severity)));
                    let color = self.services.styles.severity_icon.color(severity).unwrap_or(self.services.styles.label.foreground);
                    tree.style_mut(self.icon).content = Content::Codicon { id: SeverityIcon::theme_icon(severity).id, size: CODICON_SIZE as f32, color };
                }
                None => {
                    tree.set_class_name(self.icon, "codicon");
                    tree.style_mut(self.icon).content = Content::None;
                }
            }
        }
        self.render_quickfix_actionbar(element);

        self.render_message_and_details(element, filter_data);

        let (enter, leave) = (self.this.clone(), self.this.clone());
        let (entered, left) = (element.clone(), element.clone());
        self.disposables.add(ui.listen(self.parent, EventKind::PointerEnter, false, move |_| {
            if let Some(this) = enter.upgrade() {
                this.hovered.set(true);
                this.markers_view_model.on_marker_mouse_hover(&entered);
                this.layout();
            }
        }));
        self.disposables.add(ui.listen(self.parent, EventKind::PointerLeave, false, move |_| {
            if let Some(this) = leave.upgrade() {
                this.hovered.set(false);
                this.markers_view_model.on_marker_mouse_leave(&left);
                this.layout();
            }
        }));
        self.layout();
    }

    fn render_quickfix_actionbar(&self, marker: &Rc<Marker>) {
        let ui = &self.services.ui;
        let Some(view_model) = self.markers_view_model.get_view_model(marker) else { return };
        let quick_fix_action = view_model.quick_fix_action();
        self.action_bar.push(
            &[quick_fix_action.clone() as ActionRef],
            IActionOptions {
                item: IActionViewItemOptions { icon: Some(true), label: Some(false), styles: self.services.styles.action_bar.clone(), ..IActionViewItemOptions::default() },
                index: None,
            },
        );
        self.action_bar.layout();
        ui.tree_mut().toggle_class(self.icon_container, "quickFix", quick_fix_action.enabled());

        let this = self.this.clone();
        self.disposables.add(quick_fix_action.on_did_change().listen(move |event| {
            if let (Some(enabled), Some(this)) = (event.enabled, this.upgrade()) {
                this.services.ui.tree_mut().toggle_class(this.icon_container, "quickFix", enabled);
                this.layout();
            }
        }));
        let this = self.this.clone();
        self.disposables.add(quick_fix_action.on_show_quick_fixes().listen(move |_| {
            let Some(this) = this.upgrade() else { return };
            let quick_fix_action_view_item = this.quick_fix_item.borrow().clone();
            if let Some(quick_fix_action_view_item) = quick_fix_action_view_item {
                quick_fix_action_view_item.show_quick_fixes();
            }
        }));
    }

    fn render_multiline_actionbar(&self, marker: &Rc<Marker>, parent: NodeId) -> (NodeId, Rc<ActionBar>) {
        let ui = &self.services.ui;
        let multiline_actions = create_el(&mut ui.tree_mut(), parent, Role::Generic, "multiline-actions");
        let provider: IActionViewItemProvider = {
            let ui = ui.clone();
            Rc::new(move |action: &ActionRef, options: &IActionViewItemOptions| {
                if action.id() != TOGGLE_MULTILINE_ACTION {
                    return None;
                }
                Some(ToggleMultilineActionViewItem::new(&ui, action.clone(), IActionViewItemOptions { icon: Some(true), ..options.clone() }) as Rc<dyn IActionViewItem>)
            })
        };
        let multiline_actionbar = ActionBar::new(ui, multiline_actions, IActionBarOptions { action_view_item_provider: Some(provider), styles: self.services.styles.action_bar.clone(), ..IActionBarOptions::default() });
        self.disposables.add(multiline_actionbar.clone());

        let view_model = self.markers_view_model.get_view_model(marker);
        let multiline = view_model.as_ref().is_some_and(|view_model| view_model.multiline());
        let run_view_model = view_model.clone();
        let action = Action::new(
            TOGGLE_MULTILINE_ACTION,
            "",
            "",
            view_model.is_some() && marker.lines().len() > 1,
            Some(Rc::new(move |_| {
                if let Some(view_model) = &run_view_model {
                    view_model.set_multiline(!view_model.multiline());
                }
                MaybePromise::Value(Dynamic::Undefined)
            })),
        );
        self.disposables.add(action.clone());
        action.set_tooltip(if multiline { localize("single line", "Show message in single line", &[]) } else { localize("multi line", "Show message in multiple lines", &[]) });
        let toggle_icon = if multiline { expanded_icon() } else { collapsed_icon() };
        action.set_class(Some(toggle_icon.as_class_name()));
        multiline_actionbar.push(
            &[action as ActionRef],
            IActionOptions { item: IActionViewItemOptions { icon: Some(true), label: Some(false), styles: self.services.styles.action_bar.clone(), ..IActionViewItemOptions::default() }, index: None },
        );
        multiline_actionbar.layout();
        (multiline_actions, multiline_actionbar)
    }

    fn render_message_and_details(&self, element: &Rc<Marker>, filter_data: Option<&FilterData>) {
        let ui = self.services.ui.clone();
        let lines = element.lines();
        let view_state = self.markers_view_model.get_view_model(element);
        let multiline = view_state.is_none_or(|view_state| view_state.multiline());
        let line_matches: Vec<Vec<Match>> = match filter_data {
            Some(FilterData::Marker { line_matches, .. }) => line_matches.clone(),
            _ => Vec::new(),
        };
        self.message_and_details_container_hover.update(ManagedHoverContentOrFactory::from(element.marker.message.as_str()), None);

        let label_styles = self.services.styles.label_styles();
        let mut message_lines: Vec<MessageLine> = Vec::new();
        for index in 0..(if multiline { lines.len() } else { 1 }).max(1) {
            let (line, message) = {
                let mut tree = ui.tree_mut();
                let line = create_el(&mut tree, self.message_and_details_container, Role::Generic, "marker-message-line");
                tree.style_mut(line).clip = true;
                let message = create_el(&mut tree, line, Role::Generic, "marker-message");
                tree.style_mut(message).clip = true;
                (line, message)
            };
            let label = HighlightedLabel::new(&ui, message, None, label_styles);
            self.disposables.add(label.clone());
            let text = lines.get(index).map(|line| {
                let line_units = line.units();
                if line_units.len() > 1000 { format!("{}...", String::from_utf16_lossy(&line_units[..1000])) } else { line.to_string_lossy() }
            });
            label.set(text.as_deref(), &highlights(line_matches.get(index).map_or(&[][..], Vec::as_slice)), "", false, false);
            message_lines.push(MessageLine { line, message, label });
        }
        let first_line = message_lines[0].line;
        *self.lines.borrow_mut() = message_lines;
        let details = self.render_details(element, filter_data, first_line);
        *self.details.borrow_mut() = Some(details);
    }

    fn render_details(&self, marker: &Rc<Marker>, filter_data: Option<&FilterData>, parent: NodeId) -> Details {
        let ui = self.services.ui.clone();
        let label_styles = self.services.styles.label_styles();
        ui.tree_mut().add_class(parent, "details-container");

        let (source_matches, code_matches): (&[Match], &[Match]) = match filter_data {
            Some(FilterData::Marker { source_matches, code_matches, .. }) => (source_matches, code_matches),
            _ => (&[], &[]),
        };
        let raw = &marker.marker;
        let source_text = raw.source.as_deref().filter(|source| !source.is_empty());
        let code = raw.code.as_ref().filter(|code| code_value(code).is_some());

        let mut source_item = None;
        let mut code_item = None;
        if source_text.is_some() || code.is_some() {
            let node = create_el(&mut ui.tree_mut(), parent, Role::Generic, "marker-source");
            ui.tree_mut().style_mut(node).opacity = 0.7;
            let label = HighlightedLabel::new(&ui, node, None, label_styles);
            self.disposables.add(label.clone());
            label.set(raw.source.as_deref(), &highlights(source_matches), "", false, false);
            source_item = Some(LabelItem { node, label });

            if let Some(code) = code {
                code_item = Some(self.render_code(parent, code, code_matches));
            }
        }

        let (line_col, line_col_width) = text_node(&ui, parent, "marker-line", &Messages::markers_panel_at_line_col_number(raw.start_line_number, raw.start_column), &label_styles, label_styles.foreground);
        ui.tree_mut().style_mut(line_col).opacity = 0.7;

        let (multiline_actions, multiline_bar) = self.render_multiline_actionbar(marker, parent);
        Details { source: source_item, code: code_item, line_col, line_col_width, multiline_actions, multiline_bar }
    }

    /// O `.marker-code`: texto simples, ou dentro de um `Link` quando o código tem `target`.
    fn render_code(&self, parent: NodeId, code: &MarkerCode, code_matches: &[Match]) -> CodeItem {
        let ui = self.services.ui.clone();
        let mut label_styles = self.services.styles.label_styles();
        let (node, container, value) = match code {
            MarkerCode::Text(text) => {
                let container = create_el(&mut ui.tree_mut(), parent, Role::Generic, "marker-code");
                (container, container, text.as_str())
            }
            MarkerCode::Target { value, target } => {
                let href = target.to_string(true).to_string_lossy();
                if let Some(link_foreground) = self.services.styles.text_link_foreground {
                    label_styles.foreground = link_foreground;
                }
                let link = create_el(&mut ui.tree_mut(), parent, Role::Link, "monaco-link");
                let container = create_el(&mut ui.tree_mut(), link, Role::Generic, "marker-code");
                self.setup_link(link, href);
                (link, container, value.as_str())
            }
        };
        ui.tree_mut().style_mut(node).opacity = 0.7;
        let (open, open_width) = text_node(&ui, container, "", "(", &label_styles, label_styles.foreground);
        let label = HighlightedLabel::new(&ui, container, None, label_styles);
        self.disposables.add(label.clone());
        label.set(Some(value), &highlights(code_matches), "", false, false);
        let (close, close_width) = text_node(&ui, container, "", ")", &label_styles, label_styles.foreground);
        CodeItem { node, container, open, close, open_width, close_width, label }
    }

    /// O comportamento do `Link`: a dica com o endereço e a abertura por clique, `Enter` ou `Space`.
    fn setup_link(&self, link: NodeId, href: String) {
        let ui = self.services.ui.clone();
        let hover = setup_mouse_hover(link, &href);
        self.disposables.add(to_disposable(move || hover.dispose()));
        let open = {
            let opener_service = self.services.opener_service.clone();
            Rc::new(move |e: &mut je_ui::Event| {
                e.stop(true);
                let options = OpenOptions { allow_commands: Some(AllowCommands::Bool(true)), ..OpenOptions::default() };
                let _ = opener_service.open(&OpenResource::Text(JsString::from(href.as_str())), Some(&options));
            })
        };
        let on_click = open.clone();
        self.disposables.add(ui.listen(link, EventKind::Click, false, move |e| (*on_click)(e)));
        self.disposables.add(ui.listen(link, EventKind::KeyDown, false, move |e| {
            if let Some(event) = StandardKeyboardEvent::from_event(e)
                && (event.equals(KeyCode::Enter.0 as u32) || event.equals(KeyCode::Space.0 as u32))
            {
                (*open)(e);
            }
        }));
    }

    /// `.monaco-list-row:hover`, `.selected` ou `.focused`.
    fn row_active(&self) -> bool {
        self.hovered.get() || row_selected_or_focused(&self.services.ui.tree(), self.parent)
    }

    /// O `display: flex` de `.monaco-tl-contents`: as ações, o ícone e o contêiner da mensagem, com as regras de `markers.css`.
    fn layout(&self) {
        let ui = self.services.ui.clone();
        let active = self.row_active();
        let row_width = ui.tree().bounds(self.parent).width();

        // `.actions .monaco-action-bar { display: none }` até o ponteiro, a seleção ou o foco; `.action-item.disabled` some
        let actions_width: f64 = (0..self.action_bar.length())
            .filter(|&index| self.action_bar.get_action(index).is_some_and(|action| action.enabled()))
            .map(|index| self.action_bar.get_width(index) + 2.0 * ACTION_ITEM_MARGIN)
            .sum();
        let actions_visible = active && actions_width > 0.0;
        // `.marker-icon.quickFix { display: none }` no mesmo caso
        let icon_visible = !(active && ui.tree().has_class(self.icon_container, "quickFix"));

        let mut x = 0.0;
        {
            let mut tree = ui.tree_mut();
            tree.style_mut(self.actions_container).visible = actions_visible;
            place(&mut tree, self.actions_container, x, 0.0, if actions_visible { actions_width } else { 0.0 }, LINE_HEIGHT);
            if actions_visible {
                x += actions_width;
            }
            tree.style_mut(self.icon_container).visible = icon_visible;
            place(&mut tree, self.icon_container, x, 0.0, MARKER_ICON_WIDTH, LINE_HEIGHT);
            place(&mut tree, self.icon, (MARKER_ICON_WIDTH - CODICON_SIZE) / 2.0, (LINE_HEIGHT - CODICON_SIZE) / 2.0, CODICON_SIZE, CODICON_SIZE);
            if icon_visible {
                x += MARKER_ICON_WIDTH;
            }
        }

        // `.marker-message-details-container { flex: 1; overflow: hidden }`
        let available = (row_width - x - CONTENTS_PADDING_RIGHT).max(0.0);
        let lines = self.lines.borrow();
        place(&mut ui.tree_mut(), self.message_and_details_container, x, 0.0, available, lines.len() as f64 * LINE_HEIGHT);
        for (index, line) in lines.iter().enumerate() {
            place(&mut ui.tree_mut(), line.line, 0.0, index as f64 * LINE_HEIGHT, available, LINE_HEIGHT);
            if index > 0 {
                line.label.set_max_width(Some(available));
                let mut tree = ui.tree_mut();
                place(&mut tree, line.message, 0.0, 0.0, line.label.width(), LINE_HEIGHT);
            }
        }
        if let (Some(first), Some(details)) = (lines.first(), &*self.details.borrow()) {
            self.layout_details(first, details, available);
        }
    }

    /// A primeira linha (`display: flex`): a mensagem encolhe com reticências, o resto mantém a largura.
    fn layout_details(&self, first: &MessageLine, details: &Details, available: f64) {
        let ui = &self.services.ui;
        let source_width = details.source.as_ref().map(|source| source.label.width());
        let code_width = details.code.as_ref().map(CodeItem::width);
        let multiline_width = if (0..details.multiline_bar.length()).any(|index| details.multiline_bar.get_action(index).is_some_and(|action| action.enabled())) {
            (0..details.multiline_bar.length()).map(|index| details.multiline_bar.get_width(index)).sum::<f64>()
        } else {
            0.0
        };
        let fixed = source_width.map_or(0.0, |width| DETAILS_MARGIN_LEFT + width)
            + code_width.unwrap_or(0.0)
            + DETAILS_MARGIN_LEFT
            + details.line_col_width
            + DETAILS_MARGIN_LEFT
            + multiline_width;
        let message_available = (available - fixed).max(0.0);
        first.label.set_max_width(Some(message_available));
        let message_width = first.label.width().min(message_available);

        let mut tree = ui.tree_mut();
        let mut x = 0.0;
        place(&mut tree, first.message, x, 0.0, message_width, LINE_HEIGHT);
        x += message_width;
        if let (Some(source), Some(width)) = (&details.source, source_width) {
            x += DETAILS_MARGIN_LEFT;
            place(&mut tree, source.node, x, 0.0, width, LINE_HEIGHT);
            x += width;
        }
        if let (Some(code), Some(width)) = (&details.code, code_width) {
            place(&mut tree, code.node, x, 0.0, width, LINE_HEIGHT);
            if code.node != code.container {
                place(&mut tree, code.container, 0.0, 0.0, width, LINE_HEIGHT);
            }
            place(&mut tree, code.open, 0.0, 0.0, code.open_width, LINE_HEIGHT);
            place(&mut tree, code.label.element(), code.open_width, 0.0, code.label.width(), LINE_HEIGHT);
            place(&mut tree, code.close, code.open_width + code.label.width(), 0.0, code.close_width, LINE_HEIGHT);
            x += width;
        }
        x += DETAILS_MARGIN_LEFT;
        place(&mut tree, details.line_col, x, 0.0, details.line_col_width, LINE_HEIGHT);
        x += details.line_col_width + DETAILS_MARGIN_LEFT;
        place(&mut tree, details.multiline_actions, x, 0.0, multiline_width, LINE_HEIGHT);
    }

    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `IMarkerTemplateData`.
struct MarkerTemplate {
    marker_widget: Rc<MarkerWidget>,
}

/// `MarkerRenderer`.
pub struct MarkerRenderer {
    markers_view_state: Rc<MarkersViewModel>,
    services: Rc<MarkersTreeServices>,
}

impl MarkerRenderer {
    pub fn new(markers_view_state: Rc<MarkersViewModel>, services: Rc<MarkersTreeServices>) -> Self {
        Self { markers_view_state, services }
    }

    fn template(template_data: &TemplateData) -> &MarkerTemplate {
        template_data.downcast_ref::<MarkerTemplate>().expect("o template do MarkerRenderer")
    }
}

impl ITreeRenderer<MarkersTreeModel> for MarkerRenderer {
    fn template_id(&self) -> String {
        template_id::MARKER.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        Rc::new(MarkerTemplate { marker_widget: MarkerWidget::new(container, self.markers_view_state.clone(), self.services.clone()) })
    }

    fn render_element(&self, node: &Rc<MarkersTreeNode>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        if let Some(MarkerElement::Marker(marker)) = node.element() {
            Self::template(template_data).marker_widget.render(&marker, node.filter_data().as_ref());
        }
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        Self::template(template_data).marker_widget.dispose();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// RelatedInformationRenderer
// ---------------------------------------------------------------------------------------------------------------------

/// `IRelatedInformationTemplateData`.
struct RelatedInformationTemplate {
    ui: Ui,
    container: NodeId,
    resource_label: Rc<HighlightedLabel>,
    ln_col: NodeId,
    separator: NodeId,
    description: Rc<HighlightedLabel>,
    separator_width: f64,
}

impl RelatedInformationTemplate {
    /// A linha em fluxo: o recurso, `[Ln, Col]`, `:` (com `padding-right: 4px`) e a descrição.
    fn layout_row(&self, ln_col_width: f64) {
        let row_width = self.ui.tree().bounds(self.container).width();
        let available = (row_width - CONTENTS_PADDING_RIGHT).max(0.0);
        let mut x = 0.0;
        let resource_width = self.resource_label.width();
        let separator_width = self.separator_width + 4.0;
        let description_available = (available - resource_width - ln_col_width - separator_width).max(0.0);
        self.description.set_max_width(Some(description_available));
        let mut tree = self.ui.tree_mut();
        place(&mut tree, self.resource_label.element(), x, 0.0, resource_width, LINE_HEIGHT);
        x += resource_width;
        place(&mut tree, self.ln_col, x, 0.0, ln_col_width, LINE_HEIGHT);
        x += ln_col_width;
        place(&mut tree, self.separator, x, 0.0, self.separator_width, LINE_HEIGHT);
        x += separator_width;
        place(&mut tree, self.description.element(), x, 0.0, self.description.width().min(description_available), LINE_HEIGHT);
    }
}

/// `RelatedInformationRenderer`.
pub struct RelatedInformationRenderer {
    services: Rc<MarkersTreeServices>,
}

impl RelatedInformationRenderer {
    pub fn new(services: Rc<MarkersTreeServices>) -> Self {
        Self { services }
    }

    fn template(template_data: &TemplateData) -> &RelatedInformationTemplate {
        template_data.downcast_ref::<RelatedInformationTemplate>().expect("o template do RelatedInformationRenderer")
    }
}

impl ITreeRenderer<MarkersTreeModel> for RelatedInformationRenderer {
    fn template_id(&self) -> String {
        template_id::RELATED_INFORMATION.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let ui = self.services.ui.clone();
        let label_styles = self.services.styles.label_styles();
        {
            let mut tree = ui.tree_mut();
            create_el(&mut tree, container, Role::Generic, "actions");
            create_el(&mut tree, container, Role::Generic, "icon");
        }

        let resource_node = create_el(&mut ui.tree_mut(), container, Role::Generic, "related-info-resource");
        ui.tree_mut().style_mut(resource_node).opacity = 0.7;
        let resource_label = HighlightedLabel::new(&ui, resource_node, None, label_styles);
        let (ln_col, _) = text_node(&ui, container, "marker-line", "", &label_styles, label_styles.foreground);
        ui.tree_mut().style_mut(ln_col).opacity = 0.7;
        let (separator, separator_width) = text_node(&ui, container, "related-info-resource-separator", ":", &label_styles, label_styles.foreground);
        ui.tree_mut().style_mut(separator).opacity = 0.7;
        let description_node = create_el(&mut ui.tree_mut(), container, Role::Generic, "marker-description");
        let description = HighlightedLabel::new(&ui, description_node, None, label_styles);

        Rc::new(RelatedInformationTemplate { ui, container, resource_label, ln_col, separator, description, separator_width })
    }

    fn render_element(&self, node: &Rc<MarkersTreeNode>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let Some(MarkerElement::RelatedInformation(related_information)) = node.element() else { return };
        let template = Self::template(template_data);
        let raw = &related_information.raw;
        let (uri_matches, message_matches) = match node.filter_data() {
            Some(FilterData::RelatedInformation { uri_matches, message_matches }) => (uri_matches, message_matches),
            _ => (Vec::new(), Vec::new()),
        };

        let resource_label_title = self.services.label_service.get_uri_label(&raw.resource, &UriLabelOptions { relative: Some(true), ..UriLabelOptions::default() });
        template.resource_label.set(Some(ext_uri().basename(&raw.resource, None).as_str()), &highlights(&uri_matches), &resource_label_title, false, false);
        let styles = self.services.styles.label_styles();
        let ln_col = Messages::markers_panel_at_line_col_number(raw.start_line_number, raw.start_column);
        let font = ui_font(styles.font_size, styles.font_weight);
        let ln_col_width = measure_text(&ln_col, &font);
        template.ui.tree_mut().style_mut(template.ln_col).content = Content::Text { text: ln_col, style: font, color: styles.foreground, line_height: LINE_HEIGHT };
        template.description.set(Some(raw.message.as_str()), &highlights(&message_matches), &raw.message, false, false);
        template.layout_row(ln_col_width);
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        let template = Self::template(template_data);
        template.resource_label.dispose();
        template.description.dispose();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Filter
// ---------------------------------------------------------------------------------------------------------------------

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

    fn filter_resource_markers(&self, resource_markers: &ResourceMarkers) -> TreeFilterResult<FilterData> {
        let options = self.options();
        if unsupported_schemas().contains(resource_markers.resource.scheme().to_string_lossy().as_str()) {
            return TreeFilterResult::Bool(false);
        }

        // Filter resource by pattern first (globs)
        // Excludes pattern
        if options.excludes_matcher.matches(&resource_markers.resource) {
            return TreeFilterResult::Bool(false);
        }

        // Includes pattern
        if options.includes_matcher.matches(&resource_markers.resource) {
            return TreeFilterResult::Bool(true);
        }

        // Fiter by text. Do not apply negated filters on resources instead use exclude patterns
        if !options.text_filter.text.is_empty() && !options.text_filter.negate {
            let uri_matches = (FilterOptions::FILTER)(&units(&options.text_filter.text), &units(&ext_uri().basename(&resource_markers.resource, None)));
            if let Some(uri_matches) = uri_matches {
                return TreeFilterResult::Data { visibility: VisibilityValue::Bool(true), data: FilterData::ResourceMarkers { uri_matches } };
            }
        }

        TreeFilterResult::Visibility(TreeVisibility::Recurse)
    }

    /// O desfecho comum de `filterMarker` e `filterRelatedInformation` depois de casar (ou não) o texto.
    fn text_filter_result(options: &FilterOptions, matched: bool, parent_visibility: TreeVisibility, data: impl FnOnce() -> FilterData) -> TreeFilterResult<FilterData> {
        // Matched and not negated
        if matched && !options.text_filter.negate {
            return TreeFilterResult::Data { visibility: VisibilityValue::Bool(true), data: data() };
        }

        // Matched and negated - exclude it only if parent visibility is not set
        if matched && options.text_filter.negate && parent_visibility == TreeVisibility::Recurse {
            return TreeFilterResult::Bool(false);
        }

        // Not matched and negated - include it only if parent visibility is not set
        if !matched && options.text_filter.negate && parent_visibility == TreeVisibility::Recurse {
            return TreeFilterResult::Bool(true);
        }

        TreeFilterResult::Visibility(parent_visibility)
    }

    fn filter_marker(&self, marker: &Marker, parent_visibility: TreeVisibility) -> TreeFilterResult<FilterData> {
        let options = self.options();
        let severity = marker.marker.severity;
        let matches_severity = options.show_errors && severity == MarkerSeverity::ERROR
            || options.show_warnings && severity == MarkerSeverity::WARNING
            || options.show_infos && severity == MarkerSeverity::INFO;

        if !matches_severity {
            return TreeFilterResult::Bool(false);
        }

        // Check source filters if present
        if !options.matches_source_filters(marker.marker.source.as_deref()) {
            return TreeFilterResult::Bool(false);
        }

        if options.text_filter.text.is_empty() {
            return TreeFilterResult::Bool(true);
        }

        let text = units(&options.text_filter.text);
        let line_matches: Vec<Vec<Match>> = marker.lines().iter().map(|line| (FilterOptions::MESSAGE_FILTER)(&text, line.units()).unwrap_or_default()).collect();

        let source_matches = marker.marker.source.as_deref().filter(|source| !source.is_empty()).and_then(|source| (FilterOptions::FILTER)(&text, &units(source)));
        let code_matches = marker.marker.code.as_ref().and_then(code_value).and_then(|code| (FilterOptions::FILTER)(&text, &units(code)));
        let matched = source_matches.is_some() || code_matches.is_some() || line_matches.iter().any(|line_match| !line_match.is_empty());

        Self::text_filter_result(&options, matched, parent_visibility, || FilterData::Marker {
            line_matches,
            source_matches: source_matches.unwrap_or_default(),
            code_matches: code_matches.unwrap_or_default(),
        })
    }

    fn filter_related_information(&self, related_information: &RelatedInformation, parent_visibility: TreeVisibility) -> TreeFilterResult<FilterData> {
        let options = self.options();
        if options.text_filter.text.is_empty() {
            return TreeFilterResult::Bool(true);
        }

        let text = units(&options.text_filter.text);
        let uri_matches = (FilterOptions::FILTER)(&text, &units(&ext_uri().basename(&related_information.raw.resource, None)));
        let message_matches = (FilterOptions::MESSAGE_FILTER)(&text, &units(&native_path().basename(&related_information.raw.message, None)));
        let matched = uri_matches.is_some() || message_matches.is_some();

        Self::text_filter_result(&options, matched, parent_visibility, || FilterData::RelatedInformation {
            uri_matches: uri_matches.unwrap_or_default(),
            message_matches: message_matches.unwrap_or_default(),
        })
    }
}

impl ITreeFilter<MarkerElement, FilterData> for Filter {
    fn filter(&self, element: &MarkerElement, parent_visibility: TreeVisibility) -> TreeFilterResult<FilterData> {
        match element {
            MarkerElement::ResourceMarkers(resource_markers) => self.filter_resource_markers(resource_markers),
            MarkerElement::Marker(marker) => self.filter_marker(marker, parent_visibility),
            MarkerElement::RelatedInformation(related_information) => self.filter_related_information(related_information, parent_visibility),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// MarkerViewModel
// ---------------------------------------------------------------------------------------------------------------------

/// `MarkerViewModel`.
pub struct MarkerViewModel {
    base: Rc<Disposable>,
    weak_self: Weak<MarkerViewModel>,
    services: MarkerViewModelServices,
    marker: Rc<Marker>,
    on_did_change: Rc<Emitter<()>>,
    model_promise: RefCell<Option<CancelablePromise<Rc<TextModel>>>>,
    code_actions_promise: RefCell<Option<CancelablePromise<Option<Rc<dyn CodeActionSet>>>>>,
    multiline: Cell<bool>,
    quick_fix_action: RefCell<Option<Rc<QuickFixAction>>>,
}

impl MarkerViewModel {
    pub fn new(marker: Rc<Marker>, services: MarkerViewModelServices) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        Rc::new_cyclic(|weak| Self {
            base,
            weak_self: weak.clone(),
            services,
            marker,
            on_did_change,
            model_promise: RefCell::new(None),
            code_actions_promise: RefCell::new(None),
            multiline: Cell::new(true),
            quick_fix_action: RefCell::new(None),
        })
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// `get multiline()`.
    pub fn multiline(&self) -> bool {
        self.multiline.get()
    }

    /// `set multiline(value)`.
    pub fn set_multiline(&self, value: bool) {
        if self.multiline.get() != value {
            self.multiline.set(value);
            self.on_did_change.fire(());
        }
    }

    /// `get quickFixAction()`.
    pub fn quick_fix_action(&self) -> Rc<QuickFixAction> {
        if let Some(action) = self.quick_fix_action.borrow().clone() {
            return action;
        }
        let action = QuickFixAction::new(self.marker.clone());
        self.base.register(action.clone());
        *self.quick_fix_action.borrow_mut() = Some(action.clone());
        action
    }

    /// `showLightBulb()`.
    pub fn show_light_bulb(&self) {
        if let Some(this) = self.weak_self.upgrade() {
            let _ = this.set_quick_fixes(true).catch(|error| errors::on_unexpected_error(&error));
        }
    }

    fn set_quick_fixes(self: &Rc<Self>, wait_for_model: bool) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let code_actions = this.get_code_actions(wait_for_model).await?;
            let quick_fix_action = this.quick_fix_action();
            quick_fix_action.set_quick_fixes(code_actions.as_ref().map(|code_actions| this.to_actions(code_actions)).unwrap_or_default());
            quick_fix_action.auto_fixable(code_actions.as_ref().is_some_and(|code_actions| code_actions.has_auto_fix()));
            Ok(())
        })
    }

    fn get_code_actions(self: &Rc<Self>, wait_for_model: bool) -> Promise<Option<Rc<dyn CodeActionSet>>> {
        if let Some(code_actions_promise) = &*self.code_actions_promise.borrow() {
            return code_actions_promise.promise.clone();
        }
        let this = self.clone();
        self.get_model(wait_for_model).then_maybe(move |model| {
            let Some(model) = model else { return MaybePromise::Value(None) };
            if this.code_actions_promise.borrow().is_none() {
                let (weak, registry, marker) = (this.weak_self.clone(), this.services.language_features_service.code_action_provider.clone(), this.marker.clone());
                let code_actions_promise = create_cancelable_promise(move |cancellation_token| {
                    let range = marker.range();
                    let trigger = CodeActionTrigger {
                        r#type: CodeActionTriggerType::Invoke,
                        trigger_action: CodeActionTriggerSource::ProblemsView,
                        filter: Some(CodeActionFilter { include: Some(CodeActionKind::quick_fix()), ..CodeActionFilter::default() }),
                        auto_apply: None,
                        context: None,
                    };
                    get_code_actions(&registry, &model, SelectionOrRange::Range(range), &trigger, Rc::new(je_editor::code_action::platform::ProgressNone), &cancellation_token).then(move |actions| {
                        // this._register(actions)
                        let to_dispose = actions.clone();
                        match weak.upgrade() {
                            Some(this) => {
                                this.base.register(to_disposable(move || to_dispose.dispose()));
                            }
                            None => to_dispose.dispose(),
                        }
                        Some(actions)
                    })
                });
                *this.code_actions_promise.borrow_mut() = Some(code_actions_promise);
            }
            let promise = this.code_actions_promise.borrow().as_ref().map(|promise| promise.promise.clone());
            MaybePromise::Promise(promise.expect("a promessa das ações de código"))
        })
    }

    fn to_actions(&self, code_actions: &Rc<dyn CodeActionSet>) -> Vec<ActionRef> {
        code_actions
            .valid_actions()
            .into_iter()
            .map(|item| {
                let (id, title) = {
                    let action = item.action.borrow();
                    (action.command.as_ref().map_or_else(|| action.title.clone(), |command| command.id.clone()), action.title.clone())
                };
                let weak = self.weak_self.clone();
                to_action(ToActionProps {
                    id,
                    label: title,
                    tooltip: None,
                    enabled: None,
                    checked: None,
                    class: None,
                    alt: None,
                    run: Rc::new(move |_| {
                        let Some(this) = weak.upgrade() else { return MaybePromise::Value(Dynamic::Undefined) };
                        let item = item.clone();
                        MaybePromise::Promise(async_fn(async move {
                            this.open_file_at_marker().await?;
                            apply_code_action(&this.services.apply_code_action_services, &item, ApplyCodeActionReason::FromProblemsView, None, &CancellationToken::None).await?;
                            Ok(Dynamic::Undefined)
                        }))
                    }),
                })
            })
            .collect()
    }

    fn open_file_at_marker(&self) -> Promise<()> {
        let range = self.marker.range();
        let input = UntypedEditorInput {
            resource: Some(self.marker.resource().clone()),
            options: Some(TextEditorOptions {
                base: EditorOptions { preserve_focus: Some(true), pinned: Some(false), reveal_if_visible: Some(true), ..EditorOptions::default() },
                selection: Some(TextEditorSelection {
                    start_line_number: f64::from(range.start_line_number),
                    start_column: f64::from(range.start_column),
                    end_line_number: Some(f64::from(range.end_line_number)),
                    end_column: Some(f64::from(range.end_column)),
                }),
                ..TextEditorOptions::default()
            }),
            ..UntypedEditorInput::default()
        };
        self.services.editor_service.open_editor(input).then(|_| ())
    }

    fn get_model(self: &Rc<Self>, wait_for_model: bool) -> Promise<Option<Rc<TextModel>>> {
        if let Some(model) = self.services.model_service.get_model(self.marker.resource()) {
            return Promise::resolved(Some(model));
        }
        if wait_for_model {
            if self.model_promise.borrow().is_none() {
                let (weak, resource) = (self.weak_self.clone(), self.marker.resource().clone());
                let model_promise = create_cancelable_promise(move |_| {
                    let (promise, resolver) = Promise::new();
                    if let Some(this) = weak.upgrade() {
                        let subscription = this.services.model_service.on_model_added().listen(move |model| {
                            if ext_uri().is_equal(Some(&model.uri()), Some(&resource), false) {
                                resolver.resolve(model.clone());
                            }
                        });
                        this.base.register(subscription);
                    }
                    promise
                });
                *self.model_promise.borrow_mut() = Some(model_promise);
            }
            let promise = self.model_promise.borrow().as_ref().map(|promise| promise.promise.clone());
            return promise.expect("a promessa do modelo").then(Some);
        }
        Promise::resolved(None)
    }
}

impl IDisposable for MarkerViewModel {
    fn dispose(&self) {
        let model_promise = self.model_promise.borrow_mut().take();
        if let Some(model_promise) = model_promise {
            model_promise.cancel();
        }
        let code_actions_promise = self.code_actions_promise.borrow_mut().take();
        if let Some(code_actions_promise) = code_actions_promise {
            code_actions_promise.cancel();
        }
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "MarkerViewModel"
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// MarkersViewModel
// ---------------------------------------------------------------------------------------------------------------------

/// O `{ viewModel, disposables }` de `markersViewStates`.
struct MarkerViewState {
    view_model: Rc<MarkerViewModel>,
    disposables: Rc<DisposableStore>,
}

/// `MarkersViewModel`.
pub struct MarkersViewModel {
    base: Rc<Disposable>,
    weak_self: Weak<MarkersViewModel>,
    services: MarkerViewModelServices,
    on_did_change: Rc<Emitter<Option<Rc<Marker>>>>,
    on_did_change_view_mode: Rc<Emitter<MarkersViewMode>>,
    markers_view_states: RefCell<HashMap<String, MarkerViewState>>,
    markers_per_resource: RefCell<HashMap<String, Vec<Rc<Marker>>>>,
    bulk_update: Cell<bool>,
    hovered_marker: RefCell<Option<Rc<Marker>>>,
    hover_delayer: Rc<Delayer<()>>,
    view_mode_context_key: Rc<dyn IContextKey>,
    multiline: Cell<bool>,
    view_mode: Cell<MarkersViewMode>,
}

impl MarkersViewModel {
    /// `new MarkersViewModel(multiline = true, viewMode = MarkersViewMode.Tree, contextKeyService, instantiationService)`.
    pub fn new(multiline: bool, view_mode: MarkersViewMode, context_key_service: &dyn IContextKeyService, services: MarkerViewModelServices) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        let on_did_change_view_mode = Emitter::new();
        base.register(on_did_change.clone());
        base.register(on_did_change_view_mode.clone());
        let hover_delayer = Rc::new(Delayer::new(Delay::Millis(300)));
        base.register(hover_delayer.clone());

        let view_mode_context_key = markers_view_mode_context_key().bind_to(context_key_service);
        view_mode_context_key.set(Dynamic::string(view_mode.as_str()));

        Rc::new_cyclic(|weak| Self {
            base,
            weak_self: weak.clone(),
            services,
            on_did_change,
            on_did_change_view_mode,
            markers_view_states: RefCell::new(HashMap::new()),
            markers_per_resource: RefCell::new(HashMap::new()),
            bulk_update: Cell::new(false),
            hovered_marker: RefCell::new(None),
            hover_delayer,
            view_mode_context_key,
            multiline: Cell::new(multiline),
            view_mode: Cell::new(view_mode),
        })
    }

    /// `onDidChange`: o marcador que mudou, ou `None` para uma mudança em todos.
    pub fn on_did_change(&self) -> Event<Option<Rc<Marker>>> {
        self.on_did_change.event()
    }

    /// `onDidChangeViewMode`.
    pub fn on_did_change_view_mode(&self) -> Event<MarkersViewMode> {
        self.on_did_change_view_mode.event()
    }

    /// `add(marker)`.
    pub fn add(&self, marker: &Rc<Marker>) {
        if self.markers_view_states.borrow().contains_key(&marker.id) {
            return;
        }
        let view_model = MarkerViewModel::new(marker.clone(), self.services.clone());
        let disposables = DisposableStore::new();
        disposables.add_rc(view_model.clone());
        view_model.set_multiline(self.multiline.get());
        let (weak, changed) = (self.weak_self.clone(), marker.clone());
        disposables.add(view_model.on_did_change().listen(move |_| {
            if let Some(this) = weak.upgrade()
                && !this.bulk_update.get()
            {
                this.on_did_change.fire(Some(changed.clone()));
            }
        }));
        self.markers_view_states.borrow_mut().insert(marker.id.clone(), MarkerViewState { view_model, disposables });

        self.markers_per_resource.borrow_mut().entry(marker.resource().to_string(false).to_string_lossy()).or_default().push(marker.clone());
    }

    /// `remove(resource)`.
    pub fn remove(&self, resource: &je_base::uri::URI) {
        let key = resource.to_string(false).to_string_lossy();
        let markers = self.markers_per_resource.borrow().get(&key).cloned().unwrap_or_default();
        for marker in markers {
            let value = self.markers_view_states.borrow_mut().remove(&marker.id);
            if let Some(value) = value {
                value.disposables.dispose();
            }
            let mut hovered_marker = self.hovered_marker.borrow_mut();
            if hovered_marker.as_ref().is_some_and(|hovered| Rc::ptr_eq(hovered, &marker)) {
                *hovered_marker = None;
            }
        }
        self.markers_per_resource.borrow_mut().remove(&key);
    }

    /// `getViewModel(marker)`.
    pub fn get_view_model(&self, marker: &Marker) -> Option<Rc<MarkerViewModel>> {
        self.markers_view_states.borrow().get(&marker.id).map(|value| value.view_model.clone())
    }

    /// `onMarkerMouseHover(marker)`.
    pub fn on_marker_mouse_hover(&self, marker: &Rc<Marker>) {
        *self.hovered_marker.borrow_mut() = Some(marker.clone());
        let weak = self.weak_self.clone();
        let _ = self
            .hover_delayer
            .trigger(
                move || {
                    if let Some(this) = weak.upgrade() {
                        let hovered_marker = this.hovered_marker.borrow().clone();
                        if let Some(model) = hovered_marker.and_then(|hovered_marker| this.get_view_model(&hovered_marker)) {
                            model.show_light_bulb();
                        }
                    }
                    MaybePromise::Value(())
                },
                None,
            )
            .catch(|_| ());
    }

    /// `onMarkerMouseLeave(marker)`.
    pub fn on_marker_mouse_leave(&self, marker: &Rc<Marker>) {
        let mut hovered_marker = self.hovered_marker.borrow_mut();
        if hovered_marker.as_ref().is_some_and(|hovered| Rc::ptr_eq(hovered, marker)) {
            *hovered_marker = None;
        }
    }

    /// `get multiline()`.
    pub fn multiline(&self) -> bool {
        self.multiline.get()
    }

    /// `set multiline(value)`.
    pub fn set_multiline(&self, value: bool) {
        let mut changed = false;
        if self.multiline.get() != value {
            self.multiline.set(value);
            changed = true;
        }
        self.bulk_update.set(true);
        let view_models: Vec<Rc<MarkerViewModel>> = self.markers_view_states.borrow().values().map(|state| state.view_model.clone()).collect();
        for view_model in view_models {
            if view_model.multiline() != value {
                view_model.set_multiline(value);
                changed = true;
            }
        }
        self.bulk_update.set(false);
        if changed {
            self.on_did_change.fire(None);
        }
    }

    /// `get viewMode()`.
    pub fn view_mode(&self) -> MarkersViewMode {
        self.view_mode.get()
    }

    /// `set viewMode(value)`.
    pub fn set_view_mode(&self, value: MarkersViewMode) {
        if self.view_mode.get() == value {
            return;
        }

        self.view_mode.set(value);
        self.on_did_change_view_mode.fire(value);
        self.view_mode_context_key.set(Dynamic::string(value.as_str()));
    }
}

impl IDisposable for MarkersViewModel {
    fn dispose(&self) {
        let states = std::mem::take(&mut *self.markers_view_states.borrow_mut());
        for state in states.into_values() {
            state.disposables.dispose();
        }
        self.markers_per_resource.borrow_mut().clear();
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "MarkersViewModel"
    }
}
