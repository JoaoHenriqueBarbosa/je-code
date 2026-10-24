//! A vista Search (`workbench/contrib/search/browser/searchView.ts`): o campo com as chaves de maiúsculas,
//! palavra inteira e regex, a mensagem `N results in M files` e a árvore de resultados (arquivo, com o número de
//! ocorrências, e as linhas com os trechos casados em destaque). As medidas são as do DOM do oráculo. A busca
//! roda numa thread (`search_backend`); a árvore é uma `ObjectTree` do `je-widgets`.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use je_base::lifecycle::DisposableRef;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Border, Content, Cursor, NodeId, Role, Tree, Ui};
use je_widgets::list::list_styles::default_list_styles;
use je_widgets::list::list_types::{IListVirtualDelegate, TemplateData};
use je_widgets::text::{measure_text, ui_font};
use je_widgets::tree::abstract_tree::ExpandOnlyOnTwistieClick;
use je_widgets::tree::index_tree_model::TreeNode;
use je_widgets::tree::object_tree::{ObjectTree, ObjectTreeOptions};
use je_widgets::tree::object_tree_model::{ObjectTreeModel, ObjectTreeModelSetChildrenOptions};
use je_widgets::tree::tree_types::{CollapsedValue, IObjectTreeElement, ITreeRenderer, TreeElementRenderDetails};

use super::Command;
use super::explorer::LanguageOf;
use super::file_icons::FileIconTheme;
use super::menus::CommandQueue;
use super::search_backend::{FileResult, Query, Search};

const ROW_HEIGHT: f64 = 22.0;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Entry {
    File(PathBuf),
    /// O arquivo e o índice da linha dentro do resultado dele.
    Match(PathBuf, usize),
}

type Model = ObjectTreeModel<Entry>;
type Results = Rc<RefCell<HashMap<PathBuf, FileResult>>>;

struct Delegate;

impl IListVirtualDelegate<Entry> for Delegate {
    fn get_height(&self, _: &Entry) -> f64 {
        ROW_HEIGHT
    }

    fn get_template_id(&self, _: &Entry) -> String {
        "search".to_owned()
    }
}

struct Renderer {
    ui: Ui,
    results: Results,
    icons: Option<FileIconTheme>,
    language_of: LanguageOf,
    foreground: Rgba,
    highlight: Rgba,
    badge_background: Rgba,
    badge_foreground: Rgba,
    width: Rc<RefCell<f64>>,
}

impl ITreeRenderer<Model> for Renderer {
    fn template_id(&self) -> String {
        "search".to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let content = self.ui.tree_mut().create(container, Role::Generic);
        Rc::new(content) as Rc<dyn Any>
    }

    fn render_element(&self, node: &Rc<TreeNode<Option<Entry>>>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let Some(content) = template_data.downcast_ref::<NodeId>().copied() else { return };
        let Some(entry) = &node.element else { return };
        let mut tree = self.ui.tree_mut();
        for child in tree.children(content).to_vec() {
            tree.remove(child);
        }
        let font = ui_font(13.0, 400);
        let results = self.results.borrow();
        // O conteúdo da linha começa depois do recuo e do twistie (8 px por nível mais 22 px de twistie).
        let row_width = *self.width.borrow() - 30.0 - if matches!(entry, Entry::Match(..)) { 8.0 } else { 0.0 };
        tree.set_bounds(content, Rect::new(0.0, 0.0, row_width, ROW_HEIGHT));
        match entry {
            Entry::File(path) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let mut x = 0.0;
                if let Some(icons) = &self.icons {
                    icons.decorate(&mut tree, content, Rect::new(0.0, 0.0, 16.0, ROW_HEIGHT), &name, (self.language_of)(&name).as_deref());
                    x = 22.0;
                }
                let text_width = measure_text(&name, &font);
                let label = tree.create(content, Role::Generic);
                tree.set_bounds(label, Rect::new(x, 3.0, x + text_width, ROW_HEIGHT));
                tree.style_mut(label).content = Content::Text { text: name, style: font.clone(), color: self.foreground, line_height: 18.0 };
                let count = results.get(path).map_or(0, |r| r.match_count);
                // O selo de contagem (`.monaco-count-badge`, 19 px, a pílula).
                let badge_font = ui_font(10.0, 400);
                let text = count.to_string();
                let w = (measure_text(&text, &badge_font) + 10.0).max(19.0).round();
                let badge = tree.create(content, Role::Generic);
                tree.set_bounds(badge, Rect::new(row_width - w - 4.0, 1.5, row_width - 4.0, 20.5));
                let style = tree.style_mut(badge);
                style.background = Some(self.badge_background);
                style.radius = 9.5;
                let t = tree.create(badge, Role::Generic);
                tree.set_bounds(t, Rect::new((w - measure_text(&text, &badge_font)) / 2.0, 0.0, w, 19.0));
                tree.style_mut(t).content = Content::Text { text, style: badge_font, color: self.badge_foreground, line_height: 19.0 };
            }
            Entry::Match(path, index) => {
                let Some(line) = results.get(path).and_then(|r| r.lines.get(*index)) else { return };
                // O texto da linha com cada trecho casado sobre o fundo de destaque.
                let units: Vec<u16> = line.preview.encode_utf16().collect();
                let slice = |from: usize, to: usize| String::from_utf16_lossy(&units[from.min(units.len())..to.min(units.len())]);
                let mut x = 0.0;
                let mut cursor = 0;
                let mut piece = |tree: &mut Tree, text: String, highlighted: bool| {
                    if text.is_empty() {
                        return;
                    }
                    let width = measure_text(&text, &font);
                    let node = tree.create(content, Role::Generic);
                    tree.set_bounds(node, Rect::new(x, 3.0, x + width, ROW_HEIGHT));
                    let style = tree.style_mut(node);
                    if highlighted {
                        style.background = Some(self.highlight);
                    }
                    style.content = Content::Text { text, style: font.clone(), color: self.foreground, line_height: 18.0 };
                    x += width;
                };
                for &(start, end) in &line.ranges {
                    if start < cursor {
                        continue;
                    }
                    piece(&mut tree, slice(cursor, start), false);
                    piece(&mut tree, slice(start, end), true);
                    cursor = end;
                }
                piece(&mut tree, slice(cursor, units.len()), false);
            }
        }
    }

    fn dispose_template(&self, _: &TemplateData) {}
}

