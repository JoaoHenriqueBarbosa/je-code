//! Porte de `src/vs/workbench/contrib/snippets/browser/snippetsFile.ts`: o snippet (`Snippet`), o arquivo de snippets
//! (`SnippetFile`, JSON com comentários) e a origem (`SnippetSource`).
//!
//! Notas de tradução:
//! - `Snippet#scopes` é o array que o `SnippetFile` compartilha com todos os snippets que ele cria (o
//!   `defaultScopes`), e o `SnippetsService` acrescenta linguagens a esse array depois de os snippets existirem. O
//!   array compartilhado é o [`SnippetScopes`] (um `Rc<RefCell<Vec<String>>>`); o construtor de [`Snippet`] recebe
//!   qualquer coisa que vire [`SnippetScopes`], inclusive um `Vec<String>` (que cria um array novo).
//! - `WindowIdleValue(getActiveWindow(), ...)` é o `GlobalIdleValue` (o porte não tem janelas): o `SnippetBodyInsights`
//!   é calculado quando o motor fica ocioso ou na primeira leitura.
//! - O texto é UTF-8 (`String`); o `SnippetParser` e as colunas do editor contam unidades UTF-16, então as conversões
//!   acontecem na fronteira (`encode_utf16`).
//! - `SnippetFile#load` devolve `Promise<this>`; aqui é `Promise<Rc<SnippetFile>>`. A promessa guardada é a do
//!   carregamento (`Promise<()>`) e cada chamada de `load` devolve uma promessa derivada dela, para o arquivo não
//!   guardar uma referência a si mesmo.
//! - O objeto JSON é o [`Value`] de `je_base::json`; `Object.entries` e a desestruturação de valores que não são
//!   objetos seguem as regras do JavaScript (`null` lança `TypeError`, primitivos não têm propriedades).
//!
//! Divergências:
//! - `IExtensionResourceLoaderService` (`platform/extensionResourceLoader/common`) ainda não existe no porte: o
//!   contrato mínimo que o `SnippetFile` lê (`readExtensionResource`) está aqui, em [`IExtensionResourceLoaderService`],
//!   e migra para a plataforma quando o serviço for portado.
//! - `IExtensionDescription` é o [`ExtensionDescription`] do je-extensions, que guarda o local da extensão como
//!   texto: o `extensionLocation` do original é a URI `file` desse caminho.
//! - `include` e `exclude` aceitam só texto: os elementos que não são texto num array do JSON (que lançariam
//!   `TypeError` em `pattern.includes` no original) são descartados.
//! - O `Object.entries` de um texto devolve um item por caractere, e nenhum deles é objeto, então não gera snippet;
//!   o laço sai direto.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::GlobalIdleValue;
use je_base::errors::Error;
use je_base::glob::{self, GlobOptions};
use je_base::js_string::trim;
use je_base::json::{self, ParseOptions, Value};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::snippet::snippet_parser::{Marker, MarkerRef, SnippetParser};
use je_editor::snippet::snippet_variables::is_known_snippet_variable_name;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_instantiation::service_id;
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::{ext_uri, fs_path, uri_file};

/// `IExtensionResourceLoaderService`, só com o que o arquivo de snippets lê (ver as divergências do módulo).
pub trait IExtensionResourceLoaderService {
    /// `readExtensionResource(uri)`: o conteúdo de um recurso dentro de uma extensão.
    fn read_extension_resource(&self, uri: &URI) -> Promise<String>;
}

service_id!(pub I_EXTENSION_RESOURCE_LOADER_SERVICE: dyn IExtensionResourceLoaderService = "extensionResourceLoaderService");

/// O texto como unidades UTF-16.
fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `String#toLowerCase` do JavaScript.
fn to_lower_case(text: &str) -> String {
    JsString::from(text).to_lower_case().to_string_lossy()
}

/// `String#trim` do JavaScript.
fn trim_js(text: &str) -> String {
    JsString::from_units(trim(&units(text)).to_vec()).to_string_lossy()
}

/// `basename` e `extname` do módulo `path` do original (o da plataforma).
fn native_path() -> PathModule<'static> {
    PathModule::native(&ProcessEnvironment)
}

pub struct SnippetBodyInsights {
    pub code_snippet: String,

    /// The snippet uses bad placeholders which collide with variable names.
    pub is_bogous: bool,

    /// The snippet has no placeholder of the final placeholder is at the end.
    pub is_trivial: bool,

