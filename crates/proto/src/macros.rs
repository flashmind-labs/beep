//! The macros that declare wire enums and the request table.

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
        $($op:literal $name:ident { $($field:ident: $ty:ty),* $(,)? })*
    }) => {
        $(#[$meta])*
        pub enum $enum {
            $($name { $($field: $ty),* },)*
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

        #[cfg(test)]
        impl $crate::compat::Sample for $enum {
            fn samples() -> Vec<Self> {
                let mut out = Vec::new();
                $(sample_each!(out, [Self::$name] { $($field),* });)*
                out
            }
        }
    };
}

/// `wire_enum!` for an enum Kotlin and Swift see too.
macro_rules! ffi_enum {
    ($(#[$meta:meta])* pub enum $enum:ident $body:tt) => {
        wire_enum! {
            #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
            $(#[$meta])*
            pub enum $enum $body
        }
    };
}

/// Plain structs on the wire: serde, the usual derives (`Debug`, `Clone`,
/// `PartialEq`; a struct's own `#[derive]` adds to them), and the samples the
/// compat test checks them with. The apps' wire encodes them with field
/// names, the node link as their fields in order.
macro_rules! wire_structs {
    ($($(#[$meta:meta])* pub struct $name:ident {
        $($(#[$fmeta:meta])* pub $field:ident: $ty:ty),* $(,)?
    })*) => {$(
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        $(#[$meta])*
        pub struct $name {
            $($(#[$fmeta])* pub $field: $ty,)*
        }

        #[cfg(test)]
        sample_struct!($name { $($field),* });
    )*};
}

/// `wire_structs!` for structs Kotlin and Swift see too.
macro_rules! ffi_structs {
    ($($(#[$meta:meta])* pub struct $name:ident $body:tt)*) => {
        wire_structs! {
            $(
                #[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
                $(#[$meta])*
                pub struct $name $body
            )*
        }
    };
}

/// Declares every request: its op, its fields and what it replies with.
macro_rules! requests {
    ($($(#[$meta:meta])* $op:literal $name:ident { $($(#[$fmeta:meta])* $field:ident: $ty:ty),* $(,)? } => $kind:ident<$reply:ty>)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u16)]
        pub enum Op {
            $($name = $op,)*
        }

        impl Op {
            pub const ALL: &[Op] = &[$(Op::$name),*];
        }

        /// Hands `visit` every request type with its reply type, in op order.
        #[cfg(test)]
        pub(crate) fn each_request(visit: &mut impl $crate::compat::Visit) {
            $(visit.request::<$name, $reply>();)*
        }

        impl TryFrom<u16> for Op {
            type Error = ApiError;

            fn try_from(op: u16) -> Result<Self, ApiError> {
                match op {
                    $($op => Ok(Op::$name),)*
                    _ => Err(ApiError::bad_request(format!("unknown op {op}"))),
                }
            }
        }

        $(
            $(#[$meta])*
            #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
            pub struct $name {
                $($(#[$fmeta])* pub $field: $ty,)*
            }

            impl Rpc for $name {
                const OP: Op = Op::$name;
            }

            impl $kind for $name {
                type Reply = $reply;
            }

            #[cfg(test)]
            sample_struct!($name { $($field),* });
        )*
    };
}
