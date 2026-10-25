//! Porte de `src/vs/workbench/contrib/canvases/common/canvasDefinitionStore.ts`: lista os tipos de canvas abertos que o
//! chat atual registrou e atualiza a lista quando a fonte reavalia.
//!
//! Divergências:
//! - `ICanvasDefinitionSource` é um trait (`owner()` no lugar da propriedade `owner`); `listCanvases()` devolve
//!   `Promise<Vec<ICanvasDefinition>>`.
//! - `definitions` é um `IObservable<Rc<Vec<ICanvasDefinition>>>`: o `Rc` é o objeto do JavaScript e o `equalsFn` de
//!   `structuralEquals` compara o conteúdo (`ICanvasDefinition` deriva `PartialEq`).
//! - `extends Disposable` vira um `DisposableStore` guardado no tipo; o `Delayer`, o `ThrottlerByKey` e o `autorun` entram
//!   nele. O `@ILogService` é passado ao construtor, e `logService.error(message, error)` vira `error_args` com o erro como
//!   argumento.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::{Delayer, ThrottlerByKey};
use je_base::errors::{Error, is_cancellation_error};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{
    DerivedOptions, IObservable, IReaderWithStore, Observable, SettableExt, autorun, derived, derived_opts, observable_value,
};
use je_base::promise::{MaybePromise, Promise};
use je_base::symbols::Delay;
use je_platform::log::{LogArg, LogMessage, LogServiceRef, Logger};

use super::canvas::{
    ICanvasDefinition, ICanvasOwner, canvas_owner_key, compare_canvas_definitions, is_openable_extension_canvas_definition,
};

/// The chat whose registered canvas types a [`CanvasDefinitionStore`] lists.
pub trait ICanvasDefinitionSource {
    fn owner(&self) -> ICanvasOwner;
    fn list_canvases(&self) -> Promise<Vec<ICanvasDefinition>>;
}

/// As definições abertas do dono atual, na ordem do menu.
pub type CanvasDefinitions = Rc<Vec<ICanvasDefinition>>;

/// A função `source` do construtor: lê os observáveis de que a fonte depende.
pub type CanvasDefinitionSourceFn = Rc<dyn Fn(&dyn IReaderWithStore) -> Option<Rc<dyn ICanvasDefinitionSource>>>;

/// `ICanvasDefinitionRequest`.
struct CanvasDefinitionRequest {
    owner_key: String,
    source: Rc<dyn ICanvasDefinitionSource>,
}

/// `ILoadedCanvasDefinitions`.
struct LoadedCanvasDefinitions {
    owner_key: String,
    definitions: CanvasDefinitions,
}

/// Lists the openable canvas types registered for one chat at a time, refreshing whenever `source` re-evaluates.
/// Refreshes keep the current list so commands built from it stay registered; only an owner change or an unavailable
/// source clears it.
pub struct CanvasDefinitionStore {
    /// The current owner's openable definitions in menu order, empty until a listing for that owner succeeds.
    pub definitions: IObservable<CanvasDefinitions>,
    me: Weak<Self>,
    loaded: Rc<ObservableValue<Option<Rc<LoadedCanvasDefinitions>>, ()>>,
    /// Coalesces synchronous bursts of invalidations into one refresh.
    refresh_delayer: Rc<Delayer<()>>,
    refreshes: Rc<ThrottlerByKey<String, ()>>,
    owner_key: RefCell<Option<String>>,
    log_prefix: String,
    log_service: LogServiceRef,
    store: Rc<DisposableStore>,
}

