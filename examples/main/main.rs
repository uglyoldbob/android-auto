//! The main example for this library. Use release mode to run it. openh264 is too slow for debug mode.
#[cfg(feature = "wireless")]
use bluetooth_rust::{BluetoothAdapterTrait, MessageToBluetoothHost};
use ringbuf::traits::Producer;
use std::{collections::HashSet, sync::Arc};
use tokio::sync::Mutex;

use android_auto::{HeadUnitInfo, VideoConfiguration};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use eframe::egui;

#[cfg(feature = "wireless")]
mod nmrs_extensions;

#[cfg(feature = "wireless")]
/// Returns the first wifi interface found on the system
async fn get_wifi_interface(nmrs: &nmrs::NetworkManager) -> Option<nmrs::Device> {
    if let Ok(devs) = nmrs.list_wireless_devices().await {
        for dev in devs {
            if dev.device_type == nmrs::DeviceType::Wifi {
                log::info!("Found wifi device {:?}", dev);
                return Some(dev);
            }
        }
    }
    None
}

type AudioProducer = ringbuf::HeapProd<i16>;

struct AndroidAutoInner {
    relay: Option<tokio::task::JoinHandle<()>>,
    connected: bool,
    send: tokio::sync::mpsc::Sender<MessageFromAsync>,
    arecv: Option<tokio::sync::mpsc::Receiver<android_auto::SendableAndroidAutoMessage>>,
    android_send: tokio::sync::mpsc::Sender<android_auto::SendableAndroidAutoMessage>,
    audio_input: Option<cpal::Device>,
    media_stream: Option<(AudioProducer, cpal::Stream)>,
    sys_stream: Option<(AudioProducer, cpal::Stream)>,
    speech_stream: Option<(AudioProducer, cpal::Stream)>,
    input_stream: Option<cpal::Stream>,
}

#[cfg(feature = "wireless")]
#[async_trait::async_trait]
impl android_auto::AndroidAutoWirelessTrait for AndroidAuto {
    async fn setup_bluetooth_profile(
        &self,
        suggestions: &bluetooth_rust::BluetoothRfcommProfileSettings,
    ) -> Result<bluetooth_rust::BluetoothRfcommProfile, String> {
        self.bluetooth
            .register_rfcomm_profile(suggestions.clone())
            .await
    }

    /// Returns wifi details
    fn get_wifi_details(&self) -> android_auto::NetworkInformation {
        self.network.as_ref().to_owned()
    }
}

#[derive(Clone)]
struct AndroidAuto {
    inner: Arc<Mutex<AndroidAutoInner>>,
    config: VideoConfiguration,
    #[cfg(feature = "wireless")]
    blue: android_auto::BluetoothInformation,
    #[cfg(feature = "wireless")]
    bluetooth: Arc<bluetooth_rust::BluetoothAdapter>,
    #[cfg(feature = "wireless")]
    /// The network information
    network: Arc<android_auto::NetworkInformation>,
    /// The sensors config
    sensors: android_auto::SensorInformation,
    /// The input channel config
    input_config: android_auto::InputConfiguration,
}

enum MessageFromAsync {
    VideoData {
        data: Vec<u8>,
        _timestamp: Option<u64>,
    },
    Connected,
    Disconnected,
    ExitContainer,
}

enum MessageToAsync {
    AndroidAutoMessage(android_auto::SendableAndroidAutoMessage),
}

#[async_trait::async_trait]
impl android_auto::AndroidAutoVideoChannelTrait for AndroidAuto {
    async fn receive_video(&self, data: Vec<u8>, timestamp: Option<u64>) {
        let i = self.inner.lock().await;
        let _ = i
            .send
            .send(MessageFromAsync::VideoData {
                data,
                _timestamp: timestamp,
            })
            .await;
    }

    async fn setup_video(&self) -> Result<(), ()> {
        Ok(())
    }

    async fn teardown_video(&self) {}

    async fn wait_for_focus(&self) {}

    async fn set_focus(&self, _focus: bool) {}

    fn retrieve_video_configuration(&self) -> &VideoConfiguration {
        &self.config
    }
}

#[cfg(feature = "wireless")]
#[async_trait::async_trait]
impl android_auto::AndroidAutoBluetoothTrait for AndroidAuto {
    async fn do_stuff(&self) {}

