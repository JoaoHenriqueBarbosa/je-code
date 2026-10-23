//! Porte de `src/vs/editor/contrib/inlineCompletions/test/browser/editKind.test.ts` (suíte `computeEditKind`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `createTextModel(text)` é `create_text_model` de `tests/support/test_text_model.rs`.
//! - `props as InsertProperties` (e as outras) viram os auxiliares `insert_props`, `delete_props` e
//!   `replace_props`, que falham se a união tiver outra variante.
//! - Os `if (props.textShape.kind === 'singleLine')` viram `if let Some(shape) = ...as_single_line()`, com o
//!   mesmo efeito: a asserção só roda quando a forma é de uma linha.
//! - `new StringEdit([a.replacements[0], b.replacements[0]])` é `StringEdit::new(vec![...])`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::edits::edit::BaseEdit;
use je_editor::edits::string_edit::StringEdit;
use je_editor::inline_completions::edit_kind::{
    DeleteProperties, EditOperation, EditProperties, InlineSuggestionEditKindEdit, InsertProperties,
    InsertLocationShape, ReplaceProperties, SingleCharacterKind, compute_edit_kind,
};
use je_editor::offset_range::OffsetRange;
use je_editor::position::Position;
use support::test_text_model::create_text_model;

fn insert_props(edit: &InlineSuggestionEditKindEdit) -> InsertProperties {
    match edit.properties {
        EditProperties::Insert(props) => props,
        other => panic!("esperava InsertProperties, veio {other:?}"),
    }
}

fn delete_props(edit: &InlineSuggestionEditKindEdit) -> DeleteProperties {
    match edit.properties {
        EditProperties::Delete(props) => props,
        other => panic!("esperava DeleteProperties, veio {other:?}"),
    }
}

fn replace_props(edit: &InlineSuggestionEditKindEdit) -> ReplaceProperties {
    match edit.properties {
        EditProperties::Replace(props) => props,
        other => panic!("esperava ReplaceProperties, veio {other:?}"),
    }
}

// Insert operations

#[test]
fn single_character_insert_syntactical() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::insert(5, ";");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    assert_eq!(result.edits[0].characters_inserted, 1);
    assert_eq!(result.edits[0].characters_deleted, 0);
    assert_eq!(result.edits[0].lines_inserted, 0);
    assert_eq!(result.edits[0].lines_deleted, 0);
    let props = insert_props(&result.edits[0]);
    assert_eq!(props.text_shape.kind(), "singleLine");
    if let Some(shape) = props.text_shape.as_single_line() {
        assert!(shape.is_single_character);
        assert_eq!(shape.single_character_kind, Some(SingleCharacterKind::Syntactical));
    }
    model.dispose();
}

#[test]
fn single_character_insert_identifier() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::insert(5, "a");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    let props = insert_props(&result.edits[0]);
    if let Some(shape) = props.text_shape.as_single_line() {
        assert!(shape.is_single_character);
        assert_eq!(shape.single_character_kind, Some(SingleCharacterKind::Identifier));
    }
    model.dispose();
}

#[test]
fn single_character_insert_whitespace() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::insert(5, " ");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    let props = insert_props(&result.edits[0]);
    if let Some(shape) = props.text_shape.as_single_line() {
        assert!(shape.is_single_character);
        assert_eq!(shape.single_character_kind, Some(SingleCharacterKind::Whitespace));
    }
    model.dispose();
}

#[test]
fn word_insert() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::insert(5, "foo");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    let props = insert_props(&result.edits[0]);
    if let Some(shape) = props.text_shape.as_single_line() {
        assert!(shape.is_word);
        assert!(!shape.is_multiple_words);
    }
    model.dispose();
}

#[test]
fn multiple_words_insert() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::insert(5, "foo bar baz");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    let props = insert_props(&result.edits[0]);
    if let Some(shape) = props.text_shape.as_single_line() {
        assert!(shape.is_multiple_words);
    }
    model.dispose();
}

#[test]
fn multi_line_insert() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::insert(5, "line1\nline2\nline3");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    assert_eq!(result.edits[0].characters_inserted, 17);
    assert_eq!(result.edits[0].characters_deleted, 0);
    assert_eq!(result.edits[0].lines_inserted, 2);
    assert_eq!(result.edits[0].lines_deleted, 0);
    let props = insert_props(&result.edits[0]);
    assert_eq!(props.text_shape.kind(), "multiLine");
    match props.text_shape {
        je_editor::inline_completions::edit_kind::TextShape::MultiLine { line_count } => assert_eq!(line_count, 3),
        _ => {}
    }
    model.dispose();
}

