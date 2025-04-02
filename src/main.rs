use anyhow::Result;
use buffer_pool::{BufferPool, BufferToken};
use wayland_client::{
    Connection, Dispatch, QueueHandle, delegate_dispatch,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_buffer::WlBuffer,
        wl_compositor::{self, WlCompositor},
        wl_registry::{self, WlRegistry},
        wl_shm::{self, WlShm},
        wl_shm_pool::{self, WlShmPool},
        wl_surface::{self, WlSurface},
    },
};
use wayland_protocols::xdg::shell::client::{
    xdg_surface::{self, XdgSurface},
    xdg_toplevel::{self, XdgToplevel},
    xdg_wm_base::{self, XdgWmBase},
};

mod buffer_pool;

struct State {
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

impl Dispatch<WlCompositor, ()> for State {
    fn event(
        _state: &mut Self,
        _compositor: &WlCompositor,
        _event: wl_compositor::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSurface, ()> for State {
    fn event(
        _state: &mut Self,
        _surface: &WlSurface,
        _event: wl_surface::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlShm, ()> for State {
    fn event(
        _state: &mut Self,
        _shm: &WlShm,
        _event: wl_shm::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlShmPool, ()> for State {
    fn event(
        _state: &mut Self,
        _pool: &WlShmPool,
        _event: wl_shm_pool::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

delegate_dispatch!(State: [WlBuffer: BufferToken] => BufferPool);
impl AsMut<BufferPool> for State {
    fn as_mut(&mut self) -> &mut BufferPool {
        &mut self.buffer_pool
    }
}

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

impl Dispatch<XdgSurface, ()> for State {
    fn event(
        _state: &mut Self,
        _xdg_surface: &XdgSurface,
        _event: xdg_surface::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
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

const WINDOW_WIDTH: u32 = 500;
const WINDOW_HEIGHT: u32 = 500;

fn draw_window(framebuffer: &mut [u32], _width: u32, _height: u32) {
    framebuffer.fill(0x000000ff);
}

fn main() -> Result<()> {
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
        buffer_pool: BufferPool::new(shm, &queue.handle(), WINDOW_WIDTH, WINDOW_HEIGHT)?,
        closed: false,
    };

    let (buffer, mapping) = state.buffer_pool.get_buffer(&queue.handle())?;

    draw_window(mapping, WINDOW_WIDTH, WINDOW_HEIGHT);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();

    while !state.closed {
        queue.blocking_dispatch(&mut state)?;
    }

    Ok(())
}
