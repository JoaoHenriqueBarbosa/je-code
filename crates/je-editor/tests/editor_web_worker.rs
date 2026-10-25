//! Porte de `src/vs/editor/test/common/services/editorWebWorker.test.ts` (suíte `EditorWebWorker`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `WorkerWithModels extends EditorWorker` vira o [`Fixture`]: o [`EditorWorker`] mais o `addModel`, que aceita o modelo
//!   (`$acceptNewModel`) e devolve a uri; o `ICommonModel` do `_getModel(uri)` é o `MirrorModel` que o
//!   [`EditorWorker::with_model`] entrega.
//! - Os `Promise` do `$computeMoreMinimalEdits` e do `$textualSuggest` já nascem resolvidos (o worker roda no mesmo
//!   thread), então o `.then(...)` é ler o `result()` da promessa.
//! - `Number.MAX_VALUE`, `Number.MIN_VALUE` e `Number.MAX_SAFE_INTEGER` viram `i32::MAX`, `0` e `i32::MAX`: o porte usa
//!   `i32` em linha e coluna e `i64` em deslocamento, e `Number.MIN_VALUE` (5e-324) vira 0 em `Math.floor`.
//! - A URI do modelo é `test:file#` mais um contador no lugar de `Date.now()`, que repetiria dentro do mesmo milissegundo.
//! - `deepStrictEqual` de `{ range, text }` é a comparação dos pares `(Range::to_string, text)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::sync::atomic::{AtomicUsize, Ordering};

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::JsString;
use je_base::js_regexp::JsRegExp;
use je_editor::code_action::languages::TextEdit;
use je_editor::diff::lines_diff_computer::LinesDiffComputerOptions;
use je_editor::editor_web_worker::{EditorWorker, RawModelData};
use je_editor::model::Eol;
use je_editor::position::Position;
use je_editor::range::Range;

struct Fixture {
    worker: EditorWorker,
    _guard: LeakGuard,
}

static MODEL_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn js(text: &str) -> JsString {
    JsString::from(text.to_string())
}

fn add_model_with_eol(worker: &EditorWorker, lines: &[&str], eol: Eol) -> String {
    let uri = format!("test:file#{}", MODEL_COUNTER.fetch_add(1, Ordering::SeqCst));
    worker.accept_new_model(RawModelData { url: uri.clone(), version_id: 1, lines: lines.iter().map(|line| js(line)).collect(), eol });
    uri
}

fn edit(range: Range, text: &str) -> TextEdit {
    TextEdit { range, text: text.to_string(), eol: None, insert_as_snippet: None, keep_whitespace: None }
}

impl Fixture {
    fn add_model(&self, lines: &[&str]) -> String {
        add_model_with_eol(&self.worker, lines, Eol::Lf)
    }

    fn offset_at(&self, uri: &str, line_number: i32, column: i32) -> usize {
        self.worker.with_model(uri, |model| model.offset_at(Position::new(line_number, column))).unwrap()
    }

    fn position_at(&self, uri: &str, offset: i64) -> Position {
        self.worker.with_model(uri, |model| model.position_at(offset)).unwrap()
    }

    fn value_in_range(&self, uri: &str, range: &Range) -> JsString {
        self.worker.with_model(uri, |model| model.get_value_in_range(range)).unwrap()
    }

    fn value(&self, uri: &str) -> JsString {
        self.worker.with_model(uri, |model| model.get_value()).unwrap()
    }

    fn compute_more_minimal_edits(&self, uri: &str, edits: Vec<TextEdit>) -> Vec<TextEdit> {
        self.worker.compute_more_minimal_edits(uri, edits, false).result().unwrap().unwrap()
    }
}

/// O `setup` da suíte: o guarda de vazamentos, o worker e o modelo de quatro linhas.
fn setup() -> (Fixture, String) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let worker = EditorWorker::new();
    let model = add_model_with_eol(
        &worker,
        &[
            "This is line one",              //16
            "and this is line number two",   //27
            "it is followed by #3",          //20
            "and finished with the fourth.", //29
        ],
        Eol::Lf,
    );
    (Fixture { worker, _guard: guard }, model)
}

