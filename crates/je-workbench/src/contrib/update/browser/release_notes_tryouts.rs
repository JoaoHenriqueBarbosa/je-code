//! Porte de `src/vs/workbench/contrib/update/browser/releaseNotesTryouts.ts`: os links "Try This" de um documento de
//! notas de versão, resolvidos localmente (o estado de cada ocorrência, a validação do protocolo com o webview, a
//! ativação com cancelamento e a devolução do foco).
//!
//! Recorte declarado (a UI é nativa, sem DOM nem webview HTML no caminho de pintura):
//! - O documento é a árvore [`Element`]/[`Node`] do `html_parser`: [`ReleaseNotesTryouts::render`] percorre o contêiner e troca
//!   cada `a[href]` como o `render(container)` do original (manter, virar texto inerte, ou virar o controle
//!   `span.release-notes-tryout` com o índice e o estado), com a decisão por link em [`ReleaseNotesTryouts::render_link`], e
//!   [`apply_release_notes_tryout_state`] é a função do mesmo nome. O foco (`document.activeElement`, `focus()`) não existe na
//!   árvore: a parte de `applyReleaseNotesTryoutState` que devolve o foco, e a mensagem `releaseNotesTryoutFocus` do
//!   documento, não têm o que fazer.
//! - `initializeReleaseNotesTryouts` e `getScript()` (o `toString()` das funções, que roda como JavaScript dentro do webview)
//!   são o [`ReleaseNotesTryoutsClient`]: o lado do documento que posta `releaseNotesTryoutsReady`, converte o clique num
//!   controle em `releaseNotesTryout` (com `run` ou `setup`) e aplica as mensagens `releaseNotesTryouts`. O clique entra
//!   pelo caminho do alvo na árvore (os índices em `children`), como o widget nativo entrega.
//! - A limpeza dos atributos `data-release-notes-tryout-*` forjados é a primeira passada de `render`.
//! - O `stub(tryouts, 'render').callThrough()` do teste é [`ReleaseNotesTryouts::render_count`].
//!
//! Divergências:
//! - O `IWebview` do porte não tem `focus()`, que o original chama. O recorte do que a classe usa do webview é o trait
//!   [`ReleaseNotesTryoutsWebview`] (mensagens recebidas, descarte, `postMessage` e `focus`).
//! - `extends Disposable` é o [`DisposableStore`] guardado em `store`, que também responde por `this._store.isDisposed`.
//! - `interact` é `async`: o corpo roda de forma síncrona até o primeiro `await`, como no original, e o
//!   `try/catch/finally` é a sequência explícita de [`ReleaseNotesTryouts::interact`]. O `this._interaction.value ===
//!   interaction` é uma sequência numérica da interação corrente. Uma exceção síncrona do serviço (`run`,
//!   `getAvailability`) vira erro por [`errors::catch`].
//! - `needsRender` troca as duas expressões regulares (`\bdata-release-notes-tryout-(?:id|index)\s*=` e
//!   `\bhref="([^"]*)"`, sem diferenciar caixa) por busca manual com a mesma semântica.
//! - A mensagem do webview é um [`Dynamic`]; o `Partial<ReleaseNotesTryoutMessage>` vira a leitura campo a campo, com o
//!   mesmo rigor (tipo exato do campo, índice inteiro de um link existente, e nenhuma chave além das cinco esperadas).

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::browser::html_parser::{Attribute, Element, Node};
use je_base::cancellation::CancellationTokenSource;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error, is_cancellation_error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable, MutableDisposable, to_disposable};
use je_base::markdown::dom::{remove_attribute, set_attribute};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::strings::escape;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_base::JsString;
use je_platform::commands::ICommandService;
use je_platform::log::{LogArg, LogMessage, LogServiceRef};
use je_platform::notification::{INotificationService, NotificationMessage};

use crate::contrib::onboarding::onboarding_tryout::{
    OnboardingTryoutAvailability, OnboardingTryoutResult, RUN_ONBOARDING_TRYOUT_COMMAND_ID, OnboardingTryoutTargetWindow,
    is_onboarding_tryout_id,
};
use crate::contrib::onboarding::onboarding_tryout_service::{
    IOnboardingTryoutService, OnboardingTryoutRunOptions, OnboardingTryoutSource, create_onboarding_tryout_uri,
    parse_onboarding_tryout_uri,
};
use crate::contrib::update::update_info_parser::is_js_whitespace;
use crate::contrib::webview::browser_webview::WebviewMessageReceivedEvent;

/// O que `ReleaseNotesTryouts` usa de um `IWebview` (ver a nota do módulo).
pub trait ReleaseNotesTryoutsWebview {
    /// `onDidDispose`.
    fn on_did_dispose(&self) -> Event<()>;
    /// `onMessage`.
    fn on_message(&self) -> Event<WebviewMessageReceivedEvent>;
    /// `postMessage(message)`.
    fn post_message(&self, message: Dynamic) -> Promise<bool>;
    /// `focus()`.
    fn focus(&self);
}

