use std::net::Ipv4Addr;

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InterfaceError {
    #[error("failed to enumerate interfaces: {0}")]
    Enumerate(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterfaceDto {
    pub name: String,
    pub description: String,
    pub ipv4: String,
    pub prefix_len: u8,
    pub is_primary: bool,
}

#[derive(Debug, Clone)]
pub struct InterfaceCandidate {
    pub name: String,
    pub description: String,
    pub ipv4: Ipv4Addr,
    pub prefix_len: u8,
    pub is_up: bool,
    pub is_loopback: bool,
    pub has_default_gateway: bool,
}

pub fn list_interfaces() -> Result<Vec<InterfaceDto>, InterfaceError> {
    enumerate_candidates().map(filter_interface_candidates)
}

pub fn filter_interface_candidates(candidates: Vec<InterfaceCandidate>) -> Vec<InterfaceDto> {
    let mut interfaces: Vec<InterfaceDto> = candidates
        .into_iter()
        .filter(|candidate| {
            candidate.is_up
                && !candidate.is_loopback
                && !is_virtual_name(&candidate.name, &candidate.description)
        })
        .map(|candidate| InterfaceDto {
            name: candidate.name,
            description: candidate.description,
            ipv4: candidate.ipv4.to_string(),
            prefix_len: candidate.prefix_len,
            is_primary: candidate.has_default_gateway,
        })
        .collect();
    interfaces.sort_by_key(|interface| !interface.is_primary);
    interfaces
}

fn is_virtual_name(name: &str, description: &str) -> bool {
    let text = format!(
        "{} {}",
        name.to_ascii_lowercase(),
        description.to_ascii_lowercase()
    );
    [
        "virtual",
        "vpn",
        "tunnel",
        "tailscale",
        "wireguard",
        "hyper-v",
        "wsl",
        "docker",
        "loopback",
        "tap",
        "tun",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

#[cfg(unix)]
fn enumerate_candidates() -> Result<Vec<InterfaceCandidate>, InterfaceError> {
    let addrs =
        if_addrs::get_if_addrs().map_err(|err| InterfaceError::Enumerate(err.to_string()))?;
    Ok(addrs
        .into_iter()
        .filter_map(|iface| match iface.addr {
            if_addrs::IfAddr::V4(addr) => Some(InterfaceCandidate {
                name: iface.name.clone(),
                description: iface.name,
                ipv4: addr.ip,
                prefix_len: prefix_len(addr.netmask),
                is_up: true,
                is_loopback: iface.is_loopback(),
                has_default_gateway: false,
            }),
            if_addrs::IfAddr::V6(_) => None,
        })
        .collect())
}

#[cfg(unix)]
fn prefix_len(mask: Ipv4Addr) -> u8 {
    u8::try_from(u32::from(mask).count_ones()).unwrap_or(32)
}

#[cfg(windows)]
fn enumerate_candidates() -> Result<Vec<InterfaceCandidate>, InterfaceError> {
    windows_impl::enumerate_candidates()
}

#[cfg(windows)]
mod windows_impl {
    use std::ffi::CStr;
    use std::net::Ipv4Addr;

    use super::{InterfaceCandidate, InterfaceError};
    use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_INCLUDE_PREFIX,
        IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};

    pub fn enumerate_candidates() -> Result<Vec<InterfaceCandidate>, InterfaceError> {
        let mut len = 0u32;
        // SAFETY: [Category 8 - FFI boundary] This probe passes a null output buffer as required by GetAdaptersAddresses to receive the needed byte length.
        let code = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_INET.0),
                GAA_FLAG_INCLUDE_PREFIX | GAA_FLAG_INCLUDE_GATEWAYS,
                None,
                None,
                &mut len,
            )
        };
        if code != ERROR_BUFFER_OVERFLOW.0 && code != NO_ERROR.0 {
            return Err(InterfaceError::Enumerate(format!(
                "GetAdaptersAddresses sizing failed: {code}"
            )));
        }
        let mut buffer = vec![
            0u8;
            usize::try_from(len).map_err(|_| InterfaceError::Enumerate(
                "adapter buffer too large".to_owned()
            ))?
        ];
        let ptr = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        // SAFETY: [Category 8 - FFI boundary] `buffer` is writable for `len` bytes and lives until traversal finishes; the API fills a linked list within that buffer.
        let code = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_INET.0),
                GAA_FLAG_INCLUDE_PREFIX | GAA_FLAG_INCLUDE_GATEWAYS,
                None,
                Some(ptr),
                &mut len,
            )
        };
        if code != NO_ERROR.0 {
            return Err(InterfaceError::Enumerate(format!(
                "GetAdaptersAddresses failed: {code}"
            )));
        }
        collect(ptr)
    }

    fn collect(
        mut adapter: *mut IP_ADAPTER_ADDRESSES_LH,
    ) -> Result<Vec<InterfaceCandidate>, InterfaceError> {
        let mut out = Vec::new();
        while !adapter.is_null() {
            // SAFETY: [Category 8 - FFI boundary] The linked list pointer is provided by a successful GetAdaptersAddresses call and remains valid while its backing buffer is alive.
            let current = unsafe { &*adapter };
            if let Some((ip, prefix_len)) = first_ipv4(current) {
                out.push(InterfaceCandidate {
                    name: adapter_name(current),
                    description: wide_string(current.Description.0),
                    ipv4: ip,
                    prefix_len,
                    is_up: current.OperStatus.0 == 1,
                    is_loopback: current.IfType == 24,
                    has_default_gateway: !current.FirstGatewayAddress.is_null(),
                });
            }
            adapter = current.Next;
        }
        Ok(out)
    }

    fn adapter_name(adapter: &IP_ADAPTER_ADDRESSES_LH) -> String {
        if adapter.AdapterName.0.is_null() {
            return String::new();
        }
        // SAFETY: [Category 8 - FFI boundary] Windows documents AdapterName as a null-terminated ANSI string for each adapter node.
        unsafe {
            CStr::from_ptr(adapter.AdapterName.0.cast())
                .to_string_lossy()
                .into_owned()
        }
    }

    fn wide_string(ptr: *const u16) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut len = 0usize;
        // SAFETY: [Category 8 - FFI boundary] Windows provides a null-terminated UTF-16 string pointer owned by the adapter buffer; we only scan to the terminator.
        unsafe {
            while *ptr.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
        }
    }

    fn first_ipv4(adapter: &IP_ADAPTER_ADDRESSES_LH) -> Option<(Ipv4Addr, u8)> {
        let unicast = adapter.FirstUnicastAddress;
        if !unicast.is_null() {
            // SAFETY: [Category 8 - FFI boundary] Unicast nodes are part of the adapter buffer returned by GetAdaptersAddresses.
            let addr = unsafe { &*unicast };
            // SAFETY: [Category 8 - FFI boundary] Address.lpSockaddr is valid for sockaddr access according to the unicast node length.
            let sock = unsafe { &*(addr.Address.lpSockaddr.cast::<SOCKADDR_IN>()) };
            // SAFETY: [Category 8 - FFI boundary] SOCKADDR_IN for AF_INET initializes the S_addr member of the IN_ADDR union.
            let raw_addr = unsafe { sock.sin_addr.S_un.S_addr };
            let octets = u32::from_be(raw_addr).to_be_bytes();
            return Some((Ipv4Addr::from(octets), addr.OnLinkPrefixLength));
        }
        None
    }
}
