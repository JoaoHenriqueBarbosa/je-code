//! Porte de `src/vs/platform/undoRedo/common/undoRedoService.ts`.
//!
//! O serviço guarda uma pilha de desfazer e refazer por recurso (chave de comparação: a URI em texto, ou o que um
//! `UriComparisonKeyComputer` registrado devolver), agrupa elementos por `UndoRedoGroup` e por `UndoRedoSource`, trava as
//! pilhas enquanto um desfazer ou refazer roda e entende elementos de um recurso ([`ResourceStackElement`]) e de
//! workspace ([`WorkspaceStackElement`], várias URIs numa operação só, com `split` e `prepareUndoRedo`).
//!
//! Decisões:
//! - **Assíncrono.** `Promise<void> | void` é [`MaybePromise`]. Os fluxos de recurso são síncronos quando o elemento é
//!   (o `undo()` de um `SingleModelEditStackElement` é), como no original; os de workspace e a confirmação são funções
//!   `async` (`async_fn_maybe`) que devolvem promessa. `IDialogService` e `INotificationService` são os serviços
//!   reais de `je-platform` (o resultado do diálogo é a promessa dele).
//! - **Pilhas compartilhadas.** `ResourceEditStack` é objeto de referência no original (o mapa, os instantâneos e os
//!   elementos de workspace apontam para a mesma pilha): aqui é [`EditStackRef`] (`Rc<RefCell<..>>`). O
//!   `missingEditStack` (singleton) é uma pilha nova por consulta: ninguém a modifica, a verificação sempre reprova
//!   antes de usá-la.
//! - **Direção.** `_undo` e `_redo` (e os pares `_checkWorkspaceUndo`/`_checkWorkspaceRedo`, `_tryToSplitAndUndo`/
//!   `_tryToSplitAndRedo`, `_workspaceUndo`/`_workspaceRedo`, `_resourceUndo`/`_resourceRedo`, `_continueUndoInGroup`/
//!   `_continueRedoInGroup`, `_findClosestUndoElementInGroup`/`_findClosestRedoElementInGroup` e os `...WithSource`) são
//!   duplicatas no original que só diferem no sentido; aqui é um método só parametrizado por `Direction`. Os textos e
//!   as chaves de `nls` continuam os de cada lado (`cannotWorkspaceUndo`, `cannotWorkspaceRedo`, ...).
//! - **Fora do porte.** `DEBUG`, `_print` e os `toString` (só existem para a depuração local do original) e as
//!   sobrecargas que aceitam a chave em texto no lugar da URI (`undo(string)`, `redo(string)`), que só o próprio
//!   serviço usa e que aqui são os métodos privados que recebem a chave.
//! - **Registro.** `registerSingleton(IUndoRedoService, UndoRedoService, InstantiationType.Delayed)` é [`register`].
// TODO(integração): mover para o crate je-platform (platform/undoRedo/common).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn_maybe;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, none, to_disposable};
use je_base::network::schemas as Schemas;
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::severity::Severity;
use je_base::uri::URI;
use je_instantiation::{Injectable, InstantiationType, register_singleton, service_impl};
use je_platform::dialogs::{
    BaseDialogOptions, CheckboxResult, Confirmation, ConfirmationResult, DialogTypeOrSeverity, IDialogService, Input, InputResult, Prompt, PromptBaseButton,
    PromptButton, PromptCancel, PromptCancelButton, PromptResult,
};
use je_platform::notification::{
    INotificationService, NoOpNotification, Notification, NotificationFilterSetting, NotificationHandle, NotificationMessage, NotificationSource,
    NotificationSourceFilter, NotificationsFilter, PromptChoiceKind, PromptOptions, StatusHandle, StatusMessageOptions,
};
use je_platform::resources::fs_path;

use crate::undo_redo::{ResourceEditStackSnapshot, UndoRedoElementRef, UndoRedoGroup, UndoRedoSource, UriComparisonKeyComputer, WorkspaceUndoRedoElement};

// Os tipos do contrato moraram aqui antes de o contrato ser portado; os usos existentes continuam valendo.
pub use crate::undo_redo::{IUndoRedoService, PastFutureElements, UndoRedoTarget};

thread_local! {
    static STACK_ELEMENT_COUNTER: Cell<u64> = const { Cell::new(0) };
}

fn next_stack_element_id() -> u64 {
    STACK_ELEMENT_COUNTER.with(|counter| {
        counter.set(counter.get() + 1);
        counter.get()
    })
}

/// `getResourceLabel(resource)`.
fn get_resource_label(resource: &URI) -> String {
    if resource.scheme() == Schemas::FILE { fs_path(resource) } else { resource.path().to_string_lossy() }
}

/// `Promise<void> | void` já resolvido.
fn done() -> MaybePromise<()> {
    MaybePromise::Value(())
}

/// O sentido da operação: `_undo` e `_redo` do original diferem só nisso.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Undo,
    Redo,
}

impl Direction {
    fn verb(self) -> &'static str {
        match self {
            Direction::Undo => "undo",
            Direction::Redo => "redo",
        }
    }

    /// A chave de `nls` de cada lado: `cannotWorkspaceUndo` e `cannotWorkspaceRedo`, e as demais.
    fn key(self, prefix: &str, suffix: &str) -> String {
        let verb = match self {
            Direction::Undo => "Undo",
            Direction::Redo => "Redo",
        };
        format!("{prefix}{verb}{suffix}")
    }
}

/// `RemovedResourceReason`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RemovedResourceReason {
    ExternalRemoval,
    NoParallelUniverses,
}

/// `ResourceReasonPair`.
#[derive(Clone)]
struct ResourceReasonPair {
    resource_label: String,
    reason: RemovedResourceReason,
}

/// `RemovedResources`: o `Map` do original itera por inserção.
#[derive(Clone, Default)]
struct RemovedResources {
    elements: Vec<(String, ResourceReasonPair)>,
}

impl RemovedResources {
    fn create_message(&self) -> String {
        let mut external_removal: Vec<&str> = Vec::new();
        let mut no_parallel_universes: Vec<&str> = Vec::new();
        for (_, element) in &self.elements {
            let dest = if element.reason == RemovedResourceReason::ExternalRemoval { &mut external_removal } else { &mut no_parallel_universes };
            dest.push(&element.resource_label);
        }

        let mut messages: Vec<String> = Vec::new();
        if !external_removal.is_empty() {
            messages.push(localize("externalRemoval", "The following files have been closed and modified on disk: {0}.", &[&external_removal.join(", ")]));
        }
        if !no_parallel_universes.is_empty() {
            messages.push(localize("noParallelUniverses", "The following files have been modified in an incompatible way: {0}.", &[&no_parallel_universes.join(", ")]));
        }
        messages.join("\n")
    }

    fn size(&self) -> usize {
        self.elements.len()
    }

    fn has(&self, str_resource: &str) -> bool {
        self.elements.iter().any(|(key, _)| key == str_resource)
    }

    fn set(&mut self, str_resource: &str, value: ResourceReasonPair) {
        match self.elements.iter_mut().find(|(key, _)| key == str_resource) {
            Some(entry) => entry.1 = value,
            None => self.elements.push((str_resource.to_owned(), value)),
        }
    }

