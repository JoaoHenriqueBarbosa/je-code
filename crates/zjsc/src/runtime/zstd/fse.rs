//! FSE do compressor zstd: porte de `lib/compress/fse_compress.c`, `lib/compress/hist.c` e do lado
//! escritor de `lib/common/bitstream.h` (libzstd 1.5.7).
//!
//! Conteúdo: histograma (`HIST_count`), `FSE_optimalTableLog`, `FSE_normalizeCount` (com
//! `FSE_normalizeM2`), `FSE_writeNCount`, `FSE_buildCTable`, o `BIT_CStream` e o estado de codificação
//! (`FSE_initCState`, `FSE_initCState2`, `FSE_encodeSymbol`, `FSE_flushCState`).

use super::params::highbit32;

/// `FSE_MIN_TABLELOG`.
pub const FSE_MIN_TABLELOG: u32 = 5;
/// `FSE_MAX_TABLELOG` (`FSE_MAX_MEMORY_USAGE` 14 menos 2).
pub const FSE_MAX_TABLELOG: u32 = 12;
/// `FSE_DEFAULT_TABLELOG` (`FSE_DEFAULT_MEMORY_USAGE` 13 menos 2).
pub const FSE_DEFAULT_TABLELOG: u32 = 11;
/// `FSE_NCOUNTBOUND`.
pub const FSE_NCOUNTBOUND: usize = 512;

/// Os códigos de erro do libzstd que as funções desta fatia podem produzir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FseError {
    Generic,
    TableLogTooLarge,
    DstSizeTooSmall,
    MaxSymbolValueTooSmall,
}

pub type FseResult<T> = Result<T, FseError>;

// ---------------------------------------------------------------------------------------------
// hist.c
// ---------------------------------------------------------------------------------------------

/// `HIST_count`: zera `count[0..=max_symbol_value]`, conta os bytes de `src` e devolve
/// `(maior frequência, maior símbolo presente)`. A contagem em quatro tabelas do C só muda a
/// velocidade, o resultado é o mesmo. Símbolo acima de `max_symbol_value` dá `MaxSymbolValueTooSmall`.
pub fn hist_count(count: &mut [u32], max_symbol_value: u32, src: &[u8]) -> FseResult<(u32, u32)> {
    let limit = max_symbol_value as usize;
    if limit > 255 || count.len() <= limit {
        return Err(FseError::Generic);
    }
    count[..=limit].fill(0);
    if src.is_empty() {
        return Ok((0, 0));
    }
    let mut local = [0u32; 256];
    for &b in src {
        local[b as usize] += 1;
    }
    let max_found = local.iter().rposition(|&c| c != 0).unwrap_or(0);
    if max_found > limit {
        return Err(FseError::MaxSymbolValueTooSmall);
    }
    count[..=limit].copy_from_slice(&local[..=limit]);
    let largest = local.iter().copied().max().unwrap_or(0);
    Ok((largest, max_found as u32))
}

// ---------------------------------------------------------------------------------------------
// fse_compress.c: escolha e normalização da tabela
// ---------------------------------------------------------------------------------------------

/// `FSE_minTableLog`. O C exige `srcSize > 1` e `maxSymbolValue > 0` (o `highbit32(0)` é indefinido).
fn min_table_log(src_size: usize, max_symbol_value: u32) -> FseResult<u32> {
    if src_size <= 1 || max_symbol_value == 0 {
        return Err(FseError::Generic);
    }
    let min_bits_src = highbit32(src_size as u32) + 1;
    let min_bits_symbols = highbit32(max_symbol_value) + 2;
    Ok(min_bits_src.min(min_bits_symbols))
}

/// `FSE_optimalTableLog_internal`.
pub fn optimal_table_log_internal(max_table_log: u32, src_size: usize, max_symbol_value: u32, minus: u32) -> FseResult<u32> {
    let min_bits = min_table_log(src_size, max_symbol_value)?;
    let max_bits_src = highbit32((src_size - 1) as u32).wrapping_sub(minus);
    let mut table_log = if max_table_log == 0 { FSE_DEFAULT_TABLELOG } else { max_table_log };
    if max_bits_src < table_log {
        table_log = max_bits_src;
    }
    if min_bits > table_log {
        table_log = min_bits;
    }
    Ok(table_log.clamp(FSE_MIN_TABLELOG, FSE_MAX_TABLELOG))
}

