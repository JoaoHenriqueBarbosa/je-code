// Sexta fatia de `parser/Parser.cpp` (linhas 3744 a 4520), incluída por `include!` em `parser.rs`.
// Sem `use` próprio: os imports ficam em `parser.rs`.
//
// Convenções adicionais desta fatia, além das de `parser_cpp1.rs` e `parser_cpp2.rs`:
//
// - `const Identifier*` (nome importado, exportado, local) vira `Identifier` (clone barato, o ponteiro
//   nunca é nulo onde o C++ o desreferencia); o `m_token.m_data.ident` do token é lido pelo mesmo
//   `clone().unwrap_or_else(null_identifier)` de `parser_cpp2`.
// - `Vector<std::pair<const Identifier*, const Identifier*>, 8>` vira `Vec<(Identifier, Identifier)>`
//   (a capacidade inline não é observável); `UncheckedKeyHashSet<UniquedStringImpl*>` vira
//   `HashSet<Option<UniquedKey>>`, com a chave de `Identifier::impl_()`.
// - `typename TreeBuilder::ImportAttributesList attributesList = 0` vira `Option<B::ImportAttributesList>`
//   (None é o zero), como os demais `TreeX x = 0`.
// - `DepthManager` (`SetForScope` sobre `m_statementDepth`): as três chamadas desta fatia não têm
//   saída por erro entre o `m_statementDepth = 1` e o fim do escopo, então salvar e restaurar em volta
//   da chamada reproduz o destrutor.
// - `hasUnpairedSurrogate(identifier->string())` vira `identifier_has_unpaired_surrogate`, definida
//   aqui uma vez só.
// - `JSTextPosition` convertido em `int` (`createExprStatement(..., tokenEndPosition())`) vira `.offset`.
// - `RELEASE_ASSERT_NOT_REACHED()` vira `unreachable!()`.

/// `hasUnpairedSurrogate(identifier->string())`.
fn identifier_has_unpaired_surrogate(identifier: &Identifier) -> bool {
    crate::wtf::text::string_view::has_unpaired_surrogate(crate::wtf::text::string_view::StringView::from(identifier.string()))
}

