//! Tradução de `parser/ModuleAnalyzer.{h,cpp}` e de `parser/NodesAnalyzeModule.cpp`: percorre a AST de
//! um módulo e preenche o registro (entradas de import, de export, `export *`, módulos requisitados).
//!
//! DIVERGÊNCIAS: o arquivo mora em `runtime/` (depende de `JSModuleRecord`). `analyzeModule` virtual dos
//! `ModuleDeclarationNode` vira um `match` sobre `Statement` (a hierarquia é um enum). A ramificação
//! `USE(BUN_JSC_ADDITIONS)` de `tryCreateAttributes` é a ligada: atributo que não é `type` é ignorado, e
//! um `type` desconhecido que não é vazio vira `HostDefined` (`ScriptFetchParameters::parseType`). O
//! `Options::dumpModuleRecord()` (`dump`) não existe.

use std::collections::HashSet;
use std::rc::Rc;

use crate::parser::nodes::{ImportAttributesListNode, ImportType, ModuleProgramNode, NodeRef, Statement};
use crate::parser::parser_modes::CodeFeatures;
use crate::parser::source_code::SourceCode;
use crate::parser::variable_environment::VariableEnvironmentEntry;
use crate::runtime::abstract_module_record::{
    AbstractModuleRecordRef, ExportEntry, ImportEntry, ImportEntryType, ModulePhase,
};
use crate::runtime::error_type::ErrorType;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_module_record::create_js_module_record;
use crate::runtime::module_map::ModuleMapKey;
use crate::runtime::script_fetch_parameters::{ScriptFetchParameters, ScriptFetchParametersRef, ScriptFetchParametersType};
use crate::runtime::vm::VM;
use crate::wtf::text::string_impl::UniquedKey;
use crate::wtf::text::wtf_string::String as WtfString;

/// `std::tuple<ErrorType, String>`.
pub type AnalyzeError = (ErrorType, String);

/// `tryCreateAttributes(vm, attributesList)`: `Ok(None)` é o `RefPtr<ScriptFetchParameters> { }`.
fn try_create_attributes(
    vm: &VM,
    attributes_list: &Option<NodeRef<ImportAttributesListNode>>,
) -> Result<Option<ScriptFetchParametersRef>, AnalyzeError> {
    let Some(attributes_list) = attributes_list else { return Ok(None) };

    let mut type_: Option<ScriptFetchParametersType> = None;
    let mut host_defined_import_type = WtfString::default();
    for (key, value) in attributes_list.borrow().attributes.iter() {
        if *key == vm.property_names.r#type {
            let value_text = String::from_utf8_lossy(&value.utf8()).into_owned();
            type_ = ScriptFetchParameters::parse_type(&value_text);
            if type_.is_none() {
                return Err((ErrorType::TypeError, format!("Import attribute type \"{value_text}\" is not valid")));
            }
            if type_ == Some(ScriptFetchParametersType::HostDefined) {
                host_defined_import_type = value.string().string().clone();
            }
        }
    }

    Ok(match type_ {
        Some(ScriptFetchParametersType::HostDefined) => Some(ScriptFetchParameters::create_host_defined(&host_defined_import_type)),
        Some(type_) => Some(ScriptFetchParameters::create(type_)),
        None => None,
    })
}

/// `class ModuleAnalyzer`.
pub struct ModuleAnalyzer<'a> {
    vm: &'a VM,
    module_record: AbstractModuleRecordRef,
    /// `m_requestedModules`, um conjunto por `ModulePhase`.
    requested_modules: [HashSet<ModuleMapKey>; 2],
    /// `m_errorMessage`.
    error: Option<AnalyzeError>,
}

