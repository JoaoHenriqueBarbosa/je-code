//! Porte de `src/vs/workbench/contrib/debug/test/browser/breakpoints.test.ts` (suíte `Debug - Breakpoints`), caso a caso e na mesma ordem.
//!
//! Os 13 casos foram portados, na ordem: `simple`, `toggling`, `two files`, `conditions`, `function breakpoints`,
//! `multiple sessions`, `exception breakpoints`, `exception breakpoints multiple sessions`, `instruction breakpoints`,
//! `data breakpoints`, `message and class name`, `decorations`, `updates when storage changes`.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` de cada caso; o `setup` (`createMockDebugModel`) roda
//!   no começo de cada um.
//! - `uri.file(path)` é `uri_file`; `getExpandedBodySize` e `getBreakpointMessageAndIcon` são os de `breakpoints_view.rs`.
//! - `{ lineNumber: 100 }` de `IBreakpointUpdateData` é `line_number: Some(Some(100))` (o `Some` externo é o
//!   `hasOwnProperty`).
//! - `DebugProtocol.Breakpoint` é `DapBreakpoint`; `{ supportsConditionalBreakpoints: true }` é o objeto JSON de capacidades.
//! - `storage.store(key, value, WORKSPACE, USER, /* external= */ true)` é o `store_with` do `TestStorageService`.
//! - `MockLabelService` é o dublê do workbench; `result.icon.id` é o `id` do `ThemeIcon`.
//! - **`decorations`.** O `TestInstantiationService` com `IDebugService` (`MockDebugService` com `getModel => model`),
//!   `ILabelService` (`MockLabelService`) e `ILanguageService` (`LanguageService`) é o `BreakpointDecorationServices` de
//!   `breakpoint_editor_contribution.rs`; o `getAdapterManager()` que o `MockDebugService` não define (e que nenhuma
//!   sessão chama aqui) é um dublê que entra em pânico. `createTextModel(text, languageId)` é o `create_text_model_with` do
//!   je-editor; `decorations[i].options.before?.inlineClassName` e os demais campos leem o `ModelDecorationOptionsInit`.
//!   O `assert.deepStrictEqual(glyphMarginHoverMessage, expected)` compara o `MarkdownString` completo do je-base campo a
//!   campo (`value`, `isTrusted`, `supportThemeIcons`, `supportHtml`, `supportAlertSyntax`), porque o tipo não deriva `PartialEq`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;
#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::html_content::{IsTrusted, MarkdownString, MarkdownStringOptions, TrustedOrOptions};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::uri::URI;
use je_editor::language_service::LanguageService;
use je_editor::model::{OverviewRulerLane, RelaxedTextModelCreationOptions};
use je_editor::range::Range;
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::log::NullLogService;
use je_platform::resources::uri_file;
use je_platform::storage::storage::{StorageScope, StorageService};
use je_workbench::contrib::debug::breakpoint_editor_contribution::{BreakpointDecorationServices, IDebuggerLookup, create_breakpoint_decorations};
use je_workbench::contrib::debug::debugger::Debugger;
use je_workbench::contrib::debug::breakpoints::{
    DapBreakpoint, DataBreakpointOptions, ExceptionBreakpointsFilter, FunctionBreakpointOptions, IBaseBreakpointOptions, InstructionBreakpointOptions,
};
use je_workbench::contrib::debug::breakpoints_view::{get_breakpoint_message_and_icon, get_expanded_body_size};
use je_workbench::contrib::debug::debug::{DataBreakpointSource, IDebugSession};
use je_workbench::contrib::debug::debug_model::{BreakpointData, BreakpointElement, BreakpointFilter, BreakpointsChangeEvent, DebugModel};
use je_workbench::contrib::debug::debug_protocol::DataBreakpointAccessType;
use je_workbench::contrib::debug::debug_types::{IBreakpointUpdateData, State};
use je_workbench::test::browser::workbench_test_services::MockLabelService;
use je_workbench::test::common::workbench_test_services::TestStorageService;
use serde_json::json;
use support::mock_debug::{NotDirty, create_mock_debug_model, create_test_session, mock_debug_storage, mock_uri_identity_service, storage_service_of, store_external};

fn data(line_number: i64, enabled: Option<bool>) -> BreakpointData {
    BreakpointData { line_number, enabled, ..BreakpointData::default() }
}

