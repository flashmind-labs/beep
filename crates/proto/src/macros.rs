//! The macros that declare wire enums and structs.

macro_rules! one {
    ($x:tt) => {
        1
    };
}

/// Enum serialized as `[op, field...]` instead of serde's `{"Variant": ...}`.
/// A variant's fields are read by position: a variant never gains one, and
/// a new shape is a new variant.
macro_rules! wire_enum {
    ($(#[$meta:meta])* pub enum $enum:ident {
        $($(#[$variant_meta:meta])* $op:literal $name:ident { $($field:ident: $ty:ty),* $(,)? })*
    }) => {
        $(#[$meta])*
        pub enum $enum {
            $($(#[$variant_meta])* $name { $($field: $ty),* },)*
        }

        impl serde::Serialize for $enum {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                use serde::ser::SerializeTuple;

                match self {
                    $(Self::$name { $($field),* } => {
                        let mut t = s.serialize_tuple(1 $(+ one!($field))*)?;
                        t.serialize_element(&($op as u8))?;
                        $(t.serialize_element($field)?;)*
                        t.end()
                    })*
                }
            }
        }

        impl<'de> serde::Deserialize<'de> for $enum {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                use serde::de::{Error, SeqAccess, Visitor};
                struct V;
                impl<'de> Visitor<'de> for V {
                    type Value = $enum;
                    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                        f.write_str(concat!("[op, field...] for ", stringify!($enum)))
                    }
                    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<$enum, A::Error> {
                        let op: u8 = seq.next_element()?.ok_or_else(|| A::Error::custom("missing op"))?;

                        match op {
                            $($op => Ok($enum::$name {
                                $($field: seq.next_element()?.ok_or_else(|| {
                                    A::Error::custom(concat!("missing ", stringify!($field)))
                                })?),*
                            }),)*
                            _ => Err(A::Error::custom(format_args!("unknown op {op}"))),
                        }
                    }
                }
                d.deserialize_seq(V)
            }
        }
    };
}

/// Plain structs on the wire: serde and the usual derives (`Debug`, `Clone`,
/// `PartialEq`; a struct's own `#[derive]` adds to them). The node link
/// encodes them as their fields in order.
macro_rules! wire_structs {
    ($($(#[$meta:meta])* pub struct $name:ident {
        $($(#[$fmeta:meta])* pub $field:ident: $ty:ty),* $(,)?
    })*) => {$(
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        $(#[$meta])*
        pub struct $name {
            $($(#[$fmeta])* pub $field: $ty,)*
        }
    )*};
}