    pub uses_clipboard_variable: bool,
    pub uses_selection_variable: bool,
}

impl SnippetBodyInsights {
    fn new(body: &str) -> Self {
        // init with defaults
        let mut is_bogous = false;
        let mut is_trivial = false;
        let mut uses_clipboard_variable = false;
        let mut uses_selection_variable = false;
        let mut code_snippet = body.to_owned();

        // check snippet...
        let textmate_snippet = SnippetParser::new().parse(&units(body), false, false);

        let mut placeholders: HashMap<String, u64> = HashMap::new();
        let mut placeholder_max: u64 = 0;
        let all_placeholders = textmate_snippet.placeholders();
        for placeholder in &all_placeholders {
            placeholder_max = placeholder_max.max(placeholder.as_placeholder().expect("Placeholder").index);
        }

        // mark snippet as trivial when there is no placeholders or when the only
        // placeholder is the final tabstop and it is at the very end.
        if all_placeholders.is_empty() {
            is_trivial = true;
        } else if placeholder_max == 0 {
            let children = textmate_snippet.children();
            is_trivial = children.last().and_then(|last| last.as_placeholder()).is_some_and(|last| last.is_final_tabstop());
        }

        let mut stack: VecDeque<MarkerRef> = textmate_snippet.children().into_iter().collect();
        while let Some(marker) = stack.pop_front() {
            if let Some(variable) = marker.as_variable() {
                if marker.children().is_empty() && !is_known_snippet_variable_name(&variable.name) {
                    // a 'variable' without a default value and not being one of our supported
                    // variables is automatically turned into a placeholder. This is to restore
                    // a bug we had before. So `${foo}` becomes `${N:foo}`
                    let index = match placeholders.get(&variable.name) {
                        Some(index) => *index,
                        None => {
                            placeholder_max += 1;
                            placeholder_max
                        }
                    };
                    placeholders.insert(variable.name.clone(), index);

                    let synthetic = Marker::new_placeholder(index);
                    synthetic.append_child(Marker::new_text(variable.name.as_str()));
                    textmate_snippet.replace(&marker, vec![synthetic]);
                    is_bogous = true;
                }

                match variable.name.as_str() {
                    "CLIPBOARD" => uses_clipboard_variable = true,
                    "SELECTION" | "TM_SELECTED_TEXT" => uses_selection_variable = true,
                    _ => {}
                }
            } else {
                // recurse
                stack.extend(marker.children());
            }
        }

        if is_bogous {
            code_snippet = textmate_snippet.to_textmate_string().to_string_lossy();
        }

        Self { code_snippet, is_bogous, is_trivial, uses_clipboard_variable, uses_selection_variable }
    }
}

/// O array `scopes` de um snippet: compartilhado com o `SnippetFile` (e mutável por ele), como o array do original.
#[derive(Clone, Debug, Default)]
pub struct SnippetScopes(Rc<RefCell<Vec<String>>>);

impl SnippetScopes {
    pub fn new(scopes: Vec<String>) -> Self {
        Self(Rc::new(RefCell::new(scopes)))
    }

    /// `scopes.push(scope)`.
    pub fn push(&self, scope: String) {
        self.0.borrow_mut().push(scope);
    }

    /// `scopes.length`.
    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }

    /// Alguma posição do array é igual a `scope` (o laço de `_scopeSelect`).
    pub fn contains(&self, scope: &str) -> bool {
        self.0.borrow().iter().any(|candidate| candidate == scope)
    }

    /// Uma cópia dos elementos.
    pub fn to_vec(&self) -> Vec<String> {
        self.0.borrow().clone()
    }
}

impl From<Vec<String>> for SnippetScopes {
    fn from(scopes: Vec<String>) -> Self {
        Self::new(scopes)
    }
}

pub struct Snippet {
    body_insights: GlobalIdleValue<Rc<SnippetBodyInsights>>,

    pub prefix_low: String,

    pub is_file_template: bool,
    pub scopes: SnippetScopes,
    pub name: String,
    pub prefix: String,
    pub description: String,
    pub body: String,
    pub source: String,
    pub snippet_source: SnippetSource,
    pub snippet_identifier: String,
    pub include: Option<Vec<String>>,
    pub exclude: Option<Vec<String>>,
    pub extension_id: Option<ExtensionIdentifier>,
}

