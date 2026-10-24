//! Porte de `src/vs/workbench/services/progress/browser/progressIndicator.ts`: o indicador de progresso do editor
//! ([`EditorProgressIndicator`]), o indicador com escopo ([`ScopedProgressIndicator`], que guarda o progresso enquanto o escopo
//! está inativo e o repete quando ele volta a ficar ativo) e o escopo abstrato ([`AbstractProgressScope`]).
//!
//! Divergências:
//! - **`ProgressBar`.** Os indicadores só chamam `infinite`, `total`, `hasTotal`, `worked`, `stop`, `show` e `hide` da barra; no
//!   original o teste a substitui por uma subclasse (`TestProgressBar extends ProgressBar`). Aqui esses sete membros são o trait
//!   [`IProgressBar`], implementado pela [`ProgressBar`] de `je-widgets` e pelos dublês, e os indicadores recebem um
//!   `Rc<dyn IProgressBar>`. O encadeamento `progressBar.stop().hide()` são duas chamadas.
//! - **`IProgressIndicator` e `IProgressRunner`.** São os de `je_editor::code_action::platform` (os mesmos que o serviço de
//!   progresso usa); `show(infinite: true | total: number, delay?)` é o [`ProgressTotal`], e `emptyProgressRunner` (de
//!   `platform/progress/common/progress.ts`, ainda não portado) é [`empty_progress_runner`].
//! - **`showWhile(promise)`.** Recebe o `&dyn Thenable` do `je-base` e o adota numa `Promise<()>` que assenta quando a recebida
//!   assenta, de qualquer forma (o `catch` do original ignora a rejeição); `Promise.allSettled([promise, whilePromise])` é a
//!   promessa que assenta quando as duas assentaram, e `this.progressState.whilePromise === promise` é a identidade
//!   ([`Promise::ptr_eq`]) dessa promessa adotada.
//! - **`ProgressIndicatorState.While`.** O construtor do original é `(whilePromise, whileStart, whileDelay)` e o
//!   `ScopedProgressIndicator.showWhile` o chama como `(promise, delay || 0, Date.now())`: o porte mantém a ordem dos
//!   argumentos, então `while_start` guarda o atraso e `while_delay` guarda o `Date.now()` (o `remainingDelay` calculado em
//!   `onDidScopeActivate` sai o mesmo). `Date.now()` é `runtime::date_now`.
//! - **`IEditorGroupView`.** O grupo é o `Rc<dyn IEditorGroup>` do porte (que traz `onDidModelChange` e `isEmpty`); o ouvinte
//!   guarda uma referência fraca, para o grupo não ser retido pelo próprio evento.
//! - **`AbstractProgressScope`.** A classe abstrata é a struct concreta que as subclasses compõem; `onScopeOpened` e
//!   `onScopeClosed` (`protected`) são públicos, porque quem a compõe os chama de fora do módulo.
//! - **Verdadeiro e falso do JavaScript.** `if (state.total)` e `if (state.worked)` valem para número diferente de zero (e de
//!   `NaN`).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::{Promise, Thenable};
use je_base::runtime;
use je_editor::code_action::platform::{IProgressIndicator, IProgressRunner, ProgressTotal};
use je_widgets::progress_bar::ProgressBar;

use crate::common::editor::GroupModelChangeKind;
use crate::services::editor::editor_groups_service::IEditorGroup;

/// Os membros de `ProgressBar` que os indicadores usam (ver o cabeçalho).
pub trait IProgressBar {
    /// `infinite()`.
    fn infinite(&self);

    /// `total(value)`.
    fn total(&self, value: f64);

    /// `hasTotal()`.
    fn has_total(&self) -> bool;

    /// `worked(value)`.
    fn worked(&self, value: f64);

    /// `stop()`.
    fn stop(&self);

    /// `show(delay?)`.
    fn show(&self, delay: Option<u64>);

