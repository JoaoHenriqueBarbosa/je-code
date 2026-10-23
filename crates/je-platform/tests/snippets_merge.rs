//! Porte de `src/vs/platform/userDataSync/test/common/snippetsMerge.test.ts` (21 casos, na ordem do original).

use je_base::JsString;
use je_platform::user_data_sync::snippets_merge::{Dictionary, MergeResult, merge};

const TS_SNIPPET1: &str = r#"{

	// Place your snippets for TypeScript here. Each snippet is defined under a snippet name and has a prefix, body and
	// description. The prefix is what is used to trigger the snippet and the body will be expanded and inserted. Possible variables are:
	// $1, $2 for tab stops, $0 for the final cursor position, Placeholders with the
	// same ids are connected.
	"Print to console": {
	// Example:
	"prefix": "log",
		"body": [
			"console.log('$1');",
			"$2"
		],
			"description": "Log output to console",
	}

}"#;

const TS_SNIPPET2: &str = r#"{

	// Place your snippets for TypeScript here. Each snippet is defined under a snippet name and has a prefix, body and
	// description. The prefix is what is used to trigger the snippet and the body will be expanded and inserted. Possible variables are:
	// $1, $2 for tab stops, $0 for the final cursor position, Placeholders with the
	// same ids are connected.
	"Print to console": {
	// Example:
	"prefix": "log",
		"body": [
			"console.log('$1');",
			"$2"
		],
			"description": "Log output to console always",
	}

}"#;

const HTML_SNIPPET1: &str = r#"{
/*
	// Place your snippets for HTML here. Each snippet is defined under a snippet name and has a prefix, body and
	// description. The prefix is what is used to trigger the snippet and the body will be expanded and inserted.
	// Example:
	"Print to console": {
	"prefix": "log",
		"body": [
			"console.log('$1');",
			"$2"
		],
			"description": "Log output to console"
	}
*/
"Div": {
	"prefix": "div",
		"body": [
			"<div>",
			"",
			"</div>"
		],
			"description": "New div"
	}
}"#;

const HTML_SNIPPET2: &str = r#"{
/*
	// Place your snippets for HTML here. Each snippet is defined under a snippet name and has a prefix, body and
	// description. The prefix is what is used to trigger the snippet and the body will be expanded and inserted.
	// Example:
	"Print to console": {
	"prefix": "log",
		"body": [
			"console.log('$1');",
			"$2"
		],
			"description": "Log output to console"
	}
*/
"Div": {
	"prefix": "div",
		"body": [
			"<div>",
			"",
			"</div>"
		],
			"description": "New div changed"
	}
}"#;

const C_SNIPPET: &str = r#"{
	// Place your snippets for c here. Each snippet is defined under a snippet name and has a prefix, body and
	// description. The prefix is what is used to trigger the snippet and the body will be expanded and inserted. Possible variables are:
	// $1, $2 for tab stops, $0 for the final cursor position.Placeholders with the
	// same ids are connected.
	// Example:
	"Print to console": {
	"prefix": "log",
		"body": [
			"console.log('$1');",
			"$2"
		],
			"description": "Log output to console"
	}
}"#;

/// Um objeto literal `{ 'nome.json': conteúdo, ... }`, na ordem dada.
fn dict(entries: &[(&str, &str)]) -> Dictionary {
    let mut result = Dictionary::new();
    for (key, value) in entries {
        result.insert(JsString::from(*key), JsString::from(*value));
    }
    result
}

/// O `assert.deepStrictEqual` entre objetos não olha a ordem das chaves.
fn assert_dict(actual: &Dictionary, expected: &[(&str, &str)]) {
    let mut actual: Vec<(String, String)> = actual.iter().map(|(k, v)| (k.to_string_lossy(), v.to_string_lossy())).collect();
    let mut expected: Vec<(String, String)> = expected.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
}

fn assert_keys(actual: &[JsString], expected: &[&str]) {
    let actual: Vec<String> = actual.iter().map(JsString::to_string_lossy).collect();
    assert_eq!(actual, expected);
}

/// As sete asserções de cada caso, na ordem do original.
#[allow(clippy::too_many_arguments)]
fn check(
    actual: &MergeResult,
    local_added: &[(&str, &str)],
    local_updated: &[(&str, &str)],
    local_removed: &[&str],
    conflicts: &[&str],
    remote_added: &[(&str, &str)],
    remote_updated: &[(&str, &str)],
    remote_removed: &[&str],
) {
    assert_dict(&actual.local.added, local_added);
    assert_dict(&actual.local.updated, local_updated);
    assert_keys(&actual.local.removed, local_removed);
    assert_keys(&actual.conflicts, conflicts);
    assert_dict(&actual.remote.added, remote_added);
    assert_dict(&actual.remote.updated, remote_updated);
    assert_keys(&actual.remote.removed, remote_removed);
}

