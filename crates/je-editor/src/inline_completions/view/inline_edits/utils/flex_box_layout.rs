//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/utils/flexBoxLayout.ts`:
//! `distributeFlexBoxLayout`, que reparte um tamanho total entre partes com regras de crescimento.
//!
//! Notas de tradução:
//! - `Record<string, IFlexBoxPartGrowthRule | IFlexBoxPartGrowthRule[]>` é uma fatia de pares (chave, [`FlexBoxPart`]),
//!   que guarda a ordem de inserção das chaves como o objeto do JavaScript; o `Record<keyof T, number>` do resultado é um
//!   `HashMap` da mesma chave. A chave é genérica (`K`): texto, ou um enum de quem chama;
//! - os campos opcionais do original são `Option`; `IFlexBoxPartGrowthRule extends IFlexBoxPartExtensionRule` é a
//!   struct com os campos de uma e da outra (`min`, `rules`, `max`, `priority`, `share`);
//! - `Math.min` do JavaScript propaga `NaN` (o `f64::min` do Rust o descarta): [`js_min`] reproduz o comportamento, que
//!   aparece quando todas as partes candidatas têm `share` zero.

use std::collections::HashMap;
use std::hash::Hash;

/// `IFlexBoxPartExtensionRule`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlexBoxPartExtensionRule {
    pub max: Option<f64>,
    pub priority: Option<f64>,
    pub share: Option<f64>,
}

/// `IFlexBoxPartGrowthRule`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlexBoxPartGrowthRule {
    pub min: Option<f64>,
    pub rules: Option<Vec<FlexBoxPartExtensionRule>>,
    pub max: Option<f64>,
    pub priority: Option<f64>,
    pub share: Option<f64>,
}

/// `IFlexBoxPartGrowthRule | IFlexBoxPartGrowthRule[]` (a lista são só as regras de extensão, com mínimo zero).
#[derive(Clone, Debug, PartialEq)]
pub enum FlexBoxPart {
    Growth(FlexBoxPartGrowthRule),
    Rules(Vec<FlexBoxPartExtensionRule>),
}

/// `Math.min(a, b)` do JavaScript: `NaN` se algum dos dois for `NaN`.
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

/// A parte normalizada: o mínimo e a lista de regras.
struct NormalizedPart {
    min: f64,
    rules: Vec<FlexBoxPartExtensionRule>,
}

/// Um candidato a crescer na rodada.
struct Candidate {
    part_index: usize,
    rule: FlexBoxPartExtensionRule,
    priority: f64,
    share: f64,
}

/// Distributes a total size into parts that each have a list of growth rules.
/// Returns `None` if the layout is not possible.
/// The sum of all returned sizes will be equal to `total_size`.
///
/// First, each part gets its minimum size.
/// Then, remaining space is distributed to the rules with the highest priority, as long as the max constraint allows it (considering share).
/// This continues with next lower priority rules until no space is left.
pub fn distribute_flex_box_layout<K: Clone + Eq + Hash>(total_size: f64, parts: &[(K, FlexBoxPart)]) -> Option<HashMap<K, f64>> {
    // Normalize parts to always have array of rules
    let normalized_parts: Vec<NormalizedPart> = parts
        .iter()
        .map(|(_, part)| match part {
            FlexBoxPart::Rules(rules) => NormalizedPart { min: 0.0, rules: rules.clone() },
            FlexBoxPart::Growth(part) => NormalizedPart {
                min: part.min.unwrap_or(0.0),
                rules: part
                    .rules
                    .clone()
                    .unwrap_or_else(|| vec![FlexBoxPartExtensionRule { max: part.max, priority: part.priority, share: part.share }]),
            },
        })
        .collect();

    // Initialize result with minimum sizes
    let mut result: Vec<f64> = normalized_parts.iter().map(|part| part.min).collect();
    let used_size: f64 = result.iter().sum();

    // Check if we can satisfy minimum constraints
    if used_size > total_size {
        return None;
    }

    let mut remaining_size = total_size - used_size;

    // Distribute remaining space by priority levels
    while remaining_size > 0.0 {
        // Find all rules at current highest priority that can still grow
        let mut candidate_rules: Vec<Candidate> = Vec::new();

        for (part_index, part) in normalized_parts.iter().enumerate() {
            for rule in &part.rules {
                let current_usage = result[part_index];
                let max_size = rule.max.unwrap_or(f64::INFINITY);

                if current_usage < max_size {
                    candidate_rules.push(Candidate {
                        part_index,
                        rule: *rule,
                        priority: rule.priority.unwrap_or(0.0),
                        share: rule.share.unwrap_or(1.0),
                    });
                }
            }
        }

        if candidate_rules.is_empty() {
            // No rules can grow anymore, but we have remaining space
            break;
        }

        // Find the highest priority among candidates
        let max_priority = candidate_rules.iter().map(|c| c.priority).fold(f64::NEG_INFINITY, f64::max);
        let highest_priority_candidates: Vec<&Candidate> = candidate_rules.iter().filter(|c| c.priority == max_priority).collect();

        // Calculate total share
        let total_share: f64 = highest_priority_candidates.iter().map(|c| c.share).sum();

        // Distribute space proportionally by share
        let mut distributed_this_round = 0.0;
        let mut distributions: Vec<(usize, f64)> = Vec::new();

        for candidate in &highest_priority_candidates {
            let current_usage = result[candidate.part_index];
            let max_size = candidate.rule.max.unwrap_or(f64::INFINITY);
            let available_for_this_rule = max_size - current_usage;

            // Calculate ideal share
            let ideal_share = (remaining_size * candidate.share) / total_share;
            let actual_amount = js_min(ideal_share, available_for_this_rule);

            distributions.push((candidate.part_index, actual_amount));

            distributed_this_round += actual_amount;
        }

        if distributed_this_round == 0.0 {
            // No progress can be made
            break;
        }

        // Apply distributions
        for (part_index, amount) in distributions {
            result[part_index] += amount;
        }

        remaining_size -= distributed_this_round;

        // Break if remaining is negligible (floating point precision)
        if remaining_size < 0.0001 {
            break;
        }
    }

    Some(parts.iter().map(|(key, _)| key.clone()).zip(result).collect())
}
