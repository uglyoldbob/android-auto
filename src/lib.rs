//! This crate provides android auto functionality for devices wishing to comunicate using the android auto protocol.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

mod cert;

use ::protobuf::Message;
use Wifi::ChannelDescriptor;
use rustls::pki_types::{CertificateDer, pem::PemObject};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod control;
use control::*;
mod common;
use common::*;
mod video;
use video::*;
mod bluetooth;
use bluetooth::*;
mod sensor;
use sensor::*;

/// The base trait for crate users to implement
pub trait AndroidAutoMainTrait {
    /// This allows the incoming video stream to be processed
    #[inline(always)]
    fn supports_video(&mut self) -> Option<&mut dyn AndroidAutoVideoChannelTrait> {
        None
    }
}

/// This trait is implemented by users wishing to display a video stream from an android auto (phone probably).
#[async_trait::async_trait]
pub trait AndroidAutoVideoChannelTrait: AndroidAutoMainTrait {
    /// Parse a chunk of h264 video data
    async fn receive_video(&mut self, data: Vec<u8>);
    /// Setup the video device to receive h264 video, if anything is required
    async fn setup_video(&mut self) -> bool;
    /// Tear down the video receiver, may be called without the setup having been called
    async fn teardown_video(&mut self);
    /// Wait for the video to be in focus
    async fn wait_for_focus(&mut self);
}

/// This is the bluetooth server for initiating wireless android auto on compatible devices.
pub struct AndriodAutoBluettothServer {
    /// The profile needed to communicate over bluetooth
    #[cfg(feature = "wireless")]
    blue: bluetooth_rust::RfcommProfileHandle,
}

#[allow(missing_docs)]
#[allow(clippy::missing_docs_in_private_items)]
mod protobufmod {
    include!(concat!(env!("OUT_DIR"), "/protobuf/mod.rs"));
}
pub use protobufmod::*;

/// The android auto version supported
const VERSION: (u16, u16) = (1, 1);

/// A message sent or received in the android auto protocol
#[cfg(feature = "wireless")]
struct AndroidAutoMessage {
    /// The message type
    t: u16,
    /// The message contained in the message
    message: Vec<u8>,
}

/// The wireless network information to relay to the compatible android auto device
#[derive(Clone)]
pub struct NetworkInformation {
    /// The ssid of the wireless network
    pub ssid: String,
    /// The password for the wireless network
    pub psk: String,
    /// Unsure, probably the mac address of the android auto host
    pub mac_addr: String,
    /// The ip address of the android auto host
    pub ip: String,
    /// The port that the android auto host should listen on
    pub port: u16,
    /// The security mode for the wireless network
    pub security_mode: Bluetooth::SecurityMode,
    /// The access point type of the wireless network
    pub ap_type: Bluetooth::AccessPointType,
}

/// Information about the head unit that will be providing android auto services for compatible devices
#[derive(Clone)]
pub struct HeadUnitInfo {
    /// The name of the head unit
    pub name: String,
    /// The model of the vehicle
    pub car_model: String,
    /// The year of the vehicle
    pub car_year: String,
    /// The serial number of the vehicle
    pub car_serial: String,
    /// True when the vehicle is a left hand drive, false when a right hand drive
    pub left_hand: bool,
    /// The manufacturer of the head unit
    pub head_manufacturer: String,
    /// The model of the head unit
    pub head_model: String,
    /// The software build for the head unit
    pub sw_build: String,
    /// The software version for the head unit
    pub sw_version: String,
    /// Does the head unit support native media during vr
    pub native_media: bool,
    /// Should the clock be hidden?
    pub hide_clock: Option<bool>,
}

/// The required bluetooth information
#[derive(Clone)]
pub struct BluetoothInformation {
    /// The mac address of the bluetooth adapter
    pub address: String,
}

/// Provides basic configuration elements for setting up an android auto head unit
#[derive(Clone)]
pub struct AndroidAutoConfiguration {
    /// The wireless network information
    pub network: NetworkInformation,
    /// The bluetooth information
    pub bluetooth: BluetoothInformation,
    /// The head unit information
    pub unit: HeadUnitInfo,
}

