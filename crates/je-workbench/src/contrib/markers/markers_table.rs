//! Porte de `src/vs/workbench/contrib/markers/browser/markersTable.ts`: a tabela do painel de problemas (`MarkersTable`), os
//! renderers de coluna (severidade, código, mensagem, arquivo e origem), o delegado de alturas e o filtro que monta as linhas.
//!
//! A tabela é a [`WorkbenchTable`] (`platform/list/browser/listService.ts`, em [`crate::browser::workbench_table`]) sobre a
//! [`Table`](je_widgets::table::table_widget::Table) (`base/browser/ui/table/tableWidget.ts`, em `je-widgets`), que nasceram
//! com este arquivo. O que a tabela compartilha com a árvore (`markers_tree_viewer`) vem de lá: o cálculo de destaques
//! (`highlights`, `source_and_code_matches`), o filtro de severidade, o rótulo relativo do recurso, o código com link
//! (`render_code`, sobre o `Link` de `platform/opener/browser`), o ícone de severidade, o provedor do item de correção
//! rápida e os serviços (`MarkersTreeServices`, `MarkersTreeStyles`).
//!
//! Notas de tradução:
//! - **Injeção.** O `@IInstantiationService` que cria a tabela e os renderers é a construção direta; o `@ILabelService`, o
//!   `@IHoverService` e o `@IOpenerService` dos renderers e do filtro são os campos de [`MarkersTreeServices`] (o `ILabelService`
//!   da tabela é o mesmo), e os da `WorkbenchTable` são os [`WorkbenchTreeServices`].
//! - **`MarkerTableItem extends Marker`.** O item é um `Rc<MarkerTableItem>` (ver `markers_model`) e o `Marker` que o
//!   `markersViewModel.onMarkerMouseHover` e o `setMarkerSelection` usam é um `Marker` com os mesmos dados (o modelo de visão
//!   acha o `MarkerViewModel` pelo `id`).
//! - **CSS de `markers.css` (tabela).** As regras viram o `layout_cell` de cada coluna, que recebe a caixa de conteúdo da
//!   célula (sem o `padding-left: 10px`, que são as opções da tabela): `align-items: center` é a posição vertical; o `.severity`
//!   some e as ações aparecem (com `margin-left: -3px`) quando a célula é `quickFix` e a linha está com o ponteiro, a seleção
//!   ou o foco (as classes `hover`, `selected` e `focused` da linha); o `(` e o `)` do `::before` e `::after` do código são
//!   nós de texto; `.file-position` tem `margin-left: 6px` e opacidade 0,7. O sublinhado do código com link não existe.
//! - **`title`.** O `title` do elemento (a dica nativa) é uma dica gerenciada do ponteiro, atualizada a cada render.
//! - **Código.** O original mantém um `HighlightedLabel` do código e um `Link` por linha de template e troca o conteúdo; aqui o
//!   código é montado a cada elemento pelo `render_code` da árvore (que cria o `Link` quando há `target`) e seu nó sai no
//!   próximo render.
//! - **`onRowHover`.** O `mouseover` dos nós `.monaco-list-row` é o `onMouseMove` da tabela com o índice da linha (fora de uma
//!   linha vale `-1`, o que o `mouseleave` do original dá ao sair); o `mouseleave` é o `PointerLeave` do nó da lista.
//! - **Índice não encontrado.** `findMarkerIndex` devolve `-1`, que a lista recusa com `Invalid index -1`; aqui o índice ausente
//!   lança o mesmo erro.
//! - **`Number.POSITIVE_INFINITY` no `splice`.** É a quantidade de linhas atual.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::actions::{ActionRef, IAction};
use je_base::event::{Event, any};
use je_base::filters::Match;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::symbols::Delay;
use je_editor::range::Range;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::markers::marker_service::unsupported_schemas;
use je_platform::markers::markers::{MarkerCode, MarkerSeverity};
use je_render::kurbo::Rect;
use je_ui::{EventKind, NodeId, Role, Tree, Ui};
use je_widgets::action_bar::{ActionBar, IActionBarOptions, IActionOptions};
use je_widgets::action_view_items::IActionViewItemOptions;
use je_widgets::dom::create_el;
use je_widgets::highlighted_label::{HighlightedLabel, HighlightedLabelStyles};
use je_widgets::hover_api::{IManagedHover, ManagedHoverContentOrFactory};
use je_widgets::list::list_types::{IListRenderer, ListElementRenderDetails, ListEvent, TemplateData, list_error};
use je_widgets::table::table_types::{ITableColumn, ITableRenderer, ITableVirtualDelegate};

use crate::browser::resource_navigator::IOpenEvent;
use crate::browser::workbench_table::{IWorkbenchTableOptions, WorkbenchTable};
use crate::browser::workbench_tree::WorkbenchTreeServices;

use super::markers_filter_options::FilterOptions;
use super::markers_model::{Marker, MarkerTableItem, ResourceMarkers, compare_markers_by_uri};
use super::markers_tree_viewer::{
    CODICON_SIZE, CodeItem, MarkersTreeServices, MarkersViewModel, QuickFixItemSlot, ancestor_has_class, code_value, highlights, listen_show_quick_fixes, matches_severity, place,
    quick_fix_view_item_provider, relative_uri_label, render_code, row_selected_or_focused, set_severity_icon, setup_mouse_hover, source_and_code_matches, units,
};
use super::markers_view::{IProblemsWidget, ProblemsContextMenuEvent, ProblemsElement, ProblemsWidgetEvent};
use super::messages::Messages;