/// `FSE_optimalTableLog`.
pub fn optimal_table_log(max_table_log: u32, src_size: usize, max_symbol_value: u32) -> FseResult<u32> {
    optimal_table_log_internal(max_table_log, src_size, max_symbol_value, 2)
}

/// `FSE_normalizeM2`: método secundário, usado quando o primário erra a soma por muito.
fn normalize_m2(norm: &mut [i16], table_log: u32, count: &[u32], total: usize, max_symbol_value: u32, low_prob_count: i16) -> FseResult<()> {
    const NOT_YET_ASSIGNED: i16 = -2;
    let last = max_symbol_value as usize;
    let mut total = total;
    let mut distributed: u32 = 0;
    let low_threshold = (total >> table_log) as u32;
    let mut low_one = ((total * 3) >> (table_log + 1)) as u32;

    for s in 0..=last {
        if count[s] == 0 {
            norm[s] = 0;
        } else if count[s] <= low_threshold {
            norm[s] = low_prob_count;
            distributed += 1;
            total -= count[s] as usize;
        } else if count[s] <= low_one {
            norm[s] = 1;
            distributed += 1;
            total -= count[s] as usize;
        } else {
            norm[s] = NOT_YET_ASSIGNED;
        }
    }
    let mut to_distribute = (1u32 << table_log).wrapping_sub(distributed);
    if to_distribute == 0 {
        return Ok(());
    }

    if (total / to_distribute as usize) as u64 > u64::from(low_one) {
        // risco de arredondar para zero
        low_one = ((total * 3) / (to_distribute.wrapping_mul(2) as usize)) as u32;
        for s in 0..=last {
            if norm[s] == NOT_YET_ASSIGNED && count[s] <= low_one {
                norm[s] = 1;
                distributed += 1;
                total -= count[s] as usize;
            }
        }
        to_distribute = (1u32 << table_log).wrapping_sub(distributed);
    }

    if distributed == max_symbol_value + 1 {
        // todos os valores são fracos, provavelmente incompressível: o que sobra vai para o maior
        let mut max_v = 0usize;
        let mut max_c = 0u32;
        for s in 0..=last {
            if count[s] > max_c {
                max_v = s;
                max_c = count[s];
            }
        }
        norm[max_v] = norm[max_v].wrapping_add(to_distribute as i16);
        return Ok(());
    }

    if total == 0 {
        // todos os símbolos caíram em lowOne ou lowThreshold
        let mut s = 0usize;
        while to_distribute > 0 {
            if norm[s] > 0 {
                to_distribute -= 1;
                norm[s] += 1;
            }
            s = (s + 1) % (last + 1);
        }
        return Ok(());
    }

    let v_step_log = 62 - u64::from(table_log);
    let mid = (1u64 << (v_step_log - 1)) - 1;
    let r_step = ((1u64 << v_step_log) * u64::from(to_distribute) + mid) / u64::from(total as u32);
    let mut tmp_total = mid;
    for s in 0..=last {
        if norm[s] == NOT_YET_ASSIGNED {
            let end = tmp_total + u64::from(count[s]) * r_step;
            let s_start = (tmp_total >> v_step_log) as u32;
            let s_end = (end >> v_step_log) as u32;
            let weight = s_end.wrapping_sub(s_start);
            if weight < 1 {
                return Err(FseError::Generic);
            }
            norm[s] = weight as i16;
            tmp_total = end;
        }
    }
    Ok(())
}

