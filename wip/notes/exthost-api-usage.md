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

## Estado da entrega de `window` (2026-10-24, escrito sem compilar: o integrador confirma)

Fonte: **eu** (leitura de `crates/je-exthost/src/api/window.rs` e dos módulos citados). Nenhum número de ativação foi medido.

| Membro | Estado | Onde |
| --- | --- | --- |
| `showInformationMessage`, `showWarningMessage`, `showErrorMessage` | entregue | `api/window.rs` |
| `createStatusBarItem`, `setStatusBarMessage` | entregue | `api/window.rs` |
| `showQuickPick` (texto e objeto, separador, `canPickMany`, `placeHolder`, `onDidSelectItem`, lista por promessa, token) | entregue | `api/quick_input.rs`, sobre `ExtHostQuickOpen` |
| `showInputBox` (`value`, `valueSelection`, `prompt`, `placeHolder`, `password`, `validateInput`, token) | entregue | `api/quick_input.rs` |
| `createOutputChannel` (canal comum, `languageId`, `{ log: true }`, `append`, `appendLine`, `replace`, `clear`, `show`, `hide`, `dispose`, `logLevel`, `onDidChangeLogLevel`, `trace` a `error`) | entregue | `api/output_channel.rs`, sobre `ExtHostOutputService` |
| `withProgress` (`ProgressLocation`, `{ viewId }`, `title`, `cancellable`, `progress.report`, token) | entregue | `api/progress.rs`, sobre `ExtHostProgress` |
| `QuickPickItemKind`, `InputBoxValidationSeverity`, `QuickInputButtons`, `ProgressLocation`, `LogLevel` | entregue | `api/enums.rs` |
| `createQuickPick`, `createInputBox` (`QuickPick`: `title`, `step`, `totalSteps`, `enabled`, `busy`, `ignoreFocusOut`, `value`, `placeholder`, `buttons`, `items` com `buttons` de item, `canSelectMany`, `matchOnDescription`, `matchOnDetail`, `sortByLabel`, `keepScrollPosition`, `activeItems`, `selectedItems`; `InputBox`: as comuns mais `password`, `prompt`, `valueSelection`, `validationMessage`; `show`, `hide`, `dispose`; `onDidAccept`, `onDidChangeValue`, `onDidChangeActive`, `onDidChangeSelection`, `onDidTriggerButton`, `onDidTriggerItemButton`, `onDidHide`) | entregue | `api/quick_session.rs`, sobre as sessões do `ExtHostQuickOpen` |
| `activeTextEditor`, `visibleTextEditors`, `onDidChangeActiveTextEditor`, `onDidChangeVisibleTextEditors`, `showTextDocument` | entregue | `api/text_editors.rs`, sobre `ExtHostDocumentsAndEditors`, `ExtHostEditors` e `MainThreadDocumentsAndEditors` |
| `onDidChangeTextEditorSelection`, `onDidChangeTextEditorOptions`, `onDidChangeTextEditorVisibleRanges`, `onDidChangeTextEditorViewColumn` | entregue | `api/text_editors.rs` |
| `TextEditor` (`document`, `selection`, `selections`, `options`, `visibleRanges`, `viewColumn`, `edit` com `TextEditorEdit`, `revealRange`, `show`, `hide`) | entregue | `api/text_editors.rs` |
| `TextEditor#insertSnippet`, `setDecorations`, `createTextEditorDecorationType` | falta | pedem `SnippetString` e as decorações de ponta a ponta |
| `tabGroups`, `createWebviewPanel` | falta | |
| `createTerminal(name, shellPath, shellArgs)`, `createTerminal(options)` (`name`, `shellPath`, `shellArgs`, `cwd`, `env`, `iconPath` como `Uri` ou `{ light, dark }`, `color`, `message`, `strictEnv`, `hideFromUser`, `isTransient`) e `createTerminal({ name, pty })` com `Pseudoterminal` (`onDidWrite`, `onDidClose`, `onDidOverrideDimensions`, `onDidChangeName`, `open`, `close`, `handleInput`, `setDimensions`, com o `this` do objeto) | entregue | `api/terminal.rs`, sobre o `ExtHostTerminalService` (`WorkerExtHostTerminalService::new_local`) |
| `terminals`, `activeTerminal`, `onDidOpenTerminal`, `onDidCloseTerminal`, `onDidChangeActiveTerminal`, `onDidChangeTerminalState`, `Terminal` (`name`, `processId`, `creationOptions`, `exitStatus`, `state`, `sendText`, `show`, `hide`, `dispose`), `registerTerminalProfileProvider`, `TerminalProfile` | entregue | `api/terminal.rs` |
| `TerminalOptions#location`, `iconPath` como `ThemeIcon`, `Terminal#shellIntegration`, `onDidChangeTerminalDimensions`, `onDidWriteTerminalData`, `onDidExecuteTerminalCommand`, `registerTerminalLinkProvider`, `registerTerminalQuickFixProvider`, `environmentVariableCollection` | falta | o serviço tem; falta a ponte (`TerminalLocation` tem o enum em `api/enums.rs`, falta ler `location`) |
| `createTreeView(viewId, { treeDataProvider, showCollapseAll, canSelectMany, manageCheckboxStateManually })`, `registerTreeDataProvider`, `TreeDataProvider` de classe (`getTreeItem`, `getChildren`, `getParent`, `resolveTreeItem`, `onDidChangeTreeData`, com `this` e elementos por identidade), `TreeItem`, `TreeItemCollapsibleState` | entregue | `api/tree_views.rs`, sobre o `ExtHostTreeViews` |
| `TreeView` (`selection`, `visible`, `message`, `title`, `description`, `onDidChangeSelection`, `onDidChangeVisibility`, `onDidExpandElement`, `onDidCollapseElement`, `reveal`, `dispose`) | entregue | `api/tree_views.rs` |
| `TreeView#badge`, `onDidChangeCheckboxState`, `dragAndDropController`, `activeItem` (proposta) e a mensagem em markdown | falta | o serviço tem; falta a ponte |
| `scm.createSourceControl(id, label, rootUri?)` com `SourceControl` (`id`, `label`, `rootUri`, `inputBox` com `value`, `placeholder`, `enabled`, `visible`; `count`, `commitTemplate`, `statusBarCommands`, `acceptInputCommand`, `quickDiffProvider`, `createResourceGroup`, `dispose`) e `SourceControlResourceGroup` (`id`, `label`, `hideWhenEmpty`, `contextValue`, `resourceStates` com `command`, `contextValue` e `decorations`, `dispose`) | entregue | `api/scm.rs`, sobre o `ExtHostSCM`; sem `MainThreadSCM` em `api/browser/` o teste usa um dublê do `MainThreadSCMShape` |
| `SourceControl#actionButton`, `historyProvider`, `artifactProvider`, `secondaryQuickDiffProvider`, `selected`, `SourceControlInputBox#validateInput`, `scm.inputBox` | falta | propostas de API (o `ExtHostSCM` as tem) ou obsoleto |

