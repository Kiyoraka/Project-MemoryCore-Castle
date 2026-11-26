//! Image XObjects and inline images to RGBA pixels: 1 to 16 bits per component in any colour space,
//! /Decode arrays, stencil masks (/ImageMask), soft masks (/SMask), explicit and colour-key masks
//! (/Mask), and JPEG (DCTDecode) through our own decoder.

use super::colour::Cs;
use super::obj::{Dict, Obj, Stream};
use super::parse::PdfFile;
use crate::ground::display::Color;
use crate::ground::picture::jpeg;

/// The largest picture we decode (width x height).
pub const MAX_PIXELS: u64 = 1 << 26;

pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Why an image could not be decoded (the renderer draws a placeholder).
#[derive(Debug)]
#[allow(dead_code)]
pub enum ImageError {
    Unsupported(String),
    Bad(String),
}

/// Read samples of `bpc` bits from a row-padded buffer.
struct Bits<'a> {
    d: &'a [u8],
}

impl<'a> Bits<'a> {
    #[inline]
    fn get(&self, row_start: usize, idx: usize, bpc: u32) -> u32 {
        match bpc {
            8 => self.d.get(row_start + idx).copied().unwrap_or(0) as u32,
            16 => {
                let p = row_start + idx * 2;
                ((self.d.get(p).copied().unwrap_or(0) as u32) << 8) | self.d.get(p + 1).copied().unwrap_or(0) as u32
            }
            _ => {
                let bit = idx * bpc as usize;
                let byte = self.d.get(row_start + bit / 8).copied().unwrap_or(0) as u32;
                let shift = 8 - bpc as usize - bit % 8;
                (byte >> shift) & ((1 << bpc) - 1)
            }
        }
    }
}

