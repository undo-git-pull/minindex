mod serialization;

use memmap2::Mmap;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use crate::serialization::{
    build_postings, checked_range, encode_string_table, flatten_dict_syms, parse_string_table,
    append_section, append_u32_section, Backing, IndexHeader, MmapIndexData, SectionOffsets,
    HEADER_SIZE,
};

#[derive(Debug, Clone)]
pub struct FsstStrVec {
    dict_syms: Vec<[u8; 8]>,
    dict_lens: Vec<u8>,
    offsets: Vec<u32>,
    data: Vec<u8>,
}

impl FsstStrVec {
    fn from_strings(strings: &[impl AsRef<str>]) -> Self {
        let sample: Vec<&[u8]> = strings.iter().map(|s| s.as_ref().as_bytes()).collect();
        let compressor = fsst::Compressor::train(&sample);

        let syms: Vec<fsst::Symbol> = compressor.symbol_table().to_vec();
        let lens: Vec<u8> = compressor.symbol_lengths().to_vec();

        let mut offsets = Vec::with_capacity(strings.len());
        let mut data = Vec::new();
        for s in strings {
            offsets.push(data.len() as u32);
            let c = compressor.compress(s.as_ref().as_bytes());
            data.extend_from_slice(&c);
        }

        let dict_syms: Vec<[u8; 8]> = syms
            .into_iter()
            .map(|sym| u64::to_le_bytes(sym.to_u64()))
            .collect();

        Self {
            dict_syms,
            dict_lens: lens,
            offsets,
            data,
        }
    }

    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    pub fn get(&self, i: usize) -> Option<String> {
        if i >= self.len() {
            return None;
        }
        let start = self.offsets[i] as usize;
        let end = if i + 1 < self.len() {
            self.offsets[i + 1] as usize
        } else {
            self.data.len()
        };
        let codes = &self.data[start..end];

        let syms: Vec<fsst::Symbol> = self
            .dict_syms
            .iter()
            .map(fsst::Symbol::from_slice)
            .collect();
        let decomp = fsst::Decompressor::new(&syms, &self.dict_lens);

        let bytes = decomp.decompress(codes);
        Some(String::from_utf8(bytes).expect("FSST preserves UTF-8 for UTF-8 input"))
    }
}

#[derive(Debug)]
pub struct Index {
    field_count: u16,
    indexed_field_count: u16,
    index_fields: Vec<String>,
    doc_id_field: Option<String>,
    storage: IndexStorage,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub doc_id: String,
    pub score: f32,
    pub matches: Vec<String>,
    pub document: HashMap<String, String>,
}

#[derive(Debug)]
enum IndexStorage {
    Owned(OwnedIndexData),
    Mmap(MmapIndexData),
}

#[derive(Debug)]
struct OwnedIndexData {
    fst: Vec<u8>,
    keyword_to_documents: Vec<Vec<(usize, u8)>>,
    doc_ids: FsstStrVec,
    doc_fields: FsstStrVec,
}

#[derive(Debug)]
enum Postings<'a> {
    Owned(&'a [(usize, u8)]),
    Mmap { doc_indices: &'a [u32], scores: &'a [u8] },
}

#[derive(Debug)]
struct PostingsIter<'a> {
    postings: &'a Postings<'a>,
    index: usize,
}

impl<'a> Postings<'a> {
    fn len(&self) -> usize {
        match self {
            Postings::Owned(docs) => docs.len(),
            Postings::Mmap { doc_indices, scores } => doc_indices.len().min(scores.len()),
        }
    }

    fn iter(&'a self) -> PostingsIter<'a> {
        PostingsIter { postings: self, index: 0 }
    }
}

impl<'a> Iterator for PostingsIter<'a> {
    type Item = (usize, u8);

    fn next(&mut self) -> Option<Self::Item> {
        let idx = self.index;
        let len = self.postings.len();
        if idx >= len {
            return None;
        }
        self.index += 1;
        match self.postings {
            Postings::Owned(docs) => docs.get(idx).copied(),
            Postings::Mmap { doc_indices, scores } => doc_indices
                .get(idx)
                .and_then(|doc| scores.get(idx).map(|score| (*doc as usize, *score))),
        }
    }
}

impl Index {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        Self::from_backing(Backing::Bytes(bytes.to_vec()))
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let file = File::open(path)?;
        // Safety: mmap requires unsafe; we only read the file and keep it alive
        // for the duration of the mapping.
        let mmap = unsafe { Mmap::map(&file)? };
        Self::from_backing(Backing::Mmap(mmap))
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        self.serialize_owned()
    }

