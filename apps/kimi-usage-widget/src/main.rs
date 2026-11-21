#![windows_subsystem = "windows"]

mod api;
mod auth;
mod config;
mod error;
mod provider;

fn main() {
    std::process::exit(widget_common::run_widget(
        Box::new(provider::KimiProvider::new()),
        &config::WIDGET_CONFIG,
    ));
}
