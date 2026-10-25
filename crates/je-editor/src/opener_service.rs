//! Porte de `src/vs/editor/browser/services/openerService.ts`: o `OpenerService`, que decide como abrir um
//! recurso (endereço web, link de comando, documento no editor) rodando os openers externos contribuídos,
//! os validadores, os openers registrados e, por último, os três padrão (externo, comando, editor).
//!
//! Os tipos do contrato (`IOpener`, `IOpenerService`, `OpenOptions`, `ResolveExternalUriOptions`...) são os
//! de `je_platform::opener`, porte de `platform/opener/common/opener.ts`.
//!
//! Divergências:
//! - **DOM.** O opener externo padrão do original chama `dom.windowOpenNoOpener(href)` (http e https) ou
//!   `mainWindow.location.href = href` (qualquer outro esquema). Não há janela aqui: [`SystemExternalOpener`]
//!   entrega o endereço ao tratador de protocolos do sistema (`xdg-open`, `open`, `url.dll`), que é o que o
//!   navegador faz nos dois ramos, então a distinção entre http/https e os demais esquemas some. Recusa o
//!   `href` que começa com `-` (não é um endereço, e seria lido como opção pelo comando) e, no alvo web
//!   (`wasm32`), onde não há processo para lançar, devolve `false`: o host define o opener com
//!   [`IOpenerService::set_default_external_opener`]. `Schemas`, `matchesScheme` e `matchesSomeScheme` vêm de
//!   `je_base::network`.
//! - **Serviços.** Os decoradores `@ICodeEditorService` e `@ICommandService` viram argumentos de
//!   [`OpenerService::new`]; `_serviceBrand` não existe.
//! - **`ICodeEditorService`.** O serviço real (`codeEditorService.ts`) ainda não foi portado: [`ICodeEditorService`]
//!   tem só os dois métodos que o `EditorOpener` usa (`openCodeEditor` e `getFocusedCodeEditor`). O `ICodeEditor`
//!   é opaco ([`CodeEditorHandle`]): o opener só o repassa. [`TextResourceEditorInput`] e [`TextEditorOptions`]
//!   são as formas mínimas de `ITextResourceEditorInput` e `ITextEditorOptions` (o recurso, as opções do
//!   editor e a seleção), as únicas que este arquivo lê e escreve.
//! - **Estado.** O original guarda tudo na classe e o `_externalResourceOpener` fecha sobre `this`. Em Rust
//!   isso seria um ciclo de `Rc`, então o que o opener externo usa (resolvedores, mapa dos URIs resolvidos
//!   e opener externo padrão) fica em `Core`, que o serviço e o opener externo compartilham.
//! - **Funções `async`.** Cada método `async` do original é um `async fn` do Rust, e o que a interface
//!   expõe volta como `Promise` por [`async_fn`]. Os `return promessa` internos (`_openDefaultExternal`)
//!   viram `.await`, o que muda só o número de rodadas de microtarefa, não a ordem das chamadas.
//! - **Argumentos do comando.** `parse` de `marshalling.ts` devolve [`Marshalled`], e o serviço de comandos
//!   recebe [`Dynamic`], que não tem variante de `URI`: um `URI` revivido chega ao comando na forma do seu
//!   `toJSON` (objeto com `$mid`, `scheme`, `path`...). O id do comando é o `path` convertido sem perda de
//!   texto válido (`to_string_lossy`), porque `ICommandService` recebe `&str`.
//! - **`encodeURI`.** É uma função global do JavaScript, sem equivalente em `je-base`: [`encode_uri`] a
//!   reproduz (e deve migrar para lá quando houver o porte das funções globais).