fn add_breakpoints_and_check_events(model: &Rc<DebugModel>, uri: &URI, data: Vec<BreakpointData>) -> Vec<Rc<je_workbench::contrib::debug::breakpoints::Breakpoint>> {
    let event_count = Rc::new(Cell::new(0));
    let to_dispose: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
    let line_numbers: Vec<i64> = data.iter().map(|d| d.line_number).collect();
    let listener_count = event_count.clone();
    let listener_dispose = to_dispose.clone();
    let subscription = model.on_did_change_breakpoints().listen(move |e: &Option<BreakpointsChangeEvent>| {
        let e = e.as_ref().expect("o evento");
        assert!(!e.session_only);
        assert!(e.changed.is_none());
        assert!(e.removed.is_none());
        let added = e.added.as_ref().expect("added");
        assert_eq!(added.len(), line_numbers.len());
        listener_count.set(listener_count.get() + 1);
        if let Some(subscription) = listener_dispose.borrow_mut().take() {
            subscription.dispose();
        }
        for (i, line_number) in line_numbers.iter().enumerate() {
            match &added[i] {
                BreakpointElement::Breakpoint(bp) => assert_eq!(bp.line_number(), *line_number),
                _ => panic!("o elemento adicionado não é um Breakpoint"),
            }
        }
    });
    *to_dispose.borrow_mut() = Some(subscription);
    let bps = model.add_breakpoints(uri, data, true);
    assert_eq!(event_count.get(), 1);
    bps
}

// Breakpoints

#[test]
fn simple() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let model_uri = uri_file("/myfolder/myfile.js");

    add_breakpoints_and_check_events(&model, &model_uri, vec![data(5, Some(true)), data(10, Some(false))]);
    assert!(model.are_breakpoints_activated());
    assert_eq!(model.get_breakpoints(None).len(), 2);

    let event_count = Rc::new(Cell::new(0));
    let to_dispose: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
    let listener_count = event_count.clone();
    let listener_dispose = to_dispose.clone();
    *to_dispose.borrow_mut() = Some(model.on_did_change_breakpoints().listen(move |e| {
        listener_count.set(listener_count.get() + 1);
        let e = e.as_ref().expect("o evento");
        assert!(e.added.is_none());
        assert!(!e.session_only);
        assert_eq!(e.removed.as_ref().map(Vec::len), Some(2));
        assert!(e.changed.is_none());

        if let Some(subscription) = listener_dispose.borrow_mut().take() {
            subscription.dispose();
        }
    }));

    model.remove_breakpoints(&model.get_breakpoints(None));
    assert_eq!(event_count.get(), 1);
    assert_eq!(model.get_breakpoints(None).len(), 0);
}

#[test]
fn toggling() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let model_uri = uri_file("/myfolder/myfile.js");

    add_breakpoints_and_check_events(&model, &model_uri, vec![data(5, Some(true)), data(10, Some(false))]);
    add_breakpoints_and_check_events(
        &model,
        &model_uri,
        vec![BreakpointData { line_number: 12, enabled: Some(true), condition: Some("fake condition".to_owned()), ..BreakpointData::default() }],
    );
    assert_eq!(model.get_breakpoints(None).len(), 3);
    let bp = model.get_breakpoints(None).pop();
    if let Some(bp) = bp {
        model.remove_breakpoints(&[bp]);
    }
    assert_eq!(model.get_breakpoints(None).len(), 2);

    model.set_breakpoints_activated(false);
    assert!(!model.are_breakpoints_activated());
    model.set_breakpoints_activated(true);
    assert!(model.are_breakpoints_activated());
}

