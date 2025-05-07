use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

mod cert;

use Wifi::ChannelDescriptor;
use protobuf::Message;
use rustls::pki_types::{CertificateDer, pem::PemObject};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod control;
use control::*;
mod common;
mod nonspecific;
use common::*;
mod video;
use video::*;
mod bluetooth;
use bluetooth::*;
mod sensor;
use sensor::*;

pub trait AndroidAutoMainTrait {
    #[inline(always)]
    fn supports_video(&mut self) -> Option<&mut dyn AndroidAutoVideoChannelTrait> {
        None
    }
}

#[async_trait::async_trait]
pub trait AndroidAutoVideoChannelTrait: AndroidAutoMainTrait {
    async fn receive_video(&mut self, data: Vec<u8>);
    async fn test(&mut self) {}
}

pub struct AndriodAutoBluettothServer {
    #[cfg(feature = "wireless")]
    blue: bluetooth_rust::RfcommProfileHandle,
}

include!(concat!(env!("OUT_DIR"), "/protobuf/mod.rs"));

const VERSION: (u16, u16) = (1, 1);

#[cfg(feature = "wireless")]
struct AndroidAutoMessage {
    t: u16,
    message: Vec<u8>,
}

#[derive(Clone)]
pub struct NetworkInformation {
    pub ssid: String,
    pub psk: String,
    pub mac_addr: String,
    pub ip: String,
    pub port: u16,
    pub security_mode: Bluetooth::SecurityMode,
    pub ap_type: Bluetooth::AccessPointType,
}

#[derive(Clone)]
pub struct HeadUnitInfo {
    pub name: String,
    pub car_model: String,
    pub car_year: String,
    pub car_serial: String,
    pub left_hand: bool,
    pub head_manufacturer: String,
    pub head_model: String,
    pub sw_build: String,
    pub sw_version: String,
    pub native_media: bool,
    pub hide_clock: Option<bool>,
}

#[derive(Clone)]
pub struct BluetoothInformation {
    pub address: String,
}

#[derive(Clone)]
pub struct AndroidAutoConfiguration {
    pub network: NetworkInformation,
    pub bluetooth: BluetoothInformation,
    pub unit: HeadUnitInfo,
}

/// The channel identifier for a frame
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Eq, Ord)]
#[repr(u8)]
enum ChannelId {
    CONTROL,
    INPUT,
    SENSOR,
    VIDEO,
    MEDIA_AUDIO,
    SPEECH_AUDIO,
    SYSTEM_AUDIO,
    AV_INPUT,
    BLUETOOTH,
    NAVIGATION,
    MEDIA_STATUS,
    NONE = 255,
}

impl TryFrom<u8> for ChannelId {
    type Error = ();
    fn try_from(val: u8) -> Result<Self, Self::Error> {
        if val == ChannelId::CONTROL as u8 {
            Ok(ChannelId::CONTROL)
        } else if val == ChannelId::INPUT as u8 {
            Ok(ChannelId::INPUT)
        } else if val == ChannelId::SENSOR as u8 {
            Ok(ChannelId::SENSOR)
        } else if val == ChannelId::VIDEO as u8 {
            Ok(ChannelId::VIDEO)
        } else if val == ChannelId::MEDIA_AUDIO as u8 {
            Ok(ChannelId::MEDIA_AUDIO)
        } else if val == ChannelId::SPEECH_AUDIO as u8 {
            Ok(ChannelId::SPEECH_AUDIO)
        } else if val == ChannelId::SYSTEM_AUDIO as u8 {
            Ok(ChannelId::SYSTEM_AUDIO)
        } else if val == ChannelId::AV_INPUT as u8 {
            Ok(ChannelId::AV_INPUT)
        } else if val == ChannelId::BLUETOOTH as u8 {
            Ok(ChannelId::BLUETOOTH)
        } else if val == ChannelId::NAVIGATION as u8 {
            Ok(ChannelId::NAVIGATION)
        } else if val == ChannelId::MEDIA_STATUS as u8 {
            Ok(ChannelId::MEDIA_STATUS)
        } else if val == ChannelId::NONE as u8 {
            Ok(ChannelId::NONE)
        } else {
            Err(())
        }
    }
}

#[derive(Debug, PartialEq)]
#[repr(u8)]
pub enum FrameHeaderType {
    Middle = 0,
    First = 1,
    Last = 2,
    Single = 3,
}

impl From<u8> for FrameHeaderType {
    fn from(value: u8) -> Self {
        match value & 3 {
            0 => FrameHeaderType::Middle,
            1 => FrameHeaderType::First,
            2 => FrameHeaderType::Last,
            _ => FrameHeaderType::Single,
        }
    }
}

impl Into<u8> for FrameHeaderType {
    fn into(self) -> u8 {
        self as u8
    }
}

bitfield::bitfield! {
    #[derive(Copy, Clone)]
    pub struct FrameHeaderContents(u8);
    impl Debug;
    impl new;
    u8;
    /// True indicates the frame is encrypted
    get_encryption, set_encryption: 3;
    from into FrameHeaderType, get_frame_type, set_frame_type: 1, 0;
    /// True when frame is for control, false when specific
    get_control, set_control: 2;
}

/// Represents the header of a frame sent to the android auto client
#[derive(Copy, Clone, Debug)]
struct FrameHeader {
    channel_id: ChannelId,
    frame: FrameHeaderContents,
}

impl FrameHeader {
    /// Add self to the given buffer to build part of a complete frame
    pub fn add_to(&self, buf: &mut Vec<u8>) {
        buf.push(self.channel_id as u8);
        buf.push(self.frame.0);
    }
}

struct FrameHeaderReceiver {
    channel_id: Option<ChannelId>,
}

impl FrameHeaderReceiver {
    pub fn new() -> Self {
        Self { channel_id: None }
    }
    pub async fn read(
        &mut self,
        stream: &mut tokio::net::TcpStream,
    ) -> Result<Option<FrameHeader>, std::io::Error> {
        if self.channel_id.is_none() {
            let mut b = [0u8];
            log::error!("trying to read channel id of frame");
            stream.read_exact(&mut b).await?;
            self.channel_id = ChannelId::try_from(b[0]).ok();
            log::error!("Got channel id {:?}", self.channel_id);
        }
        if let Some(channel_id) = &self.channel_id {
            let mut b = [0u8];
            log::error!("Trying to read frame header");
            stream.read_exact(&mut b).await?;
            log::error!("Got frame header {:x?}", b);
            let mut a = FrameHeaderContents::new(false, FrameHeaderType::Single, false);
            a.0 = b[0];
            let fh = FrameHeader {
                channel_id: *channel_id,
                frame: a,
            };
            return Ok(Some(fh));
        }
        Ok(None)
    }
}

#[derive(Debug)]
struct AndroidAutoFrame {
    header: FrameHeader,
    data: Vec<u8>,
}

impl AndroidAutoFrame {
    const MAX_FRAME_DATA_SIZE: usize = 0x4000;
    fn build_multi_frame(f: FrameHeader, d: Vec<u8>) -> Vec<Self> {
        let mut m = Vec::new();
        if d.len() < Self::MAX_FRAME_DATA_SIZE {
            let fr = AndroidAutoFrame { header: f, data: d };
            m.push(fr);
        } else {
            let packets = d.chunks(Self::MAX_FRAME_DATA_SIZE);
            let max = packets.len();
            for (i, p) in packets.enumerate() {
                let first = i == 0;
                let last = i == (max - 1);
                let mut h = f.clone();
                if first {
                    h.frame.set_frame_type(FrameHeaderType::First);
                } else if last {
                    h.frame.set_frame_type(FrameHeaderType::Last);
                } else {
                    h.frame.set_frame_type(FrameHeaderType::Middle);
                }
                let fr = AndroidAutoFrame {
                    header: h,
                    data: p.to_vec(),
                };
                m.push(fr);
            }
        }
        m
    }

    async fn build_vec(&self, stream: Option<&mut rustls::client::ClientConnection>) -> Vec<u8> {
        let mut buf = Vec::new();
        self.header.add_to(&mut buf);
        log::error!("Sending packet {:02x?} {:02x?}", buf, self.data);
        if self.header.frame.get_encryption() {
            if let Some(stream) = stream {
                let mut data = Vec::new();
                stream.writer().write_all(&self.data).unwrap();
                stream.write_tls(&mut data).unwrap();
                let mut p = (data.len() as u16).to_be_bytes().to_vec();
                buf.append(&mut p);
                buf.append(&mut data);
            } else {
                panic!("No ssl object when encryption was required");
            }
        } else {
            let mut data = self.data.clone();
            let mut p = (data.len() as u16).to_be_bytes().to_vec();
            buf.append(&mut p);
            buf.append(&mut data);
        }
        log::error!("Converted packet to final format to send out: {:02x?}", buf);
        buf
    }
}

