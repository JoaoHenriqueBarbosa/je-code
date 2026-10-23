//! Porte de `src/vs/editor/test/common/model/annotations.test.ts` (suíte `Annotations Suite`), caso a caso e na mesma ordem.
//!
//! A infraestrutura visual (`[id:texto]` marca uma anotação, `<id:texto>` apaga numa atualização) é a mesma do
//! original. O texto base é guardado em unidades UTF-16 (`Vec<u16>`), como o `string` do JavaScript.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::annotations::{AnnotatedString, Annotation, AnnotationUpdate, AnnotationsUpdate};
use je_editor::edits::edit::BaseEdit;
use je_editor::edits::string_edit::{BaseStringEdit, StringEdit};
use je_editor::offset_range::OffsetRange;

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn text_of(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// `visual.indexOf(ch, from)`.
fn index_of(visual: &[u16], ch: char, from: usize) -> Option<usize> {
    (from..visual.len()).find(|&i| visual[i] == ch as u16)
}

/// Lê `[id:texto]` (e `<id:texto>` quando `allow_delete`): devolve os intervalos com o id (`None` para o
/// apagar) e o texto base.
fn parse_visual(visual: &[u16], allow_delete: bool) -> (Vec<(OffsetRange, Option<String>)>, Vec<u16>) {
    let mut entries: Vec<(OffsetRange, Option<String>)> = Vec::new();
    let mut base_string: Vec<u16> = Vec::new();
    let mut i = 0;

    while i < visual.len() {
        let is_set = visual[i] == '[' as u16;
        let is_delete = allow_delete && visual[i] == '<' as u16;
        if is_set || is_delete {
            let close = if is_set { ']' } else { '>' };
            let colon_idx = index_of(visual, ':', i + 1);
            let close_idx = colon_idx.and_then(|c| index_of(visual, close, c + 1));
            let (Some(colon_idx), Some(close_idx)) = (colon_idx, close_idx) else {
                let what = if is_set { "annotation" } else { "delete" };
                panic!("Invalid {what} format at position {i}");
            };
            let id = text_of(&visual[i + 1..colon_idx]);
            let text = &visual[colon_idx + 1..close_idx];
            let start_offset = base_string.len() as i32;
            base_string.extend_from_slice(text);
            let range = OffsetRange::new(start_offset, base_string.len() as i32);
            entries.push((range, if is_set { Some(id) } else { None }));
            i = close_idx + 1;
        } else {
            base_string.push(visual[i]);
            i += 1;
        }
    }

    (entries, base_string)
}

/// `parseVisualAnnotations`.
fn parse_visual_annotations(visual: &str) -> (Vec<Annotation<String>>, Vec<u16>) {
    let (entries, base_string) = parse_visual(&units(visual), false);
    let annotations = entries
        .into_iter()
        .map(|(range, id)| Annotation { range, annotation: id.expect("anotação definida") })
        .collect();
    (annotations, base_string)
}

/// `toVisualString`.
fn to_visual_string(annotations: &[Annotation<String>], base_string: &[u16]) -> String {
    if annotations.is_empty() {
        return text_of(base_string);
    }

    // Ordena as anotações pela posição inicial
    let mut sorted_annotations = annotations.to_vec();
    sorted_annotations.sort_by_key(|a| a.range.start);

    // Monta a representação visual
    let mut result = String::new();
    let mut pos: i64 = 0;

    for ann in &sorted_annotations {
        // Texto simples antes desta anotação
        result += &text_of(je_base::js_string::substring(base_string, pos, i64::from(ann.range.start)));
        // Conteúdo anotado com o id
        let annotated_text = text_of(ann.range.substring(base_string));
        result += &format!("[{}:{}]", ann.annotation, annotated_text);
        pos = i64::from(ann.range.end_exclusive);
    }

    // Texto restante depois da última anotação
    result += &text_of(je_base::js_string::substring(base_string, pos, base_string.len() as i64));

    result
}

/// `VisualAnnotatedString`: uma `AnnotatedString` com o texto base, para teste visual.
struct VisualAnnotatedString {
    annotated_string: AnnotatedString<String>,
    base_string: Vec<u16>,
}

impl VisualAnnotatedString {
    fn new(annotated_string: AnnotatedString<String>, base_string: Vec<u16>) -> Self {
        Self { annotated_string, base_string }
    }

    fn set_annotations(&mut self, update: &AnnotationsUpdate<String>) {
        self.annotated_string.set_annotations(update);
    }

    fn apply_edit(&mut self, edit: &StringEdit) {
        self.annotated_string.apply_edit(edit);
        self.base_string = edit.apply(&self.base_string).units().to_vec();
    }

    fn get_annotations_intersecting(&self, range: OffsetRange) -> Vec<Annotation<String>> {
        self.annotated_string.get_annotations_intersecting(range)
    }

    fn get_all_annotations(&self) -> Vec<Annotation<String>> {
        self.annotated_string.get_all_annotations()
    }

    fn clone(&self) -> VisualAnnotatedString {
        VisualAnnotatedString::new(self.annotated_string.clone_shallow(), self.base_string.clone())
    }
}

/// `fromVisual`.
fn from_visual(visual: &str) -> VisualAnnotatedString {
    let (annotations, base_string) = parse_visual_annotations(visual);
    VisualAnnotatedString::new(AnnotatedString::new(annotations), base_string)
}

/// `toVisual`.
fn to_visual(vas: &VisualAnnotatedString) -> String {
    to_visual_string(&vas.get_all_annotations(), &vas.base_string)
}

/// `parseVisualUpdate`: `[id:texto]` define a anotação e `<id:texto>` a apaga (o intervalo é guardado, a
/// anotação é `None`).
fn parse_visual_update(visual: &str) -> (Vec<AnnotationUpdate<String>>, Vec<u16>) {
    let (entries, base_string) = parse_visual(&units(visual), true);
    let updates = entries.into_iter().map(|(range, id)| Annotation { range, annotation: id }).collect();
    (updates, base_string)
}

/// `updateFromVisual`.
fn update_from_visual(visuals: &[&str]) -> AnnotationsUpdate<String> {
    let mut updates: Vec<AnnotationUpdate<String>> = Vec::new();

    for visual in visuals {
        let (parsed_updates, _) = parse_visual_update(visual);
        updates.extend(parsed_updates);
    }

    AnnotationsUpdate::create(updates)
}

fn edit_delete(start: i32, end: i32) -> StringEdit {
    StringEdit::replace(OffsetRange::new(start, end), "")
}

fn edit_insert(pos: i32, text: &str) -> StringEdit {
    StringEdit::insert(pos, text)
}

fn edit_replace(start: i32, end: i32, text: &str) -> StringEdit {
    StringEdit::replace(OffsetRange::new(start, end), text)
}

/// `assertVisual`: só compara as anotações, não o texto base (o `setAnnotations` não muda o texto).
fn assert_visual(vas: &VisualAnnotatedString, expected_visual: &str) {
    let actual = to_visual(vas);
    let (expected_annotations, _) = parse_visual_annotations(expected_visual);
    let actual_annotations = vas.get_all_annotations();

    // Compara as anotações para mensagens de erro melhores
    if actual_annotations.len() != expected_annotations.len() {
        panic!(
            "Annotation count mismatch.\n  Expected: {expected_visual}\n  Actual:   {actual}\n  Expected {} annotations, got {}",
            expected_annotations.len(),
            actual_annotations.len()
        );
    }

    for (i, (actual_ann, expected)) in actual_annotations.iter().zip(&expected_annotations).enumerate() {
        if actual_ann.range.start != expected.range.start || actual_ann.range.end_exclusive != expected.range.end_exclusive {
            panic!(
                "Annotation {i} range mismatch.\n  Expected: ({}, {})\n  Actual:   ({}, {})\n  Expected visual: {expected_visual}\n  Actual visual:   {actual}",
                expected.range.start, expected.range.end_exclusive, actual_ann.range.start, actual_ann.range.end_exclusive
            );
        }
        if actual_ann.annotation != expected.annotation {
            panic!(
                "Annotation {i} value mismatch.\n  Expected: \"{}\"\n  Actual:   \"{}\"",
                expected.annotation, actual_ann.annotation
            );
        }
    }
}

/// `visualizeEdit`: o estado antes e depois da edição, como texto visual.
struct VisualizedEdit {
    #[allow(dead_code)]
    before: String,
    after: String,
}

fn visualize_edit(before_annotations: &str, edit: &StringEdit) -> VisualizedEdit {
    let mut vas = from_visual(before_annotations);
    let before = to_visual(&vas);

    vas.apply_edit(edit);

    let after = to_visual(&vas);
    VisualizedEdit { before, after }
}

fn ids(annotations: &[Annotation<String>]) -> Vec<&str> {
    annotations.iter().map(|a| a.annotation.as_str()).collect()
}

// suite('Annotations Suite')

#[test]
fn set_annotations_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut vas = from_visual("[1:Lorem] ipsum [2:dolor] sit [3:amet]");
    vas.set_annotations(&update_from_visual(&["[4:Lorem i]"]));
    assert_visual(&vas, "[4:Lorem i]psum [2:dolor] sit [3:amet]");
    vas.set_annotations(&update_from_visual(&["Lorem ip[5:s]"]));
    assert_visual(&vas, "[4:Lorem i]p[5:s]um [2:dolor] sit [3:amet]");
}

