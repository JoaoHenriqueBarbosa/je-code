//! Tradução de `JavaScriptCore/parser/ParserFunctionInfo.h`.
//!
//! No C++ as duas structs são templates sobre o `TreeBuilder`; aqui são genéricas sobre o trait
//! `crate::parser::tree_builder::TreeBuilder` e guardam o `FunctionBody` do construtor da árvore.

use crate::parser::tree_builder::TreeBuilder;
use crate::runtime::identifier::Identifier;

/// `ParserFunctionInfo<TreeBuilder>`. O `const Identifier* name` (ponteiro nulo por padrão) vira
/// `Option<Identifier>`; `body` começa em `0` no C++, que é o valor padrão do tipo associado.
pub struct ParserFunctionInfo<T: TreeBuilder> {
    pub name: Option<Identifier>,
    pub body: T::FunctionBody,
    pub parameter_count: u32,
    pub function_length: u32,
    pub start_offset: u32,
    pub end_offset: u32,
    pub start_line: i32,
    pub end_line: i32,
    pub parameters_start_column: u32,
}

impl<T: TreeBuilder> Default for ParserFunctionInfo<T> {
    fn default() -> Self {
        ParserFunctionInfo {
            name: None,
            body: T::FunctionBody::default(),
            parameter_count: 0,
            function_length: 0,
            start_offset: 0,
            end_offset: 0,
            start_line: 0,
            end_line: 0,
            parameters_start_column: 0,
        }
    }
}

/// `ParserClassInfo<TreeBuilder>`.
pub struct ParserClassInfo<T: TreeBuilder> {
    pub class_name: Option<Identifier>,
    pub start_offset: u32,
    pub end_offset: u32,
    pub start_line: i32,
    pub start_column: u32,
    _marker: std::marker::PhantomData<T>,
}

impl<T: TreeBuilder> Default for ParserClassInfo<T> {
    fn default() -> Self {
        ParserClassInfo {
            class_name: None,
            start_offset: 0,
            end_offset: 0,
            start_line: 0,
            start_column: 0,
            _marker: std::marker::PhantomData,
        }
    }
}
