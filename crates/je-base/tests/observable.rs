//! Porte de `src/vs/base/test/common/observables/observable.test.ts` (suíte `observables`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `/** @description X */` não existe em Rust (ver `je_base::observable::base`), e nenhum caso depende do nome.
//! - O resumo de mudanças usa `RecordedChanges` (`record_changes`), formatado à mão onde o original usa
//!   `JSON.stringify`. O `waitForState` rejeita com um `Error` cuja mensagem é o JSON do estado.
//! - `observableValue<T, TChange>` com `T` objeto usa `Rc<_>` (a igualdade `===` é a identidade).
//! - O caso `prevent invalid usage` guarda o leitor como `Rc<Derived>`/`Rc<AutorunObserver>`, já que uma
//!   referência `&dyn IReader` não pode sair do fecho.
//! - `catches cyclic dependencies` é `test.skip` no original e fica `#[ignore]`.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event, Listener};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, plain_disposable, to_disposable};
use je_base::observable::base::{AnyObservable, IObservable, ISettableObservable, ITransaction, Observable, ObserverRef, Settable, SettableExt};
use je_base::observable::change_tracker::{FnChangeTracker, IChangeContext, RecordedChanges, record_changes};
use je_base::observable::debug_name::{DebugNameOptions, DebugOwner};
use je_base::observable::deps::StrictEquals;
use je_base::observable::observables::base_observable::{BaseObservable, ObservableExt, ObservableHooks};
use je_base::observable::observables::derived::{Derived, DerivedHandleChangesOptions, IDerivedReader, derived, derived_disposable, derived_handle_changes};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::reactions::autorun::{
    autorun, autorun_handle_changes, autorun_per_keyed_item, autorun_with_store_handle_changes,
};
use je_base::observable::reactions::autorun_impl::AutorunObserver;
use je_base::observable::experimental::reducer::{ReducerOptions, observable_reducer};
use je_base::observable::observables::derived::NoTracker;
use je_base::observable::utils::run_on_change::run_on_change;
use je_base::observable::utils::utils::keep_observed;
use je_base::observable::utils::utils_cancellation::wait_for_state;
use je_base::observable::{
    observable_from_event, observable_signal, observable_value as new_observable_value, transaction,
};

macro_rules! clones {
    ($($n:ident),* $(,)?) => { $(let $n = $n.clone();)* };
}

macro_rules! assert_log {
    ($log:expr, [$($e:expr),* $(,)?]) => {{
        let expected: Vec<String> = vec![$($e.to_string()),*];
        assert_eq!($log.get_and_clear_entries(), expected);
    }};
}

#[derive(Clone, Default)]
struct Log(Rc<RefCell<Vec<String>>>);

impl Log {
    fn log(&self, message: impl Into<String>) {
        self.0.borrow_mut().push(message.into());
    }

    fn get_and_clear_entries(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.borrow_mut())
    }
}

fn ov<T: Clone + StrictEquals + 'static>(name: &str, value: T) -> Rc<ObservableValue<T, ()>> {
    new_observable_value(name, value)
}

fn js_opt(v: Option<i32>) -> String {
    v.map_or_else(|| "undefined".into(), |v| v.to_string())
}

/// O `${value}` do JavaScript.
trait JsStr {
    fn js_str(&self) -> String;
}

impl JsStr for i32 {
    fn js_str(&self) -> String {
        self.to_string()
    }
}

impl JsStr for bool {
    fn js_str(&self) -> String {
        self.to_string()
    }
}

struct State {
    state: &'static str,
}

impl JsStr for Rc<State> {
    fn js_str(&self) -> String {
        "[object Object]".into()
    }
}

/// `LoggingObservableValue`: `BaseObservable` com os ganchos e o `get`/`set` registrados no log.
struct LoggingObservableValue<T: Clone + StrictEquals + JsStr + 'static> {
    base: BaseObservable,
    me: Weak<Self>,
    debug_name: String,
    value: RefCell<T>,
    logger: Log,
}

impl<T: Clone + StrictEquals + JsStr + 'static> LoggingObservableValue<T> {
    fn new(debug_name: &str, initial: T, logger: &Log) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            base: BaseObservable::new(me.clone() as Weak<dyn AnyObservable>),
            me: me.clone(),
            debug_name: debug_name.to_owned(),
            value: RefCell::new(initial),
            logger: logger.clone(),
        })
    }
}

impl<T: Clone + StrictEquals + JsStr + 'static> ObservableHooks for LoggingObservableValue<T> {
    fn on_first_observer_added(&self) {
        self.logger.log(format!("{}.firstObserverAdded", self.debug_name));
    }

    fn on_last_observer_removed(&self) {
        self.logger.log(format!("{}.lastObserverRemoved", self.debug_name));
    }
}

impl<T: Clone + StrictEquals + JsStr + 'static> AnyObservable for LoggingObservableValue<T> {
    fn report_changes(&self) {
        self.get();
    }

    fn add_observer(&self, observer: ObserverRef) {
        self.base.add_observer(observer, self);
    }

    fn remove_observer(&self, observer: &ObserverRef) {
        self.base.remove_observer(observer, self);
    }

    fn debug_name(&self) -> String {
        self.debug_name.clone()
    }

    fn to_any_observable(&self) -> Rc<dyn AnyObservable> {
        self.base.to_any_observable()
    }
}

impl<T: Clone + StrictEquals + JsStr + 'static> Observable<T> for LoggingObservableValue<T> {
    fn get(&self) -> T {
        self.logger.log(format!("{}.get", self.debug_name));
        self.value.borrow().clone()
    }

    fn to_observable(&self) -> IObservable<T> {
        self.me.upgrade().expect("vivo")
    }
}

impl<T: Clone + StrictEquals + JsStr + 'static> Settable<T, ()> for LoggingObservableValue<T> {
    fn set_with_change(&self, value: T, tx: Option<&dyn ITransaction>, change: ()) {
        if self.value.borrow().strict_equals(&value) {
            return;
        }
        let Some(tx) = tx else {
            transaction(|tx| self.set_with_change(value, Some(tx), change));
            return;
        };
        self.logger.log(format!("{}.set (value {})", self.debug_name, value.js_str()));
        *self.value.borrow_mut() = value;
        let me = self.base.to_any_observable();
        self.base.for_each_observer(|observer| {
            tx.update_observer(observer.clone(), me.clone());
            observer.handle_change(&*me, &change);
        });
    }

    fn to_settable(&self) -> ISettableObservable<T, ()> {
        self.me.upgrade().expect("vivo")
    }
}

/// O `JSON.stringify` do resumo de `recordChanges` com deltas e valores `i32`.
fn json_summary(summary: &RecordedChanges, keys: &[&str]) -> String {
    let changes: Vec<String> = summary
        .changes
        .borrow()
        .iter()
        .map(|c| format!("{{\"key\":\"{}\",\"change\":{}}}", c.key, c.change::<i32>().copied().unwrap_or_default()))
        .collect();
    let mut out = format!("{{\"changes\":[{}]", changes.join(","));
    for key in keys {
        out.push_str(&format!(",\"{key}\":{}", summary.value::<i32>(key).expect("valor lido")));
    }
    out.push('}');
    out
}

/// Reader tutorial: ler os testes para entender como usar observáveis.
mod tutorial {
    use super::*;

    #[test]
    fn observable_and_autorun() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let log = Log::default();
        // Cria uma variável cujas mudanças podem ser observadas. O nome só serve para debug.
        let my_observable = ov("myObservable", 0);

        {
            clones!(log, my_observable);
            // O autorun roda já e de novo sempre que uma dependência muda. Precisa ser descartado.
            ds.add(autorun(move |reader| {
                // `observable.get()` só devolve o valor; `read(reader)` também se inscreve.
                log.log(format!("myAutorun.run(myObservable: {})", my_observable.read(reader)));
            }));
        }
        assert_log!(log, ["myAutorun.run(myObservable: 0)"]);

        my_observable.set(1, None);
        assert_log!(log, ["myAutorun.run(myObservable: 1)"]);

        my_observable.set(1, None);
        // O valor não mudou, então o autorun não roda.
        assert_log!(log, []);

