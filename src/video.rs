//! Contains code for the the video channel

use super::{
    AndroidAutoCommonMessage, AndroidAutoConfiguration, AndroidAutoFrame, AndroidAutoMainTrait,
    AvChannelMessage, ChannelHandlerTrait, ChannelId,
};
use crate::Wifi;
use protobuf::Message;
use tokio::io::AsyncWriteExt;

/// The handler for the video channel on android auto
pub struct VideoChannelHandler {}

impl ChannelHandlerTrait for VideoChannelHandler {
    fn build_channel(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
    ) -> Option<Wifi::ChannelDescriptor> {
        let mut chan = Wifi::ChannelDescriptor::new();
        let mut avchan = Wifi::AVChannel::new();
        chan.set_channel_id(chanid as u32);
        avchan.set_stream_type(Wifi::avstream_type::Enum::VIDEO);
        avchan.set_available_while_in_call(true);
        avchan.set_audio_type(Wifi::audio_type::Enum::SYSTEM);
        let mut vconfs = Vec::new();
        vconfs.push({
            let mut vc = Wifi::VideoConfig::new();
            vc.set_video_resolution(Wifi::video_resolution::Enum::_480p);
            vc.set_video_fps(Wifi::video_fps::Enum::_60);
            vc.set_dpi(111);
            vc.set_margin_height(0);
            vc.set_margin_width(0);
            if !vc.is_initialized() {
                panic!();
            }
            vc
        });
        for v in vconfs {
            avchan.video_configs.push(v);
        }

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
        let channel = msg.header.channel_id;
        let msg2: Result<AndroidAutoCommonMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AndroidAutoCommonMessage::ChannelOpenResponse(_, _) => unimplemented!(),
                AndroidAutoCommonMessage::ChannelOpenRequest(m) => {
                    log::info!("Got channel open request for video: {:?}", m);
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
                AvChannelMessage::MediaIndication(_chan, time, data) => {
                    log::error!("Got media with timestamp {:?}", time);
                    if let Some(a) = main.supports_video() {
                        a.receive_video(data).await;
                    }

                    let mut m2 = Wifi::AVMediaAckIndication::new();
                    m2.set_session(0);
                    m2.set_value(1);
                    let d: AndroidAutoFrame =
                        AvChannelMessage::MediaIndicationAck(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
                AvChannelMessage::SetupRequest(chan, m) => {
                    log::info!("Got channel setup request for channel {:?}: {:?}", chan, m);

                    let mut m2 = Wifi::VideoFocusIndication::new();
                    m2.set_focus_mode(Wifi::video_focus_mode::Enum::FOCUSED);
                    m2.set_unrequested(false);
                    let d: AndroidAutoFrame =
                        AvChannelMessage::VideoIndicationResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;

                    let mut m2 = Wifi::AVChannelSetupResponse::new();
                    m2.set_max_unacked(1);
                    m2.set_media_status(Wifi::avchannel_setup_status::Enum::OK);
                    m2.configs.push(0);
                    let d: AndroidAutoFrame = AvChannelMessage::SetupResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
                AvChannelMessage::SetupResponse(_chan, _m) => unimplemented!(),
                AvChannelMessage::VideoFocusRequest(_chan, m) => {
                    log::error!("Got video focus request {:?}", m);
                    let mut m2 = Wifi::VideoFocusIndication::new();
                    m2.set_focus_mode(Wifi::video_focus_mode::Enum::FOCUSED);
                    m2.set_unrequested(false);
                    let d: AndroidAutoFrame =
                        AvChannelMessage::VideoIndicationResponse(channel, m2).into();
                    let d2: Vec<u8> = d.build_vec(Some(ssl_stream)).await;
                    stream.write_all(&d2).await?;
                }
                AvChannelMessage::VideoIndicationResponse(_, _) => unimplemented!(),
                AvChannelMessage::StartIndication(chan, _) => {
                    log::error!("Got start indication for channel {:?}", chan);
                }
            }
            return Ok(());
        }
        todo!("{:x?}", msg);
    }
}
