// `ParsedNode`, `IsEvalNode` e `IsFunctionMetadataNode` para os nós de topo que `Parser::parse<ParsedNode>`
// instancia (Parser.h 2230 a 2245): `ProgramNode`, `EvalNode`, `ModuleProgramNode` e `FunctionNode`.
// Cada nó ignora os argumentos que o seu construtor C++ ignora (a macro `scope_node!` já faz isso).
// `FunctionMetadataNode` não é `ParsedNode` no C++ (não tem `scopeIsFunction` nem o construtor de
// `ScopeNode`); só a sobrecarga `isFunctionMetadataNode(FunctionMetadataNode*)` é dele.

use std::rc::Rc;

use crate::parser::ast_builder::Link;
use crate::parser::module_scope_data::ModuleScopeData;
use crate::parser::nodes::{
    EvalNode, FunctionMetadataNode, FunctionNode, FunctionParameters, FunctionStack, ModuleProgramNode,
    ProgramNode, SourceElements,
};
use crate::parser::parser::{IsEvalNode, IsFunctionMetadataNode, ParsedNode};
use crate::parser::parser_arena::ParserArena;
use crate::parser::parser_modes::{
    CodeFeatures, InnerArrowFunctionCodeFeatures, LexicallyScopedFeatures,
};
use crate::parser::parser_tokens::JSTokenLocation;
use crate::parser::source_code::SourceCode;
use crate::parser::variable_environment::VariableEnvironment;

impl IsEvalNode for ProgramNode {}
impl IsEvalNode for ModuleProgramNode {}
impl IsEvalNode for FunctionNode {}
impl IsEvalNode for FunctionMetadataNode {}
impl IsEvalNode for EvalNode {
    const IS_EVAL_NODE: bool = true;
}

impl IsFunctionMetadataNode for ProgramNode {}
impl IsFunctionMetadataNode for EvalNode {}
impl IsFunctionMetadataNode for ModuleProgramNode {}
impl IsFunctionMetadataNode for FunctionNode {}
impl IsFunctionMetadataNode for FunctionMetadataNode {
    const IS_FUNCTION_METADATA_NODE: bool = true;
}

/// O corpo de `ParsedNode::create` é o mesmo nos quatro nós: só o tipo e `SCOPE_IS_FUNCTION` mudam.
macro_rules! impl_parsed_node {
    ($name:ident, $scope_is_function:expr) => {
        impl ParsedNode for $name {
            const SCOPE_IS_FUNCTION: bool = $scope_is_function;

            fn create(
                parser_arena: &mut ParserArena,
                start_location: &JSTokenLocation,
                end_location: &JSTokenLocation,
                start_column: u32,
                end_column: u32,
                source_elements: Link<SourceElements>,
                var_declarations: VariableEnvironment,
                function_declarations: FunctionStack,
                lexical_variables: VariableEnvironment,
                parameters: Link<FunctionParameters>,
                source: &SourceCode,
                features: CodeFeatures,
                lexically_scoped_features: LexicallyScopedFeatures,
                inner_arrow_function_features: InnerArrowFunctionCodeFeatures,
                num_constants: i32,
                module_scope_data: Option<Rc<ModuleScopeData>>,
            ) -> Box<Self> {
                Box::new($name::new(
                    parser_arena,
                    start_location,
                    end_location,
                    start_column,
                    end_column,
                    source_elements.opt(),
                    var_declarations,
                    function_declarations,
                    lexical_variables,
                    parameters.opt(),
                    source,
                    features,
                    lexically_scoped_features,
                    inner_arrow_function_features,
                    num_constants,
                    module_scope_data,
                ))
            }

            fn set_loc(&mut self, first_line: u32, last_line: u32, start_offset: i32, line_start_offset: i32) {
                self.base.set_loc(first_line, last_line, start_offset, line_start_offset);
            }

            fn set_end_offset(&mut self, offset: i32) {
                self.base.set_end_offset(offset);
            }
        }
    };
}

impl_parsed_node!(ProgramNode, false);
impl_parsed_node!(EvalNode, false);
impl_parsed_node!(ModuleProgramNode, false);
impl_parsed_node!(FunctionNode, true);
