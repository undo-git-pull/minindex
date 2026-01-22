use memmap2::Mmap;
use std::error::Error;
use std::ops::Range;

#[derive(Debug)]
pub(crate) enum Backing {
    Mmap(Mmap),
    Bytes(Vec<u8>),
}

pub(crate) const MAGIC: &[u8; 4] = b"MIDX";
pub(crate) const VERSION: u16 = 1;
pub(crate) const HEADER_SIZE: usize = 240;

#[derive(Debug)]
pub(crate) struct IndexHeader {
    pub(crate) field_count: u16,
    pub(crate) indexed_field_count: u16,
    pub(crate) index_fields_offset: u64,
    pub(crate) index_fields_len: u64,
    pub(crate) doc_id_field_offset: u64,
    pub(crate) doc_id_field_len: u64,
    pub(crate) fst_offset: u64,
    pub(crate) fst_len: u64,
    pub(crate) keyword_offsets_offset: u64,
    pub(crate) keyword_offsets_len: u64,
    pub(crate) postings_doc_indices_offset: u64,
    pub(crate) postings_doc_indices_len: u64,
    pub(crate) postings_scores_offset: u64,
    pub(crate) postings_scores_len: u64,
    pub(crate) doc_ids_dict_syms_offset: u64,
    pub(crate) doc_ids_dict_syms_len: u64,
    pub(crate) doc_ids_dict_lens_offset: u64,
    pub(crate) doc_ids_dict_lens_len: u64,
    pub(crate) doc_ids_offsets_offset: u64,
    pub(crate) doc_ids_offsets_len: u64,
    pub(crate) doc_ids_data_offset: u64,
    pub(crate) doc_ids_data_len: u64,
    pub(crate) doc_fields_dict_syms_offset: u64,
    pub(crate) doc_fields_dict_syms_len: u64,
    pub(crate) doc_fields_dict_lens_offset: u64,
    pub(crate) doc_fields_dict_lens_len: u64,
    pub(crate) doc_fields_offsets_offset: u64,
    pub(crate) doc_fields_offsets_len: u64,
    pub(crate) doc_fields_data_offset: u64,
    pub(crate) doc_fields_data_len: u64,
}

impl Backing {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        match self {
            Backing::Mmap(mmap) => mmap,
            Backing::Bytes(bytes) => bytes,
        }
    }

    pub(crate) fn slice(&self, range: Range<usize>) -> &[u8] {
        &self.as_bytes()[range]
    }

    pub(crate) fn len(&self) -> usize {
        self.as_bytes().len()
    }
}

impl IndexHeader {
    pub(crate) fn read(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        if bytes.len() < HEADER_SIZE {
            return Err("Index header is truncated".into());
        }
        if &bytes[0..4] != MAGIC {
            return Err("Index file magic mismatch".into());
        }

        let mut offset = 4;
        let version = read_u16(bytes, &mut offset)?;
        if version != VERSION {
            return Err(format!("Unsupported index version: {version}").into());
        }

        let _flags = read_u16(bytes, &mut offset)?;
        let field_count = read_u16(bytes, &mut offset)?;
        let indexed_field_count = read_u16(bytes, &mut offset)?;
        let header_size = read_u32(bytes, &mut offset)?;
        if header_size as usize != HEADER_SIZE {
            return Err("Index header size mismatch".into());
        }

        Ok(IndexHeader {
            field_count,
            indexed_field_count,
            index_fields_offset: read_u64(bytes, &mut offset)?,
            index_fields_len: read_u64(bytes, &mut offset)?,
            doc_id_field_offset: read_u64(bytes, &mut offset)?,
            doc_id_field_len: read_u64(bytes, &mut offset)?,
            fst_offset: read_u64(bytes, &mut offset)?,
            fst_len: read_u64(bytes, &mut offset)?,
            keyword_offsets_offset: read_u64(bytes, &mut offset)?,
            keyword_offsets_len: read_u64(bytes, &mut offset)?,
            postings_doc_indices_offset: read_u64(bytes, &mut offset)?,
            postings_doc_indices_len: read_u64(bytes, &mut offset)?,
            postings_scores_offset: read_u64(bytes, &mut offset)?,
            postings_scores_len: read_u64(bytes, &mut offset)?,
            doc_ids_dict_syms_offset: read_u64(bytes, &mut offset)?,
            doc_ids_dict_syms_len: read_u64(bytes, &mut offset)?,
            doc_ids_dict_lens_offset: read_u64(bytes, &mut offset)?,
            doc_ids_dict_lens_len: read_u64(bytes, &mut offset)?,
            doc_ids_offsets_offset: read_u64(bytes, &mut offset)?,
            doc_ids_offsets_len: read_u64(bytes, &mut offset)?,
            doc_ids_data_offset: read_u64(bytes, &mut offset)?,
            doc_ids_data_len: read_u64(bytes, &mut offset)?,
            doc_fields_dict_syms_offset: read_u64(bytes, &mut offset)?,
            doc_fields_dict_syms_len: read_u64(bytes, &mut offset)?,
            doc_fields_dict_lens_offset: read_u64(bytes, &mut offset)?,
            doc_fields_dict_lens_len: read_u64(bytes, &mut offset)?,
            doc_fields_offsets_offset: read_u64(bytes, &mut offset)?,
            doc_fields_offsets_len: read_u64(bytes, &mut offset)?,
            doc_fields_data_offset: read_u64(bytes, &mut offset)?,
            doc_fields_data_len: read_u64(bytes, &mut offset)?,
        })
    }

