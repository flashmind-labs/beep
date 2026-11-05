//! Reading compact msgpack the way a field-by-position format has to: a
//! struct (or tuple, or struct variant) that arrives with more elements than
//! this build knows keeps the ones it knows and skips the rest. `rmp_serde`
//! alone refuses such an array, which would make every added field break
//! every older reader. A map with unknown keys is skipped by serde already.
//!
//! Short arrays need nothing here: a field added at the end carries
//! `#[serde(default)]`.

use serde::de::{
    self, DeserializeOwned, DeserializeSeed, Deserializer, EnumAccess, IgnoredAny, MapAccess,
    SeqAccess, VariantAccess, Visitor,
};
use std::fmt;

/// Reads one value from msgpack, skipping unknown trailing elements at every
/// depth. Bytes after the value are ignored, as `rmp_serde::from_slice` does.
pub fn from_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, rmp_serde::decode::Error> {
    T::deserialize(Lenient(&mut rmp_serde::Deserializer::new(bytes)))
}

struct Lenient<D>(D);

macro_rules! forward {
    ($($method:ident),* $(,)?) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            self.0.$method(Wrap(visitor))
        }
    )*};
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for Lenient<D> {
    type Error = D::Error;

    forward!(
        deserialize_any,
        deserialize_bool,
        deserialize_i8,
        deserialize_i16,
        deserialize_i32,
        deserialize_i64,
        deserialize_i128,
        deserialize_u8,
        deserialize_u16,
        deserialize_u32,
        deserialize_u64,
        deserialize_u128,
        deserialize_f32,
        deserialize_f64,
        deserialize_char,
        deserialize_str,
        deserialize_string,
        deserialize_bytes,
        deserialize_byte_buf,
        deserialize_option,
        deserialize_unit,
        deserialize_seq,
        deserialize_map,
        deserialize_identifier,
        deserialize_ignored_any,
    );

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.deserialize_unit_struct(name, Wrap(visitor))
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.deserialize_newtype_struct(name, Wrap(visitor))
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.deserialize_tuple(len, Wrap(visitor))
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.deserialize_tuple_struct(name, len, Wrap(visitor))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.deserialize_struct(name, fields, Wrap(visitor))
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.deserialize_enum(name, variants, Wrap(visitor))
    }

    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }
}

/// Hands a visitor lenient accessors, and skips what a sequence has left.
struct Wrap<V>(V);

