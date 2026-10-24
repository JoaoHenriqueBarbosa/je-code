// Fatia do `Nodes.h` de `ProgramNode` até o fim, com os construtores do `NodeConstructors.h` e do `Nodes.cpp`.
// Incluída por `include!` em `nodes.rs`: compartilha os `use` e as macros do módulo.
//
// Notas desta fatia:
//
// - Os tipos aninhados do C++ viram tipos do módulo com o nome da classe na frente, como `PropertyNodeType`:
//   `ImportDeclarationNode::ImportType` é `ImportType`, `DefineFieldNode::Type` é `DefineFieldType`,
//   `ArrayPatternNode::BindingType` é `ArrayPatternBindingType` e `ObjectPatternNode::BindingType` é
//   `ObjectPatternBindingType`.
// - `ParserArenaDeletable`, `ParserArenaFreeable` e `JSC_MAKE_PARSER_ARENA_DELETABLE_ALLOCATED` somem: a posse
//   compartilhada por `NodeRef` (`Rc<RefCell<T>>`) os substitui.
// - `DestructuringPatternNode` é abstrata e sem campos, mas o `TryNode` a guarda por ponteiro:
//   ela vira um `enum` por classe concreta que é a própria alça (como `Expression`), sem base comum.
// - `collectBoundIdentifiers`, `toString`, `bindValue` e afins dos padrões de destructuring vivem no
//   `NodesCodegen.cpp` e só o bytecompiler os usa: ficam para a camada dele.

/// Classe com `m_next` própria, encadeada por `NodeRef`: o que `NodeList` precisa para ligar o rabo.
pub trait ChainNode: Sized {
    fn next_mut(&mut self) -> &mut Option<NodeRef<Self>>;
}

impl ChainNode for ElementNode {
    fn next_mut(&mut self) -> &mut Option<NodeRef<ElementNode>> {
        &mut self.next
    }
}

impl ChainNode for PropertyListNode {
    fn next_mut(&mut self) -> &mut Option<NodeRef<PropertyListNode>> {
        &mut self.next
    }
}

impl ChainNode for ArgumentListNode {
    fn next_mut(&mut self) -> &mut Option<NodeRef<ArgumentListNode>> {
        &mut self.next
    }
}

impl ChainNode for ClauseListNode {
    fn next_mut(&mut self) -> &mut Option<NodeRef<ClauseListNode>> {
        &mut self.next
    }
}

/// `ElementList`, `PropertyList`, `ArgumentList` e `ClauseList` do C++ (`{ head, tail }`).
///
/// Como no C++: `head` e `tail` são alças para os mesmos nós, e `push` faz `tail->m_next = node; tail = node`
/// em tempo constante.
pub struct NodeList<T: ChainNode> {
    pub head: Option<NodeRef<T>>,
    pub tail: Option<NodeRef<T>>,
}

impl<T: ChainNode> NodeList<T> {
    pub fn new() -> Self {
        NodeList { head: None, tail: None }
    }

    /// `tail->m_next = node; tail = node`.
    pub fn push(&mut self, node: NodeRef<T>) {
        match self.tail.replace(node.clone()) {
            Some(tail) => *tail.borrow_mut().next_mut() = Some(node),
            None => self.head = Some(node),
        }
    }

    /// O `head` que o C++ guardaria.
    pub fn into_head(self) -> Option<NodeRef<T>> {
        self.head
    }
}

impl<T: ChainNode> Default for NodeList<T> {
    fn default() -> Self {
        NodeList::new()
    }
}

pub type ElementList = NodeList<ElementNode>;
pub type PropertyList = NodeList<PropertyListNode>;
pub type ArgumentList = NodeList<ArgumentListNode>;
pub type ClauseList = NodeList<ClauseListNode>;

