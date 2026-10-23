//! Porte de `src/vs/base/browser/ui/progressbar/progressbar.ts`, `progressbar.css` e
//! `progressAccessibilitySignal.ts`: a [`ProgressBar`], a linha fina de 2 px (determinada ou infinita) que
//! aparece sob um título ou uma visão.
//!
//! Do DOM e do CSS para o `je-ui`:
//!
//! - O `.monaco-progress-container` (role `progressbar`, `width: 100%`, `height: 2px`, `overflow: hidden`) é um nó
//!   com as mesmas classes (`active`, `infinite`, `infinite-long-running`, `discrete`, `done`) e o
//!   `.progress-bit` é o filho. `display: none` é `visible = false`; o `display: none` do `.progress-bit` sem
//!   `.active` também.
//! - A animação `progress` do CSS (`4s linear infinite`, com `translateX(0 -> 2500% -> 4900%)` e `scaleX(1 -> 3
//!   -> 1)` em 0%, 50% e 100%, e `steps(100)` com `.infinite-long-running`) é [`ProgressBar::refresh`] chamada a
//!   cada quadro com `Ui::schedule_at_next_animation_frame`, a fase vindo do relógio do loop de eventos
//!   (`je_base::runtime::now`), o que a torna determinística com o relógio virtual dos testes. A
//!   `transition: width 100ms linear` do `.discrete` é instantânea (divergência).
//! - `aria-valuemin/max/now` não existem na árvore: ficam em [`ProgressBar::aria_value_max`] e
//!   [`ProgressBar::aria_value_now`]; o `aria-label` é o rótulo do nó.
//! - O sinal de acessibilidade de progresso (`progressAccessibilitySignal.ts`) é o par
//!   [`set_progress_accessibility_signal_scheduler`]/[`get_progress_accessibility_signal_scheduler`]; o padrão
//!   não faz nada.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::RunOnceScheduler;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, MutableDisposable, none};
use je_base::nls::localize;
use je_base::runtime;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{NodeId, Role, Ui};

use crate::dom::{create_el, hide, show};

const CSS_DONE: &str = "done";
const CSS_ACTIVE: &str = "active";
const CSS_INFINITE: &str = "infinite";
const CSS_INFINITE_LONG_RUNNING: &str = "infinite-long-running";
const CSS_DISCRETE: &str = "discrete";

/// A cor do `.progress-bit` quando o estilo não define uma (`'#0E70C0'`).
const DEFAULT_BIT_COLOR: Rgba = Rgba::opaque(0x0E, 0x70, 0xC0);
/// A altura do contêiner e do `.progress-bit`.
const HEIGHT: f64 = 2.0;
/// A duração do ciclo da animação `progress`, em milissegundos.
const ANIMATION_DURATION: u64 = 4000;
/// O tempo até o infinito virar `infinite-long-running`.
const LONG_RUNNING_INFINITE_THRESHOLD: u64 = 10000;
/// O atraso padrão do sinal de acessibilidade.
const PROGRESS_SIGNAL_DEFAULT_DELAY: u64 = 3000;

type SignalFactory = Rc<dyn Fn(u64, Option<u64>) -> DisposableRef>;

thread_local! {
    static SIGNAL_FACTORY: RefCell<SignalFactory> = RefCell::new(Rc::new(|_, _| none()));
}

/// `setProgressAccessibilitySignalScheduler`.
pub fn set_progress_accessibility_signal_scheduler(factory: impl Fn(u64, Option<u64>) -> DisposableRef + 'static) {
    SIGNAL_FACTORY.with(|f| *f.borrow_mut() = Rc::new(factory));
}

/// `getProgressAccessibilitySignalScheduler(msDelayTime, msLoopTime)`.
pub fn get_progress_accessibility_signal_scheduler(ms_delay_time: u64, ms_loop_time: Option<u64>) -> DisposableRef {
    let factory = SIGNAL_FACTORY.with(|f| f.borrow().clone());
    factory(ms_delay_time, ms_loop_time)
}

/// O `IProgressBarStyles`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IProgressBarStyles {
    pub progress_bar_background: Option<Rgba>,
}

