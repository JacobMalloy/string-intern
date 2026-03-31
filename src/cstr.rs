use core::ffi::CStr;
use core::fmt;
use core::hash::{Hash, Hasher};
use std::sync::atomic::Ordering;

use crate::Intern;

/// Error returned by [`InternC::try_new`] when the string contains an interior null byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteriorNulError(usize);

impl InteriorNulError {
    /// Returns the byte position of the first interior null byte.
    pub fn nul_position(&self) -> usize {
        self.0
    }
}

impl fmt::Display for InteriorNulError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "interior null byte at position {}", self.0)
    }
}

impl std::error::Error for InteriorNulError {}

/// An interned string guaranteed to contain no interior null bytes.
///
/// Identical to [`Intern`] but enforces the no-interior-null invariant at
/// construction time, enabling [`AsRef<CStr>`] and FFI use.
///
/// The two types share the same intern pool, so interning the same string
/// as both [`Intern`] and [`InternC`] produces a single allocation.
#[derive(Clone, Copy)]
pub struct InternC(Intern);

unsafe impl Send for InternC {}
unsafe impl Sync for InternC {}

impl InternC {
    /// Interns a string. Panics if the string contains an interior null byte.
    pub fn new(s: impl AsRef<str>) -> Self {
        Self::try_new(s).expect("interned string contains interior null byte")
    }

    /// Interns a string, returning an error if it contains an interior null byte.
    /// Note: the string is interned regardless; invalid strings are accessible as [`Intern`]
    /// but not as [`InternC`].
    pub fn try_new(s: impl AsRef<str>) -> Result<Self, InteriorNulError> {
        InternC::try_from(Intern::intern(s.as_ref()))
    }

    pub fn from_static(s: &'static str) -> Self {
        Self::new(s)
    }

    pub fn as_str(&self) -> &'static str {
        self.0.as_str()
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.0.as_ptr()
    }
}

impl AsRef<CStr> for InternC {
    fn as_ref(&self) -> &'static CStr {
        // SAFETY: alloc_length_prefixed always writes a null terminator after the string data.
        // InternC::new panics (and try_new errors) on interior null bytes, so the first null
        // byte is always the terminator. 'static is valid because interned allocations are
        // never freed.
        unsafe { CStr::from_ptr(self.0.as_ptr() as *const std::ffi::c_char) }
    }
}

impl PartialEq for InternC {
    fn eq(&self, other: &Self) -> bool { self.0 == other.0 }
}

impl Eq for InternC {}

impl Hash for InternC {
    fn hash<H: Hasher>(&self, state: &mut H) { self.0.hash(state) }
}

impl PartialOrd for InternC {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}

impl Ord for InternC {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering { self.0.cmp(&other.0) }
}

impl fmt::Display for InternC {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(f) }
}

impl fmt::Debug for InternC {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InternC({:?})", self.as_str())
    }
}

impl AsRef<str> for InternC {
    fn as_ref(&self) -> &str { self.0.as_str() }
}

impl AsRef<std::path::Path> for InternC {
    fn as_ref(&self) -> &std::path::Path { self.0.as_ref() }
}

impl std::ops::Deref for InternC {
    type Target = str;
    fn deref(&self) -> &str { self.0.as_str() }
}

impl From<&str> for InternC {
    fn from(s: &str) -> Self { InternC::new(s) }
}

impl From<String> for InternC {
    fn from(s: String) -> Self { InternC::new(s) }
}

impl From<Box<str>> for InternC {
    fn from(s: Box<str>) -> Self { InternC::new(&*s) }
}

impl From<std::borrow::Cow<'_, str>> for InternC {
    fn from(s: std::borrow::Cow<'_, str>) -> Self { InternC::new(s) }
}

impl Default for InternC {
    fn default() -> Self { InternC::from_static("default_intern") }
}

impl From<InternC> for Intern {
    fn from(ic: InternC) -> Self { ic.0 }
}

