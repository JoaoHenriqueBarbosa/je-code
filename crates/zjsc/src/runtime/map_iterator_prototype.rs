//! Porte de `runtime/MapIteratorPrototype.{h,cpp}`: o `%MapIteratorPrototype%` (um `JSNonFinalObject`
//! com o `ClassInfo` `"Map Iterator"`), com `next` (`mapIteratorProtoFuncNext`, intrínseco
//! `JSMapIteratorNextIntrinsic`) e `@@toStringTag`.
//!
//! O passo do iterador é `map_iterator_proto_next` (`js_map.rs`) e a casca é
//! `define_collection_iterator_prototype!` (ver `collection_support.rs`).

use crate::define_collection_iterator_prototype;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_map::map_iterator_proto_next;

define_collection_iterator_prototype!(
    MapIteratorPrototype,
    MAP_ITERATOR_PROTOTYPE_S_INFO,
    "Map Iterator",
    Intrinsic::JSMapIteratorNextIntrinsic,
    map_iterator_proto_next,
    map_iterator_proto_func_next
);
