//! Porte parcial de `src/vs/workbench/services/editor/common/editorGroupsService.ts`: o contrato do
//! `IEditorGroupsService` reduzido ao que o `QueryBuilder` lê (os grupos e o recurso de cada editor aberto).
//!
//! Notas de tradução:
//! - **Contrato reduzido.** O `IEditorGroupsService` do original tem dezenas de membros (layout, ativação, mover e
//!   mesclar grupos...) e depende do `EditorInput`, que não foi portado. Aqui `groups` devolve os [`IEditorGroup`] e
//!   cada grupo devolve os seus [`EditorInput`], que expõem só o `resource`. Os demais membros entram junto com os
//!   tipos de que dependem.
//! - `readonly IEditorGroup[]` é `Vec<Rc<dyn IEditorGroup>>`, e `resource: URI | undefined` é `Option<URI>`.

use std::rc::Rc;

use je_base::uri::URI;
use je_instantiation::service_id;

/// `EditorInput`, reduzido ao `resource`.
pub trait EditorInput {
    /// O recurso que o editor mostra, quando há (`undefined` em editores sem recurso).
    fn resource(&self) -> Option<URI>;
}

/// `IEditorGroup`, reduzido aos `editors`.
pub trait IEditorGroup {
    /// Todos os editores abertos no grupo (`editors`).
    fn editors(&self) -> Vec<Rc<dyn EditorInput>>;
}

/// `IEditorGroupsService`, reduzido aos `groups`.
pub trait IEditorGroupsService {
    /// Os grupos de editores, na ordem em que o serviço os mantém (`groups`).
    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>>;
}

service_id!(pub I_EDITOR_GROUPS_SERVICE: dyn IEditorGroupsService = "editorGroupsService");
