//! `fogu build --target wasm`: puts the program bundle inside the Fogu
//! WebAssembly runtime (`fogu-wasm.wasm`).
//!
//! A WASI module cannot read its own file, so the bundle goes into linear
//! memory instead: a new active data segment is added right after the
//! module's initial memory (which is enlarged to hold it), and the runtime's
//! slot — a static `FOGU-WASM-BUNDLE` marker followed by two u32s — gets the
//! address and length. The allocator grows memory past the current end, so
//! the bundle is never overwritten.

const PAGE: usize = 65536;

/// The marker of the runtime's slot, decoded at run time so that this code
/// (which is part of the runtime too) does not contain the marker itself.
fn marker() -> Vec<u8> {
    const XORED: [u8; 16] = [
        0x1c, 0x15, 0x1d, 0x0f, 0x77, 0x0d, 0x1b, 0x09, 0x17, 0x77, 0x18, 0x0f, 0x14, 0x1e, 0x16, 0x1f,
    ];
    XORED.iter().map(|b| b ^ 0x5a).collect()
}

fn read_uleb(b: &[u8], pos: &mut usize) -> Result<u64, String> {
    let (mut v, mut shift) = (0u64, 0);
    loop {
        let byte = *b.get(*pos).ok_or("wasm: truncated module")?;
        *pos += 1;
        v |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok(v);
        }
        shift += 7;
        if shift > 63 {
            return Err("wasm: bad LEB128".into());
        }
    }
}

fn uleb(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn sleb(mut v: i64, out: &mut Vec<u8>) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        let done = (v == 0 && byte & 0x40 == 0) || (v == -1 && byte & 0x40 != 0);
        out.push(if done { byte } else { byte | 0x80 });
        if done {
            return;
        }
    }
}

/// Returns a copy of `module` carrying `payload`.
pub fn embed(module: &[u8], payload: &[u8]) -> Result<Vec<u8>, String> {
    if module.len() < 8 || &module[..4] != b"\0asm" {
        return Err("not a WebAssembly module".into());
    }
    // sections: (id, start of content, end)
    let mut sections = Vec::new();
    let mut pos = 8;
    while pos < module.len() {
        let id = module[pos];
        pos += 1;
        let size = read_uleb(module, &mut pos)? as usize;
        let start = pos;
        pos = start.checked_add(size).filter(|&e| e <= module.len()).ok_or("wasm: truncated section")?;
        sections.push((id, start, pos));
    }
    let mem = sections.iter().find(|s| s.0 == 5).ok_or("wasm: the module has no memory section")?;
    let data = sections.iter().find(|s| s.0 == 11).ok_or("wasm: the module has no data section")?;

    // memory section: vec(limits) — the first memory's minimum grows
    let mut p = mem.1;
    let count = read_uleb(module, &mut p)?;
    if count != 1 {
        return Err("wasm: expected exactly one memory".into());
    }
    let flags = read_uleb(module, &mut p)?;
    if flags > 1 {
        return Err("wasm: shared or 64-bit memories are not supported".into());
    }
    let min = read_uleb(module, &mut p)? as usize;
    let max = if flags == 1 { Some(read_uleb(module, &mut p)? as usize) } else { None };
    let offset = min * PAGE;
    let new_min = min + payload.len().div_ceil(PAGE).max(1);
    if max.is_some_and(|m| m < new_min) {
        return Err("wasm: the program does not fit in the runtime's maximum memory".into());
    }
    if offset + payload.len() > u32::MAX as usize {
        return Err("wasm: the program is too large".into());
    }
    let mut new_mem = Vec::new();
    uleb(1, &mut new_mem);
    uleb(flags, &mut new_mem);
    uleb(new_min as u64, &mut new_mem);
    if let Some(m) = max {
        uleb(m as u64, &mut new_mem);
    }

    // data section: patch the slot, then append the new segment
    let mut new_data = module[data.1..data.2].to_vec();
    let mk = marker();
    let hits: Vec<usize> = new_data.windows(mk.len()).enumerate().filter(|(_, w)| *w == mk.as_slice()).map(|(i, _)| i).collect();
    let slot = match hits.as_slice() {
        [one] => one + mk.len(),
        [] => return Err("this runtime has no bundle slot (is it a fogu WebAssembly runtime?)".into()),
        _ => return Err("wasm: the bundle slot is ambiguous".into()),
    };
    if new_data[slot..slot + 8].iter().any(|b| *b != 0) {
        return Err("this runtime already carries a program: start from fogu-wasm.wasm".into());
    }
    new_data[slot..slot + 4].copy_from_slice(&(offset as u32).to_le_bytes());
    new_data[slot + 4..slot + 8].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    let mut dp = 0;
    let n = read_uleb(&new_data, &mut dp)?;
    let mut seg = Vec::new();
    uleb(n + 1, &mut seg);
    seg.extend_from_slice(&new_data[dp..]);
    // active segment for memory 0: i32.const offset; end; bytes
    seg.push(0);
    seg.push(0x41);
    sleb(offset as i64, &mut seg);
    seg.push(0x0b);
    uleb(payload.len() as u64, &mut seg);
    seg.extend_from_slice(payload);

    // write the module back, with the data count section (if any) updated
    let mut out = module[..8].to_vec();
    for &(id, start, end) in &sections {
        let content: Vec<u8> = match id {
            5 => new_mem.clone(),
            11 => seg.clone(),
            12 => {
                let mut c = Vec::new();
                uleb(n + 1, &mut c);
                c
            }
            _ => module[start..end].to_vec(),
        };
        out.push(id);
        uleb(content.len() as u64, &mut out);
        out.extend_from_slice(&content);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leb128_round_trips() {
        for v in [0u64, 1, 127, 128, 300, 65536, u32::MAX as u64] {
            let mut b = Vec::new();
            uleb(v, &mut b);
            let mut p = 0;
            assert_eq!(read_uleb(&b, &mut p).unwrap(), v);
        }
        let mut b = Vec::new();
        sleb(65536 * 20, &mut b);
        assert_eq!(b, vec![0x80, 0x80, 0xd0, 0x00]);
    }
}
