//! ADR-0003 v0 hard limits。

#[derive(Debug, Clone, Copy)]
pub struct ResourceLimits {
    pub max_memory_bytes: usize,
    pub max_cpu_ms: u64,
    pub max_modules_per_channel: usize,
}

pub const DEFAULT_LIMITS: ResourceLimits = ResourceLimits {
    max_memory_bytes: 16 * 1024 * 1024,
    max_cpu_ms: 50,
    max_modules_per_channel: 1,
};

impl Default for ResourceLimits {
    fn default() -> Self {
        DEFAULT_LIMITS
    }
}
