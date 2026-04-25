use core::fmt;
use std::{collections::HashMap, fmt::Display, iter};

use plist::{Uid, Value};

// Archive keys used in the keyed archiver plist format. These match the keys used by
// `NSKeyedArchiver` Foundation.
const NK_KEY_ARCHIVE: &str = "$archiver";
const NK_KEY_VERSION: &str = "$version";
const NK_KEY_OBJECTS: &str = "$objects";
const NK_KEY_TOP: &str = "$top";

// Sentinel values used in the keyed archiver plist format.
const NK_VAL_NULL: &str = "$null";
const NK_VAL_VERSION: i32 = 100_000;

/// Errors that can occur during archiving or unarchiving.
enum ArchiveError {
    /// An error originating from the underlying plist layer.
    Plist(plist::Error),
    /// A general error with a descriptive message.
    Other(String),
}

/// A convenience alias for results in this module.
pub type Result<T> = std::result::Result<T, ArchiveError>;

impl Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plist(err) => write!(f, "{err}"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

/// `NSCoder` defines an abstract class that declares the interface used by concrete subclasses to
/// bridge between representations in memory and in a given other format. The `NSKeyedArchiver` and
/// `NSKeyedUnarchiver` are the two concrete subclasses provided by foundatin for this purpose.
trait NSCoder {
    /// Encodes `boolv` and associates it with the given `key`.
    fn encode_bool(&mut self, boolv: bool, key: &str);

    /// Encodes the 32-bit integer `intv` and associates it with the string `key`.
    fn encode_i32(&mut self, intv: i32, key: &str);

    /// Encodes the 64-bit integer `intv` and associates it with the string `key`.
    fn encode_i64(&mut self, intv: i64, key: &str);

    /// Encodes `realv` and associates it with the string `key`.
    fn encode_isize(&mut self, realv: isize, key: &str);

    /// Encodes `realv` and associates it with the string `key`.
    fn encode_f32(&mut self, realv: f32, key: &str);

    /// Encodes `realv` and associates it with the string `key`.
    fn encode_f64(&mut self, realv: f64, key: &str);

    /// Encodes `strv` and associates it with the given `key`.
    ///
    /// This method has no direct equivalent in the Swift `NSCoder` API, where strings are bridged
    /// to `NSString` (Foundation's string class) and encoded as objects via `encode(_:forKey:)`.
    fn encode_string(&mut self, strv: &str, key: &str);

    /// Encodes a buffer of data and associates it with the given `key`.
    ///
    /// Equivalent to `encodeBytes(_:length:forKey:)` in the Swift `NSCoder` API.
    fn encode_bytes(&mut self, value: &[u8], key: &str);

    /// Encodes `objv` and associates it with the given `key`.
    ///
    /// Equivalent to `encode(_:forKey:)` in the Swift `NSCoder` API, where the first argument is
    /// typed as `Any?`. Here, the value is constrained to types implementing the `NSCoding` trait.
    fn encode_object(&mut self, objv: &dyn NSCoding, key: &str);
}

/// `NSCoding` declares the methods that a type must implement so that its values can be encoded and
/// decoded.
///
/// Following the design of Foundation, a value being encoded or decoded is responsible for doing so
/// on its own fields. A coder instructs the value to do so by invoking the encoding methods defined
/// by the [`NSCoder`] trait.
///
/// Because Rust has no classes or inheritance, this trait includes two methods not present in the
/// Foundation source: `class_name` and `class_chain`. These provide the type identity and hierarchy
/// information that would normally be available through the Objective-C runtime.
trait NSCoding {
    fn encode(&self, coder: &mut dyn NSCoder);
    // TODO: Add decoding init method signature

    fn class_name(&self) -> &'static str;
    fn class_chain(&self) -> Vec<&'static str>;
}

impl From<plist::Error> for ArchiveError {
    fn from(err: plist::Error) -> Self {
        Self::Plist(err)
    }
}

enum PlistFormat {
    Binary,
    XML,
}

struct EncodingContext {
    dict: HashMap<String, Value>,
    generic_key: Uid,
}

impl Default for EncodingContext {
    fn default() -> Self {
        Self {
            dict: HashMap::new(),
            generic_key: Uid::new(0),
        }
    }
}

struct ObjectRef(usize);

struct NSKeyedArchiver {
    // NOTE: The _stream field originally contained in the Swift implementation of
    // corelibs-foundation is left out on purpose: Swift and historically Objective-C's memory
    // models build around tightly coupled shared references. In the case of NSKeyedArchiver this
    // meant that both the caller and callee have access and own the same underlying mutable input
    // buffer. Rust's ownership model does not match this pattern. Users configure the archiver,
    // encode their data while letting the archiver borrow a reference to it and finally return the
    // encoded data as a vector of bytes. to the archiver
    containers: Vec<EncodingContext>,
    objects: Vec<Option<Value>>,
    obj_ref_map: HashMap<ObjectRef, Uid>,
    replacement_map: (), // TODO: Implement as replacement_map: HashMap<ObjectRef, Box<dyn Encodable>>
    // where Encodeable should implement some of the methods that are epxected
    // from NSCoding and NSObject
    class_rename_map: HashMap<String, String>,
    classes: HashMap<String, Uid>,
    cache: Vec<Uid>,

    output_format: PlistFormat,

    // archive flags
    finish_encoding: bool,
    secure_encoding: bool,
}

impl NSKeyedArchiver {
    fn new<T: NSCoding>() -> Self {
        Self {
            containers: vec![EncodingContext::default()],
            objects: vec![Some(Value::String(NK_VAL_NULL.to_owned()))],
            obj_ref_map: HashMap::new(),
            replacement_map: (),
            class_rename_map: HashMap::new(),
            classes: HashMap::new(),
            cache: Vec::new(),

            output_format: PlistFormat::Binary,
            finish_encoding: false,
            secure_encoding: true,
        }
    }

    const fn with_output_format(mut self, fmt: PlistFormat) -> Self {
        self.output_format = fmt;
        self
    }

    fn escape_archiver_key(key: &str) -> String {
        if key.starts_with('$') {
            return format!("${key}");
        }
        key.to_owned()
    }
}
