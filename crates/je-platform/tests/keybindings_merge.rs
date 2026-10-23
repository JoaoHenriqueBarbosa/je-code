//! Porte de `src/vs/platform/userDataSync/test/common/keybindingsMerge.test.ts` (31 casos, na ordem do
//! original) e da classe `TestUserDataSyncUtilService` de `test/common/userDataSyncClient.ts`.

mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::json::{Object, Value};
use je_base::json_formatter::FormattingOptions;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::user_data_sync::keybindings_merge::{MergeResult, merge};
use je_platform::user_data_sync::user_data_sync::IUserDataSyncUtilService;

use common::*;

/// `TestUserDataSyncUtilService`. O `resolveDefaultCoreIgnoredSettings` do original devolve
/// `getDefaultIgnoredSettings()` (de `userDataSync.ts`, ainda não portado); nenhum caso deste arquivo o chama.
struct TestUserDataSyncUtilService;

impl IUserDataSyncUtilService for TestUserDataSyncUtilService {
    fn resolve_user_bindings(&self, userbindings: Vec<JsString>) -> Promise<HashMap<JsString, JsString>> {
        Promise::resolved(userbindings.into_iter().map(|keybinding| (keybinding.clone(), keybinding)).collect())
    }

    fn resolve_formatting_options(&self, _file: Option<&URI>) -> Promise<FormattingOptions> {
        Promise::resolved(FormattingOptions {
            eol: Some(JsString::from("\n")),
            insert_spaces: Some(false),
            tab_size: Some(4),
        })
    }

    fn resolve_default_core_ignored_settings(&self) -> Promise<Vec<JsString>> {
        Promise::resolved(Vec::new())
    }
}

const WHEN: &str = "editorTextFocus && !editorReadonly";

/// `{ key, command }`.
fn k(key: &str, command: &str) -> Value {
    entry(key, command, None, None)
}

/// `{ key, command, when }`.
fn kw(key: &str, command: &str, when: &str) -> Value {
    entry(key, command, Some(when), None)
}

/// `{ key, command, args: { text } }`.
fn ka(key: &str, command: &str, text: &str) -> Value {
    let mut args = Object::new();
    args.insert(JsString::from("text"), Value::from(text));
    entry(key, command, None, Some(Value::Object(args)))
}

fn entry(key: &str, command: &str, when: Option<&str>, args: Option<Value>) -> Value {
    let mut object = Object::new();
    object.insert(JsString::from("key"), Value::from(key));
    object.insert(JsString::from("command"), Value::from(command));
    if let Some(when) = when {
        object.insert(JsString::from("when"), Value::from(when));
    }
    if let Some(args) = args {
        object.insert(JsString::from("args"), args);
    }
    Value::Object(object)
}

/// `JSON.stringify(value, null, '\t')`.
fn stringify(items: &[Value]) -> String {
    let mut out = String::new();
    write_value(&Value::Array(items.to_vec()), 0, &mut out);
    out
}

fn write_value(value: &Value, depth: usize, out: &mut String) {
    let indent = |depth: usize| "\t".repeat(depth);
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(&b.to_string()),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => {
            out.push('"');
            for c in s.to_string_lossy().chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Array(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                out.push_str(&indent(depth + 1));
                write_value(item, depth + 1, out);
                out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&indent(depth));
            out.push(']');
        }
        Value::Object(object) if object.is_empty() => out.push_str("{}"),
        Value::Object(object) => {
            out.push_str("{\n");
            let count = object.len();
            for (i, (name, item)) in object.iter().enumerate() {
                out.push_str(&indent(depth + 1));
                write_value(&Value::String(name.clone()), depth + 1, out);
                out.push_str(": ");
                write_value(item, depth + 1, out);
                out.push_str(if i + 1 < count { ",\n" } else { "\n" });
            }
            out.push_str(&indent(depth));
            out.push('}');
        }
    }
}

/// Um texto escrito por linhas (os literais com tabulação do original).
fn text(lines: &[&str]) -> String {
    lines.join("\n")
}