/// Decode an image stream. `fill` is the current fill colour (stencil masks paint with it).
pub fn decode_image(file: &PdfFile, s: &Stream, resources: Option<&Dict>, fill: Color, depth: u32) -> Result<Pixels, ImageError> {
    let d = &s.dict;
    let get = |k: &str, abbr: &str| -> Obj { file.resolve(d.get(k).or_else(|| d.get(abbr)).unwrap_or(&Obj::Null)) };
    let w = get("Width", "W").as_i64().unwrap_or(0);
    let h = get("Height", "H").as_i64().unwrap_or(0);
    if w <= 0 || h <= 0 || (w as u64) * (h as u64) > MAX_PIXELS {
        return Err(ImageError::Bad(format!("bad size {}x{}", w, h)));
    }
    let (w, h) = (w as u32, h as u32);
    let stencil = get("ImageMask", "IM").as_bool().unwrap_or(false);
    let decoded = file.decode_stream(s, 0).map_err(ImageError::Bad)?;
    let decode_arr = get("Decode", "D").nums();

    let mut px = if let Some((codec, _)) = &decoded.image_codec {
        if codec != "DCTDecode" {
            return Err(ImageError::Unsupported(codec.clone()));
        }
        let img = jpeg::decode(&decoded.data).map_err(|e| ImageError::Bad(e.to_string()))?;
        let mut rgba = img.pixels;
        // An inverted Decode on a JPEG (Adobe CMYK files often say [1 0 1 0 ...]).
        if decode_arr.len() >= 2 && decode_arr[0] > decode_arr[1] {
            for p in rgba.chunks_mut(4) {
                p[0] = 255 - p[0];
                p[1] = 255 - p[1];
                p[2] = 255 - p[2];
            }
        }
        Pixels { width: img.width, height: img.height, rgba }
    } else if stencil {
        let data = &decoded.data;
        let invert = decode_arr.first().copied().unwrap_or(0.0) > 0.5;
        let row = (w as usize + 7) / 8;
        let bits = Bits { d: data };
        let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h as usize {
            for x in 0..w as usize {
                let v = bits.get(y * row, x, 1);
                let paint = (v == 0) != invert;
                rgba.extend_from_slice(&[fill.r, fill.g, fill.b, if paint { fill.a } else { 0 }]);
            }
        }
        Pixels { width: w, height: h, rgba }
    } else {
        let bpc = get("BitsPerComponent", "BPC").as_i64().unwrap_or(8);
        if !matches!(bpc, 1 | 2 | 4 | 8 | 16) {
            return Err(ImageError::Bad(format!("bits per component {}", bpc)));
        }
        let bpc = bpc as u32;
        let cs_obj = d.get("ColorSpace").or_else(|| d.get("CS")).cloned().unwrap_or(Obj::name("DeviceGray"));
        let cs = Cs::parse(file, &cs_obj, resources, 0);
        let n = cs.n();
        let decode = if decode_arr.len() >= 2 * n { decode_arr.clone() } else { cs.default_decode(bpc) };
        let max = ((1u32 << bpc) - 1) as f64;
        let row = (w as usize * n * bpc as usize + 7) / 8;
        let bits = Bits { d: &decoded.data };
        let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
        let plain = bpc == 8 && decode == cs.default_decode(8) && matches!(cs, Cs::Gray | Cs::Rgb);
        let mut comps = vec![0f64; n];
        // A small cache for Indexed and 1-component images.
        let cacheable = n == 1 && bpc <= 8;
        let mut cache: Vec<Option<[u8; 3]>> = if cacheable { vec![None; 256] } else { Vec::new() };
        for y in 0..h as usize {
            let rs = y * row;
            for x in 0..w as usize {
                if plain {
                    if n == 1 {
                        let v = bits.get(rs, x, 8) as u8;
                        rgba.extend_from_slice(&[v, v, v, 255]);
                    } else {
                        let r = bits.get(rs, x * 3, 8) as u8;
                        let g = bits.get(rs, x * 3 + 1, 8) as u8;
                        let b = bits.get(rs, x * 3 + 2, 8) as u8;
                        rgba.extend_from_slice(&[r, g, b, 255]);
                    }
                    continue;
                }
                if cacheable {
                    let raw = bits.get(rs, x, bpc) as usize;
                    let rgb = match cache.get(raw).copied().flatten() {
                        Some(c) => c,
                        None => {
                            let v = decode[0] + raw as f64 * (decode[1] - decode[0]) / max;
                            let c = cs.to_rgb(&[v]);
                            if let Some(slot) = cache.get_mut(raw) {
                                *slot = Some(c);
                            }
                            c
                        }
                    };
                    rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                    continue;
                }
                for (k, c) in comps.iter_mut().enumerate() {
                    let raw = bits.get(rs, x * n + k, bpc) as f64;
                    *c = decode[2 * k] + raw * (decode[2 * k + 1] - decode[2 * k]) / max;
                }
                let rgb = cs.to_rgb(&comps);
                rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
        // Colour-key masking.
        if let Obj::Arr(ranges) = get("Mask", "Mask") {
            let r: Vec<f64> = ranges.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect();
            if r.len() >= 2 * n {
                for y in 0..h as usize {
                    for x in 0..w as usize {
                        let masked = (0..n).all(|k| {
                            let raw = bits.get(y * row, x * n + k, bpc) as f64;
                            raw >= r[2 * k] && raw <= r[2 * k + 1]
                        });
                        if masked {
                            rgba[(y * w as usize + x) * 4 + 3] = 0;
                        }
                    }
                }
            }
        }
        Pixels { width: w, height: h, rgba }
    };

    // Soft mask, or an explicit mask image.
    if depth == 0 && !stencil {
        let smask = get("SMask", "SMask");
        if let Some(ms) = smask.as_stream() {
            if let Ok(m) = decode_image(file, ms, None, Color::WHITE, 1) {
                apply_alpha(&mut px, &m, false);
            }
        } else if let Some(ms) = get("Mask", "Mask").as_stream() {
            let mut md = ms.dict.clone();
            md.set("ImageMask", Obj::Bool(true));
            let mstream = Stream { dict: md, data: ms.data.clone() };
            // A stencil painted black where the mask says paint; 1 bits mask out by default.
            if let Ok(m) = decode_image(file, &mstream, None, Color::BLACK, 1) {
                apply_alpha(&mut px, &m, true);
            }
        }
    }
    Ok(px)
}

/// Multiply the image's alpha by the mask (gray value, or the stencil's own alpha), resampling the
/// mask to the image's size.
fn apply_alpha(px: &mut Pixels, m: &Pixels, from_alpha: bool) {
    if m.width == 0 || m.height == 0 {
        return;
    }
    for y in 0..px.height as usize {
        let my = (y * m.height as usize / px.height.max(1) as usize).min(m.height as usize - 1);
        for x in 0..px.width as usize {
            let mx = (x * m.width as usize / px.width.max(1) as usize).min(m.width as usize - 1);
            let mi = (my * m.width as usize + mx) * 4;
            let a = if from_alpha { m.rgba.get(mi + 3).copied().unwrap_or(255) } else { m.rgba.get(mi).copied().unwrap_or(255) };
            let pi = (y * px.width as usize + x) * 4 + 3;
            if let Some(p) = px.rgba.get_mut(pi) {
                *p = ((*p as u16 * a as u16) / 255) as u8;
            }
        }
    }
}