Terminais e árvores usam o mecanismo novo `api/js_object.rs` (`JsObjects`): leitura de propriedades, chamada de método e escuta de evento POR
REFERÊNCIA (um leitor em JavaScript entrega o valor a uma função nativa por referência), mais `identify` (um `WeakMap` dá um número estável a cada
objeto), porque o motor copia objetos comuns e o `ExtHostTreeViews` compara os elementos por identidade. As funções de `window` que recebem objetos da
extensão (`createTerminal`, `registerTerminalProfileProvider`, `createTreeView`, `registerTreeDataProvider`) são campos de função do próprio namespace,
não métodos do protótipo (que recebem cópias). Os encaminhadores `LateExtHostTerminalService` e `LateExtHostTreeViews` (`in_process.rs`) entregam ao
serviço real as chamadas que o cliente faz no construtor.

Os testes de ponta a ponta são `crates/je-workbench/tests/extension_host_api_terminal.rs` e `extension_host_api_tree_views.rs`. Em ambos o lado do
workbench é um DUBLÊ do `MainThread*Shape` (o `MainThreadTerminalService` real pede doze serviços do terminal e o `MainThreadTreeViews` real a camada de
visões, sem dublês prontos): o dublê anota os pedidos da extensão e devolve ao `ExtHost*` real o que o cliente real devolveria.

Pendências conhecidas do que acabou de entrar (a confirmar na primeira compilação): `ITerminalDimensionsDto`, `TerminalLaunchConfig` e
`TreeViewRegistrationOptions` precisam ter campos públicos; `ICreateContributedTerminalProfileOptions` precisa de `Default`; os nomes
`ExtHostContext::EXT_HOST_TREE_VIEWS` e `MainContext::MAIN_THREAD_TREE_VIEWS`; `Promise::then` com `Fn`; a ordem dos eventos de seleção e visibilidade no
teste de árvore; o `TreeItem` que `getTreeItem` devolve e o `Pseudoterminal` devolvido por um provedor de perfil atravessam como cópia (o `this` do
`pty` de um perfil não vale; o `pty` passado direto a `createTerminal` vai por referência e vale).

Em `workspace` (mesmo módulo de ponte): `textDocuments`, `openTextDocument` (`Uri`, caminho e opções `{ language, content, encoding }`), `onDidOpenTextDocument`,
`onDidCloseTextDocument`, `onDidChangeTextDocument` e `onDidSaveTextDocument` entregues; `onWillSaveTextDocument` e `registerTextDocumentContentProvider` faltam.

## Estado da entrega de `scm` e das sessões de seletor (2026-10-24, escrito sem compilar: o integrador confirma)

Fonte: **eu** (leitura de `api/scm.rs`, `api/quick_session.rs`, `ext_host_scm.rs`, `ext_host_quick_open.rs` e do oráculo). Nada foi medido em execução.

`scm` entra em `vscode` quando o binder registra `MainThreadSCM`; os documentos de texto são opcionais (sem eles o host usa um `MainThreadDocuments` que rejeita,
`NoDocuments` em `host.rs`, porque só a proposta `scmTextDocument` lê documentos). O encaminhador `LateExtHostSCM` (`in_process.rs`) entrega ao `ExtHostSCM` real as
chamadas do cliente. O `quickDiffProvider` chega como cópia (função `provideOriginalResource` própria do objeto, sem `this`). `createQuickPick` e `createInputBox` guardam
os itens e os botões da extensão por cópia: `activeItems` e `selectedItems` casam por igualdade estrutural e os eventos devolvem a cópia guardada (com as propriedades
extras); `QuickInputButtons.Back` é reconhecido pelo `ThemeIcon` `arrow-left`.

Testes: `extension_host_api_scm.rs` (dublê do `MainThreadSCMShape`, `debounce` de 100 ms esperado com `sleep`) e `extension_host_api_window_quick_pick_session.rs`
(dublê do `MainThreadQuickOpenShape` que anota cada `$createOrUpdate`).

