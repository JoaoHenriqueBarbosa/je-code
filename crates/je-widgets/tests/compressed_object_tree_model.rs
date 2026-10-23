//! Porte de `src/vs/base/test/browser/ui/tree/compressedObjectTreeModel.test.ts` (suítes `compress & decompress` e `CompressedObjectTreeModel`, ambas dentro de `CompressedObjectTree`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `number` é `i32` (o elemento precisa ser `Eq + Hash`); `ICompressedTreeElement` e o nó comprimido
//!   `{ elements, incompressible }` (um [`CompressedRef`], de identidade por referência) são os tipos do módulo.
//! - `resolve` é genérico no tipo do elemento (recebe como ler o valor), e vira o [`Resolved`]: o
//!   `deepStrictEqual` do original compara o `resolve(compress(x))` com o literal já resolvido, e aqui o
//!   literal é construído como elemento e passa pelo mesmo `resolve`.
//! - `bindListToModel` é genérico no modelo (o nó vem do tipo associado).
//! - `withSmartSplice` roda o corpo duas vezes: com opções vazias e com o `diffIdentityProvider`.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::{DisposableRef, IDisposable};
use je_widgets::list::list_types::IIdentityProvider;
use je_widgets::tree::compressed_object_tree_model::{CompressedObjectTreeModel, CompressedRef, ICompressedTreeElement, ICompressedTreeNode, compress, decompress};
use je_widgets::tree::index_tree_model::TreeNode;
use je_widgets::tree::object_tree_model::ObjectTreeModelSetChildrenOptions;
use je_widgets::tree::tree_types::{ByRef, ITreeModel};
use support::ensure_no_disposables_are_leaked_in_test_suite;

type Elem = ICompressedTreeElement<i32>;
type CompressedElem = ICompressedTreeElement<CompressedRef<i32>>;

/// `IResolvedCompressedTreeElement<T>`: só o que o `deepStrictEqual` enxerga (`incompressible` só quando
/// verdadeiro, `children` só quando não vazio, e aqui a ausência é a lista vazia).
#[derive(Debug, PartialEq)]
struct Resolved<V> {
    element: V,
    children: Vec<Resolved<V>>,
    incompressible: bool,
}

/// `resolve`.
fn resolve<E, V>(tree_element: &ICompressedTreeElement<E>, read: &dyn Fn(&E) -> V) -> Resolved<V> {
    Resolved {
        element: read(&tree_element.element),
        children: tree_element.children.iter().flatten().map(|child| resolve(child, read)).collect(),
        incompressible: tree_element.incompressible,
    }
}

fn resolve_decompressed(tree_element: &Elem) -> Resolved<i32> {
    resolve(tree_element, &|e| *e)
}

fn resolve_compressed(tree_element: &CompressedElem) -> Resolved<(Vec<i32>, bool)> {
    resolve(tree_element, &|node: &CompressedRef<i32>| (node.elements.clone(), node.incompressible))
}

/// `{ element: { elements, incompressible }, children }`.
fn node(elements: &[i32], incompressible: bool, children: Vec<CompressedElem>) -> CompressedElem {
    let element = ByRef::new(ICompressedTreeNode { elements: elements.to_vec(), incompressible });
    if children.is_empty() { ICompressedTreeElement::new(element) } else { ICompressedTreeElement::with_children(element, children) }
}

fn el(element: i32, children: Vec<Elem>) -> Elem {
    if children.is_empty() { ICompressedTreeElement::new(element) } else { ICompressedTreeElement::with_children(element, children) }
}

/// `assert.deepStrictEqual(resolve(compress(decompressed)), compressed)` e
/// `assert.deepStrictEqual(resolve(decompress(compressed)), decompressed)`.
fn assert_compresses_and_decompresses(decompressed: &Elem, compressed: &CompressedElem) {
    assert_eq!(resolve_compressed(&compress(decompressed)), resolve_compressed(compressed));
    assert_eq!(resolve_decompressed(&decompress(compressed)), resolve_decompressed(decompressed));
}

mod compress_and_decompress {
    use super::*;

    #[test]
    fn small() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let decompressed = el(1, vec![]);
        let compressed = node(&[1], false, vec![]);

