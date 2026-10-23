//! Porte de `src/vs/base/common/scrollable.ts`: o modelo de rolagem (dimensões, posição validada) e a
//! rolagem suave com animação.
//!
//! Decisões de tradução:
//! - Os números são `f64` (o `number` do JavaScript). `forceIntegerValues` aplica o `| 0` do original
//!   (`ToInt32`: trunca e dá a volta em 32 bits, `NaN` e infinito viram `0`).
//! - Os parâmetros opcionais (`INewScrollDimensions`, `INewScrollPosition`) são `Option<f64>`, e o
//!   `typeof x !== 'undefined'` do original é `is_some()`.
//! - `IScrollDimensions`, `IScrollPosition` e `ISmoothScrollPosition` são structs de valor (`Copy`). O
//!   `ScrollState`, que implementa as duas primeiras no original, converte para elas com `From`, e
//!   `getScrollDimensions`/`getCurrentScrollPosition` devolvem a cópia do estado atual (o `ScrollState` é
//!   imutável no original, então a cópia é indistinguível).
//! - `Scrollable` é um [`Disposable`] com o `Emitter` de `onScroll` registrado, e vive num `Rc`. O callback
//!   do quadro de animação guarda só um `Weak` do `Scrollable`, então um agendador que o retém não segura
//!   o objeto vivo.
//! - `scheduleAtNextAnimationFrame` é `Rc<dyn Fn(Box<dyn FnOnce()>) -> DisposableRef>`.
//! - `SmoothScrollingOperation._tick(now)` é protegido no original (o teste herda para chamá-lo) e aqui é
//!   [`SmoothScrollingOperation::tick_at`]; `tick()` lê o `Date.now()` do [`runtime`](crate::runtime).
//! - As curvas de animação (`IAnimation`, `createEaseOutCubic`, `createComposed`) são o `enum` privado
//!   `Animation`. A chamada `Math.pow(t, 3)` é `powf(3.0)`, que pode diferir do `pow` do V8 no último bit.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use crate::event::{Emitter, Event};
use crate::lifecycle::{Disposable, DisposableRef, IDisposable};
use crate::runtime;

/// `ScrollbarVisibility` (um `const enum`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ScrollbarVisibility {
    Auto = 1,
    Hidden = 2,
    Visible = 3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollEvent {
    pub in_smooth_scrolling: bool,

    pub old_width: f64,
    pub old_scroll_width: f64,
    pub old_scroll_left: f64,

    pub width: f64,
    pub scroll_width: f64,
    pub scroll_left: f64,

    pub old_height: f64,
    pub old_scroll_height: f64,
    pub old_scroll_top: f64,

    pub height: f64,
    pub scroll_height: f64,
    pub scroll_top: f64,

    pub width_changed: bool,
    pub scroll_width_changed: bool,
    pub scroll_left_changed: bool,

    pub height_changed: bool,
    pub scroll_height_changed: bool,
    pub scroll_top_changed: bool,
}

