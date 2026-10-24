//! Porte de `src/vs/workbench/services/textfile/test/common/textFileService.io.test.ts` (suíte `Files - BrowserTextFileService i/o`,
//! que é a função `createSuite` chamada por `src/vs/workbench/services/textfile/test/browser/browserTextFileService.io.test.ts`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`createSuite(params)` e o arranjo do navegador.** A função `createSuite` recebe `setup`, `teardown`, `exists`,
//!   `stat`, `readFile` e `detectEncodingByBOM`; o arranjo do `browserTextFileService.io.test.ts` os fornece sobre um
//!   `TestInMemoryFileSystemProvider`. O porte junta os dois: [`Suite`] guarda o serviço, o provedor e o diretório, e traz
//!   [`Suite::exists`], [`Suite::stat`], [`Suite::read_file`] e [`Suite::detect_encoding_by_bom`] com a implementação do
//!   navegador (leitura direta do provedor). `readFile(path, encoding)` só é chamada com `utf8` na suíte, então
//!   [`Suite::read_file_as_utf8`] é a sobrecarga com codificação (o `TextDecoder('utf8')`).
//! - **`isWeb`.** O original só roda esta suíte quando `isWeb` é verdadeiro (a suíte nativa cobre o outro ambiente); o porte
//!   roda sempre, porque é o arranjo do navegador que o porte implementa.
//! - **`setup` e `teardown` do arranjo.** `workbenchInstantiationService`, o `FileService` com o provedor registrado para o
//!   esquema `file`, o `WorkingCopyFileService` (com `WorkingCopyService` e `UriIdentityService` próprios), o
//!   `createChild(collection)` e o `TestBrowserTextFileServiceWithEncodingOverrides` estão em
//!   `tests/support/text_file_support.rs` ([`browser_io_child_services`], [`TextFileTestAccessor::with_child_services`] e
//!   `TestTextFileService::with_encoding_overrides`). O `teardown` (`disposables.clear()`) é o descarte do store do guarda
//!   de vazamentos ([`LeakGuard`]), que roda ao sair do caso e antes de conferir os vazamentos. Os casos rodam em
//!   `run_with_faked_timers` (o `readFileStream` do provedor usa `timeout(0)`); `await` é `.await.unwrap()`.
//! - **Fixtures.** O `files.ts` (`src/vs/workbench/services/textfile/test/common/fixtures/files.ts`) é lido de `ref/vscode`
//!   em tempo de teste, pelo caminho relativo ao `CARGO_MANIFEST_DIR` (`ref/` fica fora do git): os `Uint8Array.from([...])`
//!   literais são lidos do próprio arquivo e o texto `lorem` (`head` e `tail`) também; as fixtures calculadas
//!   (`some_cyrillic.txt`, `lorem*.txt`) são reconstruídas com a mesma regra do arquivo. A suíte não lê os arquivos de
//!   `test/node/encoding/fixtures`, só este módulo.
//! - **`createTextModel(text)`** e `textModel.createSnapshot(preserveBOM)` são o [`create_text_model`] do `testTextModel` e o
//!   `create_snapshot` do modelo; o modelo é descartado pelo store do guarda (`disposables.add`).
//! - **`resolved.value.create(eol).textBuffer`** é a fábrica retirada de [`TextBufferFactoryRef`] e consumida por `create`
//!   (que devolve o buffer), descartado pelo store do guarda; `snapshotToString(textBuffer.createSnapshot(false))` é
//!   [`buffer_text`]. `isWindows ? CRLF : LF` é `cfg!(windows)`.
//! - **`catch` de `service.readStream(...)`** é o `Result` da promessa; `error.textFileOperationResult` é
//!   `text_file_operation_result_of`.
//! - **`contents.indexOf(needle, from)`** conta em unidades UTF-16, como o JavaScript ([`index_of`]).
//! - **`btoa(...)`** da cadeia Latin-1 do caso `read - encoding picked up (base64)` é o Base64 dos bytes Latin-1 dela.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;
#[allow(dead_code)]
#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use std::future::Future;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::buffer::{VSBuffer, buffer_to_stream, encode_base64};
use je_base::path::PathModule;
use je_base::uri::URI;
use je_editor::model::{DefaultEndOfLine, TextSnapshot};
use je_editor::piece_tree_text_buffer::PieceTreeTextBuffer;
use je_platform::files::files::{FileSystemProvider, FileWriteOptions};
use je_platform::resources::{fs_path, host_environment, uri_file};
use je_workbench::services::textfile::encoding::{UTF8, UTF8_BOM, UTF8_WITH_BOM, UTF16BE, UTF16BE_BOM, UTF16LE, UTF16LE_BOM, detect_encoding_by_bom_from_buffer};
use je_workbench::services::textfile::text_buffer_factory::create_text_buffer_factory_from_stream;
use je_workbench::services::textfile::textfiles::{
    ITextFileService, ReadTextFileEncodingOptions, ReadTextFileOptions, TextBufferFactoryRef, TextFileCreateOperation, TextFileOperationResult, TextOrSnapshot, WriteTextFileOptions,
    snapshot_to_string, string_to_snapshot, text_file_operation_result_of,
};
use support::text_file_support::{TestInMemoryFileSystemProvider, TextFileTestAccessor, browser_io_child_services};
use test_text_model::create_text_model;

