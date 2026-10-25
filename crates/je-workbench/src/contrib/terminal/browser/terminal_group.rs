//! Porte de `src/vs/workbench/contrib/terminal/browser/terminalGroup.ts`: o [`TerminalGroup`], o conjunto de instâncias de terminal
//! de uma aba do painel (as instâncias divididas), com a instância ativa, a edição do conjunto (`addInstance`, `removeInstance`,
//! `moveInstance`, `split`) e os eventos que o serviço de grupos repassa.
//!
//! Recortes (são do frontend nativo, que desenha e dimensiona os painéis):
//! - **O contêiner de painéis.** `SplitPaneContainer` e `SplitView` (`_createSplitView`, `split`, `remove`, `layout`,
//!   `setOrientation`, `resizePane`, `getPaneSize`), `attachToElement`, `layout`, `resizePane`, `resizePanes`,
//!   `getLayoutInfo`, `setVisible`, `onPanelOrientationChanged` e a posição do painel (`_panelPosition`, `_terminalLocation`,
//!   `_getPosition`, `_getOrientation`), que dependem do `IWorkbenchLayoutService`, do `IViewDescriptorService`, do
//!   `IPaneCompositePartService` e do DOM. A ordem e a seleção das instâncias, que são o estado observável do grupo, ficam.
//! - **`title`.** Lê `description` e `statusList` da instância (para o asterisco do sino), que o contrato do porte não tem.
//!
//! Divergências:
//! - **`addInstance(IShellLaunchConfig | ITerminalInstance, parentTerminalId?)`.** O argumento é o [`TerminalGroupSource`]; a
//!   configuração vira uma instância no painel pelo `ITerminalInstanceService`, como no original. O construtor recebe o serviço
//!   de instâncias no lugar do `IInstantiationService` (o grupo só o usa para isso).
//! - **`instance.focus(true)` e `hadFocusOnExit`.** O foco é do frontend nativo: `focus` é o do contrato (que não faz nada na
//!   instância do porte) e `hadFocusOnExit` vale o que a instância informa (`false` por padrão).
//! - **`addDisposable`.** Registra no store do grupo, que o `dispose` esvazia.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_platform::terminal::terminal::{IShellLaunchConfig, TerminalLocation};

use super::terminal::{ITerminalGroup, ITerminalInstance, ITerminalInstanceService, TerminalGroupSource, TerminalMovePosition};

/// O `===` de duas instâncias.
fn same_instance(a: &Rc<dyn ITerminalInstance>, b: &Rc<dyn ITerminalInstance>) -> bool {
    Rc::ptr_eq(a, b)
}

/// O miolo de `moveInstance`, `moveGroup` e `moveGroupToEnd`: `list.splice(insertIndex, 0, ...items)` e, para cada item, a
/// retirada da ocorrência original (`indexOf`, a primeira, depois de `after`; `lastIndexOf`, a última, depois de `before`).
pub(super) fn move_items<T: Clone>(list: &mut Vec<T>, insert_index: usize, items: &[T], position: TerminalMovePosition, same: impl Fn(&T, &T) -> bool) {
    let at = insert_index.min(list.len());
    for (offset, item) in items.iter().enumerate() {
        list.insert(at + offset, item.clone());
    }
    for item in items {
        let origin = match position {
            TerminalMovePosition::After => list.iter().position(|candidate| same(candidate, item)),
            TerminalMovePosition::Before => list.iter().rposition(|candidate| same(candidate, item)),
        };
        if let Some(origin) = origin {
            list.remove(origin);
        }
    }
}

/// `TerminalGroup`.
pub struct TerminalGroup {
    this: Weak<Self>,
    /// O `_store` do `Disposable` do original.
    store: Rc<DisposableStore>,
    terminal_instance_service: Rc<dyn ITerminalInstanceService>,

    terminal_instances: RefCell<Vec<Rc<dyn ITerminalInstance>>>,
    instance_disposables: RefCell<HashMap<u32, Vec<DisposableRef>>>,
    active_instance_index: Cell<isize>,
    had_focus_on_exit: Cell<bool>,

    on_did_dispose_instance: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    on_did_focus_instance: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    on_did_change_instance_capability: Rc<Emitter<Rc<dyn ITerminalInstance>>>,
    on_disposed: Rc<Emitter<Rc<dyn ITerminalGroup>>>,
    on_instances_changed: Rc<Emitter<()>>,
    on_did_change_active_instance: Rc<Emitter<Option<Rc<dyn ITerminalInstance>>>>,
}

