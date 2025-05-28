//! This crate provides android auto functionality for devices wishing to comunicate using the android auto protocol.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

use std::{
    collections::HashSet,
    io::{Cursor, Read, Write},
    sync::Arc,
};

mod cert;

use ::protobuf::Message;
use Wifi::ChannelDescriptor;
use bluetooth_rust::{
    BluetoothRfcommConnectableTrait, BluetoothRfcommProfileTrait, BluetoothStream,
};
use rustls::pki_types::{CertificateDer, pem::PemObject};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

mod avinput;
use avinput::*;
mod bluetooth;
use bluetooth::*;
mod common;
use common::*;
mod control;
use control::*;
mod input;
use input::*;
mod mediaaudio;
use mediaaudio::*;
mod mediastatus;
use mediastatus::*;
mod navigation;
use navigation::*;
mod sensor;
use sensor::*;
mod speechaudio;
use speechaudio::*;
mod sysaudio;
use sysaudio::*;
mod video;
use video::*;

pub use protobuf;

/// Errors that can occur when trying to receive frames
#[derive(Debug)]
pub enum FrameReceiptError {
    /// A timeout occurred when trying to receive the frame header
    TimeoutHeader,
    /// The connection was disconnected
    Disconnected,
    /// An unexpected error receiving the frame channel id
    UnexpectedDuringFrameChannel(std::io::Error),
    /// An unexpected error receiving the frame header
    UnexpectedDuringFrameHeader(std::io::Error),
    /// An unexpected error receiving the frame length
    UnexpectedDuringFrameLength(std::io::Error),
    /// An unexpected error receiving the frame contents
    UnexpectedDuringFrameContents(std::io::Error),
    /// An error occurred calling read_tls with the received frame payload
    TlsReadError(std::io::Error),
    /// An error occurred processing tls data received
    TlsProcessingError(rustls::Error),
}

/// An error that can occur when transmitting a frame
#[derive(Debug)]
pub enum FrameTransmissionError {
    /// A timeout occurred while transmitting
    Timeout,
    /// The connection was disconnected
    Disconnected,
    /// An unexpected error
    Unexpected(std::io::Error),
}

/// A sequence error in frames received
#[derive(Debug)]
pub enum FrameSequenceError {
    /// Video data was received with the video channel not being open
    VideoChannelNotOpen,
}

/// Errors that can occur when either sending or receiving frames
#[derive(Debug)]
pub enum FrameIoError {
    /// An error receiving a frame
    Rx(FrameReceiptError),
    /// An error sending a frame
    Tx(FrameTransmissionError),
    /// A shutdown was requested
    ShutdownRequested,
    /// The client has an incompatible version
    IncompatibleVersion(u16, u16),
    /// An error occurred during the ssl handshake
    SslHandshake(SslHandshakeError),
    /// A logical error due to frames not being received in the expected order
    Sequence(FrameSequenceError),
    /// An error occurred opening the audio input channel
    AudioInputOpenError,
    /// An error occurred closing the audio input channel
    AudioInputCloseError,
}

/// Errors that can occur during the handshake process
#[derive(Debug)]
pub enum SslHandshakeError {
    /// A timeout occurred
    Timeout,
    /// The connection was disconnected
    Disconnected,
    /// An unexpected error
    Unexpected(std::io::Error),
}

/// Errors that can occur during communication with a client
#[derive(Debug)]
pub enum ClientError {
    /// The root certificate for the ssl communications was invalid
    InvalidRootCert,
    /// The client certificate was invalid
    InvalidClientCertificate,
    /// The client private key was invalid
    InvalidClientPrivateKey,
    /// A communication error
    IoError(FrameIoError),
}

impl From<FrameTransmissionError> for FrameIoError {
    fn from(value: FrameTransmissionError) -> Self {
        Self::Tx(value)
    }
}

impl From<SslHandshakeError> for FrameIoError {
    fn from(value: SslHandshakeError) -> Self {
        FrameIoError::SslHandshake(value)
    }
}

impl From<FrameSequenceError> for FrameIoError {
    fn from(value: FrameSequenceError) -> Self {
        FrameIoError::Sequence(value)
    }
}

impl From<FrameIoError> for ClientError {
    fn from(value: FrameIoError) -> Self {
        ClientError::IoError(value)
    }
}

/// The list of channel handlers for the current android auto instance
static CHANNEL_HANDLERS: tokio::sync::RwLock<Vec<ChannelHandler>> =
    tokio::sync::RwLock::const_new(Vec::new());

/// The base trait for crate users to implement
#[async_trait::async_trait]
pub trait AndroidAutoMainTrait: Send + Sync {
    /// This allows the incoming video stream to be processed
    #[inline(always)]
    fn supports_video(&self) -> Option<&dyn AndroidAutoVideoChannelTrait> {
        None
    }

    /// Implement this to indicate that bluetooth hardware is possible, return None if bluetooth hardware is not present
    #[inline(always)]
    fn supports_bluetooth(&self) -> Option<&dyn AndroidAutoBluetoothTrait> {
        None
    }
    

    /// Implement this to support wireless android auto communications
    #[inline(always)]
    fn supports_wireless(&self) -> Option<Arc<dyn AndroidAutoWirelessTrait>> {
        None
    }

    /// Implement this to support input
    #[inline(always)]
    fn supports_input(&self) -> Option<&dyn AndroidAutoInputChannelTrait> {
        None
    }

