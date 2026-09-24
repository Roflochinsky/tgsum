//! Lenient field deserializers for Telegram's `result.json`.
//!
//! The export is loosely typed: ids are numbers or strings, `text` is a string
//! or an array of runs, old exports use other shapes. Every helper here accepts
//! any JSON value and coerces it the way the original JS implementation did, so
//! an odd field never aborts parsing a multi-GB file.

use std::fmt;

use serde::de::{DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

use crate::text::js_number;

/// Implements the scalar visitor methods by routing each one to `self.$m(..)`.
macro_rules! scalars_via {
    ($m:ident) => {
        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
            Ok(self.$m(Scalar::Str(v)))
        }
        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
            Ok(self.$m(Scalar::Text(v.to_string())))
        }
        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
            Ok(self.$m(Scalar::Text(v.to_string())))
        }
        fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
            Ok(self.$m(Scalar::Num(v)))
        }
        fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
            Ok(self.$m(Scalar::Bool(v)))
        }
        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(self.$m(Scalar::Null))
        }
        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(self.$m(Scalar::Null))
        }
    };
}

/// A JSON scalar as seen by a visitor.
enum Scalar<'a> {
    Str(&'a str),
    Text(String),
    Num(f64),
    Bool(bool),
    Null,
}

impl Scalar<'_> {
    /// JS `String(x)`; `null` stays `None`.
    fn into_string(self) -> Option<String> {
        match self {
            Scalar::Str(s) => Some(s.to_owned()),
            Scalar::Text(s) => Some(s),
            Scalar::Num(n) => Some(js_number(n)),
            Scalar::Bool(b) => Some(b.to_string()),
            Scalar::Null => None,
        }
    }
}

fn drain_seq<'de, A: SeqAccess<'de>>(mut seq: A) -> Result<(), A::Error> {
    while seq.next_element::<IgnoredAny>()?.is_some() {}
    Ok(())
}

fn drain_map<'de, A: MapAccess<'de>>(mut map: A) -> Result<(), A::Error> {
    while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
    Ok(())
}

struct StringVisitor;

impl StringVisitor {
    fn scalar(self, s: Scalar) -> Option<String> {
        s.into_string()
    }
}

impl<'de> Visitor<'de> for StringVisitor {
    type Value = Option<String>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }

    scalars_via!(scalar);

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_any(self)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        drain_seq(seq).map(|_| None)
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        drain_map(map).map(|_| None)
    }
}

/// Any value as a string (numbers keep their exact digits, so 64-bit ids
/// survive); `null`, arrays and objects become `None`.
pub fn opt_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    d.deserialize_any(StringVisitor)
}

/// Like [`opt_string`], with `None` collapsed to an empty string.
pub fn string<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(opt_string(d)?.unwrap_or_default())
}

/// `type == "service"`.
pub fn is_service<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    Ok(opt_string(d)?.as_deref() == Some("service"))
}

struct TruthyVisitor;

impl TruthyVisitor {
    fn scalar(self, s: Scalar) -> bool {
        match s {
            Scalar::Str(s) => !s.is_empty(),
            Scalar::Text(s) => s != "0",
            Scalar::Num(n) => n != 0.0 && !n.is_nan(),
            Scalar::Bool(b) => b,
            Scalar::Null => false,
        }
    }
}

impl<'de> Visitor<'de> for TruthyVisitor {
    type Value = bool;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }

    scalars_via!(scalar);

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_any(self)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        drain_seq(seq).map(|_| true)
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        drain_map(map).map(|_| true)
    }
}

/// JS truthiness (`photo`, `file`: any non-empty path counts).
pub fn truthy<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    d.deserialize_any(TruthyVisitor)
}

struct NumberVisitor;

impl NumberVisitor {
    fn scalar(self, s: Scalar) -> Option<f64> {
        match s {
            Scalar::Str(s) => s.trim().parse().ok(),
            Scalar::Text(s) => s.parse().ok(),
            Scalar::Num(n) => Some(n),
            Scalar::Bool(b) => Some(f64::from(u8::from(b))),
            Scalar::Null => None,
        }
    }
}

impl<'de> Visitor<'de> for NumberVisitor {
    type Value = Option<f64>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }

    scalars_via!(scalar);

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_any(self)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        drain_seq(seq).map(|_| None)
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        drain_map(map).map(|_| None)
    }
}

/// A numeric field (`duration_seconds`); unparseable values become `None`.
pub fn number<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    d.deserialize_any(NumberVisitor)
}

/// Map-key seed answering "is this key `name`?" without allocating.
pub(crate) struct KeyIs(pub &'static str);

impl<'de> DeserializeSeed<'de> for KeyIs {
    type Value = bool;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<bool, D::Error> {
        d.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for KeyIs {
    type Value = bool;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an object key")
    }
    fn visit_str<E>(self, v: &str) -> Result<bool, E> {
        Ok(v == self.0)
    }
}

/// Appends one run of a `text` / `text_entities` array: a plain string or an
/// object whose `text` field is used (JS: `typeof r === 'string' ? r : r.text`).
struct Run<'a>(&'a mut String);

impl Run<'_> {
    // Non-string scalars have no `.text` in JS, so they contribute nothing.
    fn scalar(self, s: Scalar) {
        if let Scalar::Str(s) = s {
            self.0.push_str(s)
        }
    }
}

impl<'de> DeserializeSeed<'de> for Run<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Run<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a text run")
    }

    scalars_via!(scalar);

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_any(self)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<(), A::Error> {
        drain_seq(seq)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut text = None;
        while let Some(is_text) = map.next_key_seed(KeyIs("text"))? {
            if is_text {
                text = map.next_value_seed(OptString)?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        if let Some(t) = text {
            self.0.push_str(&t);
        }
        Ok(())
    }
}

struct OptString;

impl<'de> DeserializeSeed<'de> for OptString {
    type Value = Option<String>;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        opt_string(d)
    }
}

struct TextVisitor {
    /// `text_entities` only counts when it is a non-empty array.
    entities: bool,
}

impl TextVisitor {
    fn scalar(self, s: Scalar) -> Option<String> {
        match s {
            Scalar::Str(s) if !self.entities => Some(s.to_owned()),
            _ => None,
        }
    }
}

impl<'de> Visitor<'de> for TextVisitor {
    type Value = Option<String>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a string or an array of text runs")
    }

    scalars_via!(scalar);

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_any(self)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut out = String::new();
        let mut runs = 0usize;
        while seq.next_element_seed(Run(&mut out))?.is_some() {
            runs += 1;
        }
        Ok((runs > 0 || !self.entities).then_some(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        drain_map(map).map(|_| None)
    }
}

/// `text`: a string, or an array of runs flattened by concatenation.
pub fn text<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    d.deserialize_any(TextVisitor { entities: false })
}

/// `text_entities`: flattened runs, `None` unless it is a non-empty array.
pub fn text_entities<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    d.deserialize_any(TextVisitor { entities: true })
}
