//! Porte de `src/vs/workbench/api/test/browser/extHostEditorTabs.test.ts` (suíte `ExtHostEditorTabs`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `store` é o
//!   `DisposableStore` dele.
//! - `new class extends mock<MainThreadEditorTabsShape>() { override $closeTab ... }` com `SingleProxyRPCProtocol` é
//!   [`MockMainThreadEditorTabs`] (um método sem `override` falha com `panic!`, como o método ausente do objeto vazio
//!   lançaria `TypeError`) e [`create_ext_host_editor_tabs`].
//! - `createTabDto({ ... })` é a sintaxe de atualização de struct sobre [`default_tab_dto`]. `assert.throws(() => ...)` é
//!   [`assert_throws`] (captura do pânico). `a === b` entre objetos de API é [`Rc::ptr_eq`] (as comparações de listas
//!   são [`same_groups`] e [`same_tabs`]), e `deepStrictEqual` de eventos com objetos de API iguais por identidade
//!   também.
//! - `vscode.window.tagGroups is immutable`: as quatro escritas do original (`tabGroups.activeTabGroup = undefined`,
//!   `all.length = 0` etc.) são erro de compilação aqui, porque a API só expõe leitura; o caso confere o que sobra de
//!   comportamento (a leitura do grupo ativo continua lançando e o vetor de `all` é uma cópia).
//! - `Update tab only sends tab change event` espera o evento por uma `Promise`; o ouvinte síncrono guarda o evento e a
//!   conferência vem logo depois da operação, que dispara o evento de forma síncrona.
//! - `instanceof TextMergeTabInput` e `instanceof TextTabInput` são `matches!` sobre [`AnyTabInput`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_base::JsString;
use je_workbench::api::common::ext_host_editor_tabs::{AnyTabInput, CloseTarget, ExtHostEditorTabs, Tab, TabChangeEvent, TabGroup, TabGroupChangeEvent};
use je_workbench::api::common::ext_host_protocol::{
    AnyInputDto, EditorGroupColumn, EditorTabDto, EditorTabGroupDto, ExtHostEditorTabsShape, MainThreadEditorTabsShape, TabInputKind, TabModelOperationKind,
    TabOperation, TextInputDto, TextMergeInputDto,
};
use support::test_rpc_protocol::single_proxy_rpc_protocol;

type CloseTabFn = Box<dyn Fn(Vec<String>, Option<bool>) -> Promise<bool>>;

/// O `new class extends mock<MainThreadEditorTabsShape>()` do original.
#[derive(Default)]
struct MockMainThreadEditorTabs {
    close_tab: Option<CloseTabFn>,
}

impl IDisposable for MockMainThreadEditorTabs {
    fn dispose(&self) {}
}

impl MainThreadEditorTabsShape for MockMainThreadEditorTabs {
    fn rpc_move_tab(&self, _tab_id: &str, _index: usize, _view_column: EditorGroupColumn, _preserve_focus: Option<bool>) {
        panic!("$moveTab não foi sobrescrito");
    }

    fn rpc_close_tab(&self, tab_ids: Vec<String>, preserve_focus: Option<bool>) -> Promise<bool> {
        match &self.close_tab {
            Some(close_tab) => close_tab(tab_ids, preserve_focus),
            None => panic!("$closeTab não foi sobrescrito"),
        }
    }

    fn rpc_close_group(&self, _group_ids: Vec<u32>, _preserve_focus: Option<bool>) -> Promise<bool> {
        panic!("$closeGroup não foi sobrescrito");
    }
}

fn create_ext_host_editor_tabs(mock: MockMainThreadEditorTabs) -> Rc<ExtHostEditorTabs> {
    let proxy: Rc<dyn MainThreadEditorTabsShape> = Rc::new(mock);
    ExtHostEditorTabs::new(&*single_proxy_rpc_protocol(proxy))
}

fn default_tab_dto() -> EditorTabDto {
    EditorTabDto {
        id: "uniquestring".to_owned(),
        input: AnyInputDto::Text(TextInputDto { uri: URI::parse("file://abc/def.txt", false) }),
        is_active: true,
        is_dirty: true,
        is_pinned: true,
        is_preview: false,
        label: "label1".to_owned(),
        editor_id: None,
    }
}