    /// Implement this to support audio output
    fn supports_audio_output(&self) -> Option<&dyn AndroidAutoAudioOutputTrait> {
        None
    }

    /// Implement this to support audio input
    fn supports_audio_input(&self) -> Option<&dyn AndroidAutoAudioInputTrait> {
        None
    }

    /// Implement this to support navigation
    fn supports_navigation(&self) -> Option<&dyn AndroidAutoNavigationTrait> {
        None
    }

    /// Implement this to support sensors
    fn supports_sensors(&self) -> Option<&dyn AndroidAutoSensorTrait> {
        None
    }

    /// The android auto device just connected
    async fn connect(&self);

    /// The android auto device disconnected
    async fn disconnect(&self);

    /// Retrieve the receiver so that the user can send messages to the android auto compatible device or crate
    async fn get_receiver(&self)
    -> Option<tokio::sync::mpsc::Receiver<SendableAndroidAutoMessage>>;
}

/// this trait is implemented by users that support bluetooth and wifi (both are required for wireless android auto)
#[async_trait::async_trait]
pub trait AndroidAutoWirelessTrait: AndroidAutoMainTrait {
    /// The function to setup the android auto profile
    async fn setup_bluetooth_profile(
        &self,
        suggestions: &bluetooth_rust::BluetoothRfcommProfileSettings,
    ) -> Result<bluetooth_rust::BluetoothRfcommProfile, String>;

    /// Returns wifi details
    fn get_wifi_details(&self) -> NetworkInformation;
}

/// This trait is implemented by users that support navigation indicators
#[async_trait::async_trait]
pub trait AndroidAutoSensorTrait: AndroidAutoMainTrait {
    /// Returns the types of sensors supported
    fn get_supported_sensors(&self) -> &SensorInformation;
    /// Start the indicated sensor
    async fn start_sensor(&self, stype: Wifi::sensor_type::Enum) -> Result<(), ()>;
}

/// This trait is implemented by users that support navigation indicators
#[async_trait::async_trait]
pub trait AndroidAutoNavigationTrait: AndroidAutoMainTrait {
    /// A turn indication update
    async fn turn_indication(&self, m: Wifi::NavigationTurnEvent);
    /// A distance indication update
    async fn distance_indication(&self, m: Wifi::NavigationDistanceEvent);
    /// A status update
    async fn nagivation_status(&self, m: Wifi::NavigationStatus);
}

/// This trait is implemented by users wishing to display a video stream from an android auto (phone probably).
#[async_trait::async_trait]
pub trait AndroidAutoVideoChannelTrait: AndroidAutoMainTrait {
    /// Parse a chunk of h264 video data
    async fn receive_video(&self, data: Vec<u8>, timestamp: Option<u64>);
    /// Setup the video device to receive h264 video, if anything is required. Return Ok(()) if setup was good, Err(()) if it was not good
    async fn setup_video(&self) -> Result<(), ()>;
    /// Tear down the video receiver, may be called without the setup having been called
    async fn teardown_video(&self);
    /// Wait for the video to be in focus
    async fn wait_for_focus(&self);
    /// Set the focus of the video stream to be as requested
    async fn set_focus(&self, focus: bool);
    /// Retrieve the video configuration for the channel
    fn retrieve_video_configuration(&self) -> &VideoConfiguration;
}

/// The types of audio channels that can exist
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum AudioChannelType {
    /// Media audio
    Media,
    /// System audio
    System,
    /// Speech audio
    Speech,
}

/// This trait is implemented by users that have audio output capabilities
#[async_trait::async_trait]
pub trait AndroidAutoAudioOutputTrait: AndroidAutoMainTrait {
    /// Opens the specified channel
    async fn open_channel(&self, t: AudioChannelType) -> Result<(), ()>;
    /// Closes the specified channel
    async fn close_channel(&self, t: AudioChannelType) -> Result<(), ()>;
    /// Receive a chunk of audio data for the specified channel
    async fn receive_audio(&self, t: AudioChannelType, data: Vec<u8>);
    /// The specified audio channel will start
    async fn start_audio(&self, t: AudioChannelType);
    /// The specified audio channel will stop
    async fn stop_audio(&self, t: AudioChannelType);
}

/// This trait is implemented by users that have audio input capabilities
#[async_trait::async_trait]
pub trait AndroidAutoAudioInputTrait: AndroidAutoMainTrait {
    /// Opens the channel
    async fn open_channel(&self) -> Result<(), ()>;
    /// Closes the channel
    async fn close_channel(&self) -> Result<(), ()>;
    /// The audio channel will start
    async fn start_audio(&self);
    /// The audio channel will stop
    async fn stop_audio(&self);
}

/// The configuration for an input channel
#[derive(Clone)]
pub struct InputConfiguration {
    /// The supported keycodes
    pub keycodes: Vec<u32>,
    /// The touchscreen width and height
    pub touchscreen: Option<(u16, u16)>,
}

/// This trait is implemented by users that have inputs for their head unit
#[async_trait::async_trait]
pub trait AndroidAutoInputChannelTrait: AndroidAutoMainTrait {
    /// A binding request for the specified keycode, generally the same code reported in `AndroidAutoConfig::keycodes_supported`
    async fn binding_request(&self, code: u32) -> Result<(), ()>;
    /// Retrieve the input configuration
    fn retrieve_input_configuration(&self) -> &InputConfiguration;
}