        // Transações agrupam as execuções dos autoruns.
        transaction(|tx| {
            my_observable.set(2, Some(tx));
            assert_log!(log, []);

            my_observable.set(3, Some(tx));
            assert_log!(log, []);
        });
        assert_log!(log, ["myAutorun.run(myObservable: 3)"]);
        // O autorun não viu o valor intermediário `2`.
    }

    #[test]
    fn derived_and_autorun() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let log = Log::default();
        let observable1 = ov("myObservable1", 0);
        let observable2 = ov("myObservable2", 0);

        let my_derived = {
            clones!(log, observable1, observable2);
            derived(move |reader| {
                let value1 = observable1.read(reader);
                let value2 = observable2.read(reader);
                let sum = value1 + value2;
                log.log(format!("myDerived.recompute: {value1} + {value2} = {sum}"));
                sum
            })
        };

        {
            clones!(log, my_derived);
            ds.add(autorun(move |reader| {
                log.log(format!("myAutorun(myDerived: {})", my_derived.read(reader)));
            }));
        }
        assert_log!(log, ["myDerived.recompute: 0 + 0 = 0", "myAutorun(myDerived: 0)"]);

        observable1.set(1, None);
        assert_log!(log, ["myDerived.recompute: 1 + 0 = 1", "myAutorun(myDerived: 1)"]);

        observable2.set(1, None);
        assert_log!(log, ["myDerived.recompute: 1 + 1 = 2", "myAutorun(myDerived: 2)"]);

        transaction(|tx| {
            observable1.set(5, Some(tx));
            assert_log!(log, []);

            observable2.set(5, Some(tx));
            assert_log!(log, []);
        });
        assert_log!(log, ["myDerived.recompute: 5 + 5 = 10", "myAutorun(myDerived: 10)"]);

        transaction(|tx| {
            observable1.set(6, Some(tx));
            assert_log!(log, []);

            observable2.set(4, Some(tx));
            assert_log!(log, []);
        });
        // O autorun não rodou de novo: a dependência foi de 10 para 10.
        assert_log!(log, ["myDerived.recompute: 6 + 4 = 10"]);
    }

    #[test]
    fn read_during_transaction() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let log = Log::default();
        let observable1 = ov("myObservable1", 0);
        let observable2 = ov("myObservable2", 0);

        let my_derived = {
            clones!(log, observable1, observable2);
            derived(move |reader| {
                let value1 = observable1.read(reader);
                let value2 = observable2.read(reader);
                let sum = value1 + value2;
                log.log(format!("myDerived.recompute: {value1} + {value2} = {sum}"));
                sum
            })
        };

        {
            clones!(log, my_derived);
            ds.add(autorun(move |reader| {
                log.log(format!("myAutorun(myDerived: {})", my_derived.read(reader)));
            }));
        }
        assert_log!(log, ["myDerived.recompute: 0 + 0 = 0", "myAutorun(myDerived: 0)"]);

        transaction(|tx| {
            observable1.set(-10, Some(tx));
            assert_log!(log, []);

            my_derived.get(); // Força uma recomputação síncrona do valor atual.
            assert_log!(log, ["myDerived.recompute: -10 + 0 = -10"]);

            observable2.set(10, Some(tx));
            assert_log!(log, []);
        });
        // O autorun roda de novo: a dependência foi de 0 para -10 e voltou a 0.
        assert_log!(log, ["myDerived.recompute: -10 + 10 = 0", "myAutorun(myDerived: 0)"]);
    }

    #[test]
    fn get_without_observers() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let log = Log::default();
        let observable1 = ov("myObservableValue1", 0);

        let computed1 = {
            clones!(log, observable1);
            derived(move |reader| {
                let value1 = observable1.read(reader);
                let result = value1 % 3;
                log.log(format!("recompute1: {value1} % 3 = {result}"));
                result
            })
        };
        let computed2 = {
            clones!(log, computed1);
            derived(move |reader| {
                let value1 = computed1.read(reader);
                let result = value1 * 2;
                log.log(format!("recompute2: {value1} * 2 = {result}"));
                result
            })
        };
        let computed3 = {
            clones!(log, computed1);
            derived(move |reader| {
                let value1 = computed1.read(reader);
                let result = value1 * 3;
                log.log(format!("recompute3: {value1} * 3 = {result}"));
                result
            })
        };
        let computed_sum = {
            clones!(log, computed2, computed3);
            derived(move |reader| {
                let value1 = computed2.read(reader);
                let value2 = computed3.read(reader);
                let result = value1 + value2;
                log.log(format!("recompute4: {value1} + {value2} = {result}"));
                result
            })
        };
        assert_log!(log, []);

        observable1.set(1, None);
        assert_log!(log, []);

        log.log(format!("value: {}", computed_sum.get()));
        assert_log!(log, ["recompute1: 1 % 3 = 1", "recompute2: 1 * 2 = 2", "recompute3: 1 * 3 = 3", "recompute4: 2 + 3 = 5", "value: 5"]);

        log.log(format!("value: {}", computed_sum.get()));
        // Sem observadores, os derived não ficam em cache: são calculados do zero.
        assert_log!(log, ["recompute1: 1 % 3 = 1", "recompute2: 1 * 2 = 2", "recompute3: 1 * 3 = 3", "recompute4: 2 + 3 = 5", "value: 5"]);

        let disposable = keep_observed(&computed_sum); // `keep_observed` mantém o cache.
        log.log(format!("value: {}", computed_sum.get()));
        assert_log!(log, ["recompute1: 1 % 3 = 1", "recompute2: 1 * 2 = 2", "recompute3: 1 * 3 = 3", "recompute4: 2 + 3 = 5", "value: 5"]);

        log.log(format!("value: {}", computed_sum.get()));
        assert_log!(log, ["value: 5"]);

        observable1.set(2, None);
        // `keep_observed` não força a recomputação: os derived continuam preguiçosos.
        assert_log!(log, []);

        log.log(format!("value: {}", computed_sum.get()));
        assert_log!(log, ["recompute1: 2 % 3 = 2", "recompute2: 2 * 2 = 4", "recompute3: 2 * 3 = 6", "recompute4: 4 + 6 = 10", "value: 10"]);
        log.log(format!("value: {}", computed_sum.get()));
        assert_log!(log, ["value: 10"]);

        disposable.dispose(); // Não esqueça de descartar para não vazar memória.

        log.log(format!("value: {}", computed_sum.get()));
        assert_log!(log, ["recompute1: 2 % 3 = 2", "recompute2: 2 * 2 = 4", "recompute3: 2 * 3 = 6", "recompute4: 4 + 6 = 10", "value: 10"]);

        log.log(format!("value: {}", computed_sum.get()));
        assert_log!(log, ["recompute1: 2 % 3 = 2", "recompute2: 2 * 2 = 4", "recompute3: 2 * 3 = 6", "recompute4: 4 + 6 = 10", "value: 10"]);
    }

    #[derive(Clone)]
    struct Msg {
        msg: String,
    }

    struct Msgs {
        msgs: RefCell<Vec<String>>,
    }

    #[test]
    fn autorun_that_receives_deltas_of_signals() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let log = Log::default();

        // Um signal é um observável sem valor que leva informação de mudança ao ser disparado.
        let signal = observable_signal::<Msg>("signal");

        let tracker = {
            clones!(signal);
            FnChangeTracker::new(
                |_| Msgs { msgs: RefCell::new(Vec::new()) },
                move |context: &dyn IChangeContext, change_summary: &Msgs| {
                    if context.did_change(&*signal) {
                        change_summary.msgs.borrow_mut().push(context.change::<Msg>().expect("o delta").msg.clone());
                    }
                    true // Queremos tratar a mudança.
                },
            )
        };
        let disposable = {
            clones!(log, signal);
            autorun_handle_changes(DebugNameOptions::default(), tracker, move |reader, change_summary: &Msgs| {
                // Ao tratar a mudança, leia o signal!
                signal.read(reader);
                log.log(format!("msgs: {}", change_summary.msgs.borrow().join(", ")));
            })
        };

        signal.trigger(None, Msg { msg: "foobar".into() });

        transaction(|tx| {
            // Dá para agrupar disparos; nenhum delta se perde.
            signal.trigger(Some(tx), Msg { msg: "hello".into() });
            signal.trigger(Some(tx), Msg { msg: "world".into() });
        });

        assert_log!(log, ["msgs: ", "msgs: foobar", "msgs: hello, world"]);

        disposable.dispose();
    }
}

