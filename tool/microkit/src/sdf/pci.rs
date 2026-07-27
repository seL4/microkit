//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use sel4_capdl_initializer_types::object;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciDevice(pub object::PCIDevice);

impl Deref for PciDevice {
    type Target = object::PCIDevice;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<PciDevice> for object::PCIDevice {
    fn from(device: PciDevice) -> Self {
        device.0
    }
}

impl From<object::PCIDevice> for PciDevice {
    fn from(device: object::PCIDevice) -> Self {
        PciDevice(device)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciDeviceParseError {
    Malformed,
    BusParse,
    DeviceParse,
    FunctionParse,
    BusOutOfRange,
    DeviceOutOfRange,
    FunctionOutOfRange,
}

impl fmt::Display for PciDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}.{:x}",
            self.bus, self.device, self.function
        )
    }
}

impl fmt::Display for PciDeviceParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PciDeviceParseError::Malformed => {
                write!(f, "expected PCI address in bus:device.function form")
            }
            PciDeviceParseError::BusParse => write!(f, "failed to parse PCI bus"),
            PciDeviceParseError::DeviceParse => write!(f, "failed to parse PCI device"),
            PciDeviceParseError::FunctionParse => write!(f, "failed to parse PCI function"),
            PciDeviceParseError::BusOutOfRange => {
                write!(
                    f,
                    "PCI bus must be within [0..{}]",
                    object::PCIDevice::PCI_BUS_MAX
                )
            }
            PciDeviceParseError::DeviceOutOfRange => {
                write!(
                    f,
                    "PCI device must be within [0..{}]",
                    object::PCIDevice::PCI_DEV_MAX
                )
            }
            PciDeviceParseError::FunctionOutOfRange => {
                write!(
                    f,
                    "PCI function must be within [0..{}]",
                    object::PCIDevice::PCI_FUNC_MAX
                )
            }
        }
    }
}

impl FromStr for PciDevice {
    type Err = PciDeviceParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (bus_str, device_function_str) =
            s.split_once(':').ok_or(PciDeviceParseError::Malformed)?;
        let (device_str, function_str) = device_function_str
            .split_once('.')
            .ok_or(PciDeviceParseError::Malformed)?;

        let bus = i64::from_str_radix(bus_str.trim(), 16)
            .map_err(|_| PciDeviceParseError::BusParse)
            .and_then(|bus| {
                match (0..=i64::from(object::PCIDevice::PCI_BUS_MAX)).contains(&bus) {
                    true => Ok(bus as u8),
                    false => Err(PciDeviceParseError::BusOutOfRange),
                }
            })?;
        let device = i64::from_str_radix(device_str.trim(), 16)
            .map_err(|_| PciDeviceParseError::DeviceParse)
            .and_then(|device| {
                match (0..=i64::from(object::PCIDevice::PCI_DEV_MAX)).contains(&device) {
                    true => Ok(device as u8),
                    false => Err(PciDeviceParseError::DeviceOutOfRange),
                }
            })?;
        let function = i64::from_str_radix(function_str.trim(), 16)
            .map_err(|_| PciDeviceParseError::FunctionParse)
            .and_then(|function| {
                match (0..=i64::from(object::PCIDevice::PCI_FUNC_MAX)).contains(&function) {
                    true => Ok(function as u8),
                    false => Err(PciDeviceParseError::FunctionOutOfRange),
                }
            })?;

        let result = object::PCIDevice {
            bus,
            device,
            function,
        };
        Ok(result.into())
    }
}
