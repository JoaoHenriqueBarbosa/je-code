//! Testes próprios de `src/vs/base/common/sequence.ts` (o VS Code não tem `sequence.test.ts`).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::sequence::{Sequence, Splice, Spliceable};

#[test]
fn splice_updates_elements_and_fires() {
    let sequence: Sequence<i32> = Sequence::new();
    let fired = Rc::new(RefCell::new(Vec::new()));
    let sink = fired.clone();
    let _subscription = sequence.on_did_splice().listen(move |splice| sink.borrow_mut().push(splice.clone()));

    sequence.splice(0, 0, vec![1, 2, 3]);
    sequence.splice(1, 1, vec![9, 8]);
    sequence.splice(10, 5, vec![7]);

    assert_eq!(*sequence.elements.borrow(), vec![1, 9, 8, 3, 7]);
    assert_eq!(
        *fired.borrow(),
        vec![
            Splice { start: 0, delete_count: 0, to_insert: vec![1, 2, 3] },
            Splice { start: 1, delete_count: 1, to_insert: vec![9, 8] },
            Splice { start: 10, delete_count: 5, to_insert: vec![7] },
        ]
    );
}
