//! Porte de `bytecode/ValueProfile.h`: o que o interpretador e o `MetadataTable` usam.
//!
//! Divergências:
//!
//! - Os baldes (`m_buckets`) do C++ são um array de `EncodedJSValue` de tamanho
//!   `numberOfBuckets + numberOfSpecFailBuckets` (parâmetros do template). Aqui o array tem sempre
//!   dois lugares e o struct guarda os dois parâmetros; só os `total_number_of_buckets` primeiros
//!   valem. O `sizeof` que o C++ usa na conta da tabela (16 bytes para `ValueProfile`) fica em
//!   `crate::bytecode::op_metadata::VALUE_PROFILE_SIZE`.
//! - `computeUpdatedPrediction`, `computeUpdatedPredictionForExtraValue`, `briefDescription` e
//!   `dump` dependem de `speculationFromValueForProfiling` (`SpeculatedType.cpp`, que olha a
//!   `Structure` da célula) e do `SpeculationDump`; entram com a camada de objetos. O LLInt só
//!   escreve nos baldes (`valueProfile` do `LowLevelInterpreter.asm`), o que este módulo cobre.
//! - `ValueProfileAndVirtualRegisterBuffer` (usado só pelo `op_catch`, com o `buffer` do metadata
//!   guardando um ponteiro) vira `Vec<ValueProfileAndVirtualRegister>`; o `buffer` do metadata
//!   (`u64`) guarda o índice dele na arena de buffers do `CodeBlock` quando o `op_catch` entrar.
//! - `clearEncodedJSValueConcurrent`/`updateEncodedJSValueConcurrent` (escrita atômica relaxada)
//!   são atribuições comuns: o porte tem uma thread só.

use crate::bytecode::speculated_type::{SpeculatedType, SPEC_NONE};
use crate::bytecode::virtual_register::VirtualRegister;
use crate::runtime::js_value::{EncodedJSValue, JSValue};

/// `ValueProfileBase<numberOfBucketsArgument, numberOfSpecFailBucketsArgument>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueProfileBase {
    number_of_buckets: u8,
    number_of_spec_fail_buckets: u8,
    pub buckets: [EncodedJSValue; 2],
    pub prediction: SpeculatedType,
}

impl ValueProfileBase {
    /// `ValueProfileBase()`: `clearBuckets()` e `m_prediction { SpecNone }`.
    pub fn new(number_of_buckets: u8, number_of_spec_fail_buckets: u8) -> ValueProfileBase {
        debug_assert!(number_of_buckets as usize + number_of_spec_fail_buckets as usize <= 2);
        let mut profile = ValueProfileBase {
            number_of_buckets,
            number_of_spec_fail_buckets,
            buckets: [0; 2],
            prediction: SPEC_NONE,
        };
        profile.clear_buckets();
        profile
    }

    /// `totalNumberOfBuckets`.
    pub fn total_number_of_buckets(&self) -> usize {
        self.number_of_buckets as usize + self.number_of_spec_fail_buckets as usize
    }

    /// `specFailBucket(i)`: o índice do balde dentro de `buckets`.
    pub fn spec_fail_bucket(&self, i: usize) -> usize {
        debug_assert!(self.number_of_buckets as usize + i < self.total_number_of_buckets());
        self.number_of_buckets as usize + i
    }

    /// `clearBuckets()`: cada balde recebe `JSValue::encode(JSValue())`.
    pub fn clear_buckets(&mut self) {
        let empty = JSValue::empty().encode();
        for i in 0..self.total_number_of_buckets() {
            self.buckets[i] = empty;
        }
    }

    /// `numberOfSamples()`: baldes com um valor que não é o vazio.
    pub fn number_of_samples(&self) -> u32 {
        let mut result = 0;
        for i in 0..self.total_number_of_buckets() {
            if !JSValue::decode(self.buckets[i]).is_empty() {
                result += 1;
            }
        }
        result
    }

    /// `isSampledBefore()`.
    pub fn is_sampled_before(&self) -> bool {
        self.prediction != SPEC_NONE
    }

    /// `totalNumberOfSamples()`.
    pub fn total_number_of_samples(&self) -> u32 {
        self.number_of_samples() + self.is_sampled_before() as u32
    }
}

/// `struct ValueProfile : ValueProfileBase<1, 0>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueProfile {
    pub base: ValueProfileBase,
}

impl Default for ValueProfile {
    /// `ValueProfile()`.
    fn default() -> ValueProfile {
        ValueProfile { base: ValueProfileBase::new(1, 0) }
    }
}

/// `struct ArgumentValueProfile : ValueProfileBase<1, 1>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArgumentValueProfile {
    pub base: ValueProfileBase,
}

impl Default for ArgumentValueProfile {
    /// `ArgumentValueProfile()`.
    fn default() -> ArgumentValueProfile {
        ArgumentValueProfile { base: ValueProfileBase::new(1, 1) }
    }
}

/// `struct ValueProfileAndVirtualRegister : ValueProfile`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueProfileAndVirtualRegister {
    pub profile: ValueProfile,
    pub operand: VirtualRegister,
}

impl Default for ValueProfileAndVirtualRegister {
    fn default() -> ValueProfileAndVirtualRegister {
        ValueProfileAndVirtualRegister { profile: ValueProfile::default(), operand: VirtualRegister::default() }
    }
}