//#region Fixtures (files.ts)

/// O `files.ts` do oráculo, em `ref/vscode` (fora do git).
const FILES_TS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ref/vscode/src/vs/workbench/services/textfile/test/common/fixtures/files.ts");

/// `range(start, end)`: de `start` até `end - 1`.
fn range(start: u8, end: u16) -> impl Iterator<Item = u8> {
    (u16::from(start)..end).map(|value| value as u8)
}

/// O texto entre o `marker` (que termina em crase) e a crase de fechamento: o conteúdo do template literal.
fn template_literal_after<'a>(source: &'a str, marker: &str) -> &'a str {
    let start = source.find(marker).unwrap_or_else(|| panic!("`{marker}` não está no files.ts")) + marker.len();
    let end = source[start..].find('`').expect("o template literal do lorem fecha") + start;

    &source[start..end]
}

/// Os `fixtures['nome'] = Uint8Array.from([n, n, ...])` literais do `files.ts`, na ordem do arquivo.
fn literal_fixtures(source: &str) -> Vec<(String, Vec<u8>)> {
    let mut fixtures = Vec::new();

    for (index, _) in source.match_indices("fixtures['") {
        let rest = &source[index + "fixtures['".len()..];
        let Some(name_end) = rest.find("']") else { continue };
        let Some(body) = rest[name_end + 2..].strip_prefix(" = Uint8Array.from([") else { continue };
        let body = &body[..body.find(']').expect("o literal do Uint8Array fecha")];

        // as entradas com `...range(...)` não são literais: o `some_cyrillic.txt` é calculado à parte
        let bytes: Option<Vec<u8>> = body.split(',').map(str::trim).filter(|token| !token.is_empty()).map(|token| token.parse().ok()).collect();
        if let Some(bytes) = bytes {
            fixtures.push((rest[..name_end].to_string(), bytes));
        }
    }

    fixtures
}

/// `getTestData(needle)`: o `needle`, um espaço, a cabeça do lorem, o `needle`, um espaço e a cauda.
fn test_data(needle: &[u8], head: &str, tail: &str) -> Vec<u8> {
    let space: &[u8] = b" ";

    [needle, space, head.as_bytes(), needle, space, tail.as_bytes()].concat()
}

/// `fixtures` de `files.ts`: nome do arquivo e conteúdo.
fn fixtures() -> Vec<(String, Vec<u8>)> {
    let source = std::fs::read_to_string(FILES_TS).unwrap_or_else(|error| panic!("o oráculo precisa estar em ref/vscode ({FILES_TS}): {error}"));
    let head = template_literal_after(&source, "head: VSBuffer.fromString(`");
    let tail = template_literal_after(&source, "tail: VSBuffer.fromString(`");

    let cyrillic: Vec<u8> = range(128, 175 + 1).chain(range(224, 239 + 1)).collect();
    let lorem = test_data(&[246, 228, 252, 223], head, tail);

    let mut fixtures = literal_fixtures(&source);
    fixtures.extend([
        ("some_cyrillic.txt".to_string(), cyrillic.clone()),
        ("lorem_cp866.txt".to_string(), test_data(&cyrillic, head, tail)),
        ("lorem_cp1252.txt".to_string(), test_data(&[246, 228, 252, 223], head, tail)),
        ("lorem_big5.txt".to_string(), test_data(&[164, 164, 164, 229, 97, 98, 99], head, tail)),
        ("lorem_shiftjis.txt".to_string(), test_data(&[146, 134, 149, 182, 97, 98, 99], head, tail)),
        ("lorem_gbk.txt".to_string(), test_data(&[214, 208, 185, 250, 97, 98, 99], head, tail)),
        ("lorem.txt".to_string(), lorem.clone()),
        ("lorem_utf8bom.txt".to_string(), [&UTF8_BOM[..], test_data(&[195, 182, 195, 164, 195, 188, 195, 159], head, tail).as_slice()].concat()),
        ("lorem_utf16be.txt".to_string(), lorem.iter().flat_map(|byte| [0, *byte]).collect()),
        ("lorem_utf16le.txt".to_string(), lorem.iter().flat_map(|byte| [*byte, 0]).collect()),
    ]);

    fixtures
}

//#endregion

//#region Arranjo da suíte

/// `const testDir = 'test'`.
const TEST_DIR: &str = "test";

/// O que `createSuite(params)` recebe do arranjo do navegador, mais o guarda de vazamentos (o `disposables` da função).
struct Suite {
    guard: Rc<LeakGuard>,
    service: Rc<dyn ITextFileService>,
    file_provider: Rc<TestInMemoryFileSystemProvider>,
    _accessor: TextFileTestAccessor,
}