#[test]
fn topological_order() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let log = Log::default();
    let my_observable1 = ov("myObservable1", 0);
    let my_observable2 = ov("myObservable2", 0);

    let my_computed1 = {
        clones!(log, my_observable1, my_observable2);
        derived(move |reader| {
            let value1 = my_observable1.read(reader);
            let value2 = my_observable2.read(reader);
            let sum = value1 + value2;
            log.log(format!("myComputed1.recompute(myObservable1: {value1} + myObservable2: {value2} = {sum})"));
            sum
        })
    };

    let my_computed2 = {
        clones!(log, my_computed1, my_observable1, my_observable2);
        derived(move |reader| {
            let value1 = my_computed1.read(reader);
            let value2 = my_observable1.read(reader);
            let value3 = my_observable2.read(reader);
            let sum = value1 + value2 + value3;
            log.log(format!("myComputed2.recompute(myComputed1: {value1} + myObservable1: {value2} + myObservable2: {value3} = {sum})"));
            sum
        })
    };

    let my_computed3 = {
        clones!(log, my_computed2, my_observable1, my_observable2);
        derived(move |reader| {
            let value1 = my_computed2.read(reader);
            let value2 = my_observable1.read(reader);
            let value3 = my_observable2.read(reader);
            let sum = value1 + value2 + value3;
            log.log(format!("myComputed3.recompute(myComputed2: {value1} + myObservable1: {value2} + myObservable2: {value3} = {sum})"));
            sum
        })
    };

    {
        clones!(log, my_computed3);
        ds.add(autorun(move |reader| {
            log.log(format!("myAutorun.run(myComputed3: {})", my_computed3.read(reader)));
        }));
    }
    assert_log!(
        log,
        [
            "myComputed1.recompute(myObservable1: 0 + myObservable2: 0 = 0)",
            "myComputed2.recompute(myComputed1: 0 + myObservable1: 0 + myObservable2: 0 = 0)",
            "myComputed3.recompute(myComputed2: 0 + myObservable1: 0 + myObservable2: 0 = 0)",
            "myAutorun.run(myComputed3: 0)",
        ]
    );

    my_observable1.set(1, None);
    assert_log!(
        log,
        [
            "myComputed1.recompute(myObservable1: 1 + myObservable2: 0 = 1)",
            "myComputed2.recompute(myComputed1: 1 + myObservable1: 1 + myObservable2: 0 = 2)",
            "myComputed3.recompute(myComputed2: 2 + myObservable1: 1 + myObservable2: 0 = 3)",
            "myAutorun.run(myComputed3: 3)",
        ]
    );

    transaction(|tx| {
        my_observable1.set(2, Some(tx));
        my_computed2.get();
        assert_log!(
            log,
            [
                "myComputed1.recompute(myObservable1: 2 + myObservable2: 0 = 2)",
                "myComputed2.recompute(myComputed1: 2 + myObservable1: 2 + myObservable2: 0 = 4)",
            ]
        );

        my_observable1.set(3, Some(tx));
        my_computed2.get();
        assert_log!(
            log,
            [
                "myComputed1.recompute(myObservable1: 3 + myObservable2: 0 = 3)",
                "myComputed2.recompute(myComputed1: 3 + myObservable1: 3 + myObservable2: 0 = 6)",
            ]
        );
    });
    assert_log!(
        log,
        ["myComputed3.recompute(myComputed2: 6 + myObservable1: 3 + myObservable2: 0 = 9)", "myAutorun.run(myComputed3: 9)"]
    );
}

mod from_event {
    use super::*;

    struct Init {
        log: Log,
        set_value: Box<dyn Fn(Option<i32>)>,
        observable: IObservable<Option<i32>>,
    }

    fn init() -> Init {
        let log = Log::default();
        let value: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(Some(0)));
        let event_emitter = Emitter::<()>::new();
        let id = Rc::new(Cell::new(0));

        let event = {
            clones!(log, event_emitter, id);
            Event::<()>::new(move |handler: Listener<()>| {
                let cur_id = id.get();
                id.set(cur_id + 1);
                log.log(format!("subscribed handler {cur_id}"));
                let disposable = event_emitter.event().listen_rc(handler);
                let log = log.clone();
                plain_disposable(move || {
                    log.log(format!("unsubscribed handler {cur_id}"));
                    disposable.dispose();
                })
            })
        };
        let observable = {
            clones!(log, value);
            observable_from_event(event, move |_: Option<&()>| {
                log.log(format!("compute value {}", js_opt(value.get())));
                value.get()
            })
        };

        Init {
            log,
            set_value: Box::new(move |new_value| {
                value.set(new_value);
                event_emitter.fire(());
            }),
            observable,
        }
    }

    #[test]
    fn handle_undefined() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let Init { log, set_value, observable } = init();

        set_value(None);

        let autorun_disposable = {
            clones!(log, observable);
            autorun(move |reader| {
                observable.read(reader);
                log.log(format!("autorun, value: {}", js_opt(observable.read(reader))));
            })
        };

        assert_log!(log, ["subscribed handler 0", "compute value undefined", "autorun, value: undefined"]);

        set_value(Some(1));

        assert_log!(log, ["compute value 1", "autorun, value: 1"]);

        autorun_disposable.dispose();

        assert_log!(log, ["unsubscribed handler 0"]);
    }

    #[test]
    fn basic() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let Init { log, set_value, observable } = init();

        let should_read_observable = ov("shouldReadObservable", true);

        let autorun_disposable = {
            clones!(log, observable, should_read_observable);
            autorun(move |reader| {
                if should_read_observable.read(reader) {
                    observable.read(reader);
                    log.log(format!("autorun, should read: true, value: {}", js_opt(observable.read(reader))));
                } else {
                    log.log("autorun, should read: false");
                }
            })
        };
        assert_log!(log, ["subscribed handler 0", "compute value 0", "autorun, should read: true, value: 0"]);

        // `get` em cache.
        log.log(format!("get value: {}", js_opt(observable.get())));
        assert_log!(log, ["get value: 0"]);

        set_value(Some(1));
        // Dispara o autorun, sem se desinscrever nem inscrever de novo.
        assert_log!(log, ["compute value 1", "autorun, should read: true, value: 1"]);

        // Desinscreve quando não é lido.
        should_read_observable.set(false, None);
        assert_log!(log, ["autorun, should read: false", "unsubscribed handler 0"]);

        should_read_observable.set(true, None);
        assert_log!(log, ["subscribed handler 1", "compute value 1", "autorun, should read: true, value: 1"]);

        autorun_disposable.dispose();
        assert_log!(log, ["unsubscribed handler 1"]);
    }

    #[test]
    fn get_without_observers() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let Init { log, observable, .. } = init();
        assert_log!(log, []);

        log.log(format!("get value: {}", js_opt(observable.get())));
        // Sem cache nem inscrição.
        assert_log!(log, ["compute value 0", "get value: 0"]);

        log.log(format!("get value: {}", js_opt(observable.get())));
        assert_log!(log, ["compute value 0", "get value: 0"]);
    }

    #[test]
    fn last_observer_removed_while_handling_event() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let Init { log, set_value, observable } = init();
        let first_value: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(None));
        let first_observer: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
        let created = {
            clones!(observable, first_value, first_observer);
            autorun(move |reader| {
                let v = observable.read(reader);
                first_value.set(v);
                if v == Some(1) {
                    let current = first_observer.borrow().clone();
                    if let Some(c) = current {
                        c.dispose();
                    }
                }
            })
        };
        *first_observer.borrow_mut() = Some(created);

        assert_eq!((first_value.get(), log.get_and_clear_entries()), (Some(0), vec!["subscribed handler 0".to_string(), "compute value 0".to_string()]));

        set_value(Some(1));
        assert_eq!((first_value.get(), log.get_and_clear_entries()), (Some(1), vec!["compute value 1".to_string(), "unsubscribed handler 0".to_string()]));

        let second_value: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(None));
        let second_observer = {
            clones!(observable, second_value);
            autorun(move |reader| second_value.set(observable.read(reader)))
        };

        assert_eq!((second_value.get(), log.get_and_clear_entries()), (Some(1), vec!["subscribed handler 1".to_string(), "compute value 1".to_string()]));

        second_observer.dispose();
        assert_log!(log, ["unsubscribed handler 1"]);
    }
}

#[test]
fn reading_derived_in_transaction_unsubscribes_unnecessary_observables() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let log = Log::default();

    let should_read_observable = ov("shouldReadMyObs1", true);
    let my_obs1 = LoggingObservableValue::new("myObs1", 0, &log);
    let my_computed = {
        clones!(log, should_read_observable, my_obs1);
        derived(move |reader| {
            log.log("myComputed.recompute");
            if should_read_observable.read(reader) {
                return my_obs1.read(reader);
            }
            1
        })
    };
    {
        clones!(log, my_computed);
        ds.add(autorun(move |reader| {
            let value = my_computed.read(reader);
            log.log(format!("myAutorun: {value}"));
        }));
    }
    assert_log!(log, ["myComputed.recompute", "myObs1.firstObserverAdded", "myObs1.get", "myAutorun: 0"]);

    transaction(|tx| {
        my_obs1.set(1, Some(tx));
        assert_log!(log, ["myObs1.set (value 1)"]);

        should_read_observable.set(false, Some(tx));
        assert_log!(log, []);

        my_computed.get();
        assert_log!(log, ["myComputed.recompute", "myObs1.lastObserverRemoved"]);
    });
    assert_log!(log, ["myAutorun: 1"]);
}