struct AndroidAutoFrameReceiver {
    len: Option<u16>,
    rx_sofar: Vec<Vec<u8>>,
}

impl AndroidAutoFrameReceiver {
    fn new() -> Self {
        Self {
            len: None,
            rx_sofar: Vec::new(),
        }
    }

    fn read_plain(
        &mut self,
        header: &FrameHeader,
        stream: &mut std::net::TcpStream,
    ) -> Result<Option<AndroidAutoFrame>, String> {
        use std::io::Read;
        if self.len.is_none() {
            let mut p = [0u8; 2];
            stream.read_exact(&mut p).map_err(|e| e.to_string())?;
            let len = u16::from_be_bytes(p);
            self.len.replace(len);
        }
        if let Some(len) = &self.len {
            let mut data_frame = vec![0u8; *len as usize];
            stream
                .read_exact(&mut data_frame)
                .map_err(|e| e.to_string())?;
            let f = AndroidAutoFrame {
                header: header.clone(),
                data: data_frame.clone(),
            };
            let f = Some(f);
            return Ok(f);
        }
        Ok(None)
    }

    async fn read(
        &mut self,
        header: &FrameHeader,
        stream: &mut tokio::net::TcpStream,
        ssl_stream: &mut rustls::client::ClientConnection,
    ) -> Result<Option<AndroidAutoFrame>, std::io::Error> {
        if self.len.is_none() {
            if header.frame.get_frame_type() == FrameHeaderType::First {
                let mut p = [0u8; 6];
                stream.read_exact(&mut p).await?;
                let len = u16::from_be_bytes([p[0], p[1]]);
                self.len.replace(len);
            } else {
                let mut p = [0u8; 2];
                stream.read_exact(&mut p).await?;
                let len = u16::from_be_bytes(p);
                self.len.replace(len);
            }
        }

        let decrypt =
            |ssl_stream: &mut rustls::client::ClientConnection, len: u16, data_frame: Vec<u8>| {
                let mut plain_data = vec![0u8; data_frame.len()];
                let mut cursor = Cursor::new(&data_frame);
                let mut index = 0;
                loop {
                    let asdf = ssl_stream.read_tls(&mut cursor).unwrap();
                    let state = ssl_stream.process_new_packets();
                    log::error!("State is {} {} {:?}", asdf, len, state);
                    if asdf == 0 {
                        break;
                    }
                    if let Ok(l) = ssl_stream.reader().read(&mut plain_data[index..]) {
                        index += l;
                    }
                }
                plain_data[0..index].to_vec()
            };

        if let Some(len) = self.len.take() {
            let mut data_frame = vec![0u8; len as usize];
            log::error!(
                "Receiving frame type {:?} {}, len {}",
                header.frame.get_frame_type(),
                header.frame.get_encryption(),
                len
            );
            stream.read_exact(&mut data_frame).await?;
            let data = if header.frame.get_frame_type() == FrameHeaderType::Single {
                let data_plain = if header.frame.get_encryption() {
                    decrypt(ssl_stream, len, data_frame)
                } else {
                    data_frame
                };
                let d = data_plain.clone();
                Some(vec![d])
            } else {
                let data_plain = if header.frame.get_encryption() {
                    decrypt(ssl_stream, len, data_frame)
                } else {
                    data_frame
                };
                self.rx_sofar.push(data_plain);
                if header.frame.get_frame_type() == FrameHeaderType::Last {
                    let d = self.rx_sofar.clone();
                    self.rx_sofar.clear();
                    Some(d)
                } else {
                    None
                }
            };
            if let Some(data) = data {
                let data: Vec<u8> = data.into_iter().flatten().collect();
                let f = AndroidAutoFrame {
                    header: header.clone(),
                    data,
                };
                let f = Some(f);
                return Ok(f);
            }
        }
        Ok(None)
    }
}

enum AndroidAutoBluetoothMessage {
    SocketInfoRequest(Bluetooth::SocketInfoRequest),
    NetworkInfoMessage(Bluetooth::NetworkInfo),
}

impl AndroidAutoBluetoothMessage {
    fn as_message(&self) -> AndroidAutoMessage {
        use protobuf::Message;
        match self {
            AndroidAutoBluetoothMessage::SocketInfoRequest(m) => AndroidAutoMessage {
                t: Bluetooth::MessageId::BLUETOOTH_SOCKET_INFO_REQUEST as u16,
                message: m.write_to_bytes().unwrap(),
            },
            AndroidAutoBluetoothMessage::NetworkInfoMessage(m) => AndroidAutoMessage {
                t: Bluetooth::MessageId::BLUETOOTH_NETWORK_INFO_MESSAGE as u16,
                message: m.write_to_bytes().unwrap(),
            },
        }
    }
}

impl Into<Vec<u8>> for AndroidAutoMessage {
    fn into(self) -> Vec<u8> {
        let mut buf = Vec::new();
        let b = self.message.len() as u16;
        let a = b.to_be_bytes();
        buf.push(a[0]);
        buf.push(a[1]);
        let a = self.t.to_be_bytes();
        buf.push(a[0]);
        buf.push(a[1]);
        for b in &self.message {
            buf.push(*b);
        }
        buf
    }
}

#[enum_dispatch::enum_dispatch]
trait ChannelHandlerTrait {
    async fn receive_data<T: AndroidAutoMainTrait>(
        &mut self,
        msg: AndroidAutoFrame,
        skip_ping: &mut bool,
        stream: &mut tokio::net::TcpStream,
        ssl_stream: &mut rustls::client::ClientConnection,
        config: &AndroidAutoConfiguration,
        main: &mut T,
    ) -> Result<(), std::io::Error>;

    fn build_channel(
        &self,
        config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<ChannelDescriptor>;

    fn set_channels(&mut self, chans: Vec<ChannelDescriptor>) {}
}

enum InputMessage {
    BindingRequest(ChannelId, Wifi::BindingRequest),
    BindingResponse(ChannelId, Wifi::BindingResponse),
}

impl Into<AndroidAutoFrame> for InputMessage {
    fn into(self) -> AndroidAutoFrame {
        match self {
            Self::BindingRequest(_, _) => unimplemented!(),
            Self::BindingResponse(chan, m) => {
                let mut data = m.write_to_bytes().unwrap();
                let t = Wifi::input_channel_message::Enum::BINDING_RESPONSE as u16;
                let t = t.to_be_bytes();
                let mut m = Vec::new();
                m.push(t[0]);
                m.push(t[1]);
                m.append(&mut data);
                AndroidAutoFrame {
                    header: FrameHeader {
                        channel_id: chan,
                        frame: FrameHeaderContents::new(true, FrameHeaderType::Single, false),
                    },
                    data: m,
                }
            }
        }
    }
}

impl TryFrom<&AndroidAutoFrame> for InputMessage {
    type Error = String;
    fn try_from(value: &AndroidAutoFrame) -> Result<Self, Self::Error> {
        use protobuf::Enum;
        let mut ty = [0u8; 2];
        ty.copy_from_slice(&value.data[0..2]);
        let ty = u16::from_be_bytes(ty);
        if let Some(sys) = Wifi::input_channel_message::Enum::from_i32(ty as i32) {
            match sys {
                Wifi::input_channel_message::Enum::BINDING_REQUEST => {
                    let m = Wifi::BindingRequest::parse_from_bytes(&value.data[2..]);
                    match m {
                        Ok(m) => Ok(Self::BindingRequest(value.header.channel_id, m)),
                        Err(e) => Err(format!("Invalid input bind request: {}", e.to_string())),
                    }
                }
                Wifi::input_channel_message::Enum::BINDING_RESPONSE => unimplemented!(),
                Wifi::input_channel_message::Enum::INPUT_EVENT_INDICATION => todo!(),
                Wifi::input_channel_message::Enum::NONE => todo!(),
            }
        } else {
            Err(format!("Not converted message: {:x?}", value.data))
        }
    }
}

struct InputChannelHandler {}

impl ChannelHandlerTrait for InputChannelHandler {
    fn build_channel(
        &self,
        config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<ChannelDescriptor> {
        let mut chan = ChannelDescriptor::new();
        chan.set_channel_id(chanid as u8 as u32);
        let mut ichan = Wifi::InputChannel::new();
        let mut tc = Wifi::TouchConfig::new();
        tc.set_height(480);
        tc.set_width(800);
        ichan.touch_screen_config.0.replace(Box::new(tc));
        chan.input_channel.0.replace(Box::new(ichan));
        if !chan.is_initialized() {
            panic!("Channel not initialized?");
        }
        Some(chan)
    }

    async fn receive_data<T: AndroidAutoMainTrait>(
        &mut self,
        msg: AndroidAutoFrame,
        _skip_ping: &mut bool,
        stream: &mut tokio::net::TcpStream,
        ssl_stream: &mut rustls::client::ClientConnection,
        _config: &AndroidAutoConfiguration,
        main: &mut T,
    ) -> Result<(), std::io::Error> {
        use std::io::Write;
        let channel = msg.header.channel_id;
        let msg2: Result<InputMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                InputMessage::BindingRequest(chan, m) => {
                    let mut m2 = Wifi::BindingResponse::new();
                    m2.set_status(Wifi::status::Enum::OK);
                    let d: AndroidAutoFrame = InputMessage::BindingResponse(chan, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
                InputMessage::BindingResponse(_, _) => unimplemented!(),
            }
            return Ok(());
        }
        let msg2: Result<AndroidAutoCommonMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoCommonMessage::ChannelOpenResponse(_, _) => unimplemented!(),
                AndroidAutoCommonMessage::ChannelOpenRequest(m) => {
                    log::info!("Got channel open request for input: {:?}", m);
                    let mut m2 = Wifi::ChannelOpenResponse::new();
                    m2.set_status(Wifi::status::Enum::OK);
                    let d: AndroidAutoFrame =
                        AndroidAutoCommonMessage::ChannelOpenResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
            }
            return Ok(());
        }
        let msg2: Result<AndroidAutoControlMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoControlMessage::PingResponse(_) => unimplemented!(),
                AndroidAutoControlMessage::PingRequest(_) => unimplemented!(),
                AndroidAutoControlMessage::AudioFocusRequest(_) => unimplemented!(),
                AndroidAutoControlMessage::AudioFocusResponse(_) => unimplemented!(),
                AndroidAutoControlMessage::ServiceDiscoveryRequest(_) => unimplemented!(),
                AndroidAutoControlMessage::ServiceDiscoveryResponse(_) => unimplemented!(),
                AndroidAutoControlMessage::SslAuthComplete(_) => unimplemented!(),
                AndroidAutoControlMessage::SslHandshake(_) => unimplemented!(),
                AndroidAutoControlMessage::VersionRequest => unimplemented!(),
                AndroidAutoControlMessage::VersionResponse {
                    major: _,
                    minor: _,
                    status: _,
                } => unimplemented!(),
            }
            return Ok(());
        }
        todo!();
    }
}

