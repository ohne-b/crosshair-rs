#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod overlay;
#[cfg(windows)]
mod probe;
#[cfg(windows)]
mod runtime;
#[cfg(windows)]
mod storage;
#[cfg(all(windows, test))]
mod tests;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod updates;
#[cfg(windows)]
mod work;

fn main() -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        let arguments: Vec<String> = std::env::args().skip(1).collect();
        if arguments
            .first()
            .is_some_and(|argument| argument == "--build-info")
        {
            println!("{}", updates::build_configuration());
            return Ok(());
        }
        if arguments
            .first()
            .is_some_and(|argument| argument == "--update-helper")
        {
            return updates::run_helper(&arguments[1..]);
        }
        if arguments
            .first()
            .is_some_and(|argument| argument == "--overlay-probe")
        {
            return probe::run(&arguments[1..]);
        }
        runtime::run(lifecycle::Options::parse(&arguments)?)
    }
    #[cfg(not(windows))]
    anyhow::bail!("crosshair-rs requires Windows 11");
}
#[cfg(windows)]
mod app;
#[cfg(windows)]
mod commands;
#[cfg(windows)]
mod discord;
#[cfg(windows)]
mod input;
#[cfg(windows)]
mod ipc;
#[cfg(windows)]
mod lifecycle;
