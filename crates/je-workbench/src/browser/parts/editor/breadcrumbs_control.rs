//! Porte de `src/vs/workbench/browser/parts/editor/breadcrumbsControl.ts`: o [`BreadcrumbsControl`] (construtor, `update`,
//! `layout`, `hide`, o seletor sob a migalha, o foco, o seletor de tipo de editor e o menu de contexto dele), os itens
//! ([`FileItem`] e [`OutlineItem`]), a [`BreadcrumbsControlFactory`] e os comandos registrados
//! ([`register_breadcrumbs_commands`]).
//!
//! Notas de tradução:
//! - **Injeção.** Os parâmetros decorados do construtor são a struct [`BreadcrumbsControlServices`]; `container`, `options`
//!   e `editorGroup` seguem posicionais. O `IInstantiationService.createInstance(X, ...)` dos filhos (`BreadcrumbsModel`,
//!   `ResourceLabels`, `FileItem`, `OutlineItem`, os seletores) é a chamada direta ao construtor deles com o pacote de
//!   serviços correspondente. `IEditorGroupView` e `IEditorGroupsView` são os contratos de `editor.rs`.
//! - **`Payload_*`** são tipos unitários comparados por tipo ([`payload_reveal`], [`payload_reveal_aside`],
//!   [`payload_pick`]); `payload instanceof StandardMouseEvent` é o `downcast_ref::<PointerData>()` do payload do widget.
//! - **`hideContextView({ source, didPick, sideBySide })`** leva o [`PickerHideData`]; o `source: this` é o endereço do
//!   controle.
//! - **`setupManagedHover`** do seletor de tipo de editor usa o `IHoverDelegate2` do `je-widgets` (o serviço de hover).
//! - **O nó do seletor de tipo** (`div.breadcrumbs-editor-type` com o rótulo e o `codicon-chevron-down`) é posicionado por
//!   [`BreadcrumbsControl::layout`] à direita, com `padding: 0 4px 0 8px`, fonte de 11px, chevron de 13px e `max-width: 40%`;
//!   o `offsetWidth` é a largura desse nó.
//! - **Arrastar** (`createBreadcrumbDndObserver`, `DragAndDropObserver`, `applyDragImage`, `fillEditorsDragData`,
//!   `fillInSymbolsDragData`, `LocalSelectionTransfer`): a `je-ui` não tem origem de arrasto nem `DataTransfer`, então os itens
//!   não são arrastáveis e a opção `dragEditor` é só guardada. Entra com o arrastar nativo.
//! - **`PixelRatio.onDidChange`** (o zoom do navegador que fecha o seletor) não existe na janela nativa: não há zoom de
//!   página, então não há escuta.
//! - **`OutlineItem.render`**: o `template.iconClass` do renderer é o [`IOutlineRendererTemplate`] (o renderer que quer o
//!   ícone escondido devolve o `Rc<dyn IOutlineRendererTemplate>` como dado do template).
//! - **`outline.reveal(element, ...)`** recebe o `OutlineElement2` embrulhado, como no original (o `element` ali é o
//!   `OutlineElement2`, não o símbolo).
//! - **`WorkbenchListFocusContextKey`** é `listFocus && !inputFocus && !treestickyScrollFocused`.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::ActionRef;
use je_base::async_::{async_fn, timeout};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::layout::IAnchor;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, MutableDisposable, to_disposable};
use je_base::nls::localize;
use je_base::scrollable::ScrollbarVisibility;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_editor::services::clipboard_service::I_CLIPBOARD_SERVICE;
use je_editor::services::theme_service::IThemeService;
use je_platform::action::{Category, CommandAction, CommandActionTitle, Title, Toggled, localize2};
use je_platform::actions::menu_ids::{command_palette, editor_title_context, menubar_appearance_menu, notebook_sticky_scroll_context, notebook_toolbar, notebook_toolbar_context, sticky_scroll_context};
use je_platform::actions::{Action2, Action2Keybinding, Action2MenuItem, Action2Options, register_action2};
use je_platform::commands::{CommandHandler, ICommandService};
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService, RawContextKey};
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression};
use je_platform::files::files::{FileKind, FileStat};
use je_platform::files::i_file_service::IFileService;
use je_platform::keybinding::keybindings_registry::{CommandAndKeybindingRule, KeybindingRule, KeybindingWeight, Keybindings, PlatformKeybindings, keybindings_registry};
use je_platform::label::label::ILabelService;
use je_platform::list::list_service::I_LIST_SERVICE;
use je_platform::opener::editor_types::EditorOptions;
use je_platform::quickinput::quick_input::IQuickInputService;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::to_rgba8;
use je_ui::{Content, EventKind, NodeId, PointerData, Role, Ui};
use je_widgets::breadcrumbs_widget::{BreadcrumbsItem, BreadcrumbsItemRef, BreadcrumbsWidget, IBreadcrumbsItemEvent, IBreadcrumbsWidgetStyles};
use je_widgets::dom::{FocusTracker, create_el, window_size};
use je_widgets::hover_api::{HoverPlacement, IHoverDelegate, IHoverDelegate2, IManagedHover, ManagedHoverContentOrFactory, get_default_hover_delegate};
use je_widgets::icon_label::{IIconLabelCreationOptions, IIconLabelValueOptions, IconLabelText};
use je_widgets::text::{natural_line_height, set_node_text, ui_font};
use je_widgets::tree::index_tree_model::TreeNode;
use je_widgets::tree::tree_types::TreeVisibility;

use super::breadcrumbs::{BreadcrumbsConfig, BreadcrumbsConfigKeys, I_BREADCRUMBS_SERVICE, IBreadcrumbsService};
use super::breadcrumbs_model::{BreadcrumbsElement, BreadcrumbsModel, BreadcrumbsModelServices, FileElement, OutlineElement2, OutlineElementValue};
use super::breadcrumbs_picker::{BreadcrumbsFilePicker, BreadcrumbsOutlinePicker, BreadcrumbsPicker, BreadcrumbsPickerServices};
use super::editor::IEditorGroupView;
use crate::services::editor::editor_groups_service::IEditorGroup;
use super::editor_type_picker::{IAvailableEditorTypes, create_editor_type_actions, editor_type_display_label, get_available_editor_types};
use crate::browser::labels::{
    FileDecorations, IFileLabelOptions, IResourceLabel, IResourceLabelOptions, IResourceLabelProps, ResourceLabelResource, ResourceLabels, ResourceLabelsServices, default_labels_container,
};
use crate::common::editor::{EditorInputOrUntyped, EditorResourceAccessor, EditorResourceAccessorOptions, SideBySideEditor, TextEditorOptions, UntypedEditorInput};
use je_platform::contextview::context_view::{
    AnchorTarget, ContextMenuRequest, HideData, IContextMenuDelegate, IContextMenuService, IContextViewDelegate, IContextViewService, hide_data_to,
};
use je_platform::contextview::view_element::ViewElement;

use crate::services::contextview::native_context_view_service::{native_node, node_element};
use crate::services::editor::editor_groups_service::{I_EDITOR_GROUPS_SERVICE, PreferredGroup};
use crate::services::editor::editor_resolver_service::{HIDDEN_EDITOR_TYPES_SETTING_ID, IEditorResolverService};
use crate::services::editor::editor_service::{I_EDITOR_SERVICE, IEditorService};
use crate::services::outline::outline::{I_OUTLINE_SERVICE, IOutline, OutlineElement, OutlineTarget, OutlineTreeElement};

/// `IBreadcrumbsControlOptions`.
#[derive(Clone, Copy, Debug, Default)]
pub struct BreadcrumbsControlOptions {
    pub show_file_icons: bool,
    pub show_symbol_icons: bool,
    pub show_decoration_colors: bool,
    pub show_placeholder: bool,
    pub drag_editor: bool,
    pub widget_styles: Option<IBreadcrumbsWidgetStyles>,
    /// Mostra o seletor do tipo de editor à direita (só na barra de migalhas abaixo das abas).
    pub show_editor_type_picker: bool,
}

/// O destino de um `Payload_Reveal*`: `SIDE_GROUP` ou `ACTIVE_GROUP`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorGroupTarget {
    /// `SIDE_GROUP`.
    Side,
    /// `ACTIVE_GROUP`.
    Active,
}

/// `Payload_Reveal`.
struct PayloadReveal;
/// `Payload_RevealAside`.
struct PayloadRevealAside;
/// `Payload_Pick`.
struct PayloadPick;

/// `BreadcrumbsControl.Payload_Reveal`.
pub fn payload_reveal() -> Rc<dyn Any> {
    Rc::new(PayloadReveal)
}

/// `BreadcrumbsControl.Payload_RevealAside`.
pub fn payload_reveal_aside() -> Rc<dyn Any> {
    Rc::new(PayloadRevealAside)
}

/// `BreadcrumbsControl.Payload_Pick`.
pub fn payload_pick() -> Rc<dyn Any> {
    Rc::new(PayloadPick)
}

/// Se `payload` é o `Payload_Pick`.
pub fn is_payload_pick(payload: Option<&Rc<dyn Any>>) -> bool {
    payload.is_some_and(|payload| (**payload).is::<PayloadPick>())
}

/// `_getEditorGroup(data)`.
pub fn editor_group_target(payload: Option<&Rc<dyn Any>>) -> Option<EditorGroupTarget> {
    let payload = payload?;
    if (**payload).is::<PayloadRevealAside>() {
        Some(EditorGroupTarget::Side)
    } else if (**payload).is::<PayloadReveal>() {
        Some(EditorGroupTarget::Active)
    } else {
        None
    }
}

