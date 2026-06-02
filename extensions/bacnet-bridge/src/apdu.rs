//! BACnet APDU encoding/decoding
//!
//! Implements the minimum subset needed for BACnet/IP operations:
//! - NPDU header (version, control)
//! - Who-Is (0x08) / I-Am (0x00)
//! - ReadProperty (0x0C) / ReadPropertyAck
//! - WriteProperty (0x0F) / WritePropertyAck
//! - SubscribeCOV (0x13) / SubscribeCOVAck
//! - ReadPropertyMultiple (0x0E) / ReadPropertyMultipleAck
//! - COVNotification (0x02/0x01)
//! - BACnet data type tags (Real, Integer, Boolean, String, Null)

use crate::types::{BacnetObjectType, BacnetValue};

// ---- NPDU Constants ----
const BACNET_PROTOCOL_VERSION: u8 = 1;
const BVLC_TYPE: u8 = 0x81;
const _BVLC_FUNCTION_RESULT: u8 = 0x00;
const BVLC_FUNCTION_ORIGINAL_UNICAST_NPDU: u8 = 0x0A;
const BVLC_FUNCTION_ORIGINAL_BROADCAST_NPDU: u8 = 0x0B;

// ---- APDU Service Codes (BACnet confirmed service choice values) ----
// Reference: ASHRAE 135-2020, Table 24-1
const SERVICE_CONFIRMED_SUBSCRIBE_COV: u8 = 0x05;
const SERVICE_CONFIRMED_READ_PROPERTY: u8 = 0x0C;
const SERVICE_CONFIRMED_READ_PROPERTY_MULTIPLE: u8 = 0x0E;
const SERVICE_CONFIRMED_WRITE_PROPERTY: u8 = 0x0F;
// Unconfirmed service choice values (Reference: ASHRAE 135-2020, Table 25-1)
const SERVICE_UNCONFIRMED_I_AM: u8 = 0x00;
const SERVICE_UNCONFIRMED_WHO_IS: u8 = 0x08;
const SERVICE_UNCONFIRMED_COV_NOTIFICATION: u8 = 0x02;
const SERVICE_CONFIRMED_COV_NOTIFICATION: u8 = 0x01;

// ---- APDU Types ----
const PDU_TYPE_CONFIRMED_REQUEST: u8 = 0x00;
const PDU_TYPE_UNCONFIRMED_REQUEST: u8 = 0x10;
const PDU_TYPE_SIMPLE_ACK: u8 = 0x20;
const PDU_TYPE_COMPLEX_ACK: u8 = 0x30;
const PDU_TYPE_ERROR: u8 = 0x50;
const _PDU_TYPE_REJECT: u8 = 0x60;
const _PDU_TYPE_ABORT: u8 = 0x70;

// ---- BACnet Property IDs ----
pub const PROPERTY_PRESENT_VALUE: u8 = 85;
pub const PROPERTY_OBJECT_NAME: u8 = 77;
pub const PROPERTY_DESCRIPTION: u8 = 28;
pub const PROPERTY_UNITS: u8 = 117;
pub const PROPERTY_OBJECT_IDENTIFIER: u8 = 75;
pub const _PROPERTY_OBJECT_TYPE: u8 = 79;
pub const PROPERTY_VENDOR_NAME: u8 = 121;
pub const PROPERTY_VENDOR_IDENTIFIER: u8 = 120;
pub const PROPERTY_MODEL_NAME: u8 = 70;
pub const PROPERTY_FIRMWARE_REVISION: u8 = 44;
pub const PROPERTY_MAX_APDU_LENGTH_ACCEPTED: u8 = 62;
pub const PROPERTY_SEGMENTATION_SUPPORTED: u8 = 107;
pub const _PROPERTY_SYSTEM_STATUS: u8 = 112;
pub const _PROPERTY_PROTOCOL_SERVICES_SUPPORTED: u8 = 97;
pub const _PROPERTY_PROTOCOL_OBJECT_TYPES_SUPPORTED: u8 = 96;

// ---- Application Tag Numbers ----
const TAG_NULL: u8 = 0;
const TAG_BOOLEAN: u8 = 1;
const TAG_UNSIGNED_INT: u8 = 2;
const TAG_SIGNED_INT: u8 = 3;
const TAG_REAL: u8 = 4;
const _TAG_DOUBLE: u8 = 5;
const _TAG_OCTET_STRING: u8 = 6;
const TAG_CHARACTER_STRING: u8 = 7;
const _TAG_BIT_STRING: u8 = 8;
const TAG_ENUMERATED: u8 = 9;
const _TAG_DATE: u8 = 10;
const _TAG_TIME: u8 = 11;
const TAG_OBJECT_IDENTIFIER: u8 = 12;

// ---- Helper: Tag encoding ----
// BACnet tag byte format: TTTT C LLL
//   T = tag number (4 bits, bits 7-4)
//   C = class bit (1 bit, bit 3: 0=application, 1=context-specific)
//   L = length/value (3 bits, bits 2-0: 0-4=short form length, 5=extended form)
fn encode_tag(tag_number: u8, is_context: bool, length: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    let class_bit = if is_context { 0x08 } else { 0x00 };

    if length <= 4 {
        // Short form: tag number + class + length in one byte
        buf.push((tag_number << 4) | class_bit | (length as u8));
    } else {
        // Extended form
        buf.push((tag_number << 4) | class_bit | 0x05);
        if length <= 253 {
            buf.push(length as u8);
        } else {
            buf.push(0xFE);
            buf.extend_from_slice(&(length as u32).to_be_bytes());
        }
    }
    buf
}

fn encode_context_tag(tag_number: u8, value: &BacnetValue) -> Vec<u8> {
    let mut buf = Vec::new();
    match value {
        BacnetValue::Null => {
            buf.push(0x08 | (tag_number << 4));
        }
        BacnetValue::Boolean(b) => {
            buf.push(0x08 | (tag_number << 4) | if *b { 1 } else { 0 });
        }
        BacnetValue::Unsigned(v) => {
            let bytes = encode_unsigned(*v);
            buf.extend(encode_tag(tag_number, true, bytes.len()));
            buf.extend(bytes);
        }
        BacnetValue::Integer(v) => {
            let bytes = encode_signed(*v);
            buf.extend(encode_tag(tag_number, true, bytes.len()));
            buf.extend(bytes);
        }
        BacnetValue::Real(v) => {
            buf.extend(encode_tag(tag_number, true, 4));
            buf.extend_from_slice(&(*v as f32).to_be_bytes());
        }
        BacnetValue::String(s) => {
            let str_bytes = s.as_bytes();
            buf.extend(encode_tag(tag_number, true, str_bytes.len()));
            buf.extend_from_slice(str_bytes);
        }
    }
    buf
}

