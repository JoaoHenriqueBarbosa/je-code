//! Porte de `src/vs/base/test/common/linkedList.test.ts` (suíte `LinkedList`), caso a caso e na mesma ordem.

use je_base::linked_list::LinkedList;

fn assert_elements<E: Clone + PartialEq + std::fmt::Debug + 'static>(list: &LinkedList<E>, elements: &[E]) {
    // check size
    assert_eq!(list.size(), elements.len());

    // assert toArray
    assert_eq!(list.iter().collect::<Vec<_>>(), elements);

    // assert Symbol.iterator (1)
    assert_eq!(list.iter().collect::<Vec<_>>(), elements);

    // assert Symbol.iterator (2)
    let mut expected = elements.to_vec();
    expected.reverse();
    for item in list.iter() {
        assert_eq!(Some(item), expected.pop());
    }
    assert_eq!(expected.len(), 0);

    // por referência, sem clonar
    let mut seen = Vec::new();
    list.for_each_ref(|e| seen.push(e.clone()));
    assert_eq!(seen, elements);
}

#[test]
fn push_iter() {
    let list = LinkedList::<i32>::new();
    list.push(0);
    list.push(1);
    list.push(2);

    assert_elements(&list, &[0, 1, 2]);
}

#[test]
fn push_remove() {
    let list = LinkedList::<i32>::new();
    let disp = list.push(0);
    list.push(1);
    list.push(2);
    disp.remove();
    assert_elements(&list, &[1, 2]);

    let list = LinkedList::<i32>::new();
    list.push(0);
    let disp = list.push(1);
    list.push(2);
    disp.remove();
    assert_elements(&list, &[0, 2]);

    let list = LinkedList::<i32>::new();
    list.push(0);
    list.push(1);
    let disp = list.push(2);
    disp.remove();
    assert_elements(&list, &[0, 1]);

    let list = LinkedList::<i32>::new();
    list.push(0);
    list.push(1);
    let disp = list.push(2);
    disp.remove();
    disp.remove();
    assert_elements(&list, &[0, 1]);
}

#[test]
fn push_to_array() {
    let list = LinkedList::<&str>::new();
    list.push("foo");
    list.push("bar");
    list.push("far");
    list.push("boo");

    assert_elements(&list, &["foo", "bar", "far", "boo"]);
}

#[test]
fn unshift_iter() {
    let list = LinkedList::<i32>::new();
    list.unshift(0);
    list.unshift(1);
    list.unshift(2);

    assert_elements(&list, &[2, 1, 0]);
}

#[test]
fn unshift_remove() {
    let list = LinkedList::<i32>::new();
    let disp = list.unshift(0);
    list.unshift(1);
    list.unshift(2);
    disp.remove();
    assert_elements(&list, &[2, 1]);

    let list = LinkedList::<i32>::new();
    list.unshift(0);
    let disp = list.unshift(1);
    list.unshift(2);
    disp.remove();
    assert_elements(&list, &[2, 0]);

    let list = LinkedList::<i32>::new();
    list.unshift(0);
    list.unshift(1);
    let disp = list.unshift(2);
    disp.remove();
    assert_elements(&list, &[1, 0]);
}

#[test]
fn unshift_to_array() {
    let list = LinkedList::<&str>::new();
    list.unshift("foo");
    list.unshift("bar");
    list.unshift("far");
    list.unshift("boo");

    assert_elements(&list, &["boo", "far", "bar", "foo"]);
}

#[test]
fn pop_unshift() {
    let list = LinkedList::<&str>::new();
    list.push("a");
    list.push("b");

    assert_elements(&list, &["a", "b"]);

    let a = list.shift();
    assert_eq!(a, Some("a"));

    assert_elements(&list, &["b"]);

    list.unshift("a");
    assert_elements(&list, &["a", "b"]);

    let b = list.pop();
    assert_eq!(b, Some("b"));

    assert_elements(&list, &["a"]);
}