impl<T: CharType> Parser<T> {
    /// `template <class TreeBuilder> TreeBuilder::ImportSpecifier parseImportClauseItem(TreeBuilder&, ImportSpecifierType)`.
    pub(crate) fn parse_import_clause_item<B: TreeBuilder>(&mut self, context: &mut B, specifier_type: ImportSpecifierType) -> Option<B::ImportSpecifier> {
        // Produced node is the item of the ImportClause.
        // That is the ImportSpecifier, ImportedDefaultBinding or NameSpaceImport.
        // http://www.ecma-international.org/ecma-262/6.0/#sec-imports
        let specifier_location = self.token_location();
        let (local_name_token, imported_name, local_name): (JSToken, Identifier, Identifier) = match specifier_type {
            ImportSpecifierType::NamespaceImport => {
                // NameSpaceImport :
                // * as ImportedBinding
                // e.g.
                //     * as namespace
                let imported_name = self.vm.property_names.star_namespace_private_name.clone();
                self.next(LexerFlagSet::empty());

                fail_if_false!(self, self.match_contextual_keyword(&self.vm.property_names.r#as), "Expected 'as' before imported binding name");
                self.next(LexerFlagSet::empty());

                fail_if_false!(self, self.match_spec_identifier(), "Expected a variable name for the import declaration");
                let local_name_token = self.token.clone();
                let local_name = self.token.data.ident.clone().unwrap_or_else(|| self.vm.property_names.null_identifier.clone());
                self.next(LexerFlagSet::empty());
                (local_name_token, imported_name, local_name)
            }

            ImportSpecifierType::NamedImport => {
                // ImportSpecifier :
                // ImportedBinding
                // IdentifierName as ImportedBinding
                // ModuleExportName as ImportedBinding
                // e.g.
                //     A
                //     A as B
                let is_module_export_name = self.match_(STRING);
                let mut local_name = self.token.data.ident.clone().unwrap_or_else(|| self.vm.property_names.null_identifier.clone());
                let imported_name = local_name.clone();
                let mut local_name_token = self.token.clone();
                if is_module_export_name {
                    fail_if_true!(self, identifier_has_unpaired_surrogate(&local_name), "Expected a well-formed-unicode string for the module export name");
                }
                self.next(LexerFlagSet::empty());

                let use_as = self.match_contextual_keyword(&self.vm.property_names.r#as);
                if is_module_export_name {
                    fail_if_false!(self, use_as, "Expected 'as' after the module export name string");
                }
                if use_as {
                    self.next(LexerFlagSet::empty());
                    fail_if_false!(self, self.match_spec_identifier(), "Expected a variable name for the import declaration");
                    local_name_token = self.token.clone();
                    local_name = self.token.data.ident.clone().unwrap_or_else(|| self.vm.property_names.null_identifier.clone());
                    self.next(LexerFlagSet::empty());
                }
                (local_name_token, imported_name, local_name)
            }

            ImportSpecifierType::DefaultImport => {
                // ImportedDefaultBinding :
                // ImportedBinding
                let local_name_token = self.token.clone();
                let local_name = self.token.data.ident.clone().unwrap_or_else(|| self.vm.property_names.null_identifier.clone());
                let imported_name = self.vm.property_names.default_keyword.clone();
                self.next(LexerFlagSet::empty());
                (local_name_token, imported_name, local_name)
            }
        };

        semantic_fail_if_true!(self, local_name_token.type_ == AWAIT, "Cannot use 'await' as an imported binding name");
        semantic_fail_if_true!(self, (local_name_token.type_ & KEYWORD_TOKEN_FLAG) != 0, "Cannot use keyword as imported binding name");
        let import_type = if matches!(specifier_type, ImportSpecifierType::NamespaceImport) { DeclarationImportType::ImportedNamespace } else { DeclarationImportType::Imported };
        let declaration_result = self.declare_variable(&local_name, DeclarationType::ConstDeclaration, import_type);
        if declaration_result != DeclarationResult::VALID {
            fail_if_true_if_strict!(self, (declaration_result & DeclarationResult::INVALID_STRICT_MODE) != 0, "Cannot declare an imported binding named ", local_name, " in strict mode");
            semantic_fail_if_true!(self, (declaration_result & DeclarationResult::INVALID_DUPLICATE_DECLARATION) != 0, "Cannot declare an imported binding name twice: '", local_name, "'");
        }

        Some(context.create_import_specifier(&specifier_location, &imported_name, &local_name))
    }

    /// `template <class TreeBuilder> TreeBuilder::ImportAttributesList parseImportAttributes(TreeBuilder&)`.
    pub(crate) fn parse_import_attributes<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::ImportAttributesList> {
        let mut keys: std::collections::HashSet<Option<UniquedKey>> = std::collections::HashSet::new();
        let attributes_list = context.create_import_attributes_list();
        consume_or_fail!(self, OPENBRACE, "Expected opening '{' at the start of import attribute");
        while !self.match_(CLOSEBRACE) {
            fail_if_false!(self, self.match_identifier_or_keyword() || self.match_(STRING), "Expected an attribute key");
            let key = self.token.data.ident.clone().unwrap_or_else(|| self.vm.property_names.null_identifier.clone());
            fail_if_false!(self, keys.insert(key.impl_()), "A duplicate key for import attributes '", key, "'");
            self.next(LexerFlagSet::empty());
            consume_or_fail!(self, COLON, "Expected ':' after attribute key");
            fail_if_false!(self, self.match_(STRING), "Expected an attribute value");
            let value = self.token.data.ident.clone().unwrap_or_else(|| self.vm.property_names.null_identifier.clone());
            self.next(LexerFlagSet::empty());
            context.append_import_assertion(&attributes_list, &key, &value);
            if !self.consume(COMMA) {
                break;
            }
        }
        handle_production_or_fail2!(self, CLOSEBRACE, "}", "end", "import attribute");
        Some(attributes_list)
    }

    /// `template <class TreeBuilder> TreeStatement parseImportDeclaration(TreeBuilder&)`.
    pub(crate) fn parse_import_declaration<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Statement> {
        // http://www.ecma-international.org/ecma-262/6.0/#sec-imports
        let import_location = self.token_location();
        self.next(LexerFlagSet::empty());

        let specifier_list = context.create_import_specifier_list();
        let mut type_ = crate::parser::nodes::ImportType::Normal;

        if self.match_(STRING) {
            // import ModuleSpecifier ;
            // import ModuleSpecifier [no LineTerminator here] WithClause ;
            let module_name = self.parse_module_name(context);
            fail_if_false!(self, module_name.is_some(), "Cannot parse the module name");

            let mut attributes_list: Option<B::ImportAttributesList> = None;
            if !self.lexer.has_line_terminator_before_token() && self.match_(WITH) {
                self.next(LexerFlagSet::empty());
                attributes_list = self.parse_import_attributes(context);
                fail_if_false!(self, attributes_list.is_some(), "Unable to parse import attributes");
            }

            fail_if_false!(self, self.auto_semi_colon(), "Expected a ';' following a targeted import declaration");
            return Some(context.create_import_declaration(&import_location, type_, specifier_list, module_name.unwrap_or_default(), attributes_list.unwrap_or_default()));
        }

        let mut is_finished_parsing_import = false;
        let mut has_import_defer = false;
        if Options::use_import_defer() && self.match_contextual_keyword(&self.vm.property_names.defer_keyword) {
            let defer_save_point = self.create_save_point(context);
            self.next(LexerFlagSet::empty());
            if self.match_(TIMES) {
                // import defer NameSpaceImport FromClause ;
                type_ = crate::parser::nodes::ImportType::Deferred;
                has_import_defer = true;
            } else {
                // import defer FromClause ;
                self.restore_save_point(context, &defer_save_point);
            }
        }

        if self.match_spec_identifier() && !has_import_defer {
            // ImportedDefaultBinding :
            // ImportedBinding
            let specifier = self.parse_import_clause_item(context, ImportSpecifierType::DefaultImport);
            fail_if_false!(self, specifier.is_some(), "Cannot parse the default import");
            context.append_import_specifier(&specifier_list, specifier.unwrap_or_default());
            if self.match_(COMMA) {
                self.next(LexerFlagSet::empty());
            } else {
                is_finished_parsing_import = true;
            }
        }

        if !is_finished_parsing_import {
            if self.match_(TIMES) {
                // import NameSpaceImport FromClause ;
                let specifier = self.parse_import_clause_item(context, ImportSpecifierType::NamespaceImport);
                fail_if_false!(self, specifier.is_some(), "Cannot parse the namespace import");
                context.append_import_specifier(&specifier_list, specifier.unwrap_or_default());
            } else {
                consume_or_fail!(self, OPENBRACE, "Expected namespace import or import list");
                // NamedImports :
                // { }
                // { ImportsList }
                // { ImportsList , }
                while !self.match_(CLOSEBRACE) {
                    fail_if_false!(self, self.match_identifier_or_keyword() || self.match_(STRING), "Expected an imported name or a module export name string for the import declaration");
                    let specifier = self.parse_import_clause_item(context, ImportSpecifierType::NamedImport);
                    fail_if_false!(self, specifier.is_some(), "Cannot parse the named import");
                    context.append_import_specifier(&specifier_list, specifier.unwrap_or_default());
                    if !self.consume(COMMA) {
                        break;
                    }
                }
                handle_production_or_fail2!(self, CLOSEBRACE, "}", "end", "import list");
            }
        }

        // FromClause :
        // from ModuleSpecifier

        fail_if_false!(self, self.match_contextual_keyword(&self.vm.property_names.from), "Expected 'from' before imported module name");
        self.next(LexerFlagSet::empty());

        let module_name = self.parse_module_name(context);
        fail_if_false!(self, module_name.is_some(), "Cannot parse the module name");

        // [no LineTerminator here] WithClause ;
        let mut attributes_list: Option<B::ImportAttributesList> = None;
        if !self.lexer.has_line_terminator_before_token() && self.match_(WITH) {
            self.next(LexerFlagSet::empty());
            attributes_list = self.parse_import_attributes(context);
            fail_if_false!(self, attributes_list.is_some(), "Unable to parse import attributes");
        }

        fail_if_false!(self, self.auto_semi_colon(), "Expected a ';' following a targeted import declaration");

        Some(context.create_import_declaration(&import_location, type_, specifier_list, module_name.unwrap_or_default(), attributes_list.unwrap_or_default()))
    }

    /// `template <class TreeBuilder> TreeBuilder::ExportSpecifier parseExportSpecifier(TreeBuilder&, Vector<...>&, bool&, bool&)`.
    pub(crate) fn parse_export_specifier<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        maybe_exported_local_names: &mut Vec<(Identifier, Identifier)>,
        has_keyword_for_local_bindings: &mut bool,
        has_referenced_module_export_names: &mut bool,
    ) -> Option<B::ExportSpecifier> {
        // ExportSpecifier :
        // IdentifierName
        // IdentifierName as IdentifierName
        // IdentifierName as ModuleExportName
        // ModuleExportName
        // ModuleExportName as IdentifierName
        // ModuleExportName as ModuleExportName
        // http://www.ecma-international.org/ecma-262/6.0/#sec-exports
        let specifier_location = self.token_location();
        let local_name = self.token.data.ident.clone().unwrap_or_else(|| self.vm.property_names.null_identifier.clone());
        let mut exported_name = local_name.clone();
        if self.match_(STRING) {
            *has_referenced_module_export_names = true;
            fail_if_true!(self, identifier_has_unpaired_surrogate(&exported_name), "Expected a well-formed-unicode string for the module export name");
        } else if (self.token.type_ & KEYWORD_TOKEN_FLAG) != 0 {
            *has_keyword_for_local_bindings = true;
        }
        self.next(LexerFlagSet::empty());

        if self.match_contextual_keyword(&self.vm.property_names.r#as) {
            self.next(LexerFlagSet::empty());
            fail_if_false!(self, self.match_identifier_or_keyword() || self.match_(STRING), "Expected an exported name or a module export name string for the export declaration");
            exported_name = self.token.data.ident.clone().unwrap_or_else(|| self.vm.property_names.null_identifier.clone());
            if self.match_(STRING) {
                fail_if_true!(self, identifier_has_unpaired_surrogate(&exported_name), "Expected a well-formed-unicode string for the module export name");
            }
            self.next(LexerFlagSet::empty());
        }

        let exported = self.export_name(&exported_name);
        semantic_fail_if_false!(self, exported, "Cannot export a duplicate name '", exported_name, "'");
        maybe_exported_local_names.push((local_name.clone(), exported_name.clone()));
        Some(context.create_export_specifier(&specifier_location, &local_name, &exported_name))
    }

    /// `template <class TreeBuilder> TreeStatement parseExportDeclaration(TreeBuilder&)`.
    pub(crate) fn parse_export_declaration<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Statement> {
        // http://www.ecma-international.org/ecma-262/6.0/#sec-exports
        let export_location = self.token_location();
        self.next(LexerFlagSet::empty());

        match self.token.type_ {
            TIMES => {
                // export * FromClause ;
                // export * as IdentifierName FromClause ;
                // export * as ModuleExportName FromClause ;
                self.next(LexerFlagSet::empty());

                let mut exported_name: Option<Identifier> = None;
                let mut specifier_location = JSTokenLocation::default();
                if self.match_contextual_keyword(&self.vm.property_names.r#as) {
                    self.next(LexerFlagSet::empty());
                    specifier_location = self.token_location();
                    fail_if_false!(self, self.match_identifier_or_keyword() || self.match_(STRING), "Expected an exported name or a module export name string for the export declaration");
                    let name = self.token.data.ident.clone().unwrap_or_else(|| self.vm.property_names.null_identifier.clone());
                    if self.match_(STRING) {
                        fail_if_true!(self, identifier_has_unpaired_surrogate(&name), "Expected a well-formed-unicode string for the module export name");
                    }
                    exported_name = Some(name);
                    self.next(LexerFlagSet::empty());
                }

                fail_if_false!(self, self.match_contextual_keyword(&self.vm.property_names.from), "Expected 'from' before exported module name");
                self.next(LexerFlagSet::empty());
                let module_name = self.parse_module_name(context);
                fail_if_false!(self, module_name.is_some(), "Cannot parse the 'from' clause");

                // [no LineTerminator here] WithClause ;
                let mut attributes_list: Option<B::ImportAttributesList> = None;
                if !self.lexer.has_line_terminator_before_token() && self.match_(WITH) {
                    self.next(LexerFlagSet::empty());
                    attributes_list = self.parse_import_attributes(context);
                    fail_if_false!(self, attributes_list.is_some(), "Unable to parse import attributes");
                }

                fail_if_false!(self, self.auto_semi_colon(), "Expected a ';' following a targeted export declaration");

                if let Some(exported_name) = exported_name {
                    let exported = self.export_name(&exported_name);
                    semantic_fail_if_false!(self, exported, "Cannot export a duplicate name '", exported_name, "'");
                    let specifier_list = context.create_export_specifier_list();
                    let local_name = self.vm.property_names.star_namespace_private_name.clone();
                    let specifier = context.create_export_specifier(&specifier_location, &local_name, &exported_name);
                    context.append_export_specifier(&specifier_list, specifier);
                    return Some(context.create_export_named_declaration(&export_location, specifier_list, module_name.unwrap_or_default(), attributes_list.unwrap_or_default()));
                }

                Some(context.create_export_all_declaration(&export_location, module_name.unwrap_or_default(), attributes_list.unwrap_or_default()))
            }

            DEFAULT => {
                // export default HoistableDeclaration[~Yield, ~Await, +Default]
                // export default ClassDeclaration[~Yield, ~Await, +Default]
                // export default [lookahead not-in { function, async [no LineTerminator here] function, class }] AssignmentExpression[+In, ~Yield, ~Await]

                self.next(LexerFlagSet::empty());

                let result: Option<B::Statement>;
                let mut is_function_or_class_declaration = false;
                let mut local_name: Option<Identifier> = None;

                let starts_with_function = self.match_(FUNCTION);
                if starts_with_function || self.match_(CLASSTOKEN) {
                    let save_point = self.create_save_point(context);
                    is_function_or_class_declaration = true;
                    self.next(LexerFlagSet::empty());

                    // ES6 Generators
                    if starts_with_function && self.match_(TIMES) {
                        self.next(LexerFlagSet::empty());
                    }
                    if self.match_(IDENT) {
                        local_name = self.token.data.ident.clone();
                    }
                    self.restore_save_point(context, &save_point);
                } else if self.match_contextual_keyword(&self.vm.property_names.r#async) {
                    // export default async function xxx() { }
                    // export default async function * yyy() { }
                    let save_point = self.create_save_point(context);
                    self.next(LexerFlagSet::empty());
                    if self.match_(FUNCTION) && !self.lexer.has_line_terminator_before_token() {
                        self.next(LexerFlagSet::empty());
                        // Async Generators
                        self.consume(TIMES);
                        if self.match_(IDENT) {
                            local_name = self.token.data.ident.clone();
                        }
                        is_function_or_class_declaration = true;
                    }
                    self.restore_save_point(context, &save_point);
                }

                let local_name = local_name.unwrap_or_else(|| self.vm.property_names.star_default_private_name.clone());

                if is_function_or_class_declaration {
                    if starts_with_function {
                        let old_statement_depth = self.statement_depth;
                        self.statement_depth = 1;
                        result = self.parse_function_declaration(context, FunctionDeclarationType::Declaration, ExportType::NotExported, DeclarationDefaultContext::ExportDefault, None);
                        self.statement_depth = old_statement_depth;
                    } else if self.match_(CLASSTOKEN) {
                        result = self.parse_class_declaration(context, ExportType::NotExported, DeclarationDefaultContext::ExportDefault);
                    } else {
                        let function_start = self.token_start();
                        self.next(LexerFlagSet::empty());
                        let old_statement_depth = self.statement_depth;
                        self.statement_depth = 1;
                        result = self.parse_async_function_declaration(context, function_start, ExportType::NotExported, DeclarationDefaultContext::ExportDefault, None);
                        self.statement_depth = old_statement_depth;
                    }
                } else {
                    // export default expr;
                    //
                    // It should be treated as the same to the following.
                    //
                    // const *default* = expr;
                    // export { *default* as default }
                    //
                    // In the above example, *default* is the invisible variable to the users.
                    // We use the private symbol to represent the name of this variable.
                    let location = self.token_location();
                    let start = *self.token_start_position();
                    let expression = self.parse_assignment_expression(context);
                    fail_if_false!(self, expression.is_some(), "Cannot parse expression");

                    let star_default_private_name = self.vm.property_names.star_default_private_name.clone();
                    let declaration_result = self.declare_variable(&star_default_private_name, DeclarationType::ConstDeclaration, DeclarationImportType::NotImported);
                    semantic_fail_if_true!(self, (declaration_result & DeclarationResult::INVALID_DUPLICATE_DECLARATION) != 0, "Only one 'default' export is allowed");

                    let assignment = context.create_assign_resolve(&location, &star_default_private_name, expression.unwrap_or_default(), start, start, *self.token_end_position(), AssignmentContext::ConstDeclarationStatement);
                    result = Some(context.create_expr_statement(&location, assignment, start, self.token_end_position().offset));
                    fail_if_false!(self, self.auto_semi_colon(), "Expected a ';' following a targeted export declaration");
                }
                fail_if_false!(self, result.is_some(), "Cannot parse the declaration");

                let default_keyword = self.vm.property_names.default_keyword.clone();
                let exported = self.export_name(&default_keyword);
                semantic_fail_if_false!(self, exported, "Only one 'default' export is allowed");
                if let Some(data) = &self.module_scope_data {
                    data.export_binding(&local_name, &default_keyword);
                }
                Some(context.create_export_default_declaration(&export_location, result.unwrap_or_default(), &local_name))
            }

            OPENBRACE => {
                // export ExportClause FromClause ;
                // export ExportClause ;
                //
                // ExportClause :
                // { }
                // { ExportsList }
                // { ExportsList , }
                //
                // ExportsList :
                // ExportSpecifier
                // ExportsList , ExportSpecifier

                self.next(LexerFlagSet::empty());

                let specifier_list = context.create_export_specifier_list();
                let mut maybe_exported_local_names: Vec<(Identifier, Identifier)> = Vec::new();

                let mut has_keyword_for_local_bindings = false;
                let mut has_referenced_module_export_names = false;
                while !self.match_(CLOSEBRACE) {
                    fail_if_false!(self, self.match_identifier_or_keyword() || self.match_(STRING), "Expected a variable name or a module export name string for the export declaration");
                    let specifier = self.parse_export_specifier(context, &mut maybe_exported_local_names, &mut has_keyword_for_local_bindings, &mut has_referenced_module_export_names);
                    fail_if_false!(self, specifier.is_some(), "Cannot parse the named export");
                    context.append_export_specifier(&specifier_list, specifier.unwrap_or_default());
                    if !self.consume(COMMA) {
                        break;
                    }
                }
                handle_production_or_fail2!(self, CLOSEBRACE, "}", "end", "export list");

                let mut module_name: Option<B::ModuleName> = None;
                let mut attributes_list: Option<B::ImportAttributesList> = None;
                if self.match_contextual_keyword(&self.vm.property_names.from) {
                    self.next(LexerFlagSet::empty());
                    module_name = self.parse_module_name(context);
                    fail_if_false!(self, module_name.is_some(), "Cannot parse the 'from' clause");

                    // [no LineTerminator here] WithClause ;
                    if !self.lexer.has_line_terminator_before_token() && self.match_(WITH) {
                        self.next(LexerFlagSet::empty());
                        attributes_list = self.parse_import_attributes(context);
                        fail_if_false!(self, attributes_list.is_some(), "Unable to parse import attributes");
                    }
                } else {
                    semantic_fail_if_true!(self, has_referenced_module_export_names, "Cannot use module export names if they reference variable names in the current module");
                }
                fail_if_false!(self, self.auto_semi_colon(), "Expected a ';' following a targeted export declaration");

                if module_name.is_none() {
                    semantic_fail_if_true!(self, has_keyword_for_local_bindings, "Cannot use keyword as exported variable name");
                    // Since this export declaration does not have module specifier part, it exports the local bindings.
                    // While the export declaration with module specifier does not have any effect on the current module's scope,
                    // the export named declaration without module specifier references the local binding names.
                    // For example,
                    //   export { A, B, C as D } from "mod"
                    // does not have effect on the current module's scope. But,
                    //   export { A, B, C as D }
                    // will reference the current module's bindings.
                    for (local_name, exported_name) in &maybe_exported_local_names {
                        if let Some(data) = &self.module_scope_data {
                            data.export_binding(local_name, exported_name);
                        }
                    }
                }

                Some(context.create_export_named_declaration(&export_location, specifier_list, module_name.unwrap_or_default(), attributes_list.unwrap_or_default()))
            }

            _ => {
                // export VariableStatement
                // export Declaration
                let result: Option<B::Statement>;
                match self.token.type_ {
                    VAR => {
                        result = self.parse_variable_declaration(context, DeclarationType::VarDeclaration, ExportType::Exported);
                    }

                    CONSTTOKEN => {
                        result = self.parse_variable_declaration(context, DeclarationType::ConstDeclaration, ExportType::Exported);
                    }

                    LET => {
                        result = self.parse_variable_declaration(context, DeclarationType::LetDeclaration, ExportType::Exported);
                    }

                    FUNCTION => {
                        let old_statement_depth = self.statement_depth;
                        self.statement_depth = 1;
                        result = self.parse_function_declaration(context, FunctionDeclarationType::Declaration, ExportType::Exported, DeclarationDefaultContext::Standard, None);
                        self.statement_depth = old_statement_depth;
                    }

                    CLASSTOKEN => {
                        result = self.parse_class_declaration(context, ExportType::Exported, DeclarationDefaultContext::Standard);
                    }

                    // O `case IDENT` do C++ cai no `default` (`[[fallthrough]]`) quando não é `async` sem escape.
                    IDENT if self.token.data.ident.as_ref() == Some(&self.vm.property_names.r#async) && !self.token.data.escaped => {
                        let function_start = self.token_start();
                        self.next(LexerFlagSet::empty());
                        semantic_fail_if_false!(self, self.match_(FUNCTION) && !self.lexer.has_line_terminator_before_token(), "Expected 'function' keyword following 'async' keyword with no preceding line terminator");
                        let old_statement_depth = self.statement_depth;
                        self.statement_depth = 1;
                        result = self.parse_async_function_declaration(context, function_start, ExportType::Exported, DeclarationDefaultContext::Standard, None);
                        self.statement_depth = old_statement_depth;
                    }

                    _ => {
                        fail_with_message!(self, "Expected either a declaration or a variable statement");
                    }
                }

                fail_if_false!(self, result.is_some(), "Cannot parse the declaration");
                Some(context.create_export_local_declaration(&export_location, result.unwrap_or_default()))
            }
        }
    }

    /// `template <class TreeBuilder> TreeExpression parseExpression(TreeBuilder&)`.
    pub(crate) fn parse_expression<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Expression> {
        fail_if_stack_overflow!(self);
        let head_location = self.token_location();
        let node = self.parse_assignment_expression(context);
        fail_if_false!(self, node.is_some(), "Cannot parse expression");
        let node = node.unwrap_or_default();
        context.set_end_offset(&node, self.last_token_location.end_offset as i32);
        if !self.match_(COMMA) {
            return Some(node);
        }
        self.record_pause_location(context.breakpoint_location(&node));
        self.next(LexerFlagSet::empty());
        self.parser_state.non_trivial_expression_count += 1;
        self.parser_state.non_lhs_count += 1;
        let mut tail_location = self.token_location();
        let right = self.parse_assignment_expression(context);
        fail_if_false!(self, right.is_some(), "Cannot parse expression in a comma expression");
        let right = right.unwrap_or_default();
        self.record_pause_location(context.breakpoint_location(&right));
        context.set_end_offset(&right, self.last_token_location.end_offset as i32);
        let head = context.create_comma_expr(&head_location, node);
        let mut tail = context.append_to_comma_expr(&tail_location, head.clone(), right);
        while self.match_(COMMA) {
            self.next(B::DONT_BUILD_STRINGS);
            tail_location = self.token_location();
            let right = self.parse_assignment_expression(context);
            fail_if_false!(self, right.is_some(), "Cannot parse expression in a comma expression");
            let right = right.unwrap_or_default();
            context.set_end_offset(&right, self.last_token_location.end_offset as i32);
            self.record_pause_location(context.breakpoint_location(&right));
            tail = context.append_to_comma_expr(&tail_location, tail, right);
        }
        // O `Comma*` do C++ converte para `ExpressionNode*` (mesmo nó) ao devolver.
        let head: B::Expression = head.into();
        context.set_end_offset(&head, self.last_token_location.end_offset as i32);
        Some(head)
    }

    /// `template <typename TreeBuilder> NEVER_INLINE const char* metaPropertyName(TreeBuilder&, TreeExpression)`.
    pub(crate) fn meta_property_name<B: TreeBuilder>(&self, context: &mut B, expr: &B::Expression) -> &'static str {
        if context.is_new_target(expr) {
            return "new.target";
        }
        if context.is_import_meta(expr) {
            return "import.meta";
        }
        // Invariante: só é chamada com a expressão de uma meta-propriedade (new.target ou import.meta).
        unreachable!()
    }

    /// `template <typename TreeBuilder> bool isSimpleAssignmentTarget(TreeBuilder&, TreeExpression, bool)`.
    pub(crate) fn is_simple_assignment_target<B: TreeBuilder>(&self, context: &mut B, expr: &B::Expression, ignore_strict_check: bool) -> bool {
        // Web compatibility concerns prevent us from handling a function call LHS as an early error in sloppy mode.
        // See https://github.com/tc39/ecma262/pull/3568 for details.
        context.is_location(expr) || (!(self.strict_mode() || ignore_strict_check) && context.is_function_call(expr))
    }

    /// `template <typename TreeBuilder> TreeExpression parseDestructuringAssignment(TreeBuilder&, SavePoint&, const JSTokenLocation&, bool)`.
    pub(crate) fn parse_destructuring_assignment<B: TreeBuilder>(&mut self, context: &mut B, save_point: &SavePoint, location: &JSTokenLocation, is_possible_pattern: bool) -> Option<B::Expression> {
        let expression_error_location = self.swap_save_point_for_error(context, save_point);
        let pattern = self.try_parse_destructuring_pattern_expression(context, AssignmentContext::AssignmentExpression);

        // The reason why we use restoreSavePointWithError only when isPossiblePattern = true is that
        // this can produce better error message.
        if is_possible_pattern && (pattern.is_none() || !self.match_(EQUAL)) {
            self.restore_save_point_with_error(context, &expression_error_location);
            propagate_error!(self);
        }
        fail_if_false!(self, pattern.is_some(), "Cannot parse assignment pattern");
        consume_or_fail!(self, EQUAL, "Expected '=' following assignment pattern");
        let rhs = self.parse_assignment_expression(context);
        if rhs.is_none() {
            propagate_error!(self);
        }
        Some(context.create_destructuring_assignment(location, pattern.unwrap_or_default(), rhs.unwrap_or_default()))
    }

    /// `template <typename TreeBuilder> TreeExpression parseArrowFunctionCandidate(TreeBuilder&, SavePoint&, const JSTokenLocation&, bool, bool, size_t, bool&)`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn parse_arrow_function_candidate<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        save_point: &SavePoint,
        location: &JSTokenLocation,
        is_arrow_function_token: bool,
        was_open_paren: bool,
        used_variables_size: usize,
        should_return_result: &mut bool,
    ) -> Option<B::Expression> {
        let error_restoration_save_point = self.swap_save_point_for_error(context, save_point);
        let mut is_async = false;
        if self.match_contextual_keyword(&self.vm.property_names.r#async) {
            self.next(LexerFlagSet::empty());
            if !self.lexer.has_line_terminator_before_token() && (self.match_(OPENPAREN) || self.match_spec_identifier()) {
                is_async = true;
            } else {
                // This is async => ... case. So this "async" is not a contextual keyword, it is parameter name.
                self.restore_save_point(context, save_point);
            }
        }

        if self.is_arrow_function_parameters(context) {
            if was_open_paren {
                let scope = self.current_scope();
                self.scope_stack[scope].revert_to_previous_used_variables(used_variables_size);
            }
            *should_return_result = true;
            return self.parse_arrow_function_expression(context, is_async, location);
        }

        // The reason why we use propagateError only when isArrowFunctionToken = true is that
        // this can produce better error message than restoring it to errorRestorationSavePoint.
        if is_arrow_function_token && self.has_error() {
            *should_return_result = true;
            return None;
        }

        self.restore_save_point_with_error(context, &error_restoration_save_point);
        if is_arrow_function_token {
            *should_return_result = true;
            fail_due_to_unexpected_token!(self);
        }
        None
    }