fn encode_unsigned(v: u32) -> Vec<u8> {
    if v <= 0xFF {
        vec![v as u8]
    } else if v <= 0xFFFF {
        (v as u16).to_be_bytes().to_vec()
    } else {
        v.to_be_bytes().to_vec()
    }
}

fn encode_signed(v: i32) -> Vec<u8> {
    if v >= -128 && v <= 127 {
        vec![v as u8]
    } else if v >= -32768 && v <= 32767 {
        (v as i16).to_be_bytes().to_vec()
    } else {
        v.to_be_bytes().to_vec()
    }
}

/// Encode a BACnet object identifier as context tag 0 (for service request parameters)
pub fn encode_object_id(object_type: BacnetObjectType, instance: u32) -> Vec<u8> {
    let id: u32 = ((object_type.code() as u32) << 22) | (instance & 0x3FFFFF);
    // Context tag 0, 4 bytes: tag_number=0, class=context, length=4
    let mut buf = vec![0x0C]; // (0 << 4) | 0x08 | 4 = 0x0C
    buf.extend_from_slice(&id.to_be_bytes());
    buf
}

/// Decode a BACnet object identifier from bytes
pub fn decode_object_id(bytes: &[u8]) -> Option<(BacnetObjectType, u32)> {
    if bytes.len() < 4 {
        return None;
    }
    let id = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let type_code = (id >> 22) as u16;
    let instance = id & 0x3FFFFF;
    BacnetObjectType::from_code(type_code).map(|t| (t, instance))
}

/// Build BVLC + NPDU header for unicast
fn build_unicast_header(payload_len: usize) -> Vec<u8> {
    let total_len = 4 + 2 + payload_len; // BVLC(4) + NPDU(2) + APDU
    let mut buf = Vec::with_capacity(total_len);
    // BVLC header
    buf.push(BVLC_TYPE);
    buf.push(BVLC_FUNCTION_ORIGINAL_UNICAST_NPDU);
    buf.extend_from_slice(&(total_len as u16).to_be_bytes());
    // NPDU header
    buf.push(BACNET_PROTOCOL_VERSION);
    buf.push(0x00); // Control: no destination, no source, no hop count
    buf
}

/// Build BVLC + NPDU header for broadcast
fn build_broadcast_header(payload_len: usize) -> Vec<u8> {
    let total_len = 4 + 2 + payload_len;
    let mut buf = Vec::with_capacity(total_len);
    buf.push(BVLC_TYPE);
    buf.push(BVLC_FUNCTION_ORIGINAL_BROADCAST_NPDU);
    buf.extend_from_slice(&(total_len as u16).to_be_bytes());
    buf.push(BACNET_PROTOCOL_VERSION);
    buf.push(0x00);
    buf
}

// ---- Public Message Builders ----

/// Build a Who-Is broadcast message
pub fn build_who_is(low: u32, high: u32) -> Vec<u8> {
    // Unconfirmed-Request-PDU: byte 0 = PDU type (0x10), byte 1 = service choice
    let mut apdu = vec![PDU_TYPE_UNCONFIRMED_REQUEST];
    apdu.push(SERVICE_UNCONFIRMED_WHO_IS);

    // Device ID range
    apdu.extend(encode_context_tag(0, &BacnetValue::Unsigned(low)));
    apdu.extend(encode_context_tag(1, &BacnetValue::Unsigned(high)));

    let mut msg = build_broadcast_header(apdu.len());
    msg.extend(apdu);
    msg
}

/// Build a ReadProperty request
pub fn build_read_property(
    _device_id: u32,
    object_type: BacnetObjectType,
    instance: u32,
    property_id: u8,
) -> Vec<u8> {
    let invoke_id = next_invoke_id();

    // Confirmed-Request-PDU:
    //   Byte 0: PDU type (0x00) | SAQ bit (0x01 = segmented response accepted)
    //   Byte 1: Max segments (3 bits) | Max APDU size (5 bits) — 0x05 = unspecified / 1476
    //   Byte 2: Invoke ID
    //   Byte 3: Service choice
    let mut apdu = vec![
        PDU_TYPE_CONFIRMED_REQUEST | 0x08, // SAQ=1 (accept segmented responses), bit 3
        0x05,                              // max segments=unspecified(0), max APDU=1476(5)
        invoke_id,
        SERVICE_CONFIRMED_READ_PROPERTY,
    ];

    // Object identifier
    apdu.extend(encode_object_id(object_type, instance));
    // Property identifier
    apdu.extend(encode_context_tag(
        1,
        &BacnetValue::Unsigned(property_id as u32),
    ));

    let mut msg = build_unicast_header(apdu.len());
    msg.extend(apdu);
    msg
}

/// Build a WriteProperty request
pub fn build_write_property(
    _device_id: u32,
    object_type: BacnetObjectType,
    instance: u32,
    property_id: u8,
    value: &BacnetValue,
    priority: Option<u8>,
) -> Vec<u8> {
    let invoke_id = next_invoke_id();

    let mut apdu = vec![
        PDU_TYPE_CONFIRMED_REQUEST | 0x08, // SAQ=1 (bit 3)
        0x05,
        invoke_id,
        SERVICE_CONFIRMED_WRITE_PROPERTY,
    ];

    // Object identifier
    apdu.extend(encode_object_id(object_type, instance));
    // Property identifier
    apdu.extend(encode_context_tag(
        1,
        &BacnetValue::Unsigned(property_id as u32),
    ));
    // Property value (opening tag)
    apdu.push(0x3E); // Context tag 3, opening
    // Value
    apdu.extend(encode_application_value(value));
    // Property value (closing tag)
    apdu.push(0x3F); // Context tag 3, closing
    // Priority
    if let Some(p) = priority {
        apdu.extend(encode_context_tag(
            4,
            &BacnetValue::Unsigned(p as u32),
        ));
    }

    let mut msg = build_unicast_header(apdu.len());
    msg.extend(apdu);
    msg
}

