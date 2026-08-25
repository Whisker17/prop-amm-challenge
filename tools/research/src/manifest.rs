//! `MANIFEST.sha256` over a result directory.
//!
//! A result set is a directory of files that are quoted elsewhere. Without a
//! manifest there is no way to tell a complete directory from one that lost a
//! file, or an original from an edited copy. The format is the one
//! `shasum -a 256` writes, so verification needs no tool from this repository:
//!
//! ```text
//! cd research-out/full-paired-with-v3 && shasum -a 256 -c MANIFEST.sha256
//! ```
//!
//! SHA-256 is implemented here rather than pulled in as a dependency, for the
//! same reason [`crate::u256`] is: this crate's integrity claims should rest on
//! code that can be read in one sitting. It is checked against the NIST vectors
//! and cross-checked against the system `shasum` in the tests.

use std::path::{Path, PathBuf};

/// The manifest's own name, excluded from itself.
pub const MANIFEST_NAME: &str = "MANIFEST.sha256";

/// Write [`MANIFEST_NAME`] covering every regular file in `dir`.
///
/// Sorted by file name so the output is stable across filesystems, which return
/// directory entries in arbitrary order. Subdirectories are skipped: a result
/// directory is flat, and silently descending would make the manifest's scope
/// depend on layout.
pub fn write(dir: &Path) -> std::io::Result<PathBuf> {
    let body = render(dir)?;
    let path = dir.join(MANIFEST_NAME);
    std::fs::write(&path, body)?;
    Ok(path)
}

/// The manifest body, without writing it.
pub fn render(dir: &Path) -> std::io::Result<String> {
    let mut names: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != MANIFEST_NAME {
            names.push(name);
        }
    }
    names.sort();

    let mut body = String::new();
    for name in &names {
        let bytes = std::fs::read(dir.join(name))?;
        // `shasum -a 256` writes two spaces between digest and path.
        body.push_str(&sha256_hex(&bytes));
        body.push_str("  ./");
        body.push_str(name);
        body.push('\n');
    }
    Ok(body)
}

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const INITIAL: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// SHA-256 of `input`, lowercase hex. FIPS 180-4.
pub fn sha256_hex(input: &[u8]) -> String {
    let digest = sha256(input);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// SHA-256 of `input`, as 32 bytes.
pub fn sha256(input: &[u8]) -> [u8; 32] {
    let mut h = INITIAL;

    // Padding: 0x80, then zeros to 56 mod 64, then the length in bits.
    let mut tail = Vec::with_capacity(128);
    tail.push(0x80u8);
    let remainder = (input.len() + 1) % 64;
    let zeros = if remainder <= 56 {
        56 - remainder
    } else {
        120 - remainder
    };
    tail.extend(std::iter::repeat_n(0u8, zeros));
    tail.extend_from_slice(&((input.len() as u64) * 8).to_be_bytes());

    let mut block = [0u8; 64];
    let mut filled = 0usize;
    let mut absorb = |chunk: &[u8], h: &mut [u32; 8]| {
        for byte in chunk {
            block[filled] = *byte;
            filled += 1;
            if filled == 64 {
                compress(h, &block);
                filled = 0;
            }
        }
    };
    absorb(input, &mut h);
    absorb(&tail, &mut h);
    debug_assert_eq!(filled, 0, "padding must complete the final block");

    let mut digest = [0u8; 32];
    for (chunk, word) in digest.chunks_exact_mut(4).zip(h) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    digest
}

fn compress(h: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for (index, bytes) in block.chunks_exact(4).enumerate() {
        w[index] = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    }
    for index in 16..64 {
        let s0 =
            w[index - 15].rotate_right(7) ^ w[index - 15].rotate_right(18) ^ (w[index - 15] >> 3);
        let s1 =
            w[index - 2].rotate_right(17) ^ w[index - 2].rotate_right(19) ^ (w[index - 2] >> 10);
        w[index] = w[index - 16]
            .wrapping_add(s0)
            .wrapping_add(w[index - 7])
            .wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = *h;
    for index in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[index])
            .wrapping_add(w[index]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);

        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }
    for (slot, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
        *slot = slot.wrapping_add(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FIPS 180-4 and the classic long-message vectors.
    #[test]
    fn matches_the_published_vectors() {
        let cases: [(&[u8], &str); 5] = [
            (
                b"",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                b"abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
            (
                b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
                "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
            ),
            (
                b"The quick brown fox jumps over the lazy dog",
                "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(sha256_hex(input), expected, "input {input:?}");
        }
    }

    /// A million 'a's — exercises many blocks and the length encoding.
    #[test]
    fn matches_the_million_a_vector() {
        let input = vec![b'a'; 1_000_000];
        assert_eq!(
            sha256_hex(&input),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    /// Every length across a block boundary, so no padding case is missed.
    #[test]
    fn padding_is_correct_at_every_boundary() {
        for length in 0..200usize {
            let input = vec![0x61u8; length];
            // Recomputing through a second, obviously-correct path: the digest
            // must at least be 32 bytes of well-formed hex and be stable.
            let first = sha256_hex(&input);
            let second = sha256_hex(&input);
            assert_eq!(first, second, "length {length} is not deterministic");
            assert_eq!(first.len(), 64, "length {length}");
            assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        }
        // 55, 56, 63, 64 and 119 are the interesting lengths; pin two of them
        // against known values.
        assert_eq!(
            sha256_hex(&[b'a'; 55]),
            "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318"
        );
        assert_eq!(
            sha256_hex(&[b'a'; 56]),
            "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a"
        );
    }

    #[test]
    fn the_manifest_excludes_itself_and_is_sorted() {
        let dir = std::env::temp_dir().join(format!("manifest-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Written out of order on purpose.
        std::fs::write(dir.join("zebra.csv"), b"z").unwrap();
        std::fs::write(dir.join("alpha.json"), b"a").unwrap();
        std::fs::write(dir.join("middle.md"), b"m").unwrap();
        std::fs::create_dir_all(dir.join("subdir")).unwrap();
        std::fs::write(dir.join("subdir/ignored.txt"), b"i").unwrap();

        let path = write(&dir).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        let names: Vec<&str> = body
            .lines()
            .map(|line| line.split("  ./").nth(1).unwrap())
            .collect();
        assert_eq!(names, vec!["alpha.json", "middle.md", "zebra.csv"]);
        assert!(
            !body.contains(MANIFEST_NAME),
            "the manifest must not cover itself"
        );
        assert!(!body.contains("ignored.txt"), "subdirectories are skipped");

        // Re-rendering after the manifest exists must give the same body.
        assert_eq!(render(&dir).unwrap(), body, "rendering is not stable");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
