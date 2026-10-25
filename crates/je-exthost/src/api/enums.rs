//! As enumerações numéricas da API `vscode` que os namespaces entregues usam (`StatusBarAlignment`, `ConfigurationTarget`, `UIKind`,
//! `ExtensionKind`, `QuickPickItemKind`, `InputBoxValidationSeverity`, `ProgressLocation`, `LogLevel`, `ViewColumn` e as do editor, e as de
//! linguagem: `DiagnosticSeverity`, `DiagnosticTag`, `SymbolKind`, `SymbolTag`, `CompletionItemKind`, `CompletionItemTag`,
//! `CompletionTriggerKind`, `CodeActionTriggerKind`, `DocumentHighlightKind`, `FoldingRangeKind`, `InlayHintKind`,
//! `SignatureHelpTriggerKind`) e o objeto `QuickInputButtons`.
//! Cada enumeração é o objeto que o TypeScript gera para um `enum`: nome para valor e valor para nome (`StatusBarAlignment.Left === 1` e
//! `StatusBarAlignment[1] === 'Left'`). Os valores são os de `ext_host_types::enums`.
//!
//! Divergências:
//! - `QuickInputButtons.Back` é um objeto comum `{ iconPath }` copiado a cada leitura (o motor copia objetos comuns): comparar um
//!   botão com `QuickInputButtons.Back` por identidade (`button === QuickInputButtons.Back`) falha, comparar por `iconPath.id` funciona.

use je_base::themables::ThemeIcon;
use je_editor::language_configuration::IndentAction;
use je_platform::files::files::FileType;
use je_platform::log::LogLevel;
use je_workbench::api::common::ext_host_debug_types::DebugConsoleMode;
use je_workbench::api::common::ext_host_file_system_consumer::ApiFilePermission;
use je_workbench::contrib::debug::debug::DebugConfigurationProviderTriggerKind;
use je_workbench::api::common::ext_host_types::{
    CodeActionTriggerKind, CommentMode, CommentState, CommentThreadApplicability, CommentThreadCollapsibleState, CommentThreadFocus, CommentThreadState,
    CompletionItemKind, CompletionItemTag, CompletionTriggerKind, ConfigurationTarget, DiagnosticSeverity, DiagnosticTag, DocumentHighlightKind, ExtensionKind,
    FileChangeType, FoldingRangeKind, InlayHintKind, InputBoxValidationSeverity, LanguageStatusSeverity, NotebookCellExecutionState, NotebookCellKind, NotebookCellStatusBarAlignment,
    NotebookControllerAffinity, NotebookEditorRevealType, ProgressLocation, QuickPickItemKind, SignatureHelpTriggerKind, StatusBarAlignment, SymbolKind, SymbolTag,
};
use zjsc::api::embed_node::JsData;

use crate::convert::api_to_js;

/// O objeto do `enum`: cada membro nos dois sentidos.
fn enum_object(members: &[(&str, i32)]) -> JsData {
    let mut fields: Vec<(String, JsData)> = Vec::with_capacity(members.len() * 2);
    for (name, value) in members {
        fields.push(((*name).to_owned(), JsData::Num(f64::from(*value))));
    }
    for (name, value) in members {
        fields.push((value.to_string(), JsData::Str((*name).to_owned())));
    }
    JsData::Object(fields)
}

/// Os membros de `vscode.SymbolKind`, do `symbols.rs` de `ext_host_types`.
fn symbol_kind_members() -> Vec<(&'static str, i32)> {
    use SymbolKind::*;
    [
        File, Module, Namespace, Package, Class, Method, Property, Field, Constructor, Enum, Interface, Function, Variable, Constant, String, Number, Boolean, Array,
        Object, Key, Null, EnumMember, Struct, Event, Operator, TypeParameter,
    ]
    .into_iter()
    .map(|kind| (kind.name(), kind.value()))
    .collect()
}

/// Os membros de `vscode.CompletionItemKind`.
fn completion_item_kind_members() -> Vec<(&'static str, i32)> {
    use CompletionItemKind::*;
    [
        Text, Method, Function, Constructor, Field, Variable, Class, Interface, Module, Property, Unit, Value, Enum, Keyword, Snippet, Color, File, Reference, Folder,
        EnumMember, Constant, Struct, Event, Operator, TypeParameter, User, Issue,
    ]
    .into_iter()
    .map(|kind| (kind.name(), kind.value()))
    .collect()
}

