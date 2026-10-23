//! Porte de `src/vs/editor/test/common/viewLayout/lineHeights.test.ts` (suíte `Editor ViewLayout - LineHeightsManager`), caso a caso e na mesma ordem.
//! A segunda suíte do arquivo, `Editor ViewLayout - LineHeightsManager (auto-commit on read)`, vem a partir de `read_after_single_insert_without_commit`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::line_heights::{CustomLineHeightData, LineHeightsManager};

#[test]
fn default_line_height_is_used_when_no_custom_heights_exist() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);

    // Check individual line heights
    assert_eq!(manager.height_for_line_number(1), 10.0);
    assert_eq!(manager.height_for_line_number(5), 10.0);
    assert_eq!(manager.height_for_line_number(100), 10.0);

    // Check accumulated heights
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(1), 10.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(5), 50.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(10), 100.0);
}

#[test]
fn can_change_default_line_height() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.set_default_line_height(20.0);

    // Check individual line heights
    assert_eq!(manager.height_for_line_number(1), 20.0);
    assert_eq!(manager.height_for_line_number(5), 20.0);

    // Check accumulated heights
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(1), 20.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(5), 100.0);
}

#[test]
fn can_add_single_custom_line_height() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 3, 3, 20.0);

    // Check individual line heights
    assert_eq!(manager.height_for_line_number(1), 10.0);
    assert_eq!(manager.height_for_line_number(2), 10.0);
    assert_eq!(manager.height_for_line_number(3), 20.0);
    assert_eq!(manager.height_for_line_number(4), 10.0);

    // Check accumulated heights
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(1), 10.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(2), 20.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(3), 40.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(4), 50.0);
}

#[test]
fn can_add_multiple_custom_line_heights() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 2, 2, 15.0);
    manager.insert_or_change_custom_line_height("dec2", 4, 4, 25.0);

    // Check individual line heights
    assert_eq!(manager.height_for_line_number(1), 10.0);
    assert_eq!(manager.height_for_line_number(2), 15.0);
    assert_eq!(manager.height_for_line_number(3), 10.0);
    assert_eq!(manager.height_for_line_number(4), 25.0);
    assert_eq!(manager.height_for_line_number(5), 10.0);

    // Check accumulated heights
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(1), 10.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(2), 25.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(3), 35.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(4), 60.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(5), 70.0);
}

#[test]
fn can_add_range_of_custom_line_heights() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 2, 4, 15.0);

    // Check individual line heights
    assert_eq!(manager.height_for_line_number(1), 10.0);
    assert_eq!(manager.height_for_line_number(2), 15.0);
    assert_eq!(manager.height_for_line_number(3), 15.0);
    assert_eq!(manager.height_for_line_number(4), 15.0);
    assert_eq!(manager.height_for_line_number(5), 10.0);

    // Check accumulated heights
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(1), 10.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(2), 25.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(3), 40.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(4), 55.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(5), 65.0);
}

#[test]
fn can_change_existing_custom_line_height() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 3, 3, 20.0);
    assert_eq!(manager.height_for_line_number(3), 20.0);

    manager.insert_or_change_custom_line_height("dec1", 3, 3, 30.0);
    assert_eq!(manager.height_for_line_number(3), 30.0);

    // Check accumulated heights after change
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(3), 50.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(4), 60.0);
}

#[test]
fn can_remove_custom_line_height() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 3, 3, 20.0);
    assert_eq!(manager.height_for_line_number(3), 20.0);

    manager.remove_custom_line_height("dec1");
    assert_eq!(manager.height_for_line_number(3), 10.0);

    // Check accumulated heights after removal
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(3), 30.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(4), 40.0);
}

#[test]
fn handles_overlapping_custom_line_heights_last_one_wins() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 3, 5, 20.0);
    manager.insert_or_change_custom_line_height("dec2", 4, 6, 30.0);

    assert_eq!(manager.height_for_line_number(2), 10.0);
    assert_eq!(manager.height_for_line_number(3), 20.0);
    assert_eq!(manager.height_for_line_number(4), 30.0);
    assert_eq!(manager.height_for_line_number(5), 30.0);
    assert_eq!(manager.height_for_line_number(6), 30.0);
    assert_eq!(manager.height_for_line_number(7), 10.0);
}

