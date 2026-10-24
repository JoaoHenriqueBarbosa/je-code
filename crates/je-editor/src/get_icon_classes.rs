//! Porte de `src/vs/editor/common/services/getIconClasses.ts`: as classes de CSS que o tema de ícones de arquivo
//! usa para escolher o ícone de um recurso.
//!
//! Divergências:
//! - `fileIconDirectoryRegex` (`/(?:\/|^)(?:([^\/]+)\/)?([^\/]+)$/`) é escrita à mão em [`match_directory`]: o
//!   último segmento do caminho (não vazio, sem barra no fim) e, se houver, o segmento não vazio anterior a ele.
//! - `icon?: ThemeIcon | URI` é o [`IconClassesIcon`] dentro de `Option`; `fileKind` ausente é `None` (o mesmo que
//!   arquivo).
//! - `str.replace(/[\s]/g, '/')` e `toLowerCase()` usam `char::is_whitespace` (mais o BOM, que o `\s` do JavaScript
//!   inclui) e `str::to_lowercase`.

use je_base::network::schemas;
use je_base::resources::data_uri;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_platform::files::files::FileKind;

use crate::language_service::ILanguageService;
use crate::modes_registry::PLAINTEXT_LANGUAGE_ID;
use crate::services_model::IModelService;

/// O `ThemeIcon | URI` do parâmetro `icon`.
#[derive(Clone, Debug)]
pub enum IconClassesIcon {
    ThemeIcon(ThemeIcon),
    Uri(URI),
}

/// `getIconClasses`.
pub fn get_icon_classes(
    model_service: &dyn IModelService,
    language_service: &dyn ILanguageService,
    resource: Option<&URI>,
    file_kind: Option<FileKind>,
    icon: Option<&IconClassesIcon>,
) -> Vec<String> {
    match icon {
        Some(IconClassesIcon::ThemeIcon(icon)) => return vec![format!("codicon-{}", icon.id), "predefined-file-icon".to_owned()],
        Some(IconClassesIcon::Uri(_)) => return Vec::new(),
        None => {}
    }

    // we always set these base classes even if we do not have a path
    let mut classes: Vec<String> = match file_kind {
        Some(FileKind::RootFolder) => vec!["rootfolder-icon".to_owned()],
        Some(FileKind::Folder) => vec!["folder-icon".to_owned()],
        _ => vec!["file-icon".to_owned()],
    };
    let Some(resource) = resource else {
        return classes;
    };

    // Get the path and name of the resource. For data-URIs, we need to parse specially
    let name: Option<String> = if resource.scheme().to_string_lossy() == schemas::DATA {
        data_uri::parse_meta_data(resource).get(&data_uri::META_DATA_LABEL.to_owned()).cloned()
    } else {
        let path = resource.path().to_string_lossy();
        match match_directory(&path) {
            Some((parent, name)) => {
                if let Some(parent) = parent {
                    classes.push(format!("{}-name-dir-icon", file_icon_selector_escape(&parent.to_lowercase()))); // parent directory
                }
                Some(file_icon_selector_escape(&name.to_lowercase()))
            }
            None => Some(file_icon_selector_escape(&resource.authority().to_string_lossy().to_lowercase())),
        }
    };
    let name_text = name.clone().unwrap_or_else(|| "undefined".to_owned());

    match file_kind {
        // Root Folders
        Some(FileKind::RootFolder) => classes.push(format!("{name_text}-root-name-folder-icon")),

        // Folders
        Some(FileKind::Folder) => classes.push(format!("{name_text}-name-folder-icon")),

        // Files
        _ => {
            // Name & Extension(s)
            if let Some(name) = name.as_deref().filter(|name| !name.is_empty()) {
                classes.push(format!("{name}-name-file-icon"));
                classes.push("name-file-icon".to_owned()); // extra segment to increase file-name score
                // Avoid doing an explosive combination of extensions for very long filenames
                // (most file systems do not allow files > 255 length) with lots of `.` characters
                // https://github.com/microsoft/vscode/issues/116199
                if name.encode_utf16().count() <= 255 {
                    let dot_segments: Vec<&str> = name.split('.').collect();
                    for index in 1..dot_segments.len() {
                        classes.push(format!("{}-ext-file-icon", dot_segments[index..].join("."))); // add each combination of all found extensions if more than one
                    }
                }
                classes.push("ext-file-icon".to_owned()); // extra segment to increase file-ext score
            }

            // Detected Mode
            if let Some(detected_language_id) = detect_language_id(model_service, language_service, resource) {
                classes.push(format!("{}-lang-file-icon", file_icon_selector_escape(&detected_language_id)));
            }
        }
    }
    classes
}

/// `getIconClassesForLanguageId`.
pub fn get_icon_classes_for_language_id(language_id: &str) -> Vec<String> {
    vec!["file-icon".to_owned(), format!("{}-lang-file-icon", file_icon_selector_escape(language_id))]
}

/// O casamento de `fileIconDirectoryRegex` em `path`: `(pai, nome)`, ou `None` se não casa (caminho vazio ou com barra
/// no fim).
fn match_directory(path: &str) -> Option<(Option<&str>, &str)> {
    if path.is_empty() || path.ends_with('/') {
        return None;
    }
    let Some(last_slash) = path.rfind('/') else {
        return Some((None, path));
    };
    let name = &path[last_slash + 1..];
    let parent = path[..last_slash].rsplit('/').next().unwrap_or("");
    Some((if parent.is_empty() { None } else { Some(parent) }, name))
}

fn detect_language_id(model_service: &dyn IModelService, language_service: &dyn ILanguageService, resource: &URI) -> Option<String> {
    let mut language_id: Option<String> = None;

    // Data URI: check for encoded metadata
    if resource.scheme().to_string_lossy() == schemas::DATA {
        let metadata = data_uri::parse_meta_data(resource);
        if let Some(mime) = metadata.get(&data_uri::META_DATA_MIME.to_owned()).filter(|mime| !mime.is_empty()) {
            language_id = language_service.get_language_id_by_mime_type(Some(mime.as_str()));
        }
    }
    // Any other URI: check for model if existing
    else if let Some(model) = model_service.get_model(resource) {
        language_id = Some(model.get_language_id());
    }

    // only take if the language id is specific (aka no just plain text)
    if let Some(language_id) = language_id.filter(|id| !id.is_empty() && id != PLAINTEXT_LANGUAGE_ID) {
        return Some(language_id);
    }

    // otherwise fallback to path based detection
    language_service.guess_language_id_by_filepath_or_first_line(Some(resource), None).filter(|id| !id.is_empty())
}

/// `fileIconSelectorEscape`: HTML class names can not contain certain whitespace characters, use `/` instead,
/// which doesn't exist in file names.
pub fn file_icon_selector_escape(text: &str) -> String {
    text.chars().map(|c| if c.is_whitespace() || c == '\u{FEFF}' { '/' } else { c }).collect()
}
