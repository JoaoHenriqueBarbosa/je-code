//! Porte de `src/vs/editor/browser/widget/multiDiffEditor/diffEditorItemTemplate.ts`: o modelo (`template`) de um arquivo do
//! editor de diff de vários arquivos, com o cabeçalho (caminho, status, recolher, ações), o editor de diff embutido, o
//! marcador de arquivo binário, a altura do item e a ligação (`binding`) que o gerenciador de itens virtualizados cria.
//!
//! Divergências:
//! - **Sem DOM.** O `h('div.multiDiffEntry', ...)` e os estilos escritos nos elementos viram o dado
//!   [`DiffEditorItemViewState`], que o pintor nativo lê ([`DiffEditorItemTemplate::view_state`]): posição, visibilidade,
//!   classes (`active`, `first-diff-entry`, `header-hovered`, `header-focused`, `shadow`, `collapsed`, `renamed`, `deleted`,
//!   `added`), `translateY` do cabeçalho (o sticky), ícone do botão de recolher, `aria-expanded`, `tabIndex` e `role`. O
//!   `appendChild(root)` no contêiner não existe. O foco pedido a um elemento (`binaryFilePlaceholder.focus()` ou o botão
//!   `Open Diff`) é o [`FocusTarget`] deixado em `focus_request`, que o dono do contêiner consome
//!   ([`DiffEditorItemTemplate::take_focus_request`]).
//! - **Eventos do DOM viram métodos.** Os `addDisposableListener`/`trackFocus` do cabeçalho, do botão de recolher, do
//!   marcador binário e do botão `Open Diff` são os métodos `on_*`, que o dono do contêiner chama quando o evento
//!   nativo chega. Cada um começa pela condição que, no original, decide se o ouvinte é registrado
//!   (`headerClickToCollapse`, `handleHeaderMiddleClick`). O `EventHelper.stop` e o `preventDefault` são o `bool`
//!   devolvido. `target.closest('.actions')` e `closest('.collapse-button')` são o [`HeaderEventTarget`].
//! - **`classList.toggle(name, undefined)`.** Com o item ausente, `isActive` e `isFirst` são `undefined` e o `toggle` do
//!   original inverte a classe em vez de forçá-la; aqui um item ausente força `false` (o elemento está escondido nessa
//!   hora e o próximo `setItem` força o valor certo).
//! - **O editor de diff embutido** é o [`IDiffEditorWidget`] (ver `diff_editor_surface`), criado por uma closure; o
//!   `DocumentDiffItemViewModel` é o [`IDocumentDiffItemViewModel`] (ver `view_model`), porque o view model mora no
//!   `je-workbench`.
//! - **Barra de ações, rótulos de recurso e botão `Open Diff`.** A barra e os rótulos são criados pelas closures do
//!   [`IWorkbenchUIElementFactory`]; sem `create_file_toolbar` o arquivo fica sem barra. O botão `Open Diff` existe quando a
//!   fábrica traz `open_diff_editor`, e o clique é [`DiffEditorItemTemplate::on_open_binary_diff_button_click`]. A cor do
//!   cabeçalho (`multiDiffEditorHeaderBackground`) é pedida ao tema por quem pinta (ver `colors`).
//! - **Binding e template.** `DiffEditorItemBinding` guarda um `Weak` do template (o template guarda a ligação atual, o que
//!   fecharia um ciclo de `Rc`); `createBinding` recebe o `Rc` do template por `me`. A herança de `VirtualizedItemTemplate`
//!   e `VirtualizedItemBinding` é composição (ver `virtualized_item_manager`).
//! - **`maxScroll`.** O valor do original traz também a `width` do lado mais largo; o [`MaxScroll`] do porte só guarda o
//!   `maxScroll`, que é o que a vista virtualizada lê.
//! - **Janela do `requestAnimationFrame`.** `getWindow(this._elements.root)` é a janela que o dono do contêiner passa.
//! - **Números.** Alturas de editor são `f64` (o `number` do editor), posições e tamanhos do layout virtualizado são `i32`.
//!   `optionsOverride` é um observável de `Rc<Dynamic>` (o `IDiffEditorOptions` do porte é um objeto [`Dynamic`]).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::browser::dom::CodeWindow;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableStore, IDisposable, MutableDisposable};
use je_base::nls::localize;
use je_base::observable::deps::try_finally;
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{IObservable, ITransaction, Observable, SettableExt, autorun, derived, global_transaction, observable_value};
use je_base::event::Event;
use je_base::uri::URI;
use je_platform::actions::menu_ids;
use je_platform::contextkey::{IContextKey, IContextKeyService, IContextKeyServiceTarget, IScopedContextKeyService};

use crate::compressed_virtualized_scroll_view::MaxScroll;
use crate::diff_editor_options::spread_options;
use crate::editor_context_keys::multi_diff_editor_item_all_unchanged_regions_shown;
use crate::multi_diff_editor::diff_editor_surface::{CreateDiffEditor, DiffEditorItemWidgetOptions, IDiffEditorWidget, IDiffSideEditor, ScrollAnchorRunner};
use crate::multi_diff_editor::utils::action_runner_with_context;
use crate::multi_diff_editor::view_model::DocumentDiffItem;
use crate::multi_diff_editor::virtualized_item_manager::{
    IVirtualizedItemBinding, IVirtualizedItemBindingContext, IVirtualizedItemTemplate, VirtualizedItemBinding, VirtualizedItemTemplate,
};
use crate::multi_diff_editor::workbench_ui_element_factory::{
    IFileToolbarRequest, IResourceLabel, IResourceLabelOptions, IWorkbenchUIElementFactory, MultiDiffEditorItemLabelKind,
};
use crate::multi_diff_editor_options::{HorizontalInsets, IMultiDiffEditorVariantConfiguration};
use crate::offset_range::OffsetRange;
use crate::view_model_event_dispatcher::ContentSizeChangedEvent;