fn assert_position_at(f: &Fixture, model: &str, offset: i64, line: i32, column: i32) {
    let position = f.position_at(model, offset);
    assert_eq!(position.line_number, line);
    assert_eq!(position.column, column);
}

fn assert_offset_at(f: &Fixture, model: &str, line_number: i32, column: i32, offset: usize) {
    assert_eq!(f.offset_at(model, line_number, column), offset);
}

#[test]
fn i_common_model_offset_at() {
    let (f, model) = setup();
    assert_offset_at(&f, &model, 1, 1, 0);
    assert_offset_at(&f, &model, 1, 2, 1);
    assert_offset_at(&f, &model, 1, 17, 16);
    assert_offset_at(&f, &model, 2, 1, 17);
    assert_offset_at(&f, &model, 2, 4, 20);
    assert_offset_at(&f, &model, 3, 1, 45);
    assert_offset_at(&f, &model, 5, 30, 95);
    assert_offset_at(&f, &model, 5, 31, 95);
    assert_offset_at(&f, &model, 5, i32::MAX, 95);
    assert_offset_at(&f, &model, 6, 30, 95);
    assert_offset_at(&f, &model, i32::MAX, 30, 95);
    assert_offset_at(&f, &model, i32::MAX, i32::MAX, 95);
}

#[test]
fn i_common_model_position_at() {
    let (f, model) = setup();
    assert_position_at(&f, &model, 0, 1, 1);
    assert_position_at(&f, &model, 0, 1, 1); // Number.MIN_VALUE
    assert_position_at(&f, &model, 1, 1, 2);
    assert_position_at(&f, &model, 16, 1, 17);
    assert_position_at(&f, &model, 17, 2, 1);
    assert_position_at(&f, &model, 20, 2, 4);
    assert_position_at(&f, &model, 45, 3, 1);
    assert_position_at(&f, &model, 95, 4, 30);
    assert_position_at(&f, &model, 96, 4, 30);
    assert_position_at(&f, &model, 99, 4, 30);
    assert_position_at(&f, &model, i64::MAX, 4, 30);
}

#[test]
fn i_common_model_validate_position_issue_15882() {
    let (f, _) = setup();
    let model = f.add_model(&["{\"id\": \"0001\",\"type\": \"donut\",\"name\": \"Cake\",\"image\":{\"url\": \"images/0001.jpg\",\"width\": 200,\"height\": 200},\"thumbnail\":{\"url\": \"images/thumbnails/0001.jpg\",\"width\": 32,\"height\": 32}}"]);
    assert_eq!(f.offset_at(&model, 1, 2), 1);
}

#[test]
fn more_minimal() {
    let (f, model) = setup();

    let edits = f.compute_more_minimal_edits(&model, vec![edit(Range::new(1, 1, 1, 17), "This is line One")]);
    assert_eq!(edits.len(), 1);
    let first = &edits[0];
    assert_eq!(first.text, "O");
    assert_eq!(first.range, Range { start_line_number: 1, start_column: 14, end_line_number: 1, end_column: 15 });
}

#[test]
fn more_minimal_merge_adjacent_edits() {
    let (f, _) = setup();

    let model = add_model_with_eol(&f.worker, &["one", "two", "three", "four", "five"], Eol::Lf);

    let new_edits = f.compute_more_minimal_edits(
        &model,
        vec![
            edit(Range::new(1, 1, 2, 1), "one\ntwo\nthree\n"),
            edit(Range::new(2, 1, 3, 1), ""),
            edit(Range::new(3, 1, 4, 1), ""),
            edit(Range::new(4, 2, 4, 3), "4"),
            edit(Range::new(5, 3, 5, 5), "5"),
        ],
    );

    assert_eq!(new_edits.len(), 2);
    assert_eq!(new_edits[0].text, "4");
    assert_eq!(new_edits[1].text, "5");
}

