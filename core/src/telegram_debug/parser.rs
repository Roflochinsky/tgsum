//! Bounded, quote-aware parser for the pinned Telegram Desktop 7.2.5 MTP dump.
//!
//! This accepts a complete dump object, not log lines. Only known update paths
//! emit events: a message nested in a history/auth/unknown response is never
//! mistaken for a live update. Error messages deliberately contain no input.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerKind {
    User,
    Chat,
    Channel,
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct TypedPeer {
    pub kind: PeerKind,
    pub id: String,
}

impl fmt::Debug for TypedPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedPeer")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    New,
    Edit,
    /// Text seen in a known history response, never advertised as a live
    /// arrival or a complete historical snapshot.
    Observed,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ParsedEvent {
    Message {
        kind: MessageKind,
        peer: TypedPeer,
        message_id: String,
        sender: Option<TypedPeer>,
        timestamp: i64,
        text: String,
    },
    Delete {
        peer: TypedPeer,
        message_ids: Vec<String>,
    },
}

impl fmt::Debug for ParsedEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message { kind, .. } => f
                .debug_struct("Message")
                .field("kind", kind)
                .finish_non_exhaustive(),
            Self::Delete { .. } => f.debug_struct("Delete").finish_non_exhaustive(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageGap {
    UnsupportedPacket,
    UnsupportedUpdate,
    UnsupportedMessage,
    IncompleteHistory,
    UnresolvedDelete,
    UnresolvedOutgoing,
    MissingSelfUser,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParsedPacket {
    pub events: Vec<ParsedEvent>,
    pub gaps: Vec<CoverageGap>,
}

#[derive(Clone, Copy, Debug)]
pub struct ParserLimits {
    pub max_bytes: usize,
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_vector_items: usize,
}

impl Default for ParserLimits {
    fn default() -> Self {
        Self {
            max_bytes: 4 * 1024 * 1024,
            max_depth: 64,
            max_nodes: 50_000,
            max_vector_items: 10_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseError {
    InvalidUtf8,
    Malformed,
    UnsupportedEncoding,
    LimitExceeded,
    InvalidIdentity,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidUtf8 => "Telegram dump is not valid UTF-8",
            Self::Malformed => "Telegram dump is malformed or incomplete",
            Self::UnsupportedEncoding => "Telegram dump contains an unsupported value encoding",
            Self::LimitExceeded => "Telegram dump exceeds parser limits",
            Self::InvalidIdentity => "Telegram dump contains an invalid native identity",
        })
    }
}

impl std::error::Error for ParseError {}

/// Parse one complete inbound dump object. The caller owns log framing and
/// account binding. `self_user_id` is necessary for outgoing private short
/// updates; transport DC/key/session fields must never be substituted for it.
pub fn parse_packet(
    input: &[u8],
    self_user_id: Option<&str>,
    limits: ParserLimits,
) -> Result<ParsedPacket, ParseError> {
    if input.len() > limits.max_bytes {
        return Err(ParseError::LimitExceeded);
    }
    let input = std::str::from_utf8(input).map_err(|_| ParseError::InvalidUtf8)?;
    if let Some(id) = self_user_id {
        validate_id(id, i64::MAX)?;
    }
    let mut parser = Parser {
        input,
        cursor: 0,
        nodes: 0,
        limits,
    };
    let value = parser.value(0)?;
    parser.whitespace();
    if parser.cursor != input.len() {
        return Err(ParseError::Malformed);
    }
    let mut packet = ParsedPacket::default();
    collect_packet(&value, self_user_id, &mut packet)?;
    Ok(packet)
}

enum Value<'a> {
    Object(Object<'a>),
    Vector(Vec<Value<'a>>),
    Text(String),
    Atom(&'a str),
}

struct Object<'a> {
    name: &'a str,
    fields: Vec<(&'a str, Value<'a>)>,
}

impl Object<'_> {
    fn field(&self, name: &str) -> Result<&Value<'_>, ParseError> {
        self.optional(name).ok_or(ParseError::Malformed)
    }

    fn optional(&self, name: &str) -> Option<&Value<'_>> {
        self.fields
            .iter()
            .find_map(|(key, value)| (*key == name).then_some(value))
    }
}

struct Parser<'a> {
    input: &'a str,
    cursor: usize,
    nodes: usize,
    limits: ParserLimits,
}