#[test]
fn avoid_recomputation_of_deriveds_that_are_no_longer_read() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let log = Log::default();

    let my_obs_should_read = LoggingObservableValue::new("myObsShouldRead", true, &log);
    let my_obs1 = LoggingObservableValue::new("myObs1", 0, &log);

    let my_computed1 = {
        clones!(log, my_obs1);
        derived(move |reader| {
            let my_obs1_val = my_obs1.read(reader);
            let result = my_obs1_val % 10;
            log.log(format!("myComputed1(myObs1: {my_obs1_val}): Computed {result}"));
            my_obs1_val
        })
    };

    {
        clones!(log, my_obs_should_read, my_computed1);
        ds.add(autorun(move |reader| {
            let should_read = my_obs_should_read.read(reader);
            if should_read {
                let v = my_computed1.read(reader);
                log.log(format!("myAutorun(shouldRead: true, myComputed1: {v}): run"));
            } else {
                log.log("myAutorun(shouldRead: false): run");
            }
        }));
    }
    assert_log!(
        log,
        [
            "myObsShouldRead.firstObserverAdded",
            "myObsShouldRead.get",
            "myObs1.firstObserverAdded",
            "myObs1.get",
            "myComputed1(myObs1: 0): Computed 0",
            "myAutorun(shouldRead: true, myComputed1: 0): run",
        ]
    );

    transaction(|tx| {
        my_obs_should_read.set(false, Some(tx));
        my_obs1.set(1, Some(tx));
        assert_log!(log, ["myObsShouldRead.set (value false)", "myObs1.set (value 1)"]);
    });
    // `myComputed1` não é recomputado aqui, mesmo que a dependência `myObs1` tenha mudado.
    assert_log!(log, ["myObsShouldRead.get", "myAutorun(shouldRead: false): run", "myObs1.lastObserverRemoved"]);

    transaction(|tx| {
        my_obs_should_read.set(true, Some(tx));
        assert_log!(log, ["myObsShouldRead.set (value true)"]);
    });
    assert_log!(
        log,
        [
            "myObsShouldRead.get",
            "myObs1.firstObserverAdded",
            "myObs1.get",
            "myComputed1(myObs1: 1): Computed 1",
            "myAutorun(shouldRead: true, myComputed1: 1): run",
        ]
    );
}

mod autorun_rerun_on_neutral_change {
    use super::*;

    #[test]
    fn autorun_reruns_on_neutral_observable_double_change() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let log = Log::default();
        let my_observable = ov("myObservable", 0);

        {
            clones!(log, my_observable);
            ds.add(autorun(move |reader| {
                log.log(format!("myAutorun.run(myObservable: {})", my_observable.read(reader)));
            }));
        }
        assert_log!(log, ["myAutorun.run(myObservable: 0)"]);

        transaction(|tx| {
            my_observable.set(2, Some(tx));
            assert_log!(log, []);

            my_observable.set(0, Some(tx));
            assert_log!(log, []);
        });
        assert_log!(log, ["myAutorun.run(myObservable: 0)"]);
    }

    #[test]
    fn autorun_does_not_rerun_on_indirect_neutral_observable_double_change() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let log = Log::default();
        let my_observable = ov("myObservable", 0);
        let my_derived = {
            clones!(log, my_observable);
            derived(move |reader| {
                let val = my_observable.read(reader);
                log.log(format!("myDerived.read(myObservable: {val})"));
                val
            })
        };

        {
            clones!(log, my_derived);
            ds.add(autorun(move |reader| {
                log.log(format!("myAutorun.run(myDerived: {})", my_derived.read(reader)));
            }));
        }
        assert_log!(log, ["myDerived.read(myObservable: 0)", "myAutorun.run(myDerived: 0)"]);

        transaction(|tx| {
            my_observable.set(2, Some(tx));
            assert_log!(log, []);

            my_observable.set(0, Some(tx));
            assert_log!(log, []);
        });
        assert_log!(log, ["myDerived.read(myObservable: 0)"]);
    }

    #[test]
    fn autorun_reruns_on_indirect_neutral_observable_double_change_when_changes_propagate() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let log = Log::default();
        let my_observable = ov("myObservable", 0);
        let my_derived = {
            clones!(log, my_observable);
            derived(move |reader| {
                let val = my_observable.read(reader);
                log.log(format!("myDerived.read(myObservable: {val})"));
                val
            })
        };

        {
            clones!(log, my_derived);
            ds.add(autorun(move |reader| {
                log.log(format!("myAutorun.run(myDerived: {})", my_derived.read(reader)));
            }));
        }
        assert_log!(log, ["myDerived.read(myObservable: 0)", "myAutorun.run(myDerived: 0)"]);

        transaction(|tx| {
            my_observable.set(2, Some(tx));
            assert_log!(log, []);

            my_derived.get(); // Isto marca o autorun como mudado.
            assert_log!(log, ["myDerived.read(myObservable: 2)"]);

            my_observable.set(0, Some(tx));
            assert_log!(log, []);
        });
        assert_log!(log, ["myDerived.read(myObservable: 0)", "myAutorun.run(myDerived: 0)"]);
    }
}

#[test]
fn self_disposing_autorun() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let log = Log::default();

    let observable1 = LoggingObservableValue::new("myObservable1", 0, &log);
    let my_observable2 = LoggingObservableValue::new("myObservable2", 0, &log);
    let my_observable3 = LoggingObservableValue::new("myObservable3", 0, &log);

    let d_slot: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
    let d = {
        clones!(log, observable1, my_observable2, my_observable3, d_slot);
        autorun(move |reader| {
            if observable1.read(reader) >= 2 {
                assert_log!(log, ["myObservable1.set (value 2)", "myObservable1.get"]);

                my_observable2.read(reader);
                // Primeira vez que este observável é lido.
                assert_log!(log, ["myObservable2.firstObserverAdded", "myObservable2.get"]);

                let current = d_slot.borrow().clone().expect("já atribuído");
                current.dispose();
                // Descartar remove todos os observadores.
                assert_log!(log, ["myObservable1.lastObserverRemoved", "myObservable2.lastObserverRemoved"]);

                my_observable3.read(reader);
                // Não inscreve o observável, porque o autorun está descartado.
                assert_log!(log, ["myObservable3.get"]);
            }
        })
    };
    *d_slot.borrow_mut() = Some(d);
    assert_log!(log, ["myObservable1.firstObserverAdded", "myObservable1.get"]);

    observable1.set(1, None);
    assert_log!(log, ["myObservable1.set (value 1)", "myObservable1.get"]);

    observable1.set(2, None);
    // Ver as asserções no autorun.
    assert_log!(log, []);
}

#[test]
fn changing_observables_in_end_update() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let log = Log::default();

    let my_observable1 = LoggingObservableValue::new("myObservable1", 0, &log);
    let my_observable2 = LoggingObservableValue::new("myObservable2", 0, &log);

    let my_derived1 = {
        clones!(log, my_observable1);
        derived(move |reader| {
            let val = my_observable1.read(reader);
            log.log(format!("myDerived1.read(myObservable: {val})"));
            val
        })
    };

    let my_derived2 = {
        clones!(log, my_observable2, my_derived1);
        derived(move |reader| {
            let val = my_observable2.read(reader);
            if val == 1 {
                my_derived1.read(reader);
            }
            log.log(format!("myDerived2.read(myObservable: {val})"));
            val
        })
    };

    {
        clones!(log, my_derived1, my_derived2);
        ds.add(autorun(move |reader| {
            let my_derived1_val = my_derived1.read(reader);
            let my_derived2_val = my_derived2.read(reader);
            log.log(format!("myAutorun.run(myDerived1: {my_derived1_val}, myDerived2: {my_derived2_val})"));
        }));
    }

    transaction(|tx| {
        my_observable2.set(1, Some(tx));
        // O fim da atualização deste observável dispara `end_update` do `myDerived1` e do autorun, e o
        // autorun acrescenta `myDerived2` como observador do `myDerived1`.
        my_observable1.set(1, Some(tx));
    });
}