/// Build a SubscribeCOV request
pub fn build_subscribe_cov(
    subscriber_id: u32,
    _device_id: u32,
    object_type: BacnetObjectType,
    instance: u32,
    lifetime: u32,
    confirmed: bool,
) -> Vec<u8> {
    let invoke_id = next_invoke_id();

    let mut apdu = vec![
        PDU_TYPE_CONFIRMED_REQUEST | 0x08, // SAQ=1 (bit 3)
        0x05,
        invoke_id,
        SERVICE_CONFIRMED_SUBSCRIBE_COV, // Service code 5 (SubscribeCOV)
    ];

    // Subscriber process identifier (context tag 0)
    apdu.extend(encode_context_tag(
        0,
        &BacnetValue::Unsigned(subscriber_id),
    ));
    // Monitored object identifier (context tag 1)
    apdu.extend(encode_object_id(object_type, instance));
    // Issue confirmed notifications (context tag 2)
    apdu.extend(encode_context_tag(2, &BacnetValue::Boolean(confirmed)));
    // Lifetime (context tag 3)
    if lifetime > 0 {
        apdu.extend(encode_context_tag(3, &BacnetValue::Unsigned(lifetime)));
    }

    let mut msg = build_unicast_header(apdu.len());
    msg.extend(apdu);
    msg
}

/// Build a ReadPropertyMultiple request
pub fn build_read_property_multiple(
    _device_id: u32,
    reads: &[(BacnetObjectType, u32, Vec<u8>)],
) -> Vec<u8> {
    let invoke_id = next_invoke_id();

    let mut apdu = vec![
        PDU_TYPE_CONFIRMED_REQUEST | 0x08, // SAQ=1 (bit 3)
        0x05,
        invoke_id,
        SERVICE_CONFIRMED_READ_PROPERTY_MULTIPLE,
    ];

    for (obj_type, instance, properties) in reads {
        // Object identifier
        apdu.extend(encode_object_id(*obj_type, *instance));
        // List of property references (opening tag context 0)
        apdu.push(0x1E); // opening tag context 0
        for prop_id in properties {
            apdu.extend(encode_context_tag(
                0,
                &BacnetValue::Unsigned(*prop_id as u32),
            ));
        }
        apdu.push(0x1F); // closing tag context 0
    }

    let mut msg = build_unicast_header(apdu.len());
    msg.extend(apdu);
    msg
}

fn encode_application_value(value: &BacnetValue) -> Vec<u8> {
    match value {
        // Null: tag number 0, application class, length 0
        BacnetValue::Null => vec![0x00],
        // Boolean: tag number 1, application class, length/value = 0 or 1
        BacnetValue::Boolean(b) => vec![(1 << 4) | if *b { 1 } else { 0 }],
        BacnetValue::Unsigned(v) => {
            let bytes = encode_unsigned(*v);
            let mut buf = encode_tag(TAG_UNSIGNED_INT, false, bytes.len());
            buf.extend(bytes);
            buf
        }
        BacnetValue::Integer(v) => {
            let bytes = encode_signed(*v);
            let mut buf = encode_tag(TAG_SIGNED_INT, false, bytes.len());
            buf.extend(bytes);
            buf
        }
        BacnetValue::Real(v) => {
            let mut buf = encode_tag(TAG_REAL, false, 4);
            buf.extend_from_slice(&(*v as f32).to_be_bytes());
            buf
        }
        BacnetValue::String(s) => {
            let str_bytes = s.as_bytes();
            let total_len = 1 + str_bytes.len(); // encoding byte + string
            let mut buf = encode_tag(TAG_CHARACTER_STRING, false, total_len);
            buf.push(0x00); // Encoding type: ANSI X3.4
            buf.extend_from_slice(str_bytes);
            buf
        }
    }
}

// ---- Invoke ID Generator ----
use std::sync::atomic::{AtomicU8, Ordering};
static INVOKE_ID_COUNTER: AtomicU8 = AtomicU8::new(0);

fn next_invoke_id() -> u8 {
    // BACnet invoke IDs are 0-254 (255 is reserved)
    let id = INVOKE_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
    // Wrap at 255 to avoid using the reserved value
    id.min(254)
}

// ---- Response Parsing ----

/// Parsed APDU response
#[derive(Debug)]
pub enum ApduResponse {
    IAm {
        device_id: u32,
        max_apdu: u32,
        segmentation: u32,
        vendor_id: u32,
    },
    ReadPropertyAck {
        object_type: BacnetObjectType,
        instance: u32,
        property_id: u8,
        value: BacnetValue,
    },
    ReadPropertyMultipleAck {
        values: Vec<(BacnetObjectType, u32, u8, BacnetValue)>,
    },
    SimpleAck {
        invoke_id: u8,
        service: u8,
    },
    CovNotification {
        subscriber_id: u32,
        device_id: u32,
        object_type: BacnetObjectType,
        instance: u32,
        values: Vec<(u8, BacnetValue)>,
    },
    Error {
        invoke_id: u8,
        error_class: u8,
        error_code: u8,
    },
    Unknown {
        pdu_type: u8,
    },
}

