use crate::types::{OnvifDevice, OnvifProfile, VideoEncoderConfig};

/// Escape special XML characters in a string to prevent XML injection.
pub fn xml_escape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '&' => result.push_str("&amp;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&apos;"),
            _ => result.push(c),
        }
    }
    result
}

/// Compute ONVIF WS-Security PasswordDigest.
///
/// `Digest = Base64(SHA-1(Nonce + Created + Password))`
/// where Nonce is 16 random bytes.
fn compute_password_digest(password: &str) -> (String, String, String) {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;

    // Generate 16-byte random nonce
    let nonce_bytes = uuid::Uuid::new_v4();
    let nonce = nonce_bytes.as_bytes()[..16].to_vec();
    let nonce_b64 = engine.encode(&nonce);

    // ISO 8601 UTC timestamp
    let created = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();

    // SHA-1(nonce + created + password)
    use sha1::Digest;
    let mut hasher = sha1::Sha1::new();
    hasher.update(&nonce);
    hasher.update(created.as_bytes());
    hasher.update(password.as_bytes());
    let digest = hasher.finalize();
    let digest_b64 = engine.encode(digest);

    (nonce_b64, created, digest_b64)
}

/// Build the WS-Security header XML fragment.
fn build_security_header(username: &str, password: &str) -> String {
    let (nonce_b64, created, digest_b64) = compute_password_digest(password);
    format!(
        r#"<wsse:Security s:mustUnderstand="1">
      <wsse:UsernameToken>
        <wsse:Username>{username}</wsse:Username>
        <wsse:Password Type="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-username-token-profile-1.0#PasswordDigest">{digest}</wsse:Password>
        <wsse:Nonce EncodingType="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-soap-message-security-1.0#Base64Binary">{nonce}</wsse:Nonce>
        <wsu:Created>{created}</wsu:Created>
      </wsse:UsernameToken>
    </wsse:Security>"#,
        username = username,
        digest = digest_b64,
        nonce = nonce_b64,
        created = created,
    )
}

/// Send a SOAP request to an ONVIF device with WS-Security PasswordDigest auth.
/// Public so other modules (ptz.rs) can reuse it with custom URLs.
pub fn soap_request_raw(url: &str, action: &str, body: &str, username: Option<&str>, password: Option<&str>) -> Result<String, String> {
    let security_header = match (username, password) {
        (Some(user), Some(pass)) if !user.is_empty() && !pass.is_empty() => {
            Some(build_security_header(user, pass))
        }
        _ => None,
    };

    let header_content = match &security_header {
        Some(sec) => format!("  <s:Header>\n    {}\n  </s:Header>", sec),
        None => "  <s:Header/>".to_string(),
    };

    let envelope = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope"
            xmlns:tds="http://www.onvif.org/ver10/device/wsdl"
            xmlns:timg="http://www.onvif.org/ver20/imaging/wsdl"
            xmlns:trt="http://www.onvif.org/ver10/media/wsdl"
            xmlns:trt2="http://www.onvif.org/ver20/media/wsdl"
            xmlns:tptz="http://www.onvif.org/ver20/ptz/wsdl"
            xmlns:tt="http://www.onvif.org/ver10/schema"
            xmlns:wsse="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd"
            xmlns:wsu="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd">
{header}
  <s:Body>
    {body}
  </s:Body>
</s:Envelope>"#,
        header = header_content,
        body = body,
    );

    // SOAP 1.2 uses Content-Type action parameter
    let content_type = format!("application/soap+xml; charset=utf-8; action=\"{}\"", action);

    let req = ureq::post(url)
        .set("Content-Type", &content_type);

    let response = req.send_string(&envelope)
        .map_err(|e| format!("SOAP request failed: {}", e))?;

    let response_text = response.into_string()
        .map_err(|e| format!("Failed to read response: {}", e))?;

    // Limit response size to prevent memory exhaustion
    if response_text.len() > 10 * 1024 * 1024 {
        return Err("SOAP response too large (exceeds 10MB)".to_string());
    }

    // Check for SOAP Fault
    if let Some(fault) = extract_soap_fault(&response_text) {
        return Err(fault);
    }

    Ok(response_text)
}

