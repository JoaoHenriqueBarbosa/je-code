//! Porte de `runtime/NativeErrorPrototype.cpp` e `NativeErrorPrototype.h`: os protótipos dos seis erros
//! nativos. O C++ só tem o construtor (`Base(vm, structure)`); `name` e `message` vêm do
//! `ErrorPrototypeBase::finishCreation(vm, name)`.
//!
//! DIVERGÊNCIA (ligação pendente): o objeto `NativeErrorPrototype` entra com `ErrorPrototype::create`
//! quando a `NativeFunction` fechar; aqui ficam os dados da criação, que são os de
//! `error_prototype::initial_name_and_message` com o nome do tipo.

use crate::runtime::error_prototype::initial_name_and_message;
use crate::runtime::error_type::{error_type_name, ErrorType};
use crate::runtime::native_error_constructor::is_native_error_type;
use crate::wtf::text::wtf_string::String as WtfString;

/// Os valores iniciais de `name` e `message` do protótipo do tipo nativo; `None` para tipo sem protótipo
/// nativo próprio.
pub fn initial_properties(error_type: ErrorType) -> Option<(WtfString, WtfString)> {
    is_native_error_type(error_type).then(|| initial_name_and_message(error_type_name(error_type)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_and_message() {
        let (name, message) = initial_properties(ErrorType::RangeError).unwrap();
        assert_eq!(name.length(), "RangeError".len() as u32);
        assert_eq!(message.length(), 0);
        assert!(initial_properties(ErrorType::Error).is_none());
    }
}
