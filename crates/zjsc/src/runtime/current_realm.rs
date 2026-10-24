//! O `JSGlobalObject*` que o C++ passa como primeiro argumento de `toNumber`, `toString`, `toPrimitive`,
//! `toObject` e companhia: o objeto global lexical de quem está executando (`callFrame->lexicalGlobalObject`).
//!
//! DIVERGÊNCIA: as conversões de `JSValue` do porte (`to_number()`, `to_string(vm)`...) nasceram sem o
//! parâmetro `globalObject`, e dezenas de funções nativas as chamam assim. Em vez de mudar a assinatura de
//! todas, o reino corrente mora aqui: `host_function!` e `Interpreter::vm_entry_to_javascript` o definem
//! pelo tempo da execução ([`CurrentRealmScope`]), e os caminhos lentos das conversões (os que lançam
//! `TypeError` ou chamam `@@toPrimitive`, `toString`, `valueOf`) o consultam. Guarda só o `cell_id`, nunca a
//! célula, e o `Drop` restaura o anterior, então execuções aninhadas (uma função nativa que chama JS que
//! chama outra nativa) voltam ao reino de quem chamou.
//!
//! Fora de qualquer execução (testes, código que converte antes de entrar no interpretador) o reino é o
//! primeiro `JSGlobalObject` registrado.

use std::cell::Cell;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_scope::JSScopeRef;

thread_local! {
    /// O `cell_id` do `JSGlobalObject` em execução; 0 é "nenhum".
    static CURRENT_REALM: Cell<usize> = const { Cell::new(0) };
}

/// Fim do programa (`cell_registry::reset_program_state`): nenhum reino em execução.
/// As raízes do coletor: o global corrente.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    marker.cell(CURRENT_REALM.with(Cell::get));
}

pub(crate) fn reset_for_program() {
    let _ = CURRENT_REALM.try_with(|current| current.set(0));
}

/// Define o reino corrente até sair do escopo; o `Drop` restaura o reino anterior.
pub struct CurrentRealmScope {
    previous: usize,
}

impl CurrentRealmScope {
    /// Passa a executar em `global_object`.
    pub fn enter(global_object: &JSGlobalObject) -> CurrentRealmScope {
        let previous = CURRENT_REALM.with(|current| current.replace(global_object.cell_id()));
        CurrentRealmScope { previous }
    }
}

impl Drop for CurrentRealmScope {
    fn drop(&mut self) {
        CURRENT_REALM.with(|current| current.set(self.previous));
    }
}

/// O `globalObject` das conversões: o do escopo em curso, ou o primeiro registrado. Panica só se nenhum
/// `JSGlobalObject` existe, caso em que também não existe célula de objeto, símbolo ou BigInt para converter.
pub fn current_global_object() -> JSGlobalObjectRef {
    try_current_global_object().expect("conversão de JSValue sem nenhum JSGlobalObject")
}

/// [`current_global_object`] sem o pânico: `None` se nenhum `JSGlobalObject` existe.
pub fn try_current_global_object() -> Option<JSGlobalObjectRef> {
    let id = CURRENT_REALM.with(Cell::get);
    if id != 0 {
        if let Some(CellEntry::Scope(JSScopeRef::GlobalObject(global_object))) = cell_registry::get(id) {
            return Some(global_object);
        }
    }
    cell_registry::first_global_object()
}

/// `vm.exception()` do reino corrente: o `RETURN_IF_EXCEPTION` das conversões que devolvem um valor
/// sentinela (0, string vazia) em vez de um resultado que carregue o erro. Sem nenhum `JSGlobalObject`
/// nada pôde lançar, então é `false`.
pub fn has_pending_exception() -> bool {
    try_current_global_object().is_some_and(|global_object| global_object.vm().exception().is_some())
}