/// O `kind` de `IReleaseNotesTryoutState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseNotesTryoutKind {
    Ready,
    Hidden,
    Unavailable,
}

impl ReleaseNotesTryoutKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Hidden => "hidden",
            Self::Unavailable => "unavailable",
        }
    }
}

/// `IReleaseNotesTryoutState`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseNotesTryoutState {
    pub id: String,
    pub index: usize,
    pub kind: ReleaseNotesTryoutKind,
    pub label: String,
    pub aria_label: String,
    pub href: String,
    pub message: String,
    pub setup_label: String,
    pub setup_aria_label: String,
}

impl ReleaseNotesTryoutState {
    /// O objeto que vai no `postMessage`.
    fn to_dynamic(&self) -> Dynamic {
        Dynamic::object([
            ("id", Dynamic::string(&self.id)),
            ("index", Dynamic::Number(self.index as f64)),
            ("kind", Dynamic::string(self.kind.as_str())),
            ("label", Dynamic::string(&self.label)),
            ("ariaLabel", Dynamic::string(&self.aria_label)),
            ("href", Dynamic::string(&self.href)),
            ("message", Dynamic::string(&self.message)),
            ("setupLabel", Dynamic::string(&self.setup_label)),
            ("setupAriaLabel", Dynamic::string(&self.setup_aria_label)),
        ])
    }
}

/// O `action: 'run' | 'setup'` do pedido do webview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseNotesTryoutAction {
    Run,
    Setup,
}

impl ReleaseNotesTryoutAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Setup => "setup",
        }
    }

    fn parse(value: &Dynamic) -> Option<Self> {
        match value {
            Dynamic::String(text) if *text == "run" => Some(Self::Run),
            Dynamic::String(text) if *text == "setup" => Some(Self::Setup),
            _ => None,
        }
    }
}

/// O que `render` faz com um `a[href]` do documento.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderedLink {
    /// Não é um link de tryout: fica como está.
    Unchanged,
    /// Link inválido ou de tryout desconhecido: vira texto inerte com a explicação.
    Fallback(String),
    /// Vira o controle do tryout (`span.release-notes-tryout`) com o índice e o estado.
    Tryout { index: usize, state: ReleaseNotesTryoutState },
}

#[derive(Clone)]
struct TryoutLink {
    id: String,
    label: Option<String>,
}

const REQUEST_KEYS: [&str; 5] = ["type", "documentId", "id", "index", "action"];

fn invalid_link_message() -> String {
    localize("releaseNotes.tryout.invalidLink", "This feature example link is invalid.", &[])
}

fn unknown_message() -> String {
    localize("releaseNotes.tryout.unknown", "This example is not available in this version of VS Code.", &[])
}

/// `parseTryoutLink`: `Ok(None)` para o que não é o comando do tryout; erro para o link do comando com autoridade,
/// fragmento ou argumentos inválidos.
fn parse_tryout_link(uri: &URI) -> Result<Option<String>, Error> {
    if uri.scheme().to_string_lossy() != schemas::COMMAND || uri.path().to_string_lossy() != RUN_ONBOARDING_TRYOUT_COMMAND_ID {
        return Ok(None);
    }
    if !uri.authority().is_empty() || !uri.fragment().is_empty() {
        return Err(Error::new(invalid_link_message()));
    }
    parse_onboarding_tryout_uri(uri)
}

/// `\w`.
fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// As posições de `needle` em `haystack` (ambos em minúsculas ASCII) com `\b` antes, quando `needle` começa por `\w`.
fn word_bounded_matches<'a>(haystack: &'a str, needle: &'a str) -> impl Iterator<Item = usize> + 'a {
    let bytes = haystack.as_bytes();
    haystack.match_indices(needle).map(|(at, _)| at).filter(move |&at| at == 0 || !is_word_byte(bytes[at - 1]))
}

/// Owns the locally resolved links and interactions for one release notes document.
pub struct ReleaseNotesTryouts {
    pub document_id: String,
    store: Rc<DisposableStore>,
    states: RefCell<BTreeMap<usize, ReleaseNotesTryoutState>>,
    links: RefCell<BTreeMap<usize, TryoutLink>>,
    interaction: Rc<MutableDisposable<dyn IDisposable>>,
    availability_listener: Rc<MutableDisposable<dyn IDisposable>>,
    interaction_target: RefCell<Option<(String, ReleaseNotesTryoutAction)>>,
    interaction_seq: Cell<u64>,
    render_count: Cell<usize>,
    webview: RefCell<Option<Rc<dyn ReleaseNotesTryoutsWebview>>>,
    can_restore_focus: RefCell<Option<Rc<dyn Fn() -> bool>>>,
    ready: Cell<bool>,
    tryout_service: Rc<dyn IOnboardingTryoutService>,
    command_service: Rc<dyn ICommandService>,
    notification_service: Rc<dyn INotificationService>,
    log_service: LogServiceRef,
    me: Weak<ReleaseNotesTryouts>,
}

