//! Porte de `src/vs/workbench/services/configurationResolver/common/configurationResolverUtils.ts`.
//!
//! `IJSONSchema` é um objeto dinâmico (ver `je_base::json_schema`): a função muda o esquema recebido no lugar, como o
//! original.

use je_base::JsString;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::nls::localize;
use je_platform::js::set_property;

/// O padrão que recusa as variáveis antigas (`${env.X}`, `${config.X}`, `${command.X}`).
const DEPRECATED_VARIABLES_PATTERN: &str = r"^(?!.*\$\{(env|config|command)\.)";

/// `applyDeprecatedVariableMessage`: completa o `pattern` e o `patternErrorMessage` do esquema, quando faltam.
pub fn apply_deprecated_variable_message(schema: &Dynamic) {
    if !schema.get_str("pattern").is_truthy() {
        set_property(schema, &JsString::from("pattern"), Dynamic::string(DEPRECATED_VARIABLES_PATTERN));
    }
    if !schema.get_str("patternErrorMessage").is_truthy() {
        set_property(
            schema,
            &JsString::from("patternErrorMessage"),
            Dynamic::string(&localize("deprecatedVariables", "'env.', 'config.' and 'command.' are deprecated, use 'env:', 'config:' and 'command:' instead.", &[])),
        );
    }
}