/// `binaryFilePlaceholderContentHeight`.
pub const BINARY_FILE_PLACEHOLDER_CONTENT_HEIGHT: i32 = 100;

/// O `ThemeIcon` `Codicon.chevronRight` do botão de recolher (item recolhido).
const ICON_CHEVRON_RIGHT: &str = "chevron-right";

/// O `ThemeIcon` `Codicon.chevronDown` do botão de recolher (item expandido).
const ICON_CHEVRON_DOWN: &str = "chevron-down";

/// O deslocamento que esconde o elemento raiz (`top: -100000px`).
const HIDDEN_TOP: i32 = -100_000;

/// Onde um evento do cabeçalho caiu: o que `target instanceof Element` e `target.closest(...)` respondem no original.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderEventTarget {
    /// O alvo não é um `Element`.
    NotElement,
    /// Dentro de `.actions`.
    Actions,
    /// Dentro de `.collapse-button`.
    CollapseButton,
    /// Qualquer outro elemento do cabeçalho.
    Other,
}

/// O elemento que o dono do contêiner deve focar (`focus_request`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusTarget {
    /// `binaryFilePlaceholder.focus()`.
    BinaryFilePlaceholder,
    /// `openBinaryDiffButton.focus()`.
    OpenBinaryDiffButton,
}

/// O `div.status` do cabeçalho: o texto (`R`, `D`, `A` ou vazio) e as classes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffEditorItemStatus {
    pub flag: String,
    pub renamed: bool,
    pub deleted: bool,
    pub added: bool,
}

/// O `div.header`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffEditorItemHeaderState {
    /// `transform: translateY(...)`, o deslocamento do cabeçalho grudado (sticky).
    pub translate_y: i32,
    /// A classe `shadow`.
    pub shadow: bool,
    /// A classe `collapsed`.
    pub collapsed: bool,
    /// `tabIndex` (0 quando o cabeçalho inteiro recolhe).
    pub tab_index: Option<i32>,
    /// `role="button"`.
    pub role_button: bool,
    /// `aria-expanded`.
    pub aria_expanded: Option<bool>,
    /// O ícone do botão de recolher.
    pub collapse_button_icon: &'static str,
}

/// O `div.binary-file-placeholder`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinaryFilePlaceholderState {
    /// `display: grid` (visível) ou `none`.
    pub visible: bool,
    pub tab_index: i32,
    /// A visibilidade de `binaryFilePlaceholderActions`.
    pub actions_visible: bool,
    /// O texto (e o `aria-label`) do marcador.
    pub label: String,
    /// O rótulo do botão `Open Diff`, quando ele existe.
    pub open_diff_label: Option<String>,
}

/// O que o pintor lê do elemento raiz do template (`div.multiDiffEntry`) e dos filhos.
#[derive(Clone, Debug, PartialEq)]
pub struct DiffEditorItemViewState {
    /// `visibility`.
    pub visible: bool,
    pub top: i32,
    pub height: i32,
    pub width: f64,
    pub active: bool,
    pub first_diff_entry: bool,
    pub header_hovered: bool,
    pub header_focused: bool,
    pub header: DiffEditorItemHeaderState,
    pub status: DiffEditorItemStatus,
    /// `display` do contêiner do editor de diff.
    pub editor_visible: bool,
    pub binary_file_placeholder: BinaryFilePlaceholderState,
    pub focus_request: Option<FocusTarget>,
}

/// O alvo escopado das ações: um nó nativo só com atributos e sem pai (o `div.actions` do original).
struct ActionsNodeTarget {
    attributes: RefCell<HashMap<String, String>>,
}

impl IContextKeyServiceTarget for ActionsNodeTarget {
    fn parent_element(&self) -> Option<Rc<dyn IContextKeyServiceTarget>> {
        None
    }

    fn set_attribute(&self, attr: &str, value: &str) {
        self.attributes.borrow_mut().insert(attr.to_string(), value.to_string());
    }

    fn remove_attribute(&self, attr: &str) {
        self.attributes.borrow_mut().remove(attr);
    }

    fn has_attribute(&self, attr: &str) -> bool {
        self.attributes.borrow().contains_key(attr)
    }

    fn get_attribute(&self, attr: &str) -> Option<String> {
        self.attributes.borrow().get(attr).cloned()
    }
}

/// Qual lado do editor de diff um acesso lê.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorSide {
    Original,
    Modified,
}

/// `value` quando é um booleano (o resultado de `x ?? y` sobre um campo booleano de opções).
fn as_bool(value: &Dynamic) -> Option<bool> {
    match value {
        Dynamic::Bool(flag) => Some(*flag),
        _ => None,
    }
}

fn throw_bug(message: &str) -> ! {
    errors::throw(Error::bug_indicating(Some(message)))
}

/// `DiffEditorItemTemplate`.
pub struct DiffEditorItemTemplate {
    /// `VirtualizedItemTemplate`: a ligação atual e o `_register`.
    base: VirtualizedItemTemplate<DocumentDiffItem, DiffEditorItemBinding>,
    me: Weak<Self>,
    window: Rc<CodeWindow>,
    factory: IWorkbenchUIElementFactory,
    variant_configuration: IMultiDiffEditorVariantConfiguration,
    options_override: Option<IObservable<Rc<Dynamic>>>,