fn merge_keybindings(local_content: &str, remote_content: &str, base_content: Option<&str>) -> MergeResult {
    let (local_content, remote_content) = (local_content.to_string(), remote_content.to_string());
    let base_content = base_content.map(str::to_string);
    let slot: Rc<RefCell<Option<MergeResult>>> = Rc::new(RefCell::new(None));
    let out = slot.clone();
    run_with_faked_timers(async move {
        let user_data_sync_util_service = TestUserDataSyncUtilService;
        let formatting_options =
            user_data_sync_util_service.resolve_formatting_options(None).await.expect("resolveFormattingOptions");
        let result = merge(
            &local_content,
            &remote_content,
            base_content.as_deref(),
            &formatting_options,
            &user_data_sync_util_service,
        )
        .await
        .expect("merge");
        *out.borrow_mut() = Some(result);
    });
    slot.borrow_mut().take().expect("o merge terminou")
}

// KeybindingsMerge - No Conflicts

#[test]
fn merge_when_local_and_remote_are_same_with_one_entry() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+c", "a", WHEN)]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN)]);
    let actual = merge_keybindings(&local_content, &remote_content, None);
    assert!(!actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, local_content);
}

#[test]
fn merge_when_local_and_remote_are_same_with_similar_when_contexts() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+c", "a", "editorTextFocus && !editorReadonly")]);
    let remote_content = stringify(&[kw("alt+c", "a", "!editorReadonly && editorTextFocus")]);
    let actual = merge_keybindings(&local_content, &remote_content, None);
    assert!(!actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, local_content);
}

#[test]
fn merge_when_local_and_remote_has_entries_in_different_order() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+d", "a", WHEN), kw("alt+a", "a", "editorTextFocus")]);
    let remote_content = stringify(&[kw("alt+a", "a", "editorTextFocus"), kw("alt+d", "a", WHEN)]);
    let actual = merge_keybindings(&local_content, &remote_content, None);
    assert!(!actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, local_content);
}

#[test]
fn merge_when_local_and_remote_are_same_with_multiple_entries() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a"), ka("cmd+c", "b", "`")]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a"), ka("cmd+c", "b", "`")]);
    let actual = merge_keybindings(&local_content, &remote_content, None);
    assert!(!actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, local_content);
}

#[test]
fn merge_when_local_and_remote_are_same_with_different_base_content() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a"), ka("cmd+c", "b", "`")]);
    let base_content = stringify(&[k("ctrl+c", "e"), ka("shift+d", "d", "`")]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a"), ka("cmd+c", "b", "`")]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&base_content));
    assert!(!actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, local_content);
}

#[test]
fn merge_when_local_and_remote_are_same_with_multiple_entries_in_different_order() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a"), ka("cmd+c", "b", "`")]);
    let remote_content = stringify(&[ka("cmd+c", "b", "`"), kw("alt+c", "a", WHEN), k("alt+d", "-a")]);
    let actual = merge_keybindings(&local_content, &remote_content, None);
    assert!(!actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, local_content);
}

#[test]
fn merge_when_local_and_remote_are_same_when_remove_entry_is_in_different_order() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a"), ka("cmd+c", "b", "`")]);
    let remote_content = stringify(&[k("alt+d", "-a"), kw("alt+c", "a", WHEN), ka("cmd+c", "b", "`")]);
    let actual = merge_keybindings(&local_content, &remote_content, None);
    assert!(!actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, local_content);
}

#[test]
fn merge_when_a_new_entry_is_added_to_remote() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a")]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a"), ka("cmd+c", "b", "`")]);
    let actual = merge_keybindings(&local_content, &remote_content, None);
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, remote_content);
}

#[test]
fn merge_when_multiple_new_entries_are_added_to_remote() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a")]);
    let remote_content =
        stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a"), ka("cmd+c", "b", "`"), k("cmd+d", "c")]);
    let actual = merge_keybindings(&local_content, &remote_content, None);
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, remote_content);
}

#[test]
fn merge_when_multiple_new_entries_are_added_to_remote_from_base_and_local_has_not_changed() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a")]);
    let remote_content =
        stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a"), ka("cmd+c", "b", "`"), k("cmd+d", "c")]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&local_content));
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, remote_content);
}

