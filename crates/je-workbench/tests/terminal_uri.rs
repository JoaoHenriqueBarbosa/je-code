//! Porte de `src/vs/workbench/contrib/terminal/test/browser/terminalUri.test.ts` (suíte `terminalUri`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `deepStrictEqual` vira `assert_eq!`; `strictEqual` sobre o objeto devolvido vira igualdade de ponteiro.
//! - `fakeDragEvent(data)` vira um `DataTransferGetData` que ignora o formato pedido.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::uri::URI;
use je_workbench::contrib::terminal::terminal_uri::{
    DataTransferGetData, ITerminalInstanceResource, IPartialDragEvent, get_instance_from_resource,
    get_terminal_resources_from_drag_event, get_terminal_uri,
};

struct FakeDataTransfer(&'static str);

impl DataTransferGetData for FakeDataTransfer {
    fn get_data(&self, _format: &str) -> JsString {
        JsString::from(self.0)
    }
}

/// `getTerminalResourcesFromDragEvent(fakeDragEvent(data))?.map(e => e.toString())`.
fn drag_event_resources(data: &'static str) -> Option<Vec<String>> {
    let data_transfer = FakeDataTransfer(data);
    let event = IPartialDragEvent { data_transfer: Some(&data_transfer) };
    get_terminal_resources_from_drag_event(&event)
        .map(|resources| resources.iter().map(|e| e.to_string(false).to_string_lossy()).collect())
}

struct FakeInstance {
    resource: URI,
}

impl ITerminalInstanceResource for FakeInstance {
    fn resource(&self) -> &URI {
        &self.resource
    }
}

fn fake(instance_id: f64, title: &str) -> FakeInstance {
    FakeInstance { resource: get_terminal_uri("workspace", instance_id, Some(title), None) }
}

// suite('getTerminalResourcesFromDragEvent')

#[test]
fn should_give_undefined_when_no_terminal_resources_is_in_event() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(drag_event_resources(""), None);
}

#[test]
fn should_give_undefined_when_an_empty_terminal_resources_array_is_in_event() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(drag_event_resources("[]"), None);
}

#[test]
fn should_return_terminal_resource_when_event_contains_one() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        drag_event_resources("[\"vscode-terminal:/1626874386474/3\"]"),
        Some(vec!["vscode-terminal:/1626874386474/3".to_string()])
    );
}

#[test]
fn should_return_multiple_terminal_resources_when_event_contains_multiple() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        drag_event_resources("[\"vscode-terminal:/foo/1\",\"vscode-terminal:/bar/2\"]"),
        Some(vec!["vscode-terminal:/foo/1".to_string(), "vscode-terminal:/bar/2".to_string()])
    );
}

// suite('getInstanceFromResource')

#[test]
fn should_return_undefined_if_there_is_no_match() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let instances = [fake(2.0, "title")];
    let wanted = get_terminal_uri("workspace", 1.0, Some("title"), None);
    assert!(get_instance_from_resource(&instances, Some(&wanted)).is_none());
}

#[test]
fn should_return_a_result_if_there_is_a_match() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let instances = [fake(1.0, "title"), fake(2.0, "title"), fake(3.0, "title")];
    let wanted = get_terminal_uri("workspace", 2.0, Some("title"), None);
    let found = get_instance_from_resource(&instances, Some(&wanted));
    assert!(std::ptr::eq(found.unwrap(), &instances[1]));
}

#[test]
fn should_ignore_the_fragment() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let instances = [fake(1.0, "title"), fake(2.0, "title"), fake(3.0, "title")];
    let wanted = get_terminal_uri("workspace", 2.0, Some("does not match!"), None);
    let found = get_instance_from_resource(&instances, Some(&wanted));
    assert!(std::ptr::eq(found.unwrap(), &instances[1]));
}
