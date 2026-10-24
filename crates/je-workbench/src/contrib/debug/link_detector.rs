//! Porte de `src/vs/workbench/contrib/debug/browser/linkDetector.ts`: a detecção de links (URLs web, caminhos de
//! arquivo absolutos e relativos, com `:linha:coluna` ou `:line N`) na saída do console de depuração, e a montagem do
//! resultado com os links decorados.
//!
//! Divergências (a UI do porte é nativa, sem DOM):
//! - **`HTMLElement` e `Node`.** O resultado de `linkify` é uma árvore pequena em Rust: [`LinkNode`] (`Text` ou
//!   `Element`) e [`Element`] (`span` ou `a`, com atributos na ordem em que foram gravados, filhos e os dados que o
//!   original pendura no elemento). Ela responde ao que o original e os testes leem do DOM: `tagName`, `children`,
//!   `firstElementChild`, `textContent` e `outerHTML` (com a mesma serialização: texto escapa `&`, `<`, `>` e o espaço
//!   não separável; atributo escapa `&` e `"`).
//! - **Listeners do DOM.** `addDisposableListener(link, 'mousemove' | 'mouseleave' | 'click' | 'keydown')` viram os
//!   métodos [`Element::mouse_move`], [`Element::mouse_leave`], [`Element::click`] e [`Element::key_down`], com a mesma
//!   lógica (classe `pointer`, ignorar o clique com texto selecionado, exigir Cmd/Ctrl, Enter e Space). A UI nativa os
//!   chama ao despachar o evento. Os quatro listeners do original são um só descartável na `store`: descartar a loja
//!   desliga todos. `getWindow(link).getSelection()` é o [`ClickSelection`].
//! - **`IHoverService`.** Ainda não foi portado. `DebugLinkHoverBehavior::Rich` grava o conteúdo do hover gerenciável em
//!   [`Element::managed_hover`] (o título que o `setupManagedHover` receberia) para a camada de hover nativa ler;
//!   `Basic` grava o atributo `title`, como o original.
//! - **`ITunnelService`** e **`IDebugSession`.** Nenhum dos dois foi portado: [`ITunnelService`] é o recorte que este
//!   arquivo lê (`canTunnel`) e [`IDebugSessionLocations`] o recorte do `IDebugSession` (`resolveLocationReference`),
//!   com o `Source.openInEditor` em [`IDebugSourceOpener`]. O [`LinkDetector`] é montado por [`LinkDetector::new`] com
//!   os serviços na ordem do construtor do original (sem o `IHoverService`).
//! - **Parâmetros opcionais de `linkify`** (`splitLines`, `workspaceFolder`, `includeFulltext`, `highlights`) são o
//!   [`LinkifyOptions`], que tem o valor padrão do original em `Default`.
//! - **Índices.** O `IHighlight` e o `index`/`length` das partes são em unidades UTF-16, como em JavaScript.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Truthy;
use je_base::lifecycle::{DisposableStore, to_disposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::path::{PathModule, Platform as PathPlatform, ProcessEnvironment};
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_base::key_codes::KeyCode;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::i_file_service::IFileService;
use je_platform::opener::editor_types::{EditorOptions, TextEditorSelection};
use je_platform::opener::opener::{IOpenerService, OpenOptions, OpenResource};
use je_platform::path::path_service::IPathService;
use je_platform::resources::{host_platform, uri_file};
use je_platform::workspace::WorkspaceFolder;
use je_widgets::highlighted_label::IHighlight;
use zyarr::RegExp;

use crate::common::editor::{TextEditorOptions, UntypedEditorInput};
use crate::services::editor::editor_service::IEditorService;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;

const CONTROL_CODES: &str = r"\u0000-\u0020\u007f-\u009f";
const WIN_ABSOLUTE_PATH: &str = r#"(?:[a-zA-Z]:(?:(?:\\|\/)[\w\s\.@\-\(\)\[\]{}!#$%^&'`~+=]+)+)"#;
const WIN_RELATIVE_PATH: &str = r#"(?:(?:\~|\.+)(?:(?:\\|\/)[\w\s\.@\-\(\)\[\]{}!#$%^&'`~+=]+)+)"#;
const POSIX_PATH: &str = r#"((?:\~|\.+)?(?:\/[\w\s\.@\-\(\)\[\]{}!#$%^&'`~+=]+)+)"#;
/// Support both ":line 123" and ":123:45" formats for line/column numbers.
const LINE_COLUMN: &str = r"(?::(?:line\s+)?([\d]+))?(?::([\d]+))?";
const LINE_COLUMN_REGEX: &str = r":(?:line\s+)?([\d]+)(?::([\d]+))?$";

const MAX_LENGTH: usize = 2000;

/// `WEB_LINK_REGEX` (`'ug'`).
fn web_link_pattern() -> String {
    format!(
        r#"(?:[a-zA-Z][a-zA-Z0-9+.-]{{2,}}:\/\/|data:|www\.)[^\s{CONTROL_CODES}"]{{2,}}[^\s{CONTROL_CODES}"')}}\],:;.!?]"#
    )
}

/// `PATH_LINK_REGEX` (`'g'`): o caminho do Windows ou o POSIX, conforme a plataforma, e a linha e coluna.
fn path_link_pattern(is_windows: bool) -> String {
    let path = if is_windows { format!(r"({WIN_ABSOLUTE_PATH}|{WIN_RELATIVE_PATH})") } else { POSIX_PATH.to_string() };
    format!("{path}{LINE_COLUMN}")
}

/// O trecho `[start, end)` de `text` em unidades UTF-16 (`String#substring`, com `end` ausente até o fim).
fn utf16_substring(text: &str, start: usize, end: Option<usize>) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let end = end.unwrap_or(units.len()).min(units.len());
    let start = start.min(end);
    String::from_utf16_lossy(&units[start..end])
}

