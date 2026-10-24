//! As sequências de escape ANSI do bun 1.4.2 (`ANSIHelpers.h`): onde começa uma (`findEscapeCharacter`) e até onde ela
//! vai (`consumeANSI`, a máquina de estados de CSI, OSC, DCS/SOS/PM/APC e escapes de um caractere, com os
//! terminadores BEL, ST de 8 bits e `ESC \`, e o CAN/SUB que aborta). As varreduras SIMD do C++ são atalhos; o
//! resultado é o do laço escalar, que é o que vale aqui, sobre unidades de UTF-16 (o Latin-1 cabe nelas sem mudança).
//!
//! Quem usa: `Bun.stripANSI` e o `Bun.stringWidth` sem `countAnsiEscapeCodes`.

/// `isEscapeCharacter`: ESC e os introdutores C1 (CSI, OSC, DCS, SOS, PM, APC).
pub(crate) fn is_escape_character(unit: u16) -> bool {
    matches!(unit, 0x1b | 0x9b | 0x9d | 0x90 | 0x98 | 0x9e | 0x9f)
}

/// `findEscapeCharacter`: o primeiro introdutor, ou o ST de 8 bits (0x9C), que o C++ também para.
pub(crate) fn find_escape_character(units: &[u16]) -> Option<usize> {
    units.iter().position(|unit| is_escape_character(*unit) || *unit == 0x9c)
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    Start,
    GotEsc,
    IgnoreNextChar,
    InCsi,
    InOsc,
    InOscGotEsc,
    NeedSt,
    NeedStGotEsc,
}

/// `consumeANSI(start, end)`: o índice logo depois da sequência que começa em `start`; `start` mesmo quando ali não
/// começa sequência nenhuma (o ST solto, por exemplo), e o fim quando a sequência não termina.
pub(crate) fn consume_ansi(units: &[u16], start: usize) -> usize {
    let mut state = State::Start;
    let mut it = start;
    while it < units.len() {
        let c = units[it];
        match state {
            State::Start => match c {
                0x1b => state = State::GotEsc,
                0x9b => state = State::InCsi,
                0x9d => state = State::InOsc,
                0x90 | 0x98 | 0x9e | 0x9f => state = State::NeedSt,
                _ => return it,
            },
            State::InOscGotEsc | State::NeedStGotEsc | State::GotEsc => {
                if state != State::GotEsc {
                    if c == u16::from(b'\\') {
                        // `ESC \` é o terminador de string.
                        state = State::Start;
                        it += 1;
                        continue;
                    }
                    state = State::GotEsc;
                }
                match c {
                    0x1b => {}
                    0x18 | 0x1a => state = State::Start,
                    0x5b => state = State::InCsi,
                    0x5d => state = State::InOsc,
                    // `P` (DCS), `X` (SOS), `^` (PM), `_` (APC).
                    0x50 | 0x58 | 0x5e | 0x5f => state = State::NeedSt,
                    0x20..=0x2f => state = State::IgnoreNextChar,
                    0x30..=0x7e | 0x9c => state = State::Start,
                    _ => return it,
                }
            }
            State::IgnoreNextChar => state = if c == 0x1b { State::GotEsc } else { State::Start },
            State::InCsi => {
                // O byte final (0x40 a 0x7E, ECMA-48) ou o ESC, CAN, SUB e ST que abortam.
                let Some(offset) = units[it..].iter().position(|unit| matches!(*unit, 0x40..=0x7e | 0x1b | 0x18 | 0x1a | 0x9c)) else {
                    return units.len();
                };
                it += offset;
                state = if units[it] == 0x1b { State::GotEsc } else { State::Start };
            }
            State::InOsc | State::NeedSt => {
                let osc = state == State::InOsc;
                let Some(offset) = units[it..].iter().position(|unit| matches!(*unit, 0x9c | 0x1b | 0x18 | 0x1a) || (osc && *unit == 0x07)) else {
                    return units.len();
                };
                it += offset;
                state = if units[it] != 0x1b {
                    State::Start
                } else if osc {
                    State::InOscGotEsc
                } else {
                    State::NeedStGotEsc
                };
            }
        }
        it += 1;
    }
    units.len()
}

/// `stripANSI`: o texto sem as sequências; `None` quando não havia nenhuma (o chamador devolve a string original).
/// O introdutor que não abre sequência (o ST solto) fica no texto.
pub(crate) fn strip_ansi(units: &[u16]) -> Option<Vec<u16>> {
    let mut output: Vec<u16> = Vec::new();
    let mut found = false;
    let mut start = 0;
    while let Some(offset) = find_escape_character(&units[start..]) {
        let escape = start + offset;
        output.extend_from_slice(&units[start..escape]);
        let next = consume_ansi(units, escape);
        if next == escape {
            output.push(units[escape]);
            start = escape + 1;
            continue;
        }
        found = true;
        start = next;
    }
    if !found {
        return None;
    }
    output.extend_from_slice(&units[start..]);
    Some(output)
}