/// O valor numérico de `vscode.LogLevel` (`Off = 0`, `Trace = 1`, `Debug = 2`, `Info = 3`, `Warning = 4`, `Error = 5`).
pub fn log_level_value(level: LogLevel) -> i32 {
    match level {
        LogLevel::Off => 0,
        LogLevel::Trace => 1,
        LogLevel::Debug => 2,
        LogLevel::Info => 3,
        LogLevel::Warning => 4,
        LogLevel::Error => 5,
    }
}

/// As enumerações como entradas do objeto `vscode`.
pub fn enums() -> Vec<(String, JsData)> {
    vec![
        (
            "StatusBarAlignment".to_owned(),
            enum_object(&[("Left", StatusBarAlignment::Left.value()), ("Right", StatusBarAlignment::Right.value())]),
        ),
        (
            "ConfigurationTarget".to_owned(),
            enum_object(&[
                ("Global", ConfigurationTarget::Global.value()),
                ("Workspace", ConfigurationTarget::Workspace.value()),
                ("WorkspaceFolder", ConfigurationTarget::WorkspaceFolder.value()),
            ]),
        ),
        ("ExtensionKind".to_owned(), enum_object(&[("UI", ExtensionKind::UI.value()), ("Workspace", ExtensionKind::Workspace.value())])),
        // `UIKind` (`extHostTypes.ts`): `Desktop = 1`, `Web = 2`.
        ("UIKind".to_owned(), enum_object(&[("Desktop", 1), ("Web", 2)])),
        (
            "QuickPickItemKind".to_owned(),
            enum_object(&[("Separator", QuickPickItemKind::Separator.value()), ("Default", QuickPickItemKind::Default.value())]),
        ),
        (
            "InputBoxValidationSeverity".to_owned(),
            enum_object(&[
                ("Info", InputBoxValidationSeverity::Info.value()),
                ("Warning", InputBoxValidationSeverity::Warning.value()),
                ("Error", InputBoxValidationSeverity::Error.value()),
            ]),
        ),
        (
            "ProgressLocation".to_owned(),
            enum_object(&[
                ("SourceControl", ProgressLocation::SourceControl.value()),
                ("Window", ProgressLocation::Window.value()),
                ("Notification", ProgressLocation::Notification.value()),
            ]),
        ),
        (
            "LogLevel".to_owned(),
            enum_object(&[
                ("Off", log_level_value(LogLevel::Off)),
                ("Trace", log_level_value(LogLevel::Trace)),
                ("Debug", log_level_value(LogLevel::Debug)),
                ("Info", log_level_value(LogLevel::Info)),
                ("Warning", log_level_value(LogLevel::Warning)),
                ("Error", log_level_value(LogLevel::Error)),
            ]),
        ),
        // `QuickInputButtons.Back`: `{ iconPath: new ThemeIcon('arrow-left') }`
        (
            "QuickInputButtons".to_owned(),
            JsData::Object(vec![("Back".to_owned(), JsData::Object(vec![("iconPath".to_owned(), api_to_js(&ThemeIcon::from_id("arrow-left")))]))]),
        ),
        // Os valores do `vscode.d.ts` e de `extHostTypes.ts` (`ViewColumn`, `EndOfLine`, `TextEditorRevealType`,
        // `TextEditorSelectionChangeKind`, `TextDocumentChangeReason`).
        (
            "ViewColumn".to_owned(),
            enum_object(&[
                ("Active", -1),
                ("Beside", -2),
                ("One", 1),
                ("Two", 2),
                ("Three", 3),
                ("Four", 4),
                ("Five", 5),
                ("Six", 6),
                ("Seven", 7),
                ("Eight", 8),
                ("Nine", 9),
            ]),
        ),
        ("EndOfLine".to_owned(), enum_object(&[("LF", 1), ("CRLF", 2)])),
        (
            "TextEditorRevealType".to_owned(),
            enum_object(&[("Default", 0), ("InCenter", 1), ("InCenterIfOutsideViewport", 2), ("AtTop", 3)]),
        ),
        ("TextEditorSelectionChangeKind".to_owned(), enum_object(&[("Keyboard", 1), ("Mouse", 2), ("Command", 3)])),
        ("TextDocumentChangeReason".to_owned(), enum_object(&[("Undo", 1), ("Redo", 2)])),
        // Os enums de linguagem (`DiagnosticSeverity`, `DiagnosticTag`, `SymbolKind`, `SymbolTag`, `CompletionItemKind`,
        // `CompletionItemTag`, `CompletionTriggerKind`, `CodeActionTriggerKind`): os valores de `ext_host_types`.
        (
            "DiagnosticSeverity".to_owned(),
            enum_object(&[
                ("Error", DiagnosticSeverity::Error.value()),
                ("Warning", DiagnosticSeverity::Warning.value()),
                ("Information", DiagnosticSeverity::Information.value()),
                ("Hint", DiagnosticSeverity::Hint.value()),
            ]),
        ),
        ("DiagnosticTag".to_owned(), enum_object(&[("Unnecessary", DiagnosticTag::Unnecessary.value()), ("Deprecated", DiagnosticTag::Deprecated.value())])),
        ("SymbolKind".to_owned(), enum_object(&symbol_kind_members())),
        ("SymbolTag".to_owned(), enum_object(&[("Deprecated", SymbolTag::Deprecated.value())])),
        ("CompletionItemKind".to_owned(), enum_object(&completion_item_kind_members())),
        ("CompletionItemTag".to_owned(), enum_object(&[("Deprecated", CompletionItemTag::Deprecated.value())])),
        (
            "CompletionTriggerKind".to_owned(),
            enum_object(&[
                ("Invoke", CompletionTriggerKind::Invoke.value()),
                ("TriggerCharacter", CompletionTriggerKind::TriggerCharacter.value()),
                ("TriggerForIncompleteCompletions", CompletionTriggerKind::TriggerForIncompleteCompletions.value()),
            ]),
        ),
        (
            "CodeActionTriggerKind".to_owned(),
            enum_object(&[("Invoke", CodeActionTriggerKind::Invoke.value()), ("Automatic", CodeActionTriggerKind::Automatic.value())]),
        ),
        // Os enums dos provedores de `languages` (`DocumentHighlightKind`, `FoldingRangeKind`, `InlayHintKind`, `SignatureHelpTriggerKind`).
        (
            "DocumentHighlightKind".to_owned(),
            enum_object(&[
                ("Text", DocumentHighlightKind::Text.value()),
                ("Read", DocumentHighlightKind::Read.value()),
                ("Write", DocumentHighlightKind::Write.value()),
            ]),
        ),
        (
            "FoldingRangeKind".to_owned(),
            enum_object(&[("Comment", FoldingRangeKind::Comment.value()), ("Imports", FoldingRangeKind::Imports.value()), ("Region", FoldingRangeKind::Region.value())]),
        ),
        ("InlayHintKind".to_owned(), enum_object(&[("Type", InlayHintKind::Type.value()), ("Parameter", InlayHintKind::Parameter.value())])),
        (
            "LanguageStatusSeverity".to_owned(),
            enum_object(&[
                ("Information", LanguageStatusSeverity::Information.value()),
                ("Warning", LanguageStatusSeverity::Warning.value()),
                ("Error", LanguageStatusSeverity::Error.value()),
            ]),
        ),
        (
            "IndentAction".to_owned(),
            enum_object(&[
                ("None", IndentAction::None as i32),
                ("Indent", IndentAction::Indent as i32),
                ("IndentOutdent", IndentAction::IndentOutdent as i32),
                ("Outdent", IndentAction::Outdent as i32),
            ]),
        ),
        (
            "SignatureHelpTriggerKind".to_owned(),
            enum_object(&[
                ("Invoke", SignatureHelpTriggerKind::Invoke.value()),
                ("TriggerCharacter", SignatureHelpTriggerKind::TriggerCharacter.value()),
                ("ContentChange", SignatureHelpTriggerKind::ContentChange.value()),
            ]),
        ),
        // `TreeItemCollapsibleState`, `TreeItemCheckboxState`, `TerminalExitReason` e `TerminalLocation` (`extHostTypes.ts`).
        ("TreeItemCollapsibleState".to_owned(), enum_object(&[("None", 0), ("Collapsed", 1), ("Expanded", 2)])),
        ("TreeItemCheckboxState".to_owned(), enum_object(&[("Unchecked", 0), ("Checked", 1)])),
        ("TerminalExitReason".to_owned(), enum_object(&[("Unknown", 0), ("Shutdown", 1), ("Process", 2), ("User", 3), ("Extension", 4)])),
        ("TerminalLocation".to_owned(), enum_object(&[("Panel", 1), ("Editor", 2)])),
        ("TerminalShellExecutionCommandLineConfidence".to_owned(), enum_object(&[("Low", 0), ("Medium", 1), ("High", 2)])),
        // Os enums de arquivo (`extHostTypes.ts` e `vscode.d.ts`): `FileType`, `FileChangeType` e `FilePermission`.
        (
            "FileType".to_owned(),
            enum_object(&[
                ("Unknown", FileType::UNKNOWN.bits() as i32),
                ("File", FileType::FILE.bits() as i32),
                ("Directory", FileType::DIRECTORY.bits() as i32),
                ("SymbolicLink", FileType::SYMBOLIC_LINK.bits() as i32),
            ]),
        ),
        (
            "FileChangeType".to_owned(),
            enum_object(&[("Changed", FileChangeType::Changed.value()), ("Created", FileChangeType::Created.value()), ("Deleted", FileChangeType::Deleted.value())]),
        ),
        ("FilePermission".to_owned(), enum_object(&[("Readonly", ApiFilePermission::Readonly as i32)])),
        // Os enums de comentários (`extHostTypes.ts`): `CommentThreadCollapsibleState`, `CommentMode`, `CommentState`,
        // `CommentThreadState`, `CommentThreadApplicability` e `CommentThreadFocus`.
        (
            "CommentThreadCollapsibleState".to_owned(),
            enum_object(&[("Collapsed", CommentThreadCollapsibleState::Collapsed.value()), ("Expanded", CommentThreadCollapsibleState::Expanded.value())]),
        ),
        ("CommentMode".to_owned(), enum_object(&[("Editing", CommentMode::Editing.value()), ("Preview", CommentMode::Preview.value())])),
        ("CommentState".to_owned(), enum_object(&[("Published", CommentState::Published.value()), ("Draft", CommentState::Draft.value())])),
        (
            "CommentThreadState".to_owned(),
            enum_object(&[("Unresolved", CommentThreadState::Unresolved.value()), ("Resolved", CommentThreadState::Resolved.value())]),
        ),
        (
            "CommentThreadApplicability".to_owned(),
            enum_object(&[("Current", CommentThreadApplicability::Current.value()), ("Outdated", CommentThreadApplicability::Outdated.value())]),
        ),
        ("CommentThreadFocus".to_owned(), enum_object(&[("Reply", CommentThreadFocus::Reply.value()), ("Comment", CommentThreadFocus::Comment.value())])),
        // Os enums de notebook (`extHostTypes.ts`): `NotebookCellKind`, `NotebookCellExecutionState`, `NotebookCellStatusBarAlignment`,
        // `NotebookEditorRevealType` e `NotebookControllerAffinity`.
        ("NotebookCellKind".to_owned(), enum_object(&[("Markup", NotebookCellKind::Markup.value()), ("Code", NotebookCellKind::Code.value())])),
        (
            "NotebookCellExecutionState".to_owned(),
            enum_object(&[
                ("Idle", NotebookCellExecutionState::Idle.value()),
                ("Pending", NotebookCellExecutionState::Pending.value()),
                ("Executing", NotebookCellExecutionState::Executing.value()),
            ]),
        ),
        (
            "NotebookCellStatusBarAlignment".to_owned(),
            enum_object(&[("Left", NotebookCellStatusBarAlignment::Left.value()), ("Right", NotebookCellStatusBarAlignment::Right.value())]),
        ),
        (
            "NotebookEditorRevealType".to_owned(),
            enum_object(&[
                ("Default", NotebookEditorRevealType::Default.value()),
                ("InCenter", NotebookEditorRevealType::InCenter.value()),
                ("InCenterIfOutsideViewport", NotebookEditorRevealType::InCenterIfOutsideViewport.value()),
                ("AtTop", NotebookEditorRevealType::AtTop.value()),
            ]),
        ),
        (
            "NotebookControllerAffinity".to_owned(),
            enum_object(&[("Default", NotebookControllerAffinity::Default.value()), ("Preferred", NotebookControllerAffinity::Preferred.value())]),
        ),
        // `DebugConsoleMode` (`extHostTypes.ts`) e `DebugConfigurationProviderTriggerKind` (`debug.ts`).
        ("DebugConsoleMode".to_owned(), enum_object(&[("Separate", DebugConsoleMode::Separate as i32), ("MergeWithParent", DebugConsoleMode::MergeWithParent as i32)])),
        (
            "DebugConfigurationProviderTriggerKind".to_owned(),
            enum_object(&[
                ("Initial", DebugConfigurationProviderTriggerKind::Initial as i32),
                ("Dynamic", DebugConfigurationProviderTriggerKind::Dynamic as i32),
            ]),
        ),
    ]
}