Correções da rodada anterior (medidas pelo integrador): `createTerminal(name, shellPath, shellArgs)` agora exporta o array `shellArgs` (a função é por referência, o
array chegava como `Handle`); `EventEmitter#fire` virou função por referência própria de cada instância, então o elemento que um `TreeDataProvider` dispara em
`onDidChangeTreeData` chega ao `$refresh` com a identidade certa; o caso do pty esperava `close` depois de a saída do processo, mas o oráculo apaga o processo em
`_onProcessExit` (o `shutdown` seguinte não faz nada), então o teste agora espera `undefined` e um caso novo cobre o `shutdown` com o processo vivo.

Pendências conhecidas (a confirmar na primeira compilação): `ExtHostContext::EXT_HOST_SCM` e `MainContext::MAIN_THREAD_SCM`; campos públicos de `SCMRawResourceSplices`,
`SCMRawResource`, `TransferQuickInput`; `InputBoxValidationSeverity::value`; `QuickInputButton` com `toggle: Option<QuickInputButtonToggle>`; `Rc::try_unwrap` do item.

## Estado da entrega de `env` e `extensions` (2026-10-24, escrito sem compilar: o integrador confirma)

Fonte: **eu** (leitura de `api/env.rs`, `api/extensions.rs` e do oráculo `extHost.api.impl.ts`). Nenhum número de ativação foi medido.

| Membro | Estado | Onde |
| --- | --- | --- |
| `env.appName`, `appRoot`, `language`, `machineId`, `sessionId`, `uriScheme`, `shell`, `uiKind` | entregue (valores do `HostEnvironment` que o binder entrega, no lugar do `initData`) | `api/env.rs` |
| `env.clipboard.readText`, `writeText` | entregue (direto no `IClipboardService`: `ExtHostClipboard` e `MainThreadClipboard` não estão portados) | `api/env.rs` |
| `env.openExternal` | entregue, sem teste (sobre `ExtHostWindow`; pede o cliente `MainThreadWindow`, ainda não portado) | `api/env.rs` |
| `env.asExternalUri`, `remoteName`, `isTelemetryEnabled`, `logLevel`, `createTelemetryLogger` e as propostas | falta | |
| `extensions.all`, `getExtension(id)` | entregue | `api/extensions.rs`, sobre os registros do `ExtHostExtensionService` |
| `Extension` (`id`, `extensionUri`, `extensionPath`, `packageJSON`, `extensionKind`, `isFromDifferentExtensionHost`, `isActive`, `exports`, `activate()`) | entregue | `api/extensions.rs` |
| `extensions.onDidChange`, `allAcrossExtensionHosts` | falta | o registro do porte não expõe o evento; a segunda é proposta |

Os testes de ponta a ponta estão em `crates/je-workbench/tests/extension_host_api_text_editors.rs` (monta o `MainThreadDocumentsAndEditors` real sobre o
`ModelService` e um `TestCodeEditorService`) e `extension_host_api_env_extensions.rs` (binder com ambiente e área de transferência, e o registro de
extensões real). O `showTextDocument` que resolve um editor e o `openTextDocument` de um recurso sem modelo não têm teste (pedem um `IEditorPane` e um
`ITextModelService` completos).

Pendências conhecidas do que acabou de entrar (a confirmar na primeira compilação): `TextDocument`, `TextEditor` e `Extension` são instâncias de classes
nativas com um `_id` (a identidade `a.document === b.document` falha, comparar por `uri.toString()` funciona); documentos e editores que já atravessaram a
fronteira ficam guardados até o host parar; `TextEditor#options` devolve cópia (só `editor.options = {...}` tem efeito); `getWordRangeAtPosition` não aceita
`regexp`; `packageJSON` e `exports` são cópias novas a cada leitura.

Como cada grupo entra em `vscode.window`: o `IMainThreadBinder` registra o cliente `MainThread*` correspondente (`MainThreadQuickOpen`,
`MainThreadOutputService`, `MainThreadProgress`, além de `MainThreadMessageService` e `MainThreadStatusBar` que já existiam). Para os canais
de saída o binder entrega também o `OutputEnvironment` (`IMainThreadBinder::output_environment`: pasta de logs, `ILoggerService` e criador de
diretórios do lado do host).

Os testes de ponta a ponta estão em `crates/je-workbench/tests/extension_host_api_window_quick_open.rs`,
`extension_host_api_window_output.rs` e `extension_host_api_window_progress.rs`: cada um liga o cliente `MainThread*` real sobre dublês do
serviço do workbench.

Pendências conhecidas do que acabou de entrar (a confirmar na primeira compilação): o item de seletor devolvido à extensão é a cópia que
atravessou a fronteira (comparar por `label`, não por identidade); `tooltip` de item só aceita texto; os argumentos extras de `trace` a
`error` são ignorados; a extensão chamadora é sempre `je.exthost`, então o progresso não é atribuído a extensão nenhuma.

## Estado da entrega de `languages` (2026-10-24, escrito sem compilar: o integrador confirma)

Fonte: **eu** (leitura de `api/languages.rs`, `api/language_types.rs` e dos serviços `ExtHostDiagnostics`, `ExtHostLanguages` e
`ExtHostLanguageFeatures`). Nenhum número de ativação foi medido.

