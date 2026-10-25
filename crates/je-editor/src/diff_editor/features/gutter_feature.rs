//! Porte de `src/vs/editor/browser/widget/diffEditor/features/gutterFeature.ts`: a calha entre os dois editores com a barra de ações de
//! cada diferença (`MenuId.DiffEditorHunkToolbar`) ou da seleção (`MenuId.DiffEditorSelectionToolbar`), que acompanha a linha do
//! editor modificado, e o sash que a calha arrasta junto.
//!
//! Divergências:
//! - **DOM vira dado.** O `div.gutter` é o [`DiffEditorGutterElement`] ([`DiffEditorGutter::element`]: `left` do `layout`, a largura
//!   fixa e o `display` do `applyStyle`), e o `div.gutterItem` de cada diferença é o [`DiffToolBarView`] ([`DiffToolBar::view`]: as
//!   classes `showAlways` e `noTransition`, a largura e a altura fixas e o `top` dos botões). O `prependRemoveOnDispose` é da dona
//!   da árvore.
//! - **Eventos de janela viram métodos.** O `click` da calha é [`DiffEditorGutter::on_click`]. O `wheel` (`handleMouseWheel` das opções
//!   de rolagem repassando o evento a `delegateScrollFromMouseWheelEvent`) fica com o dono da janela: o `CodeEditorWidget` do porte
//!   não tem a `View` que recebe o evento, então [`DiffEditorGutter::should_delegate_mouse_wheel`] responde se o repasse vale e quem
//!   tem a `View` do editor modificado faz o repasse.
//! - **A barra de ações.** O `MenuWorkbenchToolBar` e o `WorkbenchHoverDelegate` moram no `je-workbench`, que o `je-editor` não enxerga
//!   (o mesmo corte do `IWorkbenchUIElementFactory` do editor de vários arquivos). O `DiffToolBar` monta a requisição
//!   ([`DiffGutterToolbarRequest`]: o menu, o serviço de chaves de contexto, o máximo de itens, a orientação vertical, o hover à
//!   direita e o contexto de execução) e a [`DiffGutterToolbarFactory`] cria a barra e devolve o [`IDiffGutterToolbar`], de que o
//!   `DiffToolBar` lê `clientHeight` e o `onDidChangeMenuItems`. O `ActionRunnerWithContext` é a função
//!   [`DiffGutterToolbarRequest::context`], que devolve o [`DiffEditorSelectionHunkToolbarContext`] do momento da execução; quem cria a
//!   barra o converte para o `Dynamic` que o `ActionRunner` repassa.
//! - **`computeStagedValue`.** Método de `DiffEditorGutter` que a barra chama antes de o gutter existir por completo (a barra é criada
//!   dentro do construtor da calha): é a função [`compute_staged_value`], sobre os editores.
//! - **Itens por identidade.** O `DiffGutterItem` é um `Rc` (a igualdade estrita do observável é a identidade do objeto). A diferença
//!   atual (`_currentDiff`) é o índice dela em `diff.mappings` (o `===` de `lineRangeMapping` do original), e a lista vazia sem
//!   diferença é sempre uma lista nova, como o `[]` do original.
//! - **`selectedDiffs`** é a [`selected_diffs_observable`] do `RevertButtonsFeature`, o mesmo cálculo.
//! - **`layout` do `DiffToolBar`.** A primeira leitura de `clientHeight` do original é sobrescrita pela segunda sem uso no meio, então
//!   só a segunda existe (a barra é recriada quando `isSmall` muda, e a leitura depois do `set` é a da barra nova).
//! - **Menu.** `_menu` e `_actions` são o menu do `IMenuService`; `_hasActions` lê `getActions().length > 0` direto, que é o único uso
//!   de `_actions`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::observable::observables::derived::derived_owned;
use je_base::observable::observables::observable_from_event::observable_from_event_owned;
use je_base::observable::observables::observable_value::{ObservableValue, observable_value_owned};
use je_base::observable::{
    DebugOwner, IObservable, IReader, Observable, ObservableExt, SettableExt, autorun, autorun_with_store, derived_with_setter,
    recompute_initially_and_on_change,
};
use je_base::runtime::set_timeout;
use je_base::uri::URI;
use je_platform::actions::menu_ids::{diff_editor_hunk_toolbar, diff_editor_selection_toolbar};
use je_platform::actions::{I_MENU_SERVICE, MenuId};
use je_platform::contextkey::{I_CONTEXT_KEY_SERVICE, IContextKeyService};
use je_widgets::sash::BoundarySashes;

