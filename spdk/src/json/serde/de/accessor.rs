use super::{Deserializer, Error, Result};

/// Provides a [`Deserializer`]'s [`Visitor`] access to sequences, map and enum variants.
///
/// [`Visitor`]: serde::de::Visitor
pub(crate) struct Accessor<'a, 'de: 'a, 'b> {
    de: &'a mut Deserializer<'de, 'b>,
}

impl<'a, 'de, 'b> Accessor<'a, 'de, 'b> {
    pub(crate) fn new(de: &'a mut Deserializer<'de, 'b>) -> Self {
        Self { de }
    }
}

impl<'a, 'de> serde::de::SeqAccess<'de> for Accessor<'a, 'de, '_> {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>>
    where
        T: serde::de::DeserializeSeed<'de>,
    {
        let v = self.de.peek()?;

        if v.is_array_end() {
            self.de.discard_next();
            Ok(None)
        } else if !v.is_object_end() {
            seed.deserialize(&mut *self.de).map(Some)
        } else {
            Err(Error::ParseFailed)
        }
    }
}

impl<'a, 'de> serde::de::MapAccess<'de> for Accessor<'a, 'de, '_> {
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>>
    where
        K: serde::de::DeserializeSeed<'de>,
    {
        let v = self.de.peek()?;

        if v.is_object_end() {
            self.de.discard_next();
            Ok(None)
        } else if !v.is_array_end() {
            seed.deserialize(&mut *self.de).map(Some)
        } else {
            Err(Error::ParseFailed)
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value>
    where
        V: serde::de::DeserializeSeed<'de>,
    {
        seed.deserialize(&mut *self.de)
    }
}

impl<'de, 'a> serde::de::EnumAccess<'de> for Accessor<'a, 'de, '_> {
    type Error = Error;
    type Variant = Self;

    #[inline(always)]
    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant)>
    where
        V: serde::de::DeserializeSeed<'de>,
    {
        seed.deserialize(&mut *self.de).map(|v| (v, self))
    }
}

impl<'de, 'a> serde::de::VariantAccess<'de> for Accessor<'a, 'de, '_> {
    type Error = Error;

    #[inline(always)]
    fn unit_variant(self) -> Result<()> {
        serde::de::Deserialize::deserialize(&mut *self.de)
    }

    #[inline(always)]
    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value>
    where
        T: serde::de::DeserializeSeed<'de>,
    {
        seed.deserialize(&mut *self.de)
    }

    #[inline(always)]
    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        serde::de::Deserializer::deserialize_seq(&mut *self.de, visitor)
    }

    #[inline(always)]
    fn struct_variant<V>(self, fields: &'static [&'static str], visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        serde::de::Deserializer::deserialize_struct(&mut *self.de, "", fields, visitor)
    }
}
