//! Porte de `src/vs/platform/undoRedo/common/undoRedoService.ts` para elementos de um recurso só.
//!
//! O serviço guarda uma pilha de desfazer e refazer por recurso (chave de comparação: a URI em texto),
//! agrupa elementos por `UndoRedoGroup` e por `UndoRedoSource` e trava a pilha enquanto um desfazer roda.
//! Desfazer e refazer são síncronos, porque o `undo()` de um `SingleModelEditStackElement` é.
//!
//! Decisões:
//! - A confirmação (`IDialogService.confirm`, assíncrona no original) vira um [`ConfirmFn`] síncrono
//!   injetado; o aviso (`INotificationService.warn`/`error`) vira [`NotifyFn`].
//!
//! Pendente (junto do elemento de workspace de `undoRedo.ts`): `WorkspaceStackElement` e tudo o que
//! depende dele (`_workspaceUndo`, `_workspaceRedo`, `_checkWorkspaceUndo`, `_tryToSplitAndUndo`,
//! `RemovedResources`, `WorkspaceVerificationError`, `_invokeWorkspacePrepare`).
// TODO(integração): mover para o crate je-platform (platform/undoRedo/common).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::uri::URI;

use crate::undo_redo::{ResourceEditStackSnapshot, UndoRedoElement, UndoRedoGroup, UndoRedoSource};

/// `IDialogService.confirm`: devolve se o usuário confirmou.
pub type ConfirmFn = Rc<dyn Fn(&str) -> bool>;

/// `INotificationService.warn` e `error`.
pub type NotifyFn = Rc<dyn Fn(&str)>;

thread_local! {
    static STACK_ELEMENT_COUNTER: Cell<u64> = const { Cell::new(0) };
}

/// `ResourceStackElement`.
struct ResourceStackElement {
    id: u64,
    actual: Rc<dyn UndoRedoElement>,
    label: String,
    confirm_before_undo: bool,
    str_resource: String,
    group_id: u64,
    group_order: u64,
    source_id: u64,
    source_order: u64,
    is_valid: Cell<bool>,
}

type StackElement = Rc<ResourceStackElement>;

impl ResourceStackElement {
    fn new(actual: Rc<dyn UndoRedoElement>, str_resource: String, group_id: u64, group_order: u64, source_id: u64, source_order: u64) -> Self {
        let id = STACK_ELEMENT_COUNTER.with(|counter| {
            counter.set(counter.get() + 1);
            counter.get()
        });
        Self {
            id,
            label: actual.label(),
            confirm_before_undo: actual.confirm_before_undo(),
            actual,
            str_resource,
            group_id,
            group_order,
            source_id,
            source_order,
            is_valid: Cell::new(true),
        }
    }
}

/// `ResourceEditStack`.
struct ResourceEditStack {
    past: Vec<StackElement>,
    future: Vec<StackElement>,
    locked: bool,
    version_id: u64,
}

impl ResourceEditStack {
    fn new() -> Self {
        Self {
            past: Vec::new(),
            future: Vec::new(),
            locked: false,
            version_id: 1,
        }
    }

    fn dispose(&mut self) {
        self.version_id += 1;
    }

    fn flush_all_elements(&mut self) {
        self.past.clear();
        self.future.clear();
        self.version_id += 1;
    }

    fn set_elements_valid_flag(&self, is_valid: bool, filter: &dyn Fn(&dyn UndoRedoElement) -> bool) {
        for element in self.past.iter().chain(self.future.iter()) {
            if filter(element.actual.as_ref()) {
                element.is_valid.set(is_valid);
            }
        }
    }

    fn push_element(&mut self, element: StackElement) {
        // remove o futuro
        self.future.clear();
        self.past.push(element);
        self.version_id += 1;
    }

    fn create_snapshot(&self, resource: &URI) -> ResourceEditStackSnapshot {
        let mut elements: Vec<u64> = self.past.iter().map(|element| element.id).collect();
        elements.extend(self.future.iter().rev().map(|element| element.id));
        ResourceEditStackSnapshot {
            resource: resource.clone(),
            elements,
        }
    }