impl TerminalGroup {
    /// `new TerminalGroup(container, shellLaunchConfigOrInstance, ...)`: sem o contêiner (ver os recortes), e com o argumento
    /// opcional que o construtor do original adiciona ao grupo.
    pub fn new(terminal_instance_service: Rc<dyn ITerminalInstanceService>, shell_launch_config_or_instance: Option<TerminalGroupSource>) -> Rc<Self> {
        let store = DisposableStore::new();
        let group = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            terminal_instance_service,
            terminal_instances: RefCell::new(Vec::new()),
            instance_disposables: RefCell::new(HashMap::new()),
            active_instance_index: Cell::new(-1),
            had_focus_on_exit: Cell::new(false),
            on_did_dispose_instance: store.add_rc(Emitter::new()),
            on_did_focus_instance: store.add_rc(Emitter::new()),
            on_did_change_instance_capability: store.add_rc(Emitter::new()),
            on_disposed: store.add_rc(Emitter::new()),
            on_instances_changed: store.add_rc(Emitter::new()),
            on_did_change_active_instance: store.add_rc(Emitter::new()),
            store: store.clone(),
        });
        if let Some(source) = shell_launch_config_or_instance {
            group.add_instance_internal(source, None);
        }
        group
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o grupo de terminais foi descartado da memória")
    }

    fn rc_group(&self) -> Rc<dyn ITerminalGroup> {
        self.rc()
    }

    /// `addInstance(shellLaunchConfigOrInstance, parentTerminalId?)`.
    fn add_instance_internal(&self, source: TerminalGroupSource, parent_terminal_id: Option<u32>) {
        // if a parent terminal is provided, find it, otherwise, parent is the active terminal
        let parent_index: isize = match parent_terminal_id.filter(|id| *id != 0) {
            Some(parent_id) => self.terminal_instances.borrow().iter().position(|instance| instance.instance_id() == parent_id).map_or(-1, |index| index as isize),
            None => self.active_instance_index.get(),
        };
        let instance = match source {
            TerminalGroupSource::Instance(instance) => instance,
            TerminalGroupSource::LaunchConfig(shell_launch_config) => self.terminal_instance_service.create_instance(shell_launch_config, TerminalLocation::Panel),
        };
        {
            let mut instances = self.terminal_instances.borrow_mut();
            if instances.is_empty() {
                instances.push(instance.clone());
                self.active_instance_index.set(0);
            } else {
                let at = (parent_index + 1).clamp(0, instances.len() as isize) as usize;
                instances.insert(at, instance.clone());
            }
        }
        self.init_instance_listeners(&instance);

        self.on_instances_changed.fire(());
    }

    /// `_initInstanceListeners`.
    fn init_instance_listeners(&self, instance: &Rc<dyn ITerminalInstance>) {
        let weak = self.this.clone();
        let disposables: Vec<DisposableRef> = vec![
            instance.on_disposed().listen({
                let weak = weak.clone();
                move |disposed| {
                    if let Some(this) = weak.upgrade() {
                        this.on_did_dispose_instance.fire(disposed.clone());
                        this.handle_on_did_dispose_instance(disposed);
                    }
                }
            }),
            instance.on_did_focus().listen({
                let weak = weak.clone();
                move |focused| {
                    if let Some(this) = weak.upgrade() {
                        this.set_active_instance(focused);
                        this.on_did_focus_instance.fire(focused.clone());
                    }
                }
            }),
            instance.capabilities().on_did_change_capabilities().listen({
                let instance = instance.clone();
                move |_| {
                    if let Some(this) = weak.upgrade() {
                        this.on_did_change_instance_capability.fire(instance.clone());
                    }
                }
            }),
        ];
        self.instance_disposables.borrow_mut().insert(instance.instance_id(), disposables);
    }

    /// `_handleOnDidDisposeInstance`.
    fn handle_on_did_dispose_instance(&self, instance: &Rc<dyn ITerminalInstance>) {
        self.remove_instance_internal(instance);
    }

    /// `_removeInstance`.
    fn remove_instance_internal(&self, instance: &Rc<dyn ITerminalInstance>) {
        let Some(index) = self.terminal_instances.borrow().iter().position(|candidate| same_instance(candidate, instance)) else {
            return;
        };

        let was_active_instance = self.active_instance().is_some_and(|active| same_instance(&active, instance));
        self.terminal_instances.borrow_mut().remove(index);

        // Adjust focus if the instance was active
        let remaining = self.terminal_instances.borrow().len();
        if was_active_instance && remaining > 0 {
            let new_index = if index < remaining { index } else { remaining - 1 };
            self.set_active_instance_by_index(new_index, false);
            // TODO: Only focus the new instance if the group had focus?
            if let Some(active) = self.active_instance() {
                active.focus();
            }
        } else if (index as isize) < self.active_instance_index.get() {
            // Adjust active instance index if needed
            self.active_instance_index.set(self.active_instance_index.get() - 1);
        }

        // Fire events and dispose group if it was the last instance
        if self.terminal_instances.borrow().is_empty() {
            self.had_focus_on_exit.set(instance.had_focus_on_exit());
            self.on_disposed.fire(self.rc_group());
            self.dispose();
        } else {
            self.on_instances_changed.fire(());
        }

        // Dispose instance event listeners
        let disposables = self.instance_disposables.borrow_mut().remove(&instance.instance_id());
        for disposable in disposables.into_iter().flatten() {
            disposable.dispose();
        }
    }

    /// `_setActiveInstance`.
    fn set_active_instance(&self, instance: &Rc<dyn ITerminalInstance>) {
        let index = self.get_index_from_id(instance.instance_id());
        self.set_active_instance_by_index(index, false);
    }

    /// `_getIndexFromId`.
    fn get_index_from_id(&self, terminal_id: u32) -> usize {
        // O original guarda a última ocorrência (`forEach` sobrescreve o índice).
        match self.terminal_instances.borrow().iter().rposition(|instance| instance.instance_id() == terminal_id) {
            Some(index) => index,
            None => errors::throw(Error::new(format!("Terminal with ID {terminal_id} does not exist (has it already been disposed?)"))),
        }
    }
}