#[test]
fn merge_when_an_entry_is_removed_from_remote_from_base_and_local_has_not_changed() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a"), ka("cmd+c", "b", "`")]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a")]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&local_content));
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, remote_content);
}

#[test]
fn merge_when_an_entry_same_command_is_removed_from_remote_from_base_and_local_has_not_changed() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a")]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN)]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&local_content));
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, remote_content);
}

#[test]
fn merge_when_an_entry_is_updated_in_remote_from_base_and_local_has_not_changed() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+d", "a", WHEN)]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN)]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&local_content));
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, remote_content);
}

#[test]
fn merge_when_a_command_with_multiple_entries_is_updated_from_remote_from_base_and_local_has_not_changed() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[k("shift+c", "c"), kw("alt+c", "a", WHEN), k("alt+d", "b"), k("cmd+c", "a")]);
    let remote_content = stringify(&[k("shift+c", "c"), kw("alt+c", "a", WHEN), k("alt+d", "b"), k("cmd+d", "a")]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&local_content));
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, remote_content);
}

#[test]
fn merge_when_remote_has_moved_forwareded_with_multiple_changes_and_local_stays_with_base() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[
        kw("alt+c", "a", WHEN),
        ka("cmd+c", "b", "`"),
        k("alt+d", "-a"),
        k("cmd+e", "d"),
        kw("cmd+d", "c", "context1"),
    ]);
    let remote_content = stringify(&[
        kw("alt+d", "a", WHEN),
        k("cmd+e", "d"),
        k("alt+d", "-a"),
        k("alt+f", "f"),
        k("alt+d", "-f"),
        kw("cmd+d", "c", "context1"),
        k("cmd+c", "-c"),
    ]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&local_content));
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, remote_content);
}

#[test]
fn merge_when_a_new_entry_is_added_to_local() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a"), ka("cmd+c", "b", "`")]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a")]);
    let actual = merge_keybindings(&local_content, &remote_content, None);
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, local_content);
}

#[test]
fn merge_when_multiple_new_entries_are_added_to_local() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content =
        stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a"), ka("cmd+c", "b", "`"), k("cmd+d", "c")]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a")]);
    let actual = merge_keybindings(&local_content, &remote_content, None);
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, local_content);
}

#[test]
fn merge_when_multiple_new_entries_are_added_to_local_from_base_and_remote_is_not_changed() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content =
        stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a"), ka("cmd+c", "b", "`"), k("cmd+d", "c")]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a")]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&remote_content));
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, local_content);
}

#[test]
fn merge_when_an_entry_is_removed_from_local_from_base_and_remote_has_not_changed() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a")]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a"), ka("cmd+c", "b", "`")]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&remote_content));
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, local_content);
}

#[test]
fn merge_when_an_entry_with_same_command_is_removed_from_local_from_base_and_remote_has_not_changed() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+c", "a", WHEN)]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN), k("alt+d", "-a")]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&remote_content));
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, local_content);
}

#[test]
fn merge_when_an_entry_is_updated_in_local_from_base_and_remote_has_not_changed() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+d", "a", "editorTextFocus")]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN)]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&remote_content));
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, local_content);
}

#[test]
fn merge_when_a_command_with_multiple_entries_is_updated_from_local_from_base_and_remote_has_not_changed() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[k("shift+c", "c"), kw("alt+c", "a", WHEN), k("alt+d", "b"), k("cmd+c", "a")]);
    let remote_content = stringify(&[k("shift+c", "c"), kw("alt+c", "a", WHEN), k("alt+d", "b"), k("cmd+d", "a")]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&remote_content));
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, local_content);
}

#[test]
fn merge_when_local_has_moved_forwareded_with_multiple_changes_and_remote_stays_with_base() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[
        kw("alt+d", "a", WHEN),
        k("cmd+e", "d"),
        k("alt+d", "-a"),
        k("alt+f", "f"),
        k("alt+d", "-f"),
        kw("cmd+d", "c", "context1"),
        k("cmd+c", "-c"),
    ]);
    let remote_content = stringify(&[
        kw("alt+c", "a", WHEN),
        ka("cmd+c", "b", "`"),
        k("alt+d", "-a"),
        k("cmd+e", "d"),
        kw("cmd+d", "c", "context1"),
    ]);
    let expected = stringify(&[
        kw("alt+d", "a", WHEN),
        k("cmd+e", "d"),
        k("alt+d", "-a"),
        k("alt+f", "f"),
        k("alt+d", "-f"),
        kw("cmd+d", "c", "context1"),
        k("cmd+c", "-c"),
    ]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&remote_content));
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, expected);
}

