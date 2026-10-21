//! The `.lfng` model file format (spec section 2.6), little-endian.

use crate::alphabet::{Language, MAX_ALPHABET};

/// File magic.
pub const MAGIC: [u8; 4] = *b"LFNG";
/// The only supported format version.
pub const VERSION: u16 = 1;
/// Largest n-gram order a key can hold.
pub const MAX_ORDER: u8 = 4;

/// Error reading a model.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    /// The file ends early or has trailing bytes.
    #[error("model file is truncated or has trailing bytes")]
    Truncated,
    /// The file does not start with `LFNG`.
    #[error("not an .lfng model (bad magic)")]
    BadMagic,
    /// A version this build cannot read.
    #[error("unsupported model version {0}")]
    UnsupportedVersion(u16),
    /// The checksum does not match the contents.
    #[error("model checksum mismatch")]
    BadCrc,
    /// The language code is not one this build knows.
    #[error("unknown model language {0:?}")]
    UnknownLanguage([u8; 2]),
    /// The order is 0 or larger than 4.
    #[error("unsupported model order {0}")]
    BadOrder(u8),
    /// The stored alphabet does not match the language's alphabet.
    #[error("model alphabet does not match language {0}")]
    BadAlphabet(&'static str),
    /// Keys are not strictly increasing, or a key is malformed.
    #[error("model tables are not sorted or hold a bad key")]
    BadTable,
}

/// Contents of a model file.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelData {
    /// N-gram order (context length + 1).
    pub order: u8,
    /// The model's language.
    pub lang: Language,
    /// Average log2 probability per char on held-out text.
    pub h: f32,
    /// `(key, count)` of each n-gram, sorted by key.
    pub ngrams: Vec<(u32, u32)>,
    /// `(key, total, types)` of each context, sorted by key.
    pub contexts: Vec<(u32, u32, u32)>,
}

/// Pack up to four alphabet indices into a key (spec section 2.6).
/// Extra symbols past the fourth are ignored.
pub fn pack_key(symbols: &[u8]) -> u32 {
    let len = symbols.len().min(4);
    let mut key = (len as u32) << 24;
    for (slot, &s) in symbols.iter().take(4).enumerate() {
        key |= u32::from(s & 0x3F) << (18 - 6 * slot);
    }
    key
}

/// Number of symbols in a key.
pub fn key_len(key: u32) -> usize {
    (key >> 24) as usize
}

/// The alphabet indices packed in a key.
pub fn unpack_key(key: u32) -> Vec<u8> {
    (0..key_len(key).min(4))
        .map(|slot| ((key >> (18 - 6 * slot)) & 0x3F) as u8)
        .collect()
}

/// Key of an n-gram's context: the n-gram without its last symbol.
pub fn context_of(key: u32) -> u32 {
    let len = key_len(key).clamp(1, 4);
    let slot_mask = 0x3F << (18 - 6 * (len - 1));
    ((len as u32 - 1) << 24) | (key & 0x00FF_FFFF & !slot_mask)
}

