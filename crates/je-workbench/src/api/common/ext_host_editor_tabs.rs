//! Porte de `src/vs/workbench/api/common/extHostEditorTabs.ts`: o lado da extensão do modelo de abas
//! (`vscode.window.tabGroups`).
//!
//! Divergências:
//! - Os objetos `vscode.Tab` e `vscode.TabGroup` do original são fachadas congeladas (literais com getters) que
//!   escondem as classes internas. Aqui [`Tab`] e [`TabGroup`] são o próprio `Rc` de [`ExtHostEditorTab`] e
//!   [`ExtHostEditorTabGroup`], cujo estado interno é privado ao módulo, o que dá a mesma garantia (não há
//!   escrita de fora). A identidade que o original mantém estável (`apiObject`) é a do `Rc` ([`Rc::ptr_eq`]);
//!   por isso `_findExtHostTabFromApi` e `_findExtHostTabGroupFromApi` comparam ponteiros.
//! - `vscode.TabGroups` é [`TabGroups`], com um `Weak` para o hospedeiro (o original guarda o `this` num
//!   fecho). Os `get all()` e `get tabs()` devolvem `Vec` por valor (o `Object.freeze` do original é a
//!   imutabilidade do `Vec` devolvido). Onde o original lança (`assertReturnsDefined`, índices inválidos) o
//!   porte levanta pânico com o [`Error`] (`errors::throw`).
//! - `tabGroups.close(tabOrTabGroup | tabs[] | groups[], preserveFocus)` recebe o enum [`CloseTarget`], porque a
//!   distinção que o original faz (`isTabGroup`, por `tabs !== undefined`) é de tipo aqui.
//! - As classes de entrada de aba de `extHostTypes.ts` (`TextTabInput` e as demais) ficam neste arquivo, com o
//!   union `AnyTabInput` como o enum [`AnyTabInput`]; `URI.revive` é a identidade, porque os DTOs já carregam [`URI`].
//! - `splice` do JavaScript tolera índice maior que o tamanho (insere no fim); o porte repete o `min` com o tamanho.
//! - `groupId` é `u32` e o `_activeGroupId` (inicializado por RPC) é um `Option<u32>` compartilhado com os grupos.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::collections::diff_sets;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::promise::Promise;
use je_base::uri::URI;

use super::ext_host_protocol::{
    AnyInputDto, EditorTabDto, EditorTabGroupDto, ExtHostEditorTabsShape, IMainContext, MainContext, MainThreadEditorTabsShape, TabModelOperationKind,
    TabOperation,
};
use super::ext_host_type_converters::ViewColumn;

// #region Tab inputs (de extHostTypes.ts)

/// `TextTabInput`.
#[derive(Clone, Debug)]
pub struct TextTabInput {
    pub uri: URI,
}

/// `TextDiffTabInput`.
#[derive(Clone, Debug)]
pub struct TextDiffTabInput {
    pub original: URI,
    pub modified: URI,
}

/// `TextMergeTabInput`.
#[derive(Clone, Debug)]
pub struct TextMergeTabInput {
    pub base: URI,
    pub input1: URI,
    pub input2: URI,
    pub result: URI,
}

/// `CustomEditorTabInput`.
#[derive(Clone, Debug)]
pub struct CustomEditorTabInput {
    pub uri: URI,
    pub view_type: String,
}

/// `WebviewEditorTabInput`.
#[derive(Clone, Debug)]
pub struct WebviewEditorTabInput {
    pub view_type: String,
}

/// `NotebookEditorTabInput`.
#[derive(Clone, Debug)]
pub struct NotebookEditorTabInput {
    pub uri: URI,
    pub notebook_type: String,
}

/// `NotebookDiffEditorTabInput`.
#[derive(Clone, Debug)]
pub struct NotebookDiffEditorTabInput {
    pub original: URI,
    pub modified: URI,
    pub notebook_type: String,
}

/// `TerminalEditorTabInput`.
#[derive(Clone, Debug, Default)]
pub struct TerminalEditorTabInput;

/// `InteractiveWindowInput`.
#[derive(Clone, Debug)]
pub struct InteractiveWindowInput {
    pub uri: URI,
    pub input_box_uri: URI,
}

/// `ChatEditorTabInput`.
#[derive(Clone, Debug, Default)]
pub struct ChatEditorTabInput;