    /// `template <typename TreeBuilder> TreeExpression parseAssignmentExpression(TreeBuilder&)`.
    pub(crate) fn parse_assignment_expression<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Expression> {
        fail_if_stack_overflow!(self);

        if self.match_(YIELD) && !self.can_use_identifier_yield() {
            return self.parse_yield_expression(context);
        }

        let mut start = *self.token_start_position();
        let location = self.token_location();
        let initial_assignment_count = self.parser_state.assignment_count;
        let initial_non_lhs_count = self.parser_state.non_lhs_count;
        let maybe_assignment_pattern = self.match_(OPENBRACE) || self.match_(OPENBRACKET);
        let was_open_paren = self.match_(OPENPAREN);
        // Do not use matchSpecIdentifier() here since it is slower than isIdentifierOrKeyword.
        // Whether spec identifier is will be validated by isArrowFunctionParameters().
        let was_identifier_or_keyword = self.match_identifier_or_keyword() || self.token.type_ == ESCAPED_KEYWORD;
        let maybe_valid_arrow_function_start = was_open_paren || was_identifier_or_keyword;
        let mut save_point: Option<SavePoint> = None;
        if maybe_valid_arrow_function_start || maybe_assignment_pattern {
            save_point = Some(self.create_save_point(context));
        }
        let mut used_variables_size = 0usize;

        if was_open_paren {
            let scope = self.current_scope();
            used_variables_size = self.scope_stack[scope].current_used_variables_size();
            self.scope_stack[scope].push_used_variable_set();
        }

        let lhs = self.parse_conditional_expression(context);

        // Current implementation of parseAssignmentExpression causes a weird parsing loop
        // for this example:
        //
        //      class C {
        //          static {
        //              ((x = await) => 0);
        //          }
        //      }
        //
        // which makes the 'await' error caught in parseConditionalExpression escaping from
        // parseAssignmentExpression. Therefore, we need to capture the error directly after
        // parseConditionalExpression. Besides, the usage of `await` is strictly limited in
        // class static block.
        {
            let scope = self.current_scope();
            if lhs.is_none() && self.scope_stack[scope].is_static_block() && self.match_(AWAIT) {
                propagate_error!(self);
            }
        }

        if maybe_valid_arrow_function_start && !self.match_(EOFTOK) {
            let is_arrow_function_token = self.match_(ARROWFUNCTION);
            if lhs.is_none() || is_arrow_function_token {
                let mut should_return_result = false;
                // Invariante: `maybeValidArrowFunctionStart` implica `savePoint` criado acima.
                let candidate_save_point = save_point.as_ref().expect("savePoint criado quando maybeValidArrowFunctionStart");
                let result = self.parse_arrow_function_candidate(context, candidate_save_point, &location, is_arrow_function_token, was_open_paren, used_variables_size, &mut should_return_result);
                if should_return_result {
                    return result;
                }
            }
        }

        if lhs.is_none() && !maybe_assignment_pattern {
            propagate_error!(self);
        }

        if maybe_assignment_pattern && (lhs.is_none() || (self.match_(EQUAL) && context.is_object_or_array_literal(lhs.as_ref().unwrap_or(&Default::default())))) {
            // Invariante: `maybeAssignmentPattern` implica `savePoint` criado acima.
            let pattern_save_point = save_point.as_ref().expect("savePoint criado quando maybeAssignmentPattern");
            return self.parse_destructuring_assignment(context, pattern_save_point, &location, lhs.is_none());
        }

        fail_if_false!(self, lhs.is_some(), "Cannot parse expression");
        let mut lhs = lhs.unwrap_or_default();
        if initial_non_lhs_count != self.parser_state.non_lhs_count {
            semantic_fail_if_true!(self, self.token.type_ >= EQUAL && self.token.type_ <= ANDEQUAL, "Left hand side of operator '", self.get_token(), "' must be a reference");
            return Some(lhs);
        }

        let mut assignment_stack: i32 = 0;
        let mut had_assignment = false;
        loop {
            let op = match self.token.type_ {
                EQUAL => Operator::Equal,
                PLUSEQUAL => Operator::PlusEq,
                MINUSEQUAL => Operator::MinusEq,
                MULTEQUAL => Operator::MultEq,
                DIVEQUAL => Operator::DivEq,
                LSHIFTEQUAL => Operator::LShift,
                RSHIFTEQUAL => Operator::RShift,
                URSHIFTEQUAL => Operator::URShift,
                BITANDEQUAL => Operator::BitAndEq,
                BITXOREQUAL => Operator::BitXOrEq,
                BITOREQUAL => Operator::BitOrEq,
                MODEQUAL => Operator::ModEq,
                POWEQUAL => Operator::PowEq,
                COALESCEEQUAL => Operator::CoalesceEq,
                OREQUAL => Operator::OrEq,
                ANDEQUAL => Operator::AndEq,
                _ => break,
            };
            self.parser_state.non_trivial_expression_count += 1;
            had_assignment = true;
            semantic_fail_if_true!(self, context.is_meta_property(&lhs), self.meta_property_name(context, &lhs), " can't be the left hand side of an assignment expression");
            // Even if in sloppy mode, we should throw a syntax error for logical assignment expressions that are not simple.
            // https://tc39.es/ecma262/#sec-assignment-operators-static-semantics-early-errors
            let is_simple_assignment_target = self.is_simple_assignment_target(context, &lhs, op == Operator::CoalesceEq || op == Operator::OrEq || op == Operator::AndEq);
            semantic_fail_if_false!(self, is_simple_assignment_target, "Left side of assignment is not a reference");
            context.assignment_stack_append(&mut assignment_stack, lhs.clone(), start, *self.token_start_position(), self.parser_state.assignment_count, op);
            start = *self.token_start_position();
            self.parser_state.assignment_count += 1;
            self.next(B::DONT_BUILD_STRINGS);
            if self.strict_mode() {
                if let Some(last_identifier) = self.parser_state.last_identifier.clone() {
                    if context.is_resolve(&lhs) {
                        fail_if_true_if_strict!(self, self.vm.property_names.eval == last_identifier, "Cannot modify 'eval' in strict mode");
                        fail_if_true_if_strict!(self, self.vm.property_names.arguments == last_identifier, "Cannot modify 'arguments' in strict mode");
                        self.parser_state.last_identifier = None;
                    }
                }
            }
            let rhs = self.parse_assignment_expression(context);
            fail_if_false!(self, rhs.is_some(), "Cannot parse the right hand side of an assignment expression");
            lhs = rhs.unwrap_or_default();
            if initial_non_lhs_count != self.parser_state.non_lhs_count {
                semantic_fail_if_true!(self, self.token.type_ >= EQUAL && self.token.type_ <= ANDEQUAL, "Left hand side of operator '", self.get_token(), "' must be a reference");
                break;
            }
        }
        if had_assignment {
            self.parser_state.non_lhs_count += 1;
        }

        while assignment_stack != 0 {
            lhs = context.create_assignment(&location, &mut assignment_stack, lhs, initial_assignment_count, self.parser_state.assignment_count, self.last_token_end_position());
        }

        Some(lhs)
    }

