//! Full-text search for posts, backed by [`tantivy`].
//!
//! One [`SearchIndex`] per process. The index lives on disk (or in
//! `tempfile`-style RAM directories for tests) and is updated as posts
//! are created / updated / deleted. A single shared [`tantivy::IndexReader`]
//! is reused across queries; writers are acquired per-mutation to keep
//! commit boundaries explicit.
//!
//! ## Schema
//!
//! | field      | type     | indexed | stored | notes                       |
//! |------------|----------|---------|--------|-----------------------------|
//! | name       | text     | ✓       | ✓      | post metadata.name (primary)|
//! | title      | text     | ✓       | ✓      |                             |
//! | slug       | text     | ✓       | ✓      |                             |
//! | body       | text     | ✓       |        | composed markdown source    |
//! | tags       | text     | ✓       | ✓      | space-joined                |
//! | categories | text     | ✓       | ✓      | space-joined                |
//! | excerpt    | text     |         | ✓      | for result snippet          |
//! | permalink  | text     |         | ✓      |                             |
//! | publish_ts | i64      |         | ✓      | unix seconds, for sort      |
//!
//! All text fields use the default English tokenizer (lowercase + simple
//! token splitting). This is intentionally simple for v1; multilingual
//! tokenizers can be wired in once we have multi-language sites.

use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use parking_lot::RwLock;
use serde::Serialize;
use tantivy::collector::TopDocs;
use tantivy::doc;
use tantivy::query::QueryParser;
use tantivy::schema::{
    Field, NumericOptions, Schema, SchemaBuilder, Value, FAST, STORED, STRING, TEXT,
};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("tantivy: {0}")]
    Tantivy(#[from] tantivy::TantivyError),
    #[error("query: {0}")]
    Query(#[from] tantivy::query::QueryParserError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("schema mismatch")]
    SchemaMismatch,
}

/// One result from [`SearchIndex::search`].
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub name: String,
    pub title: String,
    pub slug: String,
    pub permalink: String,
    pub excerpt: String,
    pub tags: Vec<String>,
    pub categories: Vec<String>,
    pub publish_time: Option<DateTime<Utc>>,
    pub score: f32,
}

/// Payload accepted by [`SearchIndex::index`] / [`SearchIndex::replace`].
#[derive(Debug, Clone)]
pub struct IndexPost {
    pub name: String,
    pub title: String,
    pub slug: String,
    pub permalink: String,
    pub excerpt: String,
    pub body: String,
    pub tags: Vec<String>,
    pub categories: Vec<String>,
    pub publish_time: Option<DateTime<Utc>>,
}

struct Fields {
    name: Field,
    title: Field,
    slug: Field,
    body: Field,
    tags: Field,
    categories: Field,
    excerpt: Field,
    permalink: Field,
    publish_ts: Field,
}

impl Fields {
    fn build(builder: &mut SchemaBuilder) -> Self {
        let int_options = NumericOptions::default().set_stored().set_indexed() | FAST;
        Self {
            name: builder.add_text_field("name", STRING | STORED),
            title: builder.add_text_field("title", TEXT | STORED),
            slug: builder.add_text_field("slug", STRING | STORED),
            body: builder.add_text_field("body", TEXT),
            tags: builder.add_text_field("tags", TEXT | STORED),
            categories: builder.add_text_field("categories", TEXT | STORED),
            excerpt: builder.add_text_field("excerpt", STORED),
            permalink: builder.add_text_field("permalink", STORED),
            publish_ts: builder.add_i64_field("publish_ts", int_options),
        }
    }

    fn from_schema(schema: &Schema) -> Result<Self, SearchError> {
        Ok(Self {
            name: schema
                .get_field("name")
                .map_err(|_| SearchError::SchemaMismatch)?,
            title: schema
                .get_field("title")
                .map_err(|_| SearchError::SchemaMismatch)?,
            slug: schema
                .get_field("slug")
                .map_err(|_| SearchError::SchemaMismatch)?,
            body: schema
                .get_field("body")
                .map_err(|_| SearchError::SchemaMismatch)?,
            tags: schema
                .get_field("tags")
                .map_err(|_| SearchError::SchemaMismatch)?,
            categories: schema
                .get_field("categories")
                .map_err(|_| SearchError::SchemaMismatch)?,
            excerpt: schema
                .get_field("excerpt")
                .map_err(|_| SearchError::SchemaMismatch)?,
            permalink: schema
                .get_field("permalink")
                .map_err(|_| SearchError::SchemaMismatch)?,
            publish_ts: schema
                .get_field("publish_ts")
                .map_err(|_| SearchError::SchemaMismatch)?,
        })
    }
}