/// `fileLinkToUri`: converts a `file:` URI detected in console output into a URI that can be handed to the file and
/// editor services.
///
/// Going through the file system path is needed because the raw URI may carry a path that is not directly usable, see
/// https://github.com/microsoft/vscode/issues/109076. The result must be built with `URI.file` rather than
/// `URI.parse`: a file system path is not a URI, and `URI.parse('c:/foo/bar.js')` reads the drive letter as the scheme,
/// which left every `file:` link with a drive letter unopenable (https://github.com/microsoft/vscode/issues/334283).
pub fn file_link_to_uri(uri: &URI, path_separator: &str) -> URI {
    let platform = host_platform();
    let fs_path = uri.fs_path(platform).to_string_lossy();
    let fs_path = if path_separator == "/" && platform == PathPlatform::Win32 { fs_path.replace('\\', "/") } else { fs_path };
    URI::file(PathModule::native(&ProcessEnvironment).normalize(&fs_path), platform)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinkKind {
    Web,
    Path,
    Text,
}

/// `LinkPart`.
struct LinkPart {
    kind: LinkKind,
    value: String,
    /// Tamanho de `value` em unidades UTF-16.
    length: usize,
    captures: Vec<Option<String>>,
    index: usize,
}

/// `DebugLinkHoverBehavior`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugLinkHoverBehavior {
    /// A nice workbench hover.
    Rich,
    /// Basic browser hover (deprecated: consumers should adopt `Rich` by propagating disposables appropriately).
    Basic,
    /// No hover.
    None,
}

/// `DebugLinkHoverBehaviorTypeData`: o tipo do hover e a loja que guarda os descartáveis da decoração (implica
/// `Rich` quando há hover gerenciável).
#[derive(Clone)]
pub struct DebugLinkHoverBehaviorTypeData {
    pub kind: DebugLinkHoverBehavior,
    pub store: Rc<DisposableStore>,
}

/// O `selection` que o clique lê de `getWindow(link).getSelection()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickSelection {
    /// `getSelection()` devolveu `null`.
    Missing,
    /// `selection.type` é `Caret` ou `None`.
    Collapsed,
    /// `selection.type === 'Range'`: o usuário está selecionando texto.
    Range,
}

type ClickHandler = Rc<dyn Fn(bool)>;

struct ElementState {
    /// Os atributos na ordem em que foram gravados (`class` guarda a lista de classes).
    attributes: Vec<(String, String)>,
    children: Vec<LinkNode>,
    managed_hover: Option<String>,
    on_click: Option<ClickHandler>,
}

/// Um elemento da saída de `linkify`: o `<span>` (contêiner, trecho com realce) ou o `<a>` (link).
pub struct Element {
    tag_name: &'static str,
    state: RefCell<ElementState>,
}

/// `Node`: um nó de texto ou um elemento.
#[derive(Clone)]
pub enum LinkNode {
    Text(String),
    Element(Rc<Element>),
}

