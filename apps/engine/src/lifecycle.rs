use std::{os::windows::process::CommandExt, path::PathBuf, process::Command};

use anyhow::{Context, Result, bail, ensure};
use crosshair_core::protocol::{Command as RequestCommand, Reply, Request, ServerMessage};
use crosshair_platform::{
    identity::{Identity, control_pipe_name, wide},
    pipe::Pipe,
};
use windows::{
    ApplicationModel::{StartupTask, StartupTaskState},
    Win32::{
        Foundation::HWND,
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    },
    core::{PCWSTR, w},
};

#[derive(Default)]
pub struct Options {
    pub scope: String,
    pub test_data: Option<PathBuf>,
    pub no_input: bool,
    pub startup: bool,
    pub restore_visibility: bool,
    pub command: Option<RequestCommand>,
    pub reply_file: Option<PathBuf>,
}

impl Options {
    pub fn parse(arguments: &[String]) -> Result<Self> {
        let mut options = Self {
            scope: "app".into(),
            ..Default::default()
        };
        let mut arguments = arguments.iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--ipc-scope" => {
                    options.scope = arguments.next().context("Missing IPC scope")?.clone()
                }
                "--test-data-dir" => {
                    options.test_data =
                        Some(arguments.next().context("Missing test directory")?.into())
                }
                "--no-input" => options.no_input = true,
                "--startup" => options.startup = true,
                "--restore-visibility" => options.restore_visibility = true,
                "--command" => {
                    options.command = Some(serde_json::from_str(
                        arguments.next().context("Missing command JSON")?,
                    )?)
                }
                "--reply-file" => {
                    options.reply_file =
                        Some(arguments.next().context("Missing reply path")?.into())
                }
                _ => bail!("Unknown engine argument: {argument}"),
            }
        }
        ensure!(
            options.scope != "app" || options.test_data.is_none(),
            "Test storage requires an isolated IPC scope"
        );
        ensure!(
            !options.no_input || options.test_data.is_some(),
            "Disabled input requires isolated test storage"
        );
        control_pipe_name(&options.scope)?;
        Ok(options)
    }

    pub fn existing(&self, identity: &Identity) -> Result<bool> {
        let pipe = Pipe::connect(
            &control_pipe_name(&self.scope)?,
            identity,
            &["crosshair-engine.exe"],
        );
        if self.command.is_none() {
            return Ok(pipe.is_ok());
        }
        let pipe = pipe.context("The engine is not running or could not authenticate")?;
        pipe.write_message(&Request::new(1, self.command.clone().unwrap()))?;
        loop {
            if let ServerMessage::Response(response) = pipe.read_message()? {
                let json = serde_json::to_string_pretty(&response)?;
                if let Some(path) = &self.reply_file {
                    std::fs::write(path, &json)?;
                } else {
                    println!("{json}");
                }
                if let Err(error) = response.result {
                    bail!("{}", error.message);
                }
                return Ok(true);
            }
        }
    }

    pub fn open_editor(&self) -> Result<()> {
        let executable = std::env::current_exe()?.with_file_name("crosshair-editor.exe");
        ensure!(executable.is_file(), "The editor executable is missing");
        let mut process = Command::new(executable);
        process.creation_flags(0x0800_0000);
        if self.scope != "app" {
            process.args(["--ipc-scope", &self.scope]);
        }
        if let Some(path) = &self.test_data {
            process.arg("--test-data-dir").arg(path);
        }
        if self.no_input {
            process.arg("--no-input");
        }
        process.spawn().context("The editor could not start")?;
        Ok(())
    }
}

pub fn with_startup<T>(enabled: bool, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    let identity = Identity::current()?;
    if !startup_package(identity.package.as_deref()) {
        ensure!(
            !enabled,
            "Launch at sign-in requires the installed application"
        );
        return operation();
    }
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    }
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    let _apartment = Apartment;
    // The engine can inherit the Editor application ID; search the whole package.
    let tasks = StartupTask::GetForCurrentPackageAsync()?
        .join()
        .context("Windows could not read launch-at-sign-in settings")?;
    let mut task = None;
    for candidate in tasks {
        if candidate.TaskId()? == "CrosshairEngineStartup" {
            task = Some(candidate);
            break;
        }
    }
    let Some(task) = task else {
        ensure!(
            !enabled,
            "Reinstall crosshair-rs with its setup to enable launch at sign-in"
        );
        return operation();
    };
    let was_enabled = startup_enabled(task.State()?);
    if enabled != was_enabled
        && let Err(error) = set_startup(&task, enabled)
    {
        return match set_startup(&task, was_enabled) {
            Ok(()) => Err(error),
            Err(rollback) => Err(error.context(format!(
                "Windows startup could not be restored after this failure: {rollback:#}"
            ))),
        };
    }
    match operation() {
        Ok(value) => Ok(value),
        Err(error) => {
            if enabled != was_enabled
                && let Err(rollback) = set_startup(&task, was_enabled)
            {
                return Err(error.context(format!(
                    "Windows startup could not be restored after this failure: {rollback:#}"
                )));
            }
            Err(error)
        }
    }
}

fn startup_package(full_name: Option<&str>) -> bool {
    matches!(
        full_name
            .and_then(|name| name.split_once('_'))
            .map(|(name, _)| name),
        Some("OhneB.crosshair-rs" | "OhneB.crosshair-rs.Preview")
    )
}

fn startup_enabled(state: StartupTaskState) -> bool {
    state == StartupTaskState::Enabled || state == StartupTaskState::EnabledByPolicy
}

fn set_startup(task: &StartupTask, enabled: bool) -> Result<()> {
    let state = if enabled {
        task.RequestEnableAsync()?.join()?
    } else {
        task.Disable()?;
        task.State()?
    };
    ensure!(
        startup_enabled(state) == enabled,
        "Windows controls startup for crosshair-rs. Check Windows Settings > Apps > Startup"
    );
    Ok(())
}

pub fn open(target: &str, hwnd: HWND) -> Result<Reply> {
    ensure!(!target.contains('\0'), "Invalid shell target");
    let target = wide(target);
    let result = unsafe {
        ShellExecuteW(
            Some(hwnd),
            w!("open"),
            PCWSTR(target.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    ensure!(
        result.0 as isize > 32,
        "Windows could not open this location"
    );
    Ok(Reply::Ack {
        document_revision: None,
    })
}

#[cfg(test)]
mod startup_tests {
    #[test]
    fn startup_requires_our_package_not_an_inherited_unrelated_identity() {
        for name in [
            None,
            Some("Other.App_1.0.0.0_x64__publisher"),
            Some("OhneB.crosshair-rs.fake_1.0.0.0_x64__publisher"),
        ] {
            assert!(!super::startup_package(name));
        }
        for name in [
            "OhneB.crosshair-rs_0.1.0.6_x64__publisher",
            "OhneB.crosshair-rs.Preview_0.1.0.6_x64__publisher",
        ] {
            assert!(super::startup_package(Some(name)));
        }
    }
}

