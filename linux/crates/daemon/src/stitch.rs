use sharek_common::image::RgbImage;
use std::collections::HashMap;
use std::ops::Range;

const WINDOW: usize = 16;
const AGREEMENT: usize = 90;
const SAME_FRAME: usize = 97;
const MOVING_COLUMN: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Started,
    Appended(usize),
    Unchanged,
    Reversed,
    NoMatch,
}

struct Rows {
    hashes: Vec<u64>,
    distinctive: Vec<bool>,
}

pub struct Stitcher {
    ignored_trailing: usize,
    columns: Option<Range<usize>>,
    first: Option<RgbImage>,
    last: Option<RgbImage>,
    previous: Option<Rows>,
    pieces: Vec<RgbImage>,
    footer: Option<usize>,
    height: usize,
}

fn crop_rows(image: &RgbImage, top: usize, count: usize) -> RgbImage {
    let row = image.width as usize * 3;
    RgbImage { width: image.width, height: count as u32, data: image.data[top * row..(top + count) * row].to_vec().into() }
}

fn rows(image: &RgbImage, columns: Range<usize>) -> Rows {
    let stride = image.width as usize * 3;
    let mut hashes = Vec::with_capacity(image.height as usize);
    let mut distinctive = Vec::with_capacity(image.height as usize);
    for line in image.data.chunks_exact(stride) {
        let pixels = &line[columns.start * 3..columns.end * 3];
        let mut hash = 0xCBF2_9CE4_8422_2325u64;
        for pixel in pixels.as_chunks::<3>().0 {
            let value = u64::from(pixel[0]) | (u64::from(pixel[1]) << 8) | (u64::from(pixel[2]) << 16);
            hash = (hash ^ value).wrapping_mul(0x0000_0100_0000_01B3);
        }
        hashes.push(hash);
        distinctive.push(pixels.as_chunks::<3>().0.iter().any(|pixel| pixel != &pixels[..3]));
    }
    Rows { hashes, distinctive }
}

fn moving_columns(before: &RgbImage, after: &RgbImage, ignored_trailing: usize) -> Option<Range<usize>> {
    let (width, height) = (before.width as usize, before.height as usize);
    let mut changed = vec![0usize; width];
    for (old, new) in before.data.chunks_exact(width * 3).zip(after.data.chunks_exact(width * 3)) {
        for (column, (a, b)) in old.as_chunks::<3>().0.iter().zip(new.as_chunks::<3>().0).enumerate() {
            if a != b {
                changed[column] += 1;
            }
        }
    }
    let moving = |column: &usize| changed[*column] * 100 >= height * MOVING_COLUMN;
    let start = (0..width).find(moving)?;
    let end = (0..width).rev().find(moving)? + 1;
    Some(start..(end.saturating_sub(ignored_trailing)).max(start + 1))
}

fn windows(rows: &Rows) -> Vec<Option<u64>> {
    if rows.hashes.len() < WINDOW {
        return Vec::new();
    }
    (0..=rows.hashes.len() - WINDOW)
        .map(|start| {
            let slice = &rows.hashes[start..start + WINDOW];
            let varied = slice.iter().any(|hash| *hash != slice[0]);
            if !rows.distinctive[start..start + WINDOW].contains(&true) || !varied {
                return None;
            }
            Some(slice.iter().fold(0x9E37_79B9_7F4A_7C15u64, |hash, row| (hash ^ row).wrapping_mul(0x0000_0100_0000_01B3)))
        })
        .collect()
}

fn offset(previous: &Rows, current: &Rows) -> Option<usize> {
    let mut positions: HashMap<u64, Vec<usize>> = HashMap::new();
    for (index, hash) in windows(previous).into_iter().enumerate() {
        if let Some(hash) = hash {
            positions.entry(hash).or_default().push(index);
        }
    }
    let mut votes: HashMap<usize, usize> = HashMap::new();
    let mut candidates = 0;
    for (index, hash) in windows(current).into_iter().enumerate() {
        let Some(matches) = hash.and_then(|hash| positions.get(&hash)) else { continue };
        if matches.len() > 3 {
            continue;
        }
        candidates += 1;
        for &found in matches.iter().filter(|&&found| found > index) {
            *votes.entry(found - index).or_default() += 1;
        }
    }
    let (best, count) = votes.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))?;
    (count >= (candidates / 5).max(4) && overlap_agrees(previous, current, best)).then_some(best)
}