#[test]
fn two_files() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let model_uri1 = uri_file("/myfolder/my file first.js");
    let model_uri2 = uri_file("/secondfolder/second/second file.js");
    add_breakpoints_and_check_events(&model, &model_uri1, vec![data(5, Some(true)), data(10, Some(false))]);
    assert_eq!(get_expanded_body_size(&model, None, 9), 44);

    add_breakpoints_and_check_events(&model, &model_uri2, vec![data(1, Some(true)), data(2, Some(true)), data(3, Some(false))]);
    assert_eq!(get_expanded_body_size(&model, None, 9), 110);

    assert_eq!(model.get_breakpoints(None).len(), 5);
    assert_eq!(model.get_breakpoints(Some(&BreakpointFilter { uri: Some(model_uri1.clone()), ..BreakpointFilter::default() })).len(), 2);
    assert_eq!(model.get_breakpoints(Some(&BreakpointFilter { uri: Some(model_uri2.clone()), ..BreakpointFilter::default() })).len(), 3);
    assert_eq!(model.get_breakpoints(Some(&BreakpointFilter { line_number: Some(5), ..BreakpointFilter::default() })).len(), 1);
    assert_eq!(model.get_breakpoints(Some(&BreakpointFilter { column: Some(5), ..BreakpointFilter::default() })).len(), 0);

    let bp = model.get_breakpoints(None)[0].clone();
    let mut update = HashMap::new();
    update.insert(bp.get_id(), IBreakpointUpdateData { line_number: Some(Some(100)), ..IBreakpointUpdateData::default() });
    let event_fired = Rc::new(Cell::new(false));
    let to_dispose: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
    let listener_fired = event_fired.clone();
    let listener_dispose = to_dispose.clone();
    *to_dispose.borrow_mut() = Some(model.on_did_change_breakpoints().listen(move |e| {
        listener_fired.set(true);
        let e = e.as_ref().expect("o evento");
        assert!(e.added.is_none());
        assert!(e.removed.is_none());
        assert_eq!(e.changed.as_ref().map(Vec::len), Some(1));
        if let Some(subscription) = listener_dispose.borrow_mut().take() {
            subscription.dispose();
        }
    }));
    model.update_breakpoints(&update);
    assert!(event_fired.get());
    assert_eq!(bp.line_number(), 100);

    assert_eq!(model.get_breakpoints(Some(&BreakpointFilter { enabled_only: true, ..BreakpointFilter::default() })).len(), 3);
    model.enable_or_disable_all_breakpoints(false);
    for bp in model.get_breakpoints(None) {
        assert!(!bp.enabled());
    }
    assert_eq!(model.get_breakpoints(Some(&BreakpointFilter { enabled_only: true, ..BreakpointFilter::default() })).len(), 0);

    model.set_enablement(&BreakpointElement::Breakpoint(bp.clone()), true);
    assert!(bp.enabled());

    model.remove_breakpoints(&model.get_breakpoints(Some(&BreakpointFilter { uri: Some(model_uri1.clone()), ..BreakpointFilter::default() })));
    assert_eq!(get_expanded_body_size(&model, None, 9), 66);

    assert_eq!(model.get_breakpoints(None).len(), 3);
}

#[test]
fn conditions() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let model_uri1 = uri_file("/myfolder/my file first.js");
    add_breakpoints_and_check_events(
        &model,
        &model_uri1,
        vec![
            BreakpointData { line_number: 5, condition: Some("i < 5".to_owned()), hit_condition: Some("17".to_owned()), ..BreakpointData::default() },
            BreakpointData { line_number: 10, condition: Some("j < 3".to_owned()), ..BreakpointData::default() },
        ],
    );
    let breakpoints = model.get_breakpoints(None);

    assert_eq!(breakpoints[0].condition().as_deref(), Some("i < 5"));
    assert_eq!(breakpoints[0].hit_condition().as_deref(), Some("17"));
    assert_eq!(breakpoints[1].condition().as_deref(), Some("j < 3"));
    assert!(!breakpoints[1].hit_condition().is_some_and(|c| !c.is_empty()));

    assert_eq!(model.get_breakpoints(None).len(), 2);
    model.remove_breakpoints(&model.get_breakpoints(None));
    assert_eq!(model.get_breakpoints(None).len(), 0);
}

#[test]
fn function_breakpoints() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    model.add_function_breakpoint(FunctionBreakpointOptions { name: "foo".to_owned(), ..FunctionBreakpointOptions::default() }, Some("1".to_owned()));
    model.add_function_breakpoint(FunctionBreakpointOptions { name: "bar".to_owned(), ..FunctionBreakpointOptions::default() }, Some("2".to_owned()));
    model.update_function_breakpoint("1", Some("fooUpdated"), None, None);
    model.update_function_breakpoint("2", Some("barUpdated"), None, None);

    let function_bps = model.get_function_breakpoints();
    assert_eq!(*function_bps[0].name.borrow(), "fooUpdated");
    assert_eq!(*function_bps[1].name.borrow(), "barUpdated");

    model.remove_function_breakpoints(None);
    assert_eq!(model.get_function_breakpoints().len(), 0);
}

