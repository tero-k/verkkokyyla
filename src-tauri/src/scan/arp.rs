use std::collections::HashMap;
use std::net::Ipv4Addr;
#[cfg(unix)]
use std::process::Command;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ArpError {
    #[error("failed to read neighbor table: {0}")]
    Read(String),
}

pub async fn read_arp_table() -> Result<HashMap<Ipv4Addr, String>, ArpError> {
    tokio::task::spawn_blocking(read_arp_table_blocking)
        .await
        .map_err(|err| ArpError::Read(format!("ARP task failed: {err}")))?
}

#[cfg(unix)]
fn read_arp_table_blocking() -> Result<HashMap<Ipv4Addr, String>, ArpError> {
    let output = Command::new("arp").arg("-a").output();
    if let Ok(output) = output {
        let text = String::from_utf8_lossy(&output.stdout);
        let table = parse_arp_command_output(&text);
        if !table.is_empty() {
            return Ok(table);
        }
    }
    match std::fs::read_to_string("/proc/net/arp") {
        Ok(text) => Ok(parse_proc_net_arp(&text)),
        Err(err) => Err(ArpError::Read(err.to_string())),
    }
}

#[cfg(windows)]
fn read_arp_table_blocking() -> Result<HashMap<Ipv4Addr, String>, ArpError> {
    windows_impl::read_arp_table()
}

#[cfg(not(any(unix, windows)))]
fn read_arp_table_blocking() -> Result<HashMap<Ipv4Addr, String>, ArpError> {
    Ok(HashMap::new())
}

#[cfg(any(unix, test))]
pub(crate) fn parse_arp_command_output(input: &str) -> HashMap<Ipv4Addr, String> {
    input
        .lines()
        .filter_map(|line| {
            let ip = line
                .split_once('(')
                .and_then(|(_, rest)| rest.split_once(')'))
                .and_then(|(ip, _)| ip.parse::<Ipv4Addr>().ok())?;
            let mac = line
                .split_whitespace()
                .collect::<Vec<_>>()
                .windows(2)
                .find_map(|pair| (pair[0] == "at").then_some(pair[1]))?;
            normalize_mac(mac).map(|mac| (ip, mac))
        })
        .collect()
}

#[cfg(any(unix, test))]
pub(crate) fn parse_proc_net_arp(input: &str) -> HashMap<Ipv4Addr, String> {
    input
        .lines()
        .skip(1)
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let ip = parts.first()?.parse::<Ipv4Addr>().ok()?;
            let flags = parts.get(2).copied().unwrap_or_default();
            let mac = normalize_mac(parts.get(3).copied().unwrap_or_default())?;
            (flags != "0x0").then_some((ip, mac))
        })
        .collect()
}

fn normalize_mac(input: &str) -> Option<String> {
    let bytes: Vec<u8> = input
        .split(|ch: char| !ch.is_ascii_hexdigit())
        .filter(|part| !part.is_empty())
        .flat_map(|part| {
            if part.len() == 1 {
                vec![format!("0{part}")]
            } else if part.len() == 12 {
                // ASCII hex only (filtered above), so byte slicing is safe.
                (0..part.len())
                    .step_by(2)
                    .map(|i| part[i..i + 2].to_owned())
                    .collect()
            } else {
                vec![part.to_owned()]
            }
        })
        .map(|part| u8::from_str_radix(&part, 16).ok())
        .collect::<Option<Vec<_>>>()?;
    if bytes.len() != 6
        || bytes.iter().all(|byte| *byte == 0)
        || bytes.iter().all(|byte| *byte == u8::MAX)
    {
        return None;
    }
    Some(
        bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(":"),
    )
}

