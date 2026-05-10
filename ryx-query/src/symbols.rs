use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::RwLock;

/// A unique identifier for a string (table name, column name, etc.)
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct Symbol(pub u32);

/// Global interner for SQL identifiers.
pub struct Interner {
    map: RwLock<HashMap<String, Symbol>>,
    vec: RwLock<Vec<String>>,
}

impl Default for Interner {
    fn default() -> Self {
        Self::new()
    }
}

impl Interner {
    pub fn new() -> Self {
        Self {
            map: RwLock::new(HashMap::new()),
            vec: RwLock::new(Vec::new()),
        }
    }

    pub fn intern(&self, s: &str) -> Symbol {
        // Fast path: read lock
        {
            let map = self.map.read().unwrap();
            if let Some(&sym) = map.get(s) {
                return sym;
            }
        }

        // Slow path: write lock
        let mut map = self.map.write().unwrap();
        let mut vec = self.vec.write().unwrap();

        // Double check to avoid race condition
        if let Some(&sym) = map.get(s) {
            return sym;
        }

        let sym = Symbol(vec.len() as u32);
        vec.push(s.to_string());
        map.insert(s.to_string(), sym);
        sym
    }

    pub fn resolve(&self, sym: Symbol) -> String {
        self.vec.read().unwrap()[sym.0 as usize].clone()
    }
}

pub static GLOBAL_INTERNER: Lazy<Interner> = Lazy::new(Interner::new);

impl From<&str> for Symbol {
    fn from(s: &str) -> Self {
        GLOBAL_INTERNER.intern(s)
    }
}

impl From<String> for Symbol {
    fn from(s: String) -> Self {
        GLOBAL_INTERNER.intern(&s)
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&GLOBAL_INTERNER.resolve(*self))
    }
}