thread_local! {
    static CK_POSSIBLE: RawContextKey = RawContextKey::new("breadcrumbsPossible", false, localize("breadcrumbsPossible", "Whether the editor can show breadcrumbs", &[]));
    static CK_VISIBLE: RawContextKey = RawContextKey::new("breadcrumbsVisible", false, localize("breadcrumbsVisible", "Whether breadcrumbs are currently visible", &[]));
    static CK_ACTIVE: RawContextKey = RawContextKey::new("breadcrumbsActive", false, localize("breadcrumbsActive", "Whether breadcrumbs have focus", &[]));
    static CK_HAS_SYMBOLS: RawContextKey = RawContextKey::new("breadcrumbsHasSymbols", false, localize("breadcrumbsHasSymbols", "Whether breadcrumbs contain symbol items", &[]));
}

/// As estáticas de `BreadcrumbsControl`.
impl BreadcrumbsControl {
    pub const HEIGHT: f64 = 22.0;

    /// `SCROLLBAR_SIZES[sizing]`: `'default'` (3) ou `'large'` (8); outro texto não tem tamanho.
    pub fn scrollbar_size(sizing: &str) -> Option<f64> {
        match sizing {
            "default" => Some(3.0),
            "large" => Some(8.0),
            _ => None,
        }
    }

    /// `SCROLLBAR_VISIBILITY[visibility]`.
    pub fn scrollbar_visibility(visibility: &str) -> Option<ScrollbarVisibility> {
        match visibility {
            "auto" => Some(ScrollbarVisibility::Auto),
            "visible" => Some(ScrollbarVisibility::Visible),
            "hidden" => Some(ScrollbarVisibility::Hidden),
            _ => None,
        }
    }

    /// `CK_BreadcrumbsPossible`.
    pub fn ck_breadcrumbs_possible() -> RawContextKey {
        CK_POSSIBLE.with(Clone::clone)
    }

    /// `CK_BreadcrumbsVisible`.
    pub fn ck_breadcrumbs_visible() -> RawContextKey {
        CK_VISIBLE.with(Clone::clone)
    }

    /// `CK_BreadcrumbsActive`.
    pub fn ck_breadcrumbs_active() -> RawContextKey {
        CK_ACTIVE.with(Clone::clone)
    }

    /// `CK_BreadcrumbsHasSymbols`.
    pub fn ck_breadcrumbs_has_symbols() -> RawContextKey {
        CK_HAS_SYMBOLS.with(Clone::clone)
    }
}

/// O retângulo de um item na tela (`dom.getDomNodePagePosition`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemRect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// O resultado do `getAnchor`: o ponto de âncora e os argumentos de `picker.show(...)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickerPlacement {
    pub x: f64,
    pub y: f64,
    pub max_height: f64,
    pub width: f64,
    pub arrow_size: f64,
    pub arrow_offset: f64,
}

/// O corpo do `getAnchor` de `_onSelectEvent`. `mouse_x` é o `posx` do `StandardMouseEvent` quando o payload do
/// evento é um clique (senão `None`, e a seta fica a 30% da largura do item).
pub fn compute_picker_placement(window_width: f64, window_height: f64, item: ItemRect, mouse_x: Option<f64>) -> PickerPlacement {
    let max_inner_width = window_width - 8.0; // a little less the full widget
    let mut max_height = (window_height * 0.7).min(300.0);

    let width = max_inner_width.min(240.0_f64.max(max_inner_width / 4.17));
    let arrow_size = 8.0;

    let y = item.top + item.height + arrow_size;
    if y + max_height >= window_height {
        max_height = window_height - y - 30.0; // room for shadow and status bar
    }
    let mut x = item.left;
    if x + width >= max_inner_width {
        x = max_inner_width - width;
    }
    let arrow_offset = match mouse_x {
        Some(posx) => {
            let max_arrow_offset = width - 2.0 * arrow_size;
            let mut offset = posx - x;
            if offset > max_arrow_offset {
                x = (max_inner_width - width).min(x + offset - max_arrow_offset);
                offset = max_arrow_offset;
            }
            offset
        }
        None => (item.left + item.width * 0.3) - x,
    };
    PickerPlacement { x, y, max_height, width, arrow_size, arrow_offset: arrow_offset.max(0.0) }
}

/// O que o renderer de símbolos do outline devolve como dado do template quando quer que o controle esconda o ícone
/// (`dom.hide(template.iconClass)` do `OutlineItem.render`).
pub trait IOutlineRendererTemplate {
    /// `iconClass`: o nó do ícone do símbolo.
    fn icon_class(&self) -> NodeId;
}

/// Os dados que `hideContextView` leva do seletor (`{ didPick?, sideBySide?, source? }`).
#[derive(Clone, Copy, Default)]
pub struct PickerHideData {
    pub did_pick: bool,
    pub side_by_side: bool,
    /// O endereço do [`BreadcrumbsControl`] que escondeu (o `source: this`); `None` quando outro esconde.
    pub source: Option<usize>,
}

/// O texto na cor herdada, num nó filho do `container`, com o tamanho dele (o `flex item` do widget de migalhas).
fn render_text(ui: &Ui, container: NodeId, text: &str, foreground: Rgba) {
    let style = ui_font(13.0, 400);
    let line_height = natural_line_height(&style).max(22.0);
    let mut tree = ui.tree_mut();
    let node = create_el(&mut tree, container, Role::Generic, "");
    let width = set_node_text(&mut tree, node, text, &style, foreground, line_height);
    tree.set_bounds(node, Rect::new(0.0, 0.0, width, line_height));
}

/// `OutlineItem`.
pub struct OutlineItem {
    pub model: Rc<BreadcrumbsModel>,
    pub element: OutlineElement2,
    pub options: BreadcrumbsControlOptions,
    disposables: Rc<DisposableStore>,
}

impl OutlineItem {
    pub fn new(model: Rc<BreadcrumbsModel>, element: OutlineElement2, options: BreadcrumbsControlOptions) -> Rc<Self> {
        Rc::new(Self { model, element, options, disposables: DisposableStore::new() })
    }
}

impl BreadcrumbsItem for OutlineItem {
    fn dispose(&self) {
        self.disposables.dispose();
    }

    fn equals(&self, other: &dyn BreadcrumbsItem) -> bool {
        let Some(other) = other.as_any().downcast_ref::<OutlineItem>() else {
            return false;
        };
        outline_element_value_is(&self.element.element, &other.element.element)
            && self.options.show_file_icons == other.options.show_file_icons
            && self.options.show_symbol_icons == other.options.show_symbol_icons
    }

    fn render(&self, ui: &Ui, container: NodeId, foreground: Rgba) {
        let element = match &self.element.element {
            // `element === outline`
            OutlineElementValue::Outline(_) => {
                render_text(ui, container, "\u{2026}", foreground);
                return;
            }
            OutlineElementValue::Element(element) => OutlineTreeElement(element.clone()),
        };
        let outline = &self.element.outline;
        let config = outline.config();

        let template_id = config.delegate.get_template_id(&element);
        let Some(renderer) = config.renderers.iter().find(|renderer| renderer.template_id() == template_id).cloned() else {
            let style = ui_font(13.0, 400);
            let mut tree = ui.tree_mut();
            let node = create_el(&mut tree, container, Role::Generic, "");
            let text = "<<NO RENDERER>>";
            let width = set_node_text(&mut tree, node, text, &style, foreground, 22.0);
            tree.set_bounds(node, Rect::new(0.0, 0.0, width, 22.0));
            return;
        };

        let template = renderer.render_template(container);
        let node = Rc::new(TreeNode {
            parent: RefCell::new(Weak::new()),
            element: Some(element),
            children: RefCell::new(Vec::new()),
            depth: 0,
            visible_children_count: Cell::new(0),
            visible_child_index: Cell::new(0),
            collapsible: Cell::new(false),
            collapsed: Cell::new(false),
            render_node_count: Cell::new(1),
            visibility: Cell::new(TreeVisibility::Visible),
            visible: Cell::new(true),
            filter_data: RefCell::new(None),
            last_diff_ids: RefCell::new(None),
        });
        renderer.render_element(&node, 0, &template, None);

        if !self.options.show_symbol_icons
            && let Some(template_icon) = template.downcast_ref::<Rc<dyn IOutlineRendererTemplate>>()
        {
            je_widgets::dom::hide(&mut ui.tree_mut(), template_icon.icon_class());
        }

        let (renderer_for_dispose, template_for_dispose) = (renderer.clone(), template.clone());
        self.disposables.add(to_disposable(move || renderer_for_dispose.dispose_template(&template_for_dispose)));

        // O arrastar do símbolo (`createBreadcrumbDndObserver`) não existe: ver as notas do módulo.
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `this.element.element === other.element.element`: o mesmo objeto.
fn outline_element_value_is(a: &OutlineElementValue, b: &OutlineElementValue) -> bool {
    match (a, b) {
        (OutlineElementValue::Outline(a), OutlineElementValue::Outline(b)) => std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b)),
        (OutlineElementValue::Element(a), OutlineElementValue::Element(b)) => std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b)),
        _ => false,
    }
}

/// `FileItem`.
pub struct FileItem {
    pub model: Rc<BreadcrumbsModel>,
    pub element: FileElement,
    pub options: BreadcrumbsControlOptions,
    labels: Rc<ResourceLabels>,
    hover_delegate: Rc<dyn IHoverDelegate>,
    disposables: Rc<DisposableStore>,
}

impl FileItem {
    pub fn new(model: Rc<BreadcrumbsModel>, element: FileElement, options: BreadcrumbsControlOptions, labels: Rc<ResourceLabels>, hover_delegate: Rc<dyn IHoverDelegate>) -> Rc<Self> {
        Rc::new(Self { model, element, options, labels, hover_delegate, disposables: DisposableStore::new() })
    }
}

impl BreadcrumbsItem for FileItem {
    fn dispose(&self) {
        self.disposables.dispose();
    }

    fn equals(&self, other: &dyn BreadcrumbsItem) -> bool {
        let Some(other) = other.as_any().downcast_ref::<FileItem>() else {
            return false;
        };
        self.element.equals(&other.element)
            && self.options.show_file_icons == other.options.show_file_icons
            && self.options.show_symbol_icons == other.options.show_symbol_icons
    }

