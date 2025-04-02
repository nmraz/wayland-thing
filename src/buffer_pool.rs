use std::{
    mem,
    os::fd::{AsFd, OwnedFd},
    slice,
};

use anyhow::Result;
use log::trace;
use memmap2::{MmapMut, RemapOptions};
use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    protocol::{
        wl_buffer::{self, WlBuffer},
        wl_shm::{Format, WlShm},
        wl_shm_pool::WlShmPool,
    },
};

pub struct BufferToken {
    offset: usize,
}

pub struct BufferPool {
    width: u32,
    height: u32,
    shm_pool: WlShmPool,
    fd: OwnedFd,
    mapping: MmapMut,
    available_buffers: Vec<(WlBuffer, usize)>,
}

impl BufferPool {
    pub fn new<S>(shm: WlShm, qh: &QueueHandle<S>, width: u32, height: u32) -> Result<Self>
    where
        S: Dispatch<WlShmPool, ()> + 'static,
    {
        let fd = memfd_create("wayland_thing_pool", MemfdFlags::CLOEXEC)?;
        let mapping = unsafe { MmapMut::map_mut(&fd)? };

        // Wayland disallows zero-sized pools, but it's easier to resize and create the new buffers
        // on demand in `get_buffer`. So lie about the size of the pool here, which is okay as long
        // as the server doesn't actually try to access it.
        let initial_size = buffer_size(width, height);
        let shm_pool = shm.create_pool(fd.as_fd(), initial_size as i32, qh, ());

        Ok(Self {
            width,
            height,
            shm_pool,
            fd,
            mapping,
            available_buffers: vec![],
        })
    }

    pub fn get_buffer<S>(&mut self, qh: &QueueHandle<S>) -> Result<(WlBuffer, &mut [u32])>
    where
        S: Dispatch<WlBuffer, BufferToken> + 'static,
    {
        if let Some((buffer, offset)) = self.available_buffers.pop() {
            let mapping = self.buffer_at_offset(offset);
            return Ok((buffer, mapping));
        }

        let old_size = self.mapping.len();
        let new_size = old_size + buffer_size(self.width, self.height);

        trace!("resize pool: {old_size} -> {new_size}");

        ftruncate(&self.fd, new_size as u64)?;
        unsafe {
            self.mapping
                .remap(new_size, RemapOptions::new().may_move(true))?;
        }
        self.shm_pool.resize(new_size as i32);

        let new_token = BufferToken { offset: old_size };

        let buffer = self.shm_pool.create_buffer(
            old_size as i32,
            self.width as i32,
            self.height as i32,
            (self.width as usize * mem::size_of::<u32>()) as i32,
            Format::Xrgb8888,
            qh,
            new_token,
        );

        Ok((buffer, self.buffer_at_offset(old_size)))
    }

    fn buffer_at_offset(&mut self, offset: usize) -> &mut [u32] {
        unsafe {
            slice::from_raw_parts_mut(
                self.mapping.as_mut_ptr().cast::<u32>().byte_add(offset),
                (self.width * self.height) as usize,
            )
        }
    }
}

impl<S> Dispatch<WlBuffer, BufferToken, S> for BufferPool
where
    S: AsMut<BufferPool> + Dispatch<WlBuffer, BufferToken>,
{
    fn event(
        state: &mut S,
        buffer: &WlBuffer,
        event: wl_buffer::Event,
        token: &BufferToken,
        _conn: &Connection,
        _qh: &QueueHandle<S>,
    ) {
        let pool = state.as_mut();
        if let wl_buffer::Event::Release = event {
            pool.available_buffers.push((buffer.clone(), token.offset));
        }
    }
}

impl Drop for BufferPool {
    fn drop(&mut self) {
        self.shm_pool.destroy();

        for (buffer, _) in &self.available_buffers {
            buffer.destroy();
        }
    }
}

fn buffer_size(width: u32, height: u32) -> usize {
    (width * height) as usize * mem::size_of::<u32>()
}
