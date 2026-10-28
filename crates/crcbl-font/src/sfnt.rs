//! The OpenType file structure, as far as finding a face's tables goes: the
//! collection header, each face's table directory, and writing a directory
//! back out around a set of tables.
//!
//! Three jobs share it. [`describe_tables`] reads, from a file on disk, only
//! the few tables a face is matched by, seeking to each rather than reading
//! the file: a CJK collection is tens of megabytes, and its name table a few
//! kilobytes of that. [`face_cmap`] reads one face's `cmap` the same way,
//! which is all that answers whether the face maps a char. [`standalone`] cuts one face out of a collection as a
//! font file of its own, which is what a parser that reads one font per file
//! — `crcbl-ui`'s `Font::parse` — takes.
//!
//! Both write the tables they keep into a new file through [`build`], as the
//! OpenType specification's "Organization of an OpenType Font" lays one out:
//! the offset table, the table records sorted by tag, then each table's bytes
//! padded to a four-byte boundary. Each record keeps the checksum its source
//! directory gave.

use std::io::{self, Read, Seek, SeekFrom};

use skrifa::raw::FileRef;

/// A collection's header tag, `ttcf`.
const COLLECTION_TAG: [u8; 4] = *b"ttcf";

/// The versions a face's offset table opens with: TrueType outlines, CFF
/// outlines, and Apple's TrueType tag.
const SFNT_VERSIONS: [[u8; 4]; 3] = [[0, 1, 0, 0], *b"OTTO", *b"true"];

/// The tables [`crate::face::describe`] reads: the family names, and the
/// three `skrifa`'s attributes come from.
const DESCRIBE_TABLES: [[u8; 4]; 4] = [*b"OS/2", *b"head", *b"name", *b"post"];

/// The table [`face_cmap`] reads.
const CMAP_TABLE: [u8; 4] = *b"cmap";

/// The offset table: the version, the table count and the three search
/// fields.
const OFFSET_TABLE_LEN: u32 = 12;

/// One table record: tag, checksum, offset and length.
const RECORD_LEN: u32 = 16;

/// One entry of a table directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Record {
    tag: [u8; 4],
    checksum: u32,
    offset: u32,
    length: u32,
}

/// Each face of the font file in `file`, as a font file of its own holding
/// only the tables matching reads, in the file's face order. A face whose
/// tables cannot be read is `None` in its place, so the faces after it keep
/// their collection index.
///
/// # Errors
///
/// The file opens with neither a collection header nor a face's offset
/// table, or its face count runs past its end: it is not a font file.
pub(crate) fn describe_tables<R: Read + Seek>(file: &mut R) -> io::Result<Vec<Option<Vec<u8>>>> {
    let len = file.seek(SeekFrom::End(0))?;
    Ok(directories(file, len)?
        .into_iter()
        .map(|offset| face_tables(file, offset, len, &DESCRIBE_TABLES).ok())
        .collect())
}

/// The face at `index` of the font file in `file`, as a font file of its own
/// holding only its `cmap`, or no table at all when it has none.
///
/// # Errors
///
/// [`describe_tables`]'s, and [`io::ErrorKind::InvalidData`] when the file
/// has no face at `index` or the face's directory or `cmap` runs past its
/// end.
pub(crate) fn face_cmap<R: Read + Seek>(file: &mut R, index: u32) -> io::Result<Vec<u8>> {
    let len = file.seek(SeekFrom::End(0))?;
    let offset = *directories(file, len)?
        .get(usize::try_from(index).map_err(invalid)?)
        .ok_or_else(|| invalid(format!("the file has no face {index}")))?;
    face_tables(file, offset, len, &[CMAP_TABLE])
}

/// Where each face's table directory starts, in the file's face order.
fn directories<R: Read + Seek>(file: &mut R, len: u64) -> io::Result<Vec<u32>> {
    let header: [u8; 12] = read_at(file, 0, len)?;
    let directories = if header[..4] == COLLECTION_TAG {
        let count = be_u32(&header[8..]);
        let offsets = read_vec(file, u64::from(OFFSET_TABLE_LEN), u64::from(count) * 4, len)?;
        offsets
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&bytes| u32::from_be_bytes(bytes))
            .collect()
    } else if SFNT_VERSIONS.contains(&[header[0], header[1], header[2], header[3]]) {
        vec![0]
    } else {
        return Err(invalid("neither a font nor a collection"));
    };
    Ok(directories)
}

