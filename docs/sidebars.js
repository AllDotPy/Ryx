const sidebars = {
  tutorialSidebar: [
    'intro',
    {
      type: 'category',
      label: 'Getting Started',
      link: { type: 'doc', id: 'getting-started/index' },
      items: [
        'getting-started/installation',
        'getting-started/quick-start',
        'getting-started/project-structure',
      ],
    },
    {
      type: 'category',
      label: 'Core Concepts',
      link: { type: 'doc', id: 'core-concepts/index' },
      items: [
        'core-concepts/models',
        'core-concepts/managers-and-querysets',
        'core-concepts/fields',
        'core-concepts/migrations',
      ],
    },
    {
      type: 'category',
      label: 'Querying',
      link: { type: 'doc', id: 'querying/index' },
      items: [
        'querying/filtering',
        'querying/q-objects',
        'querying/ordering-and-pagination',
        'querying/aggregations',
        'querying/values-and-annotate',
      ],
    },
    {
      type: 'category',
      label: 'Relationships',
      link: { type: 'doc', id: 'relationships/index' },
      items: [
        'relationships/foreign-key',
        'relationships/one-to-one',
        'relationships/many-to-many',
        'relationships/select-related',
        'relationships/prefetch-related',
      ],
    },
    {
      type: 'category',
      label: 'CRUD',
      link: { type: 'doc', id: 'crud/index' },
      items: [
        'crud/creating',
        'crud/reading',
        'crud/updating',
        'crud/deleting',
        'crud/bulk-operations',
      ],
    },
    {
      type: 'category',
      label: 'Advanced',
      link: { type: 'doc', id: 'advanced/index' },
      items: [
        'advanced/transactions',
        'advanced/validation',
        'advanced/signals',
        'advanced/hooks',
        'advanced/caching',
        'advanced/custom-lookups',
        'advanced/sync-async',
        'advanced/multi-db',
        'advanced/raw-sql',
        'advanced/cli',
      ],
    },
    {
      type: 'category',
      label: 'Reference',
      link: { type: 'doc', id: 'reference/index' },
      items: [
        'reference/api-reference',
        'reference/field-reference',
        'reference/lookup-reference',
        'reference/exceptions',
        'reference/signals-reference',
      ],
    },
    {
      type: 'category',
      label: 'Internals',
      link: { type: 'doc', id: 'internals/index' },
      items: [
        'internals/architecture',
        'internals/rust-core',
        'internals/query-compiler',
        'internals/connection-pool',
        'internals/type-conversion',
      ],
    },
    {
      type: 'category',
      label: 'Cookbook',
      link: { type: 'doc', id: 'cookbook/index' },
      items: [
        'cookbook/blog-tutorial',
        'cookbook/testing',
        'cookbook/deployment',
      ],
    },
  ],
};

module.exports = sidebars;
