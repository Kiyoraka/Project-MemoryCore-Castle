//! Colour spaces (DeviceGray/RGB/CMYK, CalGray/CalRGB, Lab, ICCBased by /N, Indexed, Separation and
//! DeviceN through their alternate space and tint function, Pattern) and the PDF functions
//! (types 0, 2 and 3; type 4 falls back to an approximation).

use super::obj::{Dict, Obj};
use super::parse::PdfFile;
use crate::ground::display::Color;

#[derive(Debug, Clone, PartialEq)]
pub enum Func {
    /// Exponential: c0 + x^n (c1 - c0).
    Exp { c0: Vec<f64>, c1: Vec<f64>, n: f64, domain: [f64; 2] },
    /// Stitching.
    Stitch { funcs: Vec<Func>, bounds: Vec<f64>, encode: Vec<f64>, domain: [f64; 2] },
    /// Sampled, one input.
    Sampled { samples: Vec<f64>, size: usize, outputs: usize, domain: [f64; 2], encode: [f64; 2], decode: Vec<f64> },
    /// Several functions, one per output.
    Many(Vec<Func>),
}

impl Func {
    pub fn parse(file: &PdfFile, o: &Obj, depth: u32) -> Option<Func> {
        if depth > 8 {
            return None;
        }
        let o = file.resolve(o);
        if let Obj::Arr(a) = &o {
            let fs: Vec<Func> = a.iter().filter_map(|f| Func::parse(file, f, depth + 1)).collect();
            return if fs.is_empty() { None } else { Some(Func::Many(fs)) };
        }
        let d = o.as_dict()?;
        let dom = file.dget(d, "Domain").nums();
        let domain = [dom.first().copied().unwrap_or(0.0), dom.get(1).copied().unwrap_or(1.0)];
        match file.dget(d, "FunctionType").as_i64()? {
            2 => {
                let c0 = file.dget(d, "C0");
                let c1 = file.dget(d, "C1");
                Some(Func::Exp {
                    c0: if c0.is_null() { vec![0.0] } else { c0.nums() },
                    c1: if c1.is_null() { vec![1.0] } else { c1.nums() },
                    n: file.dget(d, "N").as_f64().unwrap_or(1.0),
                    domain,
                })
            }
            3 => {
                let funcs: Vec<Func> = match file.dget(d, "Functions") {
                    Obj::Arr(a) => a.iter().filter_map(|f| Func::parse(file, f, depth + 1)).collect(),
                    _ => Vec::new(),
                };
                if funcs.is_empty() {
                    return None;
                }
                Some(Func::Stitch { funcs, bounds: file.dget(d, "Bounds").nums(), encode: file.dget(d, "Encode").nums(), domain })
            }
            0 => {
                let s = o.as_stream()?;
                let size = file.dget(d, "Size").nums().first().copied().unwrap_or(0.0).clamp(0.0, 65536.0) as usize;
                let bps = file.dget(d, "BitsPerSample").as_i64().unwrap_or(8);
                let range = file.dget(d, "Range").nums();
                let outputs = (range.len() / 2).max(1);
                if size < 1 || !matches!(bps, 1 | 2 | 4 | 8 | 16 | 32) {
                    return None;
                }
                let data = file.stream_data(s);
                let max = ((1u64 << bps.min(32)) - 1) as f64;
                let mut samples = Vec::with_capacity(size * outputs);
                let mut bit = 0usize;
                for _ in 0..size * outputs {
                    let mut v: u64 = 0;
                    for _ in 0..bps {
                        let byte = data.get(bit / 8).copied().unwrap_or(0);
                        v = (v << 1) | ((byte >> (7 - bit % 8)) & 1) as u64;
                        bit += 1;
                    }
                    samples.push(v as f64 / max);
                }
                let enc = file.dget(d, "Encode").nums();
                let encode = [enc.first().copied().unwrap_or(0.0), enc.get(1).copied().unwrap_or((size - 1) as f64)];
                let mut decode = file.dget(d, "Decode").nums();
                if decode.len() < outputs * 2 {
                    decode = range.clone();
                }
                Some(Func::Sampled { samples, size, outputs, domain, encode, decode })
            }
            _ => None,
        }
    }