use crate::code_editor_widget::ICodeEditor;
use crate::config::editor_options::scrollbar::SCROLLBAR;
use crate::diff::range_mapping::DetailedLineRangeMapping;
use crate::diff_editor::components::diff_editor_editors::DiffEditorEditors;
use crate::diff_editor::components::diff_editor_sash::{DiffEditorSash, SashLayout};
use crate::diff_editor::feature_contracts::{DiffEditorFeatureContext, IDiffEditorGutter};
use crate::diff_editor::features::revert_buttons_feature::selected_diffs_observable;
use crate::diff_editor::utils::editor_gutter::{EditorGutter, IGutterItemInfo, IGutterItemProvider, IGutterItemView, PixelRange};
use crate::diff_editor_utils::derived_optional_disposable_of;
use crate::edits::text_edit::TextEdit;
use crate::inline_completions::utils::dispose_handle;
use crate::line_range::LineRange;
use crate::text_model_text::TextModelText;

/// `const width = 35`.
const GUTTER_WIDTH: f64 = 35.0;

/// O que a barra de ações da calha lê do contexto de execução: `DiffEditorSelectionHunkToolbarContext`.
#[derive(Clone)]
pub struct DiffEditorSelectionHunkToolbarContext {
    pub mapping: DetailedLineRangeMapping,
    /// The original text with the selected modified changes applied.
    pub original_with_modified_changes: String,
    pub modified_uri: URI,
    pub original_uri: URI,
}

/// Tudo o que o `createInstance(MenuWorkbenchToolBar, buttons, menuId, {...})` do `DiffToolBar` recebe (ver as divergências do módulo).
#[derive(Clone)]
pub struct DiffGutterToolbarRequest {
    pub menu_id: MenuId,
    /// O `IContextKeyService` do editor de diff.
    pub context_key_service: Rc<dyn IContextKeyService>,
    /// `orientation: ActionsOrientation.VERTICAL`.
    pub vertical: bool,
    /// `overflowBehavior: { maxItems }`: 1 quando a diferença é pequena, senão 3.
    pub max_items: usize,
    /// `toolbarOptions: { primaryGroup: g => g.startsWith('primary') }`.
    pub primary_group_prefix: &'static str,
    /// `hiddenItemStrategy: HiddenItemStrategy.Ignore`.
    pub ignore_hidden_items: bool,
    /// `menuOptions: { shouldForwardArgs: true }`.
    pub should_forward_args: bool,
    /// `new WorkbenchHoverDelegate('element', { instantHover: true }, ...)`.
    pub instant_hover: bool,
    /// `{ position: { hoverPosition: HoverPosition.RIGHT } }`.
    pub hover_position_right: bool,
    /// O contexto do `ActionRunnerWithContext`, calculado a cada execução.
    pub context: Rc<dyn Fn() -> DiffEditorSelectionHunkToolbarContext>,
}

/// A barra criada pela [`DiffGutterToolbarFactory`].
pub trait IDiffGutterToolbar: IDisposable {
    /// `buttons.clientHeight`: a altura que a barra ocupa agora.
    fn client_height(&self) -> f64;

    /// `onDidChangeMenuItems`.
    fn on_did_change_menu_items(&self) -> Event<()>;
}

