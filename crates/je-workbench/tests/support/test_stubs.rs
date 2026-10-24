//! Os auxiliares de `src/vs/workbench/contrib/testing/test/common/testStubs.ts` que os testes de resultados usam:
//! `TestTestItem`, `TestTestCollection`, `testStubs.nested` e `getInitializedMainTestCollection` (mais o dublê de
//! `IUriIdentityService` que os testes do original montam com `upcastPartial`).
//!
//! Divergências:
//! - **`makeSimpleStubTree`** não entra: nenhum teste portado até aqui o usa.
//! - **`TestTestItem` como `T` da coleção.** O item é o handle [`TestItemHandle`] (um `Rc<TestTestItem>`, num tipo
//!   local porque a regra de órfãos não deixa implementar os traits do `je-workbench` para um `Rc`).
//! - **`TestTestItem.get(key)` e `set(key, value)`** (genéricos sobre a chave de `ITestItem`) são
//!   [`TestTestItem::props`] (o `ITestItem` atual) e [`TestTestItem::set`] (aplica um `PartialTestItem` e avisa o
//!   `SetProp`).
//! - **`TestTestCollection extends TestItemCollection`** é [`TestTestCollection`], com `Deref` para a coleção;
//!   `currentDiff` e `setDiff` são [`TestTestCollection::current_diff`] e [`TestTestCollection::set_diff`].
//! - **`(root as TestTestItem & { _isRoot: boolean })._isRoot = true`** é o `is_root` do item.
//! - **`testStubs.nested(idPrefix = 'id-')`** é [`nested`], com [`DEFAULT_ID_PREFIX`] para o padrão.
//! - **`getInitializedMainTestCollection(singleUse = testStubs.nested())`** devolve `Promise<Rc<MainThreadTestCollection>>`;
//!   o `async (t, l) => singleUse.expand(t, l)` é um fecho que devolve a promessa da expansão (ou uma resolvida).
//! - **`upcastPartial<IUriIdentityService>({ asCanonicalUri })`** é [`TestUriIdentityService`]: só `asCanonicalUri`
//!   existe; `extUri` lança `not implemented`, como chamar um membro ausente.

use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::errors::{Error, throw};
use je_base::lifecycle::IDisposable;
use je_base::promise::{MaybePromise, Promise};
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_platform::resources::uri_file;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_workbench::contrib::testing::common::main_thread_test_collection::MainThreadTestCollection;
use je_workbench::contrib::testing::common::test_id::{TestId, TestItemLike};
use je_workbench::contrib::testing::common::test_item_collection::{
    ExtHostTestItemEvent, ITestChildrenLike, ITestItemApi, ITestItemChildren, ITestItemCollectionOptions, ITestItemLike, TestItemCollection, create_test_item_children,
};
use je_workbench::contrib::testing::common::test_service::IMainThreadTestCollection;
use je_workbench::contrib::testing::common::test_types::{ITestItem, ITestTag, PartialTestItem, TestsDiff};

/// `testStubs.nested`: o prefixo padrão dos IDs.
pub const DEFAULT_ID_PREFIX: &str = "id-";

/// `{ asCanonicalUri(uri) { return uri; } } as IUriIdentityService`.
pub struct TestUriIdentityService;

impl IUriIdentityService for TestUriIdentityService {
    fn ext_uri(&self) -> ExtUri {
        throw(Error::type_error("not implemented"))
    }

    fn as_canonical_uri(&self, uri: &URI) -> URI {
        uri.clone()
    }
}

pub struct TestTestItem {
    props: RefCell<ITestItem>,
    can_resolve_children: Cell<bool>,
    ext_id: TestId,
    is_root: Cell<bool>,
    pub api: Rc<ITestItemApi<TestItemHandle>>,
    pub children: Rc<dyn ITestItemChildren<TestItemHandle>>,
}

/// O item como o `T` da coleção.
#[derive(Clone)]
pub struct TestItemHandle(pub Rc<TestTestItem>);

impl TestTestItem {
    pub fn new(ext_id: TestId, label: &str, uri: Option<URI>) -> TestItemHandle {
        let api = ITestItemApi::new(ext_id.controller_id());
        let children = create_test_item_children(api.clone(), Rc::new(|item: &TestItemHandle| item.0.api.clone()));
        let props = ITestItem {
            ext_id: ext_id.to_string(),
            busy: false,
            description: None,
            error: None,
            label: label.to_string(),
            range: None,
            sort_text: None,
            tags: Vec::new(),
            uri,
        };

        TestItemHandle(Rc::new(TestTestItem { props: RefCell::new(props), can_resolve_children: Cell::new(false), ext_id, is_root: Cell::new(false), api, children }))
    }

    pub fn tags(&self) -> Vec<ITestTag> {
        self.props.borrow().tags.iter().map(|id| ITestTag { id: id.clone() }).collect()
    }

    pub fn set_tags(&self, value: Vec<ITestTag>) {
        let old: Vec<ITestTag> = self.tags();
        self.api.notify(&ExtHostTestItemEvent::SetTags { new: value.clone(), old });
        self.props.borrow_mut().tags = value.into_iter().map(|tag| tag.id).collect();
    }

    pub fn can_resolve_children(&self) -> bool {
        self.can_resolve_children.get()
    }

    pub fn set_can_resolve_children(&self, value: bool) {
        self.can_resolve_children.set(value);
        self.api.notify(&ExtHostTestItemEvent::UpdateCanResolveChildren { state: value });
    }

