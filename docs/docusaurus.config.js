// @ts-check
import { themes as prismThemes } from 'prism-react-renderer';

/** @type {import('@docusaurus/types').Config} */
const config = {
  title: 'Ryx ORM',
  tagline: 'Django-style Python ORM. Powered by Rust.',
  favicon: 'img/favicon.ico',

  url: 'https://ryx.alldotpy.com',
  baseUrl: '/',

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
      /** @type {import('@docusaurus/preset-classic').Options} */
      ({
        docs: {
          path: './doc',
          routeBasePath: '/',
          sidebarPath: './sidebars.js',
          editUrl: 'https://github.com/AllDotPy/Ryx/tree/main/docs/doc/',
          showLastUpdateTime: true,
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      }),
    ],
  ],

  themeConfig:
    /** @type {import('@docusaurus/preset-classic').ThemeConfig} */
    ({
      image: 'img/social-card.png',
      navbar: {
        title: 'Ryx',
        logo: {
          alt: 'Ryx ORM Logo',
          src: 'img/logo.svg',
          width: 32,
          height: 32,
        },
        items: [
          {
            type: 'docSidebar',
            sidebarId: 'tutorialSidebar',
            position: 'left',
            label: 'Documentation',
          },
          {
            type: 'custom-search-bar',
            position: 'right',
          },
          {
            type: 'custom-github-stats',
            position: 'right',
          },
          {
            href: 'https://github.com/AllDotPy/Ryx/blob/main/CONTRIBUTING.md',
            position: 'right',
            label: 'Contributing',
          },
        ],
      },
      footer: undefined,
      prism: {
        theme: prismThemes.oneDark,
        darkTheme: prismThemes.oneDark,
        additionalLanguages: ['python', 'rust', 'bash', 'sql', 'toml'],
        magicComments: [
          {
            className: 'theme-code-block-highlighted-line',
            line: 'highlight-next-line',
            block: { start: 'highlight-start', end: 'highlight-end' },
          },
        ],
      },
      colorMode: {
        defaultMode: 'dark',
        respectPrefersColorScheme: true,
        disableSwitch: false,
      },
      docs: {
        sidebar: {
          hideable: true,
          autoCollapseCategories: true,
        },
      },
      algolia: undefined,
    }),

  markdown: {
    mermaid: true,
  },
  themes: ['@docusaurus/theme-mermaid'],
};

export default config;
