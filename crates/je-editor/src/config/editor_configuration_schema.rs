//! Porte parcial de `src/vs/editor/common/config/editorConfigurationSchema.ts`: o `editorConfigurationBaseNode`, o nó base
//! (`id: 'editor'`) que as contribuições de configuração do editor espalham (`...editorConfigurationBaseNode`) ao registrar
//! as propriedades `editor.*`.
//!
//! Divergências:
//! - O nó do original é um objeto congelado que cada uso copia com `{ ...editorConfigurationBaseNode, properties }`. Aqui o
//!   [`editor_configuration_base_node`] devolve uma cópia nova dos dados a cada chamada, e quem registra acrescenta as
//!   propriedades com `with_properties`.
//! - O registro das propriedades `editor.*` do mesmo arquivo (`editorConfiguration`, `updateConfigurationSchema` e o
//!   esquema de `[lang]`) fica para a fatia que as portar.

use je_base::dynamic::Dynamic;
use je_base::nls::localize;
use je_platform::configuration::registry::{ConfigurationNodeData, ConfigurationScope};

/// `editorConfigurationBaseNode`.
pub fn editor_configuration_base_node() -> ConfigurationNodeData {
    ConfigurationNodeData {
        id: Some("editor".to_owned()),
        order: Some(5.0),
        type_: Some(Dynamic::string("object")),
        title: Some(localize("editorConfigurationTitle", "Editor", &[])),
        scope: Some(ConfigurationScope::LanguageOverridable),
        ..Default::default()
    }
}