/// Serialize a model.
pub fn encode(m: &ModelData) -> Vec<u8> {
    let alphabet = m.lang.alphabet();
    let mut out = Vec::with_capacity(32 + 8 * m.ngrams.len() + 12 * m.contexts.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.push(m.order);
    out.extend_from_slice(m.lang.code().as_bytes());
    out.push(alphabet.len() as u8);
    for &c in alphabet {
        out.extend_from_slice(&u32::from(c).to_le_bytes());
    }
    out.extend_from_slice(&m.h.to_le_bytes());
    out.extend_from_slice(&(m.ngrams.len() as u32).to_le_bytes());
    for &(key, count) in &m.ngrams {
        out.extend_from_slice(&key.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
    }
    out.extend_from_slice(&(m.contexts.len() as u32).to_le_bytes());
    for &(key, total, types) in &m.contexts {
        out.extend_from_slice(&key.to_le_bytes());
        out.extend_from_slice(&total.to_le_bytes());
        out.extend_from_slice(&types.to_le_bytes());
    }
    let crc = crc32(&out);
    out.extend_from_slice(&crc.to_le_bytes());
    out
}

/// Parse and validate a model.
pub fn decode(b: &[u8]) -> Result<ModelData, ModelError> {
    if b.len() < 4 {
        return Err(ModelError::Truncated);
    }
    if b[..4] != MAGIC {
        return Err(ModelError::BadMagic);
    }
    let (body, crc) = b.split_at(b.len() - 4);
    let mut r = Reader { b: body, pos: 4 };
    let version = r.u16()?;
    if version != VERSION {
        return Err(ModelError::UnsupportedVersion(version));
    }
    if crc32(body).to_le_bytes() != crc {
        return Err(ModelError::BadCrc);
    }

    let order = r.u8()?;
    if order == 0 || order > MAX_ORDER {
        return Err(ModelError::BadOrder(order));
    }
    let code = [r.u8()?, r.u8()?];
    let lang = std::str::from_utf8(&code)
        .ok()
        .and_then(Language::from_code)
        .ok_or(ModelError::UnknownLanguage(code))?;
    let alphabet_len = usize::from(r.u8()?);
    let mut alphabet = Vec::with_capacity(alphabet_len);
    for _ in 0..alphabet_len {
        alphabet.push(char::from_u32(r.u32()?));
    }
    if alphabet_len > MAX_ALPHABET
        || !alphabet
            .iter()
            .copied()
            .eq(lang.alphabet().iter().map(|&c| Some(c)))
    {
        return Err(ModelError::BadAlphabet(lang.code()));
    }
    let h = f32::from_le_bytes(r.take::<4>()?);

    let n = r.u32()? as usize;
    let mut ngrams = Vec::with_capacity(n.min(r.remaining() / 8));
    for _ in 0..n {
        ngrams.push((r.u32()?, r.u32()?));
    }
    let n = r.u32()? as usize;
    let mut contexts = Vec::with_capacity(n.min(r.remaining() / 12));
    for _ in 0..n {
        contexts.push((r.u32()?, r.u32()?, r.u32()?));
    }
    if r.remaining() != 0 {
        return Err(ModelError::Truncated);
    }

    let ok_key = |key: u32, min_len: usize, max_len: usize| {
        let len = key_len(key);
        len >= min_len
            && len <= max_len
            && unpack_key(key)
                .iter()
                .all(|&s| usize::from(s) < alphabet_len)
            && pack_key(&unpack_key(key)) == key
    };
    let order = usize::from(order);
    let sorted_ngrams = ngrams.windows(2).all(|w| w[0].0 < w[1].0)
        && ngrams.iter().all(|&(k, _)| ok_key(k, 1, order));
    let sorted_contexts = contexts.windows(2).all(|w| w[0].0 < w[1].0)
        && contexts.iter().all(|&(k, _, _)| ok_key(k, 0, order - 1));
    if !sorted_ngrams || !sorted_contexts {
        return Err(ModelError::BadTable);
    }

    Ok(ModelData {
        order: order as u8,
        lang,
        h,
        ngrams,
        contexts,
    })
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], ModelError> {
        let bytes = self
            .b
            .get(self.pos..self.pos + N)
            .ok_or(ModelError::Truncated)?;
        self.pos += N;
        let mut out = [0; N];
        out.copy_from_slice(bytes);
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, ModelError> {
        Ok(self.take::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, ModelError> {
        Ok(u16::from_le_bytes(self.take()?))
    }

    fn u32(&mut self) -> Result<u32, ModelError> {
        Ok(u32::from_le_bytes(self.take()?))
    }

    fn remaining(&self) -> usize {
        self.b.len() - self.pos
    }
}

/// CRC-32 (IEEE 802.3, reflected, polynomial 0xEDB88320).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc = CRC_TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

const CRC_TABLE: [u32; 256] = crc_table();

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            bit += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ModelData {
        ModelData {
            order: 4,
            lang: Language::Fa,
            h: -3.25,
            ngrams: vec![(pack_key(&[3]), 7), (pack_key(&[0, 3]), 2)],
            contexts: vec![(pack_key(&[]), 9, 1), (pack_key(&[0]), 2, 1)],
        }
    }

    #[test]
    fn crc32_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn key_packing() {
        let k = pack_key(&[1, 2, 3, 4]);
        assert_eq!(k, (4 << 24) | (1 << 18) | (2 << 12) | (3 << 6) | 4);
        assert_eq!(pack_key(&[5]), (1 << 24) | (5 << 18));
        assert_eq!(pack_key(&[]), 0);
        assert_eq!(unpack_key(k), vec![1, 2, 3, 4]);
        assert_eq!(context_of(k), pack_key(&[1, 2, 3]));
        assert_eq!(context_of(pack_key(&[5])), pack_key(&[]));
        assert!(pack_key(&[0, 0]) < pack_key(&[0, 0, 0]));
    }

    #[test]
    fn round_trip() {
        let m = sample();
        let bytes = encode(&m);
        assert_eq!(&bytes[..4], b"LFNG");
        assert_eq!(decode(&bytes), Ok(m));
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = encode(&sample());
        bytes[0] = b'X';
        assert_eq!(decode(&bytes), Err(ModelError::BadMagic));
        assert_eq!(decode(b"LF"), Err(ModelError::Truncated));
        assert_eq!(decode(b""), Err(ModelError::Truncated));
    }

    #[test]
    fn rejects_bad_version() {
        let mut bytes = encode(&sample());
        bytes[4] = 2;
        assert_eq!(decode(&bytes), Err(ModelError::UnsupportedVersion(2)));
    }

    #[test]
    fn rejects_bad_crc() {
        let mut bytes = encode(&sample());
        let n = bytes.len();
        bytes[n - 10] ^= 1;
        assert_eq!(decode(&bytes), Err(ModelError::BadCrc));
        let mut bytes = encode(&sample());
        bytes[n - 1] ^= 1;
        assert_eq!(decode(&bytes), Err(ModelError::BadCrc));
    }

    /// Re-encode after editing the body so the CRC is valid again.
    fn with_body(mut bytes: Vec<u8>, edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        bytes.truncate(bytes.len() - 4);
        edit(&mut bytes);
        let crc = crc32(&bytes);
        bytes.extend_from_slice(&crc.to_le_bytes());
        bytes
    }

    #[test]
    fn rejects_bad_header_fields() {
        let bytes = with_body(encode(&sample()), |b| b[6] = 5);
        assert_eq!(decode(&bytes), Err(ModelError::BadOrder(5)));
        let bytes = with_body(encode(&sample()), |b| b[7..9].copy_from_slice(b"de"));
        assert_eq!(decode(&bytes), Err(ModelError::UnknownLanguage(*b"de")));
        let bytes = with_body(encode(&sample()), |b| b[7..9].copy_from_slice(b"en"));
        assert_eq!(decode(&bytes), Err(ModelError::BadAlphabet("en")));
        let bytes = with_body(encode(&sample()), |b| b.push(0));
        assert_eq!(decode(&bytes), Err(ModelError::Truncated));
        let bytes = with_body(encode(&sample()), |b| {
            b.pop();
        });
        assert_eq!(decode(&bytes), Err(ModelError::Truncated));
    }

    #[test]
    fn rejects_unsorted_tables() {
        let mut m = sample();
        m.ngrams.reverse();
        assert_eq!(decode(&encode(&m)), Err(ModelError::BadTable));
        let mut m = sample();
        m.ngrams.push((pack_key(&[60]), 1));
        assert_eq!(decode(&encode(&m)), Err(ModelError::BadTable));
    }

    #[test]
    fn huge_counts_do_not_allocate() {
        // n_ngrams claims u32::MAX entries; decoding must fail, not abort.
        let m = ModelData {
            ngrams: vec![],
            contexts: vec![],
            ..sample()
        };
        let bytes = with_body(encode(&m), |b| {
            let n = b.len();
            b[n - 8..n - 4].copy_from_slice(&u32::MAX.to_le_bytes());
        });
        assert_eq!(decode(&bytes), Err(ModelError::Truncated));
    }
}