    pub(crate) fn write(&self, bytes: &mut [u8]) -> Result<(), Box<dyn Error>> {
        if bytes.len() < HEADER_SIZE {
            return Err("Index header buffer is too small".into());
        }

        bytes[0..4].copy_from_slice(MAGIC);
        let mut offset = 4;
        write_u16(bytes, &mut offset, VERSION);
        write_u16(bytes, &mut offset, 0);
        write_u16(bytes, &mut offset, self.field_count);
        write_u16(bytes, &mut offset, self.indexed_field_count);
        write_u32(bytes, &mut offset, HEADER_SIZE as u32);

        write_u64(bytes, &mut offset, self.index_fields_offset);
        write_u64(bytes, &mut offset, self.index_fields_len);
        write_u64(bytes, &mut offset, self.doc_id_field_offset);
        write_u64(bytes, &mut offset, self.doc_id_field_len);
        write_u64(bytes, &mut offset, self.fst_offset);
        write_u64(bytes, &mut offset, self.fst_len);
        write_u64(bytes, &mut offset, self.keyword_offsets_offset);
        write_u64(bytes, &mut offset, self.keyword_offsets_len);
        write_u64(bytes, &mut offset, self.postings_doc_indices_offset);
        write_u64(bytes, &mut offset, self.postings_doc_indices_len);
        write_u64(bytes, &mut offset, self.postings_scores_offset);
        write_u64(bytes, &mut offset, self.postings_scores_len);
        write_u64(bytes, &mut offset, self.doc_ids_dict_syms_offset);
        write_u64(bytes, &mut offset, self.doc_ids_dict_syms_len);
        write_u64(bytes, &mut offset, self.doc_ids_dict_lens_offset);
        write_u64(bytes, &mut offset, self.doc_ids_dict_lens_len);
        write_u64(bytes, &mut offset, self.doc_ids_offsets_offset);
        write_u64(bytes, &mut offset, self.doc_ids_offsets_len);
        write_u64(bytes, &mut offset, self.doc_ids_data_offset);
        write_u64(bytes, &mut offset, self.doc_ids_data_len);
        write_u64(bytes, &mut offset, self.doc_fields_dict_syms_offset);
        write_u64(bytes, &mut offset, self.doc_fields_dict_syms_len);
        write_u64(bytes, &mut offset, self.doc_fields_dict_lens_offset);
        write_u64(bytes, &mut offset, self.doc_fields_dict_lens_len);
        write_u64(bytes, &mut offset, self.doc_fields_offsets_offset);
        write_u64(bytes, &mut offset, self.doc_fields_offsets_len);
        write_u64(bytes, &mut offset, self.doc_fields_data_offset);
        write_u64(bytes, &mut offset, self.doc_fields_data_len);

        Ok(())
    }
}

