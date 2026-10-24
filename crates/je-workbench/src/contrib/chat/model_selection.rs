//! Porte parcial de `src/vs/workbench/contrib/chat/common/modelSelection.ts`: só o `getDefaultModelPolicyDescription`, o
//! texto que a interface mostra quando uma política da organização define o modelo padrão do chat. O resto do arquivo
//! (`resolveModelIdentifier`, `IModelVendorResolution`...) depende do catálogo de modelos de linguagem (o chat está fora
//! do escopo deste porte, ver `workbench_test_services.rs`) e entra com ele.
//!
//! Divergências:
//! - **`(policyValue: unknown, configuredValue: unknown)`** são [`Dynamic`]; `String(policyValue)` é o `to_js_string` da
//!   plataforma. [`default_model_policy_description_function`] é a mesma função como função dinâmica, o formato em que ela
//!   entra no esquema da propriedade (`policyValueDescription: getDefaultModelPolicyDescription`).

use je_base::dynamic::{Dynamic, FunctionRef};
use je_base::js_string::trim;
use je_base::nls::localize;
use je_platform::js::to_js_string;

/// `getDefaultModelPolicyDescription`.
pub fn get_default_model_policy_description(policy_value: &Dynamic, configured_value: &Dynamic) -> String {
    let policy_text = to_js_string(policy_value);
    let trimmed = |text: &je_base::JsString| je_base::JsString::from(trim(text.units()));
    let policy_trimmed = trimmed(&policy_text);
    let overrides_policy = match configured_value {
        Dynamic::String(configured) => {
            let configured_trimmed = trimmed(configured);
            !configured_trimmed.is_empty() && configured_trimmed.to_lower_case().units() != policy_trimmed.to_lower_case().units()
        }
        _ => false,
    };
    if overrides_policy {
        let configured_text = to_js_string(configured_value).to_string_lossy();
        localize(
            "chat.defaultModel.policyOverrideDescription",
            "Your organization sets \"{0}\" as the default for new conversations. Your saved preference \"{1}\" is preserved but does not apply while this policy is in effect. You can choose another model for each conversation.",
            &[&policy_text.to_string_lossy(), &configured_text],
        )
    } else {
        localize(
            "chat.defaultModel.policyValueDescription",
            "Your organization sets \"{0}\" as the default for new conversations. You can choose another model for each conversation.",
            &[&policy_text.to_string_lossy()],
        )
    }
}

/// `getDefaultModelPolicyDescription` como a função dinâmica de duas entradas que o esquema da propriedade guarda.
pub fn default_model_policy_description_function() -> FunctionRef {
    FunctionRef::native("getDefaultModelPolicyDescription", 2, |args| {
        let argument = |index: usize| args.get(index).cloned().unwrap_or(Dynamic::Undefined);
        Dynamic::string(&get_default_model_policy_description(&argument(0), &argument(1)))
    })
}
