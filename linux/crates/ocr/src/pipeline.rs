const DET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const DET_STD: [f32; 3] = [0.229, 0.224, 0.225];
const DET_THRESHOLD: f32 = 0.3;
const BOX_THRESHOLD: f32 = 0.6;
const UNCLIP_RATIO: f64 = 1.5;
const MIN_SIZE: f64 = 3.0;
const SAME_ROW: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextBox {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub score: f32,
}

impl TextBox {
    fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    fn center_y(&self) -> f64 {
        (self.y0 + self.y1) / 2.0
    }
}

pub struct Image<'a> {
    pub rgba: &'a [u8],
    pub width: usize,
    pub height: usize,
}

impl Image<'_> {
    fn bgr(&self, x: f64, y: f64) -> [f32; 3] {
        let x = x.clamp(0.0, (self.width - 1) as f64);
        let y = y.clamp(0.0, (self.height - 1) as f64);
        let (left, top) = (x.floor() as usize, y.floor() as usize);
        let (right, bottom) = ((left + 1).min(self.width - 1), (top + 1).min(self.height - 1));
        let (fx, fy) = ((x - left as f64) as f32, (y - top as f64) as f32);
        let pixel = |px: usize, py: usize, channel: usize| f32::from(self.rgba[(py * self.width + px) * 4 + channel]);
        let mut bgr = [0.0; 3];
        for (index, channel) in [2usize, 1, 0].into_iter().enumerate() {
            let upper = pixel(left, top, channel) * (1.0 - fx) + pixel(right, top, channel) * fx;
            let lower = pixel(left, bottom, channel) * (1.0 - fx) + pixel(right, bottom, channel) * fx;
            bgr[index] = upper * (1.0 - fy) + lower * fy;
        }
        bgr
    }
}

fn multiple_of_32(value: f64) -> usize {
    (((value / 32.0).round() as usize) * 32).max(32)
}

pub fn detection_size(width: usize, height: usize, min_side: usize, max_side: usize) -> (usize, usize) {
    let (mut w, mut h) = (width as f64, height as f64);
    let shortest = w.min(h);
    if shortest < min_side as f64 {
        let ratio = min_side as f64 / shortest;
        w *= ratio;
        h *= ratio;
    }
    let longest = w.max(h);
    if longest > max_side as f64 {
        let ratio = max_side as f64 / longest;
        w *= ratio;
        h *= ratio;
    }
    (multiple_of_32(w), multiple_of_32(h))
}

pub fn detection_input(image: &Image, size: (usize, usize)) -> Vec<f32> {
    let (width, height) = size;
    let plane = width * height;
    let mut data = vec![0.0f32; 3 * plane];
    let (sx, sy) = (image.width as f64 / width as f64, image.height as f64 / height as f64);
    for y in 0..height {
        for x in 0..width {
            let bgr = image.bgr((x as f64 + 0.5) * sx - 0.5, (y as f64 + 0.5) * sy - 0.5);
            for channel in 0..3 {
                data[channel * plane + y * width + x] = (bgr[channel] / 255.0 - DET_MEAN[channel]) / DET_STD[channel];
            }
        }
    }
    data
}

pub fn boxes(probability: &[f32], width: usize, height: usize, scale: (f64, f64)) -> Vec<TextBox> {
    let mut labels = vec![0u32; width * height];
    let mut found = Vec::new();
    let mut stack = Vec::new();
    let mut next = 0u32;
    for start in 0..width * height {
        if labels[start] != 0 || probability[start] <= DET_THRESHOLD {
            continue;
        }
        next += 1;
        labels[start] = next;
        stack.push(start);
        let (mut x0, mut y0, mut x1, mut y1) = (start % width, start / width, start % width, start / width);
        while let Some(index) = stack.pop() {
            let (x, y) = (index % width, index / width);
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
            let neighbours = [(x > 0).then(|| index - 1), (x + 1 < width).then(|| index + 1), (y > 0).then(|| index - width), (y + 1 < height).then(|| index + width)];
            for neighbour in neighbours.into_iter().flatten() {
                if labels[neighbour] == 0 && probability[neighbour] > DET_THRESHOLD {
                    labels[neighbour] = next;
                    stack.push(neighbour);
                }
            }
        }
        let (w, h) = ((x1 - x0 + 1) as f64, (y1 - y0 + 1) as f64);
        if w.min(h) < MIN_SIZE {
            continue;
        }
        let mut sum = 0.0f64;
        for y in y0..=y1 {
            sum += probability[y * width + x0..=y * width + x1].iter().map(|value| f64::from(*value)).sum::<f64>();
        }
        let score = (sum / (w * h)) as f32;
        if score < BOX_THRESHOLD {
            continue;
        }
        let distance = w * h * UNCLIP_RATIO / (2.0 * (w + h));
        let (left, top) = (x0 as f64 - distance, y0 as f64 - distance);
        let (right, bottom) = (x1 as f64 + 1.0 + distance, y1 as f64 + 1.0 + distance);
        if (right - left).min(bottom - top) < MIN_SIZE + 2.0 {
            continue;
        }
        found.push(TextBox {
            x0: (left * scale.0).max(0.0),
            y0: (top * scale.1).max(0.0),
            x1: (right * scale.0).min(width as f64 * scale.0),
            y1: (bottom * scale.1).min(height as f64 * scale.1),
            score,
        });
    }
    found
}