#[test]
fn set_annotations_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut vas = from_visual("[1:Lorem] ipsum [2:dolor] sit [3:amet]");
    vas.set_annotations(&update_from_visual(&["L<_:orem ipsum d>", "[4:Lorem ]"]));
    assert_visual(&vas, "[4:Lorem ]ipsum dolor sit [3:amet]");
    vas.set_annotations(&update_from_visual(&["Lorem <_:ipsum dolor sit amet>", "[5:Lor]"]));
    assert_visual(&vas, "[5:Lor]em ipsum dolor sit amet");
    vas.set_annotations(&update_from_visual(&["L[6:or]"]));
    assert_visual(&vas, "L[6:or]em ipsum dolor sit amet");
}

#[test]
fn set_annotations_3() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut vas = from_visual("[1:Lorem] ipsum [2:dolor] sit [3:amet]");
    vas.set_annotations(&update_from_visual(&["Lore[4:m ipsum dolor ]"]));
    assert_visual(&vas, "Lore[4:m ipsum dolor ]sit [3:amet]");
    vas.set_annotations(&update_from_visual(&["Lorem ipsum dolor sit [5:a]"]));
    assert_visual(&vas, "Lore[4:m ipsum dolor ]sit [5:a]met");
}

#[test]
fn set_annotations_4() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // 54 chars before 'i': "Lorem ipsum dolor sit amet, consectetur adipiscing el"
    let mut vas = from_visual("Lorem ipsum dolor sit amet, consectetur adipiscing el[:it]");
    vas.set_annotations(&update_from_visual(&["Lorem ipsum dolor sit amet, consectetur adipiscing el<_:i>t"]));
    assert_visual(&vas, "Lorem ipsum dolor sit amet, consectetur adipiscing elit");
}

