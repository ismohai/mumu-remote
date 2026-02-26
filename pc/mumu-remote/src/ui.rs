use std::time::{Duration, Instant};

use eframe::egui;
use image::Luma;
use log::{error, info, warn};
use qrcode::QrCode;

use crate::input::InputController;
use crate::pairing::{
    default_store_path, detect_local_ip, encode_pairing_info, load_store, save_store, PairedDevice,
    PairingInfo, PairingStore,
};
use crate::pairing_service::{
    DiscoveredDevice, IncomingPairRequest, PairingEvent, PairingService, DEFAULT_CONTROL_PORT,
};
use crate::stream::StreamController;

fn load_windows_cjk_font() -> Option<Vec<u8>> {
    let candidates = [
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simsun.ttc",
    ];

    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            return Some(bytes);
        }
    }

    None
}

fn setup_cjk_font(ctx: &egui::Context) {
    if let Some(bytes) = load_windows_cjk_font() {
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .font_data
            .insert("windows_cjk".to_string(), egui::FontData::from_owned(bytes));
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "windows_cjk".to_string());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push("windows_cjk".to_string());
        ctx.set_fonts(fonts);
        info!("loaded Windows CJK font for UI");
    } else {
        warn!("no Windows CJK font found, Chinese text may fallback incorrectly");
    }
}

pub fn run() {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([880.0, 680.0]),
        ..Default::default()
    };

    let run_result = eframe::run_native(
        "MuMu Remote",
        native_options,
        Box::new(|cc| {
            setup_cjk_font(&cc.egui_ctx);
            Box::<MumuRemoteApp>::default()
        }),
    );

    if let Err(err) = run_result {
        error!("failed to open GUI: {err}");
    }
}

struct UiDiscoveredDevice {
    device: DiscoveredDevice,
    last_seen: Instant,
}

#[derive(Default)]
struct MumuRemoteApp {
    pairing_service: Option<PairingService>,
    control_enabled: bool,
    controller: Option<StreamController>,
    input_controller: Option<InputController>,
    status: String,
    loaded: bool,

    store: PairingStore,
    selected_device_id: Option<String>,

    discovered: Vec<UiDiscoveredDevice>,
    pending_incoming: Option<IncomingPairRequest>,

    pairing_payload: String,
    active_pair_token: Option<String>,
    qr_texture: Option<egui::TextureHandle>,
}