#[test]
fn insert_at_end_of_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello");
    let edit = StringEdit::insert(5, " world");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    let props = insert_props(&result.edits[0]);
    assert_eq!(props.location_shape, InsertLocationShape::EndOfLine);
    model.dispose();
}

#[test]
fn insert_on_empty_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello\n\nworld");
    let edit = StringEdit::insert(6, "text");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    let props = insert_props(&result.edits[0]);
    assert_eq!(props.location_shape, InsertLocationShape::EmptyLine);
    model.dispose();
}

#[test]
fn insert_at_start_of_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::insert(0, "prefix");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    let props = insert_props(&result.edits[0]);
    assert_eq!(props.location_shape, InsertLocationShape::StartOfLine);
    model.dispose();
}

#[test]
fn insert_in_middle_of_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::insert(5, "_");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    let props = insert_props(&result.edits[0]);
    assert_eq!(props.location_shape, InsertLocationShape::MiddleOfLine);
    model.dispose();
}

#[test]
fn insert_relative_to_cursor_at_cursor() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::insert(5, "text");
    let cursor = Position::new(1, 6); // column is 1-based
    let result = compute_edit_kind(&edit, &model, Some(cursor));

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    let props = insert_props(&result.edits[0]);
    let relative = props.relative_to_cursor.unwrap();
    assert!(relative.at_cursor);
    model.dispose();
}

#[test]
fn insert_relative_to_cursor_before_cursor_on_same_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::insert(2, "text");
    let cursor = Position::new(1, 8);
    let result = compute_edit_kind(&edit, &model, Some(cursor));

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    let props = insert_props(&result.edits[0]);
    let relative = props.relative_to_cursor.unwrap();
    assert!(relative.before_cursor_on_same_line);
    model.dispose();
}

#[test]
fn insert_relative_to_cursor_after_cursor_on_same_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::insert(8, "text");
    let cursor = Position::new(1, 4);
    let result = compute_edit_kind(&edit, &model, Some(cursor));

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    let props = insert_props(&result.edits[0]);
    let relative = props.relative_to_cursor.unwrap();
    assert!(relative.after_cursor_on_same_line);
    model.dispose();
}

#[test]
fn insert_relative_to_cursor_lines_above() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("line1\nline2\nline3");
    let edit = StringEdit::insert(0, "text");
    let cursor = Position::new(3, 1);
    let result = compute_edit_kind(&edit, &model, Some(cursor));

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    let props = insert_props(&result.edits[0]);
    let relative = props.relative_to_cursor.unwrap();
    assert_eq!(relative.lines_above, Some(2));
    model.dispose();
}

#[test]
fn insert_relative_to_cursor_lines_below() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("line1\nline2\nline3");
    let edit = StringEdit::insert(12, "text"); // after 'line2\n'
    let cursor = Position::new(1, 1);
    let result = compute_edit_kind(&edit, &model, Some(cursor));

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    let props = insert_props(&result.edits[0]);
    let relative = props.relative_to_cursor.unwrap();
    assert_eq!(relative.lines_below, Some(2));
    model.dispose();
}

#[test]
fn duplicated_whitespace_insert() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello");
    let edit = StringEdit::insert(5, "  ");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    let props = insert_props(&result.edits[0]);
    if let Some(shape) = props.text_shape.as_single_line() {
        assert!(shape.has_duplicated_whitespace);
    }
    model.dispose();
}

// Delete operations

#[test]
fn single_character_delete_identifier() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::delete(OffsetRange::new(4, 5)); // delete 'o'
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Delete);
    let props = delete_props(&result.edits[0]);
    if let Some(shape) = props.text_shape.as_single_line() {
        assert!(shape.is_single_character);
        assert_eq!(shape.single_character_kind, Some(SingleCharacterKind::Identifier));
    }
    model.dispose();
}

#[test]
fn single_character_delete_syntactical() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello;world");
    let edit = StringEdit::delete(OffsetRange::new(5, 6)); // delete ';'
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Delete);
    let props = delete_props(&result.edits[0]);
    if let Some(shape) = props.text_shape.as_single_line() {
        assert!(shape.is_single_character);
        assert_eq!(shape.single_character_kind, Some(SingleCharacterKind::Syntactical));
    }
    model.dispose();
}

#[test]
fn word_delete() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::delete(OffsetRange::new(0, 5)); // delete 'hello'
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Delete);
    assert_eq!(result.edits[0].characters_inserted, 0);
    assert_eq!(result.edits[0].characters_deleted, 5);
    assert_eq!(result.edits[0].lines_inserted, 0);
    assert_eq!(result.edits[0].lines_deleted, 0);
    let props = delete_props(&result.edits[0]);
    if let Some(shape) = props.text_shape.as_single_line() {
        assert!(shape.is_word);
    }
    model.dispose();
}

