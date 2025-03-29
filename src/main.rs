use std::{mem, os::fd::AsFd, slice};

use anyhow::Result;
use memmap2::MmapMut;
use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_buffer::{self, WlBuffer},
        wl_compositor::{self, WlCompositor},
        wl_registry::{self, WlRegistry},
        wl_shm::{self, Format, WlShm},
        wl_shm_pool::{self, WlShmPool},
        wl_surface::{self, WlSurface},
    },
};
use wayland_protocols::xdg::shell::client::{
    xdg_surface::{self, XdgSurface},
    xdg_toplevel::{self, XdgToplevel},
    xdg_wm_base::{self, XdgWmBase},
};

struct State {
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

impl Dispatch<WlBuffer, ()> for State {
    fn event(
        _state: &mut Self,
        _buffer: &WlBuffer,
        _event: wl_buffer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
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
const WINDOW_BUFFER_SIZE: usize = (4 * WINDOW_WIDTH * WINDOW_HEIGHT) as usize;
const POOL_SIZE: usize = WINDOW_BUFFER_SIZE;

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

    let pool_fd = memfd_create("wayland_thing_pool", MemfdFlags::empty())?;
    ftruncate(&pool_fd, POOL_SIZE as u64)?;

    let shm_pool = shm.create_pool(pool_fd.as_fd(), POOL_SIZE.try_into()?, &queue.handle(), ());
    let buffer = shm_pool.create_buffer(
        0,
        WINDOW_WIDTH as i32,
        WINDOW_HEIGHT as i32,
        WINDOW_WIDTH as i32,
        Format::Xrgb8888,
        &queue.handle(),
        (),
    );

    let mut pool_mapping = unsafe { MmapMut::map_mut(&pool_fd)? };
    let framebuffer = unsafe {
        slice::from_raw_parts_mut(
            pool_mapping.as_mut_ptr().cast::<u32>(),
            POOL_SIZE / mem::size_of::<u32>(),
        )
    };

    draw_window(framebuffer, WINDOW_WIDTH, WINDOW_HEIGHT);

    surface.attach(Some(&buffer), 0, 0);
    surface.commit();

    let mut state = State { closed: false };
    while !state.closed {
        queue.blocking_dispatch(&mut state)?;
    }

    Ok(())
}