        assert_compresses_and_decompresses(&decompressed, &compressed);
    }

    #[test]
    fn no_compression() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let decompressed = el(1, vec![el(11, vec![]), el(12, vec![]), el(13, vec![])]);

        let compressed = node(&[1], false, vec![node(&[11], false, vec![]), node(&[12], false, vec![]), node(&[13], false, vec![])]);

        assert_compresses_and_decompresses(&decompressed, &compressed);
    }

    #[test]
    fn single_hierarchy() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let decompressed = el(1, vec![el(11, vec![el(111, vec![el(1111, vec![])])])]);

        let compressed = node(&[1, 11, 111, 1111], false, vec![]);

        assert_compresses_and_decompresses(&decompressed, &compressed);
    }

    #[test]
    fn deep_compression() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let decompressed = el(1, vec![el(11, vec![el(111, vec![el(1111, vec![]), el(1112, vec![]), el(1113, vec![]), el(1114, vec![])])])]);

        let compressed = node(&[1, 11, 111], false, vec![node(&[1111], false, vec![]), node(&[1112], false, vec![]), node(&[1113], false, vec![]), node(&[1114], false, vec![])]);

        assert_compresses_and_decompresses(&decompressed, &compressed);
    }

    #[test]
    fn double_deep_compression() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let decompressed = el(1, vec![el(11, vec![el(111, vec![el(1112, vec![]), el(1113, vec![])])]), el(12, vec![el(121, vec![el(1212, vec![]), el(1213, vec![])])])]);

        let compressed = node(
            &[1],
            false,
            vec![
                node(&[11, 111], false, vec![node(&[1112], false, vec![]), node(&[1113], false, vec![])]),
                node(&[12, 121], false, vec![node(&[1212], false, vec![]), node(&[1213], false, vec![])]),
            ],
        );

        assert_compresses_and_decompresses(&decompressed, &compressed);
    }

    #[test]
    fn incompressible_leaf() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let decompressed = el(1, vec![el(11, vec![el(111, vec![el(1111, vec![]).incompressible()])])]);

        let compressed = node(&[1, 11, 111], false, vec![node(&[1111], true, vec![])]);

        assert_compresses_and_decompresses(&decompressed, &compressed);
    }

    #[test]
    fn incompressible_branch() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let decompressed = el(1, vec![el(11, vec![el(111, vec![el(1111, vec![])]).incompressible()])]);

        let compressed = node(&[1, 11], false, vec![node(&[111, 1111], true, vec![])]);

        assert_compresses_and_decompresses(&decompressed, &compressed);
    }

    #[test]
    fn incompressible_chain() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let decompressed = el(1, vec![el(11, vec![el(111, vec![el(1111, vec![]).incompressible()]).incompressible()])]);

        let compressed = node(&[1, 11], false, vec![node(&[111], true, vec![node(&[1111], true, vec![])])]);

        assert_compresses_and_decompresses(&decompressed, &compressed);
    }

    #[test]
    fn incompressible_tree() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let decompressed = el(1, vec![el(11, vec![el(111, vec![el(1111, vec![]).incompressible()]).incompressible()]).incompressible()]);

        let compressed = node(&[1], false, vec![node(&[11], true, vec![node(&[111], true, vec![node(&[1111], true, vec![])])])]);

        assert_compresses_and_decompresses(&decompressed, &compressed);
    }
}

type List = Rc<RefCell<Vec<Rc<TreeNode<Option<CompressedRef<i32>>>>>>>;

/// `bindListToModel`.
fn bind_list_to_model<M: ITreeModel>(list: &Rc<RefCell<Vec<Rc<M::Node>>>>, model: &M) -> DisposableRef {
    let list = list.clone();
    model.on_did_splice_rendered_nodes().listen(move |e| {
        let mut list = list.borrow_mut();
        let end = (e.start + e.delete_count).min(list.len());
        list.splice(e.start..end, e.elements.iter().cloned());
    })
}

/// `toArray`.
fn to_array(list: &List) -> Vec<Vec<i32>> {
    list.borrow().iter().map(|i| i.element.as_ref().expect("só a raiz tem elemento nulo").elements.clone()).collect()
}

/// `{ getId: n => String(n) }`.
struct NumberIdentity;

impl IIdentityProvider<i32> for NumberIdentity {
    fn get_id(&self, n: &i32) -> String {
        n.to_string()
    }
}