pub struct SearchView {
    ui: Ui,
    tree: Rc<ObjectTree<Entry>>,
    results: Results,
    order: Vec<PathBuf>,
    collapsed: Rc<RefCell<std::collections::HashSet<PathBuf>>>,
    width: Rc<RefCell<f64>>,
    running: Option<Search>,
    last_query: Option<Query>,
    pub value: Vec<char>,
    pub cursor: usize,
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub select_all: bool,
    _listeners: Vec<DisposableRef>,
}

impl SearchView {
    pub fn new(ui: &Ui, container: NodeId, theme: &ColorTheme, icons: Option<FileIconTheme>, language_of: LanguageOf, queue: CommandQueue) -> Self {
        let color = |id: &str| theme.get_color(id).unwrap_or(Rgba::opaque(0xff, 0, 0xff));
        let results: Results = Rc::default();
        let width = Rc::new(RefCell::new(289.0));
        let renderer = Rc::new(Renderer {
            ui: ui.clone(),
            results: results.clone(),
            icons,
            language_of,
            foreground: color("sideBar.foreground"),
            highlight: color("editor.findMatchHighlightBackground"),
            badge_background: color("badge.background"),
            badge_foreground: color("badge.foreground"),
            width: width.clone(),
        });
        let mut options = ObjectTreeOptions::default();
        options.tree.update.indent = Some(8.0);
        options.tree.update.expand_only_on_twistie_click = Some(ExpandOnlyOnTwistieClick::Always(false));
        let tree = ObjectTree::new("search", ui, container, Rc::new(Delegate), vec![renderer as Rc<dyn ITreeRenderer<Model>>], options);
        let mut styles = default_list_styles(theme);
        styles.row_radius = 4.0;
        tree.style(&styles);
        let collapsed = Rc::new(RefCell::new(std::collections::HashSet::new()));
        let mut listeners = Vec::new();
        listeners.push(tree.on_did_change_collapse_state().listen({
            let collapsed = collapsed.clone();
            move |event| {
                if let Some(Entry::File(path)) = &event.node.element {
                    if event.node.collapsed.get() {
                        collapsed.borrow_mut().insert(path.clone());
                    } else {
                        collapsed.borrow_mut().remove(path);
                    }
                }
            }
        }));
        listeners.push(tree.on_did_change_selection().listen({
            let results = results.clone();
            move |event| {
                for entry in event.elements.iter().flatten() {
                    if let Entry::Match(path, index) = entry
                        && let Some(line) = results.borrow().get(path).and_then(|r| r.lines.get(*index))
                    {
                        let range = line.ranges.first().copied().unwrap_or((0, 0));
                        queue.borrow_mut().push(Command::OpenAt(path.clone(), line.line_number, line.column, line.column + (range.1 - range.0) as i32));
                    }
                }
            }
        }));
        Self { ui: ui.clone(), tree, results, order: Vec::new(), collapsed, width, running: None, last_query: None, value: Vec::new(), cursor: 0, match_case: false, whole_word: false, regex: false, select_all: false, _listeners: listeners }
    }

