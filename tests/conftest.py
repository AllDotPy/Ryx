"""
Pytest configuration and shared fixtures for Ryx ORM tests.
"""

import asyncio
import os
import pytest
import sys
from pathlib import Path

# Add the project root to Python path
sys.path.insert(0, str(Path(__file__).parent.parent))

# Mock ryx_core for unit tests
mock_core = None
if "PYTEST_CURRENT_TEST" in os.environ:
    # We're running under pytest, set up mocks for unit tests
    import types

    mock_core = types.ModuleType("ryx.ryx_core")
    mock_core.__version__ = "0.1.0"

    class MockQueryBuilder:
        def __init__(self, table):
            self._table = table
            self._filters = []
            self._order = []
            self._limit = None
            self._offset = None
            self._distinct = False
            self._annotations = []
            self._group_by = []
            self._joins = []

        def add_filter(self, field, lookup, value, negated=False, **kwargs):
            new_qb = MockQueryBuilder(self._table)
            new_qb._filters = self._filters + [(field, lookup, value, negated)]
            new_qb._order = self._order[:]
            new_qb._limit = self._limit
            new_qb._offset = self._offset
            new_qb._distinct = self._distinct
            new_qb._annotations = self._annotations[:]
            new_qb._group_by = self._group_by[:]
            new_qb._joins = self._joins[:]
            return new_qb

        def add_order_by(self, field):
            new_qb = MockQueryBuilder(self._table)
            new_qb._filters = self._filters[:]
            new_qb._order = self._order + [field]
            new_qb._limit = self._limit
            new_qb._offset = self._offset
            new_qb._distinct = self._distinct
            new_qb._annotations = self._annotations[:]
            new_qb._group_by = self._group_by[:]
            new_qb._joins = self._joins[:]
            return new_qb

        def set_limit(self, n):
            new_qb = MockQueryBuilder(self._table)
            new_qb._filters = self._filters[:]
            new_qb._order = self._order[:]
            new_qb._limit = n
            new_qb._offset = self._offset
            new_qb._distinct = self._distinct
            new_qb._annotations = self._annotations[:]
            new_qb._group_by = self._group_by[:]
            new_qb._joins = self._joins[:]
            return new_qb

        def set_offset(self, n):
            new_qb = MockQueryBuilder(self._table)
            new_qb._filters = self._filters[:]
            new_qb._order = self._order[:]
            new_qb._limit = self._limit
            new_qb._offset = n
            new_qb._distinct = self._distinct
            new_qb._annotations = self._annotations[:]
            new_qb._group_by = self._group_by[:]
            new_qb._joins = self._joins[:]
            return new_qb

        def set_distinct(self):
            new_qb = MockQueryBuilder(self._table)
            new_qb._filters = self._filters[:]
            new_qb._order = self._order[:]
            new_qb._limit = self._limit
            new_qb._offset = self._offset
            new_qb._distinct = True
            new_qb._annotations = self._annotations[:]
            new_qb._group_by = self._group_by[:]
            new_qb._joins = self._joins[:]
            return new_qb

        def add_annotation(self, alias, func, field, distinct):
            new_qb = MockQueryBuilder(self._table)
            new_qb._filters = self._filters[:]
            new_qb._order = self._order[:]
            new_qb._limit = self._limit
            new_qb._offset = self._offset
            new_qb._distinct = self._distinct
            new_qb._annotations = self._annotations + [(alias, func, field, distinct)]
            new_qb._group_by = self._group_by[:]
            new_qb._joins = self._joins[:]
            return new_qb

        def add_group_by(self, field):
            new_qb = MockQueryBuilder(self._table)
            new_qb._filters = self._filters[:]
            new_qb._order = self._order[:]
            new_qb._limit = self._limit
            new_qb._offset = self._offset
            new_qb._distinct = self._distinct
            new_qb._annotations = self._annotations[:]
            new_qb._group_by = self._group_by + [field]
            new_qb._joins = self._joins[:]
            return new_qb

        def add_join(self, kind, table, alias, left_field, right_field):
            new_qb = MockQueryBuilder(self._table)
            new_qb._filters = self._filters[:]
            new_qb._order = self._order[:]
            new_qb._limit = self._limit
            new_qb._offset = self._offset
            new_qb._distinct = self._distinct
            new_qb._annotations = self._annotations[:]
            new_qb._group_by = self._group_by[:]
            new_qb._joins = self._joins + [
                (kind, table, alias, left_field, right_field)
            ]
            return new_qb

        def compiled_sql(self):
            filters = " AND ".join(
                f'{"NOT " if neg else ""}"{f}" {lk} ?'
                for f, lk, v, neg in self._filters
            )
            where = f" WHERE {filters}" if filters else ""
            order = f" ORDER BY {', '.join(self._order)}" if self._order else ""
            limit = f" LIMIT {self._limit}" if self._limit else ""
            offset = f" OFFSET {self._offset}" if self._offset else ""
            distinct = " DISTINCT" if self._distinct else ""
            return (
                f'SELECT{distinct} * FROM "{self._table}"{where}{order}{limit}{offset}'
            )

        async def fetch_all(self):
            return []

        async def fetch_count(self):
            return 0

        async def fetch_first(self):
            return None

        async def fetch_get(self):
            raise RuntimeError("No matching object found")

        async def execute_delete(self):
            return 0

        async def execute_update(self, assignments):
            return 0

        async def execute_insert(self, values, returning_id=False):
            return 1

        async def fetch_aggregate(self):
            return {}

    mock_core.QueryBuilder = MockQueryBuilder
    mock_core.available_lookups = lambda: [
        "exact",
        "gt",
        "gte",
        "lt",
        "lte",
        "contains",
        "icontains",
        "startswith",
        "istartswith",
        "endswith",
        "iendswith",
        "isnull",
        "in",
        "range",
    ]
    mock_core.register_lookup = lambda name, tpl: None

    sys.modules["ryx.ryx_core"] = mock_core


