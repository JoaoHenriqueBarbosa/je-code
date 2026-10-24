//! Tradução da parte pura de `runtime/Error.cpp`/`Error.h` (`addErrorInfo(VM&, JSObject*, int,
//! const SourceCode&)`, `createSyntaxError`, `createRangeError`, `createOutOfMemoryError`,
//! `createStackOverflowError`) e de `parser/ParserError.h::toErrorObject`.
//!
//! Divergência documentada: os construtores `create*Error` e `addErrorInfo` trabalham sobre o
//! `ErrorData` (o dado observável do `ErrorInstance`, veja `error_instance`) e só
//! `toErrorObject` materializa a célula no registro central, com a `Structure` do `JSGlobalObject`.
//! O `ErrorHandlingScope` do ramo `StackOverflow` só afeta o limite de pilha do VM e não tem
//! efeito sobre o dado, então não aparece. Como `source.provider()` nulo derrefenciaria um nulo no
//! C++, aqui vale `sourceURL` vazio.

use crate::parser::parser_error::{ErrorType as ParserErrorType, ParserError};
use crate::parser::source_code::SourceCode;
use crate::parser::source_provider::SourceProvider;
use crate::runtime::error_instance::{ErrorData, ErrorInstance, ErrorInstanceRef};
use crate::runtime::error_natives::put_message_property;
use crate::runtime::error_type::ErrorType;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::wtf::text::wtf_string::String as WtfString;

/// `createSyntaxError(globalObject, message)`.
pub fn create_syntax_error(message: &WtfString) -> ErrorData {
    ErrorData::create(ErrorType::SyntaxError, message.clone())
}

/// `createRangeError(globalObject, message)`.
pub fn create_range_error(message: &str) -> ErrorData {
    ErrorData::create(ErrorType::RangeError, WtfString::from_utf8(message.as_bytes()))
}

/// `createOutOfMemoryError(globalObject)`.
pub fn create_out_of_memory_error() -> ErrorData {
    let mut error = create_range_error("Out of memory");
    error.is_out_of_memory_error = true;
    error
}

/// `createStackOverflowError(globalObject)`.
pub fn create_stack_overflow_error() -> ErrorData {
    let mut error = create_range_error("Maximum call stack size exceeded.");
    error.is_stack_overflow_error = true;
    error
}

/// `addErrorInfo(VM&, JSObject*, int line, const SourceCode&)`, ramo `USE(BUN_JSC_ADDITIONS)`.
pub fn add_error_info(mut error: ErrorData, line: i32, source: &SourceCode) -> ErrorData {
    let source_url = match source.provider() {
        Some(provider) => provider.source_url().clone(),
        None => WtfString::default(),
    };
    if line != -1 {
        error.line = line;
        error.column = 0;
    }
    if !source_url.is_empty() {
        error.source_url = source_url;
    }
    error
}

impl ParserError {
    /// `ParserError::toErrorObject(globalObject, source, overrideLineNumber)`. `None` é o
    /// `nullptr` do caso `ErrorNone`. A célula nasce com `globalObject->errorStructure(type)` (o
    /// protótipo do tipo) e a propriedade própria `message`.
    pub fn to_error_object_override_line(
        &self,
        global_object: &JSGlobalObject,
        source: &SourceCode,
        override_line_number: i32,
    ) -> Option<ErrorInstanceRef> {
        let mut data = self.to_error_data_override_line(source, override_line_number)?;
        let vm = global_object.vm();
        let error_type = data.error_type.unwrap_or(ErrorType::SyntaxError);
        data.message = crate::runtime::error::v8_or_jsc_message(error_type, &data.message);
        let message = data.message.clone();
        let instance = ErrorInstance::create_with_data(vm, global_object.error_structure_for(error_type), data);
        // `ErrorInstance::finishCreation` grava `message` (`DontEnum`) quando não vazia.
        if !message.is_empty() {
            put_message_property(vm, &instance, &message);
        }
        Some(instance)
    }

    /// `toErrorObject(globalObject, source)` com `overrideLineNumber = -1`.
    pub fn to_error_object(&self, global_object: &JSGlobalObject, source: &SourceCode) -> Option<ErrorInstanceRef> {
        self.to_error_object_override_line(global_object, source, -1)
    }