#[test]
fn handles_deleting_lines_before_custom_line_heights() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 10, 12, 20.0);

    manager.on_lines_deleted(5, 7); // Delete lines 5-7

    assert_eq!(manager.height_for_line_number(7), 20.0); // Was line 10
    assert_eq!(manager.height_for_line_number(8), 20.0); // Was line 11
    assert_eq!(manager.height_for_line_number(9), 20.0); // Was line 12
    assert_eq!(manager.height_for_line_number(10), 10.0);
}

#[test]
fn handles_deleting_lines_overlapping_with_custom_line_heights() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 5, 10, 20.0);

    manager.on_lines_deleted(7, 12); // Delete lines 7-12, including part of decoration

    assert_eq!(manager.height_for_line_number(5), 20.0);
    assert_eq!(manager.height_for_line_number(6), 20.0);
    assert_eq!(manager.height_for_line_number(7), 10.0);
}

#[test]
fn handles_deleting_lines_containing_custom_line_heights_completely() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 5, 7, 20.0);

    manager.on_lines_deleted(4, 8); // Delete lines 4-8, completely contains decoration

    // The decoration collapses to a single line which matches the behavior in the text buffer
    assert_eq!(manager.height_for_line_number(3), 10.0);
    assert_eq!(manager.height_for_line_number(4), 20.0);
    assert_eq!(manager.height_for_line_number(5), 10.0);
}

#[test]
fn handles_deleting_lines_at_the_very_beginning() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("decA", 1, 1, 40.0);

    manager.on_lines_deleted(2, 4); // Delete lines 2-4 after the variable line height

    // Check individual line heights
    assert_eq!(manager.height_for_line_number(1), 40.0);
}

#[test]
fn handles_inserting_lines_before_custom_line_heights() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 5, 7, 20.0);

    manager.on_lines_inserted(3, 4); // Insert 2 lines at line 3

    assert_eq!(manager.height_for_line_number(5), 10.0);
    assert_eq!(manager.height_for_line_number(6), 10.0);
    assert_eq!(manager.height_for_line_number(7), 20.0); // Was line 5
    assert_eq!(manager.height_for_line_number(8), 20.0); // Was line 6
    assert_eq!(manager.height_for_line_number(9), 20.0); // Was line 7
}

#[test]
fn handles_inserting_lines_inside_custom_line_heights_range() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 5, 7, 20.0);

    manager.on_lines_inserted(6, 7); // Insert 2 lines at line 6

    assert_eq!(manager.height_for_line_number(5), 20.0);
    assert_eq!(manager.height_for_line_number(6), 20.0);
    assert_eq!(manager.height_for_line_number(7), 20.0);
    assert_eq!(manager.height_for_line_number(8), 20.0);
    assert_eq!(manager.height_for_line_number(9), 20.0);
}

#[test]
fn changing_decoration_id_maintains_custom_line_height() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 5, 7, 20.0);

    manager.remove_custom_line_height("dec1");
    manager.insert_or_change_custom_line_height("dec2", 5, 7, 20.0);

    assert_eq!(manager.height_for_line_number(5), 20.0);
    assert_eq!(manager.height_for_line_number(6), 20.0);
    assert_eq!(manager.height_for_line_number(7), 20.0);
}

#[test]
fn accumulates_heights_correctly_with_complex_setup() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 3, 3, 15.0);
    manager.insert_or_change_custom_line_height("dec2", 5, 7, 20.0);
    manager.insert_or_change_custom_line_height("dec3", 10, 10, 30.0);

    // Check accumulated heights
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(1), 10.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(2), 20.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(3), 35.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(4), 45.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(5), 65.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(7), 105.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(9), 125.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(10), 155.0);
}

#[test]
fn partial_deletion_with_multiple_lines_for_the_same_decoration_id() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("decSame", 5, 5, 20.0);
    manager.insert_or_change_custom_line_height("decSame", 6, 6, 25.0);

    // Delete one line that partially intersects the same decoration
    manager.on_lines_deleted(6, 6);

    // Check individual line heights
    assert_eq!(manager.height_for_line_number(5), 10.0);
    assert_eq!(manager.height_for_line_number(6), 25.0);
}

