//! Porte de `src/vs/editor/contrib/gotoSymbol/browser/peek/referencesWidget.ts`: o `ReferenceWidget` (o `PeekViewWidget` do
//! peek de referências, com a árvore de arquivos e referências à esquerda e o editor de prévia embutido à direita, separados
//! por um sash), o `LayoutData` guardado entre aberturas, o `DecorationsManager` (as decorações das referências no editor de
//! prévia) e a árvore `ReferencesTree`.
//!
//! Representação:
//! - `class ReferenceWidget extends peekView.PeekViewWidget` é a struct [`ReferenceWidget`], dona do
//!   [`PeekViewWidget`](crate::peek_view::PeekViewWidget) e registrada nele como os ganchos ([`PeekViewHooks`]): `_fillBody`,
//!   `_onTitleClick`, `_onWidth`, `_doLayoutBody` e `_applyStyles` são os membros do gancho; `dispose` é o do
//!   [`IDisposable`], que chama o `dispose` do peek no fim (o `super.dispose()`). O controlador a usa pelo contrato
//!   [`IReferenceWidget`], e o `IPeekViewService` pelo [`IPeekViewWidget`] (`addExclusiveWidget(editor, this)`).
//! - **Construção direta.** O `createInstance(ReferenceWidget, editor, defaultTreeKeyboardSupport, data)` do controlador é
//!   [`ReferenceWidget::new`]; os serviços injetados (`@IThemeService`, `@ITextModelService`, `@IPeekViewService`,
//!   `@ILabelService`, `@IKeybindingService`, `@IInstantiationService`) chegam em [`ReferenceWidgetServices`]. O que o
//!   `@IInstantiationService` criava e depende do dono do editor é uma fábrica: o editor de prévia
//!   ([`IReferencePreviewEditorFactory`], com o contêiner `div.preview.inline` que o `EmbeddedCodeEditorWidget` recebe) e o
//!   nó do peek na árvore de widgets (`zone_host`).
//! - **DOM vira a árvore de `je-ui`.** `div.messages`, `div.preview.inline` e `div.ref-tree.inline` são nós com as mesmas
//!   classes; `dom.hide` e `dom.show` são `display: none`. O `innerText` da mensagem é o rótulo do nó. `.messages { height:
//!   100%; width: 100% }` é o `fill` do nó; o `padding: 3em 0` e o `text-align: center` são do pintor.
//!   `.reference-zone-widget { border-top-width: 1px; border-bottom-width: 1px }` é [`PeekViewHooks::apply_styles`]:
//!   a borda do contêiner da zona passa a 1px depois do `_applyStyles` da base (o `ZoneWidget` só conhece a largura da
//!   moldura, e esta é uma regra de classe).
//! - **O sash** é o [`SplitView`] horizontal do `je-widgets` com as duas views (`ViewDescriptor` do original: `onDidChange:
//!   Event.None`, `maximumSize: Number.MAX_VALUE`); o `layout` de cada view é a função que o original passa.
//! - **O editor de prévia.** `this._preview.layout({ height, width })` é `editor.layout(Some(Dimension))`. A `View` do
//!   editor embutido pertence a quem monta o editor (ver `embedded_code_editor_widget`): o contêiner de prévia daqui só
//!   reparte a largura do split view. `_previewNotAvailableMessage` (o `TextModel` "no preview available") só serve ao ramo
//!   em que `ITextEditorModel.textEditorModel` é nulo; o [`ModelReference`](crate::editor_extensions::ModelReference) do
//!   porte sempre traz o `TextModel` (mesma observação de `references_model`), então o ramo e o modelo não existem.
//! - **A árvore.** `ReferencesTree extends WorkbenchAsyncDataTree` é [`ReferencesTree`], sobre a `AsyncDataTree` do
//!   `je-widgets` (o `WorkbenchAsyncDataTree` mora no `je-workbench`, que o `je-editor` não alcança). Dele vêm, no que o peek
//!   usa: o `onDidOpen` (as regras do `ResourceNavigator` com `openOnSingleClick: true`), o registro no `ListService`
//!   (o `lastFocusedList` dos atalhos do peek) e as chaves de contexto de lista e de árvore em um serviço escopado
//!   (`listFocus`, `listSelectionNavigation`, `listHasSelectionOrFocus` e as quatro `treeElement*`; as demais, de rolagem,
//!   busca e multisseleção, não são lidas pelo peek). `overrideStyles: { listBackground }` é o `style` com as cores de
//!   [`references_tree_styles`]. `keyboardSupport` é `AsyncDataTreeOptions::keyboard_support`.
//! - `ReferencesDragAndDrop` é o `dnd` da árvore (`AsyncDataTreeOptions::dnd`): `getDragURI`, `getDragLabel`, `onDragStart`
//!   (grava `ResourceURLs` e `text/plain` no `dataTransfer` do arrasto), `onDragOver` que recusa e `drop` vazio. O
//!   `AccessibilityProvider` entra em `AsyncDataTreeOptions::accessibility_provider`. O `dragstart` da linha chega à
//!   árvore pelo `je_widgets::dnd::listen_drag_event` (o `draggable` da linha é atributo do nó); a opção está ligada.
//! - `async revealReference`, `_revealReference` e `setSelection().then(...)` são [`Promise`]s; o que o original roda antes do
//!   primeiro `await` roda dentro da chamada. `this._tree.rerender(reference)` de uma referência cuja árvore ainda não a tem
//!   (um arquivo recolhido) lançaria `Data tree node not found` dentro de um ouvinte; aqui o ouvinte confere antes
//!   ([`AsyncDataTree::has_node`](je_widgets::tree::async_data_tree::AsyncDataTree::has_node)).
//! - `e.target.range!` do clique duplo no editor é `MouseTarget::range()`; sem intervalo o clique não vira seleção.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::Error;
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::filters::FuzzyScore;
use je_base::json;
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService, IContextKeyServiceTarget};
use je_platform::keybinding::{IKeybindingService, KeyboardEvent};
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::list::list_service::{
    IListWidget, ListService, raw_workbench_list_focus_context_key, workbench_list_has_selection_or_focus, workbench_list_selection_navigation,
    workbench_tree_element_can_collapse, workbench_tree_element_can_expand, workbench_tree_element_has_child, workbench_tree_element_has_parent,
};
use je_platform::opener::opener::with_selection;
use je_theme::color::Color;
use je_theme::to_rgba8;
use je_ui::{Border, Event as UiEvent, EventKind, NodeId, PointerButton, Role, Ui};
use je_widgets::dnd::{DataTransfers, DragEvent, IDragAndDropData};
use je_widgets::dom::{StandardKeyboardEvent, create_detached_el, create_el, hide, show};
use je_widgets::list::list_dnd::{ElementsDragAndDropData, IListDragAndDrop, ListViewTargetSector};
use je_widgets::list::list_styles::IListStyles;
use je_widgets::list::list_types::IListVirtualDelegate;
use je_widgets::splitview::{Orientation, Sizing, SplitView, SplitViewOptions, SplitViewView};
use je_widgets::tree::async_data_tree::{AsyncDataTree, ObjectBackend};
use je_widgets::tree::async_data_tree_node::{AsyncDataTreeOptions, IAsyncDataTreeRenderer};
use je_widgets::tree::tree_types::{IAsyncDataSource, InputOrElement, TreeDragOverResult, TreeEvent, TreeMouseEvent, TreeMouseEventTarget, TreeNodeApi};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::core_2d::dimension::Dimension;
use crate::editor_browser::EditorMouseEvent;
use crate::editor_common::ScrollType;
use crate::editor_extensions::{ITextModelService, ModelReference};
use crate::embedded_code_editor_widget::EmbeddedCodeEditorWidget;
use crate::goto_symbol::peek::references_controller::{IReferenceWidget, SelectionEvent, SelectionKind, SelectionSource, location_of};
use crate::goto_symbol::peek::references_tree::{
    AccessibilityProvider, DataSource, Delegate, FileReferencesRenderer, IdentityProvider, OneReferenceRenderer, ReferencesTreeInput, ReferencesTreeRendererServices, StringRepresentationProvider,
    TreeElement, references_tree_styles,
};
use crate::goto_symbol::references_model::{FileReferences, OneReference, ReferencesModel, ext_uri};
use crate::language_features_service::Location;
use crate::model::{ModelDecorationOptionsInit, ModelDeltaDecoration, TrackedRangeStickiness};
use crate::opener_service::range_to_text_editor_selection;
use crate::peek_view::{
    IPeekViewService, IPeekViewWidget, PEEK_VIEW_BORDER, PEEK_VIEW_TITLE_BACKGROUND, PEEK_VIEW_TITLE_FOREGROUND, PEEK_VIEW_TITLE_INFO_FOREGROUND, PeekViewHooks,
    PeekViewOptions, PeekViewStyles, PeekViewWidget,
};
use crate::range::Range;
use crate::services::theme_service::{IColorTheme, IThemeService};
use crate::text_model::ModelDecorationOptions;
use crate::zone_widget::{ZoneWidgetHost, ZoneWidgetOptions, ZoneWidgetStyles};