#[test]
fn get_annotations_intersecting_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let vas = from_visual("[1:Lorem] ipsum [2:dolor] sit [3:amet]");
    let result1 = vas.get_annotations_intersecting(OffsetRange::new(0, 13));
    assert_eq!(result1.len(), 2);
    assert_eq!(ids(&result1), ["1", "2"]);
    let result2 = vas.get_annotations_intersecting(OffsetRange::new(0, 22));
    assert_eq!(result2.len(), 3);
    assert_eq!(ids(&result2), ["1", "2", "3"]);
}

#[test]
fn get_annotations_intersecting_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let vas = from_visual("[1:Lorem] [2:i]p[3:s]");

    let result1 = vas.get_annotations_intersecting(OffsetRange::new(5, 7));
    assert_eq!(result1.len(), 1);
    assert_eq!(ids(&result1), ["2"]);
    let result2 = vas.get_annotations_intersecting(OffsetRange::new(5, 9));
    assert_eq!(result2.len(), 2);
    assert_eq!(ids(&result2), ["2", "3"]);
}

#[test]
fn get_annotations_intersecting_3() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut vas = from_visual("[1:Lorem] ipsum [2:dolor]");
    let result1 = vas.get_annotations_intersecting(OffsetRange::new(4, 13));
    assert_eq!(result1.len(), 2);
    assert_eq!(ids(&result1), ["1", "2"]);
    vas.set_annotations(&update_from_visual(&["[3:Lore]m[4: ipsu]"]));
    assert_visual(&vas, "[3:Lore]m[4: ipsu]m [2:dolor]");
    let result2 = vas.get_annotations_intersecting(OffsetRange::new(7, 13));
    assert_eq!(result2.len(), 2);
    assert_eq!(ids(&result2), ["4", "2"]);
}

#[test]
fn get_annotations_intersecting_4() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut vas = from_visual("[1:Lorem ipsum] sit");
    vas.set_annotations(&update_from_visual(&["Lorem ipsum [2:sit]"]));
    let result = vas.get_annotations_intersecting(OffsetRange::new(2, 8));
    assert_eq!(result.len(), 1);
    assert_eq!(ids(&result), ["1"]);
}

#[test]
fn get_annotations_intersecting_5() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let vas = from_visual("[1:Lorem ipsum] [2:dol] [3:or]");
    let result = vas.get_annotations_intersecting(OffsetRange::new(1, 16));
    assert_eq!(result.len(), 3);
    assert_eq!(ids(&result), ["1", "2", "3"]);
}

#[test]
fn get_annotations_intersecting_6() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let vas = from_visual("[1:Lorem ][2:ip][3:sum]");
    let result = vas.get_annotations_intersecting(OffsetRange::new(6, 6));
    assert_eq!(result.len(), 1);
    assert_eq!(ids(&result), ["2"]);
}

#[test]
fn apply_edit_1_deletion_within_annotation() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = visualize_edit("[1:Lorem] ipsum [2:dolor] sit [3:amet]", &edit_delete(0, 3));
    assert_eq!(result.after, "[1:em] ipsum [2:dolor] sit [3:amet]");
}