    /// `hide()`.
    fn hide(&self);
}

impl IProgressBar for ProgressBar {
    fn infinite(&self) {
        ProgressBar::infinite(self);
    }

    fn total(&self, value: f64) {
        ProgressBar::total(self, value);
    }

    fn has_total(&self) -> bool {
        ProgressBar::has_total(self)
    }

    fn worked(&self, value: f64) {
        ProgressBar::worked(self, value);
    }

    fn stop(&self) {
        ProgressBar::stop(self);
    }

    fn show(&self, delay: Option<u64>) {
        ProgressBar::show(self, delay);
    }

    fn hide(&self) {
        ProgressBar::hide(self);
    }
}

/// `emptyProgressRunner`: o `IProgressRunner` que ignora tudo.
struct EmptyProgressRunner;

impl IProgressRunner for EmptyProgressRunner {
    fn total(&self, _value: f64) {}

    fn worked(&self, _value: f64) {}

    fn done(&self) {}
}

/// `emptyProgressRunner`.
pub fn empty_progress_runner() -> Rc<dyn IProgressRunner> {
    Rc::new(EmptyProgressRunner)
}

/// `promise.then(settle, settle)` como uma promessa: assenta (sem erro) quando `promise` assenta, de qualquer forma.
fn settled_promise(promise: &dyn Thenable) -> Promise<()> {
    Promise::with_executor(|resolver| promise.on_settled(Box::new(move || resolver.resolve(()))))
}

/// `Promise.allSettled([first, second])`, sem o resultado: assenta quando as duas assentaram.
fn all_settled(first: &Promise<()>, second: &Promise<()>) -> Promise<()> {
    Promise::with_executor(|resolver| {
        let pending = Rc::new(Cell::new(2));
        for promise in [first, second] {
            let pending = pending.clone();
            let resolver = resolver.clone();
            promise.on_settled(Box::new(move || {
                pending.set(pending.get() - 1);
                if pending.get() == 0 {
                    resolver.resolve(());
                }
            }));
        }
    })
}

/// `try { this.progressBar.infinite().show(delay); await promise; } catch {} finally { this.progressBar.stop().hide(); }`.
fn do_show_while(progress_bar: Rc<dyn IProgressBar>, promise: Promise<()>, delay: Option<u64>) -> Promise<()> {
    progress_bar.infinite();
    progress_bar.show(delay);

    async_fn(async move {
        // catch (error) { // ignore }
        let _ = promise.await;

        progress_bar.stop();
        progress_bar.hide();

        Ok(())
    })
}

//#region EditorProgressIndicator

/// `EditorProgressIndicator`.
pub struct EditorProgressIndicator {
    base: Rc<Disposable>,
    progress_bar: Rc<dyn IProgressBar>,
    group: Rc<dyn IEditorGroup>,
}

impl EditorProgressIndicator {
    pub fn new(progress_bar: Rc<dyn IProgressBar>, group: Rc<dyn IEditorGroup>) -> Rc<Self> {
        let base = Disposable::new();
        let this = Rc::new(Self { base, progress_bar, group });
        this.register_listeners();
        this
    }

    fn register_listeners(&self) {
        // Stop any running progress when the active editor changes or the group becomes empty.
        // In contrast to the composite progress indicator, we do not track active editor progress and replay it later (yet).
        let progress_bar = self.progress_bar.clone();
        let group = Rc::downgrade(&self.group);
        self.base.register(self.group.on_did_model_change().listen(move |event| {
            let group_is_empty = || group.upgrade().is_some_and(|group| group.is_empty());
            if event.kind == GroupModelChangeKind::EditorActive || (event.kind == GroupModelChangeKind::EditorClose && group_is_empty()) {
                progress_bar.stop();
                progress_bar.hide();
            }
        }));
    }