    fn delete(&mut self, str_resource: &str) -> bool {
        match self.elements.iter().position(|(key, _)| key == str_resource) {
            Some(index) => {
                self.elements.remove(index);
                true
            }
            None => false,
        }
    }
}

/// `ResourceStackElement`.
struct ResourceStackElement {
    id: u64,
    actual: UndoRedoElementRef,
    label: String,
    confirm_before_undo: bool,
    resource_label: String,
    str_resource: String,
    group_id: u64,
    group_order: u64,
    source_id: u64,
    source_order: u64,
    is_valid: Cell<bool>,
}

impl ResourceStackElement {
    fn new(actual: UndoRedoElementRef, resource_label: String, str_resource: String, group_id: u64, group_order: u64, source_id: u64, source_order: u64) -> Self {
        Self {
            id: next_stack_element_id(),
            label: actual.label(),
            confirm_before_undo: actual.confirm_before_undo(),
            actual,
            resource_label,
            str_resource,
            group_id,
            group_order,
            source_id,
            source_order,
            is_valid: Cell::new(true),
        }
    }
}

/// `WorkspaceStackElement`.
struct WorkspaceStackElement {
    id: u64,
    actual: Rc<dyn WorkspaceUndoRedoElement>,
    label: String,
    confirm_before_undo: bool,
    resource_labels: Vec<String>,
    str_resources: Vec<String>,
    group_id: u64,
    group_order: u64,
    source_id: u64,
    source_order: u64,
    removed_resources: RefCell<Option<RemovedResources>>,
    invalidated_resources: RefCell<Option<RemovedResources>>,
}

impl WorkspaceStackElement {
    fn new(actual: Rc<dyn WorkspaceUndoRedoElement>, resource_labels: Vec<String>, str_resources: Vec<String>, group_id: u64, group_order: u64, source_id: u64, source_order: u64) -> Self {
        Self {
            id: next_stack_element_id(),
            label: actual.label(),
            confirm_before_undo: actual.confirm_before_undo(),
            actual,
            resource_labels,
            str_resources,
            group_id,
            group_order,
            source_id,
            source_order,
            removed_resources: RefCell::new(None),
            invalidated_resources: RefCell::new(None),
        }
    }

    /// `canSplit()`.
    fn can_split(&self) -> bool {
        self.actual.can_split()
    }

    /// `removeResource(resourceLabel, strResource, reason)`.
    fn remove_resource(&self, resource_label: &str, str_resource: &str, reason: RemovedResourceReason) {
        let mut removed = self.removed_resources.borrow_mut();
        let removed = removed.get_or_insert_with(RemovedResources::default);
        if !removed.has(str_resource) {
            removed.set(str_resource, ResourceReasonPair { resource_label: resource_label.to_owned(), reason });
        }
    }

    /// `setValid(resourceLabel, strResource, isValid)`.
    fn set_valid(&self, resource_label: &str, str_resource: &str, is_valid: bool) {
        let mut invalidated = self.invalidated_resources.borrow_mut();
        if is_valid {
            if let Some(resources) = invalidated.as_mut() {
                resources.delete(str_resource);
                if resources.size() == 0 {
                    *invalidated = None;
                }
            }
        } else {
            let resources = invalidated.get_or_insert_with(RemovedResources::default);
            if !resources.has(str_resource) {
                resources.set(str_resource, ResourceReasonPair { resource_label: resource_label.to_owned(), reason: RemovedResourceReason::ExternalRemoval });
            }
        }
    }
}

/// `StackElement`: `ResourceStackElement | WorkspaceStackElement`.
#[derive(Clone)]
enum StackElement {
    Resource(Rc<ResourceStackElement>),
    Workspace(Rc<WorkspaceStackElement>),
}

impl StackElement {
    fn id(&self) -> u64 {
        match self {
            Self::Resource(element) => element.id,
            Self::Workspace(element) => element.id,
        }
    }

    fn group_id(&self) -> u64 {
        match self {
            Self::Resource(element) => element.group_id,
            Self::Workspace(element) => element.group_id,
        }
    }

    fn group_order(&self) -> u64 {
        match self {
            Self::Resource(element) => element.group_order,
            Self::Workspace(element) => element.group_order,
        }
    }

    fn source_id(&self) -> u64 {
        match self {
            Self::Resource(element) => element.source_id,
            Self::Workspace(element) => element.source_id,
        }
    }

    fn source_order(&self) -> u64 {
        match self {
            Self::Resource(element) => element.source_order,
            Self::Workspace(element) => element.source_order,
        }
    }

    fn label(&self) -> &str {
        match self {
            Self::Resource(element) => &element.label,
            Self::Workspace(element) => &element.label,
        }
    }

    fn confirm_before_undo(&self) -> bool {
        match self {
            Self::Resource(element) => element.confirm_before_undo,
            Self::Workspace(element) => element.confirm_before_undo,
        }
    }

    fn str_resources(&self) -> Vec<String> {
        match self {
            Self::Resource(element) => vec![element.str_resource.clone()],
            Self::Workspace(element) => element.str_resources.clone(),
        }
    }

    fn resource_labels(&self) -> Vec<String> {
        match self {
            Self::Resource(element) => vec![element.resource_label.clone()],
            Self::Workspace(element) => element.resource_labels.clone(),
        }
    }

    /// `element.actual`.
    fn actual(&self) -> UndoRedoElementRef {
        match self {
            Self::Resource(element) => element.actual.clone(),
            Self::Workspace(element) => UndoRedoElementRef::Workspace(element.actual.clone()),
        }
    }