/// `FSE_normalizeCount`: devolve o `tableLog` usado, ou 0 no caso especial RLE (um símbolo só).
/// `use_low_prob_count` escolhe o `-1` (probabilidade menor que 1) para os símbolos raros.
pub fn normalize_count(
    normalized: &mut [i16],
    table_log: u32,
    count: &[u32],
    total: usize,
    max_symbol_value: u32,
    use_low_prob_count: bool,
) -> FseResult<u32> {
    const RTB_TABLE: [u64; 8] = [0, 473195, 504333, 520860, 550000, 700000, 750000, 830000];
    let table_log = if table_log == 0 { FSE_DEFAULT_TABLELOG } else { table_log };
    let last = max_symbol_value as usize;
    if table_log < FSE_MIN_TABLELOG {
        return Err(FseError::Generic);
    }
    if table_log > FSE_MAX_TABLELOG {
        return Err(FseError::TableLogTooLarge);
    }
    if table_log < min_table_log(total, max_symbol_value)? {
        return Err(FseError::Generic);
    }
    if normalized.len() <= last || count.len() <= last {
        return Err(FseError::Generic);
    }

    let low_prob_count: i16 = if use_low_prob_count { -1 } else { 1 };
    let scale = 62 - u64::from(table_log);
    let step = (1u64 << 62) / u64::from(total as u32);
    let v_step = 1u64 << (scale - 20);
    let mut still_to_distribute: i32 = 1 << table_log;
    let mut largest = 0usize;
    let mut largest_p: i16 = 0;
    let low_threshold = (total >> table_log) as u32;

    for s in 0..=last {
        if count[s] as usize == total {
            return Ok(0); // caso especial RLE
        }
        if count[s] == 0 {
            normalized[s] = 0;
            continue;
        }
        if count[s] <= low_threshold {
            normalized[s] = low_prob_count;
            still_to_distribute -= 1;
        } else {
            let product = u64::from(count[s]) * step;
            let mut proba = (product >> scale) as i16;
            if proba < 8 {
                let rest_to_beat = v_step * RTB_TABLE[proba as usize];
                proba += i16::from(product - ((proba as u64) << scale) > rest_to_beat);
            }
            if proba > largest_p {
                largest_p = proba;
                largest = s;
            }
            normalized[s] = proba;
            still_to_distribute -= i32::from(proba);
        }
    }
    if -still_to_distribute >= i32::from(normalized[largest] >> 1) {
        // caso de canto: precisa do outro método de normalização
        normalize_m2(normalized, table_log, count, total, max_symbol_value, low_prob_count)?;
    } else {
        normalized[largest] += still_to_distribute as i16;
    }
    Ok(table_log)
}

// ---------------------------------------------------------------------------------------------
// fse_compress.c: cabeçalho da tabela (NCount)
// ---------------------------------------------------------------------------------------------

/// `FSE_NCountWriteBound`.
pub fn ncount_write_bound(max_symbol_value: u32, table_log: u32) -> usize {
    if max_symbol_value == 0 {
        return FSE_NCOUNTBOUND;
    }
    (((max_symbol_value + 1) * table_log + 4 + 2) / 8) as usize + 1 + 2
}

fn put_le16(out: &mut [u8], pos: usize, bit_stream: u32) -> FseResult<()> {
    let dst = out.get_mut(pos..pos + 2).ok_or(FseError::DstSizeTooSmall)?;
    dst[0] = bit_stream as u8;
    dst[1] = (bit_stream >> 8) as u8;
    Ok(())
}

