//! 插件资源上限：fuel（指令预算）、epoch（墙钟超时）、线性内存。
//!
//! 三者都有硬上限常量：配置只能收紧、不能放开（防止「关掉沙箱限制」的配置面）。

use poly_core::codes;

use crate::error::PluginError;

/// fuel 硬上限（约数十秒的常规插件执行）。
pub const MAX_FUEL: u64 = 10_000_000_000;
/// epoch 硬上限（tick 间隔 10ms，即最多约 10 分钟）。
pub const MAX_EPOCH_TICKS: u64 = 60_000;
/// 线性内存硬上限（1 GiB）。
pub const MAX_MEMORY_BYTES: usize = 1 << 30;
/// epoch tick 间隔（宿主后台线程递增 epoch 的周期）。
pub const EPOCH_TICK: std::time::Duration = std::time::Duration::from_millis(10);

/// 单次插件调用的资源上限。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceLimits {
    /// fuel（wasm 指令预算）。
    pub fuel: u64,
    /// epoch ticks（每 tick 10ms）。
    pub epoch_ticks: u64,
    /// 线性内存上限（字节）。
    pub max_memory_bytes: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            // 参考插件级别的工作量绰绰有余；无限循环在 ~秒级被 fuel 截断。
            fuel: 1_000_000_000,
            epoch_ticks: 500,
            max_memory_bytes: 64 << 20,
        }
    }
}

impl ResourceLimits {
    /// 构造并校验上限：0 或超过硬上限一律拒绝。
    pub fn new(fuel: u64, epoch_ticks: u64, max_memory_bytes: usize) -> Result<Self, PluginError> {
        let limits = Self {
            fuel,
            epoch_ticks,
            max_memory_bytes,
        };
        limits.validate()?;
        Ok(limits)
    }

    /// 校验当前值（`Default` 亦须过此关）。
    pub fn validate(&self) -> Result<(), PluginError> {
        if self.fuel == 0 || self.fuel > MAX_FUEL {
            return Err(PluginError::new(
                codes::E_PLUGIN_0003,
                format!("fuel 上限必须在 1..={MAX_FUEL} 之间（当前 {}）", self.fuel),
            ));
        }
        if self.epoch_ticks == 0 || self.epoch_ticks > MAX_EPOCH_TICKS {
            return Err(PluginError::new(
                codes::E_PLUGIN_0003,
                format!(
                    "epoch 上限必须在 1..={MAX_EPOCH_TICKS} tick 之间（当前 {}）",
                    self.epoch_ticks
                ),
            ));
        }
        if self.max_memory_bytes == 0 || self.max_memory_bytes > MAX_MEMORY_BYTES {
            return Err(PluginError::new(
                codes::E_PLUGIN_0004,
                format!(
                    "内存上限必须在 1..={MAX_MEMORY_BYTES} 字节之间（当前 {}）",
                    self.max_memory_bytes
                ),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_within_hard_caps() {
        ResourceLimits::default().validate().expect("默认上限合法");
    }

    #[test]
    fn zero_and_over_cap_are_rejected() {
        assert_eq!(
            ResourceLimits::new(0, 1, 1).unwrap_err().code,
            codes::E_PLUGIN_0003
        );
        assert_eq!(
            ResourceLimits::new(MAX_FUEL + 1, 1, 1).unwrap_err().code,
            codes::E_PLUGIN_0003
        );
        assert_eq!(
            ResourceLimits::new(1, MAX_EPOCH_TICKS + 1, 1)
                .unwrap_err()
                .code,
            codes::E_PLUGIN_0003
        );
        assert_eq!(
            ResourceLimits::new(1, 1, MAX_MEMORY_BYTES + 1)
                .unwrap_err()
                .code,
            codes::E_PLUGIN_0004
        );
    }
}
