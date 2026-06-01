//! Cayenne LPP and custom binary payload decoders.

use crate::types::{CustomDataType, CustomDecoderField, DecodedField};

// ---------------------------------------------------------------------------
// Cayenne LPP type codes
// ---------------------------------------------------------------------------

const LPP_DIGITAL_INPUT: u8 = 0x00;
const LPP_DIGITAL_OUTPUT: u8 = 0x01;
const LPP_ANALOG_IN: u8 = 0x02;
const LPP_TEMPERATURE: u8 = 0x67;
const LPP_HUMIDITY: u8 = 0x68;
const LPP_BAROMETER: u8 = 0x73;
const LPP_ILLUMINANCE: u8 = 0x65;

/// Data byte length for known Cayenne LPP type codes.
/// Used to skip unknown types gracefully instead of breaking.
const fn lpp_data_length(type_code: u8) -> Option<usize> {
    match type_code {
        // 1-byte types
        0x00 | 0x01 | 0x68 => Some(1),
        // 2-byte types
        0x02 | 0x03 | 0x04 | 0x05 | 0x65 | 0x67 | 0x73 => Some(2),
        // GPS (9 bytes: lat 3 + lon 3 + alt 3)
        0x06 => Some(9),
        // 4-byte types
        0x07 | 0x08 => Some(4),
        _ => None,
    }
}

/// Decode a Cayenne LPP payload into a list of named fields.
///
/// Cayenne LPP format per data point: `[channel, type_code, data_bytes...]`
pub fn decode_cayenne_lpp(payload: &[u8]) -> Vec<DecodedField> {
    let mut fields = Vec::new();
    let mut pos = 0;

    while pos + 2 < payload.len() {
        let _channel = payload[pos];
        let type_code = payload[pos + 1];
        pos += 2;

        match type_code {
            LPP_TEMPERATURE => {
                if pos + 2 <= payload.len() {
                    let raw = i16::from_be_bytes([payload[pos], payload[pos + 1]]);
                    fields.push(DecodedField {
                        name: "temperature".to_string(),
                        value: raw as f64 * 0.1,
                        unit: "\u{00b0}C".to_string(),
                    });
                    pos += 2;
                }
            }
            LPP_HUMIDITY => {
                if pos < payload.len() {
                    let raw = payload[pos];
                    fields.push(DecodedField {
                        name: "humidity".to_string(),
                        value: raw as f64 * 0.5,
                        unit: "%".to_string(),
                    });
                    pos += 1;
                }
            }
            LPP_BAROMETER => {
                if pos + 2 <= payload.len() {
                    let raw = u16::from_be_bytes([payload[pos], payload[pos + 1]]);
                    fields.push(DecodedField {
                        name: "barometric_pressure".to_string(),
                        value: raw as f64 * 0.1,
                        unit: "hPa".to_string(),
                    });
                    pos += 2;
                }
            }
            LPP_ILLUMINANCE => {
                if pos + 2 <= payload.len() {
                    let raw = u16::from_be_bytes([payload[pos], payload[pos + 1]]);
                    fields.push(DecodedField {
                        name: "illuminance".to_string(),
                        value: raw as f64,
                        unit: "lux".to_string(),
                    });
                    pos += 2;
                }
            }
            LPP_ANALOG_IN => {
                if pos + 2 <= payload.len() {
                    let raw = u16::from_be_bytes([payload[pos], payload[pos + 1]]);
                    fields.push(DecodedField {
                        name: "analog_in".to_string(),
                        value: raw as f64 * 0.01,
                        unit: "V".to_string(),
                    });
                    pos += 2;
                }
            }
            LPP_DIGITAL_INPUT => {
                if pos < payload.len() {
                    let raw = payload[pos];
                    fields.push(DecodedField {
                        name: "digital_input".to_string(),
                        value: raw as f64,
                        unit: String::new(),
                    });
                    pos += 1;
                }
            }
            LPP_DIGITAL_OUTPUT => {
                if pos < payload.len() {
                    let raw = payload[pos];
                    fields.push(DecodedField {
                        name: "digital_output".to_string(),
                        value: raw as f64,
                        unit: String::new(),
                    });
                    pos += 1;
                }
            }
            0x06 => {
                // GPS Location: 9 bytes — lat 3 + lon 3 + alt 3
                // Each value is a signed 24-bit integer.
                if pos + 9 <= payload.len() {
                    let lat_raw = sign_extend_24(payload[pos], payload[pos + 1], payload[pos + 2]);
                    let lon_raw = sign_extend_24(payload[pos + 3], payload[pos + 4], payload[pos + 5]);
                    let alt_raw = sign_extend_24(payload[pos + 6], payload[pos + 7], payload[pos + 8]);
                    fields.push(DecodedField {
                        name: "latitude".to_string(),
                        value: lat_raw as f64 * 0.0001,
                        unit: "\u{00b0}".to_string(),
                    });
                    fields.push(DecodedField {
                        name: "longitude".to_string(),
                        value: lon_raw as f64 * 0.0001,
                        unit: "\u{00b0}".to_string(),
                    });
                    fields.push(DecodedField {
                        name: "altitude".to_string(),
                        value: alt_raw as f64 * 0.01,
                        unit: "m".to_string(),
                    });
                    pos += 9;
                }
            }
            _ => {
                // Unknown type – skip over its data bytes if length is known
                if let Some(data_len) = lpp_data_length(type_code) {
                    if pos + data_len <= payload.len() {
                        pos += data_len;
                    } else {
                        // Not enough bytes remaining; stop parsing
                        break;
                    }
                } else {
                    // Truly unknown type with unknown length; stop parsing
                    break;
                }
            }
        }
    }

    fields
}

