use std::{ffi::CStr, sync::Arc};

use anyhow::{Result, anyhow};
use ash::vk;
use log::info;

pub struct Instance {
    _entry: ash::Entry,
    instance: ash::Instance,
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            self.instance.destroy_instance(None);
        }
    }
}

impl Instance {
    pub fn new() -> Result<Arc<Self>> {
        let entry = unsafe { ash::Entry::load()? };

        let instance_create_info = vk::InstanceCreateInfo {
            p_application_info: &vk::ApplicationInfo {
                api_version: vk::make_api_version(0, 1, 0, 0),
                ..Default::default()
            },
            ..Default::default()
        };

        let instance = unsafe { entry.create_instance(&instance_create_info, None)? };

        Ok(Arc::new(Self {
            _entry: entry,
            instance,
        }))
    }

    pub fn create_default_graphics_device(self: &Arc<Self>) -> Result<Arc<Device>> {
        let available_devices = unsafe { self.instance.enumerate_physical_devices()? };
        let (physical_device, queue_family_index) = available_devices
            .iter()
            .find_map(|&physical_device| {
                let queue_families = unsafe {
                    self.instance
                        .get_physical_device_queue_family_properties(physical_device)
                };
                let (_, queue_family_index) =
                    queue_families.iter().zip(0..).find(|(properties, _idx)| {
                        properties
                            .queue_flags
                            .contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::TRANSFER)
                    })?;

                Some((physical_device, queue_family_index))
            })
            .ok_or_else(|| anyhow!("no usable vulkan devices available"))?;

        let device_properties = unsafe {
            self.instance
                .get_physical_device_properties(physical_device)
        };
        let device_name = unsafe { CStr::from_ptr(device_properties.device_name.as_ptr()) };
        info!(
            "selected device: {} ({:?})",
            device_name.to_string_lossy(),
            device_properties.device_type
        );

        let device_create_info = vk::DeviceCreateInfo {
            queue_create_info_count: 1,
            p_queue_create_infos: &vk::DeviceQueueCreateInfo {
                queue_family_index,
                queue_count: 1,
                p_queue_priorities: [1f32].as_ptr(),
                ..Default::default()
            },
            ..Default::default()
        };
        let device = unsafe {
            self.instance
                .create_device(physical_device, &device_create_info, None)?
        };

        Ok(Arc::new(Device {
            _instance: Arc::clone(self),
            device,
            _queue_family_index: queue_family_index,
        }))
    }
}

pub struct Device {
    _instance: Arc<Instance>,
    device: ash::Device,
    _queue_family_index: u32,
}

impl Device {}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_device(None);
        }
    }
}
