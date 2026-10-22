//! The values the API is spoken in, each checked once where it is made: object
//! ids branded by kind, ref names git would accept, paths that cannot leave
//! the tree, and signatures git writes back unchanged.

use std::fmt;

use gix_hash::ObjectId;
use gix_object::bstr::ByteSlice;

/// A value the caller made that git would refuse or rewrite.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Invalid {
    #[error("{value:?} is not a SHA-1 object id")]
    ObjectId { value: String },
    #[error("{value:?} is not a branch name git accepts: {reason}")]
    Branch { value: String, reason: String },
    #[error("{value:?} is not a tag name git accepts: {reason}")]
    Tag { value: String, reason: String },
    #[error("{value:?} is not a path inside a tree: {reason}")]
    Path { value: String, reason: String },
    #[error("{value:?} cannot be a signature's name or email: {reason}")]
    Identity { value: String, reason: &'static str },
    #[error("{value} is not a time git writes: {reason}")]
    Time { value: i64, reason: &'static str },
}

macro_rules! object_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub(crate) ObjectId);

        impl $name {
            /// Parse 40 hex digits. The kind is a claim until the object is
            /// read, which refuses it if the object is another kind.
            pub fn from_hex(hex: &str) -> Result<Self, Invalid> {
                ObjectId::from_hex(hex.as_bytes())
                    .map(Self)
                    .map_err(|_| Invalid::ObjectId { value: hex.to_owned() })
            }

            pub fn to_hex(&self) -> String {
                self.0.to_string()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.0)
            }
        }
    };
}

object_id!(
    /// A commit's id.
    CommitId
);
object_id!(
    /// A tree's id.
    TreeId
);
object_id!(
    /// A blob's id: a file's bytes.
    BlobId
);
object_id!(
    /// An annotated tag object's id, not the commit it points at.
    TagId
);

pub(crate) fn refname(prefix: &str, name: &str) -> Result<(), String> {
    let full = format!("{prefix}{name}");
    let checked = if prefix == "refs/heads/" {
        gix_validate::reference::branch_name(full.as_bytes().as_bstr()).map(|_| ())
    } else {
        gix_validate::reference::name(full.as_bytes().as_bstr()).map(|_| ())
    };
    checked.map_err(|e| e.to_string())
}

/// A branch, `main` or `deck/lantern`, without `refs/heads/`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct BranchName(String);

impl BranchName {
    pub fn new(name: impl Into<String>) -> Result<Self, Invalid> {
        let name = name.into();
        match refname("refs/heads/", &name) {
            Ok(()) => Ok(Self(name)),
            Err(reason) => Err(Invalid::Branch {
                value: name,
                reason,
            }),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn ref_path(&self) -> String {
        format!("refs/heads/{}", self.0)
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A tag, `v1` or `snapshot/2026-10-21`, without `refs/tags/`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct TagName(String);

impl TagName {
    pub fn new(name: impl Into<String>) -> Result<Self, Invalid> {
        let name = name.into();
        match refname("refs/tags/", &name) {
            Ok(()) => Ok(Self(name)),
            Err(reason) => Err(Invalid::Tag {
                value: name,
                reason,
            }),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn ref_path(&self) -> String {
        format!("refs/tags/{}", self.0)
    }
}

impl fmt::Display for TagName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A file or directory inside a commit's tree: `decks/lantern.deck.toml`.
/// Never empty, never absolute, and no component git would refuse to check
/// out on any of Windows, macOS or Linux (`..`, `.git`, `CON`, NUL).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct RepoPath(String);

impl RepoPath {
    pub fn new(path: impl Into<String>) -> Result<Self, Invalid> {
        let path = path.into();
        let options = gix_validate::path::component::Options {
            protect_windows: true,
            protect_hfs: true,
            protect_ntfs: true,
        };
        for component in path.split('/') {
            if let Err(e) =
                gix_validate::path::component(component.as_bytes().as_bstr(), None, options)
            {
                return Err(Invalid::Path {
                    reason: e.to_string(),
                    value: path,
                });
            }
        }
        Ok(Self(path))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

impl fmt::Display for RepoPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Who: a name and an email as a commit or tag header holds them.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Identity {
    name: String,
    email: String,
}

fn identity_part(value: &str, can_be_empty: bool) -> Result<(), Invalid> {
    let refuse = |reason| {
        Err(Invalid::Identity {
            value: value.to_owned(),
            reason,
        })
    };
    if value.is_empty() && !can_be_empty {
        return refuse("it is empty");
    }
    if value.contains(['<', '>', '\n', '\0']) {
        return refuse("it holds <, >, a newline or NUL, which end the header field");
    }
    // git trims both on read, so they would not survive a round trip.
    if value.trim() != value {
        return refuse("it starts or ends with whitespace");
    }
    Ok(())
}

impl Identity {
    pub fn new(name: impl Into<String>, email: impl Into<String>) -> Result<Self, Invalid> {
        let (name, email) = (name.into(), email.into());
        identity_part(&name, false)?;
        identity_part(&email, true)?;
        Ok(Self { name, email })
    }

    /// As read from an object some other tool wrote, which git accepts even
    /// when [`Identity::new`] would not have.
    pub(crate) fn read(name: String, email: String) -> Self {
        Self { name, email }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn email(&self) -> &str {
        &self.email
    }
}

/// When: seconds since the Unix epoch and the writer's UTC offset. The core
/// never reads a clock; the caller says what time it is.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Timestamp {
    seconds: i64,
    offset_minutes: i32,
}

impl Timestamp {
    /// `offset_minutes` east of UTC: `+0200` is 120. JavaScript's
    /// `getTimezoneOffset()` is the negation of this.
    pub fn new(seconds: i64, offset_minutes: i32) -> Result<Self, Invalid> {
        if seconds < 0 {
            // `git fsck --strict` refuses dates before the epoch.
            return Err(Invalid::Time {
                value: seconds,
                reason: "it is before 1970",
            });
        }
        // git writes the offset as ±HHMM, so a day is as far as it reaches.
        if offset_minutes.abs() >= 24 * 60 {
            return Err(Invalid::Time {
                value: offset_minutes.into(),
                reason: "the offset is a day or more",
            });
        }
        Ok(Self {
            seconds,
            offset_minutes,
        })
    }

    pub(crate) fn read(seconds: i64, offset_seconds: i32) -> Self {
        Self {
            seconds,
            offset_minutes: offset_seconds / 60,
        }
    }

    pub fn seconds(&self) -> i64 {
        self.seconds
    }

    pub fn offset_minutes(&self) -> i32 {
        self.offset_minutes
    }
}

/// A commit's author or committer, or a tag's tagger.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Signature {
    pub who: Identity,
    pub when: Timestamp,
}

impl Signature {
    pub(crate) fn to_gix(&self) -> gix_actor::Signature {
        gix_actor::Signature {
            name: self.who.name.as_str().into(),
            email: self.who.email.as_str().into(),
            time: gix_object::date::Time::new(self.when.seconds, self.when.offset_minutes * 60),
        }
    }
}
