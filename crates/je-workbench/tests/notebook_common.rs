//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookCommon.test.ts` (suítes `Notebook stream
//! compression`, `NotebookCommon`, `CellUri`, `CellRange` e `NotebookWorkingCopyTypeIdentifier`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso.
//! - O `setup` da suíte `NotebookCommon` (o `DisposableStore`, o `setupInstantiationService` e o `ILanguageService`)
//!   é [`setup`], chamado no começo de cada caso dessa suíte; os `disposables.dispose()` do fim ficam onde estão.
//! - `TestCell` e `setupInstantiationService` vêm de `support::test_notebook_editor` (ver as divergências lá).
//! - `diff<TestCell>` trabalha com `Rc<TestCell>`; `cells.indexOf(cell) > -1` é a comparação por identidade
//!   (`Rc::ptr_eq`), e os `deepStrictEqual` de emendas comparam as células por identidade (`PartialEq` do `TestCell`).
//! - Os `Uint8Array` de `VSBuffer.fromString(input).buffer` e os `subarray` são fatias de bytes.
//! - `JSON.stringify(input)` da mensagem de falha é a forma `Debug` do texto.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::buffer::VSBuffer;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::mime::mimes;
use je_base::sequence::Splice;
use je_base::uri::URI;
use je_editor::language_service::ILanguageService;
use je_instantiation::InstantiationService;
use je_workbench::contrib::notebook::common::notebook_common::{
    CellKind, CellUri, MimeTypeDisplayOrder, NotebookWorkingCopyType, NotebookWorkingCopyTypeIdentifier, compress_output_item_streams, diff,
};
use je_workbench::contrib::notebook::common::notebook_range::{ICellRange, cell_indexes_to_ranges, cell_ranges_to_indexes, reduce_cell_ranges};
use support::test_notebook_editor::{TestCell, get_language_service, setup_instantiation_service};

// Suíte `Notebook stream compression`

/// `[input, expected, didCompression]`.
const CASES: [(&str, &str, bool); 22] = [
    ("", "", false),
    ("plain\ntext", "plain\ntext", false),
    ("abcdef\rxy", "xycdef", true),
    ("abcdef\rxy\rz", "zycdef", true),
    ("xy\rlonger\rz", "zonger", true),
    ("abc\r", "abc\r", false),
    ("abc\r\r", "abc", true),
    ("abc\r\r\r", "abc", true),
    ("abcdef\rxy\r", "xycdef\r", true),
    ("abc\r\n", "abc\n", true),
    ("abc\r\r\n", "abc\n", true),
    ("abc\rx\ndef\ry", "xbc\nyef", true),
    ("abc\u{2028}def\rx\u{2029}ghi\ry", "abc\u{2028}xef\u{2029}yhi", true),
    ("\u{6f22}\u{5b57}\rX", "X\u{5b57}", true),
    ("\u{1f600}xyz\rA", "A\u{fffd}xyz", true),
    ("abc\x08\x08X\rY", "YX", true),
    ("a\n\x08", "a\n\x08", false),
    ("abc\r\x08X", "abcX", true),
    ("$$\r\r", "$", true),
    ("abc\r$xy", "abc\r$xy", false),
    ("abc\r$xy\nq\rr", "$xy\nr", true),
    ("price $5\nabcdef\rxy", "price $5\nxycdef", true),
];

/// `{ text: result.data.toString(), didCompression: result.didCompression }`.
fn compress(streams: &[&[u8]]) -> (String, bool) {
    let result = compress_output_item_streams(streams);
    (result.data.to_string(), result.did_compression)
}

#[test]
fn preserves_overwrite_line_ending_backspace_unicode_and_dollar_semantics() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let actual: Vec<(String, bool)> = CASES
        .iter()
        .map(|(input, _, _)| {
            let bytes = VSBuffer::from_string(input).to_vec();
            compress(&[&bytes])
        })
        .collect();
    let expected: Vec<(String, bool)> = CASES.iter().map(|(_, text, did_compression)| (text.to_string(), *did_compression)).collect();
    assert_eq!(actual, expected);
}

