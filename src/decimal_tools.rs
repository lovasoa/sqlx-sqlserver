use sqlx_core::error::BoxDynError;

pub(crate) fn decode_money_bytes(bytes: &[u8]) -> Result<i64, BoxDynError> {
    let amount = match bytes {
        [a, b, c, d] => i64::from(i32::from_le_bytes([*a, *b, *c, *d])),
        [a, b, c, d, e, f, g, h] => {
            let amount_h = i64::from(i32::from_le_bytes([*a, *b, *c, *d]));
            let amount_l = i64::from(u32::from_le_bytes([*e, *f, *g, *h]));
            (amount_h << 32) | amount_l
        }
        _ => {
            return Err(format!("expected 8/4 bytes for Money, got {}", bytes.len()).into());
        }
    };

    Ok(amount)
}

pub(crate) fn decode_numeric_bytes(bytes: &[u8]) -> Result<(i8, u128), BoxDynError> {
    let Some((&sign, rest)) = bytes.split_first() else {
        return Err("numeric bytes cannot be empty".into());
    };

    let sign = match sign {
        0 => -1,
        1 => 1,
        other => return Err(format!("invalid sign byte: 0x{other:02x}").into()),
    };

    if rest.len() > 16 {
        return Err("numeric value exceeds 16 bytes".into());
    }

    let mut fixed_bytes = [0_u8; 16];
    fixed_bytes[..rest.len()].copy_from_slice(rest);

    Ok((sign, u128::from_le_bytes(fixed_bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_money_bytes_rejects_bad_lengths() {
        assert_eq!(
            "expected 8/4 bytes for Money, got 0",
            decode_money_bytes(&[]).unwrap_err().to_string()
        );
        assert_eq!(
            "expected 8/4 bytes for Money, got 3",
            decode_money_bytes(&[0x01, 0x02, 0x03])
                .unwrap_err()
                .to_string()
        );
    }

    #[test]
    fn decode_money_bytes_handles_boundaries() {
        assert_eq!(
            1_234_561_234,
            decode_money_bytes(&[0xd2, 0xe8, 0x95, 0x49]).unwrap()
        );
        assert_eq!(
            -1_234_561_234,
            decode_money_bytes(&[0x2e, 0x17, 0x6a, 0xb6]).unwrap()
        );
        assert_eq!(
            1_234_567_891_234,
            decode_money_bytes(&[0x1f, 0x01, 0x00, 0x00, 0x22, 0x09, 0xfb, 0x71]).unwrap()
        );
        assert_eq!(
            -1_234_567_891_234,
            decode_money_bytes(&[0xe0, 0xfe, 0xff, 0xff, 0xde, 0xf6, 0x04, 0x8e]).unwrap()
        );
        assert_eq!(
            i64::MAX,
            decode_money_bytes(&[0xff, 0xff, 0xff, 0x7f, 0xff, 0xff, 0xff, 0xff]).unwrap()
        );
        assert_eq!(
            i64::MIN,
            decode_money_bytes(&[0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00]).unwrap()
        );
        assert_eq!(
            0,
            decode_money_bytes(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]).unwrap()
        );
    }

    #[test]
    fn decode_numeric_bytes_rejects_invalid_payloads() {
        assert_eq!(
            "numeric bytes cannot be empty",
            decode_numeric_bytes(&[]).unwrap_err().to_string()
        );
        assert_eq!(
            "invalid sign byte: 0x02",
            decode_numeric_bytes(&[0x02, 0x01, 0x02])
                .unwrap_err()
                .to_string()
        );
        assert_eq!(
            "numeric value exceeds 16 bytes",
            decode_numeric_bytes(&[0x01; 18]).unwrap_err().to_string()
        );
    }

    #[test]
    fn decode_numeric_bytes_decodes_sign_and_little_endian_amount() {
        assert_eq!(
            (1, 412_345),
            decode_numeric_bytes(&[0x01, 0xb9, 0x4a, 0x06, 0x00]).unwrap()
        );
        assert_eq!(
            (-1, 123_456_789_123_400),
            decode_numeric_bytes(&[0x00, 0x48, 0x91, 0x0f, 0x86, 0x48, 0x70, 0x00, 0x00]).unwrap()
        );
        assert_eq!(
            (1, 0),
            decode_numeric_bytes(&[0x01, 0x00, 0x00, 0x00, 0x00]).unwrap()
        );
        assert_eq!(
            (-1, 0),
            decode_numeric_bytes(&[0x00, 0x00, 0x00, 0x00, 0x00]).unwrap()
        );
        assert_eq!(
            (1, u128::MAX),
            decode_numeric_bytes(&[
                0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xff, 0xff, 0xff,
            ])
            .unwrap()
        );
        assert_eq!(
            (1, 1 << 120),
            decode_numeric_bytes(&[
                0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x01,
            ])
            .unwrap()
        );
    }
}