impl LinkNode {
    /// `node.textContent`.
    pub fn text_content(&self) -> String {
        match self {
            LinkNode::Text(text) => text.clone(),
            LinkNode::Element(element) => element.text_content(),
        }
    }

    fn write_html(&self, out: &mut String) {
        match self {
            LinkNode::Text(text) => {
                for c in text.chars() {
                    match c {
                        '&' => out.push_str("&amp;"),
                        '<' => out.push_str("&lt;"),
                        '>' => out.push_str("&gt;"),
                        '\u{a0}' => out.push_str("&nbsp;"),
                        c => out.push(c),
                    }
                }
            }
            LinkNode::Element(element) => element.write_html(out),
        }
    }
}

impl Element {
    /// `document.createElement(tag)`.
    pub fn new(tag_name: &'static str) -> Rc<Self> {
        Rc::new(Self { tag_name, state: RefCell::new(ElementState { attributes: Vec::new(), children: Vec::new(), managed_hover: None, on_click: None }) })
    }

    /// `tagName`, em maiúsculas como no HTML.
    pub fn tag_name(&self) -> String {
        self.tag_name.to_uppercase()
    }

    /// `childNodes`.
    pub fn child_nodes(&self) -> Vec<LinkNode> {
        self.state.borrow().children.clone()
    }

    /// `children`: só os elementos filhos.
    pub fn children(&self) -> Vec<Rc<Element>> {
        self.state
            .borrow()
            .children
            .iter()
            .filter_map(|child| if let LinkNode::Element(element) = child { Some(element.clone()) } else { None })
            .collect()
    }

    /// `firstElementChild`.
    pub fn first_element_child(&self) -> Option<Rc<Element>> {
        self.children().into_iter().next()
    }

    /// `append(...nodes)`.
    pub fn append(&self, nodes: impl IntoIterator<Item = LinkNode>) {
        self.state.borrow_mut().children.extend(nodes);
    }

    /// `reset(node, ...children)` de `dom.ts`: troca os filhos.
    pub fn reset(&self, children: Vec<LinkNode>) {
        self.state.borrow_mut().children = children;
    }

    /// `textContent`.
    pub fn text_content(&self) -> String {
        self.state.borrow().children.iter().map(LinkNode::text_content).collect()
    }

    /// `getAttribute(name)`.
    pub fn get_attribute(&self, name: &str) -> Option<String> {
        self.state.borrow().attributes.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone())
    }

    /// `setAttribute(name, value)`: mantém a posição de um atributo que já existe.
    pub fn set_attribute(&self, name: &str, value: &str) {
        let mut state = self.state.borrow_mut();
        match state.attributes.iter_mut().find(|(key, _)| key == name) {
            Some(entry) => entry.1 = value.to_string(),
            None => state.attributes.push((name.to_string(), value.to_string())),
        }
    }

    /// `classList.add(...classes)`.
    pub fn add_classes<'a>(&self, classes: impl IntoIterator<Item = &'a str>) {
        let mut list = self.class_list();
        for class in classes {
            if !list.iter().any(|existing| existing == class) {
                list.push(class.to_string());
            }
        }
        self.set_attribute("class", &list.join(" "));
    }

    /// `classList.remove(class)`.
    pub fn remove_class(&self, class: &str) {
        if self.get_attribute("class").is_none() {
            return;
        }
        let list: Vec<String> = self.class_list().into_iter().filter(|existing| existing != class).collect();
        self.set_attribute("class", &list.join(" "));
    }

    /// `classList.toggle(class, force)`.
    pub fn toggle_class(&self, class: &str, force: bool) {
        if force {
            self.add_classes([class]);
        } else {
            self.remove_class(class);
        }
    }

    /// `classList.contains(class)`.
    pub fn has_class(&self, class: &str) -> bool {
        self.class_list().iter().any(|existing| existing == class)
    }

    fn class_list(&self) -> Vec<String> {
        self.get_attribute("class").map(|value| value.split_whitespace().map(str::to_string).collect()).unwrap_or_default()
    }

    /// O conteúdo do hover gerenciável (`setupManagedHover(..., link, title)`), quando o comportamento é `Rich`.
    pub fn managed_hover(&self) -> Option<String> {
        self.state.borrow().managed_hover.clone()
    }

    /// `outerHTML`.
    pub fn outer_html(&self) -> String {
        let mut out = String::new();
        self.write_html(&mut out);
        out
    }

    fn write_html(&self, out: &mut String) {
        let state = self.state.borrow();
        out.push('<');
        out.push_str(self.tag_name);
        for (name, value) in &state.attributes {
            out.push(' ');
            out.push_str(name);
            out.push_str("=\"");
            out.push_str(&value.replace('&', "&amp;").replace('"', "&quot;"));
            out.push('"');
        }
        out.push('>');
        for child in &state.children {
            child.write_html(out);
        }
        out.push_str("</");
        out.push_str(self.tag_name);
        out.push('>');
    }

    /// O `addDisposableListener(link, 'mousemove', ...)`: `link.classList.toggle('pointer', isMac ? metaKey : ctrlKey)`.
    pub fn mouse_move(&self, modifier_pressed: bool) {
        if self.state.borrow().on_click.is_some() {
            self.toggle_class("pointer", modifier_pressed);
        }
    }

    /// O listener de `mouseleave`: `link.classList.remove('pointer')`.
    pub fn mouse_leave(&self) {
        if self.state.borrow().on_click.is_some() {
            self.remove_class("pointer");
        }
    }

    /// O listener de `click`. `modifier_pressed` é `isMac ? event.metaKey : event.ctrlKey`. Devolve `true` quando o evento
    /// foi consumido (`preventDefault` e `stopImmediatePropagation`) e a ação rodou.
    pub fn click(&self, modifier_pressed: bool, selection: ClickSelection) -> bool {
        let on_click = self.state.borrow().on_click.clone();
        let Some(on_click) = on_click else {
            return false;
        };
        if selection != ClickSelection::Collapsed {
            return false; // do not navigate when user is selecting
        }
        if !modifier_pressed {
            return false;
        }
        on_click(false);
        true
    }

    /// O listener de `keydown`: Enter e Space ativam o link (Space preserva o foco). Devolve `true` quando consumiu a tecla.
    pub fn key_down(&self, key_code: KeyCode) -> bool {
        let on_click = self.state.borrow().on_click.clone();
        match on_click {
            Some(on_click) if key_code == KeyCode::Enter || key_code == KeyCode::Space => {
                on_click(key_code == KeyCode::Space);
                true
            }
            _ => false,
        }
    }

    fn set_on_click(&self, on_click: Option<ClickHandler>) {
        self.state.borrow_mut().on_click = on_click;
    }
}