/// `defaultProgressBarStyles`: a cor `progressBar.background`.
pub fn default_progress_bar_styles(theme: &ColorTheme) -> IProgressBarStyles {
    IProgressBarStyles { progress_bar_background: theme.get_color("progressBar.background") }
}

/// O `IProgressBarOptions` (estende os estilos).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IProgressBarOptions {
    pub styles: IProgressBarStyles,
    pub aria_label: Option<String>,
}

/// `unthemedProgressBarOptions`.
pub fn unthemed_progress_bar_options() -> IProgressBarOptions {
    IProgressBarOptions::default()
}

/// O `style.width` do `.progress-bit`: `inherit` (o contêiner inteiro) ou uma porcentagem.
#[derive(Clone, Copy, Debug, PartialEq)]
enum BitWidth {
    Inherit,
    Percent(f64),
}

pub struct ProgressBar {
    base: Rc<Disposable>,
    weak_self: Weak<ProgressBar>,
    ui: Ui,
    element: NodeId,
    bit: NodeId,
    worked_val: Cell<f64>,
    total_work: Cell<Option<f64>>,
    aria_value_max: Cell<Option<f64>>,
    aria_value_now: Cell<Option<f64>>,
    bit_width: Cell<BitWidth>,
    bit_opacity: Cell<f32>,
    show_delayed_scheduler: Rc<RunOnceScheduler>,
    long_running_scheduler: Rc<RunOnceScheduler>,
    progress_signal: Rc<MutableDisposable<dyn IDisposable>>,
    /// Quando a animação `progress` começou (`runtime::now`).
    animation_start: Cell<u64>,
    frame: RefCell<Option<DisposableRef>>,
    disposed: Cell<bool>,
}

impl ProgressBar {
    /// Cria a barra como último filho de `container`.
    pub fn new(ui: &Ui, container: NodeId, options: Option<&IProgressBarOptions>) -> Rc<Self> {
        let aria_label = options.and_then(|o| o.aria_label.as_deref()).map(str::trim).filter(|l| !l.is_empty()).map_or_else(|| localize("progress", "Progress", &[]), str::to_owned);
        let (element, bit) = {
            let mut tree = ui.tree_mut();
            let element = create_el(&mut tree, container, Role::ProgressBar, "monaco-progress-container");
            tree.set_label(element, aria_label);
            tree.set_fill(element, true, false);
            tree.style_mut(element).clip = true;
            let origin = tree.bounds(element);
            tree.set_bounds(element, Rect::new(origin.x0, origin.y0, origin.x0, origin.y0 + HEIGHT));
            let bit = create_el(&mut tree, element, Role::Generic, "progress-bit");
            let style = tree.style_mut(bit);
            style.background = Some(options.and_then(|o| o.styles.progress_bar_background).unwrap_or(DEFAULT_BIT_COLOR));
            style.visible = false;
            (element, bit)
        };

        let base = Disposable::new();
        let progress_signal = MutableDisposable::<dyn IDisposable>::new();
        base.register(progress_signal.clone());
        let this = Rc::new_cyclic(|weak: &Weak<ProgressBar>| {
            let (w1, w2) = (weak.clone(), weak.clone());
            let ui_for_show = ui.clone();
            let show_delayed_scheduler = RunOnceScheduler::new(
                move || {
                    if w1.upgrade().is_some() {
                        show(&mut ui_for_show.tree_mut(), element);
                    }
                },
                0,
            );
            let long_running_scheduler = RunOnceScheduler::new(
                move || {
                    if let Some(bar) = w2.upgrade() {
                        bar.infinite_long_running();
                    }
                },
                LONG_RUNNING_INFINITE_THRESHOLD,
            );
            base.register(show_delayed_scheduler.clone());
            base.register(long_running_scheduler.clone());
            Self {
                base,
                weak_self: weak.clone(),
                ui: ui.clone(),
                element,
                bit,
                worked_val: Cell::new(0.0),
                total_work: Cell::new(None),
                aria_value_max: Cell::new(None),
                aria_value_now: Cell::new(None),
                bit_width: Cell::new(BitWidth::Percent(2.0)),
                bit_opacity: Cell::new(1.0),
                show_delayed_scheduler,
                long_running_scheduler,
                progress_signal,
                animation_start: Cell::new(0),
                frame: RefCell::new(None),
                disposed: Cell::new(false),
            }
        });
        this.refresh();
        this
    }

