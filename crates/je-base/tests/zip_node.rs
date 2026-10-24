//! Porte de `src/vs/base/test/node/zip/zip.test.ts` (suíte `Zip`), caso a caso e na mesma ordem.
//!
//! Notas de tradução: o `getRandomTestPath(tmpdir(), 'vsctests', 'zip')` é um diretório aleatório no `tmpdir` e o
//! `Promises.rm(testDir)` é o `Drop` do `TestDir` (que também roda se o teste falhar). O `createCancelablePromise(token =>
//! extract(...))` é o `extract` com o token de uma fonte de cancelamento. As funções do original são bloqueantes aqui
//! (ver `je_base::node::zip`), então `assert.rejects` é o `Err`. As fixtures são lidas de `ref/vscode`.

mod common;

use std::path::{Path, PathBuf};
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationTokenSource;
use je_base::errors::is_cancellation_error;
use je_base::node::zip::{ExtractOptions, ZipInput, ZipOptions, ZipValidationOptions, buffer, extract, validate_zip, zip};
use je_base::uuid::generate_uuid;
use je_base::zip::{ZipFile, create_zip};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ref/vscode/src/vs/base/test/node/zip/fixtures");

struct TestDir(PathBuf);

impl TestDir {
    /// Só o caminho: cada teste cria o diretório, como no original.
    fn new(suite: &str) -> Self {
        Self(std::env::temp_dir().join("vsctests").join(suite).join(generate_uuid()))
    }

    fn create(&self) {
        std::fs::create_dir_all(&self.0).unwrap();
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn text(path: &Path, entry: &str) -> String {
    String::from_utf8(buffer(path, entry).unwrap()).unwrap()
}

#[test]
fn extracts_browser_created_zip_archives_without_changing_binary_contents_or_filenames() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let test_dir = TestDir::new("zip");
    test_dir.create();
    let files = [
        ("generated-image-1.png", VSBuffer::wrap((0..=255u8).collect())),
        ("images/\u{e9}.jpeg", VSBuffer::from_string("original image bytes")),
        ("empty.webp", VSBuffer::alloc(0)),
    ];
    let archive = test_dir.join("images.zip");
    let output = test_dir.join("extracted");
    let zip_files: Vec<ZipFile> = files.iter().map(|(path, contents)| ZipFile { path: path.to_string(), contents: contents.clone() }).collect();
    std::fs::write(&archive, create_zip(&zip_files).unwrap().to_vec()).unwrap();
    let source = CancellationTokenSource::new(None);
    extract(&archive, &output, &ExtractOptions::default(), &source.token()).unwrap();
    let extracted: Vec<(String, Vec<u8>)> = files.iter().map(|(path, _)| (path.to_string(), std::fs::read(output.join(path)).unwrap())).collect();
    let expected: Vec<(String, Vec<u8>)> = files.iter().map(|(path, contents)| (path.to_string(), contents.to_vec())).collect();
    assert_eq!(extracted, expected);
}

#[test]
fn stores_a_nested_archive_without_recompressing_it_and_reports_source_bytes() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let test_dir = TestDir::new("zip");
    test_dir.create();
    let host_zip = test_dir.join("host.zip");
    let export_zip = test_dir.join("export.zip");
    zip(&host_zip, &[ZipInput::from_contents("events.jsonl", "event\n".repeat(100_000))], ZipOptions::default()).unwrap();
    let source = std::fs::read(&host_zip).unwrap();
    let mut progress: Vec<u64> = Vec::new();
    let mut on_progress = |bytes: u64, _: &str| progress.push(bytes);
    zip(
        &export_zip,
        &[ZipInput {
            local_path_size: Some(source.len() as u64),
            compress: Some(false),
            ..ZipInput::from_local("host-logs.zip", &host_zip)
        }],
        ZipOptions { token: None, on_progress: Some(&mut on_progress) },
    )
    .unwrap();
    let exported = std::fs::read(&export_zip).unwrap();
    let compression_method = u16::from_le_bytes([exported[8], exported[9]]);
    assert_eq!(compression_method, 0);
    assert_eq!(progress.last().copied(), Some(source.len() as u64));
    assert!(buffer(&export_zip, "host-logs.zip").unwrap() == source, "host archive unchanged");
    assert!(source.len() < 600_000 / 10, "host compressed");
}

#[test]
fn cancels_zip_creation_while_reading_a_source() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let test_dir = TestDir::new("zip");
    test_dir.create();
    let cancellation = store.add_rc(Rc::new(CancellationTokenSource::new(None)));
    let source = test_dir.join("source.log");
    std::fs::write(&source, vec![0u8; 1024 * 1024]).unwrap();
    let mut on_progress = |_: u64, _: &str| cancellation.cancel();
    let error = zip(
        &test_dir.join("export.zip"),
        &[ZipInput::from_local("source.log", &source)],
        ZipOptions { token: Some(cancellation.token()), on_progress: Some(&mut on_progress) },
    )
    .unwrap_err();
    assert!(is_cancellation_error(&error));
}

