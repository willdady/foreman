use std::collections::HashSet;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum PortManagerError {
    #[error("all ports in range have been reserved")]
    OutOfPorts,
    #[error("end_port must be greater than start_port")]
    InvalidPortRange,
    #[error("can not release port {0} as it is not reserved")]
    CanNotReleaseUnreservedPort(u16),
}

#[derive(Debug)]
pub struct PortManager {
    start_port: u16,
    end_port: u16,
    reserved_ports: HashSet<u16>,
}

impl PortManager {
    pub fn new(start_port: Option<u16>, end_port: Option<u16>) -> Result<Self, PortManagerError> {
        let start_port = start_port.unwrap_or(49152);
        let end_port = end_port.unwrap_or(65535);
        if end_port < start_port {
            return Err(PortManagerError::InvalidPortRange);
        }
        Ok(PortManager {
            start_port,
            end_port,
            reserved_ports: HashSet::new(),
        })
    }

    pub fn reserve_port(&mut self) -> Result<u16, PortManagerError> {
        let mut port = self.start_port;
        while self.reserved_ports.contains(&port) {
            port += 1;
        }
        if port > self.end_port {
            return Err(PortManagerError::OutOfPorts);
        }
        self.reserved_ports.insert(port);
        Ok(port)
    }

    pub fn release_port(&mut self, port: u16) -> Result<(), PortManagerError> {
        if !self.reserved_ports.contains(&port) {
            return Err(PortManagerError::CanNotReleaseUnreservedPort(port));
        }
        self.reserved_ports.remove(&port);
        Ok(())
    }
}
