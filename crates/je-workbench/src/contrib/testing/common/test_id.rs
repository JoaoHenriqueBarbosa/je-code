//! Porte de `src/vs/workbench/contrib/testing/common/testId.ts`: o identificador de um teste, que é o caminho de
//! IDs locais da raiz (o controlador) até o teste, separados por `\0` na forma de texto.
//!
//! Divergências:
//! - **Estáticos e instância com o mesmo nome.** Em TypeScript `TestId.isRoot(texto)` e o getter `isRoot` convivem;
//!   em Rust funções associadas e métodos dividem o espaço de nomes. As formas estáticas sobre texto levam o sufixo
//!   `_string`: `is_root_string`, `root_string`, `parent_id_string`, `local_id_string` e `compare_strings`.
//! - **`TestItemLike`.** O tipo estrutural `{ id, parent?, _isRoot? }` é o trait [`TestItemLike`].
//! - **Geradores.** `idsFromRoot` e `idsToRoot` devolvem `Vec<TestId>` (o consumo é sempre inteiro).
//! - **`joinToString`** aceita `string | TestId`: o parâmetro é qualquer `Display`.
//! - O ID é texto Rust (`String`), não UTF-16: um ID de teste nunca carrega unidades soltas de substituição.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use je_base::errors::{Error, throw};

/// `TestIdPathParts.Delimiter`: o delimitador das partes do caminho nos IDs de teste.
pub const TEST_ID_PATH_DELIMITER: &str = "\0";

const DELIMITER_CHAR: char = '\0';

/// `TestPosition`: as posições relativas de dois testes, como o `node.compareDocumentPosition` do DOM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestPosition {
    /// a === b
    IsSame,
    /// Neither a nor b are a child of one another. They may share a common parent, though.
    Disconnected,
    /// b is a child of a
    IsChild,
    /// b is a parent of a
    IsParent,
}

/// `TestItemLike`: `{ id: string; parent?: TestItemLike; _isRoot?: boolean }`.
pub trait TestItemLike {
    fn id(&self) -> String;
    fn parent(&self) -> Option<Rc<dyn TestItemLike>>;
    fn is_root(&self) -> bool;
}

/// The test ID is a stringifiable client that
#[derive(Clone, Debug)]
pub struct TestId {
    path: Rc<Vec<String>>,
    view_end: usize,
    stringified: RefCell<Option<String>>,
}

impl TestId {
    /// Creates a test ID from an ext host test item. `parent` ausente (`undefined`) usa o `item.parent`.
    pub fn from_ext_host_test_item(item: &dyn TestItemLike, root_id: &str, parent: Option<Rc<dyn TestItemLike>>) -> TestId {
        if item.is_root() {
            return TestId::new(vec![root_id.to_string()]);
        }

        let mut path = vec![item.id()];
        let mut current = parent.or_else(|| item.parent());
        while let Some(i) = current {
            if i.id() == root_id {
                break;
            }
            path.push(i.id());
            current = i.parent();
        }
        path.push(root_id.to_string());

        path.reverse();
        TestId::new(path)
    }

    /// Cheaply ets whether the ID refers to the root .
    pub fn is_root_string(id_string: &str) -> bool {
        !id_string.contains(DELIMITER_CHAR)
    }

    /// Cheaply gets whether the ID refers to the root .
    pub fn root_string(id_string: &str) -> &str {
        match id_string.find(DELIMITER_CHAR) {
            Some(idx) => &id_string[..idx],
            None => id_string,
        }
    }

    /// Creates a test ID from a serialized TestId instance.
    pub fn from_string(id_string: &str) -> TestId {
        TestId::new(id_string.split(DELIMITER_CHAR).map(str::to_string).collect())
    }

    /// Gets the ID resulting from adding b to the base ID.
    pub fn join(base: &TestId, b: &str) -> TestId {
        let mut path = base.path.as_ref().clone();
        path.push(b.to_string());
        TestId::new(path)
    }

    /// Splits a test ID into its parts.
    pub fn split(id_string: &str) -> Vec<String> {
        id_string.split(DELIMITER_CHAR).map(str::to_string).collect()
    }

    /// Gets the string ID resulting from adding b to the base ID.
    pub fn join_to_string(base: impl fmt::Display, b: &str) -> String {
        format!("{base}{TEST_ID_PATH_DELIMITER}{b}")
    }

    /// Cheaply gets the parent ID of a test identified with the string.
    pub fn parent_id_string(id_string: &str) -> Option<&str> {
        id_string.rfind(DELIMITER_CHAR).map(|idx| &id_string[..idx])
    }