#[test]
fn more_minimal_issue_15385_newline_changes_only() {
    let (f, _) = setup();

    let model = add_model_with_eol(&f.worker, &["{", "\t\"a\":1", "}"], Eol::Lf);

    let edits = f.compute_more_minimal_edits(&model, vec![edit(Range::new(1, 1, 3, 2), "{\r\n\t\"a\":1\r\n}")]);
    assert_eq!(edits.len(), 0);
}

#[test]
fn more_minimal_issue_15385_newline_changes_and_other() {
    let (f, _) = setup();

    let model = add_model_with_eol(&f.worker, &["{", "\t\"a\":1", "}"], Eol::Lf);

    let edits = f.compute_more_minimal_edits(&model, vec![edit(Range::new(1, 1, 3, 2), "{\r\n\t\"b\":1\r\n}")]);
    assert_eq!(edits.len(), 1);
    let first = &edits[0];
    assert_eq!(first.text, "b");
    assert_eq!(first.range, Range { start_line_number: 2, start_column: 3, end_line_number: 2, end_column: 4 });
}

#[test]
fn more_minimal_issue_15385_newline_changes_and_other_2_of_2() {
    let (f, _) = setup();

    let model = f.add_model(&[
        "package main", // 1
        "func foo() {", // 2
        "}",            // 3
    ]);

    let edits = f.compute_more_minimal_edits(&model, vec![edit(Range::new(3, 2, 4, 1000), "\n")]);
    assert_eq!(edits.len(), 1);
    let first = &edits[0];
    assert_eq!(first.text, "\n");
    assert_eq!(first.range, Range { start_line_number: 3, start_column: 2, end_line_number: 3, end_column: 2 });
}

/// `applyEdits(text, edits)` do original: aplica as edições do fim para o começo, com o `PositionOffsetTransformer` do teste.
fn apply_edits(text: &[u16], edits: &[TextEdit]) -> Vec<u16> {
    let mut line_start_offset_by_line_idx: Vec<i64> = vec![0];
    for (i, unit) in text.iter().enumerate() {
        if *unit == u16::from(b'\n') {
            line_start_offset_by_line_idx.push(i as i64 + 1);
        }
    }
    line_start_offset_by_line_idx.push(text.len() as i64 + 1);

    let get_offset = |position: Position| -> usize {
        let max_line_offset = if position.line_number as usize >= line_start_offset_by_line_idx.len() {
            text.len() as i64
        } else {
            line_start_offset_by_line_idx[position.line_number as usize] - 1
        };
        (line_start_offset_by_line_idx[(position.line_number - 1) as usize] + i64::from(position.column) - 1).min(max_line_offset) as usize
    };

    let mut offset_edits: Vec<(usize, usize, Vec<u16>)> = edits
        .iter()
        .map(|e| (get_offset(e.range.get_start_position()), get_offset(e.range.get_end_position()), e.text.encode_utf16().collect()))
        .collect();

    offset_edits.sort_by(|a, b| b.0.cmp(&a.0));

    let mut text = text.to_vec();
    for (start_offset, end_offset, new_text) in offset_edits {
        let mut next = text[..start_offset].to_vec();
        next.extend_from_slice(&new_text);
        next.extend_from_slice(&text[end_offset..]);
        text = next;
    }

    text
}

/// `testEdits(lines, edits)`.
fn test_edits(f: &Fixture, lines: &[&str], edits: Vec<TextEdit>) -> Vec<(String, String)> {
    let model = f.add_model(lines);

    let options = LinesDiffComputerOptions { ignore_trim_whitespace: false, max_computation_time_ms: 0.0, compute_moves: false, extend_to_subwords: None };
    let smaller_edits = f.worker.compute_human_readable_diff(&model, edits.clone(), &options);

    let value = f.value(&model);
    let t1 = apply_edits(value.units(), &edits);
    let t2 = apply_edits(value.units(), &smaller_edits);
    assert_eq!(t1, t2);

    smaller_edits.iter().map(|e| (e.range.to_string(), e.text.clone())).collect()
}

fn expected(range: &str, text: &str) -> (String, String) {
    (range.to_string(), text.to_string())
}