    view_model: Rc<ObservableValue<Option<DocumentDiffItem>, ()>>,
    collapsed: IObservable<bool>,
    editor_content_height: Rc<ObservableValue<f64, ()>>,
    pub size: IObservable<i32>,
    modified_content_width: Rc<ObservableValue<f64, ()>>,
    modified_width: Rc<ObservableValue<f64, ()>>,
    original_content_width: Rc<ObservableValue<f64, ()>>,
    original_width: Rc<ObservableValue<f64, ()>>,
    item_horizontal_insets: HorizontalInsets,
    pub max_scroll: IObservable<MaxScroll>,

    pub editor: Rc<dyn IDiffEditorWidget>,
    is_binary_file_placeholder_focused: Rc<ObservableValue<bool, ()>>,
    pub is_focused: IObservable<bool>,

    resource_label: Option<Rc<dyn IResourceLabel>>,
    resource_label2: Option<Rc<dyn IResourceLabel>>,
    has_open_binary_diff_button: bool,
    vertical_state_update: Rc<MutableDisposable<dyn IDisposable>>,
    observed_editor_content_height: Cell<f64>,
    is_setting_data: Cell<bool>,
    binding_context: RefCell<Option<IVirtualizedItemBindingContext>>,

    header_height: i32,
    outer_editor_height: i32,
    context_key_service: Rc<dyn IScopedContextKeyService>,
    data_store: Rc<DisposableStore>,
    view: RefCell<DiffEditorItemViewState>,
}

