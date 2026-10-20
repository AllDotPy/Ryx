import React, { useState, useRef, useEffect } from 'react';
import { useHistory } from '@docusaurus/router';
import './styles.css';

export default function SearchBar() {
  const [query, setQuery] = useState('');
  const [isOpen, setIsOpen] = useState(false);
  const [results, setResults] = useState([]);
  const inputRef = useRef(null);
  const history = useHistory();

  const allPages = [
    { title: 'Introduction', url: '/', section: 'Home' },
    { title: 'Installation', url: '/getting-started/installation', section: 'Getting Started' },
    { title: 'Quick Start', url: '/getting-started/quick-start', section: 'Getting Started' },
    { title: 'Project Structure', url: '/getting-started/project-structure', section: 'Getting Started' },
    { title: 'Models', url: '/core-concepts/models', section: 'Core Concepts' },
    { title: 'Managers & QuerySets', url: '/core-concepts/managers-and-querysets', section: 'Core Concepts' },
    { title: 'Fields', url: '/core-concepts/fields', section: 'Core Concepts' },
    { title: 'Migrations', url: '/core-concepts/migrations', section: 'Core Concepts' },
    { title: 'Filtering', url: '/querying/filtering', section: 'Querying' },
    { title: 'Q Objects', url: '/querying/q-objects', section: 'Querying' },
    { title: 'Ordering & Pagination', url: '/querying/ordering-and-pagination', section: 'Querying' },
    { title: 'Aggregations', url: '/querying/aggregations', section: 'Querying' },
    { title: 'Values & Annotate', url: '/querying/values-and-annotate', section: 'Querying' },
    { title: 'ForeignKey', url: '/relationships/foreign-key', section: 'Relationships' },
    { title: 'OneToOne', url: '/relationships/one-to-one', section: 'Relationships' },
    { title: 'ManyToMany', url: '/relationships/many-to-many', section: 'Relationships' },
    { title: 'select_related', url: '/relationships/select-related', section: 'Relationships' },
    { title: 'prefetch_related', url: '/relationships/prefetch-related', section: 'Relationships' },
    { title: 'Creating Records', url: '/crud/creating', section: 'CRUD' },
    { title: 'Reading Records', url: '/crud/reading', section: 'CRUD' },
    { title: 'Updating Records', url: '/crud/updating', section: 'CRUD' },
    { title: 'Deleting Records', url: '/crud/deleting', section: 'CRUD' },
    { title: 'Bulk Operations', url: '/crud/bulk-operations', section: 'CRUD' },
    { title: 'Transactions', url: '/advanced/transactions', section: 'Advanced' },
    { title: 'Validation', url: '/advanced/validation', section: 'Advanced' },
    { title: 'Signals', url: '/advanced/signals', section: 'Advanced' },
    { title: 'Hooks', url: '/advanced/hooks', section: 'Advanced' },
    { title: 'Caching', url: '/advanced/caching', section: 'Advanced' },
    { title: 'Custom Lookups', url: '/advanced/custom-lookups', section: 'Advanced' },
    { title: 'Sync/Async', url: '/advanced/sync-async', section: 'Advanced' },
    { title: 'Raw SQL', url: '/advanced/raw-sql', section: 'Advanced' },
    { title: 'CLI', url: '/advanced/cli', section: 'Advanced' },
    { title: 'API Reference', url: '/reference/api-reference', section: 'Reference' },
    { title: 'Field Reference', url: '/reference/field-reference', section: 'Reference' },
    { title: 'Lookup Reference', url: '/reference/lookup-reference', section: 'Reference' },
    { title: 'Exceptions', url: '/reference/exceptions', section: 'Reference' },
    { title: 'Signals Reference', url: '/reference/signals-reference', section: 'Reference' },
    { title: 'Architecture', url: '/internals/architecture', section: 'Internals' },
    { title: 'Rust Core', url: '/internals/rust-core', section: 'Internals' },
    { title: 'Query Compiler', url: '/internals/query-compiler', section: 'Internals' },
    { title: 'Connection Pool', url: '/internals/connection-pool', section: 'Internals' },
    { title: 'Type Conversion', url: '/internals/type-conversion', section: 'Internals' },
    { title: 'Blog Tutorial', url: '/cookbook/blog-tutorial', section: 'Cookbook' },
    { title: 'Testing', url: '/cookbook/testing', section: 'Cookbook' },
    { title: 'Deployment', url: '/cookbook/deployment', section: 'Cookbook' },
  ];

  useEffect(() => {
    const handleKeyDown = (e) => {
      if (e.key  '/' && !['INPUT', 'TEXTAREA'].includes(document.activeElement.tagName)) {
        e.preventDefault();
        inputRef.current?.focus();
      }
      if (e.key  'Escape') {
        setIsOpen(false);
        inputRef.current?.blur();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  useEffect(() => {
    if (query.trim().length < 2) {
      setResults([]);
      return;
    }
    const q = query.toLowerCase();
    const filtered = allPages.filter(
      (p) => p.title.toLowerCase().includes(q) || p.section.toLowerCase().includes(q)
    );
    setResults(filtered);
    setIsOpen(true);
  }, [query]);

  const handleSelect = (url) => {
    history.push(url);
    setIsOpen(false);
    setQuery('');
    inputRef.current?.blur();
  };

  return (
    <div className="ryx-search" data-component="search-bar">
      <svg
        className="ryx-search__icon"
        viewBox="0 0 24 24"
        width="18"
        height="18"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <circle cx="11" cy="11" r="8" />
        <path d="M21 21l-4.35-4.35" />
      </svg>
      <input
        ref={inputRef}
        type="text"
        placeholder="Search docs..."
        className="ryx-search__input"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onFocus={() => query.length >= 2 && setIsOpen(true)}
        onBlur={() => setTimeout(() => setIsOpen(false), 200)}
      />
      <kbd className="ryx-search__shortcut">/</kbd>

      {isOpen && results.length > 0 && (
        <div className="ryx-search__dropdown">
          {results.slice(0, 8).map((page, i) => (
            <button
              key={i}
              className="ryx-search__result"
              onMouseDown={() => handleSelect(page.url)}
            >
              <span className="ryx-search__result-title">{page.title}</span>
              <span className="ryx-search__result-section">{page.section}</span>
            </button>
          ))}
        </div>
      )}

      {isOpen && query.length >= 2 && results.length  0 && (
        <div className="ryx-search__dropdown">
          <div className="ryx-search__no-results">No results for "{query}"</div>
        </div>
      )}
    </div>
  );
}