#[cfg(windows)]
mod windows_impl {
    use super::{normalize_mac, ArpError};
    use std::collections::HashMap;
    use std::net::Ipv4Addr;
    use windows::Win32::Foundation::NO_ERROR;
    use windows::Win32::NetworkManagement::IpHelper::{
        FreeMibTable, GetIpNetTable2, MIB_IPNET_TABLE2,
    };
    use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_INET};

    pub fn read_arp_table() -> Result<HashMap<Ipv4Addr, String>, ArpError> {
        let mut table: *mut MIB_IPNET_TABLE2 = std::ptr::null_mut();
        // SAFETY: [Category 8 - FFI boundary] GetIpNetTable2 initializes `table` on success for AF_INET and does not retain pointers into Rust memory.
        let code = unsafe { GetIpNetTable2(AF_INET, &mut table) };
        if code != NO_ERROR {
            return Err(ArpError::Read(format!("GetIpNetTable2 failed: {code:?}")));
        }
        let result = collect(table);
        // SAFETY: [Category 8 - FFI boundary] `table` was allocated by GetIpNetTable2 and is freed exactly once after rows are copied.
        unsafe { FreeMibTable(table.cast()) };
        Ok(result)
    }

    fn collect(table: *mut MIB_IPNET_TABLE2) -> HashMap<Ipv4Addr, String> {
        if table.is_null() {
            return HashMap::new();
        }
        // SAFETY: [Category 8 - FFI boundary] A successful GetIpNetTable2 call returns a valid MIB_IPNET_TABLE2 pointer until FreeMibTable.
        let table_ref = unsafe { &*table };
        let count = usize::try_from(table_ref.NumEntries).unwrap_or(0);
        // SAFETY: [Category 8 - FFI boundary] The Table field is the first element of a variable-length array of `count` rows allocated by GetIpNetTable2.
        let rows = unsafe { std::slice::from_raw_parts(table_ref.Table.as_ptr(), count) };
        rows.iter()
            .filter_map(|row| {
                let ip = sockaddr_ipv4(&row.Address)?;
                let len = usize::try_from(row.PhysicalAddressLength).ok()?;
                // SAFETY: [Category 8 - FFI boundary] `len` is the byte length reported by the row for its fixed-size PhysicalAddress array.
                let mac = unsafe { std::slice::from_raw_parts(row.PhysicalAddress.as_ptr(), len) }
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(":");
                normalize_mac(&mac).map(|mac| (ip, mac))
            })
            .collect()
    }

    fn sockaddr_ipv4(addr: &SOCKADDR_INET) -> Option<Ipv4Addr> {
        // SAFETY: [Category 8 - FFI boundary] Rows requested for AF_INET initialize the Ipv4 union field.
        let raw = unsafe { addr.Ipv4.sin_addr.S_un.S_addr };
        Some(Ipv4Addr::from(u32::from_be(raw).to_be_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::{parse_arp_command_output, parse_proc_net_arp};

    #[test]
    fn arp_command_parser_normalizes_macs_and_skips_incomplete_entries() {
        let input = "? (192.168.1.1) at aa-bb-cc-dd-ee-ff on en0\n? (192.168.1.2) at (incomplete) on en0\n? (192.168.1.3) at 0:11:22:33:44:55 on en0\n";

        let table = parse_arp_command_output(input);

        assert_eq!(
            table.get(&Ipv4Addr::new(192, 168, 1, 1)),
            Some(&"AA:BB:CC:DD:EE:FF".to_owned())
        );
        assert_eq!(
            table.get(&Ipv4Addr::new(192, 168, 1, 3)),
            Some(&"00:11:22:33:44:55".to_owned())
        );
        assert!(!table.contains_key(&Ipv4Addr::new(192, 168, 1, 2)));
    }

    #[test]
    fn proc_net_arp_parser_keeps_complete_entries_only() {
        let input = "IP address       HW type     Flags       HW address            Mask     Device\n192.168.1.1      0x1         0x2         aa:bb:cc:dd:ee:ff     *        eth0\n192.168.1.2      0x1         0x0         00:00:00:00:00:00     *        eth0\n";

        let table = parse_proc_net_arp(input);

        assert_eq!(table.len(), 1);
        assert_eq!(
            table.get(&Ipv4Addr::new(192, 168, 1, 1)),
            Some(&"AA:BB:CC:DD:EE:FF".to_owned())
        );
    }
}
