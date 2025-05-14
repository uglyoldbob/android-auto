//! Contains code for the the video channel

use super::{
    AndroidAutoCommonMessage, AndroidAutoConfiguration, AndroidAutoFrame,
    AvChannelMessage, ChannelHandlerTrait, ChannelId,
};
use crate::{StreamMux, Wifi};
use protobuf::Message;

/// The inner protected data for a video stream
struct InnerChannelHandler {
    /// The active session for a video stream
    session: Option<i32>,
}

impl InnerChannelHandler {
    /// construct a new self
    pub fn new() -> Self {
        Self { session: None }
    }
}

/// The handler for the video channel on android auto
pub struct VideoChannelHandler {
    /// The protected contents of a video stream
    inner: std::sync::Mutex<InnerChannelHandler>,
}

impl VideoChannelHandler {
    /// construct a new self
    pub fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(InnerChannelHandler::new()),
        }
    }
}

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

    async fn receive_data<
        T: super::AndroidAutoMainTrait + ?Sized,
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
                AndroidAutoCommonMessage::ChannelOpenRequest(m) => {
                    log::info!("Got channel open request for video: {:?}", m);
                    let mut m2 = Wifi::ChannelOpenResponse::new();
                    if let Some(v) = main.supports_video() {
                        m2.set_status(if v.setup_video().await.is_ok() {
                            Wifi::status::Enum::OK
                        } else {
                            Wifi::status::Enum::FAIL
                        });
                    } else {
                        m2.set_status(Wifi::status::Enum::FAIL);
                    }
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
                AvChannelMessage::MediaIndicationAck(_, _) => unimplemented!(),
                AvChannelMessage::MediaIndication(_chan, time, data) => {
                    if let Some(a) = main.supports_video() {
                        a.receive_video(data, time).await;
                        let mut m2 = Wifi::AVMediaAckIndication::new();
                        {
                            let inner = self.inner.lock().unwrap();
                            m2.set_session(
                                inner
                                    .session
                                    .ok_or(super::FrameSequenceError::VideoChannelNotOpen)?,
                            );
                        }
                        m2.set_value(1);
                        stream
                            .write_frame(AvChannelMessage::MediaIndicationAck(channel, m2).into())
                            .await?;
                    }
                }
                AvChannelMessage::SetupRequest(_chan, _m) => {
                    let mut m2 = Wifi::AVChannelSetupResponse::new();
                    m2.set_max_unacked(1);
                    m2.set_media_status(Wifi::avchannel_setup_status::Enum::OK);
                    m2.configs.push(0);
                    stream
                        .write_frame(AvChannelMessage::SetupResponse(channel, m2).into())
                        .await?;
                    if let Some(v) = main.supports_video() {
                        v.wait_for_focus().await;
                        let mut m2 = Wifi::VideoFocusIndication::new();
                        m2.set_focus_mode(Wifi::video_focus_mode::Enum::FOCUSED);
                        m2.set_unrequested(false);
                        stream
                            .write_frame(
                                AvChannelMessage::VideoIndicationResponse(channel, m2).into(),
                            )
                            .await?;
                    }
                }
                AvChannelMessage::SetupResponse(_chan, _m) => unimplemented!(),
                AvChannelMessage::VideoFocusRequest(_chan, m) => {
                    if let Some(v) = main.supports_video() {
                        let mut m2 = Wifi::VideoFocusIndication::new();
                        v.set_focus(m.focus_mode() == Wifi::video_focus_mode::Enum::FOCUSED)
                            .await;
                        m2.set_focus_mode(m.focus_mode());
                        m2.set_unrequested(false);
                        stream
                            .write_frame(
                                AvChannelMessage::VideoIndicationResponse(channel, m2).into(),
                            )
                            .await?;
                    }
                }
                AvChannelMessage::VideoIndicationResponse(_, _) => unimplemented!(),
                AvChannelMessage::StartIndication(_chan, m) => {
                    let mut inner = self.inner.lock().unwrap();
                    inner.session = Some(m.session());
                }
            }
            return Ok(());
        }
        todo!("{:x?}", msg);
    }
}
