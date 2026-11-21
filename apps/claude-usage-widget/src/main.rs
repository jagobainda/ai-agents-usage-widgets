#![windows_subsystem = "windows"]

mod api;
mod auth;
mod config;
mod http;
mod keyring;
mod provider;

fn main() {
    std::process::exit(widget_common::run_widget(
        Box::new(provider::ClaudeProvider::new()),
        &config::WIDGET_CONFIG,
    ));
}
