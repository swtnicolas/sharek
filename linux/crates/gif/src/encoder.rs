use std::io::{self, Write};

pub struct Encoder<W: Write> {
    width: usize,
    height: usize,
    output: W,
    reference: Vec<u32>,
    has_reference: bool,
    pending: Option<Vec<u8>>,
    pending_time: f64,
    quantizer: PaletteQuantizer,
    lzw: LzwCompressor,
    indices: Vec<u8>,
    changed: Vec<bool>,
}

#[derive(Clone, Copy)]
struct Bounds {
    min_x: usize,
    min_y: usize,
    max_x: usize,
    max_y: usize,
}

fn pixel_at(pixels: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([pixels[offset], pixels[offset + 1], pixels[offset + 2], pixels[offset + 3]]) | 0xFF00_0000
}

fn push_le16(data: &mut Vec<u8>, value: usize) {
    data.push((value & 0xFF) as u8);
    data.push(((value >> 8) & 0xFF) as u8);
}

impl<W: Write> Encoder<W> {
    pub fn new(mut output: W, width: usize, height: usize) -> io::Result<Encoder<W>> {
        let mut header = b"GIF89a".to_vec();
        push_le16(&mut header, width);
        push_le16(&mut header, height);
        header.extend_from_slice(&[0x70, 0, 0]);
        header.extend_from_slice(&[0x21, 0xFF, 0x0B]);
        header.extend_from_slice(b"NETSCAPE2.0");
        header.extend_from_slice(&[0x03, 0x01, 0x00, 0x00, 0x00]);
        output.write_all(&header)?;
        Ok(Encoder {
            width,
            height,
            output,
            reference: vec![0; width * height],
            has_reference: false,
            pending: None,
            pending_time: 0.0,
            quantizer: PaletteQuantizer::new(),
            lzw: LzwCompressor::new(),
            indices: Vec::new(),
            changed: Vec::new(),
        })
    }

    pub fn is_empty(&self) -> bool {
        !self.has_reference
    }

    pub fn append(&mut self, bgrx: &[u8], stride: usize, time: f64) -> io::Result<()> {
        let Some(bounds) = self.changed_bounds(bgrx, stride) else { return Ok(()) };
        self.flush_pending(time)?;

        let frame_width = bounds.max_x - bounds.min_x + 1;
        let frame_height = bounds.max_y - bounds.min_y + 1;
        let count = frame_width * frame_height;
        if self.changed.len() < count {
            self.changed = vec![false; count];
            self.indices = vec![0; count];
        }

        self.quantizer.begin();
        for y in 0..frame_height {
            let row = (bounds.min_y + y) * stride;
            let reference_row = (bounds.min_y + y) * self.width;
            for x in 0..frame_width {
                let pixel = pixel_at(bgrx, row + (bounds.min_x + x) * 4);
                let differs = !self.has_reference || self.reference[reference_row + bounds.min_x + x] != pixel;
                self.changed[y * frame_width + x] = differs;
                if differs {
                    self.quantizer.add(pixel);
                }
            }
        }
        let palette = self.quantizer.build(255);
        let transparent = palette.len() as u8;

        for y in 0..frame_height {
            let row = (bounds.min_y + y) * stride;
            let reference_row = (bounds.min_y + y) * self.width;
            for x in 0..frame_width {
                let offset = y * frame_width + x;
                if self.changed[offset] {
                    let pixel = pixel_at(bgrx, row + (bounds.min_x + x) * 4);
                    self.indices[offset] = self.quantizer.index_of(pixel);
                    self.reference[reference_row + bounds.min_x + x] = pixel;
                } else {
                    self.indices[offset] = transparent;
                }
            }
        }
        self.quantizer.reset();
        self.has_reference = true;

        let mut table_bits = 1;
        while (1 << table_bits) < palette.len() + 1 {
            table_bits += 1;
        }

        let mut frame = vec![0x21, 0xF9, 0x04, (1 << 2) | 1, 0, 0, transparent, 0, 0x2C];
        push_le16(&mut frame, bounds.min_x);
        push_le16(&mut frame, bounds.min_y);
        push_le16(&mut frame, frame_width);
        push_le16(&mut frame, frame_height);
        frame.push(0x80 | (table_bits - 1) as u8);
        for color in &palette {
            frame.extend_from_slice(&[(color >> 16) as u8, (color >> 8) as u8, *color as u8]);
        }
        frame.resize(frame.len() + ((1 << table_bits) - palette.len()) * 3, 0);
        self.lzw.compress(&self.indices[..count], table_bits.max(2), &mut frame);
        self.pending = Some(frame);
        self.pending_time = time;
        Ok(())
    }

