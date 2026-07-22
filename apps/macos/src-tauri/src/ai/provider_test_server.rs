use std::io::{ErrorKind, Read, Write};
use std::net::TcpListener;
use std::thread;

pub(super) fn serve_once(
  status: u16,
  body: String,
  inspect: impl FnOnce(&str) + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
  serve_once_with_retry_after(status, body, None, inspect)
}

pub(super) fn serve_once_with_retry_after(
  status: u16,
  body: String,
  retry_after: Option<&str>,
  inspect: impl FnOnce(&str) + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
  let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
  let address = listener
    .local_addr()
    .expect("test server address should resolve");
  let retry_after = retry_after.map(str::to_string);
  let server = thread::spawn(move || {
    let (mut stream, _) = listener.accept().expect("test server should accept");
    let mut request = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
      let bytes_read = stream
        .read(&mut buffer)
        .expect("request should be readable");
      if bytes_read == 0 {
        break;
      }
      request.extend_from_slice(&buffer[..bytes_read]);
      let text = String::from_utf8_lossy(&request);
      if let Some(header_end) = text.find("\r\n\r\n") {
        let content_length = text[..header_end]
          .lines()
          .find_map(|line| {
            line
              .to_ascii_lowercase()
              .strip_prefix("content-length:")
              .and_then(|value| value.trim().parse::<usize>().ok())
          })
          .unwrap_or(0);
        if request.len() >= header_end + 4 + content_length {
          break;
        }
      }
    }
    inspect(&String::from_utf8_lossy(&request));
    let reason = if status == 200 { "OK" } else { "Error" };
    let retry_after = retry_after
      .as_deref()
      .map(|value| format!("Retry-After: {value}\r\n"))
      .unwrap_or_default();
    if let Err(error) = write!(
      stream,
      "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n{retry_after}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
      body.len()
    ) {
      assert!(
        matches!(error.kind(), ErrorKind::BrokenPipe | ErrorKind::ConnectionReset),
        "unexpected response write failure: {error}"
      );
    }
  });
  (format!("http://{address}"), server)
}