thread_local! {
    /// `DecorationsManager.DecorationOptions`.
    static DECORATION_OPTIONS: Rc<ModelDecorationOptions> = ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        description: "reference-decoration".to_owned(),
        stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
        class_name: Some("reference-decoration".to_owned()),
        ..ModelDecorationOptionsInit::default()
    });
}

// #region DecorationsManager

/// `DecorationsManager`: as decorações das referências do arquivo do editor de prévia, que seguem as edições e saem
/// quando uma referência deixa de valer.
struct DecorationsManager {
    me: Weak<DecorationsManager>,
    editor: Rc<CodeEditorWidget>,
    model: Rc<ReferencesModel>,
    /// `_decorations`, na ordem de inserção (um `Map`).
    decorations: RefCell<Vec<(String, Rc<OneReference>)>>,
    decoration_ignore_set: RefCell<HashSet<String>>,
    call_on_dispose: Rc<DisposableStore>,
    call_on_model_change: Rc<DisposableStore>,
}

impl DecorationsManager {
    fn new(editor: Rc<CodeEditorWidget>, model: Rc<ReferencesModel>) -> Rc<Self> {
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            editor,
            model,
            decorations: RefCell::new(Vec::new()),
            decoration_ignore_set: RefCell::new(HashSet::new()),
            call_on_dispose: DisposableStore::new(),
            call_on_model_change: DisposableStore::new(),
        });
        let weak = this.me.clone();
        this.call_on_dispose.add(this.editor.on_did_change_model().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_model_changed();
            }
        }));
        this.on_model_changed();
        this
    }

    /// `_onModelChanged()`.
    fn on_model_changed(&self) {
        self.call_on_model_change.clear();
        let Some(model) = self.editor.get_model() else {
            return;
        };
        let uri = model.uri().to_string(false);
        for reference in self.model.references() {
            if reference.uri().to_string(false) == uri {
                self.add_decorations(&reference.parent());
                return;
            }
        }
    }

    /// `_addDecorations(reference)`.
    fn add_decorations(&self, reference: &FileReferences) {
        let Some(model) = self.editor.get_model() else {
            return;
        };
        let weak = self.me.clone();
        self.call_on_model_change.add(model.on_did_change_decorations().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_decoration_changed();
            }
        }));

        let model_uri = model.uri().to_string(false);
        let children = reference.children();
        let mut new_decorations: Vec<ModelDeltaDecoration> = Vec::new();
        let mut new_decorations_actual_index: Vec<usize> = Vec::new();

        for (index, one_reference) in children.iter().enumerate() {
            if self.decoration_ignore_set.borrow().contains(&one_reference.id) {
                continue;
            }
            if one_reference.uri().to_string(false) != model_uri {
                continue;
            }
            new_decorations.push(ModelDeltaDecoration { range: one_reference.range(), options: DECORATION_OPTIONS.with(Rc::clone) });
            new_decorations_actual_index.push(index);
        }

        self.editor.change_decorations(|change_accessor| {
            let decorations = change_accessor.delta_decorations(&[], &new_decorations);
            let mut stored = self.decorations.borrow_mut();
            for (index, decoration) in decorations.into_iter().enumerate() {
                stored.push((decoration, Rc::clone(&children[new_decorations_actual_index[index]])));
            }
        });
    }

    /// `_onDecorationChanged()`.
    fn on_decoration_changed(&self) {
        let Some(model) = self.editor.get_model() else {
            return;
        };

        let mut to_remove: Vec<String> = Vec::new();
        let entries = self.decorations.borrow().clone();
        for (decoration_id, reference) in &entries {
            let Some(new_range) = model.get_decoration_range(decoration_id) else {
                continue;
            };

            let range = reference.range();
            if new_range == range {
                continue;
            }

            let ignore = if new_range.spans_multiple_lines() {
                true
            } else {
                let line_length = range.end_column - range.start_column;
                let new_line_length = new_range.end_column - new_range.start_column;
                line_length != new_line_length
            };

            if ignore {
                self.decoration_ignore_set.borrow_mut().insert(reference.id.clone());
                to_remove.push(decoration_id.clone());
            } else {
                reference.set_range(new_range);
            }
        }

        self.decorations.borrow_mut().retain(|(decoration_id, _)| !to_remove.contains(decoration_id));
        self.editor.remove_decorations(&to_remove);
    }

    /// `removeDecorations()`.
    fn remove_decorations(&self) {
        let ids: Vec<String> = self.decorations.borrow().iter().map(|(decoration_id, _)| decoration_id.clone()).collect();
        self.editor.remove_decorations(&ids);
        self.decorations.borrow_mut().clear();
    }
}

