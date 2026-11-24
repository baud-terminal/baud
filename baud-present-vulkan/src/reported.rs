//! **The device a frame is drawn on, as it reports itself**: its name, its
//! PCI vendor and device ids, its Vulkan device type, and its driver's name
//! and info (`VkPhysicalDeviceDriverProperties`). Written on one line in a
//! fixed format, so a device that runs on the CPU reads as one:
//!
//! ```text
//! "<name>", pci <vendor>:<device>, type <type>, driver "<driver>", info "<info>"
//! ```
//!
//! The ids are lowercase hex, at least four digits each; the strings are
//! quoted and escaped as Rust's `Debug` writes them; the type is one of
//! `other`, `integrated-gpu`, `discrete-gpu`, `virtual-gpu` and `cpu`. A
//! driver name and info the device does not report are written empty.

use core::fmt;

/// A PCI id, vendor or device, as the device reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciId(pub u32);

/// The Vulkan device type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceType {
    Other,
    IntegratedGpu,
    DiscreteGpu,
    VirtualGpu,
    Cpu,
}

/// What a device reports of itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reported {
    pub name: String,
    pub vendor: PciId,
    pub device: PciId,
    pub kind: DeviceType,
    pub driver: String,
    pub info: String,
}

impl DeviceType {
    /// The type as the line writes it.
    pub fn text(self) -> &'static str {
        match self {
            DeviceType::Other => "other",
            DeviceType::IntegratedGpu => "integrated-gpu",
            DeviceType::DiscreteGpu => "discrete-gpu",
            DeviceType::VirtualGpu => "virtual-gpu",
            DeviceType::Cpu => "cpu",
        }
    }
}

impl fmt::Display for Reported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?}, pci {:04x}:{:04x}, type {}, driver {:?}, info {:?}",
            self.name,
            self.vendor.0,
            self.device.0,
            self.kind.text(),
            self.driver,
            self.info
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The line names the device exactly**: quoted name, the two ids in
    /// hex, the type, then the driver's name and info.
    #[test]
    fn a_device_is_written_in_the_fixed_format() {
        let radv = Reported {
            name: String::from("AMD Radeon RX 7900 XTX (RADV NAVI31)"),
            vendor: PciId(0x1002),
            device: PciId(0x744c),
            kind: DeviceType::DiscreteGpu,
            driver: String::from("radv"),
            info: String::from("Mesa 25.0.7"),
        };
        assert_eq!(
            radv.to_string(),
            r#""AMD Radeon RX 7900 XTX (RADV NAVI31)", pci 1002:744c, type discrete-gpu, driver "radv", info "Mesa 25.0.7""#
        );
    }

    /// **A device on the CPU reads as one**, and an id past four digits,
    /// a quote in a string and an unreported driver are written whole.
    #[test]
    fn a_cpu_device_reads_as_a_cpu_device() {
        let lavapipe = Reported {
            name: String::from("llvmpipe (LLVM 19.1.7, 256 bits)"),
            vendor: PciId(0x10005),
            device: PciId(0),
            kind: DeviceType::Cpu,
            driver: String::from("llvmpipe"),
            info: String::from("Mesa \"25\""),
        };
        assert_eq!(
            lavapipe.to_string(),
            r#""llvmpipe (LLVM 19.1.7, 256 bits)", pci 10005:0000, type cpu, driver "llvmpipe", info "Mesa \"25\"""#
        );
        let bare = Reported {
            driver: String::new(),
            info: String::new(),
            kind: DeviceType::Other,
            ..lavapipe
        };
        assert!(
            bare.to_string()
                .ends_with(r#"type other, driver "", info """#)
        );
    }

    /// Every type has its own word.
    #[test]
    fn each_type_has_its_word() {
        let all = [
            DeviceType::Other,
            DeviceType::IntegratedGpu,
            DeviceType::DiscreteGpu,
            DeviceType::VirtualGpu,
            DeviceType::Cpu,
        ];
        let words: Vec<_> = all.iter().map(|t| t.text()).collect();
        assert_eq!(
            words,
            [
                "other",
                "integrated-gpu",
                "discrete-gpu",
                "virtual-gpu",
                "cpu"
            ]
        );
    }
}
