# Plan d'implémentation — Support pgvector dans Ryx

> Vecteurs d'embeddings (K-NN) pour PostgreSQL, côté Python **et** Rust.

## Décisions de conception

| Décision | Choix |
|---|---|
| API publique | `.nearest_neighbors(field, vector, k, operator)` **et** `.order_by_distance(field, vector, operator)` + `.limit(k)` |
| Opérateurs de distance | Les 3 : `<->` (L2), `<=>` (cosine), `<#>` (produit scalaire) |
| Représentation | `SqlValue::Vector(Vec<f64>)` — nouveau variant dédié |
| Backends non-PG | Erreur claire (`UnsupportedBackend`) — pas de faux support |
| DDL | Pas de changement nécessaire : pass-through `vector(n)` déjà en place |

---

## Architecture cible

```
Python:  Post.objects.nearest_neighbors("embedding", v, k=5).order_by("id")
  │            │                                │
  │            ▼                                ▼
  │     QuerySet._with_op("nearest_neighbor",   order_by: existing
  │            (field, vector, operator, k))
  │            ▼
Rust FFI: plan.rs — tag "nearest_neighbor" → node.with_nearest_neighbor(...)
  │            ▼
  │     QueryNode { nearest_neighbor: Option<NearestNeighborClause> }
  │            ▼
  │     compiler/compilr.rs — ORDER BY "col" <-> ?  (LIMIT ?)
  │            ▼
  │     values: [SqlValue::Vector(...)]
  │            ▼
  └──►  PostgresBackend — bind pgvector format "[1,2,3]"
```

---

## Étapes d'implémentation

### Phase 1 — Types fondamentaux (Rust core)

#### 1.1 `SqlValue::Vector` — `ryx-query/src/ast.rs`

Ajouter un variant au enum (après `Json`, ~ligne 46) :

```rust
/// pgvector embedding vector (PostgreSQL only). Bound as `[1,2,3]`.
Vector(Vec<f64>),
```

Mettre à jour `type_name()` (~ligne 53) :

```rust
SqlValue::Vector(_) => "vector",
```

#### 1.2 `NearestNeighborClause` — `ryx-query/src/ast.rs`

Nouvelle struct (à côté de `OrderByClause`, ~ligne 234) :

```rust
/// K-nearest-neighbor ordering: `ORDER BY "col" <op> ? LIMIT ?`.
#[derive(Debug, Clone)]
pub struct NearestNeighborClause {
    pub field: Symbol,
    pub value: SqlValue,
    /// `<->` (L2), `<=>` (cosine), `<#>` (inner product).
    pub operator: DistanceOperator,
    /// K in K-NN. If set, forces `LIMIT k`.
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistanceOperator {
    L2,        // <->   Euclidean
    Cosine,    // <=>   cosine similarity
    Inner,     // <#>   dot product
}

