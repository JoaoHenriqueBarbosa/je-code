//! Porte de `src/vs/base/common/hotReload.ts`: o registro dos tratadores de recarga a quente (a recarga de
//! módulos JavaScript em desenvolvimento).
//!
//! O estado global do original (`_isHotReloadEnabled`, o `Set` de tratadores e o
//! `globalThis.$hotReload_applyNewExports`) é um `thread_local!`, o equivalente do `globalThis` de um realm.
//! Os "exports" de um módulo (`Record<string, unknown>`) são um mapa de [`Dynamic`].
//!
//! Não portado: o tratador embutido `patch-prototype`, que troca métodos no `prototype` de classes
//! JavaScript e não tem sentido em Rust. A recarga a quente do código Rust do je-code é outra coisa (patching
//! do binário, ver a skill `setup-hot-reload`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use indexmap::IndexMap;

use crate::dynamic::Dynamic;
use crate::lifecycle::{DisposableRef, plain_disposable};

/// `Record<string, unknown>`: os exports de um módulo.
pub type Exports = IndexMap<String, Dynamic>;

/// `IHotReloadConfig`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HotReloadConfig {
    /// `mode?: 'patch-prototype'`.
    pub mode: Option<String>,
}

/// Os argumentos de um [`HotReloadHandler`].
#[derive(Clone)]
pub struct HotReloadArgs {
    pub old_exports: Exports,
    pub new_src: String,
    pub config: HotReloadConfig,
}

/// `AcceptNewExportsHandler`: recebe os exports novos (que pode alterar) e diz se os aplicou.
pub type AcceptNewExportsHandler = Rc<dyn Fn(&mut Exports) -> bool>;

/// `HotReloadHandler`: recebe os exports antigos do módulo a recarregar e devolve a função que aplica os novos.
/// Se devolve `None`, este tratador não sabe tratar o módulo. Se nenhum tratador aplica os exports novos, o
/// módulo não é recarregado.
pub type HotReloadHandler = Rc<dyn Fn(&HotReloadArgs) -> Option<AcceptNewExportsHandler>>;

thread_local! {
    static IS_HOT_RELOAD_ENABLED: Cell<bool> = const { Cell::new(false) };
    static HANDLERS: RefCell<Vec<HotReloadHandler>> = const { RefCell::new(Vec::new()) };
}

pub fn enable_hot_reload() {
    IS_HOT_RELOAD_ENABLED.with(|e| e.set(true));
}

pub fn is_hot_reload_enabled() -> bool {
    IS_HOT_RELOAD_ENABLED.with(Cell::get)
}

/// `registerHotReloadHandler`: sem a recarga a quente ligada não registra nada.
pub fn register_hot_reload_handler(handler: HotReloadHandler) -> DisposableRef {
    if !is_hot_reload_enabled() {
        return plain_disposable(|| {});
    }
    HANDLERS.with(|h| h.borrow_mut().push(handler.clone()));
    plain_disposable(move || HANDLERS.with(|h| h.borrow_mut().retain(|x| !Rc::ptr_eq(x, &handler))))
}

/// `globalThis.$hotReload_applyNewExports`: pergunta a cada tratador registrado. Devolve `None` se nenhum
/// sabe tratar o módulo; senão, uma função que aplica os exports novos em todos os que sabem e diz se algum
/// aplicou.
pub fn apply_new_exports(args: HotReloadArgs) -> Option<AcceptNewExportsHandler> {
    let handlers: Vec<HotReloadHandler> = HANDLERS.with(|h| h.borrow().clone());
    let results: Vec<AcceptNewExportsHandler> = handlers.iter().filter_map(|h| h(&args)).collect();
    if results.is_empty() {
        return None;
    }
    Some(Rc::new(move |new_exports: &mut Exports| {
        let mut result = false;
        for r in &results {
            if r(new_exports) {
                result = true;
            }
        }
        result
    }))
}
