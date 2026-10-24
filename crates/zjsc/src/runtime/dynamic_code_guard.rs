//! O gancho do hospedeiro sobre o código que nasce de texto (`eval`, direto e indireto, e o construtor de `Function`):
//! o que o `Options::useTrustedTypes()` e o `canCompileStrings` do `JSGlobalObjectMethodTable` do JSC fazem no
//! WebKit. O hospedeiro registra uma função do programa; antes de compilar um texto o motor a chama com o tipo da
//! operação (`"eval"` ou `"Function"`) e o valor. Ela devolve `undefined` para seguir com o valor original, ou o texto
//! que o substitui (o código de um `TrustedScript`, ou o que a política padrão devolveu), e lança o `EvalError` que o
//! hospedeiro quiser. Sem função registrada, nada muda.
//!
//! A função fica em `thread_local` com um `Root` (o `Strong<Unknown>` do C++), para o coletor não a varrer enquanto
//! registrada, e cai com o programa (`cell_registry::reset_program_state`).

use std::cell::RefCell;

use crate::runtime::gc::Root;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::object_to_primitive::call_function;
use crate::wtf::text::wtf_string::String as WtfString;

thread_local! {
    static GUARD: RefCell<Option<(JSValue, Root)>> = const { RefCell::new(None) };
}

/// Registra (ou, com `None`, remove) a função de guarda.
pub fn set_code_guard(function: Option<JSValue>) {
    let previous = GUARD.with(|guard| std::mem::replace(&mut *guard.borrow_mut(), function.map(|value| (value, Root::new(value)))));
    drop(previous);
}

/// Fim do programa: o gancho some com ele.
pub(crate) fn reset_for_program() {
    let taken = GUARD.try_with(|guard| guard.borrow_mut().take());
    drop(taken);
}

fn registered() -> Option<JSValue> {
    GUARD.try_with(|guard| guard.borrow().as_ref().map(|(value, _)| *value)).ok().flatten()
}

/// O argumento do `eval` depois da guarda: o próprio valor (inclusive o que não é texto, que o `eval` devolve como
/// está) ou o texto que a função de guarda escolheu. `Err(())` quando ela lançou: a exceção fica pendente no `VM`.
pub fn guard_eval(global_object: &JSGlobalObject, value: JSValue) -> Result<JSValue, ()> {
    let Some(function) = registered() else { return Ok(value) };
    let kind = JSValue::from_js_string(crate::runtime::js_string::js_string(global_object.vm(), &WtfString::from_utf8(b"eval")));
    match call_function(global_object, function, JSValue::undefined(), &[kind, value]) {
        Some(replacement) if replacement.is_undefined() => Ok(value),
        Some(replacement) => Ok(replacement),
        None => Err(()),
    }
}

/// O texto completo que o construtor de `Function` vai compilar (`(function anonymous(a\n) {\n...\n})`) depois da
/// guarda.
pub fn guard_function_source(global_object: &JSGlobalObject, source: WtfString) -> Result<WtfString, ()> {
    let Some(function) = registered() else { return Ok(source) };
    let vm = global_object.vm();
    let kind = JSValue::from_js_string(crate::runtime::js_string::js_string(vm, &WtfString::from_utf8(b"Function")));
    let text = JSValue::from_js_string(crate::runtime::js_string::js_string(vm, &source));
    match call_function(global_object, function, JSValue::undefined(), &[kind, text]) {
        Some(replacement) if replacement.is_undefined() => Ok(source),
        Some(replacement) => Ok(replacement.to_wtf_string()),
        None => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::eval::{evaluate, evaluate_hidden_program, named_program_source, new_global_object, VmFinalizer};
    use crate::runtime::cell_registry::run_program;
    use crate::runtime::gc;
    use crate::wtf::text::conversion_mode::ConversionMode;

    /// A função de guarda vem de um programa escondido e só o `Root` a segura: com coleta a cada alocação, laços de
    /// lixo antes de cada `eval` e de cada `Function`, e o motor ainda chama a mesma função (que troca o texto).
    #[test]
    fn the_guard_survives_collections() {
        let (shown, collections) = run_program(|| {
            let (vm, global_object) = new_global_object();
            let _finalizer = VmFinalizer::new(&vm);
            let guard = evaluate_hidden_program(
                &global_object,
                "(function (kind, value) { if (typeof value !== 'string') return undefined; \
                 return kind === 'eval' ? value.replace('1', '40') : value.replace('return 1', 'return 41'); })",
            )
            .unwrap_or_else(|_| panic!("o programa da guarda lançou"));
            set_code_guard(Some(guard));
            gc::set_stress_for_thread(Some(1));
            // O ponto seguro só coleta com uma execução do `llint` na pilha nativa: os laços de lixo ficam no topo.
            let churn = "for (let i = 0; i < 50; i++) { const g = [i]; }";
            let page = format!(
                "{churn} var a = eval('1 + 2'); {churn} var b = (0, eval)('1 + 1'); {churn} var c = new Function('return 1')(); {churn} [a, b, c, eval(7)].join('|')"
            );
            let value = evaluate(&global_object, &named_program_source(&page, "page.js"));
            gc::set_stress_for_thread(None);
            let collections = gc::statistics().collections;
            let value = value.unwrap_or_else(|_| panic!("a página lançou"));
            (String::from_utf8_lossy(&value.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned(), collections)
        });
        assert_eq!(shown, "42|41|41|7");
        assert!(collections >= 1, "o teste não coletou");
    }
}