#[test]
fn overlapping_decorations_use_maximum_line_height() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("decA", 3, 5, 40.0);
    manager.insert_or_change_custom_line_height("decB", 4, 6, 30.0);

    // Check individual line heights
    assert_eq!(manager.height_for_line_number(3), 40.0);
    assert_eq!(manager.height_for_line_number(4), 40.0);
    assert_eq!(manager.height_for_line_number(5), 40.0);
    assert_eq!(manager.height_for_line_number(6), 30.0);
}

#[test]
fn on_lines_inserted_with_same_decoration_id_extending_to_inserted_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    // Set up a special line at line 1 with decoration 'decA'
    manager.insert_or_change_custom_line_height("decA", 1, 1, 30.0);

    assert_eq!(manager.height_for_line_number(1), 30.0);
    assert_eq!(manager.height_for_line_number(2), 10.0);

    // Insert line 2 to line 2, with the same decoration ID 'decA' covering line 2
    manager.on_lines_inserted(2, 2);
    manager.insert_or_change_custom_line_height("decA", 2, 2, 30.0);

    // After insertion, the decoration 'decA' now covers line 2
    // Since insertOrChangeCustomLineHeight removes the old decoration first,
    // line 1 no longer has the custom height, and line 2 gets it
    assert_eq!(manager.height_for_line_number(1), 10.0);
    assert_eq!(manager.height_for_line_number(2), 30.0);
    assert_eq!(manager.height_for_line_number(3), 10.0);
}

// Suíte `Editor ViewLayout - LineHeightsManager (auto-commit on read)`.

#[test]
fn read_after_single_insert_without_commit() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 3, 3, 20.0);
    // No commit() call, read should still work
    assert_eq!(manager.height_for_line_number(1), 10.0);
    assert_eq!(manager.height_for_line_number(3), 20.0);
    assert_eq!(manager.height_for_line_number(4), 10.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(3), 40.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(4), 50.0);
}

#[test]
fn read_after_multiple_inserts_without_commit() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 2, 2, 15.0);
    manager.insert_or_change_custom_line_height("dec2", 4, 4, 25.0);
    // No commit() call
    assert_eq!(manager.height_for_line_number(2), 15.0);
    assert_eq!(manager.height_for_line_number(3), 10.0);
    assert_eq!(manager.height_for_line_number(4), 25.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(4), 60.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(5), 70.0);
}

#[test]
fn read_after_remove_without_commit() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 3, 3, 20.0);
    assert_eq!(manager.height_for_line_number(3), 20.0);

    manager.remove_custom_line_height("dec1");
    // No commit() call
    assert_eq!(manager.height_for_line_number(3), 10.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(3), 30.0);
}

#[test]
fn insert_then_remove_same_decoration_without_commit() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 3, 3, 20.0);
    manager.remove_custom_line_height("dec1");
    // No commit() call, should see default height
    assert_eq!(manager.height_for_line_number(3), 10.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(3), 30.0);
}

#[test]
fn insert_same_decoration_id_twice_without_commit_replaces_first() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 3, 3, 20.0);
    manager.insert_or_change_custom_line_height("dec1", 5, 5, 30.0);
    // No commit(), second call should replace first
    assert_eq!(manager.height_for_line_number(3), 10.0);
    assert_eq!(manager.height_for_line_number(5), 30.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(5), 70.0);
}

#[test]
fn interleaved_callers_remove_must_cancel_queued_inserts_before_first_flush() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);

    // Caller A queues decoration insert.
    manager.insert_or_change_custom_line_height("decA", 3, 3, 20.0);
    // Caller B queues independent insert.
    manager.insert_or_change_custom_line_height("decB", 4, 4, 30.0);
    // Caller A removes its decoration before any flush occurs.
    manager.remove_custom_line_height("decA");
    // Caller B triggers a structural change that causes queue flush in the middle of commit.
    manager.on_lines_inserted(1, 1);

    // decA must stay removed. If queued inserts are not canceled on remove, decA incorrectly survives.
    assert_eq!(manager.height_for_line_number(4), 10.0);
    assert_eq!(manager.height_for_line_number(5), 30.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(5), 70.0);
}

