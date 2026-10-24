//! As enumerações de `extHostTypes.ts` que não pertencem a uma classe: valores iguais aos do original.
//!
//! Divergências: `TaskEventKind` e `ChatImageMimeType` são enumerações de texto no original e aqui têm
//! `as_str`. `SyntaxTokenType.toString` e `TextEditorSelectionChangeKind.fromValue` (os `namespace` que
//! se fundem ao `enum`) são as funções associadas [`SyntaxTokenType::label`] e
//! [`TextEditorSelectionChangeKind::from_source`].

// Os nomes dos membros são os da API (`LLM_RANKED`, `UI`), nunca renomeados.
#![allow(non_camel_case_types)]

api_enum!(TerminalOutputAnchor { Top = 0, Bottom = 1 });
api_enum!(TerminalQuickFixType { TerminalCommand = 0, Opener = 1, Command = 3 });
api_enum!(EnvironmentVariableMutatorType { Replace = 1, Append = 2, Prepend = 3 });
api_enum!(HoverVerbosityAction { Increase = 0, Decrease = 1 });
api_enum!(DocumentHighlightKind { Text = 0, Read = 1, Write = 2 });
api_enum!(CodeActionTriggerKind { Invoke = 1, Automatic = 2 });
api_enum!(LanguageStatusSeverity { Information = 0, Warning = 1, Error = 2 });
api_enum!(SignatureHelpTriggerKind { Invoke = 1, TriggerCharacter = 2, ContentChange = 3 });
api_enum!(InlayHintKind { Type = 1, Parameter = 2 });
api_enum!(CompletionTriggerKind { Invoke = 0, TriggerCharacter = 1, TriggerForIncompleteCompletions = 2 });
api_enum!(CompletionItemKind {
    Text = 0, Method = 1, Function = 2, Constructor = 3, Field = 4, Variable = 5, Class = 6, Interface = 7,
    Module = 8, Property = 9, Unit = 10, Value = 11, Enum = 12, Keyword = 13, Snippet = 14, Color = 15,
    File = 16, Reference = 17, Folder = 18, EnumMember = 19, Constant = 20, Struct = 21, Event = 22,
    Operator = 23, TypeParameter = 24, User = 25, Issue = 26
});
api_enum!(CompletionItemTag { Deprecated = 1 });
api_enum!(PartialAcceptTriggerKind { Unknown = 0, Word = 1, Line = 2, Suggest = 3 });
api_enum!(InlineCompletionEndOfLifeReasonKind { Accepted = 0, Rejected = 1, Ignored = 2 });
api_enum!(InlineCompletionDisplayLocationKind { Code = 1, Label = 2 });
api_enum!(StatusBarAlignment { Left = 1, Right = 2 });
api_enum!(TextEditorLineNumbersStyle { Off = 0, On = 1, Relative = 2, Interval = 3 });
api_enum!(TextDocumentSaveReason { Manual = 1, AfterDelay = 2, FocusOut = 3 });
api_enum!(TextEditorRevealType { Default = 0, InCenter = 1, InCenterIfOutsideViewport = 2, AtTop = 3 });
api_enum!(TextEditorSelectionChangeKind { Keyboard = 1, Mouse = 2, Command = 3 });
api_enum!(TextEditorChangeKind { Addition = 1, Deletion = 2, Modification = 3 });
api_enum!(TextDocumentChangeReason { Undo = 1, Redo = 2 });
// Valores iguais aos de `TrackedRangeStickiness`: OpenOpen = AlwaysGrowsWhenTypingAtEdges,
// ClosedClosed = NeverGrowsWhenTypingAtEdges, OpenClosed = GrowsOnlyWhenTypingBefore,
// ClosedOpen = GrowsOnlyWhenTypingAfter.
api_enum!(DecorationRangeBehavior { OpenOpen = 0, ClosedClosed = 1, OpenClosed = 2, ClosedOpen = 3 });
api_enum!(SyntaxTokenType { Other = 0, Comment = 1, String = 2, RegEx = 3 });
api_enum!(ColorFormat { RGB = 0, HEX = 1, HSL = 2 });
api_enum!(SourceControlInputBoxValidationType { Error = 0, Warning = 1, Information = 2 });
api_enum!(TerminalExitReason { Unknown = 0, Shutdown = 1, Process = 2, User = 3, Extension = 4 });
api_enum!(TerminalShellExecutionCommandLineConfidence { Low = 0, Medium = 1, High = 2 });
api_enum!(TerminalShellType {
    Sh = 1, Bash = 2, Fish = 3, Csh = 4, Ksh = 5, Zsh = 6, CommandPrompt = 7, GitBash = 8, PowerShell = 9,
    Python = 10, Julia = 11, NuShell = 12, Node = 13, Xonsh = 14
});
api_enum!(TerminalLocation { Panel = 1, Editor = 2 });
api_enum!(TerminalCompletionItemKind {
    File = 0, Folder = 1, Method = 2, Alias = 3, Argument = 4, Option = 5, OptionValue = 6, Flag = 7,
    SymbolicLinkFile = 8, SymbolicLinkFolder = 9, ScmCommit = 10, ScmBranch = 11, ScmTag = 12,
    ScmStash = 13, ScmRemote = 14, PullRequest = 15, PullRequestDone = 16
});
api_enum!(TaskRevealKind { Always = 1, Silent = 2, Never = 3 });
api_enum!(TaskPanelKind { Shared = 1, Dedicated = 2, New = 3 });
api_enum!(ShellQuoting { Escape = 1, Strong = 2, Weak = 3 });
api_enum!(TaskScope { Global = 1, Workspace = 2 });
api_enum!(TaskRunOn { Default = 1, FolderOpen = 2, WorktreeCreated = 3 });
api_enum!(ProgressLocation { SourceControl = 1, Window = 10, Notification = 15 });
api_enum!(TreeItemCollapsibleState { None = 0, Collapsed = 1, Expanded = 2 });
api_enum!(TreeItemCheckboxState { Unchecked = 0, Checked = 1 });
api_enum!(DocumentPasteTriggerKind { Automatic = 0, PasteAs = 1 });
api_enum!(ConfigurationTarget { Global = 1, Workspace = 2, WorkspaceFolder = 3 });
api_enum!(InlineCompletionTriggerKind { Invoke = 0, Automatic = 1 });
api_enum!(InlineCompletionsDisposeReasonKind { Other = 0, Empty = 1, TokenCancellation = 2, LostRace = 3, NotTaken = 4 });
api_enum!(NewSymbolNameTag { AIGenerated = 1 });
api_enum!(NewSymbolNameTriggerKind { Invoke = 0, Automatic = 1 });
api_enum!(FileChangeType { Changed = 1, Created = 2, Deleted = 3 });
api_enum!(FoldingRangeKind { Comment = 1, Imports = 2, Region = 3 });
api_enum!(CommentThreadCollapsibleState { Collapsed = 0, Expanded = 1 });
api_enum!(CommentMode { Editing = 0, Preview = 1 });
api_enum!(CommentState { Published = 0, Draft = 1 });
api_enum!(CommentThreadState { Unresolved = 0, Resolved = 1 });
api_enum!(CommentThreadApplicability { Current = 0, Outdated = 1 });
api_enum!(CommentThreadFocus { Reply = 1, Comment = 2 });
api_enum!(DebugConsoleMode { Separate = 0, MergeWithParent = 1 });
api_enum!(QuickInputButtonLocation { Title = 1, Inline = 2, Input = 3 });
api_enum!(QuickPickItemKind { Separator = -1, Default = 0 });
api_enum!(InputBoxValidationSeverity { Info = 1, Warning = 2, Error = 3 });
api_enum!(ExtensionKind { UI = 1, Workspace = 2 });
api_enum!(ColorThemeKind { Light = 1, Dark = 2, HighContrast = 3, HighContrastLight = 4 });
api_enum!(NotebookCellExecutionState { Idle = 1, Pending = 2, Executing = 3 });
api_enum!(NotebookCellStatusBarAlignment { Left = 1, Right = 2 });
api_enum!(NotebookEditorRevealType { Default = 0, InCenter = 1, InCenterIfOutsideViewport = 2, AtTop = 3 });
api_enum!(NotebookControllerAffinity { Default = 1, Preferred = 2 });
api_enum!(NotebookControllerAffinity2 { Default = 1, Preferred = 2, Hidden = -1 });
api_enum!(NotebookVariablesRequestKind { Named = 1, Indexed = 2 });
api_enum!(TimelineTriggerKind { Invoke = 1, Automatic = 2 });
api_enum!(ExtensionMode { Production = 1, Development = 2, Test = 3 });
api_enum!(ExtensionRuntime { Node = 1, Webworker = 2 });
api_enum!(StandardTokenType { Other = 0, Comment = 1, String = 2, RegEx = 3 });
api_enum!(SyntaxHighlightingTokenFontStyle { None = 0, Italic = 1, Bold = 2, Underline = 4, Strikethrough = 8 });
api_enum!(TestResultState { Queued = 1, Running = 2, Passed = 3, Failed = 4, Skipped = 5, Errored = 6 });
api_enum!(TestRunProfileKind { Run = 1, Debug = 2, Coverage = 3 });
api_enum!(ExternalUriOpenerPriority { None = 0, Option = 1, Default = 2, Preferred = 3 });
api_enum!(WorkspaceTrustState { Untrusted = 0, Trusted = 1, Unspecified = 2 });
api_enum!(PortAutoForwardAction { Notify = 1, OpenBrowser = 2, OpenPreview = 3, Silent = 4, Ignore = 5, OpenBrowserOnce = 6 });
api_enum!(InteractiveSessionVoteDirection { Down = 0, Up = 1 });
api_enum!(ChatCopyKind { Action = 1, Toolbar = 2 });
api_enum!(ChatVariableLevel { Short = 1, Medium = 2, Full = 3 });
api_enum!(ChatEditingSessionActionOutcome { Accepted = 1, Rejected = 2, Saved = 3 });
api_enum!(ChatRequestEditedFileEventKind { Keep = 1, Undo = 2, UserModification = 3 });
api_enum!(InteractiveEditorResponseFeedbackKind { Unhelpful = 0, Helpful = 1, Undone = 2, Accepted = 3, Bug = 4 });
api_enum!(ChatResultFeedbackKind { Unhelpful = 0, Helpful = 1 });
api_enum!(ChatQuestionType { Text = 1, SingleSelect = 2, MultiSelect = 3 });
api_enum!(ChatTodoStatus { NotStarted = 1, InProgress = 2, Completed = 3 });
api_enum!(ChatDebugSubagentStatus { Running = 0, Completed = 1, Failed = 2 });
api_enum!(ChatLocation { Panel = 1, Terminal = 2, Notebook = 3, Editor = 4 });
api_enum!(ChatSessionStatus { Failed = 0, Completed = 1, InProgress = 2, NeedsInput = 3 });
api_enum!(ChatDebugLogLevel { Trace = 0, Info = 1, Warning = 2, Error = 3 });
api_enum!(ChatDebugToolCallResult { Success = 0, Error = 1 });
api_enum!(ChatDebugHookResult { Success = 0, Error = 1, NonBlockingError = 2 });
api_enum!(ChatDebugMessageContentType { User = 0, Agent = 1 });
api_enum!(ChatResponseReferencePartStatusKind { Complete = 1, Partial = 2, Omitted = 3 });
api_enum!(ChatResponseClearToPreviousToolInvocationReason { NoReason = 0, FilteredContentRetry = 1, CopyrightContentRetry = 2 });
api_enum!(LanguageModelChatApiType { ChatCompletions = 1, Responses = 2, Messages = 3 });
api_enum!(LanguageModelChatMessageRole { User = 1, Assistant = 2, System = 3 });
api_enum!(ChatErrorLevel { Info = 0, Warning = 1, Error = 2 });
api_enum!(ChatInputNotificationSeverity { Info = 0, Warning = 1, Error = 2 });
api_enum!(LanguageModelPartAudience { Assistant = 0, User = 1, Extension = 2 });
api_enum!(LanguageModelChatToolMode { Auto = 1, Required = 2 });
api_enum!(RelatedInformationType { SymbolInformation = 1, CommandInformation = 2, SearchInformation = 3, SettingInformation = 4 });
api_enum!(SettingsSearchResultKind { EMBEDDED = 1, LLM_RANKED = 2, CANCELED = 3 });
api_enum!(SpeechToTextStatus { Started = 1, Recognizing = 2, Recognized = 3, Stopped = 4, Error = 5 });
api_enum!(TextToSpeechStatus { Started = 1, Stopped = 2, Error = 3 });
api_enum!(KeywordRecognitionStatus { Recognized = 1, Stopped = 2 });
api_enum!(McpToolAvailability { Initial = 0, Dynamic = 1 });

