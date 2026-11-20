#[cfg(any(unix, test))]
mod job {
    use std::io::{self, Read, Write};

    use crosshair_showcase::{Error, PresetValidator, ShowcaseCard, ShowcaseRenderer};

    fn read_field(input: &mut impl Read, max: usize) -> io::Result<Vec<u8>> {
        let mut length = [0; 4];
        input.read_exact(&mut length)?;
        let length = u32::from_be_bytes(length) as usize;
        if length > max {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "field too large",
            ));
        }
        let mut bytes = vec![0; length];
        input.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    fn write_field(output: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
        output.write_all(&(bytes.len() as u32).to_be_bytes())?;
        output.write_all(bytes)
    }

    fn rejection(error: Error) -> &'static str {
        match error {
            Error::Format(message) | Error::Unsupported(message) => message,
            Error::LimitExceeded { .. } => "This preset exceeds the showcase rendering limits.",
            Error::Asset { .. } => "An embedded asset is invalid. Export the preset again.",
            Error::EmptyPreset => "The preset has no visible crosshair in its base state.",
            Error::DisplayText(_) => "The preset or display name cannot be drawn.",
            Error::Json(_) | Error::Document(_) => {
                "Upload a valid crosshair-rs preset JSON export."
            }
            _ => "The crosshair preview could not be rendered.",
        }
    }

    fn render(
        input: &mut impl Read,
        renderer: &ShowcaseRenderer<'_>,
    ) -> Result<(String, Vec<u8>), &'static str> {
        let author = read_field(input, 1024).map_err(|_| "Invalid submitter name.")?;
        let author = std::str::from_utf8(&author).map_err(|_| "Invalid submitter name.")?;
        let validator = PresetValidator::default();
        let json = read_field(input, validator.limits().max_json_bytes)
            .map_err(|_| "The preset upload is incomplete or too large.")?;
        let preset = validator.validate(&json).map_err(rejection)?;
        let png = renderer
            .render_png(ShowcaseCard {
                preset: &preset,
                author: Some(author),
            })
            .map_err(rejection)?;
        Ok((preset.download_filename().to_owned(), png))
    }

    pub(super) fn handle(
        mut input: impl Read,
        mut output: impl Write,
        renderer: &ShowcaseRenderer<'_>,
    ) -> io::Result<()> {
        match render(&mut input, renderer) {
            Ok((filename, png)) => {
                output.write_all(&[0])?;
                write_field(&mut output, filename.as_bytes())?;
                write_field(&mut output, &png)
            }
            Err(reason) => {
                output.write_all(&[1])?;
                write_field(&mut output, reason.as_bytes())
            }
        }
    }

    #[cfg(test)]
    #[test]
    fn returns_only_the_preview_and_safe_filename_for_valid_input() {
        let renderer = ShowcaseRenderer::new(include_bytes!("../../assets/Manrope.ttf")).unwrap();
        let mut request = Vec::new();
        write_field(&mut request, b"Player").unwrap();
        write_field(
            &mut request,
            include_bytes!("../tests/fixtures/preset-v1.json"),
        )
        .unwrap();
        let mut response = Vec::new();
        handle(request.as_slice(), &mut response, &renderer).unwrap();
        assert_eq!(response[0], 0);
        let mut fields = &response[1..];
        let filename = String::from_utf8(read_field(&mut fields, 128).unwrap()).unwrap();
        assert!(filename.starts_with("crosshair-training-grid-") && filename.ends_with(".json"));
        assert!(
            read_field(&mut fields, 4 * 1024 * 1024)
                .unwrap()
                .starts_with(b"\x89PNG\r\n\x1a\n")
        );
        assert!(fields.is_empty());
        response.clear();
        handle(&[0xff; 4][..], &mut response, &renderer).unwrap();
        assert_eq!(response[0], 1);
    }
}

#[cfg(unix)]
fn main() -> anyhow::Result<()> {
    use crosshair_showcase::ShowcaseRenderer;
    use std::{
        fs,
        io::ErrorKind,
        os::unix::{
            fs::{FileTypeExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
        sync::mpsc,
        thread,
        time::Duration,
    };

    const SOCKET: &str = "/run/crosshair/showcase.sock";
    let renderer = ShowcaseRenderer::new(include_bytes!("../../assets/Manrope.ttf"))?;
    let listener = match UnixListener::bind(SOCKET) {
        Err(error)
            if error.kind() == ErrorKind::AddrInUse
                && fs::symlink_metadata(SOCKET)?.file_type().is_socket()
                && UnixStream::connect(SOCKET)
                    .is_err_and(|err| err.kind() == ErrorKind::ConnectionRefused) =>
        {
            fs::remove_file(SOCKET)?;
            UnixListener::bind(SOCKET)?
        }
        result => result?,
    };
    fs::set_permissions(SOCKET, fs::Permissions::from_mode(0o600))?;
    for stream in listener.incoming() {
        let stream = stream?;
        let (finished, done) = mpsc::channel::<()>();
        thread::scope(|scope| {
            scope.spawn(move || {
                if let Err(mpsc::RecvTimeoutError::Timeout) =
                    done.recv_timeout(Duration::from_secs(30))
                {
                    eprintln!("showcase render timed out");
                    std::process::exit(1);
                }
            });
            let _ = job::handle(&stream, &stream, &renderer);
            drop(finished);
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("Run the showcase worker in its Linux container.")
}