# Import ryx components (after mock setup)
def _import_ryx_components():
    try:
        import ryx
        from ryx import (
            Model,
            CharField,
            IntField,
            BooleanField,
            TextField,
            DateTimeField,
            FloatField,
            DecimalField,
            UUIDField,
            EmailField,
            ForeignKey,
            Index,
            Constraint,
            ValidationError,
            Q,
            Count,
            Sum,
            Avg,
            Min,
            Max,
            transaction,
            run_sync,
            bulk_create,
            bulk_update,
            bulk_delete,
            stream,
            MemoryCache,
            configure_cache,
            invalidate_model,
        )
        from ryx.migrations import MigrationRunner
        from ryx.exceptions import (
            RyxError,
            DatabaseError,
            DoesNotExist,
            MultipleObjectsReturned,
        )

        return (
            True,
            ryx,
            Model,
            CharField,
            IntField,
            BooleanField,
            TextField,
            DateTimeField,
            FloatField,
            DecimalField,
            UUIDField,
            EmailField,
            ForeignKey,
            Index,
            Constraint,
            ValidationError,
            Q,
            Count,
            Sum,
            Avg,
            Min,
            Max,
            transaction,
            run_sync,
            bulk_create,
            bulk_update,
            bulk_delete,
            stream,
            MemoryCache,
            configure_cache,
            invalidate_model,
            MigrationRunner,
            RyxError,
            DatabaseError,
            DoesNotExist,
            MultipleObjectsReturned,
        )
    except ImportError:
        return (
            False,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )


(
    RUST_AVAILABLE,
    ryx_import,
    Model_import,
    CharField_import,
    IntField_import,
    BooleanField_import,
    TextField_import,
    DateTimeField_import,
    FloatField_import,
    DecimalField_import,
    UUIDField_import,
    EmailField_import,
    ForeignKey_import,
    Index_import,
    Constraint_import,
    ValidationError_import,
    Q_import,
    Count_import,
    Sum_import,
    Avg_import,
    Min_import,
    Max_import,
    transaction_import,
    run_sync_import,
    bulk_create_import,
    bulk_update_import,
    bulk_delete_import,
    stream_import,
    MemoryCache_import,
    configure_cache_import,
    invalidate_model_import,
    MigrationRunner_import,
    RyxError_import,
    DatabaseError_import,
    DoesNotExist_import,
    MultipleObjectsReturned_import,
) = _import_ryx_components()

# Only assign if imports succeeded
if RUST_AVAILABLE:
    ryx = ryx_import
    Model = Model_import
    CharField = CharField_import
    IntField = IntField_import
    BooleanField = BooleanField_import
    TextField = TextField_import
    DateTimeField = DateTimeField_import
    FloatField = FloatField_import
    DecimalField = DecimalField_import
    UUIDField = UUIDField_import
    EmailField = EmailField_import
    ForeignKey = ForeignKey_import
    Index = Index_import
    Constraint = Constraint_import
    ValidationError = ValidationError_import
    Q = Q_import
    Count = Count_import
    Sum = Sum_import
    Avg = Avg_import
    Min = Min_import
    Max = Max_import
    transaction = transaction_import
    run_sync = run_sync_import
    bulk_create = bulk_create_import
    bulk_update = bulk_update_import
    bulk_delete = bulk_delete_import
    stream = stream_import
    MemoryCache = MemoryCache_import
    configure_cache = configure_cache_import
    invalidate_model = invalidate_model_import
    MigrationRunner = MigrationRunner_import
    RyxError = RyxError_import
    DatabaseError = DatabaseError_import
    DoesNotExist = DoesNotExist_import
    MultipleObjectsReturned = MultipleObjectsReturned_import