impl SyntaxTokenType {
    /// `SyntaxTokenType.toString(v)`: o nome da API; qualquer valor desconhecido é `other`.
    pub fn label(value: Option<i32>) -> &'static str {
        match value.and_then(Self::from_value) {
            Some(Self::Comment) => "comment",
            Some(Self::String) => "string",
            Some(Self::RegEx) => "regex",
            Some(Self::Other) | None => "other",
        }
    }
}

impl TextEditorSelectionChangeKind {
    /// `TextEditorSelectionChangeKind.fromValue(s)`: `s` é um `TextEditorSelectionSource`
    /// (`'api'`, `'code.navigation'`, `'code.jump'`) ou `'keyboard'`/`'mouse'`.
    pub fn from_source(source: Option<&str>) -> Option<Self> {
        match source? {
            "keyboard" => Some(Self::Keyboard),
            "mouse" => Some(Self::Mouse),
            "api" | "code.jump" | "code.navigation" => Some(Self::Command),
            _ => None,
        }
    }
}

/// `TaskEventKind` (enumeração de texto).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TaskEventKind {
    Changed,
    ProcessStarted,
    ProcessEnded,
    Terminated,
    Start,
    AcquiredInput,
    DependsOnStarted,
    Active,
    Inactive,
    End,
    ProblemMatcherStarted,
    ProblemMatcherEnded,
    ProblemMatcherFoundErrors,
}

impl TaskEventKind {
    /// O valor de texto do membro no original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Changed => "changed",
            Self::ProcessStarted => "processStarted",
            Self::ProcessEnded => "processEnded",
            Self::Terminated => "terminated",
            Self::Start => "start",
            Self::AcquiredInput => "acquiredInput",
            Self::DependsOnStarted => "dependsOnStarted",
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::End => "end",
            Self::ProblemMatcherStarted => "problemMatcherStarted",
            Self::ProblemMatcherEnded => "problemMatcherEnded",
            Self::ProblemMatcherFoundErrors => "problemMatcherFoundErrors",
        }
    }
}

/// `ChatImageMimeType` (enumeração de texto).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChatImageMimeType {
    Png,
    Jpeg,
    Gif,
    Webp,
    Bmp,
}

impl ChatImageMimeType {
    /// O valor de texto do membro no original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
            Self::Bmp => "image/bmp",
        }
    }
}
