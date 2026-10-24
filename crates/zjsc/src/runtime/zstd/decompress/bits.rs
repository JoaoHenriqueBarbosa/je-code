//! Leitores de bits do descompressor: o `BIT_DStream_t` (lido de trás para frente, fecha com a marca
//! de fim) e o leitor direto LSB primeiro que o `FSE_readNCount` usa.

use super::super::params::highbit32;
use super::Error;

/// `BIT_DStream_t`: o último byte tem um bit 1 de marca; os bits de dados são os que ficam abaixo dela
/// e são consumidos do mais significativo para o menos. Ler além do começo devolve zeros e deixa
/// `remaining` negativo, que é o `BIT_DStream_overflow` do C.
pub struct RevBits<'a> {
    data: &'a [u8],
    /// Bits de dados ainda não consumidos (negativo quando passou do começo).
    remaining: i64,
}

impl<'a> RevBits<'a> {
    /// `BIT_initDStream`: falha com fonte vazia ou último byte zero.
    pub fn new(data: &'a [u8]) -> Result<Self, Error> {
        let last = *data.last().ok_or(Error::Corruption)?;
        if last == 0 {
            return Err(Error::Corruption);
        }
        let marker_bit = highbit32(last as u32) as i64;
        Ok(RevBits { data, remaining: (data.len() as i64 - 1) * 8 + marker_bit })
    }

    /// Os próximos `n` bits (n <= 32) sem consumir; o que cai antes do começo vale zero.
    pub fn peek(&self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let start = self.remaining - n as i64;
        let from = start.max(0);
        let first_byte = (from / 8) as usize;
        let last_byte = ((self.remaining - 1).max(0) / 8) as usize;
        let mut window: u64 = 0;
        for (i, idx) in (first_byte..=last_byte).enumerate() {
            let byte = if self.remaining > 0 { self.data.get(idx).copied().unwrap_or(0) } else { 0 };
            window |= (byte as u64) << (8 * i);
        }
        let mut value = if self.remaining > 0 { window >> (from % 8) } else { 0 };
        if start < 0 {
            value <<= -start;
        }
        (value & ((1u64 << n) - 1)) as u32
    }

    pub fn skip(&mut self, n: u32) {
        self.remaining -= n as i64;
    }

    pub fn read(&mut self, n: u32) -> u32 {
        let v = self.peek(n);
        self.skip(n);
        v
    }

    /// `BIT_DStream_overflow`: consumiu mais bits do que a fonte tinha.
    pub fn overflowed(&self) -> bool {
        self.remaining < 0
    }

    /// `BIT_endOfDStream`: consumiu exatamente todos os bits.
    pub fn at_end(&self) -> bool {
        self.remaining == 0
    }
}

/// Leitor LSB primeiro sobre bytes, para o cabeçalho de contagens normalizadas.
pub struct FwdBits<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl<'a> FwdBits<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        FwdBits { data, bit_pos: 0 }
    }

    /// `n` <= 24 bits; além do fim da fonte lê zeros (o chamador confere `bytes_used` no fim).
    pub fn peek(&self, n: u32) -> u32 {
        let byte = self.bit_pos / 8;
        let mut window: u64 = 0;
        for i in 0..5 {
            window |= (self.data.get(byte + i).copied().unwrap_or(0) as u64) << (8 * i);
        }
        ((window >> (self.bit_pos % 8)) & ((1u64 << n) - 1)) as u32
    }

    pub fn skip(&mut self, n: u32) {
        self.bit_pos += n as usize;
    }

    pub fn bytes_used(&self) -> usize {
        self.bit_pos.div_ceil(8)
    }
}