/// The channel identifier for channels in the android auto protocol
type ChannelId = u8;

/// Specifies the type of frame header, whether the data of a packet is contained in a single frame, or if it was too large and broken up into multiple frames for transmission.
#[derive(Debug, PartialEq)]
#[repr(u8)]
pub enum FrameHeaderType {
    /// This frame is neither the first or the last of a multi-frame packet
    Middle = 0,
    /// This is the first frame of a multi-frame packet
    First = 1,
    /// This is the last frame of a multi-frame packet
    Last = 2,
    /// The packet is contained in a single frame
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

impl From<FrameHeaderType> for u8 {
    fn from(value: FrameHeaderType) -> Self {
        value as u8
    }
}

#[allow(missing_docs)]
/// The frame header module, because bitfield new does not make documentation yet.
mod frame_header {
    bitfield::bitfield! {
        #[derive(Copy, Clone)]
        pub struct FrameHeaderContents(u8);
        impl Debug;
        impl new;
        u8;
        /// True indicates the frame is encrypted
        pub get_encryption, set_encryption: 3;
        /// The frame header type
        pub from into super::FrameHeaderType, get_frame_type, set_frame_type: 1, 0;
        /// True when frame is for control, false when specific
        pub get_control, set_control: 2;
    }
}
use frame_header::FrameHeaderContents;

/// Represents the header of a frame sent to the android auto client
#[derive(Copy, Clone, Debug)]
struct FrameHeader {
    /// The channelid that this frame is intended for
    channel_id: ChannelId,
    /// The contents of the frame header
    frame: FrameHeaderContents,
}

impl FrameHeader {
    /// Add self to the given buffer to build part of a complete frame
    pub fn add_to(&self, buf: &mut Vec<u8>) {
        buf.push(self.channel_id);
        buf.push(self.frame.0);
    }
}

/// Responsible for receiving frame headers in the the android auto protocol.
struct FrameHeaderReceiver {
    /// The channel id received for a frame header, if one has been received.
    channel_id: Option<ChannelId>,
}

impl FrameHeaderReceiver {
    /// Construct a new self
    pub fn new() -> Self {
        Self { channel_id: None }
    }

