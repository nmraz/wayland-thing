use anyhow::Result;
use wayland_client::{Connection, globals::registry_queue_init};
use window::Window;

mod vulkan;
mod window;

fn main() -> Result<()> {
    env_logger::init();

    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init(&conn)?;

    let mut window = Window::new(
        &conn,
        &queue.handle(),
        &globals,
        500,
        500,
        "Wayland Thing".to_owned(),
    )?;

    while !window.closed {
        queue.blocking_dispatch(&mut window)?;
    }

    Ok(())
}