    pub fn restyle(&self, theme: &ColorTheme) {
        let mut styles = default_list_styles(theme);
        styles.row_radius = 4.0;
        self.tree.style(&styles);
    }

    pub fn text(&self) -> String {
        self.value.iter().collect()
    }

    pub fn query(&self) -> Query {
        Query { text: self.text(), match_case: self.match_case, whole_word: self.whole_word, regex: self.regex }
    }

    /// Começa (ou recomeça) a busca em `root` com o campo atual e esvazia os resultados.
    pub fn run(&mut self, root: Option<&Path>, wake: je_terminal::Wake) {
        self.running = None;
        self.results.borrow_mut().clear();
        self.order.clear();
        self.last_query = None;
        self.rebuild();
        let query = self.query();
        if query.text.is_empty() {
            return;
        }
        if let Some(root) = root {
            self.last_query = Some(query.clone());
            self.running = Some(Search::start(root, query, wake));
        }
    }

    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
        self.run(None, std::sync::Arc::new(|| {}));
    }

    /// Junta os resultados que chegaram da thread. Devolve se algo mudou.
    pub fn poll(&mut self) -> bool {
        let Some(search) = &self.running else { return false };
        let batch = search.poll();
        let done = search.is_done();
        if batch.is_empty() {
            if done {
                self.running = None;
                return true;
            }
            return false;
        }
        for result in batch {
            self.order.push(result.path.clone());
            self.results.borrow_mut().insert(result.path.clone(), result);
        }
        self.rebuild();
        if done {
            self.running = None;
        }
        true
    }

    fn rebuild(&self) {
        let results = self.results.borrow();
        let elements: Vec<IObjectTreeElement<Entry>> = self
            .order
            .iter()
            .filter_map(|path| results.get(path))
            .map(|file| {
                let mut element = IObjectTreeElement::new(Entry::File(file.path.clone()));
                element.collapsible = Some(true);
                element.collapsed = Some(CollapsedValue::Bool(self.collapsed.borrow().contains(&file.path)));
                element.children = Some((0..file.lines.len()).map(|i| IObjectTreeElement::new(Entry::Match(file.path.clone(), i))).collect());
                element
            })
            .collect();
        self.tree.set_children(&None, elements, ObjectTreeModelSetChildrenOptions::default());
    }

    pub fn collapse_all(&self) {
        for path in &self.order {
            self.collapsed.borrow_mut().insert(path.clone());
        }
        self.rebuild();
    }

    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// `4 results in 1 file` (ou a mensagem de nada achado).
    pub fn message(&self) -> Option<String> {
        let query = self.last_query.as_ref()?;
        let results = self.results.borrow();
        let (files, matches) = (results.len(), results.values().map(|r| r.match_count).sum::<usize>());
        if matches == 0 {
            return if self.running.is_some() { None } else { Some(format!("No results found for '{}'", query.text)) };
        }
        Some(format!("{matches} result{} in {files} file{}", if matches == 1 { "" } else { "s" }, if files == 1 { "" } else { "s" }))
    }

    pub fn layout(&self, width: f64, height: f64) {
        *self.width.borrow_mut() = width;
        self.tree.layout(Some(height), Some(width));
    }

    pub fn scroll_by(&self, dy: f64) {
        self.tree.set_scroll_top((self.tree.scroll_top() + dy).max(0.0));
    }

    /// O cabeçalho (campo e mensagem) como filhos de `parent`, na largura `width`; devolve os alvos de clique e a
    /// altura ocupada (onde a árvore começa).
    pub fn render_header(&self, tree: &mut Tree, parent: NodeId, width: f64, focused: bool, theme: &ColorTheme) -> (Vec<(NodeId, Command)>, f64) {
        let color = |id: &str| theme.get_color(id).unwrap_or(Rgba::opaque(0xff, 0, 0xff));
        let font = ui_font(13.0, 400);
        let foreground = color("sideBar.foreground");
        let mut hits = Vec::new();
        // O botão de expandir a substituição (ainda sem a linha de substituição).
        let toggle = tree.create(parent, Role::Button);
        tree.set_bounds(toggle, Rect::new(5.0, 39.0, 21.0, 63.0));
        tree.style_mut(toggle).content = Content::Codicon { id: "chevron-right".into(), size: 16.0, color: color("icon.foreground") };
        // O campo: de x = 21 até a borda direita, 26 px de altura.
        let input = tree.create(parent, Role::TextInput);
        tree.set_bounds(input, Rect::new(21.0, 39.0, width - 10.0, 65.0));
        hits.push((input, Command::SearchFocus));
        let style = tree.style_mut(input);
        style.background = Some(color("input.background"));
        style.border = Some(Border { width: 1.0, color: if focused { color("focusBorder") } else { color("input.border") } });
        style.radius = 4.0;
        let text = tree.create(input, Role::Generic);
        let inner = width - 10.0 - 21.0;
        tree.set_bounds(text, Rect::new(7.0, 1.0, inner - 70.0, 24.0));
        if self.value.is_empty() {
            tree.style_mut(text).content = Content::Text { text: "Search".to_owned(), style: font.clone(), color: color("input.placeholderForeground"), line_height: 24.0 };
        } else {
            tree.style_mut(text).content = Content::Text { text: self.text(), style: font.clone(), color: foreground, line_height: 24.0 };
        }
        if focused {
            if self.select_all && !self.value.is_empty() {
                let w = measure_text(&self.text(), &font).min(inner - 77.0);
                let selection = tree.create(input, Role::Generic);
                tree.set_bounds(selection, Rect::new(7.0, 4.0, 7.0 + w, 21.0));
                tree.style_mut(selection).background = Some(color("editor.selectionBackground"));
                tree.insert_before(input, text, None);
            }
            let before: String = self.value[..self.cursor].iter().collect();
            let x = 7.0 + measure_text(&before, &font).min(inner - 77.0);
            let caret = tree.create(input, Role::Generic);
            tree.set_bounds(caret, Rect::new(x, 5.0, x + 1.0, 20.0));
            tree.style_mut(caret).background = Some(foreground);
        }
        let toggles = [("case-sensitive", self.match_case, Command::SearchToggleCase), ("whole-word", self.whole_word, Command::SearchToggleWord), ("regex", self.regex, Command::SearchToggleRegex)];
        for (i, (icon, on, command)) in toggles.into_iter().enumerate() {
            let x = inner - 66.0 + 22.0 * i as f64;
            let node = tree.create(input, Role::Checkbox);
            tree.set_bounds(node, Rect::new(x, 2.0, x + 20.0, 22.0));
            let style = tree.style_mut(node);
            style.content = Content::Codicon { id: icon.into(), size: 16.0, color: if on { color("inputOption.activeForeground") } else { foreground } };
            style.radius = 3.0;
            style.cursor = Some(Cursor::Pointer);
            if on {
                style.background = Some(color("inputOption.activeBackground"));
                style.border = Some(Border { width: 1.0, color: color("inputOption.activeBorder") });
            }
            hits.push((node, command));
        }
        // O `...` dos filtros de arquivos (incluir e excluir), ainda sem os campos.
        let more = tree.create(parent, Role::Button);
        tree.set_bounds(more, Rect::new(width - 30.0, 68.0, width - 12.0, 80.0));
        tree.style_mut(more).content = Content::Codicon { id: "ellipsis".into(), size: 16.0, color: foreground };
        // A mensagem.
        if let Some(message) = self.message() {
            let muted = Rgba { a: (f32::from(foreground.a) * 0.65) as u8, ..foreground };
            let label = tree.create(parent, Role::Status);
            let w = measure_text(&message, &font);
            tree.set_bounds(label, Rect::new(23.0, 79.0, 23.0 + w, 105.0));
            tree.style_mut(label).content = Content::Text { text: message, style: font.clone(), color: muted, line_height: 26.0 };
            // `Open in editor` é um link da mensagem quando há resultados.
            if self.results.borrow().values().any(|r| r.match_count > 0) {
                let link = "Open in editor";
                let lx = 23.0 + w + measure_text(" - ", &font);
                let lw = measure_text(link, &font);
                let sep = tree.create(parent, Role::Generic);
                tree.set_bounds(sep, Rect::new(23.0 + w, 79.0, lx, 105.0));
                tree.style_mut(sep).content = Content::Text { text: " - ".to_owned(), style: font.clone(), color: muted, line_height: 26.0 };
                let node = tree.create(parent, Role::Link);
                tree.set_bounds(node, Rect::new(lx, 79.0, lx + lw, 105.0));
                let style = tree.style_mut(node);
                style.content = Content::Text { text: link.to_owned(), style: font, color: color("textLink.foreground"), line_height: 26.0 };
                style.cursor = Some(Cursor::Pointer);
            }
        }
        (hits, 105.0)
    }
}
