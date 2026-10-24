//! Porte de `src/vs/platform/label/common/label.ts`: o contrato `ILabelService` (rótulos legíveis de URIs, de
//! workspaces e de hosts), a `Verbosity` e os formatadores de rótulo de recurso.
//!
//! Notas de tradução:
//! - **Uniões.** `IWorkspaceIdentifier | ISingleFolderWorkspaceIdentifier | URI | IWorkspace` de `getWorkspaceLabel`
//!   é o enum [`WorkspaceLabelTarget`]; `ResourceLabelFormatter | ResourceLabelTemplateFormatter` de
//!   `registerFormatter` é o enum [`LabelFormatter`] (o `isTemplateFormatter` do serviço, que olha se `home` é um
//!   `URI`, é o discriminante do enum).
//! - **Identidade.** O serviço remove formatadores por identidade (`f !== formatter`): por isso o
//!   [`LabelFormatter`] carrega `Rc`, e dois formatadores iguais registrados em separado continuam distintos.
//! - **Separador.** `'/' | '\\' | ''` é [`LabelSeparator`]. O que vem de fora do tipo (JSON de extensão ou do cache
//!   em disco) e não é um dos três cai no separador nativo, como o valor ausente.
//! - **Opções posicionais** (`getUriLabel(resource, options?)`) são as structs [`UriLabelOptions`] e
//!   [`WorkspaceLabelOptions`]; os campos opcionais são `Option`.
//! - **Sobrecargas de `userHome`** não existem aqui (ver `path_service`).
//! - **Dados dinâmicos.** O cache de formatadores e as contribuições de extensão passam por `Dynamic`
//!   ([`ResourceLabelFormatter::to_dynamic`] e [`ResourceLabelFormatter::from_dynamic`]), que leem só as
//!   propriedades com o tipo certo, porque o original avisa que o que vem de extensão não é confiável.

use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::uri::URI;
use je_instantiation::{delayed_proxy, service_id};

use crate::workspace::{IWorkspace, SingleFolderWorkspaceIdentifier, WorkspaceIdentifier};

/// `Verbosity`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verbosity {
    Short,
    Medium,
    Long,
}

/// `'/' | '\\' | ''`: o separador de um rótulo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LabelSeparator {
    #[default]
    Slash,
    Backslash,
    Empty,
}

impl LabelSeparator {
    /// O texto do separador.
    pub fn as_str(self) -> &'static str {
        match self {
            LabelSeparator::Slash => "/",
            LabelSeparator::Backslash => "\\",
            LabelSeparator::Empty => "",
        }
    }

    /// O separador que o texto representa, se for um dos três.
    pub fn from_text(text: &str) -> Option<Self> {
        match text {
            "/" => Some(LabelSeparator::Slash),
            "\\" => Some(LabelSeparator::Backslash),
            "" => Some(LabelSeparator::Empty),
            _ => None,
        }
    }
}

/// `IFormatterChangeEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatterChangeEvent {
    pub scheme: String,
}

/// `ResourceLabelFormatting`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResourceLabelFormatting {
    /// `label`: por exemplo `myLabel:/${path}`.
    pub label: String,
    pub separator: LabelSeparator,
    pub tildify: Option<bool>,
    pub normalize_drive_letter: Option<bool>,
    pub workspace_suffix: Option<String>,
    pub workspace_tooltip: Option<String>,
    pub authority_prefix: Option<String>,
    pub strip_path_starting_separator: Option<bool>,
    /// Number of leading path segments to strip from `${path}` before substitution. For example, a value of `2`
    /// turns `/scheme/authority/rest/of/path` into `/rest/of/path`.
    pub strip_path_segments: Option<usize>,
}

/// `ResourceLabelFormatter`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResourceLabelFormatter {
    pub scheme: String,
    pub authority: Option<String>,
    /// URI path used as a display home. Runtime registrations only.
    pub home: Option<String>,
    pub priority: Option<bool>,
    pub formatting: ResourceLabelFormatting,
}

/// `ResourceLabelFormattingContext`: o que o formatador de `home` recebe.
#[derive(Clone, Debug)]
pub struct ResourceLabelFormattingContext {
    pub resource: URI,
    pub home: URI,
    pub parameters: HashMap<String, String>,
}

/// O tipo de `formatting` de [`ResourceLabelTemplateFormatter`]: resolve o formato (cujo rótulo é texto literal).
pub type TemplateFormattingFn = Rc<dyn Fn(&ResourceLabelFormattingContext) -> Option<ResourceLabelFormatting>>;