impl Snippet {
    #[allow(clippy::too_many_arguments)] // os parâmetros do construtor do original
    pub fn new(
        is_file_template: bool,
        scopes: impl Into<SnippetScopes>,
        name: &str,
        prefix: &str,
        description: &str,
        body: &str,
        source: &str,
        snippet_source: SnippetSource,
        snippet_identifier: &str,
        include: Option<Vec<String>>,
        exclude: Option<Vec<String>>,
        extension_id: Option<ExtensionIdentifier>,
    ) -> Self {
        let insights_body = body.to_owned();
        Self {
            body_insights: GlobalIdleValue::new(move || Rc::new(SnippetBodyInsights::new(&insights_body))),
            prefix_low: to_lower_case(prefix),
            is_file_template,
            scopes: scopes.into(),
            name: name.to_owned(),
            prefix: prefix.to_owned(),
            description: description.to_owned(),
            body: body.to_owned(),
            source: source.to_owned(),
            snippet_source,
            snippet_identifier: snippet_identifier.to_owned(),
            include,
            exclude,
            extension_id,
        }
    }

    pub fn code_snippet(&self) -> String {
        self.body_insights.value().code_snippet.clone()
    }

    pub fn is_bogous(&self) -> bool {
        self.body_insights.value().is_bogous
    }

    pub fn is_trivial(&self) -> bool {
        self.body_insights.value().is_trivial
    }

    pub fn needs_clipboard(&self) -> bool {
        self.body_insights.value().uses_clipboard_variable
    }

    pub fn uses_selection(&self) -> bool {
        self.body_insights.value().uses_selection_variable
    }

    pub fn is_file_included(&self, resource_uri: &URI) -> bool {
        let uri_path = if resource_uri.scheme() == schemas::FILE { fs_path(resource_uri) } else { resource_uri.path().to_string_lossy() };
        let file_name = native_path().basename(&uri_path, None);

        let get_match_target = |pattern: &str| -> String { if pattern.contains('/') { uri_path.clone() } else { file_name.clone() } };

        let options = GlobOptions { ignore_case: Some(true), ..GlobOptions::default() };

        if let Some(exclude) = &self.exclude {
            for pattern in exclude.iter().filter(|pattern| !pattern.is_empty()) {
                if glob::match_pattern(pattern.as_str(), &get_match_target(pattern), &options) {
                    return false;
                }
            }
        }

        if let Some(include) = &self.include {
            for pattern in include.iter().filter(|pattern| !pattern.is_empty()) {
                if glob::match_pattern(pattern.as_str(), &get_match_target(pattern), &options) {
                    return true;
                }
            }
            return false;
        }

        true
    }
}

/// `isObject(thing) && Boolean(thing.body)` e o `Boolean(...)` do JavaScript sobre um valor JSON.
fn is_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => *value != 0.0 && !value.is_nan(),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

fn is_json_serialized_snippet(thing: &Value) -> bool {
    match thing {
        Value::Object(object) => is_truthy(object.get("body")),
        _ => false,
    }
}

/// A propriedade `key` de um valor: só objetos têm (o resto devolve `undefined`).
fn property<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(object) => object.get(key),
        _ => None,
    }
}

/// O número do JavaScript como texto (`String(number)`).
fn number_to_string(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e21 { format!("{}", value as i128) } else { format!("{value}") }
}

/// `String(value)`.
fn js_string_of(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => number_to_string(*value),
        Value::String(text) => text.to_string_lossy(),
        Value::Array(items) => join_values(items, ","),
        Value::Object(_) => "[object Object]".to_owned(),
    }
}

/// `array.join(separator)`: `null` vira texto vazio.
fn join_values(items: &[Value], separator: &str) -> String {
    items
        .iter()
        .map(|item| match item {
            Value::Null => String::new(),
            other => js_string_of(other),
        })
        .collect::<Vec<_>>()
        .join(separator)
}

/// `Object.entries(value)`; `null` lança `TypeError`.
fn object_entries(value: &Value) -> Result<Vec<(String, Value)>, Error> {
    match value {
        Value::Object(object) => Ok(object.iter().map(|(key, value)| (key.to_string_lossy(), value.clone())).collect()),
        Value::Array(items) => Ok(items.iter().enumerate().map(|(index, item)| (index.to_string(), item.clone())).collect()),
        // um texto dá um item por caractere, e nenhum é um objeto com `body`; números e booleanos não têm entradas
        Value::String(_) | Value::Number(_) | Value::Bool(_) => Ok(Vec::new()),
        Value::Null => Err(Error::named("TypeError", "Cannot convert undefined or null to object")),
    }
}

