//! Argument parsing with `zend_parse_parameters` semantics.
//!
//! ext-grpc parses every argument with `zend_parse_parameters` and throws
//! `InvalidArgumentException` when that fails. To throw the same exception we
//! take arguments as raw zvals, which bypasses the engine's parser — so its
//! rules are reproduced here: exact types when the calling file declares
//! `strict_types=1`, scalar coercion otherwise. Each function is named after
//! the format specifier it mirrors and returns `None` where the engine's
//! parser would fail. The deprecation notices the engine emits on lossy weak
//! coercions (null to scalar, fractional float to int) are not reproduced.

use ext_php_rs::ffi::ZEND_ACC_STRICT_TYPES;
use ext_php_rs::types::Zval;
use ext_php_rs::zend::ExecutorGlobals;

/// `ZEND_ARG_USES_STRICT_TYPES()`: whether the frame calling the current
/// internal function was compiled with `declare(strict_types=1)`.
fn caller_uses_strict_types() -> bool {
    let globals = ExecutorGlobals::get();
    // SAFETY: `current_execute_data` is null or points at the live frame of
    // the internal function PHP is executing on this thread right now.
    let current = unsafe { globals.current_execute_data.as_ref() };
    let Some(func) = current
        .and_then(|frame| frame.previous())
        .and_then(|caller| caller.function())
    else {
        return false;
    };
    // SAFETY: `common` is the shared prefix of every `zend_function` variant.
    let flags = unsafe { func.common.fn_flags };
    flags & ZEND_ACC_STRICT_TYPES != 0
}

/// Format `s`.
pub(crate) fn string(zv: &Zval) -> Option<String> {
    let zv = zv.dereference();
    if zv.is_string() {
        return zv.string();
    }
    if caller_uses_strict_types() || zv.is_array() {
        return None;
    }
    zv.coerce_to_string()
}

/// Format `s!`: `Some(None)` for null.
pub(crate) fn nullable_string(zv: &Zval) -> Option<Option<String>> {
    if zv.dereference().is_null() {
        return Some(None);
    }
    string(zv).map(Some)
}

/// Format `l`.
pub(crate) fn long(zv: &Zval) -> Option<i64> {
    let zv = zv.dereference();
    if let Some(l) = zv.long() {
        return Some(l);
    }
    if caller_uses_strict_types() {
        return None;
    }
    if let Some(d) = zv.double() {
        return double_to_long(d);
    }
    if let Some(s) = zv.str() {
        return match numeric_prefix(s)? {
            Numeric::Long(l) => Some(l),
            Numeric::Double(d) => double_to_long(d),
        };
    }
    if let Some(b) = zv.bool() {
        return Some(i64::from(b));
    }
    zv.is_null().then_some(0)
}

/// Format `d`.
pub(crate) fn double(zv: &Zval) -> Option<f64> {
    let zv = zv.dereference();
    if let Some(d) = zv.double() {
        return Some(d);
    }
    if let Some(l) = zv.long() {
        // Strict mode still widens int to float.
        return Some(l as f64);
    }
    if caller_uses_strict_types() {
        return None;
    }
    if let Some(s) = zv.str() {
        return match numeric_prefix(s)? {
            Numeric::Long(l) => Some(l as f64),
            Numeric::Double(d) => Some(d),
        };
    }
    if let Some(b) = zv.bool() {
        return Some(if b { 1.0 } else { 0.0 });
    }
    zv.is_null().then_some(0.0)
}

/// Format `b`.
pub(crate) fn boolean(zv: &Zval) -> Option<bool> {
    let zv = zv.dereference();
    if let Some(b) = zv.bool() {
        return Some(b);
    }
    if caller_uses_strict_types() {
        return None;
    }
    (zv.is_scalar() || zv.is_null()).then(|| zv.coerce_to_bool())
}

/// Weak float-to-int: NaN, infinities and out-of-range values are rejected,
/// everything else truncates toward zero.
fn double_to_long(d: f64) -> Option<i64> {
    // 2^63 is exactly representable; i64::MAX as f64 rounds up to it.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    (-LIMIT..LIMIT).contains(&d).then_some(d as i64)
}