/// `TextMultiDiffTabInput`.
#[derive(Clone, Debug)]
pub struct TextMultiDiffTabInput {
    pub text_diffs: Vec<TextDiffTabInput>,
}

// #endregion

/// `AnyTabInput`.
#[derive(Clone, Debug)]
pub enum AnyTabInput {
    Text(TextTabInput),
    TextDiff(TextDiffTabInput),
    TextMultiDiff(TextMultiDiffTabInput),
    Custom(CustomEditorTabInput),
    Notebook(NotebookEditorTabInput),
    NotebookDiff(NotebookDiffEditorTabInput),
    Webview(WebviewEditorTabInput),
    Terminal(TerminalEditorTabInput),
    Interactive(InteractiveWindowInput),
    Chat(ChatEditorTabInput),
    TextMerge(TextMergeTabInput),
}

/// `vscode.Tab`.
pub type Tab = Rc<ExtHostEditorTab>;

/// `vscode.TabGroup`.
pub type TabGroup = Rc<ExtHostEditorTabGroup>;

/// `vscode.TabChangeEvent`.
#[derive(Clone)]
pub struct TabChangeEvent {
    pub opened: Vec<Tab>,
    pub closed: Vec<Tab>,
    pub changed: Vec<Tab>,
}

/// `vscode.TabGroupChangeEvent`.
#[derive(Clone)]
pub struct TabGroupChangeEvent {
    pub opened: Vec<TabGroup>,
    pub closed: Vec<TabGroup>,
    pub changed: Vec<TabGroup>,
}

/// O argumento de `tabGroups.close`: `Tab | readonly Tab[] | TabGroup | readonly TabGroup[]`.
pub enum CloseTarget {
    Tab(Tab),
    Tabs(Vec<Tab>),
    TabGroup(TabGroup),
    TabGroups(Vec<TabGroup>),
}

/// `splice(index, 0, item)`.
fn splice_insert<T>(items: &mut Vec<T>, index: usize, item: T) {
    let index = index.min(items.len());
    items.insert(index, item);
}

/// `splice(index, 1)[0]`: `None` quando o índice não existe.
fn splice_remove<T>(items: &mut Vec<T>, index: usize) -> Option<T> {
    (index < items.len()).then(|| items.remove(index))
}

/// `assertReturnsDefined`.
fn assert_returns_defined<T>(value: Option<T>) -> T {
    value.unwrap_or_else(|| errors::throw(Error::new("Assertion Failed: argument is undefined or null")))
}

// #region ExtHostEditorTab

struct EditorTabState {
    dto: EditorTabDto,
    input: Option<AnyTabInput>,
}

pub struct ExtHostEditorTab {
    state: RefCell<EditorTabState>,
    parent_group: Weak<ExtHostEditorTabGroup>,
}

impl ExtHostEditorTab {
    fn new(dto: EditorTabDto, parent_group: Weak<ExtHostEditorTabGroup>) -> Rc<Self> {
        let input = Self::init_input(&dto);
        Rc::new(Self { state: RefCell::new(EditorTabState { dto, input }), parent_group })
    }

    /// `isActive`: um getter, para garantir no máximo uma aba ativa por grupo sem precisar iterar.
    pub fn is_active(&self) -> bool {
        let id = self.state.borrow().dto.id.clone();
        self.parent_group.upgrade().is_some_and(|group| group.active_tab_id() == id)
    }

    pub fn label(&self) -> String {
        self.state.borrow().dto.label.clone()
    }

    pub fn input(&self) -> Option<AnyTabInput> {
        self.state.borrow().input.clone()
    }

    pub fn is_dirty(&self) -> bool {
        self.state.borrow().dto.is_dirty
    }

    pub fn is_pinned(&self) -> bool {
        self.state.borrow().dto.is_pinned
    }

    pub fn is_preview(&self) -> bool {
        self.state.borrow().dto.is_preview
    }

    pub fn group(&self) -> TabGroup {
        self.try_group().expect("o grupo da aba foi descartado")
    }

    /// `group`, sem lançar: `None` quando o grupo já saiu do modelo (o original mantém o objeto vivo pela referência da aba;
    /// aqui a aba guarda o grupo fraco para não formar ciclo).
    pub fn try_group(&self) -> Option<TabGroup> {
        self.parent_group.upgrade()
    }

    fn tab_id(&self) -> String {
        self.state.borrow().dto.id.clone()
    }

