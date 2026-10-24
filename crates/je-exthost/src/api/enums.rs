//! As enumerações numéricas da API `vscode` que os namespaces entregues usam (`StatusBarAlignment`, `ConfigurationTarget`, `UIKind`,
//! `ExtensionKind`). Cada uma é o objeto que o TypeScript gera para um `enum`: nome para valor e valor para nome
//! (`StatusBarAlignment.Left === 1` e `StatusBarAlignment[1] === 'Left'`). Os valores são os de `ext_host_types::enums`.

use je_workbench::api::common::ext_host_types::{ConfigurationTarget, ExtensionKind, StatusBarAlignment};
use zjsc::api::embed_node::JsData;

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
    ]
}