impl MumuRemoteApp {
    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }

        self.store = load_store(&default_store_path());
        self.selected_device_id = self.store.devices.first().map(|it| it.device_id.clone());
        self.status = "等待操作".to_string();

        match PairingService::start() {
            Ok(service) => {
                self.pairing_service = Some(service);
            }
            Err(err) => {
                self.status = format!("配对服务启动失败: {err}");
            }
        }

        self.loaded = true;
    }

    fn save_pairings(&mut self) {
        let path = default_store_path();
        match save_store(&path, &self.store) {
            Ok(_) => {}
            Err(err) => {
                self.status = format!("保存配对列表失败: {err}");
            }
        }
    }

    fn selected_device(&self) -> Option<&PairedDevice> {
        if let Some(id) = &self.selected_device_id {
            if let Some(device) = self.store.devices.iter().find(|it| &it.device_id == id) {
                return Some(device);
            }
        }
        self.store.devices.first()
    }

    fn selected_device_clone(&self) -> Option<PairedDevice> {
        self.selected_device().cloned()
    }

    fn current_ports_for_control(&self) -> (u16, u16) {
        if let Some(device) = self.selected_device() {
            let video = if device.port == 0 { 5000 } else { device.port };
            let control = if device.control_port == 0 {
                DEFAULT_CONTROL_PORT
            } else {
                device.control_port
            };
            (video, control)
        } else {
            (5000, DEFAULT_CONTROL_PORT)
        }
    }

    fn start_input_listener(&mut self, control_port: u16) -> bool {
        let input_listen_addr = format!("0.0.0.0:{control_port}");
        match InputController::start(input_listen_addr.clone()) {
            Ok(input_controller) => {
                self.input_controller = Some(input_controller);
                true
            }
            Err(err) => {
                self.status = format!("控制输入监听失败: {err}");
                false
            }
        }
    }

    fn try_start_stream_to_device(&mut self, device: PairedDevice) {
        if self.controller.is_some() {
            return;
        }

        let remote_addr = format!("{}:{}", device.ip, device.port);
        match StreamController::start(remote_addr.clone()) {
            Ok(controller) => {
                self.controller = Some(controller);
                self.status = format!("控制已启动，目标 {}", remote_addr);
            }
            Err(err) => {
                self.status = format!("推流启动失败: {err}");
            }
        }
    }

    fn make_runtime_qr(
        &mut self,
        ctx: &egui::Context,
        target_video_port: u16,
        target_control_port: u16,
    ) {
        let ip = detect_local_ip();
        let info = PairingInfo {
            ip,
            port: target_video_port,
            control_port: target_control_port,
            pair_port: crate::pairing_service::DEFAULT_PAIR_PORT,
            token: crate::pairing::generate_token(),
        };
        self.active_pair_token = Some(info.token.clone());
        self.pairing_payload = encode_pairing_info(&info);
        self.rebuild_qr_texture(ctx);
    }

    fn rebuild_qr_texture(&mut self, ctx: &egui::Context) {
        if self.pairing_payload.trim().is_empty() {
            self.qr_texture = None;
            return;
        }

        let code = match QrCode::new(self.pairing_payload.as_bytes()) {
            Ok(code) => code,
            Err(err) => {
                self.status = format!("二维码生成失败: {err}");
                self.qr_texture = None;
                return;
            }
        };

        let qr = code.render::<Luma<u8>>().build();
        let size = [qr.width() as usize, qr.height() as usize];
        let pixels = qr
            .into_raw()
            .into_iter()
            .map(|value| {
                if value == 0 {
                    egui::Color32::BLACK
                } else {
                    egui::Color32::WHITE
                }
            })
            .collect::<Vec<_>>();

        let image = egui::ColorImage { size, pixels };
        self.qr_texture =
            Some(ctx.load_texture("pairing_qr_texture", image, egui::TextureOptions::NEAREST));
    }

    fn toggle_control(&mut self, ctx: &egui::Context) {
        if self.control_enabled {
            if let Some(controller) = self.controller.as_mut() {
                controller.stop();
            }
            self.controller = None;

            if let Some(input_controller) = self.input_controller.as_mut() {
                input_controller.stop();
            }
            self.input_controller = None;

            self.control_enabled = false;
            self.pairing_payload.clear();
            self.active_pair_token = None;
            self.qr_texture = None;
            self.status = "控制已关闭".to_string();
            return;
        }

        self.control_enabled = true;

        let (video_port, control_port) = self.current_ports_for_control();
        if !self.start_input_listener(control_port) {
            self.control_enabled = false;
            return;
        }

        self.make_runtime_qr(ctx, video_port, control_port);

        if let Some(device) = self.selected_device_clone() {
            self.try_start_stream_to_device(device);
        } else {
            self.status = "控制已启动，等待扫码配对或电脑发起配对".to_string();
        }
    }

    fn upsert_discovered(&mut self, device: DiscoveredDevice) {
        if device.from != "phone" {
            return;
        }

        if let Some(existing) = self
            .discovered
            .iter_mut()
            .find(|it| it.device.device_id == device.device_id)
        {
            existing.device = device;
            existing.last_seen = Instant::now();
            return;
        }

        self.discovered.push(UiDiscoveredDevice {
            device,
            last_seen: Instant::now(),
        });
    }

    fn upsert_paired_device(
        &mut self,
        device_id: String,
        device_name: String,
        ip: String,
        video_port: u16,
        control_port: u16,
    ) {
        if let Some(existing) = self
            .store
            .devices
            .iter_mut()
            .find(|it| it.device_id == device_id)
        {
            existing.name = device_name;
            existing.ip = ip;
            existing.port = video_port;
            existing.control_port = control_port;
        } else {
            self.store.devices.push(PairedDevice {
                name: device_name,
                ip,
                port: video_port,
                control_port,
                device_id: device_id.clone(),
            });
        }

        if self.selected_device_id.is_none() {
            self.selected_device_id = Some(device_id);
        }
        self.save_pairings();
    }

    fn poll_pairing_events(&mut self) {
        let events = if let Some(service) = &self.pairing_service {
            service.poll_events()
        } else {
            Vec::new()
        };

        let mut deferred_replies: Vec<(IncomingPairRequest, bool)> = Vec::new();

        for event in events {
            match event {
                PairingEvent::Discovered(device) => {
                    self.upsert_discovered(device);
                }
                PairingEvent::IncomingRequest(request) => {
                    let token_ok = match (&self.active_pair_token, &request.token) {
                        (Some(expected), Some(actual)) => expected == actual,
                        _ => false,
                    };

                    if token_ok {
                        self.pending_incoming = Some(request);
                    } else {
                        self.status = "收到无效扫码配对请求，已拒绝".to_string();
                        deferred_replies.push((request, false));
                    }
                }
                PairingEvent::PairResponse(response) => {
                    if response.accepted {
                        let paired_id = response.device_id.clone();
                        self.upsert_paired_device(
                            response.device_id,
                            response.device_name,
                            response.addr.ip().to_string(),
                            response.video_port,
                            response.control_port,
                        );
                        self.selected_device_id = Some(paired_id.clone());
                        self.status = format!("设备配对成功，请求ID {}", response.request_id);

                        if self.control_enabled && self.controller.is_none() {
                            if let Some(device) = self
                                .store
                                .devices
                                .iter()
                                .find(|it| it.device_id == paired_id)
                                .cloned()
                            {
                                self.try_start_stream_to_device(device);
                            }
                        }
                    } else {
                        self.status = "对方拒绝了配对请求".to_string();
                    }
                }
                PairingEvent::Error(message) => {
                    self.status = message;
                }
            }
        }

        if let Some(service) = &self.pairing_service {
            for (request, accepted) in deferred_replies {
                service.reply_incoming(&request, accepted);
            }
        }

        self.discovered
            .retain(|it| it.last_seen.elapsed() <= Duration::from_secs(15));
    }
}

