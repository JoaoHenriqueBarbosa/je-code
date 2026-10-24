//! A vista Source Control (`workbench/contrib/scm/browser/scmViewPane.ts` com a extensão `git`): o campo da
//! mensagem, o botão Commit e os grupos `Staged Changes` e `Changes`, cada arquivo com a letra de estado
//! (`M`, `A`, `D`, `U`) na cor de `gitDecoration.*`. As medidas são as do DOM do oráculo.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::DisposableRef;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Border, Content, Cursor, NodeId, Role, Ui};
use je_widgets::list::list_styles::default_list_styles;
use je_widgets::list::list_types::{IListVirtualDelegate, TemplateData};
use je_widgets::text::{ellipsize, measure_text, ui_font};
use je_widgets::tree::abstract_tree::ExpandOnlyOnTwistieClick;
use je_widgets::tree::index_tree_model::TreeNode;
use je_widgets::tree::object_tree::{ObjectTree, ObjectTreeOptions};
use je_widgets::tree::object_tree_model::{ObjectTreeModel, ObjectTreeModelSetChildrenOptions};
use je_widgets::tree::tree_types::{CollapsedValue, IObjectTreeElement, ITreeRenderer, TreeElementRenderDetails};

use super::Command;
use super::explorer::LanguageOf;
use super::file_icons::FileIconTheme;
use super::git::{Change, Status};
use super::menus::CommandQueue;

const ROW_HEIGHT: f64 = 22.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    Staged,
    Changes,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Entry {
    Group(Group),
    File(Group, String),
}

type Model = ObjectTreeModel<Entry>;

struct Delegate;

impl IListVirtualDelegate<Entry> for Delegate {
    fn get_height(&self, _: &Entry) -> f64 {
        ROW_HEIGHT
    }

    fn get_template_id(&self, _: &Entry) -> String {
        "scm".to_owned()
    }
}

struct Renderer {
    ui: Ui,
    status: Rc<RefCell<Status>>,
    icons: Option<FileIconTheme>,
    language_of: LanguageOf,
    theme: Rc<RefCell<ColorTheme>>,
    width: Rc<RefCell<f64>>,
}

impl Renderer {
    fn color(&self, id: &str) -> Rgba {
        self.theme.borrow().get_color(id).unwrap_or(Rgba::opaque(0xff, 0, 0xff))
    }
}