/// `ResourceLabelTemplateFormatter`.
#[derive(Clone)]
pub struct ResourceLabelTemplateFormatter {
    /// URI home whose optional template parameters occupy an entire path segment, such as `/sessions/${sessionId}`.
    pub home: URI,
    /// Fires when state used by `formatting` changes and existing labels must be recomputed.
    pub on_did_change_formatting: Event<()>,
    /// Resolves formatting whose label is treated as literal text.
    pub formatting: TemplateFormattingFn,
}

/// `ResourceLabelFormatter | ResourceLabelTemplateFormatter`.
#[derive(Clone)]
pub enum LabelFormatter {
    Resource(Rc<ResourceLabelFormatter>),
    Template(Rc<ResourceLabelTemplateFormatter>),
}

impl From<ResourceLabelFormatter> for LabelFormatter {
    fn from(formatter: ResourceLabelFormatter) -> Self {
        LabelFormatter::Resource(Rc::new(formatter))
    }
}

impl From<ResourceLabelTemplateFormatter> for LabelFormatter {
    fn from(formatter: ResourceLabelTemplateFormatter) -> Self {
        LabelFormatter::Template(Rc::new(formatter))
    }
}

/// `{ relative?, noPrefix?, separator?, appendWorkspaceSuffix? }` de `getUriLabel`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UriLabelOptions {
    pub relative: Option<bool>,
    pub no_prefix: Option<bool>,
    pub separator: Option<LabelSeparator>,
    pub append_workspace_suffix: Option<bool>,
}

/// `{ verbose: Verbosity }` de `getWorkspaceLabel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkspaceLabelOptions {
    pub verbose: Verbosity,
}

/// `IWorkspaceIdentifier | ISingleFolderWorkspaceIdentifier | URI | IWorkspace`.
#[derive(Clone)]
pub enum WorkspaceLabelTarget {
    Workspace(WorkspaceIdentifier),
    SingleFolder(SingleFolderWorkspaceIdentifier),
    Uri(URI),
    Full(Rc<dyn IWorkspace>),
}

/// `ILabelService`.
pub trait ILabelService {
    /// Gets the human readable label for a uri.
    /// If `relative` is passed returns a label relative to the workspace root that the uri belongs to.
    /// If `noPrefix` is passed does not tildify the label and also does not prepand the root name for relative labels in a multi root scenario.
    /// If `separator` is passed, will use that over the defined path separator of the formatter.
    /// If `appendWorkspaceSuffix` is passed, will append the name of the workspace to the label.
    fn get_uri_label(&self, resource: &URI, options: &UriLabelOptions) -> String;
    fn get_uri_basename_label(&self, resource: &URI) -> String;
    fn get_workspace_label(&self, workspace: &WorkspaceLabelTarget, options: Option<&WorkspaceLabelOptions>) -> String;
    fn get_host_label(&self, scheme: &str, authority: Option<&str>) -> String;
    fn get_host_tooltip(&self, scheme: &str, authority: Option<&str>) -> Option<String>;
    fn get_separator(&self, scheme: &str, authority: Option<&str>) -> LabelSeparator;
    /// Returns the display home containing `resource`.
    fn get_uri_home(&self, resource: &URI) -> Option<URI>;

    fn register_formatter(&self, formatter: LabelFormatter) -> DisposableRef;
    fn on_did_change_formatters(&self) -> Event<FormatterChangeEvent>;

    /// Registers a formatter that's cached for the machine beyond the lifecycle of the current window. Disposing
    /// the formatter _will not_ remove it from the cache.
    fn register_cached_formatter(&self, formatter: ResourceLabelFormatter) -> DisposableRef;
}

delayed_proxy!(pub LabelServiceProxy for dyn ILabelService {
    methods {
        fn get_uri_label(&self, resource: &URI, options: &UriLabelOptions) -> String;
        fn get_uri_basename_label(&self, resource: &URI) -> String;
        fn get_workspace_label(&self, workspace: &WorkspaceLabelTarget, options: Option<&WorkspaceLabelOptions>) -> String;
        fn get_host_label(&self, scheme: &str, authority: Option<&str>) -> String;
        fn get_host_tooltip(&self, scheme: &str, authority: Option<&str>) -> Option<String>;
        fn get_separator(&self, scheme: &str, authority: Option<&str>) -> LabelSeparator;
        fn get_uri_home(&self, resource: &URI) -> Option<URI>;
        fn register_formatter(&self, formatter: LabelFormatter) -> DisposableRef;
        fn register_cached_formatter(&self, formatter: ResourceLabelFormatter) -> DisposableRef;
    }
    events {
        fn on_did_change_formatters(&self) -> FormatterChangeEvent;
    }
});