    /// `template <class TreeBuilder> TreeExpression parseYieldExpression(TreeBuilder&)`.
    pub(crate) fn parse_yield_expression<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Expression> {
        // YieldExpression[In] :
        //     yield
        //     yield [no LineTerminator here] AssignmentExpression[?In, Yield]
        //     yield [no LineTerminator here] * AssignmentExpression[?In, Yield]

        // http://ecma-international.org/ecma-262/6.0/#sec-generator-function-definitions
        let scope = self.current_scope();
        fail_if_false!(self, self.scope_stack[scope].is_generator_function() && !self.scope_stack[scope].is_arrow_function_boundary(), "Cannot use yield expression out of generator");

        // http://ecma-international.org/ecma-262/6.0/#sec-generator-function-definitions-static-semantics-early-errors
        fail_if_true!(self, self.parser_state.function_parse_phase == FunctionParsePhase::Parameters, "Cannot use yield expression within parameters");

        // https://github.com/tc39/ecma262/issues/3333
        fail_if_true!(self, self.parser_state.is_parsing_class_field_initializer, "Cannot use yield expression inside class field initializer expression");

        let location = self.token_location();
        let divot_start = *self.token_start_position();
        let save_point = self.create_save_point(context);
        self.next(LexerFlagSet::empty());
        if self.lexer.has_line_terminator_before_token() {
            return Some(context.create_yield(&location));
        }

        let delegate = self.consume(TIMES);
        let argument_start = *self.token_start_position();
        let argument = self.parse_assignment_expression(context);
        let Some(argument) = argument else {
            self.restore_save_point(context, &save_point);
            self.next(LexerFlagSet::empty());
            return Some(context.create_yield(&location));
        };
        Some(context.create_yield_argument(&location, argument, delegate, divot_start, argument_start, self.last_token_end_position()))
    }
}
