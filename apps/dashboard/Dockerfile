# Development container for the minimal operator dashboard (Vite dev server).
FROM node:20-alpine
WORKDIR /app
COPY package.json ./
RUN npm install
COPY . .
ENV VITE_API_TARGET=http://api:8088
EXPOSE 5173
CMD ["npm", "run", "dev", "--", "--host", "0.0.0.0"]
