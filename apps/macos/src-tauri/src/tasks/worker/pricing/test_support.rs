use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use rusqlite::params;

use crate::tikhub::{build_collection_request, TikHubCollectionRequest};
use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

pub(super) const PRICING_PLAN_ID: &str = "plan-pricing-fixture";
pub(super) const PRICING_RUN_ID: &str = "run-pricing-fixture";
pub(super) const PRICING_TASK_ID: &str = "task-pricing-fixture";

pub(super) struct FirstRequestGate {
  pub(super) entered: mpsc::Sender<()>,
  pub(super) release: mpsc::Receiver<()>,
}

pub(super) struct PricingHttpServer {
  pub(super) base_url: String,
  request_count: Arc<AtomicUsize>,
  stop: mpsc::Sender<()>,
  handle: thread::JoinHandle<()>,
}

impl PricingHttpServer {
  pub(super) fn start(first_request_gate: Option<FirstRequestGate>) -> Self {
    let listener = TcpListener::bind("127.0.0.1:0").expect("pricing server should bind");
    listener
      .set_nonblocking(true)
      .expect("pricing server should be nonblocking");
    let address = listener
      .local_addr()
      .expect("pricing server address should resolve");
    let request_count = Arc::new(AtomicUsize::new(0));
    let server_count = Arc::clone(&request_count);
    let (stop, stop_rx) = mpsc::channel();
    let handle = thread::spawn(move || {
      let mut first_request_gate = first_request_gate;
      let deadline = Instant::now() + Duration::from_secs(5);
      loop {
        match stop_rx.try_recv() {
          Ok(()) | Err(mpsc::TryRecvError::Disconnected) => break,
          Err(mpsc::TryRecvError::Empty) => {}
        }
        assert!(
          Instant::now() < deadline,
          "pricing test server did not receive a stop signal"
        );
        let (mut stream, _) = match listener.accept() {
          Ok(connection) => connection,
          Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            thread::sleep(Duration::from_millis(5));
            continue;
          }
          Err(error) => panic!("pricing server accept failed: {error}"),
        };
        let request = read_http_request(&mut stream);
        let current = server_count.fetch_add(1, Ordering::SeqCst) + 1;
        if current == 1 {
          if let Some(gate) = first_request_gate.take() {
            gate
              .entered
              .send(())
              .expect("first request signal should send");
            gate
              .release
              .recv_timeout(Duration::from_secs(3))
              .expect("first pricing request should be released");
          }
        }
        let body = if request.starts_with("GET /api/v1/tikhub/user/get_user_info ") {
          serde_json::json!({
            "code": 200,
            "user_data": {
              "balance": 5.0,
              "free_credit": 1.0,
              "available_credit": 6.0
            }
          })
        } else if request.starts_with("GET /api/v1/tikhub/user/calculate_price?") {
          serde_json::json!({
            "code": 200,
            "data": {
              "total_price": 0.01,
              "base_price": 0.01,
              "currency": "USD"
            }
          })
        } else {
          panic!("unexpected pricing request: {request}");
        };
        write_json_response(&mut stream, &body.to_string());
      }
    });
    Self {
      base_url: format!("http://{address}"),
      request_count,
      stop,
      handle,
    }
  }

  pub(super) fn request_count(&self) -> usize {
    self.request_count.load(Ordering::SeqCst)
  }

  pub(super) fn finish(self) -> usize {
    let Self {
      request_count,
      stop,
      handle,
      ..
    } = self;
    stop.send(()).ok();
    handle.join().expect("pricing server should finish");
    request_count.load(Ordering::SeqCst)
  }
}

fn read_http_request(stream: &mut TcpStream) -> String {
  stream
    .set_nonblocking(false)
    .expect("accepted pricing stream should block while reading");
  stream
    .set_read_timeout(Some(Duration::from_secs(2)))
    .expect("pricing request read timeout should set");
  let mut request = Vec::new();
  let mut buffer = [0_u8; 512];
  while !request.windows(4).any(|window| window == b"\r\n\r\n") {
    let read = stream
      .read(&mut buffer)
      .expect("pricing request should read");
    if read == 0 {
      break;
    }
    request.extend_from_slice(&buffer[..read]);
    assert!(request.len() <= 8_192, "pricing request headers too large");
  }
  String::from_utf8(request).expect("pricing request should be utf-8")
}

