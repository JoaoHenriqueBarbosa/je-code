//! As enumerações numéricas da API `vscode` que os namespaces entregues usam (`StatusBarAlignment`, `ConfigurationTarget`, `UIKind`,
//! `ExtensionKind`, `QuickPickItemKind`, `InputBoxValidationSeverity`, `ProgressLocation`, `LogLevel`) e o objeto `QuickInputButtons`.
//! Cada enumeração é o objeto que o TypeScript gera para um `enum`: nome para valor e valor para nome (`StatusBarAlignment.Left === 1` e
//! `StatusBarAlignment[1] === 'Left'`). Os valores são os de `ext_host_types::enums`.
//!
//! Divergências:
//! - `QuickInputButtons.Back` é um objeto comum `{ iconPath }` copiado a cada leitura (o motor copia objetos comuns): comparar um
//!   botão com `QuickInputButtons.Back` por identidade (`button === QuickInputButtons.Back`) falha, comparar por `iconPath.id` funciona.

use je_base::themables::ThemeIcon;
use je_platform::log::LogLevel;
use je_workbench::api::common::ext_host_types::{ConfigurationTarget, ExtensionKind, InputBoxValidationSeverity, ProgressLocation, QuickPickItemKind, StatusBarAlignment};
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
    ]
}