impl IDisposable for DecorationsManager {
    fn dispose(&self) {
        self.call_on_model_change.dispose();
        self.call_on_dispose.dispose();
        self.remove_decorations();
    }
}

// #endregion

/// `LayoutData`: a proporção entre o editor e a árvore e a altura do peek, guardadas entre aberturas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutData {
    pub ratio: f64,
    pub height_in_lines: f64,
}

impl Default for LayoutData {
    fn default() -> Self {
        Self { ratio: 0.7, height_in_lines: 18.0 }
    }
}

impl LayoutData {
    /// `LayoutData.fromJSON(raw)`: o texto inválido, ou um campo ausente, zero ou que não é número, vale o padrão (o `||`
    /// do original).
    pub fn from_json(raw: &str) -> LayoutData {
        let mut errors = Vec::new();
        let parsed = json::parse_str(raw, &mut errors).filter(|_| errors.is_empty()).map(|value| Dynamic::from_json(&value));
        let number = |key: &str| match &parsed {
            Some(data @ Dynamic::Object(_)) => match data.get_str(key) {
                Dynamic::Number(value) if value != 0.0 && !value.is_nan() => Some(value),
                _ => None,
            },
            _ => None,
        };
        let defaults = LayoutData::default();
        LayoutData { ratio: number("ratio").unwrap_or(defaults.ratio), height_in_lines: number("heightInLines").unwrap_or(defaults.height_in_lines) }
    }

    /// `JSON.stringify(layoutData)`.
    pub fn to_json(&self) -> String {
        json_stringify(&Dynamic::object([("ratio", Dynamic::from(self.ratio)), ("heightInLines", Dynamic::from(self.height_in_lines))])).unwrap_or_else(|| "{}".to_owned())
    }
}

/// `ReferencesDragAndDrop` (o `dnd` da árvore: `getDragURI`, `getDragLabel` e `onDragStart`; `onDragOver` recusa e `drop` não
/// faz nada).
pub struct ReferencesDragAndDrop {
    label_service: Rc<dyn ILabelService>,
}

impl ReferencesDragAndDrop {
    pub fn new(label_service: Rc<dyn ILabelService>) -> Self {
        Self { label_service }
    }
}

impl IListDragAndDrop<TreeElement, TreeDragOverResult> for ReferencesDragAndDrop {
    /// `getDragURI(element)`.
    fn get_drag_uri(&self, element: &TreeElement) -> Option<String> {
        match element {
            TreeElement::File(file) => Some(file.uri.to_string(false).to_string_lossy()),
            TreeElement::Reference(reference) => Some(with_selection(reference.uri(), &range_to_text_editor_selection(&reference.range())).to_string(false).to_string_lossy()),
        }
    }

    /// `getDragLabel(elements)`.
    fn get_drag_label(&self, elements: &[TreeElement], _original_event: &DragEvent) -> Option<String> {
        if elements.is_empty() {
            return None;
        }
        let labels: Vec<String> = elements.iter().map(|element| self.label_service.get_uri_basename_label(element.uri())).collect();
        Some(labels.join(", "))
    }

    /// `onDragStart(data, originalEvent)`: grava no `dataTransfer` a lista de recursos em JSON (`DataTransfers.RESOURCES`) e os
    /// mesmos recursos, um por linha, como texto (`DataTransfers.TEXT`), se há recurso a arrastar.
    fn on_drag_start(&self, data: &Rc<dyn IDragAndDropData>, original_event: &DragEvent) {
        let Some(elements) = data.as_any().downcast_ref::<ElementsDragAndDropData<TreeElement>>() else {
            return;
        };
        let resources: Vec<String> = elements.elements.iter().filter_map(|element| self.get_drag_uri(element)).filter(|resource| !resource.is_empty()).collect();
        if resources.is_empty() {
            return;
        }
        // Apply resources as resource-list
        let list = Dynamic::array(resources.iter().map(|resource| Dynamic::string(resource)));
        if let Some(json) = json_stringify(&list) {
            original_event.data_transfer.set_data(DataTransfers::RESOURCES, &json);
        }
        // Also add as plain text for outside consumers
        original_event.data_transfer.set_data(DataTransfers::TEXT, &resources.join("\n"));
    }

    fn on_drag_over(
        &self,
        _data: &Rc<dyn IDragAndDropData>,
        _target_element: Option<&TreeElement>,
        _target_index: Option<usize>,
        _target_sector: Option<ListViewTargetSector>,
        _original_event: &DragEvent,
    ) -> TreeDragOverResult {
        TreeDragOverResult::Bool(false)
    }

    fn drop(&self, _data: &Rc<dyn IDragAndDropData>, _target_element: Option<&TreeElement>, _target_index: Option<usize>, _target_sector: Option<ListViewTargetSector>, _original_event: &DragEvent) {}
}

// #region ReferencesTree

/// A `AsyncDataTree` das referências.
pub type ReferencesAsyncTree = AsyncDataTree<ReferencesTreeInput, TreeElement, FuzzyScore, ObjectBackend>;

/// `IOpenEvent<TreeElement>`, no que o peek lê: o elemento, `sideBySide` e `editorOptions.pinned`.
#[derive(Clone)]
pub struct ReferencesTreeOpenEvent {
    pub element: TreeElement,
    pub side_by_side: bool,
    pub pinned: bool,
}

/// Os serviços que o `WorkbenchAsyncDataTree` recebe por injeção, no que o peek usa.
#[derive(Clone)]
pub struct ReferencesTreeServices {
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub list_service: Rc<ListService>,
}

/// Um nó da árvore de widgets como alvo do serviço de chaves de contexto escopado (o `HTMLElement` de
/// `createScoped(element)`).
struct TreeNodeTarget {
    ui: Ui,
    node: NodeId,
}

impl IContextKeyServiceTarget for TreeNodeTarget {
    fn parent_element(&self) -> Option<Rc<dyn IContextKeyServiceTarget>> {
        let parent = self.ui.tree().parent(self.node)?;
        Some(Rc::new(TreeNodeTarget { ui: self.ui.clone(), node: parent }))
    }

    fn set_attribute(&self, attr: &str, value: &str) {
        self.ui.tree_mut().set_attribute(self.node, attr, value);
    }

    fn remove_attribute(&self, attr: &str) {
        self.ui.tree_mut().remove_attribute(self.node, attr);
    }

    fn has_attribute(&self, attr: &str) -> bool {
        self.ui.tree().has_attribute(self.node, attr)
    }