    fn render(&self, ui: &Ui, container: NodeId, _foreground: Rgba) {
        // file/folder
        let label = self.labels.create(container, Some(IIconLabelCreationOptions { hover_delegate: Some(self.hover_delegate.clone()), ..Default::default() }));
        let resource_options = IResourceLabelOptions {
            icon_label: IIconLabelValueOptions { hide_icon: self.element.kind == FileKind::Folder || !self.options.show_file_icons, ..Default::default() },
            file_kind: Some(self.element.kind),
            file_decorations: Some(FileDecorations { colors: self.options.show_decoration_colors, badges: false }),
            ..Default::default()
        };
        match &self.element.label {
            Some(name) => label.set_resource(
                IResourceLabelProps { resource: Some(ResourceLabelResource::Uri(self.element.uri.clone())), name: Some(IconLabelText::Single(name.clone())), ..Default::default() },
                Some(IResourceLabelOptions { force_label: true, ..resource_options }),
            ),
            None => label.set_file(&self.element.uri, Some(IFileLabelOptions { resource: resource_options, hide_path: true, ..Default::default() })),
        }
        let kind_class = match self.element.kind {
            FileKind::File => "file",
            FileKind::Folder => "folder",
            FileKind::RootFolder => "root_folder",
        };
        ui.tree_mut().add_class(container, kind_class);
        let label_disposable: DisposableRef = label;
        self.disposables.add(label_disposable);

        // O arrastar do arquivo (`createBreadcrumbDndObserver`) não existe: ver as notas do módulo.
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// O item de `no elements` (a classe anônima de `update`).
struct EmptyItem;

impl BreadcrumbsItem for EmptyItem {
    fn dispose(&self) {}

    fn equals(&self, other: &dyn BreadcrumbsItem) -> bool {
        std::ptr::addr_eq(other.as_any(), self.as_any())
    }

    fn render(&self, ui: &Ui, container: NodeId, foreground: Rgba) {
        render_text(ui, container, &localize("empty", "no elements", &[]), foreground);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `separatorIcon`: `registerIcon('breadcrumb-separator', Codicon.chevronRight, ...)`.
fn separator_icon() -> ThemeIcon {
    ThemeIcon::from_id("chevron-right")
}

/// Os serviços injetados no construtor do `BreadcrumbsControl` (ver as notas do módulo).
#[derive(Clone)]
pub struct BreadcrumbsControlServices {
    pub ui: Ui,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub context_view_service: Rc<dyn IContextViewService>,
    pub context_menu_service: Rc<dyn IContextMenuService>,
    pub quick_input_service: Rc<dyn IQuickInputService>,
    pub file_service: Rc<dyn IFileService>,
    pub editor_service: Rc<dyn IEditorService>,
    pub editor_resolver_service: Rc<dyn IEditorResolverService>,
    pub command_service: Rc<dyn ICommandService>,
    pub label_service: Rc<dyn ILabelService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub hover_service: Rc<dyn IHoverDelegate2>,
    pub breadcrumbs_service: Rc<dyn IBreadcrumbsService>,
    pub theme_service: Rc<dyn IThemeService>,
    /// Para o `BreadcrumbsModel` criado em `update`.
    pub model_services: BreadcrumbsModelServices,
    /// Para os seletores criados em `_onSelectEvent`.
    pub picker_services: Rc<BreadcrumbsPickerServices>,
    /// Para o `ResourceLabels` do controle.
    pub resource_labels_services: Rc<ResourceLabelsServices>,
}

/// O nó do seletor de tipo de editor (`_editorTypeNode`, `_editorTypeLabel` e `_editorTypeHover`).
struct EditorTypeControl {
    node: NodeId,
    label: NodeId,
    chevron: NodeId,
    hover: Rc<dyn IManagedHover>,
    /// O texto do rótulo (para medir o `offsetWidth`).
    text: RefCell<String>,
}

/// O `defaultBreadcrumbsWidgetStyles` de `defaultStyles.ts`, a partir do tema de cores do serviço.
fn default_widget_styles(theme_service: &dyn IThemeService) -> IBreadcrumbsWidgetStyles {
    let theme = theme_service.get_color_theme();
    let color = |id: &str| theme.get_color(id, None).map(|color| to_rgba8(&color));
    IBreadcrumbsWidgetStyles {
        breadcrumbs_background: color("breadcrumb.background"),
        breadcrumbs_foreground: color("breadcrumb.foreground"),
        breadcrumbs_hover_foreground: color("breadcrumb.focusForeground"),
        breadcrumbs_focus_foreground: color("breadcrumb.focusForeground"),
        breadcrumbs_focus_and_selection_foreground: color("breadcrumb.activeSelectionForeground"),
    }
}

/// `BreadcrumbsControl`.
pub struct BreadcrumbsControl {
    this: Weak<BreadcrumbsControl>,
    services: BreadcrumbsControlServices,
    options: BreadcrumbsControlOptions,
    editor_group: Rc<dyn IEditorGroupView>,

    ck_breadcrumbs_possible: Rc<dyn IContextKey>,
    ck_breadcrumbs_visible: Rc<dyn IContextKey>,
    ck_breadcrumbs_active: Rc<dyn IContextKey>,
    ck_breadcrumbs_has_symbols: Rc<dyn IContextKey>,

    cf_use_quick_pick: Rc<BreadcrumbsConfig<bool>>,
    cf_show_icons: Rc<BreadcrumbsConfig<bool>>,
    cf_show_editor_type: Rc<BreadcrumbsConfig<bool>>,
    cf_title_scrollbar_sizing: Rc<BreadcrumbsConfig<String>>,
    cf_title_scrollbar_visibility: Rc<BreadcrumbsConfig<String>>,

    /// `domNode`.
    dom_node: NodeId,
    widget: Rc<BreadcrumbsWidget>,
    editor_type: RefCell<Option<Rc<EditorTypeControl>>>,
    last_layout_dimension: Cell<Option<(f64, f64)>>,

    disposables: Rc<DisposableStore>,
    editor_type_disposables: Rc<DisposableStore>,
    breadcrumbs_disposables: Rc<DisposableStore>,
    labels: Rc<ResourceLabels>,
    model: Rc<MutableDisposable<BreadcrumbsModel>>,
    breadcrumbs_picker_showing: Cell<bool>,
    breadcrumbs_picker_ignore_once_item: RefCell<Option<BreadcrumbsItemRef>>,

    hover_delegate: Rc<dyn IHoverDelegate>,

    on_did_visibility_change: Rc<Emitter<()>>,
}

impl BreadcrumbsControl {
    pub fn new(container: NodeId, options: BreadcrumbsControlOptions, editor_group: Rc<dyn IEditorGroupView>, services: BreadcrumbsControlServices) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let on_did_visibility_change = Emitter::new();
        disposables.add(on_did_visibility_change.clone());

        let dom_node = {
            let mut tree = services.ui.tree_mut();
            let node = create_el(&mut tree, container, Role::Generic, "breadcrumbs-control");
            tree.toggle_class(node, "with-editor-type", options.show_editor_type_picker);
            node
        };

        let configuration_service = services.configuration_service.clone();
        let cf_use_quick_pick = BreadcrumbsConfigKeys::USE_QUICK_PICK.bind_to(configuration_service.clone());
        let cf_show_icons = BreadcrumbsConfigKeys::ICONS.bind_to(configuration_service.clone());
        let cf_show_editor_type = BreadcrumbsConfigKeys::SHOW_EDITOR_TYPE.bind_to(configuration_service.clone());
        let cf_title_scrollbar_sizing = BreadcrumbsConfigKeys::TITLE_SCROLLBAR_SIZING.bind_to(configuration_service.clone());
        let cf_title_scrollbar_visibility = BreadcrumbsConfigKeys::TITLE_SCROLLBAR_VISIBILITY.bind_to(configuration_service.clone());

        let labels = ResourceLabels::new(default_labels_container(), services.resource_labels_services.clone());

        let sizing = cf_title_scrollbar_sizing.get_value(None).unwrap_or_else(|| "default".to_owned());
        let styles = options.widget_styles.unwrap_or_else(|| default_widget_styles(&*services.theme_service));
        let visibility = cf_title_scrollbar_visibility.get_value(None).unwrap_or_else(|| "auto".to_owned());

        let widget = BreadcrumbsWidget::new(
            &services.ui,
            dom_node,
            Self::scrollbar_size(&sizing).unwrap_or(3.0),
            Self::scrollbar_visibility(&visibility),
            separator_icon(),
            styles,
            None,
        );

        let editor_type_disposables = DisposableStore::new();
        disposables.add(editor_type_disposables.clone());
        let context_key_service = services.context_key_service.clone();
        let control = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            options,
            ck_breadcrumbs_possible: Self::ck_breadcrumbs_possible().bind_to(&*context_key_service),
            ck_breadcrumbs_visible: Self::ck_breadcrumbs_visible().bind_to(&*context_key_service),
            ck_breadcrumbs_active: Self::ck_breadcrumbs_active().bind_to(&*context_key_service),
            ck_breadcrumbs_has_symbols: Self::ck_breadcrumbs_has_symbols().bind_to(&*context_key_service),
            cf_use_quick_pick,
            cf_show_icons,
            cf_show_editor_type,
            cf_title_scrollbar_sizing,
            cf_title_scrollbar_visibility,
            dom_node,
            widget,
            editor_type: RefCell::new(None),
            last_layout_dimension: Cell::new(None),
            editor_type_disposables,
            breadcrumbs_disposables: DisposableStore::new(),
            labels,
            model: MutableDisposable::new(),
            breadcrumbs_picker_showing: Cell::new(false),
            breadcrumbs_picker_ignore_once_item: RefCell::new(None),
            hover_delegate: get_default_hover_delegate(HoverPlacement::Mouse),
            on_did_visibility_change,
            disposables,
            editor_group,
            services,
        });

        let this = Rc::downgrade(&control);
        control.widget.on_did_select_item().listen_in({ let this = this.clone(); move |event| if let Some(control) = this.upgrade() { control.on_select_event(event) } }, &control.disposables);
        control.widget.on_did_focus_item().listen_in({ let this = this.clone(); move |event| if let Some(control) = this.upgrade() { control.on_focus_event(event) } }, &control.disposables);
        control.widget.on_did_change_focus().listen_in({ let this = this.clone(); move |_| if let Some(control) = this.upgrade() { control.update_ck_breadcrumbs_active() } }, &control.disposables);

        if control.options.show_editor_type_picker {
            control.cf_show_editor_type.on_did_change().listen_in({ let this = this.clone(); move |_| if let Some(control) = this.upgrade() { control.update_editor_type_control() } }, &control.disposables);
            control.services.configuration_service.on_did_change_configuration().listen_in(
                {
                    let this = this.clone();
                    move |event| {
                        if event.affects_configuration(HIDDEN_EDITOR_TYPES_SETTING_ID, None)
                            && let Some(control) = this.upgrade()
                        {
                            control.update_editor_type_control();
                        }
                    }
                },
                &control.disposables,
            );
        }

        control.disposables.add(control.services.breadcrumbs_service.register(control.editor_group.id(), control.widget.clone()));
        control.hide();
        control
    }
}

impl BreadcrumbsControl {
    /// `onDidVisibilityChange`.
    pub fn on_did_visibility_change(&self) -> Event<()> {
        self.on_did_visibility_change.event()
    }