/// O elemento da tabela (`MarkerTableItem`).
type Item = Rc<MarkerTableItem>;

/// `padding-left: 10px` do `.monaco-table-th` e do `.monaco-table-td` (`markers.css`).
const CELL_PADDING_LEFT: f64 = 10.0;

/// `font-weight: 600` do `.monaco-table-th` (`markers.css`).
const HEADER_FONT_WEIGHT: u16 = 600;

/// `margin: 0 6px` do `.marker-icon`.
const MARKER_ICON_MARGIN: f64 = 6.0;

/// `height: 22px` do `.marker-icon`.
const MARKER_ICON_HEIGHT: f64 = 22.0;

/// `margin-left: -3px` do `.actions`.
const ACTIONS_MARGIN_LEFT: f64 = -3.0;

/// `margin-left: 6px` do `.file-position`.
const FILE_POSITION_MARGIN_LEFT: f64 = 6.0;

/// O `Rc<Marker>` de um item da tabela: um `Marker` com os mesmos dados (ver as notas do módulo).
fn marker_of(item: &MarkerTableItem) -> Rc<Marker> {
    Rc::new(Marker::new(item.base.id.clone(), item.base.marker.clone(), item.base.related_information.clone()))
}

/// O rótulo dos renderers: o realce em negrito (`.highlight { font-weight: bold }`).
fn cell_label_styles(services: &MarkersTreeServices) -> HighlightedLabelStyles {
    HighlightedLabelStyles { highlight_weight: 700, ..services.styles.label }
}

/// `.monaco-list-row:hover`, `.selected` ou `.focused` da linha que contém a célula.
fn row_active(tree: &Tree, cell: NodeId) -> bool {
    ancestor_has_class(tree, cell, "hover") || row_selected_or_focused(tree, cell)
}

/// O `title` de um elemento (ver as notas do módulo).
struct CellTitle(Rc<dyn IManagedHover>);

impl CellTitle {
    fn new(node: NodeId) -> Self {
        Self(setup_mouse_hover(node, ""))
    }

    fn set(&self, title: &str) {
        self.0.update(ManagedHoverContentOrFactory::from(title), None);
    }

    fn dispose(&self) {
        self.0.dispose();
    }
}

/// Os destaques de um campo do item, ou nenhum.
fn matches_or_empty(matches: &Option<Vec<Match>>) -> &[Match] {
    matches.as_deref().unwrap_or(&[])
}

// ---------------------------------------------------------------------------------------------------------------------
// MarkerSeverityColumnRenderer
// ---------------------------------------------------------------------------------------------------------------------

/// `IMarkerIconColumnTemplateData`.
struct SeverityTemplate {
    /// `.monaco-table-td`.
    cell: NodeId,
    /// `.severity`.
    severity_column: NodeId,
    icon: NodeId,
    icon_title: CellTitle,
    /// `.actions`.
    actions_container: NodeId,
    action_bar: Rc<ActionBar>,
    quick_fix_item: QuickFixItemSlot,
    element_disposables: Rc<DisposableStore>,
    content: Cell<Rect>,
}

/// O `.severity` (ou as ações) dentro da célula: ver as notas do módulo.
fn layout_severity(ui: &Ui, template: &SeverityTemplate) {
    let content = template.content.get();
    let (active, quick_fix) = {
        let tree = ui.tree();
        (row_active(&tree, template.cell), tree.has_class(template.cell, "quickFix"))
    };
    let show_actions = active && quick_fix;
    let actions_width: f64 = if show_actions { (0..template.action_bar.length()).map(|index| template.action_bar.get_width(index)).sum() } else { 0.0 };
    let icon_box_width = MARKER_ICON_MARGIN * 2.0 + CODICON_SIZE;
    let top = (content.height() - MARKER_ICON_HEIGHT) / 2.0;
    let mut x = content.x0;

    let mut tree = ui.tree_mut();
    tree.style_mut(template.severity_column).visible = !show_actions;
    place(&mut tree, template.severity_column, x, top, icon_box_width, MARKER_ICON_HEIGHT);
    place(&mut tree, template.icon, MARKER_ICON_MARGIN, (MARKER_ICON_HEIGHT - CODICON_SIZE) / 2.0, CODICON_SIZE, CODICON_SIZE);
    if !show_actions {
        x += icon_box_width;
    }
    tree.style_mut(template.actions_container).visible = show_actions;
    place(&mut tree, template.actions_container, x + ACTIONS_MARGIN_LEFT, top, actions_width, MARKER_ICON_HEIGHT);
}

/// `MarkerSeverityColumnRenderer`.
struct MarkerSeverityColumnRenderer {
    markers_view_model: Rc<MarkersViewModel>,
    services: Rc<MarkersTreeServices>,
}

impl MarkerSeverityColumnRenderer {
    const TEMPLATE_ID: &'static str = "severity";

    fn template(template_data: &TemplateData) -> &SeverityTemplate {
        template_data.downcast_ref::<SeverityTemplate>().expect("o template do MarkerSeverityColumnRenderer")
    }
}

