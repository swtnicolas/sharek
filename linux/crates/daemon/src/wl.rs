use crate::app::App;
use smithay_client_toolkit::compositor::{CompositorState, Region};
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::error::GlobalError;
use smithay_client_toolkit::globals::{GlobalData, ProvidesBoundGlobal};
use smithay_client_toolkit::shell::wlr_layer::LayerShell;
use smithay_client_toolkit::shm::Shm;
use smithay_client_toolkit::subcompositor::SubcompositorState;
use wayland_client::globals::GlobalList;
use wayland_client::protocol::{wl_buffer::WlBuffer, wl_pointer::WlPointer, wl_surface::WlSurface};
use wayland_client::{Connection, Proxy, QueueHandle};
use wayland_protocols::wp::alpha_modifier::v1::client::{wp_alpha_modifier_surface_v1::WpAlphaModifierSurfaceV1, wp_alpha_modifier_v1::WpAlphaModifierV1};
use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1},
    wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use wayland_protocols::wp::single_pixel_buffer::v1::client::wp_single_pixel_buffer_manager_v1::WpSinglePixelBufferManagerV1;
use wayland_protocols::wp::pointer_gestures::zv1::client::zwp_pointer_gesture_pinch_v1::{self, ZwpPointerGesturePinchV1};
use wayland_protocols::wp::pointer_gestures::zv1::client::zwp_pointer_gestures_v1::ZwpPointerGesturesV1;
use wayland_protocols::wp::viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter};
use wayland_protocols::xdg::shell::client::xdg_wm_base::XdgWmBase;
use wayland_protocols_plasma::fake_input::client::org_kde_kwin_fake_input::OrgKdeKwinFakeInput;
use wayland_protocols_plasma::screencast::v1::client::zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1;

pub struct Globals {
    pub compositor: CompositorState,
    pub subcompositor: SubcompositorState,
    pub shm: Shm,
    pub layer_shell: LayerShell,
    pub viewporter: WpViewporter,
    pub single_pixel: WpSinglePixelBufferManagerV1,
    pub fractional: Option<WpFractionalScaleManagerV1>,
    pub cursor_shape: Option<WpCursorShapeManagerV1>,
    pub alpha: Option<WpAlphaModifierV1>,
    pub screencast: Option<ZkdeScreencastUnstableV1>,
    pub fake_input: Option<OrgKdeKwinFakeInput>,
    pub gestures: Option<ZwpPointerGesturesV1>,
}

pub struct Pinch;

pub struct Data;
pub struct PixelBuffer;
pub struct WmBase(XdgWmBase);

impl WmBase {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<App>) -> Option<WmBase> {
        globals.bind(qh, 1..=5, GlobalData).ok().map(WmBase)
    }
}

impl ProvidesBoundGlobal<XdgWmBase, 5> for WmBase {
    fn bound_global(&self) -> Result<XdgWmBase, GlobalError> {
        Ok(self.0.clone())
    }
}

impl ProvidesBoundGlobal<XdgWmBase, 6> for WmBase {
    fn bound_global(&self) -> Result<XdgWmBase, GlobalError> {
        Ok(self.0.clone())
    }
}
pub struct FractionalScale(pub WlSurface);

