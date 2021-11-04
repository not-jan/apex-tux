# apex-hardware

This crate provides the hardware abstraction layer for the OLED screen of the Apex devices. 
It also exposes a trait so that different devices such as a simulator can be added.

## Adding support for more devices

Currently, the following devices are supported:
- Apex Pro 
- Apex 7

If you want to add your own device you have to edit `src/usb.rs` and add the product id of your device to the `SupportedDevices` enum. 

## The `async` story
The building blocks to move the `Device` trait into the async world are here but none of the current implementations of Device support async (yet).
There are efforts to move at least the USB crate we're using to be async as it'd also make a lot of sense from a technical standpoint.
You may read about the progress [here](https://github.com/ruabmbua/hidapi-rs/issues/51). 
Replacing the dependency on `hidapi-rs` is also an option, but I have yet to explore it thoroughly.