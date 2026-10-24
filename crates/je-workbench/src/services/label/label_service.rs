//! Porte de `src/vs/workbench/services/label/common/labelService.ts`: o `LabelService`, que monta o rótulo legível de
//! URIs, de workspaces e de hosts a partir dos formatadores registrados (por extensões, pelo workbench e pelo cache
//! que sobrevive à janela), e o `ResourceLabelFormattersHandler`, que registra os formatadores contribuídos por
//! extensões.
//!
//! Notas de tradução:
//! - **Injeção.** Os parâmetros decorados são a tupla `Deps` do [`Injectable`], e `create` é o construtor (os testes
//!   o chamam direto). `registerSingleton(ILabelService, LabelService, InstantiationType.Delayed)` é [`register`]; o
//!   proxy da instanciação atrasada é o `LabelServiceProxy` do contrato. A classe herda de `Disposable`, aqui um
//!   `Disposable` no campo `base`.
//! - **Texto.** Os rótulos são `String`: o `getPathLabel` do `je-base` e as operações de caminho já trabalham assim,
//!   e um surrogate isolado vira U+FFFD. A comparação `relativeLabel[overlap]` (índice em unidades UTF-16) é feita
//!   sobre `JsString`.
//! - **Expressão regular.** `labelMatchingRegexp` (`/\$\{(scheme|authoritySuffix|authority|path|(query)\.(.+?))\}/g`)
//!   é o analisador [`parse_label_token`], com as mesmas alternativas na mesma ordem e o `.+?` preguiçoso que não
//!   atravessa terminador de linha. `posixPathSeparatorRegexp` e `winPathSeparatorRegexp` são as trocas de
//!   [`LabelService::adjust_path_separators`]. O `JSON.parse(query)[chave] || ''` lê o valor do JSON e o converte
//!   para texto como o JavaScript.
//! - **Formatadores.** `this.formatters` guarda os formatadores por identidade (`Rc`), como o original, e a lista do
//!   cache em disco (`storedFormatters.formatters`) é o array dinâmico dentro do memento, que `registerCachedFormatter`
//!   edita no lugar. Um item do cache que não seja um objeto com `formatting` objeto não vira formatador.
//! - **Ponto de extensão.** O registro do esquema JSON do ponto `resourceLabelFormatters` (`jsonSchema`) e o
//!   `Registry.registerWorkbenchContribution(..., LifecyclePhase.Restored)` não existem no porte (o registro de pontos
//!   de extensão não tem esquema nem tratador, ver `extensions_registry`): [`register_extension_point`] registra o
//!   nome do ponto, e o [`ResourceLabelFormattersHandler`] recebe as mudanças (`delta.added` e `delta.removed`) de
//!   quem hospedar o ponto. `isProposedApiEnabled(added.description, 'contribLabelFormatterWorkspaceTooltip')` chega
//!   como o booleano `proposed_api_enabled` de cada contribuição.
//! - **`removed`.** O original guarda o desfazer pela cópia (`{ ...untrustedFormatter }`) e procura pela instância
//!   original ao remover, então nunca encontra: o comportamento é preservado.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::{ArrayRef, Dynamic, ObjectRef, number_to_js_string};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, EmitterOptions, Event};
use je_base::glob::{self, GlobOptions};
use je_base::json::Value;
use je_base::jsonc::json_parse;
use je_base::labels::{self, PathLabelFormatting, RelativePathProvider, UserHomeProvider};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, plain_disposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::path::PathModule;
use je_base::platform::{OperatingSystem, Platform};
use je_base::resources::ExtUri;
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_instantiation::{Injectable, InstantiationType, register_singleton, service_impl};
use je_platform::label::label::{
    FormatterChangeEvent, ILabelService, I_LABEL_SERVICE, LabelFormatter, LabelSeparator, ResourceLabelFormatter, ResourceLabelFormatting,
    ResourceLabelFormattingContext, UriLabelOptions, Verbosity, WorkspaceLabelOptions, WorkspaceLabelTarget,
};
use je_platform::path::path_service::IPathService;
use je_platform::resources::{ext_uri, host_environment};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::{
    AnyWorkspaceIdentifier, IWorkspaceContextService, WORKSPACE_EXTENSION, WorkspaceFolder, is_temporary_workspace, is_untitled_workspace,
    to_workspace_identifier,
};
use je_platform::js::set_property;

use super::resource_label_template::{ResourceLabelTemplate, with_path_only};
use crate::common::memento::Memento;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::extensions::extensions_registry::{ExtensionPoint, ExtensionPointDescriptor, extensions_registry};
use crate::services::lifecycle::lifecycle::ILifecycleService;
use crate::services::remote::remote_agent_service::IRemoteAgentService;

const FORMATTER_CACHE_SIZE: usize = 50;

