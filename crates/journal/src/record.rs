//! Wire format for journal records.
//!
//! A record is `len:u32le | crc32:u32le | kind:u8 | payload:json`. The CRC
//! covers `kind` and `payload` only, so it stays valid when the length prefix is
//! intact but the payload is short — the case a torn append actually produces.

use unfy_domain::{Rect, WindowId, WindowMode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppliedResult, JournalError, Preimage, Result, SCHEMA_VERSION};

/// The maximum accepted record size. A larger length prefix means the file is
/// not a journal or the length itself was corrupted; either way we must not
/// allocate it.
const MAX_RECORD: u32 = 1 << 20;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Record {
    Begin {
        schema_version: u32,
        transaction: Uuid,
        display_summary: String,
        created_unix_ms: u64,
    },
    Preimage {
        transaction: Uuid,
        preimage: Preimage,
    },
    Intent {
        transaction: Uuid,
        id: WindowId,
        target: Rect,
        mode: WindowMode,
    },
    Result {
        transaction: Uuid,
        result: AppliedResult,
    },
    /// A window reached a terminal state and owes nothing further.
    ///
    /// Without this record a later run could not tell "this window was already
    /// put back" from "this window was never handled", so it would restore it
    /// again - overwriting a position the user had chosen in the meantime.
    Resolved {
        transaction: Uuid,
        id: WindowId,
        resolution: unfy_domain::WindowOutcome,
    },
    Closed {
        transaction: Uuid,
        closed_unix_ms: u64,
    },
}

impl Record {
    pub fn kind_byte(&self) -> u8 {
        match self {
            Record::Begin { .. } => 1,
            Record::Preimage { .. } => 2,
            Record::Intent { .. } => 3,
            Record::Result { .. } => 4,
            Record::Resolved { .. } => 6,
            Record::Closed { .. } => 5,
        }
    }

    pub fn transaction(&self) -> Uuid {
        match self {
            Record::Begin { transaction, .. }
            | Record::Preimage { transaction, .. }
            | Record::Intent { transaction, .. }
            | Record::Result { transaction, .. }
            | Record::Resolved { transaction, .. }
            | Record::Closed { transaction, .. } => *transaction,
        }
    }

    /// Serialise to the wire format.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let payload = serde_json::to_vec(self)
            .map_err(|e| JournalError::Io(format!("serialise record: {e}")))?;
        if payload.len() as u64 + 1 > MAX_RECORD as u64 {
            return Err(JournalError::Io(format!(
                "record of {} bytes exceeds the {} byte limit",
                payload.len() + 1,
                MAX_RECORD
            )));
        }
        let len = (payload.len() + 1) as u32;
        let mut buf = Vec::with_capacity(8 + len as usize);
        buf.extend_from_slice(&len.to_le_bytes());