impl eframe::App for MumuRemoteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_loaded();
        self.poll_pairing_events();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("MuMu Remote 控制中心");
            ui.separator();

            let running = self.control_enabled;
            let button_text = if running {
                "关闭控制"
            } else {
                "启动控制"
            };
            if ui
                .add_sized([160.0, 40.0], egui::Button::new(button_text))
                .clicked()
            {
                self.toggle_control(ctx);
            }

            ui.add_space(8.0);
            ui.label(format!("状态: {}", self.status));

            if running {
                ui.separator();
                ui.label("扫码配对二维码（控制开启后显示）");
                if let Some(texture) = &self.qr_texture {
                    ui.image((texture.id(), egui::Vec2::new(240.0, 240.0)));
                }
            }

            ui.separator();
            ui.heading("配对列表");
            let mut remove_index: Option<usize> = None;
            for (idx, device) in self.store.devices.iter().enumerate() {
                ui.horizontal(|ui| {
                    let selected = self
                        .selected_device_id
                        .as_ref()
                        .map(|id| id == &device.device_id)
                        .unwrap_or(false);
                    if ui.radio(selected, "").clicked() {
                        self.selected_device_id = Some(device.device_id.clone());
                    }
                    ui.label(format!("{}  {}:{}", device.name, device.ip, device.port));
                    if ui.button("删除").clicked() {
                        remove_index = Some(idx);
                    }
                });
            }
            if let Some(idx) = remove_index {
                self.store.devices.remove(idx);
                if self
                    .selected_device_id
                    .as_ref()
                    .map(|id| !self.store.devices.iter().any(|d| &d.device_id == id))
                    .unwrap_or(false)
                {
                    self.selected_device_id =
                        self.store.devices.first().map(|d| d.device_id.clone());
                }
                self.save_pairings();
            }

            ui.separator();
            ui.heading("可配对设备（自动搜索）");
            if self.discovered.is_empty() {
                ui.label("正在自动搜索局域网可配对手机...");
            }
            for discovered in &self.discovered {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{}  {}  视频:{} 控制:{}",
                        discovered.device.device_name,
                        discovered.device.ip,
                        discovered.device.video_port,
                        discovered.device.control_port
                    ));
                    if ui.button("发起配对").clicked() {
                        if let Some(service) = &self.pairing_service {
                            service.send_pair_request(discovered.device.ip.clone());
                            self.status = "已发送配对请求，等待手机确认".to_string();
                        }
                    }
                });
            }
        });

        if let Some(request) = self.pending_incoming.clone() {
            egui::Window::new("配对确认")
                .resizable(false)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(format!("设备: {}", request.device_name));
                    ui.label(format!("IP: {}", request.addr.ip()));
                    ui.label("是否允许该手机完成配对？");

                    ui.horizontal(|ui| {
                        if ui.button("拒绝").clicked() {
                            if let Some(service) = &self.pairing_service {
                                service.reply_incoming(&request, false);
                            }
                            self.pending_incoming = None;
                            self.status = "已拒绝扫码配对请求".to_string();
                        }

                        if ui.button("允许").clicked() {
                            if let Some(service) = &self.pairing_service {
                                service.reply_incoming(&request, true);
                            }
                            let paired_id = request.device_id.clone();
                            self.upsert_paired_device(
                                request.device_id,
                                request.device_name,
                                request.addr.ip().to_string(),
                                request.video_port,
                                request.control_port,
                            );
                            self.selected_device_id = Some(paired_id.clone());

                            if self.control_enabled && self.controller.is_none() {
                                if let Some(device) = self
                                    .store
                                    .devices
                                    .iter()
                                    .find(|it| it.device_id == paired_id)
                                    .cloned()
                                {
                                    self.try_start_stream_to_device(device);
                                }
                            }

                            self.pending_incoming = None;
                            self.status = "扫码配对成功".to_string();
                        }
                    });
                });
        }
    }
}

impl Drop for MumuRemoteApp {
    fn drop(&mut self) {
        if let Some(controller) = self.controller.as_mut() {
            controller.stop();
        }
        if let Some(input_controller) = self.input_controller.as_mut() {
            input_controller.stop();
        }
        self.controller = None;
        self.input_controller = None;
        self.pairing_service = None;
    }
}
