//! Porte de `src/vs/workbench/api/common/extHostTestItem.ts` e de `extHostTestingPrivateApi.ts`: o `TestItem` que a
//! extensão manipula ([`TestItemImpl`]), a coleção de itens de um controlador ([`ExtHostTestItemCollection`]) e a
//! conversão do contexto de um item da UI de volta num item da extensão ([`to_item_from_context`]).
//!
//! Divergências:
//! - **`TestItemImpl` é um handle** (`Rc`) barato de clonar, a identidade do objeto do original é [`Rc::ptr_eq`]
//!   ([`ITestItemLike::is_same`]). As propriedades que o original define com `Object.defineProperties` (`range`, `label`,
//!   `description`, `sortText`, `canResolveChildren`, `busy`, `error`, `tags`) são células com getter e `set_*`: o setter
//!   compara com o comparador da propriedade e, se mudou, atribui e avisa o `listener` da API privada, nessa ordem.
//! - **API privada.** O `WeakMap` de `createPrivateApiFor` e `getPrivateApiFor` é o campo `api` do item (um
//!   [`ITestItemApi`]), que a extensão não alcança: o acesso é [`TestItemImpl::private_api`]. `getPrivateApiFor` lança
//!   `InvalidTestItemError` para um objeto sem API, o que o tipo já garante.
//! - **`error`** (`string | MarkdownString`) é [`TestItemError`]: o texto compara por valor e o markdown por identidade
//!   (`strictEqualComparator`), como no original.
//! - **`TestItemRootImpl`** é um [`TestItemImpl`] com `is_root` verdadeiro ([`TestItemImpl::new_root`]). O `instanceof
//!   TestItemRootImpl` do getter `parent` é esse campo.
//! - **O construtor** de [`TestItemImpl`] devolve [`Result`]: lança se o ID tem o delimitador `\0`.
//! - **`ExtHostTestItemCollection`** (`extends TestItemCollection`) é uma struct com `Deref` para a coleção, como o
//!   `TestTestCollection` dos testes. O `ExtHostDocumentsAndEditors` que o construtor recebe só serve para ler a versão
//!   de um documento, então o parâmetro é o trait [`IDocumentVersionSource`].
//! - **`toItemFromContext`** lança no original se o contexto é vazio (o `node!` é `undefined`); aqui devolve [`Option`].

use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use je_base::errors::{Error, throw};
use je_base::lifecycle::IDisposable;
use je_base::uri::URI;
use je_platform::resources::host_platform;

use super::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use super::ext_host_testing_converters as convert;
use super::ext_host_testing_types::TestTag;
use super::ext_host_type_converters::Range as RangeConverter;
use super::ext_host_types::{Documentation, MarkdownString, Range};
use crate::contrib::testing::common::test_id::{TEST_ID_PATH_DELIMITER, TestId, TestItemLike};
use crate::contrib::testing::common::test_item_collection::{
    ExtHostTestItemEvent, ITestChildrenLike, ITestItemApi, ITestItemChildren, ITestItemCollectionOptions, ITestItemLike, TestItemCollection,
    create_test_item_children,
};
use crate::contrib::testing::common::test_types::{ITestItemContext, ITestTag, PartialTestItem, denamespace_test_tag};

/// `string | vscode.MarkdownString`: o `error` de um item.
#[derive(Clone, Debug)]
pub enum TestItemError {
    Text(String),
    Markdown(Rc<MarkdownString>),
}