    pub fn finish(mut self, time: f64) -> io::Result<W> {
        let end = time.max(self.pending_time + 0.1);
        self.flush_pending(end)?;
        self.output.write_all(&[0x3B])?;
        self.output.flush()?;
        Ok(self.output)
    }

    fn flush_pending(&mut self, time: f64) -> io::Result<()> {
        let Some(mut frame) = self.pending.take() else { return Ok(()) };
        let centiseconds = (((time * 100.0).round() - (self.pending_time * 100.0).round()) as i64).clamp(2, 65535) as u16;
        frame[4..6].copy_from_slice(&centiseconds.to_le_bytes());
        self.output.write_all(&frame)
    }

    fn changed_bounds(&self, bgrx: &[u8], stride: usize) -> Option<Bounds> {
        if !self.has_reference {
            return Some(Bounds { min_x: 0, min_y: 0, max_x: self.width - 1, max_y: self.height - 1 });
        }
        let mut bounds: Option<Bounds> = None;
        for y in 0..self.height {
            let row = y * stride;
            let reference_row = &self.reference[y * self.width..(y + 1) * self.width];
            let Some(first) = (0..self.width).find(|&x| reference_row[x] != pixel_at(bgrx, row + x * 4)) else { continue };
            let last = (first..self.width).rev().find(|&x| reference_row[x] != pixel_at(bgrx, row + x * 4)).unwrap_or(first);
            bounds = Some(match bounds {
                Some(found) => Bounds { min_x: found.min_x.min(first), min_y: found.min_y, max_x: found.max_x.max(last), max_y: y },
                None => Bounds { min_x: first, min_y: y, max_x: last, max_y: y },
            });
        }
        bounds
    }
}

const BINS: usize = 1 << 18;

struct PaletteQuantizer {
    counts: Vec<u32>,
    sum_r: Vec<u32>,
    sum_g: Vec<u32>,
    sum_b: Vec<u32>,
    lookup: Vec<u8>,
    touched: Vec<u32>,
}

fn bin(pixel: u32) -> usize {
    let r = ((pixel >> 16) & 0xFF) as usize >> 2;
    let g = ((pixel >> 8) & 0xFF) as usize >> 2;
    let b = (pixel & 0xFF) as usize >> 2;
    (r << 12) | (g << 6) | b
}

impl PaletteQuantizer {
    fn new() -> PaletteQuantizer {
        PaletteQuantizer {
            counts: vec![0; BINS],
            sum_r: vec![0; BINS],
            sum_g: vec![0; BINS],
            sum_b: vec![0; BINS],
            lookup: vec![0; BINS],
            touched: Vec::new(),
        }
    }

    fn begin(&mut self) {
        self.touched.clear();
    }

    fn add(&mut self, pixel: u32) {
        let index = bin(pixel);
        if self.counts[index] == 0 {
            self.touched.push(index as u32);
        }
        self.counts[index] = self.counts[index].wrapping_add(1);
        self.sum_r[index] = self.sum_r[index].wrapping_add((pixel >> 16) & 0xFF);
        self.sum_g[index] = self.sum_g[index].wrapping_add((pixel >> 8) & 0xFF);
        self.sum_b[index] = self.sum_b[index].wrapping_add(pixel & 0xFF);
    }

    fn index_of(&self, pixel: u32) -> u8 {
        self.lookup[bin(pixel)]
    }

    fn measure(&self, start: usize, end: usize) -> (usize, i64, u32) {
        let mut low = [63usize; 3];
        let mut high = [0usize; 3];
        let mut population = 0u32;
        for &bin in &self.touched[start..end] {
            let bin = bin as usize;
            population = population.wrapping_add(self.counts[bin]);
            let components = [(bin >> 12) & 63, (bin >> 6) & 63, bin & 63];
            for channel in 0..3 {
                low[channel] = low[channel].min(components[channel]);
                high[channel] = high[channel].max(components[channel]);
            }
        }
        let ranges = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
        let widest = *ranges.iter().max().unwrap_or(&0);
        let channel = ranges.iter().position(|&range| range == widest).unwrap_or(0);
        (channel, widest as i64, population)
    }

