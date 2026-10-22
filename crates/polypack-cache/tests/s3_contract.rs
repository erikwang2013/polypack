//! S3 / OSS 兼容后端的网络契约测试：进程内 mock S3 + 独立 SigV4 重算。
//!
//! mock 服务端从**原始请求**重算 AWS SigV4（HMAC-SHA256 链），签名不符即 403：
//! 因此 PUT / GET / HEAD / DELETE / ListObjectsV2 只有在客户端签名正确时才会成功。
//! 覆盖：强制分页、损坏条目删除、remove 返回对象大小、FallbackStore 在
//! 403 / 500 / 连接拒绝时只告警一次的降级。

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use hmac::{Hmac, Mac};
use percent_encoding::percent_decode_str;
use polypack_cache::{Digest, FallbackStore, LocalStore, ObjectStore, S3Config, S3Env, S3Store};
use sha2::{Digest as _, Sha256};

const ACCESS_KEY: &str = "AKIDCONTRACT";
const SECRET_KEY: &str = "contract-secret-key";
const REGION: &str = "us-east-1";
const BUCKET: &str = "test-bucket";

// ---------------------------------------------------------------------------
// mock S3
// ---------------------------------------------------------------------------

struct Object {
    body: Vec<u8>,
    last_modified: String,
}

struct Record {
    method: String,
    signing_headers: String,
    oss_header: Option<String>,
}

struct State {
    objects: BTreeMap<String, Object>,
    deletions: Vec<String>,
    requests: Vec<Record>,
    fail_status: Option<u16>,
    page_size: usize,
}

impl State {
    fn new(page_size: usize) -> Self {
        Self {
            objects: BTreeMap::new(),
            deletions: Vec::new(),
            requests: Vec::new(),
            fail_status: None,
            page_size,
        }
    }
}

struct MockS3 {
    addr: SocketAddr,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MockS3 {
    fn start(page_size: usize) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定 mock 端口");
        listener.set_nonblocking(true).expect("非阻塞监听");
        let addr = listener.local_addr().expect("监听地址");
        let state = Arc::new(Mutex::new(State::new(page_size)));
        let stop = Arc::new(AtomicBool::new(false));
        let handle = {
            let state = Arc::clone(&state);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            // Windows 上非阻塞 listener 的 accepted socket 可能继承非阻塞；
                            // 读请求前显式切回阻塞，否则 read 立即 WouldBlock → 连接被提前关闭。
                            let _ = stream.set_nonblocking(false);
                            // keep-alive + 每连接独立线程：ureq 可能复用连接，也可能新开连接；
                            // accept 线程必须始终可接受新连接，否则 Windows 上 PUT 会 connection aborted。
                            let state = Arc::clone(&state);
                            let stop = Arc::clone(&stop);
                            thread::spawn(move || {
                                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                                while !stop.load(Ordering::Relaxed) {
                                    let Some(request) = read_request(&mut stream) else {
                                        break;
                                    };
                                    handle_request(&mut stream, &request, &state);
                                }
                            });
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(_) => break,
                    }
                }
            })
        };
        Self {
            addr,
            state,
            stop,
            handle: Some(handle),
        }
    }

    fn endpoint(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn set_fail_status(&self, status: u16) {
        self.state.lock().unwrap().fail_status = Some(status);
    }

    fn object(&self, key: &str) -> Option<Vec<u8>> {
        self.state
            .lock()
            .unwrap()
            .objects
            .get(key)
            .map(|object| object.body.clone())
    }

    fn corrupt(&self, key: &str) {
        let mut state = self.state.lock().unwrap();
        let object = state.objects.get_mut(key).expect("对象存在");
        let last = object.body.len() - 1;
        object.body[last] ^= 0xff;
    }

    fn deleted(&self, key: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .deletions
            .iter()
            .any(|deleted| deleted == key)
    }

    fn requests(&self) -> Vec<(String, String, Option<String>)> {
        self.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .map(|record| {
                (
                    record.method.clone(),
                    record.signing_headers.clone(),
                    record.oss_header.clone(),
                )
            })
            .collect()
    }
}

