//! Contains bluetooth channel code

use super::{
    AndroidAutoCommonMessage, AndroidAutoConfiguration, AndroidAutoControlMessage,
    AndroidAutoFrame, AndroidAutoMainTrait, ChannelDescriptor, ChannelHandlerTrait, ChannelId,
    FrameHeader, FrameHeaderContents, FrameHeaderType,
};
use crate::Wifi;
use protobuf::{EnumOrUnknown, Message};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

/// A message about bluetooth operations
#[derive(Debug)]
pub enum BluetoothMessage {
    /// A request to pair with a specified bluetooth device
    PairingRequest(ChannelId, Wifi::BluetoothPairingRequest),
    /// A response to a pairing request
    PairingResponse(ChannelId, Wifi::BluetoothPairingResponse),
    /// An authentication message of some variety for the bluetooth channel?
    Auth,
}

impl From<BluetoothMessage> for AndroidAutoFrame {
    fn from(value: BluetoothMessage) -> Self {
        match value {
            BluetoothMessage::PairingRequest(_, _) => todo!(),
            BluetoothMessage::PairingResponse(chan, m) => {
                let mut data = m.write_to_bytes().unwrap();
                let t = Wifi::bluetooth_channel_message::Enum::PAIRING_RESPONSE as u16;
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
            BluetoothMessage::Auth => unimplemented!(),
        }
    }
}

impl TryFrom<&AndroidAutoFrame> for BluetoothMessage {
    type Error = String;
    fn try_from(value: &AndroidAutoFrame) -> Result<Self, Self::Error> {
        use protobuf::Enum;
        let mut ty = [0u8; 2];
        ty.copy_from_slice(&value.data[0..2]);
        let ty = u16::from_be_bytes(ty);
        if let Some(sys) = Wifi::bluetooth_channel_message::Enum::from_i32(ty as i32) {
            match sys {
                Wifi::bluetooth_channel_message::Enum::PAIRING_REQUEST => {
                    let m = Wifi::BluetoothPairingRequest::parse_from_bytes(&value.data[2..]);
                    match m {
                        Ok(m) => Ok(Self::PairingRequest(value.header.channel_id, m)),
                        Err(e) => Err(e.to_string()),
                    }
                }
                Wifi::bluetooth_channel_message::Enum::PAIRING_RESPONSE => unimplemented!(),
                Wifi::bluetooth_channel_message::Enum::AUTH_DATA => todo!(),
                Wifi::bluetooth_channel_message::Enum::NONE => unimplemented!(),
            }
        } else {
            Err(format!("Not converted message: {:x?}", value.data))
        }
    }
}

/// The handler for the bluetooth channel in the android auto protocol. This is different than the bluetooth channel used to initialize wireless android auto.
pub struct BluetoothChannelHandler {}

impl ChannelHandlerTrait for BluetoothChannelHandler {
    fn build_channel(
        &self,
        config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<ChannelDescriptor> {
        let mut chan = ChannelDescriptor::new();
        chan.set_channel_id(chanid as u32);
        let mut bchan = Wifi::BluetoothChannel::new();
        bchan.set_adapter_address(config.bluetooth.address.clone());
        let meth = Wifi::bluetooth_pairing_method::Enum::HFP;
        bchan
            .supported_pairing_methods
            .push(EnumOrUnknown::new(meth));
        chan.bluetooth_channel.0.replace(Box::new(bchan));
        if !chan.is_initialized() {
            panic!("Channel not initialized?");
        }
        Some(chan)
    }

    async fn receive_data<T: AndroidAutoMainTrait, U: AsyncRead + AsyncWrite + Unpin>(
        &mut self,
        msg: AndroidAutoFrame,
        stream: &super::StreamMux<U>,
        ssl_stream: &mut rustls::client::ClientConnection,
        _config: &AndroidAutoConfiguration,
        _main: &mut T,
    ) -> Result<(), std::io::Error> {
        let channel = msg.header.channel_id;
        let msg2: Result<BluetoothMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                BluetoothMessage::PairingResponse(_, _) => unimplemented!(),
                BluetoothMessage::Auth => unimplemented!(),
                BluetoothMessage::PairingRequest(_chan, _m) => {
                    let mut m2 = Wifi::BluetoothPairingResponse::new();
                    m2.set_already_paired(true);
                    m2.set_status(Wifi::bluetooth_pairing_status::Enum::OK);
                    let d: AndroidAutoFrame = BluetoothMessage::PairingResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_frame(&d2).await?;
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
                    stream.write_frame(&d2).await?;
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
        todo!("{:02x?} {:?} {:?} {:?}", msg, msg2, msg3, msg4);
    }
}
