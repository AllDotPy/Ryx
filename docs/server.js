import express from 'express';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const app = express();
const PORT = 2026;

// Middleware pour les headers SEO et de sécurité
app.use((req, res, next) => {
  // Sécurité
  res.setHeader('X-Content-Type-Options', 'nosniff');
  res.setHeader('X-Frame-Options', 'SAMEORIGIN');
  res.setHeader('X-XSS-Protection', '1; mode=block');
  res.setHeader('Referrer-Policy', 'strict-origin-when-cross-origin');
  
  // Performance
  res.setHeader('Cache-Control', 'public, max-age=3600');
  
  next();
});

// Servir les fichiers statiques du dossier public
app.use(express.static(join(__dirname, 'public'), {
  maxAge: '1d',
  etag: false
}));

// Servir les fichiers statiques du dossier dist avec cache agressif
app.use(express.static(join(__dirname, 'dist'), {
  maxAge: '365d',
  etag: false
}));

// Routes SEO
app.get('/robots.txt', (req, res) => {
  res.type('text/plain');
  res.sendFile(join(__dirname, 'public', 'robots.txt'));
});

app.get('/sitemap.xml', (req, res) => {
  res.type('application/xml');
  res.sendFile(join(__dirname, 'public', 'sitemap.xml'));
});

app.get('/manifest.json', (req, res) => {
  res.type('application/manifest+json');
  res.sendFile(join(__dirname, 'public', 'manifest.json'));
});

// Health check
app.get('/health', (req, res) => {
  res.setHeader('Cache-Control', 'no-cache');
  res.json({ status: 'ok', timestamp: new Date().toISOString() });
});

// Rediriger toutes les autres routes vers index.html (SPA routing)
app.get('*', (req, res) => {
  res.setHeader('Cache-Control', 'no-cache, no-store, must-revalidate');
  res.sendFile(join(__dirname, 'dist', 'index.html'));
});

app.listen(PORT, () => {
  console.log(`🚀 Serveur lancé sur http://localhost:${PORT}`);
  console.log(`📍 Robots: http://localhost:${PORT}/robots.txt`);
  console.log(`📍 Sitemap: http://localhost:${PORT}/sitemap.xml`);
  console.log(`📍 Manifest: http://localhost:${PORT}/manifest.json`);
});