/// Parse a received BACnet message
pub fn parse_response(data: &[u8]) -> Option<ApduResponse> {
    if data.len() < 6 {
        return None;
    }

    // Skip BVLC header (4 bytes)
    // Check BVLC type and validate length
    if data[0] != BVLC_TYPE {
        return None;
    }
    let bvlc_length = u16::from_be_bytes([data[2], data[3]]) as usize;
    if bvlc_length > 2048 || data.len() < bvlc_length {
        return None; // Oversized or truncated packet
    }

    // Skip NPDU header
    let npdu_start = 4;
    if data.len() <= npdu_start + 1 {
        return None;
    }
    // data[npdu_start] = protocol version, data[npdu_start+1] = control

    let apdu_start = npdu_start + 2;
    if data.len() <= apdu_start {
        return None;
    }

    let pdu_type_byte = data[apdu_start];
    let pdu_type = pdu_type_byte & 0xF0;

    match pdu_type {
        PDU_TYPE_CONFIRMED_REQUEST => {
            // Confirmed-Request-PDU: SAQ (1 byte), invoke_id (1 byte), service_choice (1 byte), service_data...
            // Used for ConfirmedCOVNotification (service 0x01)
            if data.len() > apdu_start + 3 {
                let saq = data[apdu_start];
                let has_segmentation = (saq & 0x08) != 0;
                let header_len = if has_segmentation { 5 } else { 3 };
                if data.len() > apdu_start + header_len {
                    let service = data[apdu_start + header_len - 1];
                    if service == SERVICE_CONFIRMED_COV_NOTIFICATION {
                        let remaining = &data[apdu_start + header_len..];
                        parse_cov_notification(remaining)
                    } else {
                        Some(ApduResponse::Unknown { pdu_type })
                    }
                } else {
                    None
                }
            } else {
                None
            }
        }
        PDU_TYPE_UNCONFIRMED_REQUEST => {
            // Unconfirmed-Request-PDU: byte 0 = PDU type, byte 1 = service choice
            if data.len() > apdu_start + 1 {
                let service = data[apdu_start + 1];
                match service {
                    SERVICE_UNCONFIRMED_I_AM => {
                        let remaining = &data[apdu_start + 2..];
                        parse_i_am(remaining)
                    }
                    SERVICE_UNCONFIRMED_COV_NOTIFICATION => {
                        let remaining = &data[apdu_start + 2..];
                        parse_cov_notification(remaining)
                    }
                    _ => None,
                }
            } else {
                None
            }
        }
        PDU_TYPE_SIMPLE_ACK => {
            if data.len() > apdu_start + 2 {
                Some(ApduResponse::SimpleAck {
                    invoke_id: data[apdu_start + 1],
                    service: data[apdu_start + 2],
                })
            } else {
                None
            }
        }
        PDU_TYPE_COMPLEX_ACK => {
            if data.len() > apdu_start + 2 {
                let _invoke_id = data[apdu_start + 1];
                let service_ack = data[apdu_start + 2];

                if service_ack == SERVICE_CONFIRMED_READ_PROPERTY {
                    parse_read_property_ack(&data[apdu_start + 3..])
                } else if service_ack == SERVICE_CONFIRMED_READ_PROPERTY_MULTIPLE {
                    parse_read_property_multiple_ack(&data[apdu_start + 3..])
                } else {
                    None
                }
            } else {
                None
            }
        }
        PDU_TYPE_ERROR => {
            // Error PDU: invoke_id (1 byte), Error Class (tag 9 + value), Error Code (tag 9 + value)
            // Each error field is application-tagged enumerated (tag 9, typically length 1)
            if data.len() > apdu_start + 6 {
                let _class_tag = data[apdu_start + 2]; // e.g. 0x91 (tag 9, length 1)
                let error_class = data[apdu_start + 3];
                let _code_tag = data[apdu_start + 4]; // e.g. 0x91
                let error_code = data[apdu_start + 5];
                Some(ApduResponse::Error {
                    invoke_id: data[apdu_start + 1],
                    error_class,
                    error_code,
                })
            } else if data.len() > apdu_start + 3 {
                // Fallback for non-standard encoding
                Some(ApduResponse::Error {
                    invoke_id: data[apdu_start + 1],
                    error_class: data[apdu_start + 2],
                    error_code: data[apdu_start + 3],
                })
            } else {
                None
            }
        }
        _ => Some(ApduResponse::Unknown { pdu_type }),
    }
}

fn parse_i_am(data: &[u8]) -> Option<ApduResponse> {
    let mut pos = 0;

    // Object identifier (device) — application tag 12, length 4
    if pos + 5 > data.len() {
        return None;
    }
    let tag = data[pos];
    let tag_num = (tag >> 4) & 0x0F;
    let tag_len = (tag & 0x07) as usize;
    if tag_num != TAG_OBJECT_IDENTIFIER || tag_len != 4 {
        return None;
    }
    pos += 1;
    let device_id_raw = u32::from_be_bytes([
        data[pos],
        data[pos + 1],
        data[pos + 2],
        data[pos + 3],
    ]);
    let _device_type = (device_id_raw >> 22) as u16;
    let device_id = device_id_raw & 0x3FFFFF;
    pos += 4;

    // Helper: skip an application tag and read a single-byte unsigned value
    // I-Am has: Max APDU (tag 2), Segmentation (tag 2), Vendor ID (tag 2)
    let read_tagged_byte = |data: &[u8], pos: &mut usize| -> Option<u32> {
        if *pos >= data.len() {
            return None;
        }
        let t = data[*pos];
        let len = (t & 0x07) as usize;
        *pos += 1;
        if *pos + len > data.len() {
            return None;
        }
        let val = match len {
            1 => data[*pos] as u32,
            2 => u16::from_be_bytes([data[*pos], data[*pos + 1]]) as u32,
            _ => return None,
        };
        *pos += len;
        Some(val)
    };

    // Max APDU length (application tag 2, unsigned)
    let max_apdu = read_tagged_byte(data, &mut pos)?;
    // Segmentation supported (application tag 2, unsigned)
    let segmentation = read_tagged_byte(data, &mut pos)?;
    // Vendor ID (application tag 2, unsigned)
    let vendor_id = read_tagged_byte(data, &mut pos)?;

    Some(ApduResponse::IAm {
        device_id,
        max_apdu,
        segmentation,
        vendor_id,
    })
}

