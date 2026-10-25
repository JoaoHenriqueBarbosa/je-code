//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViews/inlineEditsCollapsedView.ts`:
//! `InlineEditsCollapsedView`, o indicador recolhido (um triângulo sob a posição inicial da edição) e a animação de pulso.
//!
//! Notas de tradução:
//! - o `n.svg` com o `path` é o [`CollapsedViewVisual`] (o retângulo de sobreposição válido do editor, que é a caixa do
//!   SVG com `overflow: hidden` e `pointer-events: none`, o contorno do triângulo nas coordenadas dessa caixa e a cor de
//!   preenchimento), lido pelo pintor nativo em [`InlineEditsCollapsedView::visual`];
//! - `ICodeEditor` e o `observableCodeEditor(editor)` do construtor são o `Rc<CodeEditorWidget>` e o
//!   [`observable_code_editor`]; `this._editor.getModel()!` do `startPosition` é o `get_model()` com `?`: sem modelo não
//!   há posição a mostrar;
//! - `@IAccessibilityService` é o `Rc<dyn IAccessibilityService>` de `je_platform::accessibility`;
//! - `triggerAnimation()` anima o elemento SVG com `Element.animate` (Web Animations) e devolve `animation.finished`. Aqui
//!   o pintor nativo é o dono do relógio: [`InlineEditsCollapsedView::trigger_animation`] publica um
//!   [`CollapsedViewPulse`] em [`InlineEditsCollapsedView::pulse`] e devolve a promessa que o `finish` dele resolve. O
//!   pintor mede o tempo desde que viu o pulso, aplica o `translateY` de [`CollapsedViewPulse::translate_y_at`] ao SVG e
//!   chama `finish` quando passa de [`PULSE_DURATION_MS`]. Com movimento reduzido o original devolve
//!   `new Animation(null, null).finished`, uma promessa já resolvida. Um pulso novo substitui o anterior (o elemento é
//!   um só) e resolve a promessa dele, já que não há mais o que animar;
//! - os keyframes são interpolados linearmente (o `easing` padrão dos keyframes do `animate`).

use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable};
use je_base::event::Event;
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{IObservable, Observable, ObservableExt, SettableExt, const_observable, derived, observable_value};
use je_base::promise::{Promise, Resolver};
use je_platform::accessibility::IAccessibilityService;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::core_2d::point::Point;
use crate::core_2d::rect::Rect;
use crate::inline_completions::compute_ghost_text::single_text_remove_common_prefix;
use crate::observable_code_editor::observable_code_editor;
use crate::position::Position;

use super::super::inline_edit_with_changes::InlineEditWithChanges;
use super::super::inline_edits_view_interface::{IInlineEditsView, InlineEditClickEvent};
use super::super::theme::INLINE_EDIT_INDICATOR_PRIMARY_BORDER;
use super::super::utils::utils::{PathBuilder, PathCommand, get_editor_valid_overlay_rect};
use super::NonOverflowOverlayWidget;

/// Um keyframe do pulso: `{ offset, transform: 'translateY(<translate_y>px)' }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PulseKeyframe {
    pub offset: f64,
    pub translate_y: f64,
}

/// Os keyframes do pulso (`// PULSE ANIMATION:`).
pub const PULSE_KEYFRAMES: [PulseKeyframe; 7] = [
    PulseKeyframe { offset: 0.00, translate_y: -3.0 },
    PulseKeyframe { offset: 0.20, translate_y: 1.0 },
    PulseKeyframe { offset: 0.36, translate_y: -1.0 },
    PulseKeyframe { offset: 0.52, translate_y: 1.0 },
    PulseKeyframe { offset: 0.68, translate_y: -1.0 },
    PulseKeyframe { offset: 0.84, translate_y: 1.0 },
    PulseKeyframe { offset: 1.00, translate_y: 0.0 },
];

/// `{ duration: 2000 }`.
pub const PULSE_DURATION_MS: f64 = 2000.0;

/// Uma execução do pulso, que o pintor nativo anima (ver as notas do módulo).
pub struct CollapsedViewPulse {
    finished: Resolver<()>,
}

impl CollapsedViewPulse {
    /// O `translateY` (em pixels) do SVG `elapsed_ms` depois do início do pulso: interpolação linear entre os keyframes.
    /// Passada a duração vale o último keyframe, e antes de começar vale o primeiro.
    pub fn translate_y_at(elapsed_ms: f64) -> f64 {
        let progress = (elapsed_ms / PULSE_DURATION_MS).clamp(0.0, 1.0);
        for pair in PULSE_KEYFRAMES.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            if progress <= to.offset {
                let fraction = (progress - from.offset) / (to.offset - from.offset);
                return from.translate_y + (to.translate_y - from.translate_y) * fraction;
            }
        }
        PULSE_KEYFRAMES[PULSE_KEYFRAMES.len() - 1].translate_y
    }

    /// O `animation.finished` resolve: o pintor chama quando o pulso terminou.
    pub fn finish(&self) {
        self.finished.resolve(());
    }
}

/// O que o pintor nativo lê da view (o `n.svg` do `getCollapsedIndicator`).
#[derive(Clone, Debug, PartialEq)]
pub struct CollapsedViewVisual {
    /// A caixa do SVG (`getEditorValidOverlayRect`), com `overflow: hidden` e `pointer-events: none`.
    pub overlay_rect: Rect,
    /// O contorno do triângulo, nas coordenadas da caixa (a posição menos o `contentLeft`). Vazio sem posição.
    pub path: Rc<Vec<PathCommand>>,
    /// O `fill` (`asCssVariable(inlineEditIndicatorPrimaryBorder)`), como identificador do tema.
    pub fill_color: &'static str,
}