#[test]
fn multiple_sessions() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let model_uri = uri_file("/myfolder/myfile.js");
    add_breakpoints_and_check_events(
        &model,
        &model_uri,
        vec![BreakpointData { line_number: 5, enabled: Some(true), condition: Some("x > 5".to_owned()), ..BreakpointData::default() }, data(10, Some(false))],
    );
    let breakpoints = model.get_breakpoints(None);
    let session = guard.add_rc(create_test_session(&model, "mockSession", None, &guard.store()));
    let mut data = HashMap::new();

    assert_eq!(breakpoints[0].line_number(), 5);
    assert_eq!(breakpoints[1].line_number(), 10);

    data.insert(breakpoints[0].get_id(), DapBreakpoint { verified: false, line: Some(10), ..DapBreakpoint::default() });
    data.insert(breakpoints[1].get_id(), DapBreakpoint { verified: true, line: Some(50), ..DapBreakpoint::default() });
    model.set_breakpoint_session_data(&session.get_id(), &json!({}), Some(&data));
    assert_eq!(breakpoints[0].line_number(), 5);
    assert_eq!(breakpoints[1].line_number(), 50);

    let session2 = guard.add_rc(create_test_session(&model, "mockSession", None, &guard.store()));
    let mut data2 = HashMap::new();
    data2.insert(breakpoints[0].get_id(), DapBreakpoint { verified: true, line: Some(100), ..DapBreakpoint::default() });
    data2.insert(breakpoints[1].get_id(), DapBreakpoint { verified: true, line: Some(500), ..DapBreakpoint::default() });
    model.set_breakpoint_session_data(&session2.get_id(), &json!({}), Some(&data2));

    // Breakpoint is verified only once, show that line
    assert_eq!(breakpoints[0].line_number(), 100);
    // Breakpoint is verified two times, show the original line
    assert_eq!(breakpoints[1].line_number(), 10);

    model.set_breakpoint_session_data(&session.get_id(), &json!({}), None);
    // No more double session verification
    assert_eq!(breakpoints[0].line_number(), 100);
    assert_eq!(breakpoints[1].line_number(), 500);

    assert!(!breakpoints[0].supported());
    model.set_breakpoint_session_data(&session2.get_id(), &json!({ "supportsConditionalBreakpoints": true }), Some(&data2));
    assert!(breakpoints[0].supported());
}

fn filter(filter: &str, label: &str, default: Option<bool>) -> ExceptionBreakpointsFilter {
    ExceptionBreakpointsFilter { filter: filter.to_owned(), label: label.to_owned(), default, ..ExceptionBreakpointsFilter::default() }
}

#[test]
fn exception_breakpoints() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let event_count = Rc::new(Cell::new(0));
    let counter = event_count.clone();
    guard.add(model.on_did_change_breakpoints().listen(move |_| counter.set(counter.get() + 1)));
    model.set_exception_breakpoints_for_session("session-id-1", &[filter("uncaught", "UNCAUGHT", Some(true))]);
    assert_eq!(event_count.get(), 1);
    let mut exception_breakpoints = model.get_exception_breakpoints_for_session(Some("session-id-1"));
    assert_eq!(exception_breakpoints.len(), 1);
    assert_eq!(exception_breakpoints[0].filter, "uncaught");
    assert!(exception_breakpoints[0].enabled());

    model.set_exception_breakpoints_for_session("session-id-2", &[filter("uncaught", "UNCAUGHT", None), filter("caught", "CAUGHT", None)]);
    assert_eq!(event_count.get(), 2);
    exception_breakpoints = model.get_exception_breakpoints_for_session(Some("session-id-2"));
    assert_eq!(exception_breakpoints.len(), 2);
    assert_eq!(exception_breakpoints[0].filter, "uncaught");
    assert!(exception_breakpoints[0].enabled());
    assert_eq!(exception_breakpoints[1].filter, "caught");
    assert_eq!(exception_breakpoints[1].label, "CAUGHT");
    assert!(!exception_breakpoints[1].enabled());

    model.set_exception_breakpoints_for_session("session-id-3", &[filter("all", "ALL", None)]);
    assert_eq!(event_count.get(), 3);
    assert_eq!(model.get_exception_breakpoints_for_session(Some("session-id-3")).len(), 1);
    exception_breakpoints = model.get_exception_breakpoints();
    assert_eq!(exception_breakpoints[0].filter, "uncaught");
    assert!(exception_breakpoints[0].enabled());
    assert_eq!(exception_breakpoints[1].filter, "caught");
    assert_eq!(exception_breakpoints[1].label, "CAUGHT");
    assert!(!exception_breakpoints[1].enabled());
    assert_eq!(exception_breakpoints[2].filter, "all");
    assert_eq!(exception_breakpoints[2].label, "ALL");
}