#[test]
fn apply_edit_2_deletion_and_insertion_within_annotation() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = visualize_edit("[1:Lorem] ipsum [2:dolor] sit [3:amet]", &edit_replace(1, 3, "XXXXX"));
    assert_eq!(result.after, "[1:LXXXXXem] ipsum [2:dolor] sit [3:amet]");
}

#[test]
fn apply_edit_3_deletion_across_several_annotations() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = visualize_edit("[1:Lorem] ipsum [2:dolor] sit [3:amet]", &edit_replace(4, 22, "XXXXX"));
    assert_eq!(result.after, "[1:LoreXXXXX][3:amet]");
}

#[test]
fn apply_edit_4_deletion_between_annotations() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = visualize_edit("[1:Lorem ip]sum and [2:dolor] sit [3:amet]", &edit_delete(10, 12));
    assert_eq!(result.after, "[1:Lorem ip]suand [2:dolor] sit [3:amet]");
}

#[test]
fn apply_edit_5_deletion_that_covers_annotation() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = visualize_edit("[1:Lorem] ipsum [2:dolor] sit [3:amet]", &edit_delete(0, 5));
    assert_eq!(result.after, " ipsum [2:dolor] sit [3:amet]");
}

#[test]
fn apply_edit_6_several_edits() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut vas = from_visual("[1:Lorem] ipsum [2:dolor] sit [3:amet]");
    let edit = StringEdit::compose_all(&[
        StringEdit::replace(OffsetRange::new(0, 6), ""),
        StringEdit::replace(OffsetRange::new(6, 12), ""),
        StringEdit::replace(OffsetRange::new(12, 17), ""),
    ]);
    vas.apply_edit(&edit);
    assert_visual(&vas, "ipsum sit [3:am]");
}

#[test]
fn apply_edit_7_several_edits() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut vas = from_visual("[1:Lorem] ipsum [2:dolor] sit [3:amet]");
    let edit1 = StringEdit::replace(OffsetRange::new(0, 3), "XXXX");
    let edit2 = StringEdit::replace(OffsetRange::new(0, 2), "");
    vas.apply_edit(&edit1.compose(&edit2));
    assert_visual(&vas, "[1:XXem] ipsum [2:dolor] sit [3:amet]");
}

#[test]
fn apply_edit_9_insertion_at_end_of_annotation() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = visualize_edit("[1:Lorem] ipsum [2:dolor] sit [3:amet]", &edit_insert(17, "XXX"));
    assert_eq!(result.after, "[1:Lorem] ipsum [2:dolor]XXX sit [3:amet]");
}

#[test]
fn apply_edit_10_insertion_in_middle_of_annotation() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = visualize_edit("[1:Lorem] ipsum [2:dolor] sit [3:amet]", &edit_insert(14, "XXX"));
    assert_eq!(result.after, "[1:Lorem] ipsum [2:doXXXlor] sit [3:amet]");
}

#[test]
fn apply_edit_11_replacement_consuming_annotation() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = visualize_edit("[1:L]o[2:rem] [3:i]", &edit_replace(1, 6, "X"));
    assert_eq!(result.after, "[1:L]X[3:i]");
}

#[test]
fn apply_edit_12_multiple_disjoint_edits() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut vas = from_visual("[1:Lorem] ipsum [2:dolor] sit [3:amet!] [4:done]");

    let edit = StringEdit::compose_all(&[
        StringEdit::insert(0, "X"),
        StringEdit::delete(OffsetRange::new(12, 13)),
        StringEdit::replace(OffsetRange::new(21, 22), "YY"),
        StringEdit::replace(OffsetRange::new(28, 32), "Z"),
    ]);
    vas.apply_edit(&edit);
    assert_visual(&vas, "X[1:Lorem] ipsum[2:dolor] sitYY[3:amet!]Z[4:e]");
}

#[test]
fn apply_edit_13_edit_on_the_left_border() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = visualize_edit("lorem ipsum dolor[1: ]", &edit_insert(17, "X"));
    assert_eq!(result.after, "lorem ipsum dolorX[1: ]");
}

#[test]
fn rebase() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let a = VisualAnnotatedString::new(
        AnnotatedString::new(vec![Annotation { range: OffsetRange::new(2, 5), annotation: "1".to_string() }]),
        units("sitamet"),
    );
    let mut a = a;
    let mut b = a.clone();
    let mut update: AnnotationsUpdate<String> = AnnotationsUpdate::create(vec![Annotation {
        range: OffsetRange::new(4, 5),
        annotation: Some("2".to_string()),
    }]);

    b.set_annotations(&update);
    let edit: StringEdit = StringEdit::replace(OffsetRange::new(1, 6), "XXX");

    a.apply_edit(&edit);
    b.apply_edit(&edit);

    update.rebase(&edit);

    a.set_annotations(&update);
    assert_eq!(a.get_all_annotations(), b.get_all_annotations());
}