#[derive(Debug, Default)]
pub(crate) struct SectionOffsets {
    pub(crate) index_fields: Section,
    pub(crate) doc_id_field: Section,
    pub(crate) fst: Section,
    pub(crate) keyword_offsets: Section,
    pub(crate) postings_doc_indices: Section,
    pub(crate) postings_scores: Section,
    pub(crate) doc_ids_dict_syms: Section,
    pub(crate) doc_ids_dict_lens: Section,
    pub(crate) doc_ids_offsets: Section,
    pub(crate) doc_ids_data: Section,
    pub(crate) doc_fields_dict_syms: Section,
    pub(crate) doc_fields_dict_lens: Section,
    pub(crate) doc_fields_offsets: Section,
    pub(crate) doc_fields_data: Section,
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Section {
    pub(crate) offset: u64,
    pub(crate) len: u64,
}

pub(crate) fn range_from(offset: u64, len: u64) -> Result<Range<usize>, Box<dyn Error>> {
    let start = usize::try_from(offset)?;
    let end = start
        .checked_add(usize::try_from(len)?)
        .ok_or_else(|| "Index section range overflow".to_string())?;
    Ok(start..end)
}

pub(crate) fn append_section(buffer: &mut Vec<u8>, data: &[u8], alignment: usize) -> Section {
    let aligned = align_up(buffer.len(), alignment);
    if aligned > buffer.len() {
        buffer.resize(aligned, 0);
    }
    let offset = buffer.len() as u64;
    buffer.extend_from_slice(data);
    Section {
        offset,
        len: data.len() as u64,
    }
}

pub(crate) fn append_u32_section(
    buffer: &mut Vec<u8>,
    data: &[u32],
    alignment: usize,
) -> Section {
    let aligned = align_up(buffer.len(), alignment);
    if aligned > buffer.len() {
        buffer.resize(aligned, 0);
    }
    let offset = buffer.len() as u64;
    for value in data {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    Section {
        offset,
        len: (data.len() * 4) as u64,
    }
}

pub(crate) fn encode_string_table(strings: &[String]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(strings.len() as u32).to_le_bytes());
    for value in strings {
        let raw = value.as_bytes();
        bytes.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        bytes.extend_from_slice(raw);
    }
    bytes
}

pub(crate) fn parse_string_table(bytes: &[u8]) -> Result<Vec<String>, Box<dyn Error>> {
    let mut offset = 0;
    let count = read_u32(bytes, &mut offset)? as usize;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let len = read_u32(bytes, &mut offset)? as usize;
        let end = offset + len;
        if end > bytes.len() {
            return Err("String table is truncated".into());
        }
        let raw = bytes[offset..end].to_vec();
        offset = end;
        values.push(String::from_utf8(raw)?);
    }
    Ok(values)
}

pub(crate) fn flatten_dict_syms(dict_syms: &[[u8; 8]]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(dict_syms.len() * 8);
    for sym in dict_syms {
        bytes.extend_from_slice(sym);
    }
    bytes
}

pub(crate) fn build_postings(
    keyword_to_documents: &[Vec<(usize, u8)>],
) -> Result<(Vec<u32>, Vec<u32>, Vec<u8>), Box<dyn Error>> {
    let mut offsets = Vec::with_capacity(keyword_to_documents.len() + 1);
    let mut doc_indices = Vec::new();
    let mut scores = Vec::new();

    offsets.push(0);
    for docs in keyword_to_documents {
        for (doc_index, score) in docs {
            let doc_index = u32::try_from(*doc_index)?;
            doc_indices.push(doc_index);
            scores.push(*score);
        }
        offsets.push(doc_indices.len() as u32);
    }

    Ok((offsets, doc_indices, scores))
}

pub(crate) fn u32_slice(bytes: &[u8]) -> Result<&[u32], Box<dyn Error>> {
    if bytes.len() % 4 != 0 {
        return Err("Index section length is not aligned to u32".into());
    }
    let (prefix, middle, suffix) = unsafe { bytes.align_to::<u32>() };
    if !prefix.is_empty() || !suffix.is_empty() {
        return Err("Index section alignment is invalid for u32".into());
    }
    Ok(middle)
}

pub(crate) fn checked_range(
    backing_len: usize,
    offset: u64,
    len: u64,
) -> Result<Range<usize>, Box<dyn Error>> {
    let range = range_from(offset, len)?;
    if range.end > backing_len {
        return Err("Index section is out of bounds".into());
    }
    Ok(range)
}

#[derive(Debug)]
pub(crate) struct FsstStrVecView {
    dict_lens_range: Range<usize>,
    offsets_range: Range<usize>,
    data_range: Range<usize>,
    syms: Vec<fsst::Symbol>,
    offsets_len: usize,
    data_len: usize,
}