        let mut crc_input = Vec::with_capacity(1 + payload.len());
        crc_input.push(self.kind_byte());
        crc_input.extend_from_slice(&payload);
        buf.extend_from_slice(&crc32fast::hash(&crc_input).to_le_bytes());
        buf.extend_from_slice(&crc_input);
        Ok(buf)
    }

    /// Decode one record from the front of `bytes`.
    ///
    /// * `Ok(Some((record, consumed)))` — a complete, CRC-valid record.
    /// * `Ok(None)` — the buffer ends inside a record (torn tail).
    /// * `Err(_)` — the length prefix or CRC is invalid.
    pub fn decode(bytes: &[u8]) -> std::result::Result<Option<(Record, usize)>, String> {
        if bytes.len() < 8 {
            return Ok(None);
        }
        let len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if len == 0 || len > MAX_RECORD {
            return Err(format!("implausible record length {len}"));
        }
        let total = 8 + len as usize;
        if bytes.len() < total {
            return Ok(None);
        }
        let expected_crc = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let body = &bytes[8..total];
        if crc32fast::hash(body) != expected_crc {
            return Err("crc mismatch".to_string());
        }
        let record: Record =
            serde_json::from_slice(&body[1..]).map_err(|e| format!("payload: {e}"))?;
        if record.kind_byte() != body[0] {
            return Err("kind byte does not match payload".to_string());
        }
        if let Record::Begin { schema_version, .. } = &record {
            if !(1..=SCHEMA_VERSION).contains(schema_version) {
                return Err(format!(
                    "journal schema {schema_version} is not supported (max {SCHEMA_VERSION})"
                ));
            }
        }
        Ok(Some((record, total)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unfy_domain::IdentityQuality;

    fn sample_preimage() -> Preimage {
        Preimage {
            id: WindowId {
                native_id: 0x1234,
                process_id: 42,
                process_start: 7,
            },
            display_native_id: "display-a".into(),
            display_quality: IdentityQuality::Strong,
            display_work_area: Rect::new(0, 0, 2560, 1400),
            display_scale: unfy_domain::Scale { x: 1.5, y: 1.5 },
            visible_rect: Rect::new(10, 20, 300, 400),
            outer_rect: Rect::new(3, 13, 307, 407),
            mode: WindowMode::Normal,
            topmost: false,
            native_placement: Some(vec![1, 2, 3, 4]),
        }
    }

    #[test]
    fn round_trip_every_record_kind() {
        let tx = Uuid::new_v4();
        let records = vec![
            Record::Begin {
                schema_version: SCHEMA_VERSION,
                transaction: tx,
                display_summary: "2 displays".into(),
                created_unix_ms: 1,
            },
            Record::Preimage {
                transaction: tx,
                preimage: sample_preimage(),
            },
            Record::Intent {
                transaction: tx,
                id: sample_preimage().id,
                target: Rect::new(0, 0, 100, 100),
                mode: WindowMode::Normal,
            },
            Record::Result {
                transaction: tx,
                result: AppliedResult {
                    id: sample_preimage().id,
                    outcome: unfy_domain::WindowOutcome::Applied,
                    observed: None,
                },
            },
            Record::Resolved {
                transaction: tx,
                id: sample_preimage().id,
                resolution: unfy_domain::WindowOutcome::Restored,
            },
            Record::Closed {
                transaction: tx,
                closed_unix_ms: 2,
            },
        ];

        let mut buf = Vec::new();
        for r in &records {
            buf.extend_from_slice(&r.encode().unwrap());
        }

        let mut offset = 0;
        let mut decoded = Vec::new();
        while offset < buf.len() {
            let (r, used) = Record::decode(&buf[offset..]).unwrap().unwrap();
            decoded.push(r);
            offset += used;
        }
        assert_eq!(decoded, records);
    }

    #[test]
    fn torn_tail_returns_none_not_error() {
        let tx = Uuid::new_v4();
        let encoded = Record::Begin {
            schema_version: SCHEMA_VERSION,
            transaction: tx,
            display_summary: "x".into(),
            created_unix_ms: 1,
        }
        .encode()
        .unwrap();

        for cut in 1..encoded.len() {
            let partial = &encoded[..cut];
            assert!(matches!(Record::decode(partial), Ok(None)), "cut at {cut}");
        }
    }

    #[test]
    fn bit_flip_is_detected() {
        let tx = Uuid::new_v4();
        let mut encoded = Record::Closed {
            transaction: tx,
            closed_unix_ms: 5,
        }
        .encode()
        .unwrap();
        let last = encoded.len() - 1;
        encoded[last] ^= 0xFF;
        assert!(Record::decode(&encoded).is_err());
    }

    #[test]
    fn absurd_length_is_rejected_without_allocating() {
        let mut bytes = u32::MAX.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        assert!(Record::decode(&bytes).is_err());
    }

    #[test]
    fn newer_schema_is_rejected() {
        let tx = Uuid::new_v4();
        let encoded = Record::Begin {
            schema_version: SCHEMA_VERSION + 1,
            transaction: tx,
            display_summary: "x".into(),
            created_unix_ms: 1,
        }
        .encode()
        .unwrap();
        assert!(Record::decode(&encoded).is_err());
    }
}