/// A trait that is implemented for users that somehow support bluetooth for their hardware
#[async_trait::async_trait]
pub trait AndroidAutoBluetoothTrait: AndroidAutoMainTrait {
    /// Do something
    async fn do_stuff(&self);
    /// Get the configuration
    fn get_config(&self) -> &BluetoothInformation;
}

/// This is the bluetooth server for initiating wireless android auto on compatible devices.
pub struct AndroidAutoServer {}

#[allow(missing_docs)]
#[allow(clippy::missing_docs_in_private_items)]
mod protobufmod {
    include!(concat!(env!("OUT_DIR"), "/protobuf/mod.rs"));
}
pub use protobufmod::*;

/// The android auto version supported
const VERSION: (u16, u16) = (1, 1);

/// The types of messages that can be sent over the android auto link
pub enum AndroidAutoMessage {
    /// An input message
    Input(Wifi::InputEventIndication),
    /// An audio packet message, optional timestamp (microseconds since UNIX_EPOCH) and data
    Audio(Option<u64>, Vec<u8>),
    /// A sensor event message
    Sensor(Wifi::SensorEventIndication),
    /// An other message
    Other,
}

/// The type of channel being sent in a sendable message
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum SendableChannelType {
    /// The input channel
    Input,
    /// The audio input channel
    AudioInput,
    /// The sensor channel
    Sensor,
    /// Other channel type
    Other,
}

/// The sendable form of an `AndroidAutoMessage`
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SendableAndroidAutoMessage {
    /// The channel id to send the message to
    channel: SendableChannelType,
    /// The message body to send
    data: Vec<u8>,
}

impl SendableAndroidAutoMessage {
    /// Convert Self into an `AndroidAutoFrame``
    async fn into_frame(self) -> AndroidAutoFrame {
        let mut chan = None;
        let chans = CHANNEL_HANDLERS.read().await;
        for (i, c) in chans.iter().enumerate() {
            match self.channel {
                SendableChannelType::Sensor => {
                    if let ChannelHandler::Sensor(_) = c {
                        chan = Some(i as u8);
                        break;
                    }
                }
                SendableChannelType::AudioInput => {
                    if let ChannelHandler::AvInput(_) = c {
                        chan = Some(i as u8);
                        break;
                    }
                }
                SendableChannelType::Input => {
                    if let ChannelHandler::Input(_) = c {
                        chan = Some(i as u8);
                        break;
                    }
                }
                SendableChannelType::Other => {
                    todo!();
                }
            }
        }
        AndroidAutoFrame {
            header: FrameHeader {
                channel_id: chan.unwrap(),
                frame: FrameHeaderContents::new(true, FrameHeaderType::Single, false),
            },
            data: self.data,
        }
    }
}

/// A message sent from an app user to this crate
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum AndroidAutoChannelMessageFromApp {
    /// A message that needs to be forwarded to the android auto device
    MessageToPhone(SendableAndroidAutoMessage),
}

impl AndroidAutoMessage {
    /// Convert the message to something that can be sent, if possible
    pub fn sendable(self) -> SendableAndroidAutoMessage {
        match self {
            Self::Sensor(m) => {
                let mut data = m.write_to_bytes().unwrap();
                let t = Wifi::sensor_channel_message::Enum::SENSOR_EVENT_INDICATION as u16;
                let t = t.to_be_bytes();
                let mut m = Vec::new();
                m.push(t[0]);
                m.push(t[1]);
                m.append(&mut data);
                SendableAndroidAutoMessage {
                    channel: SendableChannelType::Sensor,
                    data: m,
                }
            }
            Self::Input(m) => {
                let mut data = m.write_to_bytes().unwrap();
                let t = Wifi::input_channel_message::Enum::INPUT_EVENT_INDICATION as u16;
                let t = t.to_be_bytes();
                let mut m = Vec::new();
                m.push(t[0]);
                m.push(t[1]);
                m.append(&mut data);
                SendableAndroidAutoMessage {
                    channel: SendableChannelType::Input,
                    data: m,
                }
            }
            Self::Audio(timestamp, mut data) => {
                let t = Wifi::avchannel_message::Enum::AV_MEDIA_WITH_TIMESTAMP_INDICATION as u16;
                let t = t.to_be_bytes();
                let mut m = Vec::new();
                m.push(t[0]);
                m.push(t[1]);
                m.append(&mut data);
                SendableAndroidAutoMessage {
                    channel: SendableChannelType::AudioInput,
                    data: m,
                }
            }
            Self::Other => todo!(),
        }
    }
}

/// A message sent or received in the android auto protocol
#[cfg(feature = "wireless")]
struct AndroidAutoRawBluetoothMessage {
    /// The message type
    t: u16,
    /// The message contained in the message
    message: Vec<u8>,
}

/// The sensor information supported by the user for android auto
#[derive(Clone)]
pub struct SensorInformation {
    /// The sensor types supported
    pub sensors: HashSet<Wifi::sensor_type::Enum>,
}

/// The wireless network information to relay to the compatible android auto device
#[derive(Clone, Debug)]
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

/// The configuration data for the video stream of android auto
#[derive(Clone)]
pub struct VideoConfiguration {
    /// Defines the desired resolution for the video stream
    pub resolution: Wifi::video_resolution::Enum,
    /// The fps for the video stream
    pub fps: Wifi::video_fps::Enum,
    /// The dots per inch of the display
    pub dpi: u16,
}

