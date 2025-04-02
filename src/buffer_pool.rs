use std::{
    mem,
    os::fd::{AsFd, OwnedFd},
    slice,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::Result;
use log::{debug, trace};
use memmap2::{MmapMut, RemapOptions};
use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    protocol::{
        wl_buffer::{self, WlBuffer},
        wl_shm::{Format, WlShm},
        wl_shm_pool::WlShmPool,
    },
};

pub struct BufferHandle {
    offset: usize,
    pool: Weak<Mutex<AvailableBufferPool>>,
    loaned: AtomicBool,
}

impl BufferHandle {
    fn release(&self, buffer: &WlBuffer) {
        trace!("release buffer {} (offset {:#x})", buffer.id(), self.offset);

        let loaned = self.loaned.swap(false, Ordering::Relaxed);
        assert!(loaned, "attempted to release buffer twice");

        if let Some(pool) = self.pool.upgrade() {
            pool.lock()
                .unwrap()
                .available_buffers
                .push((buffer.clone(), self.offset));
        } else {
            buffer.destroy();
        }
    }
}

pub struct BufferDispatch;
impl<S> Dispatch<WlBuffer, BufferHandle, S> for BufferDispatch
where
    S: Dispatch<WlBuffer, BufferHandle>,
{
    fn event(
        _state: &mut S,
        buffer: &WlBuffer,
        event: wl_buffer::Event,
        handle: &BufferHandle,
        _conn: &Connection,
        _qh: &QueueHandle<S>,
    ) {
        if let wl_buffer::Event::Release = event {
            handle.release(buffer);
        }
    }
}

pub struct BufferPool {
    width: u32,
    height: u32,
    shm_pool: WlShmPool,
    fd: OwnedFd,
    mapping: MmapMut,
    available_buffers: Arc<Mutex<AvailableBufferPool>>,
}

impl BufferPool {
    pub fn new<S>(shm: &WlShm, qh: &QueueHandle<S>, width: u32, height: u32) -> Result<Self>
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
            available_buffers: Arc::new(Mutex::new(AvailableBufferPool {
                available_buffers: vec![],
            })),
        })
    }

    pub fn get_buffer<S>(&mut self, qh: &QueueHandle<S>) -> Result<(WlBuffer, &mut [u32])>
    where
        S: Dispatch<WlBuffer, BufferHandle> + 'static,
    {
        {
            let mut available_buffers = self.available_buffers.lock().unwrap();
            if let Some((buffer, offset)) = available_buffers.available_buffers.pop() {
                drop(available_buffers);
                let handle = buffer.data::<BufferHandle>().unwrap();
                handle.loaned.store(true, Ordering::Relaxed);
                let mapping = self.buffer_at_offset(offset);
                return Ok((buffer, mapping));
            }
        }

        let old_size = self.mapping.len();
        let new_size = old_size + buffer_size(self.width, self.height);

        debug!("resize pool: {old_size} -> {new_size}");

        ftruncate(&self.fd, new_size as u64)?;
        unsafe {
            self.mapping
                .remap(new_size, RemapOptions::new().may_move(true))?;
        }
        self.shm_pool.resize(new_size as i32);

        let new_handle = BufferHandle {
            offset: old_size,
            pool: Arc::downgrade(&self.available_buffers),
            loaned: AtomicBool::new(true),
        };

        let buffer = self.shm_pool.create_buffer(
            old_size as i32,
            self.width as i32,
            self.height as i32,
            (self.width as usize * mem::size_of::<u32>()) as i32,
            Format::Xrgb8888,
            qh,
            new_handle,
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

impl Drop for BufferPool {
    fn drop(&mut self) {
        self.shm_pool.destroy();
    }
}

struct AvailableBufferPool {
    available_buffers: Vec<(WlBuffer, usize)>,
}

impl Drop for AvailableBufferPool {
    fn drop(&mut self) {
        for (buffer, _) in &self.available_buffers {
            buffer.destroy();
        }
    }
}

fn buffer_size(width: u32, height: u32) -> usize {
    (width * height) as usize * mem::size_of::<u32>()
}