/// `FSE_writeNCount`: escreve o histograma normalizado e devolve os bytes usados. O C tem uma variante
/// "segura" sem checagem de limite; aqui o limite é sempre conferido, o que não muda o resultado quando
/// o buffer tem o tamanho de `ncount_write_bound`.
pub fn write_ncount(out: &mut [u8], normalized: &[i16], max_symbol_value: u32, table_log: u32) -> FseResult<usize> {
    if table_log > FSE_MAX_TABLELOG {
        return Err(FseError::TableLogTooLarge);
    }
    if table_log < FSE_MIN_TABLELOG {
        return Err(FseError::Generic);
    }
    let alphabet_size = max_symbol_value + 1;
    if normalized.len() < alphabet_size as usize {
        return Err(FseError::Generic);
    }
    let mut pos = 0usize;
    let table_size: i32 = 1 << table_log;
    let mut bit_stream: u32 = table_log - FSE_MIN_TABLELOG;
    let mut bit_count: i32 = 4;
    let mut remaining: i32 = table_size + 1; // +1 para precisão extra
    let mut threshold: i32 = table_size;
    let mut nb_bits: i32 = table_log as i32 + 1;
    let mut symbol: u32 = 0;
    let mut previous_is_0 = false;

    while symbol < alphabet_size && remaining > 1 {
        if previous_is_0 {
            let mut start = symbol;
            while symbol < alphabet_size && normalized[symbol as usize] == 0 {
                symbol += 1;
            }
            if symbol == alphabet_size {
                break; // distribuição incorreta
            }
            while symbol >= start + 24 {
                start += 24;
                bit_stream = bit_stream.wrapping_add(0xFFFFu32 << bit_count);
                put_le16(out, pos, bit_stream)?;
                pos += 2;
                bit_stream >>= 16;
            }
            while symbol >= start + 3 {
                start += 3;
                bit_stream = bit_stream.wrapping_add(3u32 << bit_count);
                bit_count += 2;
            }
            bit_stream = bit_stream.wrapping_add((symbol - start) << bit_count);
            bit_count += 2;
            if bit_count > 16 {
                put_le16(out, pos, bit_stream)?;
                pos += 2;
                bit_stream >>= 16;
                bit_count -= 16;
            }
        }
        {
            let mut count = i32::from(normalized[symbol as usize]);
            symbol += 1;
            let max = (2 * threshold - 1) - remaining;
            remaining -= count.abs();
            count += 1; // +1 para precisão extra
            if count >= threshold {
                count += max; // [0..max[ [max..threshold[ (...) [threshold+max 2*threshold[
            }
            bit_stream = bit_stream.wrapping_add((count as u32) << bit_count);
            bit_count += nb_bits;
            bit_count -= i32::from(count < max);
            previous_is_0 = count == 1;
            if remaining < 1 {
                return Err(FseError::Generic);
            }
            while remaining < threshold {
                nb_bits -= 1;
                threshold >>= 1;
            }
        }
        if bit_count > 16 {
            put_le16(out, pos, bit_stream)?;
            pos += 2;
            bit_stream >>= 16;
            bit_count -= 16;
        }
    }

    if remaining != 1 {
        return Err(FseError::Generic); // distribuição normalizada incorreta
    }
    put_le16(out, pos, bit_stream)?;
    Ok(pos + ((bit_count + 7) / 8) as usize)
}

// ---------------------------------------------------------------------------------------------
// fse_compress.c: tabela de compressão
// ---------------------------------------------------------------------------------------------

/// `FSE_symbolCompressionTransform`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SymbolTransform {
    pub delta_find_state: i32,
    pub delta_nb_bits: u32,
}

/// `FSE_CTable`: o cabeçalho (`tableLog`, `maxSymbolValue`), a tabela de estados e a de transformação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CTable {
    pub table_log: u32,
    pub max_symbol_value: u32,
    pub state_table: Vec<u16>,
    pub symbol_tt: Vec<SymbolTransform>,
}

/// `FSE_TABLESTEP`.
fn table_step(table_size: u32) -> u32 {
    (table_size >> 1) + (table_size >> 3) + 3
}

