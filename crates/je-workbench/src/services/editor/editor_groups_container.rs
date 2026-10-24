//! O `IEditorGroupsContainer` de `src/vs/workbench/services/editor/common/editorGroupsService.ts`, como o serviço de
//! editor e o observador de editores o recebem: o `IEditorGroupsService` inteiro ou uma única parte de editor.
//!
//! Divergências:
//! - **Não é um trait.** O original herda o `IEditorGroupsContainer` no `IEditorPart` e no `IEditorGroupsService` (ver o
//!   cabeçalho de [`editor_groups_service`](super::editor_groups_service)), e aqui os dois traits repetem os membros do
//!   container a partir de uma macro. Quem recebe "um container" (`editorGroupsContainer ?? editorGroupService`) recebe este
//!   enum, que repassa cada membro ao trait da variante.
//! - **Identidade.** O `===` do original (`this.editorGroupsContainer === this.editorGroupService.mainPart`) é a identidade do
//!   objeto dentro do `Rc` ([`EditorGroupsContainer::is_main_part_of`]).

use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::id_of;

use crate::common::editor::GroupIdentifier;
use crate::services::editor::editor_groups_service::{GroupsOrder, IEditorGroup, IEditorGroupsService, IEditorPart};

/// `IEditorGroupsContainer`: o serviço de grupos (`editorGroupService`) ou uma parte de editor.
#[derive(Clone)]
pub enum EditorGroupsContainer {
    Service(Rc<dyn IEditorGroupsService>),
    Part(Rc<dyn IEditorPart>),
}

impl EditorGroupsContainer {
    /// `container === service.mainPart || container === service`.
    pub fn is_main_part_of(&self, service: &Rc<dyn IEditorGroupsService>) -> bool {
        match self {
            EditorGroupsContainer::Service(container) => id_of(container) == id_of(service),
            EditorGroupsContainer::Part(container) => id_of(container) == id_of(&service.main_part()),
        }
    }

    /// `activeGroup`.
    pub fn active_group(&self) -> Rc<dyn IEditorGroup> {
        match self {
            EditorGroupsContainer::Service(container) => container.active_group(),
            EditorGroupsContainer::Part(container) => container.active_group(),
        }
    }

    /// `groups`.
    pub fn groups(&self) -> Vec<Rc<dyn IEditorGroup>> {
        match self {
            EditorGroupsContainer::Service(container) => container.groups(),
            EditorGroupsContainer::Part(container) => container.groups(),
        }
    }

    /// `getGroups(order)`.
    pub fn get_groups(&self, order: GroupsOrder) -> Vec<Rc<dyn IEditorGroup>> {
        match self {
            EditorGroupsContainer::Service(container) => container.get_groups(order),
            EditorGroupsContainer::Part(container) => container.get_groups(order),
        }
    }

    /// `getGroup(identifier)`.
    pub fn get_group(&self, identifier: GroupIdentifier) -> Option<Rc<dyn IEditorGroup>> {
        match self {
            EditorGroupsContainer::Service(container) => container.get_group(identifier),
            EditorGroupsContainer::Part(container) => container.get_group(identifier),
        }
    }

    /// `onDidChangeActiveGroup`.
    pub fn on_did_change_active_group(&self) -> Event<Rc<dyn IEditorGroup>> {
        match self {
            EditorGroupsContainer::Service(container) => container.on_did_change_active_group(),
            EditorGroupsContainer::Part(container) => container.on_did_change_active_group(),
        }
    }

    /// `onDidAddGroup`.
    pub fn on_did_add_group(&self) -> Event<Rc<dyn IEditorGroup>> {
        match self {
            EditorGroupsContainer::Service(container) => container.on_did_add_group(),
            EditorGroupsContainer::Part(container) => container.on_did_add_group(),
        }
    }
}

impl From<Rc<dyn IEditorGroupsService>> for EditorGroupsContainer {
    fn from(service: Rc<dyn IEditorGroupsService>) -> Self {
        EditorGroupsContainer::Service(service)
    }
}

impl From<Rc<dyn IEditorPart>> for EditorGroupsContainer {
    fn from(part: Rc<dyn IEditorPart>) -> Self {
        EditorGroupsContainer::Part(part)
    }
}
