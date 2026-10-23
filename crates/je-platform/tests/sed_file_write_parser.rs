//! Porte de `src/vs/platform/terminal/test/common/autoApprove/sedFileWriteParser.test.ts` (suíte `SedFileWriteParser`), caso a caso e na mesma ordem.
//!
//! Divergências: o `assert.deepStrictEqual` do objeto `{ single, multiple, bsd }` é a comparação de uma tupla
//! com o mesmo conteúdo, na mesma ordem dos campos.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_platform::terminal::auto_approve::sed_file_write_parser::SedFileWriteParser;

#[test]
fn detects_supported_in_place_options() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let parser = SedFileWriteParser::new();
    let command_lines = [
        "sed -i \"s/foo/bar/\" file.txt",
        "sed -I \"s/foo/bar/\" file.txt",
        "sed -ni \"s/foo/bar/\" file.txt",
        "sed -i.bak \"s/foo/bar/\" file.txt",
        "sed -i '' \"s/foo/bar/\" file.txt",
        "sed --in-place \"s/foo/bar/\" file.txt",
        "sed --in-place=.bak \"s/foo/bar/\" file.txt",
    ];
    assert_eq!(
        command_lines.iter().map(|line| parser.can_handle(line)).collect::<Vec<_>>(),
        command_lines.iter().map(|_| true).collect::<Vec<_>>()
    );
}

#[test]
fn does_not_classify_non_in_place_commands() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let parser = SedFileWriteParser::new();
    let command_lines = ["sed \"s/foo/bar/\" file.txt", "sed -n \"s/foo/bar/p\" file.txt", "echo sed -i file.txt"];
    assert_eq!(
        command_lines.iter().map(|line| parser.can_handle(line)).collect::<Vec<_>>(),
        command_lines.iter().map(|_| false).collect::<Vec<_>>()
    );
}

#[test]
fn extracts_in_place_file_targets() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let parser = SedFileWriteParser::new();
    assert_eq!(
        (
            parser.extract_file_writes("sed -i \"s/foo/bar/\" file.txt"),
            parser.extract_file_writes("sed -i \"s/foo/bar/\" file1.txt file2.txt"),
            parser.extract_file_writes("sed -i '' \"s/foo/bar/\" file.txt"),
        ),
        (vec!["file.txt".to_owned()], vec!["file1.txt".to_owned(), "file2.txt".to_owned()], vec!["file.txt".to_owned()])
    );
}