#[test]
fn merge_when_local_and_remote_has_moved_forwareded_with_conflicts() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let base_content = stringify(&[
        kw("alt+d", "a", WHEN),
        k("ctrl+c", "-a"),
        k("cmd+e", "d"),
        k("alt+a", "f"),
        k("alt+d", "-f"),
        kw("cmd+d", "c", "context1"),
        k("cmd+c", "-c"),
    ]);
    let local_content = stringify(&[
        k("alt+d", "-f"),
        k("cmd+e", "d"),
        k("cmd+c", "-c"),
        kw("cmd+d", "c", "context1"),
        k("alt+a", "f"),
        k("alt+e", "e"),
    ]);
    let remote_content = stringify(&[
        k("alt+a", "f"),
        k("cmd+c", "-c"),
        k("cmd+d", "d"),
        k("alt+d", "-f"),
        kw("alt+c", "c", "context1"),
        kw("alt+g", "g", "context2"),
    ]);
    let expected = stringify(&[
        k("alt+d", "-f"),
        k("cmd+d", "d"),
        k("cmd+c", "-c"),
        kw("alt+c", "c", "context1"),
        k("alt+a", "f"),
        k("alt+e", "e"),
        kw("alt+g", "g", "context2"),
    ]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&base_content));
    assert!(actual.has_changes);
    assert!(!actual.has_conflicts);
    assert_eq!(actual.merge_content, expected);
}

#[test]
fn merge_when_local_and_remote_with_one_entry_but_different_value() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+d", "a", WHEN)]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN)]);
    let actual = merge_keybindings(&local_content, &remote_content, None);
    assert!(actual.has_changes);
    assert!(actual.has_conflicts);
    assert_eq!(
        actual.merge_content,
        text(&[
            "[",
            "\t{",
            "\t\t\"key\": \"alt+d\",",
            "\t\t\"command\": \"a\",",
            "\t\t\"when\": \"editorTextFocus && !editorReadonly\"",
            "\t}",
            "]",
        ])
    );
}

#[test]
fn merge_when_local_and_remote_with_different_keybinding() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let local_content = stringify(&[kw("alt+d", "a", WHEN), kw("alt+a", "-a", WHEN)]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN), kw("alt+a", "-a", WHEN)]);
    let actual = merge_keybindings(&local_content, &remote_content, None);
    assert!(actual.has_changes);
    assert!(actual.has_conflicts);
    assert_eq!(
        actual.merge_content,
        text(&[
            "[",
            "\t{",
            "\t\t\"key\": \"alt+d\",",
            "\t\t\"command\": \"a\",",
            "\t\t\"when\": \"editorTextFocus && !editorReadonly\"",
            "\t},",
            "\t{",
            "\t\t\"key\": \"alt+a\",",
            "\t\t\"command\": \"-a\",",
            "\t\t\"when\": \"editorTextFocus && !editorReadonly\"",
            "\t}",
            "]",
        ])
    );
}

#[test]
fn merge_when_the_entry_is_removed_in_local_but_updated_in_remote() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let base_content = stringify(&[kw("alt+d", "a", WHEN)]);
    let local_content = stringify(&[]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN)]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&base_content));
    assert!(actual.has_changes);
    assert!(actual.has_conflicts);
    assert_eq!(actual.merge_content, "[]");
}

#[test]
fn merge_when_the_entry_is_removed_in_local_but_updated_in_remote_and_a_new_entry_is_added_in_local() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let base_content = stringify(&[kw("alt+d", "a", WHEN)]);
    let local_content = stringify(&[k("alt+b", "b")]);
    let remote_content = stringify(&[kw("alt+c", "a", WHEN)]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&base_content));
    assert!(actual.has_changes);
    assert!(actual.has_conflicts);
    assert_eq!(
        actual.merge_content,
        text(&["[", "\t{", "\t\t\"key\": \"alt+b\",", "\t\t\"command\": \"b\"", "\t}", "]"])
    );
}

