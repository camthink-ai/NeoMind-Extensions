use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::Duration;

use crate::types::DiscoveryMatch;

const MULTICAST_ADDR: &str = "239.255.255.250";
const MULTICAST_PORT: u16 = 3702;

/// WS-Discovery probe message for ONVIF devices
fn build_probe_message() -> String {
    let message_id = uuid::Uuid::new_v4();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope"
            xmlns:a="http://www.w3.org/2005/08/addressing">
  <s:Header>
    <a:Action s:mustUnderstand="1">http://schemas.xmlsoap.org/ws/2005/04/discovery/Probe</a:Action>
    <a:MessageID>urn:uuid:{message_id}</a:MessageID>
    <a:ReplyTo><a:Address>http://schemas.xmlsoap.org/ws/2004/08/addressing/role/anonymous</a:Address></a:ReplyTo>
    <a:To s:mustUnderstand="1">urn:schemas-xmlsoap-org:ws:2005:04:discovery</a:To>
  </s:Header>
  <s:Body>
    <Probe xmlns="http://schemas.xmlsoap.org/ws/2005/04/discovery">
      <d:Types xmlns:d="http://schemas.xmlsoap.org/ws/2005/04/discovery"
               xmlns:dp0="http://www.onvif.org/ver10/network/wsdl">dp0:NetworkVideoTransmitter</d:Types>
    </Probe>
  </s:Body>
</s:Envelope>"#,
        message_id = message_id,
    )
}

/// Find XML body content, handling various namespace prefixes (s:, SOAP-ENV:, soap:, soapenv:, env:)
fn find_body_start(response: &str) -> Option<usize> {
    for prefix in &["s:", "SOAP-ENV:", "soap:", "soapenv:", "env:", ""] {
        let tag = format!("<{}Body>", prefix);
        if let Some(pos) = response.find(&tag) {
            return Some(pos);
        }
    }
    None
}

/// Extract text content from an XML element with flexible namespace prefix matching.
/// E.g., extract_tagged_content(xml, "XAddrs") matches <d:XAddrs>, <SOAP-ENV:XAddrs>, <XAddrs>, etc.
fn extract_tagged_content<'a>(xml: &'a str, local_name: &str) -> Option<&'a str> {
    for prefix in &["d:", "SOAP-ENV:", "soap:", "soapenv:", "env:", "s:", ""] {
        let open_tag = format!("<{}{}>", prefix, local_name);
        if let Some(pos) = xml.find(&open_tag) {
            let content_start = pos + open_tag.len();
            let close_tag = format!("</{}{}>", prefix, local_name);
            if let Some(end_pos) = xml[content_start..].find(&close_tag) {
                return Some(&xml[content_start..content_start + end_pos]);
            }
        }
    }
    None
}

/// Parse a WS-Discovery ProbeMatch response
fn parse_probe_matches(response: &str) -> Vec<DiscoveryMatch> {
    let mut matches = Vec::new();

    // Simple XML parsing for ProbeMatch elements
    // Look for Body with flexible namespace prefix
    if let Some(body_start) = find_body_start(response) {
        let body = &response[body_start..];

        // Find all ProbeMatch elements
        let mut search_pos = 0;
        while let Some(pm_start) = body[search_pos..].find("<ProbeMatch") {
            let pm_section = &body[search_pos + pm_start..];
            let pm_end = pm_section.find("</ProbeMatch>").unwrap_or(pm_section.len());
            let pm_content = &pm_section[..pm_end];

            let mut xaddrs = Vec::new();
            let mut scopes = Vec::new();
            let types = Vec::new();

            // Extract XAddrs — try multiple namespace prefixes
            let xa_content = extract_tagged_content(pm_content, "XAddrs");
            if let Some(content) = &xa_content {
                for addr in content.split_whitespace() {
                    if !addr.is_empty() && (addr.starts_with("http://") || addr.starts_with("https://")) {
                        xaddrs.push(addr.to_string());
                    }
                }
            }

            // Extract Scopes — try multiple namespace prefixes
            let sc_content = extract_tagged_content(pm_content, "Scopes");
            if let Some(content) = &sc_content {
                for scope in content.split_whitespace() {
                    if !scope.is_empty() {
                        scopes.push(scope.to_string());
                    }
                }
            }

            // Extract endpoint reference
            let endpoint = xaddrs.first().cloned().unwrap_or_default();

            if !endpoint.is_empty() {
                matches.push(DiscoveryMatch {
                    endpoint,
                    types,
                    scopes,
                    xaddrs,
                });
            }

            search_pos += pm_start + pm_end + "</ProbeMatch>".len();
        }
    }

    matches
}

