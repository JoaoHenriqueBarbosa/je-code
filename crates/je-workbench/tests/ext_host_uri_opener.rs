//! Porte de `src/vs/workbench/api/test/browser/extHostUriOpener.test.ts` (suíte `ExtHostUriOpeners`), caso a caso e na mesma ordem.
//!
//! PORTADOS: nenhum (0 dos 3 casos do original, contando cada passada do laço `for (const openToSide of [true, false, undefined])`).
//!
//! FORA, e por quê (nenhum vira `#[test]` enquanto não puder ser fiel): os três casos `maps internal openToSide=... to viewColumn`
//! montam a cadeia inteira `OpenerService` -> `ExternalUriOpenerService` -> `MainThreadUriOpeners` -> `ExtHostUriOpeners` sobre um
//! `TestRPCProtocol`, e nada dessa cadeia, além do `OpenerService`, existe no porte:
//! - `ExtHostUriOpeners` (`src/vs/workbench/api/common/extHostUriOpener.ts`), `MainThreadUriOpeners`
//!   (`src/vs/workbench/api/browser/mainThreadUriOpeners.ts`) e os identificadores e formas `ExtHostUriOpenersShape` e
//!   `MainThreadUriOpenersShape` de `extHost.protocol.ts` (`MainContext.MainThreadUriOpeners`, `ExtHostContext.ExtHostUriOpeners`)
//!   não foram portados.
//! - `ExternalUriOpenerService` (`src/vs/workbench/contrib/externalUriOpener/common/externalUriOpenerService.ts`), que o
//!   `MainThreadUriOpeners` registra como `IExternalOpenerProvider`, também não existe em `crates/je-workbench/src/contrib`.
//! - Os dublês de `workbenchTestServices.ts` que o `setup` usa (`TestExtensionService`, `TestStorageService`) e os de
//!   `TestNotificationService`, `TestConfigurationService` (da plataforma) e `IQuickInputService`/`IPreferencesService` ainda não têm
//!   equivalente em `tests/support`. O `OpenerService` e o `TestCodeEditorService` já existem (`je-editor/src/opener_service.rs`,
//!   `je-editor/tests/support/test_code_editor.rs`).
//!
//! Quando o `ExtHostUriOpeners` e o `MainThreadUriOpeners` forem portados, os três casos entram aqui: o `openToSide` `true`, `false` e
//! `undefined` abre `https://source.example.com` com `allowContributedOpeners: true` e o opener da extensão recebe
//! `{ sourceUri, viewColumn: openToSide ? ViewColumn.Beside : undefined }` com o URI resolvido pelo resolvedor externo.