#[test]
fn preserves_normalization_when_input_is_split_across_byte_buffers() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    for (input, text, did_compression) in CASES {
        let bytes = VSBuffer::from_string(input).to_vec();
        let results: Vec<(String, bool)> = (0..=bytes.len()).map(|index| compress(&[&bytes[..index], &bytes[index..]])).collect();
        let expected: Vec<(String, bool)> = results.iter().map(|_| (text.to_string(), did_compression)).collect();
        assert_eq!(results, expected, "{input:?}");
    }
}

#[test]
fn combines_thousands_of_carriage_return_segments_on_one_line() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let input = format!("abcdefghijklmnop{}\n", "\r01234567\rxy\rz".repeat(4000));
    let bytes = VSBuffer::from_string(&input).to_vec();
    assert_eq!(compress(&[&bytes]), ("zy234567ijklmnop\n".to_string(), true));
}

// Suíte `NotebookCommon`

struct Fixture {
    disposables: Rc<DisposableStore>,
    _instantiation_service: Rc<InstantiationService>,
    language_service: Rc<dyn ILanguageService>,
}

/// O `setup` da suíte.
fn setup() -> Fixture {
    let disposables = DisposableStore::new();
    let instantiation_service = setup_instantiation_service(&disposables);
    let language_service = get_language_service(&instantiation_service);
    Fixture { disposables, _instantiation_service: instantiation_service, language_service }
}

#[test]
fn sort_mime_types_default_orders() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup();
    assert_eq!(
        MimeTypeDisplayOrder::default().sort([
            "application/json",
            "application/javascript",
            "text/html",
            "image/svg+xml",
            mimes::LATEX,
            mimes::MARKDOWN,
            "image/png",
            "image/jpeg",
            mimes::TEXT
        ]),
        vec![
            "application/json",
            "application/javascript",
            "text/html",
            "image/svg+xml",
            mimes::LATEX,
            mimes::MARKDOWN,
            "image/png",
            "image/jpeg",
            mimes::TEXT
        ]
    );

    assert_eq!(
        MimeTypeDisplayOrder::default().sort([
            "application/json",
            mimes::LATEX,
            mimes::MARKDOWN,
            "application/javascript",
            "text/html",
            mimes::TEXT,
            "image/png",
            "image/jpeg",
            "image/svg+xml"
        ]),
        vec![
            "application/json",
            "application/javascript",
            "text/html",
            "image/svg+xml",
            mimes::LATEX,
            mimes::MARKDOWN,
            "image/png",
            "image/jpeg",
            mimes::TEXT
        ]
    );

    assert_eq!(
        MimeTypeDisplayOrder::default().sort([
            mimes::MARKDOWN,
            "application/json",
            mimes::TEXT,
            "image/jpeg",
            "application/javascript",
            "text/html",
            "image/png",
            "image/svg+xml"
        ]),
        vec![
            "application/json",
            "application/javascript",
            "text/html",
            "image/svg+xml",
            mimes::MARKDOWN,
            "image/png",
            "image/jpeg",
            mimes::TEXT
        ]
    );

    fixture.disposables.dispose();
}