impl Suite {
    /// O `setup` do arranjo do navegador: o serviço, o provedor em memória e os arquivos de `files.ts` em `testDir`.
    async fn setup(guard: Rc<LeakGuard>) -> Suite {
        let (child, file_provider) = browser_io_child_services(&guard.store());
        let accessor = TextFileTestAccessor::with_child_services(&guard.store(), child);
        guard.add_rc(accessor.files());
        let service: Rc<dyn ITextFileService> = accessor.text_file_service.clone();

        file_provider.mkdir(&uri_file(TEST_DIR)).await.unwrap();
        for (file_name, content) in fixtures() {
            file_provider
                .write_file(&uri_file(&join(&[TEST_DIR, &file_name])), VSBuffer::wrap(content), &FileWriteOptions { create: true, overwrite: false, unlock: false, ..FileWriteOptions::default() })
                .await
                .unwrap();
        }

        Suite { guard, service, file_provider, _accessor: accessor }
    }

    /// `URI.file(join(testDir, name))`.
    fn resource(&self, name: &str) -> URI {
        uri_file(&join(&[TEST_DIR, name]))
    }

    /// `exists(fsPath)`.
    async fn exists(&self, fs_path: &str) -> bool {
        self.file_provider.read_file(&uri_file(fs_path), false).await.is_ok()
    }

    /// `stat(fsPath)`: o tamanho.
    async fn stat(&self, fs_path: &str) -> u64 {
        self.file_provider.stat(&uri_file(fs_path)).await.unwrap().size
    }

    /// `readFile(fsPath)`.
    async fn read_file(&self, fs_path: &str) -> VSBuffer {
        self.file_provider.read_file(&uri_file(fs_path), false).await.unwrap()
    }

    /// `readFile(fsPath, 'utf8')`.
    async fn read_file_as_utf8(&self, fs_path: &str) -> String {
        String::from_utf8_lossy(&self.read_file(fs_path).await.to_vec()).into_owned()
    }

    /// `detectEncodingByBOM(fsPath)`: `null` quando o arquivo não existe (o erro é ignorado).
    async fn detect_encoding_by_bom(&self, fs_path: &str) -> Option<&'static str> {
        match self.file_provider.read_file(&uri_file(fs_path), false).await {
            Ok(buffer) => detect_encoding_by_bom_from_buffer(Some(&buffer.slice(Some(0), Some(3))), 3),
            Err(_) => None,
        }
    }

    /// `createTextModelSnapshot(text, preserveBOM)`.
    fn create_text_model_snapshot(&self, text: &str, preserve_bom: Option<bool>) -> Box<dyn TextSnapshot> {
        let text_model = self.guard.add_rc(create_text_model(text));

        text_model.create_snapshot(preserve_bom.unwrap_or(false))
    }

    /// `disposables.add(factory.create(eol).textBuffer)`.
    fn create_text_buffer(&self, factory: &TextBufferFactoryRef, default_eol: DefaultEndOfLine) -> Rc<PieceTreeTextBuffer> {
        self.guard.add_rc(Rc::new(factory.take().expect("a fábrica ainda não foi consumida").create(default_eol)))
    }

    /// `testEncoding(resource, encoding, content, expectedContent)`.
    async fn test_encoding(&self, resource: URI, encoding: &str, content: TextOrSnapshot, expected_content: &str) {
        self.service.write(&resource, content, write_options(encoding)).await.unwrap();

        let detected_encoding = self.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(encoding));

        let resolved = self.service.read_stream(&resource, None).await.unwrap();
        assert_eq!(resolved.base.encoding, encoding);

        let text_buffer = self.create_text_buffer(&resolved.value, platform_eol());
        assert_eq!(buffer_text(&text_buffer), expected_content);
    }

    /// `testEncodingKeepsData(resource, encoding, expected)`.
    async fn test_encoding_keeps_data(&self, resource: URI, encoding: &str, expected: &str) {
        let mut resolved = self.service.read_stream(&resource, read_options(encoding)).await.unwrap();
        let text_buffer = self.create_text_buffer(&resolved.value, platform_eol());
        let content = buffer_text(&text_buffer);
        assert_eq!(content, expected);

        self.service.write(&resource, TextOrSnapshot::from(content.as_str()), write_options(encoding)).await.unwrap();

        resolved = self.service.read_stream(&resource, read_options(encoding)).await.unwrap();
        let text_buffer2 = self.create_text_buffer(&resolved.value, DefaultEndOfLine::CrLf);
        assert_eq!(buffer_text(&text_buffer2), content);

        self.service.write(&resource, TextOrSnapshot::from(self.create_text_model_snapshot(&content, None)), write_options(encoding)).await.unwrap();

        resolved = self.service.read_stream(&resource, read_options(encoding)).await.unwrap();
        let text_buffer3 = self.create_text_buffer(&resolved.value, DefaultEndOfLine::CrLf);
        assert_eq!(buffer_text(&text_buffer3), content);
    }

    /// `testReadStream(resource)`.
    async fn test_read_stream(&self, resource: URI) {
        let result = self.service.read_stream(&resource, None).await.unwrap();

        assert_eq!(result.base.name, basename(&fs_path(&resource)));
        assert_eq!(result.base.size, self.stat(&fs_path(&resource)).await);

        let content = self.read_file(&fs_path(&resource)).await.to_string();
        let text_buffer = self.create_text_buffer(&result.value, DefaultEndOfLine::Lf);
        assert_eq!(buffer_text(&text_buffer), snapshot_text(self.create_text_model_snapshot(&content, Some(false))));
    }

    /// `testRead(resource)`.
    async fn test_read(&self, resource: URI) {
        let result = self.service.read(&resource, None).await.unwrap();

        assert_eq!(result.base.name, basename(&fs_path(&resource)));
        assert_eq!(result.base.size, self.stat(&fs_path(&resource)).await);
        assert_eq!(result.value, self.read_file(&fs_path(&resource)).await.to_string());
    }

    /// `testLargeEncoding(encoding, needle)`.
    async fn test_large_encoding(&self, encoding: &str, needle: &str) {
        let resource = self.resource(&format!("lorem_{encoding}.txt"));

        // Verify via `ITextFileService.readStream`
        let result = self.service.read_stream(&resource, read_options(encoding)).await.unwrap();
        assert_eq!(result.base.encoding, encoding);

        let text_buffer = self.create_text_buffer(&result.value, DefaultEndOfLine::Lf);
        let mut contents = buffer_text(&text_buffer);

        assert_eq!(index_of(&contents, needle, 0), Some(0));
        assert!(index_of(&contents, needle, 10).is_some_and(|index| index > 0));

        // Verify via `ITextFileService.getDecodedTextFactory`
        let raw_file = self.read_file(&fs_path(&resource)).await;

        let decoded_stream = self.service.get_decoded_stream(Some(&resource), buffer_to_stream(raw_file), Some(encoding_options(encoding))).await.unwrap();
        let factory = create_text_buffer_factory_from_stream(&*decoded_stream).await.unwrap();

        let text_buffer2 = self.create_text_buffer(&factory, DefaultEndOfLine::Lf);
        contents = buffer_text(&text_buffer2);

        assert_eq!(index_of(&contents, needle, 0), Some(0));
        assert!(index_of(&contents, needle, 10).is_some_and(|index| index > 0));
    }
}

