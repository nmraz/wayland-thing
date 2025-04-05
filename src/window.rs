use std::time::Duration;

use anyhow::Result;
use log::{debug, trace};
use wayland_client::{
    Connection, Dispatch, QueueHandle, delegate_dispatch, delegate_noop,
    globals::{GlobalList, GlobalListContents},
    protocol::{
        wl_buffer::WlBuffer,
        wl_callback::{self, WlCallback},
        wl_compositor::WlCompositor,
        wl_registry::{self, WlRegistry},
        wl_shm::WlShm,
        wl_shm_pool::WlShmPool,
        wl_surface::{self, WlSurface},
    },
};
use wayland_protocols::{
    wp::{
        fractional_scale::v1::client::{
            wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
            wp_fractional_scale_v1::{self, WpFractionalScaleV1},
        },
        viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
    },
    xdg::shell::client::{
        xdg_surface::XdgSurface,
        xdg_toplevel::{self, XdgToplevel},
        xdg_wm_base::{self, XdgWmBase},
    },
};

use crate::buffer_pool::{BufferDispatch, BufferHandle, BufferPool};

pub struct Window {
    pub closed: bool,
    width: u32,
    height: u32,
    shm: WlShm,
    surface: WlSurface,
    viewport: WpViewport,
    fractional_scale_supported: bool,
    scale: f64,
    buffer_pool: BufferPool,
    #[allow(clippy::type_complexity)]
    draw_callback: Box<dyn FnMut(&mut [u32], u32, u32, Duration)>,
}

impl Window {
    pub fn new(
        globals: &GlobalList,
        qh: &QueueHandle<Self>,
        width: u32,
        height: u32,
        title: String,
        draw: impl FnMut(&mut [u32], u32, u32, Duration) + 'static,
    ) -> Result<Self> {
        let compositor: WlCompositor = globals.bind(qh, 4..=6, ())?;
        let shm: WlShm = globals.bind(qh, 1..=1, ())?;
        let xdg_wm_base: XdgWmBase = globals.bind(qh, 1..=1, ())?;
        let viewporter: WpViewporter = globals.bind(qh, 1..=1, ())?;
        let fractional_scale_manager: Option<WpFractionalScaleManagerV1> =
            globals.bind(qh, 1..=1, ()).ok();

        let surface = compositor.create_surface(qh, ());
        let viewport = viewporter.get_viewport(&surface, qh, ());

        if let Some(fractional_scale_manager) = &fractional_scale_manager {
            fractional_scale_manager.get_fractional_scale(&surface, qh, ());
        }

        let xdg_surface = xdg_wm_base.get_xdg_surface(&surface, qh, ());
        let xdg_toplevel = xdg_surface.get_toplevel(qh, ());

        xdg_toplevel.set_title(title);

        let buffer_pool = BufferPool::new(&shm, qh, width, height)?;

        let mut window = Self {
            closed: false,
            width,
            height,
            shm,
            surface,
            viewport,
            fractional_scale_supported: fractional_scale_manager.is_some(),
            scale: 1.0,
            buffer_pool,
            draw_callback: Box::new(draw),
        };

        // Kick off the frame timer by drawing our first frame.
        window.handle_frame(qh, Duration::from_millis(0))?;

        Ok(window)
    }

    fn handle_frame(&mut self, qh: &QueueHandle<Self>, timestamp: Duration) -> Result<()> {
        let (buffer, mapping) = self.buffer_pool.get_buffer(qh)?;

        trace!("frame at {timestamp:?}");

        let (width, height) = (
            (self.width as f64 * self.scale).round() as u32,
            (self.height as f64 * self.scale).round() as u32,
        );

        (self.draw_callback)(mapping, width, height, timestamp);

        trace!("attach buffer {width}×{height}, scale {}", self.scale);

        self.surface.attach(Some(&buffer), 0, 0);
        self.viewport
            .set_source(0.0, 0.0, width as f64, height as f64);
        self.viewport
            .set_destination(self.width as i32, self.height as i32);
        self.surface
            .damage_buffer(0, 0, width as i32, height as i32);

        self.surface.frame(qh, FrameCallbackToken);
        self.surface.commit();

        Ok(())
    }

    fn set_scale(&mut self, qh: &QueueHandle<Self>, scale: f64) {
        if scale != self.scale {
            debug!("buffer scale: {} -> {}", self.scale, scale);

            let new_width = (self.width as f64 * scale).round() as u32;
            let new_height = (self.height as f64 * scale).round() as u32;

            self.scale = scale;
            self.buffer_pool = BufferPool::new(&self.shm, qh, new_width, new_height)
                .expect("failed to create new buffer pool");
        }
    }
}

struct FrameCallbackToken;

delegate_noop!(Window: ignore WlCompositor);
delegate_noop!(Window: ignore WlShm);
delegate_noop!(Window: ignore WlShmPool);
delegate_noop!(Window: ignore WpViewporter);
delegate_noop!(Window: ignore WpViewport);
delegate_noop!(Window: ignore WpFractionalScaleManagerV1);
delegate_noop!(Window: ignore XdgSurface);

impl Dispatch<WlRegistry, GlobalListContents> for Window {
    fn event(
        _window: &mut Self,
        _registry: &WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSurface, ()> for Window {
    fn event(
        window: &mut Self,
        _surface: &WlSurface,
        event: wl_surface::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if !window.fractional_scale_supported {
            if let wl_surface::Event::PreferredBufferScale { factor } = event {
                window.set_scale(qh, factor as f64);
            }
        }
    }
}

delegate_dispatch!(Window: [WlBuffer: BufferHandle] => BufferDispatch);

impl Dispatch<WpFractionalScaleV1, ()> for Window {
    fn event(
        window: &mut Self,
        _proxy: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            window.set_scale(qh, (scale as f64) / 120.0);
        }
    }
}

impl Dispatch<XdgWmBase, ()> for Window {
    fn event(
        _window: &mut Self,
        xdg_wm_base: &XdgWmBase,
        event: xdg_wm_base::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            xdg_wm_base.pong(serial)
        }
    }
}

impl Dispatch<XdgToplevel, ()> for Window {
    fn event(
        window: &mut Self,
        _xdg_toplevel: &XdgToplevel,
        event: xdg_toplevel::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel::Event::Close = event {
            window.closed = true;
        }
    }
}

impl Dispatch<WlCallback, FrameCallbackToken> for Window {
    fn event(
        window: &mut Self,
        _callback: &WlCallback,
        event: wl_callback::Event,
        _token: &FrameCallbackToken,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { callback_data } = event {
            window
                .handle_frame(qh, Duration::from_millis(callback_data as u64))
                .expect("frame callback failed");
        }
    }
}
