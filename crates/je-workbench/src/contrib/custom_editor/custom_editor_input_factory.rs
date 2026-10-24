//! Porte do tipo `CustomDocumentBackupData` de `src/vs/workbench/contrib/customEditor/browser/customEditorInputFactory.ts`: os
//! metadados que o backup de um documento personalizado guarda (`IWorkingCopyBackupMeta`).
//!
//! Fora deste módulo: `CustomEditorInputSerializer`, `CustomEditorDiffInputSerializer`,
//! `ComplexCustomWorkingCopyEditorHandler` e `CustomEditorInputWorkingCopyEditorHandler` (a serialização e a restauração de
//! entradas, que dependem do `WebviewEditorInputSerializer`, do `IWorkingCopyEditorService` e do `IWorkingCopyBackupService`
//! de um workbench inteiro).
//!
//! Divergências:
//! - `IWorkingCopyBackupMeta` (`{ [key: string]: unknown }`) é um [`Dynamic`] objeto; [`CustomDocumentBackupData::to_dynamic`]
//!   o monta e [`CustomDocumentBackupData::backup_id_of`] lê o `backupId` de volta (o único campo que o
//!   `MainThreadCustomEditors` consulta do `workingCopy.meta as CustomDocumentBackupData`).
//! - `UriComponents` são os componentes não vazios do `URI` (as propriedades do `toJSON` que o desserializador lê), e o
//!   `webview.options` é o [`WebviewOptions`] do webview, sem o `transformCssVariables` (uma função, que o `JSON.stringify`
//!   do backup descarta).

use je_base::dynamic::Dynamic;
use je_base::uri::URI;

use crate::contrib::webview::browser_webview::WebviewOptions;
use crate::contrib::webview_panel::webview_editor_input::WebviewIconPath;

/// O `extension` de `CustomDocumentBackupData`.
#[derive(Clone, Debug)]
pub struct CustomDocumentBackupExtension {
    pub location: Option<URI>,
    pub id: String,
}

/// O `webview` de `CustomDocumentBackupData`.
#[derive(Clone)]
pub struct CustomDocumentBackupWebview {
    pub origin: Option<String>,
    pub options: WebviewOptions,
    pub state: Option<String>,
}

/// `CustomDocumentBackupData`.
#[derive(Clone)]
pub struct CustomDocumentBackupData {
    pub view_type: String,
    pub editor_resource: URI,

    pub custom_title: Option<String>,
    pub icon_path: Option<WebviewIconPath>,

    pub backup_id: String,

    pub extension: Option<CustomDocumentBackupExtension>,

    pub webview: CustomDocumentBackupWebview,
}

/// Os componentes não vazios do `URI`, como o objeto `UriComponents`.
fn uri_to_dynamic(uri: &URI) -> Dynamic {
    let mut entries: Vec<(&str, Dynamic)> = Vec::new();
    for (name, value) in [("scheme", uri.scheme()), ("authority", uri.authority()), ("path", uri.path()), ("query", uri.query()), ("fragment", uri.fragment())] {
        if !value.is_empty() {
            entries.push((name, Dynamic::string(&value.to_string_lossy())));
        }
    }
    Dynamic::object(entries)
}

fn optional_string(value: &Option<String>) -> Dynamic {
    value.as_deref().map_or(Dynamic::Undefined, Dynamic::string)
}

fn optional_bool(value: Option<bool>) -> Dynamic {
    value.map_or(Dynamic::Undefined, Dynamic::Bool)
}

fn options_to_dynamic(options: &WebviewOptions) -> Dynamic {
    Dynamic::object([
        ("purpose", options.purpose.map_or(Dynamic::Undefined, |purpose| Dynamic::string(purpose.as_str()))),
        ("customClasses", optional_string(&options.custom_classes)),
        ("enableFindWidget", optional_bool(options.enable_find_widget)),
        ("disableServiceWorker", optional_bool(options.disable_service_worker)),
        ("tryRestoreScrollPosition", optional_bool(options.try_restore_scroll_position)),
        ("retainContextWhenHidden", optional_bool(options.retain_context_when_hidden)),
    ])
}

fn icon_path_to_dynamic(icon_path: &Option<WebviewIconPath>) -> Dynamic {
    match icon_path {
        None => Dynamic::Undefined,
        Some(WebviewIconPath::ThemeIcon(icon)) => {
            Dynamic::object([("id", Dynamic::string(&icon.id)), ("color", icon.color.as_ref().map_or(Dynamic::Undefined, |color| Dynamic::object([("id", Dynamic::string(&color.id.to_string()))])))])
        }
        Some(WebviewIconPath::LightDark { light, dark }) => Dynamic::object([("dark", uri_to_dynamic(dark)), ("light", uri_to_dynamic(light))]),
    }
}

impl CustomDocumentBackupData {
    /// O `IWorkingCopyBackupMeta` que o backup devolve.
    pub fn to_dynamic(&self) -> Dynamic {
        Dynamic::object([
            ("viewType", Dynamic::string(&self.view_type)),
            ("editorResource", uri_to_dynamic(&self.editor_resource)),
            ("customTitle", optional_string(&self.custom_title)),
            ("iconPath", icon_path_to_dynamic(&self.icon_path)),
            ("backupId", Dynamic::string(&self.backup_id)),
            (
                "extension",
                self.extension.as_ref().map_or(Dynamic::Undefined, |extension| {
                    Dynamic::object([("id", Dynamic::string(&extension.id)), ("location", extension.location.as_ref().map_or(Dynamic::Undefined, uri_to_dynamic))])
                }),
            ),
            (
                "webview",
                Dynamic::object([
                    ("origin", optional_string(&self.webview.origin)),
                    ("options", options_to_dynamic(&self.webview.options)),
                    ("state", optional_string(&self.webview.state)),
                ]),
            ),
        ])
    }

    /// `(meta as CustomDocumentBackupData).backupId`.
    pub fn backup_id_of(meta: &Dynamic) -> Option<String> {
        match meta.get_str("backupId") {
            Dynamic::String(backup_id) => Some(backup_id.to_string_lossy()),
            _ => None,
        }
    }
}
