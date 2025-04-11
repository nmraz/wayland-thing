use std::{ffi::CStr, sync::Arc};

use anyhow::{Result, anyhow};
use ash::vk;
use log::info;

pub struct Instance {
    entry: ash::Entry,
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
    pub fn new(extension_names: &[&CStr]) -> Result<Arc<Self>> {
        let entry = unsafe { ash::Entry::load()? };

        let extension_names: Vec<_> = extension_names.iter().map(|name| name.as_ptr()).collect();

        let instance_create_info = vk::InstanceCreateInfo {
            p_application_info: &vk::ApplicationInfo {
                api_version: vk::make_api_version(0, 1, 0, 0),
                ..Default::default()
            },
            enabled_extension_count: extension_names.len() as u32,
            pp_enabled_extension_names: extension_names.as_ptr(),
            ..Default::default()
        };

        let instance = unsafe { entry.create_instance(&instance_create_info, None)? };

        Ok(Arc::new(Self { entry, instance }))
    }

    pub fn create_device(
        self: &Arc<Self>,
        extension_names: &[&CStr],
        mut match_dev: impl FnMut(vk::PhysicalDevice, u32, &vk::QueueFamilyProperties) -> bool,
    ) -> Result<Arc<Device>> {
        let available_devices = unsafe { self.instance.enumerate_physical_devices()? };
        let (physical_device, queue_family_index) = available_devices
            .iter()
            .find_map(|&physical_device| {
                let queue_families = unsafe {
                    self.instance
                        .get_physical_device_queue_family_properties(physical_device)
                };
                let (_, queue_family_index) = queue_families
                    .iter()
                    .zip(0..)
                    .find(|&(properties, idx)| match_dev(physical_device, idx, properties))?;

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

        let extension_names: Vec<_> = extension_names.iter().map(|name| name.as_ptr()).collect();

        let device_create_info = vk::DeviceCreateInfo {
            queue_create_info_count: 1,
            p_queue_create_infos: &vk::DeviceQueueCreateInfo {
                queue_family_index,
                queue_count: 1,
                p_queue_priorities: [1f32].as_ptr(),
                ..Default::default()
            },
            enabled_extension_count: extension_names.len() as u32,
            pp_enabled_extension_names: extension_names.as_ptr(),
            ..Default::default()
        };

        // NOTE: Don't exit this block early, because `device` will be leaked if so.
        {
            let device = unsafe {
                self.instance
                    .create_device(physical_device, &device_create_info, None)?
            };

            let queue = unsafe { device.get_device_queue(queue_family_index, 0) };

            Ok(Arc::new(Device {
                instance: Arc::clone(self),
                device,
                queue_family_index,
                queue,
            }))
        }
    }

    pub fn entry(&self) -> &ash::Entry {
        &self.entry
    }

    pub fn instance(&self) -> &ash::Instance {
        &self.instance
    }
}

pub struct Device {
    device: ash::Device,
    instance: Arc<Instance>,
    queue_family_index: u32,
    queue: vk::Queue,
}

impl Device {
    pub fn instance(&self) -> &Arc<Instance> {
        &self.instance
    }

    pub fn device(&self) -> &ash::Device {
        &self.device
    }

    pub fn queue_family_index(&self) -> u32 {
        self.queue_family_index
    }

    pub fn queue(&self) -> vk::Queue {
        self.queue
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_device(None);
        }
    }
}
