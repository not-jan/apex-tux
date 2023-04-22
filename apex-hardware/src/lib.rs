#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
mod device;
#[cfg(feature = "usb")]
mod usb;
pub use bitvec::prelude::BitVec;
#[cfg(feature = "async")]
pub use device::AsyncDevice;
pub use device::Device;
#[cfg(feature = "usb")]
pub use usb::USBDevice;

pub use device::FrameBuffer;
