//! O explorador de arquivos (`vs/workbench/contrib/files/browser/views/explorerView.ts`) sobre a `ObjectTree` do
//! `je-widgets`: a pasta do workspace com as pastas primeiro e os arquivos depois, em ordem de nome sem caixa
//! (o `explorer.sortOrder: default`), filhos lidos do disco quando a pasta abre, e o `files.exclude` padrão.
//! Selecionar um arquivo abre o arquivo no editor (o `explorer.openEditors` de clique único).

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use je_base::lifecycle::DisposableRef;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Content, NodeId, PointerButton, Role, Ui};
use je_widgets::list::list_styles::default_list_styles;
use je_widgets::list::list_types::{IListVirtualDelegate, TemplateData};
use je_widgets::text::ui_font;
use je_widgets::tree::index_tree_model::TreeNode;
use je_widgets::tree::abstract_tree::ExpandOnlyOnTwistieClick;
use je_widgets::tree::object_tree::{ObjectTree, ObjectTreeOptions};
use je_widgets::tree::object_tree_model::{ObjectTreeModel, ObjectTreeModelSetChildrenOptions};
use je_widgets::tree::tree_types::{CollapsedValue, IObjectTreeElement, ITreeRenderer, TreeElementRenderDetails};

use super::Command;
use super::file_icons::FileIconTheme;
use super::menus::CommandQueue;

type Model = ObjectTreeModel<PathBuf>;

/// A altura das linhas do explorador (`ExplorerDelegate.ITEM_HEIGHT`).
const ITEM_HEIGHT: f64 = 22.0;
/// Onde o nome começa depois do ícone do arquivo.
const LABEL_LEFT: f64 = 22.0;
/// O `files.exclude` padrão.
const EXCLUDED: [&str; 6] = [".git", ".svn", ".hg", "CVS", ".DS_Store", "Thumbs.db"];

struct Delegate;

impl IListVirtualDelegate<PathBuf> for Delegate {
    fn get_height(&self, _: &PathBuf) -> f64 {
        ITEM_HEIGHT
    }

    fn get_template_id(&self, _: &PathBuf) -> String {
        "file".to_owned()
    }
}

/// O id de linguagem de um nome de arquivo, para o ícone (o `getIconClasses` consulta o serviço de linguagens).
pub type LanguageOf = Rc<dyn Fn(&str) -> Option<String>>;

/// O `FilesRenderer`: ícone e nome do arquivo (o `ResourceLabel`).
struct Renderer {
    ui: Ui,
    icons: Option<FileIconTheme>,
    language_of: LanguageOf,
    foreground: je_render::Rgba,
}

/// Os nós de uma linha: o contêiner do ícone e o do nome.
struct Template {
    icon: NodeId,
    label: NodeId,
}

impl ITreeRenderer<Model> for Renderer {
    fn template_id(&self) -> String {
        "file".to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let mut tree = self.ui.tree_mut();
        let icon = tree.create(container, Role::Generic);
        tree.set_bounds(icon, Rect::new(0.0, 0.0, 16.0, ITEM_HEIGHT));
        let label = tree.create(container, Role::Generic);
        tree.set_bounds(label, Rect::new(LABEL_LEFT, 0.0, 400.0, ITEM_HEIGHT));
        Rc::new(Template { icon, label }) as Rc<dyn Any>
    }

    fn render_element(&self, node: &Rc<TreeNode<Option<PathBuf>>>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let Some(template) = template_data.downcast_ref::<Template>() else { return };
        let Some(path) = &node.element else { return };
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let mut tree = self.ui.tree_mut();
        for child in tree.children(template.icon).to_vec() {
            tree.remove(child);
        }
        // O ícone (`::before` de 16 px com 6 px de `padding-right`); as pastas não têm ícone no Seti e o nome
        // começa onde o ícone começaria.
        let folder = node.collapsible.get();
        if !folder && let Some(icons) = &self.icons {
            icons.decorate(&mut tree, template.icon, Rect::new(0.0, 0.0, 16.0, ITEM_HEIGHT), &name, (self.language_of)(&name).as_deref());
        }
        let left = if folder { 0.0 } else { LABEL_LEFT };
        tree.set_bounds(template.label, Rect::new(left, 0.0, left + 400.0, ITEM_HEIGHT));
        tree.set_label(template.label, name.clone());
        tree.style_mut(template.label).content = Content::Text { text: name, style: ui_font(13.0, 400), color: self.foreground, line_height: ITEM_HEIGHT };
    }

    fn dispose_template(&self, _: &TemplateData) {}
}