/// Cria a barra de ações de uma diferença.
pub type DiffGutterToolbarFactory = Rc<dyn Fn(DiffGutterToolbarRequest) -> Rc<dyn IDiffGutterToolbar>>;

/// `computeStagedValue(mapping)`: o texto original com as mudanças internas da diferença aplicadas.
pub fn compute_staged_value(editors: &DiffEditorEditors, mapping: &DetailedLineRangeMapping) -> String {
    let changes = mapping.inner_changes.as_deref().unwrap_or(&[]);
    let modified = TextModelText::new(editors.modified_model.get().expect("o editor modificado tem modelo"));
    let original = TextModelText::new(editors.original.get_model().expect("o editor original tem modelo"));

    let edit = TextEdit::new(changes.iter().map(|c| c.to_text_edit(&modified)).collect());
    String::from(edit.apply(&original).to_string_lossy())
}

/// O `div.gutter`: o que o pintor lê.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffEditorGutterElement {
    /// `style.left`, gravado por `layout(left)`.
    pub left: f64,
    /// `style.width`.
    pub width: f64,
    /// `style.display`: `block` quando o menu tem ações, senão `none`.
    pub display: bool,
}

/// `DiffEditorGutter`.
pub struct DiffEditorGutter {
    base: Rc<Disposable>,
    editors: Rc<DiffEditorEditors>,
    width: IObservable<f64>,
    element: Rc<RefCell<DiffEditorGutterElement>>,
}