/// Declara uma subclasse final de `ScopeNode` (`ProgramNode`, `EvalNode`, `ModuleProgramNode`,
/// `FunctionNode`): o construtor repassa os argumentos comuns ao `ScopeNode` e inicializa os campos
/// próprios. Os identificadores entre parênteses nomeiam, nesta ordem, os parâmetros `startColumn`,
/// `endColumn`, `FunctionParameters*`, `features` e `RefPtr<ModuleScopeData>&&` do construtor do C++
/// (os que a classe ignora levam prefixo `_`).
macro_rules! scope_node {
    (
        $(#[$meta:meta])*
        $name:ident { $($field:ident : $ty:ty = $init:expr),* $(,)? }
        ($start_column:ident, $end_column:ident, $parameters:ident, $features:ident, $module_scope_data:ident)
    ) => {
        $(#[$meta])*
        pub struct $name {
            pub base: ScopeNode,
            $(pub $field: $ty),*
        }

        inherit!($name => ScopeNode);

        impl $name {
            pub fn new(
                parser_arena: &mut ParserArena,
                start_location: &JSTokenLocation,
                end_location: &JSTokenLocation,
                $start_column: u32,
                $end_column: u32,
                children: Option<NodeRef<SourceElements>>,
                var_environment: VariableEnvironment,
                func_stack: FunctionStack,
                lexical_variables: VariableEnvironment,
                $parameters: Option<NodeRef<FunctionParameters>>,
                source: &SourceCode,
                $features: CodeFeatures,
                lexically_scoped_features: LexicallyScopedFeatures,
                inner_arrow_function_code_features: InnerArrowFunctionCodeFeatures,
                num_constants: i32,
                $module_scope_data: Option<Rc<ModuleScopeData>>,
            ) -> Self {
                $name {
                    base: ScopeNode::with_statements(
                        parser_arena,
                        start_location,
                        end_location,
                        source,
                        children,
                        var_environment,
                        func_stack,
                        lexical_variables,
                        $features,
                        lexically_scoped_features,
                        inner_arrow_function_code_features,
                        num_constants,
                    ),
                    $($field: $init),*
                }
            }
        }
    };
}

scope_node!(
    ProgramNode {
        start_column: u32 = start_column,
        end_column: u32 = end_column,
    }
    (start_column, end_column, _parameters, features, _module_scope_data)
);

scope_node!(
    /// `startColumn()` é sempre 0 (o `EvalNode` ignora o argumento do construtor).
    EvalNode {
        end_column: u32 = end_column,
    }
    (_start_column, end_column, _parameters, features, _module_scope_data)
);

impl EvalNode {
    pub fn start_column(&self) -> u32 {
        0
    }
}

scope_node!(
    ModuleProgramNode {
        start_column: u32 = start_column,
        end_column: u32 = end_column,
        uses_await: bool = (features & AWAIT_FEATURE) != 0,
        // `*WTF::move(moduleScopeData)`: o `RefPtr` nunca é nulo para um módulo (invariante do parser).
        module_scope_data: Rc<ModuleScopeData> = module_scope_data.expect("ModuleProgramNode sem ModuleScopeData"),
    }
    (start_column, end_column, _parameters, features, module_scope_data)
);

/// `m_moduleName` é `const Identifier&` no C++; aqui o nó guarda uma cópia.
pub struct ModuleNameNode {
    pub base: Node,
    pub module_name: Identifier,
}

inherit!(ModuleNameNode => Node);

impl ModuleNameNode {
    pub fn new(location: &JSTokenLocation, module_name: Identifier) -> Self {
        ModuleNameNode { base: Node::new(location), module_name }
    }
}

pub struct ImportSpecifierNode {
    pub base: Node,
    pub imported_name: Identifier,
    pub local_name: Identifier,
}

inherit!(ImportSpecifierNode => Node);

impl ImportSpecifierNode {
    pub fn new(location: &JSTokenLocation, imported_name: Identifier, local_name: Identifier) -> Self {
        ImportSpecifierNode { base: Node::new(location), imported_name, local_name }
    }
}

#[derive(Default)]
pub struct ImportSpecifierListNode {
    pub specifiers: Vec<NodeRef<ImportSpecifierNode>>,
}

impl ImportSpecifierListNode {
    pub fn append(&mut self, specifier: NodeRef<ImportSpecifierNode>) {
        self.specifiers.push(specifier);
    }
}

#[derive(Default)]
pub struct ImportAttributesListNode {
    pub attributes: Vec<(Identifier, Identifier)>,
}

impl ImportAttributesListNode {
    pub fn append(&mut self, key: Identifier, value: Identifier) {
        self.attributes.push((key, value));
    }
}

/// `ImportDeclarationNode::ImportType`. A classe abstrata `ModuleDeclarationNode` não tem campos: os filhos
/// (`ImportDeclarationNode`, `ExportAllDeclarationNode`, ...) apontam direto para o `StatementNode`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportType {
    Normal,
    Deferred,
}

pub struct ImportDeclarationNode {
    pub base: StatementNode,
    pub specifier_list: NodeRef<ImportSpecifierListNode>,
    pub module_name: NodeRef<ModuleNameNode>,
    pub attributes_list: Option<NodeRef<ImportAttributesListNode>>,
    pub type_: ImportType,
}

inherit!(ImportDeclarationNode => StatementNode);

impl ImportDeclarationNode {
    pub fn new(
        location: &JSTokenLocation,
        type_: ImportType,
        import_specifier_list: NodeRef<ImportSpecifierListNode>,
        module_name: NodeRef<ModuleNameNode>,
        import_attributes_list: Option<NodeRef<ImportAttributesListNode>>,
    ) -> Self {
        ImportDeclarationNode {
            base: StatementNode::new(location),
            specifier_list: import_specifier_list,
            module_name,
            attributes_list: import_attributes_list,
            type_,
        }
    }
}

pub struct ExportAllDeclarationNode {
    pub base: StatementNode,
    pub module_name: NodeRef<ModuleNameNode>,
    pub attributes_list: Option<NodeRef<ImportAttributesListNode>>,
}

inherit!(ExportAllDeclarationNode => StatementNode);

impl ExportAllDeclarationNode {
    pub fn new(
        location: &JSTokenLocation,
        module_name: NodeRef<ModuleNameNode>,
        import_attributes_list: Option<NodeRef<ImportAttributesListNode>>,
    ) -> Self {
        ExportAllDeclarationNode {
            base: StatementNode::new(location),
            module_name,
            attributes_list: import_attributes_list,
        }
    }
}

pub struct ExportDefaultDeclarationNode {
    pub base: StatementNode,
    pub declaration: Statement,
    pub local_name: Identifier,
}

inherit!(ExportDefaultDeclarationNode => StatementNode);

impl ExportDefaultDeclarationNode {
    pub fn new(location: &JSTokenLocation, declaration: Statement, local_name: Identifier) -> Self {
        ExportDefaultDeclarationNode { base: StatementNode::new(location), declaration, local_name }
    }
}

pub struct ExportLocalDeclarationNode {
    pub base: StatementNode,
    pub declaration: Statement,
}

inherit!(ExportLocalDeclarationNode => StatementNode);

impl ExportLocalDeclarationNode {
    pub fn new(location: &JSTokenLocation, declaration: Statement) -> Self {
        ExportLocalDeclarationNode { base: StatementNode::new(location), declaration }
    }
}

pub struct ExportSpecifierNode {
    pub base: Node,
    pub local_name: Identifier,
    pub exported_name: Identifier,
}

inherit!(ExportSpecifierNode => Node);

impl ExportSpecifierNode {
    pub fn new(location: &JSTokenLocation, local_name: Identifier, exported_name: Identifier) -> Self {
        ExportSpecifierNode { base: Node::new(location), local_name, exported_name }
    }
}

#[derive(Default)]
pub struct ExportSpecifierListNode {
    pub specifiers: Vec<NodeRef<ExportSpecifierNode>>,
}

impl ExportSpecifierListNode {
    pub fn append(&mut self, specifier: NodeRef<ExportSpecifierNode>) {
        self.specifiers.push(specifier);
    }
}

pub struct ExportNamedDeclarationNode {
    pub base: StatementNode,
    pub specifier_list: NodeRef<ExportSpecifierListNode>,
    pub module_name: Option<NodeRef<ModuleNameNode>>,
    pub attributes_list: Option<NodeRef<ImportAttributesListNode>>,
}

inherit!(ExportNamedDeclarationNode => StatementNode);

impl ExportNamedDeclarationNode {
    pub fn new(
        location: &JSTokenLocation,
        export_specifier_list: NodeRef<ExportSpecifierListNode>,
        module_name: Option<NodeRef<ModuleNameNode>>,
        import_attributes_list: Option<NodeRef<ImportAttributesListNode>>,
    ) -> Self {
        ExportNamedDeclarationNode {
            base: StatementNode::new(location),
            specifier_list: export_specifier_list,
            module_name,
            attributes_list: import_attributes_list,
        }
    }
}

/// `FunctionMetadataNode` fica num `Rc` (o `FuncDeclNode`, o `BaseFuncExprNode` e a pilha de funções do
/// escopo apontam para o mesmo nó) e o parser o altera depois de criado: `setLoc`, `setEndPosition`,
/// `finishParsing`, `overrideName`, `setEcmaName`, `setClassSource` e os `set*` de flags. Só esses campos
/// têm mutabilidade interior (`Cell`/`RefCell`); os que o construtor fixa ficam como valor simples. O
/// `Node` base também (o `setLoc` troca a posição), por isso `base` é `RefCell<Node>` e não há `Deref`.
///
/// Omitidos: `dump` (depuração por `PrintStream`) e o construtor que recebe `ParserArena&` e a ignora (é o
/// mesmo `new`, sem a arena).
pub struct FunctionMetadataNode {
    pub base: RefCell<Node>,
    pub implementation_visibility: ImplementationVisibility,
    pub lexically_scoped_features: LexicallyScopedFeatures,
    pub super_binding: SuperBinding,
    pub constructor_kind: ConstructorKind,
    /// `NeedsClassFieldInitializer` (`bool` no C++): `No` é `false`.
    pub needs_class_field_initializer: Cell<bool>,
    pub is_arrow_function_body_expression: bool,
    pub is_sloppy_mode_hoisted_function: Cell<bool>,
    pub private_brand_requirement: Cell<PrivateBrandRequirement>,
    pub parse_mode: SourceParseMode,
    pub function_mode: Cell<FunctionMode>,
    pub ident: RefCell<Identifier>,
    pub ecma_name: RefCell<Identifier>,
    pub start_column: u32,
    pub end_column: Cell<u32>,
    pub function_start: u32,
    pub function_name_start: i32,
    pub parameters_start: i32,
    pub source: RefCell<SourceCode>,
    pub class_source: RefCell<SourceCode>,
    pub start_start_offset: i32,
    pub parameter_count: u32,
    pub last_line: Cell<i32>,
}

impl FunctionMetadataNode {
    pub fn new(
        start_location: &JSTokenLocation,
        end_location: &JSTokenLocation,
        start_column: u32,
        end_column: u32,
        function_start: u32,
        function_name_start: i32,
        parameters_start: i32,
        implementation_visibility: ImplementationVisibility,
        lexically_scoped_features: LexicallyScopedFeatures,
        constructor_kind: ConstructorKind,
        super_binding: SuperBinding,
        parameter_count: u32,
        mode: SourceParseMode,
        is_arrow_function_body_expression: bool,
    ) -> Self {
        FunctionMetadataNode {
            base: RefCell::new(Node::new(end_location)),
            implementation_visibility,
            lexically_scoped_features,
            super_binding,
            constructor_kind,
            needs_class_field_initializer: Cell::new(false),
            is_arrow_function_body_expression,
            is_sloppy_mode_hoisted_function: Cell::new(false),
            private_brand_requirement: Cell::new(PrivateBrandRequirement::None),
            parse_mode: mode,
            // O C++ deixa `m_functionMode` sem valor até o `finishParsing`.
            function_mode: Cell::new(FunctionMode::None),
            ident: RefCell::new(Identifier::default()),
            ecma_name: RefCell::new(Identifier::default()),
            start_column,
            end_column: Cell::new(end_column),
            function_start,
            function_name_start,
            parameters_start,
            source: RefCell::new(SourceCode::default()),
            class_source: RefCell::new(SourceCode::default()),
            start_start_offset: start_location.start_offset as i32,
            parameter_count,
            last_line: Cell::new(0),
        }
    }

    pub fn finish_parsing(&self, source: &SourceCode, ident: &Identifier, function_mode: FunctionMode) {
        *self.source.borrow_mut() = source.clone();
        *self.ident.borrow_mut() = ident.clone();
        self.function_mode.set(function_mode);
    }

    /// `isConstructorAndNeedsClassFieldInitializer()`: o campo `m_needsClassFieldInitializer`.
    pub fn is_constructor_and_needs_class_field_initializer(&self) -> bool {
        self.needs_class_field_initializer.get()
    }

    /// `setNeedsClassFieldInitializer(bool)`.
    pub fn set_needs_class_field_initializer(&self, value: bool) {
        debug_assert!(!value || self.constructor_kind != ConstructorKind::None);
        self.needs_class_field_initializer.set(value);
    }

    /// `ecmaName()`: o nome próprio quando existe, senão o da especificação.
    pub fn ecma_name(&self) -> Identifier {
        let ident = self.ident.borrow();
        if ident.is_empty() {
            self.ecma_name.borrow().clone()
        } else {
            ident.clone()
        }
    }

    /// `setEcmaName`: `m_ecmaName = m_name.isNull() ? name : m_name`.
    pub fn set_ecma_name(&self, name: &Identifier) {
        let own_name = self.ident.borrow().clone();
        *self.ecma_name.borrow_mut() = if own_name.is_null() { name.clone() } else { own_name };
    }

    pub fn set_class_source(&self, class_source: &SourceCode) {
        *self.class_source.borrow_mut() = class_source.clone();
    }

    /// `overrideName(const Identifier&)`: `m_ident = ident`.
    pub fn override_name(&self, ident: &Identifier) {
        *self.ident.borrow_mut() = ident.clone();
    }

    pub fn set_end_position(&self, position: JSTextPosition) {
        self.last_line.set(position.line);
        self.end_column.set(position.column() as u32);
    }

    pub fn set_loc(&self, first_line: u32, last_line: u32, start_offset: i32, line_start_offset: i32) {
        self.last_line.set(last_line as i32);
        self.base.borrow_mut().position = JSTextPosition::new(first_line as i32, start_offset, line_start_offset);
    }

    pub fn last_line(&self) -> u32 {
        self.last_line.get() as u32
    }
}

impl PartialEq for FunctionMetadataNode {
    fn eq(&self, other: &FunctionMetadataNode) -> bool {
        self.parse_mode == other.parse_mode
            && self.implementation_visibility == other.implementation_visibility
            && self.lexically_scoped_features == other.lexically_scoped_features
            && self.super_binding == other.super_binding
            && self.constructor_kind == other.constructor_kind
            && self.is_arrow_function_body_expression == other.is_arrow_function_body_expression
            && self.is_sloppy_mode_hoisted_function.get() == other.is_sloppy_mode_hoisted_function.get()
            && *self.ident.borrow() == *other.ident.borrow()
            && *self.ecma_name.borrow() == *other.ecma_name.borrow()
            && self.function_mode.get() == other.function_mode.get()
            && self.start_column == other.start_column
            && self.end_column.get() == other.end_column.get()
            && self.function_start == other.function_start
            && self.function_name_start == other.function_name_start
            && self.parameters_start == other.parameters_start
            && *self.source.borrow() == *other.source.borrow()
            && *self.class_source.borrow() == *other.class_source.borrow()
            && self.start_start_offset == other.start_start_offset
            && self.parameter_count == other.parameter_count
            && self.last_line.get() == other.last_line.get()
            && self.base.borrow().position == other.base.borrow().position
    }
}

scope_node!(
    FunctionNode {
        ident: Identifier = Identifier::default(),
        // O C++ deixa `m_functionMode` sem valor até o `finishParsing`.
        function_mode: FunctionMode = FunctionMode::None,
        parameters: Option<NodeRef<FunctionParameters>> = parameters,
        start_column: u32 = start_column,
        end_column: u32 = end_column,
    }
    (start_column, end_column, parameters, features, _module_scope_data)
);

impl FunctionNode {
    pub fn finish_parsing(&mut self, ident: Identifier, function_mode: FunctionMode) {
        self.ident = ident;
        self.function_mode = function_mode;
    }
}

/// Classe abstrata: pai de `FuncExprNode` e `ArrowFuncExprNode`.
pub struct BaseFuncExprNode {
    pub base: ExpressionNode,
    pub metadata: Rc<FunctionMetadataNode>,
}

inherit!(BaseFuncExprNode => ExpressionNode);

impl BaseFuncExprNode {
    pub fn new(
        location: &JSTokenLocation,
        ident: &Identifier,
        metadata: Rc<FunctionMetadataNode>,
        source: &SourceCode,
        function_mode: FunctionMode,
    ) -> Self {
        metadata.finish_parsing(source, ident, function_mode);
        BaseFuncExprNode { base: ExpressionNode::new(location), metadata }
    }
}

pub struct FuncExprNode {
    pub base: BaseFuncExprNode,
}

inherit!(FuncExprNode => BaseFuncExprNode);

impl FuncExprNode {
    pub fn new(
        location: &JSTokenLocation,
        ident: &Identifier,
        metadata: Rc<FunctionMetadataNode>,
        source: &SourceCode,
    ) -> Self {
        Self::with_function_mode(location, ident, metadata, source, FunctionMode::FunctionExpression)
    }

    pub fn with_function_mode(
        location: &JSTokenLocation,
        ident: &Identifier,
        metadata: Rc<FunctionMetadataNode>,
        source: &SourceCode,
        function_mode: FunctionMode,
    ) -> Self {
        FuncExprNode { base: BaseFuncExprNode::new(location, ident, metadata, source, function_mode) }
    }
}

pub struct ArrowFuncExprNode {
    pub base: BaseFuncExprNode,
}

inherit!(ArrowFuncExprNode => BaseFuncExprNode);

impl ArrowFuncExprNode {
    pub fn new(
        location: &JSTokenLocation,
        ident: &Identifier,
        metadata: Rc<FunctionMetadataNode>,
        source: &SourceCode,
    ) -> Self {
        ArrowFuncExprNode {
            base: BaseFuncExprNode::new(location, ident, metadata, source, FunctionMode::FunctionExpression),
        }
    }
}

pub struct MethodDefinitionNode {
    pub base: FuncExprNode,
}

inherit!(MethodDefinitionNode => FuncExprNode);

impl MethodDefinitionNode {
    pub fn new(
        location: &JSTokenLocation,
        ident: &Identifier,
        metadata: Rc<FunctionMetadataNode>,
        source: &SourceCode,
    ) -> Self {
        MethodDefinitionNode {
            base: FuncExprNode::with_function_mode(location, ident, metadata, source, FunctionMode::MethodDefinition),
        }
    }
}

pub struct YieldExprNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub argument: Option<Expression>,
    pub delegate: bool,
}