/// Provides basic configuration elements for setting up an android auto head unit
#[derive(Clone)]
pub struct AndroidAutoConfiguration {
    /// The head unit information
    pub unit: HeadUnitInfo,
    /// The android auto client certificate and private key in pem format (only if a custom one is desired)
    pub custom_certificate: Option<(Vec<u8>, Vec<u8>)>,
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
    pub async fn read<T: AsyncRead + Unpin>(
        &mut self,
        stream: &mut T,
    ) -> Result<Option<FrameHeader>, FrameReceiptError> {
        if self.channel_id.is_none() {
            let mut b = [0u8];
            stream
                .read_exact(&mut b)
                .await
                .map_err(|e| match e.kind() {
                    std::io::ErrorKind::TimedOut => FrameReceiptError::TimeoutHeader,
                    std::io::ErrorKind::UnexpectedEof => FrameReceiptError::Disconnected,
                    _ => FrameReceiptError::UnexpectedDuringFrameChannel(e),
                })?;
            self.channel_id = ChannelId::try_from(b[0]).ok();
        }
        if let Some(channel_id) = &self.channel_id {
            let mut b = [0u8];
            stream
                .read_exact(&mut b)
                .await
                .map_err(|e| match e.kind() {
                    std::io::ErrorKind::TimedOut => FrameReceiptError::TimeoutHeader,
                    std::io::ErrorKind::UnexpectedEof => FrameReceiptError::Disconnected,
                    _ => FrameReceiptError::UnexpectedDuringFrameHeader(e),
                })?;
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
    async fn read<T: tokio::io::AsyncRead + Unpin>(
        &mut self,
        header: &FrameHeader,
        stream: &mut T,
        ssl_stream: &mut rustls::client::ClientConnection,
    ) -> Result<Option<AndroidAutoFrame>, FrameReceiptError> {
        if self.len.is_none() {
            if header.frame.get_frame_type() == FrameHeaderType::First {
                let mut p = [0u8; 6];
                stream
                    .read_exact(&mut p)
                    .await
                    .map_err(|e| match e.kind() {
                        std::io::ErrorKind::TimedOut => FrameReceiptError::TimeoutHeader,
                        std::io::ErrorKind::UnexpectedEof => FrameReceiptError::Disconnected,
                        _ => FrameReceiptError::UnexpectedDuringFrameLength(e),
                    })?;
                let len = u16::from_be_bytes([p[0], p[1]]);
                self.len.replace(len);
            } else {
                let mut p = [0u8; 2];
                stream
                    .read_exact(&mut p)
                    .await
                    .map_err(|e| match e.kind() {
                        std::io::ErrorKind::TimedOut => FrameReceiptError::TimeoutHeader,
                        std::io::ErrorKind::UnexpectedEof => FrameReceiptError::Disconnected,
                        _ => FrameReceiptError::UnexpectedDuringFrameLength(e),
                    })?;
                let len = u16::from_be_bytes(p);
                self.len.replace(len);
            }
        }

        let decrypt = |ssl_stream: &mut rustls::client::ClientConnection,
                       _len: u16,
                       data_frame: Vec<u8>|
         -> Result<Vec<u8>, FrameReceiptError> {
            let mut plain_data = vec![0u8; data_frame.len()];
            let mut cursor = Cursor::new(&data_frame);
            let mut index = 0;
            loop {
                let asdf = ssl_stream
                    .read_tls(&mut cursor)
                    .map_err(|e| FrameReceiptError::TlsReadError(e))?;
                let _ = ssl_stream
                    .process_new_packets()
                    .map_err(|e| FrameReceiptError::TlsProcessingError(e))?;
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
            stream
                .read_exact(&mut data_frame)
                .await
                .map_err(|e| match e.kind() {
                    std::io::ErrorKind::TimedOut => FrameReceiptError::TimeoutHeader,
                    std::io::ErrorKind::UnexpectedEof => FrameReceiptError::Disconnected,
                    _ => FrameReceiptError::UnexpectedDuringFrameContents(e),
                })?;
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
    fn as_message(&self) -> AndroidAutoRawBluetoothMessage {
        use protobuf::Message;
        match self {
            AndroidAutoBluetoothMessage::SocketInfoRequest(m) => AndroidAutoRawBluetoothMessage {
                t: Bluetooth::MessageId::BLUETOOTH_SOCKET_INFO_REQUEST as u16,
                message: m.write_to_bytes().unwrap(),
            },
            AndroidAutoBluetoothMessage::NetworkInfoMessage(m) => AndroidAutoRawBluetoothMessage {
                t: Bluetooth::MessageId::BLUETOOTH_NETWORK_INFO_MESSAGE as u16,
                message: m.write_to_bytes().unwrap(),
            },
        }
    }
}

impl From<AndroidAutoRawBluetoothMessage> for Vec<u8> {
    fn from(value: AndroidAutoRawBluetoothMessage) -> Self {
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
    async fn receive_data<
        T: AndroidAutoMainTrait + ?Sized,
        U: tokio::io::AsyncRead + Unpin,
        V: tokio::io::AsyncWrite + Unpin,
    >(
        &self,
        msg: AndroidAutoFrame,
        stream: &StreamMux<U, V>,
        _config: &AndroidAutoConfiguration,
        _main: &T,
    ) -> Result<(), FrameIoError>;

    /// Construct the channeldescriptor with the channel handler so it can be conveyed to the compatible android auto device
    fn build_channel<T: AndroidAutoMainTrait + ?Sized>(
        &self,
        config: &AndroidAutoConfiguration,
        chanid: ChannelId,
        main: &T,
    ) -> Option<ChannelDescriptor>;

    /// Set the list of all channels for the current channel. Only used for the control channel. This is because the control channel must be created first.
    fn set_channels(&self, _chans: Vec<ChannelDescriptor>) {}
}

/// A message sent for an av channel
enum AvChannelMessage {
    /// A message to start setup of the av channel
    SetupRequest(ChannelId, Wifi::AVChannelSetupRequest),
    /// A message that responds to a setup request
    SetupResponse(ChannelId, Wifi::AVChannelSetupResponse),
    /// Message requesting the focus of the video channel to be set
    VideoFocusRequest(ChannelId, Wifi::VideoFocusRequest),
    /// Message requesting to open the channel
    AvChannelOpen(ChannelId, Wifi::AVInputOpenRequest),
    /// Message indication the focus status of the video stream on the head unit
    VideoIndicationResponse(ChannelId, Wifi::VideoFocusIndication),
    /// The stream is about to start
    StartIndication(ChannelId, Wifi::AVChannelStartIndication),
    /// The stream is about to stop
    StopIndication(ChannelId, Wifi::AVChannelStopIndication),
    /// A media indication message, optionally containing a timestamp
    MediaIndication(ChannelId, Option<u64>, Vec<u8>),
    /// An acknowledgement of receiving a media indication message
    MediaIndicationAck(ChannelId, Wifi::AVMediaAckIndication),
}

impl From<AvChannelMessage> for AndroidAutoFrame {
    fn from(value: AvChannelMessage) -> Self {
        match value {
            AvChannelMessage::AvChannelOpen(_, _) => unimplemented!(),
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
            AvChannelMessage::MediaIndication(chan, timestamp, mut data) => {
                let (t, mut data) = if let Some(ts) = timestamp {
                    let mut m = Vec::new();
                    let mut tsb = ts.to_be_bytes().to_vec();
                    m.append(&mut tsb);
                    m.append(&mut data);
                    (
                        Wifi::avchannel_message::Enum::AV_MEDIA_WITH_TIMESTAMP_INDICATION as u16,
                        m,
                    )
                } else {
                    let mut m = Vec::new();
                    m.append(&mut data);
                    (Wifi::avchannel_message::Enum::AV_MEDIA_INDICATION as u16, m)
                };
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
            AvChannelMessage::StopIndication(_, _) => unimplemented!(),
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
                        Err(e) => Err(format!("Invalid channel setup request: {}", e)),
                    }
                }
                Wifi::avchannel_message::Enum::START_INDICATION => {
                    let m = Wifi::AVChannelStartIndication::parse_from_bytes(&value.data[2..]);
                    match m {
                        Ok(m) => Ok(Self::StartIndication(value.header.channel_id, m)),
                        Err(e) => Err(format!("Invalid channel start request: {}", e)),
                    }
                }
                Wifi::avchannel_message::Enum::STOP_INDICATION => {
                    let m = Wifi::AVChannelStopIndication::parse_from_bytes(&value.data[2..]);
                    match m {
                        Ok(m) => Ok(Self::StopIndication(value.header.channel_id, m)),
                        Err(e) => Err(format!("Invalid channel stop request: {}", e)),
                    }
                }
                Wifi::avchannel_message::Enum::SETUP_RESPONSE => unimplemented!(),
                Wifi::avchannel_message::Enum::AV_MEDIA_ACK_INDICATION => todo!(),
                Wifi::avchannel_message::Enum::AV_INPUT_OPEN_REQUEST => {
                    let m = Wifi::AVInputOpenRequest::parse_from_bytes(&value.data[2..]);
                    match m {
                        Ok(m) => Ok(Self::AvChannelOpen(value.header.channel_id, m)),
                        Err(e) => Err(format!("Invalid request: {}", e)),
                    }
                }
                Wifi::avchannel_message::Enum::AV_INPUT_OPEN_RESPONSE => todo!(),
                Wifi::avchannel_message::Enum::VIDEO_FOCUS_REQUEST => {
                    let m = Wifi::VideoFocusRequest::parse_from_bytes(&value.data[2..]);
                    match m {
                        Ok(m) => Ok(Self::VideoFocusRequest(value.header.channel_id, m)),
                        Err(e) => Err(format!("Invalid request: {}", e)),
                    }
                }
                Wifi::avchannel_message::Enum::VIDEO_FOCUS_INDICATION => unimplemented!(),
            }
        } else {
            Err(format!("Not converted message: {:x?}", value.data))
        }
    }
}

/// An object that allows multiple tasks to send or receive frames
struct StreamMux<T: AsyncRead + Unpin, U: AsyncWrite + Unpin> {
    /// The reader for receiving frames from the android auto device
    reader: Arc<tokio::sync::Mutex<T>>,
    /// The writer for sending frames to the android auto device
    writer: Arc<tokio::sync::Mutex<U>>,
    /// The object used for tls communication
    ssl_client: Arc<tokio::sync::Mutex<rustls::client::ClientConnection>>,
}

impl<T: AsyncRead + Unpin, U: AsyncWrite + Unpin> Clone for StreamMux<T, U> {
    fn clone(&self) -> Self {
        Self {
            reader: self.reader.clone(),
            writer: self.writer.clone(),
            ssl_client: self.ssl_client.clone(),
        }
    }
}

impl<T: AsyncRead + Unpin, U: AsyncWrite + Unpin> StreamMux<T, U> {
    /// Construct a new self
    pub fn new(sr: T, ss: U, ssl_client: rustls::client::ClientConnection) -> Self {
        Self {
            reader: Arc::new(tokio::sync::Mutex::new(sr)),
            writer: Arc::new(tokio::sync::Mutex::new(ss)),
            ssl_client: Arc::new(tokio::sync::Mutex::new(ssl_client)),
        }
    }

    /// Is the stream currently in handshake process?
    pub async fn is_handshaking(&self) -> bool {
        let ssl_stream = self.ssl_client.lock().await;
        ssl_stream.is_handshaking()
    }

    /// Start the ssl handshake process
    pub async fn start_handshake(&self) -> Result<(), SslHandshakeError> {
        let mut w = self.writer.lock().await;
        let mut ssl_stream = self.ssl_client.lock().await;
        let mut s = Vec::new();
        if ssl_stream.wants_write() {
            let l = ssl_stream.write_tls(&mut s);
            if l.is_ok() {
                let f: AndroidAutoFrame = AndroidAutoControlMessage::SslHandshake(s).into();
                let d2: Vec<u8> = f.build_vec(Some(&mut *ssl_stream)).await;
                w.write_all(&d2).await.map_err(|e| match e.kind() {
                    std::io::ErrorKind::TimedOut => SslHandshakeError::Timeout,
                    std::io::ErrorKind::UnexpectedEof => SslHandshakeError::Disconnected,
                    _ => SslHandshakeError::Unexpected(e),
                })?;
            }
        }
        Ok(())
    }

    /// Continue the handshake process
    pub async fn do_handshake(&self, data: Vec<u8>) -> Result<(), SslHandshakeError> {
        let mut w = self.writer.lock().await;
        let mut ssl_stream = self.ssl_client.lock().await;
        if ssl_stream.wants_read() {
            let mut dc = std::io::Cursor::new(data);
            let _ = ssl_stream.read_tls(&mut dc);
            let _ = ssl_stream.process_new_packets();
        }
        if ssl_stream.wants_write() {
            let mut s = Vec::new();
            let l = ssl_stream.write_tls(&mut s);
            if l.is_ok() {
                let f: AndroidAutoFrame = AndroidAutoControlMessage::SslHandshake(s).into();
                let d2: Vec<u8> = f.build_vec(Some(&mut *ssl_stream)).await;
                w.write_all(&d2).await.map_err(|e| match e.kind() {
                    std::io::ErrorKind::TimedOut => SslHandshakeError::Timeout,
                    std::io::ErrorKind::UnexpectedEof => SslHandshakeError::Disconnected,
                    _ => SslHandshakeError::Unexpected(e),
                })?;
            }
        }
        Ok(())
    }

    /// Read a frame from the stream
    pub async fn read_frame(
        &self,
        fr2: &mut AndroidAutoFrameReceiver,
    ) -> Result<AndroidAutoFrame, FrameReceiptError> {
        let mut s = self.reader.lock().await;
        loop {
            let mut fr = FrameHeaderReceiver::new();
            let f = fr.read(&mut *s).await?;
            let f2 = if let Some(f) = f {
                let mut ssl_stream = self.ssl_client.lock().await;
                fr2.read(&f, &mut *s, &mut ssl_stream).await?
            } else {
                None
            };
            if let Some(f) = f2 {
                return Ok(f);
            }
        }
    }

    /// Write a frame to the stream, encrypting if necessary
    pub async fn write_frame(&self, f: AndroidAutoFrame) -> Result<(), FrameTransmissionError> {
        let mut s = self.writer.lock().await;
        let mut ssl_stream = self.ssl_client.lock().await;
        let d2: Vec<u8> = f.build_vec(Some(&mut *ssl_stream)).await;
        s.write_all(&d2).await.map_err(|e| match e.kind() {
            std::io::ErrorKind::TimedOut => FrameTransmissionError::Timeout,
            std::io::ErrorKind::UnexpectedEof => FrameTransmissionError::Disconnected,
            _ => FrameTransmissionError::Unexpected(e),
        })
    }

    /// Write a frame to the stream, encrypting if necessary
    pub async fn write_sendable(
        &self,
        f: SendableAndroidAutoMessage,
    ) -> Result<(), FrameTransmissionError> {
        let mut s = self.writer.lock().await;
        let mut ssl_stream = self.ssl_client.lock().await;
        let d2: Vec<u8> = f.into_frame().await.build_vec(Some(&mut *ssl_stream)).await;
        s.write_all(&d2).await.map_err(|e| match e.kind() {
            std::io::ErrorKind::TimedOut => FrameTransmissionError::Timeout,
            std::io::ErrorKind::UnexpectedEof => FrameTransmissionError::Disconnected,
            _ => FrameTransmissionError::Unexpected(e),
        })
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

/// This is a wrapper around a join handle, it aborts the handle when it is dropped.
struct DroppingJoinHandle<T> {
    /// The handle for the struct
    handle: tokio::task::JoinHandle<T>,
}

impl<T> Drop for DroppingJoinHandle<T> {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// The handler function for a single bluetooth connection
async fn handle_bluetooth_client(
    stream: &mut BluetoothStream,
    network2: &NetworkInformation,
) -> Result<(), String> {
    let mut s = Bluetooth::SocketInfoRequest::new();
    s.set_ip_address(network2.ip.clone());
    s.set_port(network2.port as u32);
    log::info!("Got a bluetooth client");
    let m1 = AndroidAutoBluetoothMessage::SocketInfoRequest(s);
    let m: AndroidAutoRawBluetoothMessage = m1.as_message();
    let mdata: Vec<u8> = m.into();
    stream.write_all(&mdata).await.map_err(|e| e.to_string())?;
    loop {
        let mut ty = [0u8; 2];
        let mut len = [0u8; 2];
        stream
            .read_exact(&mut len)
            .await
            .map_err(|e| e.to_string())?;
        stream
            .read_exact(&mut ty)
            .await
            .map_err(|e| e.to_string())?;
        let len = u16::from_be_bytes(len);
        let ty = u16::from_be_bytes(ty);
        let mut message = vec![0; len as usize];
        stream
            .read_exact(&mut message)
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
                    log::debug!("Network info for bluetooth response: {:?}", network2);
                    response.set_ssid(network2.ssid.clone());
                    response.set_psk(network2.psk.clone());
                    response.set_mac_addr(network2.mac_addr.clone());
                    response.set_security_mode(network2.security_mode);
                    response.set_ap_type(network2.ap_type);
                    let response = AndroidAutoBluetoothMessage::NetworkInfoMessage(response);
                    let m: AndroidAutoRawBluetoothMessage = response.as_message();
                    let mdata: Vec<u8> = m.into();
                    let _ = stream.write_all(&mdata).await;
                }
                Bluetooth::MessageId::BLUETOOTH_SOCKET_INFO_RESPONSE => {
                    let message = Bluetooth::SocketInfoResponse::parse_from_bytes(&message);
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
    Ok(())
}

/// Runs the bluetooth service that allows wireless android auto connections to start up
async fn bluetooth_service(
    mut profile: bluetooth_rust::BluetoothRfcommProfile,
    wireless: Arc<dyn AndroidAutoWirelessTrait>,
) -> Result<(), String> {
    log::info!("Starting bluetooth service");
    loop {
        if let Ok(c) = profile.connectable().await {
            let network2 = wireless.get_wifi_details();
            let mut stream = c.accept().await?;
            let e = handle_bluetooth_client(&mut stream, &network2).await;
            log::info!("Bluetooth client disconnected: {:?}", e);
        }
    }
    Ok(())
}

/// Runs the wifi service for android auto
async fn wifi_service<T: AndroidAutoWirelessTrait + Send + ?Sized>(
    config: AndroidAutoConfiguration,
    wireless: Arc<T>,
) -> Result<(), String> {
    let network = wireless.get_wifi_details();

    log::info!("Starting android auto wireless service on port {}", network.port);
    if let Ok(a) = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", network.port)).await {
        loop {
            if let Ok((stream, addr)) = a.accept().await {
                let config2 = config.clone();
                let _ = stream.set_nodelay(true);
                wireless.connect().await;
                if let Err(e) = handle_client(stream, addr, config2, wireless.as_ref()).await {
                    wireless.disconnect().await;
                    if false {
                        let mut ch = CHANNEL_HANDLERS.write().await;
                        ch.clear();
                    }
                    log::error!("Disconnect from client: {:?}", e);
                }
            }
        }
        Ok(())
    } else {
        Err(format!("Failed to listen on port {} tcp", network.port))
    }
}

/// Handle a single android auto device for a head unit
async fn handle_client<T: AndroidAutoMainTrait + ?Sized>(
    stream: tokio::net::TcpStream,
    addr: std::net::SocketAddr,
    config: AndroidAutoConfiguration,
    main: &T,
) -> Result<(), ClientError> {
    log::info!("Got wifi client: {:?}", addr);
    let mut root_store =
        rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let aautocertder = {
        let mut br = std::io::Cursor::new(cert::AAUTO_CERT.to_string().as_bytes().to_vec());
        let aautocertpem = rustls::pki_types::pem::from_buf(&mut br)
            .map_err(|_| ClientError::InvalidRootCert)?
            .ok_or(ClientError::InvalidRootCert)?;
        CertificateDer::from_pem(aautocertpem.0, aautocertpem.1)
            .ok_or(ClientError::InvalidRootCert)?
    };

    let client_cert_data_pem = if let Some(custom) = &config.custom_certificate {
        custom
    } else {
        &(
            cert::CERTIFICATE.to_string().as_bytes().to_vec(),
            cert::PRIVATE_KEY.to_string().as_bytes().to_vec(),
        )
    };

    let cert = {
        let mut br = std::io::Cursor::new(&client_cert_data_pem.0);
        let aautocertpem = rustls::pki_types::pem::from_buf(&mut br)
            .map_err(|_| ClientError::InvalidClientCertificate)?
            .ok_or(ClientError::InvalidClientCertificate)?;
        CertificateDer::from_pem(aautocertpem.0, aautocertpem.1)
            .ok_or(ClientError::InvalidClientCertificate)?
    };
    let key = {
        let mut br = std::io::Cursor::new(&client_cert_data_pem.1);
        let aautocertpem = rustls::pki_types::pem::from_buf(&mut br)
            .map_err(|_| ClientError::InvalidClientPrivateKey)?
            .ok_or(ClientError::InvalidClientPrivateKey)?;
        rustls::pki_types::PrivateKeyDer::from_pem(aautocertpem.0, aautocertpem.1)
            .ok_or(ClientError::InvalidClientPrivateKey)?
    };
    let cert = vec![cert];
    root_store
        .add(aautocertder)
        .map_err(|_| ClientError::InvalidRootCert)?;
    let root_store = Arc::new(root_store);
    let mut ssl_client_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store.clone())
        .with_client_auth_cert(cert, key)
        .unwrap();
    let sver = Arc::new(AndroidAutoServerVerifier::new(root_store));
    ssl_client_config.dangerous().set_certificate_verifier(sver);
    let sslconfig = Arc::new(ssl_client_config);
    let server = "idontknow.com".try_into().unwrap();
    let ssl_client =
        rustls::ClientConnection::new(sslconfig, server).expect("Failed to build ssl client");

    let stream = stream.into_split();
    let sm = StreamMux::new(stream.0, stream.1, ssl_client);
    let message_recv = main.get_receiver().await;
    let sm2 = sm.clone();
    let _task2 = if let Some(mut msgr) = message_recv {
        let jh: tokio::task::JoinHandle<Result<(), FrameTransmissionError>> =
            tokio::task::spawn(async move {
                while let Some(m) = msgr.recv().await {
                    sm2.write_sendable(m)
                        .await
                        .inspect_err(|e| log::error!("Error passing message: {:?}", e))?;
                }
                Ok(())
            });
        Some(DroppingJoinHandle { handle: jh })
    } else {
        None
    };

    {
        let mut channel_handlers: Vec<ChannelHandler> = Vec::new();
        channel_handlers.push(ControlChannelHandler::new().into());
        if main.supports_input().is_some() {
            channel_handlers.push(InputChannelHandler {}.into());
        }
        if main.supports_sensors().is_some() {
            channel_handlers.push(SensorChannelHandler {}.into());
        }
        if main.supports_video().is_some() {
            channel_handlers.push(VideoChannelHandler::new().into());
        }
        if main.supports_audio_output().is_some() {
            channel_handlers.push(MediaAudioChannelHandler {}.into());
            channel_handlers.push(SpeechAudioChannelHandler {}.into());
            channel_handlers.push(SystemAudioChannelHandler {}.into());
        }
        if main.supports_audio_input().is_some() {
            channel_handlers.push(AvInputChannelHandler {}.into());
        }
        if main.supports_bluetooth().is_some() {
            channel_handlers.push(BluetoothChannelHandler {}.into());
        }
        if main.supports_navigation().is_some() {
            channel_handlers.push(NavigationChannelHandler {}.into());
        }
        channel_handlers.push(MediaStatusChannelHandler {}.into());

        let mut chans = Vec::new();
        for (index, handler) in channel_handlers.iter().enumerate() {
            let chan: ChannelId = index as u8;
            if let Some(chan) = handler.build_channel(&config, chan, main) {
                chans.push(chan);
            }
        }
        channel_handlers.get_mut(0).unwrap().set_channels(chans);
        {
            let mut ch = CHANNEL_HANDLERS.write().await;
            ch.clear();
            log::error!(
                "Adding {} channels to CHANNEL_HANDLERS",
                channel_handlers.len()
            );
            ch.append(&mut channel_handlers);
        }
    }
    log::debug!("Got a connection from {:?}", addr);
    sm.write_frame(AndroidAutoControlMessage::VersionRequest.into())
        .await
        .map_err(|e| {
            let e2: FrameIoError = e.into();
            e2
        })?;
    let mut fr2 = AndroidAutoFrameReceiver::new();
    let channel_handlers = CHANNEL_HANDLERS.read().await;
    log::debug!("Waiting on first packet from android auto client");
    loop {
        if let Ok(f) = sm.read_frame(&mut fr2).await {
            if let Some(handler) = channel_handlers.get(f.header.channel_id as usize) {
                handler.receive_data(f, &sm, &config, main).await?;
            } else {
                panic!("Unknown channel id: {:?}", f.header.channel_id);
            }
        }
    }
}

impl AndroidAutoServer {
    /// Runs the android auto server
    pub async fn run<T: AndroidAutoMainTrait + Send + 'static>(
        self,
        config: AndroidAutoConfiguration,
        js: &mut tokio::task::JoinSet<Result<(), String>>,
        main: T,
    ) -> Result<(), String> {
        log::info!("Running android auto server");
        if let Some(wireless) = main.supports_wireless() {
            let psettings = bluetooth_rust::BluetoothRfcommProfileSettings {
                uuid: bluetooth_rust::BluetoothUuid::AndroidAuto
                    .as_str()
                    .to_string(),
                name: Some("Android Auto Bluetooth Service".to_string()),
                service_uuid: Some(
                    bluetooth_rust::BluetoothUuid::AndroidAuto
                        .as_str()
                        .to_string(),
                ),
                channel: Some(22),
                psm: None,
                authenticate: Some(true),
                authorize: Some(true),
                auto_connect: Some(true),
                sdp_record: None,
                sdp_version: None,
                sdp_features: None,
            };

            let profile = wireless.setup_bluetooth_profile(&psettings).await?;
            log::info!("Setup bluetooth profile is ok?");
            let wireless2 = wireless.clone();
            js.spawn(async move {
                let e = bluetooth_service(profile, wireless2).await;
                log::error!("Android auto bluetooth service stopped: {:?}", e);
                e
            });
            js.spawn(async move {
                let e = wifi_service(config, wireless.clone()).await;
                log::error!("Android auto wireless service stopped: {:?}", e);
                e
            });
            Ok(())
        } else {
            Err("Wireless not supported when it is currently required".to_string())
        }
    }

    /// Construct a new self
    pub async fn new() -> Self {
        Self {}
    }
}

/// Perform any setup required on startup of the library
pub fn setup() {
    let cp = rustls::crypto::ring::default_provider();
    cp.install_default().expect("Failed to set ssl provider");
}
