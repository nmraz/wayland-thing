use std::{ptr, sync::Arc, time::Duration};

use anyhow::Result;
use ash::vk;
use log::{debug, trace};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, delegate_noop,
    globals::{GlobalList, GlobalListContents},
    protocol::{
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

use crate::vulkan;

pub struct Window {
    pub closed: bool,
    width: u32,
    height: u32,
    surface: WlSurface,
    viewport: WpViewport,
    fractional_scale_supported: bool,
    scale: f64,
    vk_device: Arc<vulkan::Device>,
    vk_surface: vk::SurfaceKHR,
    vk_swapchain: vk::SwapchainKHR,
    vk_swapchain_images: Vec<vk::Image>,
    acquire_image_sem: vk::Semaphore,
}

impl Window {
    pub fn new(
        conn: &Connection,
        qh: &QueueHandle<Self>,
        globals: &GlobalList,
        width: u32,
        height: u32,
        title: String,
    ) -> Result<Self> {
        let vk_instance = vulkan::Instance::new()?;

        let compositor: WlCompositor = globals.bind(qh, 4..=6, ())?;
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

        let display_ptr = conn.display().id().as_ptr().cast();
        let surface_ptr = surface.id().as_ptr().cast();

        let vk_device = vk_instance.create_device(|physical_device, idx, properties| {
            properties
                .queue_flags
                .contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::TRANSFER)
                && unsafe {
                    vk_instance
                        .khr_wayland_instance()
                        .get_physical_device_wayland_presentation_support(
                            physical_device,
                            idx,
                            &mut *display_ptr,
                        )
                }
        })?;

        let vk_surface = unsafe {
            vk_instance.khr_wayland_instance().create_wayland_surface(
                &vk::WaylandSurfaceCreateInfoKHR {
                    display: display_ptr,
                    surface: surface_ptr,
                    ..Default::default()
                },
                None,
            )?
        };

        let acquire_image_sem = unsafe {
            vk_device
                .device()
                .create_semaphore(&Default::default(), None)?
        };

        let (vk_swapchain, vk_swapchain_images) = create_vk_swapchain(
            &vk_device,
            vk_surface,
            vk::SwapchainKHR::null(),
            width,
            height,
        )?;

        let mut window = Self {
            closed: false,
            width,
            height,
            surface,
            viewport,
            fractional_scale_supported: fractional_scale_manager.is_some(),
            scale: 1.0,
            vk_device,
            vk_surface,
            vk_swapchain,
            vk_swapchain_images,
            acquire_image_sem,
        };

        // Kick off the frame timer by drawing our first frame.
        window.handle_frame(qh, Duration::from_millis(0))?;

        Ok(window)
    }

    fn handle_frame(&mut self, qh: &QueueHandle<Self>, timestamp: Duration) -> Result<()> {
        trace!("frame at {timestamp:?}");

        // TODO: Recreate if suboptimal.
        let (image_idx, _) = unsafe {
            self.vk_device.khr_swapchain_device().acquire_next_image(
                self.vk_swapchain,
                0,
                self.acquire_image_sem,
                vk::Fence::null(),
            )?
        };

        let _image = self.vk_swapchain_images[image_idx as usize];

        // TODO: bind and draw into image...

        let (width, height) = (
            (self.width as f64 * self.scale).round() as u32,
            (self.height as f64 * self.scale).round() as u32,
        );

        self.viewport
            .set_source(0.0, 0.0, width as f64, height as f64);
        self.viewport
            .set_destination(self.width as i32, self.height as i32);

        self.surface.frame(qh, FrameCallbackToken);

        // This present call will also commit the surface.
        unsafe {
            self.vk_device.khr_swapchain_device().queue_present(
                self.vk_device.queue(),
                &vk::PresentInfoKHR {
                    wait_semaphore_count: 1,
                    p_wait_semaphores: [self.acquire_image_sem].as_ptr(),
                    swapchain_count: 1,
                    p_swapchains: [self.vk_swapchain].as_ptr(),
                    p_image_indices: [image_idx].as_ptr(),
                    p_results: ptr::null_mut(),
                    ..Default::default()
                },
            )?;
        }

        Ok(())
    }

    fn set_scale(&mut self, _qh: &QueueHandle<Self>, scale: f64) {
        if scale != self.scale {
            debug!("buffer scale: {} -> {}", self.scale, scale);

            let new_width = (self.width as f64 * scale).round() as u32;
            let new_height = (self.height as f64 * scale).round() as u32;

            self.scale = scale;

            let (new_swapchain, new_images) = create_vk_swapchain(
                &self.vk_device,
                self.vk_surface,
                self.vk_swapchain,
                new_width,
                new_height,
            )
            .expect("failed to create new swapchain");

            // TODO: Destroy old stuff

            self.vk_swapchain = new_swapchain;
            self.vk_swapchain_images = new_images;
        }
    }
}

fn create_vk_swapchain(
    device: &vulkan::Device,
    vk_surface: vk::SurfaceKHR,
    old_swapchain: vk::SwapchainKHR,
    width: u32,
    height: u32,
) -> Result<(vk::SwapchainKHR, Vec<vk::Image>)> {
    let khr_swapchain_device = device.khr_swapchain_device();

    let vk_swapchain = unsafe {
        khr_swapchain_device.create_swapchain(
            &vk::SwapchainCreateInfoKHR {
                surface: vk_surface,
                min_image_count: 2,
                image_format: vk::Format::R8G8B8_UNORM,
                image_color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
                image_extent: vk::Extent2D { width, height },
                image_array_layers: 1,
                image_usage: vk::ImageUsageFlags::COLOR_ATTACHMENT,
                image_sharing_mode: vk::SharingMode::EXCLUSIVE,
                queue_family_index_count: 1,
                p_queue_family_indices: [device.queue_family_index()].as_ptr(),
                pre_transform: vk::SurfaceTransformFlagsKHR::IDENTITY,
                composite_alpha: vk::CompositeAlphaFlagsKHR::OPAQUE,
                present_mode: vk::PresentModeKHR::MAILBOX,
                clipped: vk::TRUE,
                old_swapchain,
                ..Default::default()
            },
            None,
        )?
    };

    let vk_swapchain_images = unsafe { khr_swapchain_device.get_swapchain_images(vk_swapchain)? };

    Ok((vk_swapchain, vk_swapchain_images))
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