/// Find a suitable local IPv4 address for multicast
fn find_local_ipv4() -> Option<Ipv4Addr> {
    // On macOS, multicast from 0.0.0.0 can fail with "No route to host"
    // Binding to a specific interface address fixes this
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // Try connecting to a public address (doesn't actually send packets)
    socket.connect("8.8.8.8:80").ok()?;
    let local = socket.local_addr().ok()?;
    match local {
        std::net::SocketAddr::V4(v4) => Some(*v4.ip()),
        _ => None,
    }
}

/// Discover ONVIF devices on the local network via WS-Discovery
pub fn discover_devices(timeout_ms: u64) -> Result<Vec<DiscoveryMatch>, String> {
    // Clamp timeout to reasonable range
    let timeout_ms = timeout_ms.clamp(500, 30_000);

    // Bind to a specific local interface to avoid "No route to host" on macOS
    let bind_addr = match find_local_ipv4() {
        Some(ip) => {
            eprintln!("[onvif-bridge] Binding multicast socket to {}", ip);
            SocketAddrV4::new(ip, 0)
        }
        None => {
            eprintln!("[onvif-bridge] Could not detect local IP, binding to 0.0.0.0");
            SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)
        }
    };

    let socket = UdpSocket::bind(bind_addr)
        .map_err(|e| format!("Failed to bind UDP socket: {}", e))?;

    // Set broadcast/multicast permissions
    socket.set_broadcast(true)
        .map_err(|e| format!("Failed to enable broadcast: {}", e))?;
    socket.set_multicast_ttl_v4(1)
        .map_err(|e| format!("Failed to set multicast TTL: {}", e))?;
    socket.set_read_timeout(Some(Duration::from_millis(timeout_ms)))
        .map_err(|e| format!("Failed to set read timeout: {}", e))?;

    let multicast_addr = SocketAddrV4::new(
        MULTICAST_ADDR.parse::<Ipv4Addr>().unwrap(),
        MULTICAST_PORT,
    );

    // Join multicast group on the detected interface
    if let Some(local_ip) = find_local_ipv4() {
        if let Err(e) = socket.join_multicast_v4(
            &MULTICAST_ADDR.parse::<Ipv4Addr>().unwrap(),
            &local_ip,
        ) {
            eprintln!("[onvif-bridge] Warning: could not join multicast group: {}", e);
        }
    }

    let probe = build_probe_message();
    if let Err(e) = socket.send_to(probe.as_bytes(), multicast_addr) {
        // Provide actionable guidance instead of raw OS error
        return Err(format!(
            "Failed to send WS-Discovery probe: {}. \
             Ensure your device is connected to a network that supports UDP multicast. \
             You can also try 'add_device' with a known camera URL instead.",
            e
        ));
    }

    let mut buf = [0u8; 8192];
    let mut discovered = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);

    loop {
        if std::time::Instant::now() >= deadline {
            break;
        }

        match socket.recv_from(&mut buf) {
            Ok((len, _addr)) => {
                let response = String::from_utf8_lossy(&buf[..len]);
                let matches = parse_probe_matches(&response);
                discovered.extend(matches);
            }
            Err(_) => break, // Timeout or error
        }
    }

    // Deduplicate by endpoint
    let mut seen = std::collections::HashSet::new();
    discovered.retain(|m| seen.insert(m.endpoint.clone()));

    Ok(discovered)
}
