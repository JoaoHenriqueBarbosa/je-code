//! Porte de `src/vs/workbench/contrib/terminal/browser/terminalGroupService.ts`: o [`TerminalGroupService`], o dono dos grupos de
//! terminais do painel, do grupo e da instância ativos, das chaves de contexto `terminalGroupCount` e `terminalSplitPaneActive`, e
//! da reordenação de grupos e de instâncias (`moveGroup`, `moveGroupToEnd`, `moveInstance`, `unsplitInstance`, `joinInstances`).
//!
//! Recortes (são do frontend nativo):
//! - **Painel e views.** `showPanel` e `hidePanel` (`IViewsService`, `IViewDescriptorService` e o `TerminalViewPane`),
//!   `focusTabs`, `focusHover`, `setContainer` e `updateVisibility`. O frontend entrega o [`ITerminalPanelHost`] por
//!   [`TerminalGroupService::set_panel_host`]; sem ele, `showPanel` só avisa que mostrou e `hidePanel` não faz nada.
//! - **`IQuickInputService`.** O `_isQuickInputOpened` do original é a pergunta [`ITerminalPanelHost::is_quick_input_opened`].
//! - **`getInstanceFromResource` e `getGroupLabels`.** O primeiro lê o `terminalUri` do original; o segundo, o `title` do grupo
//!   (ver `terminal_group.rs`).
//!
//! Divergências:
//! - **`onDidChangeActiveGroup`.** Dispara sem carga (o contrato [`ITerminalGroupService`] do porte não a leva).
//! - **`lastAccessedMenu` e `onDidChangePanelOrientation`.** São do menu e do contêiner de painéis, do frontend.
//! - **O próprio serviço é o host de instâncias.** [`ITerminalGroupService::instance_host`] devolve o serviço.
//! - **`createGroup(slcOrInstance)`.** O argumento é o [`TerminalGroupSource`] opcional.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_instantiation::{Injectable, service_impl};
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService};
use je_platform::terminal::terminal::IShellLaunchConfig;

use super::terminal::{
    ITerminalGroup, ITerminalGroupService, ITerminalInstance, ITerminalInstanceHost, ITerminalInstanceService, TerminalGroupSource, TerminalMovePosition,
};
use super::terminal_group::{TerminalGroup, move_items};
use crate::contrib::terminal::common::terminal_context_key;

/// O que o frontend nativo entrega ao serviço para mostrar, esconder e perguntar sobre o painel do terminal.
pub trait ITerminalPanelHost {
    /// `showPanel(focus)`: abre a view do terminal (se não está aberta), expande, e com `focus` foca a instância ativa.
    fn show_panel(&self, focus: bool) -> Promise<()>;
    /// `hidePanel()`: esconde o painel quando o terminal está nele e não tem views irmãs.
    fn hide_panel(&self);
    /// `_isQuickInputOpened`.
    fn is_quick_input_opened(&self) -> bool;
}

/// O `===` de dois grupos.
fn same_group(a: &Rc<dyn ITerminalGroup>, b: &Rc<dyn ITerminalGroup>) -> bool {
    Rc::ptr_eq(a, b)
}

fn same_instance(a: &Rc<dyn ITerminalInstance>, b: &Rc<dyn ITerminalInstance>) -> bool {
    Rc::ptr_eq(a, b)
}

/// `IInstanceLocation`.
struct InstanceLocation {
    group: Rc<dyn ITerminalGroup>,
    group_index: usize,
    instance_index: usize,
}

/// `TerminalGroupService`.
pub struct TerminalGroupService {
    this: Weak<Self>,
    /// O `_store` do `Disposable` do original.
    store: Rc<DisposableStore>,
    terminal_instance_service: Rc<dyn ITerminalInstanceService>,
    panel_host: RefCell<Option<Rc<dyn ITerminalPanelHost>>>,

    groups: RefCell<Vec<Rc<dyn ITerminalGroup>>>,
    active_group_index: Cell<isize>,
    group_count_context_key: Rc<dyn IContextKey>,
    split_terminal_active_context_key: Rc<dyn IContextKey>,

    on_did_change_active_group: Rc<Emitter<()>>,
    on_did_dispose_group: Rc<Emitter<Rc<dyn ITerminalGroup>>>,
    on_did_change_groups: Rc<Emitter<()>>,
    on_did_show: Rc<Emitter<()>>,
    on_did_dispose_instance: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    on_did_focus_instance: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    on_did_change_active_instance: Rc<Emitter<Option<Rc<dyn ITerminalInstance>>>>,
    on_did_change_instances: Rc<Emitter<()>>,
    on_did_change_instance_capability: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
}

