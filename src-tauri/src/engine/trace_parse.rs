#[derive(Clone, Debug, PartialEq)]
pub struct RawHop {
    pub hop: u32,
    pub address: Option<String>,
    pub rtts: Vec<Option<f64>>,
    pub annotation: Option<String>,
}

pub fn parse_tracert_line(_line: &str) -> Option<RawHop> {
    parse_line(_line, Flavor::Windows)
}

pub fn parse_traceroute_line(_line: &str) -> Option<RawHop> {
    parse_line(_line, Flavor::Posix)
}

enum Flavor {
    Windows,
    Posix,
}

fn parse_line(line: &str, flavor: Flavor) -> Option<RawHop> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let hop_token = tokens.first()?;
    if !hop_token.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let hop = hop_token.parse().ok()?;
    let rest = &tokens[1..];
    match flavor {
        Flavor::Windows => parse_windows_hop(hop, rest),
        Flavor::Posix => parse_posix_hop(hop, rest),
    }
}

fn parse_windows_hop(hop: u32, tokens: &[&str]) -> Option<RawHop> {
    let mut rtts = Vec::new();
    let mut address = None;
    let mut saw_payload = false;
    let mut index = 0;

    while index < tokens.len() {
        if rtts.len() < 3 {
            if let Some((rtt, consumed)) = parse_rtt_tokens(tokens, index) {
                rtts.push(rtt);
                saw_payload = true;
                index += consumed;
                continue;
            }
        }

        if address.is_none() && is_numeric_endpoint(tokens[index]) {
            address = Some(tokens[index].to_owned());
            saw_payload = true;
        }

        index += 1;
    }

    if !saw_payload {
        return None;
    }

    Some(RawHop {
        hop,
        address,
        rtts,
        annotation: None,
    })
}

fn parse_posix_hop(hop: u32, tokens: &[&str]) -> Option<RawHop> {
    let mut rtts = Vec::new();
    let mut address = None;
    let mut annotation = None;
    let mut saw_payload = false;
    let mut index = 0;

    while index < tokens.len() {
        if let Some((rtt, consumed)) = parse_rtt_tokens(tokens, index) {
            rtts.push(rtt);
            saw_payload = true;
            index += consumed;
            continue;
        }

        if annotation.is_none() && is_annotation(tokens[index]) {
            annotation = Some(tokens[index].to_owned());
            saw_payload = true;
            index += 1;
            continue;
        }

        if address.is_none() && is_numeric_endpoint(tokens[index]) {
            address = Some(tokens[index].to_owned());
            saw_payload = true;
            index += 1;
            continue;
        }

        index += 1;
    }

    if !saw_payload {
        return None;
    }

    Some(RawHop {
        hop,
        address,
        rtts,
        annotation,
    })
}

fn parse_rtt_tokens(tokens: &[&str], index: usize) -> Option<(Option<f64>, usize)> {
    let token = *tokens.get(index)?;

    if token == "*" {
        return Some((None, 1));
    }

    if token == "<1" && tokens.get(index + 1) == Some(&"ms") {
        return Some((Some(0.5), 2));
    }

    if token == "<" && tokens.get(index + 1) == Some(&"1") && tokens.get(index + 2) == Some(&"ms") {
        return Some((Some(0.5), 3));
    }

    if let Some(value) = token.strip_suffix("ms") {
        if let Ok(ms) = value.parse::<f64>() {
            return Some((Some(ms), 1));
        }
    }

    if let Ok(ms) = token.parse::<f64>() {
        if tokens.get(index + 1) == Some(&"ms") {
            return Some((Some(ms), 2));
        }
    }

    None
}

fn is_numeric_endpoint(token: &str) -> bool {
    let mut has_digit = false;
    let mut has_separator = false;
    for ch in token.chars() {
        if ch.is_ascii_digit() {
            has_digit = true;
            continue;
        }
        if matches!(ch, '.' | ':' | '[' | ']' | '%') {
            has_separator = true;
            continue;
        }
        if ch.is_ascii_hexdigit() {
            continue;
        }
        return false;
    }
    has_digit && (has_separator || token.chars().all(|ch| ch.is_ascii_digit()))
}

fn is_annotation(token: &str) -> bool {
    token.starts_with('!') && token.len() > 1
}