/// `withSmartSplice`: roda o corpo com opções vazias e depois com o `diffIdentityProvider`.
fn with_smart_splice(f: impl Fn(&ObjectTreeModelSetChildrenOptions<i32>)) {
    f(&Default::default());
    f(&ObjectTreeModelSetChildrenOptions { diff_identity_provider: Some(Rc::new(NumberIdentity)), ..Default::default() });
}

mod compressed_object_tree_model {
    use super::*;

    #[test]
    fn ctor() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = CompressedObjectTreeModel::<i32>::new("test", Default::default());
        assert_eq!(model.size(), 0);
    }

    #[test]
    fn flat() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        with_smart_splice(|options| {
            let list: List = Rc::default();
            let model = CompressedObjectTreeModel::<i32>::new("test", Default::default());
            let disposable = bind_list_to_model(&list, &model);

            model.set_children(&None, vec![el(0, vec![]), el(1, vec![]), el(2, vec![])], options.clone());

            assert_eq!(to_array(&list), [[0], [1], [2]]);
            assert_eq!(model.size(), 3);

            model.set_children(&None, vec![el(3, vec![]), el(4, vec![]), el(5, vec![])], options.clone());

            assert_eq!(to_array(&list), [[3], [4], [5]]);
            assert_eq!(model.size(), 3);

            model.set_children(&None, vec![], options.clone());
            assert_eq!(to_array(&list), Vec::<Vec<i32>>::new());
            assert_eq!(model.size(), 0);

            disposable.dispose();
        });
    }

    #[test]
    fn nested() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        with_smart_splice(|options| {
            let list: List = Rc::default();
            let model = CompressedObjectTreeModel::<i32>::new("test", Default::default());
            let disposable = bind_list_to_model(&list, &model);

            model.set_children(&None, vec![el(0, vec![el(10, vec![]), el(11, vec![]), el(12, vec![])]), el(1, vec![]), el(2, vec![])], options.clone());

            assert_eq!(to_array(&list), [vec![0], vec![10], vec![11], vec![12], vec![1], vec![2]]);
            assert_eq!(model.size(), 6);

            model.set_children(&Some(12), vec![el(120, vec![]), el(121, vec![])], options.clone());

            assert_eq!(to_array(&list), [vec![0], vec![10], vec![11], vec![12], vec![120], vec![121], vec![1], vec![2]]);
            assert_eq!(model.size(), 8);

            model.set_children(&Some(0), vec![], options.clone());
            assert_eq!(to_array(&list), [[0], [1], [2]]);
            assert_eq!(model.size(), 3);

            model.set_children(&None, vec![], options.clone());
            assert_eq!(to_array(&list), Vec::<Vec<i32>>::new());
            assert_eq!(model.size(), 0);

            disposable.dispose();
        });
    }

    #[test]
    fn compressed() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        with_smart_splice(|options| {
            let list: List = Rc::default();
            let model = CompressedObjectTreeModel::<i32>::new("test", Default::default());
            let disposable = bind_list_to_model(&list, &model);

            model.set_children(&None, vec![el(1, vec![el(11, vec![el(111, vec![el(1111, vec![]), el(1112, vec![]), el(1113, vec![])])])])], options.clone());

            assert_eq!(to_array(&list), [vec![1, 11, 111], vec![1111], vec![1112], vec![1113]]);
            assert_eq!(model.size(), 6);

            model.set_children(&Some(11), vec![el(111, vec![]), el(112, vec![]), el(113, vec![])], options.clone());

            assert_eq!(to_array(&list), [vec![1, 11], vec![111], vec![112], vec![113]]);
            assert_eq!(model.size(), 5);

            model.set_children(&Some(113), vec![el(1131, vec![])], options.clone());

            assert_eq!(to_array(&list), [vec![1, 11], vec![111], vec![112], vec![113, 1131]]);
            assert_eq!(model.size(), 6);

            model.set_children(&Some(1131), vec![el(1132, vec![])], options.clone());

            assert_eq!(to_array(&list), [vec![1, 11], vec![111], vec![112], vec![113, 1131, 1132]]);
            assert_eq!(model.size(), 7);

            model.set_children(&Some(1131), vec![el(1132, vec![]), el(1133, vec![])], options.clone());

            assert_eq!(to_array(&list), [vec![1, 11], vec![111], vec![112], vec![113, 1131], vec![1132], vec![1133]]);
            assert_eq!(model.size(), 8);

            disposable.dispose();
        });
    }
}
