# O que as extensões embutidas pedem do `vscode`, ordenado por frequência

Fonte de todos os números: **eu**, 2026-10-24, `grep` sobre o TypeScript de `ref/vscode/extensions/*/` (as 33 extensões que
têm `main` no `package.json`, menos `copilot`, fora de escopo). Só os fontes de produção entram: ficam de fora `*.d.ts`,
`*.test.ts`, `test/`, `test-*` e `vscode-api-tests` (a suíte de testes do próprio oráculo, não uma extensão de usuário).
Sobram 773 arquivos `.ts`, dos quais 416 importam `vscode`. Os bundles (`out/`, `dist/`) NÃO existem em `ref/` (só o
código-fonte), então o que se mede aqui é **uso estático no fonte**, não o que a ativação toca.

## Método

- Forma `vscode.<namespace>.<membro>` (`import * as vscode from 'vscode'`): `grep -ohE '\bvscode\.(workspace|window|languages|env|extensions|commands|debug|tasks|scm|authentication|l10n|notebooks|tests|lm|chat)\.[A-Za-z_]+'`.
- Forma `import { window, workspace } from 'vscode'`: o mesmo padrão sem o prefixo, `grep -P` com `(?<![.\w'"])`, restrito aos
  arquivos que importam `vscode`. Tem falso positivo de variável local com o mesmo nome (`languages.add`, `tasks.push`,
  `tasks.length`, `env.PATH`, `scm.com`, `tasks.json`): estes foram descartados à mão da lista abaixo.
- Cada linha soma as duas formas. É contagem de ocorrências, não de extensões, salvo onde a coluna "ext" aparece (número de
  extensões distintas que usam o membro, por `grep -l`).
- Classes: `vscode.<Classe>` mais os nomes importados por `import { ... } from 'vscode'` (conta arquivos que importam).

## NÃO MEDIDO

- O que cada extensão toca **na ativação** (o `crates/je-exthost/examples/api_usage.rs` mede isso sobre um bundle executado;
  sem os bundles em `ref/` e sem poder rodar `cargo` nesta tarefa, não rodou). Os 15 de 34 que não ativam e os nomes que faltam
  estão em `exthost-plan.md` (fork, 2026-10-23).
- Acesso dinâmico (`vscode['x']`, desestruturação de `vscode` em variável) e re-exportação por módulo próprio.
- Peso por frequência de execução (chamadas por segundo): só a presença no fonte.
- Os 202 membros de primeiro nível do `vscode.d.ts` que ninguém usa: não foram contados um a um.

## Membros, em ordem de frequência (soma das duas formas)