/// `sep` do módulo `path` nativo, como separador de rótulo.
pub fn native_label_separator() -> LabelSeparator {
    LabelSeparator::from_text(PathModule::native(host_environment().as_ref()).sep()).unwrap_or_default()
}

/// `registerExtensionPoint({ extensionPoint: 'resourceLabelFormatters', ... })`.
pub fn register_extension_point() -> Rc<ExtensionPoint> {
    extensions_registry().register_extension_point(ExtensionPointDescriptor {
        extension_point: "resourceLabelFormatters".to_string(),
        default_extension_kind: None,
        can_handle_resolver: None,
    })
}

// ---------------------------------------------------------------------------------------------
// `ResourceLabelFormattersHandler`
// ---------------------------------------------------------------------------------------------

/// Uma entrada de `delta.added`: os formatadores do `package.json` (não confiáveis) e se a extensão tem a API
/// proposta `contribLabelFormatterWorkspaceTooltip`.
pub struct ExtensionLabelFormatters {
    pub formatters: Vec<Dynamic>,
    pub proposed_api_enabled: bool,
}

/// `ResourceLabelFormattersHandler`.
pub struct ResourceLabelFormattersHandler {
    label_service: Rc<dyn ILabelService>,
    formatters_disposables: RefCell<HashMap<usize, DisposableRef>>,
}

impl ResourceLabelFormattersHandler {
    pub fn new(label_service: Rc<dyn ILabelService>) -> Self {
        Self { label_service, formatters_disposables: RefCell::new(HashMap::new()) }
    }