impl TerminalGroupService {
    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço de grupos de terminais foi descartado da memória")
    }

    /// Registra o host do painel (ver "Recortes").
    pub fn set_panel_host(&self, panel_host: Option<Rc<dyn ITerminalPanelHost>>) {
        *self.panel_host.borrow_mut() = panel_host;
    }

    /// `groups`.
    pub fn groups(&self) -> Vec<Rc<dyn ITerminalGroup>> {
        self.groups.borrow().clone()
    }

    /// `activeGroupIndex`.
    pub fn active_group_index(&self) -> isize {
        self.active_group_index.get()
    }

    /// `onDidDisposeGroup`.
    pub fn on_did_dispose_group(&self) -> Event<Rc<dyn ITerminalGroup>> {
        self.on_did_dispose_group.event()
    }

    /// `onDidChangeGroups`.
    pub fn on_did_change_groups(&self) -> Event<()> {
        self.on_did_change_groups.event()
    }

    /// `onDidShow`.
    pub fn on_did_show(&self) -> Event<()> {
        self.on_did_show.event()
    }

    /// `activeGroup`.
    pub fn active_group(&self) -> Option<Rc<dyn ITerminalGroup>> {
        let groups = self.groups.borrow();
        usize::try_from(self.active_group_index.get()).ok().and_then(|index| groups.get(index).cloned())
    }

    /// `activeGroup = value`: `None` não faz nada (só a remoção do último grupo desfaz o grupo ativo).
    pub fn set_active_group(&self, value: Option<&Rc<dyn ITerminalGroup>>) {
        let Some(value) = value else { return };
        let index = self.groups.borrow().iter().position(|group| same_group(group, value)).map_or(-1, |index| index as isize);
        self.set_active_group_by_index(index, false);
    }

    /// `_getIndexFromId`.
    fn get_index_from_id(&self, terminal_id: u32) -> usize {
        match ITerminalInstanceHost::instances(self).iter().position(|instance| instance.instance_id() == terminal_id) {
            Some(index) => index,
            None => errors::throw(Error::new(format!("Terminal with ID {terminal_id} does not exist (has it already been disposed?)"))),
        }
    }

    /// `focusInstance(instance)`.
    pub fn focus_instance(&self, instance: &Rc<dyn ITerminalInstance>) -> Promise<()> {
        if ITerminalInstanceHost::instances(self).iter().any(|candidate| same_instance(candidate, instance)) {
            self.set_active_instance_ref(instance);
        }
        self.show_panel_task(true)
    }

    /// `focusActiveInstance()`.
    pub fn focus_active_instance(&self) -> Promise<()> {
        self.show_panel_task(true)
    }

    /// `createGroup(slcOrInstance?)`.
    pub fn create_group_from_source(&self, shell_launch_config_or_instance: Option<TerminalGroupSource>) -> Rc<dyn ITerminalGroup> {
        let group = TerminalGroup::new(self.terminal_instance_service.clone(), shell_launch_config_or_instance);
        let group_ref: Rc<dyn ITerminalGroup> = group.clone();
        self.groups.borrow_mut().push(group_ref.clone());
        group.add_disposable(group_ref.on_did_dispose_instance().forward(&self.on_did_dispose_instance));
        group.add_disposable(group_ref.on_did_focus_instance().forward(&self.on_did_focus_instance));
        group.add_disposable(group_ref.on_did_change_instance_capability().forward(&self.on_did_change_instance_capability));
        group.add_disposable(group_ref.on_instances_changed().forward(&self.on_did_change_instances));
        group.add_disposable(group_ref.on_disposed().forward(&self.on_did_dispose_group));
        group.add_disposable(group_ref.on_did_change_active_instance().listen({
            let (weak, group) = (self.this.clone(), group_ref.clone());
            move |instance| {
                let Some(this) = weak.upgrade() else { return };
                if this.active_group().is_some_and(|active| same_group(&active, &group)) {
                    this.on_did_change_active_instance.fire(instance.clone());
                }
            }
        }));
        if !group_ref.terminal_instances().is_empty() {
            self.on_did_change_instances.fire(());
        }
        if ITerminalInstanceHost::instances(self).len() == 1 {
            // It's the first instance so it should be made active automatically, this must fire after onInstancesChanged so
            // consumers can react to the instance being added first
            self.set_active_instance_by_index_impl(0);
        }
        self.on_did_change_groups.fire(());
        group_ref
    }

    /// `showPanel(focus?)`.
    fn show_panel_task(&self, focus: bool) -> Promise<()> {
        let this = self.rc();
        let host = self.panel_host.borrow().clone();
        async_fn(async move {
            if let Some(host) = host {
                host.show_panel(focus).await?;
            }
            this.on_did_show.fire(());
            Ok(())
        })
    }

    /// `_removeGroup`.
    fn remove_group(&self, group: &Rc<dyn ITerminalGroup>) {
        // Get the index of the group and remove it from the list
        let was_active_group = self.active_group().is_some_and(|active| same_group(&active, group));
        let found = self.groups.borrow().iter().position(|candidate| same_group(candidate, group));
        let index = found.map_or(-1, |index| index as isize);
        if let Some(found) = found {
            self.groups.borrow_mut().remove(found);
            self.on_did_change_groups.fire(());
        }

        let remaining = self.groups.borrow().len() as isize;
        if was_active_group {
            // Adjust focus if the group was active
            if remaining > 0 && !self.is_quick_input_opened() {
                let new_index = if index < remaining { index } else { remaining - 1 };
                self.set_active_group_by_index(new_index, true);
                if group.had_focus_on_exit()
                    && let Some(active) = ITerminalInstanceHost::active_instance(self)
                {
                    active.focus();
                }
            }
        } else if self.active_group_index.get() > index {
            // Adjust the active group if the removed group was above the active group
            self.set_active_group_by_index(self.active_group_index.get() - 1, false);
        }
        // Ensure the active group is still valid, this should set the activeGroupIndex to -1 if there are no groups
        let remaining = self.groups.borrow().len() as isize;
        if self.active_group_index.get() >= remaining {
            self.set_active_group_by_index(remaining - 1, false);
        }

        self.on_did_change_instances.fire(());
        self.on_did_change_groups.fire(());
        if was_active_group {
            self.on_did_change_active_group.fire(());
            self.on_did_change_active_instance.fire(ITerminalInstanceHost::active_instance(self));
        }
    }

    fn is_quick_input_opened(&self) -> bool {
        self.panel_host.borrow().as_ref().is_some_and(|host| host.is_quick_input_opened())
    }

    /// `setActiveGroupByIndex(index, force?)`: `force` é para quando o grupo ativo anterior foi removido.
    pub fn set_active_group_by_index(&self, index: isize, force: bool) {
        // Unset active group when the last group is removed
        if index == -1 && self.groups.borrow().is_empty() {
            if self.active_group_index.get() != -1 {
                self.active_group_index.set(-1);
                self.on_did_change_active_group.fire(());
                self.on_did_change_active_instance.fire(ITerminalInstanceHost::active_instance(self));
            }
            return;
        }

        // Ensure index is valid
        if index < 0 || index >= self.groups.borrow().len() as isize {
            return;
        }

        // Fire group/instance change if needed
        let old_active_group = self.active_group();
        self.active_group_index.set(index);
        let changed = match (&old_active_group, &self.active_group()) {
            (Some(old), Some(new)) => !same_group(old, new),
            (None, None) => false,
            _ => true,
        };
        if force || changed {
            self.on_did_change_active_group.fire(());
            self.on_did_change_active_instance.fire(ITerminalInstanceHost::active_instance(self));
        }
    }

    /// `_getInstanceLocation`.
    fn get_instance_location(&self, index: usize) -> Option<InstanceLocation> {
        let mut index = index;
        for (group_index, group) in self.groups.borrow().iter().enumerate() {
            let count = group.terminal_instances().len();
            if index < count {
                return Some(InstanceLocation { group: group.clone(), group_index, instance_index: index });
            }
            index -= count;
        }
        None
    }

    /// `setActiveInstanceByIndex(index)`.
    fn set_active_instance_by_index_impl(&self, index: usize) {
        let active_instance = ITerminalInstanceHost::active_instance(self);
        let Some(location) = self.get_instance_location(index) else { return };
        let new_active_instance = location.group.terminal_instances().get(location.instance_index).cloned();
        let unchanged = match (&active_instance, &new_active_instance) {
            (Some(active), Some(new)) => same_instance(active, new),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }

        self.active_group_index.set(location.group_index as isize);
        self.on_did_change_active_group.fire(());
        location.group.set_active_instance_by_index(location.instance_index, true);
    }

    /// `setActiveInstance(instance)`.
    fn set_active_instance_ref(&self, instance: &Rc<dyn ITerminalInstance>) {
        self.set_active_instance_by_index_impl(self.get_index_from_id(instance.instance_id()));
    }

    /// `setActiveGroupToNext()`.
    pub fn set_active_group_to_next(&self) {
        let len = self.groups.borrow().len() as isize;
        if len <= 1 {
            return;
        }
        let mut new_index = self.active_group_index.get() + 1;
        if new_index >= len {
            new_index = 0;
        }
        self.set_active_group_by_index(new_index, false);
    }

    /// `setActiveGroupToPrevious()`.
    pub fn set_active_group_to_previous(&self) {
        let len = self.groups.borrow().len() as isize;
        if len <= 1 {
            return;
        }
        let mut new_index = self.active_group_index.get() - 1;
        if new_index < 0 {
            new_index = len - 1;
        }
        self.set_active_group_by_index(new_index, false);
    }

    /// `_getValidTerminalGroups(sources)`: o `Set` de grupos, na ordem do primeiro aparecimento.
    fn get_valid_terminal_groups(&self, sources: &[Rc<dyn ITerminalInstance>]) -> Vec<Rc<dyn ITerminalGroup>> {
        let mut groups: Vec<Rc<dyn ITerminalGroup>> = Vec::new();
        for source in sources {
            if let Some(group) = self.get_group_for_instance_ref(source)
                && !groups.iter().any(|existing| same_group(existing, &group))
            {
                groups.push(group);
            }
        }
        groups
    }

    fn group_index(&self, group: &Rc<dyn ITerminalGroup>) -> usize {
        self.groups.borrow().iter().position(|candidate| same_group(candidate, group)).unwrap_or(usize::MAX)
    }

    /// `moveGroup(source, target)`.
    pub fn move_group(&self, source: &[Rc<dyn ITerminalInstance>], target: &Rc<dyn ITerminalInstance>) {
        let source_groups = self.get_valid_terminal_groups(source);
        let Some(target_group) = self.get_group_for_instance_ref(target) else { return };
        if source_groups.is_empty() {
            return;
        }

        // The groups are the same, rearrange within the group
        if source_groups.len() == 1 && same_group(&source_groups[0], &target_group) {
            let instances = target_group.terminal_instances();
            let index_of = |instance: &Rc<dyn ITerminalInstance>| instances.iter().position(|candidate| same_instance(candidate, instance)).unwrap_or(usize::MAX);
            let target_index = index_of(target);
            let mut sorted_sources = source.to_vec();
            sorted_sources.sort_by_key(|instance| index_of(instance));
            let first_target_index = index_of(&sorted_sources[0]);
            let position = if first_target_index < target_index { TerminalMovePosition::After } else { TerminalMovePosition::Before };
            target_group.move_instance(&sorted_sources, target_index, position);
            self.on_did_change_instances.fire(());
            return;
        }

        // The groups differ, rearrange groups
        let target_group_index = self.group_index(&target_group);
        let mut sorted_source_groups = source_groups;
        sorted_source_groups.sort_by_key(|group| self.group_index(group));
        let first_source_group_index = self.group_index(&sorted_source_groups[0]);
        let position = if first_source_group_index < target_group_index { TerminalMovePosition::After } else { TerminalMovePosition::Before };
        let insert_index = if position == TerminalMovePosition::After { target_group_index + 1 } else { target_group_index };
        move_items(&mut self.groups.borrow_mut(), insert_index, &sorted_source_groups, position, same_group);
        self.on_did_change_instances.fire(());
    }

    /// `moveGroupToEnd(source)`.
    pub fn move_group_to_end(&self, source: &[Rc<dyn ITerminalInstance>]) {
        let mut source_groups = self.get_valid_terminal_groups(source);
        if source_groups.is_empty() {
            return;
        }
        source_groups.sort_by_key(|group| self.group_index(group));
        let end = self.groups.borrow().len();
        move_items(&mut self.groups.borrow_mut(), end, &source_groups, TerminalMovePosition::After, same_group);
        self.on_did_change_instances.fire(());
    }

    /// `moveInstance(source, target, side)`.
    pub fn move_instance(&self, source: &Rc<dyn ITerminalInstance>, target: &Rc<dyn ITerminalInstance>, side: TerminalMovePosition) {
        let (Some(source_group), Some(target_group)) = (self.get_group_for_instance_ref(source), self.get_group_for_instance_ref(target)) else {
            return;
        };

        // Move from the source group to the target group
        if !same_group(&source_group, &target_group) {
            // Move groups
            source_group.remove_instance(source);
            target_group.add_instance(TerminalGroupSource::Instance(source.clone()), None);
        }

        // Rearrange within the target group
        let target_index = target_group.terminal_instances().iter().position(|candidate| same_instance(candidate, target)).unwrap_or(0);
        let index = target_index + usize::from(side == TerminalMovePosition::After);
        target_group.move_instance(std::slice::from_ref(source), index, side);
    }

    /// `unsplitInstance(instance)`.
    pub fn unsplit_instance(&self, instance: &Rc<dyn ITerminalInstance>) {
        let Some(old_group) = self.get_group_for_instance_ref(instance) else { return };
        if old_group.terminal_instances().len() < 2 {
            return;
        }

        old_group.remove_instance(instance);
        self.create_group_from_source(Some(TerminalGroupSource::Instance(instance.clone())));
    }

    /// `joinInstances(instances)`.
    pub fn join_instances(&self, instances: &[Rc<dyn ITerminalInstance>]) {
        let Some(first) = instances.first() else { return };
        if let Some(group) = self.get_group_for_instance_ref(first) {
            let group_instances = group.terminal_instances();
            let mut different_groups = true;
            for i in 1..group_instances.len() {
                if instances.get(i).is_some_and(|candidate| group_instances.iter().any(|instance| same_instance(instance, candidate))) {
                    different_groups = false;
                    break;
                }
            }
            if !different_groups && group_instances.len() == instances.len() {
                return;
            }
        }
        // Find the group of the first instance that is the only instance in the group, if one exists
        let mut candidate_instance: Option<Rc<dyn ITerminalInstance>> = None;
        let mut candidate_group: Option<Rc<dyn ITerminalGroup>> = None;
        for instance in instances {
            if let Some(group) = self.get_group_for_instance_ref(instance)
                && group.terminal_instances().len() == 1
            {
                candidate_instance = Some(instance.clone());
                candidate_group = Some(group);
                break;
            }
        }

        // Create a new group if needed
        let candidate_group = candidate_group.unwrap_or_else(|| self.create_group_from_source(None));

        let was_active_group = self.active_group().is_some_and(|active| same_group(&active, &candidate_group));

        // Unsplit all other instances and add them to the new group
        for instance in instances {
            if candidate_instance.as_ref().is_some_and(|candidate| same_instance(candidate, instance)) {
                continue;
            }

            let Some(old_group) = self.get_group_for_instance_ref(instance) else {
                // Something went wrong, don't join this one
                continue;
            };
            old_group.remove_instance(instance);
            candidate_group.add_instance(TerminalGroupSource::Instance(instance.clone()), None);
        }

        // Set the active terminal
        self.set_active_instance_ref(first);

        // Fire events
        self.on_did_change_instances.fire(());
        if !was_active_group {
            self.on_did_change_active_group.fire(());
        }
    }

    /// `instanceIsSplit(instance)`.
    pub fn instance_is_split(&self, instance: &Rc<dyn ITerminalInstance>) -> bool {
        self.get_group_for_instance_ref(instance).is_some_and(|group| group.terminal_instances().len() > 1)
    }

    /// `getGroupForInstance(instance)`.
    fn get_group_for_instance_ref(&self, instance: &Rc<dyn ITerminalInstance>) -> Option<Rc<dyn ITerminalGroup>> {
        self.groups.borrow().iter().find(|group| group.terminal_instances().iter().any(|candidate| same_instance(candidate, instance))).cloned()
    }
}