| Membro | Estado | Onde |
| --- | --- | --- |
| `createDiagnosticCollection(name?)` com `set` (Uri, lista de entradas, `undefined`), `get`, `has`, `delete`, `clear`, `forEach`, `dispose`, `name` | entregue | `api/languages.rs`, sobre `ExtHostDiagnostics` e o `MainThreadDiagnostics` real |
| `getDiagnostics()`, `getDiagnostics(uri)`, `onDidChangeDiagnostics` | entregue | `api/languages.rs` |
| `getLanguages`, `setTextDocumentLanguage`, `match` | entregue | `api/languages.rs`, sobre `ExtHostLanguages` |
| `registerCompletionItemProvider`, `registerHoverProvider`, `registerDefinitionProvider`, `registerDeclarationProvider`, `registerImplementationProvider`, `registerTypeDefinitionProvider`, `registerCodeActionsProvider`, `registerDocumentFormattingEditProvider`, `registerDocumentRangeFormattingEditProvider`, `registerDocumentSymbolProvider` | entregue | `api/languages.rs`, sobre `ExtHostLanguageFeatures` |
| `Diagnostic`, `DiagnosticRelatedInformation`, `Location`, `TextEdit`, `MarkdownString` (construtor e propriedades), `SnippetString` (construtor e `value`), `CompletionItem`, `CompletionList`, `Hover`, `CodeAction`, `DocumentSymbol`, `CodeActionKind` (membros estáticos e `append`, `contains`, `intersects`) | entregue | `api/language_types.rs` |
| `DiagnosticSeverity`, `DiagnosticTag`, `SymbolKind`, `SymbolTag`, `CompletionItemKind`, `CompletionItemTag`, `CompletionTriggerKind`, `CodeActionTriggerKind` | entregue | `api/enums.rs` |
| `MarkdownString#appendText`, `appendMarkdown`, `appendCodeblock`, `SnippetString#append*`, `WorkspaceEdit`, `CodeAction#edit` | falta | o motor entrega objeto comum por cópia, então um método não altera o receptor; pede uma API de referência no `zjsc` para o `this` (a função por referência de provedor já existe) |
| `resolveCompletionItem`, `resolveCodeAction`, `provideDocumentRangesFormattingEdits`, `HoverContext#previousHover` | falta | o host não consegue perguntar se o provedor tem o método (`typeof provider.x === 'function'`), então os `has_*` valem `false` |
| `registerReferenceProvider`, `registerRenameProvider`, `registerSignatureHelpProvider`, `registerFoldingRangeProvider`, `registerCodeLensProvider`, `registerInlayHintsProvider`, `registerDocumentLinkProvider`, `registerDocumentHighlightProvider`, `registerOnTypeFormattingEditProvider`, `registerWorkspaceSymbolProvider`, `setLanguageConfiguration`, `createLanguageStatusItem` | falta | o serviço do host os tem, falta só a ponte |

Como entra em `vscode.languages`: o `IMainThreadBinder` registra `MainThreadDiagnostics`, `MainThreadLanguages` e `MainThreadLanguageFeatures` (os
registros de provedor também pedem `MainThreadDiagnostics`, porque o `ExtHostLanguageFeatures` lê os diagnósticos nas ações de código), além dos
documentos de texto. Os encaminhadores `Late` de `ExtHostDiagnostics`, `ExtHostLanguages` e `ExtHostLanguageFeatures` (este com os 65 métodos
do trait) entregam ao serviço real as chamadas que os clientes fazem nos construtores.

Mudança no `zjsc` (`crates/zjsc/src/api/embed_node.rs`): `NodeRunner::make_function_by_reference`, uma função nativa cujos argumentos que são objetos chegam
como `JsData::Handle` (identidade, protótipo e métodos de classe do usuário intactos), usada por todo `register*Provider` para o `this` do provedor funcionar.

Os testes de ponta a ponta estão em `crates/je-workbench/tests/extension_host_api_languages_diagnostics.rs` (`MainThreadDiagnostics` real sobre o
`MarkerService` real) e `extension_host_api_languages_providers.rs` (provedores chamados pelo proxy `ExtHostLanguageFeaturesShape` do host), com o
binder em `tests/support/extension_host_api_languages_support.rs`.

Pendências conhecidas do que acabou de entrar (a confirmar na primeira compilação): `Event` precisa ser `Clone` (o `onDidChangeDiagnostics` o clona);
`FormattingOptions` é lido por `tab_size` e `insert_spaces`; as instâncias de `DiagnosticCollection` são identificadas por `_id` (a identidade `a === b`
falha); `CodeActionKind` não é construtível (`new CodeActionKind(x)` não existe).

## Estado da entrega de `WorkspaceEdit` e `workspace.applyEdit` (2026-10-24, escrito sem compilar: o integrador confirma)

Fonte: **eu** (leitura de `crates/je-exthost/src/api/workspace_edit.rs`, `ext_host_types/edits.rs` e `ext_host_bulk_edits.rs`). Nenhum
número de ativação foi medido.

| Membro | Estado | Onde |
| --- | --- | --- |
| `new WorkspaceEdit()` com `replace`, `insert`, `delete`, `set`, `createFile`, `deleteFile`, `renameFile`, `has`, `get`, `entries`, `size`, `toJSON` | entregue (estado por id no lado Rust) | `api/workspace_edit.rs` |
| `workspace.applyEdit(edit, metadata?)` | entregue, ligado ao `ExtHostBulkEdits` (precisa do cliente `MainThreadBulkEdits`) | `api/workspace_edit.rs`, `host.rs` |
| `WorkspaceEdit#set` com `NotebookEdit`, `createFile` com `contents` | falta (erro explícito) | `NotebookEdit` não existe no host; `Uint8Array` não atravessa o motor |
| `CodeAction#edit` | falta (a conversão `WorkspaceEdit` para o provedor de code action ainda não foi ligada à classe) | `api/language_types.rs` |
| `createFileSystemWatcher`, `onDid/WillCreate/Delete/RenameFiles` | entregue (2026-10-24, escrito sem compilar), ver a seção seguinte | `api/file_system_events.rs` |