    fn build(&mut self, max_colors: usize) -> Vec<u32> {
        if self.touched.is_empty() {
            return vec![0];
        }
        let mut boxes = vec![(0usize, self.touched.len())];
        while boxes.len() < max_colors {
            let mut best: Option<(usize, usize)> = None;
            let mut best_score = 0i64;
            for (index, &(start, end)) in boxes.iter().enumerate() {
                if end - start <= 1 {
                    continue;
                }
                let (channel, range, population) = self.measure(start, end);
                if range <= 0 {
                    continue;
                }
                let score = range * range * f64::from(population).sqrt().ceil() as i64;
                if score > best_score {
                    best_score = score;
                    best = Some((index, channel));
                }
            }
            let Some((index, channel)) = best else { break };
            let (start, end) = boxes[index];
            let shift = (2 - channel) * 6;
            self.touched[start..end].sort_by_key(|&bin| (bin >> shift) & 63);
            let total = self.touched[start..end].iter().fold(0u32, |sum, &bin| sum.wrapping_add(self.counts[bin as usize]));
            let mut running = 0u32;
            let mut cut = start + 1;
            for position in start..end {
                running = running.wrapping_add(self.counts[self.touched[position] as usize]);
                if running >= total / 2 {
                    cut = (position + 1).max(start + 1).min(end - 1);
                    break;
                }
            }
            boxes[index] = (start, cut);
            boxes.push((cut, end));
        }

        let palette: Vec<u32> = boxes
            .iter()
            .map(|&(start, end)| {
                let (mut population, mut r, mut g, mut b) = (0u64, 0u64, 0u64, 0u64);
                for &bin in &self.touched[start..end] {
                    let bin = bin as usize;
                    population += u64::from(self.counts[bin]);
                    r += u64::from(self.sum_r[bin]);
                    g += u64::from(self.sum_g[bin]);
                    b += u64::from(self.sum_b[bin]);
                }
                let divisor = population.max(1);
                let channel = |sum: u64| ((sum + divisor / 2) / divisor) as u32;
                (channel(r) << 16) | (channel(g) << 8) | channel(b)
            })
            .collect();

        let channels: Vec<(i64, i64, i64)> = palette.iter().map(|&color| (i64::from(color >> 16), i64::from((color >> 8) & 0xFF), i64::from(color & 0xFF))).collect();
        for &bin in &self.touched {
            let bin = bin as usize;
            let population = self.counts[bin].max(1);
            let (r, g, b) = (i64::from(self.sum_r[bin] / population), i64::from(self.sum_g[bin] / population), i64::from(self.sum_b[bin] / population));
            let mut best = 0;
            let mut best_distance = i64::MAX;
            for (candidate, &(cr, cg, cb)) in channels.iter().enumerate() {
                let (dr, dg, db) = (r - cr, g - cg, b - cb);
                let distance = dr * dr * 3 + dg * dg * 4 + db * db * 2;
                if distance < best_distance {
                    best_distance = distance;
                    best = candidate;
                    if distance == 0 {
                        break;
                    }
                }
            }
            self.lookup[bin] = best as u8;
        }
        palette
    }

    fn reset(&mut self) {
        for &bin in &self.touched {
            let bin = bin as usize;
            self.counts[bin] = 0;
            self.sum_r[bin] = 0;
            self.sum_g[bin] = 0;
            self.sum_b[bin] = 0;
        }
        self.touched.clear();
    }
}

const TABLE_SIZE: usize = 8192;

struct LzwCompressor {
    keys: Vec<i32>,
    codes: Vec<u16>,
}

struct BitWriter<'a> {
    output: &'a mut Vec<u8>,
    block: Vec<u8>,
    buffer: u32,
    count: u32,
    code_size: u32,
}