    /// `domNode`.
    pub fn dom_node(&self) -> NodeId {
        self.dom_node
    }

    /// `dispose()`.
    pub fn dispose(&self) {
        self.disposables.dispose();
        self.breadcrumbs_disposables.dispose();
        self.model.dispose();
        self.ck_breadcrumbs_possible.reset();
        self.ck_breadcrumbs_visible.reset();
        self.ck_breadcrumbs_active.reset();
        self.ck_breadcrumbs_has_symbols.reset();
        self.cf_use_quick_pick.dispose();
        self.cf_show_icons.dispose();
        self.cf_show_editor_type.dispose();
        self.cf_title_scrollbar_sizing.dispose();
        self.cf_title_scrollbar_visibility.dispose();
        self.widget.dispose();
        self.labels.dispose();
        self.services.ui.remove(self.dom_node);
    }

    /// `model`.
    pub fn model(&self) -> Option<Rc<BreadcrumbsModel>> {
        self.model.value()
    }

    /// `layout(dim)`.
    pub fn layout(&self, dim: Option<(f64, f64)>) {
        if let Some(dim) = dim {
            self.last_layout_dimension.set(Some(dim));
        }
        // When the editor type dropdown is visible it occupies space on the right, so shrink the breadcrumbs widget
        // accordingly to avoid it rendering behind the dropdown.
        let mut widget_dim = dim;
        if let (Some((width, height)), Some(editor_type)) = (dim, self.editor_type.borrow().clone()) {
            let editor_type_width = self.layout_editor_type_node(&editor_type, width, height);
            widget_dim = Some(((width - editor_type_width).max(0.0), height));
        }
        self.widget.layout(widget_dim);
    }

    /// O `offsetWidth` do nó do seletor de tipo: `padding: 0 4px 0 8px`, o rótulo, o chevron de 13px com `margin-left: 2px`,
    /// no máximo 40% do controle. Posiciona o nó à direita e devolve a largura.
    fn layout_editor_type_node(&self, editor_type: &EditorTypeControl, container_width: f64, height: f64) -> f64 {
        const FONT_SIZE: f32 = 11.0;
        const CHEVRON_SIZE: f64 = 13.0;
        let style = ui_font(FONT_SIZE, 400);
        let line_height = natural_line_height(&style);
        let text = editor_type.text.borrow().clone();
        let natural_label_width = je_widgets::text::measure_text(&text, &style);
        let max_width = container_width * 0.4;
        let natural_width = 8.0 + natural_label_width + 2.0 + CHEVRON_SIZE + 4.0;
        let width = natural_width.min(max_width);
        let label_width = (width - (8.0 + 2.0 + CHEVRON_SIZE + 4.0)).max(0.0).min(natural_label_width);

        let foreground = self.editor_type_foreground();
        let displayed = je_widgets::text::ellipsize(&text, &style, label_width);
        let mut tree = self.services.ui.tree_mut();
        tree.set_bounds(editor_type.node, Rect::new(container_width - width, 0.0, container_width, height));
        let label_top = (height - line_height) / 2.0;
        tree.set_bounds(editor_type.label, Rect::new(8.0, label_top, 8.0 + label_width, label_top + line_height));
        set_node_text(&mut tree, editor_type.label, &displayed, &style, foreground, line_height);
        let chevron_left = 8.0 + label_width + 2.0;
        let chevron_top = (height - CHEVRON_SIZE) / 2.0;
        tree.set_bounds(editor_type.chevron, Rect::new(chevron_left, chevron_top, chevron_left + CHEVRON_SIZE, chevron_top + CHEVRON_SIZE));
        tree.style_mut(editor_type.chevron).content = Content::Codicon { id: "chevron-down".to_owned(), size: CHEVRON_SIZE as f32, color: foreground };
        width
    }

    /// `color: var(--vscode-breadcrumb-foreground)`.
    fn editor_type_foreground(&self) -> Rgba {
        let theme = self.services.theme_service.get_color_theme();
        theme.get_color("breadcrumb.foreground", None).map(|color| to_rgba8(&color)).unwrap_or(Rgba::opaque(0xCC, 0xCC, 0xCC))
    }

    /// `isHidden()`.
    pub fn is_hidden(&self) -> bool {
        self.services.ui.tree().has_class(self.dom_node, "hidden")
    }

    fn toggle_hidden(&self, hidden: bool) {
        let mut tree = self.services.ui.tree_mut();
        tree.toggle_class(self.dom_node, "hidden", hidden);
        tree.style_mut(self.dom_node).visible = !hidden;
    }

    /// `hide()`.
    pub fn hide(&self) {
        let was_hidden = self.is_hidden();

        self.breadcrumbs_disposables.clear();
        self.ck_breadcrumbs_visible.set(Dynamic::Bool(false));
        self.ck_breadcrumbs_has_symbols.set(Dynamic::Bool(false));
        self.toggle_hidden(true);
        self.hide_editor_type_control();

        if !was_hidden {
            self.on_did_visibility_change.fire(());
        }
    }

    fn show(&self) {
        let was_hidden = self.is_hidden();

        self.ck_breadcrumbs_visible.set(Dynamic::Bool(true));
        self.toggle_hidden(false);

        if was_hidden {
            self.on_did_visibility_change.fire(());
        }
    }

    /// `revealLast()`.
    pub fn reveal_last(&self) {
        self.widget.reveal_last();
    }

    /// `update()`: devolve se a visibilidade mudou.
    pub fn update(self: &Rc<Self>) -> bool {
        self.breadcrumbs_disposables.clear();

        // honor diff editors and such
        let active_editor = self.editor_group.active_editor().map(EditorInputOrUntyped::Typed);
        let uri_options = EditorResourceAccessorOptions { support_side_by_side: Some(SideBySideEditor::Primary), filter_by_scheme: None };
        let canonical_uri = EditorResourceAccessor::get_canonical_uri(active_editor.as_ref(), Some(&uri_options));
        let original_uri = EditorResourceAccessor::get_original_uri(active_editor.as_ref(), Some(&uri_options));
        let uri = original_uri.or(canonical_uri);
        let was_hidden = self.is_hidden();

        let uri = match uri {
            Some(uri) if self.services.file_service.has_provider(&uri) => uri,
            _ => {
                // cleanup and return when there is no input or when we cannot handle this input
                self.ck_breadcrumbs_possible.set(Dynamic::Bool(false));
                self.ck_breadcrumbs_has_symbols.set(Dynamic::Bool(false));
                if !was_hidden {
                    self.hide();
                    return true;
                }
                return false;
            }
        };

        self.show();
        self.ck_breadcrumbs_possible.set(Dynamic::Bool(true));
        self.update_editor_type_control();

        let model = BreadcrumbsModel::new(uri.clone(), self.editor_group.active_editor_pane(), self.services.model_services.clone());
        self.model.set(Some(model.clone()));

        let backslash = self.services.label_service.get_separator(&uri.scheme().to_string_lossy(), Some(uri.authority().to_string_lossy().as_str())).as_str() == "\\";
        self.services.ui.tree_mut().toggle_class(self.dom_node, "backslash-path", backslash);

        let update_breadcrumbs: Rc<dyn Fn()> = {
            let (this, model) = (Rc::downgrade(self), model.clone());
            Rc::new(move || {
                if let Some(control) = this.upgrade() {
                    control.update_breadcrumbs(&model);
                }
            })
        };
        let listener = model.on_did_update().listen({
            let update_breadcrumbs = update_breadcrumbs.clone();
            move |_| update_breadcrumbs()
        });
        let config_listener = self.cf_show_icons.on_did_change().listen({
            let update_breadcrumbs = update_breadcrumbs.clone();
            move |_| update_breadcrumbs()
        });
        update_breadcrumbs();
        self.breadcrumbs_disposables.clear();
        self.breadcrumbs_disposables.add(listener);
        let model_holder = self.model.clone();
        self.breadcrumbs_disposables.add(to_disposable(move || model_holder.clear()));
        self.breadcrumbs_disposables.add(config_listener);
        let widget = self.widget.clone();
        self.breadcrumbs_disposables.add(to_disposable(move || widget.set_items(Vec::new())));

        let update_scrollbar_sizing: Rc<dyn Fn()> = {
            let this = Rc::downgrade(self);
            Rc::new(move || {
                let Some(control) = this.upgrade() else { return };
                let sizing = control.cf_title_scrollbar_sizing.get_value(None).unwrap_or_else(|| "default".to_owned());
                let visibility = control.cf_title_scrollbar_visibility.get_value(None).unwrap_or_else(|| "auto".to_owned());
                if let Some(size) = Self::scrollbar_size(&sizing) {
                    control.widget.set_horizontal_scrollbar_size(size);
                }
                if let Some(visibility) = Self::scrollbar_visibility(&visibility) {
                    control.widget.set_horizontal_scrollbar_visibility(visibility);
                }
            })
        };
        update_scrollbar_sizing();
        let update_scrollbar_size_listener = self.cf_title_scrollbar_sizing.on_did_change().listen({
            let update = update_scrollbar_sizing.clone();
            move |_| update()
        });
        let update_scrollbar_visibility_listener = self.cf_title_scrollbar_visibility.on_did_change().listen({
            let update = update_scrollbar_sizing.clone();
            move |_| update()
        });
        self.breadcrumbs_disposables.add(update_scrollbar_size_listener);
        self.breadcrumbs_disposables.add(update_scrollbar_visibility_listener);

        // close picker on hide/update
        let this = Rc::downgrade(self);
        self.breadcrumbs_disposables.add(to_disposable(move || {
            if let Some(control) = this.upgrade()
                && control.breadcrumbs_picker_showing.get()
            {
                control.services.context_view_service.hide_context_view(Some(Rc::new(PickerHideData { source: Some(control.source_id()), ..Default::default() })));
            }
        }));

        was_hidden != self.is_hidden()
    }