Teste: `crates/je-workbench/tests/extension_host_api_workspace_edit.rs` (nenhum rodou).

## Estado da entrega dos eventos de arquivo (2026-10-24, escrito sem compilar: o integrador confirma)

Fonte: **eu** (leitura de `api/file_system_events.rs`, `ext_host_file_system_event_service.rs` e `extHost.api.impl.ts`). Nenhum número de
ativação foi medido.

| Membro | Estado | Onde |
| --- | --- | --- |
| `workspace.createFileSystemWatcher(glob, ignoreCreate?, ignoreChange?, ignoreDelete?)` com `onDidCreate`, `onDidChange`, `onDidDelete`, `ignore*Events`, `dispose` | entregue, ligado ao `ExtHostFileSystemEventService` (precisa dos clientes `MainThreadFileSystemEventService`, workspace e configuração) | `api/file_system_events.rs`, `host.rs` |
| `workspace.onDidCreateFiles`, `onDidDeleteFiles`, `onDidRenameFiles` | entregue (copiar conta como criar; gravar não gera evento) | `api/file_system_events.rs` |
| `workspace.onWillCreateFiles`, `onWillDeleteFiles`, `onWillRenameFiles` com `token`, `files`, `waitUntil` | entregue; só a `WorkspaceEdit` que a promessa devolve entra na operação; `waitUntil` fora do despacho lança | `api/file_system_events.rs`, `events.rs` (`event_function_with_epilogue`) |
| `FileSystemWatcher` como objeto copiado | divergência: `dispose` e os eventos são funções próprias do objeto, e `extension` é o `je.exthost` | `api/file_system_events.rs` |

Teste: `crates/je-workbench/tests/extension_host_api_file_system_events.rs`, com os dublês de workspace em
`tests/support/extension_host_workspace_doubles.rs` (nenhum rodou). Pendências a confirmar na primeira compilação: o erro de `waitUntil` chega
ao JavaScript com a mensagem exata; o `RelativePattern` com `**/*.txt` casa em minúsculas contra `/w/...`; `ExtHostFileSystemInfoShape` ainda não
é ligado (o `accept_provider_infos` do workbench não chega ao host).

Correção posterior (o integrador mediu 5 de 6): `will_run_file_operation_collects_only_workspace_edits_from_wait_until` falhava porque o `Emitter` guarda cada
inscrição num contêiner novo, então o listener que o `PromiseJoin` recebe nunca era o registrado em `WillEmitter::listeners` (a busca por `Rc::ptr_eq` dava `None`,
a edição era descartada e o `$onWillRunFileOperation` devolvia `None`). O original guarda a `extension` como propriedade da própria função do listener; o porte agora
marca a extensão enquanto o listener embrulhado roda (`CurrentExtension`) e o `waitUntil`, síncrono, a lê no instante do `PromiseJoin`. Escrito sem compilar.

## Estado da entrega de `debug` (2026-10-25, escrito sem compilar: o integrador confirma)

Fonte: **eu** (leitura de `api/debug.rs`, `ext_host_debug_service.rs`, `ext_host_debug_types.rs` e do oráculo `extHost.api.impl.ts`). Nenhum número de ativação foi medido.

| Membro | Estado | Onde |
| --- | --- | --- |
| `debug.registerDebugConfigurationProvider(type, provider, triggerKind?)` (`provideDebugConfigurations`, `resolveDebugConfiguration`, `resolveDebugConfigurationWithSubstitutedVariables`, chamados por referência com o `this` do objeto) | entregue | `api/debug.rs`, sobre o `ExtHostDebugService` |
| `debug.registerDebugAdapterDescriptorFactory(type, factory)` com `DebugAdapterExecutable`, `DebugAdapterServer`, `DebugAdapterNamedPipeServer` | entregue (só a extensão do host que contribui o depurador registra, ver as divergências do módulo) | `api/debug.rs` |
| `debug.startDebugging(folder, nameOrConfig, parentSessionOrOptions)` com `DebugSessionOptions` (`parentSession`, `lifecycleManagedByParent`, `consoleMode`, `noDebug`, `compact`, `suppressDebug*`, `debugUI`), `stopDebugging(session?)` | entregue | `api/debug.rs` |
| `debug.activeDebugSession`, `onDidStartDebugSession`, `onDidTerminateDebugSession`, `onDidChangeActiveDebugSession`, `onDidReceiveDebugSessionCustomEvent`, `activeDebugConsole` (`append`, `appendLine`) | entregue | `api/debug.rs` |
| `DebugSession` (`id`, `type`, `name` com atribuição, `workspaceFolder`, `configuration`, `parentSession`, `customRequest`, `getDebugProtocolBreakpoint`) | entregue | `api/debug.rs` |
| `debug.breakpoints`, `addBreakpoints`, `removeBreakpoints`, `onDidChangeBreakpoints`, `SourceBreakpoint`, `FunctionBreakpoint`, `Breakpoint`, `DebugConsoleMode`, `DebugConfigurationProviderTriggerKind` | entregue | `api/debug.rs`, `api/enums.rs` |
| `registerDebugAdapterTrackerFactory`, `asDebugSourceUri`, `activeStackItem`, `onDidChangeActiveStackItem`, `DebugAdapterInlineImplementation`, `DataBreakpoint` construível, `DebugSessionOptions#testRun`, as propostas `debugVisualization` | falta | o serviço tem; o `DebugAdapterInlineImplementation` pede o adaptador por referência no construtor (a classe existe e rejeita com mensagem explícita) |

