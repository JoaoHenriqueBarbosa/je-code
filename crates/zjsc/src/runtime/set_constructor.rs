//! Porte de `runtime/SetConstructor.{h,cpp}`: o construtor `Set` (um `InternalFunction`) com
//! `new Set(iterable)` e o `@@species`.
//!
//! O corpo de `constructSet` é `CollectionConstructor::construct` e a casca é
//! `define_collection_constructor!` (ver `collection_support.rs`, onde estão as DIVERGÊNCIAS do atalho
//! `canPerformFastAdd` e do `clone`). As funções privadas `setPrivateFuncSet*` estão em
//! `ordered_hash_table_storage.rs`; o `Set.prototype.forEach` (`set_prototype.rs`) é o JS embutido que as usa.

use crate::define_collection_constructor;
use crate::runtime::js_set::JSSet;

define_collection_constructor!(SetConstructor, SET_CONSTRUCTOR_S_INFO, JSSet, "Set", "add", None, true, call_set, construct_set);
