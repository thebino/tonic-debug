//! Human-readable protobuf wire format inspection.
//!
//! Since we don't have access to `.proto` schemas at runtime, this module
//! decodes raw protobuf bytes using the wire format specification. Each field
//! is displayed with its field number, wire type, and value — giving developers
//! meaningful insight into what is being sent over the wire.

use bytes::Buf;
use std::fmt;

/// Wire types as defined by the protobuf encoding specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireType {
    /// Varint (int32, int64, uint32, uint64, sint32, sint64, bool, enum)
    Varint,
    /// 64-bit (fixed64, sfixed64, double)
    Bit64,
    /// Length-delimited (string, bytes, embedded messages, packed repeated fields)
    LengthDelimited,
    /// Start group (deprecated)
    StartGroup,
    /// End group (deprecated)
    EndGroup,
    /// 32-bit (fixed32, sfixed32, float)
    Bit32,
    /// Unknown wire type
    Unknown(u32),
}

impl fmt::Display for WireType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireType::Varint => write!(f, "varint"),
            WireType::Bit64 => write!(f, "64-bit"),
            WireType::LengthDelimited => write!(f, "length-delimited"),
            WireType::StartGroup => write!(f, "start-group"),
            WireType::EndGroup => write!(f, "end-group"),
            WireType::Bit32 => write!(f, "32-bit"),
            WireType::Unknown(v) => write!(f, "unknown({})", v),
        }
    }
}

impl From<u32> for WireType {
    fn from(value: u32) -> Self {
        match value {
            0 => WireType::Varint,
            1 => WireType::Bit64,
            2 => WireType::LengthDelimited,
            3 => WireType::StartGroup,
            4 => WireType::EndGroup,
            5 => WireType::Bit32,
            v => WireType::Unknown(v),
        }
    }
}

/// A single decoded protobuf field.
#[derive(Debug, Clone, PartialEq)]
pub struct ProtoField {
    /// The field number from the protobuf tag.
    pub field_number: u32,
    /// The wire type of this field.
    pub wire_type: WireType,
    /// The decoded value.
    pub value: ProtoValue,
}

/// Represents a decoded protobuf value.
#[derive(Debug, Clone, PartialEq)]
pub enum ProtoValue {
    /// A varint-encoded integer.
    Varint(u64),
    /// A 32-bit fixed value.
    Fixed32(u32),
    /// A 64-bit fixed value.
    Fixed64(u64),
    /// Length-delimited bytes — may be a string, nested message, or raw bytes.
    Bytes(Vec<u8>),
    /// Nested message fields (when bytes successfully decode as protobuf).
    Nested(Vec<ProtoField>),
}

impl fmt::Display for ProtoValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtoValue::Varint(v) => write!(f, "{}", v),
            ProtoValue::Fixed32(v) => write!(f, "0x{:08x}", v),
            ProtoValue::Fixed64(v) => write!(f, "0x{:016x}", v),
            ProtoValue::Bytes(b) => {
                // Try to display as UTF-8 string first
                if let Ok(s) = std::str::from_utf8(b) {
                    if s.chars()
                        .all(|c| !c.is_control() || c == '\n' || c == '\r' || c == '\t')
                    {
                        return write!(f, "\"{}\"", s);
                    }
                }
                // Fall back to hex representation
                write!(f, "0x")?;
                for byte in b {
                    write!(f, "{:02x}", byte)?;
                }
                Ok(())
            }
            ProtoValue::Nested(fields) => {
                write!(f, "{{ ")?;
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", field)?;
                }
                write!(f, " }}")
            }
        }
    }
}

impl fmt::Display for ProtoField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "field {} ({}): {}",
            self.field_number, self.wire_type, self.value
        )
    }
}

