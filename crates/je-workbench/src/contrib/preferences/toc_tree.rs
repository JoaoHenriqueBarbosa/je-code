//! Porte parcial de `src/vs/workbench/contrib/preferences/browser/tocTree.ts`: o modelo do sumário (`TOCTreeModel`, que
//! conta as configurações visíveis de cada grupo) e o iterador que alimenta a árvore (`createTOCIterator`). O
//! `TOCRenderer`, o delegado de alturas, o provedor de acessibilidade e o `TOCTree` (um `WorkbenchObjectTree`) são widgets
//! e entram com a árvore de widgets nativa.
//!
//! Divergências:
//! - **`@IWorkbenchEnvironmentService`** do construtor é o `Rc<dyn ...>` passado em [`TocTreeModel::new`].
//! - **`settingsTreeRoot`** (`!` no original, lido antes de atribuído apenas por erro) é uma `Option`; o getter falha como o
//!   original falharia ao ler `children` de `undefined`. **`currentSearchModel`** é um `Option<Rc<SearchResultModel>>`.
//! - **`createTOCIterator(model, tree)`** recebe os filhos do modelo ou do grupo (`model.children`) no lugar do modelo, e
//!   o `tree`, que o original nunca lê, não existe.

use std::cell::RefCell;
use std::rc::Rc;

use je_widgets::tree::tree_types::ITreeElement;

use crate::contrib::preferences::settings_tree_models::{SearchResultModel, SettingsTreeGroupChild, SettingsTreeGroupElement, ViewStateRef};
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;

/// `TOCTreeModel`.
pub struct TocTreeModel {
    view_state: ViewStateRef,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    current_search_model: RefCell<Option<Rc<SearchResultModel>>>,
    settings_tree_root: RefCell<Option<Rc<SettingsTreeGroupElement>>>,
}

impl TocTreeModel {
    pub fn new(view_state: ViewStateRef, environment_service: Rc<dyn IWorkbenchEnvironmentService>) -> Rc<Self> {
        Rc::new(Self { view_state, environment_service, current_search_model: RefCell::new(None), settings_tree_root: RefCell::new(None) })
    }

    /// `get settingsTreeRoot()`.
    pub fn settings_tree_root(&self) -> Rc<SettingsTreeGroupElement> {
        self.settings_tree_root.borrow().clone().expect("o sumário ainda não recebeu a raiz da árvore de configurações")
    }

    /// `set settingsTreeRoot(value)`.
    pub fn set_settings_tree_root(&self, value: Rc<SettingsTreeGroupElement>) {
        *self.settings_tree_root.borrow_mut() = Some(value);
        self.update();
    }

    /// `get currentSearchModel()`.
    pub fn current_search_model(&self) -> Option<Rc<SearchResultModel>> {
        self.current_search_model.borrow().clone()
    }

    /// `set currentSearchModel(model)`.
    pub fn set_current_search_model(&self, model: Option<Rc<SearchResultModel>>) {
        *self.current_search_model.borrow_mut() = model;
        self.update();
    }

    /// `get children()`.
    pub fn children(&self) -> Vec<SettingsTreeGroupChild> {
        self.settings_tree_root().children()
    }

    pub fn update(&self) {
        let root = self.settings_tree_root.borrow().clone();
        if let Some(root) = root {
            self.update_group_count(&root);
        }
    }

    fn update_group_count(&self, group: &Rc<SettingsTreeGroupElement>) {
        let children = group.children();
        for child in &children {
            if let Some(child) = child.as_group() {
                self.update_group_count(child);
            }
        }

        let child_count: usize = children.iter().filter_map(SettingsTreeGroupChild::as_group).map(|child| child.count().unwrap_or(0)).sum();

        group.set_count(Some(child_count + self.get_group_count(group)));
    }

    fn get_group_count(&self, group: &Rc<SettingsTreeGroupElement>) -> usize {
        let current_search_model = self.current_search_model();
        // Check everything that the SettingsFilter checks except whether it's filtered by a category
        let is_remote = self.environment_service.remote_authority().is_some_and(|authority| !authority.is_empty());
        let view_state = self.view_state.borrow();
        group
            .children()
            .iter()
            .filter(|child| {
                let Some(child) = child.as_setting() else {
                    return false;
                };

                if current_search_model.as_ref().is_some_and(|search_model| !search_model.root().contains_setting(&child.setting.key)) {
                    return false;
                }

                child.matches_scope(&view_state.settings_target, is_remote)
                    && child.matches_all_tags(view_state.tag_filters.as_ref())
                    && child.matches_any_feature(view_state.feature_filters.as_ref())
                    && child.matches_any_extension(view_state.extension_filters.as_ref())
                    && child.matches_any_id(view_state.id_filters.as_ref())
            })
            .count()
    }
}

/// `createTOCIterator`: os grupos entre os filhos, cada um com os seus (`collapsible` quando tem subgrupos).
pub fn create_toc_iterator(children: &[SettingsTreeGroupChild]) -> Vec<ITreeElement<Rc<SettingsTreeGroupElement>>> {
    children
        .iter()
        .filter_map(SettingsTreeGroupChild::as_group)
        .map(|group| {
            let group_children = group.children();
            let has_group_children = group_children.iter().any(|child| child.as_group().is_some());

            ITreeElement { element: group.clone(), collapsed: None, collapsible: Some(has_group_children), children: Some(create_toc_iterator(&group_children)) }
        })
        .collect()
}