/// `ITunnelService`, reduzido ao `canTunnel` que o detector lê.
pub trait ITunnelService {
    fn can_tunnel(&self, uri: &URI) -> bool;
}

/// O destino de um `resolveLocationReference`: o `Source` e a posição.
#[derive(Clone)]
pub struct DebugLocation {
    pub source: Rc<dyn IDebugSourceOpener>,
    pub line: f64,
    pub column: f64,
    pub end_line: Option<f64>,
    pub end_column: Option<f64>,
}

/// `Source.openInEditor(editorService, selection, preserveFocus)`, com a seleção `IRange` aberta em quatro números.
pub trait IDebugSourceOpener {
    fn open_in_editor(
        &self,
        editor_service: &Rc<dyn IEditorService>,
        start_line_number: f64,
        start_column: f64,
        end_line_number: f64,
        end_column: f64,
        preserve_focus: bool,
    ) -> Promise<()>;
}

/// O recorte de `IDebugSession` que o detector lê: `resolveLocationReference(locationReference)`.
pub trait IDebugSessionLocations {
    fn resolve_location_reference(&self, location_reference: f64) -> Promise<DebugLocation>;
}

/// Os argumentos opcionais de `linkify`.
#[derive(Clone, Default)]
pub struct LinkifyOptions {
    pub split_lines: bool,
    pub workspace_folder: Option<WorkspaceFolder>,
    pub include_fulltext: bool,
    pub highlights: Vec<IHighlight>,
}

/// `ILinkDetector`.
pub trait ILinkDetector {
    fn linkify(&self, text: &str, hover_behavior: &DebugLinkHoverBehaviorTypeData, options: &LinkifyOptions) -> Rc<Element>;

    fn linkify_location(
        &self,
        text: &str,
        location_reference: f64,
        session: Rc<dyn IDebugSessionLocations>,
        hover_behavior: &DebugLinkHoverBehaviorTypeData,
    ) -> Rc<Element>;
}

#[derive(Clone)]
struct DefaultRef {
    location_reference: f64,
    session: Rc<dyn IDebugSessionLocations>,
}