/// O corpo de um caso: o `setup` do `createSuite` dentro do guarda de vazamentos, e o descarte (`teardown`) ao fim.
fn run_suite<F, Fut>(body: F)
where
    F: FnOnce(Rc<Suite>) -> Fut + 'static,
    Fut: Future<Output = ()> + 'static,
{
    run_with_faked_timers(async move {
        let guard = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
        let suite = Rc::new(Suite::setup(guard.clone()).await);

        body(suite).await;
    });
}

/// `join(...)` do módulo `path` da plataforma.
fn join(parts: &[&str]) -> String {
    PathModule::native(host_environment().as_ref()).join(parts)
}

/// `basename(path)`.
fn basename(path: &str) -> String {
    PathModule::native(host_environment().as_ref()).basename(path, None)
}

/// `isWindows ? DefaultEndOfLine.CRLF : DefaultEndOfLine.LF`.
fn platform_eol() -> DefaultEndOfLine {
    if cfg!(windows) { DefaultEndOfLine::CrLf } else { DefaultEndOfLine::Lf }
}

/// `{ encoding }` das opções de escrever.
fn write_options(encoding: &str) -> Option<WriteTextFileOptions> {
    Some(WriteTextFileOptions { encoding: Some(encoding.to_string()), ..WriteTextFileOptions::default() })
}

/// `{ encoding }` das opções de decodificar.
fn encoding_options(encoding: &str) -> ReadTextFileEncodingOptions {
    ReadTextFileEncodingOptions { encoding: Some(encoding.to_string()), ..ReadTextFileEncodingOptions::default() }
}

/// `{ encoding }` das opções de ler.
fn read_options(encoding: &str) -> Option<ReadTextFileOptions> {
    Some(ReadTextFileOptions { encoding: encoding_options(encoding), ..ReadTextFileOptions::default() })
}

/// As opções de ler só com as `ReadTextFileEncodingOptions` dadas.
fn read_options_of(encoding: ReadTextFileEncodingOptions) -> Option<ReadTextFileOptions> {
    Some(ReadTextFileOptions { encoding, ..ReadTextFileOptions::default() })
}

/// `{ acceptTextOnly: true }`.
fn accept_text_only() -> Option<ReadTextFileOptions> {
    read_options_of(ReadTextFileEncodingOptions { accept_text_only: Some(true), ..ReadTextFileEncodingOptions::default() })
}

/// `snapshotToString(textBuffer.createSnapshot(false))`.
fn buffer_text(buffer: &PieceTreeTextBuffer) -> String {
    let mut snapshot = buffer.create_snapshot(false);

    snapshot_to_string(&mut snapshot)
}

/// `snapshotToString(snapshot)`.
fn snapshot_text(mut snapshot: Box<dyn TextSnapshot>) -> String {
    snapshot_to_string(&mut *snapshot)
}

/// `contents.indexOf(needle, from)`, com o índice em unidades UTF-16 (`None` é o `-1`).
fn index_of(contents: &str, needle: &str, from: usize) -> Option<usize> {
    let (contents, needle): (Vec<u16>, Vec<u16>) = (contents.encode_utf16().collect(), needle.encode_utf16().collect());

    (from..=contents.len().saturating_sub(needle.len())).find(|&start| contents[start..].starts_with(&needle))
}

/// `btoa(text)` para uma cadeia em que todo caractere cabe em Latin-1.
fn btoa(text: &str) -> String {
    let bytes: Vec<u8> = text.chars().map(|character| u8::try_from(u32::from(character)).expect("o caractere cabe em Latin-1")).collect();

    encode_base64(&VSBuffer::wrap(bytes), true, false)
}

