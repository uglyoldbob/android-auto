//! Contains bluetooth channel code

use super::{
    AndroidAutoCommonMessage, AndroidAutoConfiguration, AndroidAutoControlMessage,
    AndroidAutoFrame, ChannelDescriptor, ChannelHandlerTrait, ChannelId, FrameHeader,
    FrameHeaderContents, FrameHeaderType,
};
use crate::{AndroidAutoMainTrait, StreamMux, Wifi};
use protobuf::{EnumOrUnknown, Message};

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
    fn build_channel<T: AndroidAutoMainTrait + ?Sized>(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
        main: &T,
    ) -> Option<Wifi::ChannelDescriptor> {
        main.supports_bluetooth().map(|bc| {
            let mut chan = ChannelDescriptor::new();
            chan.set_channel_id(chanid as u32);
            let mut bchan = Wifi::BluetoothChannel::new();
            let bluetooth_config = bc.get_config();
            bchan.set_adapter_address(bluetooth_config.address.clone());
            let meth = Wifi::bluetooth_pairing_method::Enum::HFP;
            bchan
                .supported_pairing_methods
                .push(EnumOrUnknown::new(meth));
            chan.bluetooth_channel.0.replace(Box::new(bchan));
            if !chan.is_initialized() {
                panic!("Channel not initialized?");
            }
            chan    
        })
    }

    async fn receive_data<
        T: super::AndroidAutoMainTrait + ?Sized,
        U: tokio::io::AsyncRead + Unpin,
        V: tokio::io::AsyncWrite + Unpin,
    >(
        &self,
        msg: AndroidAutoFrame,
        stream: &StreamMux<U, V>,
        _config: &AndroidAutoConfiguration,
        _main: &T,
    ) -> Result<(), super::FrameIoError> {
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
                    stream
                        .write_frame(BluetoothMessage::PairingResponse(channel, m2).into())
                        .await?;
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
                    stream
                        .write_frame(
                            AndroidAutoCommonMessage::ChannelOpenResponse(channel, m2).into(),
                        )
                        .await?;
                }
            }
            return Ok(());
        }
        todo!("{:02x?} {:?} {:?} ", msg, msg2, msg3);
    }
}
