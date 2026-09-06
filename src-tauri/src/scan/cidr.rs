use std::net::Ipv4Addr;

use thiserror::Error;

const MAX_PREFIX_LEN: u8 = 22;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CidrError {
    #[error("CIDR must be IPv4 address/prefix")]
    InvalidFormat,
    #[error("CIDR prefix must be between 0 and 32")]
    InvalidPrefix,
    #[error("CIDR /{prefix_len} is larger than the /22 limit")]
    RangeTooLarge { prefix_len: u8 },
}

pub fn expand_ipv4_cidr(cidr: &str) -> Result<Vec<Ipv4Addr>, CidrError> {
    let (addr, prefix) = cidr.split_once('/').ok_or(CidrError::InvalidFormat)?;
    let base: Ipv4Addr = addr.parse().map_err(|_| CidrError::InvalidFormat)?;
    let prefix_len: u8 = prefix.parse().map_err(|_| CidrError::InvalidPrefix)?;
    if prefix_len > 32 {
        return Err(CidrError::InvalidPrefix);
    }
    if prefix_len < MAX_PREFIX_LEN {
        return Err(CidrError::RangeTooLarge { prefix_len });
    }

    let host_bits = u32::from(32 - prefix_len);
    let mask = if prefix_len == 0 {
        0
    } else {
        u32::MAX << host_bits
    };
    let network = u32::from(base) & mask;
    let size = 1u32 << host_bits;
    if size <= 2 {
        return Ok(Vec::new());
    }

    Ok((1..(size - 1))
        .map(|offset| Ipv4Addr::from(network + offset))
        .collect())
}