impl BitWriter<'_> {
    fn emit(&mut self, code: usize) {
        self.buffer |= (code as u32) << self.count;
        self.count += self.code_size;
        while self.count >= 8 {
            self.block.push(self.buffer as u8);
            self.buffer >>= 8;
            self.count -= 8;
            if self.block.len() == 255 {
                self.output.push(255);
                self.output.extend_from_slice(&self.block);
                self.block.clear();
            }
        }
    }

    fn finish(mut self) {
        if self.count > 0 {
            self.block.push(self.buffer as u8);
        }
        if !self.block.is_empty() {
            self.output.push(self.block.len() as u8);
            self.output.extend_from_slice(&self.block);
        }
        self.output.push(0);
    }
}

impl LzwCompressor {
    fn new() -> LzwCompressor {
        LzwCompressor { keys: vec![-1; TABLE_SIZE], codes: vec![0; TABLE_SIZE] }
    }

    fn compress(&mut self, input: &[u8], minimum_code_size: u32, output: &mut Vec<u8>) {
        output.push(minimum_code_size as u8);
        let clear = 1usize << minimum_code_size;
        let end = clear + 1;
        let mut next_code = end + 1;
        let mut writer = BitWriter { output, block: Vec::with_capacity(255), buffer: 0, count: 0, code_size: minimum_code_size + 1 };

        self.keys.fill(-1);
        writer.emit(clear);
        let Some((&first, rest)) = input.split_first() else {
            writer.emit(end);
            writer.finish();
            return;
        };

        let mut prefix = usize::from(first);
        for &symbol in rest {
            let symbol = usize::from(symbol);
            let key = ((prefix << 8) | symbol) as i32;
            let mut slot = ((symbol << 5) ^ prefix) & (TABLE_SIZE - 1);
            let mut found = false;
            while self.keys[slot] != -1 {
                if self.keys[slot] == key {
                    prefix = usize::from(self.codes[slot]);
                    found = true;
                    break;
                }
                slot = (slot + 1) & (TABLE_SIZE - 1);
            }
            if found {
                continue;
            }

            writer.emit(prefix);
            if next_code < 4096 {
                self.keys[slot] = key;
                self.codes[slot] = next_code as u16;
                next_code += 1;
                if next_code > (1 << writer.code_size) && writer.code_size < 12 {
                    writer.code_size += 1;
                }
            } else {
                writer.emit(clear);
                self.keys.fill(-1);
                writer.code_size = minimum_code_size + 1;
                next_code = end + 1;
            }
            prefix = symbol;
        }
        writer.emit(prefix);
        if next_code >= (1 << writer.code_size) && writer.code_size < 12 {
            writer.code_size += 1;
        }
        writer.emit(end);
        writer.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Decoded {
        frames: Vec<Vec<[u8; 3]>>,
        delays: Vec<u16>,
        rects: Vec<(u16, u16, u16, u16)>,
    }

    fn bgrx(colors: &[[u8; 3]]) -> Vec<u8> {
        colors.iter().flat_map(|&[r, g, b]| [b, g, r, 0]).collect()
    }

    fn decode(bytes: &[u8], width: usize, height: usize) -> Decoded {
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = options.read_info(bytes).unwrap();
        let mut canvas = vec![[0u8; 3]; width * height];
        let mut decoded = Decoded { frames: Vec::new(), delays: Vec::new(), rects: Vec::new() };
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            let (left, top, frame_width) = (frame.left as usize, frame.top as usize, frame.width as usize);
            for (index, pixel) in frame.buffer.as_chunks::<4>().0.iter().enumerate() {
                if pixel[3] != 0 {
                    canvas[(top + index / frame_width) * width + left + index % frame_width] = [pixel[0], pixel[1], pixel[2]];
                }
            }
            decoded.frames.push(canvas.clone());
            decoded.delays.push(frame.delay);
            decoded.rects.push((frame.left, frame.top, frame.width, frame.height));
        }
        decoded
    }

