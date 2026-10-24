//! Porte de `runtime/WeakMapConstructor.{h,cpp}`: o construtor `WeakMap` (um `InternalFunction`) com
//! `new WeakMap(iterable)`. Sem `@@species` (o C++ não o define).
//!
//! O corpo de `constructWeakMap` é `CollectionConstructor::construct` e a casca é
//! `define_collection_constructor!` (ver `collection_support.rs`). O item que não é objeto lança
//! `"WeakMap requires that an entry be an Object."`; a chave inválida lança o
//! `WeakMapInvalidKeyError` pelo próprio `set` do protótipo (o `canPerformFastSet` do C++ lança a mesma
//! mensagem).

use crate::define_collection_constructor;
use crate::runtime::js_weak_map::JSWeakMap;

define_collection_constructor!(
    WeakMapConstructor,
    WEAK_MAP_CONSTRUCTOR_S_INFO,
    JSWeakMap,
    "WeakMap",
    "set",
    Some("WeakMap requires that an entry be an Object."),
    false,
    call_weak_map,
    construct_weak_map
);
