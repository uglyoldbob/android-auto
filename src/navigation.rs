//! This is for the navigation channel handler code

use protobuf::Message;

use crate::{common::AndroidAutoCommonMessage, AndroidAutoConfiguration, AndroidAutoFrame, AndroidAutoMainTrait, ChannelHandlerTrait, ChannelId, StreamMux, Wifi};

/// The handler for navigation for the android auto protocol
pub struct NavigationChannelHandler {}

impl ChannelHandlerTrait for NavigationChannelHandler {
    fn build_channel(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
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
        _main: &T,
    ) -> Result<(), super::FrameIoError> {
        let channel = msg.header.channel_id;
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