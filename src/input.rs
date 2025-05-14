//! This is for the input channel handler code

use protobuf::Message;

use crate::{common::AndroidAutoCommonMessage, frame_header::FrameHeaderContents, AndroidAutoConfiguration, AndroidAutoFrame, AndroidAutoMainTrait, ChannelHandlerTrait, ChannelId, FrameHeader, FrameHeaderType, StreamMux, Wifi};

/// A message about binding input buttons on a compatible android auto head unit
#[derive(Debug)]
enum InputMessage {
    /// A message requesting input buttons to be bound
    BindingRequest(ChannelId, Wifi::BindingRequest),
    /// A message that responds to a binding request, indicating success or failure of the request
    BindingResponse(ChannelId, Wifi::BindingResponse),
    /// A message that conveys input data from the user
    InputEvent(ChannelId, Wifi::InputEventIndication),
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
            InputMessage::InputEvent(chan, m) => {
                let mut data = m.write_to_bytes().unwrap();
                let t = Wifi::input_channel_message::Enum::INPUT_EVENT_INDICATION as u16;
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
pub struct InputChannelHandler {}

impl ChannelHandlerTrait for InputChannelHandler {
    fn build_channel(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<Wifi::ChannelDescriptor> {
        let mut chan = Wifi::ChannelDescriptor::new();
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
    ) -> Result<(), super::FrameIoError> {
        let channel = msg.header.channel_id;
        let msg2: Result<InputMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                InputMessage::BindingRequest(chan, _m) => {
                    let mut m2 = Wifi::BindingResponse::new();
                    m2.set_status(Wifi::status::Enum::OK);
                    stream
                        .write_frame(InputMessage::BindingResponse(chan, m2).into())
                        .await?;
                }
                InputMessage::BindingResponse(_, _) => unimplemented!(),
                InputMessage::InputEvent(_, _) => unimplemented!(),
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
                    stream
                        .write_frame(
                            AndroidAutoCommonMessage::ChannelOpenResponse(channel, m2).into(),
                        )
                        .await?;
                }
            }
            return Ok(());
        }
        todo!();
    }
}