#[test]
fn merge_when_local_and_remote_are_same_with_one_snippet() {
    let local = dict(&[("html.json", HTML_SNIPPET1)]);
    let remote = dict(&[("html.json", HTML_SNIPPET1)]);

    let actual = merge(&local, Some(&remote), None);

    check(&actual, &[], &[], &[], &[], &[], &[], &[]);
}

#[test]
fn merge_when_local_and_remote_are_same_with_multiple_entries() {
    let local = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);
    let remote = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);

    let actual = merge(&local, Some(&remote), None);

    check(&actual, &[], &[], &[], &[], &[], &[], &[]);
}

#[test]
fn merge_when_local_and_remote_are_same_with_multiple_entries_in_different_order() {
    let local = dict(&[("typescript.json", TS_SNIPPET1), ("html.json", HTML_SNIPPET1)]);
    let remote = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);

    let actual = merge(&local, Some(&remote), None);

    check(&actual, &[], &[], &[], &[], &[], &[], &[]);
}

#[test]
fn merge_when_local_and_remote_are_same_with_different_base_content() {
    let local = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);
    let remote = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);
    let base = dict(&[("html.json", HTML_SNIPPET2), ("typescript.json", TS_SNIPPET2)]);

    let actual = merge(&local, Some(&remote), Some(&base));

    check(&actual, &[], &[], &[], &[], &[], &[], &[]);
}

#[test]
fn merge_when_a_new_entry_is_added_to_remote() {
    let local = dict(&[("html.json", HTML_SNIPPET1)]);
    let remote = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);

    let actual = merge(&local, Some(&remote), None);

    check(&actual, &[("typescript.json", TS_SNIPPET1)], &[], &[], &[], &[], &[], &[]);
}

#[test]
fn merge_when_multiple_new_entries_are_added_to_remote() {
    let local = dict(&[]);
    let remote = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);

    let actual = merge(&local, Some(&remote), None);

    check(&actual, &[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)], &[], &[], &[], &[], &[], &[]);
}

#[test]
fn merge_when_new_entry_is_added_to_remote_from_base_and_local_has_not_changed() {
    let local = dict(&[("html.json", HTML_SNIPPET1)]);
    let remote = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);

    let actual = merge(&local, Some(&remote), Some(&local));

    check(&actual, &[("typescript.json", TS_SNIPPET1)], &[], &[], &[], &[], &[], &[]);
}

#[test]
fn merge_when_an_entry_is_removed_from_remote_from_base_and_local_has_not_changed() {
    let local = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);
    let remote = dict(&[("html.json", HTML_SNIPPET1)]);

    let actual = merge(&local, Some(&remote), Some(&local));

    check(&actual, &[], &[], &["typescript.json"], &[], &[], &[], &[]);
}

#[test]
fn merge_when_all_entries_are_removed_from_base_and_local_has_not_changed() {
    let local = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);
    let remote = dict(&[]);

    let actual = merge(&local, Some(&remote), Some(&local));

    check(&actual, &[], &[], &["html.json", "typescript.json"], &[], &[], &[], &[]);
}

#[test]
fn merge_when_an_entry_is_updated_in_remote_from_base_and_local_has_not_changed() {
    let local = dict(&[("html.json", HTML_SNIPPET1)]);
    let remote = dict(&[("html.json", HTML_SNIPPET2)]);

    let actual = merge(&local, Some(&remote), Some(&local));

    check(&actual, &[], &[("html.json", HTML_SNIPPET2)], &[], &[], &[], &[], &[]);
}

#[test]
fn merge_when_remote_has_moved_forwarded_with_multiple_changes_and_local_stays_with_base() {
    let local = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);
    let remote = dict(&[("html.json", HTML_SNIPPET2), ("c.json", C_SNIPPET)]);

    let actual = merge(&local, Some(&remote), Some(&local));

    check(&actual, &[("c.json", C_SNIPPET)], &[("html.json", HTML_SNIPPET2)], &["typescript.json"], &[], &[], &[], &[]);
}