    /// Cheaply gets the local ID of a test identified with the string.
    pub fn local_id_string(id_string: &str) -> &str {
        match id_string.rfind(DELIMITER_CHAR) {
            Some(idx) => &id_string[idx + TEST_ID_PATH_DELIMITER.len()..],
            None => id_string,
        }
    }

    /// Gets whether maybeChild is a child of maybeParent.
    /// todo@connor4312: review usages of this to see if using the WellDefinedPrefixTree is better
    pub fn is_child(maybe_parent: &str, maybe_child: &str) -> bool {
        maybe_child.as_bytes().get(maybe_parent.len()) == Some(&0) && maybe_child.starts_with(maybe_parent)
    }

    /// Compares the position of the two ID strings.
    /// todo@connor4312: review usages of this to see if using the WellDefinedPrefixTree is better
    pub fn compare_strings(a: &str, b: &str) -> TestPosition {
        if a == b {
            return TestPosition::IsSame;
        }

        if TestId::is_child(a, b) {
            return TestPosition::IsChild;
        }

        if TestId::is_child(b, a) {
            return TestPosition::IsParent;
        }

        TestPosition::Disconnected
    }

    pub fn get_length_of_common_prefix(length: usize, get_id: impl Fn(usize) -> TestId) -> usize {
        if length == 0 {
            return 0;
        }

        let mut common_prefix = 0;
        while common_prefix < length - 1 {
            for i in 1..length {
                let a = get_id(i - 1);
                let b = get_id(i);
                if a.path.get(common_prefix) != b.path.get(common_prefix) {
                    return common_prefix;
                }
            }

            common_prefix += 1;
        }

        common_prefix
    }

    /// `new TestId(path)`: a visão vai até o fim do caminho.
    pub fn new(path: Vec<String>) -> TestId {
        let view_end = path.len();
        TestId::with_view_end(Rc::new(path), view_end)
    }

    /// `new TestId(path, viewEnd)`: uma visão dos `view_end` primeiros segmentos de um caminho compartilhado.
    pub fn with_view_end(path: Rc<Vec<String>>, view_end: usize) -> TestId {
        if path.is_empty() || view_end < 1 {
            throw(Error::new("cannot create test with empty path"));
        }

        TestId { path, view_end, stringified: RefCell::new(None) }
    }

    /// O caminho completo (não só a visão), como o `path` do original.
    pub fn path(&self) -> &[String] {
        &self.path
    }

    /// Gets the ID of the parent test.
    pub fn root_id(&self) -> TestId {
        TestId::with_view_end(self.path.clone(), 1)
    }

    /// Gets the ID of the parent test.
    pub fn parent_id(&self) -> Option<TestId> {
        if self.view_end > 1 { Some(TestId::with_view_end(self.path.clone(), self.view_end - 1)) } else { None }
    }

    /// Gets the local ID of the current full test ID.
    pub fn local_id(&self) -> &str {
        &self.path[self.view_end - 1]
    }

    /// Gets whether this ID refers to the root.
    pub fn controller_id(&self) -> &str {
        &self.path[0]
    }

    /// Gets whether this ID refers to the root.
    pub fn is_root(&self) -> bool {
        self.view_end == 1
    }

    /// Returns an iterable that yields IDs of all parent items down to and
    /// including the current item.
    pub fn ids_from_root(&self) -> Vec<TestId> {
        (1..=self.view_end).map(|i| TestId::with_view_end(self.path.clone(), i)).collect()
    }

    /// Returns an iterable that yields IDs of the current item up to the root
    /// item.
    pub fn ids_to_root(&self) -> Vec<TestId> {
        (1..=self.view_end).rev().map(|i| TestId::with_view_end(self.path.clone(), i)).collect()
    }

    /// Compares the other test ID with this one.
    pub fn compare(&self, other: &TestId) -> TestPosition {
        for i in 0..other.view_end.min(self.view_end) {
            if other.path[i] != self.path[i] {
                return TestPosition::Disconnected;
            }
        }

        if other.view_end > self.view_end {
            return TestPosition::IsChild;
        }

        if other.view_end < self.view_end {
            return TestPosition::IsParent;
        }

        TestPosition::IsSame
    }

    /// `compare(other: string)`.
    pub fn compare_to_string(&self, other: &str) -> TestPosition {
        TestId::compare_strings(&self.to_string(), other)
    }

    /// Serializes the ID.
    pub fn to_json(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for TestId {
    /// Serializes the ID to a string.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut cached = self.stringified.borrow_mut();
        let text = cached.get_or_insert_with(|| {
            let mut stringified = self.path[0].clone();
            for part in &self.path[1..self.view_end] {
                stringified.push(DELIMITER_CHAR);
                stringified.push_str(part);
            }
            stringified
        });
        f.write_str(text)
    }
}