/// `FSE_buildCTable`. O `normalized` precisa somar `1 << table_log` (o `-1` conta 1); o C só afirma isso
/// com `assert`, aqui vira `Generic`. A trilha de 8 em 8 bytes do C (sem símbolos de baixa
/// probabilidade) é otimização: o laço geral produz a mesma tabela.
pub fn build_ctable(normalized: &[i16], max_symbol_value: u32, table_log: u32) -> FseResult<CTable> {
    if table_log == 0 || table_log > FSE_MAX_TABLELOG {
        return Err(FseError::TableLogTooLarge);
    }
    let table_size = 1u32 << table_log;
    let table_mask = table_size - 1;
    let step = table_step(table_size);
    let max_sv1 = max_symbol_value as usize + 1;
    if normalized.len() < max_sv1 {
        return Err(FseError::Generic);
    }
    let mut sum: u32 = 0;
    for &n in &normalized[..max_sv1] {
        if n < -1 {
            return Err(FseError::Generic);
        }
        sum += if n == -1 { 1 } else { n as u32 };
    }
    if sum != table_size {
        return Err(FseError::Generic);
    }

    let mut table_symbol = vec![0u16; table_size as usize];
    let mut cumul = vec![0u16; max_sv1 + 1];
    let mut high_threshold = table_size - 1;

    // posições iniciais de cada símbolo
    for u in 1..=max_sv1 {
        let n = normalized[u - 1];
        if n == -1 {
            cumul[u] = cumul[u - 1] + 1;
            table_symbol[high_threshold as usize] = (u - 1) as u16;
            high_threshold = high_threshold.wrapping_sub(1);
        } else {
            cumul[u] = cumul[u - 1] + n as u16;
        }
    }
    cumul[max_sv1] = (table_size + 1) as u16;

    // espalha os símbolos
    let mut position: u32 = 0;
    for (symbol, &n) in normalized[..max_sv1].iter().enumerate() {
        for _ in 0..n.max(0) {
            table_symbol[position as usize] = symbol as u16;
            position = (position + step) & table_mask;
            while position > high_threshold {
                position = (position + step) & table_mask; // área de baixa probabilidade
            }
        }
    }
    if position != 0 {
        return Err(FseError::Generic);
    }

    // tabela de estados, ordenada por símbolo: dá o próximo valor de estado
    let mut state_table = vec![0u16; table_size as usize];
    for (u, &s) in table_symbol.iter().enumerate() {
        let slot = &mut cumul[s as usize];
        state_table[*slot as usize] = (table_size as usize + u) as u16;
        *slot += 1;
    }

    // tabela de transformação de símbolos
    let mut symbol_tt = vec![SymbolTransform::default(); max_sv1];
    let mut total: u32 = 0;
    for (s, &n) in normalized[..max_sv1].iter().enumerate() {
        let tt = &mut symbol_tt[s];
        match n {
            0 => {
                // preenche mesmo assim, por compatibilidade com `FSE_getMaxNbBits`
                tt.delta_nb_bits = ((table_log + 1) << 16).wrapping_sub(1 << table_log);
            }
            -1 | 1 => {
                tt.delta_nb_bits = (table_log << 16).wrapping_sub(1 << table_log);
                tt.delta_find_state = total.wrapping_sub(1) as i32;
                total += 1;
            }
            _ => {
                let max_bits_out = table_log - highbit32(n as u32 - 1);
                let min_state_plus = (n as u32) << max_bits_out;
                tt.delta_nb_bits = (max_bits_out << 16).wrapping_sub(min_state_plus);
                tt.delta_find_state = total.wrapping_sub(n as u32) as i32;
                total += n as u32;
            }
        }
    }
    Ok(CTable { table_log, max_symbol_value, state_table, symbol_tt })
}

/// `FSE_buildCTable_rle`: tabela falsa para entrada com um símbolo só.
pub fn build_ctable_rle(symbol_value: u8) -> CTable {
    let mut symbol_tt = vec![SymbolTransform::default(); symbol_value as usize + 1];
    symbol_tt[symbol_value as usize] = SymbolTransform { delta_find_state: 0, delta_nb_bits: 0 };
    CTable { table_log: 0, max_symbol_value: u32::from(symbol_value), state_table: vec![0, 0], symbol_tt }
}

// ---------------------------------------------------------------------------------------------
// bitstream.h: BIT_CStream
// ---------------------------------------------------------------------------------------------

/// `BIT_CStream_t` com contêiner de 64 bits. Dono do buffer de saída, de capacidade fixa.
#[derive(Debug)]
pub struct BitCStream {
    bit_container: u64,
    bit_pos: u32,
    buf: Vec<u8>,
    ptr: usize,
    end_ptr: usize,
}