fn text_input(path: &str) -> AnyInputDto {
    AnyInputDto::Text(TextInputDto { uri: URI::parse(&format!("file://abc/{path}"), false) })
}

fn test_uri(path: &str) -> URI {
    URI::from(&UriComponents { scheme: Some(JsString::from("test")), path: Some(JsString::from(path)), ..UriComponents::default() }, false)
}

/// `{ isActive, viewColumn: 0, groupId, tabs }`.
fn group_dto(group_id: u32, is_active: bool, tabs: Vec<EditorTabDto>) -> EditorTabGroupDto {
    EditorTabGroupDto { is_active, view_column: 0.0, group_id, tabs }
}

fn operation(kind: TabModelOperationKind, index: usize, old_index: Option<usize>, tab_dto: EditorTabDto) -> TabOperation {
    TabOperation { kind, index, tab_dto, group_id: 12, old_index }
}

fn all_tabs(ext_host_editor_tabs: &ExtHostEditorTabs) -> Vec<Tab> {
    ext_host_editor_tabs.tab_groups().all().iter().flat_map(|group| group.tabs()).collect()
}

fn same_groups(actual: &[TabGroup], expected: &[TabGroup]) -> bool {
    actual.len() == expected.len() && actual.iter().zip(expected).all(|(a, b)| Rc::ptr_eq(a, b))
}

fn same_tabs(actual: &Option<Tab>, expected: &Option<Tab>) -> bool {
    match (actual, expected) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// `assert.throws(() => body())`.
fn assert_throws<T>(body: impl FnOnce() -> T) {
    assert!(catch_unwind(AssertUnwindSafe(body)).is_err(), "deveria lançar um erro");
}

fn uri_string(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

#[test]
fn ensure_empty_model_throws_when_accessing_active_group() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host_editor_tabs = create_ext_host_editor_tabs(MockMainThreadEditorTabs::default());

    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 0);
    // Active group should never be undefined (there is always an active group). Ensure accessing it undefined throws.
    // Pendência do original (lramos15): lançar um erro no lado principal quando um modelo é enviado sem grupo ativo.
    assert_throws(|| ext_host_editor_tabs.tab_groups().active_tab_group());
}

#[test]
fn single_tab() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host_editor_tabs = create_ext_host_editor_tabs(MockMainThreadEditorTabs::default());

    let tab = EditorTabDto { id: "uniquestring".to_owned(), is_active: true, is_dirty: true, is_pinned: true, label: "label1".to_owned(), ..default_tab_dto() };

    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![tab.clone()])]);
    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    let first = ext_host_editor_tabs.tab_groups().all()[0].clone();
    let active_tab = first.active_tab().expect("deveria haver aba ativa");
    assert_eq!(first.tabs().iter().position(|candidate| Rc::ptr_eq(candidate, &active_tab)), Some(0));

    {
        ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![tab.clone()])]);
        assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
        let first = ext_host_editor_tabs.tab_groups().all()[0].clone();
        let active_tab = first.active_tab().expect("deveria haver aba ativa");
        assert_eq!(first.tabs().iter().position(|candidate| Rc::ptr_eq(candidate, &active_tab)), Some(0));
    }
}

#[test]
fn empty_tab_group() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host_editor_tabs = create_ext_host_editor_tabs(MockMainThreadEditorTabs::default());

    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![])]);
    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    let first = ext_host_editor_tabs.tab_groups().all()[0].clone();
    assert!(first.active_tab().is_none());
    assert_eq!(first.tabs().len(), 0);
}

#[test]
fn ensure_tab_group_change_events_fires() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let ext_host_editor_tabs = create_ext_host_editor_tabs(MockMainThreadEditorTabs::default());

    let count = Rc::new(RefCell::new(0));
    let counter = count.clone();
    store.add(ext_host_editor_tabs.tab_groups().on_did_change_tab_groups().listen(move |_| *counter.borrow_mut() += 1));

    assert_eq!(*count.borrow(), 0);

    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![])]);
    let active_tab_group = ext_host_editor_tabs.tab_groups().active_tab_group();
    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    assert_eq!(active_tab_group.tabs().len(), 0);
    assert_eq!(*count.borrow(), 1);
}

