//! Porte de `src/vs/workbench/contrib/codeEditor/browser/outline/documentSymbolsTree.ts`: o que a árvore e as migalhas do
//! outline de símbolos do documento usam (rótulo de navegação, acessibilidade, identidade, altura, renderers, filtro por
//! tipo de símbolo e comparadores).
//!
//! Notas de tradução:
//! - **`DocumentSymbolItem = OutlineGroup | OutlineElement`.** É o [`DocumentSymbolItem`]. O elemento do outline
//!   ([`OutlineItem`], `Rc<dyn Any>`) guarda diretamente o `Rc<OutlineGroup>` ou o `Rc<OutlineElement>` do `je-editor`, de modo
//!   que a identidade do objeto (a igualdade do [`OutlineTreeElement`]) é a do elemento do modelo.
//! - **Provedores da árvore.** A árvore de dados do porte é uma `ObjectTree` cujo elemento é `Option<T>` (a raiz é `None`), então
//!   [`DocumentSymbolIdentityProvider`], [`DocumentSymbolNavigationLabelProvider`] e [`DocumentSymbolAccessibilityProvider`] são
//!   implementados sobre `Option<OutlineTreeElement>`; o filtro é [`ITreeFilter`] sobre o mesmo tipo.
//! - **`symbolKindNames` e `getAriaLabelForSymbol`** (`editor/common/languages.ts`) ainda não têm porte em `je-editor`; ficam
//!   aqui ([`symbol_kind_name`] e [`get_aria_label_for_symbol`]), com os textos literais do original.
//! - **`DocumentSymbolRenderer.renderElement`.** O `IconLabel` com realces, o ícone colorido do tipo (`SymbolKinds.toIcon`, ligado
//!   por `outline.icons`), a marca de problemas (decoração e cor `--outline-element-color`), o estilo de símbolo obsoleto e o
//!   título `{0} ({1})` dependem de widgets de rótulo com decoração que o `je-ui` ainda não tem; o renderer escreve o rótulo do nó
//!   (nome e detalhe), que é o conteúdo textual do original.
//! - **`DocumentSymbolDragAndDrop`.** Não entra: o `dnd` da árvore depende de `fillInSymbolsDragData` e `fillEditorsDragData`
//!   (`platform/dnd`), sem porte.
//! - **`DocumentSymbolFilter.kindToConfigName`** é [`document_symbol_kind_to_config_name`] (`Object.freeze` de um mapa).
//! - **`createInstance(DocumentSymbolFilter, 'outline')`** vira [`DocumentSymbolFilter::new`] com o serviço injetado.

use std::any::Any;
use std::rc::Rc;

use je_base::collator::{Collator, CollatorOptions};
use je_base::dynamic::Dynamic;
use je_base::filters::FuzzyScore;
use je_editor::language_providers::SymbolKind;
use je_editor::outline_model::{OutlineElement as SymbolElement, OutlineGroup, OutlineModel, TreeElement};
use je_editor::range::Range;
use je_editor::text_resource_configuration_service::ITextResourceConfigurationService;
use je_ui::{NodeId, Ui};
use je_widgets::list::list_types::{IIdentityProvider, IKeyboardNavigationLabelProvider, IListAccessibilityProvider, IListVirtualDelegate, TemplateData};
use je_widgets::tree::object_tree_model::ObjectTreeModel;
use je_widgets::tree::tree_types::{ITreeFilter, ITreeModel, ITreeRenderer, TreeElementRenderDetails, TreeFilterResult, TreeNodeApi, TreeVisibility};

use crate::services::outline::outline::{IOutlineComparator, OutlineElement as OutlineItem, OutlineTreeElement};

/// `DocumentSymbolItem`: `OutlineGroup | OutlineElement`.
#[derive(Clone)]
pub enum DocumentSymbolItem {
    Group(Rc<OutlineGroup>),
    Element(Rc<SymbolElement>),
}

impl DocumentSymbolItem {
    /// O item que o elemento do outline guarda (`None` para um objeto que não é grupo nem elemento).
    pub fn from_outline_item(item: &OutlineItem) -> Option<DocumentSymbolItem> {
        if let Ok(element) = Rc::clone(item).downcast::<SymbolElement>() {
            return Some(DocumentSymbolItem::Element(element));
        }
        Rc::clone(item).downcast::<OutlineGroup>().ok().map(DocumentSymbolItem::Group)
    }

    /// O item de um elemento de árvore da raiz para baixo (`None` para o modelo).
    pub fn from_tree_element(element: &TreeElement) -> Option<DocumentSymbolItem> {
        match element {
            TreeElement::Model(_) => None,
            TreeElement::Group(group) => Some(DocumentSymbolItem::Group(Rc::clone(group))),
            TreeElement::Element(element) => Some(DocumentSymbolItem::Element(Rc::clone(element))),
        }
    }