/// Thread-safe, cloneable handle to the search index. Cheap to clone
/// (`Arc<Inner>`).
#[derive(Clone)]
pub struct SearchIndex {
    inner: Arc<Inner>,
}

struct Inner {
    index: Index,
    schema: Schema,
    fields: Fields,
    reader: RwLock<IndexReader>,
}

impl SearchIndex {
    /// Open a search index on disk at `dir`. Creates the directory if it
    /// doesn't exist; reuses any existing one (the schema is asserted on
    /// open).
    pub fn open(dir: &Path) -> Result<Self, SearchError> {
        std::fs::create_dir_all(dir)?;
        let mut builder = Schema::builder();
        Fields::build(&mut builder);
        let schema = builder.build();
        let index = Index::open_or_create(
            tantivy::directory::MmapDirectory::open(dir)
                .map_err(|e| SearchError::Tantivy(tantivy::TantivyError::from(e)))?,
            schema.clone(),
        )?;
        let fields = Fields::from_schema(&schema)?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;
        Ok(Self {
            inner: Arc::new(Inner {
                index,
                schema,
                fields,
                reader: RwLock::new(reader),
            }),
        })
    }

    /// In-memory index, for tests.
    pub fn in_memory() -> Result<Self, SearchError> {
        let mut builder = Schema::builder();
        Fields::build(&mut builder);
        let schema = builder.build();
        let index = Index::create_in_ram(schema.clone());
        let fields = Fields::from_schema(&schema)?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;
        Ok(Self {
            inner: Arc::new(Inner {
                index,
                schema,
                fields,
                reader: RwLock::new(reader),
            }),
        })
    }

    fn writer(&self) -> Result<IndexWriter, SearchError> {
        // 50 MiB writer buffer is plenty for v1: tantivy recommends >= 15 MiB.
        Ok(self.inner.index.writer(50_000_000)?)
    }

    /// Add or replace `post` in the index. Commits before returning.
    pub fn replace(&self, post: &IndexPost) -> Result<(), SearchError> {
        let mut writer = self.writer()?;
        writer.delete_term(Term::from_field_text(self.inner.fields.name, &post.name));
        let f = &self.inner.fields;
        let ts = post.publish_time.map_or(0, |t| t.timestamp());
        writer.add_document(doc!(
            f.name => post.name.clone(),
            f.title => post.title.clone(),
            f.slug => post.slug.clone(),
            f.body => post.body.clone(),
            f.tags => post.tags.join(" "),
            f.categories => post.categories.join(" "),
            f.excerpt => post.excerpt.clone(),
            f.permalink => post.permalink.clone(),
            f.publish_ts => ts,
        ))?;
        writer.commit()?;
        self.inner.reader.read().reload()?;
        Ok(())
    }

    /// Remove a post from the index by `metadata.name`.
    pub fn delete(&self, name: &str) -> Result<(), SearchError> {
        let mut writer = self.writer()?;
        writer.delete_term(Term::from_field_text(self.inner.fields.name, name));
        writer.commit()?;
        self.inner.reader.read().reload()?;
        Ok(())
    }

    /// Wipe and re-index the entire corpus. Use sparingly — once on boot
    /// and once when an admin clicks "rebuild".
    pub fn rebuild<I>(&self, posts: I) -> Result<(), SearchError>
    where
        I: IntoIterator<Item = IndexPost>,
    {
        let mut writer = self.writer()?;
        writer.delete_all_documents()?;
        let f = &self.inner.fields;
        for post in posts {
            let ts = post.publish_time.map_or(0, |t| t.timestamp());
            writer.add_document(doc!(
                f.name => post.name.clone(),
                f.title => post.title.clone(),
                f.slug => post.slug.clone(),
                f.body => post.body.clone(),
                f.tags => post.tags.join(" "),
                f.categories => post.categories.join(" "),
                f.excerpt => post.excerpt.clone(),
                f.permalink => post.permalink.clone(),
                f.publish_ts => ts,
            ))?;
        }
        writer.commit()?;
        self.inner.reader.read().reload()?;
        Ok(())
    }