/// Extract SOAP Fault message from response XML.
fn extract_soap_fault(xml: &str) -> Option<String> {
    // SOAP 1.2 fault structure:
    // <s:Fault><s:Code><s:Value>s:Sender</s:Value>
    //   <s:Subcode><s:Value>ter:NotAuthorized</s:Value></s:Subcode>
    // </s:Code><s:Reason><s:Text>...</s:Text></s:Reason></s:Fault>
    if !xml.contains(":Fault") && !xml.contains("<Fault") {
        return None;
    }

    // Find the <s:Code> block to extract code + subcode
    let code_block = extract_tag(xml, "s:Code").or_else(|| extract_tag(xml, "Code"));
    let code = if let Some(block) = &code_block {
        let top = extract_tag(block, "s:Value").or_else(|| extract_tag(block, "Value"))
            .unwrap_or_else(|| "Unknown".to_string());
        // Look for Subcode within the Code block
        if let Some(sub_block) = extract_tag(block, "s:Subcode").or_else(|| extract_tag(block, "Subcode")) {
            if let Some(sub_val) = extract_tag(&sub_block, "s:Value").or_else(|| extract_tag(&sub_block, "Value")) {
                format!("{} / {}", top, sub_val)
            } else {
                top
            }
        } else {
            top
        }
    } else {
        "Unknown".to_string()
    };

    let reason = extract_tag(xml, "s:Text")
        .or_else(|| extract_tag(xml, "Text"))
        .unwrap_or_else(|| "No reason provided".to_string());

    Some(format!("SOAP Fault: {} — {}", code, reason))
}

/// Extract text between XML tags, handling attributes in opening tags.
/// Matches both `<tag>content</tag>` and `<tag attr="...">content</tag>`
pub fn extract_tag(xml: &str, tag: &str) -> Option<String> {
    let close = format!("</{}>", tag);
    // Look for opening tag (may have attributes): <tag> or <tag ...
    let open_simple = format!("<{}>", tag);
    let open_with_attr = format!("<{} ", tag);

    let content_start = if let Some(pos) = xml.find(&open_simple) {
        pos + open_simple.len()
    } else if let Some(pos) = xml.find(&open_with_attr) {
        // Find the '>' closing the opening tag
        let after = &xml[pos + open_with_attr.len()..];
        let gt = after.find('>')?;
        pos + open_with_attr.len() + gt + 1
    } else {
        return None;
    };

    if content_start >= xml.len() {
        return None;
    }
    let remaining = &xml[content_start..];
    if let Some(end) = remaining.find(&close) {
        Some(remaining[..end].trim().to_string())
    } else {
        None
    }
}

/// Extract all occurrences of text between XML tags
#[allow(dead_code)]
fn extract_all_tags(xml: &str, tag: &str) -> Vec<String> {
    let mut results = Vec::new();
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let mut pos = 0;

    while let Some(start) = xml[pos..].find(&open) {
        let content_start = pos + start + open.len();
        if let Some(end) = xml[content_start..].find(&close) {
            results.push(xml[content_start..content_start + end].trim().to_string());
            pos = content_start + end + close.len();
        } else {
            break;
        }
    }

    results
}

/// Get device information
pub fn get_device_info(device: &OnvifDevice) -> Result<serde_json::Value, String> {
    let service_url = resolve_service_url(&device.device_url, "device");
    let body = r#"<tds:GetDeviceInformation/>"#;

    let response = soap_request_raw(
        &service_url,
        "http://www.onvif.org/ver10/device/wsdl/GetDeviceInformation",
        body,
        device.username.as_deref(),
        device.password.as_deref(),
    )?;

    Ok(serde_json::json!({
        "manufacturer": extract_tag(&response, "tt:Manufacturer").unwrap_or_default(),
        "model": extract_tag(&response, "tt:Model").unwrap_or_default(),
        "firmware_version": extract_tag(&response, "tt:FirmwareVersion").unwrap_or_default(),
        "serial_number": extract_tag(&response, "tt:SerialNumber").unwrap_or_default(),
        "hardware_id": extract_tag(&response, "tt:HardwareId").unwrap_or_default(),
    }))
}