impl IListRenderer<Item> for MarkerSeverityColumnRenderer {
    fn template_id(&self) -> String {
        Self::TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let ui = &self.services.ui;
        let (severity_column, icon, actions_container) = {
            let mut tree = ui.tree_mut();
            let severity_column = create_el(&mut tree, container, Role::Generic, "severity");
            let icon = create_el(&mut tree, severity_column, Role::Image, "");
            let actions_container = create_el(&mut tree, container, Role::Generic, "actions");
            (severity_column, icon, actions_container)
        };
        let icon_title = CellTitle::new(icon);

        let quick_fix_item: QuickFixItemSlot = Rc::default();
        let action_bar = ActionBar::new(
            ui,
            actions_container,
            IActionBarOptions {
                action_view_item_provider: Some(quick_fix_view_item_provider(ui, &self.services.context_menu_service, &quick_fix_item)),
                styles: self.services.styles.action_bar.clone(),
                ..IActionBarOptions::default()
            },
        );

        Rc::new(SeverityTemplate {
            cell: container,
            severity_column,
            icon,
            icon_title,
            actions_container,
            action_bar,
            quick_fix_item,
            element_disposables: DisposableStore::new(),
            content: Cell::new(Rect::ZERO),
        })
    }

    fn render_element(&self, element: &Item, _index: usize, template_data: &TemplateData, _details: Option<&ListElementRenderDetails>) {
        let ui = &self.services.ui;
        let template = Self::template(template_data);
        template.element_disposables.clear();

        let severity = element.base.marker.severity;
        template.icon_title.set(&severity.to_display_string());
        {
            let severity_kind = severity.to_severity();
            let class_prefix = format!("marker-icon {} ", severity_kind.map_or("", |severity| severity.as_str()));
            set_severity_icon(&mut ui.tree_mut(), template.icon, severity_kind, &class_prefix, &self.services.styles);
        }

        template.action_bar.clear();
        if let Some(view_model) = self.markers_view_model.get_view_model(&element.base) {
            let quick_fix_action = view_model.quick_fix_action();
            template.action_bar.push(
                &[quick_fix_action.clone() as ActionRef],
                IActionOptions { item: IActionViewItemOptions { icon: Some(true), label: Some(false), styles: self.services.styles.action_bar.clone(), ..IActionViewItemOptions::default() }, index: None },
            );
            template.action_bar.layout();
            ui.tree_mut().toggle_class(template.cell, "quickFix", quick_fix_action.enabled());

            // O `Rc` do template, para o ouvinte do `onDidChange` achar a coluna sem prendê-la.
            if let Ok(template_rc) = Rc::downcast::<SeverityTemplate>(template_data.clone()) {
                let (weak, ui) = (Rc::downgrade(&template_rc), ui.clone());
                template.element_disposables.add(quick_fix_action.on_did_change().listen(move |event| {
                    if let (Some(enabled), Some(template)) = (event.enabled, weak.upgrade()) {
                        ui.tree_mut().toggle_class(template.cell, "quickFix", enabled);
                        layout_severity(&ui, &template);
                    }
                }));
            }
            template.element_disposables.add(listen_show_quick_fixes(&quick_fix_action, &template.quick_fix_item));
        }

        layout_severity(ui, template);
    }

    fn dispose_element(&self, _element: &Item, _index: i64, template_data: &TemplateData, _details: Option<&ListElementRenderDetails>) {
        let template = Self::template(template_data);
        template.element_disposables.clear();
        template.action_bar.clear();
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        let template = Self::template(template_data);
        template.element_disposables.dispose();
        template.action_bar.dispose();
        template.icon_title.dispose();
    }
}

