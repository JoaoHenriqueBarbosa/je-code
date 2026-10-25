//! Porte de `src/vs/editor/browser/widget/diffEditor/utils/editorGutter.ts`: a calha ao lado de um editor, com uma visão por item que
//! acompanha a linha (ou o intervalo de linhas) do item enquanto o editor rola, muda de zonas de visão ou de tamanho.
//!
//! Divergências:
//! - **DOM vira dado.** O `domNode` da calha (`div.gutter.monaco-editor`) e o `div` de cada item não existem. A posição vertical de
//!   cada visão (`style.top` e `style.height`) é o [`GutterItemLayout`] ([`EditorGutter::item_layouts`]), a classe
//!   `scroll-decoration` é [`EditorGutter::scroll_decoration_visible`], e a visão do item recebe só o observável do item no
//!   `createView(item, target)`: o `target` (o `div` onde a visão se desenha) é do pintor, que acha a visão pelo id do item.
//! - **Altura da calha.** O `ResizeObserver` do `domNode` e o `clientHeight` são o observável de altura recebido no construtor: ler a
//!   altura no `render` faz o papel do `domNodeSizeChanged` e do `clientHeight`.
//! - **`OffsetRange` de pixels.** O `OffsetRange` do porte é inteiro, e as posições da calha são pixels fracionários (o `layout` do
//!   `DiffToolBar` divide o comprimento por dois), então o intervalo que a visão recebe é o [`PixelRange`], com os mesmos membros
//!   usados (`start`, `endExclusive`, `length`, `isEmpty`, `ofStartAndLength`).
//! - **`IGutterItemInfo`** (`{ id, range }`) é o trait [`IGutterItemInfo`], que o item implementa, e o item é um valor com igualdade
//!   estrita do observável (`StrictEquals`), como o objeto do original.
//! - `this._editor.getModel()!` é o `expect` do modelo; `unusedIds` é um `HashSet` e a ordem em que as visões velhas morrem não é
//!   observável.

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use je_base::lifecycle::{Disposable, DisposableMap, IDisposable};
use je_base::observable::observables::observable_from_event::observable_from_event_owned;
use je_base::observable::observables::observable_signal_from_event::observable_signal_from_event_owned;
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{
    DebugOwner, IObservable, IReader, Observable, ObservableExt, SettableExt, StrictEquals, autorun, observable_value, transaction,
};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::cursor_common::CursorSimpleModel;
use crate::line_range::LineRange;

/// `OffsetRange` com pixels fracionários (ver as divergências do módulo).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelRange {
    pub start: f64,
    pub end_exclusive: f64,
}

impl PixelRange {
    /// `OffsetRange.ofStartAndLength(start, length)`.
    pub fn of_start_and_length(start: f64, length: f64) -> Self {
        Self { start, end_exclusive: start + length }
    }

    /// `length`.
    pub fn length(&self) -> f64 {
        self.end_exclusive - self.start
    }

    /// `isEmpty`.
    pub fn is_empty(&self) -> bool {
        self.start >= self.end_exclusive
    }
}

/// `IGutterItemInfo`.
pub trait IGutterItemInfo {
    fn id(&self) -> String;

    fn range(&self) -> LineRange;
}

/// `IGutterItemView`.
pub trait IGutterItemView: IDisposable {
    /// `layout(itemRange, viewRange)`.
    fn layout(&self, item_range: PixelRange, view_range: PixelRange);
}

/// `IGutterItemProvider<TItem>`.
pub trait IGutterItemProvider<T> {
    /// `getIntersectingGutterItems(range, reader)`.
    fn get_intersecting_gutter_items(&self, range: LineRange, reader: &dyn IReader) -> Vec<T>;

    /// `createView(item, target)`, sem o `target` (ver as divergências do módulo).
    fn create_view(&self, item: IObservable<T>) -> Rc<dyn IGutterItemView>;
}

/// A posição vertical da visão de um item (o `style.top` e o `style.height` do `div` dela), em pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GutterItemLayout {
    pub top: f64,
    pub height: f64,
}

/// `ManagedGutterItemView`.
struct ManagedGutterItemView<T: Clone + StrictEquals + 'static> {
    item: Rc<ObservableValue<T, ()>>,
    gutter_item_view: Rc<dyn IGutterItemView>,
    layout: Cell<GutterItemLayout>,
}

impl<T: Clone + StrictEquals + 'static> IDisposable for ManagedGutterItemView<T> {
    fn dispose(&self) {
        self.gutter_item_view.dispose();
    }
}

/// O que o `render` lê, reunido para o autorun capturar de uma vez.
struct GutterRenderer<T: IGutterItemInfo + Clone + StrictEquals + 'static> {
    editor: Rc<CodeEditorWidget>,
    item_provider: Rc<dyn IGutterItemProvider<T>>,
    dom_node_height: IObservable<f64>,
    scroll_top: IObservable<f64>,
    model_attached: IObservable<bool>,
    editor_on_did_change_view_zones: IObservable<()>,
    editor_on_did_content_size_change: IObservable<()>,
    views: Rc<DisposableMap<String, ManagedGutterItemView<T>>>,
}

