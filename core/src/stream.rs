//! Streaming reader for `result.json`.
//!
//! A full account export is one pretty-printed JSON document of up to a few
//! GB. It is never loaded whole: a serde visitor walks the root object, skips
//! everything but `chats.list`, and hands over one chat at a time. A per-chat
//! export is a single root chat object and uses the same message deserializer.
//! Memory is bounded by one chat, not by one message.

use std::cell::Cell;
use std::fmt;
use std::io::{self, BufReader, Read};
use std::marker::PhantomData;
use std::ops::ControlFlow;

use serde::de::{
    self, DeserializeOwned, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess,
    Visitor,
};

use crate::de::KeyIs;
use crate::model::RawChat;

/// Calls `on_chat` for every chat in `chats.list[]` or the single root chat. Returning
/// [`ControlFlow::Break`] stops reading the rest of the file.
/// Callbacks run before EOF validation: use `Continue` and stage any persistent
/// writes until this function succeeds when importing an entire snapshot.
///
/// `M` picks how much of each message is kept (e.g. [`crate::MessageMeta`] for
/// the light index pass, [`crate::RawMessage`] for extraction).
pub fn stream_chats<M, R, F>(reader: R, mut on_chat: F) -> io::Result<()>
where
    M: DeserializeOwned,
    R: Read,
    F: FnMut(RawChat<M>) -> ControlFlow<()>,
{
    let stopped = Cell::new(false);
    let mut de = serde_json::Deserializer::from_reader(BufReader::with_capacity(1 << 18, reader));
    let root = Root {
        on_chat: &mut on_chat,
        stopped: &stopped,
        _m: PhantomData,
    };
    match root.deserialize(&mut de).and_then(|()| de.end()) {
        Ok(()) => Ok(()),
        Err(_) if stopped.get() => Ok(()),
        Err(e) if e.is_io() => Err(e.into()),
        Err(e) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("не похоже на выгрузку Telegram (result.json): {e}"),
        )),
    }
}

struct Root<'a, M, F> {
    on_chat: &'a mut F,
    stopped: &'a Cell<bool>,
    _m: PhantomData<fn() -> M>,
}

impl<'de, M, F> DeserializeSeed<'de> for Root<'_, M, F>
where
    M: DeserializeOwned,
    F: FnMut(RawChat<M>) -> ControlFlow<()>,
{
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_map(self)
    }
}

impl<'de, M, F> Visitor<'de> for Root<'_, M, F>
where
    M: DeserializeOwned,
    F: FnMut(RawChat<M>) -> ControlFlow<()>,
{
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a Telegram export object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut full = false;
        let mut id = None;
        let mut name = None;
        let mut kind = None;
        let mut messages = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "chats" if !full && messages.is_none() => {
                    full = true;
                    map.next_value_seed(Chats {
                        on_chat: &mut *self.on_chat,
                        stopped: self.stopped,
                        _m: PhantomData,
                    })?;
                }
                "messages" if !full && messages.is_none() => {
                    messages = Some(map.next_value::<Vec<M>>()?);
                }
                "id" if id.is_none() => id = Some(map.next_value_seed(crate::de::Id)?),
                "name" if name.is_none() => name = Some(map.next_value_seed(crate::de::OptString)?),
                "type" if kind.is_none() => {
                    kind = Some(
                        map.next_value_seed(crate::de::OptString)?
                            .unwrap_or_default(),
                    )
                }
                "chats" | "messages" | "id" | "name" | "type" => {
                    return Err(de::Error::custom("duplicate or mixed export fields"))
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        if !full {
            let id = id
                .filter(|s| !s.is_empty())
                .ok_or_else(|| de::Error::custom("missing chat id"))?;
            let messages = messages.ok_or_else(|| de::Error::custom("missing chat messages"))?;
            if (self.on_chat)(RawChat {
                id,
                name: name.flatten(),
                kind: kind.unwrap_or_default(),
                messages,
            })
            .is_break()
            {
                self.stopped.set(true);
                return Err(de::Error::custom("stopped early"));
            }
        }
        Ok(())
    }
}

/// The `chats` object: only its `list` is read.
struct Chats<'a, M, F> {
    on_chat: &'a mut F,
    stopped: &'a Cell<bool>,
    _m: PhantomData<fn() -> M>,
}

impl<'de, M, F> DeserializeSeed<'de> for Chats<'_, M, F>
where
    M: DeserializeOwned,
    F: FnMut(RawChat<M>) -> ControlFlow<()>,
{
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_map(self)
    }
}

impl<'de, M, F> Visitor<'de> for Chats<'_, M, F>
where
    M: DeserializeOwned,
    F: FnMut(RawChat<M>) -> ControlFlow<()>,
{
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("the `chats` object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut found = false;
        while let Some(is_list) = map.next_key_seed(KeyIs("list"))? {
            if is_list {
                if found {
                    return Err(de::Error::duplicate_field("list"));
                }
                found = true;
                map.next_value_seed(List {
                    on_chat: &mut *self.on_chat,
                    stopped: self.stopped,
                    _m: PhantomData,
                })?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        if !found {
            return Err(de::Error::missing_field("list"));
        }
        Ok(())
    }
}

/// `chats.list`: each element is deserialized and handed to the callback.
struct List<'a, M, F> {
    on_chat: &'a mut F,
    stopped: &'a Cell<bool>,
    _m: PhantomData<fn() -> M>,
}

impl<'de, M, F> DeserializeSeed<'de> for List<'_, M, F>
where
    M: DeserializeOwned,
    F: FnMut(RawChat<M>) -> ControlFlow<()>,
{
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}

impl<'de, M, F> Visitor<'de> for List<'_, M, F>
where
    M: DeserializeOwned,
    F: FnMut(RawChat<M>) -> ControlFlow<()>,
{
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("the `chats.list` array")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while let Some(chat) = seq.next_element::<RawChat<M>>()? {
            if (self.on_chat)(chat).is_break() {
                // Unwind out of the parser; `stream_chats` reports success.
                self.stopped.set(true);
                return Err(de::Error::custom("stopped early"));
            }
        }
        Ok(())
    }
}
