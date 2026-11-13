use super::*;
use crosshair_core::Id;
use crosshair_platform::identity::{owned, raw, wide};
use std::{
    fs::File,
    io::{Read, Write},
    sync::mpsc,
};
use windows::{
    Win32::{
        Foundation::ERROR_PIPE_CONNECTED,
        Storage::FileSystem::PIPE_ACCESS_DUPLEX,
        System::Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
        },
    },
    core::PCWSTR,
};

fn fixture(test: impl FnOnce(File) + Send + 'static) -> (Pipe, JoinHandle<()>) {
    let name = format!(r"\\.\pipe\crosshair-rs.discord-test-{}", Id::new_v4());
    let path = wide(&name);
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(path.as_ptr()),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            1,
            65536,
            65536,
            0,
            None,
        )
    };
    assert!(!handle.is_invalid());
    let handle = unsafe { owned(handle) };
    let server = thread::spawn(move || {
        let connected = unsafe { ConnectNamedPipe(raw(&handle), None) };
        assert!(
            connected.is_ok() || connected.unwrap_err().code() == ERROR_PIPE_CONNECTED.to_hresult()
        );
        test(File::from(handle));
    });
    let identity = Identity::current().unwrap();
    let filename = identity.image.file_name().unwrap().to_str().unwrap();
    (
        Pipe::connect(&name, &identity, &[filename]).unwrap(),
        server,
    )
}

fn read(file: &mut File) -> (u32, Value) {
    let mut header = [0; 8];
    file.read_exact(&mut header).unwrap();
    let length = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
    assert!(length <= MAX_FRAME);
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes).unwrap();
    (
        u32::from_le_bytes(header[..4].try_into().unwrap()),
        serde_json::from_slice(&bytes).unwrap(),
    )
}

fn send(file: &mut File, opcode: u32, value: Value) {
    let bytes = serde_json::to_vec(&value).unwrap();
    file.write_all(&opcode.to_le_bytes()).unwrap();
    file.write_all(&(bytes.len() as u32).to_le_bytes()).unwrap();
    // Split the payload to exercise partial reads of the byte-stream protocol.
    for part in bytes.chunks(7) {
        file.write_all(part).unwrap();
    }
}

fn handshake(file: &mut File) {
    assert_eq!(
        read(file),
        (0, json!({"v": 1, "client_id": APPLICATION_ID}))
    );
    send(file, 1, json!({"cmd": "DISPATCH", "evt": "READY"}));
}

fn acknowledge(file: &mut File, request: &Value) {
    send(
        file,
        1,
        json!({"cmd": "SET_ACTIVITY", "nonce": request["nonce"]}),
    );
}

#[test]
fn payloads_are_fixed_and_do_not_share_names_or_join_data() {
    let active = Activity::Simple(true).payload();
    assert_eq!(active["details"], "Overlay active");
    assert_eq!(
        Activity::Simple(false).payload()["details"],
        "Overlay hidden"
    );
    assert!(active.get("state").is_none());
    assert_eq!(active["assets"]["large_image"], "crosshair_rs");
    assert_eq!(
        active["buttons"],
        json!([
            {"label": "Download", "url": "https://github.com/ohne-b/crosshair-rs/releases"},
            {"label": "Source code", "url": "https://github.com/ohne-b/crosshair-rs"}
        ])
    );
    assert_eq!(active.as_object().unwrap().len(), 3);
    let quote = Activity::Quote.payload();
    assert_eq!(quote["details"], "I've got you in my sights.");
    assert_eq!(quote["state"], "— Soldier: 76");
    assert_eq!(quote.as_object().unwrap().len(), 4);
    let mut presence = Presence::default();
    presence.update(&Settings::default(), true);
    assert!(presence.worker.is_none());
}

