//! 真实 S3 / OSS 兼容环境槽（不在 CI / 本机默认跑）。
//!
//! - 缺省：打印 SKIP 并返回（不失败）；
//! - `POLYPACK_S3_REAL=1` 但缺少 `POLYPACK_S3_TEST_URI` / 凭据：**失败**（开关打开
//!   却配置不全不得静默跳过）；
//! - 开关与 URI 齐备：对真实桶做 PUT → GET → LIST → DELETE，并在退出时兜底删除。
//!
//! 运行示例：
//! ```text
//! POLYPACK_S3_REAL=1 POLYPACK_S3_TEST_URI=s3://bucket/ci-prefix \
//! POLYPACK_S3_ENDPOINT=http://127.0.0.1:9000 POLYPACK_S3_REGION=us-east-1 \
//! POLYPACK_S3_ACCESS_KEY_ID=... POLYPACK_S3_SECRET_ACCESS_KEY=... \
//! cargo test -p polypack-cache --test s3_real -- --nocapture
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use polypack_cache::{Digest, ObjectStore, S3Config, S3Env, S3Store};

/// 共享对象信封开销：头行 23 + `\n` + blake3 十六进制 64 + `\n`。
const ENVELOPE_OVERHEAD: u64 = 89;

struct Cleanup<'a> {
    store: &'a S3Store,
    key: Digest,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = self.store.remove(&self.key);
    }
}

#[test]
fn real_s3_roundtrip() {
    if std::env::var("POLYPACK_S3_REAL").as_deref() != Ok("1") {
        println!(
            "SKIP: 真实 S3 环境槽未启用（设置 POLYPACK_S3_REAL=1 与 \
             POLYPACK_S3_TEST_URI=s3://<bucket>[/<prefix>]）"
        );
        return;
    }
    let uri = std::env::var("POLYPACK_S3_TEST_URI")
        .expect("POLYPACK_S3_REAL=1 时必须设置 POLYPACK_S3_TEST_URI");
    let config = S3Config::from_uri(&uri, S3Env::from_env())
        .expect("真实 S3 配置非法（凭据经 POLYPACK_S3_* / AWS_*）");
    let store = S3Store::new(&config).expect("真实 S3 后端装配失败");
    store.ensure_ready().expect("ListObjectsV2 探针失败");

    let tag = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时钟")
        .as_nanos();
    let key = Digest::of_bytes(format!("polypack-real-{}-{tag}", std::process::id()).as_bytes());
    let cleanup = Cleanup {
        store: &store,
        key: key.clone(),
    };

    let payload = b"polypack s3 real slot payload";
    store.put(&key, payload).expect("真实 PUT 失败");
    assert_eq!(
        store.get(&key).expect("真实 GET 失败").as_deref(),
        Some(payload.as_slice()),
        "真实读回内容不一致"
    );

    let entry = store
        .entries()
        .expect("真实 LIST 失败")
        .into_iter()
        .find(|entry| entry.key == key)
        .expect("真实 LIST 应包含刚写入的对象");
    assert_eq!(
        entry.bytes,
        ENVELOPE_OVERHEAD + payload.len() as u64,
        "对象大小应含共享信封"
    );

    assert_eq!(
        store.remove(&key).expect("真实 DELETE 失败"),
        ENVELOPE_OVERHEAD + payload.len() as u64,
        "remove 应返回对象字节数"
    );
    assert_eq!(store.get(&key).expect("删除后 GET 失败"), None);
    drop(cleanup);
}