pub struct Explorer {
    tree: Rc<ObjectTree<PathBuf>>,
    root: PathBuf,
    /// As pastas cujos filhos já foram lidos do disco.
    loaded: Rc<RefCell<HashSet<PathBuf>>>,
    /// As pastas abertas (o estado sobrevive ao `refresh`).
    expanded: Rc<RefCell<HashSet<PathBuf>>>,
    /// Os caminhos que estão na árvore (o `ObjectTreeModel` não expõe `has`).
    known: Rc<RefCell<HashSet<PathBuf>>>,
    last_listing: RefCell<Vec<(PathBuf, Vec<String>)>>,
    _listeners: Vec<DisposableRef>,
}

/// As entradas de `dir` como elementos da árvore: pastas primeiro, depois arquivos, por nome sem caixa. As pastas
/// em `expanded` já vêm abertas com os filhos lidos (recursivamente).
fn read_children(dir: &Path, known: &RefCell<HashSet<PathBuf>>, expanded: &RefCell<HashSet<PathBuf>>, loaded: &RefCell<HashSet<PathBuf>>) -> Vec<IObjectTreeElement<PathBuf>> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut entries: Vec<(bool, String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (!EXCLUDED.contains(&name.as_str())).then(|| (e.path().is_dir(), name, e.path()))
        })
        .collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    entries
        .into_iter()
        .map(|(is_dir, _, path)| {
            known.borrow_mut().insert(path.clone());
            let mut element = IObjectTreeElement::new(path.clone());
            if is_dir {
                element.collapsible = Some(true);
                if expanded.borrow().contains(&path) {
                    loaded.borrow_mut().insert(path.clone());
                    element.collapsed = Some(CollapsedValue::Bool(false));
                    element.children = Some(read_children(&path, known, expanded, loaded));
                } else {
                    element.collapsed = Some(CollapsedValue::Bool(true));
                }
            }
            element
        })
        .collect()
}

impl Explorer {
    pub fn new(ui: &Ui, container: NodeId, root: &Path, theme: &ColorTheme, icons: Option<FileIconTheme>, language_of: LanguageOf, queue: CommandQueue) -> Self {
        let foreground = theme.get_color("sideBar.foreground").unwrap_or(je_render::Rgba::opaque(0xcc, 0xcc, 0xcc));
        if icons.is_some() {
            let mut tree = ui.tree_mut();
            tree.add_class(container, "file-icon-themable-tree");
            tree.add_class(container, "align-icons-and-twisties");
        }
        let mut options = ObjectTreeOptions::default();
        options.tree.update.indent = Some(8.0);
        // `workbench.tree.expandMode: singleClick` (o `WorkbenchObjectTree` traduz em `expandOnlyOnTwistieClick`).
        options.tree.update.expand_only_on_twistie_click = Some(ExpandOnlyOnTwistieClick::Always(false));
        let renderer = Rc::new(Renderer { ui: ui.clone(), icons, language_of, foreground });
        let tree = ObjectTree::new("explorer", ui, container, Rc::new(Delegate), vec![renderer as Rc<dyn ITreeRenderer<Model>>], options);
        // O raio de 4 px das linhas no tema 2026 (`.monaco-list-row` do workbench).
        let mut styles = default_list_styles(theme);
        styles.row_radius = 4.0;
        tree.style(&styles);
        let loaded = Rc::new(RefCell::new(HashSet::from([root.to_path_buf()])));
        let expanded = Rc::new(RefCell::new(HashSet::new()));
        let known = Rc::new(RefCell::new(HashSet::new()));
        tree.set_children(&None, read_children(root, &known, &expanded, &loaded), ObjectTreeModelSetChildrenOptions::default());
        let mut listeners = Vec::new();
        // Abrir uma pasta lê os filhos dela na primeira vez; o estado aberto fica guardado.
        listeners.push(tree.on_did_change_collapse_state().listen({
            let (weak, loaded, known, expanded) = (Rc::downgrade(&tree), loaded.clone(), known.clone(), expanded.clone());
            move |event| {
                let node = &event.node;
                let (Some(tree), Some(path)) = (weak.upgrade(), node.element.clone()) else { return };
                if node.collapsed.get() {
                    expanded.borrow_mut().remove(&path);
                    return;
                }
                expanded.borrow_mut().insert(path.clone());
                if !loaded.borrow_mut().insert(path.clone()) {
                    return;
                }
                let children = read_children(&path, &known, &expanded, &loaded);
                tree.set_children(&Some(path), children, ObjectTreeModelSetChildrenOptions::default());
            }
        }));
        // Selecionar um arquivo abre ele; o botão direito pede o menu de contexto do item.
        listeners.push(tree.on_did_change_selection().listen({
            let queue = queue.clone();
            move |event| {
                // A seleção por clique direito não abre o arquivo: quem pede o menu cuida do resto.
                let secondary = event.browser_event.as_ref().is_some_and(|b| b.button == PointerButton::Secondary);
                if secondary {
                    return;
                }
                for path in event.elements.iter().flatten() {
                    if path.is_file() {
                        queue.borrow_mut().push(Command::OpenFile(path.clone()));
                    }
                }
            }
        }));
        listeners.push(tree.on_context_menu().listen({
            let weak = Rc::downgrade(&tree);
            move |event| {
                if let Some(Some(path)) = &event.element {
                    // O botão direito põe o foco na linha (sem abrir o arquivo) e pede o menu.
                    if let Some(tree) = weak.upgrade() {
                        tree.set_focus(&[Some(path.clone())], None);
                    }
                    queue.borrow_mut().push(Command::ExplorerContextMenu(path.clone()));
                }
            }
        }));
        let explorer = Self { tree, root: root.to_path_buf(), loaded, expanded, known, last_listing: RefCell::new(Vec::new()), _listeners: listeners };
        *explorer.last_listing.borrow_mut() = explorer.listing();
        explorer
    }

