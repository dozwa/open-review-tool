# Stufe 1: Oberfläche bauen
FROM node:22-slim AS frontend
WORKDIR /src
COPY frontend/package.json frontend/package-lock.json* ./
RUN npm install --no-audit --no-fund
COPY frontend/ ./
RUN npm run build

# Stufe 2: Anwendung
FROM python:3.13-slim
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1
RUN useradd --system --create-home --uid 10001 ort
WORKDIR /app
COPY backend/requirements.txt ./
RUN pip install --no-cache-dir -r requirements.txt
COPY backend/ ./
COPY --from=frontend /src/dist ./frontend_dist
RUN DJANGO_SECRET_KEY=build-only python manage.py collectstatic --noinput
USER ort
EXPOSE 8000
CMD ["gunicorn", "ort.wsgi", "--bind", "0.0.0.0:8000", "--workers", "3", "--timeout", "120"]