/// `service.create([{ resource }])` e `service.create([{ resource, value }])`.
fn create_operation(resource: URI, value: Option<TextOrSnapshot>) -> Vec<TextFileCreateOperation> {
    vec![TextFileCreateOperation { resource, value, overwrite: None }]
}

//#endregion

#[test]
fn create_no_encoding_content_empty() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small_new.txt");

        suite.service.create(create_operation(resource.clone(), None), None).await.unwrap();

        let res = suite.read_file(&fs_path(&resource)).await;
        assert_eq!(res.byte_length(), 0 /* no BOM */);
    });
}

#[test]
fn create_no_encoding_content_provided_string() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small_new.txt");

        suite.service.create(create_operation(resource.clone(), Some(TextOrSnapshot::from("Hello World"))), None).await.unwrap();

        let res = suite.read_file(&fs_path(&resource)).await;
        assert_eq!(res.to_string(), "Hello World");
        assert_eq!(res.byte_length(), "Hello World".len());
    });
}

#[test]
fn create_no_encoding_content_provided_snapshot() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small_new.txt");

        suite.service.create(create_operation(resource.clone(), Some(TextOrSnapshot::from(string_to_snapshot("Hello World")))), None).await.unwrap();

        let res = suite.read_file(&fs_path(&resource)).await;
        assert_eq!(res.to_string(), "Hello World");
        assert_eq!(res.byte_length(), "Hello World".len());
    });
}

#[test]
fn create_utf_16_le_no_content() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small_new.utf16le");

        suite.service.create(create_operation(resource.clone(), None), None).await.unwrap();

        assert!(suite.exists(&fs_path(&resource)).await);

        let detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(UTF16LE));

        let res = suite.read_file(&fs_path(&resource)).await;
        assert_eq!(res.byte_length(), UTF16LE_BOM.len());
    });
}

#[test]
fn create_utf_16_le_content_provided() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small_new.utf16le");

        suite.service.create(create_operation(resource.clone(), Some(TextOrSnapshot::from("Hello World"))), None).await.unwrap();

        assert!(suite.exists(&fs_path(&resource)).await);

        let detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(UTF16LE));

        let res = suite.read_file(&fs_path(&resource)).await;
        assert_eq!(res.byte_length(), "Hello World".len() * 2 /* UTF16 2bytes per char */ + UTF16LE_BOM.len());
    });
}

#[test]
fn create_utf_16_be_no_content() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small_new.utf16be");

        suite.service.create(create_operation(resource.clone(), None), None).await.unwrap();

        assert!(suite.exists(&fs_path(&resource)).await);

        let detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(UTF16BE));

        let res = suite.read_file(&fs_path(&resource)).await;
        assert_eq!(res.byte_length(), UTF16LE_BOM.len());
    });
}

#[test]
fn create_utf_16_be_content_provided() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small_new.utf16be");

        suite.service.create(create_operation(resource.clone(), Some(TextOrSnapshot::from("Hello World"))), None).await.unwrap();

        assert!(suite.exists(&fs_path(&resource)).await);

        let detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(UTF16BE));

        let res = suite.read_file(&fs_path(&resource)).await;
        assert_eq!(res.byte_length(), "Hello World".len() * 2 /* UTF16 2bytes per char */ + UTF16BE_BOM.len());
    });
}

#[test]
fn create_utf_8_bom_no_content() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small_new.utf8bom");

        suite.service.create(create_operation(resource.clone(), None), None).await.unwrap();

        assert!(suite.exists(&fs_path(&resource)).await);

        let detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(UTF8_WITH_BOM));

        let res = suite.read_file(&fs_path(&resource)).await;
        assert_eq!(res.byte_length(), UTF8_BOM.len());
    });
}

#[test]
fn create_utf_8_bom_content_provided() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small_new.utf8bom");

        suite.service.create(create_operation(resource.clone(), Some(TextOrSnapshot::from("Hello World"))), None).await.unwrap();

        assert!(suite.exists(&fs_path(&resource)).await);

        let detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(UTF8_WITH_BOM));

        let res = suite.read_file(&fs_path(&resource)).await;
        assert_eq!(res.byte_length(), "Hello World".len() + UTF8_BOM.len());
    });
}

#[test]
fn create_utf_8_bom_empty_content_snapshot() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small_new.utf8bom");

        suite.service.create(create_operation(resource.clone(), Some(TextOrSnapshot::from(suite.create_text_model_snapshot("", None)))), None).await.unwrap();

        assert!(suite.exists(&fs_path(&resource)).await);

        let detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(UTF8_WITH_BOM));

        let res = suite.read_file(&fs_path(&resource)).await;
        assert_eq!(res.byte_length(), UTF8_BOM.len());
    });
}

#[test]
fn create_utf_8_bom_content_provided_snapshot() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small_new.utf8bom");

        suite.service.create(create_operation(resource.clone(), Some(TextOrSnapshot::from(suite.create_text_model_snapshot("Hello World", None)))), None).await.unwrap();

        assert!(suite.exists(&fs_path(&resource)).await);

        let detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(UTF8_WITH_BOM));

        let res = suite.read_file(&fs_path(&resource)).await;
        assert_eq!(res.byte_length(), "Hello World".len() + UTF8_BOM.len());
    });
}

