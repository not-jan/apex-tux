#![feature(generic_associated_types, type_alias_impl_trait)]
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

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