#[test]
fn handshake_ping_activity_errors_and_frame_bounds() {
    let (pipe, server) = fixture(|mut file| {
        handshake(&mut file);
        let (_, request) = read(&mut file);
        assert_eq!(request["args"]["activity"], Activity::Quote.payload());
        send(&mut file, 3, json!({"ping": true}));
        assert_eq!(read(&mut file), (4, json!({"ping": true})));
        acknowledge(&mut file, &request);
        let (_, request) = read(&mut file);
        send(
            &mut file,
            1,
            json!({"evt": "ERROR", "nonce": request["nonce"]}),
        );
    });
    let mut session = Session::handshake(pipe).unwrap();
    session.set_activity(Some(Activity::Quote)).unwrap();
    assert!(session.set_activity(Some(Activity::Simple(false))).is_err());
    let mut header = [0; 8];
    header[4..].copy_from_slice(&((MAX_FRAME + 1) as u32).to_le_bytes());
    assert!(session.frame(header, Instant::now() + IO_TIMEOUT).is_err());
    server.join().unwrap();
}

#[test]
fn worker_debounces_reconnects_and_clears_without_a_dashboard() {
    let (events, received) = mpsc::channel();
    let (first, server1) = fixture({
        let events = events.clone();
        move |mut file| {
            handshake(&mut file);
            let (_, request) = read(&mut file);
            assert_eq!(
                request["args"]["activity"],
                Activity::Simple(false).payload()
            );
            acknowledge(&mut file, &request);
            events.send("first").unwrap();
            // Closing the pipe simulates Discord restarting.
        }
    });
    let (second, server2) = fixture(move |mut file| {
        handshake(&mut file);
        let (_, request) = read(&mut file);
        assert_eq!(
            request["args"]["activity"],
            Activity::Simple(false).payload()
        );
        acknowledge(&mut file, &request);
        events.send("reconnected").unwrap();
        let (_, clear) = read(&mut file);
        assert!(clear["args"]["activity"].is_null());
        events.send("cleared").unwrap();
    });
    let state = Arc::new(Mutex::new(State {
        activity: Some(Activity::Simple(true)),
        changed: Instant::now(),
        stop: false,
    }));
    let wake = StopSignal::new().unwrap();
    let mut pipes = vec![second, first];
    let worker = thread::spawn({
        let state = state.clone();
        let wake = wake.clone();
        move || {
            run(state, wake, || {
                Session::handshake(pipes.pop().ok_or_else(|| anyhow::anyhow!("offline"))?)
            });
        }
    });
    {
        let mut state = state.lock().unwrap();
        state.activity = Some(Activity::Simple(false));
        state.changed = Instant::now();
        wake.signal();
    }
    assert_eq!(
        received.recv_timeout(Duration::from_secs(3)).unwrap(),
        "first"
    );
    assert_eq!(
        received.recv_timeout(Duration::from_secs(4)).unwrap(),
        "reconnected"
    );
    state.lock().unwrap().activity = None;
    wake.signal();
    assert_eq!(
        received.recv_timeout(Duration::from_secs(2)).unwrap(),
        "cleared"
    );
    let start = Instant::now();
    state.lock().unwrap().stop = true;
    wake.signal();
    worker.join().unwrap();
    assert!(start.elapsed() < Duration::from_secs(1));
    server1.join().unwrap();
    server2.join().unwrap();
}

#[test]
fn an_unresponsive_peer_times_out_and_an_idle_read_wakes_cleanly() {
    let (release, wait) = mpsc::channel();
    let (pipe, server) = fixture(move |_file| {
        let _ = wait.recv_timeout(Duration::from_secs(3));
    });
    let wake = StopSignal::new().unwrap();
    let mut buffer = [0];
    wake.signal();
    assert_eq!(pipe.read_or_wake(&mut buffer, None, &wake).unwrap(), None);
    wake.reset();
    assert_eq!(
        pipe.read_or_wake(
            &mut buffer,
            Some(Instant::now() + Duration::from_millis(10)),
            &wake
        )
        .unwrap(),
        None
    );
    let start = Instant::now();
    assert!(Session::handshake(pipe).is_err());
    assert!(start.elapsed() < Duration::from_secs(2));
    release.send(()).unwrap();
    server.join().unwrap();
}