    pub fn to_path(&self, path: impl AsRef<Path>) -> Result<(), Box<dyn Error>> {
        let bytes = self.serialize_owned()?;
        let mut file = File::create(path)?;
        file.write_all(&bytes)?;
        Ok(())
    }

    pub fn doc_count(&self) -> usize {
        self.doc_ids_len()
    }

    fn from_backing(backing: Backing) -> Result<Self, Box<dyn Error>> {
        if !cfg!(target_endian = "little") {
            return Err("Index loading is only supported on little-endian targets".into());
        }

        let header = IndexHeader::read(backing.as_bytes())?;
        let indexed_field_count = if header.indexed_field_count == 0 {
            header.field_count
        } else {
            header.indexed_field_count
        };
        if indexed_field_count > header.field_count {
            return Err("Indexed field count exceeds stored field count".into());
        }

        let index_fields_range = checked_range(
            backing.len(),
            header.index_fields_offset,
            header.index_fields_len,
        )?;
        let index_fields = parse_string_table(backing.slice(index_fields_range))?;

        let doc_id_field = if header.doc_id_field_len == 0 {
            None
        } else {
            let bytes = backing.slice(checked_range(
                backing.len(),
                header.doc_id_field_offset,
                header.doc_id_field_len,
            )?);
            Some(String::from_utf8(bytes.to_vec())?)
        };

        let storage = MmapIndexData::new(backing, &header)?;

        if index_fields.len() != header.field_count as usize {
            return Err("Index field count mismatch".into());
        }

        Ok(Index {
            field_count: header.field_count,
            indexed_field_count,
            index_fields,
            doc_id_field,
            storage: IndexStorage::Mmap(storage),
        })
    }

    fn serialize_owned(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        let data = match &self.storage {
            IndexStorage::Owned(data) => data,
            IndexStorage::Mmap(_) => return Err("Cannot serialize a mmap-backed index".into()),
        };

        let (keyword_offsets, postings_doc_indices, postings_scores) =
            build_postings(&data.keyword_to_documents)?;

        let index_fields_bytes = encode_string_table(&self.index_fields);
        let doc_id_field_bytes = self
            .doc_id_field
            .as_ref()
            .map(|s| s.as_bytes().to_vec())
            .unwrap_or_default();

        let doc_ids_dict_syms = flatten_dict_syms(&data.doc_ids.dict_syms);
        let doc_fields_dict_syms = flatten_dict_syms(&data.doc_fields.dict_syms);

        let mut buffer = vec![0u8; HEADER_SIZE];
        let mut offsets = SectionOffsets::default();

        offsets.index_fields = append_section(&mut buffer, &index_fields_bytes, 8);
        offsets.doc_id_field = append_section(&mut buffer, &doc_id_field_bytes, 8);
        offsets.fst = append_section(&mut buffer, &data.fst, 8);
        offsets.keyword_offsets = append_u32_section(&mut buffer, &keyword_offsets, 8);
        offsets.postings_doc_indices =
            append_u32_section(&mut buffer, &postings_doc_indices, 8);
        offsets.postings_scores = append_section(&mut buffer, &postings_scores, 8);
        offsets.doc_ids_dict_syms = append_section(&mut buffer, &doc_ids_dict_syms, 8);
        offsets.doc_ids_dict_lens = append_section(&mut buffer, &data.doc_ids.dict_lens, 8);
        offsets.doc_ids_offsets = append_u32_section(&mut buffer, &data.doc_ids.offsets, 8);
        offsets.doc_ids_data = append_section(&mut buffer, &data.doc_ids.data, 8);
        offsets.doc_fields_dict_syms = append_section(&mut buffer, &doc_fields_dict_syms, 8);
        offsets.doc_fields_dict_lens = append_section(&mut buffer, &data.doc_fields.dict_lens, 8);
        offsets.doc_fields_offsets = append_u32_section(&mut buffer, &data.doc_fields.offsets, 8);
        offsets.doc_fields_data = append_section(&mut buffer, &data.doc_fields.data, 8);

        let header = IndexHeader {
            field_count: self.field_count,
            indexed_field_count: self.indexed_field_count,
            index_fields_offset: offsets.index_fields.offset,
            index_fields_len: offsets.index_fields.len,
            doc_id_field_offset: offsets.doc_id_field.offset,
            doc_id_field_len: offsets.doc_id_field.len,
            fst_offset: offsets.fst.offset,
            fst_len: offsets.fst.len,
            keyword_offsets_offset: offsets.keyword_offsets.offset,
            keyword_offsets_len: offsets.keyword_offsets.len,
            postings_doc_indices_offset: offsets.postings_doc_indices.offset,
            postings_doc_indices_len: offsets.postings_doc_indices.len,
            postings_scores_offset: offsets.postings_scores.offset,
            postings_scores_len: offsets.postings_scores.len,
            doc_ids_dict_syms_offset: offsets.doc_ids_dict_syms.offset,
            doc_ids_dict_syms_len: offsets.doc_ids_dict_syms.len,
            doc_ids_dict_lens_offset: offsets.doc_ids_dict_lens.offset,
            doc_ids_dict_lens_len: offsets.doc_ids_dict_lens.len,
            doc_ids_offsets_offset: offsets.doc_ids_offsets.offset,
            doc_ids_offsets_len: offsets.doc_ids_offsets.len,
            doc_ids_data_offset: offsets.doc_ids_data.offset,
            doc_ids_data_len: offsets.doc_ids_data.len,
            doc_fields_dict_syms_offset: offsets.doc_fields_dict_syms.offset,
            doc_fields_dict_syms_len: offsets.doc_fields_dict_syms.len,
            doc_fields_dict_lens_offset: offsets.doc_fields_dict_lens.offset,
            doc_fields_dict_lens_len: offsets.doc_fields_dict_lens.len,
            doc_fields_offsets_offset: offsets.doc_fields_offsets.offset,
            doc_fields_offsets_len: offsets.doc_fields_offsets.len,
            doc_fields_data_offset: offsets.doc_fields_data.offset,
            doc_fields_data_len: offsets.doc_fields_data.len,
        };

        header.write(&mut buffer[..HEADER_SIZE])?;

        Ok(buffer)
    }

