import type * as Preset from '@docusaurus/preset-classic';
import type { Config } from '@docusaurus/types';

const config: Config = {
  title: 'Ryx ORM',
  tagline: 'Django-style Python ORM. Powered by Rust.',
  favicon: 'img/favicon.ico',
  url: 'https://ryx.alldotpy.dev',
  baseUrl: '/Ryx/',
  organizationName: 'AllDotPy',
  projectName: 'Ryx',
  onBrokenLinks: 'throw',
  onBrokenMarkdownLinks: 'warn',
  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },
  presets: [
    [
      'classic',
      {
        docs: {
          path: './doc',
          sidebarPath: './sidebars.js',
          editUrl: 'https://github.com/AllDotPy/Ryx/tree/main/docs/',
          routeBasePath: '/',
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],
  themeConfig: {
    image: 'img/social-card.png',
    navbar: {
      title: 'Ryx',
      logo: { alt: 'Ryx ORM Logo', src: 'img/logo.svg' },
      items: [
        { type: 'docSidebar', sidebarId: 'tutorialSidebar', position: 'left', label: 'Docs' },
        { href: 'https://github.com/AllDotPy/Ryx', label: 'GitHub', position: 'right' },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        { title: 'Docs', items: [
          { label: 'Getting Started', to: '/getting-started/installation' },
          { label: 'Core Concepts', to: '/core-concepts/models' },
          { label: 'Querying', to: '/querying/filtering' },
          { label: 'API Reference', to: '/reference/api-reference' },
        ]},
        { title: 'Community', items: [
          { label: 'GitHub', href: 'https://github.com/AllDotPy/Ryx' },
          { label: 'Contributing', href: 'https://github.com/AllDotPy/Ryx/blob/main/CONTRIBUTING.md' },
        ]},
        { title: 'More', items: [
          { label: 'License (Python)', href: 'https://github.com/AllDotPy/Ryx/blob/main/LICENSE' },
        ]},
      ],
      copyright: `Ryx ORM — Python: AGPL-3.0 · Rust: MIT OR Apache-2.0`,
    },
    prism: {
      theme: require('prism-react-renderer').themes.github,
      darkTheme: require('prism-react-renderer').themes.dracula,
      additionalLanguages: ['python', 'rust', 'bash', 'sql', 'toml'],
    },
    colorMode: { defaultMode: 'dark', respectPrefersColorScheme: true },
  } satisfies Preset.ThemeConfig,
};

export default config;
