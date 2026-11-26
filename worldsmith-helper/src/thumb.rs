//! `thumb`: a tiny greyscale fingerprint of the game's window - 32x18 cells, one hex nibble per
//! cell, row-major - so a caller can tell "the same screen" from "a different screen" cheaply.
//! Pure code, tested on any OS.

// only the Windows capture calls encode
#![cfg_attr(not(windows), allow(dead_code))]

pub const W: usize = 32;
pub const H: usize = 18;

/// Downsample a top-down BGRA image (`w` x `h`) to 32x18 by box average of each pixel's luma
/// `(77R + 150G + 29B) >> 8`, quantise each cell to 4 bits and return 576 hex characters.
/// Images smaller than 32x18 repeat their pixels (each cell takes at least one pixel); an empty
/// or short buffer gives all zeros.
pub fn encode(w: usize, h: usize, bgra: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(W * H);
    let usable = w > 0 && h > 0 && w.checked_mul(h).and_then(|n| n.checked_mul(4)).is_some_and(|n| bgra.len() >= n);
    if !usable {
        out.extend(std::iter::repeat_n('0', W * H));
        return out;
    }
    let span = |i: usize, cells: usize, len: usize| {
        let a = (i * len / cells).min(len - 1);
        let b = ((i + 1) * len / cells).clamp(a + 1, len);
        (a, b)
    };
    for ty in 0..H {
        let (y0, y1) = span(ty, H, h);
        for tx in 0..W {
            let (x0, x1) = span(tx, W, w);
            let mut sum: u64 = 0;
            for y in y0..y1 {
                let row = &bgra[(y * w + x0) * 4..(y * w + x1) * 4];
                for p in row.chunks_exact(4) {
                    sum += ((77 * p[2] as u32 + 150 * p[1] as u32 + 29 * p[0] as u32) >> 8) as u64;
                }
            }
            let avg = sum / ((y1 - y0) * (x1 - x0)) as u64;
            out.push(HEX[(avg >> 4) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: usize, h: usize, bgr: [u8; 3]) -> Vec<u8> {
        (0..w * h).flat_map(|_| [bgr[0], bgr[1], bgr[2], 255]).collect()
    }

    fn rows(s: &str) -> Vec<&str> {
        (0..H).map(|r| &s[r * W..(r + 1) * W]).collect()
    }

    #[test]
    fn solid_colours() {
        assert_eq!(encode(64, 36, &solid(64, 36, [0, 0, 0])), "0".repeat(576));
        assert_eq!(encode(64, 36, &solid(64, 36, [255, 255, 255])), "f".repeat(576));
        // pure red: 77*255>>8 = 76 -> 4; pure green: 150*255>>8 = 149 -> 9; pure blue: 28 -> 1
        assert_eq!(encode(100, 50, &solid(100, 50, [0, 0, 255])), "4".repeat(576));
        assert_eq!(encode(100, 50, &solid(100, 50, [0, 255, 0])), "9".repeat(576));
        assert_eq!(encode(100, 50, &solid(100, 50, [255, 0, 0])), "1".repeat(576));
        // mid grey 128 -> 128 -> 8
        assert_eq!(encode(1920, 1080, &solid(1920, 1080, [128, 128, 128])), "8".repeat(576));
    }

    #[test]
    fn half_black_half_white() {
        let (w, h) = (64, 36);
        let img: Vec<u8> = (0..w * h)
            .flat_map(|i| if i % w < w / 2 { [0, 0, 0, 255] } else { [255, 255, 255, 255] })
            .collect();
        let t = encode(w, h, &img);
        assert_eq!(t.len(), 576);
        for r in rows(&t) {
            assert_eq!(r, format!("{}{}", "0".repeat(16), "f".repeat(16)));
        }
        // top half white, bottom half black
        let img: Vec<u8> = (0..w * h)
            .flat_map(|i| if i / w < h / 2 { [255, 255, 255, 255] } else { [0, 0, 0, 255] })
            .collect();
        let t = encode(w, h, &img);
        let rs = rows(&t);
        assert!(rs[..9].iter().all(|r| *r == "f".repeat(32)));
        assert!(rs[9..].iter().all(|r| *r == "0".repeat(32)));
    }

    #[test]
    fn box_average_mixes_a_cell() {
        // 2x1 pixels per cell, one black and one white -> 127 -> 7
        let (w, h) = (64, 18);
        let img: Vec<u8> = (0..w * h)
            .flat_map(|i| if i % 2 == 0 { [0, 0, 0, 255] } else { [255, 255, 255, 255] })
            .collect();
        assert_eq!(encode(w, h, &img), "7".repeat(576));
    }

    #[test]
    fn odd_sizes_smaller_than_the_grid() {
        assert_eq!(encode(1, 1, &solid(1, 1, [255, 255, 255])), "f".repeat(576));
        assert_eq!(encode(5, 3, &solid(5, 3, [128, 128, 128])), "8".repeat(576));
        // 2x1: left pixel black, right pixel white -> left half 0, right half f on every row
        let t = encode(2, 1, &[0, 0, 0, 255, 255, 255, 255, 255]);
        for r in rows(&t) {
            assert_eq!(r, format!("{}{}", "0".repeat(16), "f".repeat(16)));
        }
        // 7x5 with odd/larger-than-grid neighbours: always 576 nibbles
        assert_eq!(encode(7, 5, &solid(7, 5, [9, 9, 9])).len(), 576);
        assert_eq!(encode(33, 19, &solid(33, 19, [9, 9, 9])).len(), 576);
    }

    #[test]
    fn empty_or_short_input_is_all_zero() {
        assert_eq!(encode(0, 0, &[]), "0".repeat(576));
        assert_eq!(encode(10, 10, &[255; 12]), "0".repeat(576));
    }
}