#[test]
fn check_tab_group_change_event_properties() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let ext_host_editor_tabs = create_ext_host_editor_tabs(MockMainThreadEditorTabs::default());

    let group1_data = group_dto(12, true, vec![]);
    let group2_data = EditorTabGroupDto { group_id: 13, ..group1_data.clone() };

    let events: Rc<RefCell<Vec<TabGroupChangeEvent>>> = Rc::default();
    let sink = events.clone();
    store.add(ext_host_editor_tabs.tab_groups().on_did_change_tab_groups().listen(move |event| sink.borrow_mut().push(event.clone())));
    let tab_groups = ext_host_editor_tabs.tab_groups();

    // OPEN
    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group1_data.clone()]);
    {
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert!(same_groups(&events[0].changed, &[]));
        assert!(same_groups(&events[0].closed, &[]));
        assert!(same_groups(&events[0].opened, &[tab_groups.active_tab_group()]));
    }

    // OPEN, CHANGE
    events.borrow_mut().clear();
    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![EditorTabGroupDto { is_active: false, ..group1_data.clone() }, group2_data.clone()]);
    {
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert!(same_groups(&events[0].changed, &[tab_groups.all()[0].clone()]));
        assert!(same_groups(&events[0].closed, &[]));
        assert!(same_groups(&events[0].opened, &[tab_groups.all()[1].clone()]));
    }

    // CHANGE
    events.borrow_mut().clear();
    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group1_data.clone(), EditorTabGroupDto { is_active: false, ..group2_data.clone() }]);
    {
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert!(same_groups(&events[0].changed, &tab_groups.all()));
        assert!(same_groups(&events[0].closed, &[]));
        assert!(same_groups(&events[0].opened, &[]));
    }

    // CLOSE, CHANGE
    events.borrow_mut().clear();
    let old_active_group = tab_groups.active_tab_group();
    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group2_data.clone()]);
    {
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert!(same_groups(&events[0].changed, &tab_groups.all()));
        assert!(same_groups(&events[0].closed, &[old_active_group]));
        assert!(same_groups(&events[0].opened, &[]));
    }
}

#[test]
fn ensure_reference_equality_for_active_tab_and_active_group() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host_editor_tabs = create_ext_host_editor_tabs(MockMainThreadEditorTabs::default());
    let tab = EditorTabDto {
        id: "uniquestring".to_owned(),
        is_active: true,
        is_dirty: true,
        is_pinned: true,
        label: "label1".to_owned(),
        editor_id: Some("default".to_owned()),
        ..default_tab_dto()
    };

    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![tab])]);
    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    let first = ext_host_editor_tabs.tab_groups().all()[0].clone();
    let active_tab = first.active_tab().expect("deveria haver aba ativa");
    assert_eq!(first.tabs().iter().position(|candidate| Rc::ptr_eq(candidate, &active_tab)), Some(0));
    assert!(Rc::ptr_eq(&active_tab, &first.tabs()[0]));
    assert!(Rc::ptr_eq(&ext_host_editor_tabs.tab_groups().active_tab_group(), &first));
}

#[test]
fn text_merge_tab_input_surfaces_in_the_ui() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host_editor_tabs = create_ext_host_editor_tabs(MockMainThreadEditorTabs::default());

    let tab = EditorTabDto {
        input: AnyInputDto::TextMerge(TextMergeInputDto {
            base: test_uri("base"),
            input1: test_uri("input1"),
            input2: test_uri("input2"),
            result: test_uri("result"),
        }),
        ..default_tab_dto()
    };

    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![tab])]);
    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    let first = ext_host_editor_tabs.tab_groups().all()[0].clone();
    let active_tab = first.active_tab().expect("deveria haver aba ativa");
    assert_eq!(first.tabs().iter().position(|candidate| Rc::ptr_eq(candidate, &active_tab)), Some(0));
    assert!(matches!(active_tab.input(), Some(AnyTabInput::TextMerge(_))));
}