service_id!(pub I_LABEL_SERVICE: dyn ILabelService = "labelService", proxy LabelServiceProxy);

// ---------------------------------------------------------------------------------------------
// Conversão de e para os dados dinâmicos (o cache em disco e as contribuições das extensões).
// ---------------------------------------------------------------------------------------------

fn string_property(value: &Dynamic, key: &str) -> Option<String> {
    match value.get_str(key) {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

fn bool_property(value: &Dynamic, key: &str) -> Option<bool> {
    match value.get_str(key) {
        Dynamic::Bool(flag) => Some(flag),
        _ => None,
    }
}

impl ResourceLabelFormatting {
    /// O objeto que o original guarda: só as propriedades definidas.
    pub fn to_dynamic(&self) -> Dynamic {
        let mut entries: Vec<(&str, Dynamic)> = vec![("label", Dynamic::string(&self.label)), ("separator", Dynamic::string(self.separator.as_str()))];
        let flags = [("tildify", self.tildify), ("normalizeDriveLetter", self.normalize_drive_letter), ("stripPathStartingSeparator", self.strip_path_starting_separator)];
        entries.extend(flags.into_iter().filter_map(|(key, flag)| flag.map(|flag| (key, Dynamic::Bool(flag)))));
        let texts = [
            ("workspaceSuffix", &self.workspace_suffix),
            ("workspaceTooltip", &self.workspace_tooltip),
            ("authorityPrefix", &self.authority_prefix),
        ];
        entries.extend(texts.into_iter().filter_map(|(key, text)| text.as_ref().map(|text| (key, Dynamic::string(text)))));
        if let Some(segments) = self.strip_path_segments {
            entries.push(("stripPathSegments", Dynamic::Number(segments as f64)));
        }
        Dynamic::object(entries)
    }

    /// O formato lido de um objeto não confiável. `default_separator` vale quando `separator` não é um dos três
    /// textos válidos; `label` ausente ou que não é texto fica vazio (quem precisa de outro padrão decide antes).
    pub fn from_dynamic(value: &Dynamic, default_separator: LabelSeparator) -> Self {
        Self {
            label: string_property(value, "label").unwrap_or_default(),
            separator: string_property(value, "separator").and_then(|text| LabelSeparator::from_text(&text)).unwrap_or(default_separator),
            tildify: bool_property(value, "tildify"),
            normalize_drive_letter: bool_property(value, "normalizeDriveLetter"),
            workspace_suffix: string_property(value, "workspaceSuffix"),
            workspace_tooltip: string_property(value, "workspaceTooltip"),
            authority_prefix: string_property(value, "authorityPrefix"),
            strip_path_starting_separator: bool_property(value, "stripPathStartingSeparator"),
            strip_path_segments: match value.get_str("stripPathSegments") {
                Dynamic::Number(count) => Some(count.max(0.0) as usize),
                _ => None,
            },
        }
    }
}

impl ResourceLabelFormatter {
    /// O objeto que o original guarda no cache: só as propriedades definidas.
    pub fn to_dynamic(&self) -> Dynamic {
        let mut entries: Vec<(&str, Dynamic)> = vec![("scheme", Dynamic::string(&self.scheme))];
        if let Some(authority) = &self.authority {
            entries.push(("authority", Dynamic::string(authority)));
        }
        if let Some(home) = &self.home {
            entries.push(("home", Dynamic::string(home)));
        }
        if let Some(priority) = self.priority {
            entries.push(("priority", Dynamic::Bool(priority)));
        }
        entries.push(("formatting", self.formatting.to_dynamic()));
        Dynamic::object(entries)
    }

    /// O formatador lido de um objeto não confiável (o cache em disco): `None` se o valor não for um objeto com
    /// `formatting` objeto.
    pub fn from_dynamic(value: &Dynamic, default_separator: LabelSeparator) -> Option<Self> {
        let formatting = value.get_str("formatting");
        if !matches!(value, Dynamic::Object(_)) || !matches!(formatting, Dynamic::Object(_)) {
            return None;
        }
        Some(Self {
            scheme: string_property(value, "scheme").unwrap_or_default(),
            authority: string_property(value, "authority"),
            home: string_property(value, "home"),
            priority: bool_property(value, "priority"),
            formatting: ResourceLabelFormatting::from_dynamic(&formatting, default_separator),
        })
    }
}