impl PartialEq for TestItemError {
    /// `strictEqualComparator`: o texto por valor, o markdown (um objeto) por identidade.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (TestItemError::Text(a), TestItemError::Text(b)) => a == b,
            (TestItemError::Markdown(a), TestItemError::Markdown(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl TestItemError {
    /// O valor como o `Documentation` que o conversor `MarkdownString.fromStrict` lê.
    pub fn to_documentation(&self) -> Documentation {
        match self {
            TestItemError::Text(text) => Documentation::Text(text.clone()),
            TestItemError::Markdown(markdown) => Documentation::Markdown(markdown.as_ref().clone()),
        }
    }
}

/// O corpo de um [`TestItemImpl`].
pub struct TestItemInner {
    id: String,
    uri: Option<URI>,
    is_root: bool,
    api: Rc<ITestItemApi<TestItemImpl>>,
    children: Rc<dyn ITestItemChildren<TestItemImpl>>,
    range: RefCell<Option<Range>>,
    description: RefCell<Option<String>>,
    sort_text: RefCell<Option<String>>,
    label: RefCell<String>,
    error: RefCell<Option<TestItemError>>,
    busy: Cell<bool>,
    can_resolve_children: Cell<bool>,
    tags: RefCell<Vec<TestTag>>,
}

/// `TestItemImpl` (`vscode.TestItem`).
#[derive(Clone)]
pub struct TestItemImpl(Rc<TestItemInner>);

impl std::fmt::Debug for TestItemImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TestItemImpl({:?})", self.0.id)
    }
}

/// `evSetProps(fn)`: o evento `SetProp` de uma propriedade.
fn set_prop(update: PartialTestItem) -> ExtHostTestItemEvent<TestItemImpl> {
    ExtHostTestItemEvent::SetProp { update }
}

/// O comparador de `range`: `a === b`, ausente nunca é igual a presente e, com os dois, `isEqual`.
fn range_equals(a: &Option<Range>, b: &Option<Range>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.is_equal(b),
        _ => false,
    }
}

/// O comparador de `tags`: o mesmo tamanho e os mesmos IDs.
fn tags_equal(a: &[TestTag], b: &[TestTag]) -> bool {
    a.len() == b.len() && !a.iter().any(|t1| !b.iter().any(|t2| t1.id == t2.id))
}

impl TestItemImpl {
    /// `new TestItemImpl(controllerId, id, label, uri)`.
    pub fn new(controller_id: &str, id: &str, label: &str, uri: Option<URI>) -> Result<TestItemImpl, Error> {
        if id.contains(TEST_ID_PATH_DELIMITER) {
            // `JSON.stringify('\0')` é `"\u0000"`.
            return Err(Error::new("Test IDs may not include the \"\\u0000\" symbol"));
        }

        Ok(TestItemImpl::build(controller_id, id, label, uri, false))
    }

    /// `new TestItemRootImpl(controllerId, label)`.
    pub fn new_root(controller_id: &str, label: &str) -> TestItemImpl {
        TestItemImpl::build(controller_id, controller_id, label, None, true)
    }

    fn build(controller_id: &str, id: &str, label: &str, uri: Option<URI>, is_root: bool) -> TestItemImpl {
        let api = ITestItemApi::new(controller_id);
        let children = create_test_item_children(api.clone(), Rc::new(|item: &TestItemImpl| item.0.api.clone()));
        TestItemImpl(Rc::new(TestItemInner {
            id: id.to_string(),
            uri,
            is_root,
            api,
            children,
            range: RefCell::new(None),
            description: RefCell::new(None),
            sort_text: RefCell::new(None),
            label: RefCell::new(label.to_string()),
            error: RefCell::new(None),
            busy: Cell::new(false),
            can_resolve_children: Cell::new(false),
            tags: RefCell::new(Vec::new()),
        }))
    }

    /// `getPrivateApiFor(item)`.
    pub fn private_api(&self) -> Rc<ITestItemApi<TestItemImpl>> {
        self.0.api.clone()
    }

    /// `getPrivateApiFor(item).controllerId`.
    pub fn controller_id(&self) -> String {
        self.0.api.controller_id.clone()
    }

    /// `instanceof TestItemRootImpl`.
    pub fn is_root_impl(&self) -> bool {
        self.0.is_root
    }

    pub fn id(&self) -> String {
        self.0.id.clone()
    }

    pub fn uri(&self) -> Option<URI> {
        self.0.uri.clone()
    }