    fn doc_ids_len(&self) -> usize {
        match &self.storage {
            IndexStorage::Owned(data) => data.doc_ids.len(),
            IndexStorage::Mmap(data) => data.doc_ids_len(),
        }
    }

    fn doc_ids_get(&self, index: usize) -> Option<String> {
        match &self.storage {
            IndexStorage::Owned(data) => data.doc_ids.get(index),
            IndexStorage::Mmap(data) => data.doc_ids_get(index),
        }
    }

    fn doc_fields_get(&self, index: usize) -> Option<String> {
        match &self.storage {
            IndexStorage::Owned(data) => data.doc_fields.get(index),
            IndexStorage::Mmap(data) => data.doc_fields_get(index),
        }
    }

    fn fst_bytes(&self) -> &[u8] {
        match &self.storage {
            IndexStorage::Owned(data) => &data.fst,
            IndexStorage::Mmap(data) => data.fst_bytes(),
        }
    }

    fn keyword_postings(&self, keyword_index: usize) -> Result<Postings<'_>, Box<dyn Error>> {
        match &self.storage {
            IndexStorage::Owned(data) => data
                .keyword_to_documents
                .get(keyword_index)
                .map(|docs| Postings::Owned(docs.as_slice()))
                .ok_or_else(|| "Missing keyword postings".into()),
            IndexStorage::Mmap(data) => {
                let (range, doc_indices, scores) = data.keyword_postings(keyword_index)?;
                Ok(Postings::Mmap {
                    doc_indices: &doc_indices[range.clone()],
                    scores: &scores[range],
                })
            }
        }
    }

    fn doc_text(&self, doc_index: usize) -> Result<String, Box<dyn Error>> {
        let field_count = self.field_count as usize;
        let indexed_field_count = self.indexed_field_count as usize;
        let mut parts = Vec::with_capacity(indexed_field_count);
        for field_index in 0..indexed_field_count {
            let slot = doc_index * field_count + field_index;
            let value = self.doc_fields_get(slot).ok_or_else(|| "Missing document field")?;
            parts.push(value);
        }
        Ok(parts.join("\n"))
    }

    fn document_map(&self, doc_index: usize) -> Result<HashMap<String, String>, Box<dyn Error>> {
        let field_count = self.field_count as usize;
        let mut map = HashMap::with_capacity(field_count);
        for (field_offset, field_name) in self.index_fields.iter().enumerate() {
            let slot = doc_index * field_count + field_offset;
            let value = self.doc_fields_get(slot).ok_or_else(|| "Missing document field")?;
            map.insert(field_name.clone(), value);
        }
        if let Some(doc_id_field) = &self.doc_id_field {
            if !self.index_fields.iter().any(|field| field == doc_id_field) {
                let doc_id = self.doc_ids_get(doc_index).ok_or_else(|| "Missing doc_id")?;
                map.insert(doc_id_field.clone(), doc_id);
            }
        }
        Ok(map)
    }
}

