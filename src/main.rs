use std::{f64, time::Duration};

use anyhow::Result;
use wayland_client::{Connection, globals::registry_queue_init};
use window::Window;

mod buffer_pool;
mod window;

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

fn main() -> Result<()> {
    env_logger::init();

    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init(&conn)?;

    let mut window = Window::new(
        &globals,
        &queue.handle(),
        500,
        500,
        "Wayland Thing".to_owned(),
        draw_window,
    )?;

    while !window.closed {
        queue.blocking_dispatch(&mut window)?;
    }

    Ok(())
}