impl ITerminalInstanceHost for TerminalGroupService {
    fn active_instance(&self) -> Option<Rc<dyn ITerminalInstance>> {
        self.active_group().and_then(|group| group.active_instance())
    }

    fn instances(&self) -> Vec<Rc<dyn ITerminalInstance>> {
        self.groups.borrow().iter().flat_map(|group| group.terminal_instances()).collect()
    }

    fn on_did_change_active_instance(&self) -> Event<Option<Rc<dyn ITerminalInstance>>> {
        self.on_did_change_active_instance.event()
    }

    fn on_did_dispose_instance(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_did_dispose_instance.event()
    }

    fn on_did_focus_instance(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_did_focus_instance.event()
    }

    fn on_did_change_instance_capability(&self) -> Event<Rc<dyn ITerminalInstance>> {
        self.on_did_change_instance_capability.event()
    }

    fn on_did_change_instances(&self) -> Event<()> {
        self.on_did_change_instances.event()
    }
}

impl ITerminalGroupService for TerminalGroupService {
    fn show_panel(&self, focus: bool) -> Promise<()> {
        self.show_panel_task(focus)
    }

    fn hide_panel(&self) {
        let host = self.panel_host.borrow().clone();
        if let Some(host) = host {
            host.hide_panel();
        }
    }

