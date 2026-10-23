//! Porte de `src/vs/platform/userDataSync/test/common/settingsMerge.test.ts` (71 casos, na ordem do original).
//!
//! `stringify` do original é `JSON.stringify(value, null, '\t')`. Os casos que o chamam como template tag
//! (`stringify\`...\``) passam o array de textos do template, então o resultado é uma lista JSON com um texto;
//! [`stringify_template`] reproduz isso. O `ensureNoDisposablesAreLeakedInTestSuite` não tem o que vigiar aqui:
//! nada do merge cria descartável.

use je_base::JsString;
use je_base::json::{Object, Value};
use je_base::json_formatter::{FormattingOptions, json_stringify};
use je_platform::user_data_sync::settings_merge::{IMergeResult, ResolvedConflict, add_setting, merge, update_ignored_settings};
use je_platform::user_data_sync::user_data_sync::IConflictSetting;

fn formatting_options() -> FormattingOptions {
    FormattingOptions { eol: Some(JsString::from("\n")), insert_spaces: Some(false), tab_size: Some(4) }
}

fn keys(items: &[&str]) -> Vec<JsString> {
    items.iter().map(|item| JsString::from(*item)).collect()
}

/// `JSON.stringify({ k: v, ... }, null, '\t')`.
fn stringify(entries: &[(&str, f64)]) -> String {
    let mut object = Object::new();
    for (key, value) in entries {
        object.insert(JsString::from(*key), Value::Number(*value));
    }
    json_stringify(&Value::Object(object), Some("\t"))
}

/// `stringify` usado como template tag: `JSON.stringify([text], null, '\t')`.
fn stringify_template(text: &str) -> String {
    json_stringify(&Value::Array(vec![Value::String(JsString::from(text))]), Some("\t"))
}

/// Um texto escrito por linhas (os literais com tabulação do original).
fn text(lines: &[&str]) -> String {
    lines.join("\n")
}

fn merge_settings(
    local_content: &str,
    remote_content: &str,
    base_content: Option<&str>,
    ignored_settings: &[&str],
    resolved_conflicts: &[(&str, Option<f64>)],
) -> IMergeResult {
    let resolved: Vec<ResolvedConflict> = resolved_conflicts
        .iter()
        .map(|(key, value)| ResolvedConflict { key: JsString::from(*key), value: value.map(Value::Number) })
        .collect();
    merge(local_content, remote_content, base_content, &keys(ignored_settings), &resolved, &formatting_options())
}

/// `{ key, localValue, remoteValue }`.
fn conflict(key: &str, local_value: Option<f64>, remote_value: Option<f64>) -> IConflictSetting {
    IConflictSetting {
        key: JsString::from(key),
        local_value: local_value.map(Value::Number),
        remote_value: remote_value.map(Value::Number),
    }
}

fn assert_contents(actual: &IMergeResult, local_content: Option<&str>, remote_content: Option<&str>) {
    assert_eq!(actual.local_content.as_deref(), local_content);
    assert_eq!(actual.remote_content.as_deref(), remote_content);
}

fn add(key: &str, source_content: &str, target_content: &str) -> String {
    add_setting(key, source_content, target_content, &formatting_options())
}

// SettingsMerge - Merge