#[test]
fn exception_breakpoints_multiple_sessions() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let event_count = Rc::new(Cell::new(0));
    let counter = event_count.clone();
    guard.add(model.on_did_change_breakpoints().listen(move |_| counter.set(counter.get() + 1)));

    model.set_exception_breakpoints_for_session("session-id-4", &[filter("uncaught", "UNCAUGHT", Some(true)), filter("caught", "CAUGHT", None)]);
    model.set_exception_breakpoint_fallback_session("session-id-4");
    assert_eq!(event_count.get(), 1);
    let mut exception_breakpoints_for_session = model.get_exception_breakpoints_for_session(Some("session-id-4"));
    assert_eq!(exception_breakpoints_for_session.len(), 2);
    assert_eq!(exception_breakpoints_for_session[0].filter, "uncaught");
    assert_eq!(exception_breakpoints_for_session[1].filter, "caught");

    model.set_exception_breakpoints_for_session("session-id-5", &[filter("all", "ALL", None), filter("caught", "CAUGHT", None)]);
    assert_eq!(event_count.get(), 2);
    exception_breakpoints_for_session = model.get_exception_breakpoints_for_session(Some("session-id-5"));
    let mut exception_breakpoints_for_undefined = model.get_exception_breakpoints_for_session(None);
    assert_eq!(exception_breakpoints_for_session.len(), 2);
    assert_eq!(exception_breakpoints_for_session[0].filter, "caught");
    assert_eq!(exception_breakpoints_for_session[1].filter, "all");
    assert_eq!(exception_breakpoints_for_undefined.len(), 2);
    assert_eq!(exception_breakpoints_for_undefined[0].filter, "uncaught");
    assert_eq!(exception_breakpoints_for_undefined[1].filter, "caught");

    model.remove_exception_breakpoints_for_session("session-id-4");
    assert_eq!(event_count.get(), 2);
    exception_breakpoints_for_undefined = model.get_exception_breakpoints_for_session(None);
    assert_eq!(exception_breakpoints_for_undefined.len(), 2);
    assert_eq!(exception_breakpoints_for_undefined[0].filter, "uncaught");
    assert_eq!(exception_breakpoints_for_undefined[1].filter, "caught");

    model.set_exception_breakpoint_fallback_session("session-id-5");
    assert_eq!(event_count.get(), 2);
    exception_breakpoints_for_undefined = model.get_exception_breakpoints_for_session(None);
    assert_eq!(exception_breakpoints_for_undefined.len(), 2);
    assert_eq!(exception_breakpoints_for_undefined[0].filter, "caught");
    assert_eq!(exception_breakpoints_for_undefined[1].filter, "all");

    let exception_breakpoints = model.get_exception_breakpoints();
    assert_eq!(exception_breakpoints.len(), 3);
}

#[test]
fn instruction_breakpoints() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let event_count = Rc::new(Cell::new(0));
    let counter = event_count.clone();
    guard.add(model.on_did_change_breakpoints().listen(move |_| counter.set(counter.get() + 1)));
    //address: string, offset: number, condition?: string, hitCondition?: string
    model.add_instruction_breakpoint(InstructionBreakpointOptions {
        instruction_reference: "0xCCCCFFFF".to_owned(),
        offset: 0,
        address: 0,
        can_persist: false,
        base: IBaseBreakpointOptions::default(),
    });

    assert_eq!(event_count.get(), 1);
    let mut instruction_breakpoints = model.get_instruction_breakpoints();
    assert_eq!(instruction_breakpoints.len(), 1);
    assert_eq!(instruction_breakpoints[0].instruction_reference, "0xCCCCFFFF");
    assert_eq!(instruction_breakpoints[0].offset, 0);

    model.add_instruction_breakpoint(InstructionBreakpointOptions {
        instruction_reference: "0xCCCCEEEE".to_owned(),
        offset: 1,
        address: 0,
        can_persist: false,
        base: IBaseBreakpointOptions::default(),
    });
    assert_eq!(event_count.get(), 2);
    instruction_breakpoints = model.get_instruction_breakpoints();
    assert_eq!(instruction_breakpoints.len(), 2);
    assert_eq!(instruction_breakpoints[0].instruction_reference, "0xCCCCFFFF");
    assert_eq!(instruction_breakpoints[0].offset, 0);
    assert_eq!(instruction_breakpoints[1].instruction_reference, "0xCCCCEEEE");
    assert_eq!(instruction_breakpoints[1].offset, 1);
}