/// Decode a varint from the buffer, returning the value and the number of bytes consumed.
fn decode_varint(buf: &[u8]) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    let mut shift = 0;
    for (i, &byte) in buf.iter().enumerate() {
        if shift >= 64 {
            return None;
        }
        value |= ((byte & 0x7F) as u64) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

/// Attempt to decode raw bytes as protobuf wire format fields.
///
/// Returns `None` if the bytes do not look like valid protobuf.
pub fn decode_protobuf(data: &[u8]) -> Option<Vec<ProtoField>> {
    let mut fields = Vec::new();
    let mut pos = 0;

    while pos < data.len() {
        // Decode the tag (field_number << 3 | wire_type)
        let (tag, tag_len) = decode_varint(&data[pos..])?;
        pos += tag_len;

        let wire_type_raw = (tag & 0x07) as u32;
        let field_number = (tag >> 3) as u32;

        // Field number 0 is invalid
        if field_number == 0 {
            return None;
        }

        let wire_type = WireType::from(wire_type_raw);

        let value = match wire_type {
            WireType::Varint => {
                let (v, v_len) = decode_varint(&data[pos..])?;
                pos += v_len;
                ProtoValue::Varint(v)
            }
            WireType::Bit64 => {
                if pos + 8 > data.len() {
                    return None;
                }
                let mut buf = &data[pos..pos + 8];
                let v = buf.get_u64_le();
                pos += 8;
                ProtoValue::Fixed64(v)
            }
            WireType::LengthDelimited => {
                let (len, len_bytes) = decode_varint(&data[pos..])?;
                pos += len_bytes;
                let len = len as usize;
                if pos + len > data.len() {
                    return None;
                }
                let payload = &data[pos..pos + len];
                pos += len;

                // Try to recursively decode as a nested protobuf message
                if let Some(nested) = decode_protobuf(payload) {
                    if !nested.is_empty() {
                        ProtoValue::Nested(nested)
                    } else {
                        ProtoValue::Bytes(payload.to_vec())
                    }
                } else {
                    ProtoValue::Bytes(payload.to_vec())
                }
            }
            WireType::Bit32 => {
                if pos + 4 > data.len() {
                    return None;
                }
                let mut buf = &data[pos..pos + 4];
                let v = buf.get_u32_le();
                pos += 4;
                ProtoValue::Fixed32(v)
            }
            WireType::StartGroup | WireType::EndGroup => {
                // Deprecated wire types — skip
                return None;
            }
            WireType::Unknown(_) => {
                return None;
            }
        };

        fields.push(ProtoField {
            field_number,
            wire_type,
            value,
        });
    }

    Some(fields)
}

/// A decoded gRPC frame.
#[derive(Debug)]
pub struct GrpcFrame {
    /// Whether the frame is compressed.
    pub compressed: bool,
    /// The raw payload bytes.
    pub payload: Vec<u8>,
    /// Decoded protobuf fields (if decoding succeeded).
    pub decoded_fields: Option<Vec<ProtoField>>,
}

impl fmt::Display for GrpcFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "gRPC frame (compressed={}, {} bytes)",
            self.compressed,
            self.payload.len()
        )?;
        if let Some(ref fields) = self.decoded_fields {
            for field in fields {
                write!(f, "\n  {}", field)?;
            }
        } else {
            write!(f, "\n  <raw bytes: {} bytes>", self.payload.len())?;
        }
        Ok(())
    }
}

/// Parse gRPC-encoded data into frames.
///
/// gRPC uses a length-prefixed framing format:
/// - 1 byte: compressed flag (0 = uncompressed, 1 = compressed)
/// - 4 bytes: message length (big-endian)
/// - N bytes: message payload
pub fn parse_grpc_frames(data: &[u8]) -> Vec<GrpcFrame> {
    let mut frames = Vec::new();
    let mut pos = 0;

    while pos + 5 <= data.len() {
        let compressed = data[pos] != 0;
        pos += 1;

        let length =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;

        if pos + length > data.len() {
            break;
        }

        let payload = data[pos..pos + length].to_vec();
        pos += length;

        let decoded_fields = if !compressed {
            decode_protobuf(&payload)
        } else {
            None
        };

        frames.push(GrpcFrame {
            compressed,
            payload,
            decoded_fields,
        });
    }

    frames
}

