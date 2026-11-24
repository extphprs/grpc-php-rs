use ext_php_rs::prelude::*;
use ext_php_rs::types::Zval;

use std::time::{SystemTime, UNIX_EPOCH};

#[php_class]
#[php(name = "Grpc\\Timeval")]
#[derive(Debug, Clone)]
pub struct GrpcTimeval {
    usec: i64,
    /// gpr clock semantics: the bare constructor creates a TIMESPAN (relative
    /// duration), while now()/infFuture()/infPast() are absolute times.
    /// C-core resolves a timespan deadline as now + span at call creation —
    /// `new Timeval(5_000_000)` as a deadline means "5s from now", not 1970.
    is_span: bool,
}

#[php_impl]
impl GrpcTimeval {
    /// ext-grpc accepts ints and (weakly coerced) floats/numeric values here;
    /// callers commonly compute deadlines with float arithmetic.
    pub fn __construct(usec: &Zval) -> PhpResult<Self> {
        // ext-grpc parses a long first and falls back to a double.
        let usec = crate::args::long(usec)
            .or_else(|| crate::args::double(usec).map(|d| d as i64))
            .ok_or_else(|| crate::error::invalid_argument("Timeval expects a long or double"))?;
        Ok(Self { usec, is_span: true })
    }

    /// Returns the internal microsecond value (used by other modules).
    pub fn get_usec(&self) -> i64 {
        self.usec
    }

    /// Resolve to absolute epoch microseconds for use as a deadline, the way
    /// C-core converts clocks at call creation: a TIMESPAN becomes now + span.
    pub fn to_absolute_usec(&self) -> i64 {
        if !self.is_span || self.usec == i64::MAX || self.usec == i64::MIN {
            return self.usec;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_micros()).ok())
            .unwrap_or(0);
        now.saturating_add(self.usec)
    }

    pub fn now() -> PhpResult<Self> {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| PhpException::from_message(e.to_string()))?;
        let usec = i64::try_from(duration.as_micros())
            .map_err(|e| PhpException::from_message(e.to_string()))?;
        Ok(Self {
            usec,
            is_span: false,
        })
    }

    #[php(name = "infFuture")]
    pub fn inf_future() -> Self {
        Self {
            usec: i64::MAX,
            is_span: false,
        }
    }

    #[php(name = "infPast")]
    pub fn inf_past() -> Self {
        Self {
            usec: i64::MIN,
            is_span: false,
        }
    }

    pub fn zero() -> Self {
        Self {
            usec: 0,
            is_span: true,
        }
    }

    pub fn similar(a: &Zval, b: &Zval, threshold: &Zval) -> PhpResult<bool> {
        let a = a
            .extract::<&GrpcTimeval>()
            .ok_or_else(|| crate::error::invalid_argument("compare expects three Timevals"))?;
        let b = b
            .extract::<&GrpcTimeval>()
            .ok_or_else(|| crate::error::invalid_argument("compare expects three Timevals"))?;
        let threshold = threshold
            .extract::<&GrpcTimeval>()
            .ok_or_else(|| crate::error::invalid_argument("compare expects three Timevals"))?;
        let diff = (a.usec.saturating_sub(b.usec)).saturating_abs();
        Ok(diff <= threshold.usec.saturating_abs())
    }

    pub fn compare(a: &Zval, b: &Zval) -> PhpResult<i64> {
        let a = a
            .extract::<&GrpcTimeval>()
            .ok_or_else(|| crate::error::invalid_argument("compare expects two Timevals"))?;
        let b = b
            .extract::<&GrpcTimeval>()
            .ok_or_else(|| crate::error::invalid_argument("compare expects two Timevals"))?;
        Ok(match a.usec.cmp(&b.usec) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        })
    }

    pub fn add(&self, other: &Zval) -> PhpResult<Self> {
        let other = other
            .extract::<&GrpcTimeval>()
            .ok_or_else(|| crate::error::invalid_argument("add expects a Timeval"))?;
        // gpr time math saturates at infinities: inf ± finite == inf.
        if self.usec == i64::MAX || self.usec == i64::MIN {
            return Ok(Self {
                usec: self.usec,
                is_span: self.is_span,
            });
        }
        Ok(Self {
            usec: self.usec.saturating_add(other.usec),
            is_span: self.is_span && other.is_span,
        })
    }

    pub fn subtract(&self, other: &Zval) -> PhpResult<Self> {
        let other = other
            .extract::<&GrpcTimeval>()
            .ok_or_else(|| crate::error::invalid_argument("subtract expects a Timeval"))?;
        if self.usec == i64::MAX || self.usec == i64::MIN {
            return Ok(Self {
                usec: self.usec,
                is_span: self.is_span,
            });
        }
        Ok(Self {
            usec: self.usec.saturating_sub(other.usec),
            is_span: self.is_span && other.is_span,
        })
    }

    #[php(name = "sleepUntil")]
    pub fn sleep_until(&self) -> PhpResult<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| PhpException::from_message(e.to_string()))?;
        let now_usec = i64::try_from(now.as_micros())
            .map_err(|e| PhpException::from_message(e.to_string()))?;
        let remaining = self.usec.saturating_sub(now_usec);
        if remaining > 0 && self.usec != i64::MAX {
            let rt = crate::runtime::get_runtime().map_err(PhpException::from)?;
            rt.block_on(async {
                tokio::time::sleep(std::time::Duration::from_micros(remaining as u64)).await;
            });
        }
        Ok(())
    }
}