    fn noise(width: usize, height: usize, seed: u32, colors: u32) -> Vec<[u8; 3]> {
        let mut state = seed;
        (0..width * height)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let color = (state >> 8) % colors;
                [(color * 4 % 256) as u8, (color / 64 * 16 % 256) as u8, (color * 12 % 256) as u8]
            })
            .collect()
    }

    #[test]
    fn frames_with_few_colors_decode_to_the_exact_pixels() {
        let (width, height) = (200, 150);
        let first = noise(width, height, 1, 200);
        let second = noise(width, height, 2, 200);
        let mut encoder = Encoder::new(Vec::new(), width, height).unwrap();
        encoder.append(&bgrx(&first), width * 4, 0.0).unwrap();
        encoder.append(&bgrx(&second), width * 4, 0.5).unwrap();
        let decoded = decode(&encoder.finish(1.0).unwrap(), width, height);
        assert_eq!(decoded.frames, vec![first, second]);
    }

    #[test]
    fn many_colors_are_quantized_close_to_the_source() {
        let (width, height) = (64, 64);
        let gradient: Vec<[u8; 3]> = (0..width * height).map(|index| [(index % width * 4) as u8, (index / width * 4) as u8, ((index % width + index / width) * 2) as u8]).collect();
        let mut encoder = Encoder::new(Vec::new(), width, height).unwrap();
        encoder.append(&bgrx(&gradient), width * 4, 0.0).unwrap();
        let decoded = decode(&encoder.finish(0.1).unwrap(), width, height);
        let worst = decoded.frames[0].iter().zip(&gradient).flat_map(|(got, want)| (0..3).map(move |channel| got[channel].abs_diff(want[channel]))).max().unwrap();
        assert!(worst <= 16, "worst channel error {worst}");
    }

    #[test]
    fn unchanged_frames_extend_the_previous_delay() {
        let (width, height) = (4, 4);
        let first = vec![[10u8, 20, 30]; width * height];
        let mut second = first.clone();
        second[5] = [200, 100, 0];
        let mut encoder = Encoder::new(Vec::new(), width, height).unwrap();
        encoder.append(&bgrx(&first), width * 4, 0.0).unwrap();
        encoder.append(&bgrx(&first), width * 4, 0.5).unwrap();
        encoder.append(&bgrx(&second), width * 4, 1.0).unwrap();
        let decoded = decode(&encoder.finish(1.25).unwrap(), width, height);
        assert_eq!(decoded.delays, vec![100, 25]);
    }

    #[test]
    fn delays_at_fifteen_frames_per_second_add_up_to_the_elapsed_time() {
        let (width, height) = (2, 2);
        let mut encoder = Encoder::new(Vec::new(), width, height).unwrap();
        for frame in 0..15u8 {
            encoder.append(&bgrx(&vec![[frame * 10, 0, 0]; width * height]), width * 4, f64::from(frame) / 15.0).unwrap();
        }
        let decoded = decode(&encoder.finish(1.1).unwrap(), width, height);
        assert_eq!(decoded.delays.iter().map(|&delay| u32::from(delay)).sum::<u32>(), 110);
    }

    #[test]
    fn a_changed_frame_only_covers_the_changed_rectangle() {
        let (width, height) = (10, 8);
        let first = vec![[0u8, 0, 0]; width * height];
        let mut second = first.clone();
        second[2 * width + 3] = [255, 0, 0];
        second[5 * width + 6] = [0, 255, 0];
        let mut encoder = Encoder::new(Vec::new(), width, height).unwrap();
        encoder.append(&bgrx(&first), width * 4, 0.0).unwrap();
        encoder.append(&bgrx(&second), width * 4, 0.1).unwrap();
        let decoded = decode(&encoder.finish(0.2).unwrap(), width, height);
        assert_eq!(decoded.rects, vec![(0, 0, 10, 8), (3, 2, 4, 4)]);
        assert_eq!(decoded.frames[1], second);
    }

    #[test]
    fn rows_are_read_with_the_given_stride() {
        let (width, height, stride) = (3, 2, 16);
        let colors = [[1u8, 2, 3], [40, 50, 60], [70, 80, 90], [100, 110, 120], [130, 140, 150], [160, 170, 180]];
        let mut padded = vec![0xEEu8; stride * height];
        for (index, &[r, g, b]) in colors.iter().enumerate() {
            let offset = index / width * stride + index % width * 4;
            padded[offset..offset + 4].copy_from_slice(&[b, g, r, 0]);
        }
        let mut encoder = Encoder::new(Vec::new(), width, height).unwrap();
        encoder.append(&padded, stride, 0.0).unwrap();
        let decoded = decode(&encoder.finish(0.1).unwrap(), width, height);
        assert_eq!(decoded.frames[0], colors.to_vec());
    }
}