| # | Membro | Ocorrências | Extensões |
| --- | --- | --- | --- |
| 1 | `l10n.t` | 1 280 (512 + 768) | NÃO MEDIDO |
| 2 | `workspace.getConfiguration` | 268 | 24 |
| 3 | `commands.executeCommand` | 199 | NÃO MEDIDO |
| 4 | `window.activeTextEditor` | 134 | 12 |
| 5 | `commands.registerCommand` | 118 | 17 |
| 6 | `workspace.workspaceFolders` | 86 | NÃO MEDIDO |
| 7 | `window.showWarningMessage` | 83 | 18 (os três `show*Message`) |
| 8 | `workspace.onDidChangeConfiguration` | 63 | 22 |
| 9 | `window.showErrorMessage` | 62 | (idem) |
| 10 | `window.showInformationMessage` | 62 | (idem) |
| 11 | `workspace.fs` | 53 | NÃO MEDIDO |
| 12 | `workspace.openTextDocument` | 36 | NÃO MEDIDO |
| 13 | `window.showQuickPick` | 29 | NÃO MEDIDO |
| 14 | `env.openExternal` | 25 | NÃO MEDIDO |
| 15 | `window.withProgress` | 24 | NÃO MEDIDO |
| 16 | `workspace.createFileSystemWatcher` | 23 | NÃO MEDIDO |
| 17 | `window.tabGroups` | 22 | NÃO MEDIDO |
| 18 | `workspace.isTrusted` | 22 | NÃO MEDIDO |
| 19 | `extensions.getExtension` | 22 | NÃO MEDIDO |
| 20 | `env.clipboard` | 21 | NÃO MEDIDO |
| 21 | `workspace.getWorkspaceFolder` | 21 | NÃO MEDIDO |
| 22 | `workspace.applyEdit` | 20 | NÃO MEDIDO |
| 23 | `workspace.textDocuments` | 19 | NÃO MEDIDO |
| 24 | `workspace.onDidChangeTextDocument` | 18 | NÃO MEDIDO |
| 25 | `window.createOutputChannel` | 17 | NÃO MEDIDO |
| 26 | `languages.registerCompletionItemProvider` | 17 | NÃO MEDIDO |
| 27 | `window.visibleTextEditors` | 15 | NÃO MEDIDO |
| 28 | `window.showInputBox` | 15 | NÃO MEDIDO |
| 29 | `window.onDidChangeActiveTextEditor` | 14 | NÃO MEDIDO |
| 30 | `window.showTextDocument` | 14 | NÃO MEDIDO |
| 31 | `env.appName` | 13 | NÃO MEDIDO |
| 32 | `window.createQuickPick` | 12 | NÃO MEDIDO |
| 33 | `onDidChangeWorkspaceFolders` (qualquer forma) | 12 | NÃO MEDIDO |
| 34 | `workspace.onDidCloseTextDocument` | 10 | NÃO MEDIDO |
| 35 | `onDidChangeVisibleTextEditors` (qualquer forma) | 10 | NÃO MEDIDO |
| 36 | `workspace.workspaceFile` | 9 | NÃO MEDIDO |
| 37 | `languages.registerCodeActionsProvider` | 9 | NÃO MEDIDO |
| 38 | `env.uriScheme` | 9 | NÃO MEDIDO |
| 39 | `workspace.onDidOpenTextDocument` | 8 | NÃO MEDIDO |
| 40 | `window.createStatusBarItem` | 8 | NÃO MEDIDO |
| 41 | `workspace.asRelativePath` | 7 | NÃO MEDIDO |
| 42 | `workspace.findFiles` | 7 | NÃO MEDIDO |
| 43 | `extensions.all` | 6 | NÃO MEDIDO |
| 44 | `env.asExternalUri` | 6 | NÃO MEDIDO |
| 45 | `env.remoteName` | 6 | NÃO MEDIDO |
| 46 | `languages.registerHoverProvider` | 5 | NÃO MEDIDO |
| 47 | `languages.createDiagnosticCollection` | 5 | NÃO MEDIDO |
| 48 | `env.uiKind` | 5 | NÃO MEDIDO |
| 49 | `languages.registerDefinitionProvider` | 4 | NÃO MEDIDO |
| 50 | `languages.match`, `languages.getLanguages` | 4 e 4 | NÃO MEDIDO |
| 51 | `languages.registerDocumentRangeFormattingEditProvider` | 4 | NÃO MEDIDO |
| 52 | `languages.getDiagnostics` | 1 | NÃO MEDIDO |
| 53 | `window.setStatusBarMessage`, `languages.setLanguageConfiguration`, `languages.registerDocumentFormattingEditProvider` | 0 | NÃO MEDIDO |

A última linha entra porque a tarefa pediu estes três nomes, e o fonte embutido não os usa (extensões de terceiros usam, é API
estável do `vscode.d.ts`).