/// Os serviços do detector, compartilhados com os callbacks assíncronos dos links.
struct LinkDetectorServices {
    editor_service: Rc<dyn IEditorService>,
    file_service: Rc<dyn IFileService>,
    opener_service: Rc<dyn IOpenerService>,
    path_service: Rc<dyn IPathService>,
    tunnel_service: Rc<dyn ITunnelService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    configuration_service: Rc<dyn IConfigurationService>,
    web_link_regex: RegExp,
    path_link_regex: RegExp,
    line_column_regex: RegExp,
}

/// `LinkDetector`.
#[derive(Clone)]
pub struct LinkDetector {
    services: Rc<LinkDetectorServices>,
    /// A referência de `makeReferencedLinkDetector`: o texto sem link próprio aponta para ela.
    default_ref: Option<DefaultRef>,
}

fn compile(pattern: &str, flags: &str) -> RegExp {
    RegExp::from_str(pattern, flags).expect("a regex do linkDetector é válida")
}

/// O texto do grupo `index` de um casamento.
fn capture(input: &[u16], found: &zyarr::Match, index: usize) -> Option<String> {
    found.captures.get(index).and_then(Option::as_ref).map(|range| String::from_utf16_lossy(&input[range.clone()]))
}

fn is_macintosh() -> bool {
    Platform::detect().is_macintosh
}

impl LinkDetector {
    /// O construtor do original: os serviços na ordem das dependências injetadas (sem o `IHoverService`).
    pub fn new(
        editor_service: Rc<dyn IEditorService>,
        file_service: Rc<dyn IFileService>,
        opener_service: Rc<dyn IOpenerService>,
        path_service: Rc<dyn IPathService>,
        tunnel_service: Rc<dyn ITunnelService>,
        environment_service: Rc<dyn IWorkbenchEnvironmentService>,
        configuration_service: Rc<dyn IConfigurationService>,
    ) -> Self {
        Self {
            services: Rc::new(LinkDetectorServices {
                editor_service,
                file_service,
                opener_service,
                path_service,
                tunnel_service,
                environment_service,
                configuration_service,
                web_link_regex: compile(&web_link_pattern(), "ug"),
                path_link_regex: compile(&path_link_pattern(Platform::detect().is_windows), "g"),
                line_column_regex: compile(LINE_COLUMN_REGEX, ""),
            }),
            default_ref: None,
        }
    }

    /// `_linkify`: o `linkify` com o `splitLines` à parte (as linhas são linkificadas sem dividir de novo).
    fn linkify_with(&self, text: &str, hover_behavior: &DebugLinkHoverBehaviorTypeData, split_lines: bool, options: &LinkifyOptions) -> Rc<Element> {
        let default_ref = self.default_ref.as_ref();
        if split_lines {
            let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
            let last = lines.len() - 1;
            for line in &mut lines[..last] {
                line.push('\n');
            }
            if lines[last].is_empty() {
                // Remove the last element ('') that split added.
                lines.pop();
            }
            let mut elements: Vec<Rc<Element>> = lines.iter().map(|line| self.linkify_with(line, hover_behavior, false, options)).collect();
            if elements.len() == 1 {
                // Do not wrap single line with extra span.
                return elements.remove(0);
            }
            let container = Element::new("span");
            container.append(elements.into_iter().map(LinkNode::Element));
            return container;
        }

        let container = Element::new("span");
        let fulltext = options.include_fulltext.then_some(text);
        for part in self.detect_links(text) {
            let node = match part.kind {
                LinkKind::Text => match default_ref {
                    Some(default_ref) => LinkNode::Element(self.linkify_location(&part.value, default_ref.location_reference, default_ref.session.clone(), hover_behavior)),
                    None => LinkNode::Text(part.value.clone()),
                },
                LinkKind::Web => LinkNode::Element(self.create_web_link(fulltext, &part.value, hover_behavior)),
                LinkKind::Path => {
                    let path = part.captures[0].clone().unwrap_or_default();
                    let line_number = part.captures.get(1).and_then(Option::as_ref).filter(|value| !value.is_empty()).map_or(0.0, |value| value.parse().unwrap_or(f64::NAN));
                    let column_number = part.captures.get(2).and_then(Option::as_ref).filter(|value| !value.is_empty()).map_or(0.0, |value| value.parse().unwrap_or(f64::NAN));
                    self.create_path_link(fulltext, &part.value, &path, line_number, column_number, options.workspace_folder.as_ref(), hover_behavior)
                }
            };

            container.append(Self::apply_highlights(node, part.index, part.length, &options.highlights));
        }
        container
    }

