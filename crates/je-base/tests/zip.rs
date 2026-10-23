//! Porte de `src/vs/base/test/common/zip.test.ts` (suíte `createZip`), caso a caso e na mesma ordem.
//!
//! Notas de tradução: `assert.throws(fn, /texto/)` é `create_zip(...)` devolvendo `Err` cuja mensagem contém o
//! texto. O `for (const path of [...]) test(...)` do original gera um teste por caminho, e aqui é um teste
//! (`rejects_unsafe_path`) que percorre os mesmos nove caminhos. `readUInt16LE` e `readUInt32LE` leem os
//! bytes do arquivo.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::buffer::{VSBuffer, read_u_int16_le};
use je_base::zip::{ZipFile, create_zip};

fn file(path: &str, contents: VSBuffer) -> ZipFile {
    ZipFile { path: path.to_owned(), contents }
}

fn u16_at(archive: &VSBuffer, offset: usize) -> u32 {
    archive.with_bytes(|b| u32::from(read_u_int16_le(b, offset)))
}

fn u32_at(archive: &VSBuffer, offset: usize) -> u32 {
    archive.read_u_int32_le(offset) as u32
}

fn error_of(files: &[ZipFile]) -> String {
    create_zip(files).expect_err("devia lançar").to_string()
}

#[test]
fn creates_an_empty_archive() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(create_zip(&[]).unwrap().to_vec(), vec![0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn writes_stored_entries_with_utf8_names_checksums_and_matching_directory_offsets() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "images/\u{e9}.png";
    let name_length = VSBuffer::from_string(name).byte_length();
    let contents = VSBuffer::from_string("_123456789_").slice(Some(1), Some(10));
    let archive = create_zip(&[file(name, contents.clone()), file("empty.png", VSBuffer::alloc(0))]).unwrap();
    let second_offset = 30 + name_length + contents.byte_length();
    let central_offset = second_offset + 30 + "empty.png".len();
    let second_central_offset = central_offset + 46 + name_length;
    let end = archive.slice(Some(-22), None);

    assert_eq!(u32_at(&archive, 0), 0x04034b50);
    assert_eq!(u16_at(&archive, 6), 0x0800);
    assert_eq!(u16_at(&archive, 8), 0);
    assert_eq!(u32_at(&archive, 14), 0xcbf43926);
    assert_eq!(u32_at(&archive, 18), 9);
    assert_eq!(u32_at(&archive, 22), 9);
    assert_eq!(archive.slice(Some(30), Some((30 + name_length) as i64)).to_vec(), name.as_bytes());
    assert_eq!(archive.slice(Some((30 + name_length) as i64), Some(second_offset as i64)).to_vec(), b"123456789");
    assert_eq!(u32_at(&archive, second_offset), 0x04034b50);
    assert_eq!(u32_at(&archive, second_offset + 14), 0);
    assert_eq!(u32_at(&archive, central_offset), 0x02014b50);
    assert_eq!(
        archive.slice(Some((central_offset + 6) as i64), Some((central_offset + 30) as i64)).to_vec(),
        archive.slice(Some(4), Some(28)).to_vec()
    );
    assert_eq!(u32_at(&archive, central_offset + 42), 0);
    assert_eq!(u32_at(&archive, second_central_offset + 42) as usize, second_offset);
    assert_eq!(u32_at(&end, 0), 0x06054b50);
    assert_eq!(u16_at(&end, 8), 2);
    assert_eq!(u16_at(&end, 10), 2);
    assert_eq!(u32_at(&end, 12) as usize, 2 * 46 + name_length + "empty.png".len());
    assert_eq!(u32_at(&end, 16) as usize, central_offset);
}

#[test]
fn rejects_unsafe_path() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    for path in ["", "/absolute.png", "../outside.png", "images/../outside.png", "./image.png", "C:/image.png", "images\\image.png", "images//image.png", "image\0.png"] {
        let message = error_of(&[file(path, VSBuffer::alloc(0))]);
        assert!(message.contains("relative file paths"), "{path:?}: {message}");
    }
}

#[test]
fn rejects_duplicate_paths() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let message = error_of(&[file("image.png", VSBuffer::alloc(0)), file("image.png", VSBuffer::alloc(0))]);
    assert!(message.contains("unique paths"), "{message}");
}

#[test]
fn checks_the_encoded_filename_length() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let message = error_of(&[file(&"\u{e9}".repeat(0x8000), VSBuffer::alloc(0))]);
    assert!(message.contains("path is too long"), "{message}");
}

#[test]
fn rejects_entry_counts_requiring_zip64() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let files: Vec<ZipFile> = (0..0xffff).map(|_| file("image.png", VSBuffer::alloc(0))).collect();
    let message = error_of(&files);
    assert!(message.contains("Too many files"), "{message}");
}
