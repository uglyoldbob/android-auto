//! This is for the navigation channel handler code

use protobuf::Message;

use crate::{
    AndroidAutoConfiguration, AndroidAutoFrame, AndroidAutoMainTrait, ChannelHandlerTrait,
    ChannelId, StreamMux, Wifi, common::AndroidAutoCommonMessage,
};

/// A message about binding input buttons on a compatible android auto head unit
#[derive(Debug)]
enum NavigationMessage {
    /// A message indicating navigation status
    Status(ChannelId, Wifi::NavigationStatus),
    /// A message that conveys turn information
    TurnIndication(ChannelId, Wifi::NavigationTurnEvent),
    /// A message that conveys distance for navigation
    DistanceIndication(ChannelId, Wifi::NavigationDistanceEvent),
}

impl From<NavigationMessage> for AndroidAutoFrame {
    fn from(value: NavigationMessage) -> Self {
        match value {
            NavigationMessage::Status(_, _) => unimplemented!(),
            NavigationMessage::DistanceIndication(_, _) => unimplemented!(),
            NavigationMessage::TurnIndication(_, _) => unimplemented!(),
        }
    }
}

impl TryFrom<&AndroidAutoFrame> for NavigationMessage {
    type Error = String;
    fn try_from(value: &AndroidAutoFrame) -> Result<Self, Self::Error> {
        use protobuf::Enum;
        let mut ty = [0u8; 2];
        ty.copy_from_slice(&value.data[0..2]);
        let ty = u16::from_be_bytes(ty);
        if let Some(sys) = Wifi::navigation_channel_message::Enum::from_i32(ty as i32) {
            match sys {
                Wifi::navigation_channel_message::Enum::STATUS => {
                    let m = Wifi::NavigationStatus::parse_from_bytes(&value.data[2..]);
                    match m {
                        Ok(m) => Ok(Self::Status(value.header.channel_id, m)),
                        Err(e) => Err(format!("Invalid frame: {}", e)),
                    }
                }
                Wifi::navigation_channel_message::Enum::NONE => unimplemented!(),
                Wifi::navigation_channel_message::Enum::TURN_EVENT => {
                    let m = Wifi::NavigationTurnEvent::parse_from_bytes(&value.data[2..]);
                    match m {
                        Ok(m) => Ok(Self::TurnIndication(value.header.channel_id, m)),
                        Err(e) => Err(format!("Invalid frame: {}", e)),
                    }
                }
                Wifi::navigation_channel_message::Enum::DISTANCE_EVENT => {
                    let m = Wifi::NavigationDistanceEvent::parse_from_bytes(&value.data[2..]);
                    match m {
                        Ok(m) => Ok(Self::DistanceIndication(value.header.channel_id, m)),
                        Err(e) => Err(format!("Invalid frame: {}", e)),
                    }
                }
            }
        } else {
            Err(format!("Not converted message: {:x?}", value.data))
        }
    }
}

/// The handler for navigation for the android auto protocol
pub struct NavigationChannelHandler {}

impl ChannelHandlerTrait for NavigationChannelHandler {
    fn build_channel<T: AndroidAutoMainTrait + ?Sized>(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
        _main: &T,
    ) -> Option<Wifi::ChannelDescriptor> {
        let mut chan = Wifi::ChannelDescriptor::new();
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

    async fn receive_data<
        T: AndroidAutoMainTrait + ?Sized,
        U: tokio::io::AsyncRead + Unpin,
        V: tokio::io::AsyncWrite + Unpin,
    >(
        &self,
        msg: AndroidAutoFrame,
        stream: &StreamMux<U, V>,
        _config: &AndroidAutoConfiguration,
        main: &T,
    ) -> Result<(), super::FrameIoError> {
        let channel = msg.header.channel_id;

        let msg1: Result<NavigationMessage, String> = (&msg).try_into();
        if let Ok(msg) = msg1 {
            match msg {
                NavigationMessage::Status(_, status) => {
                    if let Some(n) = main.supports_navigation() {
                        n.nagivation_status(status).await;
                    }
                }
                NavigationMessage::TurnIndication(_, turn) => {
                    if let Some(n) = main.supports_navigation() {
                        n.turn_indication(turn).await;
                    }
                }
                NavigationMessage::DistanceIndication(_, distance) => {
                    if let Some(n) = main.supports_navigation() {
                        n.distance_indication(distance).await;
                    }
                }
            }
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
        todo!("{:x?}", msg);
    }
}