    fn get_attribute(&self, attr: &str) -> Option<String> {
        self.ui.tree().attribute(self.node, attr).map(str::to_owned)
    }
}

/// A árvore como o [`ListService`] a guarda (`WorkbenchListWidget`).
struct ReferencesTreeListWidget {
    ui: Ui,
    tree: Rc<ReferencesAsyncTree>,
}

impl IListWidget for ReferencesTreeListWidget {
    fn is_active_element(&self) -> bool {
        self.tree.is_dom_focused()
    }

    fn get_focused_element(&self) -> Option<Rc<dyn std::any::Any>> {
        self.tree.get_focus().first().map(TreeElement::as_any)
    }

    fn workbench_data_tree_input(&self) -> Option<Option<Rc<dyn std::any::Any>>> {
        Some(self.tree.get_input().map(|input| input.as_any()))
    }

    fn on_did_focus(&self) -> BaseEvent<()> {
        let (ui, node) = (self.ui.clone(), self.tree.get_html_element());
        BaseEvent::new(move |listener| ui.listen(node, EventKind::Focus, false, move |_| listener(&())))
    }

    fn on_did_dispose(&self) -> BaseEvent<()> {
        self.tree.on_did_dispose()
    }

    fn set_last_focused(&self, last_focused: bool) {
        self.ui.tree_mut().toggle_class(self.tree.get_html_element(), "last-focused", last_focused);
    }
}

/// `ReferencesTree extends WorkbenchAsyncDataTree<ReferencesModel | FileReferences, TreeElement, FuzzyScore>` (ver o topo do
/// módulo).
pub struct ReferencesTree {
    tree: Rc<ReferencesAsyncTree>,
    on_did_open: Rc<Emitter<ReferencesTreeOpenEvent>>,
    base: Rc<Disposable>,
}

impl ReferencesTree {
    /// `new ReferencesTree(user, container, delegate, renderers, dataSource, options)`; `override_styles` é o
    /// `overrideStyles` das opções.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ui: &Ui,
        container: NodeId,
        delegate: Rc<dyn IListVirtualDelegate<TreeElement>>,
        renderers: Vec<Rc<dyn IAsyncDataTreeRenderer<ReferencesTreeInput, TreeElement, FuzzyScore>>>,
        data_source: Rc<dyn IAsyncDataSource<ReferencesTreeInput, TreeElement>>,
        options: AsyncDataTreeOptions<TreeElement, FuzzyScore>,
        override_styles: &IListStyles,
        services: &ReferencesTreeServices,
    ) -> Rc<Self> {
        let tree = AsyncDataTree::<_, _, _, ObjectBackend>::new("ReferencesWidget", ui, container, delegate, renderers, data_source, options);
        tree.style(override_styles);

        let base = Disposable::new();
        let on_did_open = Emitter::<ReferencesTreeOpenEvent>::new();
        base.register(on_did_open.clone());

        // O `ResourceNavigator` com `openOnSingleClick: true`.
        {
            let (emitter, weak_tree) = (on_did_open.clone(), Rc::downgrade(&tree));
            base.register(tree.on_did_change_selection().listen(move |event: &TreeEvent<TreeElement>| {
                // onSelectionFromKeyboard
                if !event.browser_event.is_some_and(|browser_event| browser_event.is_keyboard) || event.elements.len() != 1 {
                    return;
                }
                if let Some(tree) = weak_tree.upgrade() {
                    // preserveFocus: true, pinned: false, sideBySide: false
                    Self::open(&emitter, tree.get_selection().into_iter().next(), false, false);
                }
            }));
        }
        {
            let emitter = on_did_open.clone();
            base.register(tree.on_pointer().listen(move |event: &TreeMouseEvent<TreeElement>| {
                // onPointer
                if event.browser_event.click_count == 2 {
                    return;
                }
                let is_middle_click = event.browser_event.button == PointerButton::Auxiliary;
                let modifiers = event.browser_event.modifiers;
                Self::open(&emitter, event.element.clone(), is_middle_click, modifiers.ctrl || modifiers.meta || modifiers.alt);
            }));
        }
        {
            let emitter = on_did_open.clone();
            base.register(tree.on_mouse_dbl_click().listen(move |event: &TreeMouseEvent<TreeElement>| {
                // onMouseDblClick: copied from AbstractTree
                if event.target == TreeMouseEventTarget::Twistie {
                    return;
                }
                let modifiers = event.browser_event.modifiers;
                Self::open(&emitter, event.element.clone(), true, modifiers.ctrl || modifiers.meta || modifiers.alt);
            }));
        }

        // O registro no `ListService` do `WorkbenchTreeInternals`.
        let list_widget: Rc<dyn IListWidget> = Rc::new(ReferencesTreeListWidget { ui: ui.clone(), tree: Rc::clone(&tree) });
        base.register(services.list_service.register(list_widget, Vec::new()));

        // As chaves de contexto do `WorkbenchTreeInternals` que o peek lê.
        let context_key_service = services.context_key_service.create_scoped(Rc::new(TreeNodeTarget { ui: ui.clone(), node: tree.get_html_element() }));
        {
            let keys: &dyn IContextKeyService = &*context_key_service;
            raw_workbench_list_focus_context_key().bind_to(keys);
            workbench_list_selection_navigation().bind_to(keys).set(Dynamic::Bool(true));
            let has_selection_or_focus = workbench_list_has_selection_or_focus().bind_to(keys);
            let tree_element_can_collapse = workbench_tree_element_can_collapse().bind_to(keys);
            let tree_element_has_parent = workbench_tree_element_has_parent().bind_to(keys);
            let tree_element_can_expand = workbench_tree_element_can_expand().bind_to(keys);
            let tree_element_has_child = workbench_tree_element_has_child().bind_to(keys);

            let update_has_selection_or_focus = {
                let (weak_tree, has_selection_or_focus) = (Rc::downgrade(&tree), Rc::clone(&has_selection_or_focus));
                move || {
                    if let Some(tree) = weak_tree.upgrade() {
                        has_selection_or_focus.set(Dynamic::Bool(!tree.get_selection().is_empty() || !tree.get_focus().is_empty()));
                    }
                }
            };
            // updateCollapseContextKeys
            let update_collapse_context_keys = {
                let weak_tree = Rc::downgrade(&tree);
                move || {
                    let Some(tree) = weak_tree.upgrade() else {
                        return;
                    };
                    let Some(focus) = tree.get_focus().into_iter().next() else {
                        return;
                    };
                    let element = InputOrElement::Element(focus.clone());
                    let node = tree.get_node(Some(&element));
                    tree_element_can_collapse.set(Dynamic::Bool(node.collapsible() && !node.collapsed()));
                    tree_element_has_parent.set(Dynamic::Bool(tree.get_parent_element(&focus).is_some()));
                    tree_element_can_expand.set(Dynamic::Bool(node.collapsible() && node.collapsed()));
                    tree_element_has_child.set(Dynamic::Bool(tree.get_first_element_child(Some(&element)).is_some()));
                }
            };

            let update = update_has_selection_or_focus.clone();
            base.register(tree.on_did_change_selection().listen(move |_| update()));
            let (update, update_collapse) = (update_has_selection_or_focus, update_collapse_context_keys.clone());
            base.register(tree.on_did_change_focus().listen(move |_| {
                update();
                update_collapse();
            }));
            let update_collapse = update_collapse_context_keys.clone();
            base.register(tree.on_did_change_collapse_state().listen(move |_| update_collapse()));
            base.register(tree.on_did_change_model().listen(move |_| update_collapse_context_keys()));
        }
        base.register(context_key_service);

        Rc::new(Self { tree, on_did_open, base })
    }

    /// `_open(element, preserveFocus = true, pinned, sideBySide)`: nada sem elemento.
    fn open(emitter: &Emitter<ReferencesTreeOpenEvent>, element: Option<TreeElement>, pinned: bool, side_by_side: bool) {
        if let Some(element) = element {
            emitter.fire(ReferencesTreeOpenEvent { element, side_by_side, pinned });
        }
    }

    /// A árvore de dados por baixo (os métodos do `AsyncDataTree` que o `WorkbenchAsyncDataTree` herda).
    pub fn tree(&self) -> &Rc<ReferencesAsyncTree> {
        &self.tree
    }

    /// `onDidOpen`.
    pub fn on_did_open(&self) -> BaseEvent<ReferencesTreeOpenEvent> {
        self.on_did_open.event()
    }
}

