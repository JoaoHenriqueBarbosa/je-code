//! Porte de `src/vs/base/test/common/virtualScheduling/processor.ts`: o `VirtualTimeProcessor`, que
//! conduz um [`VirtualClock`] a partir do laço de eventos do host (o *embedding* de um pequeno laço de
//! eventos virtual no laço do host).
//!
//! Notas de tradução:
//! - `extends Disposable` vira um `Rc<Disposable>` guardado em `base` ([`IDisposable`] delega a ele); o
//!   `_register({ dispose: () => this._onDispose() })` do construtor é o registro de um descartável que
//!   chama [`VirtualTimeProcessor::on_dispose`].
//! - `Promise<void>` é `Promise<()>` do porte, que se espera com `await` (`Result<(), Error>`).
//! - `RunOptions` leva construtores encadeados ([`RunOptions::new`], [`RunOptions::with_max_events`],
//!   [`RunOptions::with_max_trace_depth`]) no lugar do objeto literal; os campos continuam públicos.
//! - `untilIdle` é a função [`until_idle`] (o original a expõe como constante, mas `TerminationPolicy`
//!   carrega um `CancellationToken` em `until_token`, que não é constante).
//! - O `Map<Run, IDisposable>` é um vetor de pares na ordem de inserção, procurado por identidade (`Rc`).
//! - Divergência: as mensagens de [`VirtualTimeProcessor`] que no original separam as duas partes por
//!   travessão usam " - ".
//! - O `try { ... } catch (e)` de `_executeOne` usa `errors::catch`; um `Error` já é o erro (o
//!   `e instanceof Error ? e : new Error(String(e))` do original não tem o que converter).
//! - O `finally { this._inDrive = false }` do trampolim é um guarda que solta a marca no `Drop`, o que
//!   cobre também o pânico.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};

use super::embedding::{Embedding, EmbeddingChoice, next_macrotask};
use super::time_api::TimeApi;
use super::trace::{RunAsHandlerOptions, TraceContext, root_trace};
use super::virtual_clock::{EventSource, VirtualClock, VirtualEvent, VirtualTime};
use crate::cancellation::CancellationToken;
use crate::errors::{self, Error};
use crate::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, plain_disposable};
use crate::promise::{Promise, Resolver};

// ============================================================================
// Política de término
// ============================================================================

/// Quando um [`Run`] deve terminar.
///
/// Escolha do projeto greenfield: o término é *sempre* explícito. Não existe um `run()` nu que termina
/// na primeira fila vazia, porque isso cria uma corrida com a cadeia de microtarefas de quem chamou (o
/// `run` poderia resolver antes de o `.then` de quem chamou ter agendado algo).
#[derive(Clone)]
pub enum TerminationPolicy {
    /// Resolve assim que a fila virtual esvazia.
    Idle,
    /// Resolve quando o token é cancelado E a fila está vazia.
    Token(CancellationToken),
    /// Resolve quando o tempo virtual chegou a `time` e todos os eventos agendados em `time` ou antes
    /// foram processados. O processador agenda um evento sentinela em `time`, de modo que o tempo
    /// virtual sempre o alcança.
    Time(VirtualTime),
}

/// `untilIdle`.
pub fn until_idle() -> TerminationPolicy {
    TerminationPolicy::Idle
}

pub fn until_token(token: CancellationToken) -> TerminationPolicy {
    TerminationPolicy::Token(token)
}

pub fn until_time(time: VirtualTime) -> TerminationPolicy {
    TerminationPolicy::Time(time)
}

#[derive(Clone)]
pub struct RunOptions {
    pub until: TerminationPolicy,
    /// Máximo de eventos virtuais que este run executa. Padrão: 100.
    pub max_events: Option<usize>,
    /// Profundidade máxima de trace causal que este run tolera. Útil para pegar timers que se
    /// reagendam sem parar.
    pub max_trace_depth: Option<usize>,
}

impl RunOptions {
    pub fn new(until: TerminationPolicy) -> Self {
        Self { until, max_events: None, max_trace_depth: None }
    }

    #[must_use]
    pub fn with_max_events(mut self, max_events: usize) -> Self {
        self.max_events = Some(max_events);
        self
    }

    #[must_use]
    pub fn with_max_trace_depth(mut self, max_trace_depth: usize) -> Self {
        self.max_trace_depth = Some(max_trace_depth);
        self
    }
}

// ============================================================================
// Run: o estado interno de uma chamada a `processor.run()`
// ============================================================================

enum RunStatus {
    Continue,
    Done,
    Error(Error),
}

static RUN_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

struct Run {
    id: u64,
    promise: Promise<()>,
    resolver: Resolver<()>,
    settled: Cell<bool>,
    options: RunOptions,
    executed_at_start: usize,
    max_events: usize,
}

