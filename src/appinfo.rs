//! Binary `appinfo.vdf` parser/writer.
//!
//! Supports Steam VDF v28 (`0x107564428`) and v29 (`0x107564429`).
//! The format is a sequence of app records, each prefixed by a fixed header,
//! followed by a recursive key-value section encoded with type-tag bytes.
//!
//! Portions of this file are derived from Steam-Metadata-Editor
//! by tralph3 <https://github.com/tralph3/Steam-Metadata-Editor>
//! Licensed under the GNU General Public License v3.0
//! SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;
use std::io::{self, Cursor, Read};

// ── version magic ────────────────────────────────────────────────────────────

const APPINFO_28: u64 = 0x107564428;
const APPINFO_29: u64 = 0x107564429;

// ── type tags ────────────────────────────────────────────────────────────────

const TYPE_DICT: u8 = 0x00;
const TYPE_STRING: u8 = 0x01;
const TYPE_INT32: u8 = 0x02;
const SECTION_END: u8 = 0x08;

// ── value type ───────────────────────────────────────────────────────────────

/// A node inside the VDF tree.
#[derive(Debug, Clone)]
pub enum Value {
    Dict(BTreeMap<String, Value>),
    String(String),
    Int32(u32),
}

impl Value {
    pub fn as_dict(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Dict(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_dict_mut(&mut self) -> Option<&mut BTreeMap<String, Value>> {
        match self {
            Value::Dict(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

// ── app record ───────────────────────────────────────────────────────────────

/// Fixed-size header that precedes each app's section data.
#[derive(Debug, Clone)]
pub struct AppHeader {
    pub appid: u32,
    pub size: u32,
    pub state: u32,
    pub last_update: u32,
    pub access_token: u64,
    pub checksum_text: [u8; 20],
    pub change_number: u32,
    pub checksum_binary: [u8; 20],
}

impl AppHeader {
    const SIZE: usize = 4 + 4 + 4 + 4 + 8 + 20 + 4 + 20; // 68 bytes

    fn read(cur: &mut Cursor<Vec<u8>>) -> io::Result<Self> {
        Ok(Self {
            appid: read_u32(cur)?,
            size: read_u32(cur)?,
            state: read_u32(cur)?,
            last_update: read_u32(cur)?,
            access_token: read_u64(cur)?,
            checksum_text: read_bytes20(cur)?,
            change_number: read_u32(cur)?,
            checksum_binary: read_bytes20(cur)?,
        })
    }

    fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.extend_from_slice(&self.appid.to_le_bytes());
        buf.extend_from_slice(&self.size.to_le_bytes());
        buf.extend_from_slice(&self.state.to_le_bytes());
        buf.extend_from_slice(&self.last_update.to_le_bytes());
        buf.extend_from_slice(&self.access_token.to_le_bytes());
        buf.extend_from_slice(&self.checksum_text);
        buf.extend_from_slice(&self.change_number.to_le_bytes());
        buf.extend_from_slice(&self.checksum_binary);
        buf
    }
}

// ── public app record ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AppRecord {
    pub header: AppHeader,
    pub sections: BTreeMap<String, Value>,
}

// ── error type ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum AppinfoError {
    Io(io::Error),
    IncompatibleVersion(u64),
    AppNotFound(u32),
}

impl std::fmt::Display for AppinfoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::IncompatibleVersion(v) => write!(f, "Unsupported VDF version: {v:#010x}"),
            Self::AppNotFound(id) => write!(f, "App {id} not found in appinfo.vdf"),
        }
    }
}

impl From<io::Error> for AppinfoError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

// ── main struct ───────────────────────────────────────────────────────────────

pub struct Appinfo {
    data: Vec<u8>,
    version: u64,
    /// v29 only: interned string pool
    string_pool: Vec<String>,
    /// Offset to the start of the string table (v29 only)
    string_table_offset: usize,
}

impl Appinfo {
    // ── construction ─────────────────────────────────────────────────────────

