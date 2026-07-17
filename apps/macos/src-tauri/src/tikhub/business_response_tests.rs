use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use super::*;

const TOKEN_SENTINEL: &str = "tk-business-code-token-sentinel";
const BODY_SENTINEL: &str = "business-response-body-sentinel";

fn request_json(body: &str) -> AppResult<Value> {
  let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
  let address = listener
    .local_addr()
    .expect("test server address should resolve");
  let body = body.to_string();
  let server = thread::spawn(move || {
    let (mut stream, _) = listener.accept().expect("test server should accept");
    let mut request = [0_u8; 4096];
    let request_bytes_read = stream
      .read(&mut request)
      .expect("request should be readable");
    assert!(request_bytes_read > 0, "request should not be empty");
    write!(
      stream,
      "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
      body.len(),
      body
    )
    .expect("response should be writable");
  });
  let client = reqwest::blocking::Client::builder()
    .timeout(Duration::from_secs(2))
    .build()
    .expect("test client should build");
  let result = get_tikhub_json(
    &client,
    &format!("http://{address}"),
    "/api/v1/tikhub/user/get_user_info",
    TOKEN_SENTINEL,
  );
  server.join().expect("test server should finish");
  result
}

fn assert_error_is_sanitized(error: &AppError) {
  let serialized = serde_json::to_string(error).expect("error should serialize");
  assert!(!serialized.contains(TOKEN_SENTINEL));
  assert!(!serialized.contains(BODY_SENTINEL));
}

#[test]
fn rejects_http_success_with_failed_business_code_without_leaking_secrets() {
  let error = request_json(&format!(r#"{{"code":401,"message":"{BODY_SENTINEL}"}}"#))
    .expect_err("HTTP success with business code 401 must fail");

  assert_eq!(error.code, AppErrorCode::TikhubAuthError);
  assert!(!error.retryable);
  assert_eq!(
    error.safe_details.get("business_code").map(String::as_str),
    Some("401")
  );
  assert_error_is_sanitized(&error);
}

#[test]
fn rejects_http_success_without_required_business_code() {
  let error = request_json(&format!(r#"{{"message":"{BODY_SENTINEL}"}}"#))
    .expect_err("HTTP success without a business code must fail closed");

  assert_eq!(error.code, AppErrorCode::TikhubRequestError);
  assert!(!error.retryable);
  assert_error_is_sanitized(&error);
}