#[test]
fn extract_should_handle_directories() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let test_dir = TestDir::new("zip");
    test_dir.create();

    let fixture = Path::new(FIXTURES).join("extract.zip");

    let source = CancellationTokenSource::new(None);
    extract(&fixture, &test_dir.0, &ExtractOptions::default(), &source.token()).unwrap();
    assert!(test_dir.join("extension").exists());
}

#[test]
fn zip_should_stream_a_fixed_prefix_of_a_local_file() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let test_dir = TestDir::new("zip");
    let source_path = test_dir.join("source.txt");
    let zip_path = test_dir.join("logs.zip");
    test_dir.create();
    std::fs::write(&source_path, "snapshot-appended").unwrap();
    let input = ZipInput { local_path_size: Some(8), ..ZipInput::from_local("source.txt", &source_path) };
    zip(&zip_path, &[input], ZipOptions::default()).unwrap();

    assert_eq!(text(&zip_path, "source.txt"), "snapshot");
}

#[test]
fn zip_should_clamp_a_prefix_larger_than_the_current_file_size() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let test_dir = TestDir::new("zip");
    let source_path = test_dir.join("source.txt");
    let zip_path = test_dir.join("logs.zip");
    test_dir.create();
    std::fs::write(&source_path, "shrunk").unwrap();
    // Request more bytes than the file holds: the entry contains the whole file.
    let input = ZipInput { local_path_size: Some(1024), ..ZipInput::from_local("source.txt", &source_path) };
    zip(&zip_path, &[input], ZipOptions::default()).unwrap();

    assert_eq!(text(&zip_path, "source.txt"), "shrunk");
}

#[test]
fn zip_should_write_an_empty_entry_for_a_zero_length_prefix() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let test_dir = TestDir::new("zip");
    let source_path = test_dir.join("source.txt");
    let zip_path = test_dir.join("logs.zip");
    test_dir.create();
    std::fs::write(&source_path, "ignored").unwrap();
    let input = ZipInput { local_path_size: Some(0), ..ZipInput::from_local("source.txt", &source_path) };
    zip(&zip_path, &[input], ZipOptions::default()).unwrap();

    assert_eq!(text(&zip_path, "source.txt"), "");
}

#[test]
fn zip_should_skip_an_unavailable_best_effort_bounded_source_without_failing_the_archive() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let test_dir = TestDir::new("zip");
    let present_path = test_dir.join("present.txt");
    let missing_path = test_dir.join("missing.txt");
    let zip_path = test_dir.join("logs.zip");
    test_dir.create();
    std::fs::write(&present_path, "present-contents").unwrap();
    zip(
        &zip_path,
        &[
            ZipInput { local_path_size: Some(8), skip_source_errors: true, ..ZipInput::from_local("missing.txt", &missing_path) },
            ZipInput { local_path_size: Some(7), ..ZipInput::from_local("present.txt", &present_path) },
        ],
        ZipOptions::default(),
    )
    .unwrap();

    assert_eq!(text(&zip_path, "present.txt"), "present");
    assert!(buffer(&zip_path, "missing.txt").is_err());
}

#[test]
fn zip_should_skip_an_unavailable_best_effort_source_without_a_size_limit() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let test_dir = TestDir::new("zip");
    let present_path = test_dir.join("present.txt");
    let missing_path = test_dir.join("missing.txt");
    let zip_path = test_dir.join("logs.zip");
    test_dir.create();
    std::fs::write(&present_path, "present-contents").unwrap();
    zip(
        &zip_path,
        &[
            ZipInput { skip_source_errors: true, ..ZipInput::from_local("missing.txt", &missing_path) },
            ZipInput::from_local("present.txt", &present_path),
        ],
        ZipOptions::default(),
    )
    .unwrap();

    assert_eq!(text(&zip_path, "present.txt"), "present-contents");
    assert!(buffer(&zip_path, "missing.txt").is_err());
}

#[test]
fn validate_zip_enforces_entry_and_expanded_size_limits() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let test_dir = TestDir::new("zip-validation");
    let zip_path = test_dir.join("logs.zip");
    test_dir.create();
    zip(&zip_path, &[ZipInput::from_contents("one.txt", "1234"), ZipInput::from_contents("two.txt", "5678")], ZipOptions::default()).unwrap();

    validate_zip(&zip_path, &ZipValidationOptions { max_entries: 10, max_uncompressed_size: None }).unwrap();
    let too_many = validate_zip(&zip_path, &ZipValidationOptions { max_entries: 1, max_uncompressed_size: Some(100) }).unwrap_err();
    assert!(too_many.message.contains("too many entries"), "{}", too_many.message);
    let too_big = validate_zip(&zip_path, &ZipValidationOptions { max_entries: 10, max_uncompressed_size: Some(7) }).unwrap_err();
    assert!(too_big.message.contains("expands beyond the allowed size"), "{}", too_big.message);
}
