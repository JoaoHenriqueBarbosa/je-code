//! Porte de `src/vs/editor/common/services/languagesRegistry.ts`: o codec de ids de linguagem e o registro
//! de linguagens (nome, mime, extensões, nomes de arquivo, aliases, arquivos de configuração, ícones).
//!
//! Divergência: o original avisa o `IConfigurationRegistry` (`registerOverrideIdentifiers`) a cada
//! `_registerLanguages`; o registro de configuração ainda não existe no porte, então esse aviso não ocorre.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::strings;
use je_base::uri::URI;

use crate::encoded_token_attributes::language_id;
use crate::js_reg_exp::JsRegExp;
use crate::language_associations::{
    LanguageAssociation, clear_platform_language_associations, get_language_ids, register_platform_language_association,
};
use crate::languages::ILanguageIdCodec;
use crate::modes_registry::{LanguageExtensionPoint, LanguageIcon, PLAINTEXT_LANGUAGE_ID, modes_registry};
use crate::text_model_search::reg_exp_leads_to_endless_loop;

const NULL_LANGUAGE_ID: &str = "vs.editor.nullLanguage";

#[derive(Default)]
struct ResolvedLanguage {
    identifier: String,
    name: Option<String>,
    mimetypes: Vec<String>,
    aliases: Vec<String>,
    extensions: Vec<String>,
    filenames: Vec<String>,
    configuration_files: Vec<URI>,
    icons: Vec<LanguageIcon>,
}

/// `LanguageIdCodec`.
pub struct LanguageIdCodec {
    next_language_id: Cell<u32>,
    language_id_to_language: RefCell<Vec<String>>,
    language_to_language_id: RefCell<HashMap<String, u32>>,
}

impl LanguageIdCodec {
    pub fn new() -> Self {
        let codec = Self {
            next_language_id: Cell::new(2),
            language_id_to_language: RefCell::new(Vec::new()),
            language_to_language_id: RefCell::new(HashMap::new()),
        };
        codec.register_internal(NULL_LANGUAGE_ID, language_id::NULL);
        codec.register_internal(PLAINTEXT_LANGUAGE_ID, language_id::PLAIN_TEXT);
        codec
    }

    fn register_internal(&self, language: &str, id: u32) {
        let mut by_id = self.language_id_to_language.borrow_mut();
        if by_id.len() <= id as usize {
            by_id.resize(id as usize + 1, String::new());
        }
        by_id[id as usize] = language.to_owned();
        self.language_to_language_id.borrow_mut().insert(language.to_owned(), id);
    }

    pub fn register(&self, language: &str) {
        if self.language_to_language_id.borrow().contains_key(language) {
            return;
        }
        let id = self.next_language_id.get();
        self.next_language_id.set(id + 1);
        self.register_internal(language, id);
    }
}

impl Default for LanguageIdCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl ILanguageIdCodec for LanguageIdCodec {
    fn encode_language_id(&self, language_id: &str) -> u32 {
        self.language_to_language_id.borrow().get(language_id).copied().unwrap_or(language_id::NULL)
    }

    fn decode_language_id(&self, language_id: u32) -> String {
        self.language_id_to_language
            .borrow()
            .get(language_id as usize)
            .filter(|l| !l.is_empty())
            .cloned()
            .unwrap_or_else(|| NULL_LANGUAGE_ID.to_owned())
    }
}

/// `ILanguageNameIdPair`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageNameIdPair {
    pub language_name: String,
    pub language_id: String,
}

#[derive(Default)]
struct RegistryState {
    dynamic_languages: Vec<LanguageExtensionPoint>,
    /// Os ids na ordem de registro (a ordem das chaves do objeto do original).
    language_order: Vec<String>,
    languages: HashMap<String, ResolvedLanguage>,
    mime_types_map: HashMap<String, String>,
    name_map: Vec<(String, String)>,
    lowercase_name_map: HashMap<String, String>,
}

/// `LanguagesRegistry`.
pub struct LanguagesRegistry {
    base: Rc<Disposable>,
    on_did_change: Rc<Emitter<()>>,
    warn_on_overwrite: bool,
    pub language_id_codec: Rc<LanguageIdCodec>,
    state: RefCell<RegistryState>,
}

thread_local! {
    static INSTANCE_COUNT: Cell<i32> = const { Cell::new(0) };
}

impl LanguagesRegistry {
    /// `LanguagesRegistry.instanceCount`.
    pub fn instance_count() -> i32 {
        INSTANCE_COUNT.with(Cell::get)
    }

    pub fn new(use_modes_registry: bool, warn_on_overwrite: bool) -> Rc<Self> {
        INSTANCE_COUNT.with(|c| c.set(c.get() + 1));
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        let registry = Rc::new(Self {
            base,
            on_did_change,
            warn_on_overwrite,
            language_id_codec: Rc::new(LanguageIdCodec::new()),
            state: RefCell::new(RegistryState::default()),
        });
        if use_modes_registry {
            registry.initialize_from_registry();
            let weak = Rc::downgrade(&registry);
            let subscription = modes_registry().on_did_change_languages().listen(move |_| {
                if let Some(registry) = weak.upgrade() {
                    registry.initialize_from_registry();
                }
            });
            registry.base.register(subscription);
        }
        registry
    }

    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    pub fn set_dynamic_languages(&self, def: Vec<LanguageExtensionPoint>) {
        self.state.borrow_mut().dynamic_languages = def;
        self.initialize_from_registry();
    }

