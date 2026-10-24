//! Porte de `src/vs/workbench/services/userAttention/browser/userAttentionBrowser.ts`: o
//! [`UserAttentionService`] (atenção do usuário a partir de foco e atividade recente) e o
//! [`UserAttentionServiceEnv`] (o adaptador que transforma foco da janela e entrada do usuário em
//! observáveis).
//!
//! Divergências:
//! - **A parte de DOM é especificação, e a fonte dos eventos é o trait [`IUserAttentionSource`].** O original
//!   lê o foco do `IHostService` (`onDidChangeFocus` e `hasFocus`) e escuta `keydown`, `mousemove`,
//!   `mousedown` e `touchstart` no `document` de cada janela (a `mainWindow` e as que `dom.onDidRegisterWindow`
//!   anuncia depois), com `{ passive: true, capture: true }`. Nada disso existe no je-code, que desenha em
//!   widgets nativos. O trait tem as duas pontas: `on_did_change_focus`/`has_focus` (o `IHostService`) e
//!   `on_user_activity` (a união dos quatro tipos de entrada em todas as janelas, que a camada de janela
//!   do je-code emite a cada tecla, movimento ou clique do ponteiro e toque). O [`UserAttentionServiceEnv`]
//!   segue o original a partir dali: o foco vira observável, e cada atividade liga `isUserActive` por 500 ms.
//! - **`instantiationService.createInstance(UserAttentionServiceEnv)`** vira a dependência
//!   [`IUserAttentionServiceEnv`] do serviço (o original constrói o adaptador dentro do construtor e o
//!   teste troca a construção com `sinon.stub(createInstance)`; em Rust o teste registra o adaptador falso na
//!   coleção de serviços). O `IInstantiationService` deixa de ser parâmetro, por não ter mais uso, e o
//!   adaptador deixa de ser descartado pelo serviço (`_register`): quem o cria, o serviço de instanciação,
//!   o descarta ([`register`] o registra com `dispose` junto).
//! - **`registerSingleton(IUserAttentionService, UserAttentionService, InstantiationType.Delayed)`** vira
//!   [`register`], que o chamador roda uma vez na montagem do workbench (Rust não tem efeito colateral ao
//!   carregar o módulo).
//! - **`setTimeout`/`clearTimeout`** são os do `je_base::runtime` (o relógio virtual dos testes), e o
//!   `ReturnType<typeof setTimeout>` é o `TimerId`.
//! - **`_isTracingEnabled`** é uma variável local: o original só a lê no construtor.
//! - Os tipos `IObservable<boolean>` são `IObservable<bool>`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::observable::base::{IObservable, Observable, SettableExt};
use je_base::observable::experimental::time::{TotalTrueTimeObservable, was_true_recently};
use je_base::observable::observables::observable_value::{ObservableValue, observable_value};
use je_base::observable::{autorun, derived, observable_from_event};
use je_base::runtime::{self, TimerId};
use je_instantiation::{Injectable, InstantiationType, register_singleton, service_id, service_impl};
use je_platform::log::{LogLevel, LogService};

use super::user_attention_service::{I_USER_ATTENTION_SERVICE, IUserAttentionService};

/// The user attention timeout in milliseconds.
/// User is considered attentive if there was activity within this time frame.
const USER_ATTENTION_TIMEOUT_MS: u64 = 60_000;

/// Quanto tempo uma atividade conta como "agora" (`isUserActive` fica ligado por esse tempo).
const ACTIVITY_DEBOUNCE_MS: u64 = 500;

/// O que o [`UserAttentionServiceEnv`] lê do ambiente: o foco do VS Code (o `IHostService`) e a entrada do
/// usuário (os eventos de DOM do original).
pub trait IUserAttentionSource {
    /// `IHostService.onDidChangeFocus`.
    fn on_did_change_focus(&self) -> Event<bool>;

    /// `IHostService.hasFocus`.
    fn has_focus(&self) -> bool;

    /// A entrada do usuário em qualquer janela: tecla pressionada, movimento do ponteiro, botão do ponteiro
    /// pressionado e toque iniciado (`keydown`, `mousemove`, `mousedown` e `touchstart`).
    fn on_user_activity(&self) -> Event<()>;
}

service_id!(pub I_USER_ATTENTION_SOURCE: dyn IUserAttentionSource = "userAttentionSource");

/// O que o [`UserAttentionService`] lê do adaptador (`UserAttentionServiceEnv`): `isVsCodeFocused` e
/// `isUserActive`.
pub trait IUserAttentionServiceEnv {
    fn is_vs_code_focused(&self) -> IObservable<bool>;
    fn is_user_active(&self) -> IObservable<bool>;
}

service_id!(pub I_USER_ATTENTION_SERVICE_ENV: dyn IUserAttentionServiceEnv = "userAttentionServiceEnv");

/// `UserAttentionService`.
pub struct UserAttentionService {
    base: Rc<Disposable>,
    is_vs_code_focused: IObservable<bool>,
    is_user_active: IObservable<bool>,
    has_user_attention: IObservable<bool>,
    time_keeper: Rc<TotalTrueTimeObservable>,
}

impl Injectable for UserAttentionService {
    type Args = ();
    type Deps = (Rc<dyn IUserAttentionServiceEnv>, Rc<dyn LogService>);