impl IDisposable for ReferencesTree {
    fn dispose(&self) {
        self.base.dispose();
        self.tree.dispose();
    }
}

// #endregion

/// O que o `createInstance(EmbeddedCodeEditorWidget, this._previewContainer, options, {}, this.editor)` do `_fillBody` pede
/// a quem monta os editores: o editor embutido, dentro do contêiner de prévia, com as opções por cima das do editor pai e as
/// opções de widget vazias (`{}`).
pub trait IReferencePreviewEditorFactory {
    fn create(&self, container: NodeId, options: Dynamic, parent_editor: &Rc<CodeEditorWidget>) -> Rc<EmbeddedCodeEditorWidget>;
}

/// Os serviços que o construtor de `ReferenceWidget` recebe por injeção (ver o topo do módulo).
#[derive(Clone)]
pub struct ReferenceWidgetServices {
    /// Onde o peek vive na árvore de widgets do editor (o `Ui` e o contêiner dos widgets de overlay).
    pub zone_host: Rc<dyn Fn(&Rc<CodeEditorWidget>) -> ZoneWidgetHost>,
    pub theme_service: Rc<dyn IThemeService>,
    pub text_model_service: Rc<dyn ITextModelService>,
    pub peek_view_service: Rc<dyn IPeekViewService>,
    pub label_service: Rc<dyn ILabelService>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub list_service: Rc<ListService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub preview_editor_factory: Rc<dyn IReferencePreviewEditorFactory>,
}

/// Uma view do `SplitView` do peek: o `ViewDescriptor` de `splitView.addView({ onDidChange: Event.None, element, minimumSize,
/// maximumSize: Number.MAX_VALUE, layout }, Sizing.Distribute)`.
struct PaneView {
    element: NodeId,
    minimum_size: f64,
    layout: Box<dyn Fn(f64)>,
}

impl SplitViewView<()> for PaneView {
    fn element(&self) -> NodeId {
        self.element
    }

    fn minimum_size(&self) -> f64 {
        self.minimum_size
    }

    fn maximum_size(&self) -> f64 {
        f64::MAX
    }

    fn on_did_change(&self) -> BaseEvent<Option<f64>> {
        BaseEvent::none()
    }

    fn layout(&self, size: f64, _offset: f64, _context: Option<&()>) {
        (self.layout)(size);
    }
}

/// O que o `_fillBody` cria: a mensagem, o split view, o editor de prévia e a árvore.
struct Parts {
    message_container: NodeId,
    split_view: Rc<SplitView>,
    preview_container: NodeId,
    preview: Rc<EmbeddedCodeEditorWidget>,
    tree_container: NodeId,
    tree: Rc<ReferencesTree>,
}

/// `ReferenceWidget`: o `ZoneWidget` mostrado dentro do editor.
pub struct ReferenceWidget {
    me: Weak<ReferenceWidget>,
    peek: Rc<PeekViewWidget>,
    default_tree_keyboard_support: bool,
    services: ReferenceWidgetServices,
    /// `layoutData`.
    layout_data: Cell<LayoutData>,
    /// `_model`.
    model: RefCell<Option<Rc<ReferencesModel>>>,
    dispose_on_new_model: Rc<DisposableStore>,
    call_on_dispose: Rc<DisposableStore>,
    /// `_onDidSelectReference`.
    on_did_select_reference: Rc<Emitter<SelectionEvent>>,
    parts: OnceCell<Parts>,
    /// `_previewModelReference`.
    preview_model_reference: RefCell<Option<ModelReference>>,
    /// `_dim` (largura e altura).
    dim: Cell<(f64, f64)>,
    /// `_isClosing`: se um `dispose` já está em andamento.
    is_closing: Cell<bool>,
    disposed: Cell<bool>,
    /// `_revealedReference`.
    revealed_reference: RefCell<Option<Rc<OneReference>>>,
}