inherit!(YieldExprNode => ExpressionNode);

impl YieldExprNode {
    pub fn new(location: &JSTokenLocation, argument: Option<Expression>, delegate: bool) -> Self {
        YieldExprNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::default(),
            argument,
            delegate,
        }
    }
}

pub struct AwaitExprNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub argument: Expression,
}

inherit!(AwaitExprNode => ExpressionNode);

impl AwaitExprNode {
    pub fn new(location: &JSTokenLocation, argument: Expression) -> Self {
        AwaitExprNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::default(),
            argument,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefineFieldType {
    Name,
    PrivateName,
    ComputedName,
}

pub struct DefineFieldNode {
    pub base: StatementNode,
    pub ident: Identifier,
    pub assign: Option<Expression>,
    pub type_: DefineFieldType,
}

inherit!(DefineFieldNode => StatementNode);

impl DefineFieldNode {
    pub fn new(location: &JSTokenLocation, ident: Identifier, assign: Option<Expression>, type_: DefineFieldType) -> Self {
        DefineFieldNode { base: StatementNode::new(location), ident, assign, type_ }
    }
}

pub struct ClassExprNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub variable_environment: VariableEnvironmentNode,
    pub class_head_environment: VariableEnvironment,
    pub class_source: SourceCode,
    pub name: Identifier,
    /// `const Identifier* m_ecmaName`: o C++ o inicia apontando para `m_name`, então `Some(name)` desde o início.
    pub ecma_name: Option<Identifier>,
    pub constructor_expression: Option<Expression>,
    pub class_heritage: Option<Expression>,
    pub class_elements: Option<NodeRef<PropertyListNode>>,
    pub needs_lexical_scope: bool,
}

inherit!(ClassExprNode => ExpressionNode);

impl ClassExprNode {
    pub fn new(
        location: &JSTokenLocation,
        name: Identifier,
        class_source: SourceCode,
        class_head_environment: VariableEnvironment,
        class_environment: VariableEnvironment,
        constructor_expression: Option<Expression>,
        class_heritage: Option<Expression>,
        class_elements: Option<NodeRef<PropertyListNode>>,
    ) -> Self {
        let needs_lexical_scope = PropertyListNode::should_create_lexical_scope_for_class(class_elements.as_ref());
        ClassExprNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::default(),
            variable_environment: VariableEnvironmentNode::with_lexical_variables(class_environment),
            class_head_environment,
            class_source,
            ecma_name: Some(name.clone()),
            name,
            constructor_expression,
            class_heritage,
            class_elements,
            needs_lexical_scope,
        }
    }