    fn create(_: (), (host_adapter, log_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let is_vs_code_focused = host_adapter.is_vs_code_focused();
        let is_user_active = host_adapter.is_user_active();

        let is_tracing_enabled = {
            let log_service = log_service.clone();
            observable_from_event(log_service.on_did_change_log_level(), move |_| log_service.get_level() == LogLevel::Trace)
        };

        let store = DisposableStore::new();
        base.register(store.clone());
        let had_recent_activity = was_true_recently(is_user_active.clone(), USER_ATTENTION_TIMEOUT_MS, &store);

        let has_user_attention = derived(move |reader| had_recent_activity.read(reader));

        let time_keeper = TotalTrueTimeObservable::new(has_user_attention.clone());
        base.register(time_keeper.clone());

        let (focused, attention) = (is_vs_code_focused.clone(), has_user_attention.clone());
        base.register(autorun(move |reader| {
            if !is_tracing_enabled.read(reader) {
                return;
            }

            let (focused, log) = (focused.clone(), log_service.clone());
            reader.store().add(autorun(move |inner_reader| {
                let focused = focused.read(inner_reader);
                log.trace(&format!("[UserAttentionService] VS Code focus changed: {focused}"));
            }));
            let (attention, log) = (attention.clone(), log_service.clone());
            reader.store().add(autorun(move |inner_reader| {
                let has_attention = attention.read(inner_reader);
                log.trace(&format!("[UserAttentionService] User attention changed: {has_attention}"));
            }));
        }));

        Rc::new(Self { base, is_vs_code_focused, is_user_active, has_user_attention, time_keeper })
    }
}

service_impl!(UserAttentionService => dyn IUserAttentionService, disposable);

impl IUserAttentionService for UserAttentionService {
    fn is_vs_code_focused(&self) -> IObservable<bool> {
        self.is_vs_code_focused.clone()
    }

    fn is_user_active(&self) -> IObservable<bool> {
        self.is_user_active.clone()
    }

    fn has_user_attention(&self) -> IObservable<bool> {
        self.has_user_attention.clone()
    }

    fn total_focus_time_ms(&self) -> u64 {
        self.time_keeper.total_time_ms()
    }

    fn fire_after_given_focus_time_passed(&self, focus_time_ms: u64, callback: Box<dyn Fn()>) -> DisposableRef {
        self.time_keeper.fire_when_time_increased_by(focus_time_ms, callback)
    }
}

impl IDisposable for UserAttentionService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `UserAttentionServiceEnv`.
pub struct UserAttentionServiceEnv {
    base: Rc<Disposable>,
    log_service: Rc<dyn LogService>,
    is_vs_code_focused: IObservable<bool>,
    is_user_active: Rc<ObservableValue<bool, ()>>,
    activity_debounce_timeout: Rc<RefCell<Option<TimerId>>>,
}

impl Injectable for UserAttentionServiceEnv {
    type Args = ();
    type Deps = (Rc<dyn IUserAttentionSource>, Rc<dyn LogService>);

    fn create(_: (), (source, log_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let is_vs_code_focused = {
            let source = source.clone();
            observable_from_event(source.on_did_change_focus(), move |_| source.has_focus())
        };

        let this = Rc::new_cyclic(|weak: &Weak<Self>| {
            let weak = weak.clone();
            base.register(source.on_user_activity().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.mark_user_activity();
                }
            }));

            Self {
                base,
                log_service,
                is_vs_code_focused,
                is_user_active: observable_value::<bool, ()>("isUserActive", false),
                activity_debounce_timeout: Rc::new(RefCell::new(None)),
            }
        });

        if source.has_focus() {
            this.mark_user_activity();
        }
        this
    }
}

service_impl!(UserAttentionServiceEnv => dyn IUserAttentionServiceEnv, disposable);

impl UserAttentionServiceEnv {
    fn mark_user_activity(&self) {
        let previous_timeout = self.activity_debounce_timeout.borrow_mut().take();
        match previous_timeout {
            Some(timeout) => runtime::clear_timeout(timeout),
            None => {
                self.log_service.trace("[UserAttentionService] User activity detected");
                self.is_user_active.set(true, None);
            }
        }

        // An activity event accounts for 500ms for immediate use activity
        let (is_user_active, activity_debounce_timeout) = (self.is_user_active.clone(), self.activity_debounce_timeout.clone());
        let timeout = runtime::set_timeout(
            move || {
                is_user_active.set(false, None);
                *activity_debounce_timeout.borrow_mut() = None;
            },
            ACTIVITY_DEBOUNCE_MS,
        );
        *self.activity_debounce_timeout.borrow_mut() = Some(timeout);
    }
}

impl IUserAttentionServiceEnv for UserAttentionServiceEnv {
    fn is_vs_code_focused(&self) -> IObservable<bool> {
        self.is_vs_code_focused.clone()
    }

    fn is_user_active(&self) -> IObservable<bool> {
        self.is_user_active.to_observable()
    }
}

impl IDisposable for UserAttentionServiceEnv {
    fn dispose(&self) {
        let timeout = self.activity_debounce_timeout.borrow_mut().take();
        if let Some(timeout) = timeout {
            runtime::clear_timeout(timeout);
        }
        self.base.dispose();
    }
}

/// `registerSingleton(IUserAttentionService, UserAttentionService, InstantiationType.Delayed)`. O adaptador
/// ([`UserAttentionServiceEnv`]) entra como singleton à parte, ver as divergências do módulo.
pub fn register() {
    register_singleton::<UserAttentionServiceEnv>(I_USER_ATTENTION_SERVICE_ENV, InstantiationType::Eager);
    register_singleton::<UserAttentionService>(I_USER_ATTENTION_SERVICE, InstantiationType::Delayed);
}
