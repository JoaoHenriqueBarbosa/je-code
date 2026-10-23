// Um `vscode` falso para a sonda H15: só o que as extensões testadas chamam.
// Os membros que não estão implementados caem num stub profundo que REGISTRA o caminho acessado
// (`vscode.workspace.getConfiguration`), e esse registro é a lista medida do que o extension host
// completo precisa implementar.
'use strict';

const touched = new Map();

function note(path, kind) {
  const key = path + ' [' + kind + ']';
  touched.set(key, (touched.get(key) || 0) + 1);
}

function stub(path) {
  const target = function () {};
  return new Proxy(target, {
    get(_t, prop) {
      if (prop === Symbol.toPrimitive) return () => '';
      if (prop === 'then') return undefined;
      if (typeof prop === 'symbol') return undefined;
      note(path + '.' + prop, 'get-stub');
      return stub(path + '.' + prop);
    },
    apply() {
      note(path, 'call-stub');
      return stub(path + '()');
    },
    construct() {
      note(path, 'new-stub');
      return stub('new ' + path);
    },
  });
}

class Disposable {
  constructor(fn) { this.fn = fn; }
  dispose() { if (this.fn) { const f = this.fn; this.fn = undefined; f(); } }
  static from(...items) { return new Disposable(() => items.forEach((i) => i.dispose && i.dispose())); }
}

class EventEmitter {
  constructor() {
    this.listeners = [];
    this.event = (listener, thisArgs, disposables) => {
      const bound = thisArgs ? listener.bind(thisArgs) : listener;
      this.listeners.push(bound);
      const d = new Disposable(() => { this.listeners = this.listeners.filter((l) => l !== bound); });
      if (disposables) disposables.push(d);
      return d;
    };
  }
  fire(data) { for (const l of [...this.listeners]) l(data); }
  dispose() { this.listeners = []; }
}