pub fn build_index_from_jsonl(
    jsonl_path: impl AsRef<Path>,
    index_fields: &[String],
    doc_id_field: Option<&str>,
    more_fields: Option<&[String]>,
) -> Result<Index, Box<dyn Error>> {
    let jsonl_path = jsonl_path.as_ref();
    if index_fields.is_empty() {
        return Err("index_fields must not be empty".into());
    }
    let mut stored_fields: Vec<String> = index_fields.to_vec();
    let mut stored_fields_set: HashSet<String> = stored_fields.iter().cloned().collect();
    if let Some(more_fields) = more_fields {
        for field in more_fields {
            if stored_fields_set.insert(field.clone()) {
                stored_fields.push(field.clone());
            }
        }
    }

    let stop_words = include_str!("../english.stop")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_lowercase())
        .collect::<HashSet<String>>();

    let stop_words_set = rake::StopWords::from(stop_words.clone());
    let rake = rake::Rake::new(stop_words_set);

    let mut doc_id_field_name = doc_id_field.map(|field| field.to_string());

    let file = File::open(jsonl_path)?;
    let reader = BufReader::new(file);

    let mut doc_ids: Vec<String> = Vec::new();
    let mut doc_fields: Vec<String> = Vec::new();

    let mut keywords_to_documents: HashMap<String, Vec<(usize, f64)>> = HashMap::new();

    for line_result in reader.lines() {
        let line = line_result?;
        if line.trim().is_empty() {
            continue;
        }

        let value: serde_json::Value = serde_json::from_str(&line)?;
        let obj = value
            .as_object()
            .ok_or_else(|| "JSONL line must be an object")?;

        if doc_id_field_name.is_none() {
            let first_key = obj
                .keys()
                .next()
                .ok_or_else(|| "JSON object has no fields")?;
            doc_id_field_name = Some(first_key.to_string());
        }

        let doc_id_field_name = doc_id_field_name
            .as_deref()
            .ok_or_else(|| "doc_id field could not be determined")?;
        let doc_id_value = obj
            .get(doc_id_field_name)
            .ok_or_else(|| format!("Missing doc_id field: {doc_id_field_name}"))?;
        let doc_id = json_value_to_string(doc_id_value)?;

        let mut indexed_values: Vec<String> = Vec::with_capacity(index_fields.len());
        for field in index_fields {
            let value = obj
                .get(field)
                .ok_or_else(|| format!("Missing required field: {field}"))?;
            indexed_values.push(json_value_to_string(value)?);
        }

        let combined_text = indexed_values.join("\n");

        let mut keyword_set: HashSet<String> = HashSet::new();
        let mut keywords: Vec<(String, f64)> = Vec::new();

        for token in tokenize_basic(&combined_text) {
            if stop_words.contains(&token) || keyword_set.contains(&token) {
                continue;
            }
            keywords.push((token.clone(), 90.0));
            keyword_set.insert(token);
        }

        let extracted = rake.run_fragments(vec![combined_text.as_str()]);
        let mut single_word_budget = 6;
        let mut double_word_budget = 4;

        for kw in extracted {
            let keyword = kw.keyword.to_lowercase();
            if keyword_set.contains(&keyword) || stop_words.contains(&keyword) {
                continue;
            }

            let whitespace_count = kw.keyword.matches(' ').count();
            if whitespace_count == 0 && single_word_budget > 0 {
                single_word_budget -= 1;
            } else if whitespace_count == 1 && double_word_budget > 0 {
                double_word_budget -= 1;
            } else {
                continue;
            }

            keywords.push((keyword.clone(), kw.score));
            keyword_set.insert(keyword);

            if single_word_budget == 0 && double_word_budget == 0 {
                break;
            }
        }

        let doc_index = doc_ids.len();
        doc_ids.push(doc_id);
        doc_fields.extend(indexed_values);
        for field in stored_fields.iter().skip(index_fields.len()) {
            let value = match obj.get(field) {
                Some(value) => json_value_to_string(value)?,
                None => String::new(),
            };
            doc_fields.push(value);
        }

        for (keyword, score) in keywords {
            keywords_to_documents
                .entry(keyword)
                .or_default()
                .push((doc_index, score));
        }
    }

    let mut fst_builder = fst::MapBuilder::memory();
    let mut keyword_to_documents: Vec<Vec<(usize, u8)>> = Vec::new();
    let mut keywords: Vec<String> = keywords_to_documents.keys().cloned().collect();
    keywords.sort();

    for (index, keyword) in keywords.iter().enumerate() {
        fst_builder.insert(keyword, index as u64)?;

        let mut doc_scores = keywords_to_documents.get(keyword).unwrap().clone();
        doc_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let entry = doc_scores
            .iter()
            .map(|(doc_index, score)| (*doc_index, score.clamp(0.0, 255.0) as u8))
            .collect::<Vec<(usize, u8)>>();

        keyword_to_documents.push(entry);
    }

    let fst = fst_builder
        .into_inner()
        .map_err(|_| "Failed to build FST")?;
    let doc_ids_vec = FsstStrVec::from_strings(&doc_ids);
    let doc_fields_vec = FsstStrVec::from_strings(&doc_fields);

    Ok(Index {
        field_count: stored_fields.len() as u16,
        indexed_field_count: index_fields.len() as u16,
        index_fields: stored_fields,
        doc_id_field: doc_id_field_name,
        storage: IndexStorage::Owned(OwnedIndexData {
            fst,
            keyword_to_documents,
            doc_ids: doc_ids_vec,
            doc_fields: doc_fields_vec,
        }),
    })
}