struct MediaAudioChannelHandler {}

impl ChannelHandlerTrait for MediaAudioChannelHandler {
    fn build_channel(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<ChannelDescriptor> {
        let mut chan = ChannelDescriptor::new();
        chan.set_channel_id(chanid as u8 as u32);
        let mut avchan = Wifi::AVChannel::new();
        avchan.set_audio_type(Wifi::audio_type::Enum::MEDIA);
        avchan.set_available_while_in_call(true);
        avchan.set_stream_type(Wifi::avstream_type::Enum::AUDIO);
        let mut ac = Wifi::AudioConfig::new();
        ac.set_bit_depth(16);
        ac.set_channel_count(2);
        ac.set_sample_rate(48000);
        avchan.audio_configs.push(ac);
        chan.av_channel.0.replace(Box::new(avchan));
        if !chan.is_initialized() {
            panic!("Channel not initialized?");
        }
        Some(chan)
    }

    async fn receive_data<T: AndroidAutoMainTrait>(
        &mut self,
        msg: AndroidAutoFrame,
        _skip_ping: &mut bool,
        stream: &mut tokio::net::TcpStream,
        ssl_stream: &mut rustls::client::ClientConnection,
        _config: &AndroidAutoConfiguration,
        main: &mut T,
    ) -> Result<(), std::io::Error> {
        use std::io::Write;
        let channel = msg.header.channel_id;
        let msg2: Result<AndroidAutoCommonMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoCommonMessage::ChannelOpenResponse(_, _) => unimplemented!(),
                AndroidAutoCommonMessage::ChannelOpenRequest(m) => {
                    log::info!("Got channel open request for media audio: {:?}", m);
                    let mut m2 = Wifi::ChannelOpenResponse::new();
                    m2.set_status(Wifi::status::Enum::OK);
                    let d: AndroidAutoFrame =
                        AndroidAutoCommonMessage::ChannelOpenResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
            }
            return Ok(());
        }
        let msg2: Result<AvChannelMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AvChannelMessage::MediaIndicationAck(_, _) => unimplemented!(),
                AvChannelMessage::MediaIndication(_, _, _) => {
                    log::error!("Received media data for media audio");
                }
                AvChannelMessage::SetupRequest(chan, m) => {
                    log::info!("Got channel setup request for {:?} audio: {:?}", chan, m);
                    let mut m2 = Wifi::AVChannelSetupResponse::new();
                    m2.set_max_unacked(10);
                    m2.set_media_status(Wifi::avchannel_setup_status::Enum::OK);
                    m2.configs.push(0);
                    let d: AndroidAutoFrame = AvChannelMessage::SetupResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
                AvChannelMessage::SetupResponse(chan, m) => unimplemented!(),
                AvChannelMessage::VideoFocusRequest(chan, m) => {
                    let mut m2 = Wifi::VideoFocusIndication::new();
                    m2.set_focus_mode(Wifi::video_focus_mode::Enum::FOCUSED);
                    m2.set_unrequested(false);
                    let d: AndroidAutoFrame =
                        AvChannelMessage::VideoIndicationResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
                AvChannelMessage::VideoIndicationResponse(_, _) => unimplemented!(),
                AvChannelMessage::StartIndication(_, _) => {}
            }
            return Ok(());
        }
        todo!("{:x?}", msg);
    }
}

#[derive(Debug)]
enum MediaStatusMessage {
    Playback(ChannelId, Wifi::MediaInfoChannelPlaybackData),
    Metadata(ChannelId, Wifi::MediaInfoChannelMetadataData),
    Invalid,
}

impl Into<AndroidAutoFrame> for MediaStatusMessage {
    fn into(self) -> AndroidAutoFrame {
        match self {
            Self::Playback(_, _) => todo!(),
            Self::Metadata(_, _) => todo!(),
            Self::Invalid => unimplemented!(),
        }
    }
}

impl TryFrom<&AndroidAutoFrame> for MediaStatusMessage {
    type Error = String;
    fn try_from(value: &AndroidAutoFrame) -> Result<Self, Self::Error> {
        use protobuf::Enum;
        let mut ty = [0u8; 2];
        ty.copy_from_slice(&value.data[0..2]);
        let ty = u16::from_be_bytes(ty);
        if let Some(sys) = Wifi::media_info_channel_message::Enum::from_i32(ty as i32) {
            match sys {
                Wifi::media_info_channel_message::Enum::PLAYBACK => {
                    let m = Wifi::MediaInfoChannelPlaybackData::parse_from_bytes(&value.data);
                    match m {
                        Ok(m) => Ok(Self::Playback(value.header.channel_id, m)),
                        Err(e) => Ok(Self::Invalid),
                    }
                }
                Wifi::media_info_channel_message::Enum::METADATA => {
                    let m = Wifi::MediaInfoChannelMetadataData::parse_from_bytes(&value.data);
                    match m {
                        Ok(m) => Ok(Self::Metadata(value.header.channel_id, m)),
                        Err(e) => Ok(Self::Invalid),
                    }
                }
                Wifi::media_info_channel_message::Enum::NONE => todo!(),
            }
        } else {
            Err(format!("Not converted message: {:x?}", value.data))
        }
    }
}

struct MediaStatusChannelHandler {}

impl ChannelHandlerTrait for MediaStatusChannelHandler {
    fn build_channel(
        &self,
        config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<ChannelDescriptor> {
        let mut chan = ChannelDescriptor::new();
        chan.set_channel_id(chanid as u8 as u32);
        let mchan = Wifi::MediaInfoChannel::new();
        chan.media_infoChannel.0.replace(Box::new(mchan));
        if !chan.is_initialized() {
            panic!("Channel not initialized?");
        }
        Some(chan)
    }

    async fn receive_data<T: AndroidAutoMainTrait>(
        &mut self,
        msg: AndroidAutoFrame,
        _skip_ping: &mut bool,
        stream: &mut tokio::net::TcpStream,
        ssl_stream: &mut rustls::client::ClientConnection,
        _config: &AndroidAutoConfiguration,
        main: &mut T,
    ) -> Result<(), std::io::Error> {
        use std::io::Write;
        let channel = msg.header.channel_id;
        let msg2: Result<MediaStatusMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                MediaStatusMessage::Metadata(_, m) => {
                    log::info!("Metadata {:?}", m);
                }
                MediaStatusMessage::Playback(_, m) => {
                    log::info!("Playback {:?}", m);
                }
                MediaStatusMessage::Invalid => {
                    log::error!("Received invalid media info frame");
                }
            }
            return Ok(());
        }
        let msg3: Result<AndroidAutoCommonMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg3 {
            match msg2 {
                AndroidAutoCommonMessage::ChannelOpenResponse(_, _) => unimplemented!(),
                AndroidAutoCommonMessage::ChannelOpenRequest(m) => {
                    log::info!("Got channel open request for media status: {:?}", m);
                    let mut m2 = Wifi::ChannelOpenResponse::new();
                    m2.set_status(Wifi::status::Enum::OK);
                    let d: AndroidAutoFrame =
                        AndroidAutoCommonMessage::ChannelOpenResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
            }
            return Ok(());
        }
        let msg4: Result<AndroidAutoControlMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg4 {
            match msg2 {
                AndroidAutoControlMessage::PingResponse(_) => unimplemented!(),
                AndroidAutoControlMessage::PingRequest(_) => unimplemented!(),
                AndroidAutoControlMessage::AudioFocusRequest(_) => unimplemented!(),
                AndroidAutoControlMessage::AudioFocusResponse(_) => unimplemented!(),
                AndroidAutoControlMessage::ServiceDiscoveryRequest(_) => unimplemented!(),
                AndroidAutoControlMessage::ServiceDiscoveryResponse(_) => unimplemented!(),
                AndroidAutoControlMessage::SslAuthComplete(_) => unimplemented!(),
                AndroidAutoControlMessage::SslHandshake(_) => unimplemented!(),
                AndroidAutoControlMessage::VersionRequest => unimplemented!(),
                AndroidAutoControlMessage::VersionResponse {
                    major: _,
                    minor: _,
                    status: _,
                } => unimplemented!(),
            }
            return Ok(());
        }
        todo!("{:?} {:?} {:?}", msg2, msg3, msg4);
    }
}