#[test]
fn ensure_reference_stability() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host_editor_tabs = create_ext_host_editor_tabs(MockMainThreadEditorTabs::default());
    let tab_dto = default_tab_dto();

    // single dirty tab

    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![tab_dto.clone()])]);
    let mut all = all_tabs(&ext_host_editor_tabs);
    assert_eq!(all.len(), 1);
    let api_tab1 = all[0].clone();
    let Some(AnyTabInput::Text(input1)) = api_tab1.input() else { panic!("a entrada deveria ser TextTabInput") };
    assert_eq!(tab_dto.input.kind(), TabInputKind::TextInput);
    let AnyInputDto::Text(dto_resource) = &tab_dto.input else { panic!("o DTO deveria ser TextInputDto") };
    assert_eq!(uri_string(&input1.uri), uri_string(&dto_resource.uri));
    assert!(api_tab1.is_dirty());

    // NOT DIRTY anymore

    let tab_dto2 = EditorTabDto { is_dirty: false, ..tab_dto.clone() };
    // Accept a simple update
    ext_host_editor_tabs.rpc_accept_tab_operation(operation(TabModelOperationKind::TabUpdate, 0, None, tab_dto2));

    all = all_tabs(&ext_host_editor_tabs);
    assert_eq!(all.len(), 1);
    let api_tab2 = all[0].clone();
    let Some(AnyTabInput::Text(input1)) = api_tab1.input() else { panic!("a entrada deveria ser TextTabInput") };
    assert_eq!(uri_string(&input1.uri), uri_string(&dto_resource.uri));
    assert!(!api_tab2.is_dirty());

    assert!(Rc::ptr_eq(&api_tab1, &api_tab2));
}

#[test]
fn tab_is_active_working() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host_editor_tabs = create_ext_host_editor_tabs(MockMainThreadEditorTabs::default());
    let tab_dto_aaa = EditorTabDto {
        id: "AAA".to_owned(),
        is_active: true,
        is_dirty: true,
        is_pinned: true,
        label: "label1".to_owned(),
        input: text_input("AAA.txt"),
        editor_id: Some("default".to_owned()),
        ..default_tab_dto()
    };

    let tab_dto_bbb = EditorTabDto {
        id: "BBB".to_owned(),
        is_active: false,
        is_dirty: true,
        is_pinned: true,
        label: "label1".to_owned(),
        input: text_input("BBB.txt"),
        editor_id: Some("default".to_owned()),
        ..default_tab_dto()
    };

    // single dirty tab

    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![tab_dto_aaa.clone(), tab_dto_bbb.clone()])]);

    let all = all_tabs(&ext_host_editor_tabs);
    assert_eq!(all.len(), 2);

    let active_tab1 = ext_host_editor_tabs.tab_groups().active_tab_group().active_tab();
    let Some(AnyTabInput::Text(active_input1)) = active_tab1.as_ref().and_then(|tab| tab.input()) else { panic!("a entrada deveria ser TextTabInput") };
    assert_eq!(tab_dto_aaa.input.kind(), TabInputKind::TextInput);
    let AnyInputDto::Text(dto_aaa_resource) = &tab_dto_aaa.input else { panic!("o DTO deveria ser TextInputDto") };
    assert_eq!(uri_string(&active_input1.uri), uri_string(&dto_aaa_resource.uri));
    assert!(active_tab1.as_ref().unwrap().is_active());

    ext_host_editor_tabs.rpc_accept_tab_operation(operation(
        TabModelOperationKind::TabUpdate,
        1,
        None,
        EditorTabDto { is_active: true, ..tab_dto_bbb.clone() }, // BBB is now active
    ));

    let active_tab2 = ext_host_editor_tabs.tab_groups().active_tab_group().active_tab();
    let Some(AnyTabInput::Text(active_input2)) = active_tab2.as_ref().and_then(|tab| tab.input()) else { panic!("a entrada deveria ser TextTabInput") };
    assert_eq!(tab_dto_bbb.input.kind(), TabInputKind::TextInput);
    let AnyInputDto::Text(dto_bbb_resource) = &tab_dto_bbb.input else { panic!("o DTO deveria ser TextInputDto") };
    assert_eq!(uri_string(&active_input2.uri), uri_string(&dto_bbb_resource.uri));
    assert!(active_tab2.as_ref().unwrap().is_active());
    assert!(!active_tab1.as_ref().unwrap().is_active());
}

