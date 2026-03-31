use core::ffi::CStr;
use core::fmt;
use core::hash::{Hash, Hasher};

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
        let s = s.as_ref();
        assert!(
            !s.bytes().any(|b| b == 0),
            "interned string contains interior null byte"
        );
        InternC(Intern::intern(s))
    }

    /// Interns a string, returning an error if it contains an interior null byte.
    pub fn try_new(s: impl AsRef<str>) -> Result<Self, InteriorNulError> {
        let s = s.as_ref();
        if let Some(pos) = s.bytes().position(|b| b == 0) {
            return Err(InteriorNulError(pos));
        }
        Ok(InternC(Intern::intern(s)))
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
}
