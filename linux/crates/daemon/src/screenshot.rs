use crate::dbus::{Options, Results};
use sharek_common::image::RgbImage;
use std::io::Read;
use std::os::fd::AsFd;
use zbus::blocking::Connection;
use zbus::zvariant::{Fd, Value};

pub struct Shot {
    pub image: RgbImage,
    pub scale: f64,
}

#[derive(Clone, Copy)]
pub enum Target<'a> {
    ActiveScreen,
    ActiveWindow,
    Screen(&'a str),
    Area { x: i32, y: i32, width: u32, height: u32 },
}

fn number(results: &Results, key: &str) -> Option<u32> {
    u32::try_from(results.get(key)?).ok()
}

pub fn capture(connection: &Connection, target: Target, include_cursor: bool) -> Result<Shot, String> {
    let (data, width, height, scale) =
        capture_with(connection, target, include_cursor, |width, height| Ok(vec![0u8; width as usize * height as usize * 3]))?;
    Ok(Shot { image: RgbImage { width, height, data: data.into() }, scale })
}

pub fn capture_with<T: AsMut<[u8]>>(
    connection: &Connection,
    target: Target,
    include_cursor: bool,
    allocate: impl FnOnce(u32, u32) -> Result<T, String>,
) -> Result<(T, u32, u32, f64), String> {
    let (mut reader, writer) = std::io::pipe().map_err(|error| error.to_string())?;
    let mut options = Options::new();
    options.insert("include-cursor", Value::from(include_cursor));
    options.insert("native-resolution", Value::from(true));
    let pipe = Fd::from(writer.as_fd());
    let (service, path, interface) = ("org.kde.KWin", "/org/kde/KWin/ScreenShot2", Some("org.kde.KWin.ScreenShot2"));
    let reply = match target {
        Target::ActiveScreen => connection.call_method(Some(service), path, interface, "CaptureActiveScreen", &(&options, &pipe)),
        Target::ActiveWindow => {
            options.insert("include-decoration", Value::from(true));
            options.insert("include-shadow", Value::from(false));
            connection.call_method(Some(service), path, interface, "CaptureActiveWindow", &(&options, &pipe))
        }
        Target::Screen(name) => connection.call_method(Some(service), path, interface, "CaptureScreen", &(name, &options, &pipe)),
        Target::Area { x, y, width, height } => {
            options.insert("hide-caller-windows", Value::from(true));
            connection.call_method(Some(service), path, interface, "CaptureArea", &(x, y, width, height, &options, &pipe))
        }
    };
    drop(pipe);
    drop(writer);
    let results: Results = reply.map_err(|error| error.to_string())?.body().deserialize().map_err(|error| error.to_string())?;
    let width = number(&results, "width").ok_or("missing width")?;
    let height = number(&results, "height").ok_or("missing height")?;
    let stride = number(&results, "stride").ok_or("missing stride")? as usize;
    let format = number(&results, "format").ok_or("missing format")?;
    let scale = results.get("scale").and_then(|value| f64::try_from(value).ok()).unwrap_or(1.0);
    let order: [usize; 3] = match format {
        4..=6 => [2, 1, 0],
        16..=18 => [0, 1, 2],
        other => return Err(format!("KWin devolvió un formato de imagen no soportado ({other})")),
    };
    if stride < width as usize * 4 {
        return Err("KWin devolvió una imagen con un stride inválido".into());
    }
    let mut destination = allocate(width, height)?;
    let pixels = destination.as_mut();
    let row_bytes = width as usize * 3;
    let mut row = vec![0u8; stride];
    for line in pixels.chunks_exact_mut(row_bytes).take(height as usize) {
        reader.read_exact(&mut row).map_err(|_| "KWin devolvió una imagen incompleta".to_string())?;
        for (target, source) in line.as_chunks_mut::<3>().0.iter_mut().zip(row.as_chunks::<4>().0) {
            target[0] = source[order[0]];
            target[1] = source[order[1]];
            target[2] = source[order[2]];
        }
    }
    Ok((destination, width, height, scale))
}