impl ITreeRenderer<Model> for Renderer {
    fn template_id(&self) -> String {
        "scm".to_owned()
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
        let foreground = self.color("sideBar.foreground");
        let status = self.status.borrow();
        // O conteúdo começa depois do recuo e do twistie (8 px por nível mais 22 px de twistie).
        let depth_indent = if matches!(entry, Entry::File(..)) { 16.0 } else { 8.0 };
        let width = *self.width.borrow() - depth_indent - 22.0;
        tree.set_bounds(content, Rect::new(0.0, 0.0, width, ROW_HEIGHT));
        match entry {
            Entry::Group(group) => {
                let (label, count) = match group {
                    Group::Staged => ("Staged Changes", status.staged.len()),
                    Group::Changes => ("Changes", status.changes.len()),
                };
                let w = measure_text(label, &font);
                let text = tree.create(content, Role::Generic);
                tree.set_bounds(text, Rect::new(0.0, 3.0, w, ROW_HEIGHT));
                tree.style_mut(text).content = Content::Text { text: label.to_owned(), style: font, color: foreground, line_height: 18.0 };
                let badge_font = ui_font(10.0, 400);
                let digits = count.to_string();
                let bw = (measure_text(&digits, &badge_font) + 10.0).max(19.0).round();
                let badge = tree.create(content, Role::Generic);
                tree.set_bounds(badge, Rect::new(width - bw - 4.0, 1.5, width - 4.0, 20.5));
                let style = tree.style_mut(badge);
                style.background = Some(self.color("badge.background"));
                style.radius = 9.5;
                let t = tree.create(badge, Role::Generic);
                tree.set_bounds(t, Rect::new((bw - measure_text(&digits, &badge_font)) / 2.0, 0.0, bw, 19.0));
                tree.style_mut(t).content = Content::Text { text: digits, style: badge_font, color: self.color("badge.foreground"), line_height: 19.0 };
            }
            Entry::File(group, path) => {
                let list = if *group == Group::Staged { &status.staged } else { &status.changes };
                let Some(change) = list.iter().find(|c| &c.path == path) else { return };
                let (dir, name) = path.rsplit_once('/').map_or(("", path.as_str()), |(d, n)| (d, n));
                if let Some(icons) = &self.icons {
                    icons.decorate(&mut tree, content, Rect::new(-12.0, 0.0, 0.0, ROW_HEIGHT), name, (self.language_of)(name).as_deref());
                }
                let deleted = change.kind == super::git::Kind::Deleted;
                let w = measure_text(name, &font);
                let label = tree.create(content, Role::Generic);
                tree.set_bounds(label, Rect::new(0.0, 3.0, w, ROW_HEIGHT));
                tree.style_mut(label).content = Content::Text { text: name.to_owned(), style: font.clone(), color: foreground, line_height: 18.0 };
                if deleted {
                    let strike = tree.create(content, Role::Separator);
                    tree.set_bounds(strike, Rect::new(0.0, 10.0, w, 11.0));
                    tree.style_mut(strike).background = Some(foreground);
                }
                // O diretório ao lado do nome, mais claro.
                let letter_width = 28.0;
                if !dir.is_empty() {
                    let muted = self.color("descriptionForeground");
                    let room = (width - w - 6.0 - letter_width).max(0.0);
                    let shown = ellipsize(dir, &font, room);
                    let dw = measure_text(&shown, &font);
                    let node = tree.create(content, Role::Generic);
                    tree.set_bounds(node, Rect::new(w + 6.0, 3.0, w + 6.0 + dw, ROW_HEIGHT));
                    tree.style_mut(node).content = Content::Text { text: shown, style: font.clone(), color: muted, line_height: 18.0 };
                }
                // A letra de estado à direita, na cor da decoração.
                let letter = tree.create(content, Role::Generic);
                tree.set_bounds(letter, Rect::new(width - 18.0, 3.0, width - 4.0, ROW_HEIGHT));
                tree.style_mut(letter).content = Content::Text { text: change.kind.letter().to_owned(), style: ui_font(12.0, 600), color: self.color(change.kind.color_id()), line_height: 18.0 };
            }
        }
    }

    fn dispose_template(&self, _: &TemplateData) {}
}

pub struct ScmView {
    tree: Rc<ObjectTree<Entry>>,
    pub status: Rc<RefCell<Status>>,
    theme: Rc<RefCell<ColorTheme>>,
    collapsed: Rc<RefCell<std::collections::HashSet<Group>>>,
    width: Rc<RefCell<f64>>,
    pub message: Vec<char>,
    pub cursor: usize,
    _listeners: Vec<DisposableRef>,
}

impl ScmView {
    pub fn new(ui: &Ui, container: NodeId, theme: &ColorTheme, icons: Option<FileIconTheme>, language_of: LanguageOf, queue: CommandQueue) -> Self {
        let status = Rc::new(RefCell::new(Status::default()));
        let theme_cell = Rc::new(RefCell::new(theme.clone()));
        let width = Rc::new(RefCell::new(289.0));
        let renderer = Rc::new(Renderer { ui: ui.clone(), status: status.clone(), icons, language_of, theme: theme_cell.clone(), width: width.clone() });
        let mut options = ObjectTreeOptions::default();
        options.tree.update.indent = Some(8.0);
        options.tree.update.expand_only_on_twistie_click = Some(ExpandOnlyOnTwistieClick::Always(false));
        let tree = ObjectTree::new("scm", ui, container, Rc::new(Delegate), vec![renderer as Rc<dyn ITreeRenderer<Model>>], options);
        let mut styles = default_list_styles(theme);
        styles.row_radius = 4.0;
        tree.style(&styles);
        let collapsed = Rc::new(RefCell::new(std::collections::HashSet::new()));
        let mut listeners = Vec::new();
        listeners.push(tree.on_did_change_collapse_state().listen({
            let collapsed = collapsed.clone();
            move |event| {
                if let Some(Entry::Group(group)) = &event.node.element {
                    if event.node.collapsed.get() {
                        collapsed.borrow_mut().insert(*group);
                    } else {
                        collapsed.borrow_mut().remove(group);
                    }
                }
            }
        }));
        // Selecionar um arquivo o abre; o botão direito pede o menu do item.
        listeners.push(tree.on_did_change_selection().listen({
            let queue = queue.clone();
            move |event| {
                for entry in event.elements.iter().flatten() {
                    if let Entry::File(_, path) = entry {
                        queue.borrow_mut().push(Command::ScmOpen(path.clone()));
                    }
                }
            }
        }));
        listeners.push(tree.on_context_menu().listen({
            let weak = Rc::downgrade(&tree);
            move |event| {
                if let Some(Some(entry)) = &event.element {
                    if let Some(tree) = weak.upgrade() {
                        tree.set_focus(&[Some(entry.clone())], None);
                    }
                    queue.borrow_mut().push(Command::ScmContextMenu(entry.clone()));
                }
            }
        }));
        Self { tree, status, theme: theme_cell, collapsed, width, message: Vec::new(), cursor: 0, _listeners: listeners }
    }