    fn has_class(&self, class: &str) -> bool {
        self.ui.tree().has_class(self.element, class)
    }

    fn remove_classes(&self, classes: &[&str]) {
        let mut tree = self.ui.tree_mut();
        for class in classes {
            tree.remove_class(self.element, class);
        }
    }

    fn add_classes(&self, classes: &[&str]) {
        let mut tree = self.ui.tree_mut();
        for class in classes {
            tree.add_class(self.element, class);
        }
    }

    fn off(&self) {
        self.bit_width.set(BitWidth::Inherit);
        self.bit_opacity.set(1.0);
        self.remove_classes(&[CSS_ACTIVE, CSS_INFINITE, CSS_INFINITE_LONG_RUNNING, CSS_DISCRETE]);

        self.worked_val.set(0.0);
        self.total_work.set(None);

        self.long_running_scheduler.cancel();
        self.progress_signal.clear();
        self.refresh();
    }

    /// `done()`: termina e some depois de 200 ms.
    pub fn done(&self) -> &Self {
        self.do_done(true)
    }

    /// `stop()`: termina e some na hora.
    pub fn stop(&self) -> &Self {
        self.do_done(false)
    }

    fn do_done(&self, delayed: bool) -> &Self {
        self.add_classes(&[CSS_DONE]);

        if !self.has_class(CSS_INFINITE) {
            self.bit_width.set(BitWidth::Inherit);
        } else {
            self.bit_opacity.set(0.0);
        }
        if delayed {
            let weak = self.weak_self.clone();
            runtime::set_timeout(
                move || {
                    if let Some(bar) = weak.upgrade().filter(|b| !b.disposed.get()) {
                        bar.off();
                    }
                },
                200,
            );
            self.refresh();
        } else {
            self.off();
        }
        self
    }

    /// `infinite()`: a barra que vai e volta sem fim.
    pub fn infinite(&self) -> &Self {
        self.bit_width.set(BitWidth::Percent(2.0));
        self.bit_opacity.set(1.0);

        let was_infinite = self.has_class(CSS_INFINITE);
        self.remove_classes(&[CSS_DISCRETE, CSS_DONE, CSS_INFINITE_LONG_RUNNING]);
        self.add_classes(&[CSS_ACTIVE, CSS_INFINITE]);
        if !was_infinite {
            self.animation_start.set(runtime::now());
        }

        self.long_running_scheduler.schedule(None);
        self.schedule_frame();
        self.refresh();
        self
    }

    fn infinite_long_running(&self) {
        self.add_classes(&[CSS_INFINITE_LONG_RUNNING]);
        self.refresh();
    }

    /// `total(value)`: o trabalho total, zerando o feito.
    pub fn total(&self, value: f64) -> &Self {
        self.worked_val.set(0.0);
        self.total_work.set(Some(value));
        self.aria_value_max.set(Some(value));
        self
    }

    /// `hasTotal()`.
    pub fn has_total(&self) -> bool {
        self.total_work.get().is_some()
    }

    /// `worked(value)`: soma ao feito (no mínimo 1).
    pub fn worked(&self, value: f64) -> &Self {
        self.do_set_worked(self.worked_val.get() + value.max(1.0))
    }

    /// `setWorked(value)`: o feito vira `value` (no mínimo 1).
    pub fn set_worked(&self, value: f64) -> &Self {
        self.do_set_worked(value.max(1.0))
    }

    fn do_set_worked(&self, value: f64) -> &Self {
        // `this.totalWork || 100`: zero também vale 100.
        let total_work = self.total_work.get().filter(|t| *t != 0.0).unwrap_or(100.0);

        self.worked_val.set(value.min(total_work));

        self.remove_classes(&[CSS_INFINITE, CSS_INFINITE_LONG_RUNNING, CSS_DONE]);
        self.add_classes(&[CSS_ACTIVE, CSS_DISCRETE]);
        // O `aria-valuenow` é o valor recebido, sem o limite do total.
        self.aria_value_now.set(Some(value));

        self.bit_width.set(BitWidth::Percent(100.0 * (self.worked_val.get() / total_work)));
        self.stop_frames();
        self.refresh();
        self
    }

