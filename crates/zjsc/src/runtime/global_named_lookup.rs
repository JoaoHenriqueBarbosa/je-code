//! O acesso nomeado da janela (o `WindowProperties` do Blink, o `JSDOMWindow::getOwnPropertySlot` do WebKit): quando
//! uma busca por nome de texto falha no global e em toda a cadeia de protótipos dele, o hospedeiro ainda pode
//! responder (o elemento de `id` ou `name` igual ao nome). Declarações e propriedades próprias vêm antes, porque o
//! gancho só roda no fim da cadeia. Vale para leitura, `in`, `typeof` e a resolução de identificador sem
//! declaração, que passam todos pela busca do slot.

use std::cell::{Cell, RefCell};

use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::JSValue;
use crate::runtime::object_to_primitive::call_function;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::PropertySlot;
use crate::wtf::text::wtf_string::String as WtfString;

thread_local! {
    /// O `cell_id` de cada global com gancho (a janela e os frames dela) e a função que responde pelo nome.
    static HOOK: RefCell<Vec<(usize, JSValue)>> = const { RefCell::new(Vec::new()) };
    /// O gancho está rodando: a busca que ele mesmo faz no global não volta a ele.
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// Instala `function(nome)` como o acesso nomeado de `global_object`: devolve o valor, ou `undefined` para ausente.
pub fn set_global_named_lookup(global_object: &JSGlobalObject, function: JSValue) {
    let cell_id = global_object.cell_id();
    HOOK.with(|hook| {
        let mut hooks = hook.borrow_mut();
        hooks.retain(|(id, _)| *id != cell_id);
        hooks.push((cell_id, function));
    });
}

/// Fim do programa.
/// As raízes do coletor: o gancho instalado.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    for (cell_id, value) in HOOK.with(|hook| hook.borrow().clone()) {
        marker.cell(cell_id);
        marker.value(value);
    }
}

pub(crate) fn reset_for_program() {
    let _ = HOOK.try_with(|hook| hook.borrow_mut().clear());
    let _ = ACTIVE.try_with(|active| active.set(false));
}

/// O fim da busca em `object` sem resultado: pergunta ao gancho quando `object` é o global dele, ou o
/// `JSGlobalProxy` (o `window`/`globalThis` da página) que aponta para ele. Exceção do gancho fica pendente para quem
/// buscou.
pub(crate) fn lookup(object: &JSObject, property_name: &PropertyName, slot: &mut PropertySlot) -> bool {
    let target_id = crate::runtime::js_global_proxy::target_of(object).map(|target| target.cell_id());
    let Some((_, function)) = HOOK.with(|hook| hook.borrow().iter().find(|(id, _)| *id == object.cell_id() || Some(*id) == target_id).copied()) else { return false };
    if ACTIVE.with(Cell::get) || property_name.parse_index().is_some() {
        return false;
    }
    let Some(name) = property_name.public_name() else { return false };
    let global_object = crate::runtime::current_realm::current_global_object();
    let key = JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from(name.0.clone())));
    ACTIVE.with(|active| active.set(true));
    let result = call_function(&global_object, function, JSValue::undefined(), &[key]);
    ACTIVE.with(|active| active.set(false));
    match result {
        Some(value) if !value.is_undefined() => {
            slot.set_value(object, DONT_ENUM, value);
            true
        }
        _ => false,
    }
}
