//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViews/jumpToView.ts`:
//! `JumpToView`, o aviso "tecla para saltar" (ou o cursor piscante) ao lado da posição para onde a edição salta.
//!
//! Notas de tradução:
//! - o `n.div` com o contêiner arredondado e os filhos (o rótulo do atalho e o texto "to jump", ou o cursor) é o
//!   [`JumpToViewVisual`], lido pelo pintor nativo em [`JumpToView::visual`]. O `display: flex | none` é a presença do
//!   dado (`None` sem layout). O `KeybindingLabel` do original, que monta o atalho num elemento DOM, é o
//!   [`ResolvedKeybinding`] em [`JumpToViewContent::Label`]: quem pinta desenha os acordes com o primeiro plano da view,
//!   fundo transparente e borda no primeiro plano (as opções que o original passa ao `KeybindingLabel`, sem sombra nem
//!   borda inferior), e sem título;
//! - as cores `getEditorBlendedColor(...).toString()` ficam como [`Color`] (o pintor serializa); `--vscodeIconForeground`
//!   é o próprio primeiro plano;
//! - `@IThemeService`, `@IKeybindingService` e `@IContextKeyService` chegam como `Rc<dyn ...>`. O `_getKeybinding` do
//!   original devolve `constObservable(undefined)` sem `commandId`; aqui o `commandId` é sempre o
//!   `INLINE_SUGGEST_COMMIT_ID`, então esse ramo não existe;
//! - `observePosition(derived(...), reader.store)` dentro de um `derived(...).flatten()` é o `derived_with_store` seguido
//!   de `flatten`, de modo que o widget de medição vive enquanto o derivado externo é observado, como no original;
//! - `animateFixedValues` (`observableFromEvent` com a função de inscrição e um `RunOnceScheduler`) é a função local
//!   [`animate_fixed_values`]: o [`Event`] inscreve o temporizador (reinicia o índice, agenda o primeiro tique em zero) e
//!   o valor lido é o do índice atual, como no original. O piscar só corre enquanto a view é observada com o cursor;
//! - `IModelDeltaDecoration` com `after: { content }` é o [`ModelDeltaDecoration`] com o [`InjectedTextOptions`] de
//!   `model`; o `satisfies` do original é só uma checagem de tipo.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::RunOnceScheduler;
use je_base::event::{Event, Listener};
use je_base::keybindings::ResolvedKeybinding;
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::observable::{
    FlattenExt, IObservable, Observable, ObservableExt, StrictEquals, const_observable, derived, derived_with_store, observable_from_event,
};
use je_platform::contextkey::IContextKeyService;
use je_platform::keybinding::IKeybindingService;
use je_theme::color::Color;

use crate::core_2d::point::Point;
use crate::core_2d::rect::Rect;
use crate::inline_completions::controller::command_ids::INLINE_SUGGEST_COMMIT_ID;
use crate::model::{InjectedTextOptions, ModelDecorationOptionsInit, ModelDeltaDecoration};
use crate::observable_code_editor::{ObservableCodeEditor, ValueOrObservable};
use crate::position::Position;
use crate::range::Range;
use crate::services::theme_service::IThemeService;
use crate::text_model::ModelDecorationOptions;

use super::super::theme::{
    INLINE_EDIT_INDICATOR_PRIMARY_BACKGROUND, INLINE_EDIT_INDICATOR_PRIMARY_BORDER, INLINE_EDIT_INDICATOR_PRIMARY_FOREGROUND, get_editor_blended_color,
};
use super::NonOverflowOverlayWidget;

/// O texto depois do atalho (`['to jump']`).
pub const JUMP_TO_LABEL_TEXT: &str = "to jump";
/// O `gap: '4px'` do contêiner.
pub const JUMP_TO_CONTAINER_GAP: f64 = 4.0;
/// O `padding: '0 4px'` do contêiner: o recuo horizontal.
pub const JUMP_TO_CONTAINER_PADDING_HORIZONTAL: f64 = 4.0;
/// A borda de `1px solid` do contêiner.
pub const JUMP_TO_BORDER_WIDTH: f64 = 1.0;
/// O `borderRadius: '3px'` do contêiner.
pub const JUMP_TO_BORDER_RADIUS: f64 = 3.0;
/// O `fontSize: '11px'` do contêiner.
pub const JUMP_TO_FONT_SIZE: f64 = 11.0;
/// O `marginTop: '-2px'` do texto "to jump".
pub const JUMP_TO_LABEL_MARGIN_TOP: f64 = -2.0;
/// O `borderLeft: '2px solid'` do cursor.
pub const JUMP_TO_CURSOR_WIDTH: f64 = 2.0;
/// O `height: 14` do cursor.
pub const JUMP_TO_CURSOR_HEIGHT: f64 = 14.0;
/// A largura do `Rect` do widget (`Rect.fromLeftTopWidthHeight(..., 100, lineHeight)`).
const WIDGET_WIDTH: f64 = 100.0;
/// O deslocamento horizontal do widget em relação à posição.
const WIDGET_OFFSET_X: f64 = 2.0;

