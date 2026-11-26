//! **Vulkan, opened at run time, and the devices as the choice reads
//! them.** `libvulkan` is loaded with `dlopen` (`ash`'s `loaded`), so a box
//! without it runs on shared memory rather than failing to start. This is
//! the one module in the crate that allows what the crate root denies:
//! every Vulkan call is `unsafe` in `ash`.

use crate::choose::{Candidate, Extensions, Kind, Major, Minor, Node, TimelineExport};
use crate::reported::{DeviceType, PciId, Reported};
use ash::vk;
use baud_present_wayland::dmabuf::Modifier;
use std::ffi::CStr;

/// The device extensions the GPU path needs, in the order a missing one is
/// named: the modifier image, its export as a dmabuf, and the release of
/// the image to the compositor's queue family.
pub const REQUIRED: [&CStr; 4] = [
    c"VK_EXT_image_drm_format_modifier",
    c"VK_KHR_external_memory_fd",
    c"VK_EXT_external_memory_dma_buf",
    c"VK_EXT_queue_family_foreign",
];

/// Explicit sync's extension: a timeline semaphore exported as an fd.
pub const SEMAPHORE_FD: &CStr = c"VK_KHR_external_semaphore_fd";

/// The driver's name and info before Vulkan 1.2.
pub const DRIVER_PROPERTIES: &CStr = c"VK_KHR_driver_properties";

/// What the device match reads: the DRM nodes.
pub const PHYSICAL_DEVICE_DRM: &CStr = c"VK_EXT_physical_device_drm";

/// Why Vulkan could not be opened.
#[derive(Debug)]
pub enum Unopened {
    /// `libvulkan` was not found or has no loader entry point.
    NoLoader,
    /// `vkCreateInstance` failed.
    Instance(vk::Result),
}

/// The loader and an instance.
pub struct Vulkan {
    // Kept for as long as the instance: the loader's library.
    _entry: ash::Entry,
    pub instance: ash::Instance,
}

impl Vulkan {
    /// Open `libvulkan` and make an instance of Vulkan 1.3.
    #[allow(unsafe_code)]
    pub fn open() -> Result<Vulkan, Unopened> {
        let entry = crate::loader::entry().ok_or(Unopened::NoLoader)?;
        let app = vk::ApplicationInfo::default()
            .application_name(c"baud")
            .api_version(vk::API_VERSION_1_3);
        let info = vk::InstanceCreateInfo::default().application_info(&app);
        // SAFETY: a valid create-info that outlives the call.
        let instance = unsafe { entry.create_instance(&info, None) }.map_err(Unopened::Instance)?;
        Ok(Vulkan {
            _entry: entry,
            instance,
        })
    }

    /// Every physical device, in the loader's order, as the choice reads it.
    #[allow(unsafe_code)]
    pub fn candidates(&self) -> Vec<(vk::PhysicalDevice, Candidate)> {
        // SAFETY: the instance is alive; the call allocates its own result.
        let devices = unsafe { self.instance.enumerate_physical_devices() }.unwrap_or_default();
        devices
            .into_iter()
            .map(|pd| (pd, self.candidate(pd)))
            .collect()
    }

    /// **What `pd` reports of itself** ([`Reported`]): its name, PCI ids
    /// and type, and its driver's name and info where the device reports
    /// them -- from Vulkan 1.2, or through the driver properties extension
    /// -- and empty where it does not.
    #[allow(unsafe_code)]
    pub fn reported(&self, pd: vk::PhysicalDevice) -> Reported {
        // SAFETY: a physical device of this instance.
        let plain = unsafe { self.instance.get_physical_device_properties(pd) };
        let mut driver = vk::PhysicalDeviceDriverProperties::default();
        let mut props = vk::PhysicalDeviceProperties2::default();
        if plain.api_version >= vk::API_VERSION_1_2 || self.has(pd, DRIVER_PROPERTIES) {
            props = props.push_next(&mut driver);
        }
        // SAFETY: a physical device of this instance, and a chain whose
        // members live through the call.
        unsafe {
            self.instance
                .get_physical_device_properties2(pd, &mut props)
        };
        let p = props.properties;
        let text = |c: Result<&CStr, _>| {
            c.map_or_else(
                |_| String::new(),
                |c: &CStr| c.to_string_lossy().into_owned(),
            )
        };
        Reported {
            name: text(p.device_name_as_c_str()),
            vendor: PciId(p.vendor_id),
            device: PciId(p.device_id),
            kind: match p.device_type {
                vk::PhysicalDeviceType::INTEGRATED_GPU => DeviceType::IntegratedGpu,
                vk::PhysicalDeviceType::DISCRETE_GPU => DeviceType::DiscreteGpu,
                vk::PhysicalDeviceType::VIRTUAL_GPU => DeviceType::VirtualGpu,
                vk::PhysicalDeviceType::CPU => DeviceType::Cpu,
                _ => DeviceType::Other,
            },
            driver: text(driver.driver_name_as_c_str()),
            info: text(driver.driver_info_as_c_str()),
        }
    }

