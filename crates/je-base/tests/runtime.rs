//! A ordem do event loop contra o Node real (`node -e`, Node 24, saída registrada no teste).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::promise::Promise;
use je_base::runtime;

#[test]
fn ordem_igual_ao_node() {
    // setTimeout(()=>log('t1'),0); setTimeout(()=>{log('t2'); Promise.resolve().then(()=>log('t2p'))},0);
    // Promise.resolve().then(()=>log('p1')).then(()=>log('p2')); queueMicrotask(()=>log('m1'));
    // (async()=>{log('a0'); await null; log('a1'); await new Promise(r=>setTimeout(r,5)); log('a2')})();
    // log('sync')
    // Node: a0 sync p1 m1 a1 p2 t1 t2 t2p a2
    let log = Rc::new(RefCell::new(Vec::<&str>::new()));
    let push = |l: &Rc<RefCell<Vec<&'static str>>>, s: &'static str| l.borrow_mut().push(s);
    runtime::with_faked_timers(|| {
        let l = log.clone();
        runtime::set_timeout(move || push(&l, "t1"), 0);
        let l = log.clone();
        runtime::set_timeout(
            move || {
                push(&l, "t2");
                let l2 = l.clone();
                Promise::resolved(()).then(move |_| push(&l2, "t2p"));
            },
            0,
        );
        let (l1, l2) = (log.clone(), log.clone());
        Promise::resolved(()).then(move |_| push(&l1, "p1")).then(move |_| push(&l2, "p2"));
        let l = log.clone();
        runtime::queue_microtask(move || push(&l, "m1"));
        let l = log.clone();
        runtime::spawn_local(async move {
            push(&l, "a0");
            let _ = Promise::resolved(()).await;
            push(&l, "a1");
            let (p, r) = Promise::<()>::new();
            runtime::set_timeout(move || r.resolve(()), 5);
            let _ = p.await;
            push(&l, "a2");
        });
        push(&log, "sync");
    });
    assert_eq!(log.borrow().join(" "), "a0 sync p1 m1 a1 p2 t1 t2 t2p a2");
}

#[test]
fn all_settled_igual_ao_node() {
    // (async()=>{ await Promise.allSettled([Promise.resolve(1),Promise.reject(2)]).then(()=>log('then')); log('after') })();
    // Promise.resolve().then(()=>log('m1')).then(()=>log('m2')) ... .then(()=>log('m6'));
    // (async()=>{ await Promise.allSettled([]).then(()=>log('e-then')); log('e-after') })();
    // Node: m1 e-then then m2 e-after after m3 m4 m5 m6
    let log = Rc::new(RefCell::new(Vec::<&str>::new()));
    let push = |l: &Rc<RefCell<Vec<&'static str>>>, s: &'static str| l.borrow_mut().push(s);
    runtime::with_faked_timers(|| {
        let l = log.clone();
        runtime::spawn_local(async move {
            let l2 = l.clone();
            let settled = Promise::all_settled(vec![Promise::resolved(1), Promise::rejected(je_base::errors::Error::new("2"))]).then(move |_| push(&l2, "then"));
            let _ = settled.await;
            push(&l, "after");
        });
        let mut chain = Promise::resolved(());
        for name in ["m1", "m2", "m3", "m4", "m5", "m6"] {
            let l = log.clone();
            chain = chain.then(move |_| push(&l, name));
        }
        let l = log.clone();
        runtime::spawn_local(async move {
            let l2 = l.clone();
            let settled = Promise::<i32>::all_settled(vec![]).then(move |_| push(&l2, "e-then"));
            let _ = settled.await;
            push(&l, "e-after");
        });
    });
    assert_eq!(log.borrow().join(" "), "m1 e-then then m2 e-after after m3 m4 m5 m6");
}

#[test]
fn adocao_finally_all_race_igual_ao_node() {
    // const log=[]; chain: Promise.resolve().then(()=>log.push('t1')).then(...) até t8
    // new Promise(r=>r(Promise.resolve(1))).then(()=>log.push('A'));
    // Promise.resolve().then(()=>Promise.resolve(2)).then(()=>log.push('B'));
    // Promise.resolve(3).finally(()=>{}).then(()=>log.push('C'));
    // Promise.reject(4).finally(()=>{}).catch(()=>log.push('D'));
    // Promise.all([Promise.resolve(1),Promise.resolve(2)]).then(()=>log.push('E'));
    // Promise.all([]).then(()=>log.push('F'));
    // Promise.race([Promise.resolve(1),Promise.resolve(2)]).then(()=>log.push('G'));
    // (a cadeia t1..t8 é criada por último)
    // Node: F t1 E G t2 A t3 B C D t4 t5 t6 t7 t8
    use je_base::errors::Error;
    let log = Rc::new(RefCell::new(Vec::<String>::new()));
    let push = |l: &Rc<RefCell<Vec<String>>>, s: &str| l.borrow_mut().push(s.to_string());
    runtime::with_faked_timers(|| {
        let l = log.clone();
        let a = Promise::<i32>::with_executor(|resolver| resolver.resolve_with(&Promise::resolved(1)));
        a.then(move |_| push(&l, "A"));

        let l = log.clone();
        Promise::resolved(()).then_promise(|_| Promise::resolved(2)).then(move |_| push(&l, "B"));

        let l = log.clone();
        Promise::resolved(3).finally(|| {}).then(move |_| push(&l, "C"));

        let l = log.clone();
        Promise::<i32>::rejected(Error::new("4")).finally(|| {}).catch(move |_| {
            push(&l, "D");
            0
        });

        let l = log.clone();
        Promise::all(vec![Promise::resolved(1), Promise::resolved(2)]).then(move |_| push(&l, "E"));
        let l = log.clone();
        Promise::<i32>::all(Vec::new()).then(move |_| push(&l, "F"));
        let l = log.clone();
        Promise::race(vec![Promise::resolved(1), Promise::resolved(2)]).then(move |_| push(&l, "G"));

        let mut chain = Promise::resolved(());
        for i in 1..=8 {
            let l = log.clone();
            chain = chain.then(move |_| push(&l, &format!("t{i}")));
        }
    });
    assert_eq!(log.borrow().join(" "), "F t1 E G t2 A t3 B C D t4 t5 t6 t7 t8");
}
