//! The main example for this library
use bluetooth_rust::{BluetoothAdapterTrait, MessageToBluetoothHost};
use std::{collections::HashSet, sync::Arc};
use tokio::sync::Mutex;

use android_auto::{HeadUnitInfo, VideoConfiguration};
use eframe::egui;

mod nmrs_extensions;

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

struct AndroidAutoInner {
    connected: bool,
    recv: tokio::sync::mpsc::Receiver<MessageToAsync>,
    send: tokio::sync::mpsc::Sender<MessageFromAsync>,
    arecv: Option<tokio::sync::mpsc::Receiver<android_auto::SendableAndroidAutoMessage>>,
    android_send: tokio::sync::mpsc::Sender<android_auto::SendableAndroidAutoMessage>,
}

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
    blue: android_auto::BluetoothInformation,
    bluetooth: Arc<bluetooth_rust::BluetoothAdapter>,
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
        timestamp: Option<u64>,
    },
}

enum MessageToAsync {
    Nothing,
}

#[async_trait::async_trait]
impl android_auto::AndroidAutoVideoChannelTrait for AndroidAuto {
    async fn receive_video(&self, data: Vec<u8>, timestamp: Option<u64>) {
        let i = self.inner.lock().await;
        let _ = i
            .send
            .send(MessageFromAsync::VideoData { data, timestamp })
            .await;
    }

    async fn setup_video(&self) -> Result<(), ()> {
        Ok(())
    }

    async fn teardown_video(&self) {}

    async fn wait_for_focus(&self) {}

    async fn set_focus(&self, focus: bool) {}

    fn retrieve_video_configuration(&self) -> &VideoConfiguration {
        &self.config
    }
}

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
    async fn open_channel(&self, t: android_auto::AudioChannelType) -> Result<(), ()> {
        let s = self.inner.lock().await;

        Ok(())
    }

    async fn close_channel(&self, t: android_auto::AudioChannelType) -> Result<(), ()> {
        let s = self.inner.lock().await;
        Ok(())
    }

    async fn receive_audio(&self, t: android_auto::AudioChannelType, data: Vec<u8>) {
        let s = self.inner.lock().await;
    }

    async fn start_audio(&self, t: android_auto::AudioChannelType) {
        let s = self.inner.lock().await;
    }

    async fn stop_audio(&self, t: android_auto::AudioChannelType) {
        let s = self.inner.lock().await;
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
    async fn open_channel(&self) -> Result<(), ()> {
        Ok(())
    }
    async fn close_channel(&self) -> Result<(), ()> {
        Ok(())
    }
    async fn start_audio(&self) {
        log::error!("Start audio input channel");
    }
    async fn stop_audio(&self) {
        log::error!("Stop audio input channel");
    }
}

#[async_trait::async_trait]
impl android_auto::AndroidAutoMainTrait for AndroidAuto {
    async fn connect(&self) {
        let mut i = self.inner.lock().await;
        i.connected = true;
    }

    async fn disconnect(&self) {
        let mut i = self.inner.lock().await;
        i.connected = false;
    }

    async fn get_receiver(
        &self,
    ) -> Option<tokio::sync::mpsc::Receiver<android_auto::SendableAndroidAutoMessage>> {
        let mut s = self.inner.lock().await;
        s.arecv.take()
    }

    fn supports_video(&self) -> Option<&dyn android_auto::AndroidAutoVideoChannelTrait> {
        Some(self)
    }

    fn supports_bluetooth(&self) -> Option<&dyn android_auto::AndroidAutoBluetoothTrait> {
        Some(self)
    }

    fn supports_wireless(&self) -> Option<Arc<dyn android_auto::AndroidAutoWirelessTrait>> {
        Some(Arc::new(self.clone()))
    }

    fn supports_sensors(&self) -> Option<&dyn android_auto::AndroidAutoSensorTrait> {
        Some(self)
    }

    fn supports_audio_output(&self) -> Option<&dyn android_auto::AndroidAutoAudioOutputTrait> {
        Some(self)
    }

    fn supports_audio_input(&self) -> Option<&dyn android_auto::AndroidAutoAudioInputTrait> {
        Some(self)
    }
}