    /// `getContainer()`.
    pub fn get_container(&self) -> NodeId {
        self.element
    }

    /// O `.progress-bit`.
    pub fn bit(&self) -> NodeId {
        self.bit
    }

    /// O `aria-valuemax`.
    pub fn aria_value_max(&self) -> Option<f64> {
        self.aria_value_max.get()
    }

    /// O `aria-valuenow`.
    pub fn aria_value_now(&self) -> Option<f64> {
        self.aria_value_now.get()
    }

    /// `show(delay)`: sem `delay` mostra na hora; com `delay`, depois dele.
    pub fn show(&self, delay: Option<u64>) {
        self.show_delayed_scheduler.cancel();
        self.progress_signal.set(Some(get_progress_accessibility_signal_scheduler(PROGRESS_SIGNAL_DEFAULT_DELAY, None)));

        match delay {
            Some(delay) => self.show_delayed_scheduler.schedule(Some(delay)),
            None => show(&mut self.ui.tree_mut(), self.element),
        }
    }

    pub fn hide(&self) {
        hide(&mut self.ui.tree_mut(), self.element);

        self.show_delayed_scheduler.cancel();
        self.progress_signal.clear();
    }

    /// O `scheduleAtNextAnimationFrame` da animação `progress`, enquanto a barra é infinita.
    fn schedule_frame(&self) {
        if self.frame.borrow().is_some() {
            return;
        }
        let weak = self.weak_self.clone();
        let disposable = self.ui.schedule_at_next_animation_frame(0, move || {
            let Some(bar) = weak.upgrade() else { return };
            bar.frame.borrow_mut().take();
            if !bar.disposed.get() && bar.has_class(CSS_INFINITE) {
                bar.refresh();
                bar.schedule_frame();
            }
        });
        *self.frame.borrow_mut() = Some(disposable);
    }

    fn stop_frames(&self) {
        let frame = self.frame.borrow_mut().take();
        if let Some(frame) = frame {
            frame.dispose();
        }
    }

    /// As regras de `progressbar.css` sobre o estado atual: o `.progress-bit` aparece com `.active`, tem a
    /// largura de `style.width` (ou a do contêiner) e, com `.infinite`, o `transform` do quadro da animação.
    pub fn refresh(&self) {
        let mut tree = self.ui.tree_mut();
        let container_width = tree.bounds(self.element).width();
        let active = tree.has_class(self.element, CSS_ACTIVE);
        let infinite = tree.has_class(self.element, CSS_INFINITE);
        let width = match self.bit_width.get() {
            BitWidth::Inherit => container_width,
            BitWidth::Percent(p) => container_width * p / 100.0,
        };
        let (left, shown) = if infinite {
            let steps = tree.has_class(self.element, CSS_INFINITE_LONG_RUNNING);
            let elapsed = runtime::now().saturating_sub(self.animation_start.get()) % ANIMATION_DURATION;
            let (translate, scale) = infinite_transform(elapsed as f64 / ANIMATION_DURATION as f64, steps);
            let center = width / 2.0 + translate / 100.0 * width;
            let half = scale * width / 2.0;
            (center - half, scale * width)
        } else {
            (0.0, width)
        };
        tree.set_bounds(self.bit, Rect::new(left, 0.0, left + shown, HEIGHT));
        let style = tree.style_mut(self.bit);
        style.visible = active;
        style.opacity = self.bit_opacity.get();
    }
}

/// O `@keyframes progress`: `translateX` (em % da largura do `.progress-bit`) e `scaleX` na fase `phase`
/// (0 a 1) da animação. Com `steps`, cada metade avança em 100 degraus (`steps(100)`).
fn infinite_transform(phase: f64, steps: bool) -> (f64, f64) {
    let (progress, from, to) = if phase < 0.5 { (phase / 0.5, (0.0, 1.0), (2500.0, 3.0)) } else { ((phase - 0.5) / 0.5, (2500.0, 3.0), (4900.0, 1.0)) };
    let progress = if steps { (progress * 100.0).floor() / 100.0 } else { progress };
    (from.0 + (to.0 - from.0) * progress, from.1 + (to.1 - from.1) * progress)
}

impl IDisposable for ProgressBar {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.stop_frames();
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ProgressBar"
    }
}