pub fn fuzz_search(
    index: &Index,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchResult>, Box<dyn Error>> {
    use fst::automaton::Levenshtein;
    use fst::map::OpBuilder;
    use fst::{Automaton, Streamer};

    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    let stop_words: HashSet<String> = include_str!("../english.stop")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_lowercase())
        .collect();

    let map = fst::Map::new(index.fst_bytes())?;
    let query_terms: Vec<String> = tokenize_basic(query)
        .into_iter()
        .filter(|term| !stop_words.contains(term))
        .collect();

    if query_terms.is_empty() {
        return Ok(Vec::new());
    }

    if query_terms.len() == 1 {
        let mut query_words: HashSet<String> = query_terms.into_iter().collect();
        query_words.insert(query.to_lowercase());

        let mut keywords: Vec<(String, u64)> = Vec::new();

        for query_word in query_words {
            use fst::automaton::Str;

            let lev = Levenshtein::new(query_word.as_str(), 1)?;
            let prefix = Str::new(query_word.as_str()).starts_with();

            let mut op = OpBuilder::new()
                .add(map.search(lev))
                .add(map.search(prefix))
                .union();

            while let Some((keyword, indexed_value)) = op.next() {
                let keyword_str = String::from_utf8(keyword.to_vec())?;
                let value = indexed_value
                    .to_vec()
                    .get(0)
                    .map(|entry| entry.value)
                    .unwrap_or(0);
                keywords.push((keyword_str, value));
            }
        }

        keywords.sort_by_key(|(kw, _)| kw.len());

        let mut documents: HashMap<usize, (u8, Vec<String>)> = HashMap::new();

        for (keyword, keyword_index) in keywords {
            let postings = index.keyword_postings(keyword_index as usize)?;
            for (document_index, score) in postings.iter() {
                let entry = documents.entry(document_index).or_insert((0, Vec::new()));
                entry.0 = entry.0.saturating_add(score);
                if entry.1.len() < 8 && !entry.1.contains(&keyword) {
                    entry.1.push(keyword.clone());
                }
            }
        }

        let mut documents: Vec<(usize, u8, Vec<String>)> = documents
            .into_iter()
            .map(|(doc_index, (score, matches))| (doc_index, score, matches))
            .collect();
        documents.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        documents.truncate(limit);

        let mut results = Vec::with_capacity(documents.len());
        for (document_index, score, matches) in documents {
            let doc_id = index
                .doc_ids_get(document_index)
                .ok_or_else(|| "Missing doc_id")?;
            let document = index.document_map(document_index)?;
            results.push(SearchResult {
                doc_id,
                score: score as f32,
                matches,
                document,
            });
        }

        return Ok(results);
    }

    let mut combined_documents: Option<HashMap<usize, (u8, Vec<String>)>> = None;

    for query_word in query_terms {
        use fst::automaton::Str;

        let lev = Levenshtein::new(query_word.as_str(), 1)?;
        let prefix = Str::new(query_word.as_str()).starts_with();

        let mut op = OpBuilder::new()
            .add(map.search(lev))
            .add(map.search(prefix))
            .union();

        let mut keywords: Vec<(String, u64)> = Vec::new();
        let mut seen_keyword_indexes: HashSet<u64> = HashSet::new();

        while let Some((keyword, indexed_value)) = op.next() {
            let value = indexed_value
                .to_vec()
                .get(0)
                .map(|entry| entry.value)
                .unwrap_or(0);
            if !seen_keyword_indexes.insert(value) {
                continue;
            }
            let keyword_str = String::from_utf8(keyword.to_vec())?;
            keywords.push((keyword_str, value));
        }

        if keywords.is_empty() {
            return Ok(Vec::new());
        }

        let mut term_documents: HashMap<usize, (u8, Vec<String>)> = HashMap::new();
        for (keyword, keyword_index) in keywords {
            let postings = index.keyword_postings(keyword_index as usize)?;
            for (document_index, score) in postings.iter() {
                let entry = term_documents.entry(document_index).or_insert((0, Vec::new()));
                entry.0 = entry.0.saturating_add(score);
                if entry.1.len() < 8 && !entry.1.contains(&keyword) {
                    entry.1.push(keyword.clone());
                }
            }
        }

        match combined_documents.take() {
            None => {
                combined_documents = Some(term_documents);
            }
            Some(mut current) => {
                current.retain(|doc_index, (score, matches)| {
                    if let Some((term_score, term_matches)) = term_documents.get(doc_index) {
                        *score = score.saturating_add(*term_score);
                        for keyword in term_matches {
                            if matches.len() < 8 && !matches.contains(keyword) {
                                matches.push(keyword.clone());
                            }
                        }
                        true
                    } else {
                        false
                    }
                });
                if current.is_empty() {
                    return Ok(Vec::new());
                }
                combined_documents = Some(current);
            }
        }
    }

    let documents = combined_documents.unwrap_or_default();

    let mut documents: Vec<(usize, u8, Vec<String>)> = documents
        .into_iter()
        .map(|(doc_index, (score, matches))| (doc_index, score, matches))
        .collect();
    documents.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    documents.truncate(limit);

    let mut results = Vec::with_capacity(documents.len());
    for (document_index, score, matches) in documents {
        let doc_id = index
            .doc_ids_get(document_index)
            .ok_or_else(|| "Missing doc_id")?;
        let document = index.document_map(document_index)?;
        results.push(SearchResult {
            doc_id,
            score: score as f32,
            matches,
            document,
        });
    }

    Ok(results)
}