    pub fn from_bytes(data: Vec<u8>) -> Result<Self, AppinfoError> {
        let mut cur = Cursor::new(data.clone());

        let version = read_u64_raw(&mut cur)?;
        if version != APPINFO_28 && version != APPINFO_29 {
            return Err(AppinfoError::IncompatibleVersion(version));
        }

        let (string_pool, string_table_offset) = if version == APPINFO_29 {
            let table_offset = read_i64_raw(&mut cur)? as usize;
            let saved = cur.position() as usize;
            cur.set_position(table_offset as u64);
            let count = read_u32_raw(&mut cur)? as usize;
            let mut pool = Vec::with_capacity(count);
            for _ in 0..count {
                pool.push(read_cstring(&mut cur)?);
            }
            // restore
            cur.set_position(saved as u64);
            (pool, table_offset)
        } else {
            (Vec::new(), 0)
        };

        Ok(Self {
            data,
            version,
            string_pool,
            string_table_offset,
        })
    }

    // ── read a single app by ID ───────────────────────────────────────────────

    /// Read and return the record for `app_id`.
    pub fn read_app(&self, app_id: u32) -> Result<AppRecord, AppinfoError> {
        // Locate the app: the pattern is `\x08` + LE u32 appid
        let needle: Vec<u8> = std::iter::once(SECTION_END)
            .chain(app_id.to_le_bytes())
            .collect();

        let start = self
            .data
            .windows(5)
            .position(|w| w == needle.as_slice())
            .ok_or(AppinfoError::AppNotFound(app_id))?
            + 1; // skip the \x08

        let mut cur = Cursor::new(self.data.clone());
        cur.set_position(start as u64);

        let header = AppHeader::read(&mut cur)?;
        let sections = self.parse_subsections(&mut cur)?;

        Ok(AppRecord { header, sections })
    }

    // ── write a modified app back ─────────────────────────────────────────────

    /// Encode `record` and splice it back into `self.data`, replacing the old bytes.
    pub fn write_app(&mut self, record: &AppRecord) -> Result<(), AppinfoError> {
        let app_id = record.header.appid;

        // Locate original app
        let needle: Vec<u8> = std::iter::once(SECTION_END)
            .chain(app_id.to_le_bytes())
            .collect();
        let app_start = self
            .data
            .windows(5)
            .position(|w| w == needle.as_slice())
            .ok_or(AppinfoError::AppNotFound(app_id))?
            + 1;

        // Encode the sections
        let encoded_sections = self.encode_subsections(&record.sections);

        // Re-compute size (excludes appid + size fields themselves = 8 bytes)
        let old_header_encoded = record.header.encode();
        let new_size = (encoded_sections.len() + old_header_encoded.len() - 8) as u32;

        // Compute checksums
        let text_vdf = dict_to_text_vdf(&record.sections, 0);
        use sha1::{Digest, Sha1};
        let checksum_text: [u8; 20] = Sha1::digest(&text_vdf).into();
        let checksum_binary: [u8; 20] = Sha1::digest(&encoded_sections).into();

        let mut new_header = record.header.clone();
        new_header.size = new_size;
        new_header.checksum_text = checksum_text;
        new_header.checksum_binary = checksum_binary;

        let new_header_encoded = new_header.encode();

        // Find where the old record ends
        // old size is in the original header at app_start + 4 bytes (appid) + 4 bytes (size field)
        let old_size = u32::from_le_bytes(
            self.data[app_start + 4..app_start + 8]
                .try_into()
                .unwrap(),
        );
        let app_end = app_start + 8 + old_size as usize;

        // Splice
        let new_record_bytes: Vec<u8> = new_header_encoded
            .into_iter()
            .chain(encoded_sections)
            .collect();
        self.data
            .splice(app_start..app_end, new_record_bytes.into_iter());

        // Update string table offset for v29
        if self.version == APPINFO_29 {
            self.update_string_table_offset();
        }

        Ok(())
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    // ── internal parsing ──────────────────────────────────────────────────────

    fn parse_subsections(&self, cur: &mut Cursor<Vec<u8>>) -> Result<BTreeMap<String, Value>, AppinfoError> {
        let mut map = BTreeMap::new();
        loop {
            let type_tag = read_byte_raw(cur)?;
            if type_tag == SECTION_END {
                break;
            }

            let key = if self.version == APPINFO_29 {
                let idx = read_u32_raw(cur)? as usize;
                self.string_pool
                    .get(idx)
                    .cloned()
                    .unwrap_or_default()
            } else {
                read_cstring(cur)?
            };

            let value = match type_tag {
                TYPE_DICT => Value::Dict(self.parse_subsections(cur)?),
                TYPE_STRING => Value::String(read_cstring(cur)?),
                TYPE_INT32 => Value::Int32(read_u32_raw(cur)?),
                other => {
                    return Err(AppinfoError::Io(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("Unknown type tag: {other:#04x}"),
                    )))
                }
            };

            map.insert(key, value);
        }
        Ok(map)
    }

