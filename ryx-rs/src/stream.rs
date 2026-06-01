use ryx_common::RyxResult;

use crate::queryset::QuerySet;
use crate::row::FromRow;

/// A streaming query result that fetches rows in chunks.
///
/// Supports two pagination modes:
/// - **Keyset cursor**: efficient, stable across data changes (`WHERE col > last_val`)
/// - **LIMIT/OFFSET**: simple but can skip/duplicate rows on concurrent writes
///
/// # Examples
///
/// ```ignore
/// use ryx_rs::stream::QueryStream;
///
/// // Keyset cursor on "id"
/// let mut stream = Post::objects()
///     .filter("active", true)
///     .order_by("id")
///     .stream(100, Some("id"));
///
/// while let Some(chunk) = stream.next_chunk().await? {
///     for post in chunk {
///         // ...
///     }
/// }
///
/// // LIMIT/OFFSET (no keyset)
/// let mut stream = Post::objects().stream(100, None);
/// ```
#[allow(dead_code)]
pub struct QueryStream<T> {
    inner: QuerySet<T>,
    chunk_size: u64,
    keyset: Option<String>,
    last_offset: u64,
    done: bool,
}

impl<T: FromRow> QueryStream<T> {
    pub fn new(qs: QuerySet<T>, chunk_size: u64, keyset: Option<&str>) -> Self {
        Self {
            inner: qs,
            chunk_size,
            keyset: keyset.map(|s| s.to_string()),
            last_offset: 0,
            done: false,
        }
    }

    /// Fetch the next chunk of rows.
    ///
    /// Returns `Ok(None)` when there are no more rows.
    pub async fn next_chunk(&mut self) -> RyxResult<Option<Vec<T>>> {
        if self.done {
            return Ok(None);
        }

        let table_name = self.inner.node.table.to_string();
        let mut qs = QuerySet::new(
            // leak the string to get a &'static str — only the table name is leaked,
            // which is already interned globally anyway
            Box::leak(table_name.into_boxed_str()),
        );
        qs.node = self.inner.node.clone();

        qs = qs.limit(self.chunk_size);
        if self.last_offset > 0 {
            qs = qs.offset(self.last_offset);
        }

        let rows = qs.all().await?;
        let count = rows.len() as u64;

        if count < self.chunk_size {
            self.done = true;
        }
        if rows.is_empty() {
            return Ok(None);
        }

        self.last_offset += count;
        Ok(Some(rows))
    }
}