#[test]
fn interleaved_callers_remove_must_cancel_queued_inserts_before_delete_flush() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);

    manager.insert_or_change_custom_line_height("decA", 3, 3, 20.0);
    manager.insert_or_change_custom_line_height("decB", 5, 5, 30.0);
    manager.remove_custom_line_height("decA");
    manager.on_lines_deleted(1, 1);

    // After deleting line 1, decB shifts from line 5 to line 4.
    // decA must remain removed even though its insert was queued before the remove.
    assert_eq!(manager.height_for_line_number(2), 10.0);
    assert_eq!(manager.height_for_line_number(3), 10.0);
    assert_eq!(manager.height_for_line_number(4), 30.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(4), 60.0);
}

// --- Interleaved operations ---

#[test]
fn interleaved_insert_insert_on_lines_inserted_on_lines_deleted_remove_remove_insert_insert_read() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    // Step 1-2: two inserts
    manager.insert_or_change_custom_line_height("dec1", 2, 2, 20.0);
    manager.insert_or_change_custom_line_height("dec2", 5, 5, 30.0);
    // Step 3: insert 2 lines at line 3 (shifts dec2 from line 5 to 7)
    manager.on_lines_inserted(3, 4);
    // Step 4: delete line 1 (shifts dec1 from line 2 to 1, dec2 from line 7 to 6)
    manager.on_lines_deleted(1, 1);
    // Step 5-6: remove the two decorations
    manager.remove_custom_line_height("dec1");
    manager.remove_custom_line_height("dec2");
    // Step 7-8: two new inserts
    manager.insert_or_change_custom_line_height("dec3", 3, 3, 40.0);
    manager.insert_or_change_custom_line_height("dec4", 5, 5, 50.0);
    // Read, no explicit commit
    assert_eq!(manager.height_for_line_number(1), 10.0);
    assert_eq!(manager.height_for_line_number(3), 40.0);
    assert_eq!(manager.height_for_line_number(4), 10.0);
    assert_eq!(manager.height_for_line_number(5), 50.0);
    assert_eq!(manager.height_for_line_number(6), 10.0);
}

#[test]
fn interleaved_insert_on_lines_inserted_remove_read() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 3, 3, 20.0);
    // Insert 1 line at line 1, dec1 shifts from 3 to 4
    manager.on_lines_inserted(1, 1);
    manager.remove_custom_line_height("dec1");
    // Read, no explicit commit
    assert_eq!(manager.height_for_line_number(3), 10.0);
    assert_eq!(manager.height_for_line_number(4), 10.0);
}

#[test]
fn interleaved_on_lines_deleted_insert_read() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 5, 5, 20.0);
    // Delete lines 1-2, dec1 shifts from 5 to 3
    manager.on_lines_deleted(1, 2);
    // Insert a new decoration
    manager.insert_or_change_custom_line_height("dec2", 1, 1, 30.0);
    // Read, no explicit commit
    assert_eq!(manager.height_for_line_number(1), 30.0);
    assert_eq!(manager.height_for_line_number(2), 10.0);
    assert_eq!(manager.height_for_line_number(3), 20.0);
}

#[test]
fn interleaved_insert_on_lines_deleted_insert_read() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 3, 3, 20.0);
    // Delete line 1, dec1 should shift from 3 to 2
    manager.on_lines_deleted(1, 1);
    // Add another decoration
    manager.insert_or_change_custom_line_height("dec2", 5, 5, 30.0);
    // Read, no explicit commit
    assert_eq!(manager.height_for_line_number(1), 10.0);
    assert_eq!(manager.height_for_line_number(2), 20.0);
    assert_eq!(manager.height_for_line_number(5), 30.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(5), 80.0);
}

// --- Edge cases ---