#[test]
fn vscode_window_tag_groups_is_immutable() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host_editor_tabs = create_ext_host_editor_tabs(MockMainThreadEditorTabs::default());

    // `tabGroups.activeTabGroup = undefined`, `tabGroups.all.length = 0`, `tabGroups.onDidChangeActiveTabGroup = undefined`
    // e `tabGroups.onDidChangeTabGroups = undefined` não compilam aqui (a API só expõe leitura). O que resta:
    // o vetor de `all` é uma cópia, e a leitura do grupo ativo do modelo vazio continua lançando.
    let mut all = ext_host_editor_tabs.tab_groups().all();
    all.clear();
    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 0);
    assert_throws(|| ext_host_editor_tabs.tab_groups().active_tab_group());

    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![default_tab_dto()])]);
    let mut all = ext_host_editor_tabs.tab_groups().all();
    all.clear();
    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
}

#[test]
fn ensure_close_is_called_with_all_tab_ids() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let _store = guard.store();
    run_with_faked_timers(async move {
        let closed_tab_ids: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
        let sink = closed_tab_ids.clone();
        let ext_host_editor_tabs = create_ext_host_editor_tabs(MockMainThreadEditorTabs {
            // override $moveTab or $closeTab
            close_tab: Some(Box::new(move |tab_ids, _preserve_focus| {
                sink.borrow_mut().push(tab_ids);
                Promise::resolved(true)
            })),
        });
        let tab = EditorTabDto {
            id: "uniquestring".to_owned(),
            is_active: true,
            is_dirty: true,
            is_pinned: true,
            label: "label1".to_owned(),
            editor_id: Some("default".to_owned()),
            ..default_tab_dto()
        };

        ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![tab])]);
        assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
        let active_tab = ext_host_editor_tabs.tab_groups().active_tab_group().active_tab().expect("deveria haver aba ativa");
        let first_close = ext_host_editor_tabs.tab_groups().close(CloseTarget::Tab(active_tab.clone()), Some(false));
        assert_eq!(closed_tab_ids.borrow().len(), 1);
        assert_eq!(closed_tab_ids.borrow()[0], vec!["uniquestring".to_owned()]);
        // Close with array
        let second_close = ext_host_editor_tabs.tab_groups().close(CloseTarget::Tabs(vec![active_tab]), Some(false));
        assert_eq!(closed_tab_ids.borrow().len(), 2);
        assert_eq!(closed_tab_ids.borrow()[1], vec!["uniquestring".to_owned()]);
        assert!(first_close.await.unwrap());
        assert!(second_close.await.unwrap());
    });
}

#[test]
fn update_tab_only_sends_tab_change_event() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    let closed_tab_ids: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
    let sink = closed_tab_ids.clone();
    let ext_host_editor_tabs = create_ext_host_editor_tabs(MockMainThreadEditorTabs {
        // override $moveTab or $closeTab
        close_tab: Some(Box::new(move |tab_ids, _preserve_focus| {
            sink.borrow_mut().push(tab_ids);
            Promise::resolved(true)
        })),
    });
    let tab_dto = EditorTabDto {
        id: "uniquestring".to_owned(),
        is_active: true,
        is_dirty: true,
        is_pinned: true,
        label: "label1".to_owned(),
        editor_id: Some("default".to_owned()),
        ..default_tab_dto()
    };

    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![tab_dto.clone()])]);

    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    assert_eq!(all_tabs(&ext_host_editor_tabs).len(), 1);

    let tab = ext_host_editor_tabs.tab_groups().all()[0].tabs()[0].clone();

    let p: Rc<RefCell<Option<TabChangeEvent>>> = Rc::default();
    let sink = p.clone();
    store.add(ext_host_editor_tabs.tab_groups().on_did_change_tabs().listen(move |event| *sink.borrow_mut() = Some(event.clone())));

    ext_host_editor_tabs.rpc_accept_tab_operation(operation(
        TabModelOperationKind::TabUpdate,
        0,
        None,
        EditorTabDto { label: "NEW LABEL".to_owned(), ..tab_dto },
    ));

    let event = p.borrow().clone().expect("o evento deveria ter disparado");
    let changed_tab = event.changed[0].clone();

    assert!(Rc::ptr_eq(&tab, &changed_tab));
    assert_eq!(changed_tab.label(), "NEW LABEL");
}

