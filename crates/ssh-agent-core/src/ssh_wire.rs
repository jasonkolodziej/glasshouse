//! SSH wire encoding (RFC 4251 §5): uint32 big-endian, length-prefixed strings, two's-complement mpints.

/// Decoding failures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SSHWireError {
    /// The message ended before the value being read.
    #[error("SSH wire data is truncated")]
    Truncated,
    /// A string is longer than [`SSHReader::MAX_STRING`].
    #[error("SSH wire string exceeds the {} byte limit", SSHReader::MAX_STRING)]
    TooLarge,
    /// A value is present but invalid (e.g. a name that is not UTF-8).
    #[error("malformed SSH wire data: {0}")]
    Malformed(String),
}

/// Appends SSH wire values to a byte buffer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SSHWriter {
    data: Vec<u8>,
}

impl SSHWriter {
    /// An empty writer.
    pub fn new() -> Self {
        Self::default()
    }

    /// The bytes written so far.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Consumes the writer, returning its bytes.
    pub fn into_data(self) -> Vec<u8> {
        self.data
    }

    /// `byte`.
    pub fn byte(&mut self, b: u8) {
        self.data.push(b);
    }

    /// `boolean`: 1 or 0.
    pub fn bool(&mut self, b: bool) {
        self.data.push(u8::from(b));
    }

    /// `uint32`, big-endian.
    pub fn uint32(&mut self, v: u32) {
        self.data.extend_from_slice(&v.to_be_bytes());
    }

    /// `string`: uint32 length followed by the bytes (accepts `&[u8]`, `Vec<u8>` and `&str`).
    ///
    /// Strings longer than `u32::MAX` cannot occur in agent messages (frames are capped at 256 KiB);
    /// their length is saturated rather than wrapped.
    pub fn string(&mut self, bytes: impl AsRef<[u8]>) {
        let bytes = bytes.as_ref();
        self.uint32(u32::try_from(bytes.len()).unwrap_or(u32::MAX));
        self.data.extend_from_slice(bytes);
    }

    /// Unsigned magnitude → mpint: strip leading zeros; prepend 0x00 if the top bit is set (keeps it positive).
    pub fn mpint_unsigned(&mut self, magnitude: &[u8]) {
        self.string(Self::mpint_bytes(magnitude));
    }

    /// The mpint body (without length prefix) for an unsigned big-endian magnitude.
    pub fn mpint_bytes(magnitude: &[u8]) -> Vec<u8> {
        let first_nonzero = magnitude
            .iter()
            .position(|&b| b != 0)
            .unwrap_or(magnitude.len());
        let bytes = &magnitude[first_nonzero..];
        let mut out = Vec::with_capacity(bytes.len() + 1);
        if bytes.first().is_some_and(|&b| b & 0x80 != 0) {
            out.push(0);
        }
        out.extend_from_slice(bytes);
        out
    }
}

/// Reads SSH wire values from a byte slice.
#[derive(Debug, Clone)]
pub struct SSHReader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> SSHReader<'a> {
    /// Defensive cap on any single string inside a message.
    pub const MAX_STRING: usize = 256 * 1024;

    /// A reader positioned at the start of `data`.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    /// True when every byte has been consumed.
    pub fn is_at_end(&self) -> bool {
        self.offset >= self.data.len()
    }

    /// Bytes not yet consumed.
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.offset)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], SSHWireError> {
        if self.remaining() < n {
            return Err(SSHWireError::Truncated);
        }
        let slice = &self.data[self.offset..self.offset + n];
        self.offset += n;
        Ok(slice)
    }

    /// `byte`.
    pub fn byte(&mut self) -> Result<u8, SSHWireError> {
        Ok(self.take(1)?[0])
    }

    /// `boolean`: any non-zero byte is true.
    pub fn bool(&mut self) -> Result<bool, SSHWireError> {
        Ok(self.byte()? != 0)
    }

    /// `uint32`, big-endian.
    pub fn uint32(&mut self) -> Result<u32, SSHWireError> {
        let bytes = self.take(4)?;
        Ok(bytes.iter().fold(0u32, |acc, &b| (acc << 8) | u32::from(b)))
    }

    /// `string` as raw bytes, capped at [`Self::MAX_STRING`].
    pub fn string(&mut self) -> Result<&'a [u8], SSHWireError> {
        let n = usize::try_from(self.uint32()?).map_err(|_| SSHWireError::TooLarge)?;
        if n > Self::MAX_STRING {
            return Err(SSHWireError::TooLarge);
        }
        self.take(n)
    }

    /// `string` decoded as UTF-8.
    pub fn utf8_string(&mut self) -> Result<String, SSHWireError> {
        let bytes = self.string()?;
        String::from_utf8(bytes.to_vec()).map_err(|_| SSHWireError::Malformed("utf8".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_round_trips_writer_values() {
        let mut w = SSHWriter::new();
        w.byte(7);
        w.bool(true);
        w.uint32(0xDEAD_BEEF);
        w.string("name");
        let mut r = SSHReader::new(w.data());
        assert_eq!(r.byte().unwrap(), 7);
        assert!(r.bool().unwrap());
        assert_eq!(r.uint32().unwrap(), 0xDEAD_BEEF);
        assert_eq!(r.utf8_string().unwrap(), "name");
        assert!(r.is_at_end());
        assert_eq!(r.byte(), Err(SSHWireError::Truncated));
    }

    #[test]
    fn rejects_non_utf8_names() {
        let mut w = SSHWriter::new();
        w.string([0xFF, 0xFE]);
        assert_eq!(
            SSHReader::new(w.data()).utf8_string(),
            Err(SSHWireError::Malformed("utf8".to_owned()))
        );
    }
}