    fn accept_dto_update(&self, dto: EditorTabDto) {
        let input = Self::init_input(&dto);
        *self.state.borrow_mut() = EditorTabState { dto, input };
    }

    fn init_input(dto: &EditorTabDto) -> Option<AnyTabInput> {
        match &dto.input {
            AnyInputDto::Text(input) => Some(AnyTabInput::Text(TextTabInput { uri: input.uri.clone() })),
            AnyInputDto::TextDiff(input) => {
                Some(AnyTabInput::TextDiff(TextDiffTabInput { original: input.original.clone(), modified: input.modified.clone() }))
            }
            AnyInputDto::TextMerge(input) => Some(AnyTabInput::TextMerge(TextMergeTabInput {
                base: input.base.clone(),
                input1: input.input1.clone(),
                input2: input.input2.clone(),
                result: input.result.clone(),
            })),
            AnyInputDto::Custom(input) => Some(AnyTabInput::Custom(CustomEditorTabInput { uri: input.uri.clone(), view_type: input.view_type.clone() })),
            AnyInputDto::Webview(input) => Some(AnyTabInput::Webview(WebviewEditorTabInput { view_type: input.view_type.clone() })),
            AnyInputDto::Notebook(input) => {
                Some(AnyTabInput::Notebook(NotebookEditorTabInput { uri: input.uri.clone(), notebook_type: input.notebook_type.clone() }))
            }
            AnyInputDto::NotebookDiff(input) => Some(AnyTabInput::NotebookDiff(NotebookDiffEditorTabInput {
                original: input.original.clone(),
                modified: input.modified.clone(),
                notebook_type: input.notebook_type.clone(),
            })),
            AnyInputDto::Terminal => Some(AnyTabInput::Terminal(TerminalEditorTabInput)),
            AnyInputDto::Interactive(input) => {
                Some(AnyTabInput::Interactive(InteractiveWindowInput { uri: input.uri.clone(), input_box_uri: input.input_box_uri.clone() }))
            }
            AnyInputDto::Chat => Some(AnyTabInput::Chat(ChatEditorTabInput)),
            AnyInputDto::MultiDiff(input) => Some(AnyTabInput::TextMultiDiff(TextMultiDiffTabInput {
                text_diffs: input
                    .diff_editors
                    .iter()
                    .map(|diff| TextDiffTabInput { original: diff.original.clone(), modified: diff.modified.clone() })
                    .collect(),
            })),
            AnyInputDto::Unknown => None,
        }
    }
}

// #endregion

// #region ExtHostEditorTabGroup

pub struct ExtHostEditorTabGroup {
    this: Weak<ExtHostEditorTabGroup>,
    group_id: u32,
    view_column: Cell<f64>,
    tabs: RefCell<Vec<Rc<ExtHostEditorTab>>>,
    active_tab_id: RefCell<String>,
    active_group_id_getter: Rc<Cell<Option<u32>>>,
}