pub fn regex_search(
    index: &Index,
    pattern: &str,
    limit: usize,
) -> Result<Vec<SearchResult>, Box<dyn Error>> {
    use regex_automata::meta::Regex;

    if pattern.trim().is_empty() {
        return Ok(Vec::new());
    }

    let regex = Regex::new(pattern)?;
    let mut results: Vec<(usize, SearchResult)> = Vec::new();

    for doc_index in 0..index.doc_count() {
        let text = index.doc_text(doc_index)?;
        let mut matches = Vec::new();

        for m in regex.find_iter(text.as_bytes()) {
            let slice = &text.as_bytes()[m.start()..m.end()];
            matches.push(String::from_utf8_lossy(slice).to_string());
            if matches.len() >= 16 {
                break;
            }
        }

        if matches.is_empty() {
            continue;
        }

        let doc_id = index
            .doc_ids_get(doc_index)
            .ok_or_else(|| "Missing doc_id")?;
        let document = index.document_map(doc_index)?;

        results.push((
            doc_index,
            SearchResult {
                doc_id,
                score: matches.len() as f32,
                matches,
                document,
            },
        ));
    }

    results.sort_by(|a, b| {
        b.1.score
            .partial_cmp(&a.1.score)
            .unwrap()
            .then_with(|| a.0.cmp(&b.0))
    });
    results.truncate(limit);

    Ok(results.into_iter().map(|(_, result)| result).collect())
}

fn tokenize_basic(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

fn json_value_to_string(value: &serde_json::Value) -> Result<String, Box<dyn Error>> {
    match value {
        serde_json::Value::String(s) => Ok(s.clone()),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        serde_json::Value::Bool(b) => Ok(b.to_string()),
        serde_json::Value::Null => Err("Null value not allowed for index field".into()),
        _ => Ok(value.to_string()),
    }
}

#[cfg(test)]
mod tests;
