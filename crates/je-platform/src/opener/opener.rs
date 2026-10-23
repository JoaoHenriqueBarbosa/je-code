//! Porte de `src/vs/platform/opener/common/opener.ts`: `IOpenerService`, os tipos de abertura e as
//! funções que codificam e extraem a seleção do fragmento do `URI` (`withSelection`, `extractSelection`).
//!
//! Divergências de representação:
//! - Os serviços são `trait`s, e `_serviceBrand` não existe (o identificador é [`I_OPENER_SERVICE`]).
//! - `URI | string` é [`OpenResource`]. `OpenInternalOptions | OpenExternalOptions` é [`OpenOptions`]: como
//!   todos os campos são opcionais, a união e a interseção do original têm a mesma forma, e as duas
//!   metades convertem para ela por `From`.
//! - `boolean | readonly string[]` e `boolean | string` são [`AllowCommands`] e [`AllowContributedOpeners`].
//! - `IResolvedExternalUri` (e o `{ resolved, dispose() }` do resolvedor) é [`ResolvedExternalUri`]:
//!   `resolved` é um campo público, e `dispose` descarta o recurso que o resolvedor guardou.

use std::rc::Rc;

use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_instantiation::service_id;

use super::editor_types::{EditorOptions, TextEditorSelection, ViewColumn};

service_id!(pub I_OPENER_SERVICE: dyn IOpenerService = "openerService");

pub const DEFAULT_EXTERNAL_URI_OPENER_ID: &str = "default";

/// `boolean | readonly string[]` (`OpenInternalOptions.allowCommands`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AllowCommands {
    Bool(bool),
    /// Só os comandos listados podem ser executados.
    Only(Vec<JsString>),
}

/// `OpenInternalOptions`.
#[derive(Clone, Default)]
pub struct OpenInternalOptions {
    /// Sinaliza a intenção de abrir um editor ao lado do editor ativo.
    pub open_to_side: Option<bool>,
    /// Opções extras de editor, caso um editor seja usado para abrir.
    pub editor_options: Option<EditorOptions>,
    /// Sinaliza que o editor a abrir foi disparado por uma ação do usuário, como teclado ou mouse.
    pub from_user_gesture: Option<bool>,
    /// Permite que links de comando sejam tratados. Se for uma lista, só os comandos dela podem rodar.
    pub allow_commands: Option<AllowCommands>,
}

/// `boolean | string` (`OpenExternalOptions.allowContributedOpeners`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AllowContributedOpeners {
    Bool(bool),
    Id(JsString),
}

/// `OpenExternalOptions`.
#[derive(Clone, Default)]
pub struct OpenExternalOptions {
    pub open_external: Option<bool>,
    pub allow_tunneling: Option<bool>,
    /// Permite openers externos contribuídos. Eles são tentados antes dos validadores, que só validam o
    /// URI resolvido quando se cai no opener externo padrão.
    pub allow_contributed_openers: Option<AllowContributedOpeners>,
    pub from_workspace: Option<bool>,
    pub skip_validation: Option<bool>,
}

/// `OpenOptions` (`OpenInternalOptions & OpenExternalOptions`).
#[derive(Clone, Default)]
pub struct OpenOptions {
    pub open_to_side: Option<bool>,
    pub editor_options: Option<EditorOptions>,
    pub from_user_gesture: Option<bool>,
    pub allow_commands: Option<AllowCommands>,
    pub open_external: Option<bool>,
    pub allow_tunneling: Option<bool>,
    pub allow_contributed_openers: Option<AllowContributedOpeners>,
    pub from_workspace: Option<bool>,
    pub skip_validation: Option<bool>,
}

impl From<OpenInternalOptions> for OpenOptions {
    fn from(options: OpenInternalOptions) -> Self {
        Self {
            open_to_side: options.open_to_side,
            editor_options: options.editor_options,
            from_user_gesture: options.from_user_gesture,
            allow_commands: options.allow_commands,
            ..Self::default()
        }
    }
}

impl From<OpenExternalOptions> for OpenOptions {
    fn from(options: OpenExternalOptions) -> Self {
        Self {
            open_external: options.open_external,
            allow_tunneling: options.allow_tunneling,
            allow_contributed_openers: options.allow_contributed_openers,
            from_workspace: options.from_workspace,
            skip_validation: options.skip_validation,
            ..Self::default()
        }
    }
}