    pub fn restyle(&self, theme: &ColorTheme) {
        *self.theme.borrow_mut() = theme.clone();
        let mut styles = default_list_styles(theme);
        styles.row_radius = 4.0;
        self.tree.style(&styles);
        self.rebuild();
    }

    pub fn text(&self) -> String {
        self.message.iter().collect()
    }

    pub fn set_status(&self, status: Status) {
        *self.status.borrow_mut() = status;
        self.rebuild();
    }

    fn rebuild(&self) {
        let status = self.status.borrow();
        let mut groups = Vec::new();
        for (group, list) in [(Group::Staged, &status.staged), (Group::Changes, &status.changes)] {
            if list.is_empty() {
                continue;
            }
            let mut element = IObjectTreeElement::new(Entry::Group(group));
            element.collapsible = Some(true);
            element.collapsed = Some(CollapsedValue::Bool(self.collapsed.borrow().contains(&group)));
            element.children = Some(list.iter().map(|c| IObjectTreeElement::new(Entry::File(group, c.path.clone()))).collect());
            groups.push(element);
        }
        drop(status);
        self.tree.set_children(&None, groups, ObjectTreeModelSetChildrenOptions::default());
    }

    pub fn changes_of(&self, group: Group) -> Vec<Change> {
        let status = self.status.borrow();
        if group == Group::Staged { status.staged.clone() } else { status.changes.clone() }
    }

    pub fn layout(&self, width: f64, height: f64) {
        *self.width.borrow_mut() = width;
        self.tree.layout(Some(height), Some(width));
    }

    pub fn scroll_by(&self, dy: f64) {
        self.tree.set_scroll_top((self.tree.scroll_top() + dy).max(0.0));
    }