impl ExtHostEditorTabGroup {
    fn new(dto: &EditorTabGroupDto, active_group_id_getter: Rc<Cell<Option<u32>>>) -> Rc<Self> {
        let group = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            group_id: dto.group_id,
            view_column: Cell::new(dto.view_column),
            tabs: RefCell::new(Vec::new()),
            active_tab_id: RefCell::new(String::new()),
            active_group_id_getter,
        });
        // Constrói todas as abas a partir do DTO dado
        group.reconcile_tabs(dto);
        group
    }

    /// `isActive`: um getter, para garantir no máximo um grupo ativo sem precisar iterar.
    pub fn is_active(&self) -> bool {
        Some(self.group_id) == self.active_group_id_getter.get()
    }

    pub fn view_column(&self) -> f64 {
        ViewColumn::to(self.view_column.get())
    }

    pub fn active_tab(&self) -> Option<Tab> {
        let active_tab_id = self.active_tab_id();
        self.tabs.borrow().iter().find(|tab| tab.tab_id() == active_tab_id).cloned()
    }

    pub fn tabs(&self) -> Vec<Tab> {
        self.tabs.borrow().clone()
    }

    fn group_id(&self) -> u32 {
        self.group_id
    }

    fn accept_group_dto_update(&self, dto: &EditorTabGroupDto) {
        self.view_column.set(dto.view_column);
    }

    /// Aceita um DTO de grupo inteiro durante uma ressincronização completa do modelo de abas, reaproveitando as
    /// instâncias de [`ExtHostEditorTab`] das abas que ainda existem, para que a identidade do objeto de API (deste
    /// grupo e das abas) continue estável. As extensões costumam usar esses objetos como chave de `Map`, `WeakMap` e
    /// `Set`, então recriá-los a cada ressincronização quebraria essas buscas e vazaria o que elas guardam.
    fn accept_model_update(&self, dto: &EditorTabGroupDto) {
        self.view_column.set(dto.view_column);
        self.reconcile_tabs(dto);
    }

    fn reconcile_tabs(&self, dto: &EditorTabGroupDto) {
        let existing_tabs: Vec<Tab> = self.tabs.borrow().clone();

        *self.active_tab_id.borrow_mut() = String::new();
        let tabs = dto
            .tabs
            .iter()
            .map(|tab_dto| {
                if tab_dto.is_active {
                    *self.active_tab_id.borrow_mut() = tab_dto.id.clone();
                }
                match existing_tabs.iter().find(|tab| tab.tab_id() == tab_dto.id) {
                    Some(existing) => {
                        existing.accept_dto_update(tab_dto.clone());
                        existing.clone()
                    }
                    None => ExtHostEditorTab::new(tab_dto.clone(), self.this.clone()),
                }
            })
            .collect();
        *self.tabs.borrow_mut() = tabs;
    }

    fn accept_tab_operation(&self, operation: &TabOperation) -> Tab {
        match operation.kind {
            // No caso de abertura, adiciona a aba ao grupo
            TabModelOperationKind::TabOpen => {
                let tab = ExtHostEditorTab::new(operation.tab_dto.clone(), self.this.clone());
                // Insere a aba no índice do editor
                splice_insert(&mut self.tabs.borrow_mut(), operation.index, tab.clone());
                if operation.tab_dto.is_active {
                    *self.active_tab_id.borrow_mut() = tab.tab_id();
                }
                return tab;
            }
            TabModelOperationKind::TabClose => {
                let removed = splice_remove(&mut self.tabs.borrow_mut(), operation.index);
                let Some(tab) = removed else {
                    errors::throw(Error::new(format!("Tab close updated received for index {} which does not exist", operation.index)));
                };
                if tab.tab_id() == *self.active_tab_id.borrow() {
                    *self.active_tab_id.borrow_mut() = String::new();
                }
                return tab;
            }
            TabModelOperationKind::TabMove => {
                let Some(old_index) = operation.old_index else {
                    errors::throw(Error::new("Invalid old index on move IPC"));
                };
                // Remove no índice antigo e insere no novo, o que move a aba
                let removed = splice_remove(&mut self.tabs.borrow_mut(), old_index);
                let Some(tab) = removed else {
                    errors::throw(Error::new(format!("Tab move updated received for index {old_index} which does not exist")));
                };
                splice_insert(&mut self.tabs.borrow_mut(), operation.index, tab.clone());
                return tab;
            }
            TabModelOperationKind::TabUpdate => {}
        }
        let found = self.tabs.borrow().iter().find(|tab| tab.tab_id() == operation.tab_dto.id).cloned();
        let Some(tab) = found else {
            errors::throw(Error::new("INVALID tab"));
        };
        if operation.tab_dto.is_active {
            *self.active_tab_id.borrow_mut() = operation.tab_dto.id.clone();
        } else if *self.active_tab_id.borrow() == operation.tab_dto.id && !operation.tab_dto.is_active {
            // Os eventos não têm ordem garantida: se chega um DTO com o id da aba ativa mas que não está ativo,
            // o id da aba ativa vira vazio. Isso evita que `onDidActiveTabChange` dispare errado
            *self.active_tab_id.borrow_mut() = String::new();
        }
        tab.accept_dto_update(operation.tab_dto.clone());
        tab
    }

    /// Não é um getter porque as abas o usam como callback.
    fn active_tab_id(&self) -> String {
        self.active_tab_id.borrow().clone()
    }
}

// #endregion

/// `vscode.TabGroups`.
pub struct TabGroups {
    host: Weak<ExtHostEditorTabs>,
}

impl TabGroups {
    fn host(&self) -> Rc<ExtHostEditorTabs> {
        self.host.upgrade().expect("o hospedeiro das abas foi descartado")
    }

    /// `onDidChangeTabGroups` (nunca muda, é um valor simples no original).
    pub fn on_did_change_tab_groups(&self) -> Event<TabGroupChangeEvent> {
        self.host().on_did_change_tab_groups.event()
    }