impl Drop for MockS3 {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct RawRequest {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl RawRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn read_request(stream: &mut TcpStream) -> Option<RawRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = find(&buffer, b"\r\n\r\n") {
            break position;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect();
    let content_length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let body_start = head_end + 4;
    while buffer.len() < body_start + content_length {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    Some(RawRequest {
        method,
        target,
        headers,
        body: buffer[body_start..].to_vec(),
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn handle_request(stream: &mut TcpStream, request: &RawRequest, shared: &Mutex<State>) {
    let mut state = shared.lock().unwrap();
    let signing_headers = match verify_signature(request) {
        Ok(signed) => signed,
        Err(code) => {
            drop(state);
            respond_error(stream, 403, code);
            return;
        }
    };
    state.requests.push(Record {
        method: request.method.clone(),
        signing_headers,
        oss_header: request.header("x-oss-s3-compat").map(str::to_string),
    });
    if let Some(status) = state.fail_status {
        respond_error(stream, status, "InternalError");
        return;
    }
    let (path, query) = split_target(&request.target);
    let prefix = format!("/{BUCKET}");
    let Some(key) = path.strip_prefix(&prefix) else {
        respond_error(stream, 400, "InvalidRequest");
        return;
    };
    let key = key.strip_prefix('/').unwrap_or("");
    match request.method.as_str() {
        "GET" if query_has_list_type(query) => list_objects(stream, &mut state, key, query),
        "GET" => match state.objects.get(key) {
            Some(object) => respond(stream, 200, &object.body),
            None => respond_error(stream, 404, "NoSuchKey"),
        },
        "HEAD" => match state.objects.get(key) {
            Some(object) => respond_head(stream, 200, object.body.len()),
            None => respond_head(stream, 404, 0),
        },
        "PUT" => {
            state.objects.insert(
                key.to_string(),
                Object {
                    body: request.body.clone(),
                    last_modified: "2024-02-29T00:00:00.000Z".to_string(),
                },
            );
            respond(stream, 200, &[]);
        }
        "DELETE" => {
            state.objects.remove(key);
            state.deletions.push(key.to_string());
            respond(stream, 204, &[]);
        }
        _ => respond_error(stream, 405, "MethodNotAllowed"),
    }
}

fn list_objects(stream: &mut TcpStream, state: &mut State, bucket_path: &str, query: &str) {
    let _ = bucket_path;
    let pairs = query_pairs(query);
    let value = |name: &str| {
        pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    };
    let prefix = value("prefix");
    let start = value("continuation-token").parse::<usize>().unwrap_or(0);
    let keys: Vec<String> = state
        .objects
        .keys()
        .filter(|key| key.starts_with(&prefix))
        .cloned()
        .collect();
    let end = (start + state.page_size).min(keys.len());
    let truncated = end < keys.len();
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">",
    );
    xml.push_str(&format!("<IsTruncated>{truncated}</IsTruncated>"));
    if truncated {
        xml.push_str(&format!(
            "<NextContinuationToken>{end}</NextContinuationToken>"
        ));
    }
    for key in &keys[start.min(keys.len())..end] {
        let object = &state.objects[key];
        xml.push_str(&format!(
            "<Contents><Key>{}</Key><LastModified>{}</LastModified><Size>{}</Size></Contents>",
            xml_escape(key),
            object.last_modified,
            object.body.len()
        ));
    }
    xml.push_str("</ListBucketResult>");
    respond(stream, 200, xml.as_bytes());
}

fn query_has_list_type(query: &str) -> bool {
    query_pairs(query)
        .iter()
        .any(|(key, value)| key == "list-type" && value == "2")
}

fn query_pairs(query: &str) -> Vec<(String, String)> {
    if query.is_empty() {
        return Vec::new();
    }
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (
                percent_decode_str(key).decode_utf8_lossy().into_owned(),
                percent_decode_str(value).decode_utf8_lossy().into_owned(),
            )
        })
        .collect()
}

fn split_target(target: &str) -> (&str, &str) {
    match target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (target, ""),
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// ---------------------------------------------------------------------------
// 独立 SigV4 重算（不复用 crate 的签名实现）
// ---------------------------------------------------------------------------

fn verify_signature(request: &RawRequest) -> Result<String, &'static str> {
    let authorization = request.header("authorization").ok_or("AccessDenied")?;
    let rest = authorization
        .strip_prefix("AWS4-HMAC-SHA256 ")
        .ok_or("AccessDenied")?;
    let mut credential = None;
    let mut signed_headers = None;
    let mut signature = None;
    for part in rest.split(',') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("Credential=") {
            credential = Some(value);
        } else if let Some(value) = part.strip_prefix("SignedHeaders=") {
            signed_headers = Some(value);
        } else if let Some(value) = part.strip_prefix("Signature=") {
            signature = Some(value);
        }
    }
    let credential = credential.ok_or("AccessDenied")?;
    let signed_headers = signed_headers.ok_or("AccessDenied")?;
    let signature = signature.ok_or("AccessDenied")?;
    let mut scope = credential.split('/');
    let access_key = scope.next().ok_or("AccessDenied")?;
    let date = scope.next().ok_or("AccessDenied")?;
    let region = scope.next().ok_or("AccessDenied")?;
    let service = scope.next().ok_or("AccessDenied")?;
    let terminator = scope.next().ok_or("AccessDenied")?;
    if access_key != ACCESS_KEY
        || region != REGION
        || service != "s3"
        || terminator != "aws4_request"
        || scope.next().is_some()
    {
        return Err("AccessDenied");
    }

