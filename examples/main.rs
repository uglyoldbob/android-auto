//! The main example for this library
use bluetooth_rust::{BluetoothAdapterTrait, MessageToBluetoothHost};
use rustls::crypto::aws_lc_rs::sign::any_ecdsa_type;
use std::sync::Arc;
use tokio::sync::Mutex;

use android_auto::{HeadUnitInfo, VideoConfiguration};
use eframe::egui;

struct AndroidAutoInner {
    connected: bool,
    recv: tokio::sync::mpsc::Receiver<MessageToAsync>,
    send: tokio::sync::mpsc::Sender<MessageFromAsync>,
    blue_recv: tokio::sync::mpsc::Receiver<MessageToBluetoothHost>,
}

struct AndroidAuto {
    inner: Arc<Mutex<AndroidAutoInner>>,
    config: VideoConfiguration,
    blue: android_auto::BluetoothInformation,
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
        let mut i = self.inner.lock().await;
        i.send
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
        None
    }

    fn supports_video(&self) -> Option<&dyn android_auto::AndroidAutoVideoChannelTrait> {
        Some(self)
    }

    fn supports_bluetooth(&self) -> Option<&dyn android_auto::AndroidAutoBluetoothTrait> {
        Some(self)
    }
}

impl AndroidAuto {
    fn new(
        recv: tokio::sync::mpsc::Receiver<MessageToAsync>,
        send: tokio::sync::mpsc::Sender<MessageFromAsync>,
        blue_recv: tokio::sync::mpsc::Receiver<MessageToBluetoothHost>,
        blue_address: String,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(AndroidAutoInner {
                connected: false,
                send,
                recv,
                blue_recv,
            })),
            blue: android_auto::BluetoothInformation {
                address: blue_address,
            },
            config: VideoConfiguration {
                resolution: android_auto::Wifi::video_resolution::Enum::_720p,
                fps: android_auto::Wifi::video_fps::Enum::_30,
                dpi: 300,
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
    let native_options = eframe::NativeOptions::default();

    let runtime_builder = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to build the Tokio runtime");

    let to_async = tokio::sync::mpsc::channel(50);
    let from_async = tokio::sync::mpsc::channel(50);

    let _thread_handle = std::thread::spawn(move || {
        // 3. Enter the runtime context and run async code within this specific thread
        runtime_builder.block_on(async {
            println!(
                "Tokio runtime is running in a new thread: {:?}",
                std::thread::current().name()
            );

            let (bluechan, bluetooth) = {
                let bluechan = tokio::sync::mpsc::channel(5);
                let mut bluetooth = bluetooth_rust::BluetoothAdapterBuilder::new();
                bluetooth.with_sender(bluechan.0);
                let bluetooth =
                    Arc::new(bluetooth.build().await.expect("Could not open bluetooth"));
                (bluechan.1, bluetooth)
            };

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

            let aa = AndroidAuto::new(to_async.1, from_async.0, bluechan, bluetooth_address);
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