    /// `onDidChangeTabs`.
    pub fn on_did_change_tabs(&self) -> Event<TabChangeEvent> {
        self.host().on_did_change_tabs.event()
    }

    /// `all`.
    pub fn all(&self) -> Vec<TabGroup> {
        self.host().ext_host_tab_groups.borrow().clone()
    }

    /// `activeTabGroup`: lança quando o modelo ainda não tem grupo ativo.
    pub fn active_tab_group(&self) -> TabGroup {
        assert_returns_defined(self.try_active_tab_group())
    }

    /// O grupo ativo, sem o `assertReturnsDefined`: `None` quando o modelo ainda não tem grupo ativo.
    pub fn try_active_tab_group(&self) -> Option<TabGroup> {
        let host = self.host();
        let active_tab_group_id = host.active_group_id.get();
        let groups = host.ext_host_tab_groups.borrow();
        groups.iter().find(|candidate| Some(candidate.group_id()) == active_tab_group_id).cloned()
    }

    /// `close(tabOrTabGroup, preserveFocus)`.
    pub fn close(&self, target: CloseTarget, preserve_focus: Option<bool>) -> Promise<bool> {
        let host = self.host();
        async_fn(async move {
            match target {
                CloseTarget::Tab(tab) => host.close_tabs(vec![tab], preserve_focus).await,
                CloseTarget::TabGroup(group) => host.close_groups(vec![group], preserve_focus).await,
                CloseTarget::Tabs(tabs) if tabs.is_empty() => Ok(true),
                CloseTarget::TabGroups(groups) if groups.is_empty() => Ok(true),
                CloseTarget::Tabs(tabs) => host.close_tabs(tabs, preserve_focus).await,
                CloseTarget::TabGroups(groups) => host.close_groups(groups, preserve_focus).await,
            }
        })
    }
}

pub struct ExtHostEditorTabs {
    this: Weak<ExtHostEditorTabs>,
    proxy: Rc<dyn MainThreadEditorTabsShape>,
    on_did_change_tabs: Rc<Emitter<TabChangeEvent>>,
    on_did_change_tab_groups: Rc<Emitter<TabGroupChangeEvent>>,
    // Começa sem valor e é inicializado via RPC
    active_group_id: Rc<Cell<Option<u32>>>,
    ext_host_tab_groups: RefCell<Vec<TabGroup>>,
    api_object: OnceCell<Rc<TabGroups>>,
}

impl ExtHostEditorTabs {
    pub fn new(ext_host_rpc: &dyn IMainContext) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            proxy: ext_host_rpc.get_proxy(&MainContext::MAIN_THREAD_EDITOR_TABS),
            on_did_change_tabs: Emitter::new(),
            on_did_change_tab_groups: Emitter::new(),
            active_group_id: Rc::new(Cell::new(None)),
            ext_host_tab_groups: RefCell::new(Vec::new()),
            api_object: OnceCell::new(),
        })
    }

    /// `tabGroups`.
    pub fn tab_groups(&self) -> Rc<TabGroups> {
        self.api_object.get_or_init(|| Rc::new(TabGroups { host: self.this.clone() })).clone()
    }

    fn find_ext_host_tab_from_api(&self, api_tab: &Tab) -> Option<Tab> {
        self.ext_host_tab_groups.borrow().iter().flat_map(|group| group.tabs()).find(|tab| Rc::ptr_eq(tab, api_tab))
    }

    fn find_ext_host_tab_group_from_api(&self, api_tab_group: &TabGroup) -> Option<TabGroup> {
        self.ext_host_tab_groups.borrow().iter().find(|candidate| Rc::ptr_eq(candidate, api_tab_group)).cloned()
    }

    fn close_tabs(&self, tabs: Vec<Tab>, preserve_focus: Option<bool>) -> Promise<bool> {
        let mut ext_host_tab_ids = Vec::new();
        for tab in &tabs {
            match self.find_ext_host_tab_from_api(tab) {
                Some(ext_host_tab) => ext_host_tab_ids.push(ext_host_tab.tab_id()),
                None => return Promise::rejected(Error::new("Tab close: Invalid tab not found!")),
            }
        }
        self.proxy.rpc_close_tab(ext_host_tab_ids, preserve_focus)
    }

    fn close_groups(&self, groups: Vec<TabGroup>, preserve_focus: Option<bool>) -> Promise<bool> {
        let mut ext_host_group_ids = Vec::new();
        for group in &groups {
            match self.find_ext_host_tab_group_from_api(group) {
                Some(ext_host_group) => ext_host_group_ids.push(ext_host_group.group_id()),
                None => return Promise::rejected(Error::new("Group close: Invalid group not found!")),
            }
        }
        self.proxy.rpc_close_group(ext_host_group_ids, preserve_focus)
    }

    fn find_group(&self, group_id: u32) -> Option<TabGroup> {
        self.ext_host_tab_groups.borrow().iter().find(|group| group.group_id() == group_id).cloned()
    }
}