impl DistanceOperator {
    pub fn sql(&self) -> &'static str {
        match self {
            Self::L2 => "<->",
            Self::Cosine => "<=>",
            Self::Inner => "<#>",
        }
    }
}
```

#### 1.3 `QueryNode.nearest_neighbor` — `ryx-query/src/ast.rs`

Champ sur `QueryNode` (~ligne 300) + builder (~ligne 390) :

```rust
pub nearest_neighbor: Option<NearestNeighborClause>,
```

```rust
#[must_use]
pub fn with_nearest_neighbor(mut self, nn: NearestNeighborClause) -> Self {
    self.nearest_neighbor = Some(nn);
    self
}
```

Mettre à jour `QueryNode::select()` (initialisation `None`) et **tous** les `QueryNode { .. }` literals dans le codebase (compiler, plan.rs, tests).

#### 1.4 Binding — `ryx-backend/src/backends/mod.rs`

Dans `bind_pg()` (~ligne 150) :

```rust
SqlValue::Vector(v) => {
    let s = format!("[{}]", v.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(","));
    q.bind(s.as_str())
}
```

`bind_mysql` / `bind_sqlite` : ces branches ne doivent pas exister — erreur. Le K-NN est bloqué plus haut par la validation backend, donc ici un `unreachable!()` ou un bind générique texte suffit pour l'exhaustivité.

### Phase 2 — Compilation SQL (Rust core)

#### 2.1 `compile_select()` — `ryx-query/src/compiler/compilr.rs`

Insérer le bloc ORDER BY distance **avant** le ORDER BY normal (~ligne 364) :

```rust
if let Some(nn) = &node.nearest_neighbor {
    // Validation backend : PG uniquement
    if node.backend != Backend::PostgreSQL {
        return Err(QueryError::UnsupportedBackend {
            feature: "nearest neighbor".into(),
            backend: node.backend,
        });
    }
    writer.write(" ORDER BY ");
    writer.write_qualified_symbol(nn.field);
    writer.write(" ");
    writer.write(nn.operator.sql());
    writer.write(" ?");
    values.push(nn.value.clone());

    // Le K-NN force LIMIT si nn.limit est défini
    if let Some(k) = nn.limit {
        writer.write(" LIMIT ");
        writer.write(&k.to_string());
    }
}
```

Si `nn.limit` est `Some`, il doit **écraser** `node.limit`. Sinon, c'est `node.limit` qui prime (cas `order_by_distance` + `.limit(k)`).

#### 2.2 Ordre des clauses

Si les deux ORDER BY coexistent (`.order_by("id")` + `.nearest_neighbors()`), PostgreSQL exige que le tri distance soit dans le ORDER BY. On concatène : `ORDER BY "col" <-> ?, "id"`. La clause distance **précède** les ORDER BY normaux.

#### 2.3 Compilation des autres opérations

`compile_count`, `compile_aggregate`, `compile_update`, `compile_delete` : si `nearest_neighbor` est défini → ignorer ou erreur. Pour `Count`, le plus sûr est une erreur claire.

### Phase 3 — API Python

#### 3.1 `VectorField` — `ryx-python/ryx/fields.py`

Nouvelle classe (près de `ArrayField`, ~ligne 862) :

```python
class VectorField(Field):
    """pgvector embedding field (PostgreSQL only).

    Args:
        dimensions: Vector dimensionality (e.g. 768 for OpenAI embeddings).
    """
    SUPPORTED_LOOKUPS = ["isnull"]

    def __init__(self, dimensions: int = 768, **kw):
        self.dimensions = dimensions
        super().__init__(**kw)

    def db_type(self) -> str:
        return f"vector({self.dimensions})"

    def to_python(self, v):
        if v is None:
            return None
        if isinstance(v, (list, tuple)):
            return v
        return json.loads(v) if isinstance(v, str) else list(v)

    def to_db(self, v):
        return None if v is None else json.dumps(v)

    def _build_implicit_validators(self):
        pass  # No NotNull / Blank validators for vectors
```

- Le DDL génère déjà `vector(768)` (pass-through PG).
- `to_db()` sérialise en JSON → la valeur devient un `SqlValue::List` via `py_to_sql_value()` actuellement. **Problème** : un `List` de floats passe par `SqlValue::List`, pas `Vector`. Il faut soit une conversion dédiée, soit accepter que le vecteur soit une list et convertir dans `nearest_neighbors()`.

#### 3.2 Méthodes QuerySet — `ryx-python/ryx/queryset.py`

```python
_DISTANCE_OPERATORS = {"<->": "l2", "<=>": "cosine", "<#>": "inner"}

def order_by_distance(self, field: str, vector: Sequence[float],
                      operator: str = "<->") -> "QuerySet":
    """Order rows by distance to `vector` (K-NN). Combine with `.limit(k)`."""
    self._validate_vector_backend()
    self._validate_distance_operator(operator)
    return self._with_op("order_by_distance", (field, list(vector), operator))

def nearest_neighbors(self, field: str, vector: Sequence[float],
                      k: int = 10, operator: str = "<->") -> "QuerySet":
    """Return the K rows closest to `vector`."""
    self._validate_vector_backend()
    self._validate_distance_operator(operator)
    return self._with_op("nearest_neighbor", (field, list(vector), operator, k))
```

Helpers de validation :

```python
def _validate_vector_backend(self):
    from ryx import ryx_core as _core
    backend = _core.get_backend(self._using or "default")
    if backend != "postgres":
        raise NotImplementedError(
            f"Vector K-NN requires PostgreSQL (pgvector). Got: {backend}"
        )

def _validate_distance_operator(self, operator):
    if operator not in _DISTANCE_OPERATORS:
        raise ValueError(
            f"Invalid distance operator '{operator}'. "
            f"Choose one of: {', '.join(_DISTANCE_OPERATORS)}"
        )