#[test]
fn set_dependency_in_derived() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let log = Log::default();

    let my_observable = LoggingObservableValue::new("myObservable", 0, &log);
    let my_computed = {
        clones!(log, my_observable);
        derived(move |reader| {
            let mut value = my_observable.read(reader);
            let orig_value = value;
            log.log(format!("myComputed(myObservable: {orig_value}): start computing"));
            if value % 3 != 0 {
                value += 1;
                my_observable.set(value, None);
            }
            log.log(format!("myComputed(myObservable: {orig_value}): finished computing"));
            value
        })
    };

    {
        clones!(log, my_computed);
        ds.add(autorun(move |reader| {
            let value = my_computed.read(reader);
            log.log(format!("myAutorun(myComputed: {value})"));
        }));
    }
    assert_log!(
        log,
        [
            "myObservable.firstObserverAdded",
            "myObservable.get",
            "myComputed(myObservable: 0): start computing",
            "myComputed(myObservable: 0): finished computing",
            "myAutorun(myComputed: 0)",
        ]
    );

    my_observable.set(1, None);
    assert_log!(
        log,
        [
            "myObservable.set (value 1)",
            "myObservable.get",
            "myComputed(myObservable: 1): start computing",
            "myObservable.set (value 2)",
            "myComputed(myObservable: 1): finished computing",
            "myObservable.get",
            "myComputed(myObservable: 2): start computing",
            "myObservable.set (value 3)",
            "myComputed(myObservable: 2): finished computing",
            "myObservable.get",
            "myComputed(myObservable: 3): start computing",
            "myComputed(myObservable: 3): finished computing",
            "myAutorun(myComputed: 3)",
        ]
    );
}

#[test]
fn set_dependency_in_autorun() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let log = Log::default();
    let my_observable = LoggingObservableValue::new("myObservable", 0, &log);

    {
        clones!(log, my_observable);
        ds.add(autorun(move |reader| {
            let value = my_observable.read(reader);
            log.log(format!("myAutorun(myObservable: {value}): start"));
            if value != 0 && value < 4 {
                my_observable.set(value + 1, None);
            }
            log.log(format!("myAutorun(myObservable: {value}): end"));
        }));
    }
    assert_log!(
        log,
        ["myObservable.firstObserverAdded", "myObservable.get", "myAutorun(myObservable: 0): start", "myAutorun(myObservable: 0): end"]
    );

    my_observable.set(1, None);
    assert_log!(
        log,
        [
            "myObservable.set (value 1)",
            "myObservable.get",
            "myAutorun(myObservable: 1): start",
            "myObservable.set (value 2)",
            "myAutorun(myObservable: 1): end",
            "myObservable.get",
            "myAutorun(myObservable: 2): start",
            "myObservable.set (value 3)",
            "myAutorun(myObservable: 2): end",
            "myObservable.get",
            "myAutorun(myObservable: 3): start",
            "myObservable.set (value 4)",
            "myAutorun(myObservable: 3): end",
            "myObservable.get",
            "myAutorun(myObservable: 4): start",
            "myAutorun(myObservable: 4): end",
        ]
    );
}

#[test]
fn get_in_transaction_between_sets() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let log = Log::default();
    let my_observable = LoggingObservableValue::new("myObservable", 0, &log);

    let my_derived1 = {
        clones!(log, my_observable);
        derived(move |reader| {
            let value = my_observable.read(reader);
            log.log(format!("myDerived1(myObservable: {value}): start computing"));
            value
        })
    };

    let my_derived2 = {
        clones!(log, my_derived1);
        derived(move |reader| {
            let value = my_derived1.read(reader);
            log.log(format!("myDerived2(myDerived1: {value}): start computing"));
            value
        })
    };

    {
        clones!(log, my_derived2);
        ds.add(autorun(move |reader| {
            let value = my_derived2.read(reader);
            log.log(format!("myAutorun(myDerived2: {value})"));
        }));
    }
    assert_log!(
        log,
        [
            "myObservable.firstObserverAdded",
            "myObservable.get",
            "myDerived1(myObservable: 0): start computing",
            "myDerived2(myDerived1: 0): start computing",
            "myAutorun(myDerived2: 0)",
        ]
    );

    transaction(|tx| {
        my_observable.set(1, Some(tx));
        assert_log!(log, ["myObservable.set (value 1)"]);

        my_derived2.get();
        assert_log!(log, ["myObservable.get", "myDerived1(myObservable: 1): start computing", "myDerived2(myDerived1: 1): start computing"]);

        my_observable.set(2, Some(tx));
        assert_log!(log, ["myObservable.set (value 2)"]);
    });
    assert_log!(
        log,
        [
            "myObservable.get",
            "myDerived1(myObservable: 2): start computing",
            "myDerived2(myDerived1: 2): start computing",
            "myAutorun(myDerived2: 2)",
        ]
    );
}

#[test]
fn bug_dont_reset_states() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let log = Log::default();
    let my_observable1 = LoggingObservableValue::new("myObservable1", 0, &log);

    let my_observable2 = LoggingObservableValue::new("myObservable2", 0, &log);
    let my_derived2 = {
        clones!(log, my_observable2);
        derived(move |reader| {
            let val = my_observable2.read(reader);
            log.log(format!("myDerived2.computed(myObservable2: {val})"));
            val % 10
        })
    };

    let my_derived3 = {
        clones!(log, my_observable1, my_derived2);
        derived(move |reader| {
            let val1 = my_observable1.read(reader);
            let val2 = my_derived2.read(reader);
            log.log(format!("myDerived3.computed(myDerived1: {val1}, myDerived2: {val2})"));
            format!("{val1} + {val2}")
        })
    };

    {
        clones!(log, my_derived3);
        ds.add(autorun(move |reader| {
            let val = my_derived3.read(reader);
            log.log(format!("myAutorun(myDerived3: {val})"));
        }));
    }
    assert_log!(
        log,
        [
            "myObservable1.firstObserverAdded",
            "myObservable1.get",
            "myObservable2.firstObserverAdded",
            "myObservable2.get",
            "myDerived2.computed(myObservable2: 0)",
            "myDerived3.computed(myDerived1: 0, myDerived2: 0)",
            "myAutorun(myDerived3: 0 + 0)",
        ]
    );

    transaction(|tx| {
        my_observable1.set(1, Some(tx)); // Marca o myDerived3 como desatualizado.
        assert_log!(log, ["myObservable1.set (value 1)"]);

        my_observable2.set(10, Some(tx)); // Isto não é mudança: myDerived3 não deve virar possible-dependency-changed.
        assert_log!(log, ["myObservable2.set (value 10)"]);
    });
    assert_log!(
        log,
        [
            "myObservable1.get",
            "myObservable2.get",
            "myDerived2.computed(myObservable2: 10)",
            "myDerived3.computed(myDerived1: 1, myDerived2: 0)",
            "myAutorun(myDerived3: 1 + 0)",
        ]
    );
}

#[test]
fn bug_add_observable_in_end_update() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let my_observable1 = ov("myObservable1", 0);
    let my_observable2 = ov("myObservable2", 0);

    let my_derived1 = {
        clones!(my_observable1);
        derived(move |reader| my_observable1.read(reader))
    };

    let my_derived2 = {
        clones!(my_observable2);
        derived(move |reader| my_observable2.read(reader))
    };

    let my_derived_a1 = {
        clones!(my_derived1, my_derived2);
        derived(move |reader| {
            let d1 = my_derived1.read(reader);
            if d1 == 1 {
                // Isto acrescenta um observador enquanto o myDerived ainda está em modo de atualização.
                // Quando ele sair, o observador não pode receber mais `end_update` que `begin_update`.
                my_derived2.read(reader);
            }
        })
    };

    {
        clones!(my_derived_a1);
        ds.add(autorun(move |reader| my_derived_a1.read(reader)));
    }

    {
        clones!(my_derived2);
        ds.add(autorun(move |reader| {
            my_derived2.read(reader);
        }));
    }

    transaction(|tx| {
        my_observable1.set(1, Some(tx));
        my_observable2.set(1, Some(tx));
    });
}

#[test]
fn bug_from_observable_light_doesnt_subscribe() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let log = Log::default();
    let my_observable = LoggingObservableValue::new("myObservable", 0, &log);

    let my_derived = {
        clones!(log, my_observable);
        derived(move |reader| {
            let val = my_observable.read(reader);
            log.log(format!("myDerived.computed(myObservable2: {val})"));
            val % 10
        })
    };

    let e = Event::<()>::from_observable_light(my_derived.to_any_observable());
    log.log("event created");
    let subscription = {
        clones!(log);
        e.listen(move |_| log.log("event fired"))
    };

    my_observable.set(1, None);

    assert_log!(
        log,
        [
            "event created",
            "myObservable.firstObserverAdded",
            "myObservable.get",
            "myDerived.computed(myObservable2: 0)",
            "myObservable.set (value 1)",
            "myObservable.get",
            "myDerived.computed(myObservable2: 1)",
            "event fired",
        ]
    );
    // O original não descarta a inscrição; aqui ela é solta para não deixar o observador vivo.
    subscription.dispose();
}