    fn initialize_from_registry(&self) {
        {
            let mut state = self.state.borrow_mut();
            state.language_order.clear();
            state.languages.clear();
            state.mime_types_map.clear();
            state.name_map.clear();
            state.lowercase_name_map.clear();
        }
        clear_platform_language_associations();
        let mut desc = modes_registry().get_languages();
        desc.extend(self.state.borrow().dynamic_languages.iter().cloned());
        self.register_languages(desc);
    }

    pub fn register_language(&self, desc: LanguageExtensionPoint) -> DisposableRef {
        modes_registry().register_language(desc)
    }

    /// `_registerLanguages(desc)`.
    pub fn register_languages(&self, desc: Vec<LanguageExtensionPoint>) {
        for d in &desc {
            self.register_language_internal(d);
        }

        {
            let mut state = self.state.borrow_mut();
            state.mime_types_map.clear();
            state.name_map.clear();
            state.lowercase_name_map.clear();
            let ids = state.language_order.clone();
            for lang_id in ids {
                let language = &state.languages[&lang_id];
                let identifier = language.identifier.clone();
                let name = language.name.clone();
                let aliases = language.aliases.clone();
                let mimetypes = language.mimetypes.clone();
                if let Some(name) = name {
                    match state.name_map.iter_mut().find(|(n, _)| *n == name) {
                        Some(entry) => entry.1 = identifier.clone(),
                        None => state.name_map.push((name, identifier.clone())),
                    }
                }
                for alias in aliases {
                    state.lowercase_name_map.insert(alias.to_lowercase(), identifier.clone());
                }
                for mimetype in mimetypes {
                    state.mime_types_map.insert(mimetype, identifier.clone());
                }
            }
        }
        self.on_did_change.fire(());
    }

    fn register_language_internal(&self, lang: &LanguageExtensionPoint) {
        let lang_id = &lang.id;
        {
            let mut state = self.state.borrow_mut();
            if !state.languages.contains_key(lang_id) {
                self.language_id_codec.register(lang_id);
                state.language_order.push(lang_id.clone());
                state.languages.insert(
                    lang_id.clone(),
                    ResolvedLanguage { identifier: lang_id.clone(), ..ResolvedLanguage::default() },
                );
            }
        }
        self.merge_language(lang);
    }

    fn merge_language(&self, lang: &LanguageExtensionPoint) {
        let lang_id = &lang.id;
        let mut state = self.state.borrow_mut();
        let resolved = state.languages.get_mut(lang_id).expect("a linguagem acabou de ser registrada");

        let mut primary_mime: Option<String> = None;

        if let Some(mimetypes) = lang.mimetypes.as_ref().filter(|m| !m.is_empty()) {
            resolved.mimetypes.extend(mimetypes.iter().cloned());
            primary_mime = Some(mimetypes[0].clone());
        }

        let primary_mime = match primary_mime.filter(|m| !m.is_empty()) {
            Some(mime) => mime,
            None => {
                let mime = format!("text/x-{lang_id}");
                resolved.mimetypes.push(mime.clone());
                mime
            }
        };

        if let Some(extensions) = &lang.extensions {
            // Language configuration supports `extensions` before, and platform definitions after.
            if lang.configuration.is_some() {
                let mut merged = extensions.clone();
                merged.extend(std::mem::take(&mut resolved.extensions));
                resolved.extensions = merged;
            } else {
                resolved.extensions.extend(extensions.iter().cloned());
            }
            for extension in extensions {
                register_platform_language_association(
                    LanguageAssociation {
                        id: lang_id.clone(),
                        mime: primary_mime.clone(),
                        extension: Some(extension.clone()),
                        ..LanguageAssociation::default()
                    },
                    self.warn_on_overwrite,
                );
            }
        }

        if let Some(filenames) = &lang.filenames {
            for filename in filenames {
                register_platform_language_association(
                    LanguageAssociation {
                        id: lang_id.clone(),
                        mime: primary_mime.clone(),
                        filename: Some(filename.clone()),
                        ..LanguageAssociation::default()
                    },
                    self.warn_on_overwrite,
                );
                resolved.filenames.push(filename.clone());
            }
        }

        if let Some(patterns) = &lang.filename_patterns {
            for pattern in patterns {
                register_platform_language_association(
                    LanguageAssociation {
                        id: lang_id.clone(),
                        mime: primary_mime.clone(),
                        filepattern: Some(pattern.clone()),
                        ..LanguageAssociation::default()
                    },
                    self.warn_on_overwrite,
                );
            }
        }

        if let Some(first_line) = lang.first_line.as_deref().filter(|f| !f.is_empty()) {
            let mut first_line_regex_str = first_line.to_owned();
            if !first_line_regex_str.starts_with('^') {
                first_line_regex_str = format!("^{first_line_regex_str}");
            }
            match JsRegExp::from_str(&first_line_regex_str, "") {
                Ok(regex) => {
                    if !reg_exp_leads_to_endless_loop(&regex) {
                        register_platform_language_association(
                            LanguageAssociation {
                                id: lang_id.clone(),
                                mime: primary_mime.clone(),
                                firstline: Some(Rc::new(regex)),
                                ..LanguageAssociation::default()
                            },
                            self.warn_on_overwrite,
                        );
                    }
                }
                Err(err) => eprintln!("[{lang_id}]: Invalid regular expression `{first_line_regex_str}`: {err:?}"),
            }
        }

        resolved.aliases.push(lang_id.clone());

        let mut lang_aliases: Option<Vec<Option<String>>> = None;
        if let Some(aliases) = &lang.aliases {
            if aliases.is_empty() {
                // signal that this language should not get a name
                lang_aliases = Some(vec![None]);
            } else {
                lang_aliases = Some(aliases.iter().cloned().map(Some).collect());
            }
        }

        if let Some(aliases) = &lang_aliases {
            for alias in aliases {
                match alias {
                    Some(a) if !a.is_empty() => resolved.aliases.push(a.clone()),
                    _ => {}
                }
            }
        }

        let contains_aliases = lang_aliases.as_ref().is_some_and(|a| !a.is_empty());
        let first_is_null = contains_aliases && lang_aliases.as_ref().is_some_and(|a| a[0].is_none());
        if !first_is_null {
            let best_name = contains_aliases
                .then(|| lang_aliases.as_ref().and_then(|a| a[0].clone()))
                .flatten()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| lang_id.clone());
            if contains_aliases || resolved.name.as_deref().is_none_or(str::is_empty) {
                resolved.name = Some(best_name);
            }
        }