    /// O elemento do outline (a identidade do objeto do modelo).
    pub fn to_outline_item(&self) -> OutlineItem {
        match self {
            DocumentSymbolItem::Group(group) => Rc::clone(group) as Rc<dyn Any>,
            DocumentSymbolItem::Element(element) => Rc::clone(element) as Rc<dyn Any>,
        }
    }

    /// O elemento de árvore do `je-editor`.
    pub fn to_tree_element(&self) -> TreeElement {
        match self {
            DocumentSymbolItem::Group(group) => TreeElement::Group(Rc::clone(group)),
            DocumentSymbolItem::Element(element) => TreeElement::Element(Rc::clone(element)),
        }
    }

    /// `id`.
    pub fn id(&self) -> String {
        match self {
            DocumentSymbolItem::Group(group) => group.id.clone(),
            DocumentSymbolItem::Element(element) => element.id.clone(),
        }
    }
}

/// O item do elemento de árvore (`None` para a raiz ou para um objeto desconhecido).
fn item_of(element: &Option<OutlineTreeElement>) -> Option<DocumentSymbolItem> {
    element.as_ref().and_then(|element| DocumentSymbolItem::from_outline_item(&element.0))
}

/// `symbolKindNames[kind]` (`editor/common/languages.ts`).
pub fn symbol_kind_name(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Array => "array",
        SymbolKind::Boolean => "boolean",
        SymbolKind::Class => "class",
        SymbolKind::Constant => "constant",
        SymbolKind::Constructor => "constructor",
        SymbolKind::Enum => "enumeration",
        SymbolKind::EnumMember => "enumeration member",
        SymbolKind::Event => "event",
        SymbolKind::Field => "field",
        SymbolKind::File => "file",
        SymbolKind::Function => "function",
        SymbolKind::Interface => "interface",
        SymbolKind::Key => "key",
        SymbolKind::Method => "method",
        SymbolKind::Module => "module",
        SymbolKind::Namespace => "namespace",
        SymbolKind::Null => "null",
        SymbolKind::Number => "number",
        SymbolKind::Object => "object",
        SymbolKind::Operator => "operator",
        SymbolKind::Package => "package",
        SymbolKind::Property => "property",
        SymbolKind::String => "string",
        SymbolKind::Struct => "struct",
        SymbolKind::TypeParameter => "type parameter",
        SymbolKind::Variable => "variable",
    }
}

/// `getAriaLabelForSymbol(symbolName, kind)`: `{0} ({1})`.
pub fn get_aria_label_for_symbol(symbol_name: &str, kind: SymbolKind) -> String {
    format!("{symbol_name} ({})", symbol_kind_name(kind))
}

/// `DocumentSymbolFilter.kindToConfigName[kind]`.
pub fn document_symbol_kind_to_config_name(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::File => "showFiles",
        SymbolKind::Module => "showModules",
        SymbolKind::Namespace => "showNamespaces",
        SymbolKind::Package => "showPackages",
        SymbolKind::Class => "showClasses",
        SymbolKind::Method => "showMethods",
        SymbolKind::Property => "showProperties",
        SymbolKind::Field => "showFields",
        SymbolKind::Constructor => "showConstructors",
        SymbolKind::Enum => "showEnums",
        SymbolKind::Interface => "showInterfaces",
        SymbolKind::Function => "showFunctions",
        SymbolKind::Variable => "showVariables",
        SymbolKind::Constant => "showConstants",
        SymbolKind::String => "showStrings",
        SymbolKind::Number => "showNumbers",
        SymbolKind::Boolean => "showBooleans",
        SymbolKind::Array => "showArrays",
        SymbolKind::Object => "showObjects",
        SymbolKind::Key => "showKeys",
        SymbolKind::Null => "showNull",
        SymbolKind::EnumMember => "showEnumMembers",
        SymbolKind::Struct => "showStructs",
        SymbolKind::Event => "showEvents",
        SymbolKind::Operator => "showOperators",
        SymbolKind::TypeParameter => "showTypeParameters",
    }
}

/// `DocumentSymbolNavigationLabelProvider`.
pub struct DocumentSymbolNavigationLabelProvider;

impl IKeyboardNavigationLabelProvider<Option<OutlineTreeElement>> for DocumentSymbolNavigationLabelProvider {
    fn get_keyboard_navigation_label(&self, element: &Option<OutlineTreeElement>) -> Vec<Option<String>> {
        let label = match item_of(element) {
            Some(DocumentSymbolItem::Group(group)) => group.label.clone(),
            Some(DocumentSymbolItem::Element(element)) => element.symbol.name.clone(),
            None => return Vec::new(),
        };
        vec![Some(label)]
    }
}

/// `DocumentSymbolAccessibilityProvider`.
pub struct DocumentSymbolAccessibilityProvider {
    aria_label: String,
}