struct NavigationChannelHandler {}

impl ChannelHandlerTrait for NavigationChannelHandler {
    fn build_channel(
        &self,
        config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<ChannelDescriptor> {
        let mut chan = ChannelDescriptor::new();
        let mut navchan = Wifi::NavigationChannel::new();
        navchan.set_minimum_interval_ms(1000);
        navchan.set_type(Wifi::navigation_turn_type::Enum::IMAGE);
        let mut io = Wifi::NavigationImageOptions::new();
        io.set_colour_depth_bits(16);
        io.set_dunno(255);
        io.set_height(256);
        io.set_width(256);
        navchan.image_options.0.replace(Box::new(io));
        chan.set_channel_id(chanid as u8 as u32);
        chan.navigation_channel.0.replace(Box::new(navchan));
        if !chan.is_initialized() {
            panic!("Channel not initialized?");
        }
        Some(chan)
    }

    async fn receive_data<T: AndroidAutoMainTrait>(
        &mut self,
        msg: AndroidAutoFrame,
        _skip_ping: &mut bool,
        stream: &mut tokio::net::TcpStream,
        ssl_stream: &mut rustls::client::ClientConnection,
        _config: &AndroidAutoConfiguration,
        _main: &mut T,
    ) -> Result<(), std::io::Error> {
        use std::io::Write;
        let channel = msg.header.channel_id;
        let msg2: Result<AndroidAutoCommonMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoCommonMessage::ChannelOpenResponse(_, _) => unimplemented!(),
                AndroidAutoCommonMessage::ChannelOpenRequest(m) => {
                    log::info!("Got channel open request for navigation: {:?}", m);
                    let mut m2 = Wifi::ChannelOpenResponse::new();
                    m2.set_status(Wifi::status::Enum::OK);
                    let d: AndroidAutoFrame =
                        AndroidAutoCommonMessage::ChannelOpenResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
            }
            return Ok(());
        }
        let msg2: Result<AndroidAutoControlMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoControlMessage::PingResponse(_) => unimplemented!(),
                AndroidAutoControlMessage::PingRequest(_) => unimplemented!(),
                AndroidAutoControlMessage::AudioFocusRequest(_) => unimplemented!(),
                AndroidAutoControlMessage::AudioFocusResponse(_) => unimplemented!(),
                AndroidAutoControlMessage::ServiceDiscoveryRequest(_) => unimplemented!(),
                AndroidAutoControlMessage::ServiceDiscoveryResponse(_) => unimplemented!(),
                AndroidAutoControlMessage::SslAuthComplete(_) => unimplemented!(),
                AndroidAutoControlMessage::SslHandshake(_) => unimplemented!(),
                AndroidAutoControlMessage::VersionRequest => unimplemented!(),
                AndroidAutoControlMessage::VersionResponse {
                    major: _,
                    minor: _,
                    status: _,
                } => unimplemented!(),
            }
            return Ok(());
        }
        todo!("{:x?}", msg);
    }
}

struct SpeechAudioChannelHandler {}

impl ChannelHandlerTrait for SpeechAudioChannelHandler {
    fn build_channel(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<ChannelDescriptor> {
        let mut chan = ChannelDescriptor::new();
        chan.set_channel_id(chanid as u8 as u32);
        let mut avchan = Wifi::AVChannel::new();
        avchan.set_audio_type(Wifi::audio_type::Enum::SPEECH);
        avchan.set_available_while_in_call(true);
        avchan.set_stream_type(Wifi::avstream_type::Enum::AUDIO);
        let mut ac = Wifi::AudioConfig::new();
        ac.set_bit_depth(16);
        ac.set_channel_count(1);
        ac.set_sample_rate(16000);
        avchan.audio_configs.push(ac);
        chan.av_channel.0.replace(Box::new(avchan));
        if !chan.is_initialized() {
            panic!("Channel not initialized?");
        }
        Some(chan)
    }

    async fn receive_data<T: AndroidAutoMainTrait>(
        &mut self,
        msg: AndroidAutoFrame,
        _skip_ping: &mut bool,
        stream: &mut tokio::net::TcpStream,
        ssl_stream: &mut rustls::client::ClientConnection,
        _config: &AndroidAutoConfiguration,
        _main: &mut T,
    ) -> Result<(), std::io::Error> {
        use std::io::Write;
        let channel = msg.header.channel_id;
        let msg2: Result<AndroidAutoCommonMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoCommonMessage::ChannelOpenResponse(_, _) => unimplemented!(),
                AndroidAutoCommonMessage::ChannelOpenRequest(m) => {
                    log::info!("Got channel open request for speech audio: {:?}", m);
                    let mut m2 = Wifi::ChannelOpenResponse::new();
                    m2.set_status(Wifi::status::Enum::OK);
                    let d: AndroidAutoFrame =
                        AndroidAutoCommonMessage::ChannelOpenResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
            }
            return Ok(());
        }
        let msg2: Result<AvChannelMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AvChannelMessage::MediaIndicationAck(_, _) => unimplemented!(),
                AvChannelMessage::MediaIndication(_, _, _) => {
                    log::error!("Received media data for speech audio");
                }
                AvChannelMessage::SetupRequest(chan, m) => {
                    log::info!("Got channel setup request for {:?} audio: {:?}", chan, m);
                    let mut m2 = Wifi::AVChannelSetupResponse::new();
                    m2.set_max_unacked(10);
                    m2.set_media_status(Wifi::avchannel_setup_status::Enum::OK);
                    m2.configs.push(0);
                    let d: AndroidAutoFrame = AvChannelMessage::SetupResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
                AvChannelMessage::SetupResponse(chan, m) => unimplemented!(),
                AvChannelMessage::VideoFocusRequest(chan, m) => {
                    let mut m2 = Wifi::VideoFocusIndication::new();
                    m2.set_focus_mode(Wifi::video_focus_mode::Enum::FOCUSED);
                    m2.set_unrequested(false);
                    let d: AndroidAutoFrame =
                        AvChannelMessage::VideoIndicationResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
                AvChannelMessage::VideoIndicationResponse(_, _) => unimplemented!(),
                AvChannelMessage::StartIndication(_, _) => {}
            }
            return Ok(());
        }
        todo!("{:x?}", msg);
    }
}

enum AvChannelMessage {
    SetupRequest(ChannelId, Wifi::AVChannelSetupRequest),
    SetupResponse(ChannelId, Wifi::AVChannelSetupResponse),
    VideoFocusRequest(ChannelId, Wifi::VideoFocusRequest),
    VideoIndicationResponse(ChannelId, Wifi::VideoFocusIndication),
    StartIndication(ChannelId, Wifi::AVChannelStartIndication),
    MediaIndication(ChannelId, Option<u64>, Vec<u8>),
    MediaIndicationAck(ChannelId, Wifi::AVMediaAckIndication),
}