    /// `a === b`.
    fn ptr_eq(&self, other: &StackElement) -> bool {
        match (self, other) {
            (Self::Resource(a), Self::Resource(b)) => Rc::ptr_eq(a, b),
            (Self::Workspace(a), Self::Workspace(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// `a === b` onde `a` pode ser `null`.
fn is_same(a: &Option<StackElement>, b: &StackElement) -> bool {
    a.as_ref().is_some_and(|a| a.ptr_eq(b))
}

/// `ResourceEditStack`.
struct ResourceEditStack {
    resource_label: String,
    str_resource: String,
    past: Vec<StackElement>,
    future: Vec<StackElement>,
    locked: bool,
    version_id: u64,
}

type EditStackRef = Rc<RefCell<ResourceEditStack>>;

impl ResourceEditStack {
    fn new(resource_label: &str, str_resource: &str) -> Self {
        Self {
            resource_label: resource_label.to_owned(),
            str_resource: str_resource.to_owned(),
            past: Vec::new(),
            future: Vec::new(),
            locked: false,
            version_id: 1,
        }
    }

    /// Os elementos de workspace do passado e do futuro (que são os que sabem dos recursos removidos).
    fn workspace_elements(&self) -> impl Iterator<Item = &Rc<WorkspaceStackElement>> {
        self.past.iter().chain(self.future.iter()).filter_map(|element| match element {
            StackElement::Workspace(element) => Some(element),
            StackElement::Resource(_) => None,
        })
    }

    fn dispose(&mut self) {
        for element in self.workspace_elements() {
            element.remove_resource(&self.resource_label, &self.str_resource, RemovedResourceReason::ExternalRemoval);
        }
        self.version_id += 1;
    }

    fn flush_all_elements(&mut self) {
        self.past = Vec::new();
        self.future = Vec::new();
        self.version_id += 1;
    }

    /// `_setElementValidFlag(element, isValid)`.
    fn set_element_valid_flag(&self, element: &StackElement, is_valid: bool) {
        match element {
            StackElement::Workspace(element) => element.set_valid(&self.resource_label, &self.str_resource, is_valid),
            StackElement::Resource(element) => element.is_valid.set(is_valid),
        }
    }

    fn set_elements_valid_flag(&self, is_valid: bool, filter: &dyn Fn(&UndoRedoElementRef) -> bool) {
        for element in self.past.iter().chain(self.future.iter()) {
            if filter(&element.actual()) {
                self.set_element_valid_flag(element, is_valid);
            }
        }
    }

    fn push_element(&mut self, element: StackElement) {
        // remove the future
        for future_element in &self.future {
            if let StackElement::Workspace(future_element) = future_element {
                future_element.remove_resource(&self.resource_label, &self.str_resource, RemovedResourceReason::NoParallelUniverses);
            }
        }
        self.future = Vec::new();
        self.past.push(element);
        self.version_id += 1;
    }

    fn create_snapshot(&self, resource: &URI) -> ResourceEditStackSnapshot {
        let mut elements: Vec<u64> = self.past.iter().map(StackElement::id).collect();
        elements.extend(self.future.iter().rev().map(StackElement::id));
        ResourceEditStackSnapshot { resource: resource.clone(), elements }
    }

    fn restore_snapshot(&mut self, snapshot: &ResourceEditStackSnapshot) {
        let snapshot_length = snapshot.elements.len();
        let mut is_ok = true;
        let mut snapshot_index = 0usize;
        let mut remove_past_after: Option<usize> = None;
        for (i, element) in self.past.iter().enumerate() {
            if is_ok && (snapshot_index >= snapshot_length || element.id() != snapshot.elements[snapshot_index]) {
                is_ok = false;
                remove_past_after = Some(i);
            }
            if !is_ok && let StackElement::Workspace(element) = element {
                element.remove_resource(&self.resource_label, &self.str_resource, RemovedResourceReason::ExternalRemoval);
            }
            snapshot_index += 1;
        }
        let mut remove_future_before: Option<usize> = None;
        for (i, element) in self.future.iter().enumerate().rev() {
            if is_ok && (snapshot_index >= snapshot_length || element.id() != snapshot.elements[snapshot_index]) {
                is_ok = false;
                remove_future_before = Some(i);
            }
            if !is_ok && let StackElement::Workspace(element) = element {
                element.remove_resource(&self.resource_label, &self.str_resource, RemovedResourceReason::ExternalRemoval);
            }
            snapshot_index += 1;
        }
        if let Some(after) = remove_past_after {
            self.past.truncate(after);
        }
        if let Some(before) = remove_future_before {
            self.future.drain(..=before);
        }
        self.version_id += 1;
    }

    fn get_elements(&self) -> PastFutureElements {
        PastFutureElements {
            past: self.past.iter().map(StackElement::actual).collect(),
            future: self.future.iter().map(StackElement::actual).collect(),
        }
    }

    fn closest_past_element(&self) -> Option<StackElement> {
        self.past.last().cloned()
    }

    fn second_closest_past_element(&self) -> Option<StackElement> {
        if self.past.len() < 2 {
            return None;
        }
        self.past.get(self.past.len() - 2).cloned()
    }

    fn closest_future_element(&self) -> Option<StackElement> {
        self.future.last().cloned()
    }

    /// O elemento pronto para a operação: o mais próximo do passado (desfazer) ou do futuro (refazer).
    fn closest_element(&self, direction: Direction) -> Option<StackElement> {
        match direction {
            Direction::Undo => self.closest_past_element(),
            Direction::Redo => self.closest_future_element(),
        }
    }

    fn has_past_elements(&self) -> bool {
        !self.past.is_empty()
    }

    fn has_future_elements(&self) -> bool {
        !self.future.is_empty()
    }

    /// `splitPastWorkspaceElement` (`direction` desfazer) e `splitFutureWorkspaceElement` (refazer).
    fn split_workspace_element(&mut self, direction: Direction, to_remove: &Rc<WorkspaceStackElement>, individual_map: &HashMap<String, Rc<ResourceStackElement>>) {
        let elements = match direction {
            Direction::Undo => &mut self.past,
            Direction::Redo => &mut self.future,
        };
        for j in (0..elements.len()).rev() {
            if matches!(&elements[j], StackElement::Workspace(element) if Rc::ptr_eq(element, to_remove)) {
                match individual_map.get(&self.str_resource) {
                    // gets replaced
                    Some(individual) => elements[j] = StackElement::Resource(individual.clone()),
                    // gets deleted
                    None => {
                        elements.remove(j);
                    }
                }
                break;
            }
        }
        self.version_id += 1;
    }

    /// `moveBackward(element)` (desfazer) e `moveForward(element)` (refazer).
    fn move_in(&mut self, direction: Direction, element: &StackElement) {
        match direction {
            Direction::Undo => {
                self.past.pop();
                self.future.push(element.clone());
            }
            Direction::Redo => {
                self.future.pop();
                self.past.push(element.clone());
            }
        }
        self.version_id += 1;
    }
}

/// `EditStackSnapshot`: as pilhas afetadas e as versões delas no momento da criação.
#[derive(Clone)]
struct EditStackSnapshot {
    edit_stacks: Vec<EditStackRef>,
    version_ids: Vec<u64>,
}

impl EditStackSnapshot {
    fn new(edit_stacks: Vec<EditStackRef>) -> Self {
        let version_ids = edit_stacks.iter().map(|edit_stack| edit_stack.borrow().version_id).collect();
        Self { edit_stacks, version_ids }
    }

    fn is_valid(&self) -> bool {
        self.edit_stacks.iter().zip(&self.version_ids).all(|(edit_stack, version_id)| edit_stack.borrow().version_id == *version_id)
    }
}

/// As pilhas por recurso, na ordem em que apareceram (o `Map` do JavaScript itera por inserção).
#[derive(Default)]
struct EditStacks {
    order: Vec<String>,
    stacks: HashMap<String, EditStackRef>,
}

impl EditStacks {
    fn get(&self, str_resource: &str) -> Option<EditStackRef> {
        self.stacks.get(str_resource).cloned()
    }

    fn get_or_create(&mut self, resource_label: &str, str_resource: &str) -> EditStackRef {
        if let Some(edit_stack) = self.stacks.get(str_resource) {
            return edit_stack.clone();
        }
        let edit_stack: EditStackRef = Rc::new(RefCell::new(ResourceEditStack::new(resource_label, str_resource)));
        self.order.push(str_resource.to_owned());
        self.stacks.insert(str_resource.to_owned(), edit_stack.clone());
        edit_stack
    }

    fn remove(&mut self, str_resource: &str) -> Option<EditStackRef> {
        let removed = self.stacks.remove(str_resource);
        if removed.is_some() {
            self.order.retain(|key| key != str_resource);
        }
        removed
    }

    /// O candidato (o mais próximo do passado ou do futuro de cada pilha) escolhido por `better`, na ordem de inserção.
    fn find_closest(
        &self,
        direction: Direction,
        matches: impl Fn(&StackElement) -> bool,
        better: impl Fn(&StackElement, &StackElement) -> bool,
    ) -> Option<(StackElement, String)> {
        let mut matched: Option<(StackElement, String)> = None;
        for key in &self.order {
            let Some(candidate) = self.stacks[key].borrow().closest_element(direction) else {
                continue;
            };
            if matches(&candidate) && matched.as_ref().is_none_or(|(current, _)| better(&candidate, current)) {
                matched = Some((candidate, key.clone()));
            }
        }
        matched
    }
}

/// `WorkspaceVerificationError`: o que a operação devolve quando a verificação reprova.
struct WorkspaceVerificationError {
    return_value: MaybePromise<()>,
}

/// `UndoChoice`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum UndoChoice {
    All = 0,
    This = 1,
    Cancel = 2,
}

impl UndoChoice {
    fn value(self) -> Dynamic {
        Dynamic::Number(f64::from(self as u8))
    }

    fn from_dynamic(value: &Option<Dynamic>) -> Option<Self> {
        match value {
            Some(Dynamic::Number(number)) if *number == 0.0 => Some(Self::All),
            Some(Dynamic::Number(number)) if *number == 1.0 => Some(Self::This),
            Some(Dynamic::Number(number)) if *number == 2.0 => Some(Self::Cancel),
            _ => None,
        }
    }
}

/// `UndoRedoService`.
pub struct UndoRedoService {
    me: Weak<UndoRedoService>,
    edit_stacks: RefCell<EditStacks>,
    uri_comparison_key_computers: Rc<RefCell<Vec<(String, Rc<dyn UriComparisonKeyComputer>)>>>,
    dialog_service: Rc<dyn IDialogService>,
    notification_service: Rc<dyn INotificationService>,
}

impl UndoRedoService {
    /// `constructor(@IDialogService dialogService, @INotificationService notificationService)`.
    pub fn new(dialog_service: Rc<dyn IDialogService>, notification_service: Rc<dyn INotificationService>) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            edit_stacks: RefCell::new(EditStacks::default()),
            uri_comparison_key_computers: Rc::new(RefCell::new(Vec::new())),
            dialog_service,
            notification_service,
        })
    }

    /// `createUndoRedoService()` do teste do original: `TestDialogService` (não confirma) e `TestNotificationService`.
    pub fn for_tests() -> Rc<Self> {
        Self::new(Rc::new(DefaultTestDialogService), Rc::new(SilentNotificationService))
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço de desfazer e refazer está vivo enquanto é usado")
    }

    fn get_edit_stack(&self, str_resource: &str) -> Option<EditStackRef> {
        self.edit_stacks.borrow().get(str_resource)
    }

    /// `hasElements`.
    pub fn has_elements(&self, resource: &URI) -> bool {
        let key = self.get_uri_comparison_key(resource);
        self.get_edit_stack(&key).is_some_and(|edit_stack| {
            let edit_stack = edit_stack.borrow();
            edit_stack.has_past_elements() || edit_stack.has_future_elements()
        })
    }

    /// `_pushElement(element)`.
    fn push_stack_element(&self, element: StackElement) {
        let resource_labels = element.resource_labels();
        for (resource_label, str_resource) in resource_labels.iter().zip(element.str_resources()) {
            let edit_stack = self.edit_stacks.borrow_mut().get_or_create(resource_label, &str_resource);
            edit_stack.borrow_mut().push_element(element.clone());
        }
    }

    /// `_splitPastWorkspaceElement` e `_splitFutureWorkspaceElement`.
    fn split_workspace_element(&self, direction: Direction, to_remove: &Rc<WorkspaceStackElement>, ignore_resources: Option<&RemovedResources>) {
        let individual_arr = to_remove.actual.split().unwrap_or_else(|| errors::throw(Error::new("element.actual.split is not a function")));
        let mut individual_map: HashMap<String, Rc<ResourceStackElement>> = HashMap::new();
        for individual in individual_arr {
            let resource = individual.resource();
            let resource_label = get_resource_label(&resource);
            let str_resource = self.get_uri_comparison_key(&resource);
            let element = Rc::new(ResourceStackElement::new(UndoRedoElementRef::Resource(individual), resource_label, str_resource.clone(), 0, 0, 0, 0));
            individual_map.insert(str_resource, element);
        }

        for str_resource in &to_remove.str_resources {
            if ignore_resources.is_some_and(|ignore| ignore.has(str_resource)) {
                continue;
            }
            let edit_stack = self
                .get_edit_stack(str_resource)
                .unwrap_or_else(|| errors::throw(Error::new("Cannot read properties of undefined (reading 'splitWorkspaceElement')")));
            edit_stack.borrow_mut().split_workspace_element(direction, to_remove, &individual_map);
        }
    }

    fn remove_elements_by_key(&self, str_resource: &str) {
        let removed = self.edit_stacks.borrow_mut().remove(str_resource);
        if let Some(edit_stack) = removed {
            edit_stack.borrow_mut().dispose();
        }
    }

    /// `_findClosestUndoElementWithSource` e `_findClosestRedoElementWithSource`: a chave da pilha do elemento da origem com
    /// a maior `sourceOrder` pronto para desfazer, ou a menor pronto para refazer.
    fn find_closest_with_source(&self, direction: Direction, source_id: u64) -> Option<String> {
        if source_id == 0 {
            return None;
        }
        self.edit_stacks
            .borrow()
            .find_closest(
                direction,
                |candidate| candidate.source_id() == source_id,
                |candidate, current| match direction {
                    Direction::Undo => candidate.source_order() > current.source_order(),
                    Direction::Redo => candidate.source_order() < current.source_order(),
                },
            )
            .map(|(_, key)| key)
    }

    /// `_findClosestUndoElementInGroup` e `_findClosestRedoElementInGroup`: outro elemento do mesmo grupo com a maior
    /// `groupOrder` pronto para desfazer, ou a menor pronto para refazer.
    fn find_closest_in_group(&self, direction: Direction, group_id: u64) -> Option<(StackElement, String)> {
        if group_id == 0 {
            return None;
        }
        self.edit_stacks.borrow().find_closest(
            direction,
            |candidate| candidate.group_id() == group_id,
            |candidate, current| match direction {
                Direction::Undo => candidate.group_order() > current.group_order(),
                Direction::Redo => candidate.group_order() < current.group_order(),
            },
        )
    }

    fn warn(&self, message: String) {
        self.notification_service.warn(vec![NotificationMessage::Text(message)]);
    }

    /// `_onError`: um erro ao desfazer ou refazer derruba a pilha de todos os recursos afetados.
    fn on_error(&self, err: &Error, element: &StackElement) -> MaybePromise<()> {
        errors::on_unexpected_error(err);
        // An error occurred while undoing or redoing => drop the undo/redo stack for all affected resources
        for str_resource in element.str_resources() {
            self.remove_elements_by_key(&str_resource);
        }
        self.notification_service.error(vec![NotificationMessage::Error(err.clone())]);
        done()
    }

    /// `_acquireLocks(editStackSnapshot)`: devolve a função que libera as travas.
    fn acquire_locks(edit_stack_snapshot: &EditStackSnapshot) -> Box<dyn FnOnce()> {
        // first, check if all locks can be acquired
        for edit_stack in &edit_stack_snapshot.edit_stacks {
            if edit_stack.borrow().locked {
                errors::throw(Error::new("Cannot acquire edit stack lock"));
            }
        }

        // can acquire all locks
        for edit_stack in &edit_stack_snapshot.edit_stacks {
            edit_stack.borrow_mut().locked = true;
        }

        let edit_stacks = edit_stack_snapshot.edit_stacks.clone();
        Box::new(move || {
            // release all locks
            for edit_stack in &edit_stacks {
                edit_stack.borrow_mut().locked = false;
            }
        })
    }

    /// `_safeInvokeWithLocks(element, invoke, editStackSnapshot, cleanup, continuation)`.
    fn safe_invoke_with_locks(
        &self,
        element: StackElement,
        invoke: impl FnOnce() -> MaybePromise<()>,
        edit_stack_snapshot: &EditStackSnapshot,
        cleanup: DisposableRef,
        continuation: impl FnOnce() -> MaybePromise<()> + 'static,
    ) -> MaybePromise<()> {
        let release_locks = Self::acquire_locks(edit_stack_snapshot);

        let result = match errors::catch(invoke) {
            Ok(result) => result,
            Err(err) => {
                release_locks();
                cleanup.dispose();
                return self.on_error(&err, &element);
            }
        };

        match result {
            // result is Promise<void>
            MaybePromise::Promise(promise) => {
                let this = self.rc();
                MaybePromise::Promise(promise.then_result_maybe(move |outcome| {
                    release_locks();
                    cleanup.dispose();
                    match outcome {
                        Ok(()) => continuation(),
                        Err(err) => this.on_error(&err, &element),
                    }
                }))
            }
            // result is void
            MaybePromise::Value(()) => {
                release_locks();
                cleanup.dispose();
                continuation()
            }
        }
    }

    /// `_invokeWorkspacePrepare(element)`.
    fn invoke_workspace_prepare(element: &Rc<WorkspaceStackElement>) -> Promise<DisposableRef> {
        let element = element.clone();
        async_fn_maybe(async move { Ok(element.actual.prepare_undo_redo().unwrap_or_else(|| MaybePromise::Value(none()))) })
    }

    /// `_invokeResourcePrepare(element, callback)`.
    fn invoke_resource_prepare(element: &Rc<ResourceStackElement>, callback: impl FnOnce(DisposableRef) -> MaybePromise<()> + 'static) -> MaybePromise<()> {
        let UndoRedoElementRef::Workspace(actual) = &element.actual else {
            // no preparation needed
            return callback(none());
        };
        match actual.prepare_undo_redo() {
            // nothing to clean up
            None => callback(none()),
            Some(MaybePromise::Value(disposable)) => callback(disposable),
            Some(MaybePromise::Promise(promise)) => MaybePromise::Promise(promise.then_maybe(callback)),
        }
    }

    /// `_getAffectedEditStacks(element)`.
    fn get_affected_edit_stacks(&self, element: &Rc<WorkspaceStackElement>) -> EditStackSnapshot {
        let affected_edit_stacks = element
            .str_resources
            .iter()
            .map(|str_resource| {
                self.get_edit_stack(str_resource).unwrap_or_else(|| {
                    // `missingEditStack`
                    let mut missing = ResourceEditStack::new("", "");
                    missing.locked = true;
                    Rc::new(RefCell::new(missing))
                })
            })
            .collect();
        EditStackSnapshot::new(affected_edit_stacks)
    }

    /// `_tryToSplitAndUndo` e `_tryToSplitAndRedo`.
    fn try_to_split_and_run(
        &self,
        direction: Direction,
        str_resource: &str,
        element: &Rc<WorkspaceStackElement>,
        ignore_resources: Option<&RemovedResources>,
        message: String,
    ) -> WorkspaceVerificationError {
        if element.can_split() {
            self.split_workspace_element(direction, element, ignore_resources);
            self.warn(message);
            WorkspaceVerificationError { return_value: self.run(direction, str_resource, 0, true) }
        } else {
            // Cannot safely split this workspace element => flush all undo/redo stacks
            for str_resource in &element.str_resources {
                self.remove_elements_by_key(str_resource);
            }
            self.warn(message);
            WorkspaceVerificationError { return_value: done() }
        }
    }

    /// `_checkWorkspaceUndo` e `_checkWorkspaceRedo`.
    fn check_workspace(
        &self,
        direction: Direction,
        str_resource: &str,
        element: &Rc<WorkspaceStackElement>,
        edit_stack_snapshot: &EditStackSnapshot,
        check_invalidated_resources: bool,
    ) -> Option<WorkspaceVerificationError> {
        let verb = direction.verb();
        let label = element.label.as_str();

        let removed_resources = element.removed_resources.borrow().clone();
        if let Some(removed_resources) = removed_resources {
            let message = localize(
                &direction.key("cannotWorkspace", ""),
                &format!("Could not {verb} '{{0}}' across all files. {{1}}"),
                &[label, &removed_resources.create_message()],
            );
            return Some(self.try_to_split_and_run(direction, str_resource, element, Some(&removed_resources), message));
        }
        let invalidated_resources = element.invalidated_resources.borrow().clone();
        if check_invalidated_resources && let Some(invalidated_resources) = invalidated_resources {
            let message = localize(
                &direction.key("cannotWorkspace", ""),
                &format!("Could not {verb} '{{0}}' across all files. {{1}}"),
                &[label, &invalidated_resources.create_message()],
            );
            return Some(self.try_to_split_and_run(direction, str_resource, element, Some(&invalidated_resources), message));
        }

        // this must be the last past (or future) element in all the impacted resources!
        let this_element = StackElement::Workspace(element.clone());
        let cannot_run_due_to_resources: Vec<String> = edit_stack_snapshot
            .edit_stacks
            .iter()
            .filter(|edit_stack| !is_same(&edit_stack.borrow().closest_element(direction), &this_element))
            .map(|edit_stack| edit_stack.borrow().resource_label.clone())
            .collect();
        if !cannot_run_due_to_resources.is_empty() {
            let message = localize(
                &direction.key("cannotWorkspace", "DueToChanges"),
                &format!("Could not {verb} '{{0}}' across all files because changes were made to {{1}}"),
                &[label, &cannot_run_due_to_resources.join(", ")],
            );
            return Some(self.try_to_split_and_run(direction, str_resource, element, None, message));
        }

        let cannot_lock_due_to_resources: Vec<String> = edit_stack_snapshot
            .edit_stacks
            .iter()
            .filter(|edit_stack| edit_stack.borrow().locked)
            .map(|edit_stack| edit_stack.borrow().resource_label.clone())
            .collect();
        if !cannot_lock_due_to_resources.is_empty() {
            let message = localize(
                &direction.key("cannotWorkspace", "DueToInProgressUndoRedo"),
                &format!("Could not {verb} '{{0}}' across all files because there is already an undo or redo operation running on {{1}}"),
                &[label, &cannot_lock_due_to_resources.join(", ")],
            );
            return Some(self.try_to_split_and_run(direction, str_resource, element, None, message));
        }

        // check if new stack elements were added in the meantime...
        if !edit_stack_snapshot.is_valid() {
            let message = localize(
                &direction.key("cannotWorkspace", "DueToInMeantimeUndoRedo"),
                &format!("Could not {verb} '{{0}}' across all files because an undo or redo operation occurred in the meantime"),
                &[label],
            );
            return Some(self.try_to_split_and_run(direction, str_resource, element, None, message));
        }

        None
    }

    /// `_workspaceUndo` e `_workspaceRedo`.
    fn workspace_run(&self, direction: Direction, str_resource: &str, element: &Rc<WorkspaceStackElement>, undo_confirmed: bool) -> MaybePromise<()> {
        let affected_edit_stacks = self.get_affected_edit_stacks(element);
        // invalidated resources will be checked after the prepare call
        if let Some(verification_error) = self.check_workspace(direction, str_resource, element, &affected_edit_stacks, false) {
            return verification_error.return_value;
        }
        MaybePromise::Promise(self.confirm_and_execute_workspace(direction, str_resource.to_owned(), element.clone(), affected_edit_stacks, undo_confirmed))
    }

    /// `_isPartOfUndoGroup(element)`.
    fn is_part_of_undo_group(&self, element: &Rc<WorkspaceStackElement>) -> bool {
        if element.group_id == 0 {
            return false;
        }
        let this_element = StackElement::Workspace(element.clone());
        // check that there is at least another element with the same groupId ready to be undone
        let edit_stacks = self.edit_stacks.borrow();
        for key in &edit_stacks.order {
            let edit_stack = edit_stacks.stacks[key].borrow();
            let Some(past_element) = edit_stack.closest_past_element() else {
                continue;
            };
            if past_element.ptr_eq(&this_element)
                && let Some(second_past_element) = edit_stack.second_closest_past_element()
                && second_past_element.group_id() == element.group_id
            {
                // there is another element with the same group id in the same stack!
                return true;
            }
            if past_element.group_id() == element.group_id {
                // there is another element with the same group id in another stack!
                return true;
            }
        }
        false
    }

    /// O `prompt` de `_confirmAndExecuteWorkspaceUndo`: desfazer em todos os arquivos, só neste ou cancelar.
    fn prompt_workspace_undo(&self, element: &Rc<WorkspaceStackElement>, files: usize) -> Promise<PromptResult> {
        let button = |label: String, choice: UndoChoice| PromptButton {
            label,
            base: PromptBaseButton { run: Rc::new(move |_| MaybePromise::Value(choice.value())) },
        };
        let mut base = BaseDialogOptions::new(localize("confirmWorkspace", "Would you like to undo '{0}' across all files?", &[&element.label]));
        base.dialog_type = Some(DialogTypeOrSeverity::Severity(Severity::Info));
        self.dialog_service.prompt(Prompt {
            base,
            buttons: Some(vec![
                button(localize("ok", "&&Undo in {0} Files", &[&files.to_string()]), UndoChoice::All),
                button(localize("nok", "Undo this &&File", &[]), UndoChoice::This),
            ]),
            cancel_button: Some(PromptCancel::Button(PromptCancelButton {
                label: None,
                base: PromptBaseButton { run: Rc::new(|_| MaybePromise::Value(UndoChoice::Cancel.value())) },
            })),
        })
    }

    /// `_confirmAndExecuteWorkspaceUndo` e `_executeWorkspaceRedo` (que é o mesmo sem a confirmação).
    fn confirm_and_execute_workspace(
        &self,
        direction: Direction,
        str_resource: String,
        element: Rc<WorkspaceStackElement>,
        edit_stack_snapshot: EditStackSnapshot,
        undo_confirmed: bool,
    ) -> Promise<()> {
        let this = self.rc();
        async_fn_maybe(async move {
            let mut undo_confirmed = undo_confirmed;
            let stack_element = StackElement::Workspace(element.clone());

            if direction == Direction::Undo && element.can_split() && !this.is_part_of_undo_group(&element) {
                // this element can be split
                let prompt_result = this.prompt_workspace_undo(&element, edit_stack_snapshot.edit_stacks.len()).await?;
                let choice = UndoChoice::from_dynamic(&prompt_result.result);

                if choice == Some(UndoChoice::Cancel) {
                    // choice: cancel
                    return Ok(done());
                }

                if choice == Some(UndoChoice::This) {
                    // choice: undo this file
                    this.split_workspace_element(Direction::Undo, &element, None);
                    return Ok(this.run(Direction::Undo, &str_resource, 0, true));
                }

                // choice: undo in all files

                // At this point, it is possible that the element has been made invalid in the meantime (due to the confirmation await)
                // (invalidated resources will be checked after the prepare call)
                if let Some(verification_error) = this.check_workspace(direction, &str_resource, &element, &edit_stack_snapshot, false) {
                    return Ok(verification_error.return_value);
                }

                undo_confirmed = true;
            }

            // prepare
            let cleanup = match Self::invoke_workspace_prepare(&element).await {
                Ok(cleanup) => cleanup,
                Err(err) => return Ok(this.on_error(&err, &stack_element)),
            };

            // At this point, it is possible that the element has been made invalid in the meantime (due to the prepare await)
            // (now also check that there are no more invalidated resources)
            if let Some(verification_error) = this.check_workspace(direction, &str_resource, &element, &edit_stack_snapshot, true) {
                cleanup.dispose();
                return Ok(verification_error.return_value);
            }

            for edit_stack in &edit_stack_snapshot.edit_stacks {
                edit_stack.borrow_mut().move_in(direction, &stack_element);
            }
            let actual = element.actual.clone();
            let group_id = element.group_id;
            let continuation_service = this.clone();
            Ok(this.safe_invoke_with_locks(
                stack_element,
                move || if direction == Direction::Undo { actual.undo() } else { actual.redo() },
                &edit_stack_snapshot,
                cleanup,
                move || continuation_service.continue_in_group(direction, group_id, undo_confirmed),
            ))
        })
    }

    /// `_resourceUndo` e `_resourceRedo`.
    fn resource_run(&self, direction: Direction, edit_stack: &EditStackRef, element: &Rc<ResourceStackElement>, undo_confirmed: bool) -> MaybePromise<()> {
        if !element.is_valid.get() {
            // invalid element => immediately flush edit stack!
            edit_stack.borrow_mut().flush_all_elements();
            return done();
        }
        if edit_stack.borrow().locked {
            self.warn(localize(
                &direction.key("cannotResource", "DueToInProgressUndoRedo"),
                &format!("Could not {} '{{0}}' because there is already an undo or redo operation running.", direction.verb()),
                &[&element.label],
            ));
            return done();
        }

        let this = self.rc();
        let edit_stack = edit_stack.clone();
        let prepared_element = element.clone();
        Self::invoke_resource_prepare(element, move |cleanup| {
            let stack_element = StackElement::Resource(prepared_element.clone());
            edit_stack.borrow_mut().move_in(direction, &stack_element);
            let snapshot = EditStackSnapshot::new(vec![edit_stack.clone()]);
            let actual = prepared_element.actual.clone();
            let group_id = prepared_element.group_id;
            let continuation_service = this.clone();
            this.safe_invoke_with_locks(
                stack_element,
                move || if direction == Direction::Undo { actual.undo() } else { actual.redo() },
                &snapshot,
                cleanup,
                move || continuation_service.continue_in_group(direction, group_id, undo_confirmed),
            )
        })
    }

    /// `_continueUndoInGroup` e `_continueRedoInGroup`.
    fn continue_in_group(&self, direction: Direction, group_id: u64, undo_confirmed: bool) -> MaybePromise<()> {
        if group_id == 0 {
            return done();
        }

        match self.find_closest_in_group(direction, group_id) {
            Some((_, matched_str_resource)) => self.run(direction, &matched_str_resource, 0, undo_confirmed),
            None => done(),
        }
    }

    /// `_undo(strResource, sourceId, undoConfirmed)` e `_redo(strResource)`: `source_id` e `undo_confirmed` só valem ao
    /// desfazer.
    fn run(&self, direction: Direction, str_resource: &str, source_id: u64, undo_confirmed: bool) -> MaybePromise<()> {
        let Some(edit_stack) = self.get_edit_stack(str_resource) else {
            return done();
        };
        let closest = edit_stack.borrow().closest_element(direction);
        let Some(element) = closest else {
            return done();
        };

        if element.group_id() != 0 {
            // this element is a part of a group, we need to make sure undoing (or redoing) in a group is in order
            if let Some((matched_element, matched_str_resource)) = self.find_closest_in_group(direction, element.group_id())
                && !element.ptr_eq(&matched_element)
            {
                // there is an element in the same group that should be run before this one
                return self.run(direction, &matched_str_resource, source_id, undo_confirmed);
            }
        }

        if direction == Direction::Undo {
            let should_prompt_for_confirmation = element.source_id() != source_id || element.confirm_before_undo();
            if should_prompt_for_confirmation && !undo_confirmed {
                // Hit a different source or the element asks for prompt before undo, prompt for confirmation
                return MaybePromise::Promise(self.confirm_and_continue_undo(str_resource.to_owned(), source_id, element));
            }
        }

        match &element {
            StackElement::Workspace(workspace_element) => self.workspace_run(direction, str_resource, workspace_element, undo_confirmed),
            StackElement::Resource(resource_element) => self.resource_run(direction, &edit_stack, resource_element, undo_confirmed),
        }
    }

    /// `_confirmAndContinueUndo(strResource, sourceId, element)`.
    fn confirm_and_continue_undo(&self, str_resource: String, source_id: u64, element: StackElement) -> Promise<()> {
        let this = self.rc();
        async_fn_maybe(async move {
            let result = this
                .dialog_service
                .confirm(Confirmation {
                    base: BaseDialogOptions::new(localize("confirmDifferentSource", "Would you like to undo '{0}'?", &[element.label()])),
                    primary_button: Some(localize("confirmDifferentSource.yes", "&&Yes", &[])),
                    cancel_button: Some(localize("confirmDifferentSource.no", "No", &[])),
                })
                .await?;

            if !result.confirmed {
                return Ok(done());
            }

            Ok(this.run(Direction::Undo, &str_resource, source_id, true))
        })
    }
}

impl IUndoRedoService for UndoRedoService {
    fn register_uri_comparison_key_computer(&self, scheme: &str, uri_comparison_key_computer: Rc<dyn UriComparisonKeyComputer>) -> DisposableRef {
        self.uri_comparison_key_computers.borrow_mut().push((scheme.to_owned(), uri_comparison_key_computer.clone()));
        let computers = self.uri_comparison_key_computers.clone();
        to_disposable(move || {
            let mut computers = computers.borrow_mut();
            if let Some(index) = computers.iter().position(|(_, computer)| Rc::ptr_eq(computer, &uri_comparison_key_computer)) {
                computers.remove(index);
            }
        })
    }