impl DiffEditorItemTemplate {
    /// `new DiffEditorItemTemplate(container, overflowWidgetsDomNode, workbenchUIElementFactory, variantConfiguration,
    /// optionsOverride, instantiationService, parentContextKeyService)`. O contêiner e o `overflowWidgetsDomNode` não
    /// existem (ver a nota do módulo); a janela, a criação do editor de diff e o serviço de chaves de contexto vêm de quem
    /// monta o editor.
    pub fn new(
        window: Rc<CodeWindow>,
        workbench_ui_element_factory: IWorkbenchUIElementFactory,
        variant_configuration: IMultiDiffEditorVariantConfiguration,
        options_override: Option<IObservable<Rc<Dynamic>>>,
        create_diff_editor: &CreateDiffEditor,
        parent_context_key_service: &dyn IContextKeyService,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|me: &Weak<Self>| {
            let base: VirtualizedItemTemplate<DocumentDiffItem, DiffEditorItemBinding> = VirtualizedItemTemplate::new();
            let store = base.store().clone();

            let view_model = observable_value::<Option<DocumentDiffItem>, ()>("viewModel", None);
            let collapsed: IObservable<bool> = {
                let view_model = view_model.clone();
                derived(move |reader| view_model.read(reader).is_some_and(|item| item.collapsed().read(reader)))
            };
            let editor_content_height = observable_value::<f64, ()>("editorContentHeight", 500.0);
            let item_horizontal_insets = variant_configuration.horizontal_insets;
            let header_height = variant_configuration.header_height;
            let outer_editor_height = header_height + variant_configuration.content_bottom_padding;
            let size: IObservable<i32> = {
                let (collapsed, editor_content_height) = (collapsed.clone(), editor_content_height.clone());
                derived(move |reader| {
                    if collapsed.read(reader) {
                        return header_height;
                    }
                    (editor_content_height.read(reader) + f64::from(outer_editor_height)).round() as i32
                })
            };
            let modified_content_width = observable_value::<f64, ()>("modifiedContentWidth", 0.0);
            let modified_width = observable_value::<f64, ()>("modifiedWidth", 0.0);
            let original_content_width = observable_value::<f64, ()>("originalContentWidth", 0.0);
            let original_width = observable_value::<f64, ()>("originalWidth", 0.0);
            let max_scroll: IObservable<MaxScroll> = {
                let (modified_content_width, modified_width) = (modified_content_width.clone(), modified_width.clone());
                let (original_content_width, original_width) = (original_content_width.clone(), original_width.clone());
                derived(move |reader| {
                    let scroll1 = modified_content_width.read(reader) - modified_width.read(reader);
                    let scroll2 = original_content_width.read(reader) - original_width.read(reader);
                    MaxScroll { max_scroll: if scroll1 > scroll2 { scroll1 } else { scroll2 } }
                })
            };

            let editor = create_diff_editor(DiffEditorItemWidgetOptions {
                variant: variant_configuration.diff_editor_variant,
                fixed_overflow_widgets: true,
                run_with_original_editor_scroll_anchor: Self::scroll_anchor_runner(me, EditorSide::Original),
                run_with_modified_editor_scroll_anchor: Self::scroll_anchor_runner(me, EditorSide::Modified),
            });
            store.add(editor.clone());

            let is_binary_file_placeholder_focused = observable_value::<bool, ()>("isBinaryFilePlaceholderFocused", false);
            let is_focused: IObservable<bool> = {
                let is_modified_focused = editor.get_modified_editor().is_focused();
                let is_original_focused = editor.get_original_editor().is_focused();
                let is_binary_file_placeholder_focused = is_binary_file_placeholder_focused.clone();
                derived(move |reader| {
                    is_modified_focused.read(reader) || is_original_focused.read(reader) || is_binary_file_placeholder_focused.read(reader)
                })
            };

            let resource_label = Self::create_resource_label(&workbench_ui_element_factory, MultiDiffEditorItemLabelKind::Primary, &store);
            let resource_label2 = Self::create_resource_label(&workbench_ui_element_factory, MultiDiffEditorItemLabelKind::Secondary, &store);

            let vertical_state_update = MutableDisposable::<dyn IDisposable>::new();
            store.add(vertical_state_update.clone());
            let data_store = DisposableStore::new();
            store.add(data_store.clone());

            let context_key_service = parent_context_key_service.create_scoped(Rc::new(ActionsNodeTarget { attributes: RefCell::new(HashMap::new()) }));
            store.add(context_key_service.clone());

            let has_open_binary_diff_button = workbench_ui_element_factory.open_diff_editor.is_some();
            let header_click_to_collapse = variant_configuration.header_click_to_collapse;
            let view = DiffEditorItemViewState {
                visible: true,
                top: 0,
                height: 0,
                width: 0.0,
                active: false,
                first_diff_entry: false,
                header_hovered: false,
                header_focused: false,
                header: DiffEditorItemHeaderState {
                    translate_y: 0,
                    shadow: false,
                    collapsed: false,
                    tab_index: header_click_to_collapse.then_some(0),
                    role_button: header_click_to_collapse,
                    aria_expanded: None,
                    collapse_button_icon: ICON_CHEVRON_DOWN,
                },
                status: DiffEditorItemStatus { flag: "R".to_string(), renamed: false, deleted: true, added: false },
                editor_visible: true,
                binary_file_placeholder: BinaryFilePlaceholderState {
                    visible: false,
                    tab_index: 0,
                    actions_visible: true,
                    label: localize("binaryFileChanged", "Binary file changed", &[]),
                    open_diff_label: has_open_binary_diff_button.then(|| localize("openBinaryDiff", "Open Diff", &[])),
                },
                focus_request: None,
            };

            Self {
                base,
                me: me.clone(),
                window,
                factory: workbench_ui_element_factory,
                variant_configuration,
                options_override,
                view_model,
                collapsed,
                editor_content_height,
                size,
                modified_content_width,
                modified_width,
                original_content_width,
                original_width,
                item_horizontal_insets,
                max_scroll,
                editor,
                is_binary_file_placeholder_focused,
                is_focused,
                resource_label,
                resource_label2,
                has_open_binary_diff_button,
                vertical_state_update,
                observed_editor_content_height: Cell::new(500.0),
                is_setting_data: Cell::new(false),
                binding_context: RefCell::new(None),
                header_height,
                outer_editor_height,
                context_key_service,
                data_store,
                view: RefCell::new(view),
            }
        });
        this.wire();
        this
    }

    /// `workbenchUIElementFactory.createResourceLabel?.(...)`, registrado no `store` do template.
    fn create_resource_label(factory: &IWorkbenchUIElementFactory, kind: MultiDiffEditorItemLabelKind, store: &Rc<DisposableStore>) -> Option<Rc<dyn IResourceLabel>> {
        let label = factory.create_resource_label.as_ref().map(|create| create(kind))?;
        store.add(label.clone());
        Some(label)
    }

    /// `runWithOriginalEditorScrollAnchor` e `runWithModifiedEditorScrollAnchor`: o deslocamento do âncora é a altura
    /// externa mais o topo da linha do lado dado.
    fn scroll_anchor_runner(me: &Weak<Self>, side: EditorSide) -> ScrollAnchorRunner {
        let me = me.clone();
        Rc::new(move |anchor_line_number: i32, update: &dyn Fn()| {
            let Some(this) = me.upgrade() else { return };
            let offset_owner = me.clone();
            this.run_with_editor_scroll_anchor(
                Rc::new(move || offset_owner.upgrade().map_or(0, |this| (f64::from(this.outer_editor_height) + this.get_editor_line_top(side, anchor_line_number)).round() as i32)),
                update,
            );
        })
    }

    /// Os ouvintes, as reações e a barra de ações do construtor, registrados depois que o `Rc` do template existe.
    fn wire(self: &Rc<Self>) {
        let store = self.base.store().clone();
        let weak = Rc::downgrade(self);

        // O ícone do botão de recolher.
        {
            let (this, collapsed) = (weak.clone(), self.collapsed.clone());
            store.add(autorun(move |reader| {
                let collapsed = collapsed.read(reader);
                if let Some(this) = this.upgrade() {
                    this.update_view(|view| view.header.collapse_button_icon = if collapsed { ICON_CHEVRON_RIGHT } else { ICON_CHEVRON_DOWN });
                }
            }));
        }

        // A visibilidade do editor, do marcador binário e o `aria-expanded`.
        {
            let (this, collapsed, view_model) = (weak.clone(), self.collapsed.clone(), self.view_model.clone());
            store.add(autorun(move |reader| {
                let collapsed = collapsed.read(reader);
                let item = view_model.read(reader);
                let Some(this) = this.upgrade() else { return };
                let is_binary = item.as_ref().is_some_and(|item| item.is_binary());
                let can_open_diff = item.as_ref().is_some_and(|item| item.original_uri().is_some() && item.modified_uri().is_some()) && this.has_open_binary_diff_button;
                let header_click_to_collapse = this.variant_configuration.header_click_to_collapse;
                this.update_view(|view| {
                    view.editor_visible = !(collapsed || is_binary);
                    view.binary_file_placeholder.visible = !collapsed && is_binary;
                    view.binary_file_placeholder.tab_index = if can_open_diff { -1 } else { 0 };
                    view.binary_file_placeholder.actions_visible = can_open_diff;
                    if header_click_to_collapse {
                        view.header.aria_expanded = Some(!collapsed);
                    }
                });
            }));
        }

        // A largura do conteúdo de cada lado.
        for side in [EditorSide::Modified, EditorSide::Original] {
            let (this, editor) = (weak.clone(), self.side_editor(side));
            store.add(editor.on_did_layout_change().listen(move |_| {
                if let Some(this) = this.upgrade() {
                    let width = this.side_editor(side).get_layout_info().content_width;
                    match side {
                        EditorSide::Modified => this.modified_width.set(width, None),
                        EditorSide::Original => this.original_width.set(width, None),
                    }
                }
            }));
        }

        let this = weak.clone();
        store.add(self.editor.on_did_content_size_change().listen(move |event| {
            if let Some(this) = this.upgrade() {
                this.on_content_size_changed(event);
            }
        }));

        // `active` e `first-diff-entry`.
        {
            let (this, view_model) = (weak.clone(), self.view_model.clone());
            store.add(autorun(move |reader| {
                let item = view_model.read(reader);
                let is_active = item.as_ref().is_some_and(|item| item.is_active().read(reader));
                let is_first = item.as_ref().is_some_and(|item| item.is_first().read(reader));
                if let Some(this) = this.upgrade() {
                    this.update_view(|view| {
                        view.active = is_active;
                        view.first_diff_entry = is_first;
                    });
                }
            }));
        }

        // A chave de contexto `multiDiffEditorItemAllUnchangedRegionsShown`.
        {
            let all_unchanged_regions_shown_key = multi_diff_editor_item_all_unchanged_regions_shown().bind_to(&*self.context_key_service);
            let all_unchanged_regions_shown = self.editor.all_unchanged_regions_shown();
            store.add(autorun(move |reader| all_unchanged_regions_shown_key.set(Dynamic::Bool(all_unchanged_regions_shown.read(reader)))));
        }

        // A barra de ações do arquivo.
        if let Some(create_file_toolbar) = self.factory.create_file_toolbar.clone() {
            let this = weak;
            let action_runner = action_runner_with_context(move || this.upgrade().map_or(Dynamic::Undefined, |this| this.toolbar_context()));
            store.add(action_runner.clone());
            store.add(create_file_toolbar(IFileToolbarRequest {
                menu_id: menu_ids::multi_diff_editor_file_toolbar(),
                context_key_service: self.context_key_service.clone(),
                action_runner,
                highlight_toggled_items: true,
                should_forward_args: true,
                primary_group_prefix: "navigation",
            }));
        }
    }

    /// O contexto das ações da barra: `modifiedUri ?? originalUri`.
    fn toolbar_context(&self) -> Dynamic {
        let resource = self.view_model.get().and_then(|item| item.modified_uri().or_else(|| item.original_uri()));
        resource.map_or(Dynamic::Undefined, |uri: URI| Dynamic::Object(ObjectRef::with_host(Rc::new(uri))))
    }

    /// Muda o estado que o pintor lê.
    fn update_view(&self, update: impl FnOnce(&mut DiffEditorItemViewState)) {
        update(&mut self.view.borrow_mut());
    }

    /// O estado atual do elemento raiz e dos filhos, para o pintor.
    pub fn view_state(&self) -> DiffEditorItemViewState {
        self.view.borrow().clone()
    }

    /// O elemento que o dono do contêiner deve focar, se algum foi pedido (o pedido é consumido).
    pub fn take_focus_request(&self) -> Option<FocusTarget> {
        self.view.borrow_mut().focus_request.take()
    }

    fn side_editor(&self, side: EditorSide) -> Rc<dyn IDiffSideEditor> {
        match side {
            EditorSide::Original => self.editor.get_original_editor(),
            EditorSide::Modified => self.editor.get_modified_editor(),
        }
    }

    /// `_getOriginalEditorLineTop` e `_getModifiedEditorLineTop`.
    fn get_editor_line_top(&self, side: EditorSide, line_number: i32) -> f64 {
        let editor = self.side_editor(side);
        if line_number > editor.get_line_count() { editor.get_content_height() } else { editor.get_top_for_line_number(line_number) }
    }

    /// `this.editor.onDidContentSizeChange(e => ...)`.
    fn on_content_size_changed(&self, event: &ContentSizeChangedEvent) {
        global_transaction(|tx| {
            self.modified_content_width.set(self.editor.get_modified_editor().get_content_width(), Some(tx));
            self.original_content_width.set(self.editor.get_original_editor().get_content_width(), Some(tx));
        });
        let Some(item) = self.view_model.get() else { return };
        if self.is_setting_data.get() || item.is_binary() || !item.diff_editor_view_model().is_diff_up_to_date.get() {
            return;
        }
        self.observed_editor_content_height.set(event.content_height);
        self.schedule_vertical_state_update();
    }

    /// `setScrollLeft(left)`.
    pub fn set_scroll_left(&self, left: f64) {
        let modified_scroll = self.modified_content_width.get() - self.modified_width.get();
        let original_scroll = self.original_content_width.get() - self.original_width.get();
        if modified_scroll > original_scroll {
            self.editor.get_modified_editor().set_scroll_left(left);
        } else {
            self.editor.get_original_editor().set_scroll_left(left);
        }
    }

    /// `getExpandedContentHeight()`.
    pub fn get_expanded_content_height(&self) -> f64 {
        self.observed_editor_content_height.get() + f64::from(self.outer_editor_height)
    }

    /// `createBinding(item, context)`.
    fn create_binding(&self, item: DocumentDiffItem, context: IVirtualizedItemBindingContext) -> Rc<DiffEditorItemBinding> {
        let Some(this) = self.me.upgrade() else { throw_bug("The diff editor item template was disposed") };
        let initial_size = context.initial_size;
        *self.binding_context.borrow_mut() = Some(context);
        if let Err(error) = errors::catch(|| self.set_item(Some(item.clone()), initial_size)) {
            let cleanup = errors::catch(|| self.set_item(None, 0));
            *self.binding_context.borrow_mut() = None;
            errors::throw(cleanup.err().unwrap_or(error));
        }
        DiffEditorItemBinding::new(item, &this)
    }

    /// `_runWithEditorScrollAnchor(getItemOffset, update)`.
    fn run_with_editor_scroll_anchor(&self, get_item_offset: Rc<dyn Fn() -> i32>, update: &dyn Fn()) {
        let Some(context) = self.binding_context.borrow().clone() else {
            throw_bug("Cannot preserve a diff editor scroll anchor without an active item binding")
        };
        context.run_with_scroll_anchor(get_item_offset, &|tx: &dyn ITransaction| {
            update();
            self.vertical_state_update.clear();
            self.observed_editor_content_height.set(self.editor.get_content_height());
            self.editor_content_height.set(self.observed_editor_content_height.get(), Some(tx));
        });
    }

    /// `updateOptions(options)` de `setItem`: as opções do item sobre as da variante e do `optionsOverride`, com as opções
    /// fixas do editor embutido por cima.
    fn compute_options(&self, options: &Dynamic) -> Dynamic {
        let variant_options = self.variant_configuration.diff_editor_options;
        let override_options = self.options_override.as_ref().map(|options_override| (*options_override.get()).clone());
        let hide_original_line_numbers = override_options
            .as_ref()
            .and_then(|override_options| as_bool(&override_options.optional_get_str("hideOriginalLineNumbers")))
            .or_else(|| as_bool(&options.optional_get_str("hideOriginalLineNumbers")))
            .or(variant_options.and_then(|variant_options| variant_options.hide_original_line_numbers))
            .unwrap_or(false);

        let variant_object = Dynamic::object(variant_options.and_then(|variant_options| variant_options.hide_original_line_numbers).map(|hide| ("hideOriginalLineNumbers", Dynamic::Bool(hide))));
        let mut merged = spread_options(&variant_object, options);
        if let Some(override_options) = &override_options {
            merged = spread_options(&merged, override_options);
        }
        spread_options(
            &merged,
            &Dynamic::object([
                ("hideOriginalLineNumbers", Dynamic::Bool(hide_original_line_numbers)),
                ("scrollBeyondLastLine", Dynamic::Bool(false)),
                ("hideUnchangedRegions", Dynamic::object([("enabled", Dynamic::Bool(true))])),
                (
                    "scrollbar",
                    Dynamic::object([
                        ("vertical", Dynamic::string("hidden")),
                        ("horizontal", Dynamic::string("hidden")),
                        ("handleMouseWheel", Dynamic::Bool(false)),
                        ("useShadows", Dynamic::Bool(false)),
                    ]),
                ),
                ("renderOverviewRuler", Dynamic::Bool(false)),
                ("fixedOverflowWidgets", Dynamic::Bool(true)),
                ("overviewRulerBorder", Dynamic::Bool(false)),
            ]),
        )
    }

    /// `this.documentDiffItem.options ?? {}`, já mesclada por [`Self::compute_options`].
    fn item_options(&self, item: &DocumentDiffItem) -> Dynamic {
        let options = item.document_diff_item().options().unwrap_or_else(|| Dynamic::object(Vec::<(&str, Dynamic)>::new()));
        self.compute_options(&options)
    }

    /// `setItem(item, initialSize = 0)`.
    fn set_item(&self, item: Option<DocumentDiffItem>, initial_size: i32) {
        self.vertical_state_update.clear();

        let Some(item) = item else {
            self.is_setting_data.set(true);
            try_finally(
                || {
                    global_transaction(|tx| {
                        self.view_model.set(None, Some(tx));
                        self.editor.set_diff_model(None, Some(tx));
                        self.data_store.clear();
                    });
                },
                || self.is_setting_data.set(false),
            );
            return;
        };

        let value = item.document_diff_item().clone();
        let editor_content_height = if item.is_binary() {
            f64::from(BINARY_FILE_PLACEHOLDER_CONTENT_HEIGHT)
        } else {
            (f64::from(initial_size).max(item.expanded_content_height()) - f64::from(self.outer_editor_height)).max(0.0)
        };
        self.observed_editor_content_height.set(editor_content_height);
        self.is_setting_data.set(true);
        try_finally(
            || {
                global_transaction(|tx| {
                    self.editor_content_height.set(editor_content_height, Some(tx));
                    let original_uri = item.original_uri();
                    let modified_uri = item.modified_uri();
                    if let Some(resource_label) = &self.resource_label {
                        resource_label.set_uri(modified_uri.clone().or_else(|| original_uri.clone()), Some(IResourceLabelOptions { strikethrough: Some(modified_uri.is_none()) }));
                    }

                    let (is_renamed, is_deleted, is_added, flag) = match (&modified_uri, &original_uri) {
                        (Some(modified), Some(original)) if modified.path() != original.path() => (true, false, false, "R"),
                        (None, _) => (false, true, false, "D"),
                        (Some(_), None) => (false, false, true, "A"),
                        _ => (false, false, false, ""),
                    };
                    self.update_view(|view| view.status = DiffEditorItemStatus { flag: flag.to_string(), renamed: is_renamed, deleted: is_deleted, added: is_added });

                    if let Some(resource_label2) = &self.resource_label2 {
                        resource_label2.set_uri(if is_renamed { original_uri.clone() } else { None }, Some(IResourceLabelOptions { strikethrough: Some(true) }));
                    }

                    self.data_store.clear();
                    self.view_model.set(Some(item.clone()), Some(tx));
                    self.editor.update_options(&self.item_options(&item));
                    self.editor.set_diff_model(Some(item.diff_editor_view_model_ref().clone()), Some(tx));
                });
            },
            || self.is_setting_data.set(false),
        );

        let weak = self.me.clone();
        self.data_store.add(autorun({
            let (weak, item) = (weak.clone(), item.clone());
            move |reader| {
                if item.is_binary() {
                    return;
                }
                let view_model = item.diff_editor_view_model();
                if !view_model.is_diff_up_to_date.read(reader) {
                    return;
                }
                let has_changes = view_model.diff.read(reader).map_or(0, |diff| diff.mappings.len()) > 0;
                if has_changes && !view_model.unchanged_regions.read(reader).is_empty() {
                    return;
                }
                if let Some(this) = weak.upgrade() {
                    this.observed_editor_content_height.set(this.editor.get_content_height());
                    this.schedule_vertical_state_update();
                }
            }
        }));
        if let Some(on_options_did_change) = value.on_options_did_change() {
            let (weak, item) = (weak.clone(), item.clone());
            self.data_store.add(on_options_did_change.listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.editor.update_options(&this.item_options(&item));
                }
            }));
        }
        if let Some(options_override) = self.options_override.clone() {
            let (weak, item) = (weak, item.clone());
            self.data_store.add(autorun(move |reader| {
                options_override.read(reader);
                if let Some(this) = weak.upgrade() {
                    this.editor.update_options(&this.item_options(&item));
                }
            }));
        }
        if let Some(context_keys) = value.context_keys() {
            for (key, context_key_value) in context_keys {
                self.context_key_service.create_key(&key, context_key_value);
            }
        }
    }

    /// `render(verticalRange, width, editorScroll, viewPort)`.
    pub fn render(&self, vertical_range: OffsetRange, width: f64, editor_scroll: i32, view_port: OffsetRange) {
        // For sticky scroll
        let max_delta = vertical_range.length() - self.header_height;
        let delta = (view_port.start - vertical_range.start).min(max_delta).max(0);
        self.update_view(|view| {
            view.visible = true;
            view.top = vertical_range.start;
            view.height = vertical_range.length();
            view.width = width;
            view.header.translate_y = delta;
        });

        global_transaction(|_tx| {
            self.editor.layout(
                width - f64::from(self.item_horizontal_insets.left) - f64::from(self.item_horizontal_insets.right),
                f64::from(vertical_range.length() - self.outer_editor_height),
            );
        });
        self.editor.get_original_editor().set_scroll_top(f64::from(editor_scroll));
        self.flush_vertical_state();

        self.update_view(|view| {
            view.header.shadow = delta > 0 || editor_scroll > 0;
            view.header.collapsed = delta == max_delta;
        });
    }

    fn schedule_vertical_state_update(&self) {
        if self.vertical_state_update.value().is_some() {
            return;
        }
        let weak = self.me.clone();
        let scheduled = self.window.schedule_at_next_animation_frame(
            move || {
                if let Some(this) = weak.upgrade() {
                    this.flush_vertical_state();
                }
            },
            0,
        );
        self.vertical_state_update.set(Some(scheduled));
    }

    fn flush_vertical_state(&self) {
        self.vertical_state_update.clear();
        global_transaction(|tx| self.editor_content_height.set(self.observed_editor_content_height.get(), Some(tx)));
    }

    /// `hide()`.
    pub fn hide(&self) {
        self.update_view(|view| {
            view.header_hovered = false;
            view.top = HIDDEN_TOP;
            // Some editor parts are still visible
            view.visible = false;
        });
    }

    /// `focusBinaryFilePlaceholder()`.
    pub fn focus_binary_file_placeholder(&self) {
        let can_open_diff = self.view_model.get().is_some_and(|item| item.original_uri().is_some() && item.modified_uri().is_some()) && self.has_open_binary_diff_button;
        self.update_view(|view| view.focus_request = Some(if can_open_diff { FocusTarget::OpenBinaryDiffButton } else { FocusTarget::BinaryFilePlaceholder }));
    }

    /// `unbind(item)`.
    pub fn unbind(&self, item: &DocumentDiffItem) {
        if !self.view_model.get().is_some_and(|current| Rc::ptr_eq(&current, item)) {
            throw_bug("Cannot unbind a diff editor template from a different item");
        }
        self.set_item(None, 0);
        *self.binding_context.borrow_mut() = None;
    }

    /// `activateItem`: `this._viewModel.get()?.setActive(undefined)`.
    fn activate_item(&self) {
        if let Some(item) = self.view_model.get() {
            item.set_active(None);
        }
    }

    /// `this._viewModel.get()?.collapsed.set(!this._collapsed.get(), undefined)`.
    fn toggle_collapsed(&self) {
        if let Some(item) = self.view_model.get() {
            item.collapsed().set(!self.collapsed.get(), None);
        }
    }

    /// O clique do botão de recolher.
    pub fn on_collapse_button_click(&self) {
        self.activate_item();
        self.toggle_collapsed();
    }

    /// `AUXCLICK` do cabeçalho: devolve se o clique foi tratado (o `EventHelper.stop` do original).
    pub fn on_header_aux_click(&self, button: i32) -> bool {
        let Some(handle_header_middle_click) = &self.factory.handle_header_middle_click else { return false };
        if button != 1 {
            return false;
        }
        let resource = self.view_model.get().and_then(|item| item.modified_uri().or_else(|| item.original_uri()));
        resource.is_some_and(|resource| handle_header_middle_click(&resource))
    }

    /// `MOUSE_ENTER` do cabeçalho.
    pub fn on_header_mouse_enter(&self) {
        if self.variant_configuration.header_click_to_collapse {
            self.update_view(|view| view.header_hovered = true);
        }
    }

    /// `MOUSE_LEAVE` do cabeçalho.
    pub fn on_header_mouse_leave(&self) {
        if self.variant_configuration.header_click_to_collapse {
            self.update_view(|view| view.header_hovered = false);
        }
    }

    /// `onDidFocus` do cabeçalho.
    pub fn on_header_focus(&self) {
        if self.variant_configuration.header_click_to_collapse {
            self.update_view(|view| view.header_focused = true);
            self.activate_item();
        }
    }

    /// `onDidBlur` do cabeçalho.
    pub fn on_header_blur(&self) {
        if self.variant_configuration.header_click_to_collapse {
            self.update_view(|view| view.header_focused = false);
        }
    }

    /// `CLICK` do cabeçalho: não alterna se o clique foi nas ações ou no botão de recolher (que já trata o seu clique).
    pub fn on_header_click(&self, target: HeaderEventTarget) {
        if !self.variant_configuration.header_click_to_collapse {
            return;
        }
        self.activate_item();
        if target != HeaderEventTarget::Other {
            return;
        }
        self.toggle_collapsed();
    }

    /// `KEY_DOWN` do cabeçalho: devolve se o evento deve ter o comportamento padrão impedido (`preventDefault`).
    pub fn on_header_key_down(&self, key: &str, target: HeaderEventTarget) -> bool {
        if !self.variant_configuration.header_click_to_collapse || (key != "Enter" && key != " ") {
            return false;
        }
        self.activate_item();
        if matches!(target, HeaderEventTarget::Actions | HeaderEventTarget::CollapseButton) {
            return false;
        }
        self.toggle_collapsed();
        true
    }

    /// `onDidFocus` do marcador de arquivo binário.
    pub fn on_binary_file_placeholder_focus(&self) {
        self.is_binary_file_placeholder_focused.set(true, None);
    }

    /// `onDidBlur` do marcador de arquivo binário.
    pub fn on_binary_file_placeholder_blur(&self) {
        self.is_binary_file_placeholder_focused.set(false, None);
    }

    /// `onDidClick` do botão `Open Diff`.
    pub fn on_open_binary_diff_button_click(&self) {
        let Some(open_diff_editor) = &self.factory.open_diff_editor else { return };
        if let Some(item) = self.view_model.get()
            && let (Some(original), Some(modified)) = (item.original_uri(), item.modified_uri())
        {
            open_diff_editor(&original, &modified);
        }
    }
}

