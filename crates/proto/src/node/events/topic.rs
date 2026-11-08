//! Dotted topic names, with literal components and an optional trailing wildcard.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};
use thiserror::Error;

pub const MAX_TOPIC_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Topic {
    name: String,
    key: Option<String>,
    resources: Vec<String>,
    wildcard: bool,
    invalid: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid dotted topic")]
pub struct TopicError;

impl Topic {
    /// Every resource in a namespace. `*` selects every namespace too.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            key: None,
            resources: Vec::new(),
            wildcard: true,
            invalid: None,
        }
    }

    /// One opaque resource key: separators and wildcard characters stay literal.
    pub fn keyed(name: impl Into<String>, key: impl Into<String>) -> Self {
        let key = key.into();
        Self {
            name: name.into(),
            key: Some(key.clone()),
            resources: vec![key],
            wildcard: false,
            invalid: None,
        }
    }

    /// One hierarchical resource, with each component escaped independently.
    pub fn scoped<I, S>(name: impl Into<String>, resources: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let resources: Vec<_> = resources.into_iter().map(Into::into).collect();
        let key = (!resources.is_empty()).then(|| resources.join("."));
        Self {
            name: name.into(),
            key,
            wildcard: resources.is_empty(),
            resources,
            invalid: None,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn namespace(&self) -> &str {
        self.name()
    }

    /// Decoded resource components joined by dots. Use `resource_segments` for identity.
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// Decoded literal components, excluding the trailing wildcard.
    pub fn resource_segments(&self) -> &[String] {
        &self.resources
    }

    pub fn is_pattern(&self) -> bool {
        self.wildcard
    }

    pub fn parse(text: &str) -> Result<Self, TopicError> {
        if text.is_empty() || text.len() > MAX_TOPIC_BYTES {
            return Err(TopicError);
        }
        if text == "*" {
            return Ok(Self::new("*"));
        }
        let mut components = text.split('.');
        let name = decode(components.next().ok_or(TopicError)?)?;
        if name == "*" {
            return Err(TopicError);
        }
        let remaining: Vec<_> = components.collect();
        let wildcard = remaining.is_empty() || remaining.last() == Some(&"*");
        let literal_count = remaining.len() - usize::from(remaining.last() == Some(&"*"));
        let resources = remaining[..literal_count]
            .iter()
            .map(|component| decode(component))
            .collect::<Result<Vec<_>, _>>()?;
        let key = if resources.is_empty() {
            None
        } else {
            let mut key = resources.join(".");
            if wildcard {
                key.push_str(".*");
            }
            Some(key)
        };
        let topic = Self {
            name,
            key,
            resources,
            wildcard,
            invalid: None,
        };
        topic.valid().then_some(topic).ok_or(TopicError)
    }

    /// Whether this subscription contains the whole other subscription.
    pub fn covers(&self, other: &Self) -> bool {
        self.valid()
            && other.valid()
            && (self.name == "*"
                || (self.name == other.name
                    && if self.wildcard {
                        other.resources.starts_with(&self.resources)
                    } else {
                        !other.wildcard && self.resources == other.resources
                    }))
    }

    /// A broad notification invalidates narrower subscriptions in that scope too.
    pub fn matches(&self, notification: &Self) -> bool {
        self.covers(notification) || notification.covers(self)
    }

    pub fn valid(&self) -> bool {
        self.invalid.is_none()
            && !self.name.is_empty()
            && self.name.len() <= 128
            && (self.name == "*"
                || self
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-/".contains(&byte)))
            && (self.name != "*" || (self.resources.is_empty() && self.wildcard))
            && self
                .resources
                .iter()
                .all(|part| !part.is_empty() && part.len() <= 4096 && !part.contains('\0'))
            && self
                .resources
                .iter()
                .try_fold(self.resources.len().saturating_sub(1), |size, part| {
                    size.checked_add(part.len())
                })
                .is_some_and(|size| size <= 4096)
    }
}

impl FromStr for Topic {
    type Err = TopicError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl fmt::Display for Topic {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(invalid) = &self.invalid {
            return out.write_str(invalid);
        }
        if self.name == "*" && self.resources.is_empty() && self.wildcard {
            return out.write_str("*");
        }
        encode(&self.name, out)?;
        for resource in &self.resources {
            out.write_str(".")?;
            encode(resource, out)?;
        }
        if self.wildcard {
            out.write_str(".*")?;
        }
        Ok(())
    }
}

impl Serialize for Topic {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Topic {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        // Keep invalid topic strings so Watch can reject their request ID without
        // dropping the connection or replacing an already valid subscription.
        Ok(Self::parse(&text).unwrap_or_else(|_| Self {
            name: String::new(),
            key: None,
            resources: Vec::new(),
            wildcard: false,
            invalid: Some(text),
        }))
    }
}

fn encode(text: &str, out: &mut fmt::Formatter<'_>) -> fmt::Result {
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"_-~".contains(&byte) {
            write!(out, "{}", char::from(byte))?;
        } else {
            write!(out, "%{byte:02X}")?;
        }
    }
    Ok(())
}

fn decode(text: &str) -> Result<String, TopicError> {
    if text.is_empty() || text == "*" {
        return Err(TopicError);
    }
    let mut bytes = Vec::with_capacity(text.len());
    let mut input = text.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let high = hex(input.next().ok_or(TopicError)?)?;
            let low = hex(input.next().ok_or(TopicError)?)?;
            bytes.push(high * 16 + low);
        } else if byte.is_ascii_alphanumeric() || b"_-~".contains(&byte) {
            bytes.push(byte);
        } else {
            return Err(TopicError);
        }
    }
    String::from_utf8(bytes).map_err(|_| TopicError)
}

fn hex(byte: u8) -> Result<u8, TopicError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(TopicError),
    }
}

#[cfg(test)]
#[path = "../../tests/node_events_topic.rs"]
mod tests;