/// `options.style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JumpToViewStyle {
    Label,
    Cursor,
}

/// `options: { style }` do construtor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JumpToViewOptions {
    pub style: JumpToViewStyle,
}

/// O `{ jumpToPosition: Position }` de `_data`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JumpToViewData {
    pub jump_to_position: Position,
}

/// O `_layout` do original: `{ widgetRect }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JumpToViewLayout {
    pub widget_rect: Rect,
}

/// O conteúdo do contêiner: o cursor piscante ou o rótulo.
#[derive(Clone)]
pub enum JumpToViewContent {
    /// O `div` com a borda esquerda; `opacity` é `0` ou `1` conforme o piscar.
    Cursor { opacity: f64 },
    /// O atalho resolvido de `inlineSuggestCommitId` (`None` sem atalho) e o texto [`JUMP_TO_LABEL_TEXT`], cuja
    /// `lineHeight` é a altura do widget.
    Label { keybinding: Option<Rc<dyn ResolvedKeybinding>>, text: &'static str, line_height: f64 },
}

/// O que o pintor nativo lê da view: o retângulo do widget, as cores do contêiner e o conteúdo. As constantes
/// `JUMP_TO_*` deste módulo são as demais medidas do contêiner.
#[derive(Clone)]
pub struct JumpToViewVisual {
    pub widget_rect: Rect,
    /// `styles.background` (`inlineEditIndicatorPrimaryBackground` sobre o fundo do editor).
    pub background: Rc<Color>,
    /// `styles.foreground`: a cor do texto, do ícone e (no rótulo) do atalho.
    pub foreground: Rc<Color>,
    /// `styles.border`.
    pub border: Rc<Color>,
    pub content: JumpToViewContent,
}

/// O `_styles` do original.
struct JumpToViewStyles {
    background: Rc<Color>,
    foreground: Rc<Color>,
    border: Rc<Color>,
}

/// Um valor e por quanto tempo ele vale (`{ value, durationMs }`).
struct FixedValue<T> {
    value: T,
    duration_ms: u64,
}

/// `animateFixedValues(values)`: o valor da vez, trocando a cada `durationMs` em ciclo.
fn animate_fixed_values<T: Clone + StrictEquals + 'static>(values: Vec<FixedValue<T>>) -> IObservable<T> {
    let values = Rc::new(values);
    let idx = Rc::new(Cell::new(0_usize));

    let event = Event::<()>::new({
        let (values, idx) = (Rc::clone(&values), Rc::clone(&idx));
        move |listener: Listener<()>| {
            idx.set(0);
            let slot: Rc<RefCell<Weak<RunOnceScheduler>>> = Rc::default();
            let timer = RunOnceScheduler::new(
                {
                    let (values, idx, slot) = (Rc::clone(&values), Rc::clone(&idx), Rc::clone(&slot));
                    move || {
                        idx.set((idx.get() + 1) % values.len());
                        listener(&());
                        let timer = slot.borrow().upgrade();
                        if let Some(timer) = timer {
                            timer.schedule(Some(values[idx.get()].duration_ms));
                        }
                    }
                },
                0,
            );
            *slot.borrow_mut() = Rc::downgrade(&timer);
            timer.schedule(Some(0));

            to_disposable(move || timer.dispose())
        }
    });

    observable_from_event(event, move |_| values[idx.get()].value.clone())
}

/// `_getKeybinding(commandId)`: o atalho do comando, relido quando o contexto muda.
fn lookup_keybinding_observable(
    command_id: &'static str,
    keybinding_service: Rc<dyn IKeybindingService>,
    context_key_service: &Rc<dyn IContextKeyService>,
) -> IObservable<Option<Rc<dyn ResolvedKeybinding>>> {
    observable_from_event(context_key_service.on_did_change_context(), move |_| keybinding_service.lookup_keybinding(command_id, None, false))
}

/// `JumpToView`.
pub struct JumpToView {
    base: Rc<Disposable>,
    visual: IObservable<Option<Rc<JumpToViewVisual>>>,
}