/// Parse COV Notification (Unconfirmed or Confirmed).
/// Per ASHRAE 135-2020 Clause 13.3:
///   - Subscriber Process Identifier (context tag 0, unsigned)
///   - Initiating Device Identifier (context tag 1, ObjectID)
///   - Monitored Object Identifier (context tag 2, ObjectID)
///   - Time of notification (context tag 3, optional — skip)
///   - List of Values (context tag 4, opening/closing) containing property references
fn parse_cov_notification(data: &[u8]) -> Option<ApduResponse> {
    let mut pos = 0;

    // Context tag 0: subscriber process identifier (unsigned, 1-4 bytes)
    if pos >= data.len() { return None; }
    let tag0 = data[pos];
    if (tag0 >> 4) & 0x0F != 0 || (tag0 & 0x08) == 0 { return None; }
    let sub_len = (tag0 & 0x07) as usize;
    if pos + 1 + sub_len > data.len() { return None; }
    let subscriber_id = match sub_len {
        1 => data[pos + 1] as u32,
        2 => u16::from_be_bytes([data[pos + 1], data[pos + 2]]) as u32,
        4 => u32::from_be_bytes([data[pos + 1], data[pos + 2], data[pos + 3], data[pos + 4]]),
        _ => return None,
    };
    pos += 1 + sub_len;

    // Context tag 1: initiating device identifier (ObjectID, 4 bytes)
    if pos + 5 > data.len() { return None; }
    let tag1 = data[pos];
    if (tag1 >> 4) & 0x0F != 1 || (tag1 & 0x08) == 0 || (tag1 & 0x07) != 4 { return None; }
    let dev_raw = u32::from_be_bytes([data[pos + 1], data[pos + 2], data[pos + 3], data[pos + 4]]);
    let device_id = dev_raw & 0x3FFFFF;
    pos += 5;

    // Context tag 2: monitored object identifier (ObjectID, 4 bytes)
    if pos + 5 > data.len() { return None; }
    let tag2 = data[pos];
    if (tag2 >> 4) & 0x0F != 2 || (tag2 & 0x08) == 0 || (tag2 & 0x07) != 4 { return None; }
    let obj_raw = u32::from_be_bytes([data[pos + 1], data[pos + 2], data[pos + 3], data[pos + 4]]);
    let obj_type_code = (obj_raw >> 22) as u16;
    let instance = obj_raw & 0x3FFFFF;
    let object_type = BacnetObjectType::from_code(obj_type_code)?;
    pos += 5;

    // Optional: context tag 3 = time of notification (skip if present)
    if pos < data.len() {
        let maybe_tag3 = data[pos];
        if (maybe_tag3 >> 4) & 0x0F == 3 && (maybe_tag3 & 0x08) != 0 {
            let tlen = (maybe_tag3 & 0x07) as usize;
            if tlen == 0 {
                // opening tag or zero-length — just skip the byte
                pos += 1;
            } else {
                pos += 1 + tlen;
            }
        }
    }

    // Context tag 4: opening tag for list of values (0x4E)
    if pos >= data.len() || data[pos] != 0x4E { return None; }
    pos += 1;

    let mut values = Vec::new();

    // Parse property value entries until closing tag 0x4F
    while pos < data.len() && data[pos] != 0x4F {
        // Each entry: context tag 0 opening (0x0E), property identifier, property value, context tag 0 closing (0x0F)
        if data[pos] != 0x0E {
            // Skip unknown tag — ensure minimum progress
            let skip_len = ((data[pos] & 0x07) as usize).max(1);
            pos += 1 + skip_len;
            if pos >= data.len() { break; }
            continue;
        }
        pos += 1; // skip opening tag

        // Property identifier: application-tagged unsigned (tag 2)
        if pos >= data.len() { break; }
        let prop_tag = data[pos];
        let prop_tag_num = (prop_tag >> 4) & 0x0F;
        let prop_len = (prop_tag & 0x07) as usize;
        if prop_tag_num != 2 || pos + 1 + prop_len > data.len() { break; }
        let property_id = match prop_len {
            1 => data[pos + 1],
            _ => data[pos + 1], // simplified — most property IDs fit in 1 byte
        };
        pos += 1 + prop_len;

        // Property value: application-tagged
        if pos >= data.len() { break; }
        let value = parse_application_value(&data[pos..]);
        if let Some(consumed) = application_value_consumed(&data[pos..]) {
            pos += consumed;
        } else {
            // Can't parse — skip to closing tag
            pos += 1;
        }

        // Context tag 0 closing (0x0F)
        if pos < data.len() && data[pos] == 0x0F {
            pos += 1;
        }

        if let Some(v) = value {
            values.push((property_id, v));
        }
    }

    Some(ApduResponse::CovNotification {
        subscriber_id,
        device_id,
        object_type,
        instance,
        values,
    })
}

fn parse_read_property_ack(data: &[u8]) -> Option<ApduResponse> {
    let mut pos = 0;

    // Object identifier: context tag 0, length 4 (tag byte = 0x0C)
    if pos + 5 > data.len() {
        return None;
    }
    let tag = data[pos];
    let tag_number = (tag >> 4) & 0x0F;
    let is_context = (tag & 0x08) != 0;
    let length = tag & 0x07;
    if tag_number != 0 || !is_context || length != 4 {
        return None;
    }
    pos += 1;
    let obj_raw = u32::from_be_bytes([
        data[pos],
        data[pos + 1],
        data[pos + 2],
        data[pos + 3],
    ]);
    let obj_type_code = (obj_raw >> 22) as u16;
    let instance = obj_raw & 0x3FFFFF;
    let obj_type = BacnetObjectType::from_code(obj_type_code)?;
    pos += 4;

    // Property identifier: context tag 1, length 1 (tag byte = 0x19)
    if pos + 1 >= data.len() {
        return None;
    }
    let prop_tag = data[pos];
    let prop_tag_number = (prop_tag >> 4) & 0x0F;
    let prop_is_context = (prop_tag & 0x08) != 0;
    let prop_length = prop_tag & 0x07;
    if prop_tag_number != 1 || !prop_is_context {
        return None;
    }
    pos += 1;
    if pos + prop_length as usize > data.len() {
        return None;
    }
    // Property ID is encoded as unsigned int with the given length
    let property_id = match prop_length {
        1 => data[pos],
        2 => {
            let v = u16::from_be_bytes([data[pos], data[pos + 1]]);
            if v > 255 { return None; } // Property IDs > 255 not supported in this parser
            v as u8
        }
        _ => return None,
    };
    pos += prop_length as usize;

    // Skip optional Property Array Index (context tag 2)
    if pos < data.len() {
        let maybe_array_tag = data[pos];
        let maybe_tag_number = (maybe_array_tag >> 4) & 0x0F;
        let maybe_is_context = (maybe_array_tag & 0x08) != 0;
        if maybe_is_context && maybe_tag_number == 2 {
            // Skip the array index tag + value
            let array_len = (maybe_array_tag & 0x07) as usize;
            pos += 1 + array_len;
        }
    }

    // Property value: opening tag context 3 (0x3E)
    if pos >= data.len() {
        return None;
    }
    let open_tag = data[pos];
    // Opening tag: context tag 3 with length = 0x06 (opening tag marker)
    if open_tag != 0x3E {
        return None;
    }
    pos += 1;

    // Parse the value
    let value = parse_application_value(&data[pos..])?;

    Some(ApduResponse::ReadPropertyAck {
        object_type: obj_type,
        instance,
        property_id,
        value,
    })
}