impl ReferenceWidget {
    /// O construtor (`editor`, `_defaultTreeKeyboardSupport`, `layoutData`).
    pub fn new(editor: &Rc<CodeEditorWidget>, default_tree_keyboard_support: bool, layout_data: LayoutData, services: ReferenceWidgetServices) -> Rc<Self> {
        let host = (services.zone_host)(editor);
        let peek = PeekViewWidget::new(
            editor,
            host,
            PeekViewOptions {
                zone: ZoneWidgetOptions { show_frame: Some(false), show_arrow: Some(true), is_resizeable: Some(true), is_accessible: Some(true), ..ZoneWidgetOptions::default() },
                support_on_title_click: Some(true),
                ..PeekViewOptions::default()
            },
        );
        let call_on_dispose = DisposableStore::new();
        let on_did_select_reference = Emitter::<SelectionEvent>::new();
        call_on_dispose.add_rc(on_did_select_reference.clone());

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            peek,
            default_tree_keyboard_support,
            services,
            layout_data: Cell::new(layout_data),
            model: RefCell::new(None),
            dispose_on_new_model: DisposableStore::new(),
            call_on_dispose,
            on_did_select_reference,
            parts: OnceCell::new(),
            preview_model_reference: RefCell::new(None),
            dim: Cell::new((0.0, 0.0)),
            is_closing: Cell::new(false),
            disposed: Cell::new(false),
            revealed_reference: RefCell::new(None),
        });
        this.peek.set_hooks(Rc::downgrade(&this) as Weak<dyn PeekViewHooks>);

        this.apply_theme(&*this.services.theme_service.get_color_theme());
        let weak = this.me.clone();
        this.call_on_dispose.add(this.services.theme_service.on_did_color_theme_change().listen(move |theme| {
            if let Some(this) = weak.upgrade() {
                this.apply_theme(&**theme);
            }
        }));
        let exclusive: Rc<dyn IPeekViewWidget> = this.clone();
        this.services.peek_view_service.add_exclusive_widget(editor, &exclusive);
        this.peek.create();
        this
    }

    fn rc(&self) -> Rc<ReferenceWidget> {
        self.me.upgrade().expect("o widget está vivo enquanto é usado")
    }

    fn ui(&self) -> Ui {
        self.peek.zone().host().ui.clone()
    }

    /// O que o `_fillBody` criou.
    fn parts(&self) -> &Parts {
        self.parts.get().expect("o corpo do peek é criado em `create`")
    }

    /// `_applyTheme(theme)`.
    fn apply_theme(&self, theme: &dyn IColorTheme) {
        let border_color = theme.get_color(PEEK_VIEW_BORDER, None).unwrap_or_else(Color::transparent_color);
        self.peek.style(PeekViewStyles {
            zone: ZoneWidgetStyles { arrow_color: Some(border_color.clone()), frame_color: Some(border_color) },
            header_background_color: Some(theme.get_color(PEEK_VIEW_TITLE_BACKGROUND, None).unwrap_or_else(Color::transparent_color)),
            primary_heading_color: theme.get_color(PEEK_VIEW_TITLE_FOREGROUND, None),
            secondary_heading_color: theme.get_color(PEEK_VIEW_TITLE_INFO_FOREGROUND, None),
        });
        // As cores da árvore são variáveis de CSS no original: mudam com o tema sem refazer nada.
        if let Some(parts) = self.parts.get() {
            let tree = parts.tree.tree();
            tree.style(&references_tree_styles(theme).list);
            tree.rerender(None);
        }
    }

    /// `_getFocusedReference()`.
    fn get_focused_reference(&self) -> Option<Rc<OneReference>> {
        match self.parts().tree.tree().get_focus().into_iter().next()? {
            TreeElement::Reference(reference) => Some(reference),
            TreeElement::File(file) => file.children().first().cloned(),
        }
    }

    /// O `onEvent` do `onDidOpen` da árvore.
    fn on_tree_event(&self, element: &TreeElement, kind: SelectionKind) {
        if let TreeElement::Reference(reference) = element {
            if kind == SelectionKind::Show {
                let _ = self.rc().reveal_reference_inner(reference, false);
            }
            self.on_did_select_reference.fire(SelectionEvent { element: Some(location_of(reference)), kind, source: SelectionSource::Tree });
        }
    }

    /// `_onNewModel()`.
    fn on_new_model(&self) -> Promise<()> {
        let Some(model) = self.model.borrow().clone() else {
            return Promise::resolved(());
        };
        let parts = self.parts();
        let ui = self.ui();

        if model.is_empty() {
            self.peek.set_title("", None);
            let mut tree = ui.tree_mut();
            tree.set_label(parts.message_container, localize("noResults", "No results", &[]));
            show(&mut tree, parts.message_container);
            return Promise::resolved(());
        }

        hide(&mut ui.tree_mut(), parts.message_container);
        let decorations_manager = DecorationsManager::new(Rc::clone(parts.preview.editor()), Rc::clone(&model));
        self.dispose_on_new_model.add_rc(decorations_manager);

        // listen on model changes
        let weak_tree = Rc::downgrade(parts.tree.tree());
        self.dispose_on_new_model.add(model.on_did_change_reference_range().listen(move |reference: &Rc<OneReference>| {
            if let Some(tree) = weak_tree.upgrade() {
                let element = TreeElement::Reference(Rc::clone(reference));
                if tree.has_node(&InputOrElement::Element(element.clone())) {
                    tree.rerender(Some(&element));
                }
            }
        }));

        // listen on editor
        let weak = self.me.clone();
        self.dispose_on_new_model.add(parts.preview.editor().on_mouse_down().listen(move |event: &EditorMouseEvent| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            if event.event.detail != 2 {
                return;
            }
            let (Some(element), Some(range)) = (this.get_focused_reference(), event.target.range()) else {
                return;
            };
            let kind = if event.event.ctrl_key || event.event.meta_key || event.event.alt_key { SelectionKind::Side } else { SelectionKind::Open };
            this.on_did_select_reference.fire(SelectionEvent { element: Some(Location { uri: element.uri().clone(), range }), kind, source: SelectionSource::Editor });
        }));

        // make sure things are rendered
        if let Some(container) = self.peek.zone().container() {
            ui.tree_mut().add_class(container, "results-loaded");
        }
        {
            let mut tree = ui.tree_mut();
            show(&mut tree, parts.tree_container);
            show(&mut tree, parts.preview_container);
        }
        parts.split_view.layout(self.dim.get().0, None);
        self.focus_on_reference_tree();

        // pick input and a reference to begin with
        let groups = model.groups();
        let input = match groups.as_slice() {
            [group] => ReferencesTreeInput::File(Rc::clone(group)),
            _ => ReferencesTreeInput::Model(model),
        };
        parts.tree.tree().set_input(input, None)
    }

    /// `private async _revealReference(reference, revealParent)`.
    fn reveal_reference_inner(self: &Rc<Self>, reference: &Rc<OneReference>, reveal_parent: bool) -> Promise<()> {
        let this = Rc::clone(self);
        let reference = Rc::clone(reference);
        async_fn(async move {
            // check if there is anything to do...
            if this.revealed_reference.borrow().as_ref().is_some_and(|revealed| Rc::ptr_eq(revealed, &reference)) {
                return Ok(());
            }
            *this.revealed_reference.borrow_mut() = Some(Rc::clone(&reference));

            // Update widget header
            if reference.uri().scheme() != schemas::IN_MEMORY {
                let directory = this.services.label_service.get_uri_label(&ext_uri().dirname(reference.uri()), &UriLabelOptions::default());
                this.peek.set_title(&ext_uri().basename_or_authority(reference.uri()), Some(directory.as_str()));
            } else {
                this.peek.set_title(&localize("peekView.alternateTitle", "References", &[]), None);
            }

            let promise = this.services.text_model_service.create_model_reference(reference.uri());

            let tree = Rc::clone(this.parts().tree.tree());
            let parent = reference.parent();
            if tree.get_input().is_some_and(|input| input.is_file(&parent)) {
                tree.reveal(&TreeElement::Reference(Rc::clone(&reference)), None);
            } else {
                if reveal_parent {
                    tree.reveal(&TreeElement::File(Rc::clone(&parent)), None);
                }
                tree.expand(&TreeElement::File(Rc::clone(&parent)), false).await?;
                tree.reveal(&TreeElement::Reference(Rc::clone(&reference)), None);
            }

            let model_reference = promise.await?;

            if this.model.borrow().is_none() {
                // disposed
                model_reference.dispose();
                return Ok(());
            }

            let previous = this.preview_model_reference.borrow_mut().take();
            if let Some(previous) = previous {
                previous.dispose();
            }

            // show in editor
            let model = Rc::clone(&model_reference.object);
            let preview = this.parts().preview.editor();
            let scroll_type = if preview.get_model().is_some_and(|current| Rc::ptr_eq(&current, &model)) { ScrollType::Smooth } else { ScrollType::Immediate };
            let selection = reference.range().collapse_to_start();
            *this.preview_model_reference.borrow_mut() = Some(model_reference);
            preview.set_model(Some(model));
            preview.set_selection(selection.into(), None);
            preview.reveal_range_in_center(selection, Some(scroll_type));
            Ok::<(), Error>(())
        })
    }
}

