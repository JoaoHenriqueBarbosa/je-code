//! Porte de `src/vs/workbench/contrib/testing/common/getComputedState.ts`: o cálculo do estado (e da duração) de um
//! nó da árvore de testes a partir do estado próprio e dos filhos, e a atualização incremental dos pais.
//!
//! Divergências:
//! - **Identidade do nó.** `T extends object` é um handle barato de clonar (um `Rc`). O `WeakMap` global
//!   `largeNodeChildrenStates` (contagem por estado dos nós com mais de 64 filhos) vira um espaço no próprio nó, que o
//!   trait [`ComputedStateNode`] expõe: o slot morre com o nó, como a entrada do `WeakMap`. O `Set<T>` devolvido por
//!   `refreshComputedState` é um `Vec<T>` sem repetição por identidade, na ordem de inserção.
//! - **`getCurrentComputedState`** devolve `Option`: o original o compara com `undefined` (nós da árvore do explorador
//!   ainda sem estado), embora o tipo diga `TestResultState`.
//! - **`getChildren` e `getParents`** devolvem `Vec<T>` (o `Iterable<T>` é consumido inteiro).
//! - **`isDurationAccessor`** (`'getOwnDuration' in accessor`) é o `as_duration_accessor` do trait.
//! - Os setters do acessor recebem `&self`: o acessor guarda o estado em células, como o objeto do original.

use std::cell::RefCell;
use std::collections::HashSet;

use super::test_types::TestResultState;
use super::testing_states::{TestStateCount, make_empty_counts, max_priority, state_priority};

/// O que o cálculo lê de um nó da árvore além do acessor.
pub trait ComputedStateNode {
    /// A identidade do nó (o endereço do objeto).
    fn identity(&self) -> usize;

    /// O slot da contagem por estado dos filhos, preenchido só para os nós com mais de `LARGE_NODE_THRESHOLD` filhos.
    fn large_children_states(&self) -> &RefCell<Option<TestStateCount>>;
}

/// Accessor for nodes in get and refresh computed state.
pub trait IComputedStateAccessor<T> {
    fn get_own_state(&self, item: &T) -> Option<TestResultState>;
    fn get_current_computed_state(&self, item: &T) -> Option<TestResultState>;
    fn set_computed_state(&self, item: &T, state: TestResultState);
    fn get_children(&self, item: &T) -> Vec<T>;
    fn get_parents(&self, item: &T) -> Vec<T>;

    /// `isDurationAccessor(accessor)`.
    fn as_duration_accessor(&self) -> Option<&dyn IComputedStateAndDurationAccessor<T>> {
        None
    }
}

pub trait IComputedStateAndDurationAccessor<T>: IComputedStateAccessor<T> {
    fn get_own_duration(&self, item: &T) -> Option<f64>;
    fn get_current_computed_duration(&self, item: &T) -> Option<f64>;
    fn set_computed_duration(&self, item: &T, duration: Option<f64>);
}

const LARGE_NODE_THRESHOLD: usize = 64;

/// Gets the computed state for the node.
/// `force`: whether to refresh the computed state for this node, even
/// if it was previously set.
fn get_computed_state<T: ComputedStateNode>(accessor: &dyn IComputedStateAccessor<T>, node: &T, force: bool) -> TestResultState {
    let current = accessor.get_current_computed_state(node);
    if let Some(computed) = current
        && !force
    {
        return computed;
    }

    let mut computed = accessor.get_own_state(node).unwrap_or(TestResultState::Unset);

    let mut children_count = 0;
    let mut state_map = make_empty_counts();

    for child in accessor.get_children(node) {
        let child_computed = get_computed_state(accessor, &child, false);
        children_count += 1;
        state_map.increment(child_computed);

        // If all children are skipped, make the current state skipped too if unset (#131537)
        computed = if child_computed == TestResultState::Skipped && computed == TestResultState::Unset {
            TestResultState::Skipped
        } else {
            max_priority(&[computed, child_computed])
        };
    }

    if children_count > LARGE_NODE_THRESHOLD {
        *node.large_children_states().borrow_mut() = Some(state_map);
    }

    accessor.set_computed_state(node, computed);
    computed
}

