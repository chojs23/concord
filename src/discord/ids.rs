use std::{fmt, marker::PhantomData, num::NonZeroU64};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(transparent)]
pub struct Id<T> {
    value: NonZeroU64,
    marker: PhantomData<fn(T) -> T>,
}

impl<T> Id<T> {
    pub const fn new(value: u64) -> Self {
        let Some(value) = NonZeroU64::new(value) else {
            panic!("Discord snowflake ids must be non-zero");
        };
        Self {
            value,
            marker: PhantomData,
        }
    }

    pub const fn new_checked(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self {
                value,
                marker: PhantomData,
            }),
            None => None,
        }
    }

    pub const fn get(self) -> u64 {
        self.value.get()
    }
}

impl<T> fmt::Display for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.get().fmt(f)
    }
}

impl<T> Serialize for Id<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.value.get().to_string())
    }
}

impl<'de, T> Deserialize<'de> for Id<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct IdVisitor<T>(PhantomData<fn(T) -> T>);

        impl<T> de::Visitor<'_> for IdVisitor<T> {
            type Value = Id<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a non-zero Discord snowflake as a string or integer")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Id::new_checked(value).ok_or_else(|| E::custom("Discord snowflake id was zero"))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let value = value.parse::<u64>().map_err(E::custom)?;
                self.visit_u64(value)
            }
        }

        deserializer.deserialize_any(IdVisitor(PhantomData))
    }
}

pub mod marker {
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct AttachmentMarker;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct ChannelMarker;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct EmojiMarker;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct GuildMarker;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct MessageMarker;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct RoleMarker;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct UserMarker;
}
