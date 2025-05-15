//! Contains sensor channel code

use super::{
    AndroidAutoCommonMessage, AndroidAutoConfiguration, AndroidAutoControlMessage,
    AndroidAutoFrame, ChannelDescriptor, ChannelHandlerTrait, ChannelId, FrameHeader,
    FrameHeaderContents, FrameHeaderType,
};
use crate::{AndroidAutoMainTrait, StreamMux, Wifi};
use protobuf::Message;

/// A message about sensors in android auto
#[derive(Debug)]
pub enum SensorMessage {
    /// A request to start a specific sensor
    SensorStartRequest(ChannelId, Wifi::SensorStartRequestMessage),
    /// A response to the sensor start request
    SensorStartResponse(ChannelId, Wifi::SensorStartResponseMessage),
    /// A message containing sensor data
    Event(ChannelId, Wifi::SensorEventIndication),
}

impl From<SensorMessage> for AndroidAutoFrame {
    fn from(value: SensorMessage) -> Self {
        match value {
            SensorMessage::SensorStartRequest(_, _) => todo!(),
            SensorMessage::SensorStartResponse(chan, m) => {
                let mut data = m.write_to_bytes().unwrap();
                let t = Wifi::sensor_channel_message::Enum::SENSOR_START_RESPONSE as u16;
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
            SensorMessage::Event(chan, m) => {
                let mut data = m.write_to_bytes().unwrap();
                let t = Wifi::sensor_channel_message::Enum::SENSOR_EVENT_INDICATION as u16;
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

impl TryFrom<&AndroidAutoFrame> for SensorMessage {
    type Error = String;
    fn try_from(value: &AndroidAutoFrame) -> Result<Self, Self::Error> {
        use protobuf::Enum;
        let mut ty = [0u8; 2];
        ty.copy_from_slice(&value.data[0..2]);
        let ty = u16::from_be_bytes(ty);
        if let Some(sys) = Wifi::sensor_channel_message::Enum::from_i32(ty as i32) {
            match sys {
                Wifi::sensor_channel_message::Enum::SENSOR_START_REQUEST => {
                    let m = Wifi::SensorStartRequestMessage::parse_from_bytes(&value.data[2..]);
                    match m {
                        Ok(m) => Ok(Self::SensorStartRequest(value.header.channel_id, m)),
                        Err(e) => Err(e.to_string()),
                    }
                }
                Wifi::sensor_channel_message::Enum::SENSOR_START_RESPONSE => unimplemented!(),
                Wifi::sensor_channel_message::Enum::SENSOR_EVENT_INDICATION => unimplemented!(),
                Wifi::sensor_channel_message::Enum::NONE => unimplemented!(),
            }
        } else {
            Err(format!("Not converted message: {:x?}", value.data))
        }
    }
}

/// The handler for the sensor channel in the android auto protocol.
pub struct SensorChannelHandler {}

impl ChannelHandlerTrait for SensorChannelHandler {
    fn build_channel<T: AndroidAutoMainTrait + ?Sized>(
        &self,
        _config: &AndroidAutoConfiguration,
        chanid: ChannelId,
        main: &T,
    ) -> Option<Wifi::ChannelDescriptor> {
        let mut chan = ChannelDescriptor::new();
        let mut sensor = Wifi::SensorChannel::new();
        if let Some(snsrs) = main.supports_sensors() {
            let s = snsrs.get_supported_sensors();
            for s in &s.sensors {
                sensor.sensors.push({
                    let mut sensor1 = Wifi::Sensor::new();
                    sensor1.set_type(*s);
                    sensor1
                });
            }
        }
        chan.sensor_channel.0.replace(Box::new(sensor));
        chan.set_channel_id(chanid as u32);
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
        let msg2: Result<SensorMessage, String> = (&msg).try_into();
        if let Ok(msg2) = msg2 {
            match msg2 {
                SensorMessage::Event(_chan, _m) => unimplemented!(),
                SensorMessage::SensorStartResponse(_, _) => unimplemented!(),
                SensorMessage::SensorStartRequest(_chan, m) => {
                    let mut m2 = Wifi::SensorStartResponseMessage::new();
                    
                    if let Some(sns) = main.supports_sensors() {
                        let stat = match sns.start_sensor(m.sensor_type()).await {
                            Ok(_) => Wifi::status::Enum::OK,
                            Err(_) => Wifi::status::Enum::FAIL,
                        };
                        m2.set_status(stat);
                            stream
                        .write_frame(SensorMessage::SensorStartResponse(channel, m2).into())
                        .await?;
                    }
                }
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
        todo!("{:x?}", msg);
    }
}
