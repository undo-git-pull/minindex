use minindex_core::{build_index_from_jsonl, fuzz_search, regex_search, Index as CoreIndex};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

#[pyclass]
#[derive(Clone)]
pub struct SearchResult {
    #[pyo3(get)]
    doc_id: String,
    #[pyo3(get)]
    score: f32,
    #[pyo3(get)]
    matches: Vec<String>,
    #[pyo3(get)]
    document: std::collections::HashMap<String, String>,
}

#[pyclass]
pub struct Index {
    index: CoreIndex,
    source_path: Option<String>,
}

#[pymethods]
impl Index {
    #[new]
    fn new(path: &str) -> PyResult<Self> {
        let index = CoreIndex::from_path(path)
            .map_err(|err| PyRuntimeError::new_err(format!("Failed to open index: {err}")))?;
        Ok(Self {
            index,
            source_path: Some(path.to_string()),
        })
    }

    #[pyo3(signature = (keywords, limit=20))]
    fn fuzz_search(&self, keywords: &str, limit: usize) -> PyResult<Vec<SearchResult>> {
        let results = fuzz_search(&self.index, keywords, limit)
            .map_err(|err| PyRuntimeError::new_err(format!("Search failed: {err}")))?;
        Ok(results
            .into_iter()
            .map(|result| SearchResult {
                doc_id: result.doc_id,
                score: result.score,
                matches: result.matches,
                document: result.document,
            })
            .collect())
    }

    #[pyo3(signature = (regex, limit=20))]
    fn regex_search(&self, regex: &str, limit: usize) -> PyResult<Vec<SearchResult>> {
        let results = regex_search(&self.index, regex, limit)
            .map_err(|err| PyRuntimeError::new_err(format!("Search failed: {err}")))?;
        Ok(results
            .into_iter()
            .map(|result| SearchResult {
                doc_id: result.doc_id,
                score: result.score,
                matches: result.matches,
                document: result.document,
            })
            .collect())
    }

    fn __str__(&self) -> String {
        let doc_count = self.index.doc_count();
        match self.source_path.as_deref() {
            Some(path) => format!("Index(path={}, doc_count={})", path, doc_count),
            None => format!("Index(doc_count={})", doc_count),
        }
    }

    fn __repr__(&self) -> String {
        self.__str__()
    }
}

#[pyfunction]
#[pyo3(signature = (jsonl_path, index_path, index_fields, doc_id_field=None, more_fields=None))]
fn index_from_jsonl(
    jsonl_path: &str,
    index_path: &str,
    index_fields: Vec<String>,
    doc_id_field: Option<String>,
    more_fields: Option<Vec<String>>,
) -> PyResult<()> {
    let index = build_index_from_jsonl(
        jsonl_path,
        &index_fields,
        doc_id_field.as_deref(),
        more_fields.as_ref().map(Vec::as_slice),
    )
    .map_err(|err| PyRuntimeError::new_err(format!("Index build failed: {err}")))?;
    index
        .to_path(index_path)
        .map_err(|err| PyRuntimeError::new_err(format!("Index write failed: {err}")))?;
    Ok(())
}

#[pymethods]
impl SearchResult {
    fn __str__(&self) -> String {
        format!(
            "SearchResult(doc_id={}, score={}, matches={:?}, document={:?})",
            self.doc_id,
            self.score,
            self.matches,
            self.document
        )
    }

    fn __repr__(&self) -> String {
        self.__str__()
    }
}

#[pymodule]
fn minindex(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(index_from_jsonl, m)?)?;
    m.add_class::<Index>()?;
    m.add_class::<SearchResult>()?;
    Ok(())
}