    fn get_uri_comparison_key(&self, resource: &URI) -> String {
        let computer = self
            .uri_comparison_key_computers
            .borrow()
            .iter()
            .find(|(scheme, _)| resource.scheme() == scheme.as_str())
            .map(|(_, computer)| computer.clone());
        match computer {
            Some(computer) => computer.get_comparison_key(resource),
            None => resource.to_string(false).to_string_lossy(),
        }
    }

    fn push_element(&self, element: UndoRedoElementRef, group: Option<&UndoRedoGroup>, source: Option<&UndoRedoSource>) {
        let none_group = UndoRedoGroup::none();
        let none_source = UndoRedoSource::none();
        let group = group.unwrap_or(&none_group);
        let source = source.unwrap_or(&none_source);
        match &element {
            UndoRedoElementRef::Resource(resource_element) => {
                let resource = resource_element.resource();
                let resource_label = get_resource_label(&resource);
                let str_resource = self.get_uri_comparison_key(&resource);
                self.push_stack_element(StackElement::Resource(Rc::new(ResourceStackElement::new(
                    element.clone(),
                    resource_label,
                    str_resource,
                    group.id,
                    group.next_order(),
                    source.id,
                    source.next_order(),
                ))));
            }
            UndoRedoElementRef::Workspace(workspace_element) => {
                let mut seen: Vec<String> = Vec::new();
                let mut resource_labels: Vec<String> = Vec::new();
                let mut str_resources: Vec<String> = Vec::new();
                for resource in workspace_element.resources() {
                    let resource_label = get_resource_label(&resource);
                    let str_resource = self.get_uri_comparison_key(&resource);

                    if seen.contains(&str_resource) {
                        continue;
                    }
                    seen.push(str_resource.clone());
                    resource_labels.push(resource_label);
                    str_resources.push(str_resource);
                }

                if resource_labels.len() == 1 {
                    self.push_stack_element(StackElement::Resource(Rc::new(ResourceStackElement::new(
                        element.clone(),
                        resource_labels.remove(0),
                        str_resources.remove(0),
                        group.id,
                        group.next_order(),
                        source.id,
                        source.next_order(),
                    ))));
                } else {
                    self.push_stack_element(StackElement::Workspace(Rc::new(WorkspaceStackElement::new(
                        workspace_element.clone(),
                        resource_labels,
                        str_resources,
                        group.id,
                        group.next_order(),
                        source.id,
                        source.next_order(),
                    ))));
                }
            }
        }
    }