    pub fn ecma_name(&self) -> &Identifier {
        self.ecma_name.as_ref().unwrap_or(&self.name)
    }

    pub fn set_ecma_name(&mut self, name: &Identifier) {
        self.ecma_name = Some(if self.name.is_null() { name.clone() } else { self.name.clone() });
    }

    pub fn has_static_property(&self, prop_name: &Identifier) -> bool {
        self.class_elements.as_ref().is_some_and(|elements| elements.borrow().has_statically_named_property(prop_name))
    }

    pub fn has_instance_fields(&self) -> bool {
        self.class_elements.as_ref().is_some_and(|elements| elements.borrow().has_instance_fields())
    }
}

/// Classe abstrata com métodos virtuais e sem campos: um `enum` por classe concreta (ver a nota do topo). O
/// próprio enum é a alça (clonar copia o ponteiro) e dois valores são iguais quando apontam para o mesmo nó.
#[derive(Clone)]
pub enum DestructuringPatternNode {
    ArrayPattern(NodeRef<ArrayPatternNode>),
    ObjectPattern(NodeRef<ObjectPatternNode>),
    Binding(NodeRef<BindingNode>),
    RestParameter(NodeRef<RestParameterNode>),
    AssignmentElement(NodeRef<AssignmentElementNode>),
}