impl Into<AndroidAutoFrame> for AvChannelMessage {
    fn into(self) -> AndroidAutoFrame {
        match self {
            Self::MediaIndicationAck(chan, m) => {
                let mut data = m.write_to_bytes().unwrap();
                let t = Wifi::avchannel_message::Enum::AV_MEDIA_ACK_INDICATION as u16;
                let t = t.to_be_bytes();
                let mut m = Vec::new();
                m.push(t[0]);
                m.push(t[1]);
                m.append(&mut data);
                AndroidAutoFrame {
                    header: FrameHeader {
                        channel_id: chan,
                        frame: FrameHeaderContents::new(true, FrameHeaderType::Single, false),
                    },
                    data: m,
                }
            }
            Self::SetupRequest(_, _) => unimplemented!(),
            Self::SetupResponse(chan, m) => {
                let mut data = m.write_to_bytes().unwrap();
                let t = Wifi::avchannel_message::Enum::SETUP_RESPONSE as u16;
                let t = t.to_be_bytes();
                let mut m = Vec::new();
                m.push(t[0]);
                m.push(t[1]);
                m.append(&mut data);
                AndroidAutoFrame {
                    header: FrameHeader {
                        channel_id: chan,
                        frame: FrameHeaderContents::new(true, FrameHeaderType::Single, false),
                    },
                    data: m,
                }
            }
            Self::MediaIndication(_, _, _) => unimplemented!(),
            Self::VideoFocusRequest(chan, m) => unimplemented!(),
            Self::VideoIndicationResponse(chan, m) => {
                let mut data = m.write_to_bytes().unwrap();
                let t = Wifi::avchannel_message::Enum::VIDEO_FOCUS_INDICATION as u16;
                let t = t.to_be_bytes();
                let mut m = Vec::new();
                m.push(t[0]);
                m.push(t[1]);
                m.append(&mut data);
                AndroidAutoFrame {
                    header: FrameHeader {
                        channel_id: chan,
                        frame: FrameHeaderContents::new(true, FrameHeaderType::Single, false),
                    },
                    data: m,
                }
            }
            Self::StartIndication(_, _) => unimplemented!(),
        }
    }
}

impl TryFrom<&AndroidAutoFrame> for AvChannelMessage {
    type Error = String;
    fn try_from(value: &AndroidAutoFrame) -> Result<Self, Self::Error> {
        use protobuf::Enum;
        let mut ty = [0u8; 2];
        ty.copy_from_slice(&value.data[0..2]);
        let ty = u16::from_be_bytes(ty);
        if let Some(sys) = Wifi::avchannel_message::Enum::from_i32(ty as i32) {
            match sys {
                Wifi::avchannel_message::Enum::AV_MEDIA_WITH_TIMESTAMP_INDICATION => {
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&value.data[2..10]);
                    log::info!(
                        "Recieved media on channel {:?} size {}",
                        value.header.channel_id,
                        value.data[10..].len()
                    );
                    let ts: u64 = u64::from_be_bytes(b);
                    Ok(Self::MediaIndication(
                        value.header.channel_id,
                        Some(ts),
                        value.data[10..].to_vec(),
                    ))
                }
                Wifi::avchannel_message::Enum::AV_MEDIA_INDICATION => Ok(Self::MediaIndication(
                    value.header.channel_id,
                    None,
                    value.data[2..].to_vec(),
                )),
                Wifi::avchannel_message::Enum::SETUP_REQUEST => {
                    let m = Wifi::AVChannelSetupRequest::parse_from_bytes(&value.data[2..]);
                    match m {
                        Ok(m) => Ok(Self::SetupRequest(value.header.channel_id, m)),
                        Err(e) => Err(format!("Invalid channel open request: {}", e.to_string())),
                    }
                }
                Wifi::avchannel_message::Enum::START_INDICATION => {
                    let m = Wifi::AVChannelStartIndication::parse_from_bytes(&value.data[2..]);
                    match m {
                        Ok(m) => Ok(Self::StartIndication(value.header.channel_id, m)),
                        Err(e) => Err(format!("Invalid channel open request: {}", e.to_string())),
                    }
                }
                Wifi::avchannel_message::Enum::STOP_INDICATION => todo!(),
                Wifi::avchannel_message::Enum::SETUP_RESPONSE => unimplemented!(),
                Wifi::avchannel_message::Enum::AV_MEDIA_ACK_INDICATION => todo!(),
                Wifi::avchannel_message::Enum::AV_INPUT_OPEN_REQUEST => todo!(),
                Wifi::avchannel_message::Enum::AV_INPUT_OPEN_RESPONSE => todo!(),
                Wifi::avchannel_message::Enum::VIDEO_FOCUS_REQUEST => {
                    let m = Wifi::VideoFocusRequest::parse_from_bytes(&value.data[2..]);
                    log::error!("Video focus request {:02x?}", m);
                    match m {
                        Ok(m) => Ok(Self::VideoFocusRequest(value.header.channel_id, m)),
                        Err(e) => Err(format!("Invalid channel open request: {}", e.to_string())),
                    }
                }
                Wifi::avchannel_message::Enum::VIDEO_FOCUS_INDICATION => unimplemented!(),
            }
        } else {
            Err(format!("Not converted message: {:x?}", value.data))
        }
    }
}

struct SystemAudioChannelHandler {}

impl ChannelHandlerTrait for SystemAudioChannelHandler {
    fn build_channel(
        &self,
        config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<ChannelDescriptor> {
        let mut chan = ChannelDescriptor::new();
        chan.set_channel_id(chanid as u8 as u32);
        let mut avchan = Wifi::AVChannel::new();
        avchan.set_audio_type(Wifi::audio_type::Enum::SYSTEM);
        avchan.set_available_while_in_call(true);
        avchan.set_stream_type(Wifi::avstream_type::Enum::AUDIO);
        let mut ac = Wifi::AudioConfig::new();
        ac.set_bit_depth(16);
        ac.set_channel_count(1);
        ac.set_sample_rate(16000);
        avchan.audio_configs.push(ac);
        chan.av_channel.0.replace(Box::new(avchan));
        if !chan.is_initialized() {
            panic!("Channel not initialized?");
        }
        Some(chan)
    }

    async fn receive_data<T: AndroidAutoMainTrait>(
        &mut self,
        msg: AndroidAutoFrame,
        _skip_ping: &mut bool,
        stream: &mut tokio::net::TcpStream,
        ssl_stream: &mut rustls::client::ClientConnection,
        _config: &AndroidAutoConfiguration,
        main: &mut T,
    ) -> Result<(), std::io::Error> {
        use std::io::Write;
        let channel = msg.header.channel_id;
        let msg2: Result<AndroidAutoCommonMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoCommonMessage::ChannelOpenResponse(_, _) => unimplemented!(),
                AndroidAutoCommonMessage::ChannelOpenRequest(m) => {
                    log::info!("Got channel open request for system audio: {:?}", m);
                    let mut m2 = Wifi::ChannelOpenResponse::new();
                    m2.set_status(Wifi::status::Enum::OK);
                    let d: AndroidAutoFrame =
                        AndroidAutoCommonMessage::ChannelOpenResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
            }
            return Ok(());
        }
        let msg2: Result<AvChannelMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AvChannelMessage::MediaIndicationAck(_, _) => unimplemented!(),
                AvChannelMessage::MediaIndication(_, _, _) => {
                    log::error!("Received media data for system audio");
                }
                AvChannelMessage::SetupRequest(chan, m) => {
                    log::info!("Got channel setup request for {:?} audio: {:?}", chan, m);
                    let mut m2 = Wifi::AVChannelSetupResponse::new();
                    m2.set_max_unacked(10);
                    m2.set_media_status(Wifi::avchannel_setup_status::Enum::OK);
                    m2.configs.push(0);
                    let d: AndroidAutoFrame = AvChannelMessage::SetupResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
                AvChannelMessage::SetupResponse(chan, m) => unimplemented!(),
                AvChannelMessage::VideoFocusRequest(chan, m) => {
                    let mut m2 = Wifi::VideoFocusIndication::new();
                    m2.set_focus_mode(Wifi::video_focus_mode::Enum::FOCUSED);
                    m2.set_unrequested(false);
                    let d: AndroidAutoFrame =
                        AvChannelMessage::VideoIndicationResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
                AvChannelMessage::VideoIndicationResponse(_, _) => unimplemented!(),
                AvChannelMessage::StartIndication(_, _) => {}
            }
            return Ok(());
        }
        todo!("{:x?}", msg);
    }
}

struct AvInputChannelHandler {}

