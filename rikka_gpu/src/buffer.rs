use std::{
    mem::{align_of, size_of_val},
    sync::Arc,
};

use anyhow::Result;
use parking_lot::Mutex;

use gpu_allocator::{
    vulkan::{Allocation, AllocationCreateDesc, Allocator},
    MemoryLocation,
};

use rikka_core::{ash, vk};

use crate::{factory::DeviceGuard, types::*};

pub enum BufferLocation {
    GpuOnly,
    CpuToGpu,
    PersistentMapped,
}

pub struct BufferDesc {
    pub usage_flags: vk::BufferUsageFlags,
    pub resource_usage: ResourceUsageType,
    pub size: u32,
    pub device_only: bool,
}

impl BufferDesc {
    pub fn new() -> Self {
        Self {
            usage_flags: vk::BufferUsageFlags::empty(),
            resource_usage: ResourceUsageType::Immutable,
            size: 0,
            device_only: true,
        }
    }

    pub fn set_usage_flags(mut self, usage_flags: vk::BufferUsageFlags) -> Self {
        self.usage_flags = usage_flags;
        self
    }

    pub fn set_resource_usage(mut self, resource_usage: ResourceUsageType) -> Self {
        self.resource_usage = resource_usage;
        self
    }

    pub fn set_size(mut self, size: u32) -> Self {
        self.size = size;
        self
    }

    pub fn set_device_only(mut self, device_only: bool) -> Self {
        self.device_only = device_only;
        self
    }
}

pub struct Buffer {
    device: DeviceGuard,
    // XXX: Remove this. Allocator can be obtianed from DeviceGuard
    allocator: Arc<Mutex<Allocator>>,

    raw: vk::Buffer,
    allocation: Allocation,
    desc: BufferDesc,
    //  XXX: Are these needed?
    // global_offset: u32,
    // usage_flags: vk::BufferUsageFlags,
    // resource_usage: ResourceUsageType,
    // mapped: bool,
    // ready: bool,
}

impl Buffer {
    pub(crate) unsafe fn create(
        device: DeviceGuard,
        allocator: Arc<Mutex<Allocator>>,
        desc: BufferDesc,
    ) -> Result<Self> {
        let create_info = vk::BufferCreateInfo::builder()
            .size(desc.size as u64)
            .usage(
                desc.usage_flags
                    | vk::BufferUsageFlags::TRANSFER_SRC
                    | vk::BufferUsageFlags::TRANSFER_DST,
            );

        let raw = device.raw().create_buffer(&create_info, None)?;
        let requirements = device.raw().get_buffer_memory_requirements(raw);

        let location = {
            if desc.device_only {
                MemoryLocation::GpuOnly
            } else {
                MemoryLocation::CpuToGpu
            }
        };

        let allocation = allocator.lock().allocate(&AllocationCreateDesc {
            name: "buffer",
            requirements,
            location,
            linear: true,
        })?;

        device
            .raw()
            .bind_buffer_memory(raw, allocation.memory(), allocation.offset())?;

        Ok(Self {
            device,
            allocator,
            raw,
            allocation,
            desc,
        })
    }

    pub(crate) unsafe fn destroy(self) {
        self.device.raw().destroy_buffer(self.raw, None);
        self.allocator.lock().free(self.allocation).unwrap();
    }

    pub fn copy_data_to_buffer<T: Copy>(&self, data: &[T]) -> Result<()> {
        unsafe {
            let data_ptr = self.allocation.mapped_ptr().unwrap().as_ptr();

            let mut align =
                ash::util::Align::new(data_ptr, align_of::<T>() as _, size_of_val(data) as _);
            align.copy_from_slice(data);
        };

        Ok(())
    }

    pub fn get_device_address(&self) -> u64 {
        let addr_info = vk::BufferDeviceAddressInfo::builder().buffer(self.raw);
        unsafe { self.device.raw().get_buffer_device_address(&addr_info) }
    }

    pub fn raw(&self) -> vk::Buffer {
        self.raw.clone()
    }

    pub fn size(&self) -> u32 {
        self.desc.size
    }

    pub fn resource_usage_type(&self) -> ResourceUsageType {
        self.desc.resource_usage
    }
}