`debug` entra em `vscode` quando o binder registra `MainThreadDebugService`. O encaminhador `LateExtHostDebugService` (`in_process.rs`) entrega ao `ExtHostDebugService` real
as chamadas sem retorno do cliente. O host não tem o `IConfigurationResolverService` (`NoVariableResolver`: `$substituteVariables` rejeita com a explicação) nem o namespace
`tests` (`NoTesting`). O registro de extensões que o serviço pede ao nascer vem do `ExtensionServiceSlot`, que o assenta quando `bind_extension_service` liga o serviço.

Teste: `crates/je-workbench/tests/extension_host_api_debug.rs` (dublê do `MainThreadDebugServiceShape` que anota cada pedido; a extensão que define o depurador `mock` usa
`run_extension_contributing` do harness). Pendências a confirmar na primeira compilação: `define_accessor_class` com `parent` (`SourceBreakpoint` e `FunctionBreakpoint` herdam de
`Breakpoint`); o `$$class` de uma instância criada por `new DebugAdapterExecutable(...)` ao voltar como valor de promessa (há um reconhecimento por campos como alternativa);
`Debug` de `IDebugSessionReplMode` nas linhas do dublê; a ordem de `registerDebugTypes` na ativação.

Os dublês de workspace e de configuração dos testes `extension_host_api_*` moram só em `tests/support/extension_host_workspace_doubles.rs` (`WorkspaceClient`,
`StaticConfigurationService` semeável, `bind_workspace_clients`); `extension_host_api_workspace.rs` e `extension_host_api_file_system_events.rs` os usam.

## Estado da entrega de `comments` (2026-10-25, escrito sem compilar: o integrador confirma)

Fonte: **eu** (leitura de `api/comments.rs`, `ext_host_comments.rs`, `ext_host_types/comments.rs` e do oráculo `extHostComments.ts`). Nenhum número de ativação foi medido.

| Membro | Estado | Onde |
| --- | --- | --- |
| `comments.createCommentController(id, label)` com `id`, `label`, `options` (`prompt`, `placeHolder`), `commentingRangeProvider` (`provideCommentingRanges`, `Range[]` ou `{ ranges, enableFileComments }`, direto ou por promessa), `reactionHandler`, `createCommentThread(uri, range, comments)` e `dispose` | entregue, ligado ao `ExtHostComments` (precisa do cliente `MainThreadComments`) | `api/comments.rs`, `host.rs` |
| `CommentThread` com `uri`, `range`, `comments`, `collapsibleState`, `canReply` (booleano ou `{ name, iconPath }`), `contextValue`, `label`, `state` e `dispose` | entregue | `api/comments.rs` |
| `CommentThreadCollapsibleState`, `CommentMode`, `CommentState`, `CommentThreadState`, `CommentThreadApplicability`, `CommentThreadFocus` | entregue | `api/enums.rs` |
| `CommentThread#reveal` e `hide`, `applicability`, `CommentController#activeComment` e `activeCommentThread`, `resourceHints`, `timestamp` como `Date` | falta | propostas que o host nunca habilita (`commentReveal`, `commentThreadApplicability`, `activeComment`, `commentingRangeHint`); `Date` não atravessa o motor (só número) |
| argumentos de comando `CommentThread`, `CommentNode`... com o `this` da thread | falta | o `ArgumentProcessor` do `ExtHostCommentsImpl` está registrado, mas o `Dynamic` que ele devolve chega ao comando da extensão como objeto de dados |

O `commentingRangeProvider` e o `reactionHandler` chegam como cópia (a atribuição a uma propriedade não é por referência): o provedor vale como objeto literal com `provideCommentingRanges` PRÓPRIA, sem `this`; o comentário que o `reactionHandler` recebe é uma cópia. `comments` devolve o que a extensão atribuiu.

Mudanças fora de `api/comments.rs`, todas por DRY ou ligação: `convert::text_or_undefined`, `commands::call_settled` e `commands::convert_maybe` (antes locais em `scm.rs`, `debug.rs` e `quick_session.rs`, agora compartilhados), `api::{handle_object, handle_field, getter, setter}` (antes locais em `scm.rs`; o texto do erro de `this` inválido de `SourceControl` agora é gerado por `handle_field`, mesma frase), `LateExtHostComments` e o campo `comments` de `LateForwarders` em `in_process.rs`.

Teste: `crates/je-workbench/tests/extension_host_api_comments.rs` (dublê do `MainThreadCommentsShape` que anota cada pedido; os documentos de texto vêm de `languages_setup` do suporte de `languages`; `debounce` de 100 ms esperado com `sleep`). Pendências a confirmar na primeira compilação: os reexports de `CommentData`, `CommentingRanges` e `CommentMode` em `ext_host_types`; `define_accessor_class` com o getter de um objeto que carrega só `_id`; a mensagem exata do erro de `this` descartado; a ordem dos quatro primeiros pedidos de `t.commentsCreate`.

## Estado da entrega de `authentication` (2026-10-25, escrito sem compilar: o integrador confirma)

