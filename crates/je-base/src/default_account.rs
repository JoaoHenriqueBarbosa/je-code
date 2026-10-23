//! Porte de `src/vs/base/common/defaultAccount.ts`: os dados da conta padrão (Copilot) e das políticas
//! delivered por ela.
//!
//! `entitlementsData?: IEntitlementsData | null` é `Option<IEntitlementsData>`: ausente e `null` viram o
//! mesmo `None`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::policy::ManagedSettingsData;
use crate::record::JsNumber;

record! {
    /// `IQuotaSnapshotData`.
    pub struct IQuotaSnapshotData {
        req {
            #[serde(rename = "overage_count")]
            overage_count: JsNumber,
            #[serde(rename = "overage_entitlement")]
            overage_entitlement: JsNumber,
            #[serde(rename = "overage_permitted")]
            overage_permitted: bool,
            #[serde(rename = "percent_remaining")]
            percent_remaining: JsNumber,
            unlimited: bool,
        }
        opt {
            #[serde(rename = "credits_used")]
            credits_used: JsNumber,
            #[serde(rename = "quota_reset_at")]
            quota_reset_at: JsNumber,
            #[serde(rename = "token_based_billing")]
            token_based_billing: bool,
            entitlement: String,
            #[serde(rename = "quota_remaining")]
            quota_remaining: JsNumber,
            #[serde(rename = "has_quota")]
            has_quota: bool,
        }
    }
}

record! {
    /// `{ chat: number; completions: number }`, os limites do plano gratuito.
    pub struct IQuotaCounts {
        req { chat: JsNumber, completions: JsNumber }
    }
}

record! {
    /// `IEntitlementsData['quota_snapshots']`.
    pub struct IQuotaSnapshots {
        opt {
            chat: IQuotaSnapshotData,
            completions: IQuotaSnapshotData,
            #[serde(rename = "premium_interactions")]
            premium_interactions: IQuotaSnapshotData,
        }
    }
}

record! {
    /// `IEntitlementsData` (com o `ILegacyQuotaSnapshotData` que ele estende).
    pub struct IEntitlementsData {
        req {
            #[serde(rename = "access_type_sku")]
            access_type_sku: String,
            #[serde(rename = "chat_enabled")]
            chat_enabled: bool,
            #[serde(rename = "assigned_date")]
            assigned_date: String,
            #[serde(rename = "can_signup_for_limited")]
            can_signup_for_limited: bool,
            #[serde(rename = "copilot_plan")]
            copilot_plan: String,
            #[serde(rename = "organization_login_list")]
            organization_login_list: Vec<String>,
            #[serde(rename = "analytics_tracking_id")]
            analytics_tracking_id: String,
        }
        opt {
            #[serde(rename = "limited_user_quotas")]
            limited_user_quotas: IQuotaCounts,
            #[serde(rename = "monthly_quotas")]
            monthly_quotas: IQuotaCounts,
            #[serde(rename = "is_staff")]
            is_staff: bool,
            /// for Copilot Free
            #[serde(rename = "limited_user_reset_date")]
            limited_user_reset_date: String,
            /// for all other Copilot SKUs
            #[serde(rename = "quota_reset_date")]
            quota_reset_date: String,
            /// for all other Copilot SKUs (includes time)
            #[serde(rename = "quota_reset_date_utc")]
            quota_reset_date_utc: String,
            #[serde(rename = "token_based_billing")]
            token_based_billing: bool,
            #[serde(rename = "can_upgrade_plan")]
            can_upgrade_plan: bool,
            #[serde(rename = "cloud_session_storage_enabled")]
            cloud_session_storage_enabled: bool,
            #[serde(rename = "quota_snapshots")]
            quota_snapshots: IQuotaSnapshots,
            endpoints: IndexMap<String, String>,
        }
    }
}

/// `IPolicyData['mcpAccess']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum McpAccess {
    #[serde(rename = "allow_all")]
    AllowAll,
    #[serde(rename = "registry_only")]
    RegistryOnly,
}

record! {
    /// `IPolicyData`.
    pub struct IPolicyData {
        opt {
            mcp: bool,
            #[serde(rename = "chat_preview_features_enabled")]
            chat_preview_features_enabled: bool,
            #[serde(rename = "chat_agent_enabled")]
            chat_agent_enabled: bool,
            #[serde(rename = "cloud_session_storage_enabled")]
            cloud_session_storage_enabled: bool,
            mcp_registry_url: String,
            mcp_access: McpAccess,
            /// Normalized enterprise-managed settings, keyed by dot-separated managed-settings paths such as
            /// `permissions.disableBypassPermissionsMode`. This is the single channel for enterprise-managed
            /// configuration: server-delivered settings and native MDM settings both project into this bag.
            managed_settings: ManagedSettingsData,
            /// Whether at least one managed-settings delivery channel currently contributes governance.
            managed_settings_active: bool,
        }
    }
}

record! {
    /// `ICopilotTokenInfo`.
    pub struct ICopilotTokenInfo {
        opt { sn: String, fcv1: String }
    }
}

record! {
    /// `IDefaultAccountAuthenticationProvider`.
    pub struct IDefaultAccountAuthenticationProvider {
        req { id: String, name: String, enterprise: bool }
    }
}

record! {
    /// `IDefaultAccount`.
    pub struct IDefaultAccount {
        req {
            authentication_provider: IDefaultAccountAuthenticationProvider,
            account_name: String,
            session_id: String,
            enterprise: bool,
        }
        opt { entitlements_data: IEntitlementsData }
    }
}
