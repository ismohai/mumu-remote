use log::info;

mod adb;
mod capture;
mod encoder;
mod input;
mod mumu;
mod net;
mod pairing;
mod pairing_service;
mod runtime_config;
mod stream;
mod ui;

fn main() {
    env_logger::init();
    info!("mumu-remote starting");
    ui::run();
}