    /// O dado do erro de `toErrorObject` (`setParseError` incluído), sem a célula.
    fn to_error_data_override_line(&self, source: &SourceCode, override_line_number: i32) -> Option<ErrorData> {
        let mut error = match self.type_() {
            ParserErrorType::ErrorNone => return None,
            ParserErrorType::SyntaxError => add_error_info(
                create_syntax_error(self.message()),
                if override_line_number == -1 { self.line() } else { override_line_number },
                source,
            ),
            ParserErrorType::EvalError => create_syntax_error(self.message()),
            ParserErrorType::StackOverflow => create_stack_overflow_error(),
            ParserErrorType::OutOfMemory => create_out_of_memory_error(),
        };
        error.is_parse_error = true;
        Some(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use crate::runtime::js_type::JSType;
    use crate::parser::parser_error::SyntaxErrorType;
    use crate::parser::parser_tokens::JSToken;
    use crate::parser::source_provider::{SourceProviderSourceType, StringSourceProvider};
    use crate::parser::source_tainted_origin::SourceTaintedOrigin;
    use crate::runtime::source_origin::SourceOrigin;
    use crate::wtf::text::text_position::TextPosition;

    fn source(url: &str) -> SourceCode {
        SourceCode::from_provider(StringSourceProvider::create(
            &WtfString::from_latin1(b"let x"),
            &SourceOrigin::default(),
            WtfString::from_latin1(url.as_bytes()),
            SourceTaintedOrigin::Untainted,
            TextPosition::default(),
            SourceProviderSourceType::Program,
        ))
    }

    fn global() -> crate::runtime::js_global_object::JSGlobalObjectRef {
        use crate::runtime::js_value::js_null;
        let vm = std::rc::Rc::new(crate::runtime::vm::VM::new());
        let structure = JSGlobalObject::create_structure(&vm, js_null());
        JSGlobalObject::create(&vm, structure, js_null())
    }

    fn syntax(line: i32) -> ParserError {
        ParserError::with_message(
            ParserErrorType::SyntaxError,
            SyntaxErrorType::SyntaxErrorIrrecoverable,
            JSToken::default(),
            &WtfString::from_latin1(b"Unexpected token"),
            line,
        )
    }

    #[test]
    fn syntax_error_carries_line_column_and_url() {
        let g = global();
        let error = syntax(3).to_error_object(&g, &source("a.js")).unwrap();
        assert_eq!(error.name(), "SyntaxError");
        assert_eq!((error.line(), error.column()), (3, 0));
        assert_eq!(error.source_url(), WtfString::from_latin1(b"a.js"));
        assert!(error.is_parse_error());
        assert_eq!(syntax(3).to_error_object_override_line(&g, &source(""), 9).unwrap().line(), 9);
        assert!(syntax(3).to_error_object(&g, &source("")).unwrap().source_url().is_empty());
    }

    #[test]
    fn eval_stack_overflow_and_oom() {
        let src = source("a.js");
        let eval = ParserError::with_message(
            ParserErrorType::EvalError,
            SyntaxErrorType::SyntaxErrorNone,
            JSToken::default(),
            &WtfString::from_latin1(b"m"),
            5,
        );
        let g = global();
        let e = eval.to_error_object(&g, &src).unwrap();
        assert_eq!((e.name(), e.line()), ("SyntaxError", -1));
        let so = ParserError::with_type(ParserErrorType::StackOverflow).to_error_object(&g, &src).unwrap();
        assert_eq!((so.name(), so.is_stack_overflow_error()), ("RangeError", true));
        assert_eq!(so.message(), WtfString::from_latin1(b"Maximum call stack size exceeded."));
        let oom = ParserError::with_type(ParserErrorType::OutOfMemory).to_error_object(&g, &src).unwrap();
        assert!(oom.is_out_of_memory_error() && oom.is_parse_error());
        assert_eq!(oom.message(), WtfString::from_latin1(b"Out of memory"));
        assert!(ParserError::new().to_error_object(&g, &src).is_none());
    }

    #[test]
    fn error_object_is_a_registered_cell() {
        let g = global();
        let error = syntax(3).to_error_object(&g, &source("a.js")).unwrap();
        let found = ErrorInstance::from_cell_id(error.cell_id()).expect("célula registrada");
        assert!(Rc::ptr_eq(&found, &error));
        assert_eq!(error.as_object().type_(), JSType::ErrorInstanceType);
    }
}