    /// Read a frame header from the compatible android auto device
    /// Returns Ok(Some(p)) when a full frame header is actually received.
    pub async fn read(
        &mut self,
        stream: &mut tokio::net::TcpStream,
    ) -> Result<Option<FrameHeader>, std::io::Error> {
        if self.channel_id.is_none() {
            let mut b = [0u8];
            stream.read_exact(&mut b).await?;
            self.channel_id = ChannelId::try_from(b[0]).ok();
        }
        if let Some(channel_id) = &self.channel_id {
            let mut b = [0u8];
            stream.read_exact(&mut b).await?;
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

/// A frame of data for comunication in the android auto. When receiving frames, multi-frames are combined into a single frame.
#[derive(Debug)]
struct AndroidAutoFrame {
    /// The header of the frame
    header: FrameHeader,
    /// The data actually relayed in the frame
    data: Vec<u8>,
}

impl AndroidAutoFrame {
    /// The largest payload for a single frame
    const MAX_FRAME_DATA_SIZE: usize = 0x4000;
    #[allow(dead_code)]
    /// Currently unused function for building a set of frames for a large packet
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
                let mut h = f;
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

    /// Build a vec with the frame that is ready to send out over the connection to the compatible android auto device.
    /// If necessary, the data will be encrypted.
    async fn build_vec(&self, stream: Option<&mut rustls::client::ClientConnection>) -> Vec<u8> {
        let mut buf = Vec::new();
        self.header.add_to(&mut buf);
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
        buf
    }
}

/// Responsible for receiving a full frame from the compatible android auto device
struct AndroidAutoFrameReceiver {
    /// The length of the frame to receive, if it is known yet
    len: Option<u16>,
    /// The data received so far for a multi-frame packet
    rx_sofar: Vec<Vec<u8>>,
}

impl AndroidAutoFrameReceiver {
    /// Construct a new frame receiver
    fn new() -> Self {
        Self {
            len: None,
            rx_sofar: Vec::new(),
        }
    }

    /// Read the contents of a frame using the details specified in the header that has already been read.
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

        let decrypt = |ssl_stream: &mut rustls::client::ClientConnection,
                       _len: u16,
                       data_frame: Vec<u8>|
         -> Result<Vec<u8>, std::io::Error> {
            let mut plain_data = vec![0u8; data_frame.len()];
            let mut cursor = Cursor::new(&data_frame);
            let mut index = 0;
            loop {
                let asdf = ssl_stream.read_tls(&mut cursor).unwrap();
                let _ = ssl_stream
                    .process_new_packets()
                    .map_err(|e| std::io::Error::other(e))?;
                if asdf == 0 {
                    break;
                }
                if let Ok(l) = ssl_stream.reader().read(&mut plain_data[index..]) {
                    index += l;
                }
            }
            Ok(plain_data[0..index].to_vec())
        };

        if let Some(len) = self.len.take() {
            let mut data_frame = vec![0u8; len as usize];
            stream.read_exact(&mut data_frame).await?;
            let data = if header.frame.get_frame_type() == FrameHeaderType::Single {
                let data_plain = if header.frame.get_encryption() {
                    decrypt(ssl_stream, len, data_frame)?
                } else {
                    data_frame
                };
                let d = data_plain.clone();
                Some(vec![d])
            } else {
                let data_plain = if header.frame.get_encryption() {
                    decrypt(ssl_stream, len, data_frame)?
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
                    header: *header,
                    data,
                };
                let f = Some(f);
                return Ok(f);
            }
        }
        Ok(None)
    }
}

/// A message sent or received over the android auto bluetooth connection. Used for setting up wireless android auto.
enum AndroidAutoBluetoothMessage {
    /// A request for socket information
    SocketInfoRequest(Bluetooth::SocketInfoRequest),
    /// A message relaying network information to the other party
    NetworkInfoMessage(Bluetooth::NetworkInfo),
}

impl AndroidAutoBluetoothMessage {
    /// Build an `AndroidAutoMessage` from self
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

impl From<AndroidAutoMessage> for Vec<u8> {
    fn from(value: AndroidAutoMessage) -> Self {
        let mut buf = Vec::new();
        let b = value.message.len() as u16;
        let a = b.to_be_bytes();
        buf.push(a[0]);
        buf.push(a[1]);
        let a = value.t.to_be_bytes();
        buf.push(a[0]);
        buf.push(a[1]);
        for b in value.message {
            buf.push(b);
        }
        buf
    }
}

/// The trait that all channel handlers must implement for android auto channels.
#[enum_dispatch::enum_dispatch]
trait ChannelHandlerTrait {
    /// Process data received that is specific to this channel. Return an error for any packets that were not handled that should cause communication to stop.
    async fn receive_data<T: AndroidAutoMainTrait>(
        &mut self,
        msg: AndroidAutoFrame,
        skip_ping: &mut bool,
        stream: &mut tokio::net::TcpStream,
        ssl_stream: &mut rustls::client::ClientConnection,
        config: &AndroidAutoConfiguration,
        main: &mut T,
    ) -> Result<(), std::io::Error>;

    /// Construct the channeldescriptor with the channel handler so it can be conveyed to the compatible android auto device
    fn build_channel(
        &self,
        config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<ChannelDescriptor>;

    /// Set the list of all channels for the current channel. Only used for the control channel. This is because the control channel must be created first.
    fn set_channels(&mut self, _chans: Vec<ChannelDescriptor>) {}
}

/// A message about binding input buttons on a compatible android auto head unit
enum InputMessage {
    /// A message requesting input buttons to be bound
    BindingRequest(ChannelId, Wifi::BindingRequest),
    /// A message that responds to a binding request, indicating success or failure of the request
    BindingResponse(ChannelId, Wifi::BindingResponse),
}

impl From<InputMessage> for AndroidAutoFrame {
    fn from(value: InputMessage) -> Self {
        match value {
            InputMessage::BindingRequest(_, _) => unimplemented!(),
            InputMessage::BindingResponse(chan, m) => {
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
                        Err(e) => Err(format!("Invalid input bind request: {}", e)),
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

/// The handler for the input channel for the android auto protocol
struct InputChannelHandler {}

impl ChannelHandlerTrait for InputChannelHandler {
    fn build_channel(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<ChannelDescriptor> {
        let mut chan = ChannelDescriptor::new();
        chan.set_channel_id(chanid as u32);
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
        _main: &mut T,
    ) -> Result<(), std::io::Error> {
        let channel = msg.header.channel_id;
        let msg2: Result<InputMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                InputMessage::BindingRequest(chan, _m) => {
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
                AndroidAutoCommonMessage::ChannelOpenRequest(_m) => {
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

/// The handler for the media audio channel for the android auto protocol
struct MediaAudioChannelHandler {}

impl ChannelHandlerTrait for MediaAudioChannelHandler {
    fn build_channel(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<ChannelDescriptor> {
        let mut chan = ChannelDescriptor::new();
        chan.set_channel_id(chanid as u32);
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
        _main: &mut T,
    ) -> Result<(), std::io::Error> {
        let channel = msg.header.channel_id;
        let msg2: Result<AndroidAutoCommonMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoCommonMessage::ChannelOpenResponse(_, _) => unimplemented!(),
                AndroidAutoCommonMessage::ChannelOpenRequest(_m) => {
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
                AvChannelMessage::SetupRequest(_chan, _m) => {
                    let mut m2 = Wifi::AVChannelSetupResponse::new();
                    m2.set_max_unacked(10);
                    m2.set_media_status(Wifi::avchannel_setup_status::Enum::OK);
                    m2.configs.push(0);
                    let d: AndroidAutoFrame = AvChannelMessage::SetupResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
                AvChannelMessage::SetupResponse(_chan, _m) => unimplemented!(),
                AvChannelMessage::VideoFocusRequest(_chan, _m) => {
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

/// A message about the media status of currently playing media
#[derive(Debug)]
enum MediaStatusMessage {
    /// A message containing basic information about changes to the currently playing media
    Playback(ChannelId, Wifi::MediaInfoChannelPlaybackData),
    /// The metadata containing information about the media currently playing
    Metadata(ChannelId, Wifi::MediaInfoChannelMetadataData),
    /// The media status message was invalid for some reason
    Invalid,
}

impl From<MediaStatusMessage> for AndroidAutoFrame {
    fn from(value: MediaStatusMessage) -> Self {
        match value {
            MediaStatusMessage::Playback(_, _) => todo!(),
            MediaStatusMessage::Metadata(_, _) => todo!(),
            MediaStatusMessage::Invalid => unimplemented!(),
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
                        Err(_) => Ok(Self::Invalid),
                    }
                }
                Wifi::media_info_channel_message::Enum::METADATA => {
                    let m = Wifi::MediaInfoChannelMetadataData::parse_from_bytes(&value.data);
                    match m {
                        Ok(m) => Ok(Self::Metadata(value.header.channel_id, m)),
                        Err(_) => Ok(Self::Invalid),
                    }
                }
                Wifi::media_info_channel_message::Enum::NONE => todo!(),
            }
        } else {
            Err(format!("Not converted message: {:x?}", value.data))
        }
    }
}

/// The handler for media status for the android auto protocol
struct MediaStatusChannelHandler {}

impl ChannelHandlerTrait for MediaStatusChannelHandler {
    fn build_channel(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<ChannelDescriptor> {
        let mut chan = ChannelDescriptor::new();
        chan.set_channel_id(chanid as u32);
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
        _main: &mut T,
    ) -> Result<(), std::io::Error> {
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
                AndroidAutoCommonMessage::ChannelOpenRequest(_m) => {
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

/// The handler for navigation for the android auto protocol
struct NavigationChannelHandler {}

impl ChannelHandlerTrait for NavigationChannelHandler {
    fn build_channel(
        &self,
        _config: &AndroidAutoConfiguration,
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
        chan.set_channel_id(chanid as u32);
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
        let channel = msg.header.channel_id;
        let msg2: Result<AndroidAutoCommonMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoCommonMessage::ChannelOpenResponse(_, _) => unimplemented!(),
                AndroidAutoCommonMessage::ChannelOpenRequest(_m) => {
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

/// The handler for speech audio for the android auto protocol
struct SpeechAudioChannelHandler {}

impl ChannelHandlerTrait for SpeechAudioChannelHandler {
    fn build_channel(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<ChannelDescriptor> {
        let mut chan = ChannelDescriptor::new();
        chan.set_channel_id(chanid as u32);
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
        let channel = msg.header.channel_id;
        let msg2: Result<AndroidAutoCommonMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoCommonMessage::ChannelOpenResponse(_, _) => unimplemented!(),
                AndroidAutoCommonMessage::ChannelOpenRequest(_m) => {
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
                AvChannelMessage::SetupRequest(_chan, _m) => {
                    let mut m2 = Wifi::AVChannelSetupResponse::new();
                    m2.set_max_unacked(10);
                    m2.set_media_status(Wifi::avchannel_setup_status::Enum::OK);
                    m2.configs.push(0);
                    let d: AndroidAutoFrame = AvChannelMessage::SetupResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
                AvChannelMessage::SetupResponse(_chan, _m) => unimplemented!(),
                AvChannelMessage::VideoFocusRequest(_chan, _m) => {
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

/// A message sent for an av channel
enum AvChannelMessage {
    /// A message to start setup of the av channel
    SetupRequest(ChannelId, Wifi::AVChannelSetupRequest),
    /// A message that responds to a setup request
    SetupResponse(ChannelId, Wifi::AVChannelSetupResponse),
    /// Message requesting the focus of the video channel to be set
    VideoFocusRequest(ChannelId, Wifi::VideoFocusRequest),
    /// Message indication the focus status of the video stream on the head unit
    VideoIndicationResponse(ChannelId, Wifi::VideoFocusIndication),
    /// The stream is about to start
    StartIndication(ChannelId, Wifi::AVChannelStartIndication),
    /// A media indication message, optionally containing a timestamp
    MediaIndication(ChannelId, Option<u64>, Vec<u8>),
    /// An acknowledgement of receiving a media indication message
    MediaIndicationAck(ChannelId, Wifi::AVMediaAckIndication),
}

impl From<AvChannelMessage> for AndroidAutoFrame {
    fn from(value: AvChannelMessage) -> Self {
        match value {
            AvChannelMessage::MediaIndicationAck(chan, m) => {
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
            AvChannelMessage::SetupRequest(_, _) => unimplemented!(),
            AvChannelMessage::SetupResponse(chan, m) => {
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
            AvChannelMessage::MediaIndication(_, _, _) => unimplemented!(),
            AvChannelMessage::VideoFocusRequest(_chan, _m) => unimplemented!(),
            AvChannelMessage::VideoIndicationResponse(chan, m) => {
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
            AvChannelMessage::StartIndication(_, _) => unimplemented!(),
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
                        Err(e) => Err(format!("Invalid channel open request: {}", e)),
                    }
                }
                Wifi::avchannel_message::Enum::START_INDICATION => {
                    let m = Wifi::AVChannelStartIndication::parse_from_bytes(&value.data[2..]);
                    match m {
                        Ok(m) => Ok(Self::StartIndication(value.header.channel_id, m)),
                        Err(e) => Err(format!("Invalid channel open request: {}", e)),
                    }
                }
                Wifi::avchannel_message::Enum::STOP_INDICATION => todo!(),
                Wifi::avchannel_message::Enum::SETUP_RESPONSE => unimplemented!(),
                Wifi::avchannel_message::Enum::AV_MEDIA_ACK_INDICATION => todo!(),
                Wifi::avchannel_message::Enum::AV_INPUT_OPEN_REQUEST => todo!(),
                Wifi::avchannel_message::Enum::AV_INPUT_OPEN_RESPONSE => todo!(),
                Wifi::avchannel_message::Enum::VIDEO_FOCUS_REQUEST => {
                    let m = Wifi::VideoFocusRequest::parse_from_bytes(&value.data[2..]);
                    match m {
                        Ok(m) => Ok(Self::VideoFocusRequest(value.header.channel_id, m)),
                        Err(e) => Err(format!("Invalid channel open request: {}", e)),
                    }
                }
                Wifi::avchannel_message::Enum::VIDEO_FOCUS_INDICATION => unimplemented!(),
            }
        } else {
            Err(format!("Not converted message: {:x?}", value.data))
        }
    }
}

/// Handles the system audo channel of the android auto protocol
struct SystemAudioChannelHandler {}

impl ChannelHandlerTrait for SystemAudioChannelHandler {
    fn build_channel(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<ChannelDescriptor> {
        let mut chan = ChannelDescriptor::new();
        chan.set_channel_id(chanid as u32);
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
        _main: &mut T,
    ) -> Result<(), std::io::Error> {
        let channel = msg.header.channel_id;
        let msg2: Result<AndroidAutoCommonMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoCommonMessage::ChannelOpenResponse(_, _) => unimplemented!(),
                AndroidAutoCommonMessage::ChannelOpenRequest(_m) => {
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
                AvChannelMessage::SetupRequest(_chan, _m) => {
                    let mut m2 = Wifi::AVChannelSetupResponse::new();
                    m2.set_max_unacked(10);
                    m2.set_media_status(Wifi::avchannel_setup_status::Enum::OK);
                    m2.configs.push(0);
                    let d: AndroidAutoFrame = AvChannelMessage::SetupResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
                AvChannelMessage::SetupResponse(_chan, _m) => unimplemented!(),
                AvChannelMessage::VideoFocusRequest(_chan, _m) => {
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

/// Handles the av input channel of the android auto protocol
struct AvInputChannelHandler {}

impl ChannelHandlerTrait for AvInputChannelHandler {
    fn build_channel(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<ChannelDescriptor> {
        let mut chan = ChannelDescriptor::new();
        chan.set_channel_id(chanid as u32);
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
        _main: &mut T,
    ) -> Result<(), std::io::Error> {
        let channel = msg.header.channel_id;
        let msg2: Result<AndroidAutoCommonMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoCommonMessage::ChannelOpenResponse(_, _) => unimplemented!(),
                AndroidAutoCommonMessage::ChannelOpenRequest(_m) => {
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

/// The server verifier for android auto head units. This verifies the certificate in the android auto compatible device (probably a phone)
#[derive(Debug)]
struct AndroidAutoServerVerifier {
    /// The object providing most of the functionality for server verification
    base: Arc<rustls::client::WebPkiServerVerifier>,
}

impl AndroidAutoServerVerifier {
    /// Build a new server verifier using the given root certificate store
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

/// The channel handler type that covers all possible channel handlers
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
    /// Create a new android auto bluetooth server, registering the profile required for android auto wireless operation.
    #[cfg(feature = "wireless")]
    pub async fn new(bluetooth: &mut bluetooth_rust::BluetoothHandler) -> Option<Self> {
        let profile = bluetooth_rust::RfcommProfile {
            uuid: bluetooth_rust::Uuid::parse_str(
                bluetooth_rust::BluetoothUuid::AndroidAuto.as_str(),
            )
            .ok()?,
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
        Some(Self { blue: a.ok()? })
    }

    /// Start a listener that listens for connections on the android auto provile
    #[cfg(feature = "wireless")]
    pub async fn bluetooth_listen(&mut self, network: NetworkInformation) -> ! {
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
                    let _ = write.write_all(&mdata).await;
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
                                    response.set_security_mode(network2.security_mode);
                                    response.set_ap_type(network2.ap_type);
                                    let response =
                                        AndroidAutoBluetoothMessage::NetworkInfoMessage(response);
                                    let m: AndroidAutoMessage = response.as_message();
                                    let mdata: Vec<u8> = m.into();
                                    let _ = write.write_all(&mdata).await;
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

    /// Handle a single android auto device for a head unit
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

        let mut channel_handlers: Vec<ChannelHandler> = Vec::new();
        channel_handlers.push(ControlChannelHandler::new().into());
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
            let chan: ChannelId = index as u8;
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
                loop {
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
                }
            } else {
                None
            };
            if let Some(f2) = f2 {
                if let Some(handler) = channel_handlers.get_mut(f2.header.channel_id as usize) {
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

    /// Listen for connections over the network for an android auto capable head unit.
    /// This will return an error if it was unable to listen on the specified port
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