fn get_computed_duration<T>(accessor: &dyn IComputedStateAndDurationAccessor<T>, node: &T, force: bool) -> Option<f64> {
    let current = accessor.get_current_computed_duration(node);
    if current.is_some() && !force {
        return current;
    }

    let own = accessor.get_own_duration(node);
    let computed = if own.is_some() {
        own
    } else {
        let mut computed: Option<f64> = None;
        for child in accessor.get_children(node) {
            if let Some(d) = get_computed_duration(accessor, &child, false) {
                computed = Some(computed.unwrap_or(0.0) + d);
            }
        }
        computed
    };

    accessor.set_computed_duration(node, computed);
    computed
}

/// O `Set<T>` por identidade que o `toUpdate` do original é.
struct UpdateSet<T> {
    seen: HashSet<usize>,
    items: Vec<T>,
}

impl<T: ComputedStateNode + Clone> UpdateSet<T> {
    fn new() -> Self {
        Self { seen: HashSet::new(), items: Vec::new() }
    }

    fn add(&mut self, item: &T) {
        if self.seen.insert(item.identity()) {
            self.items.push(item.clone());
        }
    }
}

/// Refreshes the computed state for the node and its parents. Any changes
/// elements cause `addUpdated` to be called. Devolve os nós alterados.
pub fn refresh_computed_state<T: ComputedStateNode + Clone>(
    accessor: &dyn IComputedStateAccessor<T>,
    node: &T,
    explicit_new_computed_state: Option<TestResultState>,
    refresh_duration: bool,
) -> Vec<T> {
    let old_state = accessor.get_current_computed_state(node);
    let old_priority = old_state.map(state_priority);
    let new_state = explicit_new_computed_state.unwrap_or_else(|| get_computed_state(accessor, node, true));
    let new_priority = state_priority(new_state);
    let mut to_update = UpdateSet::new();

    if Some(new_priority) != old_priority {
        accessor.set_computed_state(node, new_state);
        to_update.add(node);

        let mut move_from_state = old_state;
        let mut move_to_state = new_state;

        for parent in accessor.get_parents(node) {
            let has_lnm = parent.large_children_states().borrow().is_some();
            if has_lnm && let Some(lnm) = parent.large_children_states().borrow_mut().as_mut() {
                if let Some(from) = move_from_state {
                    lnm.decrement(from);
                }
                lnm.increment(move_to_state);
            }

            let prev = accessor.get_current_computed_state(&parent);
            if old_priority.is_some_and(|old| new_priority > old) {
                // Update all parents to ensure they're at least this priority.
                if prev.is_some_and(|prev| state_priority(prev) >= new_priority) {
                    break;
                }

                if has_lnm && parent.large_children_states().borrow().as_ref().is_some_and(|lnm| lnm[move_to_state] > 1) {
                    break;
                }

                // moveToState remains the same, the new higher priority node state
                accessor.set_computed_state(&parent, new_state);
                to_update.add(&parent);
            } else {
                /* newProirity < oldPriority */
                // Update all parts whose statese might have been based on this one
                if prev.is_none_or(|prev| old_priority.is_some_and(|old| state_priority(prev) > old)) {
                    break;
                }

                if has_lnm
                    && let Some(from) = move_from_state
                    && parent.large_children_states().borrow().as_ref().is_some_and(|lnm| lnm[from] > 0)
                {
                    break;
                }

                move_to_state = get_computed_state(accessor, &parent, true);
                accessor.set_computed_state(&parent, move_to_state);
                to_update.add(&parent);
            }

            move_from_state = prev;
        }
    }

    if let Some(duration_accessor) = accessor.as_duration_accessor()
        && refresh_duration
    {
        let mut chain = vec![node.clone()];
        chain.extend(accessor.get_parents(node));
        for parent in chain {
            let old_duration = duration_accessor.get_current_computed_duration(&parent);
            let new_duration = get_computed_duration(duration_accessor, &parent, true);
            if old_duration == new_duration {
                break;
            }

            duration_accessor.set_computed_duration(&parent, new_duration);
            to_update.add(&parent);
        }
    }

    to_update.items
}
