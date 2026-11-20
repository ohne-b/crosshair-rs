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
    pub configure_startup: Option<bool>,
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
                "--configure-startup" => {
                    options.configure_startup = Some(
                        arguments
                            .next()
                            .context("Missing startup setting")?
                            .parse()
                            .context("Startup must be true or false")?,
                    );
                }
                "--startup" => options.startup = true,
                "--resume-update" => {} // Normal startup recovers the durable update journal.
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
        ensure!(
            options.configure_startup.is_none() || options.command.is_none(),
            "Startup configuration cannot include another command"
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

fn with_startup_task<T>(operation: impl FnOnce(Option<StartupTask>) -> Result<T>) -> Result<T> {
    let identity = Identity::current()?;
    if !startup_package(identity.package.as_deref()) {
        return operation(None);
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
    operation(task)
}

pub fn read_startup() -> Result<bool> {
    with_startup_task(|task| task.map_or(Ok(false), |task| Ok(startup_enabled(task.State()?))))
}

pub fn with_startup<T>(enabled: bool, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    if !startup_package(Identity::current()?.package.as_deref()) {
        ensure!(
            !enabled,
            "Launch at sign-in is available in the installed app. Open crosshair-rs from Start."
        );
        return operation();
    }
    with_startup_task(|task| {
        let Some(task) = task else {
            ensure!(
                !enabled,
                "The installed Windows startup task is missing. Reinstall crosshair-rs with its setup."
            );
            return operation();
        };
        let was_enabled = startup_enabled(task.State()?);
        if enabled != was_enabled
            && let Err(error) = set_startup(&task, enabled)
        {
            if task
                .State()
                .is_ok_and(|state| startup_enabled(state) == was_enabled)
            {
                return Err(error);
            }
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
    })
}

pub fn configure_startup(options: &Options) -> Result<()> {
    let enabled = options
        .configure_startup
        .context("Missing startup setting")?;
    let result = (|| {
        let identity = Identity::current()?;
        ensure!(
            startup_package(identity.package.as_deref()),
            "Launch at sign-in is available in the installed app. Open crosshair-rs from Start."
        );
        if let Ok(pipe) = Pipe::connect(
            &control_pipe_name(&options.scope)?,
            &identity,
            &["crosshair-engine.exe"],
        ) {
            pipe.write_message(&Request::new(1, RequestCommand::SetStartup { enabled }))?;
            loop {
                if let ServerMessage::Response(response) = pipe.read_message()? {
                    if let Err(error) = response.result {
                        bail!("{}", error.message);
                    }
                    return Ok(());
                }
            }
        }
        let root = options
            .test_data
            .clone()
            .map_or_else(crate::storage::default_data_directory, Ok)?;
        let store = crate::storage::Store::open(root)?;
        let mut state = store.load()?;
        if state.settings.launch_at_login != enabled {
            state.settings.launch_at_login = enabled;
            state.revision = state
                .revision
                .checked_add(1)
                .context("Settings revision exhausted")?;
        }
        with_startup(enabled, || store.save(&state))
    })();
    if let Some(path) = &options.reply_file {
        let reply = match &result {
            Ok(()) => "ok".into(),
            Err(error) => format!("error\n{error:#}"),
        };
        let pending = path.with_extension("pending");
        std::fs::write(&pending, reply)?;
        std::fs::rename(pending, path)?;
    }
    result
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
    if enabled {
        match task.State()? {
            StartupTaskState::DisabledByUser => bail!(
                "Launch at sign-in is disabled in Windows. Enable crosshair-rs in Windows Settings > Apps > Startup."
            ),
            StartupTaskState::DisabledByPolicy => {
                bail!("Launch at sign-in is disabled by Windows policy.")
            }
            _ => {}
        }
    }
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
    fn installer_startup_options_are_explicit_and_do_not_accept_another_command() {
        let parse = |values: &[&str]| {
            super::Options::parse(&values.iter().map(|v| v.to_string()).collect::<Vec<_>>())
        };
        assert_eq!(
            parse(&["--configure-startup", "true"])
                .unwrap()
                .configure_startup,
            Some(true)
        );
        assert_eq!(
            parse(&["--configure-startup", "false"])
                .unwrap()
                .configure_startup,
            Some(false)
        );
        assert!(parse(&["--configure-startup"]).is_err());
        assert!(parse(&["--configure-startup", "yes"]).is_err());
        assert!(
            parse(&[
                "--configure-startup",
                "true",
                "--command",
                "{\"type\":\"subscribe\"}"
            ])
            .is_err()
        );
        assert_eq!(parse(&[]).unwrap().configure_startup, None);
    }

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