pub fn sort_boxes(boxes: &mut [TextBox]) {
    boxes.sort_by(|a, b| a.y0.total_cmp(&b.y0).then(a.x0.total_cmp(&b.x0)));
    for i in 0..boxes.len().saturating_sub(1) {
        for j in (0..=i).rev() {
            if (boxes[j + 1].y0 - boxes[j].y0).abs() < SAME_ROW && boxes[j + 1].x0 < boxes[j].x0 {
                boxes.swap(j, j + 1);
            } else {
                break;
            }
        }
    }
}

pub fn recognition_input(image: &Image, text: &TextBox, height: usize, min_width: usize, max_width: usize) -> (Vec<f32>, usize) {
    let (crop_width, crop_height) = ((text.x1 - text.x0).max(1.0), (text.y1 - text.y0).max(1.0));
    let resized = ((height as f64 * crop_width / crop_height).ceil() as usize).clamp(1, max_width);
    let width = resized.max(min_width);
    let plane = width * height;
    let mut data = vec![0.0f32; 3 * plane];
    let (sx, sy) = (crop_width / resized as f64, crop_height / height as f64);
    for y in 0..height {
        for x in 0..resized {
            let bgr = image.bgr(text.x0 + (x as f64 + 0.5) * sx - 0.5, text.y0 + (y as f64 + 0.5) * sy - 0.5);
            for channel in 0..3 {
                data[channel * plane + y * width + x] = (bgr[channel] / 255.0 - 0.5) / 0.5;
            }
        }
    }
    (data, width)
}

pub fn decode(probabilities: &[f32], steps: usize, classes: usize, dictionary: &[String]) -> (String, f32) {
    let mut text = String::new();
    let (mut total, mut kept) = (0.0f32, 0usize);
    let mut previous = 0usize;
    for step in 0..steps {
        let row = &probabilities[step * classes..(step + 1) * classes];
        let (best, score) = row.iter().enumerate().fold((0, f32::MIN), |found, (index, value)| if *value > found.1 { (index, *value) } else { found });
        if best != 0 && best != previous {
            match dictionary.get(best - 1) {
                Some(character) => text.push_str(character),
                None => text.push(' '),
            }
            total += score;
            kept += 1;
        }
        previous = best;
    }
    (text, if kept == 0 { 0.0 } else { total / kept as f32 })
}

