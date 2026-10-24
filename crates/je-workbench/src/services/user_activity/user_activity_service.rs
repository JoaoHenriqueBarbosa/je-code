//! Porte de `src/vs/workbench/services/userActivity/common/userActivityService.ts`: o serviço que observa a
//! atividade do usuário na janela.
//!
//! Divergências:
//! - `IMarkActiveOptions` é [`MarkActiveOptions`] e o parâmetro opcional `opts?` é `Option<MarkActiveOptions>`;
//!   `whenHeldFor` é `Option<u64>` em milissegundos (zero vale ausente, como a verificação de verdade do JS).
//! - `Disposable.None` é [`lifecycle::none`]. A herança de `Disposable` vira o campo `base`.
//! - O `@IInstantiationService` do construtor é a dependência `Rc<InstantiationService>` do [`Injectable`], e o
//!   `registerSingleton(..., Delayed)` do fim do módulo é [`register_user_activity_service`], chamada por quem
//!   monta o workbench (Rust não tem efeito colateral na carga do módulo).

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::async_::{disposable_timeout, run_when_global_idle, RunOnceScheduler};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{self, to_disposable, Disposable, DisposableRef, DisposableStore, IDisposable};
use je_instantiation::{register_singleton, Injectable, InstantiationService, InstantiationType};

use super::user_activity_registry;

/// `IMarkActiveOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarkActiveOptions {
    pub when_held_for: Option<u64>,
    /// Só considera este progresso se o estado já está ativo. Evita que trabalho em segundo plano marque o
    /// usuário como ativo por engano (#237386).
    pub extend_only: bool,
}

/// `IUserActivityService`: serviço que observa a atividade do usuário na janela.
pub trait IUserActivityService {
    /// Se o usuário está ativo agora.
    fn is_active(&self) -> bool;

    /// Dispara quando o estado de atividade muda.
    fn on_did_change_is_active(&self) -> Event<bool>;

    /// Marca o usuário como ativo até o disposable ser descartado. Vários consumidores chamam o método; o
    /// usuário só é considerado inativo quando todos descartaram os seus.
    fn mark_active(&self, opts: Option<MarkActiveOptions>) -> DisposableRef;
}

const MARK_INACTIVE_DEBOUNCE: u64 = 10_000;

je_instantiation::delayed_proxy!(pub UserActivityServiceProxy for dyn IUserActivityService {
    methods {
        fn is_active(&self) -> bool;
        fn mark_active(&self, opts: Option<MarkActiveOptions>) -> DisposableRef;
    }
    events {
        fn on_did_change_is_active(&self) -> bool;
    }
});

je_instantiation::service_id!(pub I_USER_ACTIVITY_SERVICE: dyn IUserActivityService = "IUserActivityService", proxy UserActivityServiceProxy);

pub struct UserActivityService {
    base: Rc<Disposable>,
    mark_inactive: Rc<RunOnceScheduler>,
    change_emitter: Rc<Emitter<bool>>,
    active: Cell<u32>,
    /// Começa `true`, já que o usuário acabou de fazer algo para abrir a janela. O `DomActivityTracker` também
    /// assume atividade no início, para desfazer isto se a janela for abandonada.
    is_active: Cell<bool>,
    this: Weak<UserActivityService>,
}

impl UserActivityService {
    fn this(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto um método dele roda")
    }
}

impl Injectable for UserActivityService {
    type Args = ();
    type Deps = (Rc<InstantiationService>,);

    fn create((): (), (instantiation_service,): Self::Deps) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| {
            let base = Disposable::new();
            let mark_inactive = RunOnceScheduler::new(
                {
                    let weak = weak.clone();
                    move || {
                        if let Some(this) = weak.upgrade() {
                            this.is_active.set(false);
                            this.change_emitter.fire(false);
                        }
                    }
                },
                MARK_INACTIVE_DEBOUNCE,
            );
            base.register(mark_inactive.clone());
            let change_emitter = Emitter::<bool>::new();
            base.register(change_emitter.clone());
            base.register(run_when_global_idle(
                {
                    let weak = weak.clone();
                    move |_| {
                        if let Some(this) = weak.upgrade() {
                            user_activity_registry::take(this, instantiation_service);
                        }
                    }
                },
                None,
            ));
            Self { base, mark_inactive, change_emitter, active: Cell::new(0), is_active: Cell::new(true), this: weak.clone() }
        })
    }
}

je_instantiation::service_impl!(UserActivityService => dyn IUserActivityService, disposable);

impl IDisposable for UserActivityService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IUserActivityService for UserActivityService {
    fn is_active(&self) -> bool {
        self.is_active.get()
    }

    fn on_did_change_is_active(&self) -> Event<bool> {
        self.change_emitter.event()
    }

    fn mark_active(&self, opts: Option<MarkActiveOptions>) -> DisposableRef {
        let opts = opts.unwrap_or_default();
        if opts.extend_only && !self.is_active.get() {
            return lifecycle::none();
        }

        if let Some(held_for) = opts.when_held_for.filter(|held_for| *held_for > 0) {
            let store = DisposableStore::new();
            let (this, held) = (self.this(), store.clone());
            store.add(disposable_timeout(
                move || {
                    held.add(this.mark_active(None));
                },
                held_for,
                None,
            ));
            return store;
        }

        let count = self.active.get() + 1;
        self.active.set(count);
        if count == 1 {
            self.is_active.set(true);
            self.change_emitter.fire(true);
            self.mark_inactive.cancel();
        }

        let this = self.this();
        to_disposable(move || {
            let count = this.active.get() - 1;
            this.active.set(count);
            if count == 0 {
                this.mark_inactive.schedule(None);
            }
        })
    }
}

/// `registerSingleton(IUserActivityService, UserActivityService, InstantiationType.Delayed)`.
pub fn register_user_activity_service() {
    register_singleton::<UserActivityService>(I_USER_ACTIVITY_SERVICE, InstantiationType::Delayed);
}
