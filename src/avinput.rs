//! This is for the av input channel handler code

use protobuf::Message;

use crate::{
    common::AndroidAutoCommonMessage, AndroidAutoConfiguration, AndroidAutoFrame, AndroidAutoMainTrait, AvChannelMessage, ChannelHandlerTrait, ChannelId, FrameIoError, StreamMux, Wifi
};

/// Handles the av input channel of the android auto protocol
pub struct AvInputChannelHandler {}

impl ChannelHandlerTrait for AvInputChannelHandler {
    fn build_channel<T: AndroidAutoMainTrait + ?Sized>(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
        _main: &T,
    ) -> Option<Wifi::ChannelDescriptor> {
        let mut chan = Wifi::ChannelDescriptor::new();
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
        let msg2: Result<AvChannelMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                AvChannelMessage::AvChannelOpen(_chan, m) => {
                    if let Some(a) = main.supports_audio_input() {
                        if m.open() {
                            a.open_channel().await.map_err(|_| FrameIoError::AudioInputOpenError)?;
                        }
                        else {
                            a.close_channel().await.map_err(|_| FrameIoError::AudioInputCloseError)?;
                        }
                    }
                }
                AvChannelMessage::MediaIndicationAck(_, _) => {}
                AvChannelMessage::MediaIndication(_chan, _timestamp, _data) => unimplemented!(),
                AvChannelMessage::SetupRequest(_chan, _m) => {
                    let mut m2 = Wifi::AVChannelSetupResponse::new();
                    m2.set_max_unacked(10);
                    m2.set_media_status(Wifi::avchannel_setup_status::Enum::OK);
                    m2.configs.push(0);
                    stream
                        .write_frame(AvChannelMessage::SetupResponse(channel, m2).into())
                        .await?;
                }
                AvChannelMessage::SetupResponse(_chan, _m) => unimplemented!(),
                AvChannelMessage::VideoFocusRequest(_chan, _m) => unimplemented!(),
                AvChannelMessage::VideoIndicationResponse(_, _) => unimplemented!(),
                AvChannelMessage::StartIndication(_, _) => {
                    if let Some(a) = main.supports_audio_input() {
                        a.start_audio().await;
                    }
                }
                AvChannelMessage::StopIndication(_, _) => {
                    if let Some(a) = main.supports_audio_input() {
                        a.stop_audio().await;
                    }
                }
            }
            return Ok(());
        }
        todo!("{:x?}", msg);
    }
}