    fn get_last_element(&self, resource: &URI) -> Option<UndoRedoElementRef> {
        let key = self.get_uri_comparison_key(resource);
        let edit_stack = self.get_edit_stack(&key)?;
        let edit_stack = edit_stack.borrow();
        if edit_stack.has_future_elements() {
            return None;
        }
        edit_stack.closest_past_element().map(|element| element.actual())
    }

    fn get_elements(&self, resource: &URI) -> PastFutureElements {
        let key = self.get_uri_comparison_key(resource);
        match self.get_edit_stack(&key) {
            Some(edit_stack) => edit_stack.borrow().get_elements(),
            None => PastFutureElements::default(),
        }
    }

    fn set_elements_valid_flag(&self, resource: &URI, is_valid: bool, filter: &dyn Fn(&UndoRedoElementRef) -> bool) {
        let key = self.get_uri_comparison_key(resource);
        if let Some(edit_stack) = self.get_edit_stack(&key) {
            edit_stack.borrow().set_elements_valid_flag(is_valid, filter);
        }
    }

    fn remove_elements(&self, resource: &URI) {
        let key = self.get_uri_comparison_key(resource);
        self.remove_elements_by_key(&key);
    }

    fn create_snapshot(&self, resource: &URI) -> ResourceEditStackSnapshot {
        let key = self.get_uri_comparison_key(resource);
        match self.get_edit_stack(&key) {
            Some(edit_stack) => edit_stack.borrow().create_snapshot(resource),
            None => ResourceEditStackSnapshot { resource: resource.clone(), elements: Vec::new() },
        }
    }