impl PeekViewHooks for ReferenceWidget {
    /// `_onTitleClick(e)`.
    fn on_title_click(&self, _peek: &PeekViewWidget, event: &mut UiEvent) {
        let Some(parts) = self.parts.get() else {
            return;
        };
        if parts.preview.editor().get_model().is_some() {
            let modifiers = event.pointer.map(|pointer| pointer.modifiers).unwrap_or_default();
            let kind = if modifiers.ctrl || modifiers.meta || modifiers.alt { SelectionKind::Side } else { SelectionKind::Open };
            self.on_did_select_reference.fire(SelectionEvent { element: self.get_focused_reference().map(|reference| location_of(&reference)), kind, source: SelectionSource::Title });
        }
    }

    /// `_fillBody(containerElement)`.
    fn fill_body(&self, container: NodeId) {
        self.peek.zone().set_css_class("reference-zone-widget", None);
        let ui = self.ui();
        let services = &self.services;
        let editor = Rc::clone(self.peek.editor());

        // message pane
        let message_container = {
            let mut tree = ui.tree_mut();
            let message_container = create_el(&mut tree, container, Role::Generic, "messages");
            tree.set_fill(message_container, true, true);
            hide(&mut tree, message_container);
            message_container
        };

        let split_view: Rc<SplitView> = SplitView::new(&ui, container, SplitViewOptions { orientation: Some(Orientation::Horizontal), ..SplitViewOptions::default() });

        // editor
        let (preview_container, tree_container) = {
            let mut tree = ui.tree_mut();
            (create_detached_el(&mut tree, Role::Generic, "preview inline"), create_detached_el(&mut tree, Role::Generic, "ref-tree inline"))
        };
        let options = Dynamic::object([
            ("scrollBeyondLastLine", Dynamic::from(false)),
            (
                "scrollbar",
                Dynamic::object([
                    ("verticalScrollbarSize", Dynamic::from(14)),
                    ("horizontal", Dynamic::from("auto")),
                    ("useShadows", Dynamic::from(true)),
                    ("verticalHasArrows", Dynamic::from(false)),
                    ("horizontalHasArrows", Dynamic::from(false)),
                    ("alwaysConsumeMouseWheel", Dynamic::from(true)),
                ]),
            ),
            ("overviewRulerLanes", Dynamic::from(2)),
            ("fixedOverflowWidgets", Dynamic::from(true)),
            ("minimap", Dynamic::object([("enabled", Dynamic::from(false))])),
        ]);
        let preview = services.preview_editor_factory.create(preview_container, options, &editor);
        hide(&mut ui.tree_mut(), preview_container);

        // tree
        let renderer_services = ReferencesTreeRendererServices { ui: ui.clone(), label_service: Rc::clone(&services.label_service), theme_service: Rc::clone(&services.theme_service) };
        let mut tree_options = AsyncDataTreeOptions::<TreeElement, FuzzyScore>::default();
        tree_options.keyboard_support = Some(self.default_tree_keyboard_support);
        tree_options.accessibility_provider = Some(Rc::new(AccessibilityProvider));
        tree_options.keyboard_navigation_label_provider = Some(Rc::new(StringRepresentationProvider));
        tree_options.identity_provider = Some(Rc::new(IdentityProvider));
        tree_options.dnd = Some(Rc::new(ReferencesDragAndDrop::new(Rc::clone(&services.label_service))));
        if self.default_tree_keyboard_support {
            // the tree will consume `Escape` and prevent the widget from closing
            let keybinding_service = Rc::clone(&services.keybinding_service);
            self.call_on_dispose.add(ui.listen(tree_container, EventKind::KeyDown, true, move |event| {
                let Some(key) = StandardKeyboardEvent::from_event(event) else {
                    return;
                };
                if key.equals(KeyCode::Escape.bits()) {
                    keybinding_service.dispatch_event(
                        &KeyboardEvent {
                            ctrl_key: key.ctrl_key,
                            shift_key: key.shift_key,
                            alt_key: key.alt_key,
                            meta_key: key.meta_key,
                            alt_graph_key: false,
                            key_code: key.key_code,
                            code: None,
                        },
                        None,
                    );
                    event.stop_propagation();
                }
            }));
        }
        let renderers: Vec<Rc<dyn IAsyncDataTreeRenderer<ReferencesTreeInput, TreeElement, FuzzyScore>>> =
            vec![Rc::new(FileReferencesRenderer::new(renderer_services.clone())), Rc::new(OneReferenceRenderer::new(renderer_services))];
        let tree = ReferencesTree::new(
            &ui,
            tree_container,
            Rc::new(Delegate),
            renderers,
            Rc::new(DataSource::new(Rc::clone(&services.text_model_service))),
            tree_options,
            &references_tree_styles(&*services.theme_service.get_color_theme()).list,
            &ReferencesTreeServices { context_key_service: Rc::clone(&services.context_key_service), list_service: Rc::clone(&services.list_service) },
        );

        // split stuff
        let weak = self.me.clone();
        split_view.add_view(
            Rc::new(PaneView {
                element: preview_container,
                minimum_size: 200.0,
                layout: Box::new({
                    let weak = weak.clone();
                    move |width| {
                        if let Some(this) = weak.upgrade()
                            && let Some(parts) = this.parts.get()
                        {
                            parts.preview.editor().layout(Some(Dimension { width, height: this.dim.get().1 }));
                        }
                    }
                }),
            }),
            Sizing::Distribute,
            None,
            false,
        );
        split_view.add_view(
            Rc::new(PaneView {
                element: tree_container,
                minimum_size: 100.0,
                layout: Box::new({
                    let weak = weak.clone();
                    move |width| {
                        if let Some(this) = weak.upgrade()
                            && let Some(parts) = this.parts.get()
                        {
                            let height = this.dim.get().1;
                            this.ui().tree_mut().set_bounds(parts.tree_container, je_render::kurbo::Rect::new(0.0, 0.0, width, height));
                            parts.tree.tree().layout(Some(height), Some(width));
                        }
                    }
                }),
            }),
            Sizing::Distribute,
            None,
            false,
        );

        self.peek.zone().disposables().add({
            let weak = weak.clone();
            split_view.on_did_sash_change().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    let (width, _) = this.dim.get();
                    if width != 0.0
                        && let Some(parts) = this.parts.get()
                    {
                        let mut layout_data = this.layout_data.get();
                        layout_data.ratio = parts.split_view.get_view_size(0) / width;
                        this.layout_data.set(layout_data);
                    }
                }
            })
        });

        // listen on selection and focus
        self.peek.zone().disposables().add(tree.on_did_open().listen(move |event: &ReferencesTreeOpenEvent| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            if event.side_by_side {
                this.on_tree_event(&event.element, SelectionKind::Side);
            } else if event.pinned {
                this.on_tree_event(&event.element, SelectionKind::Goto);
            } else {
                this.on_tree_event(&event.element, SelectionKind::Show);
            }
        }));

        hide(&mut ui.tree_mut(), tree_container);

        self.parts.set(Parts { message_container, split_view, preview_container, preview, tree_container, tree }).ok().expect("o corpo do peek é criado uma vez");
    }

    /// `_onWidth(widthInPixel)`.
    fn on_width(&self, peek: &PeekViewWidget, width_in_pixel: f64) {
        self.do_layout_body(peek, self.dim.get().1, width_in_pixel);
    }

    /// `_doLayoutBody(heightInPixel, widthInPixel)`.
    fn do_layout_body(&self, peek: &PeekViewWidget, height_in_pixel: f64, width_in_pixel: f64) {
        peek.do_layout_body_base(height_in_pixel, width_in_pixel);
        self.dim.set((width_in_pixel, height_in_pixel));
        if let Some(height_in_lines) = peek.zone().view_zone_height_in_lines() {
            let mut layout_data = self.layout_data.get();
            layout_data.height_in_lines = height_in_lines;
            self.layout_data.set(layout_data);
        }
        if let Some(parts) = self.parts.get() {
            parts.split_view.layout(width_in_pixel, None);
            parts.split_view.resize_view(0, width_in_pixel * self.layout_data.get().ratio);
        }
    }

    /// `_applyStyles()` com `.reference-zone-widget { border-top-width: 1px; border-bottom-width: 1px }`.
    fn apply_styles(&self, peek: &PeekViewWidget) {
        peek.apply_styles_base();
        let zone = peek.zone();
        if let (Some(container), Some(frame_color)) = (zone.container(), zone.options().frame_color) {
            let mut tree = zone.host().ui.tree_mut();
            let style = tree.style_mut(container);
            style.border = Some(Border { width: 1.0, color: to_rgba8(&frame_color) });
            style.border_widths = Some([1.0, 0.0, 1.0, 0.0]);
        }
    }
}