/// `URI | string`.
#[derive(Clone)]
pub enum OpenResource {
    Uri(URI),
    Text(JsString),
}

impl From<URI> for OpenResource {
    fn from(uri: URI) -> Self {
        Self::Uri(uri)
    }
}

impl From<JsString> for OpenResource {
    fn from(text: JsString) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for OpenResource {
    fn from(text: &str) -> Self {
        Self::Text(JsString::from(text))
    }
}

/// `ResolveExternalUriOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResolveExternalUriOptions {
    pub allow_tunneling: Option<bool>,
}

/// `IResolvedExternalUri`.
#[derive(Clone)]
pub struct ResolvedExternalUri {
    pub resolved: URI,
    /// O que `dispose()` descarta (um túnel, por exemplo). `None` quando não há o que descartar.
    pub disposable: Option<DisposableRef>,
}

impl IDisposable for ResolvedExternalUri {
    fn dispose(&self) {
        if let Some(disposable) = &self.disposable {
            disposable.dispose();
        }
    }
}

/// `IOpener`.
pub trait IOpener {
    fn open(&self, resource: &OpenResource, options: Option<&OpenOptions>) -> Promise<bool>;
}

/// O `ctx` de `IExternalOpener.openExternal`.
#[derive(Clone)]
pub struct ExternalOpenerContext {
    pub source_uri: URI,
    pub preferred_opener_id: Option<JsString>,
    pub view_column: Option<ViewColumn>,
}

/// `IExternalOpener`.
pub trait IExternalOpener {
    fn open_external(&self, href: &JsString, ctx: &ExternalOpenerContext, token: &CancellationToken) -> Promise<bool>;

    /// O `dispose?()` opcional do original.
    fn dispose(&self) {}
}

/// `IValidator`.
pub trait IValidator {
    fn should_open(&self, resource: &OpenResource, open_options: Option<&OpenOptions>) -> Promise<bool>;
}

/// `IExternalUriResolver`.
pub trait IExternalUriResolver {
    fn resolve_external_uri(&self, resource: &URI, options: Option<&OpenOptions>) -> Promise<Option<ResolvedExternalUri>>;
}

/// `IOpenerService`.
pub trait IOpenerService {
    /// Registra um participante que pode tratar a chamada `open()`.
    fn register_opener(&self, opener: Rc<dyn IOpener>) -> DisposableRef;

    /// Registra um participante que valida se o recurso pode ser aberto. Os validadores rodam antes dos
    /// openers, a menos que os openers externos contribuídos estejam habilitados.
    fn register_validator(&self, validator: Rc<dyn IValidator>) -> DisposableRef;

    /// Registra um participante que resolve um recurso externo a ser aberto.
    fn register_external_uri_resolver(&self, resolver: Rc<dyn IExternalUriResolver>) -> DisposableRef;

    /// Define o tratador de abertura externa. Se não for informado, um tratador padrão é usado.
    fn set_default_external_opener(&self, opener: Rc<dyn IExternalOpener>);

    /// Registra um novo opener de recursos externos.
    fn register_external_opener(&self, opener: Rc<dyn IExternalOpener>) -> DisposableRef;

    /// Abre um recurso, como um endereço web, um URI de documento ou a execução de um comando.
    fn open(&self, resource: &OpenResource, options: Option<&OpenOptions>) -> Promise<bool>;

    /// Resolve um recurso para a sua forma externa. A promessa é rejeitada quando nenhum resolvedor
    /// consegue resolvê-lo externamente.
    fn resolve_external_uri(
        &self,
        resource: &URI,
        options: Option<&ResolveExternalUriOptions>,
    ) -> Promise<ResolvedExternalUri>;
}

/// O `${number}` do JavaScript para um número finito de uso comum.
fn js_number(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string()
    } else {
        value.to_string()
    }
}

/// O "truthy" de um `number | undefined`: presente, diferente de zero e de `NaN`.
fn is_truthy_number(value: Option<f64>) -> Option<f64> {
    value.filter(|n| *n != 0.0 && !n.is_nan())
}

