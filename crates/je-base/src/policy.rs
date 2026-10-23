//! Porte de `src/vs/base/common/policy.ts`: os tipos das políticas de empresa (`IPolicy`) e o caminho do
//! arquivo de política do Linux.

use std::fmt;
use std::rc::Rc;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::default_account::IPolicyData;
use crate::nls::localize;

/// System-wide policy file path for Linux systems.
pub const LINUX_SYSTEM_POLICY_FILE_PATH: &str = "/etc/vscode/policy.json";

pub type PolicyName = String;

/// `PolicyValue`: `string | number | boolean`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PolicyValue {
    Boolean(bool),
    Number(f64),
    String(String),
}

pub type ManagedSettingValue = PolicyValue;
/// `ManagedSettingsData`: `Readonly<Record<string, ManagedSettingValue>>`.
pub type ManagedSettingsData = IndexMap<String, ManagedSettingValue>;

/// `IManagedSettingPolicyDefinition['type']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ManagedSettingType {
    String,
    Number,
    Boolean,
}

record! {
    /// `IManagedSettingPolicyDefinition`.
    pub struct IManagedSettingPolicyDefinition {
        req { r#type: ManagedSettingType }
    }
}

pub type IManagedSettingsPolicyDefinitions = IndexMap<String, IManagedSettingPolicyDefinition>;

/// `LocalizedValue`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalizedValue {
    pub key: String,
    pub value: String,
}

/// `PolicyCategory`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PolicyCategory {
    Extensions,
    IntegratedTerminal,
    InteractiveSession,
    Telemetry,
    Update,
}

impl PolicyCategory {
    /// `PolicyCategoryData[category].name`.
    pub fn name(self) -> LocalizedValue {
        let (key, message) = match self {
            PolicyCategory::Extensions => ("extensionsConfigurationTitle", "Extensions"),
            PolicyCategory::IntegratedTerminal => ("terminalIntegratedConfigurationTitle", "Integrated Terminal"),
            PolicyCategory::InteractiveSession => ("interactiveSessionConfigurationTitle", "Chat"),
            PolicyCategory::Telemetry => ("telemetryConfigurationTitle", "Telemetry"),
            PolicyCategory::Update => ("updateConfigurationTitle", "Update"),
        };
        LocalizedValue { key: key.to_owned(), value: localize(key, message, &[]) }
    }
}

/// `(policyData: IPolicyData) => string | number | boolean | undefined`.
type PolicyValueCallback = Rc<dyn Fn(&IPolicyData) -> Option<PolicyValue>>;

/// A função `value` de uma política: o valor que uma funcionalidade ligada à conta recebe quando a política
/// vale (`None` é o `undefined`: a configuração não fica travada).
#[derive(Clone, Default)]
pub struct PolicyValueFn(pub Option<PolicyValueCallback>);

impl PolicyValueFn {
    pub fn new(f: impl Fn(&IPolicyData) -> Option<PolicyValue> + 'static) -> Self {
        Self(Some(Rc::new(f)))
    }

    pub fn call(&self, policy_data: &IPolicyData) -> Option<PolicyValue> {
        self.0.as_ref().and_then(|f| f(policy_data))
    }
}

impl fmt::Debug for PolicyValueFn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_some() { "PolicyValueFn(..)" } else { "PolicyValueFn(None)" })
    }
}

impl PartialEq for PolicyValueFn {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

record! {
    /// `IPolicy.localization`.
    pub struct IPolicyLocalization {
        req { description: LocalizedValue }
        opt { enum_descriptions: Vec<LocalizedValue> }
    }
}

record! {
    /// `IPolicy`.
    pub struct IPolicy {
        req {
            /// The policy name.
            name: PolicyName,
            /// The policy category.
            category: PolicyCategory,
            /// The Code version in which this policy was introduced (`${number}.${number}`).
            minimum_version: String,
            /// Localization info for the policy.
            localization: IPolicyLocalization,
            /// The value that an ACCOUNT-based feature will use when its corresponding policy is active. Fica
            /// de fora do JSON: uma função não viaja em `product.json`.
            #[serde(skip)]
            value: PolicyValueFn,
        }
        opt {
            /// Copilot managed-settings keys this policy's value callback reads.
            managed_settings: IManagedSettingsPolicyDefinitions,
            /// The most-restrictive value that should be applied when the "Require Approved Account" gate is
            /// not yet satisfied.
            restricted_value: PolicyValue,
        }
    }
}

record! {
    /// Uma política como o `product.json` a declara (`extensionConfigurationPolicy`): sem `localization`
    /// (o texto vem de `description`).
    pub struct IProductPolicy {
        req { name: PolicyName, category: PolicyCategory, minimum_version: String }
        opt { description: String, localization: IPolicyLocalization, managed_settings: IManagedSettingsPolicyDefinitions, restricted_value: PolicyValue }
    }
}

record! {
    /// `IPolicyReference`: um ponteiro para uma política de outra configuração.
    pub struct IPolicyReference {
        req { name: PolicyName }
    }
}

record! {
    /// `IExtensionConfigurationPolicyReference`.
    pub struct IExtensionConfigurationPolicyReference {
        req { policy_reference: IPolicyReference }
    }
}

/// `IPolicy | IExtensionConfigurationPolicyReference`, a forma de uma entrada de
/// `extensionConfigurationPolicy` do `product.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExtensionConfigurationPolicy {
    Reference(IExtensionConfigurationPolicyReference),
    Policy(IPolicy),
    /// A forma que o `product.json` do VS Code usa de fato: o texto em `description`, sem `localization`.
    Declared(IProductPolicy),
}