    fn do_show(&self, infinite_or_total: ProgressTotal, delay: Option<u64>) -> Rc<dyn IProgressRunner> {
        match infinite_or_total {
            ProgressTotal::Infinite => {
                self.progress_bar.infinite();
                self.progress_bar.show(delay);
            }
            ProgressTotal::Total(total) => {
                self.progress_bar.total(total);
                self.progress_bar.show(delay);
            }
        }

        Rc::new(EditorProgressRunner { progress_bar: self.progress_bar.clone() })
    }
}

/// O objeto `IProgressRunner` que `EditorProgressIndicator.doShow` devolve.
struct EditorProgressRunner {
    progress_bar: Rc<dyn IProgressBar>,
}

impl IProgressRunner for EditorProgressRunner {
    fn total(&self, total: f64) {
        self.progress_bar.total(total);
    }

    fn worked(&self, worked: f64) {
        if self.progress_bar.has_total() {
            self.progress_bar.worked(worked);
        } else {
            self.progress_bar.infinite();
            self.progress_bar.show(None);
        }
    }

    fn done(&self) {
        self.progress_bar.stop();
        self.progress_bar.hide();
    }
}

impl IProgressIndicator for EditorProgressIndicator {
    fn show(&self, infinite_or_total: ProgressTotal, delay: Option<u64>) -> Rc<dyn IProgressRunner> {
        // No editor open: ignore any progress reporting
        if self.group.is_empty() {
            return empty_progress_runner();
        }

        self.do_show(infinite_or_total, delay)
    }

    fn show_while(&self, promise: &dyn Thenable, delay: Option<u64>) -> Promise<()> {
        let promise = settled_promise(promise);
        let group_is_empty = self.group.is_empty();
        let progress_bar = self.progress_bar.clone();

        async_fn(async move {
            // No editor open: ignore any progress reporting
            if group_is_empty {
                // catch (error) { // ignore }
                let _ = promise.clone().await;
            }

            do_show_while(progress_bar, promise, delay).await
        })
    }
}

impl IDisposable for EditorProgressIndicator {
    fn dispose(&self) {
        self.base.dispose();
    }
}

//#endregion

//#region ProgressIndicatorState

/// `ProgressIndicatorState.State`.
#[derive(Clone)]
enum ProgressIndicatorState {
    None,
    Done,
    Infinite,
    /// `ProgressIndicatorState.While(whilePromise, whileStart, whileDelay)`.
    While { while_promise: Promise<()>, while_start: f64, while_delay: f64 },
    /// `ProgressIndicatorState.Work(total, worked)`.
    Work { total: Option<f64>, worked: Option<f64> },
}

/// A veracidade de um `number | undefined` no JavaScript.
fn is_truthy_number(value: Option<f64>) -> Option<f64> {
    value.filter(|value| *value != 0.0 && !value.is_nan())
}

//#endregion

//#region ScopedProgressIndicator

/// `IProgressScope`.
pub trait IProgressScope {
    /// Fired whenever `isActive` value changed.
    fn on_did_change_active(&self) -> Event<()>;

    /// Whether progress should be active or not.
    fn is_active(&self) -> bool;
}

/// `ScopedProgressIndicator`.
pub struct ScopedProgressIndicator {
    base: Rc<Disposable>,
    this: Weak<ScopedProgressIndicator>,
    progress_state: RefCell<ProgressIndicatorState>,
    progress_bar: Rc<dyn IProgressBar>,
    scope: Rc<dyn IProgressScope>,
}

impl ScopedProgressIndicator {
    pub fn new(progress_bar: Rc<dyn IProgressBar>, scope: Rc<dyn IProgressScope>) -> Rc<Self> {
        let this = Rc::new_cyclic(|this| Self { base: Disposable::new(), this: this.clone(), progress_state: RefCell::new(ProgressIndicatorState::None), progress_bar, scope });
        this.register_listeners();
        this
    }

