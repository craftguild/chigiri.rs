use super::WebSocketWriteProgressError;

const BASE_HEADER_BYTES: usize = 2;
const LONG_HEADER_BYTES: usize = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FrameKind {
    Text,
    Close,
    Control,
}

#[derive(Debug)]
enum State {
    Header {
        bytes: [u8; LONG_HEADER_BYTES],
        used: usize,
        needed: usize,
    },
    Payload {
        kind: FrameKind,
        remaining: u64,
    },
}

#[derive(Debug)]
pub(super) struct FrameParser {
    state: State,
}

impl FrameParser {
    pub(super) const fn new() -> Self {
        Self {
            state: State::Header {
                bytes: [0; LONG_HEADER_BYTES],
                used: 0,
                needed: BASE_HEADER_BYTES,
            },
        }
    }

    pub(super) const fn at_header(&self) -> bool {
        matches!(self.state, State::Header { .. })
    }

    pub(super) const fn at_frame_boundary(&self) -> bool {
        matches!(self.state, State::Header { used: 0, .. })
    }

    pub(super) fn partial_text_header(&self) -> bool {
        matches!(
            &self.state,
            State::Header { bytes, used, .. } if *used > 0 && bytes[0] & 0x0f == 0x1
        )
    }

    pub(super) fn accept_header(&mut self, bytes: &[u8]) -> usize {
        let State::Header {
            bytes: header,
            used,
            needed,
        } = &mut self.state
        else {
            return 0;
        };
        let take = (*needed - *used).min(bytes.len());
        header[*used..*used + take].copy_from_slice(&bytes[..take]);
        *used += take;
        if *used == BASE_HEADER_BYTES {
            *needed = match header[1] & 0x7f {
                126 => 4,
                127 => LONG_HEADER_BYTES,
                _ => BASE_HEADER_BYTES,
            };
        }
        take
    }

    pub(super) fn decode_header(
        &self,
    ) -> Result<Option<(FrameKind, u64)>, WebSocketWriteProgressError> {
        let State::Header {
            bytes,
            used,
            needed,
        } = &self.state
        else {
            return Ok(None);
        };
        if used != needed {
            return Ok(None);
        }
        if bytes[0] & 0xf0 != 0x80 || bytes[1] & 0x80 != 0 {
            return Err(WebSocketWriteProgressError::MalformedFrame);
        }

        let kind = match bytes[0] & 0x0f {
            0x1 => FrameKind::Text,
            0x8 => FrameKind::Close,
            0x9 | 0xa => FrameKind::Control,
            _ => return Err(WebSocketWriteProgressError::MalformedFrame),
        };
        let marker = bytes[1] & 0x7f;
        if kind != FrameKind::Text && marker > 125 {
            return Err(WebSocketWriteProgressError::MalformedFrame);
        }
        let payload_len = match marker {
            0..=125 => u64::from(marker),
            126 => {
                let length = u64::from(u16::from_be_bytes([bytes[2], bytes[3]]));
                if length < 126 {
                    return Err(WebSocketWriteProgressError::MalformedFrame);
                }
                length
            }
            127 => {
                if bytes[2] & 0x80 != 0 {
                    return Err(WebSocketWriteProgressError::LengthOverflow);
                }
                let length = u64::from_be_bytes(
                    bytes[2..10]
                        .try_into()
                        .expect("a complete long WebSocket header has eight length bytes"),
                );
                if length < 65_536 {
                    return Err(WebSocketWriteProgressError::MalformedFrame);
                }
                length
            }
            _ => unreachable!("the seven-bit marker is exhaustive"),
        };
        if kind == FrameKind::Close && payload_len == 1 {
            return Err(WebSocketWriteProgressError::MalformedFrame);
        }
        Ok(Some((kind, payload_len)))
    }

    pub(super) fn begin_payload(&mut self, kind: FrameKind, payload_len: u64) {
        self.state = State::Payload {
            kind,
            remaining: payload_len,
        };
    }

    pub(super) fn accept_payload(&mut self, bytes: &[u8]) -> (FrameKind, usize, bool) {
        let State::Payload { kind, remaining } = &mut self.state else {
            unreachable!("payload acceptance requires a decoded frame header");
        };
        let accepted = if *remaining > bytes.len() as u64 {
            bytes.len()
        } else {
            *remaining as usize
        };
        *remaining -= accepted as u64;
        (*kind, accepted, *remaining == 0)
    }

    pub(super) fn finish_payload(&mut self) {
        self.state = State::Header {
            bytes: [0; LONG_HEADER_BYTES],
            used: 0,
            needed: BASE_HEADER_BYTES,
        };
    }
}
