//! Porte de `src/vs/workbench/contrib/policyExport/common/policyDto.ts`: o formato do JSON que a exportação de
//! políticas gera e que o site de documentação lê.
//!
//! Notas de tradução:
//! - `type?: string | string[]` é [`PolicyTypes`] dentro de um `Option`.
//! - `default?: unknown` é um [`serde_json::Value`] dentro de um `Option` (ausente é `None`).
//! - `minimumVersion: `${number}.${number}`` é um `String` (o template literal só restringe o texto no tipo).
//! - As palavras reservadas `type`, `default` e `enum` viram `policy_type`, `default_value` e `enum_values`.
//! - O `status` de `agentHost` é o enum [`AgentHostStatus`], com os mesmos quatro textos do original.

use serde_json::Value;

/// `LocalizedValueDto`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalizedValueDto {
    pub key: String,
    pub value: String,
}

/// `CategoryDto`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategoryDto {
    pub key: String,
    pub name: LocalizedValueDto,
}

/// `string | string[]`: o tipo de uma política (`PolicyDto['type']`) ou de uma referência.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyTypes {
    One(String),
    Many(Vec<String>),
}

impl PolicyTypes {
    /// Os tipos como lista: `Array.isArray(types) ? types : [types]`.
    pub fn as_list(&self) -> &[String] {
        match self {
            Self::One(single) => std::slice::from_ref(single),
            Self::Many(list) => list,
        }
    }
}

impl From<&str> for PolicyTypes {
    fn from(single: &str) -> Self {
        Self::One(single.to_owned())
    }
}

impl<const N: usize> From<[&str; N]> for PolicyTypes {
    fn from(list: [&str; N]) -> Self {
        Self::Many(list.iter().map(|single| (*single).to_owned()).collect())
    }
}

/// O `localization` de `PolicyDto`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyLocalizationDto {
    pub description: LocalizedValueDto,
    pub enum_descriptions: Option<Vec<LocalizedValueDto>>,
}

/// O `status` de `agentHost`: quão completamente o Agent Host aplica a política.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentHostStatus {
    Enforced,
    Partial,
    NotEnforced,
    NotApplicable,
}

impl AgentHostStatus {
    /// O texto do original (`'enforced' | 'partial' | 'notEnforced' | 'notApplicable'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enforced => "enforced",
            Self::Partial => "partial",
            Self::NotEnforced => "notEnforced",
            Self::NotApplicable => "notApplicable",
        }
    }
}

/// O `agentHost` de `PolicyDto`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentHostDto {
    pub status: AgentHostStatus,
}

/// `PolicyDto`.
#[derive(Clone, Debug, PartialEq)]
pub struct PolicyDto {
    pub key: String,
    pub name: String,
    pub category: String,
    pub minimum_version: String,
    pub localization: PolicyLocalizationDto,
    pub policy_type: Option<PolicyTypes>,
    pub default_value: Option<Value>,
    pub enum_values: Option<Vec<String>>,

    /// Outras configurações governadas pela mesma política via `policyReference` (além da configuração dona em
    /// `key`). Ordenadas para a saída ser estável; ausente quando a política governa só a configuração dona.
    pub referenced_settings: Option<Vec<String>>,

    /// Com `false`, a configuração não é ajustável pelo usuário e só se gerencia por política. Vale `true` quando
    /// ausente.
    pub included: Option<bool>,

    /// Quão completamente o Agent Host aplica esta política.
    pub agent_host: Option<AgentHostDto>,
}

/// `ExportedPolicyDataDto`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportedPolicyDataDto {
    pub categories: Vec<CategoryDto>,
    pub policies: Vec<PolicyDto>,
}