#[test]
fn bug_event_from_observable_always_should_get_events() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let emitter = Emitter::<i32>::new();
    let log = Log::default();
    let i = Rc::new(Cell::new(0));
    let obs = {
        clones!(i);
        observable_from_event(emitter.event(), move |_: Option<&i32>| i.get())
    };

    i.set(i.get() + 1);
    emitter.fire(1);

    let evt2 = Event::from_observable(obs, None);
    let d = {
        clones!(log);
        evt2.listen(move |e| log.log(format!("event fired {e}")))
    };

    i.set(i.get() + 1);
    emitter.fire(2);
    assert_log!(log, ["event fired 2"]);

    i.set(i.get() + 1);
    emitter.fire(3);
    assert_log!(log, ["event fired 3"]);

    d.dispose();
}

#[test]
fn dont_run_autorun_after_dispose() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let log = Log::default();
    let my_observable = LoggingObservableValue::new("myObservable", 0, &log);

    let d = {
        clones!(log, my_observable);
        autorun(move |reader| {
            let v = my_observable.read(reader);
            log.log(format!("autorun, myObservable:{v}"));
        })
    };

    transaction(|tx| {
        my_observable.set(1, Some(tx));
        d.dispose();
    });

    assert_log!(
        log,
        [
            "myObservable.firstObserverAdded",
            "myObservable.get",
            "autorun, myObservable:0",
            "myObservable.set (value 1)",
            "myObservable.lastObserverRemoved",
        ]
    );
}

mod wait_for_state_suite {
    use super::*;

    fn state(s: &'static str) -> Rc<State> {
        Rc::new(State { state: s })
    }

    fn json(s: &State) -> String {
        format!("{{\"state\":\"{}\"}}", s.state)
    }

    fn wait(log: &Log, my_observable: &Rc<LoggingObservableValue<Rc<State>>>) -> je_base::promise::Promise<()> {
        type IsError = Rc<dyn Fn(&Rc<State>) -> Option<Error>>;
        let is_error: IsError = Rc::new(|p| (p.state == "error").then(|| Error::new(json(p))));
        let (l1, l2) = (log.clone(), log.clone());
        wait_for_state(my_observable.to_observable(), |p| p.state == "ready", Some(is_error), None).then_result(move |r| {
            match r {
                Ok(r) => l1.log(format!("resolved {}", json(&r))),
                Err(err) => l2.log(format!("rejected {}", err.message)),
            }
            Ok(())
        })
    }

    #[test]
    fn resolve() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async {
            let log = Log::default();
            let my_observable = LoggingObservableValue::new("myObservable", state("initializing"), &log);

            let p = wait(&log, &my_observable);

            assert_log!(log, ["myObservable.firstObserverAdded", "myObservable.get"]);

            my_observable.set(state("ready"), None);

            assert_log!(log, ["myObservable.set (value [object Object])", "myObservable.get", "myObservable.lastObserverRemoved"]);

            p.await.expect("resolve");

            assert_log!(log, ["resolved {\"state\":\"ready\"}"]);
        });
    }

    #[test]
    fn resolve_immediate() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async {
            let log = Log::default();
            let my_observable = LoggingObservableValue::new("myObservable", state("ready"), &log);

            let p = wait(&log, &my_observable);

            assert_log!(log, ["myObservable.firstObserverAdded", "myObservable.get", "myObservable.lastObserverRemoved"]);

            my_observable.set(state("error"), None);

            assert_log!(log, ["myObservable.set (value [object Object])"]);

            p.await.expect("resolve");

            assert_log!(log, ["resolved {\"state\":\"ready\"}"]);
        });
    }

    #[test]
    fn reject() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async {
            let log = Log::default();
            let my_observable = LoggingObservableValue::new("myObservable", state("initializing"), &log);

            let p = wait(&log, &my_observable);

            assert_log!(log, ["myObservable.firstObserverAdded", "myObservable.get"]);

            my_observable.set(state("error"), None);

            assert_log!(log, ["myObservable.set (value [object Object])", "myObservable.get", "myObservable.lastObserverRemoved"]);

            p.await.expect("resolve");

            assert_log!(log, ["rejected {\"state\":\"error\"}"]);
        });
    }

    #[test]
    fn derived_as_lazy() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let store = DisposableStore::new();
        let log = Log::default();
        let i = Rc::new(Cell::new(0));
        let d = {
            clones!(log, i);
            derived_disposable(move |_| {
                let id = i.get();
                i.set(id + 1);
                log.log(format!("myDerived {id}"));
                let log = log.clone();
                Some(plain_disposable(move || log.log(format!("disposed {id}"))))
            })
        };

        d.get();
        assert_log!(log, ["myDerived 0", "disposed 0"]);
        d.get();
        assert_log!(log, ["myDerived 1", "disposed 1"]);

        d.keep_observed(&store);
        assert_log!(log, []);
        d.get();
        assert_log!(log, ["myDerived 2"]);
        d.get();
        assert_log!(log, []);

        store.dispose();

        assert_log!(log, ["disposed 2"]);
    }
}

#[test]
fn observable_value() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let log = Log::default();
    #[derive(Clone)]
    struct Message {
        _message: &'static str,
    }
    let my_observable1 = new_observable_value::<i32, ()>("myObservable1", 0);
    let my_observable2 = new_observable_value::<i32, Message>("myObservable2", 0);

    let d = {
        clones!(log, my_observable1, my_observable2);
        autorun(move |reader| {
            let v1 = my_observable1.read(reader);
            let v2 = my_observable2.read(reader);
            log.log(format!("autorun, myObservable1:{v1}, myObservable2:{v2}"));
        })
    };

    assert_log!(log, ["autorun, myObservable1:0, myObservable2:0"]);

    // Não dispara o autorun: não houve delta e o valor não mudou.
    my_observable1.set(0, None);

    assert_log!(log, []);

    // Dispara o autorun. O valor não mudou, mas houve um delta.
    my_observable2.set_with_change(0, None, Message { _message: "change1" });

    assert_log!(log, ["autorun, myObservable1:0, myObservable2:0"]);

    d.dispose();
}

mod autorun_error_handling {
    use super::*;

    #[test]
    fn immediate_throw() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let log = Log::default();

        {
            clones!(log);
            errors::set_unexpected_error_handler(move |e| log.log(format!("error: {}", e.message)));
        }

        let my_observable = LoggingObservableValue::new("myObservable", 0, &log);

        let d = {
            clones!(my_observable);
            autorun(move |reader| {
                my_observable.read(reader);
                errors::throw(Error::new("foobar"));
            })
        };

        assert_log!(log, ["myObservable.firstObserverAdded", "myObservable.get", "error: foobar"]);

        my_observable.set(1, None);

        assert_log!(log, ["myObservable.set (value 1)", "myObservable.get", "error: foobar"]);

        d.dispose();
    }

    #[test]
    fn late_throw() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let log = Log::default();

        {
            clones!(log);
            errors::set_unexpected_error_handler(move |e| log.log(format!("error: {}", e.message)));
        }

        let my_observable = LoggingObservableValue::new("myObservable", 0, &log);

        let d = {
            clones!(my_observable);
            autorun(move |reader| {
                let value = my_observable.read(reader);
                if value >= 1 {
                    errors::throw(Error::new("foobar"));
                }
            })
        };

        assert_log!(log, ["myObservable.firstObserverAdded", "myObservable.get"]);

        my_observable.set(1, None);

        assert_log!(log, ["myObservable.set (value 1)", "myObservable.get", "error: foobar"]);

        my_observable.set(2, None);

        assert_log!(log, ["myObservable.set (value 2)", "myObservable.get", "error: foobar"]);

        d.dispose();
    }
}