        if let Some(configuration) = &lang.configuration {
            resolved.configuration_files.push(configuration.clone());
        }

        if let Some(icon) = &lang.icon {
            resolved.icons.push(icon.clone());
        }
    }

    pub fn is_registered_language_id(&self, language_id: Option<&str>) -> bool {
        match language_id {
            None | Some("") => false,
            Some(id) => self.state.borrow().languages.contains_key(id),
        }
    }

    pub fn get_registered_language_ids(&self) -> Vec<String> {
        self.state.borrow().language_order.clone()
    }

    pub fn get_sorted_registered_language_names(&self) -> Vec<LanguageNameIdPair> {
        let mut result: Vec<LanguageNameIdPair> = self
            .state
            .borrow()
            .name_map
            .iter()
            .map(|(name, id)| LanguageNameIdPair { language_name: name.clone(), language_id: id.clone() })
            .collect();
        result.sort_by(|a, b| {
            strings::compare_ignore_case(JsString::from(a.language_name.as_str()).units(), JsString::from(b.language_name.as_str()).units()).cmp(&0)
        });
        result
    }

    pub fn get_language_name(&self, language_id: &str) -> Option<String> {
        self.state.borrow().languages.get(language_id)?.name.clone()
    }

    pub fn get_mime_type(&self, language_id: &str) -> Option<String> {
        self.state.borrow().languages.get(language_id)?.mimetypes.first().cloned()
    }

    pub fn get_extensions(&self, language_id: &str) -> Vec<String> {
        self.state.borrow().languages.get(language_id).map(|l| l.extensions.clone()).unwrap_or_default()
    }

    pub fn get_filenames(&self, language_id: &str) -> Vec<String> {
        self.state.borrow().languages.get(language_id).map(|l| l.filenames.clone()).unwrap_or_default()
    }

    pub fn get_icon(&self, language_id: &str) -> Option<LanguageIcon> {
        self.state.borrow().languages.get(language_id)?.icons.first().cloned()
    }

    pub fn get_configuration_files(&self, language_id: &str) -> Vec<URI> {
        self.state.borrow().languages.get(language_id).map(|l| l.configuration_files.clone()).unwrap_or_default()
    }

    pub fn get_language_id_by_language_name(&self, language_name: &str) -> Option<String> {
        self.state.borrow().lowercase_name_map.get(&language_name.to_lowercase()).cloned()
    }

    pub fn get_language_id_by_mime_type(&self, mime_type: Option<&str>) -> Option<String> {
        let mime_type = mime_type.filter(|m| !m.is_empty())?;
        self.state.borrow().mime_types_map.get(mime_type).cloned()
    }

    pub fn guess_language_id_by_filepath_or_first_line(&self, resource: Option<&URI>, first_line: Option<&JsString>) -> Vec<String> {
        if resource.is_none() && first_line.is_none_or(JsString::is_empty) {
            return Vec::new();
        }
        get_language_ids(resource, first_line)
    }
}

impl IDisposable for LanguagesRegistry {
    fn dispose(&self) {
        INSTANCE_COUNT.with(|c| c.set(c.get() - 1));
        IDisposable::dispose(&*self.base);
    }
}