#[test]
fn multi_line_delete() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("line1\nline2\nline3");
    let edit = StringEdit::delete(OffsetRange::new(0, 12)); // delete 'line1\nline2\n'
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Delete);
    assert_eq!(result.edits[0].characters_inserted, 0);
    assert_eq!(result.edits[0].characters_deleted, 12);
    assert_eq!(result.edits[0].lines_inserted, 0);
    assert_eq!(result.edits[0].lines_deleted, 2);
    let props = delete_props(&result.edits[0]);
    assert_eq!(props.text_shape.kind(), "multiLine");
    model.dispose();
}

#[test]
fn delete_entire_line_content() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello");
    let edit = StringEdit::delete(OffsetRange::new(0, 5));
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Delete);
    let props = delete_props(&result.edits[0]);
    assert!(props.deletes_entire_line_content);
    model.dispose();
}

// Replace operations

#[test]
fn word_to_word_replacement() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::replace(OffsetRange::new(0, 5), "goodbye");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Replace);
    assert_eq!(result.edits[0].characters_inserted, 7);
    assert_eq!(result.edits[0].characters_deleted, 5);
    assert_eq!(result.edits[0].lines_inserted, 0);
    assert_eq!(result.edits[0].lines_deleted, 0);
    let props = replace_props(&result.edits[0]);
    assert!(props.is_word_to_word_replacement);
    model.dispose();
}

#[test]
fn additive_replacement() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hi world");
    let edit = StringEdit::replace(OffsetRange::new(0, 2), "hello");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Replace);
    assert_eq!(result.edits[0].characters_inserted, 5);
    assert_eq!(result.edits[0].characters_deleted, 2);
    let props = replace_props(&result.edits[0]);
    assert!(props.is_additive);
    assert!(!props.is_subtractive);
    model.dispose();
}

#[test]
fn subtractive_replacement() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::replace(OffsetRange::new(0, 5), "hi");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Replace);
    assert_eq!(result.edits[0].characters_inserted, 2);
    assert_eq!(result.edits[0].characters_deleted, 5);
    let props = replace_props(&result.edits[0]);
    assert!(props.is_subtractive);
    assert!(!props.is_additive);
    model.dispose();
}

#[test]
fn single_line_to_multi_line_replacement() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::replace(OffsetRange::new(0, 5), "line1\nline2");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Replace);
    assert_eq!(result.edits[0].lines_inserted, 1);
    assert_eq!(result.edits[0].lines_deleted, 0);
    let props = replace_props(&result.edits[0]);
    assert!(props.is_single_line_to_multi_line);
    model.dispose();
}

#[test]
fn multi_line_to_single_line_replacement() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("line1\nline2\nline3");
    let edit = StringEdit::replace(OffsetRange::new(0, 12), "hello");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Replace);
    assert_eq!(result.edits[0].lines_inserted, 0);
    assert_eq!(result.edits[0].lines_deleted, 2);
    let props = replace_props(&result.edits[0]);
    assert!(props.is_multi_line_to_single_line);
    model.dispose();
}

#[test]
fn single_line_to_single_line_replacement() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::replace(OffsetRange::new(0, 5), "goodbye");
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 1);
    assert_eq!(result.edits[0].operation, EditOperation::Replace);
    let props = replace_props(&result.edits[0]);
    assert!(props.is_single_line_to_single_line);
    model.dispose();
}

// Empty edit

#[test]
fn empty_edit_returns_undefined() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::empty();
    let result = compute_edit_kind(&edit, &model, None);

    assert_eq!(result, None);
    model.dispose();
}

// Multiple replacements

#[test]
fn multiple_inserts() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::new(vec![
        StringEdit::insert(0, "A").replacements()[0].clone(),
        StringEdit::insert(5, "B").replacements()[0].clone(),
    ]);
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 2);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    assert_eq!(result.edits[1].operation, EditOperation::Insert);
    model.dispose();
}

#[test]
fn mixed_operations() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model("hello world");
    let edit = StringEdit::new(vec![
        StringEdit::insert(0, "prefix").replacements()[0].clone(),
        StringEdit::delete(OffsetRange::new(5, 6)).replacements()[0].clone(),
    ]);
    let result = compute_edit_kind(&edit, &model, None);

    let result = result.unwrap();
    assert_eq!(result.edits.len(), 2);
    assert_eq!(result.edits[0].operation, EditOperation::Insert);
    assert_eq!(result.edits[1].operation, EditOperation::Delete);
    model.dispose();
}