fn data_breakpoint_options(description: &str, data_id: &str, can_persist: bool, access_type: DataBreakpointAccessType) -> DataBreakpointOptions {
    DataBreakpointOptions {
        base: IBaseBreakpointOptions::default(),
        description: description.to_owned(),
        src: DataBreakpointSource::Variable { data_id: data_id.to_owned() },
        can_persist,
        initial_session_data: None,
        access_types: Some(vec![access_type]),
        access_type,
    }
}

#[test]
fn data_breakpoints() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let event_count = Rc::new(Cell::new(0));
    let counter = event_count.clone();
    guard.add(model.on_did_change_breakpoints().listen(move |_| counter.set(counter.get() + 1)));

    model.add_data_breakpoint(data_breakpoint_options("label", "id", true, DataBreakpointAccessType::Read), Some("1".to_owned()));
    model.add_data_breakpoint(data_breakpoint_options("second", "secondId", false, DataBreakpointAccessType::ReadWrite), Some("2".to_owned()));
    model.update_data_breakpoint("1", None, Some("aCondition"));
    model.update_data_breakpoint("2", Some("10"), None);
    let data_breakpoints = model.get_data_breakpoints();
    assert!(data_breakpoints[0].can_persist);
    assert_eq!(data_breakpoints[0].src, DataBreakpointSource::Variable { data_id: "id".to_owned() });
    assert_eq!(data_breakpoints[0].access_type, DataBreakpointAccessType::Read);
    assert_eq!(data_breakpoints[0].base.condition.borrow().as_deref(), Some("aCondition"));
    assert!(!data_breakpoints[1].can_persist);
    assert_eq!(data_breakpoints[1].description, "second");
    assert_eq!(data_breakpoints[1].access_type, DataBreakpointAccessType::ReadWrite);
    assert_eq!(data_breakpoints[1].base.hit_condition.borrow().as_deref(), Some("10"));

    assert_eq!(event_count.get(), 4);

    model.remove_data_breakpoints(Some(&data_breakpoints[0].get_id()));
    assert_eq!(event_count.get(), 5);
    assert_eq!(model.get_data_breakpoints().len(), 1);

    model.remove_data_breakpoints(None);
    assert_eq!(model.get_data_breakpoints().len(), 0);
    assert_eq!(event_count.get(), 6);
}