impl JumpToView {
    pub fn new(
        editor: Rc<ObservableCodeEditor>,
        options: JumpToViewOptions,
        data: IObservable<Option<Rc<JumpToViewData>>>,
        theme_service: Rc<dyn IThemeService>,
        keybinding_service: Rc<dyn IKeybindingService>,
        context_key_service: Rc<dyn IContextKeyService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let style = options.style;
        let keybinding = lookup_keybinding_observable(INLINE_SUGGEST_COMMIT_ID, keybinding_service, &context_key_service);

        base.register(editor.create_overlay_widget(NonOverflowOverlayWidget::new(const_observable(None), const_observable(0.0))));

        base.register(editor.set_decorations(&{
            let data = Rc::clone(&data);
            derived(move |reader| {
                let Some(data) = data.read(reader) else {
                    return Rc::new(Vec::new());
                };
                // use injected text at position
                Rc::new(vec![ModelDeltaDecoration {
                    range: Range::from_positions(data.jump_to_position, data.jump_to_position),
                    options: ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
                        description: "inline-edit-jump-to-decoration".to_owned(),
                        inline_class_name_affects_letter_spacing: true,
                        show_if_collapsed: true,
                        after: Some(InjectedTextOptions {
                            content: if style == JumpToViewStyle::Label { "          " } else { "  " }.to_owned(),
                            ..InjectedTextOptions::default()
                        }),
                        ..Default::default()
                    }),
                }])
            })
        }));

        let styles: IObservable<Rc<JumpToViewStyles>> = derived(move |reader| {
            let blended = |id: &'static str| get_editor_blended_color(ValueOrObservable::Value(id), &theme_service).read(reader);
            Rc::new(JumpToViewStyles {
                background: blended(INLINE_EDIT_INDICATOR_PRIMARY_BACKGROUND),
                foreground: blended(INLINE_EDIT_INDICATOR_PRIMARY_FOREGROUND),
                border: blended(INLINE_EDIT_INDICATOR_PRIMARY_BORDER),
            })
        });

        let pos: IObservable<Option<Point>> = {
            let (editor, data) = (Rc::clone(&editor), Rc::clone(&data));
            derived_with_store(move |_, store| {
                let jump_to_position: IObservable<Option<Position>> = {
                    let data = Rc::clone(&data);
                    derived(move |reader| data.read(reader).map(|data| data.jump_to_position))
                };
                editor.observe_position(&jump_to_position, store)
            })
            .flatten()
        };

        let layout: IObservable<Option<Rc<JumpToViewLayout>>> = {
            let (editor, data) = (Rc::clone(&editor), Rc::clone(&data));
            derived(move |reader| {
                let data = data.read(reader)?;

                let position = data.jump_to_position;
                let line_height = editor.observe_line_height_for_line(ValueOrObservable::Observable(const_observable(position.line_number))).read(reader);
                let scroll_left = editor.scroll_left.read(reader);

                let point = pos.read(reader)?;

                let layout = editor.layout_info.read(reader);

                let widget_rect = Rect::from_left_top_width_height(
                    point.x + layout.content_left + WIDGET_OFFSET_X - scroll_left,
                    point.y,
                    WIDGET_WIDTH,
                    line_height,
                );

                Some(Rc::new(JumpToViewLayout { widget_rect }))
            })
        };

        let blink = animate_fixed_values(vec![FixedValue { value: true, duration_ms: 600 }, FixedValue { value: false, duration_ms: 600 }]);

        // `_widget`
        let visual = derived(move |reader| {
            let layout = layout.read(reader)?;
            // O contêiner só existe com o dado presente.
            data.read(reader)?;

            let styles = styles.read(reader);
            let content = match style {
                JumpToViewStyle::Cursor => JumpToViewContent::Cursor { opacity: if blink.read(reader) { 0.0 } else { 1.0 } },
                JumpToViewStyle::Label => JumpToViewContent::Label {
                    keybinding: keybinding.read(reader),
                    text: JUMP_TO_LABEL_TEXT,
                    line_height: layout.widget_rect.height(),
                },
            };

            Some(Rc::new(JumpToViewVisual {
                widget_rect: layout.widget_rect,
                background: Rc::clone(&styles.background),
                foreground: Rc::clone(&styles.foreground),
                border: Rc::clone(&styles.border),
                content,
            }))
        })
        .recompute_initially_and_on_change(base.store(), None);

        Rc::new(Self { base, visual })
    }

    /// O dado que o pintor nativo lê (`None` quando o widget está escondido).
    pub fn visual(&self) -> IObservable<Option<Rc<JumpToViewVisual>>> {
        Rc::clone(&self.visual)
    }
}

impl IDisposable for JumpToView {
    fn dispose(&self) {
        self.base.dispose();
    }
}