impl<'a> Parser<'a> {
    fn whitespace(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
            self.cursor += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.cursor).copied()
    }

    fn literal(&mut self, expected: &str) -> Result<(), ParseError> {
        if !self.input[self.cursor..].starts_with(expected) {
            return Err(ParseError::Malformed);
        }
        self.cursor += expected.len();
        Ok(())
    }

    fn identifier(&mut self) -> Result<&'a str, ParseError> {
        let start = self.cursor;
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            self.cursor += 1;
        }
        if self.cursor == start {
            return Err(ParseError::Malformed);
        }
        Ok(&self.input[start..self.cursor])
    }

    fn value(&mut self, depth: usize) -> Result<Value<'a>, ParseError> {
        // Even an accidentally unbounded caller limit cannot turn malicious
        // nesting into unbounded recursion on the process stack.
        if depth > self.limits.max_depth.min(128) || self.nodes >= self.limits.max_nodes {
            return Err(ParseError::LimitExceeded);
        }
        self.nodes += 1;
        self.whitespace();
        if self.input[self.cursor..].starts_with("[GZIPPED] ") {
            self.cursor += "[GZIPPED] ".len();
            return self.value(depth + 1);
        }
        if self.input[self.cursor..].starts_with("[LAYER") {
            self.cursor += "[LAYER".len();
            let start = self.cursor;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.cursor += 1;
            }
            let layer = &self.input[start..self.cursor];
            self.literal("] ")?;
            // api.tl from the pinned Desktop 7.2.5 source is LAYER229.
            // A different explicit layer is not implicitly qualified by a
            // compatible-looking message shape.
            if layer != "229" {
                return Err(ParseError::UnsupportedEncoding);
            }
            return self.value(depth + 1);
        }
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') if self.input[self.cursor..].starts_with("[ vector<0x") => {
                self.vector(depth)
            }
            Some(b'"') => self.text(),
            Some(b'[') | None => Err(ParseError::Malformed),
            _ => self.atom(),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value<'a>, ParseError> {
        self.literal("{")?;
        self.whitespace();
        let name = self.identifier()?;
        let mut fields = Vec::new();
        let mut keys = BTreeSet::new();
        loop {
            self.whitespace();
            if self.peek() == Some(b'}') {
                self.cursor += 1;
                break;
            }
            let key = self.identifier()?;
            if !keys.insert(key) {
                return Err(ParseError::Malformed);
            }
            self.whitespace();
            self.literal(":")?;
            let value = self.value(depth + 1)?;
            fields.push((key, value));
            self.whitespace();
            // Every non-empty constructor in the pinned generator has a
            // trailing comma; also accept its absence for a complete object.
            match self.peek() {
                Some(b',') => self.cursor += 1,
                Some(b'}') => {}
                _ => return Err(ParseError::Malformed),
            }
        }
        Ok(Value::Object(Object { name, fields }))
    }

    fn vector(&mut self, depth: usize) -> Result<Value<'a>, ParseError> {
        self.literal("[ vector<0x")?;
        // mtpPrime is signed int32 in the pinned client. In particular the
        // manual container serializer uses mtpc_core_message == -1, so its
        // real header is vector<0x-1>. Other bare types can also have a signed
        // hexadecimal representation; this is a type tag, not item count.
        if self.peek() == Some(b'-') {
            self.cursor += 1;
        }
        let hex_start = self.cursor;
        while self.peek().is_some_and(|b| b.is_ascii_hexdigit()) {
            self.cursor += 1;
        }
        if self.cursor == hex_start {
            return Err(ParseError::Malformed);
        }
        self.literal("> (")?;
        let count_start = self.cursor;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.cursor += 1;
        }
        let count = self.input[count_start..self.cursor]
            .parse::<usize>()
            .map_err(|_| ParseError::Malformed)?;
        if count > self.limits.max_vector_items
            || count > self.limits.max_nodes.saturating_sub(self.nodes)
        {
            return Err(ParseError::LimitExceeded);
        }
        self.literal(")")?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.value(depth + 1)?);
            self.whitespace();
            self.literal(",")?;
        }
        self.whitespace();
        self.literal("]")?;
        Ok(Value::Vector(values))
    }

    fn text(&mut self) -> Result<Value<'a>, ParseError> {
        self.literal("\"")?;
        let mut text = String::new();
        let mut segment = self.cursor;
        loop {
            match self.peek() {
                Some(b'"') => {
                    text.push_str(&self.input[segment..self.cursor]);
                    self.cursor += 1;
                    break;
                }
                Some(b'\\') => {
                    text.push_str(&self.input[segment..self.cursor]);
                    self.cursor += 1;
                    match self.peek() {
                        Some(b'\\') => text.push('\\'),
                        Some(b'"') => text.push('"'),
                        Some(b'n') => text.push('\n'),
                        _ => return Err(ParseError::UnsupportedEncoding),
                    }
                    self.cursor += 1;
                    segment = self.cursor;
                }
                Some(_) => self.cursor += 1,
                None => return Err(ParseError::Malformed),
            }
        }
        self.whitespace();
        self.literal("[STRING]")?;
        Ok(Value::Text(text))
    }

    fn atom(&mut self) -> Result<Value<'a>, ParseError> {
        let start = self.cursor;
        let mut annotation = false;
        while let Some(byte) = self.peek() {
            match byte {
                b'[' if !annotation => annotation = true,
                b']' if annotation => annotation = false,
                b',' | b'}' | b']' if !annotation => break,
                b'{' | b'"' => return Err(ParseError::Malformed),
                _ => {}
            }
            self.cursor += 1;
        }
        let atom = self.input[start..self.cursor].trim();
        if atom.is_empty() || annotation || atom.contains("[ERROR]") {
            return Err(ParseError::Malformed);
        }
        Ok(Value::Atom(atom))
    }
}