impl<'a> ModuleAnalyzer<'a> {
    /// `ModuleAnalyzer(globalObject, moduleKey, sourceCode, features)`.
    pub fn new(vm: &'a VM, module_key: &Identifier, source_code: &SourceCode, features: CodeFeatures) -> ModuleAnalyzer<'a> {
        ModuleAnalyzer {
            vm,
            module_record: create_js_module_record(module_key.clone(), source_code.clone(), features),
            requested_modules: [HashSet::new(), HashSet::new()],
            error: None,
        }
    }

    /// `appendRequestedModule(specifier, attributes, phase)`.
    fn append_requested_module(&mut self, specifier: &Identifier, attributes: Option<ScriptFetchParametersRef>, phase: ModulePhase) {
        let type_ = attributes.as_ref().map(|attributes| attributes.type_()).unwrap_or(ScriptFetchParametersType::JavaScript);
        let key: ModuleMapKey = (specifier.impl_(), type_);
        if self.requested_modules[phase as usize].insert(key) {
            self.module_record.append_requested_module(specifier, attributes, phase);
        }
    }

    /// `fail(errorMessage)` seguido de `return false`.
    fn fail(&mut self, error: AnalyzeError) -> bool {
        self.error = Some(error);
        false
    }

    /// `exportVariable(moduleProgramNode, localName, variable)`.
    fn export_variable(&self, node: &ModuleProgramNode, local_name: &UniquedKey, variable: &VariableEnvironmentEntry) {
        // In the parser, we already marked the variables as Exported and Imported.
        if !variable.is_exported() {
            return;
        }
        let vm = self.vm;
        let local_key = Some(local_name.clone());
        let export_names: Vec<Option<UniquedKey>> =
            node.module_scope_data.exported_bindings().get(&local_key).cloned().unwrap_or_default();
        let local_identifier = Identifier::from_uid(vm, Some(local_name));

        // Exported module local variable.
        if !variable.is_imported() {
            for export_name in &export_names {
                self.module_record
                    .add_export_entry(&ExportEntry::create_local(&Identifier::from_uid(vm, export_name.as_ref()), &local_identifier));
            }
            return;
        }

        let import_entry = self.module_record.try_get_import_entry(&local_key).expect("export de import sem ImportEntry");

        if variable.is_imported_namespace() {
            // Exported namespace binding: import * as namespace from "mod"; export { namespace }
            for export_name in &export_names {
                let export_identifier = Identifier::from_uid(vm, export_name.as_ref());
                if import_entry.phase == ModulePhase::Defer {
                    self.module_record.add_export_entry(&ExportEntry::create_local(&export_identifier, &local_identifier));
                } else {
                    self.module_record.add_export_entry(&ExportEntry::create_namespace(
                        &export_identifier,
                        &import_entry.module_request,
                        import_entry.module_request_type,
                    ));
                }
            }
            return;
        }

        // Indirectly exported binding: import a from "mod"; export { a }
        for export_name in &export_names {
            self.module_record.add_export_entry(&ExportEntry::create_indirect(
                &Identifier::from_uid(vm, export_name.as_ref()),
                &import_entry.import_name,
                &import_entry.module_request,
                import_entry.module_request_type,
            ));
        }
    }

    /// `ScopeNode::analyzeModule` e `SourceElements::analyzeModule`: só declarações de módulo entram no
    /// nível mais alto na fase de análise.
    fn analyze_statements(&mut self, node: &ModuleProgramNode) -> bool {
        let Some(statements) = node.base.statements.clone() else { return true };
        let statements: Vec<Statement> = statements.borrow().iter().collect();
        for statement in statements {
            let ok = match statement {
                Statement::ImportDeclaration(declaration) => self.analyze_import(&declaration.borrow()),
                Statement::ExportAllDeclaration(declaration) => self.analyze_export_all(&declaration.borrow()),
                Statement::ExportNamedDeclaration(declaration) => self.analyze_export_named(&declaration.borrow()),
                // `ExportDefaultDeclarationNode::analyzeModule` e `ExportLocalDeclarationNode::analyzeModule`.
                Statement::ExportDefaultDeclaration(_) | Statement::ExportLocalDeclaration(_) => true,
                // Só declarações de módulo entram no nível mais alto na fase de análise.
                _ => true,
            };
            if !ok {
                return false;
            }
        }
        true
    }

    /// `ImportDeclarationNode::analyzeModule`.
    fn analyze_import(&mut self, node: &crate::parser::nodes::ImportDeclarationNode) -> bool {
        let attributes = match try_create_attributes(self.vm, &node.attributes_list) {
            Ok(attributes) => attributes,
            Err(error) => return self.fail(error),
        };
        let phase = if node.type_ == ImportType::Deferred { ModulePhase::Defer } else { ModulePhase::Evaluation };
        let module_request_type = attributes.as_ref().map(|attributes| attributes.type_()).unwrap_or(ScriptFetchParametersType::JavaScript);
        let module_name = node.module_name.borrow().module_name.clone();
        self.append_requested_module(&module_name, attributes, phase);
        for specifier in node.specifier_list.borrow().specifiers.iter() {
            let specifier = specifier.borrow();
            let type_ = if specifier.imported_name == self.vm.property_names.star_namespace_private_name {
                ImportEntryType::Namespace
            } else {
                ImportEntryType::Single
            };
            self.module_record.add_import_entry(&ImportEntry {
                type_,
                phase,
                module_request_type,
                module_request: module_name.clone(),
                import_name: specifier.imported_name.clone(),
                local_name: specifier.local_name.clone(),
            });
        }
        true
    }

    /// `ExportAllDeclarationNode::analyzeModule`.
    fn analyze_export_all(&mut self, node: &crate::parser::nodes::ExportAllDeclarationNode) -> bool {
        let attributes = match try_create_attributes(self.vm, &node.attributes_list) {
            Ok(attributes) => attributes,
            Err(error) => return self.fail(error),
        };
        let module_request_type = attributes.as_ref().map(|attributes| attributes.type_()).unwrap_or(ScriptFetchParametersType::JavaScript);
        let module_name = node.module_name.borrow().module_name.clone();
        self.append_requested_module(&module_name, attributes, ModulePhase::Evaluation);
        self.module_record.add_star_export_entry(&module_name, module_request_type);
        true
    }

    /// `ExportNamedDeclarationNode::analyzeModule`.
    fn analyze_export_named(&mut self, node: &crate::parser::nodes::ExportNamedDeclarationNode) -> bool {
        let mut module_request_type = ScriptFetchParametersType::JavaScript;
        let module_name = node.module_name.as_ref().map(|module_name| module_name.borrow().module_name.clone());
        if let Some(module_name) = &module_name {
            let attributes = match try_create_attributes(self.vm, &node.attributes_list) {
                Ok(attributes) => attributes,
                Err(error) => return self.fail(error),
            };
            if let Some(attributes) = &attributes {
                module_request_type = attributes.type_();
            }
            self.append_requested_module(module_name, attributes, ModulePhase::Evaluation);
        }

        for specifier in node.specifier_list.borrow().specifiers.iter() {
            let Some(module_name) = &module_name else { continue };
            let specifier = specifier.borrow();
            // `export { v } from "mod"`: no local variable names are imported into the current module.
            // `export * as v from "mod"`: a namespace export uses createNamespace.
            if specifier.local_name == self.vm.property_names.star_namespace_private_name {
                self.module_record
                    .add_export_entry(&ExportEntry::create_namespace(&specifier.exported_name, module_name, module_request_type));
            } else {
                self.module_record.add_export_entry(&ExportEntry::create_indirect(
                    &specifier.exported_name,
                    &specifier.local_name,
                    module_name,
                    module_request_type,
                ));
            }
        }
        true
    }

    /// `analyze(moduleProgramNode)`: o registro, ou o `(ErrorType, mensagem)` da falha.
    pub fn analyze(mut self, node: &ModuleProgramNode) -> Result<AbstractModuleRecordRef, AnalyzeError> {
        // Traverse the module AST and collect import entries, export entries that have a FromClause, star
        // exports and aliased export names.
        if !self.analyze_statements(node) {
            return Err(self.error.take().expect("analyzeModule falhou sem mensagem"));
        }

        // Based on the collected information, categorize export entries into local, namespace, indirect
        // and star export entries.
        for (key, variable) in node.base.var_declarations.iter() {
            self.export_variable(node, key, variable);
        }
        for (key, variable) in node.base.variable_environment.lexical_variables.iter() {
            self.export_variable(node, key, variable);
        }

        self.module_record.set_has_tla(node.uses_await);
        Ok(Rc::clone(&self.module_record))
    }
}