    fn apply_highlights(node: LinkNode, start_index: usize, length: usize, highlights: &[IHighlight]) -> Vec<LinkNode> {
        let mut children: Vec<LinkNode> = Vec::new();
        let mut current_index = start_index;
        let end_index = start_index + length;
        let content = node.text_content();

        for highlight in highlights {
            if highlight.end <= current_index || highlight.start >= end_index {
                continue;
            }

            if highlight.start > current_index {
                children.push(LinkNode::Text(utf16_substring(&content, current_index - start_index, Some(highlight.start - start_index))));
                current_index = highlight.start;
            }

            let highlight_end = highlight.end.min(end_index);
            let highlighted_text = utf16_substring(&content, current_index - start_index, Some(highlight_end - start_index));
            let highlight_span = Element::new("span");
            highlight_span.add_classes(["highlight"]);
            highlight_span.add_classes(highlight.extra_classes.iter().map(String::as_str));
            highlight_span.append([LinkNode::Text(highlighted_text)]);
            children.push(LinkNode::Element(highlight_span));
            current_index = highlight_end;
        }

        if current_index == start_index {
            return vec![node]; // no changes made
        }

        if current_index < end_index {
            children.push(LinkNode::Text(utf16_substring(&content, current_index - start_index, None)));
        }

        // reuse the element if it's a link
        if let LinkNode::Element(element) = &node {
            element.reset(children);
            return vec![node];
        }

        children
    }

    /// Makes an [`ILinkDetector`] that links everything in the output to the reference if they don't have other explicit
    /// links.
    pub fn make_referenced_link_detector(&self, location_reference: f64, session: Rc<dyn IDebugSessionLocations>) -> Rc<dyn ILinkDetector> {
        Rc::new(Self { services: self.services.clone(), default_ref: Some(DefaultRef { location_reference, session }) })
    }

    fn create_web_link(&self, fulltext: Option<&str>, url: &str, hover_behavior: &DebugLinkHoverBehaviorTypeData) -> Rc<Element> {
        let link = Self::create_link(url);

        let mut uri = URI::parse(url, false);
        // if the URI ends with something like `foo.js:12:3`, parse that into a fragment to reveal that location (#150702)
        let path_units = uri.path().units().to_vec();
        let line_col = self.services.line_column_regex.exec(&path_units, 0);
        let mut selection: Option<(f64, Option<f64>)> = None;
        if let Some(found) = &line_col {
            let line = capture(&path_units, found, 1).unwrap_or_default();
            let column = capture(&path_units, found, 2);
            selection = Some((line.parse().unwrap_or(f64::NAN), column.as_ref().map(|column| column.parse().unwrap_or(f64::NAN))));
            uri = uri.with(&UriChange {
                path: Field::Set(JsString::from(&path_units[..found.start()])),
                // `L<line>,<column>` so that the fragment stays parseable by `extractSelection`
                fragment: Field::Set(JsString::from(format!("L{}{}", line, column.map(|column| format!(",{column}")).unwrap_or_default()))),
                ..UriChange::default()
            });
        }

        let services = self.services.clone();
        let click_uri = uri.clone();
        let url = url.to_string();
        self.decorate_link(
            &link,
            Some(&uri),
            fulltext,
            hover_behavior,
            Rc::new(move |_preserve_focus| {
                if click_uri.scheme().to_string_lossy() == schemas::FILE {
                    let services = services.clone();
                    let uri = click_uri.clone();
                    let _ = services.path_service.path().then_promise(move |flavor| {
                        let separator = if flavor == PathPlatform::Win32 { "\\" } else { "/" };
                        let file_uri = file_link_to_uri(&uri, separator);
                        let services = services.clone();
                        services.file_service.exists(&file_uri).then_promise(move |exists| {
                            if !exists {
                                return Promise::resolved(());
                            }
                            let options = TextEditorOptions {
                                base: EditorOptions { pinned: Some(true), ..EditorOptions::default() },
                                selection: selection.map(|(line, column)| TextEditorSelection {
                                    start_line_number: line,
                                    start_column: column.unwrap_or(1.0),
                                    ..TextEditorSelection::default()
                                }),
                                ..TextEditorOptions::default()
                            };
                            services.editor_service.open_editor(UntypedEditorInput { resource: Some(file_uri), options: Some(options), ..UntypedEditorInput::default() }).then(|_| ())
                        })
                    });
                    return;
                }

                let allow_tunneling = services.environment_service.remote_authority().is_some_and(|authority| !authority.is_empty())
                    && services.configuration_service.get_value(Some("remote.forwardOnOpen"), None).is_truthy();
                let _ = services.opener_service.open(
                    &OpenResource::Text(JsString::from(url.as_str())),
                    Some(&OpenOptions { allow_tunneling: Some(allow_tunneling), ..OpenOptions::default() }),
                );
            }),
        );

        link
    }

