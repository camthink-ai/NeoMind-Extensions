use crate::soap_client::{extract_tag, resolve_service_url, xml_escape};
use crate::types::OnvifDevice;

/// Send a PTZ relative move command
pub fn ptz_relative_move(
    device: &OnvifDevice,
    profile_token: &str,
    pan: f64,
    tilt: f64,
    zoom: f64,
    speed: f64,
) -> Result<(), String> {
    let service_url = resolve_ptz_url(&device.device_url);
    let body = format!(
        r#"<tptz:RelativeMove>
      <tptz:ProfileToken>{profile_token}</tptz:ProfileToken>
      <tptz:Translation>
        <tt:PanTilt x="{pan}" y="{tilt}" space="http://www.onvif.org/ver10/tptz/PanTiltSpaces/TranslationGenericSpace"/>
        <tt:Zoom x="{zoom}" space="http://www.onvif.org/ver10/tptz/ZoomSpaces/TranslationGenericSpace"/>
      </tptz:Translation>
      <tptz:Speed>
        <tt:PanTilt x="{speed}" y="{speed}" space="http://www.onvif.org/ver10/tptz/PanTiltSpaces/GenericSpeedSpace"/>
        <tt:Zoom x="{speed}" space="http://www.onvif.org/ver10/tptz/ZoomSpaces/ZoomGenericSpeedSpace"/>
      </tptz:Speed>
    </tptz:RelativeMove>"#,
        profile_token = xml_escape(profile_token),
        pan = pan,
        tilt = tilt,
        zoom = zoom,
        speed = speed,
    );

    crate::soap_client::soap_request_raw(
        &service_url,
        "http://www.onvif.org/ver20/ptz/wsdl/RelativeMove",
        &body,
        device.username.as_deref(),
        device.password.as_deref(),
    )?;

    Ok(())
}

/// Send a PTZ absolute move command
pub fn ptz_absolute_move(
    device: &OnvifDevice,
    profile_token: &str,
    pan: f64,
    tilt: f64,
    zoom: f64,
    speed: f64,
) -> Result<(), String> {
    let service_url = resolve_ptz_url(&device.device_url);
    let body = format!(
        r#"<tptz:AbsoluteMove>
      <tptz:ProfileToken>{profile_token}</tptz:ProfileToken>
      <tptz:Position>
        <tt:PanTilt x="{pan}" y="{tilt}" space="http://www.onvif.org/ver10/tptz/PanTiltSpaces/PositionGenericSpace"/>
        <tt:Zoom x="{zoom}" space="http://www.onvif.org/ver10/tptz/ZoomSpaces/PositionGenericSpace"/>
      </tptz:Position>
      <tptz:Speed>
        <tt:PanTilt x="{speed}" y="{speed}" space="http://www.onvif.org/ver10/tptz/PanTiltSpaces/GenericSpeedSpace"/>
        <tt:Zoom x="{speed}" space="http://www.onvif.org/ver10/tptz/ZoomSpaces/ZoomGenericSpeedSpace"/>
      </tptz:Speed>
    </tptz:AbsoluteMove>"#,
        profile_token = xml_escape(profile_token),
        pan = pan,
        tilt = tilt,
        zoom = zoom,
        speed = speed,
    );

    crate::soap_client::soap_request_raw(
        &service_url,
        "http://www.onvif.org/ver20/ptz/wsdl/AbsoluteMove",
        &body,
        device.username.as_deref(),
        device.password.as_deref(),
    )?;

    Ok(())
}

/// Stop PTZ movement
pub fn ptz_stop(device: &OnvifDevice, profile_token: &str) -> Result<(), String> {
    let service_url = resolve_ptz_url(&device.device_url);
    let body = format!(
        r#"<tptz:Stop>
      <tptz:ProfileToken>{profile_token}</tptz:ProfileToken>
      <tptz:PanTilt>true</tptz:PanTilt>
      <tptz:Zoom>true</tptz:Zoom>
    </tptz:Stop>"#,
        profile_token = xml_escape(profile_token),
    );

    crate::soap_client::soap_request_raw(
        &service_url,
        "http://www.onvif.org/ver20/ptz/wsdl/Stop",
        &body,
        device.username.as_deref(),
        device.password.as_deref(),
    )?;

    Ok(())
}