#[test]
fn on_lines_inserted_then_on_lines_deleted_without_reads_between() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 3, 3, 20.0);
    // Insert 2 lines at line 1, dec1 moves from 3 to 5
    manager.on_lines_inserted(1, 2);
    // Delete line 1, dec1 moves from 5 to 4
    manager.on_lines_deleted(1, 1);
    // Read
    assert_eq!(manager.height_for_line_number(4), 20.0);
    assert_eq!(manager.height_for_line_number(3), 10.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(4), 50.0);
}

#[test]
fn multiple_on_lines_inserted_without_reads_between() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 3, 3, 20.0);
    // Insert 1 line at line 1, dec1 at 3 to 4
    manager.on_lines_inserted(1, 1);
    // Insert 1 line at line 1, dec1 at 4 to 5
    manager.on_lines_inserted(1, 1);
    // Read
    assert_eq!(manager.height_for_line_number(5), 20.0);
    assert_eq!(manager.height_for_line_number(3), 10.0);
    assert_eq!(manager.height_for_line_number(4), 10.0);
}

#[test]
fn multiple_on_lines_deleted_without_reads_between() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 10, 10, 20.0);
    // Delete lines 1-2, dec1 at 10 to 8
    manager.on_lines_deleted(1, 2);
    // Delete lines 1-2, dec1 at 8 to 6
    manager.on_lines_deleted(1, 2);
    // Read
    assert_eq!(manager.height_for_line_number(6), 20.0);
    assert_eq!(manager.height_for_line_number(7), 10.0);
}

#[test]
fn pending_insert_then_on_lines_deleted_affecting_that_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    // Insert a decoration at line 3 (pending, not committed)
    manager.insert_or_change_custom_line_height("dec1", 3, 3, 20.0);
    // Delete line 3, should remove/collapse the pending decoration
    manager.on_lines_deleted(3, 3);
    // Read, the decoration was on the deleted line
    // The decoration collapses to line 3 (fromLineNumber) per onLinesDeleted behavior
    assert_eq!(manager.height_for_line_number(3), 20.0);
}

#[test]
fn pending_insert_then_on_lines_inserted_shifting_that_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    // Insert a decoration at line 3 (pending, not committed)
    manager.insert_or_change_custom_line_height("dec1", 3, 3, 20.0);
    // Insert 2 lines before it at line 1, should shift dec1 from 3 to 5
    manager.on_lines_inserted(1, 2);
    // Read
    assert_eq!(manager.height_for_line_number(3), 10.0);
    assert_eq!(manager.height_for_line_number(5), 20.0);
}

#[test]
fn accumulated_heights_correct_after_interleaved_ops_without_commit() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 2, 2, 15.0);
    manager.insert_or_change_custom_line_height("dec2", 4, 4, 25.0);
    // No commit, verify accumulated heights
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(1), 10.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(2), 25.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(3), 35.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(4), 60.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(5), 70.0);
}

#[test]
fn constructor_with_initial_data_works_without_explicit_commit() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let data = [CustomLineHeightData::new("dec1", 2, 4, 20.0), CustomLineHeightData::new("dec2", 6, 6, 30.0)];
    let mut manager = LineHeightsManager::new(10.0, &data);
    assert_eq!(manager.height_for_line_number(1), 10.0);
    assert_eq!(manager.height_for_line_number(2), 20.0);
    assert_eq!(manager.height_for_line_number(3), 20.0);
    assert_eq!(manager.height_for_line_number(4), 20.0);
    assert_eq!(manager.height_for_line_number(5), 10.0);
    assert_eq!(manager.height_for_line_number(6), 30.0);
    assert_eq!(manager.get_accumulated_line_heights_including_line_number(6), 110.0);
}

#[test]
fn deleting_line_2_with_line_heights_removed_re_adding_at_line_1_moves_special_line_to_line_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut manager = LineHeightsManager::new(10.0, &[]);
    manager.insert_or_change_custom_line_height("dec1", 2, 2, 20.0);
    assert_eq!(manager.height_for_line_number(2), 20.0);
    manager.on_lines_deleted(2, 2);
    manager.insert_or_change_custom_line_height("dec1", 1, 1, 20.0);
    assert_eq!(manager.height_for_line_number(1), 20.0);
}