    pub fn children(&self) -> Rc<dyn ITestItemChildren<TestItemImpl>> {
        self.0.children.clone()
    }

    /// `parent`: `undefined` quando o pai é a raiz do controlador.
    pub fn parent(&self) -> Option<TestItemImpl> {
        let parent = self.0.api.parent.borrow().clone();
        parent.filter(|parent| !parent.0.is_root)
    }

    pub fn range(&self) -> Option<Range> {
        *self.0.range.borrow()
    }

    /// O setter de `range`: avisa que o documento foi sincronizado mesmo que o valor não mude.
    pub fn set_range(&self, new_value: Option<Range>) {
        self.0.api.notify(&ExtHostTestItemEvent::DocumentSynced);
        let changed = !range_equals(&self.0.range.borrow(), &new_value);
        if changed {
            *self.0.range.borrow_mut() = new_value;
            let range = new_value.map(|range| RangeConverter::from(&range));
            self.0.api.notify(&set_prop(PartialTestItem { range: Some(range), ..PartialTestItem::default() }));
        }
    }

    pub fn label(&self) -> String {
        self.0.label.borrow().clone()
    }

    pub fn set_label(&self, label: &str) {
        if *self.0.label.borrow() != label {
            *self.0.label.borrow_mut() = label.to_string();
            self.0.api.notify(&set_prop(PartialTestItem { label: Some(label.to_string()), ..PartialTestItem::default() }));
        }
    }

    pub fn description(&self) -> Option<String> {
        self.0.description.borrow().clone()
    }

    pub fn set_description(&self, description: Option<&str>) {
        let description = description.map(str::to_string);
        if *self.0.description.borrow() != description {
            *self.0.description.borrow_mut() = description.clone();
            self.0.api.notify(&set_prop(PartialTestItem { description: Some(description), ..PartialTestItem::default() }));
        }
    }

    pub fn sort_text(&self) -> Option<String> {
        self.0.sort_text.borrow().clone()
    }

    pub fn set_sort_text(&self, sort_text: Option<&str>) {
        let sort_text = sort_text.map(str::to_string);
        if *self.0.sort_text.borrow() != sort_text {
            *self.0.sort_text.borrow_mut() = sort_text.clone();
            self.0.api.notify(&set_prop(PartialTestItem { sort_text: Some(sort_text), ..PartialTestItem::default() }));
        }
    }

    pub fn busy(&self) -> bool {
        self.0.busy.get()
    }

    pub fn set_busy(&self, busy: bool) {
        if self.0.busy.get() != busy {
            self.0.busy.set(busy);
            self.0.api.notify(&set_prop(PartialTestItem { busy: Some(busy), ..PartialTestItem::default() }));
        }
    }

    pub fn error(&self) -> Option<TestItemError> {
        self.0.error.borrow().clone()
    }

    pub fn set_error(&self, error: Option<TestItemError>) {
        if *self.0.error.borrow() != error {
            *self.0.error.borrow_mut() = error.clone();
            let text = convert::error_to_text(error.as_ref());
            self.0.api.notify(&set_prop(PartialTestItem { error: Some(text), ..PartialTestItem::default() }));
        }
    }

    /// `tags`: as tags da API.
    pub fn tags(&self) -> Vec<TestTag> {
        self.0.tags.borrow().clone()
    }

    pub fn set_tags(&self, tags: Vec<TestTag>) {
        if !tags_equal(&self.0.tags.borrow(), &tags) {
            let previous = std::mem::replace(&mut *self.0.tags.borrow_mut(), tags.clone());
            self.0.api.notify(&ExtHostTestItemEvent::SetTags {
                new: tags.iter().map(convert::TestTag::from).collect(),
                old: previous.iter().map(convert::TestTag::from).collect(),
            });
        }
    }

    pub fn can_resolve_children(&self) -> bool {
        self.0.can_resolve_children.get()
    }
}

impl TestItemLike for TestItemImpl {
    fn id(&self) -> String {
        self.0.id.clone()
    }

