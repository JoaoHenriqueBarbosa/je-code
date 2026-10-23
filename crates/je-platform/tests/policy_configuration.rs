//! Porte de `src/vs/platform/configuration/test/common/policyConfiguration.test.ts` (suíte `PolicyConfiguration`), caso a caso e na mesma ordem.
//!
//! Pendente: nenhum dos 24 casos roda ainda. A suíte exercita `PolicyConfiguration` (de `configurations.ts`),
//! que depende de `IPolicyService` e `AbstractPolicyService` (`vs/platform/policy/common/policy.ts`), do
//! `FilePolicyService`, do `FileService` com o `InMemoryFileSystemProvider` (`vs/platform/files`) e do
//! `runWithFakedTimers` sobre eles; nada disso foi portado. Os casos, na ordem do original:
//!
//! - initialize: submits included and excluded policy definitions together
//! - initialize: with policies
//! - initialize: no policies
//! - initialize: with policies but not registered
//! - initialize: with object type policy
//! - initialize: with object type policy preserves __proto__ as own data
//! - initialize: with array type policy
//! - initialize: with boolean type policy as false
//! - initialize: with boolean type policy as true
//! - initialize: with object type policy ignores policy if value is not valid
//! - initialize: with object type policy ignores policy if there are duplicate keys
//! - change: when policy is added
//! - change: when policy is updated
//! - change: when policy is removed
//! - initialize: an owning policy applies to both the owner and its references
//! - initialize: a reference resolves even when its owner is not registered
//! - initialize: nullable boolean reference preserves an explicit false policy and an unset default
//! - initialize: the owner definition is authoritative; a reference only contributes the policy name
//! - change: a late-registering owner supersedes an earlier reference definition
//! - change: deregistering the owner falls back to a surviving reference definition
//! - change: an owning policy update propagates to both the owner and its references
//! - change: when policy setting is registered
//! - change: when policy setting is deregistered
//! - initialize: with internal policies