    /// O cabeçalho: a linha `Changes` do repositório com as ações, o campo da mensagem e o botão Commit. Devolve os
    /// alvos de clique e onde a árvore começa (relativo à barra lateral).
    pub fn render_header(&self, tree: &mut je_ui::Tree, parent: NodeId, width: f64, focused: bool, branch: &str) -> (Vec<(NodeId, Command)>, f64) {
        let theme = self.theme.borrow().clone();
        let color = |id: &str| theme.get_color(id).unwrap_or(Rgba::opaque(0xff, 0, 0xff));
        let font = ui_font(13.0, 400);
        let fg = color("sideBarSectionHeader.foreground");
        let mut hits = Vec::new();
        // A linha do repositório (`.pane-header`): seta, rótulo em negrito e as ações à direita.
        let header = tree.create(parent, Role::Button);
        tree.set_bounds(header, Rect::new(5.0, 33.0, width - 6.0, 61.0));
        let chevron = tree.create(header, Role::Generic);
        tree.set_bounds(chevron, Rect::new(2.0, 6.0, 18.0, 22.0));
        tree.style_mut(chevron).content = Content::Codicon { id: "chevron-down".into(), size: 16.0, color: fg };
        let label = tree.create(header, Role::Generic);
        tree.set_bounds(label, Rect::new(20.0, 0.0, 120.0, 28.0));
        tree.style_mut(label).content = Content::Text { text: "Changes".to_owned(), style: ui_font(11.0, 700), color: fg, line_height: 28.0 };
        for (k, (icon, command)) in [("check", Some(Command::ScmCommit)), ("refresh", Some(Command::ScmRefresh)), ("ellipsis", Some(Command::ScmMoreMenu))].into_iter().enumerate() {
            let x = width - 27.0 - 24.0 * (2 - k) as f64;
            let button = tree.create(parent, Role::Button);
            tree.set_bounds(button, Rect::new(x, 36.0, x + 22.0, 58.0));
            let style = tree.style_mut(button);
            style.content = Content::Codicon { id: icon.into(), size: 16.0, color: fg };
            style.radius = 5.0;
            if let Some(command) = command {
                style.cursor = Some(Cursor::Pointer);
                hits.push((button, command));
            }
        }
        // O campo da mensagem: x = 24 até a borda, 26 px de altura.
        let input = tree.create(parent, Role::TextInput);
        tree.set_bounds(input, Rect::new(24.0, 65.0, width - 10.0, 91.0));
        hits.push((input, Command::ScmFocus));
        let style = tree.style_mut(input);
        style.background = Some(color("input.background"));
        style.border = Some(Border { width: 1.0, color: if focused { color("focusBorder") } else { color("input.border") } });
        style.radius = 4.0;
        let inner = width - 10.0 - 24.0;
        let text = tree.create(input, Role::Generic);
        tree.set_bounds(text, Rect::new(7.0, 1.0, inner - 7.0, 24.0));
        if self.message.is_empty() {
            let placeholder = ellipsize(&format!("Message (Ctrl+Enter to commit on '{branch}')"), &font, inner - 16.0);
            tree.style_mut(text).content = Content::Text { text: placeholder, style: font.clone(), color: color("input.placeholderForeground"), line_height: 24.0 };
        } else {
            let shown = ellipsize(&self.text(), &font, inner - 16.0);
            tree.style_mut(text).content = Content::Text { text: shown, style: font.clone(), color: fg, line_height: 24.0 };
        }
        if focused {
            let before: String = self.message[..self.cursor].iter().collect();
            let x = 7.0 + measure_text(&before, &font).min(inner - 16.0);
            let caret = tree.create(input, Role::Generic);
            tree.set_bounds(caret, Rect::new(x, 5.0, x + 1.0, 20.0));
            tree.style_mut(caret).background = Some(fg);
        }
        // O botão Commit com a seta de opções à direita.
        let button = tree.create(parent, Role::Button);
        tree.set_bounds(button, Rect::new(24.0, 100.0, width - 10.0, 125.0));
        let style = tree.style_mut(button);
        style.background = Some(color("button.background"));
        style.radius = 4.0;
        style.cursor = Some(Cursor::Pointer);
        hits.push((button, Command::ScmCommit));
        let bw = width - 10.0 - 24.0;
        let label = "Commit";
        let lw = measure_text(label, &font);
        let start = (bw - 24.0 - (lw + 22.0)) / 2.0;
        let icon = tree.create(button, Role::Image);
        tree.set_bounds(icon, Rect::new(start, 4.0, start + 16.0, 20.0));
        tree.style_mut(icon).content = Content::Codicon { id: "check".into(), size: 16.0, color: color("button.foreground") };
        let t = tree.create(button, Role::Generic);
        tree.set_bounds(t, Rect::new(start + 22.0, 0.0, start + 22.0 + lw, 25.0));
        tree.style_mut(t).content = Content::Text { text: label.to_owned(), style: font, color: color("button.foreground"), line_height: 25.0 };
        let sep = tree.create(button, Role::Separator);
        tree.set_bounds(sep, Rect::new(bw - 25.0, 4.0, bw - 24.0, 21.0));
        tree.style_mut(sep).background = Some(color("button.separator"));
        let arrow = tree.create(button, Role::Image);
        tree.set_bounds(arrow, Rect::new(bw - 22.0, 4.0, bw - 6.0, 20.0));
        tree.style_mut(arrow).content = Content::Codicon { id: "chevron-down".into(), size: 16.0, color: color("button.foreground") };
        (hits, 125.0)
    }
}