#[test]
fn write_use_encoding_utf_16_be_small_content_as_string() {
    run_suite::<_, _>(|suite| async move {
        suite.test_encoding(suite.resource("small.txt"), UTF16BE, TextOrSnapshot::from("Hello\nWorld"), "Hello\nWorld").await;
    });
}

#[test]
fn write_use_encoding_utf_16_be_small_content_as_snapshot() {
    run_suite::<_, _>(|suite| async move {
        suite.test_encoding(suite.resource("small.txt"), UTF16BE, TextOrSnapshot::from(suite.create_text_model_snapshot("Hello\nWorld", None)), "Hello\nWorld").await;
    });
}

#[test]
fn write_use_encoding_utf_16_be_large_content_as_string() {
    run_suite::<_, _>(|suite| async move {
        suite.test_encoding(suite.resource("lorem.txt"), UTF16BE, TextOrSnapshot::from("Hello\nWorld"), "Hello\nWorld").await;
    });
}

#[test]
fn write_use_encoding_utf_16_be_large_content_as_snapshot() {
    run_suite::<_, _>(|suite| async move {
        suite.test_encoding(suite.resource("lorem.txt"), UTF16BE, TextOrSnapshot::from(suite.create_text_model_snapshot("Hello\nWorld", None)), "Hello\nWorld").await;
    });
}

#[test]
fn write_use_encoding_cp1252() {
    run_suite::<_, _>(|suite| async move {
        let file_path = fs_path(&suite.resource("some_cp1252.txt"));
        let contents = suite.read_file_as_utf8(&file_path).await;
        let eol = if contents.contains("\r\n") { "\r\n" } else { "\n" };
        suite
            .test_encoding_keeps_data(
                uri_file(&file_path),
                "cp1252",
                &["ObjectCount = LoadObjects(\"Öffentlicher Ordner\");", "", "Private = \"Persönliche Information\"", ""].join(eol),
            )
            .await;
    });
}

#[test]
fn write_use_encoding_shiftjis() {
    run_suite::<_, _>(|suite| async move {
        suite.test_encoding_keeps_data(suite.resource("some_shiftjis.txt"), "shiftjis", "中文abc").await;
    });
}

#[test]
fn write_use_encoding_gbk() {
    run_suite::<_, _>(|suite| async move {
        suite.test_encoding_keeps_data(suite.resource("some_gbk.txt"), "gbk", "中国abc").await;
    });
}

#[test]
fn write_use_encoding_cyrillic() {
    run_suite::<_, _>(|suite| async move {
        suite
            .test_encoding_keeps_data(suite.resource("some_cyrillic.txt"), "cp866", "АБВГДЕЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯабвгдежзийклмнопрстуфхцчшщъыьэюя")
            .await;
    });
}

#[test]
fn write_use_encoding_big5() {
    run_suite::<_, _>(|suite| async move {
        suite.test_encoding_keeps_data(suite.resource("some_big5.txt"), "cp950", "中文abc").await;
    });
}

#[test]
fn write_no_encoding_content_as_string() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small.txt");

        let content = suite.read_file(&fs_path(&resource)).await.to_string();

        suite.service.write(&resource, TextOrSnapshot::from(content.as_str()), None).await.unwrap();

        let resolved = suite.service.read_stream(&resource, None).await.unwrap();
        assert_eq!(resolved.value.get_first_line_text(999999), content);
    });
}

#[test]
fn write_no_encoding_content_as_snapshot() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small.txt");

        let content = suite.read_file(&fs_path(&resource)).await.to_string();

        suite.service.write(&resource, TextOrSnapshot::from(suite.create_text_model_snapshot(&content, None)), None).await.unwrap();

        let resolved = suite.service.read_stream(&resource, None).await.unwrap();
        assert_eq!(resolved.value.get_first_line_text(999999), content);
    });
}

#[test]
fn write_encoding_preserved_utf_16_le_content_as_string() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("some_utf16le.css");

        let resolved = suite.service.read_stream(&resource, None).await.unwrap();
        assert_eq!(resolved.base.encoding, UTF16LE);

        suite.test_encoding(suite.resource("some_utf16le.css"), UTF16LE, TextOrSnapshot::from("Hello\nWorld"), "Hello\nWorld").await;
    });
}

#[test]
fn write_encoding_preserved_utf_16_le_content_as_snapshot() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("some_utf16le.css");

        let resolved = suite.service.read_stream(&resource, None).await.unwrap();
        assert_eq!(resolved.base.encoding, UTF16LE);

        suite.test_encoding(suite.resource("some_utf16le.css"), UTF16LE, TextOrSnapshot::from(suite.create_text_model_snapshot("Hello\nWorld", None)), "Hello\nWorld").await;
    });
}

