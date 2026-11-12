use std::{
    io::Write,
    ptr,
    sync::{
        Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use windows::{
    Win32::Networking::WinHttp::*,
    core::{PCWSTR, w},
};

use super::manifest::request_target;

#[derive(Default)]
pub struct Cancellation {
    cancelled: AtomicBool,
    timed_out: AtomicBool,
    request: Mutex<Option<usize>>,
}

impl Cancellation {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.abort_request();
    }
    fn abort_request(&self) {
        if let Some(request) = self.request.lock().unwrap().take() {
            // Closing an outstanding WinHTTP request interrupts its network I/O.
            unsafe {
                let _ = WinHttpCloseHandle(request as _);
            }
        }
    }
    pub fn check(&self) -> Result<()> {
        ensure!(!self.is_cancelled(), "Update cancelled");
        ensure!(
            !self.timed_out.load(Ordering::Acquire),
            "Update request timed out"
        );
        Ok(())
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

struct Handle(*mut std::ffi::c_void);
impl Handle {
    fn new(value: *mut std::ffi::c_void) -> Result<Self> {
        if value.is_null() {
            Err(windows::core::Error::from_thread().into())
        } else {
            Ok(Self(value))
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

struct Request<'a> {
    value: usize,
    cancellation: &'a Cancellation,
}
impl<'a> Request<'a> {
    fn new(value: *mut std::ffi::c_void, cancellation: &'a Cancellation) -> Result<Self> {
        let handle = Handle::new(value)?;
        let mut active = cancellation.request.lock().unwrap();
        cancellation.check()?;
        *active = Some(value as usize);
        std::mem::forget(handle);
        Ok(Self {
            value: value as usize,
            cancellation,
        })
    }
    fn raw(&self) -> *mut std::ffi::c_void {
        self.value as _
    }
}
impl Drop for Request<'_> {
    fn drop(&mut self) {
        let mut active = self.cancellation.request.lock().unwrap();
        if *active == Some(self.value) {
            *active = None;
            unsafe {
                let _ = WinHttpCloseHandle(self.raw());
            }
        }
    }
}

pub fn download(
    initial: &str,
    writer: &mut impl Write,
    limit: u64,
    deadline: Duration,
    cancellation: &Cancellation,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<u64> {
    // WinHTTP receive timeouts alone do not impose a total slow-response limit.
    // This sleeping watchdog exists only during a requested network operation.
    let finished = (Mutex::new(false), Condvar::new());
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("update-deadline".into())
            .spawn_scoped(scope, || {
                let done = finished.0.lock().unwrap();
                let (done, _) = finished
                    .1
                    .wait_timeout_while(done, deadline, |done| !*done)
                    .unwrap();
                if !*done {
                    cancellation.timed_out.store(true, Ordering::Release);
                    cancellation.abort_request();
                }
            })?;
        let _finished = Finish(&finished);
        let result = download_request(
            initial,
            writer,
            limit,
            deadline,
            cancellation,
            &mut progress,
        );
        cancellation.check()?;
        result
    })
}

struct Finish<'a>(&'a (Mutex<bool>, Condvar));
impl Drop for Finish<'_> {
    fn drop(&mut self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_one();
    }
}

fn download_request(
    initial: &str,
    writer: &mut impl Write,
    limit: u64,
    deadline: Duration,
    cancellation: &Cancellation,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<u64> {
    let started = Instant::now();
    unsafe {
        let session = Handle::new(WinHttpOpen(
            w!("crosshair-rs updater/1"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))?;
        WinHttpSetTimeouts(session.0, 5000, 5000, 5000, 5000)?;
        WinHttpSetOption(
            Some(session.0),
            WINHTTP_OPTION_SECURE_PROTOCOLS,
            Some(&WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2.to_ne_bytes()),
        )?;
        let mut url = initial.to_owned();
        for redirects in 0..=3 {
            cancellation.check()?;
            ensure!(started.elapsed() < deadline, "Update request timed out");
            let (host, path) = request_target(&url, initial, redirects > 0)?;
            let host = wide(host);
            let path = wide(&path);
            let connection = Handle::new(WinHttpConnect(session.0, PCWSTR(host.as_ptr()), 443, 0))?;
            let request = Request::new(
                WinHttpOpenRequest(
                    connection.0,
                    w!("GET"),
                    PCWSTR(path.as_ptr()),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    ptr::null(),
                    WINHTTP_FLAG_SECURE,
                ),
                cancellation,
            )?;
            let budget =
                (deadline.saturating_sub(started.elapsed()).as_millis() / 4).clamp(1, 5000) as i32;
            WinHttpSetTimeouts(request.raw(), budget, budget, budget, budget)?;
            // Redirects and ambient credentials are never followed automatically.
            let redirect_policy = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
            WinHttpSetOption(
                Some(request.raw()),
                WINHTTP_OPTION_REDIRECT_POLICY,
                Some(&redirect_policy.to_ne_bytes()),
            )?;
            let disabled = WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION;
            WinHttpSetOption(
                Some(request.raw()),
                WINHTTP_OPTION_DISABLE_FEATURE,
                Some(&disabled.to_ne_bytes()),
            )?;
            WinHttpSendRequest(request.raw(), None, None, 0, 0, 0)?;
            WinHttpReceiveResponse(request.raw(), ptr::null_mut())?;
            let status = header(&request, WINHTTP_QUERY_STATUS_CODE)?
                .context("Missing HTTP status")?
                .parse::<u32>()?;
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                ensure!(redirects < 3, "Too many update redirects");
                url = header(&request, WINHTTP_QUERY_LOCATION)?
                    .context("Update redirect has no destination")?;
                request_target(&url, initial, true)?;
                continue;
            }
            ensure!(status == 200, "Update server returned HTTP {status}");
            ensure!(
                header(&request, WINHTTP_QUERY_CONTENT_ENCODING)?
                    .is_none_or(|value| value.eq_ignore_ascii_case("identity")),
                "Compressed update responses are not accepted"
            );
            let total = header(&request, WINHTTP_QUERY_CONTENT_LENGTH)?
                .map(|value| value.parse::<u64>())
                .transpose()
                .context("Invalid update size")?;
            ensure!(
                total.is_none_or(|size| size > 0 && size <= limit),
                "Update response exceeds its size limit"
            );
            let mut received = 0_u64;
            let mut buffer = [0; 64 * 1024];
            progress(0, total);
            loop {
                cancellation.check()?;
                ensure!(started.elapsed() < deadline, "Update request timed out");
                let timeout = deadline
                    .saturating_sub(started.elapsed())
                    .as_millis()
                    .clamp(1, 5000) as u32;
                WinHttpSetOption(
                    Some(request.raw()),
                    WINHTTP_OPTION_RECEIVE_TIMEOUT,
                    Some(&timeout.to_ne_bytes()),
                )?;
                let mut count = 0;
                WinHttpReadData(
                    request.raw(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut count,
                )?;
                if count == 0 {
                    break;
                }
                received = received
                    .checked_add(count as u64)
                    .context("Update size overflow")?;
                ensure!(
                    received <= limit && total.is_none_or(|size| received <= size),
                    "Update response exceeds its declared size or limit"
                );
                writer.write_all(&buffer[..count as usize]).context(
                    "Could not save the update; check free disk space and folder access",
                )?;
                progress(received, total);
            }
            cancellation.check()?;
            ensure!(
                received > 0 && total.is_none_or(|size| size == received),
                "Update download is truncated or empty"
            );
            return Ok(received);
        }
    }
    anyhow::bail!("Too many update redirects")
}

unsafe fn header(request: &Request<'_>, name: u32) -> Result<Option<String>> {
    unsafe {
        let mut count = 0;
        if let Err(error) = WinHttpQueryHeaders(
            request.raw(),
            name,
            PCWSTR::null(),
            None,
            &mut count,
            ptr::null_mut(),
        ) {
            if error.code().0 as u32 == 0x8007_2F76 {
                return Ok(None);
            } // ERROR_WINHTTP_HEADER_NOT_FOUND
            ensure!(count > 0, "Could not read update response headers: {error}");
        }
        ensure!(
            count > 0 && count <= 32 * 1024 && count % 2 == 0,
            "Update response header exceeds its limit"
        );
        let mut buffer = vec![0_u16; count as usize / 2];
        WinHttpQueryHeaders(
            request.raw(),
            name,
            PCWSTR::null(),
            Some(buffer.as_mut_ptr().cast()),
            &mut count,
            ptr::null_mut(),
        )?;
        let length = buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len());
        Ok(Some(String::from_utf16(&buffer[..length])?))
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