#[test]
fn sort_mime_types_user_orders() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup();
    assert_eq!(
        MimeTypeDisplayOrder::new(&["image/png", mimes::TEXT, mimes::MARKDOWN, "text/html", "application/json"], None).sort([
            "application/json",
            "application/javascript",
            "text/html",
            "image/svg+xml",
            mimes::MARKDOWN,
            "image/png",
            "image/jpeg",
            mimes::TEXT
        ]),
        vec![
            "image/png",
            mimes::TEXT,
            mimes::MARKDOWN,
            "text/html",
            "application/json",
            "application/javascript",
            "image/svg+xml",
            "image/jpeg",
        ]
    );

    assert_eq!(
        MimeTypeDisplayOrder::new(&["application/json", "text/html", "text/html", mimes::MARKDOWN, "application/json"], None).sort([
            mimes::MARKDOWN,
            "application/json",
            mimes::TEXT,
            "application/javascript",
            "text/html",
            "image/svg+xml",
            "image/jpeg",
            "image/png"
        ]),
        vec![
            "application/json",
            "text/html",
            mimes::MARKDOWN,
            "application/javascript",
            "image/svg+xml",
            "image/png",
            "image/jpeg",
            mimes::TEXT
        ]
    );

    fixture.disposables.dispose();
}

#[test]
fn prioritizes_mimetypes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup();
    let mut m = MimeTypeDisplayOrder::new(&[mimes::MARKDOWN, "text/html", "application/json"], None);
    assert_eq!(m.to_array(), vec![mimes::MARKDOWN, "text/html", "application/json"]);

    // no-op if already in the right order
    m.prioritize("text/html", &["application/json"]);
    assert_eq!(m.to_array(), vec![mimes::MARKDOWN, "text/html", "application/json"]);

    // sorts to highest priority
    m.prioritize("text/html", &["application/json", mimes::MARKDOWN]);
    assert_eq!(m.to_array(), vec!["text/html", mimes::MARKDOWN, "application/json"]);

    // adds in new type
    m.prioritize("text/plain", &["application/json", mimes::MARKDOWN]);
    assert_eq!(m.to_array(), vec!["text/plain", "text/html", mimes::MARKDOWN, "application/json"]);

    // moves multiple, preserves order
    m.prioritize(mimes::MARKDOWN, &["text/plain", "application/json", mimes::MARKDOWN]);
    assert_eq!(m.to_array(), vec!["text/html", mimes::MARKDOWN, "text/plain", "application/json"]);

    // deletes multiple
    m.prioritize("text/plain", &["text/plain", "text/html", mimes::MARKDOWN]);
    assert_eq!(m.to_array(), vec!["text/plain", "text/html", mimes::MARKDOWN, "application/json"]);

    // handles multiple mimetypes, unknown mimetype
    let mut m2 = MimeTypeDisplayOrder::new(&["a", "b"], None);
    m2.prioritize("b", &["a", "b", "a", "q"]);
    assert_eq!(m2.to_array(), vec!["b", "a"]);

    fixture.disposables.dispose();
}

#[test]
fn prioritizes_mimetypes_with_10_plus_entries_numeric_index_sort() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup();
    // Regression for the case where `Array.from(uniqueIndices).sort()` did a
    // lexicographic sort on numeric indices, so `[2, 10]` became `[10, 2]`
    // and the reverse-splice loop removed the wrong entries.
    let mimes: Vec<String> = (0..12).map(|i| format!("type/{i}")).collect();
    let mut m = MimeTypeDisplayOrder::new(&mimes, None);
    assert_eq!(m.to_array(), mimes);

    m.prioritize("type/11", &["type/2", "type/10"]);
    assert_eq!(
        m.to_array(),
        vec!["type/0", "type/1", "type/3", "type/4", "type/5", "type/6", "type/7", "type/8", "type/9", "type/11", "type/2", "type/10"]
    );

    fixture.disposables.dispose();
}

#[test]
fn sort_mime_types_glob() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup();
    assert_eq!(
        MimeTypeDisplayOrder::new(&["application/vnd-vega*", mimes::MARKDOWN, "text/html", "application/json"], None).sort([
            "application/json",
            "application/javascript",
            "text/html",
            "application/vnd-plot.json",
            "application/vnd-vega.json"
        ]),
        vec!["application/vnd-vega.json", "text/html", "application/json", "application/vnd-plot.json", "application/javascript"],
        "glob *"
    );

    fixture.disposables.dispose();
}