impl Run {
    fn new(options: RunOptions, executed_at_start: usize, max_events: usize) -> Rc<Self> {
        let (promise, resolver) = Promise::new();
        Rc::new(Self {
            id: RUN_ID_COUNTER.fetch_add(1, Ordering::Relaxed) + 1,
            promise,
            resolver,
            settled: Cell::new(false),
            options,
            executed_at_start,
            max_events,
        })
    }

    fn settle(&self, error: Option<Error>) {
        if self.settled.replace(true) {
            return;
        }
        match error {
            Some(error) => self.resolver.reject(error),
            None => self.resolver.resolve(()),
        }
    }

    fn evaluate(&self, clock: &VirtualClock, executed_total: usize, make_overflow: impl FnOnce() -> Error) -> RunStatus {
        let local = executed_total - self.executed_at_start;
        if local >= self.max_events && clock.has_events() {
            return RunStatus::Error(make_overflow());
        }

        match &self.options.until {
            TerminationPolicy::Idle => {
                if clock.has_events() {
                    RunStatus::Continue
                } else {
                    RunStatus::Done
                }
            }
            TerminationPolicy::Token(token) => {
                if token.is_cancellation_requested() && !clock.has_events() {
                    RunStatus::Done
                } else {
                    RunStatus::Continue
                }
            }
            TerminationPolicy::Time(time) => {
                // Pronto se todo evento restante está estritamente depois do prazo. O sentinela
                // garante que a fila não esvazia antes de o prazo ser alcançado, então nunca resolve
                // "cedo" só porque nada foi agendado ainda.
                match clock.peek_next() {
                    Some(next) if next.time <= *time => RunStatus::Continue,
                    _ => RunStatus::Done,
                }
            }
        }
    }
}

// ============================================================================
// Resultado de um passo: o que a máquina de estados pura diz ao trampolim
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum StepOutcome {
    /// Um evento virtual foi executado, ou um run foi rejeitado por contabilidade (estouro de
    /// profundidade ou de eventos). O trampolim deixa o embedding decidir como chegar ao próximo passo.
    Progress,
    /// Nenhum evento acionável sob nenhum prazo ativo. O trampolim estaciona até algo acordar o
    /// processador.
    Park,
    /// Nenhum run ativo. O trampolim para de conduzir.
    Quiesce,
}

// ============================================================================
// VirtualTimeProcessor
// ============================================================================

#[derive(Clone, Copy, Default)]
pub struct VirtualTimeProcessorOptions {
    pub default_max_events: Option<usize>,
}

/// Solta a marca de "dentro do trampolim" ao sair, por qualquer caminho.
struct DriveGuard<'a>(&'a Cell<bool>);

impl Drop for DriveGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// # `VirtualTimeProcessor`
///
/// Conduz um [`VirtualClock`] a partir do laço de eventos do host. É o **embedding** de um pequeno laço
/// de eventos virtual no laço do host.
///
/// ## Responsabilidades, separadas
///
/// - `step` é um avanço *puro* da máquina de estados. Lê o relógio, decide o que fazer, opcionalmente
///   executa um evento virtual e devolve um [`StepOutcome`]. Nunca toca o tempo do host.
/// - `drive` é o *trampolim*. Chama `step` e deixa o [`Embedding`] decidir se repete o laço no lugar
///   (`ContinueSync`) ou agenda a próxima iteração no host (`CbScheduled`). É o único código que toca o
///   tempo do host.
/// - [`Run`] carrega o predicado de término do usuário. Os runs são puros sobre as observações de `step`;
///   nunca agendam.
///
/// ## Invariantes
///
/// 1. **Condutor único.** A cada momento há no máximo uma invocação de `drive` ativa por processador (a
///    guarda `in_drive`).
/// 2. **`step` é puro em relação ao tempo do host.** Só lê o relógio, muda o conjunto de runs ao
///    assentá-los e roda de forma síncrona no máximo um evento virtual. Nunca chama uma API de tempo do
///    host.
/// 3. **O embedding escolhe a primitiva do host.** Se o próximo passo roda no lugar, depois de um
///    esvaziamento de microtarefas ou num quadro de pintura é decisão inteira do embedding, *por evento*.
/// 4. **Estacionar é interrompível.** Estacionado, o processador acorda com
///    [`VirtualClock::on_event_scheduled`], com o cancelamento de um token e com um novo run.
/// 5. **Descartar é terminal.** Depois de `dispose`, todos os runs são rejeitados e `step`/`drive`
///    devolvem `Quiesce` direto.
///
/// ## O escoadouro do reset do trace
///
/// O reset adiado do contexto de trace (ver `TraceContext::run_as_handler`) precisa de uma primitiva
/// "dispara depois do fecho de microtarefas". O processador passa a *sua própria* `next_macrotask` como
/// esse escoadouro, de modo que o reset passa pela mesma primitiva que o embedding usa nos próprios
/// saltos para o host. Isso elimina qualquer corrida entre os saltos do processador e o timer do reset.
pub struct VirtualTimeProcessor {
    base: Rc<Disposable>,
    me: Weak<Self>,
    runs: RefCell<Vec<(Rc<Run>, DisposableRef)>>,
    history: RefCell<Vec<VirtualEvent>>,
    executed_total: Cell<usize>,
    disposed: Cell<bool>,
    in_drive: Cell<bool>,
    park_cleanup: RefCell<Option<DisposableRef>>,
    default_max_events: usize,
    clock: Rc<VirtualClock>,
    embedding: Embedding,
    real_api: Rc<dyn TimeApi>,
}