    pub fn register_listeners(&self) {
        let this = self.this.clone();
        self.base.register(self.scope.on_did_change_active().listen(move |_| {
            let Some(this) = this.upgrade() else { return };
            if this.scope.is_active() {
                this.on_did_scope_activate();
            } else {
                this.on_did_scope_deactivate();
            }
        }));
    }

    fn state(&self) -> ProgressIndicatorState {
        self.progress_state.borrow().clone()
    }

    fn set_state(&self, state: ProgressIndicatorState) {
        *self.progress_state.borrow_mut() = state;
    }

    fn on_did_scope_activate(&self) {
        match self.state() {
            // Return early if progress state indicates that progress is done
            ProgressIndicatorState::Done => {}

            // Replay Infinite Progress from Promise
            ProgressIndicatorState::While { while_delay, while_start, .. } => {
                let mut delay: Option<u64> = None;
                if while_delay > 0.0 {
                    let remaining_delay = while_delay - (runtime::date_now() as f64 - while_start);
                    if remaining_delay > 0.0 {
                        delay = Some(remaining_delay as u64);
                    }
                }

                self.do_show_while(delay);
            }

            // Replay Infinite Progress
            ProgressIndicatorState::Infinite => {
                self.progress_bar.infinite();
                self.progress_bar.show(None);
            }

            // Replay Finite Progress (Total & Worked)
            ProgressIndicatorState::Work { total, worked } => {
                if let Some(total) = is_truthy_number(total) {
                    self.progress_bar.total(total);
                    self.progress_bar.show(None);
                }

                if let Some(worked) = is_truthy_number(worked) {
                    self.progress_bar.worked(worked);
                    self.progress_bar.show(None);
                }
            }

            ProgressIndicatorState::None => {}
        }
    }

    fn on_did_scope_deactivate(&self) {
        self.progress_bar.stop();
        self.progress_bar.hide();
    }

    fn do_show_while(&self, delay: Option<u64>) {
        // Show Progress when active
        if self.scope.is_active() {
            self.progress_bar.infinite();
            self.progress_bar.show(delay);
        }
    }
}

/// O objeto `IProgressRunner` que `ScopedProgressIndicator.show` devolve.
struct ScopedProgressRunner {
    indicator: Weak<ScopedProgressIndicator>,
}

impl IProgressRunner for ScopedProgressRunner {
    fn total(&self, total: f64) {
        let Some(this) = self.indicator.upgrade() else { return };
        let worked = match this.state() {
            ProgressIndicatorState::Work { worked, .. } => worked,
            _ => None,
        };
        this.set_state(ProgressIndicatorState::Work { total: Some(total), worked });

        if this.scope.is_active() {
            this.progress_bar.total(total);
        }
    }

    fn worked(&self, worked: f64) {
        let Some(this) = self.indicator.upgrade() else { return };

        // Verify first that we are either not active or the progressbar has a total set
        if !this.scope.is_active() || this.progress_bar.has_total() {
            let (current_total, current_worked) = match this.state() {
                ProgressIndicatorState::Work { total, worked } => (total, worked),
                _ => (None, None),
            };
            this.set_state(ProgressIndicatorState::Work { total: current_total, worked: Some(current_worked.map_or(worked, |current| current + worked)) });

            if this.scope.is_active() {
                this.progress_bar.worked(worked);
            }
        }
        // Otherwise the progress bar does not support worked(), we fallback to infinite() progress
        else {
            this.set_state(ProgressIndicatorState::Infinite);
            this.progress_bar.infinite();
            this.progress_bar.show(None);
        }
    }

    fn done(&self) {
        let Some(this) = self.indicator.upgrade() else { return };
        this.set_state(ProgressIndicatorState::Done);

        if this.scope.is_active() {
            this.progress_bar.stop();
            this.progress_bar.hide();
        }
    }
}