    // ── internal encoding ─────────────────────────────────────────────────────

    fn encode_subsections(&mut self, sections: &BTreeMap<String, Value>) -> Vec<u8> {
        let mut buf = Vec::new();
        for (key, value) in sections {
            let encoded_key = if self.version == APPINFO_29 {
                self.intern_key(key)
            } else {
                encode_cstring(key)
            };

            match value {
                Value::Dict(d) => {
                    buf.push(TYPE_DICT);
                    buf.extend_from_slice(&encoded_key);
                    buf.extend(self.encode_subsections(d));
                }
                Value::String(s) => {
                    buf.push(TYPE_STRING);
                    buf.extend_from_slice(&encoded_key);
                    buf.extend(encode_cstring(s));
                }
                Value::Int32(n) => {
                    buf.push(TYPE_INT32);
                    buf.extend_from_slice(&encoded_key);
                    buf.extend_from_slice(&n.to_le_bytes());
                }
            }
        }
        buf.push(SECTION_END);
        buf
    }

    /// For v29: look up or add `key` to the string pool and return its 4-byte LE index.
    fn intern_key(&mut self, key: &str) -> Vec<u8> {
        let idx = if let Some(pos) = self.string_pool.iter().position(|s| s == key) {
            pos
        } else {
            self.string_pool.push(key.to_owned());
            // Append to the raw data's string table as well
            let entry = encode_cstring(key);
            self.data.extend(entry);
            self.string_pool.len() - 1
        };
        (idx as u32).to_le_bytes().to_vec()
    }

    fn update_string_table_offset(&mut self) {
        // The string-table offset sits at bytes [8..16] of the file (right after the version u64).
        // Find the last app sentinel \x08\x00\x00\x00\x00, string table starts after it.
        let sentinel = [SECTION_END, 0x00, 0x00, 0x00, 0x00];
        if let Some(pos) = self
            .data
            .windows(5)
            .rposition(|w| w == sentinel)
        {
            let new_offset = pos + 5;
            let encoded = (new_offset as i64).to_le_bytes();
            self.data[8..16].copy_from_slice(&encoded);

            // Update the count at the new offset
            let count = self.string_pool.len() as u32;
            let count_bytes = count.to_le_bytes();
            if new_offset + 4 <= self.data.len() {
                self.data[new_offset..new_offset + 4].copy_from_slice(&count_bytes);
            }
        }
    }
}

// ── helper: deep path access on a Value tree ─────────────────────────────────

/// Walk `sections["appinfo"][p0][p1]...` and return a mutable reference to the
/// leaf `Value`, creating intermediate dicts as needed.
pub fn get_nested_mut<'a>(
    sections: &'a mut BTreeMap<String, Value>,
    path: &[&str],
) -> Option<&'a mut Value> {
    if path.is_empty() {
        return None;
    }
    let (head, tail) = path.split_first().unwrap();
    let node = sections.get_mut(*head)?;
    if tail.is_empty() {
        Some(node)
    } else {
        get_nested_mut(node.as_dict_mut()?, tail)
    }
}

