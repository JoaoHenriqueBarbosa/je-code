//! Porte de `src/vs/workbench/contrib/testing/common/testingStates.ts`: as prioridades de exibição dos estados de
//! execução, os estados terminais e a contagem de testes por estado.
//!
//! Divergências:
//! - **`TestStateCount`** é um `Uint32Array` disfarçado de objeto no original (`makeEmptyCounts`); aqui é a struct
//!   [`TestStateCount`] indexada por [`TestResultState`], com `increment` e `decrement` que dão a volta como o
//!   `Uint32Array` (o `--` de uma contagem em zero vira `4294967295`, não um pânico).
//! - **`statePriority`, `terminalStatePriorities`, `stateNodes`** são funções sobre o estado (`state_priority`,
//!   `terminal_state_priority`, `state_node`).
//! - **`maxPriority(...states)`** recebe uma fatia.

use std::ops::{Index, IndexMut};

use super::test_types::TestResultState;

/// `TreeStateNode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeStateNode {
    pub status_node: bool,
    pub state: TestResultState,
    pub priority: i32,
}

/// List of display priorities for different run states. When tests update,
/// the highest-priority state from any of their children will be the state
/// reflected in the parent node.
pub fn state_priority(state: TestResultState) -> i32 {
    match state {
        TestResultState::Running => 6,
        TestResultState::Errored => 5,
        TestResultState::Failed => 4,
        TestResultState::Queued => 3,
        TestResultState::Passed => 2,
        TestResultState::Unset => 0,
        TestResultState::Skipped => 1,
    }
}

pub fn is_failed_state(s: TestResultState) -> bool {
    s == TestResultState::Errored || s == TestResultState::Failed
}

pub fn is_state_with_result(s: TestResultState) -> bool {
    s == TestResultState::Errored || s == TestResultState::Failed || s == TestResultState::Passed
}

/// `stateNodes[state]`.
pub fn state_node(state: TestResultState) -> TreeStateNode {
    TreeStateNode { status_node: true, state, priority: state_priority(state) }
}

pub fn cmp_priority(a: TestResultState, b: TestResultState) -> i32 {
    state_priority(b) - state_priority(a)
}

pub fn max_priority(states: &[TestResultState]) -> TestResultState {
    match states.len() {
        0 => TestResultState::Unset,
        1 => states[0],
        2 => {
            if state_priority(states[0]) > state_priority(states[1]) {
                states[0]
            } else {
                states[1]
            }
        }
        _ => {
            let mut max = states[0];
            for state in &states[1..] {
                if state_priority(max) < state_priority(*state) {
                    max = *state;
                }
            }

            max
        }
    }
}

/// `statesInOrder`: os estados do de maior para o de menor prioridade.
pub const STATES_IN_ORDER: [TestResultState; 7] = [
    TestResultState::Running,
    TestResultState::Errored,
    TestResultState::Failed,
    TestResultState::Queued,
    TestResultState::Passed,
    TestResultState::Skipped,
    TestResultState::Unset,
];

/// Some states are considered terminal; once these are set for a given test run, they
/// are not reset back to a non-terminal state, or to a terminal state with lower
/// priority.
pub fn terminal_state_priority(state: TestResultState) -> Option<i32> {
    match state {
        TestResultState::Passed => Some(0),
        TestResultState::Skipped => Some(1),
        TestResultState::Failed => Some(2),
        TestResultState::Errored => Some(3),
        _ => None,
    }
}

/// Count of the number of tests in each run state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TestStateCount([u32; 7]);

impl TestStateCount {
    /// `counts[state]++` (com a volta do `Uint32Array`).
    pub fn increment(&mut self, state: TestResultState) {
        let slot = &mut self.0[state as usize];
        *slot = slot.wrapping_add(1);
    }

    /// `counts[state]--` (com a volta do `Uint32Array`).
    pub fn decrement(&mut self, state: TestResultState) {
        let slot = &mut self.0[state as usize];
        *slot = slot.wrapping_sub(1);
    }
}

impl Index<TestResultState> for TestStateCount {
    type Output = u32;

    fn index(&self, state: TestResultState) -> &u32 {
        &self.0[state as usize]
    }
}

impl IndexMut<TestResultState> for TestStateCount {
    fn index_mut(&mut self, state: TestResultState) -> &mut u32 {
        &mut self.0[state as usize]
    }
}

pub fn make_empty_counts() -> TestStateCount {
    // shh! don't tell anyone this is actually an array!
    TestStateCount::default()
}
