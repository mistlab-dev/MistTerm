// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2017-2020 Alexey Arbuzov
// Copyright (c) 2023-2026 Jarkko Sakkinen

//! ZMODEM transmission state and logic.

use crate::buffer::Buffer;
use crate::crc;
use crate::error::Error;
use crate::header::{
    write_slice_escaped, Encoding, Frame, Header, Zrinit, HEADER_PAYLOAD_SIZE, HEADER_SIZE,
    ZACK_HEADER, ZDATA_HEADER, ZEOF_HEADER, ZFIN_HEADER, ZNAK_HEADER, ZRPOS_HEADER, ZRQINIT_HEADER,
};
use crate::io::{Read, Write};
use crate::string::String;
use crate::zdle;
use crate::{ZDLE, ZPAD};
use core::cmp::min;
use core::fmt::Write as _;
#[cfg(feature = "std")]
use std::time::{Duration, Instant};

/// Size of the unescaped subpacket payload. The size is picked from the
/// original ZMODEM specification.
const SUBPACKET_MAX_SIZE: usize = 1024;
const SUBPACKET_PER_ACK: usize = 10;
const MAX_HEADER_ESCAPED: usize = 128;
const MAX_SUBPACKET_ESCAPED: usize = SUBPACKET_MAX_SIZE * 2 + 2 + 8;
const WIRE_BUF_SIZE: usize = MAX_HEADER_ESCAPED + MAX_SUBPACKET_ESCAPED;
const RECEIVER_EVENT_QUEUE_CAP: usize = 4;
/// `WaitFilePos` 下 lrzsz 可能先发 **ZRQINIT** 再发 **ZRINIT**（或多次超时重发），各需一次 recover。
/// 配额过小会在第二次邀请时已用尽，表现为只写出首轮 ZFILE、随后 `incoming_len=0` 空转。
const MAX_WAIT_FILE_POS_RECOVER_ATTEMPTS: u8 = 4;
#[cfg(feature = "std")]
const WAIT_FILE_POS_RECOVER_MIN_INTERVAL: Duration = Duration::from_millis(1200);

#[cfg(feature = "std")]
fn mistterm_sender_reply_zrinit_enabled() -> bool {
    std::env::var("MISTTERM_ZMODEM_SENDER_REPLY_ZRINIT")
        .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("no")))
        .unwrap_or(false)
}

#[cfg(not(feature = "std"))]
const fn mistterm_sender_reply_zrinit_enabled() -> bool {
    false
}

#[cfg(feature = "std")]
fn mistterm_zfile_bin32_enabled() -> bool {
    std::env::var("MISTTERM_ZMODEM_ZFILE_BIN32")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[cfg(not(feature = "std"))]
const fn mistterm_zfile_bin32_enabled() -> bool {
    false
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ZfileWireMode {
    Bin16DoubleZpad,
    Bin16SingleZpad,
    Bin32DoubleZpad,
    Bin32SingleZpad,
}

impl ZfileWireMode {
    const fn encoding(self) -> Encoding {
        match self {
            Self::Bin16DoubleZpad | Self::Bin16SingleZpad => Encoding::ZBIN,
            Self::Bin32DoubleZpad | Self::Bin32SingleZpad => Encoding::ZBIN32,
        }
    }

    const fn extra_zpad(self) -> bool {
        match self {
            Self::Bin16DoubleZpad | Self::Bin32DoubleZpad => true,
            Self::Bin16SingleZpad | Self::Bin32SingleZpad => false,
        }
    }

    const fn next(self) -> Self {
        match self {
            Self::Bin16DoubleZpad => Self::Bin16SingleZpad,
            Self::Bin16SingleZpad => Self::Bin32DoubleZpad,
            Self::Bin32DoubleZpad => Self::Bin32SingleZpad,
            Self::Bin32SingleZpad => Self::Bin16DoubleZpad,
        }
    }
}

fn zfile_wire_mode_default() -> ZfileWireMode {
    if mistterm_zfile_bin32_enabled() {
        ZfileWireMode::Bin32DoubleZpad
    } else {
        ZfileWireMode::Bin16DoubleZpad
    }
}

/// The ZMODEM protocol subpacket type
#[repr(u8)]
#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SubpacketType {
    ZCRCE = 0x68,
    ZCRCG = 0x69,
    ZCRCQ = 0x6a,
    ZCRCW = 0x6b,
}

impl TryFrom<u8> for SubpacketType {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x68 => Ok(SubpacketType::ZCRCE),
            0x69 => Ok(SubpacketType::ZCRCG),
            0x6a => Ok(SubpacketType::ZCRCQ),
            0x6b => Ok(SubpacketType::ZCRCW),
            _ => Err(Error::MalformedPacket(value)),
        }
    }
}

