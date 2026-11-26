//! Images as PDF XObjects: RGBA pixels become an RGB stream plus an SMask for alpha; a JPEG's own
//! bytes go in unchanged (DCTDecode).

use super::PdfWriter;
use crate::ground::picture::jpeg::{self, Image};

/// A picture the writer can embed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfImage {
    /// Decoded pixels, RGBA 8-bit, top row first, not premultiplied.
    Rgba(Image),
    /// A JPEG file's bytes, embedded as they are.
    Jpeg {
        data: Vec<u8>,
        width: u32,
        height: u32,
        /// 1 (grey), 3 (YCbCr/RGB) or 4 (CMYK/YCCK).
        components: u8,
        /// Adobe APP14 present: a 4-component Adobe JPEG stores inverted CMYK.
        adobe: bool,
    },
}

impl PdfImage {
    /// A JPEG to embed. Baseline, extended and progressive 8-bit Huffman JPEGs keep their bytes; one
    /// the PDF filter cannot take (arithmetic coding, 12-bit) or one with an EXIF orientation other
    /// than upright is decoded by us instead and embedded as pixels (turned upright). `None` when the
    /// bytes are not a JPEG we can read at all.
    pub fn from_jpeg(data: Vec<u8>) -> Option<PdfImage> {
        let info = scan_jpeg(&data);
        if let Some(h) = info {
            if h.passthrough && h.orientation == 1 && h.width > 0 && h.height > 0 {
                return Some(PdfImage::Jpeg { width: h.width, height: h.height, components: h.components, adobe: h.adobe, data });
            }
        }
        let (img, ji) = jpeg::decode_with_info(&data).ok()?;
        Some(PdfImage::Rgba(img.oriented(ji.orientation)))
    }

    pub fn width(&self) -> u32 {
        match self {
            PdfImage::Rgba(i) => i.width,
            PdfImage::Jpeg { width, .. } => *width,
        }
    }

    pub fn height(&self) -> u32 {
        match self {
            PdfImage::Rgba(i) => i.height,
            PdfImage::Jpeg { height, .. } => *height,
        }
    }
}

struct JpegHeader {
    width: u32,
    height: u32,
    components: u8,
    adobe: bool,
    orientation: u16,
    passthrough: bool,
}

/// Reads the markers up to the first scan: frame size, components, Adobe APP14, EXIF orientation.
fn scan_jpeg(d: &[u8]) -> Option<JpegHeader> {
    if d.len() < 4 || d[0] != 0xFF || d[1] != 0xD8 {
        return None;
    }
    let mut h = JpegHeader { width: 0, height: 0, components: 0, adobe: false, orientation: 1, passthrough: false };
    let mut seen_frame = false;
    let mut i = 2;
    while i + 1 < d.len() {
        if d[i] != 0xFF {
            return None;
        }
        let mut m = d[i + 1];
        while m == 0xFF {
            i += 1;
            m = *d.get(i + 1)?;
        }
        i += 2;
        if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) {
            continue;
        }
        if m == 0xD9 || m == 0xDA {
            break;
        }
        let len = u16::from_be_bytes([*d.get(i)?, *d.get(i + 1)?]) as usize;
        if len < 2 {
            return None;
        }
        let seg = d.get(i + 2..i + len)?;
        match m {
            0xC0..=0xCF if m != 0xC4 && m != 0xC8 && m != 0xCC => {
                if seg.len() < 6 {
                    return None;
                }
                h.height = u16::from_be_bytes([seg[1], seg[2]]) as u32;
                h.width = u16::from_be_bytes([seg[3], seg[4]]) as u32;
                h.components = seg[5];
                h.passthrough = seg[0] == 8 && matches!(m, 0xC0 | 0xC1 | 0xC2) && matches!(h.components, 1 | 3 | 4);
                seen_frame = true;
            }
            0xEE if seg.starts_with(b"Adobe") => h.adobe = true,
            0xE1 if seg.starts_with(b"Exif\0\0") => {
                if let Some(o) = exif_orientation(&seg[6..]) {
                    h.orientation = o;
                }
            }
            _ => {}
        }
        i += len;
    }
    if seen_frame {
        Some(h)
    } else {
        None
    }
}

/// The orientation tag (0x0112) of IFD0 in a TIFF block.
fn exif_orientation(t: &[u8]) -> Option<u16> {
    let le = match t.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let r16 = |o: usize| -> Option<u16> {
        let b = t.get(o..o + 2)?;
        Some(if le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) })
    };
    let r32 = |o: usize| -> Option<u32> {
        let b = t.get(o..o + 4)?;
        Some(if le { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) })
    };
    let ifd = r32(4)? as usize;
    let n = r16(ifd)? as usize;
    for k in 0..n.min(512) {
        let e = ifd + 2 + 12 * k;
        if r16(e)? == 0x0112 {
            let v = r16(e + 8)?;
            return if (1..=8).contains(&v) { Some(v) } else { None };
        }
    }
    None
}

/// Write the picture's XObject (and its SMask); `None` for a picture with no pixels.
pub(super) fn write_image(w: &mut PdfWriter, img: &PdfImage) -> Option<u32> {
    match img {
        PdfImage::Jpeg { data, width, height, components, adobe } => {
            let (cs, decode) = match components {
                1 => ("/DeviceGray", ""),
                4 if *adobe => ("/DeviceCMYK", " /Decode [1 0 1 0 1 0 1 0]"),
                4 => ("/DeviceCMYK", ""),
                _ => ("/DeviceRGB", ""),
            };
            let dict = format!(
                "/Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace {cs} /BitsPerComponent 8{decode} /Filter /DCTDecode"
            );
            Some(w.new_stream(&dict, data, false))
        }
        PdfImage::Rgba(im) => {
            let n = im.width as usize * im.height as usize;
            if n == 0 || im.pixels.len() < n * 4 {
                return None;
            }
            let mut rgb = Vec::with_capacity(n * 3);
            let mut alpha = Vec::with_capacity(n);
            for p in im.pixels[..n * 4].chunks_exact(4) {
                rgb.extend_from_slice(&p[..3]);
                alpha.push(p[3]);
            }
            let smask = if alpha.iter().any(|&a| a != 255) {
                let dict = format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8", im.width, im.height);
                Some(w.new_stream(&dict, &alpha, true))
            } else {
                None
            };
            let mut dict = format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8", im.width, im.height);
            if let Some(s) = smask {
                dict.push_str(&format!(" /SMask {s} 0 R"));
            }
            Some(w.new_stream(&dict, &rgb, true))
        }
    }
}
