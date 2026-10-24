//! Porte de `runtime/MapConstructor.{h,cpp}`: o construtor `Map` (um `InternalFunction`) com
//! `new Map(iterable)` e o `@@species`.
//!
//! `Map.groupBy` é `JSC_BUILTIN_FUNCTION_WITHOUT_TRANSITION(groupByPublicName, mapConstructorGroupByCodeGenerator)`
//! (`builtins/MapConstructor.js`): é instalado junto do construtor por `install_json_reflect_and_collections`
//! (`js_global_object_init.rs`), depois do `@@species`, como na ordem do C++.
//!
//! As funções privadas `mapPrivateFuncMapIterationNext`, `...Entry`, `...EntryKey`, `...EntryValue` e
//! `mapPrivateFuncMapStorage` estão em `ordered_hash_table_storage.rs` (com a divergência do `Storage`);
//! o `Map.prototype.forEach` (`map_prototype.rs`) é o JS embutido que as usa.
//!
//! O corpo de `constructMap` é `CollectionConstructor::construct` e a casca é
//! `define_collection_constructor!` (ver `collection_support.rs`, onde estão as DIVERGÊNCIAS do atalho
//! `canPerformFastSet`). O item que não é objeto lança o `throwTypeError(globalObject, scope)` sem
//! mensagem (`"Type error"`), por isso `Some("")`.

use crate::define_collection_constructor;
use crate::runtime::js_map::JSMap;

define_collection_constructor!(MapConstructor, MAP_CONSTRUCTOR_S_INFO, JSMap, "Map", "set", Some(""), true, call_map, construct_map);
