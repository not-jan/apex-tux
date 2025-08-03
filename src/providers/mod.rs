pub(crate) mod clock;

#[cfg(feature = "image")]
pub(crate) mod image;
#[cfg(any(feature = "dbus-support", target_os = "windows"))]
pub(crate) mod music;
#[cfg(feature = "sysinfo")]
pub(crate) mod sysinfo;