/// The face whose table directory is at `offset`, rebuilt with only the
/// tables `wanted` names that it has.
fn face_tables<R: Read + Seek>(
    file: &mut R,
    offset: u32,
    len: u64,
    wanted: &[[u8; 4]],
) -> io::Result<Vec<u8>> {
    let header: [u8; 12] = read_at(file, u64::from(offset), len)?;
    let version = [header[0], header[1], header[2], header[3]];
    if !SFNT_VERSIONS.contains(&version) {
        return Err(invalid("a face's offset table has no font version"));
    }
    let count = u64::from(u16::from_be_bytes([header[4], header[5]]));
    let directory = read_vec(
        file,
        u64::from(offset) + u64::from(OFFSET_TABLE_LEN),
        count * u64::from(RECORD_LEN),
        len,
    )?;
    let mut tables = Vec::new();
    for record in directory
        .as_chunks::<{ RECORD_LEN as usize }>()
        .0
        .iter()
        .map(parse_record)
    {
        if wanted.contains(&record.tag) {
            let data = read_vec(
                file,
                u64::from(record.offset),
                u64::from(record.length),
                len,
            )?;
            tables.push((record, data));
        }
    }
    build(
        u32::from_be_bytes(version),
        tables
            .iter()
            .map(|(record, data)| (record.tag, record.checksum, data.as_slice())),
    )
}

/// The face at `index` of the font file `data`, as a font file of its own: a
/// collection's face rebuilt with every table it names, a single font
/// returned as it is.
///
/// # Errors
///
/// [`io::ErrorKind::InvalidData`] when `data` is not a font file, has no face
/// at `index` (a single font has only index 0), or names a table outside it.
pub(crate) fn standalone(data: Vec<u8>, index: u32) -> io::Result<Vec<u8>> {
    let font = match FileRef::new(&data).map_err(invalid)? {
        FileRef::Font(_) if index == 0 => return Ok(data),
        FileRef::Font(_) => {
            return Err(invalid(format!(
                "face {index} asked of a font file that is not a collection"
            )));
        }
        FileRef::Collection(collection) => collection.get(index).map_err(invalid)?,
    };
    let directory = font.table_directory();
    let tables = directory
        .table_records()
        .iter()
        .map(|record| {
            let tag = record.tag();
            let bytes = font
                .table_data(tag)
                .ok_or_else(|| invalid(format!("the {tag} table is outside the file")))?;
            Ok((tag.to_be_bytes(), record.checksum(), bytes.as_bytes()))
        })
        .collect::<io::Result<Vec<_>>>()?;
    build(directory.sfnt_version(), tables)
}