    /// O `source: this` do `hideContextView`.
    fn source_id(&self) -> usize {
        self.this.as_ptr() as usize
    }

    /// O corpo de `updateBreadcrumbs` de `update`.
    fn update_breadcrumbs(self: &Rc<Self>, model: &Rc<BreadcrumbsModel>) {
        self.services.ui.tree_mut().toggle_class(self.dom_node, "relative-path", model.is_relative());
        let show_icons = self.cf_show_icons.get_value(None).unwrap_or(false);
        let options = BreadcrumbsControlOptions { show_file_icons: self.options.show_file_icons && show_icons, show_symbol_icons: self.options.show_symbol_icons && show_icons, ..self.options };
        let elements = model.get_elements();
        self.ck_breadcrumbs_has_symbols.set(Dynamic::Bool(elements.iter().any(|element| !matches!(element, BreadcrumbsElement::File(_)))));
        let items: Vec<BreadcrumbsItemRef> = elements
            .into_iter()
            .map(|element| match element {
                BreadcrumbsElement::File(element) => FileItem::new(model.clone(), element, options, self.labels.clone(), self.hover_delegate.clone()) as BreadcrumbsItemRef,
                BreadcrumbsElement::Outline(element) => OutlineItem::new(model.clone(), element, options) as BreadcrumbsItemRef,
            })
            .collect();
        if items.is_empty() {
            self.widget.set_enabled(false);
            self.widget.set_items(vec![Rc::new(EmptyItem)]);
        } else {
            self.widget.set_enabled(true);
            let last = items[items.len() - 1].clone();
            self.widget.set_items(items);
            self.widget.reveal(&last);
        }
    }

    /// `_updateEditorTypeControl()`.
    fn update_editor_type_control(&self) {
        let previous_width = self.editor_type_width();

        let available = if self.options.show_editor_type_picker && self.cf_show_editor_type.get_value(None).unwrap_or(false) { self.get_available_editor_types() } else { None };
        match available {
            None => self.hide_editor_type_control(),
            Some(available) => {
                let editor_type = self.create_editor_type_control();
                let current = available.editors.iter().find(|editor| editor.id == available.current_id);
                let label = current.map_or_else(|| available.current_id.clone(), |current| editor_type_display_label(current, available.is_diff_editor));
                *editor_type.text.borrow_mut() = label.clone();
                editor_type.hover.update(ManagedHoverContentOrFactory::from(localize("editorType.hover", "Editor: {0}", &[label.as_str()]).as_str()), None);
            }
        }

        // The dropdown width may have changed (different editor label or visibility toggled). Since the breadcrumbs widget
        // uses an explicit pixel width that reserves room for the dropdown, re-run the layout so the widget shrinks/grows to
        // match the new dropdown width.
        if let Some(last) = self.last_layout_dimension.get() {
            // Antes do primeiro layout a largura ainda é zero; o `offsetWidth` do original só muda depois dele.
            let editor_type = self.editor_type.borrow().clone();
            if let Some(editor_type) = editor_type {
                let current_width = self.layout_editor_type_node(&editor_type, last.0, last.1);
                if current_width != previous_width {
                    self.layout(Some(last));
                }
            } else if previous_width != 0.0 {
                self.layout(Some(last));
            }
        }
    }

    /// O `offsetWidth` do nó do seletor de tipo (0 sem ele).
    fn editor_type_width(&self) -> f64 {
        match self.editor_type.borrow().as_ref() {
            Some(editor_type) => self.services.ui.tree().bounds(editor_type.node).width(),
            None => 0.0,
        }
    }

    /// `_getAvailableEditorTypes()`.
    fn get_available_editor_types(&self) -> Option<IAvailableEditorTypes> {
        let hidden = match self.services.configuration_service.get_value(Some(HIDDEN_EDITOR_TYPES_SETTING_ID), None) {
            Dynamic::Array(items) => Some(items.to_vec().into_iter().filter_map(|item| if let Dynamic::String(text) = item { Some(text.to_string_lossy()) } else { None }).collect::<Vec<String>>()),
            _ => None,
        };
        get_available_editor_types(self.editor_group.active_editor().as_ref(), &*self.services.editor_resolver_service, hidden.as_deref())
    }

    /// `_createEditorTypeControl()`.
    fn create_editor_type_control(&self) -> Rc<EditorTypeControl> {
        if let Some(editor_type) = self.editor_type.borrow().clone() {
            return editor_type;
        }

        let (node, label, chevron) = {
            let mut tree = self.services.ui.tree_mut();
            let node = create_el(&mut tree, self.dom_node, Role::Button, "breadcrumbs-editor-type");
            let label = create_el(&mut tree, node, Role::Generic, "label");
            let chevron = create_el(&mut tree, node, Role::Generic, "codicon codicon-chevron-down");
            (node, label, chevron)
        };
        let hover = self.services.hover_service.setup_managed_hover(get_default_hover_delegate(HoverPlacement::Mouse), node, ManagedHoverContentOrFactory::from(""), None);
        let hover_disposable: DisposableRef = hover.clone();
        self.editor_type_disposables.add(hover_disposable);
        let this = self.this.clone();
        self.editor_type_disposables.add(self.services.ui.listen(node, EventKind::Click, false, move |event| {
            event.stop(true);
            if let Some(control) = this.upgrade() {
                control.show_editor_type_picker();
            }
        }));
        let editor_type = Rc::new(EditorTypeControl { node, label, chevron, hover, text: RefCell::new(String::new()) });
        *self.editor_type.borrow_mut() = Some(editor_type.clone());
        editor_type
    }

    /// `_hideEditorTypeControl()`.
    fn hide_editor_type_control(&self) {
        self.editor_type_disposables.clear();
        if let Some(editor_type) = self.editor_type.borrow_mut().take() {
            self.services.ui.remove(editor_type.node);
        }
    }

    /// `_showEditorTypePicker()`.
    fn show_editor_type_picker(&self) {
        let Some(editor_type) = self.editor_type.borrow().clone() else { return };
        let Some(available) = self.get_available_editor_types() else { return };
        let actions = create_editor_type_actions(
            &available,
            self.editor_group.clone(),
            self.services.editor_resolver_service.clone(),
            self.services.command_service.clone(),
            self.services.editor_service.clone(),
        );
        self.services.context_menu_service.show_context_menu(ContextMenuRequest::Delegate(Rc::new(EditorTypeMenuDelegate { anchor: editor_type.node, actions })));
    }

    /// `_onFocusEvent(event)`.
    fn on_focus_event(&self, event: &IBreadcrumbsItemEvent) {
        if let Some(item) = &event.item
            && self.breadcrumbs_picker_showing.get()
        {
            *self.breadcrumbs_picker_ignore_once_item.borrow_mut() = None;
            self.widget.set_selection(Some(item), None);
        }
    }

    /// `_updateCkBreadcrumbsActive()`.
    fn update_ck_breadcrumbs_active(&self) {
        let value = self.widget.is_dom_focused() || self.breadcrumbs_picker_showing.get();
        self.ck_breadcrumbs_active.set(Dynamic::Bool(value));
    }
}

/// O delegado do menu de contexto do seletor de tipo (`{ getAnchor: () => editorTypeNode, getActions: () => actions }`).
struct EditorTypeMenuDelegate {
    anchor: NodeId,
    actions: Vec<ActionRef>,
}

impl IContextMenuDelegate for EditorTypeMenuDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        AnchorTarget::Element(node_element(self.anchor))
    }

    fn get_actions(&self) -> Vec<ActionRef> {
        self.actions.clone()
    }
}

/// O delegado do `showContextView` de `_onSelectEvent`.
struct PickerContextViewDelegate {
    control: Weak<BreadcrumbsControl>,
    event: IBreadcrumbsItemEvent,
    element: BreadcrumbsElement,
    resource: URI,
    is_file: bool,
    picker: RefCell<Option<Rc<BreadcrumbsPicker>>>,
    picker_anchor: Cell<Option<(f64, f64)>>,
}