    #[allow(clippy::too_many_arguments)]
    fn create_path_link(
        &self,
        fulltext: Option<&str>,
        text: &str,
        path: &str,
        line_number: f64,
        column_number: f64,
        workspace_folder: Option<&WorkspaceFolder>,
        hover_behavior: &DebugLinkHoverBehaviorTypeData,
    ) -> LinkNode {
        let mut chars = path.chars();
        let first = chars.next();
        if first == Some('/') && chars.next() == Some('/') {
            // Most likely a url part which did not match, for example ftp://path.
            return LinkNode::Text(text.to_string());
        }

        // Only set selection if we have a valid line number (greater than 0)
        let selection = (line_number > 0.0).then_some(TextEditorSelection {
            start_line_number: line_number,
            start_column: if column_number > 0.0 { column_number } else { 1.0 },
            ..TextEditorSelection::default()
        });
        let services = self.services.clone();
        let open_resource = move |uri: URI| -> ClickHandler {
            let services = services.clone();
            Rc::new(move |preserve_focus| {
                let options = TextEditorOptions {
                    base: EditorOptions { preserve_focus: Some(preserve_focus), ..EditorOptions::default() },
                    selection,
                    ..TextEditorOptions::default()
                };
                let _ = services.editor_service.open_editor(UntypedEditorInput { resource: Some(uri.clone()), options: Some(options), ..UntypedEditorInput::default() });
            })
        };

        if first == Some('.') {
            let Some(workspace_folder) = workspace_folder else {
                return LinkNode::Text(text.to_string());
            };
            let uri = workspace_folder.to_resource(path);
            let link = Self::create_link(text);
            self.decorate_link(&link, Some(&uri), fulltext, hover_behavior, open_resource(uri.clone()));
            return LinkNode::Element(link);
        }

        let mut path = path.to_string();
        if first == Some('~')
            && let Some(user_home) = self.services.path_service.resolved_user_home()
        {
            let home = user_home.fs_path(host_platform()).to_string_lossy();
            path = PathModule::native(&ProcessEnvironment).join(&[&home, &path[1..]]);
        }

        let link = Self::create_link(text);
        link.set_attribute("tabindex", "0");
        let uri = uri_file(&PathModule::native(&ProcessEnvironment).normalize(&path));
        let detector = self.clone();
        let hover_behavior = hover_behavior.clone();
        let decorated = link.clone();
        let fulltext = fulltext.map(str::to_string);
        // If the uri can not be resolved we should not spam the console with error, remain quite #86587
        let _ = self.services.file_service.stat(&uri).then_result(move |result| {
            if let Ok(stat) = result
                && !stat.is_directory
            {
                detector.decorate_link(&decorated, Some(&uri), fulltext.as_deref(), &hover_behavior, open_resource(uri.clone()));
            }
            Ok(())
        });
        LinkNode::Element(link)
    }

    fn create_link(text: &str) -> Rc<Element> {
        let link = Element::new("a");
        link.append([LinkNode::Text(text.to_string())]);
        link
    }

    fn decorate_link(&self, link: &Rc<Element>, uri: Option<&URI>, fulltext: Option<&str>, hover_behavior: &DebugLinkHoverBehaviorTypeData, on_click: ClickHandler) {
        if hover_behavior.store.is_disposed() {
            return;
        }
        link.add_classes(["link"]);
        let follow_link = if uri.is_some_and(|uri| self.services.tunnel_service.can_tunnel(uri)) {
            localize("followForwardedLink", "follow link using forwarded port", &[])
        } else {
            localize("followLink", "follow link", &[])
        };
        let title = match fulltext {
            Some(fulltext) if !fulltext.is_empty() => {
                if is_macintosh() {
                    localize("fileLinkWithPathMac", "Cmd + click to {0}\n{1}", &[follow_link.as_str(), fulltext])
                } else {
                    localize("fileLinkWithPath", "Ctrl + click to {0}\n{1}", &[follow_link.as_str(), fulltext])
                }
            }
            _ => {
                if is_macintosh() {
                    localize("fileLinkMac", "Cmd + click to {0}", &[follow_link.as_str()])
                } else {
                    localize("fileLink", "Ctrl + click to {0}", &[follow_link.as_str()])
                }
            }
        };
        link.set_attribute("aria-label", &title);

        match hover_behavior.kind {
            DebugLinkHoverBehavior::Rich => link.state.borrow_mut().managed_hover = Some(title),
            DebugLinkHoverBehavior::Basic => link.set_attribute("title", &title),
            DebugLinkHoverBehavior::None => {}
        }

        link.set_on_click(Some(on_click));
        let link = link.clone();
        hover_behavior.store.add(to_disposable(move || link.set_on_click(None)));
    }