impl ITerminalGroup for TerminalGroup {
    fn terminal_instances(&self) -> Vec<Rc<dyn ITerminalInstance>> {
        self.terminal_instances.borrow().clone()
    }

    fn active_instance(&self) -> Option<Rc<dyn ITerminalInstance>> {
        let instances = self.terminal_instances.borrow();
        if instances.is_empty() {
            return None;
        }
        usize::try_from(self.active_instance_index.get()).ok().and_then(|index| instances.get(index).cloned())
    }

    fn had_focus_on_exit(&self) -> bool {
        self.had_focus_on_exit.get()
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

    fn on_disposed(&self) -> Event<Rc<dyn ITerminalGroup>> {
        self.on_disposed.event()
    }

    fn on_instances_changed(&self) -> Event<()> {
        self.on_instances_changed.event()
    }

    fn on_did_change_active_instance(&self) -> Event<Option<Rc<dyn ITerminalInstance>>> {
        self.on_did_change_active_instance.event()
    }

    fn add_instance(&self, source: TerminalGroupSource, parent_terminal_id: Option<u32>) {
        self.add_instance_internal(source, parent_terminal_id);
    }

    fn remove_instance(&self, instance: &Rc<dyn ITerminalInstance>) {
        self.remove_instance_internal(instance);
    }

    fn move_instance(&self, instances: &[Rc<dyn ITerminalInstance>], index: usize, position: TerminalMovePosition) {
        let has_invalid_instance = {
            let current = self.terminal_instances.borrow();
            instances.iter().any(|instance| !current.iter().any(|candidate| same_instance(candidate, instance)))
        };
        if has_invalid_instance {
            return;
        }
        let insert_index = match position {
            TerminalMovePosition::Before => index,
            TerminalMovePosition::After => index + 1,
        };
        move_items(&mut self.terminal_instances.borrow_mut(), insert_index, instances, position, same_instance);
        self.on_instances_changed.fire(());
    }

    fn set_active_instance_by_index(&self, index: usize, force: bool) {
        // Check for invalid value
        if index >= self.terminal_instances.borrow().len() {
            return;
        }

        let old_active_instance = self.active_instance();
        self.active_instance_index.set(index as isize);
        let active_instance = self.active_instance();
        let changed = match (&old_active_instance, &active_instance) {
            (None, None) => false,
            (Some(old), Some(new)) => !same_instance(old, new),
            _ => true,
        };
        if changed || force {
            self.on_instances_changed.fire(());
            self.on_did_change_active_instance.fire(active_instance);
        }
    }

    fn focus_previous_pane(&self) {
        let len = self.terminal_instances.borrow().len();
        let current = self.active_instance_index.get();
        let new_index = if current == 0 { len as isize - 1 } else { current - 1 };
        if let Ok(new_index) = usize::try_from(new_index) {
            self.set_active_instance_by_index(new_index, false);
        }
    }

    fn focus_next_pane(&self) {
        let len = self.terminal_instances.borrow().len();
        let current = self.active_instance_index.get();
        let new_index = if current == len as isize - 1 { 0 } else { current + 1 };
        if let Ok(new_index) = usize::try_from(new_index) {
            self.set_active_instance_by_index(new_index, false);
        }
    }

    fn split(&self, shell_launch_config: IShellLaunchConfig) -> Rc<dyn ITerminalInstance> {
        let parent_terminal_id = shell_launch_config.parent_terminal_id;
        let instance = self.terminal_instance_service.create_instance(shell_launch_config, TerminalLocation::Panel);
        self.add_instance_internal(TerminalGroupSource::Instance(instance.clone()), parent_terminal_id);
        self.set_active_instance(&instance);
        instance
    }

    fn add_disposable(&self, disposable: DisposableRef) {
        self.store.add(disposable);
    }
}

impl IDisposable for TerminalGroup {
    fn dispose(&self) {
        if self.store.is_disposed() {
            return;
        }
        self.terminal_instances.borrow_mut().clear();
        self.on_instances_changed.fire(());
        self.store.dispose();
    }
}