impl DocumentSymbolAccessibilityProvider {
    pub fn new(aria_label: &str) -> Self {
        Self { aria_label: aria_label.to_owned() }
    }
}

impl IListAccessibilityProvider<Option<OutlineTreeElement>> for DocumentSymbolAccessibilityProvider {
    fn get_widget_aria_label(&self) -> String {
        self.aria_label.clone()
    }

    fn get_aria_label(&self, element: &Option<OutlineTreeElement>) -> Option<String> {
        match item_of(element)? {
            DocumentSymbolItem::Group(group) => Some(group.label.clone()),
            DocumentSymbolItem::Element(element) => Some(get_aria_label_for_symbol(&element.symbol.name, element.symbol.kind)),
        }
    }
}

/// `DocumentSymbolIdentityProvider`.
pub struct DocumentSymbolIdentityProvider;

impl IIdentityProvider<Option<OutlineTreeElement>> for DocumentSymbolIdentityProvider {
    fn get_id(&self, element: &Option<OutlineTreeElement>) -> String {
        item_of(element).map(|item| item.id()).unwrap_or_default()
    }
}

/// `DocumentSymbolVirtualDelegate`.
pub struct DocumentSymbolVirtualDelegate;

impl DocumentSymbolVirtualDelegate {
    /// `DocumentSymbolGroupTemplate.id`.
    pub const GROUP_TEMPLATE_ID: &'static str = "DocumentSymbolGroupTemplate";
    /// `DocumentSymbolTemplate.id`.
    pub const SYMBOL_TEMPLATE_ID: &'static str = "DocumentSymbolTemplate";
}

impl IListVirtualDelegate<OutlineTreeElement> for DocumentSymbolVirtualDelegate {
    fn get_height(&self, _element: &OutlineTreeElement) -> f64 {
        22.0
    }

    fn get_template_id(&self, element: &OutlineTreeElement) -> String {
        match DocumentSymbolItem::from_outline_item(&element.0) {
            Some(DocumentSymbolItem::Group(_)) => Self::GROUP_TEMPLATE_ID.to_owned(),
            _ => Self::SYMBOL_TEMPLATE_ID.to_owned(),
        }
    }
}

type OutlineTreeModel = ObjectTreeModel<OutlineTreeElement, FuzzyScore>;
type OutlineTreeNode = <OutlineTreeModel as ITreeModel>::Node;

/// `DocumentSymbolGroupRenderer`: o rótulo do grupo.
pub struct DocumentSymbolGroupRenderer {
    ui: Ui,
}

impl DocumentSymbolGroupRenderer {
    pub fn new(ui: Ui) -> Self {
        Self { ui }
    }
}

impl ITreeRenderer<OutlineTreeModel> for DocumentSymbolGroupRenderer {
    fn template_id(&self) -> String {
        DocumentSymbolVirtualDelegate::GROUP_TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        Rc::new(container)
    }

    fn render_element(&self, node: &Rc<OutlineTreeNode>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let Some(container) = template_data.downcast_ref::<NodeId>() else { return };
        let Some(element) = node.element() else { return };
        if let Some(DocumentSymbolItem::Group(group)) = DocumentSymbolItem::from_outline_item(&element.0) {
            self.ui.tree_mut().set_label(*container, group.label.clone());
        }
    }

    fn dispose_template(&self, _template_data: &TemplateData) {}
}

/// `DocumentSymbolRenderer`: o nome do símbolo e o detalhe.
pub struct DocumentSymbolRenderer {
    ui: Ui,
}

impl DocumentSymbolRenderer {
    /// `new DocumentSymbolRenderer(renderMarker, target, configurationService, themeService)`: o `renderMarker`, o `target` e os
    /// serviços de configuração e de tema só escolhem o ícone e a decoração de problemas, que o renderer não desenha (ver o
    /// cabeçalho do módulo).
    pub fn new(ui: Ui) -> Self {
        Self { ui }
    }
}

impl ITreeRenderer<OutlineTreeModel> for DocumentSymbolRenderer {
    fn template_id(&self) -> String {
        DocumentSymbolVirtualDelegate::SYMBOL_TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        Rc::new(container)
    }

    fn render_element(&self, node: &Rc<OutlineTreeNode>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let Some(container) = template_data.downcast_ref::<NodeId>() else { return };
        let Some(element) = node.element() else { return };
        let Some(DocumentSymbolItem::Element(element)) = DocumentSymbolItem::from_outline_item(&element.0) else { return };
        let label = if element.symbol.detail.is_empty() { element.symbol.name.clone() } else { format!("{} {}", element.symbol.name, element.symbol.detail) };
        self.ui.tree_mut().set_label(*container, label);
    }

    fn dispose_template(&self, _template_data: &TemplateData) {}
}

