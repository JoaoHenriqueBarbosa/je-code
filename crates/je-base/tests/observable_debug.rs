//! Porte de `src/vs/base/test/common/observables/debug.test.ts` (suíte `debug`), caso a caso e na mesma ordem.

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::observable::base::{DebugNode, Observable};
use je_base::observable::logging::debug_get_dependency_graph::{GraphOptions, GraphType, debug_get_observable_graph};
use je_base::observable::{autorun, derived, observable_value};

#[test]
fn debug_get_dependency_graph() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();

    let my_observable1 = observable_value::<i32, ()>("myObservable1", 0);
    let my_observable2 = observable_value::<i32, ()>("myObservable2", 0);

    let my_computed1 = {
        let (o1, o2) = (my_observable1.clone(), my_observable2.clone());
        derived(move |reader| {
            let value1 = o1.read(reader);
            let value2 = o2.read(reader);
            value1 + value2
        })
    };

    let my_computed2 = {
        let (c1, o1, o2) = (my_computed1.clone(), my_observable1.clone(), my_observable2.clone());
        derived(move |reader| {
            let value1 = c1.read(reader);
            let value2 = o1.read(reader);
            let value3 = o2.read(reader);
            value1 + value2 + value3
        })
    };

    let my_computed3 = {
        let (c2, o1, o2) = (my_computed2.clone(), my_observable1.clone(), my_observable2.clone());
        derived(move |reader| {
            let value1 = c2.read(reader);
            let value2 = o1.read(reader);
            let value3 = o2.read(reader);
            value1 + value2 + value3
        })
    };

    {
        let c3 = my_computed3.clone();
        ds.add(autorun(move |reader| {
            c3.read(reader);
        }));
    }

    let idx = Rc::new(Cell::new(0));
    let counter = idx.clone();
    assert_eq!(
        debug_get_observable_graph(
            &DebugNode::Observable(my_computed3.to_any_observable()),
            &GraphOptions {
                kind: GraphType::Dependencies,
                debug_name_post_processor: Some(Box::new(move |_name| {
                    counter.set(counter.get() + 1);
                    format!("name{}", counter.get())
                })),
            }
        ),
        "* derived name1:\n  value: 0\n  state: upToDate\n  dependencies:\n\t\t* derived name2:\n\t\t  value: 0\n\t\t  state: upToDate\n\t\t  dependencies:\n\t\t\t\t* derived name3:\n\t\t\t\t  value: 0\n\t\t\t\t  state: upToDate\n\t\t\t\t  dependencies:\n\t\t\t\t\t\t* observableValue name4:\n\t\t\t\t\t\t  value: 0\n\t\t\t\t\t\t  state: upToDate\n\t\t\t\t\t\t* observableValue name5:\n\t\t\t\t\t\t  value: 0\n\t\t\t\t\t\t  state: upToDate\n\t\t\t\t* observableValue name6 (already listed)\n\t\t\t\t* observableValue name7 (already listed)\n\t\t* observableValue name8 (already listed)\n\t\t* observableValue name9 (already listed)",
    );
}
