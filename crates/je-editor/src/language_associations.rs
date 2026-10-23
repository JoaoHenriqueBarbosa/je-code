//! Porte de `src/vs/editor/common/services/languagesAssociations.ts`: as associações entre linguagem e
//! extensão, nome de arquivo, padrão de arquivo ou primeira linha.
//!
//! As três listas do original são variáveis de módulo; aqui são estado por thread (`thread_local!`), um
//! por thread de UI, como o resto do porte.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::glob::{self, Glob, GlobOptions, ParsedGlob};
use je_base::mime::mimes as Mimes;
use je_base::network::schemas as Schemas;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::resources::data_uri;
use je_base::strings;
use je_base::uri::URI;

use crate::js_reg_exp::JsRegExp;

/// `ILanguageAssociation`.
#[derive(Clone, Debug, Default)]
pub struct LanguageAssociation {
    pub id: String,
    pub mime: String,
    pub filename: Option<String>,
    pub extension: Option<String>,
    pub filepattern: Option<String>,
    pub firstline: Option<Rc<JsRegExp>>,
}

#[derive(Clone)]
struct AssociationItem {
    association: LanguageAssociation,
    user_configured: bool,
    filepattern_parsed: Option<ParsedGlob>,
    filepattern_on_path: bool,
}

#[derive(Default)]
struct State {
    registered: Vec<AssociationItem>,
    non_user_registered: Vec<AssociationItem>,
    user_registered: Vec<AssociationItem>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// `registerPlatformLanguageAssociation(association, warnOnOverwrite)`.
pub fn register_platform_language_association(association: LanguageAssociation, warn_on_overwrite: bool) {
    register_language_association(association, false, warn_on_overwrite);
}

/// `registerConfiguredLanguageAssociation(association)`.
pub fn register_configured_language_association(association: LanguageAssociation) {
    register_language_association(association, true, false);
}

fn register_language_association(association: LanguageAssociation, user_configured: bool, warn_on_overwrite: bool) {
    let item = to_item(association, user_configured);
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.registered.push(item.clone());
        if !item.user_configured {
            state.non_user_registered.push(item.clone());
        } else {
            state.user_registered.push(item.clone());
        }

        if warn_on_overwrite && !item.user_configured {
            for a in &state.registered {
                if a.association.mime == item.association.mime || a.user_configured {
                    continue; // same mime or userConfigured is ok
                }
                let (new, old) = (&item.association, &a.association);
                if new.extension.is_some() && old.extension == new.extension {
                    eprintln!("Overwriting extension <<{}>> to now point to mime <<{}>>", new.extension.as_deref().unwrap_or(""), new.mime);
                }
                if new.filename.is_some() && old.filename == new.filename {
                    eprintln!("Overwriting filename <<{}>> to now point to mime <<{}>>", new.filename.as_deref().unwrap_or(""), new.mime);
                }
                if new.filepattern.is_some() && old.filepattern == new.filepattern {
                    eprintln!("Overwriting filepattern <<{}>> to now point to mime <<{}>>", new.filepattern.as_deref().unwrap_or(""), new.mime);
                }
            }
        }
    });
}

fn to_item(association: LanguageAssociation, user_configured: bool) -> AssociationItem {
    let options = GlobOptions { ignore_case: Some(true), ..GlobOptions::default() };
    let filepattern_parsed = association.filepattern.as_deref().filter(|p| !p.is_empty()).map(|p| glob::parse(Glob::Pattern(p), &options));
    let filepattern_on_path = association.filepattern.as_deref().is_some_and(|p| p.contains('/'));
    AssociationItem { association, user_configured, filepattern_parsed, filepattern_on_path }
}

/// `clearPlatformLanguageAssociations()`.
pub fn clear_platform_language_associations() {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.registered.retain(|a| a.user_configured);
        state.non_user_registered.clear();
    });
}

/// `clearConfiguredLanguageAssociations()`.
pub fn clear_configured_language_associations() {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.registered.retain(|a| !a.user_configured);
        state.user_registered.clear();
    });
}

/// `getMimeTypes(resource, firstLine)`.
pub fn get_mime_types(resource: Option<&URI>, first_line: Option<&JsString>) -> Vec<String> {
    get_associations(resource, first_line).into_iter().map(|(_, mime)| mime).collect()
}

/// `getLanguageIds(resource, firstLine)`.
pub fn get_language_ids(resource: Option<&URI>, first_line: Option<&JsString>) -> Vec<String> {
    get_associations(resource, first_line).into_iter().map(|(id, _)| id).collect()
}

