use core::ffi::CStr;
use core::fmt;

use crate::Intern;

/// Error returned by [`Intern::try_new`] when the string contains an interior null byte.
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

impl Intern {
    /// Interns a string, returning an error if it contains an interior null byte.
    pub fn try_new(s: impl AsRef<str>) -> Result<Self, InteriorNulError> {
        let s = s.as_ref();
        if let Some(pos) = s.bytes().position(|b| b == 0) {
            return Err(InteriorNulError(pos));
        }
        Ok(Self::intern(s))
    }
}

impl AsRef<CStr> for Intern {
    fn as_ref(&self) -> &'static CStr {
        // SAFETY: alloc_length_prefixed writes a null byte after the string data.
        // Intern::new panics on interior null bytes when this feature is enabled,
        // so the first null byte is always the terminator. The 'static lifetime
        // is valid because interned allocations are never freed.
        unsafe { CStr::from_ptr(self.0.as_ptr() as *const std::ffi::c_char) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Intern;

    #[test]
    fn asref_cstr() {
        let i = Intern::new("cstr");
        let c: &CStr = i.as_ref();
        assert_eq!(c.to_str().unwrap(), "cstr");
    }

    #[test]
    fn cstr_no_interior_null_in_bytes() {
        // Ensure the null terminator is appended *after* the content.
        let i = Intern::new("abc");
        let c: &CStr = i.as_ref();
        assert_eq!(c.to_bytes(), b"abc");
    }

    #[test]
    fn new_interior_null_panics() {
        let result = std::panic::catch_unwind(|| Intern::new("foo\0bar"));
        assert!(result.is_err());
    }

    #[test]
    fn try_new_interior_null_error() {
        let err = Intern::try_new("foo\0bar").unwrap_err();
        assert_eq!(err.nul_position(), 3);
    }

    #[test]
    fn try_new_ok() {
        let i = Intern::try_new("hello").unwrap();
        let c: &CStr = i.as_ref();
        assert_eq!(c.to_str().unwrap(), "hello");
    }
}