@pytest.fixture(scope="session")
def event_loop():
    """Create an instance of the default event loop for the test session."""
    loop = asyncio.get_event_loop_policy().new_event_loop()
    yield loop
    loop.close()


@pytest.fixture(scope="session", autouse=True)
def setup_database():
    """Set up the test database once per test session."""
    if not RUST_AVAILABLE:
        pytest.skip("Rust extension not available. Run 'maturin develop' first.")

    # Use an on-disk SQLite file for tests to allow migrations and transactions.
    # Starting with a clean database file avoids schema drift across reruns.
    db_path = "test_db.sqlite3"
    if os.path.exists(db_path):
        os.remove(db_path)

    # Create the DB file for SQLite mode=rwc so it can open it.
    Path(db_path).touch()

    db_url = f"sqlite://{db_path}?mode=rwc"
    os.environ["RYX_DATABASE_URL"] = db_url
    asyncio.run(ryx.setup(db_url))

    # Run migrations against test models so tables exist for integration tests
    runner = MigrationRunner([Author, Post, Tag, PostTag])
    asyncio.run(runner.migrate())


# Test Models
class Author(Model):
    class Meta:
        table_name = "test_authors"
        indexes = [Index(fields=["email"], name="author_email_idx")]

    name = CharField(max_length=100)
    email = EmailField(unique=True, null=True)
    active = BooleanField(default=True)
    bio = TextField(null=True, blank=True)


class Post(Model):
    class Meta:
        table_name = "test_posts"
        ordering = ["-created_at"]
        unique_together = [("author_id", "slug")]
        indexes = [
            Index(fields=["title"], name="post_title_idx"),
            Index(fields=["created_at"], name="post_created_at_idx"),
        ]
        constraints = [
            Constraint(check="views >= 0", name="post_views_positive"),
        ]

    title = CharField(max_length=200)
    slug = CharField(max_length=200, unique=True, null=True, blank=True)
    body = TextField(null=True, blank=True)
    views = IntField(default=0, min_value=0)
    active = BooleanField(default=True)
    score = FloatField(default=0.0)
    author = ForeignKey(Author, null=True, on_delete="SET_NULL")
    created_at = DateTimeField(auto_now_add=True, null=True)
    updated_at = DateTimeField(auto_now=True, null=True)

    async def clean(self):
        if self.views < 0:
            raise ValidationError({"views": ["Views must be >= 0"]})
        if len(self.title) < 3:
            raise ValidationError({"title": ["Title must be at least 3 characters"]})


class Tag(Model):
    class Meta:
        table_name = "test_tags"

    name = CharField(max_length=50, unique=True)
    color = CharField(max_length=7, default="#000000")
    description = TextField(null=True)


class PostTag(Model):
    """Many-to-many relationship between Post and Tag."""

    class Meta:
        table_name = "test_post_tags"
        unique_together = [("post_id", "tag_id")]

    post = ForeignKey(Post, on_delete="CASCADE")
    tag = ForeignKey(Tag, on_delete="CASCADE")


@pytest.fixture(scope="function", autouse=True)
async def clean_tables():
    """Clean all test tables before each test."""
    tables = ["test_posts", "test_authors", "test_tags", "test_post_tags"]
    from ryx.executor_helpers import raw_execute

    for table in tables:
        try:
            await raw_execute(f'DELETE FROM "{table}"')
        except Exception:
            pass  # Table might not exist yet


@pytest.fixture
async def sample_author():
    """Create a sample author for testing."""
    return await Author.objects.create(
        name="John Doe", email="john@example.com", bio="A test author"
    )


@pytest.fixture
async def sample_post(sample_author):
    """Create a sample post for testing."""
    return await Post.objects.create(
        title="Test Post",
        slug="test-post",
        body="This is a test post content.",
        views=10,
        author=sample_author,
    )


@pytest.fixture
async def sample_tags():
    """Create sample tags for testing."""
    tag1 = await Tag.objects.create(name="Python", color="#3776AB")
    tag2 = await Tag.objects.create(name="Django", color="#092E20")
    return [tag1, tag2]


@pytest.fixture
def mock_ryx_core():
    """Mock ryx_core for unit tests that don't need the real Rust extension."""
    return mock_core