use std::any::Any;
use std::cell::RefCell;
#[cfg(not(target_arch = "wasm32"))]
use std::process::{Command, Stdio};
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableRef, IDisposable, id_of, plain_disposable};
use je_base::linked_list::{LinkedList, Removal};
use je_base::map::ResourceMap;
use je_base::marshalling::{Marshalled, parse};
use je_base::network::{UriOrString, matches_some_scheme, schemas};
use je_base::path::{Environment, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::{Field, MARSHALLED_ID_URI, URI, UriChange, UriState, decode_uri_component};
use je_platform::commands::ICommandService;
use je_platform::opener::editor_types::{EditorOpenSource, EditorOptions, TextEditorSelection, TextEditorSelectionRevealType, ViewColumn};
use je_platform::opener::{
    AllowCommands, AllowContributedOpeners, DEFAULT_EXTERNAL_URI_OPENER_ID, ExternalOpenerContext, ExtractedSelection, IExternalOpener,
    IExternalUriResolver, IOpener, IOpenerService, IValidator, OpenOptions, OpenResource, ResolveExternalUriOptions, ResolvedExternalUri,
    extract_selection,
};

/// `ICodeEditor`: o editor é opaco para o opener, que só o repassa ao serviço de editores.
pub type CodeEditorHandle = Rc<dyn Any>;

/// `ITextEditorOptions`, só com o que o `EditorOpener` preenche: as opções do editor (`IEditorOptions`) e a
/// seleção.
#[derive(Clone, Default)]
pub struct TextEditorOptions {
    pub editor: EditorOptions,
    pub selection: Option<TextEditorSelection>,
    /// `selectionRevealType`.
    pub selection_reveal_type: Option<TextEditorSelectionRevealType>,
}

/// `ITextResourceEditorInput`, só com o recurso e as opções.
#[derive(Clone)]
pub struct TextResourceEditorInput {
    pub resource: URI,
    pub options: Option<TextEditorOptions>,
}

/// `ICodeEditorService`, só com o que o `EditorOpener` usa.
pub trait ICodeEditorService {
    /// O editor com o foco (no editor ou num widget dele), se houver.
    fn get_focused_code_editor(&self) -> Option<CodeEditorHandle>;

    fn open_code_editor(
        &self,
        input: TextResourceEditorInput,
        source: Option<CodeEditorHandle>,
        side_by_side: Option<bool>,
    ) -> Promise<Option<CodeEditorHandle>>;
}

/// `IExternalUriOpenTarget`.
struct ExternalUriOpenTarget {
    source_uri: URI,
    href: JsString,
    validation_target: OpenResource,
}

/// `typeof target === 'string' ? URI.parse(target) : target`.
fn to_uri(target: &OpenResource) -> URI {
    match target {
        OpenResource::Uri(uri) => uri.clone(),
        OpenResource::Text(text) => URI::parse(text.clone(), false),
    }
}

/// `matchesSomeScheme(target, ...schemes)`.
fn matches_some_scheme_of(target: &OpenResource, schemes: &[&str]) -> bool {
    match target {
        OpenResource::Uri(uri) => matches_some_scheme(UriOrString::Uri(uri), schemes),
        OpenResource::Text(text) => matches_some_scheme(UriOrString::Str(&text.to_string_lossy()), schemes),
    }
}

/// `encodeURI`: mantém letras, dígitos, o conjunto reservado e `#`, e codifica o resto em UTF-8 com
/// percent-encoding maiúsculo. Um surrogate isolado dá `URIError`.
pub fn encode_uri(text: &JsString) -> Result<JsString, Error> {
    const KEEP: &str = ";,/?:@&=+$-_.!~*'()#";
    let mut out: Vec<u16> = Vec::with_capacity(text.len());
    let mut buffer = [0u8; 4];
    for decoded in char::decode_utf16(text.units().iter().copied()) {
        let c = decoded.map_err(|_| Error::named("URIError", "URI malformed"))?;
        if c.is_ascii_alphanumeric() || KEEP.contains(c) {
            out.push(c as u16);
        } else {
            for byte in c.encode_utf8(&mut buffer).bytes() {
                out.extend(format!("%{byte:02X}").encode_utf16());
            }
        }
    }
    Ok(JsString::from_units(out))
}

/// O `URI` revivido na forma do seu `toJSON` (ver "Argumentos do comando" no cabeçalho).
fn uri_state_to_dynamic(state: UriState) -> Dynamic {
    let text = |value: Option<JsString>| value.map(Dynamic::String);
    let components = state.components;
    let fields = [
        ("fsPath", text(state.fs_path)),
        ("_sep", state.sep.map(|sep| Dynamic::Number(f64::from(sep)))),
        ("external", text(state.external)),
        ("path", text(components.path)),
        ("scheme", text(components.scheme)),
        ("authority", text(components.authority)),
        ("query", text(components.query)),
        ("fragment", text(components.fragment)),
    ];
    let mid = Dynamic::from(state.mid.unwrap_or(MARSHALLED_ID_URI));
    Dynamic::object(std::iter::once(("$mid", mid)).chain(fields.into_iter().filter_map(|(name, value)| value.map(|value| (name, value)))))
}

fn marshalled_to_dynamic(value: &Marshalled) -> Dynamic {
    match value {
        Marshalled::Undefined => Dynamic::Undefined,
        Marshalled::Null => Dynamic::Null,
        Marshalled::Bool(value) => Dynamic::Bool(*value),
        Marshalled::Number(value) => Dynamic::Number(*value),
        Marshalled::String(value) => Dynamic::String(value.clone()),
        Marshalled::Array(items) => Dynamic::array(items.iter().map(marshalled_to_dynamic)),
        Marshalled::Object(entries) => Dynamic::object(entries.iter().map(|(key, item)| (key.clone(), marshalled_to_dynamic(item)))),
        Marshalled::Uri(uri) => uri_state_to_dynamic(uri.to_json()),
        Marshalled::RegExp(regexp) => Dynamic::regexp(&regexp.source.to_string_lossy(), &regexp.flags),
        Marshalled::Date(time_value) => Dynamic::date(*time_value),
        Marshalled::Buffer(buffer) => Dynamic::typed_array(buffer.to_vec()),
        Marshalled::Bytes(bytes) => Dynamic::typed_array(bytes.clone()),
    }
}

/// Os argumentos de um link de comando: `parse(decodeURIComponent(query))`, e se falhar `parse(query)`, e se
/// falhar de novo nenhum argumento. Um valor que não é array vira o único argumento.
fn parse_command_args(query: &JsString) -> Vec<Dynamic> {
    let platform = ProcessEnvironment.platform();
    let parse_text = |text: &[u16]| errors::catch(|| parse(text, platform));
    let parsed = decode_uri_component(query.units()).and_then(|decoded| parse_text(&decoded)).or_else(|_| parse_text(query.units()));
    match parsed {
        Ok(Marshalled::Array(items)) => items.iter().map(marshalled_to_dynamic).collect(),
        Ok(other) => vec![marshalled_to_dynamic(&other)],
        Err(_) => Vec::new(),
    }
}

/// `CommandOpener`.
struct CommandOpener {
    command_service: Rc<dyn ICommandService>,
}

impl IOpener for CommandOpener {
    fn open(&self, resource: &OpenResource, options: Option<&OpenOptions>) -> Promise<bool> {
        let command_service = Rc::clone(&self.command_service);
        let (resource, options) = (resource.clone(), options.cloned());
        async_fn(async move {
            if !matches_some_scheme_of(&resource, &[schemas::COMMAND]) {
                return Ok(false);
            }

            let allow_commands = match options.and_then(|options| options.allow_commands) {
                // silently ignore commands when command-links are disabled, also
                // suppress other openers by returning TRUE
                None | Some(AllowCommands::Bool(false)) => return Ok(true),
                Some(allow_commands) => allow_commands,
            };

            let target = to_uri(&resource);

            if let AllowCommands::Only(commands) = &allow_commands
                && !commands.contains(target.path())
            {
                // Only allow specific commands. Suppress other openers by returning TRUE
                return Ok(true);
            }

            // execute as command
            command_service.execute_command(&target.path().to_string_lossy(), parse_command_args(target.query())).await?;
            Ok(true)
        })
    }
}

/// `EditorOpener`.
struct EditorOpener {
    editor_service: Rc<dyn ICodeEditorService>,
    ext_uri: ExtUri,
}

impl IOpener for EditorOpener {
    fn open(&self, resource: &OpenResource, options: Option<&OpenOptions>) -> Promise<bool> {
        let (editor_service, ext_uri) = (Rc::clone(&self.editor_service), self.ext_uri.clone());
        let (resource, options) = (resource.clone(), options.cloned());
        async_fn(async move {
            let ExtractedSelection { selection, uri } = extract_selection(&to_uri(&resource));

            let target = if uri.scheme() == schemas::FILE {
                ext_uri.normalize_path(&uri) // workaround for non-normalized paths (https://github.com/microsoft/vscode/issues/12954)
            } else {
                uri
            };

            // `{ selection, source, ...options?.editorOptions }`: o que `editorOptions` define vence.
            let mut editor = options.as_ref().and_then(|options| options.editor_options.clone()).unwrap_or_default();
            if editor.source.is_none() {
                let from_user_gesture = options.as_ref().is_some_and(|options| options.from_user_gesture == Some(true));
                editor.source = Some(if from_user_gesture { EditorOpenSource::User } else { EditorOpenSource::Api });
            }

            let input = TextResourceEditorInput { resource: target, options: Some(TextEditorOptions { editor, selection, selection_reveal_type: None }) };
            let side_by_side = options.as_ref().and_then(|options| options.open_to_side);
            editor_service.open_code_editor(input, editor_service.get_focused_code_editor(), side_by_side).await?;

            Ok(true)
        })
    }
}

/// O opener externo padrão: entrega o endereço ao tratador de protocolos do sistema (ver "DOM" no cabeçalho).
struct SystemExternalOpener;

impl IExternalOpener for SystemExternalOpener {
    fn open_external(&self, href: &JsString, _ctx: &ExternalOpenerContext, _token: &CancellationToken) -> Promise<bool> {
        Promise::resolved(open_with_system_handler(&href.to_string_lossy()))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn open_with_system_handler(href: &str) -> bool {
    if href.starts_with('-') {
        return false;
    }
    let mut command = if cfg!(windows) {
        let mut command = Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    } else if cfg!(target_os = "macos") {
        Command::new("open")
    } else {
        Command::new("xdg-open")
    };
    command.arg(href).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    match command.spawn() {
        Ok(mut child) => {
            // recolhe o processo para não deixar zumbi
            std::thread::spawn(move || drop(child.wait()));
            true
        }
        Err(_) => false,
    }
}

#[cfg(target_arch = "wasm32")]
fn open_with_system_handler(_href: &str) -> bool {
    false
}

/// `shouldOpenExternal`.
fn should_open_external(target: &OpenResource, options: Option<&OpenOptions>) -> bool {
    options.is_some_and(|options| options.open_external == Some(true))
        || matches_some_scheme_of(target, &[schemas::MAILTO, schemas::HTTP, schemas::HTTPS, schemas::VSLS])
}

/// `shouldUseContributedExternalOpeners`.
fn should_use_contributed_external_openers(target: &OpenResource, options: Option<&OpenOptions>) -> bool {
    let allowed = match options.and_then(|options| options.allow_contributed_openers.as_ref()) {
        Some(AllowContributedOpeners::Bool(allowed)) => *allowed,
        Some(AllowContributedOpeners::Id(id)) => !id.is_empty() && *id != DEFAULT_EXTERNAL_URI_OPENER_ID,
        None => false,
    };
    allowed && should_open_external(target, options)
}

/// O `{ dispose: remove }` que os `register*` devolvem.
fn removal_disposable<E: 'static>(removal: Removal<E>) -> DisposableRef {
    plain_disposable(move || removal.remove())
}

/// O estado que o opener externo padrão e o serviço compartilham.
struct Core {
    resolvers: LinkedList<Rc<dyn IExternalUriResolver>>,
    resolved_uri_targets: RefCell<ResourceMap<URI>>,
    default_external_opener: RefCell<Rc<dyn IExternalOpener>>,
}

impl Core {
    /// `resolveExternalUri`. A rejeição de um resolvedor o descarta e segue para o próximo.
    async fn resolve_external_uri(&self, resource: &URI, options: Option<&OpenOptions>) -> Result<ResolvedExternalUri, Error> {
        for resolver in self.resolvers.iter() {
            if let Ok(Some(result)) = resolver.resolve_external_uri(resource, options).await {
                {
                    let mut targets = self.resolved_uri_targets.borrow_mut();
                    if !targets.has(&result.resolved) {
                        targets.set(result.resolved.clone(), resource.clone());
                    }
                }
                return Ok(result);
            }
        }

        Err(Error::new(format!("Could not resolve external URI: {}", resource.to_string(false))))
    }

    /// `_resolveExternalUriOpenTarget`.
    async fn resolve_external_uri_open_target(&self, resource: &OpenResource, options: Option<&OpenOptions>) -> Result<ExternalUriOpenTarget, Error> {
        //todo@jrieken IExternalUriResolver should support `uri: URI | string`
        let uri = to_uri(resource);

        let external_uri = match self.resolve_external_uri(&uri, options).await {
            Ok(resolved) => resolved.resolved,
            Err(_) => uri.clone(),
        };

        let (href, validation_target) = match resource {
            // open the url-string AS IS
            OpenResource::Text(text) if uri.to_string(false) == external_uri.to_string(false) => (text.clone(), resource.clone()),
            // open URI using the toString(noEncode)+encodeURI-trick
            _ => (encode_uri(&external_uri.to_string(true))?, OpenResource::Uri(external_uri)),
        };

        Ok(ExternalUriOpenTarget { source_uri: uri, href, validation_target })
    }

    /// `_openDefaultExternal`.
    fn open_default_external(&self, target: &ExternalUriOpenTarget) -> Promise<bool> {
        let opener = self.default_external_opener.borrow().clone();
        let ctx = ExternalOpenerContext { source_uri: target.source_uri.clone(), preferred_opener_id: None, view_column: None };
        opener.open_external(&target.href, &ctx, &CancellationToken::None)
    }

    /// `_doOpenExternal`.
    async fn do_open_external(&self, resource: &OpenResource, options: Option<&OpenOptions>) -> Result<bool, Error> {
        let target = self.resolve_external_uri_open_target(resource, options).await?;
        self.open_default_external(&target).await
    }
}

/// O opener padrão para qualquer coisa externa (`mailto`, `http(s)`, `vsls`, ou `openExternal`).
struct ExternalResourceOpener {
    core: Rc<Core>,
}

impl IOpener for ExternalResourceOpener {
    fn open(&self, resource: &OpenResource, options: Option<&OpenOptions>) -> Promise<bool> {
        let core = Rc::clone(&self.core);
        let (resource, options) = (resource.clone(), options.cloned());
        async_fn(async move {
            if should_open_external(&resource, options.as_ref()) {
                // open externally
                core.do_open_external(&resource, options.as_ref()).await?;
                return Ok(true);
            }
            Ok(false)
        })
    }
}

struct Inner {
    core: Rc<Core>,
    openers: LinkedList<Rc<dyn IOpener>>,
    validators: LinkedList<Rc<dyn IValidator>>,
    external_openers: LinkedList<Rc<dyn IExternalOpener>>,
    external_resource_opener: Rc<dyn IOpener>,
}

impl Inner {
    /// `open`.
    async fn open(&self, target: OpenResource, options: Option<OpenOptions>) -> Result<bool, Error> {
        let target_uri = to_uri(&target);

        // Internal schemes are not openable and must instead be handled in event listeners
        if target_uri.scheme() == schemas::INTERNAL {
            return Ok(false);
        }

        let mut external_uri_open_target: Option<ExternalUriOpenTarget> = None;
        if should_use_contributed_external_openers(&target, options.as_ref()) {
            let open_target = self.core.resolve_external_uri_open_target(&target, options.as_ref()).await?;
            if self.open_with_contributed_external_openers(&open_target, options.as_ref()).await? {
                return Ok(true);
            }
            external_uri_open_target = Some(open_target);
        }

        if !options.as_ref().is_some_and(|options| options.skip_validation == Some(true)) {
            let validation_target = external_uri_open_target
                .as_ref()
                .map(|open_target| open_target.validation_target.clone())
                .or_else(|| self.core.resolved_uri_targets.borrow().get(&target_uri).cloned().map(OpenResource::Uri))
                .unwrap_or_else(|| target.clone());
            if !self.validate(&validation_target, options.as_ref()).await? {
                return Ok(false);
            }
        }

        // check with contributed openers
        for opener in self.openers.iter() {
            if let Some(open_target) = &external_uri_open_target
                && id_of(&opener) == id_of(&self.external_resource_opener)
            {
                return self.core.open_default_external(open_target).await;
            }
            let handled = opener.open(&target, options.as_ref()).await?;
            if handled {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// `_openWithContributedExternalOpeners`.
    async fn open_with_contributed_external_openers(&self, target: &ExternalUriOpenTarget, options: Option<&OpenOptions>) -> Result<bool, Error> {
        let preferred_opener_id = match options.and_then(|options| options.allow_contributed_openers.as_ref()) {
            Some(AllowContributedOpeners::Id(id)) => Some(id.clone()),
            _ => None,
        };
        let open_to_side = options.is_some_and(|options| options.open_to_side == Some(true));
        for opener in self.external_openers.iter() {
            let ctx = ExternalOpenerContext {
                source_uri: target.source_uri.clone(),
                preferred_opener_id: preferred_opener_id.clone(),
                view_column: open_to_side.then_some(ViewColumn::Beside),
            };
            if opener.open_external(&target.href, &ctx, &CancellationToken::None).await? {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// `_validate`.
    async fn validate(&self, resource: &OpenResource, options: Option<&OpenOptions>) -> Result<bool, Error> {
        for validator in self.validators.iter() {
            if !validator.should_open(resource, options).await? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// `OpenerService`.
pub struct OpenerService {
    inner: Rc<Inner>,
}

impl OpenerService {
    pub fn new(editor_service: Rc<dyn ICodeEditorService>, command_service: Rc<dyn ICommandService>) -> Self {
        // Default external opener is going through the system protocol handler
        let default_external_opener: Rc<dyn IExternalOpener> = Rc::new(SystemExternalOpener);
        let core = Rc::new(Core {
            resolvers: LinkedList::new(),
            resolved_uri_targets: RefCell::new(ResourceMap::with_key_fn(Rc::new(|uri: &URI| {
                uri.with(&UriChange { path: Field::Clear, fragment: Field::Clear, query: Field::Clear, ..UriChange::default() }).to_string(false)
            }))),
            default_external_opener: RefCell::new(default_external_opener),
        });

        // Default opener: any external, maito, http(s), command, and catch-all-editors
        let external_resource_opener: Rc<dyn IOpener> = Rc::new(ExternalResourceOpener { core: Rc::clone(&core) });
        let openers: LinkedList<Rc<dyn IOpener>> = LinkedList::new();
        openers.push(Rc::clone(&external_resource_opener));
        openers.push(Rc::new(CommandOpener { command_service }));
        openers.push(Rc::new(EditorOpener { editor_service, ext_uri: ExtUri::unbiased(Rc::new(ProcessEnvironment)) }));

        Self {
            inner: Rc::new(Inner {
                core,
                openers,
                validators: LinkedList::new(),
                external_openers: LinkedList::new(),
                external_resource_opener,
            }),
        }
    }
}

impl IOpenerService for OpenerService {
    fn register_opener(&self, opener: Rc<dyn IOpener>) -> DisposableRef {
        removal_disposable(self.inner.openers.unshift(opener))
    }

    fn register_validator(&self, validator: Rc<dyn IValidator>) -> DisposableRef {
        removal_disposable(self.inner.validators.push(validator))
    }

    fn register_external_uri_resolver(&self, resolver: Rc<dyn IExternalUriResolver>) -> DisposableRef {
        removal_disposable(self.inner.core.resolvers.push(resolver))
    }

    fn set_default_external_opener(&self, opener: Rc<dyn IExternalOpener>) {
        *self.inner.core.default_external_opener.borrow_mut() = opener;
    }

    fn register_external_opener(&self, opener: Rc<dyn IExternalOpener>) -> DisposableRef {
        removal_disposable(self.inner.external_openers.push(opener))
    }

    fn open(&self, resource: &OpenResource, options: Option<&OpenOptions>) -> Promise<bool> {
        let inner = Rc::clone(&self.inner);
        let (resource, options) = (resource.clone(), options.cloned());
        async_fn(async move { inner.open(resource, options).await })
    }

    fn resolve_external_uri(&self, resource: &URI, options: Option<&ResolveExternalUriOptions>) -> Promise<ResolvedExternalUri> {
        let core = Rc::clone(&self.inner.core);
        let resource = resource.clone();
        // `ResolveExternalUriOptions` é o que sobra de `OpenOptions` quando só `allowTunneling` é passado
        let options = options.map(|options| OpenOptions { allow_tunneling: options.allow_tunneling, ..OpenOptions::default() });
        async_fn(async move { core.resolve_external_uri(&resource, options.as_ref()).await })
    }
}

impl IDisposable for OpenerService {
    fn dispose(&self) {
        self.inner.validators.clear();
    }
}