    let (path, query) = split_target(&request.target);
    let mut pairs: Vec<&str> = query.split('&').filter(|pair| !pair.is_empty()).collect();
    pairs.sort_unstable();
    let canonical_query = pairs.join("&");
    let mut canonical_headers = String::new();
    for name in signed_headers.split(';') {
        let value = request.header(name).ok_or("SignatureDoesNotMatch")?;
        canonical_headers.push_str(&format!(
            "{}:{}\n",
            name.to_ascii_lowercase(),
            value.split_whitespace().collect::<Vec<_>>().join(" ")
        ));
    }
    let amz_date = request
        .header("x-amz-date")
        .ok_or("SignatureDoesNotMatch")?;
    let payload_hash = request
        .header("x-amz-content-sha256")
        .ok_or("SignatureDoesNotMatch")?;
    if payload_hash != sha256_hex(&request.body) {
        return Err("XAmzContentSHA256Mismatch");
    }
    let canonical_request = format!(
        "{}\n{path}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}",
        request.method
    );
    let credential_scope = format!("{date}/{region}/s3/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let mut key = hmac(format!("AWS4{SECRET_KEY}").as_bytes(), date.as_bytes());
    key = hmac(&key, region.as_bytes());
    key = hmac(&key, b"s3");
    key = hmac(&key, b"aws4_request");
    let expected = hex(&hmac(&key, string_to_sign.as_bytes()));
    if expected != signature {
        return Err("SignatureDoesNotMatch");
    }
    Ok(signed_headers.to_string())
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC 密钥长度任意");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

// ---------------------------------------------------------------------------
// HTTP 响应
// ---------------------------------------------------------------------------

fn respond(stream: &mut TcpStream, status: u16, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
        reason(status),
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

/// HEAD：Content-Length 报对象大小，但不写正文。
fn respond_head(stream: &mut TcpStream, status: u16, content_length: usize) {
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/octet-stream\r\nContent-Length: {content_length}\r\nConnection: keep-alive\r\n\r\n",
        reason(status)
    );
    let _ = stream.write_all(head.as_bytes());
}

fn respond_error(stream: &mut TcpStream, status: u16, code: &str) {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>{code}</Code><Message>mock</Message></Error>"
    );
    respond(stream, status, body.as_bytes());
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Error",
    }
}

// ---------------------------------------------------------------------------
// 测试夹具
// ---------------------------------------------------------------------------

const ENVELOPE_OVERHEAD: u64 = 89; // 头行 23 + \n + 64 + \n

fn config(endpoint: &str, secret: &str) -> S3Config {
    S3Config::from_uri(
        &format!("s3://{BUCKET}/cache"),
        S3Env {
            endpoint: Some(endpoint.to_string()),
            access_key_id: Some(ACCESS_KEY.to_string()),
            secret_access_key: Some(secret.to_string()),
            ..S3Env::default()
        },
    )
    .expect("测试配置合法")
}

fn key(tag: &str) -> Digest {
    Digest::of_bytes(tag.as_bytes())
}

fn object_key(key: &Digest) -> String {
    format!("cache/{}/{}", &key.as_str()[..2], key.as_str())
}

fn temp_root(tag: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "polypack-cache-s3-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn put_get_list_remove_roundtrip_with_forced_pagination() {
    let mock = MockS3::start(2); // 每页 2 条 → 5 个对象强制 3 页
    let store = S3Store::new(&config(&mock.endpoint(), SECRET_KEY)).expect("后端装配");
    store.ensure_ready().expect("ListObjectsV2 探针成功");

    let mut expected = Vec::new();
    for index in 0..5 {
        let key = key(&format!("artifact-{index}"));
        let payload = format!("payload-{index}").into_bytes();
        store.put(&key, &payload).expect("PUT 签名正确");
        expected.push((key, payload));
    }
    for (key, payload) in &expected {
        assert_eq!(
            store.get(key).expect("GET 签名正确").as_deref(),
            Some(payload.as_slice())
        );
    }

    let entries = store.entries().expect("分页列表成功");
    assert_eq!(entries.len(), 5, "跨页聚合应覆盖全部对象");
    for (key, payload) in &expected {
        let entry = entries
            .iter()
            .find(|entry| &entry.key == key)
            .expect("条目存在");
        assert_eq!(entry.bytes, ENVELOPE_OVERHEAD + payload.len() as u64);
    }

    // remove = HEAD 大小 + DELETE（S3 DELETE 不返回长度）
    let (key, payload) = &expected[0];
    assert_eq!(
        store.remove(key).expect("DELETE 成功"),
        ENVELOPE_OVERHEAD + payload.len() as u64
    );
    assert!(mock.deleted(&object_key(key)), "mock 收到 DELETE");
    assert_eq!(store.get(key).unwrap(), None);
    assert_eq!(store.entries().unwrap().len(), 4);
}

#[test]
fn corrupt_object_is_deleted_and_reported_miss() {
    let mock = MockS3::start(10);
    let store = S3Store::new(&config(&mock.endpoint(), SECRET_KEY)).expect("后端装配");
    let key = key("corrupt");
    store.put(&key, b"payload").expect("PUT");

    mock.corrupt(&object_key(&key));
    assert_eq!(store.get(&key).expect("损坏视为未命中"), None);
    assert!(mock.deleted(&object_key(&key)), "损坏条目应被 DELETE");
    assert!(mock.object(&object_key(&key)).is_none());
}

#[test]
fn oss_compat_header_is_sent_and_signed() {
    let mock = MockS3::start(10);
    let mut config = config(&mock.endpoint(), SECRET_KEY);
    config.oss_compat = true;
    let store = S3Store::new(&config).expect("后端装配");
    store.put(&key("oss"), b"payload").expect("PUT");

    let requests = mock.requests();
    let (_, signing_headers, oss_header) = requests.last().expect("至少一次请求");
    assert_eq!(oss_header.as_deref(), Some("true"));
    assert!(
        signing_headers
            .split(';')
            .any(|name| name == "x-oss-s3-compat"),
        "OSS 兼容头必须在 SignedHeaders 中: {signing_headers}"
    );
}

fn fallback(
    endpoint: &str,
    secret: &str,
    local_root: &std::path::Path,
) -> (FallbackStore<S3Store, LocalStore>, Arc<Mutex<Vec<String>>>) {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&warnings);
    let store = FallbackStore::new(
        S3Store::new(&config(endpoint, secret)).expect("后端装配"),
        LocalStore::new(local_root),
        move |message: &str| sink.lock().unwrap().push(message.to_string()),
    );
    (store, warnings)
}

fn assert_degrades_once(store: FallbackStore<S3Store, LocalStore>, warnings: &Mutex<Vec<String>>) {
    let key = key("degrade");
    // 读：主后端错误 → 告警一次 + 后备未命中
    assert_eq!(store.get(&key).unwrap(), None);
    // 写：主后端错误不阻断，后备写入
    store.put(&key, b"payload").unwrap();
    // 再读：后备命中
    assert_eq!(
        store.get(&key).unwrap().as_deref(),
        Some(b"payload".as_slice())
    );
    // 再写 / 删：主后端继续错误，但告警不重复
    store.put(&key, b"payload2").unwrap();
    assert!(store.remove(&key).unwrap() > 0);

    let warnings = warnings.lock().unwrap();
    assert_eq!(warnings.len(), 1, "降级告警只报一次: {warnings:?}");
    assert!(warnings[0].contains("降级"), "告警应说明降级: {warnings:?}");
}

#[test]
fn fallback_degrades_once_on_403() {
    let mock = MockS3::start(10);
    let root = temp_root("403");
    let (store, warnings) = fallback(&mock.endpoint(), "wrong-secret", &root);
    assert_degrades_once(store, &warnings);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn fallback_degrades_once_on_500() {
    let mock = MockS3::start(10);
    mock.set_fail_status(500);
    let root = temp_root("500");
    let (store, warnings) = fallback(&mock.endpoint(), SECRET_KEY, &root);
    assert_degrades_once(store, &warnings);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn fallback_degrades_once_on_connection_refused() {
    // 拿一个已释放的端口：监听 → 取地址 → 关闭
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let root = temp_root("refused");
    let (store, warnings) = fallback(&format!("http://{addr}"), SECRET_KEY, &root);
    assert_degrades_once(store, &warnings);
    let _ = std::fs::remove_dir_all(root);
}