/// `value | 0`.
fn to_int32(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    f64::from(value.trunc().rem_euclid(4_294_967_296.0) as u32 as i32)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollState {
    force_integer_values: bool,

    pub raw_scroll_left: f64,
    pub raw_scroll_top: f64,

    pub width: f64,
    pub scroll_width: f64,
    pub scroll_left: f64,
    pub height: f64,
    pub scroll_height: f64,
    pub scroll_top: f64,
}

impl ScrollState {
    pub fn new(force_integer_values: bool, mut width: f64, mut scroll_width: f64, mut scroll_left: f64, mut height: f64, mut scroll_height: f64, mut scroll_top: f64) -> Self {
        if force_integer_values {
            width = to_int32(width);
            scroll_width = to_int32(scroll_width);
            scroll_left = to_int32(scroll_left);
            height = to_int32(height);
            scroll_height = to_int32(scroll_height);
            scroll_top = to_int32(scroll_top);
        }

        let raw_scroll_left = scroll_left; // before validation
        let raw_scroll_top = scroll_top; // before validation

        if width < 0.0 {
            width = 0.0;
        }
        if scroll_left + width > scroll_width {
            scroll_left = scroll_width - width;
        }
        if scroll_left < 0.0 {
            scroll_left = 0.0;
        }

        if height < 0.0 {
            height = 0.0;
        }
        if scroll_top + height > scroll_height {
            scroll_top = scroll_height - height;
        }
        if scroll_top < 0.0 {
            scroll_top = 0.0;
        }

        Self { force_integer_values, raw_scroll_left, raw_scroll_top, width, scroll_width, scroll_left, height, scroll_height, scroll_top }
    }

    /// `equals(other)`: não olha o `forceIntegerValues`, como o original.
    pub fn equals(&self, other: &ScrollState) -> bool {
        self.raw_scroll_left == other.raw_scroll_left
            && self.raw_scroll_top == other.raw_scroll_top
            && self.width == other.width
            && self.scroll_width == other.scroll_width
            && self.scroll_left == other.scroll_left
            && self.height == other.height
            && self.scroll_height == other.scroll_height
            && self.scroll_top == other.scroll_top
    }

    pub fn with_scroll_dimensions(&self, update: &INewScrollDimensions, use_raw_scroll_positions: bool) -> ScrollState {
        ScrollState::new(
            self.force_integer_values,
            update.width.unwrap_or(self.width),
            update.scroll_width.unwrap_or(self.scroll_width),
            if use_raw_scroll_positions { self.raw_scroll_left } else { self.scroll_left },
            update.height.unwrap_or(self.height),
            update.scroll_height.unwrap_or(self.scroll_height),
            if use_raw_scroll_positions { self.raw_scroll_top } else { self.scroll_top },
        )
    }

    pub fn with_scroll_position(&self, update: &INewScrollPosition) -> ScrollState {
        ScrollState::new(
            self.force_integer_values,
            self.width,
            self.scroll_width,
            update.scroll_left.unwrap_or(self.raw_scroll_left),
            self.height,
            self.scroll_height,
            update.scroll_top.unwrap_or(self.raw_scroll_top),
        )
    }

    pub fn create_scroll_event(&self, previous: &ScrollState, in_smooth_scrolling: bool) -> ScrollEvent {
        ScrollEvent {
            in_smooth_scrolling,
            old_width: previous.width,
            old_scroll_width: previous.scroll_width,
            old_scroll_left: previous.scroll_left,

            width: self.width,
            scroll_width: self.scroll_width,
            scroll_left: self.scroll_left,

            old_height: previous.height,
            old_scroll_height: previous.scroll_height,
            old_scroll_top: previous.scroll_top,

            height: self.height,
            scroll_height: self.scroll_height,
            scroll_top: self.scroll_top,

            width_changed: self.width != previous.width,
            scroll_width_changed: self.scroll_width != previous.scroll_width,
            scroll_left_changed: self.scroll_left != previous.scroll_left,

            height_changed: self.height != previous.height,
            scroll_height_changed: self.scroll_height != previous.scroll_height,
            scroll_top_changed: self.scroll_top != previous.scroll_top,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IScrollDimensions {
    pub width: f64,
    pub scroll_width: f64,
    pub height: f64,
    pub scroll_height: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct INewScrollDimensions {
    pub width: Option<f64>,
    pub scroll_width: Option<f64>,
    pub height: Option<f64>,
    pub scroll_height: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IScrollPosition {
    pub scroll_left: f64,
    pub scroll_top: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ISmoothScrollPosition {
    pub scroll_left: f64,
    pub scroll_top: f64,

    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct INewScrollPosition {
    pub scroll_left: Option<f64>,
    pub scroll_top: Option<f64>,
}

impl From<&ScrollState> for IScrollDimensions {
    fn from(state: &ScrollState) -> Self {
        Self { width: state.width, scroll_width: state.scroll_width, height: state.height, scroll_height: state.scroll_height }
    }
}

impl From<&ScrollState> for IScrollPosition {
    fn from(state: &ScrollState) -> Self {
        Self { scroll_left: state.scroll_left, scroll_top: state.scroll_top }
    }
}

impl From<&ScrollState> for ISmoothScrollPosition {
    fn from(state: &ScrollState) -> Self {
        Self { scroll_left: state.scroll_left, scroll_top: state.scroll_top, width: state.width, height: state.height }
    }
}

/// A função que agenda um callback para o próximo quadro de animação e devolve como cancelá-lo.
pub type ScheduleAtNextAnimationFrame = Rc<dyn Fn(Box<dyn FnOnce()>) -> DisposableRef>;

#[derive(Clone)]
pub struct IScrollableOptions {
    /// Define if the scroll values should always be integers.
    pub force_integer_values: bool,
    /// Set the duration (ms) used for smooth scroll animations.
    pub smooth_scroll_duration: f64,
    /// A function to schedule an update at the next frame (used for smooth scroll animations).
    pub schedule_at_next_animation_frame: ScheduleAtNextAnimationFrame,
}

pub struct Scrollable {
    base: Rc<Disposable>,
    me: Weak<Scrollable>,

    smooth_scroll_duration: Cell<f64>,
    schedule_at_next_animation_frame: ScheduleAtNextAnimationFrame,
    state: Cell<ScrollState>,
    smooth_scrolling: RefCell<Option<SmoothScrollingOperation>>,

    on_scroll: Rc<Emitter<ScrollEvent>>,
}

impl Scrollable {
    pub fn new(options: IScrollableOptions) -> Rc<Self> {
        let base = Disposable::new();
        let on_scroll = Emitter::new();
        base.register(on_scroll.clone());
        Rc::new_cyclic(|me| Self {
            base,
            me: me.clone(),
            smooth_scroll_duration: Cell::new(options.smooth_scroll_duration),
            schedule_at_next_animation_frame: options.schedule_at_next_animation_frame,
            state: Cell::new(ScrollState::new(options.force_integer_values, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0)),
            smooth_scrolling: RefCell::new(None),
            on_scroll,
        })
    }

    pub fn on_scroll(&self) -> Event<ScrollEvent> {
        self.on_scroll.event()
    }

    pub fn set_smooth_scroll_duration(&self, smooth_scroll_duration: f64) {
        self.smooth_scroll_duration.set(smooth_scroll_duration);
    }

    pub fn validate_scroll_position(&self, scroll_position: &INewScrollPosition) -> IScrollPosition {
        IScrollPosition::from(&self.state.get().with_scroll_position(scroll_position))
    }

    pub fn get_scroll_dimensions(&self) -> IScrollDimensions {
        IScrollDimensions::from(&self.state.get())
    }

    pub fn set_scroll_dimensions(&self, dimensions: &INewScrollDimensions, use_raw_scroll_positions: bool) {
        let new_state = self.state.get().with_scroll_dimensions(dimensions, use_raw_scroll_positions);
        let in_smooth_scrolling = self.smooth_scrolling.borrow().is_some();
        self.set_state(new_state, in_smooth_scrolling);

        // Validate outstanding animated scroll position target
        let state = self.state.get();
        if let Some(smooth_scrolling) = self.smooth_scrolling.borrow_mut().as_mut() {
            smooth_scrolling.accept_scroll_dimensions(&state);
        }
    }

    /// Returns the final scroll position that the instance will have once the smooth scroll animation concludes.
    /// If no scroll animation is occurring, it will return the current scroll position instead.
    pub fn get_future_scroll_position(&self) -> IScrollPosition {
        match self.smooth_scrolling.borrow().as_ref() {
            Some(smooth_scrolling) => IScrollPosition { scroll_left: smooth_scrolling.to.scroll_left, scroll_top: smooth_scrolling.to.scroll_top },
            None => IScrollPosition::from(&self.state.get()),
        }
    }

    /// Returns the current scroll position.
    /// Note: This result might be an intermediate scroll position, as there might be an ongoing smooth scroll animation.
    pub fn get_current_scroll_position(&self) -> IScrollPosition {
        IScrollPosition::from(&self.state.get())
    }

    pub fn set_scroll_position_now(&self, update: &INewScrollPosition) {
        // no smooth scrolling requested
        let new_state = self.state.get().with_scroll_position(update);

        // Terminate any outstanding smooth scrolling
        self.terminate_smooth_scrolling();

        self.set_state(new_state, false);
    }

    /// `setScrollPositionSmooth(update, reuseAnimation)`; `reuse_animation` ausente é `false`.
    pub fn set_scroll_position_smooth(&self, update: &INewScrollPosition, reuse_animation: bool) {
        if self.smooth_scroll_duration.get() == 0.0 {
            // Smooth scrolling not supported.
            return self.set_scroll_position_now(update);
        }

        let state = self.state.get();
        let new_smooth_scrolling = {
            let current = self.smooth_scrolling.borrow();
            match current.as_ref() {
                Some(smooth_scrolling) => {
                    // Combine our pending scrollLeft/scrollTop with incoming scrollLeft/scrollTop
                    let update = INewScrollPosition {
                        scroll_left: Some(update.scroll_left.unwrap_or(smooth_scrolling.to.scroll_left)),
                        scroll_top: Some(update.scroll_top.unwrap_or(smooth_scrolling.to.scroll_top)),
                    };

                    // Validate `update`
                    let valid_target = state.with_scroll_position(&update);

                    if smooth_scrolling.to.scroll_left == valid_target.scroll_left && smooth_scrolling.to.scroll_top == valid_target.scroll_top {
                        // No need to interrupt or extend the current animation since we're going to the same place
                        return;
                    }
                    if reuse_animation {
                        SmoothScrollingOperation::new(smooth_scrolling.from, ISmoothScrollPosition::from(&valid_target), smooth_scrolling.start_time, smooth_scrolling.duration)
                    } else {
                        smooth_scrolling.combine(&ISmoothScrollPosition::from(&state), &ISmoothScrollPosition::from(&valid_target), self.smooth_scroll_duration.get())
                    }
                }
                None => {
                    // Validate `update`
                    let valid_target = state.with_scroll_position(update);

                    SmoothScrollingOperation::start(&ISmoothScrollPosition::from(&state), &ISmoothScrollPosition::from(&valid_target), self.smooth_scroll_duration.get())
                }
            }
        };
        self.terminate_smooth_scrolling();
        *self.smooth_scrolling.borrow_mut() = Some(new_smooth_scrolling);

        // Begin smooth scrolling animation
        self.schedule_next_frame();
    }

    pub fn has_pending_scroll_animation(&self) -> bool {
        self.smooth_scrolling.borrow().is_some()
    }

    /// Descarta a rolagem suave em andamento, se houver.
    fn terminate_smooth_scrolling(&self) {
        let smooth_scrolling = self.smooth_scrolling.borrow_mut().take();
        if let Some(mut smooth_scrolling) = smooth_scrolling {
            smooth_scrolling.dispose();
        }
    }

    /// Agenda o próximo passo da animação e guarda como cancelá-lo na rolagem suave atual.
    fn schedule_next_frame(&self) {
        let me = self.me.clone();
        let disposable = (self.schedule_at_next_animation_frame)(Box::new(move || {
            let Some(this) = me.upgrade() else {
                return;
            };
            match this.smooth_scrolling.borrow_mut().as_mut() {
                Some(smooth_scrolling) => smooth_scrolling.animation_frame_disposable = None,
                None => return,
            }
            this.perform_smooth_scrolling();
        }));
        if let Some(smooth_scrolling) = self.smooth_scrolling.borrow_mut().as_mut() {
            smooth_scrolling.animation_frame_disposable = Some(disposable);
        }
    }

    fn perform_smooth_scrolling(&self) {
        let update = match self.smooth_scrolling.borrow().as_ref() {
            Some(smooth_scrolling) => smooth_scrolling.tick(),
            None => return,
        };
        let new_state = self.state.get().with_scroll_position(&INewScrollPosition { scroll_left: Some(update.scroll_left), scroll_top: Some(update.scroll_top) });

        self.set_state(new_state, true);

        if self.smooth_scrolling.borrow().is_none() {
            // Looks like someone canceled the smooth scrolling
            // from the scroll event handler
            return;
        }

        if update.is_done {
            self.terminate_smooth_scrolling();
            return;
        }

        // Continue smooth scrolling animation
        self.schedule_next_frame();
    }

    fn set_state(&self, new_state: ScrollState, in_smooth_scrolling: bool) {
        let old_state = self.state.get();
        if old_state.equals(&new_state) {
            // no change
            return;
        }
        self.state.set(new_state);
        self.on_scroll.fire(new_state.create_scroll_event(&old_state, in_smooth_scrolling));
    }
}

impl IDisposable for Scrollable {
    fn dispose(&self) {
        self.terminate_smooth_scrolling();
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "Scrollable"
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmoothScrollingUpdate {
    pub scroll_left: f64,
    pub scroll_top: f64,
    pub is_done: bool,
}

/// `easeInCubic(t)`.
fn ease_in_cubic(t: f64) -> f64 {
    t.powf(3.0)
}

/// `easeOutCubic(t)`.
fn ease_out_cubic(t: f64) -> f64 {
    1.0 - ease_in_cubic(1.0 - t)
}

/// `createEaseOutCubic(from, to)`.
#[derive(Clone, Copy)]
struct EaseOutCubic {
    from: f64,
    delta: f64,
}

impl EaseOutCubic {
    fn new(from: f64, to: f64) -> Self {
        Self { from, delta: to - from }
    }

    fn at(&self, completion: f64) -> f64 {
        self.from + self.delta * ease_out_cubic(completion)
    }
}

/// `IAnimation`: uma curva simples ou a composição de duas cortada em `cut`.
#[derive(Clone, Copy)]
enum Animation {
    Simple(EaseOutCubic),
    /// `createComposed(a, b, cut)`.
    Composed(EaseOutCubic, EaseOutCubic, f64),
}

impl Animation {
    fn at(&self, completion: f64) -> f64 {
        match *self {
            Animation::Simple(animation) => animation.at(completion),
            Animation::Composed(a, b, cut) => {
                if completion < cut {
                    return a.at(completion / cut);
                }
                b.at((completion - cut) / (1.0 - cut))
            }
        }
    }

    fn init(from: f64, to: f64, viewport_size: f64) -> Animation {
        let delta = (from - to).abs();
        if delta > 2.5 * viewport_size {
            let (stop1, stop2) = if from < to {
                // scroll to 75% of the viewportSize
                (from + 0.75 * viewport_size, to - 0.75 * viewport_size)
            } else {
                (from - 0.75 * viewport_size, to + 0.75 * viewport_size)
            };
            return Animation::Composed(EaseOutCubic::new(from, stop1), EaseOutCubic::new(stop2, to), 0.33);
        }
        Animation::Simple(EaseOutCubic::new(from, to))
    }
}

pub struct SmoothScrollingOperation {
    pub from: ISmoothScrollPosition,
    pub to: ISmoothScrollPosition,
    pub duration: f64,
    pub start_time: f64,
    pub animation_frame_disposable: Option<DisposableRef>,

    scroll_left: Animation,
    scroll_top: Animation,
}

impl SmoothScrollingOperation {
    pub fn new(from: ISmoothScrollPosition, to: ISmoothScrollPosition, start_time: f64, duration: f64) -> Self {
        let (scroll_left, scroll_top) = Self::init_animations(&from, &to);
        Self { from, to, duration, start_time, animation_frame_disposable: None, scroll_left, scroll_top }
    }

    fn init_animations(from: &ISmoothScrollPosition, to: &ISmoothScrollPosition) -> (Animation, Animation) {
        (Animation::init(from.scroll_left, to.scroll_left, to.width), Animation::init(from.scroll_top, to.scroll_top, to.height))
    }

    pub fn dispose(&mut self) {
        if let Some(animation_frame_disposable) = self.animation_frame_disposable.take() {
            animation_frame_disposable.dispose();
        }
    }

    pub fn accept_scroll_dimensions(&mut self, state: &ScrollState) {
        let to = state.with_scroll_position(&INewScrollPosition { scroll_left: Some(self.to.scroll_left), scroll_top: Some(self.to.scroll_top) });
        self.to = ISmoothScrollPosition::from(&to);
        (self.scroll_left, self.scroll_top) = Self::init_animations(&self.from, &self.to);
    }

    /// `tick()`: o passo da animação no `Date.now()` atual.
    pub fn tick(&self) -> SmoothScrollingUpdate {
        self.tick_at(runtime::date_now() as f64)
    }

    /// `_tick(now)`: o passo da animação no instante `now` (ms), exposto para os testes.
    pub fn tick_at(&self, now: f64) -> SmoothScrollingUpdate {
        let completion = (now - self.start_time) / self.duration;

        if completion < 1.0 {
            let new_scroll_left = self.scroll_left.at(completion);
            let new_scroll_top = self.scroll_top.at(completion);
            return SmoothScrollingUpdate { scroll_left: new_scroll_left, scroll_top: new_scroll_top, is_done: false };
        }

        SmoothScrollingUpdate { scroll_left: self.to.scroll_left, scroll_top: self.to.scroll_top, is_done: true }
    }

    pub fn combine(&self, from: &ISmoothScrollPosition, to: &ISmoothScrollPosition, duration: f64) -> SmoothScrollingOperation {
        SmoothScrollingOperation::start(from, to, duration)
    }

    pub fn start(from: &ISmoothScrollPosition, to: &ISmoothScrollPosition, duration: f64) -> SmoothScrollingOperation {
        // +10 / -10 : pretend the animation already started for a quicker response to a scroll request
        let duration = duration + 10.0;
        let start_time = runtime::date_now() as f64 - 10.0;

        SmoothScrollingOperation::new(*from, *to, start_time, duration)
    }
}
