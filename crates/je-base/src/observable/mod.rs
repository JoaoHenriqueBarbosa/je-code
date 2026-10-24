//! Porte do sistema de observáveis do VS Code: `src/vs/base/common/observable.ts` e tudo em
//! `observableInternal/`. Os submódulos espelham os arquivos do original (`observables/`, `reactions/`,
//! `utils/`, `experimental/`, `logging/`); este módulo é a fachada (o `index.ts`).
//!
//! Ver [`base`] para as decisões de tradução (observável apagado vs tipado, mudança `()` como
//! `undefined`, identidade por endereço, logging como no-op) e [`deps`] para a igualdade estrita.
//!
//! Fica de fora: o logging (`logging.ts`, `ConsoleObservableLogger`, `DevToolsLogger`), `DebugLocation`,
//! `isObservable` (precisa de tipagem dinâmica) e `observableFromEvent.Observer`. O `debugGetObservableGraph`
//! está em [`logging::debug_get_dependency_graph`].

#![allow(clippy::type_complexity)]

pub mod base;
pub mod change_tracker;
pub mod debug_name;
pub mod deps;
pub mod experimental;
pub mod logging;
pub mod map;
pub mod observables;
pub mod reactions;
pub mod set;
pub mod transaction;
pub mod utils;

pub use base::{
    AnyObservable, IObservable, IObserver, IReader, IReaderWithStore, ISettableObservable, ITransaction, Observable, Settable, SettableExt,
    handle_bug_indicating_error_recovery,
};
pub use change_tracker::{IChangeContext, IChangeTracker, RecordedChanges, record_changes, record_changes_lazy};
pub use debug_name::{DebugNameOptions, DebugOwner};
pub use deps::StrictEquals;
pub use experimental::utils::{derived_const_once_defined, latest_changed_value};
pub use map::ObservableMap;
pub use observables::base_observable::{BaseObservable, FlattenExt, ObservableExt, ObservableHooks};
pub use observables::const_observable::const_observable;
pub use observables::derived::{
    DerivedOptions, IDerivedReader, derived, derived_disposable, derived_handle_changes, derived_opts, derived_with_setter, derived_with_store,
};
pub use observables::observable_from_event::{observable_from_event, observable_from_event_opts};
pub use observables::observable_signal::{ObservableSignal, observable_signal};
pub use observables::observable_signal_from_event::observable_signal_from_event;
pub use observables::observable_value::{disposable_observable_value, observable_value, observable_value_opts};
pub use reactions::autorun::{
    autorun, autorun_delta, autorun_handle_changes, autorun_iterable_delta, autorun_opts, autorun_per_keyed_item, autorun_self_disposable,
    autorun_with_store, autorun_with_store_handle_changes, register_autorun_self_disposable,
};
pub use set::ObservableSet;
pub use transaction::{TransactionImpl, async_transaction, global_transaction, subtransaction, transaction};
pub use utils::promise::{ObservableLazy, ObservableLazyPromise, ObservablePromise, ObservableResolvedPromise, PromiseResult};
pub use utils::run_on_change::{run_on_change, run_on_change_with_cancellation_token, run_on_change_with_store};
pub use utils::utils::{
    debounced_observable, debounced_observable2, derived_observable_with_cache, derived_observable_with_writable_cache, keep_observed,
    map_observable_array_cached, map_observable_array_cached_by, observable_from_promise, recompute_initially_and_on_change, signal_from_observable, throttled_observable,
    was_event_triggered_recently,
};
pub use utils::utils_cancellation::{derived_with_cancellation_token, wait_for_state};
pub use utils::value_with_change_event::{ValueWithChangeEventFromObservable, observable_from_value_with_change_event};