#[test]
fn compute_human_readable_diff_1() {
    let (f, _) = setup();
    assert_eq!(
        test_edits(&f, &["function test() {}"], vec![edit(Range::new(1, 1, 1, 1), "\n/** Some Comment */\n")]),
        vec![expected("[1,1 -> 1,1]", "\n/** Some Comment */\n")]
    );
}

#[test]
fn compute_human_readable_diff_2() {
    let (f, _) = setup();
    assert_eq!(
        test_edits(&f, &["function test() {}"], vec![edit(Range::new(1, 1, 1, i32::MAX), "function test(myParam: number) { console.log(myParam); }")]),
        vec![expected("[1,15 -> 1,15]", "myParam: number"), expected("[1,18 -> 1,18]", " console.log(myParam); ")]
    );
}

#[test]
fn compute_human_readable_diff_3() {
    let (f, _) = setup();
    assert_eq!(
        test_edits(&f, &["", "", "", ""], vec![edit(Range::new(2, 1, 3, 20), "function test(myParam: number) { console.log(myParam); }\n\n")]),
        vec![expected("[2,1 -> 2,1]", "function test(myParam: number) { console.log(myParam); }\n")]
    );
}

#[test]
fn compute_human_readable_diff_4() {
    let (f, _) = setup();
    assert_eq!(
        test_edits(&f, &["function algorithm() {}"], vec![edit(Range::new(1, 1, 1, i32::MAX), "function alm() {}")]),
        vec![expected("[1,10 -> 1,19]", "alm")]
    );
}

#[test]
fn bug_overlapping_ranges_are_not_allowed_and_nothing_happens_with_inline_chat() {
    let (f, _) = setup();
    let original = "const API = require('../src/api');\n\ndescribe('API', () => {\n  let api;\n  let database;\n\n  beforeAll(() => {\n    database = {\n      getAllBooks: jest.fn(),\n      getBooksByAuthor: jest.fn(),\n      getBooksByTitle: jest.fn(),\n    };\n    api = new API(database);\n  });\n\n  describe('GET /books', () => {\n    it('should return all books', async () => {\n      const mockBooks = [{ title: 'Book 1' }, { title: 'Book 2' }];\n      database.getAllBooks.mockResolvedValue(mockBooks);\n\n      const req = {};\n      const res = {\n        json: jest.fn(),\n      };\n\n      await api.register({\n        get: (path, handler) => {\n          if (path === '/books') {\n            handler(req, res);\n          }\n        },\n      });\n\n      expect(database.getAllBooks).toHaveBeenCalled();\n      expect(res.json).toHaveBeenCalledWith(mockBooks);\n    });\n  });\n\n  describe('GET /books/author/:author', () => {\n    it('should return books by author', async () => {\n      const mockAuthor = 'John Doe';\n      const mockBooks = [{ title: 'Book 1', author: mockAuthor }, { title: 'Book 2', author: mockAuthor }];\n      database.getBooksByAuthor.mockResolvedValue(mockBooks);\n\n      const req = {\n        params: {\n          author: mockAuthor,\n        },\n      };\n      const res = {\n        json: jest.fn(),\n      };\n\n      await api.register({\n        get: (path, handler) => {\n          if (path === `/books/author/${mockAuthor}`) {\n            handler(req, res);\n          }\n        },\n      });\n\n      expect(database.getBooksByAuthor).toHaveBeenCalledWith(mockAuthor);\n      expect(res.json).toHaveBeenCalledWith(mockBooks);\n    });\n  });\n\n  describe('GET /books/title/:title', () => {\n    it('should return books by title', async () => {\n      const mockTitle = 'Book 1';\n      const mockBooks = [{ title: mockTitle, author: 'John Doe' }];\n      database.getBooksByTitle.mockResolvedValue(mockBooks);\n\n      const req = {\n        params: {\n          title: mockTitle,\n        },\n      };\n      const res = {\n        json: jest.fn(),\n      };\n\n      await api.register({\n        get: (path, handler) => {\n          if (path === `/books/title/${mockTitle}`) {\n            handler(req, res);\n          }\n        },\n      });\n\n      expect(database.getBooksByTitle).toHaveBeenCalledWith(mockTitle);\n      expect(res.json).toHaveBeenCalledWith(mockBooks);\n    });\n  });\n});\n";
    let modified = "const request = require('supertest');\nconst API = require('../src/api');\n\ndescribe('API', () => {\n  let api;\n  let database;\n\n  beforeAll(() => {\n    database = {\n      getAllBooks: jest.fn(),\n      getBooksByAuthor: jest.fn(),\n      getBooksByTitle: jest.fn(),\n    };\n    api = new API(database);\n  });\n\n  describe('GET /books', () => {\n    it('should return all books', async () => {\n      const mockBooks = [{ title: 'Book 1' }, { title: 'Book 2' }];\n      database.getAllBooks.mockResolvedValue(mockBooks);\n\n      const response = await request(api.app).get('/books');\n\n      expect(database.getAllBooks).toHaveBeenCalled();\n      expect(response.status).toBe(200);\n      expect(response.body).toEqual(mockBooks);\n    });\n  });\n\n  describe('GET /books/author/:author', () => {\n    it('should return books by author', async () => {\n      const mockAuthor = 'John Doe';\n      const mockBooks = [{ title: 'Book 1', author: mockAuthor }, { title: 'Book 2', author: mockAuthor }];\n      database.getBooksByAuthor.mockResolvedValue(mockBooks);\n\n      const response = await request(api.app).get(`/books/author/${mockAuthor}`);\n\n      expect(database.getBooksByAuthor).toHaveBeenCalledWith(mockAuthor);\n      expect(response.status).toBe(200);\n      expect(response.body).toEqual(mockBooks);\n    });\n  });\n\n  describe('GET /books/title/:title', () => {\n    it('should return books by title', async () => {\n      const mockTitle = 'Book 1';\n      const mockBooks = [{ title: mockTitle, author: 'John Doe' }];\n      database.getBooksByTitle.mockResolvedValue(mockBooks);\n\n      const response = await request(api.app).get(`/books/title/${mockTitle}`);\n\n      expect(database.getBooksByTitle).toHaveBeenCalledWith(mockTitle);\n      expect(response.status).toBe(200);\n      expect(response.body).toEqual(mockBooks);\n    });\n  });\n});\n";
    let lines: Vec<&str> = original.split('\n').collect();
    test_edits(&f, &lines, vec![edit(Range { start_line_number: 1, start_column: 1, end_line_number: 96, end_column: 1 }, modified)]);
}