#[test]
fn active_tab() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host_editor_tabs = create_ext_host_editor_tabs(MockMainThreadEditorTabs::default());

    let mut tab1 = EditorTabDto { id: "uniquestring".to_owned(), is_active: true, is_dirty: true, is_pinned: true, label: "label1".to_owned(), ..default_tab_dto() };

    let mut tab2 = EditorTabDto { is_active: false, id: "uniquestring2".to_owned(), ..default_tab_dto() };

    let tab3 = EditorTabDto { is_active: false, id: "uniquestring3".to_owned(), ..default_tab_dto() };

    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![tab1.clone(), tab2.clone(), tab3.clone()])]);

    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    assert_eq!(all_tabs(&ext_host_editor_tabs).len(), 3);

    let active_of = |index: Option<usize>| {
        let group = ext_host_editor_tabs.tab_groups().active_tab_group();
        match index {
            Some(index) => (group.active_tab(), group.tabs().get(index).cloned()),
            None => (group.active_tab(), None),
        }
    };

    // Active tab is correct
    let (active, expected) = active_of(Some(0));
    assert!(same_tabs(&active, &expected));

    // Switching active tab works
    tab1.is_active = false;
    tab2.is_active = true;
    ext_host_editor_tabs.rpc_accept_tab_operation(operation(TabModelOperationKind::TabUpdate, 0, None, tab1.clone()));
    ext_host_editor_tabs.rpc_accept_tab_operation(operation(TabModelOperationKind::TabUpdate, 1, None, tab2.clone()));
    let (active, expected) = active_of(Some(1));
    assert!(same_tabs(&active, &expected));

    // Closing tabs out works
    let tab3 = EditorTabDto { is_active: true, ..tab3 };
    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![tab3])]);
    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    assert_eq!(all_tabs(&ext_host_editor_tabs).len(), 1);
    let (active, expected) = active_of(Some(0));
    assert!(same_tabs(&active, &expected));

    // Closing out all tabs returns undefine active tab
    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![])]);
    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    assert_eq!(all_tabs(&ext_host_editor_tabs).len(), 0);
    let (active, _) = active_of(None);
    assert!(active.is_none());
}

#[test]
fn tab_operations_patches_open_and_close_correctly() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host_editor_tabs = create_ext_host_editor_tabs(MockMainThreadEditorTabs::default());

    let tab1 = EditorTabDto { id: "uniquestring".to_owned(), is_active: true, label: "label1".to_owned(), ..default_tab_dto() };

    let tab2 = EditorTabDto { is_active: false, id: "uniquestring2".to_owned(), label: "label2".to_owned(), ..default_tab_dto() };

    let mut tab3 = EditorTabDto { is_active: false, id: "uniquestring3".to_owned(), label: "label3".to_owned(), ..default_tab_dto() };

    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![tab1.clone(), tab2.clone(), tab3.clone()])]);

    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    assert_eq!(all_tabs(&ext_host_editor_tabs).len(), 3);

    // Close tab 2
    ext_host_editor_tabs.rpc_accept_tab_operation(operation(TabModelOperationKind::TabClose, 1, None, tab2.clone()));
    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    assert_eq!(all_tabs(&ext_host_editor_tabs).len(), 2);

    // Close active tab and update tab 3 to be active
    ext_host_editor_tabs.rpc_accept_tab_operation(operation(TabModelOperationKind::TabClose, 0, None, tab1.clone()));
    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    assert_eq!(all_tabs(&ext_host_editor_tabs).len(), 1);
    tab3.is_active = true;
    ext_host_editor_tabs.rpc_accept_tab_operation(operation(TabModelOperationKind::TabUpdate, 0, None, tab3.clone()));
    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    assert_eq!(all_tabs(&ext_host_editor_tabs).len(), 1);
    assert_eq!(ext_host_editor_tabs.tab_groups().all()[0].active_tab().map(|tab| tab.label()), Some("label3".to_owned()));

    // Open tab 2 back
    ext_host_editor_tabs.rpc_accept_tab_operation(operation(TabModelOperationKind::TabOpen, 1, None, tab2.clone()));
    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    assert_eq!(all_tabs(&ext_host_editor_tabs).len(), 2);
    assert_eq!(ext_host_editor_tabs.tab_groups().all()[0].tabs().get(1).map(|tab| tab.label()), Some("label2".to_owned()));
}