/// Format a byte slice as a hex dump suitable for debug logging.
pub fn hex_dump(data: &[u8], max_bytes: usize) -> String {
    let truncated = data.len() > max_bytes;
    let display = &data[..data.len().min(max_bytes)];

    let mut result = String::new();
    for (i, chunk) in display.chunks(16).enumerate() {
        if i > 0 {
            result.push('\n');
        }
        result.push_str(&format!("  {:04x}: ", i * 16));
        for (j, byte) in chunk.iter().enumerate() {
            if j == 8 {
                result.push(' ');
            }
            result.push_str(&format!("{:02x} ", byte));
        }
        // Pad remaining space
        let remaining = 16 - chunk.len();
        for j in 0..remaining {
            if chunk.len() + j == 8 {
                result.push(' ');
            }
            result.push_str("   ");
        }
        result.push_str(" |");
        for byte in chunk {
            if byte.is_ascii_graphic() || *byte == b' ' {
                result.push(*byte as char);
            } else {
                result.push('.');
            }
        }
        result.push('|');
    }

    if truncated {
        result.push_str(&format!(
            "\n  ... ({} bytes truncated)",
            data.len() - max_bytes
        ));
    }

    result
}

/// Format gRPC request/response data into a human-readable string.
pub fn format_grpc_message(data: &[u8]) -> String {
    let frames = parse_grpc_frames(data);
    if frames.is_empty() {
        return format!("  <no valid gRPC frames, {} raw bytes>", data.len());
    }

    let mut result = String::new();
    for (i, frame) in frames.iter().enumerate() {
        if i > 0 {
            result.push('\n');
        }
        result.push_str(&format!("{}", frame));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_varint() {
        // Single byte varint: 1
        assert_eq!(decode_varint(&[0x01]), Some((1, 1)));
        // Single byte varint: 127
        assert_eq!(decode_varint(&[0x7F]), Some((127, 1)));
        // Two byte varint: 128
        assert_eq!(decode_varint(&[0x80, 0x01]), Some((128, 2)));
        // Two byte varint: 300
        assert_eq!(decode_varint(&[0xAC, 0x02]), Some((300, 2)));
    }

    #[test]
    fn test_decode_simple_protobuf() {
        // Field 1, varint, value 150 (encoded as: 08 96 01)
        let data = vec![0x08, 0x96, 0x01];
        let fields = decode_protobuf(&data).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].field_number, 1);
        assert_eq!(fields[0].wire_type, WireType::Varint);
        match &fields[0].value {
            ProtoValue::Varint(v) => assert_eq!(*v, 150),
            _ => panic!("Expected varint"),
        }
    }

    #[test]
    fn test_decode_string_field() {
        // Field 2, length-delimited, value "testing" (encoded as: 12 07 74 65 73 74 69 6e 67)
        let data = vec![0x12, 0x07, 0x74, 0x65, 0x73, 0x74, 0x69, 0x6e, 0x67];
        let fields = decode_protobuf(&data).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].field_number, 2);
        assert_eq!(fields[0].wire_type, WireType::LengthDelimited);
        match &fields[0].value {
            ProtoValue::Bytes(b) => assert_eq!(b, b"testing"),
            _ => panic!("Expected bytes"),
        }
    }

    #[test]
    fn test_parse_grpc_frame() {
        // gRPC frame: uncompressed, 3-byte payload (field 1, varint 150)
        let mut data = vec![0x00]; // compressed flag = false
        data.extend_from_slice(&3u32.to_be_bytes()); // length = 3
        data.extend_from_slice(&[0x08, 0x96, 0x01]); // protobuf payload

        let frames = parse_grpc_frames(&data);
        assert_eq!(frames.len(), 1);
        assert!(!frames[0].compressed);
        assert!(frames[0].decoded_fields.is_some());
        let fields = frames[0].decoded_fields.as_ref().unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].field_number, 1);
    }

    #[test]
    fn test_invalid_protobuf() {
        // Random bytes that don't form valid protobuf
        let data = vec![
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        ];
        assert!(decode_protobuf(&data).is_none());
    }

    #[test]
    fn test_hex_dump() {
        let data = b"Hello, World!";
        let dump = hex_dump(data, 256);
        assert!(dump.contains("48 65 6c 6c"));
        assert!(dump.contains("|Hello, World!|"));
    }

    #[test]
    fn test_empty_data() {
        let fields = decode_protobuf(&[]);
        assert_eq!(fields, Some(vec![]));
    }

    #[test]
    fn test_wire_type_display() {
        assert_eq!(format!("{}", WireType::Varint), "varint");
        assert_eq!(format!("{}", WireType::LengthDelimited), "length-delimited");
        assert_eq!(format!("{}", WireType::Unknown(99)), "unknown(99)");
    }
}