impl IProgressIndicator for ScopedProgressIndicator {
    fn show(&self, infinite_or_total: ProgressTotal, delay: Option<u64>) -> Rc<dyn IProgressRunner> {
        // Sort out Arguments
        match infinite_or_total {
            ProgressTotal::Infinite => self.set_state(ProgressIndicatorState::Infinite),
            ProgressTotal::Total(total) => self.set_state(ProgressIndicatorState::Work { total: Some(total), worked: None }),
        }

        // Active: Show Progress
        if self.scope.is_active() {
            match self.state() {
                // Infinite: Start Progressbar and Show after Delay
                ProgressIndicatorState::Infinite => {
                    self.progress_bar.infinite();
                    self.progress_bar.show(delay);
                }

                // Finite: Start Progressbar and Show after Delay
                ProgressIndicatorState::Work { total: Some(total), .. } => {
                    self.progress_bar.total(total);
                    self.progress_bar.show(delay);
                }

                _ => {}
            }
        }

        Rc::new(ScopedProgressRunner { indicator: self.this.clone() })
    }

    fn show_while(&self, promise: &dyn Thenable, delay: Option<u64>) -> Promise<()> {
        let mut promise = settled_promise(promise);

        // Join with existing running promise to ensure progress is accurate
        if let ProgressIndicatorState::While { while_promise, .. } = self.state() {
            promise = all_settled(&promise, &while_promise);
        }

        // Keep Promise in State
        self.set_state(ProgressIndicatorState::While { while_promise: promise.clone(), while_start: delay.unwrap_or(0) as f64, while_delay: runtime::date_now() as f64 });

        self.do_show_while(delay);

        let this = self.this.clone();
        async_fn(async move {
            // catch (error) { // ignore }
            let _ = promise.clone().await;

            // finally
            let Some(this) = this.upgrade() else { return Ok(()) };

            // If this is not the last promise in the list of joined promises, skip this
            let is_last = match this.state() {
                ProgressIndicatorState::While { while_promise, .. } => while_promise.ptr_eq(&promise),
                _ => true,
            };
            if is_last {
                // The while promise is either null or equal the promise we last hooked on
                this.set_state(ProgressIndicatorState::None);

                if this.scope.is_active() {
                    this.progress_bar.stop();
                    this.progress_bar.hide();
                }
            }

            Ok(())
        })
    }
}

impl IDisposable for ScopedProgressIndicator {
    fn dispose(&self) {
        self.base.dispose();
    }
}

//#endregion

//#region AbstractProgressScope

/// `AbstractProgressScope`.
pub struct AbstractProgressScope {
    base: Rc<Disposable>,
    on_did_change_active: Rc<Emitter<()>>,
    scope_id: String,
    is_active: Cell<bool>,
}

impl AbstractProgressScope {
    /// `constructor(scopeId, _isActive)`.
    pub fn new(scope_id: &str, is_active: bool) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_active = Emitter::new();
        base.register(on_did_change_active.clone());

        Rc::new(Self { base, on_did_change_active, scope_id: scope_id.to_owned(), is_active: Cell::new(is_active) })
    }

    /// `onScopeOpened(scopeId)` (`protected` no original).
    pub fn on_scope_opened(&self, scope_id: &str) {
        if scope_id == self.scope_id && !self.is_active.get() {
            self.is_active.set(true);

            self.on_did_change_active.fire(());
        }
    }

    /// `onScopeClosed(scopeId)` (`protected` no original).
    pub fn on_scope_closed(&self, scope_id: &str) {
        if scope_id == self.scope_id && self.is_active.get() {
            self.is_active.set(false);

            self.on_did_change_active.fire(());
        }
    }
}

impl IProgressScope for AbstractProgressScope {
    fn on_did_change_active(&self) -> Event<()> {
        self.on_did_change_active.event()
    }

    fn is_active(&self) -> bool {
        self.is_active.get()
    }
}

impl IDisposable for AbstractProgressScope {
    fn dispose(&self) {
        self.base.dispose();
    }
}

//#endregion