#[test]
fn diff_cells() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup();
    let new_cell = |handle: i64, source: &str| {
        fixture.disposables.add_rc(Rc::new(TestCell::new("notebook", handle, source, "javascript", CellKind::Code, Vec::new(), fixture.language_service.clone())))
    };
    let mut cells: Vec<Rc<TestCell>> = Vec::new();

    for i in 0..5 {
        cells.push(new_cell(i, &format!("var a = {i};")));
    }

    let contains_cell = |cell: &Rc<TestCell>| cells.iter().any(|candidate| Rc::ptr_eq(candidate, cell));

    assert_eq!(diff(&cells, &[], contains_cell, None), vec![Splice { start: 0, delete_count: 5, to_insert: vec![] }]);

    assert_eq!(diff(&[], &cells, |_cell| false, None), vec![Splice { start: 0, delete_count: 0, to_insert: cells.clone() }]);

    let cell_a = new_cell(6, "var a = 6;");
    let cell_b = new_cell(7, "var a = 7;");

    let modified_cells = vec![cells[0].clone(), cells[1].clone(), cell_a.clone(), cells[3].clone(), cell_b.clone(), cells[4].clone()];

    let splices = diff(&cells, &modified_cells, contains_cell, None);

    assert_eq!(
        splices,
        vec![
            Splice { start: 2, delete_count: 1, to_insert: vec![cell_a] },
            Splice { start: 4, delete_count: 0, to_insert: vec![cell_b] },
        ]
    );

    fixture.disposables.dispose();
}

// Suíte `CellUri`

#[test]
fn parse_generate_file_scheme() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let nb = URI::parse("file:///bar/følder/file.nb", false);
    let id = 17;

    let data = CellUri::generate(&nb, id);
    let actual = CellUri::parse(&data);
    assert!(actual.is_some());
    let actual = actual.unwrap();
    assert_eq!(actual.handle, id);
    assert_eq!(actual.notebook.to_string(false), nb.to_string(false));
}

#[test]
fn parse_generate_foo_scheme() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let nb = URI::parse("foo:///bar/følder/file.nb", false);
    let id = 17;

    let data = CellUri::generate(&nb, id);
    let actual = CellUri::parse(&data);
    assert!(actual.is_some());
    let actual = actual.unwrap();
    assert_eq!(actual.handle, id);
    assert_eq!(actual.notebook.to_string(false), nb.to_string(false));
}

#[test]
fn stable_order() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let nb = URI::parse("foo:///bar/følder/file.nb", false);
    let handles: [i64; 8] = [1, 2, 9, 10, 88, 100, 666666, 7777777];

    // `Array#sort` sem comparador ordena pela conversão em texto (`String(uri)`, em unidades UTF-16).
    let mut uris: Vec<URI> = handles.iter().map(|&h| CellUri::generate(&nb, h)).collect();
    uris.sort_by_key(|uri| uri.to_string(false));

    let mut str_uris: Vec<_> = uris.iter().map(|uri| uri.to_string(false)).collect();
    str_uris.sort();
    let parsed_uris: Vec<URI> = str_uris.iter().map(|s| URI::parse(s.clone(), false)).collect();

    let actual: Vec<Option<i64>> = parsed_uris.iter().map(|u| CellUri::parse(u).map(|parsed| parsed.handle)).collect();

    assert_eq!(actual, handles.iter().map(|&h| Some(h)).collect::<Vec<_>>());
}

// Suíte `CellRange`

fn r(start: i64, end: i64) -> ICellRange {
    ICellRange { start, end }
}