/// Parse a ReadPropertyMultipleAck response.
///
/// Format per ASHRAE 135-2020 Clause 15.8:
/// - Sequence of "Read Access Result":
///   - Context tag 0: Object Identifier (4 bytes)
///   - Context tag 1: List of Read Access Result (opening tag)
///     - For each property:
///       - Context tag 2: Property Identifier
///       - Optional: Context tag 3: Array Index
///       - Context tag 4: Property Value (opening tag)
///       - Application-tagged value
///       - Context tag 4: Property Value (closing tag)
///   - Context tag 1: closing tag
fn parse_read_property_multiple_ack(data: &[u8]) -> Option<ApduResponse> {
    let mut pos = 0;
    let mut results = Vec::new();

    while pos < data.len() {
        // Object identifier: context tag 0, length 4
        if pos + 5 > data.len() {
            break;
        }
        let tag = data[pos];
        let tag_number = (tag >> 4) & 0x0F;
        let is_context = (tag & 0x08) != 0;
        let length = tag & 0x07;
        if tag_number != 0 || !is_context || length != 4 {
            break;
        }
        pos += 1;
        let obj_raw = u32::from_be_bytes([
            data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
        ]);
        let obj_type_code = (obj_raw >> 22) as u16;
        let instance = obj_raw & 0x3FFFFF;
        let obj_type = match BacnetObjectType::from_code(obj_type_code) {
            Some(t) => t,
            None => { let _ = pos; pos += 4; break; } // skip unknown object type
        };
        pos += 4;

        // Opening tag context 1 (0x1E)
        if pos >= data.len() || data[pos] != 0x1E {
            break;
        }
        pos += 1;

        // Read property results until closing tag context 1 (0x1F)
        while pos < data.len() && data[pos] != 0x1F {
            // Property identifier: context tag 2
            if pos + 1 >= data.len() {
                break;
            }
            let prop_tag = data[pos];
            let prop_tag_num = (prop_tag >> 4) & 0x0F;
            let prop_is_ctx = (prop_tag & 0x08) != 0;
            let prop_len = prop_tag & 0x07;
            if prop_tag_num != 2 || !prop_is_ctx {
                pos += 1; // skip unknown tag
                continue;
            }
            pos += 1;
            let property_id = if (pos + prop_len as usize) <= data.len() {
                match prop_len {
                    1 => data[pos],
                    2 => {
                        let v = u16::from_be_bytes([data[pos], data[pos + 1]]);
                        if v > 255 { pos += prop_len as usize; continue; }
                        v as u8
                    }
                    _ => { pos += prop_len as usize; continue; }
                }
            } else {
                break;
            };
            pos += prop_len as usize;

            // Skip optional array index (context tag 3)
            if pos < data.len() {
                let maybe_tag = data[pos];
                let maybe_num = (maybe_tag >> 4) & 0x0F;
                let maybe_ctx = (maybe_tag & 0x08) != 0;
                if maybe_ctx && maybe_num == 3 {
                    let skip_len = (maybe_tag & 0x07) as usize;
                    pos += 1 + skip_len;
                }
            }

            // Property value: opening tag context 4 (0x4E)
            if pos >= data.len() || data[pos] != 0x4E {
                break;
            }
            pos += 1;

            let value = parse_application_value(&data[pos..]);
            // Advance past the value using correct byte count
            if let Some(consumed) = application_value_consumed(&data[pos..]) {
                pos += consumed;
            } else {
                // Can't determine size — skip 1 byte and hope for recovery
                pos += 1;
            }

            // Closing tag context 4 (0x4F)
            if pos < data.len() && data[pos] == 0x4F {
                pos += 1;
            }

            results.push((obj_type.clone(), instance, property_id, value.unwrap_or(BacnetValue::Null)));
        }

        // Closing tag context 1 (0x1F)
        if pos < data.len() && data[pos] == 0x1F {
            pos += 1;
        }
    }

    if results.is_empty() {
        None
    } else {
        Some(ApduResponse::ReadPropertyMultipleAck { values: results })
    }
}

/// Calculate total bytes consumed by an application-tagged value at the start of data.
/// Returns None if data is empty or the tag is context-specific.
fn application_value_consumed(data: &[u8]) -> Option<usize> {
    if data.is_empty() {
        return None;
    }
    let tag_byte = data[0];
    let is_context = (tag_byte & 0x08) != 0;
    if is_context {
        return None;
    }
    let raw_len = (tag_byte & 0x07) as usize;
    let (value_offset, length) = if raw_len == 5 {
        if data.len() < 2 { return None; }
        let ext_len = data[1] as usize;
        if ext_len == 254 {
            if data.len() < 6 { return None; }
            (6, u32::from_be_bytes([data[2], data[3], data[4], data[5]]) as usize)
        } else if ext_len == 255 {
            if data.len() < 4 { return None; }
            (4, u16::from_be_bytes([data[2], data[3]]) as usize)
        } else {
            (2, ext_len)
        }
    } else {
        (1, raw_len)
    };
    Some(value_offset + length)
}

