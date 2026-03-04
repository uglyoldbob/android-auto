use nusb::transfer::EndpointType;

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
    send_aoa_string(device, AoaStringIndex::Manufacturer as u16, "Android").await;
    send_aoa_string(device, AoaStringIndex::Model as u16, "Android Auto").await;
    send_aoa_string(device, AoaStringIndex::Description as u16, "Android Auto").await;
    send_aoa_string(device, AoaStringIndex::Version as u16, "2.0.1").await;
    send_aoa_string(device, AoaStringIndex::Uri as u16, "").await;
    send_aoa_string(device, AoaStringIndex::SerialNumber as u16, "HU-AAAAAA").await;
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

pub async fn wait_for_accessory() -> Result<nusb::Device, nusb::Error> {
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        if let Ok(devices) = nusb::list_devices().await {
            for info in devices {
                if info.vendor_id() == 0x18d1
                    && (info.product_id() == 0x2D00 || info.product_id() == 0x2D01)
                {
                    log::info!("About to open {:?}", info);
                    return info.open().await;
                }
            }
        }
        log::info!("Didnt find accessory");
    }
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

#[pin_project::pin_project]
pub struct AndroidAutoUsb {
    #[pin]
    ep_in: nusb::io::EndpointRead<nusb::transfer::Bulk>,
    #[pin]
    ep_out: nusb::io::EndpointWrite<nusb::transfer::Bulk>,
}

impl AndroidAutoUsb {
    /// construct a new interface to the android usb device
    pub fn new(interface: nusb::Interface) -> Option<Self> {
        if let Ok(w) = interface.endpoint::<nusb::transfer::Bulk, nusb::transfer::In>(0x81) {
            let a = w.reader(4096);
            if let Ok(w) = interface.endpoint::<nusb::transfer::Bulk, nusb::transfer::Out>(0x1) {
                let b = w.writer(4096);
                return Some(Self {
                    ep_in: a,
                    ep_out: b,
                });
            }
        }
        None
    }
}

impl tokio::io::AsyncRead for AndroidAutoUsb {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.project().ep_in.poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for AndroidAutoUsb {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.project().ep_out.poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.project().ep_out.poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.project().ep_out.poll_shutdown(cx)
    }
}