```

#### 3.3 Plan builder — `ryx-python/src/plan.rs`

Deux nouveaux tags (~ligne 180) :

```rust
"nearest_neighbor" => {
    let t = tuple.get_item(1)?.cast::<PyTuple>()?;
    let field: String = t.get_item(0)?.extract()?;
    let vector = py_to_sql_value(&t.get_item(1)?)?;   // Vec<f64> → Vector
    let operator: String = t.get_item(2)?.extract()?;
    let k: u64 = t.get_item(3)?.extract()?;
    let op = match operator.as_str() {
        "<->" => DistanceOperator::L2,
        "<=>" => DistanceOperator::Cosine,
        "<#>" => DistanceOperator::Inner,
        _ => return Err(pyo3::exceptions::PyValueError::new_err("invalid operator")),
    };
    node = node.with_nearest_neighbor(NearestNeighborClause {
        field: field.into(),
        value: vector,
        operator: op,
        limit: Some(k),
    });
}
"order_by_distance" => {
    // Same, but limit: None
}
```

Le `py_to_sql_value` retourne actuellement `List` pour une liste Python. Il faut une conversion dédiée `py_to_vector()` qui vérifie `Vec<f64>` et construit `SqlValue::Vector`. Sinon on réutilise `SqlValue::Vector` directement en extrayant `Vec<f64>`.

#### 3.4 Export — `ryx-python/ryx/__init__.py`

Ajouter `VectorField` à l'export (`from ryx.fields import ...`).

### Phase 4 — API Rust

#### 4.1 `QuerySet<T>` — `ryx-rs/src/queryset.rs`

```rust
/// Order rows by distance to `vector` (K-NN). Combine with `.limit(k)`.
pub fn order_by_distance(mut self, field: &str, vector: Vec<f64>,
                         operator: DistanceOperator) -> Self {
    self.node = self.node.with_nearest_neighbor(NearestNeighborClause {
        field: field.into(),
        value: SqlValue::Vector(vector),
        operator,
        limit: None,
    });
    self
}