    /// Run `query` against `title`, `body`, `tags`, `categories`. Returns
    /// up to `limit` hits, ordered by relevance.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, SearchError> {
        if query.trim().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let reader = self.inner.reader.read().clone();
        let searcher = reader.searcher();
        let f = &self.inner.fields;
        let parser = QueryParser::for_index(
            &self.inner.index,
            vec![f.title, f.body, f.tags, f.categories, f.slug],
        );
        let parsed = parser.parse_query(query)?;
        let collector = TopDocs::with_limit(limit);
        let hits = searcher.search(&parsed, &collector)?;
        let mut out = Vec::with_capacity(hits.len());
        for (score, doc_addr) in hits {
            let document: TantivyDocument = searcher.doc(doc_addr)?;
            out.push(hit_from_doc(&document, f, score, &self.inner.schema));
        }
        Ok(out)
    }

    /// Total document count (handy for `/admin/system/info`).
    pub fn count(&self) -> usize {
        let reader = self.inner.reader.read().clone();
        let searcher = reader.searcher();
        usize::try_from(searcher.num_docs()).unwrap_or(usize::MAX)
    }
}

fn hit_from_doc(doc: &TantivyDocument, f: &Fields, score: f32, schema: &Schema) -> SearchHit {
    let _ = schema;
    let text = |field: Field| -> String {
        doc.get_first(field)
            .and_then(|v| v.as_str().map(ToOwned::to_owned))
            .unwrap_or_default()
    };
    let split = |field: Field| -> Vec<String> {
        text(field).split_whitespace().map(str::to_owned).collect()
    };
    let publish_time = doc
        .get_first(f.publish_ts)
        .and_then(|v| v.as_i64())
        .filter(|&ts| ts > 0)
        .and_then(|ts| Utc.timestamp_opt(ts, 0).single());
    SearchHit {
        name: text(f.name),
        title: text(f.title),
        slug: text(f.slug),
        permalink: text(f.permalink),
        excerpt: text(f.excerpt),
        tags: split(f.tags),
        categories: split(f.categories),
        publish_time,
        score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> IndexPost {
        IndexPost {
            name: "hello-world".into(),
            title: "Hello World in Rust".into(),
            slug: "hello-world".into(),
            permalink: "/archives/hello-world".into(),
            excerpt: "Rust example post".into(),
            body: "We discuss Rust traits, async, and tokio here.".into(),
            tags: vec!["rust".into(), "async".into()],
            categories: vec!["news".into()],
            publish_time: Some(Utc.timestamp_opt(1_700_000_000, 0).single().unwrap()),
        }
    }

    #[test]
    fn round_trip_index_and_search() {
        let idx = SearchIndex::in_memory().unwrap();
        idx.replace(&sample()).unwrap();
        let hits = idx.search("Rust", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "hello-world");
        assert_eq!(hits[0].tags, vec!["rust", "async"]);
        assert!(hits[0].score > 0.0);
    }

    #[test]
    fn delete_removes_post() {
        let idx = SearchIndex::in_memory().unwrap();
        idx.replace(&sample()).unwrap();
        idx.delete("hello-world").unwrap();
        let hits = idx.search("Rust", 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn replace_overwrites_existing() {
        let idx = SearchIndex::in_memory().unwrap();
        idx.replace(&sample()).unwrap();
        let mut updated = sample();
        updated.title = "Goodbye World".into();
        updated.body = "We say farewell.".into();
        updated.tags = vec![];
        updated.categories = vec![];
        idx.replace(&updated).unwrap();
        let hits = idx.search("traits", 10).unwrap();
        assert!(hits.is_empty(), "old post must be replaced, not duplicated");
        let hits = idx.search("Goodbye", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(idx.count() == 1, "exactly one document should be stored");
    }

    #[test]
    fn rebuild_replaces_corpus() {
        let idx = SearchIndex::in_memory().unwrap();
        idx.replace(&sample()).unwrap();
        let other = IndexPost {
            name: "alt".into(),
            title: "Alt post".into(),
            slug: "alt".into(),
            permalink: "/archives/alt".into(),
            excerpt: String::new(),
            body: "Postgres SQLite differences".into(),
            tags: vec![],
            categories: vec![],
            publish_time: None,
        };
        idx.rebuild(vec![other.clone()]).unwrap();
        assert!(idx.search("Rust", 10).unwrap().is_empty());
        let hits = idx.search("Postgres", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "alt");
    }

    #[test]
    fn empty_query_returns_no_hits() {
        let idx = SearchIndex::in_memory().unwrap();
        idx.replace(&sample()).unwrap();
        assert!(idx.search("", 10).unwrap().is_empty());
    }
}