impl<T: IGutterItemInfo + Clone + StrictEquals + 'static> GutterRenderer<T> {
    /// `render(reader)`.
    fn render(&self, reader: &dyn IReader) {
        if !self.model_attached.read(reader) {
            return;
        }

        self.editor_on_did_change_view_zones.read(reader);
        self.editor_on_did_content_size_change.read(reader);

        let scroll_top = self.scroll_top.read(reader);

        let visible_ranges = self.editor.get_visible_ranges();
        let mut unused_ids: HashSet<String> = self.views.keys().into_iter().collect();

        let view_range = PixelRange::of_start_and_length(0.0, self.dom_node_height.read(reader));

        if !view_range.is_empty() {
            for visible_range in visible_ranges {
                let visible_range2 = LineRange::new(visible_range.start_line_number, visible_range.end_line_number + 1);

                let gutter_items = self.item_provider.get_intersecting_gutter_items(visible_range2, reader);

                transaction(|tx| {
                    // EditorGutter.render
                    for gutter_item in gutter_items {
                        let range = gutter_item.range();
                        if range.intersect(&visible_range2).is_none() {
                            continue;
                        }

                        let id = gutter_item.id();
                        unused_ids.remove(&id);
                        let view = match self.views.get(&id) {
                            Some(view) => {
                                view.item.set(gutter_item.clone(), Some(tx));
                                view
                            }
                            None => {
                                let gutter_item_obs = observable_value::<T, ()>("item", gutter_item.clone());
                                let item_view = self.item_provider.create_view(gutter_item_obs.to_observable());
                                let view = Rc::new(ManagedGutterItemView {
                                    item: gutter_item_obs,
                                    gutter_item_view: item_view,
                                    layout: Cell::new(GutterItemLayout { top: 0.0, height: 0.0 }),
                                });
                                self.views.set(id, view.clone(), false);
                                view
                            }
                        };

                        let line_count = self.editor.get_model().expect("o editor da calha tem modelo").get_line_count();
                        let top = if range.start_line_number <= line_count {
                            self.editor.get_top_for_line_number(range.start_line_number, Some(true)) - scroll_top
                        } else if range.start_line_number > 1 {
                            self.editor.get_bottom_for_line_number(range.start_line_number - 1, Some(false)) - scroll_top
                        } else {
                            0.0
                        };
                        let bottom = if range.end_line_number_exclusive == 1 {
                            top.max(self.editor.get_top_for_line_number(range.start_line_number, Some(false)) - scroll_top)
                        } else {
                            top.max(self.editor.get_bottom_for_line_number(range.end_line_number_exclusive - 1, Some(true)) - scroll_top)
                        };

                        let height = bottom - top;
                        view.layout.set(GutterItemLayout { top, height });

                        view.gutter_item_view.layout(PixelRange::of_start_and_length(top, height), view_range);
                    }
                });
            }
        }

        for id in unused_ids {
            self.views.delete_and_dispose(&id);
        }
    }
}

/// `EditorGutter<T>`.
pub struct EditorGutter<T: IGutterItemInfo + Clone + StrictEquals + 'static> {
    base: Rc<Disposable>,
    views: Rc<DisposableMap<String, ManagedGutterItemView<T>>>,
    scroll_decoration_visible: Rc<Cell<bool>>,
}

impl<T: IGutterItemInfo + Clone + StrictEquals + 'static> EditorGutter<T> {
    /// `new EditorGutter(editor, domNode, itemProvider)`; `dom_node_height` é o `clientHeight` do `domNode` como observável.
    pub fn new(editor: &Rc<CodeEditorWidget>, dom_node_height: IObservable<f64>, item_provider: Rc<dyn IGutterItemProvider<T>>) -> Rc<Self> {
        let owner = DebugOwner::new("EditorGutter");
        let base = Disposable::new();

        let scroll_top = {
            let editor = editor.clone();
            observable_from_event_owned(owner.clone(), editor.on_did_scroll_change(), move |_| editor.get_scroll_top())
        };
        let is_scroll_top_zero = scroll_top.map(|scroll_top, _| scroll_top == 0.0);
        let model_attached = {
            let editor = editor.clone();
            observable_from_event_owned(owner.clone(), editor.on_did_change_model(), move |_| editor.has_model())
        };
        let editor_on_did_change_view_zones = observable_signal_from_event_owned(owner.clone(), editor.on_did_change_view_zones());
        let editor_on_did_content_size_change = observable_signal_from_event_owned(owner, editor.on_did_content_size_change());
        let views = DisposableMap::<String, ManagedGutterItemView<T>>::new();
        base.register(views.clone());

        let scroll_decoration_visible = Rc::new(Cell::new(false));
        {
            let scroll_decoration_visible = scroll_decoration_visible.clone();
            base.register(autorun(move |reader| {
                // update scroll decoration
                let reader: &dyn IReader = reader;
                scroll_decoration_visible.set(!is_scroll_top_zero.read(reader));
            }));
        }

        let renderer = GutterRenderer {
            editor: editor.clone(),
            item_provider,
            dom_node_height,
            scroll_top,
            model_attached,
            editor_on_did_change_view_zones,
            editor_on_did_content_size_change,
            views: views.clone(),
        };
        base.register(autorun(move |reader| {
            // EditorGutter.Render
            let reader: &dyn IReader = reader;
            renderer.render(reader);
        }));

        Rc::new(Self { base, views, scroll_decoration_visible })
    }

    /// A classe `scroll-decoration` do `div.scroll-decoration` (presente quando o editor não está no topo).
    pub fn scroll_decoration_visible(&self) -> bool {
        self.scroll_decoration_visible.get()
    }

    /// O id e a posição vertical da visão de cada item que a calha mostra agora.
    pub fn item_layouts(&self) -> Vec<(String, GutterItemLayout)> {
        self.views.keys().into_iter().filter_map(|id| self.views.get(&id).map(|view| (id, view.layout.get()))).collect()
    }
}

impl<T: IGutterItemInfo + Clone + StrictEquals + 'static> IDisposable for EditorGutter<T> {
    fn dispose(&self) {
        // super.dispose(); reset(this._domNode)
        self.base.dispose();
    }
}