impl ChannelHandlerTrait for AvInputChannelHandler {
    fn build_channel(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<ChannelDescriptor> {
        let mut chan = ChannelDescriptor::new();
        chan.set_channel_id(chanid as u8 as u32);
        let mut avchan = Wifi::AVInputChannel::new();
        //avchan.set_available_while_in_call(true);
        avchan.set_stream_type(Wifi::avstream_type::Enum::AUDIO);
        let mut ac = Wifi::AudioConfig::new();
        ac.set_bit_depth(16);
        ac.set_channel_count(1);
        ac.set_sample_rate(16000);
        avchan.audio_config.0.replace(Box::new(ac));
        chan.av_input_channel.0.replace(Box::new(avchan));
        if !chan.is_initialized() {
            panic!("Channel not initialized?");
        }
        Some(chan)
    }

    async fn receive_data<T: AndroidAutoMainTrait>(
        &mut self,
        msg: AndroidAutoFrame,
        _skip_ping: &mut bool,
        stream: &mut tokio::net::TcpStream,
        ssl_stream: &mut rustls::client::ClientConnection,
        _config: &AndroidAutoConfiguration,
        main: &mut T,
    ) -> Result<(), std::io::Error> {
        use std::io::Write;
        let channel = msg.header.channel_id;
        let msg2: Result<AndroidAutoCommonMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoCommonMessage::ChannelOpenResponse(_, _) => unimplemented!(),
                AndroidAutoCommonMessage::ChannelOpenRequest(m) => {
                    log::info!("Got channel open request for av input: {:?}", m);
                    let mut m2 = Wifi::ChannelOpenResponse::new();
                    m2.set_status(Wifi::status::Enum::OK);
                    let d: AndroidAutoFrame =
                        AndroidAutoCommonMessage::ChannelOpenResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
            }
            return Ok(());
        }
        let msg2: Result<AndroidAutoControlMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoControlMessage::PingResponse(_) => unimplemented!(),
                AndroidAutoControlMessage::PingRequest(_) => unimplemented!(),
                AndroidAutoControlMessage::AudioFocusRequest(_) => unimplemented!(),
                AndroidAutoControlMessage::AudioFocusResponse(_) => unimplemented!(),
                AndroidAutoControlMessage::ServiceDiscoveryRequest(_) => unimplemented!(),
                AndroidAutoControlMessage::ServiceDiscoveryResponse(_) => unimplemented!(),
                AndroidAutoControlMessage::SslAuthComplete(_) => unimplemented!(),
                AndroidAutoControlMessage::SslHandshake(_) => unimplemented!(),
                AndroidAutoControlMessage::VersionRequest => unimplemented!(),
                AndroidAutoControlMessage::VersionResponse {
                    major: _,
                    minor: _,
                    status: _,
                } => unimplemented!(),
            }
            return Ok(());
        }
        todo!("{:x?}", msg);
    }
}

struct ControlChannelHandler {
    channels: Vec<ChannelDescriptor>,
}

impl ChannelHandlerTrait for ControlChannelHandler {
    fn set_channels(&mut self, chans: Vec<ChannelDescriptor>) {
        self.channels = chans;
    }

    fn build_channel(
        &self,
        _config: &AndroidAutoConfiguration,
        _chanid: ChannelId,
    ) -> Option<ChannelDescriptor> {
        None
    }

