//! worker 任务作用域的内存凭据注入（T-M8-07）：档案名 → base64(JSON) 载荷，仅驻内存。
//!
//! CLI 在任务开始时用 [`CredentialScope::enter`] 载入载荷（值来自环境变量或本机后端），
//! [`crate::secret::resolve`] 优先命中的作用域载荷；guard drop 即清空——跨任务零残留。
//! 同一作用域还把签名临时材料根指向任务的一次性私有目录（[`crate::temp`] 消费），
//! 因此密钥材料与临时目录同生共死。
//!
//! 进程内单任务：farm worker 每进程一次只执行一个任务（并发在 worker 进程之间）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// 作用域状态。
struct ScopeState {
    /// 代际号：只允许本次 `enter` 的 guard 清空（重叠作用域不互相踩踏）。
    generation: u64,
    /// 档案名 → base64(JSON) 载荷原文。
    payloads: BTreeMap<String, Vec<u8>>,
    /// 签名临时材料根（任务一次性目录，任务结束随目录删除）。
    temp_root: PathBuf,
}

/// 当前作用域（`None` = 不在任务内，走既有后端链与系统临时目录）。
static STATE: Mutex<Option<ScopeState>> = Mutex::new(None);
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// 任务作用域守卫：`enter` 载入内存载荷与临时根，drop 清空。
pub struct CredentialScope {
    generation: u64,
}

impl CredentialScope {
    /// 进入作用域。`payloads` = (档案名, base64(JSON) 载荷原文)；`temp_root` 为任务私有目录。
    pub fn enter(payloads: Vec<(String, Vec<u8>)>, temp_root: &Path) -> Self {
        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        *lock() = Some(ScopeState {
            generation,
            payloads: payloads.into_iter().collect(),
            temp_root: temp_root.to_path_buf(),
        });
        Self { generation }
    }
}

impl Drop for CredentialScope {
    fn drop(&mut self) {
        let mut state = lock();
        if state
            .as_ref()
            .is_some_and(|state| state.generation == self.generation)
        {
            *state = None;
        }
    }
}

/// 作用域内的内存载荷（[`crate::secret::resolve`] 的优先来源）。
pub(crate) fn payload(name: &str) -> Option<Vec<u8>> {
    lock().as_ref()?.payloads.get(name).cloned()
}

/// 作用域内的签名临时根（[`crate::temp`] 替代 `std::env::temp_dir()`）。
pub(crate) fn temp_root() -> Option<PathBuf> {
    lock().as_ref().map(|state| state.temp_root.clone())
}

/// 取状态锁；毒化（持锁线程 panic）只丢缓存，不在签名路径上二次 panic。
fn lock() -> std::sync::MutexGuard<'static, Option<ScopeState>> {
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_payloads_and_temp_root_are_visible_then_cleared_on_drop() {
        let root = std::env::temp_dir();
        let guard = CredentialScope::enter(
            vec![("injected-profile".to_string(), b"payload".to_vec())],
            &root,
        );
        assert_eq!(
            payload("injected-profile").as_deref(),
            Some(b"payload".as_slice())
        );
        assert_eq!(temp_root().as_deref(), Some(root.as_path()));
        drop(guard);
        assert!(
            payload("injected-profile").is_none(),
            "drop 后内存载荷必须清空"
        );
        assert!(temp_root().is_none(), "drop 后临时根必须清空");
    }
}
