//! Porte de `src/vs/workbench/services/textfile/test/common/textFileService.io.test.ts` (suíte `Files - BrowserTextFileService i/o`,
//! que é a função `createSuite` chamada por `src/vs/workbench/services/textfile/test/browser/browserTextFileService.io.test.ts`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`createSuite(params)`.** A função é compartilhada com `nativeTextFileService.io.test.ts` e vive em
//!   `tests/support/text_file_service_io_suite.rs` (módulo `io_suite`, com todas as notas de tradução dos casos, das fixtures e
//!   do arranjo). Este arquivo só escolhe a classe do serviço do `setup`: `TestBrowserTextFileServiceWithEncodingOverrides`,
//!   que é [`TextFileTestAccessor::with_child_services`].
//! - **`isWeb`.** O original só roda esta suíte quando `isWeb` é verdadeiro (a suíte nativa cobre o outro ambiente); o porte
//!   roda sempre, porque é o arranjo do navegador que o porte implementa.

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

io_suite::text_file_service_io_cases!(TextFileTestAccessor::with_child_services);
