use core::borrow::Borrow;
use core::ffi::CStr;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::mem::{align_of, size_of};
use core::ptr::NonNull;
use std::alloc::{Layout, alloc};
use std::collections::HashSet;
use std::sync::{LazyLock, RwLock};

#[cfg(feature = "serde")]
use serde::de::{Deserialize, Deserializer, Visitor};

// Private thin-pointer wrapper stored in the intern set.
// Hash and Eq are content-based for deduplication; Borrow<str> lets
// set.get(s) accept a plain &str without any fat-pointer storage.
struct InternPtr(NonNull<u8>);

unsafe impl Send for InternPtr {}
unsafe impl Sync for InternPtr {}

impl InternPtr {
    fn as_str(&self) -> &'static str {
        unsafe {
            let ptr = self.0.as_ptr();
            let len = *ptr.sub(size_of::<usize>()).cast::<usize>();
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len))
        }
    }
}

impl Hash for InternPtr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl PartialEq for InternPtr {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for InternPtr {}

impl Borrow<str> for InternPtr {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

static INTERNED: LazyLock<RwLock<HashSet<InternPtr>>> =
    LazyLock::new(|| RwLock::new(HashSet::new()));

// Allocates [len: usize][utf-8 bytes][null byte] and returns a pointer to the bytes.
fn alloc_length_prefixed(s: &str) -> NonNull<u8> {
    let layout = Layout::from_size_align(size_of::<usize>() + s.len() + 1, align_of::<usize>())
        .expect("layout overflow");
    unsafe {
        let base = alloc(layout);
        assert!(!base.is_null(), "allocation failed");
        base.cast::<usize>().write(s.len());
        let bytes = base.add(size_of::<usize>());
        bytes.copy_from_nonoverlapping(s.as_ptr(), s.len());
        bytes.add(s.len()).write(0); // null terminator for CStr support
        NonNull::new_unchecked(bytes)
    }
}

#[derive(Clone, Copy)]
pub struct Intern(NonNull<u8>);

unsafe impl Send for Intern {}
unsafe impl Sync for Intern {}

impl Intern {
    pub fn new(s: impl AsRef<str>) -> Self {
        let s = s.as_ref();

        // Try read lock first for the common case
        {
            let set = INTERNED.read().unwrap();
            if let Some(existing) = set.get(s) {
                return Intern(existing.0);
            }
        }

        // Need to insert - take write lock
        let mut set = INTERNED.write().unwrap();

        // Double-check in case another thread inserted while we waited
        if let Some(existing) = set.get(s) {
            return Intern(existing.0);
        }

        let ptr = alloc_length_prefixed(s);
        set.insert(InternPtr(ptr));
        Intern(ptr)
    }

    pub fn from_static(s: &'static str) -> Self {
        Self::new(s)
    }

    pub fn as_str(&self) -> &'static str {
        unsafe {
            let ptr = self.0.as_ptr();
            let len = *ptr.sub(size_of::<usize>()).cast::<usize>();
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len))
        }
    }

    /// Returns the pointer address - useful for debugging interning behavior
    pub fn as_ptr(&self) -> *const u8 {
        self.0.as_ptr()
    }
}

// Pointer-based equality - two Interns are equal iff they point to the same address
impl PartialEq for Intern {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.0.as_ptr(), other.0.as_ptr())
    }
}

impl Eq for Intern {}

// Pointer-based hashing for consistency with PartialEq
impl Hash for Intern {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_ptr().hash(state);
    }
}

// Ordering still uses string ordering for sensible sort behavior
impl PartialOrd for Intern {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Intern {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Fast path: same pointer means equal
        if std::ptr::eq(self.0.as_ptr(), other.0.as_ptr()) {
            std::cmp::Ordering::Equal
        } else {
            self.as_str().cmp(other.as_str())
        }
    }
}

impl fmt::Display for Intern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for Intern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Intern({:?})", self.as_str())
    }
}

impl AsRef<str> for Intern {
    fn as_ref(&self) -> &'static str {
        self.as_str()
    }
}

impl AsRef<std::path::Path> for Intern {
    fn as_ref(&self) -> &'static std::path::Path {
        std::path::Path::new(self.as_str())
    }
}

impl AsRef<CStr> for Intern {
    fn as_ref(&self) -> &'static CStr {
        // SAFETY: alloc_length_prefixed writes a null byte after the string data,
        // so the pointer is always null-terminated. The 'static lifetime is valid
        // because interned allocations are never freed.
        unsafe { CStr::from_ptr(self.0.as_ptr() as *const std::ffi::c_char) }
    }
}

impl std::ops::Deref for Intern {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl From<&str> for Intern {
    fn from(value: &str) -> Self {
        Intern::new(value)
    }
}

impl From<String> for Intern {
    fn from(value: String) -> Self {
        Intern::new(value)
    }
}

impl From<Box<str>> for Intern {
    fn from(value: Box<str>) -> Self {
        Intern::new(&*value)
    }
}

impl From<std::borrow::Cow<'_, str>> for Intern {
    fn from(value: std::borrow::Cow<'_, str>) -> Self {
        Intern::new(value)
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for Intern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct InternVisitor;

        impl Visitor<'_> for InternVisitor {
            type Value = Intern;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Intern::new(v))
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Intern::new(v))
            }
        }

        deserializer.deserialize_str(InternVisitor)
    }
}

impl Default for Intern {
    fn default() -> Self {
        Intern::from_static("default_intern")
    }
}
