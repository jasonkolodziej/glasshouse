//! JSON in the shape Swift's `JSONEncoder` writes with `[.prettyPrinted, .sortedKeys]`: two-space indent,
//! `"key" : value`, and `/` escaped as `\/`. Keys come out sorted because the record structs list them in order.

use std::io;

use serde::{Deserialize, Serialize};
use serde_json::ser::{Formatter, PrettyFormatter};

use crate::{ManagedSshHost, SecurityKey};

#[derive(Serialize, Deserialize)]
pub(crate) struct ConfigRecord {
    pub(crate) hosts: Vec<HostRecord>,
}

/// Field order is the sorted key order.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostRecord {
    alias: String,
    /// Hosts saved before backup keys existed have no such field.
    #[serde(default)]
    backup_keys: Option<Vec<String>>,
    host_name: String,
    key_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    port: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    security_key: Option<SecurityKeyRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecurityKeyRecord {
    key_name: String,
    provider: String,
}

impl From<&ManagedSshHost> for HostRecord {
    fn from(host: &ManagedSshHost) -> Self {
        Self {
            alias: host.alias.clone(),
            backup_keys: Some(host.backup_keys.clone()),
            host_name: host.host_name.clone(),
            key_name: host.key_name.clone(),
            port: host.port,
            security_key: host.security_key.as_ref().map(|key| SecurityKeyRecord {
                key_name: key.key_name.clone(),
                provider: key.provider.clone(),
            }),
            user: host.user.clone(),
        }
    }
}

impl From<HostRecord> for ManagedSshHost {
    /// Goes through [`ManagedSshHost::new`] so values are trimmed, as Swift's `init(from:)` does. The security
    /// key is taken as stored, like Swift's synthesized decoding.
    fn from(record: HostRecord) -> Self {
        Self::new(
            &record.alias,
            &record.host_name,
            record.user.as_deref(),
            record.port,
            &record.key_name,
        )
        .with_security_key(record.security_key.map(|key| SecurityKey {
            key_name: key.key_name,
            provider: key.provider,
        }))
        .with_backup_keys(record.backup_keys.unwrap_or_default())
    }
}

/// Serializes `value` the way Swift's pretty-printing, key-sorting `JSONEncoder` does.
pub(crate) fn to_swift_json<T: Serialize>(value: &T) -> serde_json::Result<String> {
    let mut out = Vec::new();
    let mut serializer =
        serde_json::Serializer::with_formatter(&mut out, SwiftFormatter::default());
    value.serialize(&mut serializer)?;
    String::from_utf8(out)
        .map_err(|error| serde_json::Error::io(io::Error::new(io::ErrorKind::InvalidData, error)))
}

struct SwiftFormatter {
    pretty: PrettyFormatter<'static>,
}

impl Default for SwiftFormatter {
    fn default() -> Self {
        Self {
            pretty: PrettyFormatter::with_indent(b"  "),
        }
    }
}

impl Formatter for SwiftFormatter {
    fn begin_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.pretty.begin_array(writer)
    }

    fn end_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.pretty.end_array(writer)
    }

    fn begin_array_value<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.pretty.begin_array_value(writer, first)
    }

    fn end_array_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.pretty.end_array_value(writer)
    }

    fn begin_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.pretty.begin_object(writer)
    }

    fn end_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.pretty.end_object(writer)
    }

    fn begin_object_key<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.pretty.begin_object_key(writer, first)
    }

    fn begin_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b" : ")
    }

    fn end_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.pretty.end_object_value(writer)
    }

    fn write_string_fragment<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        for (index, part) in fragment.split('/').enumerate() {
            if index > 0 {
                writer.write_all(b"\\/")?;
            }
            writer.write_all(part.as_bytes())?;
        }
        Ok(())
    }
}