    fn restore_snapshot(&mut self, snapshot: &ResourceEditStackSnapshot) {
        let snapshot_length = snapshot.elements.len();
        let mut is_ok = true;
        let mut snapshot_index = 0usize;
        let mut remove_past_after: Option<usize> = None;
        for (i, element) in self.past.iter().enumerate() {
            if is_ok && (snapshot_index >= snapshot_length || element.id != snapshot.elements[snapshot_index]) {
                is_ok = false;
                remove_past_after = Some(i);
            }
            snapshot_index += 1;
        }
        let mut remove_future_before: Option<usize> = None;
        for (i, element) in self.future.iter().enumerate().rev() {
            if is_ok && (snapshot_index >= snapshot_length || element.id != snapshot.elements[snapshot_index]) {
                is_ok = false;
                remove_future_before = Some(i);
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

    fn closest_past_element(&self) -> Option<StackElement> {
        self.past.last().cloned()
    }

    fn closest_future_element(&self) -> Option<StackElement> {
        self.future.last().cloned()
    }

    fn move_backward(&mut self, element: &StackElement) {
        self.past.pop();
        self.future.push(element.clone());
        self.version_id += 1;
    }

    fn move_forward(&mut self, element: &StackElement) {
        self.future.pop();
        self.past.push(element.clone());
        self.version_id += 1;
    }
}

/// `IPastFutureElements`.
pub struct PastFutureElements {
    pub past: Vec<Rc<dyn UndoRedoElement>>,
    pub future: Vec<Rc<dyn UndoRedoElement>>,
}

/// O que identifica quem desfaz: um recurso ou uma origem (`URI | UndoRedoSource`).
pub enum UndoRedoTarget<'a> {
    Resource(&'a URI),
    Source(&'a UndoRedoSource),
}

/// As pilhas por recurso, na ordem em que apareceram (o `Map` do JavaScript itera por inserção).
#[derive(Default)]
struct EditStacks {
    order: Vec<String>,
    stacks: HashMap<String, ResourceEditStack>,
}

impl EditStacks {
    fn get_or_create(&mut self, str_resource: &str) -> &mut ResourceEditStack {
        if !self.stacks.contains_key(str_resource) {
            self.order.push(str_resource.to_owned());
            self.stacks.insert(str_resource.to_owned(), ResourceEditStack::new());
        }
        self.stacks.get_mut(str_resource).expect("a pilha acabou de ser criada")
    }

    fn remove(&mut self, str_resource: &str) {
        if let Some(mut stack) = self.stacks.remove(str_resource) {
            stack.dispose();
            self.order.retain(|key| key != str_resource);
        }
    }

    /// O candidato mais próximo (desfazer ou refazer) escolhido por `better`, na ordem de inserção.
    fn find_closest(
        &self,
        candidate_of: impl Fn(&ResourceEditStack) -> Option<StackElement>,
        matches: impl Fn(&StackElement) -> bool,
        better: impl Fn(&StackElement, &StackElement) -> bool,
    ) -> Option<(StackElement, &str)> {
        let mut matched: Option<(StackElement, &str)> = None;
        for key in &self.order {
            let Some(candidate) = candidate_of(&self.stacks[key]) else {
                continue;
            };
            if matches(&candidate) && matched.as_ref().is_none_or(|(current, _)| better(&candidate, current)) {
                matched = Some((candidate, key));
            }
        }
        matched
    }
}

/// `UndoRedoService`.
pub struct UndoRedoService {
    edit_stacks: RefCell<EditStacks>,
    confirm: ConfirmFn,
    warn: NotifyFn,
    error: NotifyFn,
}

impl UndoRedoService {
    pub fn new(confirm: ConfirmFn, warn: NotifyFn, error: NotifyFn) -> Rc<Self> {
        Rc::new(Self {
            edit_stacks: RefCell::new(EditStacks::default()),
            confirm,
            warn,
            error,
        })
    }

    /// O serviço dos testes (`TestDialogService` sem resultado: não confirma; notificações ignoradas).
    pub fn for_tests() -> Rc<Self> {
        Self::new(Rc::new(|_| false), Rc::new(|_| {}), Rc::new(|_| {}))
    }

    /// `getUriComparisonKey`.
    pub fn get_uri_comparison_key(&self, resource: &URI) -> String {
        resource.to_string(false).to_string_lossy()
    }

    /// `pushElement`.
    pub fn push_element(&self, element: Rc<dyn UndoRedoElement>, group: Option<&UndoRedoGroup>, source: Option<&UndoRedoSource>) {
        let none_group = UndoRedoGroup::none();
        let none_source = UndoRedoSource::none();
        let group = group.unwrap_or(&none_group);
        let source = source.unwrap_or(&none_source);
        let str_resource = self.get_uri_comparison_key(&element.resource());
        let stack_element = Rc::new(ResourceStackElement::new(
            element,
            str_resource.clone(),
            group.id,
            group.next_order(),
            source.id,
            source.next_order(),
        ));
        self.edit_stacks.borrow_mut().get_or_create(&str_resource).push_element(stack_element);
    }

    /// `getLastElement`.
    pub fn get_last_element(&self, resource: &URI) -> Option<Rc<dyn UndoRedoElement>> {
        let key = self.get_uri_comparison_key(resource);
        let stacks = self.edit_stacks.borrow();
        let stack = stacks.stacks.get(&key)?;
        if !stack.future.is_empty() {
            return None;
        }
        stack.closest_past_element().map(|element| element.actual.clone())
    }

    /// `removeElements`.
    pub fn remove_elements(&self, resource: &URI) {
        let key = self.get_uri_comparison_key(resource);
        self.remove_elements_by_key(&key);
    }

    fn remove_elements_by_key(&self, str_resource: &str) {
        self.edit_stacks.borrow_mut().remove(str_resource);
    }

    /// `setElementsValidFlag`.
    pub fn set_elements_valid_flag(&self, resource: &URI, is_valid: bool, filter: &dyn Fn(&dyn UndoRedoElement) -> bool) {
        let key = self.get_uri_comparison_key(resource);
        if let Some(stack) = self.edit_stacks.borrow().stacks.get(&key) {
            stack.set_elements_valid_flag(is_valid, filter);
        }
    }

    /// `hasElements`.
    pub fn has_elements(&self, resource: &URI) -> bool {
        let key = self.get_uri_comparison_key(resource);
        self.edit_stacks
            .borrow()
            .stacks
            .get(&key)
            .is_some_and(|stack| !stack.past.is_empty() || !stack.future.is_empty())
    }

    /// `createSnapshot`.
    pub fn create_snapshot(&self, resource: &URI) -> ResourceEditStackSnapshot {
        let key = self.get_uri_comparison_key(resource);
        match self.edit_stacks.borrow().stacks.get(&key) {
            Some(stack) => stack.create_snapshot(resource),
            None => ResourceEditStackSnapshot {
                resource: resource.clone(),
                elements: Vec::new(),
            },
        }
    }

    /// `restoreSnapshot`.
    pub fn restore_snapshot(&self, snapshot: &ResourceEditStackSnapshot) {
        let key = self.get_uri_comparison_key(&snapshot.resource);
        let mut stacks = self.edit_stacks.borrow_mut();
        let Some(stack) = stacks.stacks.get_mut(&key) else {
            return;
        };
        stack.restore_snapshot(snapshot);
        if stack.past.is_empty() && stack.future.is_empty() {
            // a pilha ficou vazia, remove de vez
            stacks.remove(&key);
        }
    }

    /// `getElements`.
    pub fn get_elements(&self, resource: &URI) -> PastFutureElements {
        let key = self.get_uri_comparison_key(resource);
        match self.edit_stacks.borrow().stacks.get(&key) {
            Some(stack) => PastFutureElements {
                past: stack.past.iter().map(|element| element.actual.clone()).collect(),
                future: stack.future.iter().map(|element| element.actual.clone()).collect(),
            },
            None => PastFutureElements {
                past: Vec::new(),
                future: Vec::new(),
            },
        }
    }

    fn find_closest_undo_element_with_source(&self, source_id: u64) -> Option<String> {
        if source_id == 0 {
            return None;
        }
        // o elemento da origem com a maior `sourceOrder` pronto para desfazer
        self.edit_stacks
            .borrow()
            .find_closest(
                ResourceEditStack::closest_past_element,
                |candidate| candidate.source_id == source_id,
                |candidate, current| candidate.source_order > current.source_order,
            )
            .map(|(_, key)| key.to_owned())
    }

    fn find_closest_redo_element_with_source(&self, source_id: u64) -> Option<String> {
        if source_id == 0 {
            return None;
        }
        // o elemento da origem com a menor `sourceOrder` pronto para refazer
        self.edit_stacks
            .borrow()
            .find_closest(
                ResourceEditStack::closest_future_element,
                |candidate| candidate.source_id == source_id,
                |candidate, current| candidate.source_order < current.source_order,
            )
            .map(|(_, key)| key.to_owned())
    }

    fn find_closest_undo_element_in_group(&self, group_id: u64) -> Option<(StackElement, String)> {
        if group_id == 0 {
            return None;
        }
        // o elemento do grupo com a maior `groupOrder` pronto para desfazer
        self.edit_stacks
            .borrow()
            .find_closest(
                ResourceEditStack::closest_past_element,
                |candidate| candidate.group_id == group_id,
                |candidate, current| candidate.group_order > current.group_order,
            )
            .map(|(element, key)| (element, key.to_owned()))
    }

    fn find_closest_redo_element_in_group(&self, group_id: u64) -> Option<(StackElement, String)> {
        if group_id == 0 {
            return None;
        }
        // o elemento do grupo com a menor `groupOrder` pronto para refazer
        self.edit_stacks
            .borrow()
            .find_closest(
                ResourceEditStack::closest_future_element,
                |candidate| candidate.group_id == group_id,
                |candidate, current| candidate.group_order < current.group_order,
            )
            .map(|(element, key)| (element, key.to_owned()))
    }

    /// `canUndo`.
    pub fn can_undo(&self, target: UndoRedoTarget<'_>) -> bool {
        match target {
            UndoRedoTarget::Source(source) => self.find_closest_undo_element_with_source(source.id).is_some(),
            UndoRedoTarget::Resource(resource) => {
                let key = self.get_uri_comparison_key(resource);
                self.edit_stacks.borrow().stacks.get(&key).is_some_and(|stack| !stack.past.is_empty())
            }
        }
    }

    /// `canRedo`.
    pub fn can_redo(&self, target: UndoRedoTarget<'_>) -> bool {
        match target {
            UndoRedoTarget::Source(source) => self.find_closest_redo_element_with_source(source.id).is_some(),
            UndoRedoTarget::Resource(resource) => {
                let key = self.get_uri_comparison_key(resource);
                self.edit_stacks.borrow().stacks.get(&key).is_some_and(|stack| !stack.future.is_empty())
            }
        }
    }

    /// `_onError`: um erro ao desfazer ou refazer derruba a pilha do recurso afetado.
    fn on_error(&self, err: &Error, element: &StackElement) {
        errors::on_unexpected_error(err);
        self.remove_elements_by_key(&element.str_resource);
        (self.error)(&err.message);
    }

    /// `_safeInvokeWithLocks` para um recurso só: trava a pilha, roda `invoke` e libera.
    /// Devolve se `invoke` terminou sem erro.
    fn safe_invoke_with_lock(&self, str_resource: &str, element: &StackElement, invoke: impl FnOnce()) -> bool {
        {
            let mut stacks = self.edit_stacks.borrow_mut();
            if let Some(stack) = stacks.stacks.get_mut(str_resource) {
                if stack.locked {
                    errors::throw(Error::new("Cannot acquire edit stack lock"));
                }
                stack.locked = true;
            }
        }
        let outcome = errors::catch(invoke);
        if let Some(stack) = self.edit_stacks.borrow_mut().stacks.get_mut(str_resource) {
            stack.locked = false;
        }
        match outcome {
            Ok(()) => true,
            Err(err) => {
                self.on_error(&err, element);
                false
            }
        }
    }

    fn resource_undo(&self, str_resource: &str, element: &StackElement, undo_confirmed: bool) {
        {
            let mut stacks = self.edit_stacks.borrow_mut();
            let Some(stack) = stacks.stacks.get_mut(str_resource) else {
                return;
            };
            if !element.is_valid.get() {
                // elemento inválido: descarta a pilha na hora
                stack.flush_all_elements();
                return;
            }
            if stack.locked {
                drop(stacks);
                (self.warn)(&format!(
                    "Could not undo '{}' because there is already an undo or redo operation running.",
                    element.label
                ));
                return;
            }
            stack.move_backward(element);
        }
        if self.safe_invoke_with_lock(str_resource, element, || element.actual.undo()) {
            self.continue_undo_in_group(element.group_id, undo_confirmed);
        }
    }

    fn continue_undo_in_group(&self, group_id: u64, undo_confirmed: bool) {
        if group_id == 0 {
            return;
        }
        if let Some((_, matched_str_resource)) = self.find_closest_undo_element_in_group(group_id) {
            self.undo_by_key(&matched_str_resource, 0, undo_confirmed);
        }
    }

    /// `undo`.
    pub fn undo(&self, target: UndoRedoTarget<'_>) {
        match target {
            UndoRedoTarget::Source(source) => {
                if let Some(matched) = self.find_closest_undo_element_with_source(source.id) {
                    self.undo_by_key(&matched, source.id, false);
                }
            }
            UndoRedoTarget::Resource(resource) => {
                let key = self.get_uri_comparison_key(resource);
                self.undo_by_key(&key, 0, false);
            }
        }
    }

    /// `_undo`.
    fn undo_by_key(&self, str_resource: &str, source_id: u64, undo_confirmed: bool) {
        let element = {
            let stacks = self.edit_stacks.borrow();
            let Some(stack) = stacks.stacks.get(str_resource) else {
                return;
            };
            let Some(element) = stack.closest_past_element() else {
                return;
            };
            element
        };

        if element.group_id != 0 {
            // o elemento é de um grupo: desfazer em um grupo precisa seguir a ordem
            if let Some((matched_element, matched_str_resource)) = self.find_closest_undo_element_in_group(element.group_id)
                && !Rc::ptr_eq(&element, &matched_element)
            {
                // há um elemento do mesmo grupo que deve ser desfeito antes deste
                self.undo_by_key(&matched_str_resource, source_id, undo_confirmed);
                return;
            }
        }

        let should_prompt_for_confirmation = element.source_id != source_id || element.confirm_before_undo;
        if should_prompt_for_confirmation && !undo_confirmed {
            // outra origem ou o elemento pede confirmação antes de desfazer
            if (self.confirm)(&format!("Would you like to undo '{}'?", element.label)) {
                self.undo_by_key(str_resource, source_id, true);
            }
            return;
        }

        self.resource_undo(str_resource, &element, undo_confirmed);
    }

    fn resource_redo(&self, str_resource: &str, element: &StackElement) {
        {
            let mut stacks = self.edit_stacks.borrow_mut();
            let Some(stack) = stacks.stacks.get_mut(str_resource) else {
                return;
            };
            if !element.is_valid.get() {
                // elemento inválido: descarta a pilha na hora
                stack.flush_all_elements();
                return;
            }
            if stack.locked {
                drop(stacks);
                (self.warn)(&format!(
                    "Could not redo '{}' because there is already an undo or redo operation running.",
                    element.label
                ));
                return;
            }
            stack.move_forward(element);
        }
        if self.safe_invoke_with_lock(str_resource, element, || element.actual.redo()) {
            self.continue_redo_in_group(element.group_id);
        }
    }

    fn continue_redo_in_group(&self, group_id: u64) {
        if group_id == 0 {
            return;
        }
        if let Some((_, matched_str_resource)) = self.find_closest_redo_element_in_group(group_id) {
            self.redo_by_key(&matched_str_resource);
        }
    }

    /// `redo`.
    pub fn redo(&self, target: UndoRedoTarget<'_>) {
        match target {
            UndoRedoTarget::Source(source) => {
                if let Some(matched) = self.find_closest_redo_element_with_source(source.id) {
                    self.redo_by_key(&matched);
                }
            }
            UndoRedoTarget::Resource(resource) => {
                let key = self.get_uri_comparison_key(resource);
                self.redo_by_key(&key);
            }
        }
    }

    /// `_redo`.
    fn redo_by_key(&self, str_resource: &str) {
        let element = {
            let stacks = self.edit_stacks.borrow();
            let Some(stack) = stacks.stacks.get(str_resource) else {
                return;
            };
            let Some(element) = stack.closest_future_element() else {
                return;
            };
            element
        };

        if element.group_id != 0
            && let Some((matched_element, matched_str_resource)) = self.find_closest_redo_element_in_group(element.group_id)
            && !Rc::ptr_eq(&element, &matched_element)
        {
            // há um elemento do mesmo grupo que deve ser refeito antes deste
            self.redo_by_key(&matched_str_resource);
            return;
        }

        self.resource_redo(str_resource, &element);
    }
}