impl ExtHostEditorTabsShape for ExtHostEditorTabs {
    fn rpc_accept_editor_tab_model(&self, tab_groups: Vec<EditorTabGroupDto>) {
        let group_ids_before: Vec<u32> = self.ext_host_tab_groups.borrow().iter().map(|group| group.group_id()).collect();
        let group_ids_after: Vec<u32> = tab_groups.iter().map(|dto| dto.group_id).collect();
        let diff = diff_sets(&group_ids_before, &group_ids_after);

        let closed: Vec<TabGroup> =
            self.ext_host_tab_groups.borrow().iter().filter(|group| diff.removed.contains(&group.group_id())).cloned().collect();
        let mut opened: Vec<TabGroup> = Vec::new();
        let mut changed: Vec<TabGroup> = Vec::new();

        // Reaproveita as instâncias dos grupos que ainda existem, para que os objetos `vscode.TabGroup` (e os
        // `vscode.Tab` dentro deles) mantenham a identidade numa ressincronização completa, como nos caminhos de
        // atualização granular. Sem isso, cada ressincronização (por exemplo ao abrir ou fechar um grupo de
        // editores) entregaria objetos novos às extensões, quebrando e vazando qualquer `Map`, `WeakMap` ou `Set`
        // com grupos ou abas como chave.
        let existing_groups: Vec<TabGroup> = self.ext_host_tab_groups.borrow().clone();

        let new_groups: Vec<TabGroup> = tab_groups
            .iter()
            .map(|tab_group| match existing_groups.iter().find(|group| group.group_id() == tab_group.group_id) {
                Some(existing) => {
                    existing.accept_model_update(tab_group);
                    changed.push(existing.clone());
                    existing.clone()
                }
                None => {
                    let group = ExtHostEditorTabGroup::new(tab_group, self.active_group_id.clone());
                    opened.push(group.clone());
                    group
                }
            })
            .collect();
        *self.ext_host_tab_groups.borrow_mut() = new_groups;

        // Define o id do grupo de abas ativo
        let active_tab_group_id = assert_returns_defined(tab_groups.iter().find(|group| group.is_active).map(|group| group.group_id));
        if self.active_group_id.get() != Some(active_tab_group_id) {
            self.active_group_id.set(Some(active_tab_group_id));
        }
        self.on_did_change_tab_groups.fire(TabGroupChangeEvent { opened, closed, changed });
    }

    fn rpc_accept_tab_group_update(&self, group_dto: EditorTabGroupDto) {
        let Some(group) = self.find_group(group_dto.group_id) else {
            errors::throw(Error::new("Update Group IPC call received before group creation."));
        };
        group.accept_group_dto_update(&group_dto);
        if group_dto.is_active {
            self.active_group_id.set(Some(group_dto.group_id));
        }
        self.on_did_change_tab_groups.fire(TabGroupChangeEvent { changed: vec![group], opened: Vec::new(), closed: Vec::new() });
    }

    fn rpc_accept_tab_operation(&self, operation: TabOperation) {
        let Some(group) = self.find_group(operation.group_id) else {
            errors::throw(Error::new("Update Tabs IPC call received before group creation."));
        };
        let tab = group.accept_tab_operation(&operation);

        // Monta o evento de mudança de abas conforme a operação
        let event = match operation.kind {
            TabModelOperationKind::TabOpen => TabChangeEvent { opened: vec![tab], closed: Vec::new(), changed: Vec::new() },
            TabModelOperationKind::TabClose => TabChangeEvent { opened: Vec::new(), closed: vec![tab], changed: Vec::new() },
            TabModelOperationKind::TabMove | TabModelOperationKind::TabUpdate => {
                TabChangeEvent { opened: Vec::new(), closed: Vec::new(), changed: vec![tab] }
            }
        };
        self.on_did_change_tabs.fire(event);
    }
}