/// `createIconPath`: o contorno do triângulo (uma base de 6 por 1 com a ponta 3 abaixo) sobre o ponto.
fn create_icon_path(point: Point) -> Vec<PathCommand> {
    let width = 6.0;
    let triangle_height = 3.0;
    let base_height = 1.0;

    let base_top_left = point.delta_x(-width / 2.0).delta_y(-base_height);
    let base_top_right = base_top_left.delta_x(width);
    let base_bottom_left = base_top_left.delta_y(base_height);
    let base_bottom_right = base_top_right.delta_y(base_height);
    let triangle_bottom_center = base_bottom_left.delta_x(width / 2.0).delta_y(triangle_height);
    let mut path = PathBuilder::new();
    path.move_to(base_top_left)
        .line_to(base_top_right)
        .line_to(base_bottom_right)
        .line_to(triangle_bottom_center)
        .line_to(base_bottom_left)
        .line_to(base_top_left);
    path.build()
}

/// `InlineEditsCollapsedView`.
pub struct InlineEditsCollapsedView {
    base: Rc<Disposable>,
    accessibility_service: Rc<dyn IAccessibilityService>,
    visual: IObservable<Rc<CollapsedViewVisual>>,
    pulse: Rc<ObservableValue<Option<Rc<CollapsedViewPulse>>, ()>>,
    pub is_visible: IObservable<bool>,
    is_hovered: IObservable<bool>,
}

impl InlineEditsCollapsedView {
    pub fn new(
        editor: Rc<CodeEditorWidget>,
        edit: IObservable<Option<Rc<InlineEditWithChanges>>>,
        accessibility_service: Rc<dyn IAccessibilityService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let editor_obs = observable_code_editor(&editor);

        let start_position: IObservable<Option<Position>> = edit.map({
            let editor = Rc::clone(&editor);
            move |inline_edit, _| {
                let first_edit = inline_edit.as_ref()?.edit.as_ref()?.replacements.first()?;
                let model = editor.get_model()?;
                Some(single_text_remove_common_prefix(first_edit, &model, None).range.get_start_position())
            }
        });
        let observed_start_point = editor_obs.observe_position(&start_position, base.store());
        let content_left = Rc::clone(&editor_obs.layout_info_content_left);
        let start_point: IObservable<Option<Point>> = {
            let (content_left, scroll_left) = (Rc::clone(&content_left), Rc::clone(&editor_obs.scroll_left));
            derived(move |reader| {
                let point = observed_start_point.read(reader)?;
                Some(Point::new(content_left.read(reader) + point.x - scroll_left.read(reader), point.y))
            })
        };

        // `getCollapsedIndicator`: o ponto sem o `contentLeft`, porque o SVG começa nele.
        let start_point_translated: IObservable<Option<Point>> =
            start_point.map(move |p, reader| p.map(|p| p.delta_x(-content_left.read(reader))));
        let icon_path: IObservable<Rc<Vec<PathCommand>>> =
            start_point_translated.map(|point, _| Rc::new(point.map(create_icon_path).unwrap_or_default()));
        let overlay_rect = get_editor_valid_overlay_rect(&editor_obs);
        let visual = derived(move |reader| {
            Rc::new(CollapsedViewVisual {
                overlay_rect: overlay_rect.read(reader),
                path: icon_path.read(reader),
                fill_color: INLINE_EDIT_INDICATOR_PRIMARY_BORDER,
            })
        })
        .recompute_initially_and_on_change(base.store(), None);

        base.register(editor_obs.create_overlay_widget(NonOverflowOverlayWidget::new(const_observable(None), const_observable(0.0))));

        let is_visible = edit.map(move |inline_edit, reader| inline_edit.is_some() && start_point.read(reader).is_some());

        Rc::new(Self {
            base,
            accessibility_service,
            visual,
            pulse: observable_value::<Option<Rc<CollapsedViewPulse>>, ()>("pulse", None),
            is_visible,
            is_hovered: const_observable(false),
        })
    }

    /// O dado que o pintor nativo lê.
    pub fn visual(&self) -> IObservable<Rc<CollapsedViewVisual>> {
        Rc::clone(&self.visual)
    }

    /// O pulso em andamento, que o pintor nativo anima (`None` antes do primeiro `trigger_animation`).
    pub fn pulse(&self) -> IObservable<Option<Rc<CollapsedViewPulse>>> {
        self.pulse.to_observable()
    }

    /// `triggerAnimation()`: a promessa resolve quando o pulso termina.
    pub fn trigger_animation(&self) -> Promise<()> {
        if self.accessibility_service.is_motion_reduced() {
            return Promise::resolved(());
        }

        // PULSE ANIMATION:
        let (finished, resolver) = Promise::new();
        if let Some(previous) = self.pulse.get() {
            previous.finish();
        }
        self.pulse.set(Some(Rc::new(CollapsedViewPulse { finished: resolver })), None);
        finished
    }
}

impl IInlineEditsView for InlineEditsCollapsedView {
    fn is_hovered(&self) -> IObservable<bool> {
        Rc::clone(&self.is_hovered)
    }

    fn on_did_click(&self) -> Event<InlineEditClickEvent> {
        Event::none()
    }
}

impl IDisposable for InlineEditsCollapsedView {
    fn dispose(&self) {
        self.base.dispose();
    }
}
