//! Porte de `runtime/SetIteratorPrototype.{h,cpp}`: o `%SetIteratorPrototype%` (um `JSNonFinalObject`
//! com o `ClassInfo` `"Set Iterator"`), com `next` (`setIteratorProtoFuncNext`, intrínseco
//! `JSSetIteratorNextIntrinsic`) e `@@toStringTag`.
//!
//! O passo do iterador é `set_iterator_proto_next` (`js_set.rs`) e a casca é
//! `define_collection_iterator_prototype!` (ver `collection_support.rs`).

use crate::define_collection_iterator_prototype;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_set::set_iterator_proto_next;

define_collection_iterator_prototype!(
    SetIteratorPrototype,
    SET_ITERATOR_PROTOTYPE_S_INFO,
    "Set Iterator",
    Intrinsic::JSSetIteratorNextIntrinsic,
    set_iterator_proto_next,
    set_iterator_proto_func_next
);