#[test]
fn cell_range_to_index() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(cell_ranges_to_indexes(&[]), Vec::<i64>::new());
    assert_eq!(cell_ranges_to_indexes(&[r(0, 0)]), Vec::<i64>::new());
    assert_eq!(cell_ranges_to_indexes(&[r(0, 1)]), vec![0]);
    assert_eq!(cell_ranges_to_indexes(&[r(0, 2)]), vec![0, 1]);
    assert_eq!(cell_ranges_to_indexes(&[r(0, 2), r(2, 3)]), vec![0, 1, 2]);
    assert_eq!(cell_ranges_to_indexes(&[r(0, 2), r(3, 4)]), vec![0, 1, 3]);
}

#[test]
fn cell_index_to_range() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(cell_indexes_to_ranges(&[]), Vec::<ICellRange>::new());
    assert_eq!(cell_indexes_to_ranges(&[0]), vec![r(0, 1)]);
    assert_eq!(cell_indexes_to_ranges(&[0, 1]), vec![r(0, 2)]);
    assert_eq!(cell_indexes_to_ranges(&[0, 1, 2]), vec![r(0, 3)]);
    assert_eq!(cell_indexes_to_ranges(&[0, 1, 3]), vec![r(0, 2), r(3, 4)]);

    assert_eq!(cell_indexes_to_ranges(&[1, 0]), vec![r(0, 2)]);
    assert_eq!(cell_indexes_to_ranges(&[1, 2, 0]), vec![r(0, 3)]);
    assert_eq!(cell_indexes_to_ranges(&[3, 1, 0]), vec![r(0, 2), r(3, 4)]);

    assert_eq!(cell_indexes_to_ranges(&[9, 10]), vec![r(9, 11)]);
    assert_eq!(cell_indexes_to_ranges(&[10, 9]), vec![r(9, 11)]);
}

#[test]
fn reduce_ranges() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(reduce_cell_ranges(&[r(0, 1), r(1, 2)]), vec![r(0, 2)]);
    assert_eq!(reduce_cell_ranges(&[r(0, 2), r(1, 3)]), vec![r(0, 3)]);
    assert_eq!(reduce_cell_ranges(&[r(1, 3), r(0, 2)]), vec![r(0, 3)]);
    assert_eq!(reduce_cell_ranges(&[r(0, 2), r(4, 5)]), vec![r(0, 2), r(4, 5)]);

    assert_eq!(reduce_cell_ranges(&[r(0, 1), r(1, 2), r(4, 6)]), vec![r(0, 2), r(4, 6)]);

    assert_eq!(reduce_cell_ranges(&[r(0, 1), r(1, 3), r(3, 4)]), vec![r(0, 4)]);
}

#[test]
fn reduce_ranges_2_empty_ranges() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(reduce_cell_ranges(&[r(0, 0), r(0, 0)]), vec![r(0, 0)]);
    assert_eq!(reduce_cell_ranges(&[r(0, 0), r(1, 2)]), vec![r(1, 2)]);
    assert_eq!(reduce_cell_ranges(&[r(2, 2)]), vec![r(2, 2)]);
}

// Suíte `NotebookWorkingCopyTypeIdentifier`

#[test]
fn supports_notebook_type_only() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let view_type = "testViewType";
    let type_ = NotebookWorkingCopyTypeIdentifier::create(view_type, None);
    assert_eq!(
        NotebookWorkingCopyTypeIdentifier::parse(&type_),
        Some(NotebookWorkingCopyType { notebook_type: view_type.to_string(), view_type: view_type.to_string() })
    );
    assert_eq!(NotebookWorkingCopyTypeIdentifier::parse("something"), None);
}

#[test]
fn supports_different_viewtype() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let notebook_type = NotebookWorkingCopyType { notebook_type: "testNotebookType".to_string(), view_type: "testViewType".to_string() };
    let type_ = NotebookWorkingCopyTypeIdentifier::create(&notebook_type.notebook_type, Some(&notebook_type.view_type));
    assert_eq!(NotebookWorkingCopyTypeIdentifier::parse(&type_), Some(notebook_type));
    assert_eq!(NotebookWorkingCopyTypeIdentifier::parse("something"), None);
}