impl TryFrom<Intern> for InternC {
    type Error = InteriorNulError;
    fn try_from(i: Intern) -> Result<Self, Self::Error> {
        match i.terminator().load(Ordering::Relaxed) {
            0x00 => return Ok(InternC(i)),
            0xFF => {} // unchecked or position >= 254 — fall through to scan
            b => return Err(InteriorNulError((b - 1) as usize)),
        }
        if let Some(pos) = i.as_str().bytes().position(|b| b == 0) {
            // Cache if position fits in the marker (0–253 → stored as 1–254)
            if pos < 254 {
                i.terminator().store((pos + 1) as u8, Ordering::Relaxed);
            }
            return Err(InteriorNulError(pos));
        }
        i.terminator().store(0x00, Ordering::Relaxed);
        Ok(InternC(i))
    }
}

#[cfg(feature = "serde")]
use serde::de::{Deserialize, Deserializer, Visitor};

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for InternC {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct InternCVisitor;

        impl Visitor<'_> for InternCVisitor {
            type Value = InternC;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string without interior null bytes")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                InternC::try_new(v).map_err(E::custom)
            }

            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
                InternC::try_new(v).map_err(E::custom)
            }
        }

        deserializer.deserialize_str(InternCVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Intern;

    #[test]
    fn asref_cstr() {
        let i = InternC::new("cstr");
        let c: &CStr = i.as_ref();
        assert_eq!(c.to_str().unwrap(), "cstr");
    }

    #[test]
    fn cstr_no_interior_null_in_bytes() {
        let i = InternC::new("abc");
        let c: &CStr = i.as_ref();
        assert_eq!(c.to_bytes(), b"abc");
    }

    #[test]
    fn new_interior_null_panics() {
        let result = std::panic::catch_unwind(|| InternC::new("foo\0bar"));
        assert!(result.is_err());
    }

    #[test]
    fn try_new_interior_null_error() {
        let err = InternC::try_new("foo\0bar").unwrap_err();
        assert_eq!(err.nul_position(), 3);
    }

    #[test]
    fn try_new_ok() {
        let i = InternC::try_new("hello").unwrap();
        let c: &CStr = i.as_ref();
        assert_eq!(c.to_str().unwrap(), "hello");
    }

    #[test]
    fn send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<InternC>();
    }

    #[test]
    fn shares_pool_with_intern() {
        let i = Intern::new("shared");
        let ic = InternC::new("shared");
        assert!(std::ptr::eq(i.as_ptr(), ic.as_ptr()), "same string must share one allocation");
    }

    #[test]
    fn intern_c_into_intern() {
        let ic = InternC::new("hello");
        let i: Intern = ic.into();
        assert_eq!(i.as_str(), "hello");
        assert!(std::ptr::eq(i.as_ptr(), ic.as_ptr()));
    }

    #[test]
    fn intern_try_into_intern_c_ok() {
        let i = Intern::new("hello");
        let ic: InternC = i.try_into().unwrap();
        assert_eq!(ic.as_str(), "hello");
        assert!(std::ptr::eq(i.as_ptr(), ic.as_ptr()));
    }

    #[test]
    fn intern_try_into_intern_c_err() {
        let i = Intern::new("foo\0bar");
        let result: Result<InternC, _> = i.try_into();
        assert!(result.is_err());
    }

    #[test]
    fn terminator_cached_after_validation() {
        use std::sync::atomic::Ordering;
        // Valid: marker should be 0x00 after InternC::new
        let ic = InternC::new("cached_valid");
        assert_eq!(ic.0.terminator().load(Ordering::Relaxed), 0x00);
        // Second try_from should hit the O(1) fast path
        let i = Intern::new("cached_valid");
        let ic2: InternC = i.try_into().unwrap();
        assert!(std::ptr::eq(ic.as_ptr(), ic2.as_ptr()));
    }

    #[test]
    fn terminator_cached_after_invalid() {
        use std::sync::atomic::Ordering;
        // Invalid: marker should encode position after try_from
        let i = Intern::new("ab\0cd"); // null at position 2
        let _: Result<InternC, _> = i.try_into();
        // position 2 → stored as 3
        assert_eq!(i.terminator().load(Ordering::Relaxed), 3);
        // Second try_from should return Err immediately with correct position
        let err = InternC::try_from(i).unwrap_err();
        assert_eq!(err.nul_position(), 2);
    }
}
