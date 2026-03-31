use core::borrow::Borrow;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::mem::{align_of, size_of};
use core::ptr::NonNull;
use std::alloc::{Layout, alloc};
use std::collections::HashSet;
use std::sync::{LazyLock, RwLock};

#[cfg(feature = "cstr")]
mod cstr;
#[cfg(feature = "cstr")]
pub use cstr::InteriorNulError;

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
        #[cfg(feature = "cstr")]
        assert!(
            !s.bytes().any(|b| b == 0),
            "interned string contains interior null byte"
        );
        Self::intern(s)
    }

    pub(crate) fn intern(s: &str) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    // --- Interning / deduplication ---

    #[test]
    fn same_string_same_pointer() {
        let a = Intern::new("hello");
        let b = Intern::new("hello");
        assert!(std::ptr::eq(a.as_ptr(), b.as_ptr()), "identical strings must share a pointer");
    }

    #[test]
    fn different_strings_different_pointers() {
        let a = Intern::new("foo");
        let b = Intern::new("bar");
        assert!(!std::ptr::eq(a.as_ptr(), b.as_ptr()));
    }

    #[test]
    fn empty_string() {
        let a = Intern::new("");
        let b = Intern::new("");
        assert_eq!(a.as_str(), "");
        assert!(std::ptr::eq(a.as_ptr(), b.as_ptr()));
    }

    #[test]
    fn round_trip_content() {
        let s = "the quick brown fox";
        assert_eq!(Intern::new(s).as_str(), s);
    }

    #[test]
    fn unicode_content() {
        let s = "héllo wörld 🦀";
        let a = Intern::new(s);
        let b = Intern::new(s);
        assert_eq!(a.as_str(), s);
        assert!(std::ptr::eq(a.as_ptr(), b.as_ptr()));
    }

    // --- Equality / hashing ---

    #[test]
    fn eq_same_content() {
        assert_eq!(Intern::new("abc"), Intern::new("abc"));
    }

    #[test]
    fn ne_different_content() {
        assert_ne!(Intern::new("abc"), Intern::new("xyz"));
    }

    #[test]
    fn hash_consistency() {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let hash = |i: Intern| {
            let mut h = DefaultHasher::new();
            i.hash(&mut h);
            h.finish()
        };
        let a = Intern::new("consistent");
        let b = Intern::new("consistent");
        assert_eq!(hash(a), hash(b));
    }

    #[test]
    fn usable_as_hashmap_key() {
        let mut map: HashMap<Intern, i32> = HashMap::new();
        let key = Intern::new("key");
        map.insert(key, 42);
        assert_eq!(map[&Intern::new("key")], 42);
    }

    #[test]
    fn usable_in_hashset() {
        let mut set: HashSet<Intern> = HashSet::new();
        set.insert(Intern::new("a"));
        set.insert(Intern::new("a")); // duplicate
        set.insert(Intern::new("b"));
        assert_eq!(set.len(), 2);
    }

    // --- Ordering ---

    #[test]
    fn ord_alphabetical() {
        let a = Intern::new("apple");
        let b = Intern::new("banana");
        assert!(a < b);
        assert!(b > a);
    }

    #[test]
    fn ord_same_pointer_is_equal() {
        let a = Intern::new("same");
        assert_eq!(a.cmp(&a), std::cmp::Ordering::Equal);
    }

    #[test]
    fn sort_vec() {
        let mut v = vec![Intern::new("cherry"), Intern::new("apple"), Intern::new("banana")];
        v.sort();
        assert_eq!(v[0].as_str(), "apple");
        assert_eq!(v[1].as_str(), "banana");
        assert_eq!(v[2].as_str(), "cherry");
    }

    // --- Display / Debug ---

    #[test]
    fn display() {
        assert_eq!(format!("{}", Intern::new("hello")), "hello");
    }

    #[test]
    fn debug() {
        assert_eq!(format!("{:?}", Intern::new("hello")), r#"Intern("hello")"#);
    }

    // --- Conversions ---

    #[test]
    fn from_str_ref() {
        let i: Intern = "from &str".into();
        assert_eq!(i.as_str(), "from &str");
    }

    #[test]
    fn from_string() {
        let i: Intern = String::from("from String").into();
        assert_eq!(i.as_str(), "from String");
    }

    #[test]
    fn from_box_str() {
        let i: Intern = Box::<str>::from("from Box<str>").into();
        assert_eq!(i.as_str(), "from Box<str>");
    }

    #[test]
    fn from_cow_borrowed() {
        use std::borrow::Cow;
        let i: Intern = Cow::Borrowed("cow borrowed").into();
        assert_eq!(i.as_str(), "cow borrowed");
    }

    #[test]
    fn from_cow_owned() {
        use std::borrow::Cow;
        let i: Intern = Cow::<str>::Owned(String::from("cow owned")).into();
        assert_eq!(i.as_str(), "cow owned");
    }

    #[test]
    fn deref_to_str() {
        let i = Intern::new("deref me");
        let s: &str = &*i;
        assert_eq!(s, "deref me");
    }

    #[test]
    fn asref_str() {
        let i = Intern::new("asref");
        let s: &str = i.as_ref();
        assert_eq!(s, "asref");
    }

    #[test]
    fn asref_path() {
        use std::path::Path;
        let i = Intern::new("some/path");
        let p: &Path = i.as_ref();
        assert_eq!(p, Path::new("some/path"));
    }

    // --- Copy / Clone / Send / Sync ---

    #[test]
    fn is_copy() {
        let a = Intern::new("copy me");
        let b = a; // copy
        assert_eq!(a, b);
    }

    #[test]
    fn send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Intern>();
    }

    // --- Default ---

    #[test]
    fn default_value() {
        assert_eq!(Intern::default().as_str(), "default_intern");
    }

    // --- Concurrency ---

    #[test]
    fn concurrent_interning_same_string() {
        use std::sync::Arc;
        use std::thread;

        let ptrs: Arc<std::sync::Mutex<Vec<usize>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));

        let handles: Vec<_> = (0..16)
            .map(|_| {
                let ptrs = Arc::clone(&ptrs);
                thread::spawn(move || {
                    let i = Intern::new("concurrent");
                    ptrs.lock().unwrap().push(i.as_ptr() as usize);
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        let ptrs = ptrs.lock().unwrap();
        let first = ptrs[0];
        assert!(ptrs.iter().all(|&p| p == first), "all threads must share one pointer");
    }
}