/// Decode a raw binary payload using a custom field descriptor list.
pub fn decode_custom(payload: &[u8], fields: &[CustomDecoderField]) -> Vec<DecodedField> {
    let mut result = Vec::new();

    for field in fields {
        let start = field.offset;
        let end = start + field.length;
        if end > payload.len() {
            continue;
        }

        let raw_bytes = &payload[start..end];
        let raw_value = match field.data_type {
            CustomDataType::Uint8 => {
                if field.length >= 1 {
                    raw_bytes[0] as f64
                } else {
                    continue;
                }
            }
            CustomDataType::Uint16 => {
                if field.length >= 2 {
                    u16::from_be_bytes([raw_bytes[0], raw_bytes[1]]) as f64
                } else {
                    continue;
                }
            }
            CustomDataType::Int16 => {
                if field.length >= 2 {
                    i16::from_be_bytes([raw_bytes[0], raw_bytes[1]]) as f64
                } else {
                    continue;
                }
            }
            CustomDataType::Uint32 => {
                if field.length >= 4 {
                    u32::from_be_bytes([raw_bytes[0], raw_bytes[1], raw_bytes[2], raw_bytes[3]])
                        as f64
                } else {
                    continue;
                }
            }
            CustomDataType::Int32 => {
                if field.length >= 4 {
                    i32::from_be_bytes([raw_bytes[0], raw_bytes[1], raw_bytes[2], raw_bytes[3]])
                        as f64
                } else {
                    continue;
                }
            }
        };

        let scaled = if field.scale != 0.0 {
            raw_value * field.scale
        } else {
            raw_value
        };

        result.push(DecodedField {
            name: field.name.clone(),
            value: scaled,
            unit: field.unit.clone(),
        });
    }

    result
}