impl BitCStream {
    /// `BIT_initCStream`: a capacidade precisa passar de 8 bytes (o tamanho do contêiner).
    pub fn new(capacity: usize) -> FseResult<Self> {
        if capacity <= 8 {
            return Err(FseError::DstSizeTooSmall);
        }
        Ok(Self { bit_container: 0, bit_pos: 0, buf: vec![0; capacity], ptr: 0, end_ptr: capacity - 8 })
    }

    /// `BIT_addBits`: o valor é mascarado em `nb_bits` bits (no máximo 31).
    pub fn add_bits(&mut self, value: u64, nb_bits: u32) {
        let mask = (1u64 << nb_bits) - 1;
        self.bit_container |= (value & mask).wrapping_shl(self.bit_pos);
        self.bit_pos += nb_bits;
    }

    /// `BIT_flushBits`: descarrega os bytes completos, sem passar de `end_ptr` (o estouro só aparece
    /// em `close`).
    pub fn flush_bits(&mut self) {
        let nb_bytes = (self.bit_pos >> 3) as usize;
        self.buf[self.ptr..self.ptr + 8].copy_from_slice(&self.bit_container.to_le_bytes());
        self.ptr = (self.ptr + nb_bytes).min(self.end_ptr);
        self.bit_pos &= 7;
        self.bit_container >>= nb_bytes * 8;
    }

    /// `BIT_closeCStream`: marca de fim, descarrega e devolve o tamanho; estouro vira `DstSizeTooSmall`
    /// (o C devolve 0).
    pub fn close(&mut self) -> FseResult<usize> {
        self.add_bits(1, 1);
        self.flush_bits();
        if self.ptr >= self.end_ptr {
            return Err(FseError::DstSizeTooSmall);
        }
        Ok(self.ptr + usize::from(self.bit_pos > 0))
    }

    /// Os primeiros `len` bytes escritos (use o valor devolvido por `close`).
    pub fn bytes(&self, len: usize) -> &[u8] {
        &self.buf[..len]
    }
}

// ---------------------------------------------------------------------------------------------
// fse.h: estado de codificação
// ---------------------------------------------------------------------------------------------

/// `FSE_CState_t`.
#[derive(Debug, Clone, Copy)]
pub struct CState<'a> {
    pub value: u32,
    ct: &'a CTable,
    state_log: u32,
}

impl<'a> CState<'a> {
    /// `FSE_initCState`.
    pub fn new(ct: &'a CTable) -> Self {
        Self { value: 1u32 << ct.table_log, ct, state_log: ct.table_log }
    }

    fn transform(&self, symbol: u32) -> FseResult<SymbolTransform> {
        self.ct.symbol_tt.get(symbol as usize).copied().ok_or(FseError::Generic)
    }

    fn next_state(&self, value: u32, nb_bits_out: u32, tt: SymbolTransform) -> FseResult<u32> {
        let index = i64::from(value >> nb_bits_out) + i64::from(tt.delta_find_state);
        usize::try_from(index)
            .ok()
            .and_then(|i| self.ct.state_table.get(i))
            .map(|&v| u32::from(v))
            .ok_or(FseError::Generic)
    }

    /// `FSE_initCState2`: inicia o estado já com o primeiro símbolo, sem emitir bits.
    pub fn with_symbol(ct: &'a CTable, symbol: u32) -> FseResult<Self> {
        let mut state = Self::new(ct);
        let tt = state.transform(symbol)?;
        let nb_bits_out = tt.delta_nb_bits.wrapping_add(1 << 15) >> 16;
        let value = (nb_bits_out << 16).wrapping_sub(tt.delta_nb_bits);
        state.value = state.next_state(value, nb_bits_out, tt)?;
        Ok(state)
    }

    /// `FSE_encodeSymbol`.
    pub fn encode_symbol(&mut self, bit_c: &mut BitCStream, symbol: u32) -> FseResult<()> {
        let tt = self.transform(symbol)?;
        let nb_bits_out = self.value.wrapping_add(tt.delta_nb_bits) >> 16;
        bit_c.add_bits(u64::from(self.value), nb_bits_out);
        self.value = self.next_state(self.value, nb_bits_out, tt)?;
        Ok(())
    }