#[test]
fn write_utf8_variations_content_as_string() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("index.html");

        let mut detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, None);

        let content = suite.read_file(&fs_path(&resource)).await.to_string() + "updates";
        suite.service.write(&resource, TextOrSnapshot::from(content.as_str()), write_options(UTF8_WITH_BOM)).await.unwrap();

        detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(UTF8_WITH_BOM));

        // ensure BOM preserved if enforced
        suite.service.write(&resource, TextOrSnapshot::from(content.as_str()), write_options(UTF8_WITH_BOM)).await.unwrap();
        detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(UTF8_WITH_BOM));

        // allow to remove BOM
        suite.service.write(&resource, TextOrSnapshot::from(content.as_str()), write_options(UTF8)).await.unwrap();
        detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, None);

        // BOM does not come back
        suite.service.write(&resource, TextOrSnapshot::from(content.as_str()), write_options(UTF8)).await.unwrap();
        detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, None);
    });
}

#[test]
fn write_utf8_variations_content_as_snapshot() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("index.html");

        let mut detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, None);

        let model = suite.guard.add_rc(create_text_model(&(suite.read_file(&fs_path(&resource)).await.to_string() + "updates")));
        suite.service.write(&resource, TextOrSnapshot::from(model.create_snapshot(false)), write_options(UTF8_WITH_BOM)).await.unwrap();

        detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(UTF8_WITH_BOM));

        // ensure BOM preserved if enforced
        suite.service.write(&resource, TextOrSnapshot::from(model.create_snapshot(false)), write_options(UTF8_WITH_BOM)).await.unwrap();
        detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(UTF8_WITH_BOM));

        // allow to remove BOM
        suite.service.write(&resource, TextOrSnapshot::from(model.create_snapshot(false)), write_options(UTF8)).await.unwrap();
        detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, None);

        // BOM does not come back
        suite.service.write(&resource, TextOrSnapshot::from(model.create_snapshot(false)), write_options(UTF8)).await.unwrap();
        detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, None);
    });
}

#[test]
fn write_preserve_utf8_bom_content_as_string() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("some_utf8_bom.txt");

        let mut detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(UTF8_WITH_BOM));

        suite.service.write(&resource, TextOrSnapshot::from("Hello World"), detected_encoding.and_then(write_options)).await.unwrap();
        detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(UTF8_WITH_BOM));
    });
}

#[test]
fn write_ensure_bom_in_empty_file_content_as_string() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small.txt");

        suite.service.write(&resource, TextOrSnapshot::from(""), write_options(UTF8_WITH_BOM)).await.unwrap();

        let detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(UTF8_WITH_BOM));
    });
}

#[test]
fn write_ensure_bom_in_empty_file_content_as_snapshot() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small.txt");

        suite.service.write(&resource, TextOrSnapshot::from(suite.create_text_model_snapshot("", None)), write_options(UTF8_WITH_BOM)).await.unwrap();

        let detected_encoding = suite.detect_encoding_by_bom(&fs_path(&resource)).await;
        assert_eq!(detected_encoding, Some(UTF8_WITH_BOM));
    });
}

#[test]
fn read_stream_small_text() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small.txt");

        suite.test_read_stream(resource).await;
    });
}

#[test]
fn read_stream_large_text() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("lorem.txt");

        suite.test_read_stream(resource).await;
    });
}

#[test]
fn read_small_text() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("small.txt");

        suite.test_read(resource).await;
    });
}

#[test]
fn read_large_text() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("lorem.txt");

        suite.test_read(resource).await;
    });
}

#[test]
fn read_stream_encoding_picked_up_cp1252() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("some_small_cp1252.txt");
        let encoding = "windows1252";

        let result = suite.service.read_stream(&resource, read_options(encoding)).await.unwrap();
        assert_eq!(result.base.encoding, encoding);
        assert_eq!(result.value.get_first_line_text(999999), "Private = \"Persönlicheß Information\"");
    });
}

#[test]
fn read_encoding_picked_up_cp1252() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("some_small_cp1252.txt");
        let encoding = "windows1252";

        let result = suite.service.read(&resource, read_options(encoding)).await.unwrap();
        assert_eq!(result.base.encoding, encoding);
        assert_eq!(result.value, "Private = \"Persönlicheß Information\"");
    });
}

#[test]
fn read_encoding_picked_up_binary() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("some_small_cp1252.txt");
        let encoding = "binary";

        let result = suite.service.read(&resource, read_options(encoding)).await.unwrap();
        assert_eq!(result.base.encoding, encoding);
        assert_eq!(result.value, "Private = \"Persönlicheß Information\"");
    });
}

#[test]
fn read_encoding_picked_up_base64() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("some_small_cp1252.txt");
        let encoding = "base64";

        let result = suite.service.read(&resource, read_options(encoding)).await.unwrap();
        assert_eq!(result.base.encoding, encoding);
        assert_eq!(result.value, btoa("Private = \"Persönlicheß Information\""));
    });
}

#[test]
fn read_stream_user_overrides_bom() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("some_utf16le.css");

        let result = suite.service.read_stream(&resource, read_options("windows1252")).await.unwrap();
        assert_eq!(result.base.encoding, "windows1252");
    });
}

#[test]
fn read_stream_bom_removed() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("some_utf8_bom.txt");

        let result = suite.service.read_stream(&resource, None).await.unwrap();
        assert_eq!(result.value.get_first_line_text(999999), "This is some UTF 8 with BOM file.");
    });
}