impl ReleaseNotesTryouts {
    pub fn new(
        tryout_service: Rc<dyn IOnboardingTryoutService>,
        command_service: Rc<dyn ICommandService>,
        notification_service: Rc<dyn INotificationService>,
        log_service: LogServiceRef,
    ) -> Rc<Self> {
        let store = DisposableStore::new();
        let interaction = store.add_rc(MutableDisposable::<dyn IDisposable>::new());
        let availability_listener = store.add_rc(MutableDisposable::<dyn IDisposable>::new());
        Rc::new_cyclic(|me| Self {
            document_id: generate_uuid(),
            store,
            states: RefCell::new(BTreeMap::new()),
            links: RefCell::new(BTreeMap::new()),
            interaction,
            availability_listener,
            interaction_target: RefCell::new(None),
            interaction_seq: Cell::new(0),
            render_count: Cell::new(0),
            webview: RefCell::new(None),
            can_restore_focus: RefCell::new(None),
            ready: Cell::new(false),
            tryout_service,
            command_service,
            notification_service,
            log_service,
            me: me.clone(),
        })
    }

    /// `renderCodeSpan(escapedText)`: `try(id, label)` vira o link de atalho; qualquer outra forma fica como código.
    pub fn render_code_span(&self, escaped_text: &str) -> Option<String> {
        // /^try\((?<id>[^,]+),(?<label>[\s\S]*)\)$/
        let inner = escaped_text.strip_prefix("try(")?.strip_suffix(')')?;
        let (id, label) = inner.split_once(',')?;
        if id.is_empty() {
            return None;
        }
        let id = id.trim_matches(is_js_whitespace);
        let label = label.trim_matches(is_js_whitespace);
        if !is_onboarding_tryout_id(id) || label.is_empty() {
            return None;
        }
        let uri = create_onboarding_tryout_uri(id).ok()?;
        let href = escape(&uri.to_string(false).units().to_vec()).to_string_lossy();
        Some(format!("<a class=\"release-notes-tryout-shorthand\" href=\"{href}\">{label}</a>"))
    }

    /// `needsRender(content)`.
    pub fn needs_render(&self, content: &str) -> bool {
        let html = content.to_ascii_lowercase();
        let bytes = html.as_bytes();
        for at in word_bounded_matches(&html, "data-release-notes-tryout-") {
            let rest = at + "data-release-notes-tryout-".len();
            let name_end = if html[rest..].starts_with("id") {
                rest + 2
            } else if html[rest..].starts_with("index") {
                rest + 5
            } else {
                continue;
            };
            let mut after = name_end;
            while html[after..].chars().next().is_some_and(is_js_whitespace) {
                after += html[after..].chars().next().map_or(1, char::len_utf8);
            }
            if bytes.get(after) == Some(&b'=') {
                return true;
            }
        }
        let mut position = 0;
        while let Some(found) = html[position..].find("href=\"") {
            let at = position + found;
            let value_start = at + "href=\"".len();
            if at != 0 && is_word_byte(bytes[at - 1]) {
                position = at + 1;
                continue;
            }
            let Some(length) = content[value_start..].find('"') else { break };
            match parse_tryout_link(&URI::parse(&content[value_start..value_start + length], false)) {
                Ok(None) => {}
                Ok(Some(_)) | Err(_) => return true,
            }
            position = value_start + length + 1;
        }
        false
    }

    /// `render(container)`: tira os atributos `data-release-notes-tryout-*` forjados do conteúdo buscado e troca cada `a[href]`
    /// (em ordem de documento) pelo que [`Self::render_link`] decide.
    pub fn render(&self, container: &mut Element) {
        self.render_count.set(self.render_count.get() + 1);
        strip_tryout_attributes(&mut container.children);
        let children = std::mem::take(&mut container.children);
        container.children = self.render_links(children);
    }

    /// Quantas vezes [`Self::render`] foi chamado (o `callCount` do `stub(tryouts, 'render').callThrough()`).
    pub fn render_count(&self) -> usize {
        self.render_count.get()
    }

    /// O `querySelectorAll('a[href]')` estático de `render`: o que substitui um link não é visitado de novo.
    fn render_links(&self, nodes: Vec<Node>) -> Vec<Node> {
        let mut out = Vec::with_capacity(nodes.len());
        for node in nodes {
            let mut element = match node {
                Node::Element(element) => element,
                text => {
                    out.push(text);
                    continue;
                }
            };
            let href = if element.tag_name == "a" { element.get_attribute("href").map(JsString::to_string_lossy) } else { None };
            if let Some(href) = href {
                let text = element.text_content().to_string_lossy();
                match self.render_link(&href, &text, has_class(&element, "release-notes-tryout-shorthand")) {
                    RenderedLink::Unchanged => {}
                    RenderedLink::Fallback(fallback) => {
                        let mut span = Element::new("span", Vec::new());
                        span.children = vec![Node::Text(JsString::from(fallback.as_str()))];
                        out.push(Node::Element(span));
                        continue;
                    }
                    RenderedLink::Tryout { index, state } => {
                        out.push(Node::Element(tryout_control(index, &state)));
                        continue;
                    }
                }
            }
            element.children = self.render_links(std::mem::take(&mut element.children));
            out.push(Node::Element(element));
        }
        out
    }