impl Globals {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<App>) -> Result<Globals, String> {
        let compositor = CompositorState::bind(globals, qh).map_err(|error| error.to_string())?;
        let subcompositor = SubcompositorState::bind(compositor.wl_compositor().clone(), globals, qh).map_err(|error| error.to_string())?;
        Ok(Globals {
            subcompositor,
            shm: Shm::bind(globals, qh).map_err(|error| error.to_string())?,
            layer_shell: LayerShell::bind(globals, qh).map_err(|error| error.to_string())?,
            viewporter: globals.bind(qh, 1..=1, Data).map_err(|error| error.to_string())?,
            single_pixel: globals.bind(qh, 1..=1, Data).map_err(|error| error.to_string())?,
            fractional: globals.bind(qh, 1..=1, Data).ok(),
            cursor_shape: globals.bind(qh, 1..=1, Data).ok(),
            alpha: globals.bind(qh, 1..=1, Data).ok(),
            screencast: globals.bind(qh, 3..=3, Data).ok(),
            fake_input: globals.bind(qh, 4..=4, Data).ok().inspect(|input: &OrgKdeKwinFakeInput| input.authenticate("ShareK".into(), "Captura con desplazamiento".into())),
            gestures: globals.bind(qh, 1..=3, Data).ok(),
            compositor,
        })
    }

    pub fn solid(&self, rgba: [f64; 4], qh: &QueueHandle<App>) -> WlBuffer {
        let channel = |value: f64| (value.clamp(0.0, 1.0) * f64::from(u32::MAX)) as u32;
        let alpha = rgba[3];
        self.single_pixel.create_u32_rgba_buffer(channel(rgba[0] * alpha), channel(rgba[1] * alpha), channel(rgba[2] * alpha), channel(alpha), qh, PixelBuffer)
    }

    pub fn viewport(&self, surface: &WlSurface, qh: &QueueHandle<App>) -> WpViewport {
        self.viewporter.get_viewport(surface, qh, Data)
    }

    pub fn fractional_scale(&self, surface: &WlSurface, qh: &QueueHandle<App>) -> Option<WpFractionalScaleV1> {
        self.fractional.as_ref().map(|manager| manager.get_fractional_scale(surface, qh, FractionalScale(surface.clone())))
    }

    pub fn alpha_surface(&self, surface: &WlSurface, qh: &QueueHandle<App>) -> Option<WpAlphaModifierSurfaceV1> {
        self.alpha.as_ref().map(|manager| manager.get_surface(surface, qh, Data))
    }

    pub fn no_input(&self, surface: &WlSurface) {
        if let Ok(region) = Region::new(&self.compositor) {
            surface.set_input_region(Some(region.wl_region()));
        }
    }

    pub fn input_outside(&self, surface: &WlSurface, size: (u32, u32), hole: Option<(i32, i32, i32, i32)>) {
        if let Ok(region) = Region::new(&self.compositor) {
            region.add(0, 0, size.0 as i32, size.1 as i32);
            if let Some((x, y, width, height)) = hole {
                region.subtract(x, y, width, height);
            }
            surface.set_input_region(Some(region.wl_region()));
        }
    }

    pub fn cursor_device(&self, pointer: &WlPointer, qh: &QueueHandle<App>) -> Option<WpCursorShapeDeviceV1> {
        self.cursor_shape.as_ref().map(|manager| manager.get_pointer(pointer, qh, Data))
    }

    pub fn pinch(&self, pointer: &WlPointer, qh: &QueueHandle<App>) -> Option<ZwpPointerGesturePinchV1> {
        self.gestures.as_ref().map(|gestures| gestures.get_pinch_gesture(pointer, qh, Pinch))
    }
}

pub fn set_cursor(device: Option<&WpCursorShapeDeviceV1>, serial: u32, shape: Shape) {
    if let Some(device) = device {
        device.set_shape(serial, shape);
    }
}

macro_rules! ignore_events {
    ($($interface:ty),* $(,)?) => {
        $(impl Dispatch2<$interface, App> for Data {
            fn event(&self, _: &mut App, _: &$interface, _: <$interface as Proxy>::Event, _: &Connection, _: &QueueHandle<App>) {}
        })*
    };
}

ignore_events!(WpViewporter, WpViewport, WpSinglePixelBufferManagerV1, WpFractionalScaleManagerV1, WpCursorShapeManagerV1, WpCursorShapeDeviceV1, WpAlphaModifierV1, WpAlphaModifierSurfaceV1, ZkdeScreencastUnstableV1, OrgKdeKwinFakeInput, ZwpPointerGesturesV1);

impl Dispatch2<ZwpPointerGesturePinchV1, App> for Pinch {
    fn event(&self, app: &mut App, _: &ZwpPointerGesturePinchV1, event: zwp_pointer_gesture_pinch_v1::Event, connection: &Connection, qh: &QueueHandle<App>) {
        match event {
            zwp_pointer_gesture_pinch_v1::Event::Begin { surface, .. } => app.pins.pinch_begin(&surface),
            zwp_pointer_gesture_pinch_v1::Event::Update { scale, .. } => app.pins.pinch_update(scale, connection, qh),
            zwp_pointer_gesture_pinch_v1::Event::End { .. } => app.pins.pinch_end(),
            _ => {}
        }
    }
}

impl Dispatch2<WlBuffer, App> for PixelBuffer {
    fn event(&self, _: &mut App, _: &WlBuffer, _: <WlBuffer as Proxy>::Event, _: &Connection, _: &QueueHandle<App>) {}
}

impl Dispatch2<WpFractionalScaleV1, App> for FractionalScale {
    fn event(&self, app: &mut App, _: &WpFractionalScaleV1, event: wp_fractional_scale_v1::Event, _: &Connection, qh: &QueueHandle<App>) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            app.surface_scale(&self.0, f64::from(scale) / 120.0, qh);
        }
    }
}
