use super::VERSION;
use super::{AndroidAutoFrame, ChannelId, FrameHeader, FrameHeaderContents, FrameHeaderType};
use crate::Wifi::{self, CommonMessage};
use protobuf::{Enum, Message};

#[derive(Debug)]
pub enum AndroidAutoCommonMessage {
    ChannelOpenRequest(Wifi::ChannelOpenRequest),
    ChannelOpenResponse(ChannelId, Wifi::ChannelOpenResponse),
}

#[cfg(feature = "wireless")]
impl TryFrom<&AndroidAutoFrame> for AndroidAutoCommonMessage {
    type Error = String;
    fn try_from(value: &AndroidAutoFrame) -> Result<Self, Self::Error> {
        let mut ty = [0u8; 2];
        ty.copy_from_slice(&value.data[0..2]);
        let ty = u16::from_be_bytes(ty);
        if value.header.frame.get_control() {
            log::error!("Control id is {:x?}", ty);
            let w = Wifi::CommonMessage::from_i32(ty as i32);
            if let Some(m) = w {
                match m {
                    Wifi::CommonMessage::CHANNEL_OPEN_RESPONSE => unimplemented!(),
                    Wifi::CommonMessage::CHANNEL_OPEN_REQUEST => {
                        let m = Wifi::ChannelOpenRequest::parse_from_bytes(&value.data[2..]);
                        match m {
                            Ok(m) => Ok(AndroidAutoCommonMessage::ChannelOpenRequest(m)),
                            Err(e) => {
                                Err(format!("Invalid channel open request: {}", e.to_string()))
                            }
                        }
                    }
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
impl Into<AndroidAutoFrame> for AndroidAutoCommonMessage {
    fn into(self) -> AndroidAutoFrame {
        match self {
            AndroidAutoCommonMessage::ChannelOpenResponse(chan, m) => {
                log::error!("Channel open response {}", m.is_initialized());
                let mut data = m.write_to_bytes().unwrap();
                let t = Wifi::CommonMessage::CHANNEL_OPEN_RESPONSE as u16;
                let t = t.to_be_bytes();
                let mut m = Vec::new();
                m.push(t[0]);
                m.push(t[1]);
                m.append(&mut data);
                AndroidAutoFrame {
                    header: FrameHeader {
                        channel_id: chan,
                        frame: FrameHeaderContents::new(true, FrameHeaderType::Single, true),
                    },
                    data: m,
                }
            }
            AndroidAutoCommonMessage::ChannelOpenRequest(_) => unimplemented!(),
        }
    }
}
