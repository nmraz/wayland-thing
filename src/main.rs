use std::{f64, time::Duration};

use anyhow::Result;
use buffer_pool::{BufferDispatch, BufferHandle, BufferPool};
use log::trace;
use wayland_client::{
    Connection, Dispatch, QueueHandle, delegate_dispatch, delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_buffer::WlBuffer,
        wl_callback::{self, WlCallback},
        wl_compositor::WlCompositor,
        wl_registry::{self, WlRegistry},
        wl_shm::WlShm,
        wl_shm_pool::WlShmPool,
        wl_surface::WlSurface,
    },
};
use wayland_protocols::xdg::shell::client::{
    xdg_surface::XdgSurface,
    xdg_toplevel::{self, XdgToplevel},
    xdg_wm_base::{self, XdgWmBase},
};

mod buffer_pool;

struct State {
    surface: WlSurface,
    buffer_pool: BufferPool,
    closed: bool,
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _state: &mut Self,
        _registry: &WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

delegate_dispatch!(State: [WlBuffer: BufferHandle] => BufferDispatch);

impl Dispatch<XdgWmBase, ()> for State {
    fn event(
        _state: &mut Self,
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

impl Dispatch<XdgToplevel, ()> for State {
    fn event(
        state: &mut Self,
        _xdg_toplevel: &XdgToplevel,
        event: xdg_toplevel::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel::Event::Close = event {
            state.closed = true;
        }
    }
}

delegate_noop!(State: ignore WlCompositor);
delegate_noop!(State: ignore WlSurface);
delegate_noop!(State: ignore WlShm);
delegate_noop!(State: ignore WlShmPool);
delegate_noop!(State: ignore XdgSurface);

impl Dispatch<WlCallback, ()> for State {
    fn event(
        state: &mut Self,
        _callback: &WlCallback,
        event: wl_callback::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { callback_data } = event {
            handle_frame(state, qh, Duration::from_millis(callback_data as u64))
                .expect("frame callback failed");
        }
    }
}

fn draw_window(framebuffer: &mut [u32], _width: u32, _height: u32, timestamp: Duration) {
    const THROB_PERIOD: Duration = Duration::from_secs(2);
    const THROB_COLOR: u32 = 0x0000ff;

    let periods = timestamp.as_secs_f64() / THROB_PERIOD.as_secs_f64();
    let t = (1.0 + f64::sin(f64::consts::TAU * periods)) * 0.5;

    // Cheap (approximate) linear -> sRGB conversion.
    let intensity = t.powf(0.4545);

    let color = (intensity * THROB_COLOR as f64) as u32;
    framebuffer.fill(color);
}

const WINDOW_WIDTH: u32 = 500;
const WINDOW_HEIGHT: u32 = 500;

fn handle_frame(state: &mut State, qh: &QueueHandle<State>, timestamp: Duration) -> Result<()> {
    let (buffer, mapping) = state.buffer_pool.get_buffer(qh)?;

    trace!("frame at {timestamp:?}");
    draw_window(mapping, WINDOW_WIDTH, WINDOW_HEIGHT, timestamp);
    state.surface.frame(qh, ());
    state.surface.attach(Some(&buffer), 0, 0);
    state
        .surface
        .damage(0, 0, WINDOW_WIDTH as i32, WINDOW_HEIGHT as i32);
    state.surface.commit();

    Ok(())
}

fn main() -> Result<()> {
    env_logger::init();

    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;

    let compositor: WlCompositor = globals.bind(&queue.handle(), 1..=1, ())?;
    let shm: WlShm = globals.bind(&queue.handle(), 1..=1, ())?;
    let xdg_wm_base: XdgWmBase = globals.bind(&queue.handle(), 1..=1, ())?;

    let surface = compositor.create_surface(&queue.handle(), ());
    let xdg_surface = xdg_wm_base.get_xdg_surface(&surface, &queue.handle(), ());
    let xdg_toplevel = xdg_surface.get_toplevel(&queue.handle(), ());

    xdg_toplevel.set_title("Wayland Thing".to_owned());

    let mut state = State {
        surface,
        buffer_pool: BufferPool::new(shm, &queue.handle(), WINDOW_WIDTH, WINDOW_HEIGHT)?,
        closed: false,
    };

    handle_frame(&mut state, &queue.handle(), Duration::default())?;

    while !state.closed {
        queue.blocking_dispatch(&mut state)?;
    }

    Ok(())
}