    fn detect_links(&self, text: &str) -> Vec<LinkPart> {
        let units: Vec<u16> = text.encode_utf16().collect();
        if units.len() > MAX_LENGTH {
            return vec![LinkPart { kind: LinkKind::Text, value: text.to_string(), length: units.len(), captures: Vec::new(), index: 0 }];
        }

        let mut result = Vec::new();
        self.split_one(&units, 0, 0, &mut result);
        result
    }

    /// `splitOne`: casa a regex `regex_index` (web, depois caminho) e entrega os trechos entre os casamentos à regex
    /// seguinte; o que sobra do fim da lista de regex é texto.
    fn split_one(&self, text: &[u16], regex_index: usize, base_index: usize, result: &mut Vec<LinkPart>) {
        if regex_index >= 2 {
            result.push(LinkPart { kind: LinkKind::Text, value: String::from_utf16_lossy(text), length: text.len(), captures: Vec::new(), index: base_index });
            return;
        }
        let (regex, kind) = if regex_index == 0 { (&self.services.web_link_regex, LinkKind::Web) } else { (&self.services.path_link_regex, LinkKind::Path) };
        let mut current_index = 0;
        let mut last_index = 0;
        while let Some(found) = regex.exec(text, last_index) {
            let Range { start, end } = found.captures[0].clone().unwrap_or(0..0);
            let string_before_match = &text[current_index..start];
            if !string_before_match.is_empty() {
                self.split_one(string_before_match, regex_index + 1, base_index + current_index, result);
            }
            result.push(LinkPart {
                kind,
                value: String::from_utf16_lossy(&text[start..end]),
                length: end - start,
                captures: (1..found.captures.len()).map(|index| capture(text, &found, index)).collect(),
                index: base_index + start,
            });
            current_index = end;
            last_index = if end == start { end + 1 } else { end };
            if last_index > text.len() {
                break;
            }
        }
        let string_after_matches = &text[current_index..];
        if !string_after_matches.is_empty() {
            self.split_one(string_after_matches, regex_index + 1, base_index + current_index, result);
        }
    }
}

impl ILinkDetector for LinkDetector {
    /// Matches and handles web urls, absolute and relative file links in the string provided. Returns the `<span/>`
    /// element that wraps the processed string, where matched links are replaced by `<a/>`. The click handler is
    /// attached to all anchored links that open them in the editor. When `split_lines` is true, each line of the text,
    /// even if it contains no links, is wrapped in a `<span>` and added as a child of the returned `<span>`. The
    /// `hover_behavior` is required and manages the lifecycle of the listeners.
    fn linkify(&self, text: &str, hover_behavior: &DebugLinkHoverBehaviorTypeData, options: &LinkifyOptions) -> Rc<Element> {
        self.linkify_with(text, hover_behavior, options.split_lines, options)
    }

    /// Linkifies a location reference.
    fn linkify_location(
        &self,
        text: &str,
        location_reference: f64,
        session: Rc<dyn IDebugSessionLocations>,
        hover_behavior: &DebugLinkHoverBehaviorTypeData,
    ) -> Rc<Element> {
        let link = Self::create_link(text);
        let services = self.services.clone();
        self.decorate_link(
            &link,
            None,
            Some(text),
            hover_behavior,
            Rc::new(move |preserve_focus| {
                let services = services.clone();
                let _ = session.resolve_location_reference(location_reference).then_promise(move |location| {
                    location.source.open_in_editor(
                        &services.editor_service,
                        location.line,
                        location.column,
                        location.end_line.unwrap_or(location.line),
                        location.end_column.unwrap_or(location.column),
                        preserve_focus,
                    )
                });
            }),
        );

        link
    }
}