#[test]
fn message_and_class_name() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let model_uri = uri_file("/myfolder/my file first.js");
    add_breakpoints_and_check_events(
        &model,
        &model_uri,
        vec![
            BreakpointData { line_number: 5, enabled: Some(true), condition: Some("x > 5".to_owned()), ..BreakpointData::default() },
            data(10, Some(false)),
            BreakpointData { line_number: 12, enabled: Some(true), log_message: Some("hello".to_owned()), ..BreakpointData::default() },
            BreakpointData { line_number: 15, enabled: Some(true), hit_condition: Some("12".to_owned()), ..BreakpointData::default() },
            data(500, Some(true)),
        ],
    );
    let breakpoints = model.get_breakpoints(None);
    let ls = MockLabelService::new(None);
    let bp = |index: usize| BreakpointElement::Breakpoint(breakpoints[index].clone());

    let mut result = get_breakpoint_message_and_icon(State::Stopped, true, &bp(0), &*ls, &model);
    assert_eq!(result.message.as_deref(), Some("Condition: x > 5"));
    assert_eq!(result.icon.id, "debug-breakpoint-conditional");

    result = get_breakpoint_message_and_icon(State::Stopped, true, &bp(1), &*ls, &model);
    assert_eq!(result.message.as_deref(), Some("Disabled Breakpoint"));
    assert_eq!(result.icon.id, "debug-breakpoint-disabled");

    result = get_breakpoint_message_and_icon(State::Stopped, true, &bp(2), &*ls, &model);
    assert_eq!(result.message.as_deref(), Some("Log Message: hello"));
    assert_eq!(result.icon.id, "debug-breakpoint-log");

    result = get_breakpoint_message_and_icon(State::Stopped, true, &bp(3), &*ls, &model);
    assert_eq!(result.message.as_deref(), Some("Hit Count: 12"));
    assert_eq!(result.icon.id, "debug-breakpoint-conditional");

    result = get_breakpoint_message_and_icon(State::Stopped, true, &bp(4), &*ls, &model);
    assert_eq!(result.message, Some(ls.get_uri_label(&breakpoints[4].uri(), &UriLabelOptions::default())));
    assert_eq!(result.icon.id, "debug-breakpoint");

    result = get_breakpoint_message_and_icon(State::Stopped, false, &bp(2), &*ls, &model);
    assert_eq!(result.message.as_deref(), Some("Disabled Logpoint"));
    assert_eq!(result.icon.id, "debug-breakpoint-log-disabled");

    model.add_data_breakpoint(data_breakpoint_options("label", "id", true, DataBreakpointAccessType::Read), None);
    let data_breakpoints = model.get_data_breakpoints();
    result = get_breakpoint_message_and_icon(State::Stopped, true, &BreakpointElement::Data(data_breakpoints[0].clone()), &*ls, &model);
    assert_eq!(result.message.as_deref(), Some("Data Breakpoint"));
    assert_eq!(result.icon.id, "debug-breakpoint-data");

    let function_breakpoint = model.add_function_breakpoint(FunctionBreakpointOptions { name: "foo".to_owned(), ..FunctionBreakpointOptions::default() }, Some("1".to_owned()));
    result = get_breakpoint_message_and_icon(State::Stopped, true, &BreakpointElement::Function(function_breakpoint.clone()), &*ls, &model);
    assert_eq!(result.message.as_deref(), Some("Function Breakpoint"));
    assert_eq!(result.icon.id, "debug-breakpoint-function");

    let mut data = HashMap::new();
    data.insert(breakpoints[0].get_id(), DapBreakpoint { verified: false, line: Some(10), ..DapBreakpoint::default() });
    data.insert(breakpoints[1].get_id(), DapBreakpoint { verified: true, line: Some(50), ..DapBreakpoint::default() });
    data.insert(breakpoints[2].get_id(), DapBreakpoint { verified: true, line: Some(50), message: Some("world".to_owned()), ..DapBreakpoint::default() });
    data.insert(function_breakpoint.get_id(), DapBreakpoint { verified: true, ..DapBreakpoint::default() });
    model.set_breakpoint_session_data(
        "mocksessionid",
        &json!({ "supportsFunctionBreakpoints": false, "supportsDataBreakpoints": true, "supportsLogPoints": true }),
        Some(&data),
    );

    result = get_breakpoint_message_and_icon(State::Stopped, true, &bp(0), &*ls, &model);
    assert_eq!(result.message.as_deref(), Some("Unverified Breakpoint"));
    assert_eq!(result.icon.id, "debug-breakpoint-unverified");

    result = get_breakpoint_message_and_icon(State::Stopped, true, &BreakpointElement::Function(function_breakpoint.clone()), &*ls, &model);
    assert_eq!(result.message.as_deref(), Some("Function breakpoints not supported by this debug type"));
    assert_eq!(result.icon.id, "debug-breakpoint-function-unverified");

    result = get_breakpoint_message_and_icon(State::Stopped, true, &bp(2), &*ls, &model);
    assert_eq!(result.message.as_deref(), Some("Log Message: hello, world"));
    assert_eq!(result.icon.id, "debug-breakpoint-log");
}

/// O `getAdapterManager()` que o `MockDebugService` não define: nenhuma sessão chama o `getDebugger` neste caso.
struct UnusedAdapterManager;

impl IDebuggerLookup for UnusedAdapterManager {
    fn get_debugger(&self, _debug_type: &str) -> Option<Rc<Debugger>> {
        panic!("getAdapterManager is not a function")
    }
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[test]
fn decorations() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let model_uri = uri_file("/myfolder/my file first.js");
    let language_id = "testMode";
    let text_model = test_text_model::create_text_model_with(
        [
            "this is line one",
            "this is line two",
            "    this is line three it has whitespace at start",
            "this is line four",
            "this is line five",
        ]
        .join("\n")
        .as_str(),
        Some(language_id),
        RelaxedTextModelCreationOptions::default(),
        None,
    );
    add_breakpoints_and_check_events(
        &model,
        &model_uri,
        vec![
            BreakpointData { line_number: 1, enabled: Some(true), condition: Some("x > 5".to_owned()), ..BreakpointData::default() },
            BreakpointData { line_number: 2, column: Some(4), enabled: Some(false), ..BreakpointData::default() },
            BreakpointData { line_number: 3, enabled: Some(true), log_message: Some("hello".to_owned()), ..BreakpointData::default() },
            BreakpointData { line_number: 500, enabled: Some(true), ..BreakpointData::default() },
        ],
    );
    let breakpoints = model.get_breakpoints(None);