fn unknown() -> Vec<(String, String)> {
    vec![("unknown".to_owned(), Mimes::UNKNOWN.to_owned())]
}

/// `getAssociations(resource, firstLine)`: pares `(id, mime)`.
fn get_associations(resource: Option<&URI>, first_line: Option<&JsString>) -> Vec<(String, String)> {
    let mut path: Option<String> = None;
    if let Some(resource) = resource {
        let scheme = resource.scheme().to_string_lossy();
        path = match scheme.as_str() {
            s if s == Schemas::FILE => Some(resource.fs_path(je_base::path::Platform::Posix).to_string_lossy()),
            s if s == Schemas::DATA => {
                let metadata = data_uri::parse_meta_data(resource);
                metadata.get(&data_uri::META_DATA_LABEL.to_owned()).cloned()
            }
            s if s == Schemas::VSCODE_NOTEBOOK_CELL => None,
            _ => Some(resource.path().to_string_lossy()),
        };
    }

    let Some(path) = path.filter(|p| !p.is_empty()) else {
        return unknown();
    };
    let path = JsString::from(path.as_str()).to_lower_case().to_string_lossy();
    let filename = PathModule::posix(&ProcessEnvironment).basename(&path, None);

    STATE.with(|state| {
        let state = state.borrow();
        // Check for dynamically registered languages first: user configured associations win.
        if let Some(configured) = get_association_by_path(&path, &filename, &state.user_registered) {
            return vec![configured, (je_base_plaintext(), Mimes::TEXT.to_owned())];
        }
        if let Some(registered) = get_association_by_path(&path, &filename, &state.non_user_registered) {
            return vec![registered, (je_base_plaintext(), Mimes::TEXT.to_owned())];
        }
        if let Some(first_line) = first_line
            && let Some(first_line_language) = get_association_by_first_line(&state.registered, first_line)
        {
            return vec![first_line_language, (je_base_plaintext(), Mimes::TEXT.to_owned())];
        }
        unknown()
    })
}

fn je_base_plaintext() -> String {
    crate::modes_registry::PLAINTEXT_LANGUAGE_ID.to_owned()
}

fn get_association_by_path(path: &str, filename: &str, associations: &[AssociationItem]) -> Option<(String, String)> {
    let mut filename_match: Option<&AssociationItem> = None;
    let mut pattern_match: Option<&AssociationItem> = None;
    let mut extension_match: Option<&AssociationItem> = None;

    // We want to prioritize associations based on the order they are registered so that the last registered
    // association wins over all other. This is for https://github.com/microsoft/vscode/issues/47353.
    for association in associations.iter().rev() {
        let a = &association.association;
        if let Some(name) = &a.filename
            && strings::equals_ignore_case(JsString::from(filename).units(), JsString::from(name.as_str()).units())
        {
            filename_match = Some(association);
            break; // take it!
        }

        if let Some(pattern) = a.filepattern.as_deref().filter(|p| !p.is_empty())
            && pattern_match.is_none_or(|m| pattern.encode_utf16().count() > m.association.filepattern.as_deref().map_or(0, |p| p.encode_utf16().count()))
        {
            // match on full path if pattern contains path separator
            let target = if association.filepattern_on_path { path } else { filename };
            let matched = match &association.filepattern_parsed {
                Some(ParsedGlob::Pattern(p)) => p.matches(target, None),
                _ => false,
            };
            if matched {
                pattern_match = Some(association);
            }
        }

        if let Some(extension) = a.extension.as_deref().filter(|e| !e.is_empty())
            && extension_match.is_none_or(|m| extension.encode_utf16().count() > m.association.extension.as_deref().map_or(0, |e| e.encode_utf16().count()))
            && strings::ends_with_ignore_case(JsString::from(filename).units(), JsString::from(extension).units())
        {
            extension_match = Some(association);
        }
    }

    filename_match
        .or(pattern_match)
        .or(extension_match)
        .map(|m| (m.association.id.clone(), m.association.mime.clone()))
}

fn get_association_by_first_line(registered: &[AssociationItem], first_line: &JsString) -> Option<(String, String)> {
    let mut units = first_line.units();
    if strings::starts_with_utf8_bom(units) {
        units = &units[1..];
    }
    if units.is_empty() {
        return None;
    }
    // We want to prioritize associations based on the order they are registered so that the last registered
    // association wins over all other.
    for item in registered.iter().rev() {
        let Some(regex) = &item.association.firstline else { continue };
        regex.set_last_index(0);
        if regex.exec(units).is_some() {
            return Some((item.association.id.clone(), item.association.mime.clone()));
        }
    }
    None
}