impl VirtualTimeProcessor {
    pub fn new(clock: Rc<VirtualClock>, embedding: Embedding, real_api: Rc<dyn TimeApi>, opts: VirtualTimeProcessorOptions) -> Rc<Self> {
        let base = Disposable::new();
        let this = Rc::new_cyclic(|me: &Weak<Self>| Self {
            base: base.clone(),
            me: me.clone(),
            runs: RefCell::new(Vec::new()),
            history: RefCell::new(Vec::new()),
            executed_total: Cell::new(0),
            disposed: Cell::new(false),
            in_drive: Cell::new(false),
            park_cleanup: RefCell::new(None),
            default_max_events: opts.default_max_events.unwrap_or(100),
            clock,
            embedding,
            real_api,
        });
        let weak = Rc::downgrade(&this);
        base.register(plain_disposable(move || {
            if let Some(processor) = weak.upgrade() {
                processor.on_dispose();
            }
        }));
        this
    }

    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o processador está vivo enquanto um método dele roda")
    }

    /// `history`.
    pub fn history(&self) -> Vec<VirtualEvent> {
        self.history.borrow().clone()
    }

    /// `executedTotal`.
    pub fn executed_total(&self) -> usize {
        self.executed_total.get()
    }

    // ---- API pública ------------------------------------------------------

    /// Começa um run com a política de término dada.
    pub fn run(&self, options: RunOptions) -> Promise<()> {
        let max_events = options.max_events.unwrap_or(self.default_max_events);
        let run = Run::new(options.clone(), self.executed_total.get(), max_events);
        let cleanup = DisposableStore::new();

        // Acorda o laço no cancelamento do token, para o run se reavaliar.
        if let TerminationPolicy::Token(token) = &options.until {
            let processor = self.this();
            cleanup.add(token.on_cancellation_requested().listen(move |_| processor.wake()));
        }

        // Para término por tempo, agenda um sentinela no prazo. Isso garante que o tempo virtual chega ao
        // prazo mesmo que o usuário nunca agende mais nada, e que o run não resolve cedo só porque a fila
        // está vazia *agora*.
        if let TerminationPolicy::Time(time) = &options.until
            && *time > self.clock.now()
        {
            let source = EventSource::new(format!("<deadline of run #{}>", run.id));
            cleanup.add(self.clock.schedule(VirtualEvent::new(*time, source, || { /* sentinela: não faz nada */ })));
        }

        let promise = run.promise.clone();
        self.runs.borrow_mut().push((run, cleanup as DisposableRef));
        self.wake();
        promise
    }

    // ---- O passo puro -----------------------------------------------------

    fn step(&self) -> StepOutcome {
        if self.disposed.get() {
            return StepOutcome::Quiesce;
        }

        self.settle_finished_runs();
        if self.runs.borrow().is_empty() {
            return StepOutcome::Quiesce;
        }

        let Some(next) = self.clock.peek_next() else { return StepOutcome::Park };

        // Confere a profundidade de trace de cada run: rejeita o run cujo limite este evento
        // estouraria, antes de executar.
        let trace_depth = next.trace.as_ref().map_or(0, |trace| trace.depth);
        let mut depth_overflow = false;
        for run in self.run_list() {
            if let Some(limit) = run.options.max_trace_depth
                && trace_depth > limit
            {
                self.settle_run(&run, Some(Self::build_depth_overflow(&run, trace_depth)));
                depth_overflow = true;
            }
        }
        if depth_overflow {
            return StepOutcome::Progress;
        }

        self.execute_one(&next);
        StepOutcome::Progress
    }

    fn execute_one(&self, event: &VirtualEvent) {
        let real_api = self.real_api.clone();
        let result = errors::catch(|| {
            TraceContext::instance().run_as_handler(
                event.trace.clone().unwrap_or_else(root_trace),
                || {
                    if let Some(executed) = self.clock.run_next() {
                        self.history.borrow_mut().push(executed);
                        self.executed_total.set(self.executed_total.get() + 1);
                    }
                },
                // Passa o reset do trace pela mesma primitiva que o embedding usa, para não haver corrida
                // entre este timer e o próximo salto do embedding.
                &RunAsHandlerOptions::new(move |cb| next_macrotask(&*real_api, cb)),
            );
        });
        if let Err(error) = result {
            // Não dá para saber qual run "era dono" do evento que lançou. Rejeita todos os runs ativos,
            // para a falha ser observada uma vez por chamador.
            for run in self.run_list() {
                self.settle_run(&run, Some(error.clone()));
            }
        }
    }

    // ---- O trampolim ------------------------------------------------------

    fn drive(&self) {
        if self.in_drive.replace(true) {
            return;
        }
        let _guard = DriveGuard(&self.in_drive);
        loop {
            match self.step() {
                StepOutcome::Quiesce => return,
                StepOutcome::Park => {
                    self.park();
                    return;
                }
                StepOutcome::Progress => {}
            }

            // 'Progress': lê o próximo evento para o embedding escolher a primitiva por evento. Se não
            // há, repete e deixa o próximo `step` decidir entre `Park` e `Quiesce`.
            let Some(next) = self.clock.peek_next() else { continue };

            let processor = self.this();
            if (self.embedding)(&next, Rc::new(move || processor.drive())) == EmbeddingChoice::CbScheduled {
                return;
            }
            // `ContinueSync`: repete no lugar.
        }
    }

    // ---- Estacionar e acordar ---------------------------------------------

    fn park(&self) {
        self.unpark();
        let store = DisposableStore::new();
        let processor = self.this();
        store.add(self.clock.on_event_scheduled().listen(move |_| processor.wake()));
        *self.park_cleanup.borrow_mut() = Some(store);
    }

    fn unpark(&self) {
        let cleanup = self.park_cleanup.borrow_mut().take();
        if let Some(cleanup) = cleanup {
            cleanup.dispose();
        }
    }

    fn wake(&self) {
        if self.disposed.get() {
            return;
        }
        self.unpark();
        // Reentra no trampolim numa macrotarefa do host, NÃO numa microtarefa. Isso:
        //  - junta várias chamadas de `wake` no mesmo tick,
        //  - tira o condutor do quadro da pilha de quem chamou, e
        //  - deixa todo o fecho de microtarefas pendente (inclusive as enfileiradas DEPOIS desta chamada
        //    dentro da mesma microtarefa externa) esvaziar antes do próximo `step`. Um salto por
        //    microtarefa poria o condutor em ordem FIFO com essas microtarefas seguintes, e ele poderia
        //    rodar um evento virtual antes de a cadeia de promessas do consumidor que depende dele ter
        //    assentado.
        let processor = self.this();
        next_macrotask(&*self.real_api, move || processor.drive());
    }

    // ---- Ciclo de vida dos runs -------------------------------------------

    fn run_list(&self) -> Vec<Rc<Run>> {
        self.runs.borrow().iter().map(|(run, _)| run.clone()).collect()
    }

    fn settle_finished_runs(&self) {
        for run in self.run_list() {
            if run.settled.get() {
                continue;
            }
            match run.evaluate(&self.clock, self.executed_total.get(), || self.build_overflow(&run)) {
                RunStatus::Done => self.settle_run(&run, None),
                RunStatus::Error(error) => self.settle_run(&run, Some(error)),
                RunStatus::Continue => {}
            }
        }
    }

    fn settle_run(&self, run: &Rc<Run>, error: Option<Error>) {
        let position = self.runs.borrow().iter().position(|(r, _)| Rc::ptr_eq(r, run));
        let Some(position) = position else { return };
        let (_, cleanup) = self.runs.borrow_mut().remove(position);
        cleanup.dispose();
        run.settle(error);
    }

    fn build_overflow(&self, run: &Run) -> Error {
        let local = self.executed_total.get() - run.executed_at_start;
        Error::new(format!(
            "[VirtualTimeProcessor] Run #{} exceeded maxEvents ({}) - executed {} virtual event(s) and the queue is still not empty.",
            run.id, run.max_events, local
        ))
    }

    fn build_depth_overflow(run: &Run, depth: usize) -> Error {
        Error::new(format!(
            "[VirtualTimeProcessor] Run #{} exceeded maxTraceDepth ({}) - next event has trace depth {}. This usually indicates a runaway self-rescheduling timer.",
            run.id,
            run.options.max_trace_depth.unwrap_or_default(),
            depth
        ))
    }

    fn on_dispose(&self) {
        self.disposed.set(true);
        self.unpark();
        let error = Error::new("VirtualTimeProcessor disposed");
        for run in self.run_list() {
            self.settle_run(&run, Some(error.clone()));
        }
    }
}

impl IDisposable for VirtualTimeProcessor {
    fn dispose(&self) {
        self.base.dispose();
    }
}