    let services = BreakpointDecorationServices {
        debug_model: model.clone(),
        adapter_manager: Rc::new(UnusedAdapterManager),
        label_service: MockLabelService::new(None),
        language_service: guard.add_rc(LanguageService::new(false)),
    };
    let mut decorations = create_breakpoint_decorations(&services, &text_model, &breakpoints, State::Running, true, true);
    assert_eq!(decorations.len(), 3); // last breakpoint filtered out since it has a large line number
    assert_eq!(decorations[0].range, Range::new(1, 1, 1, 2));
    assert_eq!(decorations[1].range, Range::new(2, 4, 2, 5));
    assert_eq!(decorations[2].range, Range::new(3, 5, 3, 6));
    assert_eq!(decorations[0].options.before_content_class_name, None);
    let before = decorations[1].options.before.as_ref();
    assert_eq!(before.and_then(|before| before.inline_class_name.as_deref()), Some("debug-breakpoint-placeholder"));
    assert_eq!(before.and_then(|before| before.width_in_em), Some(0.9));
    assert_eq!(decorations[0].options.overview_ruler.as_ref().and_then(|overview_ruler| overview_ruler.position), Some(OverviewRulerLane::Left));
    let mut expected = MarkdownString::new(
        None,
        TrustedOrOptions::Options(MarkdownStringOptions {
            is_trusted: Some(IsTrusted::Bool(true)),
            support_theme_icons: Some(true),
            ..MarkdownStringOptions::default()
        }),
    );
    expected.append_codeblock(&utf16(language_id), &utf16("Condition: x > 5"));
    let actual = decorations[0].glyph_margin_hover_message.as_ref().expect("o hover da margem");
    assert_eq!(actual.value, expected.value);
    assert_eq!(actual.is_trusted, expected.is_trusted);
    assert_eq!(actual.support_theme_icons, expected.support_theme_icons);
    assert_eq!(actual.support_html, expected.support_html);
    assert_eq!(actual.support_alert_syntax, expected.support_alert_syntax);

    decorations = create_breakpoint_decorations(&services, &text_model, &breakpoints, State::Running, true, false);
    assert_eq!(decorations[0].options.overview_ruler, None);

    text_model.dispose();
}

#[test]
fn updates_when_storage_changes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let storage1 = guard.add_rc(TestStorageService::new());
    let debug_storage1 = guard.add_rc(mock_debug_storage(storage_service_of(&storage1)));
    let model1 = guard.add_rc(DebugModel::new(debug_storage1.clone(), Rc::new(NotDirty), Some(mock_uri_identity_service()), NullLogService::shared()));

    // 1. create breakpoints in the first model
    let model_uri = uri_file("/myfolder/my file first.js");
    let first = vec![
        BreakpointData { line_number: 1, enabled: Some(true), condition: Some("x > 5".to_owned()), ..BreakpointData::default() },
        BreakpointData { line_number: 2, column: Some(4), enabled: Some(false), ..BreakpointData::default() },
    ];

    add_breakpoints_and_check_events(&model1, &model_uri, first);
    debug_storage1.store_breakpoints(&model1);
    let stored = StorageService::get(&storage1, "debug.breakpoint", StorageScope::Workspace, None).unwrap();

    // 2. hydrate a new model and ensure external breakpoints get applied
    let storage2 = guard.add_rc(TestStorageService::new());
    let model2 = guard.add_rc(DebugModel::new(
        guard.add_rc(mock_debug_storage(storage_service_of(&storage2))),
        Rc::new(NotDirty),
        Some(mock_uri_identity_service()),
        NullLogService::shared(),
    ));
    store_external(&storage2, "debug.breakpoint", &stored, true);
    let ids = |model: &Rc<DebugModel>| -> Vec<String> { model.get_breakpoints(None).iter().map(|b| b.get_id()).collect() };
    assert_eq!(ids(&model2), ids(&model1));

    // 3. ensure non-external changes are ignored
    store_external(&storage2, "debug.breakpoint", "[]", false);
    assert_eq!(ids(&model2), ids(&model1));
}