    fn parent(&self) -> Option<Rc<dyn TestItemLike>> {
        TestItemImpl::parent(self).map(|parent| Rc::new(parent) as Rc<dyn TestItemLike>)
    }

    fn is_root(&self) -> bool {
        self.0.is_root
    }
}

impl ITestItemLike for TestItemImpl {
    fn tags(&self) -> Vec<ITestTag> {
        self.0.tags.borrow().iter().map(convert::TestTag::from).collect()
    }

    fn uri(&self) -> Option<URI> {
        self.0.uri.clone()
    }

    fn can_resolve_children(&self) -> bool {
        self.0.can_resolve_children.get()
    }

    fn set_can_resolve_children(&self, value: bool) {
        if self.0.can_resolve_children.get() != value {
            self.0.can_resolve_children.set(value);
            self.0.api.notify(&ExtHostTestItemEvent::UpdateCanResolveChildren { state: value });
        }
    }

    fn is_same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// `toItemFromPlain`.
fn to_item_from_plain(item: &crate::contrib::testing::common::test_types::ITestItemSerialized) -> TestItemImpl {
    let test_id = TestId::from_string(&item.ext_id);
    let uri = item.uri.as_ref().map(|uri| URI::revive(uri, host_platform()));
    let test_item = TestItemImpl::new(test_id.controller_id(), test_id.local_id(), &item.label, uri).unwrap_or_else(|error| throw(error));
    test_item.set_range(item.range.as_ref().map(|range| RangeConverter::to(range).unwrap_or_else(|error| throw(error))));
    test_item.set_description(item.description.as_deref().filter(|description| !description.is_empty()));
    test_item.set_sort_text(item.sort_text.as_deref().filter(|sort_text| !sort_text.is_empty()));
    test_item.set_tags(item.tags.iter().map(|tag| TestTag::new(&denamespace_test_tag(tag).1)).collect());
    test_item
}

/// `toItemFromContext`.
pub fn to_item_from_context(context: &ITestItemContext) -> Option<TestItemImpl> {
    let mut node: Option<TestItemImpl> = None;
    for test in &context.tests {
        let next = to_item_from_plain(&test.item);
        *next.0.api.parent.borrow_mut() = node;
        node = Some(next);
    }

    node
}

/// O que a coleção lê do `ExtHostDocumentsAndEditors`: a versão do documento aberto num URI.
pub trait IDocumentVersionSource {
    /// `editors.getDocument(uri)?.version`.
    fn get_document_version(&self, uri: &URI) -> Option<f64>;
}

impl IDocumentVersionSource for ExtHostDocumentsAndEditors {
    fn get_document_version(&self, uri: &URI) -> Option<f64> {
        self.get_document(uri).map(|document| document.document().version())
    }
}

/// `ExtHostTestItemCollection`.
pub struct ExtHostTestItemCollection {
    inner: Rc<TestItemCollection<TestItemImpl>>,
}

impl ExtHostTestItemCollection {
    pub fn new(controller_id: &str, controller_label: &str, editors: Rc<dyn IDocumentVersionSource>) -> Rc<ExtHostTestItemCollection> {
        let inner = TestItemCollection::new(ITestItemCollectionOptions {
            controller_id: controller_id.to_string(),
            get_document_version: Rc::new(move |uri| uri.and_then(|uri| editors.get_document_version(uri))),
            get_api_for: Rc::new(|item: &TestItemImpl| item.private_api()),
            get_children: Rc::new(|item: &TestItemImpl| item.children() as Rc<dyn ITestChildrenLike<TestItemImpl>>),
            root: TestItemImpl::new_root(controller_id, controller_label),
            to_i_test_item: Rc::new(convert::TestItem::from),
        });

        Rc::new(ExtHostTestItemCollection { inner })
    }
}

impl Deref for ExtHostTestItemCollection {
    type Target = TestItemCollection<TestItemImpl>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl IDisposable for ExtHostTestItemCollection {
    fn dispose(&self) {
        self.inner.dispose();
    }
}