    fn restore_snapshot(&self, snapshot: &ResourceEditStackSnapshot) {
        let key = self.get_uri_comparison_key(&snapshot.resource);
        let Some(edit_stack) = self.get_edit_stack(&key) else {
            return;
        };
        edit_stack.borrow_mut().restore_snapshot(snapshot);

        let is_empty = {
            let edit_stack = edit_stack.borrow();
            !edit_stack.has_past_elements() && !edit_stack.has_future_elements()
        };
        if is_empty {
            // the edit stack is now empty, just remove it entirely
            self.remove_elements_by_key(&key);
        }
    }

    fn can_undo(&self, target: UndoRedoTarget<'_>) -> bool {
        match target {
            UndoRedoTarget::Source(source) => self.find_closest_with_source(Direction::Undo, source.id).is_some(),
            UndoRedoTarget::Resource(resource) => {
                let key = self.get_uri_comparison_key(resource);
                self.get_edit_stack(&key).is_some_and(|edit_stack| edit_stack.borrow().has_past_elements())
            }
        }
    }

    fn undo(&self, target: UndoRedoTarget<'_>) -> MaybePromise<()> {
        match target {
            UndoRedoTarget::Source(source) => match self.find_closest_with_source(Direction::Undo, source.id) {
                Some(matched_str_resource) => self.run(Direction::Undo, &matched_str_resource, source.id, false),
                None => done(),
            },
            UndoRedoTarget::Resource(resource) => self.run(Direction::Undo, &self.get_uri_comparison_key(resource), 0, false),
        }
    }

