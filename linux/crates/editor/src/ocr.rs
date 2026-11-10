use crate::app::with;
use sharek_common::image::RgbImage;
use sharek_common::ocr;
use std::sync::Arc;

pub fn recognize(image: Arc<RgbImage>, scale: f32) {
    std::thread::spawn(move || {
        let result = ocr::recognize(&image, f64::from(scale));
        let _ = slint::invoke_from_event_loop(move || {
            with(move |c| match result {
                Ok(text) => c.show_text(text),
                Err(error) => c.notify("No se pudo reconocer el texto", &error),
            })
        });
    });
}