#[test]
fn merge_when_the_entry_is_removed_in_remote_but_updated_in_local() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let base_content = stringify(&[kw("alt+d", "a", WHEN)]);
    let local_content = stringify(&[kw("alt+c", "a", WHEN)]);
    let remote_content = stringify(&[]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&base_content));
    assert!(actual.has_changes);
    assert!(actual.has_conflicts);
    assert_eq!(
        actual.merge_content,
        text(&[
            "[",
            "\t{",
            "\t\t\"key\": \"alt+c\",",
            "\t\t\"command\": \"a\",",
            "\t\t\"when\": \"editorTextFocus && !editorReadonly\"",
            "\t}",
            "]",
        ])
    );
}

#[test]
fn merge_when_the_entry_is_removed_in_remote_but_updated_in_local_and_a_new_entry_is_added_in_remote() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let base_content = stringify(&[kw("alt+d", "a", WHEN)]);
    let local_content = stringify(&[kw("alt+c", "a", WHEN)]);
    let remote_content = stringify(&[k("alt+b", "b")]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&base_content));
    assert!(actual.has_changes);
    assert!(actual.has_conflicts);
    assert_eq!(
        actual.merge_content,
        text(&[
            "[",
            "\t{",
            "\t\t\"key\": \"alt+c\",",
            "\t\t\"command\": \"a\",",
            "\t\t\"when\": \"editorTextFocus && !editorReadonly\"",
            "\t},",
            "\t{",
            "\t\t\"key\": \"alt+b\",",
            "\t\t\"command\": \"b\"",
            "\t}",
            "]",
        ])
    );
}

#[test]
fn merge_when_local_and_remote_has_moved_forwareded_with_conflicts_2() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let base_content = stringify(&[
        kw("alt+d", "a", WHEN),
        k("alt+c", "-a"),
        k("cmd+e", "d"),
        k("alt+a", "f"),
        k("alt+d", "-f"),
        kw("cmd+d", "c", "context1"),
        k("cmd+c", "-c"),
    ]);
    let local_content = stringify(&[
        k("alt+d", "-f"),
        k("cmd+e", "d"),
        k("cmd+c", "-c"),
        kw("cmd+d", "c", "context1"),
        k("alt+a", "f"),
        k("alt+e", "e"),
    ]);
    let remote_content = stringify(&[
        k("alt+a", "f"),
        k("cmd+c", "-c"),
        k("cmd+d", "d"),
        k("alt+d", "-f"),
        kw("alt+c", "c", "context1"),
        kw("alt+g", "g", "context2"),
    ]);
    let actual = merge_keybindings(&local_content, &remote_content, Some(&base_content));
    assert!(actual.has_changes);
    assert!(actual.has_conflicts);
    assert_eq!(
        actual.merge_content,
        text(&[
            "[",
            "\t{",
            "\t\t\"key\": \"alt+d\",",
            "\t\t\"command\": \"-f\"",
            "\t},",
            "\t{",
            "\t\t\"key\": \"cmd+d\",",
            "\t\t\"command\": \"d\"",
            "\t},",
            "\t{",
            "\t\t\"key\": \"cmd+c\",",
            "\t\t\"command\": \"-c\"",
            "\t},",
            "\t{",
            "\t\t\"key\": \"cmd+d\",",
            "\t\t\"command\": \"c\",",
            "\t\t\"when\": \"context1\"",
            "\t},",
            "\t{",
            "\t\t\"key\": \"alt+a\",",
            "\t\t\"command\": \"f\"",
            "\t},",
            "\t{",
            "\t\t\"key\": \"alt+e\",",
            "\t\t\"command\": \"e\"",
            "\t},",
            "\t{",
            "\t\t\"key\": \"alt+g\",",
            "\t\t\"command\": \"g\",",
            "\t\t\"when\": \"context2\"",
            "\t}",
            "]",
        ])
    );
}