#[test]
fn i_common_model_get_value_in_range_issue_17424() {
    let (f, _) = setup();

    let model = f.add_model(&[
        "package main", // 1
        "func foo() {", // 2
        "}",            // 3
    ]);

    let value = f.value_in_range(&model, &Range { start_line_number: 3, start_column: 1, end_line_number: 4, end_column: 1 });
    assert_eq!(value, js("}"));
}

#[test]
fn textual_suggest_issue_17785() {
    let (f, _) = setup();

    let model = f.add_model(&[
        "foobar", // 1
        "f f",    // 2
    ]);

    let leading_word: Vec<u16> = "f".encode_utf16().collect();
    let word_def: Vec<u16> = "[a-z]+".encode_utf16().collect();
    let result = f.worker.textual_suggest(&[model], Some(&leading_word), &word_def, "img").result().unwrap().unwrap();
    let Some(result) = result else {
        panic!("o resultado do textualSuggest não pode ser nulo");
    };
    assert_eq!(result.words.len(), 1);
    assert!(result.duration.is_finite());
    assert_eq!(result.words[0], js("foobar"));
}

#[test]
fn get_words_via_iterator_issue_46930() {
    let (f, _) = setup();

    let model = f.add_model(&[
        "one line", // 1
        "two line", // 2
        "",
        "past empty",
        "single",
        "",
        "and now we are done",
    ]);

    let word_def = JsRegExp::from_str("[a-z]+", "img").unwrap();
    let words: Vec<String> = f.worker.with_model(&model, |m| m.words(&word_def).map(|word| word.to_string_lossy()).collect()).unwrap();

    assert_eq!(words, ["one", "line", "two", "line", "past", "empty", "single", "and", "now", "we", "are", "done"]);
}