    /// A decisão que `render` toma para um `a[href]`: `text_content` é o `textContent` do link e `is_shorthand` diz se
    /// ele tem a classe `release-notes-tryout-shorthand`.
    pub fn render_link(&self, href: &str, text_content: &str, is_shorthand: bool) -> RenderedLink {
        let replace_invalid = |message: String| {
            let label = if text_content.is_empty() { localize("releaseNotes.tryout.label", "Try This", &[]) } else { text_content.to_string() };
            RenderedLink::Fallback(localize("releaseNotes.tryout.fallback", "{0} ({1})", &[&label, &message]))
        };
        let id = match parse_tryout_link(&URI::parse(href, false)) {
            Ok(Some(id)) => id,
            Ok(None) => return RenderedLink::Unchanged,
            Err(_) => return replace_invalid(invalid_link_message()),
        };
        if self.tryout_service.get_tryout(&id).is_none() {
            return replace_invalid(unknown_message());
        }

        if self.availability_listener.value().is_none() {
            let weak = self.me.clone();
            self.availability_listener.set(Some(self.tryout_service.on_did_change().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update(false);
                }
            })));
        }
        let index = self.links.borrow().len();
        let label = (is_shorthand && !text_content.is_empty()).then(|| text_content.to_string());
        let state = self.get_state(&id, index, label.as_deref());
        self.links.borrow_mut().insert(index, TryoutLink { id, label });
        self.states.borrow_mut().insert(index, state.clone());
        RenderedLink::Tryout { index, state }
    }

    fn get_state(&self, id: &str, index: usize, custom_label: Option<&str>) -> ReleaseNotesTryoutState {
        let metadata = self.tryout_service.get_tryout(id).and_then(|scenario| scenario.tryout.clone());
        let availability = if metadata.is_some() {
            self.get_availability(id)
        } else {
            OnboardingTryoutAvailability::Unavailable(crate::contrib::onboarding::onboarding_tryout::OnboardingTryoutUnavailable {
                message: unknown_message(),
                action: None,
            })
        };
        let empty = ReleaseNotesTryoutState {
            id: id.to_string(),
            index,
            kind: ReleaseNotesTryoutKind::Hidden,
            label: String::new(),
            aria_label: String::new(),
            href: String::new(),
            message: String::new(),
            setup_label: String::new(),
            setup_aria_label: String::new(),
        };
        if matches!(availability, OnboardingTryoutAvailability::Hidden) {
            return empty;
        }
        let label = match (custom_label, &metadata) {
            (Some(custom), _) => custom.to_string(),
            (None, Some(metadata)) => localize("releaseNotes.tryout.contextualLabel", "Try This: {0}", &[&metadata.title]),
            (None, None) => localize("releaseNotes.tryout.label", "Try This", &[]),
        };
        let (message, setup_label) = match &availability {
            OnboardingTryoutAvailability::Unavailable(unavailable) => {
                (unavailable.message.clone(), unavailable.action.as_ref().map(|action| action.label.clone()).unwrap_or_default())
            }
            _ => (String::new(), String::new()),
        };
        let ready = matches!(availability, OnboardingTryoutAvailability::Ready);
        let mut aria_parts = vec![label.clone()];
        if let Some(metadata) = &metadata {
            if custom_label.is_some_and(|custom| custom != metadata.title) {
                aria_parts.push(metadata.title.clone());
            }
            aria_parts.push(metadata.description.clone());
            if metadata.target_window == Some(OnboardingTryoutTargetWindow::Agents) {
                aria_parts.push(localize("releaseNotes.tryout.anotherWindow", "Opens in the Agents window.", &[]));
            }
            if metadata.is_ai == Some(true) {
                aria_parts.push(localize(
                    "releaseNotes.tryout.noAutoSend",
                    "Chat examples are prepared for review and are not sent automatically.",
                    &[],
                ));
            }
        }
        aria_parts.push(message.clone());
        aria_parts.retain(|part| !part.is_empty());
        let setup_aria_label = match (&metadata, setup_label.is_empty()) {
            (Some(metadata), false) => localize("releaseNotes.tryout.setupLabel", "{0} for {1}", &[&setup_label, &metadata.title]),
            _ => String::new(),
        };
        ReleaseNotesTryoutState {
            id: id.to_string(),
            index,
            kind: if ready { ReleaseNotesTryoutKind::Ready } else { ReleaseNotesTryoutKind::Unavailable },
            label,
            aria_label: aria_parts.join(" "),
            href: if ready { create_onboarding_tryout_uri(id).map(|uri| uri.to_string(false).to_string_lossy()).unwrap_or_default() } else { String::new() },
            message,
            setup_label,
            setup_aria_label,
        }
    }

    /// `attach(webview, canRestoreFocus)`.
    pub fn attach(&self, webview: Rc<dyn ReleaseNotesTryoutsWebview>, can_restore_focus: Rc<dyn Fn() -> bool>) {
        *self.webview.borrow_mut() = Some(webview.clone());
        *self.can_restore_focus.borrow_mut() = Some(can_restore_focus);
        let weak = self.me.clone();
        self.store.add(webview.on_did_dispose().once().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.dispose();
            }
        }));
        let weak = self.me.clone();
        self.store.add(webview.on_message().listen(move |event| {
            if let Some(this) = weak.upgrade() {
                this.on_message(&event.message);
            }
        }));
    }

    fn get_availability(&self, id: &str) -> OnboardingTryoutAvailability {
        match errors::catch(|| self.tryout_service.get_availability(id)) {
            Ok(availability) => availability,
            Err(error) => {
                self.log_service.error_args(
                    &LogMessage::from(format!("[ReleaseNotesTryouts] Could not resolve '{id}'")),
                    &[LogArg::Error(error)],
                );
                OnboardingTryoutAvailability::Unavailable(crate::contrib::onboarding::onboarding_tryout::OnboardingTryoutUnavailable {
                    message: localize("releaseNotes.tryout.failed", "This feature example could not be loaded.", &[]),
                    action: None,
                })
            }
        }
    }

    /// O link de `index` quando o número do pedido é um índice inteiro de um link existente.
    fn link_at(&self, index: f64) -> Option<TryoutLink> {
        if index < 0.0 || index.fract() != 0.0 || index > u32::MAX as f64 {
            return None;
        }
        self.links.borrow().get(&(index as usize)).cloned()
    }

    fn on_message(&self, message: &Dynamic) {
        if !matches!(message, Dynamic::Object(_)) {
            return;
        }
        match message.get_str("documentId") {
            Dynamic::String(document_id) if document_id.to_string_lossy() == self.document_id => {}
            _ => return,
        }
        let Dynamic::String(kind) = message.get_str("type") else { return };
        if kind == "releaseNotesTryoutsReady" {
            self.ready.set(true);
            self.update(true);
        } else if kind == "releaseNotesTryout" {
            let id = match message.get_str("id") {
                Dynamic::String(id) if is_onboarding_tryout_id(&id.to_string_lossy()) => Some(id.to_string_lossy()),
                _ => None,
            };
            let index = match message.get_str("index") {
                Dynamic::Number(index) => Some(index),
                _ => None,
            };
            let action = ReleaseNotesTryoutAction::parse(&message.get_str("action"));
            let known_link = id.as_ref().zip(index).and_then(|(id, index)| self.link_at(index).filter(|link| link.id == *id).map(|_| index));
            let only_known_keys = message.own_keys().iter().all(|key| REQUEST_KEYS.contains(&key.to_string_lossy().as_str()));
            match (id, known_link, action) {
                (Some(id), Some(index), Some(action)) if only_known_keys => {
                    let _ = self.interact(&id, index as usize, action);
                }
                _ => self.notification_service.error(vec![NotificationMessage::from(invalid_link_message())]),
            }
        }
    }

    /// `openLink(uri)`.
    pub fn open_link(&self, uri: &URI) -> Promise<()> {
        let Ok(Some(id)) = parse_tryout_link(uri) else { return Promise::resolved(()) };
        let link = self.links.borrow().iter().find(|(_, value)| value.id == id).map(|(index, _)| *index);
        match link {
            Some(index) => self.interact(&id, index, ReleaseNotesTryoutAction::Run),
            None => Promise::resolved(()),
        }
    }

    fn interact(&self, id: &str, index: usize, action: ReleaseNotesTryoutAction) -> Promise<()> {
        let this = self.me.upgrade().expect("o documento está vivo enquanto é usado");
        let repeated = this.interaction_target.borrow().as_ref().is_some_and(|(target, kind)| target == id && *kind == action);
        if this.store.is_disposed() || repeated {
            return Promise::resolved(());
        }
        *this.interaction_target.borrow_mut() = Some((id.to_string(), action));
        let token_source = CancellationTokenSource::new(None);
        let interaction = to_disposable({
            let token_source = token_source.clone();
            move || token_source.dispose_with(true)
        });
        let seq = this.interaction_seq.get() + 1;
        this.interaction_seq.set(seq);
        this.interaction.set(Some(interaction));
        let id = id.to_string();
        async_fn(async move {
            if let Err(error) = this.run_interaction(&id, index, action, &token_source).await
                && !token_source.token().is_cancellation_requested()
                && !this.store.is_disposed()
            {
                if !is_cancellation_error(&error) {
                    this.notification_service.error(vec![NotificationMessage::from(error)]);
                }
                this.restore_focus(index, action);
                this.update(false);
            }
            // finally
            if this.interaction_seq.get() == seq {
                *this.interaction_target.borrow_mut() = None;
                this.interaction.clear();
            }
            Ok(())
        })
    }

    /// O corpo do `try` de `interact`.
    async fn run_interaction(
        &self,
        id: &str,
        index: usize,
        action: ReleaseNotesTryoutAction,
        token_source: &CancellationTokenSource,
    ) -> Result<(), Error> {
        let availability = self.tryout_service.get_tryout(id).map(|_| self.get_availability(id));
        let setup_action = match &availability {
            Some(OnboardingTryoutAvailability::Unavailable(unavailable)) => unavailable.action.clone(),
            _ => None,
        };
        let proceed = match &availability {
            Some(OnboardingTryoutAvailability::Ready) => true,
            Some(_) => action == ReleaseNotesTryoutAction::Setup && setup_action.is_some(),
            None => false,
        };
        if !proceed {
            self.update(false);
            if !matches!(availability, Some(OnboardingTryoutAvailability::Hidden)) {
                let message = match &availability {
                    Some(OnboardingTryoutAvailability::Unavailable(unavailable)) => unavailable.message.clone(),
                    _ => unknown_message(),
                };
                self.notification_service.warn(vec![NotificationMessage::from(message)]);
            }
            self.restore_focus(index, action);
            return Ok(());
        }
        if action == ReleaseNotesTryoutAction::Setup {
            if let Some(setup) = setup_action {
                let command = setup.command;
                self.command_service.execute_command(&command.id, command.arguments.unwrap_or_default()).await?;
            } else {
                self.notification_service.info(vec![NotificationMessage::from(localize(
                    "releaseNotes.tryout.nowReady",
                    "This example is ready. Activate the example link to continue.",
                    &[],
                ))]);
                self.restore_focus(index, ReleaseNotesTryoutAction::Run);
            }
        } else {
            let options = OnboardingTryoutRunOptions { source: OnboardingTryoutSource::ReleaseNotes, run_id: None };
            let result = errors::catch(|| self.tryout_service.run(id, token_source.token(), Some(options)))?.await?;
            if token_source.token().is_cancellation_requested() || self.store.is_disposed() {
                return Ok(());
            }
            if let OnboardingTryoutResult::Unavailable(unavailable) = &result {
                self.notification_service.warn(vec![NotificationMessage::from(unavailable.message.clone())]);
            }
            if matches!(result, OnboardingTryoutResult::Cancelled | OnboardingTryoutResult::Unavailable(_)) {
                self.restore_focus(index, action);
            }
        }
        if !token_source.token().is_cancellation_requested() {
            self.update(false);
        }
        Ok(())
    }

    fn restore_focus(&self, index: usize, action: ReleaseNotesTryoutAction) {
        if self.store.is_disposed() {
            return;
        }
        let can_restore = self.can_restore_focus.borrow().clone();
        if !can_restore.is_some_and(|can_restore| can_restore()) {
            return;
        }
        let webview = self.webview.borrow().clone();
        if let Some(webview) = webview {
            webview.focus();
            let _ = webview.post_message(Dynamic::object([
                ("type", Dynamic::string("releaseNotesTryoutFocus")),
                ("documentId", Dynamic::string(&self.document_id)),
                ("index", Dynamic::Number(index as f64)),
                ("action", Dynamic::string(action.as_str())),
            ]));
        }
    }

    fn update(&self, force: bool) {
        let webview = self.webview.borrow().clone();
        let Some(webview) = webview else { return };
        if !self.ready.get() || self.store.is_disposed() {
            return;
        }
        let links: Vec<(usize, TryoutLink)> = self.links.borrow().iter().map(|(index, link)| (*index, link.clone())).collect();
        let mut states = Vec::new();
        for (index, link) in links {
            let state = self.get_state(&link.id, index, link.label.as_deref());
            if force || self.states.borrow().get(&index) != Some(&state) {
                self.states.borrow_mut().insert(index, state.clone());
                states.push(state);
            }
        }
        if !states.is_empty() {
            let _ = webview.post_message(Dynamic::object([
                ("type", Dynamic::string("releaseNotesTryouts")),
                ("documentId", Dynamic::string(&self.document_id)),
                ("states", Dynamic::array(states.iter().map(ReleaseNotesTryoutState::to_dynamic))),
            ]));
        }
    }
}

