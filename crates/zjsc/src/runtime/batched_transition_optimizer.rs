//! Tradução de `runtime/BatchedTransitionOptimizer.h`.

use crate::runtime::js_object::JSObject;
use crate::runtime::vm::VM;

/// `class BatchedTransitionOptimizer`: converte o objeto em dicionário (se ainda não for) enquanto
/// vive. O C++ não tem destrutor: a conversão não se desfaz, então não há `Drop`.
pub struct BatchedTransitionOptimizer {
    _private: (),
}

impl BatchedTransitionOptimizer {
    /// `BatchedTransitionOptimizer(VM&, JSObject*)`.
    pub fn new(vm: &VM, object: &JSObject) -> BatchedTransitionOptimizer {
        if !object.structure().is_dictionary() {
            object.convert_to_dictionary(vm);
        }
        BatchedTransitionOptimizer { _private: () }
    }
}
