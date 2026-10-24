//! Porte de `runtime/WeakSetConstructor.{h,cpp}`: o construtor `WeakSet` (um `InternalFunction`) com
//! `new WeakSet(iterable)`. Sem `@@species` (o C++ não o define).
//!
//! O corpo de `constructWeakSet` é `CollectionConstructor::construct` e a casca é
//! `define_collection_constructor!` (ver `collection_support.rs`). O valor inválido lança o
//! `WeakSetInvalidValueError` pelo próprio `add` do protótipo (o `canPerformFastAdd` do C++ lança a mesma
//! mensagem).

use crate::define_collection_constructor;
use crate::runtime::js_weak_set::JSWeakSet;

define_collection_constructor!(WeakSetConstructor, WEAK_SET_CONSTRUCTOR_S_INFO, JSWeakSet, "WeakSet", "add", None, false, call_weak_set, construct_weak_set);
