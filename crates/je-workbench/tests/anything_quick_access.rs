//! Porte de `src/vs/workbench/contrib/search/test/browser/anythingQuickAccess.test.ts` (suíte `AnythingQuickAccess - links`), caso a caso e na mesma ordem.
//!
//! NENHUM caso foi portado (sem `#[test]`), todos de fora pelo mesmo motivo: o `AnythingQuickAccessProvider`
//! (`contrib/search/browser/anythingQuickAccess.ts`, 1267 linhas) estende o `PickerQuickAccessProvider` de
//! `platform/quickinput/browser/pickerQuickAccess.ts`, que não existe no porte (`je-platform/src/quickinput` só tem os tipos
//! de `quickInput.ts`, sem `IQuickAccessProvider`, `TriggerAction`, `Picks`/`FastAndSlowPicks` nem o `provide` que liga o
//! picker). O `_getPicks` devolve o resultado de `doGetPicks` (busca de arquivos, histórico, símbolos de editor com
//! `IOutlineModelService`, `IQuickChatService`, `IEditorService`) para tudo que não é link, e os casos de "preserva a consulta
//! de arquivo" (`!Array.isArray(picks)`, `null` ao mudar só o intervalo) leem exatamente esse caminho. Os casos de link ainda
//! exigem `IEditorGroupsService.activeGroup`/`activateGroup`, `IOpenerService` com `OpenerService` real e abridor externo
//! contribuído, `INotificationService`, `IQuickInputService.createQuickPick` e a `workbenchInstantiationService`. Portar só o
//! `_getLinkPick` deixaria os casos infiéis (o teste os exercita pelo provider inteiro), então ficam de fora.
//!
//! Os casos (expandindo os `for`): `opens through shared handlers without changing the link: {link}` (6 links), `Alt+Enter
//! opens beside the originating group even after the active group changes`, `side button uses shared handlers and closes the
//! picker`, `renders the link icon inline rather than applying icon font classes to the row`, `side-opening intent reaches a
//! contributed external URI handler`, `respects preview configuration and the existing Ctrl/Cmd side-opening gesture`,
//! `background acceptance preserves focus and pins the editor`, `preserves file queries and Quick Access prefixes: {filter}`
//! (9), `reports an invalid or disallowed link without an open action: {link}` (8), `respects caller filtering`, `file-only
//! and delegated pickers do not offer link actions`, `refreshes file picks after {before} -> {link} -> {after}` (6),
//! `refreshes file picks after a link was filtered out`, `does not require an active editor group for a specialized picker:
//! {filesOnly,fileSearchDelegate,handleAccept}` (3), `does not require an active editor group for quick navigation`, `reports
//! unhandled links`, `reports opener errors`, `captures the source group when the picker is provided, not when the link is
//! typed`.