impl IContextViewDelegate for PickerContextViewDelegate {
    fn render(&self, container: &Rc<ViewElement>) -> Option<DisposableRef> {
        let container = native_node(container)?;
        let control = self.control.upgrade()?;
        let services = control.services.picker_services.clone();
        let picker = if self.is_file {
            BreadcrumbsFilePicker::new(services, container, self.resource.clone())
        } else {
            BreadcrumbsOutlinePicker::new(services, container, self.resource.clone())
        };
        *self.picker.borrow_mut() = Some(picker.clone());

        let listeners = DisposableStore::new();
        let context_view_service = control.services.context_view_service.clone();
        let source = control.source_id();
        listeners.add(picker.on_will_pick_element().listen({
            let context_view_service = context_view_service.clone();
            move |side_by_side: &bool| context_view_service.hide_context_view(Some(Rc::new(PickerHideData { did_pick: true, side_by_side: *side_by_side, source: Some(source) })))
        }));

        // `PixelRatio.onDidChange` (o zoom que fecha o seletor) não existe: ver as notas do módulo.

        let focus_tracker = FocusTracker::new(&control.services.ui, container);
        let blur_listener = focus_tracker.on_did_blur().listen({
            let (this, item) = (self.control.clone(), self.event.item.clone());
            move |_| {
                let Some(control) = this.upgrade() else { return };
                *control.breadcrumbs_picker_ignore_once_item.borrow_mut() = if control.widget.is_dom_focused() { item.clone() } else { None };
                control.services.context_view_service.hide_context_view(Some(Rc::new(PickerHideData { source: Some(control.source_id()), ..Default::default() })));
            }
        });
        listeners.add(blur_listener);
        let tracker: DisposableRef = focus_tracker;
        listeners.add(tracker);

        control.breadcrumbs_picker_showing.set(true);
        control.update_ck_breadcrumbs_active();

        // O `picker` também é descartado em `on_hide` (como no original): o descarte aqui só solta as escutas.
        let listeners_ref: DisposableRef = listeners;
        Some(listeners_ref)
    }

    fn get_anchor(&self) -> AnchorTarget {
        if self.picker_anchor.get().is_none() {
            let Some(control) = self.control.upgrade() else {
                return AnchorTarget::Anchor(IAnchor { x: 0.0, y: 0.0, width: None, height: None });
            };
            let (window_width, window_height) = window_size(&control.services.ui);
            let rect = self.event.node.map(|node| control.services.ui.tree().absolute_bounds(node)).unwrap_or(Rect::ZERO);
            let mouse_x = self.event.payload.as_ref().and_then(|payload| payload.downcast_ref::<PointerData>()).map(|pointer| pointer.x);
            let placement = compute_picker_placement(window_width, window_height, ItemRect { left: rect.x0, top: rect.y0, width: rect.width(), height: rect.height() }, mouse_x);
            if let Some(picker) = self.picker.borrow().clone() {
                let _ = picker.show(self.element.clone(), placement.max_height, placement.width, placement.arrow_size, placement.arrow_offset);
            }
            self.picker_anchor.set(Some((placement.x, placement.y)));
        }
        let (x, y) = self.picker_anchor.get().unwrap_or((0.0, 0.0));
        AnchorTarget::Anchor(IAnchor { x, y, width: None, height: None })
    }

    fn on_hide(&self, data: Option<HideData>) {
        let data = hide_data_to::<PickerHideData>(data.as_ref());
        let picker = self.picker.borrow().clone();
        if let Some(picker) = &picker
            && (!data.is_some_and(|data| data.did_pick) || data.is_some_and(|data| data.side_by_side))
        {
            picker.restore_view_state();
        }
        if let Some(control) = self.control.upgrade() {
            control.breadcrumbs_picker_showing.set(false);
            control.update_ck_breadcrumbs_active();
            if data.is_some_and(|data| data.source == Some(control.source_id())) {
                control.widget.set_focused(None, None);
                control.widget.set_selection(None, None);
            }
        }
        if let Some(picker) = picker {
            picker.dispose();
        }
    }
}

impl BreadcrumbsControl {
    /// `_onSelectEvent(event)`.
    fn on_select_event(self: &Rc<Self>, event: &IBreadcrumbsItemEvent) {
        let Some(item) = event.item.clone() else { return };

        let ignored = self.breadcrumbs_picker_ignore_once_item.borrow().clone();
        if ignored.is_some_and(|ignored| std::ptr::addr_eq(Rc::as_ptr(&ignored), Rc::as_ptr(&item))) {
            *self.breadcrumbs_picker_ignore_once_item.borrow_mut() = None;
            self.widget.set_focused(None, None);
            self.widget.set_selection(None, None);
            return;
        }

        let element = if let Some(file_item) = item.as_any().downcast_ref::<FileItem>() {
            BreadcrumbsElement::File(file_item.element.clone())
        } else if let Some(outline_item) = item.as_any().downcast_ref::<OutlineItem>() {
            BreadcrumbsElement::Outline(outline_item.element.clone())
        } else {
            return;
        };
        // Activate the owning group without letting delayed webview focus steal focus from the picker.
        self.editor_group.groups_view().activate_group(&self.editor_group);

        let group = editor_group_target(event.payload.as_ref());
        if group.is_some() {
            // reveal the item
            self.widget.set_focused(None, None);
            self.widget.set_selection(None, None);
            self.reveal_in_editor(event, &element, group, false);
            return;
        }

        if self.cf_use_quick_pick.get_value(None).unwrap_or(false) {
            // using quick pick
            self.widget.set_focused(None, None);
            self.widget.set_selection(None, None);
            self.services.quick_input_service.quick_access().show(if matches!(element, BreadcrumbsElement::Outline(_)) { "@" } else { "" }, None);
            return;
        }

        // show picker
        let resource = if let Some(file_item) = item.as_any().downcast_ref::<FileItem>() {
            file_item.model.resource.clone()
        } else if let Some(outline_item) = item.as_any().downcast_ref::<OutlineItem>() {
            outline_item.model.resource.clone()
        } else {
            return;
        };
        let is_file = matches!(element, BreadcrumbsElement::File(_));
        let delegate = Rc::new(PickerContextViewDelegate { control: Rc::downgrade(self), event: event.clone(), element, resource, is_file, picker: RefCell::new(None), picker_anchor: Cell::new(None) });
        self.services.context_view_service.show_context_view(delegate, None, false);
    }

    /// `_revealInEditor(event, element, group, pinned)`.
    fn reveal_in_editor(&self, event: &IBreadcrumbsItemEvent, element: &BreadcrumbsElement, group: Option<EditorGroupTarget>, pinned: bool) {
        match element {
            BreadcrumbsElement::File(file) => {
                if file.kind == FileKind::File {
                    self.editor_group.focus();
                    let options = TextEditorOptions::from(EditorOptions { pinned: Some(pinned), ..EditorOptions::default() });
                    let preferred = group.map(|group| match group {
                        EditorGroupTarget::Side => PreferredGroup::Side,
                        EditorGroupTarget::Active => PreferredGroup::Active,
                    });
                    let _ = self.services.editor_service.open_editor_in(UntypedEditorInput { resource: Some(file.uri.clone()), options: Some(options), ..UntypedEditorInput::default() }, preferred);
                } else {
                    // show next picker
                    let items = self.widget.get_items();
                    let next = event.item.as_ref().and_then(|current| items.iter().position(|item| std::ptr::addr_eq(Rc::as_ptr(item), Rc::as_ptr(current)))).and_then(|idx| items.get(idx + 1));
                    self.widget.set_focused(next, None);
                    self.widget.set_selection(next, Some(payload_pick()));
                }
            }
            BreadcrumbsElement::Outline(outline_element) => {
                self.editor_group.focus();
                // O original entrega o `OutlineElement2` ao `reveal` (ver as notas do módulo).
                let entry: OutlineElement = Rc::new(outline_element.clone());
                let _ = outline_element.outline.reveal(&entry, &EditorOptions { pinned: Some(pinned), ..EditorOptions::default() }, group == Some(EditorGroupTarget::Side), false);
            }
        }
    }
}

impl IDisposable for BreadcrumbsControl {
    fn dispose(&self) {
        BreadcrumbsControl::dispose(self);
    }
}

/// `BreadcrumbsControlFactory`.
pub struct BreadcrumbsControlFactory {
    container: NodeId,
    editor_group: Rc<dyn IEditorGroupView>,
    options: BreadcrumbsControlOptions,
    services: BreadcrumbsControlServices,
    disposables: Rc<DisposableStore>,
    control_disposables: Rc<DisposableStore>,
    control: RefCell<Option<Rc<BreadcrumbsControl>>>,
    on_did_enablement_change: Rc<Emitter<()>>,
    on_did_visibility_change: Rc<Emitter<()>>,
}

impl BreadcrumbsControlFactory {
    pub fn new(container: NodeId, editor_group: Rc<dyn IEditorGroupView>, options: BreadcrumbsControlOptions, services: BreadcrumbsControlServices) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let on_did_enablement_change = Emitter::new();
        disposables.add(on_did_enablement_change.clone());
        let on_did_visibility_change = Emitter::new();
        disposables.add(on_did_visibility_change.clone());
        let config = BreadcrumbsConfigKeys::IS_ENABLED.bind_to(services.configuration_service.clone());
        let config_disposable: DisposableRef = config.clone();
        disposables.add(config_disposable);

        let factory = Rc::new(Self {
            container,
            editor_group,
            options,
            services,
            disposables,
            control_disposables: DisposableStore::new(),
            control: RefCell::new(None),
            on_did_enablement_change,
            on_did_visibility_change,
        });

        let this = Rc::downgrade(&factory);
        let is_enabled = {
            let config = config.clone();
            let editor_group = factory.editor_group.clone();
            move || config.get_value(None).unwrap_or(false) && editor_group.groups_view().part_options().show_breadcrumbs != Some(false)
        };
        let update_control = {
            let (this, is_enabled) = (this.clone(), Rc::new(is_enabled.clone()));
            Rc::new(move || {
                let Some(factory) = this.upgrade() else { return };
                let enabled = is_enabled();
                let has_control = factory.control.borrow().is_some();
                if !enabled && has_control {
                    factory.control_disposables.clear();
                    *factory.control.borrow_mut() = None;
                    factory.on_did_enablement_change.fire(());
                } else if enabled && !has_control {
                    let control = factory.create_control();
                    *factory.control.borrow_mut() = Some(control.clone());
                    control.update();
                    factory.on_did_enablement_change.fire(());
                }
            })
        };