    fn instance_host(&self) -> Rc<dyn ITerminalInstanceHost> {
        self.rc()
    }

    fn on_did_change_active_group(&self) -> Event<()> {
        self.on_did_change_active_group.event()
    }

    fn set_active_instance(&self, instance: Rc<dyn ITerminalInstance>) {
        self.set_active_instance_ref(&instance);
    }

    fn set_active_instance_by_index(&self, index: usize) {
        self.set_active_instance_by_index_impl(index);
    }

    fn create_group(&self, shell_launch_config: IShellLaunchConfig) -> Rc<dyn ITerminalInstance> {
        let group = self.create_group_from_source(Some(TerminalGroupSource::LaunchConfig(shell_launch_config)));
        group.terminal_instances().remove(0)
    }

    fn create_group_for_instance(&self, instance: Rc<dyn ITerminalInstance>) {
        self.create_group_from_source(Some(TerminalGroupSource::Instance(instance)));
    }

    fn split_instance(&self, parent: Rc<dyn ITerminalInstance>, shell_launch_config: IShellLaunchConfig) -> Option<Rc<dyn ITerminalInstance>> {
        self.get_group_for_instance_ref(&parent).map(|group| group.split(shell_launch_config))
    }

    fn get_group_for_instance(&self, instance: &Rc<dyn ITerminalInstance>) -> Option<Rc<dyn ITerminalGroup>> {
        self.get_group_for_instance_ref(instance)
    }
}