    async fn receive_data<T: AndroidAutoMainTrait>(
        &mut self,
        msg: AndroidAutoFrame,
        skip_ping: &mut bool,
        stream: &mut tokio::net::TcpStream,
        ssl_stream: &mut rustls::client::ClientConnection,
        config: &AndroidAutoConfiguration,
        main: &mut T,
    ) -> Result<(), std::io::Error> {
        let msg2: Result<AndroidAutoControlMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoControlMessage::PingResponse(_) => {
                    *skip_ping = true;
                }
                AndroidAutoControlMessage::PingRequest(a) => {
                    let mut m = Wifi::PingResponse::new();
                    m.set_timestamp(a.timestamp() + 1);
                    let m = AndroidAutoControlMessage::PingResponse(m);
                    let d: AndroidAutoFrame = m.into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
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
                    stream.write_all(&d2).await?;
                }
                AndroidAutoControlMessage::ServiceDiscoveryResponse(_) => unimplemented!(),
                AndroidAutoControlMessage::ServiceDiscoveryRequest(m) => {
                    let mut m2 = Wifi::ServiceDiscoveryResponse::new();
                    m2.set_car_model(config.unit.car_model.clone());
                    m2.set_can_play_native_media_during_vr(false);
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

                    let m3data = [
                        0x0au8, 0x0f, 0x08, 0x07, 0x2a, 0x0b, 0x08, 0x01, 0x12, 0x07, 0x08, 0x80,
                        0x7d, 0x10, 0x10, 0x18, 0x01, 0x0a, 0x14, 0x08, 0x04, 0x1a, 0x10, 0x08,
                        0x01, 0x10, 0x03, 0x1a, 0x08, 0x08, 0x80, 0xf7, 0x02, 0x10, 0x10, 0x18,
                        0x02, 0x28, 0x01, 0x0a, 0x13, 0x08, 0x05, 0x1a, 0x0f, 0x08, 0x01, 0x10,
                        0x01, 0x1a, 0x07, 0x08, 0x80, 0x7d, 0x10, 0x10, 0x18, 0x01, 0x28, 0x01,
                        0x0a, 0x13, 0x08, 0x06, 0x1a, 0x0f, 0x08, 0x01, 0x10, 0x02, 0x1a, 0x07,
                        0x08, 0x80, 0x7d, 0x10, 0x10, 0x18, 0x01, 0x28, 0x01, 0x0a, 0x0c, 0x08,
                        0x02, 0x12, 0x08, 0x0a, 0x02, 0x08, 0x0d, 0x0a, 0x02, 0x08, 0x0a, 0x0a,
                        0x14, 0x08, 0x03, 0x1a, 0x10, 0x08, 0x03, 0x22, 0x0a, 0x08, 0x01, 0x10,
                        0x02, 0x18, 0x00, 0x20, 0x00, 0x28, 0x6f, 0x28, 0x01, 0x0a, 0x19, 0x08,
                        0x08, 0x32, 0x15, 0x0a, 0x11, 0x30, 0x30, 0x3a, 0x39, 0x33, 0x3a, 0x33,
                        0x37, 0x3a, 0x45, 0x46, 0x3a, 0x42, 0x37, 0x3a, 0x35, 0x37, 0x10, 0x04,
                        0x0a, 0x16, 0x08, 0x09, 0x42, 0x12, 0x08, 0xe8, 0x07, 0x10, 0x01, 0x1a,
                        0x0b, 0x08, 0x80, 0x02, 0x10, 0x80, 0x02, 0x18, 0x10, 0x20, 0xff, 0x01,
                        0x0a, 0x04, 0x08, 0x0a, 0x4a, 0x00, 0x0a, 0x0c, 0x08, 0x01, 0x22, 0x08,
                        0x12, 0x06, 0x08, 0x80, 0x0f, 0x10, 0xb8, 0x08, 0x12, 0x08, 0x4f, 0x70,
                        0x65, 0x6e, 0x41, 0x75, 0x74, 0x6f, 0x1a, 0x09, 0x55, 0x6e, 0x69, 0x76,
                        0x65, 0x72, 0x73, 0x61, 0x6c, 0x22, 0x04, 0x32, 0x30, 0x31, 0x38, 0x2a,
                        0x08, 0x32, 0x30, 0x31, 0x38, 0x30, 0x33, 0x30, 0x31, 0x30, 0x01, 0x3a,
                        0x03, 0x66, 0x31, 0x78, 0x42, 0x10, 0x4f, 0x70, 0x65, 0x6e, 0x41, 0x75,
                        0x74, 0x6f, 0x20, 0x41, 0x75, 0x74, 0x6f, 0x61, 0x70, 0x70, 0x4a, 0x01,
                        0x31, 0x52, 0x03, 0x31, 0x2e, 0x30, 0x58, 0x00, 0x60, 0x00,
                    ];
                    let m3 = Wifi::ServiceDiscoveryResponse::parse_from_bytes(&m3data);
                    log::error!("Golden service response is {:?}", m3);
                    log::error!("Our service response is {:?}", m2);

                    let m3 = AndroidAutoControlMessage::ServiceDiscoveryResponse(m2);
                    let d: AndroidAutoFrame = m3.into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
                AndroidAutoControlMessage::SslAuthComplete(_) => unimplemented!(),
                AndroidAutoControlMessage::SslHandshake(data) => {
                    log::info!("SSL Handshake data is {:x?}", data);
                    log::error!(
                        "SSL WANTS RX {} TX {}",
                        ssl_stream.wants_read(),
                        ssl_stream.wants_write()
                    );
                    if ssl_stream.wants_read() {
                        let mut dc = std::io::Cursor::new(data);
                        let asdf = ssl_stream.read_tls(&mut dc);
                        log::error!("SSL Client process received handshake is {:?}", asdf);
                        let asdg = ssl_stream.process_new_packets();
                        log::error!("Process new packets from SSL: {:?}", asdg);
                    }
                    log::error!(
                        "SSL WANTS RX {} TX {}",
                        ssl_stream.wants_read(),
                        ssl_stream.wants_write()
                    );
                    log::error!("ssl handshaking {}", ssl_stream.is_handshaking());
                    if ssl_stream.wants_write() {
                        let mut s = Vec::new();
                        let l = ssl_stream.write_tls(&mut s);
                        if let Ok(l) = l {
                            log::debug!("Got buffer length {} to send for ssl stuff {:x?}", l, s);
                            let m = AndroidAutoControlMessage::SslHandshake(s);
                            let d: AndroidAutoFrame = m.into();
                            let d2: Vec<u8> = d.build_vec(None).await;
                            stream.write_all(&d2).await?;
                            log::error!(
                                "ssl handshaking {} RX {} TX {}",
                                ssl_stream.is_handshaking(),
                                ssl_stream.wants_read(),
                                ssl_stream.wants_write()
                            );
                        }
                    }
                    if !ssl_stream.is_handshaking() {
                        let m = AndroidAutoControlMessage::SslAuthComplete(true);
                        let d: AndroidAutoFrame = m.into();
                        let d2: Vec<u8> = d.build_vec(None).await;
                        stream.write_all(&d2).await?;
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
                        if let Ok(l) = l {
                            log::debug!("Got buffer length {} to send for ssl stuff {:x?}", l, s);
                            let m = AndroidAutoControlMessage::SslHandshake(s);
                            let d: AndroidAutoFrame = m.into();
                            let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                            stream.write_all(&d2).await?;
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

#[derive(Debug)]
struct AndroidAutoServerVerifier {
    base: Arc<rustls::client::WebPkiServerVerifier>,
}

impl AndroidAutoServerVerifier {
    fn new(roots: Arc<rustls::RootCertStore>) -> Self {
        Self {
            base: rustls::client::WebPkiServerVerifier::builder(roots)
                .build()
                .unwrap(),
        }
    }
}

impl rustls::client::danger::ServerCertVerifier for AndroidAutoServerVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.base.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.base.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.base.supported_verify_schemes()
    }
}

#[enum_dispatch::enum_dispatch(ChannelHandlerTrait)]
enum ChannelHandler {
    Control(ControlChannelHandler),
    Bluetooth(BluetoothChannelHandler),
    AvInput(AvInputChannelHandler),
    SystemAudio(SystemAudioChannelHandler),
    SpeechAudio(SpeechAudioChannelHandler),
    Sensor(SensorChannelHandler),
    Video(VideoChannelHandler),
    Navigation(NavigationChannelHandler),
    MediaStatus(MediaStatusChannelHandler),
    Input(InputChannelHandler),
    MediaAudio(MediaAudioChannelHandler),
}

impl AndriodAutoBluettothServer {
    #[cfg(feature = "wireless")]
    pub async fn new(bluetooth: &mut bluetooth_rust::BluetoothHandler) -> Self {
        let profile = bluetooth_rust::RfcommProfile {
            uuid: bluetooth_rust::Uuid::parse_str(
                bluetooth_rust::BluetoothUuid::AndroidAuto.as_str(),
            )
            .unwrap(),
            name: Some("Android Auto Bluetooth Service".to_string()),
            service: bluetooth_rust::Uuid::parse_str(
                bluetooth_rust::BluetoothUuid::AndroidAuto.as_str(),
            )
            .ok(),
            role: None,
            channel: Some(22),
            psm: None,
            require_authentication: Some(true),
            require_authorization: Some(true),
            auto_connect: Some(true),
            service_record: None,
            version: None,
            features: None,
            ..Default::default()
        };
        let a = bluetooth.register_rfcomm_profile(profile).await;
        Self { blue: a.unwrap() }
    }

    #[cfg(feature = "wireless")]
    pub async fn bluetooth_listen(&mut self, network: NetworkInformation) -> Result<(), String> {
        use futures::StreamExt;
        use protobuf::Message;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        log::info!("Listening for connections on android auto profile");
        loop {
            if let Some(cr) = self.blue.next().await {
                let network2 = network.clone();
                tokio::task::spawn(async move {
                    log::info!("Got a connection to android auto profile on {:?}", cr);
                    let stream = cr.accept().unwrap();
                    let (mut read, mut write) = stream.into_split();
                    let mut s = Bluetooth::SocketInfoRequest::new();
                    s.set_ip_address(network2.ip.clone());
                    s.set_port(network2.port as u32);

                    let m1 = AndroidAutoBluetoothMessage::SocketInfoRequest(s);
                    let m: AndroidAutoMessage = m1.as_message();
                    let mdata: Vec<u8> = m.into();
                    let r1 = write.write_all(&mdata).await;
                    loop {
                        let mut ty = [0u8; 2];
                        let mut len = [0u8; 2];
                        read.read_exact(&mut len).await.map_err(|e| e.to_string())?;
                        read.read_exact(&mut ty).await.map_err(|e| e.to_string())?;
                        let len = u16::from_be_bytes(len);
                        let ty = u16::from_be_bytes(ty);
                        let mut message = vec![0; len as usize];
                        read.read_exact(&mut message)
                            .await
                            .map_err(|e| e.to_string())?;
                        use protobuf::Enum;
                        match Bluetooth::MessageId::from_i32(ty as i32) {
                            Some(m) => match m {
                                Bluetooth::MessageId::BLUETOOTH_SOCKET_INFO_REQUEST => {
                                    log::error!("Got a socket info request {:x?}", message);
                                    break;
                                }
                                Bluetooth::MessageId::BLUETOOTH_NETWORK_INFO_REQUEST => {
                                    let mut response = Bluetooth::NetworkInfo::new();
                                    response.set_ssid(network2.ssid.clone());
                                    response.set_psk(network2.psk.clone());
                                    response.set_mac_addr(network2.mac_addr.clone());
                                    response.set_security_mode(network2.security_mode.clone());
                                    response.set_ap_type(network2.ap_type.clone());
                                    let response =
                                        AndroidAutoBluetoothMessage::NetworkInfoMessage(response);
                                    let m: AndroidAutoMessage = response.as_message();
                                    let mdata: Vec<u8> = m.into();
                                    let r1 = write.write_all(&mdata).await;
                                }
                                Bluetooth::MessageId::BLUETOOTH_SOCKET_INFO_RESPONSE => {
                                    let message =
                                        Bluetooth::SocketInfoResponse::parse_from_bytes(&message);
                                    log::info!("Message is now {:?}", message);
                                }
                                _ => {}
                            },
                            _ => {
                                log::error!("Unknown bluetooth packet {} {:x?}", ty, message);
                            }
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    Ok::<(), String>(())
                });
            }
        }
    }

    async fn handle_client<T: AndroidAutoMainTrait>(
        mut stream: tokio::net::TcpStream,
        addr: std::net::SocketAddr,
        config: AndroidAutoConfiguration,
        main: &mut T,
    ) -> Result<(), String> {
        let mut root_store =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let aautocertder = {
            let mut br = std::io::Cursor::new(cert::AAUTO_CERT.to_string().as_bytes().to_vec());
            let aautocertpem = rustls::pki_types::pem::from_buf(&mut br)
                .expect("Failed to parse pem for aauto server")
                .expect("Invalid pem sert vor aauto server");
            CertificateDer::from_pem(aautocertpem.0, aautocertpem.1).unwrap()
        };
        let cert = {
            let mut br = std::io::Cursor::new(cert::CERTIFICATE.to_string().as_bytes().to_vec());
            let aautocertpem = rustls::pki_types::pem::from_buf(&mut br)
                .expect("Failed to parse pem for aauto client")
                .expect("Invalid pem cert for aauto client");
            CertificateDer::from_pem(aautocertpem.0, aautocertpem.1).unwrap()
        };
        log::error!("Cert is {:02x?}", cert);
        let key = {
            let mut br = std::io::Cursor::new(cert::PRIVATE_KEY.to_string().as_bytes().to_vec());
            let aautocertpem = rustls::pki_types::pem::from_buf(&mut br)
                .expect("Failed to parse pem for aauto client")
                .expect("Invalid pem cert for aauto client");
            rustls::pki_types::PrivateKeyDer::from_pem(aautocertpem.0, aautocertpem.1).unwrap()
        };
        let cert = vec![cert];
        root_store
            .add(aautocertder)
            .expect("Failed to load android auto server cert");
        let root_store = Arc::new(root_store);
        let mut ssl_client_config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store.clone())
            .with_client_auth_cert(cert, key)
            .unwrap();
        let sver = Arc::new(AndroidAutoServerVerifier::new(root_store));
        ssl_client_config.dangerous().set_certificate_verifier(sver);
        let sslconfig = Arc::new(ssl_client_config);
        let server = "idontknow.com".try_into().unwrap();
        let mut ssl_client =
            rustls::ClientConnection::new(sslconfig, server).expect("Failed to build ssl client");
        log::error!(
            "SSL WANTS RX {} TX {}",
            ssl_client.wants_read(),
            ssl_client.wants_write()
        );

        let mut channel_handlers: Vec<ChannelHandler> = Vec::new();
        channel_handlers.push(
            ControlChannelHandler {
                channels: Vec::new(),
            }
            .into(),
        );
        channel_handlers.push(InputChannelHandler {}.into());
        channel_handlers.push(SensorChannelHandler {}.into());
        if main.supports_video().is_some() {
            log::info!("Setting up video channel");
            channel_handlers.push(VideoChannelHandler {}.into());
        }
        channel_handlers.push(MediaAudioChannelHandler {}.into());
        channel_handlers.push(SpeechAudioChannelHandler {}.into());
        channel_handlers.push(SystemAudioChannelHandler {}.into());
        channel_handlers.push(AvInputChannelHandler {}.into());
        channel_handlers.push(BluetoothChannelHandler {}.into());
        channel_handlers.push(NavigationChannelHandler {}.into());
        channel_handlers.push(MediaStatusChannelHandler {}.into());

        let mut chans = Vec::new();
        let chan_visit = [7, 4, 5, 6, 2, 3, 8, 9, 10, 1];
        for index in chan_visit {
            let handler = &channel_handlers[index];
            let chan: ChannelId = (index as u8).try_into().unwrap();
            if let Some(chan) = handler.build_channel(&config, chan) {
                chans.push(chan);
            }
        }
        channel_handlers.get_mut(0).unwrap().set_channels(chans);
        log::debug!(
            "Got a connection on port {} from {:?}",
            config.network.port,
            addr
        );
        let m = AndroidAutoControlMessage::VersionRequest;
        let d: AndroidAutoFrame = m.into();
        let d2: Vec<u8> = d.build_vec(Some(&mut ssl_client)).await;
        stream.write_all(&d2).await.map_err(|e| e.to_string())?;
        let mut fr2 = AndroidAutoFrameReceiver::new();
        loop {
            let mut skip_ping = true;
            let mut fr = FrameHeaderReceiver::new();
            let f = loop {
                match fr.read(&mut stream).await {
                    Ok(Some(f)) => break Some(f),
                    Err(e) => match e.kind() {
                        std::io::ErrorKind::NotFound => todo!(),
                        std::io::ErrorKind::PermissionDenied => todo!(),
                        std::io::ErrorKind::ConnectionRefused => todo!(),
                        std::io::ErrorKind::ConnectionReset => todo!(),
                        std::io::ErrorKind::HostUnreachable => todo!(),
                        std::io::ErrorKind::NetworkUnreachable => todo!(),
                        std::io::ErrorKind::ConnectionAborted => todo!(),
                        std::io::ErrorKind::NotConnected => todo!(),
                        std::io::ErrorKind::AddrInUse => todo!(),
                        std::io::ErrorKind::AddrNotAvailable => todo!(),
                        std::io::ErrorKind::NetworkDown => todo!(),
                        std::io::ErrorKind::BrokenPipe => todo!(),
                        std::io::ErrorKind::AlreadyExists => todo!(),
                        std::io::ErrorKind::WouldBlock => break None,
                        std::io::ErrorKind::NotADirectory => todo!(),
                        std::io::ErrorKind::IsADirectory => todo!(),
                        std::io::ErrorKind::DirectoryNotEmpty => todo!(),
                        std::io::ErrorKind::ReadOnlyFilesystem => todo!(),
                        std::io::ErrorKind::StaleNetworkFileHandle => todo!(),
                        std::io::ErrorKind::InvalidInput => todo!(),
                        std::io::ErrorKind::InvalidData => todo!(),
                        std::io::ErrorKind::TimedOut => todo!(),
                        std::io::ErrorKind::WriteZero => todo!(),
                        std::io::ErrorKind::StorageFull => todo!(),
                        std::io::ErrorKind::NotSeekable => todo!(),
                        std::io::ErrorKind::QuotaExceeded => todo!(),
                        std::io::ErrorKind::Deadlock => todo!(),
                        std::io::ErrorKind::CrossesDevices => todo!(),
                        std::io::ErrorKind::TooManyLinks => todo!(),
                        std::io::ErrorKind::ArgumentListTooLong => todo!(),
                        std::io::ErrorKind::Interrupted => todo!(),
                        std::io::ErrorKind::Unsupported => todo!(),
                        std::io::ErrorKind::UnexpectedEof => todo!(),
                        std::io::ErrorKind::OutOfMemory => todo!(),
                        std::io::ErrorKind::Other => todo!("{}", e.to_string()),
                        _ => return Err("Unknown error reading frame header".to_string()),
                    },
                    _ => break None,
                }
            };
            let f2 = if let Some(f) = f {
                let f2 = loop {
                    match fr2.read(&f, &mut stream, &mut ssl_client).await {
                        Ok(Some(f2)) => break Some(f2),
                        Ok(None) => {
                            skip_ping = true;
                            break None;
                        }
                        Err(e) => match e.kind() {
                            std::io::ErrorKind::NotFound => todo!(),
                            std::io::ErrorKind::PermissionDenied => todo!(),
                            std::io::ErrorKind::ConnectionRefused => todo!(),
                            std::io::ErrorKind::ConnectionReset => todo!(),
                            std::io::ErrorKind::HostUnreachable => todo!(),
                            std::io::ErrorKind::NetworkUnreachable => todo!(),
                            std::io::ErrorKind::ConnectionAborted => todo!(),
                            std::io::ErrorKind::NotConnected => todo!(),
                            std::io::ErrorKind::AddrInUse => todo!(),
                            std::io::ErrorKind::AddrNotAvailable => todo!(),
                            std::io::ErrorKind::NetworkDown => todo!(),
                            std::io::ErrorKind::BrokenPipe => todo!(),
                            std::io::ErrorKind::AlreadyExists => todo!(),
                            std::io::ErrorKind::WouldBlock => {}
                            std::io::ErrorKind::NotADirectory => todo!(),
                            std::io::ErrorKind::IsADirectory => todo!(),
                            std::io::ErrorKind::DirectoryNotEmpty => todo!(),
                            std::io::ErrorKind::ReadOnlyFilesystem => todo!(),
                            std::io::ErrorKind::StaleNetworkFileHandle => todo!(),
                            std::io::ErrorKind::InvalidInput => todo!(),
                            std::io::ErrorKind::InvalidData => todo!(),
                            std::io::ErrorKind::TimedOut => todo!(),
                            std::io::ErrorKind::WriteZero => todo!(),
                            std::io::ErrorKind::StorageFull => todo!(),
                            std::io::ErrorKind::NotSeekable => todo!(),
                            std::io::ErrorKind::QuotaExceeded => todo!(),
                            std::io::ErrorKind::FileTooLarge => todo!(),
                            std::io::ErrorKind::ResourceBusy => todo!(),
                            std::io::ErrorKind::ExecutableFileBusy => todo!(),
                            std::io::ErrorKind::Deadlock => todo!(),
                            std::io::ErrorKind::CrossesDevices => todo!(),
                            std::io::ErrorKind::TooManyLinks => todo!(),
                            std::io::ErrorKind::ArgumentListTooLong => todo!(),
                            std::io::ErrorKind::Interrupted => todo!(),
                            std::io::ErrorKind::Unsupported => todo!(),
                            std::io::ErrorKind::UnexpectedEof => todo!(),
                            std::io::ErrorKind::OutOfMemory => todo!(),
                            std::io::ErrorKind::Other => todo!("{}", e.to_string()),
                            _ => return Err("Unknown error reading frame header".to_string()),
                        },
                    }
                };
                f2
            } else {
                None
            };
            if let Some(f2) = f2 {
                if let Some(handler) = channel_handlers.get_mut(f2.header.channel_id as usize) {
                    log::error!("Receiving data for channel {:?}", f2.header.channel_id);
                    handler
                        .receive_data(
                            f2,
                            &mut skip_ping,
                            &mut stream,
                            &mut ssl_client,
                            &config,
                            main,
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                } else {
                    panic!("Unknown channel id: {:?}", f2.header.channel_id);
                }
            }
        }
        log::info!("Disconnecting normally");
        Ok(())
    }

    #[cfg(feature = "wireless")]
    pub async fn wifi_listen<T: AndroidAutoMainTrait>(
        config: AndroidAutoConfiguration,
        mut main: T,
    ) -> Result<(), String> {
        let cp = rustls::crypto::ring::default_provider();
        cp.install_default().expect("Failed to set ssl provider");

        log::debug!(
            "Listening on port {} for android auto stuff",
            config.network.port
        );
        if let Ok(a) =
            tokio::net::TcpListener::bind(format!("0.0.0.0:{}", config.network.port)).await
        {
            loop {
                if let Ok((stream, addr)) = a.accept().await {
                    let config2 = config.clone();
                    if let Err(e) = Self::handle_client(stream, addr, config2, &mut main).await {
                        log::error!("Disconnect from client: {:?}", e);
                    }
                }
            }
        } else {
            Err(format!(
                "Failed to listen on port {} tcp",
                config.network.port
            ))
        }
    }

    #[cfg(not(feature = "wireless"))]
    pub async fn new() -> Self {
        Self {}
    }
}