    fn can_redo(&self, target: UndoRedoTarget<'_>) -> bool {
        match target {
            UndoRedoTarget::Source(source) => self.find_closest_with_source(Direction::Redo, source.id).is_some(),
            UndoRedoTarget::Resource(resource) => {
                let key = self.get_uri_comparison_key(resource);
                self.get_edit_stack(&key).is_some_and(|edit_stack| edit_stack.borrow().has_future_elements())
            }
        }
    }

    fn redo(&self, target: UndoRedoTarget<'_>) -> MaybePromise<()> {
        match target {
            UndoRedoTarget::Source(source) => match self.find_closest_with_source(Direction::Redo, source.id) {
                Some(matched_str_resource) => self.run(Direction::Redo, &matched_str_resource, 0, false),
                None => done(),
            },
            UndoRedoTarget::Resource(resource) => self.run(Direction::Redo, &self.get_uri_comparison_key(resource), 0, false),
        }
    }
}

impl Injectable for UndoRedoService {
    type Args = ();
    type Deps = (Rc<dyn IDialogService>, Rc<dyn INotificationService>);

    fn create(_: (), (dialog_service, notification_service): Self::Deps) -> Rc<Self> {
        Self::new(dialog_service, notification_service)
    }
}

service_impl!(UndoRedoService => dyn IUndoRedoService);

/// `registerSingleton(IUndoRedoService, UndoRedoService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<UndoRedoService>(crate::undo_redo::I_UNDO_REDO_SERVICE, InstantiationType::Delayed);
}