#[test]
fn tab_operations_patches_move_correctly() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host_editor_tabs = create_ext_host_editor_tabs(MockMainThreadEditorTabs::default());

    let tab1 = EditorTabDto { id: "uniquestring".to_owned(), is_active: true, label: "label1".to_owned(), ..default_tab_dto() };

    let tab2 = EditorTabDto { is_active: false, id: "uniquestring2".to_owned(), label: "label2".to_owned(), ..default_tab_dto() };

    let tab3 = EditorTabDto { is_active: false, id: "uniquestring3".to_owned(), label: "label3".to_owned(), ..default_tab_dto() };

    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![tab1, tab2.clone(), tab3.clone()])]);

    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    assert_eq!(all_tabs(&ext_host_editor_tabs).len(), 3);

    let label_at = |index: usize| ext_host_editor_tabs.tab_groups().all()[0].tabs().get(index).map(|tab| tab.label());

    // Move tab 2 to index 0
    ext_host_editor_tabs.rpc_accept_tab_operation(operation(TabModelOperationKind::TabMove, 0, Some(1), tab2));
    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    assert_eq!(all_tabs(&ext_host_editor_tabs).len(), 3);
    assert_eq!(label_at(0), Some("label2".to_owned()));

    // Move tab 3 to index 1
    ext_host_editor_tabs.rpc_accept_tab_operation(operation(TabModelOperationKind::TabMove, 1, Some(2), tab3));
    assert_eq!(ext_host_editor_tabs.tab_groups().all().len(), 1);
    assert_eq!(all_tabs(&ext_host_editor_tabs).len(), 3);
    assert_eq!(label_at(1), Some("label3".to_owned()));
    assert_eq!(label_at(0), Some("label2".to_owned()));
    assert_eq!(label_at(2), Some("label1".to_owned()));
}

#[test]
fn reference_stability_across_full_model_resync() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_host_editor_tabs = create_ext_host_editor_tabs(MockMainThreadEditorTabs::default());

    let tab_aaa = EditorTabDto { id: "AAA".to_owned(), label: "AAA".to_owned(), is_active: true, input: text_input("AAA.txt"), ..default_tab_dto() };
    let tab_bbb = EditorTabDto { id: "BBB".to_owned(), label: "BBB".to_owned(), is_active: false, input: text_input("BBB.txt"), ..default_tab_dto() };

    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![group_dto(12, true, vec![tab_aaa.clone(), tab_bbb.clone()])]);

    let group_before = ext_host_editor_tabs.tab_groups().all()[0].clone();
    let tab_aaa_before = group_before.tabs()[0].clone();
    let tab_bbb_before = group_before.tabs()[1].clone();

    // A second group is opened: the existing model is resent wholesale, but
    // the surviving group/tab objects must keep their identity so that
    // extensions keying Maps/WeakMaps by them keep working.
    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![
        group_dto(12, false, vec![tab_aaa.clone(), tab_bbb.clone()]),
        EditorTabGroupDto { is_active: true, view_column: 1.0, group_id: 13, tabs: vec![] },
    ]);

    let group_after = ext_host_editor_tabs.tab_groups().all().into_iter().find(|group| group.tabs().len() == 2).expect("grupo com duas abas");
    assert!(Rc::ptr_eq(&group_after, &group_before));
    assert!(Rc::ptr_eq(&group_after.tabs()[0], &tab_aaa_before));
    assert!(Rc::ptr_eq(&group_after.tabs()[1], &tab_bbb_before));

    // A tab is closed during the resync: the survivor keeps its identity,
    // and the removed one does not reappear.
    ext_host_editor_tabs.rpc_accept_editor_tab_model(vec![
        group_dto(12, false, vec![EditorTabDto { is_active: true, ..tab_aaa }]),
        EditorTabGroupDto { is_active: true, view_column: 1.0, group_id: 13, tabs: vec![] },
    ]);

    let surviving_group = ext_host_editor_tabs.tab_groups().all().into_iter().find(|group| group.tabs().len() == 1).expect("grupo com uma aba");
    assert!(Rc::ptr_eq(&surviving_group, &group_before));
    assert_eq!(surviving_group.tabs().len(), 1);
    assert!(Rc::ptr_eq(&surviving_group.tabs()[0], &tab_aaa_before));
    assert!(Rc::ptr_eq(&surviving_group.active_tab().expect("deveria haver aba ativa"), &tab_aaa_before));
}