fn as_object<'a, 'b>(value: &'a Value<'b>) -> Result<&'a Object<'b>, ParseError> {
    match value {
        Value::Object(object) => Ok(object),
        _ => Err(ParseError::Malformed),
    }
}

fn as_vector<'a, 'b>(value: &'a Value<'b>) -> Result<&'a [Value<'b>], ParseError> {
    match value {
        Value::Vector(values) => Ok(values),
        _ => Err(ParseError::Malformed),
    }
}

fn as_text<'a>(value: &'a Value<'_>) -> Result<&'a str, ParseError> {
    match value {
        Value::Text(text) => Ok(text),
        _ => Err(ParseError::UnsupportedEncoding),
    }
}

fn decimal<'a>(value: &'a Value<'_>, annotation: &str) -> Result<&'a str, ParseError> {
    let Value::Atom(atom) = value else {
        return Err(ParseError::Malformed);
    };
    let number = atom
        .strip_suffix(annotation)
        .ok_or(ParseError::Malformed)?
        .trim_end();
    let digits = number.strip_prefix('-').unwrap_or(number);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseError::Malformed);
    }
    Ok(number)
}

fn validate_id(id: &str, maximum: i64) -> Result<(), ParseError> {
    // Native user/chat/channel IDs are positive signed 64-bit values, and
    // message IDs are positive signed 32-bit values. Reject non-canonical
    // forms instead of silently changing exact identity strings.
    if id.starts_with('0') || id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseError::InvalidIdentity);
    }
    if id.parse::<i64>().map_err(|_| ParseError::InvalidIdentity)? > maximum {
        return Err(ParseError::InvalidIdentity);
    }
    Ok(())
}

fn identity(value: &Value<'_>, long: bool) -> Result<String, ParseError> {
    let id = decimal(value, if long { "[LONG]" } else { "[INT]" })?;
    validate_id(id, if long { i64::MAX } else { i64::from(i32::MAX) })?;
    Ok(id.to_owned())
}

fn integer(value: &Value<'_>) -> Result<i64, ParseError> {
    decimal(value, "[INT]")?
        .parse::<i32>()
        .map(i64::from)
        .map_err(|_| ParseError::Malformed)
}

fn user(id: String) -> TypedPeer {
    TypedPeer {
        kind: PeerKind::User,
        id,
    }
}

fn peer(value: &Value<'_>) -> Result<TypedPeer, ParseError> {
    let object = as_object(value)?;
    let (kind, field) = match object.name {
        "peerUser" => (PeerKind::User, "user_id"),
        "peerChat" => (PeerKind::Chat, "chat_id"),
        "peerChannel" => (PeerKind::Channel, "channel_id"),
        _ => return Err(ParseError::Malformed),
    };
    Ok(TypedPeer {
        kind,
        id: identity(object.field(field)?, true)?,
    })
}

fn gap(packet: &mut ParsedPacket, reason: CoverageGap) {
    if !packet.gaps.contains(&reason) {
        packet.gaps.push(reason);
    }
}

fn collect_packet(
    value: &Value<'_>,
    self_user_id: Option<&str>,
    packet: &mut ParsedPacket,
) -> Result<(), ParseError> {
    let object = as_object(value)?;
    match object.name {
        "core_message" => collect_packet(object.field("body")?, self_user_id, packet)?,
        "msg_container" => {
            for message in as_vector(object.field("messages")?)? {
                if as_object(message)?.name != "core_message" {
                    return Err(ParseError::Malformed);
                }
                collect_packet(message, self_user_id, packet)?;
            }
        }
        "rpc_result" => {
            let result = object.field("result")?;
            // Some valid API replies are vectors or scalar values, for
            // example users.getUsers -> Vector<User>. They are unsupported
            // capture paths, not malformed dump syntax.
            let Value::Object(reply) = result else {
                gap(packet, CoverageGap::UnsupportedPacket);
                return Ok(());
            };
            if matches!(
                reply.name,
                "messages_messages" | "messages_messagesSlice" | "messages_channelMessages"
            ) {
                for message in as_vector(reply.field("messages")?)? {
                    full_message(message, MessageKind::Observed, packet)?;
                }
                gap(packet, CoverageGap::IncompleteHistory);
            } else {
                collect_packet(result, self_user_id, packet)?;
            }
        }
        "updates" | "updatesCombined" => {
            for update in as_vector(object.field("updates")?)? {
                collect_update(update, self_user_id, packet)?;
            }
        }
        "updateShort" => collect_update(object.field("update")?, self_user_id, packet)?,
        "updateShortMessage" | "updateShortChatMessage" => {
            short_message(object, self_user_id, packet)?;
        }
        "updates_difference" | "updates_differenceSlice" | "updates_channelDifference" => {
            for message in as_vector(object.field("new_messages")?)? {
                full_message(message, MessageKind::New, packet)?;
            }
            for update in as_vector(object.field("other_updates")?)? {
                collect_update(update, self_user_id, packet)?;
            }
            if object
                .optional("new_encrypted_messages")
                .map(as_vector)
                .transpose()?
                .is_some_and(|messages| !messages.is_empty())
            {
                gap(packet, CoverageGap::UnsupportedMessage);
            }
        }
        "updatesTooLong" | "updates_differenceTooLong" | "updates_channelDifferenceTooLong" => {
            // An incomplete catch-up response must never silently turn into
            // a complete historical snapshot or erase absent messages.
            gap(packet, CoverageGap::IncompleteHistory);
        }
        "updateShortSentMessage" => gap(packet, CoverageGap::UnresolvedOutgoing),
        // History responses are recognized explicitly; their nested message
        // objects are not live arrivals. Account/auth and transport responses
        // are also not recursively searched for message-shaped objects.
        "messages_messages"
        | "messages_messagesSlice"
        | "messages_channelMessages"
        | "messages_messagesNotModified"
        | "updates_differenceEmpty"
        | "updates_channelDifferenceEmpty"
        | "updates_state"
        | "msgs_ack"
        | "pong"
        | "bad_msg_notification"
        | "bad_server_salt"
        | "new_session_created"
        | "msgs_state_info"
        | "msgs_all_info"
        | "msg_detailed_info"
        | "msg_new_detailed_info"
        | "rpc_error"
        | "auth_authorization"
        | "auth_sentCode"
        | "auth_loggedOut"
        | "boolTrue"
        | "boolFalse" => {}
        name if name.starts_with("update") => collect_update(value, self_user_id, packet)?,
        _ => gap(packet, CoverageGap::UnsupportedPacket),
    }
    Ok(())
}