impl IDisposable for ReleaseNotesTryouts {
    fn dispose(&self) {
        *self.interaction_target.borrow_mut() = None;
        *self.webview.borrow_mut() = None;
        *self.can_restore_focus.borrow_mut() = None;
        self.states.borrow_mut().clear();
        self.links.borrow_mut().clear();
        self.store.dispose();
    }
}

fn attribute(name: &str, value: &str) -> Attribute {
    Attribute { name: name.to_owned(), value: JsString::from(value) }
}

/// `element.classList.contains(class)`.
fn has_class(element: &Element, class: &str) -> bool {
    element.get_attribute("class").is_some_and(|value| value.to_string_lossy().split_ascii_whitespace().any(|name| name == class))
}

/// `removeAttribute('data-release-notes-tryout-id' e '-index')` em todos os descendentes.
fn strip_tryout_attributes(nodes: &mut [Node]) {
    for node in nodes {
        if let Node::Element(element) = node {
            remove_attribute(element, "data-release-notes-tryout-id");
            remove_attribute(element, "data-release-notes-tryout-index");
            strip_tryout_attributes(&mut element.children);
        }
    }
}

/// O controle que substitui o link: `span.release-notes-tryout` com o link, a mensagem e o botão de preparação.
fn tryout_control(index: usize, state: &ReleaseNotesTryoutState) -> Element {
    let link = Element::new("a", vec![attribute("class", "release-notes-tryout-link"), attribute("role", "link"), attribute("tabindex", "0")]);
    let message = Element::new("span", vec![attribute("class", "release-notes-tryout-message")]);
    let setup = Element::new("button", vec![attribute("class", "release-notes-tryout-setup"), attribute("type", "button")]);
    let mut element = Element::new(
        "span",
        vec![
            attribute("class", "release-notes-tryout"),
            attribute("data-release-notes-tryout-id", &state.id),
            attribute("data-release-notes-tryout-index", &index.to_string()),
        ],
    );
    element.children = vec![Node::Element(link), Node::Element(message), Node::Element(setup)];
    apply_release_notes_tryout_state(&mut element, state);
    element
}