impl IVirtualizedItemTemplate<DocumentDiffItem, DiffEditorItemBinding> for DiffEditorItemTemplate {
    fn current_binding(&self) -> IObservable<Option<Rc<DiffEditorItemBinding>>> {
        self.base.current_binding()
    }

    fn bind(&self, item: DocumentDiffItem, context: IVirtualizedItemBindingContext) -> Rc<DiffEditorItemBinding> {
        let Some(this) = self.me.upgrade() else { throw_bug("The diff editor item template was disposed") };
        self.base.bind(item, context, move |item, context| this.create_binding(item, context))
    }
}

impl IDisposable for DiffEditorItemTemplate {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `DiffEditorItemBinding`.
pub struct DiffEditorItemBinding {
    base: VirtualizedItemBinding<DocumentDiffItem>,
    size: IObservable<i32>,
    max_scroll: IObservable<MaxScroll>,
    should_keep_alive: IObservable<bool>,
    pub editor: Rc<dyn IDiffEditorWidget>,
    template: Weak<DiffEditorItemTemplate>,
}

impl DiffEditorItemBinding {
    pub fn new(item: DocumentDiffItem, template: &Rc<DiffEditorItemTemplate>) -> Rc<Self> {
        Rc::new(Self {
            base: VirtualizedItemBinding::new(item),
            size: template.size.clone(),
            max_scroll: template.max_scroll.clone(),
            should_keep_alive: template.is_focused.clone(),
            editor: template.editor.clone(),
            template: Rc::downgrade(template),
        })
    }

