//! This is for the media status channel handler code

use protobuf::Message;

use crate::{
    AndroidAutoConfiguration, AndroidAutoFrame, AndroidAutoMainTrait, ChannelHandlerTrait,
    ChannelId, StreamMux, Wifi, common::AndroidAutoCommonMessage,
};

/// A message about the media status of currently playing media
#[derive(Debug)]
enum MediaStatusMessage {
    /// A message containing basic information about changes to the currently playing media
    Playback(ChannelId, Wifi::MediaInfoChannelPlaybackData),
    /// The metadata containing information about the media currently playing
    Metadata(ChannelId, Wifi::MediaInfoChannelMetadataData),
    /// The media status message was invalid for some reason
    Invalid,
}

impl From<MediaStatusMessage> for AndroidAutoFrame {
    fn from(value: MediaStatusMessage) -> Self {
        match value {
            MediaStatusMessage::Playback(_, _) => todo!(),
            MediaStatusMessage::Metadata(_, _) => todo!(),
            MediaStatusMessage::Invalid => unimplemented!(),
        }
    }
}

impl TryFrom<&AndroidAutoFrame> for MediaStatusMessage {
    type Error = String;
    fn try_from(value: &AndroidAutoFrame) -> Result<Self, Self::Error> {
        use protobuf::Enum;
        let mut ty = [0u8; 2];
        ty.copy_from_slice(&value.data[0..2]);
        let ty = u16::from_be_bytes(ty);
        if let Some(sys) = Wifi::media_info_channel_message::Enum::from_i32(ty as i32) {
            match sys {
                Wifi::media_info_channel_message::Enum::PLAYBACK => {
                    let m = Wifi::MediaInfoChannelPlaybackData::parse_from_bytes(&value.data);
                    match m {
                        Ok(m) => Ok(Self::Playback(value.header.channel_id, m)),
                        Err(_) => Ok(Self::Invalid),
                    }
                }
                Wifi::media_info_channel_message::Enum::METADATA => {
                    let m = Wifi::MediaInfoChannelMetadataData::parse_from_bytes(&value.data);
                    match m {
                        Ok(m) => Ok(Self::Metadata(value.header.channel_id, m)),
                        Err(_) => Ok(Self::Invalid),
                    }
                }
                Wifi::media_info_channel_message::Enum::NONE => todo!(),
            }
        } else {
            Err(format!("Not converted message: {:x?}", value.data))
        }
    }
}

/// The handler for media status for the android auto protocol
pub struct MediaStatusChannelHandler {}

impl ChannelHandlerTrait for MediaStatusChannelHandler {
    fn build_channel<T: AndroidAutoMainTrait + ?Sized>(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
        _main: &T,
    ) -> Option<Wifi::ChannelDescriptor> {
        let mut chan = Wifi::ChannelDescriptor::new();
        chan.set_channel_id(chanid as u32);
        let mchan = Wifi::MediaInfoChannel::new();
        chan.media_infoChannel.0.replace(Box::new(mchan));
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
        let msg2: Result<MediaStatusMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                MediaStatusMessage::Metadata(_, m) => {
                    log::info!("Metadata {:?}", m);
                }
                MediaStatusMessage::Playback(_, m) => {
                    log::info!("Playback {:?}", m);
                }
                MediaStatusMessage::Invalid => {
                    log::error!("Received invalid media info frame");
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
        todo!("{:?} {:?}", msg2, msg3);
    }
}