/// O caminho (índices em `children`) do primeiro descendente que satisfaz `matches`, em ordem de documento.
fn find_path(element: &Element, matches: &dyn Fn(&Element) -> bool) -> Option<Vec<usize>> {
    for (index, child) in element.children.iter().enumerate() {
        if let Node::Element(child) = child {
            if matches(child) {
                return Some(vec![index]);
            }
            if let Some(mut rest) = find_path(child, matches) {
                rest.insert(0, index);
                return Some(rest);
            }
        }
    }
    None
}

fn element_at_mut<'a>(element: &'a mut Element, path: &[usize]) -> Option<&'a mut Element> {
    let Some((first, rest)) = path.split_first() else { return Some(element) };
    match element.children.get_mut(*first)? {
        Node::Element(child) => element_at_mut(child, rest),
        Node::Text(_) | Node::Comment(_) => None,
    }
}

fn descendant_with_class_mut<'a>(element: &'a mut Element, class: &str) -> Option<&'a mut Element> {
    let path = find_path(element, &|candidate| has_class(candidate, class))?;
    element_at_mut(element, &path)
}

/// `element.textContent = text`.
fn set_text_content(element: &mut Element, text: &str) {
    element.children = if text.is_empty() { Vec::new() } else { vec![Node::Text(JsString::from(text))] };
}