/// A request for file data from the sender.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FileRequest {
    pub offset: u32,
    pub len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SenderEvent {
    FileComplete,
    SessionComplete,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ReceiverEvent {
    FileStart,
    FileComplete,
    SessionComplete,
}

/// Internal state for reading a subpacket byte-by-byte
#[derive(Clone, Copy, Debug, PartialEq)]
enum SubpacketState {
    Idle,
    Reading,
    Writing(SubpacketType),
    Crc(SubpacketType),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ZpadState {
    Idle,
    Zpad,
    ZpadZpad,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum HeaderReadState {
    SeekingZpad,
    ReadingEncoding,
    ReadingData,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SendState {
    WaitReceiverInit,
    ReadyForFile,
    WaitFilePos,
    NeedFileData,
    WaitFileAck,
    WaitFileDone,
    WaitFinish,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum RecvState {
    SessionBegin,
    FileBegin,
    FileReadingMetadata,
    FileReadingSubpacket,
    FileWaitingSubpacket,
    SessionEnd,
}

struct HeaderReader {
    state: HeaderReadState,
    zpad_state: ZpadState,
    buf: Buffer<HEADER_SIZE>,
    encoding: Option<Encoding>,
    expected_len: usize,
    escape_pending: bool,
}

impl HeaderReader {
    const fn new() -> Self {
        Self {
            state: HeaderReadState::SeekingZpad,
            zpad_state: ZpadState::Idle,
            buf: Buffer::<HEADER_SIZE>::new(),
            encoding: None,
            expected_len: 0,
            escape_pending: false,
        }
    }

    fn reset(&mut self) {
        self.state = HeaderReadState::SeekingZpad;
        self.zpad_state = ZpadState::Idle;
        self.encoding = None;
        self.expected_len = 0;
        self.escape_pending = false;
        self.buf.clear();
    }

    fn advance_zpad_state(&mut self, byte: u8) -> bool {
        match self.zpad_state {
            ZpadState::Idle => {
                if byte == ZPAD {
                    self.zpad_state = ZpadState::Zpad;
                }
            }
            ZpadState::Zpad | ZpadState::ZpadZpad => {
                if byte == ZDLE {
                    self.zpad_state = ZpadState::Idle;
                    return true;
                }
                if byte == ZPAD {
                    self.zpad_state = ZpadState::ZpadZpad;
                } else {
                    self.zpad_state = ZpadState::Idle;
                }
            }
        }
        false
    }

    fn read<P>(&mut self, port: &mut P) -> Result<Option<Header>, Error>
    where
        P: Read + ?Sized,
    {
        loop {
            match self.state {
                HeaderReadState::SeekingZpad => {
                    let Some(byte) = port.read_byte()? else {
                        return Ok(None);
                    };
                    if self.advance_zpad_state(byte) {
                        self.state = HeaderReadState::ReadingEncoding;
                    }
                }
                HeaderReadState::ReadingEncoding => {
                    let Some(byte) = port.read_byte()? else {
                        return Ok(None);
                    };
                    let encoding = match Encoding::try_from(byte) {
                        Ok(encoding) => encoding,
                        Err(e) => {
                            self.reset();
                            return Err(e);
                        }
                    };
                    self.expected_len = Header::read_size(encoding);
                    self.encoding = Some(encoding);
                    self.escape_pending = false;
                    self.buf.clear();
                    self.state = HeaderReadState::ReadingData;
                }
                HeaderReadState::ReadingData => {
                    while self.buf.len() < self.expected_len {
                        let Some(byte) =
                            read_byte_unescaped_stateful(port, &mut self.escape_pending)?
                        else {
                            return Ok(None);
                        };
                        self.buf.push(byte).map_err(|_| Error::OutOfMemory)?;
                    }

                    let Some(encoding) = self.encoding else {
                        self.reset();
                        return Err(Error::MalformedHeader);
                    };

                    let header = match decode_header(encoding, &self.buf) {
                        Ok(header) => header,
                        Err(e) => {
                            self.reset();
                            return Err(e);
                        }
                    };
                    self.reset();
                    return Ok(Some(header));
                }
            }
        }
    }
}

struct SliceReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> SliceReader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn consumed(&self) -> usize {
        self.pos
    }
}

impl Read for SliceReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<Option<u32>, Error> {
        if self.pos >= self.buf.len() {
            return Ok(None);
        }
        let n = min(buf.len(), self.buf.len() - self.pos);
        buf[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        u32::try_from(n).map(Some).map_err(|_| Error::OutOfMemory)
    }

    fn read_byte(&mut self) -> Result<Option<u8>, Error> {
        if let Some(byte) = self.buf.get(self.pos) {
            self.pos += 1;
            Ok(Some(*byte))
        } else {
            Ok(None)
        }
    }
}

struct BufferWriter<'a, const N: usize> {
    buf: &'a mut Buffer<N>,
}

impl<'a, const N: usize> BufferWriter<'a, N> {
    fn new(buf: &'a mut Buffer<N>) -> Self {
        Self { buf }
    }
}

impl<const N: usize> Write for BufferWriter<'_, N> {
    fn write_all(&mut self, buf: &[u8]) -> Result<Option<()>, Error> {
        if self.buf.len() + buf.len() > self.buf.capacity() {
            return Ok(None);
        }
        self.buf
            .extend_from_slice(buf)
            .map_err(|_| Error::OutOfMemory)?;
        Ok(Some(()))
    }

    fn write_byte(&mut self, value: u8) -> Result<Option<()>, Error> {
        if self.buf.len() == self.buf.capacity() {
            return Ok(None);
        }
        self.buf.push(value).map_err(|_| Error::OutOfMemory)?;
        Ok(Some(()))
    }
}

struct RxCrc {
    calc16: crc::Crc16,
    calc32: crc::Crc32,
    buf: [u8; 4],
    bytes_read: u8,
    escape_pending: bool,
}

impl RxCrc {
    fn new() -> Self {
        Self {
            calc16: crc::Crc16::new(),
            calc32: crc::Crc32::new(),
            buf: [0; 4],
            bytes_read: 0,
            escape_pending: false,
        }
    }

    fn reset(&mut self) {
        self.calc16 = crc::Crc16::new();
        self.calc32 = crc::Crc32::new();
        self.bytes_read = 0;
        self.escape_pending = false;
    }

    fn update(&mut self, byte: u8, encoding: Encoding) {
        if encoding == Encoding::ZBIN32 {
            self.calc32.update_byte(byte);
        } else {
            self.calc16.update_byte(byte);
        }
    }

    fn process<P: Read + ?Sized>(
        &mut self,
        port: &mut P,
        encoding: Encoding,
    ) -> Result<Option<()>, Error> {
        let crc_len = if encoding == Encoding::ZBIN32 { 4 } else { 2 };
        let Some(byte) = read_byte_unescaped_stateful(port, &mut self.escape_pending)? else {
            return Ok(None);
        };
        self.buf[self.bytes_read as usize] = byte;
        self.bytes_read += 1;

        if self.bytes_read < crc_len {
            return Ok(None);
        }

        if encoding == Encoding::ZBIN32 {
            let expected = self.calc32.finalize().to_le_bytes();
            if expected != self.buf {
                return Err(Error::UnexpectedCrc32);
            }
        } else {
            let expected = self.calc16.finalize().to_be_bytes();
            if expected != [self.buf[0], self.buf[1]] {
                return Err(Error::UnexpectedCrc16);
            }
        }
        Ok(Some(()))
    }
}

/// ZMODEM sender state machine.
pub struct Sender {
    state: SendState,
    file_name: String,
    file_size: u32,
    has_file: bool,
    pending_request: Option<FileRequest>,
    frame_remaining: usize,
    frame_needs_header: bool,
    max_subpacket_size: usize,
    max_subpackets_per_ack: usize,
    buf: Buffer<SUBPACKET_MAX_SIZE>,
    outgoing: Buffer<WIRE_BUF_SIZE>,
    outgoing_offset: usize,
    header_reader: HeaderReader,
    pending_event: Option<SenderEvent>,
    finish_requested: bool,
    /// `WaitFilePos` 下 recover（补发 ZRINIT+ZFILE）已执行次数（上限 4，见模块内同名常量）。
    zrinit_extra_zfile_in_wait_file_pos: u8,
    /// `WaitFilePos` 重邀恢复节流时间戳，避免同一批 backlog 邀请导致瞬间连发多次 ZFILE。
    #[cfg(feature = "std")]
    last_wait_file_pos_recover_at: Option<Instant>,
    /// 最近一次对端 `ZRQINIT`/`ZRINIT` 能力字节（`flags[2]|flags[3]`），用于回显 `ESCCTL` 等。
    peer_cap_byte: u8,
    /// ZFILE 线材兼容模式（在重邀 recover 时自动轮换）。
    zfile_wire_mode: ZfileWireMode,
}

impl Sender {
    /// Create a new sender instance.
    ///
    /// # Errors
    ///
    /// * [`Write`](crate::Error::Write) when the write I/O fails with the serial port
    pub fn new() -> Result<Self, Error> {
        let mut sender = Self {
            state: SendState::WaitReceiverInit,
            file_name: String::new(),
            file_size: 0,
            has_file: false,
            pending_request: None,
            frame_remaining: 0,
            frame_needs_header: false,
            max_subpacket_size: SUBPACKET_MAX_SIZE,
            max_subpackets_per_ack: SUBPACKET_PER_ACK,
            buf: Buffer::<SUBPACKET_MAX_SIZE>::new(),
            outgoing: Buffer::<WIRE_BUF_SIZE>::new(),
            outgoing_offset: 0,
            header_reader: HeaderReader::new(),
            pending_event: None,
            finish_requested: false,
            zrinit_extra_zfile_in_wait_file_pos: 0,
            #[cfg(feature = "std")]
            last_wait_file_pos_recover_at: None,
            peer_cap_byte: 0,
            zfile_wire_mode: zfile_wire_mode_default(),
        };
        sender.queue_zrqinit()?;
        Ok(sender)
    }

    /// Starts sending a file with the provided metadata.
    ///
    /// # Errors
    ///
    /// * [`Write`](crate::Error::Write) when the write I/O fails with the serial port
    pub fn start_file(&mut self, file_name: &[u8], file_size: u32) -> Result<(), Error> {
        if matches!(self.state, SendState::Done | SendState::WaitFinish)
            || (!matches!(
                self.state,
                SendState::WaitReceiverInit | SendState::ReadyForFile
            ))
        {
            return Err(Error::Unsupported);
        }

        self.file_name.clear();
        self.file_name
            .extend_from_slice(file_name)
            .map_err(|_| Error::OutOfMemory)?;
        self.file_size = file_size;
        self.has_file = true;
        self.pending_request = None;
        self.frame_remaining = 0;
        self.frame_needs_header = false;
        self.zrinit_extra_zfile_in_wait_file_pos = 0;
        #[cfg(feature = "std")]
        {
            self.last_wait_file_pos_recover_at = None;
        }
        self.zfile_wire_mode = zfile_wire_mode_default();

        if self.state == SendState::ReadyForFile {
            if self.outgoing() {
                return Err(Error::Unsupported);
            }
            self.queue_zfile()?;
            self.state = SendState::WaitFilePos;
        }
        Ok(())
    }

    /// Requests to finish the session after the current file completes.
    ///
    /// # Errors
    ///
    /// * [`Write`](crate::Error::Write) when the write I/O fails with the serial port
    pub fn finish_session(&mut self) -> Result<(), Error> {
        self.finish_requested = true;
        if self.state == SendState::ReadyForFile {
            if self.outgoing() {
                return Err(Error::Unsupported);
            }
            self.queue_zfin()?;
            self.state = SendState::WaitFinish;
        }
        Ok(())
    }

    /// Returns a pending file data request, if any.
    #[must_use]
    pub fn poll_file(&self) -> Option<FileRequest> {
        self.pending_request
    }

    /// Feeds a chunk of file data for the current request.
    ///
    /// # Errors
    ///
    /// * [`Write`](crate::Error::Write) when the write I/O fails with the serial port
    pub fn feed_file(&mut self, data: &[u8]) -> Result<(), Error> {
        if self.state != SendState::NeedFileData {
            return Err(Error::Unsupported);
        }
        let Some(request) = self.pending_request else {
            return Err(Error::Unsupported);
        };

        if data.is_empty() {
            return Err(Error::UnexpectedEof);
        }
        if data.len() > request.len {
            return Err(Error::UnexpectedEof);
        }
        let remaining = self.file_size.saturating_sub(request.offset) as usize;
        if data.len() > remaining {
            return Err(Error::UnexpectedEof);
        }
        if self.outgoing() {
            return Err(Error::Unsupported);
        }

        let offset = request.offset;
        let next_offset = offset
            .checked_add(u32::try_from(data.len()).map_err(|_| Error::OutOfMemory)?)
            .ok_or(Error::OutOfMemory)?;
        let remaining_after = self.file_size.saturating_sub(next_offset);
        let max_len = min(self.max_subpacket_size, remaining_after as usize);
        let is_last_in_frame =
            self.frame_remaining <= 1 || data.len() < request.len || remaining_after == 0;
        let kind = if is_last_in_frame {
            SubpacketType::ZCRCW
        } else {
            SubpacketType::ZCRCG
        };

        self.queue_zdata(offset, data, kind, self.frame_needs_header)?;
        self.frame_needs_header = false;

        if self.frame_remaining > 0 {
            self.frame_remaining -= 1;
        }

        if is_last_in_frame {
            self.pending_request = None;
            self.state = SendState::WaitFileAck;
            self.frame_remaining = 0;
        } else {
            self.pending_request = Some(FileRequest {
                offset: next_offset,
                len: max_len,
            });
        }
        Ok(())
    }

    /// Feeds incoming wire data into the state machine.
    ///
    /// Returns the number of bytes consumed.
    ///
    /// # Errors
    ///
    /// * [`Read`](crate::Error::Read) when the read I/O fails with the serial port
    /// * [`Write`](crate::Error::Write) when the write I/O fails with the serial port
    /// * [`UnexpectedCrc16`](crate::Error::UnexpectedCrc16) or
    ///   [`UnexpectedCrc32`](crate::Error::UnexpectedCrc32) when corrupted data has been detected
    pub fn feed_incoming(&mut self, input: &[u8]) -> Result<usize, Error> {
        let mut reader = SliceReader::new(input);

        loop {
            if self.outgoing() || self.state == SendState::Done || self.pending_request.is_some() {
                break;
            }

            let before = reader.consumed();
            let header = match self.header_reader.read(&mut reader) {
                Ok(Some(header)) => header,
                Ok(None) => break,
                Err(e) => {
                    let _ = self.queue_nak();
                    return Err(e);
                }
            };

            self.handle_header(header)?;

            if reader.consumed() == before || reader.consumed() == input.len() {
                break;
            }
        }

        Ok(reader.consumed())
    }

    /// Returns pending outgoing bytes.
    #[must_use]
    pub fn drain_outgoing(&self) -> &[u8] {
        &self.outgoing[self.outgoing_offset..]
    }

    /// Advances the outgoing cursor by `n` bytes.
    pub fn advance_outgoing(&mut self, n: usize) {
        let remaining = self.outgoing.len().saturating_sub(self.outgoing_offset);
        let n = min(n, remaining);
        self.outgoing_offset += n;
        if self.outgoing_offset >= self.outgoing.len() {
            self.outgoing.clear();
            self.outgoing_offset = 0;
        }
    }

    /// Returns the next pending sender event.
    pub fn poll_event(&mut self) -> Option<SenderEvent> {
        self.pending_event.take()
    }

    /// In `WaitFilePos`, proactively retries handshake bytes when peer goes silent.
    ///
    /// Returns `true` when a retry (`ZRINIT? + ZFILE`) has been queued.
    ///
    /// # Errors
    ///
    /// * [`Write`](crate::Error::Write) when the write I/O fails with the serial port
    pub fn retry_wait_file_pos_handshake(&mut self) -> Result<bool, Error> {
        if self.state != SendState::WaitFilePos || !self.has_file || self.outgoing() {
            return Ok(false);
        }
        if self.zrinit_extra_zfile_in_wait_file_pos >= MAX_WAIT_FILE_POS_RECOVER_ATTEMPTS {
            return Ok(false);
        }
        if mistterm_sender_reply_zrinit_enabled() {
            self.queue_zrinit_reply()?;
        }
        self.zfile_wire_mode = self.zfile_wire_mode.next();
        self.queue_zfile()?;
        self.zrinit_extra_zfile_in_wait_file_pos =
            self.zrinit_extra_zfile_in_wait_file_pos.saturating_add(1);
        Ok(true)
    }

    fn outgoing(&self) -> bool {
        self.outgoing_offset < self.outgoing.len()
    }

    fn queue_writer(&mut self) -> Result<BufferWriter<'_, WIRE_BUF_SIZE>, Error> {
        if self.outgoing() {
            return Err(Error::Unsupported);
        }
        Ok(BufferWriter::new(&mut self.outgoing))
    }

    fn queue_zrqinit(&mut self) -> Result<(), Error> {
        let mut writer = self.queue_writer()?;
        if ZRQINIT_HEADER.write(&mut writer)?.is_none() {
            return Err(Error::OutOfMemory);
        }
        Ok(())
    }

    /// Queue ZRINIT for the receiving program's ZRQINIT (e.g. lrzsz `rz` inviting the sender).
    fn queue_zrinit_reply(&mut self) -> Result<(), Error> {
        let peer_caps = self.peer_cap_byte;
        let mut writer = self.queue_writer()?;
        if write_zrinit_sender_reply(&mut writer, peer_caps)?.is_none() {
            return Err(Error::OutOfMemory);
        }
        Ok(())
    }

    fn queue_zfile(&mut self) -> Result<(), Error> {
        let file_size = self.file_size;
        let file_name = &self.file_name;
        let wire_mode = self.zfile_wire_mode;
        let mut writer = BufferWriter::new(&mut self.outgoing);
        if write_zfile(&mut writer, &mut self.buf, file_name, file_size, wire_mode)?.is_none() {
            return Err(Error::OutOfMemory);
        }
        Ok(())
    }

    fn queue_zdata(
        &mut self,
        offset: u32,
        data: &[u8],
        kind: SubpacketType,
        include_header: bool,
    ) -> Result<(), Error> {
        let mut writer = self.queue_writer()?;
        if include_header
            && ZDATA_HEADER
                .with_count(offset)
                .write(&mut writer)?
                .is_none()
        {
            return Err(Error::OutOfMemory);
        }
        if write_subpacket(&mut writer, Encoding::ZBIN32, kind, data)?.is_none() {
            return Err(Error::OutOfMemory);
        }
        Ok(())
    }

    fn queue_zeof(&mut self, offset: u32) -> Result<(), Error> {
        let mut writer = self.queue_writer()?;
        if ZEOF_HEADER.with_count(offset).write(&mut writer)?.is_none() {
            return Err(Error::OutOfMemory);
        }
        Ok(())
    }

    fn queue_zfin(&mut self) -> Result<(), Error> {
        let mut writer = self.queue_writer()?;
        if ZFIN_HEADER.write(&mut writer)?.is_none() {
            return Err(Error::OutOfMemory);
        }
        Ok(())
    }

    fn queue_nak(&mut self) -> Result<(), Error> {
        let mut writer = self.queue_writer()?;
        if ZNAK_HEADER.write(&mut writer)?.is_none() {
            return Err(Error::OutOfMemory);
        }
        Ok(())
    }

    fn queue_oo(&mut self) -> Result<(), Error> {
        let mut writer = self.queue_writer()?;
        if writer.write_byte(b'O')?.is_none() {
            return Err(Error::OutOfMemory);
        }
        if writer.write_byte(b'O')?.is_none() {
            return Err(Error::OutOfMemory);
        }
        Ok(())
    }

    fn handle_header(&mut self, header: Header) -> Result<(), Error> {
        match header.frame() {
            Frame::ZRINIT => self.on_zrinit(header),
            Frame::ZRQINIT => {
                // Receiving program sends ZRQINIT first; default behavior is to answer with ZFILE
                // only (same direction as common `sz`), and optionally prepend sender ZRINIT when
                // `MISTTERM_ZMODEM_SENDER_REPLY_ZRINIT=1`.
                //
                // **必须**把对端 `header` 交给 `on_zrinit`：先前若传全零 flags，`update_receiver_caps`
                // 会丢掉 lrzsz 在邀请帧里的缓冲/能力位，表现为首轮 ZFILE 后远端长期不写 ZRPOS。
                // `on_zrinit` 内在 `WaitReceiverInit` 下会 `queue_zrinit_reply` + `queue_zfile`，勿在此处先排队。
                if self.state == SendState::WaitReceiverInit {
                    self.on_zrinit(header)?;
                } else if self.state == SendState::WaitFilePos {
                    // lrzsz 在 ZFILE 后仍可能按超时重发 **ZRQINIT**（而非 ZRINIT）；此前整段被忽略，
                    // 与 `WaitFilePos` 下忽略 ZRINIT 一样会导致长时间无 ZRPOS。
                    self.recover_wait_file_pos_after_peer_reinvite()?;
                }
                Ok(())
            }
            Frame::ZRPOS | Frame::ZACK => self.on_zrpos(header.count()),
            Frame::ZFIN => self.on_zfin(),
            _ => {
                if self.state == SendState::WaitReceiverInit {
                    self.queue_zrqinit()?;
                }
                Ok(())
            }
        }
    }

    fn on_zrinit(&mut self, header: Header) -> Result<(), Error> {
        self.update_receiver_caps(header);
        match self.state {
            SendState::WaitReceiverInit => {
                if self.has_file {
                    // 默认更贴近外部 `sz`：直接发 ZFILE；仅在显式开启时额外回 sender ZRINIT。
                    if !self.outgoing() && mistterm_sender_reply_zrinit_enabled() {
                        self.queue_zrinit_reply()?;
                    }
                    self.queue_zfile()?;
                    self.state = SendState::WaitFilePos;
                } else {
                    self.state = SendState::ReadyForFile;
                    if self.finish_requested {
                        self.queue_zfin()?;
                        self.state = SendState::WaitFinish;
                    }
                }
            }
            SendState::WaitFileDone => {
                self.pending_event = Some(SenderEvent::FileComplete);
                self.has_file = false;
                if self.finish_requested {
                    self.queue_zfin()?;
                    self.state = SendState::WaitFinish;
                } else {
                    self.state = SendState::ReadyForFile;
                }
            }
            SendState::WaitFinish => {
                self.queue_oo()?;
                self.state = SendState::Done;
                self.pending_event = Some(SenderEvent::SessionComplete);
            }
            SendState::WaitFilePos => {
                self.recover_wait_file_pos_after_peer_reinvite()?;
            }
            _ => {}
        }
        Ok(())
    }

    /// `WaitFilePos` 下对端重发「邀请」帧（ZRINIT 或 ZRQINIT）时补发 **ZFILE**（有次数上限），
    /// 仅在 `MISTTERM_ZMODEM_SENDER_REPLY_ZRINIT=1` 时额外补发 sender ZRINIT。
    fn recover_wait_file_pos_after_peer_reinvite(&mut self) -> Result<(), Error> {
        if self.state != SendState::WaitFilePos || !self.has_file || self.outgoing() {
            return Ok(());
        }
        if self.zrinit_extra_zfile_in_wait_file_pos >= MAX_WAIT_FILE_POS_RECOVER_ATTEMPTS {
            return Ok(());
        }
        #[cfg(feature = "std")]
        if let Some(last) = self.last_wait_file_pos_recover_at {
            if last.elapsed() < WAIT_FILE_POS_RECOVER_MIN_INTERVAL {
                return Ok(());
            }
        }
        if mistterm_sender_reply_zrinit_enabled() {
            self.queue_zrinit_reply()?;
        }
        self.zfile_wire_mode = self.zfile_wire_mode.next();
        self.queue_zfile()?;
        self.zrinit_extra_zfile_in_wait_file_pos =
            self.zrinit_extra_zfile_in_wait_file_pos.saturating_add(1);
        #[cfg(feature = "std")]
        {
            self.last_wait_file_pos_recover_at = Some(Instant::now());
        }
        Ok(())
    }

    fn update_receiver_caps(&mut self, header: Header) {
        let flags = header.count().to_le_bytes();
        let caps = flags[2] | flags[3];
        self.peer_cap_byte = caps;
        let rx_buf_size = u16::from_le_bytes([flags[0], flags[1]]) as usize;
        let can_ovio = (caps & Zrinit::CANOVIO.bits()) != 0;

        if rx_buf_size == 0 {
            self.max_subpacket_size = SUBPACKET_MAX_SIZE;
            self.max_subpackets_per_ack = if can_ovio { SUBPACKET_PER_ACK } else { 1 };
            return;
        }

        self.max_subpacket_size = min(SUBPACKET_MAX_SIZE, rx_buf_size);
        if !can_ovio {
            self.max_subpackets_per_ack = 1;
            return;
        }

        let subpackets = rx_buf_size / self.max_subpacket_size;
        self.max_subpackets_per_ack = if subpackets == 0 { 1 } else { subpackets };
    }

    fn on_zrpos(&mut self, offset: u32) -> Result<(), Error> {
        match self.state {
            SendState::WaitReceiverInit => {
                self.queue_zrqinit()?;
            }
            SendState::WaitFilePos | SendState::WaitFileAck | SendState::NeedFileData => {
                if offset >= self.file_size {
                    self.queue_zeof(offset)?;
                    self.state = SendState::WaitFileDone;
                    self.pending_request = None;
                } else {
                    let remaining = (self.file_size - offset) as usize;
                    let max_subpackets = remaining.div_ceil(self.max_subpacket_size);
                    self.frame_remaining = min(self.max_subpackets_per_ack, max_subpackets);
                    self.frame_needs_header = true;
                    let len = min(self.max_subpacket_size, remaining);
                    self.pending_request = Some(FileRequest { offset, len });
                    self.state = SendState::NeedFileData;
                    self.zrinit_extra_zfile_in_wait_file_pos = 0;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn on_zfin(&mut self) -> Result<(), Error> {
        if self.state == SendState::WaitFinish {
            self.queue_oo()?;
            self.state = SendState::Done;
            self.pending_event = Some(SenderEvent::SessionComplete);
        }
        Ok(())
    }
}

/// ZMODEM receiver state machine.
pub struct Receiver {
    state: RecvState,
    count: u32,
    file_name: String,
    file_size: u32,
    buf: Buffer<SUBPACKET_MAX_SIZE>,
    buf_write_offset: usize,
    data_encoding: Encoding,
    header_reader: HeaderReader,
    subpacket_state: SubpacketState,
    subpacket_escape_pending: bool,
    crc: RxCrc,
    outgoing: Buffer<WIRE_BUF_SIZE>,
    outgoing_offset: usize,
    pending_events: [Option<ReceiverEvent>; RECEIVER_EVENT_QUEUE_CAP],
    pending_event_head: usize,
    pending_event_len: usize,
}

impl Receiver {
    /// Create a new receiver instance.
    ///
    /// # Errors
    ///
    /// * [`Write`](crate::Error::Write) when the write I/O fails with the serial port
    pub fn new() -> Result<Self, Error> {
        let mut receiver = Self {
            state: RecvState::SessionBegin,
            count: 0,
            file_name: String::new(),
            file_size: 0,
            buf: Buffer::<SUBPACKET_MAX_SIZE>::new(),
            buf_write_offset: 0,
            data_encoding: Encoding::ZBIN,
            header_reader: HeaderReader::new(),
            subpacket_state: SubpacketState::Idle,
            subpacket_escape_pending: false,
            crc: RxCrc::new(),
            outgoing: Buffer::<WIRE_BUF_SIZE>::new(),
            outgoing_offset: 0,
            pending_events: [None; RECEIVER_EVENT_QUEUE_CAP],
            pending_event_head: 0,
            pending_event_len: 0,
        };
        receiver.queue_zrinit()?;
        Ok(receiver)
    }

    /// Feeds incoming wire data into the state machine.
    ///
    /// Returns the number of bytes consumed.
    ///
    /// # Errors
    ///
    /// * [`Read`](crate::Error::Read) when the read I/O fails with the serial port
    /// * [`Write`](crate::Error::Write) when the write I/O fails with the serial port
    /// * [`UnexpectedCrc16`](crate::Error::UnexpectedCrc16) or
    ///   [`UnexpectedCrc32`](crate::Error::UnexpectedCrc32) when corrupted data has been detected
    pub fn feed_incoming(&mut self, input: &[u8]) -> Result<usize, Error> {
        let mut reader = SliceReader::new(input);

        loop {
            if self.outgoing() || !self.drain_file().is_empty() || self.pending_events_full() {
                break;
            }

            let before = reader.consumed();

            if matches!(
                self.state,
                RecvState::FileReadingSubpacket | RecvState::FileReadingMetadata
            ) {
                match self.process_subpacket(&mut reader) {
                    Ok(Some(())) => {
                        if self.outgoing()
                            || !self.drain_file().is_empty()
                            || self.pending_events_full()
                        {
                            break;
                        }
                        if reader.consumed() == before {
                            break;
                        }
                        continue;
                    }
                    Ok(None) => break,
                    Err(e) => return Err(e),
                }
            }

            let header = match self.header_reader.read(&mut reader) {
                Ok(Some(header)) => header,
                Ok(None) => break,
                Err(e) => {
                    let _ = self.queue_nak();
                    return Err(e);
                }
            };

            self.handle_header(header)?;

            if self.pending_events_full() {
                break;
            }

            if reader.consumed() == before || reader.consumed() == input.len() {
                break;
            }
        }

        Ok(reader.consumed())
    }

    /// Returns pending outgoing bytes.
    #[must_use]
    pub fn drain_outgoing(&self) -> &[u8] {
        &self.outgoing[self.outgoing_offset..]
    }

    /// Advances the outgoing cursor by `n` bytes.
    pub fn advance_outgoing(&mut self, n: usize) {
        let remaining = self.outgoing.len().saturating_sub(self.outgoing_offset);
        let n = min(n, remaining);
        self.outgoing_offset += n;
        if self.outgoing_offset >= self.outgoing.len() {
            self.outgoing.clear();
            self.outgoing_offset = 0;
        }
    }

    /// Returns pending file data bytes.
    #[must_use]
    pub fn drain_file(&self) -> &[u8] {
        match self.subpacket_state {
            SubpacketState::Writing(_) => &self.buf[self.buf_write_offset..],
            _ => &[],
        }
    }

    /// Advances the file output cursor by `n` bytes.
    ///
    /// # Errors
    ///
    /// * [`Write`](crate::Error::Write) when the write I/O fails with the serial port
    pub fn advance_file(&mut self, n: usize) -> Result<(), Error> {
        let SubpacketState::Writing(packet) = self.subpacket_state else {
            return Ok(());
        };

        let remaining = self.buf.len().saturating_sub(self.buf_write_offset);
        let n = min(n, remaining);
        self.buf_write_offset = self
            .buf_write_offset
            .checked_add(n)
            .ok_or(Error::OutOfMemory)?;

        if self.buf_write_offset < self.buf.len() {
            return Ok(());
        }

        self.finish_subpacket(packet)
    }

    /// Returns the next pending receiver event.
    pub fn poll_event(&mut self) -> Option<ReceiverEvent> {
        self.pop_event()
    }

    #[must_use]
    pub fn file_name(&self) -> &[u8] {
        &self.file_name
    }

    #[must_use]
    pub fn file_size(&self) -> u32 {
        self.file_size
    }

    fn outgoing(&self) -> bool {
        self.outgoing_offset < self.outgoing.len()
    }

    fn pending_events_full(&self) -> bool {
        self.pending_event_len >= RECEIVER_EVENT_QUEUE_CAP
    }

    fn push_event(&mut self, event: ReceiverEvent) -> Result<(), Error> {
        if self.pending_events_full() {
            return Err(Error::OutOfMemory);
        }
        let index = (self.pending_event_head + self.pending_event_len) % RECEIVER_EVENT_QUEUE_CAP;
        self.pending_events[index] = Some(event);
        self.pending_event_len += 1;
        Ok(())
    }

    fn pop_event(&mut self) -> Option<ReceiverEvent> {
        if self.pending_event_len == 0 {
            return None;
        }
        let event = self.pending_events[self.pending_event_head].take();
        self.pending_event_head = (self.pending_event_head + 1) % RECEIVER_EVENT_QUEUE_CAP;
        self.pending_event_len -= 1;
        event
    }

    fn queue_writer(&mut self) -> Result<BufferWriter<'_, WIRE_BUF_SIZE>, Error> {
        if self.outgoing() {
            return Err(Error::Unsupported);
        }
        Ok(BufferWriter::new(&mut self.outgoing))
    }

    fn queue_zrinit(&mut self) -> Result<(), Error> {
        let mut writer = self.queue_writer()?;
        if write_zrinit(&mut writer)?.is_none() {
            return Err(Error::OutOfMemory);
        }
        Ok(())
    }

    fn queue_zrpos(&mut self, count: u32) -> Result<(), Error> {
        let mut writer = self.queue_writer()?;
        if ZRPOS_HEADER.with_count(count).write(&mut writer)?.is_none() {
            return Err(Error::OutOfMemory);
        }
        Ok(())
    }

    fn queue_zack(&mut self) -> Result<(), Error> {
        let count = self.count;
        let mut writer = self.queue_writer()?;
        if ZACK_HEADER.with_count(count).write(&mut writer)?.is_none() {
            return Err(Error::OutOfMemory);
        }
        Ok(())
    }

    fn queue_zfin(&mut self) -> Result<(), Error> {
        let mut writer = self.queue_writer()?;
        if ZFIN_HEADER.write(&mut writer)?.is_none() {
            return Err(Error::OutOfMemory);
        }
        Ok(())
    }

    fn queue_nak(&mut self) -> Result<(), Error> {
        let mut writer = self.queue_writer()?;
        if ZNAK_HEADER.write(&mut writer)?.is_none() {
            return Err(Error::OutOfMemory);
        }
        Ok(())
    }

    fn handle_header(&mut self, header: Header) -> Result<(), Error> {
        match header.frame() {
            Frame::ZRQINIT => {
                if self.state == RecvState::SessionBegin {
                    self.queue_zrinit()?;
                }
            }
            Frame::ZFILE => {
                if self.state == RecvState::SessionBegin || self.state == RecvState::FileBegin {
                    self.data_encoding = header.encoding();
                    self.state = RecvState::FileReadingMetadata;
                    self.subpacket_state = SubpacketState::Reading;
                    self.subpacket_escape_pending = false;
                    self.crc.reset();
                    self.buf.clear();
                    self.buf_write_offset = 0;
                }
            }
            Frame::ZDATA => {
                if self.state == RecvState::SessionBegin {
                    self.queue_zrinit()?;
                } else if self.state == RecvState::FileBegin
                    || self.state == RecvState::FileWaitingSubpacket
                {
                    if header.count() != self.count {
                        self.queue_zrpos(self.count)?;
                        return Ok(());
                    }
                    self.data_encoding = header.encoding();
                    self.state = RecvState::FileReadingSubpacket;
                    self.subpacket_state = SubpacketState::Reading;
                    self.subpacket_escape_pending = false;
                    self.crc.reset();
                    self.buf.clear();
                    self.buf_write_offset = 0;
                }
            }
            Frame::ZEOF => {
                if self.state == RecvState::FileWaitingSubpacket && header.count() == self.count {
                    self.queue_zrinit()?;
                    self.state = RecvState::FileBegin;
                    self.push_event(ReceiverEvent::FileComplete)?;
                }
            }
            Frame::ZFIN => {
                if self.state == RecvState::FileWaitingSubpacket
                    || self.state == RecvState::FileBegin
                {
                    self.queue_zfin()?;
                    self.state = RecvState::SessionEnd;
                    self.push_event(ReceiverEvent::SessionComplete)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Parses the file info buffer after a ZFILE subpacket is received.
    fn parse_zfile_buf(&mut self) -> Result<(), Error> {
        let payload = &self.buf;
        let mut fields = payload.split(|&b| b == b'\0');

        let file_name_bytes = fields.next().ok_or(Error::MalformedFileName)?;
        if file_name_bytes.is_empty() {
            return Err(Error::MalformedFileName);
        }

        self.file_name.clear();
        self.file_name
            .extend_from_slice(file_name_bytes)
            .map_err(|_| Error::OutOfMemory)?;

        if let Some(size_str_bytes) = fields.next() {
            let size_field_bytes = size_str_bytes
                .split(|&b| b == b' ')
                .next()
                .unwrap_or_default();

            self.file_size = parse_file_size(size_field_bytes)?;
        } else {
            self.file_size = 0;
        }

        self.count = 0;
        Ok(())
    }

    /// Handles reading a single byte for the `SubpacketState::Reading` state.
    fn receive_subpacket_data_byte<P>(&mut self, port: &mut P) -> Result<Option<()>, Error>
    where
        P: Read + ?Sized,
    {
        let handle_followup = |this: &mut Self, byte: u8| -> Result<Option<()>, Error> {
            if let Ok(packet) = SubpacketType::try_from(byte) {
                this.crc.update(packet as u8, this.data_encoding);
                this.subpacket_state = SubpacketState::Crc(packet);
            } else {
                let unescaped = zdle::UNZDLE_TABLE[byte as usize];
                this.buf.push(unescaped).map_err(|_| Error::OutOfMemory)?;
                this.crc.update(unescaped, this.data_encoding);
            }
            Ok(Some(()))
        };

        if self.subpacket_escape_pending {
            let Some(byte) = port.read_byte()? else {
                return Ok(None);
            };
            self.subpacket_escape_pending = false;
            return handle_followup(self, byte);
        }

        let Some(byte) = port.read_byte()? else {
            return Ok(None);
        };
        if byte == ZDLE {
            let Some(next) = port.read_byte()? else {
                self.subpacket_escape_pending = true;
                return Ok(None);
            };
            return handle_followup(self, next);
        }

        self.buf.push(byte).map_err(|_| Error::OutOfMemory)?;
        self.crc.update(byte, self.data_encoding);
        Ok(Some(()))
    }

    fn process_subpacket<P>(&mut self, port: &mut P) -> Result<Option<()>, Error>
    where
        P: Read + ?Sized,
    {
        match self.subpacket_state {
            SubpacketState::Reading => self.receive_subpacket_data_byte(port),
            SubpacketState::Crc(packet) => {
                if self.crc.process(port, self.data_encoding)?.is_none() {
                    return Ok(None);
                }

                if self.state == RecvState::FileReadingMetadata {
                    self.parse_zfile_buf()?;
                    self.buf.clear();
                    self.buf_write_offset = 0;
                    self.crc.reset();
                    self.subpacket_escape_pending = false;

                    self.queue_zrpos(0)?;

                    self.state = RecvState::FileBegin;
                    self.subpacket_state = SubpacketState::Idle;
                    self.push_event(ReceiverEvent::FileStart)?;
                } else {
                    self.subpacket_state = SubpacketState::Writing(packet);
                    self.buf_write_offset = 0;
                    if self.buf.is_empty() {
                        self.finish_subpacket(packet)?;
                    }
                }
                Ok(Some(()))
            }
            SubpacketState::Writing(_) => Ok(Some(())),
            SubpacketState::Idle => Err(Error::Unsupported),
        }
    }

    fn finish_subpacket(&mut self, packet: SubpacketType) -> Result<(), Error> {
        self.count += u32::try_from(self.buf.len()).map_err(|_| Error::OutOfMemory)?;
        self.buf.clear();
        self.buf_write_offset = 0;
        self.crc.reset();

        match packet {
            SubpacketType::ZCRCW => {
                self.queue_zack()?;
                self.state = RecvState::FileWaitingSubpacket;
                self.subpacket_state = SubpacketState::Idle;
                self.subpacket_escape_pending = false;
            }
            SubpacketType::ZCRCQ => {
                self.queue_zack()?;
                self.subpacket_state = SubpacketState::Reading;
                self.subpacket_escape_pending = false;
            }
            SubpacketType::ZCRCG => {
                self.subpacket_state = SubpacketState::Reading;
                self.subpacket_escape_pending = false;
            }
            SubpacketType::ZCRCE => {
                self.state = RecvState::FileWaitingSubpacket;
                self.subpacket_state = SubpacketState::Idle;
                self.subpacket_escape_pending = false;
            }
        }
        Ok(())
    }
}

fn read_byte_unescaped_stateful<P>(port: &mut P, pending: &mut bool) -> Result<Option<u8>, Error>
where
    P: Read + ?Sized,
{
    if *pending {
        let Some(b) = port.read_byte()? else {
            return Ok(None);
        };
        *pending = false;
        return Ok(Some(zdle::UNZDLE_TABLE[b as usize]));
    }

    let Some(b) = port.read_byte()? else {
        return Ok(None);
    };
    if b == ZDLE {
        let Some(next) = port.read_byte()? else {
            *pending = true;
            return Ok(None);
        };
        return Ok(Some(zdle::UNZDLE_TABLE[next as usize]));
    }

    Ok(Some(b))
}

fn decode_header(encoding: Encoding, data: &[u8]) -> Result<Header, Error> {
    let mut out: Buffer<HEADER_SIZE> = Buffer::new();
    if encoding == Encoding::ZHEX {
        if data.len() % 2 != 0 {
            return Err(Error::MalformedHeader);
        }
        let mut out_bytes = [0u8; HEADER_SIZE / 2];
        let out_len = data.len() / 2;
        let out_buf = out_bytes.get_mut(..out_len).ok_or(Error::UnexpectedEof)?;
        hex::decode_to_slice(data, out_buf).map_err(|_| Error::MalformedHeader)?;
        out.extend_from_slice(out_buf)
            .map_err(|_| Error::OutOfMemory)?;
    } else {
        out.extend_from_slice(data)
            .map_err(|_| Error::OutOfMemory)?;
    }

    let crc_len = if encoding == Encoding::ZBIN32 { 4 } else { 2 };
    if out.len() < HEADER_PAYLOAD_SIZE + crc_len {
        return Err(Error::MalformedHeader);
    }
    let (payload, crc_bytes) = out.split_at(HEADER_PAYLOAD_SIZE);
    if encoding == Encoding::ZBIN32 {
        let expected_crc = crc::crc32_iso_hdlc(payload).to_le_bytes();
        if crc_bytes != &expected_crc[..crc_len] {
            return Err(Error::UnexpectedCrc32);
        }
    } else {
        let expected_crc = crc::crc16_xmodem(payload).to_be_bytes();
        if crc_bytes != &expected_crc[..crc_len] {
            return Err(Error::UnexpectedCrc16);
        }
    }

    let frame = Frame::try_from(payload[0])?;
    let mut flags = [0u8; 4];
    flags.copy_from_slice(&payload[1..=4]);
    Ok(Header::new(encoding, frame, &flags))
}

/// 发送端回应 `rz` 的 ZRINIT：在 PTY/SSH 上 lrzsz 依赖发送端声明 **ESCCTL**（与 `write_subpacket` 使用的 ZDLE 转义一致）。
/// 若仅回显对端能力位，当 `ZRQINIT` 里能力为 0 时会出现 **0x21**（无 ESCCTL），远端 `rz` 可能不推进协议。
fn write_zrinit_sender_reply<P>(port: &mut P, _peer_caps: u8) -> Result<Option<()>, Error>
where
    P: Write + ?Sized,
{
    let zrinit = Zrinit::CANFDX | Zrinit::CANFC32 | Zrinit::ESCCTL;
    let buffer_size = u16::try_from(SUBPACKET_MAX_SIZE)
        .map_err(|_| Error::Unsupported)?
        .to_le_bytes();
    Header::new(
        Encoding::ZHEX,
        Frame::ZRINIT,
        &[buffer_size[0], buffer_size[1], 0, zrinit.bits()],
    )
    .write(port)
}

/// Writes ZRINIT（接收端 `Receiver` 答复 `sz` 等；无对端 PTY 能力快照时用 peer_caps=0。）
fn write_zrinit<P>(port: &mut P) -> Result<Option<()>, Error>
where
    P: Write + ?Sized,
{
    write_zrinit_sender_reply(port, 0)
}

/// Parses a u32 from a slice of ASCII decimal bytes.
fn parse_file_size(bytes: &[u8]) -> Result<u32, Error> {
    if bytes.is_empty() {
        return Ok(0);
    }

    let mut result: u32 = 0;
    for &byte in bytes {
        let digit = match byte {
            b'0'..=b'9' => u32::from(byte - b'0'),
            _ => return Err(Error::MalformedFileSize),
        };
        result = result
            .checked_mul(10)
            .and_then(|r| r.checked_add(digit))
            .ok_or(Error::MalformedFileSize)?;
    }
    Ok(result)
}

/// Write ZRFILE
fn write_zfile<P>(
    port: &mut P,
    buf: &mut Buffer<SUBPACKET_MAX_SIZE>,
    name: &[u8],
    size: u32,
    wire_mode: ZfileWireMode,
) -> Result<Option<()>, Error>
where
    P: Write + ?Sized,
{
    buf.clear();
    buf.extend_from_slice(name)
        .map_err(|_| Error::OutOfMemory)?;
    buf.push(b'\0').map_err(|_| Error::OutOfMemory)?;

    // Keep YMODEM-compatible ZFILE info with extended fields:
    // "<size> <mtime(oct)> <mode(oct)> <serial(oct)> <filesleft> <bytesleft>\0"
    // Some rz variants are stricter when only "<size>\0" is present.
    // We don't yet plumb per-file metadata here, so use conservative defaults.
    write!(buf, "{size} 0 100644 0 0 {size}\0").map_err(|_| Error::OutOfMemory)?;

    let zfile_encoding = wire_mode.encoding();
    // Some rz paths are sensitive to single/double ZPAD before binary header;
    // we rotate this on recover attempts in `WaitFilePos`.
    if wire_mode.extra_zpad() {
        if port.write_byte(ZPAD)?.is_none() {
            return Ok(None);
        }
    }
    if Header::new(zfile_encoding, Frame::ZFILE, &[0; 4])
        .write(port)?
        .is_none()
    {
        return Ok(None);
    }
    write_subpacket(port, zfile_encoding, SubpacketType::ZCRCW, buf)
}

/// Writes a subpacket.
///
/// # Errors
///
/// This function returns `Error::Read` or `Error::Write` on an I/O error, or
/// `Error::Unsupported` if `ZHEX` encoding is requested.
fn write_subpacket<P>(
    port: &mut P,
    encoding: Encoding,
    kind: SubpacketType,
    data: &[u8],
) -> Result<Option<()>, Error>
where
    P: Write + ?Sized,
{
    let kind = kind as u8;
    if write_slice_escaped(port, data)?.is_none() {
        return Ok(None);
    }
    if port.write_byte(ZDLE)?.is_none() {
        return Ok(None);
    }
    if port.write_byte(kind)?.is_none() {
        return Ok(None);
    }
    match encoding {
        Encoding::ZBIN32 => {
            let mut crc = crc::Crc32::new();
            crc.update(data);
            crc.update_byte(kind);
            let buf = crc.finalize().to_le_bytes();
            write_slice_escaped(port, &buf)
        }
        Encoding::ZBIN => {
            let mut crc = crc::Crc16::new();
            crc.update(data);
            crc.update_byte(kind);
            let buf = crc.finalize().to_be_bytes();
            write_slice_escaped(port, &buf)
        }
        Encoding::ZHEX => Err(Error::Unsupported),
    }
}
