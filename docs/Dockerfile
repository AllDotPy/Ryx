# Étape 1 : Construction (Build)
FROM node:20-alpine AS build

WORKDIR /app

COPY package*.json ./

# Installation propre des dépendances (y compris devDependencies nécessaires pour le build)
RUN npm install

# Copie de l'intégralité du code source
COPY . .

# Construction de l'application de production (génère le dossier /dist)
RUN npm run build

# Étape 2 : Serveur de Production
FROM node:20-alpine

WORKDIR /app

# Copie seulement le package.json et lock file
COPY package*.json ./

# Installation de seulement les dépendances de production
RUN npm install

# Copie du serveur et des fichiers buildés
COPY server.js .
COPY --from=build /app/dist ./dist

# Port sur lequel le serveur écoute
EXPOSE 2026

# Lancer le serveur
CMD ["node", "server.js"]