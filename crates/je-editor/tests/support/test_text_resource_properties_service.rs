//! Porte de `src/vs/editor/test/common/services/testTextResourcePropertiesService.ts`: o
//! `TestTextResourcePropertiesService`, que lê o fim de linha de `files.eol` da configuração do recurso.
//!
//! `isLinux || isMacintosh` é o `Platform::detect()` do processo.

use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::platform::Platform;
use je_base::uri::URI;
use je_editor::text_resource_configuration_service::ITextResourcePropertiesService;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;

pub struct TestTextResourcePropertiesService {
    configuration_service: Rc<dyn IConfigurationService>,
}

impl TestTextResourcePropertiesService {
    pub fn new(configuration_service: Rc<dyn IConfigurationService>) -> Rc<Self> {
        Rc::new(Self { configuration_service })
    }
}

impl ITextResourcePropertiesService for TestTextResourcePropertiesService {
    fn get_eol(&self, resource: &URI, language: Option<&str>) -> String {
        let overrides = ConfigurationOverrides { resource: Some(resource.clone()), override_identifier: language.map(JsString::from) };
        if let Dynamic::String(eol) = self.configuration_service.get_value(Some("files.eol"), Some(&overrides)) {
            let eol = eol.to_string_lossy();
            if !eol.is_empty() && eol != "auto" {
                return eol;
            }
        }
        let platform = Platform::detect();
        (if platform.is_linux || platform.is_macintosh { "\n" } else { "\r\n" }).to_string()
    }
}
