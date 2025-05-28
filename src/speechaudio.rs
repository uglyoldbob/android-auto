//! This is for the speech audio channel handler code

use protobuf::Message;

use crate::{
    AndroidAutoConfiguration, AndroidAutoFrame, AndroidAutoMainTrait, AvChannelMessage,
    ChannelHandlerTrait, ChannelId, StreamMux, Wifi, common::AndroidAutoCommonMessage,
};

/// The handler for speech audio for the android auto protocol
pub struct SpeechAudioChannelHandler {}

impl ChannelHandlerTrait for SpeechAudioChannelHandler {
    fn build_channel<T: AndroidAutoMainTrait + ?Sized>(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
        _main: &T,
    ) -> Option<Wifi::ChannelDescriptor> {
        let mut chan = Wifi::ChannelDescriptor::new();
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
                    let mut status = false;
                    if let Some(a) = main.supports_audio_output() {
                        if a.open_channel(crate::AudioChannelType::Speech)
                            .await
                            .is_ok()
                        {
                            status = true;
                        }
                    }
                    m2.set_status(if status {
                        Wifi::status::Enum::OK
                    } else {
                        Wifi::status::Enum::FAIL
                    });
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
                AvChannelMessage::AvChannelOpen(_chan, _m) => todo!(),
                AvChannelMessage::MediaIndicationAck(_, _) => unimplemented!(),
                AvChannelMessage::MediaIndication(_chan, _timestamp, data) => {
                    if let Some(a) = main.supports_audio_output() {
                        a.receive_audio(crate::AudioChannelType::Speech, data).await
                    }
                }
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
                AvChannelMessage::VideoFocusRequest(_chan, _m) => {
                    let mut m2 = Wifi::VideoFocusIndication::new();
                    m2.set_focus_mode(Wifi::video_focus_mode::Enum::FOCUSED);
                    m2.set_unrequested(false);
                    stream
                        .write_frame(AvChannelMessage::VideoIndicationResponse(channel, m2).into())
                        .await?;
                }
                AvChannelMessage::VideoIndicationResponse(_, _) => unimplemented!(),
                AvChannelMessage::StartIndication(_, _) => {
                    if let Some(a) = main.supports_audio_output() {
                        a.start_audio(crate::AudioChannelType::Speech).await;
                    }
                }
                AvChannelMessage::StopIndication(_, _) => {
                    if let Some(a) = main.supports_audio_output() {
                        a.stop_audio(crate::AudioChannelType::Speech).await;
                    }
                }
            }
            return Ok(());
        }
        todo!("{:x?}", msg);
    }
}
