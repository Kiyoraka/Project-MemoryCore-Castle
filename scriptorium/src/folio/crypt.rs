//! Locked files: the standard security handler, revisions 2 to 6, written here with std only and used
//! only to read (and append to) a file the member already has.
//!
//! The primitives: MD5 (RFC 1321), RC4, SHA-256 / SHA-384 / SHA-512 (FIPS 180-4) and AES-128/256 in
//! CBC mode with PKCS#5 padding (FIPS 197, SP 800-38A). On top of them the key derivations of ISO
//! 32000: Algorithm 2 (RC4 and AES-128 files), 2.A and 2.B (AES-256, revisions 5 and 6), the user and
//! owner password checks, and the per-object keys (object number, generation and the AES salt).
//!
//! What is never encrypted stays as it is: the /Encrypt dictionary itself, cross-reference streams,
//! objects inside object streams (the stream around them is), the /Contents of a signature, metadata
//! streams when /EncryptMetadata is false, and streams that name the Identity crypt filter.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::obj::{Dict, Obj, Stream};

// ---------------------------------------------------------------------------------------------------
// MD5
// ---------------------------------------------------------------------------------------------------

/// MD5 of `data` (RFC 1321).
pub fn md5(data: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11,
        16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let mut k = [0u32; 64];
    for (i, v) in k.iter_mut().enumerate() {
        *v = ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32;
    }
    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_le_bytes());
    for chunk in msg.chunks(64) {
        let mut m = [0u32; 16];
        for (i, w) in m.iter_mut().enumerate() {
            *w = u32::from_le_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f2 = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f2.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

// ---------------------------------------------------------------------------------------------------
// RC4
// ---------------------------------------------------------------------------------------------------

/// RC4 over `data` with `key` (encrypting and decrypting are the same).
pub fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut s = [0u8; 256];
    for (i, v) in s.iter_mut().enumerate() {
        *v = i as u8;
    }
    if !key.is_empty() {
        let mut j: u8 = 0;
        for i in 0..256 {
            j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
            s.swap(i, j as usize);
        }
    }
    let (mut i, mut j) = (0u8, 0u8);
    data.iter()
        .map(|&b| {
            i = i.wrapping_add(1);
            j = j.wrapping_add(s[i as usize]);
            s.swap(i as usize, j as usize);
            b ^ s[s[i as usize].wrapping_add(s[j as usize]) as usize]
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------------
// SHA-2
// ---------------------------------------------------------------------------------------------------

/// The 80 SHA-512 round constants; SHA-256's 64 are the high halves of the first 64.
const K512: [u64; 80] = [
    0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc, 0x3956c25bf348b538, 0x59f111f1b605d019, 0x923f82a4af194f9b,
    0xab1c5ed5da6d8118, 0xd807aa98a3030242, 0x12835b0145706fbe, 0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2, 0x72be5d74f27b896f, 0x80deb1fe3b1696b1,
    0x9bdc06a725c71235, 0xc19bf174cf692694, 0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65, 0x2de92c6f592b0275,
    0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5, 0x983e5152ee66dfab, 0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2, 0xd5a79147930aa725, 0x06ca6351e003826f, 0x142929670a0e6e70, 0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df, 0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b, 0xa2bfe8a14cf10364, 0xa81a664bbc423001,
    0xc24b8b70d0f89791, 0xc76c51a30654be30, 0xd192e819d6ef5218, 0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8, 0x19a4c116b8d2d0c8,
    0x1e376c085141ab53, 0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8, 0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373, 0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec, 0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915,
    0xc67178f2e372532b, 0xca273eceea26619c, 0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178, 0x06f067aa72176fba, 0x0a637dc5a2c898a6,
    0x113f9804bef90dae, 0x1b710b35131c471b, 0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc, 0x431d67c49c100d4c, 0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
];

const IV512: [u64; 8] = [0x6a09e667f3bcc908, 0xbb67ae8584caa73b, 0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1, 0x510e527fade682d1, 0x9b05688c2b3e6c1f, 0x1f83d9abfb41bd6b, 0x5be0cd19137e2179];
const IV384: [u64; 8] = [0xcbbb9d5dc1059ed8, 0x629a292a367cd507, 0x9159015a3070dd17, 0x152fecd8f70e5939, 0x67332667ffc00b31, 0x8eb44a8768581511, 0xdb0c2e0d64f98fa7, 0x47b5481dbefa4fa4];

/// SHA-256 of `data`.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [0; 8];
    for (i, v) in h.iter_mut().enumerate() {
        *v = (IV512[i] >> 32) as u32;
    }
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    let mut w = [0u32; 64];
    for chunk in msg.chunks(64) {
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add((K512[i] >> 32) as u32).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    let mut out = [0u8; 32];
    for i in 0..8 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

fn sha512_core(data: &[u8], iv: [u64; 8]) -> [u8; 64] {
    let mut h = iv;
    let mut msg = data.to_vec();
    let bits = (data.len() as u128).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 128 != 112 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    let mut w = [0u64; 80];
    for chunk in msg.chunks(128) {
        for i in 0..16 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&chunk[i * 8..i * 8 + 8]);
            w[i] = u64::from_be_bytes(b);
        }
        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..80 {
            let s1 = v[4].rotate_right(14) ^ v[4].rotate_right(18) ^ v[4].rotate_right(41);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K512[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(28) ^ v[0].rotate_right(34) ^ v[0].rotate_right(39);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    let mut out = [0u8; 64];
    for i in 0..8 {
        out[i * 8..i * 8 + 8].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

/// SHA-512 of `data`.
pub fn sha512(data: &[u8]) -> [u8; 64] {
    sha512_core(data, IV512)
}

/// SHA-384 of `data`.
pub fn sha384(data: &[u8]) -> [u8; 48] {
    let full = sha512_core(data, IV384);
    let mut out = [0u8; 48];
    out.copy_from_slice(&full[..48]);
    out
}

// ---------------------------------------------------------------------------------------------------
// AES
// ---------------------------------------------------------------------------------------------------

struct Tables {
    sbox: [u8; 256],
    inv: [u8; 256],
}

fn tables() -> &'static Tables {
    static T: std::sync::OnceLock<Tables> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut sbox = [0u8; 256];
        let (mut p, mut q) = (1u8, 1u8);
        loop {
            // p times 3, q divided by 3, in GF(2^8)
            p = p ^ (p << 1) ^ if p & 0x80 != 0 { 0x1B } else { 0 };
            q ^= q << 1;
            q ^= q << 2;
            q ^= q << 4;
            if q & 0x80 != 0 {
                q ^= 0x09;
            }
            let x = q ^ q.rotate_left(1) ^ q.rotate_left(2) ^ q.rotate_left(3) ^ q.rotate_left(4);
            sbox[p as usize] = x ^ 0x63;
            if p == 1 {
                break;
            }
        }
        sbox[0] = 0x63;
        let mut inv = [0u8; 256];
        for (i, &s) in sbox.iter().enumerate() {
            inv[s as usize] = i as u8;
        }
        Tables { sbox, inv }
    })
}

fn xtime(b: u8) -> u8 {
    (b << 1) ^ if b & 0x80 != 0 { 0x1B } else { 0 }
}

fn gmul(mut a: u8, mut b: u8) -> u8 {
    let mut r = 0u8;
    while b != 0 {
        if b & 1 != 0 {
            r ^= a;
        }
        a = xtime(a);
        b >>= 1;
    }
    r
}

/// An AES key schedule (128, 192 or 256 bits).
pub struct Aes {
    round_keys: Vec<[u8; 16]>,
}

impl Aes {
    /// The schedule for a 16, 24 or 32 byte key.
    pub fn new(key: &[u8]) -> Option<Aes> {
        let nk = key.len() / 4;
        if !matches!(key.len(), 16 | 24 | 32) {
            return None;
        }
        let nr = nk + 6;
        let t = tables();
        let total = 4 * (nr + 1);
        let mut w: Vec<[u8; 4]> = Vec::with_capacity(total);
        for i in 0..nk {
            w.push([key[4 * i], key[4 * i + 1], key[4 * i + 2], key[4 * i + 3]]);
        }
        let mut rcon = 1u8;
        for i in nk..total {
            let mut temp = w[i - 1];
            if i % nk == 0 {
                temp = [t.sbox[temp[1] as usize] ^ rcon, t.sbox[temp[2] as usize], t.sbox[temp[3] as usize], t.sbox[temp[0] as usize]];
                rcon = xtime(rcon);
            } else if nk > 6 && i % nk == 4 {
                temp = [t.sbox[temp[0] as usize], t.sbox[temp[1] as usize], t.sbox[temp[2] as usize], t.sbox[temp[3] as usize]];
            }
            let prev = w[i - nk];
            w.push([prev[0] ^ temp[0], prev[1] ^ temp[1], prev[2] ^ temp[2], prev[3] ^ temp[3]]);
        }
        let round_keys = (0..=nr)
            .map(|r| {
                let mut k = [0u8; 16];
                for c in 0..4 {
                    k[c * 4..c * 4 + 4].copy_from_slice(&w[r * 4 + c]);
                }
                k
            })
            .collect();
        Some(Aes { round_keys })
    }

    fn add(state: &mut [u8; 16], k: &[u8; 16]) {
        for i in 0..16 {
            state[i] ^= k[i];
        }
    }

    /// Encrypt one block in place.
    pub fn encrypt_block(&self, s: &mut [u8; 16]) {
        let t = tables();
        let nr = self.round_keys.len() - 1;
        Self::add(s, &self.round_keys[0]);
        for round in 1..=nr {
            for b in s.iter_mut() {
                *b = t.sbox[*b as usize];
            }
            let old = *s;
            for r in 0..4 {
                for c in 0..4 {
                    s[r + 4 * c] = old[r + 4 * ((c + r) % 4)];
                }
            }
            if round != nr {
                for c in 0..4 {
                    let a = [s[4 * c], s[4 * c + 1], s[4 * c + 2], s[4 * c + 3]];
                    s[4 * c] = xtime(a[0]) ^ (xtime(a[1]) ^ a[1]) ^ a[2] ^ a[3];
                    s[4 * c + 1] = a[0] ^ xtime(a[1]) ^ (xtime(a[2]) ^ a[2]) ^ a[3];
                    s[4 * c + 2] = a[0] ^ a[1] ^ xtime(a[2]) ^ (xtime(a[3]) ^ a[3]);
                    s[4 * c + 3] = (xtime(a[0]) ^ a[0]) ^ a[1] ^ a[2] ^ xtime(a[3]);
                }
            }
            Self::add(s, &self.round_keys[round]);
        }
    }

    /// Decrypt one block in place.
    pub fn decrypt_block(&self, s: &mut [u8; 16]) {
        let t = tables();
        let nr = self.round_keys.len() - 1;
        Self::add(s, &self.round_keys[nr]);
        for round in (0..nr).rev() {
            let old = *s;
            for r in 0..4 {
                for c in 0..4 {
                    s[r + 4 * c] = old[r + 4 * ((c + 4 - r) % 4)];
                }
            }
            for b in s.iter_mut() {
                *b = t.inv[*b as usize];
            }
            Self::add(s, &self.round_keys[round]);
            if round != 0 {
                for c in 0..4 {
                    let a = [s[4 * c], s[4 * c + 1], s[4 * c + 2], s[4 * c + 3]];
                    s[4 * c] = gmul(a[0], 14) ^ gmul(a[1], 11) ^ gmul(a[2], 13) ^ gmul(a[3], 9);
                    s[4 * c + 1] = gmul(a[0], 9) ^ gmul(a[1], 14) ^ gmul(a[2], 11) ^ gmul(a[3], 13);
                    s[4 * c + 2] = gmul(a[0], 13) ^ gmul(a[1], 9) ^ gmul(a[2], 14) ^ gmul(a[3], 11);
                    s[4 * c + 3] = gmul(a[0], 11) ^ gmul(a[1], 13) ^ gmul(a[2], 9) ^ gmul(a[3], 14);
                }
            }
        }
    }
}

/// AES-CBC decryption. With `unpad`, a valid PKCS#5 tail is taken off (an invalid one is left, since
/// some writers forget it). `None` for a bad key; a partial last block is dropped.
pub fn aes_cbc_decrypt(key: &[u8], iv: &[u8; 16], data: &[u8], unpad: bool) -> Option<Vec<u8>> {
    let aes = Aes::new(key)?;
    let mut prev = *iv;
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks_exact(16) {
        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        let cipher = block;
        aes.decrypt_block(&mut block);
        for i in 0..16 {
            block[i] ^= prev[i];
        }
        out.extend_from_slice(&block);
        prev = cipher;
    }
    if unpad {
        if let Some(&n) = out.last() {
            let n = n as usize;
            if (1..=16).contains(&n) && n <= out.len() && out[out.len() - n..].iter().all(|&b| b as usize == n) {
                out.truncate(out.len() - n);
            }
        }
    }
    Some(out)
}

/// AES-CBC encryption; with `pad`, PKCS#5 padding is added (without it the data must be whole blocks,
/// a partial tail is zero-filled).
pub fn aes_cbc_encrypt(key: &[u8], iv: &[u8; 16], data: &[u8], pad: bool) -> Option<Vec<u8>> {
    let aes = Aes::new(key)?;
    let mut buf = data.to_vec();
    if pad {
        let n = 16 - buf.len() % 16;
        buf.extend(std::iter::repeat(n as u8).take(n));
    } else {
        while buf.len() % 16 != 0 {
            buf.push(0);
        }
    }
    let mut prev = *iv;
    let mut out = Vec::with_capacity(buf.len());
    for chunk in buf.chunks_exact(16) {
        let mut block = [0u8; 16];
        for i in 0..16 {
            block[i] = chunk[i] ^ prev[i];
        }
        aes.encrypt_block(&mut block);
        out.extend_from_slice(&block);
        prev = block;
    }
    Some(out)
}

// ---------------------------------------------------------------------------------------------------
// The standard security handler
// ---------------------------------------------------------------------------------------------------

/// The 32 padding bytes of Algorithm 2.
pub const PAD: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08, 0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80,
    0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

/// How strings or streams are locked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Not at all (the Identity crypt filter).
    Identity,
    Rc4,
    AesV2,
    AesV3,
}

impl Method {
    fn name(self) -> &'static str {
        match self {
            Method::Identity => "none",
            Method::Rc4 => "RC4",
            Method::AesV2 => "AES-128",
            Method::AesV3 => "AES-256",
        }
    }
}

/// Why a locked file did not open.
#[derive(Debug, Clone, PartialEq)]
pub enum UnlockError {
    /// The password (none, or the one given) does not open it.
    Password,
    /// A handler or method we do not read (certificates, an unknown crypt filter).
    Unsupported(String),
}

/// An unlocked file's keys and rules.
#[derive(Debug, Clone)]
pub struct Security {
    key: Vec<u8>,
    pub v: i64,
    pub r: i64,
    /// The permission bits (/P).
    pub p: i32,
    strings: Method,
    streams: Method,
    /// Named crypt filters a stream may ask for by /DecodeParms /Name.
    filters: Vec<(String, Method)>,
    encrypt_metadata: bool,
    /// Opened with the owner password (every permission granted).
    pub owner: bool,
    /// The /Encrypt dictionary's object number (never decrypted), 0 when direct.
    pub dict_num: u32,
}

/// Pad or cut a password to 32 bytes (Algorithm 2 step a).
fn pad32(pw: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let n = pw.len().min(32);
    out[..n].copy_from_slice(&pw[..n]);
    out[n..].copy_from_slice(&PAD[..32 - n]);
    out
}

fn xor_key(key: &[u8], i: u8) -> Vec<u8> {
    key.iter().map(|b| b ^ i).collect()
}

/// Algorithm 2: the file key from a (padded) user password, revisions 2 to 4.
fn key_r2_4(pw: &[u8], o: &[u8], p: i32, id0: &[u8], r: i64, n: usize, encrypt_metadata: bool) -> Vec<u8> {
    let mut buf = pad32(pw).to_vec();
    buf.extend_from_slice(&o[..o.len().min(32)]);
    buf.extend_from_slice(&p.to_le_bytes());
    buf.extend_from_slice(id0);
    if r >= 4 && !encrypt_metadata {
        buf.extend_from_slice(&[0xFF; 4]);
    }
    let mut h = md5(&buf);
    let n = n.min(16);
    if r >= 3 {
        for _ in 0..50 {
            h = md5(&h[..n]);
        }
    }
    h[..n].to_vec()
}

/// Algorithms 4 and 5: the /U value a file key gives.
fn u_value(key: &[u8], id0: &[u8], r: i64) -> Vec<u8> {
    if r == 2 {
        return rc4(key, &PAD);
    }
    let mut buf = PAD.to_vec();
    buf.extend_from_slice(id0);
    let mut x = rc4(key, &md5(&buf));
    for i in 1..=19u8 {
        x = rc4(&xor_key(key, i), &x);
    }
    x.extend_from_slice(&[0u8; 16]);
    x
}

/// Algorithm 3 steps a-d: the RC4 key the owner password makes.
fn owner_rc4_key(owner_pw: &[u8], r: i64, n: usize) -> Vec<u8> {
    let mut h = md5(&pad32(owner_pw));
    if r >= 3 {
        for _ in 0..50 {
            h = md5(&h);
        }
    }
    h[..n.min(16)].to_vec()
}

/// Algorithm 3: the /O value (revisions 2 to 4); only the tests' invented files need it.
#[cfg(test)]
pub fn o_value(owner_pw: &[u8], user_pw: &[u8], r: i64, n: usize) -> Vec<u8> {
    let owner = if owner_pw.is_empty() { user_pw } else { owner_pw };
    let key = owner_rc4_key(owner, r, if r == 2 { 5 } else { n });
    let mut x = rc4(&key, &pad32(user_pw));
    if r >= 3 {
        for i in 1..=19u8 {
            x = rc4(&xor_key(&key, i), &x);
        }
    }
    x
}

/// Algorithm 2.B (revision 6), or plain SHA-256 for revision 5: the hash of a password, a salt and
/// (for the owner) the 48 bytes of /U.
pub fn hash_2b(pw: &[u8], salt: &[u8], udata: &[u8], r: i64) -> [u8; 32] {
    let mut buf = pw.to_vec();
    buf.extend_from_slice(salt);
    buf.extend_from_slice(udata);
    let mut k: Vec<u8> = sha256(&buf).to_vec();
    if r < 6 {
        let mut out = [0u8; 32];
        out.copy_from_slice(&k[..32]);
        return out;
    }
    let mut round: u32 = 0;
    loop {
        let mut k1 = Vec::with_capacity(64 * (pw.len() + k.len() + udata.len()));
        for _ in 0..64 {
            k1.extend_from_slice(pw);
            k1.extend_from_slice(&k);
            k1.extend_from_slice(udata);
        }
        let mut iv = [0u8; 16];
        iv.copy_from_slice(&k[16..32]);
        let e = aes_cbc_encrypt(&k[..16], &iv, &k1, false).unwrap_or_default();
        let sum: u32 = e[..16].iter().map(|&b| b as u32).sum();
        k = match sum % 3 {
            0 => sha256(&e).to_vec(),
            1 => sha384(&e).to_vec(),
            _ => sha512(&e).to_vec(),
        };
        round += 1;
        let last = *e.last().unwrap_or(&0) as u32;
        if round >= 64 && last + 32 <= round {
            break;
        }
        if round > 2000 {
            break;
        }
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&k[..32]);
    out
}

fn bytes_of(o: &Obj) -> Vec<u8> {
    o.as_bytes().map(|b| b.to_vec()).unwrap_or_default()
}

/// A password as the handler wants it: Latin-1 bytes for revisions 2 to 4, UTF-8 (at most 127 bytes)
/// for 5 and 6.
fn password_bytes(pw: &str, r: i64) -> Vec<u8> {
    if r >= 5 {
        let b = pw.as_bytes();
        let mut n = b.len().min(127);
        while n > 0 && !pw.is_char_boundary(n) {
            n -= 1;
        }
        b[..n].to_vec()
    } else {
        pw.chars().map(|c| if (c as u32) < 256 { c as u32 as u8 } else { b'?' }).collect()
    }
}

/// Unlock a file: `encrypt` is the /Encrypt dictionary (its CF entries resolved by `resolve`), `id0`
/// the first /ID string. The empty password is tried first (as user, then as owner); then `password`.
pub fn unlock(encrypt: &Dict, dict_num: u32, id0: &[u8], password: Option<&str>, resolve: &dyn Fn(&Obj) -> Obj) -> Result<Security, UnlockError> {
    let filter = encrypt.name("Filter").unwrap_or("Standard");
    if filter != "Standard" {
        return Err(UnlockError::Unsupported(format!("it is locked with the '{}' handler (certificates), which Folio does not read", filter)));
    }
    let num = |k: &str| resolve(encrypt.get(k).unwrap_or(&Obj::Null)).as_i64();
    let v = num("V").unwrap_or(0);
    let r = num("R").unwrap_or(2);
    let p = num("P").unwrap_or(-1) as i32;
    let length_bits = num("Length").unwrap_or(40).clamp(40, 256);
    let o = bytes_of(&resolve(encrypt.get("O").unwrap_or(&Obj::Null)));
    let u = bytes_of(&resolve(encrypt.get("U").unwrap_or(&Obj::Null)));
    let encrypt_metadata = resolve(encrypt.get("EncryptMetadata").unwrap_or(&Obj::Null)).as_bool().unwrap_or(true);
    if !(2..=6).contains(&r) {
        return Err(UnlockError::Unsupported(format!("it uses security revision {}, which Folio does not read", r)));
    }
    // The methods: V1/V2 are RC4 everywhere; V4/V5 name crypt filters.
    let mut filters: Vec<(String, Method)> = vec![("Identity".into(), Method::Identity)];
    let mut n = if v == 1 || r == 2 { 5 } else { (length_bits / 8) as usize };
    let (strings, streams) = match v {
        1 | 2 | 3 => (Method::Rc4, Method::Rc4),
        4 | 5 => {
            let cf = resolve(encrypt.get("CF").unwrap_or(&Obj::Null));
            if let Some(cf) = cf.as_dict() {
                for (name, f) in &cf.0 {
                    let f = resolve(f);
                    let Some(fd) = f.as_dict() else { continue };
                    let m = match fd.name("CFM") {
                        Some("V2") => Method::Rc4,
                        Some("AESV2") => Method::AesV2,
                        Some("AESV3") => Method::AesV3,
                        Some("None") | None => Method::Identity,
                        Some(other) => return Err(UnlockError::Unsupported(format!("it uses the '{}' crypt method, which Folio does not read", other))),
                    };
                    if m == Method::Rc4 {
                        if let Some(len) = resolve(fd.get("Length").unwrap_or(&Obj::Null)).as_i64() {
                            // Length is in bytes here (some writers put bits).
                            let bytes = if len > 32 { len / 8 } else { len };
                            n = bytes.clamp(5, 16) as usize;
                        }
                    }
                    filters.push((name.clone(), m));
                }
            }
            let pick = |key: &str| -> Result<Method, UnlockError> {
                let name = encrypt.name(key).unwrap_or("Identity");
                filters.iter().find(|(n, _)| n == name).map(|(_, m)| *m).ok_or_else(|| UnlockError::Unsupported(format!("its crypt filter '{}' is missing", name)))
            };
            (pick("StrF")?, pick("StmF")?)
        }
        _ => return Err(UnlockError::Unsupported(format!("it uses encryption version {}, which Folio does not read", v))),
    };
    if strings == Method::AesV2 || streams == Method::AesV2 {
        n = 16;
    }
    let mut sec = Security { key: Vec::new(), v, r, p, strings, streams, filters, encrypt_metadata, owner: false, dict_num };
    // The password given first (so an owner's password grants the owner's rights), then the empty one.
    let mut tries: Vec<&str> = Vec::new();
    if let Some(pw) = password {
        if !pw.is_empty() {
            tries.push(pw);
        }
    }
    tries.push("");
    for pw in tries {
        let pwb = password_bytes(pw, r);
        if r >= 5 {
            if u.len() < 48 || o.len() < 48 {
                return Err(UnlockError::Unsupported("its /O or /U entry is too short".into()));
            }
            let ue = bytes_of(&resolve(encrypt.get("UE").unwrap_or(&Obj::Null)));
            let oe = bytes_of(&resolve(encrypt.get("OE").unwrap_or(&Obj::Null)));
            let zero = [0u8; 16];
            if hash_2b(&pwb, &o[32..40], &u[..48], r)[..] == o[..32] {
                let k = hash_2b(&pwb, &o[40..48], &u[..48], r);
                if let Some(key) = aes_cbc_decrypt(&k, &zero, &oe[..oe.len().min(32)], false) {
                    if key.len() == 32 {
                        sec.key = key;
                        sec.owner = true;
                        return Ok(sec);
                    }
                }
            }
            if hash_2b(&pwb, &u[32..40], &[], r)[..] == u[..32] {
                let k = hash_2b(&pwb, &u[40..48], &[], r);
                if let Some(key) = aes_cbc_decrypt(&k, &zero, &ue[..ue.len().min(32)], false) {
                    if key.len() == 32 {
                        sec.key = key;
                        return Ok(sec);
                    }
                }
            }
            continue;
        }
        if o.len() < 32 || u.len() < 16 {
            return Err(UnlockError::Unsupported("its /O or /U entry is too short".into()));
        }
        let check = |user_pw: &[u8]| -> Option<Vec<u8>> {
            let key = key_r2_4(user_pw, &o, p, id0, r, n, encrypt_metadata);
            let want = u_value(&key, id0, r);
            let cmp = if r == 2 { 32 } else { 16 };
            (u.len() >= cmp && want[..cmp] == u[..cmp]).then_some(key)
        };
        // As the owner: the owner key opens /O to the user password.
        let ok = owner_rc4_key(&pwb, r, n);
        let user_pw = if r == 2 {
            rc4(&ok, &o[..32])
        } else {
            let mut x = o[..32].to_vec();
            for i in (0..=19u8).rev() {
                x = rc4(&xor_key(&ok, i), &x);
            }
            x
        };
        if let Some(key) = check(&user_pw) {
            sec.key = key;
            sec.owner = true;
            return Ok(sec);
        }
        if let Some(key) = check(&pwb) {
            sec.key = key;
            return Ok(sec);
        }
    }
    Err(UnlockError::Password)
}

/// A fresh initialisation vector (not secret, only unique).
fn fresh_iv(num: u32, gen: u16) -> [u8; 16] {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut buf = t.to_le_bytes().to_vec();
    buf.extend_from_slice(&COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    buf.extend_from_slice(&num.to_le_bytes());
    buf.extend_from_slice(&gen.to_le_bytes());
    md5(&buf)
}

impl Security {
    /// Algorithm 1: the key for one object.
    fn object_key(&self, num: u32, gen: u16, m: Method) -> Vec<u8> {
        if m == Method::AesV3 {
            return self.key.clone();
        }
        let mut buf = self.key.clone();
        buf.extend_from_slice(&num.to_le_bytes()[..3]);
        buf.extend_from_slice(&gen.to_le_bytes());
        if m == Method::AesV2 {
            buf.extend_from_slice(b"sAlT");
        }
        let h = md5(&buf);
        h[..(self.key.len() + 5).min(16)].to_vec()
    }

    fn decrypt(&self, num: u32, gen: u16, m: Method, data: &[u8]) -> Vec<u8> {
        match m {
            Method::Identity => data.to_vec(),
            Method::Rc4 => rc4(&self.object_key(num, gen, m), data),
            Method::AesV2 | Method::AesV3 => {
                if data.len() < 16 {
                    return Vec::new();
                }
                let mut iv = [0u8; 16];
                iv.copy_from_slice(&data[..16]);
                aes_cbc_decrypt(&self.object_key(num, gen, m), &iv, &data[16..], true).unwrap_or_default()
            }
        }
    }

    fn encrypt(&self, num: u32, gen: u16, m: Method, data: &[u8]) -> Vec<u8> {
        match m {
            Method::Identity => data.to_vec(),
            Method::Rc4 => rc4(&self.object_key(num, gen, m), data),
            Method::AesV2 | Method::AesV3 => {
                let iv = fresh_iv(num, gen);
                let mut out = iv.to_vec();
                out.extend(aes_cbc_encrypt(&self.object_key(num, gen, m), &iv, data, true).unwrap_or_default());
                out
            }
        }
    }

    /// The method a stream is locked with (none for cross-reference streams, metadata left in the
    /// clear, and the Identity crypt filter).
    fn stream_method(&self, d: &Dict, resolve: &dyn Fn(&Obj) -> Obj) -> Method {
        if d.is_type("XRef") {
            return Method::Identity;
        }
        if d.is_type("Metadata") && !self.encrypt_metadata {
            return Method::Identity;
        }
        let filters = resolve(d.get("Filter").unwrap_or(&Obj::Null));
        let has_crypt = match &filters {
            Obj::Name(n) => n == "Crypt",
            Obj::Arr(a) => a.iter().any(|f| f.as_name() == Some("Crypt")),
            _ => false,
        };
        if has_crypt {
            let parms = resolve(d.get("DecodeParms").unwrap_or(&Obj::Null));
            let name = match &parms {
                Obj::Dict(p) => p.name("Name").map(str::to_string),
                Obj::Arr(a) => a.iter().filter_map(|p| resolve(p).as_dict().and_then(|p| p.name("Name").map(str::to_string))).next(),
                _ => None,
            }
            .unwrap_or_else(|| "Identity".into());
            return self.filters.iter().find(|(n, _)| *n == name).map(|(_, m)| *m).unwrap_or(Method::Identity);
        }
        self.streams
    }

    /// An object as read from the file, with its strings and stream data unlocked.
    pub fn decrypt_obj(&self, o: Obj, num: u32, gen: u16, resolve: &dyn Fn(&Obj) -> Obj) -> Obj {
        if num == self.dict_num && num != 0 {
            return o;
        }
        self.walk(o, num, gen, resolve, false, 0)
    }

    /// An object as it is written back: strings and stream data locked again with the same key.
    pub fn encrypt_obj(&self, o: &Obj, num: u32, gen: u16, resolve: &dyn Fn(&Obj) -> Obj) -> Obj {
        if num == self.dict_num && num != 0 {
            return o.clone();
        }
        self.walk(o.clone(), num, gen, resolve, true, 0)
    }

    fn walk(&self, o: Obj, num: u32, gen: u16, resolve: &dyn Fn(&Obj) -> Obj, enc: bool, depth: u32) -> Obj {
        if depth > 64 {
            return o;
        }
        let run = |m: Method, data: &[u8]| if enc { self.encrypt(num, gen, m, data) } else { self.decrypt(num, gen, m, data) };
        match o {
            Obj::Str(s) => Obj::Str(run(self.strings, &s)),
            Obj::Arr(a) => Obj::Arr(a.into_iter().map(|v| self.walk(v, num, gen, resolve, enc, depth + 1)).collect()),
            Obj::Dict(d) => Obj::Dict(self.walk_dict(d, num, gen, resolve, enc, depth)),
            Obj::Stream(s) => {
                let m = self.stream_method(&s.dict, resolve);
                let dict = self.walk_dict(s.dict.clone(), num, gen, resolve, enc, depth);
                let data = run(m, &s.data);
                Obj::Stream(Arc::new(Stream { dict, data }))
            }
            other => other,
        }
    }

    fn walk_dict(&self, d: Dict, num: u32, gen: u16, resolve: &dyn Fn(&Obj) -> Obj, enc: bool, depth: u32) -> Dict {
        // A signature's /Contents is never encrypted.
        let is_sig = d.is_type("Sig") || d.is_type("DocTimeStamp") || d.contains("ByteRange");
        Dict(d.0.into_iter().map(|(k, v)| if is_sig && k == "Contents" { (k, v) } else { (k, self.walk(v, num, gen, resolve, enc, depth + 1)) }).collect())
    }

    /// True when the permission bit (1-based, as ISO 32000 numbers them) is granted.
    pub fn permits(&self, bit: u32) -> bool {
        self.owner || (self.p as u32) & (1 << (bit - 1)) != 0
    }

    /// The method's plain name ("RC4", "AES-128", "AES-256").
    pub fn method_name(&self) -> &'static str {
        if self.streams != Method::Identity {
            self.streams.name()
        } else {
            self.strings.name()
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// Making locked values (for the tests' invented files)
// ---------------------------------------------------------------------------------------------------

/// The /O and /U values and the file key for a revision 2 to 4 file.
#[cfg(test)]
pub fn make_r2_4(user_pw: &str, owner_pw: &str, p: i32, id0: &[u8], r: i64, n: usize, encrypt_metadata: bool) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let upw = password_bytes(user_pw, r);
    let opw = password_bytes(owner_pw, r);
    let n = if r == 2 { 5 } else { n };
    let o = o_value(&opw, &upw, r, n);
    let key = key_r2_4(&upw, &o, p, id0, r, n, encrypt_metadata);
    let u = u_value(&key, id0, r);
    (o, u, key)
}

/// The /O, /U, /OE, /UE and /Perms values for a revision 6 file locked with `key` (32 bytes).
#[cfg(test)]
pub fn make_r6(user_pw: &str, owner_pw: &str, p: i32, key: &[u8; 32]) -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
    let zero = [0u8; 16];
    let upw = password_bytes(user_pw, 6);
    let opw = password_bytes(owner_pw, 6);
    let (uvs, uks) = ([1u8, 2, 3, 4, 5, 6, 7, 8], [9u8, 10, 11, 12, 13, 14, 15, 16]);
    let mut u = hash_2b(&upw, &uvs, &[], 6).to_vec();
    u.extend_from_slice(&uvs);
    u.extend_from_slice(&uks);
    let ue = aes_cbc_encrypt(&hash_2b(&upw, &uks, &[], 6), &zero, key, false).unwrap();
    let (ovs, oks) = ([17u8, 18, 19, 20, 21, 22, 23, 24], [25u8, 26, 27, 28, 29, 30, 31, 32]);
    let mut o = hash_2b(&opw, &ovs, &u, 6).to_vec();
    o.extend_from_slice(&ovs);
    o.extend_from_slice(&oks);
    let oe = aes_cbc_encrypt(&hash_2b(&opw, &oks, &u, 6), &zero, key, false).unwrap();
    let mut perms = p.to_le_bytes().to_vec();
    perms.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, b'T', b'a', b'd', b'b', 0, 0, 0, 0]);
    let perms = aes_cbc_encrypt(key, &zero, &perms, false).unwrap();
    (o, u, oe, ue, perms)
}

/// Encrypt with a file key the way a writer would (for the tests' invented files).
#[cfg(test)]
pub fn lock_with(key: &[u8], m: Method, num: u32, gen: u16, data: &[u8]) -> Vec<u8> {
    let s = Security { key: key.to_vec(), v: 4, r: 4, p: -1, strings: m, streams: m, filters: Vec::new(), encrypt_metadata: true, owner: false, dict_num: 0 };
    s.encrypt(num, gen, m, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{:02x}", x)).collect()
    }

    fn unhex(s: &str) -> Vec<u8> {
        let s: Vec<u8> = s.bytes().filter(|c| c.is_ascii_hexdigit()).collect();
        s.chunks(2).map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap()).collect()
    }

    #[test]
    fn md5_rfc_1321_suite() {
        let cases = [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            ("abcdefghijklmnopqrstuvwxyz", "c3fcd3d76192e4007dfb496cca67e13b"),
            ("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789", "d174ab98d277d9f5a5611c2c9f419d9f"),
            ("12345678901234567890123456789012345678901234567890123456789012345678901234567890", "57edf4a22be3c955ac49da2e2107b67a"),
        ];
        for (input, want) in cases {
            assert_eq!(hex(&md5(input.as_bytes())), want, "md5({:?})", input);
        }
    }

    #[test]
    fn rc4_known_vectors() {
        assert_eq!(hex(&rc4(b"Key", b"Plaintext")), "bbf316e8d940af0ad3");
        assert_eq!(hex(&rc4(b"Wiki", b"pedia")), "1021bf0420");
        assert_eq!(hex(&rc4(b"Secret", b"Attack at dawn")), "45a01f645fc35b383552544b9bf5");
        // RFC 6229, the 40-bit key 0x0102030405: the first 16 keystream bytes.
        assert_eq!(hex(&rc4(&[1, 2, 3, 4, 5], &[0u8; 16])), "b2396305f03dc027ccc3524a0a1118a8");
        assert_eq!(rc4(b"k", &rc4(b"k", b"round trip")), b"round trip");
    }

    #[test]
    fn sha2_fips_180_vectors() {
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
        assert_eq!(hex(&sha384(b"abc")), "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7");
        assert_eq!(
            hex(&sha384(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu")),
            "09330c33f71147e83d192fc782cd1b4753111b173b3b05d22fa08086e3b0f712fcc7c71a557e2db966c3e9fa91746039"
        );
        assert_eq!(
            hex(&sha512(b"abc")),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        assert_eq!(
            hex(&sha512(b"")),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
        assert_eq!(
            hex(&sha512(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu")),
            "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909"
        );
        // A million 'a's crosses many blocks.
        let million = vec![b'a'; 1_000_000];
        assert_eq!(hex(&sha256(&million)), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    }

    #[test]
    fn aes_fips_197_blocks() {
        let pt: [u8; 16] = unhex("00112233445566778899aabbccddeeff").try_into().unwrap();
        for (key, want) in [
            ("000102030405060708090a0b0c0d0e0f", "69c4e0d86a7b0430d8cdb78070b4c55a"),
            ("000102030405060708090a0b0c0d0e0f1011121314151617", "dda97ca4864cdfe06eaf70a0ec0d7191"),
            ("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f", "8ea2b7ca516745bfeafc49904b496089"),
        ] {
            let aes = Aes::new(&unhex(key)).unwrap();
            let mut b = pt;
            aes.encrypt_block(&mut b);
            assert_eq!(hex(&b), want, "key {}", key);
            aes.decrypt_block(&mut b);
            assert_eq!(b, pt);
        }
        assert!(Aes::new(&[0u8; 15]).is_none());
    }

    #[test]
    fn aes_cbc_sp_800_38a() {
        let iv: [u8; 16] = unhex("000102030405060708090a0b0c0d0e0f").try_into().unwrap();
        let pt = unhex("6bc1bee22e409f96e93d7e117393172a ae2d8a571e03ac9c9eb76fac45af8e51 30c81c46a35ce411e5fbc1191a0a52ef f69f2445df4f9b17ad2b417be66c3710");
        let k128 = unhex("2b7e151628aed2a6abf7158809cf4f3c");
        let c128 = "7649abac8119b246cee98e9b12e9197d5086cb9b507219ee95db113a917678b273bed6b8e3c1743b7116e69e222295163ff1caa1681fac09120eca307586e1a7";
        assert_eq!(hex(&aes_cbc_encrypt(&k128, &iv, &pt, false).unwrap()), c128);
        assert_eq!(aes_cbc_decrypt(&k128, &iv, &unhex(c128), false).unwrap(), pt);
        let k256 = unhex("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4");
        let c256 = "f58c4c04d6e5f1ba779eabfb5f7bfbd69cfc4e967edb808d679f777bc6702c7d39f23369a9d9bacfa530e26304231461b2eb05e2c39be9fcda6c19078c6a9d1b";
        assert_eq!(hex(&aes_cbc_encrypt(&k256, &iv, &pt, false).unwrap()), c256);
        assert_eq!(aes_cbc_decrypt(&k256, &iv, &unhex(c256), false).unwrap(), pt);
        // PKCS#5: padding on, padding off.
        for len in [0usize, 1, 15, 16, 17, 40] {
            let data: Vec<u8> = (0..len as u8).collect();
            let c = aes_cbc_encrypt(&k128, &iv, &data, true).unwrap();
            assert_eq!(c.len() % 16, 0);
            assert!(c.len() > len);
            assert_eq!(aes_cbc_decrypt(&k128, &iv, &c, true).unwrap(), data);
        }
    }

    fn resolve(o: &Obj) -> Obj {
        o.clone()
    }

    fn encrypt_dict(v: i64, r: i64, len: i64, o: &[u8], u: &[u8], p: i32, cfm: Option<&str>) -> Dict {
        let mut d = Dict::new()
            .with("Filter", Obj::name("Standard"))
            .with("V", Obj::Int(v))
            .with("R", Obj::Int(r))
            .with("Length", Obj::Int(len))
            .with("O", Obj::Str(o.to_vec()))
            .with("U", Obj::Str(u.to_vec()))
            .with("P", Obj::Int(p as i64));
        if let Some(cfm) = cfm {
            d.set("CF", Obj::Dict(Dict::new().with("StdCF", Obj::Dict(Dict::new().with("CFM", Obj::name(cfm)).with("Length", Obj::Int(16))))));
            d.set("StmF", Obj::name("StdCF"));
            d.set("StrF", Obj::name("StdCF"));
        }
        d
    }

    #[test]
    fn passwords_open_revisions_2_to_4() {
        let id0 = b"0123456789abcdef";
        for (v, r, len, cfm) in [(1, 2, 40, None), (2, 3, 128, None), (4, 4, 128, Some("V2")), (4, 4, 128, Some("AESV2"))] {
            let (o, u, key) = make_r2_4("", "boss", -8, id0, r, (len / 8) as usize, true);
            let d = encrypt_dict(v, r, len, &o, &u, -8, cfm);
            let s = unlock(&d, 0, id0, None, &resolve).unwrap_or_else(|e| panic!("R{} empty user password: {:?}", r, e));
            assert_eq!(s.key, key);
            assert!(!s.owner);
            assert!(!s.permits(3), "print is off in -8");
            let s = unlock(&d, 0, id0, Some("boss"), &resolve).unwrap();
            assert!(s.owner && s.key == key, "the owner's password grants the owner's rights");

            // A real user password: refused without it, opened with it or with the owner's.
            let (o, u, key) = make_r2_4("open sesame", "boss", -4, id0, r, (len / 8) as usize, true);
            let d = encrypt_dict(v, r, len, &o, &u, -4, cfm);
            assert_eq!(unlock(&d, 0, id0, None, &resolve).unwrap_err(), UnlockError::Password);
            assert_eq!(unlock(&d, 0, id0, Some("wrong"), &resolve).unwrap_err(), UnlockError::Password);
            let s = unlock(&d, 0, id0, Some("open sesame"), &resolve).unwrap();
            assert_eq!((s.key.clone(), s.owner), (key.clone(), false));
            let s = unlock(&d, 0, id0, Some("boss"), &resolve).unwrap();
            assert_eq!((s.key.clone(), s.owner), (key, true));
            assert!(s.permits(3), "the owner may do everything");
        }
    }

    #[test]
    fn passwords_open_revision_6_and_5() {
        let key = [7u8; 32];
        let (o, u, oe, ue, perms) = make_r6("user", "owner", -1028, &key);
        let mut d = encrypt_dict(5, 6, 256, &o, &u, -1028, Some("AESV3"));
        d.set("OE", Obj::Str(oe));
        d.set("UE", Obj::Str(ue));
        d.set("Perms", Obj::Str(perms));
        assert_eq!(unlock(&d, 0, b"", None, &resolve).unwrap_err(), UnlockError::Password);
        let s = unlock(&d, 0, b"", Some("user"), &resolve).unwrap();
        assert_eq!((s.key.as_slice(), s.owner), (&key[..], false));
        let s = unlock(&d, 0, b"", Some("owner"), &resolve).unwrap();
        assert_eq!((s.key.as_slice(), s.owner), (&key[..], true));
        assert_eq!(s.method_name(), "AES-256");

        // Revision 5 is plain SHA-256.
        let zero = [0u8; 16];
        let mut u5 = hash_2b(b"", &[1; 8], &[], 5).to_vec();
        u5.extend_from_slice(&[1; 8]);
        u5.extend_from_slice(&[2; 8]);
        let ue5 = aes_cbc_encrypt(&hash_2b(b"", &[2; 8], &[], 5), &zero, &key, false).unwrap();
        let mut d5 = encrypt_dict(5, 5, 256, &[0u8; 48], &u5, -1, Some("AESV3"));
        d5.set("UE", Obj::Str(ue5));
        d5.set("OE", Obj::Str(vec![0; 32]));
        let s = unlock(&d5, 0, b"", None, &resolve).unwrap();
        assert_eq!(s.key.as_slice(), &key[..]);
    }

    #[test]
    fn objects_lock_and_unlock_per_object() {
        let id0 = b"id";
        for cfm in [None, Some("AESV2"), Some("AESV3")] {
            let (d, key) = if cfm == Some("AESV3") {
                let key = [3u8; 32];
                let (o, u, oe, ue, perms) = make_r6("", "x", -1, &key);
                let mut d = encrypt_dict(5, 6, 256, &o, &u, -1, cfm);
                d.set("OE", Obj::Str(oe));
                d.set("UE", Obj::Str(ue));
                d.set("Perms", Obj::Str(perms));
                (d, key.to_vec())
            } else {
                let (v, r) = if cfm.is_some() { (4, 4) } else { (2, 3) };
                let (o, u, key) = make_r2_4("", "x", -1, id0, r, 16, true);
                (encrypt_dict(v, r, 128, &o, &u, -1, cfm), key)
            };
            let s = unlock(&d, 9, id0, None, &resolve).unwrap();
            assert_eq!(s.key, key);
            let plain = Obj::Dict(
                Dict::new()
                    .with("T", Obj::Str(b"Name".to_vec()))
                    .with("Kids", Obj::Arr(vec![Obj::Str(b"a".to_vec()), Obj::Int(3)]))
                    .with("Sig", Obj::Dict(Dict::new().with("Type", Obj::name("Sig")).with("Contents", Obj::Str(vec![1, 2, 3])))),
            );
            let locked = s.encrypt_obj(&plain, 12, 0, &resolve);
            assert_ne!(locked, plain);
            let sig = locked.as_dict().unwrap().get("Sig").unwrap().as_dict().unwrap().get("Contents").cloned();
            assert_eq!(sig, Some(Obj::Str(vec![1, 2, 3])), "a signature's contents stay clear");
            assert_eq!(s.decrypt_obj(locked.clone(), 12, 0, &resolve), plain);
            if cfm != Some("AESV3") {
                assert_ne!(s.decrypt_obj(locked, 13, 0, &resolve), plain, "the object number is in the key");
            }
            let st = Obj::Stream(Arc::new(Stream { dict: Dict::new(), data: b"BT (x) Tj ET".to_vec() }));
            let ls = s.encrypt_obj(&st, 4, 0, &resolve);
            assert_eq!(s.decrypt_obj(ls, 4, 0, &resolve), st);
            // Cross-reference streams and the encrypt dictionary itself are never touched.
            let xr = Obj::Stream(Arc::new(Stream { dict: Dict::new().with("Type", Obj::name("XRef")), data: vec![1, 2, 3] }));
            assert_eq!(s.decrypt_obj(xr.clone(), 4, 0, &resolve), xr);
            assert_eq!(s.decrypt_obj(plain.clone(), 9, 0, &resolve), plain);
        }
    }

    #[test]
    fn unsupported_handlers_say_so() {
        let d = Dict::new().with("Filter", Obj::name("Adobe.PubSec")).with("V", Obj::Int(4));
        assert!(matches!(unlock(&d, 0, b"", None, &resolve), Err(UnlockError::Unsupported(m)) if m.contains("certificates")));
        let d = Dict::new().with("Filter", Obj::name("Standard")).with("V", Obj::Int(2)).with("R", Obj::Int(9));
        assert!(matches!(unlock(&d, 0, b"", None, &resolve), Err(UnlockError::Unsupported(_))));
    }
}
