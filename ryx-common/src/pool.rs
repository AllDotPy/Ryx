#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout_secs: u64,
    pub idle_timeout_secs: u64,
    pub max_lifetime_secs: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 1,
            connect_timeout_secs: 30,
            idle_timeout_secs: 600,
            max_lifetime_secs: 1800,
        }
    }
}