impl DiffEditorGutter {
    /// `new DiffEditorGutter(diffEditorRoot, diffModel, editors, options, sashLayout, boundarySashes)`, com `diffEditorRoot`,
    /// `diffModel`, `editors`, `options` e o `IInstantiationService` (de onde saem o `IContextKeyService` e o `IMenuService`) do
    /// contexto.
    #[allow(clippy::too_many_lines)]
    pub fn new(
        context: &DiffEditorFeatureContext,
        sash_layout: Rc<SashLayout>,
        boundary_sashes: IObservable<Option<Rc<BoundarySashes>>>,
        create_toolbar: DiffGutterToolbarFactory,
    ) -> Rc<Self> {
        let owner = DebugOwner::new("DiffEditorGutter");
        let base = Disposable::new();
        let editors = context.editors.clone();
        let diff_model = context.diff_model.clone();
        let options = context.options.clone();

        let (context_key_service, menu_service) =
            context.instantiation_service.invoke_function(|accessor| (accessor.get(I_CONTEXT_KEY_SERVICE), accessor.get(I_MENU_SERVICE)));

        let menu = menu_service.create_menu(&diff_editor_hunk_toolbar(), context_key_service.clone(), None);
        base.register(dispose_handle(&menu));
        let has_actions: IObservable<bool> = {
            let menu = menu.clone();
            observable_from_event_owned(owner.clone(), menu.on_did_change(), move |_| !menu.get_actions(None).is_empty())
        };
        let show_sash: IObservable<bool> = {
            let (render_side_by_side, has_actions) = (options.render_side_by_side.clone(), has_actions.clone());
            derived_owned(owner.clone(), move |reader| render_side_by_side.read(reader) && has_actions.read(reader))
        };
        let width: IObservable<f64> = {
            let has_actions = has_actions.clone();
            derived_owned(owner.clone(), move |reader| if has_actions.read(reader) { GUTTER_WIDTH } else { 0.0 })
        };

        let element = Rc::new(RefCell::new(DiffEditorGutterElement { left: 0.0, width: GUTTER_WIDTH, display: false }));
        {
            // applyStyle(this.elements.root, { display: this._hasActions.map(a => a ? 'block' : 'none') })
            let (element, has_actions) = (element.clone(), has_actions.clone());
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                element.borrow_mut().display = has_actions.read(reader);
            }));
        }

        let current_diff: IObservable<Option<usize>> = {
            let (diff_model, editors) = (diff_model.clone(), editors.clone());
            derived_owned(owner.clone(), move |reader| {
                let model = diff_model.read(reader)?;
                let diff = model.diff.read(reader);

                let cursor_position = editors.modified_cursor.read(reader);

                diff.and_then(|diff| diff.mappings.iter().position(|m| m.line_range_mapping.modified.contains(cursor_position.line_number)))
            })
        };
        let selected_diffs = selected_diffs_observable(owner.clone(), editors.clone(), diff_model.clone());

        let sash = {
            let (ui, root, sash_layout, enable_split_view_resizing) =
                (context.ui.clone(), context.root, sash_layout.clone(), options.enable_split_view_resizing.clone());
            derived_optional_disposable_of(Some(owner.clone()), move |reader| {
                if !show_sash.read(reader) {
                    return None;
                }
                let (sash_left, sash_left_for_setter) = (sash_layout.sash_left.clone(), sash_layout.sash_left.clone());
                let shifted_sash_left = derived_with_setter(
                    Some(DebugOwner::new("DiffEditorGutter")),
                    move |reader| sash_left.read(reader) - GUTTER_WIDTH,
                    move |v, tx| sash_left_for_setter.set(v + GUTTER_WIDTH, tx),
                );
                let sash_layout_for_reset = sash_layout.clone();
                Some(DiffEditorSash::new(
                    &ui,
                    root,
                    sash_layout.dimensions.clone(),
                    enable_split_view_resizing.clone(),
                    boundary_sashes.clone(),
                    shifted_sash_left,
                    Rc::new(move || sash_layout_for_reset.reset_sash()),
                ))
            })
        };
        base.register(recompute_initially_and_on_change(&sash, None));

        let gutter_items: IObservable<Rc<Vec<Rc<DiffGutterItem>>>> = derived_owned(owner.clone(), move |reader| {
            let Some(model) = diff_model.read(reader) else { return Rc::new(Vec::new()) };
            let Some(diffs) = model.diff.read(reader) else { return Rc::new(Vec::new()) };

            let selection = selected_diffs.read(reader);
            if !selection.is_empty() {
                let m = DetailedLineRangeMapping::from_range_mappings(selection.iter().flat_map(|s| s.range_mappings.iter().copied()).collect());
                return Rc::new(vec![Rc::new(DiffGutterItem {
                    mapping: m,
                    show_always: true,
                    menu_id: diff_editor_selection_toolbar(),
                    range_override: None,
                    original_uri: model.model.original.uri(),
                    modified_uri: model.model.modified.uri(),
                })]);
            }

            let current_diff = current_diff.read(reader);

            Rc::new(
                diffs
                    .mappings
                    .iter()
                    .enumerate()
                    .map(|(index, m)| {
                        Rc::new(DiffGutterItem {
                            mapping: m.line_range_mapping.with_inner_changes_from_line_ranges(),
                            show_always: Some(index) == current_diff,
                            menu_id: diff_editor_hunk_toolbar(),
                            range_override: None,
                            original_uri: model.model.original.uri(),
                            modified_uri: model.model.modified.uri(),
                        })
                    })
                    .collect(),
            )
        });

        base.register(EditorGutter::<Rc<DiffGutterItem>>::new(
            &editors.modified,
            sash_layout.dimensions.height.clone(),
            Rc::new(DiffGutterItemProvider { gutter_items, editors: editors.clone(), create_toolbar, context_key_service }),
        ));

        Rc::new(Self { base, editors, width, element })
    }

    /// O `div.gutter`.
    pub fn element(&self) -> DiffEditorGutterElement {
        *self.element.borrow()
    }

    /// O `click` da calha: foca o editor modificado.
    pub fn on_click(&self) {
        self.editors.modified.focus();
    }

    /// O `wheel` da calha só é repassado ao editor modificado (`delegateScrollFromMouseWheelEvent`) quando as opções de rolagem do
    /// modificado têm `handleMouseWheel`.
    pub fn should_delegate_mouse_wheel(&self) -> bool {
        self.editors.modified.get_option(SCROLLBAR).handle_mouse_wheel
    }
}

