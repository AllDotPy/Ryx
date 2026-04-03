# Ryx ORM Test Suite

This directory contains comprehensive tests for the Ryx ORM, organized into unit and integration tests.

## Test Structure

```
tests/
├── conftest.py              # Shared fixtures and configuration
├── unit/                    # Unit tests (no database required)
│   ├── test_models.py       # Model metaclass, fields, managers
│   ├── test_fields.py       # Field types and validation
│   ├── test_validators.py   # Validator classes
│   ├── test_queryset.py     # QuerySet and Q objects
│   └── test_exceptions.py   # Exception hierarchy
└── integration/             # Integration tests (database required)
    ├── test_crud.py         # Create, Read, Update, Delete operations
    ├── test_queries.py      # Filtering, ordering, pagination
    ├── test_bulk_operations.py  # Bulk create/update/delete/stream
    └── test_transactions.py     # Transaction management
```

## Prerequisites

1. **Rust Extension**: Compile the Rust extension first:
   ```bash
   maturin develop
   ```

2. **Python Dependencies**: Install test dependencies:
   ```bash
   pip install pytest pytest-asyncio
   ```

## Running Tests

### All Tests
```bash
pytest
```

### Unit Tests Only (Fast, no DB)
```bash
pytest tests/unit/
```

### Integration Tests Only (Requires DB)
```bash
pytest tests/integration/
```

### Specific Test File
```bash
pytest tests/integration/test_crud.py
```

### Specific Test
```bash
pytest tests/integration/test_crud.py::TestCreate::test_create_simple
```

### With Coverage
```bash
pytest --cov=ryx --cov-report=html
```

## Test Configuration

- **Database**: Tests use SQLite in-memory database (`sqlite://:memory:`)
- **Isolation**: Each test function gets a clean database state
- **Async**: All tests are async and use `pytest-asyncio`
- **Fixtures**: Shared test data via `conftest.py`

## Test Models

The test suite uses these models defined in `conftest.py`:

- **Author**: Basic model with CharField, EmailField, BooleanField, TextField
- **Post**: Complex model with ForeignKey, unique constraints, indexes, custom validation
- **Tag**: Simple model with unique CharField

## Key Test Areas

### Unit Tests
- Model metaclass and field contribution
- Field validation and type conversion
- Validator logic
- QuerySet building and Q object operations
- Exception hierarchy

### Integration Tests
- CRUD operations (create, get, update, delete)
- Complex queries with filters, ordering, pagination
- Q object combinations
- Bulk operations (create, update, delete, stream)
- Transaction management and isolation
- Foreign key relationships
- Model validation and constraints

## Writing New Tests

### Unit Tests
Use mock for `ryx_core` to test Python logic in isolation:

```python
import sys
mock_core = types.ModuleType("ryx.ryx_core")
sys.modules["ryx.ryx_core"] = mock_core
```

### Integration Tests
Use fixtures from `conftest.py` for database setup and sample data:

```python
@pytest.mark.asyncio
async def test_something(clean_tables, sample_author):
    # Test logic here
    pass
```

### Async Tests
All database tests must be async and marked with `@pytest.mark.asyncio`.

## Troubleshooting

### Import Errors
Make sure the Rust extension is compiled:
```bash
maturin develop
```

### Database Errors
Tests expect SQLite. Check that the database URL in `conftest.py` is correct.

### Test Failures
- Check test isolation (each test should clean up after itself)
- Verify fixture dependencies
- Check async/await usage

## Coverage Goals

- **Models**: 95%+ coverage of model creation, field handling, validation
- **QuerySet**: 90%+ coverage of query building, filtering, ordering
- **Fields**: 95%+ coverage of all field types and validation
- **Integration**: 85%+ coverage of real database operations