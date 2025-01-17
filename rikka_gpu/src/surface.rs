use anyhow::Result;
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};

use rikka_core::{ash::extensions::khr, vk};

use crate::instance::Instance;

pub struct Surface {
    ash_surface: khr::Surface,
    vulkan_surface: vk::SurfaceKHR,
}

impl Surface {
    pub fn new(
        instance: &Instance,
        window_handle: &dyn HasRawWindowHandle,
        display_handle: &dyn HasRawDisplayHandle,
    ) -> Result<Self> {
        let ash_surface = khr::Surface::new(instance.entry(), instance.raw());
        let vulkan_surface = unsafe {
            ash_window::create_surface(
                instance.entry(),
                instance.raw(),
                display_handle.raw_display_handle(),
                window_handle.raw_window_handle(),
                None,
            )?
        };

        Ok(Self {
            ash_surface,
            vulkan_surface,
        })
    }

    pub fn raw(&self) -> &khr::Surface {
        &self.ash_surface
    }

    pub fn raw_vulkan(&self) -> vk::SurfaceKHR {
        self.vulkan_surface
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            self.ash_surface.destroy_surface(self.vulkan_surface, None);
        }
    }
}
