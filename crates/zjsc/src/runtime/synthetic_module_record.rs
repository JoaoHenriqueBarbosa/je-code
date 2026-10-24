//! Tradução de `runtime/SyntheticModuleRecord.{h,cpp}`: o registro de módulo que não tem código-fonte
//! ECMAScript, só uma lista de exports com valor pronto (módulos JSON e Text, e o módulo de export
//! `default`).
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê:
//!
//! - Como `JSModuleRecord`, o `SyntheticModuleRecord` é o `Rc<AbstractModuleRecord>` da base, sem o
//!   `JSModuleData` (`is_cyclic()` falso, ver `js_module_record.rs`): `link` e `evaluate` são os ramos
//!   `!cyclic` de `AbstractModuleRecord::link`/`evaluate` (`Synchronousness::Sync` e `jsUndefined()`).
//!   O `syntheticModuleRecordStructure()` do `JSGlobalObject` não existe: o registro não é um `JSObject`.
//! - `USE(BUN_JSC_ADDITIONS)`: o export preguiçoso (`lazyExportsSource`, `materializeLazyExport`, o
//!   valor vazio de `tryCreateWithExportNamesAndValues`) e `__esModule` não existem; todo export entra
//!   com valor. Os exports de um módulo sintético são sempre `ExportEntry::createLocal(nome, nome)`.

use crate::parser::source_code::SourceCode;
use crate::parser::source_provider::SourceProviderSourceType;
use crate::runtime::abstract_module_record::{AbstractModuleRecord, AbstractModuleRecordRef, ExportEntry};
use crate::runtime::enumeration_mode::{DontEnumPropertiesMode, PrivateSymbolMode, PropertyNameMode};
use crate::runtime::host_call::Thrown;
use crate::runtime::iterator_operations::get_value_property;
use crate::runtime::own_property_names::get_own_property_names;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_name_array::PropertyNameArrayBuilder;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_environment::JSModuleEnvironment;
use crate::runtime::js_module_record::{register_module_record, ModuleResult};
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_tdz_value, JSValue};
use crate::runtime::json_object::{json_parse, throw_json_error};
use crate::runtime::symbol_table::{SymbolTable, SymbolTableEntry, SymbolTableRef};
use crate::runtime::var_offset::VarOffset;

/// `SyntheticModuleRecord::create(globalObject, vm, structure, moduleKey, sourceType)`.
fn create(module_key: &Identifier, source_type: SourceProviderSourceType) -> AbstractModuleRecordRef {
    let record = AbstractModuleRecord::create(module_key.clone(), source_type);
    register_module_record(&record);
    record
}

/// Acrescenta `name` à tabela de símbolos do ambiente, no próximo `ScopeOffset`.
fn add_export_slot(symbol_table: &SymbolTableRef, name: &Identifier) {
    let mut table = symbol_table.borrow_mut();
    let offset = table.take_next_scope_offset();
    let key = name.impl_().expect("nome de export sem UniquedStringImpl");
    table.add(key, SymbolTableEntry::from_var_offset(VarOffset::from_scope_offset(offset)));
}

/// `SyntheticModuleRecord::tryCreateWithExportNamesAndValues(globalObject, moduleKey, exportNames,
/// exportValues, sourceType)`.
pub fn try_create_with_export_names_and_values(
    global_object: &JSGlobalObject,
    module_key: &Identifier,
    export_names: &[Identifier],
    export_values: &[JSValue],
    source_type: SourceProviderSourceType,
) -> ModuleResult<AbstractModuleRecordRef> {
    let vm = global_object.vm();
    debug_assert!(export_names.len() == export_values.len());

    let module_record = create(module_key, source_type);

    let export_symbol_table = SymbolTable::create(vm);
    add_export_slot(&export_symbol_table, &vm.property_names.star_namespace_private_name);
    for export_name in export_names {
        add_export_slot(&export_symbol_table, export_name);
        module_record.add_export_entry(&ExportEntry::create_local(export_name, export_name));
    }

    let module_environment = JSModuleEnvironment::create_for_global_object(
        vm,
        global_object,
        None,
        export_symbol_table,
        js_tdz_value(),
        Some(std::rc::Rc::clone(&module_record)),
    );
    module_record.set_module_environment(std::rc::Rc::clone(&module_environment));

    for (export_name, export_value) in export_names.iter().zip(export_values) {
        AbstractModuleRecord::put_binding(&module_environment, export_name, *export_value);
    }

    Ok(module_record)
}

