pub const COLORS: [[u8; 3]; 4] = [[0xFF, 0x3D, 0x7F], [0xFF, 0x9F, 0x1C], [0x2E, 0xC4, 0xB6], [0x5B, 0x5B, 0xFF]];

const OUTER: (f64, f64) = (-26.13, -27.54);
const RADIUS: f64 = 92.0;
const INNER_OFFSET: (f64, f64) = (14.0, 56.0);
const INNER_RADIUS: f64 = 72.0;
pub const EXTENT: f64 = 240.0;

#[derive(Clone, Copy)]
pub struct Geometry {
    pub gap: f64,
    pub tip_rounding: f64,
}

pub const MENU_BAR: Geometry = Geometry { gap: 6.0, tip_rounding: 4.0 };

pub fn rotate(point: (f64, f64), quarter_turns: usize) -> (f64, f64) {
    (0..quarter_turns).fold(point, |(x, y), _| (-y, x))
}

fn outer_center(k: usize) -> (f64, f64) {
    rotate(OUTER, k)
}

fn inner_center(k: usize) -> (f64, f64) {
    rotate((OUTER.0 + INNER_OFFSET.0, OUTER.1 + INNER_OFFSET.1), k)
}

fn distance_1d(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    let mut k = 0;
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    for q in 1..n {
        loop {
            let p = v[k];
            let s = ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * q as f64 - 2.0 * p as f64);
            if s <= z[k] && k > 0 {
                k -= 1;
                continue;
            }
            if s <= z[k] {
                v[0] = q;
                z[0] = f64::NEG_INFINITY;
                z[1] = f64::INFINITY;
                k = 0;
                break;
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = f64::INFINITY;
            break;
        }
    }
    k = 0;
    for (q, value) in d.iter_mut().enumerate().take(n) {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        *value = (q as f64 - p as f64).powi(2) + f[p];
    }
}

fn squared_distance_to(feature: &[bool], n: usize) -> Vec<f64> {
    let infinity = 1e20;
    let mut grid: Vec<f64> = feature.iter().map(|&inside| if inside { 0.0 } else { infinity }).collect();
    let (mut f, mut d, mut v, mut z) = (vec![0.0; n], vec![0.0; n], vec![0; n], vec![0.0; n + 1]);
    for x in 0..n {
        for y in 0..n {
            f[y] = grid[y * n + x];
        }
        distance_1d(&f, &mut d, &mut v, &mut z);
        for y in 0..n {
            grid[y * n + x] = d[y];
        }
    }
    for y in 0..n {
        f.copy_from_slice(&grid[y * n..(y + 1) * n]);
        distance_1d(&f, &mut d, &mut v, &mut z);
        grid[y * n..(y + 1) * n].copy_from_slice(&d);
    }
    grid
}

fn erode(set: &[bool], n: usize, amount: f64) -> Vec<bool> {
    let outside: Vec<bool> = set.iter().map(|&inside| !inside).collect();
    let limit = amount * amount;
    squared_distance_to(&outside, n).iter().zip(set).map(|(&distance, &inside)| inside && distance > limit).collect()
}

fn dilate(set: &[bool], n: usize, amount: f64) -> Vec<bool> {
    let limit = amount * amount;
    squared_distance_to(set, n).iter().map(|&distance| distance <= limit).collect()
}

pub fn blade_masks(n: usize, center: (f64, f64), units: f64, geometry: Geometry) -> [Vec<bool>; 4] {
    let crescents: Vec<Vec<bool>> = (0..4)
        .map(|k| {
            let (outer, inner) = (outer_center(k), inner_center(k));
            let raw: Vec<bool> = (0..n * n)
                .map(|index| {
                    let x = ((index % n) as f64 + 0.5 - center.0) / units;
                    let y = ((index / n) as f64 + 0.5 - center.1) / units;
                    (x - outer.0).hypot(y - outer.1) <= RADIUS && (x - inner.0).hypot(y - inner.1) > INNER_RADIUS
                })
                .collect();
            let amount = geometry.tip_rounding * units;
            dilate(&erode(&raw, n, amount), n, amount)
        })
        .collect();
    std::array::from_fn(|k| {
        let neighbor = dilate(&crescents[(k + 3) % 4], n, geometry.gap * units);
        crescents[k].iter().zip(&neighbor).map(|(&inside, &covered)| inside && !covered).collect()
    })
}

pub fn render_flat(size: usize, inset: f64, geometry: Geometry) -> Vec<u8> {
    let factor = 4;
    let n = size * factor;
    let units = (size as f64 - 2.0 * inset) * factor as f64 / EXTENT;
    let masks = blade_masks(n, (n as f64 / 2.0, n as f64 / 2.0), units, geometry);
    let mut rgba = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let mut coverage = [0u32; 4];
            for sy in 0..factor {
                for sx in 0..factor {
                    let index = (y * factor + sy) * n + x * factor + sx;
                    for (k, mask) in masks.iter().enumerate() {
                        coverage[k] += u32::from(mask[index]);
                    }
                }
            }
            let total: u32 = coverage.iter().sum();
            if total == 0 {
                continue;
            }
            let pixel = &mut rgba[(y * size + x) * 4..][..4];
            for channel in 0..3 {
                let sum: u32 = (0..4).map(|k| coverage[k] * u32::from(COLORS[k][channel])).sum();
                pixel[channel] = (sum / total) as u8;
            }
            pixel[3] = (total * 255 / (factor * factor) as u32).min(255) as u8;
        }
    }
    rgba
}