/// Read the value at `path` inside `sections`, returning `None` if any key
/// is missing.
pub fn get_nested<'a>(sections: &'a BTreeMap<String, Value>, path: &[&str]) -> Option<&'a Value> {
    if path.is_empty() {
        return None;
    }
    let (head, tail) = path.split_first().unwrap();
    let node = sections.get(*head)?;
    if tail.is_empty() {
        Some(node)
    } else {
        get_nested(node.as_dict()?, tail)
    }
}

/// Set `value` at `path`, creating intermediate dicts as needed.
pub fn set_nested(sections: &mut BTreeMap<String, Value>, path: &[&str], value: Value) {
    if path.is_empty() {
        return;
    }
    if path.len() == 1 {
        sections.insert(path[0].to_owned(), value);
        return;
    }
    let (head, tail) = path.split_first().unwrap();
    let entry = sections
        .entry(head.to_string())
        .or_insert_with(|| Value::Dict(BTreeMap::new()));
    if let Value::Dict(d) = entry {
        set_nested(d, tail, value);
    }
}

// ── text VDF serialiser (for checksum) ───────────────────────────────────────

pub fn dict_to_text_vdf(data: &BTreeMap<String, Value>, depth: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let tabs = "\t".repeat(depth);
    for (key, value) in data {
        match value {
            Value::Dict(d) => {
                let inner = dict_to_text_vdf(d, depth + 1);
                out.extend(format!("{tabs}\"{key}\"\n{tabs}{{\n").into_bytes());
                out.extend(inner);
                out.extend(format!("{tabs}}}\n").into_bytes());
            }
            Value::String(s) => {
                out.extend(format!("{tabs}\"{key}\"\t\t\"{s}\"\n").into_bytes());
            }
            Value::Int32(n) => {
                out.extend(format!("{tabs}\"{key}\"\t\t\"{n}\"\n").into_bytes());
            }
        }
    }
    out
}

// ── low-level cursor helpers ──────────────────────────────────────────────────

fn read_byte_raw(cur: &mut Cursor<Vec<u8>>) -> io::Result<u8> {
    let mut b = [0u8; 1];
    cur.read_exact(&mut b)?;
    Ok(b[0])
}

fn read_u32_raw(cur: &mut Cursor<Vec<u8>>) -> io::Result<u32> {
    let mut b = [0u8; 4];
    cur.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u32(cur: &mut Cursor<Vec<u8>>) -> io::Result<u32> {
    read_u32_raw(cur)
}

fn read_u64_raw(cur: &mut Cursor<Vec<u8>>) -> io::Result<u64> {
    let mut b = [0u8; 8];
    cur.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn read_u64(cur: &mut Cursor<Vec<u8>>) -> io::Result<u64> {
    read_u64_raw(cur)
}

fn read_i64_raw(cur: &mut Cursor<Vec<u8>>) -> io::Result<i64> {
    let mut b = [0u8; 8];
    cur.read_exact(&mut b)?;
    Ok(i64::from_le_bytes(b))
}

fn read_bytes20(cur: &mut Cursor<Vec<u8>>) -> io::Result<[u8; 20]> {
    let mut b = [0u8; 20];
    cur.read_exact(&mut b)?;
    Ok(b)
}

/// Read a null-terminated UTF-8 (fallback latin-1) string.
fn read_cstring(cur: &mut Cursor<Vec<u8>>) -> io::Result<String> {
    let pos = cur.position() as usize;
    let end = {
        let data = cur.get_ref();
        data[pos..]
            .iter()
            .position(|&b| b == 0)
            .map(|i| pos + i)
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "unterminated string"))?
    };
    // Copy the bytes out before mutably borrowing cur again
    let bytes: Vec<u8> = cur.get_ref()[pos..end].to_vec();
    cur.set_position((end + 1) as u64);
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn encode_cstring(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}