    pub fn eval(&self, x: f64) -> Vec<f64> {
        match self {
            Func::Exp { c0, c1, n, domain } => {
                let x = x.clamp(domain[0].min(domain[1]), domain[1].max(domain[0]));
                let k = if *n == 1.0 { x } else { x.max(0.0).powf(*n) };
                let len = c0.len().max(c1.len());
                (0..len)
                    .map(|i| {
                        let a = c0.get(i).copied().unwrap_or(0.0);
                        let b = c1.get(i).copied().unwrap_or(1.0);
                        a + k * (b - a)
                    })
                    .collect()
            }
            Func::Stitch { funcs, bounds, encode, domain } => {
                let x = x.clamp(domain[0], domain[1].max(domain[0]));
                let mut i = 0;
                while i < bounds.len() && x >= bounds[i] {
                    i += 1;
                }
                let i = i.min(funcs.len() - 1);
                let lo = if i == 0 { domain[0] } else { bounds[i - 1] };
                let hi = if i < bounds.len() { bounds[i] } else { domain[1] };
                let e0 = encode.get(2 * i).copied().unwrap_or(0.0);
                let e1 = encode.get(2 * i + 1).copied().unwrap_or(1.0);
                let t = if hi > lo { e0 + (x - lo) / (hi - lo) * (e1 - e0) } else { e0 };
                funcs[i].eval(t)
            }
            Func::Sampled { samples, size, outputs, domain, encode, decode } => {
                let x = x.clamp(domain[0], domain[1].max(domain[0]));
                let span = (domain[1] - domain[0]).abs().max(1e-12);
                let e = (encode[0] + (x - domain[0]) / span * (encode[1] - encode[0])).clamp(0.0, (*size - 1) as f64);
                let i0 = e.floor() as usize;
                let i1 = (i0 + 1).min(size - 1);
                let f = e - i0 as f64;
                (0..*outputs)
                    .map(|o| {
                        let a = samples.get(i0 * outputs + o).copied().unwrap_or(0.0);
                        let b = samples.get(i1 * outputs + o).copied().unwrap_or(0.0);
                        let v = a + (b - a) * f;
                        let d0 = decode.get(2 * o).copied().unwrap_or(0.0);
                        let d1 = decode.get(2 * o + 1).copied().unwrap_or(1.0);
                        d0 + v * (d1 - d0)
                    })
                    .collect()
            }
            Func::Many(fs) => fs.iter().map(|f| f.eval(x).first().copied().unwrap_or(0.0)).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cs {
    Gray,
    Rgb,
    Cmyk,
    Lab { white: [f64; 3], range: [f64; 4] },
    Indexed { base: Box<Cs>, hival: usize, lut: Vec<u8> },
    /// Separation (n = 1) or DeviceN.
    Tint { n: usize, alt: Box<Cs>, func: Option<Func>, none: bool },
    Pattern,
}

fn b(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

impl Cs {
    /// Components per colour value.
    pub fn n(&self) -> usize {
        match self {
            Cs::Gray | Cs::Indexed { .. } => 1,
            Cs::Rgb | Cs::Lab { .. } => 3,
            Cs::Cmyk => 4,
            Cs::Tint { n, .. } => *n,
            Cs::Pattern => 1,
        }
    }

    /// The initial colour when the space is selected.
    pub fn initial(&self) -> Vec<f64> {
        match self {
            Cs::Cmyk => vec![0.0, 0.0, 0.0, 1.0],
            Cs::Tint { n, .. } => vec![1.0; *n],
            Cs::Lab { .. } => vec![0.0, 0.0, 0.0],
            other => vec![0.0; other.n()],
        }
    }

    /// The default Decode range for image samples (per component).
    pub fn default_decode(&self, bpc: u32) -> Vec<f64> {
        match self {
            Cs::Indexed { .. } => vec![0.0, ((1u64 << bpc.min(16)) - 1) as f64],
            Cs::Lab { range, .. } => vec![0.0, 100.0, range[0], range[1], range[2], range[3]],
            other => (0..other.n()).flat_map(|_| [0.0, 1.0]).collect(),
        }
    }

    pub fn to_rgb(&self, c: &[f64]) -> [u8; 3] {
        let g = |i: usize| c.get(i).copied().unwrap_or(0.0);
        match self {
            Cs::Gray => {
                let v = b(g(0));
                [v, v, v]
            }
            Cs::Rgb => [b(g(0)), b(g(1)), b(g(2))],
            Cs::Cmyk => {
                let k = g(3).clamp(0.0, 1.0);
                [b((1.0 - g(0)) * (1.0 - k)), b((1.0 - g(1)) * (1.0 - k)), b((1.0 - g(2)) * (1.0 - k))]
            }
            Cs::Lab { white, .. } => lab_to_rgb(g(0), g(1), g(2), white),
            Cs::Indexed { base, hival, lut } => {
                let i = (g(0).round().max(0.0) as usize).min(*hival);
                let n = base.n();
                let comps: Vec<f64> = (0..n).map(|k| lut.get(i * n + k).copied().unwrap_or(0) as f64 / 255.0).collect();
                // Lab lookups hold the raw range; others are 0..1.
                if let Cs::Lab { range, .. } = base.as_ref() {
                    let l = comps[0] * 100.0;
                    let a = range[0] + comps.get(1).copied().unwrap_or(0.0) * (range[1] - range[0]);
                    let bb = range[2] + comps.get(2).copied().unwrap_or(0.0) * (range[3] - range[2]);
                    return base.to_rgb(&[l, a, bb]);
                }
                base.to_rgb(&comps)
            }
            Cs::Tint { n, alt, func, none } => {
                if *none {
                    return [255, 255, 255];
                }
                match func {
                    Some(f) => {
                        let out = if *n == 1 {
                            f.eval(g(0))
                        } else {
                            // Multi-input functions are not evaluated: use the average tint.
                            let avg = (0..*n).map(g).sum::<f64>() / *n as f64;
                            f.eval(avg)
                        };
                        alt.to_rgb(&out)
                    }
                    None => {
                        let t = (0..*n).map(g).fold(0.0f64, f64::max);
                        let v = b(1.0 - t);
                        [v, v, v]
                    }
                }
            }
            Cs::Pattern => [128, 128, 128],
        }
    }

    pub fn color(&self, c: &[f64], alpha: f64) -> Color {
        let [r, g, bl] = self.to_rgb(c);
        Color::rgba(r, g, bl, b(alpha))
    }

    /// Parse a colour space object; names not built in are looked up in `resources`'s /ColorSpace.
    pub fn parse(file: &PdfFile, o: &Obj, resources: Option<&Dict>, depth: u32) -> Cs {
        if depth > 8 {
            return Cs::Rgb;
        }
        let o = file.resolve(o);
        match &o {
            Obj::Name(n) => match n.as_str() {
                "DeviceGray" | "G" | "CalGray" => Cs::Gray,
                "DeviceRGB" | "RGB" | "CalRGB" => Cs::Rgb,
                "DeviceCMYK" | "CMYK" => Cs::Cmyk,
                "Pattern" => Cs::Pattern,
                other => {
                    let named = resources.and_then(|r| file.dict_of(r, "ColorSpace")).and_then(|cs| cs.get(other).cloned());
                    match named {
                        Some(v) => Cs::parse(file, &v, None, depth + 1),
                        None => Cs::Rgb,
                    }
                }
            },
            Obj::Arr(a) => {
                let kind = a.first().map(|k| file.resolve(k)).and_then(|k| k.as_name().map(str::to_string)).unwrap_or_default();
                let arg = |i: usize| a.get(i).map(|v| file.resolve(v)).unwrap_or(Obj::Null);
                match kind.as_str() {
                    "DeviceGray" | "CalGray" | "G" => Cs::Gray,
                    "DeviceRGB" | "CalRGB" | "RGB" => Cs::Rgb,
                    "DeviceCMYK" | "CMYK" => Cs::Cmyk,
                    "Lab" => {
                        let d = arg(1);
                        let d = d.as_dict().cloned().unwrap_or_default();
                        let wp = file.dget(&d, "WhitePoint").nums();
                        let r = file.dget(&d, "Range").nums();
                        Cs::Lab {
                            white: [wp.first().copied().unwrap_or(0.9505), wp.get(1).copied().unwrap_or(1.0), wp.get(2).copied().unwrap_or(1.089)],
                            range: if r.len() >= 4 { [r[0], r[1], r[2], r[3]] } else { [-100.0, 100.0, -100.0, 100.0] },
                        }
                    }
                    "ICCBased" => {
                        let s = arg(1);
                        let d = s.as_dict().cloned().unwrap_or_default();
                        if let Some(alt) = d.get("Alternate") {
                            let cs = Cs::parse(file, alt, resources, depth + 1);
                            if !matches!(cs, Cs::Pattern) {
                                return cs;
                            }
                        }
                        match file.dget(&d, "N").as_i64() {
                            Some(1) => Cs::Gray,
                            Some(4) => Cs::Cmyk,
                            _ => Cs::Rgb,
                        }
                    }
                    "Indexed" | "I" => {
                        let base = Cs::parse(file, a.get(1).unwrap_or(&Obj::Null), resources, depth + 1);
                        let hival = arg(2).as_i64().unwrap_or(0).clamp(0, 255) as usize;
                        let lut = match arg(3) {
                            Obj::Str(s) => s,
                            Obj::Stream(s) => file.stream_data(&s),
                            _ => Vec::new(),
                        };
                        Cs::Indexed { base: Box::new(base), hival, lut }
                    }
                    "Separation" | "DeviceN" => {
                        let (n, none) = if kind == "Separation" {
                            (1, arg(1).as_name() == Some("None"))
                        } else {
                            let names = arg(1);
                            let names = names.as_arr().map(|x| x.to_vec()).unwrap_or_default();
                            (names.len().max(1), !names.is_empty() && names.iter().all(|x| x.as_name() == Some("None")))
                        };
                        let alt = Cs::parse(file, a.get(2).unwrap_or(&Obj::Null), resources, depth + 1);
                        let func = a.get(3).and_then(|f| Func::parse(file, f, 0));
                        Cs::Tint { n: n.min(32), alt: Box::new(alt), func, none }
                    }
                    "Pattern" => Cs::Pattern,
                    _ => Cs::Rgb,
                }
            }
            _ => Cs::Rgb,
        }
    }
}

fn lab_to_rgb(l: f64, a: f64, bb: f64, white: &[f64; 3]) -> [u8; 3] {
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - bb / 200.0;
    let g = |t: f64| if t > 6.0 / 29.0 { t * t * t } else { 108.0 / 841.0 * (t - 4.0 / 29.0) };
    let x = white[0] * g(fx);
    let y = white[1] * g(fy);
    let z = white[2] * g(fz);
    let r = 3.2406 * x - 1.5372 * y - 0.4986 * z;
    let gg = -0.9689 * x + 1.8758 * y + 0.0415 * z;
    let bl = 0.0557 * x - 0.2040 * y + 1.0570 * z;
    let gamma = |c: f64| if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.max(0.0).powf(1.0 / 2.4) - 0.055 };
    [b(gamma(r)), b(gamma(gg)), b(gamma(bl))]
}