    #[cfg(test)]
    pub fn contains(&self, path: &Path) -> bool {
        self.known.borrow().contains(path)
    }

    /// Relê a árvore se alguma pasta aberta mudou no disco (entrada criada, apagada ou renomeada).
    pub fn refresh_if_changed(&self) {
        let snapshot = self.listing();
        if *self.last_listing.borrow() != snapshot {
            *self.last_listing.borrow_mut() = snapshot;
            self.refresh();
        }
    }

    /// Os nomes das entradas de cada pasta visível (a raiz e as abertas), para notar mudanças.
    fn listing(&self) -> Vec<(PathBuf, Vec<String>)> {
        let mut dirs: Vec<PathBuf> = std::iter::once(self.root.clone()).chain(self.expanded.borrow().iter().cloned()).collect();
        dirs.sort();
        dirs.into_iter()
            .map(|dir| {
                let mut names: Vec<String> = std::fs::read_dir(&dir).map(|r| r.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
                names.sort();
                (dir, names)
            })
            .collect()
    }

    /// Relê o disco mantendo as pastas abertas (depois de criar, renomear ou apagar).
    pub fn refresh(&self) {
        self.loaded.borrow_mut().clear();
        self.loaded.borrow_mut().insert(self.root.clone());
        self.known.borrow_mut().clear();
        self.tree.set_children(&None, read_children(&self.root, &self.known, &self.expanded, &self.loaded), ObjectTreeModelSetChildrenOptions::default());
    }

    /// Abre a pasta (e as de cima) para o caminho aparecer, relendo o que faltar.
    pub fn expand_to(&self, path: &Path) {
        let mut parents: Vec<PathBuf> = path.ancestors().skip(1).take_while(|p| *p != self.root.as_path() && p.starts_with(&self.root)).map(Path::to_path_buf).collect();
        parents.reverse();
        let mut changed = false;
        for dir in parents {
            changed |= self.expanded.borrow_mut().insert(dir);
        }
        if changed {
            self.refresh();
        }
    }

    /// Os arquivos selecionados na árvore.
    #[cfg(test)]
    pub fn selection(&self) -> Vec<PathBuf> {
        self.tree.get_selection().into_iter().flatten().collect()
    }

    /// Reaplica as cores do tema às linhas.
    pub fn restyle(&self, theme: &ColorTheme) {
        let mut styles = default_list_styles(theme);
        styles.row_radius = 4.0;
        self.tree.style(&styles);
    }

    pub fn layout(&self, width: f64, height: f64) {
        self.tree.layout(Some(height), Some(width));
    }

    pub fn scroll_by(&self, dy: f64) {
        self.tree.set_scroll_top((self.tree.scroll_top() + dy).max(0.0));
    }

    /// Mostra o arquivo do editor ativo selecionado (o `explorer.autoReveal`), se ele já está na árvore.
    pub fn reveal(&self, path: Option<&Path>) {
        let Some(path) = path else { return };
        if !path.starts_with(&self.root) {
            return;
        }
        if !self.known.borrow().contains(path) {
            self.expand_to(path);
        }
        if !self.known.borrow().contains(path) {
            return;
        }
        let location = Some(path.to_path_buf());
        if self.tree.get_selection().first() != Some(&location) {
            self.tree.set_focus(std::slice::from_ref(&location), None);
            self.tree.set_selection(std::slice::from_ref(&location), None);
        }
        let _ = &self.loaded;
    }
}
