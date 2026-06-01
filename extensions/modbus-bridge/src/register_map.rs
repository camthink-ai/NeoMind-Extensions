use crate::types::{RegisterConfig, RegisterDataType, RegisterValue, WordOrder};

#[cfg(test)]
use crate::types::RegisterType;

pub fn decode_register(config: &RegisterConfig, words: &[u16]) -> RegisterValue {
    let raw_value = match config.data_type {
        RegisterDataType::Uint16 => words.first().copied().unwrap_or(0) as f64,
        RegisterDataType::Int16 => {
            let v = words.first().copied().unwrap_or(0) as i16;
            v as f64
        }
        RegisterDataType::Uint32 => {
            let combined = combine_words(words, &config.word_order);
            combined as f64
        }
        RegisterDataType::Int32 => {
            let combined = combine_words(words, &config.word_order);
            combined as i32 as f64
        }
        RegisterDataType::Float32 => {
            let combined = combine_words(words, &config.word_order);
            f32::from_bits(combined) as f64
        }
        RegisterDataType::Bool => (words.first().copied().unwrap_or(0) != 0) as u8 as f64,
    };

    let value = if config.scale != 0.0 {
        raw_value * config.scale
    } else {
        raw_value
    };

    RegisterValue {
        name: config.name.clone(),
        value,
        unit: config.unit.clone(),
        raw: words.to_vec(),
    }
}

/// Combine two 16-bit words into a 32-bit value according to the configured word order.
/// Big (default): first word = hi, second word = lo (standard Modbus).
/// Little: first word = lo, second word = hi (some PLCs).
fn combine_words(words: &[u16], order: &WordOrder) -> u32 {
    let first = words.first().copied().unwrap_or(0) as u32;
    let second = words.get(1).copied().unwrap_or(0) as u32;
    match order {
        WordOrder::Big => (first << 16) | second,
        WordOrder::Little => (second << 16) | first,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(data_type: RegisterDataType, scale: f64) -> RegisterConfig {
        RegisterConfig {
            address: 0,
            count: data_type.register_count(),
            name: "test".into(),
            data_type,
            scale,
            unit: String::new(),
            word_order: WordOrder::Big,
            register_type: RegisterType::Holding,
        }
    }

    #[test]
    fn test_decode_uint16() {
        let config = make_config(RegisterDataType::Uint16, 0.1);
        let val = decode_register(&config, &[650]);
        assert!((val.value - 65.0).abs() < 0.001);
    }

    #[test]
    fn test_decode_int16_negative() {
        let config = make_config(RegisterDataType::Int16, 0.1);
        let val = decode_register(&config, &[0xFF9C]); // -100 as u16
        assert!((val.value - (-10.0)).abs() < 0.001);
    }

    #[test]
    fn test_decode_float32() {
        let config = make_config(RegisterDataType::Float32, 0.0);
        let bits = 23.5f32.to_bits();
        let hi = (bits >> 16) as u16;
        let lo = bits as u16;
        let val = decode_register(&config, &[hi, lo]);
        assert!((val.value - 23.5).abs() < 0.01);
    }

    #[test]
    fn test_decode_bool() {
        let config = make_config(RegisterDataType::Bool, 0.0);
        let val = decode_register(&config, &[1]);
        assert!((val.value - 1.0).abs() < 0.001);
        let val2 = decode_register(&config, &[0]);
        assert!((val2.value - 0.0).abs() < 0.001);
    }
}