    /// O corpo do tratador do ponto de extensão: `delta.added` e `delta.removed` (os formatadores originais das
    /// extensões removidas).
    pub fn handle_delta(&self, added: &[ExtensionLabelFormatters], removed: &[Vec<Dynamic>]) {
        for added in added {
            for untrusted_formatter in &added.formatters {
                // We cannot trust that the formatter as it comes from an extension
                // adheres to our interface, so for the required properties we fill
                // in some defaults if missing.
                let Dynamic::Object(untrusted) = untrusted_formatter else {
                    errors::throw(Error::type_error("Cannot read properties of a non-object resource label formatter"));
                };
                let formatter = Dynamic::Object(ObjectRef::from_entries(untrusted.entries()));
                let formatting = formatter.get_str("formatting");
                if !matches!(formatting, Dynamic::Object(_)) {
                    errors::throw(Error::type_error("Cannot read properties of undefined (reading 'label')"));
                }
                if !matches!(formatting.get_str("label"), Dynamic::String(_)) {
                    set_property(&formatting, &JsString::from("label"), Dynamic::string("${authority}${path}"));
                }
                if !matches!(formatting.get_str("separator"), Dynamic::String(_)) {
                    set_property(&formatting, &JsString::from("separator"), Dynamic::string(native_label_separator().as_str()));
                }
                if !added.proposed_api_enabled && matches!(formatting.get_str("workspaceTooltip"), Dynamic::String(ref tooltip) if !tooltip.is_empty()) {
                    set_property(&formatting, &JsString::from("workspaceTooltip"), Dynamic::Undefined); // workspaceTooltip is only proposed
                }

                if let (Some(key), Some(resource_formatter)) = (formatter.identity(), ResourceLabelFormatter::from_dynamic(&formatter, native_label_separator())) {
                    let registration = self.label_service.register_formatter(LabelFormatter::Resource(Rc::new(resource_formatter)));
                    self.formatters_disposables.borrow_mut().insert(key, registration);
                }
            }
        }

        for removed in removed {
            for formatter in removed {
                let registration = formatter.identity().and_then(|key| self.formatters_disposables.borrow().get(&key).cloned());
                if let Some(registration) = registration {
                    registration.dispose();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// `LabelService`
// ---------------------------------------------------------------------------------------------

/// `IHomeFormatterRegistration`.
struct HomeFormatterRegistration {
    formatter: Rc<je_platform::label::label::ResourceLabelTemplateFormatter>,
    template: ResourceLabelTemplate,
}

/// `IResolvedHomeFormatter`.
struct ResolvedHomeFormatter {
    home: URI,
    formatting: ResourceLabelFormatting,
    literal_label: bool,
    home_length: usize,
    authority_length: usize,
}

/// `this.userHome.fsPath` e a lista de pastas do workspace, vistos pelo `getPathLabel` do `je-base`.
struct WorkspaceRelativePathProvider<'a> {
    context_service: &'a dyn IWorkspaceContextService,
    no_prefix: bool,
}

fn to_label_folder(folder: &WorkspaceFolder) -> labels::WorkspaceFolder {
    labels::WorkspaceFolder { uri: folder.uri.clone(), name: Some(folder.name.clone()) }
}

impl RelativePathProvider for WorkspaceRelativePathProvider<'_> {
    fn no_prefix(&self) -> bool {
        self.no_prefix
    }

    fn get_workspace(&self) -> labels::Workspace {
        labels::Workspace { folders: self.context_service.get_workspace().folders().iter().map(to_label_folder).collect() }
    }

    fn get_workspace_folder(&self, resource: &URI) -> Option<labels::WorkspaceFolder> {
        self.context_service.get_workspace_folder(resource).map(|folder| to_label_folder(&folder))
    }
}

/// `hasDriveLetterIgnorePlatform`.
fn has_drive_letter_ignore_platform(path: &str) -> bool {
    JsString::from(path).units().get(2) == Some(&u16::from(b':'))
}

/// O comprimento em unidades UTF-16 (`string.length`).
fn js_length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Um texto que o JavaScript trata como verdadeiro (não vazio).
fn non_empty(text: &Option<String>) -> Option<&str> {
    text.as_deref().filter(|text| !text.is_empty())
}

/// `match(pattern, value, { ignoreCase: true })`.
fn authority_matches(pattern: &str, authority: &JsString) -> bool {
    glob::match_pattern(pattern, &authority.to_string_lossy(), &GlobOptions { ignore_case: Some(true), ..GlobOptions::default() })
}

/// O que `labelMatchingRegexp` captura.
enum LabelToken {
    Scheme,
    AuthoritySuffix,
    Authority,
    Path,
    Query(String),
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// O `${...}` que começa no início de `rest`: o token e quantos bytes ele ocupa. As alternativas são as de
/// `labelMatchingRegexp`, na mesma ordem.
fn parse_label_token(rest: &str) -> Option<(LabelToken, usize)> {
    let body = rest.strip_prefix("${")?;
    let simple = [("scheme", LabelToken::Scheme), ("authoritySuffix", LabelToken::AuthoritySuffix), ("authority", LabelToken::Authority), ("path", LabelToken::Path)];
    for (name, token) in simple {
        if body.strip_prefix(name).is_some_and(|tail| tail.starts_with('}')) {
            return Some((token, 2 + name.len() + 1));
        }
    }

    // (query)\.(.+?)\}: o primeiro caractere do valor é obrigatório (e pode ser a própria `}`), e o resto vai até a
    // primeira `}`, sem atravessar terminador de linha.
    let tail = body.strip_prefix("query.")?;
    let mut chars = tail.char_indices();
    let (_, first) = chars.next()?;
    if is_line_terminator(first) {
        return None;
    }
    for (index, c) in chars {
        if c == '}' {
            return Some((LabelToken::Query(tail[..index].to_string()), 2 + "query.".len() + index + 1));
        }
        if is_line_terminator(c) {
            return None;
        }
    }
    None
}

/// `label.replace(labelMatchingRegexp, ...)`: `resolve` devolve o texto de cada token.
fn replace_label_tokens(label: &str, resolve: impl Fn(LabelToken) -> String) -> String {
    let mut result = String::new();
    let mut index = 0;
    while index < label.len() {
        let rest = &label[index..];
        if let Some((token, length)) = parse_label_token(rest) {
            result.push_str(&resolve(token));
            index += length;
            continue;
        }
        let c = rest.chars().next().unwrap_or_default();
        result.push(c);
        index += c.len_utf8();
    }
    result
}

/// A veracidade de um valor JSON (`value || ''`).
fn json_is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => *number != 0.0 && !number.is_nan(),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `String(value)` de um valor JSON.
fn json_to_js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number_to_js_string(*number),
        Value::String(text) => text.to_string_lossy(),
        Value::Array(items) => items.iter().map(|item| if matches!(item, Value::Null) { String::new() } else { json_to_js_string(item) }).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// `JSON.parse(query)[key] || ''` para uma consulta no formato `{...}`; qualquer erro dá o texto vazio.
fn query_value(query: &JsString, key: &str) -> String {
    let units = query.units();
    if units.first() != Some(&u16::from(b'{')) || units.last() != Some(&u16::from(b'}')) {
        return String::new();
    }
    match json_parse(units) {
        Ok(Value::Object(object)) => object.get(key).filter(|value| json_is_truthy(value)).map(json_to_js_string).unwrap_or_default(),
        _ => String::new(),
    }
}

/// O formatador de host: `URI.from({ scheme, authority })`.
fn host_uri(scheme: &str, authority: Option<&str>) -> URI {
    URI::from(&UriComponents { scheme: Some(JsString::from(scheme)), authority: authority.map(JsString::from), ..UriComponents::default() }, false)
}

/// `LabelService`.
pub struct LabelService {
    base: Rc<Disposable>,
    formatters: Rc<RefCell<Vec<Rc<ResourceLabelFormatter>>>>,
    home_formatters: Rc<RefCell<Vec<Rc<HomeFormatterRegistration>>>>,
    on_did_change_formatters: Rc<Emitter<FormatterChangeEvent>>,
    stored_formatters_memento: Memento,
    stored_formatters: ObjectRef,
    os: Rc<Cell<OperatingSystem>>,
    user_home: Rc<RefCell<Option<URI>>>,
    host: Platform,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    context_service: Rc<dyn IWorkspaceContextService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
}

impl Injectable for LabelService {
    type Args = ();
    type Deps = (
        Rc<dyn IWorkbenchEnvironmentService>,
        Rc<dyn IWorkspaceContextService>,
        Rc<dyn IPathService>,
        Rc<dyn IRemoteAgentService>,
        Rc<dyn StorageService>,
        Rc<dyn ILifecycleService>,
        Rc<dyn IUriIdentityService>,
    );

    fn create(
        _: (),
        (environment_service, context_service, path_service, remote_agent_service, storage_service, _lifecycle_service, uri_identity_service): Self::Deps,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_formatters = Emitter::with_options(EmitterOptions {
            leak_warning_threshold: Some(400),
            leak_warning_name: Some("LabelService._onDidChangeFormatters".to_string()),
            ..EmitterOptions::default()
        });
        base.register(on_did_change_formatters.clone());

        // Find some meaningful defaults until the remote environment
        // is resolved, by taking the current OS we are running in
        // and by taking the local `userHome` if we run on a local
        // file scheme.
        let host = Platform::detect();
        let os = Rc::new(Cell::new(host.os));
        let initial_user_home = (path_service.default_uri_scheme() == schemas::FILE).then(|| path_service.local_user_home());
        let user_home = Rc::new(RefCell::new(initial_user_home));

        let memento = Memento::new("cachedResourceLabelFormatters2", storage_service);
        let stored_formatters = memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
        let formatters: Vec<Rc<ResourceLabelFormatter>> = match stored_formatters.get(&JsString::from("formatters")) {
            Some(Dynamic::Array(list)) => {
                let separator = native_label_separator();
                list.to_vec().iter().filter_map(|item| ResourceLabelFormatter::from_dynamic(item, separator)).map(Rc::new).collect()
            }
            _ => Vec::new(),
        };

        // Remote environment is potentially long running
        Self::resolve_remote_environment(remote_agent_service.as_ref(), path_service, os.clone(), user_home.clone());

        Rc::new(Self {
            base,
            formatters: Rc::new(RefCell::new(formatters)),
            home_formatters: Rc::new(RefCell::new(Vec::new())),
            on_did_change_formatters,
            stored_formatters_memento: memento,
            stored_formatters,
            os,
            user_home,
            host,
            environment_service,
            context_service,
            uri_identity_service,
        })
    }
}

service_impl!(LabelService => dyn ILabelService, disposable);

/// `registerSingleton(ILabelService, LabelService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<LabelService>(I_LABEL_SERVICE, InstantiationType::Delayed);
}

impl IDisposable for LabelService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl LabelService {
    fn resolve_remote_environment(
        remote_agent_service: &dyn IRemoteAgentService,
        path_service: Rc<dyn IPathService>,
        os: Rc<Cell<OperatingSystem>>,
        user_home: Rc<RefCell<Option<URI>>>,
    ) {
        // OS and user home
        let _ = remote_agent_service.get_environment().then_promise(move |env| {
            os.set(env.map_or_else(|| Platform::detect().os, |env| env.os));

            path_service.user_home().then(move |home| *user_home.borrow_mut() = Some(home))
        });
    }

    fn ext_uri(&self) -> ExtUri {
        self.uri_identity_service.ext_uri()
    }

    fn find_home_formatter(&self, resource: &URI) -> Option<ResolvedHomeFormatter> {
        let ext_uri = self.ext_uri();
        let mut best_result = self.find_static_home_formatter(resource, &ext_uri);
        let mut best_result_is_template = false;
        let registrations = self.home_formatters.borrow().clone();
        for registration in registrations {
            let formatter = &registration.formatter;
            if formatter.home.scheme() != resource.scheme()
                || (!formatter.home.authority().is_empty() && formatter.home.authority().to_lower_case() != resource.authority().to_lower_case())
            {
                continue;
            }
            let Some(template_match) = registration.template.match_resource(resource, &ext_uri) else { continue };
            let context = ResourceLabelFormattingContext { resource: resource.clone(), home: template_match.home.clone(), parameters: template_match.parameters };
            let Some(formatting) = (formatter.formatting)(&context) else { continue };

            let result = ResolvedHomeFormatter {
                home_length: template_match.home.path().len(),
                home: template_match.home,
                formatting,
                literal_label: true,
                authority_length: formatter.home.authority().len(),
            };
            let replaces = best_result.as_ref().is_none_or(|best| {
                result.home_length > best.home_length
                    || (result.home_length == best.home_length && result.authority_length > best.authority_length)
                    || (result.home_length == best.home_length && result.authority_length == best.authority_length && !best_result_is_template)
            });
            if replaces {
                best_result = Some(result);
                best_result_is_template = true;
            }
        }
        best_result
    }

    fn find_static_home_formatter(&self, resource: &URI, ext_uri: &ExtUri) -> Option<ResolvedHomeFormatter> {
        let mut best_result: Option<ResolvedHomeFormatter> = None;
        for formatter in self.formatters.borrow().iter() {
            let Some(home) = non_empty(&formatter.home) else { continue };
            if resource.scheme() != formatter.scheme.as_str()
                || non_empty(&formatter.authority).is_some_and(|authority| !authority_matches(authority, resource.authority()))
                || !ext_uri.is_equal_or_parent(resource, &resource.with(&UriChange { path: Field::from(home), ..UriChange::default() }), false, false)
            {
                continue;
            }

            let result = ResolvedHomeFormatter {
                home: with_path_only(resource, home),
                formatting: formatter.formatting.clone(),
                literal_label: false,
                home_length: js_length(home),
                authority_length: non_empty(&formatter.authority).map_or(0, js_length),
            };
            let replaces = best_result.as_ref().is_none_or(|best| {
                result.home_length > best.home_length
                    || (result.home_length == best.home_length && result.authority_length > best.authority_length)
                    || (result.home_length == best.home_length && result.authority_length == best.authority_length && formatter.priority == Some(true))
            });
            if replaces {
                best_result = Some(result);
            }
        }
        best_result
    }

    pub fn find_formatting(&self, resource: &URI) -> Option<ResourceLabelFormatting> {
        let mut best_result: Option<Rc<ResourceLabelFormatter>> = None;
        for formatter in self.formatters.borrow().iter() {
            if non_empty(&formatter.home).is_some() || resource.scheme() != formatter.scheme.as_str() {
                continue;
            }
            let priority = formatter.priority == Some(true);
            let Some(authority) = non_empty(&formatter.authority) else {
                if best_result.is_none() || priority {
                    best_result = Some(formatter.clone());
                }
                continue;
            };
            let best_authority_length = best_result.as_ref().and_then(|best| non_empty(&best.authority)).map(js_length);
            if authority_matches(authority, resource.authority())
                && best_authority_length.is_none_or(|best_length| js_length(authority) > best_length || (js_length(authority) == best_length && priority))
            {
                best_result = Some(formatter.clone());
            }
        }
        best_result.map(|best| best.formatting.clone())
    }

    fn do_get_uri_label(&self, resource: &URI, formatting: Option<&ResourceLabelFormatting>, relative: bool, no_prefix: bool) -> String {
        let Some(formatting) = formatting else {
            let tildify = self.user_home.borrow().clone().map(|user_home| UserHomeProvider { user_home });
            let relative_provider = WorkspaceRelativePathProvider { context_service: self.context_service.as_ref(), no_prefix };
            return labels::get_path_label(
                resource,
                &PathLabelFormatting { os: self.os.get(), tildify: tildify.as_ref(), relative: relative.then_some(&relative_provider as &dyn RelativePathProvider) },
                &self.host,
            );
        };

        // Relative label
        if relative {
            let mut folder = self.context_service.get_workspace_folder(resource);
            if folder.is_none() {
                // It is possible that the resource we want to resolve the
                // workspace folder for is not using the same scheme as
                // the folders in the workspace, so we help by trying again
                // to resolve a workspace folder by trying again with a
                // scheme that is workspace contained.

                let workspace = self.context_service.get_workspace();
                if let Some(first_folder) = workspace.folders().first()
                    && resource.scheme() != first_folder.uri.scheme()
                    && resource.path().units().first() == Some(&u16::from(b'/'))
                {
                    folder = self.context_service.get_workspace_folder(&first_folder.uri.with(&UriChange { path: Field::Set(resource.path().clone()), ..UriChange::default() }));
                }
            }

            if let Some(folder) = folder {
                let folder_label = JsString::from(self.format_uri(&folder.uri, formatting, no_prefix));
                let mut relative_label = JsString::from(self.format_uri(resource, formatting, no_prefix));
                let (folder_units, relative_units) = (folder_label.units(), relative_label.units());
                let mut overlap = 0;
                while relative_units.get(overlap).is_some_and(|unit| folder_units.get(overlap) == Some(unit)) {
                    overlap += 1;
                }

                let separator_unit = formatting.separator.as_str().encode_utf16().next();
                let length = relative_label.len();
                if relative_units.get(overlap).is_none_or(|unit| Some(*unit) == separator_unit) {
                    relative_label = relative_label.slice((1 + overlap).min(length), length);
                } else if overlap == folder_label.len() && folder.uri.path() == "/" {
                    relative_label = relative_label.slice(overlap, length);
                }
                let mut relative_label = relative_label.to_string_lossy();

                // always show root basename if there are multiple folders
                let has_multiple_roots = self.context_service.get_workspace().folders().len() > 1;
                if has_multiple_roots && !no_prefix {
                    let root_name = folder.name.clone();
                    relative_label = if relative_label.is_empty() { root_name } else { format!("{root_name} \u{2022} {relative_label}") };
                }

                return relative_label;
            }
        }

        // Absolute label
        self.format_uri(resource, formatting, no_prefix)
    }

    fn do_get_workspace_label(&self, workspace_uri: &URI, options: Option<&WorkspaceLabelOptions>) -> String {
        // Workspace: Untitled
        if is_untitled_workspace(workspace_uri, &self.environment_service.untitled_workspaces_home()) {
            return localize("untitledWorkspace", "Untitled (Workspace)", &[]);
        }

        // Workspace: Temporary
        if is_temporary_workspace(Some(workspace_uri)) {
            return localize("temporaryWorkspace", "Workspace", &[]);
        }

        // Workspace: Saved
        let ext_uri = ext_uri();
        let mut filename = ext_uri.basename(workspace_uri, None);
        if filename.ends_with(WORKSPACE_EXTENSION) {
            filename = filename.chars().take((filename.chars().count() - WORKSPACE_EXTENSION.chars().count()).saturating_sub(1)).collect();
        }

        let label = match options.map(|options| options.verbose) {
            // skip suffix for short label
            Some(Verbosity::Short) => return filename,
            Some(Verbosity::Long) => {
                let folder = ext_uri.join_path(&ext_uri.dirname(workspace_uri), &[filename.as_str()]);
                let folder_label = self.get_uri_label(&folder, &UriLabelOptions::default());
                localize("workspaceNameVerbose", "{0} (Workspace)", &[folder_label.as_str()])
            }
            Some(Verbosity::Medium) | None => localize("workspaceName", "{0} (Workspace)", &[filename.as_str()]),
        };

        self.append_workspace_suffix(label, workspace_uri)
    }

    fn do_get_single_folder_workspace_label(&self, folder_uri: &URI, options: Option<&WorkspaceLabelOptions>) -> String {
        let label = match options.map(|options| options.verbose) {
            Some(Verbosity::Long) => self.get_uri_label(folder_uri, &UriLabelOptions::default()),
            // skip suffix for short label
            Some(Verbosity::Short) => return Self::folder_basename_label(folder_uri),
            Some(Verbosity::Medium) | None => Self::folder_basename_label(folder_uri),
        };

        self.append_workspace_suffix(label, folder_uri)
    }

    /// `basename(folderUri) || posix.sep`.
    fn folder_basename_label(folder_uri: &URI) -> String {
        let basename = ext_uri().basename(folder_uri, None);
        if basename.is_empty() { "/".to_string() } else { basename }
    }

    fn format_uri(&self, resource: &URI, formatting: &ResourceLabelFormatting, force_no_tildify: bool) -> String {
        let mut label = replace_label_tokens(&formatting.label, |token| match token {
            LabelToken::Scheme => resource.scheme().to_string_lossy(),
            LabelToken::Authority => resource.authority().to_string_lossy(),
            LabelToken::AuthoritySuffix => {
                let authority = resource.authority().to_string_lossy();
                authority.find('+').map_or_else(|| authority.clone(), |index| authority[index + 1..].to_string())
            }
            LabelToken::Path => {
                let mut path_value = resource.path().to_string_lossy();
                if let Some(segments) = formatting.strip_path_segments.filter(|segments| *segments > 0) {
                    let mut pos = 0;
                    for _ in 0..segments {
                        // `pathValue.indexOf('/', pos + 1)`: a barra é ASCII, então a busca em bytes é segura
                        let Some(next) = path_value.as_bytes().iter().skip(pos + 1).position(|byte| *byte == b'/') else { break };
                        pos += 1 + next;
                    }
                    path_value = path_value[pos..].to_string();
                }
                if formatting.strip_path_starting_separator == Some(true) {
                    // `pathValue[0] === formatting.separator ? 1 : 0`
                    let separator = formatting.separator.as_str();
                    let strip = usize::from(!separator.is_empty() && path_value.starts_with(separator));
                    path_value[strip..].to_string()
                } else {
                    path_value
                }
            }
            LabelToken::Query(key) => query_value(resource.query(), &key),
        });

        // convert \c:\something => C:\something
        if formatting.normalize_drive_letter == Some(true) && has_drive_letter_ignore_platform(&label) {
            let units = JsString::from(label.as_str());
            let mut converted = units.slice(1, 2).to_upper_case();
            converted.push_units(units.slice(2, units.len()).units());
            label = converted.to_string_lossy();
        }

        if formatting.tildify == Some(true)
            && !force_no_tildify
            && let Some(user_home) = self.user_home.borrow().clone()
        {
            label = labels::tildify(&label, &user_home.fs_path(self.host.path_flavor()).to_string_lossy(), self.os.get(), &self.host);
        }

        if let Some(prefix) = non_empty(&formatting.authority_prefix)
            && !resource.authority().is_empty()
        {
            label = format!("{prefix}{label}");
        }

        self.adjust_path_separators(&label, formatting.separator)
    }

    /// `label.replace(os === Windows ? /[\\\/]/g : /\//g, separator)`: no Unix a barra invertida é um caractere de
    /// nome de arquivo válido, e no Windows nenhuma das duas barras é.
    fn adjust_path_separators(&self, label: &str, separator: LabelSeparator) -> String {
        let label = label.replace('/', separator.as_str());
        if self.os.get() == OperatingSystem::Windows { label.replace('\\', separator.as_str()) } else { label }
    }

    fn append_workspace_suffix(&self, label: String, uri: &URI) -> String {
        let formatting = self.find_formatting(uri);
        match formatting.as_ref().and_then(|formatting| non_empty(&formatting.workspace_suffix)) {
            Some(suffix) => format!("{label} [{suffix}]"),
            None => label,
        }
    }
}

impl ILabelService for LabelService {
    fn get_uri_label(&self, resource: &URI, options: &UriLabelOptions) -> String {
        let no_prefix = options.no_prefix == Some(true);
        let home_formatter = if no_prefix { None } else { self.find_home_formatter(resource) };
        if let Some(home_formatter) = home_formatter {
            let separator = options.separator.unwrap_or(home_formatter.formatting.separator);
            let resource_path = resource.path();
            let home_casing = resource_path.slice(0, home_formatter.home.path().len().min(resource_path.len()));
            let home_with_resource_casing = home_formatter.home.with(&UriChange { path: Field::Set(home_casing), ..UriChange::default() });
            let path = self.ext_uri().relative_path(&home_with_resource_casing, resource);
            let label = if home_formatter.literal_label { home_formatter.formatting.label.clone() } else { self.format_uri(&home_formatter.home, &home_formatter.formatting, false) };
            return match path.filter(|path| !path.is_empty()) {
                Some(path) => format!("{label}{}{}", separator.as_str(), self.adjust_path_separators(&path, separator)),
                None => label,
            };
        }

        let mut formatting = self.find_formatting(resource);
        if let (Some(formatting), Some(separator)) = (formatting.as_mut(), options.separator) {
            // mixin separator if defined from the outside
            formatting.separator = separator;
        }

        let mut label = self.do_get_uri_label(resource, formatting.as_ref(), options.relative == Some(true), no_prefix);

        // Without formatting we still need to support the separator
        // as provided in options (https://github.com/microsoft/vscode/issues/130019)
        if formatting.is_none()
            && let Some(separator) = options.separator
        {
            label = self.adjust_path_separators(&label, separator);
        }

        if options.append_workspace_suffix == Some(true) && formatting.as_ref().is_some_and(|formatting| non_empty(&formatting.workspace_suffix).is_some()) {
            label = self.append_workspace_suffix(label, resource);
        }

        label
    }

    fn get_uri_basename_label(&self, resource: &URI) -> String {
        let formatting = self.find_formatting(resource);
        let label = self.do_get_uri_label(resource, formatting.as_ref(), false, false);

        let env = host_environment();
        let path_lib = match formatting.as_ref().map(|formatting| formatting.separator) {
            Some(LabelSeparator::Backslash) => PathModule::win32(env.as_ref()),
            Some(LabelSeparator::Slash) => PathModule::posix(env.as_ref()),
            _ if self.os.get() == OperatingSystem::Windows => PathModule::win32(env.as_ref()),
            _ => PathModule::posix(env.as_ref()),
        };

        path_lib.basename(&label, None)
    }

    fn get_workspace_label(&self, workspace: &WorkspaceLabelTarget, options: Option<&WorkspaceLabelOptions>) -> String {
        match workspace {
            WorkspaceLabelTarget::Full(workspace) => match to_workspace_identifier(workspace.as_ref()) {
                AnyWorkspaceIdentifier::SingleFolder(identifier) => self.do_get_single_folder_workspace_label(&identifier.uri, options),
                AnyWorkspaceIdentifier::Workspace(identifier) => self.do_get_workspace_label(&identifier.config_path, options),
                AnyWorkspaceIdentifier::Empty(_) => String::new(),
            },
            // Workspace: Single Folder (as URI)
            WorkspaceLabelTarget::Uri(uri) => self.do_get_single_folder_workspace_label(uri, options),
            // Workspace: Single Folder (as workspace identifier)
            WorkspaceLabelTarget::SingleFolder(identifier) => self.do_get_single_folder_workspace_label(&identifier.uri, options),
            // Workspace: Multi Root
            WorkspaceLabelTarget::Workspace(identifier) => self.do_get_workspace_label(&identifier.config_path, options),
        }
    }

    fn get_host_label(&self, scheme: &str, authority: Option<&str>) -> String {
        let formatter = self.find_formatting(&host_uri(scheme, authority));

        formatter.and_then(|formatter| formatter.workspace_suffix).filter(|suffix| !suffix.is_empty()).or_else(|| authority.filter(|authority| !authority.is_empty()).map(str::to_string)).unwrap_or_default()
    }

    fn get_host_tooltip(&self, scheme: &str, authority: Option<&str>) -> Option<String> {
        self.find_formatting(&host_uri(scheme, authority)).and_then(|formatter| formatter.workspace_tooltip)
    }

    fn get_separator(&self, scheme: &str, authority: Option<&str>) -> LabelSeparator {
        match self.find_formatting(&host_uri(scheme, authority)).map(|formatter| formatter.separator) {
            Some(LabelSeparator::Backslash) => LabelSeparator::Backslash,
            _ => LabelSeparator::Slash,
        }
    }

    fn get_uri_home(&self, resource: &URI) -> Option<URI> {
        self.find_home_formatter(resource).map(|formatter| formatter.home)
    }

    fn register_formatter(&self, formatter: LabelFormatter) -> DisposableRef {
        let (scheme, home_registration) = match &formatter {
            LabelFormatter::Template(template) => {
                let registration = Rc::new(HomeFormatterRegistration { formatter: template.clone(), template: ResourceLabelTemplate::new(&template.home) });
                self.home_formatters.borrow_mut().push(registration.clone());
                (template.home.scheme().to_string_lossy(), Some(registration))
            }
            LabelFormatter::Resource(resource) => {
                self.formatters.borrow_mut().push(resource.clone());
                (resource.scheme.clone(), None)
            }
        };
        let on_did_change_formatters = self.on_did_change_formatters.clone();
        on_did_change_formatters.fire(FormatterChangeEvent { scheme: scheme.clone() });
        let change_listener = home_registration.as_ref().map(|registration| {
            let (emitter, scheme) = (on_did_change_formatters.clone(), scheme.clone());
            registration.formatter.on_did_change_formatting.listen(move |_| emitter.fire(FormatterChangeEvent { scheme: scheme.clone() }))
        });

        let (formatters, home_formatters) = (self.formatters.clone(), self.home_formatters.clone());
        plain_disposable(move || {
            if let Some(change_listener) = &change_listener {
                change_listener.dispose();
            }
            if let LabelFormatter::Resource(resource) = &formatter {
                formatters.borrow_mut().retain(|candidate| !Rc::ptr_eq(candidate, resource));
            }
            if let Some(home_registration) = &home_registration {
                home_formatters.borrow_mut().retain(|candidate| !Rc::ptr_eq(candidate, home_registration));
            }
            on_did_change_formatters.fire(FormatterChangeEvent { scheme });
        })
    }

    fn on_did_change_formatters(&self) -> Event<FormatterChangeEvent> {
        self.on_did_change_formatters.event()
    }

    fn register_cached_formatter(&self, formatter: ResourceLabelFormatter) -> DisposableRef {
        // `this.storedFormatters.formatters ??= []` (um valor que não seja array é substituído)
        let formatters_key = JsString::from("formatters");
        let list = match self.stored_formatters.get(&formatters_key) {
            Some(Dynamic::Array(list)) => list,
            _ => {
                let list = ArrayRef::new(Vec::new());
                self.stored_formatters.set(&formatters_key, Dynamic::Array(list.clone()));
                list
            }
        };

        let scheme = Dynamic::string(&formatter.scheme);
        let authority = formatter.authority.as_deref().map_or(Dynamic::Undefined, Dynamic::string);
        let length = list.len();
        let existing = list.to_vec().iter().position(|item| item.get_str("scheme").strict_equals(&scheme) && item.get_str("authority").strict_equals(&authority));
        let replace = existing
            // at max capacity, replace the last element
            .or((length >= FORMATTER_CACHE_SIZE).then_some(FORMATTER_CACHE_SIZE - 1))
            // `list.unshift(formatter)`: abre uma vaga no fim e desloca tudo
            .unwrap_or_else(|| {
                list.push(Dynamic::Undefined);
                length
            });
        for index in (1..=replace).rev() {
            list.set(index, list.get(index - 1));
        }
        list.set(0, formatter.to_dynamic());

        self.stored_formatters_memento.save_memento();

        self.register_formatter(LabelFormatter::Resource(Rc::new(formatter)))
    }
}