/// `DocumentSymbolFilter`.
pub struct DocumentSymbolFilter {
    prefix: &'static str,
    text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
}

impl DocumentSymbolFilter {
    /// `new DocumentSymbolFilter(prefix, textResourceConfigService)`: `prefix` é `'breadcrumbs'` ou `'outline'`.
    pub fn new(prefix: &'static str, text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>) -> Self {
        Self { prefix, text_resource_configuration_service }
    }

    /// `filter(element)`.
    pub fn matches(&self, element: &DocumentSymbolItem) -> bool {
        let DocumentSymbolItem::Element(element) = element else { return true };
        let outline = OutlineModel::get(Some(&TreeElement::Element(Rc::clone(element))));
        let config_key = format!("{}.{}", self.prefix, document_symbol_kind_to_config_name(element.symbol.kind));
        let uri = outline.map(|model| model.uri.clone());
        matches!(self.text_resource_configuration_service.get_value(uri.as_ref(), Some(&config_key)), Dynamic::Bool(true))
    }
}

impl ITreeFilter<Option<OutlineTreeElement>, FuzzyScore> for DocumentSymbolFilter {
    fn filter(&self, element: &Option<OutlineTreeElement>, _parent_visibility: TreeVisibility) -> TreeFilterResult<FuzzyScore> {
        match item_of(element) {
            Some(item) => TreeFilterResult::Bool(self.matches(&item)),
            None => TreeFilterResult::Bool(true),
        }
    }
}

/// `DocumentSymbolComparator`.
pub struct DocumentSymbolComparator {
    collator: Collator,
}

impl Default for DocumentSymbolComparator {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentSymbolComparator {
    pub fn new() -> Self {
        Self { collator: Collator::new(None, CollatorOptions { numeric: true, ..CollatorOptions::default() }) }
    }

    fn compare_names(&self, a: &str, b: &str) -> f64 {
        let (a, b): (Vec<u16>, Vec<u16>) = (a.encode_utf16().collect(), b.encode_utf16().collect());
        f64::from(self.collator.compare(&a, &b))
    }

    fn compare_ranges(a: &Range, b: &Range) -> f64 {
        f64::from(Range::compare_ranges_using_starts(Some(a), Some(b)))
    }

    /// `a.order - b.order`.
    fn compare_order(a: &OutlineGroup, b: &OutlineGroup) -> f64 {
        a.order as f64 - b.order as f64
    }
}

impl IOutlineComparator for DocumentSymbolComparator {
    fn compare_by_position(&self, a: &OutlineItem, b: &OutlineItem) -> f64 {
        match (DocumentSymbolItem::from_outline_item(a), DocumentSymbolItem::from_outline_item(b)) {
            (Some(DocumentSymbolItem::Group(a)), Some(DocumentSymbolItem::Group(b))) => Self::compare_order(&a, &b),
            (Some(DocumentSymbolItem::Element(a)), Some(DocumentSymbolItem::Element(b))) => {
                // `compareRangesUsingStarts(...) || collator.compare(...)`
                let by_range = Self::compare_ranges(&a.symbol.range, &b.symbol.range);
                if by_range != 0.0 { by_range } else { self.compare_names(&a.symbol.name, &b.symbol.name) }
            }
            _ => 0.0,
        }
    }

    fn compare_by_type(&self, a: &OutlineItem, b: &OutlineItem) -> f64 {
        match (DocumentSymbolItem::from_outline_item(a), DocumentSymbolItem::from_outline_item(b)) {
            (Some(DocumentSymbolItem::Group(a)), Some(DocumentSymbolItem::Group(b))) => Self::compare_order(&a, &b),
            (Some(DocumentSymbolItem::Element(a)), Some(DocumentSymbolItem::Element(b))) => {
                // `a.symbol.kind - b.symbol.kind || collator.compare(...)`
                let by_kind = a.symbol.kind as i32 as f64 - b.symbol.kind as i32 as f64;
                if by_kind != 0.0 { by_kind } else { self.compare_names(&a.symbol.name, &b.symbol.name) }
            }
            _ => 0.0,
        }
    }

    fn compare_by_name(&self, a: &OutlineItem, b: &OutlineItem) -> f64 {
        match (DocumentSymbolItem::from_outline_item(a), DocumentSymbolItem::from_outline_item(b)) {
            (Some(DocumentSymbolItem::Group(a)), Some(DocumentSymbolItem::Group(b))) => Self::compare_order(&a, &b),
            (Some(DocumentSymbolItem::Element(a)), Some(DocumentSymbolItem::Element(b))) => {
                // `collator.compare(...) || compareRangesUsingStarts(...)`
                let by_name = self.compare_names(&a.symbol.name, &b.symbol.name);
                if by_name != 0.0 { by_name } else { Self::compare_ranges(&a.symbol.range, &b.symbol.range) }
            }
            _ => 0.0,
        }
    }
}