/// `SyntheticModuleRecord::tryCreateDefaultExportSyntheticModule(globalObject, moduleKey, defaultExport,
/// sourceType)`.
pub fn try_create_default_export_synthetic_module(
    global_object: &JSGlobalObject,
    module_key: &Identifier,
    default_export: JSValue,
    source_type: SourceProviderSourceType,
) -> ModuleResult<AbstractModuleRecordRef> {
    let vm = global_object.vm();
    let export_names = [vm.property_names.default_keyword.clone()];
    try_create_with_export_names_and_values(global_object, module_key, &export_names, &[default_export], source_type)
}

/// `SyntheticModuleRecord::parseJSONModule(globalObject, moduleKey, sourceCode)`. Com `named_exports` (o Bun,
/// medido no 1.4.2), o objeto de topo exporta também cada chave própria (com `default` sempre o objeto todo), e
/// o que não é objeto (array, número, string, `null`) exporta `__esModule` verdadeiro ao lado de `default`.
pub fn parse_json_module(
    global_object: &JSGlobalObject,
    module_key: &Identifier,
    source_code: &SourceCode,
    named_exports: bool,
) -> ModuleResult<AbstractModuleRecordRef> {
    // https://tc39.es/proposal-json-modules/#sec-parse-json-module
    // `JSONParseWithException(globalObject, sourceCode.view())`: o parse estrito, com o `SyntaxError` do JSON.
    let result = match json_parse(global_object, &source_code.view(), None, false) {
        Ok(result) => result,
        Err(error) => {
            throw_json_error(global_object, error);
            return Err(Thrown::Pending);
        }
    };
    if !named_exports {
        return try_create_default_export_synthetic_module(global_object, module_key, result, SourceProviderSourceType::JSON);
    }
    let vm = global_object.vm();
    let default_name = vm.property_names.default_keyword.clone();
    let mut names = vec![default_name.clone()];
    let mut values = vec![result];
    if result.is_object() && result.as_object().type_() != crate::runtime::js_type::JSType::ArrayType {
        let mut keys = PropertyNameArrayBuilder::new(vm, PropertyNameMode::Strings, PrivateSymbolMode::Exclude);
        get_own_property_names(vm, &result.as_object(), &mut keys, DontEnumPropertiesMode::Exclude)?;
        for key in keys.iter().filter(|key| **key != default_name) {
            names.push(key.clone());
            values.push(get_value_property(global_object, result, &PropertyName::from_identifier(key))?);
        }
    } else {
        names.push(Identifier::from_span(vm, b"__esModule"));
        values.push(JSValue::Bool(true));
    }
    try_create_with_export_names_and_values(global_object, module_key, &names, &values, SourceProviderSourceType::JSON)
}

/// `SyntheticModuleRecord::createTextModule(globalObject, moduleKey, sourceCode)`.
pub fn create_text_module(
    global_object: &JSGlobalObject,
    module_key: &Identifier,
    source_code: &SourceCode,
) -> ModuleResult<AbstractModuleRecordRef> {
    // https://tc39.es/proposal-import-text/#sec-create-text-module
    let text = JSValue::from_js_string(js_string(global_object.vm(), &source_code.view()));
    try_create_default_export_synthetic_module(global_object, module_key, text, SourceProviderSourceType::Text)
}