        factory.disposables.add(config.on_did_change().listen({
            let update_control = update_control.clone();
            move |_| update_control()
        }));
        factory.disposables.add(factory.editor_group.groups_view().on_did_change_editor_part_options().listen({
            let update_control = update_control.clone();
            move |event| {
                if event.old_part_options.show_breadcrumbs != event.new_part_options.show_breadcrumbs {
                    update_control();
                }
            }
        }));

        if is_enabled() {
            let control = factory.create_control();
            *factory.control.borrow_mut() = Some(control);
        }

        factory.disposables.add(factory.services.file_service.on_did_change_file_system_provider_registrations().listen({
            let this = this.clone();
            move |event| {
                let Some(factory) = this.upgrade() else { return };
                let control = factory.control.borrow().clone();
                if let Some(control) = &control
                    && let Some(model) = control.model()
                    && model.resource.scheme().to_string_lossy() != event.scheme
                {
                    // ignore if the scheme of the breadcrumbs resource is not affected
                    return;
                }
                if control.is_some_and(|control| control.update()) {
                    factory.on_did_enablement_change.fire(());
                }
            }
        }));

        factory
    }

    /// `control`.
    pub fn control(&self) -> Option<Rc<BreadcrumbsControl>> {
        self.control.borrow().clone()
    }

    /// `onDidEnablementChange`.
    pub fn on_did_enablement_change(&self) -> Event<()> {
        self.on_did_enablement_change.event()
    }

    /// `onDidVisibilityChange`.
    pub fn on_did_visibility_change(&self) -> Event<()> {
        self.on_did_visibility_change.event()
    }

    fn create_control(&self) -> Rc<BreadcrumbsControl> {
        let control = BreadcrumbsControl::new(self.container, self.options, self.editor_group.clone(), self.services.clone());
        let control_disposable: DisposableRef = control.clone();
        self.control_disposables.add(control_disposable);
        let on_did_visibility_change = self.on_did_visibility_change.clone();
        self.control_disposables.add(control.on_did_visibility_change().listen(move |_| on_did_visibility_change.fire(())));
        control
    }

    pub fn dispose(&self) {
        self.disposables.dispose();
        self.control_disposables.dispose();
    }
}

//#region commands

/// `breadcrumbs.focusAndSelect` e `breadcrumbs.focus`: find widget and focus/select.
fn focus_and_select_handler(accessor: &je_instantiation::ServicesAccessor, select: bool) {
    let groups = accessor.get(I_EDITOR_GROUPS_SERVICE);
    let breadcrumbs = accessor.get(I_BREADCRUMBS_SERVICE);
    let Some(widget) = breadcrumbs.get_widget(groups.active_group().id()) else { return };
    let item = widget.get_items().last().cloned();
    widget.set_focused(item.as_ref(), None);
    if select {
        widget.set_selection(item.as_ref(), Some(payload_pick()));
    }
}

/// O widget do grupo ativo (`breadcrumbs.getWidget(groups.activeGroup.id)`).
fn active_widget(accessor: &je_instantiation::ServicesAccessor) -> Option<Rc<BreadcrumbsWidget>> {
    let groups = accessor.get(I_EDITOR_GROUPS_SERVICE);
    accessor.get(I_BREADCRUMBS_SERVICE).get_widget(groups.active_group().id())
}

/// `WorkbenchListFocusContextKey`: `listFocus && !inputFocus && !treestickyScrollFocused`.
fn workbench_list_focus_context_key() -> ContextKeyExpression {
    ContextKeyExpr::and([Some(ContextKeyExpr::has("listFocus")), Some(ContextKeyExpr::not("inputFocus")), Some(ContextKeyExpr::has("treestickyScrollFocused").negate())]).expect("a expressão tem termos")
}

fn visible_and_active() -> Option<ContextKeyExpression> {
    ContextKeyExpr::and([BreadcrumbsControl::ck_breadcrumbs_visible().expr(), BreadcrumbsControl::ck_breadcrumbs_active().expr()].map(Some))
}

fn visible_active_and_list_focused() -> Option<ContextKeyExpression> {
    ContextKeyExpr::and([BreadcrumbsControl::ck_breadcrumbs_visible().expr(), BreadcrumbsControl::ck_breadcrumbs_active().expr(), workbench_list_focus_context_key()].map(Some))
}

fn primary_secondary(primary: u32, secondary: Vec<u32>, mac: Option<PlatformKeybindings>) -> Keybindings {
    Keybindings { primary: Some(primary), secondary: Some(secondary), mac, ..Default::default() }
}

fn register_rule(id: &str, weight: i64, when: Option<ContextKeyExpression>, keybindings: Keybindings, handler: CommandHandler) -> DisposableRef {
    keybindings_registry().register_command_and_keybinding_rule(CommandAndKeybindingRule { rule: KeybindingRule { keybindings, id: id.to_owned(), weight, args: Dynamic::Undefined, when }, handler, metadata: None })
}

fn localized_title(key: &str, message: &str) -> Title {
    let localized = localize2(key, message);
    Title::Localized(CommandActionTitle { value: localized.value, original: localized.original, mnemonic_title: None })
}

fn category_view() -> Category {
    Category::Localized(localize2("view", "View"))
}

fn menu_item(id: je_platform::actions::MenuId, group: Option<&str>, order: Option<f64>, when: Option<ContextKeyExpression>) -> Action2MenuItem {
    Action2MenuItem { id, null_precondition: false, alt: None, when, group: group.map(str::to_owned), order, is_hidden_by_default: None }
}

/// `ToggleBreadcrumb`.
struct ToggleBreadcrumb;

impl Action2 for ToggleBreadcrumb {
    fn desc(&self) -> Action2Options {
        let toggled = je_platform::action::CommandActionToggleInfo {
            condition: ContextKeyExpr::equals("config.breadcrumbs.enabled", true),
            icon: None,
            tooltip: None,
            title: Some(localize("cmd.toggle2", "Breadcrumbs", &[])),
            mnemonic_title: Some(localize("miBreadcrumbs2", "&&Breadcrumbs", &[])),
        };
        Action2Options {
            command: CommandAction {
                short_title: Some(localized_title("cmd.toggle.short", "Breadcrumbs")),
                category: Some(category_view()),
                toggled: Some(Toggled::Info(toggled)),
                ..CommandAction::new("breadcrumbs.toggle", localized_title("cmd.toggle", "Toggle Breadcrumbs"))
            },
            menu: vec![
                menu_item(command_palette(), None, None, None),
                menu_item(menubar_appearance_menu(), Some("4_editor"), Some(2.0), None),
                menu_item(notebook_toolbar(), Some("notebookLayout"), Some(2.0), None),
                menu_item(sticky_scroll_context(), None, None, None),
                menu_item(notebook_sticky_scroll_context(), Some("notebookView"), Some(2.0), None),
                menu_item(notebook_toolbar_context(), Some("notebookView"), Some(2.0), None),
            ],
            keybinding: Vec::new(),
            f1: false,
        }
    }

    fn run(&self, accessor: &je_instantiation::ServicesAccessor, _args: &[Dynamic]) {
        let config = accessor.get(I_CONFIGURATION_SERVICE);
        let bread_crumbs_config = BreadcrumbsConfigKeys::IS_ENABLED.bind_to(config);
        let value = bread_crumbs_config.get_value(None).unwrap_or(false);
        let _ = bread_crumbs_config.update_value(&!value, None);
        bread_crumbs_config.dispose();
    }
}

/// `FocusAndSelectBreadcrumbs` (`select`) e `FocusBreadcrumbs`.
struct FocusBreadcrumbsAction {
    select: bool,
}

impl Action2 for FocusBreadcrumbsAction {
    fn desc(&self) -> Action2Options {
        let (id, title_key, title, key) = if self.select {
            ("breadcrumbs.focusAndSelect", "cmd.focusAndSelect", "Focus and Select Breadcrumbs", KeyCode::Period)
        } else {
            ("breadcrumbs.focus", "cmd.focus", "Focus Breadcrumbs", KeyCode::Semicolon)
        };
        Action2Options {
            command: CommandAction { precondition: Some(BreadcrumbsControl::ck_breadcrumbs_visible().expr()), ..CommandAction::new(id, localized_title(title_key, title)) },
            menu: Vec::new(),
            keybinding: vec![je_platform::actions::Action2Keybinding {
                keybindings: Keybindings { primary: Some(KeyMod::CTRL_CMD | KeyMod::SHIFT | key.bits()), ..Default::default() },
                weight: KeybindingWeight::WORKBENCH_CONTRIB,
                args: Dynamic::Undefined,
                when: Some(BreadcrumbsControl::ck_breadcrumbs_possible().expr()),
            }],
            f1: true,
        }
    }

    fn run(&self, accessor: &je_instantiation::ServicesAccessor, _args: &[Dynamic]) {
        focus_and_select_handler(accessor, self.select);
    }
}

/// `CopyBreadcrumbPath`.
struct CopyBreadcrumbPath;

impl Action2 for CopyBreadcrumbPath {
    fn desc(&self) -> Action2Options {
        let possible_and_symbols = ContextKeyExpr::and([Some(BreadcrumbsControl::ck_breadcrumbs_possible().expr()), Some(BreadcrumbsControl::ck_breadcrumbs_has_symbols().expr())]);
        Action2Options {
            command: CommandAction {
                category: Some(category_view()),
                precondition: ContextKeyExpr::and([Some(BreadcrumbsControl::ck_breadcrumbs_visible().expr()), Some(BreadcrumbsControl::ck_breadcrumbs_has_symbols().expr())]),
                ..CommandAction::new("breadcrumbs.copyPath", localized_title("cmd.copyPath", "Copy Breadcrumbs Path"))
            },
            menu: vec![menu_item(editor_title_context(), Some("1_cutcopypaste"), Some(100.0), possible_and_symbols)],
            keybinding: Vec::new(),
            f1: true,
        }
    }