impl IDisposable for TerminalGroupService {
    fn dispose(&self) {
        self.store.dispose();
    }
}

service_impl!(TerminalGroupService => dyn ITerminalGroupService, disposable);

impl Injectable for TerminalGroupService {
    type Args = ();
    type Deps = (Rc<dyn IContextKeyService>, Rc<dyn ITerminalInstanceService>);

    fn create(_: (), (context_key_service, terminal_instance_service): Self::Deps) -> Rc<Self> {
        let store = DisposableStore::new();
        let group_count_context_key = terminal_context_key::group_count_bind_to(&*context_key_service);
        let split_terminal_active_context_key = terminal_context_key::split_terminal_active_bind_to(&*context_key_service);

        let this = Rc::new_cyclic(|weak: &Weak<Self>| Self {
            this: weak.clone(),
            terminal_instance_service,
            panel_host: RefCell::new(None),
            groups: RefCell::new(Vec::new()),
            active_group_index: Cell::new(-1),
            group_count_context_key,
            split_terminal_active_context_key,
            on_did_change_active_group: store.add_rc(Emitter::new()),
            on_did_dispose_group: store.add_rc(Emitter::new()),
            on_did_change_groups: store.add_rc(Emitter::new()),
            on_did_show: store.add_rc(Emitter::new()),
            on_did_dispose_instance: store.add_rc(Emitter::new()),
            on_did_focus_instance: store.add_rc(Emitter::new()),
            on_did_change_active_instance: store.add_rc(Emitter::new()),
            on_did_change_instances: store.add_rc(Emitter::new()),
            on_did_change_instance_capability: store.add_rc(Emitter::new()),
            store: store.clone(),
        });

        let weak = Rc::downgrade(&this);
        let group_count_listener: DisposableRef = this.on_did_change_groups.event().run_and_subscribe(
            {
                let weak = weak.clone();
                move |_| {
                    if let Some(this) = weak.upgrade() {
                        this.group_count_context_key.set(Dynamic::Number(this.groups.borrow().len() as f64));
                    }
                }
            },
            None,
        );
        store.add(group_count_listener);

        let split_active_listener: DisposableRef = this.on_did_focus_instance.event().run_and_subscribe(
            {
                let weak = weak.clone();
                move |_| {
                    if let Some(this) = weak.upgrade() {
                        let split = ITerminalInstanceHost::active_instance(&*this).is_some_and(|active| this.instance_is_split(&active));
                        this.split_terminal_active_context_key.set(Dynamic::Bool(split));
                    }
                }
            },
            None,
        );
        store.add(split_active_listener);

        store.add(this.on_did_dispose_group.event().listen(move |group| {
            if let Some(this) = weak.upgrade() {
                this.remove_group(group);
            }
        }));

        this
    }
}