impl FsstStrVecView {
    pub(crate) fn new(
        backing: &Backing,
        dict_syms_range: Range<usize>,
        dict_lens_range: Range<usize>,
        offsets_range: Range<usize>,
        data_range: Range<usize>,
    ) -> Result<Self, Box<dyn Error>> {
        let dict_syms_bytes = backing.slice(dict_syms_range);
        if dict_syms_bytes.len() % 8 != 0 {
            return Err("FSST dictionary symbols are misaligned".into());
        }
        let mut syms = Vec::with_capacity(dict_syms_bytes.len() / 8);
        for chunk in dict_syms_bytes.chunks_exact(8) {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(chunk);
            syms.push(fsst::Symbol::from_slice(&bytes));
        }

        let dict_lens_bytes = backing.slice(dict_lens_range.clone());
        if dict_lens_bytes.len() != syms.len() {
            return Err("FSST dictionary length mismatch".into());
        }

        let offsets_bytes = backing.slice(offsets_range.clone());
        let offsets_len = u32_slice(offsets_bytes)?.len();
        let data_len = data_range.len();

        Ok(FsstStrVecView {
            dict_lens_range,
            offsets_range,
            data_range,
            syms,
            offsets_len,
            data_len,
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.offsets_len
    }

    pub(crate) fn get(&self, backing: &Backing, index: usize) -> Option<String> {
        if index >= self.offsets_len {
            return None;
        }
        let offsets_bytes = backing.slice(self.offsets_range.clone());
        let offsets = u32_slice(offsets_bytes).ok()?;
        let start = offsets[index] as usize;
        let end = if index + 1 < offsets.len() {
            offsets[index + 1] as usize
        } else {
            self.data_len
        };
        if end < start || end > self.data_len {
            return None;
        }
        let data = backing.slice(self.data_range.clone());
        let codes = &data[start..end];

        let dict_lens = backing.slice(self.dict_lens_range.clone());
        let decomp = fsst::Decompressor::new(&self.syms, dict_lens);
        let bytes = decomp.decompress(codes);
        String::from_utf8(bytes).ok()
    }
}

#[derive(Debug)]
pub(crate) struct MmapIndexData {
    backing: Backing,
    fst_range: Range<usize>,
    keyword_offsets_range: Range<usize>,
    postings_doc_indices_range: Range<usize>,
    postings_scores_range: Range<usize>,
    doc_ids: FsstStrVecView,
    doc_fields: FsstStrVecView,
}

impl MmapIndexData {
    pub(crate) fn new(backing: Backing, header: &IndexHeader) -> Result<Self, Box<dyn Error>> {
        let backing_len = backing.len();
        let fst_range = checked_range(backing_len, header.fst_offset, header.fst_len)?;
        let keyword_offsets_range =
            checked_range(backing_len, header.keyword_offsets_offset, header.keyword_offsets_len)?;
        let postings_doc_indices_range = checked_range(
            backing_len,
            header.postings_doc_indices_offset,
            header.postings_doc_indices_len,
        )?;
        let postings_scores_range =
            checked_range(backing_len, header.postings_scores_offset, header.postings_scores_len)?;

        let doc_ids_dict_syms_range =
            checked_range(backing_len, header.doc_ids_dict_syms_offset, header.doc_ids_dict_syms_len)?;
        let doc_ids_dict_lens_range =
            checked_range(backing_len, header.doc_ids_dict_lens_offset, header.doc_ids_dict_lens_len)?;
        let doc_ids_offsets_range =
            checked_range(backing_len, header.doc_ids_offsets_offset, header.doc_ids_offsets_len)?;
        let doc_ids_data_range =
            checked_range(backing_len, header.doc_ids_data_offset, header.doc_ids_data_len)?;

        let doc_fields_dict_syms_range = checked_range(
            backing_len,
            header.doc_fields_dict_syms_offset,
            header.doc_fields_dict_syms_len,
        )?;
        let doc_fields_dict_lens_range = checked_range(
            backing_len,
            header.doc_fields_dict_lens_offset,
            header.doc_fields_dict_lens_len,
        )?;
        let doc_fields_offsets_range =
            checked_range(backing_len, header.doc_fields_offsets_offset, header.doc_fields_offsets_len)?;
        let doc_fields_data_range =
            checked_range(backing_len, header.doc_fields_data_offset, header.doc_fields_data_len)?;

        let _ = u32_slice(backing.slice(keyword_offsets_range.clone()))?;
        let postings_doc_indices_len =
            u32_slice(backing.slice(postings_doc_indices_range.clone()))?.len();
        let postings_scores_len = postings_scores_range.end - postings_scores_range.start;
        if postings_scores_len != postings_doc_indices_len {
            return Err("Postings sections length mismatch".into());
        }

        let doc_ids = FsstStrVecView::new(
            &backing,
            doc_ids_dict_syms_range,
            doc_ids_dict_lens_range,
            doc_ids_offsets_range,
            doc_ids_data_range,
        )?;
        let doc_fields = FsstStrVecView::new(
            &backing,
            doc_fields_dict_syms_range,
            doc_fields_dict_lens_range,
            doc_fields_offsets_range,
            doc_fields_data_range,
        )?;

        Ok(MmapIndexData {
            backing,
            fst_range,
            keyword_offsets_range,
            postings_doc_indices_range,
            postings_scores_range,
            doc_ids,
            doc_fields,
        })
    }

    pub(crate) fn fst_bytes(&self) -> &[u8] {
        self.backing.slice(self.fst_range.clone())
    }

    pub(crate) fn doc_ids_len(&self) -> usize {
        self.doc_ids.len()
    }

    pub(crate) fn doc_ids_get(&self, index: usize) -> Option<String> {
        self.doc_ids.get(&self.backing, index)
    }

    pub(crate) fn doc_fields_get(&self, index: usize) -> Option<String> {
        self.doc_fields.get(&self.backing, index)
    }

    pub(crate) fn keyword_postings(
        &self,
        keyword_index: usize,
    ) -> Result<(Range<usize>, &[u32], &[u8]), Box<dyn Error>> {
        let offsets = u32_slice(self.backing.slice(self.keyword_offsets_range.clone()))?;
        if keyword_index + 1 >= offsets.len() {
            return Err("Missing keyword postings".into());
        }
        let start = offsets[keyword_index] as usize;
        let end = offsets[keyword_index + 1] as usize;
        let doc_indices = u32_slice(self.backing.slice(self.postings_doc_indices_range.clone()))?;
        let scores = self.backing.slice(self.postings_scores_range.clone());
        if end > doc_indices.len() || end > scores.len() {
            return Err("Keyword postings range is out of bounds".into());
        }
        Ok((start..end, doc_indices, scores))
    }
}

fn read_u16(bytes: &[u8], offset: &mut usize) -> Result<u16, Box<dyn Error>> {
    let end = *offset + 2;
    if end > bytes.len() {
        return Err("Index header is truncated".into());
    }
    let mut buf = [0u8; 2];
    buf.copy_from_slice(&bytes[*offset..end]);
    *offset = end;
    Ok(u16::from_le_bytes(buf))
}

fn read_u32(bytes: &[u8], offset: &mut usize) -> Result<u32, Box<dyn Error>> {
    let end = *offset + 4;
    if end > bytes.len() {
        return Err("Index header is truncated".into());
    }
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&bytes[*offset..end]);
    *offset = end;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64(bytes: &[u8], offset: &mut usize) -> Result<u64, Box<dyn Error>> {
    let end = *offset + 8;
    if end > bytes.len() {
        return Err("Index header is truncated".into());
    }
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[*offset..end]);
    *offset = end;
    Ok(u64::from_le_bytes(buf))
}

fn write_u16(bytes: &mut [u8], offset: &mut usize, value: u16) {
    let end = *offset + 2;
    bytes[*offset..end].copy_from_slice(&value.to_le_bytes());
    *offset = end;
}

fn write_u32(bytes: &mut [u8], offset: &mut usize, value: u32) {
    let end = *offset + 4;
    bytes[*offset..end].copy_from_slice(&value.to_le_bytes());
    *offset = end;
}

fn write_u64(bytes: &mut [u8], offset: &mut usize, value: u64) {
    let end = *offset + 8;
    bytes[*offset..end].copy_from_slice(&value.to_le_bytes());
    *offset = end;
}

fn align_up(value: usize, alignment: usize) -> usize {
    if alignment == 0 {
        return value;
    }
    (value + alignment - 1) / alignment * alignment
}
