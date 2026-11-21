#![windows_subsystem = "windows"]

mod app_server;
mod config;
mod process;
mod provider;
mod usage;
mod value_text;

fn main() {
    std::process::exit(widget_common::run_widget(
        Box::new(provider::CodexProvider::new()),
        &config::WIDGET_CONFIG,
    ));
}