    #[allow(unsafe_code)]
    fn has(&self, pd: vk::PhysicalDevice, name: &CStr) -> bool {
        // SAFETY: a physical device of this instance.
        let exts =
            unsafe { self.instance.enumerate_device_extension_properties(pd) }.unwrap_or_default();
        exts.iter()
            .any(|e| e.extension_name_as_c_str().is_ok_and(|n| n == name))
    }

    fn candidate(&self, pd: vk::PhysicalDevice) -> Candidate {
        let extensions = REQUIRED
            .iter()
            .find(|n| !self.has(pd, n))
            .map_or(Extensions::All, |n| {
                Extensions::Missing(n.to_str().unwrap_or("an extension"))
            });
        let kind = self.kind(pd);
        let modifiers = match extensions {
            Extensions::All => self.modifiers(pd),
            Extensions::Missing(_) => Vec::new(),
        };
        let timeline = match self.has(pd, SEMAPHORE_FD) && self.exports_timeline(pd) {
            true => TimelineExport::Yes,
            false => TimelineExport::No,
        };
        Candidate {
            kind,
            extensions,
            modifiers,
            timeline,
        }
    }

    #[allow(unsafe_code)]
    pub(crate) fn kind(&self, pd: vk::PhysicalDevice) -> Kind {
        let mut drm = vk::PhysicalDeviceDrmPropertiesEXT::default();
        let has_drm = self.has(pd, PHYSICAL_DEVICE_DRM);
        let mut props = vk::PhysicalDeviceProperties2::default();
        if has_drm {
            props = props.push_next(&mut drm);
        }
        // SAFETY: a physical device of this instance, and a chain whose
        // members live through the call.
        unsafe {
            self.instance
                .get_physical_device_properties2(pd, &mut props)
        };
        if props.properties.device_type == vk::PhysicalDeviceType::CPU {
            return Kind::Cpu;
        }
        let node = |has: vk::Bool32, major: i64, minor: i64| {
            (has == vk::TRUE).then(|| Node {
                major: Major(u32::try_from(major).unwrap_or(u32::MAX)),
                minor: Minor(u32::try_from(minor).unwrap_or(u32::MAX)),
            })
        };
        match has_drm {
            true => Kind::Gpu {
                primary: node(drm.has_primary, drm.primary_major, drm.primary_minor),
                render: node(drm.has_render, drm.render_major, drm.render_minor),
            },
            false => Kind::Gpu {
                primary: None,
                render: None,
            },
        }
    }

    /// The modifiers `B8G8R8A8_UNORM` renders to as a single-plane colour
    /// attachment and exports as a dmabuf.
    #[allow(unsafe_code)]
    fn modifiers(&self, pd: vk::PhysicalDevice) -> Vec<Modifier> {
        let format = vk::Format::B8G8R8A8_UNORM;
        let mut list = vk::DrmFormatModifierPropertiesListEXT::default();
        let mut props = vk::FormatProperties2::default().push_next(&mut list);
        // SAFETY: the first call fills in the count only.
        unsafe {
            self.instance
                .get_physical_device_format_properties2(pd, format, &mut props)
        };
        let n = usize::try_from(list.drm_format_modifier_count).unwrap_or(0);
        let mut each = vec![vk::DrmFormatModifierPropertiesEXT::default(); n];
        let mut list = vk::DrmFormatModifierPropertiesListEXT::default()
            .drm_format_modifier_properties(&mut each);
        let mut props = vk::FormatProperties2::default().push_next(&mut list);
        // SAFETY: the second call writes at most `n` entries into `each`.
        unsafe {
            self.instance
                .get_physical_device_format_properties2(pd, format, &mut props)
        };
        each.iter()
            .filter(|m| m.drm_format_modifier_plane_count == 1)
            .filter(|m| {
                m.drm_format_modifier_tiling_features
                    .contains(vk::FormatFeatureFlags::COLOR_ATTACHMENT)
            })
            .map(|m| Modifier(m.drm_format_modifier))
            .filter(|&m| self.exports(pd, format, m))
            .collect()
    }

