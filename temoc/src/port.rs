use std::fmt::Debug;

#[derive(Debug, Clone)]
pub struct CyclePort {
    port: u16,
    base_port: u16,
    pool_size: u16,
}

impl CyclePort {
    pub fn new(port: u16, base_port: u16, pool_size: u16) -> Self {
        Self {
            port,
            base_port,
            pool_size,
        }
    }

    pub fn new_port(&mut self) {
        self.port += 1;
        if self.port > self.base_port + self.pool_size {
            self.port = self.base_port;
        }
    }

    pub fn to_port(&self) -> u16 {
        self.port
    }
}