#[test]
fn read_stream_invalid_encoding() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("index.html");

        let result = suite.service.read_stream(&resource, read_options("superduper")).await.unwrap();
        assert_eq!(result.base.encoding, "utf8");
    });
}

#[test]
fn read_stream_encoding_override() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("some.utf16le");

        let result = suite.service.read_stream(&resource, read_options("windows1252")).await.unwrap();
        assert_eq!(result.base.encoding, "utf16le");
        assert_eq!(result.value.get_first_line_text(999999), "This is some UTF 16 with BOM file.");
    });
}

#[test]
fn read_stream_large_big5() {
    run_suite::<_, _>(|suite| async move {
        suite.test_large_encoding("big5", "中文abc").await;
    });
}

#[test]
fn read_stream_large_cp1252() {
    run_suite::<_, _>(|suite| async move {
        suite.test_large_encoding("cp1252", "öäüß").await;
    });
}

#[test]
fn read_stream_large_cyrillic() {
    run_suite::<_, _>(|suite| async move {
        suite.test_large_encoding("cp866", "АБВГДЕЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯабвгдежзийклмнопрстуфхцчшщъыьэюя").await;
    });
}

#[test]
fn read_stream_large_gbk() {
    run_suite::<_, _>(|suite| async move {
        suite.test_large_encoding("gbk", "中国abc").await;
    });
}

#[test]
fn read_stream_large_shiftjis() {
    run_suite::<_, _>(|suite| async move {
        suite.test_large_encoding("shiftjis", "中文abc").await;
    });
}

#[test]
fn read_stream_large_utf8_bom() {
    run_suite::<_, _>(|suite| async move {
        suite.test_large_encoding("utf8bom", "öäüß").await;
    });
}

#[test]
fn read_stream_large_utf16_le() {
    run_suite::<_, _>(|suite| async move {
        suite.test_large_encoding("utf16le", "öäüß").await;
    });
}

#[test]
fn read_stream_large_utf16_be() {
    run_suite::<_, _>(|suite| async move {
        suite.test_large_encoding("utf16be", "öäüß").await;
    });
}

#[test]
fn read_stream_utf16_le_no_bom() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("utf16_le_nobom.txt");

        let result = suite.service.read_stream(&resource, None).await.unwrap();
        assert_eq!(result.base.encoding, "utf16le");
    });
}

#[test]
fn read_stream_utf16_be_no_bom() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("utf16_be_nobom.txt");

        let result = suite.service.read_stream(&resource, None).await.unwrap();
        assert_eq!(result.base.encoding, "utf16be");
    });
}

#[test]
fn read_stream_autoguess_encoding() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("some_cp1252.txt");

        let result = suite
            .service
            .read_stream(&resource, read_options_of(ReadTextFileEncodingOptions { auto_guess_encoding: Some(true), ..ReadTextFileEncodingOptions::default() }))
            .await
            .unwrap();
        assert_eq!(result.base.encoding, "windows1252");
    });
}

#[test]
fn read_stream_autoguess_encoding_candidate_guess_encodings() {
    run_suite::<_, _>(|suite| async move {
        // This file is determined to be Windows-1252 unless candidateDetectEncoding is set.
        let resource = suite.resource("some.shiftjis.1.txt");

        let result = suite
            .service
            .read_stream(
                &resource,
                read_options_of(ReadTextFileEncodingOptions {
                    auto_guess_encoding: Some(true),
                    candidate_guess_encodings: Some(vec!["utf-8".to_string(), "shiftjis".to_string(), "euc-jp".to_string()]),
                    ..ReadTextFileEncodingOptions::default()
                }),
            )
            .await
            .unwrap();
        assert_eq!(result.base.encoding, "shiftjis");
    });
}

#[test]
fn read_stream_autoguess_encoding_candidate_guess_encodings_is_empty() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("some_cp1252.txt");

        let result = suite
            .service
            .read_stream(
                &resource,
                read_options_of(ReadTextFileEncodingOptions { auto_guess_encoding: Some(true), candidate_guess_encodings: Some(Vec::new()), ..ReadTextFileEncodingOptions::default() }),
            )
            .await
            .unwrap();
        assert_eq!(result.base.encoding, "windows1252");
    });
}

#[test]
fn read_stream_file_is_binary() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("binary.txt");

        let error = suite.service.read_stream(&resource, accept_text_only()).await.err();

        let error = error.expect("o arquivo binário é rejeitado");
        assert_eq!(text_file_operation_result_of(&error), Some(TextFileOperationResult::FileIsBinary));

        let result = suite.service.read_stream(&suite.resource("small.txt"), accept_text_only()).await.unwrap();
        assert_eq!(result.base.name, "small.txt");
    });
}

#[test]
fn read_file_is_binary() {
    run_suite::<_, _>(|suite| async move {
        let resource = suite.resource("binary.txt");

        let error = suite.service.read(&resource, accept_text_only()).await.err();

        let error = error.expect("o arquivo binário é rejeitado");
        assert_eq!(text_file_operation_result_of(&error), Some(TextFileOperationResult::FileIsBinary));

        let result = suite.service.read(&suite.resource("small.txt"), accept_text_only()).await.unwrap();
        assert_eq!(result.base.name, "small.txt");
    });
}
