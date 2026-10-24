//! Porte de `src/vs/workbench/contrib/policyExport/common/policyReference.ts`.
//!
//! Notas de tradução:
//! - `PolicyDto['type']` (`string | string[] | undefined`) é `Option<&PolicyTypes>` e `string | string[]` da
//!   referência é `&PolicyTypes`.
//! - `.sort()` do JavaScript ordena por unidade UTF-16 e o `sort` de `String` por byte UTF-8; os dois são
//!   ordens totais, então duas listas com os mesmos elementos ficam iguais depois de ordenadas em qualquer uma.

use je_base::arrays;

use super::policy_dto::PolicyTypes;

/// Configurações anuláveis aceitam os mesmos valores gerenciados; `null` representa uma preferência pessoal não
/// definida.
pub fn are_policy_reference_types_compatible(policy_type: Option<&PolicyTypes>, reference_type: &PolicyTypes) -> bool {
    let non_null_sorted = |types: &PolicyTypes| {
        let mut list: Vec<&String> = types.as_list().iter().filter(|single| *single != "null").collect();
        list.sort();
        list
    };
    // `[policyType]` com `policyType` ausente é `[undefined]`, que nunca iguala a lista de uma referência: `None`
    // guarda esse caso à parte.
    let Some(policy_type) = policy_type else {
        return false;
    };
    let policy_types = non_null_sorted(policy_type);
    let reference_types = non_null_sorted(reference_type);
    arrays::equals(Some(policy_types.as_slice()), Some(reference_types.as_slice()), |one, other| one == other)
}