impl IDiffEditorGutter for DiffEditorGutter {
    fn width(&self) -> IObservable<f64> {
        self.width.clone()
    }

    fn layout(&self, left: f64) {
        self.element.borrow_mut().left = left;
    }
}

impl IDisposable for DiffEditorGutter {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `DiffGutterItem`.
pub struct DiffGutterItem {
    pub mapping: DetailedLineRangeMapping,
    pub show_always: bool,
    pub menu_id: MenuId,
    pub range_override: Option<LineRange>,
    pub original_uri: URI,
    pub modified_uri: URI,
}

impl IGutterItemInfo for Rc<DiffGutterItem> {
    fn id(&self) -> String {
        self.mapping.modified.to_string()
    }

    fn range(&self) -> LineRange {
        self.range_override.unwrap_or(self.mapping.modified)
    }
}

/// O objeto literal `{ getIntersectingGutterItems, createView }` que o `DiffEditorGutter` passa ao `EditorGutter`.
struct DiffGutterItemProvider {
    gutter_items: IObservable<Rc<Vec<Rc<DiffGutterItem>>>>,
    editors: Rc<DiffEditorEditors>,
    create_toolbar: DiffGutterToolbarFactory,
    context_key_service: Rc<dyn IContextKeyService>,
}

impl IGutterItemProvider<Rc<DiffGutterItem>> for DiffGutterItemProvider {
    fn get_intersecting_gutter_items(&self, _range: LineRange, reader: &dyn IReader) -> Vec<Rc<DiffGutterItem>> {
        self.gutter_items.read(reader).to_vec()
    }

    fn create_view(&self, item: IObservable<Rc<DiffGutterItem>>) -> Rc<dyn IGutterItemView> {
        DiffToolBar::new(item, self.editors.clone(), self.create_toolbar.clone(), self.context_key_service.clone())
    }
}

/// O `div.gutterItem`: o que o pintor lê.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffToolBarView {
    /// `style.width` (`34px`).
    pub width: f64,
    /// `style.height` (`20px`).
    pub height: f64,
    /// A classe `showAlways`.
    pub show_always: bool,
    /// A classe `noTransition`.
    pub no_transition: bool,
    /// `buttons.style.top`.
    pub buttons_top: f64,
}

/// `DiffToolBar`.
pub struct DiffToolBar {
    base: Rc<Disposable>,
    item: IObservable<Rc<DiffGutterItem>>,
    is_small: Rc<ObservableValue<bool, ()>>,
    toolbar: Rc<RefCell<Option<Rc<dyn IDiffGutterToolbar>>>>,
    view: Rc<RefCell<DiffToolBarView>>,
    last_item_range: Cell<Option<PixelRange>>,
    last_view_range: Cell<Option<PixelRange>>,
}