impl AndroidAuto {
    fn new(
        recv: tokio::sync::mpsc::Receiver<MessageToAsync>,
        send: tokio::sync::mpsc::Sender<MessageFromAsync>,
        bluetooth: Arc<bluetooth_rust::BluetoothAdapter>,
        blue_address: String,
        network: android_auto::NetworkInformation,
        android_recv: tokio::sync::mpsc::Receiver<android_auto::SendableAndroidAutoMessage>,
        android_send: tokio::sync::mpsc::Sender<android_auto::SendableAndroidAutoMessage>,
    ) -> Self {
        let mut s = HashSet::new();
        s.insert(android_auto::Wifi::sensor_type::Enum::DRIVING_STATUS);
        s.insert(android_auto::Wifi::sensor_type::Enum::NIGHT_DATA);
        Self {
            inner: Arc::new(Mutex::new(AndroidAutoInner {
                connected: false,
                send,
                recv,
                arecv: Some(android_recv),
                android_send,
            })),
            bluetooth,
            network: Arc::new(network),
            blue: android_auto::BluetoothInformation {
                address: blue_address,
            },
            config: VideoConfiguration {
                resolution: android_auto::Wifi::video_resolution::Enum::_720p,
                fps: android_auto::Wifi::video_fps::Enum::_30,
                dpi: 300,
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
        let aas = android_auto::AndroidAutoServer::new().await;
        let mut joinset = tokio::task::JoinSet::new();
        aas.run(config, &mut joinset, self).await?;
        joinset.join_all().await;
        Ok(())
    }
}

struct MyEguiApp {
    recv: tokio::sync::mpsc::Receiver<MessageFromAsync>,
    send: tokio::sync::mpsc::Sender<MessageToAsync>,
}

impl MyEguiApp {
    fn new(
        _cc: &eframe::CreationContext<'_>,
        recv: tokio::sync::mpsc::Receiver<MessageFromAsync>,
        send: tokio::sync::mpsc::Sender<MessageToAsync>,
    ) -> Self {
        Self { recv, send }
    }
}

impl eframe::App for MyEguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint();
        while let Ok(v) = self.recv.try_recv() {
            match v {
                MessageFromAsync::VideoData { data, timestamp: _ } => {
                    log::info!("Received video data len {}", data.len());
                }
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("I am groot");
        });
    }
}

fn main() -> Result<(), u32> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()
        .unwrap();
    let native_options = eframe::NativeOptions::default();

    let runtime_builder = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to build the Tokio runtime");

    let to_async = tokio::sync::mpsc::channel(50);
    let from_async = tokio::sync::mpsc::channel(50);

    android_auto::setup();

    let _thread_handle = std::thread::spawn(move || {
        // 3. Enter the runtime context and run async code within this specific thread
        runtime_builder.block_on(async {
            println!(
                "Tokio runtime is running in a new thread: {:?}",
                std::thread::current().name()
            );

            let wifi = nmrs::NetworkManager::new().await.expect("Wifi not found");
            let wifi_dev = get_wifi_interface(&wifi)
                .await
                .expect("No wifi device found");

            let hotspot_ssid = "Hotspot".to_string();
            let hotspot_psk = "qwertyuiop".to_string();
            nmrs_extensions::start_hotspot(
                hotspot_ssid.clone(),
                hotspot_psk.clone(),
                &wifi_dev.path,
            )
            .await
            .expect("Failed to build wifi hotspot");

            let (mut bluechan, bluetooth) = {
                let bluechan = tokio::sync::mpsc::channel(5);
                let mut bluetooth = bluetooth_rust::BluetoothAdapterBuilder::new();
                bluetooth.with_sender(bluechan.0);
                let bluetooth =
                    Arc::new(bluetooth.build().await.expect("Could not open bluetooth"));
                (bluechan.1, bluetooth)
            };

            tokio::spawn(async move {
                loop {
                    if let Some(m) = bluechan.recv().await {
                        match m {
                            MessageToBluetoothHost::DisplayPasskey(_, sender) => {
                                let _ = sender.send(bluetooth_rust::ResponseToPasskey::Yes).await;
                            }
                            MessageToBluetoothHost::ConfirmPasskey(_, sender) => {
                                let _ = sender.send(bluetooth_rust::ResponseToPasskey::Yes).await;
                            }
                            MessageToBluetoothHost::CancelDisplayPasskey => {}
                        }
                    }
                }
            });

            let blue_addresses: Vec<[u8; 6]> = bluetooth.addresses().await;
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
                bluetooth,
                bluetooth_address,
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
            aa.start_android_auto(config).await?;
            Ok::<(), String>(())
        })
    });

    let _ = eframe::run_native(
        "Android auto demo",
        native_options,
        Box::new(move |cc| Ok(Box::new(MyEguiApp::new(cc, from_async.1, to_async.0)))),
    );
    Ok(())
}