/// Get media profiles from a device
pub fn get_profiles(device: &OnvifDevice) -> Result<Vec<OnvifProfile>, String> {
    let service_url = resolve_service_url(&device.device_url, "media");
    let body = r#"<trt:GetProfiles/>"#;

    let response = soap_request_raw(
        &service_url,
        "http://www.onvif.org/ver10/media/wsdl/GetProfiles",
        body,
        device.username.as_deref(),
        device.password.as_deref(),
    )?;

    let mut profiles = Vec::new();

    // Find profile sections
    let mut pos = 0;
    while let Some(p_start) = response[pos..].find("<trt:Profiles") {
        let section = &response[pos + p_start..];
        let p_end = section.find("</trt:Profiles>").unwrap_or(section.len());
        let profile_section = &section[..p_end];

        let token = extract_tag(profile_section, "token")
            .or_else(|| {
                // Try extracting from attribute
                if let Some(attr_start) = profile_section.find("token=\"") {
                    let rest = &profile_section[attr_start + 7..];
                    if let Some(attr_end) = rest.find("\"") {
                        Some(rest[..attr_end].to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .unwrap_or_else(|| format!("profile-{}", profiles.len()));

        let name = extract_tag(profile_section, "tt:Name")
            .unwrap_or_else(|| format!("Profile {}", profiles.len()));

        let video_source_token = extract_tag(profile_section, "tt:VideoSourceToken");

        let encoding = extract_tag(profile_section, "tt:Encoding").unwrap_or_default();
        let width = extract_tag(profile_section, "tt:Width")
            .and_then(|v| v.parse::<u32>().ok());
        let height = extract_tag(profile_section, "tt:Height")
            .and_then(|v| v.parse::<u32>().ok());
        let framerate = extract_tag(profile_section, "tt:FrameRateLimit")
            .and_then(|v| v.parse::<f64>().ok());
        let bitrate = extract_tag(profile_section, "tt:BitrateLimit")
            .and_then(|v| v.parse::<u32>().ok());

        let video_encoder = if !encoding.is_empty() {
            Some(VideoEncoderConfig {
                encoding,
                width: width.unwrap_or(1920),
                height: height.unwrap_or(1080),
                framerate: framerate.unwrap_or(30.0),
                bitrate: bitrate.unwrap_or(4000),
            })
        } else {
            None
        };

        profiles.push(OnvifProfile {
            token,
            name,
            video_source_token,
            video_encoder,
            stream_uri: None,
            snapshot_uri: None,
        });

        pos += p_start + p_end + "</trt:Profiles>".len();
    }

    Ok(profiles)
}

/// Get stream URI for a profile
pub fn get_stream_uri(device: &OnvifDevice, profile_token: &str, stream_type: &str) -> Result<String, String> {
    let service_url = resolve_service_url(&device.device_url, "media");
    let st = if stream_type == "RTP-Unicast" { "RTP-Unicast" } else { "RTP-Multicast" };
    let body = format!(
        r#"<trt:GetStreamUri>
      <trt:StreamSetup>
        <tt:Stream>{st}</tt:Stream>
        <tt:Transport>
          <tt:Protocol>RTSP</tt:Protocol>
        </tt:Transport>
      </trt:StreamSetup>
      <trt:ProfileToken>{profile_token}</trt:ProfileToken>
    </trt:GetStreamUri>"#,
        st = st,
        profile_token = xml_escape(profile_token),
    );

    let response = soap_request_raw(
        &service_url,
        "http://www.onvif.org/ver10/media/wsdl/GetStreamUri",
        &body,
        device.username.as_deref(),
        device.password.as_deref(),
    )?;

    extract_tag(&response, "tt:Uri")
        .ok_or_else(|| "Stream URI not found in response".to_string())
}

/// Get snapshot URI for a profile
pub fn get_snapshot_uri(device: &OnvifDevice, profile_token: &str) -> Result<String, String> {
    let service_url = resolve_service_url(&device.device_url, "media");
    let body = format!(
        r#"<trt:GetSnapshotUri>
      <trt:ProfileToken>{profile_token}</trt:ProfileToken>
    </trt:GetSnapshotUri>"#,
        profile_token = xml_escape(profile_token),
    );

    let response = soap_request_raw(
        &service_url,
        "http://www.onvif.org/ver10/media/wsdl/GetSnapshotUri",
        &body,
        device.username.as_deref(),
        device.password.as_deref(),
    )?;

    extract_tag(&response, "tt:Uri")
        .ok_or_else(|| "Snapshot URI not found in response".to_string())
}

/// Check if PTZ is supported for a profile
pub fn is_ptz_supported(device: &OnvifDevice) -> bool {
    // Try to get PTZ configuration - if it works, PTZ is supported
    let service_url = resolve_service_url(&device.device_url, "media");
    let body = r#"<trt:GetProfiles/>"#;

    let response = soap_request_raw(
        &service_url,
        "http://www.onvif.org/ver10/media/wsdl/GetProfiles",
        body,
        device.username.as_deref(),
        device.password.as_deref(),
    );

    match response {
        Ok(resp) => resp.contains("PTZToken") || resp.contains("tt:PTZ"),
        Err(_) => false,
    }
}

/// Resolve the service URL from device URL and service type.
/// Handles device URLs that already contain an ONVIF service path by replacing
/// the service suffix (e.g., `device_service` → `media_service`).
pub fn resolve_service_url(device_url: &str, service: &str) -> String {
    let base = device_url.trim_end_matches('/');

    let suffix = match service {
        "device" => "/onvif/device_service",
        "media" => "/onvif/media_service",
        "ptz" => "/onvif/ptz_service",
        _ => "/onvif/device_service",
    };

    // If the URL already has an ONVIF service path, replace it
    if let Some(slash_pos) = base.find("/onvif/") {
        return format!("{}{}", &base[..slash_pos], suffix);
    }

    // Otherwise append the standard path
    format!("{}{}", base, suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_digest_produces_valid_output() {
        let (nonce_b64, created, digest_b64) = compute_password_digest("mypassword");
        // Nonce should be valid base64 (16 bytes = 24 chars base64)
        assert_eq!(nonce_b64.len(), 24);
        // Created should be ISO 8601 format
        assert!(created.starts_with("20"));
        assert!(created.contains("T"));
        assert!(created.ends_with("Z"));
        // Digest should be valid base64 (20 bytes SHA-1 = 28 chars base64)
        assert_eq!(digest_b64.len(), 28);
        // Same password with same inputs should produce same digest
        let (_, _created2, digest2_b64) = compute_password_digest("mypassword");
        // Timestamps differ so digests differ — just verify format
        assert_eq!(digest2_b64.len(), 28);
    }

    #[test]
    fn test_security_header_contains_digest_type() {
        let header = build_security_header("admin", "secret123");
        assert!(header.contains("#PasswordDigest"));
        assert!(header.contains("<wsse:Username>admin</wsse:Username>"));
        assert!(header.contains("<wsse:Nonce"));
        assert!(header.contains("<wsu:Created>"));
    }

    #[test]
    fn test_extract_soap_fault() {
        let xml = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope">
  <s:Body>
    <s:Fault>
      <s:Code>
        <s:Value>s:Sender</s:Value>
        <s:Subcode>
          <s:Value>ter:NotAuthorized</s:Value>
        </s:Subcode>
      </s:Code>
      <s:Reason>
        <s:Text xml:lang="en">Device requires digest authentication</s:Text>
      </s:Reason>
    </s:Fault>
  </s:Body>
</s:Envelope>"#;

        let fault = extract_soap_fault(xml);
        assert!(fault.is_some());
        let msg = fault.unwrap();
        assert!(msg.contains("s:Sender"));
        assert!(msg.contains("ter:NotAuthorized")); // Subcode is included
        assert!(msg.contains("Device requires digest authentication"));
    }

    #[test]
    fn test_extract_soap_fault_none() {
        let xml = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope">
  <s:Body>
    <tds:GetDeviceInformationResponse>
      <tds:Manufacturer>Test</tds:Manufacturer>
    </tds:GetDeviceInformationResponse>
  </s:Body>
</s:Envelope>"#;

        assert!(extract_soap_fault(xml).is_none());
    }

    #[test]
    fn test_extract_tag_simple() {
        let xml = "<tt:Manufacturer>Hikvision</tt:Manufacturer>";
        assert_eq!(extract_tag(xml, "tt:Manufacturer"), Some("Hikvision".to_string()));
    }

    #[test]
    fn test_extract_tag_with_attributes() {
        let xml = r#"<trt:Profiles token="profile1"><tt:Name>Main</tt:Name></trt:Profiles>"#;
        // extract_tag should handle both simple and attribute forms
        assert_eq!(extract_tag(xml, "tt:Name"), Some("Main".to_string()));
    }

    #[test]
    fn test_resolve_service_url() {
        // Base URL — append service path
        assert_eq!(resolve_service_url("http://192.168.1.1", "device"),
                   "http://192.168.1.1/onvif/device_service");
        assert_eq!(resolve_service_url("http://192.168.1.1", "media"),
                   "http://192.168.1.1/onvif/media_service");
        assert_eq!(resolve_service_url("http://192.168.1.1", "ptz"),
                   "http://192.168.1.1/onvif/ptz_service");
        // Already has /onvif/ — replace service suffix
        assert_eq!(resolve_service_url("http://192.168.1.1/onvif/device_service", "media"),
                   "http://192.168.1.1/onvif/media_service");
        assert_eq!(resolve_service_url("http://192.168.1.1/onvif/device_service", "ptz"),
                   "http://192.168.1.1/onvif/ptz_service");
        // Same service — stays the same
        assert_eq!(resolve_service_url("http://192.168.1.1/onvif/device_service", "device"),
                   "http://192.168.1.1/onvif/device_service");
    }

    #[test]
    fn test_extract_tag_not_found() {
        assert_eq!(extract_tag("<foo>bar</foo>", "baz"), None);
    }
}