#[test]
fn recompute_initially_and_on_change_should_work_when_a_dependency_sets_an_observable() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let store = DisposableStore::new();
    let log = Log::default();

    let my_observable = LoggingObservableValue::new("myObservable", 0, &log);

    let should_update = Rc::new(Cell::new(true));

    let my_derived = {
        clones!(log, my_observable, should_update);
        derived(move |reader| {
            log.log("myDerived.computed start");

            let val = my_observable.read(reader);

            if should_update.get() {
                should_update.set(false);
                my_observable.set(1, None);
            }

            log.log("myDerived.computed end");

            val
        })
    };

    assert_log!(log, []);

    {
        clones!(log);
        my_derived.recompute_initially_and_on_change(&store, Some(Rc::new(move |val: &i32| log.log(format!("recomputeInitiallyAndOnChange, myDerived: {val}")))));
    }

    assert_log!(
        log,
        [
            "myDerived.computed start",
            "myObservable.firstObserverAdded",
            "myObservable.get",
            "myObservable.set (value 1)",
            "myDerived.computed end",
            "myDerived.computed start",
            "myObservable.get",
            "myDerived.computed end",
            "recomputeInitiallyAndOnChange, myDerived: 1",
        ]
    );

    my_derived.get();
    assert_log!(log, []);

    store.dispose();
}

mod prevent_invalid_usage {
    use super::*;

    mod reading_outside_of_compute_function {
        use super::*;

        #[test]
        fn derived_case() {
            let _ds = ensure_no_disposables_are_leaked_in_test_suite();

            let obs = ov("obs", 0);
            let reader_holder: Rc<Derived<i32, (), ()>> = {
                clones!(obs);
                Derived::new_untracked(
                    je_base::observable::debug_name::DebugNameData::default(),
                    move |reader| obs.read(reader),
                    None,
                    je_base::observable::deps::strict_equals(),
                )
            };
            let d: IObservable<i32> = reader_holder.clone();

            let disp = autorun(move |reader| {
                d.read(reader);
            });

            // Ler com o leitor do derived fora da função de cálculo.
            assert!(errors::catch(|| obs.read(&*reader_holder)).is_err());

            disp.dispose();
        }

        #[test]
        fn autorun_case() {
            let _ds = ensure_no_disposables_are_leaked_in_test_suite();

            let obs = ov("obs", 0);
            let ar: Rc<AutorunObserver<()>> = {
                clones!(obs);
                AutorunObserver::new_untracked(je_base::observable::debug_name::DebugNameData::default(), move |reader| {
                    obs.read(reader);
                })
            };

            assert!(errors::catch(|| obs.read(&*ar)).is_err());

            ar.dispose();
        }
    }

    #[test]
    #[ignore = "test.skip no original"]
    fn catches_cyclic_dependencies() {}
}

mod observable_reducer_suite {
    use super::*;

    #[test]
    fn main() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let store = DisposableStore::new();
        let log = Log::default();

        let my_observable1 = new_observable_value::<i32, i32>("myObservable1", 5);
        let my_observable2 = new_observable_value::<i32, i32>("myObservable2", 9);

        let (o1, o2): (IObservable<i32>, IObservable<i32>) = (my_observable1.to_observable(), my_observable2.to_observable());
        let sum = {
            let (l_initial, l_dispose, l_update) = (log.clone(), log.clone(), log.clone());
            let (i1, i2) = (o1.clone(), o2.clone());
            observable_reducer(
                DebugOwner::plain(),
                ReducerOptions::<i32, RecordedChanges, i32> {
                    initial: Rc::new(move || {
                        l_initial.log("createInitial");
                        i1.get() + i2.get()
                    }),
                    dispose_final: Some(Rc::new(move |values: &i32| l_dispose.log(format!("disposeFinal {values}")))),
                    change_tracker: record_changes().add::<i32, i32>("myObservable1", &o1).add::<i32, i32>("myObservable2", &o2).build(),
                    equality_comparer: None,
                    update: Rc::new(move |reader: &dyn IDerivedReader<i32>, previous_value: &i32, changes: &RecordedChanges| {
                        l_update.log(format!("update {}", json_summary(changes, &["myObservable1", "myObservable2"])));
                        let mut delta = 0;
                        for change in changes.changes.borrow().iter() {
                            delta += change.change::<i32>().copied().unwrap_or(0);
                        }

                        reader.report_change(delta);
                        let result_value = previous_value + delta;
                        l_update.log(format!("update -> {result_value}"));
                        result_value
                    }),
                },
            )
        };

        assert_log!(log, []);

        {
            clones!(log);
            store.add(autorun_with_store_handle_changes(
                DebugNameOptions::default(),
                record_changes().add::<i32, i32>("sum", &sum).build(),
                move |_reader, changes: &RecordedChanges, _store| {
                    log.log(format!("autorun {}", json_summary(changes, &["sum"])));
                },
            ));
        }

        assert_log!(log, ["createInitial", "update {\"changes\":[],\"myObservable1\":5,\"myObservable2\":9}", "update -> 14", "autorun {\"changes\":[],\"sum\":14}"]);

        transaction(|tx| {
            my_observable1.set_with_change(my_observable1.get() + 1, Some(tx), 1);
            my_observable2.set_with_change(my_observable2.get() + 3, Some(tx), 3);
        });

        assert_log!(
            log,
            [
                "update {\"changes\":[{\"key\":\"myObservable1\",\"change\":1},{\"key\":\"myObservable2\",\"change\":3}],\"myObservable1\":6,\"myObservable2\":12}",
                "update -> 18",
                "autorun {\"changes\":[{\"key\":\"sum\",\"change\":4}],\"sum\":18}",
            ]
        );

        transaction(|tx| {
            my_observable1.set_with_change(my_observable1.get() + 1, Some(tx), 1);
            let s = sum.get();
            log.log(format!("sum.get() {s}"));
            my_observable2.set_with_change(my_observable2.get() + 3, Some(tx), 3);
        });

        assert_log!(
            log,
            [
                "update {\"changes\":[{\"key\":\"myObservable1\",\"change\":1}],\"myObservable1\":7,\"myObservable2\":12}",
                "update -> 19",
                "sum.get() 19",
                "update {\"changes\":[{\"key\":\"myObservable2\",\"change\":3}],\"myObservable1\":7,\"myObservable2\":15}",
                "update -> 22",
                "autorun {\"changes\":[{\"key\":\"sum\",\"change\":1}],\"sum\":22}",
            ]
        );

        store.dispose();

        assert_log!(log, ["disposeFinal 22"]);
        let _ = NoTracker;
    }
}

mod disposable_stores {
    use super::*;

    #[test]
    fn derived_with_store() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let log = Log::default();
        let observable1 = ov("myObservableValue1", 0);

        let computed1 = {
            clones!(log, observable1);
            derived(move |reader| {
                let value = observable1.read(reader);
                log.log(format!("computed {value}"));
                let log = log.clone();
                reader.store().add(to_disposable(move || log.log(format!("computed1: {value} disposed"))));
                value
            })
        };

        let a = {
            clones!(log, computed1);
            autorun(move |reader| log.log(format!("a: {}", computed1.read(reader))))
        };

        assert_log!(log, ["computed 0", "a: 0"]);

        observable1.set(1, None);

        assert_log!(log, ["computed1: 0 disposed", "computed 1", "a: 1"]);

        a.dispose();

        assert_log!(log, ["computed1: 1 disposed"]);
    }

    #[test]
    fn derived_with_delayed_store() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let log = Log::default();
        let observable1 = ov("myObservableValue1", 0);

        let computed1 = {
            clones!(log, observable1);
            derived(move |reader| {
                let value = observable1.read(reader);
                log.log(format!("computed {value}"));
                let log = log.clone();
                reader.delayed_store().add(to_disposable(move || log.log(format!("computed1: {value} disposed"))));
                value
            })
        };

        let a = {
            clones!(log, computed1);
            autorun(move |reader| log.log(format!("a: {}", computed1.read(reader))))
        };

        assert_log!(log, ["computed 0", "a: 0"]);

        observable1.set(1, None);

        assert_log!(log, ["computed 1", "computed1: 0 disposed", "a: 1"]);

        a.dispose();

        assert_log!(log, ["computed1: 1 disposed"]);
    }
}

#[derive(Clone)]
struct Message {
    message: String,
}