    fn get_config(&self) -> &android_auto::BluetoothInformation {
        &self.blue
    }
}

#[async_trait::async_trait]
impl android_auto::AndroidAutoSensorTrait for AndroidAuto {
    fn get_supported_sensors(&self) -> &android_auto::SensorInformation {
        &self.sensors
    }

    async fn start_sensor(&self, stype: android_auto::Wifi::sensor_type::Enum) -> Result<(), ()> {
        if self.sensors.sensors.contains(&stype) {
            let mut m3 = android_auto::Wifi::SensorEventIndication::new();
            match stype {
                android_auto::Wifi::sensor_type::Enum::DRIVING_STATUS => {
                    let mut ds = android_auto::Wifi::DrivingStatus::new();
                    ds.set_status(android_auto::Wifi::DrivingStatusEnum::UNRESTRICTED as i32);
                    m3.driving_status.push(ds);
                }
                android_auto::Wifi::sensor_type::Enum::NIGHT_DATA => {
                    let mut ds = android_auto::Wifi::NightMode::new();
                    ds.set_is_night(false);
                    m3.night_mode.push(ds);
                }
                _ => {
                    todo!();
                }
            }
            let s = self.inner.lock().await;
            let m = android_auto::AndroidAutoMessage::Sensor(m3);
            s.android_send.send(m.sendable()).await.map_err(|_| ())?;
            Ok(())
        } else {
            Err(())
        }
    }
}

#[async_trait::async_trait]
impl android_auto::AndroidAutoAudioOutputTrait for AndroidAuto {
    async fn open_output_channel(&self, _t: android_auto::AudioChannelType) -> Result<(), ()> {
        Ok(())
    }

    async fn close_output_channel(&self, _t: android_auto::AudioChannelType) -> Result<(), ()> {
        Ok(())
    }

    async fn receive_output_audio(&self, t: android_auto::AudioChannelType, data: Vec<u8>) {
        let mut s = self.inner.lock().await;
        let r2: Vec<i16> = data
            .chunks_exact(2)
            .map(|v| i16::from_le_bytes([v[0], v[1]]))
            .collect();
        match t {
            android_auto::AudioChannelType::Media => {
                s.media_stream.as_mut().map(|m| m.0.push_slice(&r2));
            }
            android_auto::AudioChannelType::System => {
                s.sys_stream.as_mut().map(|m| m.0.push_slice(&r2));
            }
            android_auto::AudioChannelType::Speech => {
                s.speech_stream.as_mut().map(|m| m.0.push_slice(&r2));
            }
        }
    }

    async fn start_output_audio(&self, t: android_auto::AudioChannelType) {
        let s = self.inner.lock().await;
        match t {
            android_auto::AudioChannelType::Media => {
                s.media_stream.as_ref().map(|m| m.1.play());
            }
            android_auto::AudioChannelType::System => {
                s.sys_stream.as_ref().map(|m| m.1.play());
            }
            android_auto::AudioChannelType::Speech => {
                s.speech_stream.as_ref().map(|m| m.1.play());
            }
        }
    }

    async fn stop_output_audio(&self, t: android_auto::AudioChannelType) {
        let s = self.inner.lock().await;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        match t {
            android_auto::AudioChannelType::Media => {
                s.media_stream.as_ref().map(|m| m.1.pause());
            }
            android_auto::AudioChannelType::System => {
                s.sys_stream.as_ref().map(|m| m.1.pause());
            }
            android_auto::AudioChannelType::Speech => {
                s.speech_stream.as_ref().map(|m| m.1.pause());
            }
        }
    }
}

#[async_trait::async_trait]
impl android_auto::AndroidAutoInputChannelTrait for AndroidAuto {
    async fn binding_request(&self, _code: u32) -> Result<(), ()> {
        Ok(())
    }

    fn retrieve_input_configuration(&self) -> &android_auto::InputConfiguration {
        &self.input_config
    }
}