/// A font file of `version` holding `tables`, each `(tag, checksum, bytes)`.
///
/// # Errors
///
/// [`io::ErrorKind::InvalidData`] when the tables do not fit the format's
/// 16-bit table count and search fields or its 32-bit offsets.
pub(crate) fn build<'a>(
    version: u32,
    tables: impl IntoIterator<Item = ([u8; 4], u32, &'a [u8])>,
) -> io::Result<Vec<u8>> {
    let mut tables: Vec<_> = tables.into_iter().collect();
    tables.sort_by_key(|&(tag, ..)| tag);
    let count = u16::try_from(tables.len()).map_err(invalid)?;
    // The search fields are the binary-search hints the specification
    // defines: the largest power of two not above the count, scaled to
    // records, its exponent, and the remainder.
    let entry_selector = count.checked_ilog2().unwrap_or(0);
    let search_range = (1u32 << entry_selector) * RECORD_LEN;
    let range_shift = (u32::from(count) * RECORD_LEN).saturating_sub(search_range);

    let mut out = Vec::new();
    out.extend_from_slice(&version.to_be_bytes());
    out.extend_from_slice(&count.to_be_bytes());
    for field in [search_range, entry_selector, range_shift] {
        out.extend_from_slice(&u16::try_from(field).map_err(invalid)?.to_be_bytes());
    }
    let mut offset = OFFSET_TABLE_LEN + u32::from(count) * RECORD_LEN;
    for &(tag, checksum, bytes) in &tables {
        let length = u32::try_from(bytes.len()).map_err(invalid)?;
        out.extend_from_slice(&tag);
        out.extend_from_slice(&checksum.to_be_bytes());
        out.extend_from_slice(&offset.to_be_bytes());
        out.extend_from_slice(&length.to_be_bytes());
        offset = length
            .checked_next_multiple_of(4)
            .and_then(|padded| offset.checked_add(padded))
            .ok_or_else(|| invalid("the tables pass the format's 32-bit offsets"))?;
    }
    for &(_, _, bytes) in &tables {
        out.extend_from_slice(bytes);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    Ok(out)
}

fn parse_record(bytes: &[u8; RECORD_LEN as usize]) -> Record {
    Record {
        tag: [bytes[0], bytes[1], bytes[2], bytes[3]],
        checksum: be_u32(&bytes[4..]),
        offset: be_u32(&bytes[8..]),
        length: be_u32(&bytes[12..]),
    }
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// `N` bytes at `offset` of a file `len` long.
fn read_at<const N: usize, R: Read + Seek>(
    file: &mut R,
    offset: u64,
    len: u64,
) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    check_span(offset, N as u64, len)?;
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// `count` bytes at `offset` of a file `len` long, refused before anything
/// is allocated when the span runs past the end: a corrupt directory names
/// lengths the file does not have.
fn read_vec<R: Read + Seek>(
    file: &mut R,
    offset: u64,
    count: u64,
    len: u64,
) -> io::Result<Vec<u8>> {
    check_span(offset, count, len)?;
    let mut bytes = vec![0; usize::try_from(count).map_err(invalid)?];
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn check_span(offset: u64, count: u64, len: u64) -> io::Result<()> {
    match offset.checked_add(count) {
        Some(end) if end <= len => Ok(()),
        _ => Err(invalid(
            "a table or directory runs past the end of the file",
        )),
    }
}

fn invalid(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Cursor;

    use skrifa::{FontRef, MetadataProvider, Tag};

    use super::*;

    pub(crate) const SANS: &[u8] =
        include_bytes!("../../crcbl-ui/fonts/AtkinsonHyperlegible-Regular.ttf");
    pub(crate) const EMOJI_TEXT: &[u8] =
        include_bytes!("../../crcbl-ui/tests/fixtures/emoji/text.ttf");

    /// A collection of `fonts`, each face's tables copied after the header
    /// and its directory pointing at them: the inverse of [`standalone`].
    pub(crate) fn collection(fonts: &[&[u8]]) -> Vec<u8> {
        let header_len = 12 + 4 * fonts.len();
        let mut out = Vec::new();
        out.extend_from_slice(&COLLECTION_TAG);
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&u32::try_from(fonts.len()).unwrap().to_be_bytes());
        out.resize(header_len, 0);
        for (at, font) in fonts.iter().enumerate() {
            let directory = out.len();
            out[12 + 4 * at..16 + 4 * at]
                .copy_from_slice(&u32::try_from(directory).unwrap().to_be_bytes());
            // The face's own file, rebased: its records' offsets move by
            // where it now starts.
            let rebased = u32::try_from(directory).unwrap();
            let count = usize::from(u16::from_be_bytes([font[4], font[5]]));
            let mut face = font.to_vec();
            for record in 0..count {
                let at = 12 + 16 * record + 8;
                let offset = be_u32(&face[at..]) + rebased;
                face[at..at + 4].copy_from_slice(&offset.to_be_bytes());
            }
            out.extend_from_slice(&face);
        }
        out
    }

    /// Each table of `font` by tag, as bytes.
    fn tables(font: &FontRef<'_>) -> Vec<([u8; 4], Vec<u8>)> {
        font.table_directory()
            .table_records()
            .iter()
            .map(|record| {
                let tag = record.tag();
                (
                    tag.to_be_bytes(),
                    font.table_data(tag).unwrap().as_bytes().to_vec(),
                )
            })
            .collect()
    }

    #[test]
    fn a_single_font_is_its_own_standalone_face() {
        assert_eq!(standalone(SANS.to_vec(), 0).unwrap(), SANS);
        let error = standalone(SANS.to_vec(), 1).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn a_collection_face_comes_out_with_every_table_it_had() {
        let ttc = collection(&[SANS, EMOJI_TEXT]);
        for (index, original) in [SANS, EMOJI_TEXT].into_iter().enumerate() {
            let face = standalone(ttc.clone(), index as u32).unwrap();
            let face = FontRef::new(&face).expect("a standalone face parses alone");
            assert_eq!(tables(&face), tables(&FontRef::new(original).unwrap()));
        }
        assert!(standalone(ttc, 2).is_err());
        assert!(standalone(b"not a font".to_vec(), 0).is_err());
    }

    #[test]
    fn the_directory_written_is_the_specifications() {
        let font = FontRef::new(SANS).unwrap();
        let count = font.table_directory().num_tables();
        let built = build(
            font.table_directory().sfnt_version(),
            tables(&font)
                .iter()
                .map(|(tag, bytes)| (*tag, 0, bytes.as_slice())),
        )
        .unwrap();
        let rebuilt = FontRef::new(&built).unwrap();
        let directory = rebuilt.table_directory();
        assert_eq!(directory.num_tables(), count);
        let selector = count.ilog2();
        assert_eq!(u32::from(directory.entry_selector()), selector);
        assert_eq!(u32::from(directory.search_range()), (1 << selector) * 16);
        assert_eq!(
            u32::from(directory.range_shift()),
            u32::from(count) * 16 - (1 << selector) * 16
        );
        let records = directory.table_records();
        assert!(records.windows(2).all(|pair| pair[0].tag() < pair[1].tag()));
        assert!(records.iter().all(|record| record.offset() % 4 == 0));
        assert_eq!(tables(&rebuilt), tables(&font));
    }

    #[test]
    fn describe_tables_keeps_only_what_matching_reads_and_every_face() {
        let ttc = collection(&[EMOJI_TEXT, SANS]);
        let faces = describe_tables(&mut Cursor::new(&ttc)).unwrap();
        assert_eq!(faces.len(), 2);
        for (face, original) in faces.iter().zip([EMOJI_TEXT, SANS]) {
            let face = FontRef::new(face.as_ref().expect("readable")).unwrap();
            let kept: Vec<_> = tables(&face).into_iter().map(|(tag, _)| tag).collect();
            let mut wanted: Vec<_> = tables(&FontRef::new(original).unwrap())
                .into_iter()
                .map(|(tag, _)| tag)
                .filter(|tag| DESCRIBE_TABLES.contains(tag))
                .collect();
            wanted.sort_unstable();
            assert_eq!(kept, wanted);
            assert_eq!(wanted.len(), DESCRIBE_TABLES.len());
        }
        let single = describe_tables(&mut Cursor::new(SANS)).unwrap();
        assert_eq!(single.len(), 1);
    }

    /// A reader that counts the bytes read through it.
    struct Counting<R> {
        inner: R,
        read: u64,
    }

    impl<R: Read> Read for Counting<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let read = self.inner.read(buf)?;
            self.read += read as u64;
            Ok(read)
        }
    }

    impl<R: Seek> Seek for Counting<R> {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.inner.seek(pos)
        }
    }

    /// **A face's `cmap` comes out alone and whole, read past the rest of
    /// the file**: each face of a collection maps exactly the chars its
    /// original does, and the bytes read are its `cmap` and the directories
    /// that lead to it.
    #[test]
    fn face_cmap_reads_only_the_faces_cmap() {
        let ttc = collection(&[SANS, EMOJI_TEXT]);
        for (index, original) in [SANS, EMOJI_TEXT].into_iter().enumerate() {
            let mut file = Counting {
                inner: Cursor::new(&ttc),
                read: 0,
            };
            let cmap = face_cmap(&mut file, index as u32).unwrap();
            let original = FontRef::new(original).unwrap();
            let cmap_len = original.table_data(Tag::new(&CMAP_TABLE)).unwrap().len() as u64;
            let directories = 12
                + 4 * 2
                + u64::from(OFFSET_TABLE_LEN)
                + u64::from(RECORD_LEN) * u64::from(original.table_directory().num_tables());
            assert_eq!(file.read, directories + cmap_len);
            let cut = FontRef::new(&cmap).expect("a cmap-only face parses");
            let kept: Vec<_> = tables(&cut).into_iter().map(|(tag, _)| tag).collect();
            assert_eq!(kept, [CMAP_TABLE]);
            let (cut, original) = (cut.charmap(), original.charmap());
            for c in (0..0x3_0000).filter_map(char::from_u32) {
                assert_eq!(cut.map(c), original.map(c), "{c:?} on face {index}");
            }
        }
        let error = face_cmap(&mut Cursor::new(&ttc), 2).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let single = face_cmap(&mut Cursor::new(SANS), 0).unwrap();
        assert!(FontRef::new(&single).unwrap().charmap().map('a').is_some());
        assert!(face_cmap(&mut Cursor::new(SANS), 1).is_err());
    }

    #[test]
    fn a_directory_past_the_end_of_the_file_is_refused_or_skipped() {
        // A header alone: too short for anything.
        assert!(describe_tables(&mut Cursor::new(b"ttcf")).is_err());
        // Long enough, but neither a collection nor a font.
        assert!(describe_tables(&mut Cursor::new(b"not a font at all")).is_err());
        // A collection claiming more faces than the file holds offsets for.
        let mut lying = collection(&[SANS]);
        lying[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(describe_tables(&mut Cursor::new(&lying)).is_err());
        // A face whose name table runs past the end: that face is skipped,
        // and the next keeps its index.
        let mut ttc = collection(&[SANS, EMOJI_TEXT]);
        let first = be_u32(&ttc[12..]) as usize;
        let count = usize::from(u16::from_be_bytes([ttc[first + 4], ttc[first + 5]]));
        let name = (0..count)
            .map(|record| first + 12 + 16 * record)
            .find(|&at| &ttc[at..at + 4] == b"name")
            .unwrap();
        ttc[name + 12..name + 16].copy_from_slice(&u32::MAX.to_be_bytes());
        let faces = describe_tables(&mut Cursor::new(&ttc)).unwrap();
        assert!(faces[0].is_none());
        assert!(faces[1].is_some());
    }
}
