//! Code for the control channel

use super::VERSION;
use super::{AndroidAutoFrame, FrameHeader, FrameHeaderContents, FrameHeaderType};
use crate::{AndroidAutoConfiguration, AndroidAutoMainTrait, ChannelHandlerTrait, ChannelId, Wifi};
use protobuf::{Enum, Message};
use tokio::io::AsyncWriteExt;

/// A control message on the android auto protocol
#[cfg(feature = "wireless")]
#[derive(Debug)]
pub enum AndroidAutoControlMessage {
    /// A message requesting version information.
    VersionRequest,
    /// A message containing version of the compatible android auto device and compatibility status
    VersionResponse {
        /// The major version
        major: u16,
        /// The minor version
        minor: u16,
        /// The status of the version compatibility, 0xffff indicates incompatibility
        status: u16,
    },
    /// A message containing ssl handshake data
    SslHandshake(Vec<u8>),
    /// A message indicating that the ssl authentication is complete
    SslAuthComplete(bool),
    /// A request to discover all channels in operation on the head unit
    ServiceDiscoveryRequest(Wifi::ServiceDiscoveryRequest),
    /// A response to the service discovery request
    ServiceDiscoveryResponse(Wifi::ServiceDiscoveryResponse),
    /// A request to set the audio focus
    AudioFocusRequest(Wifi::AudioFocusRequest),
    /// A response to an audio focus request
    AudioFocusResponse(Wifi::AudioFocusResponse),
    /// A request for ping
    PingRequest(Wifi::PingRequest),
    /// A response to a ping response
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
                            Err(e) => Err(format!("Invalid ping request: {}", e)),
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
                            Err(e) => Err(format!("Invalid ping response: {}", e)),
                        }
                    }
                    Wifi::ControlMessage::AUDIO_FOCUS_REQUEST => {
                        let m = Wifi::AudioFocusRequest::parse_from_bytes(&value.data[2..]);
                        match m {
                            Ok(m) => Ok(AndroidAutoControlMessage::AudioFocusRequest(m)),
                            Err(e) => Err(format!("Invalid audio focus request: {}", e)),
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
                            Err(e) => Err(format!("Invalid service discovery request: {}", e)),
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
impl From<AndroidAutoControlMessage> for AndroidAutoFrame {
    fn from(value: AndroidAutoControlMessage) -> Self {
        match value {
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
                        channel_id: 0,
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
                        channel_id: 0,
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
                        channel_id: 0,
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
                        channel_id: 0,
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
                        channel_id: 0,
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
                        channel_id: 0,
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
                        channel_id: 0,
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

/// Handles the control channel of the android auto protocol
pub struct ControlChannelHandler {
    /// The list of all channels for the head unit. This is filled out after the control channel is created
    channels: Vec<Wifi::ChannelDescriptor>,
}

impl ControlChannelHandler {
    /// Construct a new self
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
        }
    }
}

impl ChannelHandlerTrait for ControlChannelHandler {
    fn set_channels(&mut self, chans: Vec<Wifi::ChannelDescriptor>) {
        self.channels = chans;
    }

    fn build_channel(
        &self,
        _config: &AndroidAutoConfiguration,
        _chanid: ChannelId,
    ) -> Option<Wifi::ChannelDescriptor> {
        None
    }

    async fn receive_data<T: AndroidAutoMainTrait, U: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
        &mut self,
        msg: AndroidAutoFrame,
        stream: &super::StreamMux<U>,
        ssl_stream: &mut rustls::client::ClientConnection,
        config: &AndroidAutoConfiguration,
        _main: &mut T,
    ) -> Result<(), std::io::Error> {
        let msg2: Result<AndroidAutoControlMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoControlMessage::PingResponse(_) => {
                }
                AndroidAutoControlMessage::PingRequest(a) => {
                    let mut m = Wifi::PingResponse::new();
                    m.set_timestamp(a.timestamp() + 1);
                    let m = AndroidAutoControlMessage::PingResponse(m);
                    let d: AndroidAutoFrame = m.into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_frame(&d2).await?;
                }
                AndroidAutoControlMessage::AudioFocusResponse(_) => unimplemented!(),
                AndroidAutoControlMessage::AudioFocusRequest(m) => {
                    let mut m2 = Wifi::AudioFocusResponse::new();
                    let s = if m.has_audio_focus_type() {
                        match m.audio_focus_type() {
                            Wifi::audio_focus_type::Enum::NONE => {
                                Wifi::audio_focus_state::Enum::NONE
                            }
                            Wifi::audio_focus_type::Enum::GAIN => {
                                Wifi::audio_focus_state::Enum::GAIN
                            }
                            Wifi::audio_focus_type::Enum::GAIN_TRANSIENT => {
                                Wifi::audio_focus_state::Enum::GAIN_TRANSIENT
                            }
                            Wifi::audio_focus_type::Enum::GAIN_NAVI => {
                                Wifi::audio_focus_state::Enum::GAIN
                            }
                            Wifi::audio_focus_type::Enum::RELEASE => {
                                Wifi::audio_focus_state::Enum::LOSS
                            }
                        }
                    } else {
                        Wifi::audio_focus_state::Enum::NONE
                    };
                    m2.set_audio_focus_state(s);
                    let d: AndroidAutoFrame =
                        AndroidAutoControlMessage::AudioFocusResponse(m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_frame(&d2).await?;
                }
                AndroidAutoControlMessage::ServiceDiscoveryResponse(_) => unimplemented!(),
                AndroidAutoControlMessage::ServiceDiscoveryRequest(_m) => {
                    let mut m2 = Wifi::ServiceDiscoveryResponse::new();
                    m2.set_car_model(config.unit.car_model.clone());
                    m2.set_can_play_native_media_during_vr(config.unit.native_media);
                    m2.set_car_serial(config.unit.car_serial.clone());
                    m2.set_car_year(config.unit.car_year.clone());
                    m2.set_head_unit_name(config.unit.name.clone());
                    m2.set_headunit_manufacturer(config.unit.head_manufacturer.clone());
                    m2.set_headunit_model(config.unit.head_model.clone());
                    if let Some(hide) = config.unit.hide_clock {
                        m2.set_hide_clock(hide);
                    }
                    m2.set_left_hand_drive_vehicle(config.unit.left_hand);
                    m2.set_sw_build(config.unit.sw_build.clone());
                    m2.set_sw_version(config.unit.sw_version.clone());

                    for s in &self.channels {
                        m2.channels.push(s.clone());
                    }

                    let m3 = AndroidAutoControlMessage::ServiceDiscoveryResponse(m2);
                    let d: AndroidAutoFrame = m3.into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_frame(&d2).await?;
                }
                AndroidAutoControlMessage::SslAuthComplete(_) => unimplemented!(),
                AndroidAutoControlMessage::SslHandshake(data) => {
                    if ssl_stream.wants_read() {
                        let mut dc = std::io::Cursor::new(data);
                        let _ = ssl_stream.read_tls(&mut dc);
                        let _ = ssl_stream.process_new_packets();
                    }
                    if ssl_stream.wants_write() {
                        let mut s = Vec::new();
                        let l = ssl_stream.write_tls(&mut s);
                        if l.is_ok() {
                            let m = AndroidAutoControlMessage::SslHandshake(s);
                            let d: AndroidAutoFrame = m.into();
                            let d2: Vec<u8> = d.build_vec(None).await;
                            stream.write_frame(&d2).await?;
                        }
                    }
                    if !ssl_stream.is_handshaking() {
                        let m = AndroidAutoControlMessage::SslAuthComplete(true);
                        let d: AndroidAutoFrame = m.into();
                        let d2: Vec<u8> = d.build_vec(None).await;
                        stream.write_frame(&d2).await?;
                    }
                }
                AndroidAutoControlMessage::VersionRequest => unimplemented!(),
                AndroidAutoControlMessage::VersionResponse {
                    major,
                    minor,
                    status,
                } => {
                    if status == 0xFFFF {
                        log::error!("Version mismatch");
                        return Err(std::io::Error::other("Version mismatch"));
                    }
                    log::info!("Android auto client version: {}.{}", major, minor);
                    let mut s = Vec::new();
                    if ssl_stream.wants_write() {
                        let l = ssl_stream.write_tls(&mut s);
                        if l.is_ok() {
                            let m = AndroidAutoControlMessage::SslHandshake(s);
                            let d: AndroidAutoFrame = m.into();
                            let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                            stream.write_frame(&d2).await?;
                        }
                    }
                }
            }
        } else {
            todo!("{:?} {:x?}", msg2.err(), msg);
        }
        Ok(())
    }
}