fn collect_update(
    value: &Value<'_>,
    self_user_id: Option<&str>,
    packet: &mut ParsedPacket,
) -> Result<(), ParseError> {
    let object = as_object(value)?;
    match object.name {
        "updateNewMessage" | "updateNewChannelMessage" => {
            full_message(object.field("message")?, MessageKind::New, packet)?;
        }
        "updateEditMessage" | "updateEditChannelMessage" => {
            full_message(object.field("message")?, MessageKind::Edit, packet)?;
        }
        "updateDeleteChannelMessages" => {
            let peer = TypedPeer {
                kind: PeerKind::Channel,
                id: identity(object.field("channel_id")?, true)?,
            };
            let message_ids = as_vector(object.field("messages")?)?
                .iter()
                .map(|id| identity(id, false))
                .collect::<Result<Vec<_>, _>>()?;
            packet
                .events
                .push(ParsedEvent::Delete { peer, message_ids });
        }
        "updateDeleteMessages" => gap(packet, CoverageGap::UnresolvedDelete),
        "updateMessageID" | "updateShortSentMessage" => {
            gap(packet, CoverageGap::UnresolvedOutgoing);
        }
        "updateShortMessage" | "updateShortChatMessage" => {
            short_message(object, self_user_id, packet)?;
        }
        "updateChannelTooLong" => gap(packet, CoverageGap::IncompleteHistory),
        // These known updates do not change the text/message identity being
        // captured. Unknown updates remain visible as a coverage limitation.
        "updateUserStatus"
        | "updateUserName"
        | "updateUserPhone"
        | "updateUserTyping"
        | "updateChatUserTyping"
        | "updateChannelUserTyping"
        | "updateReadHistoryInbox"
        | "updateReadHistoryOutbox"
        | "updateReadChannelInbox"
        | "updateReadChannelOutbox"
        | "updateReadMessagesContents"
        | "updateChannelReadMessagesContents"
        | "updateMessageReactions"
        | "updateChannelMessageViews"
        | "updateChannelMessageForwards"
        | "updateNotifySettings"
        | "updatePeerSettings"
        | "updateChannel"
        | "updateChat"
        | "updatePtsChanged"
        | "updateConfig"
        | "updateDcOptions"
        | "updateDraftMessage" => {}
        _ => gap(packet, CoverageGap::UnsupportedUpdate),
    }
    Ok(())
}

fn full_message(
    value: &Value<'_>,
    kind: MessageKind,
    packet: &mut ParsedPacket,
) -> Result<(), ParseError> {
    let object = as_object(value)?;
    if object.name != "message" || object.optional("rich_message").is_some() {
        gap(packet, CoverageGap::UnsupportedMessage);
        return Ok(());
    }
    let chat_peer = peer(object.field("peer_id")?)?;
    let message_id = identity(object.field("id")?, false)?;
    let sender = object.optional("from_id").map(peer).transpose()?;
    // A catch-up "new_messages" entry can already contain edited text. Its
    // revision time must not become the original sent time just because the
    // transport envelope calls it new rather than an edit update.
    let timestamp = integer(
        object
            .optional("edit_date")
            .unwrap_or(object.field("date")?),
    )?;
    let text = as_text(object.field("message")?)?.to_owned();
    packet.events.push(ParsedEvent::Message {
        kind,
        peer: chat_peer,
        message_id,
        sender,
        timestamp,
        text,
    });
    Ok(())
}