/// Codifica a seleção no `URI`.
///
/// IMPORTANTE: é preciso usar [`extract_selection`] para separar a seleção do `URI` original antes de
/// passá-lo a qualquer componente que não conheça seleções.
pub fn with_selection(uri: &URI, selection: &TextEditorSelection) -> URI {
    let mut fragment = format!("{},{}", js_number(selection.start_line_number), js_number(selection.start_column));
    if let Some(end_line_number) = is_truthy_number(selection.end_line_number) {
        fragment.push('-');
        fragment.push_str(&js_number(end_line_number));
        if let Some(end_column) = is_truthy_number(selection.end_column) {
            fragment.push(',');
            fragment.push_str(&js_number(end_column));
        }
    }
    uri.with(&UriChange { fragment: Field::Set(JsString::from(fragment)), ..UriChange::default() })
}

/// O resultado de [`extract_selection`]: `{ selection, uri }`.
#[derive(Clone)]
pub struct ExtractedSelection {
    pub selection: Option<TextEditorSelection>,
    pub uri: URI,
}

const UNIT_UPPER_L: u16 = b'L' as u16;
const UNIT_COMMA: u16 = b',' as u16;
const UNIT_DASH: u16 = b'-' as u16;

/// O fim da sequência de dígitos ASCII (`\d+`) que começa em `start`, ou `None` se não há dígito.
fn digits_end(units: &[u16], start: usize) -> Option<usize> {
    let length = units[start.min(units.len())..].iter().take_while(|u| (b'0' as u16..=b'9' as u16).contains(u)).count();
    (length > 0).then_some(start + length)
}

/// `parseInt` de uma sequência de dígitos ASCII.
fn parse_digits(units: &[u16]) -> f64 {
    units.iter().fold(0.0, |acc, u| acc * 10.0 + f64::from(*u - b'0' as u16))
}

/// `(?:,(\d+))?` em `at`: o número e o fim do trecho consumido, ou `None` se a vírgula e os dígitos não
/// aparecem.
fn optional_column(units: &[u16], at: usize) -> Option<(f64, usize)> {
    if units.get(at) != Some(&UNIT_COMMA) {
        return None;
    }
    let end = digits_end(units, at + 1)?;
    Some((parse_digits(&units[at + 1..end]), end))
}

/// `L?(\d+)(?:,(\d+))?` em `at`: a linha, a coluna opcional e o fim do trecho consumido.
fn line_and_column(units: &[u16], at: usize) -> Option<(f64, Option<f64>, usize)> {
    let digits_start = if units.get(at) == Some(&UNIT_UPPER_L) { at + 1 } else { at };
    let digits_stop = digits_end(units, digits_start)?;
    let line = parse_digits(&units[digits_start..digits_stop]);
    match optional_column(units, digits_stop) {
        Some((column, end)) => Some((line, Some(column), end)),
        None => Some((line, None, digits_stop)),
    }
}

/// Extrai a seleção do fragmento do `URI`, que o devolve sem o fragmento quando há seleção:
///
/// ```text
/// file:///some/file.js#73
/// file:///some/file.js#L73
/// file:///some/file.js#73,84
/// file:///some/file.js#L73,84
/// file:///some/file.js#73-83
/// file:///some/file.js#L73-L83
/// file:///some/file.js#73,84-83,52
/// file:///some/file.js#L73,84-L83,52
/// ```
///
/// O original usa `/^L?(\d+)(?:,(\d+))?(-L?(\d+)(?:,(\d+))?)?/`: o casamento é manual, grupo a grupo.
pub fn extract_selection(uri: &URI) -> ExtractedSelection {
    let fragment = uri.fragment();
    let units = fragment.units();
    let Some((start_line, start_column, after_start)) = line_and_column(units, 0) else {
        return ExtractedSelection { selection: None, uri: uri.clone() };
    };

    // `(-L?(\d+)(?:,(\d+))?)?`
    let end = if units.get(after_start) == Some(&UNIT_DASH) { line_and_column(units, after_start + 1) } else { None };

    let selection = TextEditorSelection {
        start_line_number: start_line,
        start_column: start_column.unwrap_or(1.0),
        end_line_number: end.map(|(line, _, _)| line),
        end_column: end.map(|(_, column, _)| column.unwrap_or(1.0)),
    };
    let uri = uri.with(&UriChange { fragment: Field::Set(JsString::new()), ..UriChange::default() });
    ExtractedSelection { selection: Some(selection), uri }
}