Fonte: **eu** (leitura de `ext_host_authentication.rs`, `main_thread_authentication.rs`, `ext_host_protocol.rs`, `api/comments.rs`, `api/tree_views.rs` e do oráculo `extHostAuthentication.ts`, `extHost.protocol.ts` e `extHost.api.impl.ts`). Nenhum número de ativação foi medido. O bloqueio anterior (nada portado) foi resolvido nesta entrega.

| Membro | Estado | Onde |
| --- | --- | --- |
| `authentication.getSession(providerId, scopes ou { wwwAuthenticate, fallbackScopes }, options)` com `createIfNone`, `forceNewSession`, `clearSessionPreference`, `silent`, `account`; pedidos iguais em andamento compartilham a promessa (`TaskSingler`) | entregue, ligado ao `ExtHostAuthentication` (precisa do cliente `MainThreadAuthentication`) | `api/authentication.rs`, `host.rs` |
| `authentication.getAccounts(providerId)` com o ícone da conta revivido | entregue | `api/authentication.rs` |
| `authentication.onDidChangeSessions` (filtro por extensão, provedores `__` ignorados) | entregue | `api/authentication.rs` |
| `authentication.registerAuthenticationProvider(id, label, provider, { supportsMultipleAccounts })` com o provedor POR REFERÊNCIA (`this` da classe vale), `getSessions`, `createSession`, `removeSession`, `onDidChangeSessions` (as mudanças disparadas antes do reconhecimento da thread principal ficam em buffer) e o `Disposable` devolvido | entregue | `api/authentication.rs`, `ext_host_authentication.rs` |
| `getSessionsFromChallenges` e `createSessionFromChallenges` do provedor | ligados no `ExtHostAuthentication` e no adaptador (`typeof ... === 'function'`), mas só alcançáveis com a proposta `authenticationChallenges`, que o host nunca habilita | `ext_host_authentication.rs` |
| `authentication.hasSession`, `options.authorizationServer`, `clientId`, `resource`, `audience`, `learnMore`, `supportedAuthorizationServers` | rejeitam como o original rejeitaria sem a proposta (`authSession`, `authIssuers`, `authSessionAudience`, `authLearnMore`) | `api/authentication.rs` |
| `DynamicAuthProvider`, `XaaifyAuthProvider`, `$registerDynamicAuthProvider`, `$registerXaaAuthProvider`, `$onDidChangeDynamicAuthProviderTokens` | falta (recorte declarado) | dependem de `fetchDynamicRegistration`, `fetch`, `URL`, `Response`, `IExtHostWindow`, `IExtHostUrlsService`, `IExtHostProgress` |

Protocolo novo em `ext_host_protocol.rs`: `MainThreadAuthenticationShape` (completo, 15 métodos), `ExtHostAuthenticationShape` (7 métodos: sem os 3 dinâmicos), os DTOs (`AuthenticationGetSessionOptions`, `AuthenticationInteractive`, `IRegisterAuthenticationProviderDetails`...) e os identificadores `MAIN_THREAD_AUTHENTICATION` e `EXT_HOST_AUTHENTICATION`. `get_authentication_session_request_key` (de `authentication.ts`) entrou em `services/authentication/authentication.rs` porque o `getSession` do host precisa dela.

Mudanças fora dos arquivos de `authentication`, por DRY ou ligação: `convert::exported` (o terceiro uso do mesmo helper de cópia de objeto por referência; `languages.rs` e `debug.rs` ainda têm cópias locais, não mexi), `LateExtHostAuthentication` e o campo `authentication` de `LateForwarders` em `in_process.rs`, `pub mod authentication` em `api/mod.rs`.

Divergências medidas por leitura, não por execução: o `vscode` é um só, então `getSession` pede em nome de `je.exthost` (o `extensionName` que chega à thread principal é `exthost`); as chaves de `options` desconhecidas não atravessam; as propriedades extras de uma `AuthenticationSession` do provedor não atravessam (o motor copia).

Teste: `crates/je-workbench/tests/extension_host_api_authentication.rs` (dublê do `MainThreadAuthenticationShape` que anota cada pedido e responde a sessão e as contas que o teste põe; `t.proposals`, `t.getSessionTwice`, `t.registerAndFire` e `t.registerTwice` cobrem as propostas, o `TaskSingler`, o buffer e o registro duplicado). Pendências a confirmar na primeira compilação: `is_none_or` e `Chain::filter`/`map` sobre `Event::chain`; a promoção de objetos por referência a `JsData::Handle` no `registerAuthenticationProvider` (inclusive `{}`); `emitter.event` chamado sem `this` por `JsObjects::listen`; a forma de um objeto com `undefined` exportado (`changed: undefined`).

## Estado da entrega de `notebooks` (2026-10-25, escrito sem compilar: o integrador confirma)

Fonte: **eu** (leitura de `ext_host_notebook.rs`, `ext_host_notebook_document.rs`, `ext_host_notebook_documents.rs`, `ext_host_notebook_kernels.rs`, `ext_host_types/notebook.rs`, `ext_host_protocol.rs`, `api/comments.rs`, `api/authentication.rs`, `api/js_object.rs` e do oráculo `extHost.api.impl.ts`). Nenhum número de ativação foi medido.

