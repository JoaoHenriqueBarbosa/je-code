//! Porte de `src/vs/workbench/services/textfile/test/common/textFileService.io.test.ts` (suíte `Files - NativeTextFileService i/o`,
//! que é a função `createSuite` chamada por `src/vs/workbench/services/textfile/test/electron-browser/nativeTextFileService.io.test.ts`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`createSuite(params)`.** A função é a mesma de `browserTextFileService.io.test.ts` e vive em
//!   `tests/support/text_file_service_io_suite.rs` (módulo `io_suite`, com todas as notas de tradução dos casos, das fixtures e
//!   do arranjo): este arquivo a reaproveita e só escolhe a classe do serviço do `setup`,
//!   `TestNativeTextFileServiceWithEncodingOverrides`, que é [`TextFileTestAccessor::with_native_child_services`] (o serviço é o
//!   `NativeTextFileService`).
//! - **Arranjo.** O `setup` do original é o do navegador (`TestInMemoryFileSystemProvider` registrado para o esquema `file`, o
//!   `WorkingCopyFileService` do filho e os arquivos de `files.ts` em `testDir`); só o `workbenchInstantiationService` vem do
//!   `electron-browser/workbenchTestServices.ts`, que acrescenta o `INativeHostService` e o `TestNativeWorkingCopyBackupService`,
//!   serviços que o serviço de arquivos de texto não lê.
//! - **`exists`, `stat`, `readFile` e `detectEncodingByBOM`.** São idênticos aos do navegador (leitura direta do provedor), e
//!   `readFile(path, encoding)` só é chamada com `utf8` (o `TextDecoder(toCanonicalName(encoding))`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[allow(dead_code)]
#[path = "support/text_file_service_io_suite.rs"]
mod io_suite;
mod support;
#[allow(dead_code)]
#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use support::text_file_support::TextFileTestAccessor;

io_suite::text_file_service_io_cases!(TextFileTestAccessor::with_native_child_services);