    /// `FSE_flushCState`.
    pub fn flush(&self, bit_c: &mut BitCStream) {
        bit_c.add_bits(u64::from(self.value), self.state_log);
        bit_c.flush_bits();
    }
}

/// `FSE_compress_usingCTable`: comprime `src` com a tabela. `Ok(None)` é o retorno 0 do C (entrada com
/// no máximo 2 símbolos, ou sem espaço em `dst_size`). A cadência de `flush_bits` do C (rápida ou não)
/// só muda a velocidade: os bytes saem iguais, então aqui o descarrega vai a cada par de símbolos.
pub fn compress_using_ctable(dst_size: usize, src: &[u8], ct: &CTable) -> FseResult<Option<Vec<u8>>> {
    let n = src.len();
    if n <= 2 {
        return Ok(None);
    }
    let Ok(mut bit_c) = BitCStream::new(dst_size) else {
        return Ok(None);
    };
    let mut ip = n;
    let (mut state1, mut state2);
    if n & 1 == 1 {
        state1 = CState::with_symbol(ct, u32::from(src[ip - 1]))?;
        state2 = CState::with_symbol(ct, u32::from(src[ip - 2]))?;
        state1.encode_symbol(&mut bit_c, u32::from(src[ip - 3]))?;
        bit_c.flush_bits();
        ip -= 3;
    } else {
        state2 = CState::with_symbol(ct, u32::from(src[ip - 1]))?;
        state1 = CState::with_symbol(ct, u32::from(src[ip - 2]))?;
        ip -= 2;
    }
    while ip > 0 {
        state2.encode_symbol(&mut bit_c, u32::from(src[ip - 1]))?;
        state1.encode_symbol(&mut bit_c, u32::from(src[ip - 2]))?;
        bit_c.flush_bits();
        ip -= 2;
    }
    state2.flush(&mut bit_c);
    state1.flush(&mut bit_c);
    match bit_c.close() {
        Ok(size) => Ok(Some(bit_c.bytes(size).to_vec())),
        Err(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hist_counts_and_finds_max_symbol() {
        let mut count = [7u32; 256];
        let r = hist_count(&mut count, 255, b"aabbbc").unwrap();
        assert_eq!(r, (3, b'c' as u32));
        assert_eq!((count[97], count[98], count[99], count[100]), (2, 3, 1, 0));
        assert_eq!(hist_count(&mut count, 255, b""), Ok((0, 0)));
        assert_eq!(count[97], 0);
        assert_eq!(hist_count(&mut count, 98, b"abc"), Err(FseError::MaxSymbolValueTooSmall));
    }

    #[test]
    fn optimal_table_log_vectors() {
        // maxBitsSrc = highbit(999) - 2 = 7; minBits = min(10, 7) = 7
        assert_eq!(optimal_table_log(9, 1000, 35), Ok(7));
        // maxBitsSrc = 4, minBits = min(7, 7) = 7 sobe o tableLog
        assert_eq!(optimal_table_log(6, 100, 52), Ok(7));
        // maxTableLog 0 vira o padrão 11: min(14, 11) = 11, minBits = 9
        assert_eq!(optimal_table_log(0, 100000, 255), Ok(11));
        assert_eq!(optimal_table_log(9, 1, 5), Err(FseError::Generic));
    }

    #[test]
    fn normalize_count_vectors() {
        let mut norm = [0i16; 2];
        assert_eq!(normalize_count(&mut norm, 5, &[8, 8], 16, 1, false), Ok(5));
        assert_eq!(norm, [16, 16]);
        // 1/3 de 32 = 10,67: cada um fica com 10 e o primeiro (maior) recebe os 2 que faltam
        let mut norm = [0i16; 3];
        assert_eq!(normalize_count(&mut norm, 5, &[1, 1, 1], 3, 2, false), Ok(5));
        assert_eq!(norm, [12, 10, 10]);
        // RLE
        let mut norm = [0i16; 2];
        assert_eq!(normalize_count(&mut norm, 5, &[5, 0], 5, 1, false), Ok(0));
    }

    #[test]
    fn normalize_m2_distributes_evenly() {
        // quatro símbolos de contagem 1: rStep = (2^62 + 2^56 - 1) / 4, pesos 8, 8, 8, 8
        let mut norm = [0i16; 4];
        normalize_m2(&mut norm, 5, &[1, 1, 1, 1], 4, 3, 1).unwrap();
        assert_eq!(norm, [8, 8, 8, 8]);
    }

    #[test]
    fn write_ncount_two_symbols() {
        // bitStream = 0 | 17 << 4 | 31 << 9 = 16144 = 0x3F10, bitCount = 14 => 2 bytes
        let mut out = [0u8; 16];
        let n = write_ncount(&mut out, &[16, 16], 1, 5).unwrap();
        assert_eq!(&out[..n], &[0x10, 0x3F]);
        assert_eq!(write_ncount(&mut out, &[16, 16], 1, 13), Err(FseError::TableLogTooLarge));
        assert_eq!(write_ncount(&mut out[..1], &[16, 16], 1, 5), Err(FseError::DstSizeTooSmall));
    }

    #[test]
    fn build_ctable_two_symbols() {
        // tableSize 32, step 23: o símbolo 0 ocupa 23*k mod 32 para k < 16
        let ct = build_ctable(&[16, 16], 1, 5).unwrap();
        let zero: [u16; 16] = [0, 1, 2, 5, 6, 10, 11, 14, 15, 19, 20, 23, 24, 25, 28, 29];
        let one: [u16; 16] = [3, 4, 7, 8, 9, 12, 13, 16, 17, 18, 21, 22, 26, 27, 30, 31];
        for i in 0..16 {
            assert_eq!(ct.state_table[i], 32 + zero[i]);
            assert_eq!(ct.state_table[16 + i], 32 + one[i]);
        }
        // maxBitsOut = 5 - highbit(15) = 2; deltaNbBits = (2 << 16) - (16 << 2)
        assert_eq!(ct.symbol_tt[0], SymbolTransform { delta_find_state: -16, delta_nb_bits: 131008 });
        assert_eq!(ct.symbol_tt[1], SymbolTransform { delta_find_state: 0, delta_nb_bits: 131008 });
        assert_eq!(build_ctable(&[16, 15], 1, 5), Err(FseError::Generic));
    }

    #[test]
    fn encode_states() {
        let ct = build_ctable(&[16, 16], 1, 5).unwrap();
        // initCState2(simbolo 0): value = 64, estado = stateTable[(64 >> 2) - 16] = 32
        assert_eq!(CState::with_symbol(&ct, 0).unwrap().value, 32);
        // símbolo 1: deltaFindState 0, índice 64 >> 2 = 16, estado = 32 + 3 = 35
        let mut st = CState::with_symbol(&ct, 1).unwrap();
        assert_eq!(st.value, 35);
        let mut bits = BitCStream::new(16).unwrap();
        // nbBitsOut = (35 + 131008) >> 16 = 1; bit emitido = 35 & 1 = 1; novo estado = stateTable[17] = 36
        st.encode_symbol(&mut bits, 1).unwrap();
        assert_eq!(st.value, 36);
        assert_eq!(bits.bit_pos, 1);
        assert_eq!(bits.bit_container, 1);
        assert_eq!(CState::new(&ct).value, 32);
    }

    #[test]
    fn bit_cstream_layout() {
        let mut b = BitCStream::new(16).unwrap();
        b.add_bits(0b101, 3);
        b.add_bits(0xFF, 8);
        b.flush_bits();
        let n = b.close().unwrap();
        // 0x7FD: um byte vai para fora (0xFD), sobram 3 bits (0b111) + marca de fim => 0x0F
        assert_eq!(b.bytes(n), &[0xFD, 0x0F]);
        assert!(BitCStream::new(8).is_err());
    }
}