#[derive(Debug, PartialEq)]
enum Numeric {
    Long(i64),
    Double(f64),
}

/// The engine's numeric-string rule for weak scalar arguments: optional
/// leading whitespace, then an integer or float literal. Trailing data is
/// accepted (the engine warns about it); a string with no leading number is
/// not numeric.
fn numeric_prefix(s: &str) -> Option<Numeric> {
    let bytes = s.as_bytes();
    let digits_from = |mut i: usize| {
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        i
    };

    let mut start = 0;
    while bytes
        .get(start)
        .is_some_and(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
    {
        start += 1;
    }

    let mut end = start;
    if matches!(bytes.get(end), Some(b'+' | b'-')) {
        end += 1;
    }
    let int_start = end;
    end = digits_from(end);
    let mut digits = end - int_start;
    let mut is_float = false;

    if bytes.get(end) == Some(&b'.') {
        let frac_end = digits_from(end + 1);
        digits += frac_end - (end + 1);
        if digits > 0 {
            is_float = true;
            end = frac_end;
        }
    }
    if digits == 0 {
        return None;
    }

    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut exp = end + 1;
        if matches!(bytes.get(exp), Some(b'+' | b'-')) {
            exp += 1;
        }
        let exp_end = digits_from(exp);
        if exp_end > exp {
            is_float = true;
            end = exp_end;
        }
    }

    let literal = s.get(start..end)?;
    if !is_float && let Ok(l) = literal.parse::<i64>() {
        return Some(Numeric::Long(l));
    }
    literal.parse::<f64>().ok().map(Numeric::Double)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers() {
        assert_eq!(numeric_prefix("1"), Some(Numeric::Long(1)));
        assert_eq!(numeric_prefix("-42"), Some(Numeric::Long(-42)));
        assert_eq!(numeric_prefix("+7"), Some(Numeric::Long(7)));
        assert_eq!(numeric_prefix(" \t\n12"), Some(Numeric::Long(12)));
    }

    #[test]
    fn floats() {
        assert_eq!(numeric_prefix("1.5"), Some(Numeric::Double(1.5)));
        assert_eq!(numeric_prefix(".5"), Some(Numeric::Double(0.5)));
        assert_eq!(numeric_prefix("1."), Some(Numeric::Double(1.0)));
        assert_eq!(numeric_prefix("1e3"), Some(Numeric::Double(1000.0)));
        assert_eq!(numeric_prefix("2E-1"), Some(Numeric::Double(0.2)));
    }

    #[test]
    fn integer_overflow_becomes_float() {
        assert_eq!(
            numeric_prefix("9223372036854775808"),
            Some(Numeric::Double(9_223_372_036_854_775_808.0))
        );
    }

    #[test]
    fn trailing_data_is_tolerated() {
        assert_eq!(numeric_prefix("12abc"), Some(Numeric::Long(12)));
        assert_eq!(numeric_prefix("12 "), Some(Numeric::Long(12)));
        // An exponent marker without digits is trailing data, not an exponent.
        assert_eq!(numeric_prefix("1e"), Some(Numeric::Long(1)));
    }

    #[test]
    fn non_numeric_is_rejected() {
        for s in ["", "x", "abc1", ".", "-", "+.", " ", "x1A"] {
            assert_eq!(numeric_prefix(s), None, "{s:?}");
        }
    }

    #[test]
    fn double_to_long_bounds() {
        assert_eq!(double_to_long(1.9), Some(1));
        assert_eq!(double_to_long(-1.9), Some(-1));
        assert_eq!(double_to_long(f64::NAN), None);
        assert_eq!(double_to_long(f64::INFINITY), None);
        assert_eq!(double_to_long(9_223_372_036_854_775_808.0), None);
        assert_eq!(double_to_long(-9_223_372_036_854_775_808.0), Some(i64::MIN));
    }
}