/// O `TestDialogService` sem resultado padrão: `confirm` não confirma e `prompt` roda o primeiro botão (ou o de cancelar).
struct DefaultTestDialogService;

impl IDialogService for DefaultTestDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn confirm(&self, _confirmation: Confirmation) -> Promise<ConfirmationResult> {
        Promise::resolved(ConfirmationResult { checkbox_checked: None, confirmed: false })
    }

    fn prompt(&self, prompt: Prompt) -> Promise<PromptResult> {
        let mut buttons: Vec<PromptBaseButton> = prompt.buttons.unwrap_or_default().into_iter().map(|button| button.base).collect();
        if let Some(PromptCancel::Button(cancel_button)) = prompt.cancel_button {
            buttons.push(cancel_button.base);
        }
        match buttons.first() {
            None => Promise::resolved(PromptResult { checkbox_checked: None, result: None }),
            Some(button) => match (button.run)(CheckboxResult { checkbox_checked: Some(false) }) {
                MaybePromise::Value(value) => Promise::resolved(PromptResult { checkbox_checked: None, result: Some(value) }),
                MaybePromise::Promise(promise) => promise.then(|value| PromptResult { checkbox_checked: None, result: Some(value) }),
            },
        }
    }

    fn input(&self, _input: Input) -> Promise<InputResult> {
        Promise::resolved(InputResult { confirmation: ConfirmationResult { checkbox_checked: None, confirmed: true }, values: Some(Vec::new()) })
    }

    fn info(&self, message: &str, detail: Option<&str>) -> Promise<()> {
        self.prompt(Self::message_prompt(Severity::Info, message, detail)).then(|_| ())
    }

    fn warn(&self, message: &str, detail: Option<&str>) -> Promise<()> {
        self.prompt(Self::message_prompt(Severity::Warning, message, detail)).then(|_| ())
    }

    fn error(&self, message: &str, detail: Option<&str>) -> Promise<()> {
        self.prompt(Self::message_prompt(Severity::Error, message, detail)).then(|_| ())
    }

    fn about(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

impl DefaultTestDialogService {
    fn message_prompt(severity: Severity, message: &str, detail: Option<&str>) -> Prompt {
        let mut base = BaseDialogOptions::new(message);
        base.dialog_type = Some(DialogTypeOrSeverity::Severity(severity));
        base.detail = detail.map(|detail| je_platform::dialogs::DialogDetail::Text(detail.to_owned()));
        Prompt { base, buttons: None, cancel_button: None }
    }
}

/// O `TestNotificationService`: não mostra nada.
struct SilentNotificationService;

/// `Disposable.None` devolvido por `status`.
struct NoOpStatus;

impl StatusHandle for NoOpStatus {
    fn close(&self) {}
}

impl INotificationService for SilentNotificationService {
    fn on_did_change_filter(&self) -> Event<()> {
        Event::none()
    }

    fn set_filter(&self, _filter: NotificationFilterSetting) {}

    fn get_filter(&self, _source: Option<&NotificationSource>) -> NotificationsFilter {
        NotificationsFilter::Off
    }

    fn get_filters(&self) -> Vec<NotificationSourceFilter> {
        Vec::new()
    }

    fn remove_filter(&self, _source_id: &str) {}

    fn notify(&self, _notification: Notification) -> Rc<dyn NotificationHandle> {
        Rc::new(NoOpNotification::new())
    }

    fn info(&self, _message: Vec<NotificationMessage>) {}

    fn warn(&self, _message: Vec<NotificationMessage>) {}

    fn error(&self, _message: Vec<NotificationMessage>) {}

    fn prompt(&self, _severity: Severity, _message: NotificationMessage, _choices: Vec<PromptChoiceKind>, _options: Option<PromptOptions>) -> Rc<dyn NotificationHandle> {
        Rc::new(NoOpNotification::new())
    }

    fn status(&self, _message: NotificationMessage, _options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
        Rc::new(NoOpStatus)
    }
}