impl DiffToolBar {
    pub fn new(
        item: IObservable<Rc<DiffGutterItem>>,
        editors: Rc<DiffEditorEditors>,
        create_toolbar: DiffGutterToolbarFactory,
        context_key_service: Rc<dyn IContextKeyService>,
    ) -> Rc<Self> {
        let owner = DebugOwner::new("DiffToolBar");
        let base = Disposable::new();
        let show_always = item.map(|item, _| item.show_always);
        let menu_id = item.map(|item, _| item.menu_id.id.clone());
        let is_small = observable_value_owned::<bool, ()>(owner, false);
        let view = Rc::new(RefCell::new(DiffToolBarView { width: 34.0, height: 20.0, show_always: false, no_transition: false, buttons_top: 0.0 }));

        {
            // update showAlways
            let view = view.clone();
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                let show_always = show_always.read(reader);
                {
                    let mut view = view.borrow_mut();
                    view.no_transition = true;
                    view.show_always = show_always;
                }
                let view = view.clone();
                let _ = set_timeout(move || view.borrow_mut().no_transition = false, 0);
            }));
        }

        let this = Rc::new(Self {
            base,
            item: item.clone(),
            is_small: is_small.clone(),
            toolbar: Rc::default(),
            view,
            last_item_range: Cell::new(None),
            last_view_range: Cell::new(None),
        });

        {
            let (weak, toolbar_slot) = (Rc::downgrade(&this), this.toolbar.clone());
            this.base.register(autorun_with_store(move |reader, store| {
                let menu_id = MenuId::for_id(&menu_id.read(reader));
                let max_items = if is_small.read(reader) { 1 } else { 3 };

                let context: Rc<dyn Fn() -> DiffEditorSelectionHunkToolbarContext> = {
                    let (item, editors) = (item.clone(), editors.clone());
                    Rc::new(move || {
                        let item = item.get();
                        let mapping = item.mapping.clone();
                        DiffEditorSelectionHunkToolbarContext {
                            original_with_modified_changes: compute_staged_value(&editors, &mapping),
                            mapping,
                            original_uri: item.original_uri.clone(),
                            modified_uri: item.modified_uri.clone(),
                        }
                    })
                };
                let toolbar = create_toolbar(DiffGutterToolbarRequest {
                    menu_id,
                    context_key_service: context_key_service.clone(),
                    vertical: true,
                    max_items,
                    primary_group_prefix: "primary",
                    ignore_hidden_items: true,
                    should_forward_args: true,
                    instant_hover: true,
                    hover_position_right: true,
                    context,
                });
                store.add(dispose_handle(&toolbar));
                *toolbar_slot.borrow_mut() = Some(toolbar.clone());
                {
                    let toolbar_slot = toolbar_slot.clone();
                    store.add(to_disposable(move || *toolbar_slot.borrow_mut() = None));
                }

                let weak = weak.clone();
                store.add(toolbar.on_did_change_menu_items().listen(move |_| {
                    let Some(this) = weak.upgrade() else { return };
                    if let (Some(item_range), Some(view_range)) = (this.last_item_range.get(), this.last_view_range.get()) {
                        this.layout(item_range, view_range);
                    }
                }));
            }));
        }

        this
    }

    /// O `div.gutterItem`.
    pub fn view(&self) -> DiffToolBarView {
        *self.view.borrow()
    }
}

impl IGutterItemView for DiffToolBar {
    fn layout(&self, item_range: PixelRange, view_range: PixelRange) {
        self.last_item_range.set(Some(item_range));
        self.last_view_range.set(Some(view_range));

        self.is_small.set(self.item.get().mapping.original.start_line_number == 1 && item_range.length() < 30.0, None);
        // Item might have changed
        let item_height = self.toolbar.borrow().as_ref().map_or(0.0, |toolbar| toolbar.client_height());

        let middle_height = item_range.length() / 2.0 - item_height / 2.0;

        let margin = item_height;

        let mut effective_checkbox_top = item_range.start + middle_height;

        let preferred_view_port_range = PixelRange::try_create(margin, view_range.end_exclusive - margin - item_height);

        let preferred_parent_range = PixelRange::try_create(item_range.start + margin, item_range.end_exclusive - item_height - margin);

        if let (Some(preferred_parent_range), Some(preferred_view_port_range)) = (preferred_parent_range, preferred_view_port_range)
            && preferred_parent_range.start < preferred_parent_range.end_exclusive
        {
            effective_checkbox_top = preferred_view_port_range.clip(effective_checkbox_top);
            effective_checkbox_top = preferred_parent_range.clip(effective_checkbox_top);
        }

        self.view.borrow_mut().buttons_top = effective_checkbox_top - item_range.start;
    }
}

impl IDisposable for DiffToolBar {
    fn dispose(&self) {
        self.base.dispose();
    }
}