impl ITableRenderer<Item> for MarkerSeverityColumnRenderer {
    fn layout_cell(&self, template_data: &TemplateData, content: Rect) {
        let template = Self::template(template_data);
        template.content.set(content);
        layout_severity(&self.services.ui, template);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// MarkerCodeColumnRenderer
// ---------------------------------------------------------------------------------------------------------------------

/// `IMarkerCodeColumnTemplateData`.
struct CodeTemplate {
    /// `.code`.
    code_column: NodeId,
    title: CellTitle,
    source_label: Rc<HighlightedLabel>,
    /// O código com o `(`, o rótulo e o `)` (e o link, se tem `target`) do elemento atual.
    code_item: RefCell<Option<CodeItem>>,
    template_disposables: Rc<DisposableStore>,
    element_disposables: Rc<DisposableStore>,
    content: Cell<Rect>,
}

impl CodeTemplate {
    /// Tira do nó o código do elemento anterior.
    fn clear_code(&self, ui: &Ui) {
        self.element_disposables.clear();
        if let Some(code_item) = self.code_item.borrow_mut().take() {
            ui.tree_mut().remove(code_item.node);
        }
    }
}

/// `.code`: a origem e, depois dela, o código, centrados na altura da célula e recortados na largura dela.
fn layout_code(ui: &Ui, template: &CodeTemplate) {
    let content = template.content.get();
    let available = content.width().max(0.0);
    let line_height = template.source_label.line_height();

    template.source_label.set_max_width(Some(available));
    let source_width = template.source_label.width();
    let mut tree = ui.tree_mut();
    place(&mut tree, template.code_column, content.x0, (content.height() - line_height) / 2.0, available, line_height);
    place(&mut tree, template.source_label.element(), 0.0, 0.0, source_width, line_height);
    if let Some(code_item) = &*template.code_item.borrow() {
        code_item.place_at(&mut tree, source_width, line_height);
    }
}

/// `MarkerCodeColumnRenderer`.
struct MarkerCodeColumnRenderer {
    services: Rc<MarkersTreeServices>,
}

impl MarkerCodeColumnRenderer {
    const TEMPLATE_ID: &'static str = "code";

    fn template(template_data: &TemplateData) -> &CodeTemplate {
        template_data.downcast_ref::<CodeTemplate>().expect("o template do MarkerCodeColumnRenderer")
    }
}

impl IListRenderer<Item> for MarkerCodeColumnRenderer {
    fn template_id(&self) -> String {
        Self::TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let ui = &self.services.ui;
        let template_disposables = DisposableStore::new();
        let element_disposables = DisposableStore::new();
        template_disposables.add_rc(element_disposables.clone());
        let code_column = {
            let mut tree = ui.tree_mut();
            let code_column = create_el(&mut tree, container, Role::Generic, "code");
            tree.style_mut(code_column).clip = true;
            code_column
        };
        let title = CellTitle::new(code_column);

        let source_label = HighlightedLabel::new(ui, code_column, None, cell_label_styles(&self.services));
        ui.tree_mut().add_class(source_label.element(), "source-label");
        template_disposables.add(source_label.clone());

        Rc::new(CodeTemplate { code_column, title, source_label, code_item: RefCell::new(None), template_disposables, element_disposables, content: Cell::new(Rect::ZERO) })
    }

    fn render_element(&self, element: &Item, _index: usize, template_data: &TemplateData, _details: Option<&ListElementRenderDetails>) {
        let ui = &self.services.ui;
        let template = Self::template(template_data);
        template.clear_code(ui);
        {
            let mut tree = ui.tree_mut();
            tree.remove_class(template.code_column, "code-label");
            tree.remove_class(template.code_column, "code-link");
        }

        let raw = &element.base.marker;
        let source = raw.source.as_deref().filter(|source| !source.is_empty());
        let code = raw.code.as_ref().filter(|code| code_value(code).is_some());
        let source_matches = highlights(matches_or_empty(&element.source_matches));

        if let (Some(source), Some(code)) = (source, code) {
            let (class_name, container_class, value) = match code {
                MarkerCode::Text(text) => ("code-label", "code-label", text.as_str()),
                MarkerCode::Target { value, .. } => ("code-link", "code-link-label", value.as_str()),
            };
            ui.tree_mut().add_class(template.code_column, class_name);
            template.title.set(&format!("{source} ({value})"));
            template.source_label.set(Some(source), &source_matches, "", false, false);

            let styles = &self.services.styles;
            let code_item = render_code(
                ui,
                &self.services.opener_service,
                &template.element_disposables,
                template.code_column,
                code,
                matches_or_empty(&element.code_matches),
                cell_label_styles(&self.services),
                styles.text_link_foreground,
                container_class,
                1.0,
            );
            *template.code_item.borrow_mut() = Some(code_item);
        } else {
            template.title.set("");
            template.source_label.set(Some("-"), &[], "", false, false);
        }

        layout_code(ui, template);
    }

    fn dispose_element(&self, _element: &Item, _index: i64, template_data: &TemplateData, _details: Option<&ListElementRenderDetails>) {
        Self::template(template_data).element_disposables.clear();
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        let template = Self::template(template_data);
        template.element_disposables.clear();
        template.template_disposables.dispose();
        template.title.dispose();
    }
}

impl ITableRenderer<Item> for MarkerCodeColumnRenderer {
    fn layout_cell(&self, template_data: &TemplateData, content: Rect) {
        let template = Self::template(template_data);
        template.content.set(content);
        layout_code(&self.services.ui, template);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// MarkerMessageColumnRenderer e MarkerSourceColumnRenderer
// ---------------------------------------------------------------------------------------------------------------------

/// `IMarkerHighlightedLabelColumnTemplateData`.
struct LabelTemplate {
    /// `.message` ou `.source`.
    column_element: NodeId,
    title: CellTitle,
    highlighted_label: Rc<HighlightedLabel>,
    content: Cell<Rect>,
}

/// A coluna de rótulo: o rótulo centrado na altura da célula e cortado com `…` na largura dela.
fn layout_label(ui: &Ui, template: &LabelTemplate) {
    let content = template.content.get();
    let available = content.width().max(0.0);
    let line_height = template.highlighted_label.line_height();

    template.highlighted_label.set_max_width(Some(available));
    let mut tree = ui.tree_mut();
    place(&mut tree, template.column_element, content.x0, (content.height() - line_height) / 2.0, available, line_height);
    place(&mut tree, template.highlighted_label.element(), 0.0, 0.0, template.highlighted_label.width(), line_height);
}

/// `MarkerMessageColumnRenderer` e `MarkerSourceColumnRenderer`: as duas colunas são o mesmo rótulo destacado dentro de um
/// elemento com o `title`, e diferem no id do template, na classe, no texto e nos destaques.
struct HighlightedLabelColumnRenderer {
    template_id: &'static str,
    class_name: &'static str,
    text_of: fn(&MarkerTableItem) -> &str,
    matches_of: fn(&MarkerTableItem) -> &Option<Vec<Match>>,
    services: Rc<MarkersTreeServices>,
}

impl HighlightedLabelColumnRenderer {
    /// `MarkerMessageColumnRenderer`.
    fn message(services: Rc<MarkersTreeServices>) -> Self {
        Self { template_id: "message", class_name: "message", text_of: |item| &item.base.marker.message, matches_of: |item| &item.message_matches, services }
    }

    /// `MarkerSourceColumnRenderer`.
    fn source(services: Rc<MarkersTreeServices>) -> Self {
        Self { template_id: "source", class_name: "source", text_of: |item| item.base.marker.source.as_deref().unwrap_or(""), matches_of: |item| &item.source_matches, services }
    }

    fn template(template_data: &TemplateData) -> &LabelTemplate {
        template_data.downcast_ref::<LabelTemplate>().expect("o template da coluna de rótulo")
    }
}

impl IListRenderer<Item> for HighlightedLabelColumnRenderer {
    fn template_id(&self) -> String {
        self.template_id.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let ui = &self.services.ui;
        let column_element = {
            let mut tree = ui.tree_mut();
            let column_element = create_el(&mut tree, container, Role::Generic, self.class_name);
            tree.style_mut(column_element).clip = true;
            column_element
        };
        let title = CellTitle::new(column_element);
        let highlighted_label = HighlightedLabel::new(ui, column_element, None, cell_label_styles(&self.services));

        Rc::new(LabelTemplate { column_element, title, highlighted_label, content: Cell::new(Rect::ZERO) })
    }

    fn render_element(&self, element: &Item, _index: usize, template_data: &TemplateData, _details: Option<&ListElementRenderDetails>) {
        let template = Self::template(template_data);
        let text = (self.text_of)(element);
        template.title.set(text);
        template.highlighted_label.set(Some(text), &highlights(matches_or_empty((self.matches_of)(element))), "", false, false);
        layout_label(&self.services.ui, template);
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        let template = Self::template(template_data);
        template.highlighted_label.dispose();
        template.title.dispose();
    }
}

impl ITableRenderer<Item> for HighlightedLabelColumnRenderer {
    fn layout_cell(&self, template_data: &TemplateData, content: Rect) {
        let template = Self::template(template_data);
        template.content.set(content);
        layout_label(&self.services.ui, template);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// MarkerFileColumnRenderer
// ---------------------------------------------------------------------------------------------------------------------

/// `IMarkerFileColumnTemplateData`.
struct FileTemplate {
    /// `.file`.
    column_element: NodeId,
    title: CellTitle,
    file_label: Rc<HighlightedLabel>,
    position_label: Rc<HighlightedLabel>,
    content: Cell<Rect>,
}

/// `.file`: o arquivo e, depois dele, a posição (`margin-left: 6px`, opacidade 0,7), recortados na largura da célula.
fn layout_file(ui: &Ui, template: &FileTemplate) {
    let content = template.content.get();
    let available = content.width().max(0.0);
    let line_height = template.file_label.line_height();

    template.file_label.set_max_width(Some(available));
    let file_width = template.file_label.width();
    let position_available = (available - file_width - FILE_POSITION_MARGIN_LEFT).max(0.0);
    template.position_label.set_max_width(Some(position_available));
    let position_width = template.position_label.width();

    let mut tree = ui.tree_mut();
    place(&mut tree, template.column_element, content.x0, (content.height() - line_height) / 2.0, available, line_height);
    place(&mut tree, template.file_label.element(), 0.0, 0.0, file_width, line_height);
    tree.style_mut(template.position_label.element()).visible = position_available > 0.0;
    place(&mut tree, template.position_label.element(), file_width + FILE_POSITION_MARGIN_LEFT, 0.0, position_width, line_height);
}

/// `MarkerFileColumnRenderer`.
struct MarkerFileColumnRenderer {
    services: Rc<MarkersTreeServices>,
}

impl MarkerFileColumnRenderer {
    const TEMPLATE_ID: &'static str = "file";

    fn template(template_data: &TemplateData) -> &FileTemplate {
        template_data.downcast_ref::<FileTemplate>().expect("o template do MarkerFileColumnRenderer")
    }
}

impl IListRenderer<Item> for MarkerFileColumnRenderer {
    fn template_id(&self) -> String {
        Self::TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let ui = &self.services.ui;
        let column_element = {
            let mut tree = ui.tree_mut();
            let column_element = create_el(&mut tree, container, Role::Generic, "file");
            tree.style_mut(column_element).clip = true;
            column_element
        };
        let title = CellTitle::new(column_element);

        let file_label = HighlightedLabel::new(ui, column_element, None, cell_label_styles(&self.services));
        let position_label = HighlightedLabel::new(ui, column_element, None, cell_label_styles(&self.services));
        {
            let mut tree = ui.tree_mut();
            tree.add_class(file_label.element(), "file-label");
            tree.add_class(position_label.element(), "file-position");
            tree.style_mut(position_label.element()).opacity = 0.7;
        }

        Rc::new(FileTemplate { column_element, title, file_label, position_label, content: Cell::new(Rect::ZERO) })
    }

    fn render_element(&self, element: &Item, _index: usize, template_data: &TemplateData, _details: Option<&ListElementRenderDetails>) {
        let template = Self::template(template_data);
        let raw = &element.base.marker;
        let position_label = Messages::markers_panel_at_line_col_number(raw.start_line_number, raw.start_column);
        let label_service = self.services.label_service.as_ref();

        template.title.set(&format!("{} {}", label_service.get_uri_label(&raw.resource, &UriLabelOptions { relative: Some(false), ..UriLabelOptions::default() }), position_label));
        template.file_label.set(Some(&relative_uri_label(label_service, &raw.resource)), &highlights(matches_or_empty(&element.file_matches)), "", false, false);
        template.position_label.set(Some(&position_label), &[], "", false, false);
        layout_file(&self.services.ui, template);
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        let template = Self::template(template_data);
        template.file_label.dispose();
        template.position_label.dispose();
        template.title.dispose();
    }
}

impl ITableRenderer<Item> for MarkerFileColumnRenderer {
    fn layout_cell(&self, template_data: &TemplateData, content: Rect) {
        let template = Self::template(template_data);
        template.content.set(content);
        layout_file(&self.services.ui, template);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// MarkersTableVirtualDelegate
// ---------------------------------------------------------------------------------------------------------------------

/// `MarkersTableVirtualDelegate`.
struct MarkersTableVirtualDelegate;

impl MarkersTableVirtualDelegate {
    const HEADER_ROW_HEIGHT: f64 = 24.0;
    const ROW_HEIGHT: f64 = 24.0;
}

impl ITableVirtualDelegate<Item> for MarkersTableVirtualDelegate {
    fn header_row_height(&self) -> f64 {
        Self::HEADER_ROW_HEIGHT
    }

    fn get_height(&self, _item: &Item) -> f64 {
        Self::ROW_HEIGHT
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// MarkersTable
// ---------------------------------------------------------------------------------------------------------------------

/// `ITableEvent<MarkerTableItem>` como o evento do `IProblemsWidget`.
fn to_widget_event(event: &ListEvent<Item>) -> ProblemsWidgetEvent {
    ProblemsWidgetEvent { elements: event.elements.iter().cloned().map(|item| Some(ProblemsElement::TableItem(item))).collect(), browser_event: event.browser_event }
}

/// `MarkersTable`.
pub struct MarkersTable {
    base: Rc<Disposable>,
    ui: Ui,
    item_count: Cell<usize>,
    table: Rc<WorkbenchTable<Item>>,
    container: NodeId,
    resource_markers: RefCell<Vec<Rc<ResourceMarkers>>>,
    filter_options: RefCell<Rc<FilterOptions>>,
    label_service: Rc<dyn ILabelService>,
}

impl MarkersTable {
    /// `new MarkersTable(container, markersViewModel, resourceMarkers, filterOptions, options, instantiationService, labelService)`.
    pub fn new(
        container: NodeId,
        markers_view_model: Rc<MarkersViewModel>,
        resource_markers: Vec<Rc<ResourceMarkers>>,
        filter_options: Rc<FilterOptions>,
        mut options: IWorkbenchTableOptions<Item>,
        workbench: &WorkbenchTreeServices,
        services: Rc<MarkersTreeServices>,
    ) -> Rc<Self> {
        let ui = services.ui.clone();
        let base = Disposable::new();

        // `.markers-table-container .monaco-table .monaco-table-th` e `.monaco-table-td` (markers.css)
        options.table.cell_padding_left = CELL_PADDING_LEFT;
        options.table.header_font_weight = HEADER_FONT_WEIGHT;

        let column = |label: String, weight: f64, template_id: &str, minimum_width: Option<f64>, maximum_width: Option<f64>| -> ITableColumn<Item, Item> {
            ITableColumn {
                label,
                tooltip: None,
                weight,
                template_id: template_id.to_owned(),
                minimum_width,
                maximum_width,
                on_did_change_width_constraints: None,
                project: Rc::new(|row: &Item| row.clone()),
            }
        };
        let columns = vec![
            column(String::new(), 0.0, MarkerSeverityColumnRenderer::TEMPLATE_ID, Some(36.0), Some(36.0)),
            column(localize("codeColumnLabel", "Code", &[]), 1.0, MarkerCodeColumnRenderer::TEMPLATE_ID, Some(100.0), Some(300.0)),
            column(localize("messageColumnLabel", "Message", &[]), 4.0, "message", None, None),
            column(localize("fileColumnLabel", "File", &[]), 2.0, MarkerFileColumnRenderer::TEMPLATE_ID, None, None),
            column(localize("sourceColumnLabel", "Source", &[]), 1.0, "source", Some(100.0), Some(300.0)),
        ];
        let renderers: Vec<Rc<dyn ITableRenderer<Item>>> = vec![
            Rc::new(MarkerSeverityColumnRenderer { markers_view_model: markers_view_model.clone(), services: services.clone() }),
            Rc::new(MarkerCodeColumnRenderer { services: services.clone() }),
            Rc::new(HighlightedLabelColumnRenderer::message(services.clone())),
            Rc::new(MarkerFileColumnRenderer { services: services.clone() }),
            Rc::new(HighlightedLabelColumnRenderer::source(services.clone())),
        ];

        let table = WorkbenchTable::new("Markers", &ui, container, Rc::new(MarkersTableVirtualDelegate), columns, renderers, options, workbench);
        base.register(table.clone());

        // mouseover/mouseleave event handlers
        let on_row_hover: Event<i64> = table.on_mouse_move().map(|event| event.index.map_or(-1, |index| index as i64), None);
        let on_list_leave: Event<i64> = {
            let (ui, node) = (ui.clone(), table.list_html_element());
            Event::new(move |listener| ui.listen(node, EventKind::PointerLeave, false, move |_| listener(&-1)))
        };
        let on_row_hover_or_leave = any(vec![on_row_hover, on_list_leave]).latch(None);
        let on_row_permanent_hover = on_row_hover_or_leave.debounce(|_, index| *index, je_base::event::DebounceOptions { delay: Delay::Millis(500), ..Default::default() }, None);

        {
            let weak = Rc::downgrade(&table);
            base.register(on_row_permanent_hover.listen(move |&index| {
                let Some(table) = weak.upgrade() else { return };
                if index != -1 && (index as usize) < table.length() {
                    markers_view_model.on_marker_mouse_hover(&marker_of(&table.row(index as usize)));
                }
            }));
        }

        Rc::new(Self {
            base,
            ui,
            item_count: Cell::new(0),
            table,
            container,
            resource_markers: RefCell::new(resource_markers),
            filter_options: RefCell::new(filter_options),
            label_service: services.label_service.clone(),
        })
    }

    /// `isVisible()`.
    fn is_visible(&self) -> bool {
        !self.ui.tree().has_class(self.container, "hidden")
    }

    /// `findMarkerIndex(marker)`: o índice da linha do marcador (comparado pelo marcador cru), ou o `Invalid index -1` que a
    /// lista lança para o `-1` do original.
    fn find_marker_index(&self, marker: &Marker) -> usize {
        (0..self.table.length()).find(|&index| Rc::ptr_eq(&self.table.row(index).base.marker, &marker.marker)).unwrap_or_else(|| Self::throw_invalid_index())
    }

    fn throw_invalid_index() -> ! {
        je_base::errors::throw(list_error("Markers", "Invalid index -1"))
    }

    /// `hasSelectedMarkerFor(resource)`.
    fn has_selected_marker_for(&self, resource: &ResourceMarkers) -> bool {
        self.table.get_selection().first().is_some_and(|&index| resource.has(self.table.row(index).base.resource()))
    }

    fn element_of(item: Item) -> Option<ProblemsElement> {
        Some(ProblemsElement::TableItem(item))
    }
}

impl IProblemsWidget for MarkersTable {
    fn context_key_service(&self) -> Rc<dyn IContextKeyService> {
        self.table.context_key_service.clone()
    }

    fn on_context_menu(&self) -> Event<ProblemsContextMenuEvent> {
        self.table.on_context_menu().map(
            |event| ProblemsContextMenuEvent {
                element: event.element.clone().and_then(Self::element_of),
                index: event.index,
                browser_event: event.browser_event,
                is_handled_by_list: event.is_handled_by_list.clone(),
            },
            None,
        )
    }

    fn on_did_open(&self) -> Event<IOpenEvent<ProblemsElement>> {
        self.table.on_did_open().map(
            |event| IOpenEvent {
                editor_options: event.editor_options.clone(),
                side_by_side: event.side_by_side,
                element: ProblemsElement::TableItem(event.element.clone()),
                browser_event: event.browser_event,
            },
            None,
        )
    }

    fn on_did_change_focus(&self) -> Event<ProblemsWidgetEvent> {
        self.table.on_did_change_focus().map(to_widget_event, None)
    }

    fn on_did_change_selection(&self) -> Event<ProblemsWidgetEvent> {
        self.table.on_did_change_selection().map(to_widget_event, None)
    }

    fn collapse_markers(&self) {}

    fn dom_focus(&self) {
        self.table.dom_focus();
    }

    fn filter_markers(&self, resource_markers: &[Rc<ResourceMarkers>], filter_options: Rc<FilterOptions>) {
        *self.filter_options.borrow_mut() = filter_options;
        self.reset(resource_markers);
    }

    fn get_focus(&self) -> Vec<Option<ProblemsElement>> {
        self.table.get_focus().into_iter().map(|index| Self::element_of(self.table.row(index))).collect()
    }

    fn get_html_element(&self) -> NodeId {
        self.table.get_html_element()
    }

    fn get_relative_top(&self, marker: Option<&ProblemsElement>) -> Option<f64> {
        let marker = marker?;
        let index = match marker {
            ProblemsElement::TableItem(item) => self.table.index_of(|row| Rc::ptr_eq(row, item)),
            ProblemsElement::Element(_) => None,
        };
        self.table.get_relative_top(index.unwrap_or_else(|| Self::throw_invalid_index()))
    }

    fn get_selection(&self) -> Vec<Option<ProblemsElement>> {
        self.table.get_selection().into_iter().map(|index| Self::element_of(self.table.row(index))).collect()
    }

    fn get_visible_item_count(&self) -> usize {
        self.item_count.get()
    }

    fn layout(&self, height: f64, width: f64) {
        {
            let mut tree = self.ui.tree_mut();
            let bounds = tree.bounds(self.container);
            tree.set_fill(self.container, true, false);
            tree.set_bounds(self.container, Rect::new(bounds.x0, bounds.y0, bounds.x1, bounds.y0 + height));
        }
        self.table.layout(Some(height), Some(width));
    }

    fn reset(&self, resource_markers: &[Rc<ResourceMarkers>]) {
        *self.resource_markers.borrow_mut() = resource_markers.to_vec();
        let filter_options = self.filter_options.borrow().clone();

        let mut items: Vec<Item> = Vec::new();
        for resource_marker in resource_markers {
            for marker in resource_marker.markers() {
                if unsupported_schemas().contains(marker.resource().scheme().to_string_lossy().as_str()) {
                    continue;
                }

                // Exclude pattern
                if filter_options.excludes_matcher.matches(marker.resource()) {
                    continue;
                }

                // Include pattern
                if filter_options.includes_matcher.matches(marker.resource()) {
                    items.push(Rc::new(MarkerTableItem::new(&marker, None, None, None, None)));
                    continue;
                }

                // Severity filter
                if !matches_severity(&filter_options, marker.marker.severity) {
                    continue;
                }

                // Source filters
                if !filter_options.matches_source_filters(marker.marker.source.as_deref()) {
                    continue;
                }

                // Text filter
                if !filter_options.text_filter.text.is_empty() {
                    let text = units(&filter_options.text_filter.text);
                    let (source_matches, code_matches) = source_and_code_matches(&marker.marker, &text);
                    let message_matches = (FilterOptions::MESSAGE_FILTER)(&text, &units(&marker.marker.message));
                    let file_matches = (FilterOptions::MESSAGE_FILTER)(&text, &units(&relative_uri_label(self.label_service.as_ref(), marker.resource())));

                    let matched = source_matches.is_some() || code_matches.is_some() || message_matches.is_some() || file_matches.is_some();
                    if (matched && !filter_options.text_filter.negate) || (!matched && filter_options.text_filter.negate) {
                        items.push(Rc::new(MarkerTableItem::new(&marker, source_matches, code_matches, message_matches, file_matches)));
                    }

                    continue;
                }

                items.push(Rc::new(MarkerTableItem::new(&marker, None, None, None, None)));
            }
        }
        self.item_count.set(items.len());
        items.sort_by(|a, b| {
            let mut result = MarkerSeverity::compare(a.base.marker.severity, b.base.marker.severity);

            if result == 0 {
                result = i64::from(compare_markers_by_uri(&a.base.marker, &b.base.marker));
            }

            if result == 0 {
                result = i64::from(Range::compare_ranges_using_starts(Some(&a.base.range()), Some(&b.base.range())));
            }

            result.cmp(&0)
        });
        self.table.splice(0, self.table.length(), &items);
    }

    fn reveal_markers(&self, active_resource: Option<&Rc<ResourceMarkers>>, focus: bool, last_selected_relative_top: f64) {
        if let Some(active_resource) = active_resource {
            let active_resource_index = self.resource_markers.borrow().iter().position(|resource_markers| Rc::ptr_eq(resource_markers, active_resource));

            if let Some(active_resource_index) = active_resource_index {
                if self.has_selected_marker_for(active_resource) {
                    let table_selection = self.table.get_selection();
                    self.table.reveal(table_selection[0], Some(last_selected_relative_top));

                    if focus {
                        self.table.set_focus(table_selection, None);
                    }
                } else {
                    self.table.reveal(active_resource_index, Some(0.0));

                    if focus {
                        self.table.set_focus(vec![active_resource_index], None);
                        self.table.set_selection(vec![active_resource_index], None);
                    }
                }
            }
        } else if focus {
            self.table.set_selection(Vec::new(), None);
            self.table.focus_first(None);
        }
    }

    fn set_aria_label(&self, label: &str) {
        self.ui.tree_mut().set_label(self.table.get_html_element(), label);
    }

    fn set_marker_selection(&self, selection: Option<&[Rc<Marker>]>, focus: Option<&[Rc<Marker>]>) {
        if !self.is_visible() {
            return;
        }

        if let Some(selection) = selection.filter(|selection| !selection.is_empty()) {
            self.table.set_selection(selection.iter().map(|marker| self.find_marker_index(marker)).collect(), None);

            match focus.filter(|focus| !focus.is_empty()) {
                Some(focus) => self.table.set_focus(focus.iter().map(|marker| self.find_marker_index(marker)).collect(), None),
                None => self.table.set_focus(vec![self.find_marker_index(&selection[0])], None),
            }

            self.table.reveal(self.find_marker_index(&selection[0]), None);
        } else if self.get_selection().is_empty() && self.get_visible_item_count() > 0 {
            self.table.set_selection(vec![0], None);
            self.table.set_focus(vec![0], None);
            self.table.reveal(0, None);
        }
    }

    fn toggle_visibility(&self, hide: bool) {
        let mut tree = self.ui.tree_mut();
        tree.toggle_class(self.container, "hidden", hide);
        // `.markers-panel .markers-panel-container .hidden { display: none }`
        tree.style_mut(self.container).visible = !hide;
    }

    fn update(&self, resource_markers: &[Rc<ResourceMarkers>]) {
        for resource_marker in resource_markers {
            let mut markers = self.resource_markers.borrow_mut();
            let index = markers.iter().position(|existing| Rc::ptr_eq(existing, resource_marker));
            // `splice(indexOf(...), 1, resourceMarker)`: o `-1` de quem não está na lista conta a partir do fim
            let start = index.unwrap_or_else(|| markers.len().saturating_sub(1));
            if start < markers.len() {
                markers[start] = resource_marker.clone();
            } else {
                markers.push(resource_marker.clone());
            }
        }

        let all = self.resource_markers.borrow().clone();
        self.reset(&all);
    }

    fn update_marker(&self, _marker: &Rc<Marker>) {
        self.table.rerender();
    }
}

impl IDisposable for MarkersTable {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "MarkersTable"
    }
}