Abaixo da linha 53 (uso 1 a 3 cada, nada que a tarefa corrente cobre): `workspace.registerTextDocumentContentProvider`,
`registerFileSystemProvider`, `registerNotebookSerializer`, `onDidSaveTextDocument`, `onDid/WillRenameFiles`,
`onDidCreateFiles`, `onDidDeleteFiles`, `window.createTextEditorDecorationType`, `createWebviewPanel`, `createTreeView`,
`registerCustomEditorProvider`, `registerWebviewPanelSerializer`, `registerUriHandler`, `showOpenDialog`, `showSaveDialog`,
`tasks.registerTaskProvider`, `tasks.executeTask`, `debug.startDebugging`, `authentication.getSession`,
`authentication.registerAuthenticationProvider`, `languages.registerReferenceProvider`, `registerDocumentSymbolProvider`,
`registerCodeLensProvider`, `registerSignatureHelpProvider`, `registerInlayHintsProvider`, `registerRenameProvider`,
`registerFoldingRangeProvider` e o resto dos provedores de linguagem.

## Classes e tipos de valor, em ordem de frequência (`vscode.X` mais importação nomeada)

Só as que existem em tempo de execução (tipos puros como `TextDocument`, `CancellationToken`, `ExtensionContext`,
`WorkspaceConfiguration`, `Event`, `Memento`, `ProviderResult`, `DocumentSelector` não são construídos pela extensão):

1. `Uri` (655 + 66 arquivos; `file`, `parse`, `joinPath`, `from`, `with`) e `Disposable` (173 + 56)
2. `Range` (235 + 13), `Position` (129 + 16), `Selection` (57 + 4)
3. `EventEmitter` (86 + 32)
4. `Location` (58 + 3), `CompletionItem` (52 + 6), `Diagnostic` (43 + 4), `WorkspaceEdit` (42 + 4), `TextEdit` (32 + 4)
5. Enums: `SymbolKind` (57), `CodeActionKind` (41), `CompletionItemKind` (32 + 4), `ViewColumn` (31), `ConfigurationTarget` (24),
   `FileType` (17 + 4), `ProgressLocation` (12 + 7), `StatusBarAlignment` (9 + 2), `DiagnosticSeverity` (3), `UIKind` (3)
6. `SnippetString` (28 + 4), `CodeAction` (26), `MarkdownString` (13 + 6), `CompletionList` (11 + 4), `RelativePattern` (13 + 4),
   `Hover` (5), `LocationLink` (12), `FoldingRange` (10), `ThemeIcon` (7), `ThemeColor` (3)
7. `CancellationTokenSource` (20), `CancellationError` (22 + 5), `FileSystemError` (13), `DataTransfer` (20)
8. Fora desta rodada: `Task`, `ShellExecution`, `TaskScope`, `TaskGroup`, `WebviewPanel`, `TerminalCompletionItemKind`,
   `AuthenticationSession`, `DebugSession`, `CallHierarchyItem`, `SourceControl*`, `Tab*`.

Os usos de nomes sem prefixo em arquivos que importam `vscode` por `import *` não entram na segunda parte (só as
importações nomeadas). Para as classes, o total por arquivo é o limite superior: um arquivo conta uma vez por nome importado.

## Como isto ordena as entregas de `crates/je-exthost/src/api/`

1. `workspace`: `getConfiguration`, `workspaceFolders`, `onDidChangeConfiguration`, `fs`, `openTextDocument`, `textDocuments`,
   `getWorkspaceFolder`, `asRelativePath`, `findFiles`, `onDidChangeWorkspaceFolders`, os eventos de documento, `name`,
   `rootPath`.
2. `window`: `activeTextEditor`, `show*Message`, `showQuickPick`, `showInputBox`, `withProgress`, `createOutputChannel`,
   `visibleTextEditors`, `onDidChangeActiveTextEditor`, `createStatusBarItem`, `setStatusBarMessage`.
3. `languages`: provedores de completion, code action, hover, definition, formatting; `createDiagnosticCollection`,
   `getDiagnostics`, `setLanguageConfiguration`, `match`, `getLanguages`.
4. `env`, `extensions`, `Uri`, `EventEmitter`, `CancellationTokenSource` e as classes de valor acima.