    fn run(&self, accessor: &je_instantiation::ServicesAccessor, _args: &[Dynamic]) {
        let groups = accessor.get(I_EDITOR_GROUPS_SERVICE);
        let clipboard_service = accessor.get(I_CLIPBOARD_SERVICE);
        let configuration_service = accessor.get(I_CONFIGURATION_SERVICE);
        let outline_service = accessor.get(I_OUTLINE_SERVICE);

        let Some(pane) = groups.active_group().active_editor_pane() else { return };
        let outline_promise = outline_service.create_outline(pane.clone(), OutlineTarget::BREADCRUMBS, CancellationToken::None);
        let _ = async_fn(async move {
            let Some(outline) = outline_promise.await? else { return Ok(()) };

            let elements = outline.config().breadcrumbs_data_source.get_breadcrumb_elements();
            let labels: Vec<String> = elements.iter().map(|item| item.label.clone()).filter(|label| !label.is_empty()).collect();

            outline.dispose();

            if labels.is_empty() {
                return Ok(());
            }

            // Get separator with language override support
            let resource = pane.input().and_then(|input| input.resource());
            let config = BreadcrumbsConfigKeys::SYMBOL_PATH_SEPARATOR.bind_to(configuration_service);
            let overrides = resource.map(|resource| ConfigurationOverrides { resource: Some(resource), ..Default::default() });
            let separator = config.get_value(overrides.as_ref()).unwrap_or_else(|| ".".to_owned());
            config.dispose();

            let path = labels.join(&separator);
            clipboard_service.write_text(&path).await
        });
    }
}

/// Registra os comandos de `//#region commands` e o `breadcrumbs.copyPath` (o original os registra ao carregar o módulo).
pub fn register_breadcrumbs_commands() -> DisposableRef {
    let registrations = DisposableStore::new();
    let weight = KeybindingWeight::WORKBENCH_CONTRIB;
    let ctrl_cmd = KeyMod::CTRL_CMD;
    let alt_arrow = |key: KeyCode| Some(PlatformKeybindings { primary: Some(key.bits()), secondary: Some(vec![KeyMod::ALT | key.bits()]) });
    let mac_alt = |key: KeyCode| Some(PlatformKeybindings { primary: Some(KeyMod::ALT | key.bits()), secondary: None });

    // toggle command
    registrations.add(register_action2(Rc::new(ToggleBreadcrumb)));

    // focus/focus-and-select
    registrations.add(register_action2(Rc::new(FocusBreadcrumbsAction { select: true })));
    registrations.add(register_action2(Rc::new(FocusBreadcrumbsAction { select: false })));

    // this commands is only enabled when breadcrumbs are disabled which it then enables and focuses
    registrations.add(register_rule(
        "breadcrumbs.toggleToOn",
        weight,
        Some(ContextKeyExpr::not("config.breadcrumbs.enabled")),
        Keybindings { primary: Some(ctrl_cmd | KeyMod::SHIFT | KeyCode::Period.bits()), ..Default::default() },
        Rc::new(|accessor, _| {
            let config = accessor.get(I_CONFIGURATION_SERVICE);
            // check if enabled and iff not enable
            let is_enabled = BreadcrumbsConfigKeys::IS_ENABLED.bind_to(config);
            let enable = if !is_enabled.get_value(None).unwrap_or(false) { Some(is_enabled.update_value(&true, None)) } else { None };
            let instantiation = accessor.get(je_instantiation::I_INSTANTIATION_SERVICE);
            let is_enabled_for_dispose = is_enabled.clone();
            let promise = async_fn(async move {
                if let Some(enable) = enable {
                    enable.await?;
                    timeout(50).await?; // hacky - the widget might not be ready yet...
                }
                is_enabled_for_dispose.dispose();
                instantiation.invoke_function(|accessor| focus_and_select_handler(accessor, true));
                Ok(Dynamic::Undefined)
            });
            let _ = promise;
            Dynamic::Undefined
        }),
    ));

    // navigation
    registrations.add(register_rule(
        "breadcrumbs.focusNext",
        weight,
        visible_and_active(),
        primary_secondary(KeyCode::RightArrow.bits(), vec![ctrl_cmd | KeyCode::RightArrow.bits()], mac_alt(KeyCode::RightArrow).map(|mac| PlatformKeybindings { primary: Some(KeyCode::RightArrow.bits()), secondary: Some(vec![KeyMod::ALT | KeyCode::RightArrow.bits()]), ..mac })),
        Rc::new(|accessor, _| {
            if let Some(widget) = active_widget(accessor) {
                widget.focus_next(None);
            }
            Dynamic::Undefined
        }),
    ));
    registrations.add(register_rule(
        "breadcrumbs.focusPrevious",
        weight,
        visible_and_active(),
        primary_secondary(KeyCode::LeftArrow.bits(), vec![ctrl_cmd | KeyCode::LeftArrow.bits()], alt_arrow(KeyCode::LeftArrow).map(|mac| PlatformKeybindings { secondary: Some(vec![KeyMod::ALT | KeyCode::LeftArrow.bits()]), ..mac })),
        Rc::new(|accessor, _| {
            if let Some(widget) = active_widget(accessor) {
                widget.focus_prev(None);
            }
            Dynamic::Undefined
        }),
    ));
    registrations.add(register_rule(
        "breadcrumbs.focusNextWithPicker",
        weight + 1,
        visible_active_and_list_focused(),
        Keybindings { primary: Some(ctrl_cmd | KeyCode::RightArrow.bits()), mac: mac_alt(KeyCode::RightArrow), ..Default::default() },
        Rc::new(|accessor, _| {
            if let Some(widget) = active_widget(accessor) {
                widget.focus_next(None);
            }
            Dynamic::Undefined
        }),
    ));
    registrations.add(register_rule(
        "breadcrumbs.focusPreviousWithPicker",
        weight + 1,
        visible_active_and_list_focused(),
        Keybindings { primary: Some(ctrl_cmd | KeyCode::LeftArrow.bits()), mac: mac_alt(KeyCode::LeftArrow), ..Default::default() },
        Rc::new(|accessor, _| {
            if let Some(widget) = active_widget(accessor) {
                widget.focus_prev(None);
            }
            Dynamic::Undefined
        }),
    ));
    registrations.add(register_rule(
        "breadcrumbs.selectFocused",
        weight,
        visible_and_active(),
        primary_secondary(KeyCode::Enter.bits(), vec![KeyCode::DownArrow.bits()], None),
        Rc::new(|accessor, _| {
            if let Some(widget) = active_widget(accessor) {
                widget.set_selection(widget.get_focused().as_ref(), Some(payload_pick()));
            }
            Dynamic::Undefined
        }),
    ));
    registrations.add(register_rule(
        "breadcrumbs.revealFocused",
        weight,
        visible_and_active(),
        primary_secondary(KeyCode::Space.bits(), vec![ctrl_cmd | KeyCode::Enter.bits()], None),
        Rc::new(|accessor, _| {
            if let Some(widget) = active_widget(accessor) {
                widget.set_selection(widget.get_focused().as_ref(), Some(payload_reveal()));
            }
            Dynamic::Undefined
        }),
    ));
    registrations.add(register_rule(
        "breadcrumbs.selectEditor",
        weight + 1,
        visible_and_active(),
        Keybindings { primary: Some(KeyCode::Escape.bits()), ..Default::default() },
        Rc::new(|accessor, _| {
            let groups = accessor.get(I_EDITOR_GROUPS_SERVICE);
            let breadcrumbs = accessor.get(I_BREADCRUMBS_SERVICE);
            let Some(widget) = breadcrumbs.get_widget(groups.active_group().id()) else { return Dynamic::Undefined };
            widget.set_focused(None, None);
            widget.set_selection(None, None);
            if let Some(pane) = groups.active_group().active_editor_pane() {
                pane.focus();
            }
            Dynamic::Undefined
        }),
    ));
    registrations.add(register_rule(
        "breadcrumbs.revealFocusedFromTreeAside",
        weight,
        visible_active_and_list_focused(),
        Keybindings { primary: Some(ctrl_cmd | KeyCode::Enter.bits()), ..Default::default() },
        Rc::new(|accessor, _| {
            let editors = accessor.get(I_EDITOR_SERVICE);
            let lists = accessor.get(I_LIST_SERVICE);

            let Some(tree) = lists.last_focused_list() else { return Dynamic::Undefined };
            let Some(input) = tree.workbench_data_tree_input() else { return Dynamic::Undefined };

            let Some(element) = tree.get_focused_element() else { return Dynamic::Undefined };

            let stat_resource = element
                .downcast_ref::<FileStat>()
                .map(|stat| stat.resource.clone())
                .or_else(|| element.downcast_ref::<super::breadcrumbs_picker::FileTreeElement>().and_then(|element| if let super::breadcrumbs_picker::FileTreeElement::Stat(stat) = element { Some(stat.resource.clone()) } else { None }));
            if let Some(resource) = stat_resource {
                // IFileStat: open file in editor
                let options = TextEditorOptions::from(EditorOptions { pinned: Some(true), ..EditorOptions::default() });
                let _ = editors.open_editor_in(UntypedEditorInput { resource: Some(resource), options: Some(options), ..UntypedEditorInput::default() }, Some(PreferredGroup::Side));
                return Dynamic::Undefined;
            }

            // IOutline: check if this the outline and iff so reveal element
            if let Some(input) = input
                && let Some(outline) = input.downcast_ref::<Rc<dyn IOutline>>()
            {
                let entry: OutlineElement = match element.downcast_ref::<OutlineTreeElement>() {
                    Some(tree_element) => tree_element.0.clone(),
                    None => element.clone(),
                };
                let _ = outline.reveal(&entry, &EditorOptions { pinned: Some(true), preserve_focus: Some(false), ..EditorOptions::default() }, true, false);
            }
            Dynamic::Undefined
        }),
    ));
    //#endregion

    registrations.add(register_action2(Rc::new(CopyBreadcrumbPath)));

    let registrations_ref: DisposableRef = registrations;
    registrations_ref
}