impl PartialEq for DestructuringPatternNode {
    fn eq(&self, other: &Self) -> bool {
        use DestructuringPatternNode::*;
        match (self, other) {
            (ArrayPattern(a), ArrayPattern(b)) => Rc::ptr_eq(a, b),
            (ObjectPattern(a), ObjectPattern(b)) => Rc::ptr_eq(a, b),
            (Binding(a), Binding(b)) => Rc::ptr_eq(a, b),
            (RestParameter(a), RestParameter(b)) => Rc::ptr_eq(a, b),
            (AssignmentElement(a), AssignmentElement(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl DestructuringPatternNode {
    pub fn is_binding_node(&self) -> bool {
        matches!(self, DestructuringPatternNode::Binding(_))
    }

    pub fn is_assignment_element_node(&self) -> bool {
        matches!(self, DestructuringPatternNode::AssignmentElement(_))
    }

    pub fn is_rest_parameter(&self) -> bool {
        matches!(self, DestructuringPatternNode::RestParameter(_))
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayPatternBindingType {
    Elision,
    Element,
    RestElement,
}

pub struct ArrayPatternEntry {
    pub binding_type: ArrayPatternBindingType,
    pub pattern: Option<DestructuringPatternNode>,
    pub default_value: Option<Expression>,
}

pub struct ArrayPatternNode {
    pub throwable: ThrowableExpressionData,
    pub target_patterns: Vec<ArrayPatternEntry>,
}

impl ArrayPatternNode {
    pub fn new() -> Self {
        ArrayPatternNode { throwable: ThrowableExpressionData::default(), target_patterns: Vec::new() }
    }

    pub fn append_index(
        &mut self,
        binding_type: ArrayPatternBindingType,
        _location: &JSTokenLocation,
        node: Option<DestructuringPatternNode>,
        default_value: Option<Expression>,
    ) {
        self.target_patterns.push(ArrayPatternEntry { binding_type, pattern: node, default_value });
    }
}

impl Default for ArrayPatternNode {
    fn default() -> Self {
        ArrayPatternNode::new()
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectPatternBindingType {
    Element,
    RestElement,
}

pub struct ObjectPatternEntry {
    pub property_name: Identifier,
    pub property_expression: Option<Expression>,
    pub was_string: bool,
    pub pattern: DestructuringPatternNode,
    pub default_value: Option<Expression>,
    pub binding_type: ObjectPatternBindingType,
}

pub struct ObjectPatternNode {
    pub throwable: ThrowableExpressionData,
    pub contains_rest_element: bool,
    pub contains_computed_property: bool,
    pub target_patterns: Vec<ObjectPatternEntry>,
}

impl ObjectPatternNode {
    pub fn new() -> Self {
        ObjectPatternNode {
            throwable: ThrowableExpressionData::default(),
            contains_rest_element: false,
            contains_computed_property: false,
            target_patterns: Vec::new(),
        }
    }

    /// `appendEntry(location, identifier, wasString, pattern, defaultValue, bindingType)`.
    pub fn append_entry(
        &mut self,
        _location: &JSTokenLocation,
        identifier: Identifier,
        was_string: bool,
        pattern: DestructuringPatternNode,
        default_value: Option<Expression>,
        binding_type: ObjectPatternBindingType,
    ) {
        self.target_patterns.push(ObjectPatternEntry {
            property_name: identifier,
            property_expression: None,
            was_string,
            pattern,
            default_value,
            binding_type,
        });
    }

    /// `appendEntry(vm, location, propertyExpression, pattern, defaultValue, bindingType)`.
    pub fn append_entry_with_expression(
        &mut self,
        vm: &VM,
        _location: &JSTokenLocation,
        property_expression: Expression,
        pattern: DestructuringPatternNode,
        default_value: Option<Expression>,
        binding_type: ObjectPatternBindingType,
    ) {
        self.target_patterns.push(ObjectPatternEntry {
            property_name: vm.property_names.null_identifier.clone(),
            property_expression: Some(property_expression),
            was_string: false,
            pattern,
            default_value,
            binding_type,
        });
    }
}

impl Default for ObjectPatternNode {
    fn default() -> Self {
        ObjectPatternNode::new()
    }
}

pub struct BindingNode {
    pub divot_start: JSTextPosition,
    pub divot_end: JSTextPosition,
    pub bound_property: Identifier,
    pub binding_context: AssignmentContext,
}

impl BindingNode {
    pub fn new(
        bound_property: Identifier,
        start: JSTextPosition,
        end: JSTextPosition,
        context: AssignmentContext,
    ) -> Self {
        BindingNode { divot_start: start, divot_end: end, bound_property, binding_context: context }
    }
}

pub struct RestParameterNode {
    pub pattern: DestructuringPatternNode,
    pub num_parameters_to_skip: u32,
}

impl RestParameterNode {
    pub fn new(pattern: DestructuringPatternNode, num_parameters_to_skip: u32) -> Self {
        RestParameterNode { pattern, num_parameters_to_skip }
    }
}

pub struct AssignmentElementNode {
    pub divot_start: JSTextPosition,
    pub divot_end: JSTextPosition,
    pub assignment_target: Expression,
}

impl AssignmentElementNode {
    pub fn new(assignment_target: Expression, start: JSTextPosition, end: JSTextPosition) -> Self {
        AssignmentElementNode { divot_start: start, divot_end: end, assignment_target }
    }
}

pub struct DestructuringAssignmentNode {
    pub base: ExpressionNode,
    pub bindings: DestructuringPatternNode,
    pub initializer: Option<Expression>,
}

inherit!(DestructuringAssignmentNode => ExpressionNode);

impl DestructuringAssignmentNode {
    pub fn new(
        location: &JSTokenLocation,
        bindings: DestructuringPatternNode,
        initializer: Option<Expression>,
    ) -> Self {
        DestructuringAssignmentNode { base: ExpressionNode::new(location), bindings, initializer }
    }
}

pub struct FunctionParameters {
    pub patterns: Vec<(DestructuringPatternNode, Option<Expression>)>,
    pub is_simple_parameter_list: bool,
}

impl FunctionParameters {
    pub fn new() -> Self {
        FunctionParameters { patterns: Vec::new(), is_simple_parameter_list: true }
    }

    pub fn append(&mut self, pattern: DestructuringPatternNode, default_value: Option<Expression>) {
        // https://tc39.es/ecma262/#sec-functiondeclarationinstantiation
        // Implementa `IsSimpleParameterList` da ECMA 2015: é falsa quando a lista tem algum valor padrão, um
        // parâmetro rest ou qualquer padrão de destructuring. Nesse caso o objeto `arguments` é criado como o
        // do modo estrito e os parâmetros são alocados em outro escopo.
        let has_default_parameter_value = default_value.is_some();
        let is_simple_parameter = !has_default_parameter_value && pattern.is_binding_node();
        self.is_simple_parameter_list &= is_simple_parameter;

        self.patterns.push((pattern, default_value));
    }
}

impl Default for FunctionParameters {
    fn default() -> Self {
        FunctionParameters::new()
    }
}

pub struct FuncDeclNode {
    pub base: StatementNode,
    pub metadata: Rc<FunctionMetadataNode>,
}

inherit!(FuncDeclNode => StatementNode);

impl FuncDeclNode {
    pub fn new(
        location: &JSTokenLocation,
        ident: &Identifier,
        metadata: Rc<FunctionMetadataNode>,
        source: &SourceCode,
    ) -> Self {
        metadata.finish_parsing(source, ident, FunctionMode::FunctionDeclaration);
        FuncDeclNode { base: StatementNode::new(location), metadata }
    }
}

pub struct ClassDeclNode {
    pub base: StatementNode,
    pub class_declaration: Expression,
}

inherit!(ClassDeclNode => StatementNode);

impl ClassDeclNode {
    pub fn new(location: &JSTokenLocation, class_declaration: Expression) -> Self {
        ClassDeclNode { base: StatementNode::new(location), class_declaration }
    }
}

/// `expr` é nulo na cláusula `default`.
pub struct CaseClauseNode {
    pub expr: Option<Expression>,
    pub statements: Option<NodeRef<SourceElements>>,
    /// O C++ deixa `m_startOffset` sem valor até o `setStartOffset`.
    pub start_offset: i32,
}

impl CaseClauseNode {
    pub fn new(expr: Option<Expression>, statements: Option<NodeRef<SourceElements>>) -> Self {
        CaseClauseNode { expr, statements, start_offset: 0 }
    }
}

pub struct ClauseListNode {
    pub clause: NodeRef<CaseClauseNode>,
    pub next: Option<NodeRef<ClauseListNode>>,
}

impl ClauseListNode {
    pub fn new(clause: NodeRef<CaseClauseNode>) -> Self {
        ClauseListNode { clause, next: None }
    }

    /// `ClauseListNode(clauseList, clause)`.
    pub fn append(clause_list: &NodeRef<ClauseListNode>, clause: NodeRef<CaseClauseNode>) -> NodeRef<ClauseListNode> {
        let tail = node(ClauseListNode::new(clause));
        clause_list.borrow_mut().next = Some(tail.clone());
        tail
    }
}

pub struct CaseBlockNode {
    pub list1: Option<NodeRef<ClauseListNode>>,
    pub default_clause: Option<NodeRef<CaseClauseNode>>,
    pub list2: Option<NodeRef<ClauseListNode>>,
}

impl CaseBlockNode {
    /// `s_tableSwitchMinimum`.
    pub const TABLE_SWITCH_MINIMUM: usize = 3;

    pub fn new(
        list1: Option<NodeRef<ClauseListNode>>,
        default_clause: Option<NodeRef<CaseClauseNode>>,
        list2: Option<NodeRef<ClauseListNode>>,
    ) -> Self {
        CaseBlockNode { list1, default_clause, list2 }
    }
}

pub struct SwitchNode {
    pub base: StatementNode,
    pub variable_environment: VariableEnvironmentNode,
    pub expr: Expression,
    pub block: NodeRef<CaseBlockNode>,
}

inherit!(SwitchNode => StatementNode);

impl SwitchNode {
    pub fn new(
        location: &JSTokenLocation,
        expr: Expression,
        block: NodeRef<CaseBlockNode>,
        lexical_variables: VariableEnvironment,
        function_stack: FunctionStack,
    ) -> Self {
        SwitchNode {
            base: StatementNode::new(location),
            variable_environment: VariableEnvironmentNode::with_function_stack(lexical_variables, function_stack),
            expr,
            block,
        }
    }
}