pub fn assemble(mut lines: Vec<(TextBox, String)>) -> String {
    lines.retain(|(_, text)| !text.trim().is_empty());
    let mut rows: Vec<Vec<(TextBox, String)>> = Vec::new();
    for (text_box, text) in lines {
        let row = rows.iter_mut().find(|row| {
            let first = &row[0].0;
            (first.center_y() - text_box.center_y()).abs() < first.height().min(text_box.height()) / 2.0
        });
        match row {
            Some(row) => row.push((text_box, text)),
            None => rows.push(vec![(text_box, text)]),
        }
    }
    rows.sort_by(|a, b| a[0].0.center_y().total_cmp(&b[0].0.center_y()));
    rows.into_iter()
        .map(|mut row| {
            row.sort_by(|a, b| a.0.x0.total_cmp(&b.0.x0));
            row.into_iter().map(|(_, text)| text.trim().to_string()).collect::<Vec<_>>().join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_box(x0: f64, y0: f64, x1: f64, y1: f64) -> TextBox {
        TextBox { x0, y0, x1, y1, score: 1.0 }
    }

    #[test]
    fn detection_input_sizes_are_multiples_of_32_within_limits() {
        assert_eq!(detection_size(1000, 50, 64, 2560), (1280, 64));
        assert_eq!(detection_size(3840, 2160, 64, 2560), (2560, 1440));
        assert_eq!(detection_size(100, 100, 64, 2560), (96, 96));
    }

    #[test]
    fn detection_input_is_normalized_bgr_planes() {
        let rgba = [255u8, 0, 0, 255].repeat(4);
        let image = Image { rgba: &rgba, width: 2, height: 2 };
        let data = detection_input(&image, (2, 2));
        assert!((data[0] - (0.0 - DET_MEAN[0]) / DET_STD[0]).abs() < 1e-5);
        assert!((data[8] - (1.0 - DET_MEAN[2]) / DET_STD[2]).abs() < 1e-5);
    }

    #[test]
    fn confident_regions_become_unclipped_scaled_boxes() {
        let (width, height) = (40, 20);
        let mut probability = vec![0.05f32; width * height];
        for y in 4..10 {
            for x in 5..25 {
                probability[y * width + x] = 0.9;
            }
        }
        for y in 14..18 {
            for x in 30..38 {
                probability[y * width + x] = 0.4;
            }
        }
        let found = boxes(&probability, width, height, (2.0, 2.0));
        assert_eq!(found.len(), 1);
        let distance = 20.0 * 6.0 * 1.5 / (2.0 * 26.0);
        let expected = text_box((5.0 - distance) * 2.0, (4.0 - distance) * 2.0, (25.0 + distance) * 2.0, (10.0 + distance) * 2.0);
        assert!((found[0].x0 - expected.x0).abs() < 1e-9 && (found[0].y1 - expected.y1).abs() < 1e-9);
    }

    #[test]
    fn boxes_on_one_row_read_left_to_right_before_the_next_row() {
        let mut found = vec![text_box(300.0, 12.0, 400.0, 30.0), text_box(10.0, 15.0, 100.0, 32.0), text_box(10.0, 60.0, 100.0, 80.0)];
        sort_boxes(&mut found);
        assert_eq!(found.iter().map(|found| found.x0 as i32).collect::<Vec<_>>(), vec![10, 300, 10]);
        assert_eq!(found[2].y0, 60.0);
    }

    #[test]
    fn recognition_crops_keep_their_aspect_and_pad_to_the_minimum_width() {
        let rgba = [255u8; 4 * 100 * 100];
        let image = Image { rgba: &rgba, width: 100, height: 100 };
        let (_, wide) = recognition_input(&image, &text_box(0.0, 0.0, 100.0, 10.0), 48, 320, 3200);
        assert_eq!(wide, 480);
        let (data, narrow) = recognition_input(&image, &text_box(0.0, 0.0, 10.0, 10.0), 48, 320, 3200);
        assert_eq!(narrow, 320);
        assert!((data[0] - 1.0).abs() < 1e-6);
        assert_eq!(data[100], 0.0);
    }

    #[test]
    fn ctc_decoding_collapses_repeats_and_skips_blanks() {
        let dictionary: Vec<String> = ["h", "o", "l", "a"].iter().map(|s| s.to_string()).collect();
        let classes = dictionary.len() + 2;
        let sequence = [1usize, 1, 2, 0, 3, 0, 3, 4, 5, 0];
        let mut probabilities = vec![0.0f32; sequence.len() * classes];
        for (step, class) in sequence.iter().enumerate() {
            probabilities[step * classes + class] = 0.9;
        }
        let (text, score) = decode(&probabilities, sequence.len(), classes, &dictionary);
        assert_eq!(text, "holla ");
        assert!((score - 0.9).abs() < 1e-6);
    }

    #[test]
    fn lines_join_boxes_of_a_row_and_break_between_rows() {
        let lines = vec![
            (text_box(200.0, 10.0, 300.0, 30.0), "mundo".to_string()),
            (text_box(10.0, 12.0, 150.0, 31.0), "Hola".to_string()),
            (text_box(10.0, 50.0, 150.0, 70.0), "Adiós".to_string()),
            (text_box(10.0, 90.0, 150.0, 110.0), "  ".to_string()),
        ];
        assert_eq!(assemble(lines), "Hola mundo\nAdiós");
    }
}