    /// Whether an image of `format` and `modifier` can be exported as a
    /// dmabuf.
    #[allow(unsafe_code)]
    fn exports(&self, pd: vk::PhysicalDevice, format: vk::Format, modifier: Modifier) -> bool {
        let mut drm = vk::PhysicalDeviceImageDrmFormatModifierInfoEXT::default()
            .drm_format_modifier(modifier.0)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let mut external = vk::PhysicalDeviceExternalImageFormatInfo::default()
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let info = vk::PhysicalDeviceImageFormatInfo2::default()
            .format(format)
            .ty(vk::ImageType::TYPE_2D)
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST)
            .push_next(&mut drm)
            .push_next(&mut external);
        let mut ext_props = vk::ExternalImageFormatProperties::default();
        let mut props = vk::ImageFormatProperties2::default().push_next(&mut ext_props);
        // SAFETY: chains whose members live through the call.
        let ok = unsafe {
            self.instance
                .get_physical_device_image_format_properties2(pd, &info, &mut props)
        };
        ok.is_ok()
            && ext_props
                .external_memory_properties
                .external_memory_features
                .contains(vk::ExternalMemoryFeatureFlags::EXPORTABLE)
    }

    /// Whether a timeline semaphore exports as an opaque fd.
    #[allow(unsafe_code)]
    fn exports_timeline(&self, pd: vk::PhysicalDevice) -> bool {
        let mut ty =
            vk::SemaphoreTypeCreateInfo::default().semaphore_type(vk::SemaphoreType::TIMELINE);
        let info = vk::PhysicalDeviceExternalSemaphoreInfo::default()
            .handle_type(vk::ExternalSemaphoreHandleTypeFlags::OPAQUE_FD)
            .push_next(&mut ty);
        let mut props = vk::ExternalSemaphoreProperties::default();
        // SAFETY: a chain whose members live through the call.
        unsafe {
            self.instance
                .get_physical_device_external_semaphore_properties(pd, &info, &mut props)
        };
        props
            .external_semaphore_features
            .contains(vk::ExternalSemaphoreFeatureFlags::EXPORTABLE)
    }

    /// **Whether a frame's end can be handed out as a sync file**: the
    /// device has `VK_KHR_external_semaphore_fd` and a binary semaphore
    /// exports as `SYNC_FD` (`[[source: vulkan-synchronization]]`).
    #[allow(unsafe_code)]
    pub(crate) fn exports_sync_fd(&self, pd: vk::PhysicalDevice) -> Has {
        if !self.has(pd, SEMAPHORE_FD) {
            return Has::No;
        }
        let info = vk::PhysicalDeviceExternalSemaphoreInfo::default()
            .handle_type(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD);
        let mut props = vk::ExternalSemaphoreProperties::default();
        // SAFETY: a physical device of this instance and a plain struct.
        unsafe {
            self.instance
                .get_physical_device_external_semaphore_properties(pd, &info, &mut props)
        };
        match props
            .external_semaphore_features
            .contains(vk::ExternalSemaphoreFeatureFlags::EXPORTABLE)
        {
            true => Has::Yes,
            false => Has::No,
        }
    }
}

/// **One device's row of the vendor matrix** (`tests/vendors.rs`): its driver, what
/// the GPU path's choice reads of it, each extension the path names, and
/// whether it blends by a second source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The driver, by its `VkDriverId` name ([`driver_name`]).
    pub driver: String,
    pub candidate: Candidate,
    pub extensions: Vec<(&'static str, Has)>,
    pub dual_source: Has,
}

/// Whether a device has a thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Has {
    Yes,
    No,
}