#[test]
fn merge_when_a_new_entries_are_added_to_local() {
    let local = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1), ("c.json", C_SNIPPET)]);
    let remote = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);

    let actual = merge(&local, Some(&remote), None);

    check(&actual, &[], &[], &[], &[], &[("c.json", C_SNIPPET)], &[], &[]);
}

#[test]
fn merge_when_multiple_new_entries_are_added_to_local_from_base_and_remote_is_not_changed() {
    let local = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1), ("c.json", C_SNIPPET)]);
    let remote = dict(&[("typescript.json", TS_SNIPPET1)]);

    let actual = merge(&local, Some(&remote), Some(&remote));

    check(&actual, &[], &[], &[], &[], &[("html.json", HTML_SNIPPET1), ("c.json", C_SNIPPET)], &[], &[]);
}

#[test]
fn merge_when_an_entry_is_removed_from_local_from_base_and_remote_has_not_changed() {
    let local = dict(&[("html.json", HTML_SNIPPET1)]);
    let remote = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);

    let actual = merge(&local, Some(&remote), Some(&remote));

    check(&actual, &[], &[], &[], &[], &[], &[], &["typescript.json"]);
}

#[test]
fn merge_when_an_entry_is_updated_in_local_from_base_and_remote_has_not_changed() {
    let local = dict(&[("html.json", HTML_SNIPPET2), ("typescript.json", TS_SNIPPET1)]);
    let remote = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);

    let actual = merge(&local, Some(&remote), Some(&remote));

    check(&actual, &[], &[], &[], &[], &[], &[("html.json", HTML_SNIPPET2)], &[]);
}

#[test]
fn merge_when_local_has_moved_forwarded_with_multiple_changes_and_remote_stays_with_base() {
    let local = dict(&[("html.json", HTML_SNIPPET2), ("c.json", C_SNIPPET)]);
    let remote = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);

    let actual = merge(&local, Some(&remote), Some(&remote));

    check(&actual, &[], &[], &[], &[], &[("c.json", C_SNIPPET)], &[("html.json", HTML_SNIPPET2)], &["typescript.json"]);
}

#[test]
fn merge_when_local_and_remote_with_one_entry_but_different_value() {
    let local = dict(&[("html.json", HTML_SNIPPET1)]);
    let remote = dict(&[("html.json", HTML_SNIPPET2)]);

    let actual = merge(&local, Some(&remote), None);

    check(&actual, &[], &[], &[], &["html.json"], &[], &[], &[]);
}

#[test]
fn merge_when_the_entry_is_removed_in_remote_but_updated_in_local_and_a_new_entry_is_added_in_remote() {
    let base = dict(&[("html.json", HTML_SNIPPET1)]);
    let local = dict(&[("html.json", HTML_SNIPPET2)]);
    let remote = dict(&[("typescript.json", TS_SNIPPET1)]);

    let actual = merge(&local, Some(&remote), Some(&base));

    check(&actual, &[("typescript.json", TS_SNIPPET1)], &[], &[], &["html.json"], &[], &[], &[]);
}

#[test]
fn merge_with_single_entry_and_local_is_empty() {
    let base = dict(&[("html.json", HTML_SNIPPET1)]);
    let local = dict(&[]);
    let remote = dict(&[("html.json", HTML_SNIPPET2)]);

    let actual = merge(&local, Some(&remote), Some(&base));

    check(&actual, &[("html.json", HTML_SNIPPET2)], &[], &[], &[], &[], &[], &[]);
}

#[test]
fn merge_when_local_and_remote_has_moved_forwareded_with_conflicts() {
    let base = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);
    let local = dict(&[("html.json", HTML_SNIPPET2), ("c.json", C_SNIPPET)]);
    let remote = dict(&[("typescript.json", TS_SNIPPET2)]);

    let actual = merge(&local, Some(&remote), Some(&base));

    check(&actual, &[("typescript.json", TS_SNIPPET2)], &[], &[], &["html.json"], &[("c.json", C_SNIPPET)], &[], &[]);
}

#[test]
fn merge_when_local_and_remote_has_moved_forwareded_with_multiple_conflicts() {
    let base = dict(&[("html.json", HTML_SNIPPET1), ("typescript.json", TS_SNIPPET1)]);
    let local = dict(&[("html.json", HTML_SNIPPET2), ("typescript.json", TS_SNIPPET2), ("c.json", C_SNIPPET)]);
    let remote = dict(&[("c.json", C_SNIPPET)]);

    let actual = merge(&local, Some(&remote), Some(&base));

    check(&actual, &[], &[], &[], &["html.json", "typescript.json"], &[], &[], &[]);
}