    fn template(&self) -> Rc<DiffEditorItemTemplate> {
        self.template.upgrade().unwrap_or_else(|| throw_bug("The diff editor item template was disposed"))
    }

    /// `setScrollLeft(scrollLeft)`.
    pub fn set_scroll_left(&self, scroll_left: f64) {
        self.template().set_scroll_left(scroll_left);
    }

    /// `getExpandedContentHeight()`.
    pub fn get_expanded_content_height(&self) -> f64 {
        self.template().get_expanded_content_height()
    }

    /// `focus()`.
    pub fn focus(&self) {
        if self.base.item.is_binary() {
            self.template().focus_binary_file_placeholder();
        } else {
            self.editor.focus();
        }
    }
}

impl IVirtualizedItemBinding<DocumentDiffItem> for DiffEditorItemBinding {
    fn item(&self) -> &DocumentDiffItem {
        &self.base.item
    }

    fn size(&self) -> IObservable<i32> {
        self.size.clone()
    }

    fn max_scroll(&self) -> IObservable<MaxScroll> {
        self.max_scroll.clone()
    }

    fn should_keep_alive(&self) -> IObservable<bool> {
        self.should_keep_alive.clone()
    }

    fn on_did_dispose(&self) -> Event<()> {
        self.base.on_did_dispose()
    }

    fn render(&self, rendered_range: OffsetRange, scroll_offset: i32, width: f64, rendered_viewport: OffsetRange) {
        self.template().render(rendered_range, width, scroll_offset, rendered_viewport);
    }

    fn hide(&self) {
        self.template().hide();
    }
}

impl IDisposable for DiffEditorItemBinding {
    fn dispose(&self) {
        if self.base.base().store().is_disposed() {
            return;
        }
        if let Some(template) = self.template.upgrade() {
            template.unbind(&self.base.item);
        }
        self.base.dispose();
    }
}
