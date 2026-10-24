//! `jsMakeNontrivialString` de `runtime/JSStringInlines.h`.
//!
//! Pela regra de nomes o `JSStringInlines.h` entra no módulo `js_string`; a função vive aqui, num
//! arquivo à parte, até o `js_string.rs` poder recebê-la (o `FunctionExecutable` já a importa deste
//! caminho). Quando migrar, este módulo some.
//!
//! DIVERGÊNCIAS:
//!
//! - `jsMakeNontrivialString` devolve `JSValue` e o valor vazio quando `tryMakeString` falha (depois de
//!   `throwOutOfMemoryError`). Aqui o resultado é `Option<JSStringRef>`: `None` é o `JSValue()` vazio, e
//!   o caso feliz é sempre uma `JSString`.
//! - Os pedaços são `&dyn StringTypeAdapter` (o `StringTypeAdapter<T>` da WTF), os tipos que o
//!   `tryMakeString` do C++ aceita pelo `StringTypeAdapter`.
//! - `jsNontrivialString(vm, String&&)` é `js_string` (a `JSString` não nasce de pequena-string aqui).

use crate::runtime::exception_helpers::throw_out_of_memory_error;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::{js_string, JSStringRef};
use crate::runtime::throw_scope::ThrowScope;
use crate::wtf::text::string_concatenate::{try_make_string_dyn, StringTypeAdapter};
use crate::wtf::text::string_impl::MAX_LENGTH;

/// `jsMakeNontrivialString(JSGlobalObject*, StringType&&, StringTypes&&...)`.
pub fn js_make_nontrivial_string(
    global_object: &JSGlobalObject,
    strings: &[&dyn StringTypeAdapter],
) -> Option<JSStringRef> {
    let vm = global_object.vm();
    let mut scope = ThrowScope::new(vm);
    let Some(result) = try_make_string_dyn(strings) else {
        throw_out_of_memory_error(global_object, &mut scope);
        return None;
    };
    debug_assert!(result.length() <= MAX_LENGTH);
    Some(js_string(vm, &result))
}