fn parse_application_value(data: &[u8]) -> Option<BacnetValue> {
    if data.is_empty() {
        return None;
    }

    let tag_byte = data[0];

    // Check if this is a context-specific tag (bit 3 set) -- not an application tag
    let is_context = (tag_byte & 0x08) != 0;
    if is_context {
        return None;
    }

    // Tag format: TTTT C LLL (application tag = class bit 0)
    //   bits 7-4 = tag number (4 bits)
    //   bit 3 = class (0=application, 1=context-specific)
    //   bits 2-0 = length/value (short form: 0-4; value 5 = extended)

    let tag_number = (tag_byte >> 4) & 0x0F;
    let raw_len = (tag_byte & 0x07) as usize;
    let (value_offset, length) = if raw_len == 5 {
        // Extended form: next byte(s) contain the actual length
        if data.len() < 2 {
            return None;
        }
        let ext_len = data[1] as usize;
        if ext_len == 254 {
            // 4-byte extended length
            if data.len() < 6 {
                return None;
            }
            let len4 = u32::from_be_bytes([data[2], data[3], data[4], data[5]]) as usize;
            if len4 > data.len() - 6 { return None; }
            (6, len4)
        } else if ext_len == 255 {
            // 2-byte extended length
            if data.len() < 4 {
                return None;
            }
            let len2 = u16::from_be_bytes([data[2], data[3]]) as usize;
            if len2 > data.len() - 4 { return None; }
            (4, len2)
        } else {
            (2, ext_len)
        }
    } else {
        (1, raw_len)
    };

    match tag_number {
        TAG_NULL => Some(BacnetValue::Null),
        TAG_BOOLEAN => Some(BacnetValue::Boolean(raw_len != 0)),
        TAG_UNSIGNED_INT => {
            if data.len() < value_offset + length {
                return None;
            }
            let value_bytes = &data[value_offset..value_offset + length];
            let v = match length {
                1 => value_bytes[0] as u32,
                2 => u16::from_be_bytes([value_bytes[0], value_bytes[1]]) as u32,
                4 => u32::from_be_bytes([
                    value_bytes[0],
                    value_bytes[1],
                    value_bytes[2],
                    value_bytes[3],
                ]),
                _ => return None,
            };
            Some(BacnetValue::Unsigned(v))
        }
        TAG_SIGNED_INT => {
            if data.len() < value_offset + length {
                return None;
            }
            let value_bytes = &data[value_offset..value_offset + length];
            let v = match length {
                1 => value_bytes[0] as i8 as i32,
                2 => i16::from_be_bytes([value_bytes[0], value_bytes[1]]) as i32,
                4 => i32::from_be_bytes([
                    value_bytes[0],
                    value_bytes[1],
                    value_bytes[2],
                    value_bytes[3],
                ]),
                _ => return None,
            };
            Some(BacnetValue::Integer(v))
        }
        TAG_REAL => {
            // Real is always 4 bytes
            if data.len() < value_offset + 4 {
                return None;
            }
            let v = f32::from_be_bytes([
                data[value_offset],
                data[value_offset + 1],
                data[value_offset + 2],
                data[value_offset + 3],
            ]);
            Some(BacnetValue::Real(v as f64))
        }
        TAG_ENUMERATED => {
            if data.len() < value_offset + length {
                return None;
            }
            let value_bytes = &data[value_offset..value_offset + length];
            let v = match length {
                1 => value_bytes[0] as u32,
                2 => u16::from_be_bytes([value_bytes[0], value_bytes[1]]) as u32,
                4 => u32::from_be_bytes([
                    value_bytes[0],
                    value_bytes[1],
                    value_bytes[2],
                    value_bytes[3],
                ]),
                _ => return None,
            };
            Some(BacnetValue::Unsigned(v)) // Treat enum as unsigned
        }
        TAG_CHARACTER_STRING => {
            // First byte after tag header is encoding type, then string bytes
            if data.len() < value_offset + 1 {
                return None;
            }
            let _encoding = data[value_offset];
            let str_len = length - 1; // length includes encoding byte
            if data.len() < value_offset + 1 + str_len {
                return None;
            }
            let s = String::from_utf8_lossy(&data[value_offset + 1..value_offset + 1 + str_len])
                .to_string();
            Some(BacnetValue::String(s))
        }
        _ => None,
    }
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_object_id() {
        let obj_type = BacnetObjectType::AnalogInput;
        let instance = 42u32;
        let encoded = encode_object_id(obj_type, instance);
        // Context tag 0, length 4
        assert_eq!(encoded[0], 0x0C);
        let (decoded_type, decoded_instance) = decode_object_id(&encoded[1..]).unwrap();
        assert_eq!(decoded_type, obj_type);
        assert_eq!(decoded_instance, instance);
    }

    #[test]
    fn test_encode_object_id_all_types() {
        for (obj_type, code) in [
            (BacnetObjectType::AnalogInput, 0u16),
            (BacnetObjectType::AnalogOutput, 1),
            (BacnetObjectType::AnalogValue, 2),
            (BacnetObjectType::BinaryInput, 3),
            (BacnetObjectType::BinaryOutput, 4),
            (BacnetObjectType::BinaryValue, 5),
            (BacnetObjectType::Device, 8),
            (BacnetObjectType::MultiStateInput, 13),
            (BacnetObjectType::MultiStateOutput, 14),
            (BacnetObjectType::MultiStateValue, 19),
        ] {
            assert_eq!(obj_type.code(), code);
            let encoded = encode_object_id(obj_type, 1);
            assert_eq!(encoded[0], 0x0C, "context tag 0 expected for {:?}", obj_type);
            let (dt, di) = decode_object_id(&encoded[1..]).unwrap();
            assert_eq!(dt, obj_type);
            assert_eq!(di, 1);
        }
    }

    #[test]
    fn test_tag_encoding_application() {
        // Tag 2 (Unsigned), application, length 1: (2<<4) | 0 | 1 = 0x21
        let tag = encode_tag(TAG_UNSIGNED_INT, false, 1);
        assert_eq!(tag, vec![0x21]);

        // Tag 4 (Real), application, length 4: (4<<4) | 0 | 4 = 0x44
        let tag = encode_tag(TAG_REAL, false, 4);
        assert_eq!(tag, vec![0x44]);

        // Tag 1 (Boolean), application, length 1: (1<<4) | 0 | 1 = 0x11
        let tag = encode_tag(TAG_BOOLEAN, false, 1);
        assert_eq!(tag, vec![0x11]);
    }

    #[test]
    fn test_tag_encoding_context() {
        // Tag 0, context, length 4: (0<<4) | 0x08 | 4 = 0x0C
        let tag = encode_tag(0, true, 4);
        assert_eq!(tag, vec![0x0C]);

        // Tag 1, context, length 1: (1<<4) | 0x08 | 1 = 0x19
        let tag = encode_tag(1, true, 1);
        assert_eq!(tag, vec![0x19]);
    }

    #[test]
    fn test_tag_encoding_extended() {
        // Tag 7, application, length 10: (7<<4) | 0 | 5 = 0x75, then 0x0A
        let tag = encode_tag(7, false, 10);
        assert_eq!(tag, vec![0x75, 10]);
    }

    #[test]
    fn test_build_who_is() {
        let msg = build_who_is(0, 4194303);
        // BVLC header
        assert_eq!(msg[0], 0x81); // BVLC type
        assert_eq!(msg[1], 0x0B); // Original broadcast
        // NPDU
        assert_eq!(msg[4], 1); // Protocol version
        // APDU: unconfirmed request
        assert_eq!(msg[6], 0x10); // PDU type = unconfirmed request (no extra bits)
        assert_eq!(msg[7], 0x08); // Service choice = Who-Is
        assert!(msg.len() > 10);
    }

    #[test]
    fn test_build_read_property() {
        let msg = build_read_property(100, BacnetObjectType::AnalogInput, 1, PROPERTY_PRESENT_VALUE);
        assert_eq!(msg[0], 0x81); // BVLC type
        assert_eq!(msg[1], 0x0A); // Original unicast
        // APDU starts at offset 6
        let apdu_start = 6;
        assert_eq!(msg[apdu_start], 0x08); // PDU type=confirmed, SAQ=1 (bit 3)
        assert_eq!(msg[apdu_start + 1], 0x05); // max segments/APDU
        // msg[apdu_start + 2] = invoke ID (dynamic)
        assert_eq!(msg[apdu_start + 3], SERVICE_CONFIRMED_READ_PROPERTY); // service choice
        // Object ID at context tag 0
        assert_eq!(msg[apdu_start + 4], 0x0C); // context tag 0, length 4
    }

    #[test]
    fn test_build_write_property() {
        let msg = build_write_property(
            100,
            BacnetObjectType::AnalogOutput,
            1,
            PROPERTY_PRESENT_VALUE,
            &BacnetValue::Real(23.5),
            Some(8),
        );
        assert_eq!(msg[0], 0x81);
        let apdu_start = 6;
        assert_eq!(msg[apdu_start], 0x08); // Confirmed request, SAQ=1 (bit 3)
        assert_eq!(msg[apdu_start + 3], SERVICE_CONFIRMED_WRITE_PROPERTY);
        assert!(msg.len() > 15);
    }

    #[test]
    fn test_build_subscribe_cov() {
        let msg = build_subscribe_cov(1, 100, BacnetObjectType::AnalogInput, 1, 3600, true);
        assert_eq!(msg[0], 0x81);
        let apdu_start = 6;
        assert_eq!(msg[apdu_start], 0x08); // Confirmed request, SAQ=1 (bit 3)
        assert_eq!(msg[apdu_start + 3], SERVICE_CONFIRMED_SUBSCRIBE_COV); // service code = 5
        assert!(msg.len() > 10);
    }

    #[test]
    fn test_encode_application_value_real() {
        let encoded = encode_application_value(&BacnetValue::Real(25.0));
        // Tag 4, application, length 4: 0x44
        assert_eq!(encoded[0], 0x44);
        assert_eq!(encoded.len(), 5);
        let decoded = parse_application_value(&encoded).unwrap();
        if let BacnetValue::Real(v) = decoded {
            assert!((v - 25.0).abs() < 0.01);
        } else {
            panic!("Expected Real value");
        }
    }

    #[test]
    fn test_encode_application_value_unsigned() {
        let encoded = encode_application_value(&BacnetValue::Unsigned(42));
        // Tag 2, application, length 1: 0x21
        assert_eq!(encoded[0], 0x21);
        let decoded = parse_application_value(&encoded).unwrap();
        assert_eq!(decoded, BacnetValue::Unsigned(42));
    }

    #[test]
    fn test_encode_application_value_unsigned_large() {
        let encoded = encode_application_value(&BacnetValue::Unsigned(1000));
        // Tag 2, application, length 2: 0x22
        assert_eq!(encoded[0], 0x22);
        let decoded = parse_application_value(&encoded).unwrap();
        assert_eq!(decoded, BacnetValue::Unsigned(1000));
    }

    #[test]
    fn test_encode_application_value_boolean() {
        let encoded_true = encode_application_value(&BacnetValue::Boolean(true));
        assert_eq!(encoded_true, vec![0x11]); // Tag 1, app, value=1 (true)
        let decoded_true = parse_application_value(&encoded_true).unwrap();
        assert_eq!(decoded_true, BacnetValue::Boolean(true));

        let encoded_false = encode_application_value(&BacnetValue::Boolean(false));
        assert_eq!(encoded_false, vec![0x10]); // Tag 1, app, value=0 (false)
        let decoded_false = parse_application_value(&encoded_false).unwrap();
        assert_eq!(decoded_false, BacnetValue::Boolean(false));
    }

    #[test]
    fn test_encode_application_value_string() {
        let encoded = encode_application_value(&BacnetValue::String("hello".to_string()));
        // Tag 7, application, length 6 > 4 so extended form: (7<<4) | 0 | 5 = 0x75, then 0x06
        assert_eq!(encoded[0], 0x75); // extended form tag
        assert_eq!(encoded[1], 6);    // actual length
        assert_eq!(encoded[2], 0x00); // ANSI encoding
        assert_eq!(&encoded[3..], b"hello");
        let decoded = parse_application_value(&encoded).unwrap();
        assert_eq!(decoded, BacnetValue::String("hello".to_string()));
    }

    #[test]
    fn test_encode_application_value_null() {
        let encoded = encode_application_value(&BacnetValue::Null);
        assert_eq!(encoded, vec![0x00]); // Tag 0, app, length 0
        let decoded = parse_application_value(&encoded).unwrap();
        assert_eq!(decoded, BacnetValue::Null);
    }

    #[test]
    fn test_encode_application_value_integer() {
        let encoded = encode_application_value(&BacnetValue::Integer(-10));
        // Tag 3, application, length 1: 0x31
        assert_eq!(encoded[0], 0x31);
        let decoded = parse_application_value(&encoded).unwrap();
        assert_eq!(decoded, BacnetValue::Integer(-10));
    }

    #[test]
    fn test_parse_i_am() {
        // Construct a valid I-Am response
        let mut data = vec![];
        // Application tag 12 (ObjectIdentifier), length 4: (12<<4) | 0 | 4 = 0xC4
        data.push(0xC4);
        // Device object: type=8 (Device) << 22 | instance=100
        let device_id: u32 = (8u32 << 22) | 100;
        data.extend_from_slice(&device_id.to_be_bytes());
        // Max APDU: application tag 2 (Unsigned), length 1: 0x21, value=480
        // Actually use a small value for simplicity: 0x21, 0x80
        data.push(0x21); // tag unsigned, len 1
        data.push(0x80); // max APDU = 128 (encoded as 480 in spec, but 128 is valid)
        // Segmentation: application tag 2 (Unsigned), length 1
        data.push(0x21);
        data.push(0x00); // no segmentation
        // Vendor ID: application tag 2 (Unsigned), length 1
        data.push(0x21);
        data.push(0x0A); // vendor 10

        let result = parse_i_am(&data).unwrap();
        if let ApduResponse::IAm { device_id, max_apdu, vendor_id, .. } = result {
            assert_eq!(device_id, 100);
            assert_eq!(max_apdu, 128);
            assert_eq!(vendor_id, 10);
        } else {
            panic!("Expected IAm response");
        }
    }

    #[test]
    fn test_roundtrip_real() {
        for val in [0.0, 1.0, -1.0, 100.5, 3.14159] {
            let encoded = encode_application_value(&BacnetValue::Real(val));
            let decoded = parse_application_value(&encoded).unwrap();
            if let BacnetValue::Real(v) = decoded {
                assert!((v - val).abs() < 0.01, "Roundtrip failed for {}", val);
            } else {
                panic!("Expected Real for {}", val);
            }
        }
    }

    #[test]
    fn test_roundtrip_unsigned() {
        for val in [0u32, 1, 42, 255, 256, 1000, 65535] {
            let encoded = encode_application_value(&BacnetValue::Unsigned(val));
            let decoded = parse_application_value(&encoded).unwrap();
            assert_eq!(decoded, BacnetValue::Unsigned(val), "Roundtrip failed for {}", val);
        }
    }
}
