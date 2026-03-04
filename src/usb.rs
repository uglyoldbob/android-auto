#[repr(u16)]
enum AoaStringIndex {
    Manufacturer = 0,
    Model = 1,
    Description = 2,
    Version = 3,
    Uri = 4,
    SerialNumber = 5,
}

async fn send_aoa_string(device: &nusb::Device, index: u16, value: &str) {
    device
        .control_out(
            nusb::transfer::ControlOut {
                control_type: nusb::transfer::ControlType::Vendor,
                recipient: nusb::transfer::Recipient::Device,
                request: 52,
                value: 0,
                data: value.as_bytes(),
                index,
            },
            std::time::Duration::from_millis(1000),
        )
        .await
        .unwrap();
}

pub async fn identify_accessory(device: &nusb::Device) {
    send_aoa_string(device, AoaStringIndex::Manufacturer as u16, "YourCompany").await;
    send_aoa_string(device, AoaStringIndex::Model as u16, "AndroidAutoHead").await;
    send_aoa_string(
        device,
        AoaStringIndex::Description as u16,
        "Android Auto Head Unit",
    )
    .await;
    send_aoa_string(device, AoaStringIndex::Version as u16, "1.0").await;
    send_aoa_string(device, AoaStringIndex::Uri as u16, "https://example.com").await;
    send_aoa_string(device, AoaStringIndex::SerialNumber as u16, "000001").await;
}

pub async fn accessory_start(device: &nusb::Device) {
    device
        .control_out(
            nusb::transfer::ControlOut {
                control_type: nusb::transfer::ControlType::Vendor,
                recipient: nusb::transfer::Recipient::Device,
                request: 53,
                value: 0,
                data: &[],
                index: 0,
            },
            std::time::Duration::from_millis(1000),
        )
        .await
        .unwrap();
}

/// Determines if the usb device is an android device
pub fn is_android_device(info: &nusb::DeviceInfo) -> bool {
    // Already in accessory mode - best case
    if info.vendor_id() == 0x18D1 && matches!(info.product_id(), 0x2D00 | 0x2D01) {
        return true;
    }

    // Has an ADB interface (class 0xFF, subclass 0x42, protocol 0x01)
    if info
        .interfaces()
        .any(|i| i.class() == 0xFF && i.subclass() == 0x42 && i.protocol() == 0x01)
    {
        return true;
    }

    // MTP/PTP mode (what your Pixel 7 showed)
    if info
        .interfaces()
        .any(|i| i.class() == 0x06 && i.subclass() == 0x01)
    {
        return true;
    }

    false
}

/// if possible, get the aoa protocol number from the device
pub async fn get_aoa_protocol(dev: &nusb::Device) -> Option<u16> {
    let result = dev
        .control_in(
            nusb::transfer::ControlIn {
                control_type: nusb::transfer::ControlType::Vendor,
                recipient: nusb::transfer::Recipient::Device,
                request: 51,
                value: 0,
                index: 0,
                length: 2,
            },
            std::time::Duration::from_millis(1000),
        )
        .await;
    if let Ok(r) = result {
        let version = u16::from_le_bytes([r[0], r[1]]);
        if version >= 1 { Some(version) } else { None }
    } else {
        None
    }
}

pub async fn claim_aoa_interface(device: &nusb::Device) -> nusb::Interface {
    // AOA uses interface 0, with one bulk-in and one bulk-out endpoint
    device.claim_interface(0).await.unwrap()
}