/// **A driver's name as `VkDriverId` spells it**, without its `VK_DRIVER_ID_`
/// prefix, for the drivers baud is run on or names; any other by its number.
pub fn driver_name(id: vk::DriverId) -> String {
    let known = [
        (vk::DriverId::MESA_RADV, "MESA_RADV"),
        (vk::DriverId::MESA_LLVMPIPE, "MESA_LLVMPIPE"),
        (
            vk::DriverId::INTEL_OPEN_SOURCE_MESA,
            "INTEL_OPEN_SOURCE_MESA",
        ),
        (vk::DriverId::AMD_PROPRIETARY, "AMD_PROPRIETARY"),
        (vk::DriverId::AMD_OPEN_SOURCE, "AMD_OPEN_SOURCE"),
        (vk::DriverId::NVIDIA_PROPRIETARY, "NVIDIA_PROPRIETARY"),
        (vk::DriverId::MESA_NVK, "MESA_NVK"),
        (vk::DriverId::MESA_TURNIP, "MESA_TURNIP"),
    ];
    known.iter().find(|(k, _)| *k == id).map_or_else(
        || format!("driver {}", id.as_raw()),
        |(_, n)| (*n).to_string(),
    )
}

impl Has {
    fn word(self) -> &'static str {
        match self {
            Has::Yes => "yes",
            Has::No => "no",
        }
    }
}

impl Row {
    /// **The lines a driver's committed file holds**: each a fact the path's
    /// choice reads, and none that moves with the driver's version or the
    /// machine -- whether any modifier exports and whether LINEAR is among
    /// them, not how many there are, and not the memory arm, which is the
    /// machine's.
    pub fn lines(&self) -> Vec<String> {
        let c = &self.candidate;
        let yes = |b: bool| match b {
            true => "yes",
            false => "no",
        };
        let (kind, render) = match c.kind {
            Kind::Gpu { render, .. } => ("gpu", render.is_some()),
            Kind::Cpu => ("cpu", false),
        };
        let mut out = vec![
            format!("driver: {}", self.driver),
            format!("kind: {kind}"),
            format!("render node known: {}", yes(render)),
        ];
        out.extend(
            self.extensions
                .iter()
                .map(|(name, has)| format!("{name}: {}", has.word())),
        );
        out.extend([
            format!("exportable modifier: {}", yes(!c.modifiers.is_empty())),
            format!(
                "LINEAR among them: {}",
                yes(c
                    .modifiers
                    .contains(&baud_present_wayland::dmabuf::Modifier::LINEAR))
            ),
            format!(
                "timeline export: {}",
                yes(c.timeline == TimelineExport::Yes)
            ),
            format!("dual-source blend: {}", self.dual_source.word()),
        ]);
        out
    }
}

impl Vulkan {
    /// **Every device's row of the vendor matrix**, in the loader's order.
    pub fn rows(&self) -> Vec<Row> {
        self.candidates()
            .into_iter()
            .map(|(pd, candidate)| {
                let mut names: Vec<&CStr> = REQUIRED.to_vec();
                names.extend([SEMAPHORE_FD, PHYSICAL_DEVICE_DRM]);
                Row {
                    driver: driver_name(self.driver(pd)),
                    candidate,
                    extensions: names
                        .into_iter()
                        .map(|n| {
                            let has = match self.has(pd, n) {
                                true => Has::Yes,
                                false => Has::No,
                            };
                            (n.to_str().unwrap_or("an extension"), has)
                        })
                        .collect(),
                    dual_source: self.dual_source(pd),
                }
            })
            .collect()
    }

    /// The driver behind `pd`, as `VkPhysicalDeviceDriverProperties` names it.
    #[allow(unsafe_code)]
    fn driver(&self, pd: vk::PhysicalDevice) -> vk::DriverId {
        let mut driver = vk::PhysicalDeviceDriverProperties::default();
        let mut props = vk::PhysicalDeviceProperties2::default().push_next(&mut driver);
        // SAFETY: a physical device of this instance, and a chain whose
        // members live through the call.
        unsafe {
            self.instance
                .get_physical_device_properties2(pd, &mut props)
        };
        driver.driver_id
    }

    /// Whether `pd` blends by a second source (`dualSrcBlend`).
    #[allow(unsafe_code)]
    fn dual_source(&self, pd: vk::PhysicalDevice) -> Has {
        // SAFETY: a physical device of this instance.
        let f = unsafe { self.instance.get_physical_device_features(pd) };
        match f.dual_src_blend {
            vk::TRUE => Has::Yes,
            _ => Has::No,
        }
    }
}

impl Drop for Vulkan {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: nothing made from the instance outlives it here.
        unsafe { self.instance.destroy_instance(None) };
    }
}