#[async_trait::async_trait]
impl android_auto::AndroidAutoAudioInputTrait for AndroidAuto {
    async fn open_input_channel(&self) -> Result<(), ()> {
        log::error!("Start audio input channel");
        let mut s = self.inner.lock().await;
        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate: 16000,
            buffer_size: cpal::BufferSize::Default,
        };
        if let Some(ai) = &s.audio_input {
            let android_send = s.android_send.clone();
            if let Ok(str) = ai.build_input_stream(
                &config,
                move |data: &[i16], _| {
                    let bytes: Vec<u8> = data.iter().flat_map(|s| s.to_le_bytes()).collect();
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_micros() as u64;
                    let msg = android_auto::AndroidAutoMessage::Audio(Some(timestamp), bytes);
                    if let Err(e) = android_send.try_send(msg.sendable()) {
                        log::warn!("Dropped audio input frame: {:?}", e);
                    }
                },
                |err| log::error!("Audio input error: {:?}", err),
                None,
            ) {
                let _ = str.play();
                s.input_stream = Some(str);
            } else {
                log::error!("Failed to open input channel stream");
            }
        }
        Ok(())
    }
    async fn close_input_channel(&self) -> Result<(), ()> {
        let mut s = self.inner.lock().await;
        s.input_stream.take();
        Ok(())
    }
    async fn start_input_audio(&self) {}

    async fn audio_input_ack(&self, chan: u8, ack: android_auto::Wifi::AVMediaAckIndication) {
        log::info!("Ack audio input for chan {chan} {ack:?}");
    }

    async fn stop_input_audio(&self) {
        log::error!("Stop audio input channel");
        let mut s = self.inner.lock().await;
        s.input_stream.take();
    }
}

#[cfg(feature = "usb")]
#[async_trait::async_trait]
impl android_auto::AndroidAutoWiredTrait for AndroidAuto {}

#[async_trait::async_trait]
impl android_auto::AndroidAutoMainTrait for AndroidAuto {
    async fn connect(&self) {
        let mut i = self.inner.lock().await;
        let _ = i.send.send(MessageFromAsync::Connected).await;
        i.connected = true;
    }

    async fn disconnect(&self) {
        let mut s = self.inner.lock().await;
        let _ = s.send.send(MessageFromAsync::Disconnected).await;
        s.connected = false;
    }

    async fn get_receiver(
        &self,
    ) -> Option<tokio::sync::mpsc::Receiver<android_auto::SendableAndroidAutoMessage>> {
        let mut s = self.inner.lock().await;
        s.arecv.take()
    }

    #[cfg(feature = "wireless")]
    fn supports_bluetooth(&self) -> Option<&dyn android_auto::AndroidAutoBluetoothTrait> {
        Some(self)
    }

    #[cfg(feature = "wireless")]
    fn supports_wireless(&self) -> Option<Arc<dyn android_auto::AndroidAutoWirelessTrait>> {
        Some(Arc::new(self.clone()))
    }

    #[cfg(feature = "usb")]
    fn supports_wired(&self) -> Option<Arc<dyn android_auto::AndroidAutoWiredTrait>> {
        Some(Arc::new(self.clone()))
    }
}