impl CanvasDefinitionStore {
    pub fn new(source: impl Fn(&dyn IReaderWithStore) -> Option<Rc<dyn ICanvasDefinitionSource>> + 'static, log_prefix: &str, log_service: LogServiceRef) -> Rc<Self> {
        let source: CanvasDefinitionSourceFn = Rc::new(source);
        let request: IObservable<Option<Rc<CanvasDefinitionRequest>>> = derived(move |reader| {
            source(reader).map(|current| Rc::new(CanvasDefinitionRequest { owner_key: canvas_owner_key(&current.owner()), source: current }))
        });

        let loaded = observable_value::<Option<Rc<LoadedCanvasDefinitions>>, ()>("loaded", None);
        let definitions: IObservable<CanvasDefinitions> = {
            let (request, loaded) = (request.clone(), loaded.clone());
            let equals_fn: Rc<dyn Fn(&CanvasDefinitions, &CanvasDefinitions) -> bool> = Rc::new(|first, second| first == second);
            derived_opts(DerivedOptions { equals_fn: Some(equals_fn), ..DerivedOptions::default() }, move |reader| {
                let owner_key = request.read(reader).map(|current| current.owner_key.clone());
                let loaded = loaded.read(reader);
                // Derived from the requested owner so readers never pair it with another owner's definitions.
                match (owner_key, loaded) {
                    (Some(owner_key), Some(loaded)) if loaded.owner_key == owner_key => loaded.definitions.clone(),
                    _ => Rc::new(Vec::new()),
                }
            })
        };

        let store = DisposableStore::new();
        let refresh_delayer = Rc::new(Delayer::<()>::new(Delay::Microtask));
        let refreshes = Rc::new(ThrottlerByKey::<String, ()>::new());
        store.add_rc(refresh_delayer.clone());
        store.add_rc(refreshes.clone());
        let this = Rc::new_cyclic(|me| Self {
            definitions,
            me: me.clone(),
            loaded,
            refresh_delayer,
            refreshes,
            owner_key: RefCell::new(None),
            log_prefix: log_prefix.to_string(),
            log_service,
            store,
        });

        let weak = Rc::downgrade(&this);
        this.store.add(autorun(move |reader| {
            let next = request.read(reader);
            if let Some(this) = weak.upgrade() {
                this.on_request(next);
            }
        }));
        this
    }

    fn on_request(&self, next: Option<Rc<CanvasDefinitionRequest>>) {
        let next_key = next.as_ref().map(|request| request.owner_key.clone());
        let changed = *self.owner_key.borrow() != next_key;
        if changed {
            *self.owner_key.borrow_mut() = next_key;
            self.loaded.set(None, None);
        }
        let Some(next) = next else {
            return;
        };
        let (weak, key) = (self.me.clone(), next.owner_key.clone());
        let refreshes = self.refreshes.clone();
        let log_service = self.log_service.clone();
        let log_prefix = self.log_prefix.clone();
        self.refresh_delayer
            .trigger(
                move || {
                    MaybePromise::Promise(refreshes.queue(key, move || match weak.upgrade() {
                        Some(this) => this.refresh(next),
                        None => Promise::resolved(()),
                    }))
                },
                None,
            )
            .catch(move |error: Error| {
                if !is_cancellation_error(&error) {
                    log_service.error_args(
                        &LogMessage::Text(format!("{log_prefix} Failed to schedule registered canvas refresh")),
                        &[LogArg::Error(error)],
                    );
                }
            });
    }

    fn refresh(&self, request: Rc<CanvasDefinitionRequest>) -> Promise<()> {
        if !self.is_current(&request) {
            return Promise::resolved(());
        }
        let (on_listed, on_failed) = (self.me.clone(), self.me.clone());
        let (listed_request, failed_request) = (request.clone(), request.clone());
        request
            .source
            .list_canvases()
            .then(move |definitions| {
                if let Some(this) = on_listed.upgrade()
                    && this.is_current(&listed_request)
                {
                    let mut definitions: Vec<ICanvasDefinition> =
                        definitions.into_iter().filter(|definition| is_openable_extension_canvas_definition(definition)).collect();
                    definitions.sort_by(compare_canvas_definitions);
                    this.loaded.set(
                        Some(Rc::new(LoadedCanvasDefinitions { owner_key: listed_request.owner_key.clone(), definitions: Rc::new(definitions) })),
                        None,
                    );
                }
            })
            .catch(move |error: Error| {
                if let Some(this) = on_failed.upgrade()
                    && this.is_current(&failed_request)
                {
                    this.log_service.error_args(
                        &LogMessage::Text(format!("{} Failed to list registered canvases", this.log_prefix)),
                        &[LogArg::Error(error)],
                    );
                }
            })
    }

    fn is_current(&self, request: &CanvasDefinitionRequest) -> bool {
        !self.store.is_disposed() && self.owner_key.borrow().as_deref() == Some(request.owner_key.as_str())
    }
}

impl IDisposable for CanvasDefinitionStore {
    fn dispose(&self) {
        self.store.dispose();
    }
}