fn overlap_agrees(previous: &Rows, current: &Rows, moved: usize) -> bool {
    let height = current.hashes.len().min(previous.hashes.len());
    let (mut compared, mut agreed) = (0, 0);
    for row in 0..height.saturating_sub(moved) {
        let fixed = |at: usize| current.hashes[at] == previous.hashes[at];
        if !current.distinctive[row] || fixed(row) || fixed(row + moved) {
            continue;
        }
        compared += 1;
        if current.hashes[row] == previous.hashes[row + moved] {
            agreed += 1;
        }
    }
    compared >= 4 && agreed * 100 >= compared * AGREEMENT
}

fn below_content(previous: &Rows, current: &Rows, moved: usize) -> usize {
    let height = previous.hashes.len();
    (moved..height)
        .rev()
        .find(|&row| previous.distinctive[row] && previous.hashes[row] == current.hashes[row - moved])
        .map_or(0, |row| height - 1 - row)
}

fn same_frame(previous: &Rows, current: &Rows) -> bool {
    let same = previous.hashes.iter().zip(&current.hashes).filter(|(a, b)| a == b).count();
    same * 100 >= current.hashes.len() * SAME_FRAME
}

impl Stitcher {
    pub fn new(ignored_trailing: usize) -> Stitcher {
        Stitcher { ignored_trailing, columns: None, first: None, last: None, previous: None, pieces: Vec::new(), footer: None, height: 0 }
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn add(&mut self, image: RgbImage) -> Outcome {
        let whole = 0..(image.width as usize).saturating_sub(self.ignored_trailing).max(1);
        let (Some(previous), Some(reference)) = (self.previous.as_ref(), self.last.as_ref().or(self.first.as_ref())) else {
            self.height = image.height as usize;
            self.previous = Some(rows(&image, whole));
            self.first = Some(image);
            return Outcome::Started;
        };
        if image.width != reference.width || image.height != reference.height {
            return Outcome::NoMatch;
        }
        let columns = match self.columns.clone() {
            Some(columns) => columns,
            None => {
                if same_frame(previous, &rows(&image, whole)) {
                    return Outcome::Unchanged;
                }
                let Some(columns) = moving_columns(reference, &image, self.ignored_trailing) else { return Outcome::Unchanged };
                self.previous = Some(rows(reference, columns.clone()));
                self.columns = Some(columns.clone());
                columns
            }
        };
        let previous = self.previous.as_ref().expect("rows of the reference frame");
        let current = rows(&image, columns);
        if same_frame(previous, &current) {
            return Outcome::Unchanged;
        }
        let frame_height = image.height as usize;
        let Some(moved) = offset(previous, &current) else {
            return if offset(&current, previous).is_some() { Outcome::Reversed } else { Outcome::NoMatch };
        };

        let footer = self.footer.unwrap_or_else(|| below_content(previous, &current, moved).max((frame_height * 6 / 100).max(8)));
        if moved + footer + WINDOW > frame_height {
            return Outcome::NoMatch;
        }
        if self.footer.is_none() {
            if let Some(first) = self.first.take() {
                self.pieces.push(crop_rows(&first, 0, frame_height - footer));
            }
            self.footer = Some(footer);
        }
        self.pieces.push(crop_rows(&image, frame_height - footer - moved, moved));
        self.previous = Some(current);
        self.last = Some(image);
        self.height = self.pieces.iter().map(|piece| piece.height as usize).sum::<usize>() + footer;
        Outcome::Appended(moved)
    }

    pub fn compose(mut self) -> Option<RgbImage> {
        if self.pieces.is_empty() {
            return self.first.take().or(self.last.take());
        }
        let last = self.last.take()?;
        let footer = self.footer.unwrap_or(0);
        let width = last.width;
        let mut data = Vec::with_capacity(self.height * width as usize * 3);
        for piece in self.pieces.drain(..) {
            data.extend_from_slice(&piece.data);
        }
        let row = width as usize * 3;
        data.extend_from_slice(&last.data[(last.height as usize - footer) * row..]);
        let height = (data.len() / row) as u32;
        Some(RgbImage { width, height, data: data.into() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: u32 = 64;
    const SCROLLBAR: usize = 4;

    fn content_row(index: usize) -> Vec<u8> {
        if index % 7 == 3 {
            return [240u8, 240, 240].repeat(WIDTH as usize);
        }
        let mut state = (index as u32).wrapping_mul(2_654_435_761).wrapping_add(12_345);
        (0..WIDTH as usize * 3)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            })
            .collect()
    }

    fn fixed_row(tag: u8) -> Vec<u8> {
        (0..WIDTH as usize * 3).map(|byte| tag.wrapping_add(byte as u8)).collect()
    }

    struct Page {
        header: usize,
        footer: usize,
        viewport: usize,
    }

    impl Page {
        fn frame(&self, offset: usize, scrollbar: u8) -> RgbImage {
            let mut data = Vec::new();
            for row in 0..self.header {
                data.extend(fixed_row(row as u8));
            }
            for row in 0..self.viewport {
                let mut line = content_row(offset + row);
                let start = (WIDTH as usize - SCROLLBAR) * 3;
                line[start..].fill(scrollbar);
                data.extend(line);
            }
            for row in 0..self.footer {
                data.extend(fixed_row(100 + row as u8));
            }
            RgbImage { width: WIDTH, height: (self.header + self.viewport + self.footer) as u32, data: data.into() }
        }

        fn document(&self, content: usize, scrollbar: u8) -> Vec<Vec<u8>> {
            let mut rows: Vec<Vec<u8>> = (0..self.header).map(|row| fixed_row(row as u8)).collect();
            for row in 0..content {
                let mut line = content_row(row);
                let start = (WIDTH as usize - SCROLLBAR) * 3;
                line[start..].fill(scrollbar);
                rows.push(line);
            }
            rows.extend((0..self.footer).map(|row| fixed_row(100 + row as u8)));
            rows
        }
    }

    fn lines(image: &RgbImage) -> Vec<Vec<u8>> {
        image.data.chunks_exact(image.width as usize * 3).map(<[u8]>::to_vec).collect()
    }

    #[test]
    fn scrolled_frames_rebuild_the_page_with_fixed_header_and_footer() {
        let page = Page { header: 20, footer: 30, viewport: 250 };
        let mut stitcher = Stitcher::new(SCROLLBAR);
        let offsets = [0, 137, 274, 411];
        let mut outcomes = Vec::new();
        for (step, offset) in offsets.iter().enumerate() {
            outcomes.push(stitcher.add(page.frame(*offset, step as u8 * 40)));
        }
        assert_eq!(outcomes, vec![Outcome::Started, Outcome::Appended(137), Outcome::Appended(137), Outcome::Appended(137)]);
        let stitched = lines(&stitcher.compose().unwrap());
        let expected = page.document(411 + 250, 0);
        assert_eq!(stitched.len(), expected.len());
        let scrollbar_free = |rows: &[Vec<u8>]| rows.iter().map(|row| row[..(WIDTH as usize - SCROLLBAR) * 3].to_vec()).collect::<Vec<_>>();
        assert_eq!(scrollbar_free(&stitched), scrollbar_free(&expected));
    }

    #[test]
    fn an_unscrolled_frame_is_unchanged() {
        let page = Page { header: 0, footer: 0, viewport: 200 };
        let mut stitcher = Stitcher::new(SCROLLBAR);
        stitcher.add(page.frame(0, 0));
        assert_eq!(stitcher.add(page.frame(0, 9)), Outcome::Unchanged);
    }

    #[test]
    fn scrolling_the_wrong_way_is_reported_as_reversed() {
        let page = Page { header: 0, footer: 0, viewport: 200 };
        let mut stitcher = Stitcher::new(SCROLLBAR);
        stitcher.add(page.frame(300, 0));
        assert_eq!(stitcher.add(page.frame(200, 0)), Outcome::Reversed);
    }

    #[test]
    fn an_unrelated_frame_does_not_match() {
        let page = Page { header: 0, footer: 0, viewport: 200 };
        let mut stitcher = Stitcher::new(SCROLLBAR);
        stitcher.add(page.frame(0, 0));
        assert_eq!(stitcher.add(page.frame(5000, 0)), Outcome::NoMatch);
    }

    fn frame_of(rows: &[Vec<u8>]) -> RgbImage {
        RgbImage { width: WIDTH, height: rows.len() as u32, data: rows.concat().into() }
    }

    #[test]
    fn a_shared_block_without_an_agreeing_overlap_does_not_match() {
        let block: Vec<Vec<u8>> = (0..24).map(|row| content_row(9000 + row)).collect();
        let mut previous: Vec<Vec<u8>> = (0..100).map(content_row).collect();
        previous.splice(60..84, block.clone());
        let mut current: Vec<Vec<u8>> = (0..100).map(|row| content_row(5000 + row)).collect();
        current.splice(20..44, block);
        let mut stitcher = Stitcher::new(SCROLLBAR);
        stitcher.add(frame_of(&previous));
        assert_eq!(stitcher.add(frame_of(&current)), Outcome::NoMatch);
    }

    #[test]
    fn a_frame_that_differs_in_one_row_is_unchanged() {
        let page = Page { header: 0, footer: 0, viewport: 200 };
        let mut stitcher = Stitcher::new(SCROLLBAR);
        stitcher.add(page.frame(0, 0));
        let mut blinking = page.frame(0, 0);
        let row = WIDTH as usize * 3;
        blinking.data[50 * row..51 * row].fill(7);
        assert_eq!(stitcher.add(blinking), Outcome::Unchanged);
    }

    #[test]
    fn a_changing_row_in_the_fixed_footer_keeps_the_footer_out_of_the_seams() {
        let page = Page { header: 0, footer: 40, viewport: 200 };
        let mut stitcher = Stitcher::new(SCROLLBAR);
        stitcher.add(page.frame(0, 0));
        let mut blinking = page.frame(100, 0);
        let row = WIDTH as usize * 3;
        blinking.data[210 * row..211 * row].fill(7);
        assert_eq!(stitcher.add(blinking), Outcome::Appended(100));
        assert_eq!(stitcher.add(page.frame(200, 0)), Outcome::Appended(100));
        let stitched = lines(&stitcher.compose().unwrap());
        let scrollbar_free = |rows: &[Vec<u8>]| rows.iter().map(|row| row[..(WIDTH as usize - SCROLLBAR) * 3].to_vec()).collect::<Vec<_>>();
        assert_eq!(scrollbar_free(&stitched), scrollbar_free(&page.document(400, 0)));
    }

    #[test]
    fn a_move_that_leaves_no_overlap_above_the_footer_does_not_match() {
        let page = Page { header: 0, footer: 0, viewport: 200 };
        let mut stitcher = Stitcher::new(SCROLLBAR);
        stitcher.add(page.frame(0, 0));
        assert_eq!(stitcher.add(page.frame(180, 0)), Outcome::NoMatch);
    }

    #[test]
    fn a_fixed_sidebar_beside_the_scrolling_content_still_matches() {
        let page = Page { header: 0, footer: 0, viewport: 200 };
        let sidebar = |mut image: RgbImage| {
            let row = WIDTH as usize * 3;
            for (index, line) in image.data.chunks_exact_mut(row).enumerate() {
                line[..60].copy_from_slice(&fixed_row(index as u8)[..60]);
            }
            image
        };
        let mut stitcher = Stitcher::new(SCROLLBAR);
        stitcher.add(sidebar(page.frame(0, 0)));
        assert_eq!(stitcher.add(sidebar(page.frame(70, 0))), Outcome::Appended(70));
        assert_eq!(stitcher.add(sidebar(page.frame(140, 0))), Outcome::Appended(70));
    }

    #[test]
    fn a_single_frame_composes_to_itself() {
        let page = Page { header: 10, footer: 10, viewport: 100 };
        let frame = page.frame(0, 0);
        let mut stitcher = Stitcher::new(SCROLLBAR);
        stitcher.add(frame.clone());
        assert_eq!(stitcher.compose().unwrap().data, frame.data);
    }
}