/// Sign-extend a 24-bit big-endian value to i32.
/// Cayenne LPP GPS fields use signed 24-bit integers.
fn sign_extend_24(b0: u8, b1: u8, b2: u8) -> i32 {
    let sign = if b0 & 0x80 != 0 { 0xFF } else { 0x00 };
    i32::from_be_bytes([sign, b0, b1, b2])
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cayenne_temperature() {
        // Channel 0, type 0x67 (temperature), raw value 0x0199 = 409 => 40.9 °C
        let payload: Vec<u8> = vec![0x00, 0x67, 0x01, 0x99];
        let fields = decode_cayenne_lpp(&payload);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "temperature");
        assert!((fields[0].value - 40.9).abs() < 1e-9);
        assert_eq!(fields[0].unit, "\u{00b0}C");
    }

    #[test]
    fn test_cayenne_humidity() {
        // Channel 1, type 0x68 (humidity), raw 100 => 50.0 %
        let payload: Vec<u8> = vec![0x01, 0x68, 100];
        let fields = decode_cayenne_lpp(&payload);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "humidity");
        assert!((fields[0].value - 50.0).abs() < f64::EPSILON);
        assert_eq!(fields[0].unit, "%");
    }

    #[test]
    fn test_cayenne_multiple_fields() {
        // Channel 0 temp 0x0064 = 10.0 °C, channel 1 humidity 80 => 40.0 %
        let payload: Vec<u8> = vec![0x00, 0x67, 0x00, 0x64, 0x01, 0x68, 80];
        let fields = decode_cayenne_lpp(&payload);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "temperature");
        assert!((fields[0].value - 10.0).abs() < f64::EPSILON);
        assert_eq!(fields[1].name, "humidity");
        assert!((fields[1].value - 40.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_cayenne_barometer() {
        // Channel 0, type 0x73, raw 0x2710 = 10000 => 1000.0 hPa
        let payload: Vec<u8> = vec![0x00, 0x73, 0x27, 0x10];
        let fields = decode_cayenne_lpp(&payload);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "barometric_pressure");
        assert!((fields[0].value - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_cayenne_illuminance() {
        // Channel 0, type 0x65, raw 0x03E8 = 1000 lux
        let payload: Vec<u8> = vec![0x00, 0x65, 0x03, 0xE8];
        let fields = decode_cayenne_lpp(&payload);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "illuminance");
        assert!((fields[0].value - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_cayenne_empty_payload() {
        let fields = decode_cayenne_lpp(&[]);
        assert!(fields.is_empty());
    }

    #[test]
    fn test_cayenne_gps() {
        // Channel 1, GPS type 0x06, 9 bytes
        // Cayenne LPP GPS: signed 24-bit values, lat/lon scale 0.0001°, alt scale 0.01 m
        // Positive lat: 0x001000 = 4096 => 0.4096°
        // Negative lon: 0xFFF000 as signed 24-bit = -4096 => -0.4096°
        // Positive alt: 0x000100 = 256 => 2.56 m
        let payload: Vec<u8> = vec![
            0x01,             // channel 1
            0x06,             // GPS type
            0x00, 0x10, 0x00, // lat = +4096 => 0.4096°
            0xFF, 0xF0, 0x00, // lon = -4096 (signed 24-bit) => -0.4096°
            0x00, 0x01, 0x00, // alt = +256 => 2.56 m
        ];
        let fields = decode_cayenne_lpp(&payload);
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].name, "latitude");
        assert!((fields[0].value - 0.4096).abs() < 1e-9);
        assert_eq!(fields[1].name, "longitude");
        assert!((fields[1].value - (-0.4096)).abs() < 1e-9);
        assert_eq!(fields[2].name, "altitude");
        assert!((fields[2].value - 2.56).abs() < 1e-9);
    }

    #[test]
    fn test_custom_multi_field_decode() {
        // Build a 6-byte payload: uint16 (temperature), uint16 (humidity), uint8 (status), uint8 (count)
        let payload: Vec<u8> = vec![
            0x00, 0xC8, // offset 0, uint16 = 200
            0x00, 0x3C, // offset 2, uint16 = 60
            0x01,       // offset 4, uint8 = 1
            0x0A,       // offset 5, uint8 = 10
        ];

        let fields_def = vec![
            CustomDecoderField {
                offset: 0,
                length: 2,
                name: "temperature".to_string(),
                data_type: CustomDataType::Uint16,
                scale: 0.1,
                unit: "\u{00b0}C".to_string(),
            },
            CustomDecoderField {
                offset: 2,
                length: 2,
                name: "humidity".to_string(),
                data_type: CustomDataType::Uint16,
                scale: 1.0,
                unit: "%".to_string(),
            },
            CustomDecoderField {
                offset: 4,
                length: 1,
                name: "status".to_string(),
                data_type: CustomDataType::Uint8,
                scale: 0.0,
                unit: String::new(),
            },
            CustomDecoderField {
                offset: 5,
                length: 1,
                name: "count".to_string(),
                data_type: CustomDataType::Uint8,
                scale: 1.0,
                unit: String::new(),
            },
        ];

        let decoded = decode_custom(&payload, &fields_def);
        assert_eq!(decoded.len(), 4);

        assert_eq!(decoded[0].name, "temperature");
        assert!((decoded[0].value - 20.0).abs() < f64::EPSILON);

        assert_eq!(decoded[1].name, "humidity");
        assert!((decoded[1].value - 60.0).abs() < f64::EPSILON);

        assert_eq!(decoded[2].name, "status");
        assert!((decoded[2].value - 1.0).abs() < f64::EPSILON);

        assert_eq!(decoded[3].name, "count");
        assert!((decoded[3].value - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_custom_int16_negative() {
        // int16 negative value: 0xFF9C = -100, with scale 0.1 => -10.0
        let payload: Vec<u8> = vec![0xFF, 0x9C];
        let fields_def = vec![CustomDecoderField {
            offset: 0,
            length: 2,
            name: "temperature".to_string(),
            data_type: CustomDataType::Int16,
            scale: 0.1,
            unit: "\u{00b0}C".to_string(),
        }];
        let decoded = decode_custom(&payload, &fields_def);
        assert_eq!(decoded.len(), 1);
        assert!((decoded[0].value - (-10.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn test_custom_uint32() {
        let payload: Vec<u8> = vec![0x00, 0x00, 0x01, 0x00]; // 256
        let fields_def = vec![CustomDecoderField {
            offset: 0,
            length: 4,
            name: "counter".to_string(),
            data_type: CustomDataType::Uint32,
            scale: 1.0,
            unit: String::new(),
        }];
        let decoded = decode_custom(&payload, &fields_def);
        assert_eq!(decoded.len(), 1);
        assert!((decoded[0].value - 256.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_custom_out_of_bounds_skipped() {
        let payload: Vec<u8> = vec![0x00, 0x01]; // only 2 bytes
        let fields_def = vec![CustomDecoderField {
            offset: 0,
            length: 4, // requires 4 bytes but only 2 available
            name: "big_field".to_string(),
            data_type: CustomDataType::Uint32,
            scale: 1.0,
            unit: String::new(),
        }];
        let decoded = decode_custom(&payload, &fields_def);
        assert!(decoded.is_empty());
    }
}