fn write_json_response(stream: &mut TcpStream, body: &str) {
  let response = format!(
    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
    body.len(),
    body
  );
  stream
    .write_all(response.as_bytes())
    .expect("pricing response should write");
}

#[test]
fn accepted_stream_waits_for_delayed_request_bytes() {
  let listener = TcpListener::bind("127.0.0.1:0").expect("pricing server should bind");
  listener
    .set_nonblocking(true)
    .expect("pricing listener should be nonblocking");
  let address = listener
    .local_addr()
    .expect("pricing server address should resolve");
  let (connected_tx, connected_rx) = mpsc::channel();
  let (release_tx, release_rx) = mpsc::channel();
  let client = thread::spawn(move || {
    let mut stream = TcpStream::connect(address).expect("pricing client should connect");
    connected_tx
      .send(())
      .expect("pricing client connection should signal");
    release_rx
      .recv_timeout(Duration::from_secs(3))
      .expect("pricing client write should be released");
    stream
      .write_all(b"GET /delayed HTTP/1.1\r\nHost: localhost\r\n\r\n")
      .expect("pricing client request should write");
  });
  connected_rx
    .recv_timeout(Duration::from_secs(3))
    .expect("pricing client should connect");
  let (mut stream, _) = listener
    .accept()
    .expect("queued pricing connection should accept");
  let release = thread::spawn(move || {
    thread::sleep(Duration::from_millis(100));
    release_tx
      .send(())
      .expect("pricing client write release should send");
  });

  let request = read_http_request(&mut stream);

  assert!(request.starts_with("GET /delayed HTTP/1.1"));
  release.join().expect("pricing release should finish");
  client.join().expect("pricing client should finish");
}

pub(super) fn create_pricing_fixture(
  root: &Path,
  status: &str,
  lease_owner: &str,
  generation: i64,
) -> rusqlite::Connection {
  create_workspace("计价栅栏测试", root).expect("workspace should be created");
  let connection =
    open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
  let now = Utc::now().to_rfc3339();
  connection
    .execute(
      "INSERT INTO collection_task (
         id, name, source_type, status, created_at, updated_at
       ) VALUES (?1, '计价任务', 'form', ?2, ?3, ?3)",
      params![PRICING_TASK_ID, status, now],
    )
    .expect("task should insert");
  connection
    .execute(
      "INSERT INTO collection_plan (
         id, task_id, source, schema_version, plan_json, validation_status,
         confirmed_by_user, created_at, updated_at
       ) VALUES (?1, ?2, 'form', 4, ?3, 'valid', 1, ?4, ?4)",
      params![
        PRICING_PLAN_ID,
        PRICING_TASK_ID,
        serde_json::json!({
          "budget_limit": {
            "currency": "USD",
            "amount_micros": 1_000_000
          }
        })
        .to_string(),
        now
      ],
    )
    .expect("plan should insert");
  connection
    .execute(
      "INSERT INTO task_run (
         id, task_id, plan_id, status, started_at
       ) VALUES (?1, ?2, ?3, ?4, ?5)",
      params![
        PRICING_RUN_ID,
        PRICING_TASK_ID,
        PRICING_PLAN_ID,
        status,
        now
      ],
    )
    .expect("run should insert");
  connection
    .execute(
      "INSERT INTO task_worker_lease (
         id, owner_id, lease_expires_at, created_at, updated_at, generation
       ) VALUES ('task_worker', ?1, 9223372036854775807, ?2, ?2, ?3)",
      params![lease_owner, now, generation],
    )
    .expect("worker lease should insert");
  connection
}

pub(super) fn pricing_request() -> TikHubCollectionRequest {
  build_collection_request(
    "tiktok",
    "user_search",
    &serde_json::json!({ "keyword": "汽车", "page_size": 1 }),
    None,
  )
  .expect("request should build")
}