/// `element.hidden = hidden`.
fn set_hidden(element: &mut Element, hidden: bool) {
    if hidden {
        set_attribute(element, "hidden", JsString::new());
    } else {
        remove_attribute(element, "hidden");
    }
}

/// `applyReleaseNotesTryoutState(element, state)`, sem a parte que devolve o foco (ver o cabeçalho do módulo).
pub fn apply_release_notes_tryout_state(element: &mut Element, state: &ReleaseNotesTryoutState) {
    set_hidden(element, state.kind == ReleaseNotesTryoutKind::Hidden);
    if let Some(link) = descendant_with_class_mut(element, "release-notes-tryout-link") {
        set_text_content(link, &state.label);
        set_attribute(link, "aria-label", JsString::from(state.aria_label.as_str()));
        set_attribute(link, "aria-disabled", JsString::from(if state.kind == ReleaseNotesTryoutKind::Ready { "false" } else { "true" }));
        if state.href.is_empty() {
            remove_attribute(link, "href");
        } else {
            set_attribute(link, "href", JsString::from(state.href.as_str()));
        }
    }
    if let Some(message) = descendant_with_class_mut(element, "release-notes-tryout-message") {
        set_text_content(message, &state.message);
        set_hidden(message, state.message.is_empty());
    }
    if let Some(setup) = descendant_with_class_mut(element, "release-notes-tryout-setup") {
        set_text_content(setup, &state.setup_label);
        set_attribute(setup, "aria-label", JsString::from(state.setup_aria_label.as_str()));
        set_hidden(setup, state.setup_label.is_empty());
    }
}

/// `Number(text)` de um `dataset`: o texto vazio é `0` e o que não é número é `NaN`.
fn parse_number(text: &str) -> f64 {
    let trimmed = text.trim();
    if trimmed.is_empty() { 0.0 } else { trimmed.parse().unwrap_or(f64::NAN) }
}

fn dataset(element: &Element, name: &str) -> Option<String> {
    element.get_attribute(name).map(JsString::to_string_lossy)
}

