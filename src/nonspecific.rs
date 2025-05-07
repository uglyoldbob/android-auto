use super::VERSION;
use super::{AndroidAutoFrame, ChannelId, FrameHeader, FrameHeaderContents, FrameHeaderType};
use crate::Wifi;
use protobuf::{Enum, Message};

#[derive(Debug)]
pub enum AndroidAutoNonspecificMessage {
    VersionRequest,
}

#[cfg(feature = "wireless")]
impl TryFrom<&AndroidAutoFrame> for AndroidAutoNonspecificMessage {
    type Error = String;
    fn try_from(value: &AndroidAutoFrame) -> Result<Self, Self::Error> {
        let mut ty = [0u8; 2];
        ty.copy_from_slice(&value.data[0..2]);
        let ty = u16::from_be_bytes(ty);
        if value.header.frame.get_control() {
            log::error!("Control id is {:x?}", ty);
            let w = Wifi::nonspecific_message::Enum::from_i32(ty as i32);
            if let Some(m) = w {
                match m {
                    Wifi::nonspecific_message::Enum::VERSION_REQUEST => unimplemented!(),
                }
            } else {
                Err(format!("Unknown packet type 0x{:x}", ty))
            }
        } else {
            Err(format!(
                "Unhandled specific message for channel {:?} {:x?}",
                value.header.channel_id, value.data
            ))
        }
    }
}

#[cfg(feature = "wireless")]
impl Into<AndroidAutoFrame> for AndroidAutoNonspecificMessage {
    fn into(self) -> AndroidAutoFrame {
        match self {
            AndroidAutoNonspecificMessage::VersionRequest => unimplemented!(),
        }
    }
}
