//! Porte de `AttachedViews`, `AttachedViewState` e `AttachedViewHandler` de
//! `src/vs/editor/common/model/tokens/abstractSyntaxTokenBackend.ts`: os editores (views) ligados a um
//! modelo e as linhas visíveis de cada um.
//!
//! Divergência: o original expõe `visibleLineRanges` como um observável derivado; nenhum consumidor do
//! porte (só o tokenizador por gramática usa o evento) precisa dele, então fica como método
//! [`AttachedViews::visible_line_ranges`] que recalcula.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::async_::RunOnceScheduler;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;

use crate::line_range::LineRange;

/// `AttachedViewState`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachedViewState {
    pub visible_line_ranges: Vec<LineRange>,
    pub stabilized: bool,
}

/// O que um view faz quando o seu estado muda: avisa o `AttachedViews` que o criou.
type StateChangeHandler = Box<dyn Fn(&Rc<AttachedView>, AttachedViewState)>;

/// `IAttachedView`.
pub struct AttachedView {
    id: u64,
    state: RefCell<Option<AttachedViewState>>,
    handle_state_change: StateChangeHandler,
}

impl AttachedView {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn state(&self) -> Option<AttachedViewState> {
        self.state.borrow().clone()
    }

    /// `setVisibleLines(visibleLines, stabilized)`: `visible_lines` são pares `(startLineNumber, endLineNumber)`.
    pub fn set_visible_lines(self: &Rc<Self>, visible_lines: &[(i32, i32)], stabilized: bool) {
        let visible_line_ranges = visible_lines.iter().map(|&(start, end)| LineRange::new(start, end + 1)).collect();
        let state = AttachedViewState { visible_line_ranges, stabilized };
        *self.state.borrow_mut() = Some(state.clone());
        (self.handle_state_change)(self, state);
    }
}

/// `AttachedViews`.
pub struct AttachedViews {
    on_did_change_visible_ranges: Rc<Emitter<(Rc<AttachedView>, Option<AttachedViewState>)>>,
    views: RefCell<Vec<Rc<AttachedView>>>,
    next_id: Cell<u64>,
}

impl AttachedViews {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { on_did_change_visible_ranges: Emitter::new(), views: RefCell::new(Vec::new()), next_id: Cell::new(1) })
    }

    pub fn on_did_change_visible_ranges(&self) -> Event<(Rc<AttachedView>, Option<AttachedViewState>)> {
        self.on_did_change_visible_ranges.event()
    }

    /// `visibleLineRanges`: a união das linhas visíveis de todos os views.
    pub fn visible_line_ranges(&self) -> Vec<LineRange> {
        let ranges: Vec<Vec<LineRange>> =
            self.views.borrow().iter().map(|v| v.state().map(|s| s.visible_line_ranges).unwrap_or_default()).collect();
        LineRange::join_many(&ranges)
    }

    pub fn attach_view(self: &Rc<Self>) -> Rc<AttachedView> {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let emitter = self.on_did_change_visible_ranges.clone();
        let view = Rc::new(AttachedView {
            id,
            state: RefCell::new(None),
            handle_state_change: Box::new(move |view, state| emitter.fire((view.clone(), Some(state)))),
        });
        self.views.borrow_mut().push(view.clone());
        view
    }

    pub fn detach_view(&self, view: &Rc<AttachedView>) {
        self.views.borrow_mut().retain(|v| v.id != view.id);
        self.on_did_change_visible_ranges.fire((view.clone(), None));
    }
}

impl IDisposable for AttachedViews {
    fn dispose(&self) {
        IDisposable::dispose(&*self.on_did_change_visible_ranges);
    }
}

/// `AttachedViewHandler`.
pub struct AttachedViewHandler {
    runner: Rc<RunOnceScheduler>,
    computed_line_ranges: Rc<RefCell<Vec<LineRange>>>,
    line_ranges: Rc<RefCell<Vec<LineRange>>>,
    refresh_tokens: Rc<dyn Fn()>,
}

impl AttachedViewHandler {
    pub fn new(refresh_tokens: impl Fn() + 'static) -> Rc<Self> {
        let refresh_tokens: Rc<dyn Fn()> = Rc::new(refresh_tokens);
        let computed_line_ranges = Rc::new(RefCell::new(Vec::new()));
        let line_ranges = Rc::new(RefCell::new(Vec::new()));
        let (c, l, r) = (computed_line_ranges.clone(), line_ranges.clone(), refresh_tokens.clone());
        let runner = RunOnceScheduler::new(move || Self::update(&c, &l, &r), 50);
        Rc::new(Self { runner, computed_line_ranges, line_ranges, refresh_tokens })
    }

    pub fn line_ranges(&self) -> Vec<LineRange> {
        self.line_ranges.borrow().clone()
    }

    fn update(computed: &Rc<RefCell<Vec<LineRange>>>, current: &Rc<RefCell<Vec<LineRange>>>, refresh: &Rc<dyn Fn()>) {
        if *computed.borrow() == *current.borrow() {
            return;
        }
        *computed.borrow_mut() = current.borrow().clone();
        refresh();
    }

    pub fn handle_state_change(&self, state: &AttachedViewState) {
        *self.line_ranges.borrow_mut() = state.visible_line_ranges.clone();
        if state.stabilized {
            self.runner.cancel();
            Self::update(&self.computed_line_ranges, &self.line_ranges, &self.refresh_tokens);
        } else {
            self.runner.schedule(None);
        }
    }
}

impl IDisposable for AttachedViewHandler {
    fn dispose(&self) {
        IDisposable::dispose(&*self.runner);
    }
}
