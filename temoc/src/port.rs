use std::fmt::Debug;

#[derive(Debug, Clone)]
pub struct CyclePort {
    port: u16,
    base_port: u16,
    pool_size: u16,
}

impl CyclePort {
    pub fn new(port: u16, base_port: u16, pool_size: u16) -> Self {
        let available = u32::from(u16::MAX) - u32::from(base_port) + 1;
        let pool_size = u32::from(pool_size.max(1)).min(available) as u16;
        Self {
            port,
            base_port,
            pool_size,
        }
    }

    pub fn new_port(&mut self) {
        let last_offset = self.pool_size - 1;
        if self.port.saturating_sub(self.base_port) >= last_offset {
            self.port = self.base_port;
        } else {
            self.port += 1;
        }
    }

    pub fn to_port(&self) -> u16 {
        self.port
    }
}

#[cfg(test)]
mod tests {
    use super::CyclePort;

    #[test]
    fn one_port_pool_never_leaves_the_base_port() {
        let mut port = CyclePort::new(8085, 8085, 1);
        port.new_port();
        assert_eq!(8085, port.to_port());
    }

    #[test]
    fn pool_wraps_within_its_declared_size_without_overflow() {
        let mut port = CyclePort::new(u16::MAX, u16::MAX - 1, 10);
        port.new_port();
        assert_eq!(u16::MAX - 1, port.to_port());
        port.new_port();
        assert_eq!(u16::MAX, port.to_port());
    }
}