#[test]
fn merge_when_local_and_remote_are_same_with_one_entry() {
    let local_content = stringify(&[("a", 1.0)]);
    let remote_content = stringify(&[("a", 1.0)]);
    let actual = merge_settings(&local_content, &remote_content, None, &[], &[]);
    assert_contents(&actual, None, None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_local_and_remote_are_same_with_multiple_entries() {
    let local_content = stringify(&[("a", 1.0), ("b", 2.0)]);
    let remote_content = stringify(&[("a", 1.0), ("b", 2.0)]);
    let actual = merge_settings(&local_content, &remote_content, None, &[], &[]);
    assert_contents(&actual, None, None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_local_and_remote_are_same_with_multiple_entries_in_different_order() {
    let local_content = stringify(&[("b", 2.0), ("a", 1.0)]);
    let remote_content = stringify(&[("a", 1.0), ("b", 2.0)]);
    let actual = merge_settings(&local_content, &remote_content, None, &[], &[]);
    assert_contents(&actual, Some(&local_content), Some(&remote_content));
    assert!(actual.has_conflicts);
    assert_eq!(actual.conflicts_settings.len(), 0);
}

#[test]
fn merge_when_local_and_remote_are_same_with_different_base_content() {
    let local_content = stringify(&[("b", 2.0), ("a", 1.0)]);
    let base_content = stringify(&[("a", 2.0), ("b", 1.0)]);
    let remote_content = stringify(&[("a", 1.0), ("b", 2.0)]);
    let actual = merge_settings(&local_content, &remote_content, Some(&base_content), &[], &[]);
    assert_contents(&actual, Some(&local_content), Some(&remote_content));
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(actual.has_conflicts);
}

#[test]
fn merge_when_a_new_entry_is_added_to_remote() {
    let local_content = stringify(&[("a", 1.0)]);
    let remote_content = stringify(&[("a", 1.0), ("b", 2.0)]);
    let actual = merge_settings(&local_content, &remote_content, None, &[], &[]);
    assert_contents(&actual, Some(&remote_content), None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_multiple_new_entries_are_added_to_remote() {
    let local_content = stringify(&[("a", 1.0)]);
    let remote_content = stringify(&[("a", 1.0), ("b", 2.0), ("c", 3.0)]);
    let actual = merge_settings(&local_content, &remote_content, None, &[], &[]);
    assert_contents(&actual, Some(&remote_content), None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_multiple_new_entries_are_added_to_remote_from_base_and_local_has_not_changed() {
    let local_content = stringify(&[("a", 1.0)]);
    let remote_content = stringify(&[("b", 2.0), ("a", 1.0), ("c", 3.0)]);
    let actual = merge_settings(&local_content, &remote_content, Some(&local_content), &[], &[]);
    assert_contents(&actual, Some(&remote_content), None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_an_entry_is_removed_from_remote_from_base_and_local_has_not_changed() {
    let local_content = stringify(&[("a", 1.0), ("b", 2.0)]);
    let remote_content = stringify(&[("a", 1.0)]);
    let actual = merge_settings(&local_content, &remote_content, Some(&local_content), &[], &[]);
    assert_contents(&actual, Some(&remote_content), None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_all_entries_are_removed_from_base_and_local_has_not_changed() {
    let local_content = stringify(&[("a", 1.0)]);
    let remote_content = stringify(&[]);
    let actual = merge_settings(&local_content, &remote_content, Some(&local_content), &[], &[]);
    assert_contents(&actual, Some(&remote_content), None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_an_entry_is_updated_in_remote_from_base_and_local_has_not_changed() {
    let local_content = stringify(&[("a", 1.0)]);
    let remote_content = stringify(&[("a", 2.0)]);
    let actual = merge_settings(&local_content, &remote_content, Some(&local_content), &[], &[]);
    assert_contents(&actual, Some(&remote_content), None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_remote_has_moved_forwareded_with_multiple_changes_and_local_stays_with_base() {
    let local_content = stringify(&[("a", 1.0)]);
    let remote_content = stringify(&[("a", 2.0), ("b", 1.0), ("c", 3.0), ("d", 4.0)]);
    let actual = merge_settings(&local_content, &remote_content, Some(&local_content), &[], &[]);
    assert_contents(&actual, Some(&remote_content), None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_remote_has_moved_forwareded_with_order_changes_and_local_stays_with_base() {
    let local_content = stringify(&[("a", 1.0), ("b", 2.0), ("c", 3.0)]);
    let remote_content = stringify(&[("a", 2.0), ("d", 4.0), ("c", 3.0), ("b", 2.0)]);
    let actual = merge_settings(&local_content, &remote_content, Some(&local_content), &[], &[]);
    assert_contents(&actual, Some(&remote_content), None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_remote_has_moved_forwareded_with_comment_changes_and_local_stays_with_base() {
    let local_content = text(&["", "{", "\t// this is comment for b", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 1,", "}"]);
    let remote_content = stringify_template(&text(&["", "{", "\t// comment b has changed", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 1,", "}"]));
    let actual = merge_settings(&local_content, &remote_content, Some(&local_content), &[], &[]);
    assert_contents(&actual, Some(&remote_content), None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_remote_has_moved_forwareded_with_comment_and_order_changes_and_local_stays_with_base() {
    let local_content = text(&["", "{", "\t// this is comment for b", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 1,", "}"]);
    let remote_content = stringify_template(&text(&["", "{", "\t// this is comment for c", "\t\"c\": 1,", "\t// comment b has changed", "\t\"b\": 2,", "}"]));
    let actual = merge_settings(&local_content, &remote_content, Some(&local_content), &[], &[]);
    assert_contents(&actual, Some(&remote_content), None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_a_new_entries_are_added_to_local() {
    let local_content = stringify(&[("a", 1.0), ("b", 2.0), ("c", 3.0), ("d", 4.0)]);
    let remote_content = stringify(&[("a", 1.0)]);
    let actual = merge_settings(&local_content, &remote_content, None, &[], &[]);
    assert_contents(&actual, None, Some(&local_content));
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_multiple_new_entries_are_added_to_local_from_base_and_remote_is_not_changed() {
    let local_content = stringify(&[("a", 2.0), ("b", 1.0), ("c", 3.0), ("d", 4.0)]);
    let remote_content = stringify(&[("a", 1.0)]);
    let actual = merge_settings(&local_content, &remote_content, Some(&remote_content), &[], &[]);
    assert_contents(&actual, None, Some(&local_content));
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_an_entry_is_removed_from_local_from_base_and_remote_has_not_changed() {
    let local_content = stringify(&[("a", 1.0), ("c", 2.0)]);
    let remote_content = stringify(&[("a", 2.0), ("b", 1.0), ("c", 3.0), ("d", 4.0)]);
    let actual = merge_settings(&local_content, &remote_content, Some(&remote_content), &[], &[]);
    assert_contents(&actual, None, Some(&local_content));
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_an_entry_is_updated_in_local_from_base_and_remote_has_not_changed() {
    let local_content = stringify(&[("a", 1.0), ("c", 2.0)]);
    let remote_content = stringify(&[("a", 2.0), ("c", 2.0)]);
    let actual = merge_settings(&local_content, &remote_content, Some(&remote_content), &[], &[]);
    assert_contents(&actual, None, Some(&local_content));
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_local_has_moved_forwarded_with_multiple_changes_and_remote_stays_with_base() {
    let local_content = stringify(&[("a", 2.0), ("b", 1.0), ("c", 3.0), ("d", 4.0)]);
    let remote_content = stringify(&[("a", 1.0)]);
    let actual = merge_settings(&local_content, &remote_content, Some(&remote_content), &[], &[]);
    assert_contents(&actual, None, Some(&local_content));
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_local_has_moved_forwarded_with_order_changes_and_remote_stays_with_base() {
    let local_content = text(&["", "{", "\t\"b\": 2,", "\t\"c\": 1,", "}"]);
    let remote_content = stringify_template(&text(&["", "{", "\t\"c\": 1,", "\t\"b\": 2,", "}"]));
    let actual = merge_settings(&local_content, &remote_content, Some(&remote_content), &[], &[]);
    assert_contents(&actual, None, Some(&local_content));
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_local_has_moved_forwarded_with_comment_changes_and_remote_stays_with_base() {
    let local_content = text(&["", "{", "\t// comment for b has changed", "\t\"b\": 2,", "\t// comment for c", "\t\"c\": 1,", "}"]);
    let remote_content = stringify_template(&text(&["", "{", "\t// comment for b", "\t\"b\": 2,", "\t// comment for c", "\t\"c\": 1,", "}"]));
    let actual = merge_settings(&local_content, &remote_content, Some(&remote_content), &[], &[]);
    assert_contents(&actual, None, Some(&local_content));
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_local_has_moved_forwarded_with_comment_and_order_changes_and_remote_stays_with_base() {
    let local_content = text(&["", "{", "\t// comment for c", "\t\"c\": 1,", "\t// comment for b has changed", "\t\"b\": 2,", "}"]);
    let remote_content = stringify_template(&text(&["", "{", "\t// comment for b", "\t\"b\": 2,", "\t// comment for c", "\t\"c\": 1,", "}"]));
    let actual = merge_settings(&local_content, &remote_content, Some(&remote_content), &[], &[]);
    assert_contents(&actual, None, Some(&local_content));
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn merge_when_local_and_remote_with_one_entry_but_different_value() {
    let local_content = stringify(&[("a", 1.0)]);
    let remote_content = stringify(&[("a", 2.0)]);
    let expected_conflicts = vec![conflict("a", Some(1.0), Some(2.0))];
    let actual = merge_settings(&local_content, &remote_content, None, &[], &[]);
    assert_contents(&actual, Some(&local_content), Some(&remote_content));
    assert!(actual.has_conflicts);
    assert_eq!(actual.conflicts_settings, expected_conflicts);
}

#[test]
fn merge_when_the_entry_is_removed_in_remote_but_updated_in_local_and_a_new_entry_is_added_in_remote() {
    let base_content = stringify(&[("a", 1.0)]);
    let local_content = stringify(&[("a", 2.0)]);
    let remote_content = stringify(&[("b", 2.0)]);
    let expected_conflicts = vec![conflict("a", Some(2.0), None)];
    let actual = merge_settings(&local_content, &remote_content, Some(&base_content), &[], &[]);
    assert_eq!(actual.local_content, Some(stringify(&[("a", 2.0), ("b", 2.0)])));
    assert_eq!(actual.remote_content.as_deref(), Some(remote_content.as_str()));
    assert!(actual.has_conflicts);
    assert_eq!(actual.conflicts_settings, expected_conflicts);
}

#[test]
fn merge_with_single_entry_and_local_is_empty() {
    let base_content = stringify(&[("a", 1.0)]);
    let local_content = stringify(&[]);
    let remote_content = stringify(&[("a", 2.0)]);
    let expected_conflicts = vec![conflict("a", None, Some(2.0))];
    let actual = merge_settings(&local_content, &remote_content, Some(&base_content), &[], &[]);
    assert_contents(&actual, Some(&local_content), Some(&remote_content));
    assert!(actual.has_conflicts);
    assert_eq!(actual.conflicts_settings, expected_conflicts);
}

#[test]
fn merge_when_local_and_remote_has_moved_forwareded_with_conflicts() {
    let base_content = stringify(&[("a", 1.0), ("b", 2.0), ("c", 3.0), ("d", 4.0)]);
    let local_content = stringify(&[("a", 2.0), ("c", 3.0), ("d", 5.0), ("e", 4.0), ("f", 1.0)]);
    let remote_content = stringify(&[("b", 3.0), ("c", 3.0), ("d", 6.0), ("e", 5.0)]);
    let expected_conflicts = vec![
        conflict("b", None, Some(3.0)),
        conflict("a", Some(2.0), None),
        conflict("d", Some(5.0), Some(6.0)),
        conflict("e", Some(4.0), Some(5.0)),
    ];
    let actual = merge_settings(&local_content, &remote_content, Some(&base_content), &[], &[]);
    assert_eq!(actual.local_content, Some(stringify(&[("a", 2.0), ("c", 3.0), ("d", 5.0), ("e", 4.0), ("f", 1.0)])));
    assert_eq!(actual.remote_content, Some(stringify(&[("b", 3.0), ("c", 3.0), ("d", 6.0), ("e", 5.0), ("f", 1.0)])));
    assert!(actual.has_conflicts);
    assert_eq!(actual.conflicts_settings, expected_conflicts);
}

#[test]
fn merge_when_local_and_remote_has_moved_forwareded_with_change_in_order() {
    let base_content = stringify(&[("a", 1.0), ("b", 2.0), ("c", 3.0), ("d", 4.0)]);
    let local_content = stringify(&[("a", 2.0), ("c", 3.0), ("b", 2.0), ("d", 4.0), ("e", 5.0)]);
    let remote_content = stringify(&[("a", 1.0), ("b", 2.0), ("c", 4.0)]);
    let actual = merge_settings(&local_content, &remote_content, Some(&base_content), &[], &[]);
    assert_eq!(actual.local_content, Some(stringify(&[("a", 2.0), ("c", 4.0), ("b", 2.0), ("e", 5.0)])));
    assert_eq!(actual.remote_content, Some(stringify(&[("a", 2.0), ("b", 2.0), ("e", 5.0), ("c", 4.0)])));
    assert!(actual.has_conflicts);
    assert_eq!(actual.conflicts_settings, Vec::<IConflictSetting>::new());
}

#[test]
fn merge_when_local_and_remote_has_moved_forwareded_with_comment_changes() {
    let base_content = text(&["", "{", "\t// this is comment for b", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 1", "}"]);
    let local_content = text(&["", "{", "\t// comment b has changed in local", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 1", "}"]);
    let remote_content = text(&["", "{", "\t// comment b has changed in remote", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 1", "}"]);
    let actual = merge_settings(&local_content, &remote_content, Some(&base_content), &[], &[]);
    assert_contents(&actual, Some(&local_content), Some(&remote_content));
    assert!(actual.has_conflicts);
    assert_eq!(actual.conflicts_settings, Vec::<IConflictSetting>::new());
}

#[test]
fn resolve_when_local_and_remote_has_moved_forwareded_with_resolved_conflicts() {
    let base_content = stringify(&[("a", 1.0), ("b", 2.0), ("c", 3.0), ("d", 4.0)]);
    let local_content = stringify(&[("a", 2.0), ("c", 3.0), ("d", 5.0), ("e", 4.0), ("f", 1.0)]);
    let remote_content = stringify(&[("b", 3.0), ("c", 3.0), ("d", 6.0), ("e", 5.0)]);
    let expected_conflicts = vec![conflict("d", Some(5.0), Some(6.0))];
    let actual = merge_settings(
        &local_content,
        &remote_content,
        Some(&base_content),
        &[],
        &[("a", Some(2.0)), ("b", None), ("e", Some(5.0))],
    );
    assert_eq!(actual.local_content, Some(stringify(&[("a", 2.0), ("c", 3.0), ("d", 5.0), ("e", 5.0), ("f", 1.0)])));
    assert_eq!(actual.remote_content, Some(stringify(&[("c", 3.0), ("d", 6.0), ("e", 5.0), ("f", 1.0), ("a", 2.0)])));
    assert!(actual.has_conflicts);
    assert_eq!(actual.conflicts_settings, expected_conflicts);
}

#[test]
fn ignored_setting_is_not_merged_when_changed_in_local_and_remote() {
    let local_content = stringify(&[("a", 1.0)]);
    let remote_content = stringify(&[("a", 2.0)]);
    let actual = merge_settings(&local_content, &remote_content, None, &["a"], &[]);
    assert_contents(&actual, None, None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn ignored_setting_is_not_merged_when_changed_in_local_and_remote_from_base() {
    let base_content = stringify(&[("a", 0.0)]);
    let local_content = stringify(&[("a", 1.0)]);
    let remote_content = stringify(&[("a", 2.0)]);
    let actual = merge_settings(&local_content, &remote_content, Some(&base_content), &["a"], &[]);
    assert_contents(&actual, None, None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn ignored_setting_is_not_merged_when_added_in_remote() {
    let local_content = stringify(&[]);
    let remote_content = stringify(&[("a", 1.0)]);
    let actual = merge_settings(&local_content, &remote_content, None, &["a"], &[]);
    assert_contents(&actual, None, None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn ignored_setting_is_not_merged_when_added_in_remote_from_base() {
    let local_content = stringify(&[("b", 2.0)]);
    let remote_content = stringify(&[("a", 1.0), ("b", 2.0)]);
    let actual = merge_settings(&local_content, &remote_content, Some(&local_content), &["a"], &[]);
    assert_contents(&actual, None, None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn ignored_setting_is_not_merged_when_removed_in_remote() {
    let local_content = stringify(&[("a", 1.0)]);
    let remote_content = stringify(&[]);
    let actual = merge_settings(&local_content, &remote_content, None, &["a"], &[]);
    assert_contents(&actual, None, None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn ignored_setting_is_not_merged_when_removed_in_remote_from_base() {
    let local_content = stringify(&[("a", 2.0)]);
    let remote_content = stringify(&[]);
    let actual = merge_settings(&local_content, &remote_content, Some(&local_content), &["a"], &[]);
    assert_contents(&actual, None, None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn ignored_setting_is_not_merged_with_other_changes_without_conflicts() {
    let base_content = stringify(&[("a", 2.0), ("b", 2.0), ("c", 3.0), ("d", 4.0), ("e", 5.0)]);
    let local_content = stringify(&[("a", 1.0), ("b", 2.0), ("c", 3.0)]);
    let remote_content = stringify(&[("a", 3.0), ("b", 3.0), ("d", 4.0), ("e", 6.0)]);
    let actual = merge_settings(&local_content, &remote_content, Some(&base_content), &["a", "e"], &[]);
    assert_eq!(actual.local_content, Some(stringify(&[("a", 1.0), ("b", 3.0)])));
    assert_eq!(actual.remote_content, Some(stringify(&[("a", 3.0), ("b", 3.0), ("e", 6.0)])));
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

#[test]
fn ignored_setting_is_not_merged_with_other_changes_conflicts() {
    let base_content = stringify(&[("a", 2.0), ("b", 2.0), ("c", 3.0), ("d", 4.0), ("e", 5.0)]);
    let local_content = stringify(&[("a", 1.0), ("b", 4.0), ("c", 3.0), ("d", 5.0)]);
    let remote_content = stringify(&[("a", 3.0), ("b", 3.0), ("e", 6.0)]);
    let expected_conflicts = vec![conflict("d", Some(5.0), None), conflict("b", Some(4.0), Some(3.0))];
    let actual = merge_settings(&local_content, &remote_content, Some(&base_content), &["a", "e"], &[]);
    assert_eq!(actual.local_content, Some(stringify(&[("a", 1.0), ("b", 4.0), ("d", 5.0)])));
    assert_eq!(actual.remote_content, Some(stringify(&[("a", 3.0), ("b", 3.0), ("e", 6.0)])));
    assert_eq!(actual.conflicts_settings, expected_conflicts);
    assert!(actual.has_conflicts);
}

#[test]
fn merge_when_remote_has_comments_and_local_is_empty() {
    let local_content = text(&["", "{", "", "}"]);
    let remote_content = stringify_template(&text(&["", "{", "\t// this is a comment", "\t\"a\": 1,", "}"]));
    let actual = merge_settings(&local_content, &remote_content, None, &[], &[]);
    assert_contents(&actual, Some(&remote_content), None);
    assert_eq!(actual.conflicts_settings.len(), 0);
    assert!(!actual.has_conflicts);
}

// SettingsMerge - Compute Remote Content

#[test]
fn local_content_is_returned_when_there_are_no_ignored_settings() {
    let local_content = stringify(&[("a", 1.0), ("b", 2.0), ("c", 3.0)]);
    let remote_content = stringify(&[("a", 3.0), ("b", 3.0), ("d", 4.0), ("e", 6.0)]);
    let actual = update_ignored_settings(&local_content, &remote_content, &[], &formatting_options());
    assert_eq!(actual, local_content);
}

#[test]
fn when_target_content_is_empty() {
    let remote_content = stringify(&[("a", 3.0)]);
    let actual = update_ignored_settings("", &remote_content, &keys(&["a"]), &formatting_options());
    assert_eq!(actual, "");
}

#[test]
fn when_source_content_is_empty() {
    let local_content = stringify(&[("a", 3.0), ("b", 3.0)]);
    let expected = stringify(&[("b", 3.0)]);
    let actual = update_ignored_settings(&local_content, "", &keys(&["a"]), &formatting_options());
    assert_eq!(actual, expected);
}

#[test]
fn ignored_settings_are_not_updated_from_remote_content() {
    let local_content = stringify(&[("a", 1.0), ("b", 2.0), ("c", 3.0)]);
    let remote_content = stringify(&[("a", 3.0), ("b", 3.0), ("d", 4.0), ("e", 6.0)]);
    let expected = stringify(&[("a", 3.0), ("b", 2.0), ("c", 3.0)]);
    let actual = update_ignored_settings(&local_content, &remote_content, &keys(&["a"]), &formatting_options());
    assert_eq!(actual, expected);
}

// SettingsMerge - Add Setting

#[test]
fn insert_after_a_setting_without_comments() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    let target_content = text(&["", "{", "\t\"a\": 2,", "\t\"d\": 3", "}"]);
    let expected = text(&["", "{", "\t\"a\": 2,", "\t\"b\": 2,", "\t\"d\": 3", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_after_a_setting_without_comments_at_the_end() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    let target_content = text(&["", "{", "\t\"a\": 2", "}"]);
    let expected = text(&["", "{", "\t\"a\": 2,", "\t\"b\": 2", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_between_settings_without_comment() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    let target_content = text(&["", "{", "\t\"a\": 1,", "\t\"c\": 3", "}"]);
    let expected = text(&["", "{", "\t\"a\": 1,", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_between_settings_and_there_is_a_comment_in_between_in_source() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    let target_content = text(&["", "{", "\t\"a\": 1,", "\t\"c\": 3", "}"]);
    let expected = text(&["", "{", "\t\"a\": 1,", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_after_a_setting_and_after_a_comment_at_the_end() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t\"b\": 2", "}"]);
    let target_content = text(&["", "{", "\t\"a\": 1", "\t// this is comment for b", "}"]);
    let expected = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t\"b\": 2", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_after_a_setting_ending_with_comma_and_after_a_comment_at_the_end() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t\"b\": 2", "}"]);
    let target_content = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "}"]);
    let expected = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t\"b\": 2", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_after_a_comment_and_there_are_no_settings() {
    let source_content = text(&["", "{", "\t// this is comment for b", "\t\"b\": 2", "}"]);
    let target_content = text(&["", "{", "\t// this is comment for b", "}"]);
    let expected = text(&["", "{", "\t// this is comment for b", "\t\"b\": 2", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_after_a_setting_and_between_a_comment_and_setting() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    let target_content = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t\"c\": 3", "}"]);
    let expected = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_after_a_setting_between_two_comments_and_there_is_a_setting_after() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 3", "}"]);
    let target_content = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t// this is comment for c", "\t\"c\": 3", "}"]);
    let expected = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 3", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_after_a_setting_between_two_comments_on_the_same_line_and_there_is_a_setting_after() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t/* this is comment for b */", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 3", "}"]);
    let target_content = text(&["", "{", "\t\"a\": 1,", "\t/* this is comment for b */ // this is comment for c", "\t\"c\": 3", "}"]);
    let expected = text(&["", "{", "\t\"a\": 1,", "\t/* this is comment for b */", "\t\"b\": 2, // this is comment for c", "\t\"c\": 3", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_after_a_setting_between_two_line_comments_on_the_same_line_and_there_is_a_setting_after() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t/* this is comment for b */", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 3", "}"]);
    let target_content = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b // this is comment for c", "\t\"c\": 3", "}"]);
    let expected = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b // this is comment for c", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_after_a_setting_between_two_comments_and_there_is_no_setting_after() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t\"b\": 2", "\t// this is a comment", "}"]);
    let target_content = text(&["", "{", "\t\"a\": 1", "\t// this is comment for b", "\t// this is a comment", "}"]);
    let expected = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t\"b\": 2", "\t// this is a comment", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_after_a_setting_with_comma_and_between_two_comments_and_there_is_no_setting_after() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t\"b\": 2", "\t// this is a comment", "}"]);
    let target_content = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t// this is a comment", "}"]);
    let expected = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t\"b\": 2", "\t// this is a comment", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_before_a_setting_without_comments() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    let target_content = text(&["", "{", "\t\"d\": 2,", "\t\"c\": 3", "}"]);
    let expected = text(&["", "{", "\t\"d\": 2,", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_before_a_setting_without_comments_at_the_end() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    let target_content = text(&["", "{", "\t\"c\": 3", "}"]);
    let expected = text(&["", "{", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_before_a_setting_with_comment() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 3", "}"]);
    let target_content = text(&["", "{", "\t// this is comment for c", "\t\"c\": 3", "}"]);
    let expected = text(&["", "{", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 3", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_before_a_setting_and_before_a_comment_at_the_beginning() {
    let source_content = text(&["", "{", "\t// this is comment for b", "\t\"b\": 2,", "\t\"c\": 3,", "}"]);
    let target_content = text(&["", "{", "\t// this is comment for b", "\t\"c\": 3", "}"]);
    let expected = text(&["", "{", "\t// this is comment for b", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_before_a_setting_ending_with_comma_and_before_a_comment_at_the_begninning() {
    let source_content = text(&["", "{", "\t// this is comment for b", "\t\"b\": 2,", "\t\"c\": 3,", "}"]);
    let target_content = text(&["", "{", "\t// this is comment for b", "\t\"c\": 3,", "}"]);
    let expected = text(&["", "{", "\t// this is comment for b", "\t\"b\": 2,", "\t\"c\": 3,", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_before_a_setting_and_between_a_setting_and_comment() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    let target_content = text(&["", "{", "\t\"d\": 1,", "\t// this is comment for b", "\t\"c\": 3", "}"]);
    let expected = text(&["", "{", "\t\"d\": 1,", "\t// this is comment for b", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_before_a_setting_between_two_comments_and_there_is_a_setting_before() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t// this is comment for b", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 3", "}"]);
    let target_content = text(&["", "{", "\t\"d\": 1,", "\t// this is comment for b", "\t// this is comment for c", "\t\"c\": 3", "}"]);
    let expected = text(&["", "{", "\t\"d\": 1,", "\t// this is comment for b", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 3", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_before_a_setting_between_two_comments_on_the_same_line_and_there_is_a_setting_before() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t/* this is comment for b */", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 3", "}"]);
    let target_content = text(&["", "{", "\t\"d\": 1,", "\t/* this is comment for b */ // this is comment for c", "\t\"c\": 3", "}"]);
    let expected = text(&["", "{", "\t\"d\": 1,", "\t/* this is comment for b */", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 3", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_before_a_setting_between_two_line_comments_on_the_same_line_and_there_is_a_setting_before() {
    let source_content = text(&["", "{", "\t\"a\": 1,", "\t/* this is comment for b */", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 3", "}"]);
    let target_content = text(&["", "{", "\t\"d\": 1,", "\t// this is comment for b // this is comment for c", "\t\"c\": 3", "}"]);
    let expected = text(&["", "{", "\t\"d\": 1,", "\t\"b\": 2,", "\t// this is comment for b // this is comment for c", "\t\"c\": 3", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_before_a_setting_between_two_comments_and_there_is_no_setting_before() {
    let source_content = text(&["", "{", "\t// this is comment for b", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 1", "}"]);
    let target_content = text(&["", "{", "\t// this is comment for b", "\t// this is comment for c", "\t\"c\": 1", "}"]);
    let expected = text(&["", "{", "\t// this is comment for b", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 1", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_before_a_setting_with_comma_and_between_two_comments_and_there_is_no_setting_before() {
    let source_content = text(&["", "{", "\t// this is comment for b", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 1", "}"]);
    let target_content = text(&["", "{", "\t// this is comment for b", "\t// this is comment for c", "\t\"c\": 1,", "}"]);
    let expected = text(&["", "{", "\t// this is comment for b", "\t\"b\": 2,", "\t// this is comment for c", "\t\"c\": 1,", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_after_a_setting_that_is_of_object_type() {
    let source_content = text(&["", "{", "\t\"b\": {", "\t\t\"d\": 1", "\t},", "\t\"a\": 2,", "\t\"c\": 1", "}"]);
    let target_content = text(&["", "{", "\t\"b\": {", "\t\t\"d\": 1", "\t},", "\t\"c\": 1", "}"]);
    assert_eq!(add("a", &source_content, &target_content), source_content);
}

#[test]
fn insert_after_a_setting_that_is_of_array_type() {
    let source_content = text(&["", "{", "\t\"b\": [", "\t\t1", "\t],", "\t\"a\": 2,", "\t\"c\": 1", "}"]);
    let target_content = text(&["", "{", "\t\"b\": [", "\t\t1", "\t],", "\t\"c\": 1", "}"]);
    assert_eq!(add("a", &source_content, &target_content), source_content);
}

#[test]
fn insert_after_a_comment_with_comma_separator_of_previous_setting_and_no_next_nodes() {
    let source_content = text(&["", "{", "\t\"a\": 1", "\t// this is comment for a", "\t,", "\t\"b\": 2", "}"]);
    let target_content = text(&["", "{", "\t\"a\": 1", "\t// this is comment for a", "\t,", "}"]);
    let expected = text(&["", "{", "\t\"a\": 1", "\t// this is comment for a", "\t,", "\t\"b\": 2", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_after_a_comment_with_comma_separator_of_previous_setting_and_there_is_a_setting_after() {
    let source_content = text(&["", "{", "\t\"a\": 1", "\t// this is comment for a", "\t,", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    let target_content = text(&["", "{", "\t\"a\": 1", "\t// this is comment for a", "\t,", "\t\"c\": 3", "}"]);
    let expected = text(&["", "{", "\t\"a\": 1", "\t// this is comment for a", "\t,", "\t\"b\": 2,", "\t\"c\": 3", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}

#[test]
fn insert_after_a_comment_with_comma_separator_of_previous_setting_and_there_is_a_comment_after() {
    let source_content = text(&["", "{", "\t\"a\": 1", "\t// this is comment for a", "\t,", "\t\"b\": 2", "\t// this is a comment", "}"]);
    let target_content = text(&["", "{", "\t\"a\": 1", "\t// this is comment for a", "\t,", "\t// this is a comment", "}"]);
    let expected = text(&["", "{", "\t\"a\": 1", "\t// this is comment for a", "\t,", "\t\"b\": 2", "\t// this is a comment", "}"]);
    assert_eq!(add("b", &source_content, &target_content), expected);
}