macro_rules! pass {
    ($($method:ident($ty:ty)),* $(,)?) => {$(
        fn $method<E: de::Error>(self, v: $ty) -> Result<V::Value, E> {
            self.0.$method(v)
        }
    )*};
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Wrap<V> {
    type Value = V::Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.expecting(f)
    }

    pass!(
        visit_bool(bool),
        visit_i8(i8),
        visit_i16(i16),
        visit_i32(i32),
        visit_i64(i64),
        visit_i128(i128),
        visit_u8(u8),
        visit_u16(u16),
        visit_u32(u32),
        visit_u64(u64),
        visit_u128(u128),
        visit_f32(f32),
        visit_f64(f64),
        visit_char(char),
        visit_str(&str),
        visit_string(String),
        visit_bytes(&[u8]),
        visit_byte_buf(Vec<u8>),
    );

    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<V::Value, E> {
        self.0.visit_borrowed_str(v)
    }

    fn visit_borrowed_bytes<E: de::Error>(self, v: &'de [u8]) -> Result<V::Value, E> {
        self.0.visit_borrowed_bytes(v)
    }

    fn visit_none<E: de::Error>(self) -> Result<V::Value, E> {
        self.0.visit_none()
    }

    fn visit_unit<E: de::Error>(self) -> Result<V::Value, E> {
        self.0.visit_unit()
    }

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<V::Value, D::Error> {
        self.0.visit_some(Lenient(d))
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, d: D) -> Result<V::Value, D::Error> {
        self.0.visit_newtype_struct(Lenient(d))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<V::Value, A::Error> {
        let value = self.0.visit_seq(Seq(&mut seq))?;
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(value)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<V::Value, A::Error> {
        self.0.visit_map(Map(map))
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<V::Value, A::Error> {
        self.0.visit_enum(Enum(data))
    }
}

struct Seed<S>(S);

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Seed<S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<S::Value, D::Error> {
        self.0.deserialize(Lenient(d))
    }
}

struct Seq<A>(A);

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Seq<A> {
    type Error = A::Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, A::Error> {
        self.0.next_element_seed(Seed(seed))
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

struct Map<A>(A);

impl<'de, A: MapAccess<'de>> MapAccess<'de> for Map<A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, A::Error> {
        self.0.next_key_seed(Seed(seed))
    }

    fn next_value_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<T::Value, A::Error> {
        self.0.next_value_seed(Seed(seed))
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

struct Enum<A>(A);

impl<'de, A: EnumAccess<'de>> EnumAccess<'de> for Enum<A> {
    type Error = A::Error;
    type Variant = Variant<A::Variant>;

    fn variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<(T::Value, Self::Variant), A::Error> {
        let (value, variant) = self.0.variant_seed(Seed(seed))?;
        Ok((value, Variant(variant)))
    }
}

struct Variant<A>(A);

impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for Variant<A> {
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), A::Error> {
        self.0.unit_variant()
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, A::Error> {
        self.0.newtype_variant_seed(Seed(seed))
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, A::Error> {
        self.0.tuple_variant(len, Wrap(visitor))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        self.0.struct_variant(fields, Wrap(visitor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Inner {
        a: u8,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Older {
        name: String,
        inner: Inner,
        list: Vec<Inner>,
        maybe: Option<Inner>,
        what: Choice,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum Choice {
        Unit,
        New(Inner),
        Tuple(u8, u8),
        Struct { x: u8 },
    }

    #[derive(Serialize)]
    struct NewerInner {
        a: u8,
        added: u8,
    }

    #[derive(Serialize)]
    struct Newer {
        name: String,
        inner: NewerInner,
        list: Vec<NewerInner>,
        maybe: Option<NewerInner>,
        what: NewerChoice,
        added: String,
    }

    #[derive(Serialize)]
    enum NewerChoice {
        New(NewerInner),
        Tuple(u8, u8, u8),
        Struct { x: u8, added: u8 },
    }

    fn newer(what: NewerChoice) -> Newer {
        let inner = || NewerInner { a: 1, added: 9 };
        Newer {
            name: "n".into(),
            inner: inner(),
            list: vec![inner(), inner()],
            maybe: Some(inner()),
            what,
            added: "extra".into(),
        }
    }

    fn older(what: Choice) -> Older {
        let inner = || Inner { a: 1 };
        Older {
            name: "n".into(),
            inner: inner(),
            list: vec![inner(), inner()],
            maybe: Some(inner()),
            what,
        }
    }

    #[test]
    fn plain_rmp_refuses_what_this_reader_skips() {
        let bytes = rmp_serde::to_vec(&newer(NewerChoice::New(NewerInner { a: 2, added: 3 })))
            .expect("encodes");
        assert!(rmp_serde::from_slice::<Older>(&bytes).is_err());
        assert_eq!(
            from_slice::<Older>(&bytes).expect("decodes"),
            older(Choice::New(Inner { a: 2 }))
        );
    }

    #[test]
    fn unknown_trailing_elements_are_skipped_at_every_depth() {
        let cases = [
            (NewerChoice::Tuple(1, 2, 3), Choice::Tuple(1, 2)),
            (
                NewerChoice::Struct { x: 5, added: 6 },
                Choice::Struct { x: 5 },
            ),
        ];
        for (sent, expect) in cases {
            let bytes = rmp_serde::to_vec(&newer(sent)).expect("encodes");
            assert_eq!(from_slice::<Older>(&bytes).expect("decodes"), older(expect));
        }
    }

    #[test]
    fn maps_arrays_and_short_arrays_still_read() {
        #[derive(Debug, PartialEq, Deserialize)]
        struct Grown {
            a: u8,
            #[serde(default)]
            b: u8,
        }
        let named = rmp_serde::to_vec_named(&Inner { a: 4 }).expect("encodes");
        let compact = rmp_serde::to_vec(&Inner { a: 4 }).expect("encodes");
        for bytes in [named, compact] {
            assert_eq!(
                from_slice::<Grown>(&bytes).expect("decodes"),
                Grown { a: 4, b: 0 }
            );
        }
        let unknown_key = rmp_serde::to_vec_named(&NewerInner { a: 1, added: 2 }).expect("encodes");
        assert_eq!(
            from_slice::<Inner>(&unknown_key).expect("decodes"),
            Inner { a: 1 }
        );
        let map = BTreeMap::from([(1u8, "x".to_owned())]);
        let bytes = rmp_serde::to_vec(&map).expect("encodes");
        assert_eq!(
            from_slice::<BTreeMap<u8, String>>(&bytes).expect("decodes"),
            map
        );
    }

    #[test]
    fn what_is_wrong_is_still_refused() {
        let unknown_variant = rmp_serde::to_vec(&"Nope").expect("encodes");
        assert!(from_slice::<Choice>(&unknown_variant).is_err());
        let wrong_type = rmp_serde::to_vec(&("x", 1u8)).expect("encodes");
        assert!(from_slice::<Inner>(&wrong_type).is_err());
        let too_short = rmp_serde::to_vec(&()).expect("encodes");
        assert!(from_slice::<Inner>(&too_short).is_err());
        assert!(from_slice::<Inner>(&[]).is_err());
    }
}