    pub fn parent(&self) -> Option<TestItemHandle> {
        self.api.parent.borrow().clone()
    }

    pub fn id(&self) -> String {
        self.ext_id.local_id().to_string()
    }

    /// `get(key)`: o `ITestItem` atual (a propriedade é um campo dele).
    pub fn props(&self) -> ITestItem {
        self.props.borrow().clone()
    }

    /// `set(key, value)`: aplica o patch (uma propriedade) e avisa `SetProp`.
    pub fn set(&self, update: PartialTestItem) {
        self.props.borrow_mut().assign(&update);
        self.api.notify(&ExtHostTestItemEvent::SetProp { update });
    }

    pub fn to_test_item(&self) -> ITestItem {
        let mut props = self.props.borrow().clone();
        props.ext_id = self.ext_id.to_string();
        props
    }
}

impl TestItemHandle {
    pub fn id(&self) -> String {
        self.0.id()
    }

    pub fn children(&self) -> Rc<dyn ITestItemChildren<TestItemHandle>> {
        self.0.children.clone()
    }

    pub fn to_test_item(&self) -> ITestItem {
        self.0.to_test_item()
    }
}

impl TestItemLike for TestItemHandle {
    fn id(&self) -> String {
        self.0.id()
    }

    fn parent(&self) -> Option<Rc<dyn TestItemLike>> {
        self.0.parent().map(|parent| Rc::new(parent) as Rc<dyn TestItemLike>)
    }

    fn is_root(&self) -> bool {
        self.0.is_root.get()
    }
}

impl ITestItemLike for TestItemHandle {
    fn tags(&self) -> Vec<ITestTag> {
        self.0.tags()
    }

    fn uri(&self) -> Option<URI> {
        self.0.props.borrow().uri.clone()
    }

    fn can_resolve_children(&self) -> bool {
        self.0.can_resolve_children()
    }

    fn set_can_resolve_children(&self, value: bool) {
        self.0.set_can_resolve_children(value);
    }

    fn is_same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

pub struct TestTestCollection {
    inner: Rc<TestItemCollection<TestItemHandle>>,
}

impl TestTestCollection {
    pub fn new(controller_id: &str) -> Rc<TestTestCollection> {
        let root = TestTestItem::new(TestId::new(vec![controller_id.to_string()]), "root", None);
        root.0.is_root.set(true);

        let inner = TestItemCollection::new(ITestItemCollectionOptions {
            controller_id: controller_id.to_string(),
            get_api_for: Rc::new(|item: &TestItemHandle| item.0.api.clone()),
            to_i_test_item: Rc::new(|item: &TestItemHandle| item.0.to_test_item()),
            get_children: Rc::new(|item: &TestItemHandle| item.0.children.clone() as Rc<dyn ITestChildrenLike<TestItemHandle>>),
            get_document_version: Rc::new(|_| None),
            root,
        });

        Rc::new(TestTestCollection { inner })
    }

    pub fn current_diff(&self) -> TestsDiff {
        self.inner.diff()
    }

    pub fn set_diff(&self, diff: TestsDiff) {
        self.inner.set_diff(diff);
    }
}

impl Deref for TestTestCollection {
    type Target = TestItemCollection<TestItemHandle>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl IDisposable for TestTestCollection {
    fn dispose(&self) {
        self.inner.dispose();
    }
}

/// `testStubs.nested(idPrefix)`.
pub fn nested(id_prefix: &str) -> Rc<TestTestCollection> {
    let collection = TestTestCollection::new("ctrlId");
    let root = collection.root().clone();
    let id_prefix = id_prefix.to_string();
    collection.set_resolve_handler(Some(Rc::new(move |item: Option<&TestItemHandle>| {
        match item {
            None => {
                let a = TestTestItem::new(TestId::new(vec!["ctrlId".to_string(), "id-a".to_string()]), "a", Some(uri_file("/")));
                a.set_can_resolve_children(true);
                let b = TestTestItem::new(TestId::new(vec!["ctrlId".to_string(), "id-b".to_string()]), "b", Some(uri_file("/")));
                root.children().add(a);
                root.children().add(b);
            }
            Some(item) if item.id() == format!("{id_prefix}a") => {
                item.children().add(TestTestItem::new(
                    TestId::new(vec!["ctrlId".to_string(), "id-a".to_string(), "id-aa".to_string()]),
                    "aa",
                    Some(uri_file("/")),
                ));
                item.children().add(TestTestItem::new(
                    TestId::new(vec!["ctrlId".to_string(), "id-a".to_string(), "id-ab".to_string()]),
                    "ab",
                    Some(uri_file("/")),
                ));
            }
            Some(_) => {}
        }
        MaybePromise::Value(())
    })));

    collection
}

/// Gets a main thread test collection initialized with the given set of
/// roots/stubs.
pub fn get_initialized_main_test_collection(single_use: Rc<TestTestCollection>) -> Promise<Rc<MainThreadTestCollection>> {
    async_fn(async move {
        let expander = single_use.clone();
        let c = Rc::new(MainThreadTestCollection::new(
            Rc::new(TestUriIdentityService),
            Rc::new(move |id: &str, levels: f64| expander.expand(id, levels).unwrap_or_else(|| Promise::resolved(()))),
        ));
        if let Some(expansion) = single_use.expand(&single_use.root().id(), f64::INFINITY) {
            expansion.await?;
        }
        c.apply(&single_use.collect_diff());
        single_use.dispose();
        Ok(c)
    })
}