/// Return the K rows closest to `vector`.
pub fn nearest_neighbors(mut self, field: &str, vector: Vec<f64>,
                         k: u64, operator: DistanceOperator) -> Self {
    self.node = self.node.with_nearest_neighbor(NearestNeighborClause {
        field: field.into(),
        value: SqlValue::Vector(vector),
        operator,
        limit: Some(k),
    });
    self
}
```

Exporter `DistanceOperator` dans `ryx-rs/src/lib.rs`.

#### 4.2 Exécution

Pas de changement : `fetch_raw_rows()` compile le node → le ORDER BY distance est déjà dans le SQL.

### Phase 5 — DDL & Migrations

#### 5.1 Vérification (aucun changement)

- Rust `col_type_for_backend` (`ddl.rs:434`) : `other => other` → `vector(768)` passe.
- Python `_translate_type` (`ddl.py:353`) : `return db_type` → idem.

Cependant, un **type d'index HNSW/IVFFlat** serait un vrai plus pour les K-NN à grande échelle. Ce sera un **changement ultérieur** (hors scope initial) :

```sql
CREATE INDEX ON items USING hnsw (embedding vector_cosine_ops);
```

Cela nécessiterait d'étendre le système d'index existant (`CreateIndex`) avec un paramètre `using` / `opclass`.

#### 5.2 Création de l'extension

Le runner de migration devrait offrir un moyen de créer `CREATE EXTENSION IF NOT EXISTS vector`. Via `RunSQL` dans un fichier de migration, ou une méthode dédiée. À documenter.

### Phase 6 — Tests

#### Rust — `ryx-query/src/compiler/compilr.rs` (unit)

```rust
#[test]
fn test_nearest_neighbor_l2() {
    let node = QueryNode::select("items")
        .with_nearest_neighbor(NearestNeighborClause {
            field: "embedding".into(),
            value: SqlValue::Vector(vec![1.0, 2.0, 3.0]),
            operator: DistanceOperator::L2,
            limit: Some(5),
        });
    let compiled = compile(&node).unwrap();
    assert_eq!(compiled.sql, r#"SELECT * FROM "items" ORDER BY "embedding" <-> ? LIMIT 5"#);
}

#[test]
fn test_nearest_neighbor_cosine() { /* <=> */ }

#[test]
fn test_nearest_neighbor_inner() { /* <#> */ }

#[test]
fn test_nearest_neighbor_no_limit() { /* no LIMIT — from order_by_distance */ }

#[test]
fn test_nearest_neighbor_non_pg_errors() { /* MySQL/SQLite → Err */ }
```

#### Rust — `ryx-rs/tests/` (integration, PG requis)

```rust
#[tokio::test]
async fn test_knn_end_to_end() {
    // setup pool → PG
    // create items table with vector(3) column
    // insert 3 embeddings
    // nearest_neighbors("embedding", [1,0,0], 2, L2) → returns 2 closest
}
```

#### Python — `ryx-python/tests/` (integration)

```python
# test_vector_field.py
async def test_vector_field_ddl():
    # VectorField().db_type() == "vector(768)"
    # MigrationRunner generates CREATE TABLE ... embedding vector(768)

async def test_nearest_neighbors_postgres():
    # requires PG + pgvector
    # insert rows, call .nearest_neighbors(), assert ordering

async def test_nearest_neighbors_non_pg_raises():
    # sqlite → NotImplementedError
```

#### Python — unit (pas de DB requise)

```python
# test_queryset_vector.py
def test_order_by_distance_op_tag():
    qs = Post.objects.order_by_distance("embedding", [1, 2, 3])
    assert ("order_by_distance", ("embedding", [1, 2, 3], "<->")) in qs._ops

def test_nearest_neighbors_op_tag():
    qs = Post.objects.nearest_neighbors("embedding", [1, 2, 3], k=5)
    assert ("nearest_neighbor", ("embedding", [1, 2, 3], "<->", 5)) in qs._ops

def test_invalid_operator_raises():
    with pytest.raises(ValueError):
        Post.objects.nearest_neighbors("embedding", [1, 2, 3], operator="??")
```

### Phase 7 — Documentation

- Nouvelle page `docs/doc/advanced/vector-search.mdx` (Python + Rust) :
  - Définition `VectorField(dimensions=768)`
  - Insertion : `Item.objects.create(embedding=[0.1, ...])`
  - K-NN : `.nearest_neighbors("embedding", q, k=10, operator="<=>")`
  - Tri + limite : `.order_by_distance("embedding", q).limit(10)`
  - Opérateurs : tableau `<->` L2 / `<=>` cosine / `<#>` produit scalaire
  - Prérequis : `CREATE EXTENSION IF NOT EXISTS vector`
  - Index HNSW (avancé, ultérieur)
  - Backends supportés : PostgreSQL uniquement
- Mise à jour de `README.md` (tableau de comparaison : « Vector search ✅ »)
- Mise à jour du champ `Field` docstring si nécessaire

---

## Ordre d'implémentation recommandé

1. **Phase 1** — `SqlValue::Vector`, `NearestNeighborClause`, `DistanceOperator`, champ `QueryNode` (types purs, aucun risque)
2. **Phase 2** — Compilation SQL + erreur backend (le cœur)
3. **Phase 3** — API Python (`VectorField`, `nearest_neighbors`, `order_by_distance`)
4. **Phase 4** — API Rust
5. **Phase 6** — Tests unitaires (Rust + Python) — la couverture avant l'intégration PG
6. **Phase 5** — DDL (vérification + doc extension pgvector)
7. **Phase 7** — Documentation
8. **Phase 6 suite** — Tests d'intégration PG (nécessite une DB pgvector)

---

## Fichiers touchés (récapitulatif)

| Fichier | Changement |
|---|---|
| `ryx-query/src/ast.rs` | `SqlValue::Vector`, `NearestNeighborClause`, `DistanceOperator`, champ `QueryNode` + builder |
| `ryx-query/src/compiler/compilr.rs` | Compilation ORDER BY distance + erreur backend + mise à jour literals |
| `ryx-backend/src/backends/mod.rs` | `bind_pg()` : variant `Vector` |
| `ryx-python/ryx/fields.py` | `VectorField` |
| `ryx-python/ryx/queryset.py` | `.nearest_neighbors()`, `.order_by_distance()`, validation |
| `ryx-python/src/plan.rs` | Tags `nearest_neighbor` / `order_by_distance` |
| `ryx-python/ryx/__init__.py` | Export `VectorField` |
| `ryx-rs/src/queryset.rs` | `.nearest_neighbors()`, `.order_by_distance()` |
| `ryx-rs/src/lib.rs` | Export `DistanceOperator`, `NearestNeighborClause` |
| `docs/doc/advanced/vector-search.mdx` | Nouvelle page |
| `README.md` | Tableau comparaison : vector search |

---

## Estimations

| Phase | Lignes estimées | Risque |
|---|---|---|
| 1. Types (ast.rs) | ~80 | Très faible |
| 2. Compilation | ~40 | Moyen (ordres de clauses) |
| 3. Python API | ~70 | Faible |
| 4. Rust API | ~30 | Faible |
| 5. DDL/extension | ~10 | Nul |
| 6. Tests | ~200 | Moyen (intégration PG) |
| 7. Docs | ~150 | Nul |
| **Total** | **~580** | |