/// `string | string[]` de `include` e `exclude`.
fn pattern_list(value: Option<&Value>) -> Option<Vec<String>> {
    match value {
        Some(Value::Array(items)) => Some(
            items
                .iter()
                .filter_map(|item| match item {
                    Value::String(text) => Some(text.to_string_lossy()),
                    _ => None,
                })
                .collect(),
        ),
        Some(Value::String(text)) if !text.is_empty() => Some(vec![text.to_string_lossy()]),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SnippetSource {
    User = 1,
    Workspace = 2,
    Extension = 3,
}

pub struct SnippetFile {
    pub source: SnippetSource,
    pub location: URI,
    pub data: RefCell<Vec<Rc<Snippet>>>,
    pub is_global_snippets: bool,
    pub is_user_snippets: bool,

    default_scopes: RefCell<Option<SnippetScopes>>,
    extension: Option<Rc<ExtensionDescription>>,
    file_service: Rc<dyn IFileService>,
    extension_resource_loader_service: Rc<dyn IExtensionResourceLoaderService>,
    load_promise: RefCell<Option<Promise<()>>>,
}

impl SnippetFile {
    pub fn new(
        source: SnippetSource,
        location: URI,
        default_scopes: Option<SnippetScopes>,
        extension: Option<Rc<ExtensionDescription>>,
        file_service: Rc<dyn IFileService>,
        extension_resource_loader_service: Rc<dyn IExtensionResourceLoaderService>,
    ) -> Rc<Self> {
        let is_global_snippets = native_path().extname(&location.path().to_string_lossy()) == ".code-snippets";
        let is_user_snippets = extension.is_none();
        Rc::new(Self {
            source,
            location,
            data: RefCell::new(Vec::new()),
            is_global_snippets,
            is_user_snippets,
            default_scopes: RefCell::new(default_scopes),
            extension,
            file_service,
            extension_resource_loader_service,
            load_promise: RefCell::new(None),
        })
    }

    /// `defaultScopes`.
    pub fn default_scopes(&self) -> Option<SnippetScopes> {
        self.default_scopes.borrow().clone()
    }

    /// `defaultScopes = value`.
    pub fn set_default_scopes(&self, value: Option<SnippetScopes>) {
        *self.default_scopes.borrow_mut() = value;
    }

    pub fn select(&self, selector: &str, bucket: &mut Vec<Rc<Snippet>>) {
        if self.is_global_snippets || !self.is_user_snippets {
            self.scope_select(selector, bucket);
        } else {
            self.filepath_select(selector, bucket);
        }
    }

    fn filepath_select(&self, selector: &str, bucket: &mut Vec<Rc<Snippet>>) {
        // for `fooLang.json` files apply inclusion/exclusion rules only
        if format!("{selector}.json") == native_path().basename(&self.location.path().to_string_lossy(), None) {
            bucket.extend(self.data.borrow().iter().cloned());
        }
    }

    fn scope_select(&self, selector: &str, bucket: &mut Vec<Rc<Snippet>>) {
        // for `my.code-snippets` files we need to look at each snippet
        for snippet in self.data.borrow().iter() {
            if snippet.scopes.is_empty() {
                // always accept
                bucket.push(Rc::clone(snippet));
            } else if snippet.scopes.contains(selector) {
                // match only once!
                bucket.push(Rc::clone(snippet));
            }
        }

        if let Some(index) = selector.rfind('.') {
            self.scope_select(&selector[..index], bucket);
        }
    }

    fn read_content(&self) -> Promise<String> {
        if self.extension.is_some() {
            self.extension_resource_loader_service.read_extension_resource(&self.location)
        } else {
            self.file_service.read_file(&self.location, None, None).then(|content| content.value.to_string())
        }
    }

    pub fn load(self: &Rc<Self>) -> Promise<Rc<SnippetFile>> {
        if self.load_promise.borrow().is_none() {
            let this = Rc::clone(self);
            let promise = self.read_content().then_result(move |content| this.parse_content(&content?));
            *self.load_promise.borrow_mut() = Some(promise);
        }
        let loaded = self.load_promise.borrow().clone().expect("a promessa de carregamento foi criada acima");
        let this = Rc::clone(self);
        loaded.then_result(move |result| result.map(|()| this))
    }

    pub fn reset(&self) {
        *self.load_promise.borrow_mut() = None;
        self.data.borrow_mut().clear();
    }

    /// O corpo do `.then(content => ...)` de `load`.
    fn parse_content(&self, content: &str) -> Result<(), Error> {
        let mut errors = Vec::new();
        let data = json::parse(&units(content), &mut errors, ParseOptions::default());
        if let Some(Value::Object(object)) = data {
            for (name, scope_or_template) in object.iter() {
                if is_json_serialized_snippet(scope_or_template) {
                    self.parse_snippet(&name.to_string_lossy(), scope_or_template)?;
                } else {
                    for (name, template) in object_entries(scope_or_template)? {
                        self.parse_snippet(&name, &template)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn parse_snippet(&self, name: &str, snippet: &Value) -> Result<(), Error> {
        // `let { isFileTemplate, prefix, body, description } = snippet;`
        if matches!(snippet, Value::Null) {
            return Err(Error::named("TypeError", "Cannot destructure property 'isFileTemplate' of 'snippet' as it is null."));
        }
        let is_file_template = property(snippet, "isFileTemplate");
        let prefix = property(snippet, "prefix");
        let body = property(snippet, "body");
        let description = property(snippet, "description");

        let body = match body {
            Some(Value::Array(items)) => join_values(items, "\n"),
            Some(Value::String(text)) => text.to_string_lossy(),
            _ => return Ok(()),
        };

        let description = match description {
            None | Some(Value::Null) => String::new(),
            Some(Value::Array(items)) => join_values(items, "\n"),
            Some(other) => js_string_of(other),
        };

        let scopes: SnippetScopes = if let Some(default_scopes) = self.default_scopes() {
            default_scopes
        } else if let Some(Value::String(scope)) = property(snippet, "scope") {
            SnippetScopes::new(scope.to_string_lossy().split(',').map(trim_js).filter(|scope| !scope.is_empty()).collect())
        } else {
            SnippetScopes::new(Vec::new())
        };

        let include = pattern_list(property(snippet, "include"));
        let exclude = pattern_list(property(snippet, "exclude"));

        let source = if let Some(extension) = &self.extension {
            // extension snippet -> show the name of the extension
            extension.manifest.display_name.clone().filter(|display_name| !display_name.is_empty()).unwrap_or_else(|| extension.manifest.name.clone())
        } else if self.source == SnippetSource::Workspace {
            // workspace -> only *.code-snippets files
            localize("source.workspaceSnippetGlobal", "Workspace Snippet", &[])
        } else if self.is_global_snippets {
            // user -> global (*.code-snippets) and language snippets
            localize("source.userSnippetGlobal", "Global User Snippet", &[])
        } else {
            localize("source.userSnippet", "User Snippet", &[])
        };

        let snippet_identifier = match &self.extension {
            Some(extension) => {
                let relative = ext_uri().relative_path(&uri_file(&extension.extension_location), &self.location);
                format!("{}/{name}", relative.as_deref().unwrap_or("undefined"))
            }
            None => format!("{}/{name}", native_path().basename(&self.location.path().to_string_lossy(), None)),
        };

        // `!prefix` vira `''`; um array dá um snippet por elemento (o `Iterable.wrap`), e todo o resto é um prefixo só
        let prefixes: Vec<Result<String, Error>> = match prefix {
            Some(Value::Array(items)) => items.iter().map(prefix_text).collect(),
            Some(value) if is_truthy(Some(value)) => vec![prefix_text(value)],
            _ => vec![Ok(String::new())],
        };

        for prefix in prefixes {
            let prefix = prefix?;
            self.data.borrow_mut().push(Rc::new(Snippet::new(
                is_truthy(is_file_template),
                scopes.clone(),
                name,
                &prefix,
                &description,
                &body,
                &source,
                self.source,
                &snippet_identifier,
                include.clone(),
                exclude.clone(),
                self.extension.as_ref().map(|extension| extension.identifier.clone()),
            )));
        }
        Ok(())
    }
}

/// O prefixo de um snippet: `prefix.toLowerCase()` do construtor lança `TypeError` para o que não é texto.
fn prefix_text(value: &Value) -> Result<String, Error> {
    match value {
        Value::String(text) => Ok(text.to_string_lossy()),
        _ => Err(Error::named("TypeError", "prefix.toLowerCase is not a function")),
    }
}