class Uri {
  constructor(scheme, authority, path, query, fragment) {
    this.scheme = scheme; this.authority = authority; this.path = path; this.query = query; this.fragment = fragment;
  }
  get fsPath() { return this.path; }
  static file(p) { return new Uri('file', '', p, '', ''); }
  static parse(v) { const m = /^([a-z][a-z0-9+.-]*):\/\/([^/]*)([^?#]*)/i.exec(v) || []; return new Uri(m[1] || '', m[2] || '', m[3] || v, '', ''); }
  with(c) { return new Uri(c.scheme ?? this.scheme, c.authority ?? this.authority, c.path ?? this.path, c.query ?? this.query, c.fragment ?? this.fragment); }
  toString() { return this.scheme + '://' + this.authority + this.path; }
}

class Position { constructor(line, character) { this.line = line; this.character = character; } }
class Range {
  constructor(a, b, c, d) {
    if (typeof a === 'number') { this.start = new Position(a, b); this.end = new Position(c, d); } else { this.start = a; this.end = b; }
  }
}

const registeredCommands = new Map();
const state = { packageJSON: {} };
const events = [];

const overrides = {
  Disposable, EventEmitter, Uri, Position, Range,
  version: '1.105.0',
  l10n: { t: (m, ...a) => (typeof m === 'string' ? m : m.message) },
  commands: {
    registerCommand(id, fn) { registeredCommands.set(id, fn); return new Disposable(() => registeredCommands.delete(id)); },
    registerTextEditorCommand(id, fn) { registeredCommands.set(id, fn); return new Disposable(() => registeredCommands.delete(id)); },
    executeCommand(id) { note('commands.executeCommand(' + id + ')', 'call'); return Promise.resolve(undefined); },
  },
  extensions: { all: [], onDidChange: () => new Disposable(), getExtension: () => ({ packageJSON: state.packageJSON }) },
  workspace: {
    workspaceFolders: undefined,
    getConfiguration() { note('workspace.getConfiguration', 'call'); return { get: (k, d) => d, has: () => false, inspect: () => undefined, update: () => Promise.resolve() }; },
    onDidChangeConfiguration: (...a) => { note('workspace.onDidChangeConfiguration', 'event'); return new Disposable(); },
  },
  window: {
    visibleTextEditors: [],
    activeTextEditor: undefined,
    createOutputChannel() { note('window.createOutputChannel', 'call'); return { appendLine() {}, append() {}, show() {}, dispose() {} }; },
  },
};

// Os nomes de primeiro nível da API (`vscode.d.ts`): o `__toESM` do esbuild copia só as propriedades próprias,
// então cada nome precisa existir como propriedade, real ou stub.
const API_NAMES = ["authentication", "BranchCoverage", "Breakpoint", "CallHierarchyIncomingCall", "CallHierarchyItem", "CallHierarchyOutgoingCall", "CancellationError", "CancellationTokenSource", "chat", "ChatRequestTurn", "ChatResponseAnchorPart", "ChatResponseCommandButtonPart", "ChatResponseFileTreePart", "ChatResponseMarkdownPart", "ChatResponseProgressPart", "ChatResponseReferencePart", "ChatResponseTurn", "ChatResultFeedbackKind", "CodeAction", "CodeActionKind", "CodeActionTriggerKind", "CodeLens", "Color", "ColorInformation", "ColorPresentation", "ColorThemeKind", "commands", "CommentMode", "CommentThreadCollapsibleState", "CommentThreadState", "CompletionItem", "CompletionItemKind", "CompletionItemTag", "CompletionList", "CompletionTriggerKind", "ConfigurationTarget", "CustomExecution", "DataTransfer", "DataTransferItem", "debug", "DebugAdapterExecutable", "DebugAdapterInlineImplementation", "DebugAdapterNamedPipeServer", "DebugAdapterServer", "DebugConfigurationProviderTriggerKind", "DebugConsoleMode", "DebugStackFrame", "DebugThread", "DeclarationCoverage", "DecorationRangeBehavior", "Diagnostic", "DiagnosticRelatedInformation", "DiagnosticSeverity", "DiagnosticTag", "Disposable", "DocumentDropEdit", "DocumentDropOrPasteEditKind", "DocumentHighlight", "DocumentHighlightKind", "DocumentLink", "DocumentPasteEdit", "DocumentPasteTriggerKind", "DocumentSymbol", "EndOfLine", "env", "EnvironmentVariableMutatorType", "EvaluatableExpression", "EventEmitter", "ExtensionKind", "ExtensionMode", "extensions", "FileChangeType", "FileCoverage", "FileDecoration", "FilePermission", "FileSystemError", "FileType", "FoldingRange", "FoldingRangeKind", "FunctionBreakpoint", "Hover", "IndentAction", "InlayHint", "InlayHintKind", "InlayHintLabelPart", "InlineCompletionItem", "InlineCompletionList", "InlineCompletionTriggerKind", "InlineValueEvaluatableExpression", "InlineValueText", "InlineValueVariableLookup", "InputBoxValidationSeverity", "l", "LanguageModelChatMessage", "LanguageModelChatMessageRole", "LanguageModelChatToolMode", "LanguageModelDataPart", "LanguageModelError", "LanguageModelPromptTsxPart", "LanguageModelTextPart", "LanguageModelToolCallPart", "LanguageModelToolResult", "LanguageModelToolResultPart", "languages", "LanguageStatusSeverity", "LinkedEditingRanges", "lm", "Location", "LogLevel", "MarkdownString", "McpHttpServerDefinition", "McpStdioServerDefinition", "NotebookCellData", "NotebookCellKind", "NotebookCellOutput", "NotebookCellOutputItem", "NotebookCellStatusBarAlignment", "NotebookCellStatusBarItem", "NotebookControllerAffinity", "NotebookData", "NotebookEdit", "NotebookEditorRevealType", "NotebookRange", "notebooks", "OverviewRulerLane", "ParameterInformation", "Position", "ProcessExecution", "ProgressLocation", "QuickInputButtonLocation", "QuickInputButtons", "QuickPickItemKind", "Range", "RelativePattern", "scm", "Selection", "SelectionRange", "SemanticTokens", "SemanticTokensBuilder", "SemanticTokensEdit", "SemanticTokensEdits", "SemanticTokensLegend", "ShellExecution", "ShellQuoting", "SignatureHelp", "SignatureHelpTriggerKind", "SignatureInformation", "SnippetString", "SnippetTextEdit", "SourceBreakpoint", "StatementCoverage", "StatusBarAlignment", "SymbolInformation", "SymbolKind", "SymbolTag", "SyntaxTokenType", "TabInputCustom", "TabInputNotebook", "TabInputNotebookDiff", "TabInputTerminal", "TabInputText", "TabInputTextDiff", "TabInputWebview", "Task", "TaskGroup", "TaskPanelKind", "TaskRevealKind", "tasks", "TaskScope", "TelemetryTrustedValue", "TerminalExitReason", "TerminalLink", "TerminalLocation", "TerminalProfile", "TerminalShellExecutionCommandLineConfidence", "TestCoverageCount", "TestMessage", "TestMessageStackFrame", "TestRunProfileKind", "TestRunRequest", "tests", "TestTag", "TextDocumentChangeReason", "TextDocumentSaveReason", "TextEdit", "TextEditorCursorStyle", "TextEditorLineNumbersStyle", "TextEditorRevealType", "TextEditorSelectionChangeKind", "ThemeColor", "ThemeIcon", "TreeItem", "TreeItemCheckboxState", "TreeItemCollapsibleState", "TypeHierarchyItem", "UIKind", "Uri", "version", "ViewColumn", "window", "workspace", "WorkspaceEdit"];
const vscode = {};
for (const name of API_NAMES) {
  if (name in overrides) continue;
  let cached;
  Object.defineProperty(overrides, name, {
    enumerable: true,
    configurable: true,
    get() { note('vscode.' + name, 'get-stub'); return cached ??= stub('vscode.' + name); },
  });
}
for (const key of Object.keys(overrides)) {
  Object.defineProperty(vscode, key, { enumerable: true, configurable: true, get() { if (!Object.getOwnPropertyDescriptor(overrides, key).get) note('vscode.' + key, 'real'); return overrides[key]; } });
}
// Os subobjetos reais também viram proxies que registram o acesso a membros que faltam.
for (const key of ['workspace', 'window', 'commands', 'l10n', 'extensions']) {
  const real = overrides[key];
  overrides[key] = new Proxy(real, {
    get(target, prop) {
      if (prop in target) { note('vscode.' + key + '.' + String(prop), 'real'); return target[prop]; }
      if (typeof prop === 'symbol' || prop === 'then') return undefined;
      note('vscode.' + key + '.' + prop, 'get-stub');
      return stub('vscode.' + key + '.' + prop);
    },
  });
}

module.exports = { vscode, touched, registeredCommands, state };
