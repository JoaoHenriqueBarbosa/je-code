//! O pedaço de `src/vs/platform/product/common/productService.ts` (`IProductService`) de que os serviços
//! portados precisam: o trait [`IProductService`] e uma implementação local ([`ProductService`]).
//!
//! O je-code lê e escreve os MESMOS diretórios do VS Code estável (retrocompatibilidade com o perfil, as
//! extensões e os temas do usuário), então o padrão é o `product.json` do VS Code estável 1.143.1:
//! `nameShort` `Code`, `dataFolderName` `.vscode` e `sharedDataFolderName` `.vscode-shared`.
//!
//! Divergência: o `IProductService` do original é o `IProductConfiguration` inteiro (uma centena de campos,
//! que a IA e o marketplace da Microsoft enchem). O trait expõe só os campos que os serviços portados leem,
//! e cresce quando um serviço novo precisar de mais um. [`je_base::product::IProductConfiguration`], o
//! `product.json` completo, também o implementa.

use je_base::product::StringsOrDictionary;
use je_instantiation::service_id;

/// `IProductService`, reduzido aos campos que os serviços portados leem.
pub trait IProductService {
    /// `version`.
    fn version(&self) -> &str;
    /// `nameShort`: o nome da pasta de dados do usuário (`~/.config/Code`).
    fn name_short(&self) -> &str;
    /// `nameLong`.
    fn name_long(&self) -> &str;
    /// `applicationName`.
    fn application_name(&self) -> &str;
    /// `urlProtocol`: o esquema dos links que abrem o aplicativo (`vscode://`).
    fn url_protocol(&self) -> &str;
    /// `dataFolderName`: a pasta no home com extensões e `argv.json` (`~/.vscode`).
    fn data_folder_name(&self) -> &str;
    /// `sharedDataFolderName`: a pasta de dados compartilhados entre aplicativos (`~/.vscode-shared`).
    fn shared_data_folder_name(&self) -> &str;
    /// `quality` (`stable`, `insider`), quando o produto declara.
    fn quality(&self) -> Option<&str>;
    /// `commit`, quando o produto declara.
    fn commit(&self) -> Option<&str>;
    /// `enableTelemetry`.
    fn enable_telemetry(&self) -> bool;
    /// `aiConfig.ariaKey`, quando o produto declara.
    fn ai_config_aria_key(&self) -> Option<&str>;
    /// `msftInternalDomains`.
    fn msft_internal_domains(&self) -> &[String];
    /// `enabledTelemetryLevels`, quando o produto declara.
    fn enabled_telemetry_levels(&self) -> Option<EnabledTelemetryLevels>;
    /// `trustedExtensionAuthAccess`, quando o produto declara. Devolve uma cópia (e não uma referência)
    /// porque o original lê a propriedade a cada chamada e um produto de teste pode mudá-la entre elas.
    fn trusted_extension_auth_access(&self) -> Option<StringsOrDictionary>;
}

/// `IEnabledTelemetryLevels`: o que o produto permite coletar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnabledTelemetryLevels {
    pub error: bool,
    pub usage: bool,
}

service_id!(pub I_PRODUCT_SERVICE: dyn IProductService = "productService");

/// Uma implementação de [`IProductService`] com os valores do `product.json` do VS Code estável 1.143.1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductService {
    pub version: String,
    pub name_short: String,
    pub name_long: String,
    pub application_name: String,
    pub url_protocol: String,
    pub data_folder_name: String,
    pub shared_data_folder_name: String,
    pub quality: Option<String>,
    pub commit: Option<String>,
    pub enable_telemetry: bool,
    pub ai_config_aria_key: Option<String>,
    pub msft_internal_domains: Vec<String>,
    pub enabled_telemetry_levels: Option<EnabledTelemetryLevels>,
    pub trusted_extension_auth_access: Option<StringsOrDictionary>,
}

impl Default for ProductService {
    fn default() -> Self {
        Self {
            version: "1.143.1".to_owned(),
            name_short: "Code".to_owned(),
            name_long: "Visual Studio Code".to_owned(),
            application_name: "code".to_owned(),
            url_protocol: "vscode".to_owned(),
            data_folder_name: ".vscode".to_owned(),
            shared_data_folder_name: ".vscode-shared".to_owned(),
            quality: Some("stable".to_owned()),
            commit: None,
            enable_telemetry: true,
            ai_config_aria_key: None,
            msft_internal_domains: Vec::new(),
            enabled_telemetry_levels: Some(EnabledTelemetryLevels { error: true, usage: true }),
            trusted_extension_auth_access: None,
        }
    }
}

impl IProductService for ProductService {
    fn version(&self) -> &str {
        &self.version
    }
    fn name_short(&self) -> &str {
        &self.name_short
    }
    fn name_long(&self) -> &str {
        &self.name_long
    }
    fn application_name(&self) -> &str {
        &self.application_name
    }
    fn url_protocol(&self) -> &str {
        &self.url_protocol
    }
    fn data_folder_name(&self) -> &str {
        &self.data_folder_name
    }
    fn shared_data_folder_name(&self) -> &str {
        &self.shared_data_folder_name
    }
    fn quality(&self) -> Option<&str> {
        self.quality.as_deref()
    }
    fn commit(&self) -> Option<&str> {
        self.commit.as_deref()
    }
    fn enable_telemetry(&self) -> bool {
        self.enable_telemetry
    }
    fn ai_config_aria_key(&self) -> Option<&str> {
        self.ai_config_aria_key.as_deref()
    }
    fn msft_internal_domains(&self) -> &[String] {
        &self.msft_internal_domains
    }
    fn enabled_telemetry_levels(&self) -> Option<EnabledTelemetryLevels> {
        self.enabled_telemetry_levels
    }
    fn trusted_extension_auth_access(&self) -> Option<StringsOrDictionary> {
        self.trusted_extension_auth_access.clone()
    }
}

impl IProductService for je_base::product::IProductConfiguration {
    fn version(&self) -> &str {
        &self.version
    }
    fn name_short(&self) -> &str {
        &self.name_short
    }
    fn name_long(&self) -> &str {
        &self.name_long
    }
    fn application_name(&self) -> &str {
        &self.application_name
    }
    fn url_protocol(&self) -> &str {
        &self.url_protocol
    }
    fn data_folder_name(&self) -> &str {
        &self.data_folder_name
    }
    fn shared_data_folder_name(&self) -> &str {
        &self.shared_data_folder_name
    }
    fn quality(&self) -> Option<&str> {
        self.quality.as_deref()
    }
    fn commit(&self) -> Option<&str> {
        self.commit.as_deref()
    }
    fn enable_telemetry(&self) -> bool {
        self.enable_telemetry.unwrap_or(false)
    }
    fn ai_config_aria_key(&self) -> Option<&str> {
        self.ai_config.as_ref().map(|config| config.aria_key.as_str())
    }
    fn msft_internal_domains(&self) -> &[String] {
        self.msft_internal_domains.as_deref().unwrap_or(&[])
    }
    fn enabled_telemetry_levels(&self) -> Option<EnabledTelemetryLevels> {
        self.enabled_telemetry_levels.as_ref().map(|levels| EnabledTelemetryLevels { error: levels.error, usage: levels.usage })
    }
    fn trusted_extension_auth_access(&self) -> Option<StringsOrDictionary> {
        self.trusted_extension_auth_access.clone()
    }
}