fn short_message(
    object: &Object<'_>,
    self_user_id: Option<&str>,
    packet: &mut ParsedPacket,
) -> Result<(), ParseError> {
    let flags = integer(object.field("flags")?)?;
    let outgoing = flags & 2 != 0;
    let (peer, sender) = if object.name == "updateShortChatMessage" {
        (
            TypedPeer {
                kind: PeerKind::Chat,
                id: identity(object.field("chat_id")?, true)?,
            },
            Some(user(identity(object.field("from_id")?, true)?)),
        )
    } else {
        let peer = user(identity(object.field("user_id")?, true)?);
        let sender = if outgoing {
            let Some(id) = self_user_id else {
                gap(packet, CoverageGap::MissingSelfUser);
                return Ok(());
            };
            user(id.to_owned())
        } else {
            peer.clone()
        };
        (peer, Some(sender))
    };
    packet.events.push(ParsedEvent::Message {
        kind: MessageKind::New,
        peer,
        message_id: identity(object.field("id")?, false)?,
        sender,
        timestamp: integer(object.field("date")?)?,
        text: as_text(object.field("message")?)?.to_owned(),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // These fixtures follow the pinned dump.cpp/generate_tl.py format: mixed
    // UTF-8 strings, annotated decimals, normalized constructor names, and
    // mandatory trailing commas in generated non-empty objects and vectors.
    fn string(text: &str) -> String {
        format!(
            "\"{}\" [STRING]",
            text.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        )
    }

    fn object(name: &str, fields: &[(&str, String)]) -> String {
        if fields.is_empty() {
            return format!("{{ {name} }}");
        }
        let mut result = format!("{{ {name}\n");
        for (field, value) in fields {
            result.push_str(&format!("  {field}: {value},\n"));
        }
        result.push('}');
        result
    }

    fn vector(values: &[String]) -> String {
        if values.is_empty() {
            return "[ vector<0x0> (0) ]".to_owned();
        }
        format!(
            "[ vector<0x0> ({})\n{},\n]",
            values.len(),
            values.join(",\n")
        )
    }

    fn full(id: &str, text: &str, edited: bool) -> String {
        let mut fields = vec![
            // message's flags + flags2 are combined into one LONG by the
            // pinned generator. Other update flags remain INT.
            (
                "flags",
                format!("{} [LONG]", if edited { 256 | 32768 } else { 256 }),
            ),
            ("id", format!("{id} [INT]")),
            (
                "from_id",
                object("peerUser", &[("user_id", "9007199254740993 [LONG]".into())]),
            ),
            (
                "peer_id",
                object(
                    "peerChannel",
                    &[("channel_id", "9007199254740995 [LONG]".into())],
                ),
            ),
            ("date", "1700000000 [INT]".to_owned()),
            ("message", string(text)),
        ];
        if edited {
            fields.push(("edit_date", "1700000010 [INT]".into()));
        }
        object("message", &fields)
    }

    fn update(message: String, edited: bool) -> String {
        object(
            if edited {
                "updateEditChannelMessage"
            } else {
                "updateNewChannelMessage"
            },
            &[
                ("message", message),
                ("pts", "20 [INT]".into()),
                ("pts_count", "1 [INT]".into()),
            ],
        )
    }

    fn parse(input: &str) -> Result<ParsedPacket, ParseError> {
        parse_packet(input.as_bytes(), None, ParserLimits::default())
    }

    fn body(input: String) -> String {
        object(
            "core_message",
            &[
                ("msg_id", "7352359257580183524 [LONG]".into()),
                ("seq_no", "7 [INT]".into()),
                ("bytes", "100 [INT]".into()),
                ("body", input),
            ],
        )
    }

    #[test]
    fn preserves_full_utf8_native_ids_and_dump_string_escapes() {
        let text = format!(
            "{}\n\\ literal \\\\n\" quote\r\ttab 😀 Recv: {{ text }} [ERROR]",
            "long текст".repeat(20_000)
        );
        let packet = parse(&body(object(
            "updateShort",
            &[
                ("update", update(full("2147483647", &text, false), false)),
                ("date", "1700000000 [INT]".into()),
            ],
        )))
        .unwrap();
        assert!(packet.gaps.is_empty());
        assert_eq!(packet.events.len(), 1);
        let ParsedEvent::Message {
            peer,
            message_id,
            sender,
            timestamp,
            text: observed,
            ..
        } = &packet.events[0]
        else {
            panic!("expected message");
        };
        assert_eq!(peer.id, "9007199254740995");
        assert_eq!(message_id, "2147483647");
        assert_eq!(sender.as_ref().unwrap().id, "9007199254740993");
        assert_eq!(*timestamp, 1_700_000_000);
        assert_eq!(observed, &text);
    }

    #[test]
    fn handles_containers_gzip_new_edit_and_channel_delete() {
        let deletion = object(
            "updateDeleteChannelMessages",
            &[
                ("channel_id", "9007199254740995 [LONG]".into()),
                ("messages", vector(&["42 [INT]".into(), "43 [INT]".into()])),
                ("pts", "22 [INT]".into()),
                ("pts_count", "2 [INT]".into()),
            ],
        );
        let updates = object(
            "updates",
            &[
                (
                    "updates",
                    vector(&[
                        update(full("42", "first", false), false),
                        update(full("42", "second", true), true),
                        deletion,
                    ]),
                ),
                ("users", vector(&[])),
                ("chats", vector(&[])),
                ("date", "1700000010 [INT]".into()),
                ("seq", "1 [INT]".into()),
            ],
        );
        let input = body(object(
            "msg_container",
            &[(
                "messages",
                vector(&[body(format!("[GZIPPED] {updates}"))]).replacen(
                    "vector<0x0>",
                    "vector<0x-1>",
                    1,
                ),
            )],
        ));
        let packet = parse(&input).unwrap();
        assert!(packet.gaps.is_empty());
        assert_eq!(packet.events.len(), 3);
        assert!(
            matches!(&packet.events[1], ParsedEvent::Message {kind:MessageKind::Edit, timestamp:1_700_000_010, text,..} if text == "second")
        );
        assert!(
            matches!(&packet.events[2], ParsedEvent::Delete {message_ids,..} if message_ids == &["42", "43"])
        );
    }

    fn short(outgoing: bool, chat: bool) -> String {
        let mut fields = vec![("flags", format!("{} [INT]", if outgoing { 2 } else { 0 }))];
        if outgoing {
            fields.push(("out", "YES [ BY BIT 1 IN FIELD flags ]".into()));
        }
        fields.push(("id", "41 [INT]".into()));
        if chat {
            fields.push(("from_id", "9007199254740993 [LONG]".into()));
            fields.push(("chat_id", "9007199254740995 [LONG]".into()));
        } else {
            fields.push(("user_id", "9007199254740993 [LONG]".into()));
        }
        fields.extend([
            ("message", string("short text")),
            ("pts", "5 [INT]".into()),
            ("pts_count", "1 [INT]".into()),
            ("date", "1700000000 [INT]".into()),
        ]);
        object(
            if chat {
                "updateShortChatMessage"
            } else {
                "updateShortMessage"
            },
            &fields,
        )
    }

    #[test]
    fn normalizes_short_messages_only_with_explicit_self_for_outgoing_private() {
        let incoming = parse(&short(false, false)).unwrap();
        assert!(
            matches!(&incoming.events[0], ParsedEvent::Message{peer,sender:Some(sender),..} if peer == sender)
        );
        let no_self = parse(&short(true, false)).unwrap();
        assert!(no_self.events.is_empty());
        assert_eq!(no_self.gaps, [CoverageGap::MissingSelfUser]);
        let with_self = parse_packet(
            short(true, false).as_bytes(),
            Some("77"),
            ParserLimits::default(),
        )
        .unwrap();
        assert!(
            matches!(&with_self.events[0], ParsedEvent::Message{sender:Some(sender),..} if sender.id == "77")
        );
        let chat = parse(&short(false, true)).unwrap();
        assert!(
            matches!(&chat.events[0], ParsedEvent::Message{peer,sender:Some(sender),..} if peer.kind == PeerKind::Chat && peer.id == "9007199254740995" && sender.id == "9007199254740993")
        );
    }

    #[test]
    fn distinguishes_known_rpc_history_observations_from_live_arrivals() {
        let history = object(
            "messages_messages",
            &[
                ("messages", vector(&[full("42", "private history", false)])),
                ("chats", vector(&[])),
                ("users", vector(&[])),
                ("topics", vector(&[])),
            ],
        );
        // Only a known RPC result path emits an explicitly marked observation.
        assert!(parse(&history).unwrap().events.is_empty());
        let rpc = object(
            "rpc_result",
            &[("req_msg_id", "1 [LONG]".into()), ("result", history)],
        );
        let packet = parse(&body(rpc)).unwrap();
        assert!(
            matches!(&packet.events[0], ParsedEvent::Message {kind:MessageKind::Observed,text,..} if text == "private history")
        );
        assert_eq!(packet.gaps, [CoverageGap::IncompleteHistory]);
    }

    #[test]
    fn ignores_messages_nested_in_unknown_or_auth_responses() {
        for constructor in ["unknownReply", "updateFutureMessage", "auth_authorization"] {
            let packet = parse(&object(
                constructor,
                &[("message", full("42", "sensitive", false))],
            ))
            .unwrap();
            assert!(packet.events.is_empty());
            assert!(!format!("{packet:?}").contains("sensitive"));
            assert!(!serde_json::to_string(&packet)
                .unwrap()
                .contains("sensitive"));
        }
    }

    #[test]
    fn binary_media_and_profile_fields_do_not_break_text_history_observation() {
        // dump.cpp: invalid UTF-8 below 64 bytes is shown completely as hex;
        // at or above 64 bytes only its first 16 bytes are shown. These are
        // unrelated media/profile fields, never a valid text representation.
        let binary_short = "FF FE 00 [3 BYTES]";
        let binary_long = "FF FE 01 02 03 04 05 06 07 08 09 0A 0B 0C 0D 0E... [128 BYTES]";
        let mut message = full("42", "complete visible text 😀", false);
        message = message.replacen("flags: 256 [LONG]", "flags: 768 [LONG]", 1);
        message.pop();
        message.push_str(&format!(
            "  media: {},\n}}",
            object(
                "messageMediaPhoto",
                &[(
                    "photo",
                    object(
                        "photo",
                        &[
                            ("file_reference", binary_long.into()),
                            (
                                "sizes",
                                vector(&[object(
                                    "photoStrippedSize",
                                    &[("type", string("i")), ("bytes", binary_short.into())]
                                )])
                            )
                        ]
                    )
                )]
            )
        ));
        let user = object(
            "user",
            &[
                ("id", "9007199254740993 [LONG]".into()),
                (
                    "photo",
                    object(
                        "userProfilePhoto",
                        &[("stripped_thumb", binary_short.into())],
                    ),
                ),
            ],
        );
        let reply = object(
            "rpc_result",
            &[
                ("req_msg_id", "1 [LONG]".into()),
                (
                    "result",
                    object(
                        "messages_channelMessages",
                        &[
                            ("messages", vector(&[message])),
                            ("users", vector(&[user])),
                            ("chats", vector(&[])),
                        ],
                    ),
                ),
            ],
        );
        let packet = parse(&reply).unwrap();
        assert_eq!(packet.events.len(), 1);
        assert!(matches!(
            &packet.events[0],
            ParsedEvent::Message { kind:MessageKind::Observed, text,.. }
                if text == "complete visible text 😀"
        ));
        assert_eq!(packet.gaps, [CoverageGap::IncompleteHistory]);
        let binary_text =
            full("42", "replace_me", false).replace("\"replace_me\" [STRING]", binary_long);
        assert_eq!(
            parse(&update(binary_text, false)),
            Err(ParseError::UnsupportedEncoding)
        );
    }

    #[test]
    fn valid_rpc_vector_or_scalar_reply_is_unsupported_instead_of_malformed() {
        for reply in [
            vector(&[object("user", &[("id", "42 [LONG]".into())])]),
            "17 [INT]".into(),
        ] {
            let rpc = object(
                "rpc_result",
                &[("req_msg_id", "1 [LONG]".into()), ("result", reply)],
            );
            let packet = parse(&rpc).unwrap();
            assert!(packet.events.is_empty());
            assert_eq!(packet.gaps, [CoverageGap::UnsupportedPacket]);
        }
    }

    #[test]
    fn accepts_pinned_layer_prefix_and_refuses_unqualified_layer() {
        let packet = body(format!(
            "[LAYER229] {}",
            update(full("42", "layer text", false), false)
        ));
        let parsed = parse(&packet).unwrap();
        assert_eq!(parsed.events.len(), 1);
        assert!(parsed.gaps.is_empty());
        assert_eq!(
            parse(&packet.replace("[LAYER229]", "[LAYER228]")),
            Err(ParseError::UnsupportedEncoding)
        );
        assert_eq!(
            parse(&packet.replace("[LAYER229]", "[LAYER]")),
            Err(ParseError::UnsupportedEncoding)
        );
    }

    #[test]
    fn accepts_signed_hex_bare_vector_type_without_accepting_negative_count() {
        let deletion = object(
            "updateDeleteChannelMessages",
            &[
                ("channel_id", "9007199254740995 [LONG]".into()),
                (
                    "messages",
                    "[ vector<0x-57af6426> (2) 42 [INT], 43 [INT], ]".into(),
                ),
                ("pts", "22 [INT]".into()),
                ("pts_count", "2 [INT]".into()),
            ],
        );
        let parsed = parse(&deletion).unwrap();
        assert!(matches!(
            &parsed.events[0],
            ParsedEvent::Delete {message_ids,..} if message_ids == &["42", "43"]
        ));
        assert_eq!(
            parse("{ msgs_ack msg_ids: [ vector<0x-1> (-2) ], }"),
            Err(ParseError::Malformed)
        );
        assert_eq!(
            parse("{ msgs_ack msg_ids: [ vector<0x-> (0) ], }"),
            Err(ParseError::Malformed)
        );
    }

    #[test]
    fn unresolved_delete_and_outgoing_never_fabricate_peer_or_leak_ids() {
        let deletion = object(
            "updateDeleteMessages",
            &[
                ("messages", vector(&["1977777777 [INT]".into()])),
                ("pts", "1 [INT]".into()),
                ("pts_count", "1 [INT]".into()),
            ],
        );
        let packet = parse(&deletion).unwrap();
        assert_eq!(packet.gaps, [CoverageGap::UnresolvedDelete]);
        assert!(packet.events.is_empty());
        assert!(!serde_json::to_string(&packet)
            .unwrap()
            .contains("1977777777"));
        let packet = parse(&object(
            "updateMessageID",
            &[
                ("id", "1 [INT]".into()),
                ("random_id", "-123 [LONG]".into()),
            ],
        ))
        .unwrap();
        assert_eq!(packet.gaps, [CoverageGap::UnresolvedOutgoing]);
        assert!(packet.events.is_empty());
    }

    #[test]
    fn rejects_truncation_broken_utf8_unknown_escaping_and_duplicate_fields() {
        assert_eq!(
            parse_packet(&[0xff], None, ParserLimits::default()),
            Err(ParseError::InvalidUtf8)
        );
        let input = update(full("42", "complete", false), false);
        assert_eq!(parse(&input[..input.len() - 1]), Err(ParseError::Malformed));
        let wrong_escape = input.replace("complete", "bad\\tcontent");
        assert_eq!(parse(&wrong_escape), Err(ParseError::UnsupportedEncoding));
        assert_eq!(
            parse("{ updateShortMessage flags: 0 [INT], flags: 0 [INT], }"),
            Err(ParseError::Malformed)
        );
        assert_eq!(
            parse("{ unknownReply broken: [ERROR] (insufficient data), }"),
            Err(ParseError::Malformed)
        );
        // The error message must not echo even partially decoded text.
        assert!(!parse(&wrong_escape)
            .unwrap_err()
            .to_string()
            .contains("content"));
    }

    #[test]
    fn enforces_byte_depth_node_and_vector_limits() {
        let input = short(false, false);
        for limits in [
            ParserLimits {
                max_bytes: 10,
                ..ParserLimits::default()
            },
            ParserLimits {
                max_depth: 0,
                ..ParserLimits::default()
            },
            ParserLimits {
                max_nodes: 2,
                ..ParserLimits::default()
            },
        ] {
            assert_eq!(
                parse_packet(input.as_bytes(), None, limits),
                Err(ParseError::LimitExceeded)
            );
        }
        let vector = object(
            "msgs_ack",
            &[("msg_ids", vector(&["1 [LONG]".into(), "2 [LONG]".into()]))],
        );
        assert_eq!(
            parse_packet(
                vector.as_bytes(),
                None,
                ParserLimits {
                    max_vector_items: 1,
                    ..ParserLimits::default()
                }
            ),
            Err(ParseError::LimitExceeded)
        );
        assert_eq!(
            parse("{ msgs_ack msg_ids: [ vector<0x0> (2) 1 [LONG], ], }"),
            Err(ParseError::Malformed)
        );
        let mut nested = object("boolTrue", &[]);
        for _ in 0..150 {
            nested = object("core_message", &[("body", nested)]);
        }
        assert_eq!(
            parse_packet(
                nested.as_bytes(),
                None,
                ParserLimits {
                    max_depth: usize::MAX,
                    ..ParserLimits::default()
                }
            ),
            Err(ParseError::LimitExceeded)
        );
    }

    #[test]
    fn refuses_invalid_or_noncanonical_native_ids_without_float_rounding() {
        for id in ["0", "-1", "01", "2147483648", "9007199254740993"] {
            assert_eq!(
                parse(&update(full(id, "test", false), false)),
                Err(ParseError::InvalidIdentity)
            );
        }
        assert_eq!(
            parse_packet(
                short(true, false).as_bytes(),
                Some("9223372036854775808"),
                ParserLimits::default()
            ),
            Err(ParseError::InvalidIdentity)
        );
    }

    #[test]
    fn tracks_catchup_as_new_but_marks_incomplete_snapshot_as_gap() {
        let difference = object(
            "updates_difference",
            &[
                ("new_messages", vector(&[full("42", "catchup", false)])),
                ("new_encrypted_messages", vector(&[])),
                ("other_updates", vector(&[])),
            ],
        );
        assert_eq!(parse(&difference).unwrap().events.len(), 1);
        let edited_catchup = object(
            "updates_difference",
            &[
                (
                    "new_messages",
                    vector(&[full("42", "edited catchup", true)]),
                ),
                ("new_encrypted_messages", vector(&[])),
                ("other_updates", vector(&[])),
            ],
        );
        assert!(matches!(
            &parse(&edited_catchup).unwrap().events[0],
            ParsedEvent::Message {
                kind: MessageKind::New,
                timestamp: 1_700_000_010,
                ..
            }
        ));
        let truncated = object(
            "updates_channelDifferenceTooLong",
            &[("messages", vector(&[full("42", "partial", false)]))],
        );
        let packet = parse(&truncated).unwrap();
        assert_eq!(packet.gaps, [CoverageGap::IncompleteHistory]);
        assert!(packet.events.is_empty());
    }

    #[test]
    fn debug_output_redacts_even_valid_message_payload_and_native_ids() {
        let packet = parse(&update(full("42", "secret content", false), false)).unwrap();
        let debug = format!("{packet:?}");
        assert!(!debug.contains("secret content"));
        assert!(!debug.contains("9007199254740995"));
    }
}