impl IDisposable for ReferenceWidget {
    fn dispose(&self) {
        self.is_closing.set(true);
        if self.disposed.replace(true) {
            return;
        }
        let _ = self.set_model(None);
        self.call_on_dispose.dispose();
        self.dispose_on_new_model.dispose();
        if let Some(parts) = self.parts.get() {
            parts.preview.dispose();
            parts.tree.dispose();
        }
        let model_reference = self.preview_model_reference.borrow_mut().take();
        if let Some(model_reference) = model_reference {
            model_reference.dispose();
        }
        if let Some(parts) = self.parts.get() {
            parts.split_view.dispose();
        }
        self.peek.dispose();
    }
}

impl IPeekViewWidget for ReferenceWidget {
    fn on_did_close(&self) -> BaseEvent<()> {
        self.peek.on_did_close()
    }
}

impl IReferenceWidget for ReferenceWidget {
    fn set_title(&self, primary_heading: &str, secondary_heading: Option<&str>) {
        self.peek.set_title(primary_heading, secondary_heading);
    }

    fn set_meta_title(&self, value: &str) {
        self.peek.set_meta_title(value);
    }

    /// `show(where)`: a altura vem do `layoutData` (`heightInLines || 18`).
    fn show(&self, range: Range) {
        let height_in_lines = self.layout_data.get().height_in_lines;
        self.peek.zone().show(range, if height_in_lines == 0.0 || height_in_lines.is_nan() { 18.0 } else { height_in_lines });
    }

    fn hide(&self) {
        self.peek.zone().hide();
    }

    fn position(&self) -> Option<crate::position::Position> {
        self.peek.zone().position()
    }

    fn on_did_select_reference(&self) -> BaseEvent<SelectionEvent> {
        self.on_did_select_reference.event()
    }

    fn layout_data_json(&self) -> String {
        self.layout_data.get().to_json()
    }

    fn is_closing(&self) -> bool {
        self.is_closing.get()
    }

    fn set_model(&self, new_model: Option<Rc<ReferencesModel>>) -> Promise<()> {
        // clean up
        self.dispose_on_new_model.clear();
        *self.model.borrow_mut() = new_model.clone();
        if new_model.is_some() {
            return self.on_new_model();
        }
        Promise::resolved(())
    }

    fn set_selection(&self, selection: &Rc<OneReference>) -> Promise<()> {
        let this = self.rc();
        let selection = Rc::clone(selection);
        this.reveal_reference_inner(&selection, true).then(move |()| {
            if this.model.borrow().is_none() {
                // disposed
                return;
            }
            // show in tree
            let tree = this.parts().tree.tree();
            let element = TreeElement::Reference(selection);
            tree.set_selection(&[element.clone()], None);
            tree.set_focus(&[element], None);
        })
    }

    fn focus_on_reference_tree(&self) {
        self.parts().tree.tree().dom_focus();
    }

    fn focus_on_preview_editor(&self) {
        self.parts().preview.editor().focus();
    }

    fn is_preview_editor_focused(&self) -> bool {
        self.parts().preview.editor().has_text_focus()
    }

    fn reveal_reference(&self, reference: &Rc<OneReference>) -> Promise<()> {
        let this = self.rc();
        let reference = Rc::clone(reference);
        async_fn(async move {
            this.reveal_reference_inner(&reference, false).await?;
            this.on_did_select_reference.fire(SelectionEvent { element: Some(location_of(&reference)), kind: SelectionKind::Goto, source: SelectionSource::Tree });
            Ok::<(), Error>(())
        })
    }
}