| Membro | Estado | Onde |
| --- | --- | --- |
| `notebooks.createNotebookController(id, notebookType, label, handler?)` com `id`, `notebookType`, `label`, `detail`, `description`, `supportedLanguages`, `supportsExecutionOrder`, `rendererScripts`, `executeHandler`, `interruptHandler` (atribuíveis), `onDidChangeSelectedNotebooks`, `createNotebookCellExecution(cell)`, `updateNotebookAffinity(notebook, affinity)` e `dispose` | entregue, ligado ao `ExtHostNotebookKernels` (precisa do cliente `MainThreadNotebookKernels`) | `api/notebooks.rs`, `host.rs` |
| `NotebookCellExecution` com `token`, `cell`, `executionOrder`, `start`, `end(success, endTime)`, `clearOutput`, `appendOutput`, `replaceOutput`, `appendOutputItems`, `replaceOutputItems` | entregue | `api/notebooks.rs` |
| `notebooks.registerNotebookCellStatusBarItemProvider(notebookType, provider)` com `provideCellStatusBarItems` (um item ou lista, direto ou por promessa) e `onDidChangeCellStatusBarItems`, provedor POR REFERÊNCIA | entregue, ligado ao `ExtHostNotebookController` | `api/notebooks.rs` |
| `workspace.notebookDocuments`, `openNotebookDocument(uri)` e `openNotebookDocument(notebookType, content?)`, `onDidOpenNotebookDocument`, `onDidCloseNotebookDocument` | entregue | `api/notebooks.rs`, `api/workspace.rs` |
| `NotebookDocument` (`uri`, `version`, `notebookType`, `isDirty`, `isUntitled`, `isClosed`, `metadata`, `cellCount`, `cellAt`, `getCells(range?)`, `save`) e `NotebookCell` (`index`, `notebook`, `kind`, `document`, `metadata`, `outputs`, `executionSummary`) | entregue (objetos com `_id` sobre os vivos do host) | `api/notebooks.rs` |
| `NotebookRange`, `NotebookData`, `NotebookCellData`, `NotebookCellOutput`, `NotebookCellOutputItem` (`text`, `json`, `stdout`, `stderr`, `error`, `bytes`), `NotebookCellStatusBarItem`, `NotebookCellKind`, `NotebookCellStatusBarAlignment`, `NotebookCellExecutionState`, `NotebookEditorRevealType`, `NotebookControllerAffinity` | entregue | `api/notebooks.rs`, `api/enums.rs` |
| `registerNotebookSerializer`, `onDidChangeNotebookDocument`, `onDidSaveNotebookDocument`, `onWillSaveNotebookDocument`, `window.activeNotebookEditor`, `visibleNotebookEditors`, `showNotebookDocument`, os eventos de editor de notebook e `NotebookEditor`, `NotebookEdit` em `WorkspaceEdit`, `createRendererMessaging` | falta | o serviço do host tem (`ExtHostNotebookDocuments` já está ligado e recebe os avisos do cliente), falta só a ponte; o serializador pede `Uint8Array` por referência |
| `NotebookController#onDidReceiveMessage`, `postMessage`, `asWebviewUri`, `variableProvider`, `createNotebookExecution`, `createNotebookControllerDetectionTask`, `registerKernelSourceActionProvider`, `NotebookCellStatusBarItem#accessibilityInformation`, o terceiro argumento de `NotebookCellExecution#end` | falta | propostas que o host nunca habilita (`notebookMessaging`, `notebookVariableProvider`, `notebookExecution`, `notebookKernelSource`) ou campo ainda não lido |

`notebooks` e os membros de `workspace` entram quando o binder registra `MainThreadNotebook`, `MainThreadNotebookDocuments` e `MainThreadNotebookEditors` (e os documentos de texto, porque cada célula é um documento de texto); `createNotebookController` pede também `MainThreadNotebookKernels`. Os encaminhadores `LateExtHostNotebook`, `LateExtHostNotebookDocuments` e `LateExtHostNotebookKernels` (`in_process.rs`) entregam ao serviço real as chamadas do cliente (o delta inicial de documentos fica na fila até o `bind`). O host não tem o `ExtHostConsumerFileSystem` (`NoConsumerFileSystem` em `host.rs`): nenhum esquema é gravável, então o `$saveNotebook` responde com o erro de arquivo somente leitura do original. Mudança fora do módulo: `api::types::marked` virou `pub(crate)`.

Divergências medidas por leitura, não por execução: a identidade (`a === b`) de notebook, célula e controlador não vale (comparar por `uri` ou `id`); `NotebookCellOutputItem#data` é uma lista de números, não um `Uint8Array` (o `Uint8Array` não atravessa o motor); `executeHandler` devolve `undefined` enquanto a extensão não atribuiu nenhum; o controlador é registrado em nome de `je.exthost`.

Teste: `crates/je-workbench/tests/extension_host_api_notebooks.rs` (dublê dos quatro `MainThreadNotebook*Shape`, `Panel`, que anota cada pedido e entrega os deltas de documentos, a associação, a execução e o pedido da barra de status pelos proxies do host; reaproveita `cell_dto` de `tests/support/ext_host_notebook_support.rs` e os documentos de `languages_setup`). Pendências a confirmar na primeira compilação: `Value::Array` ser `Vec<Value>` e `Object` construir de `(&str, Value)` em `json_value_of`; `then_result` sobre `Promise<URI>` no `openNotebookDocument(tipo)`; `Debug` de `CellStatusbarAlignment` (a asserção compara `"Right"`); o texto exato do erro de `NotebookController` descartado; o `sleep` de 50 ms antes do `pump` na execução (o coletor de atualizações da célula usa um temporizador de 10 ms, mas `end` o esvazia na hora); os campos `execution_order`, `run_start_time`, `last_run_success` e `run_end_time` serem os lidos do dublê.