#[test]
fn derived_handle_changes_with_report_changes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let log = Log::default();

    let signal1 = observable_signal::<Message>("signal1");
    let signal2 = observable_signal::<Message>("signal2");
    let (s1, s2): (IObservable<()>, IObservable<()>) = (signal1.to_observable(), signal2.to_observable());

    let signal2_derived = derived_handle_changes(
        DerivedHandleChangesOptions {
            debug: DebugNameOptions::default(),
            change_tracker: record_changes().add::<(), Message>("signal2", &s2).build(),
            equality_comparer: None,
        },
        |reader: &dyn IDerivedReader<Message>, change_summary: &RecordedChanges| {
            for c in change_summary.changes.borrow().iter() {
                reader.report_change(Message { message: format!("{} (derived)", c.change::<Message>().expect("delta").message) });
            }
        },
    );

    let d = derived_handle_changes(
        DerivedHandleChangesOptions {
            debug: DebugNameOptions::default(),
            change_tracker: record_changes().add::<(), Message>("signal1", &s1).add::<(), Message>("signal2Derived", &signal2_derived).build(),
            equality_comparer: None,
        },
        |r: &dyn IDerivedReader<String>, changes: &RecordedChanges| {
            let log = changes
                .changes
                .borrow()
                .iter()
                .map(|c| format!("{}: {}", c.key, c.change::<Message>().expect("delta").message))
                .collect::<Vec<_>>()
                .join(", ");
            r.report_change(log);
        },
    );

    let disp = {
        clones!(log);
        run_on_change::<(), String>(d, move |_val, _prev, changes| {
            log.log(format!("runOnChange [{}]", changes.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(",")));
        })
    };

    assert_log!(log, []);

    transaction(|tx| {
        signal1.trigger(Some(tx), Message { message: "foo".into() });
        signal2.trigger(Some(tx), Message { message: "bar".into() });
    });

    assert_log!(log, ["runOnChange [\"signal1: foo, signal2Derived: bar (derived)\"]"]);

    transaction(|tx| {
        signal2.trigger(Some(tx), Message { message: "baz".into() });
    });

    assert_log!(log, ["runOnChange [\"signal2Derived: baz (derived)\"]"]);

    disp.dispose();
}

mod autorun_per_keyed_item_suite {
    use super::*;

    struct Item {
        id: &'static str,
        v: i32,
    }

    fn item(id: &'static str, v: i32) -> Rc<Item> {
        Rc::new(Item { id, v })
    }

    type Items = Rc<Vec<Rc<Item>>>;

    fn items_of(v: Vec<Rc<Item>>) -> Items {
        Rc::new(v)
    }

    #[test]
    fn runs_setup_once_per_key_fires_per_key_observable_on_in_place_value_change_disposes_on_removal() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let log = Log::default();
        let items = new_observable_value::<Items, ()>("items", items_of(vec![]));

        let d = {
            clones!(log);
            ds.add(autorun_per_keyed_item(
                items.to_observable(),
                |it: &Rc<Item>| it.id,
                move |key, value, store| {
                    log.log(format!("setup({key})"));
                    let (log1, key1) = (log.clone(), *key);
                    store.add(to_disposable(move || log1.log(format!("dispose({key1})"))));
                    let (log2, key2) = (log.clone(), *key);
                    store.add(autorun(move |reader| {
                        let v = value.read(reader);
                        log2.log(format!("autorun({key2}): v={}", v.v));
                    }));
                },
            ))
        };
        assert_log!(log, []);

        items.set(items_of(vec![item("a", 1), item("b", 1)]), None);
        assert_log!(log, ["setup(a)", "autorun(a): v=1", "setup(b)", "autorun(b): v=1"]);

        // Mudança no lugar em `a` (mesma chave, objeto novo): o observável da chave dispara. `b` também é
        // um objeto novo, então o dele dispara: comparação por identidade, não por igualdade profunda.
        items.set(items_of(vec![item("a", 2), item("b", 1)]), None);
        assert_log!(log, ["autorun(a): v=2", "autorun(b): v=1"]);

        // Remove `a`: o store dele é descartado; `b` sobrevive (o dele também dispara, porque o novo
        // array traz um objeto novo para `b`).
        items.set(items_of(vec![item("b", 1)]), None);
        assert_log!(log, ["dispose(a)", "autorun(b): v=1"]);

        // Devolve `a`: o setup roda do zero. `b` dispara de novo, pelo mesmo motivo.
        items.set(items_of(vec![item("b", 1), item("a", 9)]), None);
        assert_log!(log, ["autorun(b): v=1", "setup(a)", "autorun(a): v=9"]);

        d.dispose();
        // Descartar o autorun descarta os stores restantes de cada chave.
        let mut entries = log.get_and_clear_entries();
        entries.sort();
        assert_eq!(entries, vec!["dispose(a)".to_string(), "dispose(b)".to_string()]);
    }

    #[test]
    fn batches_per_key_value_updates_atomically_across_one_items_change() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let log = Log::default();
        let items = new_observable_value::<Items, ()>("items", items_of(vec![item("a", 0), item("b", 0)]));

        {
            clones!(log);
            ds.add(autorun_per_keyed_item(
                items.to_observable(),
                |it: &Rc<Item>| it.id,
                move |key, value, store| {
                    let (log, key) = (log.clone(), *key);
                    store.add(autorun(move |reader| log.log(format!("{key}={}", value.read(reader).v))));
                },
            ));
        }
        assert_log!(log, ["a=0", "b=0"]);

        // Uma mudança a montante atualiza as duas chaves; cada autorun roda uma vez com os valores finais.
        items.set(items_of(vec![item("a", 1), item("b", 2)]), None);
        assert_log!(log, ["a=1", "b=2"]);
    }

    #[test]
    fn does_not_fire_per_key_observable_when_same_item_identity_is_reused() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let log = Log::default();
        let a = item("a", 1);
        let items = new_observable_value::<Items, ()>("items", items_of(vec![a.clone()]));

        {
            clones!(log);
            ds.add(autorun_per_keyed_item(
                items.to_observable(),
                |it: &Rc<Item>| it.id,
                move |_key, value, store| {
                    let log = log.clone();
                    store.add(autorun(move |reader| log.log(format!("v={}", value.read(reader).v))));
                },
            ));
        }
        assert_log!(log, ["v=1"]);

        // Mesmo formato de array, mesma identidade de item: sem mudança de valor, sem disparo.
        items.set(items_of(vec![a]), None);
        assert_log!(log, []);
    }

    struct Part {
        id: &'static str,
        content: &'static str,
    }

    struct Turn {
        parts: Rc<Vec<Rc<Part>>>,
    }

    struct SessionState {
        active: Option<Rc<Turn>>,
    }

    #[test]
    fn per_key_setup_fires_when_items_derived_through_observable_from_event_chain_updates() {
        // Espelha o agentHostSessionHandler: observableFromEvent, derived(activeTurn), derived(responseParts),
        // autorunPerKeyedItem. Confere que os disparos incrementais do Event a montante propagam pela
        // cadeia e que o setup por chave vê os itens novos.
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let log = Log::default();

        let on_change = ds.add_rc(Emitter::<Rc<SessionState>>::new());
        let value: Rc<RefCell<Option<Rc<SessionState>>>> = Rc::new(RefCell::new(None));
        let session_state = {
            clones!(value);
            observable_from_event(on_change.event(), move |_: Option<&Rc<SessionState>>| value.borrow().clone())
        };
        let fire = {
            clones!(value, on_change);
            move |s: SessionState| {
                let s = Rc::new(s);
                *value.borrow_mut() = Some(s.clone());
                on_change.fire(s);
            }
        };

        let turn = {
            clones!(session_state);
            derived(move |reader| session_state.read(reader).and_then(|s| s.active.clone()))
        };
        let parts = {
            clones!(turn);
            derived(move |reader| turn.read(reader).map(|t| t.parts.clone()).unwrap_or_else(|| Rc::new(Vec::new())))
        };

        {
            clones!(log);
            ds.add(autorun_per_keyed_item(
                parts,
                |p: &Rc<Part>| p.id,
                move |key, p, store| {
                    log.log(format!("setup({key})"));
                    let (log, key) = (log.clone(), *key);
                    store.add(autorun(move |reader| log.log(format!("{key}={}", p.read(reader).content.len()))));
                },
            ));
        }
        assert_log!(log, []);

        let part = |id, content| Rc::new(Part { id, content });
        // Primeiro estado com uma parte: como um turno que começa com conteúdo.
        fire(SessionState { active: Some(Rc::new(Turn { parts: Rc::new(vec![part("p1", "hello")]) })) });
        assert_log!(log, ["setup(p1)", "p1=5"]);

        // Acrescenta conteúdo a p1.
        fire(SessionState { active: Some(Rc::new(Turn { parts: Rc::new(vec![part("p1", "hello world")]) })) });
        assert_log!(log, ["p1=11"]);

        // Nova parte p2. p1 também dispara, porque o novo array aloca um objeto novo para ela.
        fire(SessionState { active: Some(Rc::new(Turn { parts: Rc::new(vec![part("p1", "hello world"), part("p2", "reasoning")]) })) });
        assert_log!(log, ["p1=11", "setup(p2)", "p2=9"]);
    }
}