impl AndroidAuto {
    fn new(
        mut recv: tokio::sync::mpsc::Receiver<MessageToAsync>,
        send: tokio::sync::mpsc::Sender<MessageFromAsync>,
        #[cfg(feature = "wireless")] bluetooth: Arc<bluetooth_rust::BluetoothAdapter>,
        #[cfg(feature = "wireless")] blue_address: String,
        #[cfg(feature = "wireless")] network: android_auto::NetworkInformation,
        android_recv: tokio::sync::mpsc::Receiver<android_auto::SendableAndroidAutoMessage>,
        android_send: tokio::sync::mpsc::Sender<android_auto::SendableAndroidAutoMessage>,
    ) -> Self {
        let mut s = HashSet::new();
        s.insert(android_auto::Wifi::sensor_type::Enum::DRIVING_STATUS);
        s.insert(android_auto::Wifi::sensor_type::Enum::NIGHT_DATA);
        let android_send2 = android_send.clone();
        let relay = tokio::spawn(async move {
            'main_loop: loop {
                while let Some(m) = recv.recv().await {
                    match m {
                        MessageToAsync::AndroidAutoMessage(android_auto_message) => {
                            let a = android_send2.send(android_auto_message).await;
                            if let Err(e) = a {
                                log::error!("Error relaying info {e:?}");
                                break 'main_loop;
                            }
                        }
                    }
                }
            }
        });
        let (ai, media_stream, sys_stream, speech_stream) = {
            let h = cpal::default_host();
            let mut ao = h.default_output_device();
            let ai = h.default_input_device();
            let mut media_stream = None;
            let mut sys_stream = None;
            let mut speech_stream = None;
            if let Some(ao) = &mut ao {
                if let Ok(c) = ao.supported_output_configs() {
                    {
                        let mut media_config = None;
                        let mut sys_config = None;
                        let mut speech_config = None;
                        for c in c {
                            const MEDIA_RATE: u32 = 48000;
                            const MEDIA_CHANNELS: u16 = 2;
                            if c.min_sample_rate() <= MEDIA_RATE
                                && c.max_sample_rate() >= MEDIA_RATE
                            {
                                if c.channels() == MEDIA_CHANNELS {
                                    if c.sample_format() == cpal::SampleFormat::I16 {
                                        media_config = c.try_with_sample_rate(MEDIA_RATE);
                                    }
                                }
                            }

                            const SYS_RATE: u32 = 16000;
                            const SYS_CHANNELS: u16 = 1;
                            if c.min_sample_rate() <= SYS_RATE && c.max_sample_rate() >= SYS_RATE {
                                if c.channels() == SYS_CHANNELS {
                                    if c.sample_format() == cpal::SampleFormat::I16 {
                                        sys_config = c.try_with_sample_rate(SYS_RATE);
                                    }
                                }
                            }

                            const SPEECH_RATE: u32 = 16000;
                            const SPEECH_CHANNELS: u16 = 1;
                            if c.min_sample_rate() <= SPEECH_RATE
                                && c.max_sample_rate() >= SPEECH_RATE
                            {
                                if c.channels() == SPEECH_CHANNELS {
                                    if c.sample_format() == cpal::SampleFormat::I16 {
                                        speech_config = c.try_with_sample_rate(SPEECH_RATE);
                                    }
                                }
                            }
                        }
                        if let Some(mc) = media_config {
                            let rb = ringbuf::HeapRb::new(48000);
                            let (producer, mut consumer) = ringbuf::traits::Split::split(rb);
                            let s = ao.build_output_stream(
                                &mc.config(),
                                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                                    let mut index = 0;
                                    while index < data.len() {
                                        let c = ringbuf::traits::Consumer::pop_slice(
                                            &mut consumer,
                                            &mut data[index..],
                                        );
                                        if c == 0 {
                                            break;
                                        }
                                        index += c;
                                    }
                                },
                                move |err| {
                                    log::error!("Error in media audio output: {:?}", err);
                                },
                                None,
                            );
                            if let Ok(s) = s {
                                media_stream = Some((producer, s));
                            }
                        }
                        if let Some(mc) = sys_config {
                            let rb = ringbuf::HeapRb::new(16000);
                            let (producer, mut consumer) = ringbuf::traits::Split::split(rb);
                            let s = ao.build_output_stream(
                                &mc.config(),
                                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                                    let mut index = 0;
                                    while index < data.len() {
                                        let c = ringbuf::traits::Consumer::pop_slice(
                                            &mut consumer,
                                            &mut data[index..],
                                        );
                                        if c == 0 {
                                            break;
                                        }
                                        index += c;
                                    }
                                },
                                move |err| {
                                    log::error!("Error in media audio output: {:?}", err);
                                },
                                None,
                            );
                            if let Ok(s) = s {
                                sys_stream = Some((producer, s));
                            }
                        }
                        if let Some(mc) = speech_config {
                            let rb = ringbuf::HeapRb::new(16000);
                            let (producer, mut consumer) = ringbuf::traits::Split::split(rb);
                            let s = ao.build_output_stream(
                                &mc.config(),
                                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                                    let mut index = 0;
                                    while index < data.len() {
                                        let c = ringbuf::traits::Consumer::pop_slice(
                                            &mut consumer,
                                            &mut data[index..],
                                        );
                                        if c == 0 {
                                            break;
                                        }
                                        index += c;
                                    }
                                },
                                move |err| {
                                    log::error!("Error in media audio output: {:?}", err);
                                },
                                None,
                            );
                            if let Ok(s) = s {
                                speech_stream = Some((producer, s));
                            }
                        }
                    }
                }
            }
            (ai, media_stream, sys_stream, speech_stream)
        };
        Self {
            inner: Arc::new(Mutex::new(AndroidAutoInner {
                relay: Some(relay),
                connected: false,
                send,
                arecv: Some(android_recv),
                android_send,
                audio_input: ai,
                media_stream,
                sys_stream,
                speech_stream,
                input_stream: None,
            })),
            #[cfg(feature = "wireless")]
            bluetooth,
            #[cfg(feature = "wireless")]
            network: Arc::new(network),
            #[cfg(feature = "wireless")]
            blue: android_auto::BluetoothInformation {
                address: blue_address,
            },
            config: VideoConfiguration {
                resolution: android_auto::Wifi::video_resolution::Enum::_480p,
                fps: android_auto::Wifi::video_fps::Enum::_30,
                dpi: 111,
            },
            sensors: android_auto::SensorInformation { sensors: s },
            input_config: android_auto::InputConfiguration {
                keycodes: vec![1, 2, 3, 4, 5],
                touchscreen: Some((800, 480)),
            },
        }
    }

    async fn start_android_auto(
        self,
        config: android_auto::AndroidAutoConfiguration,
    ) -> Result<(), String> {
        let mut joinset = tokio::task::JoinSet::new();
        let relay = {
            let mut s = self.inner.lock().await;
            s.relay.take()
        };
        use android_auto::AndroidAutoMainTrait;
        let b = Box::new(self);
        let a = b.run(config, &mut joinset).await;
        log::info!("join_all on the android auto joinset");
        joinset.join_all().await;
        log::info!("Done with join_all");
        relay.map(|r| r.abort());
        a
    }
}

