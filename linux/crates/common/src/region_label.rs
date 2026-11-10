const COMMON: [(u32, u32); 7] = [(1, 1), (5, 4), (4, 3), (3, 2), (16, 10), (16, 9), (21, 9)];

fn common() -> impl Iterator<Item = (u32, u32)> {
    COMMON.into_iter().flat_map(|(a, b)| if a == b { vec![(a, b)] } else { vec![(a, b), (b, a)] })
}

pub fn decimal_separator() -> char {
    let locale = ["LC_ALL", "LC_NUMERIC", "LANG"]
        .iter()
        .find_map(|variable| std::env::var(variable).ok().filter(|value| !value.is_empty()))
        .unwrap_or_default();
    let language = locale.split(['_', '.', '@']).next().unwrap_or("");
    let point = ["", "C", "POSIX", "en", "ja", "ko", "zh", "he", "th", "ga", "cy", "mt"];
    if point.contains(&language) { '.' } else { ',' }
}

fn decimal(value: f64, separator: char) -> String {
    format!("{value:.2}").replace('.', &separator.to_string())
}

pub fn text(width: u32, height: u32, screen_width: u32, screen_height: u32, separator: char) -> String {
    let parts: Vec<String> = [
        Some(format!("{width} × {height} px")),
        ratio(width, height, separator),
        share(u64::from(width) * u64::from(height), u64::from(screen_width) * u64::from(screen_height)),
    ]
    .into_iter()
    .flatten()
    .collect();
    format!(" {} ", parts.join(" · "))
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

pub fn ratio(width: u32, height: u32, separator: char) -> Option<String> {
    if width == 0 || height == 0 {
        return None;
    }
    if let Some((a, b)) = common().find(|&(a, b)| u64::from(width) * u64::from(b) == u64::from(height) * u64::from(a)) {
        return Some(format!("{a}:{b}"));
    }
    let divisor = gcd(width, height);
    let (a, b) = (width / divisor, height / divisor);
    if a.max(b) <= 10 {
        return Some(format!("{a}:{b}"));
    }
    let value = f64::from(width) / f64::from(height);
    let distance = |(a, b): (u32, u32)| (value / (f64::from(a) / f64::from(b)) - 1.0).abs();
    let nearest = common().min_by(|&x, &y| distance(x).total_cmp(&distance(y)))?;
    if distance(nearest) <= 0.01 {
        return Some(format!("≈ {}:{}", nearest.0, nearest.1));
    }
    Some(if width >= height { format!("{}:1", decimal(value, separator)) } else { format!("1:{}", decimal(1.0 / value, separator)) })
}

pub fn share(area: u64, total: u64) -> Option<String> {
    if area == 0 || total == 0 {
        return None;
    }
    if area >= total {
        return Some("100 %".into());
    }
    let percent = (area as f64 * 100.0 / total as f64).round() as u64;
    Some(format!("{} %", percent.clamp(1, 99)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_label_joins_pixels_ratio_and_screen_share() {
        assert_eq!(text(1920, 1080, 3840, 2160, '.'), " 1920 × 1080 px · 16:9 · 25 % ");
    }

    #[test]
    fn an_empty_selection_shows_only_its_pixels() {
        assert_eq!(text(0, 540, 3840, 2160, '.'), " 0 × 540 px ");
    }

    #[test]
    fn common_ratios_keep_their_usual_names() {
        assert_eq!(ratio(1440, 900, '.').unwrap(), "16:10");
        assert_eq!(ratio(1080, 1920, '.').unwrap(), "9:16");
    }

    #[test]
    fn other_ratios_with_small_terms_are_exact() {
        assert_eq!(ratio(700, 500, '.').unwrap(), "7:5");
    }

    #[test]
    fn ratios_within_one_percent_of_a_common_one_are_approximated() {
        assert_eq!(ratio(1000, 557, '.').unwrap(), "≈ 16:9");
        assert_eq!(ratio(1000, 556, '.').unwrap(), "1.80:1");
        assert_eq!(ratio(1000, 556, ',').unwrap(), "1,80:1");
    }

    #[test]
    fn portrait_ratios_with_large_terms_are_decimal_over_one() {
        assert_eq!(ratio(556, 1000, '.').unwrap(), "1:1.80");
    }

    #[test]
    fn partial_selections_stay_between_one_and_ninety_nine_percent() {
        assert_eq!(share(1, 3840 * 2160).unwrap(), "1 %");
        assert_eq!(share(3839 * 2160, 3840 * 2160).unwrap(), "99 %");
        assert_eq!(share(3840 * 2160, 3840 * 2160).unwrap(), "100 %");
    }
}