/// Go to home position
pub fn ptz_go_home(device: &OnvifDevice, profile_token: &str) -> Result<(), String> {
    let service_url = resolve_ptz_url(&device.device_url);
    let body = format!(
        r#"<tptz:GotoHomePosition>
      <tptz:ProfileToken>{profile_token}</tptz:ProfileToken>
      <tptz:Speed>
        <tt:PanTilt x="1.0" y="1.0" space="http://www.onvif.org/ver10/tptz/PanTiltSpaces/GenericSpeedSpace"/>
        <tt:Zoom x="1.0" space="http://www.onvif.org/ver10/tptz/ZoomSpaces/ZoomGenericSpeedSpace"/>
      </tptz:Speed>
    </tptz:GotoHomePosition>"#,
        profile_token = xml_escape(profile_token),
    );

    crate::soap_client::soap_request_raw(
        &service_url,
        "http://www.onvif.org/ver20/ptz/wsdl/GotoHomePosition",
        &body,
        device.username.as_deref(),
        device.password.as_deref(),
    )?;

    Ok(())
}

/// List PTZ presets
pub fn list_presets(device: &OnvifDevice, profile_token: &str) -> Result<Vec<serde_json::Value>, String> {
    let service_url = resolve_ptz_url(&device.device_url);
    let body = format!(
        r#"<tptz:GetPresets>
      <tptz:ProfileToken>{profile_token}</tptz:ProfileToken>
    </tptz:GetPresets>"#,
        profile_token = xml_escape(profile_token),
    );

    let response = crate::soap_client::soap_request_raw(
        &service_url,
        "http://www.onvif.org/ver20/ptz/wsdl/GetPresets",
        &body,
        device.username.as_deref(),
        device.password.as_deref(),
    )?;

    let mut presets = Vec::new();

    // Simple XML parsing for presets
    let mut pos = 0;
    while let Some(p_start) = response[pos..].find("<tptz:Preset") {
        let section = &response[pos + p_start..];
        let p_end = section.find("</tptz:Preset>").unwrap_or(section.len());
        let preset_section = &section[..p_end];

        let token = if let Some(attr_start) = preset_section.find("token=\"") {
            let rest = &preset_section[attr_start + 7..];
            if let Some(attr_end) = rest.find("\"") {
                rest[..attr_end].to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let name = extract_tag(preset_section, "tt:Name")
            .unwrap_or_else(|| "Unnamed Preset".to_string());

        presets.push(serde_json::json!({
            "token": token,
            "name": name,
        }));

        // Always advance past the closing tag to prevent infinite loops
        pos += p_start + p_end + "</tptz:Preset>".len();
    }

    Ok(presets)
}

/// Go to a PTZ preset
pub fn goto_preset(device: &OnvifDevice, profile_token: &str, preset_token: &str) -> Result<(), String> {
    let service_url = resolve_ptz_url(&device.device_url);
    let body = format!(
        r#"<tptz:GotoPreset>
      <tptz:ProfileToken>{profile_token}</tptz:ProfileToken>
      <tptz:PresetToken>{preset_token}</tptz:PresetToken>
      <tptz:Speed>
        <tt:PanTilt x="1.0" y="1.0" space="http://www.onvif.org/ver10/tptz/PanTiltSpaces/GenericSpeedSpace"/>
        <tt:Zoom x="1.0" space="http://www.onvif.org/ver10/tptz/ZoomSpaces/ZoomGenericSpeedSpace"/>
      </tptz:Speed>
    </tptz:GotoPreset>"#,
        profile_token = xml_escape(profile_token),
        preset_token = xml_escape(preset_token),
    );

    crate::soap_client::soap_request_raw(
        &service_url,
        "http://www.onvif.org/ver20/ptz/wsdl/GotoPreset",
        &body,
        device.username.as_deref(),
        device.password.as_deref(),
    )?;

    Ok(())
}

fn resolve_ptz_url(device_url: &str) -> String {
    resolve_service_url(device_url, "ptz")
}
