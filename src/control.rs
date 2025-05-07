use super::VERSION;
use super::{AndroidAutoFrame, ChannelId, FrameHeader, FrameHeaderContents, FrameHeaderType};
use crate::Wifi;
use protobuf::{Enum, Message};

#[cfg(feature = "wireless")]
#[derive(Debug)]
pub enum AndroidAutoControlMessage {
    VersionRequest,
    VersionResponse { major: u16, minor: u16, status: u16 },
    SslHandshake(Vec<u8>),
    SslAuthComplete(bool),
    ServiceDiscoveryRequest(Wifi::ServiceDiscoveryRequest),
    ServiceDiscoveryResponse(Wifi::ServiceDiscoveryResponse),
    AudioFocusRequest(Wifi::AudioFocusRequest),
    AudioFocusResponse(Wifi::AudioFocusResponse),
    PingRequest(Wifi::PingRequest),
    PingResponse(Wifi::PingResponse),
}

#[cfg(feature = "wireless")]
impl TryFrom<&AndroidAutoFrame> for AndroidAutoControlMessage {
    type Error = String;
    fn try_from(value: &AndroidAutoFrame) -> Result<Self, Self::Error> {
        let mut ty = [0u8; 2];
        ty.copy_from_slice(&value.data[0..2]);
        let ty = u16::from_be_bytes(ty);
        if !value.header.frame.get_control() {
            let w = Wifi::ControlMessage::from_i32(ty as i32);
            if let Some(m) = w {
                match m {
                    Wifi::ControlMessage::VERSION_REQUEST => unimplemented!(),
                    Wifi::ControlMessage::AUTH_COMPLETE => unimplemented!(),
                    Wifi::ControlMessage::MESSAGE_NONE => unimplemented!(),
                    Wifi::ControlMessage::SERVICE_DISCOVERY_RESPONSE => unimplemented!(),
                    Wifi::ControlMessage::PING_REQUEST => {
                        let m = Wifi::PingRequest::parse_from_bytes(&value.data[2..]);
                        match m {
                            Ok(m) => Ok(AndroidAutoControlMessage::PingRequest(m)),
                            Err(e) => Err(format!("Invalid ping request: {}", e.to_string())),
                        }
                    }
                    Wifi::ControlMessage::NAVIGATION_FOCUS_REQUEST => unimplemented!(),
                    Wifi::ControlMessage::NAVIGATION_FOCUS_RESPONSE => unimplemented!(),
                    Wifi::ControlMessage::SHUTDOWN_REQUEST => unimplemented!(),
                    Wifi::ControlMessage::SHUTDOWN_RESPONSE => unimplemented!(),
                    Wifi::ControlMessage::VOICE_SESSION_REQUEST => unimplemented!(),
                    Wifi::ControlMessage::AUDIO_FOCUS_RESPONSE => unimplemented!(),
                    Wifi::ControlMessage::PING_RESPONSE => {
                        let m = Wifi::PingResponse::parse_from_bytes(&value.data[2..]);
                        match m {
                            Ok(m) => Ok(AndroidAutoControlMessage::PingResponse(m)),
                            Err(e) => Err(format!("Invalid ping response: {}", e.to_string())),
                        }
                    }
                    Wifi::ControlMessage::AUDIO_FOCUS_REQUEST => {
                        let m = Wifi::AudioFocusRequest::parse_from_bytes(&value.data[2..]);
                        match m {
                            Ok(m) => Ok(AndroidAutoControlMessage::AudioFocusRequest(m)),
                            Err(e) => {
                                Err(format!("Invalid audio focus request: {}", e.to_string()))
                            }
                        }
                    }
                    Wifi::ControlMessage::VERSION_RESPONSE => {
                        if value.data.len() == 8 {
                            let major = u16::from_be_bytes([value.data[2], value.data[3]]);
                            let minor = u16::from_be_bytes([value.data[4], value.data[5]]);
                            let status = u16::from_be_bytes([value.data[6], value.data[7]]);
                            Ok(AndroidAutoControlMessage::VersionResponse {
                                major,
                                minor,
                                status,
                            })
                        } else {
                            Err("Invalid version response packet".to_string())
                        }
                    }
                    Wifi::ControlMessage::SSL_HANDSHAKE => Ok(
                        AndroidAutoControlMessage::SslHandshake(value.data[2..].to_vec()),
                    ),
                    Wifi::ControlMessage::SERVICE_DISCOVERY_REQUEST => {
                        let m = Wifi::ServiceDiscoveryRequest::parse_from_bytes(&value.data[2..]);
                        match m {
                            Ok(m) => Ok(AndroidAutoControlMessage::ServiceDiscoveryRequest(m)),
                            Err(e) => Err(format!(
                                "Invalid service discovery request: {}",
                                e.to_string()
                            )),
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
impl Into<AndroidAutoFrame> for AndroidAutoControlMessage {
    fn into(self) -> AndroidAutoFrame {
        match self {
            AndroidAutoControlMessage::PingResponse(m) => {
                let mut data = m.write_to_bytes().unwrap();
                let t = Wifi::ControlMessage::PING_RESPONSE as u16;
                let t = t.to_be_bytes();
                let mut m = Vec::new();
                m.push(t[0]);
                m.push(t[1]);
                m.append(&mut data);
                AndroidAutoFrame {
                    header: FrameHeader {
                        channel_id: ChannelId::CONTROL,
                        frame: FrameHeaderContents::new(false, FrameHeaderType::Single, false),
                    },
                    data: m,
                }
            }
            AndroidAutoControlMessage::PingRequest(m) => {
                let mut data = m.write_to_bytes().unwrap();
                let t = Wifi::ControlMessage::PING_REQUEST as u16;
                let t = t.to_be_bytes();
                let mut m = Vec::new();
                m.push(t[0]);
                m.push(t[1]);
                m.append(&mut data);
                AndroidAutoFrame {
                    header: FrameHeader {
                        channel_id: ChannelId::CONTROL,
                        frame: FrameHeaderContents::new(false, FrameHeaderType::Single, false),
                    },
                    data: m,
                }
            }
            AndroidAutoControlMessage::AudioFocusResponse(m) => {
                let mut data = m.write_to_bytes().unwrap();
                let t = Wifi::ControlMessage::AUDIO_FOCUS_RESPONSE as u16;
                let t = t.to_be_bytes();
                let mut m = Vec::new();
                m.push(t[0]);
                m.push(t[1]);
                m.append(&mut data);
                AndroidAutoFrame {
                    header: FrameHeader {
                        channel_id: ChannelId::CONTROL,
                        frame: FrameHeaderContents::new(true, FrameHeaderType::Single, false),
                    },
                    data: m,
                }
            }
            AndroidAutoControlMessage::AudioFocusRequest(_) => unimplemented!(),
            AndroidAutoControlMessage::ServiceDiscoveryResponse(m) => {
                let mut data = m.write_to_bytes().unwrap();
                let t = Wifi::ControlMessage::SERVICE_DISCOVERY_RESPONSE as u16;
                let t = t.to_be_bytes();
                let mut m = Vec::new();
                m.push(t[0]);
                m.push(t[1]);
                m.append(&mut data);
                AndroidAutoFrame {
                    header: FrameHeader {
                        channel_id: ChannelId::CONTROL,
                        frame: FrameHeaderContents::new(true, FrameHeaderType::Single, false),
                    },
                    data: m,
                }
            }
            AndroidAutoControlMessage::VersionRequest => {
                let mut m = Vec::with_capacity(4);
                let t = Wifi::ControlMessage::VERSION_REQUEST as u16;
                let t = t.to_be_bytes();
                let major = VERSION.0.to_be_bytes();
                let minor = VERSION.1.to_be_bytes();
                m.push(t[0]);
                m.push(t[1]);
                m.push(major[0]);
                m.push(major[1]);
                m.push(minor[0]);
                m.push(minor[1]);
                AndroidAutoFrame {
                    header: FrameHeader {
                        channel_id: ChannelId::CONTROL,
                        frame: FrameHeaderContents::new(false, FrameHeaderType::Single, false),
                    },
                    data: m,
                }
            }
            AndroidAutoControlMessage::SslHandshake(mut data) => {
                let mut m = Vec::with_capacity(4);
                let t = Wifi::ControlMessage::SSL_HANDSHAKE as u16;
                let t = t.to_be_bytes();
                m.push(t[0]);
                m.push(t[1]);
                m.append(&mut data);
                AndroidAutoFrame {
                    header: FrameHeader {
                        channel_id: ChannelId::CONTROL,
                        frame: FrameHeaderContents::new(false, FrameHeaderType::Single, false),
                    },
                    data: m,
                }
            }
            AndroidAutoControlMessage::SslAuthComplete(status) => {
                let mut m = Wifi::AuthCompleteIndication::new();
                let status = if status {
                    Wifi::AuthCompleteIndicationStatus::OK
                } else {
                    Wifi::AuthCompleteIndicationStatus::FAIL
                };
                m.set_status(status);
                let mut data = m.write_to_bytes().unwrap();
                let t = Wifi::ControlMessage::AUTH_COMPLETE as u16;
                let t = t.to_be_bytes();
                let mut m = Vec::new();
                m.push(t[0]);
                m.push(t[1]);
                m.append(&mut data);
                AndroidAutoFrame {
                    header: FrameHeader {
                        channel_id: ChannelId::CONTROL,
                        frame: FrameHeaderContents::new(false, FrameHeaderType::Single, false),
                    },
                    data: m,
                }
            }
            AndroidAutoControlMessage::ServiceDiscoveryRequest(_) => unimplemented!(),
            AndroidAutoControlMessage::VersionResponse {
                major: _,
                minor: _,
                status: _,
            } => {
                unimplemented!();
            }
        }
    }
}