impl ReleaseNotesTryoutState {
    /// O estado que veio numa mensagem `releaseNotesTryouts` (o inverso de `to_dynamic`).
    fn from_dynamic(value: &Dynamic) -> Option<Self> {
        let text = |key: &str| match value.get_str(key) {
            Dynamic::String(text) => Some(text.to_string_lossy()),
            _ => None,
        };
        let Dynamic::Number(index) = value.get_str("index") else { return None };
        let kind = match text("kind")?.as_str() {
            "ready" => ReleaseNotesTryoutKind::Ready,
            "hidden" => ReleaseNotesTryoutKind::Hidden,
            "unavailable" => ReleaseNotesTryoutKind::Unavailable,
            _ => return None,
        };
        Some(Self {
            id: text("id")?,
            index: index as usize,
            kind,
            label: text("label")?,
            aria_label: text("ariaLabel")?,
            href: text("href")?,
            message: text("message")?,
            setup_label: text("setupLabel")?,
            setup_aria_label: text("setupAriaLabel")?,
        })
    }
}

/// `initializeReleaseNotesTryouts(targetDocument, documentId, vscode)`: o lado do documento. No original é um script
/// serializado que roda no webview; aqui é o objeto que o widget nativo do documento chama com os cliques e as mensagens.
pub struct ReleaseNotesTryoutsClient {
    document_id: String,
    post_message: Rc<dyn Fn(Dynamic)>,
    aborted: Cell<bool>,
}

impl ReleaseNotesTryoutsClient {
    /// Registra o cliente e posta `releaseNotesTryoutsReady`.
    pub fn initialize(document_id: &str, post_message: Rc<dyn Fn(Dynamic)>) -> Rc<Self> {
        let client = Rc::new(Self { document_id: document_id.to_owned(), post_message, aborted: Cell::new(false) });
        (client.post_message)(Dynamic::object([("type", Dynamic::string("releaseNotesTryoutsReady")), ("documentId", Dynamic::string(document_id))]));
        client
    }

    /// O ouvinte de `click` da captura: `target_path` é o caminho do alvo a partir de `root`. Devolve se o clique foi tratado
    /// (o `preventDefault` e o `stopImmediatePropagation`).
    pub fn click(&self, root: &Element, target_path: &[usize]) -> bool {
        if self.aborted.get() {
            return false;
        }
        let mut chain: Vec<&Element> = vec![root];
        for &index in target_path {
            let Some(Node::Element(next)) = chain.last().and_then(|element| element.children.get(index)) else { return false };
            chain.push(next);
        }
        // `event.target.closest('a, button')` e `control.closest('[data-release-notes-tryout-id]')`
        let Some(control_position) = chain.iter().rposition(|element| matches!(element.tag_name.as_str(), "a" | "button")) else { return false };
        let control = chain[control_position];
        let Some(element) = chain[..=control_position].iter().rev().find(|element| element.get_attribute("data-release-notes-tryout-id").is_some()) else {
            return false;
        };
        let aria_disabled = control.get_attribute("aria-disabled").is_some_and(|value| *value == "true");
        if element.get_attribute("hidden").is_some() || aria_disabled {
            return true;
        }
        let index = parse_number(&dataset(element, "data-release-notes-tryout-index").unwrap_or_default());
        (self.post_message)(Dynamic::object([
            ("type", Dynamic::string("releaseNotesTryout")),
            ("documentId", Dynamic::string(&self.document_id)),
            ("id", Dynamic::string(&dataset(element, "data-release-notes-tryout-id").unwrap_or_default())),
            ("index", Dynamic::Number(index)),
            ("action", Dynamic::string(if control.tag_name == "button" { "setup" } else { "run" })),
        ]));
        true
    }

    /// O ouvinte de `message`: aplica os estados de `releaseNotesTryouts` aos controles (a mensagem `releaseNotesTryoutFocus`
    /// só move o foco, que a árvore não tem).
    pub fn handle_message(&self, root: &mut Element, message: &Dynamic) {
        if self.aborted.get() {
            return;
        }
        match message.get_str("documentId") {
            Dynamic::String(document_id) if document_id.to_string_lossy() == self.document_id => {}
            _ => return,
        }
        let is_states = matches!(message.get_str("type"), Dynamic::String(kind) if kind == "releaseNotesTryouts");
        let Dynamic::Array(states) = message.get_str("states") else { return };
        if !is_states {
            return;
        }
        for value in states.to_vec() {
            let Some(state) = ReleaseNotesTryoutState::from_dynamic(&value) else { continue };
            let wanted = state.index as f64;
            let path = find_path(root, &|candidate| {
                dataset(candidate, "data-release-notes-tryout-id").is_some()
                    && parse_number(&dataset(candidate, "data-release-notes-tryout-index").unwrap_or_default()) == wanted
            });
            let Some(path) = path else { continue };
            if let Some(element) = element_at_mut(root, &path)
                && dataset(element, "data-release-notes-tryout-id").as_deref() == Some(state.id.as_str())
            {
                apply_release_notes_tryout_state(element, &state);
            }
        }
    }

    /// O `AbortController.abort()` (do `pagehide` ou da função devolvida): os ouvintes saem.
    pub fn dispose(&self) {
        self.aborted.set(true);
    }
}