struct MyEguiApp {
    android_auto_video_decoder: openh264::decoder::Decoder,
    android_auto_texture: Option<egui::TextureHandle>,
    container: Option<AndroidAutoContainer>,
}

impl MyEguiApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            android_auto_video_decoder: openh264::decoder::Decoder::new().unwrap(),
            android_auto_texture: None,
            container: Some(AndroidAutoContainer::new()),
        }
    }
}

impl eframe::App for MyEguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut replace_container = false;
        if let Some(con) = &mut self.container {
            while let Ok(v) = con.recv.try_recv() {
                match v {
                    MessageFromAsync::ExitContainer => {
                        log::info!("Got request to exit container");
                        replace_container = true;
                    }
                    MessageFromAsync::Connected => {
                        log::info!("Connected");
                    }
                    MessageFromAsync::Disconnected => {
                        log::info!("Android auto disconnected");
                        let _ = self.android_auto_video_decoder.flush_remaining();
                        self.android_auto_texture.take();
                    }
                    MessageFromAsync::VideoData {
                        data,
                        _timestamp: _,
                    } => {
                        let mut units = openh264::nal_units(&data).peekable();
                        while let Some(p) = units.next() {
                            match self.android_auto_video_decoder.decode(p) {
                                Err(e) => {
                                    log::error!("Failed to decode android auto video {:?}", e);
                                }
                                Ok(Some(image)) => {
                                    use openh264::formats::YUVSource;
                                    let rgb_len = image.rgb8_len();
                                    let mut rgb_raw = vec![0; rgb_len];
                                    image.write_rgb8(&mut rgb_raw);
                                    let (w, h) = image.dimensions_uv();
                                    let pixels: Vec<egui::Color32> = rgb_raw
                                        .chunks_exact(3)
                                        .map(|i| egui::Color32::from_rgb(i[0], i[1], i[2]))
                                        .collect();
                                    let image = egui::ColorImage {
                                        source_size: egui::Vec2 {
                                            x: w as f32 * 2.0,
                                            y: h as f32 * 2.0,
                                        },
                                        size: [w * 2usize, h * 2usize],
                                        pixels,
                                    };
                                    if self.android_auto_texture.is_none() {
                                        self.android_auto_texture = Some(ctx.load_texture(
                                            "android_auto",
                                            image,
                                            egui::TextureOptions::LINEAR,
                                        ));
                                    } else if let Some(t) = &mut self.android_auto_texture {
                                        t.set_partial([0, 0], image, egui::TextureOptions::LINEAR);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        if replace_container {
            self.container = Some(AndroidAutoContainer::new());
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            let size = ui.available_size();
            if let Some(t) = &self.android_auto_texture {
                ctx.request_repaint();
                let isize = t.size();
                let zoom = isize[1] as f32 / size.y;
                let zoom2 = isize[0] as f32 / size.x;
                let zoom = zoom.max(zoom2);
                let dsize = t.size_vec2() / zoom;
                let p = ui.cursor();
                let r = ui.add(
                    egui::Image::from_texture(egui::load::SizedTexture {
                        id: t.id(),
                        size: dsize,
                    })
                    .sense(egui::Sense::drag()),
                );
                let o = if let Some(mut o) = r.interact_pointer_pos() {
                    o.x -= p.left();
                    o.y -= p.top();
                    o.x *= zoom;
                    o.y *= zoom;
                    Some(o)
                } else if let Some(mut o) = r.hover_pos() {
                    o.x -= p.left();
                    o.y -= p.top();
                    o.x *= zoom;
                    o.y *= zoom;
                    Some(o)
                } else {
                    None
                };
                if let Some(o) = o {
                    let mut i_event = android_auto::Wifi::InputEventIndication::new();
                    let timestamp: u64 = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_micros() as u64;
                    i_event.set_timestamp(timestamp);
                    let mut te = android_auto::Wifi::TouchEvent::new();
                    let mut tl = android_auto::Wifi::TouchLocation::new();
                    tl.set_x(o.x as u32);
                    tl.set_y(o.y as u32);
                    tl.set_pointer_id(0);
                    te.touch_location = vec![tl];
                    let mut do_touch = true;
                    if r.drag_started() {
                        te.set_touch_action(android_auto::Wifi::touch_action::Enum::POINTER_DOWN);
                    } else if r.drag_stopped() {
                        te.set_touch_action(android_auto::Wifi::touch_action::Enum::POINTER_UP);
                    } else if r.dragged() {
                        te.set_touch_action(android_auto::Wifi::touch_action::Enum::DRAG);
                    } else if r.hovered() {
                        te.set_touch_action(android_auto::Wifi::touch_action::Enum::DRAG);
                    } else {
                        do_touch = false;
                    }
                    if do_touch {
                        i_event.touch_event = android_auto::protobuf::MessageField::some(te);
                        let e = android_auto::AndroidAutoMessage::Input(i_event);
                        if let Some(con) = &mut self.container {
                            let a = con
                                .send
                                .blocking_send(MessageToAsync::AndroidAutoMessage(e.sendable()));
                            if let Err(e) = a {
                                log::error!("Error sending touch event {:?}", e);
                            }
                        }
                    }
                }
            }
        });
    }
}

struct AndroidAutoContainer {
    thread: Option<std::thread::JoinHandle<Result<(), String>>>,
    recv: tokio::sync::mpsc::Receiver<MessageFromAsync>,
    send: tokio::sync::mpsc::Sender<MessageToAsync>,
    kill: Option<tokio::sync::oneshot::Sender<()>>,
}

impl AndroidAutoContainer {
    fn new() -> Self {
        let to_async = tokio::sync::mpsc::channel(50);
        let from_async = tokio::sync::mpsc::channel(50);
        let kill = tokio::sync::oneshot::channel::<()>();

        let runtime_builder = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to build the Tokio runtime");
        let send_exit = from_async.0.clone();
        let thread_handle = std::thread::spawn(move || {
            // 3. Enter the runtime context and run async code within this specific thread
            let r = runtime_builder.block_on(async {
                #[cfg(feature = "wireless")]
                let wifi = nmrs::NetworkManager::new().await.expect("Wifi not found");
                #[cfg(feature = "wireless")]
                let wifi_dev = get_wifi_interface(&wifi)
                    .await
                    .expect("No wifi device found");

                #[cfg(feature = "wireless")]
                let hotspot_ssid = "Hotspot".to_string();
                #[cfg(feature = "wireless")]
                let hotspot_psk = "qwertyuiop".to_string();
                #[cfg(feature = "wireless")]
                nmrs_extensions::start_hotspot(
                    hotspot_ssid.clone(),
                    hotspot_psk.clone(),
                    &wifi_dev.path,
                )
                .await
                .expect("Failed to build wifi hotspot");

                #[cfg(feature = "wireless")]
                let (mut bluechan, bluetooth) = {
                    let bluechan = tokio::sync::mpsc::channel(5);
                    let mut bluetooth = bluetooth_rust::BluetoothAdapterBuilder::new();
                    bluetooth.with_sender(bluechan.0);
                    let bluetooth =
                        Arc::new(bluetooth.build().await.expect("Could not open bluetooth"));
                    (bluechan.1, bluetooth)
                };
                #[cfg(feature = "wireless")]
                bluetooth
                    .set_discoverable(true)
                    .await
                    .expect("Failed to make bluetooth discoverable");

                #[cfg(feature = "wireless")]
                tokio::spawn(async move {
                    loop {
                        if let Some(m) = bluechan.recv().await {
                            match m {
                                MessageToBluetoothHost::DisplayPasskey(a, sender) => {
                                    log::info!("Passkey is {}", a);
                                    let _ =
                                        sender.send(bluetooth_rust::ResponseToPasskey::Yes).await;
                                }
                                MessageToBluetoothHost::ConfirmPasskey(a, sender) => {
                                    log::info!("Passkey is confirmed {}", a);
                                    let _ =
                                        sender.send(bluetooth_rust::ResponseToPasskey::Yes).await;
                                }
                                MessageToBluetoothHost::CancelDisplayPasskey => {
                                    log::info!("Cancel show passkey");
                                }
                            }
                        }
                    }
                });

                #[cfg(feature = "wireless")]
                let blue_addresses: Vec<[u8; 6]> = bluetooth.addresses().await;
                #[cfg(feature = "wireless")]
                let bluetooth_address = blue_addresses
                    .first()
                    .map(|b| {
                        let a = format!(
                            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                            b[0], b[1], b[2], b[3], b[4], b[5]
                        );
                        a
                    })
                    .expect("No bluetooth hardware found");

                let aauto = tokio::sync::mpsc::channel(50);

                let aa = AndroidAuto::new(
                    to_async.1,
                    from_async.0,
                    #[cfg(feature = "wireless")]
                    bluetooth,
                    #[cfg(feature = "wireless")]
                    bluetooth_address,
                    #[cfg(feature = "wireless")]
                    android_auto::NetworkInformation {
                        ssid: hotspot_ssid,
                        psk: hotspot_psk,
                        mac_addr: wifi_dev.identity.current_mac.clone(),
                        ip: "10.42.0.1".to_string(),
                        port: 5277,
                        security_mode: android_auto::Bluetooth::SecurityMode::WPA2_PERSONAL,
                        ap_type: android_auto::Bluetooth::AccessPointType::STATIC,
                    },
                    aauto.1,
                    aauto.0,
                );
                let config = android_auto::AndroidAutoConfiguration {
                    unit: HeadUnitInfo {
                        name: "Example".to_string(),
                        car_model: "Example".to_string(),
                        car_year: "1943".to_string(),
                        car_serial: "42".to_string(),
                        left_hand: false,
                        head_manufacturer: "Example".to_string(),
                        head_model: "Example".to_string(),
                        sw_build: "37".to_string(),
                        sw_version: "1.2.3".to_string(),
                        native_media: true,
                        hide_clock: Some(true),
                    },
                    custom_certificate: None,
                };
                tokio::select! {
                    _ = aa.start_android_auto(config) => {
                        log::info!("android auto exited");
                    }
                    _ = kill.1 => {
                        log::info!("Killing the android auto container");
                    }
                }
                Ok::<(), String>(())
            });
            log::info!("Exiting the android auto container thread");
            send_exit
                .blocking_send(MessageFromAsync::ExitContainer)
                .map_err(|e| e.to_string())?;
            r
        });
        Self {
            thread: Some(thread_handle),
            recv: from_async.1,
            send: to_async.0,
            kill: Some(kill.0),
        }
    }
}

impl Drop for AndroidAutoContainer {
    fn drop(&mut self) {
        let _ = self.kill.take().map(|s| s.send(()));
        self.thread.take().map(|t| t.join());
    }
}

fn main() -> Result<(), u32> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()
        .unwrap();
    let native_options = eframe::NativeOptions::default();

    android_auto::setup();

    let _ = eframe::run_native(
        "Android auto demo",
        native_options,
        Box::new(move |cc| Ok(Box::new(MyEguiApp::new(cc)))),
    );
    Ok(())
}
